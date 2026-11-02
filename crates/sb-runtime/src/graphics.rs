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
            // The weights of the two colours are worked out on 0-255 and
            // rounded down before they are used.
            Blend::Alpha2 => {
                let a = sa + da - sa * da / 255;
                if a == 0 {
                    return 0;
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
        for y in t.max(c.y0)..=(b - 1).min(c.y1) {
            self.plot(l, y, color, blend);
            if r != l {
                self.plot(r, y, color, blend);
            }
        }
    }

    /// GFILL.
    pub fn fill(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, color: u32, blend: Blend) {
        let (left, right) = (x0.min(x1), x0.max(x1));
        let rows = y0.min(y1)..=y0.max(y1);
        self.fill_rows(rows, |_| Some((left, right)), color, blend);
    }

    /// Fills, in each row of `rows`, the columns `span` gives for it, both
    /// ends included, inside the clipping rectangle.
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
        for (y, (left, right)) in rows {
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
                    sides.extend(line_points(cx, cy, near(x), near(y)));
                }
            }
        }
        if !sides.is_empty() {
            points.extend(sides);
            points.sort_unstable();
            points.dedup();
        }
        for (x, y) in points {
            self.plot(x, y, color, blend);
        }
    }

    /// GTRI: a filled triangle, drawn a row at a time from the leftmost
    /// to the rightmost pixel any of its edges reaches in the row, so that
    /// each pixel is drawn once. See [`edge_span`] for an edge's pixels.
    pub fn triangle(&mut self, p: [(i32, i32); 3], color: u32, blend: Blend) {
        let top = p.iter().map(|p| p.1).min().unwrap();
        let bottom = p.iter().map(|p| p.1).max().unwrap();
        let span = |y| {
            let spans = [(0, 1), (1, 2), (2, 0)]
                .into_iter()
                .filter_map(|(a, b)| edge_span(p[a], p[b], y));
            let (mut l, mut r) = (i64::MAX, i64::MIN);
            for (a, b) in spans {
                (l, r) = (l.min(a), r.max(b));
            }
            let near = |v: i64| v.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32;
            (l <= r).then(|| (near(l), near(r)))
        };
        self.fill_rows(top..=bottom, span, color, blend);
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
            // Drawing scales the spacing as the width does.
            left = left.saturating_add((self.advance(font, code, size) + spacing) * sx);
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
/// the page, without repeats: the midpoint circle, whose pixel in each
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
        for y in 0..=last {
            while x * (x - 1) >= rr - y * y {
                x -= 1;
            }
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
        points.sort_unstable();
        points.dedup();
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
/// `(major + 2·minor) mod (2·major)` when the walk starts on the page:
/// the line as rounded one step further on, moved back so that it starts
/// at its first end (`round((t+1)·minor/major) - round(minor/major)`,
/// halves up). A walk that starts off the page takes `k = major`, the
/// line itself rounded (halves up), counted from its real start; only
/// the steps on the page are worked out.
// UNVERIFIED: that it is the page that counts (not GCLIP's rectangle),
// and that a start off the page across the walk (an X-major line
// starting above the page, say) counts as off the page too.
fn line_points(x0: i32, y0: i32, x1: i32, y1: i32) -> Vec<(i32, i32)> {
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
    let on_page = |v: i64| (0..=page).contains(&v);
    let k = if on_page(a.0) && on_page(a.1) {
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

/// The pixels a GTRI edge from `a` to `b` takes in row `y`, as the first
/// and last column, or `None` if it does not reach the row. An edge is
/// walked like a GLINE, one pixel per step along its longer axis
/// (`major` steps, `minor` across), the offset across at step `t` being
/// `(2·t·minor + k) / (2·major)` rounded down:
/// - an edge taller than it is wide is walked down from its top end with
///   `k = major` (the edge rounded, halves towards its bottom end);
/// - one at least as wide as it is tall is walked up from its bottom end
///   with `k = (major - 2·minor) mod (2·major)`: the edge rounded one step
///   back, moved so that it starts at that end.
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
    let (x0, y0, x1, y1, y) = (
        T::from(x0),
        T::from(y0),
        T::from(x1),
        T::from(y1),
        T::from(y),
    );
    let (zero, one, two) = (T::from(0), T::from(1), T::from(2));
    let (dx, dy) = (x1 - x0, y1 - y0);
    let wide = dx.abs();
    if wide < dy {
        // Down from the top: one pixel a row.
        let x = (two * (y - y0) * wide + dy).div_euclid(two * dy);
        let x = if dx < zero { x0 - x } else { x0 + x };
        return Some((x.narrow(), x.narrow()));
    }
    if dy == zero {
        return Some((x0.min(x1).narrow(), x0.max(x1).narrow()));
    }
    // Up from the bottom: the steps `t` whose offset is `j`.
    let (major, minor, j) = (wide, dy, y1 - y);
    let mut k = major - two * minor;
    if k < zero {
        k = k + two * major;
    }
    let ceil = |n: T, d: T| -(-n).div_euclid(d);
    let first = ceil(two * major * j - k, two * minor).max(zero);
    let last = (ceil(two * major * (j + one) - k, two * minor) - one).min(major);
    if first > last {
        return None;
    }
    // X runs from the bottom end towards the top one.
    let (l, r) = if dx < zero {
        (x1 + first, x1 + last)
    } else {
        (x1 - last, x1 - first)
    };
    Some((l.narrow(), r.narrow()))
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
                    line_points(50 + dx, 50 + dy, 50, 50)
                } else {
                    line_points(50, 50, 50 + dx, 50 + dy)
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
                let mut points = line_points(a.0, a.1, b.0, b.1);
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

    #[test]
    fn circle_points_are_the_midpoint_circle() {
        for r in 0..600 {
            let mut expected: Vec<(i32, i32)> = midpoint(r)
                .into_iter()
                .map(|(x, y)| (x + 1000, y + 900))
                .collect();
            expected.sort_unstable();
            assert_eq!(circle_points(1000, 900, r), expected, "radius {r}");
        }
        // Off the edges, only what lands on the page.
        let mut expected: Vec<(i32, i32)> = midpoint(300)
            .into_iter()
            .map(|(x, y)| (x + 100, y - 50))
            .filter(|&(x, y)| x >= 0 && y >= 0)
            .collect();
        expected.sort_unstable();
        assert_eq!(circle_points(100, -50, 300), expected);
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
