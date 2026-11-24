//! Glyphs from the editor's other font sources put on the font page
//! (GRP5), so that programs draw with them: an option of the settings,
//! chosen separately for the 8- and the 16-pixel font. SmileBASIC 4 has
//! no such setting.
//!
//! Only characters that have their own place on the font page change, and
//! only those the chosen source has; the rest keep SmileBASIC's glyphs.
//! Characters the page has no place for keep the box. The glyphs are made
//! once, when the setting or the font file changes, and written over the
//! page then and every time the page is put back: at start-up and by ACLS.

use crate::extra_glyphs::{ExtraGlyphs, FontSource, Part};
use crate::font::{Font, FontSize, PAGE_SIZE};

/// Glyph numbers on the font page: font.map's glyph numbers are 12 bits.
const GLYPHS: usize = 4096;

/// The glyphs written over the font page.
#[derive(Default)]
pub struct FontPatch {
    /// Top left corner and size of each glyph's cell on the page.
    cells: Vec<(u32, u32, FontSize)>,
    /// The glyphs' alpha bytes, one per pixel, in the order of `cells`.
    alpha: Vec<u8>,
}

impl FontPatch {
    /// The glyphs `sources` (for the 8- and the 16-pixel font) put over
    /// `font`'s page. Empty when both are SmileBASIC's font, the default.
    pub fn build(font: &Font, extra: &ExtraGlyphs, sources: [FontSource; 2]) -> FontPatch {
        let mut patch = FontPatch::default();
        let mut glyph = Vec::new();
        for (size, source) in [FontSize::Small, FontSize::Large].into_iter().zip(sources) {
            let part = match source {
                FontSource::SmileBasic => continue,
                FontSource::File if !extra.has_file() => continue,
                FontSource::File => Part::File,
                FontSource::Bundled => Part::Bundled,
            };
            // Codes that share a glyph take the glyph of the first one that
            // the source can draw.
            let mut done = vec![false; GLYPHS];
            for code in 0..=u16::MAX {
                let Some(n) = font.own_glyph(code) else {
                    continue;
                };
                let n = n as usize % GLYPHS;
                if done[n] || !extra.render(u32::from(code), size, part, &mut glyph) {
                    continue;
                }
                done[n] = true;
                let Some((x, y)) = font.origin(code, size) else {
                    continue;
                };
                patch.cells.push((x, y, size));
                patch.alpha.extend_from_slice(&glyph);
            }
        }
        patch
    }

    pub fn is_empty(&self) -> bool {
        self.cells.is_empty()
    }

    /// Writes the glyphs over `page`, `PAGE_SIZE` pixels square, in white
    /// like SmileBASIC's font.
    pub fn apply(&self, page: &mut [u32]) {
        let mut alpha = self.alpha.iter();
        for &(x, y, size) in &self.cells {
            let n = size as u32;
            for row in y..y + n {
                let at = (row * PAGE_SIZE + x) as usize;
                for p in &mut page[at..at + n as usize] {
                    let a = u32::from(*alpha.next().unwrap_or(&0));
                    *p = if a == 0 { 0 } else { a << 24 | 0x00FF_FFFF };
                }
            }
        }
    }

    /// Copies the cells this patch writes from `from` to `to`, both
    /// `PAGE_SIZE` pixels square: puts back the pixels the patch covered.
    pub fn restore(&self, to: &mut [u32], from: &[u32]) {
        for &(x, y, size) in &self.cells {
            let n = size as u32;
            for row in y..y + n {
                let at = (row * PAGE_SIZE + x) as usize;
                to[at..at + n as usize].copy_from_slice(&from[at..at + n as usize]);
            }
        }
    }

    /// The glyph cells written, as top left corner and size.
    #[cfg(test)]
    pub fn cells(&self) -> impl Iterator<Item = (u32, u32, FontSize)> + '_ {
        self.cells.iter().copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::page::Page;

    #[test]
    fn the_default_changes_nothing() {
        let font = Font::fallback();
        let patch = FontPatch::build(&font, &ExtraGlyphs::bundled(), [FontSource::SmileBasic; 2]);
        assert!(patch.is_empty());
        // A file chosen first with no file read is the same.
        let patch = FontPatch::build(&font, &ExtraGlyphs::bundled(), [FontSource::File; 2]);
        assert!(patch.is_empty());
    }

    #[test]
    fn sizes_are_chosen_apart() {
        let font = Font::fallback();
        let extra = ExtraGlyphs::bundled();
        let small = FontPatch::build(&font, &extra, [FontSource::Bundled, FontSource::SmileBasic]);
        assert!(!small.is_empty());
        assert!(small.cells().all(|(_, _, size)| size == FontSize::Small));
        let large = FontPatch::build(&font, &extra, [FontSource::SmileBasic, FontSource::Bundled]);
        assert!(!large.is_empty());
        assert!(large.cells().all(|(_, _, size)| size == FontSize::Large));
        // 가 has no place on the page, so it is never written.
        let (gx, gy) = font.origin('A' as u16, FontSize::Large).unwrap();
        assert!(large.cells().any(|(x, y, _)| (x, y) == (gx, gy)));
        assert_eq!(font.own_glyph('가' as u16), None);
    }

