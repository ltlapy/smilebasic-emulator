//! The system font: `font.map` maps character codes to glyph numbers, and
//! the `GDEFFONT` page holds the glyph images.
//!
//! Both files come from the user's own copy of SmileBASIC 4 (see
//! [`crate::resources`]); nothing from them is part of this repository.
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

pub struct Font {
    glyphs: Vec<u16>,
    /// `PAGE_SIZE` x `PAGE_SIZE` pixels, `0xAARRGGBB`.
    page: Vec<u32>,
}

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
        Ok(Font { glyphs, page })
    }

    /// Glyph number for a character code.
    pub fn glyph(&self, code: u16) -> u16 {
        self.glyphs[usize::from(code)]
    }

    /// Top-left corner of a code's glyph on the page.
    pub fn origin(&self, code: u16, size: FontSize) -> (u32, u32) {
        let n = u32::from(self.glyph(code));
        match size {
            FontSize::Small => ((n % 64) * 8, (n / 64) * 8),
            FontSize::Large => ((n % 64) * 16, 1024 + (n / 64) * 16),
        }
    }

    /// Page pixel at `(x, y)`; both must be below [`PAGE_SIZE`].
    pub fn pixel(&self, x: u32, y: u32) -> u32 {
        self.page[(y * PAGE_SIZE + x) as usize]
    }

    /// A font with every code mapped to glyph `code % 4096` on a blank
    /// page, for tests that do not have the real files.
    pub fn blank() -> Font {
        Font {
            glyphs: (0..CODES).map(|c| (c % 4096) as u16).collect(),
            page: vec![0; (PAGE_SIZE * PAGE_SIZE) as usize],
        }
    }

    #[cfg(test)]
    pub(crate) fn page_mut(&mut self) -> &mut [u32] {
        &mut self.page
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
        font.glyphs[0x6F22] = 1432;
        // FONTINFO "漢",16 returns 384,1376 on hardware.
        assert_eq!(font.origin(0x6F22, FontSize::Large), (384, 1376));
        assert_eq!(font.origin(0x41, FontSize::Small), (8, 8));
    }
}
