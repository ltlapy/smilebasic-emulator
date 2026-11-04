//! Draws one text screen item on the CPU.

use std::collections::HashMap;

use crate::compose::{Mixed, Paint, Placement, draw_image_part, page_signs};
use crate::font::{Font, FontSize};
use crate::framebuffer::Framebuffer;
use crate::graphics::{Clip, Graphics};
use crate::scene::TextItem;
use crate::sprites::sample;
use crate::text::{Cell, FONT_PAGE, USER_CHARS, USER_GRID, attr, glyph_pixel};

/// Draws the screen, its cells being `cells`, within `clip`, with the INPUT
/// cursor on top if it has one.
pub fn draw(
    item: &TextItem,
    cells: &[Cell],
    font: &Font,
    grp: &Graphics,
    fb: &mut Framebuffer,
    clip: Option<&Clip>,
) {
    if item.empty {
        return;
    }
    let place = Placement {
        matrix: item.matrix,
        clip,
        add: item.add,
    };
    let cs = item.char_size;
    let (w, h) = ((item.width * cs) as i32, (item.height * cs) as i32);
    if place.is_upright() {
        item.draw_upright(cells, fb, font, grp, &place);
    } else {
        item.draw_turned(cells, fb, font, grp, &place);
    }
    if let Some(bar) = &item.cursor {
        let part = bar.part;
        let (x0, x1) = (f64::from(part.0), f64::from(part.0) + f64::from(bar.width));
        let paint = Mixed::Solid(bar.color);
        let (y0, y1) = (f64::from(part.1), f64::from(part.3));
        draw_image_part(fb, &place, w, h, part, |x, y| {
            (x >= x0 && x < x1 && y >= y0 && y < y1).then_some(paint)
        });
    }
}

impl TextItem {
    /// The pixel at `(x, y)` of a character's `font_type`-sized image, or just
    /// off it, which a tie may read: the page goes on there.
    fn glyph_pixel(&self, font: &Font, grp: &Graphics, code: u16, x: i32, y: i32) -> u32 {
        let ft = self.font_type;
        if USER_CHARS.contains(&code) {
            let i = i32::from(code - USER_CHARS.start());
            let (ox, oy) = self.page_origin;
            let size = ft as i32;
            let gx = ox + i % USER_GRID * size + x;
            let gy = oy + i / USER_GRID * size + y;
            return sample(grp, self.page, gx, gy);
        }
        let on_image = |v: i32| (0..ft as i32).contains(&v);
        if !on_image(x) || !on_image(y) {
            let size = match ft {
                8 => FontSize::Small,
                16 => FontSize::Large,
                _ => return 0,
            };
            return font.origin(code, size).map_or(0, |(gx, gy)| {
                grp.pixel(FONT_PAGE, gx as i32 + x, gy as i32 + y)
            });
        }
        let (x, y) = (x as u32, y as u32);
        match ft {
            8 => glyph_pixel(font, grp, code, FontSize::Small, x, y),
            16 => glyph_pixel(font, grp, code, FontSize::Large, x, y),
            // Larger font types show only user-defined characters.
            _ => 0,
        }
    }

