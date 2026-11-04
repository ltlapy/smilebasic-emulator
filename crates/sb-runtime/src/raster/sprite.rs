//! Draws one sprite item on the CPU.

use crate::compose::{self, Paint, Placement, draw_image};
use crate::framebuffer::Framebuffer;
use crate::graphics::{Clip, GRP_SIZE, Graphics, PIECE, Tint, TintJob};
use crate::scene::SpriteItem;
use crate::sprites::attr;

/// Draws a sprite over the frame, clipped to its layer's LCLIP, and
/// through LMATRIX's 4x4 matrix with the frame's pixels to a layer pixel
/// when the layer has one.
pub fn draw(
    s: &SpriteItem,
    grp: &Graphics,
    fb: &mut Framebuffer,
    clip: Option<&Clip>,
    projection: Option<(&[f64; 16], u32)>,
) {
    if s.shows_nothing() {
        return;
    }
    // The shown rectangle swaps sides for quarter turns.
    let quarter = s.attr & attr::ROTATION;
    let (bw, bh) = if quarter % 2 == 1 {
        (s.h, s.w)
    } else {
        (s.w, s.h)
    };
    let place = Placement {
        matrix: s.matrix,
        clip,
        add: s.attr & attr::ADD != 0,
    };
    let source = Source::new(s, grp);
    let tint = Tint::new(s.color, s.area as usize);
    if let Some((m, sample)) = projection {
        struct Projected<'a, 'b> {
            fb: &'a mut Framebuffer,
            place: &'a Placement<'b>,
            m: &'a [f64; 16],
            sample: u32,
            size: (i32, i32),
            source: &'a Source<'a>,
        }
        impl TintJob for Projected<'_, '_> {
            type Out = ();
            fn run<P: Paint>(self, paint: impl Fn(u32) -> P + Copy) {
                let (bw, bh) = self.size;
                let source = self.source;
                // The usual image: every point is on the page as it is.
                if let Some((page, u, v)) = source.plain() {
                    let (fb, m, sample, place) = (self.fb, self.m, self.sample, self.place);
                    return compose::project_image(fb, m, sample, place, bw, bh, |x, y| {
                        let p = page[(v + y as usize) * GRP_SIZE as usize + u + x as usize];
                        (p >> 24 != 0).then(|| paint(p))
                    });
                }
                compose::project_image(self.fb, self.m, self.sample, self.place, bw, bh, |x, y| {
                    let (x, y) = source.page_xy(x, y);
                    let p = source.pixel(x, y);
                    (p >> 24 != 0).then(|| paint(p))
                });
            }
        }
        return tint.with(
            place.add,
            Projected {
                fb,
                place: &place,
                m,
                sample,
                size: (bw, bh),
                source: &source,
            },
        );
    }
    let tie = match place.is_aligned() {
        true => (0.0, 0.0),
        false => place.texel_tie(
            (bw, bh),
            compose::page_signs(
                s.attr & attr::ROTATION,
                s.attr & attr::REVH != 0,
                s.attr & attr::REVV != 0,
            ),
        ),
    };
    if place.is_upright() {
        return draw_upright(&source, &tint, fb, &place, (bw, bh), tie, grp);
    }
    struct Turned<'a, 'b> {
        fb: &'a mut Framebuffer,
        place: &'a Placement<'b>,
        size: (i32, i32),
        tie: (f64, f64),
        source: &'a Source<'a>,
    }
    impl TintJob for Turned<'_, '_> {
        type Out = ();
        fn run<P: Paint>(self, paint: impl Fn(u32) -> P + Copy) {
            let (bw, bh) = self.size;
            let source = self.source;
            let (tx, ty) = self.tie;
            // At most angles nothing ties, and this is for each pixel.
            if (tx, ty) == (0.0, 0.0) {
                return draw_image(self.fb, self.place, bw, bh, |x, y| {
                    let (x, y) = source.page_xy(x as i32, y as i32);
                    let p = source.pixel(x, y);
                    (p >> 24 != 0).then(|| paint(p))
                });
            }
            draw_image(self.fb, self.place, bw, bh, |x, y| {
                let (x, y) = source.page_xy(texel(x + tx), texel(y + ty));
                let p = source.pixel(x, y);
                (p >> 24 != 0).then(|| paint(p))
            });
        }
    }
    tint.with(
        place.add,
        Turned {
            fb,
            place: &place,
            size: (bw, bh),
            tie,
            source: &source,
        },
    );
}

