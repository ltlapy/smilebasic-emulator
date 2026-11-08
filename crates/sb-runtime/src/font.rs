//! The system font: `font.map` maps character codes to glyph numbers, and
//! the `GDEFFONT` page holds the glyph images.
//!
//! Both files come from the user's own copy of SmileBASIC 4 (see
//! [`crate::resources`]); nothing from them is part of this repository.
//! Without them, [`Font::fallback`] draws the 8-pixel font with
//! GalmuriMono7 and the 16-pixel one with M PLUS 1p Bold, the face
//! SmileBASIC 4's own 16-pixel font is drawn from, then Galmuri14.
//!
//! Glyph `n` of the 8-pixel font is at `((n % 64) * 8, (n / 64) * 8)` on the
//! page, and of the 16-pixel font at `((n % 64) * 16, 1024 + (n / 64) * 16)`.
//! This matches FONTINFO on real hardware.

use std::cell::{Cell, OnceCell};
use std::collections::HashMap;
use std::fmt;
use std::rc::Rc;
use std::sync::{Mutex, OnceLock};

use ab_glyph::{Font as _, FontRef, PxScale, point};

use crate::graphics::Graphics;
use crate::half_width::HalfGlyphs;
use crate::sbfile::{self, DataValues, FileError};

/// Width and height of the font page in pixels.
pub const PAGE_SIZE: u32 = 2048;
/// Glyph shown for codes the font does not cover.
pub const MISSING_GLYPH: u16 = 703;
const CODES: usize = 0x1_0000;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FontSize {
    Small = 8,
    Large = 16,
}

#[derive(Clone)]
pub struct Font(Source, Pages);

/// What is made from the font the first time it is wanted and kept: the
/// font page on GRP5 of pages otherwise blank, for text drawn with the
/// font alone, as the editor's (copying the page for every frame took
/// longer than drawing), the half-width glyphs, and the [`Span`]s of
/// fonts without font.map's metrics.
#[derive(Clone, Default)]
struct Pages(
    OnceCell<Rc<Graphics>>,
    OnceCell<Rc<HalfGlyphs>>,
    OnceCell<Rc<[Cell<u16>]>>,
);

/// GPUTCHRP's view of a character: it draws columns `first..` of the
/// glyph, as many as `width` and the cell allow, and then moves on
/// `width` pixels before the spacing. Fixed for each code:
/// it does not follow what a program draws on GRP5.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Span {
    pub first: i32,
    pub width: i32,
}

impl Span {
    /// From a 4.x font.map entry's bits 12 to 27 (see [`parse_map`]):
    /// at 8 pixels bits 0-2 are the first column and 3-6 the width, at
    /// 16 pixels bits 7-10 the first column and 11-15 the width.
    fn from_metrics(m: u16, size: FontSize) -> Span {
        let (first, width) = match size {
            FontSize::Small => (m & 0x7, m >> 3 & 0xF),
            FontSize::Large => (m >> 7 & 0xF, m >> 11),
        };
        Span {
            first: i32::from(first),
            width: i32::from(width),
        }
    }

    /// Packed for [`Pages`]; never 0, as a width is at least 1.
    fn pack(self) -> u16 {
        (self.first as u16) << 8 | self.width as u16
    }

    fn unpack(v: u16) -> Span {
        Span {
            first: i32::from(v >> 8),
            width: i32::from(v & 0xFF),
        }
    }
}

