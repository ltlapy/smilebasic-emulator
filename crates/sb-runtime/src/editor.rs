//! The program editor that LIST and F4 open. It follows SmileBASIC 4's
//! editor loosely: line numbers on the left, a column ruler and the slot's
//! name at the bottom, and long lines either scrolled or wrapped. Text is
//! selected with Shift and the cursor keys or by dragging the mouse.

use crate::font::Font;
use crate::framebuffer::Framebuffer;
use crate::runtime::{SLOTS, keys};
use crate::text::{TAB_WIDTH, TextScreen};

/// Six digits on three number tiles (U+E100-E16E) and a thin cap.
const TILE_GUTTER: usize = 4;
/// Rows above the code, left for the emulator's menu bar.
const HEADER: usize = 1;
/// Rows below the code: the ruler and the status line.
const FOOTER: usize = 2;
/// Closes the number tiles on the right.
const TILE_CAP: u16 = 0xE17F;

const BACKGROUND: u32 = 0xFF00_0000;
const TEXT: u32 = 0xFFFF_FFFF;
const LINE_NUMBER: u32 = 0xFF80_8080;
const CURRENT_LINE: u32 = 0xFF20_2838;
const BAR: u32 = 0xFF30_3040;
const DIM: u32 = 0xFF70_7080;
const RETURN_MARK: u32 = 0xFF50_5868;
const SELECTION: u32 = 0xFF30_5090;

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

/// A line and a column in it.
type Pos = (usize, usize);

