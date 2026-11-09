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

- Each VM instruction and statement, DEF call, builtin call and argument,
  array element and index past the first, string join and look-up by name
  (`VAR()`, `CALL`, `GOTO` with a string) costs a set amount. A join also
  costs each character of the string it makes, as TEST71's joins of two
  short strings and of one that grows to 2000 characters find. A
  comparison of strings, or any operator but `+` with a string on its
  left, costs a set amount of its own. A character of a string read or
  written by index (`S$[I]`) costs more than an array element. `ON`'s jump
  costs a builtin call with no arguments, and `FILL` each element it
  writes. A FOR's test gets a credit, as `NEXT` takes many of our
  instructions but one step of SmileBASIC 4's.
- Builtins cost by their class. `ASC`, `INT`, `FLOOR`, `ABS` and `MIN` of
  two arguments cost no more than reading a variable, so they count as no
  call (TEST69 to TEST71). The model takes `MAX` of two to cost what `MIN`
  of two does. `STR$`, `FORMAT$`, `VAL`, `INSTR`, `HEX$` and `MID$` cost a
  call and an amount of their own on top, each what TEST71 times for it
  alone. `CHR$` costs more than a call too. Every other builtin costs a
  call and its arguments. TEST71 times `SIN`, `SQR` and `LEN` as cheap as
  `INT`, but TPERF's VREAL and VSTR keep their times only with them
  charged as calls, so they stay calls.
- An `IF` or `ELSEIF` whose condition is an array element, as in
  `IF MAP%[ASC(S$[I])] THEN`, costs more than one of a comparison or a
  variable. TEST69 and TEST70 time each of these alone; TEST68's and
  TEST69's runs of a Hangul library's conversions need them together.
- Each host command costs by its kind. Drawing costs by the rows and pixels
  inside `GCLIP`, `GTRI` by its rows and pixels, `GBOX` as a `GFILL` of its
  four sides, `GPAINT` by the pixels it fills, `GPUTCHR` by the pixels of
  its characters, more each in the 16-pixel font than in the 8-pixel one,
  `TFILL` and `SCROLL` by the cells, `PRINT` by each thing it prints, its
  characters and each time it scrolls the screen, `LOADG` by the pixels of
  the image and the bytes of its file, and `EXEC` a base and the
  characters it compiles, code at a higher rate than strings and comments.
  `SPSET` costs by its arguments, more for a sprite not in use, and its
  function form pays for each sprite its search passes. `SPCLR` of a range
  costs by its size, along straight lines through TEST70's and TEST71's
  times for 1, 100, 500, 1000, 2000 and 4096 sprites, and `SPCLR` with no
  arguments costs what a range of all 4096 does, as TEST71 finds. `GCLIP`
  costs more with arguments than without. `LOCATE` with two arguments,
  `SPSHOW`, `SPVAR`, `SPCHR`, `STICK`, `SPOFS` with `OUT`, `GPSET`,
  `GPGET`, `CHKCHR` and `BGMCHK` each cost what TEST71 times for them
  alone. Other sprite commands cost a set amount each. Sprites, layers and
  text screens cost nothing to show, as TPERF and TEST67 find on both
  consoles.
- Real arguments and a `GCLIP` smaller than the page cost nothing extra, as
  TEST69 finds on both consoles. A `GCLS` on a Switch 2 costs 1.5
  microseconds before its rows and pixels, which shows under a small
  `GCLIP` (TEST70, TEST71). On a Switch 2 a `GCOPY` that leaves out
  transparent pixels costs more a pixel than one that copies them all.
- A `GFILL` whose left edge is not on a 16-pixel boundary costs more for
  each row. The model keeps that cost for fills 8, 16, 32, 64 and 128
  pixels wide, with straight lines between, in proportion below 8 and the
  same past 128, in two tables: one for a left edge on a 4-pixel boundary
  and one for an edge off it. TEST71 times fills at x 16, 17, 20 and 24.
  On a Switch 1 a 32x32 `GFILL` takes 2.1 microseconds at x 16, 4.1 at
  x 20 or x 24 and 7.5 at x 17. Fills 64 and 128 wide cost the same at
  every place off a 16-pixel boundary, and fills 8 and 16 wide cost little
  or nothing more anywhere. A Switch 2 shows much less of it. `GCOPY` pays
  60% of the table by where it draws, whatever its source, and a blended
  `GFILL` 18% on a Switch 1 and nothing on a Switch 2.