#[derive(Clone)]
enum Source {
    /// SmileBASIC 4's own font.
    Page {
        glyphs: Vec<u16>,
        /// Each code's GPUTCHRP metrics (see [`Span::from_metrics`]);
        /// 1.x maps have none.
        metrics: Option<Vec<u16>>,
        /// `PAGE_SIZE` x `PAGE_SIZE` pixels, `0xAARRGGBB`.
        page: Vec<u32>,
    },
    /// 8x8 cells; the 16-pixel font is drawn from M PLUS 1p Bold and
    /// Galmuri14, and what they lack is the 8x8 cell at twice its size.
    Cells {
        /// For each code, an index into `cells`, `SYMBOL | i` for
        /// `symbols[i]`, or `NO_CELL`.
        index: Vec<u16>,
        /// One-bit rows, leftmost pixel in bit 7.
        cells: Vec<[u8; 8]>,
        /// Indices into `SYMBOL_PALETTE`, row by row.
        symbols: Vec<[u8; 64]>,
        missing: u16,
        /// For each code, its glyph number on the font page, or `NO_CELL`
        /// for codes drawn straight from `cells`: the page has room for
        /// only `PAGE_GLYPHS` of them.
        slots: Vec<u16>,
    },
}

/// Glyphs the 16-pixel half of the font page holds.
const PAGE_GLYPHS: usize = 4096;
/// Codes the fallback font puts on the font page first; the rest follow in
/// code order while there is room.
const FIRST_ON_PAGE: &[std::ops::RangeInclusive<u16>] = &[
    0x20..=0x7E,
    0xFFFD..=0xFFFD,
    0xE100..=0xE17F,
    0xE200..=0xE2FF,
    0xA0..=0x24F,
    0x370..=0x4FF,
    0x2000..=0x2BFF,
    0x3000..=0x30FF,
    0xFF00..=0xFFEF,
];

/// The 16-pixel fallback font's outlines (see `fonts/mplus`).
pub(crate) static OUTLINES: &[u8] = include_bytes!("../fonts/mplus/SBEmulator1p-Bold.ttf");
/// Top of the outline font's em box above the baseline, in font units of
/// 1000: M+'s typographic ascender, 860 over a descender of -140.
const EM_TOP: f32 = 860.0;
/// SmileBASIC 4 draws its 16-pixel font with a 15-pixel em in the top left
/// of the cell.
const EM_PIXELS: f32 = 15.0;
const LARGE: usize = FontSize::Large as usize;

/// A 16-pixel glyph drawn from the outlines: one alpha byte per pixel, in
/// the 16 steps of SmileBASIC 4's font page.
type Bitmap = [u8; LARGE * LARGE];

pub(crate) fn outlines() -> &'static FontRef<'static> {
    static FONT: OnceLock<FontRef<'static>> = OnceLock::new();
    FONT.get_or_init(|| FontRef::try_from_slice(OUTLINES).expect("bundled font"))
}

/// The 16-pixel glyph of `c` from the outline font, or else from
/// Galmuri14; `None` if neither has it. Drawing is slow next to looking
/// pixels up, so glyphs are kept once drawn.
fn large_glyph(c: char) -> Option<Bitmap> {
    static DRAWN: OnceLock<Mutex<HashMap<char, Option<Bitmap>>>> = OnceLock::new();
    let drawn = DRAWN.get_or_init(Default::default);
    if let Some(&bitmap) = drawn.lock().expect("glyph cache").get(&c) {
        return bitmap;
    }
    let bitmap = draw_outline(c).or_else(|| {
        let mut cell = Vec::with_capacity(LARGE * LARGE);
        crate::extra_glyphs::galmuri16_cell(c as u32, &mut cell)
            .then(|| cell.try_into().expect("16x16 cell"))
    });
    drawn.lock().expect("glyph cache").insert(c, bitmap);
    bitmap
}

