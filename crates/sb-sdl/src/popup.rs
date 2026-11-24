//! A menu that opens where the mouse was right-clicked, kept within the
//! display.

use sb_runtime::Framebuffer;
use sb_runtime::font::Font;

use crate::ui::{
    BACKGROUND, BORDER, DISABLED, HIGHLIGHT, LINE, PAD, Rect, TEXT, draw_text, fill, outline,
    text_width,
};

pub struct Popup<T> {
    /// Each item's text, whether it can be chosen, and what it does.
    items: Vec<(String, bool, T)>,
    frame: Rect,
    hovered: Option<usize>,
}

impl<T: Clone> Popup<T> {
    pub fn new(at: (i32, i32), items: Vec<(String, bool, T)>, (w, h): (u32, u32)) -> Popup<T> {
        let width = (items.iter())
            .map(|(t, _, _)| text_width(t))
            .max()
            .unwrap_or(0)
            + 2 * PAD;
        let height = items.len() as u32 * LINE + 4;
        let x = (at.0.max(0) as u32).min(w.saturating_sub(width));
        let y = (at.1.max(0) as u32).min(h.saturating_sub(height));
        Popup {
            items,
            frame: Rect {
                x,
                y,
                w: width,
                h: height,
            },
            hovered: None,
        }
    }

    fn item(&self, i: usize) -> Rect {
        Rect {
            x: self.frame.x + 2,
            y: self.frame.y + 2 + i as u32 * LINE,
            w: self.frame.w - 4,
            h: LINE,
        }
    }

    fn at(&self, at: (i32, i32)) -> Option<usize> {
        (0..self.items.len()).find(|&i| self.item(i).contains(at))
    }

    pub fn hover(&mut self, at: (i32, i32)) {
        self.hovered = self.at(at).filter(|&i| self.items[i].1);
    }

    /// Returns the action of the item under a click, if it can be chosen.
    pub fn click(&self, at: (i32, i32)) -> Option<T> {
        let (_, enabled, what) = &self.items[self.at(at)?];
        enabled.then(|| what.clone())
    }

    pub fn draw(&self, fb: &mut Framebuffer, font: &Font) {
        fill(fb, self.frame, BACKGROUND | 0xFF00_0000);
        outline(fb, self.frame, BORDER);
        for (i, (text, enabled, _)) in self.items.iter().enumerate() {
            let r = self.item(i);
            if self.hovered == Some(i) {
                fill(fb, r, HIGHLIGHT);
            }
            let color = if *enabled { TEXT } else { DISABLED };
            draw_text(fb, font, r.x + PAD - 2, r.y + 4, text, color);
        }
    }
}
