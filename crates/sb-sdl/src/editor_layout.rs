//! How the editor's own window is laid out. The editor's frame and the
//! emulator's UI over it (the menu bar, the debugger's panel, the help
//! and the index) each have pixels that are a whole number of the
//! window's, so the dot fonts stay sharp, and a larger window shows more
//! rows and columns instead of larger pixels.
//!
//! The UI's scale follows the desktop's scaling setting, rounded to a
//! whole number. The editor's characters are drawn at the whole-number
//! zoom nearest the size F2 chose, as that size looks on a display of
//! SmileBASIC 4's height at that scale.

use sb_runtime::editor::{Area, EditorSize, frame_size};
use sb_runtime::font::FontSize;

/// The smallest the UI gets, in its own pixels: room for the debugger's
/// tabs, at SmileBASIC 4's shape. The window cannot be made smaller than
/// this at its scale.
pub const MIN_UI: (u32, u32) = (800, 450);

/// The largest UI scale taken from the desktop.
const MAX_SCALE: u32 = 8;

/// The UI scale for a window with SDL's display scale: the desktop's
/// scaling setting times the window's pixel density, rounded to the
/// nearest whole number, 150% to 2.
pub fn ui_scale(display_scale: f32) -> u32 {
    if !display_scale.is_finite() {
        return 1;
    }
    (display_scale.round().max(1.0) as u32).min(MAX_SCALE)
}

