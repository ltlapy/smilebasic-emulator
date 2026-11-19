//! The window of a program's DIALOG, drawn over the screen. Clicks, Enter
//! and typing answer it here; controller buttons and the timeout are the
//! runtime's, and so is the focus of a choice, which the keys here move
//! too.

use sb_runtime::Framebuffer;
use sb_runtime::dialog::{DialogAnswer, DialogKind, ProgramDialog, name_char};
use sb_runtime::font::Font;
use sdl3::keyboard::Keycode;

use crate::ui::{
    BACKGROUND, BORDER, CHAR, HIGHLIGHT, LINE, PAD, Rect, TEXT, char_width, draw_text, fill,
    focus_face, text_width,
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
    /// For a choice, the runtime's, which [`ProgramView::follow`] brings.
    focus: usize,
    /// What has been typed, for a file name.
    name: String,
    /// The button under the mouse, which a click chooses.
    hover: Option<usize>,
}

impl ProgramView {
    pub fn new(dialog: &ProgramDialog, size: (u32, u32)) -> ProgramView {
        let name = match &dialog.kind {
            DialogKind::Name { initial, .. } => initial.clone(),
            _ => String::new(),
        };
        let mut view = ProgramView {
            dialog: dialog.clone(),
            lines: Vec::new(),
            size: (0, 0),
            focus: 0,
            name,
            hover: None,
        };
        view.set_size(size);
        view.focus = view.labels().len().saturating_sub(1);
        view
    }

    /// Centers the window on a display of another size, as the desktop's
    /// UI is when the window's height changes, keeping what was chosen and
    /// typed.
    pub fn set_size(&mut self, size: (u32, u32)) {
        if size == self.size {
            return;
        }
        // Room for the window's padding and a margin around it, and for
        // the lines below the title bar and above the buttons.
        let room = size.0.saturating_sub(8 * PAD).max(CHAR);
        let body = size.1.saturating_sub(2 * LINE + 6 * PAD) / LINE;
        self.lines = wrap_lines(&self.dialog.lines, room, body as usize);
        self.size = size;
    }

    /// Takes the focus of a choice from the runtime, which the controller
    /// moves; other dialogs keep their own.
    pub fn follow(&mut self, focus: Option<usize>) {
        if let Some(i) = focus {
            self.focus = i.min(self.labels().len().saturating_sub(1));
        }
    }

    /// The button with the focus, for the runtime to take.
    pub fn focus(&self) -> usize {
        self.focus
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
        let label = self.labels().iter().map(|l| text_width(l)).max();
        label.unwrap_or(0) + 4 * PAD
    }

    fn buttons_width(&self) -> u32 {
        let n = self.labels().len() as u32;
        (n * self.button_width() + n.saturating_sub(1) * PAD).max(1)
    }

    /// The width of a name's field: the most it takes, or the window's
    /// inside on a small display, where a name of full length still fits
    /// since names are half-width.
    fn field_width(&self) -> u32 {
        match self.dialog.kind {
            DialogKind::Name { max, .. } => {
                (max as u32 * CHAR + 2 * PAD).min(self.size.0.saturating_sub(4 * PAD))
            }
            _ => 0,
        }
    }

    fn frame(&self) -> Rect {
        let field = self.field_width();
        let text = (self.lines.iter())
            .chain([&self.dialog.caption])
            .map(|l| text_width(l))
            .max()
            .unwrap_or(0);
        // No wider than the display: a long caption is cut.
        let w = (text.max(field).max(self.buttons_width()) + 4 * PAD)
            .max(MIN_WIDTH)
            .min(self.size.0);
        let buttons = if self.labels().is_empty() { 0 } else { LINE };
        // Title bar, body, buttons.
        let h = LINE + self.body_lines() as u32 * LINE + buttons + 6 * PAD;
        centered(self.size, w, h)
    }

