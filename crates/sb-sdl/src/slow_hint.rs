//! A notice for a program that runs slower than real time. By default an
//! iteration of a program's main loop that runs out of time keeps its frame
//! (`Machine::keep_frames`), so a program too slow for the computer gets
//! fewer than 60 frames per second. When that lasts, the notice says where
//! "Skip frames instead of slowing down" is, once per program per session.
//!
//! [`Detector`] decides from what each iteration of the window's loop saw,
//! with no SDL code, so that tests can feed it made-up iterations.

use std::collections::HashSet;
use std::time::{Duration, Instant};

use sb_runtime::Framebuffer;
use sb_runtime::font::Font;
use sb_runtime::settings::Settings;

use crate::corner::{self, SHOW_FRAMES};
use crate::ui::{CHAR, DISABLED, PAD, Rect, TEXT, draw_text};

/// The iterations of a span, about half a second, are summed up before
/// they are judged: 30 frames at full speed, so that one frame is 2 frames
/// per second.
const SPAN: Duration = Duration::from_millis(500);
/// A span slower than this, 10% below 60, counts as slow. At that speed
/// the slowdown is easy to see and the sound has gaps.
const SLOW_FPS: u128 = 54;
/// A span at least this fast ends the slow state, so that a program at
/// about 54 frames per second does not go in and out of it.
const FAST_FPS: u128 = 57;
/// Slow spans in a row before the program counts as slow: 2 seconds, much
/// longer than a heavy moment such as a burst of sprites.
const SLOW_SPANS: u32 = 4;
/// Fast spans in a row that end the slow state: 1 second.
const FAST_SPANS: u32 = 2;
/// The time after the program starts that is not counted, to allow for
/// setup and loading.
const GRACE: Duration = Duration::from_secs(3);
/// An iteration this long means that the window's loop stalled, for
/// example because the window is dragged, the system suspended the
/// emulator, or a file loaded in one go. Skipping frames would not help,
/// so the span starts again.
const STALL: Duration = Duration::from_millis(100);
const TITLE: &str = "Running slower than real time";
const DESKTOP_LINES: [&str; 2] = [
    "To keep time, turn on \"Skip frames instead of slowing down\"",
    "in File > Settings > General.",
];
/// Each line fits a 320-pixel UI, the Simple UI's Large size on 640x480.
const SIMPLE_UI_LINES: [&str; 2] = [
    "To keep time, turn on Frame skip",
    "in the menu (Select+A).",
];
const FOOTER: &str = "Frame-counting games may misbehave.";

/// An iteration of the window's loop, as the detector sees it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Iteration {
    /// The time since the previous iteration began.
    pub elapsed: Duration,
    /// Program frames that started in it: how far `MAINCNT` went.
    pub started: u32,
    /// Calls of `Machine::frame` in it that started no frame, because the
    /// iteration of the program's main loop under way kept its frame.
    pub kept: u32,
    /// Whether the iteration counts: a program runs, it is not paused or in
    /// a dialog, the window is shown, the speed is unlimited, frames are
    /// kept and the notice is wanted.
    pub counts: bool,
}

/// Tells when a program has run slower than real time for a while: its
/// frames per second are below [`SLOW_FPS`] for [`SLOW_SPANS`] spans in a
/// row, with frames kept in each, after the [`GRACE`] that follows its
/// start. An iteration that does not count, or a [`STALL`], starts the span
/// and the run of slow spans again. Once slow, it stays slow until
/// [`FAST_SPANS`] spans run at [`FAST_FPS`] or more.
#[derive(Clone, Copy, Debug, Default)]
pub struct Detector {
    /// Time counted since the program started, up to [`GRACE`].
    warmed: Duration,
    /// The span so far.
    span: Duration,
    started: u32,
    kept: u32,
    slow_spans: u32,
    fast_spans: u32,
    slow: bool,
}

impl Detector {
    /// Forgets everything, as for a program that starts.
    pub fn reset(&mut self) {
        *self = Detector::default();
    }

