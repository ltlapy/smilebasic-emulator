# Benchmarking

Rendering and running a program take most of the emulator's time, and small
differences between builds matter on the low-end machines it aims at. Timing
alone cannot show them: moving code around changes times by a few percent with
no change in the work done.

This file says how to measure. [benchmark-results.md](benchmark-results.md)
holds what was measured.

## Which build

`cargo build --release` is the everyday build. `cargo build --profile dist` is
what ships, with whole-program optimization, in `target/dist`. Its numbers are
the ones a user gets, so give them too when reporting a result. Compare
`release` with `release` while working, as a `dist` build takes twice as long.

## Comparing two builds

Count instructions on a run that does the same work each time:

```
perf stat -e instructions:u -- smilebasic-check --bench --fixed 400 PROJECT PROGRAM
```

`--fixed` runs on a fresh copy of the project and those beside it, makes
`MILLISEC`, `DATE$` and `TIME$` follow the frame count from a set date, makes
`PERFEND` give 0, and starts the random series from set seeds.

- Compare the `checksum` lines first. Different checksums mean the two runs
  did different work, so their counts say nothing. The checksum covers the
  console and every 60th screen. With `SB_BENCH_SOUND` set, a fixed run also
  prints a checksum of the sound mixed, to the bit. `--wav FILE` saves that
  sound to listen to (see [hardware-tests.md](hardware-tests.md#recording-sound)).
- Run it with `FRAMES` 0 to see what start-up costs, and subtract that from
  both counts, so that loading the font and sounds does not dilute the change.
- Take times as well: instruction counts do not see cache misses or slow
  instructions. `--bench` without `--fixed` measures as a program normally
  runs.

With `BENCH_DEBUG` set, a `--bench` run ends by listing the sprites shown and
the large ones among them, the text screens, the layers and the screen mode.

## What `--fixed` does not fix

- Files keep the dates they are copied or saved with.
- A program that waits for input does the same work, but only what it does
  without input.
- Hash tables and addresses change a little from run to run, which moves the
  count by a tiny amount. Compare it with what two runs of one build differ
  by.

## Comparing with SmileBASIC 4

`bench/TPERF.txt` is a program of our own that runs the same way on the
emulator and on SmileBASIC 4, so its numbers compare directly. It runs 32
scenes in order, each isolating one kind of work (the BASIC itself, sprites,
text, GRP, layers, a small game with bursts of extra load), and takes about
four minutes. For each scene it shows and saves:

- `WORK`: the time the program spent between `VSYNC`s
- `FRAME`: the time from `VSYNC` to `VSYNC`
- `LOST`: the frames `MAINCNT` lost

Two more scenes raise the number of sprites and of triangles until 60 fps
cannot be held, and give that as a score.

Pack it as the file `TPERF`, as [hardware-tests.md](hardware-tests.md) says
for check programs. Run `PERF` from a project and answer yes when it asks to
save: the results go into the text file `RESPERF`. `#ONLY`, at the top of the
program, runs one scene alone, and `#LOADPCT` scales the load. Compare only
runs with the same values.

What the numbers mean depends on where TPERF runs:

- On SmileBASIC 4 the times are real, and `FRAME` is about 16.7 ms until the
  load is too much.
- A `--bench` run does not wait for the display, so `FRAME` is what the frame
  really cost, drawn on the CPU, and can be under 16.7 ms. Use it to compare
  builds.
- The frontend keeps to 60 frames a second by leaving out drawing and, far
  behind, dropping time. So its `FRAME` and `LOST` hide much of what drawing
  costs: they show what a person would see, not the cost.
- `--bench --fixed` gives 0 for every time and leaves out the two score
  scenes, which go by the clock. Count instructions, as above, over the 10500
  frames it takes to reach the table.
- `--play` draws only the frames a script asks for, so `FRAME` leaves drawing
  out. Use it to see the table, not for its numbers.

The scores go by the clock, so they move between runs of one build. The search
stops within a sixteenth of the score.

## Running at a console's speed

File > Settings > General > Speed, the `speed` line of the settings file and
`smilebasic-check --speed switch1` (or `switch2`) run programs at about a
console's speed, for checking that a program written on a PC keeps up on the
console. The default, Unlimited, runs as before.

At a console's speed, the emulator charges what a program does in the
console's time and gives each frame a sixtieth of a second of it:

- Each VM instruction and statement, DEF call, builtin call, array element,
  string join and look-up by name (`VAR()`, `CALL`, `GOTO` with a string)
  costs a set amount. A FOR's test gets a credit, as `NEXT` takes many of our
  instructions but one step of SmileBASIC 4's.
- Each host command costs by its kind: drawing by the pixels inside `GCLIP`,
  `TFILL` and `SCROLL` by the cells, `PRINT` by the characters, sprite
  commands by a set amount each. Sprites, layers and text screens cost nothing
  to show, as TPERF finds on both consoles.
- A program that needs more than the frame goes on in the next one, and its
  `VSYNC` finds its frame gone by, as on the console. `MAINCNT`, `VSYNC` and
  the frames lost come out as there.
- `PERFBEGIN` and `PERFEND` measure the console's time, which is the same on
  every run, also with `--fixed`. TPERF's `WORK` then compares with a
  console's directly, and its two score scenes run.

The window keeps its own time limit as well, so on a computer slower than the
console the program gets less.

Counting this work takes the VM off its fused fast path, which a console's
speed leaves plenty of time for. Unlimited counts nothing and runs exactly as
before.

The costs come from TPERF's scenes and TEST65's loops on 4.4.9 (in
`crates/sb-runtime/src/speed.rs`). TPERF's `WORK` in ms, run with
`smilebasic-check --speed` and `--play`:

| Scene | Switch 1 | ours | Switch 2 | ours |
|---|---|---|---|---|
| VINT | 13.38 | 13.17 | 5.15 | 5.06 |
| VREAL | 8.42 | 8.41 | 3.29 | 3.29 |
| VSTR | 5.42 | 5.42 | 2.28 | 2.28 |
| VARRAY | 20.96 | 18.92 | 7.43 | 6.74 |
| VDEF | 7.83 | 7.83 | 2.61 | 2.61 |
| VVAR | 7.24 | 7.24 | 2.98 | 2.97 |
| SPMOVE | 7.32 | 7.21 | 2.28 | 2.31 |
| SPROT | 3.77 | 4.05 | 1.28 | 1.27 |
| SPCHURN | 8.50 | 8.46 | 3.72 | 3.66 |
| SPHIT | 6.05 | 6.07 | 2.60 | 2.59 |
| SPANIM | 0.21 | 0.19 | 0.09 | 0.09 |
| TPRINT | 2.35 | 2.32 | 1.15 | 1.15 |
| TPUT | 11.04 | 11.04 | 3.82 | 3.81 |
| TSCROLL | 7.09 | 7.14 | 3.62 | 3.63 |
| TMULTI | 0.31 | 0.31 | 0.18 | 0.18 |
| GFILL | 15.33 | 15.13 | 2.78 | 2.72 |
| GALPHA | 27.93 | 27.80 | 11.03 | 10.99 |
| GLINE | 7.60 | 7.55 | 3.13 | 3.15 |
| GCIRCLE | 3.22 | 3.22 | 1.28 | 1.28 |
| GTRI | 8.76 | 8.65 | 2.83 | 2.80 |
| GPUTCHR | 5.71 | 5.65 | 1.62 | 1.60 |
| GCOPY | 11.11 | 11.01 | 3.33 | 3.31 |
| GLOAD | 1.08 | 1.08 | 0.45 | 0.45 |
| GUPLOAD | 0.03 | 0.01 | 0.01 | 0.01 |
| GARRAY | 11.69 | 10.63 | 4.13 | 3.91 |
| LCOMP | 0.45 | 0.43 | 0.17 | 0.13 |
| LBLUR | 0.22 | 0.20 | 0.09 | 0.08 |
| LMOSAIC | 0.20 | 0.20 | 0.09 | 0.08 |
| LCOLOR | 0.26 | 0.27 | 0.11 | 0.11 |
| LMATRIX | 0.28 | 0.28 | 0.12 | 0.11 |
| LRASTER | 1.21 | 1.07 | 0.49 | 0.40 |
| GAME | 7.70 | 7.57 | 3.02 | 2.87 |
| MAXSP (score) | 2866 | 3197 | 4000 | 4000 |
| MAXTRI (score) | 4410 | 4410 | 13471 | 13007 |

`LOST` matches too: GALPHA lost 202 frames on a Switch 1 and 200 here, VARRAY
77 and 40. Most scenes come within 10%. A loop of one kind of statement can be
further off: TEST65's loops go from 9% fast (`CASE` on a Switch 1, `GOTO` on a
Switch 2) to 27% slow (`ON GOTO` on a Switch 1). Where TPERF times only
one size of a command, the split between a base cost and a cost per pixel is a
guess, as are the costs of commands TPERF leaves out; `speed.rs` marks them
UNVERIFIED.

## Per-frame costs and overload

A mean hides what a player sees as stutter. `--log FILE` writes one CSV row a
frame: `frame`, `run_us`, `draw_us`, `instructions`, `vsyncs` (those the
program reached), `redrawn` and `end_us` (since the start). `--script FILE`
gives the program input in the form `--play` takes, so that a run reaches the
states that need it; with `--fixed`, this still does the same work each time.

On a busy machine, wall-clock times swing with other processes' load. With
`--fixed`, run it a few times and take each frame's minimum: the frames line
up, so this leaves the frame's own cost.

`--window` runs frames as the window does: to the clock, with its time limits,
catching up without drawing. It cannot go with `--fixed`, as the work then
follows the clock. `--slower N` divides that clock and its limits by N, to see
how a machine N times slower would go. It has no window, so no upload, scaling
or present is included.

## The GPU path

`crates/sb-sdl/src/gpu.rs` and [renderer.md](renderer.md) describe the path
itself.

### `SB_GPU`

`SB_GPU` overrides the setting in File > Settings > Screen:

| Value | What it does |
|---|---|
| unset | The setting decides: Auto acts as `1`, CPU as `0` |
| `0` or empty | Keeps to the CPU, whatever the setting |
| `1` | Draws with the GPU where it can |
| `sync` | As `1`, and waits for the GPU each frame to time its work |
| `check` | As `1`, and also draws each frame on the CPU, counts the pixels that differ and prints the differences at the end |
| `check:N` | As `check`, for every Nth frame the GPU draws |

Any other value acts as `1`. The path stays off on SDL's software renderer,
and when the start-up self-test fails.

These variables go with it:

| Variable | What it does |
|---|---|
| `SB_GPU_DUMP=PATH` | With `check`, saves the frame that differed most as `PATH-gpu.pam` and `PATH-cpu.pam` |
| `SB_GPU_TOL=PIXELS` | Sets how far off a texel may be inside a cell of a 4x4 layer's grid, to measure it |
| `SB_GPU_TEST=off` | Skips the start-up self-test |
| `SB_GPU_TEST=fail` | Acts as if the self-test failed |
| `SB_GPU_GOLDEN=1` | Runs all the self-test's scenes instead of the emulator, prints how each came out and exits, with 1 if one differed too much |
| `SB_GPU_ALONE=1` | Draws on a renderer of its own with no window, through SDL's GPU driver, to measure or check that driver without a display. The window shows nothing of it |

### `SB_UI_BENCH`

`SB_UI_BENCH` is a comma-separated list. The emulator sets itself up as the
list says, times each frame's drawing, prints the results and quits. It runs
the project that `run=` names from the data directory, so use a scratch one
(set `XDG_DATA_HOME` on Linux).

| Key | What it does |
|---|---|
| `frames=N` | Times N frames (600 by default) |
| `settle=N` | Draws N frames before the timing begins (60 by default), to get past what a program sets up |
| `run=PROJECT/PROGRAM` | Runs a program of a project from the start. `PROGRAM` defaults to `MAIN.PRG` |
| `load=PROJECT/PROGRAM` | Loads a program of a project into PRG0 without running it |
| `end` | Stops timing when the program ends, however many frames it was |
| `novsync` | Presents without waiting for vsync |
| `stop` | Stops the program, as the Break key does, a second in |
| `type=TEXT` | Half a second after `stop`, or after the start without it, types TEXT, which can hold no commas |
| `editor` | Opens the editor |
| `apart` | Has the editor in its own window |
| `panel` | Shows the debugger's panel |
| `menu`, `file-menu`, `help`, `about`, `colors`, `settings`, `toast` | Once the rest is set up, shows the menu bar (as F8 does), the bar with its File menu open, the help beside the editor, the About dialog, the editor colors window, the settings window at its Editor page, or a notice |
| `cpu-smart` | Scales `XSCREEN`'s mode 1 on the CPU, the fallback for a renderer that cannot make the texture the GPU route needs |
| `slow=MS` | From when the timing begins, makes each present of the main window that waits for vsync take MS milliseconds more, as a compositor that throttles a covered window does (SDL's Wayland backend waits up to 50 ms) |
| `occluded` | From when the timing begins, has the main window count as covered, as the system reports it when it can |
| `shot=PATH` | Saves the last frame timed of each window shown, as `PATH-main.pam` and `PATH-editor.pam`, to compare drawings |
| `luma=PATH` | Writes each timed frame's number and mean brightness (0 to 255), read as it goes to the window, to PATH, and reports the frames darker than both neighbors. That is a flicker, which `check` cannot see |
| `log=PATH` | Writes one line per timed frame to PATH at the end of the run: `frame w h screen editor gather overlays present uploaded made events run wait`. `w` and `h` are the window's size in pixels, `uploaded` is bytes and `made` is textures made. The stages are in microseconds |
| `at=FRAME:WHAT` | Does WHAT to the frontend at that frame, counted from the start of the run. Give it any number of times |

`at=FRAME:WHAT` takes:

| `WHAT` | What it does |
|---|---|
| `targets` | Acts as if the renderer reset its targets, and spoils what the GPU path holds |
| `device` | Acts as if the renderer reset its device |
| `resize:W:H` | Sets the window's size |
| `fullscreen`, `windowed` | Changes the window's mode |
| `auto`, `cpu` | Changes the renderer setting |
| `tool` | Starts the F9 tool as a subprogram |
| `substop` | Ends the subprogram |
| `lost` | Acts as if the device were gone |
| `fail` | Acts as if drawing failed |
| `refuse:N` | Has the next N tries at a renderer refused |
| `panic` | Panics |
| `unsaved` | Gives PRG0 unsaved edits |
| `sleep:MS` | Stops the loop for that long and counts it as a suspend |
| `key:NAME` | Presses the key SDL calls NAME, such as `Escape` |
| `usb:USAGE` | Has `KEYBOARD` read that key as down for a frame, which `key` does not |
| `pad:NAME`, `padup:NAME` | Presses or lets go the first controller's button SDL calls NAME, as both the event and what is held. `b` is the right face button, as SDL goes by Xbox labels |

`lost`, `fail`, `refuse:N`, `panic`, `unsaved` and `sleep:MS` try the
renderer's failures.

The report says who drew, and why any frame stayed on the CPU. It gives the
mean, median, 99th percentile and maximum of each stage:

| Stage | What it times |
|---|---|
| `screen` | The screens' or the inline editor's frame, made and uploaded |
| `editor` | The editor's own window's frame |
| `gather` | The debugger's panel: gathering what it shows, and the screens it shows |
| `overlays` | Copying the textures, and the panel and overlays drawn and uploaded |
| `present` | Presenting the frame |
| `events` | Outside the drawing: from the top of the loop through the system's events |
| `run` | The program's frames, and their sound |
| `wait` | The wait for the next frame |

Below the stages come:

| Line | What it gives |
|---|---|
| `busy scr`, `busy pres` | `screen` and `present` over the frames the screens were drawn again in |
| `whole` | The whole trip round the loop |
| `stalls` | Trips of 100 ms or more, where sound starts to run out |
| `drawing` | The stages before `present` |
| `frames` | Program frames run per second of the time timed |
| `sound` | What the sound device's buffer went through |
| `uploaded` | Bytes given to textures a frame |

To measure sound with no device, set `SDL_AUDIO_DRIVER=dummy` and
`SDL_AUDIO_DEVICE_SAMPLE_FRAMES` to 960, which takes samples at very nearly
the real rate.

### Running with no screen

`SDL_VIDEO_DRIVER=offscreen` shows no window and, through EGL, still reaches
the GPU, but leaves real presents out of the numbers.

A real swapchain needs a compositor, but not a screen:

```
mutter --headless --no-x11 --wayland --wayland-display=NAME --virtual-monitor 3840x2160
```

Run this under `dbus-run-session`, with its own `XDG_RUNTIME_DIR`, and run the
emulator with `SDL_VIDEO_DRIVER=wayland`. The emulator then has a real window,
with libdecor's decorations, on the real GPU, and nothing shows on a screen.

Mutter's `org.gnome.Mutter.RemoteDesktop` takes keys and pointer moves, so
Alt+F8, the arrow keys and pointer moves make an interactive resize.
`at=N:resize:W:H` only sets the size, which leaves out most of what a drag
costs.

## Measuring on a handheld

Cross-build `smilebasic-check` for the device
([building-handheld.md](building-handheld.md)), and copy the program over as a
project. Run the same `--bench --fixed` lines under `perf stat` where it
exists, or `--bench` for times where not. building-handheld.md gives the
commands, including one for the GPU path under GLES2.

On such a core, trust instruction, cache-miss and branch-miss counts over a
desktop's times. [benchmark-results.md](benchmark-results.md) says why.
