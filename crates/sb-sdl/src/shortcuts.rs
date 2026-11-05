//! Help > Keyboard shortcuts: the README's table of keys, in a window.

use sb_runtime::Framebuffer;
use sb_runtime::font::Font;
use sdl3::keyboard::Keycode;

use crate::ui::{BORDER, CHAR, DISABLED, LINE, PAD, Rect, TEXT, draw_text, outline, window};

const README: &str = include_str!("../../../README.md");
const WIDTH: u32 = 1160;
const HEIGHT: u32 = 640;
/// The width of the keys' column.
const KEYS_WIDTH: u32 = 20 * CHAR;
const HINT: &str = "Up/Down, wheel: scroll  Esc: close";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Stay,
    Close,
}

/// The rows of the README's shortcut tables, one under each sub-heading of
/// "Shortcuts": the keys and what they do.
fn table() -> Vec<(String, String)> {
    let mut rows = Vec::new();
    let section = README
        .lines()
        .skip_while(|l| l.trim() != "### Shortcuts")
        .skip(1)
        .take_while(|l| !l.starts_with("### ") && !l.starts_with("## "));
    for line in section {
        let Some(row) = line.strip_prefix('|') else {
            continue;
        };
        let mut cells = row.split('|').map(|c| c.trim().replace('`', ""));
        if let (Some(keys), Some(what)) = (cells.next(), cells.next()) {
            // Each table's head and the line under it.
            if keys != "Key" && keys != "---" {
                rows.push((keys, what));
            }
        }
    }
    rows
}

/// `text` broken at spaces into lines of at most `width` characters.
pub fn wrap(text: &str, width: usize) -> Vec<String> {
    let mut lines = vec![String::new()];
    for word in text.split(' ') {
        let line = lines.last_mut().expect("never empty");
        if !line.is_empty() && line.len() + 1 + word.len() > width {
            lines.push(word.to_string());
        } else {
            if !line.is_empty() {
                line.push(' ');
            }
            line.push_str(word);
        }
    }
    lines
}

pub struct ShortcutsWindow {
    size: (u32, u32),
    /// Each screen line: the keys (on a row's first line) and a piece of
    /// what they do.
    lines: Vec<(String, String)>,
    scroll: usize,
}

impl ShortcutsWindow {
    pub fn new(size: (u32, u32)) -> ShortcutsWindow {
        let mut window = ShortcutsWindow {
            size,
            lines: Vec::new(),
            scroll: 0,
        };
        let width = (window.list().w - KEYS_WIDTH - 3 * PAD) / CHAR;
        for (keys, what) in table() {
            for (i, piece) in wrap(&what, width as usize).into_iter().enumerate() {
                let keys = if i == 0 { keys.clone() } else { String::new() };
                window.lines.push((keys, piece));
            }
        }
        window
    }

    fn frame(&self) -> Rect {
        let (w, h) = (WIDTH.min(self.size.0), HEIGHT.min(self.size.1));
        Rect {
            x: (self.size.0 - w) / 2,
            y: (self.size.1 - h) / 2,
            w,
            h,
        }
    }

    fn list(&self) -> Rect {
        let f = self.frame();
        Rect {
            x: f.x + 2 * PAD,
            y: f.y + LINE + 2 * PAD,
            w: f.w - 4 * PAD,
            h: f.h - 2 * LINE - 4 * PAD,
        }
    }

    fn rows(&self) -> usize {
        (self.list().h / LINE) as usize
    }

    fn scroll_by(&mut self, lines: i32) {
        let most = self.lines.len().saturating_sub(self.rows());
        self.scroll = self.scroll.saturating_add_signed(lines as isize).min(most);
    }

    pub fn key(&mut self, key: Keycode) -> Outcome {
        let page = self.rows().saturating_sub(1) as i32;
        match key {
            Keycode::Escape | Keycode::Return | Keycode::KpEnter => return Outcome::Close,
            Keycode::Up => self.scroll_by(-1),
            Keycode::Down => self.scroll_by(1),
            Keycode::PageUp => self.scroll_by(-page),
            Keycode::PageDown => self.scroll_by(page),
            Keycode::Home => self.scroll = 0,
            Keycode::End => self.scroll_by(i32::MAX),
            _ => {}
        }
        Outcome::Stay
    }

    pub fn wheel(&mut self, lines: i32) {
        self.scroll_by(3 * lines);
    }

    /// A click outside the window closes it.
    pub fn click(&self, at: (i32, i32)) -> Outcome {
        match self.frame().contains(at) {
            true => Outcome::Stay,
            false => Outcome::Close,
        }
    }

    pub fn draw(&self, fb: &mut Framebuffer, font: &Font) {
        let f = self.frame();
        window(fb, font, f, "Keyboard shortcuts");
        let list = self.list();
        let shown = self.lines.iter().skip(self.scroll).take(self.rows());
        for (row, (keys, what)) in shown.enumerate() {
            let y = list.y + row as u32 * LINE + 4;
            draw_text(fb, font, list.x + PAD, y, keys, TEXT);
            draw_text(fb, font, list.x + KEYS_WIDTH + 2 * PAD, y, what, TEXT);
        }
        outline(fb, list, BORDER);
        let y = f.y + f.h - LINE - PAD + 4;
        draw_text(fb, font, list.x, y, HINT, DISABLED);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_readme_table() {
        let rows = table();
        assert!(rows.len() > 20);
        assert_eq!(rows[0].0, "Up, Down");
        assert!(rows.iter().any(|(keys, _)| keys == "Ctrl+Space"));
        assert!(rows.iter().all(|(_, what)| !what.contains('`')));
    }

    #[test]
    fn wraps_and_scrolls() {
        assert_eq!(wrap("a bb ccc dd", 6), ["a bb", "ccc dd"]);
        let mut w = ShortcutsWindow::new((1280, 720));
        assert!(w.lines.len() > table().len(), "long rows wrap");
        w.key(Keycode::End);
        assert_eq!(w.scroll, w.lines.len() - w.rows());
        w.wheel(-1);
        assert_eq!(w.scroll, w.lines.len() - w.rows() - 3);
        assert_eq!(w.key(Keycode::Escape), Outcome::Close);
    }
}
