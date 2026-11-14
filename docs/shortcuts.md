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
| Ctrl+C | Stop, if turned on in File > Settings |
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
| F5, in the projects | Run the selected project's MAIN.PRG |

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
| − (Select) | Open the projects, in DIRECT mode |
| A | Run MAIN.PRG or the selected file |
| X | Switch between projects and files |
| L, R | Move a page |
| Y | Expand the info |
| B | Go back |
| Left stick | D-pad |
