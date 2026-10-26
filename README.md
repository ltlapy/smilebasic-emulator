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
SmileBASIC's own characters (number tiles, buttons, box drawing and other
symbols at U+E100-E17F and U+E200-E2FF) are redrawn for this emulator in
`crates/sb-runtime/fonts/symbols.txt`.

To see SmileBASIC's own font, import it from your own copy of SmileBASIC 4:
extract the RomFS of the game (and of its update, if you have one) and run

```sh
smilebasic import BASE_ROMFS UPDATE_ROMFS
```

The importer copies `font.map` and `GDEFFONT` into the emulator's data
directory. List the update's RomFS last so its files take precedence.

## Running

```sh
smilebasic           # start in direct mode
smilebasic PROGRAM   # load PROGRAM into PRG0 and run it
```

`PROGRAM` is a SmileBASIC 4 `TXT` file or a plain UTF-8 text file. In direct
mode, typed lines run immediately; `RUN [slot]`, `NEW [slot]` and `CLEAR` work
on the program slots PRG0-PRG5. Letters type in upper case unless Shift or
Caps Lock is on. Pause/Break stops the running program.

F4 switches between direct mode and the program editor. The first F4 after
a program error opens the editor at the error, as `LIST ERR` does; `LIST`,
`LIST line` and `LIST slot:line` open it at a given place. In the editor,
Insert switches to overwriting, Ctrl+Home and Ctrl+End jump to the start and
end of the program, and Ctrl+W wraps long lines instead of scrolling sideways.

Move the mouse to the top edge of the window for the menu bar: it can stop
the program, open the editor and switch the options above (upper-case
letters, Ctrl+C to stop, half-width on SAVE, wrapping in the editor).
Settings are kept in `settings.txt` in the data directory.

`LOAD`, `SAVE`, `FILES`, `PROJECT`, `CHKFILE`, `DELETE` and `RENAME` work on
project folders in the emulator's data directory (`projects/`; the workspace
project is `@WORKSPACE`). Programs are saved as plain UTF-8 text, so they can
be edited with other tools; `SAVE` turns full-width letters and digits into
ASCII. Close the window to
quit. As on SmileBASIC 4, file commands ask before reading
or writing (a running program's `LOAD` only when it would replace unsaved
edits); Options > File access can turn that into short notices or silence
it. Slots with edits that were not saved are marked with `*` in
the editor's status line, and quitting, `LOAD`, `NEW` and `PROJECT` always
ask before those edits could be lost (a cancelled file command sets
`RESULT()` to -1). Esc and the other keys go to the program.

## License

MIT OR Apache-2.0. The bundled font in `crates/sb-runtime/fonts/galmuri`
is under the SIL Open Font License 1.1.