- A misaligned `GFILL`, or a blended one, at a place that is not among the
  last 64 places such fills drew at costs more again: each row on top for
  a misaligned one, each pixel for a blended one. A place is the fill's
  corner and size. TPERF's fills land at random places and cost that much
  more than fills at one place or at 10 to 64 places in turn (TEST69 to
  TEST71). `GCOPY` pays nothing for a new place. That the console's memory
  cache is the cause, and that it holds 64 places, are guesses.
- A sprite command for a sprite that is not among the last 64 changed
  costs a set amount on a Switch 1, as TEST69 finds for `SPOFS`, `SPSET`
  and `SPCLR` over 1000 sprites in turn, where TEST70 finds 1, 10 and 64
  sprites in turn cost the same. On a Switch 1 TEST71 finds no step at 64
  sprites, but a cost that grows slowly with the number of sprites, from
  858 ns a `SPOFS` over 64 sprites in turn to 976 ns over 4000. The model keeps its step.
  That the console's memory is the cause is a guess.
- The first `EXEC` of a file since the program started costs more, and so
  does the first read of each file by `EXEC` or `LOADG` since the emulator
  began running at that speed, the more the larger the file: the console
  reads a file faster once it has read it (TEST69 to TEST71). On a Switch 1
  the first `EXEC` adds 2.15 ms, and the first read of a file 0.8 ms and
  30.5 ns a byte. On a Switch 2 they add 1.2 ms, 0.3 ms and 9.25 ns a
  byte.
- A subprogram gets a frame's time of its own, so a busy one does not slow
  the main program: TEST71 finds the main program's `A=A+1` and `GFILL`
  as fast beside a subprogram that loops without `VSYNC` as alone, on both
  consoles.
- A program that needs more than the frame goes on in the next one, and its
  `VSYNC` finds its frame gone by, as on the console. The VM stops at the
  host command or `PRINT` that uses up the frame, so 100 `GCLS` of a whole
  page on a Switch 1 (0.4 s) take 24 frames of `MAINCNT`. A command that costs more than a frame
  holds up the frames after it. `MAINCNT`, `VSYNC` and
  the frames lost come out as there.
- `PERFBEGIN`, `PERFEND` and `MILLISEC` measure the console's time, which is
  the same on every run, also with `--fixed`. TPERF's `WORK` then compares
  with a console's directly, and its two score scenes run.

The window keeps its own time limit as well, so on a computer slower than the
console the program gets less.

Counting this work takes the VM off its fused fast path, which a console's
speed leaves plenty of time for. Unlimited counts nothing and runs exactly as
before.

The costs come from TPERF's scenes and the loops of TEST65, TEST67, TEST68,
TEST69, TEST70 and TEST71 on 4.4.9 (in `crates/sb-runtime/src/speed.rs`).
TEST67 times single commands at several sizes, which splits their cost into
a base and a cost per row and pixel. TPERF's `WORK` in ms, run with
`smilebasic-check --speed` and `--play`:

| Scene | Switch 1 | ours | Switch 2 | ours |
|---|---|---|---|---|
| VINT | 13.38 | 12.64 | 5.15 | 4.81 |
| VREAL | 8.42 | 8.33 | 3.29 | 3.26 |
| VSTR | 5.42 | 5.38 | 2.28 | 2.30 |
| VARRAY | 20.96 | 20.92 | 7.43 | 7.33 |
| VDEF | 7.83 | 7.83 | 2.61 | 2.60 |
| VVAR | 7.24 | 7.24 | 2.98 | 2.97 |
| SPMOVE | 7.32 | 6.39 | 2.28 | 2.17 |
| SPROT | 3.77 | 3.39 | 1.28 | 1.11 |
| SPCHURN | 8.50 | 8.61 | 3.72 | 3.72 |
| SPHIT | 6.05 | 6.09 | 2.60 | 2.60 |
| SPANIM | 0.21 | 0.18 | 0.09 | 0.09 |
| TPRINT | 2.35 | 2.32 | 1.15 | 1.14 |
| TPUT | 11.04 | 10.70 | 3.82 | 3.69 |
| TSCROLL | 7.09 | 7.14 | 3.62 | 3.63 |
| TMULTI | 0.31 | 0.28 | 0.18 | 0.15 |
| GFILL | 15.33 | 15.27 | 2.78 | 2.72 |
| GALPHA | 27.93 | 26.52 | 11.03 | 10.77 |
| GLINE | 7.60 | 7.38 | 3.13 | 3.08 |
| GCIRCLE | 3.22 | 3.16 | 1.28 | 1.26 |
| GTRI | 8.76 | 8.04 | 2.83 | 2.25 |
| GPUTCHR | 5.71 | 5.68 | 1.62 | 1.61 |
| GCOPY | 11.11 | 11.02 | 3.33 | 3.35 |
| GLOAD | 1.08 | 1.08 | 0.45 | 0.45 |
| GUPLOAD | 0.03 | 0.01 | 0.01 | 0.00 |
| GARRAY | 11.69 | 11.70 | 4.13 | 4.19 |
| LCOMP | 0.45 | 0.36 | 0.17 | 0.12 |
| LBLUR | 0.22 | 0.17 | 0.09 | 0.07 |
| LMOSAIC | 0.20 | 0.17 | 0.09 | 0.07 |
| LCOLOR | 0.26 | 0.23 | 0.11 | 0.10 |
| LMATRIX | 0.28 | 0.24 | 0.12 | 0.10 |
| LRASTER | 1.21 | 1.15 | 0.49 | 0.42 |
| GAME | 7.70 | 6.78 | 3.02 | 2.45 |
| MAXSP (score) | 2866 | 3646 | 4000 | 4000 |
| MAXTRI (score) | 4410 | 4410 | 13471 | 15484 |

