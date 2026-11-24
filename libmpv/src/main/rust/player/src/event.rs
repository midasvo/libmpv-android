//! Decoding of `mpv_event`s into what the event thread delivers to Kotlin, and
//! the logcat side of log messages.

use std::ffi::{CStr, c_char, c_int};
use std::fmt;

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
    /// A change of an observed property. `userdata` says whose observation it
    /// is: 0 or negative for `MPVLib.observeProperty` (delivered to
    /// `eventProperty(name[, value])`), positive for a property flow.
    Property {
        userdata: u64,
        name: &'a CStr,
        value: Value<'a>,
    },
    /// `MPVLib.event(MPV_EVENT_START_FILE)`; the id goes to the event flow
    StartFile {
        playlist_entry_id: i64,
    },
    /// `MPVLib.eventEndFile(reason, error)`; Kotlin also delivers
    /// `event(MPV_EVENT_END_FILE)` from there
    EndFile(EndFile),
    /// `MPVLib.event(MPV_EVENT_CLIENT_MESSAGE)`; the arguments go to the event flow
    ClientMessage(Vec<&'a CStr>),
    /// replies to the asynchronous calls, only for the requests' owner
    GetPropertyReply {
        userdata: u64,
        error: c_int,
        value: Value<'a>,
    },
    SetPropertyReply {
        userdata: u64,
        error: c_int,
    },
    CommandReply {
        userdata: u64,
        error: c_int,
        result: Option<NodeRef<'a>>,
    },
    /// `MPVLib.event(id)`
    Other(mpv_event_id),
}

#[derive(Debug, PartialEq, Clone, Copy)]
pub struct EndFile {
    pub reason: c_int,
    pub error: c_int,
    pub playlist_entry_id: i64,
    pub playlist_insert_id: i64,
    pub playlist_insert_num_entries: c_int,
}

#[derive(Debug, PartialEq)]
pub enum Value<'a> {
    None,
    Flag(bool),
    Int64(i64),
    Double(f64),
    String(&'a CStr),
    Node(NodeRef<'a>),
    /// a format with no MPVLib callback (OSD string, byte array): not delivered
    Unsupported(mpv_format),
}

impl Value<'_> {
    /// The value as an `mpv_node`, pointing into the event (a string or a node
    /// is not copied), so that it can be encoded like a node: a get-property
    /// reply in a scalar format reaches Kotlin as the matching scalar node.
    /// `Unsupported` is `MPV_FORMAT_NONE`.
    pub fn as_node(&self) -> mpv_node {
        match *self {
            Value::None | Value::Unsupported(_) => mpv_node::NONE,
            Value::Flag(z) => mpv_node {
                u: mpv_node_u {
                    flag: c_int::from(z),
                },
                format: MPV_FORMAT_FLAG,
            },
            Value::Int64(int64) => mpv_node {
                u: mpv_node_u { int64 },
                format: MPV_FORMAT_INT64,
            },
            Value::Double(double_) => mpv_node {
                u: mpv_node_u { double_ },
                format: MPV_FORMAT_DOUBLE,
            },
            // only read through: node::encode never writes to a node
            Value::String(s) => mpv_node {
                u: mpv_node_u {
                    string: s.as_ptr().cast_mut(),
                },
                format: MPV_FORMAT_STRING,
            },
            Value::Node(n) => *n.0,
        }
    }
}

/// An `mpv_node` inside an event; compared by address in tests.
#[derive(Clone, Copy)]
pub struct NodeRef<'a>(pub &'a mpv_node);

impl fmt::Debug for NodeRef<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Node(format {} at {:p})", self.0.format, self.0)
    }
}

impl PartialEq for NodeRef<'_> {
    fn eq(&self, other: &Self) -> bool {
        std::ptr::eq(self.0, other.0)
    }
}

unsafe fn c_str<'a>(p: *const c_char) -> Option<&'a CStr> {
    // SAFETY: the caller passes null or a NUL-terminated string valid for 'a
    (!p.is_null()).then(|| unsafe { CStr::from_ptr(p) })
}