fn draw_outline(c: char) -> Option<Bitmap> {
    let font = outlines();
    let id = font.glyph_id(c);
    if id.0 == 0 {
        return None;
    }
    let em = font.units_per_em().unwrap_or(1000.0);
    // ab_glyph scales by ascent minus descent; one em should be EM_PIXELS.
    let scale = PxScale::from(EM_PIXELS * font.height_unscaled() / em);
    // Narrow glyphs sit in the middle of the em.
    let advance = font.h_advance_unscaled(id) * EM_PIXELS / em;
    let origin = point((EM_PIXELS - advance) / 2.0, EM_PIXELS * EM_TOP / em);
    let mut bitmap = [0; LARGE * LARGE];
    if let Some(outline) = font.outline_glyph(id.with_scale_and_position(scale, origin)) {
        let min = outline.px_bounds().min;
        outline.draw(|x, y, coverage| {
            let (x, y) = (min.x as i32 + x as i32, min.y as i32 + y as i32);
            if (0..LARGE as i32).contains(&x) && (0..LARGE as i32).contains(&y) {
                bitmap[y as usize * LARGE + x as usize] =
                    (coverage.min(1.0) * 15.0).round() as u8 * 17;
            }
        });
    }
    Some(bitmap)
}

const NO_CELL: u16 = u16::MAX;
const SYMBOL: u16 = 0x8000;
/// Colors of `fonts/symbols.txt`: transparent, white, black, gray, sky blue.
const SYMBOL_PALETTE: [u32; 5] = [0, 0xFFFF_FFFF, 0xFF00_0000, 0xFFBF_BFBF, 0xFF00_BFFF];
/// Built by `build.rs` from `fonts/galmuri`: records of a u16 code and
/// 8 row bytes.
pub(crate) static FALLBACK: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/fallback_font.bin"));
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

/// Reads `font.map`: each code's glyph number, and from 4.x maps its
/// GPUTCHRP metrics. Version 1.x stores one u16 glyph number per code;
/// 4.x stores a u32 per code, bits 0-11 being the glyph number and bits
/// 12-27 the metrics, kept here as a u16.
pub fn parse_map(bytes: &[u8]) -> Result<(Vec<u16>, Option<Vec<u16>>), FontError> {
    match bytes.len() {
        n if n == CODES * 2 => Ok((
            bytes
                .chunks_exact(2)
                .map(|e| u16::from_le_bytes([e[0], e[1]]) & 0x0FFF)
                .collect(),
            None,
        )),
        n if n == CODES * 4 => {
            let entry = |e: &[u8]| u32::from_le_bytes([e[0], e[1], e[2], e[3]]);
            Ok((
                bytes
                    .chunks_exact(4)
                    .map(|e| entry(e) as u16 & 0x0FFF)
                    .collect(),
                Some(
                    bytes
                        .chunks_exact(4)
                        .map(|e| (entry(e) >> 12) as u16)
                        .collect(),
                ),
            ))
        }
        n => Err(FontError::MapSize(n)),
    }
}

/// Where glyph `n` sits on the font page.
fn slot_origin(n: u32, size: FontSize) -> (u32, u32) {
    match size {
        FontSize::Small => ((n % 64) * 8, (n / 64) * 8),
        FontSize::Large => ((n % 64) * 16, 1024 + (n / 64) * 16),
    }
}

impl Font {
    /// Builds the font from the bytes of `font.map` and of the `GDEFFONT`
    /// file (a SmileBASIC 4 DAT container).
    pub fn from_files(map: &[u8], page: &[u8]) -> Result<Font, FontError> {
        let (glyphs, metrics) = parse_map(map)?;
        let data = sbfile::parse(page)?.data()?;
        let side = PAGE_SIZE as usize;
        let page = match data.values {
            DataValues::Int(px) if data.dims == [side, side] => {
                px.into_iter().map(|p| p as u32).collect()
            }
            _ => return Err(FontError::PageShape),
        };
        let source = Source::Page {
            glyphs,
            metrics,
            page,
        };
        Ok(Font(source, Pages::default()))
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
        let mut slots = vec![NO_CELL; CODES];
        let mut used = 0;
        let order = FIRST_ON_PAGE.iter().cloned().flatten().chain(0..=u16::MAX);
        for code in order {
            if used == PAGE_GLYPHS {
                break;
            }
            let drawn = match code {
                0x21..=0x7E => code + 0xFEE0,
                _ => code,
            };
            if slots[usize::from(code)] == NO_CELL && index[usize::from(drawn)] != NO_CELL {
                slots[usize::from(code)] = used as u16;
                used += 1;
            }
        }
        Font(
            Source::Cells {
                index,
                cells,
                symbols,
                missing,
                slots,
            },
            Pages::default(),
        )
    }

