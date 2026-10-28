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
1.1: [GalmuriMono7](https://github.com/quiple/galmuri) at 8 pixels, and a
subset of [Noto Sans Mono CJK JP](https://github.com/notofonts/noto-cjk) at
16 pixels.
SmileBASIC's own characters (number tiles, buttons, box drawing and other
symbols at U+E100-E17F and U+E200-E2FF) are redrawn for this emulator in
`crates/sb-runtime/fonts/symbols.txt`.

Music plays with [GeneralUser GS](https://www.schristiancollins.com/generaluser)
by S. Christian Collins, bundled in `crates/sb-sdl/sounds`, until
SmileBASIC's own instruments are imported. Options > Instruments picks
between them and another SoundFont (`.sf2`).

To see and hear SmileBASIC's own font, sprites and instruments, import them
from your own copy of
SmileBASIC 4: extract the RomFS and ExeFS of the game (and of its update, if
you have one) and run the separate importer

```sh
smilebasic-import BASE_ROMFS BASE_EXEFS UPDATE_ROMFS UPDATE_EXEFS
```

The importer copies `font.map`, `GDEFFONT` and the default sprite image
`GDEFGRP` into the emulator's data directory, and takes the default sprite
definitions (SPDEF) out of the executable `main` and the instruments out
of the sound archive `petc4.bfsar`. List the update's
directories last so its files take precedence. Only the font is required;
without the ExeFS, sprite definitions are a plain grid of 16x16 cells. The
first release's ExeFS has only definitions 0-4095; the others are empty.

`--sf2 FILE` also writes the instruments as a SoundFont, to play them in
other programs: `@0`-`@127` are bank 0, the drum sets `@128`-`@148` are
bank 128 (programs 0-20), and the rest are bank `@/128`, program `@%128`.

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
| F4 | Switch between direct mode and the editor; after an error, open the editor at the failing line |
| F12 | Open the projects: switch, load a program into a slot, or run a project's MAIN.PRG |
| Ctrl+1 to Ctrl+6 | Open slot PRG0 to PRG5 in the editor |
| Pause/Break | Stop the program |
| Ctrl+W | Wrap long lines in the editor, or scroll them |
| F2, Ctrl+B | Make the editor's characters smaller, then large again |
| Insert | Toggle overwriting in the editor |
| Ctrl+Home, Ctrl+End | Jump to the start or end of the program |
| Shift+cursor keys, mouse drag | Select text in the editor |
| Ctrl+A, Ctrl+X, Ctrl+C, Ctrl+V | Select all, cut, copy and paste in the editor |
| Ctrl+Shift+V | Paste anywhere; outside the editor the text is typed as keys |

### Buttons

Gamepads work as controllers 1 to 4. The keyboard also presses controller
1's buttons, unless *Keys press buttons* is turned off in Options:

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
and `crates/sb-runtime/fonts/noto-cjk` are under the SIL Open Font License
1.1. GeneralUser GS in `crates/sb-sdl/sounds` is under its own license, in
`GeneralUser-GS-LICENSE.txt` there.
