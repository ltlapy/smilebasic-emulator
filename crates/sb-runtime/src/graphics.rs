//! Graphic pages GRP0-GRP5 and drawing on them. A page takes memory only
//! once something is drawn on it; until then every pixel is 0.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};

use sb_core::value::{Array, ArrayData, ArrayRef, ElemType};

use crate::compose::{Lit, Over1, Over3, Paint};
use crate::font::{Font, FontSize, Span};
use crate::page::Page;
use crate::text::{FONT_PAGE, USER_CHARS, UserPage, glyph_origin_px, glyph_pixel_px};

pub const PAGES: usize = 6;
/// `#GRPWIDTH` and `#GRPHEIGHT`.
pub const GRP_SIZE: i32 = 2048;

/// How drawn pixels combine with the page (`#G_NORMAL` to `#G_ADD`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Blend {
    Normal,
    /// Like `Normal`, but fully transparent pixels are skipped.
    Normal2,
    Alpha,
    /// `Alpha` that also weighs the page's own transparency.
    Alpha2,
    Add,
}

impl Blend {
    pub fn from_int(n: i32) -> Option<Blend> {
        Some(match n {
            0 => Blend::Normal,
            1 => Blend::Normal2,
            2 => Blend::Alpha,
            3 => Blend::Alpha2,
            4 => Blend::Add,
            _ => return None,
        })
    }

    #[inline]
    fn apply(self, dst: u32, src: u32) -> u32 {
        match self {
            Blend::Normal => src,
            Blend::Normal2 if src >> 24 == 0 => dst,
            Blend::Normal2 => src,
            _ => self.mix(dst, src),
        }
    }

    /// The modes that weigh the two colours.
    fn mix(self, dst: u32, src: u32) -> u32 {
        let [sa, sr, sg, sb] = src.to_be_bytes().map(u32::from);
        let [da, dr, dg, db] = dst.to_be_bytes().map(u32::from);
        let pack = |a: u32, r: u32, g: u32, b: u32| {
            u32::from_be_bytes([a as u8, r as u8, g as u8, b as u8])
        };
        match self {
            Blend::Normal | Blend::Normal2 => src,
            // Modes 2 and 4 keep the page's alpha; mode 3 combines both.
            // The rounding matches values read back on hardware.
            Blend::Alpha => {
                let mix = |s: u32, d: u32| (s * sa + d * (255 - sa)) / 255;
                pack(da, mix(sr, dr), mix(sg, dg), mix(sb, db))
            }
            // The weights of the two colours are worked out on 0-255 and
            // rounded down before they are used. A clear pixel over a clear
            // pixel leaves it as it was, color included.
            Blend::Alpha2 => {
                let a = sa + da - sa * da / 255;
                if a == 0 {
                    return dst;
                }
                let (sw, dw) = (sa * 255 / a, da * (255 - sa) / a);
                let mix = |s: u32, d: u32| (s * sw + d * dw) / 255;
                pack(a, mix(sr, dr), mix(sg, dg), mix(sb, db))
            }
            Blend::Add => {
                let add = |s: u32, d: u32| (d + s * sa / 255).min(255);
                pack(da, add(sr, dr), add(sg, dg), add(sb, db))
            }
        }
    }
}

/// Per-channel product, alpha included.
pub fn multiply(a: u32, b: u32) -> u32 {
    if b == 0xFFFF_FFFF {
        return a;
    }
    let (a, b) = (a.to_be_bytes(), b.to_be_bytes());
    u32::from_be_bytes(std::array::from_fn(|i| {
        (u32::from(a[i]) * u32::from(b[i]) / 255) as u8
    }))
}

/// GCOPY's rows: each source row onto its target row, the two of a
/// length, multiplied by `tint` and blended. The mode is chosen once for
/// all the rows, so that each row runs a loop of its own mode.
fn copy_rows<'a>(
    rows: impl Iterator<Item = (&'a mut [u32], &'a [u32])>,
    tint: Option<u32>,
    blend: Blend,
) {
    fn each<'a>(
        rows: impl Iterator<Item = (&'a mut [u32], &'a [u32])>,
        pixel: impl Fn(u32, u32) -> u32,
    ) {
        for (to, from) in rows {
            for (d, &p) in to.iter_mut().zip(from) {
                *d = pixel(*d, p);
            }
        }
    }
    match (tint, blend) {
        (None, Blend::Normal) => rows.for_each(|(to, from)| to.copy_from_slice(from)),
        (None, blend) => each(rows, |d, p| blend.apply(d, p)),
        (Some(t), blend) => each(rows, |d, p| blend.apply(d, multiply(p, t))),
    }
}

/// How a sprite's color tints its image as it is drawn.
// Made for one sprite on the stack and dropped; boxing the tables would
// only add an allocation.
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tint {
    /// White, which changes nothing.
    White,
    Color(u32),
    /// A color with, for opaque pixels, each channel's products with it
    /// rounded (blue, green, red), when there are many pixels to tint.
    Tables(u32, [[u8; 256]; 3]),
}

impl Tint {
    /// For about `pixels` pixels.
    pub fn new(color: u32, pixels: usize) -> Tint {
        let table = |shift: u32| {
            let c = color >> shift & 0xFF;
            std::array::from_fn(|v| ((v as u32 * c + 127) / 255) as u8)
        };
        match color {
            0xFFFF_FFFF => Tint::White,
            c if pixels < 4096 || c >> 24 != 0xFF => Tint::Color(c),
            c => Tint::Tables(c, [table(0), table(8), table(16)]),
        }
    }

    /// Runs `job` with what a page pixel puts on the frame, over it or,
    /// with `add`, added to it, as a plain function decided once rather
    /// than for every pixel.
    #[inline]
    pub fn with<J: TintJob>(&self, add: bool, job: J) -> J::Out {
        match (self, add) {
            (Tint::White, false) => job.run(Over1::new),
            (Tint::White, true) => job.run(Lit::new),
            (&Tint::Color(c) | &Tint::Tables(c, _), true) => job.run(move |p| Lit::tinted(p, c)),
            (&Tint::Color(c), false) => job.run(move |p| Over3::new(p, c)),
            (Tint::Tables(c, tables), false) => job.run(move |p| Over3::tabled(p, *c, tables)),
        }
    }
}

/// Work done with a [`Tint`], see [`Tint::with`]. `paint` is only for
/// page pixels that are not clear.
pub trait TintJob {
    type Out;
    fn run<P: Paint>(self, paint: impl Fn(u32) -> P + Copy) -> Self::Out;
}

/// An inclusive rectangle of page coordinates.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Clip {
    pub x0: i32,
    pub y0: i32,
    pub x1: i32,
    pub y1: i32,
}

impl Clip {
    pub const PAGE: Clip = Clip {
        x0: 0,
        y0: 0,
        x1: GRP_SIZE - 1,
        y1: GRP_SIZE - 1,
    };

    /// The rectangle between two corners in any order, limited to a page.
    pub fn new(x0: i32, y0: i32, x1: i32, y1: i32) -> Clip {
        let c = |v: i32| v.clamp(0, GRP_SIZE - 1);
        Clip {
            x0: c(x0.min(x1)),
            y0: c(y0.min(y1)),
            x1: c(x0.max(x1)),
            y1: c(y0.max(y1)),
        }
    }

    fn contains(self, x: i32, y: i32) -> bool {
        x >= self.x0 && x <= self.x1 && y >= self.y0 && y <= self.y1
    }
}

/// How many pixels a side of a tile of [`Dirty`] has, as a power of two.
const TILE_BITS: u32 = 6;
/// How many pixels a side of a tile of [`Dirty`] has.
pub const TILE: i32 = 1 << TILE_BITS;
/// How many tiles a side of a page has.
const TILES: usize = (GRP_SIZE >> TILE_BITS) as usize;

/// Which tiles of a page have changed, a tile being [`TILE`] pixels square.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Dirty {
    /// Of each row of tiles, a bit for each tile.
    rows: [u32; TILES],
}

impl Dirty {
    /// Every tile.
    pub const ALL: Dirty = Dirty {
        rows: [u32::MAX; TILES],
    };

    /// The pixel lies on the page.
    #[inline]
    fn mark_pixel(&mut self, x: i32, y: i32) {
        self.rows[(y >> TILE_BITS) as usize] |= 1 << (x >> TILE_BITS);
    }

    /// The bits of a row of tiles for the columns from `left` to `right`.
    #[inline]
    fn columns(left: i32, right: i32) -> u32 {
        (u32::MAX >> (31 - (right >> TILE_BITS))) & (u32::MAX << (left >> TILE_BITS))
    }

    /// `area` lies on the page.
    #[inline]
    fn mark_area(&mut self, area: Clip) {
        let mask = Dirty::columns(area.x0, area.x1);
        let rows = (area.y0 >> TILE_BITS) as usize..=(area.y1 >> TILE_BITS) as usize;
        self.rows[rows].iter_mut().for_each(|row| *row |= mask);
    }

    pub fn is_empty(&self) -> bool {
        self.rows == [0; TILES]
    }

    /// Marks what `other` has marked.
    pub fn add(&mut self, other: Dirty) {
        for (row, more) in self.rows.iter_mut().zip(other.rows) {
            *row |= more;
        }
    }

    /// Takes off and returns what is marked of the tiles `area` touches, which
    /// lies on the page.
    pub fn take_area(&mut self, area: Clip) -> Dirty {
        let mut taken = Dirty::default();
        let mask = Dirty::columns(area.x0, area.x1);
        for y in (area.y0 >> TILE_BITS) as usize..=(area.y1 >> TILE_BITS) as usize {
            taken.rows[y] = self.rows[y] & mask;
            self.rows[y] &= !mask;
        }
        taken
    }

    /// Whether the tile that holds a pixel is marked.
    pub fn holds(&self, x: i32, y: i32) -> bool {
        self.rows[(y >> TILE_BITS) as usize] >> (x >> TILE_BITS) & 1 != 0
    }

    /// Calls `f` with rectangles of pixels that together make up the tiles
    /// marked, each tile in one, and neighbours together where they can be.
    pub fn for_each_area(&self, mut f: impl FnMut(Clip)) {
        let mut rows = self.rows;
        for y in 0..TILES {
            while rows[y] != 0 {
                let first = rows[y].trailing_zeros();
                let run = (rows[y] >> first).trailing_ones();
                let mask = (u32::MAX >> (32 - run)) << first;
                let height = rows[y..].iter().take_while(|r| *r & mask == mask).count();
                rows[y..y + height].iter_mut().for_each(|r| *r &= !mask);
                let (x, y, run, height) = (first as i32, y as i32, run as i32, height as i32);
                f(Clip {
                    x0: x * TILE,
                    y0: y * TILE,
                    x1: (x + run) * TILE - 1,
                    y1: (y + height) * TILE - 1,
                });
            }
        }
    }
}

pub struct Graphics {
    /// Blank until drawn on.
    pages: Vec<Page>,
    /// GTARGET: the page drawing commands work on.
    pub target: usize,
    /// GCOLOR: the color used when a command leaves it out.
    pub color: u32,
    /// GCLIP.
    /// One rectangle shared by all pages, as on hardware.
    pub clip: Clip,
    /// GSAMPLE of each page: 0 for border, 1 for repeat. Kept for sprites
    /// and text screens, which read pages.
    pub sample: [i32; PAGES],
    /// GARRAY's array of each page, `[y, x]`, which stands for the page's
    /// memory: every pixel drawn is written to it as well, and commands
    /// that read the page read it. The page itself is what the screen
    /// shows, which takes the array's writes on GUPDATE (checked on
    /// hardware: GPGET sees them before).
    arrays: [Option<ArrayRef>; PAGES],
    /// Of each page, a count that moves on whenever its pixels may have
    /// changed, for the debugger's viewer to copy a page only then.
    changes: [u64; PAGES],
    /// Which tiles of each page have changed since the last
    /// [`Graphics::take_dirty`]. Everything that hands out a page's pixels
    /// to write marks what it will write: [`Graphics::page_pixels`] asks.
    dirty: [Cell<Dirty>; PAGES],
    /// Which parts of each page's rows hold something, for sprites to
    /// pass over the clear rest; see [`Graphics::pieces`].
    coverage: RefCell<[Coverage; PAGES]>,
}

/// How many pixels along a row one bit of [`Graphics::pieces`] stands for.
pub const PIECE: usize = 32;

/// Of a page's rows, which [`PIECE`]-pixel pieces have a pixel that is not
/// clear, worked out a piece at a time as asked for, until the page
/// changes.
#[derive(Default)]
struct Coverage {
    /// The page's [`Graphics::changes`] these hold for.
    changes: u64,
    /// Of each row, the pieces worked out.
    known: Vec<u64>,
    /// Of each row, the pieces worked out that hold something.
    filled: Vec<u64>,
}

/// A [`Graphics::changes`] above these bits says which [`Graphics`] it is of.
pub const COUNT_BITS: u32 = 40;

/// Where the next [`Graphics`] starts counting its pages' changes, so
/// that a count is never one that another seen before had.
static CHANGES_FROM: AtomicU64 = AtomicU64::new(0);

impl Default for Graphics {
    fn default() -> Self {
        let from = CHANGES_FROM.fetch_add(1, Ordering::Relaxed) << COUNT_BITS;
        Graphics {
            pages: vec![Page::default(); PAGES],
            target: 0,
            color: 0xFFFF_FFFF,
            clip: Clip::PAGE,
            sample: [0; PAGES],
            arrays: Default::default(),
            changes: [from; PAGES],
            dirty: Default::default(),
            coverage: RefCell::new(std::array::from_fn(|_| Coverage::default())),
        }
    }
}

/// Where a pixel lies in an image of [`GRP_SIZE`] rows of [`GRP_SIZE`]
/// pixels one after another, as GARRAY arrays and files hold a page. The
/// page itself lies otherwise; see [`crate::page`].
fn row_major(x: i32, y: i32) -> usize {
    (y * GRP_SIZE + x) as usize
}

impl Graphics {
    /// A pixel of a page; 0 outside it.
    #[inline]
    pub fn pixel(&self, page: usize, x: i32, y: i32) -> u32 {
        self.pages[page].pixel(x, y)
    }

    /// A page's pixels as shown.
    pub fn page(&self, page: usize) -> &Page {
        &self.pages[page]
    }

    /// A pixel of a page as programs read it: from its GARRAY array if it
    /// has one, else as shown.
    pub fn read(&self, page: usize, x: i32, y: i32) -> u32 {
        if let Some(array) = &self.arrays[page]
            && Clip::PAGE.contains(x, y)
            && let Ok(array) = array.try_borrow()
            && let ArrayData::Int(v) = &array.data
            && let Some(&p) = v.get(row_major(x, y))
        {
            return p as u32;
        }
        self.pixel(page, x, y)
    }

    /// Whether a page is empty, as at start-up or after GCLS with 0.
    pub fn is_blank(&self, page: usize) -> bool {
        self.pages[page].is_blank()
    }

    /// A count that differs from the last one read whenever the page's
    /// pixels may have changed since.
    pub fn changes(&self, page: usize) -> u64 {
        self.changes[page]
    }

    /// The tiles of a page that have changed since this was last called. A
    /// page that went blank in between has them all marked, so what comes
    /// after a blank page is whole. There is one such history per page, for
    /// one holder of a copy of the page.
    pub fn take_dirty(&self, page: usize) -> Dirty {
        self.dirty[page].take()
    }

    /// Of the pieces `from..=to` of row `y` of a page that is not blank,
    /// each [`PIECE`] pixels wide from the page's left edge, those with a
    /// pixel that is not clear (alpha 0), as the bits of their numbers.
    /// Where a sprite's image has nothing, it draws nothing, so it need
    /// only look at these.
    pub fn pieces(&self, page: usize, y: usize, from: usize, to: usize) -> u64 {
        let want = (u64::MAX >> (63 - to)) & (u64::MAX << from);
        let mut all = self.coverage.borrow_mut();
        let c = &mut all[page];
        let rows = GRP_SIZE as usize;
        if c.changes != self.changes[page] || c.known.len() != rows {
            c.changes = self.changes[page];
            c.known.clear();
            c.known.resize(rows, 0);
            c.filled.resize(rows, 0);
        }
        let mut missing = want & !c.known[y];
        if missing != 0 {
            let row = self.pages[page].row(y);
            c.known[y] |= missing;
            c.filled[y] &= !missing;
            while missing != 0 {
                let b = missing.trailing_zeros() as usize;
                missing &= missing - 1;
                let piece = &row[b * PIECE..][..PIECE];
                if piece.iter().fold(0, |a, &p| a | p) >> 24 != 0 {
                    c.filled[y] |= 1 << b;
                }
            }
        }
        c.filled[y] & want
    }

