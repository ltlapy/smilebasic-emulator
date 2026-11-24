//! The emulator's drawing over the screens and the editor: the UI
//! of each window, on a frame in the pixels of that window, and the
//! focus border on the window itself.

use sb_runtime::Framebuffer;
use sb_runtime::font::Font;
use sb_runtime::settings::Settings;
use sdl3::pixels::Color;
use sdl3::render::{BlendMode, Canvas, FRect};
use sdl3::video::Window;

use crate::debug_panel::sprites::Outline;
use crate::debug_panel::{self, DebugPanel, View};
use crate::keyhint::KeyHint;
use crate::menu::{Context, MenuBar};
use crate::popup::Popup;
use crate::program_dialog::ProgramView;
use crate::slow_hint::SlowHint;
use crate::toast::Toasts;
use crate::ui::{self, Rect};
use crate::{Beside, Choice, FileDialog, Screens, Side, corner};

/// The debugger's panel as an iteration shows it: where it is, what it
/// shows, and the size of the screens in it, if it shows them.
pub type Debug = Option<(Rect, View, Option<(u32, u32)>)>;
/// The chosen sprite's outline: the frame it is on, where that frame is
/// on this UI, and what it is clipped to.
pub type Outlined = Option<(Outline, (u32, u32), (f64, f64, f64, f64), Rect)>;

/// The UI of one window for an iteration of the loop: what is open, and
/// which parts are drawn on this window.
pub struct Ui<'a> {
    /// The window: 0 for the main window, 1 for the editor window. `ui`
    /// is the window with the menu bar and the emulator's windows, `ed`
    /// the window with the editor and its panels, and `side_on` the window
    /// with the help or the index.
    pub window: usize,
    pub ui: usize,
    pub ed: usize,
    pub side_on: usize,
    /// On this window, the emulator's windows are on a separate layer.
    pub windows_on_layer: bool,
    /// A dialog or a similar window dims what is behind it.
    pub dim: bool,
    /// The place of the debugger's panel on this window, if it is there.
    pub panel: Option<Rect>,
    pub debug: &'a Debug,
    pub outline: &'a Outlined,
    /// The corner's notices show on this window.
    pub notices: bool,
    pub key_hint: &'a KeyHint,
    pub slow_hint: &'a SlowHint,
    pub side: &'a Beside,
    pub screens: &'a Screens,
    pub program_dialog: &'a Option<ProgramView>,
    /// The menu a right click opened on the editor or the panel.
    pub popup: &'a Option<Popup<Choice>>,
    pub toasts: &'a Toasts,
    pub menu: &'a MenuBar,
    pub context: Context,
    /// The menu bar shows on this window; its menus are separate from
    /// the display, on their own layer; and the place of the bar here.
    pub bar_shown: bool,
    pub menus_apart: bool,
    pub bar: Rect,
    pub font: &'a Font,
    pub settings: &'a Settings,
    /// The debugger's text size.
    pub scale: u32,
    /// The text cursors' blink.
    pub cursor_on: bool,
}

