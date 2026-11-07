//! Glyphs the editor draws for characters SmileBASIC's font lacks, such as
//! Hangul and emoji, which SmileBASIC 4 shows as a box.
//!
//! There are three sources of glyphs ([`FontSource`]):
//!
//! - SmileBASIC's font, imported or the bundled stand-in ([`Font`]);
//! - the font file chosen in the settings (`editor_extra_font`): BDF,
//!   TrueType or OpenType;
//! - the bundled glyphs. At 8 pixels: GalmuriMono7, Noto Emoji, then
//!   M PLUS 1p Bold. At 16 pixels: Galmuri14's Hangul, M PLUS 1p Bold,
//!   Noto Emoji, then GalmuriMono7 doubled.
//!
//! The editor looks a character up in them in the order the settings give
//! ([`Fonts`]), by default in the order above. When none has it, it draws
//! the font's box. Every glyph fills one cell, as SmileBASIC draws every
//! character.
//!
//! Programs see these glyphs only where the settings put them on the font
//! page (see [`crate::program_font`]); by default they never do.

use std::cell::RefCell;
use std::collections::HashMap;
use std::fmt;
use std::path::Path;
use std::sync::OnceLock;

use ab_glyph::FontVec;

use crate::cell_glyph::{self, BdfGlyph, BdfLayout};
use crate::font::{self, Font, FontSize};
use crate::framebuffer::Framebuffer;
use crate::text::TextScreen;

/// Where a character's glyph comes from, or which source is tried first.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum FontSource {
    /// SmileBASIC's font.
    #[default]
    SmileBasic,
    /// The font file chosen in the settings.
    File,
    /// The fonts bundled with the emulator.
    Bundled,
}

/// Which of [`ExtraGlyphs`]' sources to look in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Part {
    File = 1,
    Bundled = 2,
    /// The file, then the bundled glyphs.
    Both = 3,
}

/// Private use characters mean SmileBASIC's own symbols, or something
/// else in each font, so they keep SmileBASIC's glyph or box; lone
/// surrogates have no glyph.
fn wanted(code: u32) -> bool {
    !(0xE000..=0xF8FF).contains(&code) && !(0xD800..=0xDFFF).contains(&code)
}

/// A cell table built by `build.rs`.
struct Table {
    codes: Vec<u32>,
    cells: Vec<u8>,
    cell: u32,
    /// Bits per pixel: 1 or 8.
    bits: u8,
}

impl Table {
    fn read(packed: &[u8]) -> Table {
        let data = miniz_oxide::inflate::decompress_to_vec(packed).expect("bundled glyph table");
        let count = u32::from_le_bytes(data[0..4].try_into().expect("count")) as usize;
        let (cell, bits) = (u32::from(data[4]), data[5]);
        let codes = data[6..6 + count * 4]
            .chunks_exact(4)
            .map(|c| u32::from_le_bytes(c.try_into().expect("code")))
            .collect();
        Table {
            codes,
            cells: data[6 + count * 4..].to_vec(),
            cell,
            bits,
        }
    }

    /// Appends `code`'s alpha bytes to `out`, if the table has it.
    fn copy(&self, code: u32, out: &mut Vec<u8>) -> bool {
        let Ok(i) = self.codes.binary_search(&code) else {
            return false;
        };
        let n = (self.cell * self.cell) as usize;
        match self.bits {
            1 => {
                let bytes = &self.cells[i * n / 8..(i + 1) * n / 8];
                out.extend((0..n).map(|p| match bytes[p / 8] & (0x80 >> (p % 8)) {
                    0 => 0,
                    _ => 255,
                }));
            }
            _ => out.extend_from_slice(&self.cells[i * n..(i + 1) * n]),
        }
        true
    }
}

macro_rules! table {
    ($name:ident, $file:literal) => {
        fn $name() -> &'static Table {
            static PACKED: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/", $file));
            static TABLE: OnceLock<Table> = OnceLock::new();
            TABLE.get_or_init(|| Table::read(PACKED))
        }
    };
}

