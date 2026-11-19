# Notes for whoever works on this code next

For an AI agent or a person about to change this repository: what each
crate is for, what to watch in it, and how to check a change. It adds to
[CONTRIBUTING.md](../../CONTRIBUTING.md), which has the rules for code,
tests and commits; read that first.

The notes come from work on the code, mostly a long refactoring of the
window's loop. The parts about `sb-sdl` are first-hand. The parts about
the other crates gather what their own documents say; follow the links
before a change there.

## The crates

| Crate | What it is | Read before changing it |
|---|---|---|
| `sb-core` | The language: lexer, parser, compiler, bytecode VM. It reaches screens, sound and files only through `host::Host` | [security.md](security.md), [jit-design.md](../performance/jit-design.md) |
| `sb-jit` | Native code for the VM's hot loops, behind the `jit` feature. It knows nothing of SmileBASIC | [jit-design.md](../performance/jit-design.md) |
| `sb-runtime` | The machine without a window: commands, screens, sprites, sound, files, the editor, the CPU renderer | [renderer.md](../rendering/renderer.md), [sb4-pitfalls.md](../smilebasic4/sb4-pitfalls.md), [differences.md](../using/differences.md) |
| `sb-profiles` | The other SmileBASIC versions, as profiles of this one. Small so far | |
| `sb-import` | `smilebasic-import`: copies the user's own SmileBASIC 4 resources into the data directory | [importing.md](../using/importing.md), [file-formats.md](../smilebasic4/file-formats.md) |
| `sb-check` | `smilebasic-check`: runs programs with no window, to compare with SmileBASIC 4 and to measure | [hardware-tests.md](../smilebasic4/hardware-tests.md), [benchmarking.md](../performance/benchmarking.md) |
| `sb-sdl` | `smilebasic`: the window, input, sound device, the GPU path, the emulator's own screens | [frontend-guide.md](frontend-guide.md), [frame-thread.md](frame-thread.md) |

A crate depends only on what CONTRIBUTING.md's table allows it. SDL and
anything of the operating system stay in `sb-sdl`.

## What holds everywhere

**SmileBASIC 4 is the reference, and each claim about it has one home.**

- The manual says it: follow the manual, name the command in the doc
  comment.
- A check on SmileBASIC 4 confirmed it: it is in
  [sb4-pitfalls.md](../smilebasic4/sb4-pitfalls.md). Put new confirmed facts there, and
  keep them out of code comments, which then only say why the code does
  what it does.
- Nobody has checked: mark the code `// UNVERIFIED: <what is assumed>`.
  Take the mark away when the check is done.
- The emulator departs from SmileBASIC 4 on purpose: say so in
  [differences.md](../using/differences.md), and leave the faithful code beside the
  departure, commented out, so that the difference can be read and undone.

**A program is untrusted.** It may fail with a SmileBASIC error. It must
not panic the emulator, reach a file outside its project, or exhaust the
computer. [security.md](security.md) has the rules for code that reads a
file format, touches files, allocates for a program, or does a lot of work
in one call.

**Speed is measured.** The targets include slow handhelds, so a change to
a hot path needs a number. Compare builds by instruction counts on a run
that does the same work (`smilebasic-check --bench --fixed`), and compare
the checksums first: different checksums mean different work, and then the
counts say nothing. [benchmarking.md](../performance/benchmarking.md) has the method.

**The repository will be published.** No path, address or name from
anyone's machine. Nothing of SmileBoom's: no fonts, sprites, sounds or text
taken from the console versions, and no data derived from them. Nothing
copied from other people's programs into code, tests or docs.

**Tests and tools open no window.** Use `SDL_VIDEO_DRIVER=offscreen` and
`SDL_AUDIO_DRIVER=dummy`, or `smilebasic-check`, which has neither.

**Words.** Docs and UI text use SmileBASIC 4's own terms (DIRECT mode, not
"the prompt"), plain sentences, American spelling.

## Changing code

- **One concern to a commit.** A move and a change of logic in one commit
  cannot be checked: the comparison that proves the move hides the change,
  or fails because of it. When a refactoring turns up a bug, report it and
  fix it in a commit of its own.
- **Rename first, then move.** To take code out of a large function, first
  group the state it uses into a struct, by a rename alone. The compiler
  finds each use. Then move the code as it was.
- **Read both before merging two paths.** Code that looks duplicated often
  differs in one guard. Compare the conditions and the bodies line by line,
  and keep the difference as an argument when there is one.
- **Do not discard a result you have not seen succeed.** `let _ = call()`
  hid for a long time that a call failed every time (see `sb-sdl` below).
- **Report what grows.** A function with dozens of locals, a file of
  several thousand lines, the same list written in three places: say so
  when you see it, even if it is not your task.

## Checking a change

Run what CONTRIBUTING.md asks: `cargo fmt --check`, clippy with
`-D warnings`, `cargo test --workspace`, and for the VM the same with
`--features unchecked-vars` and `--features jit`. Write the test run to a
file and search it for `FAILED`. A test that fails once in fifty runs is
then caught with its name.

The tests do not cover everything. The other tools:

