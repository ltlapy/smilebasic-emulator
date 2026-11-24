//! LFILTER's effects on a layer drawn on its own frame: mosaic, blur,
//! raster deformation and color conversion.

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
    /// offset and Y scale. The rows are a copy of the program's array at
    /// LFILTER: later writes to the array have no effect.
    Raster {
        vertical: bool,
        rows: Vec<[f64; 4]>,
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

    /// Applies the filter to a layer's premultiplied frame. `sample` is
    /// the number of frame pixels per screen pixel, and `mode` is how the
    /// layer is composed afterwards.
    #[cfg(test)]
    pub fn apply(&self, fb: &mut Framebuffer, sample: u32, mode: Compose) {
        let mut rows = Vec::new();
        self.op(&mut rows).apply(fb, sample, mode, &rows);
    }

    /// The filter in the form a frame uses, with the rows of a raster
    /// filter appended to `scene_rows`.
    pub fn op(&self, scene_rows: &mut Vec<[f64; 4]>) -> FilterOp {
        match self {
            Filter::None => FilterOp::None,
            Filter::Mosaic(n) => FilterOp::Mosaic(*n),
            Filter::Blur(n) => FilterOp::Blur(*n),
            Filter::Raster {
                vertical,
                rows,
                repeat,
            } => {
                let start = scene_rows.len();
                scene_rows.extend_from_slice(rows);
                FilterOp::Raster {
                    vertical: *vertical,
                    repeat: *repeat,
                    rows: start..scene_rows.len(),
                }
            }
            Filter::Color {
                hue,
                saturation,
                value,
            } => FilterOp::Color {
                hue: *hue,
                saturation: *saturation,
                value: *value,
            },
        }
    }
}

/// A layer's filter for one frame, as plain data: a raster filter's rows
/// are in the scene, not in the program's array.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum FilterOp {
    #[default]
    None,
    Mosaic(u32),
    Blur(u32),
    Raster {
        vertical: bool,
        repeat: bool,
        /// The range of the scene's raster rows that belong to this filter.
        rows: std::ops::Range<usize>,
    },
    Color {
        hue: f64,
        saturation: f64,
        value: f64,
    },
}

impl FilterOp {
    /// Applies the filter to a layer's premultiplied frame. `sample` is
    /// the number of frame pixels per screen pixel, and `mode` is how the
    /// layer is composed afterwards.
    pub fn apply(&self, fb: &mut Framebuffer, sample: u32, mode: Compose, rows: &[[f64; 4]]) {
        match self {
            // Amount 0 leaves the layer unchanged, not halved.
            FilterOp::None | FilterOp::Blur(0) => {}
            FilterOp::Mosaic(n) => mosaic(fb, *n, sample),
            FilterOp::Blur(n) => blur(fb, *n),
            FilterOp::Raster {
                vertical,
                repeat,
                rows: range,
            } => raster(fb, &rows[range.clone()], *vertical, *repeat),
            FilterOp::Color {
                hue,
                saturation,
                value,
            } => color(fb, *hue, *saturation, *value, mode),
        }
    }
}

/// A raster array's entries as X offset, X scale, Y offset and Y scale.
/// With two values per entry, they are the offset and scale along the
/// deformation: X for type 3, Y for type 4.
pub(crate) fn raster_rows(info: &sb_core::value::Array, vertical: bool, rows: &mut Vec<[f64; 4]>) {
    let width = match info.dims[..] {
        [_, w @ (2 | 4)] => w,
        _ => return,
    };
    let at = |i: usize| info.get(i).to_real().unwrap_or(0.0);
    rows.extend((0..info.len() / width).map(|r| match (width, vertical) {
        (2, false) => [at(r * 2), at(r * 2 + 1), 0.0, 1.0],
        (2, true) => [0.0, 1.0, at(r * 2), at(r * 2 + 1)],
        _ => std::array::from_fn(|k| at(r * 4 + k)),
    }));
}

