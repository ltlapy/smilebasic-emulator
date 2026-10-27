//! The emulator's menu bar, drawn over the screen at display resolution.
//! It stays hidden until the mouse reaches the top edge, so it takes no
//! keys away from programs.

use sb_runtime::Framebuffer;
use sb_runtime::confirm::FileConfirm;
use sb_runtime::font::Font;
use sb_runtime::runtime::Settings;
use sb_runtime::screen::Filter;

use crate::ui::{
    BACKGROUND, BORDER, DISABLED, HIGHLIGHT, PAD, Rect, TEXT, draw_text, fill, outline, text_width,
};

const ROW_HEIGHT: u32 = 24;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Projects,
    OpenFolder,
    Quit,
    Stop,
    ToggleEditor,
    EditSlot(usize),
    ToggleUpperCase,
    ToggleCtrlC,
    ToggleHalfWidth,
    ToggleWrap,
    ToggleNumberTiles,
    ToggleKeyboardButtons,
    FileAccess(FileConfirm),
    ScreenFilter(Option<Filter>),
    /// Use the bundled SoundFont.
    BundledSoundFont,
    /// Pick a SoundFont file.
    ChooseSoundFont,
}

impl Action {
    /// The mark before an item: a check box for toggles, a radio button
    /// for choices, blank for commands.
    fn mark(self, settings: &Settings) -> &'static str {
        let check = |on: bool| if on { "[x]" } else { "[ ]" };
        match self {
            Action::ToggleUpperCase => check(settings.upper_case_letters),
            Action::ToggleCtrlC => check(settings.ctrl_c_stops),
            Action::ToggleHalfWidth => check(settings.half_width_on_save),
            Action::ToggleWrap => check(settings.editor_wrap),
            Action::ToggleNumberTiles => check(settings.editor_number_tiles),
            Action::ToggleKeyboardButtons => check(settings.keyboard_buttons),
            Action::FileAccess(mode) if settings.file_confirm == mode => "(*)",
            Action::FileAccess(_) => "( )",
            Action::ScreenFilter(f) if settings.screen_filter == f => "(*)",
            Action::ScreenFilter(_) => "( )",
            Action::BundledSoundFont if settings.sound_font.is_none() => "(*)",
            Action::ChooseSoundFont if settings.sound_font.is_some() => "(*)",
            Action::BundledSoundFont | Action::ChooseSoundFont => "( )",
            Action::Projects
            | Action::OpenFolder
            | Action::Quit
            | Action::Stop
            | Action::ToggleEditor
            | Action::EditSlot(_) => "   ",
        }
    }

    /// Flips the setting of a toggle, or picks a choice.
    pub fn toggle(self, settings: &mut Settings) {
        let flag = match self {
            Action::ToggleUpperCase => &mut settings.upper_case_letters,
            Action::ToggleCtrlC => &mut settings.ctrl_c_stops,
            Action::ToggleHalfWidth => &mut settings.half_width_on_save,
            Action::ToggleWrap => &mut settings.editor_wrap,
            Action::ToggleNumberTiles => &mut settings.editor_number_tiles,
            Action::ToggleKeyboardButtons => &mut settings.keyboard_buttons,
            Action::FileAccess(mode) => {
                settings.file_confirm = mode;
                return;
            }
            Action::ScreenFilter(f) => {
                settings.screen_filter = f;
                return;
            }
            Action::BundledSoundFont => {
                settings.sound_font = None;
                return;
            }
            Action::ChooseSoundFont
            | Action::Projects
            | Action::OpenFolder
            | Action::Quit
            | Action::Stop
            | Action::ToggleEditor
            | Action::EditSlot(_) => return,
        };
        *flag = !*flag;
    }

    /// Stop needs a running program; the editor and the project manager
    /// cannot open during one.
    fn enabled(self, running: bool) -> bool {
        match self {
            Action::Stop => running,
            Action::ToggleEditor | Action::EditSlot(_) | Action::Projects => !running,
            _ => true,
        }
    }
}

