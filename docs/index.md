# Documentation

Each folder holds one subject. A new document goes into the folder of its
subject and gets a line here.

## using: using the emulator

- [shortcuts.md](using/shortcuts.md): keyboard shortcuts
- [differences.md](using/differences.md): where the emulator differs from SmileBASIC 4 on purpose
- [importing.md](using/importing.md): importing resources, and exporting instruments and songs

## building: building it

- [building-windows.md](building/building-windows.md): building for Windows
- [building-handheld.md](building/building-handheld.md): building for an aarch64 or armhf handheld

## smilebasic4: SmileBASIC 4 itself

- [sb4-pitfalls.md](smilebasic4/sb4-pitfalls.md): behavior the manual leaves open, as checked
- [file-formats.md](smilebasic4/file-formats.md): project and file formats
- [hardware-tests.md](smilebasic4/hardware-tests.md): comparing the emulator with SmileBASIC 4

## rendering: how a frame is drawn

- [renderer.md](rendering/renderer.md): how the emulator draws a frame
- [renderer-verification.md](rendering/renderer-verification.md): rendering rules and how they were checked
- [renderer-decisions.md](rendering/renderer-decisions.md): choices made for the GPU renderer

## performance: speed and how to measure it

- [benchmarking.md](performance/benchmarking.md): how to measure performance
- [benchmark-results.md](performance/benchmark-results.md): performance results
- [jit-design.md](performance/jit-design.md): native code for the interpreter's hot loops

## development: working on the project

- [CONTRIBUTING.md](../CONTRIBUTING.md): code and commit rules
- [working-notes.md](development/working-notes.md): what each crate is for, what to watch in it, and how to check a change
- [writing-style.md](development/writing-style.md): how comments, documents and UI text are worded
- [frontend-guide.md](development/frontend-guide.md): where the window's code and state live, and how to check a change to it
- [frame-thread.md](development/frame-thread.md): how frames and sound continue while the main thread is blocked
- [security.md](development/security.md): how the emulator treats untrusted programs and files
