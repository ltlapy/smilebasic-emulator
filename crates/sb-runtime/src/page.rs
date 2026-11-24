//! The memory layout of a GRP page's pixels.
//!
//! A page is [`GRP_SIZE`] rows of [`GRP_SIZE`] pixels, each row starting
//! [`STRIDE`] pixels after the one before. Only [`Page`] knows the stride:
//! everything else takes rows from it as slices of exactly [`GRP_SIZE`]
//! pixels, or, where a loop steps through a page by offsets (the sprite
//! rasterizer, GPU uploads), uses [`STRIDE`] and [`at`] by name. The pixels
//! past a row's end within its stride are never written and never read.

use std::ops::Range;

use crate::graphics::GRP_SIZE;

/// The number of pixels in a row.
const SIDE: usize = GRP_SIZE as usize;

/// Pixels from the start of one row of a page to the start of the next.
///
/// Rows 2048 pixels apart are 8 KiB apart, the size of one way of a 32 KiB
/// 4-way L1 data cache such as the one in the Cortex-A35, so the same
/// column of successive rows falls in one cache set. 64 pixels more, 256
/// bytes, spreads the rows over the sets: on an RK3326, GCOPY's scene took
/// 52% less time and GFILL's 23% less, for 3% more memory per page. See
/// docs/performance/benchmark-results.md, "GRP rows padded".
pub const STRIDE: usize = SIDE + 64;
const _: () = assert!(STRIDE >= SIDE);

/// The number of pixels in a page that is not blank, counting the rows and
/// the padding at the end of each row, the last one too.
pub const LEN: usize = SIDE * STRIDE;

/// The offset of pixel `(x, y)` of a page in [`Page::raw`].
#[inline]
pub const fn at(x: usize, y: usize) -> usize {
    y * STRIDE + x
}

/// [`at`] in 32 bits, for pixels plotted one at a time: on aarch64 the
/// offset then takes one instruction fewer to compute than in 64 bits.
#[inline]
fn at32(x: u32, y: u32) -> usize {
    y.wrapping_mul(STRIDE as u32).wrapping_add(x) as usize
}

/// A page's pixels, `0xAARRGGBB`. A blank page holds none and reads 0
/// everywhere, the same as at start-up.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Page {
    /// Empty, or [`LEN`] pixels laid out as the module comment describes.
    pixels: Vec<u32>,
}

impl Page {
    /// A page that is not blank, every pixel 0.
    pub fn cleared() -> Page {
        Page {
            pixels: vec![0; LEN],
        }
    }

    /// A page from an image of [`GRP_SIZE`] rows of [`GRP_SIZE`] pixels,
    /// one after another, the way files and arrays hold pages.
    pub fn from_rows(rows: Vec<u32>) -> Page {
        assert_eq!(rows.len(), SIDE * SIDE, "a page image is 2048x2048");
        if STRIDE == SIDE {
            return Page { pixels: rows };
        }
        let mut page = Page::cleared();
        for (to, from) in page.rows_mut(0..SIDE).zip(rows.chunks_exact(SIDE)) {
            to.copy_from_slice(from);
        }
        page
    }

    /// The page as an image of [`GRP_SIZE`] rows of [`GRP_SIZE`] pixels,
    /// one after another; all 0 when it is blank.
    pub fn to_rows(&self) -> Vec<u32> {
        if self.is_blank() {
            return vec![0; SIDE * SIDE];
        }
        if STRIDE == SIDE {
            return self.pixels.clone();
        }
        let mut rows = Vec::with_capacity(SIDE * SIDE);
        for row in self.rows(0..SIDE) {
            rows.extend_from_slice(row);
        }
        rows
    }

    pub fn is_blank(&self) -> bool {
        self.pixels.is_empty()
    }

    /// Turns a blank page into one that is not blank, every pixel 0.
    #[inline]
    pub fn make_room(&mut self) {
        if self.is_blank() {
            *self = Page::cleared();
        }
    }

