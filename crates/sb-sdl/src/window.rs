//! The emulator's windows: what size the main one starts at, the sizes
//! View > Window size gives it, which one comes to the front for input
//! that another window's dialog keeps, and where the desktop puts the
//! program's screen and the editor in the main one.

use sb_runtime::screen::{Filter, ScreenMode};
use sb_runtime::settings::Settings;
use sdl3::event::Event;
use sdl3::render::FRect;

/// View > Window size: each item's label and the multiple of the
/// profile's display (1280x720 for SmileBASIC 4) it sets the main
/// window's pixels to, in halves. A line here is an item there.
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
/// `density` pixels to a coordinate: 2 on a Mac's Retina display, the
/// desktop's scale on Wayland, 1 on Windows and X11, whose coordinates are
/// pixels.
pub fn coordinates((w, h): (u32, u32), density: f32) -> (u32, u32) {
    let density = if density > 0.0 { density } else { 1.0 };
    let at = |n: u32| ((n as f32 / density).round() as u32).max(1);
    (at(w), at(h))
}

/// The items of [`SCALES`] whose size fits a display's usable area
/// `usable` with the window's `borders` (top, left, bottom, right) around
/// it, as bits from the first item up. The others are disabled rather
/// than shrunk, as a shrunk size is not the multiple asked for.
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

/// The item of [`SCALES`] the main window is at, which is checked: its
/// pixels are that multiple exactly, and it is not fullscreen.
pub fn current(display: (u32, u32), pixels: (u32, u32), fullscreen: bool) -> Option<usize> {
    let at = |&(_, halves): &(&str, u32)| scaled(display, halves) == pixels;
    SCALES.iter().position(at).filter(|_| !fullscreen)
}

/// One of the emulator's windows: the main one, or the editor's own.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Which {
    Main,
    Editor,
}

/// The window that shows the emulator's window taking all input, if one
/// is `modal` and the editor's own window is shown (`apart`): the window
/// that had the keys when it opened, `ui_editor` for the editor's. With
/// one window there is nothing to bring forward.
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
/// shortcuts, Ctrl+S's and Ctrl+L's windows) shows on `modal_on`. A
/// press, a click, the wheel or typing in the other window brings that
/// one forward, and goes no further: it would otherwise reach the window
/// the user cannot see, or nothing, which looks like a hang. The pointer's
/// motion and releases are left alone.
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

/// The size the main window starts at, in the system's window
/// coordinates, when it asks for `w` x `h` on a display whose usable area
/// is `usable`: no larger on either side. A window larger than the
/// display shows only in part on most systems; on Linux's KMS/DRM, the
/// console without a desktop, SDL draws into a buffer the display's size
/// while the window keeps the size it asked for, so the renderer would
/// lay its frames out for the wrong size.
pub fn start_size((w, h): (u32, u32), usable: Option<(u32, u32)>) -> (u32, u32) {
    match usable {
        Some((uw, uh)) if uw > 0 && uh > 0 => (w.min(uw), h.min(uh)),
        _ => (w, h),
    }
}

/// Whether the desktop's 1280x720 display, at `display` in a window
/// `out_h` pixels high, leaves room below it: in a window narrower than
/// 16:9, where it is scaled to the window's width at its top. Elsewhere
/// it fills the window's height, give or take rounding.
fn room_below(display: FRect, out_h: u32) -> bool {
    display.y + display.h + 0.5 < out_h as f32
}

/// The height the desktop places the program's screen in, from the top
/// of a window `out_h` pixels high whose display is at `display`: the
/// whole window's. Where the display fills the window's height, the
/// display's, which differs from the window's only by rounding: that
/// keeps the frames in such windows as they were when the screen was
/// confined to the display.
pub fn screen_height(display: FRect, out_h: u32) -> f32 {
    match room_below(display, out_h) {
        true => out_h as f32,
        false => display.h.min(out_h as f32),
    }
}

/// How far down the desktop's UI goes, in its own pixels, in which the
/// display at `display` is `display_h` high: to the bottom of a window
/// `out_h` pixels high. More than the display only where it leaves room
/// below, where the screen, the editor, the debugger's panel, the
/// notices and the program's DIALOG go too.
pub fn ui_height(display: FRect, display_h: u32, out_h: u32) -> u32 {
    if !room_below(display, out_h) || display.h <= 0.0 {
        return display_h;
    }
    let below = (out_h as f32 - display.y) * display_h as f32 / display.h;
    (below.floor() as u32).max(display_h)
}

