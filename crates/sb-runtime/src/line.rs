//! The line typed in direct mode or for INPUT and LINPUT, edited where
//! it is on the console: the cursor moves along it, Shift selects in
//! direct mode, and the line is drawn again after each change.

use crate::complete::Completion;
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

/// SmileBASIC 4's codes of keys that are not characters, as `PUSHKEY` takes
/// them and they wait in the key buffers. INKEY$ gives other values for them:
/// see [`crate::runtime::key_value`].
pub mod codes {
    pub const UP: u32 = 0x10000;
    pub const DOWN: u32 = 0x10001;
    pub const LEFT: u32 = 0x10002;
    pub const RIGHT: u32 = 0x10003;
    pub const BACKSPACE: u32 = 0x10004;
    pub const DELETE: u32 = 0x10005;
    pub const PAGE_UP: u32 = 0x10006;
    pub const PAGE_DOWN: u32 = 0x10007;
    pub const TAB: u32 = 0x10008;
    pub const HOME: u32 = 0x10009;
    pub const END: u32 = 0x1000A;
    pub const FILE_TOP: u32 = 0x1000C;
    pub const FILE_END: u32 = 0x1000D;
    pub const DELETE_LINE: u32 = 0x1000F;
    pub const SELECT_START: u32 = 0x10013;
    pub const SELECT_STOP: u32 = 0x10014;
    pub const COPY: u32 = 0x10015;
    pub const CUT: u32 = 0x10016;
    pub const PASTE: u32 = 0x10017;
    pub const CLEAR_LINE: u32 = 0x10018;
    pub const ESCAPE: u32 = 0x10029;
    pub const INSERT: u32 = 0x10053;
    pub const ENTER: u32 = 0x0D;
    /// Marks a key a controller typed, which `INKEY$(1)` passes over.
    pub const FROM_PAD: u32 = 0x20000;
    /// The code itself, without [`FROM_PAD`].
    pub const CODE: u32 = FROM_PAD - 1;
}

/// The most characters a line holds; one more typed is dropped, whatever
/// the console's size.
pub const LINE_MAX: usize = 256;

/// A line end, as CLIPBOARD's text can put one into the line.
const LINE_END: u16 = 0x0A;

/// How many of the console's rows a line keeps free under its cursor: the
/// one below it and those the hint bar at the bottom of the display covers
/// whole. `height` and `aspect` are XSCREEN's, `cell` the console's
/// character size. The bar is 16 pixels of a 1280x720 display.
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

const S_LEFT: u32 = commands::SELECT_LEFT as u32;
const S_RIGHT: u32 = commands::SELECT_RIGHT as u32;
const S_HOME: u32 = commands::SELECT_HOME as u32;
const S_END: u32 = commands::SELECT_END as u32;
const S_ALL: u32 = commands::SELECT_ALL as u32;
const D_LINE: u32 = commands::DELETE_LINE as u32;
const D_END: u32 = commands::DELETE_TO_END as u32;

