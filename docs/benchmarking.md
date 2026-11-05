# Benchmarking

Rendering and running a program take most of the emulator's time, and
small differences between builds matter on the low-end machines it aims
at. Timing alone cannot show them: moving code around changes times by a
few percent with no change in the work done.

## Which build

`cargo build --release` is the everyday build. `cargo build --profile dist` is what
ships, with whole-program optimization, in `target/dist`. Its numbers are the ones a
user gets, so give them too when reporting a result; compare `release` with `release`
while working, as a `dist` build takes twice as long.

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

## Measuring on a handheld

The rest of the options, with what each would give up, are for whoever picks up
the handheld port. To measure on a device: cross-build `smilebasic-check` for its
target ([building-handheld.md](building-handheld.md)), copy the program over as a
project, and run the same `--bench --fixed` lines under `perf stat` where it exists, or
`--bench` for times where not.
