//! The emulator's windows: the start size of the main window, the
//! sizes that View > Window size gives it, which window comes to the
//! front when a dialog on the other window keeps the input, and the
//! shape the main window is kept at. [`crate::placement`] decides
//! where things go inside a window.

use sdl3::event::Event;
use std::time::{Duration, Instant};

/// View > Window size: the label of each item, and the multiple of the
/// profile's display (1280x720 for SmileBASIC 4) that it sets the main
/// window's pixels to, in halves. Each line here is one menu item.
pub const SCALES: [(&str, u32); 6] = [
    ("0.5x", 1),
    ("1x", 2),
    ("1.5x", 3),
    ("2x", 4),
    ("2.5x", 5),
    ("3x", 6),
];

/// The main window's size in pixels at `halves` halves of `display`.
pub fn scaled(display: (u32, u32), halves: u32) -> (u32, u32) {
    (display.0 * halves / 2, display.1 * halves / 2)
}

/// The size in the system's window coordinates that holds `pixels`, at
/// `density` pixels per coordinate: 2 on a Mac's Retina display, the
/// desktop's scale on Wayland, 1 on Windows and X11, whose coordinates are
/// pixels.
pub fn coordinates((w, h): (u32, u32), density: f32) -> (u32, u32) {
    let density = if density > 0.0 { density } else { 1.0 };
    let at = |n: u32| ((n as f32 / density).round() as u32).max(1);
    (at(w), at(h))
}

/// The items of [`SCALES`] whose size fits a display's usable area
/// `usable` with the window's `borders` (top, left, bottom, right) around
/// it, as bits with the first item in the lowest bit. The others are
/// disabled rather than shrunk, because a shrunk size is not the multiple
/// asked for.
pub fn fitting(
    display: (u32, u32),
    density: f32,
    (top, left, bottom, right): (u32, u32, u32, u32),
    (uw, uh): (u32, u32),
) -> u32 {
    let fits = |&(_, halves): &(&str, u32)| {
        let (w, h) = coordinates(scaled(display, halves), density);
        w + left + right <= uw && h + top + bottom <= uh
    };
    (SCALES.iter().enumerate())
        .filter(|(_, scale)| fits(scale))
        .fold(0, |bits, (i, _)| bits | 1 << i)
}

/// The item of [`SCALES`] that the main window is at, which the menu shows
/// checked: its pixels are exactly that multiple, and it is not fullscreen.
pub fn current(display: (u32, u32), pixels: (u32, u32), fullscreen: bool) -> Option<usize> {
    let at = |&(_, halves): &(&str, u32)| scaled(display, halves) == pixels;
    SCALES.iter().position(at).filter(|_| !fullscreen)
}

/// One of the emulator's windows: the main window or the editor window.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Which {
    Main,
    Editor,
}

/// The window showing the emulator's window that takes all input, if one
/// is open (`modal`) and the editor window is shown (`apart`). This is the
/// window that had the keys when the modal window opened: the editor
/// window if `ui_editor`. With one window there is nothing to bring
/// forward.
pub fn modal_window(apart: bool, modal: bool, ui_editor: bool) -> Option<Which> {
    match (apart && modal, ui_editor) {
        (false, _) => None,
        (true, false) => Some(Which::Main),
        (true, true) => Some(Which::Editor),
    }
}

/// The window to bring to the front for `event`, from window `from`,
/// while one of the emulator's windows that take all input (a dialog,
/// the projects, the settings, the editor colors, the keyboard
/// shortcuts, the windows of Ctrl+S and Ctrl+L) shows on `modal_on`. A
/// press, a click, the wheel or typing in the other window brings that
/// window forward and goes no further. Otherwise it would reach the window
/// the user cannot see, or nothing, which looks like a hang. Pointer
/// motion and releases are not blocked.
pub fn blocked_by(event: &Event, modal_on: Option<Which>, from: Which) -> Option<Which> {
    let input = matches!(
        event,
        Event::KeyDown { .. }
            | Event::TextInput { .. }
            | Event::TextEditing { .. }
            | Event::MouseButtonDown { .. }
            | Event::MouseWheel { .. }
    );
    modal_on.filter(|&on| input && on != from)
}