/// Raster deformation. Each row of the frame (type 3) or column (type 4)
/// takes the entry that its center falls in. Each pixel shows the source
/// pixel at its center, scaled and then moved by that entry, the way
/// OpenGL does it: from the left for X and from the bottom for Y, with
/// offsets in widths and heights. Without `repeat`, points off the frame
/// show nothing.
fn raster(fb: &mut Framebuffer, rows: &[[f64; 4]], vertical: bool, repeat: bool) {
    if rows.is_empty() {
        return;
    }
    thread_local! {
        /// The output frame, kept between frames to avoid allocating a
        /// frame's worth of pixels for every layer every frame.
        static OUT: std::cell::RefCell<Vec<u32>> = const { std::cell::RefCell::new(Vec::new()) };
    }
    OUT.with_borrow_mut(|out| {
        out.clear();
        out.resize(fb.pixels().len(), 0);
        raster_into(fb, out, rows, vertical, repeat);
        fb.swap_pixels(out);
    });
}

/// A line of a raster filter's source points, in fixed point: each point is
/// `step` past the last. Rounding a point down gives the same result as
/// rounding the exact sum down, unless the point is within `near` of a
/// whole number.
#[derive(Clone, Copy)]
struct Stepper {
    at: i64,
    step: i64,
    near: i64,
}

impl Stepper {
    /// Fractional bits.
    const SHIFT: u32 = 32;
    const FRACTION: i64 = (1 << Self::SHIFT) - 1;

    /// For `n` points from `first`; None if they are too far out to step.
    #[inline]
    fn new(first: f64, step: f64, n: usize) -> Option<Stepper> {
        /// Above this, too few bits are left for the fraction, or the
        /// rounding of the exact sum is too large for a simple bound.
        const LIMIT: f64 = (1u64 << 24) as f64;
        let one = (1u64 << Self::SHIFT) as f64;
        let last = first + step * n as f64;
        // Also false for NaN.
        let fits = first.abs() < LIMIT && last.abs() < LIMIT;
        if !fits {
            return None;
        }
        // An error of half a unit in the step for each point, plus the
        // rounding of the exact sums, far less than a unit at this size.
        Some(Stepper {
            at: (first * one) as i64,
            step: (step * one) as i64,
            near: n as i64 + 16,
        })
    }

    /// Returns the point rounded down, if certain, and steps to the next.
    #[inline(always)]
    fn next(&mut self) -> Option<i64> {
        let at = self.at;
        self.at = at + self.step;
        let sure = ((at + self.near) & Self::FRACTION) >= 2 * self.near;
        sure.then_some(at >> Self::SHIFT)
    }
}

