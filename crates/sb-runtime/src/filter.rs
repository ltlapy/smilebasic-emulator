//! LFILTER's effects on a layer drawn on its own frame: mosaic, blur,
//! raster deformation and color conversion.
//!
//! The manual names the effects and their parameters but not how they are
//! worked out, so all of this is a guess.
// Checked on hardware with screenshots: mosaic blocks start at the
// screen's corner and take the color of their middle pixel; color
// conversion adds to the hue in degrees and to the saturation and
// brightness on HSV's 0 to 255 scale.
// UNVERIFIED: mosaic sizes are in screen pixels.

use sb_core::value::ArrayRef;

use crate::framebuffer::Framebuffer;

/// LFILTER's filter types 0 to 5.
#[derive(Clone, Debug, Default)]
pub enum Filter {
    #[default]
    None,
    Mosaic(u32),
    Blur(u32),
    /// Types 3 (each line moves sideways) and 4 (each column up and
    /// down), from rows of offset and scale, or of X offset, X scale, Y
    /// offset and Y scale, read from the program's array every frame.
    Raster {
        vertical: bool,
        info: ArrayRef,
        repeat: bool,
    },
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
            Filter::Raster { vertical, .. } => 3 + i32::from(*vertical),
            Filter::Color { .. } => 5,
        }
    }

    /// Applies the filter to a layer's premultiplied frame; `sample` is
    /// how many frame pixels make one screen pixel.
    pub fn apply(&self, fb: &mut Framebuffer, sample: u32) {
        match self {
            Filter::None => {}
            Filter::Mosaic(n) => mosaic(fb, n.saturating_mul(sample)),
            Filter::Blur(n) => blur(fb, *n),
            Filter::Raster {
                vertical,
                info,
                repeat,
            } => {
                let rows = match info.try_borrow() {
                    Ok(a) => raster_rows(&a, *vertical),
                    Err(_) => return,
                };
                raster(fb, &rows, *vertical, *repeat);
            }
            Filter::Color {
                hue,
                saturation,
                value,
            } => {
                // Neighbors are mostly the same color.
                let mut last = None;
                for p in fb.pixels_mut() {
                    *p = match last {
                        Some((from, to)) if from == *p => to,
                        _ => {
                            let to = shift_color(*p, *hue, *saturation, *value);
                            last = Some((*p, to));
                            to
                        }
                    };
                }
            }
        }
    }
}

/// A raster array's entries as X offset, X scale, Y offset and Y scale.
/// With two to an entry they are the offset and scale along the
/// deformation: X for type 3, Y for type 4.
// UNVERIFIED: the two-value form's meaning.
fn raster_rows(info: &sb_core::value::Array, vertical: bool) -> Vec<[f64; 4]> {
    let width = match info.dims[..] {
        [_, w @ (2 | 4)] => w,
        _ => return Vec::new(),
    };
    let at = |i: usize| info.get(i).to_real().unwrap_or(0.0);
    (0..info.len() / width)
        .map(|r| match (width, vertical) {
            (2, false) => [at(r * 2), at(r * 2 + 1), 0.0, 1.0],
            (2, true) => [0.0, 1.0, at(r * 2), at(r * 2 + 1)],
            _ => std::array::from_fn(|k| at(r * 4 + k)),
        })
        .collect()
}

/// Raster deformation. Each row of the frame (type 3) or column (type 4)
/// takes an entry, and each pixel shows the source pixel at its center
/// scaled and then moved by that entry, in OpenGL's way: from the left
/// for X and from the bottom for Y, offsets in widths and heights.
/// Without `repeat`, points off the frame show nothing.
// UNVERIFIED: the direction of Y offsets, and whether scale comes before
// the offset.
fn raster(fb: &mut Framebuffer, rows: &[[f64; 4]], vertical: bool, repeat: bool) {
    if rows.is_empty() {
        return;
    }
    thread_local! {
        /// The frame as it was, kept between frames so as not to
        /// allocate a frame's worth for every layer every frame.
        static SOURCE: std::cell::RefCell<Vec<u32>> = const { std::cell::RefCell::new(Vec::new()) };
    }
    SOURCE.with_borrow_mut(|source| {
        source.clear();
        source.extend_from_slice(fb.pixels());
        raster_from(fb, source, rows, vertical, repeat);
    });
}

