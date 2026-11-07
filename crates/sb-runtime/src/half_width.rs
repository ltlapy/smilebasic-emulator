//! Glyphs half a cell wide for half-width characters, which SmileBASIC 4
//! draws as wide as any other. The emulator's own windows and menus use
//! them, and so does the editor when its half-width setting is on;
//! programs never see them.
//!
//! SmileBASIC 4's glyphs fill a square cell, so they cannot be cut in
//! half. The 8-pixel ones come from GalmuriMono7, a pixel font drawn for
//! 4x8 half-width cells. The 16-pixel ones squeeze the font's own glyph to
//! half its width, which keeps its look; characters the font lacks, such
//! as half-width kana, take GalmuriMono7's doubled.

use crate::font::{self, Font, FontSize};

/// Whether a character is half-width: ASCII, and the half-width forms
/// that GalmuriMono7 draws in a half cell. Latin-1 and other characters
/// East Asian fonts may draw either way stay full-width, as in
/// SmileBASIC 4.
#[inline]
pub fn is_half_width(code: u16) -> bool {
    slot(code).is_some()
}

/// A half-width character's place in the tables.
#[inline]
fn slot(code: u16) -> Option<usize> {
    match code {
        0x20..=0x7E => Some(usize::from(code - 0x20)),
        // Half-width katakana and punctuation.
        0xFF61..=0xFF9F => Some(usize::from(code - 0xFF61) + 95),
        // ￨, ￭, ￮ and ₩.
        0xFFE8 => Some(158),
        0xFFED => Some(159),
        0xFFEE => Some(160),
        0x20A9 => Some(161),
        _ => None,
    }
}

const SLOTS: usize = 162;
const SMALL: usize = 4 * 8;
const LARGE: usize = 8 * 16;

/// The half-width glyphs of one font, one alpha byte per pixel.
pub struct HalfGlyphs {
    /// 4x8 cells.
    small: Vec<u8>,
    /// 8x16 cells.
    large: Vec<u8>,
}

impl HalfGlyphs {
    pub(crate) fn new(font: &Font) -> HalfGlyphs {
        let mut small = vec![0; SLOTS * SMALL];
        let mut large = vec![0; SLOTS * LARGE];
        let codes = (0..=u16::MAX).filter_map(|c| slot(c).map(|s| (c, s)));
        for (code, s) in codes {
            let half = &mut small[s * SMALL..(s + 1) * SMALL];
            if let Some(rows) = galmuri(code) {
                // GalmuriMono7's half-width glyphs sit in columns 2 to 5
                // of the fallback font's 8x8 cell (see build.rs).
                for (y, row) in rows.iter().enumerate() {
                    for x in 0..4 {
                        if row & (0x80 >> (x + 2)) != 0 {
                            half[y * 4 + x] = 255;
                        }
                    }
                }
            }
            let big = &mut large[s * LARGE..(s + 1) * LARGE];
            if (0x21..=0x7E).contains(&code) && font.has_glyph(code) {
                for y in 0..16 {
                    for x in 0..8 {
                        let alpha = |x| font.pixel(code, FontSize::Large, x, y) >> 24;
                        let pair = alpha(2 * x) + alpha(2 * x + 1);
                        big[(y * 8 + x) as usize] = pair.div_ceil(2) as u8;
                    }
                }
            } else {
                for (i, a) in big.iter_mut().enumerate() {
                    *a = half[i / 16 * 4 + i % 8 / 2];
                }
            }
        }
        HalfGlyphs { small, large }
    }

    /// The glyph of a half-width character at `size`: `size / 2` pixels
    /// wide and `size` high, row by row. `None` for other characters.
    #[inline]
    pub fn get(&self, code: u16, size: FontSize) -> Option<&[u8]> {
        let s = slot(code)?;
        Some(match size {
            FontSize::Small => &self.small[s * SMALL..(s + 1) * SMALL],
            FontSize::Large => &self.large[s * LARGE..(s + 1) * LARGE],
        })
    }
}

/// The rows of GalmuriMono7's 8x8 cell for `code`, if it has one.
fn galmuri(code: u16) -> Option<&'static [u8]> {
    let records = font::FALLBACK;
    let count = records.len() / 10;
    let code_at = |i: usize| u16::from_le_bytes([records[i * 10], records[i * 10 + 1]]);
    let (mut lo, mut hi) = (0, count);
    while lo < hi {
        let mid = (lo + hi) / 2;
        match code_at(mid).cmp(&code) {
            std::cmp::Ordering::Less => lo = mid + 1,
            std::cmp::Ordering::Greater => hi = mid,
            std::cmp::Ordering::Equal => return Some(&records[mid * 10 + 2..mid * 10 + 10]),
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ink(glyph: &[u8]) -> usize {
        glyph.iter().filter(|&&a| a >= 0x80).count()
    }

    #[test]
    fn half_width_characters() {
        for c in [' ', 'A', '~', 'ｱ', '｡', 'ﾟ', '￨', '￮', '₩'] {
            assert!(is_half_width(c as u16), "{c}");
        }
        for c in ['\u{1F}', '\u{7F}', 'é', 'Ａ', 'ア', '가', '漢', '\u{FFA0}'] {
            assert!(!is_half_width(c as u16), "{c}");
        }
    }

    #[test]
    fn every_half_width_character_has_a_glyph() {
        let half = HalfGlyphs::new(&Font::fallback());
        for code in (0..=u16::MAX).filter(|&c| is_half_width(c) && c != 0x20) {
            for size in [FontSize::Small, FontSize::Large] {
                let glyph = half.get(code, size).unwrap();
                assert_eq!(glyph.len(), (size as usize / 2) * size as usize);
                assert!(ink(glyph) > 0, "U+{code:04X} at {size:?} is blank");
            }
        }
        assert_eq!(ink(half.get(0x20, FontSize::Large).unwrap()), 0);
        assert_eq!(half.get('Ａ' as u16, FontSize::Small), None);
    }

    #[test]
    fn large_glyphs_squeeze_the_fonts_own() {
        // A page font whose A is a full 16x16 square.
        let mut font = Font::blank();
        let (gx, gy) = font.origin(0x41, FontSize::Large).unwrap();
        let page = font.page_mut();
        for y in gy..gy + 16 {
            for x in gx..gx + 16 {
                page[(y * font::PAGE_SIZE + x) as usize] = 0xFFFF_FFFF;
            }
        }
        let half = HalfGlyphs::new(&font);
        assert_eq!(ink(half.get(0x41, FontSize::Large).unwrap()), 8 * 16);
        // B is blank there, as the font draws it.
        assert_eq!(ink(half.get(0x42, FontSize::Large).unwrap()), 0);
        // Half-width kana, which the font lacks, come from GalmuriMono7.
        assert!(ink(half.get('ｱ' as u16, FontSize::Large).unwrap()) > 0);
    }
}
