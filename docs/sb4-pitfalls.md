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
  editor is open. So does `FADE color,time`: `FADE()` still gives the
  start right after it, and the first step shows after one `VSYNC 1`
  (TEST47). An `SPANIM` set in the same frame as the sprite's `SPCLR`
  and `SPSET` shows its first step only after two `VSYNC 1`s; set a
  frame later, after one (TEST48).
- A sliding animation mixes its two ends in single precision as
  `from*left + to*(1-left)`, `left` being the share of frames still to
  go. Rotation and scale keep the result: 0 to -10 over three frames
  reads -3.33333302 and -6.66666603, and a scale of 1 to -0.5 reads 0.5
  and 0.00000003. X, Y and Z are rounded towards zero (-3, -6, -10), and
  colors are mixed per channel and rounded down, so an alpha of `&HFF`
  at both ends reads `&HFE` on the second of three frames. Text screens
  (`TANIM`) do the same (TEST47, TEST48).
- An animation key that lasts 0 frames is `Illegal animation data`.
- How much a program runs in a frame (4.4.9, timed with `MAINCNT()`):
  in 60 frames a `WHILE MAINCNT()-T<60:INC N:WEND` loop goes round
  2395924 times on a Switch, 5962406 on a Switch 2 and 2837664 in
  Ryujinx; a loop of `FOR J=1 TO 100:X=X+J*2:Y=Y XOR J:NEXT` 21371,
  52145 and 29141 times. No program needs that speed: the emulator runs
  code as fast as it can.

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
  Components outside 0 to 1.0 are clamped (`R=1.1:?HEX$(RGBF(R,0,0))` is
  `FFFF0000`, an integer variable too), but a constant one outside them
  is `Out of range` when the program is compiled (`RGBF(1.1,0.5,0)`,
  `RGBF(1,-1,0)`).
- `CONST` and `DATA` take `HSV` as well as `RGB`.
- `XSCREEN OUT` gives the aspect ratio in single precision: 1.66666663
  for 400x240.
- Only one-dimensional arrays may be empty: `DIM A[0]` is fine, but
  `DIM A[0,0]` and `DIM A[0,3]` are `Subscript out of range`.
- An array holds nothing (`TYPEOF` 0) until its `DIM` runs, as when a DEF
  reads it before. `DIM A[,]` compiles, and is `Type mismatch` when it
  runs.
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
- `REMOVE` with a count of 0 removes nothing and is no error, also on an
  empty array and at the position just past the end.
- `RGB` takes components outside 0 to 255 and clamps them:
  `RGB(584,0,0)` is `&HFFFF0000`, a component of -1 gives 0, and so
  do alpha 300 and `RGB(C,,256,,)`.
- `TVAR` cuts a real key down to an integer: keys 1.5 and 1 are the
  same.
- `SPVAR` and `TVAR` keep an array given to them as the same array, not a
  copy: what is changed through one shows through the other.
- `INC` adds to a string where it is, as `PUSH` does: `INC T$,C$` on a
  DEF's parameter adds to the caller's string.
- `OPTION STRICT` holds from where it stands. A variable used before it
  without `VAR` can be used after it, and a DEF before it is not checked.
- A DEF's parameters may be written `A[]`; passing a number for one and
  indexing it is `Type mismatch` on the indexing line.
- Statements need no colon between them where one cannot go on: a
  `PRINT` item ends where no `;` or `,` follows it (`?"A" B=1` prints A,
  then sets B), and `NEXT` takes a loop variable only if a name follows
  (`NEXT?I`).
- `T?` is short for `TPRINT`.
- A label used as a value keeps its case (`A$=@Lp` is `"@Lp"`); as a
  place to go to, case does not matter.
- `ON BREAK GOTO ""` takes the handler away.
- A constant's name may start with a digit, as a label's may (`#2P`).
- A `#` after a number makes it a real (`0#`).
- A `\` joins its line with the next; the rest of its line is left out
  (`?1\ XYZ` and `+2` on the next line prints 3).
- `INPUT` and `LINPUT` take any string expression as the guide, a label
  among them (`LINPUT@G;A$` shows `@G`). A string variable is a guide
  only with `;`.

## Direct mode

- `INSPECT` (or `??`) prints `INT: 1`, `REAL: 1.50000000` (always 8
  decimals) or `STRING: (2)"AB"` (a 50-character string still whole).
  An array prints `INT[2,2]:`, then ` [0,1]: 0` for each element
  (strings as `(2)"HI"`), then an empty line. A string shows at most 256
  characters; a longer one is cut there, without its closing quote. A long listing stops every 28 lines with
  `=== Press ENTER ===` on the last row, which stays free while it
  prints.
- `CHKCALL` is 0 for `PRINT`, `GOTO`, `INC`, `DEC`, `STOP`, `CONT`,
  `RUN`, `NEW`, `CLEAR`, `LIST`, `TRACE`, `BACKTRACE`, `EXEC`, `DEFOUT`
  and `END`, and 1 for `VSYNC`, `WAIT`, `INSPECT`, `CALLIDX`, `DEFARGC`,
  `PROJECT`, `FILES`, `ACLS`, `LOAD` and `SAVE`. It ignores case.
  `CHKLABEL` needs the `@`: `CHKLABEL("P1")` is 0. `CHKLABEL("1:@X")` on
  a slot `EXEC` has not run is 0. With a slot, it looks at that slot's
  top level, also from a DEF in the same slot: FINITI finds its own
  models so.
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

- `GOTO` and `GOSUB` to a label that is not there are `Undefined label`
  only when they run. `THEN @L` may be followed by more statements on
  the line, which belong to the branch, and `ELSEIF A GOTO @L` takes
  GOTO in place of THEN.
- `CHKCALL("1:ACLS")` is 1 for the system's commands and functions,
  whatever is in slot 1. `CALL "WAIT",n`, `CALL "VSYNC",n` and
  `CALL("FREEMEM")` work.
- `PRGEDIT slot,n` past the last line goes to the last line, and
  `PRGEDIT slot,-1` past it, where `PRGGET$` is empty. `PRGSEEK 1` from
  the last line goes past it and gives 0. `PRGDEL` at the end of a slot
  deletes back towards its start. `PRGSET` on an empty line in the
  middle replaces it. (SmileBASIC Debugger's model of these commands
  agrees with all of this.)
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
  slot 1 that slot 0 called, is `Can't use the specified slot`. The slot
  running may be `EXEC`'d, and starts again (`EXEC 0` in slot 0). Slots
  that `EXEC` ran are gone after the next `RUN`.
