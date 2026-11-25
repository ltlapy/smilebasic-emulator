//! Drawing for the emulator's own UI, at display resolution.

use std::cell::RefCell;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::atomic::{AtomicU8, Ordering};

use sb_runtime::Framebuffer;
use sb_runtime::extra_glyphs::BundledSet;
use sb_runtime::font::{Font, FontSize};
use sb_runtime::half_width::is_half_width;
use sb_runtime::settings::UiFont;
use sb_runtime::ui_glyphs::UiGlyphs;

/// Glyphs are the console's 16x16 font or smooth outlines of that size
/// (see [`set_font`]). Text drawn at an odd scale takes its 8x8 font.
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
/// The face of a dialog's button with the focus: a lighter blue than
/// [`HIGHLIGHT`], the color of the title bar.
pub const FOCUS: u32 = 0xFF4A_88E0;
/// The face of a dialog's button under the mouse without the focus: a
/// lighter gray than [`BORDER`], so that it never looks like the focus.
const DIALOG_HOVER: u32 = 0xFF9C_9C9C;
/// The face of a button that cannot be pressed.
const DISABLED_FACE: u32 = 0xFF40_4040;
/// The face of a button that starts something, like Run, and its face
/// under the mouse. White text on both is over 4.5:1.
pub const GO: u32 = 0xFF2E_7D32;
pub const GO_HOVERED: u32 = 0xFF38_8E3C;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Rect {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

impl Rect {
    /// The pixel in the middle, rounded toward the top left.
    pub fn center(self) -> (i32, i32) {
        ((self.x + self.w / 2) as i32, (self.y + self.h / 2) as i32)
    }

    pub fn contains(self, (x, y): (i32, i32)) -> bool {
        let (x, y) = (i64::from(x), i64::from(y));
        x >= i64::from(self.x)
            && y >= i64::from(self.y)
            && x < i64::from(self.x + self.w)
            && y < i64::from(self.y + self.h)
    }

    /// The part of the rectangle on a frame of `w` x `h`, if any.
    pub fn within(self, (w, h): (u32, u32)) -> Option<Rect> {
        let (x0, y0) = (self.x.min(w), self.y.min(h));
        let x1 = self.x.saturating_add(self.w).min(w);
        let y1 = self.y.saturating_add(self.h).min(h);
        (x1 > x0 && y1 > y0).then_some(Rect {
            x: x0,
            y: y0,
            w: x1 - x0,
            h: y1 - y0,
        })
    }

    /// Whether the two share a pixel.
    pub fn overlaps(self, other: Rect) -> bool {
        self.x < other.x + other.w
            && other.x < self.x + self.w
            && self.y < other.y + other.h
            && other.y < self.y + self.h
    }

    /// The smallest rectangle around both.
    pub fn union(self, other: Rect) -> Rect {
        let (x0, y0) = (self.x.min(other.x), self.y.min(other.y));
        let x1 = (self.x + self.w).max(other.x + other.w);
        let y1 = (self.y + self.h).max(other.y + other.h);
        Rect {
            x: x0,
            y: y0,
            w: x1 - x0,
            h: y1 - y0,
        }
    }
}

/// Returns either rectangle, or the smallest one around both.
pub fn union(a: Option<Rect>, b: Option<Rect>) -> Option<Rect> {
    match (a, b) {
        (Some(a), Some(b)) => Some(a.union(b)),
        _ => a.or(b),
    }
}

/// The data that a dry run of drawing gathers: a fingerprint of what
/// would be drawn, and the area around it.
struct Dry {
    state: DefaultHasher,
    bounds: Option<Rect>,
    translucent: Option<Rect>,
}

/// The result of a dry run: what would be drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Drawn {
    /// A fingerprint of everything drawn, in order: drawing that gives
    /// the same one draws the same pixels on the same ones.
    pub key: u64,
    /// The rectangle around everything drawn.
    pub bounds: Option<Rect>,
    /// The rectangle around the translucent fills, which let what is
    /// under the frame show through.
    pub translucent: Option<Rect>,
}

