//! Holding a controller's D-pad or left stick up or down in the
//! emulator's own screens (the projects, the file viewer) repeats the
//! step, faster the longer it is held. The stick is also read as the
//! D-pad there. Running programs never see any of this.

use std::time::Duration;

use sdl3::gamepad::Button;

/// Held this long, a direction starts repeating.
const DELAY: Duration = Duration::from_millis(300);
/// Steps a second when the repeat starts, and after `FAST_AT` held.
const SLOW_RATE: f32 = 8.0;
const FAST_RATE: f32 = 20.0;
/// The rate rises in a line from `SLOW_RATE` to `FAST_RATE` by this time
/// since the press, in seconds.
const FAST_AT: f32 = 1.5;
/// The most steps one tick gives, so that a stalled frame does not skip
/// the whole list.
const MOST_STEPS: u32 = 4;
/// A stick is pushed over at this share of its travel, and let go of
/// under the lower one; between them it keeps what it was.
const PRESS: f32 = 0.7;
const RELEASE: f32 = 0.4;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Dir {
    Up,
    Down,
    Left,
    Right,
}

impl Dir {
    pub fn button(self) -> Button {
        match self {
            Dir::Up => Button::DPadUp,
            Dir::Down => Button::DPadDown,
            Dir::Left => Button::DPadLeft,
            Dir::Right => Button::DPadRight,
        }
    }

    fn vertical(self) -> bool {
        matches!(self, Dir::Up | Dir::Down)
    }
}

#[derive(Default)]
pub struct PadRepeat {
    /// The direction repeating, up or down, and for how long it has been
    /// held (`credit` is the part of a step owed).
    held: Option<Dir>,
    time: Duration,
    credit: f32,
    /// Which of Up and Down a D-pad button holds.
    dpad: [bool; 2],
    /// The direction the left stick is pushed over, if any.
    stick: Option<Dir>,
    axes: (f32, f32),
}

impl PadRepeat {
    /// A D-pad button went down; the caller takes the single step.
    pub fn down(&mut self, dir: Dir) {
        if dir.vertical() {
            self.dpad[usize::from(dir == Dir::Down)] = true;
            self.press(dir);
        }
    }

    pub fn up(&mut self, dir: Dir) {
        if dir.vertical() {
            self.dpad[usize::from(dir == Dir::Down)] = false;
            self.release(dir);
        }
    }

    /// The left stick's horizontal or vertical axis moved (positive is
    /// right, or down as in SDL). Returns the direction when it has just
    /// been pushed over, for the caller to take the single step.
    pub fn stick_x(&mut self, value: i16) -> Option<Dir> {
        self.axes.0 = f32::from(value) / f32::from(i16::MAX);
        self.stick_moved()
    }

    pub fn stick_y(&mut self, value: i16) -> Option<Dir> {
        self.axes.1 = f32::from(value) / f32::from(i16::MAX);
        self.stick_moved()
    }

    /// Stops the repeat and forgets the buttons held, as a screen closes
    /// or changes: what is held acts again only when pressed again. A
    /// stick stays pushed over until it is let go.
    pub fn reset(&mut self) {
        self.held = None;
        self.dpad = [false; 2];
    }

    /// Time passed: the direction to step in and how many times, if the
    /// repeat is due.
    pub fn tick(&mut self, dt: Duration) -> Option<(Dir, u32)> {
        let dir = self.held?;
        let before = self.time;
        self.time += dt;
        if self.time < DELAY {
            return None;
        }
        // The first step is due as the delay ends, then the rate rises
        // over the span of this tick, taken at its middle.
        let from = before.max(DELAY);
        if before < DELAY {
            self.credit = 1.0;
        }
        let delay = DELAY.as_secs_f32();
        let middle = (from + self.time).as_secs_f32() / 2.0;
        let rise = ((middle - delay) / (FAST_AT - delay)).clamp(0.0, 1.0);
        let rate = SLOW_RATE + (FAST_RATE - SLOW_RATE) * rise;
        self.credit += rate * (self.time - from).as_secs_f32();
        let steps = (self.credit as u32).min(MOST_STEPS);
        self.credit = (self.credit - steps as f32).min(1.0);
        (steps > 0).then_some((dir, steps))
    }

    fn press(&mut self, dir: Dir) {
        if dir.vertical() {
            (self.held, self.time, self.credit) = (Some(dir), Duration::ZERO, 0.0);
        }
    }

    /// Stops the repeat of `dir` unless the D-pad or the stick still
    /// holds it.
    fn release(&mut self, dir: Dir) {
        let still = self.stick == Some(dir) || self.dpad[usize::from(dir == Dir::Down)];
        if self.held == Some(dir) && !still {
            self.held = None;
        }
    }

    fn stick_moved(&mut self) -> Option<Dir> {
        let (x, y) = self.axes;
        let now = direction(x, y, self.stick);
        if now == self.stick {
            return None;
        }
        let before = std::mem::replace(&mut self.stick, now);
        if let Some(d) = before {
            self.release(d);
        }
        if let Some(d) = now {
            self.press(d);
        }
        now
    }
}

