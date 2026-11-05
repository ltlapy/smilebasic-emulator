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
    src: u32,
    keep: u64,
}

impl Over1 {
    #[inline]
    pub fn new(src: u32) -> Over1 {
        Over1 {
            src,
            keep: 255 - u64::from(src >> 24),
        }
    }
}

impl Paint for Over1 {
    #[inline]
    fn put(&self, dst: u32) -> u32 {
        // Most pixels of most images are opaque.
        if self.keep == 0 {
            return self.src;
        }
        // All four channels at once, 16 bits apiece: each sum is at most
        // 255 * 255, so none carries into the next.
        let light = spread(self.src | 0xFF00_0000) * (255 - self.keep);
        gather(div255_lanes(light + spread(dst) * self.keep))
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
    sample: impl FnMut(f64, f64) -> Option<P>,
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
    mut sample: impl FnMut(f64, f64) -> Option<P>,
) {
    let Some(inv) = place.sampling() else {
        return;
    };
    let Some((x0, y0, x1, y1)) = span(fb, place, w, h, part) else {
        return;
    };
    let (w, h) = (f64::from(w), f64::from(h));
    let (part_x, part_y) = (
        (f64::from(part.0), f64::from(part.2)),
        (f64::from(part.1), f64::from(part.3)),
    );
    let width = fb.width() as usize;
    let pixels = fb.pixels_mut();
    for py in y0..y1 {
        let line = &mut pixels[py as usize * width..][..x1 as usize];
        let fy = f64::from(py) + 0.5;
        let (from, to) = columns(&inv, fy, part_x, part_y, x0, x1);
        for px in from..to {
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

/// [`draw_image`] for what reads whole texels: `pixel` gets the point under
/// each frame pixel's center, moved by `tie` from [`Placement::texel_tie`],
/// rounded down. That is on the image, or one off it by a tie. Most pixels
/// take no floating point, which a small CPU feels most: the point is stepped
/// along a row in fixed point, and only where that lies so near a whole number
/// that it might round the other way is it worked out as `draw_image` does, so
/// the same texels are read.
pub fn draw_texels<P: Paint>(
    fb: &mut Framebuffer,
    place: &Placement,
    w: i32,
    h: i32,
    (tx, ty): (f64, f64),
    mut pixel: impl FnMut(i32, i32) -> Option<P>,
) {
    /// One texel, in fixed point.
    const ONE: f64 = (1u64 << 40) as f64;
    /// The fraction of a fixed point number.
    const FRACTION: i64 = (1 << 40) - 1;
    /// How near a whole number the fixed point may be off from the exact sum:
    /// far more than the steps of a frame's width at one unit each, and than the
    /// exact sum's rounding within the limit below. A tie adds to it.
    const NEAR: i64 = 1 << 12;
    /// Above this, for any term of a sum, the sum's rounding is not small
    /// enough, or fixed point not large enough.
    const LIMIT: f64 = 65536.0;
    let Some(inv) = place.sampling() else {
        return;
    };
    let width = f64::from(fb.width());
    let height = f64::from(fb.height());
    let in_limit = inv.a.abs() * width < LIMIT
        && inv.c.abs() * width < LIMIT
        && inv.b.abs() * height < LIMIT
        && inv.d.abs() * height < LIMIT
        && inv.tx.abs() < LIMIT
        && inv.ty.abs() < LIMIT
        && w < 65536
        && h < 65536
        && width < 3000.0
        && tx.abs() < 1e-6
        && ty.abs() < 1e-6;
    let whole = |v: f64| v as i32 - i32::from(v < 0.0);
    if !in_limit {
        return draw_image(fb, place, w, h, |x, y| pixel(whole(x + tx), whole(y + ty)));
    }
    let near = NEAR + (tx.abs().max(ty.abs()) * ONE) as i64;
    let Some((x0, y0, x1, y1)) = span(fb, place, w, h, (0, 0, w, h)) else {
        return;
    };
    let (fw, fh) = (f64::from(w), f64::from(h));
    let (dx, dy) = ((inv.a * ONE) as i64, (inv.c * ONE) as i64);
    let stride = fb.width() as usize;
    let pixels = fb.pixels_mut();
    for py in y0..y1 {
        let line = &mut pixels[py as usize * stride..][..x1 as usize];
        let fy = f64::from(py) + 0.5;
        let (from, to) = columns(&inv, fy, (0.0, fw), (0.0, fh), x0, x1);
        if from >= to {
            continue;
        }
        let (lx, ly) = inv.apply(f64::from(from) + 0.5, fy);
        let (mut gx, mut gy) = ((lx * ONE) as i64, (ly * ONE) as i64);
        for (px, dst) in (from..to).zip(&mut line[from as usize..to as usize]) {
            let (x, y) = (gx >> 40, gy >> 40);
            let exact = ((gx + near) & FRACTION) < 2 * near || ((gy + near) & FRACTION) < 2 * near;
            (gx, gy) = (gx + dx, gy + dy);
            let at = if exact {
                let (lx, ly) = inv.apply(f64::from(px) + 0.5, fy);
                (lx >= 0.0 && ly >= 0.0 && lx < fw && ly < fh)
                    .then(|| (whole(lx + tx), whole(ly + ty)))
            } else {
                // Also false for a negative one.
                ((x as u64) < w as u64 && (y as u64) < h as u64).then_some((x as i32, y as i32))
            };
            if let Some((x, y)) = at
                && let Some(paint) = pixel(x, y)
            {
                *dst = paint.put(*dst);
            }
        }
    }
}

/// The columns of the frame's row with centers at `fy`, from `x0` up to but not
/// including `x1`, that may lie within the given ranges of the image's x and
/// y under `inv`: only those near where the row crosses them need trying, each
/// being tested exactly after this.
#[inline]
fn columns(inv: &Affine, fy: f64, xs: (f64, f64), ys: (f64, f64), x0: u32, x1: u32) -> (u32, u32) {
    let (from, to) = crossing(inv.a, inv.b * fy + inv.tx, xs)
        .and_then(|(f, t)| {
            let (g, u) = crossing(inv.c, inv.d * fy + inv.ty, ys)?;
            Some((f.max(g), t.min(u)))
        })
        .map_or((1.0, 0.0), |(f, t)| (f - 1.5, t + 0.5));
    let from = (from.max(f64::from(x0)) as u32).max(x0);
    let to = if to < f64::from(x1) {
        (to.max(0.0) as u32) + 1
    } else {
        x1
    };
    (from, to.min(x1))
}

/// Where `k * x + base` lies from `lo` up to `hi`: the range of x, which
/// may be empty; `None` if it never does.
fn crossing(k: f64, base: f64, (lo, hi): (f64, f64)) -> Option<(f64, f64)> {
    if k == 0.0 {
        return (lo..hi).contains(&base).then_some((f64::MIN, f64::MAX));
    }
    let (a, b) = ((lo - base) / k, (hi - base) / k);
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
    /// `size` whose page coordinates run along the image as `page` says, from
    /// [`page_signs`]. An image side and a scale that are both powers of two
    /// make the arithmetic exact, and a tie goes to the page's higher texel;
    /// anything else, to the lower. The point may land just off the image,
    /// which reads the page there. Exactly zero where nothing can tie, so that
    /// callers may leave out the adding.
    pub fn texel_tie(&self, size: (i32, i32), page: (f64, f64)) -> (f64, f64) {
        let m = &self.matrix;
        let (nx, ny) = COVER_NUDGE;
        let Some(inv) = m.inverse() else {
            return (0.0, 0.0);
        };
        let moved = (inv.a * nx + inv.b * ny, inv.c * nx + inv.d * ny);
        let along = |tie: (f64, f64)| (tie.0 - moved.0, tie.1 - moved.1);
        let (sx, sy) = (m.a.abs() + m.c.abs(), m.b.abs() + m.d.abs());
        if !self.is_right_angle() {
            // Only at multiples of 45 degrees can a center land exactly
            // between texels at all.
            let even = |p: f64, q: f64| (p.abs() - q.abs()).abs() <= 1e-9 * (p.abs() + q.abs());
            return match even(m.a, m.c) || even(m.b, m.d) {
                true => along((TIE, -TIE)),
                false => (0.0, 0.0),
            };
        }
        let axis = |size: i32, scale: f64, page: f64| match size.unsigned_abs().is_power_of_two()
            && is_power_of_two(scale)
        {
            true => TIE * page,
            false => -TIE * page,
        };
        along((axis(size.0, sx, page.0), axis(size.1, sy, page.1)))
    }

    /// Whether the image's axes lie along the frame's, in either order.
    pub fn is_right_angle(&self) -> bool {
        let m = &self.matrix;
        let none_of = |p: f64, q: f64| p.abs() <= 1e-9 * q.abs();
        (none_of(m.a, m.c) && none_of(m.d, m.b)) || (none_of(m.c, m.a) && none_of(m.b, m.d))
    }

    /// Along which side of each image axis [`Placement::texel_tie`] settles
    /// a tie, as -1 or 1, for a rasterizer that samples in its own way; 0 for
    /// both where nothing can tie. Inline, so that a headless build has no
    /// call to it, which changes what the compiler inlines on the CPU's paths
    /// (0.4% of TMULTI's instructions, measured).
    #[inline]
    pub fn tie_sides(&self, size: (i32, i32), page: (f64, f64)) -> (f64, f64) {
        let tie = self.texel_tie(size, page);
        let Some(inv) = self.matrix.inverse().filter(|_| tie != (0.0, 0.0)) else {
            return (0.0, 0.0);
        };
        let (nx, ny) = COVER_NUDGE;
        let moved = (inv.a * nx + inv.b * ny, inv.c * nx + inv.d * ny);
        ((tie.0 + moved.0).signum(), (tie.1 + moved.1).signum())
    }
}

/// Which way the page's coordinates run along the two axes of the shown
/// rectangle, as 1 or -1, for an attribute's turn and flips: the image is
/// flipped, then turned.
#[inline]
pub fn page_signs(turn: i32, revh: bool, revv: bool) -> (f64, f64) {
    let (h, v) = (if revh { -1.0 } else { 1.0 }, if revv { -1.0 } else { 1.0 });
    match turn & 3 {
        0 => (h, v),
        1 => (-v, h),
        2 => (-h, -v),
        _ => (v, -h),
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
    let (gx0, xs) = on_image(x0..x1, |px| at(px, y0).0, w)?;
    let (gy0, ys) = on_image(y0..y1, |py| at(x0, py).1, h)?;
    Some(Grid {
        x0: gx0,
        y0: gy0,
        xs,
        ys,
    })
}

/// The first frame column or row in `range` on the image, of `len`, where `f`
/// says what image x or y each shows, and those of the run from there.
#[inline]
fn on_image(
    range: std::ops::Range<u32>,
    f: impl Fn(u32) -> f64,
    len: f64,
) -> Option<(u32, Vec<f64>)> {
    let on = |v: f64| v >= 0.0 && v < len;
    let (mut first, mut run) = (None, Vec::new());
    for p in range.clone() {
        let v = f(p);
        if on(v) {
            if first.is_none() {
                first = Some(p);
                run.reserve_exact((range.end - p) as usize);
            }
            run.push(v);
        } else if first.is_some() {
            break;
        }
    }
    Some((first?, run))
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

impl Layer {
    /// The layer's settings for a frame with `sample` pixels to one of
    /// the screen's, as XSCREEN's sampling makes it finer.
    pub fn fine(&self, sample: u32) -> Layer {
        let s = sample as i32;
        if s == 1 {
            return *self;
        }
        let f = f64::from(s);
        let scale = Affine {
            a: f,
            d: f,
            ..Affine::IDENTITY
        };
        Layer {
            matrix: self.matrix.then(&scale),
            clip: self.clip.map(|c| Clip {
                x0: c.x0 * s,
                y0: c.y0 * s,
                x1: c.x1 * s + s - 1,
                y1: c.y1 * s + s - 1,
            }),
            ..*self
        }
    }
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
        let (gx, gy, q) = (at(0, nx0), at(1, nx0), at(2, nx0));
        let (dx, dy, dq) = (inv[0][0] * step, inv[1][0] * step, inv[2][0] * step);
        let (first, end) = depth_span(
            z[0] * gx + z[1] * gy + z[2] * q,
            z[0] * dx + z[1] * dy + z[2] * dq,
            // Above x1 when the layer lies wholly left of the clip: the pixels
            // from x0 to x1 are then none unless Z / W ends them first.
            x1.wrapping_sub(x0),
        );
        let (start, stop) = (x0.wrapping_add(first), x0.wrapping_add(end));
        if start >= stop {
            continue;
        }
        let (first, end) = (start - x0, stop - x0);
        let line = py as usize * width;
        let along = Along {
            layer: (gx, gy, q),
            step: (dx, dy, dq),
            to_read: Affine {
                a: sample,
                b: 0.0,
                c: 0.0,
                d: sample,
                tx: -ox,
                ty: -oy,
            },
            front: Some(h[2]),
        };
        along.each(
            (first, end),
            (sw, sh),
            |gx, gy, q| {
                let (lx, ly) = (gx / q, gy / q);
                // In front only; a NaN is not.
                if (h[2][0] * lx + h[2][1] * ly + h[2][2]).partial_cmp(&0.0)
                    != Some(Ordering::Greater)
                {
                    return None;
                }
                Some((
                    floor_in(lx * sample - ox, sw)?,
                    floor_in(ly * sample - oy, sh)?,
                ))
            },
            |t, sx, sy| {
                let s = from[sy * sw as usize + sx];
                if s != 0
                    && let Some(d) = pixels.get_mut(line + (x0 + t) as usize)
                {
                    *d = over_premultiplied(*d, s);
                }
            },
        );
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

/// Whether a layer with LMATRIX's 4x4 matrix `m` shows anything.
pub fn can_project(m: &[f64; 16]) -> bool {
    invert3(&homography(m)).is_some()
}

/// X, Y, Z and W of a layer's point through LMATRIX's 4x4 matrix `m`, each as
/// what the point's `(x, y, 1)` is multiplied by.
pub fn clip_rows(m: &[f64; 16]) -> [[f64; 3]; 4] {
    [
        [m[0], m[4], m[12]],
        [m[1], m[5], m[13]],
        [m[2], m[6], m[14]],
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
        // Only where q is above 0 is the point in front: W is 1 / q. Each
        // (k, c) asks for k * t + c above 0.
        let bounds = [
            (dq, q),
            (kx, ax),
            (iw * dq - kx, iw * q - ax),
            (ky, ay),
            (ih * dq - ky, ih * q - ay),
        ];
        let (mut lo, mut hi) = (0.0, n);
        for (k, c) in bounds {
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
        let along = Along {
            layer: (gx, gy, q),
            step: (dx, dy, dq),
            to_read: back,
            front: None,
        };
        let size = (i64::from(w), i64::from(h));
        along.each(
            (from, to),
            size,
            |gx, gy, q| {
                let (lx, ly) = (gx / q, gy / q);
                // A NaN is not in front either.
                if q.partial_cmp(&0.0) != Some(Ordering::Greater) {
                    return None;
                }
                let (ix, iy) = back.apply(lx, ly);
                Some((floor_in(ix, size.0)?, floor_in(iy, size.1)?))
            },
            |t, ix, iy| {
                if let Some(paint) = pixel(ix as i32, iy as i32) {
                    let d = &mut line[(x0 + t) as usize];
                    *d = paint.put(*d);
                }
            },
        );
    }
}

/// A row of the frame through a homography: the layer's point at pixel `t`
/// is `(gx + t * dx, gy + t * dy) / (q + t * dq)`, and what it reads is that
/// point through `to_read`. Two divisions a pixel are what a small CPU feels
/// most, so what is read is stepped in fixed point between exact points
/// [`PERSPECTIVE_BLOCK`] pixels apart. A pixel where that may fall on the other
/// side of a whole number is worked out exactly, with one division, and only
/// if even that is too close to tell, with the sums added up pixel by pixel as
/// `plain` expects them; so the same texels are read.
struct Along {
    layer: (f64, f64, f64),
    step: (f64, f64, f64),
    to_read: Affine,
    /// The row that a point is tested against for lying in front, if that
    /// is not settled by `q` being above 0.
    front: Option<[f64; 3]>,
}

/// How many pixels are stepped between exact points, as a power of 2.
const PERSPECTIVE_SHIFT: u32 = 4;
const PERSPECTIVE_BLOCK: u32 = 1 << PERSPECTIVE_SHIFT;
/// Shorter runs go the plain way: they would save less than setting up costs.
const PERSPECTIVE_SHORTEST: u32 = 6;
/// One texel, in the fixed point of [`Along`].
const ALONG_ONE: f64 = (1u64 << 32) as f64;

/// How close to a whole number [`Along`]'s fixed point may be for a run.
struct AlongRun {
    /// How close a pixel with an exact point of its own may be.
    tight: f64,
    /// What stepping along a curve adds to that, times the fourth power of
    /// the smallest `q` of the block.
    wander: f64,
    /// How much each step changes in a block, in fixed point, as x and y, times
    /// the cube of a `q` of the block.
    bend: (f64, f64),
}

/// Whether a fixed point number is within `near` of a whole number.
#[inline(always)]
fn near_whole(f: i64, near: i64) -> bool {
    /// The fraction of a fixed point number.
    const FRACTION: i64 = (1 << 32) - 1;
    (f.wrapping_add(near) & FRACTION) < 2 * near
}

impl Along {
    /// Calls `put` with `(t, x, y)` for each pixel `t` from `from` up to but
    /// not including `to` that reads something: `x` from 0 up to `size.0` and
    /// `y` from 0 up to `size.1`, as `plain` works them out from the sums
    /// `layer + t * step` added up pixel by pixel.
    #[inline(always)]
    fn each(
        &self,
        (from, to): (u32, u32),
        size: (i64, i64),
        plain: impl Fn(f64, f64, f64) -> Option<(usize, usize)>,
        mut put: impl FnMut(u32, usize, usize),
    ) {
        /// What stepping can be off by over a block, by itself, in fixed
        /// point, with a lot to spare.
        const STEPPING: i64 = 1 << 11;
        /// What a point's own truncations can be off by.
        const TRUNCATION: i64 = 4;
        /// This close to a whole number all the time, or closer, a pixel
        /// is not worth stepping to.
        const TOO_CLOSE: f64 = (1u64 << 30) as f64;
        let (gx, gy, q) = self.layer;
        let (dx, dy, dq) = self.step;
        let ft = f64::from(from);
        // Those sums, added up only as far as a pixel that needs them does.
        let mut sums = (gx + dx * ft, gy + dy * ft, q + dq * ft);
        let mut done = from;
        let mut added_up = |t: u32, put: &mut dyn FnMut(u32, usize, usize)| {
            while done < t {
                sums = (sums.0 + dx, sums.1 + dy, sums.2 + dq);
                done += 1;
            }
            if let Some((x, y)) = plain(sums.0, sums.1, sums.2) {
                put(t, x, y);
            }
        };
        let run = (to.saturating_sub(from) >= PERSPECTIVE_SHORTEST)
            .then(|| self.run(from, to))
            .flatten();
        let Some(run) = run else {
            for t in from..to {
                added_up(t, &mut put);
            }
            return;
        };
        let tight = run.tight as i64 + TRUNCATION;
        let (w, h) = (size.0 as u64, size.1 as u64);
        let mut t = from;
        let mut start = self.point(t);
        while t < to {
            let n = (to - t).min(PERSPECTIVE_BLOCK);
            let end = self.point(t + PERSPECTIVE_BLOCK);
            let inv = f64::max(start.0, end.0);
            let cube = inv * inv * inv;
            let most = run.tight + run.wander * (cube * inv);
            // Every pixel of the block gets a point of its own otherwise.
            let near = if most < TOO_CLOSE {
                most as i64 + STEPPING
            } else {
                1 << 32
            };
            // Stepped with the change of the step it has at the block's side
            // that bends most, so that it leaves the true curve far less than a
            // straight line would, and hardly a pixel needs a closer look.
            let (mut fx, mut fy) = (start.1, start.2);
            let (bend_x, bend_y) = ((run.bend.0 * cube) as i64, (run.bend.1 * cube) as i64);
            let across = i64::from(PERSPECTIVE_BLOCK - 1);
            let mut kx = (end.1.wrapping_sub(fx) >> PERSPECTIVE_SHIFT)
                .wrapping_sub(bend_x.wrapping_mul(across) >> 1);
            let mut ky = (end.2.wrapping_sub(fy) >> PERSPECTIVE_SHIFT)
                .wrapping_sub(bend_y.wrapping_mul(across) >> 1);
            for p in t..t + n {
                let (cx, cy) = (fx, fy);
                // Only a block far too bent to go by, whose every pixel is
                // looked at closely, can run out of bits.
                fx = fx.wrapping_add(kx);
                fy = fy.wrapping_add(ky);
                kx = kx.wrapping_add(bend_x);
                ky = ky.wrapping_add(bend_y);
                let (mut x, mut y) = (cx >> 32, cy >> 32);
                if near_whole(cx, near) | near_whole(cy, near) {
                    let (_, ex, ey) = self.point(p);
                    if near_whole(ex, tight) || near_whole(ey, tight) {
                        added_up(p, &mut put);
                        continue;
                    }
                    (x, y) = (ex >> 32, ey >> 32);
                }
                // Negative ones are out of range too.
                if (x as u64) < w && (y as u64) < h {
                    put(p, x as usize, y as usize);
                }
            }
            t += n;
            start = end;
        }
    }

    /// `1 / q` at pixel `t`, and what it reads in fixed point.
    #[inline(always)]
    fn point(&self, t: u32) -> (f64, i64, i64) {
        let t = f64::from(t);
        let (gx, gy, q) = self.layer;
        let (dx, dy, dq) = self.step;
        let inv = 1.0 / (q + dq * t);
        let (ix, iy) = (self.to_read).apply((gx + dx * t) * inv, (gy + dy * t) * inv);
        (inv, (ix * ALONG_ONE) as i64, (iy * ALONG_ONE) as i64)
    }

    /// How close to a whole number the pixels from `from` up to `to` must be
    /// to be looked at more closely; None if they cannot be stepped: a pixel,
    /// or what lies a block on from the last, is not surely in front, or too
    /// far out for the fixed point.
    #[inline(never)]
    fn run(&self, from: u32, to: u32) -> Option<AlongRun> {
        /// Above this what is read has too few fraction bits left.
        const LIMIT: f64 = (1u64 << 24) as f64;
        /// This much would look at every pixel more closely.
        const TOO_CLOSE: f64 = (1u64 << 30) as f64;
        let (gx, gy, q) = self.layer;
        let (dx, dy, dq) = self.step;
        let m = &self.to_read;
        let (ft, tt) = (f64::from(from), f64::from(to + PERSPECTIVE_BLOCK));
        let far = |a: f64, k: f64| (a + k * ft, a + k * tt);
        let ((x0, x1), (y0, y1), (q0, q1)) = (far(gx, dx), far(gy, dy), far(q, dq));
        // The added up sums are off from the ideal ones by a rounding for
        // each addition, of no more than the largest they reach, and a point
        // here by a few more.
        let adds = (tt - ft + 8.0) * f64::EPSILON;
        let eq = adds * f64::max(q0.abs(), q1.abs());
        let least = f64::min(q0, q1);
        // Also false for NaN. Each of the layer point's coordinates, what
        // is read and what is in front go one way from one end of the run to
        // the other, there being no pole between, so that their ends bound all
        // that is between.
        let in_front = q0 > 2.0 * eq && q1 > 2.0 * eq;
        if !in_front {
            return None;
        }
        let inv = 1.0 / least;
        let lx = f64::max(x0.abs(), x1.abs()) * inv;
        let ly = f64::max(y0.abs(), y1.abs()) * inv;
        // How far the layer point can be off.
        let of_x = 2.0 * inv * adds * (f64::max(x0.abs(), x1.abs()) + lx * q1.abs().max(q0.abs()))
            + 4.0 * f64::EPSILON * lx;
        let of_y = 2.0 * inv * adds * (f64::max(y0.abs(), y1.abs()) + ly * q1.abs().max(q0.abs()))
            + 4.0 * f64::EPSILON * ly;
        // What that does through a row of a matrix, with its own roundings.
        let through = |r: [f64; 3]| {
            let (p, q) = (r[0].abs(), r[1].abs());
            p * of_x + q * of_y + 8.0 * f64::EPSILON * (p * lx + q * ly + r[2].abs())
        };
        let at = |r: [f64; 3], x: f64, y: f64, q: f64| (r[0] * x + r[1] * y) / q + r[2];
        if let Some(r) = self.front {
            // Also false for NaN.
            let least = through(r);
            let in_front = at(r, x0, y0, q0) > least && at(r, x1, y1, q1) > least;
            if !in_front {
                return None;
            }
        }
        let (rx, ry) = ([m.a, m.b, m.tx], [m.c, m.d, m.ty]);
        let (i0, i1) = (
            (at(rx, x0, y0, q0), at(ry, x0, y0, q0)),
            (at(rx, x1, y1, q1), at(ry, x1, y1, q1)),
        );
        let tight = f64::max(through(rx), through(ry)) * ALONG_ONE;
        // Also false for NaN.
        if !(i0.0.abs() < LIMIT
            && i0.1.abs() < LIMIT
            && i1.0.abs() < LIMIT
            && i1.1.abs() < LIMIT
            && tight < TOO_CLOSE)
        {
            return None;
        }
        // What is read is (c + k * t) / q. Its second derivative is
        // 2 * dq * (c * dq - k * q) / q^3 and its third 3 * dq / q of that the
        // other way, with a bracket that is the same all along the row. Stepped
        // with the second at some point of a block of n, it leaves the true
        // curve by at most n^2 / 8 of what the second changes by over the block.
        let n = f64::from(PERSPECTIVE_BLOCK);
        let bracket = |r: [f64; 3]| {
            let k = r[0] * dx + r[1] * dy + r[2] * dq;
            let c = r[0] * gx + r[1] * gy + r[2] * q;
            let (kq, cd) = (k * q, c * dq);
            (
                cd - kq,
                (cd - kq).abs() + 8.0 * f64::EPSILON * (kq.abs() + cd.abs()),
            )
        };
        let ((bx, wx), (by, wy)) = (bracket(rx), bracket(ry));
        Some(AlongRun {
            tight,
            wander: f64::max(wx, wy) * 0.75 * n * n * n * dq * dq * ALONG_ONE,
            bend: (2.0 * dq * bx * ALONG_ONE, 2.0 * dq * by * ALONG_ONE),
        })
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

/// A rectangle of a frame's pixels, `x1` and `y1` left out.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Area {
    pub x0: u32,
    pub y0: u32,
    pub x1: u32,
    pub y1: u32,
}

impl Area {
    /// All of a frame.
    pub fn all(width: u32, height: u32) -> Area {
        Area {
            x0: 0,
            y0: 0,
            x1: width,
            y1: height,
        }
    }

    /// Whether this holds all of a frame.
    pub fn covers(&self, width: u32, height: u32) -> bool {
        self.x0 == 0 && self.y0 == 0 && self.x1 >= width && self.y1 >= height
    }

    /// The smallest area holding both; an empty one adds nothing.
    pub fn with(&self, other: &Area) -> Area {
        if other.x0 >= other.x1 || other.y0 >= other.y1 {
            return *self;
        }
        if self.x0 >= self.x1 || self.y0 >= self.y1 {
            return *other;
        }
        Area {
            x0: self.x0.min(other.x0),
            y0: self.y0.min(other.y0),
            x1: self.x1.max(other.x1),
            y1: self.y1.max(other.y1),
        }
    }

    /// The pixels an image of `w` by `h` can touch through `m`, within a
    /// `width` by `height` frame and `clip`, with some to spare; all of the
    /// frame for a matrix out of range.
    pub fn of_image(
        m: &Affine,
        w: f64,
        h: f64,
        clip: Option<&Clip>,
        (width, height): (u32, u32),
    ) -> Area {
        const SPARE: f64 = 2.0;
        let (x0, y0, x1, y1) = bounds(m, (0.0, 0.0, w, h));
        let (fw, fh) = (f64::from(width), f64::from(height));
        // Also false for NaN.
        if !(x0 <= x1 && y0 <= y1) {
            return Area::all(width, height);
        }
        let mut area = Area {
            x0: (x0 - SPARE).clamp(0.0, fw) as u32,
            y0: (y0 - SPARE).clamp(0.0, fh) as u32,
            x1: (x1 + SPARE + 1.0).clamp(0.0, fw) as u32,
            y1: (y1 + SPARE + 1.0).clamp(0.0, fh) as u32,
        };
        if let Some(c) = clip {
            area.x0 = area.x0.max(c.x0.max(0) as u32);
            area.y0 = area.y0.max(c.y0.max(0) as u32);
            area.x1 = area.x1.min((c.x1 + 1).max(0) as u32);
            area.y1 = area.y1.min((c.y1 + 1).max(0) as u32);
        }
        area
    }
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
    compose_pixels(dst.pixels_mut(), layer.pixels(), mode, color, opaque);
}

/// [`compose`] for the part of a layer that is not clear, which is all it has
/// to do where the layer is clear only if the frame is to stay as it is
/// there: not in modes 3 and 4 unless `opaque`.
pub fn compose_area(
    dst: &mut Framebuffer,
    layer: &Framebuffer,
    mode: Compose,
    color: u32,
    opaque: bool,
    area: &Area,
) {
    let width = dst.width() as usize;
    if area.covers(dst.width(), dst.height()) {
        return compose(dst, layer, mode, color, opaque);
    }
    let (x0, x1) = (area.x0 as usize, area.x1 as usize);
    let (rows, from) = (dst.pixels_mut().chunks_exact_mut(width), layer.pixels());
    let lines = rows.zip(from.chunks_exact(width));
    let range = area.y0 as usize..area.y1 as usize;
    for (d, l) in lines.skip(range.start).take(range.len()) {
        compose_pixels(&mut d[x0..x1], &l[x0..x1], mode, color, opaque);
    }
}

#[inline]
fn compose_pixels(dst: &mut [u32], layer: &[u32], mode: Compose, color: u32, opaque: bool) {
    if color != 0xFFFF_FFFF {
        return compose_tinted(dst, layer, mode, color, opaque);
    }
    match mode {
        // The usual case, a plain layer, as one blend a pixel.
        Compose::Direct | Compose::Normal => {
            for (d, &l) in dst.iter_mut().zip(layer) {
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
            for (d, &l) in dst.iter_mut().zip(layer) {
                let (a, b) = (d.to_le_bytes(), (l & 0x00FF_FFFF).to_le_bytes());
                *d = u32::from_le_bytes(std::array::from_fn(|i| a[i].saturating_add(b[i])));
            }
        }
        // Each mode gets its own loop, so that its sum is worked inline.
        Compose::Multiply if opaque => {
            compose_each::<false>(dst, layer, plain(|p, d, a| div255(d * (p + 255 - a))))
        }
        Compose::Multiply => {
            compose_each::<true>(dst, layer, plain(|p, d, a| div255(d * (p + 255 - a))))
        }
        Compose::Screen if opaque => {
            compose_each::<false>(dst, layer, plain(|p, d, _| d + div255(p * (255 - d))))
        }
        Compose::Screen => {
            compose_each::<true>(dst, layer, plain(|p, d, _| d + div255(p * (255 - d))))
        }
    }
}

/// How many pixels [`compose_each`] tests for being clear at once.
const COMPOSE_RUN: usize = 16;

/// [`compose`] pixel by pixel: `put` gives a frame pixel with a layer pixel
/// that is not clear put on it. A pixel clear in the layer changes nothing,
/// but for making the frame opaque with `MAKE_OPAQUE`, which modes 3 and 4 do
/// over the whole screen. Layers are mostly flat or clear, over frames
/// that are too, so a stretch of clear pixels takes one test and a pixel
/// like the last, over the same, the last's result.
#[inline(always)]
fn compose_each<const MAKE_OPAQUE: bool>(
    dst: &mut [u32],
    layer: &[u32],
    put: impl Fn(u32, u32) -> u32,
) {
    let (mut last_l, mut last_d, mut last) = (0, 0, 0);
    let runs = dst.chunks_mut(COMPOSE_RUN);
    for (ds, ls) in runs.zip(layer.chunks(COMPOSE_RUN)) {
        if ls.iter().fold(0, |a, &l| a | l) == 0 {
            if MAKE_OPAQUE {
                for d in ds {
                    *d |= 0xFF00_0000;
                }
            }
            continue;
        }
        for (d, &l) in ds.iter_mut().zip(ls) {
            if l == 0 {
                if MAKE_OPAQUE {
                    *d |= 0xFF00_0000;
                }
                continue;
            }
            if l != last_l || *d != last_d {
                (last_l, last_d, last) = (l, *d, put(*d, l));
            }
            *d = last;
        }
    }
}

/// A frame pixel for [`compose_each`] in modes 3 and 4, from `f`, which gives
/// a channel from the layer's light `p`, the frame's `d` and the layer's
/// coverage `a`; it is held to 255.
#[inline(always)]
fn plain(f: impl Fn(u32, u32, u32) -> u32) -> impl Fn(u32, u32) -> u32 {
    move |d, l| {
        let [a, pr, pg, pb] = l.to_be_bytes().map(u32::from);
        let [_, dr, dg, db] = d.to_be_bytes().map(u32::from);
        let c = |p: u32, d: u32| f(p, d, a).min(255) as u8;
        u32::from_be_bytes([0xFF, c(pr, dr), c(pg, dg), c(pb, db)])
    }
}

/// [`compose`] the general way, for any layer color, even white. With
/// the layer's premultiplied light `p` and coverage `a`, the frame's `d`
/// and the color's `c` and alpha `ca`, the layer gives the light
/// `P = p*c*ca` and the coverage `A = a*ca` (over 255²), and each mode
/// rounds once:
/// 1: `P + d*(1-A)`; 2: `d + P`; 3: `d*(P + 1-A)`; 4: `d + P*(1-d)`.
#[inline(never)]
fn compose_tinted(dst: &mut [u32], layer: &[u32], mode: Compose, color: u32, opaque: bool) {
    const N: u32 = 255 * 255;
    const N3: u64 = 255 * 255 * 255;
    let multiply = tinted(
        color,
        |p, d, a| {
            let x = u64::from(d) * u64::from(p + 255 * (N - a));
            ((x + N3 / 2) / N3) as u32
        },
        |_, _| 255,
    );
    let screen = tinted(
        color,
        |p, d, _| {
            let x = u64::from(p) * u64::from(255 - d);
            d + ((x + N3 / 2) / N3) as u32
        },
        |_, _| 255,
    );
    // Each mode gets its own loop, so that its sum is worked inline.
    match mode {
        Compose::Direct | Compose::Normal => compose_each::<false>(
            dst,
            layer,
            tinted(
                color,
                |p, d, a| (p + d * (N - a) + N / 2) / N,
                |da, a| (da * (N - a) + 255 * a + N / 2) / N,
            ),
        ),
        Compose::Add => compose_each::<false>(
            dst,
            layer,
            tinted(color, |p, d, _| d + (p + N / 2) / N, |da, _| da),
        ),
        // As in plain layers, over the whole screen.
        Compose::Multiply if opaque => compose_each::<false>(dst, layer, multiply),
        Compose::Multiply => compose_each::<true>(dst, layer, multiply),
        Compose::Screen if opaque => compose_each::<false>(dst, layer, screen),
        Compose::Screen => compose_each::<true>(dst, layer, screen),
    }
}

/// A frame pixel for [`compose_each`] under a layer color, by `sum`, which
/// gives a channel from `P`, the frame's `d` and `A`, as [`compose_tinted`]
/// calls them; it is held to 255. `alpha` gives the frame's alpha from its
/// own and `A`.
#[inline(always)]
fn tinted(
    color: u32,
    sum: impl Fn(u32, u32, u32) -> u32 + Copy,
    alpha: impl Fn(u32, u32) -> u32 + Copy,
) -> impl Fn(u32, u32) -> u32 + Copy {
    let [ca, cr, cg, cb] = color.to_be_bytes().map(u32::from);
    move |d, l| {
        let [la, lr, lg, lb] = l.to_be_bytes().map(u32::from);
        let a = la * ca;
        let [da, dr, dg, db] = d.to_be_bytes().map(u32::from);
        let c = |p: u32, d: u32| sum(p, d, a).min(255);
        alpha(da, a) << 24
            | c(lr * cr * ca, dr) << 16
            | c(lg * cg * ca, dg) << 8
            | c(lb * cb * ca, db)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Texels are read as [`draw_image`] reads them, near whole numbers too.
    #[test]
    fn stepping_reads_the_same_texels() {
        let mut seed = 0x1234_5678_u32;
        let mut next = move || {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            seed
        };
        let color = |x: i32, y: i32| 0xFF00_0000 | ((y as u32 & 0xFFF) << 12) | (x as u32 & 0xFFF);
        for i in 0..4000 {
            let r = &mut next;
            let pos = (
                f64::from(r() % 2000) / 8.0 - 60.0,
                f64::from(r() % 1200) / 8.0 - 60.0,
            );
            let home = (f64::from(r() % 32), f64::from(r() % 32));
            // Whole degrees, the 45s and quarter turns, which can tie, and any.
            let rot = match i % 4 {
                0 => f64::from(r() % 360),
                1 => f64::from(r() % 8) * 45.0,
                2 => f64::from(r() % 4) * 90.0,
                _ => f64::from(r()) / 1e6,
            };
            let scale = match i % 3 {
                0 => (f64::from(r() % 16 + 1) / 4.0, f64::from(r() % 16 + 1) / 4.0),
                1 => (1.0, 1.0),
                _ => (f64::from(r()) / 1e9 + 0.01, f64::from(r()) / 1e9 + 0.01),
            };
            let place = Placement {
                matrix: Affine::place(pos, home, rot, scale),
                clip: None,
                add: false,
            };
            let (w, h) = ((r() % 64 + 1) as i32, (r() % 64 + 1) as i32);
            let tie = place.texel_tie((w, h), (1.0, 1.0));
            let whole = |v: f64| v as i32 - i32::from(v < 0.0);
            let mut slow = Framebuffer::new(160, 96);
            draw_image(&mut slow, &place, w, h, |x, y| {
                Some(Over1::new(color(whole(x + tie.0), whole(y + tie.1))))
            });
            let mut fast = Framebuffer::new(160, 96);
            draw_texels(&mut fast, &place, w, h, tie, |x, y| {
                Some(Over1::new(color(x, y)))
            });
            assert_eq!(slow.pixels(), fast.pixels(), "{place:?} {w} {h}");
        }
    }

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
            compose_tinted(slow.pixels_mut(), layer.pixels(), mode, 0xFFFF_FFFF, false);
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
    fn ties_settle_along_the_page() {
        let tie = |rot, scale, size, page| {
            let place = Placement {
                matrix: Affine::place((100.5, 50.5), (0.0, 0.0), rot, (scale, scale)),
                clip: None,
                add: false,
            };
            let (x, y) = place.texel_tie((size, size), page);
            // What matters is the sign of what the point ends up moved by
            // from where it was before covering was settled.
            let inv = place.matrix.inverse().unwrap();
            let (nx, ny) = COVER_NUDGE;
            let (mx, my) = (inv.a * nx + inv.b * ny, inv.c * nx + inv.d * ny);
            ((x + mx).signum() as i32, (y + my).signum() as i32)
        };
        let plain = (1.0, 1.0);
        // A side and a scale that are powers of two: the page's higher texel.
        assert_eq!(tie(0.0, 0.5, 16, plain), (1, 1));
        assert_eq!(tie(90.0, 1.0, 32, plain), (1, 1));
        assert_eq!(tie(0.0, 1.0, 16, (-1.0, 1.0)), (-1, 1));
        // Anything else: the lower, whichever way the image lies.
        assert_eq!(tie(0.0, 1.5, 16, plain), (-1, -1));
        assert_eq!(tie(180.0, 1.5, 16, (-1.0, 1.0)), (1, -1));
        assert_eq!(tie(0.0, 1.0, 15, plain), (-1, -1));
        assert_eq!(tie(90.0, 1.0, 24, (1.0, -1.0)), (-1, 1));
        // Neither upright nor at a quarter turn.
        assert_eq!(tie(45.0, 1.0, 15, plain), (1, -1));
    }

    #[test]
    fn the_page_runs_as_the_image_is_flipped_then_turned() {
        assert_eq!(page_signs(0, false, false), (1.0, 1.0));
        assert_eq!(page_signs(0, true, false), (-1.0, 1.0));
        assert_eq!(page_signs(1, false, false), (-1.0, 1.0));
        assert_eq!(page_signs(1, true, true), (1.0, -1.0));
        assert_eq!(page_signs(2, false, false), (-1.0, -1.0));
        assert_eq!(page_signs(3, false, false), (1.0, -1.0));
        assert_eq!(page_signs(3, false, true), (-1.0, -1.0));
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

    /// What [`Along::each`] gives for a row and what going over it with the
    /// sums added up, pixel by pixel, gives.
    fn along_both(along: &Along, (from, to): (u32, u32), size: (i64, i64)) {
        let plain = |gx: f64, gy: f64, q: f64| {
            let (lx, ly) = (gx / q, gy / q);
            let front = match along.front {
                Some(r) => {
                    (r[0] * lx + r[1] * ly + r[2]).partial_cmp(&0.0) == Some(Ordering::Greater)
                }
                None => q.partial_cmp(&0.0) == Some(Ordering::Greater),
            };
            if !front {
                return None;
            }
            let (x, y) = along.to_read.apply(lx, ly);
            Some((floor_in(x, size.0)?, floor_in(y, size.1)?))
        };
        let mut got = Vec::new();
        along.each((from, to), size, plain, |t, x, y| got.push((t, x, y)));
        let (gx, gy, q) = along.layer;
        let (dx, dy, dq) = along.step;
        let ft = f64::from(from);
        let (mut gx, mut gy, mut q) = (gx + dx * ft, gy + dy * ft, q + dq * ft);
        let mut want = Vec::new();
        for t in from..to {
            if let Some((x, y)) = plain(gx, gy, q) {
                want.push((t, x, y));
            }
            (gx, gy, q) = (gx + dx, gy + dy, q + dq);
        }
        assert_eq!(got, want, "{:?} {:?}", along.layer, along.step);
    }

    #[test]
    fn stepping_through_a_matrix_reads_the_same_texels() {
        let mut next = numbers(0x1234_5679);
        let mut unit = move || f64::from(next() % 2_000_001) / 1e6 - 1.0;
        let fw = 1280.0;
        for i in 0..4000 {
            // A layer turned and zoomed, tilted a little or a lot, even so
            // far that a row runs past the horizon.
            let (angle, zoom) = (unit() * 3.2, 0.2 + (unit() + 1.0) * 2.0);
            let (c, sn) = (angle.cos() * zoom, angle.sin() * zoom);
            let tilt = match i % 4 {
                0 => 0.0,
                1 => 0.0003,
                2 => 0.003,
                _ => 0.03,
            };
            let mut m = [0.0; 16];
            (m[0], m[4], m[1], m[5]) = (c / 640.0, -sn / 640.0, -sn / 360.0, -c / 360.0);
            (m[12], m[13]) = (unit() * 2.0, unit() * 2.0);
            (m[3], m[7], m[15]) = (tilt * unit(), tilt * unit(), 1.0);
            let h = homography(&m);
            let Some(inv) = invert3(&h) else {
                continue;
            };
            let ny = unit();
            let nx0 = 0.5 / fw * 2.0 - 1.0;
            let at = |r: usize| inv[r][0] * nx0 + inv[r][1] * ny + inv[r][2];
            let step = 2.0 / fw;
            let tx = (unit() * 300.0).round();
            let sample = if i % 3 == 0 { 2.0 } else { 1.0 };
            let along = Along {
                layer: (at(0), at(1), at(2)),
                step: (inv[0][0] * step, inv[1][0] * step, inv[2][0] * step),
                to_read: Affine {
                    a: sample,
                    b: 0.0,
                    c: 0.0,
                    d: sample,
                    tx,
                    ty: 0.0,
                },
                front: (i % 2 == 0).then_some(h[2]),
            };
            let from = ((unit() + 1.0) * 100.0) as u32;
            along_both(&along, (from, 1280), (2000, 2000));
            along_both(&along, (from, from + 5), (2000, 2000));
            along_both(&along, (from, from + 40), (64, 64));
        }
        // Points exactly on whole numbers and exactly between, at all sizes
        // of step, where added up sums and worked out ones may fall either way.
        for k in [1.0, 0.5, 0.25, 0.1, 1.0 / 3.0, 3.0, 1e-3, 1e-9] {
            for dq in [0.0, 1e-4, 1e-12, -1e-4] {
                let along = Along {
                    layer: (0.0, 0.5, 1.0),
                    step: (k * 1.0, k * 0.5, dq),
                    to_read: Affine::IDENTITY,
                    front: None,
                };
                along_both(&along, (0, 1280), (2000, 2000));
                along_both(&along, (3, 400), (40, 40));
            }
        }
        // Past where anything can be stepped, and nothing at all.
        for (layer, step) in [
            ((1e300, 1e300, 1e-300), (1e300, 1.0, 0.0)),
            ((1.0, 1.0, 0.0), (1.0, 1.0, 0.0)),
            ((f64::NAN, 0.0, 1.0), (0.0, 0.0, 0.0)),
            ((0.0, 0.0, -1.0), (0.1, 0.1, 1e-2)),
        ] {
            let along = Along {
                layer,
                step,
                to_read: Affine::IDENTITY,
                front: None,
            };
            along_both(&along, (0, 300), (64, 64));
        }
    }

    #[test]
    fn an_image_stays_in_its_area() {
        let mut next = numbers(0x2468_ACE1);
        let mut unit = move || f64::from(next() % 2_000_001) / 1e6 - 1.0;
        let clip = Clip {
            x0: 20,
            y0: 10,
            x1: 100,
            y1: 60,
        };
        for i in 0..400 {
            let (w, h) = (1 + (i % 40), 1 + (i / 3 % 40));
            let matrix = Affine::place(
                (unit() * 200.0 + 60.0, unit() * 200.0 + 40.0),
                (unit() * 16.0, unit() * 16.0),
                if i % 2 == 0 { 0.0 } else { unit() * 360.0 },
                (unit() * 4.0, unit() * 4.0),
            );
            let clip = (i % 3 == 0).then_some(&clip);
            let mut fb = Framebuffer::new(160, 100);
            fb.clear(0);
            let place = Placement {
                matrix,
                clip,
                add: false,
            };
            draw_image(&mut fb, &place, w, h, |_, _| Some(Over1::new(0xFFFF_FFFF)));
            let (fw, fh) = (f64::from(w), f64::from(h));
            let area = Area::of_image(&matrix, fw, fh, clip, (160, 100));
            for (at, &p) in fb.pixels().iter().enumerate() {
                let (x, y) = (at as u32 % 160, at as u32 / 160);
                let inside = x >= area.x0 && x < area.x1 && y >= area.y0 && y < area.y1;
                assert!(p == 0 || inside, "{matrix:?} {area:?} {x},{y}");
            }
        }
    }

    #[test]
    fn a_layer_clear_outside_an_area_composes_as_a_whole_one() {
        let area = Area {
            x0: 3,
            y0: 2,
            x1: 9,
            y1: 5,
        };
        for seed in 1..40 {
            let (frame, layer) = layer_and_frame(seed);
            let (w, h) = (layer.width(), layer.height());
            let mut only = Framebuffer::new(w, h);
            only.clear(0);
            for y in area.y0..area.y1.min(h) {
                for x in area.x0..area.x1.min(w) {
                    only.set(x, y, layer.get(x, y).unwrap());
                }
            }
            for mode in [
                Compose::Normal,
                Compose::Add,
                Compose::Multiply,
                Compose::Screen,
            ] {
                for color in [0xFFFF_FFFF, 0xC080_40FF] {
                    let opaque = frame.pixels().iter().map(|p| p | 0xFF00_0000);
                    let mut whole = Framebuffer::new(w, h);
                    whole
                        .pixels_mut()
                        .iter_mut()
                        .zip(opaque)
                        .for_each(|(d, p)| *d = p);
                    let mut part = whole.clone();
                    compose(&mut whole, &only, mode, color, true);
                    let inside = Area {
                        x1: area.x1.min(w),
                        y1: area.y1.min(h),
                        ..area
                    };
                    compose_area(&mut part, &only, mode, color, true, &inside);
                    assert_eq!(whole.pixels(), part.pixels());
                }
            }
        }
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
