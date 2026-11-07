//! Glyphs fitted to one square character cell, from BDF bitmaps or from
//! outlines, as one alpha byte per pixel. `build.rs` includes this file
//! too, so it uses only std and ab_glyph.

use ab_glyph::{Font, PxScale, point};

/// One glyph of a BDF font.
pub struct BdfGlyph {
    pub code: u32,
    /// DWIDTH: how far the pen moves.
    pub advance: i32,
    /// BBX: width, height, and the offset of its bottom left corner from
    /// the origin, y upward.
    pub bbx: [i32; 4],
    /// `(w + 7) / 8` bytes per row from the top, leftmost pixel in bit 7.
    pub rows: Vec<u8>,
}

impl BdfGlyph {
    /// Whether the pixel `x` right of the origin and `y` above the
    /// baseline is set.
    fn ink(&self, x: i32, y: i32) -> bool {
        let [w, h, xoff, yoff] = self.bbx;
        let (col, row) = (x - xoff, yoff + h - 1 - y);
        if !(0..w).contains(&col) || !(0..h).contains(&row) {
            return false;
        }
        let stride = (w as usize).div_ceil(8);
        let byte = self.rows.get(row as usize * stride + col as usize / 8);
        byte.is_some_and(|b| b & (0x80 >> (col % 8)) != 0)
    }
}

pub struct Bdf {
    pub glyphs: Vec<BdfGlyph>,
    pub ascent: i32,
    pub descent: i32,
}

/// Reads a BDF font, skipping what it cannot read rather than failing:
/// a glyph with a broken bitmap row is drawn without that row.
pub fn parse_bdf(text: &str) -> Bdf {
    let mut bdf = Bdf {
        glyphs: Vec::new(),
        ascent: 0,
        descent: 0,
    };
    let (mut code, mut advance, mut bbx) = (None, 0, [0; 4]);
    let mut bitmap: Option<Vec<u8>> = None;
    for line in text.lines() {
        let mut words = line.split_whitespace();
        let Some(key) = words.next() else { continue };
        let mut num = || words.next().and_then(|w| w.parse::<i32>().ok());
        match key {
            "FONT_ASCENT" => bdf.ascent = num().unwrap_or(0),
            "FONT_DESCENT" => bdf.descent = num().unwrap_or(0),
            "STARTCHAR" => (code, advance, bbx) = (None, 0, [0; 4]),
            // -1 marks a glyph with no Unicode code.
            "ENCODING" => code = num().and_then(|n| u32::try_from(n).ok()),
            "DWIDTH" => advance = num().unwrap_or(0),
            "BBX" => bbx = [0; 4].map(|_| num().unwrap_or(0)),
            "BITMAP" => bitmap = Some(Vec::new()),
            "ENDCHAR" => {
                let rows = bitmap.take().unwrap_or_default();
                if let Some(code) = code
                    && bbx[0] > 0
                    && bbx[1] > 0
                {
                    bdf.glyphs.push(BdfGlyph {
                        code,
                        advance,
                        bbx,
                        rows,
                    });
                }
            }
            hex => {
                if let Some(rows) = &mut bitmap {
                    let stride = (bbx[0].max(0) as usize).div_ceil(8);
                    let mut row = vec![0; stride];
                    for (i, byte) in row.iter_mut().enumerate() {
                        *byte = hex
                            .get(i * 2..i * 2 + 2)
                            .and_then(|h| u8::from_str_radix(h, 16).ok())
                            .unwrap_or(0);
                    }
                    rows.extend_from_slice(&row);
                }
            }
        }
    }
    bdf
}

/// How a BDF font's glyphs are fitted to cells: the rows its letters
/// take and how wide its widest characters are.
pub struct BdfLayout {
    /// Rows above the baseline that capitals and CJK reach.
    top: i32,
    /// Rows below the baseline that descenders reach, as a negative y.
    bottom: i32,
    /// Advance of a full-width character.
    full: i32,
}

impl BdfLayout {
    /// Measures the font on a few reference characters, which BDF's own
    /// ascent and descent often overstate.
    pub fn new(bdf: &Bdf) -> BdfLayout {
        let find = |c: char| bdf.glyphs.iter().find(|g| g.code == c as u32);
        let tall: Vec<_> = "H가あ漢".chars().filter_map(find).collect();
        let deep: Vec<_> = "gjpy".chars().filter_map(find).collect();
        let top = match tall.iter().map(|g| g.bbx[3] + g.bbx[1]).max() {
            Some(top) => top,
            None => bdf.ascent,
        };
        let bottom = match deep.iter().map(|g| g.bbx[3]).min() {
            Some(bottom) => bottom.min(0),
            None if tall.is_empty() => -bdf.descent,
            None => tall.iter().map(|g| g.bbx[3]).min().unwrap_or(0).min(0),
        };
        let wide = "가あ漢Ｍ".chars().filter_map(find).map(|g| g.advance).max();
        let full = wide.unwrap_or_else(|| bdf.glyphs.iter().map(|g| g.advance).max().unwrap_or(0));
        BdfLayout {
            top: top.max(1),
            bottom,
            full: full.max(1),
        }
    }
}

/// Subsamples per pixel side when scaling a bitmap.
const SUBSAMPLES: i32 = 4;

