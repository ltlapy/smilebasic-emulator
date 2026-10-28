# Where the emulator differs from SmileBASIC 4 on purpose

What the emulator leaves out or does another way, by choice or for want
of SmileBASIC's own data. Guesses about behaviour nobody has checked on
hardware yet are not listed here: they are marked `UNVERIFIED` in the
source, and what has been checked is in [sb4-pitfalls.md](sb4-pitfalls.md).

## Without SmileBASIC's own sounds

SmileBASIC 4 keeps its instruments, sound effects and preset songs in its
sound archive, which the emulator cannot ship. Until they can be
imported from your own copy:

- Preset songs (`BGMPLAY 0` to `45`) play silence, until stopped, so
  that programs waiting on `BGMCHK` still wait.
- Preset sound effects (`BEEP 0` to `155`) are silent. They still take
  a handle.
- MML's sound effect tones (`@256` and up) are silent.
- Instruments `@0` to `@127` come from a General MIDI SoundFont
  (GeneralUser GS, or one chosen in Options), so they sound different.
- The drum sets `@128` to `@148` are the GS drum kits nearest by name.

## Sound that is not played

- The effector (`EFCEN`, `EFCSET`, `EFCWET`, `BGMWET`) keeps its
  settings but does not change the sound.
- `TALK` speaks with a voice of the emulator's own, a Klatt formant
  synthesizer (klattsch-core) reading the kana or phoneme letters, so it
  says the same words in another voice. In MML, a sung note (`*WA,C4`)
  plays as a plain note.
- MML's tremolo (`@MA`) and pan modulation (`@ML`) are read and
  ignored. Vibrato (`@MP`) is the SoundFont's own, through the
  modulation wheel. Envelopes (`@E`) shape the PSG and user waveforms
  only.
- `SNDMSBAL` keeps its value; there are no subprograms to balance.

## Input

- There are no motion sensors: `ACCEL`, `GYROA` and `GYROV` read 0.
- The mouse's left button is the touch screen (touch 0 only).
- Controller 1 is the keyboard and the first gamepad together, reported
  as a handheld controller; other gamepads are Pro Controllers.
  `XCTRLSTYLE` shows no connection screen and always succeeds.
- The keyboard also presses controller 1's buttons; Options turns this
  off.

## Other

- `RND` and `RNDF` use a generator of their own, so a seeded series
  gives other numbers than on hardware.
- Error messages add detail in parentheses, such as which argument was
  out of range.
- File commands ask, notify or stay silent as set in Options, rather
  than always asking. A file command that fails in a program shows a
  notice, rather than a dialog to close, unless file access is silent.
- `SAVE` and `SAVEV "TXT:..."` write plain UTF-8 text, and `SAVEG` and
  `SAVEV "GRP:..."` write PNG images, so the files open on a computer.
  `DAT` files keep SmileBASIC 4's layout. All of these are read back, as
  are files from SmileBASIC 4.
- `METASAVE` keeps a project's title and description in `@META.TXT`
  and its icon in `@ICON.PNG`, in the project's folder. SmileBASIC 4's
  own `META` file is neither read nor written, since its layout is not
  known.
- There are no subprograms yet, so `ENVTYPE`, `ENVSTAT` and the other
  `ENV` commands always answer for the main interpreter, where `ENVLOAD`
  and `ENVSAVE` do nothing.
- `DIALOG` is drawn as the emulator's own window, in English. Besides
  the controller's A and B, it answers clicks and Enter.
- `PRGEDIT` leaves the editor's line wrapping as it is: the PRG commands
  count lines of the source, which wrapping does not change.
- The screen's scaling filter can be chosen in Options instead of
  following `XSCREEN`.

## Editor

- The editor's colors start as SmileBASIC 4's, and can be swapped for
  preset palettes from other editors, typed in as hex values, or loaded
  from and saved to text files. Selected text has a color of its own.
- The search match color is kept in palettes, but the editor has no
  search yet.
- Besides SmileBASIC 4's three sizes (21, 28 and 43 lines of code), the
  editor has a smaller one with 57, about 60 rows with the ruler, the
  status line and the room left for the menu bar.
