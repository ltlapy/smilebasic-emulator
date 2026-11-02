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

use crate::compose::Compose;
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
    /// how many frame pixels make one screen pixel, and `mode` how the
    /// layer is composed afterwards.
    pub fn apply(&self, fb: &mut Framebuffer, sample: u32, mode: Compose) {
        match self {
            // Amount 0 leaves the layer as it is, not halved.
            Filter::None | Filter::Blur(0) => {}
            Filter::Mosaic(n) => mosaic(fb, *n, sample),
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
            } => color(fb, *hue, *saturation, *value, mode),
        }
    }
}

/// A raster array's entries as X offset, X scale, Y offset and Y scale.
/// With two to an entry they are the offset and scale along the
/// deformation: X for type 3, Y for type 4.
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
/// takes the entry its center falls in, and each pixel shows the source
/// pixel at its center scaled and then moved by that entry, in OpenGL's
/// way: from the left for X and from the bottom for Y, offsets in widths
/// and heights. Without `repeat`, points off the frame show nothing.
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
    let entry = |i: usize, n: usize| rows[(2 * i + 1) * rows.len() / (2 * n)];
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

/// One texel coordinate of bilinear sampling: the two texels on either
/// side and how much of the second, in 256ths.
#[derive(Clone, Copy)]
struct Tap {
    first: u32,
    second: u32,
    part: u32,
}

/// The half-size frames that mosaic and blur work in and the lookups in
/// and out of them, kept between frames so as not to allocate them for
/// every filtered layer every frame.
#[derive(Default)]
struct Half {
    pixels: Vec<u32>,
    across: Vec<Tap>,
    down: Vec<Tap>,
    out_across: Vec<u32>,
    expanded: Vec<u32>,
}

thread_local! {
    static HALF: std::cell::RefCell<Half> = std::cell::RefCell::default();
}

/// Flips the alpha channel from the coverage we keep to how much shows
/// through, and back: the GPU's frames hold the latter, so that is what
/// filters average and scale.
const THROUGH: u32 = 0xFF00_0000;

/// Where the centers of `count` half-size pixels, stretched over `q` of
/// them so that they span a line of `size`, fall on that line, from the
/// line's end if `flip`.
fn taps(out: &mut Vec<Tap>, size: u32, q: f64, count: usize, flip: bool) {
    let last = i64::from(size) - 1;
    let n = f64::from(size);
    out.clear();
    out.extend((0..count).map(|i| {
        // Rows count from the bottom on the GPU, but the half-size frame
        // here is kept top first.
        let i = if flip { count - 1 - i } else { i };
        let along = (i as f64 + 0.5) / q * n;
        let at = if flip { n - along } else { along } - 0.5;
        // Rounded down without the library call f64::floor makes on
        // processors before SSE4.1.
        let t = at as i64;
        let t = t - i64::from((t as f64) > at);
        let part = ((at - t as f64) * 256.0 + 0.5) as i64;
        let (t, part) = if part == 256 { (t + 1, 0) } else { (t, part) };
        Tap {
            first: t.clamp(0, last) as u32,
            second: (t + 1).clamp(0, last) as u32,
            part: part as u32,
        }
    }));
}

/// Draws the frame into `half.pixels`, `hw` wide, by bilinear sampling at
/// the texels `half.across` and `half.down` give. Channels round to the
/// nearest, with halves down.
fn shrink(half: &mut Half, source: &[u32], w: usize, hw: usize) {
    const LO: u32 = 0x00FF_00FF;
    let at = |x: u32, row: &[u32]| row[x as usize] ^ THROUGH;
    // Exactly between four texels, as with blocks of an even size: the
    // sum of four over four, two channels of a word at once.
    let mean = |sum: u32| (2 * sum + 3 * 0x0001_0001) >> 3 & LO;
    let ties = half.across.iter().all(|t| t.part == 128);
    let on_texels = half.across.iter().all(|t| t.part == 0);
    let Half {
        pixels,
        across,
        down,
        ..
    } = half;
    for (out, y) in pixels.chunks_mut(hw).zip(down.iter()) {
        let r0 = &source[y.first as usize * w..][..w];
        let r1 = &source[y.second as usize * w..][..w];
        if on_texels && y.part == 0 {
            for (o, t) in out.iter_mut().zip(across.iter()) {
                *o = at(t.first, r0);
            }
        } else if ties && y.part == 128 {
            for (o, t) in out.iter_mut().zip(across.iter()) {
                let (a, b) = (at(t.first, r0), at(t.second, r0));
                let (c, d) = (at(t.first, r1), at(t.second, r1));
                *o = a;
                if a != b || a != c || a != d {
                    let lo = (a & LO) + (b & LO) + (c & LO) + (d & LO);
                    let hi = (a >> 8 & LO) + (b >> 8 & LO) + (c >> 8 & LO) + (d >> 8 & LO);
                    *o = mean(lo) | mean(hi) << 8;
                }
            }
        } else {
            for (o, t) in out.iter_mut().zip(across.iter()) {
                let (a, b) = (at(t.first, r0), at(t.second, r0));
                let (c, d) = (at(t.first, r1), at(t.second, r1));
                *o = a;
                if a != b || a != c || a != d {
                    let (wx, wy) = (t.part, y.part);
                    let top = |p: u32, q: u32| p * (256 - wx) + q * wx;
                    *o = u32::from_le_bytes(std::array::from_fn(|k| {
                        let s = 8 * k as u32;
                        let ch = |p: u32| p >> s & 0xFF;
                        let row0 = top(ch(a), ch(b));
                        let row1 = top(ch(c), ch(d));
                        ((row0 * (256 - wy) + row1 * wy + 0x7FFF) >> 16) as u8
                    }));
                }
            }
        }
    }
}

