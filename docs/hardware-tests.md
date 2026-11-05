# Hardware tests

Behavior the manual leaves open is checked by running small programs on
SmileBASIC 4 and comparing what they show with the emulator. Confirmed
results go into [sb4-pitfalls.md](sb4-pitfalls.md).

Under an emulator such as Ryujinx, the files a program saves can be read
from the save data on the host. Check programs use that: each part saves
the console's text, and `smilebasic-check` runs the same parts on the
emulator and compares the two screens row by row.

A real console can run these programs too. Where the docs say so, a result
from a real Switch outranks Ryujinx's.

The TEST and TTEST numbers in the docs name check programs kept outside the
repository. TTEST, which stops at TTEST39, is the older series; the later
programs are TEST40 and up.

## Writing a check program

One program holds numbered parts, each fitting one 50x30 console screen.
The main code asks for the part, runs it as a subroutine and saves the
screen into a TXT file whose name starts with `RES`:

```smilebasic
ACLS
INPUT "PART";P
ON P-1 GOSUB @P1,@P2
DUMPCON "RES12P"+STR$(P)
END
@P1
?"[";FORMAT$("%5.2F",3.14159);"]"
RETURN
@P2
?RND(0)
RETURN
DEF DUMPCON N$
 VAR S$,L$,X,Y,C
 FOR Y=0 TO 29
  L$=""
  FOR X=0 TO 49
   C=CHKCHR(X,Y)
   IF C==0 THEN C=32
   L$=L$+CHR$(C)
  NEXT
  S$=S$+L$+CHR$(10)
 NEXT
 SAVEV "TXT:"+N$,S$
END
```

- A part that stops with an error saves nothing. Its screen still has to
  be read off SmileBASIC 4's display; `smilebasic-check` prints the
  emulator's console for it.
- Keep one risky error check per part, and avoid constant expressions
  that fail: SmileBASIC 4 rejects them when compiling, which stops every
  part.
- SmileBASIC 4 asks before each SAVEV in a program; answer yes.

## Running

1. Put the program into a project in SmileBASIC 4's save data, packed as a
   signed TXT file (see [file-formats.md](file-formats.md)). Ryujinx keeps
   two copies of the save data and copies the working one over the other
   while SmileBASIC 4 runs, so put the file into both.
2. In SmileBASIC 4, load the program and run each part.
3. Compare:

   ```
   smilebasic-check PROJECT_FOLDER TEST12 1-26
   ```

The arguments:

- `PROJECT_FOLDER`: the project's folder in the save data.
- `TEST12`: the program's name as SmileBASIC 4 loads it.
- `1-26`: the parts to run, as a range or a list (`1-26`, `2,5,7`).

For each `RES` file, `smilebasic-check` prints `same` or the rows that differ.
Where SmileBASIC 4 has not saved one yet, it prints the emulator's screen, to
compare by eye.

Each part runs on a fresh copy of the folder. The projects beside it are copied
once, `###` as the workspace, for parts that read them.

The part's number is typed after three seconds, as a person would. A `VSYNC`
right after `INPUT` counts from the `VSYNC` before it, so how long the answer
takes changes how long it waits.

## Comparing pictures

With `--screen DIR`, `smilebasic-check` also saves the emulator's screen as a
PNG for each part two seconds in, and then presses Enter.

A part that draws a picture should wait for a key or button before it ends,
since SmileBASIC 4 shows a status bar once a program has ended.

Compare the PNG with a lossless screenshot of SmileBASIC 4 at the same size (for
1:1, `XSCREEN 1280,720` and a 1280x720 screenshot).
