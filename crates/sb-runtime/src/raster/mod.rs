//! Rasterizers draw a [`Scene`]. They hold none of SmileBASIC 4's rules for
//! what is shown: those rules are in the scene builder. A rasterizer is
//! called per frame, so the cost of dispatching to one does not matter; each
//! item is drawn through a plain match.
//!
//! [`CpuRasterizer`] is the reference: it needs only memory, and its output
//! is what the tests and headless tools see.

pub mod sprite;
pub mod text;

use crate::compose::{self, Area, Compose};
use crate::filter::FilterOp;
use crate::font::Font;
use crate::framebuffer::Framebuffer;
use crate::graphics::Graphics;
use crate::scene::{Item, LayerPass, Scene};
use crate::screen::cover;
use crate::text::{Cell, SCREENS};

/// The data that a scene refers to instead of copying: the graphic pages,
/// the font and the cells of the text screens, in their state when the
/// scene is drawn.
#[derive(Clone, Copy)]
pub struct RasterInput<'a> {
    pub grp: &'a Graphics,
    pub font: &'a Font,
    pub cells: [&'a [Cell]; SCREENS],
}

pub trait Rasterizer {
    /// Draws the scene.
    fn draw(&mut self, scene: &Scene, input: &RasterInput);
}

/// A rasterizer outside this crate, which may draw a scene in place of
/// [`CpuRasterizer`] and keeps the result itself. The frontend's GPU path
/// is one: this crate does not know what it is.
pub trait SceneBackend {
    /// Draws a scene for a frame of `frame` pixels. Returning false leaves
    /// it to the CPU, for a scene with something the backend cannot draw.
    fn draw(&mut self, scene: &Scene, input: &RasterInput, frame: (u32, u32)) -> bool;

    /// Takes the overlay of the emulator itself, to go over the scene just
    /// drawn: a frame of premultiplied pixels that are clear elsewhere, or
    /// None if there is none. False leaves the frame, with the overlay, to
    /// the CPU.
    fn overlay(&mut self, _frame: Option<&Framebuffer>) -> bool {
        true
    }

    /// Whether [`CpuRasterizer`] is to draw what the backend has drawn as
    /// well, and [`SceneBackend::compare`] to see it.
    fn compares(&self) -> bool {
        false
    }

    /// Receives the frame that the CPU has just drawn for the scene.
    fn compare(&mut self, _cpu: &Framebuffer) {}
}

/// Draws scenes into a [`Framebuffer`].
pub struct CpuRasterizer {
    fb: Framebuffer,
    /// The frame that a composed layer is drawn into first.
    scratch: Framebuffer,
    /// The only part of `scratch` that may not be clear.
    scratch_used: Option<Area>,
    /// Room for the values that drawing a sprite computes.
    room: sprite::Room,
    /// The text screens' characters.
    glyphs: [text::KeptGlyphs; SCREENS],
    /// A layer with LMATRIX's 4x4 form, drawn flat before it is put
    /// through the matrix.
    flat: Framebuffer,
}

impl CpuRasterizer {
    pub fn new(width: u32, height: u32) -> CpuRasterizer {
        CpuRasterizer {
            fb: Framebuffer::new(width, height),
            scratch: Framebuffer::new(width, height),
            scratch_used: Some(Area::all(width, height)),
            room: Default::default(),
            glyphs: Default::default(),
            flat: Framebuffer::new(width, height),
        }
    }

    /// Replaces the frames with new ones of this size, one at a time so
    /// that no more are held at the same time than needed.
    pub fn set_size(&mut self, width: u32, height: u32) {
        self.fb = Framebuffer::new(width, height);
        self.scratch = Framebuffer::new(width, height);
        self.scratch_used = Some(Area::all(width, height));
        self.flat = Framebuffer::new(width, height);
    }

    /// The frame drawn last.
    pub fn frame(&self) -> &Framebuffer {
        &self.fb
    }

    /// For drawing over the frame after a scene.
    pub fn frame_mut(&mut self) -> &mut Framebuffer {
        &mut self.fb
    }
}

impl CpuRasterizer {
    /// Draws the first `count` passes of a scene, for a backend that does the
    /// rest: a direct layer's filter works on everything behind it.
    pub fn draw_first(&mut self, scene: &Scene, input: &RasterInput, count: usize) -> &Framebuffer {
        self.fb.clear(scene.back);
        for pass in &scene.passes[..count] {
            self.draw_pass(scene, input, pass);
        }
        &self.fb
    }

    /// Draws a composed pass, including its filter, on a clear layer, and does
    /// not put the layer on the frame: for a backend that does that.
    pub fn draw_layer(
        &mut self,
        scene: &Scene,
        input: &RasterInput,
        pass: &LayerPass,
    ) -> &Framebuffer {
        self.layer_alone(scene, input, pass);
        &self.scratch
    }

    /// A sparse layer costs only what it holds: only the area the last layer
    /// used is cleared, and the caller composes only the area this returns.
    fn layer_alone(&mut self, scene: &Scene, input: &RasterInput, pass: &LayerPass) -> Area {
        let size = (self.scratch.width(), self.scratch.height());
        if let Some(used) = self.scratch_used.take() {
            if used.covers(size.0, size.1) {
                self.scratch.clear(0);
            } else {
                self.scratch.clear_area(&used);
            }
        }
        let area = layer_area(scene, pass, size);
        self.scratch_used = Some(area);
        let CpuRasterizer {
            scratch,
            flat,
            glyphs,
            room,
            ..
        } = self;
        items_and_filter(scene, input, pass, scratch, flat, glyphs, room);
        area
    }

