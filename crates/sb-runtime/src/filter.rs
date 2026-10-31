//! LFILTER's effects on a layer drawn on its own frame: mosaic, blur and
//! color conversion. The manual's raster deformation (types 3 and 4) is
//! not in SmileBASIC 4.4.9.
//!
//! The manual names the effects and their parameters but not how they are
//! worked out, so all of this is a guess.
// UNVERIFIED: sizes are in screen pixels; mosaic and blur average each
// block or window; color conversion adds to the hue in degrees and to the
// saturation and brightness on HSV's 0 to 255 scale.

use crate::framebuffer::Framebuffer;

/// LFILTER's filter types 0 to 5.
#[derive(Clone, Debug, Default)]
pub enum Filter {
    #[default]
    None,
    Mosaic(u32),
    Blur(u32),
    Color {
        hue: f64,
        saturation: f64,
        value: f64,
    },
}

impl Filter {
    pub fn kind(&self) -> i32 {
        match self {
            Filter::None => 0,
            Filter::Mosaic(_) => 1,
            Filter::Blur(_) => 2,
            Filter::Color { .. } => 5,
        }
    }

    /// Applies the filter to a layer's premultiplied frame; `sample` is
    /// how many frame pixels make one screen pixel.
    pub fn apply(&self, fb: &mut Framebuffer, sample: u32) {
        match self {
            Filter::None => {}
            Filter::Mosaic(n) => mosaic(fb, n.saturating_mul(sample)),
            Filter::Blur(n) => blur(fb, n.saturating_mul(sample)),
            Filter::Color {
                hue,
                saturation,
                value,
            } => {
                for p in fb.pixels_mut() {
                    *p = shift_color(*p, *hue, *saturation, *value);
                }
            }
        }
    }
}

/// The average of premultiplied pixels.
fn average(pixels: impl Iterator<Item = u32>) -> u32 {
    let (mut sum, mut n) = ([0u64; 4], 0u64);
    for p in pixels {
        for (s, c) in sum.iter_mut().zip(p.to_be_bytes()) {
            *s += u64::from(c);
        }
        n += 1;
    }
    if n == 0 {
        return 0;
    }
    u32::from_be_bytes(sum.map(|s| ((s + n / 2) / n) as u8))
}

fn mosaic(fb: &mut Framebuffer, size: u32) {
    if size <= 1 {
        return;
    }
    let (w, h) = (fb.width(), fb.height());
    let pixels = fb.pixels_mut();
    for by in (0..h).step_by(size as usize) {
        for bx in (0..w).step_by(size as usize) {
            let (x1, y1) = ((bx + size).min(w), (by + size).min(h));
            let at = |x: u32, y: u32| (y * w + x) as usize;
            let block = (by..y1).flat_map(|y| (bx..x1).map(move |x| (x, y)));
            let color = average(block.clone().map(|(x, y)| pixels[at(x, y)]));
            for (x, y) in block {
                pixels[at(x, y)] = color;
            }
        }
    }
}

/// One pass of a box blur `radius` pixels each way along rows of `len`
/// pixels `stride` apart, the window shrinking at the edges.
fn box_pass(pixels: &mut [u32], lines: u32, len: u32, step: (usize, usize), radius: u32) {
    let (line_step, stride) = step;
    let mut row = vec![0u32; len as usize];
    for l in 0..lines as usize {
        for (i, r) in row.iter_mut().enumerate() {
            *r = pixels[l * line_step + i * stride];
        }
        let mut sums = vec![[0u64; 4]; len as usize + 1];
        for (i, p) in row.iter().enumerate() {
            let c = p.to_be_bytes();
            sums[i + 1] = std::array::from_fn(|k| sums[i][k] + u64::from(c[k]));
        }
        for i in 0..len as usize {
            let lo = i.saturating_sub(radius as usize);
            let hi = (i + radius as usize + 1).min(len as usize);
            let n = (hi - lo) as u64;
            let c = std::array::from_fn(|k| ((sums[hi][k] - sums[lo][k] + n / 2) / n) as u8);
            pixels[l * line_step + i * stride] = u32::from_be_bytes(c);
        }
    }
}

fn blur(fb: &mut Framebuffer, radius: u32) {
    if radius == 0 {
        return;
    }
    let (w, h) = (fb.width(), fb.height());
    let pixels = fb.pixels_mut();
    box_pass(pixels, h, w, (w as usize, 1), radius);
    box_pass(pixels, w, h, (1, w as usize), radius);
}

/// A premultiplied pixel with HSV's hue, saturation and value moved.
fn shift_color(p: u32, hue: f64, saturation: f64, value: f64) -> u32 {
    let [a, r, g, b] = p.to_be_bytes().map(f64::from);
    if a == 0.0 {
        return p;
    }
    let straight = |c: f64| (c * 255.0 / a).min(255.0);
    let (r, g, b) = (straight(r), straight(g), straight(b));
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let d = max - min;
    let h = match d {
        0.0 => 0.0,
        _ if max == r => 60.0 * ((g - b) / d),
        _ if max == g => 60.0 * ((b - r) / d + 2.0),
        _ => 60.0 * ((r - g) / d + 4.0),
    };
    let s = if max == 0.0 { 0.0 } else { d / max * 255.0 };
    let h = (h + hue).rem_euclid(360.0);
    let s = (s + saturation).clamp(0.0, 255.0) / 255.0;
    let v = (max + value).clamp(0.0, 255.0);
    let c = v * s;
    let x = c * (1.0 - ((h / 60.0) % 2.0 - 1.0).abs());
    let (r, g, b) = match (h / 60.0) as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = v - c;
    let pre = |c: f64| ((c + m) * a / 255.0).round().clamp(0.0, 255.0) as u8;
    u32::from_be_bytes([a as u8, pre(r), pre(g), pre(b)])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(pixels: &[u32], w: u32) -> Framebuffer {
        let mut fb = Framebuffer::new(w, pixels.len() as u32 / w);
        fb.pixels_mut().copy_from_slice(pixels);
        fb
    }

    #[test]
    fn mosaic_averages_blocks() {
        let mut fb = frame(&[0xFF00_0000, 0xFF00_00FF, 0xFF00_00FF, 0xFF00_0000], 2);
        Filter::Mosaic(2).apply(&mut fb, 1);
        assert!(fb.pixels().iter().all(|&p| p == 0xFF00_0080));
    }

    #[test]
    fn blur_spreads() {
        let mut fb = frame(&[0, 0, 0xFFFF_FFFF, 0, 0], 5);
        Filter::Blur(1).apply(&mut fb, 1);
        assert_eq!(fb.pixels()[1], 0x5555_5555);
        assert_eq!(fb.pixels()[0], 0);
    }

    #[test]
    fn color_turns_hue() {
        let mut fb = frame(&[0xFFFF_0000, 0x8080_0000], 2);
        Filter::Color {
            hue: 120.0,
            saturation: 0.0,
            value: 0.0,
        }
        .apply(&mut fb, 1);
        assert_eq!(fb.pixels(), &[0xFF00_FF00, 0x8000_8000]);
    }
}