`LOST` matches for VARRAY: 77 frames on a Switch 1 and 76 here; GALPHA lost
202 there and 177 here. Most scenes come within 10%. On a Switch 1 SPMOVE
runs 15% fast, GAME 13%, SPROT and SPANIM 11 to 12%, and MAXSP scores 27%
high. GFILL and GALPHA ran 20 and 22% fast before TEST71 and now come
within 6%: TPERF's fills land at random places, and TEST71 finds fills at
random places dearer than fills at one place or at 10 to 64 places in
turn. The layer scenes take under half a millisecond and run 6 to 31%
fast. On a Switch 2 GTRI runs 26% fast, GAME 23%, SPROT 15%, and MAXTRI
scores 15% high, where GFILL, 30% fast before TEST71, now comes within
2%. TEST71's `GTRI` scene runs as fast on a Switch 2 at places rounded to
16 pixels as at random ones, within 3%, so where `GTRI` draws does not
explain its gap. TPERF ran
handheld there and TEST67 to TEST70 docked, but TEST70 finds a Switch 2 as
fast in the hand as docked, within 1% on every row but a small `GCLS`, so
the dock does not explain the gap.

TEST67's rows land within 10% on both consoles, the loop's own `INC` and
`EMPTY` passes and `RGB` on a Switch 1 at the edge. TEST65's loops go from 13%
fast (`CASE` on a Switch 1) and 23% fast (`GOTO` on a Switch 2) to 16% slow
(`R%[K%[A]]` on a Switch 1); its RV32 core runs 7% fast on a Switch 1 and 3% on
a Switch 2. The costs of commands no test times are guesses, and `speed.rs`
marks them UNVERIFIED.

TEST68 timed a Hangul library on a Switch 2 only. Its rows in ms on a
Switch 2, docked:

| Row | Switch 2 | ours |
|---|---|---|
| `EXEC` of a 9798-character library, and its set-up | 3.52 | 3.27 |
| `LOADG` of 512x312 | 2.78 | 2.92 |
| `LOADG` of 1024x624 | 10.36 | 10.42 |
| `LOADG` of 2048x1248 | 38.71 | 39.87 |
| Converting 1000 characters, Hangul and ASCII | 0.481 | 0.457 |
| Searching 1000 ASCII characters | 0.285 | 0.277 |
| Converting 1000 characters back | 0.464 | 0.483 |
| `FILL` of 11172 elements, then a loop over 2350 | 0.600 | 0.546 |
| `PRINT` of 1000 characters, 25 lines of scrolling | 0.047 | 0.047 |

TEST69 ran on both consoles (a Switch 1 handheld, a Switch 2 docked). It
times `GCLIP`, real arguments, `SPCLR` of a range, `FILL`, `PRINT` with
scrolling, `EXEC` of short, code-heavy and string-heavy files, `LOADG` of an
empty and a busy image, and `GPAINT` of small boxes. Its rows land within
10% on both consoles, with these exceptions:

- On a Switch 1, HGIME's key handling, which runs 20 to 32% fast.
- `C=ASC(S$)` on a Switch 1 and `T$=S$[I]` on a Switch 2, 13 and 16% slow.
- `GCLS` under a 100x100 `GCLIP` on a Switch 2, 33% fast. It ran 1.7
  times as fast before TEST71.
- `EXEC` and `LOADG` of a file the first time cost more on the console,
  2.3 ms for a 3-line file on a Switch 2. Since TEST71 the model charges
  that too, and the first `EXEC` of the 3-line file runs 6% fast on a
  Switch 1 and 19% fast on a Switch 2.
