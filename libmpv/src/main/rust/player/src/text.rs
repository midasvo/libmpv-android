//! Conversions between mpv's C strings (UTF-8, not always valid) and Java's
//! UTF-16 strings.
//!
//! JNI's own string functions (`NewStringUTF`, `GetStringUTFChars`) speak
//! Modified UTF-8, which encodes U+0000 and supplementary characters
//! differently from standard UTF-8 and is undefined for invalid input. Both
//! directions therefore go through UTF-16 (`NewString`, `GetStringRegion`) and
//! are converted here.

use std::ffi::CString;

/// Decodes the UTF-8 sequence at the start of `s`: its length and code point,
/// or `None` if the bytes there are not valid UTF-8. Overlong forms,
/// surrogates (U+D800 to U+DFFF) and code points above U+10FFFF are invalid, as
/// is a sequence cut short by the end of `s`.
fn decode_utf8(s: &[u8]) -> Option<(usize, u32)> {
    let lead = *s.first()?;
    if lead < 0x80 {
        return Some((1, u32::from(lead)));
    }

    // allowed range of the second byte, narrowed for the leads where the
    // shortest form, the surrogates or the U+10FFFF limit are decided there
    let (len, lo, hi) = match lead {
        0xc2..=0xdf => (2, 0x80, 0xbf),
        0xe0 => (3, 0xa0, 0xbf), // overlong below
        0xed => (3, 0x80, 0x9f), // surrogates above
        0xe1..=0xef => (3, 0x80, 0xbf),
        0xf0 => (4, 0x90, 0xbf), // overlong below
        0xf4 => (4, 0x80, 0x8f), // above U+10FFFF
        0xf1..=0xf3 => (4, 0x80, 0xbf),
        _ => return None, // continuation byte, 0xc0, 0xc1 or 0xf5..=0xff
    };

    let seq = s.get(..len)?;
    if !(lo..=hi).contains(&seq[1]) {
        return None;
    }
    if seq[2..].iter().any(|&b| b & 0xc0 != 0x80) {
        return None;
    }

    let mut cp = u32::from(lead & (0x7f >> len)); // payload bits of the lead byte
    for &b in &seq[1..] {
        cp = (cp << 6) | u32::from(b & 0x3f);
    }
    Some((len, cp))
}

/// Converts mpv's bytes to UTF-16 for `NewString`. Each byte that does not
/// start a valid UTF-8 sequence becomes one `'?'`, and decoding resumes at the
/// next byte, so nothing invalid can reach Java.
pub fn utf8_to_utf16(bytes: &[u8]) -> Vec<u16> {
    let mut units = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match decode_utf8(&bytes[i..]) {
            Some((len, cp)) => {
                // cp is a valid scalar value, so this cannot fail
                let c = char::from_u32(cp).unwrap_or('?');
                let mut buf = [0u16; 2];
                units.extend_from_slice(c.encode_utf16(&mut buf));
                i += len;
            }
            None => {
                units.push(u16::from(b'?'));
                i += 1;
            }
        }
    }
    units
}

