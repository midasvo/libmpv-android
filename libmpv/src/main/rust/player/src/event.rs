//! Decoding of `mpv_event`s into what the event thread delivers to Kotlin, and
//! the logcat side of log messages.

use std::ffi::{CStr, c_char, c_int};

use crate::ffi::*;

#[derive(Debug, PartialEq)]
pub enum Event<'a> {
    /// `MPVLib.logMessage(prefix, level, text)`, after logcat
    Log {
        prefix: &'a CStr,
        level: &'a CStr,
        text: &'a CStr,
        log_level: c_int,
    },
    /// `MPVLib.eventProperty(name[, value])`
    Property { name: &'a CStr, value: Value<'a> },
    /// `MPVLib.eventEndFile(reason, error)`; Kotlin also delivers
    /// `event(MPV_EVENT_END_FILE)` from there
    EndFile { reason: c_int, error: c_int },
    /// `MPVLib.event(id)`
    Other(mpv_event_id),
}

#[derive(Debug, PartialEq)]
pub enum Value<'a> {
    None,
    Flag(bool),
    Int64(i64),
    Double(f64),
    String(&'a CStr),
    /// a format MPVLib has no callback for (OSD string, node, byte array): not delivered
    Unsupported(mpv_format),
}

unsafe fn c_str<'a>(p: *const c_char) -> Option<&'a CStr> {
    // SAFETY: the caller passes null or a NUL-terminated string valid for 'a
    (!p.is_null()).then(|| unsafe { CStr::from_ptr(p) })
}

/// Decodes `ev`. `None` for `MPV_EVENT_NONE` (a wakeup or timeout) and for
/// events whose data mpv left out, which it never does for these event types.
///
/// # Safety
/// `ev` must be an event as returned by `mpv_wait_event`, valid (with
/// everything it points to) for `'a`, that is until the next `mpv_wait_event`.
pub unsafe fn decode<'a>(ev: &'a mpv_event) -> Option<Event<'a>> {
    // SAFETY (whole function): mpv documents which struct `data` points to for
    // each event_id; the strings in them are NUL-terminated.
    unsafe {
        match ev.event_id {
            MPV_EVENT_NONE => None,
            MPV_EVENT_LOG_MESSAGE => {
                let msg = ev.data.cast::<mpv_event_log_message>().as_ref()?;
                Some(Event::Log {
                    prefix: c_str(msg.prefix).unwrap_or(c""),
                    level: c_str(msg.level).unwrap_or(c""),
                    text: c_str(msg.text).unwrap_or(c""),
                    log_level: msg.log_level,
                })
            }
            MPV_EVENT_PROPERTY_CHANGE => {
                let prop = ev.data.cast::<mpv_event_property>().as_ref()?;
                let name = c_str(prop.name)?;
                let data = prop.data;
                let value = if data.is_null() {
                    // mpv sets format NONE whenever data is null
                    Value::None
                } else {
                    match prop.format {
                        MPV_FORMAT_NONE => Value::None,
                        MPV_FORMAT_FLAG => Value::Flag(*data.cast::<c_int>() != 0),
                        MPV_FORMAT_INT64 => Value::Int64(*data.cast::<i64>()),
                        MPV_FORMAT_DOUBLE => Value::Double(*data.cast::<f64>()),
                        MPV_FORMAT_STRING => match c_str(*data.cast::<*const c_char>()) {
                            Some(s) => Value::String(s),
                            None => Value::None,
                        },
                        other => Value::Unsupported(other),
                    }
                };
                Some(Event::Property { name, value })
            }
            MPV_EVENT_END_FILE => {
                let end = ev.data.cast::<mpv_event_end_file>().as_ref()?;
                Some(Event::EndFile {
                    reason: end.reason,
                    error: end.error,
                })
            }
            id => Some(Event::Other(id)),
        }
    }
}

/// The logcat priority for an mpv log level, so messages keep their severity.
pub fn android_priority(log_level: c_int) -> c_int {
    match log_level {
        MPV_LOG_LEVEL_FATAL => ANDROID_LOG_FATAL,
        MPV_LOG_LEVEL_ERROR => ANDROID_LOG_ERROR,
        MPV_LOG_LEVEL_WARN => ANDROID_LOG_WARN,
        MPV_LOG_LEVEL_INFO => ANDROID_LOG_INFO,
        MPV_LOG_LEVEL_V => ANDROID_LOG_DEBUG,
        _ => ANDROID_LOG_VERBOSE, // debug, trace, unknown
    }
}

