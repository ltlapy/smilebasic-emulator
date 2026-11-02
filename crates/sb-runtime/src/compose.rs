//! Putting display elements (sprites and text screens) on the frame:
//! placement by an affine transform, clipping, blending, and composing
//! layers.

use std::cmp::Ordering;

use crate::framebuffer::Framebuffer;
use crate::graphics::Clip;

/// Maps `(x, y)` to `(a*x + b*y + tx, c*x + d*y + ty)`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Affine {
    pub a: f64,
    pub b: f64,
    pub c: f64,
    pub d: f64,
    pub tx: f64,
    pub ty: f64,
}

impl Affine {
    pub const IDENTITY: Affine = Affine {
        a: 1.0,
        b: 0.0,
        c: 0.0,
        d: 1.0,
        tx: 0.0,
        ty: 0.0,
    };

    /// Takes `home` to `pos`, scaled and then rotated by `rot` degrees
    /// clockwise around it: what SPHOME, SPOFS, SPSCALE and SPROT do, and
    /// their text screen and layer counterparts.
    pub fn place(pos: (f64, f64), home: (f64, f64), rot: f64, scale: (f64, f64)) -> Affine {
        let (sin, cos) = rot.to_radians().sin_cos();
        let (a, b) = (cos * scale.0, -sin * scale.1);
        let (c, d) = (sin * scale.0, cos * scale.1);
        Affine {
            a,
            b,
            c,
            d,
            tx: pos.0 - (a * home.0 + b * home.1),
            ty: pos.1 - (c * home.0 + d * home.1),
        }
    }

    /// This transform followed by `outer`.
    pub fn then(&self, outer: &Affine) -> Affine {
        let o = outer;
        Affine {
            a: o.a * self.a + o.b * self.c,
            b: o.a * self.b + o.b * self.d,
            c: o.c * self.a + o.d * self.c,
            d: o.c * self.b + o.d * self.d,
            tx: o.a * self.tx + o.b * self.ty + o.tx,
            ty: o.c * self.tx + o.d * self.ty + o.ty,
        }
    }

    pub fn apply(&self, x: f64, y: f64) -> (f64, f64) {
        (
            self.a * x + self.b * y + self.tx,
            self.c * x + self.d * y + self.ty,
        )
    }

    /// `None` if the transform flattens everything (a scale of 0).
    pub fn inverse(&self) -> Option<Affine> {
        let det = self.a * self.d - self.b * self.c;
        if det == 0.0 || !det.is_finite() {
            return None;
        }
        let (a, b, c, d) = (self.d / det, -self.b / det, -self.c / det, self.a / det);
        Some(Affine {
            a,
            b,
            c,
            d,
            tx: -(a * self.tx + b * self.ty),
            ty: -(c * self.tx + d * self.ty),
        })
    }
}

// Frames and layers hold premultiplied colors: each channel already
// multiplied by the alpha. The alpha is coverage: 255 less the
// transmittance SmileBASIC 4's render targets keep in theirs, so a clear
// layer is 0 and an opaque frame 255. In a layer of its own, an
// additive element adds light without adding alpha, and lights what is
// under the layer once it is composed.
//
// A draw is worked out exactly and rounded once, to the nearest, when it
// is stored: an element's color times its tint times its alpha, plus
// what it leaves of the frame, as one sum. Every denominator here is a
// power of 255, which is odd, so no exact result lies halfway between
// two values: rounding half up or half down would give the same pixels,
// and `(x + (d-1)/2) / d` is the nearest for an odd `d`. A GPU working
// in floating point may still round a result very near a half either
// way.

/// 255³, what [`Over3`]'s numerators are over.
const D3: u32 = 255 * 255 * 255;
/// 255⁵, what [`Over5`]'s numerators are over.
const D5: u64 = 255 * 255 * 255 * 255 * 255;

/// One pixel of an element as it goes on the frame, with what it gives
/// worked out once, so that drawing it many times costs only the blend.
pub trait Paint: Copy {
    /// The frame pixel `dst` with this put on.
    fn put(&self, dst: u32) -> u32;
}

/// An element's straight color over `dst`, premultiplied, untinted: the
/// light `s*a` and what is left of `dst`, `d*(255-a)`, added and
/// divided by 255, rounded once.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Over1 {
    /// In [`spread`]'s lanes, times 255.
    light: u64,
    keep: u64,
}

impl Over1 {
    #[inline]
    pub fn new(src: u32) -> Over1 {
        let sa = u64::from(src >> 24);
        Over1 {
            light: spread(src | 0xFF00_0000) * sa,
            keep: 255 - sa,
        }
    }
}

impl Paint for Over1 {
    #[inline]
    fn put(&self, dst: u32) -> u32 {
        // All four channels at once, 16 bits apiece: each sum is at most
        // 255 * 255, so none carries into the next.
        gather(div255_lanes(self.light + spread(dst) * self.keep))
    }
}

/// An element's light added to the frame, each channel held to 255; the
/// frame's alpha stays as it is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Lit(u64);

impl Lit {
    /// Of an untinted straight color: each channel times its alpha.
    #[inline]
    pub fn new(src: u32) -> Lit {
        Lit(light(src))
    }

    /// Of a straight color tinted by `color`: each channel times the
    /// other's, times both alphas, rounded once.
    #[inline]
    pub fn tinted(src: u32, color: u32) -> Lit {
        let a = (src >> 24) * (color >> 24);
        let c = |shift: u32| {
            let l = (src >> shift & 0xFF) * (color >> shift & 0xFF) * a;
            (l + D3 / 2) / D3
        };
        Lit(spread(c(16) << 16 | c(8) << 8 | c(0)))
    }

    /// Of a straight color tinted by `c1` and then `c2`, as text is by
    /// its cell's color and TCOLOR: one rounding of the whole product.
    #[inline]
    pub fn tinted2(src: u32, c1: u32, c2: u32) -> Lit {
        let a = u64::from((src >> 24) * (c1 >> 24)) * u64::from(c2 >> 24);
        let c = |shift: u32| {
            let ch = |v: u32| u64::from(v >> shift & 0xFF);
            let l = ch(src) * ch(c1) * ch(c2) * a;
            ((l + D5 / 2) / D5) as u32
        };
        Lit(spread(c(16) << 16 | c(8) << 8 | c(0)))
    }
}

impl Paint for Lit {
    #[inline]
    fn put(&self, dst: u32) -> u32 {
        add_light(dst, self.0)
    }
}

/// A straight color tinted by another, over the frame: the products of
/// the two colors' channels and alphas, and what is left of the frame,
/// as numerators over 255³, rounded once.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Over3 {
    /// Blue, green, red and alpha; when `keep` is 0, the pixel it puts.
    light: [u32; 4],
    keep: u32,
}

impl Over3 {
    #[inline]
    pub fn new(src: u32, color: u32) -> Over3 {
        let a = (src >> 24) * (color >> 24);
        let ch = |v: u32, shift: u32| v >> shift & 0xFF;
        // Opaque, the usual case, covers what is under it: the colors'
        // product is all there is.
        if a == 255 * 255 {
            let c = |shift: u32| div255(ch(src, shift) * ch(color, shift));
            return Over3::solid(0xFF00_0000 | c(16) << 16 | c(8) << 8 | c(0));
        }
        Over3::partial(src, color, a)
    }

    /// [`Over3::new`] with `tables` holding each channel times the
    /// color's, divided by 255 and rounded: blue, green and red.
    #[inline]
    pub fn tabled(src: u32, color: u32, tables: &[[u8; 256]; 3]) -> Over3 {
        let a = (src >> 24) * (color >> 24);
        if a == 255 * 255 {
            let c = |i: usize| u32::from(tables[i][(src >> (8 * i) & 0xFF) as usize]) << (8 * i);
            return Over3::solid(0xFF00_0000 | c(2) | c(1) | c(0));
        }
        Over3::partial(src, color, a)
    }

    /// What covers what is under it whole.
    #[inline]
    fn solid(pixel: u32) -> Over3 {
        Over3 {
            light: [pixel, 0, 0, 0],
            keep: 0,
        }
    }

    /// For `src` and `color` whose alphas multiply to `a`, not 255².
    #[inline]
    fn partial(src: u32, color: u32, a: u32) -> Over3 {
        let ch = |v: u32, shift: u32| v >> shift & 0xFF;
        // Each light is at most 255² times `a`, and each `keep` times a
        // channel at most 255² times 255² - `a`: a sum fits 32 bits.
        let c = |shift: u32| ch(src, shift) * ch(color, shift) * a;
        Over3 {
            light: [c(0), c(8), c(16), 255 * 255 * a],
            keep: 255 * (255 * 255 - a),
        }
    }
}

