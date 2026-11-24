//! A thin layer over the raw JNI function table: only the calls this library
//! makes, with local and global references owned by values that delete them
//! on drop, and strings converted by `crate::text`.

use std::ffi::{CStr, CString, c_void};
use std::marker::PhantomData;
use std::ptr;

use jni_sys::*;

use crate::text;

/// A Java exception is pending on the current thread: return to Java without
/// further JNI calls (other than the exception functions).
#[derive(Debug)]
pub struct Pending;

pub type Result<T> = std::result::Result<T, Pending>;

/// The `JNIEnv` of the current thread. Not `Send`: it is only valid on the
/// thread it was obtained on.
pub struct Env {
    raw: *mut JNIEnv,
    _not_send: PhantomData<*mut ()>,
}

macro_rules! jni {
    ($env:expr, $name:ident $(, $arg:expr)* $(,)?) => {{
        let raw = $env.raw;
        // SAFETY: `raw` is a valid JNIEnv for this thread (an invariant of
        // Env); JNI_VERSION_1_2 functions exist on every Android version.
        unsafe { ((**raw).v1_2.$name)(raw $(, $arg)*) }
    }};
}

impl Env {
    /// # Safety
    /// `raw` must be the `JNIEnv` of the current thread and outlive the `Env`.
    pub unsafe fn from_raw(raw: *mut JNIEnv) -> Env {
        Env {
            raw,
            _not_send: PhantomData,
        }
    }

    pub fn exception_pending(&self) -> bool {
        jni!(self, ExceptionCheck)
    }

    /// Logs (through the VM, which on Android goes to logcat) and clears a
    /// pending exception, so the thread can keep making JNI calls.
    pub fn describe_and_clear_exception(&self) {
        if self.exception_pending() {
            jni!(self, ExceptionDescribe);
            jni!(self, ExceptionClear);
        }
    }

    /// Throws a new `class` with `message`. Always ends with an exception
    /// pending: if the class cannot be found, the resulting error is.
    pub fn throw(&self, class: &CStr, message: &str) -> Pending {
        // ThrowNew takes Modified UTF-8; keep the message ASCII
        let message: Vec<u8> = message
            .bytes()
            .map(|b| if b.is_ascii() && b != 0 { b } else { b'?' })
            .collect();
        let message = CString::new(message).unwrap_or_default();
        if let Ok(cls) = self.find_class(class) {
            jni!(self, ThrowNew, cls.raw, message.as_ptr());
        }
        Pending
    }

    /// Fails with an exception pending if one already is or the call raised one.
    fn check<T>(&self, value: T, failed: bool) -> Result<T> {
        if failed || self.exception_pending() {
            Err(Pending)
        } else {
            Ok(value)
        }
    }

    pub fn find_class(&self, name: &CStr) -> Result<Local<'_>> {
        let cls = jni!(self, FindClass, name.as_ptr());
        let cls = self.local(cls);
        self.check((), cls.is_none())?;
        cls.ok_or(Pending)
    }

    pub fn method_id(&self, cls: &Local<'_>, name: &CStr, sig: &CStr) -> Result<MethodId> {
        let id = jni!(self, GetMethodID, cls.raw, name.as_ptr(), sig.as_ptr());
        self.check(MethodId(id), id.is_null())
    }

    pub fn static_method_id(&self, cls: &Local<'_>, name: &CStr, sig: &CStr) -> Result<MethodId> {
        let id = jni!(
            self,
            GetStaticMethodID,
            cls.raw,
            name.as_ptr(),
            sig.as_ptr()
        );
        self.check(MethodId(id), id.is_null())
    }

    /// Takes ownership of a local reference (`None` for null).
    fn local(&self, raw: jobject) -> Option<Local<'_>> {
        (!raw.is_null()).then_some(Local { env: self, raw })
    }

    pub fn vm(&self) -> Result<Vm> {
        let mut vm: *mut JavaVM = ptr::null_mut();
        let r = jni!(self, GetJavaVM, &mut vm);
        self.check(Vm(vm), r != JNI_OK || vm.is_null())
    }

    /// A new global reference to `obj`, or `None` if `obj` is null (or the VM
    /// is out of memory, then with an exception pending).
    pub fn new_global(&self, vm: Vm, obj: jobject) -> Option<Global> {
        if obj.is_null() {
            return None;
        }
        let raw = jni!(self, NewGlobalRef, obj);
        (!raw.is_null()).then_some(Global { vm, raw })
    }

    /// Like `new_global`, but the reference is never deleted: for objects that
    /// another library keeps for the life of the process.
    pub fn new_global_leaked(&self, obj: jobject) -> jobject {
        if obj.is_null() {
            return ptr::null_mut();
        }
        jni!(self, NewGlobalRef, obj)
    }

    /// The contents of a Java string as a C string for mpv. A null `s` throws
    /// `NullPointerException`, as the C++ wrapper did.
    pub fn c_string(&self, s: jstring) -> Result<CString> {
        if s.is_null() {
            return Err(self.throw(c"java/lang/NullPointerException", "string argument is null"));
        }
        let len = jni!(self, GetStringLength, s);
        self.check((), len < 0)?;
        let mut units = vec![0u16; len as usize];
        jni!(self, GetStringRegion, s, 0, len, units.as_mut_ptr());
        self.check((), false)?;
        Ok(text::utf16_to_c_string(&units))
    }

    /// A Java string from mpv's bytes, with invalid UTF-8 replaced (see
    /// `text::utf8_to_utf16`).
    pub fn new_string(&self, utf8: &CStr) -> Result<Local<'_>> {
        let units = text::utf8_to_utf16(utf8.to_bytes());
        // never hand NewString a null pointer, even for an empty string
        let empty = [0u16; 1];
        let data = if units.is_empty() {
            empty.as_ptr()
        } else {
            units.as_ptr()
        };
        let len = jsize::try_from(units.len())
            .map_err(|_| self.throw(c"java/lang/OutOfMemoryError", "string too long for Java"))?;
        let s = jni!(self, NewString, data, len);
        let s = self.local(s);
        self.check((), s.is_none())?;
        s.ok_or(Pending)
    }

    pub fn array_length(&self, array: jobjectArray) -> Result<jsize> {
        let len = jni!(self, GetArrayLength, array);
        self.check(len, false)
    }

    /// Element `i`, `None` if it is null.
    pub fn array_element(&self, array: jobjectArray, i: jsize) -> Result<Option<Local<'_>>> {
        let e = jni!(self, GetObjectArrayElement, array, i);
        let e = self.local(e);
        self.check(e, false)
    }

    /// Calls a `void` method. A Java exception it throws is left pending.
    pub fn call_void(&self, obj: jobject, method: MethodId, args: &[jvalue]) {
        jni!(self, CallVoidMethodA, obj, method.0, args.as_ptr());
    }

    pub fn call_static_object(
        &self,
        cls: &Global,
        method: MethodId,
        args: &[jvalue],
    ) -> Result<Local<'_>> {
        let r = jni!(
            self,
            CallStaticObjectMethodA,
            cls.raw,
            method.0,
            args.as_ptr()
        );
        let r = self.local(r);
        self.check((), r.is_none())?;
        r.ok_or(Pending)
    }
}

