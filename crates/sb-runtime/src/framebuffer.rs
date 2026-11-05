/// Final composited screen image.
///
/// Pixels are stored as `0xAARRGGBB`, the same layout SmileBASIC uses for
/// color values such as `&HFFFF0000`.
#[derive(Clone, Debug)]
pub struct Framebuffer {
    width: u32,
    height: u32,
    pixels: Vec<u32>,
}

impl Framebuffer {
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            pixels: vec![0xFF00_0000; (width * height) as usize],
        }
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    pub fn pixels(&self) -> &[u32] {
        &self.pixels
    }

    pub fn pixels_mut(&mut self) -> &mut [u32] {
        &mut self.pixels
    }

    /// Makes the frame `width` x `height`, keeping its memory where it
    /// can; what it holds afterwards is left to the caller to clear.
    pub fn resize(&mut self, width: u32, height: u32) {
        (self.width, self.height) = (width, height);
        self.pixels.resize((width * height) as usize, 0);
    }

    /// Trades pixels with `other`, which must hold as many: a pass that
    /// reads the frame while it writes a new one can then write into a
    /// buffer it keeps instead of copying the frame first.
    pub fn swap_pixels(&mut self, other: &mut Vec<u32>) {
        assert_eq!(other.len(), self.pixels.len());
        std::mem::swap(&mut self.pixels, other);
    }

    pub fn clear(&mut self, color: u32) {
        self.pixels.fill(color);
    }

    /// Makes a part of the frame clear.
    pub fn clear_area(&mut self, area: &crate::compose::Area) {
        let width = self.width as usize;
        if width == 0 {
            return;
        }
        let (x0, x1) = (area.x0 as usize, area.x1 as usize);
        let rows = self.pixels.chunks_exact_mut(width);
        let range = area.y0 as usize..area.y1 as usize;
        for row in rows.skip(range.start).take(range.len()) {
            row[x0..x1].fill(0);
        }
    }

    pub fn set(&mut self, x: u32, y: u32, color: u32) {
        if x < self.width && y < self.height {
            self.pixels[(y * self.width + x) as usize] = color;
        }
    }

    pub fn get(&self, x: u32, y: u32) -> Option<u32> {
        (x < self.width && y < self.height).then(|| self.pixels[(y * self.width + x) as usize])
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn clearing_a_part() {
        let mut fb = super::Framebuffer::new(4, 3);
        fb.clear(7);
        let area = crate::compose::Area {
            x0: 1,
            y0: 1,
            x1: 3,
            y1: 3,
        };
        fb.clear_area(&area);
        assert_eq!(fb.pixels(), [7, 7, 7, 7, 7, 0, 0, 7, 7, 0, 0, 7]);
    }

    use super::*;

    #[test]
    fn new_is_opaque_black() {
        let fb = Framebuffer::new(4, 3);
        assert_eq!(fb.pixels().len(), 12);
        assert!(fb.pixels().iter().all(|&p| p == 0xFF00_0000));
    }

    #[test]
    fn set_get_ignores_out_of_range() {
        let mut fb = Framebuffer::new(2, 2);
        fb.set(1, 1, 0xFFFF_0000);
        fb.set(5, 0, 0xFFFF_FFFF);
        assert_eq!(fb.get(1, 1), Some(0xFFFF_0000));
        assert_eq!(fb.get(2, 0), None);
    }
}
