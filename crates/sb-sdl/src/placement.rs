//! Where things go in a window, and which pixel a pointer is over: the
//! display, the program's screen and the editor in the main window, the
//! emulator's windows over the editor's own, the menus when they go
//! apart, and the [`Mapping`] from a window's coordinates back to them.

use crate::{editor_layout, simple_ui};
use sb_runtime::editor::{Area, EditorSize};
use sb_runtime::font::FontSize;
use sb_runtime::screen::{Filter, ScreenMode};
use sb_runtime::settings::Settings;
use sdl3::render::FRect;

/// How a window's pointer positions map to the display's pixels and to
/// those of the frame shown in `shown`, which has `space` pixels.
#[derive(Clone, Copy)]
pub struct Mapping {
    /// Pixels to a window coordinate.
    pub px: f32,
    pub display: FRect,
    pub display_w: u32,
    pub shown: FRect,
    pub space: (u32, u32),
    /// The menus' top left corner in the window's pixels, where they go
    /// apart at 1x (see [`menu_place`]); `None` where they are on
    /// the display.
    pub menu: Option<(f32, f32)>,
}

impl Mapping {
    /// The menu bar's pixel under a window position: the display's, or
    /// the 1x one of menus apart.
    pub fn to_menu(self, x: f32, y: f32) -> (i32, i32) {
        match self.menu {
            Some((mx, my)) => (
                (x * self.px - mx).floor() as i32,
                (y * self.px - my).floor() as i32,
            ),
            None => self.to_display(x, y),
        }
    }

    pub fn to_display(self, x: f32, y: f32) -> (i32, i32) {
        let scale = self.display.w / self.display_w as f32;
        (
            ((x * self.px - self.display.x) / scale) as i32,
            ((y * self.px - self.display.y) / scale) as i32,
        )
    }

    pub fn to_screen(self, x: f32, y: f32) -> (i32, i32) {
        let (shown, (w, h)) = (self.shown, self.space);
        (
            ((x * self.px - shown.x) * w as f32 / shown.w).floor() as i32,
            ((y * self.px - shown.y) * h as f32 / shown.h).floor() as i32,
        )
    }

    /// The window coordinates of the middle of the screen's or the
    /// editor's pixel (x, y), which [`Mapping::to_screen`] maps back.
    pub fn screen_to_window(self, x: i32, y: i32) -> (f32, f32) {
        let (shown, (w, h)) = (self.shown, self.space);
        (
            (shown.x + (x as f32 + 0.5) * shown.w / w as f32) / self.px,
            (shown.y + (y as f32 + 0.5) * shown.h / h as f32) / self.px,
        )
    }
}

/// Where the display (emulator UI coordinates) goes in a window of
/// `out_w` x `out_h` pixels: scaled to fit, centered across and at the
/// top, so the menu bar stays at the window's top in a narrow window.
pub fn display_rect(width: u32, height: u32, out_w: u32, out_h: u32) -> FRect {
    let scale = (out_w as f32 / width as f32).min(out_h as f32 / height as f32);
    let (w, h) = (width as f32 * scale, height as f32 * scale);
    FRect::new((out_w as f32 - w) / 2.0, 0.0, w, h)
}

/// Where the display goes on the editor's own window for the emulator's
/// windows over it: centered, at the UI's scale `ui` or the largest whole
/// number below it that fits, or else scaled down to fit.
pub fn windows_rect((w, h): (u32, u32), (out_w, out_h): (u32, u32), ui: u32) -> FRect {
    let fits = (out_w / w.max(1)).min(out_h / h.max(1)).min(ui);
    let scale = match fits {
        0 => (out_w as f32 / w as f32).min(out_h as f32 / h as f32),
        n => n as f32,
    };
    let (sw, sh) = (w as f32 * scale, h as f32 * scale);
    let at = |out: u32, size: f32| ((out as f32 - size) / 2.0).floor();
    FRect::new(at(out_w, sw), at(out_h, sh), sw, sh)
}

/// A rectangle of `w` x `h` pixels from a window's top left corner.
pub fn from_corner((w, h): (u32, u32)) -> FRect {
    FRect::new(0.0, 0.0, w as f32, h as f32)
}

/// Where the screen goes in a window `out_w` pixels wide whose display
/// is `out_h` high: centered in the display, at XSCREEN's aspect ratio,
/// and scaled to fit, in every composite mode. The sharp choice in View >
/// Screen filter, `screen_filter`'s own Nearest, keeps to whole factors,
/// which SmileBASIC 4 does not do, where the screen fits once; in a
/// smaller window it is scaled down to fit, as the other choices are.
pub fn screen_rect(
    mode: &ScreenMode,
    screen_filter: Option<Filter>,
    out_w: u32,
    out_h: f32,
) -> FRect {
    let height = mode.height as f32;
    let width = height * mode.aspect() as f32;
    let fit = (out_w as f32 / width).min(out_h / height);
    let scale = match screen_filter {
        Some(Filter::Nearest) if fit >= 1.0 => fit.floor(),
        _ => fit,
    };
    let (w, h) = (width * scale, height * scale);
    FRect::new((out_w as f32 - w) / 2.0, (out_h - h) / 2.0, w, h)
}