/// Where the desktop shows the screen of `mode` in a window of `out`
/// pixels whose display is at `display`, and the filter it is scaled
/// with: centered in the whole window, at the largest size View > Screen
/// filter allows (see [`crate::screen_place`]). With `editor`, the
/// editor's frame of that many pixels instead, `mode` being the editor's,
/// as [`editor_rect`] says; its filter is then [`crate::editor_filter`]'s
/// to choose.
pub fn screen_place(
    mode: &ScreenMode,
    settings: &Settings,
    editor: Option<(u32, u32)>,
    out: (u32, u32),
    display: FRect,
) -> (FRect, Filter) {
    let height = screen_height(display, out.1);
    let (rect, filter) = crate::screen_place(mode, settings, false, out.0, height);
    match editor {
        Some(frame) => {
            let nearest = settings.screen_filter == Some(Filter::Nearest);
            (editor_rect(rect, frame, nearest, out, display), filter)
        }
        None => (rect, filter),
    }
}

/// Where the desktop shows the editor's frame of `frame` pixels, which
/// goes where the screen of the editor's shape goes, at `screen`, in a
/// window of `out` pixels whose display is at `display`. `nearest` is
/// View > Screen filter's Nearest, which keeps to whole factors.
///
/// In a window of 16:9 or wider it goes at `screen`. Where the display
/// leaves room below, Nearest's rule is applied to the frame as well as
/// to the screen's size, and the larger wins: a whole factor of the
/// frame where it fits once, at which the editor is sharp (see
/// [`crate::editor_filter`]), and scaled down to fit where it does not.
/// The screen's whole factors alone leave the editor small there:
/// XSCREEN 400,240 fits 640x480 once, and the editor's frame, twice as
/// fine for 16-pixel characters, shows at half its size. It is centered
/// in the display, or where taller than that, starts at the window's top,
/// so that the menu bar covers the row the editor leaves it; on a whole
/// pixel, where a whole factor stays sharp.
pub fn editor_rect(
    screen: FRect,
    frame: (u32, u32),
    nearest: bool,
    out: (u32, u32),
    display: FRect,
) -> FRect {
    if !room_below(display, out.1) {
        return screen;
    }
    let (out_w, out_h) = (out.0 as f32, out.1 as f32);
    let (fw, fh) = (frame.0.max(1) as f32, frame.1.max(1) as f32);
    let (mut w, mut h) = (screen.w, screen.h);
    let fit = (out_w / fw).min(out_h / fh);
    let scale = if fit >= 1.0 { fit.floor() } else { fit };
    if nearest && fh * scale > h {
        (w, h) = (fw * scale, fh * scale);
    }
    let y = match h <= display.h {
        true => display.y + (display.h - h) / 2.0,
        false => 0.0,
    };
    FRect::new(((out_w - w) / 2.0).round(), y.round(), w, h)
}

