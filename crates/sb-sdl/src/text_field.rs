//! A box to type text into, one line or several, with a cursor that the
//! arrow keys move. The projects window edits a project's title and
//! description with it.

use sb_runtime::Framebuffer;
use sb_runtime::font::Font;
use sdl3::keyboard::Keycode;

use crate::ui::{
    BORDER, HIGHLIGHT, LINE, PAD, Rect, TEXT, char_width, draw_text, fill, outline, text_width,
};

/// The inside of a field.
const FACE: u32 = 0xFF0B_0C10;

pub struct TextField {
    chars: Vec<char>,
    cursor: usize,
    /// The most UTF-16 units it holds, the unit SmileBASIC counts
    /// characters in.
    max: usize,
    multiline: bool,
    line_width: u32,
    visible_rows: usize,
    /// The first line shown.
    scroll: usize,
}

/// The effect of a key on a field.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Keyed {
    /// The field used the key.
    Used,
    /// The key is not for the field: Return in a one-line field, Tab,
    /// Escape and the like.
    Passed,
}

impl TextField {
    /// A field over `r`, holding `text` cut to `max` units.
    pub fn new(text: &str, max: usize, multiline: bool, r: Rect) -> TextField {
        let mut field = TextField {
            chars: Vec::new(),
            cursor: 0,
            max,
            multiline,
            line_width: r.w.saturating_sub(2 * PAD),
            visible_rows: (r.h / LINE).max(1) as usize,
            scroll: 0,
        };
        field.insert(text);
        field.cursor = 0;
        field.scroll = 0;
        field
    }

    pub fn text(&self) -> String {
        self.chars.iter().collect()
    }

    fn units(&self) -> usize {
        self.chars.iter().map(|c| c.len_utf16()).sum()
    }

    /// Typed text at the cursor, as far as there is room. Line breaks
    /// only go into a field of several lines; other control characters
    /// are dropped.
    pub fn insert(&mut self, text: &str) {
        let mut units = self.units();
        for c in text.chars() {
            let c = if c == '\r' { '\n' } else { c };
            let wanted = match c {
                '\n' => self.multiline,
                c => !c.is_control(),
            };
            if !wanted || units + c.len_utf16() > self.max {
                continue;
            }
            units += c.len_utf16();
            self.chars.insert(self.cursor, c);
            self.cursor += 1;
        }
        self.keep_in_view();
    }

    pub fn key(&mut self, key: Keycode) -> Keyed {
        match key {
            Keycode::Backspace if self.cursor > 0 => {
                self.cursor -= 1;
                self.chars.remove(self.cursor);
            }
            Keycode::Delete if self.cursor < self.chars.len() => {
                self.chars.remove(self.cursor);
            }
            Keycode::Backspace | Keycode::Delete => {}
            Keycode::Left => self.cursor = self.cursor.saturating_sub(1),
            Keycode::Right => self.cursor = (self.cursor + 1).min(self.chars.len()),
            Keycode::Home => self.cursor = self.lines()[self.line_of_cursor()].0,
            Keycode::End => self.cursor = self.lines()[self.line_of_cursor()].1,
            Keycode::Up | Keycode::Down if self.multiline => {
                let lines = self.lines();
                let at = self.line_of_cursor();
                let column = self.cursor - lines[at].0;
                let to = match key {
                    Keycode::Up => at.checked_sub(1),
                    _ => Some(at + 1).filter(|&l| l < lines.len()),
                };
                match to {
                    Some(l) => self.cursor = (lines[l].0 + column).min(lines[l].1),
                    None if key == Keycode::Up => self.cursor = 0,
                    None => self.cursor = self.chars.len(),
                }
            }
            Keycode::Return | Keycode::KpEnter if self.multiline => self.insert("\n"),
            // Typed letters and spaces come as text.
            Keycode::Space => {}
            _ => return Keyed::Passed,
        }
        self.keep_in_view();
        Keyed::Used
    }

