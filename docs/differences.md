# Where the emulator differs from SmileBASIC 4 on purpose

What the emulator leaves out or does another way, by choice or for want
of SmileBASIC's own data. Guesses about behaviour nobody has checked on
hardware yet are not listed here: they are marked `UNVERIFIED` in the
source, and what has been checked is in [sb4-pitfalls.md](sb4-pitfalls.md).

## Without SmileBASIC's own sounds

SmileBASIC 4 keeps its instruments, sound effects and preset songs in its
sound archive, which the emulator cannot ship. The importer takes them
from your own copy, and the emulator plays them on a sampler and a
sequence player of its own. Envelopes, volume curves and the LFOs follow
SmileBASIC 4's sound engine and recordings made on it; how BEEP pans an
effect is a guess.

Without them:

- Preset songs (`BGMPLAY 0` to `73`) play silence, so that programs
  waiting on `BGMCHK` still wait: the three short ones (4 to 6) for as
  long as they last, the others, which loop, until stopped.
- Preset sound effects (`BEEP 0` to `155`) are silent. They still take
  a handle. So are `BEEP 224` to `255` and `@224` to `@255` in MML with nothing
  set: on SmileBASIC 4 they are the sound archive's sine.

- MML's sound effect tones (`@256` and up) are silent.
- Instruments `@0` to `@127` come from a General MIDI SoundFont
  (GeneralUser GS, or one chosen in Settings), so they sound different.
- The drum sets `@128` to `@148` are the GS drum kits nearest by name.

## Limits

- `GOSUB`s, `DEF` calls, `RESTORE "PUSH"` and the values of expressions share one stack on
  SmileBASIC 4. Here the first three count as there, and values of expressions not at all,
  so a deep call ending in a long expression can go a few levels further. For `RESTORE
  "PUSH"`, 1.5 slots each is inferred.

## Sound that is not played

- Waveforms are read without the console's resampling, whose ringing lifts a
  square's peak over its level: `BEEP` and `BGMPLAY` of a square record the same
  RMS as SmileBASIC 4 but peak about 12% lower (4096 to its 4623), and a sine
  about the same as there. `RECSAVE` of a program that looks at peaks shows this.
- `BEEPSTOP` without a fade, and a `BEEP` that takes over a handle, cut a preset
  effect at once where a waveform's notes fall by their release; by SmileBASIC 4 all of them are let go the same way, so an effect's tail goes on there.
  Past about 48 notes piled up by repeating sounds, or 48 falling once their handles
  have gone, the oldest make way, and a note too quiet to reach a 16-bit sample
  ends there: SmileBASIC 4 has 96 voices shared with BGM.
- The effector (`EFCEN`, `EFCSET`, `EFCWET`, `BGMWET`) is a small reverb
  of the emulator's own, early reflections and a Freeverb-like tail,
  given the settings SmileBASIC 4 gives the console's reverb: the
  presets and `EFCSET`'s nine values change it alike, but it does not
  sound like the console's. What `RECSTART` records is as on SmileBASIC
  4, where the effector does not reach the recording either.
- `TALK` speaks with a voice of the emulator's own, a Klatt formant
  synthesizer (klattsch-core) reading the kana or phoneme letters, so it
  says the same words in another voice. In MML, a sung note (`*WA,C4`)
  sings with that voice at the note's pitch, one voice to a channel,
  instead of the channel's instrument.
- With a SoundFont's instruments, MML's tremolo (`@MA`) and pan
  modulation (`@ML`) are ignored, vibrato (`@MP`) takes only its depth
  on the modulation wheel, and envelopes (`@E`) do nothing. SmileBASIC's
  own instruments and the PSG and user waveforms take them all.
- A note whose envelope decays to a sustain of 0 ends there, at -72.3
  dB, where SmileBASIC 4 holds it until it is let go: drums that ignore
  being let go would otherwise sound, unheard, for ever.
- `SNDMSBAL` balances a subprogram's sound against the main program's
  in a straight line, the full sound of both at 64.

## Sound limits

Limits and errors the emulator adds so that a program or a damaged file
cannot freeze or exhaust it. SmileBASIC 4's own answers to these are not
known.

