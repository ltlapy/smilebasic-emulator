//! The GPU path, a prototype behind `SB_GPU` (see docs/renderer-design.md,
//! section 6): draws a scene with SDL_Renderer's textured triangles in the
//! place of the CPU rasterizer, on any render driver with no shader of ours.
//! A frame with something it cannot draw goes to the CPU whole; the CPU's
//! pages stay what programs read, and nothing is read back.
//!
//! SmileBASIC 4's sums need no shader. A texel times a vertex's color gives
//! `t*c` and `a*ca`, and fixed-function blending then rounds `t*c*a + d*(1-a)`
//! once, as the CPU's paints do:
//!
//! - an element over the frame or a layer: `SDL_BLENDMODE_BLEND`;
//! - an additive one: `SDL_BLENDMODE_ADD`, which keeps the destination's alpha;
//! - a layer, its premultiplied texels times the color `c*ca`, `ca`: in modes
//!   1 to 3 the premultiplied blend, the premultiplied add, `SDL_BLENDMODE_MUL`,
//!   and in mode 4 a custom blend of `1-dst, 1`.
//!
//! `SB_GPU` says how it runs:
//!
//! - `1`: draws what it can;
//! - `sync`: as well, waits for the GPU after each frame, to time its work;
//! - `check` or `check:N`: every Nth frame it draws (every one by default)
//!   the CPU draws too, and the two are compared, printing the differences at
//!   the end. `SB_GPU_DUMP=PATH` saves the frame that differed most as
//!   `PATH-gpu.pam` and `PATH-cpu.pam`.
//!
//! `SB_GPU_ALONE=1` draws on a renderer of its own with no window, through
//! SDL's GPU driver, to measure or check that driver without a display; the
//! window then shows nothing of it.
//!
//! Mosaic and blur need no shader either: they work on half-size targets by
//! drawing with smoothing and offsets, see [`Gpu::mosaic`] and [`Gpu::blur`].
//!
//! The color filter is the one thing here with a shader of ours, through
//! `SDL_GPURenderState`, so only on SDL's `gpu` driver, on Vulkan. Elsewhere it
//! goes to the CPU, as the raster filter does everywhere.

use std::collections::HashMap;
use std::mem::ManuallyDrop;
use std::ptr::{null, null_mut};
use std::time::{Duration, Instant};

use sb_runtime::compose::{Affine, Compose, Placement};
use sb_runtime::filter::{self, BlurKernel, FilterOp, MosaicGrid};
use sb_runtime::font::FontSize;
use sb_runtime::framebuffer::Framebuffer;
use sb_runtime::graphics::{COUNT_BITS, Clip, Dirty, GRP_SIZE, Graphics, PAGES};
use sb_runtime::raster::{RasterInput, SceneBackend};
use sb_runtime::scene::{Item, LayerPass, Scene, SpriteItem, TextItem};
use sb_runtime::sprites::attr;
use sb_runtime::text::{self, Cell, FONT_PAGE, USER_CHARS};
use sdl3::render::{Canvas, Texture};
use sdl3::sys::blendmode::{
    SDL_BLENDFACTOR_ONE, SDL_BLENDFACTOR_ONE_MINUS_DST_COLOR, SDL_BLENDFACTOR_ONE_MINUS_SRC_ALPHA,
    SDL_BLENDFACTOR_ZERO, SDL_BLENDMODE_ADD, SDL_BLENDMODE_ADD_PREMULTIPLIED, SDL_BLENDMODE_BLEND,
    SDL_BLENDMODE_BLEND_PREMULTIPLIED, SDL_BLENDMODE_MUL, SDL_BLENDMODE_NONE,
    SDL_BLENDOPERATION_ADD, SDL_BlendMode, SDL_ComposeCustomBlendMode,
};
use sdl3::sys::gpu::{
    SDL_CreateGPUShader, SDL_GPU_SHADERFORMAT_SPIRV, SDL_GPU_SHADERSTAGE_FRAGMENT, SDL_GPUDevice,
    SDL_GPUShader, SDL_GPUShaderCreateInfo, SDL_GetGPUDeviceProperties, SDL_GetGPUShaderFormats,
    SDL_PROP_GPU_DEVICE_NAME_STRING, SDL_ReleaseGPUShader,
};
use sdl3::sys::pixels::{SDL_FColor, SDL_PIXELFORMAT_ARGB8888};
use sdl3::sys::properties::SDL_GetStringProperty;
use sdl3::sys::rect::{SDL_FPoint, SDL_Rect};
use sdl3::sys::render::{
    SDL_CreateGPURenderState, SDL_CreateGPURenderer, SDL_CreateTexture, SDL_DestroyGPURenderState,
    SDL_DestroyRenderer, SDL_DestroyTexture, SDL_FlushRenderer, SDL_GPURenderState,
    SDL_GPURenderStateCreateInfo, SDL_GetGPURendererDevice, SDL_RenderClear, SDL_RenderGeometry,
    SDL_RenderPresent, SDL_RenderReadPixels, SDL_Renderer, SDL_SetGPURenderState,
    SDL_SetGPURenderStateFragmentUniforms, SDL_SetRenderClipRect, SDL_SetRenderDrawBlendMode,
    SDL_SetRenderDrawColor, SDL_SetRenderTarget, SDL_SetRenderTextureAddressMode,
    SDL_SetTextureBlendMode, SDL_SetTextureScaleMode, SDL_TEXTURE_ADDRESS_AUTO,
    SDL_TEXTURE_ADDRESS_CLAMP, SDL_TEXTURE_ADDRESS_WRAP, SDL_TEXTUREACCESS_STATIC,
    SDL_TEXTUREACCESS_TARGET, SDL_Texture, SDL_UpdateTexture, SDL_Vertex,
};
use sdl3::sys::surface::{
    SDL_ConvertSurface, SDL_DestroySurface, SDL_LockSurface, SDL_SCALEMODE_LINEAR,
    SDL_SCALEMODE_NEAREST, SDL_ScaleMode, SDL_Surface, SDL_UnlockSurface,
};
use sdl3::sys::video::SDL_GL_GetProcAddress;
use sdl3::video::Window;

/// Why a frame was left to the CPU.
#[derive(Clone, Copy, Debug)]
enum Reason {
    /// The hint or completion list goes over it on the CPU.
    Overlay,
    /// A sub interpreter's frame, or darkened beside a subprogram.
    Sub,
    Raster,
    ColorFilter,
    /// LMATRIX's 4x4 form.
    Projection,
    /// An inverted cell may show on the console.
    Inverted,
    /// The cursor of a line being typed.
    Cursor,
    /// An image reaching off a page that does not repeat.
    OffPage,
    /// The fallback glyphs' atlas is full.
    Atlas,
    /// The renderer refused a texture or a blend.
    Refused,
}

const REASONS: usize = 10;
const REASON_NAMES: [&str; REASONS] = [
    "overlay",
    "sub screen or dimmed",
    "raster filter",
    "color filter",
    "4x4 matrix",
    "inverted cells",
    "line cursor",
    "image off the page",
    "atlas full",
    "renderer refused",
];

/// How many vertices are handed to the driver at a time. SDL holds them all
/// until it flushes, and then makes one draw of those that go together. Drivers
/// keep what it took for a large draw: on NVIDIA's OpenGL, a frame of 224000
/// vertices in one draw held 700 MiB more than the same frame in draws of
/// 60000 or fewer.
const MOST_VERTICES: usize = 30000;

/// How far above a sample is moved, in texels, for a pixel exactly between
/// two rows of a half-size target: the frame's rows count from its bottom on
/// SmileBASIC 4, so that it takes the upper one, where a GPU takes the lower.
/// A pixel not exactly between two is at least half a row of the frame's away
/// from it, in whatever size of half-size row, 1/1440 of a texel on a frame 720
/// high, and has to stay on its side.
const ROW_BIAS: f32 = 1.0 / 4096.0;

const WHITE: SDL_FColor = SDL_FColor {
    r: 1.0,
    g: 1.0,
    b: 1.0,
    a: 1.0,
};

/// What the GPU's textures are measured in.
const PAGE_SIZE: f32 = GRP_SIZE as f32;
/// The cells of the atlas of fallback glyphs, up to the larger font's size.
const ATLAS_CELL: u32 = 16;

