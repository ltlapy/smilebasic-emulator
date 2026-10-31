# SmileBASIC 4: what the manual leaves out

Behaviour checked on SmileBASIC 4 for Nintendo Switch that the manual
does not describe, or that is easy to read the wrong way. The emulator
follows all of it. Items still waiting for a check are marked
`UNVERIFIED` in the source instead.

## Timing

- `VSYNC n` waits until n frames after the frame the previous `VSYNC`
  waited for, not n frames from now. When that frame has gone by, it
  returns at once and counts from then: a second behind, ten `VSYNC`s
  take nine frames, while one `VSYNC 200` takes 140. After a slow
  `ACLS`, `VSYNC:VSYNC` takes one frame.
- `WAIT`, `WAIT 0` included, starts `VSYNC`'s count anew; waiting in
  `INPUT` does not.
- `MAINCNT` and `MILLISEC` need their brackets: without them they are
  plain variables, always 0 to begin with.
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
- `INPUT`'s question mark is followed by a space; with a comma after the
  guide there is neither. Extra values typed are left out. After `?Redo from
  start`, the line is typed again on the next line, without the prompt.
- `RGBF` rounds components down: `RGBF(0.5,0.5,0.5)` is `&HFF7F7F7F`.
- `XSCREEN OUT` gives the aspect ratio in single precision: 1.66666663
  for 400x240.
- Only one-dimensional arrays may be empty: `DIM A[0]` is fine, but
  `DIM A[0,0]` and `DIM A[0,3]` are `Subscript out of range`.
- `RND(0)` is 0 and takes no number from the series; a negative maximum
  is `Out of range`.
- Each random series is TinyMT32 with its standard parameters, seeded
  with `RANDOMIZE`'s value as a 32-bit number (-1 is the same seed as
  &HFFFFFFFF), whatever the series number. `RND(n)` is the next number
  modulo n, and `RNDF()` takes two numbers and makes 53 bits of them as
  TinyMT's `res53` does.
- `FORMAT$` takes its conversions in lower case too and ignores values
  left over. `%F` rounds to the nearest, but a value exactly half way goes
  toward zero: `%.0F` gives 2 for 2.5, 3 for 3.5 and -2 for -2.5. Too few values is `Wrong number of arguments`, a value of the
  wrong type `Type mismatch`, and a conversion it does not know (`%Q`)
  `Illegal format`.
- `SORT` keeps equal keys in their order; `RSORT` gives `SORT`'s order
  turned round, so equal keys come out reversed. Strings sort by
  character code: `""`, `"A"`, `"AA"`, `"B"`, `"a"`, `"b"`.
- `RESIZE` of a 2D array takes every size, the later ones as they are:
  `RESIZE A,3` on `DIM A[2,3]` is `Wrong number of arguments`, and
  changing a later size `Illegal function call`.
- `RINGCOPY` reading past the end of its source is `Illegal function
  call`, not `Out of range` as for `COPY`.
- `TVAR` cuts a real key down to an integer: keys 1.5 and 1 are the
  same.

## Direct mode

- `INSPECT` (or `??`) prints `INT: 1`, `REAL: 1.50000000` (always 8
  decimals) or `STRING: (2)"AB"` (a 50-character string still whole).
  An array prints `INT[2,2]:`, then ` [0,1]: 0` for each element
  (strings as `(2)"HI"`), then an empty line. A long array stops with
  `=== Press ENTER ===` on the row above the last.
- `CHKCALL` is 0 for `PRINT`, `GOTO`, `INC`, `STOP`, `CONT` and `RUN`, and
  1 for `VSYNC`, `INSPECT`, `CALLIDX` and `DEFARGC`. It ignores case.
  `CHKLABEL` needs the `@`: `CHKLABEL("P1")` is 0. `CHKLABEL("1:@X")` on
  a slot `EXEC` has not run is 0.
- `? ?1` prints an empty line, then 1.

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
- `FILES` prints a long list a screen at a time, in direct mode and in
  programs alike: the list fills every row of the console but the last,
  which says `=== Press ENTER ===`, so the page follows the console's
  size (29 rows of the list on the usual 30). Any key or button goes on, and a held key goes on as it
  repeats.

## Program slots and files

- A call to a name defined nowhere compiles, and is a `Syntax error`
  only when it is reached: the name may be a `COMMON DEF` of a slot that
  `EXEC` runs later.
- `EXEC` of a file that is not there shows a dialog (File not found) and
  then stops with `Load failed`. In direct mode, `EXEC` is
  `Can't use from direct mode`, shown without a line.
