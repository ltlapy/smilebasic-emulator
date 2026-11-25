//! Glyphs half a cell wide for half-width characters, which SmileBASIC 4
//! draws as wide as any other. The emulator's own windows and menus use
//! them, and so does the editor when its half-width setting is on;
//! programs never see them.
//!
//! SmileBASIC 4's glyphs fill a square cell, so they cannot be cut in
//! half. The glyphs come from fonts drawn for half-width cells: the 8-pixel
//! ones from GalmuriMono7 (4x8), the 16-pixel ones from NeoDunggeunmo
//! (8x16), then Unifont JP for the characters it lacks. A font file chosen
//! in the settings comes before or after them, as the editor's font order
//! says, for the glyphs it draws half-width. Squeezing SmileBASIC's own
//! glyph to half its width is the last resort.

use std::sync::OnceLock;

use ab_glyph::FontRef;

use crate::cell_glyph;
use crate::extra_glyphs::{self, ExtraGlyphs, FontSource};
use crate::font::{self, Font, FontSize};

/// NeoDunggeunmo, the bundled 16-pixel dot font (see `fonts/neodgm`),
/// whose half-width glyphs are drawn for 8x16 cells.
/// Compressed by `build.rs`, and unpacked on first use.
static HALF_FONT: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/dot_font.bin"));

fn half_font() -> &'static FontRef<'static> {
    static BYTES: OnceLock<Vec<u8>> = OnceLock::new();
    static FONT: OnceLock<FontRef<'static>> = OnceLock::new();
    FONT.get_or_init(|| {
        let bytes = BYTES.get_or_init(|| {
            miniz_oxide::inflate::decompress_to_vec(HALF_FONT).expect("bundled font")
        });
        FontRef::try_from_slice(bytes).expect("bundled font")
    })
}

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
        0xFF61..=0xFF9F => Some(usize::from(code - 0xFF61) + FIRST_KANA_SLOT),
        // ￨, ￭, ￮ and ₩.
        0xFFE8 => Some(FIRST_SYMBOL_SLOT),
        0xFFED => Some(FIRST_SYMBOL_SLOT + 1),
        0xFFEE => Some(FIRST_SYMBOL_SLOT + 2),
        0x20A9 => Some(FIRST_SYMBOL_SLOT + 3),
        _ => None,
    }
}

/// The slot of U+FF61, after the slots of ASCII (U+0020 to U+007E).
const FIRST_KANA_SLOT: usize = 0x7F - 0x20;
/// The slot of U+FFE8, after the slots of U+FF61 to U+FF9F.
const FIRST_SYMBOL_SLOT: usize = FIRST_KANA_SLOT + (0xFFA0 - 0xFF61);
const SLOTS: usize = FIRST_SYMBOL_SLOT + 4;
/// The first column of GalmuriMono7's half-width glyphs in the fallback
/// font's 8x8 cell: `HALF_LEFT` in build.rs, which centers them.
const GALMURI_HALF_LEFT: usize = 2;
const SMALL: usize = 4 * 8;
const LARGE: usize = 8 * 16;

/// The half-width glyphs of one font, one alpha byte per pixel.
pub struct HalfGlyphs {
    /// 4x8 cells.
    small: Vec<u8>,
    /// 8x16 cells.
    large: Vec<u8>,
}

impl std::fmt::Debug for HalfGlyphs {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("HalfGlyphs")
    }
}

impl HalfGlyphs {
    /// The bundled glyphs, then `font`'s glyphs squeezed.
    pub(crate) fn new(font: &Font) -> HalfGlyphs {
        HalfGlyphs::with_file(font, None, FontSource::SmileBasic)
    }

