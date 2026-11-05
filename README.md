# SmileBASIC emulator

An emulator for SmileBASIC 4 programs, written in Rust with SDL3. It targets
Windows, macOS and Linux; so far it has only been run on Linux on x86-64.
Support for SmileBASIC 3 and Petit Computer is planned.

The emulator is still in development. The language, the console, the program
editor, file commands, graphic pages, sprites (with animation and collisions),
text screen placement, layers, filters, controllers and BGM from MML run, as do
the debugger and subprograms. BEEP, SmileBASIC 4's preset songs and its own
instruments play once imported from your own copy. Rendering uses the GPU where
a start-up test passes, and otherwise the CPU. What differs on purpose is in
[differences.md](docs/differences.md).

## Building

```sh
cargo build --release
```

For a build to hand out, `cargo build --profile dist` puts it in `target/dist`.
It takes twice as long and runs up to 16% faster.

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

### Shortcuts

#### Typing and direct mode

| Key | Action |
|---|---|
| Up, Down | In direct mode, bring back the lines typed before; Down returns to what was being typed |
| Left, Right, Home, End | Move along the line typed in direct mode or for `INPUT`; with Shift in direct mode, select |
| Ctrl+F1 to Ctrl+F5 | Type what `KEY` set for the key, `FILES` to `RUN` to start with |
| Ctrl+A, Ctrl+X, Ctrl+C, Ctrl+V | Select all, cut, copy and paste in the editor and the line typed in direct mode |
| Ctrl+Backspace | Delete the line, in the editor and in direct mode |
| Ctrl+Delete | Delete the rest of the line, in the editor and in direct mode |
| Ctrl+Shift+V | Paste anywhere; outside the editor the text is typed as keys |
| Up, Down, Enter, Tab, Esc | While the list of words shows: pick a word and take it with Enter, or take the first with Tab; Esc closes it. In direct mode Up and Down keep to the history (File > Settings > Editor) |
| Ctrl+Space | Hide the list of words and the hints, or show them again |

#### Editor

| Key | Action |
|---|---|
| Ctrl+1 to Ctrl+6 | Open slot PRG0 to PRG5 in the editor |
| Ctrl+0 | Leave the editor for direct mode |
| Ctrl+S | Save the edited slot: type a name, its own to start with |
| Ctrl+L | Load a program from a project's files into the edited slot; type to narrow the list |
| Shift+cursor keys, mouse drag | Select text in the editor |
| Ctrl+Z, Ctrl+Shift+Z (or Ctrl+Y) | Undo and redo in the editor |
| Ctrl+Enter | Insert an empty line below and go there |
| Tab, Shift+Tab | Indent the selected lines, or unindent them |
| Ctrl+K, Ctrl+Shift+K | Comment out the selected lines, or uncomment them |
| Insert | Toggle overwriting in the editor |
| Ctrl+W | Wrap long lines in the editor, or scroll them |
| F2, Ctrl+B | Make the editor's characters smaller, then large again |

#### Find and navigate

| Key | Action |
|---|---|
| Ctrl+Home, Ctrl+End | Jump to the start or end of the program |
| Ctrl+G | Go to a line: type its number and press Enter |
| F3, Ctrl+F | Find as you type; Down, F3 or Enter for the next match, Up or Shift+F3 for the one before, Tab to replace |
| Ctrl+H | Replace: Enter replaces the selected match, Ctrl+Enter replaces them all, Tab goes back to finding |
| Ctrl+J | List the labels and DEFs; type to narrow the list, Enter to go there, Esc to go back |
| Esc | Close the help or the list beside the editor |

#### Running and debugging

| Key | Action |
|---|---|
| F5, controller + | Run PRG0, or stop the running program; hold for a second to stop it past `ON BREAK GOTO` |
| Pause/Break | Stop the program |
| Ctrl+C | Stop the program, if the setting for it is on (File > Settings); off by default, and the editor still copies |
| F8 | Show the menu bar and pause the program; press again to resume |
| Shift+F7 | Set or clear a breakpoint on the cursor's line, as a click on its number does; the program stops before it and the editor opens there |
| Ctrl+F7 | Show or hide the debugger's panel below the editor; once shown it stays, so watches can be set before running |
| F6, F7, Shift+F6 | With a program stopped: run one statement, run over a DEF or GOSUB, or run out of the one under way |
| Ctrl+F6, Ctrl+Shift+F6 | Run to the cursor's line, or go on from it (within the stopped DEF) |
| Shift+F5, Ctrl+Shift+F5 | Let the stopped program go on, or run it again from the start |

#### Windows and tools

| Key | Action |
|---|---|
| F1 | Show SmileBASIC 4's help for the command at the cursor, in the editor or on the line typed in direct mode (needs the imported help) |
| F2 | While the help has the focus, change its text size |
| F4 | Switch between direct mode and the editor; after an error, open the editor at the failing line. With View > Editor in a separate window, the editor opens beside the screen and programs keep running; F4 on the screen brings it to the front |
| F9, F10, F11 | Start the tools set for `TOOL1` to `TOOL3` (File > Settings > Tools) as subprograms, also while a program runs: SmileBASIC 4's SMILETOOL and GAHAKU at first, from the `#TOOL` project the importer copies |
| F12 | Open the projects: run (or double-click) or switch, preview and view files, load or run a program, copy files (or drag them onto a project); Space expands the info to edit a project's title, description and icon; the menu key or right-click for a menu |
| Ctrl+/, Ctrl+Tab | Split the editor side by side, then one above the other, then join it again; move between the sides |

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

- [differences.md](docs/differences.md): where the emulator differs from SmileBASIC 4 on purpose
- [sb4-pitfalls.md](docs/sb4-pitfalls.md): SmileBASIC 4 behavior the manual leaves open, as checked
- [importing.md](docs/importing.md): importing resources, and exporting instruments and songs
- [file-formats.md](docs/file-formats.md): SmileBASIC 4's project and file formats
- [hardware-tests.md](docs/hardware-tests.md): comparing the emulator with SmileBASIC 4
- [benchmarking.md](docs/benchmarking.md): how to measure performance
- [benchmark-results.md](docs/benchmark-results.md): performance results
- [renderer.md](docs/renderer.md): how the emulator draws a frame
- [renderer-verification.md](docs/renderer-verification.md): rendering rules and how they were checked
- [renderer-decisions.md](docs/renderer-decisions.md): choices made for the GPU renderer
- [building-handheld.md](docs/building-handheld.md): building for an aarch64 handheld
- [security.md](docs/security.md): how the emulator treats untrusted programs and files
- [CONTRIBUTING.md](CONTRIBUTING.md): code and commit rules

## License

Licensed under either of [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE)
(`MIT OR Apache-2.0`), at your option. Unless you state otherwise, any
contribution you intentionally submit for inclusion is dual-licensed the same
way, with no additional terms or conditions.

The bundled fonts in `crates/sb-runtime/fonts/galmuri` and
`crates/sb-runtime/fonts/mplus` are under the SIL Open Font License 1.1.
GeneralUser GS in `crates/sb-sdl/sounds` is under its own license, in
`GeneralUser-GS-LICENSE.txt` there.
