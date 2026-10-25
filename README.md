# SmileBASIC emulator

An emulator for SmileBASIC 4 programs, written in Rust with SDL3. It runs on
Windows, macOS and Linux. Support for SmileBASIC 3 and Petit Computer is
planned.

Early work: the language runs, and PRINT output appears on the console.
Graphics, sprites, sound and input are not implemented yet.

## Building

```sh
cargo build --release
```

SDL3 is built from source. On Linux, install the X11, Wayland and audio
development headers listed in SDL's `docs/README-linux.md` first.

## Font

Without any setup, the console uses a bundled font,
[GalmuriMono7](https://github.com/quiple/galmuri) (SIL Open Font License 1.1).

To see SmileBASIC's own font, import it from your own copy of SmileBASIC 4:
extract the RomFS of the game (and of its update, if you have one) and run

```sh
smilebasic import BASE_ROMFS UPDATE_ROMFS
```

The importer copies `font.map` and `GDEFFONT` into the emulator's data
directory. List the update's RomFS last so its files take precedence.

## Running a program

```sh
smilebasic PROGRAM
```

`PROGRAM` is a SmileBASIC 4 `TXT` file or a plain UTF-8 text file. Esc quits.

## License

MIT OR Apache-2.0. The bundled font in `crates/sb-runtime/fonts/galmuri`
is under the SIL Open Font License 1.1.
