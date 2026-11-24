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
//!   for that thread); so are the stream registries' locks (`stream`) and
//!   the protocol list's;
//! - the surface lock is held around one `mpv_set_option("wid")` call, and the
//!   observations lock around `mpv_observe_property`/`mpv_unobserve_property`
//!   calls (so that an observation and its removal cannot interleave); each is
//!   taken only by a thread that is inside the gate, never together, and they
//!   are the only locks ever held while another (the lifecycle lock, when an
//!   `Active` drops) is taken, so there is a single order: surface or
//!   observations, then lifecycle. mpv runs neither call through its core
//!   thread nor calls back into us from them;
//! - `destroy` holds no lock while it waits. It waits for two things: the
//!   event thread to finish (whose callbacks may call back into the instance:
//!   the gate still admits them, without blocking, until the join is done),
//!   then for the calls in flight to leave the gate. Those wait only for mpv's
//!   core, which does not depend on us, with one exception: a synchronous
//!   command that opens a URL of a custom protocol itself (mpv runs such
//!   commands on the calling thread) runs that stream's callbacks, app code,
//!   inside the call. Those callbacks cannot call the instance or `destroy`
//!   (refused, see below), so they cannot wait for `destroy`; one that
//!   blocks forever blocks `destroy` too, as MPVLib's documentation says.
//!   Nothing else either waits for can be waiting for `destroy`, so the
//!   waits end. A second thread calling `destroy` meanwhile waits for the
//!   first to finish (`Lifecycle::wait_destroyed`), unless it is the event
//!   thread or in a stream callback, which are refused instead: the first
//!   one may be waiting for exactly them.
//! - Stream cancellation never runs app code on a waiting thread: mpv calls
//!   `cancel_fn` on the thread that triggers it, with its `abort_lock` held
//!   (an app thread in `mpv_abort_async_command`, inside the gate, or the
//!   thread in `mpv_terminate_destroy`), and `cancel_fn` only queues the
//!   cancellation for the `stream::canceller` thread.
//! - The asynchronous calls only queue a request in the gate; their replies
//!   are events like any other, delivered by the event thread. A reply that
//!   was still on its way when `destroy` stopped the event thread is never
//!   delivered: Kotlin fails the request when `destroy` returns.
//! - Stream callbacks (`stream`) run on mpv's own threads, outside the gate
//!   (or inside a call, as above), and `mpv_terminate_destroy` waits for them
//!   (it closes every stream). A
//!   thread inside a stream callback of this instance is refused at `enter`
//!   and in `destroy` (stream_cb.h: calling mpv from there deadlocks), so
//!   they never wait for anything of ours either. Protocols stay registered
//!   until `mpv_terminate_destroy` has returned, the last moment mpv may call
//!   their `open_fn`.
//!
//! The one way to wedge `destroy` is in Kotlin: a callback or stream provider
//! that blocks on the thread calling `destroy`, which MPVLib's documentation
//! forbids.

use std::borrow::Cow;
use std::ffi::{CStr, CString, c_int, c_void};
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
use crate::node;
use crate::observations::{self, Observations};
use crate::registry::Registry;
use crate::stream;
use crate::stream_jni;

pub const ILLEGAL_STATE: &CStr = c"java/lang/IllegalStateException";
pub const RUNTIME: &CStr = c"java/lang/RuntimeException";

/// Why a JNI call returns to Java with an exception.
pub enum Failure {
    /// already pending (thrown by a JNI function, or NullPointerException)
    Pending,
    Throw(&'static CStr, Cow<'static, str>),
    /// `dev.jdtech.mpv.MpvException(error, operation)`
    Mpv {
        error: c_int,
        operation: String,
    },
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

    pub fn mpv(error: c_int, operation: impl Into<String>) -> Failure {
        Failure::Mpv {
            error,
            operation: operation.into(),
        }
    }

