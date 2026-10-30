//! Putting display elements (sprites and text screens) on the frame:
//! placement by an affine transform, clipping, blending, and composing
//! layers.

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
pub fn over(dst: u32, src: u32) -> u32 {
    if src >> 24 == 0xFF {
        return src;
    }
    let [sa, sr, sg, sb] = src.to_be_bytes().map(u32::from);
    let [da, dr, dg, db] = dst.to_be_bytes().map(u32::from);
    let mix = |s: u32, d: u32| div255(s * sa + d * (255 - sa)) as u8;
    u32::from_be_bytes([mix(255, da), mix(sr, dr), mix(sg, dg), mix(sb, db)])
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
    let m = &place.matrix;
    let Some(inv) = m.inverse() else { return };
    if w <= 0 || h <= 0 {
        return;
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
    if x0 >= x1 || y0 >= y1 {
        return;
    }
    for py in y0 as u32..y1 as u32 {
        for px in x0 as u32..x1 as u32 {
            let (lx, ly) = inv.apply(f64::from(px) + 0.5, f64::from(py) + 0.5);
            if lx < 0.0 || ly < 0.0 || lx >= w || ly >= h {
                continue;
            }
            let src = sample(lx, ly);
            if src >> 24 == 0 {
                continue;
            }
            let Some(dst) = fb.get(px, py) else { continue };
            fb.set(
                px,
                py,
                if place.add {
                    add(dst, src)
                } else {
                    over(dst, src)
                },
            );
        }
    }
}

/// The smallest and largest x and y that `m` takes the corners of the
/// rectangle from `(x0, y0)` to `(x1, y1)` to.
fn bounds(m: &Affine, (x0, y0, x1, y1): (f64, f64, f64, f64)) -> (f64, f64, f64, f64) {
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
}

impl Default for Layer {
    fn default() -> Self {
        Layer {
            compose: Compose::Direct,
            color: 0xFFFF_FFFF,
            clip: None,
            matrix: Affine::IDENTITY,
        }
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