- `BGMPLAY` and `BGMSET` take macros that copy up to about four million
  characters for one song, counting a macro's body each time it is
  defined or used; past that the MML is `Illegal MML`. SmileBASIC 4 has no
  such limit in practice and hangs for good on nine nested levels, which
  is not worth copying. The largest use in any song seen is about 9,000,
  and MML without macros has no such limit. `CHKMML` does not apply it: it
  reads each macro's body once and gives -1 as SmileBASIC 4 does, but for
  an error in such MML the place it gives counts from that shorter text.
- A preset sequence (from the importer's `sequences.bin`) that runs more
  than 20,000 commands and sounding notes in one frame is stopped.
  SmileBASIC 4's own sequences peak at 195.

## Input

- There are no motion sensors: `ACCEL`, `GYROA` and `GYROV` read 0.
- Nothing vibrates. `VIBRATE()` still reads back the pattern or
  frequencies last asked for, until `VIBRATE` stops them.
- The IR Motion Camera starts and stops (`IRSTART`, `IRSTOP`, `IRSTATE`)
  but never sees anything: `IRREAD` gives no clusters, or 48 dark
  blocks in moment mode, and `IRSPRITE` shows no image.
- Toy-Con set up with `XCTRLSTYLE` are never recognized: `TCPIANO` and
  the other `TC` commands answer 0 for everything.
- Pasting into a line goes through the key buffers without losing keys,
  however long the text, 32 characters a frame. On the keyboard, Tab takes the
  word from completion's list when one shows; with none it is a space, as in
  SmileBASIC 4.
- The mouse's left button is the touch screen (touch 0 only).
- Controller 1 is the keyboard and the first gamepad together, reported
  as a handheld controller; other gamepads are Pro Controllers.
  `XCTRLSTYLE` shows no connection screen and always succeeds.
- Text pasted at the prompt runs line by line, though SmileBASIC 4's prompt
  drops keys waiting for it, as `PUSHKEY` shows.
- The keyboard also presses controller 1's buttons; Run > "Use the
  keyboard as a controller" turns this off.
- A program that hides the mouse cursor (`SYSPARAM "MOUSECURSOR",0`,
  as programs start) hides the computer's cursor over the window, until
  the menu bar shows; Settings can keep it shown. `MOUSE X,Y` moves the
  computer's pointer too; Settings can keep it where it is (the program
  then sees the move until the next frame).

## System settings

- `SYSPARAM("LANG")` answers `en-US`, as the English edition does, or
  `ja`, as set in File > Settings > General.
- `SYSPARAM("TABSTEP")` is the editor's tab width, set in File > Settings >
  Editor.
- `SYSPARAM("TVMODE")` is always 1, and `SYSBEEP` keeps its value: the
  emulator has no system sounds. `TOOL1` to `TOOL3`, which F9 to F11
  start, name SmileBASIC 4's own tools. The emulator does not have
  them: they start only from the `#TOOL` project that the importer copies
  from SmileBASIC 4 into the projects folder. A tool that is not there,
  or a key with no tool, shows a notice.

## Other

- Where a turned or scaled sprite's or text's sample point lands exactly
  between two texels, SmileBASIC 4's result comes from the GPU's
  arithmetic, and can change along a sprite's diagonal or with where it
  is on the screen. The emulator takes one rule for each case, the most
  usual outcome (`Placement::texel_tie`), so a few of these pixels show
  the neighbouring texel. It also never reads the page just outside the
  image at such a tie, which SmileBASIC 4 can do.
- What a program writes to a `GARRAY` array shows on the screen only
  after `GUPDATE`; `GPGET` and other commands that read the page see it
  at once, as on SmileBASIC 4. There, drawing commands near the writes
  also bring them to the screen.
- `FREEMEM` counts what strings and arrays take as SmileBASIC 4 does,
  but memory a program lets go of is free again at once. SmileBASIC 4
  frees it only when a request would not fit: emptying a string or an array,
  or leaving a DEF, does not raise `FREEMEM` there, even after `VSYNC` or
  `WAIT`. A program would have to watch `FREEMEM` to see the difference.
