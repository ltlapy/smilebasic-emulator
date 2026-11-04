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
- `SPANIM`, `TANIM` and `FADE color,time` take their first step at the
  first update of the frame job after the command, the three together,
  whatever the program was doing. So after a `VSYNC` that finds its frame
  gone by, or a long run of code, the next `VSYNC 1` goes straight on and it
  takes a second one to see the step. On a Switch 2 the step comes about a
  millisecond before `MAINCNT` rises (TEST60). `FADE()` still gives the start
  right after `FADE`. Animations also play in direct mode, and go on while
  the editor is open.
- `RUN` leaves `VSYNC`'s count where it was, so the first `VSYNC` of a program,
  `VSYNC 5` too, finds its frames gone by: `T=MAINCNT():VSYNC 5:?MAINCNT()-T` as
  a program's first line prints 0 (TEST62, on a Switch 2). So does a subprogram's
  first one, and a program waiting by a lone `VSYNC 60` at its start does not wait.
- `XSCREEN` holds the program until the frame job has taken the new mode,
  without touching `VSYNC`'s count, so a `VSYNC 1` right after it goes
  straight on (TEST60). The program goes on inside that job, past its
  animation step and with `MAINCNT` still to rise. So with `ACLS:XSCREEN
  400,240,2`, then a `SPANIM` and `VSYNC 1:SPOFS` in a loop, in a program that
  has not waited yet, the first two rows show nothing and the third the first step: the
  second `VSYNC 1` only sees that job end (TEST62 on a Switch 2, as TEST56 did on Ryujinx).
- `SPCHR`'s attribute goes up to 31: 32 is `Out of range (SPCHR:6)` (TEST60).
- `MAINCNT` follows real time while the program is busy: 180 in 3 seconds of
  a loop with no `VSYNC`. When the frame job is held up it falls behind real
  time with it: 600 sprites of 64x64 scaled by 21 on a Switch 2 gave 49 in
  4077 ms, one `VSYNC` to each (TEST60).
- A sliding animation of rotation, scale or color mixes its two ends in
  single precision as `from*left + to*(1-left)`, `left` being the share of
  frames still to go. Rotation and scale keep the result: 0 to -10 over
  three frames reads -3.33333302 and -6.66666603, and a scale of 1 to -0.5
  reads 0.5 and 0.00000003. Colors are mixed per channel and rounded down,
  so an alpha of `&HFF` at both ends reads `&HFE` on the second of three
  frames. X, Y and Z are whole numbers that divide,
  `(from*frames_left + to*frames_gone)/frames`, dropping the fraction: 0
  to -10 reads -3, -6, -10, and 0 to 3 reads 1, 2, 3. Text screens (`TANIM`)
  do the same (TEST47, TEST48, TEST56).
- An animation key that lasts 0 frames is `Illegal animation data`.
- `SPANIM` keys for XY, Z, UV and I outside -32768 to 32767 are `Out of range
  (SPANIM)`; `TANIM` and `ANIMDEF` go through the same code. R has no such
  limit (TEST60).
- How much a program runs in a frame (4.4.9, timed with `MAINCNT()`):
  in 60 frames a `WHILE MAINCNT()-T<60:INC N:WEND` loop goes round
  2395924 times on a Switch, 5962406 on a Switch 2 and 2837664 in
  Ryujinx; a loop of `FOR J=1 TO 100:X=X+J*2:Y=Y XOR J:NEXT` 21371,
  52145 and 29141 times. No program needs that speed: the emulator runs
  code as fast as it can.

## Values and variables

- `STR$` of a real gives six significant digits, not `PRINT`'s eight
  decimals: `STR$(2/3)` is `0.666667` where `?2/3` shows `0.66666667`
  (TEST60). It is C's `%g`, so a million is `1e+06`.
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
- `VAL` takes underscores in `&H` and `&B` digits as a program does:
  `VAL("&H_FF")` and `VAL("&HFF_")` are 255. `VAL("1_000")` and `VAL("&H_")`
  are 0 (TEST56).
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
- `RESIZE A,0,3` on `DIM A[1,3]` leaves `LEN` 0 and `DIM(A,0)` 0, and
  `SAVEV` and `LOADV` bring that back (TEST51). `RINGCOPY A,0,A` on such
  an array does nothing and is no error (TEST51).
- `LOADV` loads array files with a zero in any size and keeps the sizes:
  `[0,3]` and `[3,0]` both give `RESULT()` 1 (TEST51).
- `FORMAT$` gives at most 1024 characters as a whole, not per field:
  `%1000D%24D` gives 1024 and `%1000D%25D` is `String is too long (FORMAT$:3)`,
  numbered by the value being formatted; `%S` of 1025 characters is
  `(FORMAT$:2)`. A format of 1025 plain characters is `(FORMAT$)` with no
  number, as is `%%` over the limit (TEST57). Of 1, `%1024D` works and `%1025D`
  does not, `%.1000F` works (a length of 1002) and `%.1023F`, which would be
  1025 long, does not (TEST51, TEST52). `%F` of 1.5 gives 1023 characters at
  `%.1021F` and `%1023F`; `%.1022F` and `%1024F` are `String is too long
  (FORMAT$:2)` (TEST58): it needs one character of room more than it gives. On `%S` the precision only limits the
  string, so `%.2000S` of a short string is fine. `%05S` pads with zeros,
  `[00ABC]`, and with `-` it pads spaces on the right (TEST57).
- `FORMAT$`'s precision on `%D` is the least number of digits (TEST58): of 5
  and of -5, `%.3D` gives `005` and `-005`, `%5.3D` gives `  005` and ` -005`,
  `%-5.3D` gives `005  ` and `-005 `, and `%05.3D`, where the precision takes the
  place of the `0` flag, gives `  005` and ` -005`. `%.1D` of 0 is `0`, `%.0D` of 5
  is `5`, and `%.0D` of 0 is nothing. `%X` and `%B` go the same way: `%.4X` of 255
  is `00FF`, `%.4B` of 5 is `0101`, `%+.0D` and `%.0X` of 0 are empty, and `%6.3X` of 10 is
  `   00A` (TEST63).