enum Entry {
    Do(Action),
    /// Opens a submenu to the right.
    Sub(&'static [Item]),
}

struct Item {
    label: &'static str,
    entry: Entry,
}

const fn item(label: &'static str, action: Action) -> Item {
    Item {
        label,
        entry: Entry::Do(action),
    }
}

struct Menu {
    title: &'static str,
    items: &'static [Item],
}

const MENUS: &[Menu] = &[
    Menu {
        title: "SmileBASIC",
        items: &[
            item("Projects...", Action::Projects),
            item("Open projects folder", Action::OpenFolder),
            item("Quit", Action::Quit),
        ],
    },
    Menu {
        title: "Program",
        items: &[
            item("Stop (Pause/Break)", Action::Stop),
            item("Editor/Direct (F4)", Action::ToggleEditor),
            Item {
                label: "Edit slot",
                entry: Entry::Sub(&[
                    item("PRG0 (Ctrl+1)", Action::EditSlot(0)),
                    item("PRG1 (Ctrl+2)", Action::EditSlot(1)),
                    item("PRG2 (Ctrl+3)", Action::EditSlot(2)),
                    item("PRG3 (Ctrl+4)", Action::EditSlot(3)),
                    item("PRG4 (Ctrl+5)", Action::EditSlot(4)),
                    item("PRG5 (Ctrl+6)", Action::EditSlot(5)),
                ]),
            },
        ],
    },
    Menu {
        title: "Options",
        items: &[
            item("Upper-case letters", Action::ToggleUpperCase),
            item("Ctrl+C stops program", Action::ToggleCtrlC),
            item("Half-width on SAVE", Action::ToggleHalfWidth),
            item("Wrap lines in editor (Ctrl+W)", Action::ToggleWrap),
            item("Line number tiles in editor", Action::ToggleNumberTiles),
            Item {
                label: "File access",
                entry: Entry::Sub(&[
                    item("Always ask", Action::FileAccess(FileConfirm::Ask)),
                    item("Notify", Action::FileAccess(FileConfirm::Notify)),
                    item("Silent", Action::FileAccess(FileConfirm::Silent)),
                ]),
            },
            Item {
                label: "Screen filter",
                entry: Entry::Sub(&[
                    item("Program (XSCREEN)", Action::ScreenFilter(None)),
                    item("Nearest", Action::ScreenFilter(Some(Filter::Nearest))),
                    item(
                        "SmartNearest",
                        Action::ScreenFilter(Some(Filter::SmartNearest)),
                    ),
                    item("Bilinear", Action::ScreenFilter(Some(Filter::Bilinear))),
                ]),
            },
            item("Keys press buttons", Action::ToggleKeyboardButtons),
            Item {
                label: "Sound font",
                entry: Entry::Sub(&[
                    item("GeneralUser GS (bundled)", Action::BundledSoundFont),
                    item("Choose a file...", Action::ChooseSoundFont),
                ]),
            },
        ],
    },
];

/// What a click did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Click {
    /// The click was not on the menu bar; pass it on.
    Ignored,
    /// The menu bar used the click.
    Consumed,
    Chosen(Action),
}

/// An item's text: a check box or radio button for settings, an arrow
/// for submenus, or the same width of space otherwise.
fn item_text(item: &Item, settings: &Settings) -> String {
    match item.entry {
        Entry::Do(action) => format!("{} {}", action.mark(settings), item.label),
        Entry::Sub(_) => format!("    {} >", item.label),
    }
}

fn title_rect(index: usize) -> Rect {
    let x = MENUS[..index]
        .iter()
        .map(|m| text_width(m.title) + 2 * PAD)
        .sum::<u32>();
    Rect {
        x,
        y: 0,
        w: text_width(MENUS[index].title) + 2 * PAD,
        h: ROW_HEIGHT,
    }
}

/// The width of a dropdown holding `items`.
fn list_width(items: &[Item]) -> u32 {
    let settings = Settings::default();
    items
        .iter()
        .map(|i| text_width(&item_text(i, &settings)))
        .max()
        .unwrap_or(0)
        + 2 * PAD
}

fn item_rect(menu: usize, item: usize) -> Rect {
    Rect {
        x: title_rect(menu).x,
        y: ROW_HEIGHT * (1 + item as u32),
        w: list_width(MENUS[menu].items),
        h: ROW_HEIGHT,
    }
}

/// An item of the submenu that item `parent` of `menu` opens.
fn sub_rect(menu: usize, parent: usize, item: usize) -> Rect {
    let p = item_rect(menu, parent);
    let Entry::Sub(items) = MENUS[menu].items[parent].entry else {
        unreachable!("only submenu items have submenus")
    };
    Rect {
        x: p.x + p.w,
        y: p.y + ROW_HEIGHT * item as u32,
        w: list_width(items),
        h: ROW_HEIGHT,
    }
}

/// The rectangle around a list of `len` rows starting at `first`.
fn list_rect(first: Rect, len: usize) -> Rect {
    Rect {
        h: ROW_HEIGHT * len as u32,
        ..first
    }
}

#[derive(Debug, Default)]
pub struct MenuBar {
    /// Mouse position in display pixels, if it is over the window.
    mouse: Option<(i32, i32)>,
    open: Option<usize>,
    /// The item of the open menu whose submenu is open.
    sub: Option<usize>,
    /// The bar stays shown, as over the editor, which leaves room for it.
    pub pinned: bool,
}

