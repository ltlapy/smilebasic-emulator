//! The program editor that LIST and F4 open. It follows SmileBASIC 4's
//! editor loosely: line numbers on the left, a column ruler and the slot's
//! name at the bottom, and long lines either scrolled or wrapped.

use crate::font::Font;
use crate::framebuffer::Framebuffer;
use crate::runtime::{SLOTS, keys};
use crate::text::{TAB_WIDTH, TextScreen};

/// Six zero-padded digits and a space, as SmileBASIC 4 shows them.
const GUTTER: usize = 7;
/// Rows below the code: the ruler and the status line.
const FOOTER: usize = 2;

const BACKGROUND: u32 = 0xFF00_0000;
const TEXT: u32 = 0xFFFF_FFFF;
const LINE_NUMBER: u32 = 0xFF80_8080;
const CURRENT_LINE: u32 = 0xFF20_2838;
const BAR: u32 = 0xFF30_3040;
const DIM: u32 = 0xFF70_7080;
const RETURN_MARK: u32 = 0xFF50_5868;

/// The mark drawn after each line's last character.
const RETURN_GLYPH: [u8; 8] = [
    0b0000_0000,
    0b0000_0010,
    0b0000_0010,
    0b0010_0010,
    0b0110_0010,
    0b1111_1110,
    0b0110_0000,
    0b0010_0000,
];

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct View {
    line: usize,
    col: usize,
    top: usize,
    left: usize,
}

pub struct Editor {
    slot: usize,
    lines: Vec<Vec<u16>>,
    view: View,
    /// The column Up and Down return to after passing shorter lines.
    want_col: usize,
    /// Typing replaces the character under the cursor (Insert toggles it).
    pub overwrite: bool,
    modified: bool,
    /// Where each slot was left, restored when it is opened again.
    saved: [View; SLOTS],
    screen: TextScreen,
}

/// Splits a program into lines the way the lexer counts them.
fn split_lines(source: &str) -> Vec<Vec<u16>> {
    let text: Vec<u16> = source.encode_utf16().collect();
    let mut lines = vec![Vec::new()];
    let mut i = 0;
    while i < text.len() {
        match text[i] {
            13 if text.get(i + 1) == Some(&10) => {
                lines.push(Vec::new());
                i += 1;
            }
            10 | 13 => lines.push(Vec::new()),
            c => lines.last_mut().expect("never empty").push(c),
        }
        i += 1;
    }
    lines
}

impl Editor {
    /// An editor for a screen of `screen_w` x `screen_h` pixels.
    pub fn new(screen_w: u32, screen_h: u32) -> Editor {
        Editor {
            slot: 0,
            lines: vec![Vec::new()],
            view: View::default(),
            want_col: 0,
            overwrite: false,
            modified: false,
            saved: [View::default(); SLOTS],
            screen: TextScreen::new(screen_w, screen_h),
        }
    }

    /// The slot being edited, or last edited.
    pub fn slot(&self) -> usize {
        self.slot
    }

    /// The cursor as a 1-based line and 0-based column.
    pub fn cursor(&self) -> (usize, usize) {
        (self.view.line + 1, self.view.col)
    }

    pub fn text(&self) -> String {
        let lines: Vec<String> = self
            .lines
            .iter()
            .map(|l| String::from_utf16_lossy(l))
            .collect();
        lines.join("\n")
    }

    /// Starts editing `source` from `slot`, at a 1-based line or where the
    /// slot was left.
    pub fn open(&mut self, slot: usize, source: &str, line: Option<usize>) {
        self.slot = slot;
        self.lines = split_lines(source);
        self.modified = false;
        self.view = self.saved[slot];
        if let Some(line) = line {
            self.view.line = line.saturating_sub(1);
            self.view.col = 0;
        }
        self.view.line = self.view.line.min(self.lines.len() - 1);
        self.view.col = self.view.col.min(self.lines[self.view.line].len());
        self.want_col = self.view.col;
    }

    /// Stops editing. Returns the new source if it was changed, so that an
    /// untouched program keeps its original line endings.
    pub fn close(&mut self) -> Option<String> {
        self.saved[self.slot] = self.view;
        std::mem::take(&mut self.modified).then(|| self.text())
    }

    fn size(&self) -> (usize, usize) {
        let (w, h) = self.screen.size();
        (
            (w as usize).saturating_sub(GUTTER).max(1),
            (h as usize).saturating_sub(FOOTER).max(1),
        )
    }

    fn line(&self) -> &[u16] {
        &self.lines[self.view.line]
    }

    fn go(&mut self, line: usize, col: usize) {
        self.view.line = line.min(self.lines.len() - 1);
        self.view.col = col.min(self.line().len());
    }

