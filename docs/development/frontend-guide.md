# Working on the frontend

This document is for anyone changing `crates/sb-sdl`. It covers where the
window's code and state are, where new code goes, and how to check a change to
it. [working-notes.md](working-notes.md) has what holds for the whole
repository; this adds what is specific to the frontend.

## The loop

`frontend()` in `main.rs` is the window's loop. Each iteration does this,
in this order:

1. Replaces a renderer that failed, and notices a wake from a suspend.
2. Lets the bench act (`SB_UI_BENCH`, see [benchmarking.md](../performance/benchmarking.md)).
3. Works out where things go in the windows.
4. Handles the events.
5. Runs the program's frames (`run_frames`).
6. Tidies up: notices, dialogs the machine asks for, settings to save.
7. Asks the `Pacer` whether to draw, and if so draws and presents.
8. Waits for the next frame.

An iteration is not always one frame. It usually runs one frame of the
program, but it runs none while the program is paused and several when the
program has to catch up. It may also skip drawing. "Iteration" refers to the
loop, and "frame" refers to the program.

## Where state lives

The loop's state is in structs, grouped by purpose. A new piece of state
goes into the struct it belongs to. Add a local to `frontend()` only if no
struct fits.

| Struct | Module | Holds |
|---|---|---|
| `Screens` | `main.rs` | The emulator's own windows and dialogs: the dialog, Projects, the file dialogs, settings, shortcuts, colors, the Simple UI's list, pause menu and welcome screen |
| `Inputs` | `inputs.rs` | Controllers, typing, the pad's repeat, the drag, the + button's hold |
| `Debugger` | `main.rs` | The debugger's panel, its menu, the stop it last showed |
| `SidePanel` | `main.rs` | The help or the index beside the editor |
| `Overlays` | `overlays.rs` | The layers drawn over the screens, and the drawn cursor |
| `Held` | `main.rs` | Textures kept in addition to the main frame texture, and what the frame textures hold |
| `gpu::Paths` | `gpu.rs` | The GPU paths for the main screens and for a subprogram's screen |
| `Pacer` | `pacing.rs` | The frame clock, the cost of a drawing, the skipped drawings |

## Where code goes

| To add or change | Go to |
|---|---|
| What a screen does with an event | `events.rs`: one function per screen, given `Ctx` |
| What a menu bar item does | `events::menu_action` |
| A key that does what a menu item does | The key gives the `Action` and calls `menu_action` (`debug_key`, `events::editor_ctrl_action`, the `act!` macro) |
| The Simple UI's hotkeys (Select+A, Select+Start, F8) | `events::pause_hotkeys` |
| Whether Select is held | `stick_mouse::Select`, the only place that follows it |
| Uploading the frame to the main window's texture | `upload_frame` in `main.rs` |
| Drawing a subprogram's screen | `draw_sub_screen` in `main.rs` |
| What a dialog and a program's DIALOG lay out and draw the same way | The functions at the end of `program_dialog.rs` (`draw_window`, `button_rect`, ...) |
| What the UI looks like on a frame | `draw::Ui::draw` |
| Copying layers to a window | `Overlays::copy_main`, `copy_editor` |
| Where something goes in a window, or which pixel the pointer is over | `placement.rs` |
| When to draw, how long a frame may run | `pacing.rs` |
| The window's size, shape, which window takes input | `window.rs` |
| A notice in the bottom right corner | `corner.rs` |

A new screen needs four things: a field in `Screens`, its place in
`Screens::any`, a handler in `events.rs`, and its drawing in
`draw::Ui::draw`. Call the handler from the loop at the place its layer has
in the order. A new layer or texture that lives on the renderer needs its
line in `Overlays::forget` or `Held::forget_device`. Without it, the layer
or texture survives the loss of its renderer.

## What stays in the loop, and why

Two parts of the event handling are still in `frontend()` on purpose.

- **The dispatch.** The arms of the last `match event` decide by key,
  window and state who gets the event. Their order and their guards are the
  behavior. An arm is a few lines; keep it short, and put anything longer in a
  function it calls.
- **The window's own events.** Renderer resets, focus, raising a window.
  They touch the canvas, the textures and the windows at the same time.

The main frame's texture and its kind are still locals of `frontend()`,
handed to `upload_frame`. They are replaced together with the renderer,
and their place among the locals decides when they are dropped (see
"Drop order" below).

The loop has six local macros (`drawing!`, `lend!`, `stick_role!`, `cx!`,
`choosers!`, `act!`). Each exists because a function could not borrow what
it needs. Do not add a seventh: group the state it would capture into a
struct and pass that.

## Drop order