/// Raster deformation of `fb`, written into `out`, which has the same size.
fn raster_into(fb: &Framebuffer, out: &mut [u32], rows: &[[f64; 4]], vertical: bool, repeat: bool) {
    let source = fb.pixels();
    let (w, h) = (fb.width() as usize, fb.height() as usize);
    let (fw, fh) = (w as f64, h as f64);
    // The source point's index along a line of n, or None off it.
    let wrap = |v: f64, n: usize| -> Option<usize> {
        // NaN and points very far off show nothing.
        if v.is_nan() || v.abs() >= 1e15 {
            return None;
        }
        // Rounded down without the library call that f64::floor makes on
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
    // The source position of a pixel's center, across and down, each
    // computed from the entry's scale and its offset in pixels.
    let across_at = |x: usize, sx: f64, off: f64| sx * (x as f64 + 0.5) + off;
    let down_at = |y: usize, sy: f64, off: f64| fh - (sy * (fh - (y as f64 + 0.5)) + off);
    let across = |x: usize, [ox, sx, _, _]: [f64; 4]| wrap(across_at(x, sx, ox * fw), w);
    let down = |y: usize, [_, _, oy, sy]: [f64; 4]| wrap(down_at(y, sy, oy * fh), h);
    // The points of a line move steadily in one direction. So when both
    // ends of a line land on the frame, all its points do, and each point
    // rounds down with a plain cast: the fast path, with the same points.
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
            steps: Option<Stepper>,
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
                    steps: Stepper::new(down_at(0, sy, off), sy, h),
                }
            })
            .collect();
        let mut columns = columns;
        for (y, row) in out.chunks_exact_mut(w).enumerate() {
            for (dst, c) in row.iter_mut().zip(&mut columns) {
                let Some(a) = c.from else {
                    *dst = 0;
                    continue;
                };
                let stepped = c.steps.as_mut().and_then(Stepper::next);
                *dst = match stepped {
                    Some(b) if (b as u64) < h as u64 => source[b as usize * w + a],
                    Some(_) if !repeat => 0,
                    _ if c.inside => source[down_at(y, c.sy, c.off) as usize * w + a],
                    _ => wrap(down_at(y, c.sy, c.off), h).map_or(0, |b| source[b * w + a]),
                };
            }
        }
        return;
    }
    // Each row takes its own entry, so it reads one row of the source.
    // A layer on its own frame is mostly rows with nothing in them.
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
        let mut steps = Stepper::new(across_at(0, sx, off), sx, w);
        for (x, dst) in row.iter_mut().enumerate() {
            *dst = match steps.as_mut().and_then(Stepper::next) {
                Some(a) if (a as u64) < w as u64 => from[a as usize],
                Some(_) if !repeat => 0,
                _ => across(x, e).map_or(0, |a| from[a]),
            };
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
/// and out of them, kept between frames to avoid allocating them for
/// every filtered layer every frame.
#[derive(Default)]
struct Half {
    pixels: Vec<u32>,
    between: Vec<u32>,
    across: Vec<Tap>,
    down: Vec<Tap>,
    out_across: Vec<u32>,
    expanded: Vec<u32>,
    lines: BlurLines,
}

thread_local! {
    static HALF: std::cell::RefCell<Half> = std::cell::RefCell::default();
}

/// Flips the alpha channel from the coverage we keep to how much shows
/// through, and back: the GPU's frames hold the latter, so that is what
/// filters average and scale.
const THROUGH: u32 = 0xFF00_0000;

/// Fills `out` with the points where the centers of `count` half-size
/// pixels fall on a line of `size`, when `q` of the pixels span the
/// line. With `flip`, the points count from the end of the line.
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
        // Rounded down without the library call that f64::floor makes on
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
    // sum of four divided by four, for two channels of a word together.
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
/// it takes the row nearer the bottom of the frame, because rows count
/// from the bottom on the GPU.
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

/// The layout of a mosaic's blocks on the half-size frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MosaicGrid {
    /// The number of half-size pixels the whole frame is drawn into, across
    /// and down. They do not need to be whole numbers.
    pub q: f64,
    pub qy: f64,
    /// The number of half-size pixels that this drawing touches, across and
    /// down.
    pub across: usize,
    pub down: usize,
}

/// The grid of a mosaic of blocks of `size` on a frame of `w` by `h`, or None
/// if it changes nothing. The number of blocks is counted in screen pixels,
/// and the size of a block in frame pixels.
pub fn mosaic_grid(w: u32, h: u32, size: u32, sample: u32) -> Option<MosaicGrid> {
    let (sw, sh) = (w / sample.max(1), h / sample.max(1));
    let (hw, hh) = (w / 2, h / 2);
    if size < 2 || hw == 0 || hh == 0 || sw < 2 || sh < 2 {
        return None;
    }
    let span = |screen: u32, half_size: u32| {
        f64::from(screen.div_ceil(size)) * f64::from(half_size) / f64::from(screen / 2)
    };
    let (q, qy) = (span(sw, hw), span(sh, hh));
    Some(MosaicGrid {
        q,
        qy,
        across: (q.ceil() as usize).min(hw as usize),
        down: (qy.ceil() as usize).min(hh as usize),
    })
}

/// LFILTER's mosaic: the whole frame is drawn small, into as many pixels of
/// the half-size frame as blocks fit, and then back at full size without
/// smoothing. The small copy is drawn with smoothing, so a block of an even
/// size is the mean of the four pixels around its middle.
fn mosaic(fb: &mut Framebuffer, size: u32, sample: u32) {
    let (w, h) = (fb.width(), fb.height());
    let Some(MosaicGrid {
        q,
        qy,
        across,
        down,
    }) = mosaic_grid(w, h, size, sample)
    else {
        return;
    };
    HALF.with_borrow_mut(|half| {
        half.pixels.clear();
        half.pixels.resize(across * down, 0);
        taps(&mut half.across, w, q, across, false);
        taps(&mut half.down, h, qy, down, true);
        shrink(half, fb.pixels(), w as usize, across);
        expand(half, fb.pixels_mut(), (w, h), (q, qy), across);
    });
}