impl Ui<'_> {
    /// Draws all of the UI on `fb`, but when the emulator's windows are on
    /// a separate layer, draws those if `windows` and the rest if not.
    /// `inside` is whether the debugger's panel is drawn with the UI
    /// rather than on a separate layer.
    pub fn draw(
        &self,
        fb: &mut Framebuffer,
        debug_panel: &mut DebugPanel,
        inside: bool,
        windows: bool,
    ) {
        let (i, ui, font, settings) = (self.window, self.ui, self.font, self.settings);
        let (scale, cursor_on) = (self.scale, self.cursor_on);
        let under = !windows || !self.windows_on_layer;
        let over = windows || !self.windows_on_layer;
        if self.dim && under {
            ui::clear(fb, 0x8000_0000);
            if let (Some(panel), false) = (self.panel, inside) {
                ui::fill(fb, panel, 0);
            }
        }
        if let (true, Some((place, view, size))) = (under && inside, self.debug) {
            debug_panel.draw_on_display(fb, font, *place, *size, view, scale);
        }
        if self.panel.is_some() && under {
            debug_panel.draw_tip(fb, font);
        }
        if let (0, Some((outline, frame, place, clip))) = (i, self.outline) {
            debug_panel::sprites::draw_outline(fb, outline, *frame, *place, *clip, 2);
        }
        // The notices are drawn under the rest of the emulator's UI.
        // They are not drawn while its screens are open, because they
        // would only be in the way. The notices of the Simple UI are
        // on a separate layer.
        if self.notices {
            corner::draw(fb, font, self.key_hint, self.slow_hint, false);
        }
        match &self.side.open {
            _ if i != self.side_on || !under => {}
            Some(Side::Help(help)) => help.draw(fb, font, self.side.focus),
            Some(Side::Index(index, _)) => index.draw(fb, font, self.side.focus),
            None => {}
        }
        // Windows over the editor also cover its panels.
        if let (true, Some(open)) = (i == ui && over, &self.screens.manager) {
            open.draw(fb, font, cursor_on);
        }
        if let (true, Some(open)) = (i == ui && over, &self.screens.game_list) {
            open.draw(fb, font);
        }
        if let (true, Some(open)) = (i == ui && over, &self.screens.welcome) {
            open.draw(fb, font);
        }
        if let (0, Some(open)) = (i, self.program_dialog) {
            open.draw(fb, font, cursor_on);
        }
        if i == ui && over {
            if let Some(open) = &self.screens.color_window {
                open.draw(fb, font, &settings.editor_colors);
            }
            if let Some(open) = &self.screens.settings_window {
                open.draw(fb, font, settings);
            }
            if let Some(open) = &self.screens.shortcuts_window {
                open.draw(fb, font);
            }
            match &self.screens.file_dialog {
                Some(FileDialog::Save(d)) => d.draw(fb, font, cursor_on),
                Some(FileDialog::Load(p)) => p.draw(fb, font, cursor_on),
                None => {}
            }
        }
        if let (true, Some(open)) = (i == self.ed && under && !self.menus_apart, self.popup) {
            open.draw(fb, font);
        }
        if i == ui && under {
            self.toasts.draw(fb, font);
        }
        if i == ui {
            match &self.screens.dialog {
                Some((open, _)) if over => open.draw(fb, font),
                None if self.bar_shown && under && !self.menus_apart => {
                    self.menu.draw_lists(fb, font, settings, self.context)
                }
                _ => {}
            }
        }
        if let (true, Some(open)) = (i == ui && over, &self.screens.pause_menu) {
            open.draw(fb, font);
        }
        // The bar covers what is under it.
        if self.dim && under && self.bar_shown {
            ui::fill(fb, self.bar, 0);
        }
    }
}

/// Shows where the focus has just moved: a yellow line on the edge of the
/// rectangle that `place` gives (see [`crate::placement::border_place`]),
/// one pixel outside and two inside, then a dark band two pixels wide.
/// Both are drawn at `alpha`, so that they fade out together.
pub fn focus_border(
    canvas: &mut Canvas<Window>,
    place: ([i32; 4], (f32, f32), (f32, f32)),
    alpha: u8,
) -> Result<(), sdl3::Error> {
    let ([x, y, w, h], (ox, oy), (sx, sy)) = place;
    let place = |x: i32, y: i32, w: i32, h: i32| {
        let at = |v: i32, s: f32| v as f32 * s;
        FRect::new(ox + at(x, sx), oy + at(y, sy), at(w, sx), at(h, sy))
    };
    canvas.set_blend_mode(BlendMode::Blend);
    for (inset, width, color) in [
        (-1, 3, Color::RGBA(255, 255, 0, alpha)),
        (2, 2, Color::RGBA(0, 0, 0, alpha)),
    ] {
        let (x, y, w, h) = (x + inset, y + inset, w - 2 * inset, h - 2 * inset);
        canvas.set_draw_color(color);
        canvas.fill_rects(&[
            place(x, y, w, width),
            place(x, y + h - width, w, width),
            place(x, y + width, width, h - 2 * width),
            place(x + w - width, y + width, width, h - 2 * width),
        ])?;
    }
    canvas.set_blend_mode(BlendMode::None);
    Ok(())
}
