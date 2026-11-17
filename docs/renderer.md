# Renderer

How the emulator makes a frame: the data it starts from, the scene, the CPU
rasterizer, the GPU path, the frontend, and how far the two drawing paths may
differ. The GPU path is the default where its start-up self-test passes.

- [renderer-verification.md](renderer-verification.md) lists each rendering rule
  with its evidence, the test results and how they were measured.
- [renderer-decisions.md](renderer-decisions.md) says why the GPU path takes the
  form it does.

Contents:

- [Frame data](#frame-data)
- [Making a frame](#making-a-frame)
- [Drawing one element](#drawing-one-element)
- [Layers, projection, filters](#layers-projection-filters)
- [Scene and rasterizers](#scene-and-rasterizers)
- [Change tracking](#change-tracking)
- [GRP pages](#grp-pages)
- [What stays on the CPU](#what-stays-on-the-cpu)
- [Who reads pixels back](#who-reads-pixels-back)
- [Frontend](#frontend)
- [Headless tools](#headless-tools)
- [GPU path](#gpu-path)
- [GPU path settings](#gpu-path-settings)
- [Driver support](#driver-support)
- [Failure handling](#failure-handling)
- [Exactness policy](#exactness-policy)
- [Open work](#open-work)

## Frame data

All of it lives in `Runtime` (`crates/sb-runtime/src/runtime.rs`):

| Data | Type / file | Notes |
|---|---|---|
| GRP0-GRP5 | `Graphics` (`graphics.rs`) | See below. |
| GRP5 font page | `text::FONT_PAGE`, `Runtime::with_font_page` | The imported font is copied into GRP5 at start-up, where programs can change it. Glyphs with no place on GRP5 come from `Font::pixel` (`font.rs`). |
| GRP4 sprite page | `text::SPRITE_PAGE`, `Runtime::set_sprite_page` | Default sprite image, imported from the user's copy. |
| Sprites 0-4095 | `Sprites` (`sprites.rs`) | `Sprite` holds U, V, W, H, attributes, home, position, Z, rotation, scale, color, page, layer, link. Sprite 4095 (`#GSPRITE`) shows GRP0. |
| Text screens 0-4 | `TextScreen` (`text.rs`) | Cells (code, color, ATTR), font type, char size, TOFS/THOME/TROT/TSCALE/TCOLOR, TBLEND, TLAYER, TPAGE. Screen 4 is the console. |
| Layers 0-7 | `compose::Layer` | `LAYER` compose mode and color, `LCLIP`, 2D `LMATRIX` (`Affine`), 4x4 `LMATRIX` (`projection: Option<[f64; 16]>`, values stored at single precision). |
| Filters 0-7 | `filter::Filter` | `LFILTER`: mosaic, blur, raster (holds a copy of the program's array, taken at `LFILTER`), color. |
| BACKCOLOR, FADE | `Runtime::back_color`, `screen::Fade` | |
| XSCREEN | `screen::ScreenMode` | Width, height, sample magnification, display filter, aspect. The frame is `width*sample` by `height*sample` (`ScreenMode::frame`). |

GRP pages are 6 pages of 2048x2048 `u32` (`0xAARRGGBB`). A page takes memory
only once something is drawn on it (`Graphics::is_blank`). Each page has a
change counter (`Graphics::changes`). GARRAY arrays mirror pages; the shown page
takes their writes on `GUPDATE`.

`page::Page` (`page.rs`) owns a page's buffer. Its rows lie `page::STRIDE`
pixels apart, 2112, so that the same column of successive rows does not fall
in one set of a small cache
([benchmark-results.md](benchmark-results.md#grp-rows-padded)).
That makes a page 16.5 MiB (2048 x 2112 x 4 bytes) instead of 16 MiB. The 64
pixels after each row are never written or read. Code takes rows from `Page`
as slices of 2048 pixels (`row`, `rows`, `spans`); the sprite rasterizer and
GPU uploads step through the buffer by `page::at` and `page::STRIDE`. Files,
GARRAY arrays, `GSAVE` and `GLOAD` keep SmileBASIC's layout of 2048 pixels a
row, copied a row at a time.

SmileBASIC 4 has no BG layer (SmileBASIC 3's BG screens are not part of it), so
the emulator has none. Programs make tile maps from text screens with `TPAGE` or
from sprites.

## Making a frame

`Machine::render` (`machine.rs`) gives the editor's frame while the editor is
shown, and otherwise `Machine::render_screens_now`. That forces a redraw
(`Runtime::touch`) in DIRECT mode, under the debugger and beside a subprogram,
then calls `Runtime::render`. The window goes through
`Machine::render_screens_with`, which offers the scene to the GPU path.

`Runtime::render` and `Runtime::render_with` (`runtime.rs`) do this:

1. **Redraw-on-change gate.** See [Change tracking](#change-tracking). The last
   frame stands if nothing changed.
2. **Scene.** `Scene::build` (`scene.rs`) turns the state into plain data, in
   vectors it keeps from frame to frame. Each shown sprite (its pose from
   `Sprites::pose`) and visible text screen gets an item and a packed `u64` sort
   key: layer (7 first), Z (larger first), kind (text screens before sprites),
   id (higher first). `Layer::fine` scales each layer's 2D matrix and LCLIP by
   the sample magnification. A layer gets a `LayerPass` if it has items, or a
   filter on a direct (mode 0) layer.
3. **Backend.** A `SceneBackend` gets the scene first. If it returns false, or
   there is none, the CPU draws. A frame the backend holds is drawn again on the
   CPU for anyone who wants it in the framebuffer.
4. **Drawing.** `CpuRasterizer::draw` (`raster/mod.rs`) clears the frame to
   `Scene::back`. For a sub interpreter's frame (`env_type != 0`), that color is
   premultiplied and its alpha is the coverage (255 less SmileBASIC 4's
   transmittance). A clear BACKCOLOR lets the main screen show through, and an
   opaque one makes the whole frame opaque at the end.
5. **Passes, layer 7 to 0:**
   - The target is the frame itself for mode 0 (`Compose::Direct`), or `scratch`
     (cleared to 0, premultiplied) for modes 1-4.
   - Without a 4x4 matrix, each item is drawn: `raster::text::draw` or
     `raster::sprite::draw`.
   - With a 4x4 matrix, sprites are drawn straight through it with
     `compose::project_image`. Runs of text screens are first drawn flat into
     `flat`, whose place and size the builder works out (`scene::flat_area`,
     `compose::source_bounds`), then put on with `compose::project`. That is why
     text screens reach at most one screen past each edge (see
     [differences.md](differences.md)).
   - `FilterOp::apply` (`filter.rs`) runs on the target. A raster filter on a
     direct layer then makes the frame opaque again.
   - For modes 1-4, `compose::compose` puts `scratch` over the frame, multiplied
     by the layer color.
6. **FADE.** `screen::cover` mixes the fade color over every pixel. It also
   darkens the frame to 224/255 in the same rounding while the other side has
   the focus beside a subprogram (`dimmed`).
7. **Emulator UI on the frame.** `Runtime::draw_overlay`: in DIRECT mode, the
   command hint and the completion list (`hint::draw_band`, `Completion::draw`)
   are drawn after the fade, so they are not darkened. These are not SmileBASIC
   4 rendering ([What stays on the CPU](#what-stays-on-the-cpu)).

## Drawing one element

**Placement.** `Affine::place(pos, home, rot, scale)` takes the home point to
the position, scaled and then turned clockwise. The layer's matrix follows
(`Affine::then`). Every rasterizer works by inverse mapping:

- For each frame pixel in the element's bounding box (`compose::span`, clipped
  to LCLIP), the pixel center `(px+0.5, py+0.5)` goes through the inverse
  matrix, moved a tiny way right and less up (`Placement::sampling`).
- The pixel is drawn when the resulting image point `(lx, ly)` lies in
  `[0,w) x [0,h)`. An edge exactly through a center takes the pixel if it is the
  left or the bottom edge, whichever way the image is turned.
- The texel read is the whole part of the point moved again by
  `Placement::texel_tie`.

**Sprites** (`raster::sprite::draw`, from a `SpriteItem`):

- A page that is still blank shows nothing: `SpriteItem::new` makes such a
  sprite, or a clear one, `SpriteItem::NOTHING`.
- `Source` reads GRP-1 (white everywhere), or a page with GSAMPLE border or
  repeat.
- `Source::page_xy` undoes the quarter turn and then the flips.
- `graphics::Tint` turns each page pixel into a `compose::Paint`, the pixel's
  light and what it leaves of the frame, kept exact: `Over1` (untinted, 16-bit
  lanes over 255), `Over3` (tinted, over 255³; opaque pixels by table), or `Lit`
  (additive). One `put` per frame pixel rounds once.
- Three paths:
  - `draw_upright`: no rotation or shear. A `compose::Grid` gives the image
    column and row for each frame column and row, worked out once per sprite.
  - general `compose::draw_image`, for rotated or sheared sprites;
  - `compose::project_image`, for a layer with a 4x4 matrix.
- A sample point exactly between texels goes the way `Placement::texel_tie` says
  ([renderer-verification.md](renderer-verification.md), rule table for
  sprites). The amounts are far above floating point noise, so a tie is never
  left to chance.
- Blend: the paint's `put` is either over (the exact sum of the element's light
  and what is left of the premultiplied destination, rounded to nearest once) or
  additive (the light rounded, added with saturation, destination alpha kept). A
  paint is worked out once for a run of the same page pixel: GRP-1 boxes and
  fades made of sprites, and scaled-up sprites.

**Text screens** (`raster::text::draw`, from a `TextItem` and the cells):

- Upright screens go through `draw_upright`. A `Grid` maps each frame column and
  row to a (cell, glyph column or row) pair. Each distinct (code, color) glyph
  is worked out once into `Glyphs`, a `Vec` and a `HashMap`. `KeptGlyphs` keeps
  them from frame to frame while what the glyphs depend on (`GlyphsAre`) stands.
- Other screens are drawn cell by cell with `draw_image_part` (`draw_turned`).
- Glyphs come from three places:
  - GRP5 (`glyph_pixel`, 8 and 16 pixel fonts);
  - the TPAGE page for user-defined characters `U+E800`-`U+F7FF`, on a 64-wide
    grid;
  - `Font::pixel`, for fallback glyphs with no place on GRP5.
- Font types 24-64 show only user-defined characters.
- The cell color and TCOLOR are multiplied in unrounded (`compose::Mixed`, up to
  `Over5` over 255⁵). Each glyph pixel's paint is kept as one `u32`: opaque
  pixels as themselves, others as an index into a side list.
- The INPUT cursor is a white bar over all this.
  [renderer-verification.md](renderer-verification.md) has its rules in the
  table for text screens.

## Layers, projection, filters

- **2D LMATRIX** is an `Affine` applied after each element's own placement.
- **4x4 LMATRIX** (`compose::homography`, `project`, `project_image`):
  - A layer point enters as `(x, y, 0, 1)`. The X, Y and W rows make a 3x3
    homography.
  - Each destination pixel center is mapped back through its inverse.
  - Only points with W > 0 and Z/W from -1 to 1 show. Z/W is linear across the
    screen. Sampling is nearest.
  - Rows are walked incrementally: the homogeneous step is constant along a row.
    `project_image` solves the row's run inside the image once, on both sides of
    the W = 0 line.
- **Compose modes** (`compose::compose`): Normal, Add, Multiply and Screen, with
  the layer color applied to light and coverage, each one rounding of the exact
  sum (`compose_tinted`). Mode 0 (`Direct`) never gets to this code. White layer
  color takes fast paths that give the same pixels.
- **Filters** (`filter.rs`):
  - mosaic: the frame sampled bilinearly into a small part of a half-size frame,
    then drawn back with nearest. Blocks are n frame pixels, and a block of even
    n is the mean of its middle four;
  - blur: 2x2 average to half size, 3 to 9 taps along rows and then columns in
    16.16 fixed point, each pass rounded to 8 bits, then scaled back with
    nearest; all in integers. Runs of one color take a short way;
  - raster: per-row or per-column offset and scale, OpenGL-style coordinates,
    from the array as it was at `LFILTER`;
  - color: HSV shift of each distinct color, through a small hash table, on the
    premultiplied texel in f32 as SmileBASIC 4 does.

## Scene and rasterizers

The split has these aims:

- SmileBASIC 4's semantics live in one place, the **scene builder** in
  `sb-runtime`. Backends only rasterize what it gives them.
- The CPU rasterizer stays the reference and the fallback.
- `sb-core` and `sb-runtime` stay frontend-free: no SDL, wgpu or OS types (see
  [CONTRIBUTING.md](../CONTRIBUTING.md)). The scene is plain data, so that a
  later WASM build can use the CPU rasterizer with a canvas, or a WebGL2/WebGPU
  backend.
- The scene allocates nothing per frame: its vectors are kept and cleared.

`crates/sb-runtime/src/scene.rs` holds the scene, built each frame that the
redraw gate lets through. In short (the source has the rest):

```rust
pub struct Scene {
    pub sample: u32,
    pub back: u32,                  // what the frame is cleared to
    pub opaque_frame: bool,         // main screen: stays opaque under composed layers
    pub opaque_end: bool,           // a sub interpreter's opaque BACKCOLOR
    pub passes: Vec<LayerPass>,     // layer 7 first; those with nothing to do left out
    pub fade: u32,
    pub dimmed: bool,               // one rounding with the fade
    pub raster_rows: Vec<[f64; 4]>, // raster filters' arrays, read by value
    // Scene::items(pass) walks these back to front:
    order: Vec<u64>, sprites: Vec<SpriteItem>, texts: Vec<TextItem>,
}

pub struct LayerPass {
    pub compose: Compose,           // Direct draws onto the frame
    pub color: u32,
    pub clip: Option<Clip>,         // in frame pixels (already "fine")
    pub projection: Option<Projection>, // matrix, flat frame's place and size
    pub filter: FilterOp,
    pub opaque_after_filter: bool,
    items: Range<u32>,
}

pub struct SpriteItem {             // 88 bytes
    pub matrix: Affine,             // image -> layer -> frame, already fine
    pub color: u32,                 // 0 alpha: shows nothing
    pub area: u32,                  // decides Tint's tables
    pub page: i32,                  // negative: GRP-1
    pub repeat: bool,               // GSAMPLE 1
    pub u: i32, pub v: i32, pub w: i32, pub h: i32,
    pub attr: i32,                  // quarter turn, flips, additive
}

pub struct TextItem {               // one per visible text screen, not per cell
    pub screen: u8,                 // whose cells: RasterInput::cells
    pub matrix: Affine,             // on a 4x4 layer, to the flat frame
    pub empty: bool,
    pub font_type: u32, pub char_size: u32, pub width: u32, pub height: u32,
    pub page: i32, pub page_origin: (i32, i32), // TPAGE
    pub tint: u32, pub add: bool, pub inverts: bool, pub may_invert: bool,
    pub cursor: Option<CursorBar>,  // rectangle and color worked out
}
```

How the code maps onto it:

- **Sprites** are one `SpriteItem` each. The `Sprites::pose` link arithmetic,
  the blank-page rule, `Affine::place` and `Layer::fine` are in the builder. A
  sprite that shows nothing keeps its place: its layer is still cleared and
  composed, and it still ends a run of text screens on a 4x4 layer, so leaving
  it out would change pixels.
- **Text screens** are one `TextItem` each, and the cells are not copied: the
  rasterizer gets them by reference. One item per cell would mean up to 14400
  cells on each of five screens, 88 bytes or so each, 6 MiB a frame, to redo
  what `draw_upright` does from the cell array. A GPU backend turns cells into
  quads itself, with the same lookups as `TextItem::glyph` (TPAGE, GRP5, then
  `Font::pixel`, which still needs an atlas). Coverage is the same either way: a
  frame pixel belongs to the cell under its center (`(x / size, y / size)`).
- **Text on a 4x4 layer** is drawn flat into an offscreen of the size in
  `Projection`, and each run is then projected.
- **Raster filters** copy the array's rows at `LFILTER`, as SmileBASIC 4 does,
  and those go into `Scene::raster_rows` when the scene is built.
- **The sort** is exact: layer, Z, kind, id, on packed `u64` keys, without
  moving items.

The largest scene, 4096 sprites and five screens of 14400 empty cells, builds in
0.15 to 0.18 ms and holds 429 KiB, of which 88 bytes a sprite. It allocates
nothing once grown, as a test checks. On a run of SPROT, `Scene::build` is below
0.2% of the samples. Measured on one x86-64 desktop, release build.

The rasterizer traits are in `raster/mod.rs`, with dispatch at most once a
frame. `Runtime` holds a `CpuRasterizer` directly, and takes a backend as an
optional `dyn SceneBackend`:

```rust
pub struct RasterInput<'a> {        // what a scene refers to and does not copy
    pub grp: &'a Graphics,
    pub font: &'a Font,
    pub cells: [&'a [Cell]; SCREENS],
}

pub trait Rasterizer {
    fn draw(&mut self, scene: &Scene, input: &RasterInput);
}
```

`scene.rs`, `sprites.rs` and `text.rs` (making items) decide what shows, and
`raster/` decides how: path choice, ties (`Placement::texel_tie`), `Tint` and
the glyph tables. `compose.rs` and `filter.rs` hold pixel arithmetic that only
`raster/` and GRP drawing use.

## Change tracking

The redraw gate comes first and is cheap: `changed`, `take_changed`,
`drawn_with`. It decides whether a scene is built at all. It keeps the last
frame (`self.raster.frame()`) when all of these hold:

- `self.changed` is false;
- no text screen reports changed cells (`TextScreen::take_changed`);
- the cursor blink and dimming are as before (`drawn_with`).

`changed` is set by:

- `Host::call`, which classifies each command with `Runtime::sight`.
  `Sight::Unseen` commands (`Command::unseen`, mostly reads, sound and input)
  leave it alone. For sprite commands (`Sight::Sprite`), it compares
  `Sprite::appearance()` before and after. `Runtime::sight` also leaves it alone
  for text-cell commands while no line is typed (the cells report their own
  changes), for one-argument `SPPAGE`/`SPLAYER`, and for a `DIALOG` still
  waiting. Every other command sets it. Once it is set, later calls skip
  `sight` and the comparison: only drawing a frame clears it.
- `Runtime::animate` when a fade, sprite or text animation moved something;
- `key_press`, `touch`, and printing while a line is typed.

`revision` counts the frames really drawn. The frontend uploads a texture only
when it moves.

A program that only writes a raster array needs no redraw: SmileBASIC 4 copies
the array at `LFILTER`, so such writes show nothing (TEST60).

Pages have marks as well: see the next section.

## GRP pages

All page reads (`GPGET`, `GSAVE`, `GCOPY`, `GPAINT`, GARRAY) and all drawing
stay on the CPU. So no GPU readback is ever needed, and SmileBASIC 4's rules for
GARRAY and GUPDATE stay in `graphics.rs`.

The GPU path mirrors each page in use as a 2048x2048 texture:

- **Format.** `ARGB8888`, the same bytes as `Graphics` (one `0xAARRGGBB` word),
  sampled with nearest, is what SDL_Renderer takes and all the sums need.
  `R32Uint` holds the same bytes for a shader that works in integers, should one
  ever need to.
- **Dirty tracking.** `graphics::Dirty` keeps a bit for each 64-pixel tile of a
  page. Pages are private to `graphics.rs`, and `page_pixels` asks for the area
  to be written. A test compares the marks with a diff of the page for each way
  of writing.
- **Uploads.** `Gpu::page` sends only marked tiles that a shown element reads,
  and the rest when one does: 0.5 MB a frame in GFILL and GPUTCHR, which were
  16.8 before the marks. GCLS fills the whole page, so SOLIDGUNNER_V, which does
  it each frame, sends only what its sprite shows. With the marks, GFILL,
  GPUTCHR and GAME are no slower than the CPU. With the path off, the marks cost
  at most 0.7% in instructions on GRP scenes; GLINE, GCIRCLE and GPUTCHR got
  cheaper, as they now do less per pixel.
- **Pitch.** Uploads pass the page's row pitch, `page::STRIDE * 4` bytes.
  SDL's GLES2 driver cannot unpack with a pitch of its own, so for any
  rectangle it first copies the rows into a buffer it allocates. On GLES2 the
  path sends at most 64 KiB of rows a call (`GLES2_BAND`), so that buffer
  stays small and in the cache. Desktop GL unpacks with the pitch, and SDL's
  GPU driver copies into a transfer buffer either way.
- **Memory.** A blank page needs no texture (sprites on it draw nothing). Six
  pages in use are 96 MiB of textures, allocated on first use.

Nothing is read back: GRP commands read the CPU's pages, which stay
authoritative.

## What stays on the CPU

- Emulator UI drawn on the frame: the hint band and the completion list, in
  `Runtime::draw_overlay`. They are drawn on the CPU either way: on its frame,
  after the rasterizer, or for a GPU frame on a clear `Framebuffer` of their
  own, which is sent when it changes and put over the frame.
- Dimming is not an overlay: it shares one rounding with FADE (`screen::cover`),
  so it stays in the scene (`Scene::dimmed`).
- The editor, menu bar, panels and dialogs. They redraw only on change, and they
  are not SmileBASIC 4 rendering.
- GRP drawing, and anything that has no GPU path. If a scene needs a feature the
  GPU path lacks, the CPU draws the whole frame. The one mix is the color filter
  off the `gpu` driver, which needs no readback: that layer is drawn on the CPU
  and sent ([Color filter](#color-filter)).

## Who reads pixels back

| Reader | Reads | Notes |
|---|---|---|
| `GPGET`, `GSAVE`, `GCOPY` (source), `GPAINT`, `GARRAY` | GRP pages (`Graphics::read`, via the GARRAY array when there is one) | Never the composed screen. |
| Sprites, text screens (TPAGE), GPUTCHR | GRP pages, GRP5 | Inside rendering. |
| `CHKCHR`, `TSAVE`, `TARRAY` | Text cells | Not pixels. |
| `smilebasic-check --screen`, `--bench ... PNG`, `--play` shots | `Machine::render()` frame, via `save_screen` | Headless; PNG at XSCREEN size, or the 1280x720 display with a sub screen. |
| `SB_UI_BENCH shot=` | SDL canvas (`read_pixels`) | Whole window, including the UI. |
| Debugger GRP tab, screens view | GRP pages, `render_screens` frame | |
| Tests (`crates/sb-runtime/tests/*.rs`) | Frame pixels | Exact pixel asserts. |

SmileBASIC 4 has no command that reads the composed screen. SmileBASIC 3's
`GSPOIT` is not in SmileBASIC 4, and `GPGET` reads only graphic pages.
[renderer-verification.md](renderer-verification.md) says what that means for
checking the rules.

## Frontend

The frontend is `crates/sb-sdl`.

**Screen texture.** `frame_texture` (`main.rs`) creates one `ARGB8888` streaming
texture per (frame size, filter). Before creating it, it sets the renderer's
default scale mode:

- XSCREEN mode 0 is `SDL_SCALEMODE_LINEAR`.
- Mode 1 goes through `SmartFrame`: a nearest texture is drawn, when the frame
  changes, into a target texture twice its size, which is shown with
  `SDL_SCALEMODE_LINEAR`. That is exactly SmileBASIC 4's sum, with no shader.
  `smart_scale` does the same on the CPU, the reference for tests and the
  fallback if the target cannot be made (`SB_UI_BENCH`'s `cpu-smart` forces it).
- Mode 2 is `SDL_SCALEMODE_NEAREST`.
- The editor's frame is drawn at a whole factor with `SDL_SCALEMODE_NEAREST`.
  Only the Simple UI scales it by other factors, SmileBASIC 4's frame to the
  screen's place, and uses `SDL_SCALEMODE_PIXELART` there. The Rust bindings
  lack a per-texture call for it, hence `SDL_SetDefaultTextureScaleMode`.
- A subprogram's screen is `SDL_SCALEMODE_LINEAR` in every mode: SmileBASIC 4
  draws it as it does a screen in mode 0 (TEST56 shows mode 1), fitted to
  XSUBSCREEN's rectangle (`SubScreen::quad`) with BACKCOLOR in the bars.

On the CPU path, the whole frame is uploaded with `Texture::update` only when
`screen_revision` moves: 3.5 MiB at 1280x720. `screen_rect` scales it to fit the
display at the aspect ratio, by whole factors only under the menu's sharp
choice. `canvas.copy` draws it.

The rest of the window:

- **Sub interpreter screen.** `Machine::sub_screen` is a premultiplied frame in
  its own texture with `SDL_BLENDMODE_BLEND_PREMULTIPLIED`, placed where
  `XSUBSCREEN` says.
- **Editor.** `Runtime::render_editors` draws into `editor_fb`, keyed by
  `editor_key`, so it redraws only on change. It is shown in the main window or
  in its own window with its own canvas and texture.
- **UI layers** (`layer.rs`): the menu bar, the debugger panel and overlays.
  Each has its own `Framebuffer` and texture, drawn again only when a dry run
  says something changed, and uploaded only in the changed rectangle.
- **Debugger.** The panel's screens view uploads `Machine::render_screens` into
  its own texture. The GRP tab (`grp_view.rs`, `PageTexture`) uploads a whole
  2048x2048 page only when `Graphics::changes(page)` moved, and keeps halved
  copies for fitted views. `Runtime::sprite_corners` gives the debugger where a
  sprite lands, using the same matrices as the renderer.
- **Present** is `pacing.rs` (vsync handling, frame skipping).
- **A held-up main thread.** While SDL holds the main thread up, as when the
  window is dragged on GNOME, a standby thread runs the program's frames and
  their sound: [frame-thread.md](frame-thread.md).
- **Window screenshots.** The app has no PrintScreen command. The only window
  capture is `SB_UI_BENCH`'s `shot=PATH` (`bench.rs`), which reads the canvas
  with `SDL_RenderReadPixels` into PAM files.

## Headless tools

`smilebasic-check` (`--bench`, `--play`, `--screen`, part comparison) and the
tests always use the CPU. It is deterministic and needs no GPU, so results do
not depend on the machine. `smilebasic-check` has no backend.

The GPU path lives in `sb-sdl`. SDL can create an offscreen GPU renderer with a
null window (`SDL_CreateGPURenderer(NULL, NULL)`), which is what `SB_GPU_ALONE`
does, so comparison runs need no display.

## GPU path

Numbers in this section were measured on one x86-64 desktop with an RTX 3060 Ti,
on opengl through EGL with no window, unless a driver is named; GPU ms means as
in [renderer-decisions.md](renderer-decisions.md), "The path against the CPU".

`gpu.rs` in `crates/sb-sdl` is a `SceneBackend`, which the runtime offers each
scene. It takes the place of the CPU rasterizer on any SDL_Renderer driver but
`software`:

- It draws sprites and text screens under any matrix, 4x4 too, with tint, both
  blends, compose modes 0-4, LCLIP and XSCREEN's sample, through
  `SDL_RenderGeometry`, with no shader on any driver.
- It draws mosaic and blur the same way, and raster as strips.
- It also draws FADE and the darkening beside a subprogram, sub screens, the
  line cursor, inverted cells and DIRECT mode's hint and list.
- The color filter has a shader on the `gpu` driver. On other drivers its layer
  comes from the CPU.

No frame of latency is added: the frame is drawn in the same iteration as on the
CPU.

The CPU draws a whole frame only in these cases, and the path counts each one by
name:

- an image off a page that does not repeat;
- a vertex that is NaN, infinite or at least 2^24 from 0, from a matrix far out
  of range;
- a raster filter that takes over 20000 quads (`MOST_RASTER_QUADS`);
- inverted cells of a font type over 16;
- a full atlas of fallback glyphs;
- a texture or blend refused.

No scene in the timing table of [renderer-decisions.md](renderer-decisions.md)
and no project in the bench set leaves a frame on the CPU.

### Blending

SmileBASIC 4's blending maps one to one onto fixed-function GPU blend states.
Color is premultiplied (alpha 0 for additive items), render targets keep
transmittance in their alpha, and every draw is rounded once, when stored to
RGBA8. Five blend states cover it, all with the ADD equation:

| State | Color (src, dst) | Alpha (src, dst) | Used for |
|---|---|---|---|
| BS0 | off (overwrite) | off | presenting the main screen, copies |
| BS1 | `SRC_ALPHA`, `1-SRC_ALPHA` | `ZERO`, `ONE` | color fills, the mouse cursor |
| BS2 | `ONE`, `1-SRC_ALPHA` | `ZERO`, `1-SRC_ALPHA` | every element; layer modes 1 and 2; presenting a subprogram's screen |
| BS3 | `DST_COLOR`, `1-SRC_ALPHA` | `ZERO`, `ZERO` | layer mode 3 |
| BS4 | `1-DST_COLOR`, `ONE` | `ZERO`, `ZERO` | layer mode 4 |

The emulator's alpha is coverage, 255 less SmileBASIC 4's transmittance. A
backend that keeps coverage takes the usual premultiplied alpha factors (`ONE`,
`1-SRC_ALPHA`) for BS2 and writes an opaque alpha for modes 3 and 4.

The path gets the same sums from SDL's blend modes, with no shader. A texel
times a vertex color gives `t*c` and `a*ca` unrounded, and `SDL_BLENDMODE_BLEND`
then gives `t*c*a + d*(1-a)` with one rounding at the store: SmileBASIC 4's own
sum. `ADD` is its additive element. The premultiplied blend, the premultiplied
add, `MUL` and a custom `1-dst, 1` are modes 1 to 4, with the layer's color
times its alpha on the vertices.

The CPU works the same sums exactly, in integers, and rounds once per draw. The
two differ only where the GPU's own float-to-8-bit conversion rounds a value
very near a half the other way (about one patch in six, by 1, on the PC GPU
behind TEST49's screenshots). The ±1 add up over layers: about one pixel in six
a layer in LCOMP.

### Ties

The host's rasterizer resolves ties its own way unless steered, whatever the
order of the vertices. So the path applies two biases, following
`Placement::tie_sides`: a sample moved by 1/128 of a texel the way
`Placement::texel_tie` says, and an element that can tie moved 1/256 of a pixel
down. A bias follows the rule, not the noise a real Switch has besides.
[renderer-verification.md](renderer-verification.md) has the results.

### Raster filter

A run of rows or columns with one entry is a quad read nearest, split where the
source point crosses the frame's edge or wraps. A sample is moved 1/512 of a
texel to round down as the CPU does. A unit test checks each pixel takes the
CPU's texel.

### 4x4 layers

`SDL_Vertex.position` is 2D, with no W, so `gpu_project.rs` draws a 4x4 layer's
element as a grid of triangles. The corners go exactly where the matrix puts
them, so straight edges stay straight and in place, and only a texel inside a
cell is off.

W above 0 and Z / W from -1 to 1 are straight lines on the element, so they and
the frame's edges cut it first. A unit test compares what is covered with the
CPU's. The grid doubles until a run's middle is within a tolerance where the
element is nearest. The tolerance is a quarter of a pixel (`GRID_TOLERANCE`),
chosen on FINITI's skybox, six sprites 1024 across:

| Tolerance, pixels | Vertices a frame | Pixels more than 2 off | GPU ms |
|---|---|---|---|
| 2 | 3800 | 5.0% | 3.4 |
| 0.5 | 16700 | 1.25% | 3.7 |
| **0.25** | 33700 | 0.59% | 4.3 |
| 0.1 | 87500 | 0.24% | 5.9 |

The differences lie scattered on texels' edges, with no line and no grid to be
seen in a picture of them. NEON_DRIFTWAY's elements are small, at about 200
vertices, and do not change with the tolerance. No shader was needed.

### Sub screens

A subprogram has screens and pages of its own, so it gets a second path on the
same renderer. A clear back keeps its alpha: no pixel's alpha was off. Its frame
goes to the window smoothed and premultiplied, in every XSCREEN mode.

### Color filter

HSV is more than blending can do. Off the `gpu` driver, the path draws and
filters that layer alone on the CPU and sends it. A direct layer filters what is
behind it, so such a frame starts with all those passes from the CPU. Nothing is
read back. LCOLOR's four whole-frame layers cost as much as the CPU does, 14.7
MB sent a frame, with no pixel differing; other layers stay on the GPU.

On the `gpu` driver a shader does the filter, and takes 1.4 ms in LCOLOR, so it
stays. It is the one shader, in `crates/sb-sdl/shaders`: GLSL, with SPIR-V from
glslang committed beside it. Only Vulkan has it. Windows' D3D12 needs DXIL and
macOS MSL too, which SDL_shadercross makes from the same source; neither was
tried.

`SDL_GPURenderState`'s shader is SPIR-V only. On the `gpu` driver over D3D12 or
Metal, `ColorFilter::new` finds no such format and the layer comes from the CPU,
as on every other driver. The `gpu` driver is reached only when all before it
fail, so this costs little.

### Mosaic and blur

These use fixed function on two half-size targets. A mosaic is a smoothed draw
into part of one and a nearest draw back.

A blur's taps cannot simply add: each draw rounds to 8 bits, and 9 draws came up
to 6 off in a simulation of frames. So the path does this:

- Each draw mixes into what is there by its share of the taps so far, the
  outermost first.
- Two neighbors are one draw, by smoothing between them. A run of one color
  stays exact.
- The fall in the weights comes last, as black over all of it, which takes what
  shows through down with the light.
- On an opaque frame a tap takes one draw. A layer's alpha decides how an
  ordinary mix would go, so on a layer a tap takes two, down and then add, and
  rounds twice.

No shader was needed.

### GPU memory

On the GPU: 16 MiB a page in use, at most six, 16 for the atlas if a glyph needs
it, three frame-sized targets and two of half that.

Peak RSS is up by 30 to 60 MiB (297 to 328 in SPROT), with one page and the font
page on the GPU, and 312 to 346 in TMULTI. TMULTI was 1006, on opengl and
opengles2, with 224000 vertices in one draw. 700 MiB of that went when those
were handed over in draws of 60000 or fewer, so a driver's keeping what it took
for a large draw is the likely cause, and the path flushes every 30000
(`MOST_VERTICES`). On opengles2 with Mesa's llvmpipe, one draw peaked at 388.

Caching a text screen in a target of its own would not help TMULTI, whose cells
change each frame. SDL expands indexed geometry into plain vertices in its GL
drivers, so indices would not help either.

## GPU path settings

The Renderer setting (File > Settings > Screen) is Auto or CPU. The default is
Auto: the GPU where the self-test passes, the CPU otherwise. `settings.txt`
keeps only a choice that differs from the default, so the default can change for
those who made none. `SB_GPU` overrides it. About and the bench report say who
draws and, if the CPU, why.

These environment variables set or measure the path (`gpu.rs` has the details):

| Variable | Effect |
|---|---|
| `SB_GPU=0` | The CPU, whatever the setting. |
| `SB_GPU=1` | Draws what it can. |
| `SB_GPU=sync` | The same, and waits for the GPU after each frame, to time its work. |
| `SB_GPU=check`, `check:N` | The CPU also draws every Nth frame the path draws (every one by default), and the differences are printed at the end. |
| `SB_GPU_DUMP=PATH` | With `check`, saves the frame that differed most as `PATH-gpu.pam` and `PATH-cpu.pam`. |
| `SB_GPU_TOL=PIXELS` | Sets the 4x4 grid's tolerance. |
| `SB_GPU_TEST=off` | Skips the self-test. |
| `SB_GPU_TEST=fail` | Acts as if the self-test failed. |
| `SB_GPU_GOLDEN=1` | Runs all golden scenes instead of the emulator, prints a row for each and exits, 1 if one is over its limit. |
| `SB_GPU_ALONE=1` | Draws on a renderer of its own with no window, through SDL's `gpu` driver. |

### Start-up self-test

At start-up the path and the CPU draw one small scene (`gpu_golden::SELF_TEST`).
It takes 30 to 100 ms on a desktop GPU and about 100 on llvmpipe. The test
passes on rounding however a driver rounds, and fails on a wrong sum:

- What counts is pixels more than 4 off (`FAR_OFF`), in any channel, alpha too,
  on at most 0.5% of the frame. An element goes through three draws in that
  scene, each of which may lose 1.
- llvmpipe rounds otherwise than an NVIDIA GPU, with 16% of SPROT's pixels 1 off
  where that has 0.01%, and passes.
- A GL shim truncating every fragment's output comes to 3 off at most, 704
  pixels of 384000 over 2, none over 4, and passes.
- The scene samples one-texel checkers at the page's far corner, where a
  fragment shader holding texture coordinates in 16 bits, as GLES2's mediump
  can, goes wrong while the page's start looks right. Emulated by a GL shim,
  this fails the test.
- A shim breaking the blend function fails, as do 16-bit coordinates, at 2.5% of
  the pixels.

## Driver support

SDL 3.4 has direct3d (9), direct3d11, direct3d12, opengl, opengles2, metal,
vulkan, gpu and software. On Emscripten it uses opengles2 (WebGL). The path
stays off on software.

**Tried.** SPROT, LCOMP and TMULTI differ from the CPU alike on opengl,
opengles2 and SDL's `gpu` driver on the NVIDIA GPU, on SDL's vulkan driver, on
the `gpu` driver on lavapipe and on opengles2 on llvmpipe. SDL's vulkan driver
costs far more on the CPU (TMULTI 29 ms), so prefer another. Only the `gpu`
driver takes shaders.

**By reading SDL 3.4.18:**

- Every driver takes all the path's blend modes, as their factors are plain and
  the operation is the same for color and alpha.
- D3D11, D3D12, Metal, GL and GLES2 keep a vertex's color as floats. Direct3D 9
  packs it to 8 bits, truncating, so a tint there may be 1 off, which the
  self-test allows. It is reached only if D3D11 and D3D12 cannot start.
- Pages are a power of two, so wrapping needs nothing of GLES2.
- D3D11 makes a staging texture for each `SDL_UpdateTexture`, so a program
  writing to many tiles a frame costs more there: the report's `pages` will
  show.
- Each driver's arithmetic is the self-test's to judge, as it reads back from a
  target on all of them.

**Reach.**

- Windows, macOS, Linux use SDL_Renderer on their usual drivers, untested on the
  first two. The color filter's shader runs on the `gpu` driver over Vulkan, and
  would reach D3D12 and Metal once their forms are made.
- `cargo build --release --target x86_64-pc-windows-gnu -p sb-sdl` builds with
  MinGW, D3D9, D3D11, D3D12, GL, GLES2 and Vulkan in it; it has not been run.
  macOS needs its SDK.
- GLES handhelds, the RK3326 among them: the path needs only GLES2's textures,
  targets and blends, so it should work, the color filter's layer coming from
  the CPU. It has been tested on GLES2 on a desktop GPU only. On Mali, timings,
  target switches on a tiler and uploads are unknown.
- Mesa's PanVK names the Mali-G31, and has a switch for hardware it does not
  call supported. Whether the G31 needs it, and what an AmberELEC build has, are
  unknown.

The CPU rasterizer remains the only path on machines with no suitable driver, so
CPU performance work keeps its value. The project puts performance first and
aims to run on old and low-end machines with few resources.

## Failure handling

The cases below, apart from Suspend, were tried with no window, through
`SB_UI_BENCH` and a GL shim that lies.

**At start-up or while drawing:**

- **No EGL or GL, the driver left to SDL.** SDL takes Vulkan, or the software
  renderer, where the path stays off.
- **`SDL_RENDER_DRIVER` naming a driver that cannot start.** Another is tried in
  SDL's order. This used to be a panic in the sdl3 crate.
- **SDL's `gpu` driver with no Vulkan.** As above; with `SB_GPU_ALONE`, the CPU.
- **Largest texture under 2048, as 1024 or 2047.** The self-test sees the frame
  stayed on the CPU, and the path stays off. Under the frame's own 1280 the
  emulator cannot start on the CPU either, with a message.
- **A texture refused, at start-up or later, on opengl.** The path sees it
  through glGetError, which SDL's driver looks at only in a debug context. After
  three frames refused in a row (`MOST_REFUSED`) the path stops for the session.
- **A texture refused on opengles2.** The path asks GL first (`texture_fits`)
  and refuses it as above. SDL 3.4.18 frees its texture data twice in
  `GLES2_CreateTexture` on GL_OUT_OF_MEMORY and aborts, where the driver looks
  at errors. The frontend's own textures can still meet that.
- **A target made in part, for a frame or half of one.** The path frees all
  of it. Drawn on as it was, the next frame would have gone to the window.

**Renderer events:**

- **`RENDER_TARGETS_RESET`.** The frames are drawn again, a subprogram's too.
- **`RENDER_DEVICE_RESET`.** Every texture is made again, and the path started
  again with its self-test. Direct3D 11, 12 and Vulkan free them all, and
  the window's own would have ended the emulator at its next upload.
- **`RENDER_DEVICE_LOST`, or a refusal of the frontend's own drawing.**
  Everything on the renderers goes. The frontend makes a new renderer for the
  same window: by SDL's choice, then any other. After two failures in a row it
  does not try the same driver again. The path then starts over, with pages sent
  again, or the CPU draws if the new driver is the software one.

**With no renderer to be had,** the program stands paused, its sound running
out, and a try is made each second. Programs with unsaved edits go to
`@RECOVERn_SLOT.PRG` in the project, or the data directory if it takes none.

The emulator also saves those files, names them in a message box, and says so
once at the next start, in each of these cases:

- eight failures with no 120 good frames between;
- the window closed in that state;
- an error or a panic outside `frame()`.

`at=N:lost`, `fail`, `refuse:N`, `panic` and `unsaved` in the bench try these.

**Suspend.** A GL context lost without SDL saying so, as some drivers do on
resuming from suspend, cannot be asked about. `glGetGraphicsResetStatus` needs a
robust context, which risks a context refused, and NVIDIA reports a purge only
with an attribute SDL cannot set.

So a wall clock a second or more ahead of the monotonic one, which stands still
through a suspend on Linux and macOS, counts as `RENDER_DEVICE_RESET`, and all
is given again. A main loop merely held up moves both clocks and resends
nothing. On Windows the monotonic clock goes on too, so this never fires there,
where Direct3D reports its own losses and a GL context keeps its textures.

This comes from reading the code; nobody has tried a real suspend.
`at=N:sleep:MS` in the bench stands in for one.

**Restarts.** Each restart of the path adds 4 to 10 MiB of RSS, which on opengl,
opengles2 and llvmpipe does not come back (the long runs in
[renderer-verification.md](renderer-verification.md)); a restart is rare.

## Exactness policy

The CPU rasterizer is the reference. Hardware stays the arbiter of the CPU
reference ([renderer-verification.md](renderer-verification.md)).

- Exact paths must match the CPU bit for bit.
- Inexact paths may differ only at pixels next to an edge or tie: the differing
  pixels must lie within one pixel of an image boundary or a texel boundary
  under the item's matrix.
- Each inexact path is listed in the test suite with its tolerance. The golden
  scenes (`gpu_golden.rs`) hold those limits.

### What can match the CPU exactly

- **Texel values**: pages uploaded as integers (`R32Uint`) or read with
  `texelFetch`/`textureLoad` at integer coordinates. Page reads never use
  filtered sampling.
- **Blending in integer arithmetic in a shader.** The shader reads the
  destination from the previous target (ping-pong) or works in a compute pass,
  with the same sums as `compose::Paint`: one rounding a draw, with saturation.
  WGSL, GLSL ES 3.0, HLSL SM5 and MSL all have 32-bit unsigned integer ops; the
  twice-tinted text sum (over 255⁵) needs 64 bits or two words. The path as
  built does not do this: it uses fixed-function blending, so its blends fall
  under the next list.
- **Compose modes, FADE, mosaic, blur** (already integer: 8.8 bilinear weights,
  16.16 blur taps), and the color filter if it is moved to an integer or
  table-based formulation first. These too need a path that blends in integers.
- **Upright sprites and text at integer scales and integer offsets.** The sample
  point `(px + 0.5 - tx) / s` is exact in f32 for screen sizes up to 1280x720.
  This covers most games. Upright sprites and text at whole scales and places,
  and the GRP scenes, are identical in the path as built.

### What differs

- **Rotated, non-integer-scaled and projected edges and ties.** The CPU maps
  pixel centers through an f64 inverse matrix (and LMATRIX values are stored at
  f32, then used in f64). A shader works in f32, with a 24-bit mantissa. Pixels
  whose sample point lies within about 1e-4 of a texel or image boundary can
  fall the other way. The tie rule (`Placement::texel_tie`) cannot be reproduced
  in f32 as written; it needs its own bias. What is not a power of two on
  SmileBASIC 4 keeps some noise of its GPU's in any case
  ([renderer-verification.md](renderer-verification.md)).
- **Raster filter coordinates**: these are f64 on the CPU. The color filter's
  HSV is f32 on both, as SmileBASIC 4's.
- **Fixed-function blending**, as SmileBASIC 4 uses: ±1 per channel where a
  value lies very near a half, between vendors and against the CPU.
- **Blur, accepted.** Done with no shader it rounds at each draw: LBLUR, three
  layers one over another, is up to 6 off and TEST54's parts up to 3. Nobody
  compares a blur by the pixel, so this stays, and should not be worked on.
- **Projected images, inside.** Without W a texel can be off inside a cell of
  the grid, by up to a quarter of a pixel as chosen; edges are not.

## Open work

- **Scene reuse.** Compare the built scene with the last one, by a hash of
  `passes`, the items, `fade`, the page change counts and a change count for
  each screen's cells, which are not in the scene. If equal, reuse the frame.
  This catches commands that set the same values, such as `LAYER` or `TOFS` with
  the same values every frame, which always set `changed` (sprite commands
  already compare `appearance()`). Inputs read by reference at render time must
  enter the comparison: GRP pages and cells by change counts. Measure it on a
  program that sets the same `LAYER` every frame.
- **Per-pass caching.** A GPU backend could keep a texture for a composed layer
  whose items, inputs and pages did not change.
- **Debugger GRP view.** It could send tiles instead of whole pages. The `Dirty`
  marks have one reader, so it would need a history of its own.
- **Text on a 4x4 layer.** Projecting each cell directly would remove the bound
  described in [differences.md](differences.md). That is a behavior change:
  decide it after TTEST36-style checks.
- **Fewer inexact pixels.** Compute sample coordinates in fixed point (for
  example 16.16 integers) in both backends for affine items. This changes the
  CPU reference too, so it needs a hardware check first. Or keep f64-like
  accuracy where it matters by working in coordinates relative to the item's
  origin.
- **Color filter shader.** Make the DXIL and MSL forms for D3D12 and Metal.
- **Untested targets.** Run the Windows build, build for macOS, and try a Mali
  handheld (see [Driver support](#driver-support)). Try a real suspend. Most of
  the path's timings have not been measured in a real window.
- **SDL's GLES2 double free.** Report it upstream.
- **CI.** Run the golden suite there;
  [renderer-verification.md](renderer-verification.md) lists the candidates.
- **Rules still open.** [renderer-verification.md](renderer-verification.md)
  lists them, with the UNVERIFIED marks left in the rendering files.
- **WASM.** A WASM build with the CPU backend first. The old WebGL2/WebGPU step
  depended on (d), which was not chosen; SDL_Renderer's opengles2 reaches WebGL,
  untried.