- `FILES("//")` ends each project's name with `/`, lists the workspace as
  `/`, and lists each `#` project twice (`#EXAMPLE/`, `#EXAMPLE/`). A `/`
  may follow a project's name in a listing (`FILES("TXT:NAME/")`), and
  `FILES("TXT:/")` lists the workspace.
- `METALOAD "NAME"` loads another project's metadata (a project that is
  not there is `Undefined project name`); `METALOAD` alone loads the
  current one's again. Before any `METALOAD`, the title is the project's
  name. `METAEDIT(item)` works as a function.
- `#_FILENAME` is the program's name (`TEST32`), and empty in direct
  mode; `#_SLOT` is the program's slot, and in direct mode the slot run
  last.

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
- `BGMSETD`'s MML may end with the program's last DATA instead of a 0,
  and any number ends it: in `"..",5,"..",0` the strings after the 5 are
  not played.
- `RESTORE @LABEL` to a label that is not there is `Undefined label`
  only when it runs.
- `RESTORE "POP"` with nothing kept by `RESTORE "PUSH"` is `Stack
  underflow`. A subroutine that keeps a place must take it back before
  `RETURN`, which is otherwise `Use RESTORE POP before RETURN`. A
  thousand places can be kept.

## Input

- A `DIALOG` with buttons closes when A is let go, not when it goes
  down. Once it has closed, `KEYBOARD` shows no Enter press:
  `KEYBOARD(40,2)` and `KEYBOARD(40,0)` stay 0 over the next 6 frames,
  whether Enter or A closed it.
- `BREPEAT` starts at 20,3 for buttons 0 to 7 and at 15,4 for 8 to 16.
  A held button repeats at its start frame and then every interval plus
  one frames: 20,3 fires at 0, 20, 24, 28...
- `BUTTON(0)`'s bits include `#B_RANY`, `#B_LANY` and `#B_ANY` (bits 14
  to 16). `BUTTON()` without arguments is `Wrong number of arguments`.
- `IRSTART` without `XCTRLSTYLE`'s IR flag is `IR Camera is not
  available`; `TCPIANO` without Toy-Con set up is `Illegal control
  style`. `IRSTATE` starts at 0.
- Vibration patterns have lengths of their own, from 3 frames (57) to
  115 (23); 34 to 43, 47 and 48 go on past two seconds, 40 past five.
  The emulator keeps the measured table.
- `VIBRATE()` reads back what was asked for, also where nothing
  vibrates: after `VIBRATE 1,40` it is 2621480 (40 on the right times
  65536, plus 40 on the left) and `VIBRATE(1,0)` 40; with frequencies
  it is -1, still a second later. `VIBRATE` alone clears it.
- `PUSHKEY` takes a number too (`PUSHKEY 65622`, `#SOFTKEY_HIDDEN`).
  One above `&HFFFF` types nothing.
- Typed keys wait in the buffer until something reads them:
  `WAIT 180:?INKEY$();INKEY$();INKEY$()` shows what was typed during the
  wait. `INPUT` and `LINPUT` drop every key waiting when they start,
  `PUSHKEY`'s included: `PUSHKEY "7":INPUT A$` waits with an empty
  line.

## Screens and text

- `ACLS` also resets `SPPAGE`, `SPLAYER`, `GSAMPLE`, `TOFS`, `COLOR`,
  `ATTR` and `TCOLOR`, which the manual's equivalent program leaves out.
- `XSCREEN` clears the console, puts its cursor at 0,0, and resizes
  `#GSPRITE` to the new screen. `#GSPRITE` goes to the layer `SPLAYER`
  gives new sprites, whatever layer it was on.
- `XSCREEN` stops the text screens' `TANIM` animations (`TCHK` gives 0)
  and keeps their `THOME`, `TROT`, `TSCALE`, `TOFS` and `TCOLOR`.
- `TSCREEN` clears the screen. Without a character size it uses the
  font type. It also stops the screen's animations and undoes `THOME`,
  `TROT`, `TSCALE`, `TCOLOR` and `TBLEND`; `TOFS` goes back to 0,0 but
  keeps its depth. `TLAYER`, `TPAGE` and `TVAR` stay (TEST47), and so does `TFUNC`
  (TEST48). An array
  from `TARRAY` takes the new size and goes on following the screen.
- `CHR$(10)` starts a new line; tab stops are every 4 columns.
- `TPUT` off the screen is `Out of range`; `CHKCHR` off the screen gives
  0.