thread_local! {
    static DRY: RefCell<Option<Dry>> = const { RefCell::new(None) };
}

/// Runs `draw` without drawing anything, to find what it would have
/// drawn. Everything drawn must go through the functions that call
/// [`dry`].
pub fn dry_run(draw: impl FnOnce()) -> Drawn {
    DRY.with_borrow_mut(|dry| {
        *dry = Some(Dry {
            state: DefaultHasher::new(),
            bounds: None,
            translucent: None,
        })
    });
    draw();
    let dry = DRY.with_borrow_mut(Option::take).expect("set above");
    Drawn {
        key: dry.state.finish(),
        bounds: dry.bounds,
        translucent: dry.translucent,
    }
}

/// During a dry run, notes that `what` would be drawn on `area` of
/// `fb`, and returns true for the caller to draw nothing; otherwise
/// returns false. `what` must differ for anything that draws differently
/// there.
pub fn dry(fb: &Framebuffer, area: Rect, what: impl Hash) -> bool {
    DRY.with_borrow_mut(|dry| {
        let Some(dry) = dry else {
            return false;
        };
        let area = area.within((fb.width(), fb.height()));
        dry.bounds = union(dry.bounds, area);
        what.hash(&mut dry.state);
        true
    })
}

/// Clears the whole frame to `color`.
pub fn clear(fb: &mut Framebuffer, color: u32) {
    let all = Rect {
        x: 0,
        y: 0,
        w: fb.width(),
        h: fb.height(),
    };
    if !dry(fb, all, ("clear", color)) {
        fb.clear(color);
    }
}

/// Returns the width of `text` drawn by [`draw_text`]: half-width
/// characters take half a [`CHAR`].
pub fn text_width(text: &str) -> u32 {
    text.encode_utf16().map(unit_width).sum()
}

/// Returns the width of one UTF-16 unit drawn by [`draw_text`].
fn unit_width(unit: u16) -> u32 {
    if is_half_width(unit) { CHAR / 2 } else { CHAR }
}

/// Returns the width of `c` drawn by [`draw_text`].
pub fn char_width(c: char) -> u32 {
    c.encode_utf16(&mut [0; 2])
        .iter()
        .copied()
        .map(unit_width)
        .sum()
}

