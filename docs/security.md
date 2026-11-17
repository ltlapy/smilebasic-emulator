# Security

SmileBASIC programs and project files reach the emulator from other
people, so the emulator treats both as untrusted. A program should at
worst fail with an error or stop itself. It should not reach files outside
its project folder, take the machine down, or stop the user getting back
to the editor.

SmileBASIC 4 relies on a closed console for much of this. The emulator
runs on an open system and has to do it itself.

## What is in place

- **No way out to the system.** The emulator has no network code and
  starts no other programs. The one exception is the menu command that
  opens the projects folder, which asks the system to show that folder.
- **`unsafe` code.** Most of it is in `sb-sdl`, around SDL and GL calls.
  `crates/sb-core/src/vm.rs` has four uses with `SAFETY` comments, kept on
  purpose for speed, each a `set_len` over values it has just checked or
  made numbers: on the VM stack in `push_num` and `drop_top`, on the stack
  after a DEF call's arguments are moved out in `take_args`, and on the stack
  or the DEF variables when a call ends in `cut`. A program cannot reach them
  other than through the VM stack and DEF calls.
  A default build has no other `unsafe` in `sb-core`. The `unchecked-vars`
  feature of `sb-core`, off by default, adds three uses in
  `crates/sb-core/src/vars.rs`: the fused code then reads and writes the
  places of variables and constants without a bounds check on each, for
  about a tenth of the interpreter's instructions on arithmetic. Its
  invariant is that every place an entry names is within its table.
  `fuse::check` bounds every global place by the program's globals and
  constants on every program the VM takes, before the VM shares it, and
  works out what each entry needs of the running DEF's variables, which
  `run_fused` compares with the DEF's before each entry. A program with a
  place out of range runs without its fused code, and an entry that needs
  more of the DEF's variables than there are runs one instruction at a
  time. Each read and write also has a `debug_assert`, and tests feed the
  VM entries moved from another DEF or program. With the feature on,
  Miri found nothing in those tests and in five of the random programs
  of `tests/fuse_random.rs`, under its default Stacked Borrows and with
  `-Zmiri-tree-borrows`, nor in 12 more tests of `tests/fused.rs` under
  Stacked Borrows. A bug there would be memory unsafety that an untrusted
  program could reach, so leave the feature off in any build you hand to
  others or use to run programs you did not write. Default builds run `fuse::check` too and keep the check
  on each read and write, so a place out of range that gets past
  `fuse::check` panics, which stops the program with `Internal Error`.
  `crates/sb-runtime/tests/scene.rs` has a counting allocator, used only by
  that test.
