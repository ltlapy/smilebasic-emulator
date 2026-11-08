# Building for Windows

You can build the Windows programs on a Linux PC with MinGW, or on Windows itself.

## Build on Linux

On Fedora:

```sh
sudo dnf install mingw64-gcc cmake make
rustup target add x86_64-pc-windows-gnu
```

On Debian or Ubuntu, install `gcc-mingw-w64-x86-64 cmake make` with apt, then run the
`rustup` line above.

Then build:

```sh
cargo build --profile dist --target x86_64-pc-windows-gnu
```

Cargo finds the MinGW linker by itself, and CMake builds SDL with `x86_64-w64-mingw32-gcc`.
Leave out `--profile dist` for a faster build that runs a little slower.

## Build on Windows

This path is untested.

1. Install Rust with [rustup](https://rustup.rs/), keeping its default MSVC toolchain.
2. Install the Visual Studio Build Tools with the "Desktop development with C++" workload.
3. Install [CMake](https://cmake.org/download/) and let it add itself to `PATH`.
4. In the repository, run `cargo build --profile dist`.

## What you get

`target/x86_64-pc-windows-gnu/dist/` holds `smilebasic.exe`, `smilebasic-check.exe` and
`smilebasic-import.exe`. A build on Windows puts them in `target\dist\`.

Each program links SDL statically and imports only DLLs that come with Windows, such as
`KERNEL32.dll`, `USER32.dll` and `msvcrt.dll`. Copy the `.exe` you need and nothing else. To
list what a program imports, run:

```sh
x86_64-w64-mingw32-objdump -p target/x86_64-pc-windows-gnu/dist/smilebasic.exe | grep "DLL Name"
```

The emulator keeps its files in `%APPDATA%\smilebasic-emulator`, where Linux uses
`$XDG_DATA_HOME/smilebasic-emulator`.

## No console output

Release and `dist` builds of `smilebasic.exe` use the Windows GUI subsystem, so double-clicking
the program opens no console window. It shows an error that stops it from starting in a
message box. Its other messages are lost even when you start it from a console: which
renderer it uses, the fallback font notice, and the results of `SB_UI_BENCH` and `SB_GPU`
from [benchmarking.md](benchmarking.md). Build without a profile (`cargo build`) to keep the
console.

`smilebasic-check.exe` and `smilebasic-import.exe` are console programs in every build.
