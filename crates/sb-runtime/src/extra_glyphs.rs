//! Glyphs the editor draws for characters SmileBASIC's font lacks, which
//! SmileBASIC 4 shows as a box.
//!
//! There are three sources of glyphs ([`FontSource`]):
//!
//! - SmileBASIC's font, imported or the bundled stand-in ([`Font`]);
//! - the font file chosen in the settings (`editor_extra_font`): BDF,
//!   TrueType or OpenType;
//! - the bundled glyphs. At 8 pixels: GalmuriMono7, Misaki Gothic 2nd,
//!   M PLUS 1p Bold, then Fusion Pixel 8px. At 16 pixels, one of two sets
//!   ([`BundledSet`], the setting `bundled_glyphs16`), each followed by the
//!   other's fonts for the characters it lacks:
//!   - antialiased, the default: M PLUS 1p Bold, the face closest to
//!     SmileBASIC 4's own 16-pixel font, then Noto Sans SC Bold (GB2312),
//!     then the dot glyphs;
//!   - pixel: Izumi Gothic 16, then Unifont JP, both made bold, then the
//!     antialiased set's fonts.
//!
//! The editor looks a character up in them in the order the settings give
//! ([`Fonts`]), by default in the order above. When none has it, it draws
//! the font's box. Every glyph takes one cell, the same as every character
//! SmileBASIC draws, but keeps the size its font draws it at: ASCII takes
//! the font's full-width forms, which are drawn for a square cell, and
//! other narrow glyphs are centered rather than widened. Bitmap glyphs are
//! never made larger: a font drawn for 8 pixels stays 8 pixels in a
//! 16-pixel cell.
//!
//! Programs see these glyphs only where the settings put them on the font
//! page (see [`crate::program_font`]); by default they never do.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::fmt;
use std::path::Path;
use std::sync::OnceLock;

use ab_glyph::FontVec;

use crate::cell_glyph::{self, BdfGlyph, BdfLayout, Ink, full_width_form};
use crate::font::{self, Font, FontSize};
use crate::framebuffer::Framebuffer;
use crate::text::TextScreen;

/// The source of a character's glyph, or the source tried first.
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

/// The set of bundled 16-pixel glyphs that comes first. The other set's
/// fonts follow it, for the characters it lacks, so both sets draw the
/// same characters.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum BundledSet {
    /// Outlines with gray edges: M PLUS 1p Bold, then Noto Sans SC Bold.
    #[default]
    Antialiased,
    /// Dot fonts made bold: Izumi Gothic 16, then Unifont JP.
    Pixel,
}