- `TSAVE`, `GSAVE` and `GLOAD` accept rectangles reaching off the
  screen or page; what lies outside reads as empty. `GSAVE` and `GLOAD`
  take rectangles of any size memory allows: 8192x1, 1x4097 and
  2048x2049 work, `GSAVE` giving 0 for what lies off the page and
  `GLOAD` dropping it, and a width or height of 0 is no error (`GSAVE`
  then gives no elements, `GLOAD` draws nothing) (TEST47, TEST48). `GLOAD` palette indexes past the palette are no error and
  leave the page's pixel as it is, with mode 0 too (TEST47).
- `GSAVE` into a real array gives the colours unsigned: opaque red is
  4294901760, where an integer array has -65536.
- `TCHK` reports nothing for `"I"` and `"UV"`, which text screens ignore.
  `SPCHK` and `TCHK` give 0 while the animations are stopped.
- `LOCATE` takes the column just past the right edge (`LOCATE 50,0` on
  the 50-column console).
- `TOFS` takes X and Y left out (`TOFS 0,,,10`).

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
- A callback `SPFUNC` names is a DEF or label of the slot `SPFUNC` ran
  in, or a `COMMON DEF`; `CALL SPRITE` finds it from any slot.
- `SPANIM id,n` and `TANIM id,n` with a number `ANIMDEF` never set do
  nothing.
- `SPHITSP` of a sprite that is not set is -1, and `SPHITSP(id,other)`
  with `other` not set is 0; neither is an error. `SPFUNC` takes a
  sprite that is not set.
- Setting what an animation changes stops it, and `SPCHK` no longer
  shows it: `SPOFS` during an `"XY"` animation, `SPCOLOR` during `"C"`,
  `SPCHR` during `"UV"` or `"I"`, `SPROT` during `"R"`, `SPSCALE`
  during `"S"` and `SPOFS` during `"Z"`.
- Spaces after an animation target's name are left out (`"S "`).
- `SPCHR` reads its arguments by how many there are (TEST45), not as the
  manual's `SPCHR id,[U],[V],[W],[H],[attr]`. Empty U and V always keep
  the sprite's own.
  - `SPCHR id,U,V` sets W and H to 16 and the attributes to 0.
  - `SPCHR id,U,V,attr`: the fourth argument is the attributes (W and H
    become 16), so `SPCHR 1,,,40` is Out of range on argument 4.
  - W and H are the size as shown (TEST45, TEST46). While the sprite is
    turned 90 or 270 degrees (attribute bit 0), an empty W takes the
    image's H and an empty H its W.
  - `SPCHR id,U,V,W,H` sets the attributes to 0 and stores W and H as
    given.
  - `SPCHR id,U,V,W,H,attr`: when the resulting attributes turn the
    sprite 90 or 270 degrees, the image is stored as H by W, so
    `SPCHR 1,8,8,40,50,5` reads back W 50, H 40. Empty slots keep the
    sprite's own values, so `SPCHR id,,,,,#A_REVH` only turns a sprite
    around (a published game relies on it).

## Graphics and layers

- `LAYER` mode 0 draws the layer's elements straight onto the screen and
  ignores the layer's color; only modes 1 to 4 multiply by it.
- Blending rounds to the nearest, not down: a sprite at &H80 alpha over
  60 gives (255x128 + 60x127)/255 = 157.9, shown as 158. With that, the
  emulator matched lossless screenshots of mode 0 and mode 1 layers
  pixel for pixel.
- Colour arithmetic (TEST49, about 4,500 patches read from lossless
  screenshots). Products round to the nearest, and an element is put on
  premultiplied:
  - A sprite's colour (SPCOLOR) multiplies its image channel by channel,
    alpha included; so do a text cell's colour and then the screen's
    TCOLOR. `&H808080` times `&H7F7F7F` is `&H404040`, not `&H3F3F3F`.
  - With colour `s` and alpha `a` after that, a normal element gives
    `s*a/255 + d*(255-a)/255`, each term rounded on its own; an
    additive one (`#A_ADD`, `TBLEND` 1) adds `s*a/255`, rounded, held to
    255, so `&H80010101` adds 1 to 60.
  - A composed layer (LAYER modes 1-4) holds premultiplied colour `p`
    and coverage `la`. With the layer colour `c` (alpha `ca`), the light
    is `p*c*ca/255²` and the coverage `a = la*ca/255`, each rounded.
    Mode 1 gives `light + d*(255-a)/255`, mode 2 `d + light`, mode 3
    `d*(light+255-a)/255` and mode 4 `d + light*(255-d)/255`, rounded
    to the nearest and held to 255.
  - `FADE` covers the frame with `(c*a + d*(255-a))/255`, rounded once.
  - These screenshots come from an emulator running SmileBASIC 4 on a
    PC's GPU. About one channel in twelve is 1 lower or higher than the
    rules above; every one of those, and every other sample, fits the
    GPU's own steps: a term is cut to 12 bits of fraction and halves
    round down (`floor(x*4096)`, then `(q*255 + 2047) >> 12`), while
    what is left of `d` rounds to the nearest. That last bit belongs to
    the GPU, not to SmileBASIC 4.
