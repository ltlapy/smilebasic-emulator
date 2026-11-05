# SmileBASIC emulator

An emulator for SmileBASIC 4 programs, written in Rust with SDL3. Still in
development.

## Features

- [x] SmileBASIC 4's language, console, graphics, sprites, layers and sound;
  [differences.md](docs/differences.md) lists what differs on purpose
- [x] SmileBASIC 4's own font, sprites and sounds, imported from your own copy
- [x] Program editor, project manager and subprograms
- [x] Debugger with breakpoints, stepping and watches
- [x] GPU rendering, with the CPU as a fallback
- [ ] Windows and macOS: both are targets, but it has only been run on Linux on
  x86-64
- [ ] SmileBASIC 3 and Petit Computer profiles

## Building

```sh
cargo build --release

# Optimized build, in target/dist: up to 16% faster, takes twice as long to build
cargo build --profile dist
```

SDL3 is built from source. On Linux, install the X11, Wayland and audio
development headers listed in SDL's `docs/README-linux.md` first.

For an aarch64 handheld, see [building-handheld.md](docs/building-handheld.md).

## Resources

Without any setup, text uses bundled fonts under the SIL Open Font License
1.1: [GalmuriMono7](https://github.com/quiple/galmuri) at 8 pixels, and
[M PLUS 1p](https://mplusfonts.github.io/) Bold, the face SmileBASIC 4's own
font is drawn from, at 16 pixels. SmileBASIC's own characters (number tiles,
buttons, box drawing and other symbols at U+E100-E17F and U+E200-E2FF) are
redrawn for this emulator in `crates/sb-runtime/fonts/symbols.txt`.

Music plays with [GeneralUser GS](https://www.schristiancollins.com/generaluser)
by S. Christian Collins, bundled in `crates/sb-sdl/sounds`, until SmileBASIC's
own instruments are imported. File > Settings > Sound picks between them and
another SoundFont (`.sf2`).

To see and hear SmileBASIC's own font, sprites and sounds, import them from your
own copy of SmileBASIC 4:

```sh
smilebasic-import BASE_ROMFS BASE_EXEFS UPDATE_ROMFS UPDATE_EXEFS
```

[importing.md](docs/importing.md) describes what it copies, and how to export
the instruments and songs for other programs.

## Running

```sh
smilebasic           # start in direct mode
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

Help > Keyboard shortcuts lists the keys, as does [shortcuts.md](docs/shortcuts.md).

### Buttons

Gamepads work as controllers 1 to 4. The keyboard also presses controller
1's buttons, unless *Use the keyboard as a controller* is turned off in the Run
menu:

| Key | Button |
|---|---|
| Cursor keys | Up, down, left, right |
| Z, X, A, S | A, B, Y, X |
| Q, W | L, R |
| 1, 2 | ZL, ZR |

A key a program reads itself, with `KEYBOARD` or `INKEY$`, presses no button
until a second after the program stops reading it. So typing programs get
their letters, and a game keeps these keys outside its name entry.

Without a gamepad, the first time a program reads the buttons, a picture of
these keys shows for a few seconds. The mouse's left button touches the screen.

In direct mode, a gamepad's − (Select) opens the projects. There A runs the
project's MAIN.PRG or the selected file (or views a file that is not a
program), X moves between the projects and the files, L and R move a page, Y
expands the info and B goes back. The left stick works as the D-pad there, and
holding up or down repeats, faster the longer it is held. The menu opens with
the mouse or keys only.

## Documentation

See [docs/index.md](docs/index.md).

## License

Licensed under either of [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE)
(`MIT OR Apache-2.0`), at your option. Unless you state otherwise, any
contribution you intentionally submit for inclusion is dual-licensed the same
way, with no additional terms or conditions.

The bundled fonts in `crates/sb-runtime/fonts/galmuri` and
`crates/sb-runtime/fonts/mplus` are under the SIL Open Font License 1.1.
GeneralUser GS in `crates/sb-sdl/sounds` is under its own license, in
`GeneralUser-GS-LICENSE.txt` there.
