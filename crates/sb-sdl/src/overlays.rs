//! The layers the emulator draws over the screens and the editor, kept
//! together so that all of them can be told at once that the renderer
//! lost what they held.

use crate::layer::Layer;
use crate::pointer::DrawnCursor;

/// The emulator's UI over the screens or the editor, drawn at display
/// resolution. The first three are one for each window, the main one and
/// the editor's own; each is drawn and uploaded only as it changes.
pub struct Overlays<'a> {
    /// The debugger's panel.
    pub panel: [Layer<'a>; 2],
    /// The menu bar.
    pub bar: [Layer<'a>; 2],
    /// Everything else.
    pub overlay: [Layer<'a>; 2],
    /// The main window's open menus and context menu, where they go apart
    /// from its display at 1x (see `placement::menu_place`).
    pub menu: Layer<'a>,
    /// The emulator's windows on the editor's window, at the display's
    /// size.
    pub windows: Layer<'a>,
    /// The Simple UI's notices in the bottom right corner, at a scale of
    /// their own (see `simple_ui::notice_place`).
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

    /// Forgets what every texture holds, as when the renderer lost them:
    /// each is drawn and uploaded again when it is next shown.
    pub fn forget(&mut self) {
        let layers = self.panel.iter_mut().chain(&mut self.bar);
        layers.chain(&mut self.overlay).for_each(Layer::forget);
        self.windows.forget();
        self.menu.forget();
        self.notice.forget();
        self.cursor.forget();
    }
}
