//! The layers the emulator draws over the screens and the editor, kept
//! together so that all of them can be told at the same time that the
//! renderer lost their contents.

use sdl3::render::{Canvas, FRect};
use sdl3::video::Window;

use crate::layer::Layer;
use crate::placement::MenuPlace;
use crate::pointer::DrawnCursor;

type AnyError = Box<dyn std::error::Error>;

/// The emulator's UI over the screens or the editor, drawn at display
/// resolution. The first three have one layer for each window: the main
/// window and the editor's own window. Each layer is drawn and uploaded
/// only when it changes.
pub struct Overlays<'a> {
    /// The debugger's panel.
    pub panel: [Layer<'a>; 2],
    /// The menu bar.
    pub bar: [Layer<'a>; 2],
    /// Everything else.
    pub overlay: [Layer<'a>; 2],
    /// The main window's open menus and context menu, when they are placed
    /// apart from its display at 1x (see `placement::menu_place`).
    pub menu: Layer<'a>,
    /// The emulator's windows on the editor window, at the display size.
    pub windows: Layer<'a>,
    /// The Simple UI's notices in the bottom right corner, at their own
    /// scale (see `simple_ui::notice_place`).
    pub notice: Layer<'a>,
    /// The cursor drawn where the system shows none.
    pub cursor: DrawnCursor<'a>,
}

impl<'a> Overlays<'a> {
    pub fn new(cursor: DrawnCursor<'a>) -> Overlays<'a> {
        Overlays {
            panel: Default::default(),
            bar: Default::default(),
            overlay: Default::default(),
            menu: Layer::default(),
            windows: Layer::default(),
            notice: Layer::default(),
            cursor,
        }
    }

    /// Forgets the contents of every texture after the renderer lost them.
    /// Each is drawn and uploaded again when it is next shown.
    pub fn forget(&mut self) {
        let layers = self.panel.iter_mut().chain(&mut self.bar);
        layers.chain(&mut self.overlay).for_each(Layer::forget);
        self.windows.forget();
        self.menu.forget();
        self.notice.forget();
        self.cursor.forget();
    }

    /// Copies the main window's layers to `canvas`, from the lowest up:
    /// the debugger's panel, the Simple UI's notices, the rest of the UI,
    /// the menu bar and the menus placed apart.
    pub fn copy_main(
        &mut self,
        canvas: &mut Canvas<Window>,
        at: MainPlaces,
    ) -> Result<(), AnyError> {
        let (display, display_w) = (at.display, at.display_w);
        if let Some(place) = at.panel {
            self.panel[0].copy(canvas, place, display, display_w)?;
        }
        if let Some((place, width)) = at.notice {
            self.notice.copy(canvas, (0, 0), place, width)?;
        }
        self.overlay[0].copy(canvas, (0, 0), display, display_w)?;
        match (at.bar, at.menus) {
            (true, Some(place)) => {
                self.bar[0].copy(canvas, (0, 0), place.rect(), place.size.0)?;
            }
            (true, None) => self.bar[0].copy(canvas, (0, 0), display, display_w)?,
            (false, _) => {}
        }
        if let Some(place) = at.menus {
            self.menu.copy(canvas, (0, 0), place.rect(), place.size.0)?;
        }
        Ok(())
    }

    /// Copies the layers of the editor's own window to `canvas`, from the
    /// lowest up: the debugger's panel, the rest of the UI, the menu bar
    /// and the emulator's windows.
    pub fn copy_editor(
        &mut self,
        canvas: &mut Canvas<Window>,
        at: EditorPlaces,
    ) -> Result<(), AnyError> {
        let (display, ui_w) = (at.display, at.ui_w);
        if let Some(place) = at.panel {
            self.panel[1].copy(canvas, place, display, ui_w)?;
        }
        self.overlay[1].copy(canvas, (0, 0), display, ui_w)?;
        if at.bar {
            self.bar[1].copy(canvas, (0, 0), display, ui_w)?;
        }
        if let Some((place, width)) = at.windows {
            self.windows.copy(canvas, (0, 0), place, width)?;
        }
        Ok(())
    }
}

/// The places of the layers on the editor's own window; see
/// [`Overlays::copy_editor`].
pub struct EditorPlaces {
    /// The UI on the window, and its width in its own pixels.
    pub display: FRect,
    pub ui_w: u32,
    /// The corner of the debugger's panel, when the panel has its own
    /// layer.
    pub panel: Option<(u32, u32)>,
    /// The menu bar shows.
    pub bar: bool,
    /// The place of the emulator's windows and the width of their layer,
    /// while they are open on this window.
    pub windows: Option<(FRect, u32)>,
}

/// The places of the layers on the main window; see [`Overlays::copy_main`].
pub struct MainPlaces {
    /// The display on the window, and its width in the UI's pixels.
    pub display: FRect,
    pub display_w: u32,
    /// The corner of the debugger's panel on the display, when the panel
    /// has its own layer.
    pub panel: Option<(u32, u32)>,
    /// The place of the Simple UI's notices and the width of their layer,
    /// while they show.
    pub notice: Option<(FRect, u32)>,
    /// The menu bar shows.
    pub bar: bool,
    /// The place of the menus, when they are placed apart from the
    /// display at 1x.
    pub menus: Option<MenuPlace>,
}
