//! Text screens 0-4. Screen 4 is the console that PRINT writes to.

use crate::compose::{Affine, Placement, draw_image, over};
use crate::font::{Font, FontSize};
use crate::framebuffer::Framebuffer;
use crate::graphics::{Clip, Graphics};
use crate::sprites::sample;

pub const SCREENS: usize = 5;
pub const CONSOLE: usize = 4;
/// Characters from here to `0xF7FF` are drawn from a graphic page (TPAGE).
pub const USER_CHARS: std::ops::RangeInclusive<u16> = 0xE800..=0xF7FF;
/// UNVERIFIED: SB3's default TABSTEP; SB4's console has not been measured.
pub const TAB_WIDTH: u32 = 4;
const WHITE: u32 = 0xFFFF_FFFF;
/// GRP5 (`#GRPF`) holds the font that text screens and GPUTCHR draw.
pub const FONT_PAGE: usize = 5;

/// Pixel `(x, y)` of a character as GRP5 now shows it; fallback glyphs
/// that have no place there come from the font itself.
pub fn glyph_pixel(font: &Font, grp: &Graphics, code: u16, size: FontSize, x: u32, y: u32) -> u32 {
    match font.origin(code, size) {
        Some((gx, gy)) => grp.pixel(FONT_PAGE, (gx + x) as i32, (gy + y) as i32),
        None => font.pixel(code, size, x, y),
    }
}

/// User-defined characters sit in a grid this many characters wide.
const USER_GRID: i32 = 64;

/// ATTR bits of a character.
pub mod attr {
    pub const ROTATION: u8 = 0b11;
    pub const REVH: u8 = 0b100;
    pub const REVV: u8 = 0b1000;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cell {
    /// 0 is an empty cell.
    pub code: u16,
    pub color: u32,
    /// ATTR: quarter turns and flips.
    pub attr: u8,
}

const EMPTY: Cell = Cell {
    code: 0,
    color: 0,
    attr: 0,
};

pub struct TextScreen {
    /// TSCREEN's font type: 8 or 16 for the normal font at that size, or
    /// up to 64 for user-defined characters only.
    font_type: u32,
    /// Size each character is drawn at, in pixels.
    char_size: u32,
    width: u32,
    height: u32,
    cells: Vec<Cell>,
    cursor: (u32, u32),
    color: u32,
    /// ATTR for characters written from now on.
    pub attr: u8,
    /// TPAGE: where user-defined characters come from.
    pub page: i32,
    pub page_origin: (i32, i32),
    pub visible: bool,
    /// TOFS, THOME, TROT and TSCALE place the screen like a sprite.
    pub pos: (f64, f64),
    /// Smaller is nearer the front: -4095 to 4095.
    pub z: i32,
    pub home: (f64, f64),
    pub rot: f64,
    pub scale: (f64, f64),
    /// TCOLOR: multiplies the whole screen.
    pub tint: u32,
    /// TBLEND 1: additive.
    pub add: bool,
    pub layer: i32,
}

impl TextScreen {
    /// A screen filling `screen_w` x `screen_h` pixels with the 8-pixel font.
    pub fn new(screen_w: u32, screen_h: u32) -> TextScreen {
        let (width, height) = (screen_w / 8, screen_h / 8);
        TextScreen {
            font_type: 8,
            char_size: 8,
            width,
            height,
            cells: vec![EMPTY; (width * height) as usize],
            cursor: (0, 0),
            color: WHITE,
            attr: 0,
            page: 4,
            page_origin: (1024, 0),
            visible: true,
            pos: (0.0, 0.0),
            z: 0,
            home: (0.0, 0.0),
            rot: 0.0,
            scale: (1.0, 1.0),
            tint: WHITE,
            add: false,
            layer: 0,
        }
    }

    pub fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    /// TSCREEN's font type and character size.
    pub fn font(&self) -> (u32, u32) {
        (self.font_type, self.char_size)
    }

