//! The editor colors window: picks a preset palette, or sets each color
//! by typing its hex value, and imports or exports palettes as files.
//! It sits at the right of the display without dimming it, so that the
//! editor behind shows the colors as they change.

use sb_runtime::Framebuffer;
use sb_runtime::font::Font;
use sb_runtime::palette::{PRESETS, Palette, Part, format_color, parse_color};
use sdl3::keyboard::Keycode;

use crate::ui::{
    BORDER, CHAR, HIGHLIGHT, LINE, PAD, Rect, TEXT, button, draw_text, fill, outline, text_width,
    window,
};

/// Wide enough for the longest preset name and its mark.
const PRESETS_WIDTH: u32 = 20 * CHAR;
const COLORS_WIDTH: u32 = 25 * CHAR;
const SWATCH: u32 = 2 * CHAR;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Stay,
    Close,
    /// Ask for a palette file to load.
    Import,
    /// Ask where to save the palette.
    Export,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Button {
    Import,
    Export,
    Close,
}

const BUTTONS: [Button; 3] = [Button::Import, Button::Export, Button::Close];

impl Button {
    fn label(self) -> &'static str {
        match self {
            Button::Import => "Import...",
            Button::Export => "Export...",
            Button::Close => "Close",
        }
    }

    fn outcome(self) -> Outcome {
        match self {
            Button::Import => Outcome::Import,
            Button::Export => Outcome::Export,
            Button::Close => Outcome::Close,
        }
    }
}

pub struct ColorWindow {
    /// The display size the window is placed in.
    size: (u32, u32),
    mouse: Option<(i32, i32)>,
    /// The index in [`Part::ALL`] of the color being set.
    selected: usize,
    /// Hex digits typed for the selected color so far.
    typed: Option<String>,
}

impl ColorWindow {
    pub fn new(size: (u32, u32)) -> ColorWindow {
        ColorWindow {
            size,
            mouse: None,
            selected: 0,
            typed: None,
        }
    }

    fn frame(&self) -> Rect {
        let w = PRESETS_WIDTH + COLORS_WIDTH + 6 * PAD;
        // Title, heading, one row for each color, the hint and buttons.
        let h = LINE + PAD + (Part::ALL.len() as u32 + 1) * LINE + 2 * LINE + 5 * PAD;
        let (w, h) = (w.min(self.size.0), h.min(self.size.1));
        Rect {
            x: self.size.0.saturating_sub(w + 2 * PAD),
            y: (self.size.1 - h) / 2,
            w,
            h,
        }
    }

    /// The top of the first row under the headings.
    fn rows_top(&self) -> u32 {
        self.frame().y + 2 * LINE + 2 * PAD
    }

    fn preset_row(&self, i: usize) -> Rect {
        Rect {
            x: self.frame().x + 2 * PAD,
            y: self.rows_top() + i as u32 * LINE,
            w: PRESETS_WIDTH,
            h: LINE,
        }
    }

    fn color_row(&self, i: usize) -> Rect {
        Rect {
            x: self.frame().x + PRESETS_WIDTH + 4 * PAD,
            y: self.rows_top() + i as u32 * LINE,
            w: COLORS_WIDTH,
            h: LINE,
        }
    }

    fn button(&self, b: Button) -> Rect {
        let f = self.frame();
        let width = |b: Button| text_width(b.label()) + 4 * PAD;
        let y = f.y + f.h - 2 * PAD - LINE;
        let x = match b {
            Button::Close => f.x + f.w - 2 * PAD - width(b),
            _ => {
                let before: u32 = BUTTONS
                    .iter()
                    .take_while(|&&o| o != b)
                    .map(|&o| width(o) + PAD)
                    .sum();
                f.x + 2 * PAD + before
            }
        };
        Rect {
            x,
            y,
            w: width(b),
            h: LINE,
        }
    }

    pub fn mouse_move(&mut self, at: (i32, i32)) {
        self.mouse = Some(at);
    }

    /// Chooses a color to set; digits typed for another are dropped.
    fn select(&mut self, i: usize) {
        self.selected = i.min(Part::ALL.len() - 1);
        self.typed = None;
    }

    pub fn click(&mut self, at: (i32, i32), palette: &mut Palette) -> Outcome {
        if let Some(&(_, preset)) = (0..PRESETS.len())
            .find(|&i| self.preset_row(i).contains(at))
            .map(|i| &PRESETS[i])
        {
            *palette = preset;
            self.typed = None;
            return Outcome::Stay;
        }
        if let Some(i) = (0..Part::ALL.len()).find(|&i| self.color_row(i).contains(at)) {
            self.select(i);
            return Outcome::Stay;
        }
        match BUTTONS.iter().find(|&&b| self.button(b).contains(at)) {
            Some(b) => b.outcome(),
            None => Outcome::Stay,
        }
    }

    /// Hex digits for the selected color, which changes once six are
    /// typed.
    pub fn text(&mut self, text: &str, palette: &mut Palette) {
        for c in text.chars().filter(char::is_ascii_hexdigit) {
            let typed = self.typed.get_or_insert_with(String::new);
            if typed.len() == 6 {
                typed.clear();
            }
            typed.push(c.to_ascii_uppercase());
            if let Some(color) = parse_color(typed) {
                palette[Part::ALL[self.selected]] = color;
            }
        }
    }