    /// [`HalfGlyphs::new`] with the half-width glyphs of a chosen font
    /// file: ahead of the bundled ones unless `first` puts those first.
    pub fn with_file(font: &Font, file: Option<&ExtraGlyphs>, first: FontSource) -> HalfGlyphs {
        let mut small = vec![0; SLOTS * SMALL];
        let mut large = vec![0; SLOTS * LARGE];
        let file_first = first != FontSource::Bundled;
        let from_file = |code: u16, cell| file.and_then(|f| f.file_half(u32::from(code), cell));
        let codes = (0..=u16::MAX).filter_map(|c| slot(c).map(|s| (c, s)));
        for (code, s) in codes {
            let half = &mut small[s * SMALL..(s + 1) * SMALL];
            let mut galmuri_small = [0; SMALL];
            if let Some(rows) = font::fallback_rows(code) {
                for (y, row) in rows.iter().enumerate() {
                    for x in 0..4 {
                        if row & (0x80 >> (x + GALMURI_HALF_LEFT)) != 0 {
                            galmuri_small[y * 4 + x] = 255;
                        }
                    }
                }
            }
            let file_small = from_file(code, FontSize::Small as u32);
            let bundled_small = font::fallback_rows(code).map(|_| galmuri_small.to_vec());
            let found = match file_first {
                true => file_small.or(bundled_small),
                false => bundled_small.or(file_small),
            };
            if let Some(glyph) = found {
                half.copy_from_slice(&glyph);
            }

            let big = &mut large[s * LARGE..(s + 1) * LARGE];
            let file_large = || from_file(code, FontSize::Large as u32);
            let bundled = || {
                char::from_u32(u32::from(code))
                    .and_then(|c| cell_glyph::outline_half(half_font(), c, 16))
                    .or_else(|| extra_glyphs::dot_half16(u32::from(code)))
            };
            let found = match file_first {
                true => file_large().or_else(bundled),
                false => bundled().or_else(file_large),
            };
            match found {
                Some(glyph) => big.copy_from_slice(&glyph),
                None if font.has_glyph(code) => {
                    for y in 0..16 {
                        for x in 0..8 {
                            let alpha = |x| font.pixel(code, FontSize::Large, x, y) >> 24;
                            let pair = alpha(2 * x) + alpha(2 * x + 1);
                            big[(y * 8 + x) as usize] = pair.div_ceil(2) as u8;
                        }
                    }
                }
                None => {}
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

#[cfg(test)]
mod tests {
    use super::*;

    fn ink(glyph: &[u8]) -> usize {
        glyph.iter().filter(|&&a| a >= 0x80).count()
    }

    /// An 8x16 glyph with each 2x2 block set to its top left pixel: the
    /// glyph itself if it was drawn at 4x8 and doubled.
    fn doubled(glyph: &[u8]) -> Vec<u8> {
        (0..LARGE)
            .map(|i| glyph[i / 16 * 16 + i % 8 / 2 * 2])
            .collect()
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
    fn large_glyphs_come_from_a_half_width_font() {
        // A page font whose A is a full 16x16 square: the bundled glyph
        // wins over squeezing it.
        let mut font = Font::blank();
        let (gx, gy) = font.origin(0x41, FontSize::Large).unwrap();
        let page = font.page_mut();
        for y in gy..gy + 16 {
            for x in gx..gx + 16 {
                page[(y * font::PAGE_SIZE + x) as usize] = 0xFFFF_FFFF;
            }
        }
        let half = HalfGlyphs::new(&font);
        let a = half.get(0x41, FontSize::Large).unwrap();
        assert!(ink(a) > 10 && ink(a) < 8 * 16, "A has {} pixels", ink(a));
        // NeoDunggeunmo is a pixel font: every pixel is on or off.
        assert!(a.iter().all(|&p| p == 0 || p == 255));
        // Half-width kana, which it lacks, come from Unifont JP, drawn
        // for 16 pixels: no pixel is doubled.
        let kana = half.get('ｱ' as u16, FontSize::Large).unwrap();
        assert!(ink(kana) > 0);
        assert_ne!(kana, &doubled(kana)[..]);
    }

    /// A BDF font with A as a filled 4x8 box and Ａ, full-width, 8x8.
    const BDF: &str = "STARTFONT 2.1\nFONT_ASCENT 8\nFONT_DESCENT 0\nCHARS 2\n\
        STARTCHAR A\nENCODING 65\nDWIDTH 4 0\nBBX 4 8 0 0\nBITMAP\nF0\nF0\nF0\nF0\nF0\nF0\nF0\nF0\nENDCHAR\n\
        STARTCHAR M\nENCODING 65325\nDWIDTH 8 0\nBBX 8 8 0 0\nBITMAP\nFF\nFF\nFF\nFF\nFF\nFF\nFF\nFF\nENDCHAR\nENDFONT\n";

    #[test]
    fn a_chosen_files_half_width_glyphs_follow_the_order() {
        let file = ExtraGlyphs::from_bytes(BDF.as_bytes().to_vec()).unwrap();
        let font = Font::fallback();
        for first in [FontSource::SmileBasic, FontSource::File] {
            let half = HalfGlyphs::with_file(&font, Some(&file), first);
            assert_eq!(ink(half.get(0x41, FontSize::Small).unwrap()), SMALL);
            // At 16 pixels the 4x8 glyph keeps its size, centered.
            assert_eq!(ink(half.get(0x41, FontSize::Large).unwrap()), SMALL);
            // B is not in the file.
            let b = half.get(0x42, FontSize::Large).unwrap();
            assert_eq!(
                b,
                HalfGlyphs::new(&font).get(0x42, FontSize::Large).unwrap()
            );
        }
        let half = HalfGlyphs::with_file(&font, Some(&file), FontSource::Bundled);
        assert!(ink(half.get(0x41, FontSize::Large).unwrap()) < LARGE);
    }
}
