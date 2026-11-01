//! Drawing for the emulator's own UI, at display resolution.

use std::cell::RefCell;
use std::hash::{DefaultHasher, Hash, Hasher};
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

/// Either rectangle, or the smallest around both.
pub fn union(a: Option<Rect>, b: Option<Rect>) -> Option<Rect> {
    match (a, b) {
        (Some(a), Some(b)) => Some(a.union(b)),
        _ => a.or(b),
    }
}

/// What a dry run of drawing gathers: a fingerprint of what would be
/// drawn, and around where.
struct Dry {
    state: DefaultHasher,
    bounds: Option<Rect>,
    translucent: Option<Rect>,
}

/// What a dry run found would be drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Drawn {
    /// A fingerprint of everything drawn, in order: drawing that gives
    /// the same one draws the same pixels on the same ones.
    pub key: u64,
    /// Around everything drawn.
    pub bounds: Option<Rect>,
    /// Around what is filled partly see-through, which lets what is
    /// under the frame show.
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
/// false. `what` must tell apart whatever draws differently there.
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

pub fn text_width(text: &str) -> u32 {
    text.chars().count() as u32 * CHAR
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
    let large = LARGE_FONT.load(Ordering::Relaxed);
    let area = Rect {
        x,
        y,
        w: (text.encode_utf16().count() as u32).saturating_mul(cell),
        h: cell,
    };
    if dry(fb, area, ("text", x, y, text, color, scale, large)) {
        return;
    }
    let (size, scale) = if large && scale.is_multiple_of(2) {
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

/// A button with `mark` left of `label`, both centred; `state` is
/// whether it is under the mouse and whether it can be pressed.
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
/// differ, for dry runs, which do not look at them.
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

/// `src` over `dst` at alpha `a` of 255, by channel.
fn blend(dst: u32, src: u32, a: u32) -> u32 {
    let mix = |shift: u32| {
        let (d, s) = ((dst >> shift) & 0xFF, (src >> shift) & 0xFF);
        ((s * a + d * (255 - a)) / 255) << shift
    };
    mix(16) | mix(8) | mix(0)
}