/// The value in a property event or get-property reply.
///
/// # Safety
/// `prop` is as mpv sends it, valid for `'a`.
unsafe fn value<'a>(prop: &'a mpv_event_property) -> Value<'a> {
    let data = prop.data;
    if data.is_null() {
        // mpv sets format NONE whenever data is null
        return Value::None;
    }
    // SAFETY: `format` says what `data` points to
    unsafe {
        match prop.format {
            MPV_FORMAT_NONE => Value::None,
            MPV_FORMAT_FLAG => Value::Flag(*data.cast::<c_int>() != 0),
            MPV_FORMAT_INT64 => Value::Int64(*data.cast::<i64>()),
            MPV_FORMAT_DOUBLE => Value::Double(*data.cast::<f64>()),
            MPV_FORMAT_STRING => match c_str(*data.cast::<*const c_char>()) {
                Some(s) => Value::String(s),
                None => Value::None,
            },
            MPV_FORMAT_NODE => Value::Node(NodeRef(&*data.cast::<mpv_node>())),
            other => Value::Unsupported(other),
        }
    }
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
                Some(Event::Property {
                    userdata: ev.reply_userdata,
                    name,
                    value: value(prop),
                })
            }
            MPV_EVENT_START_FILE => {
                let start = ev.data.cast::<mpv_event_start_file>().as_ref()?;
                Some(Event::StartFile {
                    playlist_entry_id: start.playlist_entry_id,
                })
            }
            MPV_EVENT_END_FILE => {
                let end = ev.data.cast::<mpv_event_end_file>().as_ref()?;
                Some(Event::EndFile(EndFile {
                    reason: end.reason,
                    error: end.error,
                    playlist_entry_id: end.playlist_entry_id,
                    playlist_insert_id: end.playlist_insert_id,
                    playlist_insert_num_entries: end.playlist_insert_num_entries,
                }))
            }
            MPV_EVENT_CLIENT_MESSAGE => {
                let msg = ev.data.cast::<mpv_event_client_message>().as_ref()?;
                let n = usize::try_from(msg.num_args).unwrap_or(0);
                let args = if n == 0 || msg.args.is_null() {
                    Vec::new()
                } else {
                    (0..n)
                        .map(|i| c_str(*msg.args.add(i)).unwrap_or(c""))
                        .collect()
                };
                Some(Event::ClientMessage(args))
            }
            MPV_EVENT_GET_PROPERTY_REPLY => Some(Event::GetPropertyReply {
                userdata: ev.reply_userdata,
                error: ev.error,
                value: match ev.data.cast::<mpv_event_property>().as_ref() {
                    Some(prop) if ev.error >= 0 => value(prop),
                    _ => Value::None,
                },
            }),
            MPV_EVENT_SET_PROPERTY_REPLY => Some(Event::SetPropertyReply {
                userdata: ev.reply_userdata,
                error: ev.error,
            }),
            MPV_EVENT_COMMAND_REPLY => Some(Event::CommandReply {
                userdata: ev.reply_userdata,
                error: ev.error,
                result: match ev.data.cast::<mpv_event_command>().as_ref() {
                    Some(cmd) if ev.error >= 0 => Some(NodeRef(&cmd.result)),
                    _ => None,
                },
            }),
            id => Some(Event::Other(id)),
        }
    }
}

/// The default `logcat_level`: every message mpv sends is echoed, as before
/// `setLogcatLevel` existed.
pub const LOGCAT_ALL: c_int = c_int::MAX;

