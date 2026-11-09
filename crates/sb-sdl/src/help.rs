//! The help panel, which shows SmileBASIC 4's own help for a command
//! beside the editor: one page at a time, scrolled when it is long.

use sb_runtime::Framebuffer;
use sb_runtime::font::Font;
use sb_runtime::help::{INTRO, Page, Style};
use sdl3::keyboard::Keycode;

use crate::ui::{
    BORDER, DISABLED, LINE, PAD, Rect, TEXT, draw_text, draw_text_scaled, fill, panel, side_rect,
};

const HINT: &str = "Left/Right: page  F2: size  Esc: close";
/// The panel is at least this many characters wide, as SmileBASIC 4's
/// help is.
const MIN_COLS: u32 = 38;
/// The scroll bar's width, and its space.
const BAR: u32 = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Stay,
    Close,
}

pub struct HelpViewer {
    /// The display size the panel is on.
    size: (u32, u32),
    /// The panel is on the display's left, away from the editor in use.
    pub left: bool,
    /// The glyphs are drawn this many times their size.
    scale: u32,
    title: String,
    source: Vec<Page>,
    /// The rows of each page, wrapped to the panel's width.
    pages: Vec<Vec<(Style, String)>>,
    page: usize,
    /// The first row shown.
    scroll: usize,
}

fn color(style: Style) -> u32 {
    match style {
        Style::Title => 0xFFFF_E070,
        Style::Section => 0xFF70_D0FF,
        Style::Argument => 0xFF90_F090,
        Style::Summary => TEXT,
        Style::Body => 0xFFD0_D0D0,
        Style::Rule => BORDER,
    }
}

/// Breaks a line at spaces to fit `cols` characters. Lines of bullets
/// (`・`) hang their continuation under the text.
fn wrap(text: &str, cols: usize) -> Vec<String> {
    let hang = if text.starts_with('・') { 1 } else { 0 };
    let mut out = Vec::new();
    let mut rest: Vec<char> = text.chars().collect();
    let mut indent = 0;
    while rest.len() + indent > cols && cols > indent + 1 {
        let room = cols - indent;
        let cut = rest[..=room.min(rest.len() - 1)]
            .iter()
            .rposition(|&c| c == ' ')
            .filter(|&at| at > 0 && at <= room)
            .unwrap_or(room);
        let head: String = rest[..cut].iter().collect();
        out.push(format!("{}{head}", " ".repeat(indent)));
        rest.drain(..cut);
        while rest.first() == Some(&' ') {
            rest.remove(0);
        }
        indent = hang;
    }
    let tail: String = rest.iter().collect();
    out.push(format!("{}{tail}", " ".repeat(indent)));
    out
}

impl HelpViewer {
    pub fn new(size: (u32, u32), topic: &str, pages: &[Page], scale: u32) -> HelpViewer {
        let mut viewer = HelpViewer {
            size,
            left: false,
            scale: scale.max(1),
            title: if topic == INTRO {
                "Help".to_string()
            } else {
                format!("Help: {topic}")
            },
            source: pages.to_vec(),
            pages: Vec::new(),
            page: 0,
            scroll: 0,
        };
        viewer.wrap_pages();
        viewer
    }

    fn wrap_pages(&mut self) {
        let cols = self.cols();
        self.pages = (self.source.iter())
            .map(|page| {
                let mut rows = Vec::new();
                for line in &page.lines {
                    // A rule is drawn as a line, not as characters.
                    let wrapped = match line.style {
                        Style::Rule => vec![String::new()],
                        _ => wrap(&line.text, cols),
                    };
                    rows.extend(wrapped.into_iter().map(|row| (line.style, row)));
                }
                rows
            })
            .collect();
        self.scroll = self.scroll.min(self.max_scroll());
    }

    /// Goes on a display of another size, as when the editor's window is
    /// resized, keeping the page.
    pub fn set_size(&mut self, size: (u32, u32)) {
        if size != self.size {
            self.size = size;
            self.wrap_pages();
        }
    }

    /// Draws the text at another size, keeping the page.
    pub fn set_scale(&mut self, scale: u32) {
        if scale != self.scale {
            self.scale = scale.max(1);
            self.wrap_pages();
        }
    }

    fn cell(&self) -> u32 {
        8 * self.scale
    }

    fn row_height(&self) -> u32 {
        self.cell() + self.cell() / 2
    }

