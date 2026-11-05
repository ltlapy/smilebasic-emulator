# Benchmarking

Rendering and running a program take most of the emulator's time, and
small differences between builds matter on the low-end machines it aims
at. Timing alone cannot show them: moving code around changes times by a
few percent with no change in the work done.

## Which build

`cargo build --release` is the everyday build. `cargo build --profile dist` is what
ships, with whole-program optimization, in `target/dist`. Its numbers are the ones a
user gets, so give them too when reporting a result; compare `release` with `release`
while working, as a `dist` build takes twice as long. Against `release`, on one x86-64
desktop: cycles TPRINT -16%, the BASIC scenes -8 to -10%, LCOMP -6.5%, ELE2020 -5.5%;
the check binary 6.02 to 5.07 MB.

## Comparing two builds

Count instructions on a run that does the same work each time:

```
perf stat -e instructions:u -- smilebasic-check --bench --fixed 400 PROJECT PROGRAM
```

`--fixed` runs on a fresh copy of the project and those beside it, makes
`MILLISEC`, `DATE$` and `TIME$` follow the frame count from a set date,
makes `PERFEND` give 0, and starts the random series from set seeds.

- Compare the `checksum` lines first. Different checksums mean the two runs
  did different work, so their counts say nothing. The checksum covers the
  console and every 60th screen.
- Run it with `FRAMES` 0 to see what start-up costs, and subtract that from
  both counts, so that loading the font and sounds does not dilute the change.
- Take times as well: instruction counts do not see cache misses or slow
  instructions. `--bench` without `--fixed` measures as a program normally runs.

## What it does not fix

- Files keep the dates they are copied or saved with.
- A program that waits for input does the same work, but only what it does
  without input.
- Hash tables and addresses change a little from run to run, which moves the
  count by a tiny amount. Compare it with what two runs of one build differ by.

## Comparing with SmileBASIC 4

`bench/TPERF.txt` is a program of our own that runs the same way on the
emulator and on SmileBASIC 4, so its numbers compare directly. It runs 32
scenes in order, each isolating one kind of work (the BASIC itself,
sprites, text, GRP, layers, a small game with bursts of extra load), and
takes about four minutes. For each it shows and saves the time the
program spent between `VSYNC`s (`WORK`), the time from `VSYNC` to `VSYNC`
(`FRAME`) and the frames `MAINCNT` lost. Two more scenes raise the number of
sprites and of triangles until 60 fps cannot be held, and give that as a
score.

Pack it as the file `TPERF`, as [hardware-tests.md](hardware-tests.md)
says for check programs. Run `PERF` from a project and answer yes when it asks to
save: the results go into the text file `RESPERF`. `#ONLY`, at the top of
the program, runs one scene alone, and `#LOADPCT` scales the load. Compare
only runs with the same values.

- On SmileBASIC 4 the times are real, and `FRAME` is about 16.7 ms until the
  load is too much.
- A `--bench` run does not wait for the display, so `FRAME` is what the frame
  really cost, drawn on the CPU, and can be under 16.7 ms. Use it to compare
  builds.
- The frontend keeps to 60 frames a second by leaving out drawing and, far
  behind, dropping time. So its `FRAME` and `LOST` hide much of what drawing
  costs: they show what a person would see, not the cost.
- `--bench --fixed` gives 0 for every time and leaves out the two score
  scenes, which go by the clock: count instructions, as above, over the
  10500 frames it takes to reach the table.
- `--play` draws only the frames a script asks for, so `FRAME` leaves
  drawing out. Use it to see the table, not for its numbers.
- The scores go by the clock, so they move between runs of one build. The
  search stops within a sixteenth of the score.

## Per-frame costs and overload

A mean hides what a player sees as stutter. `--log FILE` writes each frame's
run and draw times, instructions and VSYNCs as CSV. `--script FILE` gives the
program input in the form `--play` takes, so that a run reaches the states
that need it; with `--fixed`, this still does the same work each time.