    /// [`draw`] for a screen turned or sheared on the frame, cell by
    /// cell.
    fn draw_turned(
        &self,
        cells: &[Cell],
        fb: &mut Framebuffer,
        font: &Font,
        grp: &Graphics,
        place: &Placement,
    ) {
        let (cs, ft) = (self.char_size, self.font_type);
        let (w, h) = ((self.width * cs) as i32, (self.height * cs) as i32);
        let size = f64::from(cs);
        let side = (cs as i32, cs as i32);
        let (up, down) = (
            place.texel_tie(side, (1.0, 1.0)),
            place.texel_tie(side, (-1.0, -1.0)),
        );
        // At a quarter turn a tie can read just off the cell, so what the
        // character's image holds goes one texel further each way.
        let margin = i32::from(place.is_right_angle());
        let stride = ft as i32 + 2 * margin;
        // The cell's character in its colors, looked up once rather than
        // for every pixel it covers.
        let mut glyph = vec![0; (stride * stride) as usize];
        let mut paints = Vec::new();
        for cy in 0..self.height {
            for cx in 0..self.width {
                let cell = cells[(cy * self.width + cx) as usize];
                // Empty cells show nothing, so need no drawing.
                if cell.code == 0 && (!self.may_invert || cell.attr & attr::INVERT == 0) {
                    continue;
                }
                paints.clear();
                self.glyph(font, grp, cell, margin, &mut glyph, &mut paints);
                let (x0, y0) = ((cx * cs) as i32, (cy * cs) as i32);
                let part = (x0, y0, x0 + cs as i32, y0 + cs as i32);
                let page = page_signs(
                    i32::from(cell.attr & attr::ROTATION),
                    cell.attr & attr::REVH != 0,
                    cell.attr & attr::REVV != 0,
                );
                let tx = if page.0 > 0.0 { up.0 } else { down.0 };
                let ty = if page.1 > 0.0 { up.1 } else { down.1 };
                draw_image_part(fb, place, w, h, part, |x, y| {
                    // Pixels of the cells around are theirs to draw.
                    if ((x / size) as u32, (y / size) as u32) != (cx, cy) {
                        return None;
                    }
                    // Where in the cell, which a fine frame splits further.
                    // As for sprites, the character is flipped, then turned
                    // (checked on hardware): undo the turn, then the flips.
                    let (px, py) = (x + tx - f64::from(cx * cs), y + ty - f64::from(cy * cs));
                    let (mut px, mut py) = match cell.attr & attr::ROTATION {
                        0 => (px, py),
                        1 => (py, size - px),
                        2 => (size - px, size - py),
                        _ => (size - py, px),
                    };
                    if cell.attr & attr::REVH != 0 {
                        px = size - px;
                    }
                    if cell.attr & attr::REVV != 0 {
                        py = size - py;
                    }
                    let at = |p: f64| {
                        let t = p * f64::from(ft) / size;
                        let t = t as i32 - i32::from(t < 0.0);
                        t.clamp(-margin, ft as i32 - 1 + margin) + margin
                    };
                    paint_at(glyph[(at(py) * stride + at(px)) as usize], &paints)
                });
            }
        }
    }

    /// A cell's character in its colors, `ft` x `ft` and `margin` further each
    /// way, as [`draw`] works it out: what each pixel puts on the frame, as
    /// [`glyph_paint`] gives it, with the paints that are not opaque pushed onto
    /// `paints`.
    fn glyph(
        &self,
        font: &Font,
        grp: &Graphics,
        cell: Cell,
        margin: i32,
        out: &mut [u32],
        paints: &mut Vec<Mixed>,
    ) {
        let inverted = self.inverts && cell.attr & attr::INVERT != 0;
        let count = out.len();
        self.each_texel(font, grp, cell.code, inverted, margin, count, |i, p| {
            let p = if inverted { inverted_texel(p) } else { p };
            let paint = Mixed::new(p, cell.color, self.tint, self.add);
            out[i] = glyph_paint(paint, paints);
        });
    }

    /// What an inverted cell's character is, `font_type` square, before its
    /// colors: for a backend, which multiplies those itself.
    pub fn inverted_glyph(&self, font: &Font, grp: &Graphics, code: u16, out: &mut [u32]) {
        let count = out.len();
        self.each_texel(font, grp, code, true, 0, count, |i, p| {
            out[i] = inverted_texel(p);
        });
    }

    /// Each of the `count` pixels of a character's image and `margin` around
    /// it, row by row, as what it is before its colors.
    #[inline]
    #[allow(clippy::too_many_arguments)]
    fn each_texel(
        &self,
        font: &Font,
        grp: &Graphics,
        code: u16,
        inverted: bool,
        margin: i32,
        count: usize,
        mut each: impl FnMut(usize, u32),
    ) {
        let ft = self.font_type;
        // Where the character comes from is looked up once, not for
        // every pixel; the pixels are those glyph_pixel gives.
        let stride = ft as i32 + 2 * margin;
        let mut fill = |pixel: &dyn Fn(i32, i32) -> u32| {
            for i in 0..count as i32 {
                each(i as usize, pixel(i % stride - margin, i / stride - margin));
            }
        };
        let size = match ft {
            8 => Some(FontSize::Small),
            16 => Some(FontSize::Large),
            _ => None,
        };
        match (code, size) {
            // An inverted one shows SmileBASIC 4's font.map entry for it,
            // which the bundled font lacks.
            (0, _) if !inverted || !font.is_smilebasic() => fill(&|_, _| 0),
            (code, _) if USER_CHARS.contains(&code) => {
                fill(&|x, y| self.glyph_pixel(font, grp, code, x, y))
            }
            (code, Some(size)) => match font.origin(code, size) {
                Some((gx, gy)) => fill(&|x, y| grp.pixel(FONT_PAGE, gx as i32 + x, gy as i32 + y)),
                None => fill(&|x, y| {
                    let on_image = |v: i32| (0..ft as i32).contains(&v);
                    match on_image(x) && on_image(y) {
                        true => font.pixel(code, size, x as u32, y as u32),
                        false => 0,
                    }
                }),
            },
            // Larger font types show only user-defined characters, and
            // the font page's corner to what is inverted.
            // UNVERIFIED: that corner.
            (_, None) if inverted => fill(&|x, y| grp.pixel(FONT_PAGE, x, y)),
            (_, None) => fill(&|_, _| 0),
        }
    }

