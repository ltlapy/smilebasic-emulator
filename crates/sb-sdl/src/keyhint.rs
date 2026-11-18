//! A picture of which keys press controller 1's buttons, shown in the
//! bottom-right corner once, the first time a program reads buttons
//! without a gamepad connected.

use sb_runtime::Framebuffer;
use sb_runtime::font::Font;
use sdl3::keyboard::Scancode;

use crate::controls::KEY_BUTTONS;
use crate::corner::{self, SHOW_FRAMES};
use crate::ui::{BORDER, CHAR, DISABLED, PAD, Rect, TEXT, draw_text, fill, outline, text_width};

const CAP: u32 = 40;
const GAP: u32 = 4;
const STEP: u32 = CAP + GAP;
/// Button names stand out from the keys.
pub(crate) const BUTTON_TEXT: u32 = 0xFF7F_BFFF;
const TITLE: &str = "Keys for the buttons";
const FOOTER: &str = "Off: Run menu";

/// BUTTON's names, by id.
const NAMES: [&str; 12] = ["X", "B", "Y", "A", "", "", "", "", "L", "R", "ZL", "ZR"];

/// Where each button's keycap goes, in steps of a cap from the picture's
/// top left: shoulders on top, the direction pad left, the face buttons
/// right.
const PLACES: [(u32, (u32, u32)); 12] = [
    (8, (0, 0)),
    (10, (1, 0)),
    (11, (5, 0)),
    (9, (6, 0)),
    (4, (1, 1)),
    (6, (0, 2)),
    (7, (2, 2)),
    (5, (1, 3)),
    (0, (5, 1)),
    (2, (4, 2)),
    (3, (6, 2)),
    (1, (5, 3)),
];

#[derive(Debug, Default)]
pub struct KeyHint {
    frames_left: u32,
    /// Shown once already, or not needed.
    done: bool,
}

impl KeyHint {
    /// The program read buttons: shows the picture if it has not been
    /// shown and the keyboard is what presses them.
    pub fn buttons_read(&mut self, keys_press_buttons: bool, gamepads: usize) {
        if self.done {
            return;
        }
        self.done = true;
        if keys_press_buttons && gamepads == 0 {
            self.frames_left = SHOW_FRAMES;
        }
    }

    pub fn tick(&mut self) {
        self.frames_left = self.frames_left.saturating_sub(1);
    }

    pub fn visible(&self) -> bool {
        self.frames_left > 0
    }

    pub fn draw(&self, fb: &mut Framebuffer, font: &Font) {
        let w = (7 * STEP - GAP).max(text_width(TITLE)) + 2 * PAD;
        let h = 2 * CHAR + 4 * STEP - GAP + 4 * PAD;
        let panel = corner::panel(fb, (w, h));
        draw_text(fb, font, panel.x + PAD, panel.y + PAD, TITLE, TEXT);
        let (x0, y0) = (panel.x + PAD, panel.y + 2 * PAD + CHAR);
        for (id, (col, row)) in PLACES {
            let Some(&(key, _)) = KEY_BUTTONS.iter().find(|(_, b)| *b == id) else {
                continue;
            };
            let cap = Rect {
                x: x0 + col * STEP,
                y: y0 + row * STEP,
                w: CAP,
                h: CAP,
            };
            draw_cap(fb, font, cap, key, NAMES[id as usize]);
        }
        let footer_y = panel.y + panel.h - PAD - CHAR;
        draw_text(fb, font, panel.x + PAD, footer_y, FOOTER, DISABLED);
    }
}

/// A keycap with the key on it, and the button's name under the key; the
/// cursor keys show an arrow, which is the button's place too.
fn draw_cap(fb: &mut Framebuffer, font: &Font, cap: Rect, key: Scancode, button: &str) {
    fill(fb, cap, 0xFF30_3030);
    outline(fb, cap, BORDER);
    let arrow = match key {
        Scancode::Up => Some((0, -1)),
        Scancode::Down => Some((0, 1)),
        Scancode::Left => Some((-1, 0)),
        Scancode::Right => Some((1, 0)),
        _ => None,
    };
    if let Some(dir) = arrow {
        draw_arrow(fb, cap, dir, TEXT);
        return;
    }
    let name = key.name();
    let center = |text: &str| cap.x + cap.w.saturating_sub(text_width(text)) / 2;
    draw_text(fb, font, center(name), cap.y + 3, name, TEXT);
    draw_text(
        fb,
        font,
        center(button),
        cap.y + cap.h - CHAR - 3,
        button,
        BUTTON_TEXT,
    );
}

/// A filled triangle pointing `dir` in the middle of `cap`.
pub(crate) fn draw_arrow(fb: &mut Framebuffer, cap: Rect, (dx, dy): (i32, i32), color: u32) {
    if crate::ui::dry(fb, cap, ("arrow", cap, dx, dy, color)) {
        return;
    }
    let size = cap.w as i32 / 4;
    let (cx, cy) = ((cap.x + cap.w / 2) as i32, (cap.y + cap.h / 2) as i32);
    for along in -size..=size {
        // Wide at the back, a point at the front.
        let half = (size - along) / 2 + 1;
        for across in -half..=half {
            let (x, y) = if dx != 0 {
                (cx + along * dx, cy + across)
            } else {
                (cx + across, cy + along * dy)
            };
            if x >= 0 && y >= 0 {
                fb.set(x as u32, y as u32, color);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shows_once_and_only_for_the_keyboard() {
        let mut hint = KeyHint::default();
        hint.buttons_read(true, 1);
        assert!(!hint.visible());
        hint.buttons_read(true, 0);
        assert!(!hint.visible(), "only the first read counts");

        let mut hint = KeyHint::default();
        hint.buttons_read(true, 0);
        assert!(hint.visible());
        for _ in 0..SHOW_FRAMES {
            hint.tick();
        }
        assert!(!hint.visible());
        hint.buttons_read(true, 0);
        assert!(!hint.visible());
    }

    #[test]
    fn every_mapped_button_has_a_place() {
        for (_, id) in KEY_BUTTONS {
            assert!(PLACES.iter().any(|(b, _)| *b == id), "button {id}");
        }
    }
}