On a busy machine, wall-clock times swing with other processes' load. With
`--fixed`, run it a few times and take each frame's minimum: the frames line
up, so this leaves the frame's own cost.

`--window` runs frames as the window does: to the clock, with its time
limits, catching up without drawing. `--slower N` divides that clock and its
limits by N, to see how a machine N times slower would go. It has no window,
so no upload, scaling or present is included.

## The GPU path

Without `SB_GPU`, the setting in File > Settings > Screen decides. `SB_GPU=0` keeps to
the CPU, `SB_GPU=1` draws with the GPU where it can, `SB_GPU=sync` waits for it each
frame to time its work, and `SB_GPU=check` also draws on the CPU and counts the pixels
that differ; see `crates/sb-sdl/src/gpu.rs` and
[renderer-design.md](renderer-design.md). With `SB_UI_BENCH` it reports who drew and why
any frame stayed on the CPU, and `at=FRAME:WHAT` does things to the frontend, resets,
resizes and the setting among them, to try the path's answers. `end` stops when the
program ends, `settle=N` skips frames first and `novsync` turns vsync off.

`luma=PATH` logs each frame's brightness as it goes to the window and reports the frames
darker than both neighbours, a flicker, which the comparison cannot see. `log=PATH` keeps
each frame's stages, window size, textures made and the rest of its trip round the loop. `at=FRAME:lost`, `fail`, `refuse:N`,
`panic`, `unsaved` and `sleep:MS` try the renderer's failures.

`SDL_VIDEO_DRIVER=offscreen` shows no window
and, through EGL, still reaches the GPU, but leaves real presents out of the numbers.

A real swapchain needs a compositor, but not a screen: `mutter --headless --no-x11 --wayland
--wayland-display=NAME --virtual-monitor 3840x2160` under `dbus-run-session`, with its own
`XDG_RUNTIME_DIR` and `SDL_VIDEO_DRIVER=wayland`, gives the emulator a real window, with
libdecor's decorations, on the real GPU, seen by nobody. Its `org.gnome.Mutter.RemoteDesktop`
takes keys and pointer moves, so Alt+F8, the arrow keys and pointer moves make an interactive
resize. `at=N:resize:W:H` only sets the size, which leaves out most of what a drag costs.

What a drag costs there, on an RTX 3060 Ti with `opengl`, per size change at 2000 and 3200 wide:

| Where | 2000 wide | 3200 wide | What it is |
|---|---|---|---|
| `events` | 9.5 ms | 19.5 ms | libdecor's GTK plugin draws the decorations again for each configure, with a shadow buffer the size of the window, inside SDL's event pump |
| `screen` | 4 ms | 8 ms | the driver, in the first GL call after the size changed: a bare `glFlush` there takes all of the growth and leaves 0.2 ms |
| `present` | 5.6 ms | 10 ms | the driver and compositor |

No texture of ours is made per size. Mutter sends up to 60 configures a second and all that are
queued are dispatched in one pump, so once the cost of one nears 16.6 ms, as it does with big
windows or a busy CPU, the backlog feeds itself and single pumps take seconds, with the program
and its sound on the same thread. Turning vsync off during the drag or skipping drawings after a
long pump made no difference under a fixed load; only a cheaper configure did (libdecor's cairo
plugin, by `LIBDECOR_PLUGIN_DIR`, at 55 frames a second against 49, but it looks different).
SDL has a way out when built against a libdecor with the resizing state (0.3).

## The CPU path on weak machines

Where the GPU path is not available, as on old PCs or a handheld with a small Arm
core, the CPU draws everything. Such a core runs instructions in order, two at most
at a time, and floating point, cache misses and missed branches cost it far more than
they cost a desktop. So instruction, cache-miss and branch-miss counts say more here
than a desktop's times; a wide core hides much of what falls in them. As a rule of
thumb, not measured: a 1.5 GHz Cortex-A35 at about 0.8 instructions a cycle has about
20 million instructions for a frame, to draw, run the program and make the sound.

