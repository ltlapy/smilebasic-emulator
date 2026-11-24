//! Glyphs from the bundled fonts for the emulator's own menus and
//! windows. Programs never see them.
//!
//! The smooth font (`ui_font=smooth`) is outlines drawn for a 16-pixel
//! cell with gray edges. Full-width characters take the editor's bundled
//! 16-pixel glyphs, from the set the settings pick
//! ([`UiGlyphs::set_bundled`]). Half-width ones take M PLUS Code Latin 50
//! Bold, drawn half an em wide, then M PLUS 1p Bold's own half-width
//! forms. Both are drawn with the em and baseline of the bundled 16-pixel
//! font, so they line up.
//!
//! With SmileBASIC's own font (`ui_font=16`, imported), the characters it
//! lacks take the editor's bundled glyphs of the size drawn
//! ([`UiGlyphs::bundled`]), as in the editor.
//!
//! A character none of them has, and SmileBASIC's own symbols, get `None`,
//! for the caller to draw from SmileBASIC's font.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::OnceLock;

use ab_glyph::FontRef;

use crate::cell_glyph;
use crate::extra_glyphs::{BundledSet, ExtraGlyphs, Part};
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

/// The kind of glyph looked up.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Kind {
    /// A smooth half-width glyph.
    Half,
    /// The editor's bundled glyph at this size, from this set.
    Full(FontSize, BundledSet),
}

/// Glyphs from the bundled fonts, each made once when first needed.
#[derive(Default)]
pub struct UiGlyphs {
    /// The lookup result for each code.
    found: RefCell<HashMap<(u32, Kind), Option<Alpha>>>,
    bundled: ExtraGlyphs,
    set: Cell<BundledSet>,
}

impl UiGlyphs {
    /// Picks the set of bundled 16-pixel glyphs. The glyphs of the other
    /// set are kept, for when it is picked again.
    pub fn set_bundled(&self, set: BundledSet) {
        self.set.set(set);
        self.bundled.set_bundled(set);
    }

    pub fn bundled_set(&self) -> BundledSet {
        self.set.get()
    }

    /// The smooth glyph for `code`, one alpha byte per pixel in rows of
    /// [`CELL`] / 2 pixels if `half`, else [`CELL`] pixels, [`CELL`] rows.
    pub fn get(&self, code: u16, half: bool) -> Option<Alpha> {
        match half {
            true => self.find(u32::from(code), Kind::Half),
            false => self.bundled(u32::from(code), FontSize::Large),
        }
    }

    /// The editor's bundled glyph for the character `code` (beyond U+FFFF
    /// too), `size` x `size` alpha bytes, row by row.
    pub fn bundled(&self, code: u32, size: FontSize) -> Option<Alpha> {
        self.find(code, Kind::Full(size, self.set.get()))
    }

    fn find(&self, code: u32, kind: Kind) -> Option<Alpha> {
        if let Some(found) = self.found.borrow().get(&(code, kind)) {
            return found.clone();
        }
        let made = match kind {
            Kind::Half => u16::try_from(code).ok().and_then(half_glyph),
            Kind::Full(size, _) => {
                let mut out = Vec::new();
                self.bundled
                    .render(code, size, Part::Bundled, &mut out)
                    .then_some(out)
            }
        };
        let made: Option<Alpha> = made.map(Rc::from);
        self.found.borrow_mut().insert((code, kind), made.clone());
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
    fn bundled_glyphs_at_both_sizes() {
        let g = UiGlyphs::default();
        for (size, side) in [(FontSize::Small, 8), (FontSize::Large, 16)] {
            for c in ['한', '汉', 'Ж', 'Ω', '★', '☀', '𠮷'] {
                let glyph = g.bundled(c as u32, size).unwrap();
                assert_eq!(glyph.len(), side * side, "{c} at {size:?}");
                assert!(ink(&glyph) > 4, "{c} at {size:?}");
            }
            assert!(g.bundled(0xE100, size).is_none());
        }
        // The smooth full-width glyph is the 16-pixel one, kept once.
        let smooth = g.get('한' as u16, false).unwrap();
        let bundled = g.bundled('한' as u32, FontSize::Large).unwrap();
        assert!(Rc::ptr_eq(&smooth, &bundled));
    }

    #[test]
    fn the_bundled_set_picks_the_full_width_glyphs() {
        let g = UiGlyphs::default();
        let outline = g.get('漢' as u16, false).unwrap();
        let small = g.bundled('漢' as u32, FontSize::Small).unwrap();
        g.set_bundled(BundledSet::Pixel);
        assert_eq!(g.bundled_set(), BundledSet::Pixel);
        let dot = g.get('漢' as u16, false).unwrap();
        let mut cell = Vec::new();
        assert!(crate::extra_glyphs::dot16_cell('漢' as u32, &mut cell));
        assert_eq!(*dot, *cell);
        assert_ne!(*dot, *outline);
        // The 8-pixel glyphs stay.
        assert_eq!(g.bundled('漢' as u32, FontSize::Small).unwrap(), small);
        g.set_bundled(BundledSet::Antialiased);
        assert!(Rc::ptr_eq(&g.get('漢' as u16, false).unwrap(), &outline));
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
