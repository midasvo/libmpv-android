//! Custom protocols (`mpv_stream_cb_add_ro`): the C callbacks mpv calls, and
//! what keeps everything they use alive.
//!
//! The callbacks are generic over a `Backend` (the JNI one in `stream_jni`, a
//! fake in the tests below), so their bookkeeping is tested on the host.
//!
//! Lifetimes, from `stream_cb.h` and mpv 0.42.0's `stream/stream_cb.c`:
//! - A protocol's `user_data` is passed to `open_fn` until
//!   `mpv_terminate_destroy` has returned (mpv looks it up under its lock,
//!   then calls `open_fn` without it, so a call may be in progress while the
//!   instance is being destroyed). Each protocol is therefore an entry in a
//!   registry, removed by `destroy` only after `mpv_terminate_destroy`.
//! - A stream's cookie is passed to `read_fn`, `seek_fn`, `size_fn` and
//!   `close_fn` until `close_fn` (all on the stream's thread), and to
//!   `cancel_fn` from any thread. `cancel_fn` can come *after* `close_fn`:
//!   mpv's `free_stream` calls `close_fn`, and only then frees the
//!   `mp_cancel` that holds the callback, so a cancellation triggered in
//!   between still calls it. The cookie is therefore not a pointer but a
//!   registry id that is never reused: `close_fn` removes the entry, a late
//!   `cancel_fn` finds nothing, and a `cancel_fn` that found the entry just
//!   before keeps it alive (an `Arc`) until it returns.
//! - `mpv_terminate_destroy` closes every stream before it returns.
//!
//! The callbacks must not call into mpv for the same instance (stream_cb.h:
//! that deadlocks). While one runs, the thread is marked with the instance
//! (`in_callback_of`) and the instance refuses calls from it with an
//! exception instead.

use std::cell::Cell;
use std::ffi::{CStr, c_char, c_int, c_void};
use std::panic::{self, AssertUnwindSafe};
use std::sync::Arc;

use crate::ffi::*;
use crate::registry::Registry;

/// mpv's read_fn returns int64, but stream_cb.c passes it on as an `int`.
const MAX_READ: usize = i32::MAX as usize;

/// One custom protocol: opens its streams.
pub trait Backend: Sized + Send + Sync + 'static {
    type Stream: Stream;
    fn protocols() -> &'static Registry<Self>;
    fn streams() -> &'static Registry<Self::Stream>;
    /// The instance the protocol belongs to (see `in_callback_of`).
    fn owner(&self) -> usize;
    /// Opens `uri`, `None` if it cannot be opened.
    fn open(&self, uri: &CStr) -> Option<Self::Stream>;
}

/// One open stream. `cancel` may be called from any thread, also while
/// another method runs and after `close`.
pub trait Stream: Send + Sync + 'static {
    fn owner(&self) -> usize;
    /// Bytes read into the start of `buf` (at most its length, which is at
    /// least 1), 0 at the end, negative on error.
    fn read(&self, buf: &mut [u8]) -> i64;
    /// The new position, or a negative mpv error.
    fn seek(&self, offset: i64) -> i64;
    /// The size in bytes, or negative if unknown.
    fn size(&self) -> i64;
    fn close(&self);
    fn cancel(&self);
}

thread_local! {
    /// The instance whose stream callback runs on this thread (0: none).
    static IN_CALLBACK_OF: Cell<usize> = const { Cell::new(0) };
}

/// Whether this thread is inside a stream callback of `owner`.
pub fn in_callback_of(owner: usize) -> bool {
    owner != 0 && IN_CALLBACK_OF.with(Cell::get) == owner
}

struct Scope(usize);

impl Scope {
    fn enter(owner: usize) -> Scope {
        Scope(IN_CALLBACK_OF.with(|c| c.replace(owner)))
    }
}

impl Drop for Scope {
    fn drop(&mut self) {
        IN_CALLBACK_OF.with(|c| c.set(self.0));
    }
}

fn to_cookie(id: i64) -> Option<*mut c_void> {
    // ids are positive; on 32-bit ABIs only the first 2^32 fit in a pointer
    usize::try_from(id).ok().map(|n| n as *mut c_void)
}

fn from_cookie(cookie: *mut c_void) -> i64 {
    // fits: every cookie came from to_cookie
    cookie as usize as i64
}

