# SmileBASIC 4: what the manual omits

Behavior checked on SmileBASIC 4 for Nintendo Switch that the manual does not
describe, or that is easy to read the wrong way. The emulator follows these
facts except where [differences.md](../using/differences.md) says otherwise.
Items still waiting for a check are marked `UNVERIFIED` in the source instead.

TEST and TTEST numbers name check programs kept outside the repository;
[hardware-tests.md](hardware-tests.md) describes the method. Each fact keeps its
test as evidence, and says "on a Switch 2" or "on Ryujinx" where the machine
matters. A real Switch outranks Ryujinx where the text says so.

TEST62 and TEST63 ran on 4.4.9 on a Switch 2. "(Not yet seen on hardware.)"
marks what is expected of version 4.4.8 and has not run. A fact that names no
test has not been checked on hardware either, unless it says so.
TEST71, TEST72, TEST74 and TEST76 ran on 4.4.9 on a Switch 1 and a Switch 2.
TEST76K, done by hand, and TEST76R, recorded over HDMI, ran on a Switch 2.

## Contents

- [Syntax and compiling](#syntax-and-compiling)
  - [When errors appear](#when-errors-appear)
  - [Statements, literals and names](#statements-literals-and-names)
  - [`DEF`, `OUT` and `RETURN`](#def-out-and-return)
- [Values and variables](#values-and-variables)
  - [Numbers](#numbers)
  - [`RGB`, `RGBF` and `HSV`](#rgb-rgbf-and-hsv)
  - [Random numbers](#random-numbers)
  - [`FORMAT$`](#format)
  - [Strings](#strings)
  - [Arrays](#arrays)
  - [`SPVAR`, `TVAR` and `INC`](#spvar-tvar-and-inc)
- [Order of evaluation](#order-of-evaluation)
- [DATA](#data)
- [Timing](#timing)
  - [`VSYNC` and `WAIT`](#vsync-and-wait)
  - [`MAINCNT`](#maincnt)
- [Animation](#animation)
- [Input](#input)
  - [Buttons](#buttons)
  - [Keyboards and controllers typing](#keyboards-and-controllers-typing)
  - [USB keyboards](#usb-keyboards)
  - [`PUSHKEY` and the key buffers](#pushkey-and-the-key-buffers)
  - [`INPUT`, `LINPUT` and the line being typed](#input-linput-and-the-line-being-typed)
  - [The line's cursor](#the-lines-cursor)
  - [Controllers, vibration and the IR camera](#controllers-vibration-and-the-ir-camera)
- [Screens and text](#screens-and-text)
  - [`ACLS`, `XSCREEN` and `TSCREEN`](#acls-xscreen-and-tscreen)
  - [Cells](#cells)
  - [`TROT`, `TOFS` and `TCOORD`](#trot-tofs-and-tcoord)
- [Sprites](#sprites)
  - [Sprite state and links](#sprite-state-and-links)
  - [Collision](#collision)
  - [`SPFUNC` and `CALL SPRITE`](#spfunc-and-call-sprite)
  - [`SPCHR`](#spchr)
- [Graphics and layers](#graphics-and-layers)
  - [Drawing on pages](#drawing-on-pages)
  - [`GPUTCHR` and `GPUTCHRP`](#gputchr-and-gputchrp)
  - [Colors and blending](#colors-and-blending)
  - [`FADE`](#fade)
  - [`LAYER`, `LCLIP` and `LMATRIX`](#layer-lclip-and-lmatrix)
  - [`LFILTER`](#lfilter)
  - [Sampling and draw order](#sampling-and-draw-order)
  - [`XSCREEN` display modes](#xscreen-display-modes)
- [Sound](#sound)
  - [`BGMPLAY`, `BGMSTOP` and `BGMVOL`](#bgmplay-bgmstop-and-bgmvol)
  - [MML](#mml)
  - [Instruments, `WAVSET` and `WAVSETA`](#instruments-wavset-and-wavseta)
  - [`BEEP`](#beep)
  - [`TALK` and sung notes](#talk-and-sung-notes)
  - [Effector, recording and balance](#effector-recording-and-balance)
- [Signal processing](#signal-processing)
- [DIRECT mode](#direct-mode)
- [Stopping](#stopping)
- [Program slots and files](#program-slots-and-files)
  - [`CHKCALL` and `CHKLABEL`](#chkcall-and-chklabel)
  - [Slots, `EXEC` and the `PRG` commands](#slots-exec-and-the-prg-commands)
  - [Files](#files)
- [Subprograms](#subprograms)
  - [Starting and ending](#starting-and-ending)
  - [Focus](#focus)
  - [A subprogram's screen](#a-subprograms-screen)
  - [`ENVSTAT` and `ENVLOCATE`](#envstat-and-envlocate)
- [Memory](#memory)
  - [`FREEMEM`](#freemem)
  - [`Out of memory`](#out-of-memory)
  - [Blocks that move](#blocks-that-move)
  - [Strings evaluated as the program compiles](#strings-evaluated-as-the-program-compiles)
- [System settings](#system-settings)
  - [`SYSPARAM`](#sysparam)
  - [`HELPGET` and `HELPINFO`](#helpget-and-helpinfo)
  - [Other system commands](#other-system-commands)
- [Editor](#editor)

## Syntax and compiling

### When errors appear

- An error message starts at the cursor, on the line where the program left the
  cursor: after `?"V";` an `Overflow` shows as `VOverflow in 1:3` (TEST65). A
  line typed in DIRECT mode does the same (TEST63).
- An error in a line typed in DIRECT mode names no place, and a space comes
  before the command it names: `Out of range (LOCATE:2)`,
  `Can't access other project in this mode (RENAME:1)` and `Syntax error`
  (TEST70).
- An error in some builtins names the command right after the place, with no
  space between: `Overflow in 1:3(FILL:2)`, `(PUSH)`, `(COPY)`, `(HEX$:1)`,
  `(CHR$:1)` and `(MID$:2)`. The number counts arguments from 1. `LEFT$` with
  2147483648 as its second argument shows `(LEFT$)` with no number, and with -1
  `Out of range (LEFT$:2)` (TEST71). Every builtin seen so far but `SQR` (see
  [Numbers](#numbers)) names itself, regardless of the error: `MID$(S$,-1,1)` is
  `Out of range in 1:2(MID$:2)`, `FILL M%,0,5` on three elements `(FILL:3)`,
  `LOAD "GRP0:X"` `Illegal format (LOAD:1)` and `LOADV "TXT:A",S$`
  `Wrong number of arguments (LOADV)` (TEST67). A value that fails as it goes
  into the array gets no number: `UNSHIFT M%,2147483648` is
  `Overflow (UNSHIFT)`, the same as for `PUSH` and `COPY` (TEST66, TEST67).
  Operators and plain stores name nothing: DIV, MOD, the bit operators, unary
  minus and `M%[0]=V` give `Overflow in 1:2`, and DIV or MOD by 0
  `Divide by zero in 1:2` (TEST65, TEST66).
- More names TEST69 saw: `ASC("")` is `Can't use empty string (ASC:1)`, error
  67; `TPUT 4,2,1,67,128` is `Out of range (TPUT:5)` and `ATTR 128`
  `Out of range (ATTR:1)`; `SPOFS` of a sprite not set is `Sprite not set
  (SPOFS)`, with no number. A `DEF` function or command given the wrong number
  of arguments is `Wrong number of arguments in 3:2`, with nothing after the
  place.
- `COPY` past the source's end is `Subscript out of range`, error 31, in every
  form (TEST71, TEST72). The function form names the argument that goes past: on
  five elements, `COPY(B,6)` is `(COPY:2)` and `COPY(B,2,5)` `(COPY:3)`, and
  `COPY("ABCDE",6)` and `COPY("ABCDE",2,5)` the same. The command form names
  none: `COPY A,0,B,6,1` and `COPY A$,0,"ABCDE",2,5` are `(COPY)`. An offset
  equal to the length with a count of 0 is no error: `COPY A,0,B,5,0` (TEST72).
  The function form fails there, with the count omitted or 0: `COPY(B,5)` on
  five elements and `COPY("ABCDE",5)` are `(COPY:2)` (TEST74), and so is
  `COPY(B,5,0)` (TEST76). An empty source is the exception: `COPY(B,0)` of an
  empty array and `COPY("",0)` give an empty result, with no error.
- `COPY` past the destination's end is no error, and grows it. After
  `DIM A[5],B[5]:COPY A,7,B,0,1`, `A` has 8 elements, and
  `A$="12345":COPY A$,7,"AB",0,2` gives `12345  AB`, spaces filling the gap
  (TEST74).
- `COPY` with a number first is `Type mismatch (COPY:1)` in both forms:
  `COPY Z,Z,B,Z,1` and `Q=COPY(Z,Z,1)`. The command form checks the
  destination before anything else (TEST76).
- `CHR$(-1)` is no error: it gives `CHR$(&HFFFF)` (TEST69).
- `CHR$` takes any int and keeps its low 16 bits: `CHR$(65601)` is `"A"`,
  `CHR$(-2)` U+FFFE, `CHR$(&H7FFFFFFF)` U+FFFF, and `CHR$(65536)` and
  `CHR$(-65536)` a one-character string holding U+0000 (TEST70). A real
  outside the int range is `Overflow (CHR$:1)`: `CHR$(2147483648)` and
  `CHR$(-1E10)` (TEST71).
- `ATTR` names an attribute out of range by its own argument: `ATTR 4,128` is
  `Out of range (ATTR:2)`. In the form with flips and a turn, the flips take 0
  or 1 and the turn 0 to 3. A horizontal flip of 2 is `(ATTR:1)`, a vertical
  flip of 2 `(ATTR:2)` and a turn of 4 `(ATTR:3)`; after a screen ID they are
  `(ATTR:2)`, `(ATTR:3)` and `(ATTR:4)`. `TFILL`'s attribute past 15 is
  `(TFILL:7)`, found before the corners are read (TEST71).
- An error message longer than the console's width wraps the same way `PRINT`
  does (TEST71).
- `Wrong number of arguments` comes when a command or function gets a number of
  arguments none of its forms takes (`LOCATE` with five, `TOFS 0,30`). Most
  calls find this as they run, not as the program compiles, so the lines before
  them run.
- The functions SmileBASIC 4 compiles in place find it as the program compiles
  when called with brackets: a line `?ABS(1,2)` stops the program before its
  first line runs, also behind `IF 0 THEN`, past a `GOTO` and in a `DEF` that
  is never called. They are `ABS`, `ACOS`, `ASC`, `ASIN`, `ATAN`, `CEIL`,
  `CHKCALL`, `CHKLABEL`, `CHKVAR`, `CHR$`, `COS`, `COSH`, `DEFARG`, `DEFARGC`,
  `DEFOUTC`, `DEG`, `DIM`, `EXP`, `FLOAT`, `FLOOR`, `HEX$`, `INT`, `LAST`,
  `LEFT$`, `LEN`, `LOG`, `POP`, `POW`, `RAD`, `RIGHT$`, `RND`, `RNDF`,
  `ROUND`, `SGN`, `SHIFT`, `SIN`, `SINH`, `SQR`, `STR$`, `TAN`, `TANH`,
  `TYPEOF` and `VAL`. `MIN` and `MAX` are compiled in place with two arguments
  only; with any other number they are called, and `MIN()` fails as it runs.
  As commands (`ABS 1,2 OUT A`) all of them check as they run (TEST76).
- `?VSYNC()` is `Wrong number of OUT arguments (VSYNC)`, and `FREEMEM 1` and
  `?FREEMEM(1)` are `Wrong number of arguments (FREEMEM)`. (TEST62)
- Whether a call no form takes is `Wrong number of arguments` or
  `Wrong number of OUT arguments` depends on which count the command checks
  first (TEST76). Most check the `OUT` count first. A number of `OUT`
  variables no form takes is `Wrong number of OUT arguments` (`ACLS OUT A`,
  `LOCATE 0 OUT X,Y,Z`). One that a form takes, with arguments that form does
  not take, is `Wrong number of arguments` (`COLOR 0,0 OUT C`). Some commands
  check the argument count first, among them `DATE$`, `DELETE`, `DTREAD`,
  `MOUSE` and `RANDOMIZE`: `RANDOMIZE 0,0,0 OUT A` is
  `Wrong number of arguments`. `SPROT`, `SPCOLOR`, `SPHOME`, `SPSCALE` and
  `SPCOL` check that a sprite number is given before the `OUT` count, so
  `SPROT OUT A,B` is `Wrong number of arguments`.
- `RNDF 0,1 OUT X`, with two arguments, is `Wrong number of OUT arguments` as
  it runs. `RNDF(0,1)` and `RNDF(0,0)` are `Wrong number of arguments` as the
  program compiles (TEST76).
- The functions and array commands do the same: a function used as a command
  with other than one `OUT` variable (`ABS Z OUT A,B`, `LEN S$`), or
  `PUSH A,1 OUT X`, is `Wrong number of OUT arguments` when the call runs.
  `FILL A OUT X` checks the `OUT` count first (error 53), `ABS OUT A,B` the
  arguments (error 52). `COPY` and `RGB` take their form from the `OUT` count,
  so `Q=COPY(A,0,1,0)` and `Q=RGB(1)` are `Wrong number of arguments`. A command
  used as a function has one `OUT` variable: `Q=PUSH(A,1)` and `Q=FILL(A,0)`
  compile, and are `Wrong number of OUT arguments` when the call runs, and
  `Q=PUSH(A)` `Wrong number of arguments` when it runs. `FIND A,V OUT I` gives
  the place by element order, the same as `FIND(A,V)` does (TEST76).
- A string or array argument of the wrong type is named by its place: `LEN(1)`
  is `Type mismatch (LEN:1)`, `INSTR(0,1,S$)` `(INSTR:2)` and `PUSH 1,1`
  `(PUSH:1)`, the same as for numbers (TEST76).
- A call to a name defined nowhere compiles, and is a `Syntax error` only when
  it is reached: the name may be a `COMMON DEF` of a slot that `EXEC` runs
  later.
- `GOTO` and `GOSUB` to a label that is not there are `Undefined label` only
  when they run.
- `GOTO` and `GOSUB` take a string `"@NAME"` or `"n:@NAME"`, with a slot n of
  0 to 5. Another string, such as `"L"` or `"6:@L"`, is `Illegal label string`
  (35), and a number `Type mismatch`. In a `DEF`, `"@NAME"` looks only at the
  `DEF`'s own labels, and `"n:@NAME"` is always `Undefined label` (TEST76).
- `GOTO` or `GOSUB` with nothing after it runs the lines before it and stops
  there with `Type mismatch` (TTEST39).
- Operations on constants are evaluated when the program compiles, so `?"A"*-1`
  or `?1/0` stops the program before it runs, even on a line that is never
  reached. A number plus a string is left to fail as it runs: `A=1+"A"` is
  `Type mismatch` when its line runs, after the lines before it (TEST76).
- An unknown `#constant` is `Undefined constant`.
- A `CONST` or `ENUM` constant exists from its position in the source. Used on
  an earlier line, also in a `DEF` above it, in `DATA` or in another `CONST`
  (`CONST #A=#B,#B=1`), it is `Undefined constant`. One defined in a `DEF`
  counts on the lines after it, top level included. The program compiles before
  it runs, so a `GOTO` past the `CONST` does not matter (TEST76).
- SmileBASIC 4 compiles a program one statement at a time, in source order,
  `DEF` bodies at their position in the source. The first error in the source is
  the one reported: `?#ZZ:?(` is `Undefined constant`, and so are
  `IF #ZZ THEN ?(`, `?#ZZ` on a line before `A=&H100000000`, and `?#ZZ` before
  `CONST #A=1,#A=2` (TEST76).
- A block that does not close has its own error, on the line its statement
  starts on, as the program compiles: `FOR without NEXT` (20),
  `WHILE without WEND` (24), `REPEAT without UNTIL` (22), `LOOP without ENDLOOP`
  (54), `CASE without ENDCASE` (62) and `THEN without ENDIF` (26). After an
  `ELSE` it is `ELSE without ENDIF` (27), still on the `IF`'s line, and `ELSEIF`
  keeps 26. A `DEF` with no `END` is `DEF without END` (29) on the `DEF`'s line.
  When blocks are nested, the innermost is named. The statements before the end
  compile first, so an `Undefined constant` in the block comes before it
  (TEST76).
- A `DEF` that starts inside a block, or a `DEF`'s `END` inside one, is that
  block's error: `FOR I=0 TO 9`, `DEF F` is `FOR without NEXT` on the `FOR`'s
  line, and so is a `DEF`'s `END` before its `NEXT`. `IF A THEN END` in a
  `DEF` is `THEN without ENDIF`. A `DEF` inside a `DEF` is `Nested DEF` (32),
  on the inner one's line (TEST76).
- A closing keyword with no matching block to close is an error on its line:
  `NEXT without FOR` (21), `WEND without WHILE` (25), `ENDLOOP without LOOP`
  (55), `UNTIL without REPEAT` (23), `ENDIF without IF` (28),
  `WHEN without CASE` (59), `OTHERWISE without CASE` (60) and
  `ENDCASE without CASE` (61). `NEXT`, `WEND` and `ENDLOOP` look past `IF`s and
  `CASE`s to the innermost loop, which has to be theirs; the others look at the
  innermost block only, so an `UNTIL` inside an `IF` is error 23, and so is an
  `ENDIF` inside a `FOR` error 28. A stray `ELSE` or `ELSEIF` is a
  `Syntax error`, on the line of the block it is in (TEST76).
- A one-line `IF` lasts until its line ends, and so do the blocks it holds:
  `IF A THEN FOR I=0 TO 9` with the `NEXT` on a later line is
  `FOR without NEXT`. An `IF` or `ELSE` in it that continues past the line takes
  its blocks with it: `IF A THEN IF B THEN` ends at an `ENDIF` (TEST76).
- `BREAK` and `CONTINUE` outside a loop are a `Syntax error` as the program
  compiles (TEST76).
- `CASE` takes `WHEN` or `OTHERWISE` next, on its line or a later one:
  `CASE A:ENDCASE` is a `Syntax error`, placed at the `ENDCASE` (TEST76).
- A call with more than 254 arguments or `OUT` variables, and a `DEF` with
  more than 254 parameters or `OUT`s, is `Too many arguments`, error 48, as
  the program compiles. A call with 254 compiles and runs (TEST76).
- `CONST` or `ENUM` of a built-in constant's name is `Duplicate constant`, error
  49, as the program compiles: `CONST #ON=#C_YELLOW,#OFF=#C_WHITE` on line 38 of
  slot 0 stops with `Duplicate constant in 0:38` (checked on hardware, version
  4.4.9). `#_LINE`, `#_SLOT` and `#_FILENAME` count as built in, and so does a
  constant defined before it in the slot's program, inside a `DEF` or not, in
  any case of letters. Each slot keeps its own. `CONST` evaluates its expression
  before it checks the name, so `CONST #ON=#NONE` is `Undefined constant`,
  placed at `#NONE`; `ENUM` checks the name first. The error is placed at the
  name (TEST76).
- `ENUM` takes a real only as a whole number: `ENUM #A=1.0` and `ENUM #A=4/2`
  give ints, and the next name continues counting from them. A fraction, as in
  `1.5` or `#PI`, is `Type mismatch`, error 8. A whole real outside -2147483648
  to 2147483647, such as `3E9`, is `Out of range`, error 10. The fraction is
  checked first, so `2147483647.5` is `Type mismatch`. These errors, and
  `Type mismatch` for a string, are placed at the value's first operand: at
  `1.5` in `-1.5`, at `1` in `(1)+0.5` (TEST76).
- A `CONST` or `ENUM` value that is not evaluated as the program compiles, such
  as `CONST #A=Z`, is `Illegal constant expression`, error 51, placed at the `Z`
  (TEST76). In `DATA` it is a `Syntax error`.

### Statements, literals and names

- A variable's suffix only sets its starting value. `A%=1E10` keeps the real,
  `A$=1` stores a number and `A%="X"` a string.
- The literal `2147483648` is a real. So is a decimal integer of more than
  10 digits, leading zeros included: `00000000001` is the real 1. A decimal
  literal of more than 99 characters, with an exponent of more than 3 digits,
  or too large for a real (`1E309`) is a `Syntax error` (TEST76).
- `&H` takes up to 8 digits and `&B` up to 32, leading zeros included: more,
  as in `&H100000000` or `&H000000001`, is a `Syntax error` as the program
  compiles (TEST76). `?&H100000000` is that `Syntax error`, not `Overflow`.
- `CONST` and `DATA` take `HSV` as well as `RGB`.
- `OPTION STRICT` applies from its position in the source to the end of the
  program, in and out of `DEF`s, and may be in a `DEF` or a block. Nothing turns
  it off. Each use of a variable is checked against the variables made so far:
  one used before it without `VAR` can be used after it, a `DEF` above it is not
  checked, and a `VAR` further down does not count yet (`OPTION STRICT:?A:VAR A`
  is `Undefined variable` at `?A`). Only `VAR`, `DIM` and a `DEF`'s parameters
  and `OUT`s make variables; `FOR`, `READ`, `INPUT`, `LINPUT`, `SWAP`, `INC`,
  `DEC` and `OUT` targets of a call are uses. An `OPTION` with another name is a
  `Syntax error` at its position, in a `DEF` too (TEST76).
- `OPTION DEFINT` applies from its position in the source to the end of the
  program, in and out of `DEF`s, and may be in a `DEF`. A variable without a
  suffix starts as an int only if it is first seen after it:
  `?TYPEOF(A):OPTION DEFINT:?TYPEOF(A);TYPEOF(B)` prints 2, then 21, and in
  `DIM X[2]:OPTION DEFINT` X is an array of reals (TEST76).
- Statements do not need a colon between them where a statement cannot continue:
  - A `PRINT` item ends where no `;` or `,` follows it: `?"A" B=1` prints A,
    then sets B.
  - `NEXT` takes a loop variable only if a name follows (`NEXT?I`).
- `T?` is short for `TPRINT`.
- A label used as a value keeps its case (`A$=@Lp` is `"@Lp"`). As a place to go
  to, case does not matter.
- `THEN @L` may be followed by more statements on the line, which belong to the
  branch.
- `ELSEIF A GOTO @L` takes GOTO in place of THEN.
- A constant's name may start with a digit, the same as a label name (`#2P`).
- `#` after a number makes it a real (`0#`).
- `\` joins its line with the next, and the rest of its line is ignored:
  `?1\ XYZ` and `+2` on the next line prints 3.
- A comment line after a `\` line end is no blank line: `?1\`, `'COMMENT`, `+2`
  is a `Syntax error` at the `+` (TEST62).
- `_` may appear anywhere among the digits of `&H` and `&B`. Ones after the last
  digit are not part of the number but the next token, a name: in DIRECT mode
  `?&HFF_` and `?&H_` are a `Syntax error` (TEST60), and in a program `?&HFF_`
  calls `_`. A decimal number takes no `_`: `?1_000` is a `Syntax error`
  (TEST60).
- A string with no closing quote ends with its line (TEST62).
- A string has no escape for a quote: `?"A""B"` is a `Syntax error` (TEST71).
- `NEXT` (also `NEXT I`), `WEND` and `ENDLOOP` inside an `IF` or another block
  of a loop continue with the next iteration of the loop: `IF A THEN NEXT`.
- `NEXT J` on `FOR I` is not checked: `J` is unchanged (TEST62).
- `VAR A` without a value only declares: in `FOR I=0 TO 1:VAR A:?A:A=5:NEXT` it
  reads 0, then 5 (TEST62).
- `IF 1 THEN ?1 ENDIF:?2` prints both (TEST62).
- `WHEN`s with nothing between them share the statements after the last one: in
  `WHEN 1:WHEN 2:?"X"`, a 1 prints X too. SmileBASIC 4's own programs list
  several values this way. A `WHEN` meant to do nothing needs its own statement,
  or its value kept out of the `CASE`.
- A function's name without parentheses is a variable. `T=MILLISEC` reads a
  variable named MILLISEC, which is 0 unless the program set it, so
  `WHILE MILLISEC-T<25:WEND` never ends; under `OPTION STRICT` it is
  `Undefined variable` as the program compiles. This holds for every built-in
  function (`MAINCNT`, `FREEMEM`, `RND`, `ABS`) and for a `DEF` function. Only
  `NAME(...)` calls. SmileBASIC 3 had `MAINCNT` and `MILLISEC` as system
  variables, so code carried over from it needs the parentheses added.
  SmileBASIC 4's own TGAME_SHOOTER has `IF MAINCNT AND 15 THEN RETURN` in a
  `DEF`, where that `RETURN` never runs. (Not yet seen on hardware.)
- A variable may have a built-in function's name: `MILLISEC=5`, `VAR MILLISEC`
  and `DIM RND[3]` are accepted, and `RND(1)` still calls the function. The
  name alone as a statement is a command call instead: `MILLISEC` on its own
  is `Wrong number of OUT arguments` as it runs. (Not yet seen on hardware.)

### `DEF`, `OUT` and `RETURN`

- A DEF's parameters may be written `A[]`. Passing a number for one and indexing
  it is `Type mismatch` on the indexing line.
- A function may be called as a command with one `OUT` variable for its result:
  `ABS -3 OUT A`, `SPCHR 0 OUT D`, `SPSET 242 OUT IX`, `HSV 120,255,255 OUT C`.
- An `OUT` variable a `DEF` never sets gives the omitted value (`TYPEOF` 0),
  regardless of its suffix. Inside the `DEF` it reads as that too.
- A `DEF` sees a global only if top-level code above the `DEF` names it, because
  the program compiles in one pass. Otherwise the name is local to the `DEF`.
- A name a `DEF` uses before its `VAR` follows the same rule (TEST76). With
  `A=5` or `VAR A=5` at the top level above the `DEF`, `DEF F:?A:VAR A=1:?A`
  prints 5, the global, then 1: the `VAR` makes a local for the rest of the
  `DEF`. With no global above, the first use makes `A` local to the `DEF`, and
  the `VAR` is `Duplicate variable` as the program compiles, placed at its `A`.
  At the top level, `?A:VAR A` is `Duplicate variable` too.
- A `DEF` with an `OUT` may be called as a function, giving the `OUT` value
  (`DEF F A OUT R` as `?F(3)`). A `DEF` that returns a value may be called with
  one `OUT` (`T OUT X`). One with two `OUT`s called as a function is
  `Wrong number of OUT arguments` when the call runs.
- `RETURN` takes a value only in a function. In a `DEF` without brackets, also
  one with an `OUT`, what follows `RETURN` is the next statement, never run, and
  it has to compile as one:
  - `RETURN 5`, `RETURN A+1` and `RETURN A*2` are `Syntax error`s when the
    program compiles, at the `5`, the `+` and the `*`, and so is `RETURN F()`,
    at the `F` (TEST60, TEST62).
  - `RETURN F(1)` compiles, as a command with a value in brackets (TEST62).
  - `RETURN X,Y` compiles too, as a command X, in `DEF G A OUT X,Y`
    (KRB_OGIRI_SMBOMB).
  - `RETURN UNDECLARED` compiles too, under `OPTION STRICT` (SMILEBOY).
- A function that ends without `RETURN`, or leaves by a bare one, gives no
  value: `TYPEOF` 0 (TEST62).
- `DEFARG()`, `DEFARGC()`, `DEFOUTC()`, `CHKLABEL()`, `CHKCALL()` and
  `CHKVAR()` are compiled in place, not called. As the program compiles,
  another number of arguments is `Wrong number of arguments`, and then the
  `DEF` ones outside a `DEF` are `Can't use outside DEF`, error 56. `DEFOUT`
  outside a `DEF` is error 56 too, before what follows it is read.
  `CALLIDX(1)` is called, and is `Wrong number of arguments` when it runs
  (TEST76).
- All of them called as commands, such as `CHKVAR "A" OUT F` or `DEFARGC OUT N`,
  run their handlers, which check as they run: two `OUT`s are
  `Wrong number of OUT arguments`. `DEFARG 0 OUT A` outside a `DEF` is error 56
  as it runs, while `DEFARGC` and `DEFOUTC` give 0 there (TEST76).
- A `DEF` with the name of a system function is `Duplicate function` when the
  program compiles, at the `DEF`, the same as two `DEF`s of one name:
  `DEF LAST(T)` on line 41 of slot 3 is `Duplicate function in 3:41` (TEST71).
  `DEF BEEP`, `DEF BGMPLAY N`, `DEF SIN(X)` and `COMMON DEF LAST(A)` are
  `Duplicate function` too, error 19, also with a `BEEP` used above the `DEF`.
  `DEF PRINT` is a `Syntax error` (TEST72).
- `GOSUB`s, `DEF` calls, `RESTORE "PUSH"` and the values of expressions share a
  stack of about 259072 slots, by the depths they reach (TEST63, 4.4.9 on a
  Switch 2). A `GOSUB` takes 1. A command `DEF` called takes 6, a function 8,
  and each argument, `OUT` and `VAR` of it 2.

  | What a program calls while printing its depth | Depth reached |
  |---|---|
  | A command with no arguments | 43178 |
  | A function with none, or a command with one | 32383 |
  | A function with one | 25906 |
  | A function with one argument and one `VAR` | 21589 |
  | A function with three | 18504 |
  | A function with one argument and four `VAR`s | 14392 |
  | `GOSUB` | 259069 |

  `RESTORE "PUSH"` takes 1.5 slots. A loop of `RESTORE "PUSH":IF I>=172600
  THEN LOCATE 0,0:?I` printed 172712 and stopped with `Stack overflow` in the
  next pass. After 100000 of them, `@L:N=N+1:LOCATE 0,0:?N:GOSUB @L` printed
  109069 (TEST64). `Stack overflow` comes on the call when the call does not
  fit. When it fits, it comes on the first statement after it that puts a
  value there: line 2 of `DEF R(N)`, `LOCATE 0,0:?N`. In both loops above it
  came on the `LOCATE`, past `IF I>=172600` and `N=N+1`, and `ENVSTAT` gives
  the `LOCATE`'s character (TEST64).

  A `GOSUB` without room is not reported on its target's first statement
  either. After 100000 pushes, a `GOSUB @S` to lines 4 to 7, `@S`, `D=D+1`,
  `LOCATE 0,1:?D;` and `GOSUB @S`, printed 109068 and stopped on line 6:
  `D=D+1` ran and the `LOCATE` stopped, as `ENVSTAT`'s character shows
  (TEST69).

  `PRINT` with a value needs room for it too. `@L:N=N+1:?N:GOSUB @L` stops
  with `Stack overflow` on the `?N` with N at 259072, and so do the same loop
  with `ON 0 GOSUB @L` and with `GOSUB "@L"`. Without the `?N`, the 259073rd
  `GOSUB` stops on itself (TEST71).

  `RESTORE "PUSH"` itself continues past that: 172713 of them pass, and 180000,
  400000 or 1000000 stop on the loop's line (TEST67, TEST69). After the 172713,
  `VSYNC`, `CLS`, `X=ABS(B)`, `?N`, `X=1+2` and a bare `PRINT` run and the
  program continues, while `LOCATE 0,0` stops. `A=F(1)` of a one-line `DEF F(P)`
  runs after 172000 or 172500 pushes and stops on its own line after 172712 or
  172713 (TEST67, TEST69).

## Values and variables

### Numbers

- `STR$` of a real gives six significant digits, not `PRINT`'s eight decimals:
  `STR$(2/3)` is `0.666667` where `?2/3` shows `0.66666667` (TEST60). It is C's
  `%g`, so a million is `1e+06`.
- `"A"*-1` is `Overflow`, not `Out of range`.
- `-2147483648 DIV -1` wraps to -2147483648.
- Unary minus of the int -2147483648 wraps to -2147483648 and stays an int
  (`TYPEOF` 1). Binary `+`, `-` and `*` give a real past 32 bits instead:
  `0-A` is 2147483648 (TEST65).
- `AND`, `OR`, `XOR`, `NOT`, the shifts and the rotates truncate a real toward
  zero, and take a real outside the 32-bit range as 2147483647 or -2147483648,
  with no error. `2147483648 AND -1` is 2147483647, `-2147483649 OR 0` is
  -2147483648, `4294967296 AND 1` is 1, `2147483648<<1` is -2, `2147483648>>+1`
  is -1073741825 and `NOT 2147483648` is -2147483648 (TEST65).
- The bit operators take a NaN as 0, with no error: `NAN AND -1`, `NAN OR 0`,
  `NAN<<1` and `NAN>>>0` are 0, `NOT NAN` is -1 and `1<<NAN` is 1. An infinity
  saturates: `INF AND -1` is 2147483647, `-INF OR 0` and `NOT INF` are
  -2147483648, `INF>>1` is 1073741823. A right operand saturates too:
  `1<<2147483648` is -2147483648 and `-1 AND 2147483648` is 2147483647 (TEST66,
  the NaN and infinity made at run time).
- `DIV` and `MOD` check a real operand the same way a store into an int array
  does, before dropping the fraction. `2147483647.5 DIV 1`,
  `-2147483648.5 MOD 10`, `2147483648 DIV 2`, `7 DIV 2147483648` and `INF DIV 1`
  are `Overflow`, and `NAN MOD 7` is 0 (TEST66).
- Shift counts are taken modulo 32: `1<<33` is 2, `1<<-1` is -2147483648.
- `MOD` takes the sign of the dividend.
- `ROUND` rounds halves away from zero.
- Reals print with eight decimals, trailing zeros removed, never in exponent
  form: `1E20` prints all 21 digits, `1/3` as `0.33333333`, `-1E-9` as `-0`.
- `1E308*10` less itself prints `nan` (TEST62).
- `VAL` takes underscores in `&H` and `&B` digits the same way a program does:
  `VAL("&H_FF")` and `VAL("&HFF_")` are 255. `VAL("1_000")` and `VAL("&H_")` are
  0 (TEST56).
- `VAL(" 12 ")` is 0: spaces after a number make the text not a number.
  `INPUT` asks again (`?Redo from start`) for the same reason.
- `VAL` skips spaces and tabs before a number and reads only up to a comma or a
  line break: `VAL(" 12")` is 12, so `VAL` reads back a number padded by
  `FORMAT$("%2D")`, and `VAL("7,8")` is 7. (Not yet seen on hardware.)
- `LOG(0)` and `ASIN(2)` are `Out of range (LOG:1)` and `(ASIN:1)` (TEST62).
- `ACOS(2)` is `Out of range (ACOS:1)`, the same as for `LOG` and `ASIN`.
  `SQR(-1)` is `Out of range` with no command name and no argument's number
  (`Out of range in 3:1`), and `ENVSTAT` gives its argument as -1 (TEST71).

### `RGB`, `RGBF` and `HSV`

- `RGBF` rounds components down: `RGBF(0.5,0.5,0.5)` is `&HFF7F7F7F`.
- `RGBF` clamps components outside 0 to 1.0 (`R=1.1:?HEX$(RGBF(R,0,0))` is
  `FFFF0000`, an integer variable too). A constant one outside them is
  `Out of range` when the program compiles (`RGBF(1.1,0.5,0)`, `RGBF(1,-1,0)`).
- `RGB` takes components outside 0 to 255 and clamps them: `RGB(584,0,0)` is
  `&HFFFF0000`, a component of -1 gives 0, and so do alpha 300 and
  `RGB(C,,256,,)`.
- `HSV` and `HSVF` as functions need all three of H, S and V: two are
  `Wrong number of arguments`, and empty ones (`HSV(120,,)`) are
  `Type mismatch`.
- `HSV` with one argument is `Wrong number of OUT arguments`, because it is read
  as the `OUT` form.
- `HSV`'s `OUT` form needs at least three results too, and fewer is
  `Wrong number of OUT arguments`. S and V may be left empty, which is what the
  4.4.9 notes mean: `HSV C OUT H,,`.

### Random numbers

- `RND(0)` is 0 and takes no number from the series. A negative maximum is
  `Out of range`.
- Each random series is TinyMT32 with its standard parameters, seeded with
  `RANDOMIZE`'s value as a 32-bit number (-1 is the same seed as &HFFFFFFFF),
  regardless of the series number.
- `RND(n)` is the next number modulo n.
- `RNDF()` takes two numbers and makes 53 bits of them the same way TinyMT's
  `res53` does.

### `FORMAT$`

- `FORMAT$` takes its conversions in lower case too and ignores values left
  over.
- `FORMAT$` errors: too few values is `Wrong number of arguments`, a value of
  the wrong type `Type mismatch`, and a conversion it does not know (`%Q`)
  `Illegal format`.
- `%F` rounds to the nearest, but a value exactly half way goes toward zero:
  `%.0F` gives 2 for 2.5, 3 for 3.5 and -2 for -2.5.
- `FORMAT$` gives at most 1024 characters as a whole, not per field:
  - `%1000D%24D` gives 1024 and `%1000D%25D` is
    `String is too long (FORMAT$:3)`, numbered by the value being formatted.
  - `%S` of 1025 characters is `(FORMAT$:2)`.
  - A format of 1025 plain characters is `(FORMAT$)` with no number, and so is
    `%%` over the limit (TEST57).
  - Of 1, `%1024D` works and `%1025D` does not. `%.1000F` works (a length of
    1002) and `%.1023F`, which would be 1025 long, does not (TEST51, TEST52).
  - `%F` of 1.5 gives 1023 characters at `%.1021F` and `%1023F`. `%.1022F` and
    `%1024F` are `String is too long (FORMAT$:2)` (TEST58): `%F` needs one more
    character of room than it gives.
- `%S`'s precision only limits the string, so `%.2000S` of a short string is
  fine (TEST57).
- `%05S` pads with zeros, `[00ABC]`, and with `-` it pads spaces on the right
  (TEST57).
- `%D`'s precision is the least number of digits (TEST58). Where the precision
  takes the place of the `0` flag, as in `%05.3D`, the padding is spaces:

  | Format | Of 5 | Of -5 |
  |---|---|---|
  | `%.3D` | `005` | `-005` |
  | `%5.3D` | `  005` | ` -005` |
  | `%-5.3D` | `005  ` | `-005 ` |
  | `%05.3D` | `  005` | ` -005` |

  `%.1D` of 0 is `0`, `%.0D` of 5 is `5`, and `%.0D` of 0 is nothing.
- `%X` and `%B` behave the same as `%D`: `%.4X` of 255 is `00FF`, `%.4B` of 5 is
  `0101`, `%+.0D` and `%.0X` of 0 are empty, and `%6.3X` of 10 is `   00A`
  (TEST63).

### Strings

- Assigning a string shares it instead of copying it. After `B$=A$`,
  `B$[0]="X"` and `PUSH B$,"M"` change `A$` too, and `B$[1]="YY"` puts two
  characters in place of one under both names. `O$=Z$[0]` shares the array
  element the same way (TEST68).
- A DEF's string parameter is the caller's string: `S$[0]="Y"` in the DEF
  changes the caller's variable. A DEF that returns its parameter returns the
  same string, so after `Q$=SAME$(P$)`, `Q$[0]="X"` changes `P$` (TEST68).
- `COPY(A$)` and concatenation make a new string: after `E$=COPY(D$)` or
  `I$=I$+"Q"`, changing one leaves the other alone, and a DEF given `R$+""`
  changes a copy. A literal stays as written: `L$="UVW":L$[0]="X"` in a loop
  gives `UVW` again on the next pass (TEST68).

### Arrays

- `DIM` with a size of 0 works only for one dimension: `DIM A[0]` is fine, but
  `DIM A[0,0]` and `DIM A[0,3]` are `Subscript out of range`. `DIM A[2,0]` names
  the 0 as its argument 2, the same as `ARRAY%(2,0)` does (TEST76).
- An array holds nothing (`TYPEOF` 0) until its `DIM` runs, for example when a
  DEF reads it before then.
- `DIM A[,]` compiles, and is `Type mismatch` when it runs.
- `DIM A[A]` compiles its size before `A` exists. Under `OPTION STRICT` it is
  `Undefined variable` at the size's `A`; without it, it is no error (TEST76).
- `DIM` at the start of a statement always declares, never calls the `DIM()`
  function, so `DIM A,Z OUT X` declares `A` and `Z`. When `A` is already
  declared that is `Duplicate variable`, and after new names the `OUT` is a
  `Syntax error`, both as the program compiles. (Not yet seen on hardware.)
- `DIM A[3]=[1,2,3,4]`, with more values than elements, is
  `Subscript out of range` (TEST71).
- An int array takes a real by truncating it toward zero (-3.9 gives -3, -0.5
  gives 0), but checks the real against -2147483648 to 2147483647 first. So
  `M%[0]=2147483647.5`, `2147483647.9` and `-2147483648.5` are `Overflow`, and
  so is anything farther out (TEST65). A `%` variable keeps any real.
- A NaN stored into an int array is 0. `FILL`, `PUSH`, `COPY` from a real array
  and array literals (`DIM M%[]=[X]`, `DIM M%[2]=[X]`) check a real the same
  way, and truncate one in range: `PUSH M%,2147483647.5` is `Overflow` (TEST66).
- `SORT` keeps equal keys in their order. `RSORT` gives `SORT`'s order in
  reverse, so equal keys come out reversed.
- `SORT` orders strings by character code: `""`, `"A"`, `"AA"`, `"B"`, `"a"`,
  `"b"`.
- `SORT` and `RSORT` take their form from the first argument. A number needs a
  count and an array after it: `SORT 0,1` is `Wrong number of arguments`, and so
  are more than eight arrays. Each array must be one-dimensional (else
  `Type mismatch`) and hold the start and the count (else `Out of range`), both
  naming it: `SORT 1,3,A,B` with `B` of 3 elements is `(SORT:4)`. The start must
  be inside the array, so `SORT E` of an empty array is `Out of range`, and so
  is `SORT 4,0,A` of 4 elements. `SORT OUT Q` is `Wrong number of arguments`,
  `SORT A OUT Q` `Wrong number of OUT arguments` (TEST76).
- `RESIZE` of a 2D array takes every size, the later ones unchanged:
  `RESIZE A,3` on `DIM A[2,3]` is `Wrong number of arguments`, and changing a
  later size `Illegal function call`.
- `RESIZE` looks at each size in turn. Past 2^30 elements so far it is
  `Array is too large`, error 47, naming the size; a negative size counts as far
  too large, unless a 0 came before it: `RESIZE A,-1` is `(RESIZE:2)`. Exactly
  2^30 elements is error 47 naming nothing. A later size other than the array's
  current size is `Illegal function call` naming it: `RESIZE A,2,4` on
  `DIM A[2,3]` is `(RESIZE:3)` (TEST76).
- The arrays `GARRAY`, `TARRAY` and `UIGETCMPL` give are the system's own
  memory, and so is any variable they are assigned to. `RESIZE` of one is
  `Can't RESIZE this array`, error 65, naming it, before the number of sizes is
  looked at: `G=GARRAY(0):RESIZE G,9` is `(RESIZE:1)`. `RECSAVE` into one is the
  same error, after the count, the position and the array's type. `LOADV` into
  one keeps its size: it reads only the elements that fit and leaves the rest
  unchanged. The other array commands do not look, and an array that `COPY`
  fills or `LOADV` makes belongs to the program (TEST76).
- `ARRAY%`, `ARRAY#` and `ARRAY$` with a 0 among two sizes or more are
  `Subscript out of range` naming the 0: `ARRAY%(2,0)` is `(ARRAY%:2)`.
  `ARRAY%(0)` is an empty array (TEST76).
- `MIN` and `MAX` take one argument only as an array: `MIN(1)` is
  `Type mismatch (MIN:1)`. An empty array is `Illegal function call`, a string
  array `Type mismatch`, neither naming it. An array among several arguments,
  as in `MIN(A,1)`, is `Type mismatch (MIN)`, naming none. Of ints they give
  an int: `TYPEOF(MIN(1,2))` is 1 (TEST76).
- `RESIZE A,0,3` on `DIM A[1,3]` leaves `LEN` 0 and `DIM(A,0)` 0, and `SAVEV`
  and `LOADV` bring that back (TEST51).
- `LOADV` loads array files with a zero in any size and keeps the sizes: `[0,3]`
  and `[3,0]` both give `RESULT()` 1 (TEST51).
- `REMOVE` with a count of 0 removes nothing and is no error, also on an empty
  array and at the position just past the end.
- `ARYOP 6` (clamp) takes the lower bound first: 5 between 9 and 1 gives 9, and
  a NaN stays. (TEST63)
- `ARYOP` reads a parameter array shorter than the result over again from its
  start. An empty one is `Array is too small`, error 116, naming it:
  `ARYOP 0,R,E,1` is `(ARYOP:3)`. The wrong number of arguments for the
  operation is `Wrong number of arguments` (TEST76).
- `RINGCOPY A,0,A` on an array left by `RESIZE A,0,3` does nothing and is no
  error (TEST51).
- `RINGCOPY` looks at things in this order (TEST57):
  1. A destination that is no array: `Type mismatch (RINGCOPY:1)`. A source that
  is no array, or element types neither the same nor both numbers:
  `(RINGCOPY:3)`.
  2. Either array neither 1D nor 2D: `Type mismatch`, no number. For 2D, the
  first size is channels and the second length.
  3. With four arguments, a count below 0 or over the source's length:
  `Illegal function call (RINGCOPY:4)`. With five, a source offset below 0 is
  `(RINGCOPY:4)`, and a count below 0 or offset + count over the length
  `(RINGCOPY:5)`.
  4. A count below 1 or a destination of length 0: nothing is copied, no error,
  and `OUT` gives 0. `RINGCOPY A,2,B,0 OUT E` gives 0 and
  `DIM A[2,4],B[3,4]:RINGCOPY A,0,B,0` is no error.
  5. Source channels neither 1 nor those of the destination: `Type mismatch`, no
  number, and the same for string arrays that get this far.
- `RINGCOPY` on `A[4]` and `B[3]`, as TEST58 saw it:
  - `RINGCOPY A,0,B,-1,1` is `(RINGCOPY:4)`.
  - `A,0,B,4,0`, `A,0,B,1,3` and `A,0,B,0,-1` are `(RINGCOPY:5)`.
  - `A,0,B,3,0 OUT E` gives 0.
  - A number as the first or the third argument is `Type mismatch (RINGCOPY:1)`
    or `(RINGCOPY:3)`.
  - With five arguments too the destination offset wraps: with B=1,2,3,
    `RINGCOPY A,9,B,1,2 OUT E` leaves 0,2,3,0 and E=3, and `A,-1,B,1,2` leaves
    3,0,0,2 and E=1.
- `RINGCOPY`'s destination offset wraps around with a floored modulo, negative
  too: `RINGCOPY A,-1,B OUT E` on `A[4]` and B=1,2,3 leaves 2,3,0,1 and E=2.
  `OUT` is (start + count) modulo the length (TEST57).

### `SPVAR`, `TVAR` and `INC`

- `SPVAR` and `TVAR` cut a real key down to an integer: keys 1.5 and 1 are the
  same. A number and a string are different keys: 5 is not `"5"` (TEST76).
- String keys take ASCII a-z as upper case, and nothing else: `"name"` and
  `"NAME"` are one key, while full-width `ａ` and `Ａ` are two (TEST76).
- A key of over 1024 characters is never found: written and read back, it
  reads 0. 1024 characters work (TEST76).
- Reading a key that is not there makes the variable, holding 0 (`TYPEOF` 1)
  (TEST76).
- Number keys and string keys of up to 16 characters take no `FREEMEM`. A new
  variable with a key of 20 to 23 characters takes 192 bytes, and so does each
  read of a missing one, because it makes the variable (TEST76).
- Writing variable 7 stops a running `"V"` animation, and `SPCHK`'s or `TCHK`'s
  V bit goes off: `SPVAR 0,7,0` and `TVAR 0,7,0` do it, and so does `SPVAR 0,7`,
  which deletes the variable. The string key `"7"` is another variable, and the
  animation continues (TEST76).
- `SPVAR` and `TVAR` keep an array given to them as the same array, not a copy:
  what is changed through one shows through the other (TEST76).
- `ACLS` clears the sprites' variables but keeps the `TVAR` of every text
  screen, the console included, with any arguments (`ACLS 1,1,1,1` too), against
  the manual. Both survive `XSCREEN` (TEST76).
- `INC` adds to a string in place, the same as `PUSH` does: `INC T$,C$` on a
  DEF's parameter adds to the caller's string.

## Order of evaluation

SmileBASIC 4 evaluates right to left, which shows when the parts have side
effects (a DEF that prints, `RND`, `INKEY$`, `BEEP`):

- Operators evaluate the right side first. `F(1)+F(2)*F(3)` calls `F(3)`,
  `F(2)`, then `F(1)`; `INKEY$()+INKEY$()` puts the first key typed last.
- Arguments of functions, commands and DEFs, and array indices, go last first.
  In `A[F(1),F(2)]=F(3)` the value comes before the indices.
- `PRINT` evaluates every item, last first, before it prints any of them:
  `?P(1);P(2)` with a `P` that prints shows `<2><1>12`.
- `FOR` evaluates `STEP`, then `TO`, again before every test, so they are called
  once more at each `NEXT`.
- Array literals and `DIM` sizes go last first too.
- `SWAP` evaluates each element's place once, the second first.
- `INC` and `DEC` evaluate the amount first, then the place, once.
- `CALL` evaluates its arguments before the name.
- `&&` and `||` still go left to right and stop early.

## DATA

- `SPANIM` with a `"@label"` reads its DATA with `READ`'s own place: it restores
  to the label, and `READ` continues after the values it took.
- `COPY` from a `"@label"` leaves `READ`'s place alone.
- `BGMSETD`'s MML may end with the program's last DATA instead of a 0, and any
  number ends it: in `"..",5,"..",0` the strings after the 5 are not played.
- `RESTORE @LABEL` to a label that is not there is `Undefined label` only when
  it runs.
- A label in a `DEF` belongs to that `DEF`, for `RESTORE` as for `GOTO`.
  `RESTORE @L` and `RESTORE "@L"` at top level look only at top-level labels; in
  a `DEF` they look at the `DEF`'s labels, then the top-level labels, a later
  one included. `RESTORE "n:@L"` looks at the top level of slot n, also from a
  `DEF`. A string other than `"PUSH"`, `"POP"` or a label, such as `"L"` or
  `"6:@L"`, is `Illegal label string`. `SPDEF`, `SPANIM` and `BGMSETD` take
  their DATA label strings the same way, and `COPY` from `"@L"` finds the label
  that way too. (Not yet seen on hardware.)
- There is one `READ` place for all slots. A `READ` in another slot's
  `COMMON DEF` continues from the running program's place: a program in slot 3
  with no DATA, after `EXEC 1`, calling a `COMMON DEF` of slot 1 that `READ`s
  stops with `Out of DATA` on that DEF's line in slot 1, though slot 1 has DATA
  (TEST71).
- A `READ` at the top level of a slot that `EXEC` runs reads that slot's DATA.
  Slot 3 read 31 from its own DATA, `EXEC 1` read 11, slot 1's first value, and
  slot 3's next `READ` gave 32 (TEST72).
- `RESTORE "1:@BEGIN"` in slot 3 moves the one `READ` place into slot 1's DATA:
  slot 3 read 11, a `COMMON DEF` of slot 1 continued with 12, slot 3 with 13,
  and after `RESTORE "1:@OTHER"` the `COMMON DEF` read 21 (TEST72).
- `RESTORE "POP"` with nothing kept by `RESTORE "PUSH"` is `Stack underflow`.
- `RESTORE "PUSH"`, `GOSUB` and DEF calls share one stack, and `RESTORE "POP"`
  takes only a place on top of it: in a subroutine or DEF that kept none since
  it was called, it is `Use RESTORE PUSH before RESTORE POP`, error 119, even
  with places kept before the call. A subroutine called from the top level,
  with no place kept anywhere, gets `Stack underflow` instead; in a DEF it is
  still error 119 (TEST76).
- `RESTORE "PUSH"` can keep a thousand places, and many more: see the stack
  under `DEF`, `OUT` and `RETURN`. A subroutine that keeps a place must take it
  back before `RETURN`, which is otherwise `Use RESTORE POP before RETURN`.

## Timing

### `VSYNC` and `WAIT`

- `VSYNC n` waits until n frames after the frame the previous `VSYNC` waited
  for, not n frames from now. When that frame has already passed, it returns
  immediately and counts from then: a second behind, ten `VSYNC`s take nine
  frames, while one `VSYNC 200` takes 140. After a slow `ACLS`, `VSYNC:VSYNC`
  takes one frame.
- `VSYNC` of 0 or less does not wait, and counts from the moment it runs:
  `VSYNC 0` and `VSYNC -1` return immediately, and a `VSYNC 2` after a `VSYNC 0`
  waits 2 frames (TEST71S).
- `WAIT`, `WAIT 0` included, restarts `VSYNC`'s count; waiting in `INPUT` does
  not.
- `VSYNC` and `WAIT` read only their first argument: `VSYNC 1,2` waits a
  frame. An `OUT` variable is `Wrong number of OUT arguments` as the call
  runs (TEST76).
- `RUN` leaves `VSYNC`'s count unchanged, so the first `VSYNC` of a program,
  `VSYNC 5` too, finds its frames already passed:
  `T=MAINCNT():VSYNC 5:?MAINCNT()-T` as a program's first line prints 0 (TEST62,
  on a Switch 2). A subprogram's first `VSYNC` finds the same, and a program
  waiting by a lone `VSYNC 60` at its start does not wait.
- Counted with `MAINCNT` from just before a subprogram's `PUSHKEY CHR$(13)`, a
  line typed in DIRECT mode runs 1 frame later, and `CONT` continues the program
  1 frame later (once 0, on a Switch 1). `RUN 3` reaches the program's first
  line 2 frames later on a Switch 2 and 4 on a Switch 1, in each of three runs
  (TEST74). TEST76 saw 2 on a Switch 2 and 3 or 4 on a Switch 1, both for a
  program of 5 lines and for one of 2002.
- `XSCREEN` holds the program until the frame job has taken the new mode,
  without touching `VSYNC`'s count, so a `VSYNC 1` right after it does not wait
  (TEST60). The program continues inside that job, past its animation step and
  with `MAINCNT` still to rise.
- `ACLS:XSCREEN 400,240,2`, then a `SPANIM` and `VSYNC 1:SPOFS` in a loop, in a
  program that has not waited yet, shows nothing on the first two rows and the
  first step on the third. The second `VSYNC 1` only sees that job end (TEST62
  on a Switch 2, the same as TEST56 on Ryujinx).
- `ACLS` holds the program for the frame job the same as `XSCREEN` does, and
  `MAINCNT` has yet to rise when the program continues (TEST63):
  - `T=MAINCNT():ACLS:?MAINCNT()-T` and the same with `XSCREEN 400,240,2` print
    0, with a `VSYNC` before or not, and `ACLS:ACLS` prints 1.
  - A `WAIT 1` or a `VSYNC 1` right after takes that rise, and the next one a
    frame:
    `VSYNC:XSCREEN 400,240,2:T=MAINCNT():WAIT 1:U=MAINCNT():WAIT 1:?U-T;MAINCNT()-U`
    prints 1 1, and the same with `VSYNC 1`.

### `MAINCNT`

- `MAINCNT` and `MILLISEC` need their brackets: without them they are plain
  variables, always 0 to begin with.
- A subprogram's `MAINCNT` is the `MAINCNT` of the main interpreter, not counted
  from `SUBRUN`: two `DIALOG` calls answered by one press read the same
  `MAINCNT` on both sides (TEST64).
- `MAINCNT` follows real time while the program is busy: 180 in 3 seconds of a
  loop with no `VSYNC`. When the frame job runs slowly, `MAINCNT` falls behind
  real time with it: 600 sprites of 64x64 scaled by 21 on a Switch 2 gave 49 in
  4077 ms, one `VSYNC` to each (TEST60).
- A program runs this much in 60 frames (4.4.9, timed with `MAINCNT()`). No
  program needs that speed: the emulator runs code as fast as it can.

  | Iterations of the loop | Switch | Switch 2 | Ryujinx |
  |---|---|---|---|
  | `WHILE MAINCNT()-T<60:INC N:WEND` | 2395924 | 5962406 | 2837664 |
  | `FOR J=1 TO 100:X=X+J*2:Y=Y XOR J:NEXT` | 21371 | 52145 | 29141 |

- A DEF call costs about five `GOSUB`s. On a Switch 2, in ns each, with the time
  of the loop around it subtracted (TEST66):

  | Statement | ns |
  |---|---|
  | `GOSUB @S`, to a lone `RETURN` | 28.7 |
  | `V=A+B` | 42.2 |
  | `CM A`, a DEF as a command, which returns nothing | 120.6 |
  | `V=F0()` | 152.7 |
  | `V=F1(A)` | 184.5 |
  | `V=CF(A)`, a `COMMON DEF` | 184.9 |
  | `FO A OUT V` | 189.1 |
  | `V=F3(A,B,X)` | 229.5 |
  | `V=FL(A)`, with two `VAR` locals | 270.1 |
  | `V=FR(3)`, recursive, four calls | 877.2 |

- Code that `EXEC` runs from inside a DEF runs as fast as code at the top level,
  within 5%. A `FOR` loop in a DEF costs 7 to 12 ns more per iteration than one
  at the top level on a Switch 1, and 11 to 12 ns on a Switch 2 (TEST76).
- Single commands, in ns each, with the time of the loop around them subtracted
  (TEST67, 4.4.9; the Switch ran handheld, the Switch 2 docked, `GCLIP` at the
  whole page):

  | Command | Switch | Switch 2 |
  |---|---|---|
  | `V=RND(R)` | 138 | 57 |
  | `V=RGB(R,G,B)` | 404 | 132 |
  | `ON X GOTO`, 2 or 30 labels | 89 | 54 |
  | `SPSET ID,DEF` on a sprite in use | 616 | 213 |
  | `SPSET ID,DEF:SPCLR ID` on a free sprite | 1239 | 422 |
  | `SPOFS ID,X,Y` | 453 | 155 |
  | `GFILL` 8x8 / 128x128 | 639 / 18974 | 238 / 7851 |
  | `GCLS`, the whole 2048x2048 page | 4171396 | 1916861 |
  | `GPAINT` inside a 128x128 / 512x512 box | 370816 / 4329906 | 166276 / 2017006 |

  `ON` costs the same with 2 labels or 30 and with the first label or the last.
  Blending (`#G_ALPHA`) makes a 128x128 `GFILL` 15 to 19 times slower. A program
  busy for 60 frames loses no measurable time to 4000 sprites that are shown,
  rotated and scaled 4 times or hidden.

## Animation

- `SPANIM`, `TANIM` and `FADE color,time` take their first step at the first
  update of the frame job after the command, the three together, regardless of
  what the program was doing. After a `VSYNC` that finds its frame already
  passed, or a long run of code, the next `VSYNC 1` does not wait and it takes a
  second one to see the step.
- The first step comes about a millisecond before `MAINCNT` rises on a Switch 2
  (TEST60).
- `FADE()` still gives the start right after `FADE`.
- Animations also play in DIRECT mode, and continue while the editor is open.
- A sliding animation of rotation, scale or color mixes its two ends in single
  precision as `from*left + to*(1-left)`, `left` being the share of frames still
  to go. Text screens (`TANIM`) do the same as sprites in all of these (TEST47,
  TEST48, TEST56).
  - Rotation and scale keep the result: 0 to -10 over three frames reads
    -3.33333302 and -6.66666603, and a scale of 1 to -0.5 reads 0.5 and
    0.00000003.
  - Colors are mixed per channel and rounded down, so an alpha of `&HFF` at both
    ends reads `&HFE` on the second of three frames.
  - X, Y and Z are whole numbers that divide,
    `(from*frames_left + to*frames_gone)/frames`, dropping the fraction: 0 to
    -10 reads -3, -6, -10, and 0 to 3 reads 1, 2, 3.
- A relative color animation (`"C+"`) multiplies the start color by the key per
  channel and divides by 256, rounding down: `&H80808080` with `&H10101010`
  gives `&H08080808`, and `&HFF` with `&HFF` gives `&HFE`.
- A looping animation's first key starts again from the value the animation
  started on, not from the value of the last key. `"I"` with keys
  `-4,1,-4,2,-4,3` and loop 0, read every 2 frames, gives
  `0 0 1 1 2 2 3 0 1 1 2 2`: the value from before the animation comes back on
  each loop (TEST74, seen for `"I"`). The other targets loop the same way.
- `SPANIM` takes a copy of its array: changing the array afterwards leaves the
  animation unchanged (TEST74).
- An animation key that lasts 0 frames is `Illegal animation data`.
- An animation takes 32 keys at most: more, given as values, an array or DATA,
  is `Animation is too long`, error 40, without a number. A DATA count below 1
  is `Illegal animation data`. A target that is an array, as in
  `SPANIM 0,A,1,2`, or a name that is no target, such as `"X"`, is
  `Animation type mismatch (SPANIM:2)`, error 66, in `TANIM` and `ANIMDEF` too.
  An array among the keys, as in `SPANIM 0,"XY",A,1`, is `Illegal function call`
  with no number. `SPANIM` checks its sprite before anything else: one not set
  is `Sprite not set (SPANIM:1)`, regardless of what comes after it (TEST76).
- `SPANIM` keys for XY, Z, UV and I outside -32768 to 32767 are
  `Out of range (SPANIM)`, and `TANIM` and `ANIMDEF` go through the same code. R
  has no such limit (TEST60).
- `SPANIM id,n` and `TANIM id,n` with a number `ANIMDEF` never set do nothing.
- Setting the value that an animation changes stops it, and `SPCHK` no longer
  shows it: `SPOFS` during an `"XY"` animation, `SPCOLOR` during `"C"`, `SPCHR`
  during `"UV"` or `"I"`, `SPROT` during `"R"`, `SPSCALE` during `"S"` and
  `SPOFS` during `"Z"`.
- Spaces after an animation target's name are ignored (`"S "`).
- `TCHK` reports nothing for `"I"` and `"UV"`, which text screens ignore.
- `SPCHK` and `TCHK` give 0 while the animations are stopped.

## Input

### Buttons

- `DIALOG` with buttons closes when A is released, not when it is pressed. Once
  it has closed, `KEYBOARD` shows no Enter press: `KEYBOARD(40,2)` and
  `KEYBOARD(40,0)` stay 0 over the next 6 frames, whether Enter or A closed it.
- `DIALOG(text,8)`, which waits for A, closes when A is pressed and returns
  81928: A's bit with `#B_RANY` and `#B_ANY` (TEST64). Answered with A while
  L is held, it returns 16392: A and `#B_RANY`, without L and without
  `#B_ANY`, which L had already turned on. The any bits it gives are those
  that turn on with the press (TEST71).
- An A held before a `DIALOG` opens never answers it: an OK dialog with a
  time of 600 frames ran out with `RESULT` 0, A still held (TEST71).
- In a `DIALOG` with two buttons (No and Yes, Back and Next, Cancel and OK,
  Cancel and Execute), Left and Right on the D-pad or the left stick move the
  focus between the buttons. The focus starts on the right button, and A
  chooses the button with the focus. B chooses the left button wherever the
  focus is. (From a user who knows the console; not yet tested. Unknown: how
  far the stick must go, whether the focus wraps at the ends, and whether a
  held direction repeats.)
- The A that closes a `DIALOG` is gone for the program after it (TEST64):
  - With OK or Yes and No, `BUTTON` shows no release of it.
  - With a wait for A, `BUTTON(0)` shows it held, `BUTTON(0,-1,2)` no press,
    and the release shows when A is released. `INKEY$` gets no `CHR$(13)` for
    the press, but A, still held, repeats into it: 20 frames after the press
    and then every 3.
- `DIALOG` covers the whole screen, so nothing else shows while it is open, and
  a screenshot of it shows only the dialog.
- `DIALOG(text,buttons)` takes button masks 1 to `&H3FFFF`: 0 and -1 are
  `Out of range (DIALOG:2)`. Each form checks its arguments in order and names
  the one that is wrong: `DIALOG 0,0` is `Type mismatch (DIALOG:1)`, and a
  caption that is not a string `(DIALOG:3)`. `DIALOG(0,"S",1)` waits for
  buttons rather than asking for a name, since a name needs two strings, so it
  is `(DIALOG:1)` too. (Not yet seen on hardware.)
- `DIALOG` shows 45 characters of a longer caption, then an ellipsis. The file
  name form does the same, where the manual gives 39 (TEST76K).
- `BREPEAT` starts at 20,3 for buttons 0 to 7 and at 15,4 for 8 to 16. It
  does not touch what controllers type for `INKEY$`.
- `BREPEAT` repeats a held button at its start frame and then every interval
  plus one frames: 20,3 fires at 0, 20, 24, 28...
- `BREPEAT`'s negative values are `Out of range`: a start of -1 is
  `(BREPEAT:2)` and an interval of -1 `(BREPEAT:3)` (TEST71).
- `BREPEAT` takes a start of 0 as 1: `BREPEAT 5,0,3:BREPEAT 5 OUT S,I` gives 1
  and 3 (TEST62), and after `BREPEAT 0,0,0` it gives 1 and 0 (TEST71).
- `BUTTON(0)`'s bits include `#B_RANY`, `#B_LANY` and `#B_ANY` (bits 14 to 16).
  L and R turn on `#B_ANY` only: L held reads `&H10100` and R `&H10200`
  (TEST74K).
- `BUTTON`'s press and release conditions (2 and 3) give `#B_RANY`, `#B_LANY`
  and `#B_ANY` only on the frame that bit itself turns on or off. L held gives
  L and `#B_ANY` on its press. Pressing A while L is held gives `&H4008`, A
  and `#B_RANY`, and releasing A gives the same: `#B_ANY` stays on for L
  throughout. Releasing L then gives L and `#B_ANY` (TEST72).
- `BUTTON`'s repeat condition (1) repeats `#B_RANY`, `#B_LANY` and `#B_ANY`
  each by its own `BREPEAT`, counted from the frame that bit turned on. Another
  button pressed while one is held does not give them again (TEST74):
  - With L held, L and `#B_ANY` came at the press, 15 frames later and then
    every 5. A pressed meanwhile gave `&H4008`, A and `#B_RANY`. From then on
    A repeated on its own clock, `#B_RANY` on its own clock, and `#B_ANY` on
    L's clock.
  - With A alone, `BREPEAT 14,40,10` and `BREPEAT 16,25,6`, A repeated at 20
    and then every 4, `#B_ANY` at 25 and then every 7, and `#B_RANY` at 40 and
    then every 11.
  - Up held gives `#B_LANY` 15 frames after the press, though Up itself has
    not repeated yet.
- `BUTTON()` without arguments is `Wrong number of arguments`.
- `#B_X`, `#B_B`, `#B_Y`, `#B_A` are 0, 1, 2, 3. (TEST62)

### Keyboards and controllers typing

- A USB keyboard's keys press no controller button: a program that takes letters
  from `INKEY$` and delete from `BUTTON`'s Y gets the keyboard's A key as a
  letter, like any other letter key.
- Controllers type into the same queue as the keyboard, and the manual
  agrees on what it lists.
  - `INKEY$()` gives `CHR$(13)` for A, `CHR$(8)` for Y and the arrows for the
    +Control Pad, as the manual says.
  - The left stick past 0.3 gives Page Up and Down (`CHR$(17)`, `CHR$(18)`) and
    Home and End (`CHR$(2)`, `CHR$(3)`). The manual omits this.
  - L or R act as Ctrl. With left or right on the pad they give `CHR$(2)` and
    `CHR$(3)`, and with the stick up or down the same two. The manual omits
    this.
  - B types Copy, L or R with B Paste and with Y Delete Line. `INKEY$` skips
    these, and a line being typed acts on them.
  - `INKEY$(1)` drops anything that controllers typed.
  - The first assignment that holds gives the frame's one key. It comes at the
    press, 20 frames later and then every 3, for the pad and the stick alike
    (TEST64). The press types on the frame the button is pressed (TEST71K, on
    a Switch 2).
  - A second input pressed meanwhile types nothing while both are held.
    With Right on the +Control Pad repeating, Down added stopped it, and when
    Down was released, Right resumed 2 frames later and then every 3
    (TEST71K, on a Switch 2).
  - TEST67 on a Switch 1 and a Switch 2 saw the left stick right give
    `CHR$(3)` at the push, 20 frames later and then every 3, with `BUTTON` 0.
    L or R with the pad's right gave `CHR$(3)` too. L, R, and L and R held
    together type nothing, for at least 323 frames. With the stick moved in a
    circle meanwhile, only `CHR$(2)` came, the same key that Ctrl with the
    stick up or left gives.
  - TEST69K: with L and R held the whole time, the stick pushed down, then
    left, then right typed `CHR$(3)`, `CHR$(2)` and `CHR$(3)`, each repeating
    20 frames after its push.
- Ctrl+C on a USB keyboard is a break, the same as a short press of the +
  button. Without `ON BREAK GOTO` it stops the running program (TEST67: the
  step that asked for it ended there on both consoles, and the test continued
  with its next part). With one, the program jumps to the label, 20 frames
  after the press (TEST76K). Ctrl+C types nothing for `INKEY$`, and `KEYBOARD`
  sees Ctrl (usage 224) held but never C's press, only its release. Whether
  Ctrl+C also ends a subprogram is not known.

### USB keyboards

TEST68 on a Switch 2 (4.4.9) with a Korean-layout USB keyboard:

- `KEYBOARD` reads the Han/Eng key as usage 144 (LANG1) and Hanja as 145
  (LANG2). That keyboard also sent Right Alt (230) after Han/Eng and Right Ctrl
  (228) after Hanja. Some Korean keyboards send only Right Alt and Right Ctrl
  for these keys. Right Alt sets bit 8 of `KEYBOARD 4 OUT R,M`'s modifiers and
  Right Ctrl bit 4.
- SmileBASIC 4 keeps its own Caps Lock, on at start: `INKEY$` gives "A" for
  the A key and "a" for Shift+A. G, K and S give "G", "K" and "S".
- Han/Eng does not switch what `INKEY$` gives: G, K and S typed after it still
  give "G", "K" and "S", and the key itself types nothing.
- A held key repeats into `INKEY$` 19 frames after the press, then every 3
  frames: Shift+A held gave "a" on the frame it was pressed and again 19, 22,
  25... frames later. A controller's keys repeat after 20 frames instead.

TEST69K on a Switch 2 (4.4.9) with a 103-key Korean-layout USB keyboard:

- The first repeat came 19 or 20 frames after the press, for letters,
  Backspace, Left and Enter alike, then one every 3 frames. Backspace gives
  `CHR$(8)`, Left `CHR$(&H1D)` and Enter `CHR$(13)`. A repeat can come on the
  frame the key is released.
- A held letter's repeats follow Shift. With A held and repeating "A",
  pressing Shift held the repeat back for 19 frames, and then "a" came every 3
  frames until A was released.
- Another key pressed takes over the repeat. With A held, B typed "B" once,
  and A, still held after B was released, did not repeat again. Left pressed
  while Backspace repeated did the same.
- After Caps Lock, A types "a", while 1, Shift+1 and / still give "1", "!" and
  "/". Caps Lock sets no bit in `KEYBOARD 4 OUT R,M`'s modifiers.
- Right Ctrl+A types nothing. Keys pressed while L and R are held on the
  controller type nothing either, though `KEYBOARD` sees them.
- Right Alt+A typed U+E2EC, one of SmileBASIC 4's own characters.
- A keyboard key held when `DIALOG(text,8)` opens does not close it.
- SmileBASIC 4's software keyboard has a Caps key and no Shift. Its letters
  start upper case. Caps makes them lower case, turns `[` and `]` into `{` and
  `}`, and swaps some keys for others (Simple SAVE/LOAD, Copy/Cut,
  Insert/Delete Line, Undo/Redo). Its keys reach `INKEY$`, but `KEYBOARD` sees
  none of them. It takes touch, so it works only in handheld mode.

TEST70K on a docked Switch 2 (4.4.9) with a USB keyboard:

- The first repeat came 19 frames after the press, then one every 3 frames.
- Pressing or releasing Shift, and releasing Ctrl, start the count again: the
  next repeat comes 19 frames later.
- Each repeat types what the modifiers held at that moment give with the key,
  digits as well as letters: with 1 held, pressing Shift gave "!" 19 frames
  later, and with Shift+A held, releasing Shift gave "a".
- A held key types nothing while Left Ctrl is held, and types again 19 frames
  after Ctrl is released. Left Ctrl acts as Right Ctrl.
- L alone or R alone on the controller keeps keys from typing, the press
  included, the same as L and R together do, though `BUTTON` sees L or R.
- Left Alt+3 types nothing. Left Alt+A and Right Alt+A both type U+E2EC.
- A key held in DIRECT mode continues to repeat into the program that `RUN`
  starts. The Enter that typed `RUN 3`, still held, gave the program
  `CHR$(13)` in its frame 18, counted with `MAINCNT` from its first statement,
  and then every 3 frames until it was released. `KEYBOARD` saw Enter held
  from the program's frame 0.

TEST71K on a docked Switch 2 (4.4.9) with a USB keyboard:

- Programs, DIRECT mode and the editor share one Caps Lock. Caps Lock pressed
  in a program changes the case `INKEY$` gives. Turned on in DIRECT mode, it
  holds in the program `RUN` starts, and a press there carries over into the
  editor. `RUN` leaves it unchanged.
- Alt or the Windows key pressed for a moment while a key repeats holds the
  repeat back: the next repeat comes 18 or 19 frames after it is released.
  Pressing or releasing Shift while Backspace or Enter repeats does the same,
  18 to 20 frames.
- Another key takes over the repeat, and the first, still held, types nothing
  more once the second is released. Pressing Caps Lock stops a letter's repeat.
- Alt with a key types from a fixed table, the same for Left and Right Alt and
  with Shift or without:

  | Keys | With Alt (U+) |
  |---|---|
  | Q W E R T Y U I O P | E2F0 E2F1 E2F2 E2F3 E2E0 E2E1 E2E2 E2E3 E2E9 E21B |
  | A S D F G H J K L | E2EC E2ED E2EE E2EF E2F4 E205 E20E E206 E2E8 |
  | Z X C V B N M | E207 E208 E20A 2606 E210 E211 E214 |
  | 1 2 0 - = [ ; ' | E2EB E2EA E29C E29D E29E E2FF E203 E297 |
  | Keypad 0 1 2 3 4 | E295 E29A E290 E29B E292 |
  | Keypad 5 6 7 8 9 | E293 E294 E298 E291 E299 |
  | Keypad / * - + | E215 E216 E217 E28F |

  3 to 9, `]`, `\`, `,`, `.`, `/` and the backquote type nothing with Alt, and
  nor does the keypad's Enter. A held Alt+V repeats like other keys.
- With Left Ctrl, L or R held, Enter and Backspace type nothing either, while
  the Left arrow still types `CHR$(&H1D)`.
- ZL and ZR held do not keep keys from typing.
- `KEYBOARD` 57 reads the Caps Lock key itself. The Windows key is bit 1 of
  `KEYBOARD 4 OUT R,M`'s modifiers. `KEYBOARD`'s condition 1 gave 0 throughout.
- A key held as a program ends continues to repeat in DIRECT mode.

TEST72K on a docked Switch 2 (4.4.9) with a USB keyboard:

- Alone, Tab types `CHR$(9)`, Delete `CHR$(&H10)`, Home `CHR$(2)`, End
  `CHR$(3)` and Esc `CHR$(&H1B)`.
- With Left Ctrl, L or R held, Tab, Delete and Esc type nothing, the same as
  Enter and Backspace. Home and End still type, the same as the Left arrow.
- Left Shift (usage 225) and Right Shift (229) each give modifiers 2 in
  `KEYBOARD 4 OUT R,M`.

TEST74K on a docked Switch 2 (4.4.9) with a USB keyboard:

- Insert types `CHR$(&H1A)`, Page Up `CHR$(&H11)` and Page Down `CHR$(&H12)`.
  They type with Left Ctrl, L or R held too, the same as Home and End.

Expected but not yet seen on hardware:

- A key released while another repeats holds the repeat back, the same as
  Shift does, and the next repeat comes 19 frames later.
- On a keyboard layout where Right Alt is AltGr, Right Alt types the layout's
  character for the keys that have one.

TEST76K on a docked Switch 2 (4.4.9) with a USB mouse:

- `MOUSE OUT X,Y,W` starts at 0,0 with W 0. X and Y stay on the screen: the
  mouse moved past the edges of a 640x360 screen ended at 0,359 and 639,5.
- W is the wheel's total, 120 per notch, up positive: a notch up read 120, and
  three down from there 0, -120 and -240.
- `MBUTTON` gives 1 for the left button, 2 for the right and 4 for the middle.

Seen on a Switch 2 (4.4.9), handheld and docked alike: in a program that sets
`SYSPARAM "MOUSECURSOR",1` and reads `TOUCH`, the right stick moves the cursor,
and pressing it (R3) touches the screen.

The right stick as the mouse:

- The option "Set the mouse emulation status with the R Stick" is on by
  default. It works while no program runs (DIRECT mode, the editor, the menus)
  or while the main program or a `SUBRUN` one has `MOUSECURSOR` at 1: the same
  rule that shows the cursor. Handheld or docked and a USB mouse make no
  difference. A USB mouse moves the pointer regardless of the rule.
- Each frame the pointer moves by a step per axis, in pixels of the 1280x720
  display, by the stick's tilt m (the length of X and Y): over 0.85,
  `INT(ABS(A)*16+0.5)` for each axis A; over 0.7, `*8`; over 0.4, `*4`; over
  0.3, `*3`, so at most 1; else nothing. The stick does not speed up while
  held, no button changes the speed, and the mouse sensitivity setting is not
  used.
- An axis moves left or up only from -0.1 (left) or 0.1 (up); any reading
  short of that moves right or down. A stick held fully right that drifts
  0.03 to 0.1 either way moves down 1 pixel per frame.
- The position is a whole display pixel, 0 to 1280 and 0 to 720.
  `MOUSE OUT` maps it to the screen and keeps it within the screen's last
  pixel, so 1280 reads as 639 on a 640x360 screen.
- R3 is mouse button 1 while the rule holds, in place of the USB mouse's
  buttons, not added to them. `MBUTTON` and `TOUCH` see it as the left
  button; `BUTTON` still sees R3 and `STICK 1` the right stick.
- The mouse is touch 0 whenever no finger is on the screen, with or without
  the cursor shown.
- The cursor is a 32x32 picture (`MOUSE.png`) drawn with its top left corner
  at the pointer, 32x32 on the display regardless of the screen's size, over
  the screen and the dialogs. It fades out over 0.7 seconds after about 5
  seconds without moving, and comes back when the pointer moves or the cursor
  is shown again.

### `PUSHKEY` and the key buffers

- `PUSHKEY` takes a number too (`PUSHKEY 65622`, `#SOFTKEY_HIDDEN`). Codes from
  `&H10000` are keys that are not characters (TEST60):

  | Code | Key |
  |---|---|
  | `&H10000` to `&H10003` | Up, down, left and right |
  | `&H10004` | Backspace |
  | `&H10005` | Delete |
  | `&H10008` | Tab |
  | `&H10009` | Home |
  | `&H1000A` | End |
  | `&H10013` | Starts selecting (plain moves then select) |
  | `&H10015` | Copy |

- `PUSHKEY 8` is not Backspace but a character, and goes into a line as a cell
  of code 8; `PUSHKEY 9` the same (TEST60).
- `PUSHKEY` holds the program for a frame, without touching `VSYNC`'s count: ten
  of them take ten frames (TEST60). `INKEY$` reads the key right after it, and
  after a `VSYNC 1`, `VSYNC 2`, `WAIT 1` or `WAIT 2` in between, with a USB
  keyboard plugged in or not (TEST71, TEST71K).
- `PUSHKEY` of a string of 64 characters is `String is too long (PUSHKEY:1)`; 63
  is accepted (TEST60).
- `PUSHKEY &H10FFF` is accepted and `&H11001` is `Out of range` (TEST62). It
  names the code: `PUSHKEY 70000` is `Out of range (PUSHKEY:1)` (TEST76).
- `INKEY$` after `PUSHKEY &H10009` (HOME) gives one character of code 2, and
  after `PUSHKEY &H10000` (up) code `&H1E` (TEST62).
- Keys pass through two buffers of 128, each losing its oldest key when full.
  Typed and pushed keys wait in the first, and each frame takes up to 32 to the
  second, which `INKEY$` and the line being typed read.
  - Five `PUSHKEY`s of 63 in a row, with 315 keys, leave 128 to read: keys 32 to
    95, 124 to 155 and 187 to 218. Keys from 219 on come in the frames after,
    with the rest of the first buffer (TEST60).
  - In DIRECT mode, five `PUSHKEY`s of 60 lose keys the same way (TEST60).
- Typed keys wait in the buffer until something reads them:
  `WAIT 180:?INKEY$();INKEY$();INKEY$()` shows what was typed during the wait.
- `INPUT` and `LINPUT` drop every key waiting when they start, keys from
  `PUSHKEY` included: `PUSHKEY "7":INPUT A$` waits with an empty line.
- DIRECT mode drops waiting keys as it starts, both after a typed line and after
  a program: `PUSHKEY "?1"+CHR$(13)+"?2"+CHR$(13)` typed in DIRECT mode runs
  neither (TEST62). From a subprogram, typing into a DIRECT mode line already
  there, they arrive.
- `PUSHKEY` of keys without Enter is dropped in DIRECT mode too, the same as
  with Enter: a line typed after `PUSHKEY "?7:"` runs unchanged. (TEST63)
- `INKEY$` gives back what `PUSHKEY` pushed of Hangul syllables, the jamo
  U+3131, U+E800 and surrogate pairs unchanged, one UTF-16 unit per call: the
  pair U+D83D U+DE00 takes two calls (TEST68).

### `INPUT`, `LINPUT` and the line being typed

- `INPUT`'s question mark is followed by a space; with a comma after the guide
  there is neither.
- `INPUT` ignores extra values typed. After `?Redo from start`, the line is
  typed again on the next line, without the guide.
- `INPUT` and `LINPUT` take any string expression as the guide, a label among
  them (`LINPUT@G;A$` shows `@G`). A string variable is a guide only with `;`.
- `INPUT A,B` given `1,` takes A=1, B=0 immediately. Given `1,2,3` it ignores
  the 3, not looking at it. (TEST63)
- `INPUT VAR("A$")` and `N$="A$":INPUT VAR(N$)` both take a string. (TEST63)
- With no USB keyboard plugged in, `INPUT` does not open the software keyboard
  by itself (TEST74).
- `INPUT` selects nothing: Shift with Left or Right moves the cursor, and
  Backspace and Delete take one character (TEST56). `LINPUT` runs with the same
  setting in SmileBASIC 4. DIRECT mode's line does select, cut, copy and
  paste.
- `INPUT A$`, ABCD, Shift+Left twice, X gives `ABXCD`: on `INPUT`'s line Shift
  only moves.
- Tab on a line types one space (TEST60).
- A selection stays after Enter (TEST60).
- Copy on a line ends the selection and keeps the text, so what is typed next
  goes in at the cursor (TEST60).
- A line holds 256 characters regardless of the console's size; a character
  typed on a full one is dropped (TEST60).
- A line typed in DIRECT mode or for `INPUT` or `LINPUT` holds 256 characters
  of any kind, regardless of the console's size, and a paste that would not
  fit pastes nothing. Expected but not yet seen on hardware: a further
  character is dropped over a selection too, which then stays, and a paste
  counts a selection as still there.
- Pasted text with line ends goes into the single line of DIRECT mode, where
  each line end shows as a line break, and Enter runs it all:
  `"?11"+CHR$(10)+"?22"` pasted from `CLIPBOARD` with the Paste key shows `?11`
  and `?22` on two rows, and Enter prints 11 and 22 (TEST71).
- Enter continues from the row under the cursor, wherever on the line that is.
  A line that exactly fills its row leaves an empty row before what comes next,
  because the cursor is already on the next row. With the cursor on the first
  row of a longer line, what comes next prints over the second (TEST60).
- Typing does not stop at the end of a row. The line wraps at the console's
  width and continues over as many rows as it takes, the console scrolling as
  it goes, and the last row stays empty (TEST60 part A, on a real Switch with a
  USB keyboard: `ACLS:XSCREEN 320,180,1,2:TSCREEN 4,8,26,12:LOCATE 0,9`, then
  about 60 plain letters).
- A line keeps rows free under its cursor according to the hint bar: at 1280x720
  with 8x8 characters, typing from row 85 leaves the cursor on row 86; at
  640x360 from row 40, on row 42 (TEST60).
- Enter ending a line scrolls the console so that the cursor stays above its
  last row; `INPUT`'s and `LINPUT`'s guides, errors and `OK` do the same. What a
  program prints can still reach the last row (TEST71).
- The hint bar at the bottom covers more rows on a screen whose rows are 16
  display pixels high or less (of the 720): 2 rows stay free at 360 rows of
  pixels, 3 at 720 with 8x8 characters. A `LINPUT` keeps them free from its
  start, with a guide or without (TEST71).
- `INSPECT` scrolls the same as Enter does while it prints, keeping the cursor
  above the last row, which has been seen (`TEST19`, on the usual screen). The
  rest of this scrolling comes from the code.
- Enter on an empty line of DIRECT mode prints `OK` (TEST60), and leaves the
  empty line above it.
- In DIRECT mode the controller types the keys that `INKEY$` reads from it
  (TEST64, by hand). A is Enter, Y deletes like Backspace, and the +Control
  Pad moves like the arrow keys. On an empty line Y and B do nothing. A USB
  keyboard plugged in or out, or the software keyboard shown, changes none of
  this.
- Typing over a selection in overwrite mode writes the character and advances
  the cursor first, then deletes the selection: one made rightwards takes the
  character with it, and one made leftwards keeps it (TEST71).
- Copying a selection in DIRECT mode drops the selection; cutting too. (Not yet
  seen on hardware.)

### The line's cursor

- `INPUT`'s cursor is a bar 2 pixels wide and 8 tall at the left of the cursor's
  cell, opaque white regardless of `COLOR`. `COLOR &H80FFFF00` over a character
  changes only the bar's two columns, and the rest of the cell is left
  unchanged. It alternates with nothing (TEST50).
- Characters typed at `INPUT` and then deleted leave their cells empty: anything
  the console showed there before does not come back (TEST50).
- The per-cell inverting flag (color times 1 minus the glyph's alpha) is not
  what draws `INPUT`'s cursor (TEST50).
- The cursor blinks by a count kept with the frame count `MAINCNT` reads: 20
  frames on, 20 off. TEST56's two shots, off at frame 72 and on at 299 of the
  line, fit that with the count 1 to 3 frames behind `MAINCNT`, and 15/15 only
  at 3 ahead, which is weak support; the code says 20.
- Insert toggles overwriting on `INPUT`'s line, and the cursor is then a block
  over the whole cell: 24 wide at scale 3 (TEST56).
- DIRECT mode's cursor bar is a quarter of the cell wide and the cell's
  height, on 16- and 24-pixel cells too (TEST71P).
- The cursor has the same scale as the text, `TSCALE` and `LMATRIX` included
  (TEST56).
- The cursor is drawn right after its own screen's cells, under a sprite in
  front, and it replaces what its layer holds, alpha too. Because a layer's
  alpha is what shows through, on a composed layer it adds its white to the
  frame rather than covering it: `&H203080` under `LAYER 2,1,&HFFC0C0C0`
  shows `&HE0F0FF` (TEST56).

### Controllers, vibration and the IR camera

- `CONTROLLER(0)` read 2, `CONTROLLER(1)` 1 and `CONTROLLER(2)` 0.
  `CONTROLLER 2 OUT A,B,C` gives 0 for all three: no colors on a controller not
  connected. (TEST63)
- With a Pro Controller and the Joy-Cons off the console, `CONTROLLER(0)` and
  `CONTROLLER(1)` read 2. `CONTROLLER 0 OUT` gives 2 with colors 0 and 0, and
  `CONTROLLER 1 OUT` 2 with `&HFF232323` and `&HFFA0A0A0`, the Pro Controller's
  own colors. (TEST64) The last two colors of
  `CONTROLLER n OUT T,C1,C2,C3,C4` are `&HFF000000` on a Pro-style controller
  (type 2) (TEST71).
- `STICK 0` takes X and Y separately: the value furthest from 0 among the
  controllers, the earlier controller's value on a tie. X and Y can come from
  different controllers.
- `IRSTART` without `XCTRLSTYLE`'s IR flag is `IR Camera is not available`.
- `IRSTART` takes, after the controller and the mode, nothing or as many
  settings as its mode has: three for moment (mode 2), four for image transfer
  (1), six for clustering (0). Another mode's count is
  `Wrong number of arguments for this IR Camera mode`, error 108, without a
  number, and 3, 4, 7 or over 8 arguments in all error 52. The settings are 0 to
  3, 1 to 16 and 7 to 600, then 0 to 4 for image transfer, and 0 to 76800,
  0 to 76800 and 0 to 255 for clustering, each named when out of range. (Not yet
  seen on hardware.)
- `TCPIANO` without Toy-Con set up is `Illegal control style`. It names the
  controller, as `(TCPIANO:1)`, and so do the other nine Toy-Con commands,
  `TCHOUSE` to `TCVEHICLE`. All but `TCVISOR` check this before the `OUT`
  count: `TCHOUSE 1 OUT F` is error 107, `TCVISOR 1 OUT F` error 53 (TEST76).
- `XCTRLSTYLE` takes two arguments, or four or five, or six or seven for
  Toy-Con on two controllers, where the first must be 2
  (`Out of range (XCTRLSTYLE:1)`) and the style 100 (`XCTRLSTYLE:2`). Toy-Con
  types are 0 to 10, each named after its argument, and a pair that does not
  go together is `Illegal Toy-Con type`, error 110: `XCTRLSTYLE 1,100,0,0` is
  `(XCTRLSTYLE:4)`. For two controllers, the first takes no type 3, 4 or 7 to
  10, contrary to the manual: `XCTRLSTYLE 2,100,10,10,6,5` is
  `Illegal Toy-Con type (XCTRLSTYLE:4)`. The IR flag with a style other than 1
  is `Illegal control style for using IR Camera (XCTRLSTYLE:2)`, error 109:
  `XCTRLSTYLE 1,0,0,1` (TEST76K).
- `IRSTATE` starts at 0.
- `IRREAD` in mode 1 is `Illegal IR Camera mode` (TEST71).
- `IRREAD`'s array has 48 rows of 8 after clustering mode, the same as after
  moment mode (TEST71).
- `IRREAD` on a camera never started is `IR Camera is not available (IRREAD)`.
  (TEST62)
- `IRREAD` after `IRSTOP` gives no entries, with no error.
- Vibration patterns have their own lengths, from 3 frames (57) to 115 (23);
  34 to 43, 47 and 48 last past two seconds, 40 past five. The emulator keeps
  the measured table.
- `VIBRATE()` reads back what was asked for, also where nothing vibrates. After
  `VIBRATE 1,40` it is 2621480 (40 on the right times 65536, plus 40 on the
  left) and `VIBRATE(1,0)` 40; with frequencies it is -1, still a second later.
  `VIBRATE` alone clears it.
- `VIBRATE`'s form with frequencies vibrates with a low frequency of 0, a low
  amplitude of 0 or a high frequency of 0 (TEST71, felt by hand). With both
  amplitudes 0 it does not: `VIBRATE 1,160,0,320,0` stopped a vibration
  `VIBRATE 1,160,100,320,100` had started, and after `VIBRATE 1,0` it started
  none (TEST72, felt by hand). `VIBRATE 1,0` stops such a vibration
  immediately too, 20 or 60 frames after it started (TEST74, felt by hand).

## Screens and text

### `ACLS`, `XSCREEN` and `TSCREEN`

- `ACLS` also resets `SPPAGE`, `SPLAYER`, `GSAMPLE`, `TOFS`, `COLOR`, `ATTR` and
  `TCOLOR`, which the manual's equivalent program omits.
- `XSCREEN` clears the console, puts its cursor at 0,0, and resizes `#GSPRITE`
  to the new screen.
- `XSCREEN` moves `#GSPRITE` to the layer `SPLAYER` gives new sprites,
  regardless of the layer it was on.
- `XSCREEN` and `ACLS` put back the rest of `#GSPRITE` too: its offset, scale,
  rotation, color, page, home point, UV and `SPHIDE` (TEST71).
- `XSCREEN` stops the text screens' `TANIM` animations (`TCHK` gives 0) and
  keeps their `THOME`, `TROT`, `TSCALE`, `TOFS` and `TCOLOR`.
- `XSCREEN OUT` gives the aspect ratio in single precision: 1.66666663 for
  400x240.
- `XSCREEN` checks each argument's range first and names it: width 128 to 1280
  (`XSCREEN:1`), height 128 to 720 (`:2`), sample 1 to 10 (`:3`), mode 0 to 2
  (`:4`). A width or height that 4 does not divide is `Use multiples of 4`,
  error 69, after that, as `(XSCREEN:1)` or `(XSCREEN:2)`. A width or height
  that the sample takes past 1280 or 720 is `Out of range (XSCREEN:3)`, and
  `(XSCREEN:4)` when all five arguments are given (TEST76).
- `TSCREEN` clears the screen. Without a character size it uses the font type.
- A width or height given to `TSCREEN` is 1 to 32768: 0 is `Out of range`,
  though the manual says 0 fills the screen, the same as omitting both. A
  font type other than 8 to 64 in steps of 8 is `Illegal font type`, error 58,
  named as `(TSCREEN:1)` or `(TSCREEN:2)`, and checked after the character
  size, width and height (TEST76).
- `TSCREEN` also stops the screen's animations and undoes `THOME`, `TROT`,
  `TSCALE`, `TCOLOR` and `TBLEND`. `TOFS` goes back to 0,0 but keeps its depth.
  `TLAYER`, `TPAGE` and `TVAR` stay (TEST47), and so does `TFUNC` (TEST48).
  The console's `TSCALE` goes back to 1,1 too (TEST69).
- `TARRAY`'s array takes the new size after `TSCREEN` and continues to follow
  the screen.

### Cells

- `CHR$(10)` starts a new line; tab stops are every 4 columns.
- `#TUSRCHR` is 59392. (TEST62)
- `TPUT` off the screen is `Out of range`.
- `TPUT` of a string puts only its first character: `TPUT 4,0,1,"XYZ"` leaves
  the cells after the X empty (TEST69).
- `CHKCHR` off the screen gives 0.
- `LOCATE` takes the column just past the right edge (`LOCATE 50,0` on the
  50-column console).
- `LOCATE 3E10,0` is `Overflow (LOCATE:1)`. (TEST62)
- `LOCATE 0` with one argument is `Wrong number of arguments (LOCATE)`, error
  52 (TEST68).
- Text screens draw the codes from U+E800 up from their own `TPAGE` at font
  types 8, 16 and 32. At font type 32 a text screen shows only user-defined
  characters (TEST68, by its picture).
- `SCROLL 0,0,2147483647` and `SCROLL 0,2147483647,0` clear the screen, the
  same as any shift of a whole screen or more (TEST51).
- `TFILL` clips its rectangle to the screen before the string starts, and the
  string continues across rows at the clipped width. On a 6x3 screen
  `TFILL 0,MIN,MIN,MAX,MAX,"ABC"` gives `ABCABC` on every row (TEST51), and
  `TFILL 0,-1,-1,4,1,"ABCDEFG"` gives `ABCDE` then `FGABC` and an empty third
  row (TEST52).
- `TCOPY` takes rectangles of any size, down to the largest coordinates, and
  copies only what is on the screens (TEST51).
- `TSCREEN` of 65536 cells a side is `Out of range` (TEST51).
- Bit 23 of a value for `TLOAD` or `TUPDATE` (attribute bit 7) does nothing on
  text screens 0 to 3 (TEST56). A plain glyph shows, under `TCOLOR` and `TBLEND`
  too. A space and an empty cell show nothing, and `TCOPY` carries the bit the
  same way.
- `TLOAD`, `TUPDATE` and `TCOPY` move cells raw: all 16 attribute bits come back
  from `TSAVE`, `TARRAY` and `CHKCHR`'s `OUT`, and on text screens 0 to 3 the
  bits other than bit 7 have no effect either (TEST56).
- Attribute bit 7 on the console inverts a cell, because only the console has
  that per-cell flag. TEST64's picture agrees with all of this list but the
  selection, for white, orange and both at alpha `&H80`.
  - The cell's color is multiplied by 1 minus the glyph's alpha. Then, the
    same as for any text, it is multiplied by `TCOLOR` and by the resulting
    alpha, so the light is left with the square of it.
  - A space is a block, and so is an empty cell.
  - DIRECT mode's selection sets the bit, and Enter does not draw the line
    again, so it should stay.
  - A console cell given bit 23 through `TLOAD` shows inverted; `CHKCHR`
    gives its attribute as 128 and `TSAVE` keeps the bit (TEST69).
- U+25A0 is in SmileBASIC 4's font: `FONTINFO` places it at 512,1072 for 16
  pixels and 256,24 for 8 (TEST69).
- A cell `TSCREEN` or `CLS` clears has code 0 and the screen's current `COLOR`
  and `ATTR`, white and 0 to start with, not an all-zero cell. A program can
  therefore write only the character codes through `TARRAY`, the even elements,
  and they show after `TUPDATE`: SmileBOY draws its background that way. A new
  `TSCREEN` cell reads `&HFFFFFFFF` and 0, after `COLOR 1,#C_RED:CLS 1`
  `&HFFFF0000`, and after `ATTR 1,8:CLS 1` attribute `&H80000`. `TSCREEN` keeps
  a `COLOR` and `ATTR` set before it. (TEST64)
- A cell that `SCROLL` or the console's scrolling uncovers is all zero, and so
  is a cell off the screen for `TSAVE`: a code written there through `TARRAY`
  alone has color 0 and does not show. (TEST64 for `SCROLL`, with a picture.)
- `TLOAD`'s width and height have to be 1 or more, and `GLOAD`'s 0 or more,
  matching what TEST62 saw for `TSAVE` and `GSAVE`. Each names its argument: a
  width of 0 is `Out of range (TLOAD:4)` and a height of 0 `(TLOAD:5)`, while
  1x1 loads; `GLOAD` takes a width or height of 0, and a width of -1 is
  `Out of range (GLOAD:3)` (TEST71).
- `TSAVE`'s width and height have to be 1 or more: 10x0 and 10x-1 are
  `Out of range (TSAVE:5)`, and 5000x1 is accepted, reaching off the screen.
  `GSAVE`'s -1 is `Out of range (GSAVE:4)`. (TEST62) A width of 0 is
  `(TSAVE:4)`. For `GSAVE`, a width of -1 is `(GSAVE:4)` and a height of -1
  `(GSAVE:5)`, while a width of 0 is no error (TEST72).

### `TROT`, `TOFS` and `TCOORD`

- `TROT` keeps the angle in single precision: `TROT 0,0.1` reads back
  0.100000001490116 (TEST56).
- `TOFS` accepts X and Y omitted (`TOFS 0,,,10`).
- `TCOORD` mode 0 drops the fractions of what it is given, 3.9,2.9 with
  `TOFS 0,100,50` giving 103,52. Mode 2 gives the home point for an axis scaled
  to 0. (TEST62)

## Sprites

### Sprite state and links

- Using a sprite before `SPSET` is `Sprite not set`, except for its variables:
  `SPVAR` works on a sprite that was never set. `SPSET`, `SPCLR`, `ACLS` and
  `RUN` clear them.
- A flip and a quarter turn together flip the image first, then turn it. Text
  characters (`ATTR`) work the same way.
- A linked child's offset turns and scales with its parent regardless of the
  link flags. The rotation and scale flags only decide whether the child's own
  image turns and scales too.
- When a parent is cleared, children without link bit 0 stay where they showed,
  Z included.
- `SPSET` drops the sprite's `SPFUNC`, also on a sprite that was set already.
- `SPSET` with an image takes attributes 0 to 31 in every form, the same as
  `SPDEF`: `SPSET 0,0,0,16,16,32` is `Out of range (SPSET:6)`, and
  `SPSET(0,0,16,16,32)` `(SPSET:5)` (TEST76).
- `SPSET()` checks its template number, width, height and attributes before it
  looks for a free sprite, so a wrong one is an error even when no sprite in
  the range is free: `Q=SPSET(0,0,0,0,-1,16,1,0)` with sprite 0 in use is
  `Out of range (SPSET:5)`, not -1 (TEST76).
- `SPDEF` checks what it is given: U and V 0 or more, width and height 0 to
  2048, home point -32768 to 32767 and attributes 0 to 31, each named after its
  argument. Copying from another template (nine arguments) takes U and V up to
  2048 too, and the image must end within 2048. An array whose length 7 does
  not divide is `Illegal array size (SPDEF:1)`, error 117 (TEST76).
- `SPDEF` from an array or DATA sets templates up to 8191 and skips the rest
  without an error. It checks each template's width, height, home point and
  attributes the same as `SPDEF` with numbers does, but with no number: the
  first one out of range stops it as `Out of range (SPDEF)`, with the templates
  before it set. A DATA count over 8192 is `Out of range` too, while a
  negative one sets nothing (TEST76).
- `SPLINK` of a sprite to itself is `Illegal function call (SPLINK:2)`, and to
  a sprite linked below it `Can't link to decendent sprite (SPLINK:2)`, error
  113, spelled that way. `SPOFS`, `SPCOL`, `SPCOLOR`, `SPHOME`, `SPROT` and
  `SPSCALE` check the sprite's number before anything else (TEST76).
- `SPHIDE id,flag` shows the sprite on a flag of 0 and hides it on any other
  value, 2 and -1 among them. `SPHIDE id` with no flag hides it (TEST71).
- Page -1 is a separate page, not a graphic page, and `SPPAGE id,-1` reads back
  -1. Sprites on page -1 show solid white regardless of their UV, and `SPCOLOR`
  tints them, its alpha included. User-defined characters on a text screen
  after `TPAGE 0,-1` show solid white too (TEST71P, by its picture).
- A sprite whose `SPSCALE` or `SPROT` is NaN shows nothing, and so does a layer
  whose `LMATRIX` holds a NaN (TEST71P).

### Collision

- Hidden sprites do not collide, including children hidden through their parent
  (link bit 0).
- Sprites without `SPCOL` never collide, and rectangles that only touch do not
  either.
- `SPCOL` without a rectangle starts it at minus the home point.
- `SPCOL` takes 1, 2, 3, 5, 6 or 7 arguments. 4 or 8 are
  `Wrong number of arguments`, error 52: `SPCOL 0,0,0,16` (TEST76).
- `SPHITSP` with a reversed range searches it from the lower number.
- `SPHITSP` of a sprite that is not set is -1, and `SPHITSP(id,other)` with
  `other` not set is 0; neither is an error.

### `SPFUNC` and `CALL SPRITE`

- `CALL SPRITE` goes through the sprites in number order as it runs: a callback
  that clears a later sprite skips it, and one that sets a later sprite's
  `SPFUNC` gets it called. Afterwards `CALLIDX()` is 4096 (5 after `CALL TEXT`).
- A callback `SPFUNC` names is a DEF or label of the slot `SPFUNC` ran in, or a
  `COMMON DEF`; `CALL SPRITE` finds it from any slot.
- `SPFUNC` takes a sprite that is not set.
- `SPFUNC` and `TFUNC` look the name up as they run, not when the callback
  is called. A name that the program or SmileBASIC knows but that is not a DEF,
  such as `"SPSET"`, is `Function must be user defined (SPFUNC:2)`, error 114.
  Any other name must be a label: `"NOPE"` and `"@"` are
  `Illegal label string`, error 35, without a number, and a label the program
  does not have is `Undefined label (SPFUNC:2)` (TEST76).

### `SPCHR`

- `SPCHR` reads its arguments by how many there are (TEST45), not as the
  manual's `SPCHR id,[U],[V],[W],[H],[attr]`. Empty U and V always keep the
  sprite's own values.
  - `SPCHR id,U,V` sets W and H to 16 and the attributes to 0.
  - `SPCHR id,U,V,attr`: the fourth argument is the attributes (W and H become
    16), so `SPCHR 1,,,40` is Out of range on argument 4.
  - W and H are the size as shown (TEST45, TEST46). While the sprite is turned
    90 or 270 degrees (attribute bit 0), an empty W takes the image's H and an
    empty H its W.
  - `SPCHR id,U,V,W,H` sets the attributes to 0 and stores W and H as given.
  - `SPCHR id,U,V,W,H,attr`: when the resulting attributes turn the sprite 90 or
    270 degrees, the image is stored as H by W, so `SPCHR 1,8,8,40,50,5` reads
    back W 50, H 40. Empty slots keep the sprite's own values, so
    `SPCHR id,,,,,#A_REVH` only turns a sprite around (a published game relies
    on it).
- `SPCHR`'s attribute goes up to 31: 32 is `Out of range (SPCHR:6)` (TEST60).

## Graphics and layers

### Drawing on pages

- `GCLIP` is shared by all pages.
- Each of `GCLIP`'s four coordinates is
  0 to 2047, or `Out of range` named after it (`GCLIP:1` to `GCLIP:4`). (Not
  yet seen on hardware.)
- `GCIRCLE` with equal start and end angles draws nothing; the sector flag draws
  only the outline.
- Drawing rules read back with `GPGET` (TEST47, all 23 parts matched):
  - `GCIRCLE` draws the pixels of the midpoint circle: in each row of the first
    octant, the largest x with x² - x + y² < r². Radii of 1000 and 70000 far off
    the page still draw what reaches it.
  - `GCIRCLE`'s arc runs between the circle pixels nearest its two angles, at
    `round(r*cos a), round(r*sin a)`, both included: 30 to 120 degrees takes the
    pixel (7,4) at 29.7 degrees. Angles past 360 or negative are taken modulo
    360. An arc of more than half a turn whose two end pixels are the same,
    such as 0 to 359 or 10 to 9, is the whole circle (TEST48). A sector's two
    sides are `GLINE`s from the center to those pixels.
  - `GLINE` takes the same pixels regardless of which end comes first, in all
    four directions and with X and Y swapped. Walked along the longer axis from
    the end with the smaller coordinate on it, `major` steps long and `minor`
    across, the offset across at step t is `(2*t*minor + k) / (2*major)`
    rounded down.
    - When that end is on the page, `k = (major + 2*minor) mod (2*major)`: the
      line rounded (halves up) one step further on, `round((t+1)*minor/major)`,
      less `round(minor/major)` so that it starts at its end. `GLINE 0,0,7,3`
      steps across at X 1, 3 and 5 (`01122333`), `GLINE 0,0,6,1` at 2, and
      `GLINE 0,0,4,2` at 2 and 4.
    - When that end is outside `GCLIP`, the page if none is set, `k = major`:
      the line itself rounded, halves up, counted from its real end (TEST56, ten
      lines, in either direction). `GLINE -1,5,30,10` steps at X 3, 9, 15, 21 and
      27, `GLINE -1000,0,1040,51` gives rows 25 for X 0-19 and 26 from 20, while
      `GLINE 0,25,40,26` steps at X 19 (TEST47, TEST48).
  - `GTRI` draws each pixel once (with `#G_ALPHA`, also where edges meet), one
    row at a time from the leftmost to the rightmost pixel an edge reaches in
    the row, regardless of the order of the corners (TEST47, TEST48, TEST56).
    - Each edge goes down from its top end with a sum that starts at `-wide` if
      it is wider than tall, or `tall` if not. Each row adds `2*wide`, and the
      edge takes a step across for each `2*tall`, at most one past its bottom
      end.
    - A row has the columns from where the last row ended to the one before
      where this one ends. Level edges add no columns.
    - Corners off the page are not clamped.
    - For example, `GTRI 0,0,1,2,10,1` fills rows 0-4, 1-10 and 1-6.
    - A triangle whose corners lie on one line is that line, drawn once.
  - `GBOX` draws the top and bottom rows, then the sides from the top row to the
    row above the bottom: with `#G_ALPHA` the two top corners are blended twice
    and the bottom ones once, regardless of which corners are given first
    (TEST48). A box one pixel wide or high draws each pixel once, except the
    top of a one-pixel-wide box. `GBOX 0,Y,5,Y` on row -2147483648 draws
    nothing on the page (TEST51).
  - `#G_ALPHA2` (mode 3): alpha `a = sa + da - floor(sa*da/255)`; each channel
    `(s*floor(sa*255/a) + d*floor(da*(255-sa)/a)) / 255` rounded down. Over an
    opaque pixel that is mode 2's result. Where both alphas are 0, the page
    pixel stays unchanged, color included: `GPUTCHR` and `GCOPY`, with a color
    or without, leave `&H00FF4020` under the source's clear pixels (TEST74).
  - `GPAINT` fills 4-connected areas.
  - `GCOPY`'s color code multiplies and rounds down, and an overlapping copy
    within a page reads the source first.
  - `GPUTCHRP` drawing scales its spacing with the characters.
- `GARRAY(page)` is a 2048x2048 Int array indexed `[y,x]`, the same one each
  time. A write to it is seen immediately by `GPGET`, also where `GPSET` drew
  next to it.
- `GUPDATE` with a coordinate of 2048 is `Out of range`.
- `TSAVE`, `GSAVE` and `GLOAD` accept rectangles reaching off the screen or
  page; the part outside reads as empty.
- `GSAVE` and `GLOAD` take rectangles of any size memory allows: 8192x1, 1x4097
  and 2048x2049 work, `GSAVE` giving 0 for the part off the page and `GLOAD`
  dropping it. A width or height of 0 is no error: `GSAVE` then gives no
  elements, and `GLOAD` draws nothing (TEST47, TEST48).
- `GLOAD` palette indexes past the palette are no error and leave the page's
  pixel unchanged, with mode 0 too (TEST47).
- `GSAVE` into a real array gives the colors unsigned: opaque red is 4294901760,
  where an integer array has -65536.

### `GPUTCHR` and `GPUTCHRP`

- `GPUTCHR` draws each character's whole cell, so with mode 0 it clears the page
  under the character's transparent pixels.
- `GPUTCHR` without a font type uses the console's font type. After
  `TSCREEN 24` it draws 24-pixel characters from GRP5, glyph i (font.map's
  number, as for 8 and 16) at column i%64 and row i/64 of 24-pixel cells, which
  is how `#GUIDE/DEFFONT24` is laid out.
- `GPUTCHR` given a font type other than 8 or 16 is `Illegal font type`.
- `PRINT` on a console of such a font type shows only user-defined characters.
- The 16-pixel font page has partial alpha at glyph edges (48 pixels of A, and
  of あ); the 8-pixel page has none. `GPUTCHR` draws the same values (TEST69).
- `GPUTCHRP` advances after each character by its width plus the spacing (1 when
  omitted). The widths come from a fixed table, not from GRP5: filling A's
  cells on GRP5 and clearing I's changes none of them (TEST69).
- For most glyphs the width runs from one empty column before what is drawn of
  the glyph (none if the glyph starts at its first column) to its last drawn
  column. The space is a quarter of the font size wide.
- font.map holds each code's `GPUTCHRP` width for both sizes in its u32 entry:
  bits 15-18 at 8 pixels and bits 23-27 at 16. The 8-pixel widths are not the
  16-pixel ones halved. Widths at 16 and 8 (TEST69), all as these bits give
  them: U+0000 14/7, U+00B2 7/4, U+2026 13/6, U+203B 14/7, U+21D2 14/7,
  U+30B8 16/8, U+E200 14/7, U+E25B 1/1, U+E2A0 16/8, U+3000 14/7, and A 13/6.
- The same entry holds the first column of what `GPUTCHRP` counts as the
  character: bits 12-14 at 8 pixels and bits 19-22 at 16 (I: 1 and 3).
  `GPUTCHRP` draws the character's whole cell with that column on the pen, so
  the columns before it land left of the pen (TEST70).
- So with mode 0 `GPUTCHRP` clears the whole cell, its columns left of the pen
  and right of the width too. Where a character's first column is more than the
  spacing, its cell clears the last columns of the character before: in "AI" at
  16, I's cell starts 3 columns left of its pen and clears A's columns 11 and
  12. "Ii" at 16 and "AI" at 8 each clear the columns this gives too (TEST70).
- With mode 1 and a color whose alpha is 0, `GPUTCHRP` (TEST70) and `GPUTCHR`
  draw nothing. With a color of alpha 1, mode 1 skips only the glyph's own
  clear pixels and writes the rest with the alpha the color gives them, so
  the glyph's soft edges are written with alpha 0 (TEST71).
- Characters not in the font draw the missing glyph, and `GPUTCHRP` gives them
  16 (U+25A2, U+AC00; TEST69). U+25A0 is in the font, 14 wide.
- A user-defined character takes 16 at 16 and 8 at 8, also when its cell is
  empty, which draws nothing. After `TPAGE 4,-1`, `GPUTCHR` of a user-defined
  character leaves a cleared page clear, and `GPUTCHRP` still gives it 16
  (TEST69). Over a filled page it changes nothing either, in mode 0 or mode 1
  (TEST70).
- `GPUTCHR` of a user-defined character from a graphic page multiplies the
  page's colors by the color, under `GSAMPLE` 0 and 1. Where its cell reaches
  past the page's edge, `GSAMPLE` 1 wraps around to the other side, and
  `GSAMPLE` 0 draws nothing there, leaving the target's pixels unchanged, in
  mode 0 too (TEST71).
- The width that the function form of `GPUTCHRP` returns scales the spacing
  with the characters: `GPUTCHRP("AB",16,2,3)` is 2*(13+11+3).

### Colors and blending

- Blending rounds to the nearest, not down: a sprite at &H80 alpha over 60 gives
  (255x128 + 60x127)/255 = 157.9, shown as 158. With that, the emulator matched
  lossless screenshots of mode 0 and mode 1 layers pixel for pixel.
- Colors are computed as follows, confirmed by TEST49 (about 4,500 patches
  read from lossless screenshots, every one within 1):
  - Each 8-bit value reaches the GPU as k/255, and the computation is in
    floating point with no 8-bit rounding inside a draw. A draw is rounded
    once, to the nearest, when it is stored: an element into its layer (or the
    screen, for mode 0), a composed layer into the screen, and the screen to
    the display.
  - What is drawn holds premultiplied color, and its alpha is how much of what
    is behind still shows through. A composed layer starts clear: black,
    letting everything through.
  - An element with image pixel `t` and color `c` (SPCOLOR; for text, the cell's
    color times TCOLOR, multiplied in unrounded) has alpha `a = t.a*c.a` and
    light `t*c*a`.
    - A normal element gives `light + d*(1-a)` and leaves `1-a` of what showed
      through.
    - An additive one (`#A_ADD`, `TBLEND` 1) gives `d + light`, held to 1, and
      changes no alpha.
    - `&H808080` tinted `&H7F7F7F` is 63.75, so `&H40`, and `&H80FFFFFF` over 60
      is 157.9, so 158.
  - A composed layer (LAYER modes 1-4) with premultiplied color `p`, what shows
    through it `T`, and the layer color `L` gives the light `P = p*L*L.a` and
    the coverage `A = L.a*(1-T)`. Mode 1 gives `P + d*(1-A)`, mode 2 `d + P`,
    mode 3 `d*(P + 1-A)` and mode 4 `d + P*(1-d)`, each held to 1. Modes 3 and 4
    leave the screen opaque, which shows only on a subprogram's screen.
  - All the divisions are by powers of 255, which is odd, so no exact result
    falls halfway between two values.
  - TEST49's screenshots come from an emulator running SmileBASIC 4 on a PC's
    GPU, and that GPU stores results in its own way.
    - About one patch in six has a channel 1 off the rules above, mostly where
      the exact value is a little above a half (`&H80010101` added to 60 is
      60.502, shown as 60).
    - A screen result cut to 12 bits of fraction, with halves rounded down
      (`floor(x*4096)`, then `(q*255 + 2047) >> 12`), gives every FADE sample
      exactly and nearly every additive one.
    - That is the PC's GPU, not SmileBASIC 4; the Switch's GPU may round
      differently.
- An additive sprite inside a composed layer adds light without covering
  anything, so it also brightens what is under the layer.

### `FADE`

- `FADE` is applied as the screen is sent to the display: `d + (c-d)*fade.a`,
  rounded once. Beside a subprogram, the side without the focus is darkened in
  the same step, times 224/255 (`&HE0`): 255 to 224, 248 to 218, 96 to 84
  (TEST49).
- `FADE color,time` mixes the two colors channel by channel in single precision
  and rounds down: `&HFF` to 0 over three frames passes `&HA9` and `&H54`, not
  `&HAA` and `&H55`.
- `FADE` to or from a clear end (alpha 0) takes the other end's color, so only
  the alpha moves; fading to `&H00FFFFFF` from black ends at `&H00000000`.
- `ACLS` clears the fade.

### `LAYER`, `LCLIP` and `LMATRIX`

- `LAYER` mode 0 draws the layer's elements straight onto the screen and ignores
  the layer's color; only modes 1 to 4 multiply by it.
- `LCLIP`'s corners are inclusive.
- `LCLIP OUT` gives a clip past the frame unchanged: 1350 0 1500 719 on a
  1280x720 screen. A layer whose `LMATRIX` puts its text at X 1270, clipped
  there, shows nothing on the screen's left edge (TEST64).
- `LCLIP` at `XSCREEN`'s sample 2 takes whole fine pixels: frame pixels `2*x0`
  to `2*x1+1`.
- `LMATRIX layer,hx,hy` moves the layer by minus the home point.
- The 2D `LMATRIX` form scales first and then turns clockwise, with x and y
  outside, the same as a sprite; it applies after each element's own placement
  (TEST53).
- `LMATRIX layer,M` with 16 reals takes the layer's coordinates as (x, y, 0, 1)
  through M to clip space, X right and Y up: the identity puts the layer's
  square from (0,0) to (1,1) over the screen's top right quarter.
  - M is column-major: M[12] moves along X. With M[3] set, W grows along X and
    the picture narrows that way, the same as OpenGL's perspective divide
    would make it.
  - Raster deformation applies to the screen after LMATRIX has placed the layer
    (TTEST27, pixel for pixel).
  - Sprites on such a layer show whole, also the parts at negative coordinates
    or past the screen's size. FINITI's floors that run to the horizon show
    this.
  - Text screens show whole too: a console moved to (-200,-120) or to (500,300)
    of a 400x240 screen shows where the matrix puts it (TTEST36, pixel for
    pixel).
  - A sprite's Z plays no part: every point goes in with z 0, so Z comes from
    M[2], M[6] and M[14]. Anything with Z/W outside -1 to 1 does not show, nor
    anything with W at 0 or below (TEST53).
  - Sampling is nearest at any magnification, and perspective correct (TEST53).
  - `LCLIP` on such a layer is in frame pixels after the matrix (TEST53).
- `LMATRIX layer,array` wants a real array of 16 elements or more and looks at
  the first 16: 15 are `Type mismatch (LMATRIX:2)`, 17 are accepted, and a
  value of 1E39 is `Out of range (LMATRIX:2)`. (TEST62)

### `LFILTER`

- `ACLS` clears every layer's `LFILTER` (TEST47).
- `LFILTER` copies a raster array when it is called: writing to the array later
  changes nothing on the screen until the next `LFILTER` (TEST60).
- `LFILTER layer,0,n` is `Out of range`.
- A type that does not match the number of arguments is
  `Out of range (LFILTER:2)`, as in `LFILTER 0,0,1`. The mosaic and blur size
  is 0 to 1280 (`LFILTER:3`) (TEST76).
- `LFILTER`'s raster deformation (types 3 and 4) needs the repeat flag:
  `LFILTER 1,3,A,#TRUE` with a real `A[240,4]` or `A[240,2]` works, while
  without the flag it is `Out of range` for the type regardless of the array.
  `A[4,100]` or an Int array with the flag is `Type mismatch`.
- `LFILTER`'s raster deformation works the way OpenGL would. Checked pixel for
  pixel with screenshots (TTEST26):
  - Each row (type 3) or column (type 4) takes an entry of X offset, X scale, Y
    offset and Y scale in screen axes for either type.
  - A pixel shows the source at its center times the scale plus the offset in
    screen widths or heights, measured from the left and from the bottom.
  - A positive X offset moves the picture left; a Y scale of 0.5 stretches the
    bottom half over the screen.
  - Without repeat, the part that falls off shows black, BACKCOLOR included on
    a layer that is not composed.
- `LFILTER`'s raster entries (TEST54):
  - The scale comes before the offset, and a positive Y offset moves the point
    up from the bottom.
  - An array of two values per entry holds the offset and scale along the
    deformation: X for type 3, Y for type 4.
  - With n entries, row or column i of `size` takes entry
    `floor((i+.5)*n/size)`, the one its center falls in.
- `LFILTER` on a layer that `LAYER` does not compose (mode 0) filters the whole
  screen drawn so far: the layers behind and `BACKCOLOR` too. That holds for a
  layer with nothing in it, too. On a composed layer it filters only that layer.
- `LFILTER`'s mosaic draws the whole frame into a few pixels with bilinear
  sampling, and then draws those pixels back over the frame without smoothing.
  - Blocks are n frame pixels regardless of `XSCREEN`'s sample (TEST54), and
    their number is computed in screen pixels: `ceil(width/n)*sample` across.
    Where n does not divide the size, blocks are a little smaller than n,
    ending at both edges of the frame.
  - Sampling puts a block of odd n on its middle pixel, and a block of even n
    exactly between four, which it averages (halves rounded down).
  - A size below 2 does nothing.
- `LFILTER`'s blur halves the layer in frame pixels, regardless of `XSCREEN`'s
  sample. It averages 2x2 blocks (halves rounded down, so a dot becomes a 2x2
  block at 64 and a line 127). It then blurs along the rows and then the
  columns, each stored in 8 bits, and scales back without smoothing. Amounts 1
  to 4 look the same; amount 0 leaves the layer sharp.
- `LFILTER`'s blur weights are `exp(-d*d/2/v)` for v = amount/10 at d = 1, 3,
  5... 31 (the middle counts as 1, and the next tap as 3, though they are
  neighbors). They are normalized over all sixteen, with those past the middle
  counting twice.
  - They are taken outward until they add up to more than 99%, which picks 3, 5,
    7 or 9 taps, and there are never more than 9.
  - Above amount 128 or so the kernel is therefore cut off without normalizing
    again: the edges stay narrow while a plateau falls, and a wide white block
    reaches 251, 224 and 173 at amounts 128, 256 and 384 (TEST54).
  - What shows through falls with the light, so a clear layer darkens what is
    behind it.
- `LFILTER`'s color conversion works directly on the layer's premultiplied
  pixels: hue in degrees, saturation and value on 0 to 255, and a gray has no
  saturation to raise.
  - Half-covered white starts from a value of 128, not 255, and a value of 30 on
    it gives 158 (TEST54).
  - On a layer in mode 1, a clear pixel stays clear.
  - On one in mode 2, 3 or 4, a raised value lights every clear pixel too, so
    the whole screen takes on the value times the layer's color:
    `LAYER 5,4,&HFF404080` with `LFILTER 5,5,200,-50,30` hazes everything by
    (7,7,15), also where layer 5 has nothing (TEST53).
  - The filter comes before the layer's color.

### Sampling and draw order

- Scaled and turned sprites and text are drawn without smoothing, at the centers
  of the frame's pixels; at `XSCREEN`'s sample 2, of the fine pixels. A pixel
  whose center is exactly on the image's edge is drawn if that is the left or
  the bottom edge on the screen, regardless of how the image is turned.
  `SPOFS 0,0.5,0.5` draws from the first column, and from the second row to one
  row past the image's height.
- A sprite or text cell whose pixel centers fall exactly between texels can show
  a pixel off its image. An image side and a scale that are powers of two read
  the page's higher texel, and anything else mostly the lower, at any flip or
  turn (TEST63, 3831 sprites and 192 cells on a Switch 2;
  [renderer-verification.md](../rendering/renderer-verification.md) has the numbers).
  - On a half-pixel position 16x16 shows the page's next row at its bottom edge,
    and 15x15 the column before it at its left.
  - A cell shows what is beside it on the font page or `TPAGE`.
  - Before the page's first texel, U or V of 0, it reads the first texel; after
    the last, U+W of 2048, nothing.
- A sample point exactly between two texels follows the rule above. A side or
  scale that is not a power of two also shows the GPU's single-precision
  arithmetic. About 1 line in 20 goes the other way, by the place on the
  screen, by a triangle of the two a quad is drawn as, or from some line along
  the image on (TEST63).
- At equal Z text screens are behind sprites, and within each group the higher
  number is behind, with `#GSPRITE` counted among the sprites as 4095 (TEST50).

### `XSCREEN` display modes

- `XSCREEN`'s composite modes all fill as much of the display as fits, at the
  aspect ratio, with black bars and no `BACKCOLOR` in them: 1000x600 in mode 2
  fills 1200x720 with uneven pixels, not 1000x600 at whole factors.
- Mode 0 is bilinear and mode 2 nearest.
- Mode 1 mixes the two nearest texels by `clamp(2*fract(s-.5)-.5, 0, 1)` along
  each axis at any scale (s in texels), so a checkerboard at 3x reads 0, 42,
  212, 255, 212, 42 (TEST55). That equals bilinear from a copy at twice the
  size, which is why mode 0 at sample 2 looks the same as mode 1 at sample 1.

## Sound

### `BGMPLAY`, `BGMSTOP` and `BGMVOL`

- `BGMPLAY` plays songs 0 to 73 (the 4.3 manual says 0 to 45).
- `BGMPLAY` of a user song number with nothing set is no error; it plays
  nothing, and `BGMCHK` returns 0.
- `BGMPLAY` of a user song number that holds nothing stops what the track
  plays: after `BGMPLAY 0,200` with song 200 unset, `BGMCHK(0)` reads 0 five
  frames later (TEST71).
- `BGMCHK` of the short songs 4, 5 and 6 stays 1 for 168 or 169, 113 and 344
  frames, a frame or two past the end of their sound (166, 112 and 342;
  TEST71, 168 on a Switch 1 and 169 on a Switch 2).
- `BGMPLAY`, `BGMSTOP`, `BGMPAUSE`, `BGMCONT` and `BEEPSTOP` take an omitted
  first argument with a comma after it (`BGMSTOP ,1`, `BGMPLAY ,0,64`).
- `BGMSTOP` with a fade keeps `BGMCHK` at 1 until a frame after the fade has
  ended: 61 frames for `BGMSTOP 0,1`, even when it comes a frame after
  `BGMPLAY`. Stopped that way in the frame `BGMPLAY` ran in, a song never
  sounds, and `BGMCHK` is 0 a frame later, or two if a `VSYNC` that did not wait
  ran in between.
- `BGMPLAY` without a volume plays at 64, and `BGMVOL` (like `BGMPLAY`'s volume)
  is a linear gain of v/127 over that range. On a square that records at 3063 by
  default, `BGMVOL 127` gives 6079, 95 gives 4547, 63 3015 and 31 1484.
- A track's MML variables read -1 while it is not playing: before any song,
  after `BGMSTOP` and after the song ends.
- `BGMVAR` setting a variable on a track that is not playing does nothing.
- The program reads the values the MML variables had when the frame started, so
  a value set by the song, by `BGMVAR` or by stopping shows a frame later.
- A song started while no other plays takes about two frames more to begin: its
  first `$0=` showed four frames after `BGMPLAY`. Its sound starts 1276
  samples (about 1.6 frames) after `BGMPLAY`, for the first `BGMPLAY` of a run
  and for later ones, and the program continues meanwhile (TEST71).
- `RECSTART` right before such a `BGMPLAY` waits with it: the song sounds within
  50 samples of the recording's start.
- A `BGMPLAY` and a `BEEP` run in the same frame sound together, within 3 ms
  of each other over HDMI (TEST76R).

### MML

- MML takes a bare length after a tie (`C4&2`, and after a chord, `|CEG|4&2`,
  which songs in shared projects use), `@MON`, `N` without a length
  (`N60`), macro names with `_` and macros used inside a macro's definition
  (`{X=CDE}{Y={X}F}`).
- `CHKMML` gives -1 for 16 million characters of notes, and for macros nested
  six and seven levels deep that would come to 10^7 and 10^8 characters
  (TEST51).
- `BGMPLAY` of nine such levels hangs SmileBASIC 4 so that even the stop key
  does nothing (TEST51).
- `CHKMML` follows the octave through `<` (up) and `>` (down) and stops just
  after the one that leaves -1 to 9, the range `O` takes: `O9C<C` gives 4,
  `O9<<<<<<<<<<C` 3 and `O-1C>C` 5. `O10C` stops at the number, 1 (TEST67).
- `@E` takes values 0 to 127, and `CHKMML` stops just after one past that:
  `@E128,0,0,0C` gives 5 and `@E0,0,0,128C` 11. Lower case letters and a
  second dot pass (TEST67, and `o4c` and `C4..` in TEST70).
- `CHKMML` checks no instrument number up to 256: `@148` to `@216` pass
  (TEST67), and so do `@224`, `@255` and `@256` (TEST70). With nothing set,
  `@224` to `@255` all play `@224`'s sine: `@n O4L2A` records at an RMS of 2886
  on 224, 225, 232, 240, 248 and 255. `@256` sounds too, at 698 (TEST72).
- `@MP`, `@MA` and `@ML` take 0 to 127 in each of their four places, and
  `CHKMML` stops just after a 128: `@MP128,1,1,1C` gives 6, `@MP1,128,1,1C` 8,
  `@MP1,1,1,128C` 12, and `@MA` and `@ML` the same (TEST69).
- These four read `-1` as one number and refuse it after its digit:
  `@MP-1,1,1,1C` gives 5. `@MP129,1,1,1C` and `@MP200,1,1,1C` give 6, the same
  as 128 (TEST70).
- `O` takes no MML variable: `CHKMML("$0=9O$0<C")` gives 5, the `$` after the
  `O`. The octave check counts `<` and `>` after `!` the other way around
  (`O9!>C` gives 4, `O9!<C` -1) and inside a chord (`O9|C<E|` 5, `O9|<CE|`
  4), but not through a repeat: `O8[<C]3` gives -1 (TEST69).
- A chord of one note fails: `CHKMML("O8|<C|<C")` gives 3, just after the
  first bar, before the `<` inside is checked (TEST71).
- A chord takes two to six notes: `|CDEFGA|` passes and `|CDEFGAB|` gives 1,
  just after the first bar. An octave change after the last note fails there
  too: `|CE<|` gives 1. Something other than a note between the bars fails at
  the end of the part before it: `|CR|` gives 2 (TEST72).
- An octave set inside a chord lasts to the chord's end: the note after
  `|A<CE|` is in the octave before the chord, so `O8|<CE|<C` passes (TEST72).
  Songs in shared projects write dozens of such chords in a row, which would
  pass octave 9 if the octave carried on.
- A space after `O` changes nothing: `CHKMML("$0=9O $0<C")` gives 5 too, just
  after the `O`. `>` reaches octave -1 from 0: `O0>C` and `O-1C` give -1 and
  `O0>>C` 4. `O9<C` gives 3 and `O9>C` -1 (TEST70).
- A dot adds half of what the one before it added, and fails on an odd number
  of ticks, with the place after the last dot: `C4...` and `C1......` pass,
  `C96..` and `C192.` give 5, while `C96.` and `C48..` pass. A note without a
  length counts from the length `L` set: `L192C.` gives 6 and `L96C.` passes
  (TEST71).
- Macros are read where they are used, so `{X=5}O{X}C` passes (TEST70).
- Places count the text as written, comments included:
  `CHKMML("/A/@MP128,1,1,1C")` gives 9 and `{X=C}/A/@MP128,1,1,1C` 14. An
  error inside a macro is placed just past the macro's use:
  `{X=@MP128,1,1,1}C{X}` gives 20 (TEST71).
- `CHKMML("C]0")` and `CHKMML("C]-")` give 2, just after the `]`, which has no
  `[` before it (TEST71). So do `C]1` and `C]`, while `[C]0`, `[C]1` and `[C]`
  pass (TEST72).
- A channel's MML must be in one piece: going back to a channel used earlier is
  refused at the channel's number.
  `CHKMML(":0@218T120O4L1@MP64,1,16,0:1R1:0@MPONC")` gives 31, just after the
  third `:` (TEST72). Channel 0 counts as used from the start: `C:1D:0E` gives
  5, `:1C:0D` 4 and `:1C:2D:1E` 7. A new channel, lower or higher, passes, and
  so does a `:` naming the channel in use: `:2C:1D`, `:1C:1D`, `:0C:0D` and
  `C:0D` give -1 (TEST74).
- Leaving a channel while one of its `[` is open is refused at the next
  channel's number: `[:1C]2` gives 2, `[C:1D]2` 3 and `:0[C:1D]2` 5, while
  `:1[C]2:2D` passes (TEST74). A `:` naming the channel it is in changes no
  channel, so `[:0C]` passes. A channel past 15 fails at its number: `:16C`
  gives 1 (TEST74).
- `BGMPLAY` and `BGMSET` stop with `Illegal MML`, error 81, only for faults
  that do not have their own error. The MML errors 82 to 106 name the rest:
  `V200C` is `MML:Volume out of range` (94), `C200` and `C5` are
  `MML:Length out of range` (93), `{AB}C` is `MML:Undefined macro` (87), `C]`
  is `MML:RepeatEnd without repeat` (84), `[C` and `[C:1D]2` are
  `MML:Repeat without repeatEnd` (85), and `C:1D:0E` is
  `MML:Track already defined` (83). A value out of range in `@E` is
  `MML:Envelope out of range` (101), and in `@MP`, `@MA` or `@ML`
  `MML:Modulation out of range` (102). Each of these names the command's first
  argument, `(BGMPLAY:1)`, regardless of which argument holds the MML. MML of
  over 1,048,576 characters is `MML:Too long` (82), with no number (TEST76).
- A macro's body is checked when it is defined, before it is used, if ever: a
  macro it uses must be defined already, so `{X={Y}}` is `MML:Undefined macro`
  (87) without Y, and no macro can use itself. A definition inside it, as in
  `{X={Y=C}}`, is `MML:Illegal macro name` (88). Defining a macro again,
  `{X=C}{X=D}`, is `MML:Macro already defined` (86), and a name of over 256
  characters `MML:Macro name is too long` (89) (TEST76).
- `L`, `Q`, `K`, `(` and `)` take no MML variable, like `O`:
  `L$0` is `MML:Can't use variable` (90), placed just after the letter. `V`,
  `P` and `T` take one. `(` and `)` check the resulting volume, which starts
  at 127 in each channel, not their step: `V100(50` and `(` with no `V` before
  it are `MML:Volume out of range` (94), while `V10(-10` passes. After `V$1`
  the steps are not checked, but after `V$0` a `)` is out of range (TEST76).
- `V` and `@V` (MML's volume and velocity) both start at 127 and both scale the
  level by their square: `V64` and `V127@V64` record the same.
- `P64` is the middle, even on both sides. The pan's position is (P-64)/63, so
  `P0` and `P1` are both hard left.
- Recorded with `RECSTART`, an A held at V127 has an RMS of 850 on `@0`, and a
  full-scale 8-bit waveform an eighth of full scale on each side (a ±96 square
  3063, a sine of amplitude 100 2257).
- `Qn` holds a note for (n+1)/33 of its length, cut to whole ticks (48 to a
  quarter note) and at least one. At T120 a `C4` is released after 10 ms with
  `Q0`, 135 with `Q8`, 250 with `Q16`, 375 with `Q24`, and `Q32` holds it all.
- A waveform with attack 127 is at full level from its first sample.
- An instrument's note starts at the level of its first 5 ms step rather than
  rising from silence: `@0`'s C4 passes 64 by its 11th sample.
- `@MP`, `@MA` and `@ML` (depth d, range r, speed s, delay t) swing a sine of
  d/128 times r units at s/2.56 Hz, starting upward. A unit is a semitone of
  pitch, 6 dB of volume, or the pan from the middle to one side.
  - `@MP7,1,7,0` moves about ±6 cents every 360 ms, `@MP32,1,20,0` ±25 cents
    every 130 ms.
  - `@MA24,2,7,0` swings the level 0.77 to 1.31 of its middle.
  - `@ML24,2,7,0` moves the sound right first, each side between about 0.77 and
    1.2 of the middle's level, which fits gains of sqrt((1-p)/2) and
    sqrt((1+p)/2) for a pan p of -1 to 1.
  - The delay counts 40 ms per unit regardless of the tempo, and the wave
    starts from its middle once it has passed: `@MP7,1,7,48` did not move
    within 1.5 s at T120.
  - The sine runs 5 ms behind the note, or behind the end of the delay:
    `@MP64,1,20,0` is at 0 5 ms in, and `@MP64,1,20,8` begins at 325 ms at
    both T60 and T120.
  - `@MOF` stops all three: a note after `@MP100,100,40,0@MOF`, and the same
    with `@MA` or `@ML`, records the same as a note with none (TEST72).
  - `@MON` turns all three back on as last set.
  - `@MPON`, `@MAON` and `@MLON`, which the manual omits, turn one back on as
    last set, and `@MPOF`, `@MAOF` and `@MLOF` turn one off. In
    `@MP64,1,16,0A@MPOFA@MPONA` the first and third notes swing and the second
    does not, the same as with `@MOF` and `@MON`, and `@MPON` with no `@MP`
    before it swings nothing (TEST72, by recording and by ear).
  - A `:` forgets the settings that `@MPON`, `@MAON`, `@MLON` and `@MON` turn
    back on, even a `:` naming the channel already in use:
    `@MP64,1,20,0@MPOF:0@MPON` plays no vibrato.
  - After `@MPO` only `F` or `N` may follow: `@MPONC`, `@MAONC`, `@MLOFC`,
    `@MONC`, `@MOFC` and lower case `@mpon` pass, and `@MPOFFC` is `@MPOF`
    and the note F. With another letter the `@` word continues through the
    letters, and `CHKMML` fails at its end: `@MPOXC` gives 6, `@MOX` 4,
    `@MAOC` and `@MLOC` 5, and `@MPO C` 4, where the space ends the word
    (TEST72).
- `@MA`'s swing is read in tenths of a dB, truncated, and stops at the sound
  engine's limits: no more than +6 dB, and silence from -90.4 dB down. A wide
  one such as `@MA127,110,12,0` (±655 dB as asked) therefore chops the note into
  bursts at twice its level with silence between, and is no louder than that. A
  song using three of these plays at a normal level on SmileBASIC 4.
- Envelopes move one millisecond at a time in tenths of a dB:
  - The attack multiplies the level, starting from -90.4 dB, until it is within
    0.003 dB of full.
  - The hold lasts (h+1)^2/4 ms.
  - Decay and release take off 65535 tenths per millisecond at 127, 24 at 126,
    (2v+1)/640 for v up to 49 and 12/(126-v) above.
  - The sustain level is 400 log10(s/127) tenths, no lower than -723 (-722 for
    1).
  - A note ends below -90.4 dB.
- `@E` sets only attack, decay, sustain and release and keeps an instrument's
  hold; `WAVSET` and `WAVSETA` have no hold.
- `_` (portamento) glides during the note it follows, in a straight line in
  semitones over that note's gate, and the next note continues from there
  without starting again: `T120O4C2_G2` slides from C to G over the first second
  and holds G for the second, and on a piano (`@0`) the G does not strike again.
- `&` (slur) to another note continues at that note immediately without
  striking again: on `@0`, `C4&G4` keeps decaying from the C through the G,
  where `C4G4` strikes the G again.

### Instruments, `WAVSET` and `WAVSETA`

- `@216` to `@222` (PSG) and `@223` (noise) are instruments of the sound archive
  like `@0`. Each PSG tone records at an RMS of about 4031 (peak 4090) at V127,
  regardless of its width, and the noise at 2839, never going below zero.
- `@216` to `@222` are pulses high for 1/8 to 7/8 of each cycle: 131, 254, 377,
  499, 623, 745 and 867 thousandths of an `O4A` recorded (TEST72).
- An instrument region's own volume is a linear gain (`@0`'s is 88), unlike `V`
  and `@V`.
- Notes of `WAVSET` and `WAVSETA` waveforms are read through a smoothing filter
  rather than by holding each sample. A 1 kHz sine at 8000 Hz leaves images
  40 dB down at 7 kHz and 62 dB down at 9 kHz, and a square of 64 samples keeps
  its harmonics within half a dB.
- A few of the sound archive's waveforms are stereo, such as the waveform of
  `BEEP 152`. Their left channel plays on the left and their right channel on
  the right.
- `WAVSET`'s waveform does not play as one cycle of the note. 32 samples of one
  cycle with reference pitch 69 play `O4A` at 55 Hz and `O5A` 3600 cents below
  A5. That is three octaves down, so a pass of the samples lasts eight cycles of
  the note.
- Songs written for SmileBASIC 4 repeat their cycles to fill eight (a 16-sample
  pattern of two cycles times 4, one of a single cycle times 8, both at
  reference pitch 69).
- `WAVSET` plays a pass at 55 Hz at its reference pitch for any of the lengths
  the manual gives, 16 to 512 samples. Any other length (17, 24 or 1024 samples)
  is `Illegal function call`.
- `WAVSET` also takes groups in square brackets, which the manual omits:
  `WAVSET 224,127,0,0,127,"[FF00][80][4080C0]",69` is no error (TEST56).
  - A string starting with `[` has groups of an even number of
    digits, at most 1024 in all, and no group over 512 over their number in
    samples.
  - Each group is stretched to that many samples, but each is written from
    the start, so only the last group stays and the rest of the 512 samples
    keeps what the buffer held.
  - `RECSAVE` shows it (TEST60): `[C040]` sounds like the same 16 digits without
    brackets, and in `[C040][F010]` the `F010` fills half of the pass with the
    other half loud noise, the same each time (peak about 4600 at 127, a DC of
    about -1500).
- `WAVSETA`'s 8-bit form (fewer than 10 arguments) plays a pass of its n samples
  at 28160 Hz over the next power of two at or above n. At reference pitch 69,
  `O4A` with one cycle of 64 samples is in tune, 16 samples sound 2400 cents up,
  512 3600 cents down, 2048 6000 down.
  - Each value is cut to an integer toward zero and keeps only its low 8 bits
    before 128 is subtracted: 256 plays as 0, 65535 and -1 as 255. A waveform
    written as 16-bit numbers, such as noise from a 16-bit shift register, is
    no louder than a full-scale 8-bit one. (Not yet seen on hardware.)
  - The stretch steps through the samples in 8.8 fixed point, n × 256 / pass
    rounded down, so some lengths skip their last samples: 32767 samples
    over 32768 never reach the last 127, 513 over 1024 the last one. The pass
    is at most 1048576 samples. (Not yet seen on hardware.)
- `WAVSETA`'s 16-bit form (10 arguments or more) takes the loop's end + 1 values
  from the offset on, with the loop counted from the offset, and drops what
  comes after; values are cut to 16 bits. A value v sounds like the value
  v/256 + 128 in the 8-bit form.
- `WAVSET` or `WAVSETA` while a note of that instrument sounds writes the new
  samples over the start of the instrument's memory. The note continues reading
  it with the length, loop and rate it started with, keeping its envelope:
  - A square swapped for a square of two cycles doubles the pitch at the same
    level.
  - A `WAVSET` square (512 samples once stretched) swapped for a 32-sample sine
    plays the sine's one cycle and then the rest of the square on each pass,
    near the square's level (2981 to 3023 against 3063) with twice the
    crossings.
- `WAVSET` checks its reference pitch before it reads the waveform:
  `WAVSET 224,0,0,0,0,"A",128` is `Out of range (WAVSET:7)`, not the bad
  waveform's error. A waveform it cannot read is
  `Illegal function call (WAVSET:6)`, for example `WAVSET 224,0,0,0,0,"Z"`
  (TEST76).
- `WAVSETA` with an empty array is `Illegal function call` (TEST51, TEST52).
- `WAVSETA`'s arguments after the array can be omitted at the end, but one
  omitted in between is `Type mismatch` with its number, in both forms and
  ahead of the check for an empty array: `WAVSETA 224,0,0,0,0,A%,,0,15` is
  `(WAVSETA:7)`, and `...,A%,69,,15` and `...,A%,69,,,28160` are
  `(WAVSETA:8)` (TEST51, TEST52). A trailing comma counts:
  `...,A%,69,0,15,28160,` is `(WAVSETA:11)` (TEST57).
- `WAVSETA` looks at its arguments in this order:
  1. Reference note 0 to 127 (`:7`), start and end 0 to
  the last element (`:8`, `:9`), rate 1 to 96000 (`:10`), offset 0 to the last
  element (`:11`), and a flag of any integer, true if not 0 (`:12`).
  2. A start past the end is `Illegal function call`, no number:
  `...,A%,69,10,5` and `...,A%,69,10,5,28160` (TEST57). A start equal to the end
  is no error and defines nothing, in both forms, so neither does an array of
  one element: `...,A%,69,5,5` (TEST57).
  3. In the 16-bit form, an end the offset leaves no room for is `Out of range`:
  `...,A%,69,0,15,28160,4` on 16 elements is `(WAVSETA:11)` (TEST57).
- `WAVSETA` on an empty array checks the range first in the 16-bit form:
  `DIM A%[0]:WAVSETA 224,0,0,0,0,A%,69,0,0,28160` is `Out of range (WAVSETA:8)`
  (TEST58).

### `BEEP`

- `BEEP` sounds 0 to 156 (4.4.5 added 156).
- `BEEP` of a waveform set by `WAVSETA` can be silent: `DIM W%[32]`,
  `WAVSETA 224,127,127,127,127,W%,69` and `BEEP(224)` gives a handle and no
  error, and nothing is heard (TEST58).
- `WAVSET` and `WAVSETA` also write the sound `BEEP` plays, a note at the
  reference pitch as long as a pass of the waveform, cut to ticks of 1/192 of a
  second (TEST59):
  - 32 and 128 samples at 28160 Hz produce no note and record as silence.
  - `WAVSET` always has a note one second long.
  - `WAVSETA`'s 8-bit form needs 147 samples or more, once rounded up to a power
    of two, so 256.
  - `WAVSETA`'s 16-bit form has a note as long as (end + 1) / rate.
  - A start equal to the end returns before any of this, so the old waveform and
    its `BEEP` stay: a `WAVSET` wave plays identically after it.
- `BEEP` of such a waveform ends on the sound engine's steps of 5 ms, each 0.96
  of those ticks, the first included, and takes one more step to fall. The level
  is steady up to the last sample that is not silent (TEST59):

  | Waveform | Last sample that is not silent |
  |---|---|
  | `WAVSET` (192 ticks) | 999 ms |
  | 8-bit, 256 samples (1 tick) | 9 ms |
  | 4096 samples (27 ticks) | 144 ms |
  | 16-bit, 28160 samples at 28160 Hz | 999 ms |

- `WAVSETA`'s flag, when not 0, repeats the note until stopped, each pass a new
  note over the last one's fall. 28160 samples of a ±30000 square at 28160 Hz,
  at `BEEP`'s default, peak at 4234 and at 7905 in the last 5 ms of each second,
  heard as a momentary noise once a second. The peak per 1/10 s is
  `4444444448444444444944444444494`. `BEEPSTOP` after 180 frames leaves the last
  sample at 3004 ms, the step's fall (TEST59).
- With that flag, the notes before do not stop. They fall by the envelope's
  release however long that takes, so with a slow one they pile up (TEST61).
  Take 2816 samples of a ±10000 square at 28160 Hz with release 32, a pass of
  about 100 ms. One note peaks at 1411, and the sound peaks at 9456 at an RMS of
  4333. The peak per 1/10 s is `235567789877654` with `BEEPSTOP` of the handle
  at 1 s.
- `BEEPSTOP` without a fade releases all the handle's notes and frees the
  handle. The sound continues past it by the release, the 1.5 s recording of
  the case above still sounding at its end. At release 127 that is one step,
  the same as when the 16th `BEEP` after a `BEEP` statement takes over its
  place: a 1 s square ends at 404 ms for a `BEEP` at 400 ms (TEST61).
- `BEEPSTOP` with a fade takes the volume to 0 first, so nothing is heard after
  it. The note is the same with or without the flag.
- The sound engine has 96 voices for everything, BGM too. A new note takes one
  from the lowest priority there is, the oldest among equals, or does not sound
  if all are higher.
- `BEEP`'s volume is a linear gain of v/127, like `BGMVOL`, with a default of
  64, the same as `BGMPLAY`. It comes out level with `BGMPLAY` at its default:
  - At envelope 127,127,127,127, a `WAVSET` square of `"FFFF0000"*8` peaks at
    4623, an 8-bit `WAVSETA` square of ±100 at 3613 and a 16-bit one of ±30000
    at 4234 (TEST59).
  - `@224` at `V127` of a ±96 square peaks at 3468 under `BGMPLAY`, for an RMS
    of 3063 (TEST40).
  - These are all 4624 for full scale, and an RMS of 4084.
  - A square peaks about 13% over its RMS because of how the console resamples
    it, where a sine does not: amplitude 100 peaks at 3217 at an RMS of 2257
    (TEST40).
  - `BEEP` of the same shapes records the same: the ±96 square an RMS of 3050
    and a peak of 3468, the sine 2257 and 3217.
  - A square's level at volume 127 and 32 is 6053 and 1525, both in `BEEP`'s
    argument and by `BEEPVOL` (TEST61).
- `BEEP` 224 to 255 with nothing set plays a sine of about 440 Hz for a second,
  peak 4117, last sample at 999 ms (`BEEP 250`, TEST59).
- `@224` in the sound archive is that sine, 16384 samples at 28160 Hz. `@225` to
  `@255` are 140 samples of silence there, but all of `@224` to `@255` start on
  `@224`'s waves. In MML too, `T120@250V127O4A2` and the same on `@240`, never
  set, peak at 4089 at an RMS of 2881 and sound from 0 to 999 ms (TEST61).
- `BEEP`'s sound for each of 224 to 255 is `T400V127P64@nT60O3A4` until set, and
  `WAVSET` of a number replaces only that number.
  Nothing a program can run puts them back: not `SNDSTOP`, `ACLS` or `CLEAR`.
- `BEEP`'s result is a number 0 to 15 that keeps rising with each `BEEP`, also
  after the last has ended and for a silent one, and through program runs.
  Running TEST59's parts in the order 20, 16, 17, 18, 19 on a started SmileBASIC
  4 gave 0, 1, 2, 3, 4, 5, 6, 7, 8 (TEST59). It wraps around at 16 and nothing
  resets it: not `ACLS`, not `RUN`. Each interpreter has its own count.
- `BEEP`'s result is a place to play in (TEST61):
  - `BEEP` used as a function holds its place until the sound ends or `BEEPSTOP`
    names it, and the next `BEEP` skips it.
  - `BEEP` as a command does not hold it, and the 16th after it ends it.
  - With all 16 held, the result is -1, nothing plays and there is no error.
  - A number stopped is the next given, and the one after it next.
- `BEEPSTOP`, `BEEPVOL`, `BEEPPIT` and `BEEPPAN` act on what plays in that place
  at that moment, do nothing if that has ended, and cut a sound that has since
  taken the number (TEST61). By the code they take 0 to 15 only, so -1 is
  `Out of range`.
- `BEEP` of a preset effect at its default volume comes out at 100/127 of its
  own note in MML for `BEEP 0` and 70/127 for `BEEP 256`, which is the effects'
  own volume (TEST61): `BEEP 0` peaks at 703 where `T130@256V127@V101O4C24`
  peaks at 893, `BEEP 256` at 1542 where `T130@0V127O4C4` peaks at 2798, and
  `BEEP 95` at 1455 (RMS over the first 100 ms 312, 429 and 303).
- `BEEP`'s volume on a preset effect is linear, the same as for waveforms:
  `BEEP 0` at 127 and 32 peaks at 1395 and 351 (TEST61).
- `BEEP`'s pan moves a preset effect at equal power. `BEEP 0` records at an RMS
  of about 150 on each side at pan 64, and at 0 or 127 about 210 on its side,
  1.41 times as much, and 0 on the other. `BEEPPAN` gives the same numbers as
  `BEEP`'s argument. A stereo effect plays its left channel on the left and its
  right channel on the right, and the pan moves both, so part of the other side
  stays at full pan: `BEEP 40` at pan 0 records 224 left and 147
  right, and `BEEP 152` 756 and 283 (TEST72).

### `TALK` and sung notes

- `TALK` reads は as "wa" when 、 comes right after it, as for a particle;
  before 。 or anywhere else it is "ha".
- Without a `<P…>` tag, `TALK` speaks at one pitch, 100 Hz. It does not fall
  through a phrase, at 、 or 。, or on は before 、.
- With a `<P…>` tag, `<P128>` included, `TALK` speaks at about 20100/P Hz:
  158 Hz at `<P128>`, 314 at `<P64>` and 39.4 at `<P512>`. The pitch falls
  through the phrase, `<P128>` from about 184 Hz to 113. The tag leaves the
  length unchanged (TEST76R).
- A vowel takes 90 ms at the standard speed, so あいうえお lasts 460 ms. 20
  vowels keep `TALKCHK` set for about 121 frames. One mora, such as `"<V32>a"`,
  `"<V32>$a"` or `"<V32>あ"`, keeps it set for one or two frames (TEST76).
- `SNDSTOP` and `TALKSTOP` cut `TALK` about a frame (15 ms) sooner than
  `SNDSTOP` cuts `BEEP` or `PCMSTREAM`. `TALKCHK()` reads 0 immediately
  (TEST76R).
- A `$` marks phoneme letters only as the first character of the text, before
  any control string: `"$<S128>koqnichiwa"` speaks. Anywhere else, `$` and
  Latin letters say nothing, so `"<V16>$a"` is silent.
- Sung notes and `TALK` pitches have a whole period at 16 kHz. A sung note's
  period is cut down to whole samples: A4 is 444.4 Hz, not 440, C5 533.3 Hz
  and F4 355.6 Hz (TEST76R).
- A new `TALK` cuts off what is being said (TEST71).
- `TALK` takes `$ca`, `$xa`, `$rya` and `$cha` with no error (TEST71).
- A sung note (`*A,C2`) is the voice alone: the channel's instrument stays
  silent under it, and plays again on the next plain note.
- Each channel sings with its own voice, so two channels' sung notes
  overlap, and `TALK` and sung notes sound together without cutting each other
  off.
- `CHKMML` does not check a sung note's letters: `*CA,C`, `*XA,C` and
  `*KXA,C` give -1 (TEST71). `*KONNICHIWA,C1` is `Illegal MML` at its second
  N, the fourth letter (TEST44). A sung note takes three letters at most
  before its comma: `*KOQNICHIWA,C1` and `*SHYA,C` give 4, and `*KON,C`
  passes (TEST72).

### Effector, recording and balance

- `EFCWET`'s four sends (BEEP, BGM, TALK and PCM) all start at 127, even with
  the effector off. `SNDSTOP` turns the effector off and sets every send back
  to 127, `RECSTART` sets them all to 0, and `RUN` leaves them alone.
- A send does not lower the dry sound of `BEEP` and BGM. They keep all of their
  sound and send a copy of wet/127 of it. With `EFCEN 0` that copy goes
  straight to the output, so at the start (sends 127, effector off) `BEEP` and
  BGM play twice as loud as after `EFCWET 0,0,0,0`.
- The part that `PCMSTREAM` and `TALK` send is taken out of their own sound, so
  with `EFCEN 0` they play at the same level regardless of the send. `TALK`
  never reaches the reverb: with the effector on, the part it sends is lost.
- `RECSTART` records the sound before the effector, or the effector did nothing:
  a short note recorded the same, to the unit, with `EFCEN` off, with
  `#EFCCAVE` at `EFCWET 0,127,0,0`, with a hall set by
  `EFCSET #EFCREFHALL,...` and with `EFCEN` off again. It records only what
  `BEEP` and BGM keep, not the copy they send.
- `BGMWET` sets the send of the song a track plays, from then until the next
  `EFCWET`, which sets the send of every track, or the track's next `BGMPLAY`,
  which goes back to `EFCWET`'s send. So a `BGMWET` before its `BGMPLAY`
  changes nothing (TEST76R).
- `EFCSET #EFCOFF` turns the effector off, so `EFCEN()` reads 0, and keeps the
  reverb set before: a later `EFCEN 1` plays that preset (TEST76R).
- `SNDMVOL` starts at 88, not 127, and the output is linear in it, so
  `SNDMVOL 127` plays 1.43 times as loud as the start.
- `SNDSTOP` and `RUN` put `SNDMVOL` back at 88.
- The main program's `SNDMVOL` scales the subprogram's sound the same as its
  own (TEST76).
- `SNDSTOP` in either program stops the sound of both: a subprogram's
  `SNDSTOP` stops the main program's song (TEST76).
- `SNDSTOP` holds the program until the next frame when the program has
  synced in this frame: right after `VSYNC` it takes one frame, and so does a
  second `SNDSTOP`. After a busy loop past the frame it continues without
  waiting (TEST76).
- A subprogram's `SNDMVOL` checks its arguments and then does nothing:
  `SNDMVOL 32` there leaves 88, and `SNDMVOL 200` is
  `Out of range (SNDMVOL:1)` (TEST76).
- `RECDATA` positions wrap around the buffer too while `RECSTART` without a
  time records into it, wrapping around (-1 is the last sample, `RECLEN()` the
  first). With a time, -1 is no error (TEST32). A position outside such a
  recording reads 0 on both sides (TEST76).
- `RECSTART` leaves the older recordings in the buffer. While a recording wraps
  around it, `RECSAVE` from -1000, from past what this recording has reached,
  or of 1000 samples from 500 before its end gives those older samples where
  this recording has none (TEST71).
- `RECSAVE` of a number is `Type mismatch (RECSAVE:1)`: `RECSAVE Z,Z,Z`. So is
  `RECSAVE` into a string array or one of three dimensions, after the count
  and the position are read (TEST76).
- `RECSTART` does not record `PCMSTREAM`: streams played at full volume
  recorded 0, while the person heard them (TEST74).
- A subprogram's `RECSTART` records nothing itself, and its `RECSAVE` gives
  what the main program recorded last. A `BEEP` that the subprogram played and
  saved came out to the unit as the main program's recording of it, at every
  balance, and a main program's `BEEP` saved by the subprogram gave that
  earlier recording again (TEST74). The subprogram's `RECSTART` and `RECSTOP`
  ignore their arguments, so `RECSTART 100` there is no error, and its
  `RECSTOP` leaves the main program's recording going.
- `SNDMSBAL` is one balance that both programs share. The subprogram's
  `SNDMSBAL` sets it, the main program's `SNDMSBAL` does nothing, and
  `SNDMSBAL()` reads it in either program.
- `SNDMSBAL` starts at 63, though the manual calls 64 even. Each `SUBRUN` puts
  the balance back halfway between 0 and 127, which reads 63, and so does
  `SNDSTOP` in either program (TEST76). `RUN` leaves it alone.
- `SNDMSBAL`'s and `SNDMVOL`'s time is in seconds. The value moves to the new
  one in a straight line, a step each frame, and `SNDMSBAL()` and `SNDMVOL()`
  read the value reached so far, rounded down. The time gone grows by 1/60 of
  a second each frame in single precision, so a move from 0 to 127 over a
  second reads 126 at its 60th frame and 127 at its 61st (TEST76).
- `SNDMSBAL`'s balance b scales the main program's sound by 2b/127 and the
  subprogram's by 2(127-b)/127, so halfway each plays as loud as the main
  program alone. A main program's `RECSTART` hears both, mixed this way. With
  the balance set in the subprogram, `BEEP 256,0,64,64` in the
  main program recorded at an RMS of 0, 199, 318 and 631 for balances 0, 40,
  64 and 127, and the same `BEEP` in the subprogram at 600, 419, 313 and 125 to
  134 (TEST72; 317 to 318 and 632 at 64 and 127 in TEST74). At 127 the
  subprogram's side still recorded something, where 2(127-b)/127 gives none.
- `SNDMSBAL()` and `PCMPOS()` work as functions.
- At the starting `SNDMVOL`, a full-scale `PCMSTREAM` at full `PCMVOL` plays at
  0.35 of full scale.
- `PCMSTREAM` sets both `PCMVOL`s to 32767, so a `PCMVOL` set before it does
  not hold: after `PCMVOL 0,4096:PCMVOL 1,4096`, `PCMVOL OUT` read 32767 and
  32767 27 frames into the stream. Before any stream it read 0 and 0, and
  `PCMPOS()` 0 (TEST74, on a freshly started Switch 2). With no stream, and
  during one, `PCMVOL OUT` reads back what was set (TEST76).
- `PCMPOS()` gives how far the stream has been fed to the sound engine, not
  the sample that plays. It reads 0 right after `PCMSTREAM` and 3073 a frame
  later, then jumps: by 2160 at 48000 Hz, 2080 or 2081 at 32000 and 8000 Hz
  and 2095 at 22050 Hz, modulo the stream's length. The jumps come every
  (jump / rate) seconds, 2 to 3 frames apart at 48000 Hz and 14 to 16 at
  8000 Hz (TEST76; TEST74 saw the same). That fits an engine that takes the
  stream in blocks of 5 ms and, when fewer than 1024 samples are left ahead,
  feeds it to 3072 ahead, `PCMPOS()` being the fed position plus 1.
- `PCMSTREAM` plays the low 16 bits of an integer as a signed sample: 65535
  plays as -1, so a program may store samples as 0 to 65535. It cuts a real to
  a whole number and holds it within -32768 to 32767 (TEST76R).
- A 2D array plays its first row on the left and its second on the right.
  Further rows do not play. An array of strings or of three dimensions is a
  `Type mismatch`, and an empty one an `Illegal function call`. Two arrays have
  one dimension each and the same length, or the second is an
  `Illegal function call` (TEST76).
- `PCMSTREAM` with a number alone, such as `PCMSTREAM 22050`, sets the rate of
  the current stream and leaves its arrays, position and volume unchanged
  (TEST76).
- A stream keeps the length its arrays had at `PCMSTREAM`. After `RESIZE` makes
  one shorter, the stream reads the memory past the array's end. (Not yet seen
  on hardware.)

## Signal processing

- `FFT` does not scale. `IFFT` divides by the length, and its result of 1,2,3,4
  prints exact zeros.
- `FFT` and `IFFT` of a length that is not a power of two are
  `Illegal function call`, and so is `FFTWFN`.
- `FFTWFN` gives the periodic windows, dividing by the length: Hamming of 8 is
  .080 .215 .540 .865 1 .865 .540 .215.
- `BIQUAD` keeps each channel's last two inputs and outputs in the coefficients'
  elements 5 to 8 (left or mono) and 9 to 12 (right), as in[-1], in[-2],
  out[-1], out[-2].
- `BIQUAD` with arrays of 4 and 8 is `Illegal function call (BIQUAD:2)`. With
  `ZS[2,8]` and `ZT[8]` it is `Illegal function call (BIQUAD)`, no argument
  named. (TEST63) An output longer than the input is no error, and keeps
  its length (TEST71).
- `BQPARAM` takes the gain for types 5 to 7 only: omitting it there, or
  giving it for types 0 to 4, is `Wrong number of arguments`.
- `BQPARAM` with a sampling rate of 0 is `Illegal function call`.
- `BQPARAM` looks at the number of its arguments before their values: type 5
  with no gain is `Wrong number of arguments (BQPARAM:2)`. A Q of 0 is
  `Illegal function call (BQPARAM:5)`. (TEST62)
- More of these arguments are named: a rate of 0 is `(BQPARAM:3)`, a type out
  of 0 to 7 `Out of range (BQPARAM:2)`, fewer than 13 coefficients
  `Illegal function call (BQPARAM:1)` and `(BIQUAD:3)`, and a window type out
  of 0 to 3 `Out of range (FFTWFN:2)`. `FFTWFN` looks at the length before the
  window type, and `BIQUAD` at the channels before the coefficients (TEST76).
- `FFTWFN`'s window and the coefficients of `BQPARAM` and `BIQUAD` must be real
  arrays: an int array is `Type mismatch`, `(FFTWFN:1)`, `(BQPARAM:1)` and
  `(BIQUAD:3)`. `BIQUAD`'s sounds may be ints. (Not yet seen on hardware.)
- `FFT` takes an int array as its window with no error. A window of zeros gave
  zero outputs (TEST76). It reads the array's bytes as
  reals, the second half of them past the array's end.
- `FFT` and `IFFT` take the length from the third array, the input's real part.
  Not a power of two is `Illegal function call` naming nothing; a later array of
  another length is named, for example `(FFT:2)`. The first array's length is
  not checked (TEST76).

## DIRECT mode

- `INSPECT` (or `??`) prints `INT: 1`, `REAL: 1.50000000` (always 8 decimals) or
  `STRING: (2)"AB"` (a 50-character string still whole).
- `INSPECT` of an array prints `INT[2,2]:`, then ` [0,1]: 0` for each element
  (strings as `(2)"HI"`), then an empty line.
- `INSPECT` shows at most 256 characters of a string; a longer one is cut there,
  without its closing quote.
- `INSPECT` cuts a string of exactly 256 characters too: no closing quote from
  256 up, 255 has it. (TEST62)
- `INSPECT` stops a long listing every 28 lines with `=== Press ENTER ===` on
  the last row, which stays free while it prints.
- `? ?1` prints an empty line, then 1.
- Variables stay until `CLEAR`, however they were created: by `DIM`, `VAR`,
  assigning or only reading them (an unknown variable reads as the real 0).
  Declaring one of them again is `Duplicate variable`.
- A line's declarations are checked as it runs, not when it compiles:
  `DIM H[2]:DIM H[2]` declares `H` and then fails on the second `DIM`. In a
  program the same line does not compile.
- A declaration that fails still declares. After `DIM D[0,3]` fails, `D` is the
  omitted value (`TYPEOF` 0, and `?D` is `Type mismatch`); after `VAR G=SQR(-1)`
  fails, `G` is 0. A line that does not compile declares nothing.
- `CONST`, `ENUM`, `RETURN`, `BREAK`, `CONTINUE`, `DEFOUT`, `DATA`, `READ`,
  `RESTORE`, `OPTION` and `STOP` typed in DIRECT mode are
  `Can't use from direct mode`, error 43. The error comes when the line
  compiles, so nothing on the line runs, and the message names no place.
  SmileBASIC 4 refuses the statement before it reads what follows the keyword,
  so `RESTORE @X`, `BREAK 1`, `DATA A` and `STOP 1,2` are error 43 too. A
  statement before it on the line still gives its own error first:
  `?#B:RETURN` is `Undefined constant`. `OPTION` and `STOP` are names, so
  `STOP=1` assigns a variable; `CONST` and `ENUM` are keywords (TEST76).
- `DEF` and `COMMON DEF` typed in DIRECT mode are error 43 too, for the whole
  line: `DEF F:VAR Q=5:END:F:?"A"` prints nothing (TEST76).
- A call to a name defined nowhere, such as `NOSUCH 1` or `?NOSUCH(1)`, is
  a `Syntax error` when a DIRECT mode line compiles, after its arguments:
  `?"A":NOSUCH` prints nothing. In a program it is a `Syntax error` only when
  the call runs (TEST76).
- DIRECT mode calls a `COMMON DEF` of the program run last: after `RUN 3` of a
  slot holding `COMMON DEF G`, a typed `G` runs it. A plain `DEF G` there is a
  `Syntax error` (TEST76).
- `#_LINE` is 0 in DIRECT mode (TEST76).
- `CLEAR` in a program is a `Syntax error` when it runs. `PROJECT` in a program
  is `Can't use in program`, error 44, when it runs: after `?"A"` it prints A,
  then `Can't use in program in 3:1(PROJECT)` (TEST71).
- DIRECT mode takes `RUN`, `CONT`, `NEW`, `CLEAR`, `LIST`, `TRACE`,
  `BACKTRACE`, `SUBRUN`, `SUBSTOP`, `SUBSHOW` and `SUBHIDE` only as a line's
  first word. Each reads its own literal arguments and ignores the rest of the
  line: `RUN 1+2` runs slot 1, `CONT:?1` only continues, and `CLEAR 5` clears.
  Anywhere else on the line, after a `:`, `THEN` or `ELSE`, they make the line
  a `Syntax error`. After a `:`, as in `?"BEFORE RUN":RUN 3` or
  `?"BEFORE CONT":CONT`, nothing on the line runs, `ENVSTAT` stays unchanged,
  and a stopped program can still continue with `CONT` alone (TEST74).
- `RUN` in DIRECT mode takes one number literal, decimal, `&H` or `&B`. A slot
  of 6 or more is `Out of range`, and a variable, a `:` or `1.5` is
  `Illegal function call`. `RUN -3` runs slot 0, and a compile error there
  reaches `ENVSTAT`, which a typed line's errors do not (TEST76).
- `LIST` in DIRECT mode takes number literals too. `LIST 1:` opens slot 1 in
  the editor, and a line of 1000000 or more is `Out of range`. A real, as in
  `LIST 1.5`, is a `Syntax error`, and anything but a line number after the
  `:`, as in `LIST 1:A`, `Type mismatch` (TEST76).
- `SUBRUN` with nothing after it is a `Syntax error`, and `SUBRUN A$`
  `Illegal function call` (TEST76).
- `ENVINPUT$` lists a line typed again as a new line and leaves the older copy
  where it was. The line typed just before, typed again, is not added (TEST71).
- `FILES` prints a long list a screen at a time, both in DIRECT mode and in
  programs. The list fills every row of the console but the last, which says
  `=== Press ENTER ===`, so the page follows the console's size (29 rows of the
  list on the usual 30). Any key or button continues the list, and a held key
  continues it each time it repeats. Once a key has continued it, the
  `=== Press ENTER ===` line does not stay among the names (TEST72).
- `FILES` of one type starts with a line giving the count: `FILES "DAT:"`
  shows `-- 1 files --` above a project's one DAT file, and `-- 0 files --`
  when there are none (TEST71).
- `FILES` of one file's name, such as `FILES "TXT:NAME"`, prints nothing, not
  even the count (TEST72). So does `FILES` of a missing file or of a name with
  no type (TEST74).

## Stopping

- `ON BREAK GOTO ""` removes the handler.
- The + button acts when it is released. A short press jumps to the label of
  `ON BREAK GOTO`, once; without one, the program stops with
  `User break on 0:55`. Held for a second, the button stops the program
  immediately, without the jump.
- `ON BREAK GOTO`'s jump leaves GOSUBs behind: a `RETURN` after it is
  `RETURN without GOSUB`. It leaves DEF calls behind too: after + in a DEF
  called from a DEF, a loop of `GOSUB`s from the label reaches the full depth
  (TEST71).
- The jump leaves an `SPFUNC` callback that `CALL SPRITE` runs, and a later
  `CALL SPRITE` works as before (TEST76K).
- `ON BREAK GOTO` inside a DEF may name a label outside it. When the DEF has a
  label of the same name too, the jump goes to the one outside (TEST71).
- Breaking off a line typed in DIRECT mode says `User break on 0:1`. A forced
  stop (holding + for a second) says `User break on` too.
- A press of + out of a waiting `INPUT` or `LINPUT` counts twice. It jumps, and
  acts again once the first statement there has run (a whole FOR loop's first
  statement only). The second time it jumps to a label that statement set with
  `ON BREAK GOTO`, or stops with `User break on` that statement's line. Printing
  continues after the guide.
- `STOP` says `Break on 0:60`, then its message, if it has one, on the next
  line. In DIRECT mode it is `Can't use from direct mode`.
- `STOP` with a message prints 255 characters of it (TEST62).
- `CONT` continues, against the manual, after a program has been edited, even
  with lines added.
- `CONT` continues, against the manual, after the + button stopped a program
  at `INPUT`: it continues with the next statement, and `User break on`
  follows the guide on the same line.
- `CONT` after an error is `Can't continue`. In a program it is a `Syntax error`
  when it runs. A `Syntax error` typed in DIRECT mode after a `STOP` does
  not count: `CONT` still continues (TEST71).
- `BACKTRACE` lists, innermost first, the line it stopped on and the line of
  each DEF call and `GOSUB` it is in, as `0:    92` (the line right-aligned in 6
  places). It works after an error too, and prints nothing when no program has
  stopped. After it, `CONT` and `TRACE` cannot continue.
- `TRACE` runs one statement and says `Break on 0:2` with that statement's line,
  in DEFs and GOSUBs too. After an error it is `Can't trace`.
  - Some places are not a separate statement: a `FOR` continues into the first
    statement of its body, and a DEF's `END` continues through the return into
    the caller's next statement.
  - `NEXT` takes two TRACEs that both say the FOR's line (the step, then the
    test).
  - After a GOSUB's `RETURN` it names the GOSUB's line.
- `Can't continue`, `Can't trace` and `Can't use from direct mode` name no line.
- `Break on` follows anything printed before it: after `?"X";` it reads
  `XBreak on 0:16`.
- `ENVSTAT()` on the main interpreter is 3 (running), also typed in DIRECT mode
  while a program is stopped or after an error; its OUT forms then report no
  error.

## Program slots and files

### `CHKCALL` and `CHKLABEL`

- `CHKCALL` is 0 for `PRINT`, `GOTO`, `INC`, `DEC`, `STOP`, `CONT`, `RUN`,
  `NEW`, `CLEAR`, `LIST`, `TRACE`, `BACKTRACE`, `EXEC`, `DEFOUT` and `END`, and
  1 for `VSYNC`, `WAIT`, `INSPECT`, `CALLIDX`, `DEFARGC`, `PROJECT`, `FILES`,
  `ACLS`, `LOAD` and `SAVE`. It ignores case.
- `CHKCALL("PRINT")` and `CHKCALL("GOTO")` are 0, because statements are not
  commands, and `CHKCALL("3:SIN")` is 1 and `CHKCALL("3:NOSUCH")` 0. (TEST63)
- `CHKCALL("1:ACLS")` is 1 for the system's commands and functions, regardless
  of what is in slot 1.
- `CALL "WAIT",n`, `CALL "VSYNC",n` and `CALL("FREEMEM")` work.
- `CHKLABEL` needs the `@`: `CHKLABEL("P1")` is 0.
- `CHKLABEL("1:@X")` on a slot `EXEC` has not run is 0. With a slot, it looks at
  that slot's top level, also from a DEF in the same slot: FINITI finds its own
  models this way.
- A slot that has not been run gives 0 for `CHKLABEL("3:@A")`,
  `Undefined variable` for `VAR("3:A")` and `Undefined label` for
  `RESTORE "3:@A"`. (TEST63)

### Slots, `EXEC` and the `PRG` commands

- `LOAD` can, against the manual, load over the program's own slot, and the
  program continues.
- `LOAD` over a slot that `EXEC` ran keeps that slot's `COMMON DEF`s callable,
  against the manual.
- `EXEC` of a slot with code still to run, such as `EXEC 0` in a DEF of slot 1
  that slot 0 called, is `Can't use the specified slot`.
- `EXEC` of the slot running starts it again (`EXEC 0` in slot 0).
- `EXEC` of a slot is `Duplicate function`, error 19, when a `DEF` there,
  `COMMON` or not, has the name of another slot's `COMMON DEF`, or a
  `COMMON DEF` there the name of any `DEF` in another slot. The error names no
  argument and is placed at the slot's `DEF`; nothing of the slot runs. The
  same slot compiled again replaces its own: `COMMON DEF FA3` put into slot 3
  twice gives no error, and calls go to the second one (TEST76). Two plain
  `DEF`s of one name in two slots have not been tried.
- Slots that `EXEC` ran are gone after the next `RUN`.
- `PRGEDIT slot,n` past the last line goes to the last line, and
  `PRGEDIT slot,-1` past it, where `PRGGET$` is empty.
- `PRGSEEK 1` from the last line goes past it and gives 0.
- `PRGDEL` at the end of a slot deletes back toward its start.
- `PRGSET` on an empty line in the middle replaces it.
- `PRGINS` that would take a slot past 1,048,576 characters inserts nothing,
  with no error: 1100 lines of 1000 characters into an empty slot leave 1047
  of them (TEST76).
- SmileBASIC Debugger's model of `PRGEDIT`, `PRGSEEK`, `PRGDEL` and `PRGSET`
  agrees with these four bullets.
- `PRGNAME$()` and `PRGSIZE()` in DIRECT mode mean slot 0, regardless of which
  slot `PRGEDIT` is on. (TEST63)
- After a fresh start of SmileBASIC, slot 0 has an empty `PRGNAME$()` and a
  `PRGSIZE()` of 0 (TEST64).
- `PRGSET`, `PRGINS` and `PRGDEL` changes do not count as unsaved: `LOAD` over
  the slot from a program, `NEW` and `PROJECT` do not ask about them (DAW_MUSIC,
  and by hand), and neither does `EXEC` into a slot holding `PRGINS` edits
  (TEST71).
- `#_FILENAME` is the program's name (`TEST32`), and empty in DIRECT mode.
- `#_SLOT` is the program's slot, and in DIRECT mode the slot run last.

### Files

- `LOADV` reads a DAT file whose header gives a size without the `PCBN` array's
  header: SmileBOY's `POCKET.GB` loads as an array of 32768 (TEST64).
- `EXEC` of a file that is not there shows a dialog (File not found) and then
  stops with `Load failed`. In DIRECT mode, `EXEC` is
  `Can't use from direct mode`, shown without a line.
- File names may start with `@`, like those of the backups:
  `LOADV "DAT:@BACKUP.DAT"` reads one, and `SAVE "@X"` saves (after the usual
  question).
- `@` may come anywhere in a file name: `A@`, `A@B` and a lone `@` are names.
  A `.` may come anywhere but first, so `@.X` is a name too (TEST76).
- Only a `:` at the fourth character marks a type, and only after `TXT`,
  `DAT`, `GRP`, `JPG` or `PNG`. `CHKFILE("TXT:A:B")` and `CHKFILE("AB:C")` are
  `Illegal format (CHKFILE:1)`, error 71. `LOAD "PNG:X",1` is
  `File type mismatch (LOAD)`, the same as `LOAD "DAT:X",1` (TEST76).
- `CHKFILE`, `LOAD`, `SAVE`, `LOADV`, `SAVEV`, `LOADG`, `SAVEG`, `DELETE` and
  `RENAME` raise errors for a name they cannot take, before any
  question, naming the argument as in `(CHKFILE:1)`: `Illegal format`,
  error 71, for a character a name cannot have (`A*`), a second `/`
  (`A/B/C`) or no file name (`TXT:`, `NAME/`, `""`), and
  `File name is too long`, error 73, for a project name or a file name of over
  32 characters. `EXEC` raises the same errors without the number. (Not yet seen
  on hardware.)
- `FILES` takes a name alone as a project name (`FILES "TXT:NAME"`), so a
  project and a file together are error 71 there, and so is `FILES ""`. Its
  name errors name no argument: `FILES "TXT:A/B"` and `FILES ""` are
  `Illegal format (FILES)` (TEST76).
- `FILES` with one argument that is neither a string nor an array is
  `Type mismatch (FILES:1)` regardless of the `OUT` count: `FILES Z`,
  `Q=FILES(Z)` and `FILES Z OUT Q0` (TEST76).
- `SAVEG` takes GRP and JPG files, and a name without a type names a GRP file;
  `SAVEV` takes TXT, DAT and GRP files and needs the type. Any other type, or
  none for `SAVEV`, is `File type mismatch`, error 77, with no number
  (`(SAVEG)`), before any question: `SAVEG "TXT:X",0`, `SAVEG "PNG:X",0` and
  `SAVEV "X",S$`. The name's errors 71 and 73 come first, and 75 and 74 for
  its project after. `EXEC` of a type other than TXT is error 77 before the
  name's other checks: `EXEC "DAT:",1` is 77, not 71 (TEST76).
- `SAVEV` stores a DAT or GRP file's array uncompressed when its values take
  16 MiB or less, and compressed otherwise. Over 16 MiB both ways, the
  compressed body counted with its header, is `Array is too large`, error 47,
  with no number, after the question. So 4,194,304 ints always save, and so
  does a 2048 by 2048 GRP: 4,194,298 random ints, which do not compress, saved
  (TEST76K). An array of more random ints is error 47 (not yet
  seen on hardware), while one as large that compresses well saves.
- `SAVEG` checks its page, rectangle and quality before the file's name, so
  `SAVEG "A*",0,0,0,0,1` is `Out of range (SAVEG:5)`, not error 71. It takes
  the quality, 1 to 100, for a GRP file too and ignores it there:
  `SAVEG "GRP:X",0,50` saves. A rectangle past the page's right edge is
  `(SAVEG:4)` and past its bottom `(SAVEG:5)`, one argument before the size.
  Two or three numbers after the page are `Wrong number of arguments`
  (TEST76).
- `DELETE` of a file in a `#` system project is
  `Can't SAVE to system project (DELETE:1)`, error 75, ahead of the 74 for a
  name with a project: `DELETE "TXT:#SYS/A"`, and `DELETE "A"` while a `#`
  project is the current one. `RENAME` is the same for each name in turn,
  `(RENAME:1)` or `(RENAME:2)`. While a `#` project is the current one,
  `SAVE "X"`, `SAVEV "TXT:X",S$` and `SAVEG "X",0` are error 75 as well,
  without a number (TEST76).
- `DIALOG(name,caption,most)` cuts its first name to `most` characters, then
  checks it the same way as the file commands do: `DIALOG("A@B","X")` opens,
  while `DIALOG("P/A","X")` and `DIALOG("A B","X")` are
  `Illegal file name (DIALOG:1)`, error 37 (TEST76).
- File names may start with `#`, as the first character only:
  `SAVEV "TXT:#NAME",S$` saves, and `LOAD "#NAME"` loads. A `#` anywhere
  else, or on its own, is refused. With a `/` after it, the `#` part is a project
  instead (`#EXAMPLE/FILE`). A `#` file name works on hardware; the rest is
  not yet seen there.
- `CON`, `PRN`, `NUL`, `AUX`, `COM1` to `COM4` and `LPT1` to `LPT4` cannot
  name a file, or a project in a path. `CHKFILE("TXT:CON")`, `SAVE "NUL/A"`
  and `FILES "TXT:LPT1"` are `Illegal format` before any question, naming the
  argument as in `(CHKFILE:1)`, except in `FILES`. Only the whole name counts: `COM5` to
  `COM9`, `LPT5` to `LPT9`, `CON.A`, `#CON` and `@CON` are names like any
  other.
- File names may end in a dot: `SAVEV "TXT:Q51.",S$` saves, and
  `CHKFILE("TXT:Q51.")` is 1 while `CHKFILE("TXT:Q51")` is 0 (TEST51).
- `LOAD` takes the file's whole name: `LOAD "TEST10"` does not find
  `TEST10.PRG`.
- `LOAD` and `LOADV` in a program take other projects' files
  (`LOAD "GAME/SUB",1`), and a program may name its own project in them.
- A program's `EXEC` of a file named with a project is
  `Can't access other project in this mode`, error 74, even for the program's
  own project: `EXEC "TEST05/HGLIB.PRG",1` in a program of project TEST05
  (TEST68, a main program while a subprogram ran). It is the same with no
  subprogram running, and for the workspace's `/NAME`: `EXEC "/NAME",1` is
  error 74, where `LOAD "/NAME",1` of a missing file gives no error and
  `RESULT` 0 (TEST69, 4.4.9 on a Switch and a Switch 2).
- A main program's `SAVEV`, `RENAME` and `DELETE` of a file named with a
  project are `Can't access other project in this mode`, error 74, for the
  program's own project and for the workspace's `/NAME` too, and change no
  file: `SAVEV "TXT:/NAME",S$` is `(SAVEV)`, `RENAME "TXT:/NAME","NEW"`
  `(RENAME:1)` and `DELETE "TXT:/NAME"` `(DELETE:1)`. `SAVEV` is error 74
  also for a name that no file has, for example `SAVEV "TXT:/#T76",S$`
  (TEST76K). Typed in DIRECT mode, `SAVEV "TXT:OWN/NAME",S$`, and
  `SAVEV "TXT:/NAME",S$` in the workspace, ask as usual and save in the
  current project, with `RESULT` 1 (TEST69).
- `LOADG "PNG:NAME",0` in a main program is error 74, `(LOADG)`.
  `LOADG "DAT:NAME",0` with no such DAT file gives no error and `RESULT` 0
  (TEST69).
- `CHKFILE`, `LOADV` and `FILES` take project names in a program:
  `CHKFILE("TXT:HGLIB/HGLIB.PRG")` is 1 (TEST69).
- `LOAD` into a slot and then `EXEC` of the slot runs another project's
  program: `LOAD "OTHER/NAME",1:EXEC 1` in a program works and shows no dialog
  (TEST68, and checked on hardware, version 4.4.9, 2026-11-08).
- `LOADG` of a name with a project loads the image, for the program's own
  project or another, with or without `GRP:`: `LOADG "HGLIB/NAME",0` and
  `LOADG "GRP:HGLIB/NAME",0` both load. A program loads it without a dialog; in
  DIRECT mode SmileBASIC 4 shows its load dialog first (checked on hardware,
  version 4.4.9, 2026-11-08).
- In TEST68, `LOADG "GRP:TEST05/HG16_NEODGM",3,0,0 OUT W,H` in a main
  program, while a `SUBRUN` subprogram ran, gave `RESULT` 0, loaded nothing
  and left the page unchanged, with no error, where
  `LOADG "GRP:HG16_NEODGM",3,0,0 OUT W,H` in the same test loaded the file.
  The command was not the cause: with TEST05 the current project, that line
  loads on both consoles, and so do its forms without `OUT`, with page 0,
  without the project and without `GRP:`, with the subprogram running and
  without it, all in the same time (TEST70). The `RESULT` 0 came from
  TEST68's set-up, most likely another current project at the time, with
  TEST05 not yet holding the file.
- A subprogram's `EXEC` of a file named with a project is error 74 too,
  printed with no command name: `Can't access other project in this mode in
  0:167`. `EXEC` of a file in SmileBASIC 4's own `#` projects is allowed, in a
  program and in a subprogram: one that is not there is `Load failed`
  (TEST71; the subprogram on a Switch 1).
- `SAVE` and `SAVEG` refuse a project name in a main program the same as
  `SAVEV` does, error 74 without an argument's number, for the program's own
  project, another one and the workspace's `/NAME` (TEST71).
- Typed in DIRECT mode and in a subprogram, `SAVE`, `SAVEV` and `SAVEG` write
  to the project named, after the usual questions: the current project named
  as `OWN/NAME` and the workspace's `/NAME` both take the file, with
  `RESULT` 1. A project that is not there asks, then shows that the save
  failed, and gives `RESULT` 0. Expected but not yet seen on hardware:
  another project that is there takes the file too. (TEST71; the subprogram
  on a Switch 1, the failure dialog on a Switch 2.)
- A file in a `#` system project is `Can't SAVE to system project`, error 75,
  for `SAVE`, `SAVEV` and `SAVEG`: `(SAVEV)` in DIRECT mode and
  `in 0:203(SAVEV)` in a subprogram (TEST71, on a Switch 1). A main program
  gets error 75 too, ahead of its error 74: `SAVEV "TXT:#T72/T72W","X"` is
  `Can't SAVE to system project in 3:1(SAVEV)`, and `SAVE` and `SAVEG` the
  same (TEST72).
- `RENAME` and `DELETE` refuse a project name in DIRECT mode too, with error
  74 and no dialog, and leave the files alone:
  `RENAME "TXT:TEST05/T70R","T70S"` is
  `Can't access other project in this mode (RENAME:1)` and
  `DELETE "TXT:/T70R"` `(DELETE:1)` (TEST70). They refuse one in a subprogram
  too (`in 0:41(RENAME:1)`, `in 0:59(DELETE:1)`). `RENAME` checks its second
  name after its first: `(RENAME:2)` when only the second has a project,
  `(RENAME:1)` when both do (TEST71; the subprogram on a Switch 1).
- `RENAME`'s second name without a type takes the type of the first:
  `RENAME "TXT:A","B"` gives `TXT:B`. Two different types, such as
  `RENAME "TXT:A","DAT:B"`, are `File type mismatch (RENAME)`, error 77, with
  no number and no question. A file renamed to its own name gives `RESULT` 1
  and asks nothing (TEST71).
- `LOADG "PNG:T70R",0` in DIRECT mode is
  `Can't access other project in this mode (LOADG)`, error 74, with no dialog
  (TEST70), and so it is in a subprogram: `in 0:95(LOADG)`. An unknown type,
  such as `XYZ:`, is `Illegal format (LOADG:1)`, error 71, and `TXT:` is
  `File type mismatch (LOADG)`, error 77, in DIRECT mode and in a subprogram.
  A `JPG:` file loads. (TEST71; the subprogram on a Switch 1.)
- `LOADG` leaves the page's pixels outside the loaded image unchanged: a
  1024x624 image loaded at 0,0 leaves 1027,5 and 5,627 alone (TEST68).
- `LOADV` and `LOADG` in a program ask nothing. A missing file still opens the
  File not found dialog, which holds the program until it is answered, both in
  a main program and in a subprogram. The command then gives `RESULT` 0,
  and `S$=LOADV("TXT:NONE")` gives `""` (TEST67, where a quick press let the
  program continue after 50 to 70 frames). `LOADG` of a missing `DAT:` or `JPG:`
  file does the same. Left alone, the dialog stays: `LOADG "DAT:NONE",1` in a
  main program came back with `RESULT` 0 after 4584 frames on one console and
  2022 on the other, once the person answered it, over 30 seconds after it
  opened (TEST72). Typed in DIRECT mode, `LOADV` asks before it loads
  (TEST71, by hand).
- `DELETE`, `RENAME`, `LOAD` and `LOADV` of a missing file in a program each
  open a dialog that holds the program until it is answered, and then give
  `RESULT` 0. `RENAME` onto a file that is there asks, and after OK replaces
  it: `RESULT` 1, the old name gone and the new one there (TEST74).
- A running program can `DELETE` the file it was loaded from: `DELETE` checks
  no slot and no running program. `RESULT` is 1 when the file is deleted, -1
  when the question is answered No, and 0 when deleting fails. (Not yet seen on
  hardware.)
- `LOADV "DAT:F",M%` of a real file into an int array converts as `INT()`
  does, with NaN as 0: NaN, 1.9 and -1.9 load as 0, 1 and -1, and 2147483647.5
  as 2147483647 (TEST67).
- The command form `LOADV "TXT:F",S$` is `Wrong number of arguments (LOADV)`,
  error 52, whether the file is there or not, the same as `LOADV "TXT:F",A$[]`
  (TEST67).
- `LOADV "DAT:F",S$` into a string is `Type mismatch (LOADV:2)`, error 8. As a
  function, `S$=LOADV("DAT:F")` of a DAT file of 3 values gives a string of 3
  characters and `RESULT` 1 (TEST69). `LOADV "GRP:X",S$` is the same
  `Type mismatch (LOADV:2)` (TEST71).
- Other wrong counts depend on the type too. A `DAT:` or `GRP:` name with two
  or more OUT variables, `LOADV "DAT:X" OUT A,B`, is `Wrong number of OUT
  arguments`, error 53, and with no OUT variable and one argument error 52. A
  `TXT:` name with one argument and no OUT variable, `LOADV "TXT:X"`, or with
  two, is 53 (TEST76).
- `LOADV` of a `JPG:` or `PNG:` name, as a function and as a command, and of
  a name without a type in either form, is `File type mismatch (LOADV)`, error
  77, with no number (TEST71; the `PNG:` function form in TEST72).
- `RESULT` is 0 after `IF CHKFILE("TXT:NONE") THEN LOADV ...` of a missing
  file, where the program before left it 1 (TEST67).
- `CHKFILE` leaves `RESULT` unchanged, whether the file is there or not:
  after a `LOADV` that gave 1, `CHKFILE` of a missing file gives 0 and of a
  file that is there 1, and `RESULT` stays 1 through both (TEST70). From a
  `RESULT` of 0 it stays 0 (TEST69).
- `RESULT` went from 1, where a main program left it, to 0 in the program
  that `RUN` started next, after some lines typed in DIRECT mode (TEST70). A
  program that ends with `RESULT` 1 leaves it 1 for DIRECT mode, and lines
  typed there that run no file command leave it too (TEST71). `RUN` sets it to
  0 once the program compiles: from 1, a program's first line read 0. A
  program with a `Syntax error` leaves it, 1 staying 1 (TEST72).
- The main and the sub interpreter each have their own `RESULT`. A
  subprogram's `LOADV` that gave 1 left the main program's `RESULT` 0, and the
  main program's `LOADV` left the subprogram's 0 (TEST70, both consoles).
- `LOAD "GRP0:X"` and `SAVE "GRP0:X"` are `Illegal format (LOAD:1)` and
  `(SAVE:1)`, error 71, with no dialog (TEST67), and so is `LOAD "PRG:X",1`.
  Another type of file is `File type mismatch`, error 77, without an
  argument's number: `LOAD "DAT:X",1`, `LOAD "GRP:X",1` and
  `LOAD "JPG:X",1` are `(LOAD)` and `SAVE "DAT:X",1` `(SAVE)`, also when
  there is no such file (TEST69).
- `LOAD "DAT:X"` typed in DIRECT mode is `File type mismatch (LOAD)` too, with
  no dialog (TEST71).
- The question before `SAVE`, `SAVEV` or `SAVEG` starts on OK, so Enter or A
  saves. Replacing a file asks twice, whether to save and then whether to
  overwrite (checked by sending files with a Flipper, 2026-11-03, and in
  TEST67). Three `SAVEV`s of one TXT file kept the copy each replaced in
  `@BACKUP.TXT`, and No on the question gave `RESULT` -1 and left the file
  alone (TEST67).
- `LOAD` and `EXEC` over a slot with unsaved edits ask first, starting on Yes,
  the right button: "There is an unsaved program in slot 0, and it will be
  overwritten and deleted when it is loaded. Are you sure?" `EXEC` answered
  Back stops with `Load failed`, naming no argument, at the `EXEC` (TEST76K).
- An Enter held for about 2 seconds on a USB keyboard to answer `SAVEV`'s
  questions does not reach the program: `KEYBOARD(40)` was 0 when `SAVEV`
  returned, and `INKEY$` stayed empty for the next 4 seconds (TEST74).
- `FILES "//"` on the console starts with the count, such as
  `-- 12 projects --`, like a list of files. `FILES("//")` and
  `FILES "//",A$` put no such line in the array.
- `FILES("//")` ends each project's name with `/`, lists the workspace as `/`,
  and lists each `#` project twice (`#EXAMPLE/`, `#EXAMPLE/`). The names sort
  in ASCII order, so the workspace's `/` comes after names that start with `#`
  or `-` (TEST72).
- `FILES` takes a `/` after a project's name (`FILES("TXT:NAME/")`), and
  `FILES("TXT:/")` lists the workspace.
- `METALOAD "NAME"` loads another project's metadata; `METALOAD` alone loads
  the metadata of the current project again. Before any `METALOAD`, the title
  is the project's name.
- `METALOAD` of a project that is not there is
  `Undefined project name (METALOAD:1)`, error 76, and `ENVSTAT` gives its
  argument as 0 (TEST72).
- `METAEDIT(item)` works as a function. `METAEDIT` cuts a title to 24
  characters and a description to 240, with no error (TEST71).
- `METASAVE` asks first, and does not set `RESULT`: after one that saved,
  `RESULT` was still 0 (TEST71).
- `METAEDIT`, `METASAVE` and `METALOAD` share one buffer. `METASAVE` answered
  No writes nothing, and `METALOAD` with no argument reads the stored
  metadata again, so after `METAEDIT` and a cancelled `METASAVE` it drops the
  edits (TEST76K).
- `METASAVE` while a `#` system project is the current one is
  `Can't SAVE to system project (METASAVE)`, error 75, with no argument's
  number, before its question. (Not yet seen on hardware.)

## Subprograms

Except under "A subprogram's screen", bullets that name no test come from
TTEST36.

### Starting and ending

- `ENVTYPE OUT T,P` gives P -1 in a subprogram.
- `XSUBSCREEN OUT` before any `XSUBSCREEN` gives the whole display,
  0,0,1280,720.
- `SNDMSBAL()` starts at 63 in a subprogram too.
- `PROJECT()` in a subprogram is the project `SUBRUN` loaded the file from.
- `ENVINPUT$(1)` is the line typed last in DIRECT mode, the `SUBRUN` itself.
- `ENVSAVE` kind 0 is `Illegal function call (ENVSAVE:1)` and kind 5 is
  `Out of range (ENVSAVE:1)`, in a main program too. Kinds 3 and 4 give no
  error (TEST71). Kind -1 is `Out of range (ENVSAVE:1)` as well (TEST72).
- A busy subprogram takes none of the main program's time: `A=A+1` and a
  32-pixel `GFILL` in the main program cost the same while the subprogram runs
  a loop with no `VSYNC` (TEST71S).
- An error, a `STOP` or `SUBSTOP` ends the subprogram, hides its screen and
  gives the focus back. The main console prints the reason between a title
  line and a rule line, and prints no OK unless the main console itself ran a
  line:

  ```
  === From subinterpreter ===
  User break on 0:11
  ===========================
  ```

  For an error, the middle line is its message (`Divide by zero in 0:8`); for
  `STOP`, it is `Break on 0:7`.
- `SUBRUN "/A/../B"` prints `OK`, then the same block with `Illegal format` as
  its middle line (TEST51), and `SUBRUN "DAT:Z51A"` does too (TEST57).
- `SUBRUN` takes a name in lower case and one starting with `TXT:`:
  `SUBRUN "test51s"` and `SUBRUN "TXT:TEST51S"` give no error and no block.
- `SUBHIDE` hides the subprogram's screen and `SUBSHOW` shows it again, while it
  keeps running. A clear `BACKCOLOR` lets the main screen show through.
- `END` in a subprogram hides its screen and prints nothing; the line being
  typed in DIRECT mode stays unchanged, and the main interpreter has the focus
  again (TTEST37, TTEST38).
- The tools on F9 to F11 also start while a program runs.
- A tool runs in the subprogram interpreter (TEST69, a tool started from the
  editor):
  - `PROJECT()` is the project of the tool's file, not the current project, and
    `SAVEV` saves into that project.
  - `PRGNAME$()` and slot 0 give the tool's file name without its project.
  - Slots 1 to 3 keep what an earlier tool left in them. The slots of the
    program being edited are not visible to it.
  - `SYSPARAM("TOOLn")` gives the tool as `PROJECT/FILE`.
- A `SUBRUN` or a tool key while a subprogram runs first stops the subprogram
  with `User break`, printed on the main console as above. It does so also when
  the new file cannot be loaded.

### Focus

- A subprogram starts with the focus.
- While a subprogram runs, the side without the focus is drawn darker, each
  color channel to 7/8 (255 shows as 224, 96 as 84). That side is the main
  screen while the subprogram has the focus, and the subprogram's screen once
  the subprogram gives the focus up. The editor is darkened the same way while
  a tool started from it has the focus.
- While a tool started from the editor is open, Ctrl+Tab does nothing: it does
  not move the keys between the editor and the tool (TEST69).
- `PUSHKEY` in the subprogram, after `ENVFOCUS 0`, types into the main
  interpreter's DIRECT mode, which runs the line.
- `DIALOG` in a subprogram responds to the controller with the focus on either
  side, and the main program keeps running meanwhile (TEST64). With the focus
  on the main program, its `BUTTON(0)` sees the A that answers, and its
  `INKEY$` gets no key from it. With the focus on the subprogram, the main
  program sees no A while the dialog is open: `BUTTON` read 0 the whole time on
  a Switch 1 (TEST67).
- The main and the sub interpreter share one dialog. With a `DIALOG` open on
  each side, the text of the later call shows: the main program's text when
  the subprogram asked first (TEST72), and the subprogram's text, replacing the
  main program's text, when the subprogram asked 2 seconds later (TEST74). One
  press of A answers both, wherever the focus is: both calls return in the same
  frame with `RESULT` 1 (TEST64, TEST72, TEST74).
- While both programs wait on the one dialog, the + button stops both, each the
  same way as it stops a single program, so an `ON BREAK GOTO` jumps. The
  earlier caller's own text never shows again.
- Each caller gets the one answer in its own form. A `DIALOG` with OK or a
  choice, behind a `DIALOG()` that waits for buttons, gets `RESULT` 1 for any
  button and 0 on a timeout. A `DIALOG()` that waits for buttons, behind a
  choice of two, returns -1 for the left one, and 0 for the right one with
  `RESULT` 1 (TEST76K).
- `ENVFOCUS` moves the focus a few frames later, and `ENVFOCUS()` gives the old
  answer until then: `ENVFOCUS 0` took one to three frames (it varied from run
  to run), `ENVFOCUS 1` one. An `ENVFOCUS` while the focus is moving is lost
  (TTEST37, TTEST38). TTEST37's wait of ten frames after `ENVFOCUS 0` took no
  frames at all: its `VSYNC 10` was already behind and returned immediately.
- The interpreter that gets the focus is framed for a moment by a yellow
  (255,255,0) border (a screen recording at 24 frames per second):
  - The border is three display pixels on the edge of its screen, one outside
    and two inside, with a black band two pixels wide inside that.
  - The subprogram's frame is its XSUBSCREEN rectangle and the main
    interpreter's frame is the whole display, every time the focus moves,
    including when `SUBRUN` starts and when a subprogram ends.
  - Both show fully for about 0.23 seconds and then fade out evenly over about
    0.25 seconds, drawn over what is under them.
  - If the other side's border starts before then, it replaces the first
    border immediately.

### A subprogram's screen

- `BACKCOLOR` on a subprogram's screen over the main one gives its color in
  full, and its alpha sets how much of the main screen shows through.
  `&H80FF4000` over black shows `&HFF4000`, and over a dimmed `&H77` green
  `&H40 + &H77*128/255`.
  - Alpha `&HFE` lets nearly all of the main screen through, `&H01` almost none,
    and alpha 0 leaves the screen clear.
  - With alpha `&HFF` the subprogram's screen replaces the main one where it is,
    and layers in modes 3 or 4 on it hide the main screen regardless of the
    alpha (TEST56).
  - The subprogram's drawing goes over its `BACKCOLOR`, the same as on any
    screen.
- A subprogram's screen is stretched smoothly regardless of `XSCREEN`'s mode,
  and keeps its aspect ratio in `XSUBSCREEN`'s rectangle: 480x272 in mode 1 on
  the whole display is scaled 2.647 times, starting at X 4 (TEST56).
- A screen wider than its rectangle gets bars above and below: 400x240 in a
  rectangle of 600x600 at 60 starts at Y 180 (TEST60).
- Mode 2 is smoothed the same way in a subprogram (TEST60).
- The bars take `BACKCOLOR`'s color plus the display times (1 minus its alpha),
  unless the alpha is 0. This was seen over a main screen that was not cleared,
  for 400x240 with `&HC0305070` and 1280x720 with `&H40305070` (TEST62).
- A layer in mode 3 makes the whole subprogram's screen opaque, also far from
  anything drawn on it (TEST62).

### `ENVSTAT` and `ENVLOCATE`

- `ENVSTAT OUT S,N,A,SL,C,L` after a fresh start of SmileBASIC, before any
  program, gives S 0, N 0, A 0, C -1 and L 0 (TEST64). TEST63, started another
  way, saw status 1 with line 386, of a system program that ran before it.
- A program that ends without an error sets the status and N to 0 and C to -1,
  and keeps A and L from the previous error: A 0 and L 0 after a fresh start,
  and A -1 and L 3 after a `Stack overflow` on line 3 (TEST63, TEST64).
- A new `SUBRUN` does not reset them either. TEST69K, started with `SUBRUN`
  after TEST69, read `ENVSTAT OUT` of the main program after its first clean
  run as S 0, N 0, A -1, C -1 and L 70, the A and L of TEST69's last error.
- `ENVSTAT OUT S,N,A,SL,C,L` after the main program's `Divide by zero` gives N
  7, A -1, SL 0, C the statement's character from the start of the program
  counting from 0, and L its line.
- After a `STOP` or a break by the + button, `ENVSTAT OUT S,N,A,SL,C,L` gives
  S 1, N 0, A 0, SL -1, C -1 and L 0 (TEST76K).
- `ENVSTAT 0` in a subprogram turns a stopped program's status 1 or 2 to 0, and
  `CONT` can no longer continue it.
- `ENVSTAT OUT` as TTEST39 saw it:
  - After `LOCATE 0,999`'s `Out of range`, it gives N 10 and A 1, the failing
    argument counted from 0; C is the statement's start.
  - A `GOTO` with nothing after it runs the lines before it and stops with
    `Type mismatch` (N 8) at its statement.
  - TTEST39 saw status 1, N 0, A, SL and C -1 after an error typed in DIRECT
    mode. The program had most likely stopped there: TEST69 and TEST70 show
    that an error in DIRECT mode leaves the fields unchanged.
  - While the editor shows, status 4 comes with N, A, SL, C and L all 0, and
    `ENVLOCATE` gives 0,0.
- A is the argument that the error message names, counted from 0: 1 after
  `Overflow in 1:3(FILL:2)`, 0 after `(HEX$:1)`, and -1 after `(PUSH)`,
  `(COPY)`, `(LEFT$)` or an operator's error (TEST66). TEST67 gave 1 after
  `(MID$:2)`, 2 after `(FILL:3)`, 0 after `(LOAD:1)` and -1 after
  `(UNSHIFT)`. It is 1 after `(LEFT$:2)`, 1 after `(BIQUAD:2)` and -1 after
  `SQR(-1)`'s unnamed `Out of range` (TEST71), and 1 after `(COPY:2)` and 2
  after `(COPY:3)` (TEST72).
- An error found while a line typed in DIRECT mode runs sets the status to 2
  and leaves N, A, C and L unchanged: from a fresh start's S 0, N 0, A 0,
  C -1, L 0 it gives S 2 and the rest the same, for `RENAME`, `DELETE` and
  `LOADG` errors in DIRECT mode and for `EXEC`'s `Can't use from direct mode`
  (TEST69, TEST70). After a program's `Out of range` with N 10, A 1, C 5 and
  L 1, the status stays 2 and the rest the same through `EXEC`,
  `Out of range`, `Syntax error` and clean lines typed after it. A clean
  line typed after an error in DIRECT mode leaves the status 2, and the next
  clean program sets it to 0 (TEST70, on a Switch and a Switch 2).
- A `Syntax error` typed in DIRECT mode leaves the status unchanged, the same
  as a program's compile error does: from 0 it stayed 0, with an empty line
  typed after it. After a `STOP` it stays 1, and `CONT` then continues the
  program and leaves 0 (TEST71).
- `?1/0` typed in DIRECT mode is a compile error too, because operations on
  constants are evaluated as the line compiles: it prints `Divide by zero`
  and leaves the status. From 0 the status stayed 0 through `?1/0`, an empty
  Enter, another `Syntax error` and `?1` (TEST72).
- `ENVSTAT OUT S,N,A,SL,C,L` after a compile error is set as for a run-time
  error, except for the status, which stays 0: S 0, N 3, A -1, C the
  character of the token it names from the start of the program, L its line
  (TEST62). A `Syntax error` in an assignment's value is placed at its `=`:
  for `A=(` on line 2, C is the `=`'s character, one past the `A` (TEST72).
  So is a `Divide by zero` of constants there, with status 0: C is 1 for
  `A=1/0` and 5 for `B=2:A=1/0`. `?7/0` gives the statement's start, 0, and
  so does a division found as the program runs: 4 for `Z=0:A=1/Z` (TEST74).
- Other compile errors are placed this way (TEST76):
  - An unknown `#constant` and a number literal that does not lex, at the
    token before them: `?#ZZ` and `IF #ZZ THEN` at the `?` or `IF`, `A=#NONE`
    at the `=`, so C is 1 for `A=#NONE` and 5 for `B=2:A=#NONE`. In a `CONST`
    or `ENUM` value they are placed at themselves, and `?&H` with no digit at
    the `&H`.
  - A function compiled in place given the wrong number of arguments, at the
    statement's start, also in an assignment: `Q=ABS(Z,Z)` at the `Q`. The
    run-time `Type mismatch` of `A=1+"A"` goes there too.
  - `Undefined variable`, `Duplicate variable` and a variable in a `CONST`
    value, at the name.
  - A `CASE` with no `WHEN`, at the token in its place: `CASE 1:ENDCASE` at
    the `ENDCASE`.
- `ENVLOCATE OUT X,Y`, which the manual does not document, gives the right
  edge and top of the main console's cursor cell on the display. With the
  400x240 screen 1200 pixels wide in its middle, the cursor at column C of row
  R is at 40+24*(C+1), 24*R (TTEST37).

## Memory

### `FREEMEM`

- `FREEMEM` is 134216720 at start, and the program and its numeric variables
  take none of it. `FREEMEM OUT A,B` gives the same in both, and
  `FREEMEM OUT A` gives it too (TEST71).
- `FREEMEM` counts a string of N characters as 2N rounded up to a multiple of
  128 bytes, plus 64 (TEST57). Strings of up to 24 characters take nothing.

  | Characters | 25, 64 | 65, 128 | 129 | 256 | 257 | 1000 |
  |---|---|---|---|---|---|---|
  | Bytes | 192 | 320 | 448 | 576 | 704 | 2112 |

- `FREEMEM` counts an array as its elements (Int 4, Real and string 8) rounded
  up to a multiple of 64 bytes, at least 128, plus 64 (TEST57). Arrays of up to
  64 bytes of elements take nothing.

  | Array | 17 ints | 33 ints | 49 ints | 1000 ints | 9 reals | 17 reals | 100 reals |
  |---|---|---|---|---|---|---|---|
  | Bytes | 192 | 256 | 320 | 4096 | 192 | 256 | 896 |

  `DIM A[100000]` takes 800064.

- `GARRAY` and `TARRAY` take nothing.
- Memory freed (emptying a string or array, a DEF's locals after it returns)
  does not come back immediately.
- Dropped strings and arrays are freed on demand (TEST52). A dropped string of
  100,000 characters stays counted in `FREEMEM` after `PRINT`, a loop, two
  `VSYNC`s and `WAIT 60`. Making 16 million characters 20 times in a row, with
  or without `VSYNC`, takes `FREEMEM` through 100662224, 67107728, 33553232
  and back to 100662224, and never fails: dropped memory is freed when the
  next request would not fit.

### `Out of memory`

- Length is limited by memory only: 63,000,000 characters, 33,000,000 ints and
  16,000,000 reals all work (TEST57).
- `Out of memory` comes when a request does not fit in `FREEMEM`'s 134216720
  bytes (TEST51).
  - `"A"*N` and `S$+S$` make 33,554,432 characters but not 67,108,864.
  - `ARRAY#` makes 8,388,608 reals but not 16,777,216.
  - Of five `DIM X%[8000000]`, the fifth fails with 6.2 million bytes free.
  - `RESIZE` and `COPY` grow to 16.7 million integers but not 33.5 million.
- `Out of memory`'s message names the command for functions and commands
  (`Out of memory in 0:68(ARRAY#)`, also `(RESIZE)`, `(INSERT)` and `(COPY)`)
  but not for `*`, `+` or `DIM` (`Out of memory in 0:54`). Its number is 11.

### Blocks that move

- `INSERT` and `RESIZE` growing an array need room for the new block next to
  the old one: `INSERT` of 16,777,216 integers into 15,728,641 fails though the
  resulting 130 MB would fit on its own, and `RESIZE` from 8 to 16 million,
  100 MB together, works (TEST51).
- `PUSH` is no different: with `DIM A%[15999999]` and `DIM F[8000000]`, 6216592
  free, the first `PUSH` works with `FREEMEM` unchanged, its block keeping the
  same size, and the second is `Out of memory (PUSH)` (TEST57). The block
  moves each time its size changes, and `UNSHIFT`, string arrays and strings
  (each 64 characters) do the same.
- `FREEMEM` overstates the memory available near the end, because a block that
  moved leaves a hole (TEST58). With 1296 bytes free beyond a block's size:
  - `UNSHIFT` on `DIM A%[100000]` (400064 bytes, 401360 free) gets to length
    100016 with 401296 free and then is `Out of memory (UNSHIFT)`.
  - `PUSH` on `DIM A$[100000]` (800064, 801360 free) gets to 100008 with 801296
    free, and then is `Out of memory (PUSH)`.
  - `PUSH` on a string of 512000 characters (1024064, 1025360 free) gets to
    512064 with 1025232 free, and then is `Out of memory (PUSH)`.
- `POP` moves a shrinking block too, and with no room for the new one leaves the
  length unchanged, with no error (TEST58). With `DIM A%[1000001]` and 1000016
  free, a `PUSH` and a `POP` stay in the block, the next `POP` leaves the length
  at 1000001 and `FREEMEM` at 1000016, and a `PUSH` after it gives 1000002.
  Twenty `POP`s in a row still leave 1000001. So a block stays only while its
  size is the same, not for 64 bytes more.
- With no room for the smaller block (TEST59):
  - `POP` still gives the last element, the same one each time.
  - `SHIFT` still gives the first, with the rest moved down and the last there
    twice.
  - A string's `POP` and `SHIFT` and an array of reals or strings behave the
    same way, and `ENVSTAT` shows nothing.
  - The size makes no difference: `DIM A%[1601]` with 5008 free, too little for
    its next block down of 6464, keeps its length and gives 901 each time with
    `FREEMEM` at 5008.
  - `REMOVE` moves the rest down and then is `Out of memory (REMOVE)`:
    `REMOVE D%,1` on 1000001 ints leaves the length, `D%[0]`=11, `D%[1]`=13,
    `D%[999999]`=901, and the last, `D%[1000000]`, read past the block, was 0
    the one time it was seen.
  - `RESIZE` to smaller is `Out of memory (RESIZE)` and changes nothing.
- A string constant takes no separate block until it is stored (TEST59). With
  100048 free, `X$=POP("ABC"+"X"*100027+"XYZ")` gives `Z` and leaves 100048, no
  error, while `SHIFT` of it, which needs a copy, is `Out of memory (SHIFT)`.
  Stored in `S$` the same constant takes 200256 bytes.
- `LOADV "DAT:Q",A%` of 2048 ints into `DIM A%[0]` with about 2 KB free by
  `FREEMEM` loaded them all, with no error (TEST76K).
- `A%=LOADV("DAT:Q")` makes the new array next to the old one: 10 MB of file
  with 4216464 free is `Out of memory (LOADV)` (TEST57).
- `LOADV "DAT:Q",A%` fills the array, gives no error and leaves `FREEMEM`
  unchanged (TEST57). If the file is longer, the array grows the way `RESIZE`
  does, unless it is a system array, such as the one `GARRAY` gives, and
  `A$=LOADV("TXT:Q")` makes its string the same way.
- `LOADV` as TEST58 saw it:
  - `S$=LOADV("TXT:Q")` of 1,000,000 characters (2000064 bytes) over a string
    of the same length is `Out of memory (LOADV)` with 1000016 free and works
    with 3000016 free.
  - `LOADV "DAT:Q",A%` of a file of 9 million integers into `DIM A%[8000000]` is
    `Out of memory (LOADV)` with 34000016 free and works with 38000016 free,
    leaving 34000016.

### Strings evaluated as the program compiles

- Constant strings are evaluated as the program compiles, so a string that is
  too long stops the program before any of it runs (TEST52, TEST57).
- `?LEN("A"*(1<<N))` works up to N=14 in DIRECT mode, then is
  `Out of code memory`. In a program it works up to N=20, then is
  `Out of memory`, in slots 0 and 1, with nearly all memory free. The same
  size made through a variable works.
- Each statement has 4 MiB to compile in. Every string evaluated on the way
  takes 24 bytes and its characters, rounded up to 8 bytes, from those 4 MiB
  and stays until the statement ends.
  - `"A"*1000000+"B"*1000000` is too long, as are `?"A"*1500000,"B"*1500000` and
    `A=LEN("A"*2097140)`, each `Out of memory` at its line (TEST58): each
    constant and the rest of the statement take from the same 4 MiB.
  - `A=LEN("A"*2097068)` compiles and prints, and 2097069 and 2097116 are
    `Out of memory` as it compiles (TEST59). That leaves 168 bytes for such a
    statement, none of them for the variable it stores to.
- A string then goes into the code, which is 32768 16-bit words in DIRECT mode
  and 4194304 in a slot, taking a word per character plus its length. Of three
  lines of `A=LEN("A"*2000000)`, the third is `Out of code memory` (TEST58).

## System settings

### `SYSPARAM`

- `SYSPARAM` takes names in lower case too.
- `SYSPARAM("TVMODE")` is 1 on a docked Switch 2 and 0 on a Switch 1 in
  handheld mode (TEST67). It follows the dock both ways while a program runs:
  on a Switch 2 taken out of the dock mid-program it went from 1 to 0
  (TEST70), and back to 1 when put back (TEST71). Put in the dock, a Switch 2
  read 1 within a frame, and `MAINCNT` continued with no gap of over 2 frames
  (TEST76K).
- `SYSPARAM`'s `LANG` is `ja` on a Switch set to Japanese. `SYSBEEP` starts at
  1, `KEYREPEAT` at 20 and 3, and `TOOL1` to `TOOL3` at `#TOOL/SBSMILE.PRG`,
  `#TOOL/GAHAKU.PRG` and nothing.
- `SYSPARAM` items that can only be read can also be read with `OUT`
  (`SYSPARAM "LANG" OUT L$`).
- `SYSPARAM` of an unknown name is `Illegal system argument name`. Setting an
  item that can only be read, or reading `KEYREPEAT` as a value, is
  `Wrong number of OUT arguments`.
- `SYSPARAM`'s unknown name is `(SYSPARAM:1)`, and it is checked before the
  counts: `SYSPARAM "NOPE",0` and `SYSPARAM S$ OUT Q0,Q1,Q2` with an unknown
  name are error 112. (Not yet seen on hardware.)
- `MOUSECURSOR` reads 0 again in a program run after one that set it to 1. `END`
  leaves it set, so DIRECT mode still reads 1.

### `HELPGET` and `HELPINFO`

- `HELPGET` and `HELPINFO` read the help of the console's language.
- `HELPGET` counts pages from 0. Colored runs come as position, length and an
  opaque color (`&HFFF39800`), in an array one run wide when no line has more.
- `HELPGET` and `HELPINFO` take names in lower case too.
- `HELPINFO OUT` names the word at the editor's cursor at that moment.
- `HELPINFO("NAME")` works as a function.
- `HELPGET` of a name the help does not have gives four empty arrays of strings
  (`TYPEOF` 5, `LEN` 0). (TEST63)
- `HELPGET "NOSUCHNAME",0 OUT ...` gives no error. (TEST62)
- `HELPINFO("NOSUCHNAME")` is no error either: it gives an array, so that `?` of
  it is `Type mismatch (PRINT:1)`. (TEST63)

### Other system commands

- `FONTINFO "A",16` is 16,1040 and `FONTINFO &H3042,8` 392,32. An empty string
  is `Can't use empty string`, a user-defined character
  `Can't use user-defined character`.
- `KEY` 1 to 5 start as `FILES`, `LOAD"`, `SAVE"`, `LIST ERR` and `RUN` (the
  settings can change them).
- `KEY` in a program changes the key for DIRECT mode for the rest of the
  session. After SmileBASIC 4 starts again, the key is the one in the settings
  (TEST71; the restart on a Switch 2).
- `KEY` ignores a string over 256 characters; `PUSHKEY` from 64 is
  `String is too long`.
- `XSUBSCREEN` and the undocumented `UISTATE` also exist in the main program:
  `XSUBSCREEN` does nothing and `UISTATE(0)` is 0. The main program's
  `XSUBSCREEN` looks at none of its arguments, so `XSUBSCREEN 0,0,2000,0` is
  no error there (TEST76).
- `UISTATE(2)` read 648 in DIRECT mode after TEST63's earlier parts had run, and
  0, 1, 3, 4, 5 read 0. (TEST63)
- `UISTATE(5)` is 0, and `UISTATE(6)` `Out of range (UISTATE:1)`. (TEST62)
- In a program, `UISTATE(0)` to `UISTATE(5)` all read 0, and setting item 5
  changes nothing (TEST71).
- `PERFEND()` gives microseconds; `PERFBEGIN` and `PERFEND` show no gauge on
  their own. Without a `PERFBEGIN`, `PERFEND` counts from when SmileBASIC
  started.

## Editor

- F4 after an error puts the cursor at the start of the statement that failed,
  syntax errors included.
- The editor shows 21, 28 or 43 lines of code, in both the 8- and the 16-pixel
  font.
- The editor colors TO, STEP, CONST and ENUM as reserved words, though the
  language specification does not reserve them, and OPTION as a command.
- Ctrl+Enter makes an empty line below the cursor's line and moves to it.
- Ctrl+K puts a quote at column 0 of each line, also before lines that are
  comments already. Only Ctrl+Shift+K looks for one.
- Finding ignores the case of letters.
- Ctrl+/ splits the display side by side, then one above the other, then joins
  it again. Both sides show the slot being edited at first.
- Ctrl+S asks for a name on the on-screen keyboard, titled "Slot 0: Write file",
  with the name the slot was saved as filled in.
- Ctrl+S while a `#` system project is the current one asks for no name. It
  plays a sound and shows an error titled "Error - Loading Files", with the
  lines "Failed to save the file." and "Project Name: #SYS" and only an OK
  button, and saves nothing (TEST76K).
- Ctrl+L opens a list of the project's files, with a way up to the projects.
- A list of SmileBASIC's words starting with what is typed shows from the first
  letter while typing in the editor. The program's own names are not in it.
  Enter while the list shows, with nothing picked, starts a new line
  (TEST71K, on a Switch 2).
- Tab inserts `SYSPARAM("TABSTEP")` spaces, and a new line keeps the indent of
  the line before (TEST71K, on a Switch 2).
