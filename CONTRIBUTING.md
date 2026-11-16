# Contributing

## Code

### Crate boundaries

| Crate | May depend on | Must not touch |
|---|---|---|
| `sb-core` | std, `stacker`; `sb-jit` with the `jit` feature | rendering, audio, input, files |
| `sb-jit` | std, Cranelift | SmileBASIC's rules, files, OS APIs but executable memory |
| `sb-runtime` | `sb-core` | SDL, OS APIs |
| `sb-profiles` | `sb-core`, `sb-runtime` | SDL, OS APIs |
| `sb-import` | `sb-runtime` | SDL |
| `sb-check` | `sb-runtime`, `sb-profiles` | SDL |
| `sb-sdl` | everything | nothing |

SDL and platform code live in `sb-sdl` only. Everything else must build and test
headless on Windows, macOS and Linux.

### Behavior

- SmileBASIC 4 is the reference. Other versions are profiles mapped onto it.
- Base each command on the SmileBASIC 4 reference manual
  (<https://sup4.smilebasic.com/doku.php>; while the site is offline, use the
  Internet Archive's copies). Name the command in the doc comment.
- If behavior comes from guessing rather than the manual or real hardware, mark it
  `// UNVERIFIED: <what is assumed>` so it can be checked later.
- A user program must never panic the emulator. Bad input produces a SmileBASIC
  error (`Syntax error`, `Type mismatch`, ...) with the line number.

### Style

- `cargo fmt` and `cargo clippy --all-targets -- -D warnings` pass.
- Code, comments, identifiers and docs are in American English.
- Comments explain why. Skip comments that repeat the code.
- `unsafe` needs a `// SAFETY:` comment. Outside `sb-sdl` and test code, avoid it
  unless a benchmark shows the need, and put the result in that comment.
- Add a dependency only when writing it ourselves is clearly worse. Say why in the
  commit body.

### Tests

- Each language feature and command gets unit tests in its crate.
- Programs that exercise whole features go in `crates/*/tests`, as source text in
  Rust tests that run headless. `sb-core` checks their `PRINT` output; `sb-runtime`
  runs them on a `Machine` and checks the console and program state.
- `cargo test --workspace` passes before every commit.
- A change to `sb-core`'s VM or fused code also passes clippy and the tests with
  `--features unchecked-vars`, the build without bounds checks on variables
  (see [docs/security.md](docs/security.md)), and with `--features jit`, the
  build that runs hot loops as native code (see
  [docs/jit-design.md](docs/jit-design.md)). Its tests check that native
  code ran; `tools/build_handheld.py --jit --test` runs them on aarch64
  under qemu.
- Optional, with a nightly toolchain and Miri installed:
  `cargo +nightly miri test -p sb-core --features unchecked-vars --test fused -- entries_naming`
  checks that build's `unsafe` reads for undefined behavior, in about ten minutes;
  add `MIRIFLAGS=-Zmiri-tree-borrows` for the other borrow model. All of
  `tests/fused.rs` takes hours there. `--test fuse_random` runs 5 of its programs
  under Miri, or `SB_RANDOM_PROGRAMS` of them.

### Assets and text

- Do not commit SmileBoom assets: fonts, sprite sheets, BGM, sound effects, or files
  extracted from the console versions. Users supply their own, or we draw
  replacements.
- Third-party assets need a license that allows redistribution (such as the
  SIL OFL). Keep the license text and a note on the source and version next
  to the files.
- The official reference is CC BY 4.0. Short quotes are fine with attribution;
  don't copy whole pages.
- No paths from a developer's own machine in code, docs or configs.

### License

Unless you state otherwise, contributions are dual-licensed `MIT OR Apache-2.0`,
as the project is, with no additional terms or conditions.

## Commits

```
<scope>: <summary in imperative mood>

<body: why the change was made, and anything a reviewer can't see in the diff>
```

- Scope is one of `core`, `runtime`, `profiles`, `import`, `check`, `sdl`, `tests`,
  `tools`, `build`, `docs`. A crate's scope covers its modules: `raster` and
  `sprites` are `runtime`, `gpu` and `sb-sdl` are `sdl`. Older commits use these
  narrower names.
  Use a comma for two (`core, runtime:`). If you need more, split the commit.
- Summary: lowercase start, no trailing period, 72 characters at most.
- One logical change per commit. Each commit builds and passes tests.
- Write the body when the reason isn't obvious from the summary. Wrap at 72.
- AI-assisted commits end with a `Co-Authored-By:` trailer naming the model.

Examples:

```
core: lex numeric literals with &H and &B prefixes
runtime: keep GRP pages on the CPU to make GSPOIT cheap
build: link SDL3 statically on all platforms
```