    pub fn from_stream_callback() -> Failure {
        Failure::Throw(
            ILLEGAL_STATE,
            Cow::Borrowed(
                "MPVLib was called from one of its own stream callbacks (mpv's stream thread); mpv would deadlock",
            ),
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
    /// registered stream protocols (`stream_jni` ids), dropped after
    /// `mpv_terminate_destroy`
    protocols: Mutex<Vec<i64>>,
    /// the ids of `MPVLib.observeProperty`'s observations
    observations: Mutex<Observations>,
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
/// app's classes (the event thread and mpv's threads, attached from native
/// code, cannot).
pub struct Methods {
    _mpvlib: Global, // keeps the method IDs below valid
    event: MethodId,
    event_end_file: MethodId,
    property: MethodId,
    property_flag: MethodId,
    property_long: MethodId,
    property_double: MethodId,
    property_string: MethodId,
    log_message: MethodId,
    on_property_change: MethodId,
    on_reply: MethodId,
    on_event: MethodId,
    integer: Global,
    integer_value_of: MethodId,
    long: Global,
    long_value_of: MethodId,
    double: Global,
    double_value_of: MethodId,
    boolean: Global,
    boolean_value_of: MethodId,
    string: Global,
    mpv_exception: Global,
    mpv_exception_init: MethodId,
    _protocol_adapter: Global,
    pub protocol_open: MethodId,
    _stream_adapter: Global,
    pub stream_buffer: MethodId,
    pub stream_read: MethodId,
    pub stream_seek: MethodId,
    pub stream_size: MethodId,
    pub stream_close: MethodId,
    pub stream_cancel: MethodId,
}

static METHODS: OnceLock<Methods> = OnceLock::new();

/// The cached methods; `None` only before the first `create`, when no
/// instance (and so no callback) exists.
pub fn methods() -> Option<&'static Methods> {
    METHODS.get()
}

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
        let on_property_change = m(c"onPropertyChange", c"(JLjava/lang/Object;)V")?;
        let on_reply = m(c"onReply", c"(JI[B)V")?;
        let on_event = m(c"onEvent", c"(IJIIJI[Ljava/lang/String;)V")?;

        let boxed = |class: &CStr, sig: &CStr| -> jni::Result<(Global, MethodId)> {
            let cls = env.find_class(class)?;
            let value_of = env.static_method_id(&cls, c"valueOf", sig)?;
            Ok((global(&cls)?, value_of))
        };
        let (integer, integer_value_of) = boxed(c"java/lang/Integer", c"(I)Ljava/lang/Integer;")?;
        let (long, long_value_of) = boxed(c"java/lang/Long", c"(J)Ljava/lang/Long;")?;
        let (double, double_value_of) = boxed(c"java/lang/Double", c"(D)Ljava/lang/Double;")?;
        let (boolean, boolean_value_of) = boxed(c"java/lang/Boolean", c"(Z)Ljava/lang/Boolean;")?;
        let string = global(&env.find_class(c"java/lang/String")?)?;

        let exception = env.find_class(c"dev/jdtech/mpv/MpvException")?;
        let mpv_exception_init = env.method_id(&exception, c"<init>", c"(ILjava/lang/String;)V")?;

        let protocol = env.find_class(c"dev/jdtech/mpv/internal/ProtocolAdapter")?;
        let protocol_open = env.method_id(
            &protocol,
            c"open",
            c"(Ljava/lang/String;)Ldev/jdtech/mpv/internal/StreamAdapter;",
        )?;
        let stream = env.find_class(c"dev/jdtech/mpv/internal/StreamAdapter")?;
        let s = |name: &CStr, sig: &CStr| env.method_id(&stream, name, sig);
        let stream_buffer = s(c"buffer", c"()[B")?;
        let stream_read = s(c"read", c"(I)I")?;
        let stream_seek = s(c"seek", c"(J)J")?;
        let stream_size = s(c"size", c"()J")?;
        let stream_close = s(c"close", c"()V")?;
        let stream_cancel = s(c"cancel", c"()V")?;

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
            on_property_change,
            on_reply,
            on_event,
            integer,
            integer_value_of,
            long,
            long_value_of,
            double,
            double_value_of,
            boolean,
            boolean_value_of,
            string,
            mpv_exception: global(&exception)?,
            mpv_exception_init,
            _protocol_adapter: global(&protocol)?,
            protocol_open,
            _stream_adapter: global(&stream)?,
            stream_buffer,
            stream_read,
            stream_seek,
            stream_size,
            stream_close,
            stream_cancel,
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
    Ok(box_local(env, m, value)?.into_raw())
}

fn box_local<'e>(env: &'e Env, m: &Methods, value: Boxed) -> jni::Result<jni::Local<'e>> {
    let (cls, method, arg) = match value {
        Boxed::Int(i) => (&m.integer, m.integer_value_of, jvalue { i }),
        Boxed::Long(j) => (&m.long, m.long_value_of, jvalue { j }),
        Boxed::Double(d) => (&m.double, m.double_value_of, jvalue { d }),
        Boxed::Boolean(z) => (&m.boolean, m.boolean_value_of, jvalue { z }),
    };
    env.call_static_object(cls, method, &[arg])
}

