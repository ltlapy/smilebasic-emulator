//! Text screens 0-4. Screen 4 is the console that PRINT writes to.

use crate::font::{Font, FontSize};
use crate::framebuffer::Framebuffer;

pub const SCREENS: usize = 5;
pub const CONSOLE: usize = 4;
/// Characters from here to `0xF7FF` are drawn from a graphic page (TPAGE).
pub const USER_CHARS: std::ops::RangeInclusive<u16> = 0xE800..=0xF7FF;
/// UNVERIFIED: SB3's default TABSTEP; SB4's console has not been measured.
pub const TAB_WIDTH: u32 = 4;
const WHITE: u32 = 0xFFFF_FFFF;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cell {
    /// 0 is an empty cell.
    pub code: u16,
    pub color: u32,
}

const EMPTY: Cell = Cell { code: 0, color: 0 };

pub struct TextScreen {
    font: FontSize,
    /// Size each character is drawn at, in pixels.
    char_size: u32,
    width: u32,
    height: u32,
    cells: Vec<Cell>,
    cursor: (u32, u32),
    color: u32,
    pub visible: bool,
}

impl TextScreen {
    /// A screen filling `screen_w` x `screen_h` pixels with the 8-pixel font.
    pub fn new(screen_w: u32, screen_h: u32) -> TextScreen {
        let (width, height) = (screen_w / 8, screen_h / 8);
        TextScreen {
            font: FontSize::Small,
            char_size: 8,
            width,
            height,
            cells: vec![EMPTY; (width * height) as usize],
            cursor: (0, 0),
            color: WHITE,
            visible: true,
        }
    }

    pub fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    pub fn cursor(&self) -> (u32, u32) {
        self.cursor
    }

    pub fn color(&self) -> u32 {
        self.color
    }

    pub fn set_color(&mut self, color: u32) {
        self.color = color;
    }

    pub fn cell(&self, x: u32, y: u32) -> Cell {
        self.cells[(y * self.width + x) as usize]
    }

    /// Clears the screen and homes the cursor.
    pub fn cls(&mut self) {
        self.cells.fill(EMPTY);
        self.cursor = (0, 0);
    }

    /// Moves the cursor; `None` keeps that coordinate. Returns false if the
    /// position is off the screen.
    pub fn locate(&mut self, x: Option<i32>, y: Option<i32>) -> bool {
        let x = x.unwrap_or(self.cursor.0 as i32);
        let y = y.unwrap_or(self.cursor.1 as i32);
        let fits = |v: i32, max: u32| u32::try_from(v).is_ok_and(|v| v < max);
        if !fits(x, self.width) || !fits(y, self.height) {
            return false;
        }
        self.cursor = (x as u32, y as u32);
        true
    }

    pub fn print(&mut self, text: &[u16]) {
        for &c in text {
            match c {
                // UNVERIFIED: whether CHR$(10) also starts a new line.
                10 | 13 => self.newline(),
                _ => self.put(c),
            }
        }
    }

    /// PRINT's `,`: pads with spaces to the next tab stop.
    pub fn tab(&mut self) {
        let pad = TAB_WIDTH - self.cursor.0 % TAB_WIDTH;
        for _ in 0..pad {
            self.put(u16::from(b' '));
        }
    }

    /// Steps the cursor back one cell, to the previous line if needed, and
    /// clears that cell.
    pub fn backspace(&mut self) {
        let (x, y) = self.cursor;
        self.cursor = match (x, y) {
            (0, 0) => return,
            (0, y) => (self.width - 1, y - 1),
            (x, y) => (x - 1, y),
        };
        let (x, y) = self.cursor;
        self.cells[(y * self.width + x) as usize] = EMPTY;
    }

    pub fn newline(&mut self) {
        self.cursor.0 = 0;
        if self.cursor.1 + 1 < self.height {
            self.cursor.1 += 1;
        } else {
            self.scroll_up();
        }
    }

    /// Writing the last column leaves the cursor just past it; the wrap
    /// happens with the next character, so a full line followed by a
    /// newline moves down only once.
    fn put(&mut self, code: u16) {
        if self.width == 0 || self.height == 0 {
            return;
        }
        if self.cursor.0 >= self.width {
            self.newline();
        }
        let (x, y) = self.cursor;
        self.cells[(y * self.width + x) as usize] = Cell {
            code,
            color: self.color,
        };
        self.cursor.0 += 1;
    }

    fn scroll_up(&mut self) {
        let w = self.width as usize;
        self.cells.copy_within(w.., 0);
        let len = self.cells.len();
        self.cells[len - w..].fill(EMPTY);
    }

    /// Fills the cursor's cell with the text color.
    // UNVERIFIED: the shape of SmileBASIC's INPUT cursor.
    pub fn draw_cursor(&self, fb: &mut Framebuffer) {
        if !self.visible {
            return;
        }
        let cs = self.char_size;
        let (cx, cy) = self.cursor;
        if cx >= self.width {
            return;
        }
        for y in cy * cs..(cy + 1) * cs {
            for x in cx * cs..(cx + 1) * cs {
                if let Some(dst) = fb.get(x, y) {
                    fb.set(x, y, blend(dst, self.color));
                }
            }
        }
    }