/// A page as the GPU holds it.
struct Page {
    texture: *mut SDL_Texture,
    /// Tiles that differ from the page and have not been needed yet.
    pending: Dirty,
    /// Whether any are.
    any_pending: bool,
    /// [`Graphics::changes`] of what it holds, which tells a set of pages from
    /// another too.
    changes: u64,
}

/// What a run of triangles is drawn with.
#[derive(Clone, Copy, PartialEq)]
struct Batch {
    /// Null for a color alone, as GRP-1 is.
    texture: *mut SDL_Texture,
    blend: SDL_BlendMode,
    wrap: bool,
}

/// Time and work over the frames drawn.
#[derive(Default)]
struct Stats {
    drawn: u64,
    /// Walking the scene, making vertices and handing them to SDL.
    build: Duration,
    /// Handing changed pages to SDL.
    pages: Duration,
    page_bytes: u64,
    /// SDL_FlushRenderer: SDL's commands turned into the driver's.
    flush: Duration,
    /// Waiting for the GPU to finish, with `sync`.
    wait: Duration,
    vertices: u64,
    batches: u64,
    targets: u64,
    left_to_cpu: [u64; REASONS],
}

/// How two frames differ, in pixels by the most any channel differs.
#[derive(Default)]
struct Compared {
    frames: u64,
    pixels: u64,
    /// Exactly the same, 1 off, 2 off, more.
    by_diff: [u64; 4],
    alpha_off: u64,
    /// The most any channel differs by, of the colors.
    largest: u32,
    /// The frame with the most pixels differing by more than 2.
    worst: Worst,
    /// How many of those, and how many differing at all, in the frame dumped.
    dumped: (u64, u64),
}

#[derive(Default)]
struct Worst {
    pixels: u64,
    frame: u64,
    /// Where the first of them is, and what the CPU and the GPU have there.
    first: Option<(u32, u32, u32, u32)>,
}

/// The color filter's shader, on a GPU device that can take it.
struct ColorFilter {
    device: *mut SDL_GPUDevice,
    shader: *mut SDL_GPUShader,
    state: *mut SDL_GPURenderState,
}

/// The color filter's compiled shader; see shaders/README.md.
static COLOR_FILTER_SPIRV: &[u8] = include_bytes!("../shaders/color_filter.frag.spv");

impl ColorFilter {
    /// None where the renderer has no GPU device, or one that refuses it: the
    /// filter then stays on the CPU.
    fn new(renderer: *mut SDL_Renderer) -> Option<ColorFilter> {
        // SAFETY: the renderer is valid; the device is its own, the shader's
        // code is copied by the call, and what is made here is released in
        // Drop before them.
        unsafe {
            let device = SDL_GetGPURendererDevice(renderer);
            if device.is_null() {
                sdl3::sys::error::SDL_ClearError();
                return None;
            }
            if SDL_GetGPUShaderFormats(device) & SDL_GPU_SHADERFORMAT_SPIRV == 0 {
                return None;
            }
            let info = SDL_GPUShaderCreateInfo {
                code_size: COLOR_FILTER_SPIRV.len(),
                code: COLOR_FILTER_SPIRV.as_ptr(),
                entrypoint: c"main".as_ptr(),
                format: SDL_GPU_SHADERFORMAT_SPIRV,
                stage: SDL_GPU_SHADERSTAGE_FRAGMENT,
                num_samplers: 1,
                num_storage_textures: 0,
                num_storage_buffers: 0,
                num_uniform_buffers: 1,
                ..Default::default()
            };
            let shader = SDL_CreateGPUShader(device, &info);
            if shader.is_null() {
                eprintln!("SB_GPU: the color filter's shader was refused");
                return None;
            }
            let info = SDL_GPURenderStateCreateInfo {
                fragment_shader: shader,
                ..Default::default()
            };
            let state = SDL_CreateGPURenderState(renderer, &info);
            if state.is_null() {
                SDL_ReleaseGPUShader(device, shader);
                eprintln!("SB_GPU: the color filter's render state was refused");
                return None;
            }
            Some(ColorFilter {
                device,
                shader,
                state,
            })
        }
    }
}

pub struct Gpu {
    renderer: *mut SDL_Renderer,
    /// It is its own renderer, with no window, to destroy with it.
    alone: bool,
    frame: *mut SDL_Texture,
    /// Where a composed layer is drawn first.
    layer: *mut SDL_Texture,
    /// What a filter draws into, which then takes the place of what it read.
    spare: *mut SDL_Texture,
    /// Half the frame's size, for mosaic and blur; made when one is first
    /// used. The second holds what a pass of a blur makes of the first.
    half: [*mut SDL_Texture; 2],
    /// Adds all four channels.
    add_all: SDL_BlendMode,
    /// Takes all four channels down by the source's alpha.
    scale_down: SDL_BlendMode,
    color_filter: Option<ColorFilter>,
    size: (u32, u32),
    pages: [Option<Page>; PAGES],
    atlas: *mut SDL_Texture,
    /// Where each fallback glyph is on the atlas.
    atlas_at: HashMap<(u16, u32), (u32, u32)>,
    screen: SDL_BlendMode,
    verts: Vec<SDL_Vertex>,
    /// How many vertices SDL holds.
    queued: usize,
    batch: Batch,
    /// The scaling of the frame texture.
    scale: Option<SDL_ScaleMode>,
    sync: bool,
    /// The renderer's driver is OpenGL or OpenGL ES.
    on_gl: bool,
    /// Compares every so many frames drawn.
    check_every: Option<u64>,
    dump: Option<String>,
    compared: Compared,
    stats: Stats,
}

fn fcolor(c: u32) -> SDL_FColor {
    let ch = |shift: u32| (c >> shift & 0xFF) as f32 / 255.0;
    SDL_FColor {
        r: ch(16),
        g: ch(8),
        b: ch(0),
        a: ch(24),
    }
}

/// The product of two colors, kept unrounded.
fn fcolor2(c1: u32, c2: u32) -> SDL_FColor {
    let ch = |shift: u32| {
        let (a, b) = ((c1 >> shift & 0xFF) as f32, (c2 >> shift & 0xFF) as f32);
        a * b / (255.0 * 255.0)
    };
    SDL_FColor {
        r: ch(16),
        g: ch(8),
        b: ch(0),
        a: ch(24),
    }
}

/// How far a sample is moved along the image to settle a tie, in texels: more
/// than a GPU's interpolation is off, and not enough to matter elsewhere.
const TIE_BIAS: f64 = 1.0 / 128.0;
/// How far an element that can tie is moved down on the frame, in pixels: a
/// pixel whose center an edge goes through then belongs to the element if
/// the edge is its bottom one, not its top, as on SmileBASIC 4. One step of
/// what GPUs usually snap vertices to.
const COVER_BIAS: f64 = 1.0 / 256.0;

/// How the image is sampled at an element's corners.
#[derive(Clone, Copy)]
struct Sampling {
    /// Added to each corner on the image.
    shift: (f64, f64),
    /// Added to each corner on the frame.
    down: f64,
}

/// The [`Sampling`] of an element placed by `matrix`, `size` long on the
/// image, whose texels are `texel` of its units.
fn sampling(matrix: &Affine, size: (i32, i32), reversed: (bool, bool), texel: f64) -> Sampling {
    const NONE: Sampling = Sampling {
        shift: (0.0, 0.0),
        down: 0.0,
    };
    let place = Placement {
        matrix: *matrix,
        clip: None,
        add: false,
    };
    if place.is_aligned() {
        return NONE;
    }
    let (sx, sy) = place.tie_sides(size, reversed);
    let Some(inv) = matrix.inverse().filter(|_| (sx, sy) != (0.0, 0.0)) else {
        return NONE;
    };
    // Moving the element down moves what lies under each pixel: the first
    // part keeps it where it was.
    Sampling {
        shift: (
            sx * TIE_BIAS * texel + inv.b * COVER_BIAS,
            sy * TIE_BIAS * texel + inv.d * COVER_BIAS,
        ),
        down: COVER_BIAS,
    }
}

