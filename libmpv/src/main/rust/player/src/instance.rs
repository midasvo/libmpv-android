//! One mpv player: its handle, its event thread and its surface, and how it
//! is created and destroyed.
//!
//! Ownership: the registry owns each `Instance` (an `Arc`) from `create` until
//! `destroy`; Kotlin holds only the registry's handle. Every JNI call looks the
//! handle up and keeps its own `Arc` for the duration of the call, so the
//! memory outlives any call that found it. Whether the *mpv handle* may still
//! be used is decided by the instance's `Lifecycle` gate: a call uses mpv only
//! through an `Active` guard, and `destroy` calls `mpv_terminate_destroy`
//! only after the event thread is joined and the gate has drained.
//!
//! Locks, and why there is no deadlock:
//! - the registry lock and the lifecycle lock are leaves: held only for a few
//!   field updates, never while calling mpv or Java or taking another lock
//!   (`Lifecycle::start_worker` spawns a thread under it, but does not wait
//!   for that thread);
//! - the surface lock is held around one `mpv_set_option("wid")` call, and is
//!   taken only by a thread that is inside the gate; it is the only lock ever
//!   held while another (the lifecycle lock, when an `Active` drops) is taken,
//!   so there is a single order: surface, then lifecycle;
//! - `destroy` holds no lock while it waits. It waits for two things: the
//!   event thread to finish (whose callbacks may call back into the instance:
//!   the gate still admits them, without blocking, until the join is done),
//!   then for the calls in flight to leave the gate (none of which runs Java
//!   code or waits for another thread; they only wait for mpv's core, which
//!   does not depend on us). Nothing either waits for can be waiting for
//!   `destroy`, so the waits end.
//!
//! The one way to wedge `destroy` is in Kotlin: a callback that blocks on the
//! thread calling `destroy`, which MPVLib's documentation forbids.

use std::borrow::Cow;
use std::ffi::{CStr, c_void};
use std::panic::{self, AssertUnwindSafe};
use std::ptr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, Once, OnceLock, PoisonError};
use std::thread::{self, JoinHandle};

use jni_sys::{jlong, jobject, jvalue};

use crate::event::{self, Event, Value};
use crate::ffi::*;
use crate::jni::{self, Env, Global, MethodId, Pending, Vm};
use crate::lifecycle::{Call, CloseError, Lifecycle, StartError};
use crate::log;
use crate::registry::Registry;

pub const ILLEGAL_STATE: &CStr = c"java/lang/IllegalStateException";
pub const RUNTIME: &CStr = c"java/lang/RuntimeException";

/// Why a JNI call returns to Java with an exception.
pub enum Failure {
    /// already pending (thrown by a JNI function, or NullPointerException)
    Pending,
    Throw(&'static CStr, Cow<'static, str>),
}

impl From<Pending> for Failure {
    fn from(_: Pending) -> Self {
        Failure::Pending
    }
}

impl Failure {
    /// A RuntimeException, logged first like the C++ wrapper's `die()`.
    pub fn runtime(message: &'static str) -> Failure {
        log::error(message);
        Failure::Throw(RUNTIME, Cow::Borrowed(message))
    }

    pub fn destroyed() -> Failure {
        Failure::Throw(
            ILLEGAL_STATE,
            Cow::Borrowed("MPVLib was destroyed (or the handle is not one of its instances)"),
        )
    }
}

pub type Result<T> = std::result::Result<T, Failure>;

/// The mpv client handle. The client API may be called from any thread.
struct Mpv(*mut mpv_handle);

// SAFETY: see above; when it may be called is the Lifecycle's business
unsafe impl Send for Mpv {}
unsafe impl Sync for Mpv {}

pub struct Instance {
    mpv: Mpv,
    vm: Vm,
    /// the MPVLib object the callbacks go to
    java: Global,
    lifecycle: Lifecycle<JoinHandle<()>>,
    stop: AtomicBool,
    /// the Surface mpv renders to, kept alive while mpv may use it
    surface: Mutex<Option<Global>>,
}