    /// Draws the screen over `fb`, blending each glyph's alpha.
    pub fn draw(&self, fb: &mut Framebuffer, font: &Font) {
        if !self.visible {
            return;
        }
        let cs = self.char_size;
        let fs = self.font as u32;
        for cy in 0..self.height {
            for cx in 0..self.width {
                let cell = self.cell(cx, cy);
                // TODO: user-defined characters need graphic pages (TPAGE).
                if cell.code == 0 || USER_CHARS.contains(&cell.code) {
                    continue;
                }
                for py in 0..cs {
                    for px in 0..cs {
                        let src = font.pixel(cell.code, self.font, px * fs / cs, py * fs / cs);
                        let (x, y) = (cx * cs + px, cy * cs + py);
                        if let Some(dst) = fb.get(x, y) {
                            fb.set(x, y, blend(dst, multiply(src, cell.color)));
                        }
                    }
                }
            }
        }
    }
}

fn channels(c: u32) -> [u32; 4] {
    c.to_be_bytes().map(u32::from)
}

/// Per-channel product, as used for text colors.
fn multiply(a: u32, b: u32) -> u32 {
    let (a, b) = (channels(a), channels(b));
    let c: [u8; 4] = std::array::from_fn(|i| (a[i] * b[i] / 255) as u8);
    u32::from_be_bytes(c)
}

/// `src` over `dst`; the result keeps `dst`'s alpha.
fn blend(dst: u32, src: u32) -> u32 {
    let (d, s) = (channels(dst), channels(src));
    let a = s[0];
    let mix = |i: usize| ((s[i] * a + d[i] * (255 - a)) / 255) as u8;
    u32::from_be_bytes([d[0] as u8, mix(1), mix(2), mix(3)])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(s: &str) -> Vec<u16> {
        s.encode_utf16().collect()
    }

    fn row(screen: &TextScreen, y: u32) -> String {
        let (w, _) = screen.size();
        let codes: Vec<u16> = (0..w)
            .map(|x| screen.cell(x, y).code)
            .take_while(|&c| c != 0)
            .collect();
        String::from_utf16(&codes).unwrap()
    }

    #[test]
    fn console_size() {
        assert_eq!(TextScreen::new(400, 240).size(), (50, 30));
    }

    #[test]
    fn print_wraps_and_scrolls() {
        let mut s = TextScreen::new(32, 24); // 4 x 3 characters
        s.print(&text("ABCDE"));
        assert_eq!((row(&s, 0).as_str(), row(&s, 1).as_str()), ("ABCD", "E"));
        assert_eq!(s.cursor(), (1, 1));
        s.newline();
        s.print(&text("X\rY"));
        assert_eq!(row(&s, 0), "E");
        assert_eq!(row(&s, 1), "X");
        assert_eq!(row(&s, 2), "Y");
    }

    #[test]
    fn full_line_then_newline_moves_down_once() {
        let mut s = TextScreen::new(32, 24);
        s.print(&text("ABCD"));
        assert_eq!(s.cursor(), (4, 0));
        s.newline();
        assert_eq!(s.cursor(), (0, 1));
        s.print(&text("EFGHI"));
        assert_eq!((row(&s, 1).as_str(), row(&s, 2).as_str()), ("EFGH", "I"));
        s.backspace();
        s.backspace();
        assert_eq!((row(&s, 1).as_str(), s.cursor()), ("EFG", (3, 1)));
    }

    #[test]
    fn backspace_crosses_lines() {
        let mut s = TextScreen::new(32, 24);
        s.print(&text("ABCDE"));
        s.backspace();
        s.backspace();
        assert_eq!((row(&s, 0).as_str(), row(&s, 1).as_str()), ("ABC", ""));
        assert_eq!(s.cursor(), (3, 0));
        s.locate(Some(0), Some(0));
        s.backspace();
        assert_eq!(s.cursor(), (0, 0));
    }

    #[test]
    fn tab_and_locate() {
        let mut s = TextScreen::new(80, 16);
        s.print(&text("A"));
        s.tab();
        s.print(&text("B"));
        assert_eq!(row(&s, 0), "A   B");
        assert!(s.locate(Some(9), None));
        assert_eq!(s.cursor(), (9, 0));
        assert!(!s.locate(Some(10), Some(0)));
        assert!(!s.locate(None, Some(-1)));
        s.cls();
        assert_eq!((s.cursor(), row(&s, 0).as_str()), ((0, 0), ""));
    }

    #[test]
    fn draw_blends_glyph_with_color() {
        let mut font = Font::blank();
        let (gx, gy) = font.origin(0x41, FontSize::Small).unwrap();
        font.page_mut()[(gy * crate::font::PAGE_SIZE + gx) as usize] = 0xFFFF_FFFF;
        font.page_mut()[(gy * crate::font::PAGE_SIZE + gx + 1) as usize] = 0x80FF_FFFF;
        let mut s = TextScreen::new(16, 8);
        s.set_color(0xFFFF_0000);
        s.print(&text("A"));
        let mut fb = Framebuffer::new(16, 8);
        s.draw(&mut fb, &font);
        assert_eq!(fb.get(0, 0), Some(0xFFFF_0000));
        assert_eq!(fb.get(1, 0), Some(0xFF80_0000));
        assert_eq!(fb.get(2, 0), Some(0xFF00_0000));
    }
}
