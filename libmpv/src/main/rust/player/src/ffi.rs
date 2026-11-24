//! The parts of the C APIs this library uses, declared by hand.
//!
//! Checked against mpv 0.42.0's `mpv/client.h` (client API 2.6) and
//! `mpv/stream_cb.h`, FFmpeg's `libavcodec/jni.h`, the NDK's `android/log.h`
//! and bionic's `locale.h`. C enums (`mpv_format`, `mpv_event_id`,
//! `mpv_log_level`, `mpv_end_file_reason`) are `int` in these ABIs and are
//! declared as `c_int`, so a value this file does not know is still
//! representable.

#![allow(non_camel_case_types)]

use std::ffi::{c_char, c_int, c_void};

/// `mpv_handle`, opaque.
#[repr(C)]
pub struct mpv_handle {
    _private: [u8; 0],
}

pub type mpv_format = c_int;
pub const MPV_FORMAT_NONE: mpv_format = 0;
pub const MPV_FORMAT_STRING: mpv_format = 1;
pub const MPV_FORMAT_OSD_STRING: mpv_format = 2;
pub const MPV_FORMAT_FLAG: mpv_format = 3;
pub const MPV_FORMAT_INT64: mpv_format = 4;
pub const MPV_FORMAT_DOUBLE: mpv_format = 5;
pub const MPV_FORMAT_NODE: mpv_format = 6;
pub const MPV_FORMAT_NODE_ARRAY: mpv_format = 7;
pub const MPV_FORMAT_NODE_MAP: mpv_format = 8;
pub const MPV_FORMAT_BYTE_ARRAY: mpv_format = 9;

pub type mpv_event_id = c_int;
pub const MPV_EVENT_NONE: mpv_event_id = 0;
pub const MPV_EVENT_LOG_MESSAGE: mpv_event_id = 2;
pub const MPV_EVENT_GET_PROPERTY_REPLY: mpv_event_id = 3;
pub const MPV_EVENT_SET_PROPERTY_REPLY: mpv_event_id = 4;
pub const MPV_EVENT_COMMAND_REPLY: mpv_event_id = 5;
pub const MPV_EVENT_START_FILE: mpv_event_id = 6;
pub const MPV_EVENT_END_FILE: mpv_event_id = 7;
pub const MPV_EVENT_CLIENT_MESSAGE: mpv_event_id = 16;
pub const MPV_EVENT_PROPERTY_CHANGE: mpv_event_id = 22;

pub const MPV_ERROR_NOMEM: c_int = -2;
pub const MPV_ERROR_UNINITIALIZED: c_int = -3;
pub const MPV_ERROR_INVALID_PARAMETER: c_int = -4;
pub const MPV_ERROR_LOADING_FAILED: c_int = -13;
pub const MPV_ERROR_UNSUPPORTED: c_int = -18;
pub const MPV_ERROR_GENERIC: c_int = -20;

pub const MPV_LOG_LEVEL_FATAL: c_int = 10;
pub const MPV_LOG_LEVEL_ERROR: c_int = 20;
pub const MPV_LOG_LEVEL_WARN: c_int = 30;
pub const MPV_LOG_LEVEL_INFO: c_int = 40;
pub const MPV_LOG_LEVEL_V: c_int = 50;

#[repr(C)]
pub struct mpv_event {
    pub event_id: mpv_event_id,
    pub error: c_int,
    pub reply_userdata: u64,
    pub data: *mut c_void,
}

#[repr(C)]
pub struct mpv_event_property {
    pub name: *const c_char,
    pub format: mpv_format,
    pub data: *mut c_void,
}

#[repr(C)]
pub struct mpv_event_log_message {
    pub prefix: *const c_char,
    pub level: *const c_char,
    pub text: *const c_char,
    pub log_level: c_int,
}

#[repr(C)]
pub struct mpv_event_start_file {
    pub playlist_entry_id: i64,
}

#[repr(C)]
pub struct mpv_event_end_file {
    pub reason: c_int,
    pub error: c_int,
    pub playlist_entry_id: i64,
    pub playlist_insert_id: i64,
    pub playlist_insert_num_entries: c_int,
}

#[repr(C)]
pub struct mpv_event_client_message {
    pub num_args: c_int,
    pub args: *mut *const c_char,
}

#[repr(C)]
pub struct mpv_event_command {
    pub result: mpv_node,
}

