# Benchmark results

[benchmarking.md](benchmarking.md) says how to take these numbers. They date
from 2026-11-05, except the handheld sets, from 2026-11-15 and 2026-11-16,
and the runtime commands, calls by name and sound sets, from 2026-11-17.
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
x86-64 over frames 3 to 83. The device times in the first table are those
of the sets above; the device's own run of this set is
[at its end](#grp-rows-on-the-device).

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
and 15 ms with contiguous ones, of 45 to 55: 10 to 27%. Translation
misses are not modeled: the target's rows lie in 240 different 4 KB pages.

So this set changed no stride. The rule for changing it was a gain of a
third or more in GCOPY or GFILL at a padded stride on the device, in every
process; the device met it ([below](#grp-rows-on-the-device)), and the
stride became 2112 ([GRP rows padded](#grp-rows-padded)).

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

#### GRP rows on the device

The handheld of the [handheld set](#handheld-cpu-from-cc49c48), front end
stopped and pinned to one core, ran the standalone benchmark in five
processes, each timing every stride in two rounds of 30 frames with the
pass over the target area, then six more processes at strides 2048 and
2112 with huge pages asked for and refused. Each round maps its pages
anew. The device's kernel, 4.4, has no transparent huge pages, so the
last six run as the first five do.

The kernels, ms a frame, median of the five processes' ten rounds, and the
change against stride 2048:

| Kernel | 2048 | 2064 | 2112 | 3072 |
|---|---|---|---|---|
| GCOPY, `da44ce5` | 42.29 | 22.64 (-46.5%) | 23.65 (-44.1%) | 27.77 (-34.3%) |
| GCOPY, `bfa7e32` | 30.58 | 18.30 (-40.1%) | 17.18 (-43.8%) | 21.50 (-29.7%) |
| GFILL | 7.07 | 4.41 (-37.6%) | 4.21 (-40.4%) | 5.62 (-20.4%) |
| GLOAD | 1.78 | 2.37 (+33.0%) | 2.37 (+33.2%) | 1.77 (-0.8%) |

Taking each process's best round, `bfa7e32`'s GCOPY gained 40 to 51% at
2064 and 44 to 59% at 2112, and GFILL 35 to 45% and 35 to 54%. At 2048
the processes' best times were 30.45 to 36.89 ms for GCOPY and 6.50 to
8.36 ms for GFILL; at 2064 and 2112, 14.95 to 18.43 and 3.87 to 4.64. The
ranges do not overlap, so the gain holds in every process and meets the
rule: padding page rows pays. GLOAD, the control, falls in two clusters at
every stride, 1.73 to 1.92 ms and 2.17 to 2.40, and more of its rounds
fell in the slow one at the padded strides: 7 and 8 of 10 at 2064 and
2112, against 2 at 2048 and 3 at 3072. A padded build has to time GLOAD
again.

Stride 3072 gained 16 to 42% for `bfa7e32`'s GCOPY, where the simulation
has it alias as 2048 does. The L2 model does not account for all the
misses that cost. One reading, not tested: the 32 KB 4-way L1 holds 8 KB a
way, so rows 8 KB apart fall in one set and rows 12 KB apart alternate
between two.

The two clusters of times at stride 2048 are 42.07 to 42.59 ms and 50.15
to 51.10 for `da44ce5`'s GCOPY, and 6.50 to 7.22 and 8.36 to 8.73 for
GFILL. Of
the 11 processes, 5 ran in the slow cluster, and each process stayed in
its cluster through both rounds, although each round mapped its pages on
other frames. The placement report does not separate them. In every round
but one, 229 or more of the target rows' 240 pages used 4 of the 8 values
of physical address bits 12 to 14, in slow and fast processes alike; the
one round spread over all 8 was fast, as was its process's other round.
At the padded strides, where a row crosses into the next page, 73 to 92%
of neighboring pages are physically neighbors too: the kernel hands out
mostly contiguous frames. The clusters are neither the pages' placement
nor huge pages. What a process keeps through its rounds, such as its
frame buffer, its inputs and where its code and stack lie, was not
measured.

The scenes, median `run_us` over frames 10 to 69, five runs a build, the
builds alternating:

| Scene | `da44ce5` (ms) | `bfa7e32` (ms) | Change, best | Change, median |
|---|---|---|---|---|
| GFILL | 12.62 to 14.12 | 12.13 to 13.65 | -3.9% | -3.1% |
| GCOPY | 49.58 to 59.77 | 45.42 to 46.32 | -8.4% | -21.3% |
| GLOAD | 2.06 to 2.22 | 2.07 to 2.12 | +0.5% | -0.9% |
| GUPLOAD | 0.22 to 0.23 | 0.22 to 0.23 | 0.0% | 0.0% |
| VINT | 8.40 to 8.41 | 8.41 to 8.42 | +0.1% | 0.0% |
| TPRINT | 3.52 to 3.54 | 3.46 to 3.57 | -1.7% | -0.6% |

Every checksum is the same for both builds. `bfa7e32`'s GCOPY took 45.4 to
46.3 ms in all five runs, while `da44ce5`'s fell four times in its slow
cluster: the gain is real, as the instruction counts fell 27.5% with it.
GFILL's 3 to 4% is under the noise rule's 10%, though its counts fell
10.5%. If the scenes save what the kernels do at 2064 or 2112, GCOPY
would lose 12 to 15 ms of its 46 and GFILL about 3 ms of its 12 to 14.

#### GRP rows padded

`page::Page` now owns a page's buffer, and rows lie `page::STRIDE` pixels
apart ([renderer.md](../rendering/renderer.md#frame-data)). To choose the stride,
the handheld ran four builds, alternating, six runs each, with the method of
the set above: main (`caa624c`), the branch at stride 2048, and the branch
at 2064 and 2112. Median `run_us` over frames 10 to 69, in ms:

| Scene | `caa624c` | 2048 | 2064 | 2112 | Change at 2112, median |
|---|---|---|---|---|---|
| GCOPY | 46.32 (38.61 to 46.59) | 46.51 | 23.00 (22.76 to 23.26) | 22.15 (22.06 to 22.61) | -52.2% |
| GFILL | 12.48 (11.63 to 13.44) | 13.14 | 9.90 (9.80 to 10.04) | 9.59 (9.52 to 9.73) | -23.2% |
| GLOAD | 2.57 (2.52 to 2.59) | 2.54 | 2.70 (2.69 to 2.72) | 2.71 (2.18 to 2.72) | +5.3% |
| GUPLOAD | 0.22 | 0.22 | 0.24 | 0.23 | +5.6% |
| GARRAY | 10.18 | 9.40 | 9.37 | 9.31 | -8.5% |
| GPUTCHR | 11.87 | 11.91 | 11.88 | 11.92 | +0.4% |
| SPMOVE | 9.17 | 9.20 | 9.33 | 9.17 | -0.1% |
| VINT | 7.76 | 7.89 | 7.77 | 7.76 | 0.0% |

Every checksum is the same in all four builds.

- **GCOPY and GFILL.** At 2112 every run is faster than every run of
  `caa624c` and of the branch at 2048, which fall in the two clusters seen
  before (GFILL at 11.6 to 11.7 and 13.1 to 13.4 ms; GCOPY once at 38.6,
  otherwise 46.0 to 47.2). The padded builds show one cluster each. 2112 is
  ahead of 2064 in every run, by 4% in GCOPY and 3% in GFILL, as it was in
  the standalone kernels (6% and 5%), which share no code layout with the
  scenes.
- **GLOAD** stays memory bound. Its usual time moves from 2.52 to 2.59 ms to
  2.69 to 2.72 at both padded strides, 0.15 ms; one run in each of the
  builds at 2048 and 2112 fell in a fast cluster near 2.1 ms. Its
  instructions do not change. GCOPY saves 24 ms where GLOAD loses 0.15.
- **GARRAY** gains from the array being filled a row at a time
  (`Graphics::array`, `mirror_page`), 8.9% fewer aarch64 instructions,
  whatever the stride.
- **GUPLOAD**'s 0.01 to 0.02 ms is of the size its runs differ by.

The stride is 2112: 2048 + 64 pixels, 256 bytes, so that successive rows
start 4 sets of the L1 apart. A page takes 2048 x 2112 x 4 bytes, 16.5 MiB,
512 KiB (3.1%) more than before; six pages in use, 3 MiB more. 2064 would
cost 128 KiB (0.8%) a page for 3 to 4% less of the gain. GARRAY arrays,
files and textures keep 2048 pixels a row. `FREEMEM` counts a program's
strings and arrays only, so pages never entered it.

The branch as merged, against main (`9c1b55a`) on the handheld, six runs
each, alternating:

| Scene | `9c1b55a` (ms) | Branch (ms) | Change, median |
|---|---|---|---|
| GCOPY | 46.54 (45.81 to 47.54) | 22.11 (22.01 to 22.29) | -52.5% |
| GFILL | 11.27 (10.94 to 12.65) | 9.65 (9.56 to 9.82) | -14.3% |
| GLOAD | 2.56 (2.55 to 2.58) | 2.70 (2.19 to 2.71) | +5.4% |
| GUPLOAD | 0.22 (0.19 to 0.22) | 0.24 (0.22 to 0.24) | +9.5% |
| GARRAY | 10.13 (10.09 to 10.19) | 9.37 (9.15 to 9.45) | -7.5% |
| GPUTCHR | 12.04 | 11.98 | -0.5% |
| SPMOVE | 9.05 | 9.07 | +0.2% |
| VINT | 7.77 | 7.77 | +0.1% |

Every checksum is the same for both builds. GFILL's main runs fell mostly
in the fast cluster this time; every run of the branch is still faster than
every one of main's. GUPLOAD's 0.02 ms is of the size of its runs' spread.

Instructions a frame, in millions, x86-64 over frames 3 to 83 and aarch64
(qemu, `setarch -R`) over frames 3 to 6, main against the branch:

| Scene | x86-64, `9c1b55a` | x86-64, branch | Change | aarch64, `9c1b55a` | aarch64, branch | Change |
|---|---|---|---|---|---|---|
| GFILL | 14.08 | 13.74 | -2.4% | 12.79 | 12.78 | -0.1% |
| GALPHA | 83.00 | 83.02 | 0.0% | 57.70 | 57.68 | 0.0% |
| GLINE | 22.19 | 22.07 | -0.5% | 20.53 | 20.40 | -0.6% |
| GCIRCLE | 18.13 | 18.03 | -0.5% | 16.58 | 16.46 | -0.7% |
| GTRI | 32.43 | 32.24 | -0.6% | 26.12 | 26.14 | +0.1% |
| GPUTCHR | 27.05 | 26.95 | -0.4% | 26.75 | 26.77 | +0.1% |
| GCOPY | 17.23 | 17.18 | -0.3% | 13.16 | 12.85 | -2.4% |
| GLOAD | 7.91 | 7.90 | -0.1% | 7.47 | 7.46 | -0.2% |
| GUPLOAD | 1.92 | 1.92 | +0.1% | 1.44 | 1.43 | -0.1% |
| GARRAY | 15.29 | 14.15 | -7.5% | 14.27 | 13.00 | -8.9% |
| SPMOVE | 78.07 | 78.07 | 0.0% | 71.58 | 71.58 | 0.0% |
| TPRINT | 42.37 | 42.37 | 0.0% | 28.44 | 28.50 | +0.2% |
| VINT | 8.74 | 8.74 | 0.0% | 8.14 | 8.14 | 0.0% |

On x86-64 no other scene of the 32 moves by more than 0.13%, and every
checksum is the same; TPERF's stay `e8e5f6dc467db861` at 400 frames,
`a6e1ed07824b1ad8` at 3600 and `5d9bb17a39eefb70` at 10500. Taking rows
whole from `Page` first cost GCOPY and GTRI 0.8 and 1.4% on x86-64, and
offsets worked out in 64 bits cost GPUTCHR 1.5% on aarch64, until
rectangles were checked once (`Page::spans`) and single pixels placed in
32 bits.

Uploads to the GPU pass the pitch. On GLES2 they go in bands of at most
64 KiB, as SDL's driver copies a rectangle to a buffer of its own whenever
the pitch is not the rectangle's width
([renderer.md](../rendering/renderer.md#grp-pages)). The device's GLES2 self-test
passes all 26 scenes with main and with the branch. Two of its counts of
pixels off differ: blur, 52048 off by one with main and 52072 with the
branch, and 4x4 text, 212 far against 208. Builds of the branch at stride
2048 with the bands and at 2112 without them give 208 too, and blur's
count moves between those builds (52072, 52068) although their textures
hold the same bytes. On the PC's GLES2 driver, main and the branch give
the same 4x4 text count and blur counts 4 pixels apart. So stride and
bands do not cause the 4x4 text change; what does was not found.

### Calls, from `48f7cfc`

This set counts what a call costs: a DEF's, a builtin's that the fused
steps do not stand for, and a runtime command's, which most game loops are
made of. Counts come from x86-64 with `perf stat -e instructions:u` on the
`release` build, and aarch64 with qemu's Cortex-A35 model on the `dist`
build under `setarch -R`, counted from whole blocks
([benchmarking.md](benchmarking.md#measuring-on-a-handheld)). A loop's count
is the difference of programs that run it 3200 and 9600 times (the game
loops 50 and 150), over 6400; the two give the same count to within a
couple of instructions, as the start-up varies a little with the hash
tables' seeds. Scenes are TPERF's, run alone with `#ONLY`, x86-64 over
frames 3 to 83 and aarch64 over frames 3 to 6. Every checksum is the same
before and after: TPERF's `e8e5f6dc467db861` at 400 frames and
`a6e1ed07824b1ad8` at 3600 on both machines, and each scene's and loop's.

The loops, each in a DEF of its own locals, `FOR I=1 TO N` round the
statement:

| Loop | Statement |
|---|---|
| Empty | none |
| Command DEF | `P I,R`, `DEF P A,B` sets a global to `A+B` |
| Function DEF | `R=F(I,R)`, `RETURN (A+B) AND 255` |
| Two OUTs | `TW R,I OUT S,T`, `DEF TW A,B OUT C,D: C=B:D=A` |
| One OUT | `ONEOUT I OUT R`, `R=K+1` |
| String argument | `R=SL(A$)`, `RETURN LEN(S$)` |
| Array argument | `R=AG(ARR,I AND 15)`, `RETURN A[K]` |
| Locals | `LV I`, a DEF of two `VAR`s and three statements |
| Recursion | `R=FB(4)`, a recursive Fibonacci: 9 calls |
| `CALL` | `CALL "P",I,R` |
| GOSUB | `GOSUB @S` to `G=G+1:RETURN`, at the top level |
| STR$, MID$, INSTR, CHR$ | `S$=STR$(I)`, `S$=MID$(A$,1,2)`, `R=INSTR(A$,"D")`, `S$=CHR$(65+(I AND 7))` |
| RND, BUTTON | `R=RND(100)`, `R=BUTTON(0)`, `R=BUTTON(0,4,1)` |
| SPOFS | `SPOFS 0,I AND 255,R` and `SPOFS 0 OUT S,T` |
| LOCATE, GPSET | `LOCATE I AND 31,3`, `GPSET I AND 255,7,R` |
| Game 1 | `B=BUTTON(0)`, then for 64 sprites: `MOVE K` (a DEF that adds a velocity to two real arrays and bounces off the edges), `SPOFS K,PX#[K],PY#[K]`, `IF NEAR(K,200,120) THEN INC HIT` |
| Game 2 | the same with the movement and the test written out in the loop |
| Game 3 | `B=BUTTON(0)`, then for 32 sprites `SPOFS K OUT X,Y`, `IF RND(8)==0 THEN X=X+1`, `SPOFS K,X,Y`, and a `LOCATE` |

Where a call's aarch64 instructions went, from one iteration's trace, in the
fused entries the loop runs (choosing each entry included). The function
DEF loop:

| Part | `48f7cfc` | Branch |
|---|---|---|
| Pushing `I` and `R` | 72 | read in the call's entry |
| The call: leaving the fused loop, `fast_call`, the frame, coming back | 275 | 188 |
| The return: the same | 131 | 86 |
| The callee's two entries, the store, the FOR | 364 | 356 |
| An iteration | 842 | 630 |

A command DEF's call takes 75 + 275 + 166 of 723 before and 187 + 140 of
529 after. Before, a call left the fused loop and its view of the
variables, took a list for the DEF's variables from a pile of spare ones,
moved each argument into it with a push and its own check of the room,
wrote a frame of 112 bytes, and came back; a return read the frame back,
scanned the variables for strings to drop and gave the list back to the
pile.

A runtime command, from the `CallHost` instruction to the fused code
again: SPOFS 758 aarch64 instructions before, 543 after. Of the 758, about 115
were `run_code`'s checks and `step`'s match around the call, and 80
telling whether the screens changed: `sight`, and the sprite's appearance
taken before and after and compared. Of the 543, about 150 are the VM's (the call, cutting the
arguments, handing READ's place to the host and back), about 390 the
runtime's: `Host::call`'s dispatch on the command (`run`, `run_counted`,
`sp_command`'s own match) and SPOFS's work. BUTTON(0) takes 975 an
iteration before and 868 after, of which 137 are still `malloc` and `free`
for the one-value list every function of the runtime returns. STR$ went
from 1295 to 1192; 423 of them are allocations in making the string.

What was made:

- `8937306`: a host call is a fused entry. `run_fast` makes it with the code
  `step` uses, factored out as `host_call`, and hands anything other than
  going on back to `run_code`, which settles it as `step`'s outcome.
- `208e0ee`: the variables of all DEF calls under way follow each other in
  one list, and a frame holds where its own start.
- `9fa9146`: a DEF call and its return are made inside the fused loop, which
  only points its view of the variables at the new frame. The fuser checks
  once that a call fits its DEF and keeps what it takes of the stack in the
  entry.
- `8edef75`: once the frame has changed, `Host::call` skips telling whether a
  command changes the screens: only drawing a frame clears that.
- `23907aa`: a host call's one result is pushed without extending from a
  list.
- `daf54eb`: a builtin's call is a fused entry, made by `builtin_call`, the
  code `step` uses.
- `b08aba3`: a frame keeps the lists of `*` DEFs in one box most frames do
  without, 112 bytes to 72, and a return cuts the stack and the variables
  with a check that what goes is numbers.
- `bd8e2c7`: a call of one to three arguments that are variables or numbers
  reads them where they are, in one entry.
- `72fd0be`: `run_fused` only says that it stopped before a builtin's or
  host's call, and `run_fast` reads the call from the entry.

aarch64 instructions an iteration:

| Loop | `48f7cfc` | Branch | Change |
|---|---|---|---|
| Empty | 136 | 132 | -2.8% |
| Command DEF | 723 | 529 | -26.9% |
| Function DEF | 842 | 630 | -25.2% |
| Two OUTs | 961 | 753 | -21.7% |
| One OUT | 768 | 600 | -21.9% |
| String argument | 811 | 630 | -22.3% |
| Array argument | 870 | 709 | -18.5% |
| Locals | 865 | 704 | -18.6% |
| Recursion | 5226 | 3717 | -28.9% |
| `CALL` | 2827 | 2798 | -1.0% |
| GOSUB | 284 | 279 | -1.7% |
| STR$ | 1295 | 1192 | -8.0% |
| MID$ | 1281 | 1169 | -8.7% |
| INSTR | 1603 | 1516 | -5.4% |
| CHR$ | 952 | 848 | -10.9% |
| RND | 934 | 828 | -11.4% |
| SPOFS | 1158 | 929 | -19.8% |
| SPOFS OUT | 1133 | 1025 | -9.5% |
| LOCATE | 854 | 734 | -14.1% |
| BUTTON(0) | 975 | 868 | -11.0% |
| BUTTON(0,4,1) | 1379 | 1263 | -8.4% |
| GPSET | 1232 | 1109 | -10.0% |
| Game 1 (64 sprites) | 277027 | 229619 | -17.1% |
| Game 2 (64 sprites) | 141539 | 125990 | -11.0% |
| Game 3 (32 sprites) | 97836 | 82969 | -15.2% |

x86-64 instructions an iteration:

| Loop | `48f7cfc` | Branch | Change |
|---|---|---|---|
| Empty | 152 | 157 | +3.2% |
| Command DEF | 823 | 610 | -25.9% |
| Function DEF | 950 | 730 | -23.1% |
| Two OUTs | 1094 | 863 | -21.1% |
| One OUT | 883 | 679 | -23.2% |
| String argument | 931 | 721 | -22.6% |
| Array argument | 1004 | 815 | -18.8% |
| Locals | 980 | 790 | -19.4% |
| Recursion | 5824 | 4134 | -29.0% |
| `CALL` | 2639 | 2656 | +0.6% |
| GOSUB | 308 | 315 | +2.2% |
| STR$ | 1179 | 1088 | -7.7% |
| MID$ | 1191 | 1098 | -7.8% |
| INSTR | 1461 | 1393 | -4.7% |
| CHR$ | 890 | 800 | -10.1% |
| RND | 972 | 847 | -12.9% |
| SPOFS | 1250 | 1015 | -18.8% |
| SPOFS OUT | 1202 | 1086 | -9.6% |
| LOCATE | 963 | 824 | -14.4% |
| BUTTON(0) | 1023 | 898 | -12.2% |
| BUTTON(0,4,1) | 1427 | 1296 | -9.2% |
| GPSET | 1282 | 1138 | -11.2% |
| Game 1 (64 sprites) | 303732 | 256023 | -15.7% |
| Game 2 (64 sprites) | 152602 | 139635 | -8.5% |
| Game 3 (32 sprites) | 103245 | 86918 | -15.8% |

Instructions a frame, in millions, program and CPU drawing:

| Scene | aarch64 `48f7cfc` | aarch64 branch | Change | x86-64 `48f7cfc` | x86-64 branch | Change |
|---|---|---|---|---|---|---|
| VINT | 8.98 | 8.93 | -0.6% | 9.44 | 9.53 | +0.9% |
| VREAL | 7.48 | 7.45 | -0.5% | 7.66 | 7.90 | +3.2% |
| VSTR | 12.86 | 12.86 | 0.0% | 11.46 | 11.60 | +1.2% |
| VARRAY | 13.58 | 13.61 | +0.2% | 14.19 | 14.32 | +0.9% |
| VDEF | 7.01 | 5.38 | -23.3% | 7.66 | 5.85 | -23.6% |
| VVAR | - | - | - | 33.41 | 33.72 | +0.9% |
| SPMOVE | - | - | - | 78.81 | 78.30 | -0.7% |
| SPROT | - | - | - | 304.20 | 303.38 | -0.3% |
| SPCHURN | - | - | - | 108.66 | 108.19 | -0.4% |
| SPHIT | 51.31 | 50.95 | -0.7% | 55.52 | 55.11 | -0.7% |
| SPANIM | - | - | - | 294.66 | 294.65 | 0.0% |
| TPRINT | - | - | - | 42.42 | 42.36 | -0.1% |
| TPUT | 32.33 | 31.87 | -1.4% | 52.90 | 52.02 | -1.7% |
| TSCROLL | - | - | - | 44.76 | 44.75 | 0.0% |
| GAME | 20.27 | 20.14 | -0.6% | 28.56 | 28.57 | 0.0% |

VDEF, whose loop is a function call and a command call with two OUTs, goes
down by a quarter. The other scenes move by under 2% of their frames: their
program parts call little, and the sprite and text scenes' counts are
mostly drawing. GAME calls GTRI, SPOFS and LOCATE a few hundred times a
frame, under 1% of its count. On x86-64 the scenes and loops that make no
calls (VINT, VREAL, VSTR, VARRAY, the empty FOR and GOSUB) cost 1 to 3%
more, as LLVM lays out `run_fused`'s registers anew; on aarch64 they move
by under 3%, the empty FOR and GOSUB down.

`run_fast` is 40872 bytes of aarch64 code at `48f7cfc` and 40612 on the
branch, with `host_call` (1828 bytes) and `builtin_call` (1708) outside it.
Making the calls in the fused loop added little to it; `step` shrank from
16648 to 15056 bytes, as its call arms went to those functions.

The handheld of the [handheld set](#handheld-cpu-from-cc49c48) ran
`72a5db7`, the last commit before this set, and `d081e7a`, its end, with
that set's method, quiet and pinned, the builds alternating within each
round with those of the [next set](#variables-and-constants-from-d081e7a).
Times are the best of three runs, and for GFILL, GCOPY, GLOAD and GUPLOAD
the median of five, as their runs fall in two clusters. The last column is
the change in aarch64 instructions a frame, in millions: for VINT to VDEF,
SPHIT and GAME from the table above (`48f7cfc` against the branch), for
the rest counted on these two builds the same way. Every checksum is the
same.

| Scene | `72a5db7` (ms) | `d081e7a` (ms) | Change | Instructions (M) |
|---|---|---|---|---|
| VINT | 8.41 | 8.61 | +2.4% | -0.05 |
| VREAL | 7.27 | 7.39 | +1.7% | -0.03 |
| VSTR | 15.74 | 15.23 | -3.2% | 0.00 |
| VARRAY | 13.51 | 13.78 | +2.0% | +0.03 |
| VDEF | 7.27 | 5.71 | -21.4% | -1.63 |
| VVAR | 43.46 | 44.87 | +3.2% | +0.22 |
| SPMOVE | 10.94 | 9.63 | -12.0% | -0.62 |
| SPROT | 7.76 | 6.65 | -14.2% | -0.80 |
| SPCHURN | 30.14 | 29.99 | -0.5% | -0.43 |
| SPHIT | 26.09 | 27.96 | +7.2% | -0.36 |
| SPANIM | 7.96 | 7.93 | -0.5% | 0.00 |
| TPRINT | 3.46 | 3.43 | -0.8% | -0.13 |
| TPUT | 19.10 | 17.17 | -10.1% | -0.46 |
| TSCROLL | 4.28 | 4.23 | -1.2% | - |
| TMULTI | 0.73 | 0.72 | -1.2% | - |
| GFILL | 13.82 | 13.04 | -5.7% | - |
| GALPHA | 61.28 | 60.84 | -0.7% | - |
| GLINE | 18.83 | 18.13 | -3.7% | - |
| GCIRCLE | 14.30 | 13.94 | -2.5% | - |
| GTRI | 22.25 | 21.52 | -3.3% | - |
| GPUTCHR | 12.27 | 12.12 | -1.2% | - |
| GCOPY | 48.43 | 46.43 | -4.1% | - |
| GLOAD | 2.58 | 2.58 | -0.1% | - |
| GUPLOAD | 0.22 | 0.22 | -2.0% | - |
| GARRAY | 10.30 | 10.23 | -0.7% | -0.06 |
| LCOMP | 0.90 | 0.79 | -12.3% | -0.16 |
| LBLUR | 0.53 | 0.49 | -8.0% | - |
| LMOSAIC | 0.54 | 0.49 | -8.2% | - |
| LCOLOR | 0.63 | 0.57 | -9.5% | - |
| LMATRIX | 0.66 | 0.59 | -9.8% | -0.09 |
| LRASTER | 1.05 | 1.04 | -0.5% | -0.06 |
| GAME | 8.93 | 8.79 | -1.5% | -0.13 |

The game loops, run 10 times a frame with a `VSYNC` after, timed the same
way, best of five (their checksums include the start-up banner, which
names the build, so they differ by build and were not compared):

| Loop | `72a5db7` (ms) | `d081e7a` (ms) | Change | aarch64 instructions |
|---|---|---|---|---|
| Game 1 | 3.82 | 2.98 | -22.2% | -17.1% |
| Game 2 | 2.14 | 1.93 | -9.8% | -11.0% |
| Game 3 | 1.52 | 1.29 | -15.2% | -15.2% |

The device runs at 1.008 GHz and 1.3 to 1.8 cycles an instruction in the
interpreter, so a million instructions is 1.3 to 1.8 ms. Real, as the time
moves with the counts: VDEF (-1.56 ms, -1.63 M), SPMOVE (-1.31 ms,
-0.62 M), SPROT (-1.11 ms, -0.80 M), LCOMP and LMATRIX (-0.11 and
-0.07 ms, -0.16 and -0.09 M), and the three game loops. These save 0.7 to
2.1 ms a million instructions. LBLUR, LMOSAIC and LCOLOR gain as LCOMP
does and were not counted. TPUT's -1.93 ms is about three times what its
-0.46 M explain: part of it is layout, as `72a5db7` ran it 1.2 ms slower
than `0dc842c` did in the chains set. Layout, with no count behind it:
VINT, VREAL and VARRAY up 2%, VVAR up 3%, SPHIT up 7%, VSTR down 3%. The
other scenes move by under 6%.

What calls still cost, for later work:

- A function of the runtime returns its value in a `Vec`: 137 aarch64
  instructions of `malloc` and `free` in each BUTTON(0), RND or similar. Its
  commands return results through `Result<Vec<Value>>` throughout, so a way
  round it is a method on `Host` for one value, which the hot functions would
  need code of their own for.
- `Host::call`'s dispatch, `run`'s match on the command and the form, then
  each group's own match on the command, the form and the count, about 100
  instructions before a command's work.
- A DEF's RETURN out of a command DEF, 140 instructions with choosing the
  entry, against 86 for a function's: it works out the OUT variables from
  the DEF's description on each return.
- `CALL "NAME"` (2798) makes a `String` of the name and looks it up in a
  hash table on each call. A cache of the last name, valid for the running
  program, would leave out most of it.

### Variables and constants, from `d081e7a`

This set counts what reading and writing a variable costs in the fused
code, and what choosing an entry costs before its work. Counts are taken
as in the [calls](#calls-from-48f7cfc) set: x86-64 with
`perf stat -e instructions:u` on the `release` build, aarch64 with qemu's
Cortex-A35 model on the `dist` build under `setarch -R`, from whole
blocks. A loop's count is the difference of programs that run it 3200 and
6400 times (the sprite and game loops 50 and 100), over the difference.
Scenes are TPERF's, run alone with `#ONLY`. Every checksum is the same
before and after: TPERF's `e8e5f6dc467db861` at 400 frames,
`a6e1ed07824b1ad8` at 3600 and `5d9bb17a39eefb70` at 10500 on x86-64, the
first two on aarch64, and each scene's.

The loops are those of the [chains](#three-address-chains-from-018017d)
and [calls](#calls-from-48f7cfc) sets, each in a DEF of its own locals.
The scalar reals' loop uses `U#`, `W#` and `T#` for `DX#`, `DY#` and `T#`.
Three more: `VINT, unsuffixed` is VINT's loop on `A` and `B`, and the
globals' rows run VINT's, the counters' and VARRAY's loops at the top
level, on globals.

Where one iteration's aarch64 instructions went at `d081e7a`, from qemu's
trace, by the source line and inlined function each instruction comes
from (finding a place includes loading the value's tag):

| Loop | Choosing an entry | Choosing a chain's step | Finding a place | A write's drop check | A write's stores | Operators | Other |
|---|---|---|---|---|---|---|---|
| Empty | 34 | 0 | 45 | 4 | 5 | 0 | 44 |
| VINT's loop | 34 | 98 | 178 | 14 | 18 | 43 | 88 |
| Counters | 98 | 52 | 200 | 12 | 19 | 65 | 99 |
| Reals | 114 | 18 | 173 | 10 | 21 | 113 | 125 |
| Array | 52 | 130 | 259 | 16 | 21 | 51 | 199 |
| VREAL's loop | 35 | 130 | 237 | 18 | 33 | 67 | 264 |

Finding a place is 30 to 38% of every loop. One operation, from the
disassembly of `run_fast`:

- **A read** (`get`) takes 9 or 10 instructions and the tag's load: the
  table bit shifted and masked, the address of the two tables, which live
  on the stack, the table's length loaded, compared and branched on, its
  pointer loaded, and the index added.
- **A write** (`at_var` and `put`) takes the same 9 to find the place, 3
  to look at the old value's tag in case it holds a string or array to drop,
  and 2 stores.
- **Choosing an entry** takes 16: the entry table's length (on the stack)
  compared with `pc`, the entry's address (`madd`, as an entry is 20 bytes),
  its length loaded and taken from the budget, its kind loaded, the jump
  table and the indirect branch, and two register moves.
- **Choosing a chain's step** took 16: the step's end compared, then six
  loads of all of the step's fields before the jump table, whatever its
  kind.
- **A constant** costs a read like any variable's, as constants are places
  after the globals.

A variable's suffix gives nothing to build on: it only sets the starting
value ([sb4-pitfalls.md](../smilebasic4/sb4-pitfalls.md)): `A%="X"` stores a string, and
a variable of any suffix can hold any kind of value. Only constants and
the chains' hidden places are known to hold numbers.

What was made:

- `cc60440`: a chain's steps are matched where they are rather than copied
  out first, so each kind loads only its own fields. Choosing a step falls
  from 16 to about 10 instructions.
- `9113d96`: an operator with a constant integer or real on its right
  carries the number in its entry or step instead of reading it from its
  place: `AddInt`, `SubInt`, `MulInt`, `AndInt` and `BinaryInt`, their
  real forms, the comparison jumps of chains (`JumpLessInt` and the others),
  and `AddStoreInt`, `SubStoreInt`, `MulStoreInt`, `AddStoreReal` and the
  `Unless` entries' integer forms. A constant on the left of an operator
  whose sides can change places (`+`, `*`, `AND`, `OR`, `XOR`, `==`, `!=`)
  goes to the right; for reals that gives the same result, as a constant is
  never NaN. Each saves the constant's read and one tag check, about 12
  instructions.
- `4ae34d1`: the random programs of `tests/fuse_random.rs` call COMMON
  DEFs of a slot they EXEC, a DEF 40 calls deep and DEFs whose variables
  have the globals' names, and put strings, arrays, reals and integers into
  variables of any suffix.

aarch64 instructions an iteration:

| Loop | `d081e7a` | `cc60440` | `9113d96` | Change |
|---|---|---|---|---|
| Empty | 132.0 | 132.2 | 130.9 | -0.8% |
| GOSUB | 279.4 | 278.8 | 262.4 | -6.1% |
| VINT's loop | 473.1 | 450.9 | 428.7 | -9.4% |
| VINT's loop, globals | 472.5 | - | 429.1 | -9.2% |
| VINT's loop, unsuffixed | 473.0 | - | 429.0 | -9.3% |
| Reals | 574.5 | 571.7 | 536.5 | -6.6% |
| Counters | 547.0 | 537.7 | 466.4 | -14.7% |
| Counters, globals | 546.7 | - | 466.5 | -14.7% |
| IF chain | 441.3 | 438.4 | 387.5 | -12.2% |
| Array | 728.0 | - | 668.2 | -8.2% |
| VARRAY's loop | 1318.8 | 1263.3 | 1144.2 | -13.2% |
| VARRAY's loop, globals | 1319.1 | - | 1143.8 | -13.3% |
| VREAL's loop | 786.4 | 758.1 | 719.9 | -8.5% |
| Sprites (64) | 111368 | - | 105469 | -5.3% |
| DEF a sprite (64) | 130598 | - | 124836 | -4.4% |
| Function DEF | 630.0 | - | 623.8 | -1.0% |
| Command DEF | 528.9 | - | 530.8 | +0.4% |
| Game 1 (64 sprites) | 229721 | - | 222436 | -3.2% |
| Game 2 (64 sprites) | 124839 | - | 120358 | -3.6% |
| Game 3 (32 sprites) | 82930 | - | 82661 | -0.3% |

x86-64 instructions an iteration:

| Loop | `d081e7a` | `9113d96` | Change |
|---|---|---|---|
| Empty | 156.4 | 157.0 | +0.4% |
| GOSUB | 315.1 | 302.8 | -3.9% |
| VINT's loop | 521.4 | 477.2 | -8.5% |
| Reals | 652.5 | 620.7 | -4.9% |
| Counters | 615.0 | 541.9 | -11.9% |
| IF chain | 497.9 | 449.0 | -9.8% |
| Array | 808.3 | 725.4 | -10.3% |
| VARRAY's loop | 1416.9 | 1228.9 | -13.3% |
| VREAL's loop | 864.1 | 763.4 | -11.7% |
| Sprites (64) | 124595 | 115500 | -7.3% |
| DEF a sprite (64) | 145528 | 136388 | -6.3% |
| Function DEF | 729.6 | 729.8 | 0.0% |
| Command DEF | 610.0 | 610.4 | +0.1% |
| Game 1 (64 sprites) | 256148 | 244749 | -4.5% |
| Game 2 (64 sprites) | 138473 | 131474 | -5.1% |
| Game 3 (32 sprites) | 86948 | 86913 | 0.0% |

The globals' and unsuffixed rows match the locals' within 0.5 on both
machines: the two tables cost the same, and a suffix changes nothing.

Instructions a frame, in millions, program and CPU drawing, aarch64 over
frames 3 to 6 and x86-64 over frames 3 to 83:

| Scene | aarch64 `d081e7a` | aarch64 `9113d96` | Change | x86-64 `d081e7a` | x86-64 `9113d96` | Change |
|---|---|---|---|---|---|---|
| VINT | 8.93 | 8.13 | -8.9% | 9.15 | 8.74 | -4.5% |
| VREAL | 7.46 | 6.86 | -8.1% | 7.49 | 7.00 | -6.5% |
| VSTR | 12.88 | 12.82 | -0.5% | 11.57 | 11.53 | -0.4% |
| VARRAY | 13.60 | 11.85 | -12.9% | 13.71 | 12.44 | -9.3% |
| VDEF | 5.38 | 5.35 | -0.5% | 5.85 | 5.85 | 0.0% |
| VVAR | - | - | - | 33.61 | 33.49 | -0.3% |
| SPHIT | 50.96 | 50.88 | -0.2% | 55.10 | 55.05 | -0.1% |
| TPUT | 31.86 | 31.60 | -0.8% | 51.87 | 51.63 | -0.5% |
| GAME | 20.13 | 19.94 | -0.9% | 28.30 | 28.28 | -0.1% |

SPMOVE, SPROT, SPCHURN, SPANIM, TPRINT and TSCROLL move by under 0.1% on
x86-64. GAME's interpreter is about a quarter of its frame, and most of
its statements read array elements and reals into reals, which carry few
constants.

`run_fast` is 40612 bytes of aarch64 code at `d081e7a`, 41132 at
`cc60440` and 46236 at `9113d96`: each new entry and step kind inlines its
operator's work on integers and reals, about 190 bytes. A loop runs only
the kinds it uses.

The handheld ran `d081e7a`, `500b8ca` (this set as merged) and `0f41de0`
in the same rounds as the [calls](#calls-from-48f7cfc) set's builds, with
the same method. `0f41de0`, the code that the `unchecked-vars` feature
builds, reads the fused code's places without a bounds check on each; its
`run_fast` is 41436 bytes. Times are the best of three runs, the median of
five for GFILL, GCOPY, GLOAD and GUPLOAD. The instruction columns are the
change in aarch64 instructions a frame, in millions: for VINT to VDEF,
SPHIT and GAME from the table above and the same count of `0f41de0`, for
the rest counted on these builds. Every checksum is the same.

| Scene | `d081e7a` (ms) | `500b8ca` (ms) | Change | Instructions (M) | `0f41de0` (ms) | Change from `500b8ca` | Instructions (M) |
|---|---|---|---|---|---|---|---|
| VINT | 8.61 | 7.90 | -8.3% | -0.80 | 6.96 | -11.8% | -0.96 |
| VREAL | 7.39 | 7.17 | -3.1% | -0.60 | 6.42 | -10.5% | -0.77 |
| VSTR | 15.23 | 16.04 | +5.4% | -0.06 | 15.17 | -5.4% | +0.21 |
| VARRAY | 13.78 | 12.52 | -9.2% | -1.75 | 10.78 | -13.9% | -1.70 |
| VDEF | 5.71 | 5.34 | -6.4% | -0.03 | 5.29 | -1.1% | +0.02 |
| VVAR | 44.87 | 46.73 | +4.1% | -0.18 | 42.70 | -8.6% | +0.07 |
| SPMOVE | 9.63 | 9.12 | -5.2% | -0.17 | 8.63 | -5.4% | -0.24 |
| SPROT | 6.65 | 6.10 | -8.4% | -0.10 | 5.87 | -3.8% | -0.02 |
| SPCHURN | 29.99 | 29.49 | -1.7% | -0.02 | 28.75 | -2.5% | +0.12 |
| SPHIT | 27.96 | 27.35 | -2.2% | -0.08 | 25.29 | -7.5% | +0.09 |
| SPANIM | 7.93 | 8.17 | +3.1% | 0.00 | 7.96 | -2.6% | +0.01 |
| TPRINT | 3.43 | 3.44 | +0.4% | 0.00 | 3.42 | -0.7% | +0.06 |
| TPUT | 17.17 | 16.85 | -1.9% | -0.26 | 16.58 | -1.6% | -0.07 |
| TSCROLL | 4.23 | 4.24 | +0.1% | - | 4.24 | +0.2% | - |
| TMULTI | 0.72 | 0.72 | -0.3% | - | 0.72 | -0.2% | - |
| GFILL | 13.04 | 12.60 | -3.3% | - | 12.66 | +0.5% | - |
| GALPHA | 60.84 | 60.91 | +0.1% | - | 60.67 | -0.4% | - |
| GLINE | 18.13 | 17.65 | -2.7% | - | 17.51 | -0.8% | - |
| GCIRCLE | 13.94 | 13.78 | -1.1% | - | 13.58 | -1.5% | - |
| GTRI | 21.52 | 21.49 | -0.1% | - | 21.34 | -0.7% | - |
| GPUTCHR | 12.12 | 12.29 | +1.3% | - | 11.97 | -2.6% | - |
| GCOPY | 46.43 | 46.71 | +0.6% | - | 47.84 | +2.4% | - |
| GLOAD | 2.58 | 2.55 | -1.0% | - | 2.55 | 0.0% | - |
| GUPLOAD | 0.22 | 0.22 | -0.2% | - | 0.21 | -0.9% | - |
| GARRAY | 10.23 | 10.31 | +0.9% | -0.57 | 8.90 | -13.7% | -0.91 |
| LCOMP | 0.79 | 0.77 | -2.9% | +0.02 | 0.75 | -2.9% | -0.03 |
| LBLUR | 0.49 | 0.48 | -2.6% | - | 0.47 | -2.1% | - |
| LMOSAIC | 0.49 | 0.48 | -2.4% | - | 0.47 | -2.1% | - |
| LCOLOR | 0.57 | 0.56 | -2.5% | - | 0.54 | -2.2% | - |
| LMATRIX | 0.59 | 0.58 | -1.7% | 0.00 | 0.56 | -3.2% | 0.00 |
| LRASTER | 1.04 | 0.98 | -6.4% | -0.06 | 0.89 | -8.6% | -0.06 |
| GAME | 8.79 | 8.70 | -1.1% | -0.19 | 7.80 | -10.3% | -0.52 |

The game loops of the calls set, timed the same way, best of five, with
the change in aarch64 instructions from the table above:

| Loop | `d081e7a` (ms) | `500b8ca` (ms) | Change | Instructions | `0f41de0` (ms) | Change from `500b8ca` | Instructions |
|---|---|---|---|---|---|---|---|
| Game 1 | 2.98 | 2.91 | -2.2% | -3.2% | 2.77 | -4.9% | -5.7% |
| Game 2 | 1.93 | 1.88 | -2.7% | -3.6% | 1.81 | -3.3% | -5.9% |
| Game 3 | 1.29 | 1.20 | -6.4% | -0.3% | 1.24 | +2.7% | +1.4% |

`500b8ca` against `d081e7a`: real, as the time moves with the counts, are
VINT (-0.71 ms, -0.80 M), VARRAY (-1.26 ms, -1.75 M), VREAL by less than
its count (-0.22 ms, -0.60 M), TPUT, LRASTER, and game loops 1 and 2.
GARRAY's -0.57 M does not show. Where the counts stay within 0.2 M, the
times move both ways: VSTR, VVAR and SPANIM up 3 to 5%, VDEF, SPMOVE and
SPROT down 5 to 8%, game 3 down 6%. That is layout. A cost from the
46 KB `run_fast`, past the 32 KB instruction cache, would make the scenes
it does not speed up slower together, and they do not.

`0f41de0` against `500b8ca`, the measured gain of the option: VINT -11.8%
(-0.94 ms, -0.96 M), VREAL -10.5% (-0.75 ms, -0.77 M), VARRAY -13.9%
(-1.74 ms, -1.70 M) and GARRAY -13.7% (-1.41 ms, -0.91 M), with the
counts, as are GAME's -10.3% (-0.90 ms, -0.52 M) and game loops 1 and 2,
-4.9 and -3.3% against -5.7 and -5.9% of instructions. VSTR, whose count
rises by 0.21 M, takes 5.4% less, and the sprite scenes 2.5 to 7.5% less
with their counts within 0.25 M: none of them loses, and their gains
beyond the counts are layout. The largest rises, game 3's +2.7% with
+1.4% of instructions and GCOPY's +2.4%, are within the noise.

What a step still costs, for later work:

- Finding a place, 9 or 10 instructions a read and a write, of which 3 are
  the bounds check against a table's length kept on the stack. Safe Rust
  keeps the check: a mask leaves a test of the length, and only a table of
  fixed size, `&[Value; 32768]`, drops it, which would take 512 KB of
  globals a slot and frames padded to as many variables
  ([tried and left out](#tried-and-left-out)).
- Choosing an entry, 16 instructions, the indirect branch included.
- A write's look at the old value, 3 instructions, which only the chains'
  hidden places could do without: they hold only numbers, but a step does
  not know whether it writes one.

The `unchecked-vars` feature of `sb-core`, off by default, takes the
bounds check out with `unsafe`, in one small module,
`crates/sb-core/src/vars.rs`. Its invariant and checks are in that file
and in [security.md](../development/security.md): `fuse::check` bounds every global
place by the program's globals and constants on every program the VM
takes, and works out what each entry needs of the running DEF's
variables, which `run_fused` compares with the DEF's before each entry,
in 3 instructions an entry. Choosing an entry takes about 19 instead of
16 with it, as LLVM now rebuilds the jump table's address and the entry
size each time instead of keeping them in registers. A read then takes 4
or 5 instructions and the tag's load, against 9 or 10: the table bit and
the index split off, the table's pointer loaded from the stack, and the
index added. A default build runs `fuse::check` too, but keeps the check
on each read and compiles out the one before each entry. It counts what
`9113d96` counts, within 0.3% on every loop and scene below on both
machines, and its `run_fast` is 46228 bytes. Against `9113d96`, aarch64
instructions an iteration with the feature on:

| Loop | `9113d96` | Unchecked | Change |
|---|---|---|---|
| Empty | 130.9 | 119.1 | -9.0% |
| GOSUB | 262.4 | 259.4 | -1.1% |
| VINT's loop | 428.7 | 374.9 | -12.5% |
| Reals | 536.5 | 492.4 | -8.2% |
| Counters | 466.4 | 428.9 | -8.0% |
| IF chain | 387.5 | 355.5 | -8.3% |
| Array | 668.2 | 572.1 | -14.4% |
| VARRAY's loop | 1144.2 | 972.9 | -15.0% |
| VREAL's loop | 719.9 | 634.5 | -11.9% |
| Sprites (64) | 105469 | 93079 | -11.7% |
| DEF a sprite (64) | 124836 | 112876 | -9.6% |
| Function DEF | 623.8 | 626.7 | +0.5% |
| Command DEF | 530.8 | 515.0 | -3.0% |
| Game 1 (64 sprites) | 222436 | 209783 | -5.7% |
| Game 2 (64 sprites) | 120358 | 113247 | -5.9% |
| Game 3 (32 sprites) | 82661 | 83801 | +1.4% |

x86-64, the same loops: empty FOR 157.0 to 136.5, VINT's loop 477.2 to
369.1, VARRAY's 1228.9 to 922.5, VREAL's 763.4 to 625.7, counters 541.9
to 460.3, sprites 115500 to 96754, game 1 244749 to 224742, game 3 86913
to 87058.

Instructions a frame, in millions:

| Scene | aarch64 `9113d96` | aarch64 unchecked | Change | x86-64 `9113d96` | x86-64 unchecked | Change |
|---|---|---|---|---|---|---|
| VINT | 8.13 | 7.18 | -11.8% | 8.74 | 6.81 | -22.0% |
| VREAL | 6.86 | 6.09 | -11.2% | 7.00 | 5.76 | -17.7% |
| VSTR | 12.82 | 13.02 | +1.6% | 11.53 | 11.60 | +0.7% |
| VARRAY | 11.85 | 10.15 | -14.3% | 12.44 | 9.37 | -24.7% |
| VDEF | 5.35 | 5.37 | +0.3% | 5.85 | 5.79 | -1.1% |
| SPHIT | 50.88 | 50.96 | +0.2% | 55.05 | 55.07 | 0.0% |
| TPUT | 31.60 | 31.54 | -0.2% | 51.63 | 51.16 | -0.9% |
| GAME | 19.94 | 19.42 | -2.6% | 28.28 | 27.51 | -2.7% |

Finding a place, aarch64 instructions an iteration, from the same traces
as the first table:

| Loop | `d081e7a` | `9113d96` | Unchecked |
|---|---|---|---|
| Empty | 45 | 48 | 23 |
| VINT's loop | 178 | 156 | 75 |
| Counters | 200 | 155 | 82 |
| Array | 259 | 232 | 104 |
| VREAL's loop | 237 | 210 | 104 |

VSTR and game 3 run many entries that read little, where the check before
each entry costs more than the reads save. `run_fast` shrinks from 46236
to 41436 bytes with the feature, as the panics of the bounds checks go.

### Runtime commands, from `7021a05`

This set asks where real games spend the program's part of a frame
outside the interpreter: the runtime's commands, the builtins that are not
fused, and the runtime's own work each frame. It ran eleven census
projects with their input scripts (`--bench --fixed --script`, start-up
and the first menus left out) and TPERF's scenes alone, on an x86-64
desktop loaded by other work. Times come from a scratch build that reads
the time stamp counter around each host call, builtin call, PRINT, CALL
SPRITE step, the sound, the animations and the VM; they move with the
load, so only their split is used. Counts come from `perf stat -e
instructions:u` on the `release` build: of whole runs, of loops of 3200
and 6400 iterations as in the [calls](#calls-from-48f7cfc) set, and of
`Machine::frame` alone over the measured frames, with perf's counters
switched on and off around it through `--control`. aarch64 counts come
from qemu's Cortex-A35 model on the `dist` build, as in the earlier sets.
Every project's and scene's checksum is the same before and after, and
TPERF's are `e8e5f6dc467db861` at 400 frames, `a6e1ed07824b1ad8` at
3600 and `5d9bb17a39eefb70` at 10500.

The program's part a frame at `7021a05`, x86-64 ms (CPU drawing is the
GPU's in the front end and is left out); the runs had no sound bank, so
the sound is the silent minimum (with one, TENSMA's sound takes 0.39 ms
of its 0.61):

| Project | Program | Interpreter | Commands and builtins | Sound, animations | Largest |
|---|---|---|---|---|---|
| S51_MEDAL4 | 4.15 | 2.65 | 1.47 | 0.02 | TOUCH OUT 0.74, BUTTON() 0.69 (25000 each a frame) |
| SOLIDGUNNER_V | 1.90 | 0.39 | 1.41 | 0.10 | GCLS 1.19 (1 a frame), SPVAR 0.11 (1000) |
| NEON_DRIFTWAY | 1.33 | 0.52 | 0.74 | 0.08 | ARYOP 0.16 (308), COPY 0.15 (171), GFILL 0.11, GLINE 0.09 (534), GCLS 0.08 |
| ELE2020 | 0.74 | 0.10 | 0.56 | 0.08 | GCOPY 0.42 (56 tinted glyphs), SPHITSP 0.05, GFILL 0.05, CALL SPRITE 0.03 |
| OTAKA_PAC | 0.59 | 0.39 | 0.15 | 0.06 | SPVAR 0.09 (961), CALL SPRITE 0.02 (732) |
| MOH_MOHAN | 0.50 | 0.00 | 0.48 | 0.02 | COPY 0.25, LOADG 0.11, GSAVE 0.10 (rare and large) |
| NK8_ROGUE2020 | 0.43 | 0.12 | 0.17 | 0.14 | GCOPY 0.13 (18) |
| FINITI | 0.31 | 0.07 | 0.23 | 0.02 | GFILL 0.12, GTRI 0.08, ARYOP 0.01 |
| MAMANATSU | 0.20 | 0.09 | 0.05 | 0.06 | CALL SPRITE 0.01, SPHITSP 0.01 |
| RYUSI | 0.10 | 0.06 | 0.01 | 0.04 | GCIRCLE |
| TENSMA | 0.07 | 0.04 | 0.01 | 0.02 | CLS |

What was made:

- `5d7aef8`: each page keeps a ground color, which every pixel outside a
  set of tiles drawn on since holds, and a plain fill in that color writes
  only those tiles. GCLS on a page with nothing drawn since went from
  2.66 million x86-64 instructions to 1600; on one drawn on in a 400x240
  corner it writes 28 of the 1024 tiles.
- `181b8f9`, `44821d3`: `Host::call_into` hands the runtime the list the
  VM keeps for a call's results, and the sprite, input, system and GRP
  groups put their values there instead of in a new `Vec` a call.
- `f00a654`: a tinted GCOPY of 256 pixels or more multiplies through
  tables of the tint's products, with a loop for each blend, and
  `#G_NORMAL2` passes clear pixels by untinted.
- `a066657`: GLINE draws its pixels as it works them out, in i64, instead
  of making a list and dividing i128s.
- `541ddd8`: GRP commands read their coordinates into arrays, not lists.
- `8912894`: SPVAR's and TVAR's maps hash with a multiply, not SipHash.
- `2970908`, `6ba52d7`: `Command::group` sends the sprite, input, system,
  GRP and text commands straight to their group's function.
- `cd278c3`: COPY of numbers into an array of their type copies slices.
- `e6921c5`: ARYOP reads real arrays other than its result in place and
  runs a loop for each operation.

Instructions an iteration:

| Loop | x86-64 `7021a05` | x86-64 `6ba52d7` | Change | aarch64 `7021a05` | aarch64 `e6921c5` | Change |
|---|---|---|---|---|---|---|
| `R=BUTTON(0)` | 908 | 779 | -14% | 864 | 694 | -20% |
| `TOUCH OUT S,T,R` | 1038 | 860 | -17% | 924 | 729 | -21% |
| `SPOFS 0,I AND 255,R` | 1015 | 956 | -6% | 929 | 869 | -6% |
| `R=SPUSED(I AND 63)` | 891 | 763 | -14% | 872 | 680 | -22% |
| `SPHIDE I AND 63` | 753 | 694 | -8% | 655 | 627 | -4% |
| `SPVAR I AND 63,1,R` | 1249 | 995 | -20% | 1112 | 905 | -19% |
| `GPSET I AND 255,7,R` | 1137 | 918 | -19% | 1110 | 851 | -23% |
| GFILL of 4x5 | 1694 | 1303 | -23% | 1588 | 1143 | -28% |
| GTRI, small | 6700 | 5828 | -13% | 5249 | 4576 | -13% |
| GLINE of up to 8 pixels | 2132 | 1663 | -22% | 2027 | 1438 | -29% |
| GLINE of up to 255 pixels | 11073 | 9931 | -10% | 9939 | 7814 | -21% |
| tinted 32x48 GCOPY, `#G_NORMAL2` | 70694 | 29975 | -58% | 54093 | 22113 | -59% |
| GCLS | 2661103 | 1601 | -99.9% | 2132619 | 1025 | -99.9% |
| `ARYOP 0,PX#,PX#,DX#`, 64 reals | 4483 | 3399 | -24% | 4313 | 3292 | -24% |
| `COPY PY#,PX#`, 64 reals | 7669 | 1007 | -87% | 6651 | 996 | -85% |
| `TPUT 0,I AND 15,3,"A"` | 1748 | 1742 | -0.3% | | | |
| `LOCATE I AND 31,3` | 825 | 801 | -3% | | | |

The program's part a frame (`Machine::frame`), x86-64 instructions in
millions, `7021a05` against `e6921c5`:

| Project | Before | After | Change |
|---|---|---|---|
| S51_MEDAL4 | 56.12 | 48.29 | -13.9% |
| NEON_DRIFTWAY | 13.45 | 11.64 | -13.4% |
| SOLIDGUNNER_V | 8.34 | 5.25 | -37.1% |
| OTAKA_PAC | 6.06 | 5.70 | -5.8% |
| MOH_MOHAN | 5.88 | 3.25 | -44.8% |
| ELE2020 | 5.35 | 3.30 | -38.4% |
| NK8_ROGUE2020 | 4.38 | 4.24 | -3.0% |
| FINITI | 2.51 | 2.02 | -19.4% |
| MAMANATSU | 1.95 | 1.87 | -4.3% |
| RYUSI | 1.17 | 1.17 | -0.3% |
| TENSMA | 0.69 | 0.68 | -0.7% |

Of TPERF's scenes the same way: GLINE 15.00 to 13.28 (-11.5%), GCOPY 10.08
to 9.33, GAME 8.57 to 8.02, GTRI 25.39 to 23.99, GFILL 6.64 to 6.29, SPROT
4.42 to 4.19; the interpreter's scenes within 0.05%. TPUT rose 1.2% at
`e6921c5`, which `6ba52d7` takes back. On aarch64, whole frames with the
CPU drawing over frames 3 to 6, millions: GLINE 20.42 to 17.58 (-13.9%),
GTRI 26.12 to 25.12, GCOPY 12.82 to 12.28, GAME 19.96 to 19.58, with the
same checksums.

Where the rest goes, for later work:

- **Sound** with a bank is most of what an idle game's frame costs:
  TENSMA's sound takes 0.39 of 0.61 ms on the desktop, about half of it
  in decoding ADPCM one sample at a time (`bank::Reader::next_of`) and the
  rest in the sampler's loop.
- **The interpreter's side of a host call**, about 150 aarch64
  instructions: the entry, `host_call`, the three calls through `dyn Host`
  around it for READ's place and a retry, and cutting the arguments.
- **GTRI** works out each edge's ends for each row with two divisions;
  stepping along the rows would leave them out.
- **CALL SPRITE** copies the callback's name for each sprite, and the VM
  looks the name up for each.
- **SPHITSP** and **GCIRCLE**, which make a list of points, at a few
  hundred to a few thousand instructions a call.

### Calls by name and leaf functions, from `7021a05`

This set counts what a DEF call costs where games make most of them: in
leaf functions of one expression, called hundreds of times a frame outside
any loop, and in callbacks of CALL SPRITE. Counts are taken as in the
[calls](#calls-from-48f7cfc) set: x86-64 with `perf stat -e instructions:u`
on the `release` build, aarch64 with qemu's Cortex-A35 model on the `dist`
build under `setarch -R`, from whole blocks. A loop's count is the
difference of programs that run it 3200 and 6400 times (recursion 800 and
1600, CALL SPRITE 20 and 40, the game loop 50 and 100), over the
difference. Every checksum is the same before and after: TPERF's
`e8e5f6dc467db861` at 400 frames and `a6e1ed07824b1ad8` at 3600 on both
machines, each scene's, and each game's on x86-64.

The loops are those of the [calls](#calls-from-48f7cfc) and
[variables](#variables-and-constants-from-d081e7a) sets, each in a DEF of
its own locals, and these:

| Loop | Statement |
|---|---|
| LINTP | `R=LINTP(I,R,3)`, `DEF LINTP(X1,X0,T): RETURN (X1-X0)*T+X0` |
| LINTP, constants | `X#=LINTP(X#,100,0.25)` |
| Five arguments | `R=LIN5(0,100,I AND 63,0,64)`, `RETURN A+(B-A)*(T-T0)/(T1-T0)` |
| Two locals | `R=L2(I,R AND 255)`, a DEF of two `VAR`s and three statements |
| Globals | `R=GG(I)`, `RETURN G+H+K` |
| One OUT as a function | `R=WG(I)`, `DEF WG NAME OUT N: N=G+NAME` |
| CALL SPRITE | 256 sprites with `SPFUNC K,"CB"`, `DEF CB: G=G+CALLIDX()` |

Where the aarch64 instructions of an iteration of the LINTP loop went at
`7021a05`, from qemu's trace, by fused entry (choosing each included):

| Part | Instructions |
|---|---|
| The call's entry: the stack's room, reading 3 arguments, the variables, the frame | 208 |
| The body's chain: 3 operators and pushing the value | 265 |
| RETURN's entry: the frame, the variables, the value on the stack | 82 |
| Storing the value in `R` | 55 |
| The FOR and its jump | 140 |
| An iteration | 750 |

So the call, its return and the store took 345, more than the body. A
callback of CALL SPRITE, `DEF CB: G=G+CALLIDX()` on 256 sprites, by the
function each instruction is in (inlined ones in the function they are
inlined into), aarch64 instructions a callback from qemu's counts of each
instruction:

| Part | `7021a05` | `b799de0` |
|---|---|---|
| `malloc`, `free`, `memcmp` | 728 | 165 |
| The runtime's `next_callback` and the VM's | 267 | 360 |
| The name: `slot_and_name`, the `String`, its hash | 458 | 0 |
| `run_callback`, `enter_def_on_stack` / `enter_found` | 308 | 156 |
| `step` and `run`, for the callback step, CALLIDX() and the call | 249 | 0 |
| The fused loop: the body, its RETURN, the jump back | 317 | 327 |
| Other | 198 | 17 |
| A callback | 2525 | 1025 |

What was made:

- `263fac8`: CALL and the callbacks keep the last 8 names that named a DEF
  of the running slot, with a weak reference to the slot's program, and go
  straight to the DEF when text, slot and program match. A call found
  that way that fits its DEF starts as the fused code starts one.
- `a1d2a12`: a string constant, CALLIDX() and the step to the next
  callback are fused entries. The step stops the fused loop, as a host's
  call does, and `run_fast` makes it with `step`'s code.
- `b799de0`: a chain takes in the call of a function whose body is one
  RETURN of numbers worked out from its arguments, the slot's globals and
  constants. The body's steps read the arguments where they are. Nothing
  of the call can be seen but its value: before its statement's last step
  a chain writes only its hidden places; a step that cannot do its work
  leaves the statement to `code`, which makes the call and fails there,
  on the body's line; the chain counts the body's instructions and its
  RETURN as run and runs whole or not at all, so slices end where they
  did; it runs only when the stack has room for its largest call
  (`FastOp::CallChain`), so a call that overflows is made by `code`; and
  the DEF has no variables but its arguments and calls nothing. Such a
  chain may end in pushing several values, as for
  `RGB(LINTP(..),LINTP(..),LINTP(..),LINTP(..))`, where RGB is not a
  builtin chains take.

After it, the LINTP loop is one chain of four steps and the room check,
about 200 instructions, and the FOR.

aarch64 instructions an iteration:

| Loop | `7021a05` | `b799de0` | Change |
|---|---|---|---|
| Empty | 130.9 | 130.8 | -0.1% |
| Function DEF | 624.2 | 265.9 | -57.4% |
| LINTP | 751.0 | 334.2 | -55.5% |
| LINTP, constants | 746.8 | 300.5 | -59.8% |
| Five arguments | 1208.1 | 518.1 | -57.1% |
| Globals | 603.9 | 283.9 | -53.0% |
| String argument | 624.8 | 249.8 | -60.0% |
| Array argument | 702.8 | 280.8 | -60.0% |
| Two locals | 876.0 | 879.1 | +0.4% |
| Locals | 677.0 | 672.0 | -0.7% |
| Command DEF | 531.5 | 528.2 | -0.6% |
| One OUT as a function | 607.1 | 600.0 | -1.2% |
| One OUT | 586.8 | 581.1 | -1.0% |
| Two OUTs | 752.1 | 748.1 | -0.5% |
| Recursion | 3551.5 | 3543.8 | -0.2% |
| `CALL` | 2806.1 | 1516.3 | -46.0% |
| GOSUB | 262.2 | 261.9 | -0.1% |
| CALL SPRITE (256 callbacks) | 652263 | 262425 | -59.8% |
| Game 1 (64 sprites) | 222405 | 222754 | +0.2% |
| VINT's loop | 429.0 | 429.0 | +0.0% |
| Reals | 536.5 | 538.0 | +0.3% |

x86-64 instructions an iteration:

| Loop | `7021a05` | `b799de0` | Change |
|---|---|---|---|
| Empty | 157.2 | 157.7 | +0.3% |
| Function DEF | 730.1 | 295.8 | -59.5% |
| LINTP | 875.2 | 372.6 | -57.4% |
| LINTP, constants | 872.3 | 337.9 | -61.3% |
| Five arguments | 1363.1 | 571.5 | -58.1% |
| Globals | 693.9 | 315.2 | -54.6% |
| String argument | 721.9 | 280.5 | -61.1% |
| Array argument | 814.9 | 308.4 | -62.2% |
| Two locals | 1004.3 | 1008.7 | +0.4% |
| Locals | 760.0 | 782.0 | +2.9% |
| Command DEF | 610.4 | 630.1 | +3.2% |
| One OUT as a function | 681.6 | 703.0 | +3.1% |
| One OUT | 663.7 | 683.4 | +3.0% |
| Two OUTs | 863.5 | 886.6 | +2.7% |
| Recursion | 4022.2 | 4094.2 | +1.8% |
| `CALL` | 2657.1 | 1582.2 | -40.5% |
| GOSUB | 302.6 | 307.5 | +1.6% |
| CALL SPRITE (256 callbacks) | 586240 | 249022 | -57.5% |
| Game 1 (64 sprites) | 245051 | 245258 | +0.1% |
| VINT's loop | 476.1 | 473.2 | -0.6% |
| Reals | 620.6 | 619.7 | -0.1% |

Calls that are not taken in move by -1.2 to +0.4% on aarch64 and by
+0.4 to +3.2% on x86-64, where LLVM laid out `run_fused` anew at
`b799de0`; leaving out the room check does not bring x86-64 back. The
string argument's DEF, `RETURN LEN(S$)`, is taken in, as LEN is a builtin
chains take.

Instructions a frame, in millions, program and CPU drawing, aarch64 over
frames 3 to 6 and x86-64 over frames 3 to 83:

| Scene | aarch64 `7021a05` | aarch64 `b799de0` | Change | x86-64 `7021a05` | x86-64 `b799de0` | Change |
|---|---|---|---|---|---|---|
| VINT | 8.13 | 8.14 | +0.2% | 8.74 | 8.67 | -0.8% |
| VREAL | 6.87 | 6.87 | -0.0% | 7.00 | 6.82 | -2.6% |
| VSTR | 12.83 | 12.24 | -4.6% | 11.53 | 10.97 | -4.8% |
| VARRAY | 11.84 | 11.80 | -0.4% | 12.44 | 12.20 | -1.9% |
| VDEF | 5.36 | 3.98 | -25.8% | 5.85 | 4.31 | -26.4% |
| VVAR | - | - | - | 33.47 | 33.67 | +0.6% |
| SPHIT | - | - | - | 55.05 | 55.11 | +0.1% |
| TPUT | - | - | - | 51.63 | 51.73 | +0.2% |
| GAME | 19.94 | 19.96 | +0.1% | 28.21 | 28.20 | -0.0% |

Two games, run headless on copies with `--bench --fixed` and the
same input script, x86-64 instructions a frame in play, in millions,
drawing included, over frames 700 to 1800 of a shooting game and 1200 to
2500 of a maze game (aarch64 was not counted, as qemu takes too long
over that many frames):

| Game | `7021a05` | `263fac8` | `a1d2a12` | `b799de0` | Change |
|---|---|---|---|---|---|
| Shooting game | 22.20 | 22.20 | 21.95 | 21.66 | -2.4% |
| Maze game | 23.98 | 22.71 | 22.34 | 22.39 | -6.6% |

The shooting game fades colors with a DEF that calls LINTP four times,
some 1100 calls a frame, and calls a one-OUT DEF that reads SPVAR with a
string constant some 840 times a frame. The maze game calls four DEFs by
CALL SPRITE on 240 to 490 sprites a frame, and has no leaf function
that is hot, so `b799de0` costs it what the calls not taken in cost on
x86-64. Their times were not compared: other builds shared the machine.

What calls still cost, for later work:

- A callback of CALL SPRITE, 1025 aarch64 instructions with the
  body: about 360 in the runtime's `next_callback`, which walks the
  sprites and copies the name into a new `Vec` each time (165 more in
  `malloc`, `free` and the comparison), and about 160 in `enter_found`.
  `Host::next_callback` handing out an `Rc<[u16]>`, with the sprites' and
  text screens' callbacks kept as one, would leave out the copy.
- A DEF that calls the host, such as `DEF WKGET NAME OUT N: N=SPVAR(..)`,
  is still called: its string argument is made and dropped on each call,
  SPVAR's value comes back in a `Vec`, and a command DEF's RETURN works
  out its OUTs from the DEF's description.
- A command DEF of one statement that writes a variable, `DEF P A,B:
  G=A+B`, could be made in a chain the same way, its store the statement's
  last step: no game measured here has one that is hot.
- On x86-64, the calls that are not taken in cost 20 instructions more
  since `b799de0`; aarch64 shows no change.

### Sound, from `c92ad81`

The sound is made on the thread that runs the program, a frame of 800
samples at a time, so its cost comes out of the frame. On the RK3326,
games spent 0.7 to 4.7 ms a frame outside the interpreter even when
nearly idle, and with the instrument bank imported from the console
nearly all of that was sound. This set ran with that bank.

Method: seven programs of our own (below) and five games with the input
scripts of the [runtime commands](#runtime-commands-from-7021a05) set,
`--bench --fixed`, `SB_BENCH_SOUND` set, the games over the frames of
play the device kit measured (TENSMA 500-649, RYUSI 300-449,
SOLIDGUNNER_V 700-849, NK8_ROGUE2020 650-799, MAMANATSU 2800-2949). x86-64 counts are `perf stat -e
instructions:u` on a scratch `release` build that switches the counters
on and off around `Sound::render_frame` through `--control`, over the
frames given; of SILENT's count after, a profile puts about 23 000 in
`render_frame` and the NaN check and the rest in the switch. Profiles are
`perf record` of the same window, flat. Times are the same build's
clock around `render_frame` and the VM, on one core, best of 3. aarch64
counts are qemu's Cortex-A35 model on the `dist` build under `setarch -R`,
from whole blocks: whole frames, as the difference of two run lengths.
Every sound checksum, screen checksum and TPERF checksum
(`e8e5f6dc467db861` at 400 frames and `a6e1ed07824b1ad8` at 3600 without
resources; the same with them, sound included) is the same before and
after, on x86-64 for all cases and on aarch64 for those given there.

| Case | What plays | Frames |
|---|---|---|
| SILENT | nothing, ever | 100-400 |
| IDLE2 | a preset song and a BEEP, stopped after a second | 200-400 |
| BGM1 | one preset song | 100-400 |
| MULTI | three preset songs, a preset effect every 10 frames | 100-400 |
| MML | 12 MML channels on instruments, drums included | 100-400 |
| REVERB | a preset song, effects every 20 frames, `EFCSET 2` | 100-400 |
| FADES | PSG and a WAVSET waveform, BGMPAUSE, BGMVOL, SNDMVOL fades | 100-400 |

Where TENSMA's sound went at `c92ad81`, by function, x86-64
instructions a frame (2.19 million in all; about 13 000 samples of
voices a frame, in 16 voices on average):

| Part | Instructions |
|---|---|
| The sampler's voice loop | 1 087 000 |
| `bank::Reader::next_of`, decoding DSP-ADPCM a sample at a time | 922 000 |
| `Sound::render_frame`, the mix and output | 59 000 |
| Tracks, BEEP, NaN check, envelopes and the rest | 120 000 |

A voice that sounds a whole frame cost about 84 instructions a sample
in the loop and 55 for each sample it decoded. An idle sound system
cost 94 000 a frame: the output loop a sample at a time, the NaN check,
and BEEP's synthesizer cleared and added in with no voice. Each track
that had played and stopped cost about 20 000 more, its synthesizer
cleared and its zeros added into the mix with its fade stepped: RYUSI's
idle tracks took 0.30 million. A preset effect, once its sequence had
ended, rendered its 60 frames of tail a sample at a time: in MULTI,
`Beeps::render`, `ceil`, `memset` and `Player::ended` together took 0.7
million.

What was made:

- `49bafe3`: an ended effect's tail goes as one run a frame.
- `bd2208c`: `Reader` decodes the rest of a 14-sample frame at once,
  the nibbles' parts before the chain of predictions, and hands samples
  out of a buffer.
- `bf91c27`: the voice loop takes samples from that buffer itself, with
  no call in the loop; around a call x86-64 keeps every float on the
  stack.
- `7560964`: `Synth::silent` lets tracks, BEEP and the mix leave out
  synthesizers with no voice and sources that would add zeros, and the
  loops for a gain that does not move have nothing else in them. The
  buffers start at +0 and only take sums, so they never hold -0, and
  adding a zero leaves them as they are.
- `3a04a87`: a sound system keeps what its notes decode, in chunks of
  1024 samples, at most 4 MiB, for the next notes of the same waveforms
  (`sound::decoded`). A waveform no note plays makes way, the one
  played longest ago first; with every waveform kept in use, a note
  decodes on alone. A loop takes the kept samples when its history is
  the first pass's, as for all 742 looping waveforms of the bank.
- `71f1e60`: PSG and WAVSET notes go as runs of a step of the envelope,
  their state in locals.

Sound's instructions a frame, x86-64, `c92ad81` against `71f1e60`:

| Case | Before | After | Change |
|---|---|---|---|
| SILENT | 93 600 | 30 900 | -67% |
| IDLE2 | 114 200 | 31 100 | -73% |
| BGM1 | 651 000 | 207 500 | -68% |
| MULTI | 2 691 600 | 526 700 | -80% |
| MML | 2 458 700 | 866 500 | -65% |
| REVERB | 1 883 000 | 729 700 | -61% |
| FADES | 1 273 600 | 711 200 | -44% |
| TENSMA | 2 191 700 | 659 900 | -70% |
| RYUSI | 688 700 | 144 000 | -79% |
| SOLIDGUNNER_V | 4 769 800 | 1 179 400 | -75% |
| NK8_ROGUE2020 | 1 918 100 | 477 600 | -75% |
| MAMANATSU | 895 200 | 230 400 | -74% |

The frame outside the interpreter (sound, animations, VSYNC and the
rest), x86-64 microseconds, `c92ad81` against `71f1e60`, with the sound
alone in brackets:

| Game | Before | After |
|---|---|---|
| TENSMA | 127 (125) | 40 (38) |
| RYUSI | 35 (34) | 9 (9) |
| SOLIDGUNNER_V | 280 (272) | 79 (71) |
| NK8_ROGUE2020 | 117 (114) | 32 (29) |
| MAMANATSU | 57 (54) | 17 (15) |

Whole frames on aarch64 (the program, the sound, and the bench's CPU
drawing), instructions a frame over the frames given, `c92ad81` against
`71f1e60`, each case's checksums the same before and after:

| Case | Frames | Before | After | Change |
|---|---|---|---|---|
| SILENT | 100-160 | 142 300 | 77 000 | -65 300 |
| IDLE2 | 200-260 | 162 600 | 77 300 | -85 300 |
| BGM1 | 100-160 | 637 000 | 251 800 | -385 300 |
| MULTI | 100-160 | 2 423 000 | 542 900 | -1 880 100 |
| MML | 100-160 | 2 102 900 | 936 000 | -1 166 900 |
| REVERB | 100-160 | 1 345 700 | 676 000 | -669 700 |
| FADES | 100-160 | 695 100 | 361 400 | -333 700 |
| TENSMA | 510-530 | 18 803 500 | 17 707 400 | -1 096 100 |

TENSMA's 1.1 million are 7 to 8% of what an A35 runs in a frame. The sound
checksums of MULTI, MML, REVERB, FADES, SOLIDGUNNER_V and NK8_ROGUE2020
over their whole runs are the same on aarch64 as on x86-64, before and
after. TENSMA's differs between the two machines, the same before and
after. Its notes use `powf`, `sin` and `exp2` more than the others',
and the two C libraries' last bits may differ; this was not followed
further.

The decoded waveforms took at most 3.5 MB in TENSMA, 1.3 MB in MML,
0.96 MB in SOLIDGUNNER_V and 31 KB in NK8_ROGUE2020; no chunk was
refused for the budget. A voice now costs about 41 x86-64 instructions a
sample, and the voice loop is 82% of TENSMA's sound.

Where the rest goes, for later work:

- **The voice loop**, about 30 instructions a sample of each voice in
  its loop: interpolating, the gains' slide, and adding into the mix,
  whose loads and stores take a third of it. Mixing two to four voices in
  one pass over the buffers would share the loads and stores, with each
  voice's sums as they are.
- **PSG and WAVSET notes**, 0.46 of FADES's 0.71 million: the filter's
  four taps, and converting the position, an `f64`, to an index and a
  phase each sample.
- **The reverb**, 0.32 of REVERB's 0.73 million while `EFCEN` is on, a
  sample at a time over six taps, four combs and two all-passes a side,
  and 0.15 million more in `render_frame` for the effector's buses. A
  tail that has died to exactly zero could be left out until something
  is sent again.
- **The first note of a waveform** decodes a chunk ahead, about 10 000
  instructions, where it decoded 14 samples before.

### Games on the handheld, `7021a05` to `dac7ad2`

The runtime command, call and sound passes together, timed on the
Cortex-A35 handheld with the imported instrument bank: program time a frame
in milliseconds, the median over each project's measured frames, best of
three rounds with the two builds alternating. Both are `dist` builds with
`unchecked-vars`, run without drawing, pinned to one core with the front end
stopped. Every project's checksum is the same in both builds.

| Project | `7021a05` | `dac7ad2` | Change |
|---|---|---|---|
| S51_MEDAL4 | 127.03 | 91.96 | -28% |
| NEON_DRIFTWAY | 25.23 | 20.60 | -18% |
| SOLIDGUNNER_V | 13.71 | 7.14 | -48% |
| OTAKA_PAC | 9.69 | 6.32 | -35% |
| ELE2020 | 7.80 | 5.03 | -35% |
| NK8_ROGUE2020 | 5.46 | 4.41 | -19% |
| FINITI | 4.97 | 3.36 | -32% |
| MAMANATSU | 3.27 | 2.44 | -25% |
| TENSMA | 3.09 | 1.89 | -39% |
| RYUSI | 1.81 | 1.25 | -31% |
| MOH_MOHAN | 0.70 | 0.67 | -4% |

The run does not separate the three passes. S51_MEDAL4 and NEON_DRIFTWAY
still take more than a 16.7 ms frame.

## Tried and left out

Each of these was measured and not kept.

- **The sampler's voice state in locals, with the reader's call left in
  the loop**: TENSMA's sound went from 1.45 to 1.55 million x86-64
  instructions a frame, as the floats still went to the stack around
  the call. Taking the decoded samples in the loop and calling the
  reader outside it (`bf91c27`) gave 1.23 million (2026-11-17).
- **The table in a variable's lowest bit** instead of its highest, to find
  the table's place in one instruction fewer: on top of `cc60440`, loops
  moved by -2.3% (VARRAY's) to +2.7% (an empty FOR); on top of the
  constants, 5 of 8 loops came out worse, by up to 3.5% (a GOSUB loop). LLVM
  lays out `run_fused`'s registers anew, which outweighs the instruction
  (2026-11-16).
- **A write that drops a string out of line**, a cold call to clear the
  place and then `mem::forget(mem::replace(..))` for the stores, to keep
  the drop's code out of the 70 or so places that write a number:
  `run_fast` 432 bytes smaller, but VINT's loop 424 to 465 aarch64
  instructions and VARRAY's 1124 to 1202, as the call kept values out of
  registers (2026-11-16).
- **Bounds checks folded in safe Rust.** For `v[i & (v.len() - 1)]` LLVM
  keeps a test of the length for zero; with the mask kept elsewhere it
  keeps the whole check. Only an index into a fixed-size array,
  `&[Value; 32768]` with the index masked to 15 bits, has none, which would
  mean 512 KB for each slot's globals, and padding every DEF call's
  variables to the same, as a call's variables follow each other in one
  list (2026-11-16).
- **Steps typed by suffix**, for `A%=B%+1`: a suffix only sets a
  variable's starting value, so the tag check stays (2026-11-16).
- **Handing a builtin's or host's call back from `run_fused` with its id
  and counts**, in an enum: LLVM kept copies of the entry's tag in six
  registers for it and the table of entries on the stack, about 6 aarch64
  instructions more to choose every entry. An empty FOR took 143 instead
  of 132, a GOSUB loop 305 instead of 279; `run_fast` read the call from
  the entry instead (2026-11-16).
- **LLVM's own inlining of `Vec::truncate`** where a fused DEF return
  drops the callee's variables: 0.7 KB more of `run_fast` for the drop of
  strings and arrays, which a return rarely has to do. `cut` checks that
  what goes is numbers and leaves the rest to a function of its own
  (2026-11-16).
- **The room check of a chain that makes calls out of line**, a function
  of its own that reads the frames: the LINTP loop of the
  [leaf functions](#calls-by-name-and-leaf-functions-from-7021a05) set
  went from 372 to 395 x86-64 instructions an iteration and the function
  DEF's from 297 to 317, and the calls not taken in were no cheaper
  (2026-11-17).

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
  [building-handheld.md](../building/building-handheld.md), added in `bc19803`, provides
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
- **ARYOP reading every array in place**, the result too, with a match
  on each parameter's type for each element: `ARYOP 0,PX#,PX#,DX#` 4482
  to 5575 x86-64 instructions an iteration, against 3399 for reading
  only other real arrays in place and a loop for each operation
  (2026-11-17).
- **A fast path for the sprite-number check** before SPOFS, SPROT and
  SPSCALE: it saved about 5 aarch64 instructions a call, and SPROT's time
  on the device did not change (2026-11-16).
- **Comparing scenes to skip a frame:** the emulator draws a frame only after
  a command changed what shows. Of about 1100 frames each in five projects, 0
  to 3 had a scene, pages and cells like the last, so a hash would cost every
  frame for nothing.