pub struct Editor {
    slot: usize,
    lines: Vec<Vec<u16>>,
    view: View,
    /// The column Up and Down return to after passing shorter lines.
    want_col: usize,
    /// Typing replaces the character under the cursor (Insert toggles it).
    pub overwrite: bool,
    /// The other end of the selection; the cursor is one end.
    anchor: Option<Pos>,
    /// Line numbers are drawn on SmileBASIC 4's number tiles rather than
    /// in digits, which fonts other than its own may lack.
    number_tiles: bool,
    /// The line and first column shown on each row of the last frame.
    layout: Vec<Pos>,
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
            anchor: None,
            number_tiles: true,
            layout: Vec::new(),
            modified: false,
            saved: [View::default(); SLOTS],
            screen: TextScreen::new(screen_w, screen_h),
        }
    }

    /// The slot being edited, or last edited.
    pub fn slot(&self) -> usize {
        self.slot
    }

    /// Whether the text changed since it was opened.
    pub fn is_modified(&self) -> bool {
        self.modified
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

    /// Starts editing `source` from `slot`, at a 1-based line and column
    /// (0 for the start of the line), or where the slot was left.
    pub fn open(&mut self, slot: usize, source: &str, at: Option<(usize, usize)>) {
        self.slot = slot;
        self.lines = split_lines(source);
        self.modified = false;
        self.view = self.saved[slot];
        if let Some((line, col)) = at {
            self.view.line = line.saturating_sub(1);
            self.view.col = col.saturating_sub(1);
        }
        self.view.line = self.view.line.min(self.lines.len() - 1);
        self.view.col = self.view.col.min(self.lines[self.view.line].len());
        self.want_col = self.view.col;
        self.anchor = None;
        self.layout.clear();
    }

    /// Stops editing. Returns the new source if it was changed, so that an
    /// untouched program keeps its original line endings.
    pub fn close(&mut self) -> Option<String> {
        self.saved[self.slot] = self.view;
        std::mem::take(&mut self.modified).then(|| self.text())
    }

    /// Columns before the code: the line numbers and a space. Digits take
    /// as many as the last line's number needs.
    fn gutter(&self) -> usize {
        if self.number_tiles {
            TILE_GUTTER
        } else {
            self.lines.len().to_string().len() + 1
        }
    }

    fn size(&self) -> (usize, usize) {
        let (w, h) = self.screen.size();
        (
            (w as usize).saturating_sub(self.gutter()).max(1),
            (h as usize).saturating_sub(HEADER + FOOTER).max(1),
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

    fn pos(&self) -> Pos {
        (self.view.line, self.view.col)
    }

    /// The selected text's start and end, if any is selected.
    fn selection(&self) -> Option<(Pos, Pos)> {
        let anchor = self.anchor?;
        let cursor = self.pos();
        (anchor != cursor).then(|| (anchor.min(cursor), anchor.max(cursor)))
    }

    /// Removes the selected text, if any, leaving the cursor where it was.
    fn delete_selection(&mut self) -> bool {
        let Some((start, end)) = self.selection() else {
            return false;
        };
        let rest = self.lines[end.0].split_off(end.1);
        self.lines[start.0].truncate(start.1);
        self.lines[start.0].extend(rest);
        self.lines.drain(start.0 + 1..=end.0);
        self.go(start.0, start.1);
        self.anchor = None;
        self.modified = true;
        true
    }

    /// The selected text, with lines ending in LF.
    pub fn copy(&self) -> Option<String> {
        let (start, end) = self.selection()?;
        let mut text = Vec::new();
        for line in start.0..=end.0 {
            let l = &self.lines[line];
            let from = if line == start.0 { start.1 } else { 0 };
            let to = if line == end.0 { end.1 } else { l.len() };
            text.extend_from_slice(&l[from..to]);
            if line < end.0 {
                text.push(10);
            }
        }
        Some(String::from_utf16_lossy(&text))
    }

    /// Takes the selected text out, for the clipboard.
    pub fn cut(&mut self) -> Option<String> {
        let text = self.copy()?;
        self.delete_selection();
        self.want_col = self.view.col;
        Some(text)
    }

    /// Puts text at the cursor in place of the selection. Insert mode
    /// does not apply.
    pub fn paste(&mut self, text: &str) {
        self.delete_selection();
        let mut parts = split_lines(text);
        let (line, col) = self.pos();
        let rest = self.lines[line].split_off(col);
        let last = parts.len() - 1;
        let end_col = parts[last].len() + if last == 0 { col } else { 0 };
        parts[last].extend(rest);
        let mut parts = parts.into_iter();
        self.lines[line].extend(parts.next().expect("never empty"));
        self.lines.splice(line + 1..line + 1, parts);
        self.go(line + last, end_col);
        self.want_col = self.view.col;
        self.modified |= !text.is_empty();
    }

    /// Selects the whole program.
    pub fn select_all(&mut self) {
        self.anchor = Some((0, 0));
        self.file_end();
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

    /// Moves the cursor with a [`keys`] code while keeping the other end
    /// of the selection, as Shift does.
    pub fn key_select(&mut self, code: u16) {
        let anchor = self.anchor.unwrap_or(self.pos());
        self.anchor = None;
        self.key(code);
        if is_movement(code) {
            self.anchor = Some(anchor);
        }
    }

    /// Edits with a typed character or one of the [`keys`] codes.
    pub fn key(&mut self, code: u16) {
        if self.selection().is_some() {
            match code {
                c if is_movement(c) => self.anchor = None,
                keys::BACKSPACE | keys::DELETE => {
                    self.delete_selection();
                    self.want_col = self.view.col;
                    return;
                }
                // Other control codes type nothing.
                c if c < 0x20 && c != keys::ENTER && c != keys::TAB || c == 0x7F => {}
                _ => {
                    self.delete_selection();
                }
            }
        }
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

    /// The text position under a pixel of the editor's screen, from the
    /// last frame drawn. Rows above or below the code reach one line
    /// further, so that dragging there scrolls.
    fn hit(&self, x: i32, y: i32) -> Option<Pos> {
        let (first, last) = (*self.layout.first()?, *self.layout.last()?);
        let col = (x.div_euclid(8) - self.gutter() as i32).max(0) as usize;
        // Wrapped lines start every row at column 0, where `left` is 0.
        let left = self.view.left;
        let row = y.div_euclid(8) - HEADER as i32;
        Some(if row < 0 {
            (first.0.saturating_sub(1), left + col)
        } else if let Some(&(line, start)) = self.layout.get(row as usize) {
            (line, start + col)
        } else if self.layout.len() < self.size().1 {
            // Below the program's end.
            (last.0, usize::MAX)
        } else {
            (last.0 + 1, left + col)
        })
    }

    /// A press of the mouse at a pixel of the editor's screen: moves the
    /// cursor there and starts a selection, or with `extend` moves the
    /// selection's end there. Presses outside the code do nothing.
    pub fn mouse_down(&mut self, x: i32, y: i32, extend: bool) {
        let row = y.div_euclid(8) - HEADER as i32;
        if row < 0 || row >= self.size().1 as i32 {
            return;
        }
        let Some((line, col)) = self.hit(x, y) else {
            return;
        };
        let from = self.pos();
        self.go(line, col);
        self.want_col = self.view.col;
        self.anchor = Some(if extend {
            self.anchor.unwrap_or(from)
        } else {
            self.pos()
        });
    }

    /// The mouse moved with its button held since [`Editor::mouse_down`].
    pub fn mouse_drag(&mut self, x: i32, y: i32) {
        if let Some((line, col)) = self.hit(x, y) {
            self.go(line, col);
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
    /// name; `tiles` draws line numbers on number tiles; `blink` shows the
    /// cursor in this frame.
    pub fn draw(
        &mut self,
        fb: &mut Framebuffer,
        font: &Font,
        name: &str,
        (wrap, tiles): (bool, bool),
        blink: bool,
    ) {
        self.number_tiles = tiles;
        self.scroll(wrap);
        let (cols, rows) = self.size();
        let gutter = self.gutter();
        let v = self.view;
        let selection = self.selection();
        self.layout.clear();
        fb.clear(BACKGROUND);
        let screen = &mut self.screen;
        screen.cls();
        let width = screen.size().0 as usize;
        let mut marks = Vec::new();
        let mut cursor = None;

        let mut row = HEADER;
        let mut line = v.top;
        while row < HEADER + rows && line < self.lines.len() {
            let text = &self.lines[line];
            let current = line == v.line;
            let chunks = if wrap { rows_of(text, cols) } else { 1 };
            for chunk in 0..chunks {
                if row >= HEADER + rows {
                    break;
                }
                if current {
                    fill_row(fb, row, 0, width, CURRENT_LINE);
                }
                if chunk == 0 {
                    let color = if current { TEXT } else { LINE_NUMBER };
                    if tiles {
                        for (x, &c) in number_tiles(line + 1).iter().enumerate() {
                            screen.put_at(x as u32, row as u32, c, color);
                        }
                        screen.put_at(3, row as u32, TILE_CAP, color);
                    } else {
                        let number = format!("{:>1$}", line + 1, gutter - 1);
                        put_str(screen, 0, row, &number, color);
                    }
                }
                let start = if wrap { chunk * cols } else { v.left };
                self.layout.push((line, start));
                for x in 0..cols {
                    let at = (line, start + x);
                    if selection.is_some_and(|(a, b)| a <= at && at < b) && at.1 <= text.len() {
                        fill_row(fb, row, gutter + x, 1, SELECTION);
                    }
                    match text.get(start + x) {
                        Some(&c) => screen.put_at((gutter + x) as u32, row as u32, c, TEXT),
                        None => {
                            if start + x == text.len() {
                                marks.push((gutter + x, row));
                            }
                            break;
                        }
                    }
                }
                if current && v.col >= start && v.col - start < cols {
                    cursor = Some((gutter + v.col - start, row));
                }
                row += 1;
            }
            line += 1;
        }

        // The ruler numbers the visible columns, every other one, on tiles
        // or in digits. Tiles other than the cursor's take the bar's color,
        // with their digits recolored after drawing.
        let ruler = HEADER + rows;
        let mut ruler_digits = Vec::new();
        fill_row(fb, ruler, 0, width, BAR);
        let first = if wrap { 0 } else { v.left };
        let cursor_col = if wrap { v.col % cols } else { v.col };
        for x in 0..cols {
            let col = first + x;
            let at = (gutter + x) as u32;
            if tiles {
                if col == cursor_col {
                    // On a white tile, as the current line's number.
                    screen.put_at(at, ruler as u32, ruler_tile(col), TEXT);
                } else if col.is_multiple_of(2) {
                    let color = if col.is_multiple_of(10) { TEXT } else { DIM };
                    screen.put_at(at, ruler as u32, ruler_tile(col), BAR);
                    ruler_digits.push((gutter + x, color));
                }
            } else if col == cursor_col {
                fill_row(fb, ruler, gutter + x, 1, TEXT);
                let digit = b'0' + (col % 10) as u8;
                screen.put_at(at, ruler as u32, u16::from(digit), BACKGROUND);
            } else if col.is_multiple_of(2) {
                let digit = b'0' + (col % 10) as u8;
                let color = if col.is_multiple_of(10) { TEXT } else { DIM };
                screen.put_at(at, ruler as u32, u16::from(digit), color);
            }
        }

        let status = ruler + 1;
        fill_row(fb, status, 0, width, BAR);
        put_str(screen, 0, status, &format!("PRG{} {name}", self.slot), TEXT);

        screen.draw(fb, font);
        for (x, color) in ruler_digits {
            for py in ruler * 8..(ruler + 1) * 8 {
                for px in x * 8..(x + 1) * 8 {
                    let (px, py) = (px as u32, py as u32);
                    if fb.get(px, py).is_some_and(|c| c != BAR) {
                        fb.set(px, py, color);
                    }
                }
            }
        }
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

/// Whether a [`keys`] code moves the cursor without editing.
pub fn is_movement(code: u16) -> bool {
    matches!(
        code,
        keys::LEFT
            | keys::RIGHT
            | keys::UP
            | keys::DOWN
            | keys::HOME
            | keys::END
            | keys::PAGE_UP
            | keys::PAGE_DOWN
    )
}

/// A line number's six digits on three tiles, two digits each, with the
/// leading zeros dimmed as SmileBASIC 4 does.
fn number_tiles(number: usize) -> [u16; 3] {
    let n = number % 1_000_000;
    let pairs = [n / 10_000, n / 100 % 100, n % 100];
    let mut leading = true;
    pairs.map(|pair| {
        let pair = pair as u16;
        if leading && pair == 0 {
            0xE16E
        } else if leading {
            // 00-09 here have their first zero dimmed; 10-99 are plain.
            leading = false;
            0xE100 + pair
        } else if pair < 10 {
            0xE164 + pair
        } else {
            0xE100 + pair
        }
    })
}

/// A column number on one tile: 0-9 on one-digit tiles, then its last
/// two digits.
fn ruler_tile(col: usize) -> u16 {
    match (col, (col % 100) as u16) {
        (0..10, n) => 0xE170 + n,
        (_, n @ 0..10) => 0xE164 + n,
        (_, n) => 0xE100 + n,
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
        e.open(0, "A\nB\nC", Some((2, 0)));
        assert_eq!(e.cursor(), (2, 0));
    }

    #[test]
    fn scrolls_to_the_cursor() {
        let mut e = editor(&"X\n".repeat(100));
        e.key(keys::PAGE_DOWN);
        e.scroll(false);
        assert_eq!((e.cursor().0, e.view.top), (28, 1));
        e.file_end();
        e.scroll(false);
        assert_eq!(e.view.top, 101 - 27);
        e.file_top();
        e.scroll(false);
        assert_eq!(e.view.top, 0);

        let mut e = editor(&"A".repeat(100));
        e.key(keys::END);
        e.scroll(false);
        assert_eq!(e.view.left, 100 + 1 - 46);
        e.scroll(true);
        assert_eq!(e.view.left, 0);
    }

    #[test]
    fn wrapped_lines_take_more_rows() {
        let long = "A".repeat(46 * 3);
        let mut e = editor(&format!("{long}\n").repeat(10));
        e.go(9, 0);
        e.scroll(true);
        // Each line takes four rows (the last for its return mark).
        assert_eq!(e.view.top, 3);
        e.file_end();
        e.scroll(true);
        assert_eq!(e.view.top, 4);
    }

    fn row(e: &Editor, y: u32) -> String {
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
    }

    #[test]
    fn draws_numbers_ruler_and_status() {
        let mut e = editor("PRINT 1");
        let mut fb = Framebuffer::new(400, 240);
        e.draw(&mut fb, &Font::blank(), "GAME", (false, true), true);
        assert_eq!(row(&e, 1), "\u{E16E}\u{E16E}\u{E101}\u{E17F}PRINT 1");
        let ruler: Vec<u16> = (4..15).map(|x| e.screen.cell(x, 28).code).collect();
        assert_eq!(
            ruler,
            [
                0xE170, 0, 0xE172, 0, 0xE174, 0, 0xE176, 0, 0xE178, 0, 0xE10A
            ]
        );
        assert_eq!(row(&e, 29), "PRG0 GAME");
        // The cursor bar at column 0 of the code.
        assert_eq!(row(&e, 0), "", "left for the menu bar");
        assert_eq!(fb.get(4 * 8, 8), Some(TEXT));
        assert_eq!(fb.get(4 * 8 + 2, 8), Some(CURRENT_LINE));
    }

    #[test]
    fn ruler_tiles_take_the_bar_color() {
        let mut e = editor("PRINT 1");
        let mut fb = Framebuffer::new(400, 240);
        e.draw(&mut fb, &Font::fallback(), "GAME", (false, true), true);
        // Column 2's tile: dim digits on the bar.
        let cell: Vec<u32> = (0..64)
            .map(|i| fb.get((4 + 2) * 8 + i % 8, 28 * 8 + i / 8).unwrap())
            .collect();
        assert!(cell.iter().all(|&c| c == BAR || c == DIM));
        assert!(cell.contains(&DIM));
    }

    #[test]
    fn plain_numbers_take_the_digits_they_need() {
        let mut e = editor(&"PRINT\n".repeat(119));
        let mut fb = Framebuffer::new(400, 240);
        e.draw(&mut fb, &Font::blank(), "GAME", (false, false), true);
        assert_eq!(row(&e, 1), "  1 PRINT");
        assert!(row(&e, 28).starts_with("    0 2 4"));
    }

    #[test]
    fn number_tiles_dim_leading_zeros() {
        assert_eq!(number_tiles(1), [0xE16E, 0xE16E, 0xE101]);
        assert_eq!(number_tiles(123), [0xE16E, 0xE101, 0xE117]);
        assert_eq!(number_tiles(1005), [0xE16E, 0xE10A, 0xE169]);
        assert_eq!(number_tiles(120_000), [0xE10C, 0xE164, 0xE164]);
        assert_eq!(ruler_tile(7), 0xE177);
        assert_eq!(ruler_tile(42), 0xE12A);
        assert_eq!(ruler_tile(105), 0xE169);
    }

    #[test]
    fn select_copy_cut_and_paste() {
        let mut e = editor("ABC\nDEF\nGHI");
        e.key(keys::RIGHT);
        e.key_select(keys::DOWN);
        e.key_select(keys::RIGHT);
        assert_eq!(e.copy().as_deref(), Some("BC\nDE"));
        assert_eq!(e.cut().as_deref(), Some("BC\nDE"));
        assert_eq!((e.text().as_str(), e.cursor()), ("AF\nGHI", (1, 1)));
        assert_eq!(e.copy(), None);
        e.paste("12\r\n34\n5");
        assert_eq!(
            (e.text().as_str(), e.cursor()),
            ("A12\n34\n5F\nGHI", (3, 1))
        );
        // Typing replaces a selection; moving drops it.
        e.select_all();
        typing(&mut e, "X");
        assert_eq!(e.text(), "X");
        e.key_select(keys::LEFT);
        e.key(keys::RIGHT);
        e.key(keys::BACKSPACE);
        assert_eq!(e.text(), "");
    }

    #[test]
    fn mouse_selects() {
        let mut e = editor("PRINT 1\nPRINT 2\nEND");
        let mut fb = Framebuffer::new(400, 240);
        e.draw(&mut fb, &Font::blank(), "GAME", (false, true), true);
        // Column 2 of line 1, then the end of line 2.
        e.mouse_down((4 + 2) * 8 + 3, 8 + 4, false);
        e.mouse_drag(40 * 8, 16 + 7);
        assert_eq!(e.copy().as_deref(), Some("INT 1\nPRINT 2"));
        // Shift keeps the start; below the end goes to the last line's end.
        e.mouse_down(0, 20 * 8, true);
        assert_eq!(e.copy().as_deref(), Some("INT 1\nPRINT 2\nEND"));
        e.draw(&mut fb, &Font::blank(), "GAME", (false, true), true);
        assert_eq!(fb.get(6 * 8, 8), Some(SELECTION));
        // The ruler and the menu bar's row do nothing.
        e.mouse_down(0, 28 * 8, false);
        e.mouse_down(0, 4, false);
        assert!(e.copy().is_some());
    }
}
