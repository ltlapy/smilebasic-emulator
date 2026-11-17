//! A part of the emulator's UI with pixels and a texture of its own,
//! drawn again only when what it shows changes, and given to the texture
//! only where it changed. A dry run of its drawing (see [`ui::dry_run`])
//! tells whether anything did.

use sb_runtime::Framebuffer;
use sdl3::pixels::{Color, PixelFormat};
use sdl3::render::{BlendMode, Canvas, FRect, ScaleMode, Texture, TextureCreator};
use sdl3::sys::render::{SDL_Renderer, SDL_SetTextureScaleMode};
use sdl3::sys::surface::SDL_ScaleMode;
use sdl3::sys::video::SDL_GL_GetProcAddress;
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
    /// Where the texture came from, for the enlarged copy.
    creator: Option<&'a TextureCreator<WindowContext>>,
    /// How the texture is sampled now.
    sampling: Sampling,
    /// The renderer last drawn to, and whether its pixel art mode blends.
    pixel_art: Option<(*mut SDL_Renderer, bool)>,
    /// Where pixel art mode does not blend: what was drawn, enlarged a
    /// whole number of times with nearest and shown with linear.
    enlarged: Option<Enlarged<'a>>,
    /// A size the renderer refused for `enlarged`.
    refused: Option<(u32, u32)>,
}

/// How a layer's texture is sampled at the scale it is shown at.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sampling {
    /// A whole number of times its size: each pixel is a square of the
    /// same size, as sharp as at 1x.
    Nearest,
    /// Larger, not a whole number of times: SDL's pixel art mode, which
    /// blends only along the edges between pixels, so every stroke keeps
    /// its width where nearest would make some one pixel wider.
    PixelArt,
    /// Smaller, where nearest would drop whole rows and columns.
    Linear,
}

impl Sampling {
    fn mode(self) -> SDL_ScaleMode {
        match self {
            Sampling::Nearest => SDL_ScaleMode::NEAREST,
            Sampling::PixelArt => SDL_ScaleMode::PIXELART,
            Sampling::Linear => SDL_ScaleMode::LINEAR,
        }
    }
}

/// How to sample a layer drawn `scale` times its size.
pub fn sampling(scale: f32) -> Sampling {
    if scale >= 0.999 && (scale - scale.round()).abs() < 0.001 {
        Sampling::Nearest
    } else if scale > 1.0 {
        Sampling::PixelArt
    } else {
        Sampling::Linear
    }
}

struct Enlarged<'a> {
    /// A render target, transparent outside what it holds.
    texture: Texture<'a>,
    size: (u32, u32),
    /// What it holds from its top left corner: how many times enlarged,
    /// and the layer's fingerprint and drawn rectangle then.
    holds: Option<(u32, u64, Rect)>,
}

impl Default for Layer<'_> {
    fn default() -> Self {
        Layer {
            fb: Framebuffer::new(0, 0),
            texture: None,
            key: None,
            drawn: None,
            creator: None,
            sampling: Sampling::Nearest,
            pixel_art: None,
            enlarged: None,
            refused: None,
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
            self.drop_textures();
            self.drawn = None;
        }
    }

    fn drop_textures(&mut self) {
        (self.texture, self.key, self.enlarged, self.refused) = (None, None, None, None);
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
                crate::bench::made();
                crate::gpu::texture_fits(creator.raw(), size.0, size.1)?;
                let mut made =
                    creator.create_texture_streaming(PixelFormat::ARGB8888, size.0, size.1)?;
                made.set_scale_mode(ScaleMode::Nearest);
                made.set_blend_mode(BlendMode::Blend);
                // Transparent outside what is drawn.
                let pixels = crate::as_bytes(self.fb.pixels());
                crate::bench::uploaded(pixels.len());
                made.update(None, pixels, (size.0 * 4) as usize)?;
                self.texture = Some(made);
                (self.creator, self.sampling) = (Some(creator), Sampling::Nearest);
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
        self.drop_textures();
        self.pixel_art = None;
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
        &mut self,
        canvas: &mut Canvas<Window>,
        at: (u32, u32),
        display: FRect,
        display_w: u32,
    ) -> Result<(), AnyError> {
        let (Some(texture), Some(r), Some(key)) = (&mut self.texture, self.drawn, self.key) else {
            return Ok(());
        };
        let scale = display.w / display_w as f32;
        let dst = FRect::new(
            display.x + (at.0 + r.x) as f32 * scale,
            display.y + (at.1 + r.y) as f32 * scale,
            r.w as f32 * scale,
            r.h as f32 * scale,
        );
        let mut want = sampling(scale);
        let mut enlarge =
            want == Sampling::PixelArt && !pixel_art_blends(&mut self.pixel_art, canvas);
        if enlarge {
            want = Sampling::Nearest;
        }
        // Only on a change: this runs every frame for each layer.
        if want != self.sampling {
            // SAFETY: the texture is alive, and SDL checks it is a texture.
            if unsafe { SDL_SetTextureScaleMode(texture.raw(), want.mode()) } {
                self.sampling = want;
            } else if want == Sampling::PixelArt {
                self.pixel_art = Some((canvas.raw(), false));
                enlarge = true;
            }
        }
        if let (true, Some(creator)) = (enlarge && self.sampling == Sampling::Nearest, self.creator)
        {
            let k = scale.ceil() as u32;
            let holds = (k, key, r);
            let (enlarged, refused) = (&mut self.enlarged, &mut self.refused);
            if enlarge_into(enlarged, refused, creator, canvas, texture, holds)?
                && let Some(e) = &self.enlarged
            {
                let src = FRect::new(0.0, 0.0, (r.w * k) as f32, (r.h * k) as f32);
                canvas.copy(&e.texture, src, dst)?;
                return Ok(());
            }
        } else if self.enlarged.is_some() {
            self.enlarged = None;
        }
        // Where the enlarged copy was refused, nearest as before.
        let src = FRect::new(r.x as f32, r.y as f32, r.w as f32, r.h as f32);
        canvas.copy(texture, src, dst)?;
        Ok(())
    }
}

