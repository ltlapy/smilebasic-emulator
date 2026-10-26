//! The emulator's menu bar, drawn over the screen at display resolution.
//! It stays hidden until the mouse reaches the top edge, so it takes no
//! keys away from programs.

use sb_runtime::Framebuffer;
use sb_runtime::confirm::FileConfirm;
use sb_runtime::font::Font;
use sb_runtime::runtime::Settings;

use crate::ui::{BACKGROUND, DISABLED, HIGHLIGHT, PAD, Rect, TEXT, draw_text, fill, text_width};

const ROW_HEIGHT: u32 = 24;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Quit,
    Stop,
    ToggleEditor,
    ToggleUpperCase,
    ToggleCtrlC,
    ToggleHalfWidth,
    ToggleWrap,
    FileAccess(FileConfirm),
}

impl Action {
    /// For toggles, whether the setting is on.
    fn checked(self, settings: &Settings) -> Option<bool> {
        match self {
            Action::ToggleUpperCase => Some(settings.upper_case_letters),
            Action::ToggleCtrlC => Some(settings.ctrl_c_stops),
            Action::ToggleHalfWidth => Some(settings.half_width_on_save),
            Action::ToggleWrap => Some(settings.editor_wrap),
            Action::FileAccess(mode) => Some(settings.file_confirm == mode),
            Action::Quit | Action::Stop | Action::ToggleEditor => None,
        }
    }

    /// Flips the setting of a toggle, or picks a choice.
    pub fn toggle(self, settings: &mut Settings) {
        let flag = match self {
            Action::ToggleUpperCase => &mut settings.upper_case_letters,
            Action::ToggleCtrlC => &mut settings.ctrl_c_stops,
            Action::ToggleHalfWidth => &mut settings.half_width_on_save,
            Action::ToggleWrap => &mut settings.editor_wrap,
            Action::FileAccess(mode) => {
                settings.file_confirm = mode;
                return;
            }
            Action::Quit | Action::Stop | Action::ToggleEditor => return,
        };
        *flag = !*flag;
    }

    /// Stop needs a running program; the editor cannot open during one.
    fn enabled(self, running: bool) -> bool {
        match self {
            Action::Stop => running,
            Action::ToggleEditor => !running,
            _ => true,
        }
    }
}

struct Item {
    label: &'static str,
    action: Action,
}

struct Menu {
    title: &'static str,
    items: &'static [Item],
}

const MENUS: &[Menu] = &[
    Menu {
        title: "SmileBASIC",
        items: &[Item {
            label: "Quit",
            action: Action::Quit,
        }],
    },
    Menu {
        title: "Program",
        items: &[
            Item {
                label: "Stop (Pause/Break)",
                action: Action::Stop,
            },
            Item {
                label: "Editor/Direct (F4)",
                action: Action::ToggleEditor,
            },
        ],
    },
    Menu {
        title: "Options",
        items: &[
            Item {
                label: "Upper-case letters",
                action: Action::ToggleUpperCase,
            },
            Item {
                label: "Ctrl+C stops program",
                action: Action::ToggleCtrlC,
            },
            Item {
                label: "Half-width on SAVE",
                action: Action::ToggleHalfWidth,
            },
            Item {
                label: "Wrap lines in editor (Ctrl+W)",
                action: Action::ToggleWrap,
            },
            Item {
                label: "File access: always ask",
                action: Action::FileAccess(FileConfirm::Ask),
            },
            Item {
                label: "File access: notify",
                action: Action::FileAccess(FileConfirm::Notify),
            },
            Item {
                label: "File access: silent",
                action: Action::FileAccess(FileConfirm::Silent),
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

/// An item's label behind a `[x] ` check box for toggles, or the same
/// width of space otherwise.
fn item_text(label: &str, checked: Option<bool>) -> String {
    match checked {
        Some(true) => format!("[x] {label}"),
        Some(false) => format!("[ ] {label}"),
        None => format!("    {label}"),
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

fn item_rect(menu: usize, item: usize) -> Rect {
    let width = MENUS[menu]
        .items
        .iter()
        .map(|i| text_width(&item_text(i.label, Some(false))))
        .max()
        .unwrap_or(0)
        + 2 * PAD;
    Rect {
        x: title_rect(menu).x,
        y: ROW_HEIGHT * (1 + item as u32),
        w: width,
        h: ROW_HEIGHT,
    }
}

#[derive(Debug, Default)]
pub struct MenuBar {
    /// Mouse position in display pixels, if it is over the window.
    mouse: Option<(i32, i32)>,
    open: Option<usize>,
}

impl MenuBar {
    pub fn visible(&self) -> bool {
        self.open.is_some()
            || self
                .mouse
                .is_some_and(|(_, y)| y >= 0 && y < ROW_HEIGHT as i32)
    }

    pub fn mouse_move(&mut self, position: Option<(i32, i32)>) {
        self.mouse = position;
    }

    /// A left click at a display position. `running` tells whether a
    /// program is running, which enables Stop and disables the editor.
    pub fn click(&mut self, at: (i32, i32), running: bool) -> Click {
        if let Some(menu) = self.open {
            for (i, item) in MENUS[menu].items.iter().enumerate() {
                if item_rect(menu, i).contains(at) {
                    if !item.action.enabled(running) {
                        return Click::Consumed;
                    }
                    self.open = None;
                    return Click::Chosen(item.action);
                }
            }
        }
        if !self.visible() && self.open.is_none() {
            return Click::Ignored;
        }
        let title = (0..MENUS.len()).find(|&i| title_rect(i).contains(at));
        let was_open = self.open.take();
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

    /// Draws the bar, and the open menu, onto a transparent overlay.
    pub fn draw(&self, fb: &mut Framebuffer, font: &Font, settings: &Settings, running: bool) {
        fill(
            fb,
            Rect {
                x: 0,
                y: 0,
                w: fb.width(),
                h: ROW_HEIGHT,
            },
            BACKGROUND,
        );
        for (i, menu) in MENUS.iter().enumerate() {
            let r = title_rect(i);
            if self.open == Some(i) || self.mouse.is_some_and(|m| r.contains(m)) {
                fill(fb, r, HIGHLIGHT);
            }
            draw_text(fb, font, r.x + PAD, r.y + 4, menu.title, TEXT);
        }
        let Some(open) = self.open else { return };
        for (i, item) in MENUS[open].items.iter().enumerate() {
            let r = item_rect(open, i);
            let hovered = self.mouse.is_some_and(|m| r.contains(m));
            let enabled = item.action.enabled(running);
            fill(
                fb,
                r,
                if hovered && enabled {
                    HIGHLIGHT
                } else {
                    BACKGROUND
                },
            );
            let label = item_text(item.label, item.action.checked(settings));
            let color = if enabled { TEXT } else { DISABLED };
            draw_text(fb, font, r.x + PAD, r.y + 4, &label, color);
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
    fn clicking_outside_closes() {
        let mut bar = MenuBar::default();
        bar.mouse_move(Some((10, 5)));
        bar.click((10, 5), false);
        assert_eq!(bar.click((600, 400), false), Click::Consumed);
        bar.mouse_move(Some((600, 400)));
        assert!(!bar.visible());
    }
}