impl Paint for Over3 {
    #[inline]
    fn put(&self, dst: u32) -> u32 {
        if self.keep == 0 {
            return self.light[0];
        }
        let c = |i: usize| {
            let d = dst >> (8 * i) & 0xFF;
            ((self.light[i] + d * self.keep + D3 / 2) / D3) << (8 * i)
        };
        // An opaque frame stays opaque, the usual case.
        let alpha = match dst >> 24 {
            0xFF => 0xFF00_0000,
            _ => c(3),
        };
        c(0) | c(1) | c(2) | alpha
    }
}

/// [`Over3`] for a color tinted twice, as text is by its cell's color and
/// TCOLOR: numerators over 255⁵.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Over5 {
    /// Blue, green, red and alpha.
    light: [u64; 4],
    keep: u64,
}

impl Over5 {
    #[inline]
    pub fn new(src: u32, c1: u32, c2: u32) -> Over5 {
        const A3: u64 = 255 * 255 * 255;
        let a = u64::from((src >> 24) * (c1 >> 24)) * u64::from(c2 >> 24);
        let c = |shift: u32| {
            let ch = |v: u32| u64::from(v >> shift & 0xFF);
            ch(src) * ch(c1) * ch(c2) * a
        };
        Over5 {
            light: [c(0), c(8), c(16), A3 * a],
            keep: 255 * 255 * (A3 - a),
        }
    }

    /// The pixel it puts on whatever is under it, when it covers that
    /// whole.
    #[cfg(test)]
    fn solid(&self) -> Option<u32> {
        (self.keep == 0).then(|| self.put(0))
    }
}

impl Paint for Over5 {
    #[inline]
    fn put(&self, dst: u32) -> u32 {
        let c = |i: usize| {
            let d = u64::from(dst >> (8 * i) & 0xFF);
            (((self.light[i] + d * self.keep + D5 / 2) / D5) as u32) << (8 * i)
        };
        let alpha = match dst >> 24 {
            0xFF => 0xFF00_0000,
            _ => c(3),
        };
        c(0) | c(1) | c(2) | alpha
    }
}

/// Any of the paints, for elements whose pixels each have their own,
/// such as text screens' cells.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mixed {
    /// Opaque: the pixel itself.
    Solid(u32),
    Lit(Lit),
    Over1(Over1),
    Over3(Over3),
    Over5(Over5),
}

impl Mixed {
    /// A straight color tinted by `c1` and then `c2`, put over the frame
    /// or, with `add`, added to it; `None` if it changes nothing. A white
    /// tint changes nothing either, so the cheapest paint that gives the
    /// same pixels is taken.
    pub fn new(src: u32, c1: u32, c2: u32, add: bool) -> Option<Mixed> {
        const WHITE: u32 = 0xFFFF_FFFF;
        if (src >> 24) * (c1 >> 24) * (c2 >> 24) == 0 {
            return None;
        }
        let one = match (c1, c2) {
            (WHITE, WHITE) => Some(WHITE),
            (c, WHITE) | (WHITE, c) => Some(c),
            _ => None,
        };
        if add {
            return Some(Mixed::Lit(match one {
                Some(WHITE) => Lit::new(src),
                Some(c) => Lit::tinted(src, c),
                None => Lit::tinted2(src, c1, c2),
            }));
        }
        let opaque = src & c1 & c2 >= 0xFF00_0000;
        let ch = |v: u32, shift: u32| v >> shift & 0xFF;
        Some(match (one, opaque) {
            (Some(WHITE), true) => Mixed::Solid(src),
            (Some(WHITE), false) => Mixed::Over1(Over1::new(src)),
            (Some(c), true) => Mixed::Solid([16, 8, 0].iter().fold(0xFF00_0000, |out, &shift| {
                out | div255(ch(src, shift) * ch(c, shift)) << shift
            })),
            (Some(c), false) => Mixed::Over3(Over3::new(src, c)),
            (None, true) => Mixed::Solid([16, 8, 0].iter().fold(0xFF00_0000, |out, &shift| {
                let x = ch(src, shift) * ch(c1, shift) * ch(c2, shift);
                out | ((x + 255 * 255 / 2) / (255 * 255)) << shift
            })),
            (None, false) => Mixed::Over5(Over5::new(src, c1, c2)),
        })
    }
}

impl Paint for Mixed {
    #[inline]
    fn put(&self, dst: u32) -> u32 {
        match self {
            Mixed::Solid(c) => *c,
            Mixed::Lit(l) => l.put(dst),
            Mixed::Over1(o) => o.put(dst),
            Mixed::Over3(o) => o.put(dst),
            Mixed::Over5(o) => o.put(dst),
        }
    }
}

/// `src`, an element's straight color, over `dst`, premultiplied: the
/// element's light `s*a` and what is left of `dst`, `d*(255-a)`, added,
/// divided by 255 and rounded to the nearest.
#[inline]
pub fn over(dst: u32, src: u32) -> u32 {
    if src >> 24 == 0xFF {
        return src;
    }
    Over1::new(src).put(dst)
}

/// The channels of a color, one to each 16 bits: blue, red, green and
/// alpha from the bottom.
#[inline]
fn spread(c: u32) -> u64 {
    let c = u64::from(c);
    c & 0x00FF_00FF | (c >> 8 & 0x00FF_00FF) << 32
}

/// Back from [`spread`]; the lanes must hold 8 bits.
#[inline]
fn gather(x: u64) -> u32 {
    (x as u32) | ((x >> 32) as u32) << 8
}

/// [`div255`] of each 16-bit lane.
#[inline]
fn div255_lanes(x: u64) -> u64 {
    // Rounds x / 255 to the nearest for x up to 255 * 255, as div255 does.
    let t = x + 0x0080_0080_0080_0080;
    ((t + (t >> 8 & LANES)) >> 8) & LANES
}

/// `src`, an element's straight color, added to `dst` in proportion to
/// its alpha, leaving `dst`'s alpha as it is.
#[inline]
pub fn add(dst: u32, src: u32) -> u32 {
    add_light(dst, light(src))
}

const LANES: u64 = 0x00FF_00FF_00FF_00FF;
const ONES: u64 = 0x0001_0001_0001_0001;

/// The light [`add`] puts on: each channel times the alpha / 255,
/// rounded to the nearest, in [`spread`]'s lanes.
#[inline]
fn light(src: u32) -> u64 {
    div255_lanes(spread(src & 0x00FF_FFFF) * u64::from(src >> 24))
}

/// `light` added to `dst`, each channel held to 255.
#[inline]
fn add_light(dst: u32, light: u64) -> u32 {
    held(spread(dst & 0x00FF_FFFF) + light) | dst & 0xFF00_0000
}

/// Lanes of up to 9 bits held to 255 and gathered.
#[inline]
fn held(sum: u64) -> u32 {
    gather((sum | ((sum >> 8 & ONES) * 0xFF)) & LANES)
}

/// Where a layer's elements may draw.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Placement<'a> {
    /// From the element's own image to the frame.
    pub matrix: Affine,
    /// LCLIP's rectangle, inclusive.
    pub clip: Option<&'a Clip>,
    pub add: bool,
}

/// Draws a `w` x `h` image whose pixels `sample` gives, placed by
/// `place`, `None` where it is clear. `sample` gets the image point under
/// each frame pixel's center, which falls between whole pixels when the
/// frame is finer than the image. The point is [`Placement::sampling`]'s;
/// add [`Placement::texel_tie`] before reading a texel with it.
pub fn draw_image<P: Paint>(
    fb: &mut Framebuffer,
    place: &Placement,
    w: i32,
    h: i32,
    sample: impl Fn(f64, f64) -> Option<P>,
) {
    draw_image_part(fb, place, w, h, (0, 0, w, h), sample);
}

/// Like [`draw_image`], but only around where the part of the image from
/// `(x0, y0)` to `(x1, y1)` falls. Pixels just outside the part may still
/// be sampled, so `sample` must give nothing there; drawing an image part
/// by part then gives the same frame as drawing it whole, while parts
/// with nothing in them need not be drawn at all.
pub fn draw_image_part<P: Paint>(
    fb: &mut Framebuffer,
    place: &Placement,
    w: i32,
    h: i32,
    part: (i32, i32, i32, i32),
    sample: impl Fn(f64, f64) -> Option<P>,
) {
    let Some(inv) = place.sampling() else {
        return;
    };
    let Some((x0, y0, x1, y1)) = span(fb, place, w, h, part) else {
        return;
    };
    let (w, h) = (f64::from(w), f64::from(h));
    let width = fb.width() as usize;
    let pixels = fb.pixels_mut();
    for py in y0..y1 {
        let line = &mut pixels[py as usize * width..][..x1 as usize];
        let fy = f64::from(py) + 0.5;
        // Only the columns near where the row crosses the image need
        // trying; each is still tested exactly below.
        let (from, to) = crossing(inv.a, inv.b * fy + inv.tx, w)
            .and_then(|(f, t)| {
                let (g, u) = crossing(inv.c, inv.d * fy + inv.ty, h)?;
                Some((f.max(g), t.min(u)))
            })
            .map_or((1.0, 0.0), |(f, t)| (f - 1.5, t + 0.5));
        let from = (from.max(f64::from(x0)) as u32).max(x0);
        let to = if to < f64::from(x1) {
            (to.max(0.0) as u32) + 1
        } else {
            x1
        };
        for px in from..to.min(x1) {
            let (lx, ly) = inv.apply(f64::from(px) + 0.5, f64::from(py) + 0.5);
            if lx < 0.0 || ly < 0.0 || lx >= w || ly >= h {
                continue;
            }
            if let Some(paint) = sample(lx, ly) {
                let dst = &mut line[px as usize];
                *dst = paint.put(*dst);
            }
        }
    }
}

