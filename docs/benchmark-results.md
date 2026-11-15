# Benchmark results

[benchmarking.md](benchmarking.md) says how to take these numbers. They date
from 2026-11-05, except the handheld sets, from 2026-11-15 and 2026-11-16.
Each set says what it ran on and which commits it compares. Compare rows
within one table, as frame counts differ between sets.

## The CPU path on weak machines

Where the GPU path is not available, as on old PCs or a handheld with a small
Arm core, the CPU draws everything. Such a core runs instructions in order,
two at most at a time, and floating point, cache misses and missed branches
cost it far more than they cost a desktop. So instruction, cache-miss and
branch-miss counts say more here than a desktop's times; a wide core hides
much of a drop in those counts.

As a rule of thumb: an RK3326's Cortex-A35, which runs at 1.3 GHz under
load, does 0.6 to 0.8 instructions a cycle on the interpreter (see
[the handheld set](#handheld-cpu-from-cc49c48)), so a
frame has room for 13 to 17 million instructions, to run the program and make
the sound, with the GPU drawing.

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

### Frames during a drag, `5e076c2` against the standby thread

This set ran on the same machine and setup, dragging a 2500x1400 window for
900 timed trips with a program that plays a looped MML tune and moves sprites.
`frames` counts program frames a second, `MAINCNT` agreed in each run. The
silence is the sound device's, with the dummy driver; about 130 ms of it comes
from start-up, as a run with no drag shows.

| Load | Build | Frames a second | Silent | Longest trip |
|---|---|---|---|---|
| None | `5e076c2` | 59.97 | 157 ms | 52 ms |
| None | standby | 59.96 | 174 ms | 590 ms |
| 8 busy cores | `5e076c2` | 55.6, 43.2 | 3.6 s, 12.3 s | 0.48 s, 1.72 s |
| 8 busy cores | standby | 59.94, 59.95 | 0.42 s, 0.31 s | 1.16 s, 0.77 s |

The longest trip round the loop varies from run to run with Mutter, and the
standby thread does not shorten it: it keeps the program and its sound going
through it. `at=200:sleep:5000` with no window gives 40.2 frames a second and
4.9 s of silence before, and 60.0 with none after.

### Handheld CPU, from `cc49c48`

This set compares an RK3326 handheld (four Cortex-A35 cores, 1 GB, aarch64
Linux 4.4, glibc 2.38) with an x86-64 desktop (Ryzen 7 5800X). The device's
governor was `ondemand`, between 1.008 and 1.296 GHz, and read 1.296 GHz on
all cores under load. The emulator's own handheld mode ran on the device at
the same time, taking about 40% of one core. The device's kernel has no
performance counters, so the device gives times only; instruction counts come
from qemu-aarch64 with its Cortex-A35 model, one instruction a block, counted
exactly from the `exec` log.

Each scene of TPERF ran alone (`#ONLY`), with `--bench --fixed 80 --log`, the
`dist` build cross-built by `tools/build_handheld.py`. A scene's time is the
median of `run_us` over frames 10 to 69, the best of three runs on the device
and of three to five on the desktop, pinned to one core. `run_us` is the
program's part of the frame with its sound, which is silent here. The
desktop's times swing with its load; its instruction counts do not. The
device's first two sessions ran unpinned; the later ones, for the
[layout set](#code-layout-on-the-handheld-44fd91e-to-314459c) and the
[chains](#three-address-chains-from-018017d), ran with the system's front end
stopped and the benchmark pinned to one core.

The device ran `44fd91e`, an older `main`. The interpreter changed little up
to `cc49c48`: up to 5% fewer instructions on loops of DEF calls and GOSUBs,
by the measurements of those commits. Its checksums at 80 frames match the
desktop's for every scene.

| Scene | Desktop (ms) | Device (ms) | Device / desktop |
|---|---|---|---|
| VINT | 0.47 | 10.59 | 22 |
| VREAL | 0.64 | 14.12 | 22 |
| VSTR | 0.63 | 17.08 | 27 |
| VARRAY | 0.98 | 22.04 | 22 |
| VDEF | 0.40 | 8.08 | 20 |
| VVAR | 1.75 | 39.88 | 23 |
| SPMOVE | 0.42 | 11.56 | 27 |
| SPROT | 0.29 | 6.93 | 24 |
| SPCHURN | 1.34 | 30.72 | 23 |
| SPHIT | 1.76 | 25.93 | 15 |
| SPANIM | 0.31 | 8.09 | 26 |
| TPRINT | 0.29 | 3.53 | 12 |
| TPUT | 0.70 | 17.38 | 25 |
| TSCROLL | 0.18 | 4.33 | 24 |
| TMULTI | 0.03 | 0.75 | 26 |
| GFILL | 0.46 | 12.52 | 27 |
| GALPHA | 4.03 | 60.00 | 15 |
| GLINE | 0.81 | 18.29 | 23 |
| GCIRCLE | 1.65 | 28.41 | 17 |
| GTRI | 1.34 | 22.39 | 17 |
| GPUTCHR | 0.49 | 11.96 | 25 |
| GCOPY | 0.80 | 59.35 | 74 |
| GLOAD | 0.06 | 2.64 | 45 |
| GUPLOAD | 0.01 | 0.22 | 37 |
| GARRAY | 0.50 | 12.87 | 26 |
| LCOMP | 0.04 | 0.89 | 24 |
| LBLUR | 0.02 | 0.53 | 25 |
| LMOSAIC | 0.02 | 0.56 | 28 |
| LCOLOR | 0.03 | 0.63 | 23 |
| LMATRIX | 0.03 | 0.66 | 23 |
| LRASTER | 0.06 | 1.54 | 26 |
| GAME | 0.44 | 9.29 | 21 |

The device takes 20 to 27 times as long on nearly every scene, about 3.6
times for the clock and 6 to 7 for instructions a cycle. No scene of the
interpreter or the sprite and text commands stands out, so there is no single
slow spot: everything on the CPU pays the same.

Instructions a cycle on the device, from qemu's count of a loop iteration
and the scene's time: VINT 0.77, VREAL 0.67, VARRAY 0.70, VDEF 0.69, VSTR
0.57. The desktop runs the same code at 2.4 (VINT) to 3.4 (VVAR).

Where the interpreter's instructions go on aarch64, from qemu's trace of one
iteration:

- Choosing a fused step takes 21 instructions: the bounds and budget checks,
  a jump table and one indirect branch. `pc`, the budget left and the step
  table's address live on the stack, three stores and three loads a step.
  All steps share the one indirect branch, which a core that predicts it
  by its last target mostly misses in a loop of different steps.
- Reading a variable takes about 10: the table (globals and constants, or
  the DEF's locals), its length for the bounds check, the address, the tag.
- An operator in a fused step switches on the operator again: a second jump
  table and indirect branch inside the step.
- Pushing and popping go through the stack's length and capacity in memory.
- VINT's loop runs 8 steps and 583 instructions an iteration; an empty FOR
  2 steps and 181. A DEF call and its return take about 900, a builtin call
  outside the fused steps about 440, a `VAR("name")` about 3000, as it
  makes four `String`s and compares the name with each global's.

The scenes far above the others, GCOPY, GLOAD and GUPLOAD, draw on GRP
pages or do almost nothing. [GRP pages and the caches](#grp-pages-and-the-caches-from-da44ce5)
takes them apart: GCOPY ran 5 to 7 cycles an instruction, which points
at memory, and GLOAD moves more data than the L2 holds.

Changes, x86-64 instructions a frame (program and CPU drawing, frames 3 to
83) of `cc49c48` against the branch, same checksums; scenes not listed moved
by under 0.5%:

| Scene | `cc49c48` (M) | Branch (M) | Change |
|---|---|---|---|
| VINT | 11.45 | 10.57 | -7.7% |
| VREAL | 12.65 | 8.95 | -29.2% |
| VSTR | 11.54 | 10.75 | -6.9% |
| VARRAY | 21.64 | 20.80 | -3.9% |
| VDEF | 7.98 | 7.66 | -4.0% |
| VVAR | 29.69 | 29.28 | -1.4% |
| GCIRCLE | 37.05 | 18.64 | -49.7% |
| GCOPY | 21.52 | 21.67 | +0.7% |

The same on aarch64, qemu's exact count over frames 3 to 5 (program and CPU
drawing), and the device's times. The device ran `44fd91e` and `314459c`,
which take in the branch and the error-number work around it, in one
session, with the system's front end running (about 6% of a core). Every
checksum is the same.

| Scene | `cc49c48` (M) | Branch (M) | Change | Device, `44fd91e` (ms) | Device, `314459c` (ms) | Change |
|---|---|---|---|---|---|---|
| VINT | 11.02 | 10.35 | -6.1% | 10.55 | 9.84 | -6.8% |
| VREAL | 12.89 | 9.18 | -28.8% | 14.14 | 9.01 | -36.3% |
| VSTR | 14.13 | 13.29 | -6.0% | 17.31 | 17.03 | -1.6% |
| VARRAY | 20.40 | 20.39 | -0.1% | 22.16 | 22.46 | +1.4% |
| VDEF | 7.74 | 7.73 | -0.2% | 8.06 | 8.23 | +2.1% |
| GCIRCLE | 32.31 | 16.86 | -47.8% | 28.09 | 14.16 | -49.6% |
| GAME | 20.45 | 20.59 | +0.7% | 9.37 | 9.14 | -2.4% |

GAME's +0.7% and GCOPY's +0.7% on x86-64 come from LLVM laying out
`run_fused`'s registers anew; neither scene's own steps changed. The other
scenes moved by under 7% on the device, except three sprite scenes, which
came out slower: SPMOVE +5.8%, SPROT +8.4%, SPHIT +8.5%. Run again with the
front end stopped and pinned, in two rounds of five runs alternating the
builds, they came to +2.4%, +7.5% and +7.4%. That is code layout, not work, as the
[layout set](#code-layout-on-the-handheld-44fd91e-to-314459c) shows.

What changed:

- `59e363e`: a circle that lies on the page no longer sorts its points to
  drop the repeats, which only occur on the axes and the diagonal. GCIRCLE
  -50%; on aarch64, 32.3 to 16.9 million instructions a frame.
- `c4eb331`: `run_code` asked `f.op != FastOp::Slow` before each single step,
  a call of the derived `PartialEq`. VSTR's loop -1.7% on aarch64.
- `59ab61d`: pure numeric builtins (`LEN`, `ASC`, `INT`, `FLOOR`, `ABS`,
  `SQR`, `SIN`, `MIN`, `MAX` and others that only read their arguments) run
  in a fused step. One that fails is called again by `step`, which fails as
  before. `A=ABS(I)` 574 to 384 aarch64 instructions, VREAL's loop 1359 to
  986. ASC no longer copies its string.
- `a9f9cdc`: an integer FOR's `NEXT` reads each value once and tests on the
  integers, when TO's and STEP's places are none of the counter's and the
  FOR's own, which the fuser notes. An empty FOR 181 to 133, VINT's loop 583
  to 543, a GOSUB loop 346 to 299.

Build options, aarch64 instructions an iteration of seven loops (empty FOR,
VINT, VREAL, VARRAY, VDEF, a builtin loop, VSTR) and `run_fast`'s size,
qemu counts only:

| Options | Empty | VINT | VREAL | VARRAY | VDEF | Builtins | VSTR | `run_fast` |
|---|---|---|---|---|---|---|---|---|
| `dist` as it is (`opt-level=3`) | 133 | 543 | 947 | 1986 | 1790 | 919 | 3955 | 28.9 KB |
| `-C target-cpu=cortex-a35` | 135 | 546 | 947 | 1992 | 1794 | 927 | 4034 | 29.5 KB |
| the same, `-aes,-sha2,-crc` | 135 | 546 | 947 | 1992 | 1794 | 927 | 4034 | 29.5 KB |
| `-C opt-level=2` | 139 | 555 | 947 | 2000 | 1738 | 913 | 3959 | 25.2 KB |
| `-C opt-level=s` | 152 | 597 | 1012 | 2170 | 2033 | 1028 | 4404 | 24.8 KB |

`-C target-cpu=cortex-a35` adds no instruction a core of ARMv8.0 lacks
except the CRC and crypto ones, which nothing here asks for; with
`-C target-feature=-aes,-sha2,-crc` it only tunes, for the A35's pipeline,
and runs on any aarch64. What tuning gains is the order of instructions on an
in-order core, which qemu does not time. On the device, each build ran every
scene once, best of three, against the plain `dist` build of `314459c` in the
same session. Leaving out GFILL, GCOPY, GLOAD and GUPLOAD, whose times fall
in two clusters ([benchmarking.md](benchmarking.md#measuring-on-a-handheld)),
`cortex-a35` with `-aes,-sha2,-crc` was 1.4% slower by geometric mean, with
5 scenes faster by more than 1% and 18 slower (GPUTCHR +9.7%, GALPHA +4.9%),
and `opt-level=2` 0.1% faster, with 11 faster and 15 slower, by swings of 4
to 7% either way that look like layout. Neither is adopted. Profile-guided
optimization needs `llvm-profdata`, which the toolchain here lacks (rustup's `llvm-tools` component has it); the aarch64 standard library
ships the profiler runtime, so an instrumented build can run under qemu.

Larger changes, not made, by what they would save against what they cost:

- **Padding a GRP page's rows**, to a stride that is not a multiple of the
  caches' way size (2048 plus 16 pixels, say). A cache simulation cuts
  GCOPY's misses to memory 2 to 5.6 times and leaves GLOAD's as they are;
  what that saves on the device is not measured yet
  ([GRP pages and the caches](#grp-pages-and-the-caches-from-da44ce5)).
  Every reader of a page's pixels assumes the 2048 stride: `index`, the
  raster, GSAVE and GLOAD, GARRAY, and the GPU path's uploads in `sb-sdl`.
- **A cache for `VAR("name")`**, keyed by the string and the running DEF,
  emptied when the program changes. A look-up costs about 3000 instructions
  now, mostly in making `String`s; VVAR would fall from 40 ms toward 5 ms on
  the device. Few programs use `VAR()` in a loop.
- **Keeping the fused loop's state in registers**: `pc`, the budget and the
  stack's pointer and length. Moving the stack out of `self` into a local
  `Vec` for the loop, and fewer values live across the `match`, might save 6
  to 10 of the 21 instructions a step takes to be chosen. LLVM decides what
  it spills in a function of 29 KB, so the gain can only be measured, and
  can move from build to build.
- **A register-based form of the fused code**, three-address steps that name
  their operands and their result, with the operator in the step's own kind
  instead of a second switch: about half the steps and indirect branches an
  iteration of VINT, perhaps 30 to 40% of the interpreter's time. Made as
  the [chains](#three-address-chains-from-018017d) below, it took VINT's loop from 8 entries to 2 but its
  instructions down by 12%, as reading and writing a variable, not choosing
  an entry, is most of what a step costs.
- **Values of 8 bytes** (NaN-boxing, or a separate tag array): halves the
  stack's and the variables' memory traffic. It changes every use of `Value`
  and needs `unsafe` for the boxing.
- **A JIT** for the fused forms: the largest gain, perhaps 5 to 10 times on
  loops, and the largest risk: writable code memory for untrusted programs,
  per CPU architecture.

### Code layout on the handheld, `44fd91e` to `314459c`

This set ran on the handheld of the set above, with the front end stopped
and the benchmark pinned, on 16 builds of the commits from `44fd91e` to
`314459c`, with the method of that set. Instruction counts come from qemu's Cortex-A35 model over frames 3
to 5, program and CPU drawing. It explains why SPMOVE, SPROT and SPHIT came
out slower in the handheld set: where the code lies, not what it runs.

| Scene | qemu (M) | Of which drawing (M) | Device, best of three (ms) |
|---|---|---|---|
| SPMOVE | 219.79 to 220.26 | 196.43 | 11.42 to 11.94 |
| SPROT | 659.59 to 660.07 | 644.48 | 6.94 to 7.67 |
| SPHIT | 154.22 to 154.71 | 80.14 | 25.73 to 27.96 |

The drawing's count is the same in each of the four builds whose count was
split by function, and `run_us` leaves drawing out. So the program's part
moves by under 0.5 million instructions, at most 3% of it, while the
device's times move by 4 to 11%. SPHIT's hot functions, `Sprites::next_hit` and
`collision::sweep`, run the same number of instructions in each of those
builds. The times move up and down along the history with no lasting step.
SPHIT's last step, to 27.96 ms, came with `d74d9ba`, which changes only
LOADV code that the scene never runs.

Functions are aligned to 4 bytes, so any commit moves the code after it.
Builds whose hot functions start at the same place in a 64-byte line take
alike: with `next_hit` and `sweep` at bytes 44 and 16 of a line, SPHIT took
25.97 to 26.43 ms in four builds; at 28 and 0, 25.82 and 25.87; at 52 and
24, 26.62 and 26.95.

A simulated instruction cache of 32 KB, 2 ways and 64-byte lines, run over
qemu's trace of three frames, misses 85 thousand times in SPMOVE at
`44fd91e` and 168 thousand at `314459c`, and 26 and 127 thousand times in
SPROT. At 10 to 15 cycles a miss, that accounts for most of those two
scenes' change. SPHIT's misses grow by only 12 thousand a frame; its extra
time comes to about 8 to 9 cycles on each pass of its scan loop, the cost of
one missed branch. The device's kernel has no performance counters to
confirm it.

One change in the range is real work: SPROT's program part gained about
1.5% instructions in `44fd91e..3ff6177`, which adds a check of the sprite
number before each SPOFS, SPROT and SPSCALE. A fast path for that check is
under [Tried and left out](#tried-and-left-out).

### Three-address chains, from `018017d`

This set counts instructions, and ends with the handheld's times. Counts
come from x86-64 with `perf stat -e instructions:u` on the `release` build,
and aarch64 with qemu's Cortex-A35 model on the `dist` build cross-built by
`tools/build_handheld.py`, one instruction a block, counted from the `exec`
log under `setarch -R`. Without `setarch -R`
the address space moves between runs, and a loop's count with it by up to
10%. Scenes are TPERF's, run alone with `#ONLY`: x86-64 over frames 3 to 83,
aarch64 over frames 3 to 6. Loops are small programs run 3200 and 6400 times
round, the difference over 3200. Every TPERF checksum is the same before and
after, `e8e5f6dc467db861` at 400 frames and `a6e1ed07824b1ad8` at 3600, on
both machines.

The loops:

| Loop | What it does |
|---|---|
| Empty | `FOR I=1 TO N:NEXT` |
| Sprites | 64 sprites' `X#[J]`, `Y#[J]` moved by `DX#[J]`, `DY#[J]` and bounced off the edges with `IF X#[J]<0 \|\| X#[J]>=400 THEN DX#[J]=-DX#[J]` |
| Counters | integer counters and comparisons in a DEF: `C=C+1`, `IF C>=60 THEN C=0:T=T+1`, `IF S<100 THEN S=S+2 ELSE S=S-100`, `L=L+(I AND 3)` |
| IF chain | `K=I MOD 5` and an `IF`/`ELSEIF` chain on it |
| DEF a sprite | a DEF called for each of 64 sprites, which moves and wraps one |
| Reals | `X#=X#+DX#*T#`, `Y#=Y#+DY#*T#`, `IF X#>100 THEN X#=X#-100`, `DY#=DY#+0.01` |
| GOSUB | `GOSUB @S` to `A=A+I:RETURN` |
| Array | `J=I AND 255:M%[J]=M%[J]+1:C%[J]=M%[(J+1) AND 255]` |
| VINT, VARRAY, VREAL | the loop bodies of those scenes, in a DEF |

What the fused code ran before, from a count of each entry over 80 frames:
VINT's loop took 8 entries an iteration (the NEXT, the jump back, and 6 for
its two statements), VREAL's 11, VARRAY's 28. Many entries only pushed a
value or swapped two, a third of VARRAY's and an eighth of VINT's, because
the compiler pushes the right side of `X#[J]<0` or `(A%+I*3) AND 1023`
first. Rebuilding each statement's expression from the stack code, with a
step for each operator and a step counted as half an entry, chains would
save 30% of VINT's entries, 40% of VREAL's, 58% of VARRAY's, 22% of GAME's,
13% of VSTR's and none of VDEF's, whose entries go to its DEF calls.

What was made:

- `0a28023`: a chain entry stands for whole statements. Its steps
  (`fuse::Micro`) read variables and constants where they are, keep what
  they make in eight hidden variables after the constants, and write the
  result straight to its variable or element, or jump on a comparison. Each
  common operator is a step kind of its own. A statement's steps write
  nothing the program sees before its last, and a `Commit` step between
  statements says where `code` goes on from. A step that cannot do its work
  leaves the statement to `step` from its start. VINT's loop now runs 2
  entries an iteration, the NEXT and one chain for both statements and the
  jump back; VREAL's and VARRAY's 2 as well.
- `39969a0`: `A=B+C`, `-` and `*`, and `IF A<B THEN` with the six
  comparisons, which stay single entries, have entry kinds of their own
  instead of a second switch on the operator.
- `63c1a38`: `Load, Load, Swap`, two arguments put back in order, is one
  `Push2`.

aarch64 instructions an iteration:

| Loop | `018017d` | Branch | Change |
|---|---|---|---|
| Empty | 132 | 136 | +3.0% |
| Sprites | 2130 | 1763 | -17.2% |
| Counters | 604 | 552 | -8.6% |
| IF chain | 505 | 477 | -5.6% |
| DEF a sprite | 1123 | 1090 | -3.0% |
| Reals | 603 | 589 | -2.3% |
| GOSUB | 298 | 284 | -4.7% |
| Array | 853 | 733 | -14.0% |
| VINT's loop | 537 | 471 | -12.3% |
| VARRAY's loop | 1984 | 1342 | -32.4% |
| VREAL's loop | 965 | 808 | -16.2% |

aarch64 instructions a frame, in millions:

| Scene | `018017d` | Branch | Change |
|---|---|---|---|
| VINT | 10.15 | 8.95 | -11.8% |
| VREAL | 8.86 | 7.45 | -15.9% |
| VSTR | 13.14 | 12.85 | -2.2% |
| VARRAY | 19.96 | 13.55 | -32.1% |
| VDEF | 7.31 | 7.00 | -4.3% |
| GAME | 20.56 | 20.21 | -1.7% |

x86-64 instructions a frame, in millions, and an iteration:

| Scene or loop | `018017d` | Branch | Change |
|---|---|---|---|
| Empty | 153 | 152 | -0.5% |
| Sprites | 2285 | 1913 | -16.3% |
| Counters | 650 | 596 | -8.4% |
| IF chain | 544 | 514 | -5.3% |
| DEF a sprite | 1244 | 1219 | -2.0% |
| Reals | 650 | 658 | +1.3% |
| GOSUB | 318 | 308 | -3.3% |
| Array | 919 | 792 | -13.8% |
| VINT's loop | 584 | 508 | -13.0% |
| VARRAY's loop | 2159 | 1443 | -33.2% |
| VREAL's loop | 1033 | 872 | -15.7% |
| VINT | 10.81 | 9.44 | -12.7% |
| VREAL | 9.12 | 7.66 | -16.0% |
| VSTR | 11.64 | 11.49 | -1.3% |
| VARRAY | 21.35 | 14.19 | -33.5% |
| VDEF | 7.82 | 7.66 | -2.1% |
| GAME | 28.72 | 28.52 | -0.7% |

The empty loop runs the same entries as before; it moves because LLVM lays
out `run_fused`'s registers anew, which costs the NEXT entry 4 aarch64
instructions more. The scalar reals' statements have two operators each,
which chains leave to entries; on x86-64 the new layout costs them more than
the operators' own entries save. GAME runs half the entries it did, but the interpreter
is about a quarter of its frame, by its entries' counts, and most of its
statements read one element, which an entry did in one already. VSTR's time
goes to strings, which chains leave alone.

Where an iteration's instructions go now, on aarch64: choosing an entry
takes 17 to 20, starting a chain 14 more, choosing a chain's step 14, and
each variable or hidden variable read about 13 (the table, its length, the
address, the tag) and written about 17, as a write looks at the old value's
tag in case it holds a string to drop. A step of two operands and a result
is about 60 against an entry's 70: what the chains save is the entries that
only moved values about, and the second switch. `pc`, the budget left and
the entry table's address and length stay in registers through the loop in
this build; in `018017d`'s the table's address was reloaded from the stack
for every entry. That is LLVM's choice and may change with the code around
it or the compiler's version.

`run_fast` grew from 27.2 KB to 40.9 KB of aarch64 code, past the A35's
32 KB instruction cache. A loop runs only the entries and steps it uses, and
the device's times below show no cost from the size.

The handheld of the handheld set ran `018017d` and the branch as merged,
`0dc842c`, with that set's method, quiet and pinned. Every checksum is the
same.

| Scene | `018017d` (ms) | `0dc842c` (ms) | Change |
|---|---|---|---|
| VINT | 9.96 | 8.27 | -17.0% |
| VREAL | 8.74 | 7.35 | -15.9% |
| VSTR | 15.71 | 15.33 | -2.4% |
| VARRAY | 21.71 | 13.41 | -38.2% |
| VDEF | 7.69 | 7.36 | -4.2% |
| VVAR | 41.38 | 44.04 | +6.4% |
| SPMOVE | 11.95 | 10.59 | -11.5% |
| SPROT | 7.58 | 7.35 | -3.1% |
| SPCHURN | 30.57 | 29.54 | -3.4% |
| SPHIT | 26.26 | 25.98 | -1.1% |
| SPANIM | 8.02 | 7.93 | -1.2% |
| TPRINT | 3.44 | 3.51 | +2.2% |
| TPUT | 18.34 | 17.95 | -2.2% |
| TSCROLL | 4.28 | 4.25 | -0.8% |
| TMULTI | 0.73 | 0.73 | 0.0% |
| GFILL | 12.54 | 13.81 | +10.1% |
| GALPHA | 60.35 | 60.32 | -0.1% |
| GLINE | 18.99 | 19.00 | 0.0% |
| GCIRCLE | 14.28 | 14.12 | -1.1% |
| GTRI | 21.81 | 21.91 | +0.5% |
| GPUTCHR | 12.00 | 12.19 | +1.6% |
| GCOPY | 54.95 | 54.41 | -1.0% |
| GLOAD | 2.57 | 2.57 | +0.1% |
| GUPLOAD | 0.22 | 0.21 | -7.8% |
| GARRAY | 12.64 | 10.19 | -19.4% |
| LCOMP | 0.89 | 0.86 | -3.1% |
| LBLUR | 0.53 | 0.52 | -1.9% |
| LMOSAIC | 0.49 | 0.48 | -1.6% |
| LCOLOR | 0.63 | 0.61 | -2.6% |
| LMATRIX | 0.66 | 0.63 | -4.0% |
| LRASTER | 1.45 | 1.06 | -26.9% |
| GAME | 9.16 | 8.73 | -4.7% |

The device gains as much as the aarch64 counts say or more: VINT -17.0%
against -11.8% of instructions, VREAL -15.9% against -15.9%, VARRAY -38.2%
against -32.1%, VDEF -4.2% against -4.3%, GAME -4.7% against -1.7%.
SPMOVE, GARRAY and LRASTER, whose loops work on array elements as VARRAY's
does, gain 11 to 27%. VVAR's +6.4% is within the
[noise of code layout](benchmarking.md#measuring-on-a-handheld), and GFILL's
+10.1% and GUPLOAD's -7.8% within that of their two clusters. The other
scenes move by 4% or less.

### GRP pages and the caches, from `da44ce5`

This set asks why GCOPY, GLOAD and GUPLOAD take 37 to 74 times as long on
the handheld as on the desktop when the other scenes take 20 to 27, and
whether a GRP page's row stride is the cause. A page is 2048 pixels of 4
bytes, so its rows lie 8 KB apart: one way of the Cortex-A35's 32 KB
4-way L1, and a quarter of a way of its 256 KB L2, taken here as 8-way.
The same column of successive rows falls in one L1 set, and in one of four
L2 sets. Counts come from qemu's Cortex-A35 model
over frames 3 to 6 of the `dist` build, as in the
[chains](#three-address-chains-from-018017d) set, and from `perf stat` on
x86-64 over frames 3 to 83. The device was off the network for this set:
its times below are those of the sets above, and the new build's are not
measured yet.

What each scene's program does a frame, and what it costs:

| Scene | Work a frame | Program, aarch64 (M) | Device (ms) | Cycles an instruction |
|---|---|---|---|---|
| GFILL | 2000 fills of 32x32 | about 8.0 | 12.5 to 13.8 | about 2.1 |
| GCOPY | 900 copies of 48x48, page 1 to page 0, mode 1 (clear pixels skipped) | about 11.5 | 45.6 to 59.4 | 5.1 to 6.7 |
| GLOAD | 6 loads of 400x240 from an integer array | about 0.8 | 2.09 to 2.64 | 3.4 to 4.3 |
| GUPLOAD | 9 GPSETs | under 0.1 | 0.21 | - |

The program's part is the frame's count less the CPU drawing's (about 6.6
million), split by function. The interpreter's scenes run 1.3 to 1.8
cycles an instruction on the device (VINT to VSTR). GCOPY's 4 to 5 extra
cycles an instruction are 33 to 47 ms of stalls a frame.

- **GLOAD** reads 384 KB and writes 384 KB six times a frame, 4.6 MB, at
  1.7 to 2.2 GB/s: memory bandwidth. Neither array fits in the L2, with any
  stride; the desktop keeps both in its 32 MB L3.
- **GUPLOAD** in `smilebasic-check` uploads nothing: its program part is
  6 µs on the desktop and 0.21 ms on the device. Its ratio is that of two
  tiny numbers on caches the drawing has just emptied, and says nothing
  about pages.

A standalone benchmark ran the emulator's inner loops for GCOPY, GLOAD and
GFILL, copied from `graphics.rs`, on page buffers of other strides, with
TPERF's sizes and places, and fed the same order of accesses to a cache
simulator: L1 32 KB 4-way, L2 256 KB 8-way, 64-byte lines, LRU, physically
indexed, with a pass over the 400x240 target area each frame as the CPU
drawing makes. Physical frames were taken either as contiguous (as huge
pages or an unfragmented free list give) or as scattered 4 KB frames.
Misses to memory a frame, in thousands; the L2 with 16 ways gives the same
within 2%:

| Kernel | Frames | 2048 | 2064 | 2112 | 3072 |
|---|---|---|---|---|---|
| GCOPY, `da44ce5` | contiguous | 330 | 65 | 59 | 150 |
| GCOPY, `da44ce5` | scattered | 150 | 72 | 65 | 151 |
| GCOPY, this branch | contiguous | 289 | 62 | 59 | 149 |
| GCOPY, this branch | scattered | 149 | 69 | 62 | 149 |
| GFILL | contiguous | 172 | 69 | 65 | 147 |
| GFILL | scattered | 148 | 75 | 65 | 146 |
| GLOAD | either | 86 | 86 | 86 | 86 |

Padding the rows by 16 or 64 pixels cuts GCOPY's and GFILL's misses to
memory 2 to 5.6 times; a stride of 3072 (4 KB more) aliases in the L2
as 2048 does with scattered frames. Moving the source page against the
target by 2, 4 or 6 KB changes nothing (330 to 338 thousand): the rows
that alias are those of the target page, whose 400x240 area is 384 KB and
uses 100 of the L2's 512 sets with contiguous frames, 200 with scattered
ones. GLOAD misses on every line with any stride.

The two clusters of times fit the two placements: scattered frames give
GCOPY 180 thousand fewer misses a frame than contiguous ones, and the
clusters lie about 10 ms apart, which prices a miss at about 55 ns. At that
price, padding would save GCOPY about 5 ms a frame with scattered frames
and 15 ms with contiguous ones, of 45 to 55: 10 to 27%. Neither the
placement nor the price is measured on the device yet, and translation
misses are not modeled: the target's rows lie in 240 different 4 KB pages.

So no stride was changed. The device has to show a gain of a third or more
in GCOPY or GFILL at a padded stride first, in every run.

What did change, as it costs instructions whatever the caches do:

- GCOPY copied its whole source rectangle into a new `Vec`, a `memcpy` a
  row, then matched the drawing mode again on each row. From another page
  that is not blank, with no GARRAY array on either, rows now go straight
  from page to page, and the mode is chosen once. The standalone kernel
  went from 9.0 to 3.8 million aarch64 instructions a frame; its inner loop
  stays NEON, 11 instructions for 8 pixels. A copy within one page still
  reads its source first.
- A plain GFILL on a page without a GARRAY array fills each row's columns
  directly instead of through `fill_rows`, which asked a closure for each
  row's span and matched the mode on each row: about 45 instructions a row
  before.

| Scene | aarch64, `da44ce5` (M) | aarch64, branch (M) | Change | x86-64, `da44ce5` (M) | x86-64, branch (M) | Change |
|---|---|---|---|---|---|---|
| GFILL | 14.64 | 13.10 | -10.5% | 16.94 | 14.42 | -14.8% |
| GCOPY | 18.30 | 13.26 | -27.5% | 21.19 | 17.38 | -18.0% |

These are whole frames, the CPU drawing included; GCOPY's program part
falls from about 11.5 to 6.7 million aarch64 instructions. Of the other
scenes on x86-64, GPUTCHR moves by +0.4% as LLVM inlines `put_chars`
differently, and the rest by under 0.5%. TPERF's checksums stay
`e8e5f6dc467db861` at 400 frames, `a6e1ed07824b1ad8` at 3600 and
`5d9bb17a39eefb70` at 10500.

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
- **The fused steps' variables as two fields** instead of an array of two
  tables indexed by the variable's table bit, for a select in place of a
  load: VINT's loop 583 to 580 aarch64 instructions, the others within 2%
  either way, as the tables' pointers still went to the stack (2026-11-15).
- **Chain registers in `run_fused`'s own locals** instead of hidden
  variables: as an array of `Num`, LLVM kept the numbers on the stack and
  copied them in and out of every step, a loop of scalar real updates 588 to
  701 aarch64 instructions an iteration; as an array of `Value`, 670. The
  hidden variables cost a variable's read and write but no branch on what
  kind of operand a step has (2026-11-16).
- **A chain's steps in a function of their own** (`#[inline(never)]`), to
  keep their registers out of the other entries': the call and the variables
  passed through memory took a loop of scalar real updates from 683 to 1005
  x86-64 instructions an iteration, VINT's frame from 9.9 to 13.4 million
  (2026-11-16).
- **Chains for every statement of two operators**, at a cost of 30 to start
  and 40 a step: a chain's step costs about 60 aarch64 instructions against
  70 for an entry, so two steps and the start cost more than two entries.
  With that weighting a loop of sprite updates in reals went from 2294 to
  2426 x86-64 instructions an iteration and one of scalar reals from 652 to
  683; the weighting in `fuse.rs` takes a chain only where it leaves out an
  entry (2026-11-16).
- **`-C target-cpu=cortex-a35` and `opt-level`** (see the handheld set).
  Under qemu, `cortex-a35` adds 0 to 2% instructions, `opt-level=2` moves
  loops by -3 to +4% and shrinks `run_fast` by 13%, and `opt-level=s` adds 6
  to 14%. On the device, `cortex-a35` with `-aes,-sha2,-crc` ran 1.4% slower
  by geometric mean, and `opt-level=2` 0.1% faster, within the noise of code
  layout. `dist` stays as it is (2026-11-16).
- **A fast path for the sprite-number check** before SPOFS, SPROT and
  SPSCALE: it saved about 5 aarch64 instructions a call, and SPROT's time
  on the device did not change (2026-11-16).
- **Comparing scenes to skip a frame:** the emulator draws a frame only after
  a command changed what shows. Of about 1100 frames each in five projects, 0
  to 3 had a scene, pages and cells like the last, so a hash would cost every
  frame for nothing.
