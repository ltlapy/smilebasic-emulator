# Keyboard shortcuts

The emulator shows this list too, in Help > Keyboard shortcuts.

## Typing and direct mode

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

## Editor

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

## Find and navigate

| Key | Action |
|---|---|
| Ctrl+Home, Ctrl+End | Jump to the start or end of the program |
| Ctrl+G | Go to a line: type its number and press Enter |
| F3, Ctrl+F | Find as you type; Down, F3 or Enter for the next match, Up or Shift+F3 for the one before, Tab to replace |
| Ctrl+H | Replace: Enter replaces the selected match, Ctrl+Enter replaces them all, Tab goes back to finding |
| Ctrl+J | List the labels and DEFs; type to narrow the list, Enter to go there, Esc to go back |
| Esc | Close the help or the list beside the editor |

## Running and debugging

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

## Windows and tools

| Key | Action |
|---|---|
| F1 | Show SmileBASIC 4's help for the command at the cursor, in the editor or on the line typed in direct mode (needs the imported help) |
| F2 | While the help has the focus, change its text size |
| F4 | Switch between direct mode and the editor; after an error, open the editor at the failing line. With View > Editor in a separate window, the editor opens beside the screen and programs keep running; F4 on the screen brings it to the front |
| F9, F10, F11 | Start the tools set for `TOOL1` to `TOOL3` (File > Settings > Tools) as subprograms, also while a program runs: SmileBASIC 4's SMILETOOL and GAHAKU at first, from the `#TOOL` project the importer copies |
| F12 | Open the projects: run (or double-click) or switch, preview and view files, load or run a program, copy files (or drag them onto a project); Space expands the info to edit a project's title, description and icon; the menu key or right-click for a menu |
| Ctrl+/, Ctrl+Tab | Split the editor side by side, then one above the other, then join it again; move between the sides |