/// Where `k * x + base` lies from 0 up to `len`: the range of x, which
/// may be empty; `None` if it never does.
fn crossing(k: f64, base: f64, len: f64) -> Option<(f64, f64)> {
    if k == 0.0 {
        return (0.0..len).contains(&base).then_some((f64::MIN, f64::MAX));
    }
    let (a, b) = ((0.0 - base) / k, (len - base) / k);
    Some((a.min(b), a.max(b)))
}

/// The frame pixels, from `(x0, y0)` up to but not including `(x1, y1)`,
/// that the part of the image may cover.
fn span(
    fb: &Framebuffer,
    place: &Placement,
    w: i32,
    h: i32,
    part: (i32, i32, i32, i32),
) -> Option<(u32, u32, u32, u32)> {
    let m = &place.matrix;
    if w <= 0 || h <= 0 {
        return None;
    }
    let (w, h) = (f64::from(w), f64::from(h));
    let (mut x0, mut y0) = (f64::from(0), f64::from(0));
    let (mut x1, mut y1) = (f64::from(fb.width()), f64::from(fb.height()));
    if let Some(c) = place.clip {
        x0 = x0.max(f64::from(c.x0));
        y0 = y0.max(f64::from(c.y0));
        x1 = x1.min(f64::from(c.x1) + 1.0);
        y1 = y1.min(f64::from(c.y1) + 1.0);
    }
    let (lx0, ly0, lx1, ly1) = bounds(m, (0.0, 0.0, w, h));
    // A pixel's margin for rounding, so that no pixel the whole image
    // would draw is missed.
    let (px0, py0, px1, py1) = bounds(
        m,
        (
            f64::from(part.0),
            f64::from(part.1),
            f64::from(part.2),
            f64::from(part.3),
        ),
    );
    let x0 = lx0.max(px0 - 1.0).max(x0).floor();
    let y0 = ly0.max(py0 - 1.0).max(y0).floor();
    let x1 = lx1.min(px1 + 1.0).min(x1).ceil();
    let y1 = ly1.min(py1 + 1.0).min(y1).ceil();
    // Also false for NaN, from a matrix far out of range.
    if !(x0 < x1 && y0 < y1) {
        return None;
    }
    Some((x0 as u32, y0 as u32, x1 as u32, y1 as u32))
}

/// Where a placement that neither turns nor shears the image takes the
/// frame: the image x under each frame column from `x0` and the image y
/// under each row from `y0`, as [`draw_image`] works them out, for the
/// columns and rows that fall on the image.
pub struct Grid {
    pub x0: u32,
    pub y0: u32,
    pub xs: Vec<f64>,
    pub ys: Vec<f64>,
}

/// How far a frame pixel's center is moved to settle whether it lies on
/// the image when it is exactly on an edge: to the right, and then, far
/// less so that it decides only what the first leaves open, up. An edge
/// through the center thus takes the pixel if it is the image's left or
/// bottom edge. Well above what rounding does to a point of the frame.
const COVER_NUDGE: (f64, f64) = (1e-8, -1e-10);
/// How far an image point is moved, along the image, to settle which
/// texel it reads when it is exactly between two.
const TIE: f64 = 1e-9;

/// Whether a length, up to rounding, is 2 to a whole power.
fn is_power_of_two(v: f64) -> bool {
    const FRACTION: u64 = (1 << 52) - 1;
    const ROUNDING: u64 = 1 << 16;
    let bits = v.to_bits() & FRACTION;
    !(ROUNDING..=FRACTION - ROUNDING).contains(&bits)
}

impl Placement<'_> {
    /// Whether [`grid`] can stand for the placement.
    pub fn is_upright(&self) -> bool {
        self.matrix.b == 0.0 && self.matrix.c == 0.0
    }

    /// Whether the placement puts the image's pixels on the frame's, so
    /// that sampling one to one nothing can tie.
    pub fn is_aligned(&self) -> bool {
        let m = &self.matrix;
        (m.a, m.b, m.c, m.d) == (1.0, 0.0, 0.0, 1.0) && m.tx.fract() == 0.0 && m.ty.fract() == 0.0
    }

    /// From the frame to the image, for what lies under a frame pixel's
    /// center moved by [`COVER_NUDGE`].
    fn sampling(&self) -> Option<Affine> {
        let mut inv = self.matrix.inverse()?;
        let (nx, ny) = COVER_NUDGE;
        inv.tx += inv.a * nx + inv.b * ny;
        inv.ty += inv.c * nx + inv.d * ny;
        Some(inv)
    }

    /// What to add to an image point from [`Placement::sampling`] to get
    /// the point whose whole part is the texel it reads, for an image of
    /// `size`. `reversed` says along which of the image's axes the page's
    /// coordinates go down. Exactly zero where nothing can tie, so that
    /// callers may leave out the adding.
    // UNVERIFIED: the usual outcome of ties, from sizes 8, 15, 16 and 32
    // with both sides alike, on a PC's GPU; see renderer-design.md.
    pub fn texel_tie(&self, size: (i32, i32), reversed: (bool, bool)) -> (f64, f64) {
        let m = &self.matrix;
        let (nx, ny) = COVER_NUDGE;
        let Some(inv) = m.inverse() else {
            return (0.0, 0.0);
        };
        let moved = (inv.a * nx + inv.b * ny, inv.c * nx + inv.d * ny);
        let along = |tie: (f64, f64)| (tie.0 - moved.0, tie.1 - moved.1);
        let (sx, sy) = (m.a.abs() + m.c.abs(), m.b.abs() + m.d.abs());
        let none_of = |p: f64, q: f64| p.abs() <= 1e-9 * q.abs();
        let right_angle =
            (none_of(m.a, m.c) && none_of(m.d, m.b)) || (none_of(m.c, m.a) && none_of(m.b, m.d));
        if !right_angle {
            // Only at multiples of 45 degrees can a center land exactly
            // between texels at all.
            let even = |p: f64, q: f64| (p.abs() - q.abs()).abs() <= 1e-9 * (p.abs() + q.abs());
            return match even(m.a, m.c) || even(m.b, m.d) {
                true => along((TIE, -TIE)),
                false => (0.0, 0.0),
            };
        }
        // Towards the frame's left and bottom, along the image.
        let frame = (
            (inv.b - inv.a).signum() * TIE,
            (inv.d - inv.c).signum() * TIE,
        );
        let axis = |size: i32, scale: f64, reversed: bool, frame: f64| match (
            size.unsigned_abs().is_power_of_two(),
            is_power_of_two(scale),
        ) {
            (false, _) if reversed => TIE,
            (false, _) => -TIE,
            (true, true) => TIE,
            (true, false) => frame,
        };
        along((
            axis(size.0, sx, reversed.0, frame.0),
            axis(size.1, sy, reversed.1, frame.1),
        ))
    }
}

/// The [`Grid`] of an upright placement, or `None` where it shows
/// nothing.
pub fn grid(fb: &Framebuffer, place: &Placement, w: i32, h: i32) -> Option<Grid> {
    debug_assert!(place.is_upright());
    let inv = place.sampling()?;
    let (x0, y0, x1, y1) = span(fb, place, w, h, (0, 0, w, h))?;
    let (w, h) = (f64::from(w), f64::from(h));
    // With b and c zero, x depends on the column alone and y on the row
    // alone, to the bit; both only grow or only shrink along the frame,
    // so the ones on the image are a run.
    let at = |px: u32, py: u32| inv.apply(f64::from(px) + 0.5, f64::from(py) + 0.5);
    let on = |range: std::ops::Range<u32>, f: &dyn Fn(u32) -> f64, len: f64| {
        let all: Vec<(u32, f64)> = range.map(|p| (p, f(p))).collect();
        let first = all.iter().position(|&(_, v)| v >= 0.0 && v < len)?;
        let n = all[first..]
            .iter()
            .take_while(|&&(_, v)| v >= 0.0 && v < len)
            .count();
        Some((
            all[first].0,
            all[first..first + n].iter().map(|&(_, v)| v).collect(),
        ))
    };
    let (gx0, xs) = on(x0..x1, &|px| at(px, y0).0, w)?;
    let (gy0, ys) = on(y0..y1, &|py| at(x0, py).1, h)?;
    Some(Grid {
        x0: gx0,
        y0: gy0,
        xs,
        ys,
    })
}

