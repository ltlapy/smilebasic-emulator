//! The line typed in direct mode or for INPUT and LINPUT, edited where
//! it is on the console: the cursor moves along it, Shift selects in
//! direct mode, and the line is drawn again after each change.

use crate::complete::Completion;
use crate::runtime::keys;
use crate::text::{Cell, TextScreen, attr};

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

/// The most characters a line holds; one more typed is dropped, whatever
/// the console's size.
// UNVERIFIED: expected of SmileBASIC 4, not yet seen.
pub const LINE_MAX: usize = 256;

/// How many of the console's rows a line keeps free under its cursor: the
/// one below it and those the hint bar at the bottom of the display covers
/// whole. `height` and `aspect` are XSCREEN's, `cell` the console's
/// character size. The bar is 16 pixels of a 1280x720 display.
// UNVERIFIED: expected of SmileBASIC 4. One row kept free has been seen, on
// screens of 240 and 180 rows of pixels; the further rows the bar's term adds on
// finer screens have not.
pub fn rows_kept_free(height: u32, aspect: f64, cell: u32) -> i64 {
    let (height, aspect) = (height as f32, aspect as f32);
    let shown = if 720.0 / height <= 1280.0 / (aspect * height) {
        720.0
    } else {
        ((1280.0 / aspect) as i32 as f32).min(720.0)
    };
    1 + (16.0 / (shown / height * cell as f32)) as i64
}

/// Scrolls the console up so that its cursor is `free` rows or more above
/// its bottom, as all that direct mode, INPUT and LINPUT print does.
pub fn keep_rows_free(console: &mut TextScreen, free: i64) {
    let (x, y) = console.cursor();
    // UNVERIFIED: that a console too small for that keeps its first row.
    let last = (i64::from(console.size().1) - 1 - free).max(0);
    let rows = i64::from(y) - last;
    if rows > 0 {
        console.scroll(0, rows as i32);
        let _ = console.locate(Some(x as i32), Some(last as i32));
    }
}

#[derive(Clone, Debug)]
pub struct LineEdit {
    pub text: Vec<u16>,
    pub cursor: usize,
    /// Whether the selection commands select: only direct mode's line
    /// does. For INPUT and LINPUT they just move the cursor.
    pub selects: bool,
    /// The other end of the selection.
    anchor: Option<usize>,
    /// Where the line starts on the console, counted in cells from its
    /// top left. Scrolling moves it up, above the console if need be.
    origin: i64,
    /// Characters drawn last time, cleared if the line got shorter.
    shown: usize,
    /// The rows kept free under the cursor: see [`rows_kept_free`].
    free: i64,
    /// Insert has been pressed: a typed character replaces the one at the
    /// cursor, and the cursor is a block.
    pub overwrite: bool,
    /// Completion's list for the word being typed.
    pub completion: Option<Completion>,
}

impl LineEdit {
    /// A line starting at the console's cursor, keeping `free` rows free
    /// under its own.
    pub fn new(console: &TextScreen, free: i64) -> LineEdit {
        let (w, _) = console.size();
        let (x, y) = console.cursor();
        LineEdit {
            text: Vec::new(),
            cursor: 0,
            selects: false,
            anchor: None,
            origin: i64::from(y) * i64::from(w) + i64::from(x),
            shown: 0,
            free,
            overwrite: false,
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
            self.anchor = None;
            return false;
        };
        self.text.drain(from..to);
        self.cursor = from;
        self.anchor = None;
        true
    }

    /// Replaces the selection, or the text from `from` to the cursor,
    /// with `text`, if the line then still fits.
    pub fn replace_before(&mut self, from: usize, text: &[u16]) {
        let (start, end) = self.selection().unwrap_or((from, self.cursor));
        if self.text.len() - (end - start) + text.len() > LINE_MAX {
            return;
        }
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
        if select && self.selects {
            self.anchor.get_or_insert(self.cursor);
        } else {
            self.anchor = None;
        }
        self.cursor = to.min(self.text.len());
    }

    /// Edits with a typed character, a [`keys`] code or one of the
    /// [`commands`]. Enter and the history keys are the caller's. Tells
    /// whether the cursor's blink starts again, as it does for all but a
    /// move that could not move.
    pub fn key(&mut self, code: u16) -> bool {
        use commands::*;
        let before = (self.cursor, self.anchor);
        self.edit(code);
        let moves = matches!(code, keys::LEFT | keys::RIGHT | SELECT_LEFT | SELECT_RIGHT);
        code != keys::INSERT && (!moves || (self.cursor, self.anchor) != before)
    }

