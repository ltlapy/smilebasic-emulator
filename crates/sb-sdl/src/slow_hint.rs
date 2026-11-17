//! A notice for a program that runs slower than real time. By default a
//! trip round a program's main loop that runs out of time keeps its frame
//! (`Machine::keep_frames`), so a program too slow for the computer gets
//! fewer than 60 frames a second. When that lasts, the notice says where
//! "Skip frames instead of slowing down" is, once per program a session.
//!
//! [`Detector`] decides from what each trip round the window's loop saw,
//! with nothing of SDL, so that tests can feed it made-up trips.

use std::collections::HashSet;
use std::time::{Duration, Instant};

use sb_runtime::Framebuffer;
use sb_runtime::font::Font;
use sb_runtime::settings::Settings;

use crate::ui::{BACKGROUND, BORDER, CHAR, DISABLED, PAD, Rect, TEXT, draw_text, fill, outline};

/// The trips of a span, about half a second, are summed up before they
/// are judged: 30 frames at full speed, so that one frame is 2 frames a
/// second.
const SPAN: Duration = Duration::from_millis(500);
/// A span slower than this, 10% short of 60, counts as slow: there the
/// slowdown is easy to see and the sound has gaps.
const SLOW_FPS: u128 = 54;
/// A span at least this fast ends the slow state, so that a program at
/// about 54 frames a second does not go in and out of it.
const FAST_FPS: u128 = 57;
/// Slow spans in a row before the program counts as slow: 2 seconds, much
/// longer than a heavy moment such as a burst of sprites.
const SLOW_SPANS: u32 = 4;
/// Fast spans in a row that end the slow state: 1 second.
const FAST_SPANS: u32 = 2;
/// After the program starts, time that counts for nothing, for what it
/// sets up and loads.
const GRACE: Duration = Duration::from_secs(3);
/// A trip this long means the window was held up, as while it is dragged,
/// the system suspended the emulator, or a file loaded in one go: skipping
/// frames would not help, so the span starts again.
const STALL: Duration = Duration::from_millis(100);
/// About 6 seconds at 60 frames per second, as the keys' picture.
const SHOW_FRAMES: u32 = 360;

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

/// A trip round the window's loop, as the detector sees it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Trip {
    /// Since the trip before began.
    pub elapsed: Duration,
    /// Program frames that started in it: how far `MAINCNT` went.
    pub started: u32,
    /// Calls of `Machine::frame` in it that started no frame, because the
    /// program's trip under way kept its frame.
    pub kept: u32,
    /// Whether the trip can tell anything: a program runs, not paused, not
    /// in a dialog, the window shown, at unlimited speed, frames are kept
    /// and the notice is wanted.
    pub counts: bool,
}

/// Tells when a program has run slower than real time for a while: its
/// frames a second below [`SLOW_FPS`] for [`SLOW_SPANS`] spans in a row,
/// with frames kept in each, past the [`GRACE`] after it started. A trip
/// that does not count, or a [`STALL`], starts the span and the run of
/// slow spans again. Once slow, it stays so until [`FAST_SPANS`] spans
/// run at [`FAST_FPS`] or more.
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

    /// Notes a trip; true if the program has just come to count as slow.
    pub fn trip(&mut self, trip: Trip) -> bool {
        if !trip.counts || trip.elapsed >= STALL {
            (self.span, self.started, self.kept) = (Duration::ZERO, 0, 0);
            self.slow_spans = 0;
            return false;
        }
        if self.warmed < GRACE {
            self.warmed += trip.elapsed;
            return false;
        }
        self.span += trip.elapsed;
        self.started += trip.started;
        self.kept += trip.kept;
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
    /// When the last trip began.
    last: Option<Instant>,
    /// The program last seen running, by its project and file.
    program: Option<(String, String)>,
    /// The programs it was shown for in this session.
    shown: HashSet<(String, String)>,
    /// Due, but waiting for the keys' picture to go.
    pending: bool,
    frames_left: u32,
}

impl SlowHint {
    /// Notes a trip round the loop that began at `now`. `program` is the
    /// project and file of the program running, if any; `trip`'s elapsed
    /// time is worked out here.
    pub fn trip(&mut self, now: Instant, mut trip: Trip, program: Option<(&str, &str)>) {
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
        trip.elapsed = elapsed;
        if self.detector.trip(trip) && !self.shown.contains(program) {
            self.shown.insert(program.clone());
            self.pending = true;
        }
    }