    /// Replaces a whole page, such as GRP4 with the default sprite image;
    /// a blank one empties it, as [`Graphics::clear_page`] does.
    pub fn set_page(&mut self, page: usize, pixels: Page) {
        self.pages[page] = pixels;
        self.changes[page] += 1;
        self.dirty[page].set(Dirty::ALL);
        self.mirror_page(page);
    }

    /// Empties a page, as GCLS does with 0.
    pub fn clear_page(&mut self, page: usize) {
        self.pages[page] = Page::default();
        self.changes[page] += 1;
        self.dirty[page].set(Dirty::ALL);
        self.mirror_page(page);
    }

    /// GTARGET, GCOLOR, GCLIP and GSAMPLE as at start-up.
    pub fn reset_state(&mut self) {
        let pages = std::mem::take(&mut self.pages);
        let arrays = std::mem::take(&mut self.arrays);
        *self = Graphics {
            pages,
            arrays,
            changes: self.changes,
            dirty: std::mem::take(&mut self.dirty),
            ..Graphics::default()
        };
    }

    /// GARRAY: the page as a 2048x2048 Int array, the same one each time.
    pub fn array(&mut self, page: usize) -> ArrayRef {
        if let Some(array) = &self.arrays[page] {
            return array.clone();
        }
        let size = GRP_SIZE as usize;
        let mut array = Array::new(ElemType::Int, vec![size, size]);
        // Only read: a blank page takes memory, but its pixels stay as they were.
        let pixels = self.pixels_to_change(page);
        let mut ints = Vec::with_capacity(size * size);
        for row in pixels.rows(0..size) {
            ints.extend(row.iter().map(|&p| p as i32));
        }
        array.data = ArrayData::Int(ints);
        array.system = true;
        let array = Rc::new(RefCell::new(array));
        self.arrays[page] = Some(array.clone());
        array
    }