/// A local reference, deleted when dropped (local references are a limited
/// resource on threads that stay in native code, such as the event thread).
pub struct Local<'e> {
    env: &'e Env,
    raw: jobject,
}

impl Local<'_> {
    pub fn as_raw(&self) -> jobject {
        self.raw
    }

    /// Hands the reference to Java as a return value.
    pub fn into_raw(self) -> jobject {
        let raw = self.raw;
        std::mem::forget(self);
        raw
    }
}

impl Drop for Local<'_> {
    fn drop(&mut self) {
        jni!(self.env, DeleteLocalRef, self.raw);
    }
}

/// A global reference, deleted when dropped, on whichever thread that is.
pub struct Global {
    vm: Vm,
    raw: jobject,
}

// SAFETY: global references are valid on every thread; Drop attaches the
// dropping thread if needed.
unsafe impl Send for Global {}
unsafe impl Sync for Global {}

impl Global {
    pub fn as_raw(&self) -> jobject {
        self.raw
    }
}

impl Drop for Global {
    fn drop(&mut self) {
        let raw = self.raw;
        self.vm.with_env(|env| jni!(env, DeleteGlobalRef, raw));
    }
}

/// A method ID. Valid on every thread while its class is loaded, which the
/// cached global class references guarantee.
#[derive(Clone, Copy)]
pub struct MethodId(jmethodID);

// SAFETY: see above
unsafe impl Send for MethodId {}
unsafe impl Sync for MethodId {}

/// The process's Java VM.
#[derive(Clone, Copy)]
pub struct Vm(*mut JavaVM);

// SAFETY: the JavaVM pointer is valid for the life of the process, on every thread
unsafe impl Send for Vm {}
unsafe impl Sync for Vm {}

macro_rules! invoke {
    ($vm:expr, $name:ident $(, $arg:expr)* $(,)?) => {{
        let raw = $vm.0;
        // SAFETY: `raw` is the process's JavaVM (an invariant of Vm)
        unsafe { ((**raw).v1_2.$name)(raw $(, $arg)*) }
    }};
}

impl Vm {
    pub fn as_raw(self) -> *mut c_void {
        self.0.cast()
    }

    /// The current thread's env if it is attached.
    fn current_env(self) -> Option<Env> {
        let mut env: *mut c_void = ptr::null_mut();
        let r = invoke!(self, GetEnv, &mut env, JNI_VERSION_1_6);
        // SAFETY: GetEnv returned this thread's env
        (r == JNI_OK && !env.is_null()).then(|| unsafe { Env::from_raw(env.cast()) })
    }

    /// Runs `f` with an env for the current thread, attaching it for the
    /// duration if it is not attached. Does nothing if attaching fails.
    pub fn with_env(self, f: impl FnOnce(&Env)) {
        if let Some(env) = self.current_env() {
            f(&env);
        } else if let Some(attached) = self.attach_current_thread() {
            f(attached.env());
        }
    }

    /// Attaches the current thread until the returned guard is dropped.
    pub fn attach_current_thread(self) -> Option<Attached> {
        let mut env: *mut c_void = ptr::null_mut();
        let r = invoke!(self, AttachCurrentThread, &mut env, ptr::null_mut());
        if r != JNI_OK || env.is_null() {
            return None;
        }
        Some(Attached {
            vm: self,
            // SAFETY: AttachCurrentThread returned this thread's env
            env: unsafe { Env::from_raw(env.cast()) },
        })
    }
}

/// The current thread attached to the VM; detached when dropped.
pub struct Attached {
    vm: Vm,
    env: Env,
}

impl Attached {
    pub fn env(&self) -> &Env {
        &self.env
    }
}

impl Drop for Attached {
    fn drop(&mut self) {
        invoke!(self.vm, DetachCurrentThread);
    }
}