- `RINGCOPY` looks at things in this order (TEST57):
  - a destination that is no array: `Type mismatch (RINGCOPY:1)`; a source
    that is no array, or element types neither the same nor both numbers:
    `(RINGCOPY:3)`;
  - either array neither 1D nor 2D: `Type mismatch`, no number. For 2D, the
    first size is channels and the second length;
  - with four arguments, a count below 0 or over the source's length: `Illegal
    function call (RINGCOPY:4)`. With five, a source offset below 0 is
    `(RINGCOPY:4)`, and a count below 0 or offset + count over the length
    `(RINGCOPY:5)`;
  - a count below 1 or a destination of length 0: nothing is copied, no error,
    and `OUT` gives 0. `RINGCOPY A,2,B,0 OUT E` gives 0 and `DIM A[2,4],B[3,4]:
    RINGCOPY A,0,B,0` is no error;
  - source channels neither 1 nor the destination's: `Type mismatch`, no
    number, as are string arrays that get this far.

  TEST58 saw these on `A[4]` and `B[3]`: `RINGCOPY A,0,B,-1,1` is `(RINGCOPY:4)`;
  `A,0,B,4,0`, `A,0,B,1,3` and `A,0,B,0,-1` are `(RINGCOPY:5)`; `A,0,B,3,0 OUT E`
  gives 0; a number as the first or the third argument is `Type mismatch
  (RINGCOPY:1)` or `(RINGCOPY:3)`. With five arguments too the destination offset
  wraps: with B=1,2,3, `RINGCOPY A,9,B,1,2 OUT E` leaves 0,2,3,0 and E=3, and
  `A,-1,B,1,2` leaves 3,0,0,2 and E=1.

  The destination offset wraps round with a floored modulo, negative too:
  `RINGCOPY A,-1,B OUT E` on `A[4]` and B=1,2,3 leaves 2,3,0,1 and E=2. `OUT` is
  (start + count) modulo the length.
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
- A file name may end in a dot: `SAVEV "TXT:Q51.",S$` saves, and
  `CHKFILE("TXT:Q51.")` is 1 while `CHKFILE("TXT:Q51")` is 0 (TEST51).
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
- `PUSHKEY` takes a number too (`PUSHKEY 65622`, `#SOFTKEY_HIDDEN`). Codes
  from `&H10000` are keys that are not characters: `&H10000` to `&H10003` up,
  down, left and right, `&H10004` Backspace, `&H10005` Delete, `&H10008` Tab,
  `&H10009` Home, `&H1000A` End, `&H10013` starts selecting (plain moves then
  select), `&H10015` Copy. `PUSHKEY 8` is not Backspace but a character, and
  goes into a line as a cell of code 8; `PUSHKEY 9` the same (TEST60).
- `PUSHKEY` holds the program for a frame, without touching `VSYNC`'s
  count: ten of them take ten frames. A string of 64 characters is `String is
  too long (PUSHKEY:1)`; 63 goes (TEST60).
- Keys pass through two buffers of 128, each losing its oldest when full.
  Typed and pushed keys wait in the first, and each frame takes up to 32 to
  the second, which `INKEY$` and the line being typed read. Five `PUSHKEY`s of
  63 in a row, with 315 keys, leave 128 to read: keys 32 to 95, 124 to 155 and
  187 to 218; 219 on come in the frames after, with the first buffer's rest.
  At the prompt, five `PUSHKEY`s of 60 lose keys the same way (TEST60).
- Typed keys wait in the buffer until something reads them:
  `WAIT 180:?INKEY$();INKEY$();INKEY$()` shows what was typed during the
  wait. `INPUT` and `LINPUT` drop every key waiting when they start,
  `PUSHKEY`'s included: `PUSHKEY "7":INPUT A$` waits with an empty
  line.

- Tab on a line types one space, and a selection stays after Enter. Copy on
  a line ends the selection and keeps the text, so what is typed next goes
  in at the cursor (TEST60). `INPUT A$`, ABCD, Shift+Left twice, X gives
  `ABXCD`: on `INPUT`'s line Shift only moves.
- A line holds 256 characters whatever the console's size; a character typed
  on a full one is dropped (TEST60).
- Enter goes on from the row under the cursor, wherever on the line that is.
  A line that exactly fills its row leaves an empty row before what comes
  next, as the cursor is on the next row already. With the cursor on the first
  row of a longer line, what comes next prints over the second (TEST60).
- A line keeps rows free under its cursor according to the hint bar: at
  1280x720 with 8x8 characters, typing from row 85 leaves the cursor on row
  86; at 640x360 from row 40, on row 42 (TEST60).

- Enter on an empty line of direct mode prints `OK` (TEST60).
- The prompt drops waiting keys as it starts, after a typed line as after a
  program: `PUSHKEY "?1"+CHR$(13)+"?2"+CHR$(13)` typed at the prompt runs neither
  (TEST62). From a subprogram, typing into a prompt already there, they arrive.
- `INKEY$` after `PUSHKEY &H10009` (HOME) gives one character of code 2, and
  after `PUSHKEY &H10000` (up) code `&H1E`. `PUSHKEY &H10FFF` goes and `&H11001`
  is `Out of range` (TEST62).

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
- `TROT` keeps the angle in single precision: `TROT 0,0.1` reads back
  0.100000001490116 (TEST56).
- `TOFS` takes X and Y left out (`TOFS 0,,,10`).
- `SCROLL 0,0,2147483647` and `SCROLL 0,2147483647,0` clear the screen,
  as any shift of a whole screen or more does (TEST51).
- `TFILL` clips its rectangle to the screen before the string starts, and
  the string runs on across rows at the clipped width: on a 6x3 screen
  `TFILL 0,MIN,MIN,MAX,MAX,"ABC"` gives `ABCABC` on every row (TEST51), and
  `TFILL 0,-1,-1,4,1,"ABCDEFG"` gives `ABCDE` then `FGABC` and an empty third
  row (TEST52).
- `TCOPY` takes rectangles of any size, down to the largest coordinates,
  and copies only what is on the screens (TEST51). `TSCREEN` of 65536
  cells a side is `Out of range` (TEST51).

- `INPUT`'s cursor is a bar 2 pixels wide and 8 tall at the left of the
  cursor's cell, opaque white whatever `COLOR` is (`COLOR &H80FFFF00`
  over a character changes only the bar's two columns, and the rest of
  the cell is left as it was). It alternates with nothing (TEST50).
- Characters typed at `INPUT` and then deleted leave their cells empty:
  whatever the console showed there before does not come back (TEST50).
- The text drawing's per-cell inverting flag (colour times 1 minus the
  glyph's alpha) is not what draws `INPUT`'s cursor (TEST50).

- The line's cursor blinks by a count kept with the frame count `MAINCNT`
  reads: 20 frames on, 20 off. TEST56's two shots, off at frame 72 and on at
  299 of the line, fit that with the count 1 to 3 frames behind `MAINCNT`,
  and 15/15 only at 3 ahead, which is weak support; the code says 20.
- Insert toggles overwriting on `INPUT`'s line, and the cursor is then a
  block over the whole cell: 24 wide at scale 3 (TEST56).