/// A call admitted by the gate: the only way to reach the mpv handle.
pub struct Active<'a> {
    instance: &'a Instance,
    _call: Call<'a, JoinHandle<()>>,
}

impl Active<'_> {
    pub fn mpv(&self) -> *mut mpv_handle {
        self.instance.mpv.0
    }
}

static INSTANCES: Registry<Instance> = Registry::new();

pub fn lookup(handle: jlong) -> Option<Arc<Instance>> {
    INSTANCES.get(handle)
}

/// Method and class references, looked up once on a thread that can see the
/// app's classes (the event thread, attached from native code, cannot).
struct Methods {
    _mpvlib: Global, // keeps the method IDs below valid
    event: MethodId,
    event_end_file: MethodId,
    property: MethodId,
    property_flag: MethodId,
    property_long: MethodId,
    property_double: MethodId,
    property_string: MethodId,
    log_message: MethodId,
    integer: Global,
    integer_value_of: MethodId,
    long: Global,
    long_value_of: MethodId,
    double: Global,
    double_value_of: MethodId,
    boolean: Global,
    boolean_value_of: MethodId,
}

static METHODS: OnceLock<Methods> = OnceLock::new();

impl Methods {
    fn get(env: &Env, vm: Vm) -> jni::Result<&'static Methods> {
        if let Some(m) = METHODS.get() {
            return Ok(m);
        }
        let loaded = Methods::load(env, vm)?;
        // if another thread won the race, `loaded` is dropped with its references
        Ok(METHODS.get_or_init(|| loaded))
    }

    fn load(env: &Env, vm: Vm) -> jni::Result<Methods> {
        let global = |cls: &jni::Local<'_>| env.new_global(vm, cls.as_raw()).ok_or(Pending);

        let mpvlib = env.find_class(c"dev/jdtech/mpv/MPVLib")?;
        let m = |name: &CStr, sig: &CStr| env.method_id(&mpvlib, name, sig);
        let event = m(c"event", c"(I)V")?;
        let event_end_file = m(c"eventEndFile", c"(II)V")?;
        let property = m(c"eventProperty", c"(Ljava/lang/String;)V")?;
        let property_flag = m(c"eventProperty", c"(Ljava/lang/String;Z)V")?;
        let property_long = m(c"eventProperty", c"(Ljava/lang/String;J)V")?;
        let property_double = m(c"eventProperty", c"(Ljava/lang/String;D)V")?;
        let property_string = m(c"eventProperty", c"(Ljava/lang/String;Ljava/lang/String;)V")?;
        let log_message = m(c"logMessage", c"(Ljava/lang/String;ILjava/lang/String;)V")?;

        let boxed = |class: &CStr, sig: &CStr| -> jni::Result<(Global, MethodId)> {
            let cls = env.find_class(class)?;
            let value_of = env.static_method_id(&cls, c"valueOf", sig)?;
            Ok((global(&cls)?, value_of))
        };
        let (integer, integer_value_of) = boxed(c"java/lang/Integer", c"(I)Ljava/lang/Integer;")?;
        let (long, long_value_of) = boxed(c"java/lang/Long", c"(J)Ljava/lang/Long;")?;
        let (double, double_value_of) = boxed(c"java/lang/Double", c"(D)Ljava/lang/Double;")?;
        let (boolean, boolean_value_of) = boxed(c"java/lang/Boolean", c"(Z)Ljava/lang/Boolean;")?;

        Ok(Methods {
            _mpvlib: global(&mpvlib)?,
            event,
            event_end_file,
            property,
            property_flag,
            property_long,
            property_double,
            property_string,
            log_message,
            integer,
            integer_value_of,
            long,
            long_value_of,
            double,
            double_value_of,
            boolean,
            boolean_value_of,
        })
    }
}

/// A boxed `java.lang.Integer`/`Long`/`Double`/`Boolean`, for the nullable
/// property getters.
pub enum Boxed {
    Int(i32),
    Long(i64),
    Double(f64),
    Boolean(bool),
}