/// Runs a callback body, returning `on_panic` instead of unwinding into C.
fn guarded<R>(on_panic: R, body: impl FnOnce() -> R) -> R {
    panic::catch_unwind(AssertUnwindSafe(body)).unwrap_or(on_panic)
}

/// Registers `backend` and returns its id and the `user_data` and `open_fn`
/// to give `mpv_stream_cb_add_ro`. If mpv refuses it, `unregister(id)`.
pub fn register<B: Backend>(backend: B) -> Option<(i64, *mut c_void, mpv_stream_cb_open_ro_fn)> {
    let id = B::protocols().insert(Arc::new(backend)).ok()?;
    let Some(user_data) = to_cookie(id) else {
        B::protocols().remove(id);
        return None;
    };
    Some((id, user_data, Some(open_fn::<B>)))
}

/// Drops a protocol; only once mpv can no longer call its `open_fn`.
pub fn unregister<B: Backend>(id: i64) {
    B::protocols().remove(id);
}

/// # Safety
/// Called by mpv as `mpv_stream_cb_open_ro_fn`: `uri` is a NUL-terminated
/// string and `info` points to a writable `mpv_stream_cb_info`.
unsafe extern "C" fn open_fn<B: Backend>(
    user_data: *mut c_void,
    uri: *mut c_char,
    info: *mut mpv_stream_cb_info,
) -> c_int {
    guarded(MPV_ERROR_LOADING_FAILED, || {
        let Some(protocol) = B::protocols().get(from_cookie(user_data)) else {
            return MPV_ERROR_LOADING_FAILED;
        };
        if uri.is_null() || info.is_null() {
            return MPV_ERROR_LOADING_FAILED;
        }
        // SAFETY: mpv passes the URI as a C string
        let uri = unsafe { CStr::from_ptr(uri) };
        let stream = {
            let _scope = Scope::enter(protocol.owner());
            protocol.open(uri)
        };
        let Some(stream) = stream else {
            return MPV_ERROR_LOADING_FAILED;
        };
        let stream = Arc::new(stream);
        let Ok(id) = B::streams().insert(Arc::clone(&stream)) else {
            stream.close();
            return MPV_ERROR_LOADING_FAILED;
        };
        let Some(cookie) = to_cookie(id) else {
            B::streams().remove(id);
            stream.close();
            return MPV_ERROR_LOADING_FAILED;
        };
        // SAFETY: `info` is mpv's, valid for this call; set field by field
        unsafe {
            (*info).cookie = cookie;
            (*info).read_fn = Some(read_fn::<B>);
            (*info).seek_fn = Some(seek_fn::<B>);
            (*info).size_fn = Some(size_fn::<B>);
            (*info).close_fn = Some(close_fn::<B>);
            (*info).cancel_fn = Some(cancel_fn::<B>);
        }
        0
    })
}

/// # Safety
/// `buf` has room for `nbytes` bytes (mpv's `mpv_stream_cb_read_fn`).
unsafe extern "C" fn read_fn<B: Backend>(
    cookie: *mut c_void,
    buf: *mut c_char,
    nbytes: u64,
) -> i64 {
    guarded(-1, || {
        let Some(stream) = B::streams().get(from_cookie(cookie)) else {
            return -1;
        };
        if nbytes == 0 {
            return 0;
        }
        if buf.is_null() {
            return -1;
        }
        let n = usize::try_from(nbytes).unwrap_or(MAX_READ).min(MAX_READ);
        // SAFETY: mpv's buffer, at least `nbytes` long, ours during the call
        let buf = unsafe { std::slice::from_raw_parts_mut(buf.cast::<u8>(), n) };
        let r = {
            let _scope = Scope::enter(stream.owner());
            stream.read(buf)
        };
        if r < 0 || r as u64 > n as u64 { -1 } else { r }
    })
}

unsafe extern "C" fn seek_fn<B: Backend>(cookie: *mut c_void, offset: i64) -> i64 {
    guarded(i64::from(MPV_ERROR_GENERIC), || {
        let Some(stream) = B::streams().get(from_cookie(cookie)) else {
            return i64::from(MPV_ERROR_GENERIC);
        };
        if offset < 0 {
            return i64::from(MPV_ERROR_GENERIC);
        }
        let r = {
            let _scope = Scope::enter(stream.owner());
            stream.seek(offset)
        };
        match r {
            r if r >= 0 => r,
            r if r == i64::from(MPV_ERROR_UNSUPPORTED) => r,
            _ => i64::from(MPV_ERROR_GENERIC),
        }
    })
}