/// The union in `mpv_node`: which member is valid depends on `format`.
#[repr(C)]
#[derive(Clone, Copy)]
pub union mpv_node_u {
    pub string: *mut c_char,
    pub flag: c_int,
    pub int64: i64,
    pub double_: f64,
    pub list: *mut mpv_node_list,
    pub ba: *mut mpv_byte_array,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct mpv_node {
    pub u: mpv_node_u,
    pub format: mpv_format,
}

impl mpv_node {
    pub const NONE: mpv_node = mpv_node {
        u: mpv_node_u { int64: 0 },
        format: MPV_FORMAT_NONE,
    };
}

#[repr(C)]
pub struct mpv_node_list {
    pub num: c_int,
    pub values: *mut mpv_node,
    /// null for arrays
    pub keys: *mut *mut c_char,
}

#[repr(C)]
pub struct mpv_byte_array {
    pub data: *mut c_void,
    pub size: usize,
}

// mpv/stream_cb.h; the callbacks are nullable function pointers in C
pub type mpv_stream_cb_read_fn =
    Option<unsafe extern "C" fn(cookie: *mut c_void, buf: *mut c_char, nbytes: u64) -> i64>;
pub type mpv_stream_cb_seek_fn =
    Option<unsafe extern "C" fn(cookie: *mut c_void, offset: i64) -> i64>;
pub type mpv_stream_cb_size_fn = Option<unsafe extern "C" fn(cookie: *mut c_void) -> i64>;
pub type mpv_stream_cb_close_fn = Option<unsafe extern "C" fn(cookie: *mut c_void)>;
pub type mpv_stream_cb_cancel_fn = Option<unsafe extern "C" fn(cookie: *mut c_void)>;

#[repr(C)]
pub struct mpv_stream_cb_info {
    pub cookie: *mut c_void,
    pub read_fn: mpv_stream_cb_read_fn,
    pub seek_fn: mpv_stream_cb_seek_fn,
    pub size_fn: mpv_stream_cb_size_fn,
    pub close_fn: mpv_stream_cb_close_fn,
    pub cancel_fn: mpv_stream_cb_cancel_fn,
}

pub type mpv_stream_cb_open_ro_fn = Option<
    unsafe extern "C" fn(
        user_data: *mut c_void,
        uri: *mut c_char,
        info: *mut mpv_stream_cb_info,
    ) -> c_int,
>;

// android/log.h, android_LogPriority
pub const ANDROID_LOG_VERBOSE: c_int = 2;
pub const ANDROID_LOG_DEBUG: c_int = 3;
pub const ANDROID_LOG_INFO: c_int = 4;
pub const ANDROID_LOG_WARN: c_int = 5;
pub const ANDROID_LOG_ERROR: c_int = 6;
pub const ANDROID_LOG_FATAL: c_int = 7;

// bionic locale.h
#[cfg(not(test))]
pub const LC_NUMERIC: c_int = 1;

// Linked by build.rs (only for Android); unit tests on the host use none of it.
#[cfg(not(test))]
unsafe extern "C" {
    pub fn mpv_error_string(error: c_int) -> *const c_char;
    pub fn mpv_free(data: *mut c_void);
    pub fn mpv_create() -> *mut mpv_handle;
    pub fn mpv_initialize(ctx: *mut mpv_handle) -> c_int;
    pub fn mpv_terminate_destroy(ctx: *mut mpv_handle);
    pub fn mpv_set_option(
        ctx: *mut mpv_handle,
        name: *const c_char,
        format: mpv_format,
        data: *mut c_void,
    ) -> c_int;
    pub fn mpv_set_option_string(
        ctx: *mut mpv_handle,
        name: *const c_char,
        data: *const c_char,
    ) -> c_int;
    pub fn mpv_command(ctx: *mut mpv_handle, args: *mut *const c_char) -> c_int;
    pub fn mpv_command_node(
        ctx: *mut mpv_handle,
        args: *mut mpv_node,
        result: *mut mpv_node,
    ) -> c_int;
    pub fn mpv_command_node_async(
        ctx: *mut mpv_handle,
        reply_userdata: u64,
        args: *mut mpv_node,
    ) -> c_int;
    pub fn mpv_abort_async_command(ctx: *mut mpv_handle, reply_userdata: u64);
    pub fn mpv_set_property(
        ctx: *mut mpv_handle,
        name: *const c_char,
        format: mpv_format,
        data: *mut c_void,
    ) -> c_int;
    pub fn mpv_set_property_async(
        ctx: *mut mpv_handle,
        reply_userdata: u64,
        name: *const c_char,
        format: mpv_format,
        data: *mut c_void,
    ) -> c_int;
    pub fn mpv_get_property(
        ctx: *mut mpv_handle,
        name: *const c_char,
        format: mpv_format,
        data: *mut c_void,
    ) -> c_int;
    pub fn mpv_get_property_async(
        ctx: *mut mpv_handle,
        reply_userdata: u64,
        name: *const c_char,
        format: mpv_format,
    ) -> c_int;
    pub fn mpv_free_node_contents(node: *mut mpv_node);
    pub fn mpv_observe_property(
        mpv: *mut mpv_handle,
        reply_userdata: u64,
        name: *const c_char,
        format: mpv_format,
    ) -> c_int;
    pub fn mpv_unobserve_property(mpv: *mut mpv_handle, registered_reply_userdata: u64) -> c_int;
    pub fn mpv_event_name(event: mpv_event_id) -> *const c_char;
    pub fn mpv_request_log_messages(ctx: *mut mpv_handle, min_level: *const c_char) -> c_int;
    pub fn mpv_wait_event(ctx: *mut mpv_handle, timeout: f64) -> *mut mpv_event;
    pub fn mpv_wakeup(ctx: *mut mpv_handle);

    // mpv/stream_cb.h
    pub fn mpv_stream_cb_add_ro(
        ctx: *mut mpv_handle,
        protocol: *const c_char,
        user_data: *mut c_void,
        open_fn: mpv_stream_cb_open_ro_fn,
    ) -> c_int;

    // libavcodec/jni.h
    pub fn av_jni_set_java_vm(vm: *mut c_void, log_ctx: *mut c_void) -> c_int;
    pub fn av_jni_set_android_app_ctx(app_ctx: *mut c_void, log_ctx: *mut c_void) -> c_int;

    // android/log.h
    pub fn __android_log_write(prio: c_int, tag: *const c_char, text: *const c_char) -> c_int;

    // locale.h
    pub fn setlocale(category: c_int, locale: *const c_char) -> *mut c_char;
}
