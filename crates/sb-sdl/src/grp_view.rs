//! The debugger panel's GRP tab, an emulator extra after otya's SBDBG:
//! one graphic page at a time, zoomed and dragged about, with the pixel
//! under the pointer. The panel leaves the page's place clear and
//! [`PageTexture`] draws the page there, copying it to a texture again
//! only once it has changed.

use sb_runtime::Framebuffer;
use sb_runtime::font::Font;
use sb_runtime::graphics::{GRP_SIZE, Graphics, PAGES};
use sb_runtime::text::{FONT_PAGE, SPRITE_PAGE};
use sdl3::pixels::{Color, PixelFormat};
use sdl3::render::{BlendMode, Canvas, FRect, ScaleMode, Texture, TextureCreator};
use sdl3::sys::render::SDL_RenderTextureTiled;
use sdl3::video::{Window, WindowContext};

use crate::debug_panel::{HEADING, Hit, Metrics, TAB, cut};
use crate::ui::{DISABLED, HIGHLIGHT, Rect, TEXT, fill};

const SIZE: i64 = GRP_SIZE as i64;

/// Page pixels from `(x0, y0)` to before `(x1, y1)`.
type Area = (i64, i64, i64, i64);
/// Lines of the grid: where each goes, and whether it starts a cell.
type Lines = Vec<(f64, bool)>;
/// What the view shows around the page.
const BACKDROP: u32 = 0xFF10_1010;
/// The squares behind transparent pixels, this many panel pixels wide.
const CHECKER: u32 = 8;
const LIGHT: u32 = 0xFF9C_9C9C;
const DARK: u32 = 0xFF6C_6C6C;
/// From this zoom on, lines part the page's pixels, brighter every
/// [`CELL`] pixels to mark the 8x8 cells.
const GRID_FROM: u32 = 4;
const CELL: i64 = 8;
const GRID: Color = Color::RGBA(128, 128, 128, 72);
const GRID_CELL: Color = Color::RGBA(176, 176, 176, 160);
/// The most times a texture of the whole page is halved for a fitted
/// view.
const MOST_HALVED: u32 = 3;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Zoom {
    /// The whole page in the view.
    Fit,
    /// Panel pixels to a page pixel.
    Times(u32),
}

/// The zooms from the farthest out.
pub const ZOOMS: [Zoom; 5] = [
    Zoom::Fit,
    Zoom::Times(1),
    Zoom::Times(2),
    Zoom::Times(4),
    Zoom::Times(8),
];

impl Zoom {
    /// Panel pixels to a page pixel, as a numerator and a denominator, in
    /// a view of `(w, h)`.
    fn scale(self, (w, h): (u32, u32)) -> (i64, i64) {
        match self {
            Zoom::Fit => (i64::from(w.min(h).max(1)), SIZE),
            Zoom::Times(n) => (i64::from(n.max(1)), 1),
        }
    }

    pub fn label(self) -> String {
        match self {
            Zoom::Fit => "Fit".to_string(),
            Zoom::Times(n) => format!("{n}x"),
        }
    }

    /// The zoom `steps` further in, or out if negative.
    fn step(self, steps: i32) -> Zoom {
        let at = ZOOMS.iter().position(|&z| z == self).unwrap_or(1) as i32;
        ZOOMS[(at + steps).clamp(0, ZOOMS.len() as i32 - 1) as usize]
    }
}

/// The page the tab shows, and how.
pub struct GrpView {
    pub page: usize,
    pub zoom: Zoom,
    /// Where the page's top left corner is from the view's, in panel
    /// pixels.
    offset: (i64, i64),
    /// Where the view was last drawn; `None` while the tab is not shown.
    place: Option<Rect>,
    /// Where the pointer was while the left button drags the page.
    drag: Option<(i32, i32)>,
    /// The glyph of the font page last looked up, and the first character
    /// code that has it.
    glyph: Option<(u32, Option<u16>)>,
}

