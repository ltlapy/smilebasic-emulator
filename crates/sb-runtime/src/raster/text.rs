//! Draws one text screen item on the CPU.

use std::collections::HashMap;
use std::hash::{BuildHasherDefault, Hasher};

use crate::compose::{Mixed, Paint, Placement, draw_image_part, draw_texels, page_signs};
use crate::font::{Font, FontSize};
use crate::framebuffer::Framebuffer;
use crate::graphics::{Clip, Graphics, PAGES};
use crate::scene::TextItem;
use crate::sprites::sample;
use crate::text::{Cell, FONT_PAGE, USER_CHARS, UserPage, attr, glyph_pixel};

/// A cell that shows nothing, where a cell's place in [`Glyphs`] is kept.
const NO_GLYPH: u32 = u32::MAX;

/// A hash for [`Glyphs`], looked up about once a cell: it need not resist
/// anything, as a program can at worst slow its own text.
#[derive(Clone, Copy, Default)]
struct KeyHasher(u64);

impl Hasher for KeyHasher {
    fn write(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.write_u64(u64::from(b));
        }
    }

    fn write_u64(&mut self, key: u64) {
        let h = (self.0 ^ key).wrapping_mul(0x9E37_79B9_7F4A_7C15);
        self.0 = h ^ h >> 32;
    }

    fn finish(&self) -> u64 {
        self.0
    }
}

/// Each character in each color worked out once, however many cells of a
/// screen show it.
#[derive(Default)]
struct Glyphs {
    /// The characters, as [`TextItem::glyph`] gives them.
    data: Vec<u32>,
    paints: Vec<Mixed>,
    made: HashMap<u64, usize, BuildHasherDefault<KeyHasher>>,
    /// The last one looked up: cells in a row are often alike.
    last: Option<(u64, usize)>,
}

/// What a screen's characters look like besides each cell's code, color and
/// inversion: as long as this stands, those worked out stay right.
#[derive(Clone, Copy, PartialEq)]
struct GlyphsAre {
    font_type: u32,
    margin: i32,
    tint: u32,
    add: bool,
    inverts: bool,
    /// TPAGE, and how the page is read.
    page: i32,
    page_origin: (i32, i32),
    page_sample: i32,
    /// A count no other [`Graphics`] has, so that this also tells another
    /// program's pages, and with them another font, from these.
    page_changes: u64,
    font_changes: u64,
}

/// Words of glyph data above which a screen's are made afresh, between frames:
/// 4 MiB, 16000 of the 8-pixel characters, so that a full screen of them in
/// a color for each row stays, as a scrolling one is.
const MOST_KEPT: usize = 1 << 20;

/// A screen's characters, kept from frame to frame: only what the program has
/// not shown yet costs anything to make.
#[derive(Default)]
pub struct KeptGlyphs {
    are: Option<GlyphsAre>,
    glyphs: Glyphs,
}

impl KeptGlyphs {
    /// The characters for `item` with `margin`, emptied of those made under
    /// anything else.
    fn ready(&mut self, item: &TextItem, grp: &Graphics, margin: i32) -> &mut Glyphs {
        let page = usize::try_from(item.page).ok().filter(|&p| p < PAGES);
        let are = GlyphsAre {
            font_type: item.font_type,
            margin,
            tint: item.tint,
            add: item.add,
            inverts: item.inverts,
            page: item.page,
            page_origin: item.page_origin,
            page_sample: page.map_or(0, |p| grp.sample[p]),
            page_changes: page.map_or(0, |p| grp.changes(p)),
            font_changes: grp.changes(FONT_PAGE),
        };
        if self.are != Some(are) || self.glyphs.data.len() > MOST_KEPT {
            self.are = Some(are);
            self.glyphs.data.clear();
            self.glyphs.paints.clear();
            self.glyphs.made.clear();
            self.glyphs.last = None;
        }
        &mut self.glyphs
    }
}

