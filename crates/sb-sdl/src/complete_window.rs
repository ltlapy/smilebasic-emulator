//! The window behind Options > Autocomplete > Advanced: what completion's
//! list offers and when it shows.

use sb_runtime::Framebuffer;
use sb_runtime::font::Font;
use sb_runtime::settings::{COMPLETE_AFTER, Settings};
use sdl3::keyboard::Keycode;

use crate::ui::{CHAR, DISABLED, HIGHLIGHT, LINE, PAD, Rect, TEXT, draw_text, fill, window};

const HINT: &str = "Up/Down: choose  Space: set  Esc: close";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Row {
    After(u32),
    Commands,
    Names,
    Enter,
    Hints,
}

fn rows() -> Vec<Row> {
    let mut rows: Vec<Row> = COMPLETE_AFTER.map(Row::After).collect();
    rows.extend([Row::Commands, Row::Names, Row::Enter, Row::Hints]);
    rows
}

fn label(row: Row, settings: &Settings) -> String {
    let check = |on: bool| if on { "[x]" } else { "[ ]" };
    match row {
        Row::After(n) => {
            let mark = if settings.complete_after == n {
                "(*)"
            } else {
                "( )"
            };
            let letters = if n == 1 { "letter" } else { "letters" };
            format!("{mark} Show after {n} {letters}")
        }
        Row::Commands => format!("{} SmileBASIC's words", check(settings.complete_commands)),
        Row::Names => format!("{} Names in the program", check(settings.complete_names)),
        Row::Enter => format!(
            "{} Enter takes a picked word",
            check(settings.complete_enter)
        ),
        Row::Hints => format!(
            "{} Show how commands are used",
            check(settings.complete_hints)
        ),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Stay,
    Close,
}

pub struct CompleteWindow {
    size: (u32, u32),
    chosen: usize,
}

impl CompleteWindow {
    pub fn new(size: (u32, u32)) -> CompleteWindow {
        CompleteWindow { size, chosen: 0 }
    }

    fn frame(&self) -> Rect {
        let w = 32 * CHAR + 4 * PAD;
        // Title bar, rows, a gap, the hint.
        let h = LINE + rows().len() as u32 * LINE + LINE + 6 * PAD;
        Rect {
            x: self.size.0.saturating_sub(w) / 2,
            y: self.size.1.saturating_sub(h) / 2,
            w,
            h,
        }
    }

    fn row_rect(&self, i: usize) -> Rect {
        let f = self.frame();
        Rect {
            x: f.x + PAD,
            y: f.y + LINE + 2 * PAD + i as u32 * LINE,
            w: f.w - 2 * PAD,
            h: LINE,
        }
    }

    fn set(row: Row, settings: &mut Settings) {
        match row {
            Row::After(n) => settings.complete_after = n,
            Row::Commands => settings.complete_commands ^= true,
            Row::Names => settings.complete_names ^= true,
            Row::Enter => settings.complete_enter ^= true,
            Row::Hints => settings.complete_hints ^= true,
        }
    }

    pub fn key(&mut self, key: Keycode, settings: &mut Settings) -> Outcome {
        let n = rows().len();
        match key {
            Keycode::Escape => return Outcome::Close,
            Keycode::Up => self.chosen = (self.chosen + n - 1) % n,
            Keycode::Down => self.chosen = (self.chosen + 1) % n,
            Keycode::Space | Keycode::Return | Keycode::KpEnter => {
                Self::set(rows()[self.chosen], settings)
            }
            _ => {}
        }
        Outcome::Stay
    }

    pub fn mouse_move(&mut self, at: (i32, i32)) {
        if let Some(i) = (0..rows().len()).find(|&i| self.row_rect(i).contains(at)) {
            self.chosen = i;
        }
    }

    /// A click on a row sets it; one outside the window closes it.
    pub fn click(&mut self, at: (i32, i32), settings: &mut Settings) -> Outcome {
        if !self.frame().contains(at) {
            return Outcome::Close;
        }
        if let Some(i) = (0..rows().len()).find(|&i| self.row_rect(i).contains(at)) {
            self.chosen = i;
            Self::set(rows()[i], settings);
        }
        Outcome::Stay
    }

    pub fn draw(&self, fb: &mut Framebuffer, font: &Font, settings: &Settings) {
        let f = self.frame();
        window(fb, font, f, "Autocomplete");
        for (i, &row) in rows().iter().enumerate() {
            let r = self.row_rect(i);
            if i == self.chosen {
                fill(fb, r, HIGHLIGHT);
            }
            let y = r.y + (LINE - CHAR) / 2;
            draw_text(fb, font, r.x + PAD, y, &label(row, settings), TEXT);
        }
        draw_text(
            fb,
            font,
            f.x + 2 * PAD,
            f.y + f.h - LINE - PAD,
            HINT,
            DISABLED,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_and_clicks_change_the_settings() {
        let mut s = Settings::default();
        let mut w = CompleteWindow::new((1280, 720));
        w.key(Keycode::Down, &mut s);
        w.key(Keycode::Space, &mut s);
        assert_eq!(s.complete_after, 2);
        // Up from the top goes round to the last rows.
        w.key(Keycode::Up, &mut s);
        w.key(Keycode::Up, &mut s);
        w.key(Keycode::Up, &mut s);
        w.key(Keycode::Return, &mut s);
        assert!(!s.complete_enter);
        let names = w.row_rect(5);
        let at = ((names.x + 4) as i32, (names.y + 4) as i32);
        assert_eq!(w.click(at, &mut s), Outcome::Stay);
        assert!(s.complete_names);
        assert_eq!(label(Row::Names, &s), "[x] Names in the program");
        assert_eq!(w.click((0, 0), &mut s), Outcome::Close);
        assert_eq!(w.key(Keycode::Escape, &mut s), Outcome::Close);
    }
}