impl Gpu {
    /// The path if `SB_GPU` asks for it and `canvas` can take it.
    pub fn from_env(canvas: &Canvas<Window>) -> Option<Gpu> {
        let spec = std::env::var("SB_GPU").ok()?;
        if spec.is_empty() || spec == "0" {
            return None;
        }
        // Drawing triangles there would only be the CPU again, less exactly.
        if canvas.renderer_name == "software" {
            eprintln!("SB_GPU: the software renderer has no GPU; staying on the CPU");
            return None;
        }
        let alone = std::env::var_os("SB_GPU_ALONE").is_some();
        let renderer = match alone {
            // SAFETY: SDL makes the device, with no window to present to.
            true => unsafe { SDL_CreateGPURenderer(null_mut(), null_mut()) },
            false => canvas.raw(),
        };
        if renderer.is_null() {
            eprintln!("SB_GPU: no GPU renderer; staying on the CPU");
            return None;
        }
        let screen = SDL_ComposeCustomBlendMode(
            SDL_BLENDFACTOR_ONE_MINUS_DST_COLOR,
            SDL_BLENDFACTOR_ONE,
            SDL_BLENDOPERATION_ADD,
            SDL_BLENDFACTOR_ZERO,
            SDL_BLENDFACTOR_ONE,
            SDL_BLENDOPERATION_ADD,
        );
        let check_every = match spec.split_once(':') {
            Some(("check", n)) => Some(n.parse().unwrap_or(1).max(1)),
            None if spec == "check" => Some(1),
            _ => None,
        };
        Some(Gpu {
            renderer,
            alone,
            frame: null_mut(),
            layer: null_mut(),
            spare: null_mut(),
            half: [null_mut(); 2],
            add_all: SDL_ComposeCustomBlendMode(
                SDL_BLENDFACTOR_ONE,
                SDL_BLENDFACTOR_ONE,
                SDL_BLENDOPERATION_ADD,
                SDL_BLENDFACTOR_ONE,
                SDL_BLENDFACTOR_ONE,
                SDL_BLENDOPERATION_ADD,
            ),
            scale_down: SDL_ComposeCustomBlendMode(
                SDL_BLENDFACTOR_ZERO,
                SDL_BLENDFACTOR_ONE_MINUS_SRC_ALPHA,
                SDL_BLENDOPERATION_ADD,
                SDL_BLENDFACTOR_ZERO,
                SDL_BLENDFACTOR_ONE_MINUS_SRC_ALPHA,
                SDL_BLENDOPERATION_ADD,
            ),
            color_filter: ColorFilter::new(renderer),
            size: (0, 0),
            pages: [const { None }; PAGES],
            atlas: null_mut(),
            atlas_at: HashMap::new(),
            screen,
            verts: Vec::new(),
            queued: 0,
            batch: Batch {
                texture: null_mut(),
                blend: SDL_BLENDMODE_NONE,
                wrap: false,
            },
            scale: None,
            sync: spec == "sync",
            on_gl: !alone && canvas.renderer_name.starts_with("opengl"),
            check_every,
            dump: std::env::var("SB_GPU_DUMP").ok(),
            compared: Compared::default(),
            stats: Stats::default(),
        })
    }

    fn leave(&mut self, reason: Reason) {
        self.stats.left_to_cpu[reason as usize] += 1;
    }

