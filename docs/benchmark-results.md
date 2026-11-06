# Benchmark results

[benchmarking.md](benchmarking.md) says how to take these numbers. All of them
date from 2026-11-05. Each set says what it ran on and which commits it
compares. Compare rows within one table, as frame counts differ between sets.

## The CPU path on weak machines

Where the GPU path is not available, as on old PCs or a handheld with a small
Arm core, the CPU draws everything. Such a core runs instructions in order,
two at most at a time, and floating point, cache misses and missed branches
cost it far more than they cost a desktop. So instruction, cache-miss and
branch-miss counts say more here than a desktop's times; a wide core hides
much of a drop in those counts.

As a rule of thumb, not measured: a 1.5 GHz Cortex-A35 at about 0.8
instructions a cycle has about 20 million instructions for a frame, to draw,
run the program and make the sound.

## What costs what

This list holds as of `8a7d9e7`, the commit that last revised it.

- **Blending** is a pixel's main cost where anything is translucent, at 15 to
  40 instructions: the sums round once, over 255 or its powers. The loops are
  scalar sums in lanes of a 64-bit word. Two forms written for the compiler to
  vectorize gained nothing on baseline x86-64 (+36% and about 0%), so they
  were dropped; NEON was not tried.
- **Placing** a turned pixel takes about 15 integer instructions, in fixed
  point, with the exact sum where it might round the other way
  (`draw_texels`).
- **Through a 4x4 matrix**, one division in 16 pixels: what a pixel reads
  steps along the curve in fixed point, with its second difference, and only a
  pixel too near a whole number gets a sum of its own (`Along`).
- **Composed layers** take a clear and a pass over only the box of what they
  hold, unless a filter or a 4x4 matrix is at work. A full-frame sprite still
  means 3.5 MB each way at 1280x720. A color on a layer takes three
  multiplications a channel unless the pixel is like the last, as it mostly
  is.
- **Text** keeps each screen's characters from frame to frame, up to 4 MiB a
  screen, for as long as the font page, the TPAGE page and its GSAMPLE,
  TCOLOR, TBLEND and the size hold. A new color on a character still takes
  making it.
- **A collision search** reads an array of boxes, brought up to date for the
  sprites changed since the last search.
- **The interpreter** takes about 20 instructions an operation on numbers, in
  `crates/sb-core/src/fuse.rs`'s larger steps, with up to 18 of each step on
  choosing the step. These steps cover jumps by ON, GOSUB and RETURN, WHEN
  tests, an array indexed by an element of another (`R%[K%[I]]`), and calls
  of a DEF in the same slot with their returns: about 500 instructions for a
  call and its return. Strings, built-in calls and anything that may fail go
  one instruction at a time, as everything does whenever a debugger, TRACE or
  a breakpoint is active: about 8 times slower, at about 140 million
  instructions a second. A built-in call costs a few hundred.
- **Sound** is small: 16 voices of a bank instrument at 3.5 million
  instructions a frame, in runs of samples between envelope steps; reverb 0.4
  million more.
- **Memory:** peak RSS is about 180 MiB, 158 of it from start-up. A sprite is
  about 360 bytes, and free ones are not walked.
- **Uploading** a frame is a copy of 3.5 MB at 1280x720, 0.4 MB at 400x240.
  Its cost on a slow memory bus was not measured.

## Results

### CPU path, `7acffef` to `9f0b8de`

This set ran on one x86-64 desktop: release build, `--bench --fixed`, 200
frames, 1200 for the projects, one core, start-up included (0.22 G
instructions). Pixels are the same in each.

