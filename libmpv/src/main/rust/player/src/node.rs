//! `mpv_node` to and from the bytes Kotlin's `NodeCodec` reads and writes.
//!
//! A node crosses JNI as one `byte[]` instead of a tree of Java objects: one
//! JNI call per value instead of one per node, no local references to manage
//! on the event thread, and both directions testable without a VM (here and in
//! the Kotlin unit tests, which decode the same bytes).
//!
//! The format, big-endian, one tag byte per node (the tags are mpv's own
//! `mpv_format` numbers):
//!
//! | tag | node        | payload                                         |
//! |-----|-------------|-------------------------------------------------|
//! | 0   | none        | nothing                                         |
//! | 1   | string      | u32 count, then that many UTF-16 code units     |
//! | 3   | flag        | u8, 0 or 1                                      |
//! | 4   | int64       | i64                                             |
//! | 5   | double      | f64 (IEEE 754 bits)                             |
//! | 7   | array       | u32 count, then that many nodes                 |
//! | 8   | map         | u32 count, then that many (string payload, node)|
//! | 9   | byte array  | u32 count, then that many bytes                 |
//!
//! Strings travel as UTF-16 so that they are converted exactly like every
//! other string this library passes (`crate::text`): invalid UTF-8 from mpv
//! becomes '?', an unpaired surrogate from Java becomes U+FFFD and U+0000
//! becomes C0 80.

use std::ffi::{CString, c_char, c_int, c_void};
use std::ptr;

use crate::ffi::*;
use crate::text;

/// Deeper nodes are refused in both directions. mpv's own nodes are a few
/// levels deep; this only stops runaway recursion on a thread's stack.
pub const MAX_DEPTH: usize = 64;

#[derive(Debug, PartialEq, Eq)]
pub enum NodeError {
    /// nested deeper than `MAX_DEPTH`
    TooDeep,
    /// the bytes are not a valid encoding of exactly one node, or mpv's node
    /// breaks its own invariants (a null list with entries)
    Malformed,
}

pub type Result<T> = std::result::Result<T, NodeError>;

// ---------------------------------------------------------------- encoding

fn put_u32(out: &mut Vec<u8>, n: usize) -> Result<()> {
    let n = u32::try_from(n).map_err(|_| NodeError::Malformed)?;
    out.extend_from_slice(&n.to_be_bytes());
    Ok(())
}

fn put_string(out: &mut Vec<u8>, utf8: &[u8]) -> Result<()> {
    let units = text::utf8_to_utf16(utf8);
    put_u32(out, units.len())?;
    out.reserve(units.len() * 2);
    for u in units {
        out.extend_from_slice(&u.to_be_bytes());
    }
    Ok(())
}

/// # Safety
/// `s` is null or a NUL-terminated string.
unsafe fn c_bytes<'a>(s: *const c_char) -> Option<&'a [u8]> {
    // SAFETY: the caller's contract
    (!s.is_null()).then(|| unsafe { std::ffi::CStr::from_ptr(s) }.to_bytes())
}

/// Appends the encoding of `node` to `out`.
///
/// # Safety
/// `node` must be a valid `mpv_node` as mpv documents it, with everything it
/// points to alive for the duration of the call.
pub unsafe fn encode(node: &mpv_node, out: &mut Vec<u8>) -> Result<()> {
    // SAFETY: the caller's contract
    unsafe { encode_at(node, out, 0) }
}

/// Appends the encoding of a string node with the bytes of `s`.
pub fn encode_string(s: &[u8], out: &mut Vec<u8>) -> Result<()> {
    out.push(MPV_FORMAT_STRING as u8);
    put_string(out, s)
}