- `Out of memory` and `Out of code memory` for constant strings as a program
  compiles count those strings and the constants they are made from.
  SmileBASIC 4's limits take in the rest of the statement and its code too,
  which here is a word an instruction and no more than that, so right at the
  limits a program may compile that SmileBASIC 4 refuses, but not the other way
  round: `A=LEN("A"*2097116)` compiles here, and there 2097068 is the most. A
  constant string that compiles is still made when its line runs, which is
  where the memory it takes may run out, though on SmileBASIC 4 it takes none
  until stored. `POP` and `SHIFT` of a constant are as there.
- `Out of memory` comes when what a program holds, counted as `FREEMEM`
  counts it, and what it asks for do not fit in SmileBASIC 4's
  134,216,720 bytes. Values a program has dropped never count, which comes to
  the same as SmileBASIC 4 freeing them when a request would not fit. As there,
  a block that changes size, growing or shrinking, needs the new one to fit
  beside the old. But SmileBASIC 4's blocks lie in one heap, and one that moved
  leaves a hole that a larger one cannot use, so nearly full it can run out
  earlier than here, some dozens of elements earlier in the `UNSHIFT` and `PUSH`
  that `docs/sb4-pitfalls.md` gives. A program that works there works here. Each
  interpreter has that much to itself, as `FREEMEM` shows.
  Strings under 64 KiB are not looked at one by one: what they add up to is
  refused at the next count, a little later than SmileBASIC 4 might. Nor are
  blocks under 64 KiB as they shrink: where SmileBASIC 4's `POP` and `SHIFT` with
  no room for the smaller block leave the length as it is, or its `REMOVE` and
  `RESIZE` stop with `Out of memory`, here they always go ahead. `DIM A%[1601]`
  with 5008 free keeps 1601 there and gives the last element each time; here it
  shrinks and gives each in turn.
- `LOAD` of a program takes nothing from this memory.
- Run on a project in the project manager returns to the manager when
  the program ends or is stopped. After an error the screen stays, with
  the emulator's own line `=== Press A Button or Enter to return.===`
  below it, until A (any controller) or Enter is pressed.
- Error messages add detail in parentheses, such as which argument was
  out of range.
- `GSAVE` and `GLOAD` with a rectangle of more pixels than SmileBASIC
  4's memory could hold give `Out of range`, not `Out of memory`.
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
  direct mode and the tools on F9 to F11. Run > "Switch keys to/from subprogram" moves the focus as
  `ENVFOCUS` does, and Run > "Stop subprogram" does `SUBSTOP`, so a
  subprogram cannot keep the keys from direct mode.
- Run > "Clear screen and sound when a program ends", off to start
  with, does `ACLS` and `SNDSTOP` after a program ENDs or is stopped
  with the Break key or F5. STOP, errors and the debugger's stops leave
  the screens and sound as they were, to be looked at.
- `ENVSTAT`'s OUT form at the start, before any program, gives status 0 and line 0. On
  SmileBASIC 4 it is status 1 and the line of a system program that ran before, which the
  emulator does not run. `UISTATE` always reads 0, where `UISTATE(2)` read 648 on a Switch 2,
  meaning unknown. `CONTROLLER(0)` is 1, handheld, where a Switch 2 read 2.
- `ENVSTAT` gives the wrong argument's number, from 0, for errors that
  come from checking one argument, such as `LOCATE 0,999`'s `Out of
  range` (1, as SmileBASIC 4 gives). Errors found by checking several
  arguments together give -1, which SmileBASIC 4 may number.
- Settings > General > "Japanese explanation under errors" prints the Japanese
  line SmileBASIC 4's Japanese edition shows under each error message,
  in the English edition too.
- Insert's overwrite setting is kept apart for the typed line and for each
  side of the editor. SmileBASIC 4 has one for all of them.
- `ENVLOCATE OUT X,Y`, which is not in the manual, follows the
  console's cursor while the editor is open in its own window, where
  SmileBASIC 4 would give 0,0 for the editor.
- `HELPINFO` and `HELPGET` need SmileBASIC 4's help imported.
  `HELPINFO OUT` gives the topic at the editor's cursor only to
  programs started from `#SYS`, as SmileBASIC 4's own help is; others
  get `DEFAULTHELPKEY`, as on hardware, unless Settings > General > "Allow
  system-only features in every program" is on. It looks where the cursor was
  when the program started, or where it is now for a subprogram.
