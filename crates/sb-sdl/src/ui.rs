//! Drawing for the emulator's own UI, at display resolution.

use sb_runtime::Framebuffer;
use sb_runtime::font::{Font, FontSize};

/// Glyphs are the console's 8x8 font drawn at twice the size.
const GLYPH_SCALE: u32 = 2;
pub const CHAR: u32 = 8 * GLYPH_SCALE;
pub const PAD: u32 = 8;

pub const BACKGROUND: u32 = 0xE818_1818;
pub const HIGHLIGHT: u32 = 0xFF2F_5FAF;
pub const TEXT: u32 = 0xFFFF_FFFF;
pub const DISABLED: u32 = 0xFF80_8080;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

impl Rect {
    pub fn contains(self, (x, y): (i32, i32)) -> bool {
        let (x, y) = (i64::from(x), i64::from(y));
        x >= i64::from(self.x)
            && y >= i64::from(self.y)
            && x < i64::from(self.x + self.w)
            && y < i64::from(self.y + self.h)
    }
}

pub fn text_width(text: &str) -> u32 {
    text.chars().count() as u32 * CHAR
}

pub fn fill(fb: &mut Framebuffer, r: Rect, color: u32) {
    for y in r.y..r.y + r.h {
        for x in r.x..r.x + r.w {
            fb.set(x, y, color);
        }
    }
}

/// Draws text in `color`, keeping the background's alpha.
pub fn draw_text(fb: &mut Framebuffer, font: &Font, x: u32, y: u32, text: &str, color: u32) {
    for (i, c) in text.encode_utf16().enumerate() {
        let left = x + i as u32 * CHAR;
        for py in 0..CHAR {
            for px in 0..CHAR {
                let glyph = font.pixel(c, FontSize::Small, px / GLYPH_SCALE, py / GLYPH_SCALE);
                if glyph >> 24 >= 0x80
                    && let Some(dst) = fb.get(left + px, y + py)
                {
                    fb.set(
                        left + px,
                        y + py,
                        (dst & 0xFF00_0000) | (color & 0x00FF_FFFF),
                    );
                }
            }
        }
    }
}
