//! Yes-or-no questions, such as before quitting with unsaved programs,
//! drawn over the screen like the menu bar.

use sb_runtime::Framebuffer;
use sb_runtime::confirm::{Confirm, unsaved_lines};
use sb_runtime::font::Font;
use sdl3::gamepad::Button;
use sdl3::keyboard::Keycode;

use crate::program_dialog::{
    button_rect, centered, draw_buttons, draw_lines, draw_window, line_y, moved_focus, wrap_lines,
};
use crate::ui::{BORDER, CHAR, LINE, PAD, Rect, TEXT, draw_text, fill, text_width};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Answer {
    Accept,
    Cancel,
}

const ANSWERS: [Answer; 2] = [Answer::Accept, Answer::Cancel];

pub struct Dialog {
    title: String,
    given_lines: Vec<String>,
    /// `given_lines` wrapped to fit the display.
    shown_lines: Vec<String>,
    /// Labels of the Accept and Cancel buttons, or of OK alone.
    labels: Vec<String>,
    /// Display size the dialog is centered in.
    size: (u32, u32),
    /// Index of the chosen button. It is Cancel at first, so that Enter
    /// does not discard work, except on questions that say otherwise.
    focus: usize,
    /// The typed text, for a question with a field to type in.
    field: Option<String>,
    /// The button under the mouse, which a click chooses.
    hover: Option<usize>,
}

const FIELD_WIDTH_CHARS: usize = 40;

/// The height of a dialog outside its body: the title bar, the buttons and
/// the padding.
const CHROME_HEIGHT: u32 = 2 * LINE + 6 * PAD;

/// `lines` wrapped to fit a display of `size`, with room for the window
/// padding and a margin around it, and cut to the lines that still fit
/// when `more` lines are added.
fn wrapped(lines: &[String], (w, h): (u32, u32), more: u32) -> Vec<String> {
    let room = w.saturating_sub(8 * PAD).max(CHAR);
    let body = h.saturating_sub(CHROME_HEIGHT) / LINE;
    wrap_lines(lines, room, body.saturating_sub(more) as usize)
}

impl Dialog {
    pub fn new(title: &str, lines: Vec<String>, accept: &str, size: (u32, u32)) -> Dialog {
        Dialog {
            title: title.to_string(),
            shown_lines: wrapped(&lines, size, 0),
            given_lines: lines,
            labels: vec![accept.to_string(), "Cancel".to_string()],
            size,
            focus: 1,
            field: None,
            hover: None,
        }
    }

    /// Asks for a line of text, starting with `text`; Enter accepts it.
    pub fn prompt(title: &str, lines: Vec<String>, text: &str, size: (u32, u32)) -> Dialog {
        let mut d = Dialog {
            focus: 0,
            field: Some(text.to_string()),
            ..Dialog::new(title, lines, "OK", size)
        };
        d.shown_lines = wrapped(&d.given_lines, size, 1);
        d
    }

    /// Returns the text typed in the field.
    pub fn field(&self) -> &str {
        self.field.as_deref().unwrap_or("")
    }

    /// Typed text, for the field.
    pub fn text(&mut self, text: &str) {
        if let Some(field) = &mut self.field {
            field.push_str(text);
        }
    }

    /// The number of lines in the body, with a line for the field.
    fn body_lines(&self) -> u32 {
        self.shown_lines.len() as u32 + u32::from(self.field.is_some())
    }

    /// Shows a message with OK alone, which accepts.
    pub fn notice(title: &str, lines: Vec<String>, size: (u32, u32)) -> Dialog {
        Dialog {
            title: title.to_string(),
            shown_lines: wrapped(&lines, size, 0),
            given_lines: lines,
            labels: vec!["OK".to_string()],
            size,
            focus: 0,
            field: None,
            hover: None,
        }
    }

    /// Asks a command's question, starting on the button the command picks.
    pub fn ask(confirm: &Confirm, size: (u32, u32)) -> Dialog {
        Dialog {
            focus: usize::from(!confirm.accept_first),
            ..Dialog::new(&confirm.title, confirm.lines.clone(), confirm.accept, size)
        }
    }

    /// Asks before quitting with unsaved slots.
    pub fn quit(unsaved: &[usize], size: (u32, u32)) -> Dialog {
        let mut lines = vec!["Quit the emulator.".to_string(), String::new()];
        lines.extend(unsaved_lines(unsaved));
        Dialog::new("Quit", lines, "Quit", size)
    }

    /// Centers the dialog in a display of another size.
    pub fn set_size(&mut self, size: (u32, u32)) {
        if size != self.size {
            self.size = size;
            let more = u32::from(self.field.is_some());
            self.shown_lines = wrapped(&self.given_lines, size, more);
        }
    }

    fn button_width(&self) -> u32 {
        self.labels.iter().map(|l| text_width(l)).max().unwrap_or(0) + 4 * PAD
    }

