//! Short notices at the bottom of the window, shown instead of asking
//! when file questions are set to notify.

use std::collections::VecDeque;

use sb_runtime::Framebuffer;
use sb_runtime::font::Font;

use crate::ui::{BACKGROUND, CHAR, PAD, Rect, TEXT, draw_text, fill, text_width};

/// About 2.5 seconds at 60 frames per second.
const SHOW_FRAMES: u32 = 150;
const SHOWN_AT_ONCE: usize = 4;
const HEIGHT: u32 = CHAR + 2 * PAD;

#[derive(Debug, Default)]
pub struct Toasts {
    /// Text and frames left, oldest first.
    items: VecDeque<(String, u32)>,
}

impl Toasts {
    pub fn push(&mut self, text: String) {
        if self.items.len() == SHOWN_AT_ONCE {
            self.items.pop_front();
        }
        self.items.push_back((text, SHOW_FRAMES));
    }

    /// Counts down one frame.
    pub fn tick(&mut self) {
        for (_, left) in &mut self.items {
            *left -= 1;
        }
        self.items.retain(|(_, left)| *left > 0);
    }

    pub fn visible(&self) -> bool {
        !self.items.is_empty()
    }

    /// Draws the notices centered at the bottom, the newest lowest.
    pub fn draw(&self, fb: &mut Framebuffer, font: &Font) {
        let mut y = fb.height().saturating_sub(PAD);
        for (text, _) in self.items.iter().rev() {
            y = y.saturating_sub(HEIGHT + PAD / 2);
            let w = text_width(text) + 2 * PAD;
            let r = Rect {
                x: fb.width().saturating_sub(w) / 2,
                y,
                w,
                h: HEIGHT,
            };
            fill(fb, r, BACKGROUND);
            draw_text(fb, font, r.x + PAD, r.y + PAD, text, TEXT);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notices_expire_and_keep_the_newest() {
        let mut t = Toasts::default();
        for i in 0..5 {
            t.push(i.to_string());
        }
        assert_eq!(t.items.front().unwrap().0, "1");
        for _ in 0..SHOW_FRAMES - 1 {
            t.tick();
        }
        assert!(t.visible());
        t.tick();
        assert!(!t.visible());
    }
}
