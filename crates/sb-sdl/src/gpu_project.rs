//! LMATRIX's 4x4 form with no shader. `SDL_RenderGeometry` has no W, so a triangle
//! maps its image evenly, which a true perspective does not. An element is drawn as
//! a grid of small triangles, each corner exactly where the matrix puts it. Straight
//! edges stay straight and in place. The error is in the place of a texel inside a
//! cell. [`Plane::off_at`] measures it, and the grid is chosen to keep it small.
//!
//! Everything that decides what shows is linear on the element: X, Y, Z and W of a
//! point are each a sum of its coordinates. So the rules that the CPU tests pixel by
//! pixel, W above 0 and Z / W from -1 to 1, cut the element along straight lines.
//! Those cuts are made here before any drawing, the same as the frame's edges.

use sb_runtime::compose::{self, Affine};

/// The most cells along a side of a grid.
const MOST_CELLS: u32 = 256;
/// The most corners a cell has when cut by all the planes.
const MOST_CORNERS: usize = 4 + 7;

/// An element's plane through a 4x4 matrix.
#[derive(Clone, Copy)]
pub struct Plane {
    /// X, Y, Z and W, each as the row that `(x, y, 1)` on the element is
    /// multiplied by.
    rows: [[f64; 3]; 4],
    /// The frame's size.
    frame: (f64, f64),
}

/// A corner of a triangle: on the frame, and on the element.
#[derive(Clone, Copy, Debug)]
pub struct Corner {
    pub at: (f32, f32),
    pub point: (f64, f64),
}

impl Plane {
    /// `place` maps the element to the frame pixels of the layer drawn flat, with
    /// `sample` of those pixels per layer pixel. None if the matrix shows nothing
    /// on the CPU either.
    pub fn new(m: &[f64; 16], sample: u32, place: &Affine, frame: (u32, u32)) -> Option<Plane> {
        if !compose::can_project(m) || place.inverse().is_none() {
            return None;
        }
        let s = f64::from(sample);
        let rows = compose::clip_rows(m).map(|[x, y, one]| {
            [
                (x * place.a + y * place.c) / s,
                (x * place.b + y * place.d) / s,
                (x * place.tx + y * place.ty) / s + one,
            ]
        });
        Some(Plane {
            rows,
            frame: (f64::from(frame.0), f64::from(frame.1)),
        })
    }

    fn eval(row: [f64; 3], (x, y): (f64, f64)) -> f64 {
        row[0] * x + row[1] * y + row[2]
    }

    /// Projects a point in front onto the frame.
    fn on_frame(&self, p: (f64, f64)) -> (f64, f64) {
        let [rx, ry, _, rw] = self.rows;
        let w = Self::eval(rw, p);
        let (nx, ny) = (Self::eval(rx, p) / w, Self::eval(ry, p) / w);
        (
            (nx + 1.0) / 2.0 * self.frame.0,
            (1.0 - ny) / 2.0 * self.frame.1,
        )
    }

    /// The lines that bound what can show, each as a sum at least 0 on the side
    /// that shows: Z / W from -1 to 1, and a little past each edge of the frame,
    /// which also keeps W above 0.
    fn planes(&self) -> [[f64; 3]; 6] {
        let [rx, ry, rz, rw] = self.rows;
        // Two pixels past each edge, so that a cut there is not visible.
        let (mx, my) = (1.0 + 4.0 / self.frame.0, 1.0 + 4.0 / self.frame.1);
        let plus = |a: [f64; 3], k: f64, b: [f64; 3]| std::array::from_fn(|i| a[i] * k + b[i]);
        [
            plus(rw, 1.0, rz),
            plus(rw, 1.0, rz.map(|v| -v)),
            plus(rw, mx, rx),
            plus(rw, mx, rx.map(|v| -v)),
            plus(rw, my, ry),
            plus(rw, my, ry.map(|v| -v)),
        ]
    }