impl Glyphs {
    /// Where the cell's character is in `data`.
    fn at(
        &mut self,
        item: &TextItem,
        font: &Font,
        grp: &Graphics,
        cell: Cell,
        margin: i32,
        len: usize,
    ) -> usize {
        let inverted = u64::from(cell.attr & attr::INVERT != 0);
        let key = inverted << 48 | u64::from(cell.code) << 32 | u64::from(cell.color);
        if let Some((_, at)) = self.last.filter(|l| l.0 == key) {
            return at;
        }
        let Glyphs {
            data, paints, made, ..
        } = self;
        let at = *made.entry(key).or_insert_with(|| {
            let at = data.len();
            data.resize(at + len, 0);
            item.glyph(font, grp, cell, margin, &mut data[at..], paints);
            at
        });
        self.last = Some((key, at));
        at
    }
}

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
    let mut kept = KeptGlyphs::default();
    draw_keeping(item, cells, font, grp, fb, clip, &mut kept);
}

/// [`draw`], with this screen's characters from the last frame in `kept`.
pub fn draw_keeping(
    item: &TextItem,
    cells: &[Cell],
    font: &Font,
    grp: &Graphics,
    fb: &mut Framebuffer,
    clip: Option<&Clip>,
    kept: &mut KeptGlyphs,
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
        item.draw_upright(cells, fb, font, grp, &place, kept);
    } else {
        item.draw_turned(cells, fb, font, grp, &place, kept);
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
            let user = UserPage {
                page: self.page,
                origin: self.page_origin,
            };
            let (cx, cy) = user.cell(code, ft as i32);
            return sample(grp, self.page, cx + x, cy + y);
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

    /// [`draw`] for a screen turned or sheared on the frame. Each frame pixel
    /// lies in one cell, so the order of the cells does not matter: a screen
    /// that is mostly full is gone over once, and otherwise each cell that
    /// shows anything for itself.
    fn draw_turned(
        &self,
        cells: &[Cell],
        fb: &mut Framebuffer,
        font: &Font,
        grp: &Graphics,
        place: &Placement,
        kept: &mut KeptGlyphs,
    ) {
        let (cs, ft) = (self.char_size, self.font_type);
        let (w, h) = ((self.width * cs) as i32, (self.height * cs) as i32);
        let size = f64::from(cs);
        let side = (cs as i32, cs as i32);
        let (up, down) = (
            place.texel_tie(side, (1.0, 1.0)),
            place.texel_tie(side, (-1.0, -1.0)),
        );
        // What a tie moves a point by, for each turn and flip.
        let ties: [(f64, f64); 16] = std::array::from_fn(|a| {
            let a = a as u16;
            let page = page_signs(
                i32::from(a & attr::ROTATION),
                a & attr::REVH != 0,
                a & attr::REVV != 0,
            );
            (
                if page.0 > 0.0 { up.0 } else { down.0 },
                if page.1 > 0.0 { up.1 } else { down.1 },
            )
        });
        // At a quarter turn a tie can read just off the cell, so what the
        // character's image holds goes one texel further each way.
        let margin = i32::from(place.is_right_angle());
        let stride = ft as i32 + 2 * margin;
        let glyph_len = (stride * stride) as usize;
        let glyphs = kept.ready(self, grp, margin);
        // Per cell, where its character is in `glyphs`, each character in
        // each color being looked up once however many cells show it, times 16,
        // and its turn and flips.
        let count = (self.width * self.height) as usize;
        let mut shown = Vec::with_capacity(count);
        let (mut filled, mut turning) = (0, false);
        for cell in &cells[..count] {
            // Empty cells show nothing, so need no drawing.
            if cell.code == 0 && (!self.may_invert || cell.attr & attr::INVERT == 0) {
                shown.push(NO_GLYPH);
                continue;
            }
            filled += 1;
            turning |= cell.attr & 15 != 0;
            let at = glyphs.at(self, font, grp, *cell, margin, glyph_len) as u32;
            shown.push(at << 4 | u32::from(cell.attr & 15));
        }
        let turned = Turned {
            shown: &shown,
            data: &glyphs.data,
            paints: &glyphs.paints,
            ties,
            width: self.width,
            cs,
            ft: ft as i32,
            size,
            simple: cs.is_power_of_two()
                && ft.is_power_of_two()
                && (up, down) == ((0.0, 0.0), (0.0, 0.0))
                && margin == 0,
            glyph_scale: f64::from(ft) / size,
            ft_shift: ft.trailing_zeros(),
            margin,
            stride,
        };
        if filled * 2 >= count {
            if turned.simple && cs == ft && !turning {
                return draw_texels(fb, place, w, h, (0.0, 0.0), |x, y| {
                    turned.plain(x as u32, y as u32)
                });
            }
            draw_image_part(fb, place, w, h, (0, 0, w, h), |x, y| {
                turned.pixel(x, y, None)
            });
            return;
        }
        for (i, &glyph) in shown.iter().enumerate() {
            if glyph == NO_GLYPH {
                continue;
            }
            let (cx, cy) = (i as u32 % self.width, i as u32 / self.width);
            let (x0, y0) = ((cx * cs) as i32, (cy * cs) as i32);
            let part = (x0, y0, x0 + cs as i32, y0 + cs as i32);
            draw_image_part(fb, place, w, h, part, |x, y| {
                turned.pixel(x, y, Some((cx, cy)))
            });
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
        // Most characters have two colors, clear and the one, so the last
        // texel's paint usually does for this one.
        let mut last = None;
        self.each_texel(font, grp, cell.code, inverted, margin, count, |i, p| {
            out[i] = match last {
                Some((before, put)) if before == p => put,
                _ => {
                    let texel = if inverted { inverted_texel(p) } else { p };
                    let paint = Mixed::new(texel, cell.color, self.tint, self.add);
                    let put = glyph_paint(paint, paints);
                    last = Some((p, put));
                    put
                }
            };
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
        kept: &mut KeptGlyphs,
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
        let glyphs = kept.ready(self, grp, margin);
        let may_invert = self.may_invert;
        // Per cell of the row, if it shows anything: its picks and where
        // its character is in `glyphs`.
        let mut shown = vec![None; self.width as usize];
        let (mut row_of, mut row_shows) = (None, false);
        // The cell each run of frame columns lies in, and where the run starts
        // and ends among them: a cell with nothing takes one test.
        let mut spans: Vec<(u32, u32, u32)> = Vec::new();
        for (i, &(cx, _)) in cols.iter().enumerate() {
            match spans.last_mut() {
                Some(run) if run.0 == cx => run.2 = i as u32 + 1,
                _ => spans.push((cx, i as u32, i as u32 + 1)),
            }
        }
        let width = fb.width() as usize;
        let pixels = fb.pixels_mut();
        for (fy, &y) in (grid.y0 as usize..).zip(&grid.ys) {
            let (cy, mut rat) = axis(y, up.1, down.1);
            if margin != 0 {
                rat = rat.map(|t| t + margin);
            }
            if row_of != Some(cy) {
                row_of = Some(cy);
                row_shows = false;
                for cx in 0..self.width {
                    let cell = cells[(cy * self.width + cx) as usize];
                    let shows = cell.code != 0 || (may_invert && cell.attr & attr::INVERT != 0);
                    row_shows |= shows;
                    shown[cx as usize] = shows.then(|| {
                        let at = glyphs.at(self, font, grp, cell, margin, glyph_len);
                        (pick(cell.attr), at)
                    });
                }
            }
            if !row_shows {
                continue;
            }
            let line = &mut pixels[fy * width + grid.x0 as usize..][..cols.len()];
            let Glyphs { data, paints, .. } = &*glyphs;
            for &(cx, start, end) in &spans {
                let Some(((p, q), at)) = shown[cx as usize] else {
                    continue;
                };
                let (start, end) = (start as usize, end as usize);
                let (line, cols) = (&mut line[start..end], &cols[start..end]);
                let put = |dst: &mut u32, texel: i32| match data[at + texel as usize] {
                    0 => {}
                    solid if solid >> 24 == 0xFF => *dst = solid,
                    n => *dst = paints[n as usize - 1].put(*dst),
                };
                // Not turned a quarter: the row of the character is the
                // same all along the cell.
                if let ((false, k), (true, r)) = (p, q) {
                    let row = rat[r] * stride;
                    for (dst, &(_, cat)) in line.iter_mut().zip(cols) {
                        put(dst, row + cat[k]);
                    }
                    continue;
                }
                let coord = |(row, k): (bool, usize), cat: &[i32; 3]| match row {
                    true => rat[k],
                    false => cat[k],
                };
                for (dst, (_, cat)) in line.iter_mut().zip(cols) {
                    put(dst, coord(q, cat) * stride + coord(p, cat));
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

/// What [`glyph_paint`] gave, as a paint.
#[derive(Clone, Copy)]
enum Texel<'a> {
    Solid(u32),
    Paint(&'a Mixed),
}

impl Paint for Texel<'_> {
    #[inline]
    fn put(&self, dst: u32) -> u32 {
        match self {
            Texel::Solid(c) => *c,
            Texel::Paint(paint) => paint.put(dst),
        }
    }
}

/// Back from [`glyph_paint`].
#[inline]
fn paint_at(v: u32, paints: &[Mixed]) -> Option<Texel<'_>> {
    match v {
        0 => None,
        solid if solid >> 24 == 0xFF => Some(Texel::Solid(solid)),
        n => Some(Texel::Paint(&paints[n as usize - 1])),
    }
}

/// What [`TextItem::draw_turned`] works each pixel from.
struct Turned<'a> {
    /// Per cell, where its character is in `data`, times 16, and its turn and
    /// flips; or [`NO_GLYPH`].
    shown: &'a [u32],
    data: &'a [u32],
    paints: &'a [Mixed],
    /// What a tie moves a point by, for each turn and flip.
    ties: [(f64, f64); 16],
    /// In cells.
    width: u32,
    cs: u32,
    ft: i32,
    size: f64,
    /// Nothing can tie and the sizes are powers of two, so that sums on a point
    /// are exact.
    simple: bool,
    /// Character pixels to a screen pixel.
    glyph_scale: f64,
    /// The base 2 logarithm of `ft`, when `simple`.
    ft_shift: u32,
    margin: i32,
    stride: i32,
}

impl Turned<'_> {
    /// The pixel at a point of the screen, on it, if it lies in the cell
    /// `only`, when one is given.
    #[inline(always)]
    fn pixel(&self, x: f64, y: f64, only: Option<(u32, u32)>) -> Option<Texel<'_>> {
        if !self.simple {
            return self.pixel_at(x, y, only);
        }
        // With nothing to settle, a point's character pixel counted from the
        // screen's corner says it all. A branch between the two axes keeps the
        // compiler from working them side by side, at twice the cost.
        let gx = (x * self.glyph_scale) as u32;
        if gx >> self.ft_shift >= self.width {
            return None;
        }
        let gy = (y * self.glyph_scale) as u32;
        let (cx, cy) = (gx >> self.ft_shift, gy >> self.ft_shift);
        // Pixels of the cells around are theirs to draw.
        if only.is_some_and(|c| c != (cx, cy)) {
            return None;
        }
        let glyph = *self.shown.get((cy * self.width + cx) as usize)?;
        if glyph == NO_GLYPH {
            return None;
        }
        if glyph & 15 != 0 {
            return self.pixel_at(x, y, only);
        }
        self.texel(glyph, gx, gy)
    }

    /// [`Turned::pixel`] for a whole character pixel, counted from the screen's
    /// corner, when `simple` and no cell has a turn or flip.
    #[inline(always)]
    fn plain(&self, gx: u32, gy: u32) -> Option<Texel<'_>> {
        let at = (gy >> self.ft_shift) * self.width + (gx >> self.ft_shift);
        let glyph = self.shown[at as usize];
        if glyph == NO_GLYPH {
            return None;
        }
        self.texel(glyph, gx, gy)
    }

    /// The pixel `(gx, gy)` of what `shown` holds for its cell.
    #[inline(always)]
    fn texel(&self, glyph: u32, gx: u32, gy: u32) -> Option<Texel<'_>> {
        let mask = self.ft as u32 - 1;
        let texel = ((gy & mask) * self.ft as u32 + (gx & mask)) as usize;
        paint_at(self.data[(glyph >> 4) as usize + texel], self.paints)
    }

    /// [`Turned::pixel`] whatever the character's turn and flips, the sizes
    /// and the ties.
    #[inline(never)]
    fn pixel_at(&self, x: f64, y: f64, only: Option<(u32, u32)>) -> Option<Texel<'_>> {
        let (cx, cy) = ((x / self.size) as u32, (y / self.size) as u32);
        // Pixels of the cells around are theirs to draw.
        if only.is_some_and(|c| c != (cx, cy)) || cx >= self.width {
            return None;
        }
        let glyph = *self.shown.get((cy * self.width + cx) as usize)?;
        if glyph == NO_GLYPH {
            return None;
        }
        let a = (glyph & 15) as u16;
        let (tx, ty) = self.ties[usize::from(a)];
        let (cs, size) = (self.cs, self.size);
        // Where in the cell, which a fine frame splits further.
        // As for sprites, the character is flipped, then turned
        // (checked on hardware): undo the turn, then the flips.
        let (px, py) = (x + tx - f64::from(cx * cs), y + ty - f64::from(cy * cs));
        let (mut px, mut py) = match a & attr::ROTATION {
            0 => (px, py),
            1 => (py, size - px),
            2 => (size - px, size - py),
            _ => (size - py, px),
        };
        if a & attr::REVH != 0 {
            px = size - px;
        }
        if a & attr::REVV != 0 {
            py = size - py;
        }
        let at = |p: f64| {
            let t = p * f64::from(self.ft) / size;
            let t = t as i32 - i32::from(t < 0.0);
            t.clamp(-self.margin, self.ft - 1 + self.margin) + self.margin
        };
        let texel = (at(py) * self.stride + at(px)) as usize;
        paint_at(self.data[(glyph >> 4) as usize + texel], self.paints)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compose::Affine;
    use crate::graphics::Blend;
    use crate::text::TextScreen;

    /// Draws the screen with what was kept and afresh, which must agree.
    fn agree(
        s: &TextScreen,
        item: &TextItem,
        font: &Font,
        grp: &Graphics,
        kept: &mut KeptGlyphs,
    ) -> Vec<u32> {
        let frame = || {
            let mut fb = Framebuffer::new(64, 32);
            fb.clear(0xFF10_2030);
            fb
        };
        let (mut with_kept, mut fresh) = (frame(), frame());
        draw_keeping(item, s.cells(), font, grp, &mut with_kept, None, kept);
        draw(item, s.cells(), font, grp, &mut fresh, None);
        assert_eq!(with_kept.pixels(), fresh.pixels());
        fresh.pixels().to_vec()
    }

    fn fill(grp: &mut Graphics, page: usize, x: i32, y: i32, color: u32) {
        grp.target = page;
        grp.fill(x, y, x + 15, y + 15, color, Blend::Normal);
    }

    #[test]
    fn kept_glyphs_go_when_what_they_show_changes() {
        let mut font = Font::blank();
        let (gx, gy) = font.origin(0x41, FontSize::Small).unwrap();
        let mut grp = Graphics::default();
        fill(&mut grp, FONT_PAGE, gx as i32, gy as i32, 0xFFFF_FFFF);
        font.page_mut()[0] = 0;
        let mut s = TextScreen::new(64, 32);
        s.print(&[0x41, 0x41]);
        s.print(&[*USER_CHARS.start()]);
        fill(&mut grp, 1, 0, 0, 0xFF00_FF00);
        let mut item = s.item(0, &Affine::IDENTITY, None);
        (item.page, item.page_origin) = (1, (0, 0));
        let mut kept = KeptGlyphs::default();
        let first = agree(&s, &item, &font, &grp, &mut kept);
        // Nothing changed: kept, as the same data shows.
        let held = kept.glyphs.data.len();
        assert!(held > 0);
        assert_eq!(agree(&s, &item, &font, &grp, &mut kept), first);
        assert_eq!(kept.glyphs.data.len(), held);
        let mut last = first;
        let mut changed = |what: &str, item: &TextItem, grp: &Graphics, kept: &mut KeptGlyphs| {
            let now = agree(&s, item, &font, grp, kept);
            assert_ne!(now, last, "{what}");
            last = now;
        };
        // The font page.
        fill(&mut grp, FONT_PAGE, gx as i32, gy as i32, 0xFFFF_0000);
        changed("font page", &item, &grp, &mut kept);
        // The page of user-defined characters.
        fill(&mut grp, 1, 0, 0, 0xFF00_00FF);
        changed("user page", &item, &grp, &mut kept);
        // TPAGE: another page, and another place.
        fill(&mut grp, 2, 0, 0, 0xFFFF_FF00);
        item.page = 2;
        changed("TPAGE", &item, &grp, &mut kept);
        fill(&mut grp, 2, 16, 0, 0xFF80_8080);
        item.page_origin = (16, 0);
        changed("TPAGE's origin", &item, &grp, &mut kept);
        // TCOLOR, TBLEND.
        item.tint = 0xFF80_FFFF;
        changed("TCOLOR", &item, &grp, &mut kept);
        item.add = true;
        changed("TBLEND", &item, &grp, &mut kept);
        item.add = false;
        changed("TBLEND back", &item, &grp, &mut kept);
        // Another size of character.
        item.font_type = 16;
        changed("font type", &item, &grp, &mut kept);
        item.font_type = 8;
        changed("font type back", &item, &grp, &mut kept);
        // Another program's pages, which count their changes apart.
        let mut other = Graphics::default();
        fill(&mut other, FONT_PAGE, gx as i32, gy as i32, 0xFF00_FFFF);
        changed("another program", &item, &other, &mut kept);
    }

    #[test]
    fn kept_glyphs_follow_gsample() {
        let font = Font::blank();
        let mut grp = Graphics::default();
        // What reads across the page's edge wraps only under GSAMPLE 1.
        fill(&mut grp, 1, 0, 0, 0xFF00_FF00);
        let mut s = TextScreen::new(64, 32);
        s.print(&[*USER_CHARS.start()]);
        let mut item = s.item(0, &Affine::IDENTITY, None);
        (item.page, item.page_origin) = (1, (2044, 0));
        let mut kept = KeptGlyphs::default();
        let before = agree(&s, &item, &font, &grp, &mut kept);
        grp.sample[1] = 1;
        let after = agree(&s, &item, &font, &grp, &mut kept);
        assert_ne!(before, after);
    }

    #[test]
    fn kept_glyphs_tell_colors_and_codes_apart() {
        let mut font = Font::blank();
        let (gx, gy) = font.origin(0x41, FontSize::Small).unwrap();
        let (hx, hy) = font.origin(0x42, FontSize::Small).unwrap();
        let mut grp = Graphics::default();
        fill(&mut grp, FONT_PAGE, gx as i32, gy as i32, 0xFFFF_FFFF);
        grp.target = FONT_PAGE;
        grp.fill(
            hx as i32,
            hy as i32,
            hx as i32 + 3,
            hy as i32 + 3,
            0x80FF_FFFF,
            Blend::Normal,
        );
        font.page_mut()[0] = 0;
        let mut s = TextScreen::new(64, 32);
        s.print(&[0x41]);
        let mut kept = KeptGlyphs::default();
        let item = s.item(0, &Affine::IDENTITY, None);
        agree(&s, &item, &font, &grp, &mut kept);
        s.set_color(0xFF00_FF00);
        s.print(&[0x41, 0x42]);
        agree(&s, &item, &font, &grp, &mut kept);
        s.set_color(0x80FF_0000);
        s.print(&[0x42, 0x41]);
        let item = s.item(0, &Affine::IDENTITY, None);
        agree(&s, &item, &font, &grp, &mut kept);
    }

    #[test]
    fn so_many_glyphs_start_over_only_between_frames() {
        let font = Font::fallback();
        let grp = Graphics::default();
        let mut s = TextScreen::new(320, 240);
        let mut kept = KeptGlyphs::default();
        let codes: Vec<u16> = (0x21..0x7E).collect();
        for color in 0..40u32 {
            s.set_color(0xFF00_0000 | (color * 0x0101));
            for _ in 0..12 {
                s.print(&codes);
            }
            let item = s.item(0, &Affine::IDENTITY, None);
            agree(&s, &item, &font, &grp, &mut kept);
        }
        // A screen's worth over the limit at most.
        assert!(kept.glyphs.data.len() < MOST_KEPT + 64 * 40 * 30);
    }

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
