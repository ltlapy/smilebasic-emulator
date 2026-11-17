//! Keeping the program's frames, and the sound they make, on time
//! whatever happens to the drawing.
//!
//! A compositor holds back the presents of a window it does not show: a
//! covered, minimized or hidden one, or one on another workspace. SDL's
//! Wayland backend then waits up to 50 ms for each present, and others
//! may wait longer. The frames are run on the main loop, a frame of sound
//! at a time, so a loop held back by its presents makes too little sound
//! and the device runs out. So the loop counts its frames by the clock,
//! does not present a window the system reports as not shown, and turns
//! vsync off for a window whose presents take far longer than a frame.

use std::time::{Duration, Instant};

use sdl3::render::Canvas;
use sdl3::sys::render::{SDL_FlushRenderer, SDL_SetRenderVSync};
use sdl3::sys::video::{SDL_WINDOW_HIDDEN, SDL_WINDOW_MINIMIZED, SDL_WINDOW_OCCLUDED};
use sdl3::video::Window;

use crate::FRAME;
use crate::bench::Bench;

/// A present that takes this long was held back by the compositor rather
/// than by vsync, which waits a frame at most.
const SLOW_PRESENT: Duration = Duration::from_millis(40);
/// Presents after the window's size changed that may be slow without counting
/// as held back: the driver is making buffers of the new size, and a compositor
/// repaints what the drag moved. A second's worth, to outlast a drag's pauses.
const RESIZE_GRACE: u32 = 60;
/// How far a frame's start may move to follow the display's vsync, as
/// against the clock: 0.3% of a frame, inside what the sound's rate
/// control makes up for (0.5%).
const MOST_DRIFT: Duration = Duration::from_nanos(50_000);
/// The display rates that frames follow vsync at, in Hz.
const LOCKED_RATES: std::ops::RangeInclusive<f32> = 59.7..=60.3;
/// Behind by more than this, as after the system stopped the emulator
/// for a while, the loop drops the time rather than catching up: the
/// sound has run out by then anyway.
const MOST_BEHIND: Duration = Duration::from_nanos(8 * 1_000_000_000 / 60);

/// The least time a frame gives a program that does not wait for the next
/// one: what every frame gave before the time was sized to the frame.
pub const LEAST_RUN_TIME: Duration = Duration::from_millis(10);
/// The most: past this, a slow trip round the loop would leave too little
/// for a drawing that takes longer than the last ones did.
const MOST_RUN_TIME: Duration = Duration::from_millis(14);
/// Left free in each frame besides what the rest of the trip took: for a
/// present's own work, which a present that waits for vsync cannot be told
/// from its wait, and for a drawing a little slower than the last ones.
const RUN_MARGIN: Duration = Duration::from_millis(2);
/// How far each trip moves [`RunTime`]'s estimate towards what it took, as
/// a fraction 1/N.
const RUN_SMOOTHING: u32 = 8;

/// How long a frame runs a program that does not wait for the next one:
/// the frame less the rest of a trip round the loop (the system's events,
/// drawing, presenting) and [`RUN_MARGIN`], from
/// [`LEAST_RUN_TIME`] to [`MOST_RUN_TIME`]. The frame is SmileBASIC's,
/// 1/60 s, whatever the display.
///
/// The rest is a moving average: each trip moves it an eighth of the way
/// to what that trip took, at most a frame. One slow drawing then takes no
/// more than 2 ms from the next frame's time, and a scene that starts to
/// draw more has the time it needs within a few frames.
#[derive(Clone, Copy, Debug)]
pub struct RunTime {
    rest: Duration,
}

impl Default for RunTime {
    /// Starts with no time to spare, as if the rest took a whole frame.
    fn default() -> Self {
        RunTime { rest: FRAME }
    }
}