impl Default for GrpView {
    fn default() -> GrpView {
        GrpView {
            page: 0,
            zoom: Zoom::Times(1),
            offset: (0, 0),
            place: None,
            drag: None,
            glyph: None,
        }
    }
}

/// The part of the page a view shows.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shown {
    pub page: usize,
    pub zoom: Zoom,
    /// The view, in panel pixels.
    pub place: Rect,
    /// The page's pixels in it.
    pub pixels: Area,
    /// Where they go in the panel, `(x, y, w, h)`, which may pass the
    /// view's edges by part of a pixel.
    pub at: (f64, f64, f64, f64),
}

impl Shown {
    /// The part of the view that the page covers.
    pub fn clear(&self) -> Option<Rect> {
        let (x, y, w, h) = self.at;
        let p = self.place;
        let span = |from: f64, len: f64, start: u32, room: u32| {
            let a = (from.floor() as i64).max(i64::from(start));
            let b = ((from + len).ceil() as i64).min(i64::from(start + room));
            (a, b)
        };
        let (x0, x1) = span(x, w, p.x, p.w);
        let (y0, y1) = span(y, h, p.y, p.h);
        (x1 > x0 && y1 > y0).then(|| Rect {
            x: x0 as u32,
            y: y0 as u32,
            w: (x1 - x0) as u32,
            h: (y1 - y0) as u32,
        })
    }

    /// The lines between pixels when zoomed in that far, across and
    /// down, in panel pixels.
    pub fn grid(&self) -> (Lines, Lines) {
        let Zoom::Times(n) = self.zoom else {
            return Default::default();
        };
        if n < GRID_FROM {
            return Default::default();
        }
        let (x0, y0, x1, y1) = self.pixels;
        let lines = |from: i64, to: i64, at: f64| -> Lines {
            (from + 1..to)
                .map(|p| (at + ((p - from) * i64::from(n)) as f64, p % CELL == 0))
                .collect()
        };
        (lines(x0, x1, self.at.0), lines(y0, y1, self.at.1))
    }
}

/// What the tab shows of the page, gathered from the machine each frame.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Probe {
    /// Nothing has been drawn on the page.
    pub blank: bool,
    /// The pixel under the pointer, and its colour.
    pub pixel: Option<((i32, i32), u32)>,
}

impl GrpView {
    fn scale(&self, place: Rect) -> (i64, i64) {
        self.zoom.scale((place.w, place.h))
    }

    /// Keeps the page in the view: in the middle while it fits, else
    /// with no space past its edges.
    fn clamp(&mut self) {
        let Some(p) = self.place else {
            return;
        };
        let (num, den) = self.scale(p);
        let extent = SIZE * num / den;
        let fit = |offset: i64, room: u32| {
            let room = i64::from(room);
            match extent <= room {
                true => (room - extent) / 2,
                false => offset.clamp(room - extent, 0),
            }
        };
        self.offset = (fit(self.offset.0, p.w), fit(self.offset.1, p.h));
    }

    /// Where the view is drawn, or `None` while the tab is not shown.
    pub fn set_place(&mut self, place: Option<Rect>) {
        self.place = place;
        self.clamp();
    }

    /// The page pixel at a point of the panel, if the view shows one
    /// there.
    pub fn page_at(&self, (x, y): (i32, i32)) -> Option<(i32, i32)> {
        let p = self.place.filter(|p| p.contains((x, y)))?;
        let (num, den) = self.scale(p);
        let at = |v: i32, start: u32, offset: i64| {
            ((i64::from(v) - i64::from(start) - offset) * den).div_euclid(num)
        };
        let (px, py) = (at(x, p.x, self.offset.0), at(y, p.y, self.offset.1));
        let inside = (0..SIZE).contains(&px) && (0..SIZE).contains(&py);
        inside.then_some((px as i32, py as i32))
    }