    /// Moves to another line, keeping the wanted column.
    fn go_line(&mut self, line: usize) {
        self.go(line, self.want_col);
    }

    fn insert(&mut self, code: u16) {
        let View { line, col, .. } = self.view;
        let text = &mut self.lines[line];
        if self.overwrite && col < text.len() {
            text[col] = code;
        } else {
            text.insert(col, code);
        }
        self.view.col += 1;
        self.modified = true;
    }

    fn split(&mut self) {
        let View { line, col, .. } = self.view;
        let rest = self.lines[line].split_off(col);
        self.lines.insert(line + 1, rest);
        self.go(line + 1, 0);
        self.modified = true;
    }

    /// Joins the line after `line` onto it.
    fn join(&mut self, line: usize) {
        if line + 1 < self.lines.len() {
            let next = self.lines.remove(line + 1);
            self.lines[line].extend(next);
            self.modified = true;
        }
    }

    /// Edits with a typed character or one of the [`keys`] codes.
    pub fn key(&mut self, code: u16) {
        let View { line, col, .. } = self.view;
        let (_, rows) = self.size();
        let keeps_column = matches!(
            code,
            keys::UP | keys::DOWN | keys::PAGE_UP | keys::PAGE_DOWN
        );
        match code {
            keys::ENTER => self.split(),
            keys::BACKSPACE if col > 0 => {
                self.lines[line].remove(col - 1);
                self.view.col -= 1;
                self.modified = true;
            }
            keys::BACKSPACE if line > 0 => {
                let len = self.lines[line - 1].len();
                self.join(line - 1);
                self.go(line - 1, len);
            }
            keys::DELETE if col < self.line().len() => {
                self.lines[line].remove(col);
                self.modified = true;
            }
            keys::DELETE => self.join(line),
            keys::LEFT if col > 0 => self.view.col -= 1,
            keys::LEFT if line > 0 => self.go(line - 1, usize::MAX),
            keys::RIGHT if col < self.line().len() => self.view.col += 1,
            keys::RIGHT if line + 1 < self.lines.len() => self.go(line + 1, 0),
            keys::UP => self.go_line(line.saturating_sub(1)),
            keys::DOWN => self.go_line(line + 1),
            keys::PAGE_UP => self.go_line(line.saturating_sub(rows)),
            keys::PAGE_DOWN => self.go_line(line + rows),
            keys::HOME => self.view.col = 0,
            keys::END => self.view.col = self.line().len(),
            // UNVERIFIED: Tab pads with spaces to the next TABSTEP column.
            keys::TAB => {
                let tab = TAB_WIDTH as usize;
                for _ in 0..tab - col % tab {
                    self.insert(u16::from(b' '));
                }
            }
            keys::INSERT => self.overwrite = !self.overwrite,
            0..0x20 | 0x7F => {}
            c => self.insert(c),
        }
        if !keeps_column {
            self.want_col = self.view.col;
        }
    }

    /// Moves to the first line (Ctrl+Home, FILETOP).
    pub fn file_top(&mut self) {
        self.go(0, 0);
        self.want_col = 0;
    }

    /// Moves to the end of the last line (Ctrl+End, FILEEND).
    pub fn file_end(&mut self) {
        self.go(usize::MAX, usize::MAX);
        self.want_col = self.view.col;
    }

    /// Scrolls so that the cursor is on screen.
    fn scroll(&mut self, wrap: bool) {
        let (cols, rows) = self.size();
        let v = &mut self.view;
        v.top = v.top.min(v.line);
        if wrap {
            v.left = 0;
            let cursor_row = v.col / cols;
            loop {
                let above: usize = (v.top..v.line).map(|l| rows_of(&self.lines[l], cols)).sum();
                if above + cursor_row < rows || v.top == v.line {
                    break;
                }
                v.top += 1;
            }
        } else {
            if v.line >= v.top + rows {
                v.top = v.line + 1 - rows;
            }
            v.left = v.left.min(v.col);
            if v.col >= v.left + cols {
                v.left = v.col + 1 - cols;
            }
        }
    }