- Against the manual: a program can `LOAD` over its own slot, and goes
  on; and loading over a slot that `EXEC` ran keeps that slot's
  `COMMON DEF`s callable.
- File names may start with `@`, as the backups' do: `LOADV
  "DAT:@BACKUP.DAT"` reads one, and `SAVE "@X"` saves (after the usual
  question).
- `LOAD` takes the file's whole name: `LOAD "TEST10"` does not find
  `TEST10.PRG`.
- Against the manual, programs `LOAD` and `LOADV` other projects' files
  too (`LOAD "GAME/SUB",1`), not only `EXEC`; naming the program's own
  project works as well. `LOADV` and `LOADG` in a program ask nothing.
- `EXEC` of a slot with code still to run, such as `EXEC 0` in a DEF of
  slot 1 that slot 0 called, is `Can't use the specified slot`. Slots
  that `EXEC` ran are gone after the next `RUN`.

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
- Array literals and `DIM` sizes: the last first too.
- `SWAP` works out each element's place once, the second first; `INC`
  and `DEC` work out the amount first, then the place, once.
- `CALL` works out its arguments before the name.
- `&&` and `||` still go left to right and stop early.

## DATA

- `SPANIM` with a `"@label"` reads its DATA with `READ`'s own place: it
  restores to the label, and `READ` goes on after the values it took.
- `COPY` from a `"@label"` leaves `READ`'s place alone.
- `RESTORE "POP"` with nothing kept by `RESTORE "PUSH"` is `Stack
  underflow`. A subroutine that keeps a place must take it back before
  `RETURN`, which is otherwise `Use RESTORE POP before RETURN`. A
  thousand places can be kept.

## Input

- `BREPEAT` starts at 20,3 for buttons 0 to 7 and at 15,4 for 8 to 16.
  A held button repeats at its start frame and then every interval plus
  one frames: 20,3 fires at 0, 20, 24, 28...
- `BUTTON(0)`'s bits include `#B_RANY`, `#B_LANY` and `#B_ANY` (bits 14
  to 16). `BUTTON()` without arguments is `Wrong number of arguments`.

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

- Using a sprite before `SPSET` is `Sprite not set`, except for its
  variables: `SPVAR` works on a sprite that was never set. `SPSET`,
  `SPCLR`, `ACLS` and `RUN` clear them.
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
- `SPSET` drops the sprite's `SPFUNC`, also on a sprite that was set
  already.
- `CALL SPRITE` goes through the sprites in number order as it runs: a
  callback that clears a later sprite skips it, and one that sets a later
  sprite's `SPFUNC` gets it called. Afterwards `CALLIDX()` is 4096 (5
  after `CALL TEXT`).
- A relative color animation (`"C+"`) multiplies the start color by the
  key per channel and divides by 256, rounding down: `&H80808080` with
  `&H10101010` gives `&H08080808`, and `&HFF` with `&HFF` gives `&HFE`.

## Graphics and layers

- `LAYER` mode 0 draws the layer's elements straight onto the screen and
  ignores the layer's color; only modes 1 to 4 multiply by it.
- Blending rounds to the nearest, not down: a sprite at &H80 alpha over
  60 gives (255x128 + 60x127)/255 = 157.9, shown as 158. With that, the
  emulator matched lossless screenshots of mode 0 and mode 1 layers
  pixel for pixel.
- `FADE color,time` mixes the two colors channel by channel in single
  precision and rounds down: `&HFF` to 0 over three frames passes `&HA9`
  and `&H54`, not `&HAA` and `&H55`. A clear end (alpha 0) takes the
  other end's color, so only the alpha moves; fading to `&H00FFFFFF`
  from black ends at `&H00000000`. `ACLS` clears the fade.

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

- A track's MML variables read -1 while it is not playing: before any
  song, after `BGMSTOP` and after the song ends. `BGMVAR` setting one on
  a track that is not playing does nothing. The program reads the
  variables as they were when the frame started, so a value set by the
  song, by `BGMVAR` or by stopping shows a frame later. A song started
  while no other plays takes about two frames more to begin: its first
  `$0=` showed four frames after `BGMPLAY`.
- `BGMPLAY` plays songs 0 to 73 (the 4.3 manual says 0 to 45), and
  `BEEP` sounds 0 to 156 (4.4.5 added 156). A user
  song number with nothing set is no error; it plays nothing, and
  `BGMCHK` says 0.

- `TALK` reads は as "wa", falling in pitch, when 、 comes right after
  it, as for a particle; before 。 or anywhere else it is "ha".

## Stopping