    /// Zooms, keeping the page pixel at `at` where it is, or else the
    /// one in the middle of the view.
    pub fn zoom_to(&mut self, zoom: Zoom, at: Option<(i32, i32)>) {
        let Some(p) = self.place else {
            self.zoom = zoom;
            return;
        };
        let (n1, d1) = self.scale(p);
        let (n2, d2) = zoom.scale((p.w, p.h));
        let (ax, ay) = at
            .filter(|&a| p.contains(a))
            .map_or((i64::from(p.w / 2), i64::from(p.h / 2)), |(x, y)| {
                (i64::from(x) - i64::from(p.x), i64::from(y) - i64::from(p.y))
            });
        let keep = |a: i64, offset: i64| a - ((a - offset) * n2 * d1).div_euclid(d2 * n1);
        self.offset = (keep(ax, self.offset.0), keep(ay, self.offset.1));
        self.zoom = zoom;
        self.clamp();
    }

    /// The wheel over the view zooms in or out at the pointer; whether
    /// it was over the view.
    pub fn wheel(&mut self, rows: i32, at: Option<(i32, i32)>) -> bool {
        let Some(at) = at.filter(|&a| self.place.is_some_and(|p| p.contains(a))) else {
            return false;
        };
        if rows != 0 {
            self.zoom_to(self.zoom.step(-rows.signum()), Some(at));
        }
        true
    }

    /// The left button went down at `at` on the view.
    pub fn press(&mut self, at: (i32, i32)) {
        self.drag = Some(at);
    }

    /// The pointer moved, dragging the page while the button is down.
    pub fn drag_to(&mut self, at: (i32, i32)) {
        if let Some(from) = self.drag.replace(at) {
            let (dx, dy) = (i64::from(at.0 - from.0), i64::from(at.1 - from.1));
            self.offset = (self.offset.0 + dx, self.offset.1 + dy);
            self.clamp();
        }
    }

    pub fn release(&mut self) {
        self.drag = None;
    }

    /// The part of the page the view shows, while the tab is shown.
    pub fn shown(&self) -> Option<Shown> {
        let p = self.place?;
        let (num, den) = self.scale(p);
        let range = |offset: i64, room: u32| {
            let first = (-offset * den).div_euclid(num).max(0);
            let end = ((i64::from(room) - offset) * den + num - 1).div_euclid(num);
            (first, end.clamp(first, SIZE))
        };
        let (x0, x1) = range(self.offset.0, p.w);
        let (y0, y1) = range(self.offset.1, p.h);
        let to = |v: i64, start: u32, offset: i64| {
            f64::from(start) + offset as f64 + (v * num) as f64 / den as f64
        };
        let (ax, ay) = (to(x0, p.x, self.offset.0), to(y0, p.y, self.offset.1));
        let (bx, by) = (to(x1, p.x, self.offset.0), to(y1, p.y, self.offset.1));
        Some(Shown {
            page: self.page,
            zoom: self.zoom,
            place: p,
            pixels: (x0, y0, x1, y1),
            at: (ax, ay, bx - ax, by - ay),
        })
    }

    /// Whether the page is blank, and the pixel under `pointer`.
    pub fn probe(&self, pointer: Option<(i32, i32)>, grp: &Graphics) -> Probe {
        let pixel = pointer.and_then(|at| self.page_at(at)).map(|(x, y)| {
            let color = grp.pixel(self.page, x, y);
            ((x, y), color)
        });
        Probe {
            blank: grp.is_blank(self.page),
            pixel,
        }
    }

