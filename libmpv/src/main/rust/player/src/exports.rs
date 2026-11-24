//! The `native*` methods of `dev.jdtech.mpv.MPVLib` (the newer API's, those of
//! `dev.jdtech.mpv.internal.Native`, are in `api`).
//!
//! Every function runs its body through `entry`, which turns a panic into a
//! `RuntimeException` instead of letting it unwind into the JVM, and a
//! `Failure` into the Java exception it describes. When an exception is
//! thrown, the return value is ignored by Java.
//!
//! Handles: an unknown or destroyed handle throws `IllegalStateException`,
//! except in `nativeDestroy` (no-op, like a second `destroy()`) and
//! `nativeCommand` (returns `MPV_ERROR_UNINITIALIZED`, the C++ wrapper's code
//! for "mpv is gone").

use std::any::Any;
use std::ffi::{CString, c_char, c_int, c_void};
use std::panic::{self, AssertUnwindSafe};
use std::ptr;

use jni_sys::{JNIEnv, jboolean, jdouble, jint, jlong, jobject, jobjectArray, jstring};

use crate::ffi::*;
use crate::instance::{self, Boxed, Failure, Instance, RUNTIME, Result, error_string};
use crate::jni::Env;
use crate::log;

fn panic_message(payload: &(dyn Any + Send)) -> &str {
    if let Some(s) = payload.downcast_ref::<&str>() {
        s
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s
    } else {
        "(no message)"
    }
}

pub fn entry<R>(raw: *mut JNIEnv, on_error: R, body: impl FnOnce(&Env) -> Result<R>) -> R {
    // SAFETY: the VM passes the calling thread's env to every native method
    let env = unsafe { Env::from_raw(raw) };
    match panic::catch_unwind(AssertUnwindSafe(|| body(&env))) {
        Ok(Ok(value)) => value,
        Ok(Err(Failure::Pending)) => on_error,
        Ok(Err(Failure::Throw(class, message))) => {
            if !env.exception_pending() {
                env.throw(class, &message);
            }
            on_error
        }
        Ok(Err(Failure::Mpv { error, operation })) => {
            if !env.exception_pending() {
                instance::throw_mpv(&env, error, &operation);
            }
            on_error
        }
        Err(payload) => {
            let message = format!("libplayer: internal error: {}", panic_message(&*payload));
            log::error(&message);
            if !env.exception_pending() {
                env.throw(RUNTIME, &message);
            }
            on_error
        }
    }
}

