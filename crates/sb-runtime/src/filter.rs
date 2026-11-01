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
            // Amount 0 leaves the layer as it is, not halved.
            Filter::None | Filter::Blur(0) => {}
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
            } => color(fb, *hue, *saturation, *value),
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
        /// The frame being made, kept between frames so as not to
        /// allocate a frame's worth for every layer every frame.
        static OUT: std::cell::RefCell<Vec<u32>> = const { std::cell::RefCell::new(Vec::new()) };
    }
    OUT.with_borrow_mut(|out| {
        out.clear();
        out.resize(fb.pixels().len(), 0);
        raster_into(fb, out, rows, vertical, repeat);
        fb.swap_pixels(out);
    });
}

/// Raster deformation of `fb`, written into `out`, which is as large.
fn raster_into(fb: &Framebuffer, out: &mut [u32], rows: &[[f64; 4]], vertical: bool, repeat: bool) {
    let source = fb.pixels();
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
    // Where a pixel's center lands in the source, across and down, each
    // from the entry's scale and its offset in pixels.
    let across_at = |x: usize, sx: f64, off: f64| sx * (x as f64 + 0.5) + off;
    let down_at = |y: usize, sy: f64, off: f64| fh - (sy * (fh - (y as f64 + 0.5)) + off);
    let across = |x: usize, [ox, sx, _, _]: [f64; 4]| wrap(across_at(x, sx, ox * fw), w);
    let down = |y: usize, [_, _, oy, sy]: [f64; 4]| wrap(down_at(y, sy, oy * fh), h);
    // Each line of points moves steadily one way, so when both its ends
    // land on the frame, all of it does, and each point rounds down by a
    // plain cast: the fast way, with the same points.
    let inside = |a: f64, b: f64, n: usize| {
        let n = n as f64;
        (0.0..n).contains(&a) && (0.0..n).contains(&b)
    };
    if vertical {
        // Each column takes its own entry.
        struct Column {
            from: Option<usize>,
            sy: f64,
            off: f64,
            inside: bool,
        }
        let columns: Vec<Column> = (0..w)
            .map(|x| {
                let [_, _, oy, sy] = entry(x, w);
                let off = oy * fh;
                Column {
                    from: across(x, entry(x, w)),
                    sy,
                    off,
                    inside: inside(down_at(0, sy, off), down_at(h - 1, sy, off), h),
                }
            })
            .collect();
        for (y, row) in out.chunks_exact_mut(w).enumerate() {
            for (dst, c) in row.iter_mut().zip(&columns) {
                *dst = match c.from {
                    Some(a) if c.inside => source[down_at(y, c.sy, c.off) as usize * w + a],
                    Some(a) => wrap(down_at(y, c.sy, c.off), h).map_or(0, |b| source[b * w + a]),
                    None => 0,
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
    for (y, row) in out.chunks_exact_mut(w).enumerate() {
        let e = entry(y, h);
        let Some(b) = down(y, e).filter(|&b| !blank[b]) else {
            row.fill(0);
            continue;
        };
        let from = &source[b * w..][..w];
        let [ox, sx, _, _] = e;
        if ox == 0.0 && sx == 1.0 {
            row.copy_from_slice(from);
            continue;
        }
        let off = ox * fw;
        if inside(across_at(0, sx, off), across_at(w - 1, sx, off), w) {
            for (x, dst) in row.iter_mut().enumerate() {
                *dst = from[across_at(x, sx, off) as usize];
            }
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
    // The four channels side by side in 32-bit lanes, so that the running
    // sum takes one add and one subtract a pixel.
    let lanes = |p: u32| {
        let p = u128::from(p);
        p & 0xFF | (p >> 8 & 0xFF) << 32 | (p >> 16 & 0xFF) << 64 | (p >> 24) << 96
    };
    let mean = |sum: u128| {
        let ch = |k: u32| {
            (((sum >> (32 * k)) as u32)
                .wrapping_mul(inv)
                .wrapping_add(1 << 23)
                >> 24)
                .min(255)
        };
        ch(0) | ch(1) << 8 | ch(2) << 16 | ch(3) << 24
    };
    for row in half.chunks_exact_mut(w) {
        line.clear();
        line.extend_from_slice(row);
        let get = |i: usize| line[i.min(w - 1)];
        // Before the row, its first pixel again.
        let mut sum = lanes(line[0]) * r as u128 + (0..=r).map(|i| lanes(get(i))).sum::<u128>();
        let mut out = mean(sum);
        for (i, o) in row.iter_mut().enumerate() {
            *o = out;
            // Where the same color comes in as goes out, as across most
            // of a frame, the sum and so the mean stay as they are.
            let (enter, leave) = (get(i + r + 1), line[i.saturating_sub(r)]);
            if enter != leave {
                sum += lanes(enter);
                sum -= lanes(leave);
                out = mean(sum);
            }
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
    let mean =
        |s: &[u32; 4]| u32::from_be_bytes(s.map(|s| ((s * inv + (1 << 23)) >> 24).min(255) as u8));
    let mut sum = vec![[0u32; 4]; w];
    for k in -(r as isize)..=r as isize {
        for (s, &p) in sum.iter_mut().zip(row(k)) {
            let c = channels(p);
            (0..4).for_each(|ch| s[ch] += c[ch]);
        }
    }
    let mut means: Vec<u32> = sum.iter().map(mean).collect();
    for (y, out) in half.chunks_exact_mut(w).enumerate() {
        let y = y as isize;
        out.copy_from_slice(&means);
        let (enter, leave) = (row(y + r as isize + 1), row(y - r as isize));
        for (((s, m), &a), &b) in sum.iter_mut().zip(&mut means).zip(enter).zip(leave) {
            // The same color coming in as going out leaves the mean.
            if a == b {
                continue;
            }
            let (a, b) = (channels(a), channels(b));
            (0..4).for_each(|ch| s[ch] = s[ch] + a[ch] - b[ch]);
            *m = mean(s);
        }
    }
}

/// The channels of a color, one to each 16 bits: blue, red, green and
/// alpha from the bottom.
#[inline]
fn spread(c: u32) -> u64 {
    let c = u64::from(c);
    c & 0x00FF_00FF | (c >> 8 & 0x00FF_00FF) << 32
}

/// What [`box_rows`] and [`box_columns`] make of each sum of a box `r`
/// each way, by the sum.
fn box_means(r: usize) -> Vec<u8> {
    let n = 2 * r as u32 + 1;
    let inv = (1u32 << 24) / n;
    (0..=n * 255)
        .map(|s| ((s * inv + (1 << 23)) >> 24).min(255) as u8)
        .collect()
}

/// The color whose channels are the means of the sums in [`spread`]'s
/// lanes, from [`box_means`].
#[inline]
fn box_mean(means: &[u8], sum: u64) -> u32 {
    let m = |lane: u32| u32::from(means[(sum >> (16 * lane) & 0xFFFF) as usize]);
    m(0) | m(2) << 8 | m(1) << 16 | m(3) << 24
}

/// [`box_rows`] for a box narrow enough that its sums fit [`spread`]'s
/// lanes, with one added before one is taken away.
fn box_rows_narrow(half: &mut [u32], w: usize, r: usize, means: &[u8], line: &mut Vec<u32>) {
    for row in half.chunks_exact_mut(w) {
        line.clear();
        line.extend_from_slice(row);
        let get = |i: usize| line[i.min(w - 1)];
        // Before the row, its first pixel again.
        let mut sum = spread(line[0]) * r as u64 + (0..=r).map(|i| spread(get(i))).sum::<u64>();
        let mut out = box_mean(means, sum);
        for (i, o) in row.iter_mut().enumerate() {
            *o = out;
            // Where the same color comes in as goes out, as across most
            // of a frame, the sum and so the mean stay as they are.
            let (enter, leave) = (get(i + r + 1), line[i.saturating_sub(r)]);
            if enter != leave {
                sum = sum + spread(enter) - spread(leave);
                out = box_mean(means, sum);
            }
        }
    }
}

/// [`box_columns`] for a box narrow enough that its sums fit
/// [`spread`]'s lanes, with one added before one is taken away.
fn box_columns_narrow(half: &mut [u32], w: usize, r: usize, means: &[u8], copy: &mut Vec<u32>) {
    let h = half.len() / w;
    copy.clear();
    copy.extend_from_slice(half);
    let row = |y: isize| {
        let y = y.clamp(0, h as isize - 1) as usize;
        &copy[y * w..y * w + w]
    };
    let mut sum = vec![0u64; w];
    for k in -(r as isize)..=r as isize {
        for (s, &p) in sum.iter_mut().zip(row(k)) {
            *s += spread(p);
        }
    }
    let mut out: Vec<u32> = sum.iter().map(|&s| box_mean(means, s)).collect();
    for (y, to) in half.chunks_exact_mut(w).enumerate() {
        let y = y as isize;
        to.copy_from_slice(&out);
        let (enter, leave) = (row(y + r as isize + 1), row(y - r as isize));
        for (((s, o), &a), &b) in sum.iter_mut().zip(&mut out).zip(enter).zip(leave) {
            // The same color coming in as going out leaves the mean.
            if a != b {
                *s = *s + spread(a) - spread(b);
                *o = box_mean(means, *s);
            }
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
    // The sum of n pixels / n with halves rounded down, both lanes at
    // once; n is 1, 2 or 4, so a shift divides, and what the shift brings
    // down from the lane above is masked off.
    let mean =
        |sum: u32, n: u32| (2 * sum + (n - 1) * 0x0001_0001) >> (2 * n).trailing_zeros() & LO;
    let block = |ps: &[u32], n: u32| {
        let lo = ps.iter().map(|p| p & LO).sum();
        let hi = ps.iter().map(|p| p >> 8 & LO).sum();
        mean(lo, n) | mean(hi, n) << 8
    };
    for (hy, out) in half.chunks_exact_mut(hw).enumerate() {
        let (y0, y1) = (hy * 2, (hy * 2 + 1).min(h - 1));
        let (r0, r1) = (&pixels[y0 * w..y0 * w + w], &pixels[y1 * w..y1 * w + w]);
        // Edge blocks count each pixel once.
        let pairs = (r0.chunks_exact(2).zip(r1.chunks_exact(2))).zip(out.iter_mut());
        if y1 != y0 {
            for ((a, b), o) in pairs {
                *o = block(&[a[0], a[1], b[0], b[1]], 4);
            }
        } else {
            for ((a, _), o) in pairs {
                *o = block(a, 2);
            }
        }
        if w % 2 == 1 {
            let n = 1 + u32::from(y1 != y0);
            out[hw - 1] = block(&[r0[w - 1], r1[w - 1]][..n as usize], n);
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
            // Sums of a box's channels fit 16 bits up to this width, so
            // all four can be added at once.
            if (2 * b + 2) * 255 <= 0xFFFF {
                let means = box_means(b);
                for _ in 0..2 {
                    box_rows_narrow(&mut half, hw, b, &means, &mut scratch);
                    box_columns_narrow(&mut half, hw, b, &means, &mut scratch);
                }
            } else {
                for _ in 0..2 {
                    box_rows(&mut half, hw, b, &mut scratch);
                    box_columns(&mut half, hw, b, &mut scratch);
                }
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

/// Color conversion over the frame. A screen has few colors, so each is
/// worked out once and kept in a small table by its bits.
fn color(fb: &mut Framebuffer, hue: f64, saturation: f64, value: f64) {
    // Left at zero, as programs leave it when an effect ends, it changes
    // nothing: each pixel comes back as it was.
    if hue.rem_euclid(360.0) == 0.0 && saturation == 0.0 && value == 0.0 {
        return;
    }
    const SLOTS: usize = 1 << 10;
    // Slot 0's key, 0, is clear and stays clear, so the table starts
    // right.
    let mut table = [(0u32, 0u32); SLOTS];
    for p in fb.pixels_mut() {
        let key = *p;
        // Fibonacci hashing: the top bits of the product mix all of the
        // key's.
        let slot = (key.wrapping_mul(0x9E37_79B9) >> (32 - SLOTS.trailing_zeros())) as usize;
        let (from, to) = &mut table[slot];
        if *from != key {
            *from = key;
            *to = shift_color(key, hue, saturation, value);
        }
        *p = *to;
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

    #[test]
    fn narrow_boxes_blur_as_wide_ones() {
        let mut seed = 0x0BAD_F00D_u32;
        let mut next = || {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            seed
        };
        let (w, h) = (37, 29);
        // Runs of one color, as frames have, and noise.
        let pixels: Vec<u32> = (0..w * h)
            .map(|i| match i % 11 < 6 {
                true => 0xFF10_2030,
                false => next(),
            })
            .collect();
        for r in [1, 3, 12, 40] {
            let (mut narrow, mut wide) = (pixels.clone(), pixels.clone());
            let mut scratch = Vec::new();
            let means = box_means(r);
            box_rows_narrow(&mut narrow, w, r, &means, &mut scratch);
            box_columns_narrow(&mut narrow, w, r, &means, &mut scratch);
            box_rows(&mut wide, w, r, &mut scratch);
            box_columns(&mut wide, w, r, &mut scratch);
            assert_eq!(narrow, wide, "r {r}");
        }
    }

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
        let mut fb = frame(&[0xFFFF_FFFF, 0, 0, 0], 4);
        Filter::Blur(0).apply(&mut fb, 1);
        assert_eq!(fb.pixels(), &[0xFFFF_FFFF, 0, 0, 0]);
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

    #[test]
    fn color_works_out_each_color() {
        // Colors that come back after others, and clear.
        let pixels = [
            0xFFFF_0000,
            0xFF00_00FF,
            0,
            0xFFFF_0000,
            0x8080_0000,
            0xFF00_00FF,
        ];
        let mut fb = frame(&pixels, 3);
        color(&mut fb, 120.0, 0.0, 0.0);
        let one_by_one = pixels.map(|p| shift_color(p, 120.0, 0.0, 0.0));
        assert_eq!(fb.pixels(), &one_by_one);
    }

    #[test]
    fn color_at_zero_keeps_the_frame() {
        let pixels = [0xFF12_3456, 0x8040_2010, 0, 0xFFFF_FFFF];
        let mut fb = frame(&pixels, 2);
        Filter::Color {
            hue: 360.0,
            saturation: -0.0,
            value: 0.0,
        }
        .apply(&mut fb, 1);
        assert_eq!(fb.pixels(), &pixels);
        // As working each one out would.
        for p in pixels {
            assert_eq!(shift_color(p, 0.0, 0.0, 0.0), p);
        }
    }
}