/// The weights of LFILTER's blur and the number of taps it takes each way.
pub struct BlurKernel {
    /// In 16.16: the weight of the middle tap, then of each further pair.
    pub weights: [u32; 5],
    /// Pixels each way, 1 to 4.
    pub reach: usize,
    /// The sum of the weights of all taps.
    pub total: u32,
}

/// The kernel for a blur `amount`. The weights are a Gaussian of variance
/// `amount / 10` at the odd distances 1, 3, 5... up to 31, where the middle
/// counts as distance 1. They are normalized over all sixteen distances
/// and taken in order until they make up 99%. SmileBASIC 4 has 3, 5, 7 or
/// 9 taps and reads those weights unchanged, on neighboring half-size
/// pixels. So a kernel that needs more than 9 taps is cut off without
/// normalizing again, and its plateau falls. A kernel that needs only the
/// middle leaves the next weight not normalized at all.
pub fn blur_kernel(amount: u32) -> BlurKernel {
    let v = amount as f32 / 10.0;
    let mut raw = [0f32; 16];
    for (k, w) in raw.iter_mut().enumerate() {
        let d = (2 * k + 1) as f32;
        *w = (-(d * d) / 2.0 / v).exp();
    }
    let sum = raw[0] + 2.0 * raw[1..].iter().sum::<f32>();
    let mut made = raw[0] / sum;
    raw[0] = made;
    let mut wanted = 1;
    if made <= 0.99 {
        for (k, w) in raw.iter_mut().enumerate().skip(1) {
            *w /= sum;
            made += 2.0 * *w;
            if made > 0.99 {
                wanted = 2 * k + 1;
                break;
            }
        }
    }
    let reach = match wanted {
        0..=3 => 1,
        4..=5 => 2,
        6..=7 => 3,
        _ => 4,
    };
    let mut weights = [0; 5];
    for (o, w) in weights.iter_mut().zip(&raw).take(reach + 1) {
        *o = (w * 65536.0 + 0.5) as u32;
    }
    let total = weights[0] + 2 * weights[1..].iter().sum::<u32>();
    BlurKernel {
        weights,
        reach,
        total,
    }
}

/// The channels of a pixel, two per word, each with 32 bits to grow in.
#[inline]
fn lanes(p: u32) -> (u64, u64) {
    let p = u64::from(p);
    (
        p & 0xFF | (p >> 16 & 0xFF) << 32,
        p >> 8 & 0xFF | (p >> 24) << 32,
    )
}

/// The pixel from sums in 16.16 of [`lanes`], rounded to the nearest with
/// halves down, each channel held to 255.
#[inline]
fn from_lanes((lo, hi): (u64, u64)) -> u32 {
    const HALF_DOWN: u64 = 0x7FFF_0000_7FFF;
    let ch = |v: u64| ((v >> 16 & 0xFFFF).min(255)) as u32;
    let (lo, hi) = (lo + HALF_DOWN, hi + HALF_DOWN);
    ch(lo) | ch(hi) << 8 | ch(lo >> 32) << 16 | ch(hi >> 32) << 24
}

/// The result for one color across all taps, scaled by the kernel's total.
#[inline]
fn plateau(p: u32, total: u32) -> u32 {
    let (lo, hi) = lanes(p);
    from_lanes((lo * u64::from(total), hi * u64::from(total)))
}

/// A line of pixels split into [`lanes`], so that a tap takes two loads
/// instead of splitting a pixel. It has `reach` copies of the first pixel
/// before the line and `reach` copies of the last pixel after it.
#[derive(Default)]
struct LaneLine {
    lo: Vec<u64>,
    hi: Vec<u64>,
}

impl LaneLine {
    fn fill(&mut self, row: &[u32], reach: usize) {
        let n = row.len() + 2 * reach;
        self.lo.resize(n, 0);
        self.hi.resize(n, 0);
        let (lo, hi) = (&mut self.lo[..n], &mut self.hi[..n]);
        let (first, last) = (lanes(row[0]), lanes(row[row.len() - 1]));
        lo[..reach].fill(first.0);
        hi[..reach].fill(first.1);
        lo[n - reach..].fill(last.0);
        hi[n - reach..].fill(last.1);
        let middle = lo[reach..].iter_mut().zip(&mut hi[reach..]).zip(row);
        for ((l, h), &p) in middle {
            (*l, *h) = lanes(p);
        }
    }
}

