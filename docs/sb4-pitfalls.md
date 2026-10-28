# SmileBASIC 4: what the manual leaves out

Behaviour checked on SmileBASIC 4 for Nintendo Switch that the manual
does not describe, or that is easy to read the wrong way. The emulator
follows all of it. Items still waiting for a check are marked
`UNVERIFIED` in the source instead.

## Timing

- `VSYNC n` waits until n frames after the frame the previous `VSYNC`
  waited for, not n frames from now. When the program is behind, `VSYNC`
  returns at once and only moves that mark on by n, so after a slow
  command such as `ACLS`, `VSYNC:VSYNC` can pass without a single frame.
- Animations (`SPANIM`, `TANIM`) take their first step on the frame
  after they are set. They also play in direct mode, and go on while the
  editor is open.
- An animation key that lasts 0 frames is `Illegal animation data`.

## Values and variables

- A suffix only sets a variable's starting value. `A%=1E10` keeps the
  real, `A$=1` stores a number and `A%="X"` a string.
- Operations on constants are worked out when the program is compiled,
  so `?"A"*-1` or `?1/0` stops the program before it runs, even on a line
  that is never reached.
- `"A"*-1` is `Overflow`, not `Out of range`.
- `-2147483648 DIV -1` wraps to -2147483648, and shift counts are taken
  modulo 32: `1<<33` is 2, `1<<-1` is -2147483648.
- `MOD` takes the sign of the dividend; `ROUND` rounds halves away from
  zero.
- The literal `2147483648` is a real.
- Reals print with eight decimals, trailing zeros removed, never in
  exponent form: `1E20` prints all 21 digits, `1/3` as `0.33333333`,
  `-1E-9` as `-0`.
- `VAL(" 12 ")` is 0: spaces make the text not a number. `INPUT` asks
  again (`?Redo from start`) for the same reason.
- `RGBF` rounds components down: `RGBF(0.5,0.5,0.5)` is `&HFF7F7F7F`.
- `XSCREEN OUT` gives the aspect ratio in single precision: 1.66666663
  for 400x240.
- Only one-dimensional arrays may be empty: `DIM A[0]` is fine, but
  `DIM A[0,0]` and `DIM A[0,3]` are `Subscript out of range`.

## Direct mode

- Variables stay until `CLEAR`, however they came about: by `DIM`,
  `VAR`, assigning or only reading them (an unknown variable reads as the
  real 0). Declaring one of them again is `Duplicate variable`.
- A line's declarations are checked as it runs, not when it compiles:
  `DIM H[2]:DIM H[2]` declares `H` and then fails on the second `DIM`.
  In a program the same line does not compile.
- A declaration that fails still declares. After `DIM D[0,3]` fails,
  `D` is the omitted value (`TYPEOF` 0, and `?D` is `Type mismatch`);
  after `VAR G=SQR(-1)` fails, `G` is 0. A line that does not compile
  declares nothing.
- `CLEAR` in a program is a `Syntax error` when it runs.
- `FILES` prints a long list a screen at a time, waiting for any key
  before the next, in direct mode and in programs alike.

## Order of evaluation

SmileBASIC 4 works things out right to left, which shows when the parts
have side effects (a DEF that prints, `RND`, `INKEY$`, `BEEP`):

- Both sides of an operator: the right one first. `F(1)+F(2)*F(3)` calls
  `F(3)`, `F(2)`, then `F(1)`; `INKEY$()+INKEY$()` puts the first key
  typed last.
- Arguments of functions, commands and DEFs, and array indices: the last
  first. In `A[F(1),F(2)]=F(3)` the value comes before the indices.
- `PRINT` works out every item, last first, before it prints any of
  them: `?P(1);P(2)` with a `P` that prints shows `<2><1>12`.
- `FOR` works out `STEP`, then `TO`, again before every test, so they
  are called once more at each `NEXT`.
- `&&` and `||` still go left to right and stop early.

## DATA

- `SPANIM` with a `"@label"` reads its DATA with `READ`'s own place: it
  restores to the label, and `READ` goes on after the values it took.

## Screens and text

- `ACLS` also resets `SPPAGE`, `SPLAYER`, `GSAMPLE`, `TOFS`, `COLOR`,
  `ATTR` and `TCOLOR`, which the manual's equivalent program leaves out.
- `XSCREEN` clears the console, puts its cursor at 0,0, and resizes
  `#GSPRITE` to the new screen.
- `TSCREEN` clears the screen. Without a character size it uses the
  font type. An array from `TARRAY` takes the new size and goes on
  following the screen.
- `CHR$(10)` starts a new line; tab stops are every 4 columns.
- `TPUT` off the screen is `Out of range`; `CHKCHR` off the screen gives
  0.
- `TSAVE`, `GSAVE` and `GLOAD` accept rectangles reaching off the
  screen or page; what lies outside reads as empty. `GLOAD` palette
  indexes past the palette are no error.
- `TCHK` reports nothing for `"I"` and `"UV"`, which text screens ignore.
  `SPCHK` and `TCHK` give 0 while the animations are stopped.

## Sprites

- Using a sprite before `SPSET` is `Sprite not set`.
- With both a flip and a quarter turn, the image is flipped first, then
  turned. Text characters (`ATTR`) work the same way.
- A linked child's offset turns and scales with its parent whatever the
  link flags; the rotation and scale flags only decide whether the
  child's own image turns and scales too.
- When a parent is cleared, children without link bit 0 stay where they
  showed, Z included.
- Hidden sprites do not collide, including children hidden through
  their parent (link bit 0). Sprites without `SPCOL` never collide, and
  rectangles that only touch do not either. `SPCOL` without a rectangle
  starts it at minus the home point.
- `SPHITSP` with a reversed range searches it from the lower number.
- `CALL SPRITE` goes through the sprites in number order as it runs: a
  callback that clears a later sprite skips it, and one that sets a later
  sprite's `SPFUNC` gets it called. Afterwards `CALLIDX()` is 4096 (5
  after `CALL TEXT`).
- A relative color animation (`"C+"`) multiplies the start color by the
  key per channel: `&H80808080` with `&H10101010` gives `&H08080808`.

## Graphics and layers

- `GCLIP` is shared by all pages.
- `GCIRCLE` with equal start and end angles draws nothing; the sector
  flag draws only the outline.
- `GPUTCHR` with mode 0 clears the page under the character's
  transparent pixels.
- `LCLIP`'s corners are inclusive.
- `LMATRIX layer,hx,hy` moves the layer by minus the home point.
- An additive sprite inside a composed layer adds light without
  covering anything, so it also brightens what is under the layer.

## Sound

- `TALK` reads は as "wa", falling in pitch, when 、 comes right after
  it, as for a particle; before 。 or anywhere else it is "ha".

## Errors and the editor

- After an error, F4 puts the cursor at the start of the statement that
  failed, syntax errors included.

## Editor

- The editor shows 21, 28 or 43 lines of code, in the 8- and the
  16-pixel font alike.
- It colors TO, STEP, CONST and ENUM as reserved words, though the
  language specification does not reserve them, and OPTION as a command.