table!(hangul16, "extra_hangul16.bin");
table!(emoji8, "extra_emoji8.bin");
table!(emoji16, "extra_emoji16.bin");

/// Appends GalmuriMono7's 8x8 cell for `code`, drawn `scale` times as
/// large, if it has one.
fn galmuri(code: u32, scale: usize, out: &mut Vec<u8>) -> bool {
    // SmileBASIC's ASCII is as wide as kana, so it takes the full-width
    // forms, as the stand-in font does.
    let code = match code {
        0x21..=0x7E => code + 0xFEE0,
        _ => code,
    };
    let Ok(code) = u16::try_from(code) else {
        return false;
    };
    let records = font::FALLBACK;
    let count = records.len() / 10;
    let code_at = |i: usize| u16::from_le_bytes([records[i * 10], records[i * 10 + 1]]);
    let (mut lo, mut hi) = (0, count);
    while lo < hi {
        let mid = (lo + hi) / 2;
        match code_at(mid).cmp(&code) {
            std::cmp::Ordering::Less => lo = mid + 1,
            std::cmp::Ordering::Greater => hi = mid,
            std::cmp::Ordering::Equal => {
                let rows = &records[mid * 10 + 2..mid * 10 + 10];
                let side = 8 * scale;
                out.extend((0..side * side).map(|p| {
                    let (x, y) = (p % side / scale, p / side / scale);
                    if rows[y] & (0x80 >> x) != 0 { 255 } else { 0 }
                }));
                return true;
            }
        }
    }
    false
}

/// The bundled M PLUS 1p Bold's glyph for `c` in a `cell`-pixel cell.
fn outlines(c: Option<char>, cell: u32) -> Option<Vec<u8>> {
    c.and_then(|c| cell_glyph::outline_cell(font::outlines(), c, cell, cell_glyph::SMILEBASIC_EM))
}

/// A font file read for the editor.
enum External {
    Bitmap {
        glyphs: HashMap<u32, BdfGlyph>,
        layout: BdfLayout,
    },
    Outline(FontVec),
}

/// The largest font file read, as a font larger than this is not one
/// meant for a character grid.
const MAX_FILE: u64 = 64 << 20;

#[derive(Debug)]
pub enum LoadError {
    Read(std::io::Error),
    TooLarge,
    /// Neither BDF nor a font ab_glyph reads.
    Format,
    /// A BDF file with no glyph that has a Unicode code.
    Empty,
}

impl fmt::Display for LoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LoadError::Read(e) => write!(f, "{e}"),
            LoadError::TooLarge => write!(f, "the file is larger than 64 MB"),
            LoadError::Format => write!(f, "not a BDF, TrueType or OpenType font"),
            LoadError::Empty => write!(f, "the BDF file has no Unicode glyphs"),
        }
    }
}

impl std::error::Error for LoadError {}

/// A glyph found by [`ExtraGlyphs::find`], to draw with
/// [`ExtraGlyphs::draw`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Glyph {
    at: u32,
    cell: u8,
}

#[derive(Default)]
struct Cache {
    /// What each code, cell size and [`Part`] was found as, so that each
    /// is looked for once.
    found: HashMap<(u32, u8, u8), Option<Glyph>>,
    /// The glyphs found, one alpha byte per pixel.
    pixels: Vec<u8>,
}

/// The editor's glyphs for characters beyond SmileBASIC's font.
#[derive(Default)]
pub struct ExtraGlyphs {
    external: Option<External>,
    cache: RefCell<Cache>,
}

impl fmt::Debug for ExtraGlyphs {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self.external {
            None => "ExtraGlyphs(bundled)",
            Some(External::Bitmap { .. }) => "ExtraGlyphs(BDF)",
            Some(External::Outline(_)) => "ExtraGlyphs(outlines)",
        })
    }
}

impl ExtraGlyphs {
    /// The bundled glyphs alone.
    pub fn bundled() -> ExtraGlyphs {
        ExtraGlyphs::default()
    }