/// Converts a Java string's UTF-16 code units to a C string for mpv: standard
/// UTF-8 (supplementary characters as one 4-byte sequence). An unpaired
/// surrogate becomes U+FFFD. U+0000 becomes the two bytes C0 80, as JNI's
/// Modified UTF-8 has it, so the C string is not cut short.
pub fn utf16_to_c_string(units: &[u16]) -> CString {
    let mut bytes = Vec::with_capacity(units.len() + units.len() / 2 + 1);
    for c in char::decode_utf16(units.iter().copied()) {
        match c.unwrap_or(char::REPLACEMENT_CHARACTER) {
            '\0' => bytes.extend_from_slice(&[0xc0, 0x80]),
            c => {
                let mut buf = [0u8; 4];
                bytes.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
            }
        }
    }
    // no 0 byte can be in `bytes`: U+0000 was replaced above and every other
    // character encodes without one
    CString::new(bytes).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn to_java(bytes: &[u8]) -> String {
        String::from_utf16(&utf8_to_utf16(bytes)).expect("always valid UTF-16")
    }

    fn from_java(s: &str) -> Vec<u8> {
        let units: Vec<u16> = s.encode_utf16().collect();
        utf16_to_c_string(&units).into_bytes()
    }

    #[test]
    fn ascii_and_empty() {
        assert_eq!(to_java(b""), "");
        assert_eq!(to_java(b"mpv 0.42.0"), "mpv 0.42.0");
        assert_eq!(from_java(""), b"");
        assert_eq!(from_java("loadfile"), b"loadfile");
    }

    #[test]
    fn valid_utf8_matches_std() {
        for s in [
            "Ünïcödé – 日本語 ✓ Ωmega",
            "notes 🎵 𝄞 end",
            "\u{7f}\u{80}\u{7ff}\u{800}\u{ffff}\u{10000}\u{10ffff}",
        ] {
            assert_eq!(to_java(s.as_bytes()), s);
            let expected: Vec<u16> = s.encode_utf16().collect();
            assert_eq!(utf8_to_utf16(s.as_bytes()), expected);
        }
    }

    #[test]
    fn supplementary_characters_become_surrogate_pairs() {
        // U+1F3B5 MUSICAL NOTE
        assert_eq!(utf8_to_utf16("🎵".as_bytes()), [0xd83c, 0xdfb5]);
        // and a pair becomes 4 bytes, not the 6 of Modified UTF-8
        assert_eq!(from_java("🎵"), [0xf0, 0x9f, 0x8e, 0xb5]);
    }

    #[test]
    fn round_trip_through_both_directions() {
        for s in [
            "libmpv-test ëmoji 🎵.wav",
            "notes 🎵 𝄞 end",
            "libmpv-test Ünïcödé – 日本語 ✓.wav",
        ] {
            assert_eq!(to_java(&from_java(s)), s);
        }
    }

    #[test]
    fn each_invalid_byte_becomes_one_question_mark() {
        // lone continuation bytes and leads that never start a sequence
        assert_eq!(to_java(b"a\x80b"), "a?b");
        assert_eq!(to_java(b"\xbf"), "?");
        assert_eq!(to_java(b"\xc0\xc1"), "??");
        assert_eq!(to_java(b"\xf5\xff"), "??");
        // Latin-1 text that is not UTF-8
        assert_eq!(to_java(b"caf\xe9"), "caf?");
    }

    #[test]
    fn overlong_forms_are_invalid() {
        assert_eq!(to_java(b"\xc0\xaf"), "??"); // '/' in 2 bytes
        assert_eq!(to_java(b"\xc1\xbf"), "??");
        assert_eq!(to_java(b"\xe0\x80\xaf"), "???"); // '/' in 3 bytes
        assert_eq!(to_java(b"\xe0\x9f\xbf"), "???");
        assert_eq!(to_java(b"\xf0\x80\x80\xaf"), "????"); // '/' in 4 bytes
        assert_eq!(to_java(b"\xf0\x8f\xbf\xbf"), "????");
        // the shortest forms right above them are valid
        assert_eq!(to_java(b"\xc2\x80"), "\u{80}");
        assert_eq!(to_java(b"\xe0\xa0\x80"), "\u{800}");
        assert_eq!(to_java(b"\xf0\x90\x80\x80"), "\u{10000}");
    }

    #[test]
    fn encoded_surrogates_are_invalid() {
        // U+D800 and U+DFFF; also CESU-8, the 6-byte form of U+1F3B5
        assert_eq!(to_java(b"\xed\xa0\x80"), "???");
        assert_eq!(to_java(b"\xed\xbf\xbf"), "???");
        assert_eq!(to_java(b"\xed\xa0\xbc\xed\xbe\xb5"), "??????");
        // U+D7FF and U+E000 on either side are valid
        assert_eq!(to_java(b"\xed\x9f\xbf"), "\u{d7ff}");
        assert_eq!(to_java(b"\xee\x80\x80"), "\u{e000}");
    }

    #[test]
    fn code_points_above_u10ffff_are_invalid() {
        assert_eq!(to_java(b"\xf4\x8f\xbf\xbf"), "\u{10ffff}");
        assert_eq!(to_java(b"\xf4\x90\x80\x80"), "????");
        assert_eq!(to_java(b"\xf7\xbf\xbf\xbf"), "????");
    }

    #[test]
    fn truncated_sequences_are_invalid_and_resume() {
        assert_eq!(to_java(b"\xe6\x97"), "??"); // 日 without its last byte
        assert_eq!(to_java(b"\xe6\x97A"), "??A");
        assert_eq!(to_java(b"\xf0\x9f\x8e"), "???");
        assert_eq!(to_java(b"\xf0\x9f\x8eZ\xe6\x97\xa5"), "???Z日");
        assert_eq!(to_java(b"\xc3"), "?");
    }

    #[test]
    fn invalid_bytes_match_std_maximal_subparts_per_byte() {
        // Our rule (one '?' per byte) agrees with std's validation on what is
        // valid: compare against from_utf8_lossy with each U+FFFD expanded to
        // the number of bytes it replaced.
        let cases: &[&[u8]] = &[
            b"\xe2\x82",
            b"\xe2\x82(",
            b"\xf0\x28\x8c\xbc",
            b"\xed\xa0\x80x",
            b"ok\xff\xfe\xfdok",
            b"\xf4\x90\x80\x80\xf4\x8f\xbf\xbf",
        ];
        for &case in cases {
            let mut expected = String::new();
            for chunk in case.utf8_chunks() {
                expected.push_str(chunk.valid());
                expected.extend(std::iter::repeat_n('?', chunk.invalid().len()));
            }
            assert_eq!(to_java(case), expected, "{case:x?}");
        }
    }

    #[test]
    fn unpaired_surrogates_from_java_become_replacement_character() {
        assert_eq!(
            utf16_to_c_string(&[u16::from(b'a'), 0xd83c, u16::from(b'b')]).as_bytes(),
            b"a\xef\xbf\xbdb"
        );
        assert_eq!(
            utf16_to_c_string(&[0xdfb5]).as_bytes(),
            "\u{fffd}".as_bytes()
        );
        // high surrogate at the very end
        assert_eq!(
            utf16_to_c_string(&[u16::from(b'x'), 0xd800]).as_bytes(),
            b"x\xef\xbf\xbd"
        );
    }

    #[test]
    fn nul_from_java_does_not_cut_the_string() {
        assert_eq!(from_java("a\0b"), b"a\xc0\x80b");
    }
}
