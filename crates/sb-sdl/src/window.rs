//! The emulator's windows: what size the main one starts at, the sizes
//! View > Window size gives it, which one comes to the front for input
//! that another window's dialog keeps, and where the desktop puts the
//! program's screen and the editor in the main one.

use crate::editor_layout;
use sb_runtime::editor::{Area, EditorSize};
use sb_runtime::font::FontSize;
use sb_runtime::screen::{Filter, ScreenMode};
use sb_runtime::settings::Settings;
use sdl3::event::Event;
use sdl3::render::FRect;
use std::time::{Duration, Instant};

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

/// The size nearest to `(w, h)` with the shape of `display` (16:9 for
/// SmileBASIC 4) that fits `room`, the display's usable area less the
/// window's borders, if known; `None` where the size is that shape to a
/// pixel already. The width stays and the height follows, unless that
/// leaves the room; then the height stays and the width follows, and
/// where neither fits, the largest size of the shape that does.
pub fn snap_to_shape(
    (w, h): (u32, u32),
    display: (u32, u32),
    room: Option<(u32, u32)>,
) -> Option<(u32, u32)> {
    let (dw, dh) = (u64::from(display.0.max(1)), u64::from(display.1.max(1)));
    let (w64, h64) = (u64::from(w), u64::from(h));
    // Off by more than a pixel of height.
    if (h64 * dw).abs_diff(w64 * dh) <= dw || w == 0 || h == 0 {
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

/// How long the main window's size has to stay before View > "Keep 16:9
/// when resizing" puts it right: a drag sends sizes more often than this.
pub const SETTLE: Duration = Duration::from_millis(300);

/// How many sizes in a row the system may refuse before the window's
/// shape is left as it is, until the item is turned on again.
const STRIKES: u32 = 2;

/// Puts the main window back to the display's shape after the user
/// resizes it, where the system ignores the shape the window asks for
/// (some Wayland compositors do): once the size has settled, never during
/// a drag, and never in a loop with a system that keeps refusing.
#[derive(Debug, Default)]
pub struct KeepShape {
    /// When the window was last resized, until its size has settled.
    resized: Option<Instant>,
    /// The size last asked for, until the window's size settles after it.
    asked: Option<(u32, u32)>,
    /// The sizes asked for in a row that the system did not give.
    strikes: u32,
}

impl KeepShape {
    /// The window's size changed, by the user, the system or a size
    /// asked for.
    pub fn resized(&mut self, now: Instant) {
        self.resized = Some(now);
    }

    /// The item was turned on, or the window has to be put right at once:
    /// starts afresh and gives the size to ask for, if any. `free` is a
    /// window the shape is not kept for: fullscreen or maximized.
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
        // The size that settled after one asked for: given, or refused.
        if self.asked.take().is_some() {
            match wanted {
                None => self.strikes = 0,
                Some(_) => self.strikes += 1,
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
        if free || self.strikes >= STRIKES {
            return None;
        }
        self.asked = snap_to_shape(size, display, room);
        self.asked
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
/// filter allows (see [`crate::screen_place`]).
pub fn screen_place(
    mode: &ScreenMode,
    settings: &Settings,
    out: (u32, u32),
    display: FRect,
) -> (FRect, Filter) {
    let height = screen_height(display, out.1);
    crate::screen_place(mode, settings, false, out.0, height)
}

/// Where the main window's menu bar, its menus and its context menus go
/// apart from the rest of the UI: at their own size, 1x, from the
/// display's top left corner, with `size` pixels of room down to the
/// window's bottom. That is in a window whose display is scaled below
/// 1x, as 640x480 scales it to 0.5x, which would draw them shrunk and
/// blurred. `None` where the display's scale is 1x or more: they are
/// scaled with the display there, as the rest of the UI is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MenuPlace {
    /// The bar's top left corner on the window, in its pixels.
    pub at: (f32, f32),
    pub size: (u32, u32),
}

impl MenuPlace {
    /// The rectangle a layer of the menus is copied to at 1x.
    pub fn rect(self) -> FRect {
        FRect::new(self.at.0, self.at.1, self.size.0 as f32, self.size.1 as f32)
    }
}

/// See [`MenuPlace`]: for a window of `out` pixels whose display of
/// `display_w` pixels across is at `display`.
pub fn menu_place(display: FRect, display_w: u32, out: (u32, u32)) -> Option<MenuPlace> {
    if display.w + 0.5 >= display_w as f32 {
        return None;
    }
    let at = (display.x.round(), display.y.round());
    let w = (display.w.floor() as u32).max(1);
    let h = out.1.saturating_sub(at.1 as u32).max(1);
    Some(MenuPlace { at, size: (w, h) })
}

/// How many of the window's pixels a pixel of the menu bar takes: 1
/// where it goes apart (see [`menu_place`]), the display's scale
/// otherwise.
pub fn menu_scale(display: FRect, display_w: u32, out: (u32, u32)) -> f32 {
    match menu_place(display, display_w, out) {
        Some(_) => 1.0,
        None => display.w / display_w as f32,
    }
}

/// The main window's editor, laid out as in the editor's own window (see
/// [`crate::editor_layout`]): its frame, the area, fills the window from
/// its top left corner, and each of its pixels is `zoom` pixels of the
/// window square, a whole number, so the dot fonts stay sharp.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fill {
    pub zoom: u32,
    pub area: Area,
}

impl Fill {
    /// Where the frame goes on the window: from its top left corner,
    /// past its right and bottom edges by less than a pixel of the frame.
    pub fn rect(self) -> FRect {
        let (w, h) = self.area.size;
        FRect::new(0.0, 0.0, (w * self.zoom) as f32, (h * self.zoom) as f32)
    }
}

/// The main window's editor in a window of `out` pixels whose display is
/// at `display`, for the editor's `font` at F2's `size`, under a menu bar
/// `bar` pixels of the window high. The display's height sets the zoom,
/// as the desktop's scale does in the editor's own window: the UI is
/// scaled with the display here.
pub fn editor_fill(
    display: FRect,
    out: (u32, u32),
    look: (FontSize, EditorSize),
    bar: u32,
) -> Fill {
    let zoom = editor_layout::zoom(out.0.max(1), display.h, look);
    let area = editor_layout::area(out, zoom, bar);
    Fill { zoom, area }
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
        let (r, filter) = screen_place(&mode, &with(filter), out, display(out));
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
                    let now = screen_place(&mode, &settings, out, display(out));
                    assert_eq!(now, before, "{out:?} {filter:?} {w}x{h}");
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

    /// The main window's editor in a window of `out` pixels under a menu
    /// bar of 24: its zoom, its frame's size and the rows under the bar.
    fn fill(out: (u32, u32), font: FontSize, size: EditorSize) -> (u32, (u32, u32), u32) {
        let Fill { zoom, area } = editor_fill(display(out), out, (font, size), 24);
        (zoom, area.size, area.top)
    }

    #[test]
    fn the_editor_fills_the_window_at_a_whole_zoom() {
        use EditorSize::{Large, Medium, Small, Tiny};
        use FontSize::{Large as F16, Small as F8};
        // The default, Medium with 16-pixel characters: 1x in each, as
        // its 28 rows take 497 pixels and the displays are 360 to 576
        // high. 40 columns on 640x480, 30 rows under the bar.
        assert_eq!(fill((640, 480), F16, Medium), (1, (640, 480), 24));
        assert_eq!(fill((800, 600), F16, Medium), (1, (800, 600), 24));
        assert_eq!(fill((1024, 768), F16, Medium), (1, (1024, 768), 24));
        assert_eq!(fill((720, 720), F16, Medium), (1, (720, 720), 24));
        // 2x in 1920x1200, whose display is 1080 high.
        assert_eq!(fill((1920, 1200), F16, Medium), (2, (960, 600), 12));
        // 8-pixel characters at Large are 2x on 640x480: 40 columns.
        assert_eq!(fill((640, 480), F8, Large), (2, (320, 240), 12));
        assert_eq!(fill((1024, 768), F8, Large), (3, (342, 256), 8));
        assert_eq!(fill((1024, 768), F8, Medium), (2, (512, 384), 12));
        assert_eq!(fill((640, 480), F8, Medium), (1, (640, 480), 24));
        assert_eq!(fill((640, 480), F8, Tiny), (1, (640, 480), 24));
        // 16-pixel Large would be 2x on 1024x768, but 32 columns are
        // fewer than 40, so 1x.
        assert_eq!(fill((1024, 768), F16, Large), (1, (1024, 768), 24));
        assert_eq!(fill((1920, 1200), F16, Large), (3, (640, 400), 8));
        assert_eq!(fill((1920, 1200), F16, Small), (1, (1920, 1200), 24));
        // A window too narrow for 40 columns even at 1x.
        assert_eq!(fill((320, 240), F16, Large), (1, (320, 240), 24));
        // Odd sizes: the frame covers the window, past it by less than a
        // pixel of the frame.
        let (zoom, (w, h), _) = fill((1025, 767), F8, Large);
        assert_eq!(zoom, 3);
        assert!(w * 3 >= 1025 && w * 3 - 1025 < 3 && h * 3 >= 767 && h * 3 - 767 < 3);
        // Nothing at all.
        let (zoom, (w, h), _) = fill((0, 0), F16, Medium);
        assert!(zoom == 1 && w >= 1 && h >= 1);
    }

    /// The main window's editor as the frontend lays it out, under the
    /// menu bar at the height the window shows it.
    fn laid_out(out: (u32, u32), look: (FontSize, EditorSize)) -> Fill {
        let scale = menu_scale(display(out), 1280, out);
        let bar = (24.0 * scale).ceil() as u32;
        editor_fill(display(out), out, look, bar)
    }

    /// F2's sizes from Large to Tiny: each one's zoom, and its columns and
    /// rows, the ruler and the status line included, under the menu bar.
    fn sizes(out: (u32, u32), font: FontSize) -> [(u32, u32, u32); 4] {
        use EditorSize::{Large, Medium, Small, Tiny};
        [Large, Medium, Small, Tiny].map(|size| {
            let Fill { zoom, area } = laid_out(out, (font, size));
            let cell = font as u32;
            let rows = (area.size.1 - area.top) / cell;
            (zoom, area.size.0 / cell, rows)
        })
    }

    #[test]
    fn a_16_9_window_lays_the_editor_out_too() {
        use EditorSize::{Large, Medium};
        use FontSize::{Large as F16, Small as F8};
        // 1280x720 with the default look: 1x, the whole window, the bar's
        // 24 pixels over its top.
        let fill = laid_out((1280, 720), (F16, Medium));
        assert_eq!(
            (fill.zoom, fill.area.size, fill.area.top),
            (1, (1280, 720), 24)
        );
        assert_eq!(fill.rect(), FRect::new(0.0, 0.0, 1280.0, 720.0));
        // 1920x1080: 2x, under a bar of 36 pixels.
        let fill = laid_out((1920, 1080), (F16, Medium));
        assert_eq!(
            (fill.zoom, fill.area.size, fill.area.top),
            (2, (960, 540), 18)
        );
        let fill = laid_out((2560, 1440), (F8, Large));
        assert_eq!(
            (fill.zoom, fill.area.size, fill.area.top),
            (8, (320, 180), 6)
        );
        // Wider than 16:9: the frame covers the whole window, beside the
        // display too.
        let fill = laid_out((2560, 1080), (F16, Medium));
        assert_eq!(
            (fill.zoom, fill.area.size, fill.area.top),
            (2, (1280, 540), 18)
        );
    }

    #[test]
    fn each_size_takes_a_whole_zoom_on_common_windows() {
        use FontSize::{Large as F16, Small as F8};
        // (zoom, columns, rows) for Large, Medium, Small and Tiny. The
        // 16-pixel font is never drawn below 1x, so smaller sizes show the
        // same where 1x is already nearest: three of four on 1280x720.
        let table = [
            ((640, 480), F16, [(1, 40, 28); 4]),
            ((1024, 768), F16, [(1, 64, 46); 4]),
            (
                (1280, 720),
                F16,
                [(2, 40, 21), (1, 80, 43), (1, 80, 43), (1, 80, 43)],
            ),
            (
                (1920, 1080),
                F16,
                [(3, 40, 21), (2, 60, 32), (1, 120, 65), (1, 120, 65)],
            ),
            (
                (2560, 1440),
                F16,
                [(4, 40, 21), (3, 53, 29), (2, 80, 43), (1, 160, 87)],
            ),
            (
                (3840, 2160),
                F16,
                [(6, 40, 21), (4, 60, 32), (3, 80, 43), (2, 120, 65)],
            ),
            (
                (640, 480),
                F8,
                [(2, 40, 28), (1, 80, 57), (1, 80, 57), (1, 80, 57)],
            ),
            (
                (1024, 768),
                F8,
                [(3, 42, 31), (2, 64, 46), (2, 64, 46), (1, 128, 93)],
            ),
            (
                (1280, 720),
                F8,
                [(4, 40, 21), (3, 53, 29), (2, 80, 43), (1, 160, 87)],
            ),
            (
                (1920, 1080),
                F8,
                [(6, 40, 21), (4, 60, 32), (3, 80, 43), (2, 120, 65)],
            ),
            (
                (2560, 1440),
                F8,
                [(8, 40, 21), (6, 53, 29), (4, 80, 43), (3, 106, 58)],
            ),
            (
                (3840, 2160),
                F8,
                [(11, 43, 23), (9, 53, 29), (6, 80, 43), (4, 120, 65)],
            ),
        ];
        for (out, font, expected) in table {
            assert_eq!(sizes(out, font), expected, "{out:?} {font:?}");
        }
        // On a desktop scaled by 125%, 1600x900 pixels.
        let scaled = sizes((1600, 900), F16);
        assert_eq!(scaled.map(|(zoom, ..)| zoom), [2, 2, 1, 1]);
    }

    #[test]
    fn the_main_window_lays_the_editor_out_as_its_own_window_does() {
        use EditorSize::{Large, Medium, Small, Tiny};
        // A window of a whole multiple of 1280x720 at that UI scale in
        // the editor's own window: the same zoom and frame.
        for (out, ui) in [((1280, 720), 1), ((2560, 1440), 2), ((3840, 2160), 3)] {
            for font in [FontSize::Large, FontSize::Small] {
                for size in [Large, Medium, Small, Tiny] {
                    let look = (font, size);
                    let own = crate::editor_layout::Layout::new(out, ui, look, (1280, 720), 24);
                    let main = laid_out(out, look);
                    assert_eq!(
                        (main.zoom, main.area),
                        (own.zoom, own.area),
                        "{out:?} {look:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn menus_go_apart_at_1x_below_the_displays_size() {
        let place = |out: (u32, u32)| menu_place(display(out), 1280, out);
        // At least 1280x720: scaled with the display, as before.
        let mut large = vec![(1280, 720), (1280, 960), (1920, 1200), (2560, 1080)];
        large.extend((16..=48).map(|n| (n * 80, n * 45)));
        for out in large {
            assert_eq!(place(out), None, "{out:?}");
            let scale = menu_scale(display(out), 1280, out);
            assert_eq!(scale, display(out).w / 1280.0, "{out:?}");
            assert!(scale >= 1.0);
        }
        // Smaller: 1x from the display's corner to the window's bottom.
        let rect = |out| place(out).map(|p| (p.at, p.size));
        assert_eq!(rect((640, 480)), Some(((0.0, 0.0), (640, 480))));
        assert_eq!(rect((800, 600)), Some(((0.0, 0.0), (800, 600))));
        assert_eq!(rect((1024, 768)), Some(((0.0, 0.0), (1024, 768))));
        assert_eq!(rect((720, 720)), Some(((0.0, 0.0), (720, 720))));
        assert_eq!(rect((640, 360)), Some(((0.0, 0.0), (640, 360))));
        assert_eq!(rect((1279, 720)), Some(((0.0, 0.0), (1279, 720))));
        // Wider than 16:9: within the display, centered across.
        assert_eq!(rect((1000, 500)), Some(((56.0, 0.0), (888, 500))));
        assert_eq!(menu_scale(display((640, 480)), 1280, (640, 480)), 1.0);
        // Nothing at all.
        assert!(place((0, 0)).is_some_and(|p| p.size.0 >= 1 && p.size.1 >= 1));
    }

    #[test]
    fn a_click_lands_on_the_editors_pixel_under_it() {
        use EditorSize::{Large, Medium, Small, Tiny};
        use FontSize::{Large as F16, Small as F8};
        let looks = [
            (F16, Large),
            (F16, Medium),
            (F16, Tiny),
            (F8, Large),
            (F8, Small),
        ];
        let mut zooms = Vec::new();
        for out in [
            (640, 480),
            (800, 600),
            (1024, 768),
            (720, 720),
            (1920, 1200),
            (1280, 720),
            (1366, 768),
            (1920, 1080),
            (2560, 1440),
            (2560, 1080),
        ] {
            for look in looks {
                let fill = laid_out(out, look);
                zooms.push(fill.zoom);
                // A window coordinate of one, one and a half or two pixels.
                for px in [1.0, 1.5, 2.0] {
                    let map = crate::Mapping {
                        px,
                        display: display(out),
                        display_w: 1280,
                        shown: fill.rect(),
                        space: fill.area.size,
                        menu: None,
                    };
                    // Every pixel of the window, across and down, is the
                    // frame's pixel drawn there.
                    for x in 0..out.0 {
                        let at = map.to_screen((x as f32 + 0.5) / px, 0.25);
                        assert_eq!(at.0, (x / fill.zoom) as i32, "{out:?} {look:?} {px} {x}");
                    }
                    for y in 0..out.1 {
                        let at = map.to_screen(0.25, (y as f32 + 0.5) / px);
                        assert_eq!(at.1, (y / fill.zoom) as i32, "{out:?} {look:?} {px} {y}");
                    }
                }
            }
        }
        for zoom in 1..=3 {
            assert!(zooms.contains(&zoom), "{zoom}");
        }
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
        for out in outs {
            for (w, h) in modes {
                for fill in [false, true] {
                    for filter in [NEAREST, BILINEAR] {
                        let mode = ScreenMode::new(w, h);
                        let look = (FontSize::Large, EditorSize::Medium);
                        let editor = editor_fill(display(out), out, look, 24);
                        let (shown, space) = match fill {
                            true => (editor.rect(), editor.area.size),
                            false => (
                                screen_place(&mode, &with(filter), out, display(out)).0,
                                (w, h),
                            ),
                        };
                        let frame = fill.then_some(space);
                        // On a Retina display, two pixels to a coordinate.
                        for px in [1.0, 2.0] {
                            let map = crate::Mapping {
                                px,
                                display: display(out),
                                display_w: 1280,
                                shown,
                                space,
                                menu: None,
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
        let (shown, _) = screen_place(&mode, &with(NEAREST), (640, 480), display((640, 480)));
        let map = crate::Mapping {
            px: 1.0,
            display: display((640, 480)),
            display_w: 1280,
            shown,
            space: (640, 480),
            menu: None,
        };
        assert_eq!(map.to_screen(0.5, 0.5), (0, 0));
        assert_eq!(map.to_screen(320.5, 400.5), (320, 400));
        assert_eq!(map.to_screen(639.5, 479.5), (639, 479));
    }

    #[test]
    fn a_window_snaps_to_16_9() {
        let sb4 = (1280, 720);
        let snap = |size, room| snap_to_shape(size, sb4, room);
        // 16:9 already, to a pixel of height: nothing.
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
        // An unknown or empty room is no limit; nothing has no shape.
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
        // Fullscreen or maximized windows are left as they are.
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
        assert_eq!(keep.strikes, 0);
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