/// The working lines of a blur, kept between frames.
#[derive(Default)]
struct BlurLines {
    /// The line being processed along the rows.
    row: LaneLine,
    /// The rows around the one being processed down the columns,
    /// indexed by row number modulo their count.
    rows: Vec<LaneLine>,
    /// Pixels of one color up to the end of the window, by column.
    runs: Vec<u32>,
}

/// One pass of [`blur`] along each row. The taps past an edge read
/// the edge pixel. Where all taps hold one color, as across most of a
/// frame, the result needs only the kernel's total.
fn blur_rows(from: &[u32], to: &mut [u32], w: usize, lines: &mut BlurLines, kernel: &BlurKernel) {
    let r = kernel.reach;
    let weights = kernel.weights.map(u64::from);
    let (mut flat, mut flat_out) = (!from[0], 0);
    let line = &mut lines.row;
    for (row, out) in from.chunks_exact(w).zip(to.chunks_exact_mut(w)) {
        // Whole rows of one color, which a layer on its own frame mostly has.
        if row.iter().all(|&p| p == row[0]) {
            if row[0] != flat {
                flat = row[0];
                flat_out = plateau(flat, kernel.total);
            }
            out.fill(flat_out);
            continue;
        }
        line.fill(row, r);
        let (lo, hi) = (&line.lo[..w + 2 * r], &line.hi[..w + 2 * r]);
        // The number of pixels of one color up to the right end of the
        // window, counting the copies before the row.
        let mut run = r + 1;
        for k in 1..=r.min(w - 1) {
            run = if row[k] == row[k - 1] { run + 1 } else { 1 };
        }
        run += (r + 1).saturating_sub(w);
        let full = 2 * r;
        for (i, o) in out.iter_mut().enumerate() {
            if i > 0 {
                let same = i + r >= w || row[i + r] == row[i + r - 1];
                run = if same { run + 1 } else { 1 };
            }
            if run > full {
                let color = row[(i + r).min(w - 1)];
                if color != flat {
                    flat = color;
                    flat_out = plateau(flat, kernel.total);
                }
                *o = flat_out;
                continue;
            }
            let c = i + r;
            let (mut l, mut h) = (lo[c] * weights[0], hi[c] * weights[0]);
            for k in 1..=r {
                l += (lo[c - k] + lo[c + k]) * weights[k];
                h += (hi[c - k] + hi[c + k]) * weights[k];
            }
            *o = from_lanes((l, h));
        }
    }
}

/// [`blur_rows`] down each column, a row at a time.
fn blur_columns(
    from: &[u32],
    to: &mut [u32],
    w: usize,
    lines: &mut BlurLines,
    kernel: &BlurKernel,
) {
    let (r, h) = (kernel.reach, from.len() / w);
    let weights = kernel.weights.map(u64::from);
    let row = |y: usize| &from[y * w..][..w];
    let span = 2 * r + 1;
    let BlurLines { rows, runs, .. } = lines;
    rows.resize_with(span, LaneLine::default);
    runs.clear();
    runs.resize(w, r as u32 + 1);
    // Taps above the frame read its top row, so a run starts with them.
    let mut held = 0;
    let mut hold = |rows: &mut [LaneLine], y: usize| {
        rows[y % span].fill(row(y), 0);
        held = y;
    };
    hold(rows, 0);
    for y in 1..=r.min(h - 1) {
        hold(rows, y);
        for ((run, a), b) in runs.iter_mut().zip(row(y)).zip(row(y - 1)) {
            *run = if a == b { *run + 1 } else { 1 };
        }
    }
    if r >= h {
        let more = (r + 1 - h) as u32;
        runs.iter_mut().for_each(|run| *run += more);
    }
    let full = 2 * r as u32;
    let (mut flat, mut flat_out) = (!from[0], 0);
    for (y, out) in to.chunks_exact_mut(w).enumerate() {
        if y > 0 {
            let end = y + r;
            if end < h {
                hold(rows, end);
                for ((run, a), b) in runs.iter_mut().zip(row(end)).zip(row(end - 1)) {
                    *run = if a == b { *run + 1 } else { 1 };
                }
            } else {
                runs.iter_mut().for_each(|run| *run += 1);
            }
        }
        let last = row((y + r).min(h - 1));
        // The two rows each pair of taps reads, for k = 1 to r.
        let pair = |k: usize| {
            let (a, b) = (y.saturating_sub(k), (y + k).min(h - 1));
            (&rows[a % span], &rows[b % span])
        };
        let center = &rows[y % span];
        let pairs = [pair(1), pair(2), pair(3), pair(4)];
        for (x, o) in out.iter_mut().enumerate() {
            if runs[x] > full {
                if last[x] != flat {
                    flat = last[x];
                    flat_out = plateau(flat, kernel.total);
                }
                *o = flat_out;
                continue;
            }
            let (mut l, mut m) = (center.lo[x] * weights[0], center.hi[x] * weights[0]);
            for (k, (a, b)) in pairs.iter().enumerate().take(r) {
                l += (a.lo[x] + b.lo[x]) * weights[k + 1];
                m += (a.hi[x] + b.hi[x]) * weights[k + 1];
            }
            *o = from_lanes((l, m));
        }
    }
}