/// The window's smallest size, in its own coordinates, that leaves the
/// UI [`MIN_UI`] at `scale`. Windows and X11 give the size in pixels,
/// macOS and Wayland in points of `density` pixels each.
pub fn min_window(scale: u32, density: f32) -> (u32, u32) {
    let density = if density.is_finite() && density > 0.0 {
        density
    } else {
        1.0
    };
    let fit = |v: u32| (v as f32 * scale as f32 / density).ceil() as u32;
    (fit(MIN_UI.0), fit(MIN_UI.1))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Layout {
    /// Window pixels to a pixel of the UI.
    pub ui: u32,
    /// The UI's size in its pixels.
    pub ui_size: (u32, u32),
    /// Window pixels to a pixel of the editor's frame.
    pub zoom: u32,
    /// The editor's frame, which fills the window from its top left
    /// corner, and the part of it under the menu bar.
    pub area: Area,
}

impl Layout {
    /// The layout for a window of `pixels` at the UI scale `scale`, with
    /// the editor's `font` at `size`. `display` is SmileBASIC 4's display
    /// in UI pixels, and `bar` the menu bar's height in them.
    pub fn new(
        pixels: (u32, u32),
        scale: u32,
        (font, size): (FontSize, EditorSize),
        display: (u32, u32),
        bar: u32,
    ) -> Layout {
        let (w, h) = (pixels.0.max(1), pixels.1.max(1));
        // A window the system made smaller than its minimum, as some
        // tiling window managers do, takes a smaller scale.
        let fits = |v: u32, least: u32| (v / least).max(1);
        let ui = scale.clamp(1, MAX_SCALE);
        let ui = ui.min(fits(w, MIN_UI.0)).min(fits(h, MIN_UI.1));
        let ui_size = ((w / ui).max(1), (h / ui).max(1));
        // The zoom at which F2's size shows about SmileBASIC 4's rows on
        // a display of its height.
        let rows = frame_size(display, font, size).1.max(1);
        let zoom = ((display.1 * ui) as f32 / rows as f32).round().max(1.0) as u32;
        let area = Area {
            size: (w.div_ceil(zoom), h.div_ceil(zoom)),
            top: (bar * ui).div_ceil(zoom),
        };
        Layout {
            ui,
            ui_size,
            zoom,
            area,
        }
    }

    /// The UI's rectangle in window pixels: `(w, h)`, from the top left
    /// corner.
    pub fn ui_pixels(&self) -> (u32, u32) {
        (self.ui_size.0 * self.ui, self.ui_size.1 * self.ui)
    }

    /// The editor's frame in window pixels, from the top left corner. It
    /// can go past the window by less than a pixel of the frame.
    pub fn frame_pixels(&self) -> (u32, u32) {
        (self.area.size.0 * self.zoom, self.area.size.1 * self.zoom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DISPLAY: (u32, u32) = (1280, 720);
    const BAR: u32 = 24;
    const LARGE: (FontSize, EditorSize) = (FontSize::Large, EditorSize::Large);

    fn layout(pixels: (u32, u32), scale: u32, look: (FontSize, EditorSize)) -> Layout {
        Layout::new(pixels, scale, look, DISPLAY, BAR)
    }

    #[test]
    fn the_desktop_scale_rounds_to_a_whole_number() {
        let scales = [1.0, 1.25, 1.49, 1.5, 1.75, 2.0, 3.0, 0.5, 0.0, 100.0];
        let got: Vec<u32> = scales.iter().map(|&s| ui_scale(s)).collect();
        assert_eq!(got, [1, 1, 1, 2, 2, 2, 3, 1, 1, MAX_SCALE]);
        assert_eq!(ui_scale(f32::NAN), 1);
        assert_eq!(ui_scale(f32::INFINITY), 1);
    }

    #[test]
    fn a_larger_window_shows_more_rows_at_the_same_zoom() {
        let small = layout((1280, 720), 1, LARGE);
        assert_eq!((small.zoom, small.area.size), (2, (640, 360)));
        // Twice the menu bar's 24 pixels at the UI's scale, in the
        // frame's pixels.
        assert_eq!(small.area.top, 12);
        assert_eq!(small.ui_size, DISPLAY);
        let large = layout((1920, 1200), 1, LARGE);
        assert_eq!((large.zoom, large.area.size), (2, (960, 600)));
        assert_eq!(large.ui_size, (1920, 1200));
    }

    #[test]
    fn the_frame_covers_the_window() {
        let odd = layout((1281, 721), 1, LARGE);
        assert_eq!(odd.area.size, (641, 361));
        let (w, h) = odd.frame_pixels();
        assert!(w >= 1281 && h >= 721 && w - 1281 < odd.zoom && h - 721 < odd.zoom);
        let (w, h) = odd.ui_pixels();
        assert!(w <= 1281 && h <= 721);
    }

    #[test]
    fn a_scaled_desktop_shows_what_an_unscaled_one_does() {
        // A 2x display: macOS and Wayland give twice the pixels of the
        // window's size.
        let one = layout((1280, 720), 1, LARGE);
        let two = layout((2560, 1440), 2, LARGE);
        assert_eq!((two.ui, two.ui_size), (2, one.ui_size));
        assert_eq!((two.zoom, two.area), (4, one.area));
        // 150% rounds to 2.
        let wide = layout((1920, 1080), ui_scale(1.5), LARGE);
        assert_eq!((wide.ui, wide.ui_size), (2, (960, 540)));
    }

    #[test]
    fn each_size_takes_the_nearest_whole_zoom() {
        let zooms = |font| {
            [
                EditorSize::Large,
                EditorSize::Medium,
                EditorSize::Small,
                EditorSize::Tiny,
            ]
            .map(|size| layout((1280, 720), 1, (font, size)).zoom)
        };
        assert_eq!(zooms(FontSize::Small), [4, 3, 2, 1]);
        // The 16-pixel font cannot be drawn smaller than its pixels.
        assert_eq!(zooms(FontSize::Large), [2, 1, 1, 1]);
    }

    #[test]
    fn a_window_below_its_minimum_takes_a_smaller_scale() {
        let squeezed = layout((1000, 600), 2, LARGE);
        assert_eq!((squeezed.ui, squeezed.ui_size), (1, (1000, 600)));
        let tiny = layout((0, 0), 3, LARGE);
        assert_eq!(tiny.ui, 1);
        assert!(tiny.area.size.0 >= 1 && tiny.area.size.1 >= 1);
        assert!(tiny.ui_size.0 >= 1 && tiny.ui_size.1 >= 1);
    }

    #[test]
    fn the_minimum_window_holds_the_smallest_ui() {
        assert_eq!(min_window(1, 1.0), MIN_UI);
        // Points on a 2x display, pixels on Windows at 200%.
        assert_eq!(min_window(2, 2.0), MIN_UI);
        assert_eq!(min_window(2, 1.0), (1600, 900));
        // Wayland at 150% has 1.5 pixels to a point and a UI scale of 2.
        assert_eq!(min_window(2, 1.5), (1067, 600));
        assert_eq!(min_window(1, 0.0), MIN_UI);
    }
}
