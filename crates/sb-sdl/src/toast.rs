//! Short notices at the bottom of the window, shown instead of asking
//! when file questions are set to notify. A program may read and write
//! many files at once, so notices stack, repeats are counted, and the
//! oldest beyond what fits are summed up in one line.

use std::collections::VecDeque;

use sb_runtime::Framebuffer;
use sb_runtime::font::Font;

use crate::ui::{BACKGROUND, BORDER, CHAR, PAD, Rect, TEXT, draw_text, fill, outline, text_width};

/// About 2.5 seconds at 60 frames per second.
const SHOW_FRAMES: u32 = 150;
const SHOWN_AT_ONCE: usize = 6;
/// Notices kept beyond those shown, for the "+N more" count.
const KEPT: usize = 256;
const HEIGHT: u32 = CHAR + 2 * PAD;

#[derive(Debug)]
struct Toast {
    text: String,
    /// How many times the same notice came in a row.
    count: u32,
    frames_left: u32,
}

impl Toast {
    fn label(&self) -> String {
        match self.count {
            1 => self.text.clone(),
            n => format!("{} (x{n})", self.text),
        }
    }
}

#[derive(Debug, Default)]
pub struct Toasts {
    /// Oldest first.
    items: VecDeque<Toast>,
}

impl Toasts {
    pub fn push(&mut self, text: String) {
        if let Some(last) = self.items.back_mut()
            && last.text == text
        {
            last.count += 1;
            last.frames_left = SHOW_FRAMES;
            return;
        }
        if self.items.len() == KEPT {
            self.items.pop_front();
        }
        self.items.push_back(Toast {
            text,
            count: 1,
            frames_left: SHOW_FRAMES,
        });
    }

    /// Counts down one frame.
    pub fn tick(&mut self) {
        for toast in &mut self.items {
            toast.frames_left -= 1;
        }
        self.items.retain(|t| t.frames_left > 0);
    }

    #[cfg(test)]
    pub fn visible(&self) -> bool {
        !self.items.is_empty()
    }

    /// The lines to show, top to bottom.
    fn lines(&self) -> Vec<String> {
        let hidden = self.items.len().saturating_sub(SHOWN_AT_ONCE);
        let mut lines = Vec::new();
        if hidden > 0 {
            lines.push(format!("+{hidden} more"));
        }
        lines.extend(self.items.iter().skip(hidden).map(Toast::label));
        lines
    }

    /// Draws the notices centered at the bottom, the newest lowest.
    pub fn draw(&self, fb: &mut Framebuffer, font: &Font) {
        let lines = self.lines();
        let mut y = fb.height().saturating_sub(PAD);
        for text in lines.iter().rev() {
            y = y.saturating_sub(HEIGHT + PAD / 2);
            let w = text_width(text) + 2 * PAD;
            let r = Rect {
                x: fb.width().saturating_sub(w) / 2,
                y,
                w,
                h: HEIGHT,
            };
            fill(fb, r, BACKGROUND);
            outline(fb, r, BORDER);
            draw_text(fb, font, r.x + PAD, r.y + PAD, text, TEXT);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notices_stack_count_repeats_and_expire() {
        let mut t = Toasts::default();
        t.push("Saving A".into());
        t.push("Saving A".into());
        t.push("Saving B".into());
        assert_eq!(t.lines(), ["Saving A (x2)", "Saving B"]);
        for i in 0..10 {
            t.push(format!("Loading {i}"));
        }
        let lines = t.lines();
        assert_eq!(lines.len(), SHOWN_AT_ONCE + 1);
        assert_eq!(lines[0], "+6 more");
        assert_eq!(lines.last().unwrap(), "Loading 9");
        for _ in 0..SHOW_FRAMES - 1 {
            t.tick();
        }
        assert!(t.visible());
        t.tick();
        assert!(!t.visible());
    }
}
