//! The `stream::Backend` behind `MPVLib.registerStreamProtocol`: each callback
//! calls the Kotlin adapters in `dev.jdtech.mpv.internal` (ProtocolAdapter,
//! StreamAdapter), which call the app's provider and translate its results
//! and exceptions into mpv's conventions.
//!
//! Threads: mpv calls these on its own threads (demuxer and stream threads,
//! and whichever thread cancels). `Vm::with_thread_env` attaches such a
//! thread on its first callback and keeps it attached until the thread exits,
//! so a stream's reads cost no attach each. The adapters catch what the
//! provider throws; whatever still reaches here (an Error thrown by the
//! adapter itself) is logged and cleared, never left pending, and the call
//! fails.
//!
//! Reads: each stream has one `byte[]` (StreamAdapter.buffer), allocated at
//! open. The provider reads into it and one `GetByteArrayRegion` copies the
//! bytes into mpv's buffer: no allocation per read and one copy. Reading
//! straight into mpv's memory (a direct ByteBuffer over it) would save that
//! copy, but the provider could keep the ByteBuffer and write through it
//! after mpv has freed the memory; Java code must not be able to do that.

use std::ffi::CStr;

use jni_sys::{jint, jobject, jsize, jvalue};

use crate::instance::{self, Failure};
use crate::jni::{Env, Global, Pending, Vm};
use crate::log;
use crate::registry::Registry;
use crate::stream::{Backend, Stream};

static PROTOCOLS: Registry<JniProtocol> = Registry::new();
static STREAMS: Registry<JniStream> = Registry::new();

pub struct JniProtocol {
    vm: Vm,
    adapter: Global,
    owner: usize,
}

pub struct JniStream {
    vm: Vm,
    adapter: Global,
    buffer: Global,
    capacity: jsize,
    owner: usize,
}

impl JniProtocol {
    pub fn new(env: &Env, vm: Vm, adapter: jobject, owner: usize) -> instance::Result<JniProtocol> {
        let adapter = env.new_global(vm, adapter).ok_or_else(|| {
            if env.exception_pending() {
                Failure::Pending
            } else {
                Failure::runtime("stream provider is null")
            }
        })?;
        Ok(JniProtocol { vm, adapter, owner })
    }

    fn open_in(&self, env: &Env, uri: &CStr) -> Option<JniStream> {
        let m = instance::methods()?;
        let uri = env.new_string(uri).ok()?;
        let adapter = env
            .call_object(
                self.adapter.as_raw(),
                m.protocol_open,
                &[jvalue { l: uri.as_raw() }],
            )
            .ok()??;
        let built = (|| -> Result<JniStream, Pending> {
            let buffer = env
                .call_object(adapter.as_raw(), m.stream_buffer, &[])?
                .ok_or(Pending)?;
            let capacity = env.array_length(buffer.as_raw())?;
            if capacity < 1 {
                return Err(Pending);
            }
            Ok(JniStream {
                vm: self.vm,
                adapter: env.new_global(self.vm, adapter.as_raw()).ok_or(Pending)?,
                buffer: env.new_global(self.vm, buffer.as_raw()).ok_or(Pending)?,
                capacity,
                owner: self.owner,
            })
        })();
        if built.is_err() {
            // the provider opened its stream: close it, since mpv never will
            clear(env, "opening a stream");
            env.call_void(adapter.as_raw(), m.stream_close, &[]);
            clear(env, "closing a stream that could not be opened");
        }
        built.ok()
    }
}

/// Logs and clears a pending exception.
fn clear(env: &Env, what: &str) {
    if env.exception_pending() {
        log::error(&format!("exception while {what} (cleared):"));
        env.describe_and_clear_exception();
    }
}

impl Backend for JniProtocol {
    type Stream = JniStream;

    fn protocols() -> &'static Registry<Self> {
        &PROTOCOLS
    }

    fn streams() -> &'static Registry<JniStream> {
        &STREAMS
    }

    fn owner(&self) -> usize {
        self.owner
    }

    fn open(&self, uri: &CStr) -> Option<JniStream> {
        self.vm
            .with_thread_env(|env| {
                let stream = self.open_in(env, uri);
                clear(env, "opening a stream");
                stream
            })
            .flatten()
    }
}

impl JniStream {
    fn call_long(
        &self,
        method: fn(&instance::Methods) -> crate::jni::MethodId,
        args: &[jvalue],
        failed: i64,
    ) -> i64 {
        let Some(m) = instance::methods() else {
            return failed;
        };
        self.vm
            .with_thread_env(
                |env| match env.call_long(self.adapter.as_raw(), method(m), args) {
                    Ok(r) => r,
                    Err(_) => {
                        clear(env, "calling a stream");
                        failed
                    }
                },
            )
            .unwrap_or(failed)
    }

    fn call_void(&self, method: fn(&instance::Methods) -> crate::jni::MethodId) {
        let Some(m) = instance::methods() else {
            return;
        };
        self.vm.with_thread_env(|env| {
            env.call_void(self.adapter.as_raw(), method(m), &[]);
            clear(env, "calling a stream");
        });
    }
}

impl Stream for JniStream {
    fn owner(&self) -> usize {
        self.owner
    }

    fn read(&self, buf: &mut [u8]) -> i64 {
        let Some(m) = instance::methods() else {
            return -1;
        };
        // at most what fits in both buffers; mpv reads the rest next time
        let n = jint::try_from(buf.len())
            .unwrap_or(jint::MAX)
            .min(self.capacity);
        self.vm
            .with_thread_env(|env| {
                let r = match env.call_int(self.adapter.as_raw(), m.stream_read, &[jvalue { i: n }])
                {
                    Ok(r) => r,
                    Err(_) => {
                        clear(env, "reading a stream");
                        return -1;
                    }
                };
                if r <= 0 {
                    return i64::from(r.max(-1)); // 0 at the end, -1 on error
                }
                if r > n {
                    return -1; // the adapter checks this; never trust a count blindly
                }
                if env
                    .byte_array_region(self.buffer.as_raw(), r, buf.as_mut_ptr())
                    .is_err()
                {
                    clear(env, "copying a stream's data");
                    return -1;
                }
                i64::from(r)
            })
            .unwrap_or(-1)
    }

    fn seek(&self, offset: i64) -> i64 {
        self.call_long(|m| m.stream_seek, &[jvalue { j: offset }], -1)
    }

    fn size(&self) -> i64 {
        self.call_long(|m| m.stream_size, &[], -1)
    }

    fn close(&self) {
        self.call_void(|m| m.stream_close);
    }

    fn cancel(&self) {
        self.call_void(|m| m.stream_cancel);
    }
}
