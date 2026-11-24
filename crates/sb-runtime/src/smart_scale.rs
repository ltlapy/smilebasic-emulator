//! XSCREEN's composite mode 1 in integers. It is the reference for the
//! output of the frontend's GPU route, and the fallback of that route.
//!
//! For a point `s` texels along an axis, the mode mixes the two texels
//! around it by `t = clamp(2 * fract(s - .5) - .5, 0, 1)`. That is plain
//! nearest, except in the middle half of the distance between two centers.
//! It is the same as bilinear sampling of a copy of the frame made twice
//! as large with nearest, which is how the frontend gets it from a GPU with
//! no shader.

/// The two texels and the mix, in 256ths, for one output pixel of an axis.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Step {
    first: u32,
    second: u32,
    part: u32,
}

/// The steps of an axis of `size` texels shown over `shown` pixels.
fn steps(size: u32, shown: u32) -> Vec<Step> {
    let last = i64::from(size) - 1;
    (0..shown)
        .map(|x| {
            let s = (f64::from(x) + 0.5) / f64::from(shown) * f64::from(size) - 0.5;
            let base = s.floor();
            let t = (2.0 * (s - base) - 0.5).clamp(0.0, 1.0);
            let base = base as i64;
            Step {
                first: base.clamp(0, last) as u32,
                second: (base + 1).clamp(0, last) as u32,
                part: (t * 256.0 + 0.5) as u32,
            }
        })
        .collect()
}

/// Scales frames of one size to one size, and keeps its lookups and lines
/// from frame to frame.
#[derive(Default)]
pub struct SmartScale {
    from: (u32, u32),
    to: (u32, u32),
    across: Vec<Step>,
    down: Vec<Step>,
    /// A source row mixed across, and its row number.
    lines: [(u32, Vec<u32>); 2],
}

/// `a` and `b` mixed with `part` 256ths of `b`, with opaque alpha.
#[inline]
fn mix(a: u32, b: u32, part: u32) -> u32 {
    const LO: u32 = 0x00FF_00FF;
    let keep = 256 - part;
    let lo = ((a & LO) * keep + (b & LO) * part + 0x0080_0080) >> 8 & LO;
    let hi = ((a >> 8 & 0xFF) * keep + (b >> 8 & 0xFF) * part + 0x80) & 0xFF00;
    0xFF00_0000 | lo | hi
}

