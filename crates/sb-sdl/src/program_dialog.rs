//! The window of a program's DIALOG, drawn over the screen. Clicks, Enter
//! and typing answer it here; controller buttons and the timeout are the
//! runtime's.

use sb_runtime::Framebuffer;
use sb_runtime::dialog::{DialogAnswer, DialogKind, ProgramDialog, name_char};
use sb_runtime::font::Font;
use sdl3::keyboard::Keycode;

use crate::ui::{
    BACKGROUND, BORDER, CHAR, HIGHLIGHT, LINE, PAD, Rect, TEXT, char_width, draw_text, fill,
    text_width,
};

/// The narrowest the window gets, so that short messages still look like
/// a dialog.
const MIN_WIDTH: u32 = 320;
const NAME_BUTTONS: [&str; 2] = ["Cancel", "OK"];

pub struct ProgramView {
    dialog: ProgramDialog,
    /// The message, wrapped to fit the display.
    lines: Vec<String>,
    /// Display size the window is centered in.
    size: (u32, u32),
    /// Index of the chosen button: the right one at first, as A chooses.
    focus: usize,
    /// What has been typed, for a file name.
    name: String,
}

impl ProgramView {
    pub fn new(dialog: &ProgramDialog, size: (u32, u32)) -> ProgramView {
        let name = match &dialog.kind {
            DialogKind::Name { initial, .. } => initial.clone(),
            _ => String::new(),
        };
        // Room for the window's padding and a margin around it.
        let room = size.0.saturating_sub(8 * PAD).max(CHAR);
        let mut view = ProgramView {
            dialog: dialog.clone(),
            lines: dialog.lines.iter().flat_map(|l| wrap(l, room)).collect(),
            size,
            focus: 0,
            name,
        };
        view.focus = view.labels().len().saturating_sub(1);
        view
    }

    /// Whether this shows `dialog`, rather than one before it.
    pub fn shows(&self, dialog: &ProgramDialog) -> bool {
        self.dialog == *dialog
    }

    fn labels(&self) -> &[&'static str] {
        match &self.dialog.kind {
            DialogKind::Choice(labels) => labels,
            DialogKind::Buttons(_) => &[],
            DialogKind::Name { .. } => &NAME_BUTTONS,
        }
    }

    /// The lines of the body: the message, or the name being typed.
    fn body_lines(&self) -> usize {
        match self.dialog.kind {
            DialogKind::Name { .. } => 1,
            _ => self.lines.len(),
        }
    }

    fn button_width(&self) -> u32 {
        self.labels()
            .iter()
            .map(|l| text_width(l))
            .max()
            .unwrap_or(0)
            + 4 * PAD
    }

    fn buttons_width(&self) -> u32 {
        let n = self.labels().len() as u32;
        (n * self.button_width() + n.saturating_sub(1) * PAD).max(1)
    }

    fn frame(&self) -> Rect {
        let field = match self.dialog.kind {
            DialogKind::Name { max, .. } => max as u32 * CHAR + 2 * PAD,
            _ => 0,
        };
        let text = (self.lines.iter())
            .chain([&self.dialog.caption])
            .map(|l| text_width(l))
            .max()
            .unwrap_or(0);
        let w = (text.max(field).max(self.buttons_width()) + 4 * PAD).max(MIN_WIDTH);
        let buttons = if self.labels().is_empty() { 0 } else { LINE };
        // Title bar, body, buttons.
        let h = LINE + self.body_lines() as u32 * LINE + buttons + 6 * PAD;
        Rect {
            x: self.size.0.saturating_sub(w) / 2,
            y: self.size.1.saturating_sub(h) / 2,
            w,
            h,
        }
    }

    /// Buttons sit along the bottom, right-aligned.
    fn button(&self, i: usize) -> Rect {
        let f = self.frame();
        let w = self.button_width();
        let n = self.labels().len() as u32;
        let right = f.x + f.w - 2 * PAD;
        let from_right = n - i as u32;
        Rect {
            x: right - from_right * w - (from_right - 1) * PAD,
            y: f.y + f.h - 2 * PAD - LINE,
            w,
            h: LINE,
        }
    }