- The cursor's scale is the text's, `TSCALE` and `LMATRIX` included. It is
  drawn right after its own screen's cells, under a sprite in front, and it
  replaces what its layer holds, alpha too. As a layer's alpha is what shows
  through, on a composed layer it adds its white to the frame rather than
  covering it: `&H203080` under `LAYER 2,1,&HFFC0C0C0` shows `&HE0F0FF`
  (TEST56).
- `INPUT` selects nothing: Shift with Left or Right moves the cursor, and
  Backspace and Delete take one character (TEST56). `LINPUT` runs with the
  same setting in SmileBASIC 4. Direct mode's line does select, cut,
  copy and paste.
- Bit 23 of a value for `TLOAD` or `TUPDATE` (attribute bit 7) does nothing
  on text screens 0 to 3: a plain glyph shows, under `TCOLOR` and `TBLEND`
  too, a space and an empty cell show nothing, and `TCOPY` carries it the
  same (TEST56). `TLOAD`, `TUPDATE` and `TCOPY` move cells raw: all 16
  attribute bits come back from `TSAVE`, `TARRAY` and `CHKCHR`'s `OUT`, the
  others with no effect either (TEST56).

Typing doesn't stop at the end of a row (TEST60 part A, on a real Switch with a
USB keyboard: `ACLS:XSCREEN 320,180,1,2:TSCREEN 4,8,26,12:LOCATE 0,9`, then about
60 plain letters). The line wraps at the console's width and goes on over as many
rows as it takes, the console scrolling as it goes, and the last row stays empty.

Read from SmileBASIC 4 (version 4.4.8) and SmileBASIC 4, and not
yet seen on hardware:

- Only the console has a program with the per-cell flag of attribute bit 7,
  so that on it the bit should invert a cell: its colour is multiplied by 1
  minus the glyph's alpha, and then, as for any text, by `TCOLOR` and the
  colour by the resulting alpha, so the light is left with the square of it.
  A space is a block, and so is an empty cell. Direct mode's selection sets
  the bit, and Enter does not draw the line again, so it should stay.
- Typing over a selection in overwrite mode writes the character and moves
  on first, then deletes the selection: one made rightwards takes the
  character with it, and one made leftwards keeps it.
- Copying a selection in direct mode drops the selection; cutting too.
- A line typed in direct mode or for `INPUT` or `LINPUT` holds 256 characters,
  whatever they are and whatever the console's size. A further character is
  dropped, also over a selection, which then stays, and pasting that would not
  fit (counting a selection as still there) pastes nothing.
- As Enter ends a line, the console scrolls so that the cursor stays above its
  last row; `INPUT`'s and `LINPUT`'s guides, errors and `OK` do the same. What a
  program prints can still reach the last row. On a screen whose rows are 16
  display pixels high or less (of the 720), the hint bar at the bottom covers
  more of them: 2 rows stay free at 360 rows of pixels, 3 at 720 with 8x8
  characters. The same scrolling while `INSPECT` prints has been seen (`TEST19`:
  the last row stays free, on the usual screen); the rest is from the code.
- `SQR` below 0 and `ACOS` past -1 to 1 are `Out of range` on their argument, as
  `LOG` and `ASIN` are, seen below.
- `VSYNC` of 0 or less does not wait, and counts on from now.
- `BREPEAT`'s negative values are `Out of range`.
- `TLOAD`'s width and height have to be 1 or more, and `GLOAD`'s 0 or more, as for
  `TSAVE` and `GSAVE`, seen below.
- `IRREAD` in mode 1 is `Illegal IR Camera mode`.
- A new `TALK` cuts off what is being said.

Seen in TEST63 (4.4.9 on a Switch 2), most of it after being read in SmileBASIC 4:

- `ACLS` holds the program for the frame job as `XSCREEN` does, and `MAINCNT` has yet to
  rise when the program goes on: `T=MAINCNT():ACLS:?MAINCNT()-T` and the same with
  `XSCREEN 400,240,2` print 0, with a `VSYNC` before or not, and `ACLS:ACLS` 1. A
  `WAIT 1` or a `VSYNC 1` right after takes that rise, and the next one a frame:
  `VSYNC:XSCREEN 400,240,2:T=MAINCNT():WAIT 1:U=MAINCNT():WAIT 1:?U-T;MAINCNT()-U` prints
  1 1, and so with `VSYNC 1`.
- `HELPGET` of a name the help does not have gives four empty arrays of strings
  (`TYPEOF` 5, `LEN` 0). `HELPINFO("NOSUCHNAME")` is no error either: it gives an array,
  so that `?` of it is `Type mismatch (PRINT:1)`.
- A `PUSHKEY` of keys without Enter is dropped at the prompt too, as with Enter: a line
  typed after `PUSHKEY "?7:"` runs as it is.
- After a clean start of SmileBASIC, before any program, `ENVSTAT`'s OUT form gives status
  1 with line 386, of a system program that went before. A program that ends without an
  error sets the status to 0 and leaves the line: 386 after it, and after a program's
  error on line 3, 3 through the clean runs that follow.
- `UISTATE(2)` read 648 at that point, and 0, 1, 3, 4, 5 read 0.
- `CONTROLLER(0)` read 2, `CONTROLLER(1)` 1 and `CONTROLLER(2)` 0; `CONTROLLER 2 OUT A,B,C`
  gives 0 for all three: no colours on a controller not connected.
- `CHKCALL("PRINT")` and `CHKCALL("GOTO")` are 0, statements not being commands, and
  `CHKCALL("3:SIN")` 1 and `CHKCALL("3:NOSUCH")` 0. For a slot that has not been run,
  `CHKLABEL("3:@A")` is 0, `VAR("3:A")` `Undefined variable` and `RESTORE "3:@A"`
  `Undefined label`.
- `ARYOP 6` (clamp) takes the lower bound first: 5 between 9 and 1 gives 9, and a NaN stays.
- `BIQUAD` with arrays of 4 and 8: `Illegal function call (BIQUAD:2)`. With `ZS[2,8]` and
  `ZT[8]`: `Illegal function call (BIQUAD)`, no argument named.
- `BGMPLAY` of a user's song number that holds nothing leaves what is playing as it is.
- `PRGNAME$()` and `PRGSIZE()` at the prompt mean slot 0, whichever slot `PRGEDIT` is on.
- `INPUT A,B` given `1,` takes A=1, B=0 at once. Given `1,2,3` it leaves the 3 out, not
  looking at it. `INPUT VAR("A$")` and `N$="A$":INPUT VAR(N$)` both take a string.