    /// A pixel; 0 off the page and on a blank one.
    #[inline]
    pub fn pixel(&self, x: i32, y: i32) -> u32 {
        // A negative coordinate is past the page as unsigned.
        let (x, y) = (x as u32, y as u32);
        if self.is_blank() || x >= SIDE as u32 || y >= SIDE as u32 {
            return 0;
        }
        self.pixels[at32(x, y)]
    }

    /// A pixel on the page to change, on a page that is not blank.
    #[inline]
    pub fn pixel_mut(&mut self, x: i32, y: i32) -> &mut u32 {
        // Callers clip to the page; a coordinate off it would land elsewhere
        // in the buffer, whose bounds are checked.
        debug_assert!((0..GRP_SIZE).contains(&x) && (0..GRP_SIZE).contains(&y));
        &mut self.pixels[at32(x as u32, y as u32)]
    }

    /// Row `y` of a page that is not blank, [`GRP_SIZE`] pixels.
    #[inline]
    pub fn row(&self, y: usize) -> &[u32] {
        &self.pixels[at(0, y)..][..SIDE]
    }

    /// Row `y` of a page that is not blank, to change.
    #[inline]
    pub fn row_mut(&mut self, y: usize) -> &mut [u32] {
        &mut self.pixels[at(0, y)..][..SIDE]
    }

    /// Rows `ys` of a page that is not blank, top to bottom.
    #[inline]
    pub fn rows(&self, ys: Range<usize>) -> impl Iterator<Item = &[u32]> {
        self.pixels[ys.start * STRIDE..ys.end * STRIDE]
            .chunks_exact(STRIDE)
            .map(|row| &row[..SIDE])
    }

    /// Rows `ys` of a page that is not blank, to change.
    #[inline]
    pub fn rows_mut(&mut self, ys: Range<usize>) -> impl Iterator<Item = &mut [u32]> {
        self.pixels[ys.start * STRIDE..ys.end * STRIDE]
            .chunks_exact_mut(STRIDE)
            .map(|row| &mut row[..SIDE])
    }

    /// Pixels `x0..=x1` of row `y` of a page that is not blank, to change.
    #[inline]
    pub fn span_mut(&mut self, y: i32, x0: i32, x1: i32) -> &mut [u32] {
        // As for `pixel_mut`: callers clip to the page, and the slice is
        // checked against the buffer. A check here would cost GALPHA and
        // GTRI 0.1 to 0.3% of their instructions.
        debug_assert!(
            (0..GRP_SIZE).contains(&x1) && x0 <= x1 + 1,
            "a span on its row"
        );
        let row = at32(0, y as u32);
        &mut self.pixels[row + x0 as usize..row + x1 as usize + 1]
    }

    /// Of rows `ys` of a page that is not blank, the `n` pixels from column
    /// `x` of each, top to bottom: a rectangle, checked once for all its
    /// rows.
    #[inline]
    pub fn spans(&self, x: usize, n: usize, ys: Range<usize>) -> impl Iterator<Item = &[u32]> {
        assert!(x + n <= SIDE && ys.end <= SIDE, "a rectangle on the page");
        // The last row's piece is shorter than STRIDE, but holds `n`.
        let rows = ys.len();
        let start = at(x, ys.start).min(self.pixels.len());
        self.pixels[start..]
            .chunks(STRIDE)
            .take(rows)
            .map(move |row| &row[..n])
    }

    /// [`Page::spans`] to change.
    #[inline]
    pub fn spans_mut(
        &mut self,
        x: usize,
        n: usize,
        ys: Range<usize>,
    ) -> impl Iterator<Item = &mut [u32]> {
        assert!(x + n <= SIDE && ys.end <= SIDE, "a rectangle on the page");
        let rows = ys.len();
        let start = at(x, ys.start).min(self.pixels.len());
        self.pixels[start..]
            .chunks_mut(STRIDE)
            .take(rows)
            .map(move |row| &mut row[..n])
    }