    fn frame(&self) -> Rect {
        let text = self
            .shown_lines
            .iter()
            .chain([&self.title])
            .map(|l| text_width(l))
            .chain(
                self.field
                    .as_ref()
                    .map(|_| FIELD_WIDTH_CHARS as u32 * CHAR + 2 * PAD),
            )
            .max()
            .unwrap_or(0);
        // No wider than the display: a long title is cut, and so is a field.
        let w = (text.max(2 * self.button_width() + PAD) + 4 * PAD).min(self.size.0);
        let h = self.body_lines() * LINE + CHROME_HEIGHT;
        centered(self.size, w, h)
    }

    fn button(&self, i: usize) -> Rect {
        button_rect(self.frame(), self.button_width(), self.labels.len(), i)
    }

    /// The mouse shows which button a click chooses. It does not move the
    /// focus, which Enter and A choose.
    pub fn mouse_move(&mut self, at: (i32, i32)) {
        self.hover = (0..self.labels.len()).find(|&i| self.button(i).contains(at));
    }

    pub fn click(&mut self, at: (i32, i32)) -> Option<Answer> {
        (0..self.labels.len())
            .find(|&i| self.button(i).contains(at))
            .map(|i| ANSWERS[i])
    }

    /// Left and Right move between the buttons and stop at the ends, Tab
    /// wraps around, Enter chooses and Esc cancels, the same as in a
    /// program's DIALOG. With a field, Backspace deletes from it and Enter
    /// accepts.
    pub fn key(&mut self, key: Keycode) -> Option<Answer> {
        if let Some(field) = &mut self.field {
            match key {
                Keycode::Backspace => {
                    field.pop();
                    return None;
                }
                Keycode::Return | Keycode::KpEnter => return Some(Answer::Accept),
                Keycode::Space => return None,
                _ => {}
            }
        }
        if let Some(focus) = moved_focus(self.focus, self.labels.len(), key) {
            self.focus = focus;
            return None;
        }
        match key {
            Keycode::Return | Keycode::KpEnter | Keycode::Space => Some(ANSWERS[self.focus]),
            Keycode::Escape => Some(Answer::Cancel),
            _ => None,
        }
    }

    /// A controller: left and right on the pad, or the left stick, which
    /// the loop passes on as the pad, move between the buttons and stop at
    /// the ends. A (the right face button) chooses and B (the bottom one)
    /// cancels, following the button layout of SmileBASIC 4.
    pub fn pad(&mut self, button: Button) -> Option<Answer> {
        match button {
            Button::DPadLeft => self.key(Keycode::Left),
            Button::DPadRight => self.key(Keycode::Right),
            Button::East if self.field.is_some() => Some(Answer::Accept),
            Button::East => Some(ANSWERS[self.focus]),
            Button::South => Some(Answer::Cancel),
            _ => None,
        }
    }