    /// Draws the tab from `(x, y)` to `(right, bottom)`: buttons for the
    /// pages, the view, whose page [`PageTexture`] draws, and what the
    /// pixel under the pointer is.
    #[allow(clippy::too_many_arguments)]
    pub fn draw(
        &mut self,
        fb: &mut Framebuffer,
        font: &Font,
        m: &Metrics,
        (x, y): (u32, u32),
        (right, bottom): (u32, u32),
        probe: &Probe,
        hits: &mut Vec<(Rect, Hit)>,
        tips: &mut Vec<(Rect, Vec<String>)>,
    ) {
        let width = right.saturating_sub(x);
        let mut bx = x;
        for page in 0..PAGES {
            let r = Rect {
                x: bx,
                y,
                w: m.char + m.pad,
                h: m.line - 2,
            };
            fill(fb, r, if page == self.page { HIGHLIGHT } else { TAB });
            m.draw(
                fb,
                font,
                bx + m.pad / 2,
                y + m.text - 1,
                &page.to_string(),
                TEXT,
            );
            hits.push((r, Hit::GrpPage(page)));
            let tip = match page {
                SPRITE_PAGE => "GRP4: the sprites' page at start-up",
                FONT_PAGE => "GRP5, #GRPF: the font",
                _ => "",
            };
            if !tip.is_empty() {
                tips.push((r, vec![tip.to_string()]));
            }
            bx += r.w + m.pad / 2;
        }
        let mut about = self.zoom.label();
        if self.page == FONT_PAGE {
            about.push_str(" #GRPF");
        }
        if probe.blank {
            about.push_str(" blank");
        }
        let room = (right.saturating_sub(bx + m.pad / 2) / m.char) as usize;
        m.draw(
            fb,
            font,
            bx + m.pad / 2,
            y + m.text,
            &cut(&about, room),
            HEADING,
        );

        let top = y + m.line;
        let info_y = bottom.saturating_sub(m.line);
        let view = Rect {
            x,
            y: top,
            w: width,
            h: info_y.saturating_sub(top + m.pad / 2),
        };
        if view.h < m.line || view.w < m.line {
            self.set_place(None);
            return;
        }
        self.set_place(Some(view));
        fill(fb, view, BACKDROP);
        if let Some(clear) = self.shown().and_then(|s| s.clear()) {
            fill(fb, clear, 0);
        }
        hits.push((view, Hit::Grp));

        let row = Rect {
            x,
            y: info_y,
            w: width,
            h: m.line,
        };
        let Some(((px, py), color)) = probe.pixel else {
            let hint = "Wheel: zoom  Drag: move  Right click: more";
            let room = (width / m.char) as usize;
            m.draw(fb, font, x, info_y + m.text, &cut(hint, room), DISABLED);
            return;
        };
        let swatch = Rect {
            x,
            y: info_y + m.text,
            w: m.char,
            h: m.line - 2 * m.text,
        };
        // Opaque, so that a transparent pixel's colour still shows.
        fill(fb, swatch, color | 0xFF00_0000);
        let mut text = pixel_text((px, py), color);
        let cell = match self.page {
            FONT_PAGE => font_cell(px, py).map(|(size, n)| {
                let code = self.glyph_code(font, n);
                let code = code.map_or(String::new(), |c| format!(" CHR$(&H{c:X})"));
                format!("{size}x{size} #{n}{code}")
            }),
            _ => Some(format!("16x16 #{}", grid_cell(px, py))),
        };
        if let Some(cell) = cell {
            text.push_str("  ");
            text.push_str(&cell);
        }
        let tx = x + 2 * m.char;
        let room = (right.saturating_sub(tx) / m.char) as usize;
        m.draw(fb, font, tx, info_y + m.text, &cut(&text, room), TEXT);
        if text.chars().count() > room {
            tips.push((row, vec![text]));
        }
    }

    /// The first character code whose glyph is the font page's glyph
    /// `n`, looked up again only for another glyph.
    fn glyph_code(&mut self, font: &Font, n: u32) -> Option<u16> {
        match self.glyph {
            Some((g, code)) if g == n => code,
            _ => {
                let code = (0..=u16::MAX).find(|&c| font.glyph_index(c) == Some(n));
                self.glyph = Some((n, code));
                code
            }
        }
    }
}

/// A pixel's place and colour, as `&HAARRGGBB` and its channels.
pub fn pixel_text((x, y): (i32, i32), color: u32) -> String {
    let [a, r, g, b] = color.to_be_bytes();
    format!("X,Y {x},{y}  &H{color:08X}  R,G,B,A {r},{g},{b},{a}")
}