/// The texel a point lies in. A point moved to settle a tie can land just off
/// the image, on either side, and reads the page there.
#[inline]
fn texel(v: f64) -> i32 {
    v as i32 - i32::from(v < 0.0)
}

/// Where a sprite's pixels come from.
struct Source<'a> {
    page: &'a [u32],
    /// Which page it is; `None` for GRP-1.
    page_no: Option<usize>,
    /// GSAMPLE 1: the page repeats in every direction.
    repeat: bool,
    /// GRP-1, white everywhere.
    white: bool,
    u: i32,
    v: i32,
    w: i32,
    h: i32,
    attr: i32,
}

impl Source<'_> {
    /// For a sprite whose page is GRP-1 or one that is not blank.
    fn new<'a>(s: &SpriteItem, grp: &'a Graphics) -> Source<'a> {
        let (page, white) = match usize::try_from(s.page) {
            Ok(p) => (grp.page(p), false),
            Err(_) => (&[][..], true),
        };
        Source {
            page,
            page_no: usize::try_from(s.page).ok(),
            repeat: s.repeat,
            white,
            u: s.u,
            v: s.v,
            w: s.w,
            h: s.h,
            attr: s.attr,
        }
    }

    /// The page and where the image starts on it, for an image neither
    /// turned nor flipped that lies wholly on a page.
    fn plain(&self) -> Option<(&[u32], usize, usize)> {
        let on_page = |at: i32, size: i32| at >= 0 && at + size <= GRP_SIZE;
        let plain = !self.white
            && self.attr & (attr::ROTATION | attr::REVH | attr::REVV) == 0
            && on_page(self.u, self.w)
            && on_page(self.v, self.h)
            && self.page.len() == (GRP_SIZE * GRP_SIZE) as usize;
        plain.then_some((self.page, self.u as usize, self.v as usize))
    }

    /// The page point under the point (lx, ly) of the shown rectangle.
    /// The image is flipped, then turned (checked on hardware): undo the
    /// turn, then the flips.
    #[inline]
    fn page_xy(&self, lx: i32, ly: i32) -> (i32, i32) {
        let (w, h) = (self.w, self.h);
        let (mut ix, mut iy) = match self.attr & attr::ROTATION {
            0 => (lx, ly),
            1 => (ly, h - 1 - lx),
            2 => (w - 1 - lx, h - 1 - ly),
            _ => (w - 1 - ly, lx),
        };
        if self.attr & attr::REVH != 0 {
            ix = w - 1 - ix;
        }
        if self.attr & attr::REVV != 0 {
            iy = h - 1 - iy;
        }
        (self.u + ix, self.v + iy)
    }

    /// A page coordinate as it is read: wrapped around when the page
    /// repeats, or `None` off the page.
    #[inline]
    fn wrap(&self, v: i32) -> Option<usize> {
        match self.repeat {
            true => Some(v.rem_euclid(GRP_SIZE) as usize),
            false => (0..GRP_SIZE).contains(&v).then_some(v as usize),
        }
    }

    /// [`Source::pixel`] for a point off the page: a tie just before the page's
    /// first texel reads that one; just after the last, nothing.
    #[cold]
    #[inline(never)]
    fn pixel_off_page(&self, x: i32, y: i32) -> u32 {
        let on = |v: i32| match v {
            -1 => Some(0),
            0..GRP_SIZE => Some(v as usize),
            _ => None,
        };
        match (on(x), on(y)) {
            (Some(x), Some(y)) if !self.repeat => self.page[y * GRP_SIZE as usize + x],
            _ => 0,
        }
    }

    /// The page pixel at a page point, as [`sample`] gives it.
    #[inline]
    fn pixel(&self, x: i32, y: i32) -> u32 {
        if self.white {
            return 0xFFFF_FFFF;
        }
        match (self.wrap(x), self.wrap(y)) {
            (Some(x), Some(y)) => self.page[y * GRP_SIZE as usize + x],
            _ => self.pixel_off_page(x, y),
        }
    }
}

