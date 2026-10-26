//! Yes-or-no questions, such as before quitting with unsaved programs,
//! drawn over the screen like the menu bar.

use sb_runtime::Framebuffer;
use sb_runtime::confirm::{Confirm, unsaved_lines};
use sb_runtime::font::Font;
use sdl3::keyboard::Keycode;

use crate::ui::{BACKGROUND, CHAR, HIGHLIGHT, PAD, Rect, TEXT, draw_text, fill, text_width};

const LINE: u32 = 24;
const BORDER: u32 = 0xFF80_8080;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Answer {
    Accept,
    Cancel,
}

const ANSWERS: [Answer; 2] = [Answer::Accept, Answer::Cancel];

pub struct Dialog {
    title: String,
    lines: Vec<String>,
    /// Labels of the Accept and Cancel buttons.
    labels: [String; 2],
    /// Display size the dialog is centered in.
    size: (u32, u32),
    /// Index of the chosen button; Cancel at first, so Enter does not
    /// throw work away.
    focus: usize,
}

impl Dialog {
    pub fn new(title: &str, lines: Vec<String>, accept: &str, size: (u32, u32)) -> Dialog {
        Dialog {
            title: title.to_string(),
            lines,
            labels: [accept.to_string(), "Cancel".to_string()],
            size,
            focus: 1,
        }
    }

    /// Asks a command's question.
    pub fn ask(confirm: &Confirm, size: (u32, u32)) -> Dialog {
        Dialog::new(&confirm.title, confirm.lines.clone(), confirm.accept, size)
    }

    /// Asks before quitting with unsaved slots.
    pub fn quit(unsaved: &[usize], size: (u32, u32)) -> Dialog {
        let mut lines = vec!["Quit the emulator.".to_string(), String::new()];
        lines.extend(unsaved_lines(unsaved));
        Dialog::new("Quit", lines, "Quit", size)
    }

    fn button_width(&self) -> u32 {
        self.labels.iter().map(|l| text_width(l)).max().unwrap_or(0) + 4 * PAD
    }

    fn frame(&self) -> Rect {
        let text = self
            .lines
            .iter()
            .chain([&self.title])
            .map(|l| text_width(l))
            .max()
            .unwrap_or(0);
        let w = text.max(2 * self.button_width() + PAD) + 4 * PAD;
        // Title bar, body, buttons.
        let h = LINE + self.lines.len() as u32 * LINE + LINE + 6 * PAD;
        Rect {
            x: self.size.0.saturating_sub(w) / 2,
            y: self.size.1.saturating_sub(h) / 2,
            w,
            h,
        }
    }

    fn button(&self, i: usize) -> Rect {
        let f = self.frame();
        let w = self.button_width();
        Rect {
            x: f.x + f.w - 2 * PAD - (2 - i as u32) * w - (1 - i as u32) * PAD,
            y: f.y + f.h - 2 * PAD - LINE,
            w,
            h: LINE,
        }
    }

    pub fn mouse_move(&mut self, at: (i32, i32)) {
        if let Some(i) = (0..ANSWERS.len()).find(|&i| self.button(i).contains(at)) {
            self.focus = i;
        }
    }

    pub fn click(&mut self, at: (i32, i32)) -> Option<Answer> {
        (0..ANSWERS.len())
            .find(|&i| self.button(i).contains(at))
            .map(|i| ANSWERS[i])
    }

    /// Left, Right and Tab move between the buttons, Enter chooses and
    /// Esc cancels.
    pub fn key(&mut self, key: Keycode) -> Option<Answer> {
        match key {
            Keycode::Left | Keycode::Right | Keycode::Tab => {
                self.focus = 1 - self.focus;
                None
            }
            Keycode::Return | Keycode::KpEnter | Keycode::Space => Some(ANSWERS[self.focus]),
            Keycode::Escape => Some(Answer::Cancel),
            _ => None,
        }
    }

    pub fn draw(&self, fb: &mut Framebuffer, font: &Font) {
        let f = self.frame();
        fill(fb, f, BORDER);
        let inner = Rect {
            x: f.x + 2,
            y: f.y + 2,
            w: f.w - 4,
            h: f.h - 4,
        };
        fill(fb, inner, BACKGROUND | 0xFF00_0000);
        let bar = Rect { h: LINE, ..inner };
        fill(fb, bar, HIGHLIGHT);
        draw_text(fb, font, f.x + 2 * PAD, f.y + 6, &self.title, TEXT);
        for (i, line) in self.lines.iter().enumerate() {
            let y = f.y + LINE + 2 * PAD + i as u32 * LINE + (LINE - CHAR) / 2;
            draw_text(fb, font, f.x + 2 * PAD, y, line, TEXT);
        }
        for (i, label) in self.labels.iter().enumerate() {
            let r = self.button(i);
            fill(fb, r, if i == self.focus { HIGHLIGHT } else { BORDER });
            let x = r.x + (r.w - text_width(label)) / 2;
            draw_text(fb, font, x, r.y + 4, label, TEXT);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn centre(r: Rect) -> (i32, i32) {
        ((r.x + r.w / 2) as i32, (r.y + r.h / 2) as i32)
    }

    #[test]
    fn names_the_slots_and_defaults_to_cancel() {
        let mut d = Dialog::quit(&[0, 2], (1280, 720));
        assert_eq!(d.lines[2], "Slots 0, 2 have unsaved changes,");
        assert_eq!(d.key(Keycode::Return), Some(Answer::Cancel));
        assert_eq!(d.key(Keycode::Left), None);
        assert_eq!(d.key(Keycode::Return), Some(Answer::Accept));
        assert_eq!(d.key(Keycode::Escape), Some(Answer::Cancel));
    }

    #[test]
    fn buttons_answer_clicks() {
        let lines = vec!["Erase the program in slot 1.".to_string()];
        let mut d = Dialog::new("Erase programs", lines, "Erase", (1280, 720));
        let (quit, cancel) = (d.button(0), d.button(1));
        assert!(quit.x + quit.w < cancel.x, "Quit sits left of Cancel");
        assert!(cancel.x + cancel.w <= d.frame().x + d.frame().w);
        assert_eq!(d.click(centre(quit)), Some(Answer::Accept));
        assert_eq!(d.click(centre(cancel)), Some(Answer::Cancel));
        assert_eq!(d.click((0, 0)), None);
        d.mouse_move(centre(quit));
        assert_eq!(d.focus, 0);
    }
}
