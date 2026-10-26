# SmileBASIC emulator

An emulator for SmileBASIC 4 programs, written in Rust with SDL3. It runs on
Windows, macOS and Linux. Support for SmileBASIC 3 and Petit Computer is
planned.

Early work: the language, the console, the program editor, file commands,
graphic pages, sprites (with animation and collisions), text screen
placement and layers run. Sound and controllers are not implemented yet.

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

To see SmileBASIC's own font and sprites, import them from your own copy of
SmileBASIC 4: extract the RomFS and ExeFS of the game (and of its update, if
you have one) and run

```sh
smilebasic import BASE_ROMFS BASE_EXEFS UPDATE_ROMFS UPDATE_EXEFS
```

The importer copies `font.map`, `GDEFFONT` and the default sprite image
`GDEFGRP` into the emulator's data directory, and takes the default sprite
definitions (SPDEF) out of the executable `main`. List the update's
directories last so its files take precedence. Only the font is required;
without the ExeFS, sprite definitions are a plain grid of 16x16 cells. The
first release's ExeFS has only definitions 0-4095; the others are empty.

## Running

```sh
smilebasic           # start in direct mode
smilebasic PROGRAM   # load PROGRAM into PRG0 and run it
```

`PROGRAM` is a SmileBASIC 4 `TXT` file or plain UTF-8 text.

Projects are folders in `projects/` in the data directory, `@WORKSPACE`
included. Programs are saved as plain UTF-8 text, with full-width letters and
digits turned into ASCII.

### Shortcuts
| Key | Action |
|---|---|
| F4 | Switch between direct mode and the editor; after an error, open the editor at the failing line |
| Ctrl+1 to Ctrl+6 | Open slot PRG0 to PRG5 in the editor |
| Pause/Break | Stop the program |
| Ctrl+W | Wrap long lines in the editor, or scroll them |
| Insert | Toggle overwriting in the editor |
| Ctrl+Home, Ctrl+End | Jump to the start or end of the program |

## License

MIT OR Apache-2.0. The bundled fonts in `crates/sb-runtime/fonts/galmuri`
and `crates/sb-runtime/fonts/noto-cjk` are under the SIL Open Font License
1.1.