/// The rectangle [`ui_height`] makes of the focus border's `rect` on the
/// display, which is `display` UI pixels large: the whole display, the
/// main interpreter's, goes down to `ui_h`, where the screen goes too; a
/// subprogram's screen stays where XSUBSCREEN put it on the display.
pub fn border_rect(rect: [i32; 4], display: (u32, u32), ui_h: u32) -> [i32; 4] {
    match rect == [0, 0, display.0 as i32, display.1 as i32] {
        true => [0, 0, display.0 as i32, ui_h as i32],
        false => rect,
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
            // The dialog on the main window, input in the editor's.
            assert_eq!(blocked_by(&event, Some(Main), Editor), Some(Main));
            // And the other way round.
            assert_eq!(blocked_by(&event, Some(Editor), Main), Some(Editor));
            // In the dialog's own window, it goes to the dialog.
            assert_eq!(blocked_by(&event, Some(Main), Main), None);
            assert_eq!(blocked_by(&event, Some(Editor), Editor), None);
            // With no dialog, or no editor's window, nothing is blocked.
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
        // A press in the editor's window with the settings on the main
        // one brings the main one forward.
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
        // In window coordinates: a Retina display's are half the pixels.
        assert_eq!(coordinates((1280, 720), 1.0), (1280, 720));
        assert_eq!(coordinates((1280, 720), 2.0), (640, 360));
        assert_eq!(coordinates((1920, 1080), 1.5), (1280, 720));
        assert_eq!(coordinates((1280, 720), 0.0), (1280, 720));
    }

    #[test]
    fn sizes_past_the_display_are_left_out() {
        let sb4 = (1280, 720);
        let none = (0, 0, 0, 0);
        // 1920x1080 with a 40-pixel panel: up to 1.5x, as 1080 lines with a
        // title bar do not fit in 1040.
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

    /// The desktop's display in a window of `out` pixels.
    fn display(out: (u32, u32)) -> FRect {
        crate::display_rect(1280, 720, out.0, out.1)
    }

    fn with(filter: Option<Filter>) -> Settings {
        Settings {
            screen_filter: filter,
            ..Settings::default()
        }
    }

    fn place(
        w: u32,
        h: u32,
        filter: Option<Filter>,
        out: (u32, u32),
    ) -> ((f32, f32, f32, f32), Filter) {
        let mode = ScreenMode::new(w, h);
        let (r, filter) = screen_place(&mode, &with(filter), None, out, display(out));
        ((r.x, r.y, r.w, r.h), filter)
    }

    const NEAREST: Option<Filter> = Some(Filter::Nearest);
    const BILINEAR: Option<Filter> = Some(Filter::Bilinear);

    #[test]
    fn the_screen_fills_a_window_narrower_than_16_9() {
        use Filter::{Bilinear, Nearest};
        // A 4:3 screen fills a 4:3 window; before, it was shrunk into the
        // display's 640x360.
        assert_eq!(
            place(640, 480, NEAREST, (640, 480)),
            ((0.0, 0.0, 640.0, 480.0), Nearest)
        );
        assert_eq!(
            place(320, 240, NEAREST, (640, 480)),
            ((0.0, 0.0, 640.0, 480.0), Nearest)
        );
        assert_eq!(
            place(640, 480, NEAREST, (800, 600)),
            ((80.0, 60.0, 640.0, 480.0), Nearest)
        );
        assert_eq!(
            place(640, 480, NEAREST, (1024, 768)),
            ((192.0, 144.0, 640.0, 480.0), Nearest)
        );
        assert_eq!(
            place(320, 240, NEAREST, (1920, 1200)),
            ((160.0, 0.0, 1600.0, 1200.0), Nearest)
        );
        // Nearest's whole factors, centered in the whole window.
        assert_eq!(
            place(400, 240, NEAREST, (640, 480)),
            ((120.0, 120.0, 400.0, 240.0), Nearest)
        );
        assert_eq!(
            place(400, 240, NEAREST, (800, 600)),
            ((0.0, 60.0, 800.0, 480.0), Nearest)
        );
        assert_eq!(
            place(400, 240, NEAREST, (720, 720)),
            ((160.0, 240.0, 400.0, 240.0), Nearest)
        );
        assert_eq!(
            place(400, 240, NEAREST, (1920, 1200)),
            ((160.0, 120.0, 1600.0, 960.0), Nearest)
        );
        // Not even once: shrunk to fit, smoothly.
        assert_eq!(
            place(1280, 720, NEAREST, (640, 480)),
            ((0.0, 60.0, 640.0, 360.0), Bilinear)
        );
        assert_eq!(
            place(640, 480, NEAREST, (720, 720)),
            ((40.0, 120.0, 640.0, 480.0), Nearest)
        );
        assert_eq!(
            place(1280, 720, NEAREST, (720, 720)),
            ((0.0, 157.5, 720.0, 405.0), Bilinear)
        );
        // Bilinear fits, and Program (XSCREEN) does with XSCREEN's filter.
        assert_eq!(
            place(400, 240, BILINEAR, (640, 480)),
            ((0.0, 48.0, 640.0, 384.0), Bilinear)
        );
        assert_eq!(
            place(640, 480, BILINEAR, (1024, 768)),
            ((0.0, 0.0, 1024.0, 768.0), Bilinear)
        );
        let (rect, _) = place(400, 240, None, (720, 720));
        assert_eq!(rect, (0.0, 144.0, 720.0, 432.0));
    }

    #[test]
    fn nothing_moves_in_a_window_of_16_9_or_wider() {
        let filters = [None, NEAREST, BILINEAR, Some(Filter::SmartNearest)];
        let modes = [
            (400, 240),
            (320, 240),
            (640, 480),
            (1280, 720),
            (1280, 480),
            (16, 720),
        ];
        let mut outs = vec![
            (2560, 1080),
            (3440, 1440),
            (1281, 720),
            (1366, 768),
            (1600, 900),
        ];
        outs.extend((1..=48).map(|n| (n * 80, n * 45)));
        outs.extend(
            (100u32..=2200)
                .step_by(7)
                .map(|h| ((h * 16).div_ceil(9), h)),
        );
        for out in outs {
            assert_eq!(ui_height(display(out), 720, out.1), 720, "{out:?}");
            for filter in filters {
                for (w, h) in modes {
                    let mode = ScreenMode::new(w, h);
                    let settings = with(filter);
                    // As before: in the display, which fills the height.
                    let screen_h = display(out).h.min(out.1 as f32);
                    let before = crate::screen_place(&mode, &settings, false, out.0, screen_h);
                    let now = screen_place(&mode, &settings, None, out, display(out));
                    assert_eq!(now, before, "{out:?} {filter:?} {w}x{h}");
                    let frame = (828, 497);
                    let editor = screen_place(&mode, &settings, Some(frame), out, display(out));
                    assert_eq!(editor, before, "{out:?} {filter:?} {w}x{h} editor");
                }
            }
        }
    }

    #[test]
    fn the_ui_goes_down_to_the_windows_bottom() {
        let at = |out: (u32, u32)| ui_height(display(out), 720, out.1);
        assert_eq!(at((1280, 720)), 720);
        assert_eq!(at((640, 360)), 720);
        assert_eq!(at((640, 480)), 960);
        assert_eq!(at((800, 600)), 960);
        assert_eq!(at((1024, 768)), 960);
        assert_eq!(at((720, 720)), 1280);
        assert_eq!(at((1920, 1200)), 800);
        assert_eq!(at((2560, 1080)), 720);
        // A minimized window, or one with no height.
        assert_eq!(ui_height(display((0, 0)), 720, 0), 720);
        // The main interpreter's focus border goes down with it; a
        // subprogram's stays where XSUBSCREEN put it.
        assert_eq!(
            border_rect([0, 0, 1280, 720], (1280, 720), 960),
            [0, 0, 1280, 960]
        );
        let sub = [640, 0, 640, 360];
        assert_eq!(border_rect(sub, (1280, 720), 960), sub);
    }

    #[test]
    fn the_editor_takes_the_room_below_the_display() {
        use sb_runtime::editor::{EditorSize, frame_size};
        use sb_runtime::font::FontSize;
        let editor = |frame: (u32, u32), filter: Option<Filter>, out: (u32, u32)| {
            let mode = ScreenMode::new(400, 240);
            let (r, _) = screen_place(&mode, &with(filter), Some(frame), out, display(out));
            (r.x, r.y, r.w, r.h)
        };
        // The default editor, Medium with 16-pixel characters, twice as
        // fine as the 400x240 screen: too large for 640x480 once, so
        // shrunk to fit, at the top, where the menu bar covers the row it
        // leaves for it. The screen's 1x would be 400x240.
        let medium = frame_size((400, 240), FontSize::Large, EditorSize::Medium);
        assert_eq!(medium, (828, 497));
        let (x, y, w, h) = editor(medium, NEAREST, (640, 480));
        assert!(
            x.abs() < 0.01 && y == 0.0 && (w - 640.0).abs() < 0.01,
            "{x} {y} {w}"
        );
        assert!((h - 384.15).abs() < 0.01, "{h}");
        // Once where it fits, centered in the display's 576 rows.
        assert_eq!(
            editor(medium, NEAREST, (1024, 768)),
            (98.0, 40.0, 828.0, 497.0)
        );
        // Twice in 1920x1200, where the screen's 4x is 1600x960.
        assert_eq!(
            editor(medium, NEAREST, (1920, 1200)),
            (132.0, 43.0, 1656.0, 994.0)
        );
        // With 8-pixel characters, Large fits 640x480 twice, sharp.
        let large = frame_size((400, 240), FontSize::Small, EditorSize::Large);
        assert_eq!(large, (318, 191));
        assert_eq!(editor(large, NEAREST, (640, 480)), (2.0, 0.0, 636.0, 382.0));
        // Where the screen's whole factor is larger, the screen's place.
        assert_eq!(editor(large, NEAREST, (800, 600)), (0.0, 0.0, 800.0, 480.0));
        // Bilinear fits the screen's shape to the window.
        assert_eq!(
            editor(medium, BILINEAR, (640, 480)),
            (0.0, 0.0, 640.0, 384.0)
        );
        // With the display's whole height, where the screen goes.
        let short = editor(large, NEAREST, (720, 720));
        assert_eq!(short, (42.0, 12.0, 636.0, 382.0));
    }

    #[test]
    fn a_pointer_hits_the_pixel_under_it() {
        let outs = [
            (640, 480),
            (800, 600),
            (1024, 768),
            (720, 720),
            (1920, 1200),
            (1280, 720),
        ];
        let modes = [(400, 240), (640, 480), (1280, 720), (320, 240)];
        let frames = [None, Some((828, 497)), Some((318, 191))];
        for out in outs {
            for (w, h) in modes {
                for frame in frames {
                    for filter in [NEAREST, BILINEAR] {
                        let mode = ScreenMode::new(w, h);
                        let (shown, _) =
                            screen_place(&mode, &with(filter), frame, out, display(out));
                        // On a Retina display, two pixels to a coordinate.
                        for px in [1.0, 2.0] {
                            let map = crate::Mapping {
                                px,
                                display: display(out),
                                display_w: 1280,
                                shown,
                                space: frame.unwrap_or((w, h)),
                            };
                            let (sw, sh) = map.space;
                            let corners = [(0, 0), (sw - 1, 0), (0, sh - 1), (sw - 1, sh - 1)];
                            for (x, y) in corners.into_iter().chain([(sw / 2, sh / 2)]) {
                                let (wx, wy) = map.screen_to_window(x as i32, y as i32);
                                let back = map.to_screen(wx, wy);
                                assert_eq!(back, (x as i32, y as i32), "{out:?} {w}x{h} {frame:?}");
                            }
                            // Just past the picture's edges, off it.
                            let (left, top) = ((shown.x - 1.0) / px, (shown.y - 1.0) / px);
                            let (x, y) = map.to_screen(left, top);
                            assert!(x < 0 && y < 0, "{out:?} {w}x{h}");
                            let right = (shown.x + shown.w + 1.0) / px;
                            let bottom = (shown.y + shown.h + 1.0) / px;
                            let (x, y) = map.to_screen(right, bottom);
                            assert!(x >= sw as i32 && y >= sh as i32, "{out:?} {w}x{h}");
                        }
                    }
                }
            }
        }
        // A 4:3 screen in a 640x480 window: the window's pixel is the
        // screen's.
        let mode = ScreenMode::new(640, 480);
        let (shown, _) = screen_place(&mode, &with(NEAREST), None, (640, 480), display((640, 480)));
        let map = crate::Mapping {
            px: 1.0,
            display: display((640, 480)),
            display_w: 1280,
            shown,
            space: (640, 480),
        };
        assert_eq!(map.to_screen(0.5, 0.5), (0, 0));
        assert_eq!(map.to_screen(320.5, 400.5), (320, 400));
        assert_eq!(map.to_screen(639.5, 479.5), (639, 479));
    }

    #[test]
    fn the_window_starts_no_larger_than_the_display() {
        assert_eq!(start_size((1280, 720), Some((1920, 1040))), (1280, 720));
        // A handheld's 640x480 display: the window covers it, as the
        // buffer SDL draws into does.
        assert_eq!(start_size((1280, 720), Some((640, 480))), (640, 480));
        // A 1024x768 display with a panel at the bottom.
        assert_eq!(start_size((1280, 720), Some((1024, 738))), (1024, 720));
        // Unknown, it asks for what it wanted.
        assert_eq!(start_size((1280, 720), None), (1280, 720));
        assert_eq!(start_size((1280, 720), Some((0, 0))), (1280, 720));
    }
}
