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
| Filters 0-7 | `filter::Filter` | `LFILTER`: mosaic, blur, raster (holds the program's array and reads it every frame), color. |
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
2. **Element list.** Visible text screens and `Sprites::draw_order()`
   (shown sprites with their linked poses from `Sprites::pose`) go into
   one `Vec<(layer, z, kind, id, Element)>`. It is sorted by layer
   (7 first), then Z (larger first), then kind (text screens before
   sprites), then id (higher first). The list is allocated anew every
   frame.
3. **Background.** `fb.clear(BACKCOLOR | opaque)`. A sub interpreter's
   frame (`env_type != 0`) is premultiplied, its alpha the coverage
   (255 less SmileBASIC 4's transmittance), so its clear BACKCOLOR
   lets the main screen show through; an opaque one makes the whole
   frame opaque at the end.
4. **Layers 7 to 0.** For each layer with elements, or with a filter
   on a direct (mode 0) layer:
   - `Runtime::fine` scales the 2D matrix and LCLIP by the sample
     magnification.
   - The target is the frame itself for mode 0 (`Compose::Direct`), or
     `scratch` (cleared to 0, premultiplied) for modes 1-4.
   - Without a 4x4 matrix, each element is drawn through
     `Runtime::draw_element`. Text screens go through
     `TextScreen::draw_placed`, and sprites through
     `sprites::draw_posed`.
   - With a 4x4 matrix, sprites are drawn straight through it with
     `compose::project_image`. Runs of text screens are first drawn flat
     into `flat`, sized by `Runtime::flat_area` and
     `compose::source_bounds`, then put on with `compose::project`.
     That is why text screens reach at most one screen past each edge
     (see [differences.md](differences.md)).
   - `Filter::apply` (`filter.rs`) runs on the target. A raster filter
     on a direct layer then makes the frame opaque again.
   - For modes 1-4, `compose::compose` puts `scratch` over the frame,
     multiplied by the layer color.
5. **FADE.** `Fade::cover` mixes the fade color over every pixel, and
   darkens the frame to 224/255 in the same rounding while the other
   side has the focus beside a subprogram (`dimmed`).
6. **Emulator UI on the frame.** In direct mode, the command hint and
   the completion list (`hint::draw_band`, `Completion::draw`) are
   drawn after the fade, so they are not darkened.
   These are not SmileBASIC 4 rendering. A GPU split should keep them
   as a CPU overlay ([3.6](#36-what-stays-on-the-cpu)).

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

**Sprites** (`sprites::draw_posed`):

- A page that is still blank shows nothing.
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

**Text screens** (`TextScreen::draw_placed`):

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
    coordinates. The array is read every frame;
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
  - the editor's frame and the sub screen in mode 1 still use
    `SDL_SCALEMODE_PIXELART`. The Rust bindings lack a per-texture call for
    it, hence `SDL_SetDefaultTextureScaleMode`;
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
| Sprite colour (SPCOLOR) multiplies each channel, alpha included, unrounded, inside the one rounding of the draw | `graphics::Tint`, `compose::Over3` | C (R2): TEST49. A child's link colour (`multiply`, rounded down) is still U |
| Text colour (cell colour, then TCOLOR) the same, unrounded | `compose::Mixed`, `Over5` | C (R2): TEST49. |
| An inverted cell, attribute bit 7: the texel is taken as `n,n,n,n` for `n = 1 - alpha` and goes through the same sum; any character, an empty cell too | `inverted_texel`, `attr::INVERT` | U: notes; TEST56 part 1. Not the INPUT cursor (TEST50) |
| TLOAD, TUPDATE and TCOPY keep all 16 attribute bits; TSAVE, TARRAY and CHKCHR read them | `Cell` | U: executable; TEST56 parts 1 and 4 |
| Tint is applied to the page pixel before blending | `sprites::draw_posed` | C (R2): TEST49 |
| Relative colour animation `"C+"`: product / 256, rounded down | `anim.rs` | C: pitfalls |
| LAYER mode 0 draws straight onto the frame, layer colour ignored | `Runtime::render` | C: pitfalls; TTEST16 |
| Modes 1-4: the layer colour scales light (`P = p*c*ca`) and coverage (`A = la*ca`), unrounded | `compose::compose_tinted` | C (R3): TEST49, colours `C0806040`, `7F40C0FF`, `FFC08040`, `80FFFFFF`, white |
| Mode 1: `P + d*(1-A)`; mode 2: `d + P`; mode 3: `d*(P + 1-A)`; mode 4: `d + P*(1-d)`, one rounding each; mode 1 adds to the frame's coverage, mode 2 keeps it, modes 3 and 4 make it opaque | `compose::compose` | C (R3): TEST49. U: that modes 3 and 4 make the whole screen opaque, not only where the layer has something (shows on a subprogram's screen only) |
| A sub interpreter's frame is premultiplied over the main one (dimmed to 224/255), cleared to BACKCOLOR's colour unmultiplied with coverage `255 - alpha` (alpha 0: clear); an opaque BACKCOLOR replaces the main screen | `Runtime::render`, `Fade::cover` | C (R4): TEST49 for alphas 0, 1, &H7F, &H80, &HFE; the opaque case is from SmileBASIC 4 only |
| Beside a subprogram, the side without the focus: times 224/255, rounded in the same step as FADE | `Fade::cover`, `screen::darken` | C: TEST49 |

### 2.2 Sprites

| Rule | Where | Status |
|---|---|---|
| Placement: home to position, scale, then clockwise rotation in degrees | `Affine::place` | C: manual (SPHOME, SPSCALE, SPROT) |
| Flip first, then quarter turn | `Source::page_xy` | C: pitfalls |
| Sampled without smoothing, at frame pixel centres; away from ties every rotation and scale fits with no misses | `draw_image`, `draw_upright` | C (R5): TEST50 generic angles |
| With XSCREEN sample > 1, sprites are sampled at the finer pixels' centres; LCLIP grows to whole fine pixels | `Runtime::fine` | C (R6): TEST50 part 2 (LCLIP exact; the fine grid misses only 56 tie pixels of 54298) |
| Coverage: a pixel is drawn when its centre, nudged right then up, lies on the image, at any rotation: an edge through the centre takes it if it is the left or bottom edge on the frame | `Placement::sampling` | C (R5): TEST50, no coverage miss in any part, text included (consistent with a rasteriser's top-left rule on a target drawn flipped) |
| A sample point exactly between texels, at a multiple of 90 degrees: image side a power of two and scale a power of two: the higher texel; power of two with another scale: towards the frame's left and bottom; another side: towards the page's lower coordinates | `Placement::texel_tie` | U (R5, R8): the usual outcome only; see below |
| The same at other angles (only 45-degree multiples can tie): image x higher, image y lower; kept on the image | `Placement::texel_tie` | U (R5): TEST50, no misses but 21 pixels reading the row above the image |
| SPCHR W and H are the size as shown (turned size rule) | `sp.rs` | C: pitfalls; TEST45, TEST46 |
| GRP-1 is white everywhere; GSAMPLE 1 repeats the page, 0 is transparent off it | `sprites::sample`, `Source` | C: manual (SPPAGE, GSAMPLE) |
| A linked child's offset turns and scales with the parent; the flags decide the child's own image | `Sprites::pose` | C: pitfalls (the `pose` UNVERIFIED comment is partly stale) |
| Children without link bit 0 stay where they showed, Z included, when the parent is cleared | `Sprites::clear` | S: pitfalls ("Z included"); marker at `sprites.rs` "that Z also becomes absolute" |
| Animated integer properties take the value rounded towards zero; a sliding XY too; colours mixed in single precision by the frames left, rounded down | `WithDefs::set_value`, `anim::mix` | C: pitfalls; TEST47 (R7) |
| Rotation and scale kept in single precision, slid as `from*left + to*(1-left)` in single precision and not rounded | `sprites::single`, `anim::mix` | C: pitfalls; TEST48. U: that XY, Z, UV, V and I mix in single precision too (only visible where a slide lands on a whole number) |
| An SPANIM set in the frame of the sprite's SPSET steps first a frame late | `Sprites::animate` (`fresh`) | **U**: TEST48 fits it, but also a VSYNC that did not wait before SPANIM, or the SPCLR before SPSET |
| SPDEF's default table when none was imported (grid of 16x16) | `grid_defs` | Stand-in, not a rule. The real table is imported |

### 2.3 Text screens

| Rule | Where | Status |
|---|---|---|
| Flip then turn for ATTR | `draw_placed` | C: pitfalls |
| Ties: as for sprites of the cell's size, per cell (a block of cells shows what a sprite of the same size does) | `draw_turned`, `draw_upright` | C (R8) that text and sprites go alike: TEST50, parts 3 and 4; U for the rule itself, as for sprites |
| Font types 24-64 show only user-defined characters; GPUTCHR with them reads GRP5 in 24-pixel cells | `glyph_pixel_px` | C: pitfalls |
| Tab stops every 4 columns | `TAB_WIDTH` | S: pitfalls; marker in `text.rs` |
| `CHR$(10)` starts a new line | `TextScreen::print` | S: pitfalls; marker in `text.rs` |
| TSCREEN clears the screen | `set_font` | S: pitfalls; marker in `text.rs` |
| TSCREEN keeps TLAYER, TPAGE, TVAR and TFUNC, and sets TBLEND back to 0 | `restart_look` | C: pitfalls; TEST47, TEST48 |
| INPUT cursor: a bar 2 by 8 at the cell's left, opaque white whatever the colour; it alternates with nothing | `draw_placed` | C (R9): TEST50, parts 6 and 7 |
| The cursor's blink: 20 frames on, 20 off, counted in frames played; on from the start of a line and from each key that moves or edits, the first period a frame short | `LINE_CURSOR_BLINK`, `LineEdit::key` | U: executable (the count steps with `MAINCNT`'s); TEST50's by-eye "about 2 times a second" allows it; TEST56 part 1 |
| The cursor's width is the cell's shifted right by two, or the whole cell after Insert; its height the cell's; with TSCALE and LMATRIX; right after its screen's cells, under what is in front | `LineCursor`, `draw_placed` | U: executable, the shared matrix from SmileBASIC 4; TEST56 part 1 |
| On a composed layer the cursor has no coverage, so it adds | `LineCursor::composed` | U: from the blend state; TEST56 part 1 |
| A selection on a typed line is drawn inverted, in the console's colour, and stays after Enter | `LineEdit::draw`, `finish` | U: executable; TEST56 parts 1 and 4 |
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
| Sprites and text screens on a 4x4 layer show whole, also off-screen parts | `project_image`, `flat_area` | C: pitfalls; TTEST36 (text bounded: [differences.md](differences.md)) |
| 4x4: a sprite's Z is not used; shown where W > 0 and Z/W is from -1 to 1 | `compose::project`, `project_image` | C (R12): pitfalls; TEST53 part 2 |
| 4x4: sampled without smoothing | `compose::project` | C (R12): pitfalls; TEST53 part 2 |
| LCLIP on a 4x4 layer is in frame pixels, after projection, inclusive, Y from the top | `project(..., clip)` | C (R13): pitfalls; TEST53 part 3 |
| Errors for a bad 16-element array | `screen.rs` | U, not rendering (skip) |

### 2.6 Filters

| Rule | Where | Status |
|---|---|---|
| Mode-0 layer: LFILTER filters everything drawn so far, BACKCOLOR included; composed layer: only its own | `Runtime::render` | C: pitfalls; TTEST22/23 |
| The same for a mode-0 layer with no elements | `Runtime::render` | S: pitfalls ("That holds for a layer with nothing in it, too"); marker in `runtime.rs` |
| Mosaic: the whole frame drawn into `ceil(width/n)*sample` x `ceil(height/n)*sample` half-buffer pixels with bilinear sampling, then back with nearest; blocks are n frame pixels at any sample | `filter::mosaic` | C (R14): pitfalls; TEST54 part 1, no pixel more than 1 off |
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
| A clear pixel is left alone in mode 1 only; in modes 0, 2, 3, 4 a raised value lights it | `filter::color` | C (R19): TEST53 part 4, layer 5 |
| ACLS clears LFILTER | `Runtime::acls` | C: pitfalls; TEST47 (R20) |

### 2.7 FADE

| Rule | Where | Status |
|---|---|---|
| Fade steps: single-precision mix, rounded down; a clear end takes the other end's colour | `Fade::step`, `Fade::start` | C: pitfalls |
| ACLS clears FADE | `acls` | S: pitfalls; marker in `runtime.rs` |
| Fade cover: `(c*a + p*(255-a) + 127) / 255` (nearest), over the final frame, its alpha kept | `Fade::cover` | C (R21): TEST49, four fade colours (alpha &H80, &H7F, &H01, &HFE) |
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
| GLINE from an end off the page: `k = major`, counted from that end (only the steps on the page are walked) | `line_points` | C: pitfalls; TEST47, TEST48 (`lines_from_off_the_page_match_hardware`). U: that the page counts, not GCLIP, and an end off the page across the walk |
| GBOX outline: top and bottom rows, then the sides from the top row down, so the top corners are drawn twice | `Graphics::rect` | C: pitfalls; TEST47 (R24), TEST48 (any corner order) |
| GFILL inclusive | `Graphics::fill` | C: manual |
| GCIRCLE pixel set (midpoint circle) | `circle_points` | C: pitfalls; TEST47 (R25) |
| GCIRCLE arc: between the pixels at `round(cos*r), round(sin*r)` of the two angles, both included | `Graphics::circle` | C: pitfalls; TEST47 (R25), TEST48 (an arc of more than half a turn whose two end pixels are one is the circle) |
| GCIRCLE: equal angles draw nothing; the sector flag draws only the outline | `Graphics::circle` | C: pitfalls |
| Sector edges: GLINEs from the center to those pixels | `Graphics::circle` | C: pitfalls; TEST47 (R25) |
| Equal angles with the sector flag draw nothing; radii of 1000 and 70000 draw | `Graphics::circle` | C: pitfalls; TEST47 (R25) |
| GTRI: rows from the leftmost to the rightmost pixel the edges reach (`edge_span`), each pixel once, a degenerate triangle too; tall edges walked down with `k = major`, wide ones up with `k = (major - 2*minor) mod (2*major)` | `Graphics::triangle` | C: pitfalls; TEST47 (R26), TEST48 (`triangle_rows_match_hardware`). U: corners off the page |
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
| GSAVE/GLOAD width and height: any, 0 included, up to what memory holds | `grp.rs` (`MOST_PIXELS`) | C: pitfalls; TEST48. U: negative sizes (Out of range here), and the error past memory (Out of range here, see differences) |
| TSAVE width and height limit | `txt.rs` | U (R31): taken as 4096 |
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

#### What TEST50 says about ties

Every page pixel in TEST50 has its own colour, so each screenshot pixel
names the texel sampled. Away from ties all rules agree. At ties the
screenshots show the GPU's arithmetic: in one quad the texel changes
along the quad's diagonal, between its two triangles, and the same image at the same scale goes different ways at
different places. Simulating the quad in single precision (vertices
through the matrix and viewport, attributes by plane equations and by
barycentrics, snapped to 1/256 or not) did not reproduce which way each
tie goes, so what remains seems to be the host GPU's, and a real Switch
may differ. The rule in [2.2](#22-sprites) takes the usual outcome:

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

A GPU backend ([4](#4-gpu-backend-options)) gets these ties from its
own arithmetic and will differ from the CPU rasterizer on them.

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
- No per-frame allocation in the steady state: the scene's vectors are
  kept and cleared, as `scratch` and `flat` are today.

### 3.2 The scene (display list)

The scene is built each frame that the redraw gate lets through:

```rust
pub struct Scene {
    pub frame: (u32, u32),          // ScreenMode::frame()
    pub sample: u32,
    pub back: Background,           // Opaque(u32) | Premultiplied(u32) for a sub interpreter
    pub passes: Vec<LayerPass>,     // layer 7 first; empty layers omitted
    pub items: Vec<Item>,           // all passes' items, back to front
    pub fade: u32,                  // 0 alpha: none
    pub pages: [PageState; 6],      // change counts / dirty tiles, see 3.4
}

pub struct LayerPass {
    pub compose: Compose,           // Direct draws onto the frame
    pub color: u32,                 // layer colour (ignored for Direct)
    pub clip: Option<Clip>,         // in frame pixels (already "fine")
    pub projection: Option<[f32; 16]>,
    pub filter: FilterOp,           // Mosaic(n) | Blur(n) | Raster(rows snapshot) | Color(h,s,v)
    pub items: Range<u32>,
}

pub struct Item {
    pub source: Source,             // Page(n) | White (GRP-1) | Glyphs (font atlas)
    pub rect: (i32, i32, i32, i32), // U, V, W, H on the source
    pub orient: u8,                 // quarter turn + flips, applied as page_xy does
    pub repeat: bool,               // GSAMPLE 1
    pub matrix: Affine,             // image -> layer -> frame, already including fine()
    pub tint: u32,                  // SPCOLOR, or cell colour times TCOLOR
    pub add: bool,                  // #A_ADD / TBLEND 1
}
```

How the current code maps onto it:

- **Sprites** become one `Item` each. The `Sprites::pose` link
  arithmetic, the blank-page rule, `Affine::place` and `Runtime::fine`
  stay in the builder.
- **Text screens** become one `Item` per non-empty cell, plus the
  cursor's bar. A cell sources GRP5, the TPAGE page, or a **glyph atlas**
  for `Font::pixel` fallbacks. The atlas is a CPU-built page, rebuilt
  only when the font changes. Per-cell items are equivalent to today's
  whole-screen drawing: today a frame pixel belongs to the cell under
  its centre (`(x / size, y / size)`), which is exactly what half-open
  per-cell coverage gives with the same matrix. On upright screens the
  CPU backend can merge a screen's cells back into today's
  `draw_upright` fast path, since it knows they share one matrix.
- **Text on a 4x4 layer** keeps today's semantics by default: a
  `FlatGroup` item (cells drawn flat into an offscreen of
  `flat_area`'s size, then projected). Projecting each cell directly
  would remove the bound described in [differences.md](differences.md).
  That is a behaviour change: decide it after TTEST36-style checks.
- **Raster filters** snapshot the array rows into the pass when the
  scene is built. The backends then never see `ArrayRef`, and the
  snapshot also serves as a cache key (see [3.3](#33-redraw-on-change)).
- **The sort** is kept exactly: layer, Z, kind, id. To avoid the
  per-frame `Vec`, the builder can bucket by layer and keep the vector.

The rasterizer trait:

```rust
pub trait Rasterizer {
    /// Uploads what changed on the pages since the last frame.
    fn sync_pages(&mut self, grp: &Graphics, dirty: &PageDirty);
    /// Draws the scene. The CPU backend fills a Framebuffer; a GPU
    /// backend draws into its own target (the frontend shows it).
    fn draw(&mut self, scene: &Scene);
}
```

The CPU backend's `draw` is today's loop: draw through `draw_posed` /
`draw_placed` equivalents, filter, compose, fade. Its output must stay
pixel for pixel what `Runtime::render` gives now.

### 3.3 Redraw on change

The redraw gate stays first and cheap: `changed`, `take_changed`,
`drawn_with`. It decides whether a scene is built at all. Then:

- The built scene is compared with the last one. Use a hash of
  `passes`, `items`, `fade` and the page change counts. If equal, the
  frame is reused. This catches commands that set the same values,
  such as `SPOFS` to the same place every frame, which today always set
  `changed`.
- Inputs read by reference at render time must enter the scene by
  value, so that the comparison sees them: raster arrays (snapshot),
  GRP pages (change counts). Check today whether a raster array that
  the program writes without calling a command redraws the frame.
- A GPU backend can go further and cache per-pass targets: a composed
  layer whose items, inputs and pages did not change keeps its texture.

### 3.4 GRP pages stay on the CPU

All page reads (`GPGET`, `GSAVE`, `GCOPY`, `GPAINT`, GARRAY) and all
drawing stay CPU-side, as now. So no GPU readback is ever needed, and
SmileBASIC 4's rules for GARRAY and GUPDATE stay in `graphics.rs`.

For the GPU, each page that is in use is mirrored as a 2048x2048
texture:

- **Format.** `R32Uint` holds the same bytes as `Graphics` (one
  `0xAARRGGBB` word). The shader unpacks it with integer ops, which
  keeps texel values exact. `RGBA8Unorm`/`BGRA8Unorm` also work if the
  shader uses `texelFetch` and does its arithmetic in integers.
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

- Emulator UI drawn on the frame: the hint band, the completion list
  and the dimming. Draw them into a separate overlay `Framebuffer`
  with its own texture (as `layer.rs` does for panels) instead of into
  the frame. Then the frame itself can come from the GPU.
- The editor, menu bar, panels and dialogs. They already redraw only
  on change, and they are not SmileBASIC 4 rendering.
- GRP drawing, and anything that has no GPU path yet. The fallback
  rule should be per frame: if a scene needs a feature the GPU backend
  lacks, the whole frame is drawn on the CPU. Mixing the two within one
  frame needs readbacks, which cost more than they save.

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
- **Exactness.** Fixed-function blending computes in the GPU's own
  precision and rounds on the way back to 8 bits, so floor-based
  formulas (tint, additive light, modes 3 and 4) **cannot be
  reproduced**. Even `over` may differ by 1 between drivers.
- **Filters** have no shader support. Some could be built from
  render-to-texture tricks (mosaic from a nearest downscale; raster as
  per-row strips), but blur and colour conversion cannot.
- **Rust.** No new dependencies.
- **Verdict.** It is fine for the final scaling it does now. As a
  scene rasterizer it is approximate everywhere and cannot do 4x4
  layers or filters.

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
  shader, built offline (glslc/dxc/spirv-cross, or SDL_shadercross)
  and committed or generated at build time.
- **Only the fragment stage is custom.** Vertices are still 2D
  `SDL_Vertex`. Blending is still SDL's blend mode, so integer-exact
  blending needs the destination read in the shader. That means
  ping-ponging between render targets, binding the previous target as
  a sampler. A projected quad works if the shader does the inverse
  homography per pixel, drawn over the quad's screen bounds: the same
  inverse mapping `compose::project_image` does on the CPU.
- **Rust.** The sdl3 crate (0.20) has no safe wrapper for
  `SDL_GPURenderState`. Raw `sdl3::sys::render` calls are needed, plus
  `sdl3::gpu::Shader::raw()` or raw `SDL_CreateGPUShader`. The device
  comes from `SDL_GetGPURendererDevice` as a raw pointer.
- **Verdict.** It is the smallest step from today's code, and the rest
  of the UI keeps using SDL_Renderer unchanged. But it is limited to
  Vulkan/D3D12/Metal machines, the fragment stage, and ping-pong for
  exact blends.

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
| Exact integer blending | no | with ping-pong | yes (compute or ping-pong) | yes (ping-pong; compute on native) |
| 4x4 projection exact | no | yes (per-pixel inverse in shader) | yes | yes |
| Filters | no | yes (fragment) | yes | yes |
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
  needs its own bias. SmileBASIC 4's own ties depend on the GPU's
  arithmetic in any case ([2.10](#210-proposed-ttest-batches)).
- **Raster filter coordinates and the colour filter's HSV**: these are
  f64 on the CPU today.
- **Fixed-function blending**, as SmileBASIC 4 uses (4.1): ±1 per
  channel where a value lies very near a half, between vendors and
  against the CPU.

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

- **Golden scenes.** A set of small scenes as `.sb` programs or built
  directly as `Scene` values, rendered by the CPU backend into checked-in
  frame hashes (small PNGs only where a picture helps review). The
  scenes cover every rule in section 2: each blend and compose mode,
  tint, flips and turns, scales 1, 1.5, 2, 2.5, rotations 30/45/90,
  XSCREEN sample 2, LCLIP, 4x4 layers (FINITI-like cube faces), each
  filter, FADE, and text with TPAGE and fallback glyphs. These hashes
  are also today's regression net for the split
  ([phase 1](#6-phased-plan)).
- **CPU vs GPU.** The same scenes are rendered by the GPU backend and
  diffed against the CPU frames under the policy above. The report
  counts differing pixels and checks they all lie on edges.
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

**Phase 1: scene builder, CPU backend.**

- Introduce `Scene`, `LayerPass`, `Item` and the `Rasterizer` trait in
  `sb-runtime`.
- Move today's drawing into `CpuRasterizer`. `Runtime::render` becomes
  "gate, build scene, compare, rasterize, overlay".
- Move the hint, completion and dimming into an overlay framebuffer
  shown by the frontend.
- Milestone: all tests and golden hashes unchanged; `--bench` draw
  times within noise of before on the projects in
  [1.7](#17-measured-per-frame-costs) (scene building for 4096 sprites
  must stay well under a millisecond); no new per-frame allocation.

**Phase 2: change tracking.**

- Tile dirty bitmaps on GRP pages, the glyph atlas, the scene-hash
  reuse, and raster array snapshots.
- Use the dirty bitmap in the debugger's GRP view.
- Milestone: frames that only repeat state are skipped (measure on a
  program that sets the same `SPOFS` every frame); page uploads
  proportional to what was drawn.

**Phase 3: choose the GPU option and prototype.**

- Decide between (b), (c) and (d) with section 4's trade-offs. Weigh
  coverage against dependencies, and decide how the UI is presented.
- Implement the exact subset: upright items at integer scale, tint, both
  blends, compose modes 0-4, FADE. Everything else falls back to the CPU
  per frame.
- Milestone: CPU-vs-GPU suite green for that subset on lavapipe;
  ELE2020 and NEON_DRIFTWAY-style scenes faster than the CPU path on a
  mid-range GPU, measured with `SB_UI_BENCH`.

**Phase 4: full feature set.**

- Rotated and scaled items, 4x4 projection (per-pixel inverse
  homography), XSCREEN sample, LCLIP, all filters, sub interpreter
  screens.
- Milestone: no CPU fallback needed for any project in the bench set;
  the inexact paths' differences stay within the policy.

**Phase 5: default and fallback.**

- GPU by default where the backend initialises and a start-up self-test
  scene matches the CPU. The CPU path otherwise, and as a menu
  setting.
- Document the choice in [differences.md](differences.md) if any
  visible difference remains.
- Later: a WASM build with the CPU backend first, then WebGL2/WebGPU
  if (d) was chosen.