    /// The distance in frame pixels by which a texel at the middle of a run from
    /// `p` to `p + d` is off, when the run is mapped evenly and not by the true
    /// perspective.
    fn off_at(&self, p: (f64, f64), d: (f64, f64)) -> f64 {
        let (a, b) = (self.on_frame(p), self.on_frame((p.0 + d.0, p.1 + d.1)));
        let m = self.on_frame((p.0 + d.0 / 2.0, p.1 + d.1 / 2.0));
        ((a.0 + b.0) / 2.0 - m.0).hypot((a.1 + b.1) / 2.0 - m.1)
    }

    /// Passes to `triangle` the triangles that cover the visible part of the
    /// rectangle from `(0, 0)` to `size` on the element, off by at most about
    /// `tolerance` pixels inside. Returns the number of cells it was split into.
    pub fn triangles(
        &self,
        size: (f64, f64),
        tolerance: f64,
        mut triangle: impl FnMut([Corner; 3]),
    ) -> u32 {
        let planes = self.planes();
        let whole = [(0.0, 0.0), (size.0, 0.0), size, (0.0, size.1)];
        let mut outline = Corners::new(&whole);
        let mut through = Corners::default();
        let mut cut = false;
        for plane in &planes {
            cut |= cut_by(&outline, *plane, &mut through);
            std::mem::swap(&mut outline, &mut through);
        }
        if outline.n < 3 {
            return 0;
        }
        let points = &outline.points[..outline.n];
        let (mut x0, mut y0, mut x1, mut y1) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
        for p in points {
            (x0, y0) = (x0.min(p.0), y0.min(p.1));
            (x1, y1) = (x1.max(p.0), y1.max(p.1));
        }
        let (across, down) = self.grid(points, (x0, y0, x1, y1), tolerance);
        let (dx, dy) = ((x1 - x0) / f64::from(across), (y1 - y0) / f64::from(down));
        let point = |i: u32, j: u32| {
            // The far side is the exact point, not a sum that only comes close.
            let px = if i == across {
                x1
            } else {
                x0 + f64::from(i) * dx
            };
            let py = if j == down {
                y1
            } else {
                y0 + f64::from(j) * dy
            };
            (px, py)
        };
        let corner = |p: (f64, f64)| {
            let at = self.on_frame(p);
            Corner {
                at: (at.0 as f32, at.1 as f32),
                point: p,
            }
        };
        for j in 0..down {
            for i in 0..across {
                let cell = [
                    point(i, j),
                    point(i + 1, j),
                    point(i + 1, j + 1),
                    point(i, j + 1),
                ];
                let mut shown = Corners::new(&cell);
                if cut {
                    for plane in &planes {
                        cut_by(&shown, *plane, &mut through);
                        std::mem::swap(&mut shown, &mut through);
                    }
                }
                if shown.n < 3 {
                    continue;
                }
                let first = corner(shown.points[0]);
                for pair in shown.points[1..shown.n].windows(2) {
                    triangle([first, corner(pair[0]), corner(pair[1])]);
                }
            }
        }
        across * down
    }

    /// The number of cells across and down that keeps what [`Plane::off_at`]
    /// reports under `tolerance`, measured where the element is nearest,
    /// where the error is worst.
    fn grid(
        &self,
        points: &[(f64, f64)],
        (x0, y0, x1, y1): (f64, f64, f64, f64),
        tolerance: f64,
    ) -> (u32, u32) {
        let rw = self.rows[3];
        // With the same W everywhere, the mapping is even and one cell is enough.
        if rw[0] == 0.0 && rw[1] == 0.0 {
            return (1, 1);
        }
        let nearest = (points.iter().copied())
            .min_by(|a, b| Self::eval(rw, *a).total_cmp(&Self::eval(rw, *b)))
            .unwrap_or((x0, y0));
        // From there, into the element.
        let toward = |at: f64, lo: f64, hi: f64| if at - lo <= hi - at { 1.0 } else { -1.0 };
        let (sx, sy) = (toward(nearest.0, x0, x1), toward(nearest.1, y0, y1));
        let (mut across, mut down) = (1, 1);
        while across < MOST_CELLS || down < MOST_CELLS {
            let dx = (x1 - x0) / f64::from(across) * sx;
            let dy = (y1 - y0) / f64::from(down) * sy;
            let wide = self.off_at(nearest, (dx, 0.0));
            let high = self.off_at(nearest, (0.0, dy));
            let corner = self.off_at(nearest, (dx, dy));
            let (more_x, more_y) = (
                wide.max(corner) > tolerance && across < MOST_CELLS,
                high.max(corner) > tolerance && down < MOST_CELLS,
            );
            if !more_x && !more_y {
                break;
            }
            // Doubling only the side that matters, or the longer on the frame.
            match (more_x, more_y) {
                (true, false) => across *= 2,
                (false, true) => down *= 2,
                _ if wide >= high => across *= 2,
                _ => down *= 2,
            }
        }
        (across, down)
    }
}