    /// The font file at `path` ahead of the bundled glyphs.
    pub fn load(path: &Path) -> Result<ExtraGlyphs, LoadError> {
        let size = std::fs::metadata(path).map_err(LoadError::Read)?.len();
        if size > MAX_FILE {
            return Err(LoadError::TooLarge);
        }
        let bytes = std::fs::read(path).map_err(LoadError::Read)?;
        ExtraGlyphs::from_bytes(bytes)
    }

    /// [`ExtraGlyphs::load`] for a file's contents.
    pub fn from_bytes(bytes: Vec<u8>) -> Result<ExtraGlyphs, LoadError> {
        let external = if bytes.starts_with(b"STARTFONT") {
            let bdf = cell_glyph::parse_bdf(&String::from_utf8_lossy(&bytes));
            if bdf.glyphs.is_empty() {
                return Err(LoadError::Empty);
            }
            let layout = BdfLayout::new(&bdf);
            let glyphs = bdf.glyphs.into_iter().map(|g| (g.code, g)).collect();
            External::Bitmap { glyphs, layout }
        } else {
            External::Outline(FontVec::try_from_vec(bytes).map_err(|_| LoadError::Format)?)
        };
        Ok(ExtraGlyphs {
            external: Some(external),
            cache: RefCell::default(),
        })
    }

    /// Whether a font file was read.
    pub fn has_file(&self) -> bool {
        self.external.is_some()
    }

    /// The glyph for `code` at `size`, `None` if neither the chosen file
    /// nor the bundled glyphs have it. Each code is looked for once;
    /// later calls cost a hash lookup.
    pub fn find(&self, code: u32, size: FontSize) -> Option<Glyph> {
        self.find_in(code, size, Part::Both)
    }

    /// [`ExtraGlyphs::find`] in `part` of the sources.
    pub fn find_in(&self, code: u32, size: FontSize, part: Part) -> Option<Glyph> {
        if !wanted(code) || (part == Part::File && self.external.is_none()) {
            return None;
        }
        let cell = size as u8;
        let key = (code, cell, part as u8);
        let mut cache = self.cache.borrow_mut();
        if let Some(&found) = cache.found.get(&key) {
            return found;
        }
        let at = cache.pixels.len();
        let found = self
            .make(code, size, part, &mut cache.pixels)
            .then_some(Glyph {
                at: at as u32,
                cell,
            });
        cache.found.insert(key, found);
        found
    }

    /// Writes the glyph for `code` from `part` into `out`, one alpha byte
    /// per pixel, without keeping it. Whether there was one.
    pub(crate) fn render(&self, code: u32, size: FontSize, part: Part, out: &mut Vec<u8>) -> bool {
        out.clear();
        wanted(code) && self.make(code, size, part, out)
    }

    /// The chosen file's glyph for a half-width character, `cell / 2`
    /// pixels wide, if the file draws it half-width.
    pub(crate) fn file_half(&self, code: u32, cell: u32) -> Option<Vec<u8>> {
        match &self.external {
            Some(External::Bitmap { glyphs, layout }) => {
                cell_glyph::bdf_half(glyphs.get(&code)?, layout, cell)
            }
            Some(External::Outline(f)) => cell_glyph::outline_half(f, char::from_u32(code)?, cell),
            None => None,
        }
    }