| To check | Use |
|---|---|
| That a program behaves as on SmileBASIC 4 | `smilebasic-check PROJECT PROGRAM`, which compares the `RES` files the program saves with those SmileBASIC 4 saved |
| That two builds do the same work, and what it costs | `smilebasic-check --bench --fixed FRAMES PROJECT PROGRAM`: equal checksums, then instruction counts |
| That the GPU path draws what the CPU draws | `SB_GPU=check`, see [renderer.md](../rendering/renderer.md) |
| What the window shows, and what the loop's timing does | `SB_UI_BENCH`, see [benchmarking.md](../performance/benchmarking.md) and [frontend-guide.md](frontend-guide.md) |

"It ran" is not "it matched". Compare the output with what it should be,
and read one run's output yourself before trusting a batch of them.

Comparing a build before a change with one after it goes wrong in ways
that leave no trace. Each of these happened:

- **The baseline was the new build.** Switching to another commit carries
  uncommitted changes along. Commit first, build the baseline from a clean
  tree, and check that the two binaries differ.
- **The test did not test.** A program stored under the wrong name did
  not run, and every scene showed the same idle screen. A program meant to
  keep running stopped with an error at its first line. A simulated fault
  was never delivered. When a case with a fault looks the same as one
  without, find out whether the fault happened.
- **The output named the build.** The version text holds the commit, so
  two builds differ wherever it shows.
- **One run was taken as the answer.** Two runs of one build can differ:
  a program that runs as much as fits in a frame gets further on a quiet
  computer, and a test that sleeps and assumes how long the sleep took
  fails on a busy one. Run a differing case again by itself, and compare
  the baseline with itself once to learn the noise.

A desktop cannot stand for a slow handheld. A change to the VM's hot
paths, the loop's timing or the GPU path needs a run on the slowest
target before it counts as settled.

In the commit or the hand-over, say what you checked and what you did not.
"Compared 37 scenes; did not run the jit build" tells the reader more than
"verified".

## By crate

### sb-core

- A user's program must never panic the VM. Every error is a SmileBASIC
  error with its line.
- The VM runs "fused" entries (`fuse.rs`) on its hot paths, and two
  features change how: `unchecked-vars` leaves out bounds checks on
  variables, `jit` runs hot loops as native code. `sb-runtime`, `sb-check`
  and `sb-sdl` forward both. A change to `vm.rs` or `fuse.rs` passes
  clippy and the tests in all three builds.
- Code that makes a string or an array asks `sb_core::memory` first
  (`request`, `grow`), before it changes anything: the VM may run the
  instruction again.
- `tests/programs.rs` checks programs by their `PRINT` output;
  `tests/fused.rs` and `tests/fuse_random.rs` check the fused code against
  the plain interpreter.

### sb-jit

- It holds no rule of the language. `sb-core`'s `jit` module says what each
  step does and tests the native code against the interpreter.
- A change to an operator's rules, to `Value`'s layout or to how the fused
  code charges its budget has a counterpart here.
  [jit-design.md](../performance/jit-design.md) lists them under "Costs to the code base".

### sb-runtime

- A command is a `Command` and a name in `COMMANDS` (`runtime.rs`). A new
  one also needs: its place in `Command::unseen` or `Runtime::sight` if it
  never changes the picture (otherwise every call redraws the frame, which
  is correct and slow); its cost in `speed::command_cost`, for the speeds
  that stand for a console; its name in `syntax.rs`'s sorted list, for the editor.
- The screens are drawn again only when something marked them changed
  ([renderer.md](../rendering/renderer.md), "Change tracking"). A command that changes
  what shows without marking it leaves a stale frame; this is the first
  thing to suspect when a picture does not update.
- The CPU renderer here is the reference. The GPU path in `sb-sdl` has to
  draw the same, and [renderer-verification.md](../rendering/renderer-verification.md)
  holds the evidence for each rule.
- Files go through `storage.rs`. Do not build a path from a program's
  string anywhere else.
- `tests/` runs programs on a `Machine` and checks the console and the
  state. Prefer that to a test of one function when a command's behavior
  is the point.

### sb-profiles

- Small so far: profiles for SmileBASIC 3 and Petit Computer are planned,
  as mappings onto the SmileBASIC 4 runtime. Version differences belong
  here, not as conditions spread through `sb-runtime`.

### sb-import

- It reads files the user extracted from their own copy. Nothing it reads,
  and nothing made from it, goes into the repository or a release.
- It parses formats it does not control: check each size against what is
  left of the file, limit what unpacks, return an error and never panic.

### sb-check

- Other tools and scripts read its output and its exit status
  ([hardware-tests.md](../smilebasic4/hardware-tests.md)). Add to them; do not reword
  what is there without changing the readers.
- `--fixed` is what makes two runs comparable. A new source of difference
  between runs (time, random numbers, a hash order that reaches the
  output) has to be fixed there too, or said in
  [benchmarking.md](../performance/benchmarking.md).

### sb-sdl

[frontend-guide.md](frontend-guide.md) has the detail. In short:

- The window's loop is `frontend()` in `main.rs`. Its state lives in a
  handful of structs and its work in modules by concern; new code goes
  there, not into the loop.
- The order in which the emulator's screens get an event, and the order of
  the loop's steps, are behavior. Moving a block changes them.
- A key that a program can read (Esc among them) is never bound to quitting
  or to another action of the emulator.
- The sdl3 crate refuses to push the renderer's reset events. The frontend
  pushes them itself (`push_render_event`); before that fix the wake from a
  suspend asked for a reset that never arrived.