| Scene | Instructions before (G) | Instructions after (G) | Cycles change | Cache misses before (M) | Cache misses after (M) | Draw before (ms) | Draw after (ms) |
|---|---|---|---|---|---|---|---|
| TMULTI | 206.7 | 42.0 | -86% | 120 | 26 | 94.8 | 12.4 |
| SPANIM | 107.6 | 63.2 | -44% | 297 | 367 | 32.9 | 18.3 |
| SPROT | 90.4 | 60.2 | -39% | 437 | 400 | 27.0 | 16.3 |
| LCOMP | 141.3 | 47.6 | -61% | 66 | 62 | 34.5 | 13.3 |
| LBLUR | 49.7 | 46.8 | -7% | 56 | 62 | 11.7 | 10.9 |
| LMATRIX | 45.6 | 44.2 | -3% | 635 | 622 | 13.1 | 12.6 |
| GAME | 13.0 | 10.5 | -12% | 26 | 24 | 1.7 | 1.3 |
| TPRINT | 19.9 | 13.3 | -23% | 44 | 34 | 6.0 | 4.5 |
| ELE2020 | 224.8 | 165.6 | -26% | 296 | 307 | 11.5 | 8.7 |
| SOLIDGUNNER_V | 37.6 | 26.5 | -23% | 204 | 189 | 1.1 | 0.7 |
| DVD_BOUNCE | 2.8 | 2.6 | -10% | 25 | 8 | 0.11 | 0.10 |
| VINT | 8.4 | 7.2 | -2% | 3.3 | 1.1 |  |  |

TPERF whole, 10500 frames: instructions -40%, cache misses -28%. The MAXSP
score went from 838 to 1489 sprites in one run each, on a machine doing other
work.

### CPU path, `28cf171` to `8bbac7a`

This set ran on the same desktop as the one above, with the same build,
options and frame counts. Pixels and sound are the same to the bit:
`SB_BENCH_SOUND` adds a checksum of the sound to a fixed run.

The after runs appear to leave out the interpreter commits `4b18ad3` to
`2a8b58e`, which the history places between these two commits: GAME ends at
10.4 G here against 7.64 G in the interpreter table.

| Scene | Instructions before (G) | Instructions after (G) | Cycles change | Cache misses before (M) | Cache misses after (M) | Branch misses before (M) | Branch misses after (M) |
|---|---|---|---|---|---|---|---|
| LMATRIX | 44.2 | 32.1 | -33% | 623 | 505 | 16.1 | 17.4 |
| LRASTER | 25.3 | 19.6 | -32% | 74 | 65 | 7.8 | 7.3 |
| TPRINT | 13.3 | 8.8 | -29% | 34 | 14 | 39.9 | 31.2 |
| SPHIT | 13.1 | 11.5 | -21% | 34 | 17 | 23.3 | 20.1 |
| SND16 | 1.15 | 0.95 | -9% | 0.7 | 0.7 | 1.4 | 1.4 |
| SPMOVE | 18.7 | 18.3 | -2% | 33 | 41 | 6.3 | 5.8 |
| GAME | 10.5 | 10.4 | -3% | 22 | 22 | 6.9 | 6.6 |
| ELE2020 | 165.6 | 164.1 | -3% | 354 | 315 | 107.7 | 106.0 |
| SOLIDGUNNER_V | 26.5 | 26.2 | -4% | 181 | 188 | 17.2 | 16.4 |
| NEON_DRIFTWAY | 297.0 | 293.4 | +0.5% | 387 | 429 | 218.1 | 218.3 |

Seven composed layers of eight 32x32 sprites each, 100 frames: instructions
-76%, draw 1.16 ms to 0.30 ms. TPERF whole: instructions -4.7%, cycles -5%,
cache misses -8%.

The code of `9f0b8de` and `28cf171` is the same, so the last table's after
column and this one's before column show how much cache misses move between
runs: ELE2020 has 307 M in one and 354 M in the other.

### Interpreter, `ac43b4e` to `2a8b58e`

This set ran on the same desktop: 100 to 300 frames, one core, start-up
included. What the two commits run and their checksums are the same.