- An empty 512x512 image that SmileBASIC 4 saved itself loads 1.6 times
  slower than the size of the file the emulator saves for it gives.

TEST70 ran on both consoles (a Switch 1 handheld, a Switch 2 docked and
again in the hand). It times `GFILL`, `GCOPY` and `GTRI` at one place and
at many, `SPOFS` over 1, 10 and 64 sprites, TPERF's MAXSP loop a command at
a time, `SPCLR` of ranges, HGCNV$'s search loop a piece at a time, and
`EXEC` and `LOADG` again. Its rows land within 10% on both consoles, with
these exceptions:

- TPERF's MAXSP loop runs 10 to 22% fast: its arithmetic alone 9% on a
  Switch 1 and 17% on a Switch 2. With `SPSCALE` and a worked-out argument
  it runs 19% fast on a Switch 1, where TEST69's `SPSCALE` of variables
  matches. What the loop spends the rest on is not known.
- Rows that pass array elements to `GFILL`, `GTRI` and `SPOFS` run 14 to
  16% slow on a Switch 1. The same commands given variables match in
  TEST67 and TEST69.
- `C%=MS%[ASC(S$[I%])]` on a Switch 2, 14% fast.
- Later `EXEC`s of a 3-line file vary by half on the console. The first
  `EXEC` and `LOADG` of each file, which the model charges since TEST71,
  come within 10% on a Switch 1. On a Switch 2 the first `EXEC` of
  TEST70's two files runs 27% and 82% slow, and the first `LOADG` of its
  image of noise 11% fast. Later loads of the image run 8 to 12% fast:
  the console loads it as fast as TEST69's, but the emulator saves it a
  fifth smaller.

TEST70's rows of `GCLIP` alone and of `GCLS` under a 100x100 `GCLIP` took
what TEST71's took, within 3%, and so match since TEST71. Before it,
`GCLIP` alone ran 12% slow on a Switch 1 and 1.7 times as slow on a
Switch 2, and the `GCLS` 25 to 31% fast on a Switch 2.

TEST71's timed part ran on both consoles right after a restart (a Switch 1
handheld, a Switch 2 docked). It times `GFILL` 8 to 128 pixels wide at
x 16, 17, 20 and 24, blended and not, `GCOPY` from and to several places,
`SPOFS` over 64 to 4000 sprites in order and at random, `SPCLR` of 100 to
4096 sprites and with no arguments, `IF` with elements, variables and
strings, `CHR$`, joins and string literals, TPERF's MAXSP loop in pieces,
`GCLIP`, TPERF's GFILL, GALPHA and GTRI scenes, the main program beside a
busy subprogram, about 40 builtins and commands one at a time, and the
first and later `EXEC` and `LOADG` of each file. Of its 188 rows, the
refit to it leaves 98 more than 5% off on a Switch 1 and 90 on a Switch 2,
from 134 and 137 before. Most rows more than 10% off fall into these
groups:

- Array elements: reading one runs 14 to 25% slow, and passing them to
  `GFILL`, `GTRI` and `SPOFS` 15 to 35% slow, as in TEST70.
- `IF` with an array element runs 1.6 to 1.8 times as slow, and `IF` with
  a variable 1.8 to 1.9 times as fast. On a Switch 1 the console's `IF`
  costs 79 ns more than reading its element and 31 ns more than reading
  its variable, where the model charges 215 ns and nothing. Reading a
  variable alone, as in `Z=V`, runs 29 to 36% fast, and
  `IF ASC(S$[I])==V` 11 to 22% fast.
- `IF S$[I]==K$` runs 1.7 times as slow on both consoles. The model
  charges a comparison of strings what TEST69's HGIMEUPDATE needs.
- `U$=CHR$(C)` and `U$=CHR$(65)` run 2.0 to 3.1 times as slow, and
  `U$=T$+CHR$(C)` 1.7 to 1.9 times. The model keeps `CHR$`'s cost for
  TEST68's and TEST69's Hangul conversions and TPERF's VSTR.
- Assigning a string literal runs 3.6 to 3.7 times as fast, and `U$=T$`
  1.5 to 1.7 times. On a Switch 2 a join of two literals runs 37% slow.
- `SIN`, `SQR` and `LEN` run 1.4 to 2.3 times as slow, as they are charged
  as calls, and `LEFT$` 32% slow on a Switch 1.
- `GLINE`, `GCIRCLE`, `TPUT` and pushing and popping an array run 19 to
  52% slow. `MAINCNT` and `MILLISEC` run 1.9 to 2.8 times as fast.