pub fn instance(handle: jlong) -> Result<std::sync::Arc<Instance>> {
    instance::lookup(handle).ok_or_else(Failure::destroyed)
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_jdtech_mpv_MPVLib_nativeCreate(
    env: *mut JNIEnv,
    _obj: jobject,
    thiz: jobject,
    appctx: jobject,
) -> jlong {
    entry(env, 0, |env| instance::create(env, thiz, appctx))
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_jdtech_mpv_MPVLib_nativeInit(
    env: *mut JNIEnv,
    _obj: jobject,
    handle: jlong,
) {
    entry(env, (), |_| instance(handle)?.init())
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_jdtech_mpv_MPVLib_nativeDestroy(
    env: *mut JNIEnv,
    _obj: jobject,
    handle: jlong,
) {
    entry(env, (), |_| instance::destroy(handle))
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_jdtech_mpv_MPVLib_nativeAttachSurface(
    env: *mut JNIEnv,
    _obj: jobject,
    handle: jlong,
    surface: jobject,
) {
    entry(env, (), |env| {
        instance(handle)?.attach_surface(env, surface)
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_jdtech_mpv_MPVLib_nativeDetachSurface(
    env: *mut JNIEnv,
    _obj: jobject,
    handle: jlong,
) {
    entry(env, (), |_| instance(handle)?.detach_surface())
}

/// The C++ wrapper's argument limit (a fixed array of 128 with a terminating
/// null); kept so the same commands are refused.
const MAX_COMMAND_ARGS: jint = 127;

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_jdtech_mpv_MPVLib_nativeCommand(
    env: *mut JNIEnv,
    _obj: jobject,
    handle: jlong,
    array: jobjectArray,
) -> jint {
    entry(env, MPV_ERROR_INVALID_PARAMETER, |env| {
        let Some(instance) = instance::lookup(handle) else {
            log::error("Cannot run command: mpv is not initialized");
            return Ok(MPV_ERROR_UNINITIALIZED);
        };
        let Ok(active) = instance.enter() else {
            log::error("Cannot run command: mpv is not initialized");
            return Ok(MPV_ERROR_UNINITIALIZED);
        };

        let len = if array.is_null() {
            0
        } else {
            env.array_length(array)?
        };
        if array.is_null() || len > MAX_COMMAND_ARGS {
            log::error("Cannot run command: no or too many arguments");
            return Ok(MPV_ERROR_INVALID_PARAMETER);
        }

        let mut args: Vec<CString> = Vec::with_capacity(len as usize);
        for i in 0..len {
            let Some(element) = env.array_element(array, i)? else {
                log::error(&format!("Cannot run command: argument {i} is null"));
                return Ok(MPV_ERROR_INVALID_PARAMETER);
            };
            match env.c_string(element.as_raw()) {
                Ok(arg) => args.push(arg),
                // an OutOfMemoryError is pending and will be thrown on return
                Err(_) => return Ok(MPV_ERROR_NOMEM),
            }
        }
        let mut argv: Vec<*const c_char> = args.iter().map(|a| a.as_ptr()).collect();
        argv.push(ptr::null());
        // SAFETY: an admitted call; argv is a null-terminated array of C
        // strings that outlive the call
        Ok(unsafe { mpv_command(active.mpv(), argv.as_mut_ptr()) })
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_jdtech_mpv_MPVLib_nativeRequestLogMessages(
    env: *mut JNIEnv,
    _obj: jobject,
    handle: jlong,
    level: jstring,
) -> jint {
    entry(env, MPV_ERROR_INVALID_PARAMETER, |env| {
        let instance = instance(handle)?;
        let active = instance.enter()?;
        let level = env.c_string(level)?;
        // SAFETY: an admitted call and a NUL-terminated string
        Ok(unsafe { mpv_request_log_messages(active.mpv(), level.as_ptr()) })
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_jdtech_mpv_MPVLib_nativeSetOptionString(
    env: *mut JNIEnv,
    _obj: jobject,
    handle: jlong,
    option: jstring,
    value: jstring,
) -> jint {
    entry(env, MPV_ERROR_INVALID_PARAMETER, |env| {
        let instance = instance(handle)?;
        let active = instance.enter()?;
        let option = env.c_string(option)?;
        let value = env.c_string(value)?;
        // SAFETY: an admitted call and NUL-terminated strings
        Ok(unsafe { mpv_set_option_string(active.mpv(), option.as_ptr(), value.as_ptr()) })
    })
}

/// `mpv_get_property` into `out`; `Ok(false)` (logged) if mpv returned an error.
fn get_property<T>(
    env: &Env,
    handle: jlong,
    property: jstring,
    format: mpv_format,
    out: &mut T,
) -> Result<bool> {
    let instance = instance(handle)?;
    let active = instance.enter()?;
    let name = env.c_string(property)?;
    // SAFETY: an admitted call; `out` has the type `format` names
    let r = unsafe {
        mpv_get_property(
            active.mpv(),
            name.as_ptr(),
            format,
            ptr::from_mut(out).cast::<c_void>(),
        )
    };
    if r < 0 {
        log::error(&format!(
            "mpv_get_property({}) format {format} returned error {}",
            name.to_string_lossy(),
            error_string(r)
        ));
    }
    Ok(r >= 0)
}

/// `mpv_set_property`; an error is only logged, as before.
fn set_property<T>(
    env: &Env,
    handle: jlong,
    property: jstring,
    format: mpv_format,
    mut value: T,
) -> Result<()> {
    let instance = instance(handle)?;
    let active = instance.enter()?;
    let name = env.c_string(property)?;
    // SAFETY: an admitted call; `value` has the type `format` names
    let r = unsafe {
        mpv_set_property(
            active.mpv(),
            name.as_ptr(),
            format,
            ptr::from_mut(&mut value).cast::<c_void>(),
        )
    };
    if r < 0 {
        log::error(&format!(
            "mpv_set_property({}) format {format} returned error {}",
            name.to_string_lossy(),
            error_string(r)
        ));
    }
    Ok(())
}

fn get_boxed<T: Default>(
    env: *mut JNIEnv,
    handle: jlong,
    property: jstring,
    format: mpv_format,
    to_box: impl FnOnce(T) -> Boxed,
) -> jobject {
    entry(env, ptr::null_mut(), |env| {
        let mut value = T::default();
        if !get_property(env, handle, property, format, &mut value)? {
            return Ok(ptr::null_mut());
        }
        Ok(instance::box_value(env, to_box(value))?)
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_jdtech_mpv_MPVLib_nativeGetPropertyInt(
    env: *mut JNIEnv,
    _obj: jobject,
    handle: jlong,
    property: jstring,
) -> jobject {
    // truncated to 32 bits, as it always was; getPropertyLong has the full value
    get_boxed(env, handle, property, MPV_FORMAT_INT64, |v: i64| {
        Boxed::Int(v as i32)
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_jdtech_mpv_MPVLib_nativeGetPropertyLong(
    env: *mut JNIEnv,
    _obj: jobject,
    handle: jlong,
    property: jstring,
) -> jobject {
    get_boxed(env, handle, property, MPV_FORMAT_INT64, Boxed::Long)
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_jdtech_mpv_MPVLib_nativeGetPropertyDouble(
    env: *mut JNIEnv,
    _obj: jobject,
    handle: jlong,
    property: jstring,
) -> jobject {
    get_boxed(env, handle, property, MPV_FORMAT_DOUBLE, Boxed::Double)
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_jdtech_mpv_MPVLib_nativeGetPropertyBoolean(
    env: *mut JNIEnv,
    _obj: jobject,
    handle: jlong,
    property: jstring,
) -> jobject {
    get_boxed(env, handle, property, MPV_FORMAT_FLAG, |v: c_int| {
        Boxed::Boolean(v != 0)
    })
}

/// A string mpv allocated, freed with `mpv_free` when dropped.
struct MpvString(*mut c_char);

impl Drop for MpvString {
    fn drop(&mut self) {
        // SAFETY: mpv allocated it (or it is null, which mpv_free accepts)
        unsafe { mpv_free(self.0.cast()) };
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_jdtech_mpv_MPVLib_nativeGetPropertyString(
    env: *mut JNIEnv,
    _obj: jobject,
    handle: jlong,
    property: jstring,
) -> jstring {
    entry(env, ptr::null_mut(), |env| {
        let mut value: *mut c_char = ptr::null_mut();
        if !get_property(env, handle, property, MPV_FORMAT_STRING, &mut value)? {
            return Ok(ptr::null_mut());
        }
        let value = MpvString(value);
        if value.0.is_null() {
            return Ok(ptr::null_mut());
        }
        // SAFETY: mpv returned a NUL-terminated string, alive until `value` drops
        let s = unsafe { std::ffi::CStr::from_ptr(value.0) };
        Ok(env.new_string(s)?.into_raw())
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_jdtech_mpv_MPVLib_nativeSetPropertyInt(
    env: *mut JNIEnv,
    _obj: jobject,
    handle: jlong,
    property: jstring,
    value: jint,
) {
    entry(env, (), |env| {
        set_property(env, handle, property, MPV_FORMAT_INT64, i64::from(value))
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_jdtech_mpv_MPVLib_nativeSetPropertyDouble(
    env: *mut JNIEnv,
    _obj: jobject,
    handle: jlong,
    property: jstring,
    value: jdouble,
) {
    entry(env, (), |env| {
        set_property(env, handle, property, MPV_FORMAT_DOUBLE, value)
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_jdtech_mpv_MPVLib_nativeSetPropertyBoolean(
    env: *mut JNIEnv,
    _obj: jobject,
    handle: jlong,
    property: jstring,
    value: jboolean,
) {
    entry(env, (), |env| {
        set_property(env, handle, property, MPV_FORMAT_FLAG, c_int::from(value))
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_jdtech_mpv_MPVLib_nativeSetPropertyString(
    env: *mut JNIEnv,
    _obj: jobject,
    handle: jlong,
    property: jstring,
    value: jstring,
) {
    entry(env, (), |env| {
        let instance = instance(handle)?;
        let active = instance.enter()?;
        let name = env.c_string(property)?;
        let value = env.c_string(value)?;
        let mut data: *const c_char = value.as_ptr();
        // SAFETY: an admitted call; MPV_FORMAT_STRING takes a char**
        let r = unsafe {
            mpv_set_property(
                active.mpv(),
                name.as_ptr(),
                MPV_FORMAT_STRING,
                (&raw mut data).cast::<c_void>(),
            )
        };
        if r < 0 {
            log::error(&format!(
                "mpv_set_property({}) format {MPV_FORMAT_STRING} returned error {}",
                name.to_string_lossy(),
                error_string(r)
            ));
        }
        Ok(())
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_jdtech_mpv_MPVLib_nativeObserveProperty(
    env: *mut JNIEnv,
    _obj: jobject,
    handle: jlong,
    property: jstring,
    format: jint,
) {
    entry(env, (), |env| {
        let instance = instance(handle)?;
        let name = env.c_string(property)?;
        instance.observe_legacy(&name, format)
    })
}