    /// The frame it drew, to scale as `filter_linear` says. Valid until the
    /// frame's size changes; it is not to be destroyed.
    pub fn view(&mut self, linear: bool) -> Option<ManuallyDrop<Texture<'static>>> {
        if self.frame.is_null() || self.alone {
            return None;
        }
        let scale = match linear {
            true => SDL_SCALEMODE_LINEAR,
            false => SDL_SCALEMODE_NEAREST,
        };
        // SAFETY: the frame texture is one of this renderer's, and lives as
        // long as `self`; ManuallyDrop keeps the wrapper from destroying it.
        unsafe {
            if self.scale != Some(scale) {
                // A filter may have swapped the frame for another target.
                SDL_SetTextureBlendMode(self.frame, SDL_BLENDMODE_NONE);
                SDL_SetTextureScaleMode(self.frame, scale);
                self.scale = Some(scale);
            }
            Some(ManuallyDrop::new(Texture::from_raw(self.frame)))
        }
    }

    /// The device it runs on, for telling results apart, if SDL can say.
    fn device_name(&self) -> String {
        const GL_RENDERER: u32 = 0x1F01;
        // SAFETY: the renderer outlives `self`; the device and the GL function
        // are the ones it uses, on this thread, and the strings they give are
        // copied at once. A renderer not on GL has no such function current,
        // so it is asked for only on those that are.
        unsafe {
            let device = SDL_GetGPURendererDevice(self.renderer);
            if !device.is_null() {
                let props = SDL_GetGPUDeviceProperties(device);
                let name = SDL_GetStringProperty(props, SDL_PROP_GPU_DEVICE_NAME_STRING, null());
                return text_of(name);
            }
            if let Some(get) = SDL_GL_GetProcAddress(c"glGetString".as_ptr()).filter(|_| self.on_gl)
            {
                let get: extern "C" fn(u32) -> *const std::ffi::c_char = std::mem::transmute(get);
                return text_of(get(GL_RENDERER));
            }
        }
        "unknown".to_string()
    }

    /// Prints what it drew and what it left to the CPU, and why.
    pub fn report(&self) {
        println!("gpu       device {}", self.device_name());
        let s = &self.stats;
        let n = s.drawn.max(1);
        let per = |d: Duration| d / n as u32;
        println!(
            "gpu       {} frames drawn: build {:.3?}, pages {:.3?} ({:.1} MB), flush {:.3?}, wait {:.3?}; \
             {} vertices, {} batches, {} target switches a frame",
            s.drawn,
            per(s.build),
            per(s.pages),
            s.page_bytes as f64 / n as f64 / 1e6,
            per(s.flush),
            per(s.wait),
            s.vertices / n,
            s.batches / n,
            s.targets / n,
        );
        for (n, name) in s.left_to_cpu.iter().zip(REASON_NAMES) {
            if *n > 0 {
                println!("gpu       {n} frames on the CPU: {name}");
            }
        }
        let c = &self.compared;
        if c.frames > 0 {
            let [same, one, two, more] = c.by_diff;
            println!(
                "compared  {} frames, {} pixels: same {same}, 1 off {one}, 2 off {two}, more {more}; alpha off {}; at most {} off",
                c.frames, c.pixels, c.alpha_off, c.largest,
            );
            if let Some((x, y, cpu, gpu)) = c.worst.first {
                println!(
                    "compared  worst frame {}: {} pixels more than 2 off, the first at \
                     {x},{y}: CPU {cpu:08X}, GPU {gpu:08X}",
                    c.worst.frame, c.worst.pixels,
                );
            }
        }
    }

    fn target(&mut self, texture: *mut SDL_Texture) {
        self.flush();
        self.stats.targets += 1;
        // SAFETY: the renderer outlives `self`, and the texture is a target
        // of its own or null.
        unsafe { SDL_SetRenderTarget(self.renderer, texture) };
    }

    /// Hands the triangles gathered to SDL.
    fn flush(&mut self) {
        if self.verts.is_empty() {
            return;
        }
        let Batch {
            texture,
            blend,
            wrap,
        } = self.batch;
        let address = match wrap {
            true => SDL_TEXTURE_ADDRESS_WRAP,
            false => SDL_TEXTURE_ADDRESS_CLAMP,
        };
        // SAFETY: the renderer outlives `self`, the texture is one of its
        // own or null, and the vertices are copied by the call.
        unsafe {
            if texture.is_null() {
                SDL_SetRenderDrawBlendMode(self.renderer, blend);
            } else {
                SDL_SetTextureBlendMode(texture, blend);
            }
            SDL_SetRenderTextureAddressMode(self.renderer, address, address);
            SDL_RenderGeometry(
                self.renderer,
                texture,
                self.verts.as_ptr(),
                self.verts.len() as i32,
                null(),
                0,
            );
        }
        self.stats.vertices += self.verts.len() as u64;
        self.stats.batches += 1;
        self.queued += self.verts.len();
        self.verts.clear();
        if self.queued >= MOST_VERTICES {
            self.queued = 0;
            // SAFETY: the renderer outlives `self`.
            unsafe { SDL_FlushRenderer(self.renderer) };
        }
    }

    /// Makes sure what follows goes in a batch of this kind.
    #[inline]
    fn batch(&mut self, batch: Batch) {
        if self.batch != batch {
            self.flush();
            self.batch = batch;
        }
    }

    /// Two triangles; the corners go around the quad.
    #[inline]
    fn quad(&mut self, corners: [(f64, f64); 4], uv: [(f64, f64); 4], color: SDL_FColor) {
        let w = self.size.0 as f64;
        let h = self.size.1 as f64;
        let lo_x = corners.iter().fold(f64::MAX, |m, c| m.min(c.0));
        let hi_x = corners.iter().fold(f64::MIN, |m, c| m.max(c.0));
        let lo_y = corners.iter().fold(f64::MAX, |m, c| m.min(c.1));
        let hi_y = corners.iter().fold(f64::MIN, |m, c| m.max(c.1));
        if hi_x <= 0.0 || hi_y <= 0.0 || lo_x >= w || lo_y >= h {
            return;
        }
        let vertex = |i: usize| SDL_Vertex {
            position: SDL_FPoint {
                x: corners[i].0 as f32,
                y: corners[i].1 as f32,
            },
            color,
            tex_coord: SDL_FPoint {
                x: uv[i].0 as f32 / PAGE_SIZE,
                y: uv[i].1 as f32 / PAGE_SIZE,
            },
        };
        let (a, b, c, d) = (vertex(0), vertex(1), vertex(2), vertex(3));
        self.verts.extend_from_slice(&[a, b, c, a, c, d]);
        if self.verts.len() >= MOST_VERTICES {
            self.flush();
        }
    }

    /// The texture of a page that is not blank, up to date in `area`, or all
    /// over with None: what no element reads is not sent until one does.
    fn page(
        &mut self,
        grp: &Graphics,
        page: usize,
        area: Option<Clip>,
    ) -> Result<*mut SDL_Texture, Reason> {
        let changes = grp.changes(page);
        if self.pages[page].is_none() {
            // SAFETY: the renderer outlives `self`.
            let texture = unsafe {
                SDL_CreateTexture(
                    self.renderer,
                    SDL_PIXELFORMAT_ARGB8888,
                    SDL_TEXTUREACCESS_STATIC,
                    GRP_SIZE,
                    GRP_SIZE,
                )
            };
            if texture.is_null() {
                return Err(Reason::Refused);
            }
            // SAFETY: as it was just made.
            unsafe { SDL_SetTextureScaleMode(texture, SDL_SCALEMODE_NEAREST) };
            self.pages[page] = Some(Page {
                texture,
                pending: Dirty::default(),
                any_pending: false,
                // Of no set of pages, so all of it is to be sent.
                changes: !changes,
            });
        }
        let held = self.pages[page].as_mut().ok_or(Reason::Refused)?;
        if held.changes != changes {
            // Another set of pages counts from elsewhere, and its marks say
            // nothing about what this texture holds.
            let dirty = grp.take_dirty(page);
            match held.changes >> COUNT_BITS == changes >> COUNT_BITS {
                true => held.pending.add(dirty),
                false => held.pending = Dirty::ALL,
            }
            held.any_pending = !held.pending.is_empty();
            held.changes = changes;
        }
        if !held.any_pending {
            return Ok(held.texture);
        }
        let start = Instant::now();
        let send = match area {
            Some(area) => held.pending.take_area(area),
            None => std::mem::take(&mut held.pending),
        };
        held.any_pending = !held.pending.is_empty();
        let pixels = grp.page(page);
        let mut bytes = 0;
        send.for_each_area(|area| {
            let (w, h) = (area.x1 - area.x0 + 1, area.y1 - area.y0 + 1);
            let rect = SDL_Rect {
                x: area.x0,
                y: area.y0,
                w,
                h,
            };
            let first = &pixels[(area.y0 * GRP_SIZE + area.x0) as usize..];
            // SAFETY: the page is GRP_SIZE square, as the texture is, and
            // the rectangle lies on it.
            unsafe { SDL_UpdateTexture(held.texture, &rect, first.as_ptr().cast(), GRP_SIZE * 4) };
            bytes += w as usize * h as usize * 4;
        });
        self.stats.pages += start.elapsed();
        self.stats.page_bytes += bytes as u64;
        if bytes > 0 {
            crate::bench::uploaded(bytes);
        }
        Ok(held.texture)
    }

    /// Where a glyph with no place on the font page is on the atlas.
    fn atlas_glyph(
        &mut self,
        font: &sb_runtime::font::Font,
        code: u16,
        size: FontSize,
    ) -> Result<(u32, u32), Reason> {
        let ft = size as u32;
        if let Some(&at) = self.atlas_at.get(&(code, ft)) {
            return Ok(at);
        }
        if self.atlas.is_null() {
            // SAFETY: the renderer outlives `self`.
            self.atlas = unsafe {
                SDL_CreateTexture(
                    self.renderer,
                    SDL_PIXELFORMAT_ARGB8888,
                    SDL_TEXTUREACCESS_STATIC,
                    GRP_SIZE,
                    GRP_SIZE,
                )
            };
            if self.atlas.is_null() {
                return Err(Reason::Refused);
            }
            // SAFETY: as it was just made.
            unsafe { SDL_SetTextureScaleMode(self.atlas, SDL_SCALEMODE_NEAREST) };
        }
        let across = GRP_SIZE as u32 / ATLAS_CELL;
        let n = self.atlas_at.len() as u32;
        if n >= across * across {
            return Err(Reason::Atlas);
        }
        let at = (n % across * ATLAS_CELL, n / across * ATLAS_CELL);
        let mut pixels = [0_u32; (ATLAS_CELL * ATLAS_CELL) as usize];
        for y in 0..ft {
            for x in 0..ft {
                pixels[(y * ATLAS_CELL + x) as usize] = font.pixel(code, size, x, y);
            }
        }
        let rect = SDL_Rect {
            x: at.0 as i32,
            y: at.1 as i32,
            w: ft as i32,
            h: ft as i32,
        };
        // SAFETY: `pixels` holds the rectangle at this pitch.
        unsafe {
            SDL_UpdateTexture(
                self.atlas,
                &rect,
                pixels.as_ptr().cast(),
                (ATLAS_CELL * 4) as i32,
            )
        };
        self.atlas_at.insert((code, ft), at);
        Ok(at)
    }

    fn sprite(&mut self, s: &SpriteItem, grp: &Graphics) -> Result<(), Reason> {
        if s.shows_nothing() {
            return Ok(());
        }
        let m = &s.matrix;
        let (w, h) = (f64::from(s.w), f64::from(s.h));
        let quarter = s.attr & attr::ROTATION;
        let (bw, bh) = if quarter % 2 == 1 { (h, w) } else { (w, h) };
        let white = s.page < 0;
        let on_page = |at: i32, size: i32| at >= 0 && at + size <= GRP_SIZE;
        if !white && !s.repeat && !(on_page(s.u, s.w) && on_page(s.v, s.h)) {
            return Err(Reason::OffPage);
        }
        // A point of the shown rectangle, on the page. As the CPU's page_xy
        // but for points: the image is flipped, then turned.
        let page_xy = |lx: f64, ly: f64| {
            let (mut ix, mut iy) = match quarter {
                0 => (lx, ly),
                1 => (ly, h - lx),
                2 => (w - lx, h - ly),
                _ => (w - ly, lx),
            };
            if s.attr & attr::REVH != 0 {
                ix = w - ix;
            }
            if s.attr & attr::REVV != 0 {
                iy = h - iy;
            }
            (f64::from(s.u) + ix, f64::from(s.v) + iy)
        };
        let (x, y) = page_xy(0.0, 0.0);
        let (x1, y1) = page_xy(1.0, 0.0);
        let (x2, y2) = page_xy(0.0, 1.0);
        let reversed = (x1 + y1 < x + y, x2 + y2 < x + y);
        let on = sampling(m, (bw as i32, bh as i32), reversed, 1.0);
        let (sx, sy) = on.shift;
        let texture = match usize::try_from(s.page) {
            Ok(p) => {
                let area = (!s.repeat).then(|| Clip::new(s.u, s.v, s.u + s.w - 1, s.v + s.h - 1));
                self.page(grp, p, area)?
            }
            Err(_) => null_mut(),
        };
        let blend = match s.attr & attr::ADD != 0 {
            true => SDL_BLENDMODE_ADD,
            false => SDL_BLENDMODE_BLEND,
        };
        self.batch(Batch {
            texture,
            blend,
            wrap: s.repeat && !white,
        });
        let at = |lx: f64, ly: f64| {
            let (x, y) = m.apply(lx, ly);
            (x, y + on.down)
        };
        self.quad(
            [at(0.0, 0.0), at(bw, 0.0), at(bw, bh), at(0.0, bh)],
            [
                page_xy(sx, sy),
                page_xy(bw + sx, sy),
                page_xy(bw + sx, bh + sy),
                page_xy(sx, bh + sy),
            ],
            fcolor(s.color),
        );
        Ok(())
    }

    fn text(&mut self, t: &TextItem, cells: &[Cell], input: &RasterInput) -> Result<(), Reason> {
        if t.empty {
            return Ok(());
        }
        if t.may_invert {
            return Err(Reason::Inverted);
        }
        if t.cursor.is_some() {
            return Err(Reason::Cursor);
        }
        let m = &t.matrix;
        if t.tint >> 24 == 0 {
            return Ok(());
        }
        let grp = input.grp;
        let (cs, ft) = (f64::from(t.char_size), t.font_type);
        let blend = match t.add {
            true => SDL_BLENDMODE_ADD,
            false => SDL_BLENDMODE_BLEND,
        };
        let size_on_image = (t.char_size as i32, t.char_size as i32);
        let on = sampling(m, size_on_image, (false, false), cs / f64::from(ft));
        // The corners of a cell on its glyph, for each ATTR. As the CPU's
        // draw_turned: the character is flipped, then turned.
        let scale = f64::from(ft) / cs;
        let mut corners = [[(0.0, 0.0); 4]; 16];
        for (a, out) in corners.iter_mut().enumerate() {
            let a = a as u16;
            let point = |lx: f64, ly: f64| {
                let (px, py) = (lx + on.shift.0, ly + on.shift.1);
                let (mut px, mut py) = match a & text::attr::ROTATION {
                    0 => (px, py),
                    1 => (py, cs - px),
                    2 => (cs - px, cs - py),
                    _ => (cs - py, px),
                };
                if a & text::attr::REVH != 0 {
                    px = cs - px;
                }
                if a & text::attr::REVV != 0 {
                    py = cs - py;
                }
                (px * scale, py * scale)
            };
            *out = [
                point(0.0, 0.0),
                point(cs, 0.0),
                point(cs, cs),
                point(0.0, cs),
            ];
        }
        let size = match ft {
            8 => Some(FontSize::Small),
            16 => Some(FontSize::Large),
            _ => None,
        };
        let font_blank = grp.is_blank(FONT_PAGE);
        // TPAGE's page, if it is one with pixels.
        let user_page = usize::try_from(t.page).ok().filter(|&p| p < PAGES);
        let user_blank = user_page.is_some_and(|p| grp.is_blank(p));
        let user_repeat = user_page.is_some_and(|p| grp.sample[p] == 1);
        let mut last_color = (
            0,
            SDL_FColor {
                r: 0.,
                g: 0.,
                b: 0.,
                a: 0.,
            },
        );
        for cy in 0..t.height {
            let row = &cells[(cy * t.width) as usize..][..t.width as usize];
            let y0 = f64::from(cy) * cs;
            for (cx, cell) in row.iter().enumerate() {
                if cell.code == 0 || cell.color >> 24 == 0 {
                    continue;
                }
                let x0 = cx as f64 * cs;
                // Where the character's image is, and on what.
                let (texture, gx, gy, wrap) = if USER_CHARS.contains(&cell.code) {
                    let i = i32::from(cell.code - USER_CHARS.start());
                    let (ox, oy) = t.page_origin;
                    let side = ft as i32;
                    let gx = ox + i % 64 * side;
                    let gy = oy + i / 64 * side;
                    match user_page {
                        // GRP-1, white all over.
                        None if t.page < 0 => (null_mut(), 0, 0, false),
                        None => continue,
                        Some(_) if user_blank => continue,
                        Some(p) => {
                            let on = |at: i32| at >= 0 && at + side <= GRP_SIZE;
                            if !user_repeat && !(on(gx) && on(gy)) {
                                return Err(Reason::OffPage);
                            }
                            (self.page(grp, p, None)?, gx, gy, user_repeat)
                        }
                    }
                } else {
                    let Some(size) = size else { continue };
                    match input.font.origin(cell.code, size) {
                        Some(_) if font_blank => continue,
                        Some((gx, gy)) => (
                            self.page(grp, FONT_PAGE, None)?,
                            gx as i32,
                            gy as i32,
                            false,
                        ),
                        None => {
                            let (gx, gy) = self.atlas_glyph(input.font, cell.code, size)?;
                            (self.atlas, gx as i32, gy as i32, false)
                        }
                    }
                };
                self.batch(Batch {
                    texture,
                    blend,
                    wrap,
                });
                if cell.color != last_color.0 {
                    last_color = (cell.color, fcolor2(cell.color, t.tint));
                }
                let glyph = &corners[usize::from(cell.attr & 0xF)];
                let (gx, gy) = (f64::from(gx), f64::from(gy));
                let at = |x: f64, y: f64| {
                    let (x, y) = m.apply(x, y);
                    (x, y + on.down)
                };
                let uv = |i: usize| (gx + glyph[i].0, gy + glyph[i].1);
                self.quad(
                    [
                        at(x0, y0),
                        at(x0 + cs, y0),
                        at(x0 + cs, y0 + cs),
                        at(x0, y0 + cs),
                    ],
                    [uv(0), uv(1), uv(2), uv(3)],
                    last_color.1,
                );
            }
        }
        Ok(())
    }

    /// Draws a layer's elements on the current target.
    fn items(
        &mut self,
        scene: &Scene,
        pass: &LayerPass,
        input: &RasterInput,
    ) -> Result<(), Reason> {
        let clip = pass.clip.map(|c| SDL_Rect {
            x: c.x0,
            y: c.y0,
            w: c.x1 - c.x0 + 1,
            h: c.y1 - c.y0 + 1,
        });
        if clip.is_some_and(|c| c.w <= 0 || c.h <= 0) {
            return Ok(());
        }
        // SAFETY: the renderer outlives `self`; the rectangle is copied.
        unsafe { SDL_SetRenderClipRect(self.renderer, clip.as_ref().map_or(null(), |c| c)) };
        for item in scene.items(pass) {
            match item {
                Item::Sprite(s) => self.sprite(s, input.grp)?,
                Item::Text(t) => self.text(t, input.cells[t.screen as usize], input)?,
            }
        }
        self.flush();
        // SAFETY: as above.
        unsafe { SDL_SetRenderClipRect(self.renderer, null()) };
        Ok(())
    }

    /// Clears the current target.
    fn clear(&mut self, color: u32) {
        let ch = |shift: u32| (color >> shift & 0xFF) as u8;
        // SAFETY: the renderer outlives `self`.
        unsafe {
            SDL_SetRenderDrawColor(self.renderer, ch(16), ch(8), ch(0), ch(24));
            SDL_RenderClear(self.renderer);
        }
    }

    /// The whole frame with this texture (or color alone) in this color.
    fn cover(&mut self, texture: *mut SDL_Texture, blend: SDL_BlendMode, color: SDL_FColor) {
        let (w, h) = (f64::from(self.size.0), f64::from(self.size.1));
        self.batch(Batch {
            texture,
            blend,
            wrap: false,
        });
        let corners = [(0.0, 0.0), (w, 0.0), (w, h), (0.0, h)];
        let uv = corners.map(|(x, y)| (x / w * f64::from(PAGE_SIZE), y / h * f64::from(PAGE_SIZE)));
        self.quad(corners, uv, color);
        self.flush();
    }

    fn ready(&mut self, (w, h): (u32, u32)) -> Result<(), Reason> {
        if self.size == (w, h) && !self.frame.is_null() {
            return Ok(());
        }
        self.scale = None;
        // SAFETY: the renderer outlives `self`; the old targets are its own
        // and not in use between frames.
        unsafe {
            let [half0, half1] = self.half;
            for texture in [self.frame, self.layer, self.spare, half0, half1] {
                if !texture.is_null() {
                    SDL_DestroyTexture(texture);
                }
            }
            self.half = [null_mut(); 2];
            let make = || {
                SDL_CreateTexture(
                    self.renderer,
                    SDL_PIXELFORMAT_ARGB8888,
                    SDL_TEXTUREACCESS_TARGET,
                    w as i32,
                    h as i32,
                )
            };
            self.frame = make();
            self.layer = make();
            self.spare = make();
            if self.frame.is_null() || self.layer.is_null() || self.spare.is_null() {
                return Err(Reason::Refused);
            }
            for texture in [self.frame, self.layer, self.spare] {
                SDL_SetTextureBlendMode(texture, SDL_BLENDMODE_NONE);
                SDL_SetTextureScaleMode(texture, SDL_SCALEMODE_NEAREST);
            }
        }
        self.size = (w, h);
        Ok(())
    }

    /// Why a scene needs the CPU, as far as a look over it shows.
    fn refuses(&self, scene: &Scene) -> Option<Reason> {
        if scene.dimmed || !scene.opaque_frame || scene.opaque_end {
            return Some(Reason::Sub);
        }
        for pass in &scene.passes {
            match pass.filter {
                FilterOp::None | FilterOp::Mosaic(_) | FilterOp::Blur(_) => {}
                FilterOp::Raster { .. } => return Some(Reason::Raster),
                FilterOp::Color { .. } if self.color_filter.is_none() => {
                    return Some(Reason::ColorFilter);
                }
                FilterOp::Color { .. } => {}
            }
            if pass.projection.is_some() {
                return Some(Reason::Projection);
            }
        }
        None
    }

    fn scene(&mut self, scene: &Scene, input: &RasterInput) -> Result<(), Reason> {
        // SAFETY: the renderer outlives `self`.
        unsafe {
            SDL_SetRenderTextureAddressMode(
                self.renderer,
                SDL_TEXTURE_ADDRESS_CLAMP,
                SDL_TEXTURE_ADDRESS_CLAMP,
            )
        };
        self.target(self.frame);
        self.clear(scene.back);
        for pass in &scene.passes {
            if pass.compose == Compose::Direct {
                self.items(scene, pass, input)?;
                self.filter(scene, pass)?;
                continue;
            }
            self.target(self.layer);
            self.clear(0);
            self.items(scene, pass, input)?;
            self.filter(scene, pass)?;
            self.target(self.frame);
            let blend = match pass.compose {
                Compose::Add => SDL_BLENDMODE_ADD_PREMULTIPLIED,
                Compose::Multiply => SDL_BLENDMODE_MUL,
                Compose::Screen => self.screen,
                _ => SDL_BLENDMODE_BLEND_PREMULTIPLIED,
            };
            let c = fcolor(pass.color);
            let color = SDL_FColor {
                r: c.r * c.a,
                g: c.g * c.a,
                b: c.b * c.a,
                a: c.a,
            };
            // SAFETY: the texture is the renderer's own.
            if !unsafe { SDL_SetTextureBlendMode(self.layer, blend) } {
                return Err(Reason::Refused);
            }
            self.cover(self.layer, blend, color);
        }
        if scene.fade >> 24 != 0 {
            self.cover(null_mut(), SDL_BLENDMODE_BLEND, fcolor(scene.fade));
        }
        self.flush();
        Ok(())
    }

    /// A layer's filter, on the current target, which is the frame for a
    /// direct layer and the layer's own otherwise. The raster filter does not
    /// come here: refuses() left it to the CPU.
    fn filter(&mut self, scene: &Scene, pass: &LayerPass) -> Result<(), Reason> {
        let direct = pass.compose == Compose::Direct;
        let from = if direct { self.frame } else { self.layer };
        match pass.filter {
            // Amount 0 leaves the layer as it is, not halved.
            FilterOp::None | FilterOp::Raster { .. } | FilterOp::Blur(0) => Ok(()),
            FilterOp::Mosaic(size) => {
                let (w, h) = self.size;
                match filter::mosaic_grid(w, h, size, scene.sample) {
                    Some(grid) => self.mosaic(from, grid),
                    None => Ok(()),
                }
            }
            // A direct layer is on the frame, which refuses() saw is opaque.
            FilterOp::Blur(amount) => self.blur(from, &filter::blur_kernel(amount), direct),
            FilterOp::Color {
                hue,
                saturation,
                value,
            } => {
                self.color_filter(pass, hue, saturation, value);
                Ok(())
            }
        }
    }

    /// Makes the half-size targets if they are not there yet.
    fn half_targets(&mut self) -> Result<(), Reason> {
        if !self.half[0].is_null() {
            return Ok(());
        }
        let (w, h) = (self.size.0 / 2, self.size.1 / 2);
        for i in 0..2 {
            // SAFETY: the renderer outlives `self`.
            let texture = unsafe {
                SDL_CreateTexture(
                    self.renderer,
                    SDL_PIXELFORMAT_ARGB8888,
                    SDL_TEXTUREACCESS_TARGET,
                    w as i32,
                    h as i32,
                )
            };
            if texture.is_null() {
                return Err(Reason::Refused);
            }
            self.half[i] = texture;
            // SAFETY: as it was just made.
            let takes = unsafe {
                SDL_SetTextureScaleMode(texture, SDL_SCALEMODE_NEAREST);
                SDL_SetTextureBlendMode(texture, self.add_all)
                    && SDL_SetRenderDrawBlendMode(self.renderer, self.scale_down)
            };
            if !takes {
                return Err(Reason::Refused);
            }
        }
        Ok(())
    }

    /// A rectangle of the current target, `to` as left, top, right, bottom,
    /// filled with a rectangle of a texture, `from` in the same order with
    /// the texture 1 long each way, or with the color alone.
    fn paint(
        &mut self,
        texture: *mut SDL_Texture,
        blend: SDL_BlendMode,
        color: SDL_FColor,
        to: [f32; 4],
        from: [f32; 4],
    ) {
        self.batch(Batch {
            texture,
            blend,
            wrap: false,
        });
        let vertex = |x: usize, y: usize| SDL_Vertex {
            position: SDL_FPoint { x: to[x], y: to[y] },
            color,
            tex_coord: SDL_FPoint {
                x: from[x],
                y: from[y],
            },
        };
        let (a, b, c, d) = (vertex(0, 1), vertex(2, 1), vertex(2, 3), vertex(0, 3));
        self.verts.extend_from_slice(&[a, b, c, a, c, d]);
        self.flush();
    }

    fn scale_mode(&mut self, texture: *mut SDL_Texture, mode: SDL_ScaleMode) {
        // SAFETY: the texture is one of the renderer's own, and each draw
        // takes its scale mode when it is queued.
        unsafe { SDL_SetTextureScaleMode(texture, mode) };
        // The frame's, if it was the frame, is to be set again when shown.
        self.scale = None;
    }

    /// The size of the half-size targets, as floats.
    fn half_size(&self) -> (f32, f32) {
        ((self.size.0 / 2) as f32, (self.size.1 / 2) as f32)
    }

    /// LFILTER's mosaic with what [`filter`] says for the rest: the frame is
    /// drawn with smoothing into a part of a half-size target, whose bottom
    /// is where the frame's is, as rows count from there, and that part is
    /// drawn back without.
    fn mosaic(&mut self, from: *mut SDL_Texture, grid: MosaicGrid) -> Result<(), Reason> {
        self.half_targets()?;
        let small = self.half[0];
        let (q, qy) = (grid.q as f32, grid.qy as f32);
        let (across, down) = (grid.across as f32, grid.down as f32);
        self.target(small);
        self.scale_mode(from, SDL_SCALEMODE_LINEAR);
        // What lies past the frame, where the part has a whole number of
        // pixels and the frame does not fill them, reads the frame's edge.
        let top = down - qy;
        self.paint(
            from,
            SDL_BLENDMODE_NONE,
            WHITE,
            [0.0, 0.0, across, down],
            [0.0, -top / qy, across / q, 1.0],
        );
        self.scale_mode(from, SDL_SCALEMODE_NEAREST);
        self.target(from);
        let (w, h) = (self.size.0 as f32, self.size.1 as f32);
        let (hw, hh) = self.half_size();
        self.paint(
            small,
            SDL_BLENDMODE_NONE,
            WHITE,
            [0.0, 0.0, w, h],
            [0.0, (top - ROW_BIAS) / hh, q / hw, (down - ROW_BIAS) / hh],
        );
        self.target(from);
        Ok(())
    }

    /// LFILTER's blur with no shader. The CPU's and SmileBASIC 4's weighted sum
    /// of up to nine taps is rounded once, each pass; here every draw rounds,
    /// to 8 bits. To keep that from adding up, the taps do not add: each mixes
    /// into what is there by its share of all those so far, the outermost first,
    /// so a run of one color stays exact however many draws there are, and later
    /// draws shrink what earlier ones got wrong. The sum of the weights, which
    /// is less than 1 when the kernel is cut off, comes last, once. Two taps side by
    /// side are one draw, by smoothing between them.
    fn blur(
        &mut self,
        from: *mut SDL_Texture,
        kernel: &BlurKernel,
        opaque: bool,
    ) -> Result<(), Reason> {
        self.half_targets()?;
        let [first, second] = self.half;
        let (hw, hh) = self.half_size();
        self.target(first);
        self.scale_mode(from, SDL_SCALEMODE_LINEAR);
        let (whole, all) = ([0.0, 0.0, hw, hh], [0.0, 0.0, 1.0, 1.0]);
        self.paint(from, SDL_BLENDMODE_NONE, WHITE, whole, all);
        self.scale_mode(from, SDL_SCALEMODE_NEAREST);
        self.scale_mode(first, SDL_SCALEMODE_LINEAR);
        self.scale_mode(second, SDL_SCALEMODE_LINEAR);
        self.blur_pass(first, second, (1.0 / hw, 0.0), kernel, opaque);
        self.blur_pass(second, first, (0.0, 1.0 / hh), kernel, opaque);
        self.scale_mode(first, SDL_SCALEMODE_NEAREST);
        self.scale_mode(second, SDL_SCALEMODE_NEAREST);
        self.target(from);
        let (w, h) = (self.size.0 as f32, self.size.1 as f32);
        let (up, down) = (-ROW_BIAS / hh, 1.0 - ROW_BIAS / hh);
        self.paint(
            first,
            SDL_BLENDMODE_NONE,
            WHITE,
            [0.0, 0.0, w, h],
            [0.0, up, 1.0, down],
        );
        self.target(from);
        Ok(())
    }

    /// One pass of [`Gpu::blur`], `step` long on the texture from tap to tap.
    /// The texture's alpha decides nothing about how it mixes if it is all
    /// opaque, which takes one draw for each. Otherwise it takes two, one to
    /// take what is there down and one to add the tap.
    fn blur_pass(
        &mut self,
        from: *mut SDL_Texture,
        to: *mut SDL_Texture,
        step: (f32, f32),
        kernel: &BlurKernel,
        opaque: bool,
    ) {
        let (hw, hh) = self.half_size();
        let whole = [0.0, 0.0, hw, hh];
        self.target(to);
        let mut done = 0;
        for (at, weight) in blur_draws(kernel).into_iter().flatten() {
            let tap = [
                at * step.0,
                at * step.1,
                1.0 + at * step.0,
                1.0 + at * step.1,
            ];
            done += weight;
            let part = weight as f32 / done as f32;
            if done == weight {
                self.paint(from, SDL_BLENDMODE_NONE, WHITE, whole, tap);
            } else if opaque {
                let color = SDL_FColor { a: part, ..WHITE };
                self.paint(from, SDL_BLENDMODE_BLEND, color, whole, tap);
            } else {
                let down = SDL_FColor {
                    r: 0.0,
                    g: 0.0,
                    b: 0.0,
                    a: part,
                };
                self.paint(null_mut(), self.scale_down, down, whole, whole);
                let color = SDL_FColor {
                    r: part,
                    g: part,
                    b: part,
                    a: part,
                };
                self.paint(from, self.add_all, color, whole, tap);
            }
        }
        if kernel.total < 1 << 16 {
            // What shows through falls with the light: black over it at what
            // the weights fall short by does both.
            let short = SDL_FColor {
                r: 0.0,
                g: 0.0,
                b: 0.0,
                a: 1.0 - kernel.total as f32 / 65536.0,
            };
            self.paint(
                null_mut(),
                SDL_BLENDMODE_BLEND_PREMULTIPLIED,
                short,
                whole,
                whole,
            );
        }
    }

    /// The color filter, with the shader.
    fn color_filter(&mut self, pass: &LayerPass, hue: f64, saturation: f64, value: f64) {
        let Some(state) = self.color_filter.as_ref().map(|c| c.state) else {
            return;
        };
        // Left at zero it changes nothing, as on the CPU.
        if hue.rem_euclid(360.0) == 0.0 && saturation == 0.0 && value == 0.0 {
            return;
        }
        let direct = pass.compose == Compose::Direct;
        let from = if direct { self.frame } else { self.layer };
        let keep_clear = pass.compose == Compose::Normal;
        let shift: [f32; 4] = [
            (hue / 360.0) as f32,
            (saturation / 255.0) as f32,
            (value / 255.0) as f32,
            f32::from(u8::from(keep_clear)),
        ];
        self.target(self.spare);
        let white = fcolor(0xFFFF_FFFF);
        // SAFETY: the state and textures are this renderer's own. The uniforms
        // are read when SDL's queue runs, so it runs before they can change.
        unsafe {
            SDL_SetGPURenderStateFragmentUniforms(
                state,
                0,
                shift.as_ptr().cast(),
                size_of_val(&shift) as u32,
            );
            SDL_SetGPURenderState(self.renderer, state);
            self.cover(from, SDL_BLENDMODE_NONE, white);
            SDL_SetGPURenderState(self.renderer, null_mut());
            SDL_FlushRenderer(self.renderer);
        }
        let spare = self.spare;
        self.spare = from;
        match direct {
            true => self.frame = spare,
            false => self.layer = spare,
        }
        self.scale = None;
        self.target(if direct { self.frame } else { self.layer });
    }

    /// Back to drawing on the window, as the frontend expects.
    fn done(&mut self) {
        self.verts.clear();
        self.target(null_mut());
        // SAFETY: the renderer outlives `self`.
        unsafe {
            SDL_SetRenderClipRect(self.renderer, null());
            SDL_SetRenderDrawBlendMode(self.renderer, SDL_BLENDMODE_NONE);
            SDL_SetRenderTextureAddressMode(
                self.renderer,
                SDL_TEXTURE_ADDRESS_AUTO,
                SDL_TEXTURE_ADDRESS_AUTO,
            );
        }
    }

    /// The frame as the GPU has it, or None where it cannot be read.
    fn read_frame(&mut self, rect: Option<SDL_Rect>) -> Option<*mut SDL_Surface> {
        self.target(self.frame);
        // SAFETY: the renderer outlives `self`.
        let read =
            unsafe { SDL_RenderReadPixels(self.renderer, rect.as_ref().map_or(null(), |r| r)) };
        self.target(null_mut());
        (!read.is_null()).then_some(read)
    }
}