    #[inline]
    fn draw_pass(&mut self, scene: &Scene, input: &RasterInput, pass: &LayerPass) {
        if pass.compose == Compose::Direct {
            let CpuRasterizer {
                fb,
                flat,
                glyphs,
                room,
                ..
            } = self;
            items_and_filter(scene, input, pass, fb, flat, glyphs, room);
            return;
        }
        let area = self.layer_alone(scene, input, pass);
        let opaque = scene.opaque_frame;
        compose::compose_area(
            &mut self.fb,
            &self.scratch,
            pass.compose,
            pass.color,
            opaque,
            &area,
        );
    }
}

/// The part of a frame a composed pass can change, on the layer and through
/// it on the frame.
fn layer_area(scene: &Scene, pass: &LayerPass, (width, height): (u32, u32)) -> Area {
    let all = Area::all(width, height);
    // A filter works on the whole layer, a 4x4 matrix can reach any part
    // of the frame, and modes 3 and 4 make the whole of a frame opaque if
    // it is not yet.
    if pass.filter != FilterOp::None
        || pass.projection.is_some()
        || (!scene.opaque_frame && matches!(pass.compose, Compose::Multiply | Compose::Screen))
    {
        return all;
    }
    let clip = pass.clip.as_ref();
    let mut area = Area {
        x0: 0,
        y0: 0,
        x1: 0,
        y1: 0,
    };
    for item in scene.items(pass) {
        let part = match item {
            Item::Sprite(s) if s.shows_nothing() => continue,
            Item::Sprite(s) => {
                // Its sides swap for quarter turns.
                let turned = s.attr & crate::sprites::attr::ROTATION & 1 != 0;
                let (w, h) = if turned { (s.h, s.w) } else { (s.w, s.h) };
                let (w, h) = (f64::from(w), f64::from(h));
                Area::of_image(&s.matrix, w, h, clip, (width, height))
            }
            Item::Text(t) if t.empty => continue,
            Item::Text(t) => {
                let (w, h) = (t.width * t.char_size, t.height * t.char_size);
                let (w, h) = (f64::from(w), f64::from(h));
                Area::of_image(&t.matrix, w, h, clip, (width, height))
            }
        };
        area = area.with(&part);
        if area.covers(width, height) {
            return all;
        }
    }
    area
}

/// A pass's items on `target`, and then its filter.
#[inline]
fn items_and_filter(
    scene: &Scene,
    input: &RasterInput,
    pass: &LayerPass,
    target: &mut Framebuffer,
    flat: &mut Framebuffer,
    glyphs: &mut [text::KeptGlyphs; SCREENS],
    room: &mut sprite::Room,
) {
    let &RasterInput { grp, font, cells } = input;
    let sample = scene.sample;
    let clip = pass.clip.as_ref();
    match &pass.projection {
        None => {
            for item in scene.items(pass) {
                match item {
                    Item::Sprite(s) => sprite::draw_in(s, grp, target, clip, None, room),
                    Item::Text(t) => {
                        let (at, kept) = (t.screen as usize, &mut glyphs[t.screen as usize]);
                        text::draw_keeping(t, cells[at], font, grp, target, clip, kept);
                    }
                }
            }
        }
        Some(p) => {
            flat.resize(p.flat_size.0, p.flat_size.1);
            let origin = p.flat_origin;
            let mut flat_drawn = false;
            for item in scene.items(pass) {
                match item {
                    Item::Sprite(s) => {
                        if flat_drawn {
                            compose::project(target, flat, origin, &p.matrix, sample, clip);
                            flat_drawn = false;
                        }
                        sprite::draw_in(s, grp, target, clip, Some((&p.matrix, sample)), room);
                    }
                    Item::Text(t) => {
                        if !flat_drawn {
                            flat.clear(0);
                            flat_drawn = true;
                        }
                        let (at, kept) = (t.screen as usize, &mut glyphs[t.screen as usize]);
                        text::draw_keeping(t, cells[at], font, grp, flat, None, kept);
                    }
                }
            }
            if flat_drawn {
                compose::project(target, flat, origin, &p.matrix, sample, clip);
            }
        }
    }
    (pass.filter).apply(target, sample, pass.compose, &scene.raster_rows);
    if pass.opaque_after_filter {
        for p in target.pixels_mut() {
            *p |= 0xFF00_0000;
        }
    }
}

impl Rasterizer for CpuRasterizer {
    fn draw(&mut self, scene: &Scene, input: &RasterInput) {
        self.fb.clear(scene.back);
        for pass in &scene.passes {
            self.draw_pass(scene, input, pass);
        }
        let fb = &mut self.fb;
        cover(scene.fade, fb.pixels_mut(), scene.dimmed);
        if scene.opaque_end {
            for p in fb.pixels_mut() {
                *p |= 0xFF00_0000;
            }
        }
    }
}