    /// Appends the glyph's alpha bytes to `out`, from the first source in
    /// `part` that has it.
    fn make(&self, code: u32, size: FontSize, part: Part, out: &mut Vec<u8>) -> bool {
        let cell = size as u32;
        let ch = char::from_u32(code);
        fn take(out: &mut Vec<u8>, alpha: Option<Vec<u8>>) -> bool {
            alpha.map(|a| out.extend_from_slice(&a)).is_some()
        }
        let from_file = match (&self.external, part) {
            (_, Part::Bundled) => None,
            (Some(External::Bitmap { glyphs, layout }), _) => glyphs
                .get(&code)
                .map(|g| cell_glyph::bdf_cell(g, layout, cell)),
            (Some(External::Outline(f)), _) => {
                ch.and_then(|c| cell_glyph::outline_cell(f, c, cell, 1.0))
            }
            (None, _) => None,
        };
        if take(out, from_file) {
            return true;
        }
        if part == Part::File {
            return false;
        }
        match size {
            // M+ is blurry at 8 pixels, but better than the box for the
            // Greek and Cyrillic GalmuriMono7 lacks.
            FontSize::Small => {
                galmuri(code, 1, out) || emoji8().copy(code, out) || take(out, outlines(ch, 8))
            }
            FontSize::Large => {
                hangul16().copy(code, out)
                    || take(out, outlines(ch, 16))
                    || emoji16().copy(code, out)
                    || galmuri(code, 2, out)
            }
        }
    }

    /// Draws a glyph in `color` over the `cell`-pixel square at
    /// `(x, y)` of `fb`.
    pub fn draw(&self, fb: &mut Framebuffer, glyph: Glyph, (x, y): (u32, u32), color: u32) {
        let cache = self.cache.borrow();
        let side = u32::from(glyph.cell);
        let n = (side * side) as usize;
        let alpha = &cache.pixels[glyph.at as usize..glyph.at as usize + n];
        for (i, &a) in alpha.iter().enumerate() {
            if a == 0 {
                continue;
            }
            let (px, py) = (x + i as u32 % side, y + i as u32 / side);
            if let Some(under) = fb.get(px, py) {
                let a = u32::from(a) + u32::from(a >> 7);
                fb.set(px, py, crate::editor::blend(under, color, a));
            }
        }
    }
}

/// What the editor draws for a character.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shown {
    /// SmileBASIC's glyph, or its box when no source has one.
    Font,
    Extra(Glyph),
    /// The second half of a surrogate pair, drawn on the first's cell.
    Blank,
}

/// The sources the editor draws with, in the order the settings give.
#[derive(Clone, Copy)]
pub struct Fonts<'a> {
    pub font: &'a Font,
    /// `None` draws with `font` alone.
    pub extra: Option<&'a ExtraGlyphs>,
    pub first: FontSource,
    pub size: FontSize,
}

