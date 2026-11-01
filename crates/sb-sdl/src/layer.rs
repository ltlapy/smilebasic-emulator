//! A part of the emulator's UI with pixels and a texture of its own,
//! drawn again only when what it shows changes, and given to the texture
//! only where it changed. A dry run of its drawing (see [`ui::dry_run`])
//! tells whether anything did.

use sb_runtime::Framebuffer;
use sdl3::pixels::PixelFormat;
use sdl3::render::{BlendMode, Canvas, FRect, ScaleMode, Texture, TextureCreator};
use sdl3::video::{Window, WindowContext};

use crate::ui::{self, Drawn, Rect};

type AnyError = Box<dyn std::error::Error>;

pub struct Layer<'a> {
    /// Transparent except within `drawn`.
    fb: Framebuffer,
    texture: Option<Texture<'a>>,
    /// The fingerprint of what `fb` and the texture hold.
    key: Option<u64>,
    /// Around what was drawn last.
    drawn: Option<Rect>,
}

impl Default for Layer<'_> {
    fn default() -> Self {
        Layer {
            fb: Framebuffer::new(0, 0),
            texture: None,
            key: None,
            drawn: None,
        }
    }
}

impl<'a> Layer<'a> {
    /// What `draw` would draw on a frame of `size` pixels, drawing
    /// nothing.
    pub fn dry_run(&mut self, size: (u32, u32), draw: impl FnOnce(&mut Framebuffer)) -> Drawn {
        self.resize(size);
        let fb = &mut self.fb;
        ui::dry_run(|| draw(fb))
    }

    fn resize(&mut self, size: (u32, u32)) {
        if (self.fb.width(), self.fb.height()) != size {
            self.fb = Framebuffer::new(size.0, size.1);
            self.fb.clear(0);
            (self.texture, self.key, self.drawn) = (None, None, None);
        }
    }

    /// Brings the layer up to what `draw` draws on a frame of `size`
    /// pixels, starting from a transparent one: it draws only if the dry
    /// run finds something changed, and uploads only around what changed.
    /// `draw` runs twice then, and must do the same both times.
    pub fn update(
        &mut self,
        creator: &'a TextureCreator<WindowContext>,
        size: (u32, u32),
        mut draw: impl FnMut(&mut Framebuffer),
    ) -> Result<(), AnyError> {
        let Drawn { key, bounds, .. } = self.dry_run(size, &mut draw);
        // Without a texture, nothing was drawn yet, or it was lost.
        if self.key == Some(key) && (self.texture.is_some() || self.drawn.is_none()) {
            return Ok(());
        }
        if let Some(old) = self.drawn {
            ui::fill(&mut self.fb, old, 0);
        }
        draw(&mut self.fb);
        let changed = match self.texture {
            Some(_) => ui::union(self.drawn, bounds),
            None => bounds,
        };
        (self.key, self.drawn) = (Some(key), bounds);
        let Some(r) = changed else {
            return Ok(());
        };
        let texture = match &mut self.texture {
            Some(texture) => texture,
            None => {
                let mut made =
                    creator.create_texture_streaming(PixelFormat::ARGB8888, size.0, size.1)?;
                made.set_scale_mode(ScaleMode::Nearest);
                made.set_blend_mode(BlendMode::Blend);
                // Transparent outside what is drawn.
                let pixels = crate::as_bytes(self.fb.pixels());
                crate::bench::uploaded(pixels.len());
                made.update(None, pixels, (size.0 * 4) as usize)?;
                self.texture = Some(made);
                return Ok(());
            }
        };
        let w = self.fb.width();
        let start = (r.y * w + r.x) as usize;
        let end = ((r.y + r.h - 1) * w + r.x + r.w) as usize;
        let pixels = crate::as_bytes(&self.fb.pixels()[start..end]);
        crate::bench::uploaded((r.w * r.h * 4) as usize);
        let rect = sdl3::rect::Rect::new(r.x as i32, r.y as i32, r.w, r.h);
        texture.update(rect, pixels, (w * 4) as usize)?;
        Ok(())
    }

    /// Forgets what the texture holds, as when the renderer lost it.
    pub fn forget(&mut self) {
        self.key = None;
        self.texture = None;
    }

    /// Lets go of the texture and pixels while the layer is not shown.
    pub fn release(&mut self) {
        if self.texture.is_some() || self.fb.width() > 0 {
            *self = Layer::default();
        }
    }

    /// Draws what the layer holds with its top left corner at `at` on the
    /// display, which is `display` on the canvas and `display_w` pixels
    /// wide.
    pub fn copy(
        &self,
        canvas: &mut Canvas<Window>,
        at: (u32, u32),
        display: FRect,
        display_w: u32,
    ) -> Result<(), AnyError> {
        let (Some(texture), Some(r)) = (&self.texture, self.drawn) else {
            return Ok(());
        };
        let scale = display.w / display_w as f32;
        let src = FRect::new(r.x as f32, r.y as f32, r.w as f32, r.h as f32);
        let dst = FRect::new(
            display.x + (at.0 + r.x) as f32 * scale,
            display.y + (at.1 + r.y) as f32 * scale,
            r.w as f32 * scale,
            r.h as f32 * scale,
        );
        canvas.copy(texture, src, dst)?;
        Ok(())
    }
}