/// Gets `enlarged` to hold `holds`: the rectangle of `from` it names drawn
/// that many times larger with nearest, at its top left corner. It grows
/// to what it must hold but does not shrink, so that a dialog opening or a
/// notice coming and going does not make it again. False where the
/// renderer refuses it.
fn enlarge_into<'a>(
    enlarged: &mut Option<Enlarged<'a>>,
    refused: &mut Option<(u32, u32)>,
    creator: &'a TextureCreator<WindowContext>,
    canvas: &mut Canvas<Window>,
    from: &mut Texture,
    holds: (u32, u64, Rect),
) -> Result<bool, AnyError> {
    let (k, _, r) = holds;
    let need = (r.w * k, r.h * k);
    let room = |e: &Enlarged| e.size.0 >= need.0 && e.size.1 >= need.1;
    if !enlarged.as_ref().is_some_and(room) {
        let size = match enlarged.take() {
            Some(e) => (e.size.0.max(need.0), e.size.1.max(need.1)),
            None => need,
        };
        if *refused == Some(size) {
            return Ok(false);
        }
        crate::bench::made();
        // On OpenGL ES, SDL 3.4 frees twice when a texture is refused for
        // want of memory, so GL is asked first.
        let fits = crate::gpu::texture_fits(canvas.raw(), size.0, size.1);
        let made = fits.and_then(|()| {
            (creator.create_texture_target(PixelFormat::ARGB8888, size.0, size.1))
                .map_err(|e| e.to_string())
        });
        let Ok(mut made) = made else {
            *refused = Some(size);
            return Ok(false);
        };
        made.set_scale_mode(ScaleMode::Linear);
        made.set_blend_mode(BlendMode::Blend);
        *enlarged = Some(Enlarged {
            texture: made,
            size,
            holds: None,
        });
    }
    let Some(e) = enlarged else {
        return Ok(false);
    };
    if e.holds != Some(holds) {
        let src = FRect::new(r.x as f32, r.y as f32, r.w as f32, r.h as f32);
        let dst = FRect::new(0.0, 0.0, need.0 as f32, need.1 as f32);
        // Copied as it is, not blended over the cleared target, which
        // would darken what is partly transparent.
        from.set_blend_mode(BlendMode::None);
        let mut drawn = Ok(());
        let target = canvas.with_texture_canvas(&mut e.texture, |c| {
            let color = c.draw_color();
            c.set_draw_color(Color::RGBA(0, 0, 0, 0));
            c.clear();
            c.set_draw_color(color);
            drawn = c.copy(&*from, src, dst);
        });
        from.set_blend_mode(BlendMode::Blend);
        target?;
        drawn?;
        e.holds = Some(holds);
    }
    Ok(true)
}

/// Whether SDL's pixel art mode blends on the canvas's renderer, asked
/// once a renderer.
fn pixel_art_blends(
    known: &mut Option<(*mut SDL_Renderer, bool)>,
    canvas: &Canvas<Window>,
) -> bool {
    let renderer = canvas.raw();
    match *known {
        Some((on, blends)) if on == renderer => blends,
        _ => {
            let blends = match canvas.renderer_name.as_str() {
                "opengl" => glsl_version().is_some_and(|v| v >= (1, 30)),
                name => driver_blends_pixel_art(name),
            };
            *known = Some((renderer, blends));
            blends
        }
    }
}