impl SmartScale {
    /// Makes `out`, `to.0` x `to.1`, from `from`, a frame of `size`.
    pub fn scale(&mut self, from: &[u32], size: (u32, u32), out: &mut [u32], to: (u32, u32)) {
        let (w, shown) = (size.0 as usize, to.0 as usize);
        if w == 0 || size.1 == 0 || shown == 0 || to.1 == 0 {
            return;
        }
        if (self.from, self.to) != (size, to) {
            (self.from, self.to) = (size, to);
            self.across = steps(size.0, to.0);
            self.down = steps(size.1, to.1);
            for line in &mut self.lines {
                line.1.resize(shown, 0);
            }
        }
        for line in &mut self.lines {
            line.0 = u32::MAX;
        }
        let across = &self.across;
        // Each needed row is either held already or replaces the line that
        // does not hold the other row needed with it.
        let hold = |lines: &mut [(u32, Vec<u32>); 2], row: u32, keep: u32| {
            if lines[0].0 == row || lines[1].0 == row {
                return;
            }
            let line = &mut lines[usize::from(lines[0].0 == keep)];
            line.0 = row;
            let source = &from[row as usize * w..][..w];
            for (o, s) in line.1.iter_mut().zip(across) {
                *o = mix(source[s.first as usize], source[s.second as usize], s.part);
            }
        };
        for (row, s) in out.chunks_exact_mut(shown).zip(&self.down) {
            hold(&mut self.lines, s.first, s.second);
            if s.part != 0 {
                hold(&mut self.lines, s.second, s.first);
            }
            let lines = &self.lines;
            let line = |n: u32| &lines[usize::from(lines[0].0 != n)].1;
            match s.part {
                0 => row.copy_from_slice(line(s.first)),
                256 => row.copy_from_slice(line(s.second)),
                part => {
                    let (top, bottom) = (line(s.first), line(s.second));
                    for ((o, &t), &b) in row.iter_mut().zip(top).zip(bottom) {
                        *o = mix(t, b, part);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steps_follow_the_formula() {
        // Three times larger: a third of each texel is pure nearest, and
        // the rest mixes by 5/6, 1/6.
        let s = steps(4, 12);
        let part = |t: f64| (t * 256.0 + 0.5) as u32;
        assert_eq!(s[0].part, part(5.0 / 6.0));
        assert_eq!((s[0].first, s[0].second), (0, 0));
        assert_eq!((s[1].first, s[1].second, s[1].part), (0, 1, 0));
        assert_eq!(
            (s[2].first, s[2].second, s[2].part),
            (0, 1, part(1.0 / 6.0))
        );
        assert_eq!(
            (s[3].first, s[3].second, s[3].part),
            (0, 1, part(5.0 / 6.0))
        );
        assert_eq!((s[4].first, s[4].second, s[4].part), (1, 2, 0));
        assert_eq!((s[11].first, s[11].second), (3, 3));
    }

    fn scaled(from: &[u32], size: (u32, u32), to: (u32, u32)) -> Vec<u32> {
        let mut out = vec![0; (to.0 * to.1) as usize];
        SmartScale::default().scale(from, size, &mut out, to);
        out
    }

    #[test]
    fn at_size_it_is_the_frame_and_twice_is_nearest() {
        let from = [0xFF11_2233, 0xFF44_5566, 0xFF77_8899, 0xFFAA_BBCC];
        assert_eq!(scaled(&from, (2, 2), (2, 2)), from);
        let out = scaled(&from, (2, 2), (4, 4));
        let want = [from[0], from[0], from[1], from[1]];
        assert_eq!(out[..4], want);
        assert_eq!(out[4..8], want);
        assert_eq!(out[8], from[2]);
    }

    #[test]
    fn three_times_shows_the_measured_checker_row() {
        // A one-pixel checkerboard, red channel, at 3x, on a row with no
        // vertical mix: 0 42 212 255 212 42 0 on SB4's screenshot.
        let (w, h) = (6, 3);
        let from: Vec<u32> = (0..w * h)
            .map(|i| match (i % w + i / w) % 2 {
                1 => 0xFFFF_FFFF,
                _ => 0xFF00_0000,
            })
            .collect();
        let out = scaled(&from, (w, h), (w * 3, h * 3));
        let row: Vec<u32> = out[(w * 3 * 4) as usize..][..7]
            .iter()
            .map(|p| p >> 16 & 0xFF)
            .collect();
        assert_eq!(row, [255, 255, 212, 43, 0, 43, 212]);
    }

    #[test]
    fn every_scale_mixes_each_axis_alone() {
        // The result is the same when computed one pixel at a time, at
        // scales up and down, with rows skipped and repeated.
        let (w, h) = (7u32, 5u32);
        let from: Vec<u32> = (0..w * h)
            .map(|i| 0xFF00_0000 | (i * 37 % 256) << 16 | (i * 91 % 256) << 8 | (i * 13 % 256))
            .collect();
        for to in [(21, 15), (10, 7), (3, 2), (7, 40), (1, 1), (64, 9)] {
            let out = scaled(&from, (w, h), to);
            let (across, down) = (steps(w, to.0), steps(h, to.1));
            for (i, &o) in out.iter().enumerate() {
                let (x, y) = (i % to.0 as usize, i / to.0 as usize);
                let (sx, sy) = (across[x], down[y]);
                let at = |c: u32, r: u32| from[(r * w + c) as usize];
                let line = |r: u32| mix(at(sx.first, r), at(sx.second, r), sx.part);
                assert_eq!(
                    o,
                    mix(line(sy.first), line(sy.second), sy.part),
                    "{to:?} {x} {y}"
                );
            }
        }
    }

    /// The cost of a frame: `cargo test --release -p sb-runtime
    /// smart_scale_cost -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn smart_scale_cost() {
        let (size, to) = ((400, 240), (1200, 720));
        let from: Vec<u32> = (0..size.0 * size.1)
            .map(|i: u32| 0xFF00_0000 | i.wrapping_mul(0x9E37_79B9) >> 8)
            .collect();
        let mut out = vec![0; (to.0 * to.1) as usize];
        let mut scale = SmartScale::default();
        let start = std::time::Instant::now();
        for _ in 0..200 {
            scale.scale(&from, size, &mut out, to);
        }
        println!("{:?} a frame", start.elapsed() / 200);
        assert_ne!(out[0], 0);
    }
}