/// The smallest and largest x and y that `m` takes the corners of the
/// rectangle from `(x0, y0)` to `(x1, y1)` to.
pub(crate) fn bounds(m: &Affine, (x0, y0, x1, y1): (f64, f64, f64, f64)) -> (f64, f64, f64, f64) {
    let corners = [
        m.apply(x0, y0),
        m.apply(x1, y0),
        m.apply(x0, y1),
        m.apply(x1, y1),
    ];
    let lo = |f: fn(&(f64, f64)) -> f64| corners.iter().map(f).fold(f64::MAX, f64::min);
    let hi = |f: fn(&(f64, f64)) -> f64| corners.iter().map(f).fold(f64::MIN, f64::max);
    (lo(|c| c.0), lo(|c| c.1), hi(|c| c.0), hi(|c| c.1))
}

/// Layers 0-7; 7 is drawn first, so lower layers are nearer the front.
pub const LAYERS: usize = 8;

/// A layer's settings: LAYER, LCLIP and LMATRIX.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Layer {
    pub compose: Compose,
    pub color: u32,
    pub clip: Option<Clip>,
    pub matrix: Affine,
    /// LMATRIX's 4x4 form, column by column as OpenGL keeps it, taking
    /// the layer's coordinates to clip space. It stands instead of
    /// `matrix`.
    pub projection: Option<[f64; 16]>,
}

impl Default for Layer {
    fn default() -> Self {
        Layer {
            compose: Compose::Direct,
            color: 0xFFFF_FFFF,
            clip: None,
            matrix: Affine::IDENTITY,
            projection: None,
        }
    }
}

/// Puts a layer drawn flat in its own coordinates, `sample` frame pixels
/// to one of them, onto `dst` through LMATRIX's 4x4 matrix `m`: a point
/// of the layer goes in as (x, y, 0, 1), and the result's X and Y, divided
/// by its W, run from -1 to 1 across the screen and up it. Only points in
/// front (W above 0) with Z / W from -1 to 1 show; `clip` is in frame
/// pixels. `src`'s top left is the flat frame's pixel `origin`, so it can
/// hold what lies off the screen. It samples without smoothing.
pub fn project(
    dst: &mut Framebuffer,
    src: &Framebuffer,
    origin: (i32, i32),
    m: &[f64; 16],
    sample: u32,
    clip: Option<&Clip>,
) {
    let h = homography(m);
    let Some(inv) = invert3(&h) else {
        return;
    };
    let z = depth_row(m);
    let Some((cx0, cy0, cx1, cy1)) = drawn_bounds(src) else {
        return;
    };
    let (w, hh) = (dst.width(), dst.height());
    let (fw, fh) = (f64::from(w), f64::from(hh));
    let sample = f64::from(sample);
    let (cx1, cy1) = (cx1 + 1, cy1 + 1);
    let (ox, oy) = (f64::from(origin.0), f64::from(origin.1));
    let corners = [(cx0, cy0), (cx1, cy0), (cx0, cy1), (cx1, cy1)]
        .map(|(x, y)| ((f64::from(x) + ox) / sample, (f64::from(y) + oy) / sample));
    let (x0, y0, x1, y1) = window(&h, corners, (w, hh), clip);
    let (sw, sh) = (i64::from(src.width()), i64::from(src.height()));
    let width = w as usize;
    let (from, pixels) = (src.pixels(), dst.pixels_mut());
    // Down a row the homogeneous coordinates change by a constant step.
    let step = 2.0 / fw;
    for py in y0..y1 {
        let ny = 1.0 - (f64::from(py) + 0.5) / fh * 2.0;
        let nx0 = (f64::from(x0) + 0.5) / fw * 2.0 - 1.0;
        let at = |r: usize, nx: f64| inv[r][0] * nx + inv[r][1] * ny + inv[r][2];
        let (mut gx, mut gy, mut q) = (at(0, nx0), at(1, nx0), at(2, nx0));
        let (dx, dy, dq) = (inv[0][0] * step, inv[1][0] * step, inv[2][0] * step);
        let (first, end) = depth_span(
            z[0] * gx + z[1] * gy + z[2] * q,
            z[0] * dx + z[1] * dy + z[2] * dq,
            x1 - x0,
        );
        let skipped = f64::from(first);
        (gx, gy, q) = (gx + dx * skipped, gy + dy * skipped, q + dq * skipped);
        for px in x0 + first..x0 + end {
            let (lx, ly) = (gx / q, gy / q);
            (gx, gy, q) = (gx + dx, gy + dy, q + dq);
            // In front only; a NaN is not.
            if (h[2][0] * lx + h[2][1] * ly + h[2][2]).partial_cmp(&0.0) != Some(Ordering::Greater)
            {
                continue;
            }
            let (Some(sx), Some(sy)) = (
                floor_in(lx * sample - ox, sw),
                floor_in(ly * sample - oy, sh),
            ) else {
                continue;
            };
            let s = from[sy * sw as usize + sx];
            if s == 0 {
                continue;
            }
            let d = &mut pixels[py as usize * width + px as usize];
            *d = over_premultiplied(*d, s);
        }
    }
}

/// The frame pixels, from `(x0, y0)` up to but not including `(x1, y1)`,
/// that a quadrilateral of the layer with `corners` may cover through the
/// homography `h`, within `clip`: only around it when all of it is in
/// front, with a pixel to spare.
/// The smallest and largest x and y of the flat frame's pixels that
/// [`project`] can put on a `w` x `hh` `dst` within `clip`, or None when
/// nothing can show. Toward the horizon they reach very far.
pub fn source_bounds(
    m: &[f64; 16],
    sample: u32,
    (w, hh): (u32, u32),
    clip: Option<&Clip>,
) -> Option<(f64, f64, f64, f64)> {
    let inv = invert3(&homography(m))?;
    let (mut x0, mut y0, mut x1, mut y1) = (0, 0, w as i32, hh as i32);
    if let Some(c) = clip {
        (x0, y0) = (x0.max(c.x0), y0.max(c.y0));
        (x1, y1) = (x1.min(c.x1 + 1), y1.min(c.y1 + 1));
    }
    let (fw, fh) = (f64::from(w), f64::from(hh));
    // The window's corners in clip space, each with the layer point that
    // shows there, (x, y) / q; it is in front where q is above 0.
    let corners = [(x0, y0), (x1, y0), (x1, y1), (x0, y1)].map(|(px, py)| {
        let (nx, ny) = (
            f64::from(px) / fw * 2.0 - 1.0,
            1.0 - f64::from(py) / fh * 2.0,
        );
        let at = |r: usize| inv[r][0] * nx + inv[r][1] * ny + inv[r][2];
        (at(0), at(1), at(2))
    });
    // The window cut at the horizon: q is linear along each edge, so the
    // part in front is where it stays above a sliver of its largest.
    let top = corners.iter().map(|c| c.2).fold(f64::MIN, f64::max);
    if top.partial_cmp(&0.0) != Some(Ordering::Greater) {
        return None;
    }
    let near = top * 1e-6;
    let s = f64::from(sample);
    let (mut lo_x, mut lo_y, mut hi_x, mut hi_y) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
    let mut take = |(x, y, q): (f64, f64, f64)| {
        let (x, y) = (x / q * s, y / q * s);
        (lo_x, lo_y, hi_x, hi_y) = (lo_x.min(x), lo_y.min(y), hi_x.max(x), hi_y.max(y));
    };
    for (i, &a) in corners.iter().enumerate() {
        let b = corners[(i + 1) % 4];
        if a.2 >= near {
            take(a);
        }
        if (a.2 >= near) != (b.2 >= near) {
            let t = (near - a.2) / (b.2 - a.2);
            take((a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t, near));
        }
    }
    Some((lo_x, lo_y, hi_x, hi_y))
}

fn window(
    h: &[[f64; 3]; 3],
    corners: [(f64, f64); 4],
    (w, hh): (u32, u32),
    clip: Option<&Clip>,
) -> (u32, u32, u32, u32) {
    let (fw, fh) = (f64::from(w), f64::from(hh));
    let (mut x0, mut y0, mut x1, mut y1) = (0, 0, w, hh);
    if let Some(c) = clip {
        x0 = x0.max(c.x0.max(0) as u32);
        y0 = y0.max(c.y0.max(0) as u32);
        x1 = x1.min((c.x1 + 1).max(0) as u32);
        y1 = y1.min((c.y1 + 1).max(0) as u32);
    }
    let corners = corners.map(|(x, y)| {
        let q = h[2][0] * x + h[2][1] * y + h[2][2];
        let nx = (h[0][0] * x + h[0][1] * y + h[0][2]) / q;
        let ny = (h[1][0] * x + h[1][1] * y + h[1][2]) / q;
        (q > 0.0).then_some(((nx + 1.0) / 2.0 * fw, (1.0 - ny) / 2.0 * fh))
    });
    if corners.iter().all(Option::is_some) {
        let pts = corners.map(|c| c.expect("all in front"));
        let lo = |f: fn(&(f64, f64)) -> f64| pts.iter().map(f).fold(f64::MAX, f64::min) - 1.0;
        let hi = |f: fn(&(f64, f64)) -> f64| pts.iter().map(f).fold(f64::MIN, f64::max) + 2.0;
        let fit = |v: f64, n: u32| v.clamp(0.0, f64::from(n)) as u32;
        x0 = x0.max(fit(lo(|p| p.0), w));
        y0 = y0.max(fit(lo(|p| p.1), hh));
        x1 = x1.min(fit(hi(|p| p.0), w));
        y1 = y1.min(fit(hi(|p| p.1), hh));
    }
    (x0, y0, x1, y1)
}