| Scene | Instructions before, release (G) | Instructions after, release (G) | Cycles change, release | Branch misses change | Instructions after, dist (G) | Cycles change, dist |
|---|---|---|---|---|---|---|
| VINT | 3.71 | 1.44 | -72% | 0% | 1.40 | -72% |
| VREAL | 3.18 | 1.52 | -57% | -1% | 1.48 | -59% |
| VARRAY | 5.15 | 2.39 | -66% | 0% | 2.35 | -64% |
| VDEF | 2.17 | 1.52 | -37% | 0% | 1.44 | -42% |
| VSTR | 1.64 | 1.42 | -19% | 0% | 1.36 | -25% |
| GAME | 10.50 | 7.64 | -39% | -26% | 7.21 | -46% |
| S51_MEDAL4 | 38.88 | 31.96 | -37% | -5% | 29.55 | -43% |
| FINITI | 17.52 | 14.78 | -17% | -24% | 14.22 | -28% |
| NEON_DRIFTWAY | 2.22 | 1.58 | -26% | -8% | 1.52 | -29% |

Programs that spend their time drawing change only by the interpreter's share
of their work: ELE2020 and BASICDEMO do not change, RYUSI and SOLIDGUNNER_V
change by 1 to 2% of instructions.

### `dist` against `release`, `ac43b4e`

This set ran on one x86-64 desktop and compares the two profiles of one
commit: cycles TPRINT -16%, the BASIC scenes -8 to -10%, LCOMP -6.5%, ELE2020
-5.5%. The check binary went from 6.02 to 5.07 MB.

### Resizing a window, recorded in `3307899`

This set ran on an RTX 3060 Ti with `opengl`, under headless Mutter as in
[benchmarking.md](benchmarking.md), with an interactive resize. It compares no
commits. The table gives the cost per size change, at 2000 and 3200 wide.

| Where | 2000 wide | 3200 wide | What it is |
|---|---|---|---|
| `events` | 9.5 ms | 19.5 ms | libdecor's GTK plugin draws the decorations again for each configure, with a shadow buffer the size of the window, inside SDL's event pump |
| `screen` | 4 ms | 8 ms | the driver, in the first GL call after the size changed: a bare `glFlush` there takes all of the growth and leaves 0.2 ms |
| `present` | 5.6 ms | 10 ms | the driver and compositor |

The emulator makes no texture per size. Mutter sends up to 60 configures a
second and all that are queued are dispatched in one pump. Once the cost of
one nears 16.6 ms, as it does with big windows or a busy CPU, the backlog
feeds itself and single pumps take seconds, with the program and its sound on
the same thread.

Turning vsync off during the drag, or skipping drawings after a long pump,
made no difference under a fixed load. Only a cheaper configure did:
libdecor's cairo plugin, by `LIBDECOR_PLUGIN_DIR`, gave 55 frames a second
against 49, but it looks different. SDL can throttle configures during an
interactive resize when built against a libdecor with the resizing state
(0.3).

## Tried and left out

Each of these was measured and not kept.

- **Dropping the memos on missed branches.** SPANIM's and LCOMP's branch
  misses come from working out a paint or a blend only when the pixel differs
  from the last. Without the memos, SPANIM has branch misses -47% and
  instructions +28%, and LCOMP has -66% and +89%, with cycles +126%. An
  in-order core spends few cycles on a miss and at least one on each
  instruction, so the memos stay (inferred for such a core).
- **Blending plain layers with SSE2**, in lanes with the same sums:
  instructions -3 to -8% on layer scenes, cycles -1 to -8%, branch misses -9
  to -20%. It would be the first `unsafe` in `crates/sb-runtime/src`, for a
  gain on x86 alone. NEON was not tried, as no Arm target was available to
  build for on 2026-11-05, when this was measured. The cross-build in
  [building-handheld.md](building-handheld.md), added in `bc19803`, provides
  one.
- **Comparing scenes to skip a frame:** the emulator draws a frame only after
  a command changed what shows. Of about 1100 frames each in five projects, 0
  to 3 had a scene, pages and cells like the last, so a hash would cost every
  frame for nothing.