/// The glyph of the font page at a pixel, and its size: 8x8 glyphs fill
/// the top half's left quarter, 16x16 ones the bottom half's left half.
pub fn font_cell(x: i32, y: i32) -> Option<(u32, u32)> {
    let (x, y) = (u32::try_from(x).ok()?, u32::try_from(y).ok()?);
    match (x, y) {
        (0..512, 0..1024) => Some((8, y / 8 * 64 + x / 8)),
        (0..1024, 1024..2048) => Some((16, (y - 1024) / 16 * 64 + x / 16)),
        _ => None,
    }
}

/// The 16x16 cell of a pixel, numbered as SPDEF's templates and BG's
/// characters lay them out, 128 to a row.
pub fn grid_cell(x: i32, y: i32) -> i32 {
    y / 16 * 128 + x / 16
}

/// The GRP tab's page as a texture, for one window's renderer.
#[derive(Default)]
pub struct PageTexture<'a> {
    page: Option<Texture<'a>>,
    /// How many times the texture's size halves the page's.
    halved: u32,
    /// The page copied, its count of changes then, and the part of it
    /// copied.
    copied: Option<(usize, u64, Area)>,
    checker: Option<Texture<'a>>,
    /// The page sampled down for a halved texture.
    sampled: Vec<u32>,
}

impl<'a> PageTexture<'a> {
    /// Lets go of the textures while the tab is not shown.
    pub fn release(&mut self) {
        if self.page.is_some() || self.checker.is_some() {
            *self = PageTexture::default();
        }
    }