    #[test]
    fn apply_and_restore() {
        let font = Font::fallback();
        let extra = ExtraGlyphs::bundled();
        let patch = FontPatch::build(&font, &extra, [FontSource::Bundled; 2]);
        let original = font.page_image();
        let mut page = original.clone();
        patch.apply(&mut page);
        assert_ne!(page, original);
        patch.restore(&mut page, &original);
        assert_eq!(page, original);
    }

    #[test]
    fn written_on_change_and_by_acls() {
        use crate::runtime::Runtime;
        use crate::text::FONT_PAGE;
        let mut rt = Runtime::for_tests(Font::fallback());
        let original = rt.font.page_image();
        // The defaults leave SmileBASIC's page unchanged.
        rt.apply_program_font();
        assert_eq!(rt.grp.page(FONT_PAGE).to_rows(), original);
        let (ax, ay) = rt.font.origin('A' as u16, FontSize::Large).unwrap();
        let (sx, sy) = rt.font.origin('A' as u16, FontSize::Small).unwrap();
        let cell = |rt: &Runtime, x: u32, y: u32, n: u32| -> Vec<u32> {
            (0..n * n)
                .map(|i| {
                    rt.grp
                        .pixel(FONT_PAGE, (x + i % n) as i32, (y + i / n) as i32)
                })
                .collect()
        };
        let original_a = cell(&rt, ax, ay, 16);
        // The 16-pixel font alone changes when the setting does.
        rt.settings.program_font = [FontSource::SmileBasic, FontSource::Bundled];
        rt.apply_program_font();
        let bundled_a = cell(&rt, ax, ay, 16);
        assert_ne!(bundled_a, original_a);
        assert_eq!(
            cell(&rt, sx, sy, 8),
            (0..64)
                .map(|i| { original[((sy + i / 8) * PAGE_SIZE + sx + i % 8) as usize] })
                .collect::<Vec<_>>()
        );
        // A program draws over it; ACLS puts the chosen glyph back.
        let mut page = rt.grp.page(FONT_PAGE).to_rows();
        page[(ay * PAGE_SIZE + ax) as usize] = 0xFFFF_0000;
        rt.grp.set_page(FONT_PAGE, Page::from_rows(page));
        assert_ne!(cell(&rt, ax, ay, 16), bundled_a);
        rt.acls(false, false, false, false);
        assert_eq!(cell(&rt, ax, ay, 16), bundled_a);
        // Back to SmileBASIC's font: the glyph is SmileBASIC's again.
        rt.settings.program_font = [FontSource::SmileBasic; 2];
        rt.apply_program_font();
        assert_eq!(cell(&rt, ax, ay, 16), original_a);
        rt.acls(false, false, false, false);
        assert_eq!(rt.grp.page(FONT_PAGE).to_rows(), original);
    }

    #[test]
    fn the_bundled_set_redraws_the_fallback_font() {
        use crate::extra_glyphs::BundledSet;
        use crate::runtime::Runtime;
        use crate::text::FONT_PAGE;
        let mut rt = Runtime::for_tests(Font::fallback());
        let outline = rt.font.page_image();
        // A program draws on the 8-pixel half, which no set changes.
        let (sx, sy) = rt.font.origin('A' as u16, FontSize::Small).unwrap();
        let mut page = rt.grp.page(FONT_PAGE).to_rows();
        page[(sy * PAGE_SIZE + sx) as usize] = 0xFFFF_0000;
        rt.grp.set_page(FONT_PAGE, Page::from_rows(page));
        // The same set changes nothing.
        rt.apply_bundled_glyphs();
        assert_eq!(rt.font.page_image(), outline);
        rt.settings.bundled_glyphs16 = BundledSet::Pixel;
        rt.apply_bundled_glyphs();
        assert_eq!(rt.extra_glyphs.bundled_set(), BundledSet::Pixel);
        let dot = rt.font.page_image();
        assert_ne!(dot, outline);
        assert_eq!(dot, Font::fallback_with(BundledSet::Pixel).page_image());
        // The 16-pixel glyphs change on GRP5; the program's pixel stays.
        let (ax, ay) = rt.font.origin('あ' as u16, FontSize::Large).unwrap();
        let cell = |page: &[u32]| -> Vec<u32> {
            (0..256)
                .map(|i| page[((ay + i / 16) * PAGE_SIZE + ax + i % 16) as usize])
                .collect()
        };
        assert_eq!(cell(&rt.grp.page(FONT_PAGE).to_rows()), cell(&dot));
        assert_ne!(cell(&dot), cell(&outline));
        assert_eq!(rt.grp.pixel(FONT_PAGE, sx as i32, sy as i32), 0xFFFF_0000);
        // The bundled program font takes the set too.
        rt.settings.program_font = [FontSource::SmileBasic, FontSource::Bundled];
        rt.apply_program_font();
        let patched = cell(&rt.grp.page(FONT_PAGE).to_rows());
        rt.settings.bundled_glyphs16 = BundledSet::Antialiased;
        rt.apply_bundled_glyphs();
        assert_ne!(cell(&rt.grp.page(FONT_PAGE).to_rows()), patched);
        assert_eq!(rt.font.page_image(), outline);
    }
}
