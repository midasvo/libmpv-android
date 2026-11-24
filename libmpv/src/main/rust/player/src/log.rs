//! logcat, under the tag `mpv` (as the C++ wrapper used).

use std::ffi::{CString, c_int};

use crate::ffi::{__android_log_write, ANDROID_LOG_ERROR, ANDROID_LOG_VERBOSE};

/// The C++ wrapper was built with DEBUG 1, so its verbose messages (one per
/// event) were in logcat; keep them.
const VERBOSE: bool = true;

pub fn write(priority: c_int, message: &[u8]) {
    let message: Vec<u8> = message
        .iter()
        .map(|&b| if b == 0 { b'?' } else { b })
        .collect();
    let message = CString::new(message).unwrap_or_default();
    // SAFETY: both strings are NUL-terminated and outlive the call
    unsafe { __android_log_write(priority, c"mpv".as_ptr(), message.as_ptr()) };
}

pub fn error(message: &str) {
    write(ANDROID_LOG_ERROR, message.as_bytes());
}

pub fn verbose(message: impl FnOnce() -> String) {
    if VERBOSE {
        write(ANDROID_LOG_VERBOSE, message().as_bytes());
    }
}