    /// Draws the page where the panel at `display` left its place, for a
    /// panel `display_w` pixels wide. Copies the page to the texture
    /// only where the texture's copy is out of date.
    pub fn draw(
        &mut self,
        canvas: &mut Canvas<Window>,
        creator: &'a TextureCreator<WindowContext>,
        grp: &Graphics,
        view: &GrpView,
        display: FRect,
        display_w: u32,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let Some(shown) = view.shown() else {
            self.release();
            return Ok(());
        };
        let Some(clear) = shown.clear() else {
            return Ok(());
        };
        let s = f64::from(display.w) / f64::from(display_w.max(1));
        let place = |(x, y, w, h): (f64, f64, f64, f64)| {
            FRect::new(
                (f64::from(display.x) + x * s) as f32,
                (f64::from(display.y) + y * s) as f32,
                (w * s) as f32,
                (h * s) as f32,
            )
        };
        let hole = place((
            f64::from(clear.x),
            f64::from(clear.y),
            f64::from(clear.w),
            f64::from(clear.h),
        ));

        if self.checker.is_none() {
            let mut made = creator.create_texture_static(PixelFormat::ARGB8888, 2, 2)?;
            made.update(None, crate::as_bytes(&[LIGHT, DARK, DARK, LIGHT]), 8)?;
            made.set_scale_mode(ScaleMode::Nearest);
            self.checker = Some(made);
        }
        if let Some(checker) = &self.checker {
            let dst = hole.to_ll();
            let tile = (f64::from(CHECKER) * s) as f32;
            // SAFETY: the renderer and texture are valid while borrowed,
            // and `dst` outlives the call.
            unsafe {
                SDL_RenderTextureTiled(canvas.raw(), checker.raw(), std::ptr::null(), tile, &dst)
            };
        }

        let page = shown.page;
        if grp.is_blank(page) {
            self.copied = None;
            return Ok(());
        }
        // Fitted, a smaller texture sampled from the page does.
        let halved = match shown.zoom {
            Zoom::Times(_) => 0,
            Zoom::Fit => {
                let extent = shown.at.2 * s;
                (0..=MOST_HALVED)
                    .rev()
                    .find(|&h| (SIZE >> h) as f64 >= extent)
                    .unwrap_or(0)
            }
        };
        if self.page.is_none() || self.halved != halved {
            let side = (SIZE >> halved) as u32;
            let mut made = creator.create_texture_streaming(PixelFormat::ARGB8888, side, side)?;
            made.set_blend_mode(BlendMode::Blend);
            made.set_scale_mode(match halved {
                0 => ScaleMode::Nearest,
                _ => ScaleMode::Linear,
            });
            self.page = Some(made);
            self.halved = halved;
            self.copied = None;
        }
        let (x0, y0, x1, y1) = shown.pixels;
        let needed = match halved {
            0 => shown.pixels,
            _ => (0, 0, SIZE, SIZE),
        };
        let changes = grp.changes(page);
        let fresh = self.copied.is_some_and(|(p, c, (a, b, c2, d))| {
            p == page
                && c == changes
                && a <= needed.0
                && b <= needed.1
                && c2 >= needed.2
                && d >= needed.3
        });
        let Some(tex) = &mut self.page else {
            return Ok(());
        };
        if !fresh {
            let pixels = grp.page(page);
            if halved == 0 {
                let start = (y0 * SIZE + x0) as usize;
                let end = ((y1 - 1) * SIZE + x1) as usize;
                let rect =
                    sdl3::rect::Rect::new(x0 as i32, y0 as i32, (x1 - x0) as u32, (y1 - y0) as u32);
                crate::bench::uploaded((end - start) * 4);
                tex.update(
                    rect,
                    crate::as_bytes(&pixels[start..end]),
                    (SIZE * 4) as usize,
                )?;
            } else {
                let side = (SIZE >> halved) as usize;
                let step = 1 << halved;
                let row = SIZE as usize * step;
                self.sampled.clear();
                self.sampled.extend(
                    (0..side).flat_map(|y| (0..side).map(move |x| pixels[y * row + x * step])),
                );
                crate::bench::uploaded(self.sampled.len() * 4);
                tex.update(None, crate::as_bytes(&self.sampled), side * 4)?;
            }
            self.copied = Some((page, changes, needed));
        }
        let src = FRect::new(
            (x0 >> halved) as f32,
            (y0 >> halved) as f32,
            ((x1 - x0) >> halved) as f32,
            ((y1 - y0) >> halved) as f32,
        );
        canvas.copy(tex, src, place(shown.at))?;

        let (across, down) = shown.grid();
        if across.is_empty() && down.is_empty() {
            return Ok(());
        }
        // A window pixel wide, however large the panel is drawn.
        let line = 1.0;
        let mut lines: [Vec<FRect>; 2] = Default::default();
        for (at, cell) in across {
            if at > f64::from(clear.x) && at < f64::from(clear.x + clear.w) {
                let r = place((at, f64::from(clear.y), 0.0, f64::from(clear.h)));
                lines[usize::from(cell)].push(FRect::new(r.x, r.y, line, r.h));
            }
        }
        for (at, cell) in down {
            if at > f64::from(clear.y) && at < f64::from(clear.y + clear.h) {
                let r = place((f64::from(clear.x), at, f64::from(clear.w), 0.0));
                lines[usize::from(cell)].push(FRect::new(r.x, r.y, r.w, line));
            }
        }
        canvas.set_blend_mode(BlendMode::Blend);
        for (rects, color) in lines.iter().zip([GRID, GRID_CELL]) {
            canvas.set_draw_color(color);
            canvas.fill_rects(rects)?;
        }
        canvas.set_blend_mode(BlendMode::None);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view(zoom: Zoom) -> GrpView {
        let mut v = GrpView {
            zoom,
            ..GrpView::default()
        };
        v.set_place(Some(Rect {
            x: 100,
            y: 50,
            w: 400,
            h: 200,
        }));
        v
    }

    #[test]
    fn pixels_under_points() {
        let v = view(Zoom::Times(1));
        assert_eq!(v.page_at((100, 50)), Some((0, 0)));
        assert_eq!(v.page_at((499, 249)), Some((399, 199)));
        assert_eq!(v.page_at((99, 50)), None);
        let v = view(Zoom::Times(4));
        assert_eq!(v.page_at((103, 53)), Some((0, 0)));
        assert_eq!(v.page_at((104, 57)), Some((1, 1)));
        // Fitted, the page is 200 pixels square in the middle.
        let v = view(Zoom::Fit);
        assert_eq!(v.page_at((199, 50)), None);
        assert_eq!(v.page_at((200, 50)), Some((0, 0)));
        assert_eq!(v.page_at((399, 249)), Some((2037, 2037)));
        assert_eq!(v.page_at((400, 100)), None);
    }

    #[test]
    fn zooming_keeps_the_pixel_under_the_pointer() {
        let mut v = view(Zoom::Times(1));
        let at = (300, 150);
        let before = v.page_at(at);
        assert!(v.wheel(-1, Some(at)));
        assert_eq!(v.zoom, Zoom::Times(2));
        assert_eq!(v.page_at(at), before);
        v.wheel(-1, Some(at));
        v.wheel(-1, Some(at));
        v.wheel(-1, Some(at));
        assert_eq!(v.zoom, Zoom::Times(8));
        assert_eq!(v.page_at(at), before);
        // Out to the whole page and no further.
        for _ in 0..6 {
            v.wheel(1, Some(at));
        }
        assert_eq!(v.zoom, Zoom::Fit);
        assert!(!v.wheel(1, Some((0, 0))));
        // In from the whole page, about where the pointer is.
        v.wheel(-1, Some((300, 150)));
        assert_eq!(v.zoom, Zoom::Times(1));
        let (x, y) = v.page_at((300, 150)).unwrap();
        assert!((1015..=1035).contains(&x) && (1015..=1035).contains(&y));
    }

    #[test]
    fn dragging_stops_at_the_edges() {
        let mut v = view(Zoom::Times(2));
        v.press((300, 150));
        v.drag_to((290, 140));
        assert_eq!(v.page_at((100, 50)), Some((5, 5)));
        // Not past the top left corner.
        v.drag_to((400, 400));
        assert_eq!(v.page_at((100, 50)), Some((0, 0)));
        // Nor past the bottom right one.
        v.drag_to((-10_000, -10_000));
        assert_eq!(v.page_at((499, 249)), Some((2047, 2047)));
        v.release();
        v.drag_to((0, 0));
        assert_eq!(v.page_at((499, 249)), Some((2047, 2047)));
    }

    #[test]
    fn the_part_shown() {
        let mut v = view(Zoom::Times(4));
        v.press((0, 0));
        v.drag_to((-6, -2));
        let s = v.shown().unwrap();
        // A pixel and a half in, the first pixel shows in part.
        assert_eq!(s.pixels, (1, 0, 102, 51));
        assert_eq!(s.at, (98.0, 48.0, 404.0, 204.0));
        assert_eq!(
            s.clear(),
            Some(Rect {
                x: 100,
                y: 50,
                w: 400,
                h: 200
            })
        );
        let (across, down) = s.grid();
        assert_eq!(across.len(), 100);
        assert_eq!(across[0], (102.0, false));
        assert_eq!(across[6], (126.0, true));
        assert_eq!(down.len(), 50);
        // No grid below 4x, and the whole page fitted.
        let s = view(Zoom::Fit).shown().unwrap();
        assert_eq!(s.pixels, (0, 0, 2048, 2048));
        assert_eq!(s.at, (200.0, 50.0, 200.0, 200.0));
        assert_eq!(s.grid(), (Vec::new(), Vec::new()));
        assert_eq!(view(Zoom::Times(2)).shown().unwrap().grid().0.len(), 0);
    }

    #[test]
    fn cells_and_colours() {
        assert_eq!(
            pixel_text((3, 4), 0x80FF_2001),
            "X,Y 3,4  &H80FF2001  R,G,B,A 255,32,1,128"
        );
        // As FONTINFO places "A" at 8,8 for 8x8 and "漢" at 384,1376.
        assert_eq!(font_cell(8, 8), Some((8, 65)));
        assert_eq!(font_cell(399, 1391), Some((16, 1432)));
        assert_eq!(font_cell(600, 8), None);
        assert_eq!(grid_cell(17, 16), 129);
    }
}
