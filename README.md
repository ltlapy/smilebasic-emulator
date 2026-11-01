# SmileBASIC emulator

An emulator for SmileBASIC 4 programs, written in Rust with SDL3. It runs on
Windows, macOS and Linux. Support for SmileBASIC 3 and Petit Computer is
planned.

Early work: the language, the console, the program editor, file commands,
graphic pages, sprites (with animation and collisions), text screen
placement, layers, controllers and BGM from MML run. Sound effects (BEEP),
SmileBASIC's preset songs and its own instruments are not there yet. What
differs on purpose is in [docs/differences.md](docs/differences.md).

## Building

```sh
cargo build --release
```

SDL3 is built from source. On Linux, install the X11, Wayland and audio
development headers listed in SDL's `docs/README-linux.md` first.

## Resources

Without any setup, text uses bundled fonts under the SIL Open Font License
1.1: [GalmuriMono7](https://github.com/quiple/galmuri) at 8 pixels, and
[M PLUS 1p](https://mplusfonts.github.io/) Bold, the face SmileBASIC 4's own
font is drawn from, at 16 pixels.
SmileBASIC's own characters (number tiles, buttons, box drawing and other
symbols at U+E100-E17F and U+E200-E2FF) are redrawn for this emulator in
`crates/sb-runtime/fonts/symbols.txt`.

Music plays with [GeneralUser GS](https://www.schristiancollins.com/generaluser)
by S. Christian Collins, bundled in `crates/sb-sdl/sounds`, until
SmileBASIC's own instruments are imported. File > Settings > Sound picks
between them and another SoundFont (`.sf2`).

To see and hear SmileBASIC's own font, sprites and sounds, import them
from your own copy of
SmileBASIC 4: extract the RomFS and ExeFS of the game (and of its update, if
you have one) and run the separate importer

```sh
smilebasic-import BASE_ROMFS BASE_EXEFS UPDATE_ROMFS UPDATE_EXEFS
```

The importer copies `font.map`, `GDEFFONT`, the default sprite image
`GDEFGRP` and the English help (`HelpData.idx`, `HelpData.dat`) into the emulator's data directory, and takes the default sprite
definitions (SPDEF) out of the executable `main` and the instruments,
preset songs and sound effects out of the sound archive `petc4.bfsar`. List the update's
directories last so its files take precedence. It also copies SmileBASIC 4's
own projects `#SYS`, `#TOOL`, `#EXAMPLE` and `#GUIDE` into the projects
folder (`--projects DIR` for another), as plain files like the project
manager's Import, and never replaces a file already there. Only the font is required;
without the ExeFS, sprite definitions are a plain grid of 16x16 cells. The
first release's ExeFS has only definitions 0-4095; the others are empty.

`--sf2 FILE` also writes the instruments as a SoundFont, to play them in
other programs: `@0`-`@127` are bank 0, the drum sets `@128`-`@148` are
bank 128 (programs 0-20), and the rest are bank `@/128`, program `@%128`.
`--midi DIR` writes the preset songs into DIR as MIDI files, up to where
each has gone round its loop once, choosing instruments by the same banks
(bank select MSB and LSB) with a pitch bend range of 24 semitones.
Add `--gm` to number `@0`-`@127` and the drum sets as General MIDI and
Roland GS do: bank 0, and bank 128 with one GS kit program each (the nearest
by name where free). The songs then put drums on channel 10 and start with
GM System On and GS Reset. Instruments from `@149`, such as the tones of
`@256` and up that many songs use, have no GM sound and keep the banks above;
without the SoundFont, a player plays them as GM instruments.

## Running

```sh
smilebasic           # start in direct mode
smilebasic PROGRAM   # load PROGRAM into PRG0 and run it
```

`PROGRAM` is a SmileBASIC 4 `TXT` file or plain UTF-8 text.

Projects are folders in `projects/` in the data directory, `@WORKSPACE`
included. Programs are saved as plain UTF-8 text, with full-width letters and
digits turned into ASCII. The project manager imports and exports projects in
SmileBASIC 4's own files.

### Shortcuts
| Key | Action |
|---|---|
| Up, Down | In direct mode, bring back the lines typed before; Down returns to what was being typed |
| Left, Right, Home, End | Move along the line typed in direct mode or for `INPUT`; with Shift, select |
| F1 | Show SmileBASIC 4's help for the command at the cursor, in the editor or on the line typed in direct mode (needs the imported help); F2 there changes its text size |
| Ctrl+F1 to Ctrl+F5 | Type what `KEY` set for the key, `FILES` to `RUN` to start with |
| F4 | Switch between direct mode and the editor; after an error, open the editor at the failing line. With View > Editor in its own window, the editor opens beside the screen and programs keep running; F4 on the screen brings it to the front |
| F12 | Open the projects: switch (or double-click), load or run a program, copy files (or drag them onto a project) and delete them; right-click for a menu |
| Ctrl+1 to Ctrl+6 | Open slot PRG0 to PRG5 in the editor |
| Ctrl+0 | Leave the editor for direct mode |
| Ctrl+S | Save the edited slot: type a name, its own to start with |
| Ctrl+L | Load a program from a project's files into the edited slot; type to narrow the list |
| Pause/Break | Stop the program |
| F5, controller + | Run PRG0, or stop the running program; hold to stop it past `ON BREAK GOTO` |
| F8 | Show the menu bar and pause the program; press again to resume |
| F9, F10, F11 | Start the tools set for `TOOL1` to `TOOL3` (File > Settings > Tools) as subprograms, also while a program runs: SmileBASIC 4's SMILETOOL and GAHAKU at first, from the `#TOOL` project the importer copies |
| Shift+F7 | Set or clear a breakpoint on the cursor's line, as a click on its number does; the program stops before it and the editor opens there |
| Ctrl+F7 | Show or hide the debugger's panel below the editor; once shown it stays, so watches can be set before running |
| F6, F7, Shift+F6 | With a program stopped: run one statement, run over a DEF or GOSUB, or run out of the one under way |
| Ctrl+F6, Ctrl+Shift+F6 | Run to the cursor's line, or go on from it (within the stopped DEF) |
| Shift+F5, Ctrl+Shift+F5 | Let the stopped program go on, or run it again from the start |
| Ctrl+W | Wrap long lines in the editor, or scroll them |
| F2, Ctrl+B | Make the editor's characters smaller, then large again |
| Insert | Toggle overwriting in the editor |
| Ctrl+Home, Ctrl+End | Jump to the start or end of the program |
| Shift+cursor keys, mouse drag | Select text in the editor |
| Ctrl+A, Ctrl+X, Ctrl+C, Ctrl+V | Select all, cut, copy and paste in the editor and the line typed in direct mode |
| Ctrl+Z, Ctrl+Shift+Z (or Ctrl+Y) | Undo and redo in the editor |
| Ctrl+Enter, Ctrl+Backspace | Insert an empty line below and go there, or delete the line (also in direct mode) |
| Ctrl+Delete | Delete the rest of the line (also in direct mode) |
| Tab, Shift+Tab | Indent the selected lines, or unindent them |
| Ctrl+K, Ctrl+Shift+K | Comment out the selected lines, or uncomment them |
| Ctrl+G | Go to a line: type its number and press Enter |
| F3, Ctrl+F | Find as you type; Down, F3 or Enter for the next match, Up or Shift+F3 for the one before, Tab to replace |
| Ctrl+H | Replace: Enter replaces the selected match, Ctrl+Enter replaces them all, Tab goes back to finding |
| Ctrl+J | List the labels and DEFs; type to narrow the list, Enter to go there, Esc to go back |
| Esc | Close the help or the list beside the editor |
| Ctrl+/, Ctrl+Tab | Split the editor side by side, then one above the other, then join it again; move between the sides |
| Up, Down, Enter, Tab, Esc | While the list of words shows: pick a word and take it with Enter, or take the first with Tab; Esc closes it. In direct mode Up and Down keep to the history (File > Settings > Editor) |
| Ctrl+Space | Hide the list of words and the hints, or show them again |
| Ctrl+Shift+V | Paste anywhere; outside the editor the text is typed as keys |

### Buttons

Gamepads work as controllers 1 to 4. The keyboard also presses controller
1's buttons, unless *Keys press buttons* is turned off in the Run menu:

| Key | Button |
|---|---|
| Cursor keys | Up, down, left, right |
| Z, X, A, S | A, B, X, Y |
| Q, W | L, R |
| 1, 2 | ZL, ZR |

Without a gamepad, the first time a program reads the buttons, a picture of
these keys shows for a few seconds. The mouse's left button touches the screen.

## License

MIT OR Apache-2.0. The bundled fonts in `crates/sb-runtime/fonts/galmuri`
and `crates/sb-runtime/fonts/mplus` are under the SIL Open Font License
1.1. GeneralUser GS in `crates/sb-sdl/sounds` is under its own license, in
`GeneralUser-GS-LICENSE.txt` there.
