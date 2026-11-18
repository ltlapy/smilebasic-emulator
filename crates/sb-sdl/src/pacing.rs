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
pub const MOST_RUN_TIME: Duration = Duration::from_millis(14);
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

/// Frames in a row, at most, whose drawing is left out while the program
/// runs: one frame in five is drawn whatever the drawing costs, 12 a
/// second, so that the picture still moves. A program whose trip takes
/// nearly the 14 ms a frame gives needs that many for one drawing.
pub const MOST_UNDRAWN: u32 = 4;
/// What drawing the window costs, from the last three drawings: from
/// when the program's frame had run to when the present came back, without
/// the wait for vsync.
#[derive(Clone, Copy, Debug, Default)]
pub struct DrawTime {
    last: [Duration; 3],
}

impl DrawTime {
    /// Notes a drawing: what it took, and whether its present waited for
    /// vsync, which leaves out the present. [`RUN_MARGIN`] stands for the
    /// present's own work then, as it does for [`RunTime`].
    pub fn drew(&mut self, took: Duration, waited: bool) {
        let margin = if waited { RUN_MARGIN } else { Duration::ZERO };
        self.last = [self.last[1], self.last[2], took + margin];
    }

    /// What the next drawing is taken to cost: the middle one of the last
    /// three, so that one slow drawing, as when a texture is made, does
    /// not count, and a scene that starts to cost more does after two.
    pub fn cost(&self) -> Duration {
        let [a, b, c] = self.last;
        a.max(b).min(a.min(b).max(c))
    }
}

/// What the loop does once a program's frame has run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Drawing {
    /// Draws the window.
    Draw,
    /// Leaves the drawing out and waits for the next frame's time.
    Wait,
    /// Leaves the drawing out and runs the next frame at once, ahead of
    /// its time.
    RunOn,
}

/// What the loop knows once a program's frame has run, for choosing
/// whether to draw the window; see [`Scene::drawing`].
#[derive(Clone, Copy, Debug, Default)]
pub struct Scene {
    /// The window shows something that every frame is drawn for: no
    /// program runs or it is paused, one of the emulator's own screens or
    /// dialogs, the editor, the debugger, a subprogram, a window being
    /// resized, the frame after any of these.
    pub must: bool,
    /// Frames in a row left undrawn so far.
    pub undrawn: u32,
    /// The program is part-way through its main loop, out of time: the
    /// screens show a scene half made.
    pub mid_loop: bool,
    /// The program waits at its VSYNC, and a trip round its main loop
    /// fits a frame by itself.
    pub fits: bool,
    /// Presents wait for the vsync of a display at about 60 Hz.
    pub locked: bool,
    /// The time left of the frame that has run, and how long ago it
    /// ended if it has.
    pub left: Duration,
    pub late: Duration,
    /// The time to the display's next vsync, as [`to_vsync`] gives it.
    pub to_vsync: Duration,
    /// What the drawing is taken to cost.
    pub draw: Duration,
    /// How long the program's next trip is taken to run.
    pub run: Duration,
}

impl Scene {
    /// Whether to draw the window now. It is drawn whenever the drawing
    /// ends within the frame, so a computer that keeps 60 frames a second
    /// draws every one. Otherwise:
    ///
    /// - a scene half made waits for the program to get to its VSYNC, as
    ///   the drawing would only make the frame late;
    /// - when the program's trip fits a frame by itself, the drawing is
    ///   left out if it would make the next trip end after its frame's
    ///   time, so that the program gets a trip every frame and the window
    ///   is drawn in every second to fifth frame;
    /// - with presents that wait for vsync, a drawing that ends after a
    ///   vsync comes back a whole frame later. The next frame then runs
    ///   at once, so that its drawing has the time this one left.
    /// - a whole frame behind, as after one slow drawing, the next frames
    ///   run at once if that catches up.
    ///
    /// At most [`MOST_UNDRAWN`] frames in a row go undrawn.
    pub fn drawing(&self) -> Drawing {
        if self.draw <= self.left || self.must || self.undrawn >= MOST_UNDRAWN {
            return Drawing::Draw;
        }
        if self.mid_loop {
            return Drawing::Wait;
        }
        if !self.fits {
            return Drawing::Draw;
        }
        // Frames run in a row until one must be drawn, and that drawing,
        // take less time than the frames are worth less one.
        let frames = MOST_UNDRAWN + 1;
        if self.late >= FRAME && frames * self.run + self.draw <= (frames - 1) * FRAME {
            return Drawing::RunOn;
        }
        if self.locked {
            return match self.draw <= self.to_vsync {
                true => Drawing::Draw,
                false => Drawing::RunOn,
            };
        }
        if self.draw + self.run <= self.left + FRAME {
            Drawing::Draw
        } else {
            Drawing::Wait
        }
    }
}

