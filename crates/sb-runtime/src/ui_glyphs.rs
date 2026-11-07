//! Smooth glyphs for the emulator's own menus and windows, when the
//! settings ask for them (`ui_font=smooth`): outlines drawn for a
//! 16-pixel cell with gray edges. Programs never see them.
//!
//! Full-width characters take the editor's bundled 16-pixel glyphs:
//! M PLUS 1p Bold, then Galmuri14 and Noto Emoji. Half-width ones take
//! M PLUS Code Latin 50 Bold, drawn half an em wide, then M PLUS 1p Bold's
//! own half-width forms. Both are drawn with the em and baseline of the
//! bundled 16-pixel font, so they line up. A character none of them has,
//! and SmileBASIC's own symbols, get `None`, for the caller to draw from
//! SmileBASIC's font.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::OnceLock;

use ab_glyph::FontRef;

use crate::cell_glyph;
use crate::extra_glyphs::{ExtraGlyphs, Part};
use crate::font::{self, FontSize};

/// The side of a full-width glyph's cell; half-width ones are half as wide.
pub const CELL: u32 = 16;

/// M PLUS Code Latin 50 Bold (see `fonts/mplus`): monospaced Latin half an
/// em wide, from the same project as M PLUS 1p.
static CODE_LATIN: &[u8] = include_bytes!("../fonts/mplus/MPLUSCodeLatin50-Bold.ttf");

fn code_latin() -> &'static FontRef<'static> {
    static FONT: OnceLock<FontRef<'static>> = OnceLock::new();
    FONT.get_or_init(|| FontRef::try_from_slice(CODE_LATIN).expect("bundled font"))
}

/// A glyph's alpha bytes, shared with the cache.
type Alpha = Rc<[u8]>;

/// Smooth glyphs, each made once when first wanted.
#[derive(Default)]
pub struct UiGlyphs {
    /// What each code was found as, full- or half-width.
    found: RefCell<HashMap<(u16, bool), Option<Alpha>>>,
    bundled: ExtraGlyphs,
}

impl UiGlyphs {
    /// The glyph for `code`, one alpha byte per pixel in rows of
    /// [`CELL`] / 2 pixels if `half`, else [`CELL`] pixels, [`CELL`] rows.
    pub fn get(&self, code: u16, half: bool) -> Option<Alpha> {
        if let Some(found) = self.found.borrow().get(&(code, half)) {
            return found.clone();
        }
        let made = match half {
            true => half_glyph(code),
            false => {
                let mut out = Vec::new();
                self.bundled
                    .render(u32::from(code), FontSize::Large, Part::Bundled, &mut out)
                    .then_some(out)
            }
        };
        let made: Option<Alpha> = made.map(Rc::from);
        self.found.borrow_mut().insert((code, half), made.clone());
        made
    }
}

fn half_glyph(code: u16) -> Option<Vec<u8>> {
    let c = char::from_u32(u32::from(code))?;
    let em = cell_glyph::SMILEBASIC_EM;
    cell_glyph::outline_narrow(code_latin(), c, CELL, em)
        .or_else(|| cell_glyph::outline_narrow(font::outlines(), c, CELL, em))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ink(alpha: &[u8]) -> usize {
        alpha.iter().filter(|&&a| a > 0).count()
    }

    #[test]
    fn half_and_full_width_glyphs() {
        let g = UiGlyphs::default();
        for c in ['A', 'g', 'W', '@', 'ｱ'] {
            let glyph = g.get(c as u16, true).unwrap();
            assert_eq!(glyph.len(), (CELL / 2 * CELL) as usize, "{c}");
            assert!(ink(&glyph) > 4, "{c}");
            // Gray edges, not only solid pixels.
            assert!(glyph.iter().any(|&a| a > 0 && a < 255), "{c}");
        }
        for c in ['あ', '漢', 'Ж', '한', '★'] {
            let glyph = g.get(c as u16, false).unwrap();
            assert_eq!(glyph.len(), (CELL * CELL) as usize, "{c}");
            assert!(ink(&glyph) > 8, "{c}");
        }
        // SmileBASIC's own symbols come from its font.
        assert!(g.get(0xE100, false).is_none());
        assert!(g.get(0xE100, true).is_none());
    }

    #[test]
    fn half_width_capitals_sit_on_the_full_width_baseline() {
        let g = UiGlyphs::default();
        let bottom = |alpha: &[u8], w: usize| {
            (0..CELL as usize)
                .rev()
                .find(|&y| alpha[y * w..(y + 1) * w].iter().any(|&a| a >= 128))
                .unwrap()
        };
        let h = g.get('H' as u16, true).unwrap();
        let full = g.get('Ｈ' as u16, false).unwrap();
        assert_eq!(bottom(&h, 8), bottom(&full, 16));
    }
}
