//! `libplayer.so`: the JNI side of `dev.jdtech.mpv.MPVLib`.
//!
//! Modules that need neither JNI nor mpv (`text`, `registry`, `lifecycle`,
//! `event`) are unit-tested on the host with `cargo test`. The rest calls into
//! libmpv, libavcodec and liblog, which exist only on Android, so it is left
//! out of the host test build (it is still type-checked and linted by
//! `cargo check`/`cargo clippy`, which do not link).

mod event;
// what only the Android-only modules use is unused in the host test build
#[cfg_attr(test, allow(dead_code))]
mod ffi;
mod lifecycle;
mod registry;
mod text;

#[cfg(not(test))]
mod exports;
#[cfg(not(test))]
mod instance;
#[cfg(not(test))]
mod jni;
#[cfg(not(test))]
mod log;