- A subprogram's screen over the main one (TEST49): while the
  subprogram has the focus, the main screen shows each channel times
  224/255, rounded (248 to 218, 255 to 224, 96 to 84). The
  subprogram's `BACKCOLOR` adds its colour at full strength and lets
  the main screen through in proportion to its alpha: `&H80FF4000`
  over black shows `&HFF4000`, and over a dimmed `&H77` green
  `&H40 + &H77*128/255`. Alpha `&HFE` lets nearly all of the main
  screen through, `&H01` almost none; alpha 0 leaves the screen clear.
  What the subprogram draws goes over its `BACKCOLOR` as on any screen,
  and covers the main screen by its alpha. An opaque `BACKCOLOR`
  (alpha `&HFF`) was not tried.
- `FADE color,time` mixes the two colors channel by channel in single
  precision and rounds down: `&HFF` to 0 over three frames passes `&HA9`
  and `&H54`, not `&HAA` and `&H55`. A clear end (alpha 0) takes the
  other end's color, so only the alpha moves; fading to `&H00FFFFFF`
  from black ends at `&H00000000`. `ACLS` clears the fade.

- `GCLIP` is shared by all pages.
- `GCIRCLE` with equal start and end angles draws nothing; the sector
  flag draws only the outline.
- Drawing rules read back with `GPGET` (TEST47, all 23 parts matched):
  - `GCIRCLE`'s pixels are the midpoint circle's: in each row of the
    first octant, the largest x with x² - x + y² < r². Radii of 1000 and
    70000 far off the page still draw what reaches it.
  - An arc runs between the circle pixels nearest its two angles, at
    `round(r*cos a), round(r*sin a)`, both included: 30 to 120 degrees
    takes the pixel (7,4) at 29.7 degrees. Angles past 360 or negative
    are taken modulo 360. An arc of more than half a turn whose two end
    pixels are one, such as 0 to 359 or 10 to 9, is the whole circle
    (TEST48). A sector's two sides are `GLINE`s from the center to those
    pixels.
  - `GLINE` takes the same pixels whichever end comes first, in all
    four directions and with X and Y swapped. Walked along the longer
    axis from the end with the smaller coordinate on it, `major` steps
    long and `minor` across, the offset across at step t is
    `(2*t*minor + k) / (2*major)` rounded down. When that end is on the
    page, `k = (major + 2*minor) mod (2*major)`: the line rounded (halves
    up) one step further on, `round((t+1)*minor/major)`, less
    `round(minor/major)` so that it starts at its end. `GLINE 0,0,7,3`
    steps across at X 1, 3 and 5 (`01122333`), `GLINE 0,0,6,1` at 2, and
    `GLINE 0,0,4,2` at 2 and 4. When that end is off the page, `k =
    major`: the line itself rounded, halves up, counted from its real
    end. `GLINE -1,5,30,10` steps at X 3, 9, 15, 21 and 27, `GLINE
    -1000,0,1040,51` gives rows 25 for X 0-19 and 26 from 20, while
    `GLINE 0,25,40,26` steps at X 19 (TEST47, TEST48).
  - `GTRI` draws each pixel once (with `#G_ALPHA`, also where edges
    meet), a row at a time from the leftmost to the rightmost pixel an
    edge reaches in the row, whatever order the corners come in. Each
    edge takes one pixel per step along its longer axis, like `GLINE`
    with its own `k`: an edge taller than it is wide is walked down from
    its top end with `k = major` (rounded, halves towards the bottom
    end); one at least as wide as it is tall is walked up from its bottom
    end with `k = (major - 2*minor) mod (2*major)`, the edge rounded one
    step back. `GTRI 0,0,1,2,10,1` fills rows 0-4, 1-10 and 1-6. A
    triangle whose corners lie on one line is that line, drawn once
    (TEST47, TEST48).
  - `GBOX` draws the top and bottom rows, then the sides from the top
    row to the row above the bottom: with `#G_ALPHA` the two top corners
    are blended twice and the bottom ones once, whichever corners are
    given first (TEST48). A box one pixel wide
    or high draws each pixel once, except the top of a one-pixel-wide
    box.
  - `#G_ALPHA2` (mode 3): alpha `a = sa + da - floor(sa*da/255)`; each
    channel `(s*floor(sa*255/a) + d*floor(da*(255-sa)/a)) / 255`
    rounded down. Over an opaque pixel that is mode 2's result.
  - `GPAINT` fills 4-connected areas; `GCOPY`'s color code multiplies
    and rounds down, and an overlapping copy within a page reads the
    source first; `GPUTCHRP` drawing scales its spacing with the
    characters.
- `GPUTCHR` with mode 0 clears the page under the character's
  transparent pixels.
- `GPUTCHR` without a font type uses the console's: after `TSCREEN 24`
  it draws 24-pixel characters from GRP5, glyph i (font.map's number,
  as for 8 and 16) at column i%64 and row i/64 of 24-pixel cells, which
  is how `#GUIDE/DEFFONT24` is laid out. A font type other than 8 or 16
  given to it is `Illegal font type`. `PRINT` on such a console shows
  only user-defined characters.
- `GPUTCHRP` takes each character from one empty column before what is
  drawn of it (none if the glyph starts at its first column) to its last
  drawn column, and moves on that many pixels plus the spacing (1 when
  left out). A blank character is a quarter of the font size wide. In
  the 16-pixel font this is the width font.map holds in bits 7-15 of
  each code's second u16. The function form's width scales the spacing
  with the characters: `GPUTCHRP("AB",16,2,3)` is 2*(13+11+3).
- `LCLIP`'s corners are inclusive.
- `LMATRIX layer,hx,hy` moves the layer by minus the home point.
- An additive sprite inside a composed layer adds light without
  covering anything, so it also brightens what is under the layer.