Rust drops locals in reverse order of declaration, and nothing checks that
a texture goes before its renderer. In `frontend()`, a variable that holds
textures is declared after the canvas and the window they belong to:
`textures` comes after `editor_gfx` because some of its textures belong to
the editor window. Destroying a texture after its renderer is a use after
free. SDL hides it by default, and `SDL_INVALID_PARAM_CHECKS=1` shows it.
`tests/editor_window.rs` quits with that variable set.
When state that owns textures moves to a new struct or a new place, keep
that order.

## Things that look the same and are not

- **`screens.any()` at different points of an iteration.** The events between two
  calls open and close windows. Ask where the loop asks; do not cache one
  answer for the iteration.
- **The order in which screens get an event.** A dialog first, then the
  welcome screen, the pause menu, and so on down to the program. A screen
  that takes the event ends its handling. Moving a block moves a priority.
- **A key and its menu item.** Most are the same and share `menu_action`.
  F1 is not: in DIRECT mode it shows the help over the screen and leaves
  the editor closed, while the menu item opens the editor. Before merging
  two paths, compare their guards and bodies line by line.
- **A menu item's check for the editor.** `menu_action` opens the editor
  first for items that need it. A key arm that requires the editor's focus
  never reaches that branch, so the two agree; a new caller may not.

## Checking a change

[working-notes.md](working-notes.md#checking-a-change) says what to run and
how a comparison of two builds goes wrong. For the frontend, the comparison
is of pictures: run the emulator headless and compare shots of the old
build and the new build.

```
SDL_VIDEO_DRIVER=offscreen SDL_AUDIO_DRIVER=dummy XDG_DATA_HOME=SCRATCH \
SB_UI_BENCH="load=PROJECT/PROGRAM,editor,menu,at=100:click:20:10,shot=OUT,novsync,frames=200" \
smilebasic
```

`at=ITERATION:` presses keys (`key:ctrl+S`), controller buttons (`pad:a`,
`padup:a`), and uses the mouse (`click:X:Y`, `rclick`, `dclick`, `press`,
`drag`, `release`, `move`, `wheel:X:Y:N`); [benchmarking.md](../performance/benchmarking.md)
lists the rest. A scene is one such line. A change is checked by several dozen
of them, each run on both builds, with the shots compared byte for byte.

`tools/ui_compare/compare.sh` does that for the scenes in `scenes.txt`
beside it, with the programs in its `PERF` folder:

```
tools/ui_compare/compare.sh BEFORE AFTER OUT_DIR [NAMES_REGEX]
```

`BEFORE` and `AFTER` are the two binaries. Build the old one from a clean
checkout of its commit: a build made with the new changes still in the tree
is the new build, and the script refuses two equal binaries. A full run
takes about 15 minutes, and a build running at the same time changes the
timing of some scenes. When a change touches a screen no scene reaches, add
a scene.

Notes on scenes:

- A program named `TEST` is the file `TTEST` in its project folder. With
  the wrong name the bench prints "could not run" and the scene shows an
  idle screen.
- A scene meant for a running program needs one that waits in a `VSYNC`
  loop and shows a still picture. `BUTTON()` without an argument is an
  error, and the program stops there.
- Copy both binaries to one folder. A binary in `target/debug` behaves
  differently in the bench from a copy elsewhere.
- DIRECT mode's banner, Help > About and the Simple UI's pause menu print
  the commit. `shothash.py` skips the pixels of the banner and the pause menu;
  a scene that ends on About differs between two builds even with no other
  change, unless both were built with the same `SB_COMMIT`.
- A scene that opens something in the first iterations shows the state the
  program reached in its first frame. Let the program settle first
  (`at=60:menu` in place of `menu`).
- Two runs of one build differ in about one scene in a hundred over a long
  run: a blinking text cursor's phase moves when an event lands an iteration
  later. Run a scene that differed again by itself, and keep the output of
  the run that differed.
- When a program takes about as long per iteration as a frame allows, the
  scene's result depends on the computer's load.

The offscreen driver draws with the desktop's GPU. A change to the loop's
timing or to the GPU path also needs a run on the slowest target, which the
bench cannot stand in for.

## Traps in what we build on

- The sdl3 crate's `push_event` refuses the renderer's events
  (`RenderTargetsReset`, `RenderDeviceReset`) and returns an error. Push
  them with `push_render_event`, which writes a raw SDL event.
- `let _ = fallible()` hid that error: the wake from a suspend
  pushed a reset that never arrived, and two bench actions did nothing.
  Before discarding a result, be sure a failure there cannot matter, and
  check once that the call succeeds.
- A bench action that simulates a fault is only as good as its delivery.
  When a scene with a fault gives the same picture as one without, find out
  whether the fault happened.