    /// Pages with only GRP5, the font page.
    pub fn pages(&self) -> &Graphics {
        self.1.0.get_or_init(|| {
            let mut grp = Graphics::default();
            grp.set_page(crate::text::FONT_PAGE, self.page_image());
            Rc::new(grp)
        })
    }

    /// Half-width glyphs that go with this font, for text that is not the
    /// programs'.
    pub fn half_glyphs(&self) -> &HalfGlyphs {
        self.1.1.get_or_init(|| Rc::new(HalfGlyphs::new(self)))
    }

    /// The font page as SmileBASIC 4 keeps it in GRP5.
    pub fn page_image(&self) -> Vec<u32> {
        match &self.0 {
            Source::Page { page, .. } => page.clone(),
            // Every fallback font is the same, so its page is drawn once.
            Source::Cells { .. } => {
                static PAGE: OnceLock<Vec<u32>> = OnceLock::new();
                PAGE.get_or_init(|| self.draw_page()).clone()
            }
        }
    }

    fn draw_page(&self) -> Vec<u32> {
        match &self.0 {
            Source::Page { page, .. } => page.clone(),
            Source::Cells { slots, .. } => {
                let mut page = vec![0; (PAGE_SIZE * PAGE_SIZE) as usize];
                for (code, &slot) in slots.iter().enumerate() {
                    if slot == NO_CELL {
                        continue;
                    }
                    for size in [FontSize::Small, FontSize::Large] {
                        let (gx, gy) = slot_origin(u32::from(slot), size);
                        let n = size as u32;
                        for y in 0..n {
                            for x in 0..n {
                                let p = self.pixel(code as u16, size, x, y);
                                page[((gy + y) * PAGE_SIZE + gx + x) as usize] = p;
                            }
                        }
                    }
                }
                page
            }
        }
    }

    /// Top-left corner of a code's glyph on the font page, as FONTINFO
    /// reports it. `None` for fallback glyphs that did not fit on the page.
    pub fn origin(&self, code: u16, size: FontSize) -> Option<(u32, u32)> {
        self.glyph_index(code).map(|n| slot_origin(n, size))
    }

    /// Whether the font draws `code` as itself rather than as the glyph
    /// for missing characters. One table lookup, as the editor asks it of
    /// every character it shows.
    pub fn has_glyph(&self, code: u16) -> bool {
        match &self.0 {
            Source::Page { glyphs, .. } => glyphs[usize::from(code)] != MISSING_GLYPH,
            Source::Cells { index, .. } => {
                let drawn = match code {
                    0x21..=0x7E => code + 0xFEE0,
                    _ => code,
                };
                index[usize::from(drawn)] != NO_CELL
            }
        }
    }

    /// The glyph number of a code with a place of its own on the font
    /// page; `None` for codes drawn as the box, or with no place there.
    pub fn own_glyph(&self, code: u16) -> Option<u32> {
        let n = match &self.0 {
            Source::Page { glyphs, .. } => glyphs[usize::from(code)],
            Source::Cells { slots, .. } => slots[usize::from(code)],
        };
        (n != MISSING_GLYPH && n != NO_CELL).then_some(u32::from(n))
    }

    /// Whether this is SmileBASIC 4's own font, not the bundled one.
    pub fn is_smilebasic(&self) -> bool {
        matches!(self.0, Source::Page { .. })
    }