    /// [`draw`] for a screen neither turned nor sheared on the frame:
    /// which cell and which point of it each frame column and row shows
    /// is worked out once, and each row of cells' characters once.
    fn draw_upright(
        &self,
        cells: &[Cell],
        fb: &mut Framebuffer,
        font: &Font,
        grp: &Graphics,
        place: &Placement,
    ) {
        let (cs, ft) = (self.char_size, self.font_type);
        let (w, h) = ((self.width * cs) as i32, (self.height * cs) as i32);
        let Some(grid) = crate::compose::grid(fb, place, w, h) else {
            return;
        };
        let size = f64::from(cs);
        let side = (cs as i32, cs as i32);
        // Page coordinates running up and down the frame's axis settle a tie
        // on different sides of a point.
        let up = place.texel_tie(side, (1.0, 1.0));
        let down = place.texel_tie(side, (-1.0, -1.0));
        let at = |p: f64| {
            let t = p * f64::from(ft) / size;
            t as i32 - i32::from(t < 0.0)
        };
        // For each column (or row): its cell, and the glyph column (or
        // row) for the point in the cell, for the point flipped, and for
        // it flipped twice, as [`draw`]'s arithmetic gives them.
        let axis = |v: f64, up: f64, down: f64| {
            let c = (v / size) as u32;
            let from = f64::from(c * cs);
            let (rising, falling) = (at(v + up - from), at(size - (v + down - from)));
            (c, [rising, falling, rising])
        };
        let mut cols: Vec<(u32, [i32; 3])> =
            grid.xs.iter().map(|&x| axis(x, up.0, down.0)).collect();
        // A tie just off a cell reads what lies beside it on the page: only
        // then does a character's image need more than itself.
        let off = |t: &[i32; 3]| t.iter().any(|&v| !(0..ft as i32).contains(&v));
        let margin = i32::from(
            cols.iter().any(|(_, t)| off(t))
                || (grid.ys.iter()).any(|&y| off(&axis(y, up.1, down.1).1)),
        );
        let stride = ft as i32 + 2 * margin;
        if margin != 0 {
            for t in cols.iter_mut().flat_map(|(_, t)| t) {
                *t += margin;
            }
        }
        // Which of those each glyph coordinate comes from, by ATTR: the
        // column's (false) or the row's (true) table, and which entry.
        let pick = |a: u16| -> ((bool, usize), (bool, usize)) {
            let (mut p, mut q) = match a & attr::ROTATION {
                0 => ((false, 0), (true, 0)),
                1 => ((true, 0), (false, 1)),
                2 => ((false, 1), (true, 1)),
                _ => ((true, 1), (false, 0)),
            };
            if a & attr::REVH != 0 {
                p.1 += 1;
            }
            if a & attr::REVV != 0 {
                q.1 += 1;
            }
            (p, q)
        };
        let glyph_len = (stride * stride) as usize;
        // Each character in each color once, however many cells show it.
        let mut glyphs: Vec<u32> = Vec::new();
        let mut paints: Vec<Mixed> = Vec::new();
        let mut made: HashMap<(u16, u32), usize> = HashMap::new();
        let mut made_inverted: HashMap<(u16, u32), usize> = HashMap::new();
        let may_invert = self.may_invert;
        // Per cell of the row, if it shows anything: its picks and where
        // its character is in `glyphs`.
        let mut shown = vec![None; self.width as usize];
        let mut row_of = None;
        let width = fb.width() as usize;
        let pixels = fb.pixels_mut();
        for (fy, &y) in (grid.y0 as usize..).zip(&grid.ys) {
            let (cy, mut rat) = axis(y, up.1, down.1);
            if margin != 0 {
                rat = rat.map(|t| t + margin);
            }
            if row_of != Some(cy) {
                row_of = Some(cy);
                for cx in 0..self.width {
                    let cell = cells[(cy * self.width + cx) as usize];
                    let shows = cell.code != 0 || (may_invert && cell.attr & attr::INVERT != 0);
                    shown[cx as usize] = shows.then(|| {
                        let key = (cell.code, cell.color);
                        let made = match cell.attr & attr::INVERT {
                            0 => &mut made,
                            _ => &mut made_inverted,
                        };
                        let at = *made.entry(key).or_insert_with(|| {
                            let at = glyphs.len();
                            glyphs.resize(at + glyph_len, 0);
                            let out = &mut glyphs[at..];
                            self.glyph(font, grp, cell, margin, out, &mut paints);
                            at
                        });
                        (pick(cell.attr), at)
                    });
                }
            }
            let line = &mut pixels[fy * width + grid.x0 as usize..][..cols.len()];
            for (dst, &(cx, cat)) in line.iter_mut().zip(&cols) {
                let Some(((p, q), at)) = shown[cx as usize] else {
                    continue;
                };
                let coord = |(row, k): (bool, usize)| if row { rat[k] } else { cat[k] };
                match glyphs[at + (coord(q) * stride + coord(p)) as usize] {
                    0 => {}
                    solid if solid >> 24 == 0xFF => *dst = solid,
                    n => *dst = paints[n as usize - 1].put(*dst),
                }
            }
        }
    }
}

