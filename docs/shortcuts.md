# Keyboard shortcuts

The emulator shows this list too, in Help > Keyboard shortcuts.

## DIRECT mode

| Key | Action |
|---|---|
| Up, Down | Command history |
| Ctrl+F1 to Ctrl+F5 | Type the `KEY` strings |
| Ctrl+Shift+C | Copy; never stops the program |
| Ctrl+Shift+V | Paste, also as keys to programs |

## Suggestions

| Key | Action |
|---|---|
| Up, Down | Choose one, in the editor |
| Enter | Take the chosen one (File > Settings) |
| Tab | Take the chosen one, or the first |
| Esc | Close the list |
| Ctrl+Space | Hide or show suggestions and hints |

## Editor

| Key | Action |
|---|---|
| Ctrl+1 to Ctrl+6 | Open PRG0 to PRG5 |
| Ctrl+0 | Back to DIRECT mode |
| Ctrl+S | Save the slot |
| Ctrl+L | Load a file into the slot |
| Ctrl+Enter | New line below |
| Ctrl+K, Ctrl+Shift+K | Comment, uncomment |
| Ctrl+W | Toggle line wrap |
| F2, Ctrl+B | Character size |
| Ctrl+/ | Split the editor |
| Ctrl+Tab | Other side of the split |

## Find

| Key | Action |
|---|---|
| Ctrl+G | Go to a line |
| F3, Ctrl+F | Find; Shift+F3 for the previous match |
| Ctrl+H | Replace; Ctrl+Enter replaces all |
| Ctrl+J | Labels and DEFs |

## Running and debugging

| Key | Action |
|---|---|
| F5, controller + | Run PRG0, or stop; hold 1 s to force a stop |
| Pause/Break | Stop |
| Ctrl+C | Stop the program |
| F8 | Menu bar and pause |
| Shift+F7 | Toggle a breakpoint |
| Ctrl+F7 | Debugger panel |
| F6 | Step |
| F7 | Step over |
| Shift+F6 | Step out |
| Ctrl+F6 | Run to the cursor |
| Ctrl+Shift+F6 | Continue from the cursor |
| Shift+F5 | Continue |
| Ctrl+Shift+F5 | Restart |

## Windows and tools

| Key | Action |
|---|---|
| F1 | Help for the command at the cursor |
| F2 | Help's text size, in the help |
| F4 | Switch between DIRECT mode and the editor |
| F9, F10, F11 | Start `TOOL1` to `TOOL3` |
| F12 | Projects |
| F5, in the projects | Run the selected project's MAIN.PRG, or the selected program from the file list |
| Ctrl+Shift+F11 | Fullscreen, or back to a window |

View > Fullscreen does the same as Ctrl+Shift+F11. Fullscreen covers the
display without changing its resolution, and the screen is scaled as View >
Screen filter says. With the editor in its own window, only the main window
goes fullscreen. The emulator starts fullscreen if it was when it quit
(`fullscreen=true` in `settings.txt`). Ctrl+Shift+F11 and Ctrl+Shift+F12 type
nothing and start no tool; as with every key, a program can still see them
held with `KEYBOARD`.

## Buttons

The keyboard presses controller 1's buttons, unless you turn off Run > *Use the
keyboard as a controller*. A key the program reads with `KEYBOARD` or `INKEY$`
presses no button until a second after the program stops reading it. The mouse's
left button touches the screen.

| Key | Button |
|---|---|
| Cursor keys | D-pad |
| Z, X, A, S | A, B, Y, X |
| Q, W | L, R |
| 1, 2 | ZL, ZR |

## Gamepad in the projects

The menu needs the mouse or keys.

| Key | Action |
|---|---|
| − (Select), alone | Open the projects, in DIRECT mode |
| A | Run MAIN.PRG or the selected file |
| X | Switch between projects and files |
| L, R | Move a page |
| Y | Expand the info |
| B | Go back |
| Left stick | D-pad |

## Gamepad as the mouse

The right stick moves the mouse pointer while no program runs, and in a
program that shows the mouse cursor, as on SmileBASIC 4. File > Settings >
Input > *Right stick moves the mouse* turns it off. Holding − (Select) makes
it work in any program; the program then does not see the right stick or R3.

| Key | Action |
|---|---|
| Right stick | Move the mouse pointer |
| R3 (press the right stick) | Left mouse button, which touches the screen |
| − (Select) and right stick | Move the pointer in any program |
| − (Select) and R3 | Left mouse button in any program |

## Simple UI

The Simple UI is for a gamepad without a keyboard, as on a Linux handheld:
fullscreen, no menu bar or editor, and a list of the projects instead of
DIRECT mode. Start it with `smilebasic --simple-ui`, or turn it on and off
with Ctrl+Shift+F12, which keeps the choice. Turning it off goes back to a
window, or to fullscreen if the desktop was. Programs never see − (Select).

The first time the Simple UI starts, a welcome screen asks you to press A.
The button you press, the right or the bottom one, works as A from then on,
and the one across from it as B. Enter continues without changing this.

The pause menu pauses the program. It has Resume, Restart, Return to Game
list and Quit Emulator; over the list it has Close and Quit Emulator. Its
top right corner shows the emulator's version, as Help > About does. Under
Settings it has:

- Screen filter: how the screen is scaled, described in
  [differences.md](differences.md#screen-scaling).
- UI size: how large the list, menus and dialogs are drawn. Normal draws
  them at the largest whole scale that leaves 480 lines or more (1x on a
  640x480 display), Medium at 1.5 times that (24-pixel text, a 427x320 UI on
  640x480), and Large at twice that (320x240 on 640x480). Where Normal is
  2x, as on 1920x1080, Medium is 3x. The program's screen keeps its size.
  The notices in the bottom right corner, the keys for the buttons and a
  program slower than real time, cover less of it: they stay at Normal's
  size at Medium, and take Medium's at Large.
  In `settings.txt` this is `simple_ui_size=normal`, `medium` or `large`.
- Swap A/B and X/Y: A and B trade places, as do X and Y, for programs too.
  The desktop has it in File > Settings > Input.
- Frame skip: On skips frames when the device is slower than a program
  needs, instead of slowing the program down. Music does not break up and
  play feels faster, but programs that count frames may misbehave. Off by
  default. The desktop has it in File > Settings > General as "Skip frames
  instead of slowing down"; [differences.md](differences.md#speed) compares
  both with SmileBASIC 4. When a program runs slower than real time for a
  while, a notice points here.
- Wait for vsync: On, the default, has the window's presents wait for the
  display's refresh. Off, each frame waits for its own time, for a game
  where the timing of input matters on a device that leaves drawings out;
  the picture may tear, and a slow device may run the program slower. The
  desktop has it in View.

A change shows at once and is kept.

| Key | Action |
|---|---|
| Ctrl+Shift+F12 | Turn the Simple UI on or off |
| − (Select) and A, F8 | Open or close the pause menu |
| − (Select) and + | Quit the emulator |
| + | As on the desktop; it does nothing in the list |
| A, Enter | Run the project's MAIN.PRG or the chosen program; choose in the menu |
| X, Tab | The project's programs |
| B, Esc, Backspace | Back to the projects; close the menu |
| L, R, Page Up, Page Down | Move a page |
| D-pad, left stick, cursor keys | Move |
| Left, Right | Change a setting in the menu |
