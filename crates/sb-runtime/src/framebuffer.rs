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

    pub fn clear(&mut self, color: u32) {
        self.pixels.fill(color);
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