/// The texel an inverted cell draws with for a glyph's `p`, multiplied by
/// the cell's color and TCOLOR as any other: SmileBASIC 4 takes the
/// colors times `1 - alpha` in place of the glyph's, and then multiplies
/// the colour by alpha as always, which is why the light is left squared.
// UNVERIFIED: the sum is read from the console's program, not yet seen.
fn inverted_texel(p: u32) -> u32 {
    (255 - (p >> 24)) * 0x0101_0101
}

/// What a glyph pixel puts on the frame, as one `u32`: 0 for nothing, an
/// opaque color for itself, or else the place of its paint in `paints`
/// plus one, which is never opaque.
fn glyph_paint(paint: Option<Mixed>, paints: &mut Vec<Mixed>) -> u32 {
    match paint {
        None => 0,
        Some(Mixed::Solid(c)) => c,
        Some(paint) => {
            paints.push(paint);
            // Memory runs out long before this reaches &HFF000000.
            paints.len() as u32
        }
    }
}

/// Back from [`glyph_paint`].
fn paint_at(v: u32, paints: &[Mixed]) -> Option<Mixed> {
    match v {
        0 => None,
        solid if solid >> 24 == 0xFF => Some(Mixed::Solid(solid)),
        n => Some(paints[n as usize - 1]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The exact sum, one rounding: `(c*n)*(a*n) + d*(1 - a*n)` with n the
    /// glyph's 1 - alpha, tints unrounded.
    #[test]
    fn inverted_texels_keep_the_exact_sum() {
        let n = 0x80_u32;
        let texel = inverted_texel(((255 - n) << 24) | 0x12_3456);
        assert_eq!(texel, 0x8080_8080);
        let (c, ca, tc, d) = (0xC0_u32, 0x90_u32, 0xE0_u32, 0x40_u32);
        let paint = Mixed::new(
            texel,
            (ca << 24) | (c * 0x1_0101),
            0xFF00_0000 | (tc * 0x1_0101),
            false,
        );
        let got = paint.unwrap().put(0xFF00_0000 | (d * 0x1_0101)) & 0xFF;
        let f = |v: u32| f64::from(v) / 255.0;
        let a = f(ca) * f(n);
        let want = f(c) * f(n) * f(tc) * a + f(d) * (1.0 - a);
        assert_eq!(got, (want * 255.0).round() as u32);
    }
}