    /// Up and Down choose a color, Backspace takes back a digit, Enter
    /// ends typing, and Esc drops what is typed or closes the window.
    pub fn key(&mut self, key: Keycode) -> Outcome {
        match key {
            Keycode::Up => self.select(self.selected.saturating_sub(1)),
            Keycode::Down => self.select(self.selected + 1),
            Keycode::Return | Keycode::KpEnter => self.typed = None,
            Keycode::Backspace => {
                if let Some(typed) = &mut self.typed {
                    typed.pop();
                }
            }
            Keycode::Escape if self.typed.is_some() => self.typed = None,
            Keycode::Escape => return Outcome::Close,
            _ => {}
        }
        Outcome::Stay
    }

    pub fn draw(&self, fb: &mut Framebuffer, font: &Font, palette: &Palette) {
        let f = self.frame();
        window(fb, font, f, "Editor colors");
        let hovered = |r: Rect| self.mouse.is_some_and(|m| r.contains(m));
        let heading_y = self.rows_top() - LINE + 4;
        draw_text(fb, font, self.preset_row(0).x, heading_y, "Presets", TEXT);
        draw_text(fb, font, self.color_row(0).x, heading_y, "Colors", TEXT);

        for (i, (name, preset)) in PRESETS.iter().enumerate() {
            let r = self.preset_row(i);
            if hovered(r) {
                fill(fb, r, HIGHLIGHT);
            }
            let mark = if preset == palette { "(*)" } else { "( )" };
            draw_text(
                fb,
                font,
                r.x + PAD,
                r.y + 4,
                &format!("{mark} {name}"),
                TEXT,
            );
        }

        for (i, &part) in Part::ALL.iter().enumerate() {
            let r = self.color_row(i);
            if i == self.selected {
                fill(fb, r, HIGHLIGHT);
            } else if hovered(r) {
                fill(fb, r, BORDER);
            }
            let swatch = Rect {
                x: r.x + PAD,
                y: r.y + 3,
                w: SWATCH,
                h: LINE - 6,
            };
            fill(fb, swatch, palette[part]);
            outline(fb, swatch, BORDER);
            let label_x = swatch.x + SWATCH + PAD;
            draw_text(fb, font, label_x, r.y + 4, part.label(), TEXT);
            let value = match &self.typed {
                Some(typed) if i == self.selected => format!("#{typed:<6}").replace(' ', "_"),
                _ => format_color(palette[part]),
            };
            let x = r.x + r.w - PAD - text_width(&value);
            draw_text(fb, font, x, r.y + 4, &value, TEXT);
        }

        let hint_y = self.button(Button::Close).y - LINE;
        draw_text(
            fb,
            font,
            f.x + 2 * PAD,
            hint_y,
            "Type a hex value for the chosen color.",
            TEXT,
        );
        for b in BUTTONS {
            button(
                fb,
                font,
                self.button(b),
                b.label(),
                hovered(self.button(b)),
                true,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sb_runtime::palette::SMILEBASIC;

    fn centre(r: Rect) -> (i32, i32) {
        ((r.x + r.w / 2) as i32, (r.y + r.h / 2) as i32)
    }

    #[test]
    fn presets_and_typed_colors() {
        let mut w = ColorWindow::new((1280, 720));
        let mut p = SMILEBASIC;
        assert_eq!(w.click(centre(w.preset_row(1)), &mut p), Outcome::Stay);
        assert_eq!(p, PRESETS[1].1);

        w.click(centre(w.color_row(1)), &mut p);
        w.text("#12ab", &mut p);
        assert_eq!(p, PRESETS[1].1, "not yet six digits");
        w.key(Keycode::Backspace);
        w.text("CD", &mut p);
        assert_eq!(p, PRESETS[1].1);
        w.text("Z9", &mut p);
        assert_eq!(p[Part::Numeric], 0xFF12_ACD9);
        // Down moves on; Esc closes once nothing is being typed.
        w.key(Keycode::Down);
        w.text("00FF00", &mut p);
        assert_eq!(p[Part::String], 0xFF00_FF00);
        w.key(Keycode::Return);
        assert_eq!(w.key(Keycode::Escape), Outcome::Close);
    }

    #[test]
    fn fits_the_display_and_buttons() {
        let mut w = ColorWindow::new((1280, 720));
        let f = w.frame();
        assert!(f.x + f.w <= 1280 && f.y + f.h <= 720);
        let last = w.color_row(Part::ALL.len() - 1);
        assert!(last.y + last.h <= w.button(Button::Close).y - LINE);
        assert!(
            w.button(Button::Export).x + w.button(Button::Export).w < w.button(Button::Close).x
        );
        let mut p = SMILEBASIC;
        assert_eq!(
            w.click(centre(w.button(Button::Import)), &mut p),
            Outcome::Import
        );
        assert_eq!(
            w.click(centre(w.button(Button::Export)), &mut p),
            Outcome::Export
        );
    }
}