/// Where the screen goes, as [`screen_rect`] says, and the filter it is
/// scaled with: View > Screen filter's on the desktop. The Simple UI
/// always fits the screen to the display and has a filter setting of its
/// own, as its display seldom holds the screen a whole number of times.
pub fn screen_place(
    mode: &ScreenMode,
    settings: &Settings,
    simple_ui: bool,
    out_w: u32,
    out_h: f32,
) -> (FRect, Filter) {
    if simple_ui {
        let rect = screen_rect(mode, None, out_w, out_h);
        let filter = simple_ui::screen_filter(settings.simple_ui_filter, mode, (rect.w, rect.h));
        return (rect, filter);
    }
    let chosen = settings.screen_filter;
    let rect = screen_rect(mode, chosen, out_w, out_h);
    // Nearest would leave out whole rows and columns of a screen shrunk
    // to fit a small window.
    match chosen.unwrap_or(mode.filter) {
        Filter::Nearest if rect.h < mode.height as f32 => (rect, Filter::Bilinear),
        filter => (rect, filter),
    }
}

/// How the Simple UI's editor frame, SmileBASIC 4's, is scaled into
/// `rect`: sharp when it fits a whole number of times, and otherwise with
/// the pixel art filter, unless the screen is filtered smoothly.
pub fn editor_filter(rect: FRect, (_, h): (u32, u32), filter: Filter) -> Filter {
    let scale = rect.h / h as f32;
    if (scale - scale.round()).abs() < 0.001 {
        Filter::Nearest
    } else if filter == Filter::Bilinear {
        Filter::Bilinear
    } else {
        Filter::SmartNearest
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
/// filter allows (see [`screen_place`]).
pub fn desktop_screen_place(
    mode: &ScreenMode,
    settings: &Settings,
    out: (u32, u32),
    display: FRect,
) -> (FRect, Filter) {
    let height = screen_height(display, out.1);
    screen_place(mode, settings, false, out.0, height)
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

    #[test]
    fn the_screen_fits_the_display_in_every_mode() {
        let mode = |width, height, aspect| ScreenMode {
            aspect,
            ..ScreenMode::new(width, height)
        };
        let box_of = |r: FRect| (r.x, r.y, r.w, r.h);
        let show = |m: &ScreenMode| box_of(screen_rect(m, None, 1280, 720.0));
        assert_eq!(show(&mode(400, 240, None)), (40.0, 0.0, 1200.0, 720.0));
        assert_eq!(show(&mode(1000, 600, None)), (40.0, 0.0, 1200.0, 720.0));
        assert_eq!(show(&mode(400, 240, Some(1.0))), (280.0, 0.0, 720.0, 720.0));
        // At other window sizes too.
        let tall = box_of(screen_rect(&mode(400, 240, None), None, 800, 900.0));
        assert_eq!(tall, (0.0, 210.0, 800.0, 480.0));
        // The menu's sharp choice keeps to whole factors.
        let sharp = screen_rect(&mode(1000, 600, None), Some(Filter::Nearest), 1280, 720.0);
        assert_eq!(box_of(sharp), (140.0, 60.0, 1000.0, 600.0));
        let sharp = screen_rect(&mode(400, 240, None), Some(Filter::Bilinear), 1000, 720.0);
        assert_eq!(box_of(sharp), (0.0, 60.0, 1000.0, 600.0));
        // A screen larger than the window is scaled down to fit it, as on
        // a 640x480 output, where the display is 640x360.
        let large = |w, h| screen_rect(&mode(w, h, None), Some(Filter::Nearest), 640, 360.0);
        assert_eq!(box_of(large(640, 480)), (80.0, 0.0, 480.0, 360.0));
        assert_eq!(box_of(large(1280, 720)), (0.0, 0.0, 640.0, 360.0));
        assert_eq!(box_of(large(400, 240)), (120.0, 60.0, 400.0, 240.0));
    }

    #[test]
    fn a_simple_ui_screen_fills_the_output() {
        use sb_runtime::settings::{SimpleUiFilter, SimpleUiSize};
        let mode = ScreenMode::new(400, 240);
        let box_of = |r: FRect| (r.x, r.y, r.w, r.h);
        let out = (640, 480);
        let ui = simple_ui::ui_size(out, SimpleUiSize::Normal);
        let display = simple_ui::display_rect(ui, out, SimpleUiSize::Normal);
        let settings = |simple_ui_filter| Settings {
            screen_filter: Some(Filter::Nearest),
            simple_ui_filter,
            ..Settings::default()
        };
        let place = |mode: &ScreenMode, chosen, simple_ui| {
            let (rect, filter) = screen_place(mode, &settings(chosen), simple_ui, out.0, display.h);
            (box_of(rect), filter)
        };
        // Whatever the filter, it fits at 1.6 times.
        for chosen in SimpleUiFilter::ALL {
            assert_eq!(place(&mode, chosen, true).0, (0.0, 48.0, 640.0, 384.0));
        }
        assert_eq!(
            place(&mode, SimpleUiFilter::Auto, true).1,
            Filter::SmartNearest
        );
        assert_eq!(
            place(&mode, SimpleUiFilter::Nearest, true).1,
            Filter::Nearest
        );
        assert_eq!(
            place(&mode, SimpleUiFilter::Xscreen, true).1,
            Filter::Bilinear
        );
        // On the desktop the sharp choice keeps to whole factors, and the
        // Simple UI's setting is not looked at.
        let desktop = ((120.0, 120.0, 400.0, 240.0), Filter::Nearest);
        for chosen in SimpleUiFilter::ALL {
            assert_eq!(place(&mode, chosen, false), desktop);
        }
        // On the desktop at 640x480 the display is 640x360, and a screen
        // shrunk to fit it is scaled smoothly, as Nearest would leave rows
        // out.
        let tall = ScreenMode::new(640, 480);
        let (rect, filter) =
            screen_place(&tall, &settings(SimpleUiFilter::Auto), false, 640, 360.0);
        assert_eq!(
            (box_of(rect), filter),
            ((80.0, 0.0, 480.0, 360.0), Filter::Bilinear)
        );
        let (rect, filter) =
            screen_place(&mode, &settings(SimpleUiFilter::Auto), false, 640, 360.0);
        assert_eq!(
            (box_of(rect), filter),
            ((120.0, 60.0, 400.0, 240.0), Filter::Nearest)
        );
        // A 4:3 mode fills the screen, at a whole factor; XSCREEN's
        // largest is half its size.
        let square = ScreenMode::new(320, 240);
        let full = ((0.0, 0.0, 640.0, 480.0), Filter::Nearest);
        assert_eq!(place(&square, SimpleUiFilter::Auto, true), full);
        let large = ScreenMode::new(1280, 720);
        let half = ((0.0, 60.0, 640.0, 360.0), Filter::Bilinear);
        assert_eq!(place(&large, SimpleUiFilter::Auto, true), half);
        // The UI's size leaves the program's screen as it is.
        let large_ui = simple_ui::ui_size(out, SimpleUiSize::Large);
        assert_eq!(large_ui, (320, 240));
        let large_display = simple_ui::display_rect(large_ui, out, SimpleUiSize::Large);
        assert_eq!(box_of(large_display), box_of(display));
        // Medium's UI goes past the output by part of a pixel, and the
        // screen stays on the output, where Normal puts it.
        for out in [(640, 480), (1280, 720), (1280, 800)] {
            let at = |size| {
                let display = simple_ui::display_rect(simple_ui::ui_size(out, size), out, size);
                let screen_h = display.h.min(out.1 as f32);
                let (rect, _) = screen_place(&mode, &Settings::default(), true, out.0, screen_h);
                box_of(rect)
            };
            assert_eq!(
                at(SimpleUiSize::Medium),
                at(SimpleUiSize::Normal),
                "{out:?}"
            );
        }
    }

    #[test]
    fn windows_over_the_editor_keep_whole_pixels_where_they_fit() {
        let box_of = |r: FRect| (r.x, r.y, r.w, r.h);
        let place = |out, ui| box_of(windows_rect((1280, 720), out, ui));
        assert_eq!(place((1500, 850), 1), (110.0, 65.0, 1280.0, 720.0));
        // No larger than the UI's scale, however large the window.
        assert_eq!(place((3000, 2000), 1), (860.0, 640.0, 1280.0, 720.0));
        assert_eq!(place((3000, 2000), 2), (220.0, 280.0, 2560.0, 1440.0));
        assert_eq!(place((2000, 1440), 3), (360.0, 360.0, 1280.0, 720.0));
        // Smaller than the display, they are scaled down to fit.
        assert_eq!(place((640, 720), 2), (0.0, 180.0, 640.0, 360.0));
    }

    /// The desktop's display in a window of `out` pixels.
    fn display(out: (u32, u32)) -> FRect {
        display_rect(1280, 720, out.0, out.1)
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
        let (r, filter) = desktop_screen_place(&mode, &with(filter), out, display(out));
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
                    let before = screen_place(&mode, &settings, false, out.0, screen_h);
                    let now = desktop_screen_place(&mode, &settings, out, display(out));
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
                    let map = Mapping {
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
                                desktop_screen_place(&mode, &with(filter), out, display(out)).0,
                                (w, h),
                            ),
                        };
                        let frame = fill.then_some(space);
                        // On a Retina display, two pixels to a coordinate.
                        for px in [1.0, 2.0] {
                            let map = Mapping {
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
        let (shown, _) =
            desktop_screen_place(&mode, &with(NEAREST), (640, 480), display((640, 480)));
        let map = Mapping {
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
}