- **Native code.** The `jit` feature of `sb-core`, off by default and a
  prototype, compiles loops of the fused code to machine code with
  Cranelift (`crates/sb-jit`) and runs it on the variables' tables. Its
  `unsafe` is in three parts: `sb-jit`'s executable memory
  (`exec.rs`: `mmap`, `mprotect`, `munmap`, and the aarch64 cache
  maintenance), the call into generated code (`Code::run`), and the one
  call of it in `crates/sb-core/src/jit.rs`, which checks the tables'
  lengths against what the code names first. Generated code is outside
  Rust's checks and outside Miri. Its addresses are a table's start plus a
  constant offset, bounded by construction; the argument, the invariants
  and the tests are in [jit-design.md](jit-design.md#memory-safety). A bug
  in it, or in Cranelift, could be memory unsafety that an untrusted
  program can reach: as with `unchecked-vars`, leave the feature off in
  any build you hand to others. A default build contains none of it.
- **File names.** Every name goes through the checks in
  `crates/sb-runtime/src/storage.rs`: a limited set of characters, 32 at
  most, no leading dot. Symbolic links are not followed.
- **File sizes.** Files over 256 MiB are not read or saved. Compressed
  bodies stop at the same size when unpacked, and array files are limited
  by the product of their sizes.
- **Memory.** `sb_core::memory` keeps a program's strings and arrays
  within SmileBASIC 4's own total and raises `Out of memory` beyond it.
  `INSPECT` makes its lines a page at a time, as they are shown.
- **Waveforms.** `WAVSET` and `WAVSETA` waveforms are outside that total,
  as in SmileBASIC 4, whose limit for them is not known, so they have
  none. A sample takes 2 bytes, and a waveform is at most an array long.
  Each instrument holds one, and a `BEEP` note holds the one it started
  with, so an interpreter holds at most 96 (32 instruments, 16 `BEEP`
  handles, 48 notes falling away): about 6.4 GB from arrays as large as
  memory allows, twice that with a subprogram.
- **Time.** A program runs a limited number of instructions per frame, so
  a loop cannot hold up the window. Calls and `GOSUB`s have depth limits.
- **Failures.** A panic while running a frame stops the program with
  `Internal Error` and keeps the slots and the editor's text. The sound
  thread catches its own panics.
- **GPU rendering.** The GPU path is on by default (Auto). These limits
  apply to what reaches the C driver:
  - At start-up, the path and the CPU draw one small scene. If they differ
    too much, the path stays off and the CPU draws.
  - Texture sizes are fixed or bounded: 2048 square for pages and the
    glyph atlas (`GRP_SIZE`), 1024 square for inverted cells
    (`INVERTED_SIZE`), and the frame's size for targets, which `XSCREEN`
    limits to 1280x720.
  - On SDL's GLES2 driver, the path asks GL whether a texture fits before
    creating it, because SDL 3.4.18 aborts on `GL_OUT_OF_MEMORY` there.
  - A refused texture or blend leaves that frame to the CPU. After three
    refused frames in a row (`MOST_REFUSED`), the emulator stays on the CPU.
  - Before each `SDL_RenderGeometry`, the path checks every vertex's place and
    texture coordinates. If one is NaN, infinite or at least 2^24 from 0
    (`MOST_REACH`), the driver does not get the batch and the CPU draws that
    frame.
  - The path hands the driver at most 30000 vertices at a time
    (`MOST_VERTICES`). A raster filter that needs over 20000 quads
    (`MOST_RASTER_QUADS`) goes to the CPU, and a 4x4 layer's grid has at
    most 256 cells along a side (`MOST_CELLS`).
- **Stopping a program from a gamepad.** Holding the + button for a second
  stops a program, and `ON BREAK GOTO` does not block it. `BUTTON` has no
  bit for it, so a program cannot read it.

[differences.md](differences.md) describes what programs see of the file
rules, under "Files from elsewhere".

## Rules for new code

- **Reading a file format.** Check every size field against what is left
  of the file before allocating. Put a limit on anything that unpacks. Bad
  input gives an error and never a panic.
- **Touching files.** Use the functions in `storage.rs`, and do not build
  paths from a program's strings anywhere else.
- **Commands that make data.** Ask `sb_core::memory` before allocating:
  `request` for something new, `grow` for something that gets bigger. Do
  it before changing anything, because the VM may run the instruction
  again.
- **Commands that do a lot of work.** One call runs to its end, whatever
  the frame's budget. Limit the work by what the program can ask for, or
  charge for it.
- **Reading a value for the debugger.** Watch expressions and viewers
  must not change the program's state.

## Conditions still open

Each of these changes what a bug can cost, so it needs its conditions met
when it is built or shipped.

- **Handheld port.** `tools/build_handheld.py` builds the emulator for
  aarch64 and armhf, and [building-handheld.md](building-handheld.md) checks
  it under qemu. The docs record no run on a device. These stay open:
  - Handheld firmware often runs ports as root, which would turn a path bug
    into writing anywhere on the system. The emulator does not drop
    privileges. Drop them if the firmware allows it, and treat the path
    checks as critical.
  - Nothing keeps host memory within the device's total: SmileBASIC 4's
    memory figure does not limit what the emulator itself uses.
  - There is no gamepad-only mode yet. It has to keep the + button's stop.
- **SmileBASIC 3 and Petit Computer profiles.** Each new file format is a
  new parser, under the rules above. Importing QR codes adds image and QR
  decoding to what a stranger's file can reach.
- **GPU rendering.** Coordinates, sizes and buffer contents reach a C
  driver, and embedded drivers cope badly with odd input. No limit covers a
  frame's total vertices.
- **Downloads from a server.** This would be the largest exposure of all.
  A project that arrives from the internet must not run without the user
  choosing to run it. Transfers need HTTPS and must keep to the same file
  limits.
- **Automatic updates.** These would be a way to run code on the user's
  machine from outside. Updates need HTTPS and a signature check against
  a key the binary holds.