/// Whether pixel art mode blends on a renderer of this driver. SDL 3.4.18
/// takes the mode on any renderer, but samples with nearest where it has
/// no shader for it: the software renderer, Direct3D 9, and OpenGL ES 2,
/// whose shader needs `fwidth` and `textureGrad`, built only when
/// `OPENGLES_300` is defined, which SDL's build never does. OpenGL has
/// the shader in GLSL 1.30, asked about apart.
fn driver_blends_pixel_art(name: &str) -> bool {
    matches!(
        name,
        "gpu" | "vulkan" | "direct3d11" | "direct3d12" | "metal"
    )
}

/// The GLSL version of the current OpenGL context.
fn glsl_version() -> Option<(u32, u32)> {
    const GL_SHADING_LANGUAGE_VERSION: u32 = 0x8B8C;
    // SAFETY: only for the opengl renderer, on its thread, where a context
    // of that driver is current; the string is GL's, read at once.
    unsafe {
        let get = SDL_GL_GetProcAddress(c"glGetString".as_ptr())?;
        let get: extern "C" fn(u32) -> *const std::ffi::c_char = std::mem::transmute(get);
        let text = get(GL_SHADING_LANGUAGE_VERSION);
        if text.is_null() {
            return None;
        }
        glsl_number(std::ffi::CStr::from_ptr(text).to_str().ok()?)
    }
}

/// The version at the start of GL's version text, such as "4.60 NVIDIA",
/// with the minor version in hundredths: (4, 60).
fn glsl_number(text: &str) -> Option<(u32, u32)> {
    let (major, rest) = text.trim_start().split_once('.')?;
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    let minor: u32 = digits.parse().ok()?;
    let minor = if digits.len() == 1 { minor * 10 } else { minor };
    Some((major.parse().ok()?, minor))
}

#[cfg(test)]
mod tests {
    use super::*;
    use sb_runtime::settings::SimpleUiSize;

    #[test]
    fn whole_scales_stay_nearest() {
        for scale in [1.0, 2.0, 3.0, 4.0] {
            assert_eq!(sampling(scale), Sampling::Nearest, "{scale}");
        }
        assert_eq!(sampling(1.5), Sampling::PixelArt);
        assert_eq!(sampling(1.0005), Sampling::Nearest);
        assert_eq!(sampling(2.25), Sampling::PixelArt);
        assert_eq!(sampling(0.75), Sampling::Linear);
        assert_eq!(sampling(0.5), Sampling::Linear);
        assert_eq!(sampling(0.0), Sampling::Linear);
    }

    #[test]
    fn desktop_windows() {
        let at = |out: (u32, u32)| {
            let display = crate::display_rect(1280, 720, out.0, out.1);
            sampling(display.w / 1280.0)
        };
        assert_eq!(at((1280, 720)), Sampling::Nearest);
        assert_eq!(at((2560, 1440)), Sampling::Nearest);
        assert_eq!(at((3840, 2160)), Sampling::Nearest);
        assert_eq!(at((1920, 1080)), Sampling::PixelArt);
        assert_eq!(at((1920, 1200)), Sampling::PixelArt);
        assert_eq!(at((960, 540)), Sampling::Linear);
    }

    #[test]
    fn simple_ui_stays_nearest_but_at_medium() {
        let outputs = [
            (320, 240),
            (480, 320),
            (640, 480),
            (720, 720),
            (800, 480),
            (854, 480),
            (960, 544),
            (1024, 600),
            (1280, 720),
            (1920, 1080),
            (2560, 1600),
        ];
        for out in outputs {
            for size in SimpleUiSize::ALL {
                let ui = crate::simple_ui::ui_size(out, size);
                let display = crate::simple_ui::display_rect(ui, out, size);
                let scale = display.w / ui.0 as f32;
                assert_eq!(display.h / ui.1 as f32, scale, "{out:?} {size:?}");
                // Medium is a half more than a whole scale where Normal's
                // is odd, and drawn as pixel art then.
                let whole = scale.fract() == 0.0;
                assert!(whole || size == SimpleUiSize::Medium && scale.fract() == 0.5);
                let want = if whole {
                    Sampling::Nearest
                } else {
                    Sampling::PixelArt
                };
                assert_eq!(sampling(scale), want, "{out:?} {size:?}");
            }
        }
    }

    #[test]
    fn drivers_without_the_shader() {
        for name in ["software", "opengles2", "direct3d"] {
            assert!(!driver_blends_pixel_art(name), "{name}");
        }
        for name in ["gpu", "vulkan", "direct3d11", "direct3d12", "metal"] {
            assert!(driver_blends_pixel_art(name), "{name}");
        }
    }

    #[test]
    fn glsl_numbers() {
        assert_eq!(glsl_number("4.60 NVIDIA"), Some((4, 60)));
        assert_eq!(glsl_number("1.30"), Some((1, 30)));
        assert_eq!(glsl_number("1.20 Mesa"), Some((1, 20)));
        assert_eq!(glsl_number("4.6"), Some((4, 60)));
        assert_eq!(glsl_number("unknown"), None);
        assert!(glsl_number("1.20").is_some_and(|v| v < (1, 30)));
    }
}
