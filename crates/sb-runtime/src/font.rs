//! The system font: `font.map` maps character codes to glyph numbers, and
//! the `GDEFFONT` page holds the glyph images.
//!
//! Both files come from the user's own copy of SmileBASIC 4 (see
//! [`crate::resources`]); nothing from them is part of this repository.
//! Without them, [`Font::fallback`] draws with GalmuriMono7 instead.
//!
//! Glyph `n` of the 8-pixel font is at `((n % 64) * 8, (n / 64) * 8)` on the
//! page, and of the 16-pixel font at `((n % 64) * 16, 1024 + (n / 64) * 16)`.
//! This matches FONTINFO on real hardware.

use std::fmt;

use crate::sbfile::{self, DataValues, FileError};

/// Width and height of the font page in pixels.
pub const PAGE_SIZE: u32 = 2048;
/// Glyph shown for codes the font does not cover.
pub const MISSING_GLYPH: u16 = 703;
const CODES: usize = 0x1_0000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FontSize {
    Small = 8,
    Large = 16,
}

pub struct Font(Source);

enum Source {
    /// SmileBASIC 4's own font.
    Page {
        glyphs: Vec<u16>,
        /// `PAGE_SIZE` x `PAGE_SIZE` pixels, `0xAARRGGBB`.
        page: Vec<u32>,
    },
    /// 8x8 cells, drawn doubled for the 16-pixel font.
    Cells {
        /// For each code, an index into `cells`, `SYMBOL | i` for
        /// `symbols[i]`, or `NO_CELL`.
        index: Vec<u16>,
        /// One-bit rows, leftmost pixel in bit 7.
        cells: Vec<[u8; 8]>,
        /// Indices into `SYMBOL_PALETTE`, row by row.
        symbols: Vec<[u8; 64]>,
        missing: u16,
    },
}

const NO_CELL: u16 = u16::MAX;
const SYMBOL: u16 = 0x8000;
/// Colors of `fonts/symbols.txt`: transparent, white, black, gray, sky blue.
const SYMBOL_PALETTE: [u32; 5] = [0, 0xFFFF_FFFF, 0xFF00_0000, 0xFFBF_BFBF, 0xFF00_BFFF];
/// Built by `build.rs` from `fonts/galmuri`: records of a u16 code and
/// 8 row bytes.
static FALLBACK: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/fallback_font.bin"));
/// Built by `build.rs` from `fonts/symbols.txt`: records of a u16 code and
/// 64 palette indices.
static SYMBOLS: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/fallback_symbols.bin"));

#[derive(Debug)]
pub enum FontError {
    /// `font.map` is neither the 1.x nor the 4.x layout.
    MapSize(usize),
    Page(FileError),
    /// The page is not a 2048x2048 integer array.
    PageShape,
}

impl fmt::Display for FontError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FontError::MapSize(n) => write!(f, "font.map has an unknown size ({n} bytes)"),
            FontError::Page(e) => write!(f, "font page: {e}"),
            FontError::PageShape => write!(f, "font page is not a 2048x2048 image"),
        }
    }
}

impl std::error::Error for FontError {}

impl From<FileError> for FontError {
    fn from(e: FileError) -> Self {
        FontError::Page(e)
    }
}

/// Reads `font.map`. Version 1.x stores one u16 glyph number per code;
/// 4.x stores a u32 per code whose low 12 bits are the glyph number.
/// The upper bits of 4.x entries (probably width and flags) are unused.
pub fn parse_map(bytes: &[u8]) -> Result<Vec<u16>, FontError> {
    let stride = match bytes.len() {
        n if n == CODES * 2 => 2,
        n if n == CODES * 4 => 4,
        n => return Err(FontError::MapSize(n)),
    };
    Ok(bytes
        .chunks_exact(stride)
        .map(|e| u16::from_le_bytes([e[0], e[1]]) & 0x0FFF)
        .collect())
}

impl Font {
    /// Builds the font from the bytes of `font.map` and of the `GDEFFONT`
    /// file (a SmileBASIC 4 DAT container).
    pub fn from_files(map: &[u8], page: &[u8]) -> Result<Font, FontError> {
        let glyphs = parse_map(map)?;
        let data = sbfile::parse(page)?.data()?;
        let side = PAGE_SIZE as usize;
        let page = match data.values {
            DataValues::Int(px) if data.dims == [side, side] => {
                px.into_iter().map(|p| p as u32).collect()
            }
            _ => return Err(FontError::PageShape),
        };
        Ok(Font(Source::Page { glyphs, page }))
    }

    /// The bundled GalmuriMono7 plus this emulator's drawings of
    /// SmileBASIC's own characters, for when no font has been imported.
    pub fn fallback() -> Font {
        let mut index = vec![NO_CELL; CODES];
        let mut cells = Vec::with_capacity(FALLBACK.len() / 10);
        for record in FALLBACK.chunks_exact(10) {
            let code = u16::from_le_bytes([record[0], record[1]]);
            index[usize::from(code)] = cells.len() as u16;
            cells.push(record[2..].try_into().expect("8 rows"));
        }
        let mut symbols = Vec::with_capacity(SYMBOLS.len() / 66);
        for record in SYMBOLS.chunks_exact(66) {
            let code = u16::from_le_bytes([record[0], record[1]]);
            index[usize::from(code)] = SYMBOL | symbols.len() as u16;
            symbols.push(record[2..].try_into().expect("64 pixels"));
        }
        let missing = index[0xFFFD];
        Font(Source::Cells {
            index,
            cells,
            symbols,
            missing,
        })
    }