/// Throws `MpvException(error, operation)`, or a RuntimeException if that
/// cannot be built.
pub fn throw_mpv(env: &Env, error: c_int, operation: &str) {
    let thrown = (|| -> jni::Result<()> {
        let m = METHODS.get().ok_or(Pending)?;
        let op = CString::new(operation.replace('\0', "?")).unwrap_or_default();
        let op = env.new_string(&op)?;
        let ex = env.new_object(
            &m.mpv_exception,
            m.mpv_exception_init,
            &[jvalue { i: error }, jvalue { l: op.as_raw() }],
        )?;
        env.throw_object(&ex);
        Ok(())
    })();
    if thrown.is_err() && !env.exception_pending() {
        env.throw(
            RUNTIME,
            &format!("{operation}: mpv error {error} ({})", error_string(error)),
        );
    }
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
        protocols: Mutex::new(Vec::new()),
        observations: Mutex::new(Observations::default()),
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
    /// What marks this instance's stream callbacks (`stream::in_callback_of`):
    /// its address, stable while it exists.
    fn owner(&self) -> usize {
        ptr::from_ref(self) as usize
    }

    /// Whether the current thread is inside one of this instance's stream
    /// callbacks, where every call into the instance is refused.
    pub fn in_own_stream_callback(&self) -> bool {
        stream::in_callback_of(self.owner())
    }

    pub fn enter(&self) -> Result<Active<'_>> {
        if stream::in_callback_of(self.owner()) {
            return Err(Failure::from_stream_callback());
        }
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

    /// `nativeObserveProperty`: observes with a new id, remembered under the
    /// name for `unobserve_legacy`. An error is logged, as before.
    pub fn observe_legacy(&self, name: &CStr, format: c_int) -> Result<()> {
        let active = self.enter()?;
        let mut observations = self
            .observations
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let id = observations.add(name);
        // SAFETY: an admitted call and a NUL-terminated name
        let r = unsafe { mpv_observe_property(active.mpv(), id, name.as_ptr(), format) };
        if r < 0 {
            observations.remove(name, id);
            log::error(&format!(
                "mpv_observe_property({}) format {format} returned error {}",
                name.to_string_lossy(),
                error_string(r)
            ));
        }
        Ok(())
    }

    /// `Native.unobserveLegacyProperty`: undoes every `observe_legacy` of
    /// `name`; the number of observations removed or a negative mpv error.
    pub fn unobserve_legacy(&self, name: &CStr) -> Result<c_int> {
        let active = self.enter()?;
        let mut observations = self
            .observations
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let mut removed: c_int = 0;
        for id in observations.take(name) {
            // SAFETY: an admitted call
            let r = unsafe { mpv_unobserve_property(active.mpv(), id) };
            if r < 0 {
                return Ok(r);
            }
            removed = removed.saturating_add(r);
        }
        Ok(removed)
    }

    /// `Native.addStreamProtocol`: registers `adapter` (a ProtocolAdapter)
    /// for `protocol://` URIs.
    pub fn add_stream_protocol(&self, env: &Env, protocol: &CStr, adapter: jobject) -> Result<()> {
        let active = self.enter()?;
        let backend = stream_jni::JniProtocol::new(env, self.vm, adapter, self.owner())?;
        let name = protocol.to_string_lossy();
        let Some((id, user_data, open_fn)) = stream::register(backend) else {
            return Err(Failure::runtime("out of stream protocol ids"));
        };
        // SAFETY: an admitted call; `user_data` and `open_fn` stay valid until
        // `destroy` unregisters them after mpv_terminate_destroy
        let r =
            unsafe { mpv_stream_cb_add_ro(active.mpv(), protocol.as_ptr(), user_data, open_fn) };
        if r < 0 {
            stream::unregister::<stream_jni::JniProtocol>(id);
            return Err(Failure::mpv(r, format!("registerStreamProtocol({name})")));
        }
        // while still in the gate, so destroy (after finish_close) sees it
        self.protocols
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(id);
        Ok(())
    }
}