unsafe extern "C" fn size_fn<B: Backend>(cookie: *mut c_void) -> i64 {
    let unsupported = i64::from(MPV_ERROR_UNSUPPORTED);
    guarded(unsupported, || {
        let Some(stream) = B::streams().get(from_cookie(cookie)) else {
            return unsupported;
        };
        let r = {
            let _scope = Scope::enter(stream.owner());
            stream.size()
        };
        if r >= 0 { r } else { unsupported }
    })
}

unsafe extern "C" fn close_fn<B: Backend>(cookie: *mut c_void) {
    guarded((), || {
        // removed first: a cancel_fn from now on finds nothing
        if let Some(stream) = B::streams().remove(from_cookie(cookie)) {
            let _scope = Scope::enter(stream.owner());
            stream.close();
        }
    });
}

unsafe extern "C" fn cancel_fn<B: Backend>(cookie: *mut c_void) {
    guarded((), || {
        if let Some(stream) = B::streams().get(from_cookie(cookie)) {
            let _scope = Scope::enter(stream.owner());
            stream.cancel();
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ptr;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicUsize, Ordering};

    // Each test uses its own owner number; the registries are shared
    // statics, as in the real backend, and tests run in parallel.
    static PROTOCOLS: Registry<Fake> = Registry::new();
    static STREAMS: Registry<FakeStream> = Registry::new();

    struct Fake {
        owner: usize,
        data: Vec<u8>,
        log: Arc<Mutex<Vec<String>>>,
    }

    struct FakeStream {
        owner: usize,
        data: Vec<u8>,
        pos: AtomicUsize,
        log: Arc<Mutex<Vec<String>>>,
    }

    impl FakeStream {
        fn log(&self, s: String) {
            self.log.lock().unwrap().push(s);
        }
    }

    impl Backend for Fake {
        type Stream = FakeStream;
        fn protocols() -> &'static Registry<Self> {
            &PROTOCOLS
        }
        fn streams() -> &'static Registry<FakeStream> {
            &STREAMS
        }
        fn owner(&self) -> usize {
            self.owner
        }
        fn open(&self, uri: &CStr) -> Option<FakeStream> {
            assert!(in_callback_of(self.owner));
            self.log
                .lock()
                .unwrap()
                .push(format!("open {}", uri.to_str().unwrap()));
            if uri.to_bytes().ends_with(b"missing") {
                return None;
            }
            if uri.to_bytes().ends_with(b"panic") {
                panic!("provider panicked");
            }
            Some(FakeStream {
                owner: self.owner,
                data: self.data.clone(),
                pos: AtomicUsize::new(0),
                log: Arc::clone(&self.log),
            })
        }
    }

    impl Stream for FakeStream {
        fn owner(&self) -> usize {
            self.owner
        }
        fn read(&self, buf: &mut [u8]) -> i64 {
            assert!(in_callback_of(self.owner));
            let pos = self.pos.load(Ordering::SeqCst);
            // short reads of at most 3 bytes, as a network stream might do
            let n = buf.len().min(3).min(self.data.len() - pos);
            buf[..n].copy_from_slice(&self.data[pos..pos + n]);
            self.pos.store(pos + n, Ordering::SeqCst);
            self.log(format!("read {}", buf.len()));
            n as i64
        }
        fn seek(&self, offset: i64) -> i64 {
            if offset as usize > self.data.len() {
                return -1; // any failure
            }
            self.pos.store(offset as usize, Ordering::SeqCst);
            offset
        }
        fn size(&self) -> i64 {
            if self.data.is_empty() {
                -1
            } else {
                self.data.len() as i64
            }
        }
        fn close(&self) {
            self.log("close".into());
        }
        fn cancel(&self) {
            self.log("cancel".into());
        }
    }

    fn setup(owner: usize, data: &[u8]) -> (i64, *mut c_void, Arc<Mutex<Vec<String>>>) {
        let log = Arc::new(Mutex::new(Vec::new()));
        let (id, user_data, open) = register(Fake {
            owner,
            data: data.to_vec(),
            log: Arc::clone(&log),
        })
        .unwrap();
        assert!(open.is_some());
        (id, user_data, log)
    }

    fn empty_info() -> mpv_stream_cb_info {
        mpv_stream_cb_info {
            cookie: ptr::null_mut(),
            read_fn: None,
            seek_fn: None,
            size_fn: None,
            close_fn: None,
            cancel_fn: None,
        }
    }

    fn open(user_data: *mut c_void, uri: &CStr) -> (c_int, mpv_stream_cb_info) {
        let mut info = empty_info();
        let r = unsafe { open_fn::<Fake>(user_data, uri.as_ptr().cast_mut(), &raw mut info) };
        (r, info)
    }

    fn read_all(info: &mpv_stream_cb_info) -> Vec<u8> {
        let read = info.read_fn.unwrap();
        let mut out = Vec::new();
        let mut buf = [0u8; 16];
        loop {
            let n = unsafe { read(info.cookie, buf.as_mut_ptr().cast(), buf.len() as u64) };
            assert!(n >= 0);
            if n == 0 {
                return out;
            }
            out.extend_from_slice(&buf[..n as usize]);
        }
    }

    #[test]
    fn a_stream_reads_seeks_and_closes() {
        let (id, user_data, log) = setup(101, b"0123456789");
        let (r, info) = open(user_data, c"fake://a");
        assert_eq!(r, 0);
        assert!(!info.cookie.is_null());
        assert!(info.close_fn.is_some() && info.cancel_fn.is_some());
        assert!(
            !in_callback_of(101),
            "the mark is cleared after the callback"
        );

        let (seek, size) = (info.seek_fn.unwrap(), info.size_fn.unwrap());
        unsafe {
            assert_eq!(seek(info.cookie, 0), 0);
            assert_eq!(size(info.cookie), 10);
        }
        assert_eq!(read_all(&info), b"0123456789");
        unsafe {
            assert_eq!(seek(info.cookie, 7), 7);
            assert_eq!(seek(info.cookie, -1), i64::from(MPV_ERROR_GENERIC));
            // a provider's own failure value becomes MPV_ERROR_GENERIC
            assert_eq!(seek(info.cookie, 99), i64::from(MPV_ERROR_GENERIC));
        }
        assert_eq!(read_all(&info), b"789");

        // a zero-length read is answered without the provider
        let before = log.lock().unwrap().len();
        let n = unsafe { info.read_fn.unwrap()(info.cookie, ptr::null_mut(), 0) };
        assert_eq!(n, 0);
        assert_eq!(log.lock().unwrap().len(), before);

        unsafe { info.close_fn.unwrap()(info.cookie) };
        assert_eq!(log.lock().unwrap().last().unwrap(), "close");
        unregister::<Fake>(id);
    }

    #[test]
    fn callbacks_after_close_find_nothing() {
        let (id, user_data, log) = setup(102, b"abc");
        let (_, info) = open(user_data, c"fake://b");
        unsafe { info.close_fn.unwrap()(info.cookie) };
        let calls = log.lock().unwrap().len();
        unsafe {
            // the window mpv leaves between close_fn and freeing its mp_cancel
            info.cancel_fn.unwrap()(info.cookie);
            // and calls mpv never makes after close, refused all the same
            info.close_fn.unwrap()(info.cookie);
            let mut b = [0u8; 4];
            assert_eq!(
                info.read_fn.unwrap()(info.cookie, b.as_mut_ptr().cast(), 4),
                -1
            );
            assert_eq!(
                info.seek_fn.unwrap()(info.cookie, 0),
                i64::from(MPV_ERROR_GENERIC)
            );
            assert_eq!(
                info.size_fn.unwrap()(info.cookie),
                i64::from(MPV_ERROR_UNSUPPORTED)
            );
        }
        assert_eq!(
            log.lock().unwrap().len(),
            calls,
            "nothing reached the stream"
        );
        unregister::<Fake>(id);
    }

    #[test]
    fn cancel_reaches_an_open_stream_from_another_thread() {
        let (id, user_data, log) = setup(103, b"abc");
        let (_, info) = open(user_data, c"fake://c");
        let cookie = info.cookie as usize;
        let cancel = info.cancel_fn.unwrap();
        std::thread::spawn(move || unsafe { cancel(cookie as *mut c_void) })
            .join()
            .unwrap();
        assert_eq!(log.lock().unwrap().last().unwrap(), "cancel");
        unsafe { info.close_fn.unwrap()(info.cookie) };
        unregister::<Fake>(id);
    }

    #[test]
    fn cookies_are_never_reused() {
        let (id, user_data, _) = setup(104, b"x");
        let (_, a) = open(user_data, c"fake://d");
        unsafe { a.close_fn.unwrap()(a.cookie) };
        let (_, b) = open(user_data, c"fake://e");
        assert_ne!(a.cookie, b.cookie);
        unsafe { b.close_fn.unwrap()(b.cookie) };
        unregister::<Fake>(id);
    }

    #[test]
    fn open_failures_are_loading_failed() {
        let (id, user_data, _) = setup(105, b"");
        let (r, info) = open(user_data, c"fake://missing");
        assert_eq!(r, MPV_ERROR_LOADING_FAILED);
        assert!(info.read_fn.is_none(), "info untouched on failure");
        // a panic does not cross into mpv
        assert_eq!(open(user_data, c"fake://panic").0, MPV_ERROR_LOADING_FAILED);
        assert!(
            !in_callback_of(105),
            "the mark is cleared after a panic too"
        );
        // an empty stream has no known size
        let (_, info) = open(user_data, c"fake://empty");
        assert_eq!(
            unsafe { info.size_fn.unwrap()(info.cookie) },
            i64::from(MPV_ERROR_UNSUPPORTED)
        );
        unsafe { info.close_fn.unwrap()(info.cookie) };
        unregister::<Fake>(id);
        // once unregistered, the protocol opens nothing
        assert_eq!(open(user_data, c"fake://f").0, MPV_ERROR_LOADING_FAILED);
    }

    #[test]
    fn short_and_oversized_reads() {
        struct Liar;
        impl Stream for Liar {
            fn owner(&self) -> usize {
                106
            }
            fn read(&self, buf: &mut [u8]) -> i64 {
                buf.len() as i64 + 1 // claims more than fits
            }
            fn seek(&self, _: i64) -> i64 {
                i64::from(MPV_ERROR_UNSUPPORTED)
            }
            fn size(&self) -> i64 {
                -5
            }
            fn close(&self) {}
            fn cancel(&self) {}
        }
        static LIARS: Registry<Liar> = Registry::new();
        static LIAR_PROTOCOLS: Registry<LiarProtocol> = Registry::new();
        struct LiarProtocol;
        impl Backend for LiarProtocol {
            type Stream = Liar;
            fn protocols() -> &'static Registry<Self> {
                &LIAR_PROTOCOLS
            }
            fn streams() -> &'static Registry<Liar> {
                &LIARS
            }
            fn owner(&self) -> usize {
                106
            }
            fn open(&self, _: &CStr) -> Option<Liar> {
                Some(Liar)
            }
        }
        let (id, user_data, _) = register(LiarProtocol).unwrap();
        let mut info = empty_info();
        let r = unsafe {
            open_fn::<LiarProtocol>(user_data, c"liar://".as_ptr().cast_mut(), &raw mut info)
        };
        assert_eq!(r, 0);
        unsafe {
            let mut b = [0u8; 8];
            assert_eq!(
                info.read_fn.unwrap()(info.cookie, b.as_mut_ptr().cast(), 8),
                -1
            );
            // "not seekable" passes through, so mpv treats the stream as such
            assert_eq!(
                info.seek_fn.unwrap()(info.cookie, 0),
                i64::from(MPV_ERROR_UNSUPPORTED)
            );
            assert_eq!(
                info.size_fn.unwrap()(info.cookie),
                i64::from(MPV_ERROR_UNSUPPORTED)
            );
            info.close_fn.unwrap()(info.cookie);
        }
        unregister::<LiarProtocol>(id);
    }

    #[test]
    fn the_callback_mark_nests_and_is_per_owner() {
        assert!(!in_callback_of(0));
        let outer = Scope::enter(7);
        assert!(in_callback_of(7));
        assert!(!in_callback_of(8));
        {
            let _inner = Scope::enter(8);
            assert!(in_callback_of(8) && !in_callback_of(7));
        }
        assert!(in_callback_of(7));
        drop(outer);
        assert!(!in_callback_of(7));
        // other threads are not marked
        let _s = Scope::enter(9);
        assert!(!std::thread::spawn(|| in_callback_of(9)).join().unwrap());
    }
}