    fn edit(&mut self, code: u16) {
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
            SELECT_ALL if self.selects => {
                self.anchor = Some(0);
                self.cursor = len;
            }
            SELECT_ALL => {}
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
            keys::INSERT => self.overwrite = !self.overwrite,
            0..0x20 | 0x7F => {}
            // As in SmileBASIC 4, the selection goes after the character is
            // written and the cursor moved on, so it can take the character.
            // UNVERIFIED: read, not yet seen.
            c if self.overwrite => {
                match self.text.get_mut(self.cursor) {
                    Some(old) => *old = c,
                    None if len < LINE_MAX => self.text.push(c),
                    None => {}
                }
                self.cursor = (self.cursor + 1).min(self.text.len());
                self.delete_selection();
            }
            // A full line keeps its selection too.
            _ if len >= LINE_MAX => {}
            c => {
                self.delete_selection();
                self.text.insert(self.cursor, c);
                self.cursor += 1;
            }
        }
    }

    /// Draws the line where it started, scrolling the console up if its
    /// end would go below it or the cursor into the rows kept free, and
    /// puts the console's cursor at the line's.
    pub fn draw(&mut self, console: &mut TextScreen) {
        let (w, h) = console.size();
        let (w, h) = (i64::from(w), i64::from(h));
        if w == 0 || h == 0 {
            return;
        }
        let end = (self.origin + self.text.len().saturating_sub(1) as i64) / w;
        let at = (self.origin + self.cursor as i64) / w;
        let rows = (end - (h - 1)).max(at - (h - 1 - self.free).max(0));
        if rows > 0 {
            console.scroll(0, rows as i32);
            self.origin -= rows * w;
        }
        let (color, plain) = (console.color(), console.attr);
        let selection = self.selection();
        for i in 0..self.text.len().max(self.shown) {
            let p = self.origin + i as i64;
            if p < 0 {
                continue;
            }
            let cell = match self.text.get(i) {
                Some(&code) => {
                    let selected = selection.is_some_and(|(a, b)| a <= i && i < b);
                    // UNVERIFIED: expected of SmileBASIC 4, not seen.
                    let attr = if selected {
                        plain | attr::INVERT
                    } else {
                        plain
                    };
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
        self.place_cursor(console);
    }

    /// Puts the console's cursor at the line's.
    fn place_cursor(&self, console: &mut TextScreen) {
        let w = i64::from(console.size().0);
        let p = (self.origin + self.cursor as i64).max(0);
        let _ = console.locate(Some((p % w) as i32), Some((p / w) as i32));
    }

    /// Ends the line with Enter: the console goes on below its end.
    pub fn finish(mut self, console: &mut TextScreen) -> Vec<u16> {
        // UNVERIFIED: that a selection stays on the console, as SmileBASIC
        // 4's Enter does not draw the line again.
        self.draw(console);
        self.cursor = self.text.len();
        let w = i64::from(console.size().0);
        if w != 0 {
            self.place_cursor(console);
        }
        // A line that fills its last row has its cursor on the next one
        // already.
        let p = self.origin + self.text.len() as i64;
        if self.text.is_empty() || w == 0 || p % w != 0 {
            console.newline();
        }
        keep_rows_free(console, self.free);
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

    fn direct_line(c: &TextScreen) -> LineEdit {
        let mut line = LineEdit::new(c, 1);
        line.selects = true;
        line
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
        let mut line = direct_line(&c);
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
    fn selected_characters_are_inverted_until_the_selection_goes() {
        let mut c = console();
        let mut line = direct_line(&c);
        typing(&mut line, "ABC");
        line.key(SELECT_LEFT);
        line.key(SELECT_LEFT);
        line.draw(&mut c);
        let inverted = |c: &TextScreen, x| c.cell(x, 0).attr & attr::INVERT != 0;
        assert_eq!(
            [0, 1, 2, 3].map(|x| inverted(&c, x)),
            [false, true, true, false]
        );
        assert_eq!(c.cell(1, 0).color, c.color());
        line.key(keys::RIGHT);
        line.draw(&mut c);
        assert!(!inverted(&c, 1) && !inverted(&c, 2));
        // Enter does not draw the line again.
        line.key(SELECT_LEFT);
        line.draw(&mut c);
        let text = line.finish(&mut c);
        assert_eq!(text.len(), 3);
        assert!(inverted(&c, 1) && !inverted(&c, 2));
        assert_eq!(c.cursor(), (0, 1));
    }

    /// TEST56: Shift with a move during INPUT moves the cursor and selects
    /// nothing, so that Backspace takes one character and nothing is drawn
    /// inverted.
    #[test]
    fn input_lines_do_not_select() {
        let mut c = console();
        let mut line = LineEdit::new(&c, 1);
        typing(&mut line, "ABCD");
        line.key(SELECT_LEFT);
        line.key(SELECT_LEFT);
        assert_eq!((line.cursor, line.selection()), (2, None));
        line.key(SELECT_ALL);
        assert_eq!((line.cursor, line.selection()), (2, None));
        line.draw(&mut c);
        assert_eq!(c.cell(2, 0).attr & attr::INVERT, 0);
        line.key(keys::BACKSPACE);
        assert_eq!(line.text, "ACD".encode_utf16().collect::<Vec<_>>());
        assert_eq!(line.selected_text(), None);
    }

    #[test]
    fn overwrites_after_insert() {
        let mut c = console();
        let mut line = direct_line(&c);
        typing(&mut line, "ABC");
        assert!(!line.key(keys::INSERT));
        assert!(line.overwrite);
        line.key(keys::HOME);
        typing(&mut line, "XY");
        assert_eq!(line.cursor, 2);
        line.draw(&mut c);
        assert_eq!(row(&c, 0), "XYC..");
        // Past the end it adds, as it does when inserting.
        typing(&mut line, "ZW");
        line.draw(&mut c);
        assert_eq!(row(&c, 0), "XYZW.");
        // A selection made rightwards takes the character written after
        // it too.
        line.key(keys::HOME);
        line.key(keys::RIGHT);
        line.key(SELECT_RIGHT);
        line.key(SELECT_RIGHT);
        typing(&mut line, "Q");
        assert_eq!(line.text, "X".encode_utf16().collect::<Vec<_>>());
        assert_eq!(line.cursor, 1);
        // One made leftwards keeps it, and loses what follows it.
        line.set_text("ABCD".encode_utf16().collect());
        line.key(keys::LEFT);
        line.key(SELECT_LEFT);
        line.key(SELECT_LEFT);
        typing(&mut line, "Q");
        assert_eq!(line.text, "AQD".encode_utf16().collect::<Vec<_>>());
        assert_eq!(line.cursor, 2);
        line.key(keys::INSERT);
        assert!(!line.overwrite);
    }

    #[test]
    fn wraps_and_scrolls() {
        let mut c = console();
        let _ = c.locate(Some(2), Some(1));
        let mut line = LineEdit::new(&c, 1);
        typing(&mut line, "ABC");
        line.draw(&mut c);
        // The cursor would go past the row above the last: a row goes up
        // for it, and the last stays free.
        assert_eq!(
            [row(&c, 0), row(&c, 1), row(&c, 2)],
            ["..ABC", ".....", "....."]
        );
        assert_eq!(c.cursor(), (0, 1));
        typing(&mut line, "DE");
        line.draw(&mut c);
        assert_eq!(row(&c, 1), "DE...");
        assert_eq!(c.cursor(), (2, 1));
        line.key(DELETE_LINE);
        line.draw(&mut c);
        assert_eq!([row(&c, 0), row(&c, 1)], [".....", "....."]);
        assert_eq!(c.cursor(), (2, 0));
        typing(&mut line, "XYZ");
        // A full last row needs no second newline.
        assert_eq!(
            line.finish(&mut c),
            "XYZ".encode_utf16().collect::<Vec<_>>()
        );
        assert_eq!(c.cursor(), (0, 1));
    }

    #[test]
    fn enter_keeps_the_last_row_free() {
        let mut c = console();
        for _ in 0..5 {
            LineEdit::new(&c, 1).finish(&mut c);
            assert!(c.cursor().1 <= 1);
        }
        assert_eq!(c.cursor(), (0, 1));
        // The program's PRINT can reach the last row; the next line starts
        // above it.
        let _ = c.locate(Some(1), Some(2));
        c.print(&[0x41]);
        keep_rows_free(&mut c, 1);
        assert_eq!(c.cursor(), (2, 1));
        assert_eq!(row(&c, 1), ".A...");
    }

    #[test]
    fn a_narrower_console_wraps_sooner_and_keeps_its_last_row_free() {
        // 26 x 12, as a program's TSCREEN 4 can make it.
        let mut c = TextScreen::new(208, 96);
        let _ = c.locate(Some(0), Some(9));
        let mut line = LineEdit::new(&c, 1);
        typing(&mut line, &"A".repeat(26));
        line.draw(&mut c);
        assert_eq!(c.cursor(), (0, 10), "the row above the last is still free");
        typing(&mut line, &"B".repeat(26));
        line.draw(&mut c);
        // Row 11 stays empty; the console scrolled instead.
        assert_eq!(c.cursor(), (0, 10));
        assert_eq!(c.cell(0, 9).code, u16::from(b'B'));
        assert_eq!(c.cell(0, 8).code, u16::from(b'A'));
        assert_eq!(c.cell(0, 11).code, 0);
    }

    #[test]
    fn moving_back_does_not_scroll_for_the_rest_of_the_line() {
        let mut c = console();
        let mut line = LineEdit::new(&c, 1);
        typing(&mut line, "ABCDEFGHIJK");
        line.key(keys::HOME);
        line.draw(&mut c);
        assert_eq!(
            [row(&c, 0), row(&c, 1), row(&c, 2)],
            ["ABCDE", "FGHIJ", "K...."]
        );
        assert_eq!(c.cursor(), (0, 0));
        line.key(keys::END);
        line.draw(&mut c);
        // The cursor went back to its row, which is the last: up it goes.
        assert_eq!([row(&c, 0), row(&c, 1)], ["FGHIJ", "K...."]);
        assert_eq!(c.cursor(), (1, 1));
    }

    #[test]
    fn holds_256_characters_whatever_they_are() {
        let mut c = TextScreen::new(400, 240);
        let mut line = direct_line(&c);
        typing(&mut line, &"\u{D55C}".repeat(300));
        assert_eq!(line.text.len(), LINE_MAX);
        assert_eq!(line.cursor, LINE_MAX);
        // Nor in the middle, and a full line's selection stays.
        line.key(keys::LEFT);
        line.key(SELECT_LEFT);
        line.key(0x41);
        assert_eq!(line.text.len(), LINE_MAX);
        assert_eq!(line.selection(), Some((254, 255)));
        line.key(keys::END);
        // Overwriting at the end adds nothing either.
        line.key(keys::INSERT);
        line.key(0x41);
        assert_eq!(line.text.len(), LINE_MAX);
        assert_eq!(line.cursor, LINE_MAX);
        line.key(keys::INSERT);
        // Backspace makes room for one again.
        line.key(keys::BACKSPACE);
        assert_eq!(line.text.len(), LINE_MAX - 1);
        line.key(0x42);
        line.key(0x43);
        assert_eq!(line.text.len(), LINE_MAX);
        assert_eq!(line.text[LINE_MAX - 1], 0x42);
        line.draw(&mut c);
    }

    #[test]
    fn a_word_that_does_not_fit_is_not_put_in() {
        let c = console();
        let mut line = LineEdit::new(&c, 1);
        typing(&mut line, &"A".repeat(LINE_MAX - 1));
        line.replace_before(LINE_MAX - 1, &[0x42, 0x43]);
        assert_eq!(line.text.len(), LINE_MAX - 1);
        line.replace_before(LINE_MAX - 2, &[0x42, 0x43]);
        assert_eq!(line.text.len(), LINE_MAX);
    }

    #[test]
    fn rows_kept_free_follow_the_screen() {
        assert_eq!(rows_kept_free(240, 400.0 / 240.0, 8), 1);
        assert_eq!(rows_kept_free(180, 320.0 / 180.0, 8), 1);
        // A row of 16 display pixels is covered whole by the bar...
        assert_eq!(rows_kept_free(360, 640.0 / 360.0, 8), 2);
        // ... and two of 8; with a larger character, one fewer.
        assert_eq!(rows_kept_free(720, 1280.0 / 720.0, 8), 3);
        assert_eq!(rows_kept_free(720, 1280.0 / 720.0, 16), 2);
        // A wide aspect shows the same rows smaller.
        assert_eq!(rows_kept_free(240, 640.0 / 240.0, 8), 2);
    }
}