- The + button acts when it is let go. A short press jumps to the label
  of `ON BREAK GOTO`, once; without one, the program stops with `User
  break on 0:55`. Held for a second, the button stops the program at
  once, without the jump.
- The jump leaves GOSUBs behind: a `RETURN` after it is `RETURN without
  GOSUB`. `ON BREAK GOTO` inside a DEF may name a label outside it.
- Breaking off a line typed in direct mode says `User break on 0:1`. A
  forced stop (holding + for a second) says `User break on` too.
- Out of a waiting `INPUT` or `LINPUT`, the press counts twice: it
  jumps, and once the first statement there has run (a whole FOR loop's
  first statement only), it is acted on again: it jumps to a label that
  statement set with `ON BREAK GOTO`, or stops with `User break on` that
  statement's line. Printing goes on after the prompt.
- `STOP` says `Break on 0:60`, then its message, if it has one, on the
  next line. In direct mode it is `Can't use from direct mode`.
- Against the manual, `CONT` goes on after a program has been edited,
  even with lines added, and after the + button stopped it at `INPUT`:
  it goes on with the next statement, and `User break on` follows the
  prompt on the same line. After an error it is `Can't continue`. In a
  program it is a `Syntax error` when it runs.
- `BACKTRACE` lists, innermost first, the line it stopped on and the
  line of each DEF call and `GOSUB` it is in, as `0:    92` (the line
  right-aligned in 6 places). It works after an error too.
- `TRACE` inside a DEF is `Can't trace`, and `CONT` cannot go on after
  that.
- `ENVSTAT()` on the main interpreter is 3 (running), also typed in
  direct mode while a program is stopped or after an error; its OUT
  forms then report no error.
- `PERFEND()` gives microseconds; `PERFBEGIN` and `PERFEND` show no
  gauge on their own.

## System settings

- `SYSPARAM` takes names in lower case too. `LANG` is `ja` on a Switch
  set to Japanese. `SYSBEEP` starts at 1, `KEYREPEAT` at 20 and 3, and
  `TOOL1` to `TOOL3` at `#TOOL/SBSMILE.PRG`, `#TOOL/GAHAKU.PRG` and
  nothing.
- Items that can only be read can also be read with `OUT`
  (`SYSPARAM "LANG" OUT L$`).
- An unknown name is `Illegal system argument name`. Setting an item
  that can only be read, or reading `KEYREPEAT` as a value, is `Wrong
  number of OUT arguments`.
- A program run after one that set `MOUSECURSOR` to 1 reads 0 again;
  `END` leaves it set, so direct mode still reads 1.
- `KEY` 1 to 5 start as `FILES`, `LOAD"`, `SAVE"`, `LIST ERR` and `RUN`
  (the settings can change them). `KEY` ignores a string over 256
  characters; `PUSHKEY` over 64 is `String is too long`.

## Errors and the editor

- A command or function given a number of arguments none of its forms
  takes is `Wrong number of arguments` (`LOCATE` with five, `TOFS 0,30`).
  This is found when the call runs, not when the program compiles, so
  the lines before it run. `HSV` with one argument is `Wrong number of
  OUT arguments`: it is read as the `OUT` form.
- `HSV` and `HSVF` as functions need all three of H, S and V: two are
  `Wrong number of arguments`, and empty ones (`HSV(120,,)`) are `Type
  mismatch`. The `OUT` form needs at least three results too; fewer is
  `Wrong number of OUT arguments`, but S and V may be left empty, which
  is what the 4.4.9 notes mean: `HSV C OUT H,,`.
- An unknown `#constant` is `Undefined constant`.

- After an error, F4 puts the cursor at the start of the statement that
  failed, syntax errors included.

## Editor

- The editor shows 21, 28 or 43 lines of code, in the 8- and the
  16-pixel font alike.
- It colors TO, STEP, CONST and ENUM as reserved words, though the
  language specification does not reserve them, and OPTION as a command.
- Ctrl+Enter makes an empty line below the cursor's and moves to it.
- Ctrl+K puts a quote at column 0 of each line, also before lines that
  are comments already. Only Ctrl+Shift+K looks for one.
- Finding ignores the case of letters.
- Ctrl+/ splits the display side by side, then one above the other,
  then joins it again. Both sides show the slot being edited at first.
- Ctrl+S asks for a name on the on-screen keyboard, titled "Slot 0:
  Write file", with the name the slot was saved as filled in. Ctrl+L
  opens a list of the project's files, with a way up to the projects.
- While typing in the editor, a list of SmileBASIC's words starting with
  what is typed shows from the first letter. The program's own names are
  not in it.
