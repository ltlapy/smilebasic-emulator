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
// multiplied by the alpha. Frames are opaque, so for them it makes no
// difference; in a layer of its own, an additive element adds light
// without adding alpha, and lights what is under the layer once it is
// composed (checked on hardware).

/// `src`, an element's straight color, over `dst`, premultiplied.
#[inline]
pub fn over(dst: u32, src: u32) -> u32 {
    let sa = u64::from(src >> 24);
    if sa == 0xFF {
        return src;
    }
    // All four channels at once, 16 bits apiece: each sum is at most
    // 255 * 255, so none carries into the next.
    let s = spread(src | 0xFF00_0000) * sa + spread(dst) * (255 - sa);
    gather(div255_lanes(s))
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
    const LANES: u64 = 0x00FF_00FF_00FF_00FF;
    let t = x + 0x0080_0080_0080_0080;
    ((t + (t >> 8 & LANES)) >> 8) & LANES
}

/// `src`, an element's straight color, added to `dst` in proportion to
/// its alpha, leaving `dst`'s alpha as it is.
pub fn add(dst: u32, src: u32) -> u32 {
    let [sa, sr, sg, sb] = src.to_be_bytes().map(u32::from);
    let [da, dr, dg, db] = dst.to_be_bytes().map(u32::from);
    let plus = |s: u32, d: u32| (d + s * sa / 255).min(255) as u8;
    u32::from_be_bytes([da as u8, plus(sr, dr), plus(sg, dg), plus(sb, db)])
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
/// `place`. `sample` gets the image point under each frame pixel's
/// center, which falls between whole pixels when the frame is finer than
/// the image.
pub fn draw_image(
    fb: &mut Framebuffer,
    place: &Placement,
    w: i32,
    h: i32,
    sample: impl Fn(f64, f64) -> u32,
) {
    draw_image_part(fb, place, w, h, (0, 0, w, h), sample);
}

/// Like [`draw_image`], but only around where the part of the image from
/// `(x0, y0)` to `(x1, y1)` falls. Pixels just outside the part may still
/// be sampled, so `sample` must give nothing there; drawing an image part
/// by part then gives the same frame as drawing it whole, while parts
/// with nothing in them need not be drawn at all.
pub fn draw_image_part(
    fb: &mut Framebuffer,
    place: &Placement,
    w: i32,
    h: i32,
    part: (i32, i32, i32, i32),
    sample: impl Fn(f64, f64) -> u32,
) {
    let Some(inv) = place.matrix.inverse() else {
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
            let src = sample(lx, ly);
            if src >> 24 == 0 {
                continue;
            }
            let dst = &mut line[px as usize];
            *dst = blend(*dst, src, place.add);
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

/// `src` over `dst`, or added to it.
#[inline]
pub fn blend(dst: u32, src: u32, add_light: bool) -> u32 {
    if add_light {
        add(dst, src)
    } else {
        over(dst, src)
    }
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

impl Placement<'_> {
    /// Whether [`grid`] can stand for the placement.
    pub fn is_upright(&self) -> bool {
        self.matrix.b == 0.0 && self.matrix.c == 0.0
    }
}

/// The [`Grid`] of an upright placement, or `None` where it shows
/// nothing.
pub fn grid(fb: &Framebuffer, place: &Placement, w: i32, h: i32) -> Option<Grid> {
    debug_assert!(place.is_upright());
    let inv = place.matrix.inverse()?;
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
/// front (W above 0) show; `clip` is in frame pixels. `src`'s top left is
/// the flat frame's pixel `origin`, so it can hold what lies off the
/// screen.
// UNVERIFIED: that Z is 0 and nothing is clipped by depth, and that the
// layer is sampled without smoothing.
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
        for px in x0..x1 {
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

/// Draws a `w` x `h` image as [`draw_image`] does, but on a layer with
/// LMATRIX's 4x4 matrix `m`, as [`project`] puts one: `place` takes the
/// image to the layer drawn flat, `sample` frame pixels to one of the
/// layer's. Unlike drawing the layer flat first, this keeps what lies off
/// the flat screen, as a sprite's corners do on SmileBASIC 4's GPU.
#[allow(clippy::too_many_arguments)]
pub fn project_image(
    dst: &mut Framebuffer,
    m: &[f64; 16],
    sample: u32,
    place: &Placement,
    w: i32,
    h: i32,
    pixel: impl Fn(i32, i32) -> u32,
) {
    let hm = homography(m);
    let (Some(inv), Some(back)) = (invert3(&hm), place.matrix.inverse()) else {
        return;
    };
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
            let from = (lo - 1.0).max(0.0) as u32;
            let to = ((hi + 1.0).min(n) as u32).min(x1 - x0);
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
                let src = pixel(ix as i32, iy as i32);
                if src >> 24 == 0 {
                    continue;
                }
                let d = &mut line[(x0 + t) as usize];
                *d = blend(*d, src, place.add);
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

/// A premultiplied color over another.
#[inline]
fn over_premultiplied(dst: u32, src: u32) -> u32 {
    let a = u64::from(src >> 24);
    if a == 0xFF {
        return src;
    }
    gather(spread(src) + div255_lanes(spread(dst) * (255 - a)))
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

/// `x / 255`, rounded to the nearest, as SmileBASIC 4 blends (checked on
/// hardware against lossless screenshots).
fn div255(x: u32) -> u32 {
    (x + 127) / 255
}

/// Puts a layer drawn on its own (transparent, premultiplied) frame over
/// `dst`, its colors first multiplied by `color`.
// Checked on hardware: the results of modes 0 to 4 over a color and over
// black, and, with lossless screenshots, that mode 0 draws the layer's
// elements straight onto the frame without its color, pixel for pixel.
pub fn compose(dst: &mut Framebuffer, layer: &Framebuffer, mode: Compose, color: u32) {
    // The usual case, a plain layer, as one blend a pixel.
    if matches!(mode, Compose::Direct | Compose::Normal) && color == 0xFFFF_FFFF {
        for (d, &l) in dst.pixels_mut().iter_mut().zip(layer.pixels()) {
            if l != 0 {
                *d = over_premultiplied(*d, l) & 0x00FF_FFFF | *d & 0xFF00_0000;
            }
        }
        return;
    }
    let [ca, cr, cg, cb] = color.to_be_bytes().map(u32::from);
    for (d, &l) in dst.pixels_mut().iter_mut().zip(layer.pixels()) {
        if l == 0 {
            continue;
        }
        let [la, lr, lg, lb] = l.to_be_bytes().map(u32::from);
        // The layer color scales the light, and its alpha the coverage.
        let a = div255(la * ca);
        let tint = |p: u32, c: u32| div255(div255(p * c) * ca);
        let [pr, pg, pb] = [tint(lr, cr), tint(lg, cg), tint(lb, cb)];
        let [da, dr, dg, db] = d.to_be_bytes().map(u32::from);
        let each = |f: &dyn Fn(u32, u32) -> u32| {
            let c = |p: u32, d: u32| f(p, d).min(255) as u8;
            u32::from_be_bytes([da as u8, c(pr, dr), c(pg, dg), c(pb, db)])
        };
        *d = match mode {
            Compose::Direct | Compose::Normal => each(&|p, d| p + div255(d * (255 - a))),
            Compose::Add => each(&|p, d| d + p),
            Compose::Multiply => each(&|p, d| d * (p + 255 - a) / 255),
            Compose::Screen => each(&|p, d| d + p * (255 - d) / 255),
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: (f64, f64), b: (f64, f64)) -> bool {
        (a.0 - b.0).abs() < 1e-9 && (a.1 - b.1).abs() < 1e-9
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

    #[test]
    fn plain_layers_compose_as_any_other() {
        // The general formula, for a premultiplied layer pixel.
        let reference = |dst: u32, l: u32| {
            let [la, lr, lg, lb] = l.to_be_bytes().map(u32::from);
            let [da, dr, dg, db] = dst.to_be_bytes().map(u32::from);
            let c = |p: u32, d: u32| (p + div255(d * (255 - la))).min(255) as u8;
            u32::from_be_bytes([da as u8, c(lr, dr), c(lg, dg), c(lb, db)])
        };
        let mut seed = 0x9E37_79B9_u32;
        let mut next = || {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            seed
        };
        let (mut dst, mut layer) = (Framebuffer::new(256, 256), Framebuffer::new(256, 256));
        for (d, l) in dst.pixels_mut().iter_mut().zip(layer.pixels_mut()) {
            let a = next() >> 24;
            let [_, r, g, b] = next().to_be_bytes().map(|c| u32::from(c) * a / 255);
            (*d, *l) = (next(), a << 24 | r << 16 | g << 8 | b);
        }
        let expected: Vec<u32> = (dst.pixels().iter().zip(layer.pixels()))
            .map(|(&d, &l)| if l == 0 { d } else { reference(d, l) })
            .collect();
        compose(&mut dst, &layer, Compose::Normal, 0xFFFF_FFFF);
        assert_eq!(dst.pixels(), &expected[..]);
    }

    #[test]
    fn over_matches_channel_by_channel() {
        let reference = |dst: u32, src: u32| {
            let [sa, sr, sg, sb] = src.to_be_bytes().map(u32::from);
            let [da, dr, dg, db] = dst.to_be_bytes().map(u32::from);
            let mix = |s: u32, d: u32| div255(s * sa + d * (255 - sa)) as u8;
            u32::from_be_bytes([mix(255, da), mix(sr, dr), mix(sg, dg), mix(sb, db)])
        };
        let mut seed = 0x1234_5678_u32;
        let mut next = || {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            seed
        };
        for _ in 0..200_000 {
            let (dst, src) = (next(), next());
            assert_eq!(over(dst, src), reference(dst, src), "{dst:08X} {src:08X}");
        }
        for a in 0..=255u32 {
            for v in [0, 1, 127, 128, 254, 255u32] {
                let src = (a << 24) | (v * 0x0001_0101);
                let dst = (255 - v) * 0x0101_0101;
                assert_eq!(over(dst, src), reference(dst, src));
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
        draw_image(&mut fb, &place, 8, 1, |_, _| 0xFFFF_FFFF);
        let row: Vec<u32> = (0..5).map(|x| fb.get(x, 0).unwrap()).collect();
        assert_eq!(row, [0xFF00_0000, 0xFF00_0000, !0, !0, 0xFF00_0000]);
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
        compose(&mut m, &layer, Compose::Multiply, !0);
        assert_eq!(m.get(0, 0), Some(0xFF80_0000));
        let mut s = dst.clone();
        compose(&mut s, &layer, Compose::Screen, !0);
        assert_eq!(s.get(0, 0), Some(0xFFFF_8080));
        let mut tinted = dst.clone();
        compose(&mut tinted, &layer, Compose::Normal, 0xFF00_FFFF);
        assert_eq!(tinted.get(0, 0), Some(0xFF00_0000));
    }
}