    /// The lines as shown, as ranges of characters: a line break ends
    /// one, and so does running out of room.
    fn lines(&self) -> Vec<(usize, usize)> {
        let mut lines = Vec::new();
        let (mut start, mut width) = (0, 0);
        for (i, &c) in self.chars.iter().enumerate() {
            if c == '\n' {
                lines.push((start, i));
                (start, width) = (i + 1, 0);
                continue;
            }
            let w = char_width(c);
            if width + w > self.line_width && i > start {
                lines.push((start, i));
                (start, width) = (i, 0);
            }
            width += w;
        }
        lines.push((start, self.chars.len()));
        lines
    }

    /// The line the cursor is on. At the end of a full line the cursor is
    /// on the next line, where the next character will be.
    fn line_of_cursor(&self) -> usize {
        let lines = self.lines();
        let last = lines.len() - 1;
        (0..last)
            .find(|&l| {
                let (start, end) = lines[l];
                let next = lines[l + 1].0;
                self.cursor >= start && (self.cursor < end || (self.cursor == end && next > end))
            })
            .unwrap_or(last)
    }

    fn keep_in_view(&mut self) {
        let line = self.line_of_cursor();
        if line < self.scroll {
            self.scroll = line;
        } else if line >= self.scroll + self.visible_rows {
            self.scroll = line + 1 - self.visible_rows;
        }
    }

    pub fn draw(&self, fb: &mut Framebuffer, font: &Font, r: Rect, focused: bool, blink: bool) {
        fill(fb, r, FACE);
        outline(fb, r, if focused { HIGHLIGHT } else { BORDER });
        let lines = self.lines();
        let cursor_line = self.line_of_cursor();
        for (row, &(start, end)) in lines
            .iter()
            .enumerate()
            .skip(self.scroll)
            .take(self.visible_rows)
        {
            let y = r.y + (row - self.scroll) as u32 * LINE;
            let text: String = self.chars[start..end].iter().collect();
            draw_text(fb, font, r.x + PAD, y + 4, &text, TEXT);
            if focused && blink && row == cursor_line {
                let before: String = self.chars[start..self.cursor].iter().collect();
                let x = r.x + PAD + text_width(&before);
                fill(
                    fb,
                    Rect {
                        x,
                        y: y + 3,
                        w: 2,
                        h: LINE - 6,
                    },
                    TEXT,
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::CHAR;

    fn field(text: &str, multiline: bool) -> TextField {
        // Four ASCII characters per line, two lines shown.
        let r = Rect {
            x: 0,
            y: 0,
            w: 2 * CHAR + 2 * PAD,
            h: 2 * LINE,
        };
        TextField::new(text, 10, multiline, r)
    }

    #[test]
    fn typing_and_moving() {
        let mut f = field("ab", false);
        assert_eq!(f.cursor, 0);
        f.key(Keycode::End);
        f.insert("c\nd");
        assert_eq!(f.text(), "abcd", "no line break in one line");
        f.key(Keycode::Left);
        f.key(Keycode::Backspace);
        assert_eq!(f.text(), "abd");
        f.key(Keycode::Home);
        f.key(Keycode::Delete);
        assert_eq!(f.text(), "bd");
        assert_eq!(f.key(Keycode::Return), Keyed::Passed);
        assert_eq!(f.key(Keycode::Tab), Keyed::Passed);
        f.insert("123456789");
        assert_eq!(f.units(), 10, "cut at the limit");
    }

    #[test]
    fn lines_wrap_by_width() {
        // A full-width character takes the room of two ASCII characters.
        let f = field("가가가ab가", true);
        assert_eq!(f.lines(), [(0, 2), (2, 5), (5, 6)]);
    }

    #[test]
    fn lines_wrap_and_scroll() {
        let mut f = field("abcdefg\nh", true);
        assert_eq!(f.lines(), [(0, 4), (4, 7), (8, 9)]);
        f.key(Keycode::Down);
        assert_eq!(f.cursor, 4);
        f.key(Keycode::Down);
        assert_eq!((f.cursor, f.scroll), (8, 1), "the third line scrolls in");
        f.key(Keycode::Up);
        f.key(Keycode::Up);
        assert_eq!((f.cursor, f.scroll), (0, 0));
        f.key(Keycode::End);
        assert_eq!(f.cursor, 4, "the end of the first line shown");
        f.key(Keycode::Return);
        assert_eq!(f.text(), "abcd\nefg\nh");
    }
}