    /// Top-left corner of a code's glyph on SmileBASIC 4's font page, as
    /// FONTINFO reports it. `None` for the fallback font, which has no page.
    pub fn origin(&self, code: u16, size: FontSize) -> Option<(u32, u32)> {
        let Source::Page { glyphs, .. } = &self.0 else {
            return None;
        };
        let n = u32::from(glyphs[usize::from(code)]);
        Some(match size {
            FontSize::Small => ((n % 64) * 8, (n / 64) * 8),
            FontSize::Large => ((n % 64) * 16, 1024 + (n / 64) * 16),
        })
    }

    /// Pixel `(x, y)` of a code's glyph, `0xAARRGGBB`; both coordinates
    /// must be below the font size.
    pub fn pixel(&self, code: u16, size: FontSize, x: u32, y: u32) -> u32 {
        match &self.0 {
            Source::Page { page, .. } => {
                let (gx, gy) = self.origin(code, size).expect("page font");
                page[((gy + y) * PAGE_SIZE + gx + x) as usize]
            }
            Source::Cells {
                index,
                cells,
                symbols,
                missing,
            } => {
                let i = match index[usize::from(code)] {
                    NO_CELL => *missing,
                    i => i,
                };
                let (x, y) = match size {
                    FontSize::Small => (x, y),
                    FontSize::Large => (x / 2, y / 2),
                };
                if i & SYMBOL != 0 {
                    let pixels = &symbols[usize::from(i & !SYMBOL)];
                    return SYMBOL_PALETTE[usize::from(pixels[(y * 8 + x) as usize])];
                }
                let set = cells[usize::from(i)][y as usize] & (0x80 >> x) != 0;
                if set { 0xFFFF_FFFF } else { 0 }
            }
        }
    }

    /// A page font with every code mapped to glyph `code % 4096` on a
    /// blank page, for tests that do not have the real files.
    pub fn blank() -> Font {
        Font(Source::Page {
            glyphs: (0..CODES).map(|c| (c % 4096) as u16).collect(),
            page: vec![0; (PAGE_SIZE * PAGE_SIZE) as usize],
        })
    }

    #[cfg(test)]
    pub(crate) fn page_mut(&mut self) -> &mut [u32] {
        match &mut self.0 {
            Source::Page { page, .. } => page,
            Source::Cells { .. } => unreachable!("tests draw on page fonts"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn map_layouts() {
        let mut v1 = vec![0u8; CODES * 2];
        v1[0x41 * 2] = 65;
        assert_eq!(parse_map(&v1).unwrap()[0x41], 65);

        let mut v4 = vec![0u8; CODES * 4];
        // Glyph 305 with flag bits set, as stored for full-width kana.
        v4[0x3042 * 4..0x3042 * 4 + 4].copy_from_slice(&[0x31, 0x81, 0x03, 0x07]);
        assert_eq!(parse_map(&v4).unwrap()[0x3042], 305);

        assert!(matches!(parse_map(&[0; 3]), Err(FontError::MapSize(3))));
    }

    #[test]
    fn glyph_origins() {
        let mut font = Font::blank();
        if let Source::Page { glyphs, .. } = &mut font.0 {
            glyphs[0x6F22] = 1432;
        }
        // FONTINFO "漢",16 returns 384,1376 on hardware.
        assert_eq!(font.origin(0x6F22, FontSize::Large), Some((384, 1376)));
        assert_eq!(font.origin(0x41, FontSize::Small), Some((8, 8)));
    }

    fn rows(font: &Font, code: char) -> Vec<String> {
        (0..8)
            .map(|y| {
                (0..8)
                    .map(|x| match font.pixel(code as u16, FontSize::Small, x, y) {
                        0 => '.',
                        _ => '#',
                    })
                    .collect()
            })
            .collect()
    }

    #[test]
    fn fallback_font() {
        let font = Font::fallback();
        assert_eq!(font.origin(0x41, FontSize::Small), None);
        // Half-width glyphs are centered in the cell.
        assert_eq!(rows(&font, '!')[0], "...#....");
        // The overrides make look-alikes distinct.
        assert_ne!(rows(&font, '０'), rows(&font, 'Ｏ'));
        assert_ne!(rows(&font, 'Ｉ'), rows(&font, '｜'));
        assert_ne!(rows(&font, 'ｌ'), rows(&font, '｜'));
        // Codes the font lacks, such as Cyrillic, draw U+FFFD.
        assert_eq!(rows(&font, 'Ж'), rows(&font, '\u{FFFD}'));
        // SmileBASIC's own characters come from symbols.txt, with colors.
        assert_eq!(font.pixel(0xE10A, FontSize::Small, 0, 0), 0xFFFF_FFFF);
        assert_eq!(font.pixel(0xE10A, FontSize::Small, 2, 1), 0xFF00_0000);
        assert_eq!(font.pixel(0xE100, FontSize::Small, 1, 1), 0xFFBF_BFBF);
        assert_eq!(
            font.pixel('A' as u16, FontSize::Large, 6, 1),
            font.pixel('A' as u16, FontSize::Small, 3, 0)
        );
    }
}