- `DIALOG` is drawn as the emulator's own window, in English. Besides
  the controller's A and B, it answers clicks and Enter.
- `PRGEDIT` leaves the editor's line wrapping as it is: the PRG commands
  count lines of the source, which wrapping does not change.
- The screen's scaling filter can be chosen in View > Screen filter instead of
  following `XSCREEN`.
- On a layer with `LMATRIX`'s 4x4 form, text screens show only as far as
  one screen past each edge of the screen, before the matrix moves them,
  to keep the memory they take bounded. SmileBASIC 4 shows them
  wherever they are, as it does sprites. Being drawn flat first, their
  partly clear or tinted pixels are also rounded once more than
  SmileBASIC 4 rounds them, which can make a channel 1 off.
- A subprogram's screen under `FADE`, or darkened while the main
  interpreter has the focus, is rounded to 8 bits before it goes over
  the main screen; SmileBASIC 4 does both in one step, so a channel can
  be 1 off where the subprogram's screen is not opaque.

## Files from elsewhere

Projects get copied in by hand and imported, so the emulator treats the
files in them as untrusted, where SmileBASIC 4 has a closed file system.
What it refuses fails like any other file that cannot be read or written:
`RESULT()` is 0 and the message says why.

- Symbolic links among the projects or inside one are not followed, read,
  written through or copied: a linked project is no project and a linked
  file is not there. The projects folder itself may be a link.
- Files over 256 MiB are not read or saved, whichever command asks, and
  show nothing when chosen in the projects window. SmileBASIC 4's own
  limit is its memory.
- `LOADV` and `LOADG` refuse an array file whose non-zero sizes multiply
  to over 2^28, also with a zero among them (`[0xFFFFFFFF,0]`), so that
  nothing that goes through its rows can run out of memory. What SmileBASIC
  4 does with such a file is not known; `[0,3]` and `[3,0]` load on both.
- Image files of no pixels are refused rather than loaded empty.
- Names Windows cannot keep are refused on Windows alone: `CON`, `PRN`,
  `AUX`, `NUL`, `COM0` to `COM9` and `LPT0` to `LPT9` for projects, also
  with an extension, and any name ending in a dot.

If the emulator itself fails while a program runs, it stops the program
and prints `Internal Error` with the reason, keeping the slots and the
editor's text. SmileBASIC 4 has an error of that name, but what it does
after one is not known. The stopped program cannot go on with `CONT`.

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
- With View > Editor in a separate window, the editor opens in a second
  window beside the screen, and programs and direct mode go on while it
  is open. Its edits reach the slot at once, so RUN, SAVE and the PRG
  commands see them, and a slot that a program or LOAD changes shows
  the change in the editor. CONT and the debugger's commands leave it
  open; a step that waits for input brings the screen's window to the
  front for typing, and the editor's back when it stops. With Settings
  > Editor > "Remember the editor window's position and size" the window opens where it
  was last. In SmileBASIC 4 the editor replaces the screen and no
  program runs while it shows. A subprogram's
  screen shows over the screens rather than the editor's window, but
  what it types with `PUSHKEY` still goes into the editor while that
  window has the keys.

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
that games and their music keep time. A frame it does draw shows the
scene the program finished: the program stops at a `VSYNC` that is behind
when another trip round its main loop would not fit in the frame, where
SmileBASIC 4 goes straight on, and the drawing waits up to three frames
for a program part-way through its loop. When even that cannot keep up,
the program's loop slows down rather than the drawing showing half a
scene.

## Filters and display scaling

- A filter's result goes to 8 bits before the layer's colour is multiplied
  in. SmileBASIC 4 apply both in one draw, with one
  rounding, so a tinted, filtered layer can be 1 off.
- `XSCREEN`'s mode 1 applies to the program's screen. The editor's frame uses
  SDL's pixel art filter.
- `WAVSET` with `[..]` groups leaves silent the part of the 512 samples that
  the last group does not fill, where SmileBASIC 4 leaves whatever its stack
  held.
- The Options menu's sharp screen filter scales by whole factors, which no
  `XSCREEN` mode does. It is the default; following `XSCREEN` is another of
  its choices.
