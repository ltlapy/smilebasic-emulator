//! The line typed in direct mode or for INPUT and LINPUT, edited where
//! it is on the console: the cursor moves along it, Shift selects, and
//! the line is drawn again after each change.

use crate::complete::Completion;
use crate::runtime::keys;
use crate::text::{Cell, TextScreen};

/// Editing commands that only the line takes, queued with the keys so
/// that they come in order with them. The frontend queues them only while
/// a line is typed, when no program reads the keys.
pub mod commands {
    pub const SELECT_LEFT: u16 = 0xF8F0;
    pub const SELECT_RIGHT: u16 = 0xF8F1;
    pub const SELECT_HOME: u16 = 0xF8F2;
    pub const SELECT_END: u16 = 0xF8F3;
    pub const SELECT_ALL: u16 = 0xF8F4;
    /// Ctrl+Backspace: empties the line.
    pub const DELETE_LINE: u16 = 0xF8F5;
    /// Ctrl+Delete: removes the line from the cursor on.
    pub const DELETE_TO_END: u16 = 0xF8F6;
}

/// The color of selected characters, which have no background of their
/// own on the console.
// UNVERIFIED: how SmileBASIC 4 shows a selection in direct mode.
const SELECTED: u32 = 0xFF60_C0FF;

#[derive(Clone, Debug)]
pub struct LineEdit {
    pub text: Vec<u16>,
    pub cursor: usize,
    /// The other end of the selection.
    anchor: Option<usize>,
    /// Where the line starts on the console, counted in cells from its
    /// top left. Scrolling moves it up, above the console if need be.
    origin: i64,
    /// Characters drawn last time, cleared if the line got shorter.
    shown: usize,
    /// Completion's list for the word being typed.
    pub completion: Option<Completion>,
}

impl LineEdit {
    /// A line starting at the console's cursor.
    pub fn new(console: &TextScreen) -> LineEdit {
        let (w, _) = console.size();
        let (x, y) = console.cursor();
        LineEdit {
            text: Vec::new(),
            cursor: 0,
            anchor: None,
            origin: i64::from(y) * i64::from(w) + i64::from(x),
            shown: 0,
            completion: None,
        }
    }

    /// The console cell of a character of the line, if on the console:
    /// `w` is the console's width.
    pub fn cell_of(&self, i: usize, w: u32) -> Option<(i64, i64)> {
        let w = i64::from(w);
        let p = self.origin + i as i64;
        (w > 0 && p >= 0).then(|| (p % w, p / w))
    }

    pub fn selection(&self) -> Option<(usize, usize)> {
        let anchor = self.anchor?;
        (anchor != self.cursor).then(|| (anchor.min(self.cursor), anchor.max(self.cursor)))
    }

    pub fn selected_text(&self) -> Option<Vec<u16>> {
        let (from, to) = self.selection()?;
        Some(self.text[from..to].to_vec())
    }

    /// Puts other text in, with the cursor at its end.
    pub fn set_text(&mut self, text: Vec<u16>) {
        self.cursor = text.len();
        self.text = text;
        self.anchor = None;
    }

    fn delete_selection(&mut self) -> bool {
        let Some((from, to)) = self.selection() else {
            return false;
        };
        self.text.drain(from..to);
        self.cursor = from;
        self.anchor = None;
        true
    }

    /// Replaces the selection, or the text from `from` to the cursor,
    /// with `text`.
    pub fn replace_before(&mut self, from: usize, text: &[u16]) {
        if !self.delete_selection() {
            self.text.drain(from..self.cursor);
            self.cursor = from;
        }
        self.text
            .splice(self.cursor..self.cursor, text.iter().copied());
        self.cursor += text.len();
    }

    /// Moves the cursor, keeping the selection's other end if `select`.
    fn move_to(&mut self, to: usize, select: bool) {
        if select {
            self.anchor.get_or_insert(self.cursor);
        } else {
            self.anchor = None;
        }
        self.cursor = to.min(self.text.len());
    }

    /// Edits with a typed character, a [`keys`] code or one of the
    /// [`commands`]. Enter and the history keys are the caller's.
    pub fn key(&mut self, code: u16) {
        use commands::*;
        let len = self.text.len();
        let left = self.cursor.saturating_sub(1);
        match code {
            keys::LEFT => self.move_to(left, false),
            keys::RIGHT => self.move_to(self.cursor + 1, false),
            keys::HOME => self.move_to(0, false),
            keys::END => self.move_to(len, false),
            SELECT_LEFT => self.move_to(left, true),
            SELECT_RIGHT => self.move_to(self.cursor + 1, true),
            SELECT_HOME => self.move_to(0, true),
            SELECT_END => self.move_to(len, true),
            SELECT_ALL => {
                self.anchor = Some(0);
                self.cursor = len;
            }
            DELETE_LINE => self.set_text(Vec::new()),
            DELETE_TO_END => {
                self.anchor = None;
                self.text.truncate(self.cursor);
            }
            keys::BACKSPACE if self.delete_selection() => {}
            keys::BACKSPACE if self.cursor > 0 => {
                self.cursor -= 1;
                self.text.remove(self.cursor);
            }
            keys::DELETE if self.delete_selection() => {}
            keys::DELETE if self.cursor < len => {
                self.text.remove(self.cursor);
            }
            0..0x20 | 0x7F => {}
            c => {
                self.delete_selection();
                self.text.insert(self.cursor, c);
                self.cursor += 1;
            }
        }
    }

