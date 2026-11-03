//! Rasterizers draw a [`Scene`]. They hold none of SmileBASIC 4's rules for
//! what shows: those are in the scene's builder. A rasterizer is called per
//! frame, so dispatching to one costs nothing that matters; each item is
//! drawn through a plain match.
//!
//! [`CpuRasterizer`] is the reference: it needs nothing but memory, and it is
//! what the tests and headless tools see.

pub mod sprite;
pub mod text;

use crate::compose::{self, Compose};
use crate::font::Font;
use crate::framebuffer::Framebuffer;
use crate::graphics::Graphics;
use crate::scene::{Item, Scene};
use crate::screen::cover;
use crate::text::{Cell, SCREENS};

/// What a scene refers to and does not copy: the graphic pages, the font
/// and the cells of the text screens, as they stand when it is drawn.
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

/// Draws scenes into a [`Framebuffer`].
pub struct CpuRasterizer {
    fb: Framebuffer,
    /// Where a layer that is composed is drawn first.
    scratch: Framebuffer,
    /// A layer with LMATRIX's 4x4 form, drawn flat before it is put
    /// through the matrix.
    flat: Framebuffer,
}

impl CpuRasterizer {
    pub fn new(width: u32, height: u32) -> CpuRasterizer {
        CpuRasterizer {
            fb: Framebuffer::new(width, height),
            scratch: Framebuffer::new(width, height),
            flat: Framebuffer::new(width, height),
        }
    }

    /// Starts afresh with frames of this size, one at a time so that no
    /// more are held at once than need be.
    pub fn set_size(&mut self, width: u32, height: u32) {
        self.fb = Framebuffer::new(width, height);
        self.scratch = Framebuffer::new(width, height);
        self.flat = Framebuffer::new(width, height);
    }

    /// The frame drawn last.
    pub fn frame(&self) -> &Framebuffer {
        &self.fb
    }

    /// For what goes over the frame after a scene.
    pub fn frame_mut(&mut self) -> &mut Framebuffer {
        &mut self.fb
    }
}

impl Rasterizer for CpuRasterizer {
    fn draw(&mut self, scene: &Scene, input: &RasterInput) {
        let CpuRasterizer { fb, scratch, flat } = self;
        let &RasterInput { grp, font, cells } = input;
        let sample = scene.sample;
        fb.clear(scene.back);
        for pass in &scene.passes {
            let direct = pass.compose == Compose::Direct;
            let target = match direct {
                true => &mut *fb,
                false => {
                    scratch.clear(0);
                    &mut *scratch
                }
            };
            let clip = pass.clip.as_ref();
            match &pass.projection {
                None => {
                    for item in scene.items(pass) {
                        match item {
                            Item::Sprite(s) => sprite::draw(s, grp, target, clip, None),
                            Item::Text(t) => {
                                let cells = cells[t.screen as usize];
                                text::draw(t, cells, font, grp, target, clip);
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
                                sprite::draw(s, grp, target, clip, Some((&p.matrix, sample)));
                            }
                            Item::Text(t) => {
                                if !flat_drawn {
                                    flat.clear(0);
                                    flat_drawn = true;
                                }
                                let cells = cells[t.screen as usize];
                                text::draw(t, cells, font, grp, flat, None);
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
            if !direct {
                let opaque = scene.opaque_frame;
                compose::compose(fb, scratch, pass.compose, pass.color, opaque);
            }
        }
        cover(scene.fade, fb.pixels_mut(), scene.dimmed);
        if scene.opaque_end {
            for p in fb.pixels_mut() {
                *p |= 0xFF00_0000;
            }
        }
    }
}