/// LFILTER's blur: the layer is drawn at half size with smoothing, blurred
/// along the rows and then down the columns, each pass stored in 8 bits,
/// and drawn back at full size without smoothing. Because the kernel is
/// cut off at 9 taps, what shows through falls together with the light
/// where the amount is large, so even a clear part of the layer darkens
/// what is behind it. It is done in integers because it runs every frame.
fn blur(fb: &mut Framebuffer, amount: u32) {
    let (w, h) = (fb.width(), fb.height());
    let (hw, hh) = (w as usize / 2, h as usize / 2);
    if hw == 0 || hh == 0 {
        return;
    }
    let kernel = blur_kernel(amount);
    HALF.with_borrow_mut(|half| {
        half.pixels.clear();
        half.pixels.resize(hw * hh, 0);
        taps(&mut half.across, w, hw as f64, hw, false);
        taps(&mut half.down, h, hh as f64, hh, true);
        shrink(half, fb.pixels(), w as usize, hw);
        half.between.clear();
        half.between.resize(hw * hh, 0);
        blur_rows(
            &half.pixels,
            &mut half.between,
            hw,
            &mut half.lines,
            &kernel,
        );
        blur_columns(
            &half.between,
            &mut half.pixels,
            hw,
            &mut half.lines,
            &kernel,
        );
        expand(half, fb.pixels_mut(), (w, h), (hw as f64, hh as f64), hw);
    });
}

/// Color conversion over the frame. A screen has few colors, so each is
/// computed once and kept in a small table indexed by its bits. A clear
/// pixel stays clear only on a layer composed in mode 1. On other
/// layers a raised value lights it, so the filter shows over the whole
/// layer.
fn color(fb: &mut Framebuffer, hue: f64, saturation: f64, value: f64, mode: Compose) {
    // Left at zero, as programs leave it when an effect ends, the filter
    // changes nothing: each pixel comes back unchanged.
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
        // Fibonacci hashing: the top bits of the product mix all bits of
        // the key.
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
/// `[dh, ds, dv]`, each as a fraction of its whole range. The conversion works
/// directly on the premultiplied color, in single precision, with the usual
/// GPU formulas. A gray has no saturation to raise. The coverage stays the
/// same.
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
        // To the nearest, with halves down, as seen on the GPU tested.
        (v * (1.0 + s * (t - 1.0)) * 255.0 + 0.499) as u8
    };
    u32::from_be_bytes([a, channel(1.0), channel(2.0 / 3.0), channel(1.0 / 3.0)])
}

#[cfg(test)]
mod tests {
    use super::*;