/// The homography LMATRIX's 4x4 matrix `m` makes of a layer's plane:
/// from the layer's (x, y, 1) to clip space's (X, Y, W).
fn homography(m: &[f64; 16]) -> [[f64; 3]; 3] {
    [
        [m[0], m[4], m[12]],
        [m[1], m[5], m[13]],
        [m[3], m[7], m[15]],
    ]
}

/// The row of LMATRIX's 4x4 matrix `m` that gives a layer point's Z from
/// (x, y, 1). With (gx, gy, q) the point a screen position maps back to
/// through the inverse homography, q is 1 / W, so that this row times
/// them is Z / W, which runs evenly across the screen.
fn depth_row(m: &[f64; 16]) -> [f64; 3] {
    [m[2], m[6], m[14]]
}

/// The pixels `from..to` of a row of `n` where Z / W is from -1 to 1, as
/// `at + step * t` at pixel t.
fn depth_span(at: f64, step: f64, n: u32) -> (u32, u32) {
    if step == 0.0 {
        return if at.abs() <= 1.0 { (0, n) } else { (0, 0) };
    }
    let (a, b) = ((-1.0 - at) / step, (1.0 - at) / step);
    let (lo, hi) = (a.min(b), a.max(b));
    if lo.is_nan() || hi.is_nan() {
        return (0, 0);
    }
    // Far past the row's ends these saturate.
    let from = lo.ceil().max(0.0) as u32;
    let to = (hi.floor() + 1.0).max(0.0) as u32;
    let to = to.min(n);
    (from.min(to), to)
}

/// Where a layer's point `(x, y)` goes on a `w` x `hh` frame through
/// LMATRIX's 4x4 matrix `m`, as [`project`] puts it, or `None` when it
/// is not in front.
pub fn project_point(m: &[f64; 16], (x, y): (f64, f64), (w, hh): (u32, u32)) -> Option<(f64, f64)> {
    let h = homography(m);
    let at = |r: usize| h[r][0] * x + h[r][1] * y + h[r][2];
    let q = at(2);
    if q.partial_cmp(&0.0) != Some(Ordering::Greater) {
        return None;
    }
    let (nx, ny) = (at(0) / q, at(1) / q);
    Some((
        (nx + 1.0) / 2.0 * f64::from(w),
        (1.0 - ny) / 2.0 * f64::from(hh),
    ))
}

/// Draws a `w` x `h` image as [`draw_image`] does, but on a layer with
/// LMATRIX's 4x4 matrix `m`, as [`project`] puts one: `place` takes the
/// image to the layer drawn flat, `sample` frame pixels to one of the
/// layer's. Unlike drawing the layer flat first, this keeps what lies off
/// the flat screen, as a sprite's corners do on SmileBASIC 4's GPU.
#[allow(clippy::too_many_arguments)]
pub fn project_image<P: Paint>(
    dst: &mut Framebuffer,
    m: &[f64; 16],
    sample: u32,
    place: &Placement,
    w: i32,
    h: i32,
    pixel: impl Fn(i32, i32) -> Option<P>,
) {
    let hm = homography(m);
    let (Some(inv), Some(back)) = (invert3(&hm), place.matrix.inverse()) else {
        return;
    };
    let z = depth_row(m);
    let s = f64::from(sample);
    let corners = [(0, 0), (w, 0), (0, h), (w, h)].map(|(x, y)| {
        let (x, y) = place.matrix.apply(f64::from(x), f64::from(y));
        (x / s, y / s)
    });
    let (fw, fh) = (f64::from(dst.width()), f64::from(dst.height()));
    let (x0, y0, x1, y1) = window(&hm, corners, (dst.width(), dst.height()), place.clip);
    // From the layer's points straight to the image's.
    let back = Affine {
        a: back.a * s,
        b: back.b * s,
        c: back.c * s,
        d: back.d * s,
        ..back
    };
    let width = dst.width() as usize;
    let pixels = dst.pixels_mut();
    let step = 2.0 / fw;
    let (iw, ih) = (f64::from(w), f64::from(h));
    let n = f64::from(x1.saturating_sub(x0));
    for py in y0..y1 {
        let ny = 1.0 - (f64::from(py) + 0.5) / fh * 2.0;
        let nx0 = (f64::from(x0) + 0.5) / fw * 2.0 - 1.0;
        let at = |r: usize, nx: f64| inv[r][0] * nx + inv[r][1] * ny + inv[r][2];
        // The layer point (gx, gy) / q, t pixels along the row.
        let (gx, gy, q) = (at(0, nx0), at(1, nx0), at(2, nx0));
        let (dx, dy, dq) = (inv[0][0] * step, inv[1][0] * step, inv[2][0] * step);
        // The image point times q is linear in t too, so where it falls
        // inside the image is found once a row, on either side of where
        // q changes sign; each pixel is still tested exactly below.
        let (ax, kx) = (
            back.a * gx + back.b * gy + back.tx * q,
            back.a * dx + back.b * dy + back.tx * dq,
        );
        let (ay, ky) = (
            back.c * gx + back.d * gy + back.ty * q,
            back.c * dx + back.d * dy + back.ty * dq,
        );
        let shown = depth_span(
            z[0] * gx + z[1] * gy + z[2] * q,
            z[0] * dx + z[1] * dy + z[2] * dq,
            x1.saturating_sub(x0),
        );
        let line = &mut pixels[py as usize * width..][..x1 as usize];
        for sign in [1.0, -1.0] {
            // Each (k, c) asks for k * t + c above 0.
            let bounds = [
                (dq, q),
                (kx, ax),
                (iw * dq - kx, iw * q - ax),
                (ky, ay),
                (ih * dq - ky, ih * q - ay),
            ];
            let (mut lo, mut hi) = (0.0, n);
            for (k, c) in bounds {
                let (k, c) = (k * sign, c * sign);
                if k == 0.0 {
                    if c < 0.0 {
                        hi = lo;
                    }
                } else if k > 0.0 {
                    lo = f64::max(lo, -c / k);
                } else {
                    hi = f64::min(hi, -c / k);
                }
            }
            if lo >= hi {
                continue;
            }
            let from = ((lo - 1.0).max(0.0) as u32).max(shown.0);
            let to = ((hi + 1.0).min(n) as u32).min(x1 - x0).min(shown.1);
            let ft = f64::from(from);
            let (mut gx, mut gy, mut qt) = (gx + dx * ft, gy + dy * ft, q + dq * ft);
            for t in from..to {
                let (lx, ly) = (gx / qt, gy / qt);
                (gx, gy, qt) = (gx + dx, gy + dy, qt + dq);
                // In front only; a NaN is not.
                if (hm[2][0] * lx + hm[2][1] * ly + hm[2][2]).partial_cmp(&0.0)
                    != Some(Ordering::Greater)
                {
                    continue;
                }
                let (ix, iy) = back.apply(lx, ly);
                let (Some(ix), Some(iy)) = (floor_in(ix, i64::from(w)), floor_in(iy, i64::from(h)))
                else {
                    continue;
                };
                if let Some(paint) = pixel(ix as i32, iy as i32) {
                    let d = &mut line[(x0 + t) as usize];
                    *d = paint.put(*d);
                }
            }
        }
    }
}

/// `v` rounded down, if that is from 0 up to `n`; without the library
/// call `f64::floor` makes on processors before SSE4.1.
#[inline]
fn floor_in(v: f64, n: i64) -> Option<usize> {
    // Also false for NaN; truncation is the floor from here on.
    if !(v >= 0.0 && v < n as f64) {
        return None;
    }
    Some(v as usize)
}

/// The smallest rectangle, inclusive, holding every pixel drawn on a
/// frame that started clear.
fn drawn_bounds(fb: &Framebuffer) -> Option<(u32, u32, u32, u32)> {
    let w = fb.width() as usize;
    if w == 0 {
        return None;
    }
    let rows: Vec<&[u32]> = fb.pixels().chunks_exact(w).collect();
    let top = rows.iter().position(|r| r.iter().any(|&p| p != 0))?;
    let bottom = rows.iter().rposition(|r| r.iter().any(|&p| p != 0))?;
    let (mut left, mut right) = (w, 0);
    for r in &rows[top..=bottom] {
        if let Some(l) = r[..left].iter().position(|&p| p != 0) {
            left = l;
        }
        if let Some(rr) = r[right..].iter().rposition(|&p| p != 0) {
            right += rr;
        }
    }
    Some((left as u32, top as u32, right as u32, bottom as u32))
}