/// Whether a message of mpv's `log_level` is echoed to logcat when the most
/// verbose level to echo is `max` (one of mpv's levels; `MPV_LOG_LEVEL_NONE`,
/// 0, echoes nothing, since every message has a level of at least
/// `MPV_LOG_LEVEL_FATAL`).
pub fn echo_to_logcat(log_level: c_int, max: c_int) -> bool {
    log_level > MPV_LOG_LEVEL_NONE && log_level <= max
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
            Event::Property { name, value, .. } => {
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
        for id in [1, 8, 11, 17, 18, 20, 21, 24, 25, 99] {
            assert_eq!(
                decode_plain(id, ptr::null_mut()),
                Some(format!("{:?}", Event::Other(id)))
            );
        }
    }

    #[test]
    fn end_file_carries_everything_mpv_sends() {
        let mut end = mpv_event_end_file {
            reason: 4,
            error: -13,
            playlist_entry_id: 1,
            playlist_insert_id: 7,
            playlist_insert_num_entries: 2,
        };
        let ev = event(MPV_EVENT_END_FILE, (&raw mut end).cast());
        assert_eq!(
            unsafe { decode(&ev) },
            Some(Event::EndFile(EndFile {
                reason: 4,
                error: -13,
                playlist_entry_id: 1,
                playlist_insert_id: 7,
                playlist_insert_num_entries: 2,
            }))
        );
        // never sent like this, but must not crash
        assert_eq!(decode_plain(MPV_EVENT_END_FILE, ptr::null_mut()), None);
    }

    #[test]
    fn start_file_carries_the_entry_id() {
        let mut start = mpv_event_start_file {
            playlist_entry_id: 42,
        };
        let ev = event(MPV_EVENT_START_FILE, (&raw mut start).cast());
        assert_eq!(
            unsafe { decode(&ev) },
            Some(Event::StartFile {
                playlist_entry_id: 42
            })
        );
        assert_eq!(decode_plain(MPV_EVENT_START_FILE, ptr::null_mut()), None);
    }

    #[test]
    fn client_message_arguments() {
        let mut args = [c"hello".as_ptr(), ptr::null(), c"wörld".as_ptr()];
        let mut msg = mpv_event_client_message {
            num_args: 3,
            args: args.as_mut_ptr(),
        };
        let ev = event(MPV_EVENT_CLIENT_MESSAGE, (&raw mut msg).cast());
        assert_eq!(
            unsafe { decode(&ev) },
            Some(Event::ClientMessage(vec![c"hello", c"", c"wörld"]))
        );
        msg.num_args = 0;
        msg.args = ptr::null_mut();
        let ev = event(MPV_EVENT_CLIENT_MESSAGE, (&raw mut msg).cast());
        assert_eq!(unsafe { decode(&ev) }, Some(Event::ClientMessage(vec![])));
    }

    #[test]
    fn replies_carry_their_userdata_error_and_value() {
        let mut node = mpv_node {
            u: mpv_node_u { int64: 5 },
            format: MPV_FORMAT_INT64,
        };
        let mut prop = property(c"volume", MPV_FORMAT_NODE, (&raw mut node).cast());
        let mut ev = event(MPV_EVENT_GET_PROPERTY_REPLY, (&raw mut prop).cast());
        ev.reply_userdata = 9;
        assert_eq!(
            unsafe { decode(&ev) },
            Some(Event::GetPropertyReply {
                userdata: 9,
                error: 0,
                value: Value::Node(NodeRef(&node)),
            })
        );
        // on error mpv sends format NONE; never look at the data then
        ev.error = -8;
        assert!(matches!(
            unsafe { decode(&ev) },
            Some(Event::GetPropertyReply {
                userdata: 9,
                error: -8,
                value: Value::None
            })
        ));

        let mut set = event(MPV_EVENT_SET_PROPERTY_REPLY, ptr::null_mut());
        set.reply_userdata = u64::MAX;
        set.error = -10;
        assert_eq!(
            unsafe { decode(&set) },
            Some(Event::SetPropertyReply {
                userdata: u64::MAX,
                error: -10
            })
        );

        let mut cmd = mpv_event_command {
            result: mpv_node::NONE,
        };
        let mut ev = event(MPV_EVENT_COMMAND_REPLY, (&raw mut cmd).cast());
        ev.reply_userdata = 3;
        let decoded = unsafe { decode(&ev) };
        assert_eq!(
            decoded,
            Some(Event::CommandReply {
                userdata: 3,
                error: 0,
                result: Some(NodeRef(&cmd.result)),
            })
        );
        ev.error = -12;
        assert!(matches!(
            unsafe { decode(&ev) },
            Some(Event::CommandReply {
                error: -12,
                result: None,
                ..
            })
        ));
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
        // OSD string and byte array have no MPVLib callback
        assert_eq!(p(2, (&raw mut string).cast()), "Unsupported(2)");
        assert_eq!(p(9, (&raw mut int).cast()), "Unsupported(9)");
    }

    #[test]
    fn node_values_point_at_mpvs_node() {
        let mut node = mpv_node {
            u: mpv_node_u { flag: 1 },
            format: MPV_FORMAT_FLAG,
        };
        let mut prop = property(c"pause", MPV_FORMAT_NODE, (&raw mut node).cast());
        let mut ev = event(MPV_EVENT_PROPERTY_CHANGE, (&raw mut prop).cast());
        ev.reply_userdata = 12;
        assert_eq!(
            unsafe { decode(&ev) },
            Some(Event::Property {
                userdata: 12,
                name: c"pause",
                value: Value::Node(NodeRef(&node)),
            })
        );
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
    fn the_logcat_echo_follows_its_level() {
        // the default echoes every level, as before
        for level in [10, 20, 30, 40, 50, 60, 70] {
            assert!(echo_to_logcat(level, LOGCAT_ALL), "{level}");
        }
        // warn: errors and warnings only, so no "v" message with a URL in it
        assert!(echo_to_logcat(MPV_LOG_LEVEL_FATAL, MPV_LOG_LEVEL_WARN));
        assert!(echo_to_logcat(MPV_LOG_LEVEL_WARN, MPV_LOG_LEVEL_WARN));
        assert!(!echo_to_logcat(MPV_LOG_LEVEL_INFO, MPV_LOG_LEVEL_WARN));
        assert!(!echo_to_logcat(MPV_LOG_LEVEL_V, MPV_LOG_LEVEL_WARN));
        // none: nothing at all
        for level in [10, 20, 30, 40, 50, 60, 70] {
            assert!(!echo_to_logcat(level, MPV_LOG_LEVEL_NONE), "{level}");
        }
        assert!(
            !echo_to_logcat(0, LOGCAT_ALL),
            "a message never has level none"
        );
    }

    /// What `node::encode` makes of `value.as_node()`.
    fn reply_bytes(value: &Value<'_>) -> Vec<u8> {
        let mut out = Vec::new();
        unsafe { crate::node::encode(&value.as_node(), &mut out) }.unwrap();
        out
    }

    #[test]
    fn typed_replies_are_passed_on_as_scalar_nodes() {
        let mut double: f64 = 1.5;
        let mut prop = property(c"speed", MPV_FORMAT_DOUBLE, (&raw mut double).cast());
        let mut ev = event(MPV_EVENT_GET_PROPERTY_REPLY, (&raw mut prop).cast());
        ev.reply_userdata = 4;
        let Some(Event::GetPropertyReply { value, .. }) = (unsafe { decode(&ev) }) else {
            panic!("not a reply");
        };
        let mut expected = vec![MPV_FORMAT_DOUBLE as u8];
        expected.extend_from_slice(&1.5f64.to_bits().to_be_bytes());
        assert_eq!(reply_bytes(&value), expected);

        let mut int: i64 = -6_000_000_000;
        let mut expected = vec![MPV_FORMAT_INT64 as u8];
        expected.extend_from_slice(&(-6_000_000_000i64).to_be_bytes());
        assert_eq!(reply_bytes(&Value::Int64(int)), expected);
        int += 1;
        assert_ne!(reply_bytes(&Value::Int64(int)), expected);

        assert_eq!(reply_bytes(&Value::Flag(true)), [MPV_FORMAT_FLAG as u8, 1]);
        assert_eq!(reply_bytes(&Value::Flag(false)), [MPV_FORMAT_FLAG as u8, 0]);
        // a string as UTF-16, like every string node
        assert_eq!(
            reply_bytes(&Value::String(c"é")),
            [MPV_FORMAT_STRING as u8, 0, 0, 0, 1, 0x00, 0xE9]
        );
        assert_eq!(reply_bytes(&Value::None), [MPV_FORMAT_NONE as u8]);
        assert_eq!(reply_bytes(&Value::Unsupported(2)), [MPV_FORMAT_NONE as u8]);

        // a node is passed through as it is
        let node = mpv_node {
            u: mpv_node_u { int64: 7 },
            format: MPV_FORMAT_INT64,
        };
        assert_eq!(
            reply_bytes(&Value::Node(NodeRef(&node))),
            reply_bytes(&Value::Int64(7))
        );
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