impl SceneBackend for Gpu {
    fn draw(&mut self, scene: &Scene, input: &RasterInput, frame: (u32, u32)) -> bool {
        if let Some(reason) = self.refuses(scene) {
            self.leave(reason);
            return false;
        }
        if let Err(reason) = self.ready(frame) {
            self.leave(reason);
            return false;
        }
        let start = Instant::now();
        let pages_before = self.stats.pages;
        let result = self.scene(scene, input);
        self.done();
        if let Err(reason) = result {
            self.leave(reason);
            return false;
        }
        self.stats.build += start
            .elapsed()
            .saturating_sub(self.stats.pages - pages_before);
        let start = Instant::now();
        // SAFETY: the renderer outlives `self`. With no window, a present
        // only hands the work to the GPU.
        unsafe {
            SDL_FlushRenderer(self.renderer);
            self.queued = 0;
            if self.alone {
                SDL_RenderPresent(self.renderer);
            }
        }
        self.stats.flush += start.elapsed();
        self.stats.drawn += 1;
        if self.sync {
            let start = Instant::now();
            let pixel = SDL_Rect {
                x: 0,
                y: 0,
                w: 1,
                h: 1,
            };
            if let Some(read) = self.read_frame(Some(pixel)) {
                // SAFETY: the surface is ours, from the read.
                unsafe { SDL_DestroySurface(read) };
            }
            self.stats.wait += start.elapsed();
        }
        true
    }

