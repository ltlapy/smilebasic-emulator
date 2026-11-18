//! The bottom right corner's notices: the picture of the keys that press
//! the buttons ([`KeyHint`]) and the notice for a program slower than
//! real time ([`SlowHint`]). Each is a panel in that corner for about six
//! seconds, and the second waits for the first to go.

use sb_runtime::Framebuffer;
use sb_runtime::font::Font;

use crate::keyhint::KeyHint;
use crate::slow_hint::SlowHint;
use crate::ui::{BACKGROUND, BORDER, PAD, Rect, fill, outline};

/// How long a notice shows: about 6 seconds at 60 frames per second.
pub const SHOW_FRAMES: u32 = 360;

/// Where a panel of `w` x `h` pixels goes on a frame of `fw` x `fh`: the
/// bottom right corner, a pad's width from the edges.
pub fn place((fw, fh): (u32, u32), (w, h): (u32, u32)) -> Rect {
    Rect {
        x: fw.saturating_sub(w + PAD),
        y: fh.saturating_sub(h + PAD),
        w,
        h,
    }
}

/// Draws an empty panel of `size` in the corner of `fb` and gives where
/// it is.
pub fn panel(fb: &mut Framebuffer, size: (u32, u32)) -> Rect {
    let panel = place((fb.width(), fb.height()), size);
    fill(fb, panel, BACKGROUND);
    outline(fb, panel, BORDER);
    panel
}

/// Whether either notice shows.
pub fn visible(keys: &KeyHint, slow: &SlowHint) -> bool {
    keys.visible() || slow.visible()
}

/// Draws the notices that show on `fb`, the slow program's worded for the
/// Simple UI or the desktop.
pub fn draw(fb: &mut Framebuffer, font: &Font, keys: &KeyHint, slow: &SlowHint, simple_ui: bool) {
    if keys.visible() {
        keys.draw(fb, font);
    }
    if slow.visible() {
        slow.draw(fb, font, simple_ui);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_panel_sits_a_pad_from_the_bottom_right() {
        let at = |frame, size| {
            let r = place(frame, size);
            (r.x, r.y, r.w, r.h)
        };
        assert_eq!(
            at((1280, 720), (100, 50)),
            (1280 - 100 - PAD, 720 - 50 - PAD, 100, 50)
        );
        // On a frame smaller than the panel it starts at the frame's edge.
        assert_eq!(at((80, 40), (100, 50)), (0, 0, 100, 50));
    }
}