/// Draws `half.pixels`, `hw` wide, back over the whole `w` x `h` frame
/// without smoothing, `q` of its pixels by `qy` of its rows spanning the
/// frame. Where a pixel's center is exactly on the line between two rows,
/// it takes the one nearer the frame's bottom, as rows count from there
/// on the GPU.
fn expand(half: &mut Half, pixels: &mut [u32], (w, h): (u32, u32), (q, qy): (f64, f64), hw: usize) {
    let (uw, count) = (w as usize, half.down.len());
    half.out_across.clear();
    let per_x = q / f64::from(w);
    half.out_across.extend((0..w).map(|x| {
        let i = (f64::from(x) + 0.5) * per_x;
        (i as usize).min(hw - 1) as u32
    }));
    half.expanded.clear();
    half.expanded.resize(uw, 0);
    let mut shown = usize::MAX;
    let per_y = qy / f64::from(h);
    for (y, line) in pixels.chunks_exact_mut(uw).enumerate() {
        let from_bottom = (f64::from(h) - y as f64 - 0.5) * per_y;
        let row = count - 1 - (from_bottom as usize).min(count - 1);
        if row != shown {
            shown = row;
            let src = &half.pixels[row * hw..][..hw];
            for (o, &x) in half.expanded.iter_mut().zip(&half.out_across) {
                *o = src[x as usize] ^ THROUGH;
            }
        }
        line.copy_from_slice(&half.expanded);
    }
}