/// The start size of the main window, in the system's window
/// coordinates, when it asks for `w` x `h` on a display with the usable
/// area `usable`: no larger than that area in either dimension. On most
/// systems a window larger than the display shows only in part. On
/// Linux's KMS/DRM, the console without a desktop, SDL draws into a buffer
/// of the display's size while the window keeps the size it asked for, so
/// the renderer would lay out its frames for the wrong size.
pub fn start_size((w, h): (u32, u32), usable: Option<(u32, u32)>) -> (u32, u32) {
    match usable {
        Some((uw, uh)) if uw > 0 && uh > 0 => (w.min(uw), h.min(uh)),
        _ => (w, h),
    }
}

/// The size nearest to `(w, h)` with the shape of `display` (16:9 for
/// SmileBASIC 4) that fits `room`, the display's usable area minus the
/// window's borders, if known. `None` when the size already has that
/// shape to within a pixel. The width stays and the height follows it,
/// unless the result does not fit the room. Then the height stays and the
/// width follows it. When neither fits, the result is the largest size of
/// the shape that fits.
pub fn snap_to_shape(
    (w, h): (u32, u32),
    display: (u32, u32),
    room: Option<(u32, u32)>,
) -> Option<(u32, u32)> {
    let (dw, dh) = (u64::from(display.0.max(1)), u64::from(display.1.max(1)));
    let (w64, h64) = (u64::from(w), u64::from(h));
    let within_a_pixel = (h64 * dw).abs_diff(w64 * dh) <= dw;
    if within_a_pixel || w == 0 || h == 0 {
        return None;
    }
    let along = |v: u64, from: u64, to: u64| ((v * to + from / 2) / from).max(1) as u32;
    let (rw, rh) = room
        .filter(|&(rw, rh)| rw > 0 && rh > 0)
        .unwrap_or((u32::MAX, u32::MAX));
    let tall = (w, along(w64, dw, dh));
    let wide = (along(h64, dh, dw), h);
    let fits = |(sw, sh): (u32, u32)| sw <= rw && sh <= rh;
    let size = if fits(tall) {
        tall
    } else if fits(wide) {
        wide
    } else {
        let by_width = (rw, along(u64::from(rw), dw, dh));
        match fits(by_width) {
            true => by_width,
            false => (along(u64::from(rh), dh, dw), rh),
        }
    };
    (size != (w, h)).then_some(size)
}

/// The time the main window's size must stay unchanged before View >
/// "Keep 16:9 when resizing" corrects it. A drag sends sizes more often
/// than this.
pub const SETTLE: Duration = Duration::from_millis(300);

/// The number of sizes in a row the system may refuse before the window's
/// shape is left unchanged until the item is turned on again.
const MOST_REFUSED_SIZES: u32 = 2;

/// Puts the main window back to the display's shape after the user
/// resizes it, on systems that ignore the shape the window asks for (some
/// Wayland compositors do). It acts once the size has settled, never
/// during a drag, and never in a loop with a system that keeps refusing.
#[derive(Debug, Default)]
pub struct KeepShape {
    /// When the window was last resized, until its size has settled.
    resized: Option<Instant>,
    /// The size last asked for, until the window's size settles after it.
    asked: Option<(u32, u32)>,
    /// The sizes asked for in a row that the system did not give.
    refused_in_a_row: u32,
}

impl KeepShape {
    /// The window's size changed, by the user, the system or a size
    /// asked for.
    pub fn resized(&mut self, now: Instant) {
        self.resized = Some(now);
    }

    /// The item was turned on, or the window must be corrected now:
    /// resets and returns the size to ask for, if any. `free` means a
    /// window that the shape is not kept for: fullscreen or maximized.
    pub fn now(
        &mut self,
        size: (u32, u32),
        free: bool,
        display: (u32, u32),
        room: Option<(u32, u32)>,
    ) -> Option<(u32, u32)> {
        *self = KeepShape::default();
        self.ask(size, free, display, room)
    }