- `GARRAY(page)` is a 2048x2048 Int array indexed `[y,x]`, the same one
  each time. A write to it is seen at once by `GPGET`, also where
  `GPSET` drew next to it.
- `GUPDATE` with a coordinate of 2048 is `Out of range`.
- `ACLS` clears every layer's `LFILTER` (TEST47).
- `LFILTER layer,0,n` is `Out of range`. Raster deformation (types 3
  and 4) needs the repeat flag: `LFILTER 1,3,A,#TRUE` with a real
  `A[240,4]` or `A[240,2]` works, while without the flag it is `Out of
  range` for the type whatever the array; `A[4,100]` or an Int array with
  the flag is `Type mismatch`.
- Raster deformation works as OpenGL would: each row (type 3) or column
  (type 4) takes an entry of X offset, X scale, Y offset and Y scale in
  screen axes for either type, and a pixel shows the source at its
  center times the scale plus the offset in screen widths or heights,
  measured from the left and from the bottom. A positive X offset moves
  the picture left; a Y scale of 0.5 stretches the bottom half over the
  screen. Without repeat, what falls off shows black, BACKCOLOR included
  on a layer that is not composed. Checked pixel for pixel with
  screenshots (TTEST26).
- `LMATRIX layer,M` with 16 reals takes the layer's coordinates as
  (x, y, 0, 1) through M to clip space, X right and Y up: the identity
  puts the layer's square from (0,0) to (1,1) over the screen's top
  right quarter. M is column-major: M[12] moves along X. With M[3]
  set, W grows along X and the picture narrows that way as OpenGL's
  perspective divide would. Raster deformation applies to the screen
  after LMATRIX has placed the layer (TTEST27, pixel for pixel).
  Sprites on such a layer show whole, also the parts at negative
  coordinates or past the screen's size, as FINITI's floors running to
  the horizon show. So do text screens: a console moved to (-200,-120)
  or to (500,300) of a 400x240 screen shows where the matrix puts it
  (TTEST36, pixel for pixel).
- On a layer that `LAYER` does not compose (mode 0), `LFILTER` filters
  the whole screen drawn so far: the layers behind and `BACKCOLOR` too.
  On a composed layer it filters only that layer. Mosaic blocks start at
  the screen's corner and take their middle pixel's color.
  That holds for a layer with nothing in it, too.
- `LFILTER`'s blur halves the layer: 2x2 blocks are averaged (halves
  rounded down, so a dot becomes a 2x2 block at 64 and a line 127),
  blurred by a Gaussian of about amount/25 half-size pixels, and scaled
  back without smoothing. Amounts 1 to 4 look the same; amount 0 leaves
  the layer sharp.
- Scaled sprites are drawn without smoothing. Where a pixel's sample
  point falls exactly on a column boundary, it takes the column to the
  left (at 1.5, columns are 2, 1, 2, 1... pixels wide); rows do not.

## Signal processing

- `FFT` does not scale; `IFFT` divides by the length, and its result of
  1,2,3,4 prints exact zeros. A length that is not a power of two is
  `Illegal function call`, for `FFTWFN` too.
- `FFTWFN` gives the periodic windows, dividing by the length:
  Hamming of 8 is .080 .215 .540 .865 1 .865 .540 .215.
- `BIQUAD` keeps each channel's last two inputs and outputs in the
  coefficients' elements 5 to 8 (left or mono) and 9 to 12 (right), as
  in[-1], in[-2], out[-1], out[-2].
- `BQPARAM` takes the gain for types 5 to 7 only: leaving it out there,
  or giving it for types 0 to 4, is `Wrong number of arguments`. A
  sampling rate of 0 is `Illegal function call`.

## Sound

- A track's MML variables read -1 while it is not playing: before any
  song, after `BGMSTOP` and after the song ends. `BGMVAR` setting one on
  a track that is not playing does nothing. The program reads the
  variables as they were when the frame started, so a value set by the
  song, by `BGMVAR` or by stopping shows a frame later. A song started
  while no other plays takes about two frames more to begin: its first
  `$0=` showed four frames after `BGMPLAY`. `RECSTART` right before
  such a `BGMPLAY` waits with it: the song sounds within 50 samples of
  the recording's start.
- `BGMPLAY` plays songs 0 to 73 (the 4.3 manual says 0 to 45), and
  `BEEP` sounds 0 to 156 (4.4.5 added 156). A user
  song number with nothing set is no error; it plays nothing, and
  `BGMCHK` says 0.

- `TALK` reads は as "wa", falling in pitch, when 、 comes right after
  it, as for a particle; before 。 or anywhere else it is "ha".
- `BGMPLAY`, `BGMSTOP`, `BGMPAUSE`, `BGMCONT` and `BEEPSTOP` take their
  first argument left out with a comma after it (`BGMSTOP ,1`,
  `BGMPLAY ,0,64`).
- After `BGMSTOP` with a fade, `BGMCHK` stays 1 until a frame after the
  fade has ended: 61 frames for `BGMSTOP 0,1`, even when it comes a
  frame after `BGMPLAY`. Stopped that way in the frame `BGMPLAY` ran in,
  a song never sounds, and `BGMCHK` is 0 a frame later, or two if a
  `VSYNC` that did not wait ran in between.
- `SNDMSBAL` starts at 63, though the manual calls 64 even.
  `SNDMSBAL()` and `PCMPOS()` work as functions.