    /// The glyph number font.map gives a code, which places it on the
    /// font page for every font size. `None` for fallback glyphs that did
    /// not fit on the page.
    pub fn glyph_index(&self, code: u16) -> Option<u32> {
        let n = match &self.0 {
            Source::Page { glyphs, .. } => glyphs[usize::from(code)],
            Source::Cells { slots, index, .. } => {
                let drawn = match code {
                    0x21..=0x7E => code + 0xFEE0,
                    _ => code,
                };
                match slots[usize::from(code)] {
                    // Codes with no glyph at all show the missing glyph.
                    NO_CELL if index[usize::from(drawn)] == NO_CELL => slots[0xFFFD],
                    slot => slot,
                }
            }
        };
        (n != NO_CELL).then_some(u32::from(n))
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
                ..
            } => {
                // SmileBASIC's font draws ASCII as wide as full-width
                // characters, so use GalmuriMono7's full-width forms.
                let code = match code {
                    0x21..=0x7E => code + 0xFEE0,
                    _ => code,
                };
                let i = match index[usize::from(code)] {
                    NO_CELL => *missing,
                    i => i,
                };
                // SmileBASIC's own symbols keep their 8x8 drawings.
                if size == FontSize::Large
                    && i & SYMBOL == 0
                    && let Some(bitmap) = char::from_u32(u32::from(code)).and_then(large_glyph)
                {
                    let alpha = u32::from(bitmap[(y * 16 + x) as usize]);
                    return if alpha == 0 {
                        0
                    } else {
                        alpha << 24 | 0x00FF_FFFF
                    };
                }
                // An 8x8 drawing, such as SmileBASIC's own symbols, fills
                // a 16-pixel cell at twice its size, both ways.
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

    /// GPUTCHRP's [`Span`] of `code`: from font.map's metrics, or else
    /// worked out once from the font's own glyph.
    pub fn span(&self, code: u16, size: FontSize) -> Span {
        if let Source::Page {
            metrics: Some(m), ..
        } = &self.0
        {
            return Span::from_metrics(m[usize::from(code)], size);
        }
        let known = self
            .1
            .2
            .get_or_init(|| (0..2 * CODES).map(|_| Cell::new(0)).collect());
        let slot = &known[usize::from(code) * 2 + usize::from(size == FontSize::Large)];
        if slot.get() == 0 {
            slot.set(self.measure(code, size).pack());
        }
        Span::unpack(slot.get())
    }

    /// A [`Span`] from the glyph's pixels: from one empty column before
    /// its ink, where it has one, to its last inked column. The glyph for
    /// missing characters takes its whole cell; blanks take a quarter of
    /// it, and the ideographic space 7/8, as in SmileBASIC 4's font.
    fn measure(&self, code: u16, size: FontSize) -> Span {
        let n = size as i32;
        if !self.has_glyph(code) {
            return Span { first: 0, width: n };
        }
        let mut inked = 0u32;
        self.each_pixel(code, size, |x, _, _| inked |= 1 << x);
        if inked == 0 {
            let width = if code == 0x3000 { n * 7 / 8 } else { n / 4 };
            return Span { first: 0, width };
        }
        let first = (inked.trailing_zeros() as i32 - 1).max(0);
        let last = 31 - inked.leading_zeros() as i32;
        Span {
            first,
            width: last - first + 1,
        }
    }

    /// Calls `f` with each pixel of a code's glyph that is not fully
    /// transparent, as `(x, y, 0xAARRGGBB)`. Faster than asking
    /// [`Font::pixel`] for each.
    pub fn each_pixel(&self, code: u16, size: FontSize, mut f: impl FnMut(u32, u32, u32)) {
        let n = size as u32;
        match (&self.0, self.origin(code, size)) {
            (Source::Page { page, .. }, Some((gx, gy))) => {
                for y in 0..n {
                    let row = ((gy + y) * PAGE_SIZE + gx) as usize;
                    for (x, &p) in page[row..row + n as usize].iter().enumerate() {
                        if p >> 24 != 0 {
                            f(x as u32, y, p);
                        }
                    }
                }
            }
            _ => {
                for y in 0..n {
                    for x in 0..n {
                        let p = self.pixel(code, size, x, y);
                        if p >> 24 != 0 {
                            f(x, y, p);
                        }
                    }
                }
            }
        }
    }

    /// A page font with every code mapped to glyph `code % 4096` on a
    /// blank page, for tests that do not have the real files.
    pub fn blank() -> Font {
        Font(
            Source::Page {
                glyphs: (0..CODES).map(|c| (c % 4096) as u16).collect(),
                metrics: None,
                page: vec![0; (PAGE_SIZE * PAGE_SIZE) as usize],
            },
            Pages::default(),
        )
    }

    /// [`Font::blank`] without glyphs for codes from `first` up, as
    /// SmileBASIC 4's font lacks Hangul and surrogates.
    pub fn blank_from(first: u16) -> Font {
        let mut font = Font::blank();
        if let Source::Page { glyphs, .. } = &mut font.0 {
            glyphs[usize::from(first)..].fill(MISSING_GLYPH);
        }
        font
    }

    /// [`Font::blank`] with font.map metrics for every code.
    #[cfg(test)]
    pub(crate) fn blank_with_metrics(metrics: Vec<u16>) -> Font {
        let mut font = Font::blank();
        if let Source::Page { metrics: m, .. } = &mut font.0 {
            *m = Some(metrics);
        }
        font
    }

    #[cfg(test)]
    pub(crate) fn page_mut(&mut self) -> &mut [u32] {
        self.1 = Pages::default();
        match &mut self.0 {
            Source::Page { page, .. } => page,
            Source::Cells { .. } => unreachable!("tests draw on page fonts"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 4.x font.map entry: the glyph number, then the first column and
    /// width at 8 pixels and at 16.
    fn entry(glyph: u32, small: (u32, u32), large: (u32, u32)) -> u32 {
        glyph | small.0 << 12 | small.1 << 15 | large.0 << 19 | large.1 << 23
    }

    #[test]
    fn map_layouts() {
        let mut v1 = vec![0u8; CODES * 2];
        v1[0x41 * 2] = 65;
        assert_eq!(parse_map(&v1).unwrap().0[0x41], 65);
        assert!(parse_map(&v1).unwrap().1.is_none());

        let mut v4 = vec![0u8; CODES * 4];
        // Glyph 305, 7 of 8 pixels and 14 of 16 wide, as full-width kana.
        let kana = entry(305, (0, 7), (0, 14));
        v4[0x3042 * 4..0x3042 * 4 + 4].copy_from_slice(&kana.to_le_bytes());
        let (glyphs, metrics) = parse_map(&v4).unwrap();
        assert_eq!(glyphs[0x3042], 305);
        assert_eq!(metrics.unwrap()[0x3042], (kana >> 12) as u16);

        assert!(matches!(parse_map(&[0; 3]), Err(FontError::MapSize(3))));
    }

    #[test]
    fn spans_from_metrics() {
        // Widths GPUTCHRP gave on hardware; the 8-pixel ones are not the
        // 16-pixel ones halved (U+00B2, U+E25B).
        let codes = [
            (0x41, (0, 6), (0, 13)),
            (0x49, (1, 4), (3, 7)),
            (0xB2, (1, 4), (3, 7)),
            (0x2026, (0, 6), (0, 13)),
            (0x3000, (0, 7), (0, 14)),
            (0x30B8, (0, 8), (0, 16)),
            (0xE25B, (7, 1), (15, 1)),
        ];
        let mut v4 = vec![0u8; CODES * 4];
        for &(code, small, large) in &codes {
            let e = entry(0x123, small, large);
            v4[code * 4..code * 4 + 4].copy_from_slice(&e.to_le_bytes());
        }
        let font = Font::blank_with_metrics(parse_map(&v4).unwrap().1.unwrap());
        let span = |(first, width): (u32, u32)| Span {
            first: first as i32,
            width: width as i32,
        };
        for &(code, small, large) in &codes {
            assert_eq!(font.span(code as u16, FontSize::Small), span(small));
            assert_eq!(font.span(code as u16, FontSize::Large), span(large));
        }
    }

    #[test]
    fn spans_from_glyphs() {
        let mut font = Font::blank_from(0xAC00);
        let page = font.page_mut();
        // A glyph inked in columns 2 to 9 at 16 and 0 to 4 at 8.
        let (x, y) = slot_origin(0x41, FontSize::Large);
        for gx in 2..10 {
            page[((y + 5) * PAGE_SIZE + x + gx) as usize] = 0x80FF_FFFF;
        }
        let (x, y) = slot_origin(0x41, FontSize::Small);
        for gx in 0..5 {
            page[((y + 1) * PAGE_SIZE + x + gx) as usize] = 0xFFFF_FFFF;
        }
        let span = |first, width| Span { first, width };
        let (small, large) = (FontSize::Small, FontSize::Large);
        assert_eq!(font.span(0x41, large), span(1, 9));
        assert_eq!(font.span(0x41, small), span(0, 5));
        // Blanks, the ideographic space, and the glyph for missing ones.
        assert_eq!(font.span(0x20, large), span(0, 4));
        assert_eq!(font.span(0x20, small), span(0, 2));
        assert_eq!(font.span(0x3000, large), span(0, 14));
        assert_eq!(font.span(0x3000, small), span(0, 7));
        assert_eq!(font.span(0xAC00, large), span(0, 16));
        assert_eq!(font.span(0xAC00, small), span(0, 8));
        // The bundled font measures its own glyphs.
        let fallback = Font::fallback();
        let a = fallback.span(0x41, large);
        assert!(a.width > 4 && a.first + a.width <= 16, "{a:?}");
        assert_eq!(fallback.span(0x41, large), a);
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
        // Common characters get a place on the font page; the page has no
        // room left for Hangul.
        assert_eq!(font.origin(0x41, FontSize::Small), Some((33 * 8, 0)));
        assert_eq!(font.origin(0xAC00, FontSize::Small), None);
        // ASCII is drawn with the full-width forms.
        assert_eq!(rows(&font, 'A'), rows(&font, 'Ａ'));
        // Other half-width glyphs are centered in the cell.
        let kana = rows(&font, 'ｱ');
        assert!(kana.iter().all(|r| r.starts_with("..")), "{kana:?}");
        assert!(kana.iter().any(|r| r.contains('#')));
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
        // The 16-pixel font is drawn from outlines, with soft edges...
        let alphas: Vec<u32> = (0..256)
            .map(|i| font.pixel('A' as u16, FontSize::Large, i % 16, i / 16) >> 24)
            .collect();
        assert!(alphas.contains(&255) && alphas.iter().any(|&a| a > 0 && a < 255));
        // ...what the outlines lack from Galmuri14, drawn for 16 pixels...
        let galmuri = |code: u16| {
            let mut cell = Vec::new();
            assert!(crate::extra_glyphs::galmuri16_cell(
                u32::from(code),
                &mut cell
            ));
            cell.iter()
                .map(|&a| {
                    if a == 0 {
                        0
                    } else {
                        u32::from(a) << 24 | 0xFF_FFFF
                    }
                })
                .collect::<Vec<_>>()
        };
        let large = |code: u16| -> Vec<u32> {
            (0..256)
                .map(|i| font.pixel(code, FontSize::Large, i % 16, i / 16))
                .collect()
        };
        assert_eq!(large('가' as u16), galmuri('가' as u16));
        // ...and SmileBASIC's symbols as their 8-pixel drawings at twice
        // their size.
        for (x, y) in [(0, 0), (3, 1), (7, 7)] {
            for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                assert_eq!(
                    font.pixel(0xE100, FontSize::Large, 2 * x + dx, 2 * y + dy),
                    font.pixel(0xE100, FontSize::Small, x, y)
                );
            }
        }
        assert_eq!(font.pixel(0xE10A, FontSize::Large, 0, 0), 0xFFFF_FFFF);
    }
}