pub fn fill(fb: &mut Framebuffer, r: Rect, color: u32) {
    if dry(fb, r, ("fill", r, color)) {
        if let (1..=0xFE, Some(r)) = (color >> 24, r.within((fb.width(), fb.height()))) {
            DRY.with_borrow_mut(|dry| {
                if let Some(dry) = dry {
                    dry.translucent = union(dry.translucent, Some(r));
                }
            });
        }
        return;
    }
    let w = fb.width();
    let Some(r) = r.within((w, fb.height())) else {
        return;
    };
    let pixels = fb.pixels_mut();
    for y in r.y..r.y + r.h {
        let row = (y * w + r.x) as usize;
        pixels[row..row + r.w as usize].fill(color);
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

/// The face of a dialog's button: [`FOCUS`] and framed when it has the
/// focus that A, Enter and the D-pad move, and gray otherwise, lighter
/// under the mouse. Other buttons turn [`HIGHLIGHT`] under the mouse, but
/// here that blue would look like the focus.
pub fn focus_face(fb: &mut Framebuffer, r: Rect, focused: bool, hovered: bool) {
    let face = match (focused, hovered) {
        (true, _) => FOCUS,
        (false, true) => DIALOG_HOVER,
        (false, false) => BORDER,
    };
    fill(fb, r, face);
    if focused {
        outline(fb, r, TEXT);
    }
}

/// Draws text in `color`, keeping the background's alpha. Half-width
/// characters are drawn half as wide as the rest, because the emulator's
/// own windows are not SmileBASIC screens.
pub fn draw_text(fb: &mut Framebuffer, font: &Font, x: u32, y: u32, text: &str, color: u32) {
    draw_glyphs(fb, font, (x, y), text, color, GLYPH_SCALE, true);
}

/// [`draw_text`] at twice its size, for a heading: twice [`text_width`]
/// across and twice [`CHAR`] high.
pub fn draw_text_double(fb: &mut Framebuffer, font: &Font, x: u32, y: u32, text: &str, color: u32) {
    draw_glyphs(fb, font, (x, y), text, color, 2 * GLYPH_SCALE, true);
}

/// The font of UI text, a [`UiFont`] stored as u8. The 16-pixel and
/// smooth fonts are used when text is drawn at an even scale.
static UI_FONT: AtomicU8 = AtomicU8::new(UiFont::Large as u8);

/// Picks the font of the UI, as the ui_font setting says.
pub fn set_font(font: UiFont) {
    UI_FONT.store(font as u8, Ordering::Relaxed);
}

fn ui_font() -> UiFont {
    match UI_FONT.load(Ordering::Relaxed) {
        x if x == UiFont::Smooth as u8 => UiFont::Smooth,
        _ => UiFont::Large,
    }
}

thread_local! {
    /// The bundled glyphs, made when text needs them.
    static GLYPHS: UiGlyphs = UiGlyphs::default();
}

/// Picks the set of bundled 16-pixel glyphs of UI text, as the
/// bundled_glyphs16 setting says.
pub fn set_bundled_glyphs(set: BundledSet) {
    GLYPHS.with(|glyphs| glyphs.set_bundled(set));
}

/// Draws text in cells `8 * scale` pixels high: the 16x16 or smooth glyphs
/// `scale / 2` times their size if `scale` is even, else the 8x8 ones
/// `scale` times. Every character takes a whole cell, as on SmileBASIC's
/// screens: the help and the debugger show SmileBASIC text laid out for
/// that.
pub fn draw_text_scaled(
    fb: &mut Framebuffer,
    font: &Font,
    x: u32,
    y: u32,
    text: &str,
    color: u32,
    scale: u32,
) {
    draw_glyphs(fb, font, (x, y), text, color, scale, false);
}

/// Draws text one glyph at a time, half-width characters at half width
/// if `half`.
fn draw_glyphs(
    fb: &mut Framebuffer,
    font: &Font,
    (x, y): (u32, u32),
    text: &str,
    color: u32,
    scale: u32,
    half: bool,
) {
    let cell = 8 * scale;
    let font_choice = ui_font();
    // The fallback font follows the set too.
    let set = GLYPHS.with(UiGlyphs::bundled_set);
    let width = |c: u16| match half && is_half_width(c) {
        true => cell / 2,
        false => cell,
    };
    let area = Rect {
        x,
        y,
        w: text.encode_utf16().map(width).sum(),
        h: cell,
    };
    if dry(
        fb,
        area,
        ("text", x, y, text, color, scale, font_choice, half, set),
    ) {
        return;
    }
    let even = scale.is_multiple_of(2);
    let (size, scale) = match even {
        true => (FontSize::Large, scale / 2),
        false => (FontSize::Small, scale),
    };
    let smooth = font_choice == UiFont::Smooth && even;
    let halves = font.half_glyphs();
    // The 16-pixel glyphs keep their gray edges, the same way SmileBASIC 4
    // draws its own 16-pixel font: cut to one bit, their strokes come out
    // uneven. SmileBASIC's 8-pixel glyphs are dot drawings and stay sharp.
    // The bundled glyphs keep their gray at both sizes, the same way the
    // editor draws them.
    let font_gray = size == FontSize::Large;
    let put = |fb: &mut Framebuffer, px: u32, py: u32, a: u32, gray: bool| {
        let a = match gray {
            true => a,
            false if a >= 0x80 => 0xFF,
            false => 0,
        };
        if a != 0
            && let Some(dst) = fb.get(px, py)
        {
            fb.set(px, py, (dst & 0xFF00_0000) | blend(dst, color, a));
        }
    };
    // An imported font shows a box for each character it lacks: those
    // characters take the editor's bundled glyphs of the size drawn, as
    // in the editor. The stand-in font draws as before.
    let fill_gaps = font.is_smilebasic() && !smooth;
    let draw = |fb: &mut Framebuffer, glyph: &[u8], side: u32, (left, w): (u32, u32), gray| {
        for py in 0..cell {
            for px in 0..w {
                let a = u32::from(glyph[((py / scale) * side + px / scale) as usize]);
                put(fb, left + px, y + py, a, gray);
            }
        }
    };
    let mut left = x;
    let mut units = text.encode_utf16().peekable();
    let mut skip_low_surrogate = false;
    while let Some(c) = units.next() {
        let w = width(c);
        if std::mem::take(&mut skip_low_surrogate) {
            left += w;
            continue;
        }
        // A character beyond U+FFFF takes two cells, as SmileBASIC counts
        // it: its glyph goes on the first and the second stays empty.
        let pair = match units.peek() {
            Some(&low) if is_high(c) && is_low(low) => {
                Some(0x10000 + ((u32::from(c) - 0xD800) << 10) + (u32::from(low) - 0xDC00))
            }
            _ => None,
        };
        let bundled = (smooth && w == cell) || (fill_gaps && w == cell && !font.has_glyph(c));
        let glyph = match (smooth && w < cell, bundled) {
            (true, _) => GLYPHS.with(|glyphs| glyphs.get(c, true)),
            (false, true) => {
                let code = pair.unwrap_or(u32::from(c));
                GLYPHS.with(|glyphs| glyphs.bundled(code, size))
            }
            (false, false) => None,
        };
        if let Some(glyph) = glyph {
            let side = if w < cell {
                size as u32 / 2
            } else {
                size as u32
            };
            draw(fb, &glyph, side, (left, w), true);
            skip_low_surrogate = pair.is_some();
            left += w;
            continue;
        }
        match halves.get(c, size).filter(|_| w < cell) {
            Some(glyph) => draw(fb, glyph, size as u32 / 2, (left, w), font_gray),
            None => {
                for py in 0..cell {
                    for px in 0..cell {
                        let a = font.pixel(c, size, px / scale, py / scale) >> 24;
                        put(fb, left + px, y + py, a, font_gray);
                    }
                }
            }
        }
        left += w;
    }
}

fn is_high(unit: u16) -> bool {
    (0xD800..0xDC00).contains(&unit)
}

fn is_low(unit: u16) -> bool {
    (0xDC00..0xE000).contains(&unit)
}

/// Returns the rectangle of a side panel (the help, the index) on a
/// display of `w` x `h`: half of the display beside the editor, or `min_w`
/// if that is wider, below the menu bar, which stays shown over the editor.
pub fn side_rect((w, h): (u32, u32), left: bool, min_w: u32) -> Rect {
    let pw = (w / 2).max(min_w).min(w);
    Rect {
        x: if left { 0 } else { w - pw },
        y: crate::menu::MenuBar::HEIGHT,
        w: pw,
        h: h.saturating_sub(crate::menu::MenuBar::HEIGHT),
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
    colored_button(fb, font, r, label, hovered, enabled, (BORDER, HIGHLIGHT));
}

/// A button whose face is `faces.0`, or `faces.1` under the mouse,
/// while it can be pressed.
pub fn colored_button(
    fb: &mut Framebuffer,
    font: &Font,
    r: Rect,
    label: &str,
    hovered: bool,
    enabled: bool,
    faces: (u32, u32),
) {
    mark_button(fb, font, r, None, label, (hovered, enabled), faces);
}

/// A small drawing in a glyph's square, for buttons whose symbols the
/// console's font lacks (it has no ▶ or ⋯).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Mark {
    /// ▶, for buttons that run something.
    Play,
    /// ⋯, for buttons that open a menu.
    More,
    /// ▼
    Down,
    /// ▲
    Up,
    /// ×, for closing a window.
    Close,
}

/// Draws `mark` in the `CHAR`-square cell at `x`, `y`.
pub fn mark(fb: &mut Framebuffer, mark: Mark, x: u32, y: u32, color: u32) {
    let px =
        |fb: &mut Framebuffer, x: u32, y: u32, w: u32, h: u32| fill(fb, Rect { x, y, w, h }, color);
    match mark {
        Mark::Play => {
            for row in 0..11 {
                let w = (row + 1).min(11 - row);
                px(fb, x + 4, y + 2 + row, w, 1);
            }
        }
        Mark::More => {
            for i in 0..3 {
                px(fb, x + 1 + 5 * i, y + 7, 3, 3);
            }
        }
        Mark::Down | Mark::Up => {
            for row in 0..6 {
                let y = match mark {
                    Mark::Down => y + 5 + row,
                    _ => y + 10 - row,
                };
                px(fb, x + 2 + row, y, 12 - 2 * row, 1);
            }
        }
        Mark::Close => {
            for i in 0..10 {
                px(fb, x + 3 + i, y + 3 + i, 2, 2);
                px(fb, x + 11 - i, y + 3 + i, 2, 2);
            }
        }
    }
}

/// The width of a button's face holding `mark`, if any, and `label`,
/// with room on both sides.
pub fn mark_button_width(mark: Option<Mark>, label: &str) -> u32 {
    content_width(mark, label) + 4 * PAD
}

fn content_width(mark: Option<Mark>, label: &str) -> u32 {
    let gap = if label.is_empty() { 0 } else { PAD / 2 };
    mark.map_or(0, |_| CHAR + gap) + text_width(label)
}

/// A button with `mark_drawn` left of `label`, both centered;
/// `(hovered, enabled)` is whether it is under the mouse and whether it can
/// be pressed.
pub fn mark_button(
    fb: &mut Framebuffer,
    font: &Font,
    r: Rect,
    mark_drawn: Option<Mark>,
    label: &str,
    (hovered, enabled): (bool, bool),
    faces: (u32, u32),
) {
    let (face, color) = match (enabled, hovered) {
        (false, _) => (DISABLED_FACE, DISABLED),
        (true, true) => (faces.1, TEXT),
        (true, false) => (faces.0, TEXT),
    };
    fill(fb, r, face);
    let mut x = r.x + r.w.saturating_sub(content_width(mark_drawn, label)) / 2;
    if let Some(m) = mark_drawn {
        mark(fb, m, x, r.y + (r.h.saturating_sub(CHAR)) / 2, color);
        x += CHAR + PAD / 2;
    }
    draw_text(fb, font, x, r.y + 4, label, color);
}

/// Draws `pixels`, `w` wide and row by row, with its top left at `x`,
/// `y`, blending by their alpha. `key` must differ for pixels that
/// differ, because dry runs do not look at the pixels.
pub fn image(fb: &mut Framebuffer, x: u32, y: u32, w: u32, pixels: &[u32], key: u64) {
    let h = pixels.len() as u32 / w.max(1);
    if dry(fb, Rect { x, y, w, h }, ("image", x, y, w, key)) {
        return;
    }
    for (i, &src) in pixels.iter().enumerate() {
        let (px, py) = (x + i as u32 % w, y + i as u32 / w);
        let a = src >> 24;
        if a == 0 {
            continue;
        }
        let Some(dst) = fb.get(px, py) else {
            continue;
        };
        let out = if a == 0xFF { src } else { blend(dst, src, a) };
        fb.set(px, py, (dst & 0xFF00_0000) | (out & 0x00FF_FFFF));
    }
}

/// Draws `pixels` the same way as [`image`], each as a `scale` x `scale`
/// square, without making a larger copy of them.
pub fn image_scaled(
    fb: &mut Framebuffer,
    (x, y): (u32, u32),
    w: u32,
    pixels: &[u32],
    key: u64,
    scale: u32,
) {
    let h = pixels.len() as u32 / w.max(1);
    let area = Rect {
        x,
        y,
        w: w * scale,
        h: h * scale,
    };
    if dry(fb, area, ("image", x, y, w, key, scale)) {
        return;
    }
    for (i, &src) in pixels.iter().enumerate() {
        let a = src >> 24;
        if a == 0 {
            continue;
        }
        let (px, py) = (x + i as u32 % w * scale, y + i as u32 / w * scale);
        for (sx, sy) in (0..scale * scale).map(|n| (n % scale, n / scale)) {
            let Some(dst) = fb.get(px + sx, py + sy) else {
                continue;
            };
            let out = if a == 0xFF { src } else { blend(dst, src, a) };
            fb.set(px + sx, py + sy, (dst & 0xFF00_0000) | (out & 0x00FF_FFFF));
        }
    }
}

/// Returns `src` over `dst` at alpha `a` out of 255, per channel.
fn blend(dst: u32, src: u32, a: u32) -> u32 {
    let mix = |shift: u32| {
        let (d, s) = ((dst >> shift) & 0xFF, (src >> shift) & 0xFF);
        ((s * a + d * (255 - a)) / 255) << shift
    };
    mix(16) | mix(8) | mix(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn half_width_characters_take_half_a_cell() {
        assert_eq!(text_width("AB"), CHAR);
        assert_eq!(text_width("A가ｱ漢"), 3 * CHAR);
        assert_eq!(char_width('😀'), 2 * CHAR, "two UTF-16 units");
        let font = Font::fallback();
        let mut fb = Framebuffer::new(64, 16);
        draw_text(&mut fb, &font, 0, 0, "|가", 0xFFFF_FFFF);
        let inked =
            |x0: u32, x1: u32| (x0..x1).any(|x| (0..16).any(|y| fb.get(x, y) != Some(0xFF00_0000)));
        assert!(inked(0, CHAR / 2), "|");
        assert!(inked(CHAR / 2, CHAR / 2 + CHAR), "가 right after it");
        assert!(!inked(CHAR / 2 + CHAR, 64));
    }

    #[test]
    fn bundled_glyphs_fill_what_an_imported_font_lacks() {
        // A page font that has every glyph blank below U+AC00 and none
        // from there, like SmileBASIC 4's font, which lacks Hangul and
        // surrogates.
        let font = Font::blank_from(0xAC00);
        for scale in [2, 3, 4] {
            let cell = 8 * scale;
            let mut fb = Framebuffer::new(5 * cell, cell);
            draw_text_scaled(&mut fb, &font, 0, 0, "A가𠮷", 0xFFFF_FFFF, scale);
            let inked = |cells: u32| {
                (cells * cell..(cells + 1) * cell)
                    .any(|x| (0..cell).any(|y| fb.get(x, y) != Some(0xFF00_0000)))
            };
            assert!(!inked(0), "A is the font's own, blank, at {scale}");
            assert!(inked(1), "가 at {scale}");
            // The character beyond U+FFFF on its first cell, the second empty.
            assert!(inked(2), "𠮷 at {scale}");
            assert!(!inked(3), "𠮷's second cell at {scale}");
        }
    }

    #[test]
    fn sixteen_pixel_glyphs_keep_their_gray_edges() {
        let font = Font::fallback();
        let shades = |scale: u32| {
            let mut fb = Framebuffer::new(64, 32);
            draw_text_scaled(&mut fb, &font, 0, 0, "漢あ", 0xFFFF_FFFF, scale);
            let mut seen: Vec<u32> = fb.pixels().iter().map(|p| p & 0xFF).collect();
            seen.sort_unstable();
            seen.dedup();
            seen
        };
        // Black, white and grays between at an even scale...
        let even = shades(2);
        assert!(even.len() > 2, "{even:?}");
        assert_eq!((even[0], even[even.len() - 1]), (0, 0xFF));
        // ...and only black and white from the 8-pixel font.
        assert_eq!(shades(3), [0, 0xFF]);
    }
}
