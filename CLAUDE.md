# SmileBASIC emulator

SmileBASIC 4 emulator in Rust + SDL3, with planned compatibility profiles for
SmileBASIC 3 and Petit Computer (v1/v2). Targets Windows, macOS and Linux.

Follow [CONTRIBUTING.md](CONTRIBUTING.md) for code and commit rules.

- Build: `cargo build`. SDL3 is built from source; Linux needs the X11/Wayland/audio
  development headers listed in SDL's `docs/README-linux.md`.
- Check before committing: `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`,
  `cargo test --workspace`.
- Before changing code, read [docs/development/working-notes.md](docs/development/working-notes.md): what each
  crate is for, what to watch in it, and how to check a change.
- This repository will be published. Write everything in it on that assumption, carefully:
  no paths from the local machine, nothing you would not show a stranger.