fn raster_from(
    fb: &mut Framebuffer,
    source: &[u32],
    rows: &[[f64; 4]],
    vertical: bool,
    repeat: bool,
) {
    let (w, h) = (fb.width() as usize, fb.height() as usize);
    let (fw, fh) = (w as f64, h as f64);
    // The source point's index along a line of n, or None off it.
    let wrap = |v: f64, n: usize| -> Option<usize> {
        // NaN and the far off show nothing.
        if v.is_nan() || v.abs() >= 1e15 {
            return None;
        }
        // Rounded down without the library call f64::floor makes on
        // processors before SSE4.1.
        let t = v as i64;
        let t = t - i64::from(t as f64 > v);
        match usize::try_from(t).ok().filter(|&t| t < n) {
            Some(t) => Some(t),
            None if repeat => Some(t.rem_euclid(n as i64) as usize),
            None => None,
        }
    };
    let entry = |i: usize, n: usize| rows[i * rows.len() / n];
    // Where a pixel's center lands in the source, across and down.
    let across = |x: usize, [ox, sx, _, _]: [f64; 4]| wrap(sx * (x as f64 + 0.5) + ox * fw, w);
    let down =
        |y: usize, [_, _, oy, sy]: [f64; 4]| wrap(fh - (sy * (fh - (y as f64 + 0.5)) + oy * fh), h);
    let pixels = fb.pixels_mut();
    if vertical {
        // Each column takes its own entry.
        let columns: Vec<([f64; 4], Option<usize>)> = (0..w)
            .map(|x| {
                let e = entry(x, w);
                (e, across(x, e))
            })
            .collect();
        for y in 0..h {
            let row = &mut pixels[y * w..][..w];
            for (dst, &(e, sx)) in row.iter_mut().zip(&columns) {
                *dst = match (sx, down(y, e)) {
                    (Some(a), Some(b)) => source[b * w + a],
                    _ => 0,
                };
            }
        }
        return;
    }
    // Each row takes its own entry, so it reads one row of the source;
    // a layer of its own is mostly rows with nothing in them.
    let blank: Vec<bool> = source
        .chunks_exact(w)
        .map(|r| r.iter().all(|&p| p == 0))
        .collect();
    for y in 0..h {
        let e = entry(y, h);
        let row = &mut pixels[y * w..][..w];
        let Some(b) = down(y, e).filter(|&b| !blank[b]) else {
            row.fill(0);
            continue;
        };
        let from = &source[b * w..][..w];
        if e[0] == 0.0 && e[1] == 1.0 {
            row.copy_from_slice(from);
            continue;
        }
        for (x, dst) in row.iter_mut().enumerate() {
            *dst = across(x, e).map_or(0, |a| from[a]);
        }
    }
}

fn mosaic(fb: &mut Framebuffer, size: u32) {
    if size <= 1 {
        return;
    }
    let (w, h) = (fb.width(), fb.height());
    let pixels = fb.pixels_mut();
    let at = |x: u32, y: u32| (y * w + x) as usize;
    for by in (0..h).step_by(size as usize) {
        for bx in (0..w).step_by(size as usize) {
            let (x1, y1) = ((bx + size).min(w), (by + size).min(h));
            let color = pixels[at((bx + size / 2).min(x1 - 1), (by + size / 2).min(y1 - 1))];
            for y in by..y1 {
                pixels[at(bx, y)..at(x1, y)].fill(color);
            }
        }
    }
}

/// How much wider the blur's Gaussian is, in half-size pixels, for each
/// step of LFILTER's amount (fitted to hardware at amount 8).
const BLUR_SIGMA: f32 = 1.0 / 25.0;
/// Kernels wider than this each way are approximated by three box blurs,
/// which cost the same at any width.
const GAUSS_RADIUS: usize = 4;

fn channels(p: u32) -> [u32; 4] {
    p.to_be_bytes().map(u32::from)
}

/// A Gaussian along each row, from 8-bit channels to 8.8 fixed point;
/// `kernel` is in 16.16 and sums to one.
fn gauss_rows(half: &[u32], w: usize, kernel: &[u32], out: &mut [[u16; 4]]) {
    for (row, out) in half.chunks_exact(w).zip(out.chunks_exact_mut(w)) {
        for (i, o) in out.iter_mut().enumerate() {
            let mut acc = channels(row[i]).map(|c| c * kernel[0]);
            for (k, &wk) in kernel.iter().enumerate().skip(1) {
                let (a, b) = (
                    channels(row[i.saturating_sub(k)]),
                    channels(row[(i + k).min(w - 1)]),
                );
                (0..4).for_each(|ch| acc[ch] += (a[ch] + b[ch]) * wk);
            }
            *o = acc.map(|v| ((v + 128) >> 8) as u16);
        }
    }
}