- While `RECSTART` without a time records round its buffer, `RECDATA`
  positions go round it too (-1 is the last sample, `RECLEN()` the
  first); with a time, -1 is `Out of range`.
- `WAVSET`'s waveform does not play as one cycle of the note. 32
  samples of one cycle with reference pitch 69 play `O4A` at 55 Hz and
  `O5A` 3600 cents below A5: three octaves down, so a pass of the
  samples lasts eight cycles of the note. Songs written for SmileBASIC 4
  repeat their cycles to fill eight (a 16-sample pattern of two cycles
  times 4, one of a single cycle times 8, both at reference pitch 69).
  Any of the lengths the manual gives, 16 to 512 samples, plays a pass
  at 55 Hz at its reference pitch; any other length (17, 24 or 1024
  samples) is `Illegal function call`.
- `WAVSETA`'s 8-bit form (fewer than 10 arguments) plays a pass of its
  n samples at 28160 Hz over the next power of two at or above n: at
  reference pitch 69, `O4A` with one cycle of 64 samples is in tune, 16
  samples sound 2400 cents up, 512 3600 cents down, 2048 6000 down.
- The PSG (`@216` to `@222`) and noise (`@223`) are instruments of the
  sound archive like `@0`: each PSG tone records at an RMS of about 4031
  (peak 4090) at V127, whatever its width, and the noise at 2839, never
  going below zero. An instrument region's own volume is a linear gain
  (`@0`'s is 88), unlike `V` and `@V`.
- `BGMPLAY` without a volume plays at 64, and `BGMVOL` (like
  `BGMPLAY`'s volume) is a linear gain of v/127 over that range: on a
  square that records at 3063 by default, `BGMVOL 127` gives 6079, 95
  gives 4547, 63 3015 and 31 1484.
- MML's volume (`V`) and velocity (`@V`) both start at 127 and both
  scale the level by their square: `V64` and `V127@V64` record alike.
  `P64` is the middle, even on both sides; the pan's position is
  (P-64)/63, so `P0` and `P1` are both hard left. Recorded with `RECSTART`, an
  A held at V127 has an RMS of 850 on `@0`, and a full-scale 8-bit
  waveform an eighth of full scale on each side (a ±96 square 3063, a
  sine of amplitude 100 2257).
- `Qn` holds a note for (n+1)/33 of its length, cut to whole ticks (48
  to a quarter note) and at least one: at T120 a `C4` is let go after
  10 ms with `Q0`, 135 with `Q8`, 250 with `Q16`, 375 with `Q24`, and
  `Q32` holds it all. A waveform with attack 127 is at full level from
  its first sample. An instrument's note also starts at the level of
  its first 5 ms step rather than rising from silence: `@0`'s C4 passes
  64 by its 11th sample.
- `@MP`, `@MA` and `@ML` (depth d, range r, speed s, delay t) swing a
  sine of d/128 times r units at s/2.56 Hz, starting upward: a unit is
  a semitone of pitch, 6 dB of volume, or the pan from the middle to one
  side. `@MP7,1,7,0` moves about ±6 cents every 360 ms, `@MP32,1,20,0`
  ±25 cents every 130 ms; `@MA24,2,7,0` swings the level 0.77 to 1.31
  of its middle, and `@ML24,2,7,0` moves the sound right first, each
  side between about 0.77 and 1.2 of the middle's level, which fits
  gains of sqrt((1-p)/2) and sqrt((1+p)/2) for a pan p of -1 to 1.
  The delay counts 40 ms a unit whatever the tempo, and the wave starts
  from its middle once it has passed: `@MP7,1,7,48` did not move within
  1.5 s at T120. The sine runs 5 ms behind the note, or behind the end
  of the delay: `@MP64,1,20,0` is at 0 5 ms in, and `@MP64,1,20,8`
  begins at 325 ms at T60 as at T120. `@MON` turns all
  three back on as last set.
- Envelopes move a millisecond at a time in tenths of a dB: the attack
  multiplies the level, starting from -90.4 dB, until it is within
  0.003 dB of full; the hold lasts (h+1)^2/4 ms; decay and release take
  off 65535 tenths a millisecond at 127, 24 at 126, (2v+1)/640 for v up
  to 49 and 12/(126-v) above; the sustain level is 400 log10(s/127)
  tenths, no lower than -723 (-722 for 1); a note ends below -90.4 dB.
  `@E` sets only attack, decay, sustain and release and keeps an
  instrument's hold; `WAVSET` and `WAVSETA` have no hold.
- A portamento (`_`) glides during the note it follows, in a straight
  line in semitones over that note's gate, and the next note goes on
  from there without starting again: `T120O4C2_G2` slides from C to G
  over the first second and holds G for the second, and on a piano
  (`@0`) the G does not strike again.
- A slur (`&`) to another note goes on at that note at once without
  striking again: on `@0`, `C4&G4` keeps decaying from the C through
  the G, where `C4G4` strikes the G anew.
- With the effector on, `EFCWET`'s four values (BEEP, BGM, TALK and
  PCM) each send that part of the sound to the reverb: wet/127 of it
  goes there and the rest straight out, in a straight line. With it
  off, they change nothing.
- `RECSTART` records the sound before the effector, or the effector
  did nothing: a short note recorded alike, to the unit, with `EFCEN`
  off, with `#EFCCAVE` at `EFCWET 0,127,0,0`, with a hall set by
  `EFCSET #EFCREFHALL,...` and with `EFCEN` off again.
- `WAVSET` or `WAVSETA` while a note of that instrument sounds writes
  the new samples over the start of the instrument's memory, and the
  note goes on reading it with the length, loop and rate it started
  with, keeping its envelope: a square swapped for a square of two
  cycles doubles the pitch at the same level, but a `WAVSET` square (512
  samples once stretched) swapped for a 32-sample sine plays the sine's
  one cycle and then the rest of the square on each pass, near the
  square's level (2981 to 3023 against 3063) with twice the crossings.
- `WAVSETA`'s 16-bit form (10 arguments or more) takes the loop's end + 1
  values from the offset on, with the loop counted from the offset, and
  drops what comes after; values are cut to 16 bits. A value v sounds
  like the 8-bit form's v/256 + 128.
- A sung note (`*A,C2`) is the voice alone: the channel's instrument
  stays silent under it, and plays again on the next plain note. Each
  channel sings with a voice of its own, so two channels' sung notes
  overlap, and `TALK` and sung notes sound together without cutting each
  other off.
- A sung note's letters must make syllables, or the MML is `Illegal
  MML`, with the caret at the first letter that cannot go on:
  `*KONNICHIWA,C1` fails at its second N. ん is `Q` (`*KOQNICHIWA,C1`),
  as in `TALK "$..."`.
- MML takes a bare length after a tie (`C4&2`), `@MON`, `N` without a
  length (`N60`), macro names with `_` and macros used inside a macro's
  definition (`{X=CDE}{Y={X}F}`).

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
  right-aligned in 6 places). It works after an error too, and prints
  nothing when no program has stopped. After it, `CONT` and `TRACE`
  cannot go on.
- `TRACE` runs one statement and says `Break on 0:2` with that
  statement's line, in DEFs and GOSUBs too. Some places are no
  statement of their own: a `FOR` runs on into the first statement of
  its body, and a DEF's `END` runs on through the return into the
  caller's next statement. `NEXT` takes two TRACEs that both say the
  FOR's line (the step, then the test). After a GOSUB's `RETURN` it
  names the GOSUB's line. After an error it is `Can't trace`.
- `Can't continue`, `Can't trace` and `Can't use from direct mode` name
  no line.
- `Break on` follows whatever was printed: after `?"X";` it reads
  `XBreak on 0:16`.
- `ENVSTAT()` on the main interpreter is 3 (running), also typed in
  direct mode while a program is stopped or after an error; its OUT
  forms then report no error.
- `PERFEND()` gives microseconds; `PERFBEGIN` and `PERFEND` show no
  gauge on their own. Without a `PERFBEGIN`, `PERFEND` counts from when
  SmileBASIC started.

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
- `FREEMEM` is 134216720 at start, and the program and its numeric
  variables take none of it. A string takes 2 bytes a character plus
  about 98, an array its elements (Int 4, Real and string 8) plus 64,
  rounded up to 64 bytes. Strings of up to 24 characters and arrays of
  up to 64 bytes of elements take nothing. `DIM A[100000]` takes 800064 bytes, 1000
  characters 2112. `GARRAY` and `TARRAY` take nothing. Memory let go of
  (emptying a string or array, a DEF's locals after it returns) does
  not come back at once. Both of its results are the same.
- `FONTINFO "A",16` is 16,1040 and `FONTINFO &H3042,8` 392,32. An empty
  string is `Can't use empty string`, a user-defined character `Can't
  use user-defined character`.
- `HELPGET` and `HELPINFO` read the help of the console's language.
  Pages count from 0; colored runs come as position, length and an
  opaque color (`&HFFF39800`), in an array one run wide when no line
  has more. Names may be in lower case. `HELPINFO OUT` names the word at
  the editor's cursor at that moment. `HELPINFO("NAME")` works as a
  function.
- `KEY` 1 to 5 start as `FILES`, `LOAD"`, `SAVE"`, `LIST ERR` and `RUN`
  (the settings can change them). `KEY` ignores a string over 256
  characters; `PUSHKEY` over 64 is `String is too long`.
- `XSUBSCREEN` and the undocumented `UISTATE` are there for the main
  program too: `XSUBSCREEN` does nothing and `UISTATE(0)` is 0.

## Subprograms

Checked with TTEST36.

- A subprogram starts with the focus. While a subprogram runs, the side
  without the focus is drawn darker, each color channel to 7/8 (255
  shows as 224, 96 as 84): the main screen while the subprogram has the
  focus, and the subprogram's screen once it gives it up. The editor is
  darkened the same way while a tool started from it has the focus.
- `ENVTYPE OUT T,P` gives P -1 in a subprogram. `XSUBSCREEN OUT` before
  any `XSUBSCREEN` gives the whole display, 0,0,1280,720.
- `SNDMSBAL()` starts at 63 there too, and `PROJECT()` is the project
  `SUBRUN` loaded the file from.
- `ENVSTAT OUT S,N,A,SL,C,L` after the main program's `Divide by zero`
  gives N 7, A -1, SL 0, C the statement's character from the start of
  the program counting from 0, and L its line. After a `STOP`, N is 0
  and A, SL and C are -1, but L keeps the line of the error before.
- A subprogram's `ENVSTAT 0` turns a stopped program's status 1 or 2 to
  0, and `CONT` can no longer go on with it.
- `ENVINPUT$(1)` is the direct-mode line typed last, the `SUBRUN` itself.
- After `ENVFOCUS 0`, the subprogram's `PUSHKEY` types into the main
  interpreter's direct mode, which runs the line.
- An error, a `STOP` or `SUBSTOP` ends the subprogram, hides its screen
  and gives the focus back. The main console prints why, between lines
  of its own, without OK unless a line of its own ran:

  ```
  === From subinterpreter ===
  User break on 0:11
  ===========================
  ```

  For an error the middle line is its message (`Divide by zero in
  0:8`), for `STOP` `Break on 0:7`.
- `SUBHIDE` hides the subprogram's screen and `SUBSHOW` shows it again,
  while it goes on running. A clear `BACKCOLOR` shows the main screen
  through.
- `ENVLOCATE OUT X,Y`, which the manual leaves out, gives the right
  edge and top of the main console's cursor cell on the display: with
  the 400x240 screen 1200 pixels wide in its middle, the cursor at
  column C of row R is at 40+24*(C+1), 24*R (TTEST37).
- `END` in a subprogram hides its screen and prints nothing; the line
  being typed in direct mode stays as it was, and the main interpreter
  has the focus again (TTEST37, TTEST38).
- `ENVFOCUS` moves the focus a few frames later, and `ENVFOCUS()` gives
  the old answer until then: `ENVFOCUS 0` took one to three frames (it
  varied from run to run), `ENVFOCUS 1` one. An `ENVFOCUS` while the
  focus is moving is lost (TTEST37, TTEST38). TTEST37's ten frames
  after `ENVFOCUS 0` were no frames at all: its `VSYNC 10` came behind
  and went straight on.
- The interpreter that gets the focus is framed for a moment by a
  yellow (255,255,0) border: three display pixels on the edge of its
  screen, one outside and two inside, with a black band two pixels wide
  inside that. The subprogram's frame is its XSUBSCREEN rectangle, the
  main interpreter's the whole display, every time the focus moves,
  `SUBRUN`'s start and a subprogram's end included. Both show whole for about 0.23
  seconds and then fade out evenly over about 0.25 seconds, drawn over
  what is under them. A frame for the other side, coming before that,
  takes its place at once (a screen recording at 24 frames a second).
- From TTEST39: after `LOCATE 0,999`'s `Out of range`, `ENVSTAT OUT`
  gives N 10 and A 1, the failing argument counted from 0; C is the
  statement's start. A `GOTO` with nothing after it runs the lines
  before it and stops with `Type mismatch` (N 8) at its statement. An
  error typed in direct mode after that gives status 1, N 0, A, SL and
  C -1, and L the program error's line. While the editor shows, status
  4 comes with N, A, SL, C and L all 0, and `ENVLOCATE` gives 0,0.
- The tools on F9 to F11 start also while a program runs. `SUBRUN`, or
  a tool key, while a subprogram runs stops it first with `User break`,
  printed on the main console as above, also when the new file cannot
  be loaded.

## Errors and the editor

- A command or function given a number of arguments none of its forms
  takes is `Wrong number of arguments` (`LOCATE` with five, `TOFS 0,30`).
  This is found when the call runs, not when the program compiles, so
  the lines before it run. `HSV` with one argument is `Wrong number of
  OUT arguments`: it is read as the `OUT` form.
- Changes made with `PRGSET`, `PRGINS` and `PRGDEL` do not count as
  unsaved: `LOAD` over the slot from a program, `NEW` and `PROJECT` do
  not ask about them (DAW_MUSIC, and by hand).
- `RETURN 1,2` (more than one value) is a `Syntax error` only when it
  runs: a program with one in a DEF it never calls runs normally
  (KRB_OGIRI_SMBOMB). A bare `GOTO` or `GOSUB` likewise runs the lines
  before it and stops there with `Type mismatch` (TTEST39).
- `HSV` and `HSVF` as functions need all three of H, S and V: two are
  `Wrong number of arguments`, and empty ones (`HSV(120,,)`) are `Type
  mismatch`. The `OUT` form needs at least three results too; fewer is
  `Wrong number of OUT arguments`, but S and V may be left empty, which
  is what the 4.4.9 notes mean: `HSV C OUT H,,`.
- A function may be called as a command with one `OUT` variable for its
  result: `ABS -3 OUT A`, `SPCHR 0 OUT D`, `SPSET 242 OUT IX`,
  `HSV 120,255,255 OUT C`.
- An `OUT` variable a `DEF` never sets gives the omitted value (`TYPEOF`
  0), whatever its suffix; inside the `DEF` it reads as that too.
- A `DEF` sees a global only if top-level code above the `DEF` names it:
  the program compiles in one pass. Otherwise the name is the `DEF`'s own.
- Inside an `IF` or another block of a loop, `NEXT` (also `NEXT I`),
  `WEND` and `ENDLOOP` go on with the loop's next turn: `IF A THEN NEXT`.
- A `DEF` with an `OUT` may be called as a function, giving the `OUT`
  value (`DEF F A OUT R` as `?F(3)`), and a `DEF` that returns a value
  may be called with one `OUT` (`T OUT X`). One with two `OUT`s called as
  a function is `Wrong number of OUT arguments` when the call runs.
- An unknown `#constant` is `Undefined constant`.
- `RETURN` with more than one value, as in a `DEF` with two `OUT`s, is a
  `Syntax error` when the program compiles.
- `RETURN` with one value in a `DEF` that is not a function, also one
  with an `OUT`, is accepted and the value ignored; under `OPTION STRICT`
  it may even name a variable never declared (`RETURN UNDECLARED` in
  `DEF T OUT Y`, SMILEBOY).

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