    /// The GARRAY arrays programs have asked for.
    pub fn arrays(&self) -> impl Iterator<Item = ArrayRef> + '_ {
        self.arrays.iter().flatten().cloned()
    }

    /// Copies a whole page to its GARRAY array, if it has one.
    fn mirror_page(&mut self, page: usize) {
        let Some(array) = &self.arrays[page] else {
            return;
        };
        let pixels = &self.pages[page];
        if let Ok(mut array) = array.try_borrow_mut()
            && let ArrayData::Int(v) = &mut array.data
        {
            // Row by row as far as the array goes, and 0 past the page.
            let size = GRP_SIZE as usize;
            let mut rest = &mut v[..];
            if !pixels.is_blank() {
                for row in pixels.rows(0..size) {
                    let (to, after) = rest.split_at_mut(rest.len().min(size));
                    for (d, &p) in to.iter_mut().zip(row) {
                        *d = p as i32;
                    }
                    rest = after;
                }
            }
            rest.fill(0);
        }
    }

    /// GUPDATE: the pixels of an inclusive rectangle from the page's
    /// GARRAY array. An array the program has resized is left alone.
    pub fn update(&mut self, page: usize, rect: Clip) {
        let Some(array) = self.arrays[page].clone() else {
            return;
        };
        let Ok(array) = array.try_borrow() else {
            return;
        };
        let ArrayData::Int(v) = &array.data else {
            return;
        };
        if v.len() != (GRP_SIZE * GRP_SIZE) as usize {
            return;
        }
        let pixels = self.page_pixels(page, rect);
        let (x0, x1) = (rect.x0 as usize, rect.x1 as usize);
        for y in rect.y0..=rect.y1 {
            let from = &v[row_major(rect.x0, y)..=row_major(rect.x1, y)];
            for (d, &p) in pixels.row_mut(y as usize)[x0..=x1].iter_mut().zip(from) {
                *d = p as u32;
            }
        }
    }

    /// Marks what falls inside the clipping rectangle of a rectangle of the
    /// target.
    #[inline]
    fn mark_inside(&mut self, x0: i32, y0: i32, x1: i32, y1: i32) {
        let c = self.clip;
        if x0 <= c.x1 && x1 >= c.x0 && y0 <= c.y1 && y1 >= c.y0 {
            self.dirty[self.target].get_mut().mark_area(Clip {
                x0: x0.max(c.x0),
                y0: y0.max(c.y0),
                x1: x1.min(c.x1),
                y1: y1.min(c.y1),
            });
        }
    }

    /// A page's pixels to change, in `area` alone.
    #[inline]
    fn page_pixels(&mut self, page: usize, area: Clip) -> &mut Page {
        self.dirty[page].get_mut().mark_area(area);
        self.pixels_to_change(page)
    }

    /// A page's pixels, with what will change on them marked by the caller.
    #[inline]
    fn pixels_to_change(&mut self, page: usize) -> &mut Page {
        self.changes[page] += 1;
        let pixels = &mut self.pages[page];
        pixels.make_room();
        pixels
    }

    /// Draws one pixel on the target page, inside the clipping rectangle.
    fn plot(&mut self, x: i32, y: i32, color: u32, blend: Blend) {
        if !self.clip.contains(x, y) {
            return;
        }
        self.dirty[self.target].get_mut().mark_pixel(x, y);
        self.plot_marked(x, y, color, blend);
    }

    /// Draws a pixel inside the clipping rectangle, already marked.
    #[inline]
    fn plot_marked(&mut self, x: i32, y: i32, color: u32, blend: Blend) {
        let (px, py) = (x as usize, y as usize);
        let page = self.target;
        // Without a GARRAY array, the page is the only copy.
        if self.arrays[page].is_none() {
            let pixel = self.pixels_to_change(page).pixel_mut(px, py);
            *pixel = blend.apply(*pixel, color);
            return;
        }
        let dst = self.read(page, x, y);
        let p = blend.apply(dst, color);
        *self.pixels_to_change(page).pixel_mut(px, py) = p;
        if let Some(array) = &self.arrays[self.target]
            && let Ok(mut array) = array.try_borrow_mut()
            && let ArrayData::Int(v) = &mut array.data
            && let Some(d) = v.get_mut(row_major(x, y))
        {
            *d = p as i32;
        }
    }

    /// [`Graphics::plot`] for a list of pixels, with less to work out for
    /// each when the page has no GARRAY array.
    fn plot_all(&mut self, points: &[(i32, i32)], color: u32, blend: Blend) {
        let (clip, page) = (self.clip, self.target);
        if self.arrays[page].is_some() {
            for &(x, y) in points {
                self.plot(x, y, color, blend);
            }
            return;
        }
        // A page nothing lands on stays as it is.
        let inside = |&(x, y): &(i32, i32)| clip.contains(x, y);
        let Some(first) = points.iter().position(inside) else {
            return;
        };
        self.pixels_to_change(page);
        let pixels = &mut self.pages[page];
        let dirty = self.dirty[page].get_mut();
        for &(x, y) in &points[first..] {
            if clip.contains(x, y) {
                dirty.mark_pixel(x, y);
                let pixel = pixels.pixel_mut(x as usize, y as usize);
                *pixel = blend.apply(*pixel, color);
            }
        }
    }

    /// GCLS: fills the clipping rectangle.
    pub fn cls(&mut self, color: u32) {
        self.fill(i32::MIN, i32::MIN, i32::MAX, i32::MAX, color, Blend::Normal);
    }

    pub fn pset(&mut self, x: i32, y: i32, color: u32, blend: Blend) {
        self.plot(x, y, color, blend);
    }

    /// GLINE: both ends included.
    pub fn line(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, color: u32, blend: Blend) {
        self.plot_all(&line_points(x0, y0, x1, y1, self.clip), color, blend);
    }

    /// GBOX: the outline as four lines: the top and bottom rows, then the
    /// sides from the top row down to the row above the bottom one, so
    /// that the two top corners are drawn twice (which shows with
    /// `#G_ALPHA`), whichever corners are given. A box one pixel wide or
    /// high has no second side.
    pub fn rect(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, color: u32, blend: Blend) {
        let (l, r) = (x0.min(x1), x0.max(x1));
        let (t, b) = (y0.min(y1), y0.max(y1));
        let c = self.clip;
        self.fill(l, t, r, t, color, blend);
        if b != t {
            self.fill(l, b, r, b, color, blend);
        }
        for y in t.max(c.y0)..=b.saturating_sub(1).min(c.y1) {
            self.plot(l, y, color, blend);
            if r != l {
                self.plot(r, y, color, blend);
            }
        }
    }

    /// GFILL.
    pub fn fill(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, color: u32, blend: Blend) {
        let (left, right) = (x0.min(x1), x0.max(x1));
        let (top, bottom) = (y0.min(y1), y0.max(y1));
        self.mark_inside(left, top, right, bottom);
        let page = self.target;
        if self.arrays[page].is_some() || blend != Blend::Normal {
            self.fill_rows(top..=bottom, |_| Some((left, right)), color, blend);
            return;
        }
        // Plain fills without a GARRAY array: the same columns of each row.
        let c = self.clip;
        let (l, r) = (left.max(c.x0), right.min(c.x1));
        let (t, b) = (top.max(c.y0), bottom.min(c.y1));
        if l > r || t > b {
            return;
        }
        let (l, r, rows) = (l as usize, r as usize, t as usize..b as usize + 1);
        for row in self.pixels_to_change(page).rows_mut(rows) {
            row[l..=r].fill(color);
        }
    }

    /// Fills, in each row of `rows`, the columns `span` gives for it, both
    /// ends included, inside the clipping rectangle, which the caller has
    /// marked.
    fn fill_rows(
        &mut self,
        rows: std::ops::RangeInclusive<i32>,
        mut span: impl FnMut(i32) -> Option<(i32, i32)>,
        color: u32,
        blend: Blend,
    ) {
        let c = self.clip;
        let (top, bottom) = ((*rows.start()).max(c.y0), (*rows.end()).min(c.y1));
        let mut clipped = |y| {
            let (l, r) = span(y)?;
            let (l, r) = (l.max(c.x0), r.min(c.x1));
            (l <= r).then_some((l, r))
        };
        let mut rows = (top..=bottom)
            .filter_map(|y| Some((y, clipped(y)?)))
            .peekable();
        if rows.peek().is_none() {
            return;
        }
        let page = self.target;
        self.pixels_to_change(page);
        // A row at a time, as plot would draw each pixel: reading the
        // GARRAY array where there is one, and writing both.
        let array = self.arrays[page].clone();
        let mut array = array.as_ref().and_then(|a| a.try_borrow_mut().ok());
        let mut mirror = match array.as_deref_mut() {
            Some(sb_core::value::Array {
                data: ArrayData::Int(v),
                ..
            }) => Some(v),
            _ => None,
        };
        let pixels = &mut self.pages[page];
        for (y, (left, right)) in rows {
            let row = &mut pixels.row_mut(y as usize)[left as usize..=right as usize];
            match mirror.as_deref_mut() {
                None if blend == Blend::Normal => row.fill(color),
                None => {
                    for p in row {
                        *p = blend.apply(*p, color);
                    }
                }
                Some(v) => {
                    for (i, p) in (row_major(left, y)..).zip(row) {
                        let dst = v.get(i).map_or(*p, |&d| d as u32);
                        *p = blend.apply(dst, color);
                        if let Some(d) = v.get_mut(i) {
                            *d = *p as i32;
                        }
                    }
                }
            }
        }
    }

    /// GCIRCLE: the whole circle, or with `arc`, the part from one angle
    /// to another (degrees, clockwise from 3 o'clock), as a sector if
    /// `sector` is set. The arc runs between the circle's pixels nearest
    /// the two angles: those at `round(cos*r), round(sin*r)`, ends
    /// included, and a sector's sides are lines from the center to them.
    pub fn circle(
        &mut self,
        (cx, cy, r): (i32, i32, i32),
        arc: Option<(i32, i32, bool)>,
        color: u32,
        blend: Blend,
    ) {
        if r < 0 {
            return;
        }
        let mut points = circle_points(cx, cy, r);
        let mut sides = Vec::new();
        if let Some((start, end, sector)) = arc {
            let (s, e) = (start.rem_euclid(360), end.rem_euclid(360));
            // An arc from an angle to the same angle draws nothing.
            if s == e {
                return;
            }
            let end_at = |angle: i32| {
                let (sin, cos) = f64::from(angle).to_radians().sin_cos();
                let r = f64::from(r);
                ((cos * r).round() as i64, (sin * r).round() as i64)
            };
            let angle =
                |(dx, dy): (i64, i64)| (dy as f64).atan2(dx as f64).to_degrees().rem_euclid(360.0);
            let (from, to) = (end_at(start), end_at(end));
            let first = angle(from);
            let mut sweep = (angle(to) - first).rem_euclid(360.0);
            // An arc of more than half a turn whose two end pixels are
            // the same is the whole circle.
            if sweep == 0.0 && (e - s).rem_euclid(360) > 180 {
                sweep = 360.0;
            }
            points.retain(|&(x, y)| {
                let d = (i64::from(x) - i64::from(cx), i64::from(y) - i64::from(cy));
                (angle(d) - first).rem_euclid(360.0) <= sweep
            });
            if sector {
                // A sector is outlined, not filled (checked on hardware).
                let near = |v: i64| v.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32;
                for (dx, dy) in [from, to] {
                    let (x, y) = (i64::from(cx) + dx, i64::from(cy) + dy);
                    sides.extend(line_points(cx, cy, near(x), near(y), Clip::PAGE));
                }
            }
        }
        if !sides.is_empty() {
            points.extend(sides);
            points.sort_unstable();
            points.dedup();
        }
        self.plot_all(&points, color, blend);
    }

    /// GTRI: a filled triangle, drawn a row at a time from the leftmost
    /// to the rightmost pixel any of its edges reaches in the row, so that
    /// each pixel is drawn once. See [`edge_span`] for an edge's pixels; a
    /// level edge has none of its own, its ends being on the other edges.
    pub fn triangle(&mut self, p: [(i32, i32); 3], color: u32, blend: Blend) {
        let mut sorted = p;
        sorted.sort_by_key(|p| p.1);
        let [a, b, c] = sorted;
        let (top, bottom) = (a.1, c.1);
        if top == bottom {
            let (l, r) = (p.iter().map(|p| p.0).min(), p.iter().map(|p| p.0).max());
            let (l, r) = (l.unwrap(), r.unwrap());
            self.fill(l, top, r, top, color, blend);
            return;
        }
        let span = |y| {
            let spans = [(a, c), (a, b), (b, c)]
                .into_iter()
                .filter_map(|(a, b)| edge_span(a, b, y));
            let (mut l, mut r) = (i64::MAX, i64::MIN);
            for (a, b) in spans {
                (l, r) = (l.min(a), r.max(b));
            }
            let near = |v: i64| v.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32;
            (l <= r).then(|| (near(l), near(r)))
        };
        // An edge can take at most one step past its bottom end.
        let (l, r) = (a.0.min(b.0).min(c.0), a.0.max(b.0).max(c.0));
        self.mark_inside(l.saturating_sub(1), top, r.saturating_add(1), bottom);
        self.fill_rows(top..=bottom, span, color, blend);
    }

    /// GPAINT: fills the area around a point that has the point's color,
    /// or, with `border`, everything up to that color. Gives the number of
    /// pixels filled.
    pub fn paint(&mut self, x: i32, y: i32, color: u32, border: Option<u32>) -> u64 {
        let c = self.clip;
        if !c.contains(x, y) {
            return 0;
        }
        let page = self.target;
        let start = self.read(page, x, y);
        let fills = |p: u32| match border {
            Some(b) => p != b && p != color,
            None => p == start,
        };
        if !fills(start) || (border.is_none() && start == color) {
            return 0;
        }
        let w = (c.x1 - c.x0 + 1) as usize;
        let mut seen = vec![false; w * (c.y1 - c.y0 + 1) as usize];
        let mut stack = vec![(x, y)];
        let mut filled = 0;
        while let Some((x, y)) = stack.pop() {
            if !c.contains(x, y) {
                continue;
            }
            let i = (y - c.y0) as usize * w + (x - c.x0) as usize;
            if seen[i] || !fills(self.read(page, x, y)) {
                continue;
            }
            seen[i] = true;
            filled += 1;
            self.plot(x, y, color, Blend::Normal);
            stack.extend([(x + 1, y), (x - 1, y), (x, y + 1), (x, y - 1)]);
        }
        filled
    }

    /// GCOPY: copies a rectangle of `src` to `(dx, dy)` on the target,
    /// multiplying each pixel by `tint`.
    #[allow(clippy::too_many_arguments)]
    pub fn copy(
        &mut self,
        src: usize,
        (x0, y0, x1, y1): (i32, i32, i32, i32),
        (dx, dy): (i32, i32),
        tint: Option<u32>,
        blend: Blend,
    ) {
        let from = Clip::new(x0, y0, x1, y1);
        let (w, h) = (from.x1 - from.x0 + 1, from.y1 - from.y0 + 1);
        let c = self.clip;
        let (ys, xs) = (
            // Past the largest coordinate wraps, to nothing on the page.
            c.y0.wrapping_sub(dy).max(0)..=c.y1.wrapping_sub(dy).min(h - 1),
            c.x0.wrapping_sub(dx).max(0)..=c.x1.wrapping_sub(dx).min(w - 1),
        );
        if ys.is_empty() || xs.is_empty() {
            return;
        }
        let target = self.target;
        let (n, rows) = (
            (xs.end() - xs.start() + 1) as usize,
            (ys.end() - ys.start() + 1) as usize,
        );
        // Without a GARRAY array to keep the same, rows go straight into
        // the target page, inside the clipping rectangle.
        let area = Clip {
            x0: dx + xs.start(),
            y0: dy + ys.start(),
            x1: dx + xs.end(),
            y1: dy + ys.end(),
        };
        let (tx, ty) = (area.x0 as usize, area.y0 as usize);
        // Rows of another page that is not blank are read where they are,
        // as nothing written can be read again.
        if src != target
            && self.arrays[src].is_none()
            && self.arrays[target].is_none()
            && !self.pages[src].is_blank()
        {
            self.page_pixels(target, area);
            if let Ok([from_page, to_page]) = self.pages.get_disjoint_mut([src, target]) {
                let (sx, sy) = (
                    (from.x0 + xs.start()) as usize,
                    (from.y0 + ys.start()) as usize,
                );
                let from_rows = from_page.rows(sy..sy + rows);
                let to_rows = to_page.rows_mut(ty..ty + rows);
                let pairs = to_rows.zip(from_rows);
                copy_rows(
                    pairs.map(|(d, s)| (&mut d[tx..tx + n], &s[sx..sx + n])),
                    tint,
                    blend,
                );
                return;
            }
        }
        // Read first, so that a copy within one page may overlap.
        let mut pixels = Vec::with_capacity((w * h) as usize);
        if self.arrays[src].is_none() {
            // The source lies within the page; read it a row at a time.
            let page = &self.pages[src];
            if page.is_blank() {
                pixels.resize((w * h) as usize, 0);
            } else {
                let (x0, x1) = (from.x0 as usize, from.x1 as usize);
                for row in page.rows(from.y0 as usize..from.y1 as usize + 1) {
                    pixels.extend_from_slice(&row[x0..=x1]);
                }
            }
        } else {
            for y in 0..h {
                for x in 0..w {
                    pixels.push(self.read(src, from.x0 + x, from.y0 + y));
                }
            }
        }
        if self.arrays[target].is_none() {
            let to_page = self.page_pixels(target, area);
            let first = (ys.start() * w + xs.start()) as usize;
            let from_rows = pixels[first..].chunks(w as usize);
            let to_rows = to_page.rows_mut(ty..ty + rows);
            let pairs = to_rows.zip(from_rows);
            copy_rows(
                pairs.map(|(d, s)| (&mut d[tx..tx + n], &s[..n])),
                tint,
                blend,
            );
            return;
        }
        for y in ys {
            for x in xs.clone() {
                let p = pixels[(y * w + x) as usize];
                let p = tint.map_or(p, |t| multiply(p, t));
                self.plot(dx + x, dy + y, p, blend);
            }
        }
    }

    /// GPUTCHR: draws text with the 8 or 16-pixel font, scaled. The whole
    /// cell is drawn, so with `#G_NORMAL` the glyph's transparent pixels
    /// clear the page, as on hardware. User-defined characters come from
    /// `user`, the console's TPAGE, in cells of the font type.
    #[allow(clippy::too_many_arguments)]
    pub fn put_chars(
        &mut self,
        font: &Font,
        user: UserPage,
        (x, y): (i32, i32),
        text: &[u16],
        px: u32,
        (sx, sy): (i32, i32),
        color: u32,
        blend: Blend,
    ) {
        let n = px as i32;
        let (top, cell) = (i64::from(y), i64::from(n) * i64::from(sx));
        if !self.reaches_rows(top, n, sy) || nothing_drawn(color, blend) {
            return;
        }
        for (i, &code) in text.iter().enumerate() {
            let left = i64::from(x) + i as i64 * cell;
            if left > i64::from(self.clip.x1) {
                break;
            }
            if left + cell > i64::from(self.clip.x0) {
                self.put_char(font, user, code, px, (left, top), (sx, sy), color, blend);
            }
        }
    }

    /// Draws one character's `px`-pixel cell, as [`Graphics::put_glyph`]
    /// does. Where its pixels come from is worked out once for the
    /// character, not for each pixel.
    #[allow(clippy::too_many_arguments)]
    fn put_char(
        &mut self,
        font: &Font,
        user: UserPage,
        code: u16,
        px: u32,
        at: (i64, i64),
        scale: (i32, i32),
        color: u32,
        blend: Blend,
    ) {
        let n = px as i32;
        if USER_CHARS.contains(&code) {
            // GSAMPLE 1 wraps round the page as it does for text screens,
            // GSAMPLE 0 draws nothing past the page's edge, and the page's
            // colors are multiplied by the color as the font's are.
            let (cx, cy) = user.cell(code, n);
            let on = |gx: u32, gy: u32| (cx + gx as i32, cy + gy as i32);
            match usize::try_from(user.page) {
                // After TPAGE -1 nothing is drawn. UNVERIFIED: that mode 0
                // does not clear the cell either.
                Err(_) => {}
                Ok(p) if p >= PAGES => {
                    self.put_glyph(at, n, scale, color, blend, |_, _, _| Some(0))
                }
                Ok(p) if self.sample[p] == 1 => {
                    self.put_glyph(at, n, scale, color, blend, |g, gx, gy| {
                        let (x, y) = on(gx, gy);
                        Some(g.pixel(p, x.rem_euclid(GRP_SIZE), y.rem_euclid(GRP_SIZE)))
                    })
                }
                Ok(p) => self.put_glyph(at, n, scale, color, blend, |g, gx, gy| {
                    let (x, y) = on(gx, gy);
                    Clip::PAGE.contains(x, y).then(|| g.pixel(p, x, y))
                }),
            }
            return;
        }
        match glyph_origin_px(font, code, px) {
            Some((ox, oy)) => self.put_glyph(at, n, scale, color, blend, |g, gx, gy| {
                Some(g.pixel(FONT_PAGE, (ox + gx) as i32, (oy + gy) as i32))
            }),
            None => self.put_glyph(at, n, scale, color, blend, |g, gx, gy| {
                Some(glyph_pixel_px(font, g, code, px, gx, gy))
            }),
        }
    }

    /// Whether `rows` rows of glyph pixels, `scale` high from `top`, reach
    /// into the clipping rectangle.
    fn reaches_rows(&self, top: i64, rows: i32, scale: i32) -> bool {
        top <= i64::from(self.clip.y1)
            && top + i64::from(rows) * i64::from(scale) > i64::from(self.clip.y0)
    }

    /// Draws an `n` by `n` glyph, scaled, with its top left at `(left,
    /// top)`. A scale can make a glyph far larger than a page, so this
    /// goes only through what lands in the clipping rectangle, in the
    /// order the whole glyph would be. A glyph pixel of `None` draws
    /// nothing in any mode.
    #[allow(clippy::too_many_arguments)]
    fn put_glyph(
        &mut self,
        (left, top): (i64, i64),
        n: i32,
        (sx, sy): (i32, i32),
        color: u32,
        blend: Blend,
        glyph: impl Fn(&Graphics, u32, u32) -> Option<u32>,
    ) {
        let clip = self.clip;
        let (sx, sy) = (i64::from(sx), i64::from(sy));
        // The glyph pixels in reach, and of each, the page pixels in reach.
        let steps = |start: i64, scale: i64, count: i32, low: i32, high: i32| {
            let step = |at: i32| (i64::from(at) - start).div_euclid(scale);
            let count = i64::from(count);
            step(low).clamp(0, count)..(step(high) + 1).clamp(0, count)
        };
        let inner = |start: i64, scale: i64, low: i32, high: i32| {
            (i64::from(low) - start).max(0)..(i64::from(high) - start + 1).min(scale)
        };
        let across = steps(left, sx, n, clip.x0, clip.x1);
        let near = |v: i64| v.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32;
        self.mark_inside(
            near(left),
            near(top),
            near(left + i64::from(n) * sx - 1),
            near(top + i64::from(n) * sy - 1),
        );
        // Only mode 0 changes the page with a clear pixel; glyphs are
        // mostly clear, so the others pass them by. Mode 1 looks at
        // the glyph pixel's own alpha, not at the alpha the color makes
        // of it: with a color of alpha 1 the glyph's soft edges are
        // written with alpha 0.
        let skip_clear = blend != Blend::Normal;
        let blend = match blend {
            Blend::Normal2 => Blend::Normal,
            blend => blend,
        };
        for gy in steps(top, sy, n, clip.y0, clip.y1) {
            let y0 = top + gy * sy;
            for gx in across.clone() {
                let Some(g) = glyph(self, gx as u32, gy as u32) else {
                    continue;
                };
                if skip_clear && g >> 24 == 0 {
                    continue;
                }
                // Glyphs are mostly clear or white.
                let p = match g {
                    0 => 0,
                    0xFFFF_FFFF => color,
                    g => multiply(g, color),
                };
                let x0 = left + gx * sx;
                for py in inner(y0, sy, clip.y0, clip.y1) {
                    for px in inner(x0, sx, clip.x0, clip.x1) {
                        self.plot_marked((x0 + px) as i32, (y0 + py) as i32, p, blend);
                    }
                }
            }
        }
    }

    /// GPUTCHRP's function form: how wide `text` is drawn, the spacing
    /// coming between characters and scaled with them.
    pub fn proportional_width(
        &self,
        font: &Font,
        text: &[u16],
        size: FontSize,
        sx: i32,
        spacing: i32,
    ) -> i32 {
        let widths: i64 = (text.iter())
            .map(|&c| i64::from(span(font, c, size).width))
            .sum();
        let gaps = i64::from(spacing) * (text.len() as i64 - 1).max(0);
        // Too wide for an integer wraps round.
        ((widths + gaps) * i64::from(sx)) as i32
    }

    /// GPUTCHRP: characters as wide as each is, `spacing` pixels apart.
    /// User-defined characters come from `user`, as for GPUTCHR.
    #[allow(clippy::too_many_arguments)]
    pub fn put_chars_proportional(
        &mut self,
        font: &Font,
        user: UserPage,
        (x, y): (i32, i32),
        text: &[u16],
        size: FontSize,
        (sx, sy): (i32, i32),
        color: u32,
        blend: Blend,
        spacing: i32,
    ) {
        let n = size as i32;
        let top = i64::from(y);
        if !self.reaches_rows(top, n, sy) || nothing_drawn(color, blend) {
            return;
        }
        let (sx64, cell) = (i64::from(sx), i64::from(n) * i64::from(sx));
        let mut pen = i64::from(x);
        for &code in text {
            // A cell starts less than its size left of the pen, and the
            // pen only moves right, so nothing later comes back.
            if pen - cell > i64::from(self.clip.x1) {
                break;
            }
            let Span { first, width } = span(font, code, size);
            // The whole cell is drawn, with column `first` on the pen, so
            // mode 0 clears the cell's blank columns too.
            let left = pen - i64::from(first) * sx64;
            if left + cell > i64::from(self.clip.x0) {
                let at = (left, top);
                self.put_char(font, user, code, n as u32, at, (sx, sy), color, blend);
            }
            // Drawing scales the spacing as the width does.
            pen += i64::from(width + spacing) * sx64;
        }
    }

    /// GSAVE: a rectangle of a page, row by row.
    pub fn save_rect(&self, page: usize, rect: (i32, i32, i32, i32)) -> Vec<u32> {
        let (w, h) = (rect.2.max(0) as usize, rect.3.max(0) as usize);
        let mut pixels = vec![0; w * h];
        for (col, row, x, y) in on_page(rect) {
            pixels[row * w + col] = self.read(page, x, y);
        }
        pixels
    }

    /// LOADG: copies an image onto a page at `(x, y)` as it is, without
    /// clipping or blending; what falls off the page is dropped.
    pub fn put_image(&mut self, page: usize, (x, y): (i32, i32), image: &crate::image::Image) {
        let width = image.width.max(1);
        let height = image.pixels.len().div_ceil(width);
        let last =
            |at: i32, len: usize| at.saturating_add(len.max(1).min(i32::MAX as usize) as i32 - 1);
        let area = Clip::new(x, y, last(x, width), last(y, height));
        let pixels = self.page_pixels(page, area);
        for (row, line) in image.pixels.chunks(width).enumerate() {
            let py = y + row as i32;
            for (col, &p) in line.iter().enumerate() {
                let px = x + col as i32;
                if Clip::PAGE.contains(px, py) {
                    *pixels.pixel_mut(px as usize, py as usize) = p;
                }
            }
        }
        self.mirror_page(page);
    }

    /// GLOAD: draws row-by-row pixels into a rectangle of the target page.
    /// With a palette, the pixels are its indexes; one past its end leaves
    /// the page as it is.
    pub fn load_rect(
        &mut self,
        (x, y, w, h): (i32, i32, i32, i32),
        pixels: &[u32],
        palette: Option<&[u32]>,
        tint: Option<u32>,
        blend: Blend,
    ) {
        let w = w.max(0) as usize;
        if palette.is_none() && self.arrays[self.target].is_none() {
            let tint = tint.unwrap_or(0xFFFF_FFFF);
            self.load_rows((x, y, w as i32, h), pixels, |p| p, tint, blend);
            return;
        }
        for (col, row, px, py) in on_page((x, y, w as i32, h)) {
            let Some(&p) = pixels.get(row * w + col) else {
                break;
            };
            let p = match palette {
                None => p,
                Some(colors) => match colors.get(p as usize) {
                    Some(&c) => c,
                    None => continue,
                },
            };
            let p = tint.map_or(p, |t| multiply(p, t));
            self.plot(px, py, p, blend);
        }
    }

    /// GLOAD straight from an integer array, when [`Graphics::load_direct`]
    /// says that can be done, so that the array need not be copied first.
    pub fn load_ints(
        &mut self,
        rect: (i32, i32, i32, i32),
        pixels: &[i32],
        tint: Option<u32>,
        blend: Blend,
    ) {
        let tint = tint.unwrap_or(0xFFFF_FFFF);
        self.load_rows(rect, pixels, |p| p as u32, tint, blend);
    }

    /// Whether the target page has no GARRAY array to keep in step, as
    /// [`Graphics::load_ints`] needs.
    pub fn load_direct(&self) -> bool {
        self.arrays[self.target].is_none()
    }

    /// GLOAD without a palette, on a page with no GARRAY array, a row at a
    /// time: the part of the rectangle inside the clipping rectangle, for
    /// as far as `pixels` goes. A page nothing lands on stays as it is.
    fn load_rows<T: Copy>(
        &mut self,
        (x, y, w, h): (i32, i32, i32, i32),
        pixels: &[T],
        color: impl Fn(T) -> u32,
        tint: u32,
        blend: Blend,
    ) {
        let c = self.clip;
        let (x, y) = (i64::from(x), i64::from(y));
        let (w, h) = (i64::from(w.max(0)), i64::from(h.max(0)));
        let (left, right) = (x.max(i64::from(c.x0)), (x + w - 1).min(i64::from(c.x1)));
        let (top, bottom) = (y.max(i64::from(c.y0)), (y + h - 1).min(i64::from(c.y1)));
        let line_at = |py: i64| (py - y) * w + (left - x);
        if left > right || top > bottom || line_at(top) as usize >= pixels.len() {
            return;
        }
        let area = Clip {
            x0: left as i32,
            y0: top as i32,
            x1: right as i32,
            y1: bottom as i32,
        };
        let page = self.page_pixels(self.target, area);
        for py in top..=bottom {
            let line = pixels.get(line_at(py) as usize..).unwrap_or(&[]);
            let line = &line[..line.len().min((right - left + 1) as usize)];
            let row = &mut page.row_mut(py as usize)[left as usize..][..line.len()];
            match blend {
                Blend::Normal if tint == 0xFFFF_FFFF => {
                    for (d, &p) in row.iter_mut().zip(line) {
                        *d = color(p);
                    }
                }
                _ => {
                    for (d, &p) in row.iter_mut().zip(line) {
                        *d = blend.apply(*d, multiply(color(p), tint));
                    }
                }
            }
        }
    }
}