    /// Draws the editor over the whole frame. `name` is the slot's file
    /// name; `blink` shows the cursor in this frame.
    pub fn draw(&mut self, fb: &mut Framebuffer, font: &Font, name: &str, wrap: bool, blink: bool) {
        self.scroll(wrap);
        let (cols, rows) = self.size();
        let v = self.view;
        fb.clear(BACKGROUND);
        let screen = &mut self.screen;
        screen.cls();
        let width = screen.size().0 as usize;
        let mut marks = Vec::new();
        let mut cursor = None;

        let mut row = 0;
        let mut line = v.top;
        while row < rows && line < self.lines.len() {
            let text = &self.lines[line];
            let current = line == v.line;
            let chunks = if wrap { rows_of(text, cols) } else { 1 };
            for chunk in 0..chunks {
                if row >= rows {
                    break;
                }
                if current {
                    fill_row(fb, row, 0, width, CURRENT_LINE);
                }
                if chunk == 0 {
                    let number = format!("{:06}", line + 1);
                    let color = if current { TEXT } else { LINE_NUMBER };
                    put_str(screen, 0, row, &number, color);
                }
                let start = if wrap { chunk * cols } else { v.left };
                for x in 0..cols {
                    match text.get(start + x) {
                        Some(&c) => screen.put_at((GUTTER + x) as u32, row as u32, c, TEXT),
                        None => {
                            if start + x == text.len() {
                                marks.push((GUTTER + x, row));
                            }
                            break;
                        }
                    }
                }
                if current && v.col >= start && v.col - start < cols {
                    cursor = Some((GUTTER + v.col - start, row));
                }
                row += 1;
            }
            line += 1;
        }

        // The ruler numbers the visible columns, every other one.
        let ruler = rows;
        fill_row(fb, ruler, 0, width, BAR);
        let first = if wrap { 0 } else { v.left };
        let cursor_col = if wrap { v.col % cols } else { v.col };
        for x in 0..cols {
            let col = first + x;
            let at = (GUTTER + x) as u32;
            if col == cursor_col {
                fill_row(fb, ruler, GUTTER + x, 1, TEXT);
                let digit = b'0' + (col % 10) as u8;
                screen.put_at(at, ruler as u32, u16::from(digit), BACKGROUND);
            } else if col.is_multiple_of(2) {
                let digit = b'0' + (col % 10) as u8;
                let color = if col.is_multiple_of(10) { TEXT } else { DIM };
                screen.put_at(at, ruler as u32, u16::from(digit), color);
            }
        }

        let status = rows + 1;
        fill_row(fb, status, 0, width, BAR);
        put_str(screen, 0, status, &format!("PRG{} {name}", self.slot), TEXT);

        screen.draw(fb, font);
        for (x, y) in marks {
            draw_glyph(fb, x, y, &RETURN_GLYPH, RETURN_MARK);
        }
        if let (Some((x, y)), true) = (cursor, blink) {
            // A bar for inserting, a block for overwriting.
            let w = if self.overwrite { 8 } else { 2 };
            for py in 0..8 {
                for px in 0..w {
                    fb.set((x * 8 + px) as u32, (y * 8 + py) as u32, TEXT);
                }
            }
        }
    }
}

/// Screen rows taken by a line when wrapping at `cols`; the last row
/// keeps room for the cursor and the return mark.
fn rows_of(line: &[u16], cols: usize) -> usize {
    line.len() / cols + 1
}

fn put_str(screen: &mut TextScreen, x: usize, y: usize, text: &str, color: u32) {
    for (i, c) in text.encode_utf16().enumerate() {
        screen.put_at((x + i) as u32, y as u32, c, color);
    }
}

/// Fills `w` cells of a text row from column `x`.
fn fill_row(fb: &mut Framebuffer, row: usize, x: usize, w: usize, color: u32) {
    for py in row * 8..(row + 1) * 8 {
        for px in x * 8..(x + w) * 8 {
            fb.set(px as u32, py as u32, color);
        }
    }
}