    fn not_offered(&mut self) {
        self.leave(Reason::Overlay);
    }

    fn compares(&self) -> bool {
        self.check_every
            .is_some_and(|n| self.stats.drawn.is_multiple_of(n))
    }

    fn compare(&mut self, cpu: &Framebuffer) {
        let Some(read) = self.read_frame(None) else {
            eprintln!("SB_GPU: could not read the frame back");
            return;
        };
        // SAFETY: the surface is ours, from the read; it is converted to the
        // CPU's own layout, locked, and read within its pitch and size.
        unsafe {
            let shot = SDL_ConvertSurface(read, SDL_PIXELFORMAT_ARGB8888);
            SDL_DestroySurface(read);
            if shot.is_null() || !SDL_LockSurface(shot) {
                eprintln!("SB_GPU: could not convert the frame read back");
                return;
            }
            let (w, h, pitch) = (
                (*shot).w as usize,
                (*shot).h as usize,
                (*shot).pitch as usize,
            );
            let bytes = (*shot).pixels.cast::<u8>();
            let number = self.stats.drawn;
            let row = |y: usize| std::slice::from_raw_parts(bytes.add(y * pitch).cast::<u32>(), w);
            let mut more = 0;
            let mut first = None;
            let c = &mut self.compared;
            c.frames += 1;
            let before = (c.pixels, c.by_diff[0]);
            let mut dumped = c.dumped;
            if (w, h) == (cpu.width() as usize, cpu.height() as usize) {
                for (y, want) in cpu.pixels().chunks(w).enumerate() {
                    for (x, (&cpu, &gpu)) in want.iter().zip(row(y)).enumerate() {
                        c.pixels += 1;
                        if cpu == gpu {
                            c.by_diff[0] += 1;
                            continue;
                        }
                        let off = |shift: u32| (cpu >> shift & 0xFF).abs_diff(gpu >> shift & 0xFF);
                        let most = off(0).max(off(8)).max(off(16));
                        c.alpha_off += u64::from(off(24) != 0);
                        c.by_diff[most.min(3) as usize] += 1;
                        c.largest = c.largest.max(most);
                        if most > 2 {
                            more += 1;
                            first.get_or_insert((x as u32, y as u32, cpu, gpu));
                        }
                    }
                }
            }
            let all_off = c.pixels - before.0 - (c.by_diff[0] - before.1);
            let dump_now = (more, all_off) > dumped;
            dumped = dumped.max((more, all_off));
            if more > c.worst.pixels {
                c.worst = Worst {
                    pixels: more,
                    frame: number,
                    first,
                };
            }
            if dump_now {
                if let Some(path) = &self.dump {
                    let gpu: Vec<u32> = (0..h).flat_map(|y| row(y).iter().copied()).collect();
                    save_pam(&format!("{path}-gpu.pam"), w, h, &gpu);
                    save_pam(&format!("{path}-cpu.pam"), w, h, cpu.pixels());
                }
            }
            self.compared.dumped = dumped;
            SDL_UnlockSurface(shot);
            SDL_DestroySurface(shot);
        }
    }
}

