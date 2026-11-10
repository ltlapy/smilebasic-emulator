//! Help > Keyboard shortcuts: docs/shortcuts.md's tables of keys, in a window.

use sb_runtime::Framebuffer;
use sb_runtime::font::Font;
use sdl3::keyboard::Keycode;

use crate::ui::{
    BORDER, CHAR, DISABLED, LINE, PAD, Rect, TEXT, draw_text, outline, text_width, window,
};

const SHORTCUTS: &str = include_str!("../../../docs/shortcuts.md");
const WIDTH: u32 = 1160;
const HEIGHT: u32 = 640;
/// The width of the keys' column.
const KEYS_WIDTH: u32 = 22 * CHAR;
const HINT: &str = "Up/Down, wheel: scroll  Esc: close";
/// A group's heading: HIGHLIGHT, light enough to read on the window.
const HEADING: u32 = 0xFF8F_B8FF;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Stay,
    Close,
}

/// A line of docs/shortcuts.md the window shows.
#[derive(Debug, PartialEq, Eq)]
enum Entry {
    Heading(String),
    /// The keys and what they do.
    Keys(String, String),
}

/// docs/shortcuts.md's groups and the rows of their tables, in order.
fn entries() -> Vec<Entry> {
    let mut entries = Vec::new();
    for line in SHORTCUTS.lines() {
        if let Some(heading) = line.strip_prefix("## ") {
            entries.push(Entry::Heading(heading.trim().to_string()));
        } else if let Some(row) = line.strip_prefix('|') {
            let mut cells = row.split('|').map(|c| c.trim().replace('`', ""));
            if let (Some(keys), Some(what)) = (cells.next(), cells.next()) {
                // Each table's head and the line under it.
                if keys != "Key" && keys != "---" {
                    entries.push(Entry::Keys(keys, what));
                }
            }
        }
    }
    entries
}

/// `text` broken at spaces into lines at most `width` pixels wide as
/// [`draw_text`] draws them.
pub fn wrap(text: &str, width: u32) -> Vec<String> {
    let mut lines = vec![String::new()];
    for word in text.split(' ') {
        let line = lines.last_mut().expect("never empty");
        if !line.is_empty() && text_width(line) + text_width(" ") + text_width(word) > width {
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

/// One screen line.
enum Line {
    Blank,
    Heading(String),
    /// A piece of the keys and a piece of what they do.
    Keys(String, String),
}

pub struct ShortcutsWindow {
    size: (u32, u32),
    lines: Vec<Line>,
    scroll: usize,
}

impl ShortcutsWindow {
    pub fn new(size: (u32, u32)) -> ShortcutsWindow {
        let mut window = ShortcutsWindow {
            size,
            lines: Vec::new(),
            scroll: 0,
        };
        let keys_width = KEYS_WIDTH;
        let width = window.list().w - KEYS_WIDTH - 3 * PAD;
        for entry in entries() {
            match entry {
                Entry::Heading(heading) => {
                    if !window.lines.is_empty() {
                        window.lines.push(Line::Blank);
                    }
                    window.lines.push(Line::Heading(heading));
                }
                Entry::Keys(keys, what) => {
                    let mut keys = wrap(&keys, keys_width).into_iter();
                    let mut what = wrap(&what, width).into_iter();
                    loop {
                        let (k, w) = (keys.next(), what.next());
                        if k.is_none() && w.is_none() {
                            break;
                        }
                        window
                            .lines
                            .push(Line::Keys(k.unwrap_or_default(), w.unwrap_or_default()));
                    }
                }
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
        for (row, line) in shown.enumerate() {
            let y = list.y + row as u32 * LINE + 4;
            match line {
                Line::Blank => {}
                Line::Heading(heading) => draw_text(fb, font, list.x + PAD, y, heading, HEADING),
                Line::Keys(keys, what) => {
                    draw_text(fb, font, list.x + PAD, y, keys, TEXT);
                    draw_text(fb, font, list.x + KEYS_WIDTH + 2 * PAD, y, what, TEXT);
                }
            }
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
    fn reads_the_groups_and_their_tables() {
        let entries = entries();
        assert_eq!(entries[0], Entry::Heading("DIRECT mode".to_string()));
        assert!(matches!(&entries[1], Entry::Keys(keys, _) if keys == "Up, Down"));
        let keys = entries.iter().filter_map(|e| match e {
            Entry::Keys(keys, what) => Some((keys, what)),
            Entry::Heading(_) => None,
        });
        assert!(keys.clone().count() > 20);
        assert!(keys.clone().any(|(keys, _)| keys == "Ctrl+Space"));
        assert!(keys.clone().all(|(keys, _)| keys != "Key" && keys != "---"));
        assert!(keys.clone().all(|(_, what)| !what.contains('`')));
    }

    #[test]
    fn long_keys_stay_in_their_column() {
        let w = ShortcutsWindow::new((1280, 720));
        assert!(w.lines.iter().all(|line| match line {
            Line::Keys(keys, _) => text_width(keys) <= KEYS_WIDTH,
            _ => true,
        }));
    }

    #[test]
    fn wraps_and_scrolls() {
        // Six ASCII characters to a line.
        assert_eq!(wrap("a bb ccc dd", 3 * CHAR), ["a bb", "ccc dd"]);
        let mut w = ShortcutsWindow::new((1280, 720));
        assert!(w.lines.len() > entries().len(), "long rows wrap");
        w.key(Keycode::End);
        assert_eq!(w.scroll, w.lines.len() - w.rows());
        w.wheel(-1);
        assert_eq!(w.scroll, w.lines.len() - w.rows() - 3);
        assert_eq!(w.key(Keycode::Escape), Outcome::Close);
    }
}