One x86-64 desktop, release build, `--bench --fixed`, 200 frames, 1200 for the
projects, one core, start-up included (0.22 G instructions), before and after this
pass; pixels the same in each:

| Scene | Instr G | Cycles change | Cache misses M | Draw ms |
|---|---|---|---|---|
| TMULTI | 206.7, 42.0 | -86% | 120, 26 | 94.8, 12.4 |
| SPANIM | 107.6, 63.2 | -44% | 297, 367 | 32.9, 18.3 |
| SPROT | 90.4, 60.2 | -39% | 437, 400 | 27.0, 16.3 |
| LCOMP | 141.3, 47.6 | -61% | 66, 62 | 34.5, 13.3 |
| LBLUR | 49.7, 46.8 | -7% | 56, 62 | 11.7, 10.9 |
| LMATRIX | 45.6, 44.2 | -3% | 635, 622 | 13.1, 12.6 |
| GAME | 13.0, 10.5 | -12% | 26, 24 | 1.7, 1.3 |
| TPRINT | 19.9, 13.3 | -23% | 44, 34 | 6.0, 4.5 |
| ELE2020 | 224.8, 165.6 | -26% | 296, 307 | 11.5, 8.7 |
| SOLIDGUNNER_V | 37.6, 26.5 | -23% | 204, 189 | 1.1, 0.7 |
| DVD_BOUNCE | 2.8, 2.6 | -10% | 25, 8 | 0.11, 0.10 |
| VINT | 8.4, 7.2 | -2% | 3.3, 1.1 | |

TPERF whole, 10500 frames: instructions -40%, cache misses -28%. The MAXSP score
went from 838 to 1489 sprites in one run each, on a machine doing other work.

What costs what, after this:

- **Blending** is a pixel's main cost where anything is translucent, at 15 to 40
  instructions: the sums round once, over 255 or its powers. The loops are scalar
  sums in lanes of a 64-bit word. Two forms written for the compiler to vectorize
  gained nothing on baseline x86-64 (+36% and about 0%), so they were dropped; NEON
  was not tried.
- **Placing** a turned pixel takes about 15 integer instructions now, in fixed point,
  with the exact sum where it might round the other way (`draw_texels`).
- **Through a 4x4 matrix**, one division in 16 pixels: what a pixel reads steps along
  the curve in fixed point, with its second difference, and only a pixel too near a
  whole number gets a sum of its own (`Along`).
- **Composed layers** take a clear and a pass over only the box of what they hold, unless
  a filter or a 4x4 matrix is at work. A full-frame sprite still means 3.5 MB each way
  at 1280x720, and a color on a layer takes three multiplications a channel unless the
  pixel is like the last, as it mostly is.
- **Text** keeps each screen's characters from frame to frame, up to 4 MiB a screen, for
  as long as the font page, the TPAGE page and its GSAMPLE, TCOLOR, TBLEND and the size
  hold. A new color on a character still takes making it.
- **A collision search** reads an array of boxes, brought up to date for the sprites
  changed since the last search.
- **The interpreter** takes about 20 instructions an operation on numbers, in
  `crates/sb-core/src/fuse.rs`'s larger steps, with up to 18 of each step on choosing
  the step. Strings, calls and anything that may fail go one instruction at a time, as
  they do whenever a debugger, TRACE or a breakpoint is about: about 8 times slower, at
  about 140 million instructions a second. A call costs a few hundred.
- **Sound** is small: 16 voices of a bank instrument at 3.5 million instructions a
  frame, in runs of samples between envelope steps; reverb 0.4 million more.
- **Memory:** peak RSS is about 180 MiB, 158 of it from start-up. A sprite is now about
  360 bytes, and free ones are not walked.
- **Uploading** a frame is a copy of 3.5 MB at 1280x720, 0.4 MB at 400x240. Its cost on
  a slow memory bus was not measured.

The interpreter's own pass, on the same desktop, 100 to 300 frames, one core, start-up
included, against the commit before it; what they run and the checksums are the same:

| Scene | Instr G, release | Cycles, release | Branch misses | Instr G, dist | Cycles, dist |
|---|---|---|---|---|---|
| VINT | 3.71, 1.44 | -72% | 0% | 1.40 | -72% |
| VREAL | 3.18, 1.52 | -57% | -1% | 1.48 | -59% |
| VARRAY | 5.15, 2.39 | -66% | 0% | 2.35 | -64% |
| VDEF | 2.17, 1.52 | -37% | 0% | 1.44 | -42% |
| VSTR | 1.64, 1.42 | -19% | 0% | 1.36 | -25% |
| GAME | 10.50, 7.64 | -39% | -26% | 7.21 | -46% |
| S51_MEDAL4 | 38.88, 31.96 | -37% | -5% | 29.55 | -43% |
| FINITI | 17.52, 14.78 | -17% | -24% | 14.22 | -28% |
| NEON_DRIFTWAY | 2.22, 1.58 | -26% | -8% | 1.52 | -29% |

Programs that spend their time drawing change by what the interpreter was of them:
ELE2020 and BASICDEMO none, RYUSI and SOLIDGUNNER_V 1 to 2% of instructions.

A second pass, from 28cf171, in the same conditions; pixels and sound the same to the
bit. Here `SB_BENCH_SOUND` adds a checksum of the sound to a fixed run:

| Scene | Instr G | Cycles change | Cache misses M | Branch misses M |
|---|---|---|---|---|
| LMATRIX | 44.2, 32.1 | -33% | 623, 505 | 16.1, 17.4 |
| LRASTER | 25.3, 19.6 | -32% | 74, 65 | 7.8, 7.3 |
| TPRINT | 13.3, 8.8 | -29% | 34, 14 | 39.9, 31.2 |
| SPHIT | 13.1, 11.5 | -21% | 34, 17 | 23.3, 20.1 |
| SND16 | 1.15, 0.95 | -9% | 0.7, 0.7 | 1.4, 1.4 |
| SPMOVE | 18.7, 18.3 | -2% | 33, 41 | 6.3, 5.8 |
| GAME | 10.5, 10.4 | -3% | 22, 22 | 6.9, 6.6 |
| ELE2020 | 165.6, 164.1 | -3% | 354, 315 | 107.7, 106.0 |
| SOLIDGUNNER_V | 26.5, 26.2 | -4% | 181, 188 | 17.2, 16.4 |
| NEON_DRIFTWAY | 297.0, 293.4 | +0.5% | 387, 429 | 218.1, 218.3 |

Seven composed layers of eight 32x32 sprites each, 100 frames: instructions -76%, draw
1.16 ms to 0.30 ms. TPERF whole: instructions -4.7%, cycles -5%, cache misses -8%.

Tried and left out, measured:

- **The memos on missed branches.** SPANIM's and LCOMP's branch misses come from working
  out a paint or a blend only when the pixel differs from the last. Without them,
  SPANIM has branch misses -47% and instructions +28%, LCOMP -66% and +89%, with cycles
  +126%. An in-order core spends few cycles on a miss and at least one on
  each instruction, so they stay (inferred for such a core).
- **Blending plain layers with SSE2**, in lanes with the same sums: instructions -3 to -8%
  on layer scenes, cycles -1 to -8%, branch misses -9 to -20%. It would be the first
  `unsafe` outside `sb-sdl` for a gain on x86 alone; NEON was not tried, as no Arm
  target was available to build for.
- **Comparing scenes to skip a frame:** a frame is drawn only after a command changed
  what shows. Of about 1100 frames each in five projects, 0 to 3 had a scene, pages and
  cells like the last, so a hash would cost every frame for nothing.

The rest of the options, with what each would give up, are for whoever picks up
the handheld port. To measure on a device: cross-build `smilebasic-check` for its
target ([building-handheld.md](building-handheld.md)), copy the program over as a
project, and run the same `--bench --fixed` lines under `perf stat` where it exists, or
`--bench` for times where not.