/// `nativeDestroy`. Returns once the instance is torn down: when another
/// thread is already destroying it, after waiting for that thread to finish.
/// A handle that is unknown or already destroyed is a no-op, like a second
/// `destroy()`. Refused (IllegalStateException, nothing changed) from the
/// event thread and from this instance's stream callbacks, which the teardown
/// waits for.
pub fn destroy(handle: jlong) -> Result<()> {
    let Some(instance) = lookup(handle) else {
        log::verbose(|| "Cannot destroy mpv: mpv is not initialized".into());
        return Ok(());
    };
    // mpv_terminate_destroy would wait for this very callback to return
    if stream::in_callback_of(instance.owner()) {
        return Err(Failure::from_stream_callback());
    }

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
        // another thread is tearing it down: return when it is done, as it will.
        // (This thread is not one that teardown waits for: those are refused above,
        // in_callback_of, or by begin_close, FromWorker.)
        Err(CloseError::AlreadyClosing) => {
            instance.lifecycle.wait_destroyed();
            return Ok(());
        }
    };
    // however this function ends (a panic included), never leave losers waiting
    let _destroyed = Destroyed(&instance.lifecycle);

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
    // SAFETY: the event thread is joined and no call is in flight or can start.
    // This closes every custom stream (their close callbacks run meanwhile, on
    // mpv's threads) and is the last point at which mpv may open new ones.
    unsafe { mpv_terminate_destroy(instance.mpv.0) };

    // mpv no longer opens streams for the protocols, nor renders to the surface
    for id in instance
        .protocols
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .drain(..)
    {
        stream::unregister::<stream_jni::JniProtocol>(id);
    }
    instance
        .surface
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .take();
    INSTANCES.remove(handle);
    // (the guard `_destroyed`, dropped here, releases the callers waiting above)
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

/// The encoding of `node` as a `byte[]`, or the mpv error to report instead.
///
/// # Safety
/// `node` is a valid node from mpv.
unsafe fn node_bytes<'e>(
    env: &'e Env,
    node: &mpv_node,
) -> std::result::Result<jni::Local<'e>, c_int> {
    let mut out = Vec::new();
    // SAFETY: the caller's contract
    if let Err(e) = unsafe { node::encode(node, &mut out) } {
        log::error(&format!("cannot pass an mpv node to Java: {e:?}"));
        return Err(MPV_ERROR_GENERIC);
    }
    env.new_byte_array(&out).map_err(|_| {
        env.describe_and_clear_exception();
        MPV_ERROR_NOMEM
    })
}

/// A property value as the Java object `onPropertyChange` takes: Boolean,
/// Long, Double, String, byte[] (a node) or null.
fn java_value<'e>(
    env: &'e Env,
    m: &Methods,
    value: &Value<'_>,
) -> jni::Result<Option<jni::Local<'e>>> {
    Ok(match value {
        Value::None | Value::Unsupported(_) => None,
        Value::Flag(z) => Some(box_local(env, m, Boxed::Boolean(*z))?),
        Value::Int64(j) => Some(box_local(env, m, Boxed::Long(*j))?),
        Value::Double(d) => Some(box_local(env, m, Boxed::Double(*d))?),
        Value::String(s) => Some(env.new_string(s)?),
        // SAFETY: a node in the current event
        Value::Node(n) => unsafe { node_bytes(env, n.0) }.ok(),
    })
}

