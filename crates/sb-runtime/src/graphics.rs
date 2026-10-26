//! Graphic pages GRP0-GRP5 and drawing on them. A page takes memory only
//! once something is drawn on it; until then every pixel is 0.

use crate::font::{Font, FontSize};
use crate::framebuffer::Framebuffer;

pub const PAGES: usize = 6;
/// `#GRPWIDTH` and `#GRPHEIGHT`.
pub const GRP_SIZE: i32 = 2048;
/// Lines whose ends lie further out than this are clipped before they are
/// walked, so that huge coordinates cannot stall the program.
const LINE_LIMIT: i32 = 1 << 16;

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
            Blend::Alpha2 => {
                let a = sa + da - sa * da / 255;
                if a == 0 {
                    return 0;
                }
                let dw = da * (255 - sa);
                let mix = |s: u32, d: u32| (s * sa * 255 + d * dw) / (255 * a);
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
    let (a, b) = (a.to_be_bytes(), b.to_be_bytes());
    u32::from_be_bytes(std::array::from_fn(|i| {
        (u32::from(a[i]) * u32::from(b[i]) / 255) as u8
    }))
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
}

impl Default for Graphics {
    fn default() -> Self {
        Graphics {
            pages: vec![Vec::new(); PAGES],
            target: 0,
            color: 0xFFFF_FFFF,
            clip: Clip::PAGE,
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

    fn target_pixels(&mut self) -> &mut Vec<u32> {
        let pixels = &mut self.pages[self.target];
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
        let pixels = self.target_pixels();
        let i = index(x, y);
        pixels[i] = blend.apply(pixels[i], color);
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

    /// GBOX: the outline, each pixel drawn once.
    pub fn rect(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, color: u32, blend: Blend) {
        let (l, r) = (x0.min(x1), x0.max(x1));
        let (t, b) = (y0.min(y1), y0.max(y1));
        let c = self.clip;
        for x in l.max(c.x0)..=r.min(c.x1) {
            self.plot(x, t, color, blend);
            if b != t {
                self.plot(x, b, color, blend);
            }
        }
        for y in (t + 1).max(c.y0)..=(b - 1).min(c.y1) {
            self.plot(l, y, color, blend);
            if r != l {
                self.plot(r, y, color, blend);
            }
        }
    }

    /// GFILL.
    pub fn fill(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, color: u32, blend: Blend) {
        let c = self.clip;
        for y in y0.min(y1).max(c.y0)..=y0.max(y1).min(c.y1) {
            for x in x0.min(x1).max(c.x0)..=x0.max(x1).min(c.x1) {
                self.plot(x, y, color, blend);
            }
        }
    }

    /// GCIRCLE: the whole circle, or with `arc`, the part from one angle
    /// to another (degrees, clockwise from 3 o'clock), as a sector if
    /// `sector` is set.
    pub fn circle(
        &mut self,
        (cx, cy, r): (i32, i32, i32),
        arc: Option<(i32, i32, bool)>,
        color: u32,
        blend: Blend,
    ) {
        // UNVERIFIED: circles too large to cross the page draw nothing.
        if !(0..LINE_LIMIT).contains(&r) {
            return;
        }
        let mut points = circle_points(r);
        if let Some((start, end, _)) = arc {
            let (s, e) = (start.rem_euclid(360), end.rem_euclid(360));
            // An arc from an angle to the same angle draws nothing.
            // UNVERIFIED: whether a sector does the same.
            if s == e {
                return;
            }
            points.retain(|&(dx, dy)| {
                let a = f64::from(dy)
                    .atan2(f64::from(dx))
                    .to_degrees()
                    .rem_euclid(360.0);
                let (s, e) = (f64::from(s), f64::from(e));
                if s <= e {
                    (s..=e).contains(&a)
                } else {
                    a >= s || a <= e
                }
            });
        }
        let mut points: Vec<(i32, i32)> = points
            .into_iter()
            .map(|(dx, dy)| (cx + dx, cy + dy))
            .collect();
        if let Some((start, end, true)) = arc {
            // A sector is outlined, not filled (checked on hardware).
            for angle in [start, end] {
                let (s, c) = f64::from(angle).to_radians().sin_cos();
                let x = cx + (c * f64::from(r)).round() as i32;
                let y = cy + (s * f64::from(r)).round() as i32;
                points.extend(line_points(cx, cy, x, y));
            }
        }
        points.sort_unstable();
        points.dedup();
        for (x, y) in points {
            self.plot(x, y, color, blend);
        }
    }

    /// GTRI: a filled triangle, edges included.
    pub fn triangle(&mut self, p: [(i32, i32); 3], color: u32, blend: Blend) {
        let c = self.clip;
        let x0 = p.iter().map(|p| p.0).min().unwrap().max(c.x0);
        let x1 = p.iter().map(|p| p.0).max().unwrap().min(c.x1);
        let y0 = p.iter().map(|p| p.1).min().unwrap().max(c.y0);
        let y1 = p.iter().map(|p| p.1).max().unwrap().min(c.y1);
        // i128, since products of far-off coordinates overflow i64.
        let p = p.map(|(x, y)| (i128::from(x), i128::from(y)));
        let edge = |a: (i128, i128), b: (i128, i128), x: i128, y: i128| {
            (b.0 - a.0) * (y - a.1) - (b.1 - a.1) * (x - a.0)
        };
        let area = edge(p[0], p[1], p[2].0, p[2].1);
        if area == 0 {
            // Degenerate: the three points are on one line.
            for (a, b) in [(0, 1), (1, 2), (2, 0)] {
                let (a, b) = (p[a], p[b]);
                for (x, y) in line_points(a.0 as i32, a.1 as i32, b.0 as i32, b.1 as i32) {
                    if c.contains(x, y) {
                        self.plot(x, y, color, blend);
                    }
                }
            }
            return;
        }
        let sign = area.signum();
        for y in y0..=y1 {
            for x in x0..=x1 {
                let (x, y) = (i128::from(x), i128::from(y));
                let inside = [(0, 1), (1, 2), (2, 0)]
                    .iter()
                    .all(|&(a, b)| edge(p[a], p[b], x, y) * sign >= 0);
                let (x, y) = (x as i32, y as i32);
                if inside {
                    self.plot(x, y, color, blend);
                }
            }
        }
    }

    /// GPAINT: fills the area around a point that has the point's color,
    /// or, with `border`, everything up to that color.
    pub fn paint(&mut self, x: i32, y: i32, color: u32, border: Option<u32>) {
        let c = self.clip;
        if !c.contains(x, y) {
            return;
        }
        let page = self.target;
        let start = self.pixel(page, x, y);
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
            if seen[i] || !fills(self.pixel(page, x, y)) {
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
        for y in 0..h {
            for x in 0..w {
                pixels.push(self.pixel(src, from.x0 + x, from.y0 + y));
            }
        }
        let c = self.clip;
        for y in (c.y0 - dy).max(0)..=(c.y1 - dy).min(h - 1) {
            for x in (c.x0 - dx).max(0)..=(c.x1 - dx).min(w - 1) {
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
        size: FontSize,
        (sx, sy): (i32, i32),
        color: u32,
        blend: Blend,
    ) {
        let n = size as i32;
        for (i, &code) in text.iter().enumerate() {
            let left = x.saturating_add((i as i32).saturating_mul(n * sx));
            for gy in 0..n {
                for gx in 0..n {
                    let p = multiply(font.pixel(code, size, gx as u32, gy as u32), color);
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

    /// Draws a page's top-left corner over the whole frame.
    pub fn draw(&self, page: usize, fb: &mut Framebuffer) {
        let pixels = &self.pages[page];
        if pixels.is_empty() {
            return;
        }
        for y in 0..fb.height().min(GRP_SIZE as u32) {
            for x in 0..fb.width().min(GRP_SIZE as u32) {
                let src = pixels[index(x as i32, y as i32)];
                if src >> 24 != 0
                    && let Some(dst) = fb.get(x, y)
                {
                    let out = Blend::Alpha.apply(dst, src);
                    fb.set(x, y, (dst & 0xFF00_0000) | (out & 0x00FF_FFFF));
                }
            }
        }
    }
}

/// The points of a circle of radius `r` around the origin, without
/// repeats.
fn circle_points(r: i32) -> Vec<(i32, i32)> {
    let mut points = Vec::new();
    let (mut x, mut y, mut err) = (r, 0, 1 - r);
    while x >= y {
        for (px, py) in [
            (x, y),
            (y, x),
            (-y, x),
            (-x, y),
            (-x, -y),
            (-y, -x),
            (y, -x),
            (x, -y),
        ] {
            points.push((px, py));
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

fn line_points(x0: i32, y0: i32, x1: i32, y1: i32) -> Vec<(i32, i32)> {
    let Some((x0, y0, x1, y1)) = clip_line(x0, y0, x1, y1) else {
        return Vec::new();
    };
    // Walk the longer axis. The rule below was worked out from lines read
    // back on SmileBASIC 4: the same pixels whichever end comes first, as
    // if drawn from the end with the smaller major coordinate, with the
    // minor offset rounded down for slopes of 1/2 and more, and up below.
    let x_major = (x1 - x0).abs() >= (y1 - y0).abs();
    let swap = |(x, y): (i32, i32)| if x_major { (x, y) } else { (y, x) };
    let (mut a, mut b) = (swap((x0, y0)), swap((x1, y1)));
    if a.0 > b.0 {
        std::mem::swap(&mut a, &mut b);
    }
    let major = i64::from(b.0 - a.0);
    let minor = i64::from(b.1 - a.1);
    let bias = if 2 * minor.abs() < major {
        major - 1
    } else {
        0
    };
    (0..=major)
        .map(|t| {
            let offset = if major == 0 {
                0
            } else {
                (t * minor.abs() + bias) / major
            };
            let p = (a.0 + t as i32, a.1 + (minor.signum() * offset) as i32);
            swap(p)
        })
        .collect()
}

/// Shortens a line whose ends lie far outside every page to the part
/// near the pages (Liang-Barsky). `None` if none of it is near.
fn clip_line(x0: i32, y0: i32, x1: i32, y1: i32) -> Option<(i32, i32, i32, i32)> {
    let near = |v: i32| (-LINE_LIMIT..=LINE_LIMIT).contains(&v);
    if near(x0) && near(y0) && near(x1) && near(y1) {
        return Some((x0, y0, x1, y1));
    }
    let (lo, hi) = (-1.0, f64::from(GRP_SIZE));
    let (fx0, fy0) = (f64::from(x0), f64::from(y0));
    let (dx, dy) = (f64::from(x1) - fx0, f64::from(y1) - fy0);
    let (mut t0, mut t1) = (0.0f64, 1.0f64);
    for (p, q) in [
        (-dx, fx0 - lo),
        (dx, hi - fx0),
        (-dy, fy0 - lo),
        (dy, hi - fy0),
    ] {
        if p == 0.0 {
            if q < 0.0 {
                return None;
            }
            continue;
        }
        let t = q / p;
        if p < 0.0 {
            t0 = t0.max(t);
        } else {
            t1 = t1.min(t);
        }
        if t0 > t1 {
            return None;
        }
    }
    let at = |t: f64| ((fx0 + dx * t).round() as i32, (fy0 + dy * t).round() as i32);
    let ((a, b), (c, d)) = (at(t0), at(t1));
    Some((a, b, c, d))
}

#[cfg(test)]
mod tests {
    use super::*;

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