    pub fn draw(&self, fb: &mut Framebuffer, font: &Font) {
        let f = self.frame();
        draw_window(fb, font, f, &self.title);
        draw_lines(fb, font, f, &self.shown_lines);
        if let Some(text) = &self.field {
            let field = Rect {
                x: f.x + 2 * PAD,
                y: line_y(f, self.shown_lines.len() as u32),
                w: f.w - 4 * PAD,
                h: LINE,
            };
            fill(fb, field, BORDER);
            // The end of a long text shows, with the cursor.
            let room = (field.w - 2 * PAD) / CHAR - 1;
            let chars: Vec<char> = text.chars().collect();
            let shown: String = chars[chars.len().saturating_sub(room as usize)..]
                .iter()
                .collect();
            let y = field.y + (LINE - CHAR) / 2;
            draw_text(fb, font, field.x + PAD, y, &format!("{shown}_"), TEXT);
        }
        let buttons = (self.labels.iter().enumerate()).map(|(i, l)| (l.as_str(), self.button(i)));
        draw_buttons(fb, font, buttons, self.focus, self.hover);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_the_slots_and_defaults_to_cancel() {
        let mut d = Dialog::quit(&[0, 2], (1280, 720));
        assert_eq!(d.shown_lines[2], "Slots 0, 2 have unsaved changes,");
        assert_eq!(d.key(Keycode::Return), Some(Answer::Cancel));
        assert_eq!(d.key(Keycode::Left), None);
        assert_eq!(d.key(Keycode::Return), Some(Answer::Accept));
        assert_eq!(d.key(Keycode::Escape), Some(Answer::Cancel));
    }

    #[test]
    fn the_focus_stops_at_the_ends_and_tab_goes_round() {
        let mut d = Dialog::quit(&[0], (1280, 720));
        // Cancel, the right one, has the focus at first.
        d.key(Keycode::Right);
        assert_eq!(d.focus, 1);
        d.pad(Button::DPadRight);
        assert_eq!(d.focus, 1);
        d.pad(Button::DPadLeft);
        assert_eq!(d.focus, 0);
        d.key(Keycode::Left);
        assert_eq!(d.focus, 0);
        d.key(Keycode::Tab);
        d.key(Keycode::Tab);
        assert_eq!(d.focus, 0);
    }

    #[test]
    fn a_controller_answers() {
        let mut d = Dialog::quit(&[0], (1280, 720));
        assert_eq!(d.pad(Button::East), Some(Answer::Cancel));
        assert_eq!(d.pad(Button::DPadLeft), None);
        assert_eq!(d.pad(Button::East), Some(Answer::Accept));
        assert_eq!(d.pad(Button::South), Some(Answer::Cancel));
    }

    fn save_question(accept_first: bool) -> Confirm {
        Confirm {
            title: "Save file".to_string(),
            lines: vec!["Save this file.".to_string()],
            accept: "Save",
            accept_first,
        }
    }

    #[test]
    fn a_saves_on_a_file_question() {
        let mut d = Dialog::ask(&save_question(true), (1280, 720));
        assert_eq!(d.pad(Button::East), Some(Answer::Accept));
        assert_eq!(d.key(Keycode::Return), Some(Answer::Accept));
        assert_eq!(d.pad(Button::South), Some(Answer::Cancel));
        assert_eq!(d.key(Keycode::Escape), Some(Answer::Cancel));
        // An offscreen mouse leaves the choice alone.
        d.mouse_move((-1, -1));
        assert_eq!(d.pad(Button::East), Some(Answer::Accept));
        let mut d = Dialog::ask(&save_question(false), (1280, 720));
        assert_eq!(d.pad(Button::East), Some(Answer::Cancel));
    }

    #[test]
    fn a_prompt_takes_typing() {
        let mut d = Dialog::prompt("Condition", Vec::new(), "A", (1280, 720));
        d.text("> 1");
        assert_eq!(d.key(Keycode::Space), None);
        assert_eq!(d.key(Keycode::Backspace), None);
        d.text("2");
        assert_eq!(d.field(), "A> 2");
        assert_eq!(d.key(Keycode::Return), Some(Answer::Accept));
        assert_eq!(d.key(Keycode::Escape), Some(Answer::Cancel));
    }

    #[test]
    fn long_lines_wrap_to_fit_a_small_display() {
        let path = "Saved PRG0 to recovered/2026-11-15/PRG0.txt in the data folder";
        let lines = vec![
            "The emulator failed last time.".to_string(),
            path.to_string(),
        ];
        let mut d = Dialog::notice("Recovered programs", lines, (1280, 720));
        assert_eq!(d.shown_lines.len(), 2, "room enough on the desktop");
        d.set_size((320, 240));
        assert!(d.shown_lines.len() > 2);
        let f = d.frame();
        assert!(f.x + f.w <= 320, "{f:?}");
        d.set_size((1280, 720));
        assert_eq!(d.shown_lines.len(), 2);
    }

    /// The Simple UI can be 320x240: a dialog is cut to fit it,
    /// regardless of how much text it has.
    #[test]
    fn a_dialog_fits_a_320x240_display() {
        let size = (320, 240);
        let mut lines = vec!["The emulator failed last time.".to_string(), String::new()];
        for slot in 0..4 {
            lines.push(format!(
                "Saved PRG{slot} to recovered/2026-11-15/PRG{slot}.txt"
            ));
        }
        let quit = Dialog::quit(&[0, 1, 2, 3], size);
        let long_title = "A title much too long for a display this small";
        let dialogs = [
            Dialog::notice("Recovered programs", lines, size),
            quit,
            Dialog::notice(long_title, vec!["Short".to_string()], size),
            Dialog::prompt("Condition", vec!["A".repeat(100)], "B", size),
        ];
        for d in dialogs {
            let f = d.frame();
            assert!(f.x + f.w <= size.0 && f.y + f.h <= size.1, "{f:?}");
            for i in 0..d.labels.len() {
                let b = d.button(i);
                assert!(b.x >= f.x && b.y + b.h <= f.y + f.h);
            }
            let room = f.w - 4 * PAD;
            assert!(
                d.shown_lines.iter().all(|l| text_width(l) <= room),
                "{:?}",
                d.shown_lines
            );
            let mut fb = Framebuffer::new(size.0, size.1);
            d.draw(&mut fb, &Font::blank());
        }
        let mut lines = vec![String::new(); 2];
        lines.extend((0..20).map(|i| format!("Line {i}")));
        let d = Dialog::notice("Notice", lines, size);
        assert!(
            d.shown_lines.last().unwrap().ends_with('~'),
            "{:?}",
            d.shown_lines
        );
    }

    #[test]
    fn buttons_answer_clicks() {
        let lines = vec!["Erase the program in slot 1.".to_string()];
        let mut d = Dialog::new("Erase programs", lines, "Erase", (1280, 720));
        let (quit, cancel) = (d.button(0), d.button(1));
        assert!(quit.x + quit.w < cancel.x, "Quit sits left of Cancel");
        assert!(cancel.x + cancel.w <= d.frame().x + d.frame().w);
        assert_eq!(d.click(quit.center()), Some(Answer::Accept));
        assert_eq!(d.click(cancel.center()), Some(Answer::Cancel));
        assert_eq!(d.click((0, 0)), None);
        d.mouse_move(quit.center());
        assert_eq!((d.hover, d.focus), (Some(0), 1), "hovered, not focused");
    }
}
