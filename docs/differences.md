# Where the emulator differs from SmileBASIC 4 on purpose

What the emulator leaves out or does another way, by choice or for want
of SmileBASIC's own data. Guesses about behaviour nobody has checked on
hardware yet are not listed here: they are marked `UNVERIFIED` in the
source, and what has been checked is in [sb4-pitfalls.md](sb4-pitfalls.md).

## Without SmileBASIC's own sounds

SmileBASIC 4 keeps its instruments, sound effects and preset songs in its
sound archive, which the emulator cannot ship. The importer takes them
from your own copy, and the emulator plays them on a sampler and a
sequence player of its own; envelopes, volume curves, vibrato and how
BEEP pans an effect are guesses.

Without them:

- Preset songs (`BGMPLAY 0` to `73`) play silence, so that programs
  waiting on `BGMCHK` still wait: the three short ones (4 to 6) for as
  long as they last, the others, which loop, until stopped.
- Preset sound effects (`BEEP 0` to `155`) are silent. They still take
  a handle.

- MML's sound effect tones (`@256` and up) are silent.
- Instruments `@0` to `@127` come from a General MIDI SoundFont
  (GeneralUser GS, or one chosen in Settings), so they sound different.
- The drum sets `@128` to `@148` are the GS drum kits nearest by name.

## Sound that is not played

- The effector (`EFCEN`, `EFCSET`, `EFCWET`, `BGMWET`) keeps its
  settings but does not change the sound.
- `TALK` speaks with a voice of the emulator's own, a Klatt formant
  synthesizer (klattsch-core) reading the kana or phoneme letters, so it
  says the same words in another voice. In MML, a sung note (`*WA,C4`)
  plays as a plain note.
- MML's tremolo (`@MA`) and pan modulation (`@ML`) are read and
  ignored. Vibrato (`@MP`) takes only its depth. Envelopes (`@E`) shape
  the PSG and user waveforms only.
- `SNDMSBAL` balances a subprogram's sound against the main program's
  in a straight line, the full sound of both at 64.

## Input

- There are no motion sensors: `ACCEL`, `GYROA` and `GYROV` read 0.
- Nothing vibrates. `VIBRATE()` still reads back the pattern or
  frequencies last asked for, until `VIBRATE` stops them.
- The IR Motion Camera starts and stops (`IRSTART`, `IRSTOP`, `IRSTATE`)
  but never sees anything: `IRREAD` gives no clusters, or 48 dark
  blocks in moment mode, and `IRSPRITE` shows no image.
- Toy-Con set up with `XCTRLSTYLE` are never recognized: `TCPIANO` and
  the other `TC` commands answer 0 for everything.
- The mouse's left button is the touch screen (touch 0 only).
- Controller 1 is the keyboard and the first gamepad together, reported
  as a handheld controller; other gamepads are Pro Controllers.
  `XCTRLSTYLE` shows no connection screen and always succeeds.
- The keyboard also presses controller 1's buttons; Run > Keys press
  buttons turns this off.
- A program that hides the mouse cursor (`SYSPARAM "MOUSECURSOR",0`,
  as programs start) hides the computer's cursor over the window, until
  the menu bar shows; Settings can keep it shown.

## System settings

- `SYSPARAM("LANG")` answers `en-US`, as the English edition does, or
  `ja`, as set in File > Settings > General.
- `SYSPARAM("TABSTEP")` is the editor's tab width, set in File > Settings >
  Editor.
- `SYSPARAM("TVMODE")` is always 1, and `SYSBEEP` keeps its value: the
  emulator has no system sounds. `TOOL1` to `TOOL3` name SmileBASIC 4's
  own tools, which the emulator does not have.

## Other

- What a program writes to a `GARRAY` array shows on the screen only
  after `GUPDATE`; `GPGET` and other commands that read the page see it
  at once, as on SmileBASIC 4. There, drawing commands near the writes
  also bring them to the screen.
- `FREEMEM` counts what strings and arrays take as SmileBASIC 4 does,
  but memory a program lets go of is free again at once. SmileBASIC 4
  frees it later: emptying a string or an array, or leaving a DEF, does
  not raise `FREEMEM` there straight away.
- Error messages add detail in parentheses, such as which argument was
  out of range.
- File commands ask, notify or stay silent as set in Settings, rather
  than always asking. A file command that fails in a program shows a
  notice, rather than a dialog to close, unless file access is silent.
