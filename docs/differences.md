# Where the emulator differs from SmileBASIC 4 on purpose

This file lists what the emulator leaves out or does another way, by choice or
because it lacks SmileBASIC 4's own data. Guesses about behavior nobody has
checked on hardware are not listed: they are marked `UNVERIFIED` in the source.
What has been checked is in [sb4-pitfalls.md](sb4-pitfalls.md).

## Contents

- [Sound](#sound)
- [Input](#input)
- [Speed](#speed)
- [Memory and stack](#memory-and-stack)
- [Rendering and GPU](#rendering-and-gpu)
- [Screen scaling](#screen-scaling)
- [System values](#system-values)
- [Direct mode, debugger and project manager](#direct-mode-debugger-and-project-manager)
- [Subprograms](#subprograms)
- [Dialogs and errors](#dialogs-and-errors)
- [Files](#files)
- [Editor](#editor)

## Sound

### Without the sound archive

SmileBASIC 4 keeps its instruments, sound effects and preset songs in its sound
archive, which the emulator cannot ship. The importer takes them from your own
copy, and the emulator plays them on a sampler and a sequence player of its own.
Envelopes, volume curves and the LFOs follow SmileBASIC 4's sound engine and
recordings made on it; how BEEP pans an effect is a guess.

Without the imported archive:

- Preset songs (`BGMPLAY 0` to `73`) play silence, so programs waiting on
  `BGMCHK` still wait. The three short ones (4 to 6) last as long as they do
  with sound; the others, which loop, last until stopped.
- Preset sound effects (`BEEP 0` to `155`) are silent. They still take a handle.
- `BEEP 224` to `255`, and `@224` to `@255` in MML, are silent with nothing set.
  On SmileBASIC 4 they play the sound archive's sine.
- MML's sound effect tones (`@256` and up) are silent.
- Instruments `@0` to `@127` come from a General MIDI SoundFont (GeneralUser GS,
  or one chosen in File > Settings > Sound), so they sound different.
- The drum sets `@128` to `@148` are the GS drum kits nearest by name.

### Playback

- Waveform peaks: the emulator reads waveforms without the console's resampling,
  whose ringing lifts a square's peak over its level. `BEEP` and `BGMPLAY` of a
  square record the same RMS as SmileBASIC 4 but peak about 12% lower (4096 to
  its 4623). A sine peaks about the same on both. `RECSAVE` of a program that
  looks at peaks shows this.
- `BEEPSTOP` without a fade, and a `BEEP` that takes over a handle, cut a preset
  effect at once, while a waveform's notes fall by their release. All of them are let go the same way, so an effect's tail goes on in
  SmileBASIC 4.
- Note count: past about 48 notes piled up by repeating sounds, or 48 falling
  once their handles have gone, the oldest make way. A note too quiet to reach a
  16-bit sample ends at that point. SmileBASIC 4 has 96 voices shared with BGM.
- Sustain of 0: a note whose envelope decays to a sustain of 0 ends at -72.3 dB.
  SmileBASIC 4 holds it until it is let go. Drums that ignore being let go would
  otherwise sound, unheard, forever.
- `WAVSET` with `[..]` groups leaves silent the part of the 512 samples that the
  last group does not fill. SmileBASIC 4 leaves whatever its stack held.
- `SNDMSBAL` balances a subprogram's sound against the main program's in a
  straight line, the full sound of both at 64.
- SoundFont instruments: MML's tremolo (`@MA`) and pan modulation (`@ML`) are
  ignored, vibrato (`@MP`) takes only its depth on the modulation wheel, and
  envelopes (`@E`) do nothing. SmileBASIC 4's own instruments and the PSG and
  user waveforms take them all.
- The effector (`EFCEN`, `EFCSET`, `EFCWET`, `BGMWET`) is a small reverb of the
  emulator's own: early reflections and a Freeverb-like tail. It takes the
  settings SmileBASIC 4 gives the console's reverb, so the presets and
  `EFCSET`'s nine values change it alike, but it does not sound like the
  console's. `RECSTART` records as on SmileBASIC 4, where the effector does not
  reach the recording either.
- `TALK` speaks with a voice of the emulator's own, a Klatt formant synthesizer
  (klattsch-core) reading the kana or phoneme letters, so it says the same words
  in another voice.
- Sung notes in MML (`*WA,C4`) sing with that voice at the note's pitch, one
  voice to a channel, instead of the channel's instrument.

### Limits on MML and sequences

The emulator adds these limits and errors so that a program or a damaged file
cannot freeze or exhaust it. SmileBASIC 4's own answers to these are not known,
apart from the macro hang below.

- Macros in `BGMPLAY` and `BGMSET` copy up to about four million characters for
  one song, counting a macro's body each time it is defined or used. Past that
  the MML is `Illegal MML`. The largest use in any song seen is about 9,000, and
  MML without macros has no such limit. SmileBASIC 4 has no such limit in
  practice and hangs for good on nine nested levels, which is not worth copying.
- `CHKMML` does not apply the macro limit. It reads each macro's body once and
  gives -1 as SmileBASIC 4 does, but for an error in such MML the place it gives
  counts from that shorter text.
- A preset sequence (from the importer's `sequences.bin`) that runs more than
  20,000 commands and sounding notes in one frame is stopped. SmileBASIC 4's own
  sequences peak at 195.

## Input

### Hardware the emulator lacks

- Motion sensors: the emulator has none. `ACCEL`, `GYROA` and `GYROV` read 0.
- Vibration: nothing vibrates. `VIBRATE()` still reads back the pattern or
  frequencies last asked for, until `VIBRATE` stops them.
- The IR Motion Camera starts and stops (`IRSTART`, `IRSTOP`, `IRSTATE`) but
  never sees anything: `IRREAD` gives no clusters, or 48 dark blocks in moment
  mode, and `IRSPRITE` shows no image.
- Toy-Con set up with `XCTRLSTYLE` are never recognized: `TCPIANO` and the other
  `TC` commands answer 0 for everything.

### Controllers, keyboard and mouse

- Controllers: controller 1 is the keyboard and the first gamepad together,
  reported as a handheld controller. Other gamepads are Pro Controllers.
  `XCTRLSTYLE` shows no connection screen and always succeeds.
- Keyboard as buttons: the keyboard also presses controller 1's buttons;
  Run > "Use the keyboard as a controller" turns this off. A key the program
  reads itself presses no button until a second after the program stops reading
  it, so that one press doesn't act twice. `KEYBOARD` keeps the key it asks
  about, and `INKEY$` keeps them all. A game that calls `INKEY$` every frame and
  takes everything else from `BUTTON` therefore needs a gamepad.
- `INKEY$` gets nothing from buttons the keyboard presses; the keys themselves
  do the typing.
- Controllers as keys: controllers type keys only as SmileBASIC 4's assignment
  does in every mode. The emulator leaves out the assignments for direct mode
  and the editor alone (X for the editor or to run) and the key settings of
  SmileBASIC 4's options.
- A held button or stick, when one of the emulator's own screens closes, reads
  as let go until it is. The A that chose Run in the project manager is one such
  press. It types nothing and `BUTTON` does not show it, in the program or at
  the prompt.
- Mouse buttons: the left button is the touch screen (touch 0 only).
- Mouse cursor: a program that hides the mouse cursor
  (`SYSPARAM "MOUSECURSOR",0`, as programs start) hides the computer's cursor
  over the window, until the menu bar shows. `MOUSE X,Y` moves the computer's
  pointer too. File > Settings > Input > "Ignore mouse cursor commands" keeps
  the cursor shown and where it is; the program then sees the move until the
  next frame.

### Pasting and Tab

- Pasting into a line goes through the key buffers without losing keys, however
  long the text, 32 characters a frame.
- Pasting at the prompt runs the text line by line, though SmileBASIC 4's prompt
  drops keys waiting for it, as `PUSHKEY` shows.
- Tab on the keyboard takes the word from completion's list when one shows. With
  no list it is a space, as in SmileBASIC 4.

## Speed

SmileBASIC 4 runs a fixed amount of code each frame (see
[sb4-pitfalls.md](sb4-pitfalls.md)). The emulator runs code that does not wait
for the next frame for as long as the frame leaves after drawing, which is far
more on any recent computer. Programs time themselves with `VSYNC`, `WAIT` and
`MAINCNT`, so none needs the slower speed. Headless runs (`smilebasic-check`,
the tests) instead run a fixed million instructions a frame, so that they come
out the same each time.

When drawing a frame takes longer than a frame, the window skips drawing up to
three frames while the program and its sound go on, so that games and their
music keep time. A frame it does draw shows the scene the program finished:

- A `VSYNC` that is behind stops the program when another trip round its main
  loop would not fit in the frame. SmileBASIC 4 goes straight on.
- The drawing waits up to three frames for a program part-way through its loop.
- When even that cannot keep up, the program's loop slows down rather than the
  drawing showing half a scene.

## Memory and stack

- The stack: `GOSUB`s, `DEF` calls, `RESTORE "PUSH"` and the values of
  expressions share one stack on SmileBASIC 4. The emulator counts the first
  three as SmileBASIC 4 does and values of expressions not at all, so a deep
  call ending in a long expression can go a few levels further. For
  `RESTORE "PUSH"`, 1.5 slots each is inferred.
- `FREEMEM` counts what strings and arrays take as SmileBASIC 4 does, but memory
  a program lets go of is free again at once. SmileBASIC 4 frees it only when a
  request would not fit: emptying a string or an array, or leaving a DEF, does
  not raise `FREEMEM` on SmileBASIC 4, even after `VSYNC` or `WAIT`. A program
  would have to watch `FREEMEM` to see the difference.
- `GSAVE` and `GLOAD` with a rectangle of more pixels than SmileBASIC 4's memory
  could hold give `Out of range`, not `Out of memory`.

### `Out of memory` while a program runs

- The limit: `Out of memory` comes when what a program holds, counted as
  `FREEMEM` counts it, and what it asks for do not fit in SmileBASIC 4's
  134,216,720 bytes. Each interpreter has that much to itself, as `FREEMEM`
  shows.
- Dropped values never count, which comes to the same as SmileBASIC 4 freeing
  them when a request would not fit.
- A block that changes size, growing or shrinking, needs the new one to fit
  beside the old, as on SmileBASIC 4.
- Holes in the heap: SmileBASIC 4's blocks lie in one heap, and one that moved
  leaves a hole that a larger one cannot use. Nearly full, SmileBASIC 4 can run
  out earlier than the emulator, some dozens of elements earlier in the
  `UNSHIFT` and `PUSH` that [sb4-pitfalls.md](sb4-pitfalls.md) gives. A program
  that works on SmileBASIC 4 works on the emulator.
- Strings under 64 KiB are not looked at one by one. What they add up to is
  refused at the next count, a little later than SmileBASIC 4 might.
- Blocks under 64 KiB are not looked at as they shrink either, so `POP`,
  `SHIFT`, `REMOVE` and `RESIZE` always go ahead in the emulator. With no room
  for the smaller block, SmileBASIC 4's `POP` and `SHIFT` leave the length as it
  is, and its `REMOVE` and `RESIZE` stop with `Out of memory`.
- `DIM A%[1601]` with 5008 free shows this: SmileBASIC 4 keeps 1601 and gives
  the last element each time, and the emulator shrinks and gives each in turn.
- `LOAD` of a program takes nothing from this memory.

### `Out of memory` while a program compiles

- What counts: `Out of memory` and `Out of code memory` for constant strings as
  a program compiles count those strings and the constants they are made from.
  SmileBASIC 4's limits take in the rest of the statement and its code too. In
  the emulator that code is a word an instruction and no more.
- Right at the limits, a program may compile on the emulator that SmileBASIC 4
  refuses, but not the other way round: `A=LEN("A"*2097116)` compiles on the
  emulator, and on SmileBASIC 4 2097068 is the most.
- A constant string that compiles is still made when its line runs, which is
  where the memory it takes may run out. On SmileBASIC 4 it takes none until
  stored.
- `POP` and `SHIFT` of a constant behave as on SmileBASIC 4.

## Rendering and GPU

- Texel ties: where a turned or scaled sprite's or text's sample point lands
  exactly between two texels, SmileBASIC 4's result comes from the GPU's
  arithmetic, and can change along a sprite's diagonal or with where it is on
  the screen. The emulator takes one rule for each case, the most usual outcome
  (`Placement::texel_tie`), so a few of these pixels show the neighboring texel.
  It also never reads the page just outside the image at such a tie, which
  SmileBASIC 4 can do.
- `GARRAY`: what a program writes to a `GARRAY` array shows on the screen only
  after `GUPDATE`. `GPGET` and other commands that read the page see it at once,
  as on SmileBASIC 4. On SmileBASIC 4, drawing commands near the writes also
  bring them to the screen.
- `LMATRIX`'s 4x4 form: on such a layer, text screens show only as far as one
  screen past each edge of the screen, before the matrix moves them, to keep the
  memory they take bounded. SmileBASIC 4 shows them wherever they are, as it
  does sprites. Being drawn flat first, their partly clear or tinted pixels are
  also rounded once more than SmileBASIC 4 rounds them, which can make a channel
  1 off.
- `FADE` on a subprogram: a subprogram's screen under `FADE`, or darkened while
  the main interpreter has the focus, is rounded to 8 bits before it goes over
  the main screen. SmileBASIC 4 does both in one step, so a channel can be 1 off
  where the subprogram's screen is not opaque.
- Filters: a filter's result goes to 8 bits before the layer's color is
  multiplied in. SmileBASIC 4 applies both in one draw, with one
  rounding, so a tinted, filtered layer can be 1 off.

### GPU rendering

With GPU rendering (File > Settings > Screen > Renderer), a frame differs from
the CPU renderer's, which `smilebasic-check` always uses, in these ways:

- Rounding: most pixels that differ are 1 off, the GPU's rounding, which is
  SmileBASIC 4's own kind. On the frames compared with SmileBASIC 4, the GPU
  comes closer. Rounding adds up over layers, to 2 where they stack.
- Blurs differ by 3 at most, and by 6 over three blurred layers, as the emulator
  does them with no shader.
- Ties: where a sample falls between two texels, or an element's edge on a
  pixel's center, the GPU's result may be the other texel, on some 0.1% of a
  turned scene's pixels. Which pixels depends on the GPU. Ties go by the rule
  SmileBASIC 4 mostly follows, not by the noise a real Switch has besides.
- 4x4 layers: inside a cell of the layer's grid, a texel can be a quarter of a
  pixel off.

## Screen scaling

- View > Screen filter chooses the screen's scaling filter instead of following
  `XSCREEN`. Following `XSCREEN` is one of its choices, Program (XSCREEN). The
  default is Nearest, which scales by whole factors. No `XSCREEN` mode does
  that.
- `XSCREEN`'s mode 1 applies to the program's screen. The editor's frame scales
  by Nearest at a whole factor, and otherwise with SDL's pixel art filter, or
  Bilinear when the screen's filter is Bilinear.

## System values

- `SYSPARAM("LANG")` answers `en-US`, as the English edition does, or `ja`, as
  set in File > Settings > General.
- `SYSPARAM("TABSTEP")` is the editor's tab width, set in
  File > Settings > Editor.
- `SYSPARAM("TVMODE")` is always 1.
- `SYSBEEP` keeps its value, as the emulator has no system sounds.
- `TOOL1` to `TOOL3`, which F9 to F11 start, name SmileBASIC 4's own tools. The
  emulator does not have them: they start only from the `#TOOL` project that the
  importer copies from SmileBASIC 4 into the projects folder. A missing tool, or
  a key with no tool, shows a notice.
- `ENVSTAT`'s OUT form at the start, before any program, gives status 0 and line
  0. On SmileBASIC 4 it is status 1 and the line of a system program that ran
  before, which the emulator does not run.
- `UISTATE` always reads 0. `UISTATE(2)` read 648 on a Switch 2, meaning
  unknown.
- `CONTROLLER(0)` is 1, handheld. A Switch 2 read 2.

## Direct mode, debugger and project manager

- `BACKTRACE`: after it, `CONT` and `TRACE` still go on with the stopped
  program. On SmileBASIC 4 they cannot. Programs cannot tell, since all three
  work in direct mode only.
- Run > "Clear screen and sound when a program ends", off to start with, does
  `ACLS` and `SNDSTOP` after a program ENDs or is stopped with the Pause/Break
  key or F5. STOP, errors and the debugger's stops leave the screens and sound
  as they were, to be looked at.
- Run on a project in the project manager returns to the manager when the
  program ends or is stopped. After an error the screen stays, with the
  emulator's own line `=== Press A Button or Enter to return.===` below it,
  until A (any controller) or Enter is pressed.

## Subprograms

- Starting: subprograms have no Smile button to start them from, only `SUBRUN`
  in direct mode and the tools on F9 to F11.
- Focus: Run > "Switch keys to/from subprogram" moves the focus as `ENVFOCUS`
  does, and Run > "Stop subprogram" does `SUBSTOP`, so a subprogram cannot keep
  the keys from direct mode.

For subprograms' sound and screen, see `SNDMSBAL` under [Sound](#sound) and
`FADE` under [Rendering and GPU](#rendering-and-gpu).

## Dialogs and errors

- `DIALOG` is drawn as the emulator's own window, in English. Besides the
  controller's A and B, it answers clicks and Enter.
- Error messages add detail in parentheses, such as which argument was out of
  range.
- File > Settings > General > "Japanese explanation under errors" prints the
  Japanese line SmileBASIC 4's Japanese edition shows under each error message,
  in the English edition too.
- `ENVSTAT` gives the wrong argument's number, from 0, for errors that come from
  checking one argument, such as `LOCATE 0,999`'s `Out of range` (1, as
  SmileBASIC 4 gives). Errors found by checking several arguments together give
  -1, which SmileBASIC 4 may number.
- `Internal Error`: if the emulator itself fails while a program runs, it stops
  the program and prints `Internal Error` with the reason, keeping the slots and
  the editor's text. The stopped program cannot go on with `CONT`. SmileBASIC 4
  has an error of that name, but what it does after one is not known.

## Files

- File commands ask, notify or stay silent as set in
  File > Settings > Files > "Before file commands such as SAVE and LOAD".
  SmileBASIC 4 always asks. A file command that fails in a program shows a
  notice, rather than a dialog to close, unless file access is silent.
- `SAVE` and `SAVEV "TXT:..."` write plain UTF-8 text, and `SAVEG` and
  `SAVEV "GRP:..."` write PNG images, so the files open on a computer.
- `DAT` files hold SmileBASIC 4's array without its container. The emulator
  reads all of these back, and also files from SmileBASIC 4.
- Export and Import: the project manager's Export writes a project in SmileBASIC
  4's own files, and Import reads them back into the emulator's.
- `METASAVE` keeps a project's title and description in `@META.TXT` and its icon
  in `@ICON.PNG`, in the project's folder. `METALOAD` reads SmileBASIC 4's own
  `META` file when those are missing.

### Files from elsewhere

Projects get copied in by hand and imported, so the emulator treats the files in
them as untrusted. SmileBASIC 4 has a closed file system. What the emulator
refuses fails like any other file that cannot be read or written: `RESULT()` is
0 and the message says why.

- Symbolic links among the projects or inside one are not followed, read,
  written through or copied. The emulator treats a linked project as no project
  and a linked file as missing. The projects folder itself may be a link.
- Files over 256 MiB are not read or saved, whichever command asks, and show
  nothing when chosen in the projects window. SmileBASIC 4's own limit is its
  memory.
- `LOADV` and `LOADG` refuse an array file whose non-zero sizes multiply to over
  2^28, also with a zero among them (`[0xFFFFFFFF,0]`), so that nothing that
  goes through its rows can run out of memory. What SmileBASIC 4 does with such
  a file is not known. `[0,3]` and `[3,0]` load on both.
- Image files of no pixels are refused rather than loaded empty.
- Names Windows cannot keep are refused on Windows alone: `CON`, `PRN`, `AUX`,
  `NUL`, `COM0` to `COM9` and `LPT0` to `LPT9` for projects, also with an
  extension, and any name ending in a dot.

## Editor

- Colors start as SmileBASIC 4's, and can be swapped for preset palettes from
  other editors, typed in as hex values, or loaded from and saved to text files.
  Selected text has a color of its own.
- Sizes: besides SmileBASIC 4's three sizes (21, 28 and 43 lines of code), the
  editor has a smaller one with 57, about 60 rows with the ruler, the status
  line and the room left for the menu bar.
- Help opens beside the editor rather than over it, at one of three text sizes
  (F2 while it has the keys). Clicking the code gives the keys back while the
  help stays open.
- `HELPINFO` and `HELPGET` need SmileBASIC 4's help imported.
- `HELPINFO OUT` gives the topic at the editor's cursor only to programs started
  from `#SYS`, as SmileBASIC 4's own help is. Other programs get
  `DEFAULTHELPKEY`, as on hardware, unless File > Settings > General > "Allow
  system-only features in every program" is on. It looks where the cursor was
  when the program started, or where it is now for a subprogram.
- Additions: the emulator adds an index of the program's labels and DEFs to jump
  to (Ctrl+J), an overview of the program behind the code's right side, and
  scrolling with the mouse wheel.
- Completion: the list of words to complete also shows in direct mode, and can
  offer the program's own names (off at first, as in SmileBASIC 4). Each entry
  is tagged with what it is, and the first form the help gives for it shows at
  the bottom of the screen. After GOTO, GOSUB and RESTORE it lists the program's
  labels, and after `#` the constants with their values.
- Argument hints: while a command's arguments are typed, every form the help
  gives for it (or a DEF's own line) shows at the bottom of the screen, with the
  argument being typed marked. The hints go to the top while the cursor is at
  the bottom, or by the cursor if set so.
- Ctrl+Space hides the list and the hints until it is pressed again.
  File > Settings > Editor sets when they show and what they offer.
- Ctrl+S takes the file name typed on the computer's keyboard rather than
  SmileBASIC 4's on-screen one.
- Insert's overwrite setting is kept apart for the typed line and for each side
  of the editor. SmileBASIC 4 has one for all of them.
- `PRGEDIT` leaves the editor's line wrapping as it is: the PRG commands count
  lines of the source, which wrapping does not change.

### Editor in a separate window

With View > "Editor in a separate window", the editor opens in a second window
beside the screen, and programs and direct mode go on while it is open. In
SmileBASIC 4 the editor replaces the screen and no program runs while it shows.

- Edits reach the slot at once, so RUN, SAVE and the PRG commands see them, and
  a slot that a program or LOAD changes shows the change in the editor.
- CONT and the debugger's commands leave it open. A step that waits for input
  brings the screen's window to the front for typing, and the editor's back when
  it stops.
- Position: with File > Settings > Editor > "Remember the editor window's
  position and size", the window opens where it was last.
- A subprogram's screen shows over the screens rather than the editor's window,
  but what it types with `PUSHKEY` still goes into the editor while that window
  has the keys.
- `ENVLOCATE OUT X,Y`, which is not in the manual, follows the console's cursor
  while the editor is open in its own window. SmileBASIC 4 would give 0,0 for
  the editor.