/// The sources of [`ExtraGlyphs`] to look in.
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
        let mut data =
            miniz_oxide::inflate::decompress_to_vec(packed).expect("bundled glyph table");
        let count = u32::from_le_bytes(data[0..4].try_into().expect("count")) as usize;
        let (cell, bits) = (u32::from(data[4]), data[5]);
        let codes = data[6..6 + count * 4]
            .chunks_exact(4)
            .map(|c| u32::from_le_bytes(c.try_into().expect("code")))
            .collect();
        // The cells stay in the buffer they were unpacked into rather than
        // being copied out: the dot glyphs' table is 2 MB.
        data.drain(..6 + count * 4);
        Table {
            codes,
            cells: data,
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

table!(dot16, "extra_dot16.bin");
table!(half16, "extra_half16.bin");
table!(misaki8, "extra_misaki8.bin");
table!(fusion8, "extra_fusion8.bin");
table!(sc16, "extra_sc16.bin");

/// Unifont JP's half-width glyph for `code`, 8x16, if it has one among
/// the half-width forms (U+FF00-U+FFEF).
pub(crate) fn dot_half16(code: u32) -> Option<Vec<u8>> {
    let mut cell = Vec::with_capacity(256);
    half16().copy(code, &mut cell).then(|| {
        let left = cell.chunks_exact(16).flat_map(|row| &row[..8]);
        left.copied().collect()
    })
}

/// Appends the 16x16 dot glyph for `code` (Izumi Gothic 16 or Unifont
/// JP, made bold), if there is one: the glyph of its [`full_width_form`].
pub(crate) fn dot16_cell(code: u32, out: &mut Vec<u8>) -> bool {
    dot16().copy(full_width_form(code), out)
}

/// Appends Noto Sans SC Bold's 16x16 glyph for `code`, in 16 steps of
/// gray, if it has one that M PLUS 1p Bold lacks.
pub(crate) fn sc16_cell(code: u32, out: &mut Vec<u8>) -> bool {
    sc16().copy(code, out)
}

/// Appends the bundled 16-pixel glyph for `code` from `set`, or else
/// from the other set's fonts. `outline` appends M PLUS 1p Bold's glyph,
/// which each caller draws in its own way. A table is unpacked only when
/// the lookup of a character reaches it.
pub(crate) fn bundled16(
    set: BundledSet,
    code: u32,
    out: &mut Vec<u8>,
    outline: impl FnOnce(&mut Vec<u8>) -> bool,
) -> bool {
    match set {
        // Noto Sans SC draws the CJK characters it shares with Japanese in
        // their Simplified Chinese forms, so it comes after M PLUS 1p,
        // whose Japanese forms win.
        BundledSet::Antialiased => outline(out) || sc16_cell(code, out) || dot16_cell(code, out),
        BundledSet::Pixel => dot16_cell(code, out) || outline(out) || sc16_cell(code, out),
    }
}

/// Appends GalmuriMono7's 8x8 cell for `code`, if it has one: the cell of
/// its [`full_width_form`], like the stand-in font.
fn galmuri(code: u32, out: &mut Vec<u8>) -> bool {
    let Ok(code) = u16::try_from(full_width_form(code)) else {
        return false;
    };
    let Some(rows) = font::fallback_rows(code) else {
        return false;
    };
    out.extend((0..64).map(|p| {
        let (x, y) = (p % 8, p / 8);
        if rows[y] & (0x80 >> x) != 0 { 255 } else { 0 }
    }));
    true
}

/// The bundled M PLUS 1p Bold's glyph for `c` in a `cell`-pixel cell.
fn outlines(c: Option<char>, cell: u32) -> Option<Vec<u8>> {
    let em = cell_glyph::SMILEBASIC_EM;
    c.and_then(|c| cell_glyph::outline_cell(font::outlines(), c, cell, em, Ink::for_cell(cell)))
}

/// A font file read for the editor.
enum External {
    Bitmap {
        glyphs: HashMap<u32, BdfGlyph>,
        layout: BdfLayout,
    },
    Outline(FontVec),
}

/// The largest font file read, because a font larger than this is not
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
    /// The lookup result for each code, cell size, [`Part`] and
    /// [`BundledSet`], so that each is looked for once. Glyphs stay when the
    /// set changes, because callers may still hold them.
    found: HashMap<(u32, u8, u8, BundledSet), Option<Glyph>>,
    /// The glyphs found, one alpha byte per pixel.
    pixels: Vec<u8>,
}