impl RunTime {
    /// Notes a trip round the loop that drew a frame: what it took besides
    /// the program's frames and the waits, or `None` when its present
    /// waited for the vsync of a display that does not refresh at 60 Hz,
    /// which then holds up the loop for up to one of its own frames
    /// besides: the program gets the least time.
    pub fn drew(&mut self, rest: Option<Duration>) {
        let rest = rest.unwrap_or(FRAME).min(FRAME);
        let (now, to) = (self.rest.as_nanos() as i64, rest.as_nanos() as i64);
        let moved = now + (to - now) / i64::from(RUN_SMOOTHING);
        self.rest = Duration::from_nanos(moved as u64);
    }

    /// The time the program gets in the next frame.
    pub fn limit(&self) -> Duration {
        let left = FRAME.saturating_sub(self.rest + RUN_MARGIN);
        left.clamp(LEAST_RUN_TIME, MOST_RUN_TIME)
    }
}

/// The emulated frames' clock.
#[derive(Clone, Copy)]
pub struct Clock {
    /// When the frame being run began, or is to begin.
    next: Instant,
}

impl Clock {
    pub fn new(now: Instant) -> Clock {
        Clock { next: now }
    }

    /// Whether the frame after the one being run is due, so that the loop
    /// is to run it at once to catch up.
    pub fn behind(&self, now: Instant) -> bool {
        now >= self.next + FRAME
    }

    /// When [`Clock::behind`] next becomes true.
    pub fn due(&self) -> Instant {
        self.next + FRAME
    }

    /// Counts a frame run to catch up.
    pub fn caught_up(&mut self) {
        self.next += FRAME;
    }

    /// Whether a drawing that ends at `end` would end after the frame's
    /// time.
    pub fn late_at(&self, end: Instant) -> bool {
        end > self.next + FRAME
    }

    /// Ends a frame at `now`, after drawing it; `vsync` is whether a
    /// present waited for the vsync of a display at about 60 Hz. Gives
    /// how long to wait for the next frame.
    pub fn end(&mut self, now: Instant, vsync: bool) -> Duration {
        self.next += FRAME;
        if vsync && now + MOST_DRIFT > self.next {
            // Following the display, a little at a time, keeps the frames
            // in step with its vsync without a drift that would make the
            // loop wait for one vsync too many now and then. A present
            // that missed a vsync moves the frame by little, and one that
            // came back well before its time did not wait at all: the
            // clock does the rest.
            self.next = now.min(self.next + MOST_DRIFT);
        }
        if self.next > now {
            self.next - now
        } else {
            if now - self.next > MOST_BEHIND {
                self.next = now;
            }
            Duration::ZERO
        }
    }
}

/// Waits `time` more precisely than `std::thread::sleep`, which on older
/// Windows wakes on a coarse timer tick.
pub fn delay(time: Duration) {
    if !time.is_zero() {
        // SAFETY: SDL_DelayPrecise takes any duration and needs no state.
        unsafe { sdl3::sys::timer::SDL_DelayPrecise(time.as_nanos() as u64) };
    }
}

/// Whether the system reports `window` as not shown: covered, minimized
/// or hidden.
pub fn hidden(window: &Window) -> bool {
    let flags = window.window_flags().0;
    let not_shown = SDL_WINDOW_OCCLUDED.0 | SDL_WINDOW_MINIMIZED.0 | SDL_WINDOW_HIDDEN.0;
    flags & not_shown != 0
}

/// Whether the display `window` is on refreshes at about 60 Hz.
pub fn at_60_hz(window: &Window) -> bool {
    let mode = window.get_display().and_then(|d| d.get_mode());
    mode.is_ok_and(|m| LOCKED_RATES.contains(&m.refresh_rate))
}

/// A window's presents: whether they wait for vsync, and how long they
/// take.
pub struct Presents {
    /// Vsync is asked for.
    wanted: bool,
    /// Vsync is on.
    vsync: bool,
    /// Presents left that a resize excuses.
    excused: u32,
}