/// A Gaussian down each column, a row at a time, back to 8 bits with
/// halves rounded down.
fn gauss_columns(v: &[[u16; 4]], w: usize, kernel: &[u32], half: &mut [u32]) {
    let h = v.len() / w;
    let row = |y: usize| &v[y * w..y * w + w];
    let mut acc = vec![[0u32; 4]; w];
    for (y, out) in half.chunks_exact_mut(w).enumerate() {
        for (a, c) in acc.iter_mut().zip(row(y)) {
            *a = c.map(|c| u32::from(c) * kernel[0]);
        }
        for (k, &wk) in kernel.iter().enumerate().skip(1) {
            let (ra, rb) = (row(y.saturating_sub(k)), row((y + k).min(h - 1)));
            for ((a, ca), cb) in acc.iter_mut().zip(ra).zip(rb) {
                (0..4).for_each(|ch| a[ch] += (u32::from(ca[ch]) + u32::from(cb[ch])) * wk);
            }
        }
        for (o, a) in out.iter_mut().zip(&acc) {
            *o = u32::from_be_bytes(a.map(|v| ((v + (1 << 23) - 1) >> 24).min(255) as u8));
        }
    }
}

/// A box blur `r` each way along each row by running sums, clamped.
fn box_rows(half: &mut [u32], w: usize, r: usize, line: &mut Vec<u32>) {
    let inv = (1u32 << 24) / (2 * r + 1) as u32;
    for row in half.chunks_exact_mut(w) {
        line.clear();
        line.extend_from_slice(row);
        let get = |i: isize| channels(line[i.clamp(0, w as isize - 1) as usize]);
        let mut sum = [0u32; 4];
        for k in -(r as isize)..=r as isize {
            let c = get(k);
            (0..4).for_each(|ch| sum[ch] += c[ch]);
        }
        for (i, o) in row.iter_mut().enumerate() {
            *o = u32::from_be_bytes(sum.map(|s| ((s * inv + (1 << 23)) >> 24).min(255) as u8));
            let (add, sub) = (
                get(i as isize + r as isize + 1),
                get(i as isize - r as isize),
            );
            (0..4).for_each(|ch| sum[ch] = sum[ch] + add[ch] - sub[ch]);
        }
    }
}

/// A box blur `r` each way down each column, a row at a time.
fn box_columns(half: &mut [u32], w: usize, r: usize, copy: &mut Vec<u32>) {
    let h = half.len() / w;
    let inv = (1u32 << 24) / (2 * r + 1) as u32;
    copy.clear();
    copy.extend_from_slice(half);
    let row = |y: isize| {
        let y = y.clamp(0, h as isize - 1) as usize;
        &copy[y * w..y * w + w]
    };
    let mut sum = vec![[0u32; 4]; w];
    for k in -(r as isize)..=r as isize {
        for (s, &p) in sum.iter_mut().zip(row(k)) {
            let c = channels(p);
            (0..4).for_each(|ch| s[ch] += c[ch]);
        }
    }
    for (y, out) in half.chunks_exact_mut(w).enumerate() {
        let y = y as isize;
        for (o, s) in out.iter_mut().zip(&sum) {
            *o = u32::from_be_bytes(s.map(|s| ((s * inv + (1 << 23)) >> 24).min(255) as u8));
        }
        for ((s, &a), &b) in sum
            .iter_mut()
            .zip(row(y + r as isize + 1))
            .zip(row(y - r as isize))
        {
            let (a, b) = (channels(a), channels(b));
            (0..4).for_each(|ch| s[ch] = s[ch] + a[ch] - b[ch]);
        }
    }
}