fn draw_glyph(fb: &mut Framebuffer, x: usize, y: usize, glyph: &[u8; 8], color: u32) {
    for (py, bits) in glyph.iter().enumerate() {
        for px in 0..8 {
            if bits & (0x80 >> px) != 0 {
                fb.set((x * 8 + px) as u32, (y * 8 + py) as u32, color);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn editor(source: &str) -> Editor {
        let mut e = Editor::new(400, 240);
        e.open(0, source, None);
        e
    }

    fn typing(e: &mut Editor, text: &str) {
        for c in text.encode_utf16() {
            e.key(c);
        }
    }

    #[test]
    fn splits_lines_like_the_lexer() {
        let e = editor("A\r\nB\rC\nD");
        assert_eq!(e.text(), "A\nB\nC\nD");
        assert_eq!(editor("").lines.len(), 1);
        assert_eq!(editor("A\n").lines.len(), 2);
    }

    #[test]
    fn type_split_and_join() {
        let mut e = editor("PRINT 1");
        e.key(keys::END);
        typing(&mut e, "23");
        e.key(keys::LEFT);
        e.key(keys::ENTER);
        assert_eq!(e.text(), "PRINT 12\n3");
        assert_eq!(e.cursor(), (2, 0));
        e.key(keys::BACKSPACE);
        assert_eq!((e.text().as_str(), e.cursor()), ("PRINT 123", (1, 8)));
        e.key(keys::END);
        e.key(keys::DELETE);
        assert_eq!(e.text(), "PRINT 123");
        assert_eq!(e.close().as_deref(), Some("PRINT 123"));
    }

    #[test]
    fn untouched_source_is_not_returned() {
        let mut e = editor("A\r\nB");
        e.key(keys::DOWN);
        e.key(keys::RIGHT);
        assert_eq!(e.close(), None);
    }

    #[test]
    fn up_and_down_keep_the_column() {
        let mut e = editor("ABCDEF\nAB\nABCDEF");
        e.key(keys::END);
        e.key(keys::DOWN);
        assert_eq!(e.cursor(), (2, 2));
        e.key(keys::DOWN);
        assert_eq!(e.cursor(), (3, 6));
        e.key(keys::LEFT);
        e.key(keys::UP);
        assert_eq!(e.cursor(), (2, 2));
        e.key(keys::RIGHT);
        assert_eq!(e.cursor(), (3, 0));
        e.key(keys::LEFT);
        assert_eq!(e.cursor(), (2, 2));
    }

    #[test]
    fn overwrite_and_tab() {
        let mut e = editor("ABC");
        e.key(keys::INSERT);
        typing(&mut e, "XY");
        assert_eq!(e.text(), "XYC");
        e.key(keys::INSERT);
        e.key(keys::HOME);
        e.key(keys::TAB);
        assert_eq!((e.text().as_str(), e.cursor()), ("    XYC", (1, 4)));
        // Control codes other than the editing keys type nothing.
        e.key(keys::ESCAPE);
        assert_eq!(e.text(), "    XYC");
    }

    #[test]
    fn open_restores_where_a_slot_was_left() {
        let mut e = editor("A\nB\nC");
        e.key(keys::DOWN);
        e.key(keys::DOWN);
        e.close();
        e.open(1, "X", None);
        assert_eq!(e.cursor(), (1, 0));
        e.close();
        e.open(0, "A\nB\nC", None);
        assert_eq!(e.cursor(), (3, 0));
        e.open(0, "A", None);
        assert_eq!(e.cursor(), (1, 0), "clamped to a shorter program");
        e.open(0, "A\nB\nC", Some(2));
        assert_eq!(e.cursor(), (2, 0));
    }

    #[test]
    fn scrolls_to_the_cursor() {
        let mut e = editor(&"X\n".repeat(100));
        e.key(keys::PAGE_DOWN);
        e.scroll(false);
        assert_eq!((e.cursor().0, e.view.top), (29, 1));
        e.file_end();
        e.scroll(false);
        assert_eq!(e.view.top, 101 - 28);
        e.file_top();
        e.scroll(false);
        assert_eq!(e.view.top, 0);

        let mut e = editor(&"A".repeat(100));
        e.key(keys::END);
        e.scroll(false);
        assert_eq!(e.view.left, 100 + 1 - 43);
        e.scroll(true);
        assert_eq!(e.view.left, 0);
    }

    #[test]
    fn wrapped_lines_take_more_rows() {
        let long = "A".repeat(43 * 3);
        let mut e = editor(&format!("{long}\n").repeat(10));
        e.go(9, 0);
        e.scroll(true);
        // Each line takes four rows (the last for its return mark).
        assert_eq!(e.view.top, 3);
        e.file_end();
        e.scroll(true);
        assert_eq!(e.view.top, 4);
    }

    #[test]
    fn draws_numbers_ruler_and_status() {
        let mut e = editor("PRINT 1");
        let mut fb = Framebuffer::new(400, 240);
        e.draw(&mut fb, &Font::blank(), "GAME", false, true);
        let row = |y: u32| -> String {
            (0..50)
                .map(|x| e.screen.cell(x, y).code)
                .map(|c| {
                    if c == 0 {
                        ' '
                    } else {
                        char::from_u32(c.into()).unwrap()
                    }
                })
                .collect::<String>()
                .trim_end()
                .to_string()
        };
        assert_eq!(row(0), "000001 PRINT 1");
        assert!(row(28).starts_with("       0 2 4 6 8 0 2"));
        assert_eq!(row(29), "PRG0 GAME");
        // The cursor bar at column 0 of the code.
        assert_eq!(fb.get(7 * 8, 0), Some(TEXT));
        assert_eq!(fb.get(7 * 8 + 2, 0), Some(CURRENT_LINE));
    }
}
