//! Graphic pages GRP0-GRP5 and drawing on them. A page takes memory only
//! once something is drawn on it; until then every pixel is 0.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};

use sb_core::value::{Array, ArrayData, ArrayRef, ElemType};

use crate::font::{Font, FontSize};
use crate::text::{glyph_pixel, glyph_pixel_px};

pub const PAGES: usize = 6;
/// `#GRPWIDTH` and `#GRPHEIGHT`.
pub const GRP_SIZE: i32 = 2048;
/// Lines whose ends lie further out than this are clipped before they are
/// walked, so that huge coordinates cannot stall the program.
const LINE_LIMIT: i32 = 1 << 16;

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

    fn apply(self, dst: u32, src: u32) -> u32 {
        let [sa, sr, sg, sb] = src.to_be_bytes().map(u32::from);
        let [da, dr, dg, db] = dst.to_be_bytes().map(u32::from);
        let pack = |a: u32, r: u32, g: u32, b: u32| {
            u32::from_be_bytes([a as u8, r as u8, g as u8, b as u8])
        };
        match self {
            Blend::Normal => src,
            Blend::Normal2 if sa == 0 => dst,
            Blend::Normal2 => src,
            // Modes 2 and 4 keep the page's alpha; mode 3 combines both.
            // The rounding matches values read back on hardware.
            Blend::Alpha => {
                let mix = |s: u32, d: u32| (s * sa + d * (255 - sa)) / 255;
                pack(da, mix(sr, dr), mix(sg, dg), mix(sb, db))
            }
            Blend::Alpha2 => {
                let a = sa + da - sa * da / 255;
                if a == 0 {
                    return 0;
                }
                let dw = da * (255 - sa);
                let mix = |s: u32, d: u32| (s * sa * 255 + d * dw) / (255 * a);
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

/// [`multiply`] by one color, by table when many pixels are to be
/// multiplied.
// Made for one sprite on the stack and dropped; boxing the tables would
// only add an allocation.
#[allow(clippy::large_enum_variant)]
pub enum Tint {
    /// White, which changes nothing.
    White,
    /// Too few pixels to be worth the tables.
    Few(u32),
    /// White but for the alpha, the usual way to fade a sprite.
    Alpha([u8; 256]),
    /// Blue, green, red and alpha.
    Tables([[u8; 256]; 4]),
}

impl Tint {
    /// For about `pixels` pixels.
    pub fn new(color: u32, pixels: usize) -> Tint {
        let table = |shift: u32| {
            let c = color >> shift & 0xFF;
            std::array::from_fn(|v| (v as u32 * c / 255) as u8)
        };
        match color {
            0xFFFF_FFFF => Tint::White,
            _ if pixels < 4096 => Tint::Few(color),
            _ if color | 0xFF00_0000 == 0xFFFF_FFFF => Tint::Alpha(table(24)),
            _ => Tint::Tables([table(0), table(8), table(16), table(24)]),
        }
    }

    #[inline]
    pub fn apply(&self, p: u32) -> u32 {
        self.with(Apply(p))
    }

    /// Runs `job` with the tint as a plain function, decided once rather
    /// than for every pixel.
    #[inline]
    pub fn with<J: TintJob>(&self, job: J) -> J::Out {
        match self {
            Tint::White => job.run(|p| p),
            Tint::Few(color) => job.run(|p| multiply(p, *color)),
            Tint::Alpha(a) => job.run(|p| p & 0x00FF_FFFF | u32::from(a[(p >> 24) as usize]) << 24),
            Tint::Tables([b, g, r, a]) => job.run(|p| {
                u32::from(b[(p & 0xFF) as usize])
                    | u32::from(g[(p >> 8 & 0xFF) as usize]) << 8
                    | u32::from(r[(p >> 16 & 0xFF) as usize]) << 16
                    | u32::from(a[(p >> 24) as usize]) << 24
            }),
        }
    }
}

/// Work done with a [`Tint`], see [`Tint::with`].
pub trait TintJob {
    type Out;
    fn run(self, tint: impl Fn(u32) -> u32 + Copy) -> Self::Out;
}

struct Apply(u32);

impl TintJob for Apply {
    type Out = u32;
    fn run(self, tint: impl Fn(u32) -> u32 + Copy) -> u32 {
        tint(self.0)
    }
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

pub struct Graphics {
    /// Empty until drawn on.
    pages: Vec<Vec<u32>>,
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

/// Where the next [`Graphics`] starts counting its pages' changes, so
/// that a count is never one that another seen before had.
static CHANGES_FROM: AtomicU64 = AtomicU64::new(0);

impl Default for Graphics {
    fn default() -> Self {
        let from = CHANGES_FROM.fetch_add(1, Ordering::Relaxed) << 40;
        Graphics {
            pages: vec![Vec::new(); PAGES],
            target: 0,
            color: 0xFFFF_FFFF,
            clip: Clip::PAGE,
            sample: [0; PAGES],
            arrays: Default::default(),
            changes: [from; PAGES],
            coverage: RefCell::new(std::array::from_fn(|_| Coverage::default())),
        }
    }
}

fn index(x: i32, y: i32) -> usize {
    (y * GRP_SIZE + x) as usize
}

impl Graphics {
    /// A pixel of a page; 0 outside it.
    pub fn pixel(&self, page: usize, x: i32, y: i32) -> u32 {
        let pixels = &self.pages[page];
        if pixels.is_empty() || !Clip::PAGE.contains(x, y) {
            return 0;
        }
        pixels[index(x, y)]
    }

    /// A page's pixels row by row as shown, 2048 to a row; empty while the
    /// page is blank.
    pub fn page(&self, page: usize) -> &[u32] {
        &self.pages[page]
    }

    /// A pixel of a page as programs read it: from its GARRAY array if it
    /// has one, else as shown.
    pub fn read(&self, page: usize, x: i32, y: i32) -> u32 {
        if let Some(array) = &self.arrays[page]
            && Clip::PAGE.contains(x, y)
            && let Ok(array) = array.try_borrow()
            && let ArrayData::Int(v) = &array.data
            && let Some(&p) = v.get(index(x, y))
        {
            return p as u32;
        }
        self.pixel(page, x, y)
    }

    /// Whether a page is empty, as at start-up or after GCLS with 0.
    pub fn is_blank(&self, page: usize) -> bool {
        self.pages[page].is_empty()
    }

    /// A count that differs from the last one read whenever the page's
    /// pixels may have changed since.
    pub fn changes(&self, page: usize) -> u64 {
        self.changes[page]
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
            let row = &self.pages[page][y * rows..][..rows];
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

    /// Replaces a whole page, such as GRP4 with the default sprite image.
    /// `pixels` holds 2048x2048 pixels row by row.
    pub fn set_page(&mut self, page: usize, pixels: Vec<u32>) {
        assert_eq!(pixels.len(), (GRP_SIZE * GRP_SIZE) as usize);
        self.pages[page] = pixels;
        self.changes[page] += 1;
        self.mirror_page(page);
    }

    /// Empties a page, as GCLS does with 0.
    pub fn clear_page(&mut self, page: usize) {
        self.pages[page] = Vec::new();
        self.changes[page] += 1;
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
        array.data = ArrayData::Int(self.page_pixels(page).iter().map(|&p| p as i32).collect());
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
            for (i, d) in v.iter_mut().enumerate() {
                *d = pixels.get(i).map_or(0, |&p| p as i32);
            }
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
        let pixels = self.page_pixels(page);
        for y in rect.y0..=rect.y1 {
            let row = index(rect.x0, y)..=index(rect.x1, y);
            for i in row {
                pixels[i] = v[i] as u32;
            }
        }
    }

    fn target_pixels(&mut self) -> &mut Vec<u32> {
        self.page_pixels(self.target)
    }

    /// A page's pixels to change.
    fn page_pixels(&mut self, page: usize) -> &mut Vec<u32> {
        self.changes[page] += 1;
        let pixels = &mut self.pages[page];
        if pixels.is_empty() {
            *pixels = vec![0; (GRP_SIZE * GRP_SIZE) as usize];
        }
        pixels
    }

    /// Draws one pixel on the target page, inside the clipping rectangle.
    fn plot(&mut self, x: i32, y: i32, color: u32, blend: Blend) {
        if !self.clip.contains(x, y) {
            return;
        }
        let i = index(x, y);
        // Without a GARRAY array, the page is the only copy.
        let shared = self.arrays[self.target].is_some();
        let dst = if shared {
            self.read(self.target, x, y)
        } else {
            self.target_pixels()[i]
        };
        let p = blend.apply(dst, color);
        self.target_pixels()[i] = p;
        if shared
            && let Some(array) = &self.arrays[self.target]
            && let Ok(mut array) = array.try_borrow_mut()
            && let ArrayData::Int(v) = &mut array.data
            && let Some(d) = v.get_mut(i)
        {
            *d = p as i32;
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
        for (x, y) in line_points(x0, y0, x1, y1) {
            self.plot(x, y, color, blend);
        }
    }

    /// GBOX: the outline, each pixel drawn once.
    pub fn rect(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, color: u32, blend: Blend) {
        let (l, r) = (x0.min(x1), x0.max(x1));
        let (t, b) = (y0.min(y1), y0.max(y1));
        let c = self.clip;
        for x in l.max(c.x0)..=r.min(c.x1) {
            self.plot(x, t, color, blend);
            if b != t {
                self.plot(x, b, color, blend);
            }
        }
        for y in (t + 1).max(c.y0)..=(b - 1).min(c.y1) {
            self.plot(l, y, color, blend);
            if r != l {
                self.plot(r, y, color, blend);
            }
        }
    }

    /// GFILL.
    pub fn fill(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, color: u32, blend: Blend) {
        let c = self.clip;
        let (left, right) = (x0.min(x1).max(c.x0), x0.max(x1).min(c.x1));
        let (top, bottom) = (y0.min(y1).max(c.y0), y0.max(y1).min(c.y1));
        if left > right || top > bottom {
            return;
        }
        let page = self.target;
        self.page_pixels(page);
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
        for y in top..=bottom {
            let (from, to) = (index(left, y), index(right, y) + 1);
            let row = &mut pixels[from..to];
            match mirror.as_deref_mut() {
                None if blend == Blend::Normal => row.fill(color),
                None => {
                    for p in row {
                        *p = blend.apply(*p, color);
                    }
                }
                Some(v) => {
                    for (i, p) in (from..to).zip(row) {
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
    /// `sector` is set.
    pub fn circle(
        &mut self,
        (cx, cy, r): (i32, i32, i32),
        arc: Option<(i32, i32, bool)>,
        color: u32,
        blend: Blend,
    ) {
        // UNVERIFIED: circles too large to cross the page draw nothing.
        if !(0..LINE_LIMIT).contains(&r) {
            return;
        }
        let mut points = circle_points(r);
        if let Some((start, end, _)) = arc {
            let (s, e) = (start.rem_euclid(360), end.rem_euclid(360));
            // An arc from an angle to the same angle draws nothing.
            // UNVERIFIED: whether a sector does the same.
            if s == e {
                return;
            }
            points.retain(|&(dx, dy)| {
                let a = f64::from(dy)
                    .atan2(f64::from(dx))
                    .to_degrees()
                    .rem_euclid(360.0);
                let (s, e) = (f64::from(s), f64::from(e));
                if s <= e {
                    (s..=e).contains(&a)
                } else {
                    a >= s || a <= e
                }
            });
        }
        let mut points: Vec<(i32, i32)> = points
            .into_iter()
            .map(|(dx, dy)| (cx + dx, cy + dy))
            .collect();
        if let Some((start, end, true)) = arc {
            // A sector is outlined, not filled (checked on hardware).
            for angle in [start, end] {
                let (s, c) = f64::from(angle).to_radians().sin_cos();
                let x = cx + (c * f64::from(r)).round() as i32;
                let y = cy + (s * f64::from(r)).round() as i32;
                points.extend(line_points(cx, cy, x, y));
            }
        }
        points.sort_unstable();
        points.dedup();
        for (x, y) in points {
            self.plot(x, y, color, blend);
        }
    }

    /// GTRI: a filled triangle, edges included.
    pub fn triangle(&mut self, p: [(i32, i32); 3], color: u32, blend: Blend) {
        let c = self.clip;
        let x0 = p.iter().map(|p| p.0).min().unwrap().max(c.x0);
        let x1 = p.iter().map(|p| p.0).max().unwrap().min(c.x1);
        let y0 = p.iter().map(|p| p.1).min().unwrap().max(c.y0);
        let y1 = p.iter().map(|p| p.1).max().unwrap().min(c.y1);
        // i128, since products of far-off coordinates overflow i64.
        let p = p.map(|(x, y)| (i128::from(x), i128::from(y)));
        let edge = |a: (i128, i128), b: (i128, i128), x: i128, y: i128| {
            (b.0 - a.0) * (y - a.1) - (b.1 - a.1) * (x - a.0)
        };
        let area = edge(p[0], p[1], p[2].0, p[2].1);
        if area == 0 {
            // Degenerate: the three points are on one line.
            for (a, b) in [(0, 1), (1, 2), (2, 0)] {
                let (a, b) = (p[a], p[b]);
                for (x, y) in line_points(a.0 as i32, a.1 as i32, b.0 as i32, b.1 as i32) {
                    if c.contains(x, y) {
                        self.plot(x, y, color, blend);
                    }
                }
            }
            return;
        }
        let sign = area.signum();
        for y in y0..=y1 {
            for x in x0..=x1 {
                let (x, y) = (i128::from(x), i128::from(y));
                let inside = [(0, 1), (1, 2), (2, 0)]
                    .iter()
                    .all(|&(a, b)| edge(p[a], p[b], x, y) * sign >= 0);
                let (x, y) = (x as i32, y as i32);
                if inside {
                    self.plot(x, y, color, blend);
                }
            }
        }
    }

    /// GPAINT: fills the area around a point that has the point's color,
    /// or, with `border`, everything up to that color.
    pub fn paint(&mut self, x: i32, y: i32, color: u32, border: Option<u32>) {
        let c = self.clip;
        if !c.contains(x, y) {
            return;
        }
        let page = self.target;
        let start = self.read(page, x, y);
        let fills = |p: u32| match border {
            Some(b) => p != b && p != color,
            None => p == start,
        };
        if !fills(start) || (border.is_none() && start == color) {
            return;
        }
        let w = (c.x1 - c.x0 + 1) as usize;
        let mut seen = vec![false; w * (c.y1 - c.y0 + 1) as usize];
        let mut stack = vec![(x, y)];
        while let Some((x, y)) = stack.pop() {
            if !c.contains(x, y) {
                continue;
            }
            let i = (y - c.y0) as usize * w + (x - c.x0) as usize;
            if seen[i] || !fills(self.read(page, x, y)) {
                continue;
            }
            seen[i] = true;
            self.plot(x, y, color, Blend::Normal);
            stack.extend([(x + 1, y), (x - 1, y), (x, y + 1), (x, y - 1)]);
        }
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
        // Read first, so that a copy within one page may overlap.
        let mut pixels = Vec::with_capacity((w * h) as usize);
        if self.arrays[src].is_none() {
            // The source lies within the page; read it a row at a time.
            let page = &self.pages[src];
            if page.is_empty() {
                pixels.resize((w * h) as usize, 0);
            } else {
                for y in from.y0..=from.y1 {
                    pixels.extend_from_slice(&page[index(from.x0, y)..=index(from.x1, y)]);
                }
            }
        } else {
            for y in 0..h {
                for x in 0..w {
                    pixels.push(self.read(src, from.x0 + x, from.y0 + y));
                }
            }
        }
        let c = self.clip;
        let (ys, xs) = (
            (c.y0 - dy).max(0)..=(c.y1 - dy).min(h - 1),
            (c.x0 - dx).max(0)..=(c.x1 - dx).min(w - 1),
        );
        if ys.is_empty() || xs.is_empty() {
            return;
        }
        if self.arrays[self.target].is_none() {
            // Without a GARRAY array to keep the same, rows go straight
            // into the page, inside the clipping rectangle.
            let n = (xs.end() - xs.start() + 1) as usize;
            let target = self.target_pixels();
            for y in ys {
                let from = &pixels[(y * w + xs.start()) as usize..][..n];
                let to = &mut target[index(dx + xs.start(), dy + y)..][..n];
                match (tint, blend) {
                    (None, Blend::Normal) => to.copy_from_slice(from),
                    _ => {
                        for (d, &p) in to.iter_mut().zip(from) {
                            *d = blend.apply(*d, tint.map_or(p, |t| multiply(p, t)));
                        }
                    }
                }
            }
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
    /// clear the page, as on hardware.
    #[allow(clippy::too_many_arguments)]
    pub fn put_chars(
        &mut self,
        font: &Font,
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
            for gy in 0..n {
                for gx in 0..n {
                    let glyph = glyph_pixel_px(font, self, code, px, gx as u32, gy as u32);
                    let p = multiply(glyph, color);
                    for py in 0..sy {
                        for px in 0..sx {
                            let (x, y) = (left + gx * sx + px, y + gy * sy + py);
                            self.plot(x, y, p, blend);
                        }
                    }
                }
            }
        }
    }

    /// GPUTCHRP's view of a character: the first column it takes and how
    /// many, or `None` for a blank one. It takes what is drawn and one
    /// empty column before that, where the glyph has one.
    fn ink(&self, font: &Font, code: u16, size: FontSize) -> Option<(i32, i32)> {
        let n = size as u32;
        let inked =
            |gx: u32| (0..n).any(|gy| glyph_pixel(font, self, code, size, gx, gy) >> 24 != 0);
        let left = (0..n).find(|&gx| inked(gx))?.saturating_sub(1);
        let right = (0..n).rev().find(|&gx| inked(gx))?;
        Some((left as i32, (right - left + 1) as i32))
    }

    /// How far GPUTCHRP moves on after a character, before the spacing; a
    /// blank one is a quarter of the font size.
    fn advance(&self, font: &Font, code: u16, size: FontSize) -> i32 {
        self.ink(font, code, size)
            .map_or(size as i32 / 4, |(_, w)| w)
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
        let widths: i32 = text.iter().map(|&c| self.advance(font, c, size)).sum();
        (widths + spacing * (text.len() as i32 - 1).max(0)) * sx
    }

    /// GPUTCHRP: characters as wide as each is, `spacing` pixels apart.
    #[allow(clippy::too_many_arguments)]
    pub fn put_chars_proportional(
        &mut self,
        font: &Font,
        (x, y): (i32, i32),
        text: &[u16],
        size: FontSize,
        (sx, sy): (i32, i32),
        color: u32,
        blend: Blend,
        spacing: i32,
    ) {
        let mut left = x;
        for &code in text {
            if let Some((ink, width)) = self.ink(font, code, size) {
                for gy in 0..size as i32 {
                    for gx in ink..ink + width {
                        let glyph = glyph_pixel(font, self, code, size, gx as u32, gy as u32);
                        let p = multiply(glyph, color);
                        for py in 0..sy {
                            for px in 0..sx {
                                let at = left.saturating_add((gx - ink) * sx + px);
                                self.plot(at, y + gy * sy + py, p, blend);
                            }
                        }
                    }
                }
            }
            // UNVERIFIED: that drawing scales the spacing as the width does.
            left = left.saturating_add((self.advance(font, code, size) + spacing) * sx);
        }
    }

    /// GSAVE: a rectangle of a page, row by row.
    pub fn save_rect(&self, page: usize, (x, y, w, h): (i32, i32, i32, i32)) -> Vec<u32> {
        let mut pixels = Vec::with_capacity((w.max(0) * h.max(0)) as usize);
        for py in y..y + h {
            for px in x..x + w {
                pixels.push(self.read(page, px, py));
            }
        }
        pixels
    }

    /// LOADG: copies an image onto a page at `(x, y)` as it is, without
    /// clipping or blending; what falls off the page is dropped.
    pub fn put_image(&mut self, page: usize, (x, y): (i32, i32), image: &crate::image::Image) {
        let pixels = self.page_pixels(page);
        for (row, line) in image.pixels.chunks(image.width.max(1)).enumerate() {
            let py = y + row as i32;
            for (col, &p) in line.iter().enumerate() {
                let px = x + col as i32;
                if Clip::PAGE.contains(px, py) {
                    pixels[index(px, py)] = p;
                }
            }
        }
        self.mirror_page(page);
    }

    /// GLOAD: draws row-by-row pixels into a rectangle of the target page.
    pub fn load_rect(
        &mut self,
        (x, y, w, _h): (i32, i32, i32, i32),
        pixels: &[u32],
        tint: Option<u32>,
        blend: Blend,
    ) {
        for (i, &p) in pixels.iter().enumerate() {
            let (px, py) = (x + i as i32 % w, y + i as i32 / w);
            let p = tint.map_or(p, |t| multiply(p, t));
            self.plot(px, py, p, blend);
        }
    }
}

/// The points of a circle of radius `r` around the origin, without
/// repeats.
fn circle_points(r: i32) -> Vec<(i32, i32)> {
    let mut points = Vec::new();
    let (mut x, mut y, mut err) = (r, 0, 1 - r);
    while x >= y {
        for (px, py) in [
            (x, y),
            (y, x),
            (-y, x),
            (-x, y),
            (-x, -y),
            (-y, -x),
            (y, -x),
            (x, -y),
        ] {
            points.push((px, py));
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

fn line_points(x0: i32, y0: i32, x1: i32, y1: i32) -> Vec<(i32, i32)> {
    let Some((x0, y0, x1, y1)) = clip_line(x0, y0, x1, y1) else {
        return Vec::new();
    };
    // Walk the longer axis. The rule below was worked out from lines read
    // back on SmileBASIC 4: the same pixels whichever end comes first, as
    // if drawn from the end with the smaller major coordinate, with the
    // minor offset rounded down for slopes of 1/2 and more, and up below.
    let x_major = (x1 - x0).abs() >= (y1 - y0).abs();
    let swap = |(x, y): (i32, i32)| if x_major { (x, y) } else { (y, x) };
    let (mut a, mut b) = (swap((x0, y0)), swap((x1, y1)));
    if a.0 > b.0 {
        std::mem::swap(&mut a, &mut b);
    }
    let major = i64::from(b.0 - a.0);
    let minor = i64::from(b.1 - a.1);
    let bias = if 2 * minor.abs() < major {
        major - 1
    } else {
        0
    };
    (0..=major)
        .map(|t| {
            let offset = if major == 0 {
                0
            } else {
                (t * minor.abs() + bias) / major
            };
            let p = (a.0 + t as i32, a.1 + (minor.signum() * offset) as i32);
            swap(p)
        })
        .collect()
}

/// Shortens a line whose ends lie far outside every page to the part
/// near the pages (Liang-Barsky). `None` if none of it is near.
fn clip_line(x0: i32, y0: i32, x1: i32, y1: i32) -> Option<(i32, i32, i32, i32)> {
    let near = |v: i32| (-LINE_LIMIT..=LINE_LIMIT).contains(&v);
    if near(x0) && near(y0) && near(x1) && near(y1) {
        return Some((x0, y0, x1, y1));
    }
    let (lo, hi) = (-1.0, f64::from(GRP_SIZE));
    let (fx0, fy0) = (f64::from(x0), f64::from(y0));
    let (dx, dy) = (f64::from(x1) - fx0, f64::from(y1) - fy0);
    let (mut t0, mut t1) = (0.0f64, 1.0f64);
    for (p, q) in [
        (-dx, fx0 - lo),
        (dx, hi - fx0),
        (-dy, fy0 - lo),
        (dy, hi - fy0),
    ] {
        if p == 0.0 {
            if q < 0.0 {
                return None;
            }
            continue;
        }
        let t = q / p;
        if p < 0.0 {
            t0 = t0.max(t);
        } else {
            t1 = t1.min(t);
        }
        if t0 > t1 {
            return None;
        }
    }
    let at = |t: f64| ((fx0 + dx * t).round() as i32, (fy0 + dy * t).round() as i32);
    let ((a, b), (c, d)) = (at(t0), at(t1));
    Some((a, b, c, d))
}

#[cfg(test)]
mod tint_tests {
    use super::*;

    #[test]
    fn tint_matches_multiply() {
        for color in [
            0xFFFF_FFFF,
            0x80FF_FFFF,
            0xFF80_40C0,
            0x0000_0000,
            0x7F12_3456,
        ] {
            for pixels in [1, 1 << 20] {
                let tint = Tint::new(color, pixels);
                for p in [0u32, 0xFFFF_FFFF, 0x8040_2010, 0x01FE_7F80, 0xDEAD_BEEF] {
                    assert_eq!(tint.apply(p), multiply(p, color), "{color:08X} {p:08X}");
                }
            }
        }
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
            let mut points = line_points(50, 50, 50 + dx, 50 + dy);
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
            g.target = 0;
            g.copy(0, (0, 0, 12, 6), (2, 1), None, Blend::Normal);
            (0..2)
                .flat_map(|page| (0..GRP_SIZE).map(move |x| (page, x)))
                .flat_map(|(page, x)| (0..12).map(move |y| (page, x, y)))
                .map(|(page, x, y)| g.pixel(page, x, y))
                .collect::<Vec<_>>()
        };
        assert_eq!(draw(false), draw(true));
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
}