    fn choose(&self, i: usize) -> DialogAnswer {
        match self.dialog.kind {
            DialogKind::Name { .. } if i == 1 => DialogAnswer::Name(Some(self.name.clone())),
            DialogKind::Name { .. } => DialogAnswer::Name(None),
            _ => DialogAnswer::Choice(i),
        }
    }

    /// Whether a point is on the window, which takes clicks there.
    pub fn contains(&self, at: (i32, i32)) -> bool {
        self.frame().contains(at)
    }

    pub fn mouse_move(&mut self, at: (i32, i32)) {
        if let Some(i) = (0..self.labels().len()).find(|&i| self.button(i).contains(at)) {
            self.focus = i;
        }
    }

    pub fn click(&mut self, at: (i32, i32)) -> Option<DialogAnswer> {
        (0..self.labels().len())
            .find(|&i| self.button(i).contains(at))
            .map(|i| self.choose(i))
    }

    /// Left, Right and Tab move between the buttons, Enter chooses, Esc
    /// chooses the left of two as B does, and Backspace deletes from a
    /// name. Returns `None` for keys it leaves
    /// alone, and `Some(None)` for keys it took without an answer.
    pub fn key(&mut self, key: Keycode) -> Option<Option<DialogAnswer>> {
        let n = self.labels().len();
        match key {
            Keycode::Left | Keycode::Right | Keycode::Tab if n > 1 => {
                self.focus = (self.focus + 1) % n;
                Some(None)
            }
            Keycode::Return | Keycode::KpEnter if n > 0 => Some(Some(self.choose(self.focus))),
            Keycode::Escape if n == 2 => Some(Some(self.choose(0))),
            Keycode::Backspace if matches!(self.dialog.kind, DialogKind::Name { .. }) => {
                self.name.pop();
                Some(None)
            }
            _ => None,
        }
    }

    /// Typed text, for a name; whether it was taken.
    pub fn text(&mut self, text: &str) -> bool {
        let DialogKind::Name { max, .. } = self.dialog.kind else {
            return false;
        };
        for c in text.chars().filter(|&c| name_char(c)) {
            if self.name.len() < max {
                self.name.push(c.to_ascii_uppercase());
            }
        }
        true
    }

    pub fn draw(&self, fb: &mut Framebuffer, font: &Font, blink: bool) {
        let f = self.frame();
        fill(fb, f, BORDER);
        let inner = Rect {
            x: f.x + 2,
            y: f.y + 2,
            w: f.w - 4,
            h: f.h - 4,
        };
        fill(fb, inner, BACKGROUND | 0xFF00_0000);
        fill(fb, Rect { h: LINE, ..inner }, HIGHLIGHT);
        draw_text(fb, font, f.x + 2 * PAD, f.y + 6, &self.dialog.caption, TEXT);
        let line_y = |i: u32| f.y + LINE + 2 * PAD + i * LINE;
        match self.dialog.kind {
            DialogKind::Name { max, .. } => {
                let field = Rect {
                    x: f.x + 2 * PAD,
                    y: line_y(0),
                    w: max as u32 * CHAR + 2 * PAD,
                    h: LINE,
                };
                fill(fb, field, BORDER);
                let shown = if blink {
                    format!("{}_", self.name)
                } else {
                    self.name.clone()
                };
                draw_text(
                    fb,
                    font,
                    field.x + PAD,
                    field.y + (LINE - CHAR) / 2,
                    &shown,
                    TEXT,
                );
            }
            _ => {
                for (i, line) in self.lines.iter().enumerate() {
                    let y = line_y(i as u32) + (LINE - CHAR) / 2;
                    draw_text(fb, font, f.x + 2 * PAD, y, line, TEXT);
                }
            }
        }
        for (i, label) in self.labels().iter().enumerate() {
            let r = self.button(i);
            fill(fb, r, if i == self.focus { HIGHLIGHT } else { BORDER });
            let x = r.x + (r.w - text_width(label)) / 2;
            draw_text(fb, font, x, r.y + 4, label, TEXT);
        }
    }
}

