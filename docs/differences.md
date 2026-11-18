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
- [DIRECT mode, debugger and project manager](#direct-mode-debugger-and-project-manager)
- [Subprograms](#subprograms)
- [Dialogs and errors](#dialogs-and-errors)
- [Files](#files)
- [Editor](#editor)

## Sound

### Without the sound archive

SmileBASIC 4 keeps its instruments, sound effects and preset songs in its sound
archive, which the emulator cannot ship. The importer takes them from your own
copy, and the emulator plays them on a sampler and a sequence player of its own.
Envelopes, volume curves, the LFOs and `BEEP`'s pan follow SmileBASIC 4's
sound engine and recordings made on it.

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
  effect at once, while a waveform's notes fall by their release. All of them
  are released the same way, so an effect's tail goes on in
  SmileBASIC 4.
- Note count: past about 48 notes piled up by repeating sounds, or 48 falling
  once their handles have gone, the oldest make way. A note too quiet to reach a
  16-bit sample ends at that point. SmileBASIC 4 has 96 voices shared with BGM.
- Sustain of 0: a note whose envelope decays to a sustain of 0 ends at -72.3 dB.
  SmileBASIC 4 holds it until it is released. Drums that ignore the release would
  otherwise sound, unheard, forever.
- A song started while nothing plays sounds two frames after `BGMPLAY`. On
  SmileBASIC 4 it sounds 1276 samples after, about half a frame sooner. With
  two frames, the song's MML variables change on the same frames as on
  SmileBASIC 4, which rhythm games follow. A `BEEP` in the same frame sounds
  two frames before the song in the emulator, and with it on SmileBASIC 4.
- `WAVSET` with `[..]` groups leaves silent the part of the 512 samples that the
  last group does not fill. SmileBASIC 4 leaves whatever its stack held.
- A few preset effects play at a slightly different pitch and end a frame or
  two sooner: `BEEP 280` at 490 Hz where SmileBASIC 4 gives 503, `BEEP 300` at
  977 against 987 to 993, and `BEEP 383` at 2063 against 1950; `BEEP 330` ends
  early too. The difference seems to come from how the console resamples
  them, and copying that would cost time on every sample.
- Instruments from the sound archive play their samples with linear
  interpolation. SmileBASIC 4 reads them through the smoothing filter that the
  emulator keeps for `WAVSET` and `WAVSETA` waveforms. The filter made a song
  of many instrument notes take a quarter longer to mix, and the instruments'
  samples are 32 kHz and up, where the two sound nearly alike.
- SoundFont instruments: MML's tremolo (`@MA`) and pan modulation (`@ML`) are
  ignored, vibrato (`@MP`) takes only its depth on the modulation wheel, and
  envelopes (`@E`) do nothing. SmileBASIC 4's own instruments and the PSG and
  user waveforms take them all.
- The effector (`EFCEN`, `EFCSET`, `EFCWET`, `BGMWET`) is a small reverb of the
  emulator's own: early reflections and a Freeverb-like tail. It takes the
  settings SmileBASIC 4 gives the console's reverb, so the presets and
  `EFCSET`'s nine values change it alike, and the presets are fitted to how the
  console's reverb answers a sine sweep. It still does not sound like the
  console's. `RECSTART` records as on SmileBASIC 4, where the effector does not
  reach the recording either.
- A song's dry sound drops a few percent on SmileBASIC 4 while the effector is
  on: an `@216` note records at 358 to 369 there, against 379 to 390 with
  `EFCEN 0`, whatever `EFCWET` and `BGMWET` say. The emulator keeps the dry
  level.
- `BEEP 256` falls 5 to 10% faster than on SmileBASIC 4: from 150 to 250, 250
  to 400 and 400 to 600 ms it records 136, 95 and 51, where SmileBASIC 4 gives
  144, 101 and 56. The emulator's envelopes share one table, so other sounds
  may fall faster too.
- A `PCMSTREAM` whose array `RESIZE` has made shorter plays silence for the
  samples that are gone, and so does the right side of a 2D array with one
  row. SmileBASIC 4 reads the memory past the array there.
- `TALK` speaks with a voice of the emulator's own, a Klatt formant synthesizer
  (klattsch-core) reading the kana or phoneme letters, so it says the same words
  in another voice. `SNDSTOP` and `TALKSTOP` cut it within a few milliseconds
  of when `SNDSTOP` cuts `BEEP`, where SmileBASIC 4 cuts `TALK` about a frame
  sooner.
- Notes far above their waveform's rate, such as `O8A` of an 8000 Hz `WAVSETA`
  sine, leave images 10 to 22 dB louder than on SmileBASIC 4, whose
  resampling filters them more.
- Some instruments' notes differ in shape: `@118` at `L16` rings for about a
  second where SmileBASIC 4's falls 15 dB in 200 ms, and a phrase of `L8`
  notes dips between notes where SmileBASIC 4's does not. A new `BGMPLAY` on a
  track cuts the last note's release, which goes on under the new song on
  SmileBASIC 4.
- The drum kits `@131` to `@133` play other kick drums than SmileBASIC 4's,
  and hi-hats in every kit come out about 1 dB quieter.
- `BEEP 152` records at 0.76 of SmileBASIC 4's level, with the same shape.
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
  gives -1 as SmileBASIC 4 does. An error inside a macro is placed just past
  the macro's use either way, as on SmileBASIC 4.
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
  mode, in an array of 48 rows either way, and `IRSPRITE` shows no image.
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
- Swap A/B and X/Y (File > Settings > Input, or the Simple UI's pause menu;
  `swap_ab_xy` in `settings.txt`), off by default, is for gamepads that
  print A at the bottom. A gamepad's bottom button then reads as `#B_RRIGHT`
  (A) in `BUTTON` and types what A types, the right one as `#B_RDOWN` (B),
  and X and Y trade the same way. The emulator's own screens follow it too.
  The keyboard's keys for buttons stay as they are. SmileBASIC 4 has no such
  setting: the Switch's buttons are where their letters say.
- Caps Lock: programs, DIRECT mode and the editor get letters by SmileBASIC 4's
  own Caps Lock, which is on at start and which the Caps Lock key toggles in
  any of the emulator's windows. The computer's Caps Lock light disagrees with
  it if the computer's Caps Lock was on when the emulator started, or was
  pressed while another program's window had the keys. With File > Settings >
  General > "Type capitals without Shift" off, letters keep the computer's
  case everywhere.
- Key repeat: a key held for a program, DIRECT mode or the editor repeats in the
  emulator's frames, as a key held for `INKEY$` does on SmileBASIC 4: 19 frames
  after the press, then every 3. Any other key going down or up, Shift and
  Ctrl among them, starts the count again, and each repeat types what the
  modifiers held then give with the key. That covers letters, digits and
  symbols, Enter, Backspace, Delete, Tab, Esc, Insert, the cursor keys, Home,
  End, Page Up and Page Down, and Shift with the cursor keys and with Tab.
  Enter and the other keys that type no character repeat what they typed at
  the press. The editor's shortcuts with Ctrl, such as Ctrl+Z, and the
  function keys repeat at the computer's rate. Text outside ASCII, such as
  what an input method types, goes as the computer sends it, with the
  computer's repeat, since the key it came from is not known.
- Alt with a key types SmileBASIC 4's own characters, as on SmileBASIC 4
  (Alt+A gives U+E2EC), whatever the computer's layout gives. Right Alt on a
  layout where it is AltGr is the exception: it types the layout's character
  for the keys that have one, such as "@" for AltGr+Q on a German layout, and
  SmileBASIC 4's for the rest. SmileBASIC 4 is expected to do the same by the
  keyboard layout set on the console; the emulator goes by the computer's.
  An AltGr character outside ASCII, such as "€", repeats at the computer's
  rate.
- Han/Eng: on SmileBASIC 4 it types nothing and letters stay ASCII. On a
  computer it may switch the computer's input method, and what that types then
  reaches the program.
- Controllers as keys: controllers type keys only as SmileBASIC 4's assignment
  does in every mode. The emulator leaves out the assignments for DIRECT mode
  and the editor alone (X for the editor or to run) and the key settings of
  SmileBASIC 4's options.
- Short presses: a button, trigger or key pressed and released between two of
  the emulator's frames, as on a computer that cannot keep 60 frames a
  second, reads as held for one frame, and one released and pressed again
  between them as released for one frame. The emulator reads what is held
  once a frame, and would otherwise miss them; SmileBASIC 4 reads the
  controllers every 1/60 s, more often than a person can press and release.
- A held button or stick, when one of the emulator's own screens closes, reads
  as released until it is. The A that chose Run in the project manager is one such
  press. It types nothing and `BUTTON` does not show it, in the program or in
  DIRECT mode.
- Mouse buttons: the left button is the touch screen (touch 0 only).
- Mouse cursor: a program that hides the mouse cursor
  (`SYSPARAM "MOUSECURSOR",0`, as programs start) hides the computer's cursor
  over the window, until the menu bar shows. `MOUSE X,Y` moves the computer's
  pointer too. File > Settings > Input > "Ignore mouse cursor commands" keeps
  the cursor shown and where it is; the program then sees the move until the
  next frame. The Simple UI follows the same rules while a program runs, but
  shows no cursor on its own screens, such as the game list and the pause
  menu, or while a program's `DIALOG` is open.
- Drawn cursor: where the system shows no cursor, as under KMSDRM on a
  handheld, the emulator draws its own arrow, about as large as SmileBASIC 4's
  on the display, with its tip at the pointer and over everything else. Like
  SmileBASIC 4's, it fades out over 0.7 seconds once the pointer has stayed
  still for 5 seconds, and comes back when it moves. The arrow is the
  emulator's own drawing, not SmileBASIC 4's. `SB_CURSOR=drawn` draws it on
  any system, and `SB_CURSOR=system` leaves the cursor to the system.
- Right stick as the mouse: as with SmileBASIC 4's option for the R Stick,
  the right stick moves the pointer and R3 is the left mouse button, while
  no program runs or the running program shows the cursor. It moves as far
  across the screen as on SmileBASIC 4, and the program still sees the stick
  in `STICK` and R3 in `BUTTON`. File > Settings > Input > "Right stick moves
  the mouse" turns it off. Unlike SmileBASIC 4's:
  - Holding − (Select), which SmileBASIC 4 does not have, gives the right stick and R3 to the pointer in any
    program and shows the cursor. The program then sees the stick centered
    and R3 released, until both are let go.
  - − (Select) opens the projects in DIRECT mode when it is released, and only
    if nothing else was pressed or moved while it was held.
  - On the desktop, R3 clicks the emulator's own screens as the left mouse
    button does: the menu bar and its menus, the emulator's windows and
    dialogs, Projects, and the editor. Holding R3 while tilting the stick
    drags. While one of these screens is open over a running program, the
    stick and R3 work there whatever `MOUSECURSOR` says, and the program sees
    neither until both are let go.
  - The Simple UI's own screens go by the buttons alone. There, and while a
    program's `DIALOG` is open, the right stick and R3 do nothing as a mouse
    and no cursor shows.
  - The pointer stops at the display's last pixel, 1279 for `MOUSE 1 OUT X`,
    where SmileBASIC 4's goes to 1280.

### Pasting and Tab

- Pasting into a line goes through the key buffers without losing keys, however
  long the text, 32 characters a frame.
- Pasting text of several lines in DIRECT mode types it one line at a time,
  with Enter after each, so that each line runs in turn, though DIRECT mode
  on SmileBASIC 4 drops keys waiting for it, as `PUSHKEY` shows. SmileBASIC
  4's paste puts the line ends into the one line, where they show as line
  breaks, and Enter runs it all. The Paste key (L or R with B on a
  controller) pastes `CLIPBOARD` that way in the emulator too.
- Tab on the keyboard takes the word from completion's list when one shows. With
  no list it is a space, as in SmileBASIC 4.

## Speed

SmileBASIC 4 runs a fixed amount of code each frame (see
[sb4-pitfalls.md](sb4-pitfalls.md)). The emulator runs code that does not wait
for the next frame for as long as the frame leaves after drawing, which is far
more on any recent computer: the 1/60 s frame less what the last frames'
events, drawing and presenting took and 2 ms to spare, from 10 to 14 ms, and
14 ms for a program whose trip round its main loop fits in that. With
vsync on a display that does not refresh at 60 Hz it is 10 ms, as a present
can then wait for most of one of the display's frames. Programs time
themselves with `VSYNC`, `WAIT` and `MAINCNT`, so none needs the slower
speed. Headless runs (`smilebasic-check`,
the tests) instead run a fixed million instructions a frame, so that they come
out the same each time.

File > Settings > General > Speed, and `smilebasic-check --speed`, choose
Switch 2 or Switch 1 instead: a program then gets about as much work done in a
frame as on that console, and misses frames when it needs more. See
[benchmarking.md](benchmarking.md#running-at-a-consoles-speed) for how close
it comes.

When the computer cannot run the program and draw the window in a frame, the
window leaves drawings out before the program falls behind. A program whose
trip round its main loop takes up to 14 ms has a trip in every frame, with its
`MAINCNT`, animation step, sound and input, whatever the drawing takes, and
the window is drawn in every second to fifth frame: 12 times a second at
least. A computer that keeps 60 frames a second draws every frame, as
before.

- A drawing is left out when it would end after the frame's time and make the
  program's next trip end after its own frame.
- Where presents wait for the display's vsync, a drawing that ends after a
  vsync comes back a whole frame later. The frames before such a drawing run
  one after the other, up to four of them ahead of their time, so that the
  drawing ends before the next vsync. The program reads its input for those
  frames up to that much early. Turn View > "Wait for vsync" off (the pause
  menu has it with the Simple UI on), for a game where the timing of input
  matters, and each frame waits for its own time instead; the window may
  then tear.
- The emulator's own screens, a paused or stopped program, a dialog, the
  editor, the debugger and a subprogram are drawn every frame.

Only when the trip itself takes more than 14 ms, or one drawing in five is
still too much, does the program slow down or miss frames as described below.
A frame that is drawn shows the scene the program finished:

- A `VSYNC` that is behind stops the program when another trip round its main
  loop would not fit in the frame. SmileBASIC 4 goes straight on.
- The drawing waits up to four frames for a program part-way through its loop.
- When even that cannot keep up, the program's loop slows down rather than the
  drawing showing half a scene.
- A frame that ends with the program part-way through its loop, having read
  no input, passes its button and key presses and releases on to the next
  frame. A press or release then shows on the frame the program reads
  `BUTTON` or `KEYBOARD` in, and one that started and ended inside such a
  frame shows as held for one frame. On a slow computer a loop can take two
  frames, and a program that reads `BUTTON` once a loop would otherwise miss
  every press that started in the loop's first frame. A program that keeps
  60 frames a second never has such a frame. At a console's speed the
  emulator does not do this. What SmileBASIC 4 shows a program whose loop
  takes more than a frame is not known.

A trip round the main loop that runs out of time keeps its frame: the next
frame, with its `MAINCNT`, animation step, sound and input, starts only when
the trip reaches its `VSYNC` or `WAIT`. A program too slow for the computer
then runs slower than 60 frames a second, as it would on a console too slow
for it, and its music slows down and breaks up.

- On SmileBASIC 4 a program gets a fixed amount of work a frame, so one made
  there fits its frames and never sees a frame pass part-way through its
  loop. Keeping the frame keeps it so on a computer too slow for the program.
- File > Settings > General > "Skip frames instead of slowing down" (Frame
  skip in the Simple UI's pause menu; `skip_frames=true` in `settings.txt`),
  off by default, lets frames pass mid-loop instead. Music does not break up
  and play feels faster, but programs that count frames may misbehave:
  animations and `MAINCNT` get ahead of the loop, and a game that starts a
  relative `SPANIM` again every few loops moves its sprites off their places.
- Keeping frames applies at unlimited speed, to the main program without a
  subprogram, editor or step, and to a trip shorter than four frames: a
  longer one, such as loading, lets frames pass as usual. At Switch 2 or
  Switch 1 speed the setting does nothing: a program that needs more than the
  console's frame misses frames, as on that console.
- When a program gets fewer than about 54 frames a second for 2 seconds
  because of this, past its first 3 seconds, a notice says where the setting
  is. It shows once per program a session, not while the program is paused
  or in a dialog, and never with the setting on. File > Settings > General >
  "Tell me when a program runs slower than real time" turns it off
  (`slow_hint=false` in `settings.txt`).

When the system holds the window up, as GNOME does while you drag or resize it,
another thread runs the program and its sound, and the window shows nothing new
until it comes back ([frame-thread.md](frame-thread.md)). The program sees the
input as it was when the window stopped. Meanwhile `CLIPBOARD` reads the text
it last read or set, and a text it sets reaches the system's clipboard when the
window comes back.

## Memory and stack

- The stack: `GOSUB`s, `DEF` calls, `RESTORE "PUSH"` and the values of
  expressions share one stack on SmileBASIC 4. The emulator counts the first
  three as SmileBASIC 4 does and values of expressions not at all, but for the
  room `PRINT` needs for a value, so a deep call ending in a long expression
  can go a few levels further. For `RESTORE "PUSH"`, 1.5 slots each is
  inferred.
- `FREEMEM` counts what strings and arrays take as SmileBASIC 4 does, but memory
  a program frees is free again at once. SmileBASIC 4 frees it only when a
  request would not fit: emptying a string or an array, or leaving a DEF, does
  not raise `FREEMEM` on SmileBASIC 4, even after `VSYNC` or `WAIT`. A program
  would have to watch `FREEMEM` to see the difference.
- `SPVAR` and `TVAR` keys of over 1024 characters store nothing. SmileBASIC 4
  adds a variable each time one is written, which only takes memory, as none
  can be found again.
- All the sprites' and text screens' variables together come to 2,097,152 at
  most, and one more is `Out of memory`. SmileBASIC 4 takes them from a pool
  of that many small blocks, which short strings share, so it can run out
  sooner.
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
- Programs' font: File > Settings > Fonts > "Programs' 8-pixel font" and
  "Programs' 16-pixel font" can put the font file's or the bundled fonts'
  glyphs on the font page (GRP5) in place of SmileBASIC's. Both are
  "SmileBASIC" by default, which leaves the page as SmileBASIC 4 has it. With
  another choice, every character that has a place of its own on the page and
  a glyph in the chosen source gets that glyph, fitted to the 8- or 16-pixel
  cell as [importing.md](importing.md#characters-the-font-lacks) describes;
  the rest keep SmileBASIC's. Characters the page has no place for still show
  the box. The glyphs go on the page when you change the
  setting or the font file, at start-up, and with each `ACLS` that puts the
  font page back (not with its keep-font flag set). Only those cells change
  when you change the setting; the rest of GRP5 keeps what the program drew.
  A program that reads the font page sees the replaced pixels: `GSAVE` from
  GRP5, `GSPOIT` and `GPGET` on it, `GARRAY` of it, and sprites or text using
  it. `FONTINFO` and `CHKCHR` answer as before.
- Font page without an imported font: the bundled stand-in font puts each
  character on GRP5 where SmileBASIC 4's font has it, by the glyph numbers of
  its font.map ([importing.md](importing.md#without-importing)). A program
  that loads or draws its own font page for SmileBASIC 4 shows its glyphs for
  every character SmileBASIC 4 has, and `FONTINFO` gives SmileBASIC 4's
  places. The glyphs on the page are the bundled fonts' own. Some characters
  SmileBASIC 4 has, such as Greek and Cyrillic, have no bundled 8-pixel glyph
  and show the box in their 8-pixel cell. Characters SmileBASIC 4 shows as the
  box but the bundled fonts have take the glyph numbers SmileBASIC 4 leaves
  unused, while they last; the rest, Hangul among them, are drawn from the
  bundled fonts directly and do not follow what a program draws on GRP5.
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
  that. In a window too small to show the screen at its own size, as a
  640x480 one is for `XSCREEN 1280,720`, Nearest shrinks the screen to fit
  with Bilinear instead.
- The desktop lays its UI out on a 1280x720 display and scales it to fit the
  window, keeping its aspect ratio, at the window's top. Below 1280x720 it is
  scaled down: on a 640x480 output it shows at half size in the top 640x360.
  The Simple UI is laid out for such outputs. The main window starts no
  larger than the display's usable area.
- The menu bar, its menus and the right-click menus are drawn at 1x where
  the display is scaled down, so on 640x480 the bar is 24 pixels high with
  80 columns of the UI font across, not half that and blurred. At 1280x720
  and larger they scale with the display as before, at 1.5x on 1920x1080.
  The menus open within the window: a list that would go past its right edge
  moves left, a submenu opens on the left of its list where the right has
  no room, and moves up to stay above the bottom. A list taller than the
  window, as View is in a 640x360 one, shows what fits and scrolls with the
  mouse wheel; a small triangle marks where it goes on. The status at the
  bar's right loses its start, behind "...", where it does not fit, and is
  left out where fewer than four of its characters would.
- The program's screen is not confined to that display. It is centered in
  the whole window at the largest size View > Screen filter allows, so
  `XSCREEN 640,480` fills a 640x480 or 1280x960 window, and `XSCREEN 320,240`
  shows at 5x in 1920x1200. In a window of 16:9 or wider the display fills
  the window's height, and nothing changes. In a narrower one:
  - The debugger's panel, and the help and index beside the editor, go down
    to the window's bottom, and so do notices and the yellow border that
    shows the focus going back to the main program. A program's `DIALOG` is
    centered on the whole window and dims all of it.
  - The emulator's windows and dialogs, and a subprogram's screen, which
    `XSUBSCREEN` places on the 1280x720 display, stay in the display. The
    menus go down to the window's bottom.
- SmileBASIC 4's editor shows a fixed frame: F2's size sets its rows (21,
  28 or 43 lines of code), and the frame, at the screen's 16:9, is scaled to
  the 1280x720 output. With the 16-pixel font at Medium that is about 51
  columns and 28 lines at 1.45x. The main window's editor is laid out as in
  a window of its own instead (see
  [Editor in a separate window](#editor-in-a-separate-window)), in a window
  of any size and shape: it fills the window under the menu bar, each of its
  pixels a whole number of the window's, with as many columns and rows as
  fit. Nothing is blurred and no margin is left.
  - The zoom is the whole number nearest to the one at which F2's size shows
    SmileBASIC 4's rows in the display's height, at least 1, made smaller
    where it would leave fewer than 40 columns. On a 16:9 window the
    display is the window's height.
  - The 16-pixel font at Medium is 1x on 1280x720 (80 columns, 43 rows with
    the ruler and the status line), 640x480, 1024x768 and 720x720, 2x on
    1920x1080 (60 by 32) and 1920x1200, 3x on 2560x1440 and 4x on 3840x2160.
    The 8-pixel font at Large is 4x on 1280x720 and 2x on 640x480.
  - The 16-pixel font is never drawn below 1x, so F2's smaller sizes can
    look the same: on 1280x720, Medium, Small and Tiny all show it at 1x, and
    only Large (2x) differs. The 8-pixel font gives four sizes there (4x, 3x,
    2x and 1x). On 2560x1440 and 3840x2160 each size differs with either
    font.
  - F2 and Ctrl+B skip the sizes that look the same as the current one in
    the window the editor shows in, so each press changes the zoom, and do
    nothing where every size looks the same. View > Size shows such sizes
    disabled: of the sizes at one zoom, only the current one, or else the
    first listed, can be chosen. The setting keeps the size picked, which
    can differ in another window.
  - The Simple UI keeps SmileBASIC 4's frame, scaled to where the screen
    goes.
- View > Window size sets the main window to 0.5, 1, 1.5, 2, 2.5 or 3 times
  1280x720 pixels, leaving fullscreen first. The menu bar goes over the
  screen, so the screen gets the whole multiple: `XSCREEN 1280,720` at 2x
  fills 2560x1440. A size that does not fit the display's usable area, with
  the window's title bar where the system reports it (X11 and Windows), is
  disabled rather than shrunk. The item the window is at, to the pixel, is
  checked; on a display scaled by a fraction some sizes cannot be reached
  exactly. With the editor in its own window, only the main window changes.
  The window keeps its size out of fullscreen when the emulator quits
  (`window_size` in `settings.txt`) and starts at it next time.
- View > "Keep 16:9 when resizing", on by default (`keep_16_9` in
  `settings.txt`), keeps the main window at the display's 16:9 while you
  resize it, so the screen and the UI fill it with no bars. The emulator
  asks the system for that shape. Where the system ignores it, as some
  Wayland compositors do, the emulator sets the nearest 16:9 size once the
  window has kept its size for 0.3 seconds: the same width, or where that
  height does not fit the display, the same height. If the system refuses
  that size twice in a row, the emulator leaves the window alone until you
  turn the item on again. Turning it on puts a window of another shape
  right at once. Fullscreen and maximized windows keep the size the system
  gives them: the emulator does not change what it asks for while the
  window is in either state, as SDL sets the size again when it does. The
  editor in its own window can take any shape. With the item off, or in
  fullscreen or maximized on a display of another shape, the window is not
  16:9 and the screen and editor are placed as described above.
- View > "Wait for vsync", on by default (`vsync` in `settings.txt`), has
  the main window's presents wait for the display's refresh, which paces
  the frames on a display at 60 Hz. Off, the frames go by the clock at 60 a
  second, as they always do on displays at other rates and while the window
  is covered or minimized. The change takes effect at once. The editor's
  own window never waits for vsync.
- With the Simple UI on, the screen fills the display at its aspect ratio, and
  the filter is the pause menu's Screen filter (`simple_ui_filter` in
  `settings.txt`), not View > Screen filter. A 640x480 display seldom holds
  the screen a whole number of times (400x240 is 1.6 times) and can show it
  smaller than it is (`XSCREEN 1280,720` is half size). SmileBASIC 4's
  1280x720 output never shows a screen smaller than it is. The choices:
  - Auto, the default, ignores `XSCREEN`'s composite mode. It scales with
    the pixel art filter where the screen is drawn larger than its frame
    (the `XSCREEN` size times its sample count), bilinear where it is drawn
    smaller, and nearest at a whole factor.
  - Pixel art and Smooth (bilinear) always use that filter.
  - Sharp is nearest at the fitted size, so pixels differ in width.
  - Program follows `XSCREEN`'s composite mode, as SmileBASIC 4 does.
- `XSCREEN`'s mode 1 applies to the program's screen. The editor's frame is
  drawn at a whole factor with Nearest, whatever the screen's filter. In the
  Simple UI, which scales SmileBASIC 4's frame to the screen's place, it is
  Nearest at a whole factor, and otherwise SDL's pixel art filter, or
  Bilinear where the screen is scaled with Bilinear.
- The emulator's own UI (menu bar, dialogs, notices, the debugger's panel)
  scales by Nearest at a whole factor, with SDL's pixel art filter when drawn
  larger, and with Bilinear when drawn smaller. Where SDL's pixel art filter is
  only Nearest (the software, OpenGL ES 2 and Direct3D 9 renderers, and OpenGL
  before GLSL 1.30), the UI goes to the next whole factor with Nearest, then
  down to its size with Bilinear. The Simple UI's Medium size is such a
  scale on most displays: 1.5 times on 640x480 and 1280x720.

## System values

- `SYSPARAM("LANG")` answers `en-US`, as the English edition does, or `ja`, as
  set in File > Settings > General.
- `SYSPARAM("TABSTEP")` is the editor's tab width, set in
  File > Settings > Editor.
- `SYSPARAM("TVMODE")` is always 1, as on a docked Switch. A Switch in
  handheld mode gives 0, and SmileBASIC 4's value follows the dock while a
  program runs.
- `SYSBEEP` keeps its value, as the emulator has no system sounds.
- `TOOL1` to `TOOL3`, which F9 to F11 start, name SmileBASIC 4's own tools. The
  emulator does not have them: they start only from the `#TOOL` project that the
  importer copies from SmileBASIC 4 into the projects folder. A missing tool, or
  a key with no tool, shows a notice.
- `UISTATE` always reads 0. `UISTATE(2)` read 648 on a Switch 2, meaning
  unknown.
- `CONTROLLER` counts a gamepad as a Pro Controller with the colors of a black
  one, and the keyboard alone as handheld (1).

## DIRECT mode, debugger and project manager

- `BACKTRACE`: after it, `CONT` and `TRACE` still go on with the stopped
  program. On SmileBASIC 4 they cannot. Programs cannot tell, since all three
  work in DIRECT mode only.
- Run > "Clear screen and sound when a program ends", off to start with, does
  `ACLS` and `SNDSTOP` after a program ENDs or is stopped with the Pause/Break
  key or F5. STOP, errors and the debugger's stops leave the screens and sound
  as they were, to be looked at. `smilebasic-check` always leaves them, as
  SmileBASIC 4 does.
- A stop that the debugger shows in the editor prints neither `Break on` nor
  `OK`, so the console keeps only what the program printed. This covers steps,
  breakpoints, `TRACE`, Run to cursor, watched changes, and any stop while the
  panel is pinned. `OK` shows once the keys go back to DIRECT mode.
- Ctrl+C's break acts as the key goes down. On SmileBASIC 4 the jump to
  `ON BREAK GOTO`'s label was seen 20 frames after the press.
- `RUN` reaches the program's first line two frames after the Enter, as on a
  Switch 2, or four with Speed set to Switch 1. A Switch 1 takes three or
  four.
- A `COMMON DEF` of the program run last cannot be called from DIRECT mode:
  it is a `Syntax error` there, as a plain `DEF` is. SmileBASIC 4 runs it.
- Run on a project in the project manager returns to the manager when the
  program ends or is stopped. After an error the screen stays, with the
  emulator's own line `=== Press A Button or Enter to return.===` below it,
  until A (any controller) or Enter is pressed.
- The Simple UI (`--simple-ui`, or Ctrl+Shift+F12) has no DIRECT mode or
  editor. It starts in a list of the projects, runs a project's MAIN.PRG or one
  of its programs, and returns to the list as Run in the project manager does.
  − (Select) with A opens the emulator's pause menu, which pauses the program and gives it
  no input; neither that A nor the Enter it would type reaches the program.
  − (Select) with + quits the emulator. SmileBASIC 4 has none of these.
- The Simple UI leaves the screens as `ACLS` does and stops the sound as
  `SNDSTOP` does each time it goes back to its list: after `END`, `STOP` or
  the + button, after an error once A is pressed, and on Return to Game list
  and Restart. The desktop does this only with Run > "Clear screen and sound
  when a program ends".

## Subprograms

- Starting: subprograms have no Smile button to start them from, only `SUBRUN`
  in DIRECT mode and the tools on F9 to F11.
- Focus: Run > "Switch keys to/from subprogram" moves the focus as `ENVFOCUS`
  does, and Run > "Stop subprogram" does `SUBSTOP`, so a subprogram cannot keep
  the keys from DIRECT mode.

For a subprogram's screen, see `FADE` under
[Rendering and GPU](#rendering-and-gpu).

## Dialogs and errors

- `DIALOG` is drawn as the emulator's own window, in English, and leaves the
  screen visible around it where SmileBASIC 4's covers the whole screen. Besides
  the controller's A and B, it answers clicks and Enter.
- Error messages add detail in parentheses, such as which argument was out of
  range. It comes after SmileBASIC 4's own naming of the command, as in
  `Wrong number of arguments in 1:28(LOADV) (TXT in the command form)`.
- File > Settings > General > "Japanese explanation under errors" prints the
  Japanese line SmileBASIC 4's Japanese edition shows under each error message,
  in the English edition too.
- `ENVSTAT` gives the wrong argument's number, from 0, for errors that come from
  checking one argument, such as `LOCATE 0,999`'s `Out of range` (1, as
  SmileBASIC 4 gives). It also numbers the checks of a length or a size that
  SmileBASIC 4 is seen to number: `LEFT$`'s count, `BIQUAD`'s output,
  `TLOAD`'s, `TSAVE`'s, `GLOAD`'s and `GSAVE`'s sizes, `ENVSAVE`'s kind and
  the offset and count of `COPY`'s function form. Other errors found by
  checking several arguments together give -1, which SmileBASIC 4 may number.
- `FFT` and `IFFT` refuse a first array shorter than the others with
  `Illegal function call` naming it. SmileBASIC 4 does not look at its length
  and writes all the results into it.
- `FFT` with an int array as its window takes each two ints as the bits of
  one real, and 0 for the half that would lie past the array's end, where
  SmileBASIC 4 reads the memory there.
- Two sound errors never come: `Sound request too much` (79), which SmileBASIC 4
  raises when `BEEP` or a `BGM` command finds its sound thread's request queue
  full, and `Out of sound memory` (80), which it raises when its 8 MiB sound
  memory cannot take a `WAVSET` waveform or a song `BGMPLAY` or `BGMSET`
  compiles. The emulator carries out sound requests at once and has no sound
  memory limit.
- `Internal Error`: if the emulator itself fails while a program runs, it stops
  the program and prints `Internal Error` with the reason, keeping the slots and
  the editor's text. The stopped program cannot go on with `CONT`. SmileBASIC 4
  has an error of that name, but what it does after one is not known.

## Files

- File commands ask, notify or stay silent as set in
  File > Settings > Files > "Before file commands such as SAVE and LOAD".
  SmileBASIC 4 always asks. A file command that fails in a program opens a
  dialog only when file access is set to ask: the dialog names the command
  and the reason, and the command waits until it is closed, then gives
  `RESULT` 0, as on SmileBASIC 4, or stops with `Load failed` for `EXEC`. Set
  to notify, the failure shows as a notice, and silent shows nothing; either
  way the command returns in the same frame. On SmileBASIC 4, `LOAD`,
  `LOADV`, `LOADG`, `DELETE` and `RENAME` of a missing file always open a
  dialog.
- A file command's question starts on the button that goes ahead, as
  SmileBASIC 4's save question starts on OK. Replacing a file asks once, with
  a line saying so, where SmileBASIC 4 asks twice, whether to save and then
  whether to overwrite.
- `SAVE` and `SAVEV "TXT:..."` write plain UTF-8 text, and `SAVEG` and
  `SAVEV "GRP:..."` write PNG images, so the files open on a computer.
- `DAT` files hold SmileBASIC 4's array without its container. The emulator
  reads all of these back, and also files from SmileBASIC 4.
- Export and Import: the project manager's Export writes a project in SmileBASIC
  4's own files, and Import reads them back into the emulator's.
- `METASAVE` keeps a project's title and description in `@META.TXT` and its icon
  in `@ICON.PNG`, in the project's folder, and then deletes SmileBASIC 4's own
  `META` file there. `METALOAD` reads `META` when those are missing.
- `LOADG "PNG:..."` of a `#` project's file fails with `RESULT` 0, as the
  emulator reads no `PNG:` files.
- `SAVE`, `SAVEV`, `SAVEG`, `DELETE` and `RENAME` of a file in a `#` project,
  named or the current one, and `METASAVE` in a current `#` project, are
  error 75, `Can't SAVE to system project`, as on SmileBASIC 4. Ctrl+S in a
  current `#` project shows that the file could not be saved and asks for no
  name, also as on SmileBASIC 4. File > Settings > Files > "Allow writing to
  system (#) projects" lets all of them write there instead. Off by default.
  A main program still gets error 74 for a file named with a project.
- File and project names take ASCII letters only. SmileBASIC 4 asks C's
  `isalnum` about characters up to U+00FF, so it may take some of U+0080 to
  U+00FF as letters; the emulator gives `Illegal format` for them.
- On Windows, which cannot keep a file name that ends in a dot, the commands
  refuse such a name (`SAVEV "TXT:A.",S$`) with `RESULT` 0. SmileBASIC 4 takes
  it.
- A project's name in a path takes no `@`, which the emulator keeps for its
  `@WORKSPACE` folder. SmileBASIC 4's parser takes `A@/NAME`; the emulator
  finds no such project, so `CHKFILE("A@/NAME")` is 0.

### A program's own folder

`smilebasic PROGRAM` makes the folder that holds `PROGRAM` the current project,
so the program can `LOAD`, `EXEC`, `LOADV`, `LOADG` and save the files beside
it. SmileBASIC 4 has no such thing. The project takes the folder's name in upper
case, with `_` for each character a project name cannot have and for a leading
`#`: `hangul lib` becomes `HANGUL_LIB`. `PROJECT` switches to the other projects
and back by that name. A project of that name in `projects/` stays out of reach
until you restart. The projects window lists the folder but will not rename,
delete or duplicate it.

- A file there can have SmileBASIC 4's name or a plain one: `THGLIB.PRG`,
  `HGLIB.PRG.txt` and `HGLIB.PRG` all give `TXT:HGLIB.PRG`. DAT, GRP and JPG
  files take SmileBASIC 4's names only. Case does not matter, and `FILES`
  leaves out names that break SmileBASIC 4's rules.
- A name starting with T, B, G or J reads as SmileBASIC 4's when the folder
  uses SmileBASIC 4's names, or when the file holds that type: SmileBASIC 4's
  container, or the DAT array, PNG or JPEG the emulator writes. Otherwise
  `TEST.PRG` is the text file `TEST.PRG`. A folder uses SmileBASIC 4's names
  when it holds a `META` file, a backup such as `T@BACKUP.PRG`, or a text file
  in SmileBASIC 4's container.
- When two files give one name, SmileBASIC 4's form wins, then the name ending
  in `.txt`. `FILES` lists the winner alone.
- Saving keeps an existing file's name on disk: Ctrl+S on `HGLIB.PRG` rewrites
  `HGLIB.PRG.txt`. A new text file takes the folder's form, SmileBASIC 4's or a
  plain name, with `.txt` when at least half of the plain text files there end
  in it. Backups follow the same rule (`@BACKUP.PRG.txt`).
- The first `SAVE`, `SAVEV`, `SAVEG`, `DELETE`, `RENAME`, `METASAVE` or Ctrl+S
  that changes a file the folder held at start-up asks, whatever File >
  Settings > Files says, in programs too. The question says that the original
  file changes. After that one change, the setting decides.
- `smilebasic-check` still takes project folders as before.

### Files from elsewhere

Projects get copied in by hand and imported, so the emulator treats the files in
them as untrusted. SmileBASIC 4 has a closed file system. What the emulator
refuses fails like any other file that cannot be read or written: `RESULT()` is
0 and the message says why.

- Symbolic links among the projects or inside one are not followed, read,
  written through or copied. The emulator treats a linked project as no project
  and a linked file as missing. The projects folder itself may be a link.
- Other programs may add, replace, rename or remove files and project folders
  while the emulator runs. Each file command (`FILES`, `LOAD`, `CHKFILE`,
  `PROJECT` and the rest) reads the folder as it is at that moment. The
  projects window and the Ctrl+L list read their folders again when an emulator
  window gets the focus back, and after a key, click or button press when the
  folder's modification time has changed. Writing over a file in place leaves
  that time alone, so the list keeps showing the old size and date until the
  window gets the focus back. Loading the file still reads its new contents.
- Files over 256 MiB are not read or saved, whichever command asks, and show
  nothing when chosen in the projects window. SmileBASIC 4's own limit is its
  memory.
- `LOADV` and `LOADG` refuse an array file whose non-zero sizes multiply to over
  2^28, also with a zero among them (`[0xFFFFFFFF,0]`), so that nothing that
  goes through its rows can run out of memory. What SmileBASIC 4 does with such
  a file is not known. `[0,3]` and `[3,0]` load on both.
- Image files of no pixels are refused rather than loaded empty.
- On Windows, a project whose name Windows keeps for a device, such as `COM5`,
  `LPT0` or `CON.A`, is stored in a folder named `@COM5`. Programs and the
  projects window still see `COM5`. A new text file of such a name beside a
  program run from the command line takes SmileBASIC 4's name, `TCOM5`.

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
- Completion: the list of words to complete also shows in DIRECT mode, and can
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
- Characters missing from SmileBASIC's font show in
  the editor from another font instead of as a box (see
  [importing.md](importing.md#characters-the-font-lacks)). File > Settings >
  Fonts > "Editor's first font" can also put the font file or the bundled
  fonts ahead of SmileBASIC's font. This covers the editor's code, line
  numbers, ruler, status line, suggestions and hints. The console, DIRECT mode,
  `CHKCHR`, the font page on GRP5 and `GPUTCHR` still show SmileBASIC's font
  and its box, unless the programs' font setting under
  [Rendering and GPU](#rendering-and-gpu) changes the page. A character above
  U+FFFF, such as most emoji,
  takes two UTF-16 units and so two columns, as `LEN` counts it: its glyph
  fills the first cell and the second stays empty.
- File > Settings > Editor > "Narrow half-width characters" (off by default)
  draws ASCII and half-width kana half a cell wide in the code area and the
  ruler. SmileBASIC 4 draws every character in a square cell. The ruler then
  numbers every tenth column on a tile and marks columns 4, 6 and 8 between
  them, as tiles on every other column would run together. Each character
  still takes one column: the cursor's column, `HELPINFO`, error positions, the
  ruler's numbers and line wrapping count as before. Lines look shorter than on
  the console, and text that mixes half-width and full-width characters no
  longer lines up from one line to the next. See
  [importing.md](importing.md#half-width-characters) for the glyphs.
- `PRGEDIT` leaves the editor's line wrapping as it is: the PRG commands count
  lines of the source, which wrapping does not change.

### Editor in a separate window

With View > "Editor in a separate window", the editor opens in a second window
beside the screen, and programs and DIRECT mode go on while it is open. In
SmileBASIC 4 the editor replaces the screen and no program runs while it shows.

- Edits reach the slot at once, so RUN, SAVE and the PRG commands see them, and
  a slot that a program or LOAD changes shows the change in the editor.
- CONT and the debugger's commands leave it open. A step that waits for input
  brings the screen's window to the front for typing, and the editor's back when
  it stops.
- Position: with File > Settings > Editor > "Remember the editor window's
  position and size", the window opens where it was last.
- Size: making the window larger shows more lines and columns at the same
  character size, where SmileBASIC 4 shows the same lines at any size. The
  editor draws each of its pixels as a whole number of window pixels, so dot
  fonts stay sharp. F2 and Ctrl+B pick the whole-number zoom nearest to each
  character size on a 720-pixel-high display at the desktop's scale, lowered
  where it would leave fewer than 40 columns, as in the main window. With the
  16-pixel font at 100% scaling, Medium, Small and Tiny all draw it at its own
  size. F2 and View > Size go by this window's zooms, not the main window's,
  and skip or disable the sizes that look the same there.
- The menu bar, the debugger's panel, the help and the index follow the
  desktop's scaling setting, rounded to a whole number: 150% draws them at
  twice their size. They follow a change at once, as when you move the window
  to another display. On X11 the setting comes from `Xft.dpi`, and the
  `SDL_VIDEO_X11_SCALING_FACTOR` environment variable overrides it. You cannot
  make the window smaller than 800x450 at that scale.
- The emulator's other windows, such as Settings, Projects, the file dialogs
  and questions, keep the layout they have on the main window. On the editor's
  window they show at the desktop's scale when they fit, and shrink to fit
  when they don't. Each shows in the window that had the keys when it opened,
  and takes all input until it closes. A key, a click or the wheel in the
  other window brings that window to the front and goes no further. Where
  the system refuses to raise a window, as Wayland may, the window is
  flashed instead.
- A subprogram's screen shows over the screens rather than the editor's window,
  but what it types with `PUSHKEY` still goes into the editor while that window
  has the keys.
- `ENVLOCATE OUT X,Y`, which is not in the manual, follows the console's cursor
  while the editor is open in its own window. SmileBASIC 4 would give 0,0 for
  the editor.