    pub fn frame(&self) -> Rect {
        let min_w = MIN_COLS * 8 * self.scale + 4 * PAD + BAR;
        side_rect(self.size, self.left, min_w)
    }

    /// The characters that fit in a row.
    fn cols(&self) -> usize {
        (self.frame().w.saturating_sub(4 * PAD + BAR) / self.cell()).max(1) as usize
    }

    /// The top of the first row.
    fn body_top(&self) -> u32 {
        self.frame().y + LINE + 2 * PAD
    }

    /// The rows that fit between the title bar and the hint.
    fn visible(&self) -> usize {
        let f = self.frame();
        let body = f.h.saturating_sub(LINE + 2 * PAD + LINE + 2 * PAD);
        (body / self.row_height()).max(1) as usize
    }

    fn max_scroll(&self) -> usize {
        let rows = self.pages.get(self.page).map_or(0, Vec::len);
        rows.saturating_sub(self.visible())
    }

    fn scroll_by(&mut self, rows: isize) {
        self.scroll = self
            .scroll
            .saturating_add_signed(rows)
            .min(self.max_scroll());
    }

    fn turn(&mut self, to: usize) {
        if to < self.pages.len() {
            self.page = to;
            self.scroll = 0;
        }
    }

    pub fn wheel(&mut self, rows: i32) {
        self.scroll_by(3 * rows as isize);
    }

    pub fn key(&mut self, key: Keycode) -> Outcome {
        let screen = self.visible().saturating_sub(1).max(1) as isize;
        match key {
            Keycode::Escape | Keycode::F1 | Keycode::Return | Keycode::KpEnter => {
                return Outcome::Close;
            }
            Keycode::Left => self.turn(self.page.wrapping_sub(1)),
            Keycode::Right => self.turn(self.page + 1),
            Keycode::Up => self.scroll_by(-1),
            Keycode::Down => self.scroll_by(1),
            Keycode::PageUp => self.scroll_by(-screen),
            Keycode::PageDown => self.scroll_by(screen),
            Keycode::Home => self.scroll = 0,
            Keycode::End => self.scroll = self.max_scroll(),
            _ => {}
        }
        Outcome::Stay
    }

    /// Draws the panel; its title bar is gray while the keys go to the
    /// editor.
    pub fn draw(&self, fb: &mut Framebuffer, font: &Font, focused: bool) {
        let f = self.frame();
        let title = match self.pages.len() {
            0 | 1 => self.title.clone(),
            n => format!("{} ({}/{n})", self.title, self.page + 1),
        };
        panel(fb, font, f, &title, focused);
        let left = f.x + 2 * PAD;
        let rows = self.pages.get(self.page).map_or(&[][..], Vec::as_slice);
        let height = self.row_height();
        for (i, (style, text)) in rows
            .iter()
            .skip(self.scroll)
            .take(self.visible())
            .enumerate()
        {
            let y = self.body_top() + i as u32 * height;
            if *style == Style::Rule {
                let rule = Rect {
                    x: left,
                    y: y + height / 2 - 1,
                    w: f.w - 4 * PAD - BAR,
                    h: 2,
                };
                fill(fb, rule, color(Style::Rule));
            } else {
                let y = y + (height - self.cell()) / 2;
                draw_text_scaled(fb, font, left, y, text, color(*style), self.scale);
            }
        }
        if rows.len() > self.visible() {
            let track = self.visible() as u32 * height;
            let thumb = (track * self.visible() as u32 / rows.len() as u32).max(height / 2);
            let at = (track - thumb) * self.scroll as u32 / self.max_scroll().max(1) as u32;
            let x = f.x + f.w - PAD - 2;
            let mut bar = |y, h, c| fill(fb, Rect { x, y, w: 4, h }, c);
            bar(self.body_top(), track, 0xFF30_3030);
            bar(self.body_top() + at, thumb, DISABLED);
        }
        draw_text(fb, font, left, f.y + f.h - LINE - PAD, HINT, DISABLED);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wraps_at_spaces() {
        assert_eq!(wrap("abc def ghi", 7), ["abc def", "ghi"]);
        assert_eq!(wrap("abc", 7), ["abc"]);
        assert_eq!(wrap("", 7), [""]);
        // A word longer than the row is cut.
        assert_eq!(wrap("abcdefghij", 4), ["abcd", "efgh", "ij"]);
        assert_eq!(wrap("・abc def ghi", 6), ["・abc", " def", " ghi"]);
    }
}