    /// [`raster_into`] with every point computed by its sum.
    fn raster_by_the_sum(
        fb: &Framebuffer,
        rows: &[[f64; 4]],
        vertical: bool,
        repeat: bool,
    ) -> Vec<u32> {
        let (w, h) = (fb.width() as usize, fb.height() as usize);
        let (fw, fh) = (w as f64, h as f64);
        let wrap = |v: f64, n: usize| -> Option<usize> {
            if v.is_nan() || v.abs() >= 1e15 {
                return None;
            }
            let t = v.floor() as i64;
            match usize::try_from(t).ok().filter(|&t| t < n) {
                Some(t) => Some(t),
                None if repeat => Some(t.rem_euclid(n as i64) as usize),
                None => None,
            }
        };
        let lines = if vertical { w } else { h };
        let entry = |i: usize| rows[(2 * i + 1) * rows.len() / (2 * lines)];
        let mut out = vec![0; w * h];
        for y in 0..h {
            for x in 0..w {
                let [ox, sx, oy, sy] = entry(if vertical { x } else { y });
                let across = wrap(sx * (x as f64 + 0.5) + ox * fw, w);
                let down = wrap(fh - (sy * (fh - (y as f64 + 0.5)) + oy * fh), h);
                if let (Some(a), Some(b)) = (across, down) {
                    out[y * w + x] = fb.pixels()[b * w + a];
                }
            }
        }
        out
    }