    /// Notes an iteration. Returns true if the program has just come to
    /// count as slow.
    pub fn iteration(&mut self, iteration: Iteration) -> bool {
        if !iteration.counts || iteration.elapsed >= STALL {
            (self.span, self.started, self.kept) = (Duration::ZERO, 0, 0);
            self.slow_spans = 0;
            return false;
        }
        if self.warmed < GRACE {
            self.warmed += iteration.elapsed;
            return false;
        }
        self.span += iteration.elapsed;
        self.started += iteration.started;
        self.kept += iteration.kept;
        if self.span < SPAN {
            return false;
        }
        let frames = u128::from(self.started) * 1_000_000_000;
        let fps_below = |fps: u128| frames < fps * self.span.as_nanos();
        let (slow, fast) = (fps_below(SLOW_FPS) && self.kept > 0, !fps_below(FAST_FPS));
        (self.span, self.started, self.kept) = (Duration::ZERO, 0, 0);
        self.slow_spans = if slow { self.slow_spans + 1 } else { 0 };
        self.fast_spans = if fast { self.fast_spans + 1 } else { 0 };
        if self.slow && self.fast_spans >= FAST_SPANS {
            self.slow = false;
        }
        let became = !self.slow && self.slow_spans >= SLOW_SPANS;
        self.slow |= became;
        became
    }

    #[cfg(test)]
    fn is_slow(&self) -> bool {
        self.slow
    }
}

/// The notice: its detector, and the programs it was shown for.
#[derive(Debug, Default)]
pub struct SlowHint {
    detector: Detector,
    /// When the last iteration began.
    last: Option<Instant>,
    /// The program last seen running, by its project and file.
    program: Option<(String, String)>,
    /// The programs it was shown for in this session.
    shown: HashSet<(String, String)>,
    /// Due, but waiting until the keys' picture is gone.
    pending: bool,
    frames_left: u32,
}

impl SlowHint {
    /// Notes an iteration of the loop that began at `now`. `program` is the
    /// project and file of the running program, if any. The elapsed time of
    /// `iteration` is computed here.
    pub fn iteration(
        &mut self,
        now: Instant,
        mut iteration: Iteration,
        program: Option<(&str, &str)>,
    ) {
        let elapsed = self.last.map(|last| now - last);
        self.last = Some(now);
        let seen = (self.program.as_ref()).map(|(p, f)| (p.as_str(), f.as_str()));
        if program != seen {
            self.program = program.map(|(p, f)| (p.to_string(), f.to_string()));
            self.detector.reset();
        }
        let (Some(program), Some(elapsed)) = (&self.program, elapsed) else {
            return;
        };
        iteration.elapsed = elapsed;
        if self.detector.iteration(iteration) && !self.shown.contains(program) {
            self.shown.insert(program.clone());
            self.pending = true;
        }
    }

    /// Counts down a frame. A notice that is due is shown unless `held`,
    /// for example while the keys' picture shows in the same corner. A
    /// notice shown when the picture appears waits until the picture is
    /// gone, and is then shown for its whole time again, so that the two
    /// never overlap.
    pub fn tick(&mut self, held: bool) {
        if held && self.visible() {
            (self.pending, self.frames_left) = (true, 0);
        }
        if self.pending && !held {
            (self.pending, self.frames_left) = (false, SHOW_FRAMES);
        }
        self.frames_left = self.frames_left.saturating_sub(1);
    }

    /// Removes the notice, for example when frames are now skipped.
    pub fn dismiss(&mut self) {
        (self.pending, self.frames_left) = (false, 0);
    }

    /// Removes the notice immediately when the settings no longer want it:
    /// frames are now skipped, or the notice is turned off.
    pub fn follow(&mut self, settings: &Settings) {
        if settings.skip_frames || !settings.slow_hint {
            self.dismiss();
        }
    }

    pub fn visible(&self) -> bool {
        self.frames_left > 0
    }