pub fn box_value(env: &Env, value: Boxed) -> jni::Result<jobject> {
    let m = METHODS.get().ok_or(Pending)?; // loaded by create, before any handle exists
    let (cls, method, arg) = match value {
        Boxed::Int(i) => (&m.integer, m.integer_value_of, jvalue { i }),
        Boxed::Long(j) => (&m.long, m.long_value_of, jvalue { j }),
        Boxed::Double(d) => (&m.double, m.double_value_of, jvalue { d }),
        Boxed::Boolean(z) => (&m.boolean, m.boolean_value_of, jvalue { z }),
    };
    Ok(env.call_static_object(cls, method, &[arg])?.into_raw())
}

static FFMPEG_JNI: Once = Once::new();

/// What every instance needs from the process, set up by the first `create`.
fn prepare_environment(env: &Env, vm: Vm, app_context: jobject) {
    // mpv parses and prints numbers with the C library: the decimal point must be '.'
    // SAFETY: a valid category and NUL-terminated locale name
    unsafe { setlocale(LC_NUMERIC, c"C".as_ptr()) };

    FFMPEG_JNI.call_once(|| {
        // SAFETY: FFmpeg keeps both pointers for the life of the process. The
        // app context's global reference is never deleted for that reason; it
        // is the Application, which lives as long as the process.
        unsafe {
            av_jni_set_java_vm(vm.as_raw(), ptr::null_mut());
            let ctx = env.new_global_leaked(app_context);
            if !ctx.is_null() {
                av_jni_set_android_app_ctx(ctx.cast(), ptr::null_mut());
            }
        }
    });
}

/// `nativeCreate`: 0 if mpv could not be created, as before.
pub fn create(env: &Env, this: jobject, app_context: jobject) -> Result<jlong> {
    let vm = env.vm()?;
    let java = env.new_global(vm, this).ok_or_else(|| {
        if env.exception_pending() {
            Failure::Pending
        } else {
            Failure::runtime("MPVLib object is null")
        }
    })?;
    prepare_environment(env, vm, app_context);
    Methods::get(env, vm)?;

    // SAFETY: no preconditions
    let mpv = unsafe { mpv_create() };
    if mpv.is_null() {
        return Ok(0); // `java` is released by its drop
    }
    // SAFETY: a fresh handle and a NUL-terminated level
    unsafe { mpv_request_log_messages(mpv, c"v".as_ptr()) };

    let instance = Arc::new(Instance {
        mpv: Mpv(mpv),
        vm,
        java,
        lifecycle: Lifecycle::new(),
        stop: AtomicBool::new(false),
        surface: Mutex::new(None),
    });
    match INSTANCES.insert(instance) {
        Ok(handle) => Ok(handle),
        Err(instance) => {
            // SAFETY: nothing else has seen this handle
            unsafe { mpv_terminate_destroy(instance.mpv.0) };
            Err(Failure::runtime("out of MPVLib handles"))
        }
    }
}