    /// Buttons sit along the bottom, right-aligned.
    fn button(&self, i: usize) -> Rect {
        button_rect(self.frame(), self.button_width(), self.labels().len(), i)
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

    /// The mouse shows which button a click chooses; it leaves the focus,
    /// which A and Enter choose, where it is.
    pub fn mouse_move(&mut self, at: (i32, i32)) {
        self.hover = (0..self.labels().len()).find(|&i| self.button(i).contains(at));
    }

    pub fn click(&mut self, at: (i32, i32)) -> Option<DialogAnswer> {
        (0..self.labels().len())
            .find(|&i| self.button(i).contains(at))
            .map(|i| self.choose(i))
    }

    /// Left, Right and Tab move between the buttons, Tab going round from
    /// the last to the first, Enter chooses, Esc
    /// chooses the left of two as B does, and Backspace deletes from a
    /// name. Returns `None` for keys it leaves
    /// alone, and `Some(None)` for keys it took without an answer.
    pub fn key(&mut self, key: Keycode) -> Option<Option<DialogAnswer>> {
        let n = self.labels().len();
        if let Some(focus) = moved_focus(self.focus, n, key) {
            self.focus = focus;
            return Some(None);
        }
        match key {
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
        draw_window(fb, font, f, &self.dialog.caption);
        match self.dialog.kind {
            DialogKind::Name { .. } => {
                let field = Rect {
                    x: f.x + 2 * PAD,
                    y: line_y(f, 0),
                    w: self.field_width(),
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
            _ => draw_lines(fb, font, f, &self.lines),
        }
        let buttons = (self.labels().iter().enumerate()).map(|(i, l)| (*l, self.button(i)));
        draw_buttons(fb, font, buttons, self.focus, self.hover);
    }
}

/// What the emulator's dialogs and a program's DIALOG lay out and draw
/// alike. A window of `w` by `h` in the middle of a display of `size`.
pub(crate) fn centered(size: (u32, u32), w: u32, h: u32) -> Rect {
    Rect {
        x: size.0.saturating_sub(w) / 2,
        y: size.1.saturating_sub(h) / 2,
        w,
        h,
    }
}

/// Button `i` of `n`, each `w` wide: they sit along the bottom of the
/// window `f`, right-aligned.
pub(crate) fn button_rect(f: Rect, w: u32, n: usize, i: usize) -> Rect {
    let right = f.x + f.w - 2 * PAD;
    let from_right = (n - i) as u32;
    Rect {
        x: right - from_right * w - (from_right - 1) * PAD,
        y: f.y + f.h - 2 * PAD - LINE,
        w,
        h: LINE,
    }
}

/// Where `key` moves the focus from button `focus` of `n`, if it moves
/// it: Left and Right stop at the ends, as SmileBASIC 4's D-pad does in a
/// DIALOG, and Tab goes round from the last to the first.
pub(crate) fn moved_focus(focus: usize, n: usize, key: Keycode) -> Option<usize> {
    match key {
        Keycode::Left if n > 1 => Some(focus.saturating_sub(1)),
        Keycode::Right if n > 1 => Some((focus + 1).min(n - 1)),
        Keycode::Tab if n > 1 => Some((focus + 1) % n),
        _ => None,
    }
}

/// The window `f` with its title bar and `title`, cut to fit.
pub(crate) fn draw_window(fb: &mut Framebuffer, font: &Font, f: Rect, title: &str) {
    fill(fb, f, BORDER);
    let inner = Rect {
        x: f.x + 2,
        y: f.y + 2,
        w: f.w - 4,
        h: f.h - 4,
    };
    fill(fb, inner, BACKGROUND | 0xFF00_0000);
    fill(fb, Rect { h: LINE, ..inner }, HIGHLIGHT);
    let title = fitted(title, f.w.saturating_sub(4 * PAD));
    draw_text(fb, font, f.x + 2 * PAD, f.y + 6, &title, TEXT);
}

/// The top of line `i` of the body of the window `f`.
pub(crate) fn line_y(f: Rect, i: u32) -> u32 {
    f.y + LINE + 2 * PAD + i * LINE
}

/// The body's lines.
pub(crate) fn draw_lines(fb: &mut Framebuffer, font: &Font, f: Rect, lines: &[String]) {
    for (i, line) in lines.iter().enumerate() {
        let y = line_y(f, i as u32) + (LINE - CHAR) / 2;
        draw_text(fb, font, f.x + 2 * PAD, y, line, TEXT);
    }
}

/// The buttons, each a label and its place; `focus` is the one Enter
/// chooses and `hover` the one under the mouse.
pub(crate) fn draw_buttons<'a>(
    fb: &mut Framebuffer,
    font: &Font,
    buttons: impl Iterator<Item = (&'a str, Rect)>,
    focus: usize,
    hover: Option<usize>,
) {
    for (i, (label, r)) in buttons.enumerate() {
        focus_face(fb, r, i == focus, hover == Some(i));
        let x = r.x + (r.w - text_width(label)) / 2;
        draw_text(fb, font, x, r.y + 4, label, TEXT);
    }
}

/// `text` cut to `width` with a `~`, or as it is if it fits, which
/// takes nothing new.
pub(crate) fn fitted(text: &str, width: u32) -> std::borrow::Cow<'_, str> {
    match text_width(text) <= width {
        true => text.into(),
        false => crate::projects::fit(text, width).into(),
    }
}

/// `lines` wrapped to `width`, and no more than `max` of them (at least
/// one): the last shown ends in `~` when there is more.
pub(crate) fn wrap_lines(lines: &[String], width: u32, max: usize) -> Vec<String> {
    let mut all = lines.iter().flat_map(|l| wrap(l, width));
    let mut shown: Vec<String> = all.by_ref().take(max.max(1)).collect();
    if all.next().is_some()
        && let Some(last) = shown.last_mut()
    {
        last.push('~');
        *last = crate::projects::fit(last, width);
    }
    shown
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

    /// The Simple UI can be 320x240: a DIALOG of the longest text,
    /// caption and name still fits it.
    #[test]
    fn a_dialog_fits_a_320x240_display() {
        let size = (320, 240);
        let mut long = dialog(DialogKind::Choice(vec!["No", "Yes"]));
        long.caption = "見出し".repeat(15);
        long.lines = vec!["A long message that goes on. ".repeat(9)];
        let mut name = dialog(DialogKind::Name {
            initial: "N".repeat(32),
            max: 32,
        });
        name.caption = "C".repeat(39);
        let execute = dialog(DialogKind::Choice(vec!["Cancel", "Execute"]));
        let all = [long, name, execute];
        for d in &all {
            let v = ProgramView::new(d, size);
            let f = v.frame();
            assert!(f.x + f.w <= size.0 && f.y + f.h <= size.1, "{f:?}");
            for i in 0..v.labels().len() {
                let b = v.button(i);
                assert!(b.x >= f.x + PAD && b.y + b.h <= f.y + f.h, "{b:?}");
            }
            assert!(f.x + 2 * PAD + v.field_width() <= f.x + f.w);
            // The whole name and its cursor show in the field.
            if !v.name.is_empty() {
                let shown = text_width(&format!("{}_", v.name)) + 2 * PAD;
                assert!(shown <= v.field_width(), "{shown}");
            }
            let mut fb = Framebuffer::new(size.0, size.1);
            v.draw(&mut fb, &Font::blank(), true);
        }
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

    /// On the desktop in a window narrower than 16:9 the UI goes below
    /// the 1280x720 display, and the window centers on all of it.
    #[test]
    fn a_new_size_centers_the_window_and_keeps_the_choice() {
        let kind = DialogKind::Choice(vec!["No", "Yes"]);
        let mut v = ProgramView::new(&dialog(kind), (1280, 720));
        v.key(Keycode::Left);
        let (focus, at) = (v.focus, v.frame());
        v.set_size((1280, 960));
        let moved = v.frame();
        assert_eq!((moved.x, moved.w, moved.h), (at.x, at.w, at.h));
        assert_eq!(moved.y, at.y + 120);
        assert_eq!(v.focus, focus);
    }

    /// Left and Right stop at the ends, as the D-pad does; Tab goes round.
    #[test]
    fn keys_move_the_focus() {
        let kind = DialogKind::Choice(vec!["No", "Yes"]);
        let mut v = ProgramView::new(&dialog(kind), (1280, 720));
        assert_eq!(v.focus(), 1);
        v.key(Keycode::Right);
        assert_eq!(v.focus(), 1);
        v.key(Keycode::Left);
        v.key(Keycode::Left);
        assert_eq!(v.focus(), 0);
        v.key(Keycode::Tab);
        v.key(Keycode::Tab);
        assert_eq!(v.focus(), 0);
        // The runtime's focus, as the controller moved it.
        v.follow(Some(1));
        assert_eq!(v.key(Keycode::Return), Some(Some(DialogAnswer::Choice(1))));
        v.follow(Some(7));
        assert_eq!(v.focus(), 1);
    }

    /// The mouse over a button leaves the focus where it is.
    #[test]
    fn the_mouse_hovers_without_moving_the_focus() {
        let kind = DialogKind::Choice(vec!["No", "Yes"]);
        let mut v = ProgramView::new(&dialog(kind), (1280, 720));
        v.mouse_move(centre(v.button(0)));
        assert_eq!((v.hover, v.focus()), (Some(0), 1));
        v.mouse_move((0, 0));
        assert_eq!(v.hover, None);
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