Seen in TEST62 (4.4.9 on a Switch 2), after being read in SmileBASIC 4:

- `LOG(0)` and `ASIN(2)` are `Out of range (LOG:1)` and `(ASIN:1)`. `LOCATE 3E10,0` is
  `Overflow (LOCATE:1)`.
- `INSPECT` cuts a string of exactly 256 characters too: no closing quote from 256 up,
  255 has it.
- `BREPEAT` takes a start of 0 as 1: `BREPEAT 5,0,3:BREPEAT 5 OUT S,I` gives 1 and 3.
- `TCOORD` mode 0 drops the fractions of what it is given, 3.9,2.9 with `TOFS 0,100,50`
  giving 103,52. Mode 2 gives the home point for an axis scaled to 0.
- `TSAVE`'s width and height have to be 1 or more: 10x0 and 10x-1 are `Out of range
  (TSAVE:5)`, and 5000x1 goes, reaching off the screen. `GSAVE`'s -1 is `Out of range
  (GSAVE:4)`.
- `LMATRIX layer,array` wants a real array of 16 elements or more and looks at the first
  16: 15 are `Type mismatch (LMATRIX:2)`, 17 go, and a value of 1E39 is `Out of range
  (LMATRIX:2)`.
- `BQPARAM` looks at the number of its arguments before their values: type 5 with no
  gain is `Wrong number of arguments (BQPARAM:2)`. A Q of 0 is `Illegal function call
  (BQPARAM:5)`.
- `UISTATE(5)` is 0, and `UISTATE(6)` `Out of range (UISTATE:1)`.
- `IRREAD` on a camera never started is `IR Camera is not available (IRREAD)`.
- `HELPGET "NOSUCHNAME",0 OUT ...` gives no error.
- `?VSYNC()` is `Wrong number of OUT arguments (VSYNC)`, and `FREEMEM 1` and `?FREEMEM(1)`
  `Wrong number of arguments (FREEMEM)`.
- `#B_X`, `#B_B`, `#B_Y`, `#B_A` are 0, 1, 2, 3, and `#TUSRCHR` 59392. `1E308*10` less
  itself prints `nan`.

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

- A sprite or text cell whose pixel centers fall exactly between texels can show a pixel
  off its image: an image side and a scale that are powers of two read the page's higher
  texel, and anything else mostly the lower, at any flip or turn (TEST63, 3831 sprites and
  192 cells on a Switch 2; renderer-design.md has the numbers). So on a half-pixel position
  16x16 shows the page's next row at its bottom edge, and 15x15 the column before it at
  its left. On a cell, what lies beside it on the font page or `TPAGE`. Before the page's
  first texel, U or V of 0, it reads the first texel; after the last, U+W of 2048, nothing.

- `LAYER` mode 0 draws the layer's elements straight onto the screen and
  ignores the layer's color; only modes 1 to 4 multiply by it.
- Blending rounds to the nearest, not down: a sprite at &H80 alpha over
  60 gives (255x128 + 60x127)/255 = 157.9, shown as 158. With that, the
  emulator matched lossless screenshots of mode 0 and mode 1 layers
  pixel for pixel.