impl Instance {
    pub fn enter(&self) -> Result<Active<'_>> {
        let call = self.lifecycle.enter().ok_or_else(Failure::destroyed)?;
        Ok(Active {
            instance: self,
            _call: call,
        })
    }

    /// `nativeInit`: `mpv_initialize` and the event thread.
    pub fn init(self: &Arc<Self>) -> Result<()> {
        let active = self.enter()?;
        // SAFETY: an admitted call
        if unsafe { mpv_initialize(active.mpv()) } < 0 {
            return Err(Failure::runtime("mpv init failed"));
        }

        let me = Arc::clone(self);
        let started = self.lifecycle.start_worker(move || {
            thread::Builder::new()
                .name("event_thread".into())
                .spawn(move || event_loop(me))
                .map(|handle| {
                    let id = handle.thread().id();
                    (handle, id)
                })
        });
        match started {
            Ok(()) => Ok(()),
            Err(StartError::Spawn(_)) => Err(Failure::runtime("thread create failed")),
            // destroy() began meanwhile: it found no thread to join, so do not start one
            Err(StartError::Closing) => Err(Failure::destroyed()),
            // unreachable, since mpv_initialize fails the second time; do not start a second
            Err(StartError::AlreadyStarted) => Ok(()),
        }
    }

    /// `nativeAttachSurface`. The previous surface is released only after mpv
    /// accepted the new one; if it refuses, it keeps the old one.
    pub fn attach_surface(&self, env: &Env, surface: jobject) -> Result<()> {
        let active = self.enter()?;
        let mut current = self.surface.lock().unwrap_or_else(PoisonError::into_inner);

        let Some(new_surface) = env.new_global(self.vm, surface) else {
            if env.exception_pending() {
                return Err(Failure::Pending);
            }
            return Err(Failure::runtime("invalid surface provided"));
        };

        let mut wid = new_surface.as_raw() as isize as i64;
        // SAFETY: an admitted call; `wid` is an int64 as the format says
        let r = unsafe {
            mpv_set_option(
                active.mpv(),
                c"wid".as_ptr(),
                MPV_FORMAT_INT64,
                (&raw mut wid).cast::<c_void>(),
            )
        };
        if r < 0 {
            log::error(&format!(
                "mpv_set_option(wid) returned error {}",
                error_string(r)
            ));
            return Ok(()); // `new_surface` is released by its drop
        }
        *current = Some(new_surface); // the old one, if any, is released here
        Ok(())
    }

    /// `nativeDetachSurface`
    pub fn detach_surface(&self) -> Result<()> {
        let active = self.enter()?;
        let mut current = self.surface.lock().unwrap_or_else(PoisonError::into_inner);
        let mut wid: i64 = 0;
        // SAFETY: an admitted call; `wid` is an int64 as the format says
        let r = unsafe {
            mpv_set_option(
                active.mpv(),
                c"wid".as_ptr(),
                MPV_FORMAT_INT64,
                (&raw mut wid).cast::<c_void>(),
            )
        };
        if r < 0 {
            log::error(&format!(
                "mpv_set_option(wid) returned error {}",
                error_string(r)
            ));
        }
        *current = None;
        Ok(())
    }
}

/// `nativeDestroy`. A handle that is unknown, or whose instance another
/// thread is already destroying, is a no-op, like a second `destroy()`.
pub fn destroy(handle: jlong) -> Result<()> {
    let Some(instance) = lookup(handle) else {
        log::verbose(|| "Cannot destroy mpv: mpv is not initialized".into());
        return Ok(());
    };

    let worker = match instance.lifecycle.begin_close() {
        Ok(worker) => worker,
        // joining the event thread from itself would deadlock; refuse and
        // leave everything as it was
        Err(CloseError::FromWorker) => {
            return Err(Failure::Throw(
                ILLEGAL_STATE,
                Cow::Borrowed(
                    "destroy() was called from an MPVLib callback (mpv's event thread); call it from another thread",
                ),
            ));
        }
        Err(CloseError::AlreadyClosing) => return Ok(()),
    };

    // Stop the event thread. Until it is joined, its callbacks (and other
    // threads) may still use mpv: the gate admits them while Closing.
    instance.stop.store(true, Ordering::SeqCst);
    // SAFETY: mpv is not terminated before finish_close below, and only we get here
    unsafe { mpv_wakeup(instance.mpv.0) };
    if let Some(worker) = worker
        && worker.join().is_err()
    {
        log::error("the mpv event thread panicked");
    }

    // Refuse new calls and wait for those in flight: then nothing else uses mpv.
    instance.lifecycle.finish_close();
    // SAFETY: the event thread is joined and no call is in flight or can start
    unsafe { mpv_terminate_destroy(instance.mpv.0) };

    // mpv no longer renders to the surface
    instance
        .surface
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .take();
    INSTANCES.remove(handle);
    // The MPVLib global reference goes with the last Arc: here, or in a call
    // that looked the handle up just before and is about to fail.
    Ok(())
}

