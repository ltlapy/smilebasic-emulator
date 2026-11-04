# Benchmarking

Rendering and running a program take most of the emulator's time, and
small differences between builds matter on the low-end machines it aims
at. Timing alone cannot show them: moving code around changes times by a
few percent with no change in the work done.

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
resizes and the setting among them, to try the path's answers. `SB_UI_BENCH`'s `end` stops when the program ends, `settle=N` skips
frames first and `novsync` turns vsync off. `SDL_VIDEO_DRIVER=offscreen` shows no window
and, through EGL, still reaches the GPU, but leaves real presents out of the numbers.

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
- **Through a 4x4 matrix**, two divisions a pixel. This is LMATRIX's 81%.
- **Composed layers** take a clear and a pass over the frame each, 3.5 MB each way at
  1280x720, whatever they hold, and a color on a layer takes three multiplications
  a channel unless the pixel is like the last, as it mostly is.
- **Each element** allocates for its grid, about 7% of SPMOVE.
- **The interpreter** takes about 55 instructions an operation and 24 cycles, with a
  large share of the samples on moving a 16-byte value just written by its parts.
- **Sound** is small: 16 voices of a bank instrument at 4.8 million instructions a
  frame, reverb 0.4 million more.
- **Memory:** peak RSS is about 180 MiB, 158 of it from start-up. A sprite is now about
  360 bytes, and free ones are not walked.
- **Uploading** a frame is a copy of 3.5 MB at 1280x720, 0.4 MB at 400x240. Its cost on
  a slow memory bus was not measured.

The rest of the options, with what each would give up, are for whoever picks up
the handheld port. To measure on a device: cross-build `smilebasic-check` for its
target, copy the program over as a project, and run the same `--bench --fixed` lines
under `perf stat` where it exists, or `--bench` for times where not.