    /// Draws the line where it started, scrolling the console up if its
    /// end or the cursor would go below it, and puts the console's cursor
    /// at the line's.
    pub fn draw(&mut self, console: &mut TextScreen) {
        let (w, h) = console.size();
        let (w, h) = (i64::from(w), i64::from(h));
        if w == 0 || h == 0 {
            return;
        }
        let bottom = (self.origin + self.text.len() as i64) / w;
        if bottom >= h {
            let rows = bottom - h + 1;
            console.scroll(0, rows as i32);
            self.origin -= rows * w;
        }
        let (color, attr) = (console.color(), console.attr);
        let selection = self.selection();
        for i in 0..self.text.len().max(self.shown) {
            let p = self.origin + i as i64;
            if p < 0 {
                continue;
            }
            let cell = match self.text.get(i) {
                Some(&code) => {
                    let selected = selection.is_some_and(|(a, b)| a <= i && i < b);
                    let color = if selected { SELECTED } else { color };
                    Cell { code, color, attr }
                }
                None => Cell {
                    code: 0,
                    color: 0,
                    attr: 0,
                },
            };
            console.set_cell((p % w) as i32, (p / w) as i32, cell);
        }
        self.shown = self.text.len();
        let p = (self.origin + self.cursor as i64).max(0);
        let _ = console.locate(Some((p % w) as i32), Some((p / w) as i32));
    }

    /// Ends the line with Enter: the console goes on below its end.
    pub fn finish(mut self, console: &mut TextScreen) -> Vec<u16> {
        self.anchor = None;
        self.cursor = self.text.len();
        self.draw(console);
        let w = i64::from(console.size().0);
        // A line that fills its last row has its cursor on the next one
        // already.
        let p = self.origin + self.text.len() as i64;
        if self.text.is_empty() || w == 0 || p % w != 0 {
            console.newline();
        }
        self.text
    }
}

#[cfg(test)]
mod tests {
    use super::commands::*;
    use super::*;

    fn console() -> TextScreen {
        // 5 x 3 cells.
        TextScreen::new(40, 24)
    }

    fn row(c: &TextScreen, y: u32) -> String {
        (0..c.size().0)
            .map(|x| match c.cell(x, y).code {
                0 => '.',
                code => char::from_u32(code.into()).unwrap(),
            })
            .collect()
    }

    fn typing(line: &mut LineEdit, text: &str) {
        for c in text.encode_utf16() {
            line.key(c);
        }
    }

    #[test]
    fn edits_in_the_middle() {
        let mut c = console();
        c.print(&[u16::from(b'>')]);
        let mut line = LineEdit::new(&c);
        typing(&mut line, "ACD");
        line.key(keys::LEFT);
        line.key(keys::LEFT);
        typing(&mut line, "B");
        line.draw(&mut c);
        assert_eq!(row(&c, 0), ">ABCD");
        assert_eq!(c.cursor(), (3, 0));
        line.key(keys::HOME);
        line.key(keys::DELETE);
        line.key(SELECT_END);
        line.key(SELECT_LEFT);
        assert_eq!(line.selected_text(), Some("BC".encode_utf16().collect()));
        typing(&mut line, "x");
        line.draw(&mut c);
        assert_eq!(row(&c, 0), ">xD..");
        line.key(keys::LEFT);
        line.key(DELETE_TO_END);
        line.draw(&mut c);
        assert_eq!((row(&c, 0).as_str(), c.cursor()), (">....", (1, 0)));
    }

    #[test]
    fn wraps_and_scrolls() {
        let mut c = console();
        let _ = c.locate(Some(2), Some(2));
        let mut line = LineEdit::new(&c);
        typing(&mut line, "ABCDEF");
        line.draw(&mut c);
        // A row went up to make room.
        assert_eq!(
            [row(&c, 0), row(&c, 1), row(&c, 2)],
            [".....", "..ABC", "DEF.."]
        );
        assert_eq!(c.cursor(), (3, 2));
        line.key(DELETE_LINE);
        line.draw(&mut c);
        assert_eq!([row(&c, 1), row(&c, 2)], [".....", "....."]);
        assert_eq!(c.cursor(), (2, 1));
        typing(&mut line, "XYZ");
        // A full last row needs no second newline.
        assert_eq!(
            line.finish(&mut c),
            "XYZ".encode_utf16().collect::<Vec<_>>()
        );
        assert_eq!(c.cursor(), (0, 2));
    }
}