/// The draws of a pass of a blur in the order they mix, up to five: how far
/// from the middle in texels, and the weight in 16.16 of what the draw reads.
fn blur_draws(kernel: &BlurKernel) -> [Option<(f32, u32)>; 5] {
    let w = kernel.weights;
    // The nearer of two taps at `k` and the further just past it, as one read
    // between them, if there is anything to read.
    let pair = |k: usize, far: u32| {
        let weight = w[k] + far;
        (weight > 0).then(|| (k as f32 + far as f32 / weight as f32, weight))
    };
    let mut draws = [None; 5];
    let outer = match kernel.reach {
        4 => pair(3, w[4]),
        3 => pair(3, 0),
        _ => None,
    };
    let inner = match kernel.reach {
        1 => pair(1, 0),
        _ => pair(1, w[2]),
    };
    let mirrored = |d: Option<(f32, u32)>| d.map(|(at, weight)| (-at, weight));
    draws[0] = outer;
    draws[1] = mirrored(outer);
    draws[2] = inner;
    draws[3] = mirrored(inner);
    draws[4] = Some((0.0, w[0]));
    draws
}

/// A C string as text, if it is one.
///
/// # Safety
/// `s` is null or a valid C string.
unsafe fn text_of(s: *const std::ffi::c_char) -> String {
    if s.is_null() {
        return "unknown".to_string();
    }
    // SAFETY: as the caller says.
    unsafe { std::ffi::CStr::from_ptr(s) }
        .to_string_lossy()
        .into_owned()
}