    /// The pixels in their memory layout, for loops that step through them
    /// by offsets from [`at`] and [`STRIDE`]: rows apart, and empty when
    /// blank. An offset of a pixel on the page is always in range; the
    /// pixels between rows must never be read.
    #[inline]
    pub fn raw(&self) -> &[u32] {
        &self.pixels
    }

    /// The memory the page takes, in bytes.
    pub fn bytes(&self) -> usize {
        self.pixels.len() * 4
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn image() -> Vec<u32> {
        (0..(SIDE * SIDE) as u32)
            .map(|i| i.wrapping_mul(0x9E37_79B9))
            .collect()
    }

    #[test]
    fn rows_go_in_and_come_out_as_they_were() {
        let rows = image();
        let page = Page::from_rows(rows.clone());
        assert_eq!(page.to_rows(), rows);
        for y in [0, 1, 1000, SIDE - 1] {
            assert_eq!(page.row(y), &rows[y * SIDE..][..SIDE]);
            for x in [0, 1, 777, SIDE - 1] {
                assert_eq!(page.pixel(x as i32, y as i32), rows[y * SIDE + x]);
                assert_eq!(page.raw()[at(x, y)], rows[y * SIDE + x]);
            }
        }
        let some: Vec<&[u32]> = page.rows(5..9).collect();
        assert_eq!(some.len(), 4);
        assert!(some.iter().enumerate().all(|(i, r)| *r == page.row(5 + i)));
        // Rectangles, the last row's included.
        let spans: Vec<&[u32]> = page.spans(SIDE - 3, 3, SIDE - 2..SIDE).collect();
        assert_eq!(spans.len(), 2);
        assert_eq!(spans[1], &rows[SIDE * SIDE - 3..]);
        assert_eq!(page.spans(0, 1, SIDE..SIDE).count(), 0);
    }

    #[test]
    fn spans_change_only_their_pixels() {
        let mut page = Page::cleared();
        for span in page.spans_mut(10, 4, 20..23) {
            span.fill(7);
        }
        page.span_mut(GRP_SIZE - 1, GRP_SIZE - 2, GRP_SIZE - 1)
            .fill(9);
        let rows = page.to_rows();
        let at = |x: usize, y: usize| rows[y * SIDE + x];
        assert_eq!(rows.iter().filter(|&&p| p == 7).count(), 12);
        assert_eq!(
            (at(10, 20), at(13, 22), at(14, 22), at(10, 23)),
            (7, 7, 0, 0)
        );
        assert_eq!(rows.iter().filter(|&&p| p == 9).count(), 2);
        assert_eq!(at(SIDE - 1, SIDE - 1), 9);
    }

    #[test]
    fn nothing_past_a_row_is_written() {
        let mut page = Page::from_rows(image());
        for row in page.rows_mut(0..SIDE) {
            row.fill(0xFFFF_FFFF);
        }
        page.row_mut(3).fill(1);
        *page.pixel_mut(GRP_SIZE - 1, GRP_SIZE - 1) = 2;
        let raw = page.raw();
        assert_eq!(raw.len(), LEN);
        for y in 0..SIDE {
            assert!(raw[at(SIDE, y)..at(0, y + 1)].iter().all(|&p| p == 0));
        }
    }

    #[test]
    fn a_blank_page_reads_zero() {
        let page = Page::default();
        assert!(page.is_blank());
        assert_eq!(page.pixel(0, 0), 0);
        assert_eq!(page.to_rows(), vec![0; SIDE * SIDE]);
        assert_eq!(page.bytes(), 0);
        let mut page = page;
        page.make_room();
        assert!(!page.is_blank());
        assert_eq!(page.bytes(), LEN * 4);
        assert_eq!(page.pixel(-1, 0), 0);
        assert_eq!(page.pixel(0, GRP_SIZE), 0);
    }
}