/// LFILTER's mosaic: a whole frame drawn small, into as many pixels of the
/// half-size frame as blocks fit, and back at full size without smoothing.
/// The number of blocks is reckoned in screen pixels, a block's size in
/// frame pixels. Drawing small samples with smoothing, so that a block of
/// an even size is the mean of the four pixels around its middle.
fn mosaic(fb: &mut Framebuffer, size: u32, sample: u32) {
    let (w, h) = (fb.width(), fb.height());
    let (sw, sh) = (w / sample.max(1), h / sample.max(1));
    let (hw, hh) = (w / 2, h / 2);
    if size < 2 || hw == 0 || hh == 0 || sw < 2 || sh < 2 {
        return;
    }
    let span = |screen: u32, half_size: u32| {
        f64::from(screen.div_ceil(size)) * f64::from(half_size) / f64::from(screen / 2)
    };
    let (q, qy) = (span(sw, hw), span(sh, hh));
    let across = (q.ceil() as usize).min(hw as usize);
    let down = (qy.ceil() as usize).min(hh as usize);
    HALF.with_borrow_mut(|half| {
        half.pixels.clear();
        half.pixels.resize(across * down, 0);
        taps(&mut half.across, w, q, across, false);
        taps(&mut half.down, h, qy, down, true);
        shrink(half, fb.pixels(), w as usize, across);
        expand(half, fb.pixels_mut(), (w, h), (q, qy), across);
    });
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
/// worked out once and kept in a small table by its bits. A clear pixel
/// stays clear only on a layer composed in mode 1; otherwise a raised
/// value lights it, so that the filter shows all over the layer.
fn color(fb: &mut Framebuffer, hue: f64, saturation: f64, value: f64, mode: Compose) {
    // Left at zero, as programs leave it when an effect ends, it changes
    // nothing: each pixel comes back as it was.
    if hue.rem_euclid(360.0) == 0.0 && saturation == 0.0 && value == 0.0 {
        return;
    }
    let shift = [hue / 360.0, saturation / 255.0, value / 255.0].map(|v| v as f32);
    let clear = match mode {
        Compose::Normal => 0,
        _ => shift_color(0, shift),
    };
    const SLOTS: usize = 1 << 10;
    // Slot 0's key, 0, is the clear pixel, so the table starts right.
    let mut table = [(0u32, clear); SLOTS];
    for p in fb.pixels_mut() {
        let key = *p;
        // Fibonacci hashing: the top bits of the product mix all of the
        // key's.
        let slot = (key.wrapping_mul(0x9E37_79B9) >> (32 - SLOTS.trailing_zeros())) as usize;
        let (from, to) = &mut table[slot];
        if *from != key {
            *from = key;
            *to = match key {
                0 => clear,
                _ => shift_color(key, shift),
            };
        }
        *p = *to;
    }
}

/// A premultiplied pixel with HSV's hue, saturation and value moved by
/// `[h, s, v]`, each as a part of its whole range. The conversion works on
/// the premultiplied color as it is and in single precision, with the
/// usual GPU formulas; a grey has no saturation to raise. The coverage
/// stays.
fn shift_color(p: u32, [dh, ds, dv]: [f32; 3]) -> u32 {
    const EPSILON: f32 = 1e-10;
    let fract = |v: f32| v - v.floor();
    let [a, r, g, b] = p.to_be_bytes();
    let [r, g, b] = [r, g, b].map(|c| f32::from(c) / 255.0);
    let (px, py, pz, pw) = match g < b {
        true => (b, g, -1.0, 2.0 / 3.0),
        false => (g, b, 0.0, -1.0 / 3.0),
    };
    let (qx, qy, qz, qw) = match r < px {
        true => (px, py, pw, r),
        false => (r, py, pz, px),
    };
    let d = qx - qw.min(qy);
    let h = fract((qz + (qw - qy) / (6.0 * d + EPSILON)).abs() + dh);
    let s = match d / (qx + EPSILON) {
        0.0 => 0.0,
        s => (s + ds).clamp(0.0, 1.0),
    };
    let v = (qx + dv).clamp(0.0, 1.0);
    let channel = |k: f32| {
        let t = ((fract(h + k) * 6.0 - 3.0).abs() - 1.0).clamp(0.0, 1.0);
        // To the nearest, with halves down, as seen under the GPU tested.
        (v * (1.0 + s * (t - 1.0)) * 255.0 + 0.499) as u8
    };
    u32::from_be_bytes([a, channel(1.0), channel(2.0 / 3.0), channel(1.0 / 3.0)])
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

    /// An opaque frame whose pixels each have their own red and green.
    fn coded(w: u32, h: u32) -> Framebuffer {
        let mut fb = Framebuffer::new(w, h);
        for (i, p) in fb.pixels_mut().iter_mut().enumerate() {
            let (x, y) = (i as u32 % w, i as u32 / w);
            *p = 0xFF00_0000 | (x * 10) << 16 | (y * 10) << 8;
        }
        fb
    }

    #[test]
    fn an_odd_mosaic_takes_the_middle_pixel() {
        let mut fb = coded(9, 9);
        Filter::Mosaic(3).apply(&mut fb, 1, Compose::Normal);
        assert_eq!(fb.get(0, 0), Some(0xFF0A_0A00));
        assert_eq!(fb.get(2, 2), Some(0xFF0A_0A00));
        assert_eq!(fb.get(3, 2), Some(0xFF28_0A00));
    }

    #[test]
    fn an_even_mosaic_takes_the_mean_of_the_middle_four() {
        let mut fb = coded(8, 8);
        Filter::Mosaic(4).apply(&mut fb, 1, Compose::Normal);
        // Of columns and rows 1 and 2.
        assert_eq!(fb.get(0, 0), Some(0xFF0F_0F00));
        assert_eq!(fb.get(3, 3), Some(0xFF0F_0F00));
        assert_eq!(fb.get(4, 0), Some(0xFF37_0F00));
    }

    #[test]
    fn mosaic_blocks_are_frame_pixels_at_sample_2() {
        let mut fb = coded(16, 8);
        Filter::Mosaic(3).apply(&mut fb, 2, Compose::Normal);
        // Eight screen pixels make three blocks to each sample, six in all
        // over sixteen frame pixels: 2.67 wide, not 6.
        let row: Vec<u32> = (0..16).map(|x| fb.get(x, 0).unwrap()).collect();
        let blocks = row.windows(2).filter(|p| p[0] != p[1]).count() + 1;
        assert_eq!(blocks, 6);
    }

    #[test]
    fn a_mosaic_of_1_changes_nothing() {
        let mut fb = coded(4, 4);
        let before = fb.pixels().to_vec();
        Filter::Mosaic(1).apply(&mut fb, 1, Compose::Normal);
        assert_eq!(fb.pixels(), &before);
    }

    #[test]
    fn blur_halves_the_layer() {
        let mut fb = frame(&[0xFFFF_FFFF, 0, 0, 0, 0, 0, 0, 0], 4);
        Filter::Blur(1).apply(&mut fb, 1, Compose::Normal);
        assert_eq!(&fb.pixels()[..4], &[0x4040_4040, 0x4040_4040, 0, 0]);
        let mut fb = frame(&[0xFFFF_FFFF, 0, 0, 0, 0xFFFF_FFFF, 0, 0, 0], 4);
        Filter::Blur(1).apply(&mut fb, 1, Compose::Normal);
        assert_eq!(fb.pixels()[1], 0x7F7F_7F7F);
        let mut fb = frame(&[0xFFFF_FFFF, 0, 0, 0], 4);
        Filter::Blur(0).apply(&mut fb, 1, Compose::Normal);
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
        filter(false).apply(&mut fb, 1, Compose::Normal);
        assert_eq!(fb.pixels(), &[2, 3, 4, 0, 1, 2, 3, 4]);
        let mut fb = frame(&[row, row].concat(), 4);
        filter(true).apply(&mut fb, 1, Compose::Normal);
        assert_eq!(&fb.pixels()[..4], &[2, 3, 4, 1]);
    }

    #[test]
    fn raster_lines_take_the_entry_of_their_centers() {
        use sb_core::value::{Array, ArrayData, ElemType};
        // Two entries over three lines: the middle line's center, 1.5 of
        // 3, falls in the second.
        let mut info = Array::new(ElemType::Real, vec![2, 2]);
        info.data = ArrayData::Real(vec![0.0, 1.0, 0.5, 1.0]);
        let filter = Filter::Raster {
            vertical: false,
            info: std::rc::Rc::new(std::cell::RefCell::new(info)),
            repeat: false,
        };
        let mut fb = frame(&[1, 2, 1, 2, 1, 2], 2);
        filter.apply(&mut fb, 1, Compose::Normal);
        assert_eq!(fb.pixels(), &[1, 2, 2, 0, 2, 0]);
    }

    #[test]
    fn color_turns_hue() {
        let mut fb = frame(&[0xFFFF_0000, 0x8080_0000], 2);
        Filter::Color {
            hue: 120.0,
            saturation: 0.0,
            value: 0.0,
        }
        .apply(&mut fb, 1, Compose::Normal);
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
        color(&mut fb, 120.0, 0.0, 0.0, Compose::Normal);
        let one_by_one = pixels.map(|p| shift_color(p, [1.0 / 3.0, 0.0, 0.0]));
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
        .apply(&mut fb, 1, Compose::Normal);
        assert_eq!(fb.pixels(), &pixels);
        // As working each one out would.
        for p in pixels {
            assert_eq!(shift_color(p, [0.0; 3]), p);
        }
    }

    #[test]
    fn color_works_on_the_premultiplied_texel() {
        // Half-covered white: 80 of light with 80 of coverage is a grey,
        // not white, so raising the value starts from 80.
        assert_eq!(
            shift_color(0x8080_8080, [0.0, 0.0, 16.0 / 255.0]),
            0x8090_9090
        );
        // A grey has no saturation to raise, and no hue to turn.
        assert_eq!(shift_color(0xFFD7_D7D7, [0.25, 0.5, 0.0]), 0xFFD7_D7D7);
        // An orange turned a third of the way round.
        assert_eq!(shift_color(0xFFFF_8040, [1.0 / 3.0, 0.0, 0.0]), 0xFF40_FF80);
        // A negative turn goes round the other way.
        assert_eq!(
            shift_color(0xFF00_FF00, [-1.0 / 3.0, 0.0, 0.0]),
            0xFFFF_0000
        );
    }

    #[test]
    fn a_raised_value_lights_clear_pixels_unless_mode_1() {
        let filter = Filter::Color {
            hue: 200.0,
            saturation: -50.0,
            value: 30.0,
        };
        for (mode, want) in [
            (Compose::Direct, 0x001E_1E1E),
            (Compose::Normal, 0),
            (Compose::Add, 0x001E_1E1E),
            (Compose::Multiply, 0x001E_1E1E),
            (Compose::Screen, 0x001E_1E1E),
        ] {
            let mut fb = frame(&[0, 0], 2);
            filter.apply(&mut fb, 1, mode);
            assert_eq!(fb.pixels(), &[want, want], "{mode:?}");
        }
    }
}
