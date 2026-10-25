//! The emulator's menu bar, drawn over the screen at display resolution.
//! It stays hidden until the mouse reaches the top edge, so it takes no
//! keys away from programs.

use sb_runtime::Framebuffer;
use sb_runtime::font::{Font, FontSize};
use sb_runtime::runtime::Settings;

/// Glyphs are the console's 8x8 font drawn at twice the size.
const GLYPH_SCALE: u32 = 2;
const CHAR: u32 = 8 * GLYPH_SCALE;
const ROW_HEIGHT: u32 = 24;
const PAD: u32 = 8;

const BACKGROUND: u32 = 0xE818_1818;
const HIGHLIGHT: u32 = 0xFF2F_5FAF;
const TEXT: u32 = 0xFFFF_FFFF;
const DISABLED: u32 = 0xFF80_8080;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Quit,
    Stop,
    ToggleUpperCase,
    ToggleCtrlC,
    ToggleHalfWidth,
}

impl Action {
    /// For toggles, whether the setting is on.
    fn checked(self, settings: &Settings) -> Option<bool> {
        match self {
            Action::ToggleUpperCase => Some(settings.upper_case_letters),
            Action::ToggleCtrlC => Some(settings.ctrl_c_stops),
            Action::ToggleHalfWidth => Some(settings.half_width_on_save),
            Action::Quit | Action::Stop => None,
        }
    }

    /// Flips the setting of a toggle.
    pub fn toggle(self, settings: &mut Settings) {
        let flag = match self {
            Action::ToggleUpperCase => &mut settings.upper_case_letters,
            Action::ToggleCtrlC => &mut settings.ctrl_c_stops,
            Action::ToggleHalfWidth => &mut settings.half_width_on_save,
            Action::Quit | Action::Stop => return,
        };
        *flag = !*flag;
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
        items: &[Item {
            label: "Stop (Pause/Break)",
            action: Action::Stop,
        }],
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Rect {
    x: u32,
    y: u32,
    w: u32,
    h: u32,
}

impl Rect {
    fn contains(self, (x, y): (i32, i32)) -> bool {
        let (x, y) = (i64::from(x), i64::from(y));
        x >= i64::from(self.x)
            && y >= i64::from(self.y)
            && x < i64::from(self.x + self.w)
            && y < i64::from(self.y + self.h)
    }
}

fn text_width(text: &str) -> u32 {
    text.chars().count() as u32 * CHAR
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
    /// program is running, which enables Stop.
    pub fn click(&mut self, at: (i32, i32), running: bool) -> Click {
        if let Some(menu) = self.open {
            for (i, item) in MENUS[menu].items.iter().enumerate() {
                if item_rect(menu, i).contains(at) {
                    if item.action == Action::Stop && !running {
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
            let enabled = item.action != Action::Stop || running;
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

fn fill(fb: &mut Framebuffer, r: Rect, color: u32) {
    for y in r.y..r.y + r.h {
        for x in r.x..r.x + r.w {
            fb.set(x, y, color);
        }
    }
}

/// Draws text in `color`, keeping the background's alpha.
fn draw_text(fb: &mut Framebuffer, font: &Font, x: u32, y: u32, text: &str, color: u32) {
    for (i, c) in text.encode_utf16().enumerate() {
        let left = x + i as u32 * CHAR;
        for py in 0..CHAR {
            for px in 0..CHAR {
                let glyph = font.pixel(c, FontSize::Small, px / GLYPH_SCALE, py / GLYPH_SCALE);
                if glyph >> 24 >= 0x80
                    && let Some(dst) = fb.get(left + px, y + py)
                {
                    fb.set(
                        left + px,
                        y + py,
                        (dst & 0xFF00_0000) | (color & 0x00FF_FFFF),
                    );
                }
            }
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