/// Whether a present that took `took` shows that the compositor holds the window's
/// presents back; `excused` is whether a resize explains it.
fn held_back(took: Duration, excused: bool) -> bool {
    !excused && took > SLOW_PRESENT
}

impl Presents {
    /// Turns vsync on for `canvas` if `wanted`.
    pub fn new(canvas: &Canvas<Window>, wanted: bool) -> Presents {
        let mut presents = Presents {
            wanted,
            vsync: false,
            excused: 0,
        };
        if wanted {
            presents.set_vsync(canvas, true);
            if !presents.vsync {
                eprintln!("vsync unavailable: {}", sdl3::get_error());
            }
        }
        presents
    }

    fn set_vsync(&mut self, canvas: &Canvas<Window>, on: bool) {
        // SAFETY: the renderer pointer is valid for the lifetime of
        // `canvas`.
        let set = unsafe { SDL_SetRenderVSync(canvas.raw(), i32::from(on)) };
        self.vsync = on && set;
    }

    /// Whether presents wait for vsync.
    pub fn vsync(&self) -> bool {
        self.vsync
    }

    /// The window's size has changed.
    pub fn resized(&mut self) {
        self.excused = RESIZE_GRACE;
    }

    /// Tries vsync again, as when the window is shown again after it was
    /// held back.
    pub fn retry(&mut self, canvas: &Canvas<Window>) {
        if self.wanted && !self.vsync {
            self.set_vsync(canvas, true);
        }
    }