#[derive(Clone, Debug)]
pub struct LineEdit {
    pub text: Vec<u16>,
    pub cursor: usize,
    /// Whether the selection commands select: only direct mode's line
    /// does. For INPUT and LINPUT they just move the cursor.
    pub selects: bool,
    /// The other end of the selection.
    anchor: Option<usize>,
    /// `SELECT_START` was given: plain moves keep the selection's other end.
    selecting: bool,
    /// Where the line starts on the console, counted in cells from its
    /// top left. Scrolling moves it up, above the console if need be.
    origin: i64,
    /// Cells from the origin drawn last time, cleared if the line got
    /// shorter.
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
            selecting: false,
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
        if w == 0 {
            return None;
        }
        let p = self.place(i, w);
        (p >= 0).then(|| (p % w, p / w))
    }

    /// Where character `i` of the line goes, or the cursor before it, in
    /// cells from the console's top left on a console `w` wide. A line end
    /// pasted into the line shows as a line break: what follows it starts
    /// the next row.
    fn place(&self, i: usize, w: i64) -> i64 {
        let before = &self.text[..i.min(self.text.len())];
        if !before.contains(&LINE_END) {
            return self.origin + i as i64;
        }
        before.iter().fold(self.origin, |p, &c| match c {
            LINE_END => (p.div_euclid(w) + 1) * w,
            _ => p + 1,
        })
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
        self.drop_selection();
    }

    /// Ends the selection, leaving the text and the cursor as they are.
    pub fn drop_selection(&mut self) {
        self.anchor = None;
        self.selecting = false;
    }

    fn delete_selection(&mut self) -> bool {
        let Some((from, to)) = self.selection() else {
            self.drop_selection();
            return false;
        };
        self.text.drain(from..to);
        self.cursor = from;
        self.drop_selection();
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
        } else if !self.selecting {
            self.anchor = None;
        }
        self.cursor = to.min(self.text.len());
    }

    /// Edits with a character, one of the [`codes`] or one of the
    /// [`commands`]. Enter, the history keys and the clipboard's are the
    /// caller's. Any code under `&H10000` goes in as a character, control
    /// characters too, as `PUSHKEY` can type them. Tells whether the cursor's
    /// blink starts again, as it does for all but a move that could not move.
    pub fn key(&mut self, code: impl Into<u32>) -> bool {
        let code = code.into();
        let before = (self.cursor, self.anchor);
        self.edit(code);
        let moves = matches!(code, codes::LEFT | codes::RIGHT | S_LEFT | S_RIGHT);
        code != codes::INSERT && (!moves || (self.cursor, self.anchor) != before)
    }

    fn edit(&mut self, code: u32) {
        let len = self.text.len();
        let left = self.cursor.saturating_sub(1);
        match code {
            codes::LEFT => self.move_to(left, false),
            codes::RIGHT => self.move_to(self.cursor + 1, false),
            codes::HOME => self.move_to(0, false),
            codes::END => self.move_to(len, false),
            S_LEFT => self.move_to(left, true),
            S_RIGHT => self.move_to(self.cursor + 1, true),
            S_HOME => self.move_to(0, true),
            S_END => self.move_to(len, true),
            S_ALL if self.selects => {
                self.anchor = Some(0);
                self.cursor = len;
            }
            S_ALL => {}
            codes::SELECT_START if self.selects => {
                self.anchor = Some(self.cursor);
                self.selecting = true;
            }
            codes::SELECT_STOP => self.drop_selection(),
            D_LINE | codes::CLEAR_LINE => self.set_text(Vec::new()),
            D_END => {
                self.drop_selection();
                self.text.truncate(self.cursor);
            }
            codes::BACKSPACE if self.delete_selection() => {}
            codes::BACKSPACE if self.cursor > 0 => {
                self.cursor -= 1;
                self.text.remove(self.cursor);
            }
            codes::DELETE if self.delete_selection() => {}
            codes::DELETE if self.cursor < len => {
                self.text.remove(self.cursor);
            }
            codes::INSERT => self.overwrite = !self.overwrite,
            // Tab types a space.
            codes::TAB => self.type_char(0x20),
            0x1_0000.. => {}
            c => self.type_char(c as u16),
        }
    }

    fn type_char(&mut self, c: u16) {
        let len = self.text.len();
        if self.overwrite {
            // As in SmileBASIC 4, the selection goes after the character is
            // written and the cursor moved on, so it can take the character.
            // UNVERIFIED: read, not yet seen.
            match self.text.get_mut(self.cursor) {
                Some(old) => *old = c,
                None if len < LINE_MAX => self.text.push(c),
                None => {}
            }
            self.cursor = (self.cursor + 1).min(self.text.len());
            self.delete_selection();
        } else if len < LINE_MAX {
            // A full line keeps its selection too.
            self.delete_selection();
            self.text.insert(self.cursor, c);
            self.cursor += 1;
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
        let end = self.place(self.text.len().saturating_sub(1), w) / w;
        let at = self.place(self.cursor, w) / w;
        let rows = (end - (h - 1)).max(at - (h - 1 - self.free).max(0));
        if rows > 0 {
            console.scroll(0, rows as i32);
            self.origin -= rows * w;
        }
        let (color, plain) = (console.color(), console.attr);
        let selection = self.selection();
        let empty = Cell {
            code: 0,
            color: 0,
            attr: 0,
        };
        let mut set = |p: i64, cell: Cell| {
            if p >= 0 {
                console.set_cell((p % w) as i32, (p / w) as i32, cell);
            }
        };
        let mut p = self.origin;
        for (i, &code) in self.text.iter().enumerate() {
            if code == LINE_END {
                // The rest of the row stays empty.
                let next = (p.div_euclid(w) + 1) * w;
                (p..next).for_each(|p| set(p, empty));
                p = next;
                continue;
            }
            let selected = selection.is_some_and(|(a, b)| a <= i && i < b);
            let attr = if selected {
                plain | attr::INVERT
            } else {
                plain
            };
            set(p, Cell { code, color, attr });
            p += 1;
        }
        let drawn = (p - self.origin) as usize;
        (drawn..self.shown).for_each(|i| set(self.origin + i as i64, empty));
        self.shown = drawn;
        self.place_cursor(console);
    }

    /// Puts the console's cursor at the line's.
    fn place_cursor(&self, console: &mut TextScreen) {
        let w = i64::from(console.size().0);
        let p = self.place(self.cursor, w).max(0);
        let _ = console.locate(Some((p % w) as i32), Some((p / w) as i32));
    }

    /// Ends the line with Enter: the console goes on from the row under the
    /// cursor, wherever on the line that is. So a line that fills its last row
    /// leaves an empty row, as the cursor is on the next one already, and
    /// Enter after Home has what follows printed over the line's second row.
    pub fn finish(mut self, console: &mut TextScreen) -> Vec<u16> {
        // A selection stays on the console: Enter does not draw the line
        // again.
        self.draw(console);
        console.newline();
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
        line.key(codes::LEFT);
        line.key(codes::LEFT);
        typing(&mut line, "B");
        line.draw(&mut c);
        assert_eq!(row(&c, 0), ">ABCD");
        assert_eq!(c.cursor(), (3, 0));
        line.key(codes::HOME);
        line.key(codes::DELETE);
        line.key(SELECT_END);
        line.key(SELECT_LEFT);
        assert_eq!(line.selected_text(), Some("BC".encode_utf16().collect()));
        typing(&mut line, "x");
        line.draw(&mut c);
        assert_eq!(row(&c, 0), ">xD..");
        line.key(codes::LEFT);
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
        line.key(codes::RIGHT);
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
        line.key(codes::BACKSPACE);
        assert_eq!(line.text, "ACD".encode_utf16().collect::<Vec<_>>());
        assert_eq!(line.selected_text(), None);
    }

    /// TEST60: the codes `PUSHKEY` takes. Selecting goes on through plain
    /// moves, and Tab types a space where a control character goes in as it is.
    #[test]
    fn the_codes_select_and_type() {
        let mut c = console();
        let mut line = direct_line(&c);
        typing(&mut line, "ABCD");
        line.key(codes::SELECT_START);
        line.key(codes::LEFT);
        line.key(codes::LEFT);
        assert_eq!(line.selected_text(), Some("CD".encode_utf16().collect()));
        line.drop_selection();
        assert_eq!(line.selected_text(), None);
        line.key(codes::LEFT);
        assert_eq!(line.selected_text(), None);
        line.key(codes::END);
        line.key(codes::TAB);
        line.key(9u16);
        assert_eq!(line.text, [65, 66, 67, 68, 32, 9]);
        line.key(codes::HOME);
        line.key(0x10006u32);
        assert_eq!(line.cursor, 0);
        line.draw(&mut c);
        assert_eq!(c.cell(4, 0).code, 32);
    }

    /// TEST60: Enter goes on from under the cursor.
    #[test]
    fn enter_goes_on_under_the_cursor() {
        let mut big = TextScreen::new(80, 80);
        // A row and a half, Enter on the first row: the second is written over.
        let mut line = direct_line(&big);
        typing(&mut line, "ABCDEFGHIJKLMNO");
        line.key(codes::HOME);
        line.finish(&mut big);
        assert_eq!(big.cursor(), (0, 1));
        // Exactly a row: the cursor is on the next, and Enter leaves it empty.
        big.locate(Some(0), Some(0)).unwrap();
        let mut line = direct_line(&big);
        typing(&mut line, "ABCDEFGHIJ");
        line.finish(&mut big);
        assert_eq!(big.cursor(), (0, 2));
    }

    #[test]
    fn overwrites_after_insert() {
        let mut c = console();
        let mut line = direct_line(&c);
        typing(&mut line, "ABC");
        assert!(!line.key(codes::INSERT));
        assert!(line.overwrite);
        line.key(codes::HOME);
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
        line.key(codes::HOME);
        line.key(codes::RIGHT);
        line.key(SELECT_RIGHT);
        line.key(SELECT_RIGHT);
        typing(&mut line, "Q");
        assert_eq!(line.text, "X".encode_utf16().collect::<Vec<_>>());
        assert_eq!(line.cursor, 1);
        // One made leftwards keeps it, and loses what follows it.
        line.set_text("ABCD".encode_utf16().collect());
        line.key(codes::LEFT);
        line.key(SELECT_LEFT);
        line.key(SELECT_LEFT);
        typing(&mut line, "Q");
        assert_eq!(line.text, "AQD".encode_utf16().collect::<Vec<_>>());
        assert_eq!(line.cursor, 2);
        line.key(codes::INSERT);
        assert!(!line.overwrite);
    }

    #[test]
    fn a_pasted_line_end_shows_as_a_line_break() {
        let mut c = TextScreen::new(40, 48);
        let _ = c.locate(Some(1), Some(0));
        let mut line = LineEdit::new(&c, 1);
        let text: Vec<u16> = "?1\n?2".encode_utf16().collect();
        line.replace_before(0, &text);
        line.draw(&mut c);
        assert_eq!(
            [row(&c, 0), row(&c, 1), row(&c, 2)],
            [".?1..", "?2...", "....."]
        );
        assert_eq!(c.cursor(), (2, 1));
        assert_eq!(line.cell_of(3, 5), Some((0, 1)));
        // Taking the line end out joins the rows again.
        line.key(codes::HOME);
        for _ in 0..3 {
            line.key(codes::RIGHT);
        }
        line.key(codes::BACKSPACE);
        line.draw(&mut c);
        assert_eq!([row(&c, 0), row(&c, 1)], [".?1?2", "....."]);
        assert_eq!(
            line.finish(&mut c),
            "?1?2".encode_utf16().collect::<Vec<_>>()
        );
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
        line.key(codes::HOME);
        line.draw(&mut c);
        assert_eq!(
            [row(&c, 0), row(&c, 1), row(&c, 2)],
            ["ABCDE", "FGHIJ", "K...."]
        );
        assert_eq!(c.cursor(), (0, 0));
        line.key(codes::END);
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
        line.key(codes::LEFT);
        line.key(SELECT_LEFT);
        line.key(0x41u16);
        assert_eq!(line.text.len(), LINE_MAX);
        assert_eq!(line.selection(), Some((254, 255)));
        line.key(codes::END);
        // Overwriting at the end adds nothing either.
        line.key(codes::INSERT);
        line.key(0x41u16);
        assert_eq!(line.text.len(), LINE_MAX);
        assert_eq!(line.cursor, LINE_MAX);
        line.key(codes::INSERT);
        // Backspace makes room for one again.
        line.key(codes::BACKSPACE);
        assert_eq!(line.text.len(), LINE_MAX - 1);
        line.key(0x42u16);
        line.key(0x43u16);
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