    /// Counts down a frame; a notice that is due shows unless `held`, as
    /// while the keys' picture shows in the same corner. One that shows
    /// when the picture comes up waits for it to go, and then shows its
    /// whole time again, so that the two never overlap.
    pub fn tick(&mut self, held: bool) {
        if held && self.visible() {
            (self.pending, self.frames_left) = (true, 0);
        }
        if self.pending && !held {
            (self.pending, self.frames_left) = (false, SHOW_FRAMES);
        }
        self.frames_left = self.frames_left.saturating_sub(1);
    }

    /// Takes the notice away, as when frames are skipped now.
    pub fn dismiss(&mut self) {
        (self.pending, self.frames_left) = (false, 0);
    }

    /// Takes the notice away at once when the settings no longer want it:
    /// frames are skipped now, or the notice is turned off.
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

    /// Where the notice goes on a frame of `size`: the bottom right
    /// corner, as the keys' picture.
    fn place((fw, fh): (u32, u32), simple_ui: bool) -> Rect {
        let lines = Self::lines(simple_ui);
        let widest = lines.map(crate::ui::text_width).max().unwrap_or(0);
        let (w, h) = (widest + 2 * PAD, 4 * CHAR + 3 * PAD);
        Rect {
            x: fw.saturating_sub(w + PAD),
            y: fh.saturating_sub(h + PAD),
            w,
            h,
        }
    }

    /// Draws the notice in the bottom right corner of `fb`, worded for
    /// the Simple UI or the desktop.
    pub fn draw(&self, fb: &mut Framebuffer, font: &Font, simple_ui: bool) {
        let panel = Self::place((fb.width(), fb.height()), simple_ui);
        fill(fb, panel, BACKGROUND);
        outline(fb, panel, BORDER);
        let x = panel.x + PAD;
        let mut y = panel.y + PAD;
        for (i, line) in Self::lines(simple_ui).enumerate() {
            let color = if i < 3 { TEXT } else { DISABLED };
            // A gap under the title and above the footer, as the keys'
            // picture has.
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

    /// A trip of a frame's time that started `started` frames and kept
    /// `kept` calls.
    fn trip(started: u32, kept: u32) -> Trip {
        Trip {
            elapsed: FRAME,
            started,
            kept,
            counts: true,
        }
    }

    /// Feeds `seconds` of trips at `fps` program frames a second, the rest
    /// of the window's 60 trips keeping their frames; gives whether the
    /// detector said slow at any of them.
    fn run(d: &mut Detector, seconds: f64, fps: u32, counts: bool) -> bool {
        let trips = (seconds * 60.0).round() as u32;
        let mut became = false;
        for i in 0..trips {
            // Spread the frames evenly over the window's trips.
            let started = (i + 1) * fps / 60 - i * fps / 60;
            let t = Trip {
                counts,
                ..trip(started, 1 - started.min(1))
            };
            became |= d.trip(t);
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
        // Half a second at 10 frames a second, twice, a second apart.
        assert!(!run(&mut d, 0.5, 10, true));
        assert!(!run(&mut d, 1.0, 60, true));
        assert!(!run(&mut d, 0.5, 10, true));
        assert!(!run(&mut d, 1.0, 60, true));
        assert!(!d.is_slow());
        // A trip of four frames' time, kept: three calls in one trip.
        for _ in 0..20 {
            assert!(!d.trip(trip(0, 3)));
            for _ in 0..60 {
                d.trip(trip(1, 0));
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
        // Fast for a second ends it, whatever part of a span the change
        // falls in; slow again for two says so again.
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
    fn trips_that_do_not_count_start_again() {
        // Paused, waiting for input, frames skipped: no trip counts.
        let mut d = Detector::default();
        run(&mut d, 3.0, 60, true);
        assert!(!run(&mut d, 10.0, 20, false));
        // A trip that does not count breaks a run of slow spans.
        for _ in 0..10 {
            assert!(!run(&mut d, 1.5, 20, true));
            assert!(!d.trip(Trip::default()));
        }
        // So does a trip held up, as while the window is dragged.
        for _ in 0..10 {
            assert!(!run(&mut d, 1.5, 20, true));
            let held = Trip {
                elapsed: Duration::from_millis(500),
                ..trip(1, 0)
            };
            assert!(!d.trip(held));
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
            slow += u32::from(d.trip(Trip {
                elapsed: 2 * FRAME,
                ..trip(1, 0)
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
                let t = trip(u32::from(i % 2 == 0), u32::from(i % 2 == 1));
                hint.trip(now, t, program);
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
        // The picture comes up while the notice shows: the notice goes,
        // and comes back for its whole time once the picture has gone.
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

    /// Either setting takes a notice away at once, shown or due.
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