    #[test]
    fn stepping_a_raster_filter_reads_the_same_points() {
        let (w, h) = (97, 61);
        let mut fb = Framebuffer::new(w, h);
        for (i, p) in fb.pixels_mut().iter_mut().enumerate() {
            *p = i as u32 + 1;
        }
        let mut seed = 0x8BAD_F00Du32;
        let mut next = move |n: u32| {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            seed % n
        };
        let mut out = vec![0; (w * h) as usize];
        for round in 0..1500 {
            let n = 1 + next(120) as usize;
            let mut value = |scale: bool| {
                let exact = [0.0, 0.5, 1.0, 2.0, -1.0, 0.25, 1.0 / 3.0];
                let v = match next(4) {
                    0 => exact[next(7) as usize],
                    1 => (f64::from(next(2000)) - 1000.0) / 97.0,
                    2 => f64::from(next(4)) * 1e9,
                    _ => f64::from(next(2000)) / 1000.0 - 1.0,
                };
                if scale { v * 2.0 } else { v * 0.5 }
            };
            let rows: Vec<[f64; 4]> = (0..n)
                .map(|_| [value(false), value(true), value(false), value(true)])
                .collect();
            let (vertical, repeat) = (round % 2 == 0, round % 3 == 0);
            raster_into(&fb, &mut out, &rows, vertical, repeat);
            let want = raster_by_the_sum(&fb, &rows, vertical, repeat);
            assert_eq!(out, want, "{rows:?} {vertical} {repeat}");
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
        // The light rounds its half down, and so does what shows through.
        assert_eq!(fb.pixels()[1], 0x807F_7F7F);
        let mut fb = frame(&[0xFFFF_FFFF, 0, 0, 0], 4);
        Filter::Blur(0).apply(&mut fb, 1, Compose::Normal);
        assert_eq!(fb.pixels(), &[0xFFFF_FFFF, 0, 0, 0]);
    }

    #[test]
    fn blur_weights_follow_the_amount() {
        let per_255 = |k: &BlurKernel, i: usize| (k.weights[i] * 255 + 32768) >> 16;
        // As a line of 255 shows on hardware.
        let k = blur_kernel(128);
        assert_eq!(k.reach, 4);
        assert_eq!(
            (0..5).map(|i| per_255(&k, i)).collect::<Vec<_>>(),
            [70, 51, 27, 11, 3]
        );
        // The plateau of a wide block falls where the kernel is cut off.
        for (amount, peak) in [(128, 252), (256, 228), (384, 197)] {
            let total = blur_kernel(amount).total;
            assert_eq!(plateau(plateau(0xFF, total), total), peak);
        }
        // Few taps: 3 taps at amount 8, and at 7 the next weight is not
        // normalized, because the sum passes 99% with the middle alone.
        assert_eq!(blur_kernel(8).reach, 1);
        assert_eq!(blur_kernel(7).reach, 1);
        assert!(blur_kernel(7).weights[1] < blur_kernel(8).weights[1] / 2);
    }

    /// The row and column passes done the slow way, tap by tap.
    fn slow_pass(from: &[u32], w: usize, h: usize, k: &BlurKernel, rows: bool) -> Vec<u32> {
        (0..w * h)
            .map(|i| {
                let (x, y) = (i % w, i / w);
                let at = |d: isize| {
                    let (ax, ay) = match rows {
                        true => ((x as isize + d).clamp(0, w as isize - 1) as usize, y),
                        false => (x, (y as isize + d).clamp(0, h as isize - 1) as usize),
                    };
                    lanes(from[ay * w + ax])
                };
                let (mut lo, mut hi) = (0, 0);
                for d in -(k.reach as isize)..=k.reach as isize {
                    let (l, m) = at(d);
                    let wd = u64::from(k.weights[d.unsigned_abs()]);
                    (lo, hi) = (lo + l * wd, hi + m * wd);
                }
                from_lanes((lo, hi))
            })
            .collect()
    }

    #[test]
    fn flat_runs_blur_as_every_pixel_would() {
        let mut seed = 0x0BAD_F00D_u32;
        let mut next = || {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            seed
        };
        for (w, h) in [(37, 29), (3, 2), (1, 1), (2, 9), (9, 1)] {
            // Runs of one color, as frames have, and noise.
            let pixels: Vec<u32> = (0..w * h)
                .map(|i| match (i / 7) % 4 < 3 {
                    true => 0xFF10_2030,
                    false => next(),
                })
                .collect();
            for amount in [1, 7, 16, 32, 64, 128, 384] {
                let k = blur_kernel(amount);
                let mut out = vec![0; w * h];
                let mut lines = BlurLines::default();
                blur_rows(&pixels, &mut out, w, &mut lines, &k);
                assert_eq!(
                    out,
                    slow_pass(&pixels, w, h, &k, true),
                    "rows {w}x{h} {amount}"
                );
                blur_columns(&pixels, &mut out, w, &mut lines, &k);
                assert_eq!(
                    out,
                    slow_pass(&pixels, w, h, &k, false),
                    "cols {w}x{h} {amount}"
                );
            }
        }
    }

    #[test]
    fn a_wide_blur_darkens_a_clear_layer() {
        let mut fb = Framebuffer::new(16, 16);
        fb.clear(0);
        Filter::Blur(384).apply(&mut fb, 1, Compose::Normal);
        // Of what shows through, 197 in 255 is left.
        assert_eq!(fb.pixels()[0] >> 24, 255 - 197);
        assert_eq!(fb.pixels()[0] & 0x00FF_FFFF, 0);
    }

    #[test]
    fn odd_sizes_blur_without_trouble() {
        for (w, h) in [(1, 1), (1, 5), (5, 1), (3, 3), (5, 7)] {
            let mut fb = coded(w, h);
            Filter::Blur(32).apply(&mut fb, 1, Compose::Normal);
            let mut fb = coded(w, h);
            Filter::Mosaic(2).apply(&mut fb, 1, Compose::Normal);
        }
    }

    #[test]
    fn raster_moves_lines() {
        use sb_core::value::{Array, ArrayData, ElemType};
        let mut info = Array::new(ElemType::Real, vec![2, 2]);
        // The first line takes from a quarter further right, the second
        // stays.
        info.data = ArrayData::Real(vec![0.25, 1.0, 0.0, 1.0]);
        let mut rows = Vec::new();
        raster_rows(&info, false, &mut rows);
        let row = [1, 2, 3, 4];
        let mut fb = frame(&[row, row].concat(), 4);
        let filter = |repeat| Filter::Raster {
            vertical: false,
            rows: rows.clone(),
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
        let mut rows = Vec::new();
        raster_rows(&info, false, &mut rows);
        let filter = Filter::Raster {
            vertical: false,
            rows,
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
        // The same result as computing each pixel with `shift_color`.
        for p in pixels {
            assert_eq!(shift_color(p, [0.0; 3]), p);
        }
    }

    #[test]
    fn color_works_on_the_premultiplied_texel() {
        // Half-covered white: 80 of light with 80 of coverage is a gray,
        // not white, so raising the value starts from 80.
        assert_eq!(
            shift_color(0x8080_8080, [0.0, 0.0, 16.0 / 255.0]),
            0x8090_9090
        );
        // A gray has no saturation to raise, and no hue to turn.
        assert_eq!(shift_color(0xFFD7_D7D7, [0.25, 0.5, 0.0]), 0xFFD7_D7D7);
        // An orange turned a third of the way around.
        assert_eq!(shift_color(0xFFFF_8040, [1.0 / 3.0, 0.0, 0.0]), 0xFF40_FF80);
        // A negative turn goes around the other way.
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