/// The inverse of a 3x3 matrix, if it has one.
fn invert3(m: &[[f64; 3]; 3]) -> Option<[[f64; 3]; 3]> {
    let c =
        |r0: usize, c0: usize, r1: usize, c1: usize| m[r0][c0] * m[r1][c1] - m[r0][c1] * m[r1][c0];
    let det = m[0][0] * c(1, 1, 2, 2) - m[0][1] * c(1, 0, 2, 2) + m[0][2] * c(1, 0, 2, 1);
    if det == 0.0 || !det.is_finite() {
        return None;
    }
    let d = 1.0 / det;
    Some([
        [c(1, 1, 2, 2) * d, -c(0, 1, 2, 2) * d, c(0, 1, 1, 2) * d],
        [-c(1, 0, 2, 2) * d, c(0, 0, 2, 2) * d, -c(0, 0, 1, 2) * d],
        [c(1, 0, 2, 1) * d, -c(0, 0, 2, 1) * d, c(0, 0, 1, 1) * d],
    ])
}

/// A premultiplied color over another, each channel held to 255 (an
/// additive element can make a layer's light more than its coverage).
#[inline]
fn over_premultiplied(dst: u32, src: u32) -> u32 {
    let a = u64::from(src >> 24);
    if a == 0xFF {
        return src;
    }
    held(spread(src) + div255_lanes(spread(dst) * (255 - a)))
}

/// How LAYER puts a layer over the ones behind it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Compose {
    /// The layer's elements are drawn straight onto the frame.
    #[default]
    Direct,
    Normal,
    Add,
    Multiply,
    Screen,
}

impl Compose {
    pub fn from_int(n: i32) -> Option<Compose> {
        Some(match n {
            0 => Compose::Direct,
            1 => Compose::Normal,
            2 => Compose::Add,
            3 => Compose::Multiply,
            4 => Compose::Screen,
            _ => return None,
        })
    }

    pub fn to_int(self) -> i32 {
        self as i32
    }
}

/// `x / 255`, rounded to the nearest.
fn div255(x: u32) -> u32 {
    (x + 127) / 255
}

/// Puts a layer drawn on its own (transparent, premultiplied) frame over
/// `dst`, its colors first multiplied by `color`. Each mode is one
/// rounding of the exact sum, as in [`compose_tinted`]; the frame's alpha
/// (coverage) grows by the layer's in mode 1, stays in mode 2 and becomes
/// opaque in modes 3 and 4. `opaque` says `dst` is opaque all over, as
/// the main screen is, which spares that last step.
pub fn compose(
    dst: &mut Framebuffer,
    layer: &Framebuffer,
    mode: Compose,
    color: u32,
    opaque: bool,
) {
    if color != 0xFFFF_FFFF {
        return compose_tinted(dst, layer, mode, color);
    }
    match mode {
        // The usual case, a plain layer, as one blend a pixel.
        Compose::Direct | Compose::Normal => {
            for (d, &l) in dst.pixels_mut().iter_mut().zip(layer.pixels()) {
                if l != 0 {
                    *d = over_premultiplied(*d, l);
                }
            }
        }
        Compose::Add => {
            // Each channel added and held to 255 is a byte's saturating
            // add, which the compiler does many pixels at a time; adding
            // nothing where the layer is clear, and to the alpha, changes
            // nothing.
            for (d, &l) in dst.pixels_mut().iter_mut().zip(layer.pixels()) {
                let (a, b) = (d.to_le_bytes(), (l & 0x00FF_FFFF).to_le_bytes());
                *d = u32::from_le_bytes(std::array::from_fn(|i| a[i].saturating_add(b[i])));
            }
        }
        // Each mode gets its own loop, so that its sum is worked inline.
        Compose::Multiply if opaque => {
            compose_each::<true>(dst, layer, |p, d, a| div255(d * (p + 255 - a)))
        }
        Compose::Multiply => compose_each::<false>(dst, layer, |p, d, a| div255(d * (p + 255 - a))),
        Compose::Screen if opaque => {
            compose_each::<true>(dst, layer, |p, d, _| d + div255(p * (255 - d)))
        }
        Compose::Screen => compose_each::<false>(dst, layer, |p, d, _| d + div255(p * (255 - d))),
    }
}

/// [`compose`] without a layer color, for modes 3 and 4, by `f`, which
/// gives a channel from the layer's light `p`, the frame's `d` and the
/// layer's coverage `a`; it is held to 255. `OPAQUE` is [`compose`]'s
/// `opaque`.
#[inline(always)]
fn compose_each<const OPAQUE: bool>(
    dst: &mut Framebuffer,
    layer: &Framebuffer,
    f: impl Fn(u32, u32, u32) -> u32,
) {
    for (d, &l) in dst.pixels_mut().iter_mut().zip(layer.pixels()) {
        // A clear pixel of the layer leaves the color as it is, but
        // the frame becomes opaque.
        // UNVERIFIED: that this holds over the whole screen, not only
        // where the layer has something.
        if l == 0 {
            if !OPAQUE {
                *d |= 0xFF00_0000;
            }
            continue;
        }
        let [a, pr, pg, pb] = l.to_be_bytes().map(u32::from);
        let [_, dr, dg, db] = d.to_be_bytes().map(u32::from);
        let c = |p: u32, d: u32| f(p, d, a).min(255) as u8;
        *d = u32::from_be_bytes([0xFF, c(pr, dr), c(pg, dg), c(pb, db)]);
    }
}

