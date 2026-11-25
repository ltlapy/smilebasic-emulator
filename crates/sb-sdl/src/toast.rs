//! Short notices at the bottom of the window, shown instead of asking
//! when file questions are set to notify. A program may read and write
//! many files in a short time, so notices stack, repeats are counted,
//! and the oldest notices that do not fit are summed up in one line.

use std::collections::VecDeque;

use sb_runtime::Framebuffer;
use sb_runtime::font::Font;

use crate::program_dialog::wrap;
use crate::ui::{BACKGROUND, BORDER, CHAR, PAD, Rect, TEXT, draw_text, fill, outline, text_width};

/// About 2.5 seconds at 60 frames per second.
const SHOW_FRAMES: u32 = 150;
const MAX_SHOWN: usize = 6;
/// Notices kept beyond those shown, for the "+N more" count.
const KEPT: usize = 256;
const HEIGHT: u32 = CHAR + 2 * PAD;

#[derive(Debug)]
struct Toast {
    text: String,
    /// The number of times the same notice came in a row.
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
        let hidden = self.items.len().saturating_sub(MAX_SHOWN);
        let mut lines = Vec::new();
        if hidden > 0 {
            lines.push(format!("+{hidden} more"));
        }
        lines.extend(self.items.iter().skip(hidden).map(Toast::label));
        lines
    }

    /// Returns the place of each notice on a frame of `size`: centered at
    /// the bottom, the newest lowest, with its text wrapped to the frame
    /// width. The oldest are dropped first when they do not all fit.
    fn places(&self, (fw, fh): (u32, u32)) -> Vec<(Rect, Vec<String>)> {
        let room = fw.saturating_sub(4 * PAD).max(CHAR);
        let mut y = fh.saturating_sub(PAD);
        let mut places = Vec::new();
        for text in self.lines().iter().rev() {
            let lines = wrap(text, room);
            let h = HEIGHT + (lines.len() as u32 - 1) * CHAR;
            if y < h + PAD / 2 {
                break;
            }
            y -= h + PAD / 2;
            let widest = lines.iter().map(|l| text_width(l)).max().unwrap_or(0);
            let w = widest + 2 * PAD;
            let r = Rect {
                x: fw.saturating_sub(w) / 2,
                y,
                w,
                h,
            };
            places.push((r, lines));
        }
        places
    }

    /// Draws the notices centered at the bottom, the newest lowest.
    pub fn draw(&self, fb: &mut Framebuffer, font: &Font) {
        for (r, lines) in self.places((fb.width(), fb.height())) {
            fill(fb, r, BACKGROUND);
            outline(fb, r, BORDER);
            for (i, line) in lines.iter().enumerate() {
                let y = r.y + PAD + i as u32 * CHAR;
                draw_text(fb, font, r.x + PAD, y, line, TEXT);
            }
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
        assert_eq!(lines.len(), MAX_SHOWN + 1);
        assert_eq!(lines[0], "+6 more");
        assert_eq!(lines.last().unwrap(), "Loading 9");
        for _ in 0..SHOW_FRAMES - 1 {
            t.tick();
        }
        assert!(t.visible());
        t.tick();
        assert!(!t.visible());
    }

    /// The Simple UI can be 320x240: a long notice wraps, and none
    /// goes past the frame's edges.
    #[test]
    fn notices_fit_a_320x240_frame() {
        let size = (320, 240);
        let mut t = Toasts::default();
        t.push("Rename the folder bad name to use it as a project".into());
        let places = t.places(size);
        assert_eq!(places[0].1.len(), 2, "{:?}", places[0].1);
        for i in 0..10 {
            t.push(format!(
                "Could not load PROGRAM{i}.PRG: the file is not there"
            ));
        }
        let places = t.places(size);
        assert!(places.len() < t.lines().len(), "not all of them fit");
        for (r, _) in places {
            assert!(
                r.x + r.w <= size.0 && r.y + r.h <= size.1 && r.y >= PAD / 2,
                "{r:?}"
            );
        }
        let mut fb = Framebuffer::new(size.0, size.1);
        t.draw(&mut fb, &Font::blank());
        // On the desktop a notice keeps to one line.
        assert!(t.places((1280, 720)).iter().all(|(_, l)| l.len() == 1));
    }
}