/// Off the page, as an offset into it.
const NONE: usize = usize::MAX;
/// Frame pixels from which a sprite is drawn piece by piece.
const LARGE: usize = 4096;

/// [`draw`] for a sprite neither turned nor sheared on the frame,
/// the usual case: which page column and row each frame column and row
/// shows is worked out once, so each pixel is a lookup and a blend.
fn draw_upright(
    source: &Source,
    tint: &Tint,
    fb: &mut Framebuffer,
    place: &Placement,
    (bw, bh): (i32, i32),
    (tx, ty): (f64, f64),
    grp: &Graphics,
) {
    let Some(grid) = compose::grid(fb, place, bw, bh) else {
        return;
    };
    let stride = GRP_SIZE as usize;
    // A tie just before the page's first texel reads that one.
    let at = |v: i32, step: usize| match source.wrap(v) {
        Some(v) => v * step,
        None if v == -1 => 0,
        None => NONE,
    };
    // A quarter turn makes frame columns go down the page, and rows
    // across it.
    let turned = (source.attr & attr::ROTATION) % 2 == 1;
    let cols: Vec<usize> = (grid.xs.iter())
        .map(|&x| {
            let (px, py) = source.page_xy(texel(x + tx), 0);
            match turned {
                false => at(px, 1),
                true => at(py, stride),
            }
        })
        .collect();
    let rows = grid.ys.iter().map(|&y| {
        let (px, py) = source.page_xy(0, texel(y + ty));
        match turned {
            false => at(py, stride),
            true => at(px, 1),
        }
    });
    // Without a flip or a turn, the frame columns show page columns
    // from left to right, so the pieces of each row with nothing in them
    // (see Graphics::pieces) can be passed over.
    // Small sprites, the many, are not worth the setting up.
    let large = cols.len() * grid.ys.len() >= LARGE;
    let run = match (turned, source.white, source.page_no) {
        (false, false, Some(page)) if large => steady(&cols).map(|run| (run, page)),
        _ => None,
    };
    struct Rows<'a, R> {
        fb: &'a mut Framebuffer,
        grid: &'a compose::Grid,
        cols: &'a [usize],
        rows: R,
        source: &'a Source<'a>,
        run: Option<(Run, usize)>,
        grp: &'a Graphics,
        /// Neither tinted nor added: what is opaque is put as it is.
        plain: bool,
    }
    impl<R: Iterator<Item = usize>> TintJob for Rows<'_, R> {
        type Out = ();
        fn run<P: Paint>(self, paint: impl Fn(u32) -> P + Copy) {
            let width = self.fb.width() as usize;
            let pixels = self.fb.pixels_mut();
            let page = self.source.page;
            // GRP-1 is one color all over, as large boxes and fades use:
            // its blend is worked out once.
            if self.source.white {
                let white = paint(0xFFFF_FFFF);
                for (fy, row) in (self.grid.y0 as usize..).zip(self.rows) {
                    if row == NONE {
                        continue;
                    }
                    let at = fy * width + self.grid.x0 as usize;
                    let line = &mut pixels[at..at + self.cols.len()];
                    for (dst, &col) in line.iter_mut().zip(self.cols) {
                        if col != NONE {
                            *dst = white.put(*dst);
                        }
                    }
                }
                return;
            }
            if let Some((run, page_no)) = self.run {
                let stride = GRP_SIZE as usize;
                let shown = &self.cols[run.at..][..run.len];
                let (first, last) = (shown[0], shown[run.len - 1]);
                let (p0, p1) = (first / PIECE, last / PIECE);
                // Where among the shown columns each piece's start.
                let mut starts = [0; GRP_SIZE as usize / PIECE + 1];
                for (b, start) in starts.iter_mut().enumerate().take(p1 + 2).skip(p0) {
                    *start = shown.partition_point(|&c| c < b * PIECE);
                }
                // At a scale of 1, the usual way to put a large image up,
                // a row of the frame is a run of a row of the page.
                let side_by_side = last - first + 1 == run.len;
                // The page pixel `paint` blends, which a clear one never
                // is.
                let (mut held, mut held_paint) = (0, paint(0));
                for (fy, row) in (self.grid.y0 as usize..).zip(self.rows) {
                    if row == NONE {
                        continue;
                    }
                    let mut pieces = (self.grp).pieces(page_no, row / stride, p0, p1);
                    let at = fy * width + self.grid.x0 as usize + run.at;
                    let line = &mut pixels[at..at + run.len];
                    let put = |dst: &mut u32, p: u32| {
                        if p >> 24 != 0 {
                            *dst = paint(p).put(*dst);
                        }
                    };
                    while pieces != 0 {
                        let b = pieces.trailing_zeros() as usize;
                        pieces &= pieces - 1;
                        let (start, end) = (starts[b], starts[b + 1]);
                        let line = &mut line[start..end];
                        if side_by_side {
                            let from = &page[row + first + start..][..end - start];
                            // Eight at a time, so that clear stretches
                            // inside a piece cost one test.
                            for (dst, from) in line.chunks_mut(8).zip(from.chunks(8)) {
                                if from.iter().fold(0, |a, &p| a | p) >> 24 == 0 {
                                    continue;
                                }
                                // Opaque pixels as they are cover what
                                // is under them.
                                if self.plain && from.iter().fold(!0, |a, &p| a & p) >> 24 == 0xFF {
                                    dst.copy_from_slice(from);
                                    continue;
                                }
                                for (dst, &p) in dst.iter_mut().zip(from) {
                                    put(dst, p);
                                }
                            }
                        } else {
                            // Scaled up, neighbors show the same page
                            // pixel, whose blend is worked out once.
                            for (dst, &col) in line.iter_mut().zip(&shown[start..end]) {
                                let p = page[row + col];
                                match p >> 24 {
                                    0 => continue,
                                    0xFF if self.plain => {
                                        *dst = p;
                                        continue;
                                    }
                                    _ => {}
                                }
                                if p != held {
                                    held = p;
                                    held_paint = paint(p);
                                }
                                *dst = held_paint.put(*dst);
                            }
                        }
                    }
                }
                return;
            }
            for (fy, row) in (self.grid.y0 as usize..).zip(self.rows) {
                if row == NONE {
                    continue;
                }
                let at = fy * width + self.grid.x0 as usize;
                let line = &mut pixels[at..at + self.cols.len()];
                // Scaled up, neighbors show the same page pixel, whose
                // paint is worked out once.
                let (mut held, mut held_paint) = (0, paint(0));
                for (dst, &col) in line.iter_mut().zip(self.cols) {
                    if col == NONE {
                        continue;
                    }
                    let p = page[col + row];
                    if p >> 24 == 0 {
                        continue;
                    }
                    if p != held {
                        held = p;
                        held_paint = paint(p);
                    }
                    *dst = held_paint.put(*dst);
                }
            }
        }
    }
    tint.with(
        place.add,
        Rows {
            fb,
            grid: &grid,
            cols: &cols,
            rows,
            source,
            run,
            grp,
            plain: matches!(tint, Tint::White) && !place.add,
        },
    );
}

/// Frame columns of a sprite that show page columns from left to
/// right: from the `at`th of the sprite's frame columns on, `len` of
/// them, the rest showing none.
#[derive(Clone, Copy)]
struct Run {
    at: usize,
    len: usize,
}

/// The [`Run`] that `cols` (as [`draw_upright`] works them out) are, if
/// they are one.
fn steady(cols: &[usize]) -> Option<Run> {
    let at = cols.iter().position(|&c| c != NONE)?;
    let len = cols[at..].iter().take_while(|&&c| c != NONE).count();
    let rightwards = cols[at..at + len].windows(2).all(|w| w[0] <= w[1]);
    let rest_off = cols[at + len..].iter().all(|&c| c == NONE);
    (rightwards && rest_off).then_some(Run { at, len })
}