pub fn error_string(error: i32) -> String {
    // SAFETY: mpv returns a static string for every value
    let s = unsafe { mpv_error_string(error) };
    if s.is_null() {
        return error.to_string();
    }
    // SAFETY: NUL-terminated and static
    unsafe { CStr::from_ptr(s) }.to_string_lossy().into_owned()
}

fn event_name(id: mpv_event_id) -> String {
    // SAFETY: mpv returns a static string or null
    let s = unsafe { mpv_event_name(id) };
    if s.is_null() {
        return id.to_string();
    }
    // SAFETY: NUL-terminated and static
    unsafe { CStr::from_ptr(s) }.to_string_lossy().into_owned()
}

fn event_loop(instance: Arc<Instance>) {
    let Some(attached) = instance.vm.attach_current_thread() else {
        log::error("failed to acquire java env");
        return;
    };
    let env = attached.env();
    let Some(methods) = METHODS.get() else {
        return; // create() loaded them before this instance existed
    };
    // The handle stays valid until destroy() has joined this thread.
    let mpv = instance.mpv.0;
    let this = instance.java.as_raw();

    while !instance.stop.load(Ordering::SeqCst) {
        // SAFETY: only this thread waits on the handle, which is valid (above)
        let ev = unsafe { mpv_wait_event(mpv, -1.0) };
        if instance.stop.load(Ordering::SeqCst) {
            break;
        }
        // SAFETY: mpv returns a valid event, unchanged until the next wait
        let Some(ev) = (unsafe { ev.as_ref() }) else {
            continue;
        };
        let delivered = panic::catch_unwind(AssertUnwindSafe(|| deliver(env, methods, this, ev)));
        if delivered.is_err() {
            log::error("panic while delivering an mpv event; continuing");
        }
        // an exception thrown by an observer must not stay pending
        env.describe_and_clear_exception();
    }
    // `attached` detaches this thread from the VM when dropped
}

fn deliver(env: &Env, m: &Methods, this: jobject, ev: &mpv_event) -> jni::Result<()> {
    // SAFETY: `ev` came from mpv_wait_event and is used before the next one
    let Some(event) = (unsafe { event::decode(ev) }) else {
        return Ok(());
    };
    let call = |method, args: &[jvalue]| env.call_void(this, method, args);
    match event {
        Event::Log {
            prefix,
            level,
            text,
            log_level,
        } => {
            log::write(
                event::android_priority(log_level),
                &event::logcat_line(prefix, level, text),
            );
            let prefix = env.new_string(prefix)?;
            let text = env.new_string(text)?;
            call(
                m.log_message,
                &[
                    jvalue { l: prefix.as_raw() },
                    jvalue { i: log_level },
                    jvalue { l: text.as_raw() },
                ],
            );
        }
        Event::Property { name, value } => {
            let name = env.new_string(name)?;
            let n = jvalue { l: name.as_raw() };
            match value {
                Value::None => call(m.property, &[n]),
                Value::Flag(z) => call(m.property_flag, &[n, jvalue { z }]),
                Value::Int64(j) => call(m.property_long, &[n, jvalue { j }]),
                Value::Double(d) => call(m.property_double, &[n, jvalue { d }]),
                Value::String(s) => {
                    let s = env.new_string(s)?;
                    call(m.property_string, &[n, jvalue { l: s.as_raw() }]);
                }
                Value::Unsupported(format) => log::verbose(|| {
                    format!(
                        "sendPropertyUpdateToJava: Unknown property update format received in callback: {format}!"
                    )
                }),
            }
        }
        // Kotlin's eventEndFile also delivers event(MPV_EVENT_END_FILE)
        Event::EndFile { reason, error } => call(
            m.event_end_file,
            &[jvalue { i: reason }, jvalue { i: error }],
        ),
        Event::Other(id) => {
            log::verbose(|| format!("event: {}", event_name(id)));
            call(m.event, &[jvalue { i: id }]);
        }
    }
    Ok(())
}