/// [`compose`] the general way, for any layer color, even white. With
/// the layer's premultiplied light `p` and coverage `a`, the frame's `d`
/// and the color's `c` and alpha `ca`, the layer gives the light
/// `P = p*c*ca` and the coverage `A = a*ca` (over 255²), and each mode
/// rounds once:
/// 1: `P + d*(1-A)`; 2: `d + P`; 3: `d*(P + 1-A)`; 4: `d + P*(1-d)`.
#[inline(never)]
fn compose_tinted(dst: &mut Framebuffer, layer: &Framebuffer, mode: Compose, color: u32) {
    const N: u32 = 255 * 255;
    const N3: u64 = 255 * 255 * 255;
    let [ca, cr, cg, cb] = color.to_be_bytes().map(u32::from);
    let opaque = matches!(mode, Compose::Multiply | Compose::Screen);
    for (d, &l) in dst.pixels_mut().iter_mut().zip(layer.pixels()) {
        // Where the layer is clear, only the alpha can change.
        if l == 0 {
            if opaque {
                *d |= 0xFF00_0000;
            }
            continue;
        }
        let [la, lr, lg, lb] = l.to_be_bytes().map(u32::from);
        let a = la * ca;
        let [da, dr, dg, db] = d.to_be_bytes().map(u32::from);
        let lights = [lr * cr * ca, lg * cg * ca, lb * cb * ca];
        let [r, g, b] = [(lights[0], dr), (lights[1], dg), (lights[2], db)].map(|(p, d)| {
            let v = match mode {
                Compose::Direct | Compose::Normal => (p + d * (N - a) + N / 2) / N,
                Compose::Add => d + (p + N / 2) / N,
                Compose::Multiply => {
                    let x = u64::from(d) * u64::from(p + 255 * (N - a));
                    ((x + N3 / 2) / N3) as u32
                }
                Compose::Screen => {
                    let x = u64::from(p) * u64::from(255 - d);
                    d + ((x + N3 / 2) / N3) as u32
                }
            };
            v.min(255)
        });
        let alpha = match mode {
            Compose::Direct | Compose::Normal => (da * (N - a) + 255 * a + N / 2) / N,
            Compose::Add => da,
            // UNVERIFIED: as in compose_each, over the whole screen.
            Compose::Multiply | Compose::Screen => 255,
        };
        *d = alpha << 24 | r << 16 | g << 8 | b;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: (f64, f64), b: (f64, f64)) -> bool {
        (a.0 - b.0).abs() < 1e-9 && (a.1 - b.1).abs() < 1e-9
    }

    #[test]
    fn projecting_points() {
        // Layer pixels to clip space on a 400x240 frame, with W = 1 + x/400
        // to put points farther away the further right they are.
        let mut m = [0.0; 16];
        (m[0], m[12]) = (2.0 / 400.0, -1.0);
        (m[5], m[13]) = (-2.0 / 240.0, 1.0);
        (m[3], m[15]) = (1.0 / 400.0, 1.0);
        let frame = (400, 240);
        assert_eq!(project_point(&m, (0.0, 0.0), frame), Some((0.0, 0.0)));
        let p = project_point(&m, (400.0, 240.0), frame).unwrap();
        assert!(close(p, (300.0, 180.0)), "{p:?}");
        assert_eq!(project_point(&m, (-400.0, 0.0), frame), None);
    }

    #[test]
    fn placing_and_inverting() {
        let m = Affine::place((100.0, 50.0), (8.0, 8.0), 90.0, (2.0, 1.0));
        assert!(close(m.apply(8.0, 8.0), (100.0, 50.0)));
        // One pixel right of home, doubled, then turned to point down.
        assert!(close(m.apply(9.0, 8.0), (100.0, 52.0)));
        let back = m.inverse().unwrap();
        assert!(close(back.apply(100.0, 52.0), (9.0, 8.0)));
        let shift = Affine::place((10.0, 0.0), (0.0, 0.0), 0.0, (1.0, 1.0));
        assert!(close(m.then(&shift).apply(8.0, 8.0), (110.0, 50.0)));
        assert_eq!(
            Affine::place((0.0, 0.0), (0.0, 0.0), 0.0, (0.0, 1.0)).inverse(),
            None
        );
    }

    /// A xorshift generator, for inputs the tests try.
    fn numbers(mut seed: u32) -> impl FnMut() -> u32 {
        move || {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            seed
        }
    }

    /// `n / d` rounded to the nearest, `d` odd so that there are no ties.
    fn nearest(n: u128, d: u128) -> u32 {
        ((2 * n + d) / (2 * d)) as u32
    }

    /// An element's straight color `src` tinted by `c1` and `c2` over the
    /// premultiplied `dst`, or added to it, worked out exactly in the
    /// plainest way and rounded once: each 8-bit value is v/255.
    fn reference(dst: u32, src: u32, c1: u32, c2: u32, add: bool) -> u32 {
        let ch = |v: u32, i: usize| u128::from(v >> (8 * i) & 0xFF);
        let d5 = 255u128.pow(5);
        // a = sa*c1a*c2a / 255³; light = s*c1*c2*a / 255³ (in 8-bit units,
        // times 255 / 255).
        let a = ch(src, 3) * ch(c1, 3) * ch(c2, 3);
        let mut out = 0;
        for i in 0..4 {
            let d = ch(dst, i);
            let v = match (i, add) {
                (3, true) => d as u32,
                (_, true) => {
                    let light = ch(src, i) * ch(c1, i) * ch(c2, i) * a;
                    nearest(d * d5 + light, d5).min(255)
                }
                // Coverage: d + (1 - d) * a, in 8-bit units.
                (3, false) => nearest(d * d5 + (255 - d) * a * 255 * 255, d5),
                (_, false) => {
                    let light = ch(src, i) * ch(c1, i) * ch(c2, i) * a;
                    nearest(light + d * 255 * 255 * (255u128.pow(3) - a), d5)
                }
            };
            out |= v << (8 * i);
        }
        out
    }

    #[test]
    fn paints_round_the_exact_value_once() {
        let mut next = numbers(0x1234_5678);
        let white = 0xFFFF_FFFF;
        // Random colors, with alphas at the edges more often.
        let mut color = || {
            let c = next();
            match c & 7 {
                0 => c | 0xFF00_0000,
                1 => c & 0x00FF_FFFF | 0x0100_0000,
                2 => white,
                _ => c,
            }
        };
        for _ in 0..200_000 {
            let (dst, src, c1, c2) = (color(), color(), color(), color());
            if src >> 24 == 0 {
                continue;
            }
            let all = |c1: u32, c2: u32, add: bool| reference(dst, src, c1, c2, add);
            assert_eq!(Over1::new(src).put(dst), all(white, white, false));
            assert_eq!(over(dst, src), all(white, white, false));
            assert_eq!(Lit::new(src).put(dst), all(white, white, true));
            assert_eq!(add(dst, src), all(white, white, true));
            assert_eq!(Over3::new(src, c1).put(dst), all(c1, white, false));
            assert_eq!(Lit::tinted(src, c1).put(dst), all(c1, white, true));
            assert_eq!(Over5::new(src, c1, c2).put(dst), all(c1, c2, false));
            assert_eq!(Lit::tinted2(src, c1, c2).put(dst), all(c1, c2, true));
            for add in [false, true] {
                let mixed = Mixed::new(src, c1, c2, add);
                let expected = all(c1, c2, add);
                match mixed {
                    Some(m) => assert_eq!(m.put(dst), expected, "{dst:08X} {src:08X}"),
                    None => assert_eq!(dst, expected),
                }
            }
        }
        // Every alpha and level over every level, in one channel each.
        for sa in 1..=255u32 {
            for s in 0..=255u32 {
                for d in (0..=255u32).step_by(5).chain([254, 255]) {
                    let src = sa << 24 | s << 16 | (255 - s) << 8 | s;
                    let dst = 0x8000_0000 | d << 16 | (255 - d) << 8 | (d / 2);
                    assert_eq!(over(dst, src), reference(dst, src, white, white, false));
                    assert_eq!(add(dst, src), reference(dst, src, white, white, true));
                }
            }
        }
    }

    #[test]
    fn over_and_add() {
        assert_eq!(over(0xFF00_0000, 0x80FF_FFFF), 0xFF80_8080);
        assert_eq!(over(0, 0x80FF_0000), 0x8080_0000);
        assert_eq!(add(0xFF80_0000, 0x80FF_0010), 0xFFFF_0008);
        // Light without coverage.
        assert_eq!(add(0, 0x8080_8080), 0x0040_4040);
        // &H80 times &H7F is 63.75 and rounds up, only once.
        assert_eq!(
            Over3::new(0xFF80_8080, 0xFF7F_7F7F).put(0xFF00_0000),
            0xFF40_4040
        );
        // Tinted twice: 128 * 127 * 127 / 255² = 31.75.
        let twice = Over5::new(0xFF80_8080, 0xFF7F_7F7F, 0xFF7F_7F7F);
        assert_eq!(twice.put(0xFF00_0000), 0xFF20_2020);
        assert_eq!(twice.solid(), Some(0xFF20_2020));
    }

    /// A layer of premultiplied colors over a frame of any, with some
    /// pixels' light above their coverage, as additive elements leave.
    fn layer_and_frame(seed: u32) -> (Framebuffer, Framebuffer) {
        let mut next = numbers(seed);
        let mut frame = Framebuffer::new(64, 64);
        let mut layer = Framebuffer::new(64, 64);
        for (d, l) in frame.pixels_mut().iter_mut().zip(layer.pixels_mut()) {
            *d = next();
            let a = match next() % 8 {
                0 => 0,
                1 => 255,
                _ => next() >> 24,
            };
            let lit = next().is_multiple_of(8);
            let [_, r, g, b] = next().to_be_bytes().map(|c| {
                if lit {
                    c
                } else {
                    div255(u32::from(c) * a) as u8
                }
            });
            *l = u32::from_be_bytes([a as u8, r, g, b]);
        }
        (frame, layer)
    }

    #[test]
    fn every_mode_without_color_matches_the_general_way() {
        let (frame, layer) = layer_and_frame(0x51ED_270B);
        for mode in [
            Compose::Direct,
            Compose::Normal,
            Compose::Add,
            Compose::Multiply,
            Compose::Screen,
        ] {
            let mut fast = frame.clone();
            compose(&mut fast, &layer, mode, 0xFFFF_FFFF, false);
            let mut slow = frame.clone();
            compose_tinted(&mut slow, &layer, mode, 0xFFFF_FFFF);
            assert_eq!(fast.pixels(), slow.pixels(), "{mode:?}");
        }
    }

    #[test]
    fn layer_colors_round_once() {
        let (frame, layer) = layer_and_frame(0x9E37_79B9);
        let ch = |v: u32, i: usize| u128::from(v >> (8 * i) & 0xFF);
        let n3 = 255u128.pow(3);
        for color in [
            0xC080_6040,
            0x7F40_C0FF,
            0xFFC0_8040,
            0x80FF_FFFF,
            0x0100_0000,
        ] {
            for mode in [
                Compose::Normal,
                Compose::Add,
                Compose::Multiply,
                Compose::Screen,
            ] {
                let mut out = frame.clone();
                compose(&mut out, &layer, mode, color, false);
                for ((&got, &d), &l) in out.pixels().iter().zip(frame.pixels()).zip(layer.pixels())
                {
                    // In 8-bit units over 255³: the layer's light P, its
                    // coverage A, and 1.
                    let a = ch(l, 3) * ch(color, 3) * 255;
                    let mut expected = 0;
                    for i in 0..3 {
                        let p = ch(l, i) * ch(color, i) * ch(color, 3) * 255;
                        let d = ch(d, i);
                        let v = match mode {
                            Compose::Normal => nearest(p * 255 + d * (n3 - a) * 255, n3 * 255),
                            Compose::Add => nearest(d * n3 + p, n3),
                            Compose::Multiply => nearest(d * (p + 255 * (n3 - a)), n3 * 255),
                            _ => nearest(d * n3 * 255 + p * (255 - d), n3 * 255),
                        };
                        expected |= v.min(255) << (8 * i);
                    }
                    let alpha = match mode {
                        Compose::Normal => {
                            let da = ch(d, 3);
                            nearest(da * (n3 - a) + 255 * a, n3)
                        }
                        Compose::Add => d >> 24,
                        _ => 255,
                    };
                    expected |= alpha << 24;
                    assert_eq!(got, expected, "{mode:?} {color:08X} {d:08X} {l:08X}");
                }
            }
        }
    }

    #[test]
    fn images_clip() {
        let mut fb = Framebuffer::new(8, 8);
        let clip = Clip {
            x0: 2,
            y0: 0,
            x1: 3,
            y1: 7,
        };
        let place = Placement {
            matrix: Affine::IDENTITY,
            clip: Some(&clip),
            add: false,
        };
        draw_image(&mut fb, &place, 8, 1, |_, _| Some(Over1::new(0xFFFF_FFFF)));
        let row: Vec<u32> = (0..5).map(|x| fb.get(x, 0).unwrap()).collect();
        assert_eq!(row, [0xFF00_0000, 0xFF00_0000, !0, !0, 0xFF00_0000]);
    }

    #[test]
    fn ties_settle_along_the_image_or_the_frame() {
        let tie = |rot, scale, size, reversed| {
            let place = Placement {
                matrix: Affine::place((100.5, 50.5), (0.0, 0.0), rot, (scale, scale)),
                clip: None,
                add: false,
            };
            let (x, y) = place.texel_tie((size, size), reversed);
            // What matters is the sign of what the point ends up moved by
            // from where it was before covering was settled.
            let inv = place.matrix.inverse().unwrap();
            let (nx, ny) = COVER_NUDGE;
            let (mx, my) = (inv.a * nx + inv.b * ny, inv.c * nx + inv.d * ny);
            ((x + mx).signum() as i32, (y + my).signum() as i32)
        };
        let none = (false, false);
        // A size that halves evenly at a scale that does: plain floor.
        assert_eq!(tie(0.0, 0.5, 16, none), (1, 1));
        assert_eq!(tie(90.0, 1.0, 32, none), (1, 1));
        // At other scales: towards the frame's left and bottom.
        assert_eq!(tie(0.0, 1.5, 16, none), (-1, 1));
        assert_eq!(tie(180.0, 2.5, 16, none), (1, -1));
        assert_eq!(tie(90.0, 1.5, 16, none), (1, 1));
        assert_eq!(tie(270.0, 2.5, 16, none), (-1, -1));
        // Other sizes: towards the page's lower coordinates.
        assert_eq!(tie(0.0, 1.5, 15, none), (-1, -1));
        assert_eq!(tie(0.0, 1.0, 15, (true, false)), (1, -1));
        // Neither upright nor at a quarter turn.
        assert_eq!(tie(45.0, 1.0, 15, none), (1, -1));
    }

    #[test]
    fn a_point_on_an_edge_goes_with_the_left_and_bottom() {
        let mut fb = Framebuffer::new(8, 8);
        fb.clear(0xFF00_0000);
        let place = Placement {
            matrix: Affine::place((1.5, 1.5), (0.0, 0.0), 0.0, (1.0, 1.0)),
            clip: None,
            add: false,
        };
        draw_image(&mut fb, &place, 2, 2, |_, _| Some(Over1::new(!0)));
        let drawn = |x, y| fb.get(x, y) == Some(!0);
        // The image runs from 1.5 to 3.5 both ways; pixel centers are on
        // those.
        assert!(!drawn(1, 1), "top edge");
        assert!(drawn(1, 2) && drawn(1, 3), "left and bottom edges");
        assert!(!drawn(3, 2), "right edge");
        assert!(drawn(2, 3));
        assert!(!drawn(2, 4));
    }

    #[test]
    fn a_turned_image_keeps_every_row() {
        // Float noise in the matrix once lost a whole row here.
        for (rot, rows) in [(90.0, 22 * 22), (180.0, 22 * 23), (270.0, 23 * 23)] {
            let mut fb = Framebuffer::new(64, 64);
            fb.clear(0xFF00_0000);
            let place = Placement {
                matrix: Affine::place((30.5, 30.5), (0.0, 0.0), rot, (1.5, 1.5)),
                clip: None,
                add: false,
            };
            draw_image(&mut fb, &place, 15, 15, |_, _| Some(Over1::new(!0)));
            let n = (0..64 * 64)
                .filter(|i| fb.get(i % 64, i / 64) == Some(!0))
                .count();
            assert_eq!(n, rows, "{rot}");
        }
    }

    #[test]
    fn source_bounds_stop_at_the_horizon() {
        let mut m = [0.0; 16];
        (m[0], m[5], m[10], m[15]) = (1.0, 1.0, 1.0, 1.0);
        // The identity shows clip space, -1 to 1 each way.
        let b = source_bounds(&m, 1, (8, 8), None).unwrap();
        assert_eq!(b, (-1.0, -1.0, 1.0, 1.0));
        // Tilted so that W is 2y + 1: the horizon crosses the screen a
        // quarter of the way down, and below it the layer shows from
        // y = -1/3 at the bottom edge to very far up.
        m[7] = 2.0;
        let (x0, y0, x1, y1) = source_bounds(&m, 1, (8, 8), None).unwrap();
        assert!((y0 + 1.0 / 3.0).abs() < 1e-9, "{y0}");
        assert!(y1 > 1e4 && x0 < -1e4 && x1 > 1e4, "{x0} {x1} {y1}");
        // Turned away altogether, nothing shows.
        m[15] = -1.0;
        m[7] = 0.0;
        assert_eq!(source_bounds(&m, 1, (8, 8), None), None);
    }

    #[test]
    fn projection_clips_by_depth() {
        // An 8x2 frame shows the layer as it is, with Z / W running from
        // -1.5 to 2.5 across it.
        let mut m = [0.0; 16];
        (m[0], m[5], m[15]) = (0.25, 1.0, 1.0);
        (m[12], m[2], m[14]) = (-1.0, 0.5, -1.5);
        let mut src = Framebuffer::new(8, 2);
        src.pixels_mut().fill(0xFF11_2233);
        let mut dst = Framebuffer::new(8, 2);
        dst.clear(0);
        project(&mut dst, &src, (0, 0), &m, 1, None);
        // Z at each pixel's center, x + 0.5, is (x + 0.5) / 2 - 1.5.
        let row: Vec<bool> = dst.pixels()[..8].iter().map(|&p| p != 0).collect();
        let want = [false, true, true, true, true, false, false, false];
        assert_eq!(row, want);
        let mut image = Framebuffer::new(8, 2);
        image.clear(0);
        project_image(
            &mut image,
            &m,
            1,
            &Placement {
                matrix: Affine::IDENTITY,
                clip: None,
                add: false,
            },
            8,
            2,
            |_, _| Some(Over1::new(0xFF11_2233)),
        );
        let row: Vec<bool> = image.pixels()[..8].iter().map(|&p| p != 0).collect();
        assert_eq!(row, want);
    }

    #[test]
    fn projection_shows_clip_space() {
        // The identity puts the layer's first unit square on the top right
        // quarter of the screen, as SmileBASIC 4 does.
        let mut src = Framebuffer::new(8, 8);
        src.clear(0);
        src.set(0, 0, 0xFFFF_0000);
        let mut dst = Framebuffer::new(8, 8);
        let mut m = [0.0; 16];
        (m[0], m[5], m[10], m[15]) = (1.0, 1.0, 1.0, 1.0);
        project(&mut dst, &src, (0, 0), &m, 1, None);
        assert_eq!(dst.get(6, 1), Some(0xFFFF_0000));
        assert_eq!(dst.get(1, 1), Some(0xFF00_0000));
        assert_eq!(dst.get(6, 6), Some(0xFF00_0000));
        assert_eq!(over_premultiplied(0xFF00_0000, 0x8080_0000), 0xFF80_0000);
    }

    #[test]
    fn layers_compose() {
        let mut dst = Framebuffer::new(1, 1);
        dst.clear(0xFF80_8080);
        let mut layer = Framebuffer::new(1, 1);
        layer.clear(0xFFFF_0000);
        let mut m = dst.clone();
        compose(&mut m, &layer, Compose::Multiply, !0, true);
        assert_eq!(m.get(0, 0), Some(0xFF80_0000));
        let mut s = dst.clone();
        compose(&mut s, &layer, Compose::Screen, !0, true);
        assert_eq!(s.get(0, 0), Some(0xFFFF_8080));
        let mut tinted = dst.clone();
        compose(&mut tinted, &layer, Compose::Normal, 0xFF00_FFFF, true);
        assert_eq!(tinted.get(0, 0), Some(0xFF00_0000));
    }
}