/// The direction a stick at `x`, `y` is pushed over in, given the one it
/// was in. Only the major axis counts when it is pushed diagonally.
fn direction(x: f32, y: f32, was: Option<Dir>) -> Option<Dir> {
    let (ax, ay) = (x.abs(), y.abs());
    if let Some(d) = was {
        let (along, across) = match d {
            Dir::Up => (-y, ax),
            Dir::Down => (y, ax),
            Dir::Left => (-x, ay),
            Dir::Right => (x, ay),
        };
        if along >= RELEASE && along >= across {
            return was;
        }
    }
    if ax >= ay {
        (ax >= PRESS).then_some(if x < 0.0 { Dir::Left } else { Dir::Right })
    } else {
        (ay >= PRESS).then_some(if y < 0.0 { Dir::Up } else { Dir::Down })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FRAME: Duration = Duration::from_micros(16_667);

    /// Runs frames for `secs` seconds; the steps given.
    fn run(r: &mut PadRepeat, secs: f32) -> u32 {
        let frames = (secs * 60.0).round() as u32;
        (0..frames)
            .filter_map(|_| r.tick(FRAME))
            .map(|(_, n)| n)
            .sum()
    }

    fn at(v: f32) -> i16 {
        (v * f32::from(i16::MAX)) as i16
    }

    #[test]
    fn waits_for_the_delay_then_steps() {
        let mut r = PadRepeat::default();
        assert_eq!(r.tick(FRAME), None);
        r.down(Dir::Down);
        assert_eq!(run(&mut r, 0.28), 0);
        assert_eq!(r.tick(Duration::from_millis(30)), Some((Dir::Down, 1)));
    }

    #[test]
    fn speeds_up_the_longer_it_is_held() {
        let mut r = PadRepeat::default();
        r.down(Dir::Up);
        run(&mut r, 0.3);
        let early = run(&mut r, 0.5);
        run(&mut r, 0.7);
        let late = run(&mut r, 0.5);
        assert!(late > early * 3 / 2, "{early} {late}");
        // Past the speed-up it holds about 20 steps a second.
        assert!((19..=21).contains(&run(&mut r, 1.0)));
    }

    #[test]
    fn release_stops_it() {
        let mut r = PadRepeat::default();
        r.down(Dir::Down);
        run(&mut r, 0.5);
        r.up(Dir::Down);
        assert_eq!(run(&mut r, 1.0), 0);
    }

    #[test]
    fn the_opposite_direction_restarts_it() {
        let mut r = PadRepeat::default();
        r.down(Dir::Down);
        run(&mut r, 1.0);
        r.down(Dir::Up);
        assert_eq!(run(&mut r, 0.28), 0);
        assert_eq!(r.tick(Duration::from_millis(30)), Some((Dir::Up, 1)));
        // Letting go of the first one does not stop the second.
        r.up(Dir::Down);
        assert!(run(&mut r, 0.5) > 0);
    }

    #[test]
    fn left_and_right_do_not_repeat() {
        let mut r = PadRepeat::default();
        r.down(Dir::Left);
        r.down(Dir::Right);
        assert_eq!(run(&mut r, 2.0), 0);
        assert_eq!(r.stick_x(at(1.0)), Some(Dir::Right));
        assert_eq!(run(&mut r, 2.0), 0);
    }

    #[test]
    fn a_stalled_frame_does_not_flood() {
        let mut r = PadRepeat::default();
        r.down(Dir::Down);
        let stalled = r.tick(Duration::from_secs(5));
        assert_eq!(stalled, Some((Dir::Down, MOST_STEPS)));
    }

    #[test]
    fn stick_has_hysteresis() {
        let mut r = PadRepeat::default();
        assert_eq!(r.stick_y(at(-0.6)), None);
        assert_eq!(r.stick_y(at(-0.75)), Some(Dir::Up));
        // Near the press threshold nothing chatters.
        for v in [0.68, 0.72, 0.5, 0.45, 0.7] {
            assert_eq!(r.stick_y(at(-v)), None);
        }
        assert!(run(&mut r, 0.5) > 0);
        assert_eq!(r.stick_y(at(-0.3)), None);
        assert_eq!(run(&mut r, 1.0), 0);
        // Having let go, it has to go past the press threshold again.
        assert_eq!(r.stick_y(at(-0.6)), None);
        assert_eq!(r.stick_y(at(-0.8)), Some(Dir::Up));
    }

    #[test]
    fn stick_down_is_positive_and_repeats() {
        let mut r = PadRepeat::default();
        assert_eq!(r.stick_y(at(0.9)), Some(Dir::Down));
        assert!(run(&mut r, 1.0) > 5);
    }

    #[test]
    fn diagonal_takes_the_major_axis() {
        let mut r = PadRepeat::default();
        assert_eq!(r.stick_x(at(0.8)), Some(Dir::Right));
        // Up grows past it: the push moves over to up.
        assert_eq!(r.stick_y(at(-0.9)), Some(Dir::Up));
        assert_eq!(r.stick_y(at(-0.85)), None);
        assert_eq!(r.stick_x(at(0.1)), None);
    }

    #[test]
    fn reset_leaves_nothing_to_the_next_screen() {
        let mut r = PadRepeat::default();
        r.down(Dir::Down);
        r.reset();
        assert_eq!(run(&mut r, 1.0), 0);
        // Letting go and pressing again works.
        r.up(Dir::Down);
        r.down(Dir::Down);
        assert!(run(&mut r, 1.0) > 0);
        // A stick still pushed over does not act until it is let go.
        let mut r = PadRepeat::default();
        assert_eq!(r.stick_y(at(0.9)), Some(Dir::Down));
        r.reset();
        assert_eq!(r.stick_y(at(0.95)), None);
        assert_eq!(run(&mut r, 1.0), 0);
        assert_eq!(r.stick_y(at(0.0)), None);
        assert_eq!(r.stick_y(at(0.9)), Some(Dir::Down));
    }
}
