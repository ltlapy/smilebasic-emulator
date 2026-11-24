//! The index panel: the program's labels and DEFs beside the editor, to
//! jump to. Typing narrows the list; the editor follows the chosen entry
//! until Enter keeps it or Esc goes back.

use sb_runtime::Framebuffer;
use sb_runtime::font::Font;
use sdl3::keyboard::Keycode;

use crate::ui::{DISABLED, HIGHLIGHT, LINE, PAD, Rect, TEXT, draw_text, fill, panel, side_rect};

const HINT: &str = "Type to narrow  Enter: go  Esc: back";
const LABEL: u32 = 0xFFFF_E070;
const DEF: u32 = 0xFF70_D0FF;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Stay,
    /// Show this line (from 0) while the panel stays open.
    Show(usize),
    /// Go to this line and close.
    Go(usize),
    /// Close and go back to where the editor was.
    Back,
}

pub struct IndexPanel {
    size: (u32, u32),
    pub left: bool,
    title: String,
    /// Each entry's line (from 0) and name.
    entries: Vec<(usize, String)>,
    filter: String,
    /// The entries that the filter lets through.
    shown: Vec<usize>,
    /// The chosen one of `shown`.
    chosen: usize,
    /// The first of `shown` on screen.
    scroll: usize,
    hovered: Option<usize>,
}

impl IndexPanel {
    /// The index of a slot's labels and DEFs. The chosen entry is the one
    /// that the cursor is in, or after.
    pub fn new(size: (u32, u32), slot: usize, entries: Vec<(usize, String)>, at: usize) -> Self {
        let mut panel = IndexPanel {
            size,
            left: false,
            title: format!("Index: PRG{slot}"),
            entries,
            filter: String::new(),
            shown: Vec::new(),
            chosen: 0,
            scroll: 0,
            hovered: None,
        };
        panel.narrow();
        panel.chosen = panel
            .shown
            .iter()
            .rposition(|&i| panel.entries[i].0 <= at)
            .unwrap_or(0);
        panel.keep_in_view();
        panel
    }

    pub fn frame(&self) -> Rect {
        side_rect(self.size, self.left, 0)
    }

    /// Moves to a display of another size, for example when the editor
    /// window is resized, and keeps the chosen entry in view.
    pub fn set_size(&mut self, size: (u32, u32)) {
        if size != self.size {
            self.size = size;
            self.keep_in_view();
        }
    }

    fn body_top(&self) -> u32 {
        // The filter's row is under the title bar.
        self.frame().y + 2 * LINE + 2 * PAD
    }

    fn visible(&self) -> usize {
        let f = self.frame();
        (f.h.saturating_sub(3 * LINE + 4 * PAD) / LINE).max(1) as usize
    }

    fn narrow(&mut self) {
        let filter = self.filter.to_ascii_uppercase();
        self.shown = (0..self.entries.len())
            .filter(|&i| self.entries[i].1.to_ascii_uppercase().contains(&filter))
            .collect();
        self.chosen = 0;
        self.scroll = 0;
    }

    fn keep_in_view(&mut self) {
        let visible = self.visible();
        self.scroll = self.scroll.min(self.chosen);
        if self.chosen >= self.scroll + visible {
            self.scroll = self.chosen + 1 - visible;
        }
    }

    fn line(&self) -> Option<usize> {
        Some(self.entries[*self.shown.get(self.chosen)?].0)
    }

    fn show(&self) -> Outcome {
        self.line().map_or(Outcome::Stay, Outcome::Show)
    }

    fn choose(&mut self, to: isize) -> Outcome {
        let last = self.shown.len().saturating_sub(1) as isize;
        self.chosen = to.clamp(0, last) as usize;
        self.keep_in_view();
        self.show()
    }

    pub fn key(&mut self, key: Keycode) -> Outcome {
        let at = self.chosen as isize;
        let page = self.visible() as isize - 1;
        match key {
            Keycode::Escape => Outcome::Back,
            Keycode::Return | Keycode::KpEnter => self.line().map_or(Outcome::Back, Outcome::Go),
            Keycode::Up => self.choose(at - 1),
            Keycode::Down => self.choose(at + 1),
            Keycode::PageUp => self.choose(at - page),
            Keycode::PageDown => self.choose(at + page),
            Keycode::Home => self.choose(0),
            Keycode::End => self.choose(isize::MAX),
            Keycode::Backspace => {
                self.filter.pop();
                self.narrow();
                self.show()
            }
            _ => Outcome::Stay,
        }
    }