unsafe fn encode_at(node: &mpv_node, out: &mut Vec<u8>, depth: usize) -> Result<()> {
    if depth > MAX_DEPTH {
        return Err(NodeError::TooDeep);
    }
    // SAFETY (whole match): `format` says which union member is valid; the
    // pointers in it are valid (the caller's contract)
    unsafe {
        match node.format {
            MPV_FORMAT_STRING | MPV_FORMAT_OSD_STRING => match c_bytes(node.u.string) {
                Some(s) => encode_string(s, out)?,
                None => out.push(MPV_FORMAT_NONE as u8),
            },
            MPV_FORMAT_FLAG => {
                out.push(MPV_FORMAT_FLAG as u8);
                out.push(u8::from(node.u.flag != 0));
            }
            MPV_FORMAT_INT64 => {
                out.push(MPV_FORMAT_INT64 as u8);
                out.extend_from_slice(&node.u.int64.to_be_bytes());
            }
            MPV_FORMAT_DOUBLE => {
                out.push(MPV_FORMAT_DOUBLE as u8);
                out.extend_from_slice(&node.u.double_.to_bits().to_be_bytes());
            }
            MPV_FORMAT_NODE_ARRAY | MPV_FORMAT_NODE_MAP => {
                let is_map = node.format == MPV_FORMAT_NODE_MAP;
                out.push(node.format as u8);
                let Some(list) = node.u.list.as_ref() else {
                    put_u32(out, 0)?; // mpv never sends this; read it as empty
                    return Ok(());
                };
                let num = usize::try_from(list.num).map_err(|_| NodeError::Malformed)?;
                put_u32(out, num)?;
                if num == 0 {
                    return Ok(());
                }
                if list.values.is_null() || (is_map && list.keys.is_null()) {
                    return Err(NodeError::Malformed);
                }
                for i in 0..num {
                    if is_map {
                        let key = c_bytes(*list.keys.add(i)).ok_or(NodeError::Malformed)?;
                        put_string(out, key)?;
                    }
                    encode_at(&*list.values.add(i), out, depth + 1)?;
                }
            }
            MPV_FORMAT_BYTE_ARRAY => {
                out.push(MPV_FORMAT_BYTE_ARRAY as u8);
                let Some(ba) = node.u.ba.as_ref() else {
                    put_u32(out, 0)?;
                    return Ok(());
                };
                put_u32(out, ba.size)?;
                if ba.size > 0 {
                    if ba.data.is_null() {
                        return Err(NodeError::Malformed);
                    }
                    out.extend_from_slice(std::slice::from_raw_parts(
                        ba.data.cast::<u8>(),
                        ba.size,
                    ));
                }
            }
            // MPV_FORMAT_NONE, and formats this file does not know: mpv says
            // nothing may be assumed about the union then
            _ => out.push(MPV_FORMAT_NONE as u8),
        }
    }
    Ok(())
}

// ---------------------------------------------------------------- decoding

/// An `mpv_node` built from bytes, owning everything it points to. The node
/// stays valid while this value exists; moving it does not move the data the
/// node points to (all of it is on the heap).
pub struct OwnedNode {
    root: mpv_node,
    _store: Store,
}

// Boxed so that what mpv points to stays put while the Vecs grow.
#[allow(clippy::vec_box)]
#[derive(Default)]
struct Store {
    strings: Vec<CString>,
    nodes: Vec<Vec<mpv_node>>,
    keys: Vec<Vec<*mut c_char>>,
    lists: Vec<Box<mpv_node_list>>,
    bytes: Vec<Vec<u8>>,
    byte_arrays: Vec<Box<mpv_byte_array>>,
}

impl OwnedNode {
    /// Decodes exactly one node from `bytes`.
    pub fn decode(bytes: &[u8]) -> Result<OwnedNode> {
        let mut reader = Reader { bytes, pos: 0 };
        let mut store = Store::default();
        let root = reader.node(&mut store, 0)?;
        if reader.pos != bytes.len() {
            return Err(NodeError::Malformed);
        }
        Ok(OwnedNode {
            root,
            _store: store,
        })
    }

    #[cfg(test)]
    pub fn format(&self) -> mpv_format {
        self.root.format
    }

    /// For mpv functions that take `mpv_node *`; mpv does not write through
    /// it for the arguments this library passes (commands, property values).
    pub fn as_mut_ptr(&mut self) -> *mut mpv_node {
        &raw mut self.root
    }

    #[cfg(test)]
    pub fn root(&self) -> &mpv_node {
        &self.root
    }
}

