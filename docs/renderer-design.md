# Renderer design: preparing a GPU backend

This document prepares a GPU renderer for the emulator. The plan, in
order:

1. Verify every rendering rule that is still a guess on SmileBASIC 4
   itself ([section 2](#2-rendering-rules-and-open-questions)).
2. Split the renderer into a scene builder, which holds SmileBASIC 4's
   semantics, and rasterizer backends, which only draw
   ([section 3](#3-proposed-split)).
3. Add a GPU backend ([section 4](#4-gpu-backend-options)). The CPU
   rasterizer stays as the reference and as the fallback
   ([section 5](#5-exactness-policy-and-testing)).

[Section 1](#1-current-pipeline-inventory) describes how a frame is made
today. [Section 6](#6-phased-plan) gives the milestones.

Contents:

- [1. Current pipeline inventory](#1-current-pipeline-inventory)
- [2. Rendering rules and open questions](#2-rendering-rules-and-open-questions)
- [3. Proposed split](#3-proposed-split)
- [4. GPU backend options](#4-gpu-backend-options)
- [5. Exactness policy and testing](#5-exactness-policy-and-testing)
- [6. Phased plan](#6-phased-plan)

File and function names refer to the tree at the time of writing. The
drawing code in `crates/sb-runtime` is being optimised at the same
time, so check names against the source.

## 1. Current pipeline inventory

### 1.1 Data the frame is made from

All of it lives in `Runtime` (`crates/sb-runtime/src/runtime.rs`):

| Data | Type / file | Notes |
|---|---|---|
| GRP0-GRP5 | `Graphics` (`graphics.rs`) | 6 pages of 2048x2048 `u32` (`0xAARRGGBB`), 16 MiB each. A page takes memory only once something is drawn on it (`Graphics::is_blank`). Each page has a change counter (`Graphics::changes`). GARRAY arrays mirror pages; the shown page takes their writes on `GUPDATE`. |
| GRP5 font page | `text::FONT_PAGE`, `Runtime::with_font_page` | The imported font is copied into GRP5 at start-up, where programs can change it. Glyphs with no place on GRP5 come from `Font::pixel` (`font.rs`). |
| GRP4 sprite page | `text::SPRITE_PAGE`, `Runtime::set_sprite_page` | Default sprite image, imported from the user's copy. |
| Sprites 0-4095 | `Sprites` (`sprites.rs`) | `Sprite` holds U, V, W, H, attributes, home, position, Z, rotation, scale, color, page, layer, link. Sprite 4095 (`#GSPRITE`) shows GRP0. |
| Text screens 0-4 | `TextScreen` (`text.rs`) | Cells (code, color, ATTR), font type, char size, TOFS/THOME/TROT/TSCALE/TCOLOR, TBLEND, TLAYER, TPAGE. Screen 4 is the console. |
| Layers 0-7 | `compose::Layer` | `LAYER` compose mode and color, `LCLIP`, 2D `LMATRIX` (`Affine`), 4x4 `LMATRIX` (`projection: Option<[f64; 16]>`, values stored at single precision). |
| Filters 0-7 | `filter::Filter` | `LFILTER`: mosaic, blur, raster (holds the program's array and reads it each frame drawn), color. |
| BACKCOLOR, FADE | `Runtime::back_color`, `screen::Fade` | |
| XSCREEN | `screen::ScreenMode` | Width, height, sample magnification, display filter, aspect. The frame is `width*sample` by `height*sample` (`ScreenMode::frame`). |

SmileBASIC 4 has no BG layer (SmileBASIC 3's BG screens are not part of
it), so there is none here. Programs make tile maps from text screens
with `TPAGE` or from sprites.

### 1.2 Making a frame: `Runtime::render`

`Machine::render` (`machine.rs`) gives the editor's frame while the
editor is shown, and otherwise `Machine::render_screens_now`. That
forces a redraw (`Runtime::touch`) in direct mode, under the debugger
and beside a subprogram, then calls `Runtime::render`.

`Runtime::render` (`runtime.rs`) does this:

1. **Redraw-on-change gate.** It returns the last frame (`self.fb`)
   when `self.changed` is false, no text screen reports changed cells
   (`TextScreen::take_changed`), and the cursor blink and dimming are as
   before (`drawn_with`). `changed` is set by:
   - `Host::call`, which classifies each command with `Runtime::sight`.
     `Sight::Unseen` commands (`Command::unseen`, mostly reads, sound
     and input) leave it alone. For sprite commands (`Sight::Sprite`),
     it compares `Sprite::appearance()` before and after. Every other
     command sets it.
   - `Runtime::animate` when a fade, sprite or text animation moved
     something;
   - `key_press`, `touch`, and printing while a line is typed.

   `revision` counts the frames really drawn. The frontend uploads a
   texture only when it moves.
2. **Scene.** `Scene::build` (`scene.rs`) turns the state into plain data,
   in vectors it keeps from frame to frame. Each shown sprite (its pose
   from `Sprites::pose`) and visible text screen gets an item and a packed
   `u64` sort key: layer (7 first), Z (larger first), kind (text screens
   before sprites), id (higher first). `Layer::fine` scales each layer's 2D
   matrix and LCLIP by the sample magnification. A layer gets a `LayerPass`
   if it has items, or a filter on a direct (mode 0) layer.
3. **Drawing.** `CpuRasterizer::draw` (`raster/mod.rs`) clears the frame to
   `Scene::back`, which for a sub interpreter's frame (`env_type != 0`) is
   premultiplied: its alpha is the coverage (255 less SmileBASIC 4's
   transmittance), so its clear BACKCOLOR lets the main screen show
   through, and an opaque one makes the whole frame opaque at the end.
4. **Passes, layer 7 to 0:**
   - The target is the frame itself for mode 0 (`Compose::Direct`), or
     `scratch` (cleared to 0, premultiplied) for modes 1-4.
   - Without a 4x4 matrix, each item is drawn: `raster::text::draw` or
     `raster::sprite::draw`.
   - With a 4x4 matrix, sprites are drawn straight through it with
     `compose::project_image`. Runs of text screens are first drawn flat
     into `flat`, whose place and size the builder works out
     (`scene::flat_area`, `compose::source_bounds`), then put on with
     `compose::project`. That is why text screens reach at most one screen
     past each edge (see [differences.md](differences.md)).
   - `FilterOp::apply` (`filter.rs`) runs on the target. A raster filter
     on a direct layer then makes the frame opaque again.
   - For modes 1-4, `compose::compose` puts `scratch` over the frame,
     multiplied by the layer color.
5. **FADE.** `screen::cover` mixes the fade color over every pixel, and
   darkens the frame to 224/255 in the same rounding while the other
   side has the focus beside a subprogram (`dimmed`).
6. **Emulator UI on the frame.** `Runtime::draw_overlay`: in direct mode,
   the command hint and the completion list (`hint::draw_band`,
   `Completion::draw`) are drawn after the fade, so they are not darkened.
   These are not SmileBASIC 4 rendering
   ([3.6](#36-what-stays-on-the-cpu)).

### 1.3 Drawing one element

**Placement.** `Affine::place(pos, home, rot, scale)` takes the home
point to the position, scaled and then turned clockwise. It is followed
by the layer's matrix (`Affine::then`). Every rasterizer works by
inverse mapping. For each frame pixel in the element's bounding box
(`compose::span`, clipped to LCLIP), the pixel centre `(px+0.5, py+0.5)`
goes through the inverse matrix, moved a tiny way right and less up
(`Placement::sampling`). The pixel is drawn when the resulting image
point `(lx, ly)` lies in `[0,w) x [0,h)`: an edge exactly through a
centre takes the pixel if it is the left or the bottom edge, whichever
way the image is turned. The texel read is the whole part of the point
moved again by `Placement::texel_tie`.

**Sprites** (`raster::sprite::draw`, from a `SpriteItem`):

- A page that is still blank shows nothing: `SpriteItem::new` makes such
  a sprite, or a clear one, `SpriteItem::NOTHING`.
- `Source` reads GRP-1 (white everywhere), or a page with GSAMPLE
  border or repeat.
- `Source::page_xy` undoes the quarter turn and then the flips.
- `graphics::Tint` turns each page pixel into a `compose::Paint`, the
  pixel's light and what it leaves of the frame, kept exact:
  `Over1` (untinted, 16-bit lanes over 255), `Over3` (tinted, over
  255³; opaque pixels by table), or `Lit` (additive). One `put` per
  frame pixel rounds once.
- Three paths:
  - `draw_upright`: no rotation or shear. A `compose::Grid` gives the
    image column and row for each frame column and row, worked out
    once per sprite.
  - general `compose::draw_image`, for rotated or sheared sprites;
  - `compose::project_image`, for a layer with a 4x4 matrix.
- A sample point exactly between texels goes the way
  [2.2](#22-sprites) says (`Placement::texel_tie`); the amounts are far
  above floating point noise, so a tie is never left to chance.
- Blend: the paint's `put` is either over (the exact sum of the
  element's light and what is left of the premultiplied destination,
  rounded to nearest once) or additive (the light rounded, added with
  saturation, destination alpha kept). A paint is worked out once for
  a run of the same page pixel: GRP-1 boxes and fades made of sprites,
  and scaled-up sprites.

**Text screens** (`raster::text::draw`, from a `TextItem` and the cells):

- Upright screens go through `draw_upright`. A `Grid` maps each frame
  column and row to a (cell, glyph column or row) pair. Each distinct
  (code, color) glyph is worked out once per frame into a
  `HashMap` and a `Vec`, both allocated per frame.
- Other screens are drawn cell by cell with `draw_image_part`.
- Glyphs come from three places:
  - GRP5 (`glyph_pixel`, 8 and 16 pixel fonts);
  - the TPAGE page for user-defined characters `U+E800`-`U+F7FF`, on a
    64-wide grid;
  - `Font::pixel`, for fallback glyphs with no place on GRP5.
- Font types 24-64 show only user-defined characters.
- The cell color and TCOLOR are multiplied in unrounded
  (`compose::Mixed`, up to `Over5` over 255⁵); each glyph pixel's
  paint is kept as one `u32` (opaque pixels as themselves, others as
  an index into a side list). The INPUT cursor is a white bar over all
  this: see [2.3](#23-text-screens).

### 1.4 Layers, projection, filters

- **2D LMATRIX** is an `Affine` applied after each element's own
  placement.
- **4x4 LMATRIX** (`compose::homography`, `project`, `project_image`):
  - A layer point enters as `(x, y, 0, 1)`. The X, Y and W rows make a
    3x3 homography.
  - Each destination pixel centre is mapped back through its inverse.
  - Only points with W > 0 and Z/W from -1 to 1 show. Z/W is linear
    across the screen. Sampling is nearest.
  - Rows are walked incrementally: the homogeneous step is constant
    along a row. `project_image` solves the row's run inside the image
    once, on both sides of the W = 0 line.
- **Compose modes** (`compose::compose`): Normal, Add, Multiply and
  Screen, with the layer color applied to light and coverage, each one
  rounding of the exact sum (`compose_tinted`). Mode 0 (`Direct`)
  never gets here. White layer color takes fast paths that give the
  same pixels.
- **Filters** (`filter.rs`):
  - mosaic: the frame sampled bilinearly into a small part of a half-size
    frame, then drawn back with nearest; blocks are n frame pixels, and a
    block of even n is the mean of its middle four;
  - blur: 2x2 average to half size, 3 to 9 taps along rows and then
    columns in 16.16 fixed point, each pass rounded to 8 bits, then scaled
    back with nearest; all in integers. Runs of one color take a short
    way;
  - raster: per-row or per-column offset and scale, OpenGL-style
    coordinates, from the array as it was at `LFILTER`;
  - color: HSV shift of each distinct color, through a small hash table,
    on the premultiplied texel in f32 as SmileBASIC 4 does.

### 1.5 The frontend (`crates/sb-sdl`)

- **Screen texture.** `frame_texture` (`main.rs`) creates one
  `ARGB8888` streaming texture per (frame size, filter). Before
  creating it, it sets the renderer's default scale mode:
  - XSCREEN mode 0 is `SDL_SCALEMODE_LINEAR`;
  - mode 1 goes through `SmartFrame`: a nearest texture is drawn, when the
    frame changes, into a target texture twice its size, which is shown
    with `SDL_SCALEMODE_LINEAR`. That sum is exactly SmileBASIC 4's, with
    no shader. `smart_scale` does the same on the CPU, the reference for
    tests and the fallback if the target cannot be made (`SB_UI_BENCH`'s
    `cpu-smart` forces it);
  - the editor's frame still uses `SDL_SCALEMODE_PIXELART`. The Rust
    bindings lack a per-texture call for it, hence
    `SDL_SetDefaultTextureScaleMode`;
  - a subprogram's screen is `SDL_SCALEMODE_LINEAR` in every mode: SB4 draws
    it as it does a screen in mode 0 (TEST56 shows mode 1), fitted to
    XSUBSCREEN's rectangle (`SubScreen::quad`) with BACKCOLOR in the bars;
  - mode 2 is `SDL_SCALEMODE_NEAREST`.

  The whole frame is uploaded with `Texture::update` only when
  `screen_revision` moves: 3.5 MiB at 1280x720. `screen_rect` scales it
  to fit the display at the aspect ratio, by whole factors only under the
  menu's sharp choice.
  `canvas.copy` draws it.
- **Sub interpreter screen.** `Machine::sub_screen` is a premultiplied
  frame in its own texture with `SDL_BLENDMODE_BLEND_PREMULTIPLIED`,
  placed where `XSUBSCREEN` says.
- **Editor.** `Runtime::render_editors` draws into `editor_fb`, keyed by
  `editor_key`, so it redraws only on change. It is shown in the main
  window or in its own window with its own canvas and texture.
- **UI layers** (`layer.rs`): the menu bar, the debugger panel and
  overlays. Each has its own `Framebuffer` and texture, drawn again
  only when a dry run says something changed, and uploaded only in the
  changed rectangle.
- **Debugger.**
  - The panel's screens view uploads `Machine::render_screens` into its
    own texture.
  - The GRP tab (`grp_view.rs`, `PageTexture`) uploads a whole
    2048x2048 page only when `Graphics::changes(page)` moved. It keeps
    halved copies for fitted views.
  - `Runtime::sprite_corners` gives the debugger where a sprite lands,
    using the same matrices as the renderer.
- **Present** is `pacing.rs` (vsync handling, frame skipping).
- **Window screenshots**: the app has no PrintScreen command. The only
  window capture is `SB_UI_BENCH`'s `shot=PATH` (`bench.rs`), which
  reads the canvas with `SDL_RenderReadPixels` into PAM files.

### 1.6 Who reads pixels back

| Reader | Reads | Notes |
|---|---|---|
| `GPGET`, `GSAVE`, `GCOPY` (source), `GPAINT`, `GARRAY` | GRP pages (`Graphics::read`, via the GARRAY array when there is one) | Never the composed screen. |
| Sprites, text screens (TPAGE), GPUTCHR | GRP pages, GRP5 | Inside rendering. |
| `CHKCHR`, `TSAVE`, `TARRAY` | Text cells | Not pixels. |
| `smilebasic-check --screen`, `--bench ... PNG`, `--play` shots | `Machine::render()` frame, via `save_screen` | Headless; PNG at XSCREEN size, or the 1280x720 display with a sub screen. |
| `SB_UI_BENCH shot=` | SDL canvas (`read_pixels`) | Whole window, including the UI. |
| Debugger GRP tab, screens view | GRP pages, `render_screens` frame | |
| Tests (`crates/sb-runtime/tests/*.rs`) | Frame pixels | Exact pixel asserts. |

SmileBASIC 4 has no command that reads the composed screen.
SmileBASIC 3's `GSPOIT` is not in SmileBASIC 4, and `GPGET` reads only
graphic pages. **Only GRP drawing can be checked on hardware by
automatic readback.** Everything composed (sprites, text screens,
layers, filters, FADE) needs a lossless screenshot. The hardware tests
in [hardware-tests.md](hardware-tests.md) already do this: Ryujinx
screenshots at 1:1 under `XSCREEN 1280,720`, compared with
`smilebasic-check --screen`.

### 1.7 Measured per-frame costs

These are the costs of `Machine::render` in headless runs:
`smilebasic-check --bench 600 PROJECT PROGRAM` on scratch copies of the
projects, release build, one x86-64 desktop with 16 threads (the
renderer is single-threaded). The tree was at d402543, before the
drawing optimisation that is under way. Take the numbers as orders of
magnitude.

| Program | Scene | draw mean | median | 99% |
|---|---|---|---|---|
| NEON_DRIFTWAY `MAIN.PRG` | 1280x720; 12 sprites of 896x400 (about 4.3 Mpx); layers 3, 5, 6 Multiply, layer 4 Add, color filters on 5 and 6, blur 384 on direct layer 7 | 11.8 ms | 9.9 ms | 65.9 ms |
| ELE2020 `MAIN.PRG` | 1280x720; about 206 sprites; five text screens | 9.6 ms | 9.4 ms | 12.7 ms |
| FINITI `DEMO-SKYBOX` | 1280x720; six 1024x1024 sprites, each on a layer with a 4x4 LMATRIX (cube faces); `#GSPRITE` | 5.6 ms | 7.2 ms | 9.0 ms |
| SOLIDGUNNER_V `MAIN.PRG` | typical sprite game | 1.15 ms | 1.24 ms | 1.72 ms |
| DVD_BOUNCE `MAIN.PRG` | one moving sprite | 0.51 ms | 0.50 ms | 0.72 ms |

How the costs split:

- They scale with covered pixels times layers. Full-screen composed
  layers and filters are the expensive part: each composed layer
  clears and composes a 3.5 MiB buffer, and blur and color filters
  touch every pixel.
- Small sprite games are cheap.
- Uploading a 1280x720 frame is a further 3.5 MiB a frame.

## 2. Rendering rules and open questions

Each rule below is marked:

- **C**: confirmed on hardware or by the manual. The source of the
  confirmation is cited.
- **U**: UNVERIFIED in the code, or guessed without a hardware check.
- **S**: a stale marker. The code says UNVERIFIED, but
  [sb4-pitfalls.md](sb4-pitfalls.md) records a hardware result. The
  marker should be dropped (the facts stay in the docs, not in code
  comments).

Each **U** item has an ID (R1, R2, ...) and a proposed check. The
checks are grouped into batches in [2.10](#210-proposed-ttest-batches).

### 2.1 Blending and colour arithmetic

| Rule | Where | Status |
|---|---|---|
| One rounding per draw: products and blends are exact (each 8-bit value is k/255) and rounded to nearest once when stored: element to layer (or frame), layer to frame, frame to display | `compose::Paint` | C: SmileBASIC 4; TEST49 (every sample within 1, the rest the host GPU's rounding); see pitfalls "How colours are worked out" |
| Normal blend of an element: `light + d*(1-a)`, one rounding | `compose::Over1`, `Over3`, `Over5` | C: TEST49; TTEST16 |
| Frames and composed layers hold premultiplied colour, alpha as coverage (SmileBASIC 4 keeps transmittance, 255 less it); a layer starts clear; an additive element in a composed layer adds light without coverage | `compose.rs` header | C: pitfalls "An additive sprite inside a composed layer..." |
| Additive element: `d + light`, the light `t*c*a` exact and rounded once, with saturation, destination alpha kept | `compose::Lit` | C (R1): TEST49 |
| Sprite colour (SPCOLOR) multiplies each channel, alpha included, unrounded, inside the one rounding of the draw | `graphics::Tint`, `compose::Over3` | C (R2): TEST49. A child's link colour `floor(a*b/255)`, alpha too, applied parent first (`multiply`): C, TEST56, no misses |
| Text colour (cell colour, then TCOLOR) the same, unrounded | `compose::Mixed`, `Over5` | C (R2): TEST49. |
| An inverted cell, attribute bit 7, on the console only (no other screen's program has the flag; TEST56 part 1 shows plain glyphs there): the texel is taken as `n,n,n,n` for `n = 1 - alpha` and goes through the same sum; any character, an empty cell too | `inverted_texel`, `attr::INVERT`, `TextScreen::inverts` | U on the console: notes. C elsewhere: TEST56. Not the INPUT cursor (TEST50) |
| TLOAD, TUPDATE and TCOPY keep all 16 attribute bits; TSAVE, TARRAY and CHKCHR read them | `Cell` | C: TEST56 parts 1 and 4 |
| Tint is applied to the page pixel before blending | `raster::sprite::draw` | C (R2): TEST49 |
| Relative colour animation `"C+"`: product / 256, rounded down | `anim.rs` | C: pitfalls |
| LAYER mode 0 draws straight onto the frame, layer colour ignored | `Runtime::render` | C: pitfalls; TTEST16 |
| Modes 1-4: the layer colour scales light (`P = p*c*ca`) and coverage (`A = la*ca`), unrounded | `compose::compose_tinted` | C (R3): TEST49, colours `C0806040`, `7F40C0FF`, `FFC08040`, `80FFFFFF`, white |
| Mode 1: `P + d*(1-A)`; mode 2: `d + P`; mode 3: `d*(P + 1-A)`; mode 4: `d + P*(1-d)`, one rounding each; mode 1 adds to the frame's coverage, mode 2 keeps it, modes 3 and 4 make it opaque | `compose::compose` | C (R3): TEST49. TEST56: modes 3 and 4 make the whole subprogram's screen opaque |
| A sub interpreter's frame is premultiplied over the main one (dimmed to 224/255), cleared to BACKCOLOR's colour unmultiplied with coverage `255 - alpha` (alpha 0: clear); an opaque BACKCOLOR replaces the main screen | `Scene::build`, `screen::cover` | C (R4): TEST49 for alphas 0, 1, &H7F, &H80, &HFE; TEST56 for &HFF, and over console text for &H40 and &HC0 |
| Beside a subprogram, the side without the focus: times 224/255, rounded in the same step as FADE | `screen::cover`, `screen::darken` | C: TEST49 |

### 2.2 Sprites

| Rule | Where | Status |
|---|---|---|
| Placement: home to position, scale, then clockwise rotation in degrees | `Affine::place` | C: manual (SPHOME, SPSCALE, SPROT) |
| Flip first, then quarter turn | `Source::page_xy` | C: pitfalls |
| Sampled without smoothing, at frame pixel centres; away from ties every rotation and scale fits with no misses | `draw_image`, `draw_upright` | C (R5): TEST50 generic angles |
| With XSCREEN sample > 1, sprites are sampled at the finer pixels' centres; LCLIP grows to whole fine pixels | `Layer::fine` | C (R6): TEST50 part 2 (LCLIP exact; the fine grid misses only 56 tie pixels of 54298) |
| Coverage: a pixel is drawn when its centre, nudged right then up, lies on the image, at any rotation: an edge through the centre takes it if it is the left or bottom edge on the frame | `Placement::sampling` | C (R5): TEST50, no coverage miss in any part, text included (consistent with a rasteriser's top-left rule on a target drawn flipped) |
| A sample point exactly between texels, at a multiple of 90 degrees: image side a power of two and scale a power of two: the page's higher texel; anything else: the page's lower, at any turn or flip. The point may land just off the image, and reads the page there; before the page's first texel it reads that one, after the last nothing | `Placement::texel_tie`, `page_signs`, `texel`, `Source::wrap` | C on a Switch 2, 3831 sprites (TEST63); the first case without a miss on the exact shots, the second about 9 in 10 of what it covers; see below |
| The same at other angles (only 45-degree multiples can tie): image x higher, image y lower; kept on the image | `Placement::texel_tie` | U (R5): TEST50, no misses but 21 pixels reading the row above the image |
| SPCHR W and H are the size as shown (turned size rule) | `sp.rs` | C: pitfalls; TEST45, TEST46 |
| GRP-1 is white everywhere; GSAMPLE 1 repeats the page, 0 is transparent off it | `sprites::sample`, `Source` | C: manual (SPPAGE, GSAMPLE) |
| A linked child's offset turns and scales with the parent; the flags decide the child's own image | `Sprites::pose` | C: pitfalls (the `pose` UNVERIFIED comment is partly stale) |
| Children without link bit 0 stay where they showed, Z included, when the parent is cleared | `Sprites::clear` | S: pitfalls ("Z included"); marker at `sprites.rs` "that Z also becomes absolute" |
| Animated XY, Z, UV and I keep whole numbers in 16 bits and divide, `(from*left + to*gone)/frames` dropping the fraction; colours mixed in single precision by the frames left, rounded down; V in double precision | `anim::mix` | C: pitfalls; TEST47 (R7), TEST56, TEST60 (UV, I), TEST62 (V reads 0.0333333333 in ten decimals) |
| Rotation and scale kept in single precision, slid as `from*left + to*(1-left)` in single precision and not rounded | `sprites::single`, `anim::mix` | C: pitfalls; TEST48 |
| SPANIM, TANIM and FADE take their first step at the frame job's first update after the command; no flag for a program that is behind | `Runtime::animate` | C: pitfalls; TEST60 on a Switch 2 |
| SPDEF's default table when none was imported (grid of 16x16) | `grid_defs` | Stand-in, not a rule. The real table is imported |

### 2.3 Text screens

| Rule | Where | Status |
|---|---|---|
| Flip then turn for ATTR | `raster::text::draw` | C: pitfalls |
| Ties: as for sprites of the cell's size, per cell and by its ATTR. A cell reads the page past itself, what lies beside it on the font page or `TPAGE` | `draw_turned`, `draw_upright` | C on a Switch 2, 12 screens of 16 cells (TEST63) |
| Font types 24-64 show only user-defined characters; GPUTCHR with them reads GRP5 in 24-pixel cells | `glyph_pixel_px` | C: pitfalls |
| Tab stops every 4 columns | `TAB_WIDTH` | S: pitfalls; marker in `text.rs` |
| `CHR$(10)` starts a new line | `TextScreen::print` | S: pitfalls; marker in `text.rs` |
| TSCREEN clears the screen | `set_font` | S: pitfalls; marker in `text.rs` |
| TSCREEN keeps TLAYER, TPAGE, TVAR and TFUNC, and sets TBLEND back to 0 | `restart_look` | C: pitfalls; TEST47, TEST48 |
| INPUT cursor: a bar 2 by 8 at the cell's left, opaque white whatever the colour; it alternates with nothing | `raster::text::draw` | C (R9): TEST50, parts 6 and 7 |
| The cursor's blink: 20 frames on, 20 off, counted in frames played; on from the start of a line and from each key that moves or edits, the first period a frame short | `LINE_CURSOR_BLINK`, `LineEdit::key` | C by SmileBASIC 4, the count stepping with `MAINCNT`'s; TEST56 part 1 fits 2 of 2 shots at offsets -3..-1, weak alone |
| The cursor's width is the cell's shifted right by two, or the whole cell after Insert; its height the cell's; with TSCALE and LMATRIX; right after its screen's cells, under what is in front | `LineCursor`, `raster::text::draw` | C: TEST56 part 1, 24 wide at scale 3, cut by a sprite in front |
| On a composed layer the cursor has no coverage, so it adds | `LineCursor::composed` | C: TEST56 part 1, `E0F0FF` |
| Only direct mode's line selects (`LineEdit::selects`), drawn inverted in the console's colour, staying after Enter | `LineEdit::draw`, `finish` | C that INPUT does not: TEST56. U: the look, from SmileBASIC 4; LINPUT |
| Text animations: as for sprites | `impl Animated for TextScreen` | C: pitfalls; TEST47 (R7) |
| TBLEND 1 is additive, as for sprites | `screen.rs` | C: manual (TBLEND); arithmetic as R1 |

### 2.4 Priorities and Z order

| Rule | Where | Status |
|---|---|---|
| Layers 7 to 0, lower layers in front | `Runtime::render` | C: manual (LAYER) |
| Within a layer: larger Z further back | sort key | C: manual (display priority -4095 to 4095) |
| Equal Z: text screens behind sprites; among either, higher id behind | sort key | C (R10): TEST50 part 5, 16 pairs of 16, `#GSPRITE` too |
| `#GSPRITE` goes to the layer SPLAYER gives new sprites after XSCREEN | `set_mode` | C: pitfalls |

### 2.5 Layers and projection

| Rule | Where | Status |
|---|---|---|
| `LCLIP` corners inclusive | `Clip` | C: pitfalls; TEST50 part 2 at XSCREEN sample 2: grows to whole fine pixels, `[2a, 2b+1]` |
| `LMATRIX layer,hx,hy` moves the layer by minus the home point | `screen.rs` | C: pitfalls |
| The 2D LMATRIX form: `Affine::place(pos, home, rot, scale)` like a sprite: scale, then turn clockwise, x and y outside; after the element's own placement | `screen.rs` | C (R11): pitfalls; TEST53 part 1 |
| 4x4 LMATRIX: `(x,y,0,1)` to clip space, X right, Y up, column-major, perspective divide | `compose::homography` | C: pitfalls; TTEST26, TTEST27 |
| Sprites and text screens on a 4x4 layer show whole, also off-screen parts | `project_image`, `scene::flat_area` | C: pitfalls; TTEST36 (text bounded: [differences.md](differences.md)) |
| 4x4: a sprite's Z is not used; shown where W > 0 and Z/W is from -1 to 1 | `compose::project`, `project_image` | C (R12): pitfalls; TEST53 part 2 |
| 4x4: sampled without smoothing | `compose::project` | C (R12): pitfalls; TEST53 part 2 |
| LCLIP on a 4x4 layer is in frame pixels, after projection, inclusive, Y from the top | `project(..., clip)` | C (R13): pitfalls; TEST53 part 3 |
| Errors for a bad 16-element array | `screen.rs` | U, not rendering (skip) |

### 2.6 Filters

| Rule | Where | Status |
|---|---|---|
| Mode-0 layer: LFILTER filters everything drawn so far, BACKCOLOR included; composed layer: only its own | `Runtime::render` | C: pitfalls; TTEST22/23 |
| The same for a mode-0 layer with no elements | `Runtime::render` | S: pitfalls ("That holds for a layer with nothing in it, too"); marker in `runtime.rs` |
| Mosaic: the whole frame drawn into `ceil(width/n)*sample` x `ceil(height/n)*sample` half-buffer pixels with bilinear sampling, then back with nearest; blocks are n frame pixels at any sample | `filter::mosaic` | C (R14): pitfalls; TEST54 part 1, no pixel more than 1 off; TEST56 part 2, n=7 on 640x360, no pixel off |
| Blur: bilinear to half size (2x2 mean, halves down), a row pass and a column pass each stored in 8 bits, nearest back; 0 sharp | `filter::blur` | C (R15): pitfalls; TEST54 parts 2, 3, TTEST24 |
| Blur weights: a Gaussian of variance amount/10 at odd distances, normalised over 31 taps, used in the 3, 5, 7 or 9 taps that the 99% rule picks, not normalised again | `filter::blur_kernel` | C (R15): SmileBASIC 4; TEST54 parts 2, 3, no pixel more than 1 off |
| Blur halving is in frame pixels whatever XSCREEN's sample | `filter::blur` | C (R15): TEST54 part 3 |
| Mosaic and blur work on what shows through (255 less our coverage), so a wide blur darkens behind a clear layer | `filter::shrink`, `expand` | C: TEST54 part 2 |
| Raster: `[n,4]` entries of X offset, X scale, Y offset, Y scale, OpenGL axes, centre times scale plus offset, repeat flag required | `filter::raster` | C: pitfalls; TTEST25/26 |
| Raster: scale, then offset; the Y offset's sign is + from the bottom | `filter::raster` | C (R16): pitfalls; TEST54 part 4 |
| Raster `[n,2]` form acts along the deformation; a row takes entry `floor((i+.5)*n/size)` | `raster_rows`, `raster_into` | C (R17): pitfalls; TEST54 parts 5, 6 |
| Raster on a direct layer: what falls off shows black | `Runtime::render` | C: pitfalls |
| Raster after LMATRIX | `Runtime::render` | C: pitfalls; TTEST27 |
| Colour conversion: hue in degrees, S and V on 0-255 | `filter::shift_color` | C: pitfalls; TEST54 part 7 |
| Colour conversion arithmetic: the shader's formulas in f32 on the premultiplied texel; a grey keeps saturation 0 | `shift_color` | C (R18): SmileBASIC 4; TEST54 part 7, no pixel more than 1 off |
| Filter before the layer colour (modes 1-4) | `Runtime::render` | C (R19): TEST53 part 4. Ours rounds to 8 bits between the two, SB4 does not: up to 1 off |
| A clear pixel is left alone in mode 1 only; in modes 0, 2, 3, 4 a raised value lights it | `filter::color` | C (R19): TEST53 part 4, layer 5; TEST56 part 2 for modes 0, 2, 3 |
| ACLS clears LFILTER | `Runtime::acls` | C: pitfalls; TEST47 (R20) |

### 2.7 FADE

| Rule | Where | Status |
|---|---|---|
| Fade steps: single-precision mix, rounded down; a clear end takes the other end's colour | `Fade::step`, `Fade::start` | C: pitfalls |
| ACLS clears FADE | `acls` | S: pitfalls; marker in `runtime.rs` |
| Fade cover: `(c*a + p*(255-a) + 127) / 255` (nearest), over the final frame, its alpha kept | `screen::cover` | C (R21): TEST49, four fade colours (alpha &H80, &H7F, &H01, &HFE) |
| A fade steps with the animations, starting on the frame after FADE | `Runtime::animate` | C: pitfalls; TEST47 (R22) |

### 2.8 Display (XSCREEN composite mode)

| Rule | Where | Status |
|---|---|---|
| Mode 0 bilinear, mode 2 nearest; BACKCOLOR not in the side bars (black) | `frame_texture` | C (R23): TEST55, window captures identical over the whole frame |
| Mode 1: `t = clamp(2*fract(s-.5)-.5, 0, 1)` per axis at every scale, by bilinear from a nearest copy twice the size, or on the CPU | `SmartFrame`, `smart_scale` | C (R23): SmileBASIC 4; TEST55 parts 2, 6, window captures within 1 |
| All modes fill the fit rectangle, not whole factors (the menu's sharp choice keeps to whole factors) | `screen_rect` | C (R23): TEST55 parts 3, 4, 5 |
| Aspect ratio other than width/height: non-square pixels, letterboxed | `screen_rect` | C (R23): TEST55 part 4 |

### 2.9 GRP drawing primitives (raster rules)

| Rule | Where | Status |
|---|---|---|
| GLINE pixels: same whichever end first, offset `(2t*minor + k) / (2*major)`, `k = (major + 2*minor) mod (2*major)` from an end on the page | `line_points` | C: pitfalls; TEST47, TEST48; `lines_match_hardware`, `shallow_lines_match_hardware`, `far_lines_match_hardware` tests |
| GLINE from an end outside GCLIP (the page if none): `k = major`, counted from that end (only the steps on the page are walked) | `line_points` | C: pitfalls; TEST47, TEST48, TEST56 (`lines_starting_outside_gclip_match_hardware`) |
| GBOX outline: top and bottom rows, then the sides from the top row down, so the top corners are drawn twice | `Graphics::rect` | C: pitfalls; TEST47 (R24), TEST48 (any corner order) |
| GFILL inclusive | `Graphics::fill` | C: manual |
| GCIRCLE pixel set (midpoint circle) | `circle_points` | C: pitfalls; TEST47 (R25) |
| GCIRCLE arc: between the pixels at `round(cos*r), round(sin*r)` of the two angles, both included | `Graphics::circle` | C: pitfalls; TEST47 (R25), TEST48 (an arc of more than half a turn whose two end pixels are one is the circle) |
| GCIRCLE: equal angles draw nothing; the sector flag draws only the outline | `Graphics::circle` | C: pitfalls |
| Sector edges: GLINEs from the center to those pixels | `Graphics::circle` | C: pitfalls; TEST47 (R25) |
| Equal angles with the sector flag draw nothing; radii of 1000 and 70000 draw | `Graphics::circle` | C: pitfalls; TEST47 (R25) |
| GTRI: rows from the leftmost to the rightmost pixel the edges reach, each pixel once, a degenerate triangle too; each edge walked down from its top by SB4's sum, corners not clamped | `edge_span`, `Graphics::triangle` | C: pitfalls; TEST47 (R26), TEST48, TEST56 (`triangles_past_the_page_match_hardware`) |
| GPAINT: 4-connected flood, border mode stops at the border colour | `Graphics::paint` | C: pitfalls; TEST47 (R27) |
| GCLIP shared by all pages | `Graphics::clip` | C: pitfalls |
| `#G_NORMAL`, `#G_NORMAL2` | `Blend::apply` | C: manual |
| `#G_ALPHA`, `#G_ADD` rounding (floor) | `Blend::apply` | C: comment "rounding matches values read back on hardware" |
| `#G_ALPHA2`: alpha `sa + da - floor(sa*da/255)`, the two colours' weights on 0-255 rounded down | `Blend::apply` | C: pitfalls; TEST47 (R28) |
| GCOPY colour-code multiply (floor), then blend; overlapping copy within a page reads first | `Graphics::copy` | C: pitfalls; TEST47 (R29) |
| GPUTCHR mode 0 clears under transparent glyph pixels | `put_chars` | C: pitfalls |
| GPUTCHRP widths, function form scales spacing | `put_chars_proportional` | C: pitfalls; TTEST27 |
| GPUTCHRP drawing scales the spacing as the width does | `put_chars_proportional` | C: pitfalls; TEST47 (R30) |
| GLOAD palette indexes past the palette leave the page's pixel | `load_rect` | C: pitfalls; TEST47 (R31) |
| GSAVE/GLOAD rectangles off the page read as empty | `save_rect`, `load_rect` | C: pitfalls |
| GSAVE/GLOAD width and height: any, 0 included, up to what memory holds | `grp.rs` (`MOST_PIXELS`) | C: pitfalls; TEST48. U: negative sizes (Out of range, from the code), and the error past memory (Out of range here, see differences) |
| TSAVE and TLOAD width and height: 0 or more, as GSAVE | `txt.rs` (`MOST_CELLS`) | U (R31): from the code; the error past memory stands in as for GSAVE |
| GARRAY/GUPDATE: GPGET sees array writes at once; the screen after GUPDATE | `Graphics::update` | C: pitfalls ([differences.md](differences.md) for the omission) |

**Count.** There were 31 open items, R1 to R31. TEST47 (batches A and
F) settled R7, R20, R22 and R24-R31, and TEST49 (batch B) R1-R4 and
R21, apart from the smaller points left U in the table above. R5, R6, R8,
R9 and R10 went with TEST50, batch C, with the tie rule and the cursor's
rate left U. TEST53, TEST54 and TEST55 (batches D, E and G) settled R11-R19
and R23, with the rest of the blur and mosaic set-up from SmileBASIC 4's own
code; the screenshots came from an emulator on a PC's GPU. TEST54's and
TEST55's are within 1 of ours in every channel, and TEST53's within 2 but
for four lone pixels.
TEST48 then settled most of TEST47's smaller
points: the shallow GLINE rule and lines from off the page, GTRI's
edges, GSAVE and GLOAD sizes, single-precision rotation and scale, GBOX
corner order, near-full arcs and TFUNC after TSCREEN. R1-R4 and R21
were then confirmed from SmileBASIC 4,
which give the rules above exactly; TEST49's screenshots came from an
emulator on a PC's GPU, whose stored results are within 1 of them (see
pitfalls). 5 stale markers can be dropped from
the code at once (ACLS clearing FADE was dropped with TEST47):

- `runtime.rs`: filter on an empty direct layer;
- `text.rs`: TAB_WIDTH; `CHR$(10)`; TSCREEN clears;
- `sprites.rs`: child Z on SPCLR.


### 2.10 Proposed TTEST batches

#### What TEST63 says about ties

TEST63 ran 3831 sprites and 12 text screens on a real Switch 2. Sizes 8 to 64 at
scales 0.5 to 2, whole, half and quarter positions, every turn and flip. The album's
JPEGs of the first three shots have coarse tables (quantisation sum 736, against 64),
so one pixel's V is not to be trusted there; shots 4 to 7, 256 sprites and the text,
read to within 1. Ties are read per line of the image and per triangle, by summing a
line's pixels.

| Rule, tie pixels missed | Sprites, all shots | Sprites, exact | Text |
|---|---|---|---|
| The rule before (TEST62's) | 527889 | 92622 | 30896 |
| All lower | 562972 | 101750 | 45360 |
| All higher | 1026837 | 229466 | 26288 |
| Power of two side and scale higher, else lower | **143769** | **15734** | **6448** |
| The same, frame's x going down higher | 375863 | 73048 | 8920 |
| Of tie pixels | 1589809 | 331216 | 71648 |

By class, for the rule kept, exact shots first and then all shots:

| Side, scale | Missed, exact | Of | Missed, all | Of |
|---|---|---|---|---|
| Power of two, power of two | 0 | 86016 | 2385 | 423973 |
| Other, power of two | 3568 | 102976 | 80879 | 599418 |
| Power of two, other | 5172 | 64512 | 26785 | 256168 |
| Other, other | 6994 | 77712 | 33720 | 310250 |

The first row is exact arithmetic: the side and the scale leave nothing to round, and
floor reads the higher texel. It holds at every flip and turn, which is why the rule goes
by the page and not by the rectangle or the frame, as the rules before did. Everything else
has rounded and comes out just low, whichever way the image runs.

What is left is GPU noise, and not a simple rule: the same 16x16 at 1.5 reads its V high at
Y 404 and low at Y 377; a line along the image can change from high to low part of the
way through; at 270 degrees often one triangle goes the other way. A family of single-precision
models of the rasteriser, Y flipped or not, plane equations relative to a vertex or
to the origin, barycentrics, by division or by a reciprocal, rounding to nearest or
truncating, at each of the 6 anchors (500 models), was scored on the exact shots: the
best, truncating barycentrics, misses 1604 lines of 22112 where the rule misses 1135,
and none of those models does better than the rule. So the rule stays simple, and the CPU
needs no triangles and no fast path for them.

Against the shots, pixels more than 20 of luma off: 280599 before, 67416 after, and on
the exact shots 58124, 10662. TEST62's 22 sprites, all in the noise class where
it counts, go from 105 to 746 off. TEST50's Ryujinx shots get better in part 1 (1249 to
880), the same in 2, and worse in part 3 (464 to 2968) and 4 (594 to 1298). TEST56 part 2,
Ryujinx, goes from 2128 to 649. A real Switch outranks these.

#### What TEST50 says about ties

Every page pixel in TEST50 has its own colour, so each screenshot pixel
names the texel sampled. Away from ties all rules agree. At ties the
screenshots show the GPU's arithmetic: in one quad the texel changes
along the quad's diagonal, between its two triangles, and the same image at the same scale goes different ways at
different places. Simulating the quad in single precision (vertices
through the matrix and viewport, attributes by plane equations and by
barycentrics, snapped to 1/256 or not) did not reproduce which way each
tie goes, so what remains seems to be the host GPU's, and a real Switch
may differ. TEST63 above settled that. What follows is the history; the rule at the time took the usual
outcome:

| Part, group | Pixels | Misses before | After |
|---|---|---|---|
| 1, sprites at multiples of 90 | 20310 | 3930 | 749 |
| 1, sprites at multiples of 45 | 3078 | 54 | 21 |
| 2, sprites at multiples of 90 | 38414 | 1136 | 56 |
| 2, sprites at multiples of 45 | 2450 | 17 | 0 |
| 3, sprites | 7844 | 800 | 192 |
| 3, text | 7844 | 1424 | 272 |
| 4, sprites at multiples of 90 | 6691 | 1489 | 352 |
| 4, text at multiples of 90 | 6691 | 1472 | 274 |

Generic angles have no misses before or after. What is left is mostly
half of a quad at 90 and 270 degrees, where the two triangles differ;
sizes other than powers of two starting on a half pixel; and text and
sprites reading a pixel just off their image, which ours never does.

TEST56 parts 1 and 2 add ties of sizes 8, 15, 16 and 16x15 at scales 1, 1.5
and 2, and in places they go against the rule: a size 15 side at scale 1
on a half pixel goes high, and 8 and 16 at scale 1.5 mostly low. Simple
per-axis changes were measured against all of it, pixels more than 1 off:

| Rule | TEST50 1 | 2 | 3 | 4 | TEST56 P1 ties | P2 ties | All |
|---|---|---|---|---|---|---|---|
| As it stands | 770 | 56 | 464 | 626 | 569 | 2566 | 5051 |
| Odd side at scale 1 goes high | 1533 | 56 | 464 | 626 | 135 | 2566 | 5380 |
| Power of two at another scale goes low on the image | 770 | 56 | 2968 | 1394 | 935 | 1278 | 7401 |
| The same, towards the page's lower coordinates | 770 | 56 | 2968 | 1394 | 935 | 1278 | 7401 |
| Both of the first changes | 1533 | 56 | 2968 | 1394 | 501 | 1278 | 7730 |

Each fixes one test at another's cost, which is what host GPU noise would
look like, so the rule stayed.

TEST62 then showed ties on a real Switch 2, on 21 sprites at multiples of 90
degrees, all with SPSET's attribute 1 (a quarter turn, not meant by the test), so a
sprite's ramp shows only one axis:

- Sizes 8 and 16 at scale 1 go to the higher texel, the point on the image's
  far edge reading the page's next texel, which ours used to keep from happening.
- Sizes 8 and 16 at scale 1.5 go to the frame's left and bottom. Here that is also the
  page's higher texel: nothing tells which of the two is the rule.
- Size 15 at scale 1.5 goes towards the shown rectangle's lower coordinates on
  both axes, where a Ryujinx run of the same sprite (TEST50, S1.5A90) was mixed.
  The page's lower coordinates, the rule before, misses one axis of the two.
- Size 15 at scale 1, the quad's diagonal shows: the first triangle of its
  strip (top left, top right, bottom left of the rectangle) goes to the higher texel
  and the second to the lower. No per-axis rule fits; ours has the second's.
- Single-precision plane equations from the vertices, or barycentrics, with
  any anchor, get all of sizes 8 and 16 and miss sizes 15's as above.

Pixels more than 1 off (TEST50 and TEST56 from Ryujinx, on a PC's GPU; TEST62's a
shot of the 22 sprites, 9 of luma off):

| Rule | TEST50 1 | 2 | 3 | 4 | TEST56 P2 | TEST62 |
|---|---|---|---|---|---|---|
| Before | 770 | 56 | 464 | 626 | 2566 | 290 |
| Reading past the image | 880 | 86 | 464 | 594 | 2128 | 266 |
| Also other sizes along the shown rectangle | 1249 | 86 | 464 | 594 | 2128 | 105 |
| Also power of two at other scales, the page's higher texel | 1249 | 86 | 2512 | 2720 | 2864 | 105 |
| Other sizes towards the frame's top left, not along the rectangle | 3553 | 206 | 464 | 594 | 4037 | 120 |

The third row is what the code does. What a real Switch shows decides over what
Ryujinx shows: TEST50's part 1 gets worse, on a host GPU's noise. Text keeps to its
own cell, off which SmileBASIC 4 reads the font page's next character's pixels.

A GPU backend ([4](#4-gpu-backend-options)) gets these ties from its
own arithmetic unless steered, as `gpu.rs` does by `Placement::tie_sides`.

The batches follow [hardware-tests.md](hardware-tests.md):

- one program with numbered parts;
- `DUMPCON` saves the console as `RESnnPn` for automatic parts;
- screenshot parts wait for a button, so that `smilebasic-check --screen`
  and a lossless SmileBASIC 4 screenshot (Ryujinx, handheld,
  `XSCREEN 1280,720`, 1:1) can be compared pixel for pixel.

Give them the next free TTEST numbers when they are written (the
highest used at the time of writing is TTEST39). For screenshot parts:

- draw each patch as a solid 16x16 or 32x32 block on a known
  BACKCOLOR, so that one pixel inside it tells the answer;
- print a legend, so that patches can be found in the screenshot.

#### Batch A: GRP raster rules (automatic, GPGET readback)

R24-R31. Each part draws on a cleared page and prints the result as
text, so `smilebasic-check` compares it row by row with no screenshot:

```smilebasic
DEF SHOWG W,H
 VAR X,Y,L$
 FOR Y=0 TO H-1
  L$=""
  FOR X=0 TO W-1
   IF GPGET(X,Y) THEN L$=L$+"#" ELSE L$=L$+"."
  NEXT
  ?L$
 NEXT
END
```

Parts (each `GCLS` first):

1. `GCIRCLE 8,8,R` for R = 1, 2, 3, 5, 7 (one part each, or side by
   side with offsets).
2. Arcs: `GCIRCLE 10,10,8,0,90`, `GCIRCLE 10,10,8,350,10`,
   `GCIRCLE 10,10,8,30,120,1` (sector), and `GCIRCLE 10,10,8,45,45,1`
   (equal angles with the sector flag).
3. `GTRI 0,0,12,3,4,11`, a thin sliver `GTRI 0,0,20,1,10,2`, and the
   degenerate `GTRI 0,0,5,5,10,10`.
4. `GPAINT` on a shape whose two halves touch only diagonally, then
   with a border colour.
5. `GBOX 2,2,10,8,&H80FF0000,#G_ALPHA` over `GFILL ... &HFF0000FF`;
   print `HEX$(GPGET(2,2),8)` and `HEX$(GPGET(6,2),8)`. A pixel blended
   twice shows at a corner. Also `GBOX` with `X0>X1` and a 1-pixel-high
   box.
6. `#G_ALPHA2` grid: for destination alphas &H00, &H40, &H80, &HFF and
   source alphas &H01, &H80, &HFE, `GPSET` a colour with mode 3 over a
   `GPSET` destination, then print `HEX$(GPGET(...),8)`.
7. GCOPY with a colour code: copy `&HFF808080` with `&HFF7F7F7F` (floor
   gives `&HFF3F3F3F`, nearest `&HFF404040`) in modes 0 and 2. Then an
   overlapping copy within one page shifted by one pixel.
8. `GPUTCHRP 0,0,"AB",16,2,3` scaled: print the leftmost drawn column
   of B from GPGET, which shows whether the drawn spacing scales.
9. GLOAD with palette indexes past the palette: print GPGET where they
   land (0 if transparent).
10. `GLINE -100000,3,100000,5` and `GCIRCLE 10,10,70000`: print a few
    rows (confirms clipping keeps the same pixels).

#### Batch B: colour arithmetic (screenshots)

Done as TEST49; see the table above for the results.

R1, R2, R3, R21, and R4 as its last part (it needs a subprogram).
Sprites are 32x32 solid blocks drawn on GRP3 (`SPSET n,U,V,32,32` and
`SPPAGE n,3`) on `BACKCOLOR &HFF3C3C3C` (60). Values that tell the
roundings apart:

| Patch | Setup | Floor | Nearest |
|---|---|---|---|
| R2 multiply | page `&HFF808080`, `SPCOLOR &HFF7F7F7F` | 63 | 64 |
| R2 alpha multiply | page alpha &H80, `SPCOLOR &H81FFFFFF`, white over 60 | coverage 64 | coverage 65 |
| R2 text | user character `&HE800`, its image a solid `&HFF808080` square on the TPAGE page; `TCOLOR 0,&HFF7F7F7F` | 63 | 64 |
| R1 additive | page `&H80010101`, `#A_ADD`, over 60 | 60 | 61 |
| R1 saturation | page `&HFFC8C8C8` additive over `&HFF969696` | 255 | 255 (control) |
| control | white `&H80` over 60 | 158 (confirmed) | |
| R21 FADE | `FADE &H80FF0000` over 60 | red 157 | red 158 |
| R3 | layers 1 to 4 in modes 1 to 4, `LAYER n,m,&HC0806040`, each holding a sprite at alpha &H80 over BACKCOLOR and over a white sprite on layer 7 | compare with emulator PNG | |
| R4 | a subprogram (`XSUBSCREEN`) whose `BACKCOLOR` is `&H80FF0000`, over a main screen of known colour | compare | |

#### Batch C: sprite and text sampling (screenshots)

R5, R6, R8, R9, R10.

1. An 8x8 checkerboard sprite (1-pixel squares) at `SPROT` 30, 45 and
   90.5, and at `SPSCALE 1.5,1.5` with `SPROT 10`. This is where
   rotated edges and ties show.
2. The same under `XSCREEN 640,360,2` (frame 1280x720, so the
   screenshot is still 1:1), plus `LCLIP 0,101,101,201,201`, odd
   corners at sample 2.
3. Text screen 0 with a user character of alternating 1-pixel columns
   at `TSCALE 0,1.5,1.5` and `2.5`, next to a sprite of the same image
   at the same scale. Tells whether text ties go left like sprites.
4. Z order: sprites 1 and 2 overlapping at equal Z; text screen 0 and a
   sprite at equal Z on the same layer; each in a different colour.
5. `INPUT` waiting on the console with the cursor on a known cell, in
   a non-white `COLOR` (R9).

#### Batch D: layers and projection (screenshots)

R11, R12, R13, R19.

1. `LMATRIX 0,hx,hy,x,y,sx,sy,rot` with sx != sy and rot 30. This
   shows the rotation direction and whether scale comes before
   rotation for layers.
2. 4x4 matrices:
   - with a nonzero Z row (`M[2]`, `M[6]`, `M[10]`, `M[14]`) that puts
     Z/W outside -1..1 for part of the layer. Does that part vanish
     (depth clipping)?
   - with a matrix that puts part of the layer behind (W < 0): only
     the front part shows.
3. A 4x4 matrix that magnifies a 1-pixel checkerboard 8 times. Nearest
   or smoothed?
4. `LCLIP` on a 4x4 layer. Is the rectangle applied in screen pixels
   after projection (ours), or in layer coordinates before?
5. `LAYER 1,1,&HFF8080FF` with `LFILTER 1,5,120,0,0` (colour
   conversion). Filtering before or after the layer colour gives
   different hues.

#### Batch E: filters (screenshots)

R14-R18.

1. `XSCREEN 640,360,2` with `LFILTER 0,1,8`: are blocks 8 or 16 frame
   pixels?
2. Blur of a single dot and a 1-pixel line at amounts 16, 32, 64, 128
   and 384, one part each (NEON_DRIFTWAY uses 384). Then blur at
   `XSCREEN 640,360,2`, amount 8.
3. Raster type 3 and type 4 with offset 0.1 and scale 0.5 in the same
   entry, on a picture with a marked origin: shows the order and the Y
   sign.
4. Raster with `A[240,2]` against `A[240,4]` holding the same
   offset/scale: tells the meaning of the two-value form.
5. Colour conversion `LFILTER 0,5,30,40,-20` over a 16-colour palette
   strip, and the same on a composed layer whose sprites are partly
   transparent (premultiplied input).

#### Batch F: timing and state (automatic, console)

R7, R20, R22, and TSCREEN's kept settings.

1. `FADE &HFFFFFFFF,4`, then six times `?HEX$(FADE(),8):VSYNC 1`. Shows
   on which frame the first step lands.
2. `ACLS`, then `?LFILTER(0)` after `LFILTER 0,1,8`.
3. `SPANIM 0,"Z",...` and `SPANIM 0,"C",...` with keys that give
   fractional values: print `SPOFS 0 OUT X,Y,Z` and `SPCOLOR(0)` each
   frame. The same with `TANIM` and `TOFS`/`TCOLOR`.
4. After `TSCREEN`, print `TLAYER(0)`, `TBLEND(0)`, `TPAGE 0 OUT ...`
   and TVAR values that were set before.

#### Batch G: display scaling (screenshots of the whole display)

R23. `XSCREEN 400,240,1,M` for M = 0, 1, 2 with a 1-pixel checkerboard
and a diagonal line. Then `XSCREEN 400,240,1,2,1.0`. Capture the 1280x720
output (Ryujinx's own scaling must be off, or the result says nothing),
and compare with the emulator window's `SB_UI_BENCH shot=`. Mode 1 may
need its own resampling code if SDL's `PIXELART` mode is not the same.

## 3. Proposed split

### 3.1 Goals

- SmileBASIC 4's semantics live in one place, a **scene builder** in
  `sb-runtime`. Backends only rasterize what it gives them.
- The CPU rasterizer is the existing code, moved behind a trait and
  kept pixel for pixel. It stays the reference and the fallback.
- `sb-core` and `sb-runtime` stay frontend-free: no SDL, wgpu or OS
  types (see [CONTRIBUTING.md](../CONTRIBUTING.md)). The scene must be
  plain data, so that a later WASM build can use the CPU rasterizer
  with a canvas, or a WebGL2/WebGPU backend.
- No per-frame allocation in the scene: its vectors are kept and cleared.

### 3.2 The scene (display list)

`crates/sb-runtime/src/scene.rs` holds it, built each frame that the redraw
gate lets through. In short (the source has the rest):

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

- **Sprites** are one `SpriteItem` each. The `Sprites::pose` link
  arithmetic, the blank-page rule, `Affine::place` and `Layer::fine` are
  in the builder. A sprite that shows nothing keeps its place: its layer
  is still cleared and composed, and it still ends a run of text screens
  on a 4x4 layer, so leaving it out would change pixels.
- **Text screens** are one `TextItem` each, and the cells are not copied:
  the rasterizer gets them by reference. The earlier plan of one item per
  cell would mean up to 14400 cells on each of five screens, 88 bytes or so
  each, 6 MiB a frame, to redo what `draw_upright` does from the cell array.
  A GPU backend turns cells into quads itself, with the same lookups as
  `TextItem::glyph` (TPAGE, GRP5, then `Font::pixel`, which still needs an
  atlas). Coverage is the same either way: a frame pixel belongs to the
  cell under its centre (`(x / size, y / size)`).
- **Text on a 4x4 layer** keeps its semantics: drawn flat into an offscreen
  of the size in `Projection`, each run then projected. Projecting each cell
  directly would remove the bound described in
  [differences.md](differences.md). That is a behaviour change: decide it
  after TTEST36-style checks.
- **Raster filters** copy the array's rows at `LFILTER`, as SmileBASIC 4
  does, and those go into `Scene::raster_rows` when the scene is built.
- **The sort** is exact: layer, Z, kind, id, on packed `u64` keys, without
  moving items.

The rasterizer trait (`raster/mod.rs`), with dispatch at most once a frame.
`Runtime` holds a `CpuRasterizer` directly, so nothing is dynamic today:

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

The file-level split is that `scene.rs`, `sprites.rs` and `text.rs` (making
items) decide what shows, and `raster/` decides how: path choice, ties
(`Placement::texel_tie`), `Tint` and the glyph tables. `compose.rs` and
`filter.rs` hold pixel arithmetic that only `raster/` and GRP drawing use.

### 3.3 Redraw on change

The redraw gate is first and cheap: `changed`, `take_changed`,
`drawn_with`. It decides whether a scene is built at all. For Phase 2:

- The built scene is compared with the last one. Use a hash of
  `passes`, the items, `fade`, the page change counts and a change count
  for each screen's cells, which are not in the scene. If equal, the
  frame is reused. This catches commands that set the same values,
  such as `SPOFS` to the same place every frame, which today always set
  `changed`.
- Inputs read by reference at render time must enter the comparison: GRP
  pages and cells by change counts. Raster rows are by value already. A
  program that only writes a raster array needs no redraw: SmileBASIC 4
  copies the array at `LFILTER`, so such writes show nothing (TEST60).
- A GPU backend can go further and cache per-pass targets: a composed
  layer whose items, inputs and pages did not change keeps its texture.

### 3.4 GRP pages stay on the CPU

All page reads (`GPGET`, `GSAVE`, `GCOPY`, `GPAINT`, GARRAY) and all
drawing stay CPU-side, as now. So no GPU readback is ever needed, and
SmileBASIC 4's rules for GARRAY and GUPDATE stay in `graphics.rs`.

For the GPU, each page that is in use is mirrored as a 2048x2048
texture:

- **Format.** `ARGB8888`, the same bytes as `Graphics` (one `0xAARRGGBB`
  word), sampled with nearest, is what SDL_Renderer takes and all the sums
  need ([4.2](#42-a-sdl_renderer-as-now)). `R32Uint` holds the same bytes for a
  shader that works in integers, should one ever need to.
- **Dirty tracking.** Extend the per-page change counter to a coarse
  tile bitmap, for example 64x64-pixel tiles, so 32x32 bits a page.
  `plot`, `fill`, `copy`, `put_chars`, `put_image`, `load_rect`,
  `update`, `set_page` and `clear_page` mark it. `sync_pages` uploads
  the marked tiles, merged into row spans, and clears the bitmap. The
  debugger's GRP tab (`grp_view.rs`) can use the same bitmap instead
  of uploading whole pages.
- **Memory.** A blank page needs no texture (sprites on it draw
  nothing). Six pages in use are 96 MiB of textures. Allocate on first
  use.

### 3.5 Headless tools

`smilebasic-check` (`--bench`, `--play`, `--screen`, part comparison)
and the tests always use the CPU backend. It is deterministic and needs
no GPU, so results do not depend on the machine. The CPU rasterizer
stays in `sb-runtime`.

A GPU backend for comparison runs sits outside `sb-runtime`:

- with wgpu, a new crate (for example `sb-render-wgpu`, depending on
  `sb-runtime`; the crate table in CONTRIBUTING.md gains a row);
- with SDL's GPU API, inside `sb-sdl`. SDL can create an offscreen GPU
  renderer with a null window (`SDL_CreateGPURenderer(NULL, NULL)`),
  so comparison tests can still run without a display.

### 3.6 What stays on the CPU

- Emulator UI drawn on the frame: the hint band and the completion list,
  in `Runtime::draw_overlay`. They are drawn on the CPU either way: on its frame,
  after the rasterizer, or for a GPU frame on a clear `Framebuffer` of their own,
  which is sent when it changes and put over the frame.
- Dimming is not an overlay: it shares one rounding with FADE
  (`screen::cover`), so it stays in the scene (`Scene::dimmed`).
- The editor, menu bar, panels and dialogs. They already redraw only
  on change, and they are not SmileBASIC 4 rendering.
- GRP drawing, and anything that has no GPU path yet. If a scene needs a feature
  the GPU backend lacks, the whole frame is drawn on the CPU. The one mix is the
  color filter off the `gpu` driver, which needs no readback: that layer is drawn
  on the CPU and sent ([phase 3](#6-phased-plan)).

## 4. GPU backend options

### 4.1 What the backend has to do

- Textured quads under an affine matrix, with **nearest sampling at
  pixel centres**. Projected quads under a
  homography with W > 0 only.
- Per-item tint, two blends (over and additive), on an opaque frame or
  a premultiplied layer target.
- Compose modes 1-4 with layer colour.
- Filters: mosaic, two-stage blur, per-row/column raster resampling,
  HSV colour shift.
- FADE, and the final scaling (linear / pixel-art / nearest).

SmileBASIC 4 itself does this with fixed-function blending, and its
model maps one to one onto GPU blend states. SmileBASIC 4 output
premultiplied colour (alpha 0 for additive items), render targets keep
transmittance in their alpha, and every draw is rounded once, by the
hardware, when stored to RGBA8. It uses five blend states, all with
the ADD equation:

| State | Colour (src, dst) | Alpha (src, dst) | Used for |
|---|---|---|---|
| BS0 | off (overwrite) | off | presenting the main screen, copies |
| BS1 | `SRC_ALPHA`, `1-SRC_ALPHA` | `ZERO`, `ONE` | colour fills, the mouse cursor |
| BS2 | `ONE`, `1-SRC_ALPHA` | `ZERO`, `1-SRC_ALPHA` | every element; layer modes 1 and 2; presenting a subprogram's screen |
| BS3 | `DST_COLOR`, `1-SRC_ALPHA` | `ZERO`, `ZERO` | layer mode 3 |
| BS4 | `1-DST_COLOR`, `ONE` | `ZERO`, `ZERO` | layer mode 4 |

A GPU backend can use the same states: an RGBA8 target per layer
cleared to transmittance 1 (coverage 0, as our layers are), the
element shaders giving `t*c*a` and `a` (or 0 for additive items),
the composite shaders giving `P` and `A`, and FADE with the 224/255
dimming in the present shader. The CPU renderer works the same sums
exactly, in integers, and rounds once per draw, so the two differ only
where the GPU's own float-to-8-bit conversion rounds a value very near
a half the other way (about one patch in six, by 1, on the PC GPU
behind TEST49's screenshots). Our alpha is coverage, 255 less
SmileBASIC 4's transmittance; a backend that keeps coverage takes the
usual premultiplied alpha factors (`ONE`, `1-SRC_ALPHA`) for BS2 and
writes an opaque alpha for modes 3 and 4.

### 4.2 (a) SDL_Renderer, as now

This is what the frontend uses today: it uploads the CPU frame and
scales it.

- **Drivers.** SDL 3.4 has direct3d (9), direct3d11, direct3d12,
  opengl, opengles2, metal, vulkan, gpu and software. On Emscripten it
  uses opengles2 (WebGL).
- **Geometry.** `SDL_RenderGeometry` / `SDL_RenderGeometryRaw` draw
  textured triangles with per-vertex colour, which covers tint. The
  sdl3 crate wraps both (`Canvas::render_geometry`).
  `SDL_SetRenderTextureAddressMode` gives clamp or wrap, which covers
  GSAMPLE. But `SDL_Vertex.position` is a 2D `SDL_FPoint`: there is no
  W, so **projected (4x4 LMATRIX) quads cannot be texture-mapped
  correctly**. They would need fine subdivision, which only
  approximates.
- **Blend modes.** The fixed ones are `BLEND`, `BLEND_PREMULTIPLIED`,
  `ADD`, `ADD_PREMULTIPLIED`, `MOD`, and `MUL` (`src*dst +
  dst*(1-srcA)`, the same formula as compose mode 3).
  `SDL_ComposeCustomBlendMode` gives factor/operation pairs, so compose
  mode 4 (`ONE_MINUS_DST_COLOR`, `ONE`, `ADD`) and "add without alpha"
  can be expressed. Support varies by driver: per SDL's header, opengl
  supports only `ADD` with all factors, opengles2 `ADD`, `SUBTRACT` and
  `REV_SUBTRACT`, and software none.
- **Exactness.** A texel times a vertex color gives `t*c` and `a*ca`
  unrounded, and `SDL_BLENDMODE_BLEND` then gives `t*c*a + d*(1-a)` with
  one rounding at the store: SmileBASIC 4's own sum. `ADD` is its additive
  element; the premultiplied blend, the premultiplied add, `MUL` and a custom
  `1-dst, 1` are modes 1 to 4, with the layer's color times its alpha on the
  vertices. So ±1 where the GPU rounds, as on SmileBASIC 4 itself
  ([phase 3](#6-phased-plan) measured it). An earlier version of this section
  said otherwise, when the CPU's formulas were floors.
- **Filters** have no shader support, but mosaic and blur can be built from
  draws on half-size targets ([phase 3](#6-phased-plan)), and raster perhaps
  from per-row strips. Colour conversion cannot.
- **Rust.** No new dependencies.
- **Verdict.** It draws sprites, text screens, blends, layers, LCLIP and FADE
  on every driver, with no shader, and mosaic and blur; 4x4 layers, raster and
  colour conversion stay on the CPU.

### 4.3 (b) SDL 3.4 `SDL_GPURenderState` inside SDL_Renderer

SDL 3.4 lets an SDL_Renderer draw with a custom **fragment** shader:

- `SDL_CreateGPURenderState(renderer, &SDL_GPURenderStateCreateInfo)`.
  The create info holds a `fragment_shader`, extra sampler bindings,
  storage textures and storage buffers.
- `SDL_SetGPURenderStateFragmentUniforms`, `SDL_SetGPURenderState`,
  `SDL_DestroyGPURenderState`.

Limits, from the vendored header (`sdl3-src` 3.4.18, `SDL_render.h`):

- **Only the `gpu` render driver** (`SDL_GPU_RENDERER`). It runs on the
  SDL GPU API, which has Vulkan, Direct3D 12 and Metal backends only
  (`src/gpu/` has `vulkan`, `d3d12` and `metal`). Machines with only
  OpenGL, OpenGL ES or D3D11, and the web, do not get it.
- **Shaders must be supplied compiled**, in a format the device
  accepts: SPIR-V for Vulkan, DXIL (or DXBC) for D3D12, MSL or metallib
  for Metal. The renderer must be created with
  `SDL_PROP_RENDERER_CREATE_GPU_SHADERS_SPIRV_BOOLEAN` /
  `..._DXIL_BOOLEAN` / `..._MSL_BOOLEAN`. The shader is created with
  `SDL_CreateGPUShader` on the renderer's device
  (`SDL_GetGPURendererDevice`). So we would ship three shader blobs per
  shader, built offline (glslang/dxc/spirv-cross, or SDL_shadercross)
  and committed or generated at build time. SDL's Vulkan backend binds
  combined image samplers: naga cannot make that (its GLSL front end refuses
  `sampler2D`, and its output keeps the two apart).
- **Only the fragment stage is custom.** Vertices are still 2D
  `SDL_Vertex`. Blending is still SDL's blend mode, which is SmileBASIC 4's
  own way of blending anyway ([4.2](#42-a-sdl_renderer-as-now)); only a sum
  that must be bit-exact would need the destination read in the shader. A
  filter draws its target through the shader into another, and they swap. A
  projected quad works if the shader does the inverse
  homography per pixel, drawn over the quad's screen bounds: the same
  inverse mapping `compose::project_image` does on the CPU.
- **Rust.** The sdl3 crate (0.20) has no safe wrapper for
  `SDL_GPURenderState`. Raw `sdl3::sys::render` calls are needed, plus
  `sdl3::gpu::Shader::raw()` or raw `SDL_CreateGPUShader`. The device
  comes from `SDL_GetGPURendererDevice` as a raw pointer.
- **Verdict.** It adds what (a) lacks, on top of (a), and the rest of the UI
  keeps using SDL_Renderer unchanged. It is limited to Vulkan/D3D12/Metal
  machines and the fragment stage.

### 4.4 (c) The SDL3 GPU API directly

- **Features.** Full pipelines (vertex and fragment shaders),
  render-to-texture, integer texture formats (`R32_UINT`,
  `R8G8B8A8_UINT`), storage textures and buffers, and **compute
  passes**. A compute pass can do a whole layer's blending in integers
  exactly, and can do the blur and colour filters.
- **Backends:** the same Vulkan, D3D12 and Metal; no OpenGL, GLES or
  web. Requirements per `SDL_gpu.h`:
  - Vulkan 1.0 with `VK_KHR_swapchain`, `VK_KHR_maintenance1`,
    `independentBlend`, `imageCubeArray`, `depthClamp`,
    `shaderClipDistance`, `drawIndirectFirstInstance` and
    `sampleRateShading`; some can be relaxed with
    `SDL_PROP_GPU_DEVICE_CREATE_FEATURE_*` for Android-class devices;
  - D3D12 on Windows 10 or later, feature level 11_0, resource binding
    tier 2; tier 2 can be relaxed for Haswell/Broadwell;
  - Metal on macOS 10.14 or later (Apple Silicon or an Intel Mac2
    family GPU).
- **Shaders:** the same three-format problem as (b).
- **Rust.** sdl3 0.20 has a `gpu` module with safe wrappers (`Device`,
  `Shader`, graphics and compute pipelines, passes, transfer buffers).
  There are no new dependencies.
- **Window.** A GPU device that claims the window
  (`Device::with_window`) and an SDL_Renderer cannot both present to
  the same window. Either:
  - the UI (`layer.rs` panels, menu bar, editor window) moves to GPU
    textures and quads, or
  - the gpu render driver is used for the UI (`SDL_CreateGPURenderer`
    with our own device), which shares one device between SDL's 2D
    renderer and our passes.

  Check the second way on all three backends before relying on it.
- **Verdict.** It gives full control and exact integer paths, with no
  new crates. It does not cover GL-only machines, GLES handhelds
  without suitable Vulkan, or the web.

### 4.5 (d) wgpu

- **Backends:** Vulkan, Metal, D3D12, OpenGL / OpenGL ES 3.0+ (via EGL
  or WGL), and on wasm32 WebGPU and WebGL2.
- **Shaders** are written once in WGSL. naga translates them at run
  time to SPIR-V, MSL, HLSL or GLSL, so there is no offline toolchain.
- **Integer paths.** Integer textures and `textureLoad` work on every
  backend, including GLES3/WebGL2, so the exact fragment-shader path
  works everywhere. Compute shaders are not available on the GLES or
  WebGL2 backends; the exact path there must be fragment shaders with
  ping-pong.
- **Window.** The sdl3 crate's `raw-window-handle` feature exposes the
  SDL window to wgpu (the crate ships a `raw-window-handle-with-wgpu`
  example). As with (c), wgpu then owns presentation of that window.
  SDL_Renderer cannot draw to it as well, so the UI must move to wgpu
  or be uploaded as textures. SDL textures cannot be shared with wgpu
  without readback.
- **Cost, measured** in a throwaway crate outside this repository:
  wgpu 30.0.1 with default features, plus `pollster`, creating an
  adapter, a device, a WGSL module and a 2048x2048x6 texture.
  - Dependencies: `cargo tree -e normal` resolves wgpu to 61 unique
    crates on x86_64 Linux, 72 on Windows MSVC, 58 on aarch64 macOS
    and 44 on wasm32. The whole workspace has 43 on Linux today.
  - Build time: a clean release build took 23 s on a 16-thread
    x86-64 desktop.
  - Binary size: 4.7 MB stripped, against about 0.3-0.4 MB for an empty
    stripped Rust program.
  - The probe ran on the NVIDIA Vulkan driver and on Mesa's lavapipe
    (software Vulkan, `VK_ICD_FILENAMES=.../lvp_icd.x86_64.json`).

  Default features build the dx12, metal, gles, vulkan, wgsl and webgpu
  backends; trimming them shrinks both build and binary.
- **Rules.** CONTRIBUTING.md asks for a reason in the commit body for
  each new dependency. This one would be the largest in the workspace.
- **Verdict.** It has the widest coverage and one shader language, at
  the price of dependencies and moving the UI's presentation off
  SDL_Renderer.

### 4.6 Coverage

| Target | (a) SDL_Renderer | (b) GPURenderState | (c) SDL GPU | (d) wgpu |
|---|---|---|---|---|
| Windows 10/11 | yes (D3D11/12, GL) | D3D12 or Vulkan | D3D12 or Vulkan | D3D12, Vulkan, GL |
| macOS | yes (Metal) | Metal | Metal | Metal |
| Linux desktop | yes (GL, Vulkan) | Vulkan | Vulkan | Vulkan, GL |
| Older machines with GL 2-3 / D3D9-11 only | yes | no (CPU) | no (CPU) | GL backend needs GL 3.3 / GLES 3.0; else CPU |
| ARM handhelds, GLES 2 | yes (opengles2) | no | no | no |
| ARM handhelds, GLES 3 | yes | Vulkan if present | Vulkan if present and features suffice | GLES3 backend |
| WASM | yes (WebGL via opengles2) | no | no | WebGL2, WebGPU |
| SmileBASIC 4's sums, ±1 where the GPU rounds | yes | yes | yes | yes |
| Bit-exact blending | no | with ping-pong | yes (compute or ping-pong) | yes (ping-pong; compute on native) |
| 4x4 projection exact | no | yes (per-pixel inverse in shader) | yes | yes |
| Filters | no (CPU, the whole frame) | yes (fragment) | yes | yes |
| New dependencies | none | none (raw sys calls) | none | wgpu tree (61 crates on Linux) |
| Shader toolchain | none | offline, three formats | offline, three formats | none (WGSL) |

Whichever option is chosen, the CPU rasterizer remains the only path
on machines with no suitable GPU driver. That includes the low-end
hardware the project aims at (see the project's performance-first
goal), so CPU performance work keeps its value.

## 5. Exactness policy and testing

### 5.1 What can be pixel-identical

These can match the CPU exactly:

- **Texel values**: pages uploaded as integers (`R32Uint`) or read
  with `texelFetch`/`textureLoad` at integer coordinates. Never use
  filtered sampling for page reads.
- **Blending in integer arithmetic in a shader.** The shader reads the
  destination from the previous target (ping-pong) or works in a
  compute pass. It uses the same sums as `compose::Paint`, one
  rounding a draw, with saturation. WGSL, GLSL ES 3.0, HLSL SM5 and
  MSL all have 32-bit unsigned integer ops; the twice-tinted text sum
  (over 255⁵) needs 64 bits or two words.
- **Compose modes, FADE, mosaic, blur** (already integer and 8.8 fixed
  point), and the colour filter if it is moved to an integer or
  table-based formulation first.
- **Upright sprites and text at integer scales and integer offsets.**
  The sample point `(px + 0.5 - tx) / s` is exact in f32 for screen
  sizes up to 1280x720. This covers most games.

### 5.2 What will differ

- **Rotated, non-integer-scaled and projected edges and ties.** The
  CPU maps pixel centres through an f64 inverse matrix (and LMATRIX
  values are stored at f32, then used in f64). A shader works in f32,
  with a 24-bit mantissa. Pixels whose sample point lies within about
  1e-4 of a texel or image boundary can fall the other way. The tie rule
  (`Placement::texel_tie`) cannot be reproduced in f32 as written; it
  needs its own bias. What is not a power of two on SmileBASIC 4 keeps
  some noise of its GPU's in any case ([2.10](#210-proposed-ttest-batches)).
- **Raster filter coordinates and the colour filter's HSV**: these are
  f64 on the CPU today.
- **Fixed-function blending**, as SmileBASIC 4 uses (4.1): ±1 per
  channel where a value lies very near a half, between vendors and
  against the CPU.

- **Blur, accepted.** Done with no shader it rounds at each draw: LBLUR, three
  layers one over another, is up to 6 off and TEST54's parts up to 3. Nobody
  compares a blur by the pixel, so this stays and is not to be worked on.
- **Projected images, inside.** Without W a texel can be off inside a cell of the
  grid, by up to a quarter of a pixel as chosen; edges are not ([phase 3](#6-phased-plan)).

**Policy.**

- Exact paths must match the CPU bit for bit.
- Inexact paths may differ only at pixels next to an edge or tie: the
  differing pixels must lie within one pixel of an image boundary or a
  texel boundary under the item's matrix.
- Each inexact path is listed in the test suite with its tolerance.

There are two ways to reduce the inexact set later:

- Compute sample coordinates in fixed point (for example 16.16
  integers) in both backends for affine items. This changes the CPU
  reference too, so it needs a hardware check (batch C) first.
- Keep f64-like accuracy where it matters by working in coordinates
  relative to the item's origin.

### 5.3 Comparison tests

- **Golden scenes.** Done, in `crates/sb-sdl/src/gpu_golden.rs`: small programs,
  each drawn by the GPU path and by the CPU at once, which is the reference, so
  no frames are kept. Each blend and compose mode, tint, flips and turns, whole and
  other scales, XSCREEN sample 2, LCLIP, 4x4 layers with text, each filter, FADE,
  repeating pages, direct mode's hint, list, cursor and selection, and sub screens.
  Each has a limit on the pixels more than 2 off. `SB_GPU_GOLDEN=1` prints a row for
  each and exits, 1 if one is over; `cargo test -p sb-sdl --test gpu_golden --
  --ignored` does it with no window. It is ignored as CI has no GPU.
- **Start-up.** The first of those scenes runs when the path starts, and the path
  stays off if over 0.5% of its pixels are more than 2 off, or 0.1% have an alpha
  off. Rounding is allowed for: on opengl, opengles2 and vulkan the scene was 1 off
  at the most, 1 to 1.7% of its pixels, and none more. It took 68 to 76 ms on opengl,
  94 on opengles2 and 133 on vulkan, on this desktop, once in a run. `SB_GPU_TEST=off`
  skips it and `fail` tries the way out.
- **Projects.** The `--bench` projects of [1.7](#17-measured-per-frame-costs)
  are rendered for N frames on both backends, and the frame diffs are
  summarised, in a manual or nightly job. They are not in the
  repository, since the projects are not ours to commit.
- **CI.**
  - Linux: Mesa's lavapipe (software Vulkan) for wgpu or SDL GPU, and
    llvmpipe for wgpu's GL backend. Lavapipe ran the wgpu probe here.
    Check SDL GPU's Vulkan feature list against lavapipe once.
  - Windows: WARP (software D3D12).
  - macOS: runners have Metal.
  - Software rasterizers also differ in f32 details, so the inexact
    tolerance applies there too.
- **Hardware stays the arbiter.** A GPU/CPU disagreement on an exact
  path is a bug in the GPU backend. A disagreement on an inexact path
  is settled, if it matters, by a screenshot from SmileBASIC 4 (batch
  C), and the CPU reference is changed only then.

## 6. Phased plan

**Phase 0: verify on hardware.**

- Write and run batches A-G ([2.10](#210-proposed-ttest-batches)).
  Record results in [sb4-pitfalls.md](sb4-pitfalls.md), fix the CPU
  renderer where it disagrees, and drop the six stale markers and
  every UNVERIFIED that a batch settles.
- Add golden-scene tests for each confirmed rule
  ([5.3](#53-comparison-tests)).
- Milestone: no UNVERIFIED left in the rendering files, or the
  remaining ones listed here as accepted unknowns; the golden suite in
  CI.

**Phase 1: scene builder, CPU backend. Done.**

- `Scene`, `LayerPass`, `SpriteItem`, `TextItem` and the builder are in
  `scene.rs`, `Rasterizer` and `CpuRasterizer` in `raster/`.
  `Runtime::render` is the gate, `Scene::build`, `Rasterizer::draw`,
  `draw_overlay`. The scene comparison is left to phase 2.
- The hint and completion list stayed on the CPU frame, after the rasterizer, and
  dimming cannot move ([3.6](#36-what-stays-on-the-cpu)).
- Measured against the commit before, on one x86-64 desktop, release build,
  `--bench --fixed`: checksums identical on TPERF whole (10500 frames), its
  scenes TMULTI, SPROT, SPANIM, LCOMP, GAME, SPHIT, TSCROLL, VINT (2200) and
  FINITI, ELE2020, RYUSI, BASICDEMO, NEON_DRIFTWAY, SOLIDGUNNER_V, DVD_BOUNCE
  (1200); `--screen` and `--play` PNGs byte-identical on the hardware test
  programs that have run files, including sub screens.
- Instructions, against 0.07% between two runs of one build: TPERF -2.1%,
  TMULTI -5.7%, SPROT -2.7%, SPANIM +0.4%, LCOMP -0.3%, GAME -1.6%, FINITI
  -2.7%, ELE2020 -2.4%, NEON_DRIFTWAY 0.0%, DVD_BOUNCE +0.2%. Part of the falls
  may be inlining rather than work saved, so do not count on them.
- The largest scene, 4096 sprites and five screens of 14400 empty cells, builds
  in 0.15 to 0.18 ms and holds 429 KiB, of which 88 bytes a sprite. It allocates
  nothing once grown, as a test checks; peak memory is unchanged. On a run
  of SPROT, `Scene::build` is below 0.2% of the samples.
- The golden scenes of [5.3](#53-comparison-tests) are still to come; the
  comparisons above are not in the repository, as the projects are not ours.

**Phase 2: change tracking. Tiles done, the rest not.**

- Done: `graphics::Dirty`, a bit for each 64-pixel tile of a page. Pages are
  private to `graphics.rs`, and `page_pixels` asks for the area to be written; a
  test compares the marks with a diff of the page for each way of writing. The GPU
  path sends only marked tiles that a shown element reads, the rest when one does.
- Still to do: the scene-hash reuse and raster array snapshots. The marks have one
  reader, so the debugger's GRP view would need a history of its own.
- Milestone: frames that only repeat state are skipped (measure on a
  program that sets the same `SPOFS` every frame).

**Phase 3: a GPU path. Done, chosen by a setting, see phase 5.**

(b) sitting on (a) was chosen, as the UI stays on SDL_Renderer. `gpu.rs` in
`crates/sb-sdl` is a `SceneBackend`, which the runtime offers each scene; the
source says how to run it. It draws sprites and text screens under any matrix,
4x4 too, with tint, both blends, compose modes 0-4, LCLIP and XSCREEN's sample,
through `SDL_RenderGeometry`, with no shader on any driver, and mosaic, blur and
raster the same way; also FADE and the darkening beside a subprogram, sub screens,
the line cursor, inverted cells and direct mode's hint and list. The color filter
has a shader on the `gpu` driver, and elsewhere its layer comes from the CPU. A frame
is left to the CPU whole, with a count saying why, only for: an image off a page that
does not repeat, a raster filter that takes over 20000 quads, inverted cells of a
font type over 16, a full atlas of fallback glyphs, and a texture or blend refused.
None of the scenes below does any of those.

Times, on one x86-64 desktop with an RTX 3060 Ti, on the opengl driver through
EGL with no window shown: ms for each frame drawn. GPU is the CPU's side (walking
the scene, pages, flush) plus the wait for the GPU when read back at once, with
no vsync and no real present. A window's present is not in these. TPERF's scenes
and ELE2020, instructions in billions over the same run:

| Scene | CPU ms | GPU ms | CPU instr | GPU instr | Peak RSS, CPU, GPU | Pixels 1 off, 2, more, of those compared | Most off |
|---|---|---|---|---|---|---|---|
| TMULTI | 72.8 | 3.3 | 153 | 14 | 313, 362 | 0, 0, 0.15% | 220 |
| SPROT | 29.1 | 0.8 | 80 | 6 | 296, 344 | 0.01%, 0.01%, 0.08% | 255 |
| LCOMP | 37.4 | 0.9 | 112 | 3 | 311, 370 | 44%, 0, 0 | 2 |
| LBLUR | 13.2 | 1.7 | 80 | 4 | 313, 374 | 42%, 20%, 2.0% | 6 |
| LMOSAIC | 6.6 | 0.9 | 33 | 3 | 311, 374 | 1.6%, 0.04%, 0 | 3 |
| LCOLOR | 10.0 | 10.2 | 49 | 52 | 311, 380 | 0, 0, 0 | 0 |
| LMATRIX | 15.4 | 1.1 | 75 | 4 | 310, 367 | 0, 0, 0.17% | 255 |
| LRASTER | 8.8 | 1.1 | 48 | 5 | 311, 368 | 0, 0, 0 | 0 |
| GAME | 2.6 | 0.8 | 22 | 11 | 297, 346 | 1.2%, 0.07%, 0.001% | 3 |
| NEON_DRIFTWAY | 65 | 3.1 | 113 | 26 | 347, 408 | 3.4%, 0.26%, 0.03% | 255 |
| FINITI skybox | 6.8 | 4.2 | 62 | 44 | 359, 409 | 0.67%, 0.32%, 0.60% | 255 |
| SOLIDGUNNER_V | 2.3 | 1.4 | 24 | 9 | 276, 329 | 0.01%, 0, 0.02% | 255 |
| ELE2020 | 13.2 | 2.1 | 143 | 11 | 313, 368 | 5.4%, 0, 0.003% | 255 |

Elements' edges and samples between texels are the 255s, as before. A plain sub
screen and the inverted cells of direct mode cost about 1 ms either way. In a real
window the user measured TMULTI at 2.47 ms for the screen, and SPROT and TMULTI look
as on the CPU; the rest of this has not been in a window yet.

With the switch off, on phase 1's bench runs, TEST48 to 56's `--screen` and `--play`
PNGs included: checksums and PNGs identical, instructions within 0.02% but for TMULTI,
+0.84%, and TPERF, +0.17%, the text code having been split to give a backend its
inverted glyphs.

The rest:

- **Exactness.** The ±1 are the GPU's rounding of one draw's sum, and add up
  over layers: about one pixel in six a layer in LCOMP. Everything larger seen
  lies on elements' edges and on lines where samples fall between texels, as
  the policy in [5.2](#52-what-will-differ) allows; upright sprites and text
  at whole scales and places, and the GRP scenes, are identical. With the same
  frames compared with screenshots of SmileBASIC 4 on an emulator on a PC's GPU,
  pixels differing, CPU and then GPU: TEST49 parts 1 to 5, all by 1 or 2, 73728 and 59392; 83968 and
  74752; 131072 and 130048; 211968 and 206848; 146432 and 134144. TEST50 parts
  1, 2, 4, the ties, 4336 and 4415; 1209 and 1245; 3441 and 3441. On TEST49
  the GPU is the closer, as it should be: SmileBASIC 4 rounds the same way.
- **Ties.** A sample moved by 1/128 of a texel the way `Placement::texel_tie`
  says, and an element that can tie moved 1/256 of a pixel down, follow the rule
  for edges and texels (TEST50 above; 183 pixels of 921600 differ from the CPU in
  part 1, none in parts 3 and 5). The golden scene `ties` of sprites and cells, on half
  pixels and at 1.5, flipped and turned, has none off. Left to the host's own rasteriser, with
  no bias, that scene has 24704 of 384000 pixels off: the host's arithmetic goes its
  own way, whatever the order of the vertices, so the bias stays. A bias follows the rule, not
  the noise a real Switch has besides.
- **Drivers.** SPROT, LCOMP and TMULTI differ from the CPU alike on opengl,
  opengles2 and SDL's gpu driver on the NVIDIA GPU, on SDL's vulkan driver, on the
  gpu driver on lavapipe and on opengles2 on llvmpipe. SDL's vulkan driver costs
  far more on the CPU (TMULTI 29 ms), so prefer another. Only the gpu driver takes
  shaders.
- **Raster filter.** A run of rows or columns with one entry is a quad read nearest,
  split where the source point crosses the frame's edge or wraps; a sample is moved
  1/512 of a texel to round down as the CPU does. A unit test checks each pixel takes
  the CPU's texel. TEST54 parts 4 to 6 and LRASTER: no pixel differs.
- **4x4.** A grid of triangles (`gpu_project.rs`), whose corners go exactly where the
  matrix puts them, so straight edges stay straight and in place, and only a texel
  inside a cell is off. W above 0 and Z / W from -1 to 1 are straight lines on the
  element, so they and the frame's edges cut it first; a unit test compares what is
  covered with the CPU's. The grid doubles until a run's middle is within a tolerance
  where the element is nearest. FINITI's skybox, six sprites 1024 across, by tolerance
  in pixels, against vertices a frame, pixels more than 2 off and GPU ms: 2, 3800,
  5.0%, 3.4; 0.5, 16700, 1.25%, 3.7; **0.25, 33700, 0.59%, 4.3**; 0.1, 87500, 0.24%,
  5.9. The differences lie scattered on texels' edges, with no line and no grid to be
  seen in a picture of them. A quarter of a pixel was chosen. NEON_DRIFTWAY's elements
  are small, at about 200 vertices, and do not change with it. No shader was needed.
- **Sub screens.** A subprogram has screens and pages of its own, so it gets a second
  path on the same renderer. A clear back keeps its alpha: no pixel's alpha was off.
  Its frame goes to the window smoothed and premultiplied, in every XSCREEN mode.
- **Color filter.** HSV is more than blending can do. Off the `gpu` driver, that
  layer alone is drawn and filtered on the CPU and sent, and a direct one, which
  filters what is behind it, starts the frame with all those passes from the CPU.
  Nothing is read back. LCOLOR's four whole-frame layers cost as much as the CPU
  does, 14.7 MB sent a frame, with no pixel differing; other layers stay on the GPU.
  On the `gpu` driver the shader takes 1.4 ms there, so it stays.
- **Shaders.** One filter, the color filter, in `crates/sb-sdl/shaders`: GLSL,
  SPIR-V from glslang committed beside it. On the gpu driver, it matched the CPU on
  all 39.6 million pixels compared of LCOLOR and on TEST54 part 7, and within 2 on
  TEST53 part 4, where the layer's color follows; the GPU's wait was 1.3 ms a frame
  with four such layers. Windows' D3D12 needs DXIL and macOS MSL too, which SDL_shadercross makes
  from the same source; neither was tried. So far only Vulkan.
- **Mosaic and blur.** Fixed function, on two half-size targets. A mosaic is a
  smoothed draw into part of one and a nearest draw back. A blur's taps cannot
  simply add: each draw rounds to 8 bits, and 9 draws came up to 6 off in a
  simulation of frames. So each mixes into what is there by its share of the taps so
  far, the outermost first, and two neighbours are one draw by smoothing between
  them; a run of one color stays exact, and the fall in the weights comes last, as
  black over all of it, which takes what shows through down with the light. On
  an opaque frame a tap takes one draw. A layer's alpha decides how an ordinary mix
  would go, so there a tap takes two, down and then add, and rounds twice. TEST54
  parts 2 and 3, against the CPU: about 1% 1 off, under 0.5% 2 off, 3 at
  most. LBLUR, three blurred layers one over another: 6 at most, 1.7% more than 2
  off, since the differences add up over the layers. No shader was needed.
- **Uploads.** Pages reach the GPU by 64-pixel tiles, those marked that a shown
  element reads: 0.5 MB a frame in GFILL and GPUTCHR, which were 16.8. GCLS fills
  the whole page, so SOLIDGUNNER_V, which does it each frame, sends only what its
  sprite shows. With the marks, GFILL, GPUTCHR and GAME are no slower than the CPU.
  Nothing is read back: GRP commands read the CPU's pages, which stay authoritative.
  With the path off, the marks cost at most 0.7% in instructions on GRP scenes;
  GLINE, GCIRCLE and GPUTCHR got cheaper, as they now do less per pixel.
- **Memory.** Peak RSS is up by 30 to 60 MiB (297 to 328 in SPROT), with one page and
  the font page on the GPU, and 312 to 346 in TMULTI. That was 1006, on opengl and
  opengles2, with 224000 vertices in one draw; 700 MiB of it went when those were
  handed over in draws of 60000 or fewer, so a driver's keeping what it took for a
  large draw is the likely cause, and the path now flushes every 30000. On opengles2 with
  Mesa's llvmpipe, one draw peaked at 388. Caching a text screen in a target of its own would
  not help TMULTI, whose cells change each frame, and SDL expands indexed geometry
  into plain vertices in its GL drivers, so indices would not either. On the GPU: 16
  MiB a page in use, at most six, 16 for the atlas if a glyph needs it, three
  frame-sized targets and two of half that. No frame of latency is added: the frame
  is drawn in the same iteration as before.
- **Failure.** See phase 5.
- **Reach.** Windows, macOS, Linux: (a) on their usual drivers, untested on the first
  two; (b)'s filters on D3D12, Metal and Vulkan once SmileBASIC 4 are made. GLES
  handhelds, the RK3326 among them: (a) needs only GLES2's textures, targets and
  blends, so the path should work, the color filter's layer coming from the CPU.
  Tested on GLES2 on a desktop GPU only: on Mali, timings, target switches on a
  tiler, and uploads are unknown. Mesa's PanVK names the Mali-G31, and has a switch
  for hardware it does not call supported; whether the G31 needs it, and what an
  AmberELEC build has, are unknown.

**Phase 4: full feature set. Done, in phase 3's path.**

No project in the bench set leaves a frame on the CPU; see above for what could.

**Phase 5: default and fallback.**

- The start-up self-test is done ([5.3](#53-comparison-tests)). It takes 30 to 100 ms
  on a desktop GPU, about 100 on llvmpipe. Besides what it covered it now samples
  one-texel checkers at the page's far corner, where a fragment shader holding texture
  coordinates in 16 bits, as GLES2's mediump can, goes wrong while the page's start
  looks right: emulated by a GL shim, that fails it, and it passed before.
- It passes on rounding however a driver rounds, and fails on a wrong sum. What counts
  is pixels more than 4 off, in any channel, alpha too, on at most 0.5% of the frame:
  an element goes through three draws there, each of which may lose 1. llvmpipe rounds
  otherwise than an NVIDIA GPU, with 16% of SPROT's pixels 1 off where that has 0.01%,
  and passes. A shim truncating every fragment's output comes to 3 off at most, 704
  pixels of 384000 over 2, none over 4, and passes. A shim breaking the blend function
  fails, as do 16-bit coordinates, at 2.5% of the pixels.
- The setting (File > Settings > Screen): Auto, or CPU. `SB_GPU` overrides it, and
  About and the bench report say who draws and, if the CPU, why. `settings.txt` keeps
  only a choice that differs from the default, so the default can change for those who
  made none. `smilebasic-check` has no backend and stays on the CPU.
- Failure, all tried with no window, through `SB_UI_BENCH` and a GL shim that lies:

  | What goes wrong | What happens |
  |---|---|
  | No EGL or GL, the driver left to SDL | SDL takes Vulkan, or the software renderer, where the path stays off |
  | `SDL_RENDER_DRIVER` naming a driver that cannot start | Another is tried in SDL's order; it was a panic in the sdl3 crate |
  | SDL's gpu driver with no Vulkan | As above; with `SB_GPU_ALONE`, the CPU |
  | Largest texture under 2048, as 1024 or 2047 | The self-test sees the frame stayed on the CPU, and the path stays off; under the frame's own 1280 the emulator cannot start on the CPU either, with a message, as before |
  | A texture refused, at start-up or later, on opengl | Seen through glGetError, which SDL's driver looks at only in a debug context; three frames refused in a row and the path stops for the session |
  | A texture refused on opengles2 | Asked of GL first and refused as above, as SDL 3.4.18 frees its texture data twice in `GLES2_CreateTexture` on GL_OUT_OF_MEMORY and aborts, where the driver looks at errors. The frontend's own textures can still meet that: to report upstream |
  | A target made in part, for a frame or half of one | All of it let go; drawn on as it was, the next frame would have gone to the window |
  | `RENDER_TARGETS_RESET` | The frames are drawn again, a subprogram's too |
  | `RENDER_DEVICE_RESET` | Every texture is made again, and the path started again with its self-test: Direct3D 11, 12 and Vulkan let go of them all, and the window's own would have ended the emulator at its next upload |

  Direct3D 11, 12 and Vulkan's `RENDER_DEVICE_LOST`, when a device does not come back,
  is not handled, by the path or the rest of the frontend: uploads then fail, and the
  emulator ends with a message. A GL context lost without SDL saying so, as some drivers
  do on resuming from suspend, leaves pages as they are; only by the code, not tried.
- Long runs: 12000 frames each on opengl, opengles2 and llvmpipe, 10 sizes of `XSCREEN`
  from 128x128 to 1280x720 in modes 0 to 2, page changes, `EXEC`, a subprogram started
  and stopped, resets, resizes, full screen and the setting off and on every 400 frames.
  No pixel more than 2 off on the main screens in 228 frames compared, at most 7 of 6.3
  million in the subprogram's. Live GL textures back to 0 at the end. With no events
  RSS holds at 397 MiB from the third sample to the last. Each restart of the path adds
  4 to 10 MiB of RSS, which on these drivers does not come back; a restart is rare.
- In the frontend, by pictures of the window with and without the path, no pixel
  differing: modes 0, 1, 2 in a window of 1280x720, resized and in full screen, menu,
  About, the colors window, a notice, help, debugger's panel, editor inline and apart,
  a stopped program, direct mode's hint, an error, an ended program with its targets
  spoiled by hand, device reset, the setting turned on and off.
- Other drivers, by reading SDL 3.4.18. Every driver takes all our blend modes, as their
  factors are plain and the operation is the same for color and alpha. D3D11, D3D12,
  Metal, GL and GLES2 keep a vertex's color as floats. Direct3D 9 packs it to 8 bits,
  truncating, so a tint there may be 1 off, which the self-test allows, and is reached
  only if D3D11 and D3D12 cannot start. Pages are a power of two, so wrapping needs
  nothing of GLES2. D3D11 makes a staging texture for each `SDL_UpdateTexture`, so a
  program writing to many tiles a frame costs more there: the report's `pages` will
  show. Each driver's arithmetic is the self-test's to judge, as it reads back from a
  target on all of them.
- `SDL_GPURenderState`'s shader is SPIR-V only. On the `gpu` driver over D3D12 or Metal
  `ColorFilter::new` finds no such format, and the layer comes from the CPU, as on every
  other driver; the `gpu` driver is reached only when all before it fail. That is fine.
- Windows: `cargo build --release --target x86_64-pc-windows-gnu -p sb-sdl` builds with
  MinGW, D3D9, D3D11, D3D12, GL, GLES2 and Vulkan in it; not run. macOS needs its SDK.
- Still to do: runs on real windows, Windows, macOS and a Mali, and then the default.
- Later: a WASM build with the CPU backend first, then WebGL2/WebGPU
  if (d) was chosen.
