//! The GPU path, a prototype behind `SB_GPU` (see docs/renderer-design.md,
//! section 6): draws a scene with SDL_Renderer's textured triangles in the
//! place of the CPU rasterizer. A frame it cannot draw goes to the CPU.
//!
//! - `SB_GPU=1` turns it on.

use sb_runtime::raster::{RasterInput, SceneBackend};
use sb_runtime::scene::Scene;
use sdl3::render::Canvas;
use sdl3::video::Window;

/// Why a frame was left to the CPU.
#[derive(Clone, Copy, Debug)]
enum Reason {
    NotWritten,
    /// The hint or completion list goes over it on the CPU.
    Overlay,
}

const REASONS: usize = 2;
const REASON_NAMES: [&str; REASONS] = ["not written", "overlay"];

pub struct Gpu {
    drawn: u64,
    left_to_cpu: [u64; REASONS],
}

impl Gpu {
    /// The path if `SB_GPU` asks for it and `canvas` can take it.
    pub fn from_env(_canvas: &Canvas<Window>) -> Option<Gpu> {
        std::env::var_os("SB_GPU")?;
        Some(Gpu {
            drawn: 0,
            left_to_cpu: [0; REASONS],
        })
    }

    fn leave(&mut self, reason: Reason) {
        self.left_to_cpu[reason as usize] += 1;
    }

    /// Prints what it drew and what it left to the CPU, and why.
    pub fn report(&self) {
        println!("gpu       {} frames drawn", self.drawn);
        for (n, name) in self.left_to_cpu.iter().zip(REASON_NAMES) {
            if *n > 0 {
                println!("gpu       {n} frames on the CPU: {name}");
            }
        }
    }
}

impl SceneBackend for Gpu {
    fn draw(&mut self, _scene: &Scene, _input: &RasterInput, _frame: (u32, u32)) -> bool {
        self.leave(Reason::NotWritten);
        false
    }

    fn not_offered(&mut self) {
        self.leave(Reason::Overlay);
    }
}