struct Reader<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl Reader<'_> {
    fn take(&mut self, n: usize) -> Result<&[u8]> {
        let end = self.pos.checked_add(n).ok_or(NodeError::Malformed)?;
        let s = self.bytes.get(self.pos..end).ok_or(NodeError::Malformed)?;
        self.pos = end;
        Ok(s)
    }

    fn array<const N: usize>(&mut self) -> Result<[u8; N]> {
        let s = self.take(N)?;
        Ok(s.try_into().expect("take returned N bytes"))
    }

    fn count(&mut self) -> Result<usize> {
        let n = u32::from_be_bytes(self.array()?);
        usize::try_from(n).map_err(|_| NodeError::Malformed)
    }

    /// At least `min_size` bytes per entry must remain, so a forged count
    /// cannot make us allocate more than the input could describe.
    fn list_count(&mut self, min_size: usize) -> Result<usize> {
        let n = self.count()?;
        if n > (self.bytes.len() - self.pos) / min_size || c_int::try_from(n).is_err() {
            return Err(NodeError::Malformed);
        }
        Ok(n)
    }

    fn string(&mut self, store: &mut Store) -> Result<*mut c_char> {
        let n = self.count()?;
        let raw = self.take(n.checked_mul(2).ok_or(NodeError::Malformed)?)?;
        let units: Vec<u16> = raw
            .as_chunks::<2>()
            .0
            .iter()
            .map(|c| u16::from_be_bytes(*c))
            .collect();
        let s = text::utf16_to_c_string(&units);
        let p = s.as_ptr().cast_mut();
        store.strings.push(s);
        Ok(p)
    }

    fn node(&mut self, store: &mut Store, depth: usize) -> Result<mpv_node> {
        if depth > MAX_DEPTH {
            return Err(NodeError::TooDeep);
        }
        let tag = c_int::from(self.array::<1>()?[0]);
        let u = match tag {
            MPV_FORMAT_NONE => return Ok(mpv_node::NONE),
            MPV_FORMAT_STRING => mpv_node_u {
                string: self.string(store)?,
            },
            MPV_FORMAT_FLAG => match self.array::<1>()?[0] {
                0 => mpv_node_u { flag: 0 },
                1 => mpv_node_u { flag: 1 },
                _ => return Err(NodeError::Malformed),
            },
            MPV_FORMAT_INT64 => mpv_node_u {
                int64: i64::from_be_bytes(self.array()?),
            },
            MPV_FORMAT_DOUBLE => mpv_node_u {
                double_: f64::from_bits(u64::from_be_bytes(self.array()?)),
            },
            MPV_FORMAT_NODE_ARRAY | MPV_FORMAT_NODE_MAP => {
                let is_map = tag == MPV_FORMAT_NODE_MAP;
                // an entry is at least a tag byte, plus a key's count for maps
                let n = self.list_count(if is_map { 5 } else { 1 })?;
                let mut values = Vec::with_capacity(n);
                let mut keys = Vec::with_capacity(if is_map { n } else { 0 });
                for _ in 0..n {
                    if is_map {
                        keys.push(self.string(store)?);
                    }
                    values.push(self.node(store, depth + 1)?);
                }
                // the heap buffers do not move when the Vecs move into the store
                let values_ptr = if n == 0 {
                    ptr::null_mut()
                } else {
                    values.as_mut_ptr()
                };
                let keys_ptr = if is_map && n > 0 {
                    keys.as_mut_ptr()
                } else {
                    ptr::null_mut()
                };
                store.nodes.push(values);
                store.keys.push(keys);
                let mut list = Box::new(mpv_node_list {
                    num: c_int::try_from(n).map_err(|_| NodeError::Malformed)?,
                    values: values_ptr,
                    keys: keys_ptr,
                });
                let p: *mut mpv_node_list = &raw mut *list;
                store.lists.push(list);
                mpv_node_u { list: p }
            }
            MPV_FORMAT_BYTE_ARRAY => {
                let n = self.count()?;
                let mut data = self.take(n)?.to_vec();
                let mut ba = Box::new(mpv_byte_array {
                    data: if n == 0 {
                        ptr::null_mut()
                    } else {
                        data.as_mut_ptr().cast::<c_void>()
                    },
                    size: n,
                });
                let p: *mut mpv_byte_array = &raw mut *ba;
                store.bytes.push(data);
                store.byte_arrays.push(ba);
                mpv_node_u { ba: p }
            }
            _ => return Err(NodeError::Malformed),
        };
        Ok(mpv_node { u, format: tag })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CStr;

    // Hand-built nodes, laid out as mpv lays them out (repr(C)), kept alive by
    // the vectors in `Fixture`.
    #[derive(Default)]
    #[allow(clippy::vec_box)]
    struct Fixture {
        strings: Vec<CString>,
        nodes: Vec<Vec<mpv_node>>,
        keys: Vec<Vec<*mut c_char>>,
        lists: Vec<Box<mpv_node_list>>,
        bas: Vec<Box<mpv_byte_array>>,
        data: Vec<Vec<u8>>,
    }

    impl Fixture {
        fn string(&mut self, s: &[u8]) -> mpv_node {
            let s = CString::new(s).unwrap();
            let p = s.as_ptr().cast_mut();
            self.strings.push(s);
            mpv_node {
                u: mpv_node_u { string: p },
                format: MPV_FORMAT_STRING,
            }
        }
        fn key(&mut self, s: &str) -> *mut c_char {
            let s = CString::new(s).unwrap();
            let p = s.as_ptr().cast_mut();
            self.strings.push(s);
            p
        }
        fn list(
            &mut self,
            format: mpv_format,
            mut values: Vec<mpv_node>,
            keys: Option<Vec<&str>>,
        ) -> mpv_node {
            let mut keys: Option<Vec<*mut c_char>> =
                keys.map(|k| k.into_iter().map(|k| self.key(k)).collect());
            let mut list = Box::new(mpv_node_list {
                num: values.len() as c_int,
                values: values.as_mut_ptr(),
                keys: keys.as_mut().map_or(ptr::null_mut(), |k| k.as_mut_ptr()),
            });
            let p: *mut mpv_node_list = &raw mut *list;
            self.nodes.push(values);
            if let Some(k) = keys {
                self.keys.push(k);
            }
            self.lists.push(list);
            mpv_node {
                u: mpv_node_u { list: p },
                format,
            }
        }
        fn bytes(&mut self, b: &[u8]) -> mpv_node {
            let mut data = b.to_vec();
            let mut ba = Box::new(mpv_byte_array {
                data: data.as_mut_ptr().cast(),
                size: data.len(),
            });
            let p: *mut mpv_byte_array = &raw mut *ba;
            self.data.push(data);
            self.bas.push(ba);
            mpv_node {
                u: mpv_node_u { ba: p },
                format: MPV_FORMAT_BYTE_ARRAY,
            }
        }
    }

    fn flag(b: bool) -> mpv_node {
        mpv_node {
            u: mpv_node_u {
                flag: c_int::from(b),
            },
            format: MPV_FORMAT_FLAG,
        }
    }
    fn int(i: i64) -> mpv_node {
        mpv_node {
            u: mpv_node_u { int64: i },
            format: MPV_FORMAT_INT64,
        }
    }
    fn double(d: f64) -> mpv_node {
        mpv_node {
            u: mpv_node_u { double_: d },
            format: MPV_FORMAT_DOUBLE,
        }
    }

    fn enc(node: &mpv_node) -> Vec<u8> {
        let mut out = Vec::new();
        unsafe { encode(node, &mut out) }.unwrap();
        out
    }

    fn hex(b: &[u8]) -> String {
        b.iter().map(|b| format!("{b:02x}")).collect()
    }

    /// A track-list entry as mpv reports it, trimmed. The same bytes are
    /// decoded by the Kotlin unit test (NodeCodecTest.GOLDEN), so the two
    /// sides agree on the format.
    const GOLDEN: &str = "0700000001\
        0800000005\
        00000002 0069 0064 04 0000000000000001\
        00000004 0074 0079 0070 0065 01 00000005 0061 0075 0064 0069 006f\
        00000008 0073 0065 006c 0065 0063 0074 0065 0064 03 01\
        00000009 0064 0065 006d 0075 0078 002d 0066 0070 0073 05 3ff8000000000000\
        00000005 0074 0069 0074 006c 0065 01 00000002 00dc 0020";

    fn golden() -> Vec<u8> {
        let h: String = GOLDEN.split_whitespace().collect();
        (0..h.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&h[i..i + 2], 16).unwrap())
            .collect()
    }

    #[test]
    fn encodes_a_track_list_like_mpv_sends_it() {
        let mut f = Fixture::default();
        let audio = f.string(b"audio");
        let title = f.string("Ü ".as_bytes());
        let track = f.list(
            MPV_FORMAT_NODE_MAP,
            vec![int(1), audio, flag(true), double(1.5), title],
            Some(vec!["id", "type", "selected", "demux-fps", "title"]),
        );
        let list = f.list(MPV_FORMAT_NODE_ARRAY, vec![track], None);
        assert_eq!(hex(&enc(&list)), hex(&golden()));
    }

    #[test]
    fn scalars() {
        assert_eq!(enc(&mpv_node::NONE), [0]);
        assert_eq!(enc(&flag(false)), [3, 0]);
        assert_eq!(enc(&flag(true)), [3, 1]);
        // any non-zero flag is true
        let odd = mpv_node {
            u: mpv_node_u { flag: 7 },
            format: MPV_FORMAT_FLAG,
        };
        assert_eq!(enc(&odd), [3, 1]);
        assert_eq!(
            enc(&int(-2)),
            [4, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xfe]
        );
        assert_eq!(hex(&enc(&double(-0.0))), "058000000000000000");
        assert_eq!(
            hex(&enc(&double(f64::NAN))),
            format!("05{:016x}", f64::NAN.to_bits())
        );
    }

    #[test]
    fn strings_are_utf16_with_invalid_utf8_replaced() {
        let mut f = Fixture::default();
        let s = f.string("🎵".as_bytes());
        assert_eq!(hex(&enc(&s)), "0100000002d83cdfb5");
        let bad = f.string(b"a\xffb");
        assert_eq!(hex(&enc(&bad)), "01000000030061003f0062");
        let null = mpv_node {
            u: mpv_node_u {
                string: ptr::null_mut(),
            },
            format: MPV_FORMAT_STRING,
        };
        assert_eq!(enc(&null), [0]);
    }

    #[test]
    fn byte_arrays_and_empty_lists() {
        let mut f = Fixture::default();
        let b = f.bytes(&[0, 1, 0xff]);
        assert_eq!(hex(&enc(&b)), "09000000030001ff");
        let empty = f.bytes(&[]);
        assert_eq!(hex(&enc(&empty)), "0900000000");
        let arr = f.list(MPV_FORMAT_NODE_ARRAY, vec![], None);
        assert_eq!(hex(&enc(&arr)), "0700000000");
        let null_list = mpv_node {
            u: mpv_node_u {
                list: ptr::null_mut(),
            },
            format: MPV_FORMAT_NODE_MAP,
        };
        assert_eq!(hex(&enc(&null_list)), "0800000000");
    }

    #[test]
    fn unknown_formats_encode_as_none() {
        let weird = mpv_node {
            u: mpv_node_u { int64: 42 },
            format: 99,
        };
        assert_eq!(enc(&weird), [0]);
    }

    #[test]
    fn broken_lists_are_refused() {
        for (num, format) in [
            (2, MPV_FORMAT_NODE_ARRAY),
            (-1, MPV_FORMAT_NODE_ARRAY),
            (1, MPV_FORMAT_NODE_MAP),
        ] {
            let mut values = [int(1)];
            let mut list = mpv_node_list {
                num,
                // a map with values but no keys, or entries without values
                values: if format == MPV_FORMAT_NODE_MAP {
                    values.as_mut_ptr()
                } else {
                    ptr::null_mut()
                },
                keys: ptr::null_mut(),
            };
            let node = mpv_node {
                u: mpv_node_u {
                    list: &raw mut list,
                },
                format,
            };
            let mut out = Vec::new();
            assert_eq!(
                unsafe { encode(&node, &mut out) },
                Err(NodeError::Malformed),
                "{num} {format}"
            );
        }
    }

    #[test]
    fn depth_is_limited_both_ways() {
        let mut f = Fixture::default();
        let mut node = int(1);
        for _ in 0..=MAX_DEPTH {
            node = f.list(MPV_FORMAT_NODE_ARRAY, vec![node], None);
        }
        let mut out = Vec::new();
        assert_eq!(unsafe { encode(&node, &mut out) }, Err(NodeError::TooDeep));

        let mut deep = Vec::new();
        for _ in 0..=MAX_DEPTH {
            deep.extend_from_slice(&[7, 0, 0, 0, 1]);
        }
        deep.push(0);
        assert!(matches!(OwnedNode::decode(&deep), Err(NodeError::TooDeep)));
        // one level less is fine
        assert!(OwnedNode::decode(&deep[5..]).is_ok());
    }

    /// Reads a decoded node back the way mpv would, through the raw pointers.
    fn describe(node: &mpv_node) -> String {
        unsafe {
            match node.format {
                MPV_FORMAT_NONE => "none".into(),
                MPV_FORMAT_STRING => format!("{:?}", CStr::from_ptr(node.u.string)),
                MPV_FORMAT_FLAG => format!("flag {}", node.u.flag),
                MPV_FORMAT_INT64 => format!("int {}", node.u.int64),
                MPV_FORMAT_DOUBLE => format!("double {}", node.u.double_),
                MPV_FORMAT_NODE_ARRAY | MPV_FORMAT_NODE_MAP => {
                    let list = &*node.u.list;
                    let items: Vec<String> = (0..list.num as usize)
                        .map(|i| {
                            let v = describe(&*list.values.add(i));
                            if node.format == MPV_FORMAT_NODE_MAP {
                                format!("{:?}: {v}", CStr::from_ptr(*list.keys.add(i)))
                            } else {
                                v
                            }
                        })
                        .collect();
                    if node.format == MPV_FORMAT_NODE_MAP {
                        assert!(list.num == 0 || !list.keys.is_null());
                        format!("{{{}}}", items.join(", "))
                    } else {
                        assert!(list.keys.is_null());
                        format!("[{}]", items.join(", "))
                    }
                }
                MPV_FORMAT_BYTE_ARRAY => {
                    let ba = &*node.u.ba;
                    if ba.size == 0 {
                        "bytes []".into()
                    } else {
                        format!(
                            "bytes {:?}",
                            std::slice::from_raw_parts(ba.data.cast::<u8>(), ba.size)
                        )
                    }
                }
                f => format!("format {f}"),
            }
        }
    }

    #[test]
    fn decodes_into_a_node_mpv_can_read() {
        let node = OwnedNode::decode(&golden()).unwrap();
        assert_eq!(node.format(), MPV_FORMAT_NODE_ARRAY);
        assert_eq!(
            describe(node.root()),
            r#"[{"id": int 1, "type": "audio", "selected": flag 1, "demux-fps": double 1.5, "title": "Ü "}]"#
        );
    }

    #[test]
    fn decoded_nodes_survive_a_move() {
        let node = OwnedNode::decode(&golden()).unwrap();
        let moved = Box::new(node);
        assert!(describe(moved.root()).contains("audio"));
    }

    #[test]
    fn round_trip_through_mpv_layout() {
        let mut f = Fixture::default();
        let s = f.string("x ✓ y".as_bytes());
        let b = f.bytes(&[9, 8]);
        let inner = f.list(MPV_FORMAT_NODE_ARRAY, vec![flag(false), double(2.25)], None);
        let map = f.list(
            MPV_FORMAT_NODE_MAP,
            vec![s, b, inner, mpv_node::NONE, int(i64::MIN)],
            Some(vec!["s", "b", "inner", "none", "min"]),
        );
        let bytes = enc(&map);
        let decoded = OwnedNode::decode(&bytes).unwrap();
        assert_eq!(describe(decoded.root()), describe(&map));
        assert_eq!(enc(decoded.root()), bytes);
    }

    #[test]
    fn java_strings_follow_the_jni_conversion() {
        // U+0000 becomes C0 80, a lone surrogate U+FFFD, as for every other string
        let decoded = OwnedNode::decode(&[1, 0, 0, 0, 3, 0, 0x61, 0, 0, 0xd8, 0]).unwrap();
        let s = unsafe { CStr::from_ptr(decoded.root().u.string) };
        assert_eq!(s.to_bytes(), b"a\xc0\x80\xef\xbf\xbd");
    }

    #[test]
    fn malformed_input_is_refused() {
        for bad in [
            &[][..],
            &[2],                            // OSD string is not a node type on the wire
            &[6],                            // nor is "node"
            &[3, 2],                         // flag must be 0 or 1
            &[4, 0, 0],                      // short int64
            &[1, 0, 0, 0, 2, 0, 0x61],       // short string
            &[7, 0, 0, 0, 1],                // array missing its entry
            &[7, 0xff, 0xff, 0xff, 0xff, 0], // forged count
            &[8, 0, 0, 0, 1, 0],             // map entry cut short
            &[9, 0, 0, 0, 4, 1, 2],          // short byte array
            &[0, 0],                         // trailing bytes
        ] {
            assert!(OwnedNode::decode(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn empty_containers_have_null_pointers() {
        let a = OwnedNode::decode(&[7, 0, 0, 0, 0]).unwrap();
        let list = unsafe { &*a.root().u.list };
        assert_eq!(list.num, 0);
        assert!(list.values.is_null() && list.keys.is_null());
        let m = OwnedNode::decode(&[8, 0, 0, 0, 0]).unwrap();
        assert_eq!(describe(m.root()), "{}");
        let b = OwnedNode::decode(&[9, 0, 0, 0, 0]).unwrap();
        assert_eq!(describe(b.root()), "bytes []");
    }
}
