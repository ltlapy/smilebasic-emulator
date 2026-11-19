# Renderer verification

The evidence for each rendering rule: its status, the TEST results behind it,
how the emulator's two drawing paths compare, and how the results were measured.

- [renderer.md](renderer.md) describes how rendering works.
- [renderer-decisions.md](renderer-decisions.md) says why the GPU path takes its
  form.
- [sb4-pitfalls.md](../smilebasic4/sb4-pitfalls.md) records confirmed SmileBASIC 4 behavior,
  and [hardware-tests.md](../smilebasic4/hardware-tests.md) how the check programs run.

Contents:

- [Status](#status)
- [Rules](#rules)
- [Ties: what TEST63 says](#ties-what-test63-says)
- [Earlier tie evidence](#earlier-tie-evidence)
- [Comparison tests](#comparison-tests)
- [GPU path results](#gpu-path-results)
- [Scene split results](#scene-split-results)
- [How results were measured](#how-results-were-measured)
- [Test batches A to G](#test-batches-a-to-g)
- [History](#history)

## Status

Each rule below is marked:

- **C**: confirmed on hardware or by the manual. The table cites the source.
  "Pitfalls" means [sb4-pitfalls.md](../smilebasic4/sb4-pitfalls.md).
- **U**: not yet seen on hardware.

R1 to R31 are the IDs of the items that were open when the test batches were
written ([Test batches A to G](#test-batches-a-to-g)).

Still open:

- An inverted cell on the console: the sum (`inverted_texel`, marked
  UNVERIFIED in `raster/text.rs`). For
  larger font types, the font page's corner it reads is marked UNVERIFIED there
  too.
- The line cursor's width on cells other than 8 pixels (marked UNVERIFIED in
  `text.rs`).
- How much of `#GSPRITE`'s settings XSCREEN resets (marked UNVERIFIED in
  `runtime.rs`, `set_mode`).
- Ties at angles other than multiples of 90 degrees (R5).
- The look of DIRECT mode's selected text, and LINPUT's selection.
- GSAVE and GLOAD: negative sizes, and the error past memory.
- TSAVE and TLOAD sizes (R31).
- Errors for a bad 16-element array. These are not rendering, and were left
  unchecked.

## Rules

### Blending and color arithmetic

| Rule | Where | Status |
|---|---|---|
| One rounding per draw: products and blends are exact (each 8-bit value is k/255) and rounded to nearest once when stored: element to layer (or frame), layer to frame, frame to display | `compose::Paint` | C: TEST49 (every sample within 1, the rest the host GPU's rounding); see pitfalls, "Colors and blending" |
| Normal blend of an element: `light + d*(1-a)`, one rounding | `compose::Over1`, `Over3`, `Over5` | C: TEST49; TTEST16 |
| Frames and composed layers hold premultiplied color, alpha as coverage (SmileBASIC 4 keeps transmittance, 255 less it); a layer starts clear; an additive element in a composed layer adds light without coverage | `compose.rs` header | C: pitfalls "An additive sprite inside a composed layer..." |
| Additive element: `d + light`, the light `t*c*a` exact and rounded once, with saturation, destination alpha kept | `compose::Lit` | C (R1): TEST49 |
| Sprite color (SPCOLOR) multiplies each channel, alpha included, unrounded, inside the one rounding of the draw | `graphics::Tint`, `compose::Over3` | C (R2): TEST49. A child's link color `floor(a*b/255)`, alpha too, applied parent first (`multiply`): C, TEST56, no misses |
| Text color (cell color, then TCOLOR) the same, unrounded | `compose::Mixed`, `Over5` | C (R2): TEST49. |
| An inverted cell, attribute bit 7: see below | `inverted_texel`, `attr::INVERT`, `TextScreen::inverts` | U on the console. C elsewhere: TEST56. Not the INPUT cursor (TEST50) |
| TLOAD, TUPDATE and TCOPY keep all 16 attribute bits; TSAVE, TARRAY and CHKCHR read them | `Cell` | C: TEST56 parts 1 and 4 |
| Tint is applied to the page pixel before blending | `raster::sprite::draw` | C (R2): TEST49 |
| Relative color animation `"C+"`: product / 256, rounded down | `anim.rs` | C: pitfalls |
| LAYER mode 0 draws straight onto the frame, layer color ignored | `Runtime::render` | C: pitfalls; TTEST16 |
| Modes 1-4: the layer color scales light (`P = p*c*ca`) and coverage (`A = la*ca`), unrounded | `compose::compose_tinted` | C (R3): TEST49, colors `C0806040`, `7F40C0FF`, `FFC08040`, `80FFFFFF`, white |
| The sum of each mode: see below | `compose::compose` | C (R3): TEST49. TEST56: modes 3 and 4 make the whole subprogram's screen opaque |
| A sub interpreter's frame is premultiplied over the main one (dimmed to 224/255), cleared to BACKCOLOR's color unmultiplied with coverage `255 - alpha` (alpha 0: clear); an opaque BACKCOLOR replaces the main screen | `Scene::build`, `screen::cover` | C (R4): TEST49 for alphas 0, 1, &H7F, &H80, &HFE; TEST56 for &HFF, and over console text for &H40 and &HC0 |
| Beside a subprogram, the side without the focus: times 224/255, rounded in the same step as FADE | `screen::cover`, `screen::darken` | C: TEST49 |

An inverted cell applies on the console only: no other screen's program has the
flag, and TEST56 part 1 shows plain glyphs there. The texel is taken as
`n,n,n,n` for `n = 1 - alpha` and goes through the same sum. This covers any character, an empty cell too. Larger font types read
the font page's corner for what is inverted.

Mode sums, one rounding each: mode 1 is `P + d*(1-A)`, mode 2 `d + P`, mode 3
`d*(P + 1-A)`, mode 4 `d + P*(1-d)`. Mode 1 adds to the frame's coverage, mode 2
keeps it, and modes 3 and 4 make it opaque.

### Sprites

| Rule | Where | Status |
|---|---|---|
| Placement: home to position, scale, then clockwise rotation in degrees | `Affine::place` | C: manual (SPHOME, SPSCALE, SPROT) |
| Flip first, then quarter turn | `Source::page_xy` | C: pitfalls |
| Sampled without smoothing, at frame pixel centers; away from ties every rotation and scale fits with no misses | `draw_image`, `draw_upright` | C (R5): TEST50 generic angles |
| With XSCREEN sample > 1, sprites are sampled at the finer pixels' centers; LCLIP grows to whole fine pixels | `Layer::fine` | C (R6): TEST50 part 2 (LCLIP exact; the fine grid misses only 56 tie pixels of 54298) |
| Coverage: a pixel is drawn when its center, nudged right then up, lies on the image, at any rotation. An edge through the center takes it if it is the left or bottom edge on the frame | `Placement::sampling` | C (R5): TEST50, no coverage miss in any part, text included (consistent with a rasterizer's top-left rule on a target drawn flipped) |
| A sample point exactly between texels, at a multiple of 90 degrees: see below | `Placement::texel_tie`, `page_signs`, `texel`, `Source::wrap` | C on a Switch 2, 3831 sprites (TEST63); the first case without a miss on the exact shots, the second about 9 in 10 of what it covers; see below |
| The same at other angles (only 45-degree multiples can tie): image x higher, image y lower; kept on the image | `Placement::texel_tie` | U (R5): TEST50, no misses but 21 pixels reading the row above the image |
| SPCHR W and H are the size as shown (turned size rule) | `sp.rs` | C: pitfalls; TEST45, TEST46 |
| GRP-1 is white everywhere; GSAMPLE 1 repeats the page, 0 is transparent off it | `sprites::sample`, `Source` | C: manual (SPPAGE, GSAMPLE) |
| A linked child's offset turns and scales with the parent; the flags decide the child's own image | `Sprites::pose` | C: pitfalls |
| Children without link bit 0 stay where they showed, Z included, when the parent is cleared | `Sprites::clear` | C: pitfalls ("Z included") |
| Animated XY, Z, UV and I keep whole numbers in 16 bits and divide, `(from*left + to*gone)/frames`, dropping the fraction. Colors mix in single precision by the frames left, rounded down. V is in double precision | `anim::mix` | C: pitfalls; TEST47 (R7), TEST56, TEST60 (UV, I), TEST62 (V reads 0.0333333333 in ten decimals) |
| Rotation and scale kept in single precision, slid as `from*left + to*(1-left)` in single precision and not rounded | `sprites::single`, `anim::mix` | C: pitfalls; TEST48 |
| SPANIM, TANIM and FADE take their first step at the frame job's first update after the command; no flag for a program that is behind | `Runtime::animate` | C: pitfalls; TEST60 on a Switch 2 |
| SPDEF's default table when none was imported (grid of 16x16) | `grid_defs` | Stand-in, not a rule. The real table is imported |

Ties at a multiple of 90 degrees, for a sample point exactly between texels:

- An image side that is a power of two with a scale that is a power of two reads
  the page's higher texel.
- Anything else reads the page's lower, at any turn or flip.
- The point may land just off the image, and reads the page there. Before the
  page's first texel it reads that one; after the last, nothing.

### Text screens

| Rule | Where | Status |
|---|---|---|
| Flip then turn for ATTR | `raster::text::draw` | C: pitfalls |
| Ties: as for sprites of the cell's size, per cell and by its ATTR. A cell reads the page past itself, what lies beside it on the font page or `TPAGE` | `draw_turned`, `draw_upright` | C on a Switch 2, 12 screens of 16 cells (TEST63) |
| Font types 24-64 show only user-defined characters; GPUTCHR with them reads GRP5 in 24-pixel cells | `glyph_pixel_px` | C: pitfalls |
| Tab stops every 4 columns | `TAB_WIDTH` | C: pitfalls |
| `CHR$(10)` starts a new line | `TextScreen::print` | C: pitfalls |
| TSCREEN clears the screen | `set_font` | C: pitfalls |
| TSCREEN keeps TLAYER, TPAGE, TVAR and TFUNC, and sets TBLEND back to 0 | `restart_look` | C: pitfalls; TEST47, TEST48 |
| INPUT cursor: a bar 2 by 8 at the cell's left, opaque white whatever the color; it alternates with nothing | `raster::text::draw` | C (R9): TEST50, parts 6 and 7 |
| The cursor's blink: 20 frames on, 20 off, counted in frames played. It is on from the start of a line and from each key that moves or edits, the first period a frame short | `LINE_CURSOR_BLINK`, `LineEdit::key` | C: the count steps with `MAINCNT`'s; TEST56 part 1 fits 2 of 2 shots at offsets -3..-1 |
| The cursor's width is the cell's shifted right by two, or the whole cell after Insert; its height the cell's. It follows TSCALE and LMATRIX, and draws right after its screen's cells, under what is in front | `LineCursor`, `raster::text::draw` | C: TEST56 part 1, 24 wide at scale 3, cut by a sprite in front. U on cells other than 8 pixels |
| On a composed layer the cursor has no coverage, so it adds | `LineCursor::composed` | C: TEST56 part 1, `E0F0FF` |
| Only DIRECT mode's line selects (`LineEdit::selects`), drawn inverted in the console's color, staying after Enter | `LineEdit::draw`, `finish` | C that INPUT does not: TEST56. U: the look, and LINPUT's |
| Text animations: as for sprites | `impl Animated for TextScreen` | C: pitfalls; TEST47 (R7) |
| TBLEND 1 is additive, as for sprites | `screen.rs` | C: manual (TBLEND); arithmetic as R1 |

### Priorities and Z order

| Rule | Where | Status |
|---|---|---|
| Layers 7 to 0, lower layers in front | `Runtime::render` | C: manual (LAYER) |
| Within a layer: larger Z further back | sort key | C: manual (display priority -4095 to 4095) |
| Equal Z: text screens behind sprites; among either, higher id behind | sort key | C (R10): TEST50 part 5, 16 pairs of 16, `#GSPRITE` too |
| `#GSPRITE` goes to the layer SPLAYER gives new sprites after XSCREEN | `set_mode` | C: pitfalls |

### Layers and projection

| Rule | Where | Status |
|---|---|---|
| `LCLIP` corners inclusive | `Clip` | C: pitfalls; TEST50 part 2 at XSCREEN sample 2: grows to whole fine pixels, `[2a, 2b+1]` |
| `LMATRIX layer,hx,hy` moves the layer by minus the home point | `screen.rs` | C: pitfalls |
| The 2D LMATRIX form: `Affine::place(pos, home, rot, scale)` like a sprite: scale, then turn clockwise, x and y outside; after the element's own placement | `screen.rs` | C (R11): pitfalls; TEST53 part 1 |
| 4x4 LMATRIX: `(x,y,0,1)` to clip space, X right, Y up, column-major, perspective divide | `compose::homography` | C: pitfalls; TTEST26, TTEST27 |
| Sprites and text screens on a 4x4 layer show whole, also off-screen parts | `project_image`, `scene::flat_area` | C: pitfalls; TTEST36 (text bounded: [differences.md](../using/differences.md)) |
| 4x4: a sprite's Z is not used; shown where W > 0 and Z/W is from -1 to 1 | `compose::project`, `project_image` | C (R12): pitfalls; TEST53 part 2 |
| 4x4: sampled without smoothing | `compose::project` | C (R12): pitfalls; TEST53 part 2 |
| LCLIP on a 4x4 layer is in frame pixels, after projection, inclusive, Y from the top | `project(..., clip)` | C (R13): pitfalls; TEST53 part 3 |
| Errors for a bad 16-element array | `screen.rs` | U, not rendering (skip) |

### Filters

| Rule | Where | Status |
|---|---|---|
| Mode-0 layer: LFILTER filters everything drawn so far, BACKCOLOR included; composed layer: only its own | `Runtime::render` | C: pitfalls; TTEST22/23 |
| The same for a mode-0 layer with no elements | `Runtime::render` | C: pitfalls ("That holds for a layer with nothing in it, too") |
| Mosaic: the whole frame drawn into `ceil(width/n)*sample` x `ceil(height/n)*sample` half-buffer pixels with bilinear sampling, then back with nearest; blocks are n frame pixels at any sample | `filter::mosaic` | C (R14): pitfalls; TEST54 part 1, no pixel more than 1 off; TEST56 part 2, n=7 on 640x360, no pixel off |
| Blur: bilinear to half size (2x2 mean, halves down), a row pass and a column pass each stored in 8 bits, nearest back; 0 sharp | `filter::blur` | C (R15): pitfalls; TEST54 parts 2, 3, TTEST24 |
| Blur weights: a Gaussian of variance amount/10 at odd distances, normalized over 31 taps, used in the 3, 5, 7 or 9 taps that the 99% rule picks, not normalized again | `filter::blur_kernel` | C (R15): TEST54 parts 2, 3, no pixel more than 1 off |
| Blur halving is in frame pixels whatever XSCREEN's sample | `filter::blur` | C (R15): TEST54 part 3 |
| Mosaic and blur work on what shows through (255 less the emulator's coverage), so a wide blur darkens behind a clear layer | `filter::shrink`, `expand` | C: TEST54 part 2 |
| Raster: `[n,4]` entries of X offset, X scale, Y offset, Y scale, OpenGL axes, center times scale plus offset, repeat flag required | `filter::raster` | C: pitfalls; TTEST25/26 |
| Raster: scale, then offset; the Y offset's sign is + from the bottom | `filter::raster` | C (R16): pitfalls; TEST54 part 4 |
| Raster `[n,2]` form acts along the deformation; a row takes entry `floor((i+.5)*n/size)` | `raster_rows`, `raster_into` | C (R17): pitfalls; TEST54 parts 5, 6 |
| Raster on a direct layer: what falls off shows black | `Runtime::render` | C: pitfalls |
| Raster after LMATRIX | `Runtime::render` | C: pitfalls; TTEST27 |
| Color conversion: hue in degrees, S and V on 0-255 | `filter::shift_color` | C: pitfalls; TEST54 part 7 |
| Color conversion arithmetic: SmileBASIC 4's formulas in f32 on the premultiplied texel; a grey keeps saturation 0 | `shift_color` | C (R18): TEST54 part 7, no pixel more than 1 off |
| Filter before the layer color (modes 1-4) | `Runtime::render` | C (R19): TEST53 part 4. The emulator rounds to 8 bits between the two, SmileBASIC 4 does not: up to 1 off |
| A clear pixel is left alone in mode 1 only; in modes 0, 2, 3, 4 a raised value lights it | `filter::color` | C (R19): TEST53 part 4, layer 5; TEST56 part 2 for modes 0, 2, 3 |
| ACLS clears LFILTER | `Runtime::acls` | C: pitfalls; TEST47 (R20) |

### FADE

| Rule | Where | Status |
|---|---|---|
| Fade steps: single-precision mix, rounded down; a clear end takes the other end's color | `Fade::step`, `Fade::start` | C: pitfalls |
| ACLS clears FADE | `acls` | C: pitfalls |
| Fade cover: `(c*a + p*(255-a) + 127) / 255` (nearest), over the final frame, its alpha kept | `screen::cover` | C (R21): TEST49, four fade colors (alpha &H80, &H7F, &H01, &HFE) |
| A fade steps with the animations, starting on the frame after FADE | `Runtime::animate` | C: pitfalls; TEST47 (R22) |

### Display (XSCREEN composite mode)

| Rule | Where | Status |
|---|---|---|
| Mode 0 bilinear, mode 2 nearest; BACKCOLOR not in the side bars (black) | `frame_texture` | C (R23): TEST55, window captures identical over the whole frame |
| Mode 1: `t = clamp(2*fract(s-.5)-.5, 0, 1)` per axis at every scale, by bilinear from a nearest copy twice the size, or on the CPU | `SmartFrame`, `smart_scale` | C (R23): TEST55 parts 2, 6, window captures within 1 |
| All modes fill the fit rectangle, not whole factors (the menu's sharp choice keeps to whole factors) | `screen_rect` | C (R23): TEST55 parts 3, 4, 5 |
| Aspect ratio other than width/height: non-square pixels, letterboxed | `screen_rect` | C (R23): TEST55 part 4 |

### GRP drawing primitives (raster rules)

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
| GTRI: rows from the leftmost to the rightmost pixel the edges reach, each pixel once, a degenerate triangle too; each edge walked down from its top by SmileBASIC 4's sum, corners not clamped | `edge_span`, `Graphics::triangle` | C: pitfalls; TEST47 (R26), TEST48, TEST56 (`triangles_past_the_page_match_hardware`) |
| GPAINT: 4-connected flood, border mode stops at the border color | `Graphics::paint` | C: pitfalls; TEST47 (R27) |
| GCLIP shared by all pages | `Graphics::clip` | C: pitfalls |
| `#G_NORMAL`, `#G_NORMAL2` | `Blend::apply` | C: manual |
| `#G_ALPHA`, `#G_ADD` rounding (floor) | `Blend::apply` | C: comment "rounding matches values read back on hardware" |
| `#G_ALPHA2`: alpha `sa + da - floor(sa*da/255)`, the two colors' weights on 0-255 rounded down | `Blend::apply` | C: pitfalls; TEST47 (R28) |
| GCOPY color-code multiply (floor), then blend; overlapping copy within a page reads first | `Graphics::copy` | C: pitfalls; TEST47 (R29) |
| GPUTCHR mode 0 clears under transparent glyph pixels | `put_chars` | C: pitfalls |
| GPUTCHRP widths, function form scales spacing | `put_chars_proportional` | C: pitfalls; TTEST27 |
| GPUTCHRP drawing scales the spacing as the width does | `put_chars_proportional` | C: pitfalls; TEST47 (R30) |
| GLOAD palette indexes past the palette leave the page's pixel | `load_rect` | C: pitfalls; TEST47 (R31) |
| GSAVE/GLOAD rectangles off the page read as empty | `save_rect`, `load_rect` | C: pitfalls |
| GSAVE/GLOAD width and height: any, 0 included, up to what memory holds | `grp.rs` (`MOST_PIXELS`) | C: pitfalls; TEST48. U: negative sizes (Out of range, from the code), and the error past memory (Out of range in the emulator, see differences.md) |
| TSAVE and TLOAD width and height: 0 or more, as GSAVE | `txt.rs` (`MOST_CELLS`) | U (R31): from the code; the error past memory stands in as for GSAVE |
| GARRAY/GUPDATE: GPGET sees array writes at once; the screen after GUPDATE | `Graphics::update` | C: pitfalls ([differences.md](../using/differences.md) for the omission) |

## Ties: what TEST63 says

TEST63 ran 3831 sprites and 12 text screens on a real Switch 2, at sizes 8 to
64, scales 0.5 to 2, whole, half and quarter positions, every turn and flip. The
album's JPEGs of the first three shots have coarse tables (quantization sum 736,
against 64), so one pixel's V cannot be trusted there; shots 4 to 7, 256 sprites
and the text, read to within 1. The analysis reads ties per line of the image
and per triangle, by summing a line's pixels.

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

The first row is exact arithmetic: the side and the scale leave nothing to
round, and floor reads the higher texel. It holds at every flip and turn, so the
rule goes by the page, where the rules before went by the rectangle or the
frame. Everything else has rounded and comes out just low, whichever way the
image runs.

What is left is GPU noise, with no simple rule:

- The same 16x16 at 1.5 reads its V high at Y 404 and low at Y 377.
- A line along the image can change from high to low part of the way through.
- At 270 degrees often one triangle goes the other way.

A family of 500 single-precision models of the rasterizer was scored on the
exact shots. They vary Y flipped or not, plane equations relative to a vertex or
to the origin, barycentrics, by division or by a reciprocal, rounding to nearest
or truncating, at each of the 6 anchors. The best, truncating barycentrics,
misses 1604 lines of 22112 where the rule misses 1135, and none of those models
does better than the rule. So the rule stays simple, and the CPU needs no
triangles and no fast path for them.

Against the shots, pixels more than 20 of luma off: 280599 before, 67416 after,
and on the exact shots 58124 and 10662. TEST62's 22 sprites, all in the noise
class where it counts, go from 105 to 746 off. TEST50's Ryujinx shots get better
in part 1 (1249 to 880), the same in 2, and worse in part 3 (464 to 2968) and 4
(594 to 1298). TEST56 part 2, Ryujinx, goes from 2128 to 649. A real Switch
outranks these Ryujinx results.

The GPU path gets ties from the host's arithmetic unless steered, which `gpu.rs`
does by `Placement::tie_sides`.

## Earlier tie evidence

TEST50, TEST56 and TEST62 came before TEST63, which settled the rule. The rules
they compare are not the code's any more.

Every page pixel in TEST50 has its own color, so each screenshot pixel names the
texel sampled. Away from ties all rules agree. At ties the screenshots show the
GPU's arithmetic: in one quad the texel changes along the quad's diagonal,
between its two triangles, and the same image at the same scale goes different
ways at different places.

Simulating the quad in single precision (vertices through the matrix and
viewport, attributes by plane equations and by barycentrics, snapped to 1/256 or
not) did not reproduce which way each tie goes. So what remained seemed to be
the host GPU's, and a real Switch might differ. The rule at the time took the
usual outcome:

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

Generic angles have no misses before or after. What was left is mostly:

- half of a quad at 90 and 270 degrees, where the two triangles differ;
- sizes other than powers of two starting on a half pixel;
- text and sprites reading a pixel just off their image, which the emulator's
  rule then never did.

TEST56 parts 1 and 2 add ties of sizes 8, 15, 16 and 16x15 at scales 1, 1.5 and
2, and in places they go against that rule. A size 15 side at scale 1 on a half
pixel goes high, and 8 and 16 at scale 1.5 mostly low. Simple per-axis changes
were measured against all of it, pixels more than 1 off:

| Rule | TEST50 1 | 2 | 3 | 4 | TEST56 P1 ties | P2 ties | All |
|---|---|---|---|---|---|---|---|
| As it stands | 770 | 56 | 464 | 626 | 569 | 2566 | 5051 |
| Odd side at scale 1 goes high | 1533 | 56 | 464 | 626 | 135 | 2566 | 5380 |
| Power of two at another scale goes low on the image | 770 | 56 | 2968 | 1394 | 935 | 1278 | 7401 |
| The same, towards the page's lower coordinates | 770 | 56 | 2968 | 1394 | 935 | 1278 | 7401 |
| Both of the first changes | 1533 | 56 | 2968 | 1394 | 501 | 1278 | 7730 |

Each fixes one test at another's cost, which is what host GPU noise would look
like, so the rule stayed.

TEST62 then showed ties on a real Switch 2, on 21 sprites at multiples of 90
degrees, all with SPSET's attribute 1 (a quarter turn, not meant by the test),
so a sprite's ramp shows only one axis:

- Sizes 8 and 16 at scale 1 go to the higher texel, the point on the image's far
  edge reading the page's next texel, which the emulator's rule then prevented.
- Sizes 8 and 16 at scale 1.5 go to the frame's left and bottom. In that run
  that is also the page's higher texel: nothing tells which of the two is the
  rule.
- Size 15 at scale 1.5 goes towards the shown rectangle's lower coordinates on
  both axes, where a Ryujinx run of the same sprite (TEST50, S1.5A90) was mixed.
  The page's lower coordinates, the rule before, misses one axis of the two.
- Size 15 at scale 1, the quad's diagonal shows: the first triangle of its strip
  (top left, top right, bottom left of the rectangle) goes to the higher texel
  and the second to the lower. No per-axis rule fits; the emulator's had the
  second's.
- Single-precision plane equations from the vertices, or barycentrics, with any
  anchor, get all of sizes 8 and 16 and miss size 15's as above.

Pixels more than 1 off (TEST50 and TEST56 from Ryujinx, on a PC's GPU; TEST62's
a shot of the 22 sprites, 9 of luma off):

| Rule | TEST50 1 | 2 | 3 | 4 | TEST56 P2 | TEST62 |
|---|---|---|---|---|---|---|
| Before | 770 | 56 | 464 | 626 | 2566 | 290 |
| Reading past the image | 880 | 86 | 464 | 594 | 2128 | 266 |
| Also other sizes along the shown rectangle | 1249 | 86 | 464 | 594 | 2128 | 105 |
| Also power of two at other scales, the page's higher texel | 1249 | 86 | 2512 | 2720 | 2864 | 105 |
| Other sizes towards the frame's top left, not along the rectangle | 3553 | 206 | 464 | 594 | 4037 | 120 |

The third row was the code's rule until TEST63. What a real Switch shows decides
over what Ryujinx shows: TEST50's part 1 gets worse, on a host GPU's noise. Text
kept to its own cell at that point, off which SmileBASIC 4 reads the font page's
next character's pixels.

## Comparison tests

The CPU is the reference for the GPU path, and hardware for the CPU.

**Golden scenes.** `crates/sb-sdl/src/gpu_golden.rs` holds small programs, each
drawn by the GPU path and by the CPU at once, so no frames are kept. They cover:

- each blend and compose mode, tint, flips and turns, whole and other scales;
- XSCREEN sample 2, LCLIP, 4x4 layers with text, each filter, FADE, repeating
  pages;
- DIRECT mode's hint, list, cursor and selection, and sub screens.

Each has a limit on the pixels more than 2 off, and at most 0.1% may have an
alpha more than 2 off. `SB_GPU_GOLDEN=1` prints a row for each and
exits, 1 if one is over. `cargo test -p sb-sdl --test gpu_golden -- --ignored`
does it with no window. It is ignored as CI has no GPU.

**Start-up.** The self-test ([renderer.md](renderer.md)) runs when the path
starts. `SB_GPU_TEST=off` skips it and `fail` tries the way out. An earlier
version of the scene was 1 off at the most on opengl, opengles2 and vulkan, on 1
to 1.7% of its pixels, and none more. It took 68 to 76 ms on opengl, 94 on
opengles2 and 133 on vulkan, on one desktop, once in a run.

**Projects.** The `--bench` projects of
[renderer-decisions.md](renderer-decisions.md) are rendered for N frames on both
paths, and the frame diffs are summarized, in a manual or nightly job. They are
not in the repository, since the projects are not the emulator's to commit.

**Hardware stays the arbiter.** A GPU/CPU disagreement on an exact path is a bug
in the GPU path. A disagreement on an inexact path is settled, if it matters, by
a screenshot from SmileBASIC 4 (batch C), and the CPU reference is changed only
then.

**CI candidates:**

- Linux: Mesa's lavapipe (software Vulkan) for SDL GPU. Check SDL GPU's Vulkan
  feature list against lavapipe once.
- Windows: WARP (software D3D12).
- macOS: runners have Metal.
- Software rasterizers also differ in f32 details, so the inexact tolerance
  applies there too.

## GPU path results

**Exactness.** Everything larger than rounding that has been seen lies on
elements' edges and on lines where samples fall between texels, as the policy in
[renderer.md](renderer.md) allows. Upright sprites and text at whole scales and
places, and the GRP scenes, are identical.

Pixels differing from screenshots of SmileBASIC 4 on an emulator on a PC's GPU,
the same frames, for each path:

| Test | CPU | GPU |
|---|---|---|
| TEST49 part 1 | 73728 | 59392 |
| TEST49 part 2 | 83968 | 74752 |
| TEST49 part 3 | 131072 | 130048 |
| TEST49 part 4 | 211968 | 206848 |
| TEST49 part 5 | 146432 | 134144 |
| TEST50 part 1 (ties) | 4336 | 4415 |
| TEST50 part 2 (ties) | 1209 | 1245 |
| TEST50 part 4 (ties) | 3441 | 3441 |

TEST49's differences are all by 1 or 2. On TEST49 the GPU is the closer, as it
should be: SmileBASIC 4 rounds the same way.

**Ties.** With the biases, the path follows the rule for edges and texels on
TEST50: 183 pixels of 921600 differ from the CPU in part 1, none in parts 3 and
5. The golden scene `ties` of sprites and cells, on half pixels and at 1.5,
flipped and turned, has none off. Left to the host's own rasterizer, with no
bias, that scene has 24704 of 384000 pixels off, so the bias stays.

**Raster filter.** TEST54 parts 4 to 6 and LRASTER: no pixel differs.

**Color filter.** The shader, on the `gpu` driver, matched the CPU on all 39.6
million pixels compared of LCOLOR and on TEST54 part 7, and within 2 on TEST53
part 4, where the layer's color follows. The GPU's wait was 1.3 ms a frame with
four such layers.

**Mosaic and blur.** TEST54 parts 2 and 3, against the CPU: about 1% 1 off,
under 0.5% 2 off, 3 at most. LBLUR, three blurred layers one over another: 6 at
most, 1.7% more than 2 off, since the differences add up over the layers.

**With the path off.** On the same bench runs as the [scene
split](#scene-split-results), TEST48 to 56's `--screen` and `--play` PNGs
included, checksums and PNGs were identical. Instructions were within 0.02%
except TMULTI, +0.84%, and TPERF, +0.17%, the text code having been split to
give a backend its inverted glyphs.

**Long runs.** 12000 frames each on opengl, opengles2 and llvmpipe:

- 10 sizes of `XSCREEN` from 128x128 to 1280x720 in modes 0 to 2, page changes,
  `EXEC`, a subprogram started and stopped;
- resets, resizes, full screen and the setting off and on every 400 frames.

No pixel was more than 2 off on the main screens in 228 frames compared, and at
most 7 of 6.3 million in the subprogram's. Live GL textures came back to 0 at
the end. With no events RSS holds at 397 MiB from the third sample to the last.

**In the frontend.** Pictures of the window with and without the path, no pixel
differing:

- modes 0, 1, 2 in a window of 1280x720, resized and in full screen;
- menu, About, the colors window, a notice, help, debugger's panel, editor
  inline and apart;
- a stopped program, DIRECT mode's hint, an error, an ended program with its
  targets spoiled by hand, device reset, the setting turned on and off.

## Scene split results

The scene builder went in with these checks, against the commit before, on one
x86-64 desktop, release build, `--bench --fixed`:

- Checksums identical on TPERF whole (10500 frames), its scenes TMULTI, SPROT,
  SPANIM, LCOMP, GAME, SPHIT, TSCROLL, VINT (2200) and FINITI, ELE2020, RYUSI,
  BASICDEMO, NEON_DRIFTWAY, SOLIDGUNNER_V, DVD_BOUNCE (1200).
- `--screen` and `--play` PNGs byte-identical on the hardware test programs that
  have run files, including sub screens.
- Instructions, against 0.07% between two runs of one build: TPERF -2.1%, TMULTI
  -5.7%, SPROT -2.7%, SPANIM +0.4%, LCOMP -0.3%, GAME -1.6%, FINITI -2.7%,
  ELE2020 -2.4%, NEON_DRIFTWAY 0.0%, DVD_BOUNCE +0.2%. Part of the falls may be
  inlining rather than work saved, so do not count on them.
- Peak memory was unchanged.

These comparisons are not in the repository, as the projects are not the
emulator's.

## How results were measured

**Only GRP drawing can be checked on hardware by automatic readback.**
SmileBASIC 4 has no command that reads the composed screen. Everything composed
(sprites, text screens, layers, filters, FADE) needs a lossless screenshot. The
tests in [hardware-tests.md](../smilebasic4/hardware-tests.md) do this: Ryujinx screenshots at
1:1 under `XSCREEN 1280,720`, compared with `smilebasic-check --screen`.

**Sources:**

- A real Switch 2: TEST60, TEST62 and TEST63. A real Switch outranks emulator
  screenshots.
- Screenshots of SmileBASIC 4 on an emulator on a PC's GPU: TEST49, TEST50,
  TEST53, TEST54, TEST55 and TEST56. That GPU stores results its own way.

**How close the emulator-hosted screenshots come.** TEST49's stored results are
within 1 of the rules (see [sb4-pitfalls.md](../smilebasic4/sb4-pitfalls.md)). TEST54's and
TEST55's are within 1 of the emulator in every channel, and TEST53's within 2
but for four lone pixels.

[renderer-decisions.md](renderer-decisions.md) has what the GPU path costs, and
[benchmarking.md](../performance/benchmarking.md) how to repeat those measurements.

## Test batches A to G

All seven batches ran. The sections below give each batch as written before it
ran, so the programs that ran may differ: TEST47, for one, had 23 parts. The
results are in [Rules](#rules) and [sb4-pitfalls.md](../smilebasic4/sb4-pitfalls.md).

| Batch | Subject | Items | Ran as | Read by |
|---|---|---|---|---|
| A | GRP raster rules | R24-R31 | TEST47; TEST48 for its smaller points | `GPGET` readback |
| B | Color arithmetic | R1-R4, R21 | TEST49 | Screenshots |
| C | Sprite and text sampling | R5, R6, R8, R9, R10 | TEST50; ties settled by TEST63 | Screenshots |
| D | Layers and projection | R11, R12, R13, R19 | TEST53 | Screenshots |
| E | Filters | R14-R18 | TEST54 | Screenshots |
| F | Timing and state | R7, R20, R22, TSCREEN's kept settings | TEST47 | Console |
| G | Display scaling | R23 | TEST55 | Window captures |

The batches follow [hardware-tests.md](../smilebasic4/hardware-tests.md):

- one program with numbered parts;
- `DUMPCON` saves the console as `RESnnPn` for automatic parts;
- screenshot parts wait for a button, so that `smilebasic-check --screen` and a
  lossless SmileBASIC 4 screenshot (Ryujinx, handheld, `XSCREEN 1280,720`, 1:1)
  can be compared pixel for pixel.

For screenshot parts:

- draw each patch as a solid 16x16 or 32x32 block on a known BACKCOLOR, so that
  one pixel inside it tells the answer;
- print a legend, so that patches can be found in the screenshot.

### Batch A: GRP raster rules (automatic, GPGET readback)

Each part draws on a cleared page and prints the result as text, so
`smilebasic-check` compares it row by row with no screenshot:

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

1. `GCIRCLE 8,8,R` for R = 1, 2, 3, 5, 7 (one part each, or side by side with
   offsets).
2. Arcs: `GCIRCLE 10,10,8,0,90`, `GCIRCLE 10,10,8,350,10`,
   `GCIRCLE 10,10,8,30,120,1` (sector), and `GCIRCLE 10,10,8,45,45,1` (equal
   angles with the sector flag).
3. `GTRI 0,0,12,3,4,11`, a thin sliver `GTRI 0,0,20,1,10,2`, and the degenerate
   `GTRI 0,0,5,5,10,10`.
4. `GPAINT` on a shape whose two halves touch only diagonally, then with a
   border color.
5. `GBOX 2,2,10,8,&H80FF0000,#G_ALPHA` over `GFILL ... &HFF0000FF`; print
   `HEX$(GPGET(2,2),8)` and `HEX$(GPGET(6,2),8)`. A pixel blended twice shows at
   a corner. Also `GBOX` with `X0>X1` and a 1-pixel-high box.
6. `#G_ALPHA2` grid: for destination alphas &H00, &H40, &H80, &HFF and source
   alphas &H01, &H80, &HFE, `GPSET` a color with mode 3 over a `GPSET`
   destination, then print `HEX$(GPGET(...),8)`.
7. GCOPY with a color code: copy `&HFF808080` with `&HFF7F7F7F` (floor gives
   `&HFF3F3F3F`, nearest `&HFF404040`) in modes 0 and 2. Then an overlapping
   copy within one page shifted by one pixel.
8. `GPUTCHRP 0,0,"AB",16,2,3` scaled: print the leftmost drawn column of B from
   GPGET, which shows whether the drawn spacing scales.
9. GLOAD with palette indexes past the palette: print GPGET where they land (0
   if transparent).
10. `GLINE -100000,3,100000,5` and `GCIRCLE 10,10,70000`: print a few rows
    (confirms clipping keeps the same pixels).

### Batch B: color arithmetic (screenshots)

R4 is the last part, as it needs a subprogram. Sprites are 32x32 solid blocks
drawn on GRP3 (`SPSET n,U,V,32,32` and `SPPAGE n,3`) on `BACKCOLOR &HFF3C3C3C`
(60). Values that tell the roundings apart:

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
| R4 | a subprogram (`XSUBSCREEN`) whose `BACKCOLOR` is `&H80FF0000`, over a main screen of known color | compare | |

### Batch C: sprite and text sampling (screenshots)

1. An 8x8 checkerboard sprite (1-pixel squares) at `SPROT` 30, 45 and 90.5, and
   at `SPSCALE 1.5,1.5` with `SPROT 10`. This is where rotated edges and ties
   show.
2. The same under `XSCREEN 640,360,2` (frame 1280x720, so the screenshot is
   still 1:1), plus `LCLIP 0,101,101,201,201`, odd corners at sample 2.
3. Text screen 0 with a user character of alternating 1-pixel columns at
   `TSCALE 0,1.5,1.5` and `2.5`, next to a sprite of the same image at the same
   scale. Tells whether text ties go left like sprites.
4. Z order: sprites 1 and 2 overlapping at equal Z; text screen 0 and a sprite
   at equal Z on the same layer; each in a different color.
5. `INPUT` waiting on the console with the cursor on a known cell, in a
   non-white `COLOR` (R9).

### Batch D: layers and projection (screenshots)

1. `LMATRIX 0,hx,hy,x,y,sx,sy,rot` with sx != sy and rot 30. This shows the
   rotation direction and whether scale comes before rotation for layers.
2. 4x4 matrices:
   - with a nonzero Z row (`M[2]`, `M[6]`, `M[10]`, `M[14]`) that puts Z/W
     outside -1..1 for part of the layer. Does that part vanish (depth
     clipping)?
   - with a matrix that puts part of the layer behind (W < 0): only the front
     part shows.
3. A 4x4 matrix that magnifies a 1-pixel checkerboard 8 times. Nearest or
   smoothed?
4. `LCLIP` on a 4x4 layer. Is the rectangle applied in screen pixels after
   projection (the emulator's way), or in layer coordinates before?
5. `LAYER 1,1,&HFF8080FF` with `LFILTER 1,5,120,0,0` (color conversion).
   Filtering before or after the layer color gives different hues.

### Batch E: filters (screenshots)

1. `XSCREEN 640,360,2` with `LFILTER 0,1,8`: are blocks 8 or 16 frame pixels?
2. Blur of a single dot and a 1-pixel line at amounts 16, 32, 64, 128 and 384,
   one part each (NEON_DRIFTWAY uses 384). Then blur at `XSCREEN 640,360,2`,
   amount 8.
3. Raster type 3 and type 4 with offset 0.1 and scale 0.5 in the same entry, on
   a picture with a marked origin: shows the order and the Y sign.
4. Raster with `A[240,2]` against `A[240,4]` holding the same offset/scale:
   tells the meaning of the two-value form.
5. Color conversion `LFILTER 0,5,30,40,-20` over a 16-color palette strip, and
   the same on a composed layer whose sprites are partly transparent
   (premultiplied input).

### Batch F: timing and state (automatic, console)

1. `FADE &HFFFFFFFF,4`, then six times `?HEX$(FADE(),8):VSYNC 1`. Shows on which
   frame the first step lands.
2. `ACLS`, then `?LFILTER(0)` after `LFILTER 0,1,8`.
3. `SPANIM 0,"Z",...` and `SPANIM 0,"C",...` with keys that give fractional
   values: print `SPOFS 0 OUT X,Y,Z` and `SPCOLOR(0)` each frame. The same with
   `TANIM` and `TOFS`/`TCOLOR`.
4. After `TSCREEN`, print `TLAYER(0)`, `TBLEND(0)`, `TPAGE 0 OUT ...` and TVAR
   values that were set before.

### Batch G: display scaling (screenshots of the whole display)

`XSCREEN 400,240,1,M` for M = 0, 1, 2 with a 1-pixel checkerboard and a diagonal
line. Then `XSCREEN 400,240,1,2,1.0`. Capture the 1280x720 output (Ryujinx's own
scaling must be off, or the result says nothing), and compare with the emulator
window's `SB_UI_BENCH shot=`. The batch asked whether mode 1 needed its own
resampling code; it uses `SmartFrame` and `smart_scale`.

## History

There were 31 open items, R1 to R31. They closed in this order:

- TEST47 (batches A and F) settled R7, R20, R22 and R24-R31, apart from smaller
  points left U.
- TEST48 settled most of those smaller points: the shallow GLINE rule and lines
  from off the page, GTRI's edges, GSAVE and GLOAD sizes, single-precision
  rotation and scale, GBOX corner order, near-full arcs and TFUNC after TSCREEN.
- TEST49 (batch B) settled R1-R4 and R21, apart from the smaller points left U
  in the tables.
- TEST50 (batch C) settled R5, R6, R8, R9 and R10, with the tie rule and the
  cursor's rate left U at the time. TEST63 settled the tie rule at multiples of
  90 degrees.
- TEST53, TEST54 and TEST55 (batches D, E and G) settled R11-R19 and R23.