/// A convex outline on the element.
#[derive(Clone, Copy, Default)]
struct Corners {
    points: [(f64, f64); MOST_CORNERS],
    n: usize,
}

impl Corners {
    fn new(points: &[(f64, f64)]) -> Corners {
        let mut out = Corners::default();
        out.points[..points.len()].copy_from_slice(points);
        out.n = points.len();
        out
    }
}

/// Writes into `to` the part of `from` where `plane` is at least 0. Returns
/// whether anything was cut off.
fn cut_by(from: &Corners, plane: [f64; 3], to: &mut Corners) -> bool {
    let side = |p: (f64, f64)| Plane::eval(plane, p);
    to.n = 0;
    let mut cut = false;
    let mut push = |p: (f64, f64)| {
        if to.n < MOST_CORNERS {
            to.points[to.n] = p;
            to.n += 1;
        }
    };
    for i in 0..from.n {
        let (a, b) = (from.points[i], from.points[(i + 1) % from.n]);
        let (sa, sb) = (side(a), side(b));
        // NaN is on neither side, and shows nothing.
        if sa >= 0.0 {
            push(a);
        } else {
            cut = true;
        }
        if (sa >= 0.0) != (sb >= 0.0) && sa.is_finite() && sb.is_finite() {
            let t = sa / (sa - sb);
            push((a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t));
        }
    }
    cut
}

#[cfg(test)]
mod tests {
    use super::*;
    use sb_runtime::compose::{Mixed, Placement, project_image};
    use sb_runtime::framebuffer::Framebuffer;

    const FRAME: (u32, u32) = (320, 180);

    /// `x` to the frame's width and `y` to its height, W growing to the right.
    fn leaning(lean: f64) -> [f64; 16] {
        let mut m = [0.0; 16];
        (m[0], m[12]) = (2.0 / 320.0, -1.0);
        (m[5], m[13]) = (-2.0 / 180.0, 1.0);
        (m[10], m[15]) = (1.0, 1.0);
        m[3] = lean;
        m
    }

    /// Pixels of the frame covered, by the triangles a plane makes and by the CPU.
    fn compare(m: &[f64; 16], place: &Affine, size: (i32, i32)) -> (u32, u32, u32) {
        let mut cpu = Framebuffer::new(FRAME.0, FRAME.1);
        cpu.clear(0);
        let placed = Placement {
            matrix: *place,
            clip: None,
            add: false,
        };
        project_image(&mut cpu, m, 1, &placed, size.0, size.1, |_, _| {
            Some(Mixed::Solid(0xFFFF_FFFF))
        });
        let mut got = vec![0_u8; (FRAME.0 * FRAME.1) as usize];
        let (w, h) = (f64::from(size.0), f64::from(size.1));
        if let Some(plane) = Plane::new(m, 1, place, FRAME) {
            plane.triangles((w, h), 0.25, |[a, b, c]| fill(&mut got, a.at, b.at, c.at));
        }
        let (mut both, mut only_cpu, mut only_grid) = (0, 0, 0);
        for (c, g) in cpu.pixels().iter().zip(&got) {
            match (*c != 0, *g != 0) {
                (true, true) => both += 1,
                (true, false) => only_cpu += 1,
                (false, true) => only_grid += 1,
                _ => {}
            }
        }
        (both, only_cpu, only_grid)
    }

