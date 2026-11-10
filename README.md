# SmileBASIC emulator

An emulator for SmileBASIC 4 programs, written in Rust with SDL3.

Still in development.

## Features

- [x] SmileBASIC 4 compatible ([differences](docs/differences.md))
- [x] Rich debugging tools
- [x] GPU rendering
- [ ] SmileBASIC 3 and Petit Computer

## Building

```sh
cargo build --release

# Optimized build (up to 16% faster, x2 build time)
cargo build --profile dist
```

SDL3 is built from source. On Linux, install the X11, Wayland and audio
development headers listed in SDL's `docs/README-linux.md` first.

The version shown at start-up and in Help > About ends with the commit the
build came from, as in `0.1.0+4e42491`. Set `SB_COMMIT` to name a commit
yourself, for example when building outside a git checkout, or set it empty
to leave the commit out.

For Windows, see [building-windows.md](docs/building-windows.md). For an aarch64
handheld, see [building-handheld.md](docs/building-handheld.md).

## Resources

Text and music work without any setup, using bundled fonts and SoundFont. 

To use SmileBASIC 4's own font, sprites and sounds, import them from your own copy:

```sh
smilebasic-import BASE_ROMFS BASE_EXEFS UPDATE_ROMFS UPDATE_EXEFS
```

See [importing.md](docs/importing.md).

## Running

```sh
smilebasic           # start in DIRECT mode
smilebasic PROGRAM   # load PROGRAM into PRG0 and run it
```

`PROGRAM` is a SmileBASIC 4 `TXT` file or plain UTF-8 text.

Projects are folders in `projects/` in the data directory, `@WORKSPACE`
included. Programs are saved as plain UTF-8 text, with full-width letters and
digits turned into ASCII. The project manager imports and exports projects in
SmileBASIC 4's own files. Run on a project returns to the projects when the
program ends or is stopped; after an error it waits for A or Enter.

File > Settings > Screen > Renderer: Auto, the default, uses GPU rendering if
the start-up test passes; CPU is exact but slower.

Gamepads work as controllers 1 to 4, and the keyboard as controller 1. Help >
Keyboard shortcuts lists the keys, as does [shortcuts.md](docs/shortcuts.md).

## Documentation

See [docs/index.md](docs/index.md).

## License

Licensed under either of [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE)
(`MIT OR Apache-2.0`), at your option. Unless you state otherwise, any
contribution you intentionally submit for inclusion is dual-licensed the same
way, with no additional terms or conditions.

The bundled fonts in `crates/sb-runtime/fonts/fusionpixel`,
`crates/sb-runtime/fonts/galmuri`, `crates/sb-runtime/fonts/mplus`,
`crates/sb-runtime/fonts/neodgm`, `crates/sb-runtime/fonts/notosanssc` and
`crates/sb-runtime/fonts/unifont`
are under the SIL Open Font License 1.1.
Misaki Gothic 2nd in `crates/sb-runtime/fonts/misaki` is under its author's
license, and Izumi Gothic 16 in `crates/sb-runtime/fonts/izumi` is
equivalent to public domain by its author's declaration; each has its
`LICENSE.txt` there.
GeneralUser GS in `crates/sb-sdl/sounds` is under its own license, in
`GeneralUser-GS-LICENSE.txt` there.
