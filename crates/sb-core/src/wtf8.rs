//! Program text as SmileBASIC 4 stores it: UTF-8 in which each UTF-16
//! unit is written on its own, so a string can hold a surrogate with no
//! partner, written as the three bytes UTF-8 would give its value.
//! HSKN_KNJLIB keeps font data in strings that way.
//!
//! A Rust string cannot hold such a surrogate, so it stands in as a
//! character of Unicode's last private use plane, which the lexer turns
//! back into the surrogate in string literals and saving writes back as
//! the three bytes.

/// The stand-in for U+D800; the others follow it.
const STAND_IN: u32 = 0x10_F800;

/// The stand-in for a lone surrogate.
pub fn stand_in(unit: u16) -> char {
    debug_assert!((0xD800..=0xDFFF).contains(&unit));
    char::from_u32(STAND_IN + u32::from(unit - 0xD800)).expect("in the private use plane")
}

/// The surrogate a character stands in for, if it is a stand-in.
pub fn unit_of(c: char) -> Option<u16> {
    let n = u32::from(c).checked_sub(STAND_IN)?;
    (n < 0x800).then(|| 0xD800 + n as u16)
}

/// Text from SmileBASIC 4's UTF-8, with its lone surrogates as stand-ins;
/// `None` if it is not UTF-8 otherwise.
pub fn decode(mut bytes: &[u8]) -> Option<String> {
    let mut text = String::with_capacity(bytes.len());
    loop {
        match std::str::from_utf8(bytes) {
            Ok(s) => {
                text.push_str(s);
                return Some(text);
            }
            Err(e) => {
                let (good, rest) = bytes.split_at(e.valid_up_to());
                text.push_str(std::str::from_utf8(good).expect("checked"));
                // Even a pair is kept as two units, as it was written.
                match rest {
                    [0xED, b1 @ 0xA0..=0xBF, b2 @ 0x80..=0xBF, ..] => {
                        let unit = 0xD000 | u16::from(b1 & 0x3F) << 6 | u16::from(b2 & 0x3F);
                        text.push(stand_in(unit));
                        bytes = &rest[3..];
                    }
                    _ => return None,
                }
            }
        }
    }
}

/// Text as SmileBASIC 4 writes it, stand-ins back as their surrogates.
pub fn encode(text: &str) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(text.len());
    for c in text.chars() {
        match unit_of(c) {
            Some(u) => bytes.extend([
                0xE0 | (u >> 12) as u8,
                0x80 | (u >> 6 & 0x3F) as u8,
                0x80 | (u & 0x3F) as u8,
            ]),
            None => bytes.extend_from_slice(c.encode_utf8(&mut [0; 4]).as_bytes()),
        }
    }
    bytes
}

/// UTF-16 units of text, stand-ins as their surrogates.
pub fn to_utf16(text: &str) -> Vec<u16> {
    let mut units = Vec::with_capacity(text.len());
    for c in text.chars() {
        match unit_of(c) {
            Some(u) => units.push(u),
            None => units.extend_from_slice(c.encode_utf16(&mut [0; 2])),
        }
    }
    units
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lone_surrogates_go_round() {
        // "A", U+D848 on its own, U+3000, U+DC00 on its own.
        let bytes = b"A\xED\xA1\x88\xE3\x80\x80\xED\xB0\x80";
        let text = decode(bytes).unwrap();
        assert_eq!(text.chars().count(), 4);
        assert_eq!(to_utf16(&text), [0x41, 0xD848, 0x3000, 0xDC00]);
        assert_eq!(encode(&text), bytes);
        assert_eq!(decode(b"\xFF"), None);
        assert_eq!(decode("한글".as_bytes()).unwrap(), "한글");
    }
}