- `SAVE` and `SAVEV "TXT:..."` write plain UTF-8 text, and `SAVEG` and
  `SAVEV "GRP:..."` write PNG images, so the files open on a computer.
  `DAT` files hold SmileBASIC 4's array without its container. All of
  these are read back, as are files from SmileBASIC 4. The project
  manager's Export writes a project in SmileBASIC 4's own files, and
  Import reads them back into the emulator's.
- `METASAVE` keeps a project's title and description in `@META.TXT`
  and its icon in `@ICON.PNG`, in the project's folder. `METALOAD`
  reads SmileBASIC 4's own `META` file when those are missing.
- After `BACKTRACE`, `CONT` and `TRACE` still go on with the stopped
  program; on SmileBASIC 4 they cannot. Programs cannot tell, since all
  three work in direct mode only.
- Subprograms have no Smile button to start them from, only `SUBRUN` in
  direct mode. Run > "Switch keys to/from subprogram" moves the focus as
  `ENVFOCUS` does, and Run > "Stop subprogram" does `SUBSTOP`, so a
  subprogram cannot keep the keys from direct mode.
- `ENVSTAT` gives -1 for the argument of every error.
- Settings > General > "Localized error messages" prints the Japanese
  line SmileBASIC 4's Japanese edition shows under each error message,
  in the English edition too.
- `ENVLOCATE OUT X,Y`, which is not in the manual, follows the
  console's cursor also while the editor is open.
- `HELPINFO OUT` looks for a command where the editor's cursor was when
  the program started, since no subprogram runs beside the editor. It
  and `HELPGET` need SmileBASIC 4's help imported.
- `DIALOG` is drawn as the emulator's own window, in English. Besides
  the controller's A and B, it answers clicks and Enter.
- `PRGEDIT` leaves the editor's line wrapping as it is: the PRG commands
  count lines of the source, which wrapping does not change.
- The screen's scaling filter can be chosen in View > Screen filter instead of
  following `XSCREEN`.
- On a layer with `LMATRIX`'s 4x4 form, text screens show only as far as
  one screen past each edge of the screen, before the matrix moves them,
  to keep the memory they take bounded. SmileBASIC 4 shows them
  wherever they are, as it does sprites.

## Editor

- The editor's colors start as SmileBASIC 4's, and can be swapped for
  preset palettes from other editors, typed in as hex values, or loaded
  from and saved to text files. Selected text has a color of its own.
- Besides SmileBASIC 4's three sizes (21, 28 and 43 lines of code), the
  editor has a smaller one with 57, about 60 rows with the ruler, the
  status line and the room left for the menu bar.
- The help opens beside the editor rather than over it, at one of three
  text sizes (F2 while it has the keys), and clicking the code gives the
  keys back while it stays open.
- The emulator adds an index of the program's labels and DEFs to jump
  to (Ctrl+J), an overview of the program behind the code's right side,
  and scrolling with the mouse wheel.
- The list of words to complete also shows in direct mode, and can
  offer the program's own names (off at first, as in SmileBASIC 4).
  Each entry is tagged with what it is, and the first form the help
  gives for it shows at the bottom of the screen. After GOTO, GOSUB and RESTORE it
  lists the program's labels, and after `#` the constants with their
  values.
- While a command's arguments are typed, every form the help gives for
  it (or a DEF's own line) shows at the bottom of the screen (at the top
  while the cursor is down there, or by the cursor if set so), with the
  argument being typed marked. Ctrl+Space hides the list and the hints
  until it is pressed again. When they show and what they offer are
  set in File > Settings > Editor.
- Ctrl+S takes the file name typed on the computer's keyboard rather
  than SmileBASIC 4's on-screen one.
- With View > Editor in its own window, the editor opens in a second
  window beside the screen, and programs and direct mode go on while it
  is open. Its edits reach the slot at once, so RUN, SAVE and the PRG
  commands see them, and a slot that a program or LOAD changes shows
  the change in the editor. CONT and the debugger's commands leave it
  open. In SmileBASIC 4 the editor replaces the screen and no program
  runs while it shows.

## Speed

SmileBASIC 4 runs a fixed amount of code each frame (see
[sb4-pitfalls.md](sb4-pitfalls.md)); the emulator runs code that does not
wait for the next frame for as long as the frame leaves after drawing,
which is far more on any recent computer. Programs time themselves with
`VSYNC`, `WAIT` and `MAINCNT`, so none needs the slower speed. Headless
runs (`smilebasic-check`, the tests) instead run a fixed million
instructions a frame, so that they come out the same each time.

When drawing a frame takes longer than a frame, the window skips
drawing up to three frames while the program and its sound go on, so
that games and their music keep time.