impl MenuBar {
    pub fn visible(&self) -> bool {
        self.pinned
            || self.open.is_some()
            || self
                .mouse
                .is_some_and(|(_, y)| y >= 0 && y < ROW_HEIGHT as i32)
    }

    /// The submenu items, with the open menu and parent item.
    fn submenu(&self) -> Option<(usize, usize, &'static [Item])> {
        let (menu, parent) = (self.open?, self.sub?);
        match MENUS[menu].items[parent].entry {
            Entry::Sub(items) => Some((menu, parent, items)),
            Entry::Do(_) => None,
        }
    }

    /// Hovering an item of the open menu opens its submenu, or closes
    /// another one.
    pub fn mouse_move(&mut self, position: Option<(i32, i32)>) {
        self.mouse = position;
        let (Some(menu), Some(at)) = (self.open, position) else {
            return;
        };
        if let Some(i) = (0..MENUS[menu].items.len()).find(|&i| item_rect(menu, i).contains(at)) {
            self.sub = matches!(MENUS[menu].items[i].entry, Entry::Sub(_)).then_some(i);
        }
    }

    fn close(&mut self) {
        self.open = None;
        self.sub = None;
    }

    /// A left click at a display position. `running` tells whether a
    /// program is running, which enables Stop and disables the editor.
    pub fn click(&mut self, at: (i32, i32), running: bool) -> Click {
        if let Some((menu, parent, items)) = self.submenu() {
            for (i, item) in items.iter().enumerate() {
                if let Entry::Do(action) = item.entry
                    && sub_rect(menu, parent, i).contains(at)
                {
                    if !action.enabled(running) {
                        return Click::Consumed;
                    }
                    self.close();
                    return Click::Chosen(action);
                }
            }
        }
        if let Some(menu) = self.open {
            for (i, item) in MENUS[menu].items.iter().enumerate() {
                if !item_rect(menu, i).contains(at) {
                    continue;
                }
                return match item.entry {
                    Entry::Do(action) if !action.enabled(running) => Click::Consumed,
                    Entry::Do(action) => {
                        self.close();
                        Click::Chosen(action)
                    }
                    Entry::Sub(_) => {
                        self.sub = Some(i);
                        Click::Consumed
                    }
                };
            }
        }
        if !self.visible() && self.open.is_none() {
            return Click::Ignored;
        }
        let title = (0..MENUS.len()).find(|&i| title_rect(i).contains(at));
        let was_open = self.open;
        self.close();
        match title {
            Some(i) => {
                self.open = (was_open != Some(i)).then_some(i);
                Click::Consumed
            }
            // A click outside closes an open menu without reaching the program.
            None if was_open.is_some() => Click::Consumed,
            None if at.1 >= 0 && at.1 < ROW_HEIGHT as i32 => Click::Consumed,
            None => Click::Ignored,
        }
    }

    /// Draws the bar, and the open menus, onto a transparent overlay.
    pub fn draw(&self, fb: &mut Framebuffer, font: &Font, settings: &Settings, running: bool) {
        let bar = Rect {
            x: 0,
            y: 0,
            w: fb.width(),
            h: ROW_HEIGHT,
        };
        fill(fb, bar, BACKGROUND);
        // A line under the bar sets it apart from a dark screen.
        fill(
            fb,
            Rect {
                y: ROW_HEIGHT - 2,
                h: 2,
                ..bar
            },
            BORDER,
        );
        for (i, menu) in MENUS.iter().enumerate() {
            let r = title_rect(i);
            if self.open == Some(i) || self.mouse.is_some_and(|m| r.contains(m)) {
                fill(fb, Rect { h: r.h - 2, ..r }, HIGHLIGHT);
            }
            draw_text(fb, font, r.x + PAD, r.y + 4, menu.title, TEXT);
        }
        let Some(open) = self.open else { return };
        let items = MENUS[open].items;
        let rects: Vec<Rect> = (0..items.len()).map(|i| item_rect(open, i)).collect();
        self.draw_list(fb, font, settings, running, items, &rects, self.sub);
        if let Some((menu, parent, sub)) = self.submenu() {
            let rects: Vec<Rect> = (0..sub.len()).map(|i| sub_rect(menu, parent, i)).collect();
            self.draw_list(fb, font, settings, running, sub, &rects, None);
        }
    }