fn deliver(env: &Env, m: &Methods, this: jobject, ev: &mpv_event) -> jni::Result<()> {
    // SAFETY: `ev` came from mpv_wait_event and is used before the next one
    let Some(event) = (unsafe { event::decode(ev) }) else {
        return Ok(());
    };
    // Each callback runs to completion before the next JNI call: an exception
    // it leaves (Kotlin's dispatch catches Exceptions, not Errors) is logged
    // and cleared, so the calls after it are still made.
    let call = |method, args: &[jvalue]| {
        env.call_void(this, method, args);
        env.describe_and_clear_exception();
    };
    // the event flow's callback, after the MPVLib.EventObserver one
    let flow_event = |id: mpv_event_id, end: Option<event::EndFile>, entry: i64, args: jobject| {
        let end = end.unwrap_or(event::EndFile {
            reason: 0,
            error: 0,
            playlist_entry_id: entry,
            playlist_insert_id: 0,
            playlist_insert_num_entries: 0,
        });
        call(
            m.on_event,
            &[
                jvalue { i: id },
                jvalue {
                    j: end.playlist_entry_id,
                },
                jvalue { i: end.reason },
                jvalue { i: end.error },
                jvalue {
                    j: end.playlist_insert_id,
                },
                jvalue {
                    i: end.playlist_insert_num_entries,
                },
                jvalue { l: args },
            ],
        );
    };
    let reply = |userdata: u64, error: c_int, result: Option<&jni::Local<'_>>| {
        call(
            m.on_reply,
            &[
                jvalue { j: userdata as i64 },
                jvalue { i: error },
                jvalue {
                    l: result.map_or(ptr::null_mut(), jni::Local::as_raw),
                },
            ],
        );
    };

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
        // MPVLib.observeProperty uses 0 (before) or negative ids
        Event::Property {
            userdata,
            name,
            value,
        } if observations::is_legacy(userdata) => {
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
                Value::Node(_) | Value::Unsupported(_) => log::verbose(|| {
                    let format = match value {
                        Value::Unsupported(f) => f,
                        _ => MPV_FORMAT_NODE,
                    };
                    format!(
                        "sendPropertyUpdateToJava: Unknown property update format received in callback: {format}!"
                    )
                }),
            }
        }
        // a property flow
        Event::Property {
            userdata, value, ..
        } => {
            let value = java_value(env, m, &value)?;
            call(
                m.on_property_change,
                &[
                    jvalue { j: userdata as i64 },
                    jvalue {
                        l: value.as_ref().map_or(ptr::null_mut(), jni::Local::as_raw),
                    },
                ],
            );
        }
        // Kotlin's eventEndFile also delivers event(MPV_EVENT_END_FILE)
        Event::EndFile(end) => {
            call(
                m.event_end_file,
                &[jvalue { i: end.reason }, jvalue { i: end.error }],
            );
            flow_event(MPV_EVENT_END_FILE, Some(end), 0, ptr::null_mut());
        }
        Event::StartFile { playlist_entry_id } => {
            log::verbose(|| format!("event: {}", event_name(MPV_EVENT_START_FILE)));
            call(
                m.event,
                &[jvalue {
                    i: MPV_EVENT_START_FILE,
                }],
            );
            flow_event(
                MPV_EVENT_START_FILE,
                None,
                playlist_entry_id,
                ptr::null_mut(),
            );
        }
        Event::ClientMessage(args) => {
            log::verbose(|| format!("event: {}", event_name(MPV_EVENT_CLIENT_MESSAGE)));
            call(
                m.event,
                &[jvalue {
                    i: MPV_EVENT_CLIENT_MESSAGE,
                }],
            );
            let args = env.new_string_array(&m.string, &args)?;
            flow_event(MPV_EVENT_CLIENT_MESSAGE, None, 0, args.as_raw());
        }
        // Replies go only to the request (MPVLib's async calls), never to an
        // EventObserver: every reply must reach it, or a coroutine would wait
        // forever, so a failure to build the result is reported as an error.
        Event::GetPropertyReply {
            userdata,
            error,
            value,
        } => match value {
            Value::Node(n) if error >= 0 => {
                // SAFETY: a node in the current event
                match unsafe { node_bytes(env, n.0) } {
                    Ok(bytes) => reply(userdata, error, Some(&bytes)),
                    Err(e) => reply(userdata, e, None),
                }
            }
            _ if error >= 0 => match env.new_byte_array(&[0]) {
                Ok(none) => reply(userdata, error, Some(&none)),
                Err(_) => {
                    env.describe_and_clear_exception();
                    reply(userdata, MPV_ERROR_NOMEM, None);
                }
            },
            _ => reply(userdata, error, None),
        },
        Event::SetPropertyReply { userdata, error } => reply(userdata, error, None),
        Event::CommandReply {
            userdata,
            error,
            result,
        } => match result {
            // SAFETY: a node in the current event
            Some(n) => match unsafe { node_bytes(env, n.0) } {
                Ok(bytes) => reply(userdata, error, Some(&bytes)),
                Err(e) => reply(userdata, e, None),
            },
            None => reply(userdata, error, None),
        },
        Event::Other(id) => {
            log::verbose(|| format!("event: {}", event_name(id)));
            call(m.event, &[jvalue { i: id }]);
            flow_event(id, None, 0, ptr::null_mut());
        }
    }
    Ok(())
}

/// Marks the teardown finished (`Lifecycle::finish_destroy`) when dropped.
struct Destroyed<'a>(&'a Lifecycle<JoinHandle<()>>);

impl Drop for Destroyed<'_> {
    fn drop(&mut self) {
        self.0.finish_destroy();
    }
}