/// LFILTER's blur: the layer is averaged down to half size in 2x2
/// blocks, kept in 8 bits, blurred there by a Gaussian, and drawn back at
/// full size without smoothing, so that even amount 1 turns a dot into a
/// 2x2 block of a quarter of its light. Halves round down. All of this
/// matches hardware screenshots at amounts 1, 2, 4 and 8 pixel for pixel.
/// It is done in integers, as it runs every frame.
// UNVERIFIED: wider blurs, approximated here by two box blurs; the
// halving is in frame pixels whatever XSCREEN's sampling.
fn blur(fb: &mut Framebuffer, amount: u32) {
    let (w, h) = (fb.width() as usize, fb.height() as usize);
    if w == 0 || h == 0 {
        return;
    }
    let (hw, hh) = (w.div_ceil(2), h.div_ceil(2));
    let pixels = fb.pixels_mut();
    let mut half = vec![0u32; hw * hh];
    // Two channels to a word: sums of four 8-bit values fit 16 bits.
    const LO: u32 = 0x00FF_00FF;
    for (hy, out) in half.chunks_exact_mut(hw).enumerate() {
        let (y0, y1) = (hy * 2, (hy * 2 + 1).min(h - 1));
        let (r0, r1) = (&pixels[y0 * w..y0 * w + w], &pixels[y1 * w..y1 * w + w]);
        for (hx, o) in out.iter_mut().enumerate() {
            let (x0, x1) = (hx * 2, (hx * 2 + 1).min(w - 1));
            let ps = [r0[x0], r0[x1], r1[x0], r1[x1]];
            let n = (1 + u32::from(x1 != x0)) * (1 + u32::from(y1 != y0));
            let (mut lo, mut hi) = (0, 0);
            // Edge blocks count each pixel once.
            for (k, p) in ps.iter().enumerate() {
                let counted = (k & 1 == 0 || x1 != x0) && (k < 2 || y1 != y0);
                if counted {
                    lo += p & LO;
                    hi += (p >> 8) & LO;
                }
            }
            // sum / n with halves rounded down, lane by lane.
            let div = |v: u32| {
                let (a, b) = (v & 0xFFFF, v >> 16);
                let f = |s: u32| (2 * s + n - 1) / (2 * n);
                f(a) | f(b) << 16
            };
            *o = div(lo) | div(hi) << 8;
        }
    }
    let sigma = amount as f32 * BLUR_SIGMA;
    let r = (3.0 * sigma).ceil() as usize;
    let raw: Vec<f32> = (0..=r.max(1))
        .map(|k| (-((k * k) as f32) / (2.0 * sigma * sigma)).exp())
        .collect();
    let total = raw[0] + 2.0 * raw[1..].iter().sum::<f32>();
    // A spread that cannot move an 8-bit value is left out.
    if raw[1] / total * 255.0 >= 0.25 {
        if r > GAUSS_RADIUS {
            // Two boxes of this width make about the same spread.
            let b = (((6.0 * sigma * sigma + 1.0).sqrt() - 1.0) / 2.0)
                .round()
                .max(1.0) as usize;
            let mut scratch = Vec::new();
            for _ in 0..2 {
                box_rows(&mut half, hw, b, &mut scratch);
                box_columns(&mut half, hw, b, &mut scratch);
            }
        } else {
            let mut kernel: Vec<u32> = raw
                .iter()
                .map(|w| (w / total * 65536.0).round() as u32)
                .collect();
            let rest: u32 = 2 * kernel[1..].iter().sum::<u32>();
            kernel[0] = 65536 - rest;
            let mut v = vec![[0u16; 4]; hw * hh];
            gauss_rows(&half, hw, &kernel, &mut v);
            gauss_columns(&v, hw, &kernel, &mut half);
        }
    }
    // Back to full size, each half row making two rows.
    let mut row = vec![0u32; w];
    for (hy, src) in half.chunks_exact(hw).enumerate() {
        for (x, p) in row.iter_mut().enumerate() {
            *p = src[x / 2];
        }
        for y in hy * 2..(hy * 2 + 2).min(h) {
            pixels[y * w..y * w + w].copy_from_slice(&row);
        }
    }
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
    fn mosaic_takes_the_middle_pixel() {
        let mut fb = frame(&[1, 2, 3, 4, 5, 6, 7, 8, 9], 3);
        Filter::Mosaic(2).apply(&mut fb, 1);
        assert_eq!(fb.pixels(), &[5, 5, 6, 5, 5, 6, 8, 8, 9]);
    }

    #[test]
    fn blur_halves_the_layer() {
        let mut fb = frame(&[0xFFFF_FFFF, 0, 0, 0, 0, 0, 0, 0], 4);
        Filter::Blur(1).apply(&mut fb, 1);
        assert_eq!(&fb.pixels()[..4], &[0x4040_4040, 0x4040_4040, 0, 0]);
        let mut fb = frame(&[0xFFFF_FFFF, 0, 0, 0, 0xFFFF_FFFF, 0, 0, 0], 4);
        Filter::Blur(1).apply(&mut fb, 1);
        assert_eq!(fb.pixels()[1], 0x7F7F_7F7F);
    }

    #[test]
    fn raster_moves_lines() {
        use sb_core::value::{Array, ArrayData, ElemType};
        let mut info = Array::new(ElemType::Real, vec![2, 2]);
        // The first line takes from a quarter further right, the second
        // stays.
        info.data = ArrayData::Real(vec![0.25, 1.0, 0.0, 1.0]);
        let info = std::rc::Rc::new(std::cell::RefCell::new(info));
        let row = [1, 2, 3, 4];
        let mut fb = frame(&[row, row].concat(), 4);
        let filter = |repeat| Filter::Raster {
            vertical: false,
            info: info.clone(),
            repeat,
        };
        filter(false).apply(&mut fb, 1);
        assert_eq!(fb.pixels(), &[2, 3, 4, 0, 1, 2, 3, 4]);
        let mut fb = frame(&[row, row].concat(), 4);
        filter(true).apply(&mut fb, 1);
        assert_eq!(&fb.pixels()[..4], &[2, 3, 4, 1]);
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