    /// The size to ask for now, once the window's size has settled after
    /// a change: see [`KeepShape::now`].
    pub fn tick(
        &mut self,
        now: Instant,
        size: (u32, u32),
        free: bool,
        display: (u32, u32),
        room: Option<(u32, u32)>,
    ) -> Option<(u32, u32)> {
        let since = self.resized?;
        if now.duration_since(since) < SETTLE {
            return None;
        }
        self.resized = None;
        let wanted = snap_to_shape(size, display, room);
        // The size that settled after a size was asked for: given or
        // refused.
        if self.asked.take().is_some() {
            match wanted {
                None => self.refused_in_a_row = 0,
                Some(_) => self.refused_in_a_row += 1,
            }
        }
        self.ask(size, free, display, room)
    }

    fn ask(
        &mut self,
        size: (u32, u32),
        free: bool,
        display: (u32, u32),
        room: Option<(u32, u32)>,
    ) -> Option<(u32, u32)> {
        if free || self.refused_in_a_row >= MOST_REFUSED_SIZES {
            return None;
        }
        self.asked = snap_to_shape(size, display, room);
        self.asked
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sdl3::keyboard::{Keycode, Mod, Scancode};
    use sdl3::mouse::{MouseButton, MouseWheelDirection};

    fn key(window_id: u32) -> Event {
        Event::KeyDown {
            timestamp: 0,
            window_id,
            keycode: Some(Keycode::Escape),
            scancode: Some(Scancode::Escape),
            keymod: Mod::NOMOD,
            repeat: false,
            which: 0,
            raw: 0,
        }
    }

    fn inputs() -> Vec<Event> {
        let (timestamp, window_id, which) = (0, 1, 0);
        vec![
            key(window_id),
            Event::TextInput {
                timestamp,
                window_id,
                text: "a".to_string(),
            },
            Event::MouseButtonDown {
                timestamp,
                window_id,
                which,
                mouse_btn: MouseButton::Left,
                clicks: 1,
                x: 10.0,
                y: 10.0,
            },
            Event::MouseWheel {
                timestamp,
                window_id,
                which,
                x: 0.0,
                y: 1.0,
                direction: MouseWheelDirection::Normal,
                integer_x: 0,
                integer_y: 1,
                mouse_x: 10.0,
                mouse_y: 10.0,
            },
        ]
    }

    #[test]
    fn input_in_the_other_window_brings_the_dialogs_forward() {
        use Which::{Editor, Main};
        for event in inputs() {
            // The dialog on the main window, input in the editor window.
            assert_eq!(blocked_by(&event, Some(Main), Editor), Some(Main));
            // And the reverse.
            assert_eq!(blocked_by(&event, Some(Editor), Main), Some(Editor));
            // In the dialog's own window, it goes to the dialog.
            assert_eq!(blocked_by(&event, Some(Main), Main), None);
            assert_eq!(blocked_by(&event, Some(Editor), Editor), None);
            // With no dialog, or no editor window, nothing is blocked.
            assert_eq!(blocked_by(&event, None, Editor), None);
            assert_eq!(blocked_by(&event, None, Main), None);
        }
    }

    #[test]
    fn the_dialog_is_on_the_window_that_had_the_keys() {
        assert_eq!(modal_window(true, true, false), Some(Which::Main));
        assert_eq!(modal_window(true, true, true), Some(Which::Editor));
        for ui_editor in [false, true] {
            assert_eq!(modal_window(false, true, ui_editor), None);
            assert_eq!(modal_window(true, false, ui_editor), None);
        }
        // A press in the editor window, with the settings on the main
        // window, brings the main window forward.
        let modal_on = modal_window(true, true, false);
        assert_eq!(
            blocked_by(&key(2), modal_on, Which::Editor),
            Some(Which::Main)
        );
    }

    #[test]
    fn pointer_motion_and_window_events_bring_nothing_forward() {
        let (timestamp, window_id, which) = (0, 1, 0);
        let left_alone = [
            Event::MouseMotion {
                timestamp,
                window_id,
                which,
                mousestate: sdl3::mouse::MouseState::from_sdl_state(0),
                x: 1.0,
                y: 1.0,
                xrel: 1.0,
                yrel: 1.0,
            },
            Event::MouseButtonUp {
                timestamp,
                window_id,
                which,
                mouse_btn: MouseButton::Left,
                clicks: 1,
                x: 10.0,
                y: 10.0,
            },
            Event::Window {
                timestamp,
                window_id,
                win_event: sdl3::event::WindowEvent::FocusGained,
            },
            Event::Quit { timestamp },
        ];
        for event in left_alone {
            assert_eq!(blocked_by(&event, Some(Which::Main), Which::Editor), None);
        }
    }

    #[test]
    fn window_sizes_are_multiples_of_the_display() {
        let sb4 = (1280, 720);
        let sizes: Vec<_> = SCALES.iter().map(|&(_, h)| scaled(sb4, h)).collect();
        assert_eq!(
            sizes,
            [
                (640, 360),
                (1280, 720),
                (1920, 1080),
                (2560, 1440),
                (3200, 1800),
                (3840, 2160)
            ]
        );
        // Each label names its multiple.
        for (label, halves) in SCALES {
            let n: f32 = label.trim_end_matches('x').parse().unwrap();
            assert_eq!(n * 2.0, halves as f32, "{label}");
        }
        // In window coordinates: on a Retina display, half the pixels.
        assert_eq!(coordinates((1280, 720), 1.0), (1280, 720));
        assert_eq!(coordinates((1280, 720), 2.0), (640, 360));
        assert_eq!(coordinates((1920, 1080), 1.5), (1280, 720));
        assert_eq!(coordinates((1280, 720), 0.0), (1280, 720));
    }

    #[test]
    fn sizes_past_the_display_are_left_out() {
        let sb4 = (1280, 720);
        let none = (0, 0, 0, 0);
        // 1920x1080 with a 40-pixel panel: up to 1x, because 1.5x takes 1080
        // lines, which with a title bar do not fit in 1040.
        assert_eq!(fitting(sb4, 1.0, none, (1920, 1040)), 0b11);
        assert_eq!(fitting(sb4, 1.0, none, (1920, 1080)), 0b111);
        assert_eq!(fitting(sb4, 1.0, (30, 1, 1, 1), (1920, 1080)), 0b11);
        // 4K: all of them, but 3x has no room for a title bar.
        assert_eq!(fitting(sb4, 1.0, none, (3840, 2160)), 0b11_1111);
        assert_eq!(fitting(sb4, 1.0, (30, 0, 0, 0), (3840, 2160)), 0b01_1111);
        // A Retina display of 1440x900 points holds 2x, 2560x1440 pixels.
        assert_eq!(fitting(sb4, 2.0, (28, 0, 0, 0), (1440, 875)), 0b1111);
        // A 640x480 handheld holds only half size.
        assert_eq!(fitting(sb4, 1.0, none, (640, 480)), 0b1);
    }

    #[test]
    fn the_size_the_window_is_at_is_checked() {
        let sb4 = (1280, 720);
        assert_eq!(current(sb4, (1280, 720), false), Some(1));
        assert_eq!(current(sb4, (1920, 1080), false), Some(2));
        assert_eq!(current(sb4, (640, 360), false), Some(0));
        // Not quite a multiple, or fullscreen: none.
        assert_eq!(current(sb4, (1280, 721), false), None);
        assert_eq!(current(sb4, (1920, 1080), true), None);
    }

    #[test]
    fn a_window_snaps_to_16_9() {
        let sb4 = (1280, 720);
        let snap = |size, room| snap_to_shape(size, sb4, room);
        // Already 16:9 to within a pixel of height: nothing.
        for size in [
            (1280, 720),
            (640, 360),
            (1920, 1080),
            (1366, 768),
            (1281, 720),
            (1280, 721),
        ] {
            assert_eq!(snap(size, None), None, "{size:?}");
        }
        // The width stays.
        assert_eq!(snap((640, 480), None), Some((640, 360)));
        assert_eq!(snap((800, 600), None), Some((800, 450)));
        assert_eq!(snap((1280, 700), None), Some((1280, 720)));
        assert_eq!(snap((1000, 1000), Some((1920, 1040))), Some((1000, 563)));
        // A height past the room: the height stays instead.
        assert_eq!(snap((1900, 900), Some((1920, 1000))), Some((1600, 900)));
        // Neither fits: the largest that does.
        assert_eq!(snap((1900, 1000), Some((1700, 900))), Some((1600, 900)));
        assert_eq!(snap((1000, 1000), Some((800, 1000))), Some((800, 450)));
        // An unknown or empty room is no limit, and a size with a zero
        // side has no shape.
        assert_eq!(snap((640, 480), Some((0, 0))), Some((640, 360)));
        assert_eq!(snap((0, 480), None), None);
    }

    #[test]
    fn the_shape_is_put_right_once_the_size_settles() {
        let sb4 = (1280, 720);
        let t0 = Instant::now();
        let at = |ms: u64| t0 + Duration::from_millis(ms);
        let mut keep = KeepShape::default();
        // Nothing resized, nothing to do.
        assert_eq!(keep.tick(at(1000), (640, 480), false, sb4, None), None);
        // A drag: sizes keep coming, and nothing is asked until they stop.
        for (ms, w) in [(0, 700), (50, 720), (100, 760), (150, 800)] {
            keep.resized(at(ms));
            assert_eq!(keep.tick(at(ms + 10), (w, 600), false, sb4, None), None);
        }
        assert_eq!(keep.tick(at(400), (800, 600), false, sb4, None), None);
        assert_eq!(
            keep.tick(at(450), (800, 600), false, sb4, None),
            Some((800, 450))
        );
        // Asked once: no more without another resize.
        assert_eq!(keep.tick(at(2000), (800, 600), false, sb4, None), None);
        // Given: the settled size is 16:9, and nothing more is asked.
        keep.resized(at(2100));
        assert_eq!(keep.tick(at(2500), (800, 450), false, sb4, None), None);
        // Fullscreen or maximized windows are left unchanged.
        keep.resized(at(3000));
        assert_eq!(keep.tick(at(3500), (1920, 1040), true, sb4, None), None);
    }

    #[test]
    fn a_system_that_keeps_refusing_is_left_alone() {
        let sb4 = (1280, 720);
        let t0 = Instant::now();
        let at = |ms: u64| t0 + Duration::from_millis(ms);
        let mut keep = KeepShape::default();
        let mut asked = 0;
        // A tiling window manager that puts the window back to 800x600
        // after every size asked for.
        for round in 0..20u64 {
            keep.resized(at(round * 1000));
            if keep
                .tick(at(round * 1000 + 500), (800, 600), false, sb4, None)
                .is_some()
            {
                asked += 1;
            }
        }
        assert_eq!(asked, 2);
        // The user resizing it does not start it again...
        keep.resized(at(30_000));
        assert_eq!(keep.tick(at(31_000), (900, 700), false, sb4, None), None);
        // ...turning the item on again does, once.
        assert_eq!(keep.now((900, 700), false, sb4, None), Some((900, 506)));
        assert_eq!(keep.now((900, 700), true, sb4, None), None);
        // A refusal followed by a size given clears the count.
        let mut keep = KeepShape::default();
        keep.resized(at(0));
        assert!(keep.tick(at(500), (800, 600), false, sb4, None).is_some());
        keep.resized(at(600));
        assert!(keep.tick(at(1000), (800, 600), false, sb4, None).is_some());
        keep.resized(at(1100));
        assert_eq!(keep.tick(at(1500), (800, 450), false, sb4, None), None);
        assert_eq!(keep.refused_in_a_row, 0);
    }

    #[test]
    fn the_window_starts_no_larger_than_the_display() {
        assert_eq!(start_size((1280, 720), Some((1920, 1040))), (1280, 720));
        // A handheld's 640x480 display: the window covers it, the same as
        // the buffer SDL draws into does.
        assert_eq!(start_size((1280, 720), Some((640, 480))), (640, 480));
        // A 1024x768 display with a panel at the bottom.
        assert_eq!(start_size((1280, 720), Some((1024, 738))), (1024, 720));
        // With the area unknown, the window asks for the size it wanted.
        assert_eq!(start_size((1280, 720), None), (1280, 720));
        assert_eq!(start_size((1280, 720), Some((0, 0))), (1280, 720));
    }
}
