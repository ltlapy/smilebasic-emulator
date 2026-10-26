//! Sprite collision detection (SPCOL, SPHITSP, SPHITRC): rectangles that
//! may move during the frame, tested for the first moment they overlap.

/// SPCOL's settings of a sprite.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Collision {
    /// The rectangle, relative to the sprite's position.
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    /// Whether the rectangle follows SPSCALE.
    pub scale: bool,
    /// Bodies collide only if their masks share a bit.
    pub mask: u32,
}

/// Something that can collide: a rectangle at a position, moving by `vel`
/// over the frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Body {
    /// Left, top, right and bottom, right and bottom excluded.
    pub rect: [f64; 4],
    /// The point SPHITINFO reports: a sprite's position or a rectangle's
    /// top-left.
    pub pos: (f64, f64),
    pub vel: (f64, f64),
    pub mask: u32,
}

impl Body {
    /// A rectangle given by its top-left and size.
    pub fn rect(x: f64, y: f64, w: f64, h: f64, vel: (f64, f64), mask: u32) -> Body {
        Body {
            rect: [x, y, x + w, y + h],
            pos: (x, y),
            vel,
            mask,
        }
    }

    /// A sprite's body at `pos`, scaled by `scale` if its settings ask.
    pub fn sprite(c: &Collision, pos: (f64, f64), scale: (f64, f64), vel: (f64, f64)) -> Body {
        let (sx, sy) = if c.scale { scale } else { (1.0, 1.0) };
        let x = [f64::from(c.x) * sx, f64::from(c.x + c.w) * sx];
        let y = [f64::from(c.y) * sy, f64::from(c.y + c.h) * sy];
        Body {
            rect: [
                pos.0 + x[0].min(x[1]),
                pos.1 + y[0].min(y[1]),
                pos.0 + x[0].max(x[1]),
                pos.1 + y[0].max(y[1]),
            ],
            pos,
            vel,
            mask: c.mask,
        }
    }

    pub fn at(&self, t: f64) -> (f64, f64) {
        (self.pos.0 + self.vel.0 * t, self.pos.1 + self.vel.1 * t)
    }
}

/// The first moment in 0 to 1 at which `a` and `b` overlap, if they do.
// UNVERIFIED: rectangles that only touch at an edge do not collide.
pub fn sweep(a: &Body, b: &Body) -> Option<f64> {
    if a.mask & b.mask == 0 {
        return None;
    }
    let mut enter = f64::NEG_INFINITY;
    let mut exit = f64::INFINITY;
    let v = [a.vel.0 - b.vel.0, a.vel.1 - b.vel.1];
    for (axis, v) in v.into_iter().enumerate() {
        let (a0, a1) = (a.rect[axis], a.rect[axis + 2]);
        let (b0, b1) = (b.rect[axis], b.rect[axis + 2]);
        if a0 >= a1 || b0 >= b1 {
            return None;
        }
        if v == 0.0 {
            if a0 >= b1 || a1 <= b0 {
                return None;
            }
            continue;
        }
        let t0 = (b0 - a1) / v;
        let t1 = (b1 - a0) / v;
        enter = enter.max(t0.min(t1));
        exit = exit.min(t0.max(t1));
    }
    (enter < exit && enter < 1.0 && exit > 0.0).then_some(enter.max(0.0))
}

/// A search for the sprites a body hits, which SPHITSP() and SPHITRC()
/// without arguments go on with.
#[derive(Clone, Debug, PartialEq)]
pub struct Search {
    pub source: Body,
    /// Sprites still to test, the next one last.
    pub rest: Vec<usize>,
    /// The sprite the search is for, which cannot hit itself.
    pub skip: Option<usize>,
}

impl Search {
    /// Tests sprites `first` to `last`, upwards from the lower one either
    /// way (checked on hardware for SPHITSP).
    pub fn new(source: Body, first: usize, last: usize, skip: Option<usize>) -> Search {
        let rest: Vec<usize> = (first.min(last)..=first.max(last)).rev().collect();
        Search { source, rest, skip }
    }
}

/// What SPHITINFO reports about the last collision found.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct HitInfo {
    pub time: f64,
    pub pos: [(f64, f64); 2],
    pub vel: [(f64, f64); 2],
}

impl HitInfo {
    pub fn new(time: f64, a: &Body, b: &Body) -> HitInfo {
        HitInfo {
            time,
            pos: [a.at(time), b.at(time)],
            vel: [a.vel, b.vel],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: u32 = u32::MAX;

    fn still(x: f64, y: f64) -> Body {
        Body::rect(x, y, 10.0, 10.0, (0.0, 0.0), ALL)
    }

    #[test]
    fn overlapping_rectangles_collide_at_once() {
        assert_eq!(sweep(&still(0.0, 0.0), &still(9.0, 9.0)), Some(0.0));
        assert_eq!(sweep(&still(0.0, 0.0), &still(10.0, 0.0)), None);
        let mut b = still(5.0, 5.0);
        b.mask = 0b10;
        assert_eq!(
            sweep(
                &Body {
                    mask: 0b01,
                    ..still(0.0, 0.0)
                },
                &b
            ),
            None
        );
    }

    #[test]
    fn moving_rectangles_meet_on_the_way() {
        let a = Body::rect(0.0, 0.0, 10.0, 10.0, (40.0, 0.0), ALL);
        let b = still(30.0, 5.0);
        assert_eq!(sweep(&a, &b), Some(0.5));
        let info = HitInfo::new(0.5, &a, &b);
        assert_eq!(info.pos, [(20.0, 0.0), (30.0, 5.0)]);
        // Too short a move, or already past.
        assert_eq!(
            sweep(
                &Body {
                    vel: (20.0, 0.0),
                    ..a
                },
                &b
            ),
            None
        );
        assert_eq!(
            sweep(
                &Body {
                    vel: (-40.0, 0.0),
                    ..a
                },
                &b
            ),
            None
        );
        // Both moving the same way never meet.
        assert_eq!(
            sweep(
                &a,
                &Body {
                    vel: (40.0, 0.0),
                    ..b
                }
            ),
            None
        );
    }

    #[test]
    fn sprite_bodies_follow_the_scale_if_asked() {
        let c = Collision {
            x: -8,
            y: -8,
            w: 16,
            h: 16,
            scale: true,
            mask: ALL,
        };
        let b = Body::sprite(&c, (100.0, 50.0), (2.0, -1.0), (0.0, 0.0));
        assert_eq!(b.rect, [84.0, 42.0, 116.0, 58.0]);
        let b = Body::sprite(
            &Collision { scale: false, ..c },
            (100.0, 50.0),
            (2.0, 2.0),
            (0.0, 0.0),
        );
        assert_eq!(b.rect, [92.0, 42.0, 108.0, 58.0]);
    }
}