/// The pixels of an X,Y,width,height rectangle that lie on the page, row
/// by row, as the column and row within the rectangle and the place on
/// the page; the rest of a rectangle far larger than the page costs
/// nothing.
fn on_page((x, y, w, h): (i32, i32, i32, i32)) -> impl Iterator<Item = (usize, usize, i32, i32)> {
    let size = i64::from(GRP_SIZE);
    let span = |from: i32, len: i32| {
        let from = i64::from(from);
        (from.max(0), (from + i64::from(len.max(0))).min(size), from)
    };
    let ((x0, x1, left), (y0, y1, top)) = (span(x, w), span(y, h));
    (y0..y1).flat_map(move |py| {
        (x0..x1).map(move |px| {
            (
                (px - left) as usize,
                (py - top) as usize,
                px as i32,
                py as i32,
            )
        })
    })
}

/// The pixels of the circle of radius `r` around `(cx, cy)` that lie on
/// the page, without repeats and in no set order: the midpoint circle, whose pixel in each
/// row of the first octant is the largest `x` with `x² - x + y² < r²`.
/// Only the rows and columns that reach the page are worked out, so that
/// huge circles take no longer than small ones.
fn circle_points(cx: i32, cy: i32, r: i32) -> Vec<(i32, i32)> {
    let on_page = |x: i64, y: i64| {
        (0..i64::from(GRP_SIZE)).contains(&x) && (0..i64::from(GRP_SIZE)).contains(&y)
    };
    let (cx, cy, r) = (i64::from(cx), i64::from(cy), i64::from(r));
    let mut points = Vec::new();
    if r == 0 {
        if on_page(cx, cy) {
            points.push((cx as i32, cy as i32));
        }
        return points;
    }
    // r is at most 2^31, so r² and x² fit in an i64.
    let rr = r * r;
    // The octant's pixel in row y.
    let x_at = |y: i64| {
        let k = rr - y * y;
        let mut x = ((1.0 + (1.0 + 4.0 * k as f64).sqrt()) / 2.0) as i64;
        while x > 0 && x * (x - 1) >= k {
            x -= 1;
        }
        while (x + 1) * x < k {
            x += 1;
        }
        x
    };
    // The octant ends at the last row whose pixel is not past the diagonal.
    let (mut lo, mut hi) = (0, r);
    while lo < hi {
        let mid = lo + (hi - lo + 1) / 2;
        if x_at(mid) >= mid {
            lo = mid;
        } else {
            hi = mid - 1;
        }
    }
    let last = lo;
    let page = i64::from(GRP_SIZE) - 1;
    if r <= cx.min(cy) && cx.max(cy) + r <= page {
        // All on the page: one walk of the octant.
        let (mut x, at) = (r, |x: i64, y: i64| ((cx + x) as i32, (cy + y) as i32));
        points.reserve(8 * (last as usize + 1));
        // A row's eight points can only repeat each other on the axes
        // (y 0) and on the diagonal (x = y), and never another row's, so
        // those rows give four and nothing needs sorting out: sorting took
        // 40% of TPERF's GCIRCLE scene.
        for y in 0..=last {
            while x * (x - 1) >= rr - y * y {
                x -= 1;
            }
            if y == 0 {
                points.extend([at(x, 0), at(0, x), at(-x, 0), at(0, -x)]);
            } else if x == y {
                points.extend([at(x, x), at(-x, x), at(-x, -x), at(x, -x)]);
            } else {
                points.extend([
                    at(x, y),
                    at(y, x),
                    at(-y, x),
                    at(-x, y),
                    at(-x, -y),
                    at(-y, -x),
                    at(y, -x),
                    at(x, -y),
                ]);
            }
        }
        return points;
    }
    // Rows of the octant whose points land on the page: those whose
    // offset from the center (as a row, or as a column) is on it. Each
    // group of rows is walked once, its pixel worked out from the last.
    let groups = [
        (cy, 1, false),
        (cy, -1, false),
        (cx, 1, true),
        (cx, -1, true),
    ]
    .map(|(base, sign, column)| {
        let (from, to) = if sign > 0 {
            (-base, page - base)
        } else {
            (base - page, base)
        };
        (from.max(0), to.min(last), sign, column)
    });
    let mut walks: Vec<(i64, i64)> = groups
        .iter()
        .filter(|g| g.0 <= g.1)
        .map(|g| (g.0, g.1))
        .collect();
    walks.sort_unstable();
    let mut merged: Vec<(i64, i64)> = Vec::new();
    for (from, to) in walks {
        match merged.last_mut() {
            Some(m) if from <= m.1 + 1 => m.1 = m.1.max(to),
            _ => merged.push((from, to)),
        }
    }
    for (from, to) in merged {
        let mut x = x_at(from);
        for y in from..=to {
            let k = rr - y * y;
            while x * (x - 1) >= k {
                x -= 1;
            }
            for &(lo, hi, sign, column) in &groups {
                if !(lo..=hi).contains(&y) {
                    continue;
                }
                let near = sign * y;
                let pair = if column {
                    [(cx + near, cy + x), (cx + near, cy - x)]
                } else {
                    [(cx + x, cy + near), (cx - x, cy + near)]
                };
                for (px, py) in pair {
                    if on_page(px, py) {
                        points.push((px as i32, py as i32));
                    }
                }
            }
        }
    }
    points.sort_unstable();
    points.dedup();
    points
}

/// The pixels of a GLINE from `(x0, y0)` to `(x1, y1)` that lie on the
/// page: the same whichever end comes first, walked along the longer axis
/// from the end with the smaller coordinate on it, `major` steps long
/// and `minor` across (taken as positive). The offset across at step `t`
/// is `(2·t·minor + k) / (2·major)` rounded down, where `k` is
/// `(major + 2·minor) mod (2·major)` when the walk starts inside GCLIP:
/// the line as rounded one step further on, moved back so that it starts
/// at its first end (`round((t+1)·minor/major) - round(minor/major)`,
/// halves up). A walk that starts outside takes `k = major`, the
/// line itself rounded (halves up), counted from its real start; only
/// the steps on the page are worked out.
fn line_points(x0: i32, y0: i32, x1: i32, y1: i32, clip: Clip) -> Vec<(i32, i32)> {
    let x_major = (i64::from(x1) - i64::from(x0)).abs() >= (i64::from(y1) - i64::from(y0)).abs();
    let swap = |(x, y): (i64, i64)| if x_major { (x, y) } else { (y, x) };
    let ends = (
        swap((i64::from(x0), i64::from(y0))),
        swap((i64::from(x1), i64::from(y1))),
    );
    let (a, b) = if ends.0.0 <= ends.1.0 {
        ends
    } else {
        (ends.1, ends.0)
    };
    let major = i128::from(b.0 - a.0);
    let minor = i128::from(b.1 - a.1);
    let (across, sign) = (minor.abs(), minor.signum() as i64);
    let page = i64::from(GRP_SIZE) - 1;
    let (sx, sy) = swap(a);
    let starts_inside = (i64::from(clip.x0)..=i64::from(clip.x1)).contains(&sx)
        && (i64::from(clip.y0)..=i64::from(clip.y1)).contains(&sy);
    let k = if starts_inside {
        (major + 2 * across) % (2 * major).max(1)
    } else {
        major
    };
    let from = (-a.0).max(0);
    let to = (page - a.0).min(b.0 - a.0);
    let mut points = Vec::with_capacity((to - from + 1).max(0) as usize);
    if from > to {
        return points;
    }
    // The offset and what is left over, stepped along without dividing.
    let step = 2 * major.max(1);
    let start = 2 * i128::from(from) * across + k;
    let (mut offset, mut left) = ((start / step) as i64, (start % step) as i64);
    let (step, rise) = (step as i64, 2 * across as i64);
    for t in from..=to {
        let (x, y) = swap((a.0 + t, a.1 + sign * offset));
        if (0..=page).contains(&x) && (0..=page).contains(&y) {
            points.push((x as i32, y as i32));
        }
        left += rise;
        if left >= step {
            left -= step;
            offset += 1;
        }
    }
    points
}

/// The pixels a GTRI edge takes in row `y`, as the first and last column,
/// or `None` if it does not reach the row or is level. Going down from its
/// top end, each row adds `2*wide` to a sum that starts at `-wide` if the
/// edge is wider than tall, or `tall` if not, and the edge takes a step
/// across for each `2*tall` in the sum, at most one past its bottom end; a
/// row has the columns from the one the last row ended at to the one before
/// where this row ends.
fn edge_span(a: (i32, i32), b: (i32, i32), y: i32) -> Option<(i64, i64)> {
    let ((x0, y0), (x1, y1)) = (a, b);
    if y < y0.min(y1) || y > y0.max(y1) {
        return None;
    }
    // Products of two coordinates and a few doublings fit in i64 while
    // the coordinates are small, as on screen; dividing i128s is slow.
    const SMALL: i32 = 1 << 28;
    if [x0, y0, x1, y1, y]
        .iter()
        .all(|v| (-SMALL..SMALL).contains(v))
    {
        return edge_span_in::<i64>(a, b, y);
    }
    edge_span_in::<i128>(a, b, y)
}

/// Integers [`edge_span`] can work in.
trait Wide:
    Copy
    + Ord
    + From<i32>
    + std::ops::Add<Output = Self>
    + std::ops::Sub<Output = Self>
    + std::ops::Mul<Output = Self>
    + std::ops::Neg<Output = Self>
{
    fn div_euclid(self, d: Self) -> Self;
    fn abs(self) -> Self;
    fn narrow(self) -> i64;
}

impl Wide for i64 {
    fn div_euclid(self, d: Self) -> Self {
        i64::div_euclid(self, d)
    }
    fn abs(self) -> Self {
        i64::abs(self)
    }
    fn narrow(self) -> i64 {
        self
    }
}

impl Wide for i128 {
    fn div_euclid(self, d: Self) -> Self {
        i128::div_euclid(self, d)
    }
    fn abs(self) -> Self {
        i128::abs(self)
    }
    fn narrow(self) -> i64 {
        self as i64
    }
}

fn edge_span_in<T: Wide>(a: (i32, i32), b: (i32, i32), y: i32) -> Option<(i64, i64)> {
    // From the top end to the bottom one.
    let ((x0, y0), (x1, y1)) = if a.1 <= b.1 { (a, b) } else { (b, a) };
    if y0 == y1 {
        return None;
    }
    let (x0, y0, x1, y1, y) = (
        T::from(x0),
        T::from(y0),
        T::from(x1),
        T::from(y1),
        T::from(y),
    );
    let (zero, one, two) = (T::from(0), T::from(1), T::from(2));
    let (dx, tall, row) = (x1 - x0, y1 - y0, y - y0);
    let wide = dx.abs();
    let start = if tall < wide { -wide } else { tall };
    // Steps taken by the end of a row.
    let steps = |row: T| {
        if row < zero {
            return zero;
        }
        (start + two * wide * (row + one))
            .div_euclid(two * tall)
            .min(wide + one)
    };
    let (from, to) = (steps(row - one), steps(row));
    let last = if to > from { to - one } else { to };
    let (l, r) = if dx < zero {
        (x0 - last, x0 - from)
    } else {
        (x0 + from, x0 + last)
    };
    Some((l.narrow(), r.narrow()))
}

/// Whether GPUTCHR and GPUTCHRP draw nothing at all: mode 1 skips
/// transparent pixels, and with a transparent color every pixel is.
fn nothing_drawn(color: u32, blend: Blend) -> bool {
    blend == Blend::Normal2 && color >> 24 == 0
}