    /// TSCREEN: a new font and size, which clears the screen.
    // UNVERIFIED: whether TSCREEN keeps the contents.
    pub fn set_font(&mut self, font_type: u32, char_size: u32, width: u32, height: u32) {
        (self.font_type, self.char_size) = (font_type, char_size);
        (self.width, self.height) = (width, height);
        self.cells = vec![EMPTY; (width * height) as usize];
        self.cursor = (0, 0);
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

    /// Writes one cell without moving the cursor; off-screen cells are
    /// ignored.
    pub fn put_at(&mut self, x: u32, y: u32, code: u16, color: u32) {
        if x < self.width && y < self.height {
            self.cells[(y * self.width + x) as usize] = Cell {
                code,
                color,
                attr: self.attr,
            };
        }
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
            attr: self.attr,
        };
        self.cursor.0 += 1;
    }

    fn scroll_up(&mut self) {
        let w = self.width as usize;
        self.cells.copy_within(w.., 0);
        let len = self.cells.len();
        self.cells[len - w..].fill(EMPTY);
    }

    /// Draws the screen over `fb` where TOFS and its companions put it.
    pub fn draw(&self, fb: &mut Framebuffer, font: &Font) {
        let mut grp = Graphics::default();
        grp.set_page(FONT_PAGE, font.page_image());
        self.draw_placed(fb, font, &grp, &Affine::IDENTITY, None, false);
    }

    /// The pixel at `(x, y)` of a character's `font_type`-sized image.
    fn glyph_pixel(&self, font: &Font, grp: &Graphics, code: u16, x: u32, y: u32) -> u32 {
        let ft = self.font_type;
        if USER_CHARS.contains(&code) {
            let i = i32::from(code - USER_CHARS.start());
            let (ox, oy) = self.page_origin;
            let size = ft as i32;
            let gx = ox + i % USER_GRID * size + x as i32;
            let gy = oy + i / USER_GRID * size + y as i32;
            return sample(grp, self.page, gx, gy);
        }
        match ft {
            8 => glyph_pixel(font, grp, code, FontSize::Small, x, y),
            16 => glyph_pixel(font, grp, code, FontSize::Large, x, y),
            // Larger font types show only user-defined characters.
            _ => 0,
        }
    }

    /// Draws the screen through its layer's matrix and clipping, with the
    /// INPUT cursor if `cursor` is set.
    // UNVERIFIED: the shape of SmileBASIC's INPUT cursor (a cell filled
    // with the text color).
    pub fn draw_placed(
        &self,
        fb: &mut Framebuffer,
        font: &Font,
        grp: &Graphics,
        layer: &Affine,
        clip: Option<&Clip>,
        cursor: bool,
    ) {
        if !self.visible {
            return;
        }
        let cs = self.char_size;
        let ft = self.font_type;
        let cursor = cursor.then_some(self.cursor);
        if cursor.is_none() && self.cells.iter().all(|c| c.code == 0) {
            return;
        }
        let place = Placement {
            matrix: Affine::place(self.pos, self.home, self.rot, self.scale).then(layer),
            clip,
            add: self.add,
        };
        let (w, h) = ((self.width * cs) as i32, (self.height * cs) as i32);
        draw_image(fb, &place, w, h, |x, y| {
            let (x, y) = (x as u32, y as u32);
            let (cx, cy) = (x / cs, y / cs);
            let cell = self.cell(cx, cy);
            let glyph = if cell.code == 0 {
                0
            } else {
                // UNVERIFIED: flips apply to the cell, then the quarter turn
                // picks the font pixel, as for sprites.
                let (mut px, mut py) = (x % cs, y % cs);
                if cell.attr & attr::REVH != 0 {
                    px = cs - 1 - px;
                }
                if cell.attr & attr::REVV != 0 {
                    py = cs - 1 - py;
                }
                let (px, py) = match cell.attr & attr::ROTATION {
                    0 => (px, py),
                    1 => (py, cs - 1 - px),
                    2 => (cs - 1 - px, cs - 1 - py),
                    _ => (cs - 1 - py, px),
                };
                let pixel = self.glyph_pixel(font, grp, cell.code, px * ft / cs, py * ft / cs);
                multiply(pixel, cell.color)
            };
            let glyph = if cursor == Some((cx, cy)) {
                over(glyph, self.color)
            } else {
                glyph
            };
            multiply(glyph, self.tint)
        });
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