/// The time from `now` to the next vsync of a display at 60 Hz, one of
/// whose vsyncs a present came back at `last`. The clock is no guide to
/// it: frames that are behind move away from the vsyncs.
pub fn to_vsync(last: Instant, now: Instant) -> Duration {
    let frame = FRAME.as_nanos();
    let past = now.saturating_duration_since(last).as_nanos() % frame;
    Duration::from_nanos((frame - past) as u64)
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

    /// The time left at `now` of the frame being run.
    pub fn left(&self, now: Instant) -> Duration {
        (self.next + FRAME).saturating_duration_since(now)
    }

    /// How long before `now` the frame being run ended.
    pub fn late(&self, now: Instant) -> Duration {
        now.saturating_duration_since(self.next + FRAME)
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

/// What the program is at once its frame has run, for a drawing that
/// would end after the frame's time; see [`Scene`].
#[derive(Clone, Copy, Debug, Default)]
pub struct Late {
    /// The drawing is wanted whatever it costs: the window is not shown,
    /// so nothing is presented anyway, or the bench saves this frame.
    pub wanted: bool,
    /// As [`Scene::mid_loop`] and [`Scene::fits`].
    pub mid_loop: bool,
    pub fits: bool,
    /// How long the program's next trip is taken to run.
    pub run: Duration,
}

/// A trip round the loop that drew the window, for [`Pacer::drew`].
#[derive(Clone, Copy, Debug)]
pub struct Drew {
    /// When the trip began, and how long it took up to now.
    pub trip_began: Instant,
    pub trip: Duration,
    /// How long the program's frames ran, and when they had run.
    pub ran: Duration,
    pub drawn_from: Instant,
    /// When the main window's present began and how long it took.
    pub present_began: Instant,
    pub present_took: Duration,
    /// The present was made and waited for vsync.
    pub waited: bool,
    /// Vsync is still on after it: the present was not held back.
    pub vsync: bool,
}

/// The loop's pacing: the frames' clock, what a drawing and the rest of a
/// trip cost, and the drawings left out. A trip round the loop asks it for
/// [`Pacer::frame_time`], runs the program's frame, asks
/// [`Pacer::drawing`] whether to draw the window, and ends with
/// [`Pacer::drew`] and [`Pacer::end`], or with [`Pacer::skipped`] if it
/// did not draw.
pub struct Pacer {
    clock: Clock,
    /// The time the frames give the program, sized to what the drawing
    /// leaves.
    run_time: RunTime,
    draw_time: DrawTime,
    /// Frames in a row left undrawn.
    undrawn: u32,
    /// The last trip showed something that every frame is drawn for.
    had_to_draw: bool,
    /// Whether the last present waited for the vsync of a display at
    /// 60 Hz, and when such a present last came back.
    was_locked: bool,
    vsync_at: Instant,
    /// Whether the display shows about 60 frames a second.
    at_60_hz: bool,
}

impl Pacer {
    pub fn new(now: Instant) -> Pacer {
        Pacer {
            clock: Clock::new(now),
            run_time: RunTime::default(),
            draw_time: DrawTime::default(),
            undrawn: 0,
            had_to_draw: true,
            was_locked: false,
            vsync_at: now,
            at_60_hz: false,
        }
    }

    /// Starts the frames' clock again at `now`, as after the renderer was
    /// replaced.
    pub fn restart(&mut self, now: Instant) {
        self.clock = Clock::new(now);
    }

    /// The frames' clock, for the standby thread to run frames by.
    pub fn clock(&mut self) -> &mut Clock {
        &mut self.clock
    }

    /// Notes whether the display refreshes at about 60 Hz.
    pub fn display(&mut self, at_60_hz: bool) {
        self.at_60_hz = at_60_hz;
    }

    /// What the next drawing is taken to cost.
    pub fn draw_cost(&self) -> Duration {
        self.draw_time.cost()
    }

    /// The time a frame gives a program that does not wait for the next
    /// one. A program whose trip round its main loop `fits` a frame gets
    /// the most, whatever the drawing takes: the drawing is what is then
    /// left out.
    pub fn frame_time(&self, fits: bool) -> Duration {
        match fits {
            true => MOST_RUN_TIME,
            false => self.run_time.limit(),
        }
    }

    /// Whether the frame after the one being run is due, for the loop to
    /// run it at once.
    pub fn behind(&self, now: Instant) -> bool {
        self.clock.behind(now)
    }

    /// Counts a frame run to catch up, which went undrawn.
    pub fn caught_up(&mut self) {
        self.clock.caught_up();
        self.undrawn += 1;
    }

    /// Counts `frames` that the standby thread ran, which went undrawn.
    pub fn kept(&mut self, frames: u32) {
        self.undrawn += frames;
    }

    /// Whether to draw the window, at `now`, when the program's frames
    /// have run. What the window shows besides the program's screens
    /// `must` be drawn every frame, and so is the frame after it went. A
    /// drawing that would end after the frame's time is left out if the
    /// program is still making the scene, or if it would cost the program
    /// its next frame; `late` is asked for then. See [`Scene::drawing`].
    pub fn drawing(&mut self, now: Instant, must: bool, late: impl FnOnce() -> Late) -> Drawing {
        let (left, cost) = (self.clock.left(now), self.draw_time.cost());
        let drawing = if must || self.had_to_draw || cost <= left {
            // A drawing that ends in the frame needs no more looking into.
            Drawing::Draw
        } else {
            let late = late();
            let scene = Scene {
                must: late.wanted,
                undrawn: self.undrawn,
                mid_loop: late.mid_loop,
                fits: late.fits,
                locked: self.was_locked,
                left,
                late: self.clock.late(now),
                to_vsync: to_vsync(self.vsync_at, now),
                draw: cost,
                run: late.run,
            };
            scene.drawing()
        };
        self.had_to_draw = must;
        match drawing {
            Drawing::Draw => self.undrawn = 0,
            Drawing::Wait | Drawing::RunOn => self.undrawn += 1,
        }
        drawing
    }

    /// Ends at `now` a trip that left the drawing out. Gives how long to
    /// wait for the next frame.
    pub fn skipped(&mut self, now: Instant) -> Duration {
        self.clock.end(now, false)
    }

    /// Notes a trip that drew the window: what the drawing and the rest
    /// of the trip took, and whether the frames follow the display.
    pub fn drew(&mut self, drew: Drew) {
        // The frames follow the vsync of a display at about 60 Hz, when
        // the present waited for it and was not so slow that vsync was
        // dropped; otherwise they go by the clock.
        self.was_locked = drew.waited && drew.vsync && self.at_60_hz;
        if self.was_locked {
            self.vsync_at = drew.present_began + drew.present_took;
        }
        // The drawing began when the program's frames had run. With a
        // present that waited for the vsync of a display at 60 Hz it ends
        // where the present began: the wait is no cost, as the frame ends
        // there anyway. At another rate the wait holds the loop up.
        let in_step = drew.waited && self.at_60_hz;
        let drawn_to = match in_step {
            true => drew.present_began,
            false => drew.trip_began + drew.trip,
        };
        let took = drawn_to.saturating_duration_since(drew.drawn_from);
        self.draw_time.drew(took, in_step);
        // A present that waits for vsync waits for as long as the drawing
        // left, which is not part of the drawing's time; on a display at
        // another rate it can hold the loop up for one of the display's
        // frames besides.
        let rest = drew.trip.saturating_sub(drew.ran);
        self.run_time.drew(match (drew.waited, self.at_60_hz) {
            (false, _) => Some(rest),
            (true, true) => Some(rest.saturating_sub(drew.present_took)),
            (true, false) => None,
        });
    }

    /// Ends at `now` a trip that drew the window, after [`Pacer::drew`].
    /// Gives how long to wait for the next frame.
    pub fn end(&mut self, now: Instant) -> Duration {
        self.clock.end(now, self.was_locked)
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

    /// Whether the window's size changed in the last second or so.
    pub fn excused(&self) -> bool {
        self.excused > 0
    }

    /// The window's size has changed.
    pub fn resized(&mut self) {
        self.excused = RESIZE_GRACE;
    }

    /// Follows View > "Wait for vsync": off, presents never wait; on,
    /// they wait as they did from the start, until the window is held
    /// back.
    pub fn want(&mut self, canvas: &Canvas<Window>, wanted: bool) {
        if wanted != self.wanted {
            self.wanted = wanted;
            self.set_vsync(canvas, wanted);
        }
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

    /// A scene of a program that fits its frame, `left` ms before the
    /// frame's end and the vsync, with a drawing of `draw` ms and trips of
    /// `run` ms.
    fn scene(left: f64, draw: f64, run: f64) -> Scene {
        Scene {
            fits: true,
            left: ms(left),
            to_vsync: ms(left),
            draw: ms(draw),
            run: ms(run),
            ..Scene::default()
        }
    }

    #[test]
    fn a_drawing_that_ends_in_the_frame_is_drawn() {
        for locked in [false, true] {
            for fits in [false, true] {
                for mid_loop in [false, true] {
                    let scene = Scene {
                        locked,
                        fits,
                        mid_loop,
                        ..scene(6.0, 5.9, 14.0)
                    };
                    assert_eq!(scene.drawing(), Drawing::Draw, "{scene:?}");
                }
            }
        }
    }

    #[test]
    fn a_late_drawing_is_left_out_for_a_program_that_fits() {
        // 11 ms of program and 7 ms of drawing: the next trip still ends
        // in its frame after one drawing, and not after a few.
        assert_eq!(scene(5.7, 7.0, 11.0).drawing(), Drawing::Draw);
        assert_eq!(scene(0.2, 7.0, 11.0).drawing(), Drawing::Wait);
        // With vsync the drawing would come back a frame late.
        let locked = Scene {
            locked: true,
            ..scene(5.7, 7.0, 11.0)
        };
        assert_eq!(locked.drawing(), Drawing::RunOn);
        // A late frame, a drawing that ends before the next vsync costs
        // no more than the wait for it.
        let behind = Scene {
            left: Duration::ZERO,
            ..locked
        };
        assert_eq!(behind.drawing(), Drawing::RunOn);
        let behind = Scene {
            to_vsync: ms(7.5),
            ..behind
        };
        assert_eq!(behind.drawing(), Drawing::Draw);
    }

    #[test]
    fn a_whole_frame_behind_frames_run_in_a_row_if_that_catches_up() {
        for locked in [false, true] {
            let behind = Scene {
                locked,
                left: Duration::ZERO,
                late: ms(17.0),
                ..scene(0.0, 4.6, 9.7)
            };
            assert_eq!(behind.drawing(), Drawing::RunOn);
            // Less than a frame behind, drawing goes on.
            let late = Scene {
                late: ms(5.0),
                to_vsync: ms(11.0),
                ..behind
            };
            assert_eq!(late.drawing(), Drawing::Draw);
        }
        // Five trips and a drawing that take more than four frames gain
        // nothing on a display that waits for vsync.
        let heavy = Scene {
            locked: true,
            left: Duration::ZERO,
            late: ms(17.0),
            to_vsync: ms(10.0),
            ..scene(0.0, 6.8, 13.2)
        };
        assert_eq!(heavy.drawing(), Drawing::Draw);
    }

    #[test]
    fn a_program_that_does_not_fit_is_drawn_unless_mid_loop() {
        let slow = Scene {
            fits: false,
            ..scene(1.0, 7.0, 11.0)
        };
        assert_eq!(slow.drawing(), Drawing::Draw);
        for locked in [false, true] {
            let half_made = Scene {
                mid_loop: true,
                locked,
                ..slow
            };
            assert_eq!(half_made.drawing(), Drawing::Wait);
        }
    }

    #[test]
    fn what_must_be_drawn_is_drawn_however_late() {
        let cases = [
            (true, false, false),
            (true, false, true),
            (false, true, false),
        ];
        for (fits, mid_loop, locked) in cases {
            let late = Scene {
                fits,
                mid_loop,
                locked,
                ..scene(0.0, 30.0, 14.0)
            };
            assert_ne!(late.drawing(), Drawing::Draw);
            let must = Scene { must: true, ..late };
            assert_eq!(must.drawing(), Drawing::Draw);
            let long_undrawn = Scene {
                undrawn: MOST_UNDRAWN,
                ..late
            };
            assert_eq!(long_undrawn.drawing(), Drawing::Draw);
        }
    }

    #[test]
    fn the_next_vsync_is_frames_after_the_last() {
        let last = Instant::now();
        for (at, want) in [(0.0, 16.667), (1.0, 15.667), (16.0, 0.667), (40.0, 10.0)] {
            let got = to_vsync(last, last + ms(at)).as_secs_f64() * 1000.0;
            assert!((got - want).abs() < 0.001, "{at}: {got}");
        }
    }

    /// What [`play`] gives: frames run and drawn, how late the latest
    /// frame and the last one ended in ms after their frames' time, and
    /// the most frames in a row that went undrawn.
    struct Played {
        ran: u32,
        drawn: u32,
        latest: f64,
        last: f64,
        gap: u32,
    }

    /// Runs the loop's frames through a [`Pacer`], as the window does, for a
    /// program that fits its frame, whose
    /// trips take `run` and a drawing that takes `draw` (the tenth one
    /// `slow` instead, if given), with presents that come back at the next
    /// sixtieth of a second if `vsync`, for `secs` seconds; the first
    /// second is left out of what it gives.
    fn play(run: f64, draw: f64, slow: Option<f64>, vsync: bool, secs: u64) -> Played {
        let (run, draw) = (ms(run), ms(draw));
        let start = Instant::now();
        let (mut now, mut pacer) = (start, Pacer::new(start));
        pacer.display(vsync);
        let mut played = Played {
            ran: 0,
            drawn: 0,
            latest: 0.0,
            last: 0.0,
            gap: 0,
        };
        let mut drawings = 0;
        let counts = |now: Instant| now - start >= Duration::from_secs(1);
        while now - start < Duration::from_secs(secs + 1) {
            let trip_began = now;
            now += run;
            if counts(now) {
                played.ran += 1;
                let late = now.saturating_duration_since(pacer.clock.due());
                played.last = late.as_secs_f64() * 1000.0;
                played.latest = played.latest.max(played.last);
            }
            let (drawn_from, undrawn) = (now, pacer.undrawn);
            let late = || Late {
                fits: true,
                run,
                ..Late::default()
            };
            match pacer.drawing(now, false, late) {
                Drawing::Draw => {
                    drawings += 1;
                    now += slow.filter(|_| drawings == 10).map_or(draw, ms);
                    let present_began = now;
                    if vsync {
                        let frames = (now - start).as_nanos() / FRAME.as_nanos() + 1;
                        now = start + FRAME * frames as u32;
                    }
                    if counts(now) {
                        played.drawn += 1;
                        played.gap = played.gap.max(undrawn);
                    }
                    pacer.drew(Drew {
                        trip_began,
                        trip: now - trip_began,
                        ran: run,
                        drawn_from,
                        present_began,
                        present_took: now - present_began,
                        waited: vsync,
                        vsync,
                    });
                    now += pacer.end(now);
                }
                Drawing::Wait => now += pacer.skipped(now),
                Drawing::RunOn => {
                    pacer.skipped(now);
                }
            }
        }
        played
    }

    #[test]
    fn a_program_that_fits_keeps_60_frames_a_second_whatever_the_drawing() {
        // The two programs measured on a handheld, and tighter ones; with
        // one drawing that takes 40 ms, and without.
        let cases = [
            (9.7, 4.6),
            (9.7, 9.0),
            (13.2, 6.8),
            (11.0, 7.0),
            (14.0, 8.0),
        ];
        for (run, draw) in cases {
            for vsync in [false, true] {
                for slow in [None, Some(40.0)] {
                    let played = play(run, draw, slow, vsync, 60);
                    let Played {
                        ran,
                        drawn,
                        latest,
                        last,
                        gap,
                    } = played;
                    let what = format!(
                        "{run} + {draw} ms, vsync {vsync}, slow {slow:?}: {ran} run, {drawn} \
                         drawn, {latest} ms late, {gap} undrawn in a row"
                    );
                    assert!((3597..=3603).contains(&ran), "{what}");
                    assert!(gap <= MOST_UNDRAWN, "{what}");
                    // A frame ends in its own frame's time, or nearly.
                    // The first drawings, whose cost is not known yet,
                    // miss their vsyncs and leave the frames up to two
                    // behind, which the clock makes up 50 µs a frame;
                    // and a slow drawing makes the frames after it late
                    // by what it took.
                    let most = match (slow, vsync) {
                        (Some(_), _) => 70.0,
                        (None, true) => 34.0,
                        (None, false) => 3.0,
                    };
                    assert!(latest < most, "{what}");
                    // What a slow drawing lost is made up where frames
                    // run in a row can, and never grows.
                    let light = 5.0 * run + draw <= 66.0;
                    let most = if light || !vsync { 3.0 } else { 34.0 };
                    assert!(last < most, "{what}, at last {last} ms late");
                }
            }
        }
    }

    #[test]
    fn a_drawing_that_fits_is_drawn_every_frame() {
        for vsync in [false, true] {
            let played = play(9.0, 4.0, None, vsync, 10);
            assert_eq!(played.ran, played.drawn, "vsync {vsync}");
            assert_eq!(played.latest, 0.0);
        }
    }

    #[test]
    fn a_drawing_is_left_out_as_seldom_as_keeps_the_frames() {
        // Two trips and a drawing fit two frames: every second frame is
        // drawn; three and a drawing fit three: every third.
        let drawn = play(11.0, 7.0, None, true, 60).drawn;
        assert!((1790..=1810).contains(&drawn), "{drawn}");
        let drawn = play(13.2, 6.8, None, true, 60).drawn;
        assert!((1190..=1210).contains(&drawn), "{drawn}");
        // Without the wait for vsync, more are drawn.
        let drawn = play(11.0, 7.0, None, false, 60).drawn;
        assert!(drawn > 2400, "{drawn}");
    }

    #[test]
    fn one_slow_drawing_is_not_taken_for_the_next_ones_cost() {
        let mut cost = DrawTime::default();
        assert_eq!(cost.cost(), Duration::ZERO);
        for _ in 0..3 {
            cost.drew(ms(4.0), false);
        }
        assert_eq!(cost.cost(), ms(4.0));
        cost.drew(Duration::from_millis(300), false);
        assert_eq!(cost.cost(), ms(4.0));
        // A scene that costs more from now on counts after two drawings.
        cost.drew(ms(9.0), false);
        assert_eq!(cost.cost(), ms(9.0));
        cost.drew(ms(8.0), false);
        cost.drew(ms(8.0), false);
        assert_eq!(cost.cost(), ms(8.0));
        // A present that waited leaves its time out, less a margin.
        let mut cost = DrawTime::default();
        for _ in 0..3 {
            cost.drew(ms(3.0), true);
        }
        assert_eq!(cost.cost(), ms(3.0) + RUN_MARGIN);
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
