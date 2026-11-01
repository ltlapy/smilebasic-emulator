//! Drawing for the emulator's own UI, at display resolution.

use std::sync::atomic::{AtomicBool, Ordering};

use sb_runtime::Framebuffer;
use sb_runtime::font::{Font, FontSize};

/// Glyphs are the console's 8x8 font drawn at twice the size, or its
/// 16x16 one (see [`set_font`]).
const GLYPH_SCALE: u32 = 2;
pub const CHAR: u32 = 8 * GLYPH_SCALE;
pub const PAD: u32 = 8;
/// The height of a line of text in windows, and of buttons.
pub const LINE: u32 = 24;

pub const BACKGROUND: u32 = 0xE818_1818;
pub const HIGHLIGHT: u32 = 0xFF2F_5FAF;
pub const TEXT: u32 = 0xFFFF_FFFF;
pub const DISABLED: u32 = 0xFF80_8080;
pub const BORDER: u32 = 0xFF80_8080;
/// The face of a button that cannot be pressed.
const DISABLED_FACE: u32 = 0xFF40_4040;

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

/// Draws a two-pixel frame just inside `r`.
pub fn outline(fb: &mut Framebuffer, r: Rect, color: u32) {
    let w = 2.min(r.w).min(r.h);
    for side in [
        Rect { h: w, ..r },
        Rect {
            y: r.y + r.h - w,
            h: w,
            ..r
        },
        Rect { w, ..r },
        Rect {
            x: r.x + r.w - w,
            w,
            ..r
        },
    ] {
        fill(fb, side, color);
    }
}

/// Draws text in `color`, keeping the background's alpha.
pub fn draw_text(fb: &mut Framebuffer, font: &Font, x: u32, y: u32, text: &str, color: u32) {
    draw_text_scaled(fb, font, x, y, text, color, GLYPH_SCALE);
}

/// UI text draws the 16-pixel font where it is drawn at an even scale.
static LARGE_FONT: AtomicBool = AtomicBool::new(false);

/// Picks the font of the UI, as the ui_font setting says.
pub fn set_font(size: FontSize) {
    LARGE_FONT.store(size == FontSize::Large, Ordering::Relaxed);
}

/// Draws text with the 8x8 glyphs `scale` times their size, or with the
/// 16x16 ones half that, if chosen and `scale` is even.
pub fn draw_text_scaled(
    fb: &mut Framebuffer,
    font: &Font,
    x: u32,
    y: u32,
    text: &str,
    color: u32,
    scale: u32,
) {
    let cell = 8 * scale;
    let (size, scale) = if LARGE_FONT.load(Ordering::Relaxed) && scale.is_multiple_of(2) {
        (FontSize::Large, scale / 2)
    } else {
        (FontSize::Small, scale)
    };
    for (i, c) in text.encode_utf16().enumerate() {
        let left = x + i as u32 * cell;
        for py in 0..cell {
            for px in 0..cell {
                let glyph = font.pixel(c, size, px / scale, py / scale);
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

/// Where a side panel (the help, the index) goes on a display of `w` x
/// `h`: half of it beside the editor, or `min_w` if that is wider, below
/// the menu bar, which stays shown over the editor.
pub fn side_rect((w, h): (u32, u32), left: bool, min_w: u32) -> Rect {
    let pw = (w / 2).max(min_w).min(w);
    Rect {
        x: if left { 0 } else { w - pw },
        // The menu bar's height.
        y: LINE,
        w: pw,
        h: h.saturating_sub(LINE),
    }
}

/// A bordered window with a title bar.
pub fn window(fb: &mut Framebuffer, font: &Font, f: Rect, title: &str) {
    panel(fb, font, f, title, true);
}

/// A window whose title bar is grayed while the keys go elsewhere.
pub fn panel(fb: &mut Framebuffer, font: &Font, f: Rect, title: &str, focused: bool) {
    fill(fb, f, BORDER);
    let inner = Rect {
        x: f.x + 2,
        y: f.y + 2,
        w: f.w - 4,
        h: f.h - 4,
    };
    fill(fb, inner, BACKGROUND | 0xFF00_0000);
    let bar = if focused { HIGHLIGHT } else { DISABLED_FACE };
    fill(fb, Rect { h: LINE, ..inner }, bar);
    draw_text(fb, font, f.x + 2 * PAD, f.y + 6, title, TEXT);
}

pub fn button(
    fb: &mut Framebuffer,
    font: &Font,
    r: Rect,
    label: &str,
    hovered: bool,
    enabled: bool,
) {
    let (face, color) = match (enabled, hovered) {
        (false, _) => (DISABLED_FACE, DISABLED),
        (true, true) => (HIGHLIGHT, TEXT),
        (true, false) => (BORDER, TEXT),
    };
    fill(fb, r, face);
    let x = r.x + (r.w - text_width(label)) / 2;
    draw_text(fb, font, x, r.y + 4, label, color);
}