    /// Draws a dropdown list with a border; `selected` stays highlighted
    /// while its submenu is open.
    #[allow(clippy::too_many_arguments)]
    fn draw_list(
        &self,
        fb: &mut Framebuffer,
        font: &Font,
        settings: &Settings,
        running: bool,
        items: &[Item],
        rects: &[Rect],
        selected: Option<usize>,
    ) {
        for (i, (item, &r)) in items.iter().zip(rects).enumerate() {
            let hovered = self.mouse.is_some_and(|m| r.contains(m)) || selected == Some(i);
            let enabled = match item.entry {
                Entry::Do(action) => action.enabled(running),
                Entry::Sub(_) => true,
            };
            let back = if hovered && enabled {
                HIGHLIGHT
            } else {
                BACKGROUND
            };
            fill(fb, r, back);
            let color = if enabled { TEXT } else { DISABLED };
            draw_text(
                fb,
                font,
                r.x + PAD,
                r.y + 4,
                &item_text(item, settings),
                color,
            );
        }
        if let Some(&first) = rects.first() {
            outline(fb, list_rect(first, rects.len()), BORDER);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hidden_until_the_mouse_reaches_the_top() {
        let mut bar = MenuBar::default();
        assert!(!bar.visible());
        assert_eq!(bar.click((10, 5), false), Click::Ignored);
        bar.mouse_move(Some((10, 300)));
        assert!(!bar.visible());
        bar.mouse_move(Some((10, 5)));
        assert!(bar.visible());
    }

    #[test]
    fn open_menu_and_choose() {
        let mut bar = MenuBar::default();
        bar.mouse_move(Some((10, 5)));
        let options = title_rect(2);
        let at = |r: Rect| ((r.x + 1) as i32, (r.y + 1) as i32);
        assert_eq!(bar.click(at(options), false), Click::Consumed);
        bar.mouse_move(Some((500, 500)));
        assert!(bar.visible(), "an open menu stays visible");
        assert_eq!(
            bar.click(at(item_rect(2, 1)), false),
            Click::Chosen(Action::ToggleCtrlC)
        );
        assert!(!bar.visible());

        let mut settings = Settings::default();
        Action::ToggleCtrlC.toggle(&mut settings);
        assert!(settings.ctrl_c_stops);
    }

    #[test]
    fn stop_needs_a_running_program() {
        let mut bar = MenuBar::default();
        bar.mouse_move(Some((10, 5)));
        let at = |r: Rect| ((r.x + 1) as i32, (r.y + 1) as i32);
        bar.click(at(title_rect(1)), false);
        assert_eq!(bar.click(at(item_rect(1, 0)), false), Click::Consumed);
        assert_eq!(
            bar.click(at(item_rect(1, 0)), true),
            Click::Chosen(Action::Stop)
        );
        bar.click(at(title_rect(1)), true);
        assert_eq!(bar.click(at(item_rect(1, 1)), true), Click::Consumed);
        assert_eq!(
            bar.click(at(item_rect(1, 1)), false),
            Click::Chosen(Action::ToggleEditor)
        );
    }

    #[test]
    fn file_access_is_a_submenu() {
        let mut bar = MenuBar::default();
        let at = |r: Rect| ((r.x + 1) as i32, (r.y + 1) as i32);
        bar.mouse_move(Some((10, 5)));
        bar.click(at(title_rect(2)), false);
        bar.mouse_move(Some(at(item_rect(2, 5))));
        assert_eq!(bar.sub, Some(5), "hovering opens the submenu");
        let silent = sub_rect(2, 5, 2);
        assert!(silent.x >= item_rect(2, 5).x + item_rect(2, 5).w);
        bar.mouse_move(Some(at(silent)));
        assert_eq!(bar.sub, Some(5), "moving into it keeps it open");
        assert_eq!(
            bar.click(at(silent), false),
            Click::Chosen(Action::FileAccess(FileConfirm::Silent))
        );
        // Clicking the parent opens it too, and another item closes it.
        bar.mouse_move(Some(at(title_rect(2))));
        bar.click(at(title_rect(2)), false);
        assert_eq!(bar.click(at(item_rect(2, 5)), false), Click::Consumed);
        assert_eq!(bar.sub, Some(5));
        bar.mouse_move(Some(at(item_rect(2, 0))));
        assert_eq!(bar.sub, None);

        let mut settings = Settings::default();
        Action::FileAccess(FileConfirm::Notify).toggle(&mut settings);
        assert_eq!(settings.file_confirm, FileConfirm::Notify);
        assert_eq!(
            Action::FileAccess(FileConfirm::Notify).mark(&settings),
            "(*)"
        );
    }

    #[test]
    fn clicking_outside_closes() {
        let mut bar = MenuBar::default();
        bar.mouse_move(Some((10, 5)));
        bar.click((10, 5), false);
        assert_eq!(bar.click((600, 400), false), Click::Consumed);
        bar.mouse_move(Some((600, 400)));
        assert!(!bar.visible());
    }
}