    pub fn text(&mut self, text: &str) -> Outcome {
        self.filter.push_str(text);
        self.narrow();
        self.show()
    }

    pub fn wheel(&mut self, rows: i32) {
        let most = self.shown.len().saturating_sub(self.visible());
        self.scroll = self
            .scroll
            .saturating_add_signed(3 * rows as isize)
            .min(most);
    }

    /// The entry under a point, as an index into `shown`.
    fn entry_at(&self, (x, y): (i32, i32)) -> Option<usize> {
        let f = self.frame();
        let top = self.body_top() as i32;
        if !f.contains((x, y)) || y < top {
            return None;
        }
        let i = self.scroll + ((y - top) / LINE as i32) as usize;
        (i < self.shown.len() && i < self.scroll + self.visible()).then_some(i)
    }

    pub fn mouse_move(&mut self, at: (i32, i32)) {
        self.hovered = self.entry_at(at);
    }

    pub fn click(&mut self, at: (i32, i32)) -> Outcome {
        match self.entry_at(at) {
            Some(i) => {
                self.chosen = i;
                self.line().map_or(Outcome::Stay, Outcome::Go)
            }
            None => Outcome::Stay,
        }
    }

    pub fn draw(&self, fb: &mut Framebuffer, font: &Font, focused: bool) {
        let f = self.frame();
        panel(fb, font, f, &self.title, focused);
        let left = f.x + 2 * PAD;
        let filter_y = f.y + LINE + PAD;
        let prompt = format!("Find: {}", self.filter);
        draw_text(fb, font, left, filter_y + 4, &prompt, TEXT);
        if self.shown.is_empty() {
            let note = if self.entries.is_empty() {
                "No labels or DEFs"
            } else {
                "Nothing matches"
            };
            draw_text(fb, font, left, self.body_top() + 4, note, DISABLED);
        }
        // Line numbers take the digits that the last one needs.
        let digits = self
            .entries
            .last()
            .map_or(1, |(line, _)| (line + 1).to_string().len());
        let width = f.w - 4 * PAD;
        for (row, &i) in self
            .shown
            .iter()
            .enumerate()
            .skip(self.scroll)
            .take(self.visible())
        {
            let y = self.body_top() + (row - self.scroll) as u32 * LINE;
            if row == self.chosen || Some(row) == self.hovered {
                let back = if row == self.chosen {
                    HIGHLIGHT
                } else {
                    0xFF30_3040
                };
                fill(
                    fb,
                    Rect {
                        x: left - PAD / 2,
                        y,
                        w: width + PAD,
                        h: LINE,
                    },
                    back,
                );
            }
            let (line, name) = &self.entries[i];
            let number = format!("{:>digits$} ", line + 1);
            draw_text(fb, font, left, y + 4, &number, DISABLED);
            let color = if name.starts_with('@') { LABEL } else { DEF };
            let x = left + (digits as u32 + 1) * crate::ui::CHAR;
            draw_text(fb, font, x, y + 4, name, color);
        }
        draw_text(fb, font, left, f.y + f.h - LINE - PAD, HINT, DISABLED);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn panel() -> IndexPanel {
        let entries = [
            (0, "@MAIN"),
            (10, "DEF DRAW"),
            (20, "@LOOP"),
            (30, "DEF MOVE"),
        ];
        let entries = entries.map(|(l, n)| (l, n.to_string())).to_vec();
        IndexPanel::new((1280, 720), 0, entries, 25)
    }

    #[test]
    fn starts_at_the_cursor_and_narrows() {
        let mut p = panel();
        assert_eq!(p.key(Keycode::Down), Outcome::Show(30));
        assert_eq!(p.key(Keycode::Up), Outcome::Show(20));
        assert_eq!(p.text("def"), Outcome::Show(10));
        assert_eq!(p.key(Keycode::End), Outcome::Show(30));
        assert_eq!(p.key(Keycode::Return), Outcome::Go(30));
        assert_eq!(p.text("x"), Outcome::Stay);
        assert_eq!(p.key(Keycode::Return), Outcome::Back);
        assert_eq!(p.key(Keycode::Backspace), Outcome::Show(10));
        assert_eq!(p.key(Keycode::Escape), Outcome::Back);
    }

    #[test]
    fn clicks_go_to_an_entry() {
        let mut p = panel();
        let f = p.frame();
        let y = p.body_top() as i32 + LINE as i32 + 2;
        assert_eq!(p.click((f.x as i32 + 20, y)), Outcome::Go(10));
        assert_eq!(p.click((f.x as i32 + 20, f.y as i32 + 2)), Outcome::Stay);
    }
}