/// Saves `0xAARRGGBB` pixels in the form the bench's shots have.
fn save_pam(path: &str, w: usize, h: usize, pixels: &[u32]) {
    let head =
        format!("P7\nWIDTH {w}\nHEIGHT {h}\nDEPTH 4\nMAXVAL 255\nTUPLTYPE RGB_ALPHA\nENDHDR\n");
    let mut out = head.into_bytes();
    for p in pixels {
        out.extend_from_slice(&[(p >> 16) as u8, (p >> 8) as u8, *p as u8, (p >> 24) as u8]);
    }
    if let Err(e) = std::fs::write(path, out) {
        eprintln!("SB_GPU: {e}");
    }
}

impl Drop for Gpu {
    fn drop(&mut self) {
        let pages = self.pages.iter().flatten().map(|p| p.texture);
        let [half0, half1] = self.half;
        let all = [self.frame, self.layer, self.spare, half0, half1, self.atlas];
        // SAFETY: these are the renderer's own, and the frontend drops `self`
        // before it.
        for texture in pages.chain(all) {
            if !texture.is_null() {
                unsafe { SDL_DestroyTexture(texture) };
            }
        }
        // SAFETY: these were made in ColorFilter::new, and the renderer, made
        // for this alone, is not used after this.
        unsafe {
            if let Some(c) = self.color_filter.take() {
                SDL_DestroyGPURenderState(c.state);
                SDL_ReleaseGPUShader(c.device, c.shader);
            }
            if self.alone {
                SDL_DestroyRenderer(self.renderer);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scaled(scale: f64, tx: f64) -> Affine {
        Affine {
            a: scale,
            d: scale,
            tx,
            ty: tx,
            ..Affine::IDENTITY
        }
    }

    /// All of a kernel's weight is read, each tap at its place.
    #[test]
    fn a_blur_reads_every_tap_once() {
        for amount in [1, 2, 8, 16, 32, 64, 128, 384, 1000] {
            let kernel = filter::blur_kernel(amount);
            let draws: Vec<_> = blur_draws(&kernel).into_iter().flatten().collect();
            assert_eq!(draws.iter().map(|d| d.1).sum::<u32>(), kernel.total);
            for side in [1.0, -1.0] {
                // The sum of weight times place, on one side.
                let moment: f64 = (draws.iter().filter(|d| d.0 * side > 0.0))
                    .map(|d| f64::from(d.0.abs()) * f64::from(d.1))
                    .sum();
                let want: f64 = (1..=kernel.reach)
                    .map(|k| k as f64 * f64::from(kernel.weights[k]))
                    .sum();
                assert!((moment - want).abs() < want.max(1.0) * 1e-5, "{amount}");
            }
            assert_eq!(draws.last().map(|d| d.0), Some(0.0));
        }
    }

    #[test]
    fn what_cannot_tie_is_sampled_as_it_is() {
        let on = sampling(&scaled(1.0, 3.0), (16, 16), (false, false), 1.0);
        assert_eq!((on.shift, on.down), ((0.0, 0.0), 0.0));
        let turn = (0.3_f64.sin(), 0.3_f64.cos());
        let m = Affine {
            a: turn.1,
            b: -turn.0,
            c: turn.0,
            d: turn.1,
            ..Affine::IDENTITY
        };
        let on = sampling(&m, (16, 16), (false, false), 1.0);
        assert_eq!((on.shift, on.down), ((0.0, 0.0), 0.0));
    }

    /// A power of two at a scale of 1, on a half pixel: the higher texel, and
    /// moving the element down does not move what lies under a pixel.
    #[test]
    fn a_tie_goes_as_the_cpu_rule_says() {
        let on = sampling(&scaled(1.0, 3.5), (16, 16), (false, false), 1.0);
        assert_eq!(on.down, COVER_BIAS);
        assert_eq!(on.shift.0, TIE_BIAS);
        assert_eq!(on.shift.1, TIE_BIAS + COVER_BIAS);
    }
}