    /// Presents what `canvas` drew, unless its window is not shown, when
    /// the drawing is only carried out; `bench` can stand for a
    /// compositor that covers the window or holds its presents back.
    /// Gives whether it presented.
    pub fn present(&mut self, canvas: &mut Canvas<Window>, bench: Option<&Bench>) -> bool {
        if hidden(canvas.window()) || bench.is_some_and(Bench::occluded) {
            // Drawing commands wait in the renderer until a present; carry
            // them out, so that they do not pile up while it is hidden.
            // SAFETY: the renderer pointer is valid for the lifetime of
            // `canvas`.
            unsafe { SDL_FlushRenderer(canvas.raw()) };
            return false;
        }
        let start = Instant::now();
        canvas.present();
        if let Some(b) = bench {
            b.present(self.vsync);
        }
        let excused = self.excused > 0;
        self.excused = self.excused.saturating_sub(1);
        if self.vsync && held_back(start.elapsed(), excused) {
            // Held back while the system says the window is shown, as
            // X11 never says it is covered: go by the clock instead,
            // until the window is shown again or gets the focus.
            self.set_vsync(canvas, false);
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Runs the clock for `frames` loop iterations, each taking `work`
    /// and then a present that returns at the display's next vsync (every
    /// `vsync_secs`) if there is one, and gives the frames run and the
    /// seconds they took; `locked` is whether the loop is told that the
    /// presents wait for vsync.
    fn run(frames: u32, work: Duration, vsync_secs: Option<f64>, locked: bool) -> (u32, f64) {
        let start = Instant::now();
        let mut now = start;
        let mut clock = Clock::new(now);
        let mut run = 0;
        for _ in 0..frames {
            run += 1;
            let mut skipped = 0;
            while skipped < crate::MOST_SKIPPED && clock.behind(now) {
                clock.caught_up();
                run += 1;
                skipped += 1;
            }
            now += work;
            if let Some(secs) = vsync_secs {
                let t = (now - start).as_secs_f64();
                now = start + Duration::from_secs_f64((t / secs).floor() * secs + secs);
            }
            now += clock.end(now, locked);
        }
        (run, (now - start).as_secs_f64())
    }

    fn rate((run, secs): (u32, f64)) -> f64 {
        run as f64 / secs
    }

    const WORK: Duration = Duration::from_millis(3);

    #[test]
    fn the_clock_runs_60_frames_a_second() {
        let rate = rate(run(6000, WORK, None, false));
        assert!((rate - 60.0).abs() < 0.01, "{rate}");
    }

    #[test]
    fn vsync_at_59_98_hz_is_followed_without_extra_frames() {
        let (run, secs) = run(6000, WORK, Some(1.0 / 59.98), true);
        assert_eq!(run, 6000);
        assert!((rate((run, secs)) - 59.98).abs() < 0.01, "{secs}");
    }

    #[test]
    fn vsync_that_does_not_wait_leaves_the_clock() {
        for work in [WORK, Duration::from_millis(16), Duration::from_millis(18)] {
            let rate = rate(run(6000, work, None, true));
            assert!((rate - 60.0).abs() < 60.0 * 0.004, "{work:?}: {rate}");
        }
    }

    #[test]
    fn other_vsync_moves_the_frames_by_little() {
        // Displays at 75 and 50 Hz are not followed, but were they, the
        // frames would stay within what the sound makes up for.
        for hz in [75.0, 50.0] {
            let rate = rate(run(6000, WORK, Some(1.0 / hz), true));
            assert!((rate - 60.0).abs() < 60.0 * 0.004, "{hz}: {rate}");
        }
    }

    #[test]
    fn slow_presents_are_caught_up() {
        // A compositor holding each present for 50 ms.
        let rate = rate(run(600, Duration::from_millis(52), None, false));
        assert!((rate - 60.0).abs() < 0.5, "{rate}");
    }

    #[test]
    fn a_slow_present_while_resizing_is_not_held_back() {
        let slow = SLOW_PRESENT + Duration::from_millis(1);
        assert!(held_back(slow, false));
        assert!(!held_back(slow, true));
        assert!(!held_back(SLOW_PRESENT, false));
    }

    fn ms(ms: f64) -> Duration {
        Duration::from_secs_f64(ms / 1000.0)
    }

    #[test]
    fn the_program_gets_what_the_frame_has_left() {
        let mut time = RunTime::default();
        assert_eq!(time.limit(), LEAST_RUN_TIME, "nothing measured yet");
        // A light drawing leaves the most; 16.7 - 3 - 2 ms in between.
        for (rest, limit) in [(0.5, 14.0), (3.0, 16.667 - 3.0 - 2.0), (6.0, 10.0)] {
            for _ in 0..100 {
                time.drew(Some(ms(rest)));
            }
            let got = time.limit().as_secs_f64() * 1000.0;
            assert!((got - limit).abs() < 0.01, "{rest}: {got}");
        }
    }

    #[test]
    fn one_slow_drawing_takes_little_from_the_next_frame() {
        let mut time = RunTime::default();
        for _ in 0..100 {
            time.drew(Some(ms(0.5)));
        }
        assert_eq!(time.limit(), MOST_RUN_TIME);
        // However slow, it counts as a frame at most.
        time.drew(Some(Duration::from_millis(300)));
        let limit = time.limit();
        assert!(limit >= ms(12.0) && limit < MOST_RUN_TIME, "{limit:?}");
        let back = (0..20).find(|_| {
            time.drew(Some(ms(0.5)));
            time.limit() >= ms(13.5)
        });
        assert!(back.is_some_and(|n| n < 10), "{back:?}");
    }

    #[test]
    fn a_display_not_at_60_hz_with_vsync_gets_the_least() {
        let mut time = RunTime::default();
        for _ in 0..100 {
            time.drew(Some(ms(1.0)));
        }
        for _ in 0..30 {
            time.drew(None);
        }
        assert_eq!(time.limit(), LEAST_RUN_TIME);
    }

    #[test]
    fn a_long_stall_drops_the_time() {
        let mut now = Instant::now();
        let mut clock = Clock::new(now);
        now += Duration::from_secs(2);
        assert_eq!(clock.end(now, false), Duration::ZERO);
        assert!(!clock.behind(now));
    }
}