    /// The notice's lines: the title, the body and the footer.
    fn lines(simple_ui: bool) -> impl Iterator<Item = &'static str> {
        let body = if simple_ui {
            SIMPLE_UI_LINES
        } else {
            DESKTOP_LINES
        };
        std::iter::once(TITLE).chain(body).chain([FOOTER])
    }

    /// Returns the rectangle of the notice on a frame of `size`: the bottom
    /// right corner, the same as the keys' picture.
    fn place((fw, fh): (u32, u32), simple_ui: bool) -> Rect {
        let lines = Self::lines(simple_ui);
        let widest = lines.map(crate::ui::text_width).max().unwrap_or(0);
        corner::place((fw, fh), (widest + 2 * PAD, 4 * CHAR + 3 * PAD))
    }

    /// Draws the notice in the bottom right corner of `fb`, worded for
    /// the Simple UI or the desktop.
    pub fn draw(&self, fb: &mut Framebuffer, font: &Font, simple_ui: bool) {
        let size = Self::place((fb.width(), fb.height()), simple_ui);
        let panel = corner::panel(fb, (size.w, size.h));
        let x = panel.x + PAD;
        let mut y = panel.y + PAD;
        for (i, line) in Self::lines(simple_ui).enumerate() {
            let color = if i < 3 { TEXT } else { DISABLED };
            // A gap under the title and above the footer, the same as in
            // the keys' picture.
            if i == 1 || i == 3 {
                y += PAD / 2;
            }
            draw_text(fb, font, x, y, line, color);
            y += CHAR;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FRAME: Duration = Duration::from_nanos(1_000_000_000 / 60);

    /// An iteration one frame long that started `started` frames and kept
    /// `kept` calls.
    fn iteration(started: u32, kept: u32) -> Iteration {
        Iteration {
            elapsed: FRAME,
            started,
            kept,
            counts: true,
        }
    }

    /// Feeds `seconds` of iterations at `fps` program frames per second;
    /// the rest of the window's 60 iterations per second keep their frames.
    /// Returns whether the detector said slow at any of them.
    fn run(d: &mut Detector, seconds: f64, fps: u32, counts: bool) -> bool {
        let iterations = (seconds * 60.0).round() as u32;
        let mut became = false;
        for i in 0..iterations {
            // Spread the frames evenly over the window's iterations.
            let started = (i + 1) * fps / 60 - i * fps / 60;
            let t = Iteration {
                counts,
                ..iteration(started, 1 - started.min(1))
            };
            became |= d.iteration(t);
        }
        became
    }

    #[test]
    fn a_slow_start_up_does_not_count() {
        let mut d = Detector::default();
        assert!(!run(&mut d, 3.0, 20, true), "within the grace");
        assert!(!run(&mut d, 1.0, 60, true));
        assert!(!d.is_slow());
    }

    #[test]
    fn a_sustained_shortfall_counts_after_two_seconds() {
        let mut d = Detector::default();
        run(&mut d, 3.0, 60, true);
        assert!(!run(&mut d, 1.9, 40, true), "not yet");
        assert!(run(&mut d, 0.2, 40, true));
        assert!(d.is_slow());
        assert!(!run(&mut d, 5.0, 40, true), "said once");
    }

    #[test]
    fn a_single_spike_does_not_count() {
        let mut d = Detector::default();
        run(&mut d, 4.0, 60, true);
        // Half a second at 10 frames per second, twice, a second apart.
        assert!(!run(&mut d, 0.5, 10, true));
        assert!(!run(&mut d, 1.0, 60, true));
        assert!(!run(&mut d, 0.5, 10, true));
        assert!(!run(&mut d, 1.0, 60, true));
        assert!(!d.is_slow());
        // An iteration of four frames' time, kept: three calls in one
        // iteration.
        for _ in 0..20 {
            assert!(!d.iteration(iteration(0, 3)));
            for _ in 0..60 {
                d.iteration(iteration(1, 0));
            }
        }
        assert!(!d.is_slow());
    }

    #[test]
    fn about_54_does_not_flicker() {
        let mut d = Detector::default();
        run(&mut d, 3.0, 60, true);
        assert!(run(&mut d, 3.0, 40, true));
        // Between the two limits it stays slow, and says nothing again.
        assert!(!run(&mut d, 10.0, 55, true));
        assert!(d.is_slow());
        // Fast for a second ends it, regardless of the part of a span the
        // change falls in. Slow again for two seconds says so again.
        assert!(!run(&mut d, 1.5, 60, true));
        assert!(!d.is_slow());
        assert!(run(&mut d, 2.5, 40, true));
        // Just under 54 counts; just over does not.
        let mut d = Detector::default();
        run(&mut d, 3.0, 60, true);
        assert!(run(&mut d, 3.0, 52, true));
        let mut d = Detector::default();
        run(&mut d, 3.0, 60, true);
        assert!(!run(&mut d, 10.0, 56, true));
    }

    #[test]
    fn iterations_that_do_not_count_start_again() {
        // Paused, waiting for input, frames skipped: no iteration counts.
        let mut d = Detector::default();
        run(&mut d, 3.0, 60, true);
        assert!(!run(&mut d, 10.0, 20, false));
        // An iteration that does not count breaks a run of slow spans.
        for _ in 0..10 {
            assert!(!run(&mut d, 1.5, 20, true));
            assert!(!d.iteration(Iteration::default()));
        }
        // So does a stalled iteration, for example while the window is
        // dragged.
        for _ in 0..10 {
            assert!(!run(&mut d, 1.5, 20, true));
            let held = Iteration {
                elapsed: Duration::from_millis(500),
                ..iteration(1, 0)
            };
            assert!(!d.iteration(held));
        }
        assert!(run(&mut d, 2.5, 20, true));
    }

    #[test]
    fn slow_without_kept_frames_is_not_this() {
        // The program got fewer frames, but not because they were kept:
        // skipping frames would not help.
        let mut d = Detector::default();
        let mut slow = 0;
        for _ in 0..600 {
            slow += u32::from(d.iteration(Iteration {
                elapsed: 2 * FRAME,
                ..iteration(1, 0)
            }));
        }
        assert_eq!(slow, 0);
    }

    #[test]
    fn shown_once_per_program_a_session() {
        let mut hint = SlowHint::default();
        let mut now = Instant::now();
        let mut feed = |hint: &mut SlowHint, seconds: u32, program: Option<(&str, &str)>| {
            for i in 0..seconds * 60 {
                now += FRAME;
                let t = iteration(u32::from(i % 2 == 0), u32::from(i % 2 == 1));
                hint.iteration(now, t, program);
                hint.tick(false);
            }
        };
        feed(&mut hint, 6, Some(("GAME", "MAIN")));
        assert!(hint.visible());
        hint.dismiss();
        feed(&mut hint, 1, None);
        feed(&mut hint, 10, Some(("GAME", "MAIN")));
        assert!(!hint.visible(), "shown for this program already");
        feed(&mut hint, 6, Some(("OTHER", "MAIN")));
        assert!(hint.visible());
    }

    #[test]
    fn waits_for_the_keys_picture() {
        let mut hint = SlowHint {
            pending: true,
            ..SlowHint::default()
        };
        hint.tick(true);
        assert!(!hint.visible());
        hint.tick(false);
        assert!(hint.visible());
        for _ in 0..SHOW_FRAMES {
            hint.tick(false);
        }
        assert!(!hint.visible());
        // The picture appears while the notice is shown: the notice is
        // hidden, and comes back for its whole time once the picture is
        // gone.
        hint.pending = true;
        for _ in 0..100 {
            hint.tick(false);
        }
        hint.tick(true);
        assert!(!hint.visible());
        hint.tick(false);
        for _ in 1..SHOW_FRAMES {
            assert!(hint.visible());
            hint.tick(false);
        }
        assert!(!hint.visible());
    }

    /// Either setting removes a notice immediately, shown or due.
    #[test]
    fn the_settings_take_it_away() {
        let mut s = Settings::default();
        let shown = || SlowHint {
            frames_left: SHOW_FRAMES,
            ..SlowHint::default()
        };
        let mut hint = shown();
        hint.follow(&s);
        assert!(hint.visible(), "wanted by default");
        s.slow_hint = false;
        hint.follow(&s);
        assert!(!hint.visible());
        let mut hint = SlowHint {
            pending: true,
            ..SlowHint::default()
        };
        hint.follow(&s);
        hint.tick(false);
        assert!(!hint.visible(), "nor shown later");
        s.slow_hint = true;
        s.skip_frames = true;
        let mut hint = shown();
        hint.follow(&s);
        assert!(!hint.visible());
    }

    /// Fits the Simple UI at its Large size on 640x480, and the desktop.
    #[test]
    fn fits_the_smallest_ui() {
        for (size, simple_ui) in [((320, 240), true), ((1280, 720), false)] {
            let r = SlowHint::place(size, simple_ui);
            assert!(r.x >= PAD && r.y >= PAD, "{size:?}: {r:?}");
            assert!(r.x + r.w + PAD <= size.0 && r.y + r.h + PAD <= size.1);
        }
        let mut fb = Framebuffer::new(320, 240);
        SlowHint::default().draw(&mut fb, &Font::blank(), true);
    }
}