/// The editor's glyphs for characters beyond SmileBASIC's font.
#[derive(Default)]
pub struct ExtraGlyphs {
    external: Option<External>,
    set: Cell<BundledSet>,
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
            set: Cell::default(),
            cache: RefCell::default(),
        })
    }

    /// Picks the set of bundled 16-pixel glyphs.
    pub fn set_bundled(&self, set: BundledSet) {
        self.set.set(set);
    }

    pub fn bundled_set(&self) -> BundledSet {
        self.set.get()
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
        let key = (code, cell, part as u8, self.set.get());
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
    /// per pixel, without keeping it. Returns whether there was one.
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
            // The full-width form first, the same as for outlines.
            (Some(External::Bitmap { glyphs, layout }), _) => glyphs
                .get(&full_width_form(code))
                .or_else(|| glyphs.get(&code))
                .map(|g| cell_glyph::bdf_cell(g, layout, cell)),
            (Some(External::Outline(f)), _) => {
                ch.and_then(|c| cell_glyph::outline_cell(f, c, cell, 1.0, Ink::for_cell(cell)))
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
            // M+ is rough at 8 pixels, but better than the box for the
            // characters the others lack. Fusion Pixel draws shared CJK
            // characters in their Simplified Chinese forms, so it comes
            // last and fills only the characters no other font has.
            FontSize::Small => {
                galmuri(code, out)
                    || misaki8().copy(code, out)
                    || take(out, outlines(ch, 8))
                    || fusion8().copy(code, out)
            }
            // The 8-pixel fonts are not used here.
            FontSize::Large => {
                bundled16(self.set.get(), code, out, |out| take(out, outlines(ch, 16)))
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
    /// The second half of a surrogate pair, drawn on the first half's cell.
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

    /// Chooses what to draw for `line[i]`. A character beyond U+FFFF takes
    /// two UTF-16 units, and so two cells, as SmileBASIC counts it: its
    /// glyph is drawn on the first and the second stays empty.
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
            // The default, and the fast path: one table lookup for the
            // characters the font has.
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

    /// Draws `screen` with these sources: [`TextScreen::draw`] for the
    /// characters from the font, then the other glyphs over their cells.
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

/// The [`Shown`] that `part` of `extra` gives for the unit `c`, between
/// `prev` and `next`.
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

    /// The bundled glyphs with `set` at 16 pixels.
    fn with_set(set: BundledSet) -> ExtraGlyphs {
        let g = ExtraGlyphs::bundled();
        g.set_bundled(set);
        g
    }

    const SETS: [BundledSet; 2] = [BundledSet::Antialiased, BundledSet::Pixel];

    #[test]
    fn bundled_glyphs_cover_many_scripts() {
        for set in SETS {
            let g = with_set(set);
            for size in [FontSize::Small, FontSize::Large] {
                for c in ['한', '글', 'Ω', 'Ж', 'ア', '漢', '汉', '★', '☀', '♪'] {
                    let glyph = g.find(c as u32, size);
                    assert!(glyph.is_some(), "{c} at {size:?} in {set:?}");
                    assert!(ink(&g, glyph.unwrap()) > 0, "{c} at {size:?} is blank");
                }
                // Private use and lone surrogates keep the box.
                assert_eq!(g.find(0xE100, size), None);
                assert_eq!(g.find(0xD83D, size), None);
                // Nothing has this one.
                assert_eq!(g.find(0x10FFFD, size), None);
            }
            // Emoji in the Basic Multilingual Plane come from Unifont JP at
            // 16 pixels; the bundled fonts draw none above it.
            assert!(g.find('✅' as u32, FontSize::Large).is_some(), "{set:?}");
            assert_eq!(g.find('😀' as u32, FontSize::Large), None, "{set:?}");
        }
    }

    fn bundled(g: &ExtraGlyphs, c: char, size: FontSize) -> Vec<u8> {
        let mut out = Vec::new();
        assert!(g.render(c as u32, size, Part::Bundled, &mut out), "{c}");
        out
    }

    fn from(table: &Table, c: char) -> Option<Vec<u8>> {
        let mut out = Vec::new();
        table.copy(c as u32, &mut out).then_some(out)
    }

    #[test]
    fn later_fonts_only_fill_gaps() {
        let g = ExtraGlyphs::bundled();
        assert_eq!(g.bundled_set(), BundledSet::Antialiased);
        let bundled = |c, size| bundled(&g, c, size);
        // Japanese forms stay: 骨 and 直 differ in Simplified Chinese.
        for c in ['骨', '直', 'あ', '한'] {
            let mut galmuri_cell = Vec::new();
            assert!(galmuri(c as u32, &mut galmuri_cell));
            assert_eq!(bundled(c, FontSize::Small), galmuri_cell, "{c}");
            // M PLUS 1p and Noto Sans SC have no Hangul, which Unifont JP
            // draws.
            let large = outlines(Some(c), 16).or_else(|| from(dot16(), c));
            assert_eq!(Some(bundled(c, FontSize::Large)), large, "{c}");
        }
        // Kanji M PLUS 1p lacks, outside GB2312, come from Izumi Gothic 16,
        // made bold.
        for c in ['𠀋', '㐂', '鷗'] {
            assert_eq!(outlines(Some(c), 16), None, "{c}");
            assert_eq!(from(sc16(), c), None, "{c}");
            assert_eq!(Some(bundled(c, FontSize::Large)), from(dot16(), c), "{c}");
        }
        // Greek and Cyrillic at 8 pixels come from Misaki Gothic 2nd.
        for c in ['Ω', 'ж'] {
            assert_eq!(Some(bundled(c, FontSize::Small)), from(misaki8(), c), "{c}");
        }
        // Simplified Chinese no other font has: Fusion Pixel at 8 pixels,
        // Noto Sans SC Bold at 16.
        for c in ['汉', '们', '这'] {
            assert_eq!(Some(bundled(c, FontSize::Small)), from(fusion8(), c), "{c}");
            assert_eq!(Some(bundled(c, FontSize::Large)), from(sc16(), c), "{c}");
        }
    }

    #[test]
    fn the_pixel_set_comes_first() {
        let g = with_set(BundledSet::Pixel);
        let large = |c| bundled(&g, c, FontSize::Large);
        for c in ['骨', 'あ', '한', '汉', 'Ж', '★', '鷗'] {
            assert_eq!(Some(large(c)), from(dot16(), c), "{c}");
        }
        // ASCII takes the full-width form.
        assert_eq!(Some(large('A')), from(dot16(), 'Ａ'));
        // Characters the dot fonts lack come from M PLUS 1p Bold.
        assert_eq!(from(dot16(), '𝔸'), None);
        assert_eq!(Some(large('𝔸')), outlines(Some('𝔸'), 16));
        // The 8-pixel glyphs do not change.
        let outline = ExtraGlyphs::bundled();
        for c in ['A', '漢', '汉', '한'] {
            let small = |g| bundled(g, c, FontSize::Small);
            assert_eq!(small(&g), small(&outline), "{c}");
        }
    }

    #[test]
    fn both_sets_draw_the_same_characters() {
        // One from each source's own part, and ones none has.
        let chars = ['A', 'あ', '𝔸', '汉', '한', '𠀋', '✅', '😀', '\u{10FFFD}'];
        let [outline, dot] = SETS.map(with_set);
        for c in chars {
            let has = |g: &ExtraGlyphs| g.find(c as u32, FontSize::Large).is_some();
            assert_eq!(has(&outline), has(&dot), "{c}");
        }
    }

    #[test]
    fn the_set_changes_glyphs_already_found() {
        let g = ExtraGlyphs::bundled();
        let outline = g.find('漢' as u32, FontSize::Large).unwrap();
        let outline_px = pixels(&g, outline);
        g.set_bundled(BundledSet::Pixel);
        let dot = g.find('漢' as u32, FontSize::Large).unwrap();
        assert_eq!(Some(pixels(&g, dot)), from(dot16(), '漢'));
        assert_ne!(pixels(&g, dot), outline_px);
        // The glyph found before still draws the same way.
        assert_eq!(pixels(&g, outline), outline_px);
        g.set_bundled(BundledSet::Antialiased);
        assert_eq!(g.find('漢' as u32, FontSize::Large), Some(outline));
    }

    #[test]
    fn found_once() {
        let g = ExtraGlyphs::bundled();
        for (c, size) in [
            ('가', FontSize::Large),
            ('A', FontSize::Large),
            ('Ω', FontSize::Small),
        ] {
            let first = g.find(c as u32, size);
            assert!(first.is_some());
            let len = g.cache.borrow().pixels.len();
            assert_eq!(g.find(c as u32, size), first);
            assert_eq!(g.cache.borrow().pixels.len(), len);
        }
    }

    /// The pixels of a glyph found in `g`.
    fn pixels(g: &ExtraGlyphs, glyph: Glyph) -> Vec<u8> {
        let n = usize::from(glyph.cell) * usize::from(glyph.cell);
        g.cache.borrow().pixels[glyph.at as usize..glyph.at as usize + n].to_vec()
    }

    /// The first and last columns with ink, of a `side`-pixel glyph.
    fn columns(px: &[u8], side: usize) -> (usize, usize) {
        let inked: Vec<usize> = (0..side)
            .filter(|&x| (0..side).any(|y| px[y * side + x] > 0))
            .collect();
        (inked[0], *inked.last().unwrap())
    }

    /// A BDF font: U+AC00 as a filled 4x4 square, and A as a 2x4 bar
    /// with Ａ, its full-width form, as a 4x4 square.
    const BDF: &str = "STARTFONT 2.1\nFONT_ASCENT 4\nFONT_DESCENT 0\nCHARS 3\n\
        STARTCHAR a\nENCODING 44032\nDWIDTH 4 0\nBBX 4 4 0 0\nBITMAP\nF0\nF0\nF0\nF0\nENDCHAR\n\
        STARTCHAR A\nENCODING 65\nDWIDTH 2 0\nBBX 2 4 0 0\nBITMAP\nC0\nC0\nC0\nC0\nENDCHAR\n\
        STARTCHAR AA\nENCODING 65313\nDWIDTH 4 0\nBBX 4 4 0 0\nBITMAP\nF0\nF0\nF0\nF0\nENDCHAR\nENDFONT\n";

    #[test]
    fn the_chosen_file_comes_first() {
        let g = ExtraGlyphs::from_bytes(BDF.as_bytes().to_vec()).unwrap();
        assert!(g.has_file());
        // Drawn at its own size, centered: never made larger.
        let small = g.find(0xAC00, FontSize::Small).unwrap();
        assert_eq!(ink(&g, small), 16);
        let large = g.find(0xAC00, FontSize::Large).unwrap();
        assert_eq!(ink(&g, large), 16);
        assert_eq!(columns(&pixels(&g, large), 16), (6, 9));
        // ASCII takes the full-width form.
        let a = g.find('A' as u32, FontSize::Large).unwrap();
        assert_eq!(pixels(&g, a), pixels(&g, large));
        // Characters the file lacks still come from the bundled glyphs.
        assert!(g.find('Ж' as u32, FontSize::Small).is_some());
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
    fn ascii_takes_full_width_forms() {
        // The bundled glyphs, and the same face read as a chosen file.
        let file = ExtraGlyphs::from_bytes(font::OUTLINES.to_vec()).unwrap();
        for (g, part) in [(ExtraGlyphs::bundled(), Part::Bundled), (file, Part::File)] {
            for size in [FontSize::Small, FontSize::Large] {
                for c in ['A', 'i', 'l', '!', '~'] {
                    let full = char::from_u32(c as u32 + 0xFEE0).unwrap();
                    let ascii = g.find_in(c as u32, size, part).unwrap();
                    let full = g.find_in(full as u32, size, part).unwrap();
                    assert_eq!(pixels(&g, ascii), pixels(&g, full), "{c} at {size:?}");
                }
            }
        }
    }

    #[test]
    fn narrow_glyphs_are_not_stretched() {
        // ı (dotless i) has no full-width form: it keeps M+'s width of
        // about a third of the em, in the middle of the cell.
        let file = ExtraGlyphs::from_bytes(font::OUTLINES.to_vec()).unwrap();
        for (g, part) in [(ExtraGlyphs::bundled(), Part::Bundled), (file, Part::File)] {
            let glyph = g.find_in('ı' as u32, FontSize::Large, part).unwrap();
            let (first, last) = columns(&pixels(&g, glyph), 16);
            assert!(last - first < 4, "ı spans columns {first} to {last}");
            assert!((5..=10).contains(&first) && (5..=10).contains(&last));
        }
        // The 8-pixel M+ glyphs are solid: no gray left to blur them.
        let g = ExtraGlyphs::bundled();
        let omega = g.find('Ω' as u32, FontSize::Small).unwrap();
        assert!(pixels(&g, omega).iter().all(|&a| a == 0 || a == 255));
    }

    #[test]
    fn popups_draw_with_the_sources() {
        // The same way the completion list and the hints draw their text.
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