impl<'a> Fonts<'a> {
    /// `font` alone, as programs see it.
    pub fn plain(font: &'a Font, size: FontSize) -> Fonts<'a> {
        Fonts {
            font,
            extra: None,
            first: FontSource::SmileBasic,
            size,
        }
    }

    /// What to draw for `line[i]`. A character beyond U+FFFF takes two
    /// UTF-16 units, and so two cells, as SmileBASIC counts it: its glyph
    /// goes on the first and the second stays empty.
    #[inline]
    pub fn choose(&self, line: &[u16], i: usize) -> Shown {
        let prev = i.checked_sub(1).and_then(|p| line.get(p)).copied();
        self.choose_unit(line[i], prev, line.get(i + 1).copied())
    }

    /// [`Fonts::choose`] for `c` between `prev` and `next`.
    pub fn choose_unit(&self, c: u16, prev: Option<u16>, next: Option<u16>) -> Shown {
        let Some(extra) = self.extra else {
            return Shown::Font;
        };
        let first = match self.first {
            FontSource::File if !extra.has_file() => FontSource::SmileBasic,
            first => first,
        };
        let look = |part| extra_unit(extra, (c, prev, next), self.size, part);
        let font = || self.font.has_glyph(c).then_some(Shown::Font);
        let shown = match first {
            // The default, and the fast path: one table lookup for what
            // the font has.
            FontSource::SmileBasic => font().or_else(|| look(Part::Both)),
            FontSource::File => look(Part::File)
                .or_else(font)
                .or_else(|| look(Part::Bundled)),
            FontSource::Bundled => look(Part::Bundled)
                .or_else(font)
                .or_else(|| look(Part::File)),
        };
        shown.unwrap_or(Shown::Font)
    }

    /// Draws `screen` with these sources: [`TextScreen::draw`] for what
    /// comes from the font, then the other glyphs over their cells.
    /// Screens whose cells are not this size draw with the font alone.
    pub fn draw_screen(&self, screen: &mut TextScreen, fb: &mut Framebuffer) {
        let (_, size) = screen.font();
        let mut over = Vec::new();
        if self.extra.is_some() && size == self.size as u32 {
            let (w, h) = screen.size();
            for y in 0..h {
                for x in 0..w {
                    let cell = screen.cell(x, y);
                    if cell.code == 0 {
                        continue;
                    }
                    let unit = |x: u32| screen.cell_at(x as i32, y as i32).map(|c| c.code);
                    let prev = x.checked_sub(1).and_then(unit);
                    match self.choose_unit(cell.code, prev, unit(x + 1)) {
                        Shown::Font => continue,
                        Shown::Extra(glyph) => over.push((x, y, Some(glyph), cell.color)),
                        Shown::Blank => over.push((x, y, None, cell.color)),
                    }
                }
            }
            for &(x, y, _, color) in &over {
                screen.put_at(x, y, 0, color);
            }
        }
        screen.draw(fb, self.font);
        if let Some(extra) = self.extra {
            let (ox, oy) = screen.pos;
            for (x, y, glyph, color) in over {
                let at = (
                    ox as i64 + i64::from(x * size),
                    oy as i64 + i64::from(y * size),
                );
                if let (Some(glyph), Ok(px), Ok(py)) =
                    (glyph, u32::try_from(at.0), u32::try_from(at.1))
                {
                    extra.draw(fb, glyph, (px, py), color);
                }
            }
        }
    }
}

/// What `part` of `extra` has for the unit `c`, between `prev` and `next`.
fn extra_unit(
    extra: &ExtraGlyphs,
    (c, prev, next): (u16, Option<u16>, Option<u16>),
    size: FontSize,
    part: Part,
) -> Option<Shown> {
    let high = |u: u16| (0xD800..0xDC00).contains(&u);
    let low = |u: u16| (0xDC00..0xE000).contains(&u);
    let pair =
        |hi: u16, lo: u16| 0x10000 + ((u32::from(hi) - 0xD800) << 10) + (u32::from(lo) - 0xDC00);
    let find = |code| extra.find_in(code, size, part);
    match (next, prev) {
        (Some(next), _) if high(c) && low(next) => find(pair(c, next)).map(Shown::Extra),
        (_, Some(prev)) if low(c) && high(prev) => find(pair(prev, c)).map(|_| Shown::Blank),
        _ => find(u32::from(c)).map(Shown::Extra),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ink(glyphs: &ExtraGlyphs, glyph: Glyph) -> usize {
        let cache = glyphs.cache.borrow();
        let n = usize::from(glyph.cell) * usize::from(glyph.cell);
        let at = glyph.at as usize;
        cache.pixels[at..at + n].iter().filter(|&&a| a > 0).count()
    }

    #[test]
    fn bundled_glyphs_cover_hangul_and_emoji() {
        let g = ExtraGlyphs::bundled();
        for size in [FontSize::Small, FontSize::Large] {
            for c in ['한', '글', '😀', 'Ω', '★'] {
                let glyph = g.find(c as u32, size);
                assert!(glyph.is_some(), "{c} at {size:?}");
                assert!(ink(&g, glyph.unwrap()) > 0, "{c} at {size:?} is blank");
            }
            // Private use and lone surrogates keep the box.
            assert_eq!(g.find(0xE100, size), None);
            assert_eq!(g.find(0xD83D, size), None);
            // Nothing has this one.
            assert_eq!(g.find(0x10FFFD, size), None);
        }
    }

    #[test]
    fn found_once() {
        let g = ExtraGlyphs::bundled();
        let first = g.find('가' as u32, FontSize::Large);
        let len = g.cache.borrow().pixels.len();
        assert_eq!(g.find('가' as u32, FontSize::Large), first);
        assert_eq!(g.cache.borrow().pixels.len(), len);
    }

    /// A one-glyph BDF font: U+AC00 as a filled 4x4 square.
    const BDF: &str = "STARTFONT 2.1\nFONT_ASCENT 4\nFONT_DESCENT 0\nCHARS 1\n\
        STARTCHAR a\nENCODING 44032\nDWIDTH 4 0\nBBX 4 4 0 0\nBITMAP\nF0\nF0\nF0\nF0\nENDCHAR\nENDFONT\n";

    #[test]
    fn the_chosen_file_comes_first() {
        let g = ExtraGlyphs::from_bytes(BDF.as_bytes().to_vec()).unwrap();
        assert!(g.has_file());
        // Scaled by whole steps to fill the cell: 4 rows become 8 and 16.
        let small = g.find(0xAC00, FontSize::Small).unwrap();
        assert_eq!(ink(&g, small), 64);
        let large = g.find(0xAC00, FontSize::Large).unwrap();
        assert_eq!(ink(&g, large), 256);
        // What the file lacks still comes from the bundled glyphs.
        assert!(g.find('😀' as u32, FontSize::Small).is_some());
    }

    #[test]
    fn unreadable_files_are_errors() {
        let missing = Path::new("no such directory/no such font.ttf");
        assert!(matches!(
            ExtraGlyphs::load(missing),
            Err(LoadError::Read(_))
        ));
        assert!(matches!(
            ExtraGlyphs::from_bytes(b"not a font".to_vec()),
            Err(LoadError::Format)
        ));
        assert!(matches!(
            ExtraGlyphs::from_bytes(b"STARTFONT 2.1\nENDFONT\n".to_vec()),
            Err(LoadError::Empty)
        ));
    }

    #[test]
    fn outline_files_are_read() {
        // The bundled 16-pixel font stands in for a chosen TTF file.
        let g = ExtraGlyphs::from_bytes(font::OUTLINES.to_vec()).unwrap();
        let glyph = g.find('漢' as u32, FontSize::Large).unwrap();
        assert!(ink(&g, glyph) > 20);
    }

    #[test]
    fn latin_fills_the_cell() {
        // Narrow outline glyphs are widened toward the cell's width.
        let g = ExtraGlyphs::bundled();
        let glyph = g.find('ŵ' as u32, FontSize::Large).unwrap();
        let cache = g.cache.borrow();
        let px = &cache.pixels[glyph.at as usize..glyph.at as usize + 256];
        let cols: Vec<usize> = (0..16)
            .filter(|&x| (0..16).any(|y| px[y * 16 + x] > 0))
            .collect();
        let width = cols.last().unwrap() - cols.first().unwrap() + 1;
        assert!(width >= 10, "ŵ is {width} pixels wide");
    }

    #[test]
    fn popups_draw_with_the_sources() {
        // As the completion list and the hints draw their text.
        let font = Font::blank_from(0xAC00);
        let extra = ExtraGlyphs::bundled();
        let fonts = Fonts {
            font: &font,
            extra: Some(&extra),
            first: FontSource::SmileBasic,
            size: FontSize::Small,
        };
        let mut fb = Framebuffer::new(64, 16);
        let mut screen = TextScreen::new(0, 0);
        screen.set_font(8, 8, 4, 1);
        screen.pos = (8.0, 0.0);
        screen.put_at(1, 0, '가' as u16, 0xFFFF_FFFF);
        fonts.draw_screen(&mut screen, &mut fb);
        assert_eq!(screen.cell(1, 0).code, 0);
        let back = fb.get(0, 0);
        assert!((16..24).any(|x| (0..8).any(|y| fb.get(x, y) != back)));
        // The font alone keeps the character, for its box.
        let mut screen2 = TextScreen::new(0, 0);
        screen2.set_font(8, 8, 4, 1);
        screen2.put_at(1, 0, '가' as u16, 0xFFFF_FFFF);
        Fonts::plain(&font, FontSize::Small).draw_screen(&mut screen2, &mut fb);
        assert_eq!(screen2.cell(1, 0).code, '가' as u16);
    }
}