    /// Marks the pixels whose centers a triangle covers.
    fn fill(out: &mut [u8], a: (f32, f32), b: (f32, f32), c: (f32, f32)) {
        let edge = |p: (f32, f32), q: (f32, f32), r: (f32, f32)| {
            (q.0 - p.0) * (r.1 - p.1) - (q.1 - p.1) * (r.0 - p.0)
        };
        let area = edge(a, b, c);
        if area == 0.0 {
            return;
        }
        for y in 0..FRAME.1 {
            for x in 0..FRAME.0 {
                let p = (x as f32 + 0.5, y as f32 + 0.5);
                let (u, v, w) = (edge(a, b, p), edge(b, c, p), edge(c, a, p));
                if (u >= 0.0 && v >= 0.0 && w >= 0.0) || (u <= 0.0 && v <= 0.0 && w <= 0.0) {
                    out[(y * FRAME.0 + x) as usize] = 1;
                }
            }
        }
    }

    fn shifted(tx: f64, ty: f64) -> Affine {
        Affine {
            tx,
            ty,
            ..Affine::IDENTITY
        }
    }

    /// Edges fall where the CPU's edges fall, within the error that ties on a
    /// line can cause.
    #[test]
    fn what_shows_is_where_the_cpu_shows_it() {
        for lean in [0.0, 0.001, 0.01, -0.002] {
            let (both, only_cpu, only_grid) =
                compare(&leaning(lean), &shifted(40.0, 20.0), (100, 80));
            assert!(both > 1000, "{lean}: {both}");
            assert!(
                only_cpu + only_grid < both / 50,
                "{lean}: {only_cpu} {only_grid} of {both}"
            );
        }
    }

    /// Behind the eye, W is below 0, and on the CPU nothing shows there.
    #[test]
    fn what_is_behind_does_not_show() {
        let mut m = leaning(-0.01);
        // X = 0 at the layer's left, so the part in front, x below 100, shows.
        m[12] = 0.0;
        let (both, only_cpu, only_grid) = compare(&m, &shifted(0.0, 20.0), (200, 80));
        assert!(both > 1000, "{both}");
        assert!(
            only_cpu + only_grid < both / 50,
            "{only_cpu} {only_grid} of {both}"
        );
        // All of it is past the line where W falls to 0.
        let (both, _, only_grid) = compare(&m, &shifted(150.0, 20.0), (40, 40));
        assert_eq!((both, only_grid), (0, 0));
    }

    /// Z / W from -1 to 1 is a line across the element, the same as on the CPU.
    #[test]
    fn depth_cuts_where_the_cpu_cuts() {
        let mut m = leaning(0.0);
        // Z = x / 60 - 2: the layer's x from 60 to 180 shows.
        (m[2], m[14]) = (1.0 / 60.0, -2.0);
        let (both, only_cpu, only_grid) = compare(&m, &shifted(0.0, 0.0), (320, 100));
        assert_eq!(both, 120 * 100);
        assert!(only_cpu + only_grid <= 200, "{only_cpu} {only_grid}");
    }

    #[test]
    fn a_matrix_with_no_inverse_shows_nothing() {
        let m = [0.0; 16];
        assert!(Plane::new(&m, 1, &Affine::IDENTITY, FRAME).is_none());
    }

    /// The grid splits more finely with more lean, and not at all with none.
    #[test]
    fn a_grid_grows_with_the_perspective() {
        let cells = |lean: f64| {
            let plane = Plane::new(&leaning(lean), 1, &Affine::IDENTITY, FRAME).unwrap();
            plane.triangles((320.0, 180.0), 0.25, |_| {})
        };
        assert_eq!(cells(0.0), 1);
        assert!(cells(0.001) > 1);
        assert!(cells(0.01) > cells(0.001));
        assert!(cells(0.01) <= MOST_CELLS * MOST_CELLS);
    }

    /// A texel inside a cell is off by about the tolerance at the most.
    #[test]
    fn what_is_inside_is_near_the_true_place() {
        let plane = Plane::new(&leaning(0.003), 1, &Affine::IDENTITY, FRAME).unwrap();
        let mut worst: f64 = 0.0;
        plane.triangles((320.0, 180.0), 0.25, |[a, b, _]| {
            worst =
                worst.max(plane.off_at(a.point, (b.point.0 - a.point.0, b.point.1 - a.point.1)));
        });
        assert!(worst > 0.0 && worst <= 0.5, "{worst}");
    }
}