/// A BDF glyph in a `cell`-pixel cell: scaled by whole steps while it
/// fits, or down to fit, and centered. Narrow glyphs are drawn twice as
/// wide, as SmileBASIC's font draws Latin as wide as kana.
pub fn bdf_cell(g: &BdfGlyph, layout: &BdfLayout, cell: u32) -> Vec<u8> {
    let cell_f = cell as f32;
    let height = layout.top - layout.bottom;
    let scale = match cell as i32 / height {
        0 => cell_f / height as f32,
        k => k as f32,
    };
    let width = g.advance.max(g.bbx[0] + g.bbx[2].max(0)).max(1);
    let stretch = if width * 2 <= layout.full { 2.0 } else { 1.0 };
    let sx = (scale * stretch).min(cell_f / width as f32);
    let x0 = ((cell_f - width as f32 * sx) / 2.0).round();
    let y0 = ((cell_f - height as f32 * scale) / 2.0).round();
    let mut out = vec![0; (cell * cell) as usize];
    for dy in 0..cell as i32 {
        for dx in 0..cell as i32 {
            let mut hits = 0;
            for sy in 0..SUBSAMPLES {
                for sxi in 0..SUBSAMPLES {
                    let px = dx as f32 + (sxi as f32 + 0.5) / SUBSAMPLES as f32;
                    let py = dy as f32 + (sy as f32 + 0.5) / SUBSAMPLES as f32;
                    let u = ((px - x0) / sx).floor() as i32;
                    let v = ((py - y0) / scale).floor() as i32;
                    if g.ink(u, layout.top - 1 - v) {
                        hits += 1;
                    }
                }
            }
            out[(dy as u32 * cell + dx as u32) as usize] =
                (hits * 255 / (SUBSAMPLES * SUBSAMPLES)) as u8;
        }
    }
    out
}

/// Share of the cell M PLUS 1p's em takes: SmileBASIC 4 draws its
/// 16-pixel font, drawn from that face, with a 15-pixel em.
pub const SMILEBASIC_EM: f32 = 15.0 / 16.0;
/// Where the baseline sits in the em, from its top: about where CJK
/// fonts put it.
const EM_BASELINE: f32 = 0.88;

/// A glyph of an outline font in a `cell`-pixel cell, `None` if the font
/// lacks it. Its em takes `em_share` of the cell; narrow glyphs are
/// widened up to twice as wide, and glyphs that stick out are moved in or
/// made smaller. The origin falls on whole pixels, which keeps pixel fonts
/// drawn as outlines sharp at their own size.
pub fn outline_cell(font: &impl Font, c: char, cell: u32, em_share: f32) -> Option<Vec<u8>> {
    let id = font.glyph_id(c);
    if id.0 == 0 {
        return None;
    }
    let cell_f = cell as f32;
    let em_units = font.units_per_em().unwrap_or(1000.0);
    let em = cell_f * em_share;
    let advance = font.h_advance_unscaled(id) * em / em_units;
    let stretch = match advance {
        a if a > 0.0 && a < em * 0.6 => (em * 0.8 / a).min(2.0),
        _ => 1.0,
    };
    // ab_glyph scales by ascent minus descent; one em should be `em`.
    let mut sy = em * font.height_unscaled() / em_units;
    let mut sx = sy * stretch;
    let mut origin = point(
        ((cell_f - advance * stretch) / 2.0).round(),
        ((cell_f - em) / 2.0 + em * EM_BASELINE).round(),
    );
    let glyph = |sx, sy, origin| id.with_scale_and_position(PxScale { x: sx, y: sy }, origin);
    let blank = vec![0; (cell * cell) as usize];
    // Spaces have a glyph but no outline.
    let Some(mut outline) = font.outline_glyph(glyph(sx, sy, origin)) else {
        return Some(blank);
    };
    let bounds = outline.px_bounds();
    let (w, h) = (bounds.width(), bounds.height());
    if w > cell_f || h > cell_f {
        let shrink = (cell_f / w).min(cell_f / h);
        (sx, sy) = (sx * shrink, sy * shrink);
        origin = point((origin.x * shrink).round(), (origin.y * shrink).round());
        outline = font.outline_glyph(glyph(sx, sy, origin))?;
    }
    let bounds = outline.px_bounds();
    let shift = |lo: f32, hi: f32| match (lo < 0.0, hi > cell_f) {
        (true, _) => -lo.floor(),
        (_, true) => cell_f - hi.ceil(),
        _ => 0.0,
    };
    let (dx, dy) = (
        shift(bounds.min.x, bounds.max.x),
        shift(bounds.min.y, bounds.max.y),
    );
    if dx != 0.0 || dy != 0.0 {
        origin = point(origin.x + dx, origin.y + dy);
        outline = font.outline_glyph(glyph(sx, sy, origin))?;
    }
    let min = outline.px_bounds().min;
    let mut out = blank;
    outline.draw(|x, y, coverage| {
        let (x, y) = (min.x as i32 + x as i32, min.y as i32 + y as i32);
        if (0..cell as i32).contains(&x) && (0..cell as i32).contains(&y) {
            out[(y as u32 * cell + x as u32) as usize] = (coverage.min(1.0) * 255.0).round() as u8;
        }
    });
    Some(out)
}