- How colours are worked out, from SmileBASIC 4, and confirmed by TEST49 (about 4,500 patches read from
  lossless screenshots, every one within 1):
  - Each 8-bit value reaches the GPU as k/255, and SmileBASIC 4 work in
    floating point with no 8-bit rounding inside a draw. A draw is
    rounded once, to the nearest, when it is stored: an element into
    its layer (or the screen, for mode 0), a composed layer into the
    screen, and the screen to the display.
  - What is drawn holds premultiplied colour, and its alpha is how much
    of what is behind still shows through. A layer of its own starts
    clear: black, letting everything through.
  - An element with image pixel `t` and colour `c` (SPCOLOR; for text,
    the cell's colour times TCOLOR, multiplied in unrounded) has alpha
    `a = t.a*c.a` and light `t*c*a`. A normal element gives
    `light + d*(1-a)` and leaves `1-a` of what showed through; an
    additive one (`#A_ADD`, `TBLEND` 1) gives `d + light`, held to 1,
    and changes no alpha. `&H808080` tinted `&H7F7F7F` is 63.75, so
    `&H40`, and `&H80FFFFFF` over 60 is 157.9, so 158.
  - A composed layer (LAYER modes 1-4) with premultiplied colour `p`,
    what shows through it `T`, and the layer colour `L` gives the light
    `P = p*L*L.a` and the coverage `A = L.a*(1-T)`. Mode 1 gives
    `P + d*(1-A)`, mode 2 `d + P`, mode 3 `d*(P + 1-A)` and mode 4
    `d + P*(1-d)`, each held to 1. Modes 3 and 4 leave the screen
    opaque, which shows only on a subprogram's screen.
  - `FADE` is done as the screen goes to the display:
    `d + (c-d)*fade.a`, rounded once. Beside a subprogram, the side
    without the focus is darkened in the same step, times 224/255
    (`&HE0`): 255 to 224, 248 to 218, 96 to 84.
  - All the divisions are by powers of 255, which is odd, so no exact
    result lies halfway between two values.
  - TEST49's screenshots come from an emulator running SmileBASIC 4 on a
    PC's GPU, and that GPU stores results its own way: about one patch in
    six has a channel 1 off the rules above, mostly where the exact
    value is a little above a half (`&H80010101` added to 60 is 60.502,
    shown as 60). A screen result cut to 12 bits of fraction,
    with halves rounded down (`floor(x*4096)`, then
    `(q*255 + 2047) >> 12`), gives every FADE sample exactly and nearly
    every additive one. That is the PC's GPU, not SmileBASIC 4; the
    Switch's may round differently.
- A subprogram's screen over the main one: its `BACKCOLOR` sets its
  colour whole and its alpha as how much of the main screen shows
  through, so `&H80FF4000` over black shows `&HFF4000`, and over a
  dimmed `&H77` green `&H40 + &H77*128/255`. Alpha `&HFE` lets nearly
  all of the main screen through, `&H01` almost none, and alpha 0 leaves
  the screen clear. With alpha `&HFF` the subprogram's screen replaces
  the main one where it is, and layers in modes 3 or 4 on it hide the main
  screen whatever the alpha (TEST56). What the subprogram draws goes over
  its `BACKCOLOR` as on any screen.
- A subprogram's screen is stretched smoothly whatever `XSCREEN`'s mode, and
  keeps its aspect ratio in `XSUBSCREEN`'s rectangle: 480x272 in mode 1 on
  the whole display is 2.647 times, starting at X 4 (TEST56). A screen wider
  than its rectangle gets bars above and below: 400x240 in a rectangle of
  600x600 at 60 starts at Y 180. Mode 2 is smoothed the same in a subprogram
  (TEST60). The bars take `BACKCOLOR`'s colour plus the display times 1 minus
  its alpha, unless the alpha is 0: seen over a main screen that was not cleared,
  for 400x240 with `&HC0305070` and 1280x720 with `&H40305070` (TEST62). A
  layer in mode 3 makes the whole subprogram's screen opaque, far from what is on it (TEST62).
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
    `GLINE 0,0,4,2` at 2 and 4. When that end is outside `GCLIP`, the page
    if none is set, `k = major`: the line itself rounded, halves up, counted
    from its real end (TEST56, ten lines, either way round). `GLINE -1,5,30,10` steps at X 3, 9, 15, 21 and 27, `GLINE
    -1000,0,1040,51` gives rows 25 for X 0-19 and 26 from 20, while
    `GLINE 0,25,40,26` steps at X 19 (TEST47, TEST48).
  - `GTRI` draws each pixel once (with `#G_ALPHA`, also where edges
    meet), a row at a time from the leftmost to the rightmost pixel an
    edge reaches in the row, whatever order the corners come in. Each edge
    goes down from its top end with a sum that starts at `-wide` if it is
    wider than tall, or `tall` if not; each row adds `2*wide`, and the edge
    takes a step across for each `2*tall`, at most one past its bottom end.
    A row has the columns from where the last row ended to the one before
    where this one ends. Level edges have none of their own. Corners off
    the page are not clamped. `GTRI 0,0,1,2,10,1` fills rows 0-4, 1-10 and
    1-6. A triangle whose corners lie on one line is that line, drawn once
    (TEST47, TEST48, TEST56).
  - `GBOX` draws the top and bottom rows, then the sides from the top
    row to the row above the bottom: with `#G_ALPHA` the two top corners
    are blended twice and the bottom ones once, whichever corners are
    given first (TEST48). A box one pixel wide
    or high draws each pixel once, except the top of a one-pixel-wide
    box. `GBOX 0,Y,5,Y` on row -2147483648 draws nothing on the page
    (TEST51).
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
- `LFILTER` copies a raster array when it is called: writing to the array
  later changes nothing on the screen until the next `LFILTER` (TEST60).
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
  screenshots (TTEST26). The scale comes before the offset, and a positive
  Y offset moves the point up from the bottom. An array of two values to
  an entry holds the offset and scale along the deformation: X for type 3,
  Y for type 4. With n entries, row or column i of `size` takes entry
  `floor((i+.5)*n/size)`, the one its center falls in (TEST54).
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
  (TTEST36, pixel for pixel). A sprite's Z plays no part: every point goes
  in with z 0, so Z comes from M[2], M[6] and M[14]. What has Z/W outside
  -1 to 1 does not show, nor what has W at 0 or below. Sampling is nearest
  at any magnification, and perspective correct. `LCLIP` on such a layer
  is in frame pixels after the matrix (TEST53).
- The 2D `LMATRIX` form scales first and then turns clockwise, with x and y
  outside, as a sprite does; it applies after each element's own placement
  (TEST53).
- On a layer that `LAYER` does not compose (mode 0), `LFILTER` filters
  the whole screen drawn so far: the layers behind and `BACKCOLOR` too.
  On a composed layer it filters only that layer.
  That holds for a layer with nothing in it, too.
- `LFILTER`'s mosaic draws the whole frame into a few pixels with bilinear
  sampling, and those back over the frame without smoothing. Blocks are n
  frame pixels whatever `XSCREEN`'s sample (TEST54), and their number is
  reckoned in screen pixels: `ceil(width/n)*sample` across. So where n does
  not divide the size, blocks are a little smaller than n, ending at both
  edges of the frame. Sampling puts a block of odd n on its middle pixel,
  and a block of even n exactly between four, which it averages (halves
  rounded down). A size below 2 does nothing.
- `LFILTER`'s blur halves the layer in frame pixels, whatever `XSCREEN`'s
  sample: 2x2 blocks are averaged (halves rounded down, so a dot becomes a
  2x2 block at 64 and a line 127), blurred along the rows and then the
  columns, each stored in 8 bits, and scaled back without smoothing.
  Amounts 1 to 4 look the same; amount 0 leaves the layer sharp.
- The blur's weights are `exp(-d*d/2/v)` for v = amount/10 at d = 1, 3, 5...
  31 (the middle counts as 1, and the next tap as 3, though they are
  neighbours), normalised over all sixteen with those past the middle
  counting twice. They are taken outward until they add up to more than
  99%, which picks 3, 5, 7 or 9 taps, and no shader has more than 9. So
  above amount 128 or so the kernel is cut off without normalising again:
  the edges stay narrow while a plateau falls, and a wide white block
  reaches 251, 224 and 173 at amounts 128, 256 and 384 (TEST54). What shows
  through falls with the light, so a clear layer darkens what is behind it.
- `LFILTER`'s colour conversion works on the layer's premultiplied pixels
  as they are: hue in degrees, saturation and value on 0 to 255, and a grey
  has no saturation to raise. So half-covered white starts from a value of
  128, not 255, and a value of 30 on it gives 158 (TEST54). On a layer in
  mode 1, a clear pixel stays clear. On one in mode 2, 3 or 4, a raised
  value lights every clear pixel too, so the whole screen takes on the
  value times the layer's colour: `LAYER 5,4,&HFF404080` with
  `LFILTER 5,5,200,-50,30` hazes everything by (7,7,15), also where layer
  5 has nothing (TEST53). The filter comes before the layer's colour.
- Scaled and turned sprites and text are drawn without smoothing, at the
  centres of the frame's pixels; at `XSCREEN`'s sample 2, of the fine
  pixels. A pixel whose centre is exactly on the image's edge is drawn if
  that is the left or the bottom edge on the screen, however the image is
  turned (`SPOFS 0,0.5,0.5` draws from the first column, and from the
  second row to one row past the image's height).
- A pixel's sample point exactly between two texels goes by the rule above. What is not a
  power of two shows the GPU's single-precision arithmetic besides: about 1 line in 20 goes
  the other way, by the place on the screen, by a triangle of the two a quad is drawn as,
  or from some line along the image on (TEST63).
- `LCLIP` at `XSCREEN`'s sample 2 takes whole fine pixels: frame pixels
  `2*x0` to `2*x1+1`.
- `XSCREEN`'s composite modes all fill as much of the display as fits, at
  the aspect ratio, with black bars and no `BACKCOLOR` in them: 1000x600 in
  mode 2 fills 1200x720 with uneven pixels, not 1000x600 at whole factors.
  Mode 0 is bilinear and mode 2 nearest. Mode 1 mixes the two nearest
  texels by `clamp(2*fract(s-.5)-.5, 0, 1)` along each axis at any scale
  (s in texels), so a checkerboard at 3x reads 0, 42, 212, 255, 212, 42
  (TEST55). That equals bilinear from a copy at twice the size, which is why
  mode 0 at sample 2 looks the same as mode 1 at sample 1.
- At equal Z text screens are behind sprites, and among either the higher
  number is behind, `#GSPRITE` among the sprites as 4095 (TEST50).

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
- `WAVSET` also takes groups in square brackets, which the manual leaves
  out: `WAVSET 224,127,0,0,127,"[FF00][80][4080C0]",69` is no error
  (TEST56). A string starting with `[` has groups of an even
  number of digits, at most 1024 in all, and no group over 512 over their
  number in samples. Each is stretched to that many samples, but each is
  written from the start, so the last group alone stays and the rest of the
  512 samples is whatever the buffer held. `RECSAVE` shows it (TEST60):
  `[C040]` sounds like the same 16 digits without brackets, and in
  `[C040][F010]` the `F010` fills half of the pass with the other half loud
  noise, the same each time (peak about 4600 at 127, a DC of about -1500).
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
- `@MA`'s swing is read in tenths of a dB, truncated, and stops at the sound
  engine's limits: no more than +6 dB, and silence from -90.4 dB down. A wide
  one such as `@MA127,110,12,0` (±655 dB as asked) therefore chops the note
  into bursts at twice its level with silence between, and is no louder
  than that: a song using three of these plays at a normal level on SB4.
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
- `WAVSETA` with an empty array is `Illegal function call`. Arguments after
  the array can be left off the end, but one left out in between is `Type
  mismatch` with its number, in both forms and ahead of the check for an empty
  array: `WAVSETA 224,0,0,0,0,A%,,0,15` is `(WAVSETA:7)`, and `...,A%,69,,15`
  and `...,A%,69,,,28160` are `(WAVSETA:8)` (TEST51, TEST52). A trailing comma
  counts: `...,A%,69,0,15,28160,` is `(WAVSETA:11)` (TEST57).
- `WAVSETA` looks at its arguments in order: reference note 0 to 127 (`:7`),
  start and end 0 to the last element (`:8`, `:9`), rate 1 to 96000 (`:10`),
  offset 0 to the last element (`:11`), and a flag of any integer, true if not 0
  (`:12`). Then a start past the end is `Illegal function
  call`, no number: `...,A%,69,10,5` and `...,A%,69,10,5,28160` (TEST57). A start
  equal to the end is no error and defines nothing, in both forms, so neither
  does an array of one element: `...,A%,69,5,5` (TEST57). Then in the 16-bit
  form an end the offset leaves no room for is `Out of range`: `...,A%,69,0,15,
  28160,4` on 16 elements is `(WAVSETA:11)` (TEST57). On an empty array the
  range comes first in the 16-bit form: `DIM A%[0]:WAVSETA 224,0,0,0,0,A%,69,0,0,
  28160` is `Out of range (WAVSETA:8)` (TEST58).
- `BEEP` of a waveform set by `WAVSETA` can be silent: `DIM W%[32]`, `WAVSETA
  224,127,127,127,127,W%,69` and `BEEP(224)` gives a handle and no error, and
  nothing is heard (TEST58). `WAVSET` and `WAVSETA` also write the sound `BEEP`
  plays, a note at the reference pitch as long as a pass of the waveform, cut to
  ticks of 1/192 of a second: 32 and 128 samples at 28160 Hz come to no note and
  record as silence. `WAVSET` always has a note of a second; `WAVSETA`'s 8-bit
  form 147 samples or more, once rounded up to a power of two, so 256; its 16-bit
  form as long as (end + 1) / rate (TEST59). A start equal to the end returns
  before any of this, so the old waveform and its `BEEP` stay: a `WAVSET` wave
  plays identically after it (TEST59).
- The sound of such a `BEEP` ends on the sound engine's steps of 5 ms, each 0.96
  of those ticks, the first included, and takes one more step to fall. The last
  sample that is not silent is at 999 ms for `WAVSET` (192 ticks), 9 ms for an
  8-bit 256 samples (1 tick), 144 ms for 4096 samples (27 ticks) and 999 ms for
  16-bit 28160 samples at 28160 Hz, each with its level steady up to there
  (TEST59).
- With `WAVSETA`'s flag not 0, the note repeats until stopped, each pass a new
  note over the last one's fall: 28160 samples of a ±30000 square at 28160 Hz, at
  `BEEP`'s default, peak at 4234 and at 7905 in the last 5 ms of each second, heard
  as a momentary noise once a second (per 1/10 s of the peak,
  `4444444448444444444944444444494`). `BEEPSTOP` after 180 frames leaves the last
  sample at 3004 ms, the step's fall (TEST59).
- The notes before do not stop: they fall by the envelope's release however long
  that takes, so with a slow one they pile up. 2816 samples of a ±10000 square at
  28160 Hz with release 32, a pass of about 100 ms, one note peaking at 1411, peak
  at 9456 at an RMS of 4333, and per 1/10 s of the peak `235567789877654` with
  `BEEPSTOP` of the handle at 1 s (TEST61).
- `BEEPSTOP` without a fade lets all the handle's notes go and frees the handle:
  the sound goes on past it by the release, the 1.5 s recording above still sounding
  at its end, and at release 127 that is one step, as when the 16th `BEEP` after a
  statement's takes over its place: a 1 s square ends at 404 ms for a `BEEP` at
  400 ms (TEST61). The note is the same with or without the
  flag, and `BEEPSTOP` with a fade takes the volume to 0 first, so nothing is heard
  after it. The sound engine has 96 voices for everything, BGM too; a new note takes
  one from the lowest priority there is, the oldest among equals, or does not sound
  if all are higher.
- `BEEP`'s volume is a linear gain of v/127 the same way
  as `BGMVOL`'s, with 64 for its default as `BGMPLAY` has. It comes out level with
  `BGMPLAY`'s default: at envelope 127,127,127,127, a `WAVSET` square of
  `"FFFF0000"*8` peaks at 4623, an 8-bit `WAVSETA` square of ±100 at 3613 and a
  16-bit one of ±30000 at 4234 (TEST59), as `@224` at `V127` of a ±96 square peaks
  at 3468 under `BGMPLAY`, for an RMS of 3063 (TEST40): these are all 4624 for full
  scale, and an RMS of 4084. A square peaks about 13% over its RMS for how
  the console resamples it, where a sine does not: amplitude 100 peaks at 3217 at
  an RMS of 2257 (TEST40). `BEEP` of the same shapes records the same: the ±96 square
  an RMS of 3050 and a peak of 3468, the sine 2257 and 3217. A square's level at
  volume 127 and 32 is 6053 and 1525, in `BEEP`'s argument and by `BEEPVOL` alike
  (TEST61).
- `BEEP` 224 to 255 with nothing set plays a sine of about 440 Hz for a second,
  peak 4117, last sample at 999 ms (`BEEP 250`, TEST59). The sound archive's `@224`
  is that sine, 16384 samples at 28160 Hz; `@225` to `@255` are 140 samples of
  silence there, but all of `@224` to `@255` start on `@224`'s waves: in MML too,
  `T120@250V127O4A2` and the same on `@240`, never set, peak at 4089 at an RMS of
  2881 and sound from 0 to 999 ms (TEST61). `BEEP`'s sound
  for each is `T400V127P64@nT60O3A4` until set, and `WAVSET` of a number replaces
  only that number. Nothing a program can run puts them back: not `SNDSTOP`, `ACLS`
  or `CLEAR`.
- `BEEP`'s result is a number 0 to 15 that goes on rising with each `BEEP`, also
  after the last has ended and for a silent one, and through program runs: running
  TEST59's parts in the order 20, 16, 17, 18, 19 on a started SmileBASIC 4 gave 0,
  1, 2, 3, 4, 5, 6, 7, 8 (TEST59). It goes round at 16 and nothing starts it again:
  not `ACLS`, not `RUN`; by SmileBASIC 4 each interpreter counts for itself.
  The number is a place to play in: `BEEP` used as a function holds its place until
  the sound ends or `BEEPSTOP` names it, and the next `BEEP` passes it by; `BEEP` as
  a command does not, and the 16th after it ends it. With all 16 held, the result is
  -1, nothing plays and there is no error. A number stopped is the next given, and
  the one after it next. `BEEPSTOP`, `BEEPVOL`, `BEEPPIT` and `BEEPPAN` act on what
  plays in that place now, do nothing if that has ended, and cut a sound that has
  since taken the number (TEST61). By the code they take 0 to 15 only, so -1 is
  `Out of range`.
- `CHKMML` gives -1 for 16 million characters of notes, and for macros
  nested six and seven levels deep that would come to 10^7 and 10^8
  characters. `BGMPLAY` of nine such levels hangs SmileBASIC 4 so that even
  the stop key does nothing (TEST51).
- `BEEP` of a preset effect at its default volume comes out at 100/127 of its own
  note in MML for `BEEP 0` and 70/127 for `BEEP 256`, which is the effects' own
  volume: `BEEP 0` peaks at 703 where `T130@256V127@V101O4C24` peaks at 893, `BEEP
  256` at 1542 where `T130@0V127O4C4` peaks at 2798, and `BEEP 95` at 1455 (RMS over
  the first 100 ms 312, 429 and 303). The volume is linear, as for waveforms: `BEEP
  0` at 127 and 32 peaks at 1395 and 351 (TEST61).
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
  variables take none of it. A string of N characters takes 2N rounded up to
  128 bytes, plus 64: 25 and 64 characters take 192, 65 and 128 take 320, 129
  takes 448, 256 takes 576, 257 takes 704, 1000 takes 2112 (TEST57). An array
  takes its elements (Int 4, Real and string 8) rounded up to 64 bytes, at
  least 128, plus 64: 17 ints 192, 33 ints 256, 49 ints 320, 1000 ints 4096, 9
  reals 192, 17 reals 256, 100 reals 896 (TEST57), `DIM A[100000]` 800064.
  Strings of up to 24 characters and arrays of up to 64 bytes of elements take
  nothing. `GARRAY` and `TARRAY` take nothing. Memory let go of (emptying a
  string or array, a DEF's locals after it returns) does not come back at once.
  Both of its results are the same.
- Length is limited by memory only: 63,000,000 characters, 33,000,000 ints and
  16,000,000 reals all work (TEST57).
- `Out of memory` comes from what fits in `FREEMEM`'s 134216720 bytes
  (TEST51). `"A"*N` and `S$+S$` make 33,554,432 characters but not
  67,108,864. `ARRAY#` makes 8,388,608 reals but not 16,777,216. Of five
  `DIM X%[8000000]`, the fifth fails with 6.2 million bytes free. `RESIZE`
  and `COPY` grow to 16.7 million integers but not 33.5 million.
- Growing an array needs the new block beside the old: `INSERT` of
  16,777,216 integers into 15,728,641 fails though the 130 MB it comes to
  would fit alone, and `RESIZE` from 8 to 16 million, 100 MB together, works
  (TEST51). `PUSH` is no different: with `DIM A%[15999999]` and `DIM F[8000000]`,
  6216592 free, the first `PUSH` works with `FREEMEM` unchanged, its block being
  as large as it was, and the second is `Out of memory (PUSH)` (TEST57). The block moves each time its size changes, and `UNSHIFT`,
  string arrays and strings (each 64 characters) do the same.
- A block that moved leaves a hole, so near the end `FREEMEM` overstates what can
  be had (TEST58). With 1296 bytes free beyond a block's size: `UNSHIFT` on
  `DIM A%[100000]` (400064 bytes, 401360 free) gets to length 100016 with 401296
  free and then is `Out of memory (UNSHIFT)`; `PUSH` on `DIM A$[100000]` (800064,
  801360 free) gets to 100008 with 801296 free; `PUSH` on a string of 512000
  characters (1024064, 1025360 free) gets to 512064 with 1025232 free, each then
  `Out of memory (PUSH)`.
- A shrinking block moves too, and `POP` with no room for the new one leaves the
  length as it is, with no error. With `DIM A%[1000001]` and 1000016 free, a
  `PUSH` and a `POP` stay in the block, the next `POP` leaves the length at 1000001
  and `FREEMEM` at 1000016, and a `PUSH` after it gives 1000002. Twenty `POP`s in a
  row still leave 1000001 (TEST58). So a block stays only while its size is the
  same, not for 64 bytes more. Then `POP` still gives the last element, the same
  one each time; `SHIFT` still gives the first, with the rest moved down and the
  last there twice; a string's `POP` and `SHIFT` and an array of reals or strings
  go the same way; and `ENVSTAT` shows nothing (TEST59). The size makes no
  difference: `DIM A%[1601]` with 5008 free, too little for its next block down of
  6464, keeps its length and gives 901 each time with `FREEMEM` at 5008 (TEST59).
  `REMOVE` moves the rest down and then is `Out of memory (REMOVE)`: `REMOVE D%,1`
  on 1000001 ints leaves the length, `D%[0]`=11, `D%[1]`=13, `D%[999999]`=901, and
  the last, `D%[1000000]`, read past the block, was 0 the one time it was seen.
  `RESIZE` to smaller is `Out of memory (RESIZE)` having changed nothing (TEST59).
- A string constant takes no block of its own until it is stored. With 100048
  free, `X$=POP("ABC"+"X"*100027+"XYZ")` gives `Z` and leaves 100048, no error, where
  `SHIFT` of it, which needs a copy, is `Out of memory (SHIFT)`. Stored in `S$` the
  same constant takes 200256 bytes (TEST59).
- `A%=LOADV("DAT:Q")` makes the new array beside the old: 10 MB of file with
  4216464 free is `Out of memory (LOADV)`. `LOADV "DAT:Q",A%` fills the array,
  gives no error and leaves `FREEMEM` as it was (TEST57). If the file is longer,
  the array grows the way `RESIZE` does, and `A$=LOADV("TXT:Q")` makes its string
  the same way. Seen in TEST58: `S$=LOADV("TXT:Q")` of
  1,000,000 characters (2000064 bytes) over a string as long is `Out of memory
  (LOADV)` with 1000016 free and works with 3000016 free. `LOADV "DAT:Q",A%` of a
  file of 9 million integers into `DIM A%[8000000]` is `Out of memory (LOADV)` with
  34000016 free and works with 38000016 free, leaving 34000016.
- The message names the command for functions and commands
  (`Out of memory in 0:68(ARRAY#)`, also `(RESIZE)`, `(INSERT)` and `(COPY)`)
  but not for `*`, `+` or `DIM` (`Out of memory in 0:54`). Its number is 11.
- Dropped strings and arrays are freed on demand. A dropped string of 100,000
  characters stays counted in `FREEMEM` after `PRINT`, a loop, two `VSYNC`s
  and `WAIT 60`. Making 16 million characters 20 times over, with or without
  `VSYNC`, takes `FREEMEM` through 100662224, 67107728, 33553232 and back to
  100662224, and never fails: what was dropped goes when the next request
  would not fit (TEST52).
- Constant strings are worked out as the program compiles, so what is too
  long stops it before any of it runs (TEST52, TEST57). `?LEN("A"*(1<<N))`
  works up to N=14 in direct mode, then is `Out of code memory`; in a program it
  works up to N=20, then is `Out of memory`, in slots 0 and 1, with nearly all
  memory free. Through a variable the same size works. Each statement has 4 MiB to compile in, from which every string worked out on
  the way takes 24 bytes and its characters, rounded up to 8 bytes, and stays
  until the statement ends: `"A"*1000000+"B"*1000000` is too long there, as are
  `?"A"*1500000,"B"*1500000` and `A=LEN("A"*2097140)`, each `Out of memory` at its
  line (TEST58): each constant and the rest of the statement take from the same
  4 MiB. `A=LEN("A"*2097068)` compiles and prints, and
  2097069 and 2097116 are `Out of memory` as it compiles (TEST59), which is 168
  bytes for such a statement, none of them for the variable it stores to. A string
  then goes into the code, which is 32768 16-bit words in direct mode and
  4194304 in a slot, taking a word a character plus its length: of three lines of
  `A=LEN("A"*2000000)`, the third is `Out of code memory` (TEST58).
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
  characters; `PUSHKEY` from 64 is `String is too long`.
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
- `SUBRUN "/A/../B"` prints `OK`, then the same block with `Illegal
  format` as its middle line (TEST51), and `SUBRUN "DAT:Z51A"` does too (TEST57).
- `SUBRUN` takes a name in lower case and one starting with `TXT:`:
  `SUBRUN "test51s"` and `SUBRUN "TXT:TEST51S"` give no error and no block.
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
- A bare `GOTO` or `GOSUB` runs the lines before it and stops there with
  `Type mismatch` (TTEST39).
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
- Only a function's `RETURN` takes a value. In a `DEF` without brackets, also one
  with an `OUT`, what follows `RETURN` is simply the next statement, never run,
  and has to compile as one: `RETURN 5`, `RETURN A+1` and `RETURN A*2` are `Syntax
  error`s when the program compiles, at the `5`, the `+` and the `*`, as is `RETURN F()`
  at the `F` (TEST60, TEST62). `RETURN F(1)` compiles, a command with a value in
  brackets (TEST62). `RETURN X,Y` too, a command X, in `DEF G A OUT X,Y`
  (KRB_OGIRI_SMBOMB); `RETURN UNDECLARED` too, under `OPTION STRICT` (SMILEBOY).
- A compile error sets `ENVSTAT OUT S,N,A,SL,C,L` as an error when running does but for
  the status, which stays 0: S 0, N 3, A -1, C the character of the token it names from
  the start of the program, L its line (TEST62).
- `VAR A` without a value only declares: in `FOR I=0 TO 1:VAR A:?A:A=5:NEXT` it reads
  0, then 5. A function that ends without `RETURN`, or leaves by a bare one, gives
  no value: `TYPEOF` 0. `IF 1 THEN ?1 ENDIF:?2` prints both. `NEXT J` on `FOR I` is
  not checked: `J` stays as it is. A string with no closing quote ends with its line.
  A comment line after a `\` line end is no blank line: `?1\`, `'COMMENT`, `+2` is a
  `Syntax error` at the `+`. `STOP` with a message prints 255 characters of it (TEST62).
- Nesting: `GOSUB`s, `DEF` calls, `RESTORE "PUSH"` and the values of expressions share a
  stack of about 259072 slots, by the depths they reach (TEST63, 4.4.9 on a Switch 2). A
  `GOSUB` takes 1. A command `DEF` called takes 6, a function 8, and each argument, `OUT`
  and `VAR` of it 2: printing its depth, a program reaches 43178 with a command and no
  arguments, 32383 with a function and none or a command and one, 25906 with a function
  and one, 21589 with one argument and one `VAR`, 18504 with a function and three, and
  14392 with one argument and four `VAR`s. `GOSUB` reaches 259069. `RESTORE "PUSH"` went
  past 172672 but not 172736, at 1.5 slots each (inferred). `Stack overflow` comes on the
  call when the call does not fit; when it fits, on the first statement that puts a value
  there, of the body: line 2 of `DEF R(N)`, `LOCATE 0,0:?N`.
- A digit group's `_` must lead to a digit: `?&HFF_` and `?&H_` are `Syntax
  error`, and so is `?1_000`, where a decimal number takes none (TEST60).

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