/// GPUTCHRP's [`Span`] of a character. A user-defined one takes its
/// whole cell, whatever the cell holds; others are the font's.
fn span(font: &Font, code: u16, size: FontSize) -> Span {
    match USER_CHARS.contains(&code) {
        true => Span {
            first: 0,
            width: size as i32,
        },
        false => font.span(code, size),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pieces_follow_the_page() {
        let mut g = Graphics::default();
        g.pset(40, 3, 0x0100_0000, Blend::Normal);
        g.pset(100, 3, 0x00FF_FFFF, Blend::Normal);
        assert_eq!(g.pieces(0, 3, 0, 63), 1 << 1);
        assert_eq!(g.pieces(0, 3, 2, 63), 0);
        assert_eq!(g.pieces(0, 4, 0, 63), 0);
        g.pset(2047, 3, 0xFF00_0000, Blend::Normal);
        g.pset(40, 3, 0, Blend::Normal);
        assert_eq!(g.pieces(0, 3, 0, 63), 1 << 63);
    }

    const RED: u32 = 0xFFFF_0000;

    /// The pixels of a small area as text: `#` for RED, `.` for 0.
    fn area(g: &Graphics, w: i32, h: i32) -> Vec<String> {
        (0..h)
            .map(|y| {
                (0..w)
                    .map(|x| match g.pixel(0, x, y) {
                        0 => '.',
                        RED => '#',
                        _ => '?',
                    })
                    .collect()
            })
            .collect()
    }

    #[test]
    fn pages_start_empty_and_out_of_range_reads_zero() {
        let mut g = Graphics::default();
        assert_eq!(g.pixel(0, 5, 5), 0);
        g.pset(5, 5, RED, Blend::Normal);
        assert_eq!(g.pixel(0, 5, 5), RED);
        assert_eq!(g.pixel(0, -1, 0), 0);
        assert_eq!(g.pixel(0, 2048, 0), 0);
        assert_eq!(g.pixel(1, 5, 5), 0);
    }

    /// Runs `draw` and checks that every pixel it changed, on any page, is in
    /// a tile marked as changed.
    fn check_marks(g: &mut Graphics, name: &str, draw: impl FnOnce(&mut Graphics)) {
        let before: Vec<Page> = (0..PAGES).map(|p| g.page(p).clone()).collect();
        (0..PAGES).for_each(|p| {
            g.take_dirty(p);
        });
        draw(g);
        let mut changed = 0;
        for (p, was) in before.iter().enumerate() {
            let dirty = g.take_dirty(p);
            let now = g.page(p);
            if was.is_blank() && now.is_blank() {
                continue;
            }
            let (was, now) = (was.to_rows(), now.to_rows());
            for i in 0..was.len() {
                if was[i] != now[i] {
                    changed += 1;
                    let (x, y) = (
                        (i % GRP_SIZE as usize) as i32,
                        (i / GRP_SIZE as usize) as i32,
                    );
                    assert!(
                        dirty.holds(x, y),
                        "{name}: page {p} pixel {x},{y} not marked"
                    );
                }
            }
        }
        assert!(changed > 0, "{name}: this case changes nothing");
    }

    /// Each way of writing on a page, on a page with something on it so
    /// that a write shows, with and without a GARRAY array and GCLIP.
    #[test]
    fn every_write_marks_what_it_changes() {
        let font = Font::fallback();
        let text: Vec<u16> = "AB".encode_utf16().collect();
        let image = crate::image::Image {
            width: 3,
            height: 2,
            pixels: vec![RED; 6],
        };
        for (arrayed, clip) in [
            (false, Clip::PAGE),
            (true, Clip::PAGE),
            (false, Clip::new(100, 100, 700, 500)),
        ] {
            let mut g = Graphics::default();
            for page in 0..PAGES {
                g.set_page(
                    page,
                    Page::from_rows(vec![0xFF10_2030; (GRP_SIZE * GRP_SIZE) as usize]),
                );
            }
            g.clip = clip;
            if arrayed {
                g.array(0);
            }
            let n = Blend::Normal;
            let mut check =
                |name: &str, draw: &dyn Fn(&mut Graphics)| check_marks(&mut g, name, draw);
            check("GPSET", &|g| g.pset(130, 140, RED, n));
            check("GLINE", &|g| g.line(110, 120, 690, 480, RED, n));
            check("GLINE off", &|g| g.line(-9000, 300, 9000, 330, RED, n));
            check("GBOX", &|g| g.rect(150, 160, 600, 400, 0xFF00_FF00, n));
            check("GFILL", &|g| g.fill(200, 210, 330, 340, 0xFF00_00FF, n));
            check("GFILL blend", &|g| {
                g.fill(250, 260, 400, 410, 0x80FF_FFFF, Blend::Alpha)
            });
            check("GCLS", &|g| g.cls(0xFF40_4040));
            check("GCIRCLE", &|g| g.circle((300, 300, 150), None, RED, n));
            check("GCIRCLE arc", &|g| {
                g.circle((300, 300, 180), Some((10, 200, true)), RED, n)
            });
            check("GTRI", &|g| {
                g.triangle([(120, 110), (640, 300), (200, 490)], 0xFF0F_F000, n)
            });
            check("GTRI thin", &|g| {
                g.triangle([(110, 450), (690, 451), (400, 452)], RED, n)
            });
            check("GTRI flat", &|g| {
                g.triangle([(110, 300), (690, 300), (400, 300)], 0xFF00_FF00, n)
            });
            check("GTRI wide", &|g| {
                g.triangle([(-500, 200), (1500, 210), (-400, 220)], 0xFF12_3456, n)
            });
            check("GPAINT", &|g| {
                g.paint(105, 105, 0xFF77_7777, None);
            });
            check("GPUTCHR", &|g| {
                g.put_chars(
                    &font,
                    UserPage::DEFAULT,
                    (300, 200),
                    &text,
                    8,
                    (3, 2),
                    RED,
                    n,
                )
            });
            check("GPUTCHRP", &|g| {
                g.put_chars_proportional(
                    &font,
                    UserPage::DEFAULT,
                    (120, 330),
                    &text,
                    FontSize::Large,
                    (2, 3),
                    RED,
                    n,
                    1,
                )
            });
            check("GCOPY", &|g| {
                g.copy(1, (0, 0, 300, 300), (150, 120), None, n)
            });
            check("GCOPY tint", &|g| {
                g.copy(
                    0,
                    (100, 100, 200, 200),
                    (400, 300),
                    Some(0xFF80_8080),
                    Blend::Alpha,
                )
            });
            check("GLOAD", &|g| {
                g.load_rect((130, 140, 100, 3), &[RED; 300], None, None, n)
            });
            check("GLOAD palette", &|g| {
                g.load_rect(
                    (130, 200, 2, 1),
                    &[0, 1],
                    Some(&[0xFF00_1100, 0xFF00_2200]),
                    None,
                    n,
                )
            });
            check("LOADG", &|g| g.put_image(0, (140, 150), &image));
            check("LOADG off", &|g| g.put_image(0, (2047, 2047), &image));
            check("page replaced", &|g| {
                g.set_page(
                    2,
                    Page::from_rows(vec![RED; (GRP_SIZE * GRP_SIZE) as usize]),
                )
            });
            check("page emptied", &|g| g.clear_page(2));
            check("page drawn again", &|g| {
                g.target = 2;
                g.fill(120, 130, 125, 135, RED, n);
                g.target = 0;
            });
            if arrayed {
                check("GUPDATE", &|g| {
                    let array = g.array(0);
                    if let ArrayData::Int(v) = &mut array.borrow_mut().data {
                        v[row_major(160, 170)] = 0x0102_0304;
                    }
                    g.update(0, Clip::new(0, 0, 2047, 2047));
                });
            }
            // What is written while drawing elsewhere is that page's.
            check("another target", &|g| {
                g.target = 3;
                g.line(110, 120, 150, 180, RED, n);
                g.target = 0;
            });
        }
    }

    #[test]
    fn marks_stay_near_what_is_drawn_and_through_a_reset() {
        let mut g = Graphics::default();
        g.pset(5, 5, RED, Blend::Normal);
        let mut areas = Vec::new();
        g.reset_state();
        let dirty = g.take_dirty(0);
        dirty.for_each_area(|a| areas.push(a));
        assert_eq!(areas, [Clip::new(0, 0, TILE - 1, TILE - 1)]);
        assert!(g.take_dirty(0).is_empty());
        // Past the clipping rectangle nothing is drawn or marked.
        g.clip = Clip::new(100, 100, 110, 110);
        g.fill(0, 0, 50, 50, RED, Blend::Normal);
        g.line(0, 0, 50, 0, RED, Blend::Normal);
        assert!(g.take_dirty(0).is_empty());
    }

    #[test]
    fn an_area_takes_off_the_tiles_it_touches() {
        let mut d = Dirty::ALL;
        let taken = d.take_area(Clip::new(TILE, 0, TILE + 1, TILE));
        let mut areas = Vec::new();
        taken.for_each_area(|a| areas.push(a));
        assert_eq!(areas, [Clip::new(TILE, 0, 2 * TILE - 1, 2 * TILE - 1)]);
        assert!(!d.holds(TILE, 0) && !d.holds(TILE, TILE));
        assert!(d.holds(0, 0) && d.holds(TILE, 2 * TILE) && d.holds(2 * TILE, 0));
    }

    #[test]
    fn marked_tiles_come_as_rectangles_that_make_up_all_of_them() {
        let mut d = Dirty::default();
        d.mark_area(Clip::new(0, 0, 2 * TILE - 1, TILE - 1));
        d.mark_area(Clip::new(0, TILE, 2 * TILE - 1, 2 * TILE - 1));
        d.mark_area(Clip::new(3 * TILE, 0, 3 * TILE, 0));
        d.mark_area(Clip::new(
            GRP_SIZE - 1,
            GRP_SIZE - 1,
            GRP_SIZE - 1,
            GRP_SIZE - 1,
        ));
        let mut areas = Vec::new();
        d.for_each_area(|a| areas.push(a));
        assert_eq!(
            areas,
            [
                Clip::new(0, 0, 2 * TILE - 1, 2 * TILE - 1),
                Clip::new(3 * TILE, 0, 4 * TILE - 1, TILE - 1),
                Clip::new(GRP_SIZE - TILE, GRP_SIZE - TILE, GRP_SIZE - 1, GRP_SIZE - 1),
            ]
        );
        let mut all = Vec::new();
        Dirty::ALL.for_each_area(|a| all.push(a));
        assert_eq!(all, [Clip::PAGE]);
        let mut none = 0;
        Dirty::default().for_each_area(|_| none += 1);
        assert_eq!(none, 0);
    }

    #[test]
    fn changes_are_counted_per_page() {
        let mut g = Graphics::default();
        let before = (g.changes(0), g.changes(1));
        g.pset(5, 5, RED, Blend::Normal);
        assert_ne!(g.changes(0), before.0);
        assert_eq!(g.changes(1), before.1);
        let drawn = g.changes(0);
        g.reset_state();
        assert_eq!(g.changes(0), drawn);
        g.clear_page(0);
        assert_ne!(g.changes(0), drawn);
        // Another set of pages never counts as this one did.
        assert_ne!(Graphics::default().changes(1), before.1);
    }

    #[test]
    fn lines_boxes_and_fills() {
        let mut g = Graphics::default();
        g.line(0, 0, 4, 2, RED, Blend::Normal);
        assert_eq!(area(&g, 5, 3), ["##...", "..##.", "....#"]);
        let mut g = Graphics::default();
        g.rect(3, 2, 0, 0, RED, Blend::Normal);
        assert_eq!(area(&g, 5, 3), ["####.", "#..#.", "####."]);
        let mut g = Graphics::default();
        g.fill(1, 1, 2, 2, RED, Blend::Normal);
        assert_eq!(area(&g, 4, 4), ["....", ".##.", ".##.", "...."]);
    }

    /// Lines read back on hardware: for each delta, the minor offset at
    /// each step along the major axis, away from the start.
    #[test]
    fn lines_match_hardware() {
        let read: [((i32, i32), &[i32]); 16] = [
            ((4, 2), &[0, 0, 1, 1, 2]),
            ((2, 4), &[0, 0, 1, 1, 2]),
            ((4, 1), &[0, 1, 1, 1, 1]),
            ((1, 4), &[0, 1, 1, 1, 1]),
            ((5, 2), &[0, 1, 1, 2, 2, 2]),
            ((2, 5), &[0, 1, 1, 2, 2, 2]),
            ((5, 3), &[0, 0, 1, 1, 2, 3]),
            ((3, 5), &[0, 0, 1, 1, 2, 3]),
            ((-4, 2), &[0, 1, 1, 2, 2]),
            ((4, -2), &[0, 0, -1, -1, -2]),
            ((-4, -2), &[0, -1, -1, -2, -2]),
            ((-2, 4), &[0, 0, -1, -1, -2]),
            ((2, -4), &[0, 1, 1, 2, 2]),
            ((-2, -4), &[0, -1, -1, -2, -2]),
            ((6, 4), &[0, 0, 1, 2, 2, 3, 4]),
            ((3, 3), &[0, 1, 2, 3]),
        ];
        for ((dx, dy), minors) in read {
            let mut points = line_points(50, 50, 50 + dx, 50 + dy, Clip::PAGE);
            points.sort_unstable();
            let mut expected: Vec<(i32, i32)> = minors
                .iter()
                .enumerate()
                .map(|(t, &u)| {
                    let t = t as i32;
                    if dx.abs() >= dy.abs() {
                        (50 + t * dx.signum(), 50 + u)
                    } else {
                        (50 + u, 50 + t * dy.signum())
                    }
                })
                .collect();
            expected.sort_unstable();
            assert_eq!(points, expected, "line {dx},{dy}");
        }
    }

    /// TEST48 parts 1 and 2: shallow lines from (50,50), for each delta
    /// the offset across at each step away from the start; the same in
    /// all four directions, either end first, and with the axes swapped.
    #[test]
    fn shallow_lines_match_hardware() {
        let read = [
            ((3, 1), "0111"),
            ((7, 3), "01122333"),
            ((8, 2), "011112222"),
            ((8, 3), "011222333"),
            ((9, 4), "0112233444"),
            ((10, 3), "01112222333"),
            ((10, 4), "01122233444"),
            ((11, 5), "011223344555"),
            ((12, 5), "0112233344555"),
            ((13, 5), "01122233344555"),
            ((6, 1), "0011111"),
            ((7, 2), "01111222"),
        ];
        for ((major, minor), offsets) in read {
            for (sx, sy, swap, reverse) in [
                (1, 1, false, false),
                (1, -1, false, true),
                (-1, 1, true, false),
                (1, 1, true, true),
            ] {
                let (dx, dy) = if swap {
                    (minor * sx, major * sy)
                } else {
                    (major * sx, minor * sy)
                };
                let mut points = if reverse {
                    line_points(50 + dx, 50 + dy, 50, 50, Clip::PAGE)
                } else {
                    line_points(50, 50, 50 + dx, 50 + dy, Clip::PAGE)
                };
                points.sort_unstable();
                let mut expected: Vec<(i32, i32)> = offsets
                    .bytes()
                    .enumerate()
                    .map(|(t, c)| {
                        let (t, u) = (t as i32, i32::from(c - b'0'));
                        // Along the major axis away from the start, across
                        // towards the end.
                        let (along, across) = if swap { (sy, sx) } else { (sx, sy) };
                        let (a, b) = (50 + t * along, 50 + u * across);
                        if swap { (b, a) } else { (a, b) }
                    })
                    .collect();
                expected.sort_unstable();
                assert_eq!(points, expected, "{dx},{dy}");
            }
        }
    }

    /// TEST48 part 3: lines whose first end is just off the page take the
    /// line itself rounded, counted from that end; offsets from the start
    /// row (or column) along X (or Y) 0 to 30.
    #[test]
    fn lines_from_off_the_page_match_hardware() {
        let read = [
            ((-1, 5, 30, 10), "0001111112222223333334444445555"),
            ((-2, 5, 30, 10), "0011111122222233333334444445555"),
            ((-3, 5, 30, 10), "0111111222222233333334444445555"),
            ((-1, 5, 30, 12), "0011112222233334444555556666777"),
            ((-3, 0, 30, 5), "0111111222222233333334444445555"),
            ((5, -1, 10, 30), "0001111112222223333334444445555"),
            ((5, -2, 10, 30), "0011111122222233333334444445555"),
            ((5, -3, 10, 30), "0111111222222233333334444445555"),
        ];
        for ((x0, y0, x1, y1), offsets) in read {
            let mut expected: Vec<(i32, i32)> = offsets
                .bytes()
                .enumerate()
                .map(|(t, c)| {
                    let (t, u) = (t as i32, i32::from(c - b'0'));
                    if y0 < 0 { (x0 + u, t) } else { (t, y0 + u) }
                })
                .collect();
            expected.sort_unstable();
            for (a, b) in [((x0, y0), (x1, y1)), ((x1, y1), (x0, y0))] {
                let mut points = line_points(a.0, a.1, b.0, b.1, Clip::PAGE);
                points.sort_unstable();
                assert_eq!(points, expected, "{a:?} to {b:?}");
            }
        }
    }

    #[test]
    fn huge_coordinates_finish() {
        let mut g = Graphics::default();
        g.line(i32::MIN, 0, i32::MAX, 0, RED, Blend::Normal);
        assert_eq!((g.pixel(0, 0, 0), g.pixel(0, 2047, 0)), (RED, RED));
        g.fill(i32::MIN, 1, i32::MAX, 1, RED, Blend::Normal);
        g.circle((0, 0, i32::MAX), None, RED, Blend::Normal);
        g.triangle(
            [(i32::MIN, 0), (i32::MAX, 0), (0, i32::MAX)],
            RED,
            Blend::Normal,
        );
        assert_eq!(g.pixel(0, 1000, 1000), RED);
    }

    #[test]
    fn clipping_limits_drawing() {
        let mut g = Graphics {
            clip: Clip::new(2, 2, 1, 1),
            ..Graphics::default()
        };
        g.cls(RED);
        assert_eq!(area(&g, 4, 4), ["....", ".##.", ".##.", "...."]);
    }

    #[test]
    fn circles_and_arcs() {
        let mut g = Graphics::default();
        g.circle((3, 3, 2), None, RED, Blend::Normal);
        assert_eq!(
            area(&g, 7, 7),
            [
                ".......", "..###..", ".#...#.", ".#...#.", ".#...#.", "..###..", "......."
            ]
        );
        // From 3 o'clock clockwise to 6 o'clock: the lower right quarter.
        let mut g = Graphics::default();
        g.circle((3, 3, 2), Some((0, 90, false)), RED, Blend::Normal);
        assert_eq!(
            &area(&g, 7, 7)[3..],
            [".....#.", ".....#.", "...##..", "......."]
        );
        // From an angle to the same one, nothing.
        let mut g = Graphics::default();
        g.circle((3, 3, 2), Some((90, 90, false)), RED, Blend::Normal);
        assert_eq!(g.pixel(0, 3, 5), 0);
        // As a sector, with lines to the center.
        let mut g = Graphics::default();
        g.circle((3, 3, 2), Some((0, 90, true)), RED, Blend::Normal);
        assert_eq!(
            &area(&g, 7, 7)[3..],
            ["...###.", "...#.#.", "...##..", "......."]
        );
    }

    /// TEST56: what counts is GCLIP's rectangle: a start outside it takes
    /// `k = major`, one inside the usual rule.
    #[test]
    fn lines_starting_outside_gclip_match_hardware() {
        let steps = |x0, y0, x1, y1| {
            let mut g = Graphics {
                clip: Clip::new(100, 100, 200, 160),
                ..Graphics::default()
            };
            g.line(x0, y0, x1, y1, RED, Blend::Normal);
            let (mut out, mut last) = (Vec::new(), -1);
            for x in 100..=200 {
                let hit = (100..=160).rev().find(|&y| g.read(g.target, x, y) != 0);
                if let Some(y) = hit.filter(|&y| y != last) {
                    out.push((x, y));
                    last = y;
                }
            }
            out
        };
        let l0 = [(100, 115), (105, 116), (115, 117), (125, 118)];
        assert_eq!(steps(50, 110, 250, 130)[..4], l0);
        assert_eq!(steps(250, 130, 50, 110)[..4], l0);
        assert_eq!(
            steps(20, 99, 300, 140)[..4],
            [(100, 111), (106, 112), (113, 113), (120, 114)]
        );
        // Starting inside.
        assert_eq!(
            steps(120, 120, 400, 141)[..4],
            [(120, 120), (126, 121), (139, 122), (153, 123)]
        );
    }

    /// TEST56 part 3: rows read back on hardware, corners off the page
    /// not clamped.
    #[test]
    fn triangles_past_the_page_match_hardware() {
        let rows = |p: [(i32, i32); 3], want: &[(i32, i32, i32)]| {
            let mut g = Graphics {
                target: 1,
                ..Graphics::default()
            };
            g.triangle(p, RED, Blend::Normal);
            for &(y, l, r) in want {
                let row: Vec<i32> = (0..2048).filter(|&x| g.read(1, x, y) != 0).collect();
                assert_eq!((row[0], row[row.len() - 1]), (l, r), "{p:?} row {y}");
            }
        };
        rows(
            [(-20, 10), (60, -30), (100, 80)],
            &[
                (0, 0, 71),
                (22, 0, 79),
                (23, 1, 79),
                (24, 3, 80),
                (80, 99, 100),
            ],
        );
        rows(
            [(0, 10), (60, 0), (100, 80)],
            &[
                (0, 58, 60),
                (10, 0, 65),
                (11, 0, 66),
                (12, 2, 66),
                (80, 99, 100),
            ],
        );
        rows(
            [(2040, 100), (2100, 140), (1990, 180)],
            &[
                (100, 2040, 2040),
                (101, 2039, 2041),
                (102, 2039, 2042),
                (104, 2037, 2045),
            ],
        );
        rows(
            [(10, 2040), (60, 2047), (0, 2047)],
            &[(2040, 10, 12), (2041, 9, 19), (2043, 6, 34), (2047, 1, 60)],
        );
        rows(
            [(-40, -40), (90, -10), (-10, 60)],
            &[(0, 0, 77), (1, 0, 75), (54, 0, 0)],
        );
    }

    #[test]
    fn triangles_and_paint() {
        let mut g = Graphics::default();
        g.triangle([(0, 0), (4, 0), (0, 4)], RED, Blend::Normal);
        assert_eq!(
            area(&g, 5, 5),
            ["#####", "####.", "###..", "##...", "#...."]
        );
        let mut g = Graphics::default();
        g.rect(0, 0, 4, 4, RED, Blend::Normal);
        g.paint(2, 2, RED, None);
        assert_eq!(area(&g, 6, 5)[2], "#####.");
        assert_eq!(g.pixel(0, 5, 2), 0, "the outside stays");
        // With a border color, other colors inside are painted over.
        let mut g = Graphics::default();
        g.rect(0, 0, 4, 4, RED, Blend::Normal);
        g.pset(2, 2, 0xFF00_FF00, Blend::Normal);
        g.paint(1, 1, 0xFF00_00FF, Some(RED));
        assert_eq!(g.pixel(0, 2, 2), 0xFF00_00FF);
        assert_eq!(g.pixel(0, 5, 5), 0);
    }

    #[test]
    fn copy_overlaps_and_tints() {
        let mut g = Graphics::default();
        g.fill(0, 0, 1, 0, RED, Blend::Normal);
        g.copy(0, (0, 0, 2, 0), (1, 0), None, Blend::Normal);
        assert_eq!(area(&g, 4, 1), ["###."]);
        g.copy(0, (1, 0, 1, 0), (0, 1), Some(0xFF00_FFFF), Blend::Normal);
        assert_eq!(g.pixel(0, 0, 1), 0xFF00_0000);
    }

    /// Plain fills give what pages with GARRAY arrays give, clipped and at
    /// the page's edges.
    #[test]
    fn plain_fills_match_fills_with_arrays() {
        let draw = |arrays: bool| {
            let mut g = Graphics::default();
            if arrays {
                g.array(0);
            }
            g.fill(-5, -5, 3, 2, RED, Blend::Normal);
            g.fill(2047, 2040, 2030, 2050, 0x8012_3456, Blend::Normal);
            g.fill(10, 1, 4, 8, 0xFF00_FF00, Blend::Alpha);
            g.clip = Clip::new(5, 3, 9, 6);
            g.fill(0, 0, 20, 20, 0x4000_00FF, Blend::Normal);
            g.fill(10, 0, 20, 20, 0xFFFF_FFFF, Blend::Normal);
            let rows = (0..10).chain(2036..GRP_SIZE);
            (0..GRP_SIZE)
                .flat_map(|x| rows.clone().map(move |y| (x, y)))
                .map(|(x, y)| g.pixel(0, x, y))
                .collect::<Vec<_>>()
        };
        assert_eq!(draw(false), draw(true));
    }

    /// Rows copied straight into a page give what drawing pixel by pixel
    /// gives, which pages with GARRAY arrays still do.
    #[test]
    fn copy_by_rows_matches_pixels() {
        let draw = |arrays: bool| {
            let mut g = Graphics::default();
            if arrays {
                g.array(0);
                g.array(1);
            }
            for (i, color) in [RED, 0x8000_FF00, 0, 0xFF12_3456].into_iter().enumerate() {
                let i = i as i32;
                g.fill(i * 3, i, i * 3 + 5, i + 4, color, Blend::Normal);
            }
            g.target = 1;
            g.copy(0, (0, 0, 15, 8), (2045, 3), None, Blend::Normal);
            g.copy(0, (15, 8, 0, 0), (-4, -2), Some(0xFF80_80FF), Blend::Alpha);
            g.clip = Clip::new(3, 3, 9, 7);
            g.copy(0, (0, 0, 15, 8), (1, 1), None, Blend::Normal2);
            g.copy(2, (0, 0, 15, 8), (0, 5), None, Blend::Add);
            g.clip = Clip::PAGE;
            // The last rows of the page, as source and as target.
            g.fill(2040, 2044, 2047, 2047, 0xFF65_4321, Blend::Normal);
            g.copy(
                0,
                (2036, 2040, 2047, 2047),
                (7, 2),
                Some(0x80FF_FFFF),
                Blend::Normal2,
            );
            g.copy(0, (0, 0, 15, 8), (2040, 2043), None, Blend::Alpha2);
            g.target = 0;
            g.copy(0, (0, 0, 12, 6), (2, 1), None, Blend::Normal);
            g.copy(
                1,
                (2040, 2040, 2047, 2047),
                (100, 2045),
                None,
                Blend::Normal,
            );
            let rows = (0..12).chain(2036..GRP_SIZE);
            (0..2)
                .flat_map(|page| (0..GRP_SIZE).map(move |x| (page, x)))
                .flat_map(|(page, x)| rows.clone().map(move |y| (page, x, y)))
                .map(|(page, x, y)| g.pixel(page, x, y))
                .collect::<Vec<_>>()
        };
        assert_eq!(draw(false), draw(true));
    }

    /// The rows `y0..y0+h` of columns `0..w`, as on hardware's readback:
    /// a pixel's red nibble, or `.` where it is 0.
    fn nibbles(g: &Graphics, y0: i32, w: i32, h: i32) -> Vec<String> {
        (y0..y0 + h)
            .map(|y| {
                (0..w)
                    .map(|x| match g.pixel(0, x, y) >> 20 & 15 {
                        0 => '.',
                        n => char::from_digit(n, 16).unwrap().to_ascii_uppercase(),
                    })
                    .collect()
            })
            .collect()
    }

    /// The circle walked row by row, as the midpoint algorithm does.
    fn midpoint(r: i32) -> Vec<(i32, i32)> {
        let mut points = Vec::new();
        let (mut x, mut y, mut err) = (r, 0, 1 - r);
        while x >= y {
            for p in [
                (x, y),
                (y, x),
                (-y, x),
                (-x, y),
                (-x, -y),
                (-y, -x),
                (y, -x),
                (x, -y),
            ] {
                points.push(p);
            }
            y += 1;
            if err < 0 {
                err += 2 * y + 1;
            } else {
                x -= 1;
                err += 2 * (y - x) + 1;
            }
        }
        points.sort_unstable();
        points.dedup();
        points
    }

    /// [`circle_points`] in order, repeats kept, to compare as a set that
    /// must have none.
    fn sorted_circle_points(cx: i32, cy: i32, r: i32) -> Vec<(i32, i32)> {
        let mut points = circle_points(cx, cy, r);
        points.sort_unstable();
        points
    }

    #[test]
    fn circle_points_are_the_midpoint_circle() {
        for r in 0..600 {
            let mut expected: Vec<(i32, i32)> = midpoint(r)
                .into_iter()
                .map(|(x, y)| (x + 1000, y + 900))
                .collect();
            expected.sort_unstable();
            assert_eq!(sorted_circle_points(1000, 900, r), expected, "radius {r}");
        }
        // Off the edges, only what lands on the page.
        let mut expected: Vec<(i32, i32)> = midpoint(300)
            .into_iter()
            .map(|(x, y)| (x + 100, y - 50))
            .filter(|&(x, y)| x >= 0 && y >= 0)
            .collect();
        expected.sort_unstable();
        assert_eq!(sorted_circle_points(100, -50, 300), expected);
    }

    /// TEST47 part 17: circles of radius 1000 and 70000 whose right edge
    /// is at X 10 show it there, on rows 1 to 9 at least.
    #[test]
    fn huge_circles_are_drawn() {
        for (cx, r) in [(-990, 1000), (-69990, 70000)] {
            let mut g = Graphics::default();
            g.circle((cx, 5, r), None, RED, Blend::Normal);
            assert_eq!(&area(&g, 12, 10)[1..], ["..........#."; 9], "radius {r}");
        }
        let mut g = Graphics::default();
        g.circle(
            (-69990, 5, 70000),
            Some((170, 190, true)),
            RED,
            Blend::Normal,
        );
        assert!(area(&g, 40, 20).iter().all(|r| !r.contains('#')));
    }

    /// TEST47 parts 4 and 5 as read back on hardware: an arc from 30 to
    /// 120 degrees includes the pixel at 29.7 degrees, the one nearest 30,
    /// and sector sides follow the line rule.
    #[test]
    fn arcs_and_sectors_match_hardware() {
        let mut g = Graphics::default();
        g.circle((9, 9, 8), Some((30, 120, true)), RED, Blend::Normal);
        g.circle((28, 9, 8), Some((30, 120, false)), RED, Blend::Normal);
        assert_eq!(
            &area(&g, 38, 19)[9..],
            [
                ".........##...........................",
                ".........#.##.........................",
                "........#....##.......................",
                "........#......#......................",
                ".......#........#..................#..",
                ".......#.......#..................#...",
                "......#.......#..................#....",
                ".....##.....##..........##.....##.....",
                ".......#####..............#####.......",
                "......................................",
            ]
        );
        let mut g = Graphics::default();
        g.circle((9, 9, 8), Some((-90, 0, false)), RED, Blend::Normal);
        g.circle((28, 9, 8), Some((400, 500, true)), RED, Blend::Normal);
        assert_eq!(
            area(&g, 38, 19),
            [
                "......................................",
                ".........###..........................",
                "............##........................",
                "..............#.......................",
                "...............#......................",
                "................#.....................",
                "................#.....................",
                ".................#....................",
                ".................#....................",
                ".................#..........#.........",
                "...........................#.#........",
                "..........................#...##......",
                "........................##......#.....",
                ".......................#.........#....",
                "......................#...........#...",
                ".......................#.........#....",
                "........................##.....##.....",
                "..........................#####.......",
                "......................................",
            ]
        );
        // Equal angles draw nothing, sector or not; 45 to 46 is one side.
        let count = |g: &Graphics| {
            (0..21)
                .flat_map(|y| (0..21).map(move |x| (x, y)))
                .filter(|&(x, y)| g.pixel(0, x, y) != 0)
                .count()
        };
        let mut g = Graphics::default();
        g.circle((10, 10, 8), Some((45, 45, true)), RED, Blend::Normal);
        assert_eq!(count(&g), 0);
        g.circle((10, 10, 8), Some((45, 46, true)), RED, Blend::Normal);
        assert_eq!(count(&g), 7);
    }

    /// TEST47 part 16: a shallow line from its end, and the same line
    /// running far off the page both ways.
    #[test]
    fn far_lines_match_hardware() {
        let runs = |g: &Graphics| -> Vec<(i32, i32, i32)> {
            (24..28)
                .filter_map(|y| {
                    let xs: Vec<i32> = (0..40).filter(|&x| g.pixel(0, x, y) != 0).collect();
                    Some((y, *xs.first()?, *xs.last()?))
                })
                .collect()
        };
        let mut g = Graphics::default();
        g.line(0, 25, 40, 26, RED, Blend::Normal);
        assert_eq!(runs(&g), [(25, 0, 18), (26, 19, 39)]);
        for (x0, y0, x1, y1) in [
            (-1000, 0, 1040, 51),
            (1040, 51, -1000, 0),
            (-100000, -2475, 104000, 2625),
        ] {
            let mut g = Graphics::default();
            g.line(x0, y0, x1, y1, RED, Blend::Normal);
            assert_eq!(runs(&g), [(25, 0, 19), (26, 20, 39)], "{x0},{y0}");
        }
        let mut g = Graphics::default();
        g.line(-100000, 3, 100000, 5, RED, Blend::Normal);
        assert!((0..40).all(|x| g.pixel(0, x, 4) == RED && g.pixel(0, x, 3) == 0));
    }

    #[test]
    fn edge_spans_agree_in_i64_and_i128() {
        let mut seed = 0x2545_f491_u64;
        let mut next = |range: i32| {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            (seed % (2 * range as u64 + 1)) as i32 - range
        };
        for range in [8, 300, 5000, (1 << 28) - 1] {
            for _ in 0..20000 {
                let (a, b) = ((next(range), next(range)), (next(range), next(range)));
                let y = next(range);
                assert_eq!(
                    edge_span_in::<i64>(a, b, y),
                    edge_span_in::<i128>(a, b, y),
                    "{a:?} {b:?} {y}"
                );
            }
        }
    }

    /// TEST47 parts 6 and 7: triangles read back on hardware, each pixel
    /// drawn once (with `#G_ALPHA` over black, the red nibble is 8).
    #[test]
    fn triangles_match_hardware() {
        let mut g = Graphics::default();
        g.fill(0, 0, 25, 15, 0xFF00_0000, Blend::Normal);
        for p in [
            [(0, 0), (12, 3), (4, 11)],
            [(15, 0), (20, 5), (25, 10)],
            [(0, 13), (20, 14), (10, 15)],
        ] {
            g.triangle(p, 0x80FF_FFFF, Blend::Alpha);
        }
        assert_eq!(
            nibbles(&g, 0, 26, 16),
            [
                "88.............8..........",
                "888888..........8.........",
                ".888888888.......8........",
                ".888888888888.....8.......",
                ".88888888888.......8......",
                "..888888888.........8.....",
                "..88888888...........8....",
                "...888888.............8...",
                "...88888...............8..",
                "...8888.................8.",
                "....88...................8",
                "....8.....................",
                "..........................",
                "8888888888................",
                "..8888888888888888888.....",
                ".......888888888..........",
            ]
        );
    }

    /// TEST48 part 4: each row's run from left to right, the same
    /// whichever order the corners come in.
    #[test]
    fn triangle_rows_match_hardware() {
        let read: [([(i32, i32); 3], &str); 4] = [
            ([(0, 0), (1, 2), (10, 1)], "0-4 1-10 1-6"),
            ([(10, 0), (9, 2), (0, 1)], "6-10 0-9 4-9"),
            ([(0, 0), (3, 6), (20, 3)], "0-2 1-9 1-15 2-20 2-18 3-12 3-6"),
            (
                [(20, 0), (17, 6), (0, 3)],
                "18-20 11-19 5-19 0-18 2-18 8-17 14-17",
            ),
        ];
        for (p, rows) in read {
            for p in [p, [p[2], p[1], p[0]], [p[1], p[0], p[2]]] {
                let mut g = Graphics::default();
                g.triangle(p, RED, Blend::Normal);
                let runs: Vec<String> = (0..7)
                    .filter_map(|y| {
                        let xs: Vec<i32> = (0..31).filter(|&x| g.pixel(0, x, y) != 0).collect();
                        let (l, r) = (*xs.first()?, *xs.last()?);
                        assert_eq!(xs.len() as i32, r - l + 1, "{p:?} row {y}");
                        Some(format!("{l}-{r}"))
                    })
                    .collect();
                assert_eq!(runs.join(" "), rows, "{p:?}");
            }
        }
    }

    /// TEST47 part 8: GBOX with `#G_ALPHA` blends its top corners twice.
    #[test]
    fn boxes_match_hardware() {
        let mut g = Graphics::default();
        g.fill(0, 0, 20, 13, 0xFF00_0000, Blend::Normal);
        for (x0, y0, x1, y1) in [
            (2, 2, 10, 8),
            (10, 11, 2, 11),
            (13, 1, 13, 9),
            (16, 3, 16, 3),
        ] {
            g.rect(x0, y0, x1, y1, 0x80FF_0000, Blend::Alpha);
        }
        assert_eq!(
            nibbles(&g, 0, 21, 13),
            [
                ".....................",
                ".............B.......",
                "..B8888888B..8.......",
                "..8.......8..8..8....",
                "..8.......8..8.......",
                "..8.......8..8.......",
                "..8.......8..8.......",
                "..8.......8..8.......",
                "..888888888..8.......",
                ".............8.......",
                ".....................",
                "..888888888..........",
                ".....................",
            ]
        );
        assert_eq!(
            (g.pixel(0, 2, 2), g.pixel(0, 6, 2)),
            (0xFFBF_0000, 0xFF80_0000)
        );
    }

    /// TEST47 part 11: `#G_ALPHA2` of &HF09030 at alphas 1, &H80 and &HFE
    /// over &H2060A0 at alphas 0, &H40, &H80 and &HFF, read back.
    #[test]
    fn alpha2_matches_hardware() {
        let read = [
            [0x01F0_9030, 0x80F0_9030, 0xFEF0_9030],
            [0x4122_5F9D, 0xA0C6_8645, 0xFFEF_8F2F],
            [0x8120_5F9E, 0xC0AA_7F54, 0xFFEF_8F2F],
            [0xFF20_609F, 0xFF88_7867, 0xFFEF_8F30],
        ];
        for (da, row) in [0u32, 0x40, 0x80, 0xFF].into_iter().zip(read) {
            for (sa, expected) in [1u32, 0x80, 0xFE].into_iter().zip(row) {
                let (dst, src) = (da << 24 | 0x2060A0, sa << 24 | 0xF09030);
                assert_eq!(Blend::Alpha2.apply(dst, src), expected, "{da:X} {sa:X}");
            }
        }
    }

    /// The row-at-a-time GLOAD against drawing pixel by pixel, which a page
    /// with a GARRAY array still does.
    #[test]
    fn load_by_rows_draws_as_plotting_does() {
        let image: Vec<u32> = (0..600u32)
            .map(|i| match i % 5 {
                0 => 0,
                1 => 0xFFFF_FFFF,
                _ => i.wrapping_mul(0x0101_0705) | 0x4000_0000,
            })
            .collect();
        let rects = [
            (2, 3, 20, 10),
            (-5, -4, 20, 10),
            (2040, 2040, 20, 10),
            (40, 30, 20, 10),
            (10, 10, 0, 5),
            (10, 10, 5, 0),
            (i32::MAX - 1, 0, 3, 3),
            (0, i32::MIN, 3, 3),
        ];
        let blends = [
            Blend::Normal,
            Blend::Normal2,
            Blend::Alpha,
            Blend::Alpha2,
            Blend::Add,
        ];
        let clips = [
            Clip::PAGE,
            Clip::new(5, 5, 15, 9),
            Clip::new(30, 30, 30, 30),
        ];
        for (rect, blend, tint, clip, len) in rects.into_iter().flat_map(|r| {
            blends.into_iter().flat_map(move |b| {
                [None, Some(0x80FF_8040), Some(0xFFFF_FFFF)]
                    .into_iter()
                    .flat_map(move |t| {
                        clips.into_iter().flat_map(move |c| {
                            // All of it, or running out inside a row.
                            [600, 57, 0].into_iter().map(move |n| (r, b, t, c, n))
                        })
                    })
            })
        }) {
            let draw = |g: &mut Graphics| {
                g.fill(0, 0, 60, 60, 0x8020_60A0, Blend::Normal);
                g.clip = clip;
            };
            let mut slow = Graphics::default();
            draw(&mut slow);
            slow.array(0);
            slow.load_rect(rect, &image[..len], None, tint, blend);
            let mut fast = Graphics::default();
            draw(&mut fast);
            fast.load_rect(rect, &image[..len], None, tint, blend);
            let ints: Vec<i32> = image[..len].iter().map(|&p| p as i32).collect();
            let mut direct = Graphics::default();
            draw(&mut direct);
            direct.load_ints(rect, &ints, tint, blend);
            let case = format!("{rect:?} {blend:?} {tint:?} {clip:?} {len}");
            assert!(slow.page(0) == fast.page(0), "{case}");
            assert!(slow.page(0) == direct.page(0), "ints {case}");
        }
    }

    #[test]
    fn load_landing_nowhere_leaves_a_blank_page_blank() {
        let mut g = Graphics {
            clip: Clip::new(100, 100, 110, 110),
            ..Default::default()
        };
        g.load_rect((0, 0, 2, 2), &[!0; 4], None, None, Blend::Normal);
        g.load_rect((100, 100, 2, 2), &[], None, None, Blend::Normal);
        g.load_rect((100, 100, 0, 2), &[!0; 4], None, None, Blend::Normal);
        assert!(g.is_blank(0));
        let changes = g.changes(0);
        g.load_rect((100, 100, 2, 2), &[!0; 4], None, None, Blend::Normal);
        assert!(!g.is_blank(0));
        assert_ne!(g.changes(0), changes);
    }

    /// TEST47 part 15: palette indexes past the palette leave the page.
    #[test]
    fn load_skips_indexes_past_the_palette() {
        let mut g = Graphics::default();
        g.fill(0, 0, 5, 0, 0xFFFF_FFFF, Blend::Normal);
        let palette = [0xFFFF_0000, 0xFF00_FF00, 0xFF00_00FF];
        g.load_rect(
            (0, 0, 6, 1),
            &[0, 1, 2, 3, 4, 100],
            Some(&palette),
            None,
            Blend::Normal,
        );
        let row: Vec<u32> = (0..6).map(|x| g.pixel(0, x, 0)).collect();
        assert_eq!(row, [0xFFFF_0000, 0xFF00_FF00, 0xFF00_00FF, !0, !0, !0]);
    }

    #[test]
    fn blend_modes() {
        let half_red = 0x80FF_0000;
        let blue = 0xFF00_00FF;
        assert_eq!(Blend::Normal.apply(blue, 0), 0);
        assert_eq!(Blend::Normal2.apply(blue, 0), blue);
        assert_eq!(Blend::Alpha.apply(blue, half_red), 0xFF80_007F);
        // Over a transparent pixel, mode 3 keeps the color as it is.
        assert_eq!(Blend::Alpha2.apply(0, half_red), half_red);
        // A clear pixel over a clear one changes nothing, not even its color.
        assert_eq!(Blend::Alpha2.apply(0x00FF_4020, 0), 0x00FF_4020);
        assert_eq!(Blend::Alpha2.apply(0x00FF_4020, 0x00C0_0000), 0x00FF_4020);
        assert_eq!(Blend::Add.apply(0xFF80_0000, half_red), 0xFFFF_0000);
        assert_eq!(Blend::from_int(5), None);
        // Read back on hardware: half-transparent red drawn with modes 2,
        // 3 and 4 over transparent, opaque blue and half-transparent blue.
        let read = [
            0x0080_0000,
            0x80FF_0000,
            0x0080_0000,
            0xFF80_007F,
            0xFF80_007F,
            0xFF80_00FF,
            0x8080_007F,
            0xC0AA_0054,
            0x8080_00FF,
        ];
        for (i, expected) in read.into_iter().enumerate() {
            let dst = [0, 0xFF00_00FF, 0x8000_00FF][i / 3];
            let mode = Blend::from_int(2 + (i % 3) as i32).unwrap();
            assert_eq!(mode.apply(dst, half_red), expected, "case {i}");
        }
    }

    /// GPUTCHR as it was before it took the clipping rectangle into
    /// account, adding as a release build did.
    #[allow(clippy::too_many_arguments)]
    fn put_chars_unclipped(
        g: &mut Graphics,
        font: &Font,
        user: UserPage,
        (x, y): (i32, i32),
        text: &[u16],
        px: u32,
        (sx, sy): (i32, i32),
        color: u32,
        blend: Blend,
    ) {
        let n = px as i32;
        for (i, &code) in text.iter().enumerate() {
            let left = x.saturating_add((i as i32).saturating_mul(n * sx));
            if draws_nothing(user, code) {
                continue;
            }
            for gy in 0..n {
                for gx in 0..n {
                    let Some((p, blend)) = char_plot(font, g, user, code, px, gx, gy, color, blend)
                    else {
                        continue;
                    };
                    for py in 0..sy {
                        for px in 0..sx {
                            let at = left.wrapping_add(gx * sx + px);
                            g.plot(at, y.wrapping_add(gy * sy + py), p, blend);
                        }
                    }
                }
            }
        }
    }

    /// GPUTCHRP the same way.
    #[allow(clippy::too_many_arguments)]
    fn put_chars_proportional_unclipped(
        g: &mut Graphics,
        font: &Font,
        user: UserPage,
        (x, y): (i32, i32),
        text: &[u16],
        size: FontSize,
        (sx, sy): (i32, i32),
        color: u32,
        blend: Blend,
        spacing: i32,
    ) {
        let mut left = x;
        let n = size as i32;
        for &code in text {
            let Span { first, width } = span(font, code, size);
            if !draws_nothing(user, code) {
                for gy in 0..n {
                    for gx in 0..n {
                        let Some((p, blend)) =
                            char_plot(font, g, user, code, n as u32, gx, gy, color, blend)
                        else {
                            continue;
                        };
                        for py in 0..sy {
                            for px in 0..sx {
                                let at = left.saturating_add((gx - first) * sx + px);
                                g.plot(at, y.wrapping_add(gy * sy + py), p, blend);
                            }
                        }
                    }
                }
            }
            left = left.saturating_add((width + spacing) * sx);
        }
    }

    /// After TPAGE -1, user-defined characters draw nothing.
    fn draws_nothing(user: UserPage, code: u16) -> bool {
        USER_CHARS.contains(&code) && user.page < 0
    }

    /// A pixel of a character's glyph, one at a time: user-defined ones
    /// from the TPAGE page as text screens read it, but none past the
    /// page's edge under GSAMPLE 0.
    fn char_pixel(
        font: &Font,
        g: &Graphics,
        user: UserPage,
        code: u16,
        px: u32,
        x: i32,
        y: i32,
    ) -> Option<u32> {
        if USER_CHARS.contains(&code) {
            let (cx, cy) = user.cell(code, px as i32);
            let (x, y) = (cx + x, cy + y);
            let page = usize::try_from(user.page).ok().filter(|&p| p < PAGES);
            if page.is_some_and(|p| g.sample[p] == 0) && !Clip::PAGE.contains(x, y) {
                return None;
            }
            Some(crate::sprites::sample(g, user.page, x, y))
        } else {
            Some(glyph_pixel_px(font, g, code, px, x as u32, y as u32))
        }
    }

    /// What a glyph pixel plots, and how: mode 1 skips only the glyph's
    /// own clear pixels, and writes the rest whatever alpha the color
    /// gives them.
    #[allow(clippy::too_many_arguments)]
    fn char_plot(
        font: &Font,
        g: &Graphics,
        user: UserPage,
        code: u16,
        px: u32,
        x: i32,
        y: i32,
        color: u32,
        blend: Blend,
    ) -> Option<(u32, Blend)> {
        let glyph = char_pixel(font, g, user, code, px, x, y)?;
        match blend {
            Blend::Normal2 if glyph >> 24 == 0 => None,
            Blend::Normal2 => Some((multiply(glyph, color), Blend::Normal)),
            blend => Some((multiply(glyph, color), blend)),
        }
    }

    fn chars_rng() -> impl FnMut(u64) -> u64 {
        let mut seed = 0x1234_5678_9ABC_DEF1u64;
        move |n| {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed % n
        }
    }

    /// Something for the blends to mix with, with the font on its page.
    fn chars_pages(font: &Font) -> (Graphics, Vec<u32>) {
        let mut g = Graphics::default();
        let base: Vec<u32> = (0..GRP_SIZE * GRP_SIZE)
            .map(|i| (i as u32).wrapping_mul(0x9E37_79B1))
            .collect();
        g.set_page(FONT_PAGE, Page::from_rows(font.page_image()));
        (g, base)
    }

    /// Text with blanks and characters the font lacks.
    fn chars_text(next: &mut impl FnMut(u64) -> u64) -> Vec<u16> {
        const CODES: [u16; 11] = [
            65, 73, 32, 46, 0x30, 0xE100, 0x3042, 0xFFFF, 0x3000, 0xE800, 0xF7FF,
        ];
        (0..1 + next(5))
            .map(|_| CODES[next(CODES.len() as u64) as usize])
            .collect()
    }

    const CHARS_BLENDS: [Blend; 3] = [Blend::Normal, Blend::Normal2, Blend::Alpha];

    fn chars_clip(next: &mut impl FnMut(u64) -> u64) -> Clip {
        match next(4) {
            0 => Clip::PAGE,
            1 => Clip::new(0, 0, 399, 239),
            _ => {
                let (x0, y0) = (next(GRP_SIZE as u64) as i32, next(GRP_SIZE as u64) as i32);
                Clip::new(x0, y0, x0 + next(70) as i32, y0 + next(70) as i32)
            }
        }
    }

    /// Near the clipping rectangle's edges in both directions, or far.
    fn chars_pos(next: &mut impl FnMut(u64) -> u64, low: i32, high: i32, reach: i32) -> i32 {
        let near = |v: i64| v.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32;
        let reach = i64::from(reach);
        match next(12) {
            0 => i32::MAX - next(3) as i32,
            1 => i32::MIN + next(3) as i32,
            2..=5 => near(i64::from(low) - next(2 * reach as u64 + 1) as i64),
            6..=8 => near(i64::from(high) - next(2 * reach as u64 + 1) as i64 + reach / 2),
            _ => low + next((high - low + 1) as u64) as i32,
        }
    }

    #[test]
    fn clipped_chars_match_drawing_every_pixel() {
        let font = Font::fallback();
        let (mut slow, base) = chars_pages(&font);
        let (mut fast, _) = chars_pages(&font);
        let mut next = chars_rng();
        const SCALES: [i32; 8] = [1, 1, 2, 3, 5, 8, 17, 40];
        // Each draw goes over the last, as copying the pages takes longer
        // than any of them.
        // User-defined characters come from a page of their own, as
        // drawing them on the page they come from is no different from
        // drawing glyphs on the font page.
        for g in [&mut slow, &mut fast] {
            g.set_page(0, Page::from_rows(base.clone()));
            g.set_page(
                3,
                Page::from_rows(base.iter().map(|p| p.rotate_left(7)).collect()),
            );
        }
        for case in 0..1500 {
            let clip = chars_clip(&mut next);
            let (sx, sy) = (SCALES[next(8) as usize], SCALES[next(8) as usize]);
            let px = [8, 16][next(2) as usize];
            let n = px as i32;
            let x = chars_pos(&mut next, clip.x0, clip.x1, 2 * n * sx);
            let y = chars_pos(&mut next, clip.y0, clip.y1, n * sy);
            let text = chars_text(&mut next);
            let blend = CHARS_BLENDS[next(3) as usize];
            let color = [0xFFFF_FFFF, 0x80FF_8040, 0x01FF_FFFF][next(3) as usize];
            let size = [FontSize::Small, FontSize::Large][next(2) as usize];
            let spacing = next(65) as i32;
            let target = [0, FONT_PAGE][usize::from(next(64) == 0)];
            let origin = (next(GRP_SIZE as u64) as i32, next(GRP_SIZE as u64) as i32);
            let user = match next(4) {
                0 => UserPage::DEFAULT,
                1 => UserPage { page: -1, origin },
                _ => UserPage { page: 3, origin },
            };
            let wrap = next(2) as i32;
            for g in [&mut slow, &mut fast] {
                g.target = target;
                g.clip = clip;
                g.sample[3] = wrap;
            }
            let what =
                format!("#{case} {clip:?} {x},{y} {sx}x{sy} {px} {text:?} {blend:?} {user:?}");
            put_chars_unclipped(
                &mut slow,
                &font,
                user,
                (x, y),
                &text,
                px,
                (sx, sy),
                color,
                blend,
            );
            fast.put_chars(&font, user, (x, y), &text, px, (sx, sy), color, blend);
            assert!(slow.page(target) == fast.page(target), "GPUTCHR {what}");
            put_chars_proportional_unclipped(
                &mut slow,
                &font,
                user,
                (x, y),
                &text,
                size,
                (sx, sy),
                color,
                blend,
                spacing,
            );
            fast.put_chars_proportional(
                &font,
                user,
                (x, y),
                &text,
                size,
                (sx, sy),
                color,
                blend,
                spacing,
            );
            assert!(
                slow.page(target) == fast.page(target),
                "GPUTCHRP {what} {spacing}"
            );
            // Glyphs drawn over would go on to change the rest.
            if target == FONT_PAGE {
                for g in [&mut slow, &mut fast] {
                    g.set_page(FONT_PAGE, Page::from_rows(font.page_image()));
                }
            }
        }
    }

    /// Long text, also drawn over the glyphs it uses.
    #[test]
    fn long_proportional_text_matches_drawing_every_pixel() {
        let font = Font::fallback();
        let (mut slow, base) = chars_pages(&font);
        let (mut fast, _) = chars_pages(&font);
        let mut next = chars_rng();
        for target in [0, FONT_PAGE] {
            for x in [-3000, -50, 5] {
                let text: Vec<u16> = (0..556)
                    .map(|_| [65, 73, 32, 0xE100, 0x3042, 0x3000, 0xE801][next(7) as usize])
                    .collect();
                for g in [&mut slow, &mut fast] {
                    g.target = target;
                    g.set_page(0, Page::from_rows(base.clone()));
                    g.set_page(FONT_PAGE, Page::from_rows(font.page_image()));
                }
                let (size, scale) = (FontSize::Large, (2, 2));
                let y = 10;
                // Read from the page drawn on, which they change as they go.
                let user = UserPage {
                    page: 0,
                    origin: (100, 0),
                };
                put_chars_proportional_unclipped(
                    &mut slow,
                    &font,
                    user,
                    (x, y),
                    &text,
                    size,
                    scale,
                    0xFFFF_FFFF,
                    Blend::Normal,
                    1,
                );
                fast.put_chars_proportional(
                    &font,
                    user,
                    (x, y),
                    &text,
                    size,
                    scale,
                    0xFFFF_FFFF,
                    Blend::Normal,
                    1,
                );
                assert!(slow.page(target) == fast.page(target), "{target} {x}");
                let widths: i64 = text
                    .iter()
                    .map(|&c| i64::from(span(&font, c, size).width))
                    .sum();
                assert_eq!(
                    i64::from(fast.proportional_width(&font, &text, size, 2, 1)),
                    (widths + text.len() as i64 - 1) * 2
                );
            }
        }
    }

    /// GPUTCHRP's widths are the font's: drawing over GRP5 changes what
    /// it draws, not how wide.
    #[test]
    fn proportional_widths_ignore_the_font_page() {
        for font in [
            Font::fallback(),
            Font::blank_with_metrics(vec![13 << 11 | 6 << 3; 0x1_0000]),
        ] {
            let (mut g, _) = chars_pages(&font);
            let text = [0x41, 0x49, 0x20, 0x3042];
            let before: Vec<i32> = [FontSize::Small, FontSize::Large]
                .map(|size| g.proportional_width(&font, &text, size, 1, 1))
                .into();
            g.target = FONT_PAGE;
            g.cls(0xFFFF_FFFF);
            g.target = 0;
            let after: Vec<i32> = [FontSize::Small, FontSize::Large]
                .map(|size| g.proportional_width(&font, &text, size, 1, 1))
                .into();
            assert_eq!(before, after);
            // What is drawn comes from the page as it is now.
            g.put_chars_proportional(
                &font,
                UserPage::DEFAULT,
                (0, 0),
                &[0x20],
                FontSize::Large,
                (1, 1),
                RED,
                Blend::Normal,
                1,
            );
            assert_eq!(g.pixel(0, 0, 0), RED);
        }
    }

    #[test]
    fn user_characters_under_tpage_minus_one_draw_nothing() {
        let font = Font::fallback();
        let (mut g, base) = chars_pages(&font);
        g.set_page(0, Page::from_rows(base.clone()));
        let user = UserPage {
            page: -1,
            origin: (0, 0),
        };
        let code = *USER_CHARS.start();
        g.put_chars(&font, user, (0, 0), &[code], 16, (1, 1), RED, Blend::Normal);
        let large = FontSize::Large;
        g.put_chars_proportional(
            &font,
            user,
            (0, 0),
            &[code],
            large,
            (1, 1),
            RED,
            Blend::Normal,
            1,
        );
        assert!(g.page(0).to_rows() == base);
        assert_eq!(g.proportional_width(&font, &[code], large, 1, 1), 16);
    }

    /// What `draw` leaves on row `y` of a page filled with red, from x 0
    /// to 39.
    fn row_after(g: &mut Graphics, y: i32, draw: impl FnOnce(&mut Graphics)) -> Vec<u32> {
        g.target = 0;
        g.cls(RED);
        draw(g);
        (0..40).map(|x| g.pixel(0, x, y)).collect()
    }

    /// GPUTCHRP draws the whole cell with column `first` on the pen: in
    /// mode 0 the next character's cell clears the end of the one before.
    #[test]
    fn proportional_cells_start_left_of_the_pen() {
        const WHITE: u32 = 0xFFFF_FFFF;
        // A: columns 0-12 at 16 and 0-5 at 8; I: 3-9 at 16 and 1-4 at 8.
        let mut metrics = vec![0; 0x1_0000];
        metrics[0x41] = 13 << 11 | 6 << 3;
        metrics[0x49] = 7 << 11 | 3 << 7 | 4 << 3 | 1;
        let font = Font::blank_with_metrics(metrics);
        let (mut g, _) = chars_pages(&font);
        // Ink in A's last column at 16, and in I's first ones at 16 and 8.
        let (ax, ay) = font.origin(0x41, FontSize::Large).unwrap();
        let (ix, iy) = font.origin(0x49, FontSize::Large).unwrap();
        let (jx, jy) = font.origin(0x49, FontSize::Small).unwrap();
        g.target = FONT_PAGE;
        g.pset(ax as i32 + 12, ay as i32, WHITE, Blend::Normal);
        g.pset(ix as i32 + 3, iy as i32, WHITE, Blend::Normal);
        g.pset(jx as i32 + 1, jy as i32, WHITE, Blend::Normal);
        let font = &font;
        let draw = |text: &'static [u16], size, x, blend| {
            move |g: &mut Graphics| {
                let user = UserPage::DEFAULT;
                g.put_chars_proportional(font, user, (x, 0), text, size, (1, 1), WHITE, blend, 1)
            }
        };
        let (small, large) = (FontSize::Small, FontSize::Large);
        let at = |row: &[u32], x: usize| row[x] == WHITE;
        // Alone, A keeps its last column; I's first ink lands on the pen,
        // and its cell from 3 columns left of it is cleared.
        let a = row_after(&mut g, 0, draw(&[0x41], large, 0, Blend::Normal));
        assert!(at(&a, 12) && a[16] == RED);
        let i = row_after(&mut g, 0, draw(&[0x49], large, 20, Blend::Normal));
        assert!(at(&i, 20));
        assert_eq!((i[16], i[17], i[32], i[33]), (RED, 0, 0, RED));
        // After A, I's pen is at 13 + 1 and its cell clears A's column 12.
        let ai = row_after(&mut g, 0, draw(&[0x41, 0x49], large, 0, Blend::Normal));
        assert!(at(&ai, 14) && ai[12] == 0 && ai[27] == RED);
        // Mode 1 passes the clear pixels by.
        let ai = row_after(&mut g, 0, draw(&[0x41, 0x49], large, 0, Blend::Normal2));
        assert!(at(&ai, 12) && at(&ai, 14) && ai[13] == RED);
        // At 8 pixels: pen at 6 + 1, I's cell from 6 to 13.
        let ai = row_after(&mut g, 0, draw(&[0x41, 0x49], small, 0, Blend::Normal));
        assert!(at(&ai, 7) && ai[6] == 0 && ai[13] == 0 && ai[14] == RED);
    }

    /// Fonts without font.map's metrics draw GPUTCHRP's cells the same
    /// way, from a span measured on their own glyphs.
    #[test]
    fn proportional_cells_of_the_bundled_font() {
        let font = Font::fallback();
        let (mut g, _) = chars_pages(&font);
        for size in [FontSize::Small, FontSize::Large] {
            // A narrow character with blank columns before its ink.
            let code = (0x21..0x7F)
                .find(|&c| span(&font, c, size).first > 1)
                .expect("a character with blank columns first");
            let Span { first, width } = span(&font, code, size);
            let n = size as i32;
            let row = row_after(&mut g, 0, |g| {
                let user = UserPage::DEFAULT;
                g.put_chars_proportional(
                    &font,
                    user,
                    (20, 0),
                    &[code],
                    size,
                    (1, 1),
                    0xFF00_FF00,
                    Blend::Normal,
                    1,
                )
            });
            let (left, right) = ((20 - first) as usize, (20 - first + n) as usize);
            let what = format!("{code:#x} {size:?}");
            assert_eq!((row[left - 1], row[right]), (RED, RED), "{what}");
            assert!(row[left..right].iter().all(|&p| p != RED), "{what}");
            assert_eq!(g.proportional_width(&font, &[code], size, 1, 1), width);
        }
    }

    /// In mode 1, a color with alpha 0 draws nothing at all.
    #[test]
    fn transparent_color_in_mode_1_draws_nothing() {
        let font = Font::fallback();
        let (mut g, base) = chars_pages(&font);
        g.set_page(0, Page::from_rows(base.clone()));
        let text = [0x41, 0x49, 0x20];
        for color in [0x00FF_FFFF, 0] {
            g.put_chars(
                &font,
                UserPage::DEFAULT,
                (5, 5),
                &text,
                16,
                (1, 1),
                color,
                Blend::Normal2,
            );
            g.put_chars_proportional(
                &font,
                UserPage::DEFAULT,
                (5, 30),
                &text,
                FontSize::Large,
                (1, 1),
                color,
                Blend::Normal2,
                1,
            );
        }
        assert!(g.page(0).to_rows() == base);
    }

    /// Scales too large to draw pixel by pixel, against what each pixel
    /// of a small clipping rectangle should show.
    #[test]
    fn huge_scales_land_where_the_pixels_would() {
        let font = Font::fallback();
        let (mut g, base) = chars_pages(&font);
        let mut next = chars_rng();
        const CODE: u16 = 65;
        for _ in 0..150 {
            let (sx, sy) = (
                [1000, 4096, 32767][next(3) as usize],
                [1000, 4096, 32767][next(3) as usize],
            );
            let clip = chars_clip(&mut next);
            let clip = Clip::new(clip.x0, clip.y0, clip.x0 + 40, clip.y0 + 40);
            let x = chars_pos(&mut next, clip.x0, clip.x1, 16 * sx);
            let y = chars_pos(&mut next, clip.y0, clip.y1, 16 * sy);
            g.clip = clip;
            g.set_page(0, Page::from_rows(base.clone()));
            g.put_chars(
                &font,
                UserPage::DEFAULT,
                (x, y),
                &[CODE, CODE],
                16,
                (sx, sy),
                RED,
                Blend::Normal,
            );
            for py in clip.y0..=clip.y1 {
                for px in clip.x0..=clip.x1 {
                    let gx = (i64::from(px) - i64::from(x)).div_euclid(i64::from(sx));
                    let gy = (i64::from(py) - i64::from(y)).div_euclid(i64::from(sy));
                    let (cell, gx) = (gx.div_euclid(16), gx.rem_euclid(16));
                    let want = if (0..2).contains(&cell) && (0..16).contains(&gy) {
                        let glyph = glyph_pixel_px(&font, &g, CODE, 16, gx as u32, gy as u32);
                        multiply(glyph, RED)
                    } else {
                        base[row_major(px, py)]
                    };
                    assert_eq!(g.pixel(0, px, py), want, "{x},{y} {sx}x{sy} at {px},{py}");
                }
            }
        }
    }

    #[test]
    fn text_off_the_page_finishes() {
        let font = Font::fallback();
        let (mut g, _) = chars_pages(&font);
        let text = vec![65; 10_000_000];
        let blanks = vec![32; 10_000_000];
        let big = (32767, 32767);
        g.put_chars(
            &font,
            UserPage::DEFAULT,
            (0, 0),
            &vec![65; 100_000],
            16,
            (1024, 1024),
            RED,
            Blend::Normal,
        );
        g.put_chars(
            &font,
            UserPage::DEFAULT,
            (i32::MAX, 0),
            &text,
            16,
            big,
            RED,
            Blend::Normal,
        );
        g.put_chars(
            &font,
            UserPage::DEFAULT,
            (i32::MIN, 0),
            &text,
            16,
            (1, 1),
            RED,
            Blend::Normal,
        );
        g.put_chars(
            &font,
            UserPage::DEFAULT,
            (0, i32::MIN),
            &text,
            16,
            big,
            RED,
            Blend::Normal,
        );
        let (small, large) = (FontSize::Small, FontSize::Large);
        g.put_chars_proportional(
            &font,
            UserPage::DEFAULT,
            (0, 0),
            &[65; 2000],
            large,
            big,
            RED,
            Blend::Normal,
            1,
        );
        g.put_chars_proportional(
            &font,
            UserPage::DEFAULT,
            (i32::MAX, i32::MAX),
            &text,
            small,
            big,
            RED,
            Blend::Normal,
            64,
        );
        g.put_chars_proportional(
            &font,
            UserPage::DEFAULT,
            (i32::MIN, 0),
            &blanks,
            large,
            (1, 1),
            RED,
            Blend::Normal,
            0,
        );
        assert_eq!(
            g.proportional_width(&font, &blanks, large, 1, 0),
            40_000_000
        );
        // Too wide for an integer wraps round, as it always has.
        assert_eq!(
            g.proportional_width(&font, &vec![32; 1 << 20], large, 32767, 64),
            ((68i64 * (1 << 20) - 64) * 32767) as i32
        );
    }
}