- The loop's own `A=A+1` runs 20 to 22% slow. TPERF's MAXSP arithmetic in
  a DEF runs 14 to 17% fast, and `SPSCALE` of a variable or of literals 20
  to 25% slow on a Switch 1.
- On a Switch 1, a `GCOPY` 32 pixels wide to x 17 runs 58% fast: the
  console pays 138% of a `GFILL`'s cost for the edge there, where the
  model pays 60% for TEST69's `GCOPY` at 10 places and TPERF's at random
  ones. A 16x16 `GFILL` runs 15 to 19% slow at every place.
- TPERF's scenes at random places run 11% fast on a Switch 1. On a
  Switch 2 its GFILL scene runs 16% fast and its GTRI scene 20 to 22%,
  at random places and at places rounded to 16 pixels.
- `SPOFS` over 64 sprites in turn runs 18% fast on a Switch 1, and over
  4000 12%.
- `LOADG` of images SmileBASIC 4 saved itself runs 1.1 to 1.6 times as
  fast, as TEST69's empty image does, but the second load of a 1x1 image
  on a Switch 2 23% slow. The model's cost a byte is fitted to the files the
  emulator saves, and no program can read the size of the console's own.
- Later `EXEC`s of a 3-line file vary from 0.55 to 1.75 ms on a Switch 1
  and from 0.23 to 0.49 ms on a Switch 2, where the model charges 1.41 and
  0.38 ms. Later `EXEC`s of the other files run from 22% slow to 23% fast
  on a Switch 2.

The other rows more than 10% off are within 18%.

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
| `scale=N` | Lays the editor's own window out at UI scale N instead of the desktop's |
| `slow=MS` | From when the timing begins, makes each present of the main window that waits for vsync take MS milliseconds more, as a compositor that throttles a covered window does (SDL's Wayland backend waits up to 50 ms) |
| `occluded` | From when the timing begins, has the main window count as covered, as the system reports it when it can |
| `shot=PATH` | Saves the last frame timed of each window shown, as `PATH-main.pam` and `PATH-editor.pam`, to compare drawings |
| `luma=PATH` | Writes each timed frame's number and mean brightness (0 to 255), read as it goes to the window, to PATH, and reports the frames darker than both neighbors. That is a flicker, which `check` cannot see |
| `log=PATH` | Writes one line per timed frame to PATH at the end of the run: `frame w h screen editor gather overlays present uploaded made events run wait`. `w` and `h` are the window's size in pixels, `uploaded` is bytes and `made` is textures made. The stages are in microseconds. Two columns follow `wait`: `kept`, the frames the standby thread ran during the trip, and `worker`, its microseconds running them |
| `at=FRAME:WHAT` | Does WHAT to the frontend at that frame, counted from the start of the run. Give it any number of times |

`at=FRAME:WHAT` takes:

| `WHAT` | What it does |
|---|---|
| `targets` | Acts as if the renderer reset its targets, and spoils what the GPU path holds |
| `device` | Acts as if the renderer reset its device |
| `resize:W:H` | Sets the window's size |
| `editor-resize:W:H` | Sets the editor's own window's size |
| `fullscreen`, `windowed` | Changes the window's mode |
| `auto`, `cpu` | Changes the renderer setting |
| `tool` | Starts the F9 tool as a subprogram |
| `substop` | Ends the subprogram |
| `lost` | Acts as if the device were gone |
| `fail` | Acts as if drawing failed |
| `refuse:N` | Has the next N tries at a renderer refused |
| `panic` | Panics |
| `unsaved` | Gives PRG0 unsaved edits |
| `sleep:MS` | Holds the main thread up for that long, as a call into the system can, and counts it as a suspend. The standby thread runs the program's frames meanwhile |
| `key:NAME` | Presses the key SDL calls NAME, such as `Escape` |
| `usb:USAGE` | Has `KEYBOARD` read that key as down for a frame, which `key` does not |
| `pad:NAME`, `padup:NAME` | Presses or lets go the first controller's button SDL calls NAME, as both the event and what is held. `b` is the right face button, as SDL goes by Xbox labels |
| `editor-focus` | Gives the editor's own window the keys |
| `menu`, `file-menu`, `help`, `about`, `colors`, `settings`, `toast` | Shows that part of the UI then, as the parts of the same names do once the rest is set up |

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
| `maincnt` | How far `MAINCNT` went per second of the time timed |
| `kept` | Frames the standby thread ran while the main thread was held up ([frame-thread.md](frame-thread.md)), in how many trips, and the time it took |
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