/// Splits a line into pieces at most `max` pixels wide as [`draw_text`]
/// draws them, after a space where there is one, as Japanese text has
/// none.
pub(crate) fn wrap(line: &str, max: u32) -> Vec<String> {
    let mut pieces = Vec::new();
    let mut rest: Vec<char> = line.chars().collect();
    loop {
        // How many characters fit, at least one.
        let mut used = 0;
        let fit = rest.iter().take_while(|&&c| {
            used += char_width(c);
            used <= max
        });
        let fits = fit.count().max(1);
        if rest.len() <= fits {
            break;
        }
        let cut = match rest[..=fits].iter().rposition(|&c| c == ' ') {
            Some(space) if space > 0 => space,
            _ => fits,
        };
        pieces.push(rest[..cut].iter().collect());
        let skip = usize::from(rest.get(cut) == Some(&' '));
        rest.drain(..cut + skip);
    }
    pieces.push(rest.into_iter().collect());
    pieces
}

#[cfg(test)]
mod tests {
    use super::*;

    fn centre(r: Rect) -> (i32, i32) {
        ((r.x + r.w / 2) as i32, (r.y + r.h / 2) as i32)
    }

    fn dialog(kind: DialogKind) -> ProgramDialog {
        ProgramDialog {
            caption: "DIALOG".to_string(),
            lines: vec!["Hello".to_string()],
            kind,
        }
    }

    #[test]
    fn esc_is_the_left_of_two_buttons_and_cancels_a_name() {
        let key = |kind| ProgramView::new(&dialog(kind), (1280, 720)).key(Keycode::Escape);
        let two = key(DialogKind::Choice(vec!["No", "Yes"]));
        assert_eq!(two, Some(Some(DialogAnswer::Choice(0))));
        assert_eq!(key(DialogKind::Choice(vec!["OK"])), None);
        let name = DialogKind::Name {
            initial: "A".to_string(),
            max: 8,
        };
        assert_eq!(key(name), Some(Some(DialogAnswer::Name(None))));
    }

    #[test]
    fn long_lines_wrap() {
        // ASCII is half as wide as other characters.
        let half = CHAR / 2;
        assert_eq!(wrap("AB CD EF", 5 * half), ["AB CD", "EF"]);
        assert_eq!(wrap("ABCDEFG", 3 * half), ["ABC", "DEF", "G"]);
        assert_eq!(wrap("", 3 * half), [""]);
        assert_eq!(wrap("あいAう", 3 * CHAR), ["あいA", "う"]);
        let mut d = dialog(DialogKind::Choice(vec!["OK"]));
        d.lines = vec!["W ".repeat(200)];
        let v = ProgramView::new(&d, (1280, 720));
        assert!(v.lines.len() > 1);
        assert!(v.frame().w <= 1280);
    }

    #[test]
    fn choices_answer_by_index() {
        let mut v = ProgramView::new(&dialog(DialogKind::Choice(vec!["No", "Yes"])), (1280, 720));
        let (no, yes) = (v.button(0), v.button(1));
        assert!(no.x + no.w < yes.x);
        assert_eq!(v.click(centre(no)), Some(DialogAnswer::Choice(0)));
        assert_eq!(
            v.key(Keycode::Return),
            Some(Some(DialogAnswer::Choice(1))),
            "the right one is chosen at first"
        );
        assert_eq!(v.key(Keycode::Z), None);
        let v = ProgramView::new(&dialog(DialogKind::Buttons(1)), (1280, 720));
        assert!(v.labels().is_empty());
    }

    #[test]
    fn names_take_file_name_characters() {
        let kind = DialogKind::Name {
            initial: "AB".to_string(),
            max: 4,
        };
        let mut v = ProgramView::new(&dialog(kind), (1280, 720));
        assert!(v.text("c d/ef"));
        assert_eq!(v.name, "ABCD");
        v.key(Keycode::Backspace);
        assert_eq!(
            v.key(Keycode::Return),
            Some(Some(DialogAnswer::Name(Some("ABC".to_string()))))
        );
        assert_eq!(v.click(centre(v.button(0))), Some(DialogAnswer::Name(None)));
    }
}