/// The logcat line for an mpv message: `[prefix:level] text`. Raw bytes, as
/// mpv sent them: logcat does not require valid UTF-8.
pub fn logcat_line(prefix: &CStr, level: &CStr, text: &CStr) -> Vec<u8> {
    let (p, l, t) = (prefix.to_bytes(), level.to_bytes(), text.to_bytes());
    let mut line = Vec::with_capacity(p.len() + l.len() + t.len() + 5);
    line.push(b'[');
    line.extend_from_slice(p);
    line.push(b':');
    line.extend_from_slice(l);
    line.extend_from_slice(b"] ");
    line.extend_from_slice(t);
    line
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::c_void;
    use std::ptr;

    fn event(event_id: mpv_event_id, data: *mut c_void) -> mpv_event {
        mpv_event {
            event_id,
            error: 0,
            reply_userdata: 0,
            data,
        }
    }

    fn property(name: &CStr, format: mpv_format, data: *mut c_void) -> mpv_event_property {
        mpv_event_property {
            name: name.as_ptr(),
            format,
            data,
        }
    }

    fn decode_property(mut prop: mpv_event_property) -> Option<(String, String)> {
        let ev = event(MPV_EVENT_PROPERTY_CHANGE, (&raw mut prop).cast());
        match unsafe { decode(&ev) }? {
            Event::Property { name, value } => {
                Some((name.to_string_lossy().into_owned(), format!("{value:?}")))
            }
            other => panic!("decoded as {other:?}"),
        }
    }

    fn decode_plain(id: mpv_event_id, data: *mut c_void) -> Option<String> {
        let ev = event(id, data);
        unsafe { decode(&ev) }.map(|e| format!("{e:?}"))
    }

    #[test]
    fn none_is_skipped() {
        assert_eq!(decode_plain(MPV_EVENT_NONE, ptr::null_mut()), None);
    }

    #[test]
    fn plain_events_keep_their_id() {
        for id in [1, 6, 8, 11, 16, 17, 18, 20, 21, 24, 25, 99] {
            assert_eq!(
                decode_plain(id, ptr::null_mut()),
                Some(format!("{:?}", Event::Other(id)))
            );
        }
    }

    #[test]
    fn end_file_carries_reason_and_error() {
        let mut end = mpv_event_end_file {
            reason: 4,
            error: -13,
            playlist_entry_id: 1,
            playlist_insert_id: 0,
            playlist_insert_num_entries: 0,
        };
        let ev = event(MPV_EVENT_END_FILE, (&raw mut end).cast());
        assert_eq!(
            unsafe { decode(&ev) },
            Some(Event::EndFile {
                reason: 4,
                error: -13
            })
        );
        // never sent like this, but must not crash
        assert_eq!(decode_plain(MPV_EVENT_END_FILE, ptr::null_mut()), None);
    }

    #[test]
    fn log_message() {
        let mut msg = mpv_event_log_message {
            prefix: c"cplayer".as_ptr(),
            level: c"v".as_ptr(),
            text: c"hello\n".as_ptr(),
            log_level: MPV_LOG_LEVEL_V,
        };
        let ev = event(MPV_EVENT_LOG_MESSAGE, (&raw mut msg).cast());
        assert_eq!(
            unsafe { decode(&ev) },
            Some(Event::Log {
                prefix: c"cplayer",
                level: c"v",
                text: c"hello\n",
                log_level: 50
            })
        );
        msg.text = ptr::null();
        let ev = event(MPV_EVENT_LOG_MESSAGE, (&raw mut msg).cast());
        assert!(matches!(unsafe { decode(&ev) }, Some(Event::Log { text, .. }) if text.is_empty()));
    }

    #[test]
    fn property_values() {
        let mut flag: c_int = 1;
        let mut zero: c_int = 0;
        let mut int: i64 = 6_000_000_000;
        let mut double: f64 = 1.75;
        let mut string: *const c_char = c"Ünïcödé 🎵".as_ptr();
        let mut null_string: *const c_char = ptr::null();
        let p = |f, d: *mut c_void| decode_property(property(c"volume", f, d)).unwrap().1;

        assert_eq!(p(MPV_FORMAT_NONE, ptr::null_mut()), "None");
        assert_eq!(p(MPV_FORMAT_FLAG, (&raw mut flag).cast()), "Flag(true)");
        assert_eq!(p(MPV_FORMAT_FLAG, (&raw mut zero).cast()), "Flag(false)");
        assert_eq!(
            p(MPV_FORMAT_INT64, (&raw mut int).cast()),
            "Int64(6000000000)"
        );
        assert_eq!(
            p(MPV_FORMAT_DOUBLE, (&raw mut double).cast()),
            "Double(1.75)"
        );
        assert_eq!(
            p(MPV_FORMAT_STRING, (&raw mut string).cast()),
            format!("String({:?})", c"Ünïcödé 🎵")
        );
        assert_eq!(p(MPV_FORMAT_STRING, (&raw mut null_string).cast()), "None");
        // unavailable: mpv sends format NONE with null data; a null with
        // another format is treated the same instead of dereferenced
        assert_eq!(p(MPV_FORMAT_DOUBLE, ptr::null_mut()), "None");
        // OSD string and node have no MPVLib callback
        assert_eq!(p(2, (&raw mut string).cast()), "Unsupported(2)");
        assert_eq!(p(6, (&raw mut int).cast()), "Unsupported(6)");
    }

    #[test]
    fn property_name_is_kept() {
        let name = decode_property(property(c"sub-text", MPV_FORMAT_NONE, ptr::null_mut()))
            .unwrap()
            .0;
        assert_eq!(name, "sub-text");
        let mut no_name = property(c"x", MPV_FORMAT_NONE, ptr::null_mut());
        no_name.name = ptr::null();
        assert_eq!(decode_property(no_name), None);
    }

    #[test]
    fn priorities() {
        assert_eq!(android_priority(10), ANDROID_LOG_FATAL);
        assert_eq!(android_priority(20), ANDROID_LOG_ERROR);
        assert_eq!(android_priority(30), ANDROID_LOG_WARN);
        assert_eq!(android_priority(40), ANDROID_LOG_INFO);
        assert_eq!(android_priority(50), ANDROID_LOG_DEBUG);
        assert_eq!(android_priority(60), ANDROID_LOG_VERBOSE);
        assert_eq!(android_priority(70), ANDROID_LOG_VERBOSE);
        assert_eq!(android_priority(0), ANDROID_LOG_VERBOSE);
        assert_eq!(android_priority(-1), ANDROID_LOG_VERBOSE);
    }

    #[test]
    fn logcat_line_format() {
        assert_eq!(
            logcat_line(c"cplayer", c"info", c"Playing: x\n"),
            b"[cplayer:info] Playing: x\n"
        );
        // invalid UTF-8 passes through to logcat unchanged
        assert_eq!(logcat_line(c"a", c"b", c"\xff"), b"[a:b] \xff");
    }
}
