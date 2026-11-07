# SmileBASIC 4: what the manual leaves out

Behavior checked on SmileBASIC 4 for Nintendo Switch that the manual does not
describe, or that is easy to read the wrong way. The emulator follows these
facts except where [differences.md](differences.md) says otherwise. Items still
waiting for a check are marked `UNVERIFIED` in the source instead.

TEST and TTEST numbers name check programs kept outside the repository;
[hardware-tests.md](hardware-tests.md) describes the method. Each fact keeps its
test as evidence, and says "on a Switch 2" or "on Ryujinx" where the machine
matters. A real Switch outranks Ryujinx where the text says so.

TEST62 and TEST63 ran on 4.4.9 on a Switch 2. What TEST62 shows had been read in
SmileBASIC 4 first, and so had most of what TEST63 shows. "By SmileBASIC 4" means expected of SmileBASIC 4. "(Not yet seen on hardware.)" marks what
was expected of SmileBASIC 4 (version 4.4.8) and SmileBASIC 4 and has not run.

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
- [Direct mode](#direct-mode)
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
  - [Strings worked out as the program compiles](#strings-worked-out-as-the-program-compiles)
- [System settings](#system-settings)
  - [`SYSPARAM`](#sysparam)
  - [`HELPGET` and `HELPINFO`](#helpget-and-helpinfo)
  - [Other system commands](#other-system-commands)
- [Editor](#editor)

## Syntax and compiling

### When errors appear

- An error message goes on from the cursor, on the line the program left it:
  after `?"V";` an `Overflow` shows as `VOverflow in 1:3` (TEST65). A line
  typed in direct mode does the same (TEST63).
- An error in some builtins names the command right after the place, with no
  space between: `Overflow in 1:3(FILL:2)`, `(PUSH)`, `(COPY)`, `(HEX$:1)`,
  `(CHR$:1)` and `(MID$:2)`. The number counts arguments from 1. `LEFT$` with
  2147483648 as its second argument shows `(LEFT$)` with no number. Every
  builtin seen so far names itself, whatever the error: `MID$(S$,-1,1)` is
  `Out of range in 1:2(MID$:2)`, `FILL M%,0,5` on three elements `(FILL:3)`,
  `LOAD "GRP0:X"` `Illegal format (LOAD:1)` and `LOADV "TXT:A",S$` `Wrong
  number of arguments (LOADV)` (TEST67). A value that fails as it goes into
  the array gets no number: `UNSHIFT M%,2147483648` is `Overflow (UNSHIFT)`,
  as `PUSH` and `COPY` are (TEST66, TEST67). Operators
  and plain stores name nothing: DIV, MOD, the bit operators, unary minus and
  `M%[0]=V` give `Overflow in 1:2`, and DIV or MOD by 0 `Divide by zero in 1:2`
  (TEST65, TEST66).
- `Wrong number of arguments` comes when a command or function gets a number of
  arguments none of its forms takes (`LOCATE` with five, `TOFS 0,30`). The call
  finds this as it runs, not as the program compiles, so the lines before it
  run.
- `?VSYNC()` is `Wrong number of OUT arguments (VSYNC)`, and `FREEMEM 1` and
  `?FREEMEM(1)` are `Wrong number of arguments (FREEMEM)`. (TEST62)
- A call to a name defined nowhere compiles, and is a `Syntax error` only when
  it is reached: the name may be a `COMMON DEF` of a slot that `EXEC` runs
  later.
- `GOTO` and `GOSUB` to a label that is not there are `Undefined label` only
  when they run.
- `GOTO` or `GOSUB` with nothing after it runs the lines before it and stops
  there with `Type mismatch` (TTEST39).
- Operations on constants are worked out when the program compiles, so `?"A"*-1`
  or `?1/0` stops the program before it runs, even on a line that is never
  reached.
- An unknown `#constant` is `Undefined constant`.

### Statements, literals and names

- A variable's suffix only sets its starting value. `A%=1E10` keeps the real,
  `A$=1` stores a number and `A%="X"` a string.
- The literal `2147483648` is a real.
- `CONST` and `DATA` take `HSV` as well as `RGB`.
- `OPTION STRICT` holds from where it stands. A variable used before it without
  `VAR` can be used after it, and a DEF before it is not checked.
- Statements need no colon between them where one cannot go on:
  - A `PRINT` item ends where no `;` or `,` follows it: `?"A" B=1` prints A,
    then sets B.
  - `NEXT` takes a loop variable only if a name follows (`NEXT?I`).
- `T?` is short for `TPRINT`.
- A label used as a value keeps its case (`A$=@Lp` is `"@Lp"`). As a place to go
  to, case does not matter.
- `THEN @L` may be followed by more statements on the line, which belong to the
  branch.
- `ELSEIF A GOTO @L` takes GOTO in place of THEN.
- A constant's name may start with a digit, as a label's may (`#2P`).
- `#` after a number makes it a real (`0#`).
- `\` joins its line with the next, and the rest of its line is left out:
  `?1\ XYZ` and `+2` on the next line prints 3.
- A comment line after a `\` line end is no blank line: `?1\`, `'COMMENT`, `+2`
  is a `Syntax error` at the `+` (TEST62).
- `_` in a group of digits must lead to a digit: `?&HFF_` and `?&H_` are
  `Syntax error`, and so is `?1_000`, as a decimal number takes none (TEST60).
- A string with no closing quote ends with its line (TEST62).
- `NEXT` (also `NEXT I`), `WEND` and `ENDLOOP` inside an `IF` or another block
  of a loop go on with the loop's next turn: `IF A THEN NEXT`.
- `NEXT J` on `FOR I` is not checked: `J` stays as it is (TEST62).
- `VAR A` without a value only declares: in `FOR I=0 TO 1:VAR A:?A:A=5:NEXT` it
  reads 0, then 5 (TEST62).
- `IF 1 THEN ?1 ENDIF:?2` prints both (TEST62).

### `DEF`, `OUT` and `RETURN`

- A DEF's parameters may be written `A[]`. Passing a number for one and indexing
  it is `Type mismatch` on the indexing line.
- A function may be called as a command with one `OUT` variable for its result:
  `ABS -3 OUT A`, `SPCHR 0 OUT D`, `SPSET 242 OUT IX`, `HSV 120,255,255 OUT C`.
- An `OUT` variable a `DEF` never sets gives the omitted value (`TYPEOF` 0),
  whatever its suffix. Inside the `DEF` it reads as that too.
- A `DEF` sees a global only if top-level code above the `DEF` names it, as the
  program compiles in one pass. Otherwise the name is the `DEF`'s own.
- A `DEF` with an `OUT` may be called as a function, giving the `OUT` value
  (`DEF F A OUT R` as `?F(3)`). A `DEF` that returns a value may be called with
  one `OUT` (`T OUT X`). One with two `OUT`s called as a function is
  `Wrong number of OUT arguments` when the call runs.
- `RETURN` takes a value only in a function. In a `DEF` without brackets, also
  one with an `OUT`, what follows `RETURN` is the next statement, never run, and
  it has to compile as one:
  - `RETURN 5`, `RETURN A+1` and `RETURN A*2` are `Syntax error`s when the
    program compiles, at the `5`, the `+` and the `*`, as is `RETURN F()` at the
    `F` (TEST60, TEST62).
  - `RETURN F(1)` compiles, as a command with a value in brackets (TEST62).
  - `RETURN X,Y` compiles too, as a command X, in `DEF G A OUT X,Y`
    (KRB_OGIRI_SMBOMB).
  - `RETURN UNDECLARED` compiles too, under `OPTION STRICT` (SMILEBOY).
- A function that ends without `RETURN`, or leaves by a bare one, gives no
  value: `TYPEOF` 0 (TEST62).
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

  `RESTORE "PUSH"` itself goes on past that: 172713 of them pass, and 180000
  stop on the loop. After the 172713, `A=F(1)` of a one-line `DEF F(P)` stops
  on its own line, while `?N`, `X=1+2` and a bare `PRINT` run and the program
  goes on (TEST67).

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
- `DIV` and `MOD` check a real operand as a store into an int array does, before
  dropping the fraction. `2147483647.5 DIV 1`, `-2147483648.5 MOD 10`,
  `2147483648 DIV 2`, `7 DIV 2147483648` and `INF DIV 1` are `Overflow`, and
  `NAN MOD 7` is 0 (TEST66).
- Shift counts are taken modulo 32: `1<<33` is 2, `1<<-1` is -2147483648.
- `MOD` takes the sign of the dividend.
- `ROUND` rounds halves away from zero.
- Reals print with eight decimals, trailing zeros removed, never in exponent
  form: `1E20` prints all 21 digits, `1/3` as `0.33333333`, `-1E-9` as `-0`.
- `1E308*10` less itself prints `nan` (TEST62).
- `VAL` takes underscores in `&H` and `&B` digits as a program does:
  `VAL("&H_FF")` and `VAL("&HFF_")` are 255. `VAL("1_000")` and `VAL("&H_")` are
  0 (TEST56).
- `VAL(" 12 ")` is 0: spaces make the text not a number. `INPUT` asks again
  (`?Redo from start`) for the same reason.
- `LOG(0)` and `ASIN(2)` are `Out of range (LOG:1)` and `(ASIN:1)` (TEST62).
- `SQR` below 0 and `ACOS` past -1 to 1 are `Out of range` on their argument, as
  TEST62 saw for `LOG` and `ASIN`. (Not yet seen on hardware.)

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
- `HSV` with one argument is `Wrong number of OUT arguments`, as it is read as
  the `OUT` form.
- `HSV`'s `OUT` form needs at least three results too, and fewer is
  `Wrong number of OUT arguments`. S and V may be left empty, which is what the
  4.4.9 notes mean: `HSV C OUT H,,`.

### Random numbers

- `RND(0)` is 0 and takes no number from the series. A negative maximum is
  `Out of range`.
- Each random series is TinyMT32 with its standard parameters, seeded with
  `RANDOMIZE`'s value as a 32-bit number (-1 is the same seed as &HFFFFFFFF),
  whatever the series number.
- `RND(n)` is the next number modulo n.
- `RNDF()` takes two numbers and makes 53 bits of them as TinyMT's `res53` does.

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
  - A format of 1025 plain characters is `(FORMAT$)` with no number, as is `%%`
    over the limit (TEST57).
  - Of 1, `%1024D` works and `%1025D` does not. `%.1000F` works (a length of
    1002) and `%.1023F`, which would be 1025 long, does not (TEST51, TEST52).
  - `%F` of 1.5 gives 1023 characters at `%.1021F` and `%1023F`. `%.1022F` and
    `%1024F` are `String is too long (FORMAT$:2)` (TEST58): it
    needs one character of room more than it gives.
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
- `%X` and `%B` go the same way as `%D`: `%.4X` of 255 is `00FF`, `%.4B` of 5 is
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
  `DIM A[0,0]` and `DIM A[0,3]` are `Subscript out of range`.
- An array holds nothing (`TYPEOF` 0) until its `DIM` runs, as when a DEF reads
  it before.
- `DIM A[,]` compiles, and is `Type mismatch` when it runs.
- An int array takes a real by truncating it toward zero (-3.9 gives -3, -0.5
  gives 0), but checks the real against -2147483648 to 2147483647 first. So
  `M%[0]=2147483647.5`, `2147483647.9` and `-2147483648.5` are `Overflow`, as
  is anything farther out (TEST65). A `%` variable keeps any real.
- A NaN stored into an int array is 0. `FILL`, `PUSH`, `COPY` from a real array
  and array literals (`DIM M%[]=[X]`, `DIM M%[2]=[X]`) check a real the same
  way, and truncate one in range: `PUSH M%,2147483647.5` is `Overflow` (TEST66).
- `SORT` keeps equal keys in their order. `RSORT` gives `SORT`'s order turned
  round, so equal keys come out reversed.
- `SORT` orders strings by character code: `""`, `"A"`, `"AA"`, `"B"`, `"a"`,
  `"b"`.
- `RESIZE` of a 2D array takes every size, the later ones as they are:
  `RESIZE A,3` on `DIM A[2,3]` is `Wrong number of arguments`, and changing a
  later size `Illegal function call`.
- `RESIZE A,0,3` on `DIM A[1,3]` leaves `LEN` 0 and `DIM(A,0)` 0, and `SAVEV`
  and `LOADV` bring that back (TEST51).
- `LOADV` loads array files with a zero in any size and keeps the sizes: `[0,3]`
  and `[3,0]` both give `RESULT()` 1 (TEST51).
- `REMOVE` with a count of 0 removes nothing and is no error, also on an empty
  array and at the position just past the end.
- `ARYOP 6` (clamp) takes the lower bound first: 5 between 9 and 1 gives 9, and
  a NaN stays. (TEST63)
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
  5. Source channels neither 1 nor the destination's: `Type mismatch`, no
  number, as are string arrays that get this far.
- `RINGCOPY` on `A[4]` and `B[3]`, as TEST58 saw it:
  - `RINGCOPY A,0,B,-1,1` is `(RINGCOPY:4)`.
  - `A,0,B,4,0`, `A,0,B,1,3` and `A,0,B,0,-1` are `(RINGCOPY:5)`.
  - `A,0,B,3,0 OUT E` gives 0.
  - A number as the first or the third argument is `Type mismatch (RINGCOPY:1)`
    or `(RINGCOPY:3)`.
  - With five arguments too the destination offset wraps: with B=1,2,3,
    `RINGCOPY A,9,B,1,2 OUT E` leaves 0,2,3,0 and E=3, and `A,-1,B,1,2` leaves
    3,0,0,2 and E=1.
- `RINGCOPY`'s destination offset wraps round with a floored modulo, negative
  too: `RINGCOPY A,-1,B OUT E` on `A[4]` and B=1,2,3 leaves 2,3,0,1 and E=2.
  `OUT` is (start + count) modulo the length (TEST57).

### `SPVAR`, `TVAR` and `INC`

- `TVAR` cuts a real key down to an integer: keys 1.5 and 1 are the same.
- `SPVAR` and `TVAR` keep an array given to them as the same array, not a copy:
  what is changed through one shows through the other.
- `INC` adds to a string where it is, as `PUSH` does: `INC T$,C$` on a DEF's
  parameter adds to the caller's string.

## Order of evaluation

SmileBASIC 4 works things out right to left, which shows when the parts have
side effects (a DEF that prints, `RND`, `INKEY$`, `BEEP`):

- Operators evaluate the right side first. `F(1)+F(2)*F(3)` calls `F(3)`,
  `F(2)`, then `F(1)`; `INKEY$()+INKEY$()` puts the first key typed last.
- Arguments of functions, commands and DEFs, and array indices, go last first.
  In `A[F(1),F(2)]=F(3)` the value comes before the indices.
- `PRINT` works out every item, last first, before it prints any of them:
  `?P(1);P(2)` with a `P` that prints shows `<2><1>12`.
- `FOR` works out `STEP`, then `TO`, again before every test, so they are called
  once more at each `NEXT`.
- Array literals and `DIM` sizes go last first too.
- `SWAP` works out each element's place once, the second first.
- `INC` and `DEC` work out the amount first, then the place, once.
- `CALL` works out its arguments before the name.
- `&&` and `||` still go left to right and stop early.

## DATA

- `SPANIM` with a `"@label"` reads its DATA with `READ`'s own place: it restores
  to the label, and `READ` goes on after the values it took.
- `COPY` from a `"@label"` leaves `READ`'s place alone.
- `BGMSETD`'s MML may end with the program's last DATA instead of a 0, and any
  number ends it: in `"..",5,"..",0` the strings after the 5 are not played.
- `RESTORE @LABEL` to a label that is not there is `Undefined label` only when
  it runs.
- `RESTORE "POP"` with nothing kept by `RESTORE "PUSH"` is `Stack underflow`.
- `RESTORE "PUSH"` can keep a thousand places, and many more: see the stack
  under `DEF`, `OUT` and `RETURN`. A subroutine that keeps a place must take it
  back before `RETURN`, which is otherwise `Use RESTORE POP before RETURN`.

## Timing

### `VSYNC` and `WAIT`

- `VSYNC n` waits until n frames after the frame the previous `VSYNC` waited
  for, not n frames from now. When that frame has gone by, it returns at once
  and counts from then: a second behind, ten `VSYNC`s take nine frames, while
  one `VSYNC 200` takes 140. After a slow `ACLS`, `VSYNC:VSYNC` takes one frame.
- `VSYNC` of 0 or less does not wait, and counts on from the moment it runs.
  (Not yet seen on hardware.)
- `WAIT`, `WAIT 0` included, starts `VSYNC`'s count anew; waiting in `INPUT`
  does not.
- `RUN` leaves `VSYNC`'s count where it was, so the first `VSYNC` of a program,
  `VSYNC 5` too, finds its frames gone by: `T=MAINCNT():VSYNC 5:?MAINCNT()-T` as
  a program's first line prints 0 (TEST62, on a Switch 2). A subprogram's first
  `VSYNC` finds the same, and a program waiting by a lone `VSYNC 60` at its
  start does not wait.
- `XSCREEN` holds the program until the frame job has taken the new mode,
  without touching `VSYNC`'s count, so a `VSYNC 1` right after it goes straight
  on (TEST60). The program goes on inside that job, past its animation step and
  with `MAINCNT` still to rise.
- `ACLS:XSCREEN 400,240,2`, then a `SPANIM` and `VSYNC 1:SPOFS` in a loop, in a
  program that has not waited yet, shows nothing on the first two rows and the
  first step on the third. The second `VSYNC 1` only sees that job end (TEST62
  on a Switch 2, as TEST56 did on Ryujinx).
- `ACLS` holds the program for the frame job as `XSCREEN` does, and `MAINCNT`
  has yet to rise when the program goes on (TEST63):
  - `T=MAINCNT():ACLS:?MAINCNT()-T` and the same with `XSCREEN 400,240,2` print
    0, with a `VSYNC` before or not, and `ACLS:ACLS` prints 1.
  - A `WAIT 1` or a `VSYNC 1` right after takes that rise, and the next one a
    frame:
    `VSYNC:XSCREEN 400,240,2:T=MAINCNT():WAIT 1:U=MAINCNT():WAIT 1:?U-T;MAINCNT()-U`
    prints 1 1, and so with `VSYNC 1`.

### `MAINCNT`

- `MAINCNT` and `MILLISEC` need their brackets: without them they are plain
  variables, always 0 to begin with.
- A subprogram's `MAINCNT` is the main interpreter's, not counted from
  `SUBRUN`: two dialogs closed by one press read the same `MAINCNT` on both
  sides (TEST64).
- `MAINCNT` follows real time while the program is busy: 180 in 3 seconds of a
  loop with no `VSYNC`. When the frame job is held up, it falls behind real time
  with it: 600 sprites of 64x64 scaled by 21 on a Switch 2 gave 49 in 4077 ms,
  one `VSYNC` to each (TEST60).
- A program runs this much in 60 frames (4.4.9, timed with `MAINCNT()`). No
  program needs that speed: the emulator runs code as fast as it can.

  | Times round the loop | Switch | Switch 2 | Ryujinx |
  |---|---|---|---|
  | `WHILE MAINCNT()-T<60:INC N:WEND` | 2395924 | 5962406 | 2837664 |
  | `FOR J=1 TO 100:X=X+J*2:Y=Y XOR J:NEXT` | 21371 | 52145 | 29141 |

- A DEF call costs about five `GOSUB`s. On a Switch 2, in ns each with the loop
  around it taken away (TEST66):

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

- Single commands, in ns each with the loop around them taken away (TEST67,
  4.4.9; the Switch ran handheld, the Switch 2 docked, `GCLIP` at the whole
  page):

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

  `ON` costs the same with 2 labels or 30 and with the first label or the
  last. Blending (`#G_ALPHA`) makes a 128x128 `GFILL` 15 to 19 times slower. A
  program busy for 60 frames loses no measurable time to 4000 sprites on show,
  rotated and scaled 4 times or hidden.

## Animation

- `SPANIM`, `TANIM` and `FADE color,time` take their first step at the first
  update of the frame job after the command, the three together, whatever the
  program was doing. After a `VSYNC` that finds its frame gone by, or a long run
  of code, the next `VSYNC 1` goes straight on and it takes a second one to see
  the step.
- The first step comes about a millisecond before `MAINCNT` rises on a Switch 2
  (TEST60).
- `FADE()` still gives the start right after `FADE`.
- Animations also play in direct mode, and go on while the editor is open.
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
- An animation key that lasts 0 frames is `Illegal animation data`.
- `SPANIM` keys for XY, Z, UV and I outside -32768 to 32767 are
  `Out of range (SPANIM)`, and `TANIM` and `ANIMDEF` go through the same code. R
  has no such limit (TEST60).
- `SPANIM id,n` and `TANIM id,n` with a number `ANIMDEF` never set do nothing.
- Setting what an animation changes stops it, and `SPCHK` no longer shows it:
  `SPOFS` during an `"XY"` animation, `SPCOLOR` during `"C"`, `SPCHR` during
  `"UV"` or `"I"`, `SPROT` during `"R"`, `SPSCALE` during `"S"` and `SPOFS`
  during `"Z"`.
- Spaces after an animation target's name are left out (`"S "`).
- `TCHK` reports nothing for `"I"` and `"UV"`, which text screens ignore.
- `SPCHK` and `TCHK` give 0 while the animations are stopped.

## Input

### Buttons

- `DIALOG` with buttons closes when A is let go, not when it goes down. Once it
  has closed, `KEYBOARD` shows no Enter press: `KEYBOARD(40,2)` and
  `KEYBOARD(40,0)` stay 0 over the next 6 frames, whether Enter or A closed it.
- `DIALOG(text,8)`, which waits for A, closes when A goes down and returns
  81928: A's bit with `#B_RANY` and `#B_ANY` (TEST64).
- The A that closes a `DIALOG` is gone for the program after it (TEST64):
  - With OK or Yes and No, `BUTTON` shows no release of it.
  - With a wait for A, `BUTTON(0)` shows it held, `BUTTON(0,-1,2)` no press,
    and the release comes when it is let go. `INKEY$` gets no `CHR$(13)` for
    the press, but A held on repeats into it: 20 frames after the press and
    then every 3.
- `DIALOG` covers the whole screen, so nothing else shows while it's open, and a
  screenshot of it shows only the dialog.
- `BREPEAT` starts at 20,3 for buttons 0 to 7 and at 15,4 for 8 to 16. It
  doesn't touch what controllers type for `INKEY$`.
- `BREPEAT` repeats a held button at its start frame and then every interval
  plus one frames: 20,3 fires at 0, 20, 24, 28...
- `BREPEAT`'s negative values are `Out of range`. (Not yet seen on hardware.)
- `BREPEAT` takes a start of 0 as 1: `BREPEAT 5,0,3:BREPEAT 5 OUT S,I` gives 1
  and 3. (TEST62)
- `BUTTON(0)`'s bits include `#B_RANY`, `#B_LANY` and `#B_ANY` (bits 14 to 16).
- `BUTTON()` without arguments is `Wrong number of arguments`.
- `#B_X`, `#B_B`, `#B_Y`, `#B_A` are 0, 1, 2, 3. (TEST62)

### Keyboards and controllers typing

- A USB keyboard's keys press no controller button: a program that takes letters
  from `INKEY$` and delete from `BUTTON`'s Y types A as any letter.
- Controllers type into the same queue as the keyboard, and the manual
  agrees on what it lists.
  - `INKEY$()` gives `CHR$(13)` for A, `CHR$(8)` for Y and the arrows for the
    +Control Pad, as the manual says.
  - The left stick past 0.3 gives Page Up and Down (`CHR$(17)`, `CHR$(18)`) and
    Home and End (`CHR$(2)`, `CHR$(3)`). The manual leaves this out.
  - L or R act as Ctrl. With left or right on the pad they give `CHR$(2)` and
    `CHR$(3)`, and with the stick up or down the same two. The manual leaves
    this out.
  - B types Copy, L or R with B Paste and with Y Delete Line. `INKEY$` passes
    over these, and a line being typed acts on them.
  - `INKEY$(1)` drops whatever controllers typed.
  - The first assignment that holds gives the frame's one key. It comes at the
    press, 20 frames later and then every 3, for the pad and the stick alike
    (TEST64).
  - A second input going down meanwhile types nothing until all are let go.
    Going back to the first one goes on with its repeats, every 3 frames.
  - TEST67 on a Switch 1 and a Switch 2 saw the left stick right give
    `CHR$(3)` at the push, 20 frames later and then every 3, with `BUTTON` 0.
    L or R with the pad's right gave `CHR$(3)` too. L, R, and L and R held
    together type nothing, for 323 frames at least. With the stick turned
    round meanwhile, only `CHR$(2)` came, as Ctrl with the stick up or left
    gives.
- Ctrl+C on a USB keyboard stops the running program (TEST67: the step that
  asked for it ended there on both consoles, and the test went on with its
  next part).

### USB keyboards

TEST68 on a Switch 2 (4.4.9) with a Korean-layout USB keyboard:

- `KEYBOARD` reads the Han/Eng key as usage 144 (LANG1) and Hanja as 145
  (LANG2). That keyboard also sent Right Alt (230) after Han/Eng and Right Ctrl
  (228) after Hanja. Some Korean keyboards send only Right Alt and Right Ctrl
  for these keys. Right Alt sets bit 8 of `KEYBOARD 4 OUT R,M`'s modifiers and
  Right Ctrl bit 4.
- SmileBASIC 4 keeps a Caps Lock of its own, on at start: `INKEY$` gives "A" for
  the A key and "a" for Shift+A. G, K and S give "G", "K" and "S".
- Han/Eng doesn't switch what `INKEY$` gives: G, K and S typed after it still
  give "G", "K" and "S", and the key itself types nothing.
- A held key repeats into `INKEY$` 19 frames after the press, then every 3
  frames: Shift+A held gave "a" on the frame it went down and again 19, 22,
  25... frames later. A controller's keys repeat after 20 frames instead.

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
  of them take ten frames (TEST60).
- `PUSHKEY` of a string of 64 characters is `String is too long (PUSHKEY:1)`; 63
  goes (TEST60).
- `PUSHKEY &H10FFF` goes and `&H11001` is `Out of range` (TEST62).
- `INKEY$` after `PUSHKEY &H10009` (HOME) gives one character of code 2, and
  after `PUSHKEY &H10000` (up) code `&H1E` (TEST62).
- Keys pass through two buffers of 128, each losing its oldest when full. Typed
  and pushed keys wait in the first, and each frame takes up to 32 to the
  second, which `INKEY$` and the line being typed read.
  - Five `PUSHKEY`s of 63 in a row, with 315 keys, leave 128 to read: keys 32 to
    95, 124 to 155 and 187 to 218. Keys 219 on come in the frames after, with
    the first buffer's rest (TEST60).
  - At the prompt, five `PUSHKEY`s of 60 lose keys the same way (TEST60).
- Typed keys wait in the buffer until something reads them:
  `WAIT 180:?INKEY$();INKEY$();INKEY$()` shows what was typed during the wait.
- `INPUT` and `LINPUT` drop every key waiting when they start, `PUSHKEY`'s
  included: `PUSHKEY "7":INPUT A$` waits with an empty line.
- The prompt drops waiting keys as it starts, after a typed line as after a
  program: `PUSHKEY "?1"+CHR$(13)+"?2"+CHR$(13)` typed at the prompt runs
  neither (TEST62). From a subprogram, typing into a prompt already there, they
  arrive.
- `PUSHKEY` of keys without Enter is dropped at the prompt too, as with Enter: a
  line typed after `PUSHKEY "?7:"` runs as it is. (TEST63)
- `INKEY$` gives back what `PUSHKEY` pushed of Hangul syllables, the jamo
  U+3131, U+E800 and surrogate pairs unchanged, one UTF-16 unit a call: the
  pair U+D83D U+DE00 takes two calls (TEST68).

### `INPUT`, `LINPUT` and the line being typed

- `INPUT`'s question mark is followed by a space; with a comma after the guide
  there is neither.
- `INPUT` leaves out extra values typed. After `?Redo from start`, the line is
  typed again on the next line, without the prompt.
- `INPUT` and `LINPUT` take any string expression as the guide, a label among
  them (`LINPUT@G;A$` shows `@G`). A string variable is a guide only with `;`.
- `INPUT A,B` given `1,` takes A=1, B=0 at once. Given `1,2,3` it leaves the 3
  out, not looking at it. (TEST63)
- `INPUT VAR("A$")` and `N$="A$":INPUT VAR(N$)` both take a string. (TEST63)
- `INPUT` selects nothing: Shift with Left or Right moves the cursor, and
  Backspace and Delete take one character (TEST56). `LINPUT` runs with the same
  setting in SmileBASIC 4. Direct mode's line does select, cut, copy and
  paste.
- `INPUT A$`, ABCD, Shift+Left twice, X gives `ABXCD`: on `INPUT`'s line Shift
  only moves.
- Tab on a line types one space (TEST60).
- A selection stays after Enter (TEST60).
- Copy on a line ends the selection and keeps the text, so what is typed next
  goes in at the cursor (TEST60).
- A line holds 256 characters whatever the console's size; a character typed on
  a full one is dropped (TEST60).
- A line typed in direct mode or for `INPUT` or `LINPUT` holds 256 characters,
  whatever they are and whatever the console's size. A further character is
  dropped, also over a selection, which then stays. Pasting that would not fit
  (counting a selection as still there) pastes nothing. (Not yet seen on
  hardware.)
- Enter goes on from the row under the cursor, wherever on the line that is. A
  line that exactly fills its row leaves an empty row before what comes next, as
  the cursor is on the next row already. With the cursor on the first row of a
  longer line, what comes next prints over the second (TEST60).
- Typing doesn't stop at the end of a row. The line wraps at the console's width
  and goes on over as many rows as it takes, the console scrolling as it goes,
  and the last row stays empty (TEST60 part A, on a real Switch with a USB
  keyboard: `ACLS:XSCREEN 320,180,1,2:TSCREEN 4,8,26,12:LOCATE 0,9`, then about
  60 plain letters).
- A line keeps rows free under its cursor according to the hint bar: at 1280x720
  with 8x8 characters, typing from row 85 leaves the cursor on row 86; at
  640x360 from row 40, on row 42 (TEST60).
- Enter ending a line scrolls the console so that the cursor stays above its
  last row; `INPUT`'s and `LINPUT`'s guides, errors and `OK` do the same. What a
  program prints can still reach the last row. (Not yet seen on hardware.)
- The hint bar at the bottom covers more rows on a screen whose rows are 16
  display pixels high or less (of the 720): 2 rows stay free at 360 rows of
  pixels, 3 at 720 with 8x8 characters. (Not yet seen on hardware.)
- `INSPECT` scrolls as Enter does while it prints, keeping the cursor above the
  last row, which has been seen (`TEST19`, on the usual screen). The rest of
  this scrolling comes from the code.
- Enter on an empty line of direct mode prints `OK` (TEST60), and leaves the
  empty line above it.
- At the prompt the controller types as `INKEY$` reads it (TEST64, by hand). A
  is Enter, Y deletes as Backspace does, and the +Control Pad moves as the
  arrow keys do. On an empty line Y and B do nothing. A USB keyboard plugged
  in or out, or the software keyboard shown, changes none of this.
- Typing over a selection in overwrite mode writes the character and moves on
  first, then deletes the selection: one made rightwards takes the character
  with it, and one made leftwards keeps it. (Not yet seen on hardware.)
- Copying a selection in direct mode drops the selection; cutting too. (Not yet
  seen on hardware.)

### The line's cursor

- `INPUT`'s cursor is a bar 2 pixels wide and 8 tall at the left of the cursor's
  cell, opaque white whatever `COLOR` is. `COLOR &H80FFFF00` over a character
  changes only the bar's two columns, and the rest of the cell is left as it
  was. It alternates with nothing (TEST50).
- Characters typed at `INPUT` and then deleted leave their cells empty: whatever
  the console showed there before does not come back (TEST50).
- The per-cell inverting flag (color times 1 minus the glyph's alpha) is not
  what draws `INPUT`'s cursor (TEST50).
- The cursor blinks by a count kept with the frame count `MAINCNT` reads: 20
  frames on, 20 off. TEST56's two shots, off at frame 72 and on at 299 of the
  line, fit that with the count 1 to 3 frames behind `MAINCNT`, and 15/15 only
  at 3 ahead, which is weak support; the code says 20.
- Insert toggles overwriting on `INPUT`'s line, and the cursor is then a block
  over the whole cell: 24 wide at scale 3 (TEST56).
- The cursor's scale is the text's, `TSCALE` and `LMATRIX` included (TEST56).
- The cursor is drawn right after its own screen's cells, under a sprite in
  front, and it replaces what its layer holds, alpha too. As a layer's alpha is
  what shows through, on a composed layer it adds its white to the frame rather
  than covering it: `&H203080` under `LAYER 2,1,&HFFC0C0C0` shows `&HE0F0FF`
  (TEST56).

### Controllers, vibration and the IR camera

- `CONTROLLER(0)` read 2, `CONTROLLER(1)` 1 and `CONTROLLER(2)` 0.
  `CONTROLLER 2 OUT A,B,C` gives 0 for all three: no colors on a controller not
  connected. (TEST63)
- With a Pro Controller and the Joy-Cons off the console, `CONTROLLER(0)` and
  `CONTROLLER(1)` read 2. `CONTROLLER 0 OUT` gives 2 with colors 0 and 0, and
  `CONTROLLER 1 OUT` 2 with `&HFF232323` and `&HFFA0A0A0`, the Pro Controller's
  own. (TEST64)
- `IRSTART` without `XCTRLSTYLE`'s IR flag is `IR Camera is not available`.
- `TCPIANO` without Toy-Con set up is `Illegal control style`.
- `IRSTATE` starts at 0.
- `IRREAD` in mode 1 is `Illegal IR Camera mode`. (Not yet seen on hardware.)
- `IRREAD` on a camera never started is `IR Camera is not available (IRREAD)`.
  (TEST62)
- Vibration patterns have lengths of their own, from 3 frames (57) to 115 (23);
  34 to 43, 47 and 48 go on past two seconds, 40 past five. The emulator keeps
  the measured table.
- `VIBRATE()` reads back what was asked for, also where nothing vibrates. After
  `VIBRATE 1,40` it is 2621480 (40 on the right times 65536, plus 40 on the
  left) and `VIBRATE(1,0)` 40; with frequencies it is -1, still a second later.
  `VIBRATE` alone clears it.

## Screens and text

### `ACLS`, `XSCREEN` and `TSCREEN`

- `ACLS` also resets `SPPAGE`, `SPLAYER`, `GSAMPLE`, `TOFS`, `COLOR`, `ATTR` and
  `TCOLOR`, which the manual's equivalent program leaves out.
- `XSCREEN` clears the console, puts its cursor at 0,0, and resizes `#GSPRITE`
  to the new screen.
- `XSCREEN` moves `#GSPRITE` to the layer `SPLAYER` gives new sprites, whatever
  layer it was on.
- `XSCREEN` stops the text screens' `TANIM` animations (`TCHK` gives 0) and
  keeps their `THOME`, `TROT`, `TSCALE`, `TOFS` and `TCOLOR`.
- `XSCREEN OUT` gives the aspect ratio in single precision: 1.66666663 for
  400x240.
- `TSCREEN` clears the screen. Without a character size it uses the font type.
- `TSCREEN` also stops the screen's animations and undoes `THOME`, `TROT`,
  `TSCALE`, `TCOLOR` and `TBLEND`. `TOFS` goes back to 0,0 but keeps its depth.
  `TLAYER`, `TPAGE` and `TVAR` stay (TEST47), and so does `TFUNC` (TEST48).
- `TARRAY`'s array takes the new size after `TSCREEN` and goes on following the
  screen.

### Cells

- `CHR$(10)` starts a new line; tab stops are every 4 columns.
- `#TUSRCHR` is 59392. (TEST62)
- `TPUT` off the screen is `Out of range`.
- `CHKCHR` off the screen gives 0.
- `LOCATE` takes the column just past the right edge (`LOCATE 50,0` on the
  50-column console).
- `LOCATE 3E10,0` is `Overflow (LOCATE:1)`. (TEST62)
- `LOCATE 0` with one argument is `Wrong number of arguments (LOCATE)`, error
  52 (TEST68).
- Text screens draw the codes from U+E800 up from their own `TPAGE` at font
  types 8, 16 and 32. At font type 32 a text screen shows only user-defined
  characters (TEST68, by its picture).
- `SCROLL 0,0,2147483647` and `SCROLL 0,2147483647,0` clear the screen, as any
  shift of a whole screen or more does (TEST51).
- `TFILL` clips its rectangle to the screen before the string starts, and the
  string runs on across rows at the clipped width. On a 6x3 screen
  `TFILL 0,MIN,MIN,MAX,MAX,"ABC"` gives `ABCABC` on every row (TEST51), and
  `TFILL 0,-1,-1,4,1,"ABCDEFG"` gives `ABCDE` then `FGABC` and an empty third
  row (TEST52).
- `TCOPY` takes rectangles of any size, down to the largest coordinates, and
  copies only what is on the screens (TEST51).
- `TSCREEN` of 65536 cells a side is `Out of range` (TEST51).
- Bit 23 of a value for `TLOAD` or `TUPDATE` (attribute bit 7) does nothing on
  text screens 0 to 3 (TEST56). A plain glyph shows, under `TCOLOR` and `TBLEND`
  too, a space and an empty cell show nothing, and `TCOPY` carries it the same.
- `TLOAD`, `TUPDATE` and `TCOPY` move cells raw: all 16 attribute bits come back
  from `TSAVE`, `TARRAY` and `CHKCHR`'s `OUT`, and on text screens 0 to 3 the
  bits other than bit 7 have no effect either (TEST56).
- Attribute bit 7 on the console inverts a cell, as only the console has
  that per-cell flag. TEST64's picture agrees with all of
  this list but the selection, for white, orange and both at alpha `&H80`.
  - The cell's color is multiplied by 1 minus the glyph's alpha, and then, as
    for any text, by `TCOLOR`, and the color by the resulting alpha, so the
    light is left with the square of it.
  - A space is a block, and so is an empty cell.
  - Direct mode's selection sets the bit, and Enter does not draw the line
    again, so it should stay.
- A cell `TSCREEN` or `CLS` clears has code 0 and the screen's current `COLOR`
  and `ATTR`, white and 0 to start with, not an all-zero cell. A program can
  therefore write only the character codes through `TARRAY`, the even elements,
  and they show after `TUPDATE`: SmileBOY draws its background that way. A new
  `TSCREEN` cell reads `&HFFFFFFFF` and 0, after `COLOR 1,#C_RED:CLS 1`
  `&HFFFF0000`, and after `ATTR 1,8:CLS 1` attribute `&H80000`. `TSCREEN` keeps
  a `COLOR` and `ATTR` set before it. (TEST64)
- A cell that `SCROLL` or the console's scrolling uncovers is all zero, as is a
  cell off the screen for `TSAVE`: a code written there through `TARRAY` alone
  has color 0 and doesn't show. (TEST64 for `SCROLL`, with a picture.)
- `TLOAD`'s width and height have to be 1 or more, and `GLOAD`'s 0 or more, as
  TEST62 saw for `TSAVE` and `GSAVE`. (Not yet seen on hardware.)
- `TSAVE`'s width and height have to be 1 or more: 10x0 and 10x-1 are
  `Out of range (TSAVE:5)`, and 5000x1 goes, reaching off the screen. `GSAVE`'s
  -1 is `Out of range (GSAVE:4)`. (TEST62)

### `TROT`, `TOFS` and `TCOORD`

- `TROT` keeps the angle in single precision: `TROT 0,0.1` reads back
  0.100000001490116 (TEST56).
- `TOFS` takes X and Y left out (`TOFS 0,,,10`).
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
- A linked child's offset turns and scales with its parent whatever the link
  flags. The rotation and scale flags only decide whether the child's own image
  turns and scales too.
- When a parent is cleared, children without link bit 0 stay where they showed,
  Z included.
- `SPSET` drops the sprite's `SPFUNC`, also on a sprite that was set already.

### Collision

- Hidden sprites do not collide, including children hidden through their parent
  (link bit 0).
- Sprites without `SPCOL` never collide, and rectangles that only touch do not
  either.
- `SPCOL` without a rectangle starts it at minus the home point.
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

### `SPCHR`

- `SPCHR` reads its arguments by how many there are (TEST45), not as the
  manual's `SPCHR id,[U],[V],[W],[H],[attr]`. Empty U and V always keep the
  sprite's own.
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
- `GCIRCLE` with equal start and end angles draws nothing; the sector flag draws
  only the outline.
- Drawing rules read back with `GPGET` (TEST47, all 23 parts matched):
  - `GCIRCLE`'s pixels are the midpoint circle's: in each row of the first
    octant, the largest x with x² - x + y² < r². Radii of 1000 and 70000 far off
    the page still draw what reaches it.
  - `GCIRCLE`'s arc runs between the circle pixels nearest its two angles, at
    `round(r*cos a), round(r*sin a)`, both included: 30 to 120 degrees takes the
    pixel (7,4) at 29.7 degrees. Angles past 360 or negative are taken modulo
    360. An arc of more than half a turn whose two end pixels are one, such as 0
    to 359 or 10 to 9, is the whole circle (TEST48). A sector's two sides are
    `GLINE`s from the center to those pixels.
  - `GLINE` takes the same pixels whichever end comes first, in all four
    directions and with X and Y swapped. Walked along the longer axis from the
    end with the smaller coordinate on it, `major` steps long and `minor`
    across, the offset across at step t is `(2*t*minor + k) / (2*major)` rounded
    down.
    - When that end is on the page, `k = (major + 2*minor) mod (2*major)`: the
      line rounded (halves up) one step further on, `round((t+1)*minor/major)`,
      less `round(minor/major)` so that it starts at its end. `GLINE 0,0,7,3`
      steps across at X 1, 3 and 5 (`01122333`), `GLINE 0,0,6,1` at 2, and
      `GLINE 0,0,4,2` at 2 and 4.
    - When that end is outside `GCLIP`, the page if none is set, `k = major`:
      the line itself rounded, halves up, counted from its real end (TEST56, ten
      lines, either way round). `GLINE -1,5,30,10` steps at X 3, 9, 15, 21 and
      27, `GLINE -1000,0,1040,51` gives rows 25 for X 0-19 and 26 from 20, while
      `GLINE 0,25,40,26` steps at X 19 (TEST47, TEST48).
  - `GTRI` draws each pixel once (with `#G_ALPHA`, also where edges meet), a row
    at a time from the leftmost to the rightmost pixel an edge reaches in the
    row, whatever order the corners come in (TEST47, TEST48, TEST56).
    - Each edge goes down from its top end with a sum that starts at `-wide` if
      it is wider than tall, or `tall` if not. Each row adds `2*wide`, and the
      edge takes a step across for each `2*tall`, at most one past its bottom
      end.
    - A row has the columns from where the last row ended to the one before
      where this one ends. Level edges have none of their own.
    - Corners off the page are not clamped.
    - For example, `GTRI 0,0,1,2,10,1` fills rows 0-4, 1-10 and 1-6.
    - A triangle whose corners lie on one line is that line, drawn once.
  - `GBOX` draws the top and bottom rows, then the sides from the top row to the
    row above the bottom: with `#G_ALPHA` the two top corners are blended twice
    and the bottom ones once, whichever corners are given first (TEST48). A box
    one pixel wide or high draws each pixel once, except the top of a
    one-pixel-wide box. `GBOX 0,Y,5,Y` on row -2147483648 draws nothing on the
    page (TEST51).
  - `#G_ALPHA2` (mode 3): alpha `a = sa + da - floor(sa*da/255)`; each channel
    `(s*floor(sa*255/a) + d*floor(da*(255-sa)/a)) / 255` rounded down. Over an
    opaque pixel that is mode 2's result.
  - `GPAINT` fills 4-connected areas.
  - `GCOPY`'s color code multiplies and rounds down, and an overlapping copy
    within a page reads the source first.
  - `GPUTCHRP` drawing scales its spacing with the characters.
- `GARRAY(page)` is a 2048x2048 Int array indexed `[y,x]`, the same one each
  time. A write to it is seen at once by `GPGET`, also where `GPSET` drew next
  to it.
- `GUPDATE` with a coordinate of 2048 is `Out of range`.
- `TSAVE`, `GSAVE` and `GLOAD` accept rectangles reaching off the screen or
  page; what lies outside reads as empty.
- `GSAVE` and `GLOAD` take rectangles of any size memory allows: 8192x1, 1x4097
  and 2048x2049 work, `GSAVE` giving 0 for what lies off the page and `GLOAD`
  dropping it. A width or height of 0 is no error: `GSAVE` then gives no
  elements, and `GLOAD` draws nothing (TEST47, TEST48).
- `GLOAD` palette indexes past the palette are no error and leave the page's
  pixel as it is, with mode 0 too (TEST47).
- `GSAVE` into a real array gives the colors unsigned: opaque red is 4294901760,
  where an integer array has -65536.

### `GPUTCHR` and `GPUTCHRP`

- `GPUTCHR` with mode 0 clears the page under the character's transparent
  pixels.
- `GPUTCHR` without a font type uses the console's. After `TSCREEN 24` it draws
  24-pixel characters from GRP5, glyph i (font.map's number, as for 8 and 16) at
  column i%64 and row i/64 of 24-pixel cells, which is how `#GUIDE/DEFFONT24` is
  laid out.
- `GPUTCHR` given a font type other than 8 or 16 is `Illegal font type`.
- `PRINT` on a console of such a font type shows only user-defined characters.
- `GPUTCHRP` takes each character from one empty column before what is drawn of
  it (none if the glyph starts at its first column) to its last drawn column. It
  then moves on that many pixels plus the spacing (1 when left out). A blank
  character is a quarter of the font size wide.
- In the 16-pixel font, `GPUTCHRP`'s width is the one font.map holds in bits
  7-15 of each code's second u16.
- `GPUTCHRP`'s function form's width scales the spacing with the characters:
  `GPUTCHRP("AB",16,2,3)` is 2*(13+11+3).

### Colors and blending

- Blending rounds to the nearest, not down: a sprite at &H80 alpha over 60 gives
  (255x128 + 60x127)/255 = 157.9, shown as 158. With that, the emulator matched
  lossless screenshots of mode 0 and mode 1 layers pixel for pixel.
- Colors are worked out as follows, confirmed by TEST49 (about 4,500 patches
  read from lossless screenshots, every one within 1):
  - Each 8-bit value reaches the GPU as k/255, and the work is in floating
    point with no 8-bit rounding inside a draw. A draw is rounded once, to the
    nearest, when it is stored: an element into its layer (or the screen, for
    mode 0), a composed layer into the screen, and the screen to the display.
  - What is drawn holds premultiplied color, and its alpha is how much of what
    is behind still shows through. A layer of its own starts clear: black,
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
    lies halfway between two values.
  - TEST49's screenshots come from an emulator running SmileBASIC 4 on a PC's
    GPU, and that GPU stores results its own way.
    - About one patch in six has a channel 1 off the rules above, mostly where
      the exact value is a little above a half (`&H80010101` added to 60 is
      60.502, shown as 60).
    - A screen result cut to 12 bits of fraction, with halves rounded down
      (`floor(x*4096)`, then `(q*255 + 2047) >> 12`), gives every FADE sample
      exactly and nearly every additive one.
    - That is the PC's GPU, not SmileBASIC 4; the Switch's may round
      differently.
- An additive sprite inside a composed layer adds light without covering
  anything, so it also brightens what is under the layer.

### `FADE`

- `FADE` is done as the screen goes to the display: `d + (c-d)*fade.a`, rounded
  once. Beside a subprogram, the side without the focus is darkened in the same
  step, times 224/255 (`&HE0`): 255 to 224, 248 to 218, 96 to 84 (TEST49).
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
- `LCLIP OUT` gives a clip past the frame as it was set: 1350 0 1500 719 on a
  1280x720 screen. A layer whose `LMATRIX` puts its text at X 1270, clipped
  there, shows nothing on the screen's left edge (TEST64).
- `LCLIP` at `XSCREEN`'s sample 2 takes whole fine pixels: frame pixels `2*x0`
  to `2*x1+1`.
- `LMATRIX layer,hx,hy` moves the layer by minus the home point.
- The 2D `LMATRIX` form scales first and then turns clockwise, with x and y
  outside, as a sprite does; it applies after each element's own placement
  (TEST53).
- `LMATRIX layer,M` with 16 reals takes the layer's coordinates as (x, y, 0, 1)
  through M to clip space, X right and Y up: the identity puts the layer's
  square from (0,0) to (1,1) over the screen's top right quarter.
  - M is column-major: M[12] moves along X. With M[3] set, W grows along X and
    the picture narrows that way as OpenGL's perspective divide would.
  - Raster deformation applies to the screen after LMATRIX has placed the layer
    (TTEST27, pixel for pixel).
  - Sprites on such a layer show whole, also the parts at negative coordinates
    or past the screen's size, as FINITI's floors running to the horizon show.
  - Text screens show whole too: a console moved to (-200,-120) or to (500,300)
    of a 400x240 screen shows where the matrix puts it (TTEST36, pixel for
    pixel).
  - A sprite's Z plays no part: every point goes in with z 0, so Z comes from
    M[2], M[6] and M[14]. What has Z/W outside -1 to 1 does not show, nor what
    has W at 0 or below (TEST53).
  - Sampling is nearest at any magnification, and perspective correct (TEST53).
  - `LCLIP` on such a layer is in frame pixels after the matrix (TEST53).
- `LMATRIX layer,array` wants a real array of 16 elements or more and looks at
  the first 16: 15 are `Type mismatch (LMATRIX:2)`, 17 go, and a value of 1E39
  is `Out of range (LMATRIX:2)`. (TEST62)

### `LFILTER`

- `ACLS` clears every layer's `LFILTER` (TEST47).
- `LFILTER` copies a raster array when it is called: writing to the array later
  changes nothing on the screen until the next `LFILTER` (TEST60).
- `LFILTER layer,0,n` is `Out of range`.
- `LFILTER`'s raster deformation (types 3 and 4) needs the repeat flag:
  `LFILTER 1,3,A,#TRUE` with a real `A[240,4]` or `A[240,2]` works, while
  without the flag it is `Out of range` for the type whatever the array.
  `A[4,100]` or an Int array with the flag is `Type mismatch`.
- `LFILTER`'s raster deformation works as OpenGL would. Checked pixel for pixel
  with screenshots (TTEST26):
  - Each row (type 3) or column (type 4) takes an entry of X offset, X scale, Y
    offset and Y scale in screen axes for either type.
  - A pixel shows the source at its center times the scale plus the offset in
    screen widths or heights, measured from the left and from the bottom.
  - A positive X offset moves the picture left; a Y scale of 0.5 stretches the
    bottom half over the screen.
  - Without repeat, what falls off shows black, BACKCOLOR included on a layer
    that is not composed.
- `LFILTER`'s raster entries (TEST54):
  - The scale comes before the offset, and a positive Y offset moves the point
    up from the bottom.
  - An array of two values to an entry holds the offset and scale along the
    deformation: X for type 3, Y for type 4.
  - With n entries, row or column i of `size` takes entry
    `floor((i+.5)*n/size)`, the one its center falls in.
- `LFILTER` on a layer that `LAYER` does not compose (mode 0) filters the whole
  screen drawn so far: the layers behind and `BACKCOLOR` too. That holds for a
  layer with nothing in it, too. On a composed layer it filters only that layer.
- `LFILTER`'s mosaic draws the whole frame into a few pixels with bilinear
  sampling, and those back over the frame without smoothing.
  - Blocks are n frame pixels whatever `XSCREEN`'s sample (TEST54), and their
    number is reckoned in screen pixels: `ceil(width/n)*sample` across. Where n
    does not divide the size, blocks are a little smaller than n, ending at both
    edges of the frame.
  - Sampling puts a block of odd n on its middle pixel, and a block of even n
    exactly between four, which it averages (halves rounded down).
  - A size below 2 does nothing.
- `LFILTER`'s blur halves the layer in frame pixels, whatever `XSCREEN`'s
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
- `LFILTER`'s color conversion works on the layer's premultiplied pixels as they
  are: hue in degrees, saturation and value on 0 to 255, and a gray has no
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
  the bottom edge on the screen, however the image is turned. `SPOFS 0,0.5,0.5`
  draws from the first column, and from the second row to one row past the
  image's height.
- A sprite or text cell whose pixel centers fall exactly between texels can show
  a pixel off its image. An image side and a scale that are powers of two read
  the page's higher texel, and anything else mostly the lower, at any flip or
  turn (TEST63, 3831 sprites and 192 cells on a Switch 2;
  [renderer-verification.md](renderer-verification.md) has the numbers).
  - On a half-pixel position 16x16 shows the page's next row at its bottom edge,
    and 15x15 the column before it at its left.
  - A cell shows what lies beside it on the font page or `TPAGE`.
  - Before the page's first texel, U or V of 0, it reads the first texel; after
    the last, U+W of 2048, nothing.
- A sample point exactly between two texels goes by the rule above. What is not
  a power of two shows the GPU's single-precision arithmetic besides. About 1
  line in 20 goes the other way, by the place on the screen, by a triangle of
  the two a quad is drawn as, or from some line along the image on (TEST63).
- At equal Z text screens are behind sprites, and among either the higher number
  is behind, `#GSPRITE` among the sprites as 4095 (TEST50).

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
  nothing, and `BGMCHK` says 0.
- `BGMPLAY` of a user's song number that holds nothing leaves what is playing as
  it is. (TEST63)
- `BGMPLAY`, `BGMSTOP`, `BGMPAUSE`, `BGMCONT` and `BEEPSTOP` take their first
  argument left out with a comma after it (`BGMSTOP ,1`, `BGMPLAY ,0,64`).
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
- The program reads the MML variables as they were when the frame started, so a
  value set by the song, by `BGMVAR` or by stopping shows a frame later.
- A song started while no other plays takes about two frames more to begin: its
  first `$0=` showed four frames after `BGMPLAY`.
- `RECSTART` right before such a `BGMPLAY` waits with it: the song sounds within
  50 samples of the recording's start.

### MML

- MML takes a bare length after a tie (`C4&2`), `@MON`, `N` without a length
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
  `@E128,0,0,0C` gives 5 and `@E0,0,0,128C` 11. `@148` to `@216` pass, and so
  do lower case letters and a second dot (TEST67).
- `V` and `@V` (MML's volume and velocity) both start at 127 and both scale the
  level by their square: `V64` and `V127@V64` record alike.
- `P64` is the middle, even on both sides. The pan's position is (P-64)/63, so
  `P0` and `P1` are both hard left.
- Recorded with `RECSTART`, an A held at V127 has an RMS of 850 on `@0`, and a
  full-scale 8-bit waveform an eighth of full scale on each side (a ±96 square
  3063, a sine of amplitude 100 2257).
- `Qn` holds a note for (n+1)/33 of its length, cut to whole ticks (48 to a
  quarter note) and at least one. At T120 a `C4` is let go after 10 ms with
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
  - The delay counts 40 ms a unit whatever the tempo, and the wave starts from
    its middle once it has passed: `@MP7,1,7,48` did not move within 1.5 s at
    T120.
  - The sine runs 5 ms behind the note, or behind the end of the delay:
    `@MP64,1,20,0` is at 0 5 ms in, and `@MP64,1,20,8` begins at 325 ms at T60
    as at T120.
  - `@MON` turns all three back on as last set.
- `@MA`'s swing is read in tenths of a dB, truncated, and stops at the sound
  engine's limits: no more than +6 dB, and silence from -90.4 dB down. A wide
  one such as `@MA127,110,12,0` (±655 dB as asked) therefore chops the note into
  bursts at twice its level with silence between, and is no louder than that. A
  song using three of these plays at a normal level on SmileBASIC 4.
- Envelopes move a millisecond at a time in tenths of a dB:
  - The attack multiplies the level, starting from -90.4 dB, until it is within
    0.003 dB of full.
  - The hold lasts (h+1)^2/4 ms.
  - Decay and release take off 65535 tenths a millisecond at 127, 24 at 126,
    (2v+1)/640 for v up to 49 and 12/(126-v) above.
  - The sustain level is 400 log10(s/127) tenths, no lower than -723 (-722 for
    1).
  - A note ends below -90.4 dB.
- `@E` sets only attack, decay, sustain and release and keeps an instrument's
  hold; `WAVSET` and `WAVSETA` have no hold.
- `_` (portamento) glides during the note it follows, in a straight line in
  semitones over that note's gate, and the next note goes on from there without
  starting again: `T120O4C2_G2` slides from C to G over the first second and
  holds G for the second, and on a piano (`@0`) the G does not strike again.
- `&` (slur) to another note goes on at that note at once without striking
  again: on `@0`, `C4&G4` keeps decaying from the C through the G, where `C4G4`
  strikes the G anew.

### Instruments, `WAVSET` and `WAVSETA`

- `@216` to `@222` (PSG) and `@223` (noise) are instruments of the sound archive
  like `@0`. Each PSG tone records at an RMS of about 4031 (peak 4090) at V127,
  whatever its width, and the noise at 2839, never going below zero.
- An instrument region's own volume is a linear gain (`@0`'s is 88), unlike `V`
  and `@V`.
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
- `WAVSET` also takes groups in square brackets, which the manual leaves out:
  `WAVSET 224,127,0,0,127,"[FF00][80][4080C0]",69` is no error (TEST56).
  - A string starting with `[` has groups of an even number of
    digits, at most 1024 in all, and no group over 512 over their number in
    samples.
  - Each group is stretched to that many samples, but each is
    written from the start, so the last group alone stays and the rest of the
    512 samples is whatever the buffer held.
  - `RECSAVE` shows it (TEST60): `[C040]` sounds like the same 16 digits without
    brackets, and in `[C040][F010]` the `F010` fills half of the pass with the
    other half loud noise, the same each time (peak about 4600 at 127, a DC of
    about -1500).
- `WAVSETA`'s 8-bit form (fewer than 10 arguments) plays a pass of its n samples
  at 28160 Hz over the next power of two at or above n. At reference pitch 69,
  `O4A` with one cycle of 64 samples is in tune, 16 samples sound 2400 cents up,
  512 3600 cents down, 2048 6000 down.
- `WAVSETA`'s 16-bit form (10 arguments or more) takes the loop's end + 1 values
  from the offset on, with the loop counted from the offset, and drops what
  comes after; values are cut to 16 bits. A value v sounds like the 8-bit form's
  v/256 + 128.
- `WAVSET` or `WAVSETA` while a note of that instrument sounds writes the new
  samples over the start of the instrument's memory. The note goes on reading it
  with the length, loop and rate it started with, keeping its envelope:
  - A square swapped for a square of two cycles doubles the pitch at the same
    level.
  - A `WAVSET` square (512 samples once stretched) swapped for a 32-sample sine
    plays the sine's one cycle and then the rest of the square on each pass,
    near the square's level (2981 to 3023 against 3063) with twice the
    crossings.
- `WAVSETA` with an empty array is `Illegal function call` (TEST51, TEST52).
- `WAVSETA`'s arguments after the array can be left off the end, but one left
  out in between is `Type mismatch` with its number, in both forms and ahead of
  the check for an empty array: `WAVSETA 224,0,0,0,0,A%,,0,15` is `(WAVSETA:7)`,
  and `...,A%,69,,15` and `...,A%,69,,,28160` are `(WAVSETA:8)` (TEST51,
  TEST52). A trailing comma counts: `...,A%,69,0,15,28160,` is `(WAVSETA:11)`
  (TEST57).
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
  - 32 and 128 samples at 28160 Hz come to no note and record as silence.
  - `WAVSET` always has a note of a second.
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
- `BEEPSTOP` without a fade lets all the handle's notes go and frees the handle.
  The sound goes on past it by the release, the 1.5 s recording of the case
  above still sounding at its end. At release 127 that is one step, as when the
  16th `BEEP` after a `BEEP` statement takes over its place: a 1 s square ends
  at 404 ms for a `BEEP` at 400 ms (TEST61).
- `BEEPSTOP` with a fade takes the volume to 0 first, so nothing is heard after
  it. The note is the same with or
  without the flag.
- The sound engine has 96 voices for everything, BGM too. A new note takes one
  from the lowest priority there is, the oldest among equals, or does not sound
  if all are higher.
- `BEEP`'s volume is a linear gain of v/127 the same way
  as `BGMVOL`'s, with 64 for its default as `BGMPLAY` has. It comes out level
  with `BGMPLAY`'s default:
  - At envelope 127,127,127,127, a `WAVSET` square of `"FFFF0000"*8` peaks at
    4623, an 8-bit `WAVSETA` square of ±100 at 3613 and a 16-bit one of ±30000
    at 4234 (TEST59).
  - `@224` at `V127` of a ±96 square peaks at 3468 under `BGMPLAY`, for an RMS
    of 3063 (TEST40).
  - These are all 4624 for full scale, and an RMS of 4084.
  - A square peaks about 13% over its RMS for how the console resamples it,
    where a sine does not: amplitude 100 peaks at 3217 at an RMS of 2257
    (TEST40).
  - `BEEP` of the same shapes records the same: the ±96 square an RMS of 3050
    and a peak of 3468, the sine 2257 and 3217.
  - A square's level at volume 127 and 32 is 6053 and 1525, in `BEEP`'s argument
    and by `BEEPVOL` alike (TEST61).
- `BEEP` 224 to 255 with nothing set plays a sine of about 440 Hz for a second,
  peak 4117, last sample at 999 ms (`BEEP 250`, TEST59).
- `@224` in the sound archive is that sine, 16384 samples at 28160 Hz. `@225` to
  `@255` are 140 samples of silence there, but all of `@224` to `@255` start on
  `@224`'s waves. In MML too, `T120@250V127O4A2` and the same on `@240`, never
  set, peak at 4089 at an RMS of 2881 and sound from 0 to 999 ms (TEST61).
- `BEEP`'s sound for each of 224 to 255 is `T400V127P64@nT60O3A4` until set, and
  `WAVSET` of a number replaces only that number.
  Nothing a program can run puts them back: not `SNDSTOP`, `ACLS` or `CLEAR`.
- `BEEP`'s result is a number 0 to 15 that goes on rising with each `BEEP`, also
  after the last has ended and for a silent one, and through program runs.
  Running TEST59's parts in the order 20, 16, 17, 18, 19 on a started SmileBASIC
  4 gave 0, 1, 2, 3, 4, 5, 6, 7, 8 (TEST59). It goes round at 16 and nothing
  starts it again: not `ACLS`, not `RUN`. Each
  interpreter counts for itself.
- `BEEP`'s result is a place to play in (TEST61):
  - `BEEP` used as a function holds its place until the sound ends or `BEEPSTOP`
    names it, and the next `BEEP` passes it by.
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
- `BEEP`'s volume on a preset effect is linear, as for waveforms: `BEEP 0` at
  127 and 32 peaks at 1395 and 351 (TEST61).

### `TALK` and sung notes

- `TALK` reads は as "wa", falling in pitch, when 、 comes right after it, as for
  a particle; before 。 or anywhere else it is "ha".
- A new `TALK` cuts off what is being said. (Not yet seen on hardware.)
- A sung note (`*A,C2`) is the voice alone: the channel's instrument stays
  silent under it, and plays again on the next plain note.
- Each channel sings with a voice of its own, so two channels' sung notes
  overlap, and `TALK` and sung notes sound together without cutting each other
  off.
- A sung note's letters must make syllables, or the MML is `Illegal MML`, with
  the caret at the first letter that cannot go on: `*KONNICHIWA,C1` fails at its
  second N. ん is `Q` (`*KOQNICHIWA,C1`), as in `TALK "$..."`.

### Effector, recording and balance

- `EFCWET`'s four values (BEEP, BGM, TALK and PCM), with the effector on, each
  send that part of the sound to the reverb: wet/127 of it goes there and the
  rest straight out, in a straight line. With it off, they change nothing.
- `RECSTART` records the sound before the effector, or the effector did nothing:
  a short note recorded alike, to the unit, with `EFCEN` off, with `#EFCCAVE` at
  `EFCWET 0,127,0,0`, with a hall set by `EFCSET #EFCREFHALL,...` and with
  `EFCEN` off again.
- `RECDATA` positions go round the buffer too while `RECSTART` without a time
  records round it (-1 is the last sample, `RECLEN()` the first). With a time,
  -1 is `Out of range`.
- `SNDMSBAL` starts at 63, though the manual calls 64 even.
- `SNDMSBAL()` and `PCMPOS()` work as functions.

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
  named. (TEST63)
- `BQPARAM` takes the gain for types 5 to 7 only: leaving it out there, or
  giving it for types 0 to 4, is `Wrong number of arguments`.
- `BQPARAM` with a sampling rate of 0 is `Illegal function call`.
- `BQPARAM` looks at the number of its arguments before their values: type 5
  with no gain is `Wrong number of arguments (BQPARAM:2)`. A Q of 0 is
  `Illegal function call (BQPARAM:5)`. (TEST62)

## Direct mode

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
- Variables stay until `CLEAR`, however they came about: by `DIM`, `VAR`,
  assigning or only reading them (an unknown variable reads as the real 0).
  Declaring one of them again is `Duplicate variable`.
- A line's declarations are checked as it runs, not when it compiles:
  `DIM H[2]:DIM H[2]` declares `H` and then fails on the second `DIM`. In a
  program the same line does not compile.
- A declaration that fails still declares. After `DIM D[0,3]` fails, `D` is the
  omitted value (`TYPEOF` 0, and `?D` is `Type mismatch`); after `VAR G=SQR(-1)`
  fails, `G` is 0. A line that does not compile declares nothing.
- `CLEAR` in a program is a `Syntax error` when it runs.
- `FILES` prints a long list a screen at a time, in direct mode and in programs
  alike. The list fills every row of the console but the last, which says
  `=== Press ENTER ===`, so the page follows the console's size (29 rows of the
  list on the usual 30). Any key or button goes on, and a held key goes on as it
  repeats.

## Stopping

- `ON BREAK GOTO ""` takes the handler away.
- The + button acts when it is let go. A short press jumps to the label of
  `ON BREAK GOTO`, once; without one, the program stops with
  `User break on 0:55`. Held for a second, the button stops the program at once,
  without the jump.
- `ON BREAK GOTO`'s jump leaves GOSUBs behind: a `RETURN` after it is
  `RETURN without GOSUB`.
- `ON BREAK GOTO` inside a DEF may name a label outside it.
- Breaking off a line typed in direct mode says `User break on 0:1`. A forced
  stop (holding + for a second) says `User break on` too.
- A press of + out of a waiting `INPUT` or `LINPUT` counts twice. It jumps, and
  acts again once the first statement there has run (a whole FOR loop's first
  statement only). The second time it jumps to a label that statement set with
  `ON BREAK GOTO`, or stops with `User break on` that statement's line. Printing
  goes on after the prompt.
- `STOP` says `Break on 0:60`, then its message, if it has one, on the next
  line. In direct mode it is `Can't use from direct mode`.
- `STOP` with a message prints 255 characters of it (TEST62).
- `CONT` goes on, against the manual, after a program has been edited, even with
  lines added.
- `CONT` goes on, against the manual, after the + button stopped a program at
  `INPUT`: it goes on with the next statement, and `User break on` follows the
  prompt on the same line.
- `CONT` after an error is `Can't continue`. In a program it is a `Syntax error`
  when it runs.
- `BACKTRACE` lists, innermost first, the line it stopped on and the line of
  each DEF call and `GOSUB` it is in, as `0:    92` (the line right-aligned in 6
  places). It works after an error too, and prints nothing when no program has
  stopped. After it, `CONT` and `TRACE` cannot go on.
- `TRACE` runs one statement and says `Break on 0:2` with that statement's line,
  in DEFs and GOSUBs too. After an error it is `Can't trace`.
  - Some places are no statement of their own: a `FOR` runs on into the first
    statement of its body, and a DEF's `END` runs on through the return into the
    caller's next statement.
  - `NEXT` takes two TRACEs that both say the FOR's line (the step, then the
    test).
  - After a GOSUB's `RETURN` it names the GOSUB's line.
- `Can't continue`, `Can't trace` and `Can't use from direct mode` name no line.
- `Break on` follows whatever was printed: after `?"X";` it reads
  `XBreak on 0:16`.
- `ENVSTAT()` on the main interpreter is 3 (running), also typed in direct mode
  while a program is stopped or after an error; its OUT forms then report no
  error.

## Program slots and files

### `CHKCALL` and `CHKLABEL`

- `CHKCALL` is 0 for `PRINT`, `GOTO`, `INC`, `DEC`, `STOP`, `CONT`, `RUN`,
  `NEW`, `CLEAR`, `LIST`, `TRACE`, `BACKTRACE`, `EXEC`, `DEFOUT` and `END`, and
  1 for `VSYNC`, `WAIT`, `INSPECT`, `CALLIDX`, `DEFARGC`, `PROJECT`, `FILES`,
  `ACLS`, `LOAD` and `SAVE`. It ignores case.
- `CHKCALL("PRINT")` and `CHKCALL("GOTO")` are 0, statements not being commands,
  and `CHKCALL("3:SIN")` is 1 and `CHKCALL("3:NOSUCH")` 0. (TEST63)
- `CHKCALL("1:ACLS")` is 1 for the system's commands and functions, whatever is
  in slot 1.
- `CALL "WAIT",n`, `CALL "VSYNC",n` and `CALL("FREEMEM")` work.
- `CHKLABEL` needs the `@`: `CHKLABEL("P1")` is 0.
- `CHKLABEL("1:@X")` on a slot `EXEC` has not run is 0. With a slot, it looks at
  that slot's top level, also from a DEF in the same slot: FINITI finds its own
  models so.
- A slot that has not been run gives 0 for `CHKLABEL("3:@A")`,
  `Undefined variable` for `VAR("3:A")` and `Undefined label` for
  `RESTORE "3:@A"`. (TEST63)

### Slots, `EXEC` and the `PRG` commands

- `LOAD` can, against the manual, go over the program's own slot, and the
  program goes on.
- `LOAD` over a slot that `EXEC` ran keeps that slot's `COMMON DEF`s callable,
  against the manual.
- `EXEC` of a slot with code still to run, such as `EXEC 0` in a DEF of slot 1
  that slot 0 called, is `Can't use the specified slot`.
- `EXEC` of the slot running starts it again (`EXEC 0` in slot 0).
- Slots that `EXEC` ran are gone after the next `RUN`.
- `PRGEDIT slot,n` past the last line goes to the last line, and
  `PRGEDIT slot,-1` past it, where `PRGGET$` is empty.
- `PRGSEEK 1` from the last line goes past it and gives 0.
- `PRGDEL` at the end of a slot deletes back towards its start.
- `PRGSET` on an empty line in the middle replaces it.
- SmileBASIC Debugger's model of `PRGEDIT`, `PRGSEEK`, `PRGDEL` and `PRGSET`
  agrees with these four bullets.
- `PRGNAME$()` and `PRGSIZE()` at the prompt mean slot 0, whichever slot
  `PRGEDIT` is on. (TEST63)
- After SmileBASIC starts afresh, slot 0 has an empty `PRGNAME$()` and a
  `PRGSIZE()` of 0 (TEST64).
- `PRGSET`, `PRGINS` and `PRGDEL` changes do not count as unsaved: `LOAD` over
  the slot from a program, `NEW` and `PROJECT` do not ask about them (DAW_MUSIC,
  and by hand).
- `#_FILENAME` is the program's name (`TEST32`), and empty in direct mode.
- `#_SLOT` is the program's slot, and in direct mode the slot run last.

### Files

- `LOADV` reads a DAT file whose header's size leaves out the `PCBN` array's
  header: SmileBOY's `POCKET.GB` loads as an array of 32768 (TEST64).
- `EXEC` of a file that is not there shows a dialog (File not found) and then
  stops with `Load failed`. In direct mode, `EXEC` is
  `Can't use from direct mode`, shown without a line.
- File names may start with `@`, as the backups' do: `LOADV "DAT:@BACKUP.DAT"`
  reads one, and `SAVE "@X"` saves (after the usual question).
- File names may end in a dot: `SAVEV "TXT:Q51.",S$` saves, and
  `CHKFILE("TXT:Q51.")` is 1 while `CHKFILE("TXT:Q51")` is 0 (TEST51).
- `LOAD` takes the file's whole name: `LOAD "TEST10"` does not find
  `TEST10.PRG`.
- `LOAD` and `LOADV` in a program take other projects' files
  (`LOAD "GAME/SUB",1`), and a program may name its own project in them.
- A program's `EXEC` of a file named with a project is
  `Can't access other project in this mode`, error 74, even for the program's
  own project: `EXEC "TEST05/HGLIB.PRG",1` in a program of project TEST05
  (TEST68, a main program while a subprogram ran).
- `LOAD` into a slot and then `EXEC` of the slot runs another project's
  program: `LOAD "OTHER/NAME",1:EXEC 1` in a program works and shows no dialog
  (TEST68, and checked on hardware, version 4.4.9, 2026-11-08).
- `LOADG` of a name with a project and no type, such as `LOADG "HGLIB/NAME",0`,
  loads the other project's image. A program loads it without a dialog; at the
  prompt SmileBASIC 4 shows its load dialog first (checked on hardware, version
  4.4.9, 2026-11-08).
- `LOADG "GRP:TEST05/HG16_NEODGM",3` in a main program of project TEST05, while
  a subprogram ran, gave `RESULT` 0, loaded nothing and left the page as it
  was, with no error (TEST68). What refuses it is not known: the `GRP:` prefix,
  the program's own project, the page, or the running subprogram.
- Expected but not yet seen on hardware: a subprogram's `EXEC` refuses a
  project name too, and `EXEC` takes SmileBASIC 4's own `#` projects. `SAVE`,
  `SAVEV` and `SAVEG` refuse a project name with error 74 in a main program,
  but not in direct mode or a subprogram. `RENAME` and `DELETE` refuse one
  everywhere. The `LOADG` refusal with `GRP:` may be new in version 4.4.9.
- `LOADG` leaves the page's pixels outside the loaded image as they were: a
  1024x624 image loaded at 0,0 leaves 1027,5 and 5,627 alone (TEST68).
- `LOADV` and `LOADG` in a program ask nothing. `S$=LOADV("TXT:NONE")` of a
  missing file still opens a dialog that holds the program for 50 to 70 frames
  (likely File not found, as for `EXEC`), and gives `""` with `RESULT` 0
  (TEST67).
- `LOADV "DAT:F",M%` of a real file into an int array converts as `INT()`
  does, with NaN as 0: NaN, 1.9 and -1.9 load as 0, 1 and -1, and 2147483647.5
  as 2147483647 (TEST67).
- The command form `LOADV "TXT:F",S$` is `Wrong number of arguments (LOADV)`,
  error 52, whether the file is there or not, as `LOADV "TXT:F",A$[]` is
  (TEST67).
- `RESULT` is 0 after `IF CHKFILE("TXT:NONE") THEN LOADV ...` of a missing
  file, where the program before left it 1 (TEST67).
- `LOAD "GRP0:X"` and `SAVE "GRP0:X"` are `Illegal format (LOAD:1)` and
  `(SAVE:1)`, error 71, with no dialog (TEST67).
- The question before `SAVE`, `SAVEV` or `SAVEG` starts on OK, so Enter or A
  saves. Replacing a file asks twice, whether to save and then whether to
  overwrite (checked by sending files with a Flipper, 2026-11-03, and in
  TEST67). Three `SAVEV`s of one TXT file kept the copy each replaced in
  `@BACKUP.TXT`, and No on the question gave `RESULT` -1 and left the file
  alone (TEST67).
- `FILES("//")` ends each project's name with `/`, lists the workspace as `/`,
  and lists each `#` project twice (`#EXAMPLE/`, `#EXAMPLE/`).
- `FILES` takes a `/` after a project's name (`FILES("TXT:NAME/")`), and
  `FILES("TXT:/")` lists the workspace.
- `METALOAD "NAME"` loads another project's metadata (a project that is not
  there is `Undefined project name`); `METALOAD` alone loads the current one's
  again. Before any `METALOAD`, the title is the project's name.
- `METAEDIT(item)` works as a function.

## Subprograms

Except under "A subprogram's screen", bullets that name no test come from
TTEST36.

### Starting and ending

- `ENVTYPE OUT T,P` gives P -1 in a subprogram.
- `XSUBSCREEN OUT` before any `XSUBSCREEN` gives the whole display,
  0,0,1280,720.
- `SNDMSBAL()` starts at 63 in a subprogram too.
- `PROJECT()` in a subprogram is the project `SUBRUN` loaded the file from.
- `ENVINPUT$(1)` is the direct-mode line typed last, the `SUBRUN` itself.
- An error, a `STOP` or `SUBSTOP` ends the subprogram, hides its screen and
  gives the focus back. The main console prints why, between lines of its own,
  without OK unless a line of its own ran:

  ```
  === From subinterpreter ===
  User break on 0:11
  ===========================
  ```

  For an error the middle line is its message (`Divide by zero in 0:8`), for
  `STOP` `Break on 0:7`.
- `SUBRUN "/A/../B"` prints `OK`, then the same block with `Illegal format` as
  its middle line (TEST51), and `SUBRUN "DAT:Z51A"` does too (TEST57).
- `SUBRUN` takes a name in lower case and one starting with `TXT:`:
  `SUBRUN "test51s"` and `SUBRUN "TXT:TEST51S"` give no error and no block.
- `SUBHIDE` hides the subprogram's screen and `SUBSHOW` shows it again, while it
  goes on running. A clear `BACKCOLOR` shows the main screen through.
- `END` in a subprogram hides its screen and prints nothing; the line being
  typed in direct mode stays as it was, and the main interpreter has the focus
  again (TTEST37, TTEST38).
- The tools on F9 to F11 start also while a program runs.
- `SUBRUN`, or a tool key, while a subprogram runs stops it first with
  `User break`, printed on the main console as above, also when the new file
  cannot be loaded.

### Focus

- A subprogram starts with the focus.
- While a subprogram runs, the side without the focus is drawn darker, each
  color channel to 7/8 (255 shows as 224, 96 as 84). That side is the main
  screen while the subprogram has the focus, and the subprogram's screen once it
  gives it up. The editor is darkened the same way while a tool started from it
  has the focus.
- `PUSHKEY` in the subprogram, after `ENVFOCUS 0`, types into the main
  interpreter's direct mode, which runs the line.
- `DIALOG` in a subprogram answers to the controller with the focus on either
  side, and the main program keeps running meanwhile (TEST64). With the focus
  on the main program, its `BUTTON(0)` sees the A that answers, and its
  `INKEY$` gets no key from it. With the focus on the subprogram, the main
  program sees no A while the dialog is open: `BUTTON` 0 all through on a
  Switch 1 (TEST67).
- With a `DIALOG` open on each side, one press of A closes both in the same
  frame, wherever the focus is (TEST64).
- `ENVFOCUS` moves the focus a few frames later, and `ENVFOCUS()` gives the old
  answer until then: `ENVFOCUS 0` took one to three frames (it varied from run
  to run), `ENVFOCUS 1` one. An `ENVFOCUS` while the focus is moving is lost
  (TTEST37, TTEST38). TTEST37's ten frames after `ENVFOCUS 0` were no frames at
  all: its `VSYNC 10` came behind and went straight on.
- The interpreter that gets the focus is framed for a moment by a yellow
  (255,255,0) border (a screen recording at 24 frames a second):
  - The border is three display pixels on the edge of its screen, one outside
    and two inside, with a black band two pixels wide inside that.
  - The subprogram's frame is its XSUBSCREEN rectangle, the main interpreter's
    the whole display, every time the focus moves, `SUBRUN`'s start and a
    subprogram's end included.
  - Both show whole for about 0.23 seconds and then fade out evenly over about
    0.25 seconds, drawn over what is under them.
  - A frame for the other side, coming before that, takes its place at once.

### A subprogram's screen

- `BACKCOLOR` on a subprogram's screen over the main one sets its color whole,
  and its alpha as how much of the main screen shows through. `&H80FF4000` over
  black shows `&HFF4000`, and over a dimmed `&H77` green `&H40 + &H77*128/255`.
  - Alpha `&HFE` lets nearly all of the main screen through, `&H01` almost none,
    and alpha 0 leaves the screen clear.
  - With alpha `&HFF` the subprogram's screen replaces the main one where it is,
    and layers in modes 3 or 4 on it hide the main screen whatever the alpha
    (TEST56).
  - What the subprogram draws goes over its `BACKCOLOR` as on any screen.
- A subprogram's screen is stretched smoothly whatever `XSCREEN`'s mode, and
  keeps its aspect ratio in `XSUBSCREEN`'s rectangle: 480x272 in mode 1 on the
  whole display is 2.647 times, starting at X 4 (TEST56).
- A screen wider than its rectangle gets bars above and below: 400x240 in a
  rectangle of 600x600 at 60 starts at Y 180 (TEST60).
- Mode 2 is smoothed the same in a subprogram (TEST60).
- The bars take `BACKCOLOR`'s color plus the display times 1 minus its alpha,
  unless the alpha is 0: seen over a main screen that was not cleared, for
  400x240 with `&HC0305070` and 1280x720 with `&H40305070` (TEST62).
- A layer in mode 3 makes the whole subprogram's screen opaque, far from what is
  on it (TEST62).

### `ENVSTAT` and `ENVLOCATE`

- `ENVSTAT OUT S,N,A,SL,C,L` after SmileBASIC starts afresh, before any
  program, gives S 0, N 0, A 0, C -1 and L 0 (TEST64). TEST63, started another
  way, saw status 1 with line 386, of a system program that went before.
- A program that ends without an error sets the status and N to 0 and C to -1,
  and keeps A and L from the error before: A 0 and L 0 after a fresh start, and
  A -1 and L 3 after a `Stack overflow` on line 3 (TEST63, TEST64).
- `ENVSTAT OUT S,N,A,SL,C,L` after the main program's `Divide by zero` gives N
  7, A -1, SL 0, C the statement's character from the start of the program
  counting from 0, and L its line. After a `STOP`, N is 0 and A, SL and C are
  -1, but L keeps the line of the error before.
- `ENVSTAT 0` in a subprogram turns a stopped program's status 1 or 2 to 0, and
  `CONT` can no longer go on with it.
- `ENVSTAT OUT` as TTEST39 saw it:
  - After `LOCATE 0,999`'s `Out of range`, it gives N 10 and A 1, the failing
    argument counted from 0; C is the statement's start.
  - A `GOTO` with nothing after it runs the lines before it and stops with
    `Type mismatch` (N 8) at its statement.
  - An error typed in direct mode after that gives status 1, N 0, A, SL and C
    -1, and L the program error's line.
  - While the editor shows, status 4 comes with N, A, SL, C and L all 0, and
    `ENVLOCATE` gives 0,0.
- A is the argument that the error message names, counted from 0: 1 after
  `Overflow in 1:3(FILL:2)`, 0 after `(HEX$:1)`, and -1 after `(PUSH)`,
  `(COPY)`, `(LEFT$)` or an operator's error (TEST66). TEST67 gave 1 after
  `(MID$:2)`, 2 after `(FILL:3)`, 0 after `(LOAD:1)` and -1 after
  `(UNSHIFT)`.
- `ENVSTAT OUT S,N,A,SL,C,L` after a compile error is set as for an error when
  running, but for the status, which stays 0: S 0, N 3, A -1, C the character of
  the token it names from the start of the program, L its line (TEST62).
- `ENVLOCATE OUT X,Y`, which the manual leaves out, gives the right edge and top
  of the main console's cursor cell on the display. With the 400x240 screen 1200
  pixels wide in its middle, the cursor at column C of row R is at 40+24*(C+1),
  24*R (TTEST37).

## Memory

### `FREEMEM`

- `FREEMEM` is 134216720 at start, and the program and its numeric variables
  take none of it. `FREEMEM OUT A,B` gives the same in both.
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
- Memory let go of (emptying a string or array, a DEF's locals after it returns)
  does not come back at once.
- Dropped strings and arrays are freed on demand (TEST52). A dropped string of
  100,000 characters stays counted in `FREEMEM` after `PRINT`, a loop, two
  `VSYNC`s and `WAIT 60`. Making 16 million characters 20 times over, with or
  without `VSYNC`, takes `FREEMEM` through 100662224, 67107728, 33553232 and
  back to 100662224, and never fails: what was dropped goes when the next
  request would not fit.

### `Out of memory`

- Length is limited by memory only: 63,000,000 characters, 33,000,000 ints and
  16,000,000 reals all work (TEST57).
- `Out of memory` comes from what fits in `FREEMEM`'s 134216720 bytes (TEST51).
  - `"A"*N` and `S$+S$` make 33,554,432 characters but not 67,108,864.
  - `ARRAY#` makes 8,388,608 reals but not 16,777,216.
  - Of five `DIM X%[8000000]`, the fifth fails with 6.2 million bytes free.
  - `RESIZE` and `COPY` grow to 16.7 million integers but not 33.5 million.
- `Out of memory`'s message names the command for functions and commands
  (`Out of memory in 0:68(ARRAY#)`, also `(RESIZE)`, `(INSERT)` and `(COPY)`)
  but not for `*`, `+` or `DIM` (`Out of memory in 0:54`). Its number is 11.

### Blocks that move

- `INSERT` and `RESIZE` growing an array need the new block beside the old:
  `INSERT` of 16,777,216 integers into 15,728,641 fails though the 130 MB it
  comes to would fit alone, and `RESIZE` from 8 to 16 million, 100 MB together,
  works (TEST51).
- `PUSH` is no different: with `DIM A%[15999999]` and `DIM F[8000000]`, 6216592
  free, the first `PUSH` works with `FREEMEM` unchanged, its block being as
  large as it was, and the second is `Out of memory (PUSH)` (TEST57). The block
  moves each time its size changes, and `UNSHIFT`,
  string arrays and strings (each 64 characters) do the same.
- `FREEMEM` overstates what can be had near the end, as a block that moved
  leaves a hole (TEST58). With 1296 bytes free beyond a block's size:
  - `UNSHIFT` on `DIM A%[100000]` (400064 bytes, 401360 free) gets to length
    100016 with 401296 free and then is `Out of memory (UNSHIFT)`.
  - `PUSH` on `DIM A$[100000]` (800064, 801360 free) gets to 100008 with 801296
    free, and then is `Out of memory (PUSH)`.
  - `PUSH` on a string of 512000 characters (1024064, 1025360 free) gets to
    512064 with 1025232 free, and then is `Out of memory (PUSH)`.
- `POP` moves a shrinking block too, and with no room for the new one leaves the
  length as it is, with no error (TEST58). With `DIM A%[1000001]` and 1000016
  free, a `PUSH` and a `POP` stay in the block, the next `POP` leaves the length
  at 1000001 and `FREEMEM` at 1000016, and a `PUSH` after it gives 1000002.
  Twenty `POP`s in a row still leave 1000001. So a block stays only while its
  size is the same, not for 64 bytes more.
- With no room for the smaller block (TEST59):
  - `POP` still gives the last element, the same one each time.
  - `SHIFT` still gives the first, with the rest moved down and the last there
    twice.
  - A string's `POP` and `SHIFT` and an array of reals or strings go the same
    way, and `ENVSTAT` shows nothing.
  - The size makes no difference: `DIM A%[1601]` with 5008 free, too little for
    its next block down of 6464, keeps its length and gives 901 each time with
    `FREEMEM` at 5008.
  - `REMOVE` moves the rest down and then is `Out of memory (REMOVE)`:
    `REMOVE D%,1` on 1000001 ints leaves the length, `D%[0]`=11, `D%[1]`=13,
    `D%[999999]`=901, and the last, `D%[1000000]`, read past the block, was 0
    the one time it was seen.
  - `RESIZE` to smaller is `Out of memory (RESIZE)` having changed nothing.
- A string constant takes no block of its own until it is stored (TEST59). With
  100048 free, `X$=POP("ABC"+"X"*100027+"XYZ")` gives `Z` and leaves 100048, no
  error, where `SHIFT` of it, which needs a copy, is `Out of memory (SHIFT)`.
  Stored in `S$` the same constant takes 200256 bytes.
- `A%=LOADV("DAT:Q")` makes the new array beside the old: 10 MB of file with
  4216464 free is `Out of memory (LOADV)` (TEST57).
- `LOADV "DAT:Q",A%` fills the array, gives no error and leaves `FREEMEM` as it
  was (TEST57). If the file is longer, the array grows the way `RESIZE` does,
  and `A$=LOADV("TXT:Q")` makes its string the same way.
- `LOADV` as TEST58 saw it:
  - `S$=LOADV("TXT:Q")` of 1,000,000 characters (2000064 bytes) over a string as
    long is `Out of memory (LOADV)` with 1000016 free and works with 3000016
    free.
  - `LOADV "DAT:Q",A%` of a file of 9 million integers into `DIM A%[8000000]` is
    `Out of memory (LOADV)` with 34000016 free and works with 38000016 free,
    leaving 34000016.

### Strings worked out as the program compiles

- Constant strings are worked out as the program compiles, so what is too long
  stops it before any of it runs (TEST52, TEST57).
- `?LEN("A"*(1<<N))` works up to N=14 in direct mode, then is
  `Out of code memory`. In a program it works up to N=20, then is
  `Out of memory`, in slots 0 and 1, with nearly all memory free. Through a
  variable the same size works.
- Each statement has 4 MiB to compile in. Every string
  worked out on the way takes 24 bytes and its characters, rounded up to 8
  bytes, from those 4 MiB and stays until the statement ends.
  - `"A"*1000000+"B"*1000000` is too long, as are `?"A"*1500000,"B"*1500000` and
    `A=LEN("A"*2097140)`, each `Out of memory` at its line (TEST58): each
    constant and the rest of the statement take from the same 4 MiB.
  - `A=LEN("A"*2097068)` compiles and prints, and 2097069 and 2097116 are
    `Out of memory` as it compiles (TEST59). That leaves 168 bytes for such a
    statement, none of them for the variable it stores to.
- A string then goes into the code, which is 32768 16-bit words in direct mode
  and 4194304 in a slot, taking a word a character plus its length. Of three
  lines of `A=LEN("A"*2000000)`, the third is `Out of code memory` (TEST58).

## System settings

### `SYSPARAM`

- `SYSPARAM` takes names in lower case too.
- `SYSPARAM("TVMODE")` is 1 on a docked Switch 2 and 0 on a Switch 1 in
  handheld mode (TEST67).
- `SYSPARAM`'s `LANG` is `ja` on a Switch set to Japanese. `SYSBEEP` starts at
  1, `KEYREPEAT` at 20 and 3, and `TOOL1` to `TOOL3` at `#TOOL/SBSMILE.PRG`,
  `#TOOL/GAHAKU.PRG` and nothing.
- `SYSPARAM` items that can only be read can also be read with `OUT`
  (`SYSPARAM "LANG" OUT L$`).
- `SYSPARAM` of an unknown name is `Illegal system argument name`. Setting an
  item that can only be read, or reading `KEYREPEAT` as a value, is
  `Wrong number of OUT arguments`.
- `MOUSECURSOR` reads 0 again in a program run after one that set it to 1. `END`
  leaves it set, so direct mode still reads 1.

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
- `KEY` ignores a string over 256 characters; `PUSHKEY` from 64 is
  `String is too long`.
- `XSUBSCREEN` and the undocumented `UISTATE` are there for the main program
  too: `XSUBSCREEN` does nothing and `UISTATE(0)` is 0.
- `UISTATE(2)` read 648 in direct mode after TEST63's earlier parts had run, and
  0, 1, 3, 4, 5 read 0. (TEST63)
- `UISTATE(5)` is 0, and `UISTATE(6)` `Out of range (UISTATE:1)`. (TEST62)
- `PERFEND()` gives microseconds; `PERFBEGIN` and `PERFEND` show no gauge on
  their own. Without a `PERFBEGIN`, `PERFEND` counts from when SmileBASIC
  started.

## Editor

- F4 after an error puts the cursor at the start of the statement that failed,
  syntax errors included.
- The editor shows 21, 28 or 43 lines of code, in the 8- and the 16-pixel font
  alike.
- The editor colors TO, STEP, CONST and ENUM as reserved words, though the
  language specification does not reserve them, and OPTION as a command.
- Ctrl+Enter makes an empty line below the cursor's and moves to it.
- Ctrl+K puts a quote at column 0 of each line, also before lines that are
  comments already. Only Ctrl+Shift+K looks for one.
- Finding ignores the case of letters.
- Ctrl+/ splits the display side by side, then one above the other, then joins
  it again. Both sides show the slot being edited at first.
- Ctrl+S asks for a name on the on-screen keyboard, titled "Slot 0: Write file",
  with the name the slot was saved as filled in.
- Ctrl+L opens a list of the project's files, with a way up to the projects.
- A list of SmileBASIC's words starting with what is typed shows from the first
  letter while typing in the editor. The program's own names are not in it.
