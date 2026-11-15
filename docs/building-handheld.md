# Building for a Linux handheld

This builds for devices such as an RK3326 on an AmberELEC-like system: Cortex-A35, GLES2
over KMSDRM, no X11 or Wayland. You build on an x86-64 Linux PC.

The cores are 64-bit, but some systems, such as older ArkOS, run a 32-bit userland on
them. Each userland needs its own build, named as PortMaster names it:

| `--arch` | Userland | Rust target | Runs on |
|---|---|---|---|
| `aarch64`, the default | 64-bit | `aarch64-unknown-linux-gnu` | ARMv8-A |
| `armhf` | 32-bit | `armv7-unknown-linux-gnueabihf` | ARMv7-A with VFPv3 and NEON |

## Install

On Fedora:

```sh
sudo dnf install zig cmake make pkgconf-pkg-config binutils file \
    libdrm-devel mesa-libgbm-devel alsa-lib-devel pulseaudio-libs-devel systemd-devel
cargo install --locked cargo-zigbuild
rustup target add aarch64-unknown-linux-gnu armv7-unknown-linux-gnueabihf
```

On Debian or Ubuntu, install `cmake make pkg-config binutils file libdrm-dev libgbm-dev
libasound2-dev libpulse-dev libudev-dev` with apt, then run the last two lines above. Ubuntu
26.04 and later package `zig`. On Debian and older Ubuntu, unpack a release from
[ziglang.org](https://ziglang.org/download/) and add it to `PATH`.

Zig 0.15.2 and 0.16.0 work. The tool warns about other versions. You need only the Rust
targets you build for.

To build without installing anything as root, see
[Build without root](#build-without-root).

Why not Fedora's `gcc-aarch64-linux-gnu`? It links against the glibc 2.43 in its sysroot, and
an older handheld system refuses those binaries. Zig carries stubs of every glibc, so you
can target 2.17 from any PC.

## Build

```sh
python3 tools/build_handheld.py
python3 tools/build_handheld.py --arch armhf
```

If something is missing, the tool lists the commands to install it and stops. It downloads
nothing.

## What you get

`target/TRIPLE/dist/` holds `smilebasic` and `smilebasic-check`, where `TRIPLE` is the Rust
target from the table. The tool prints their sizes, the newest glibc they need, the
libraries they link and SDL's backends.

Both programs link only libc, libm, libdl and libpthread, with SDL inside. SDL opens libdrm,
gbm, ALSA, PulseAudio and udev when it starts, so a device without one of them lacks that
backend and nothing more. The build needs only those libraries' headers from your `-devel`
packages, and no x86-64 library reaches the linker. EGL and GLES2 headers come with SDL.

Both builds need glibc 2.17, the oldest Rust supports and aarch64's first. `--glibc` raises
it. Delete `target/TRIPLE` after changing it.

The armhf build takes Rust's `armv7-unknown-linux-gnueabihf`, which is ARMv7-A, Thumb-2 and
VFPv3 with 32 double registers, and adds NEON with `-C target-feature=+neon`. A Cortex-A35
has all of it in 32-bit mode, as does every other ARMv8-A Cortex-A core. The standard
library and SDL are built without NEON.

NEON is in because it pays. Counted under `qemu-arm`, with each TPERF scene run alone for 4
to 25 frames and the start-up taken out, it cuts instructions by this much. Every checksum
stays the same, and all of TPERF's 10500 frames give the x86-64 checksum with NEON and
without.

| Start-up | SPROT | GALPHA | GTRI | LCOMP | LBLUR | GAME |
|---|---|---|---|---|---|---|
| 6% | 3% | 6% | 5% | 15% | 12% | 8% |

SDL asks for a 64-bit `time_t` on 32-bit glibc, which needs glibc 2.34. Below that, the
tool builds SDL with a 32-bit one, as Rust's standard library uses.

## Check it on the PC

Install qemu and a libc for it. On Fedora, for aarch64:

```sh
sudo dnf install qemu-user "sysroot-aarch64-fc$(rpm -E %fedora)-glibc"
```

Fedora packages no armhf libc: use [`--rootless`](#build-without-root), or set
`QEMU_LD_PREFIX` to the root of one. On Debian or Ubuntu, install `qemu-user` and
`libc6-arm64-cross` or `libc6-armhf-cross`. `QEMU_LD_PREFIX` picks another libc for either.

```sh
python3 tools/build_handheld.py --run smilebasic-check --bench --fixed 1200 DIR PROGRAM
python3 tools/build_handheld.py --test --no-fail-fast
```

Put `--arch armhf` first for the armhf build. Both lines run under `qemu-aarch64` with a
Cortex-A35's instruction set, or `qemu-arm` with a Cortex-A8's: the armhf baseline, without
the integer divide and VFPv4 of later cores. An x86-64 build of the same line must print
the same checksum. A difference is a real finding, most likely in floating point. Times
under qemu say nothing about the device.

`--test` skips `sb-sdl`. `a_busy_subprogram_keeps_to_the_frames_time` times a 2 ms limit and
can fail on a busy machine, natively too. Run it alone to tell.

`smilebasic` runs too, with `SDL_VIDEO_DRIVER=dummy SDL_AUDIO_DRIVER=dummy` and no window.
Only the device can test KMSDRM and GLES2.

## On the device

Find out what it has, over SSH:

```sh
ls /lib/ld-linux-*                              # aarch64.so.1: aarch64; armhf.so.3: armhf. uname -m may say aarch64 for both
ldd --version | head -1
ls /usr/lib/lib{EGL,GLESv2,gbm,drm,asound}*
```

Quit the front end, which holds the display. On AmberELEC and its forks, run
`systemctl stop emustation`. Copy `smilebasic` and `smilebasic-check` over, and put a project
in `projects/NAME` under `$XDG_DATA_HOME/smilebasic-emulator`.

```sh
export XDG_DATA_HOME=/storage/sb

# Compares GLES2 drawing with software drawing. Exits 1 if a scene differs too much.
SB_GPU_GOLDEN=1 SDL_VIDEO_DRIVER=kmsdrm SDL_RENDER_DRIVER=opengles2 ./smilebasic

# Frame times under GLES2, and which renderer drew each frame.
SB_GPU=sync SB_UI_BENCH=frames=600,run=NAME/PROGRAM \
    SDL_VIDEO_DRIVER=kmsdrm SDL_RENDER_DRIVER=opengles2 ./smilebasic

# Software drawing alone. The checksum must match the PC's.
perf stat -e instructions:u,cycles:u -- \
    ./smilebasic-check --bench --fixed 400 "$XDG_DATA_HOME/smilebasic-emulator/projects/NAME" PROGRAM
```

Without `perf`, run the last line from `./smilebasic-check` on for times.

SDL looks for `libEGL.so.1` and `libGLESv2.so.2`. If your driver uses other names, as a Mali
blob may, set `SDL_VIDEO_EGL_DRIVER` and `SDL_VIDEO_GL_DRIVER` to its paths.
[benchmarking.md](benchmarking.md) explains the output.

To play, start it in handheld mode, which needs only the gamepad:

```sh
SDL_VIDEO_DRIVER=kmsdrm SDL_RENDER_DRIVER=opengles2 ./smilebasic --handheld
```

It opens a list of the projects, scales its text by whole factors of 640x480, and fits the
program's screen to the display. [shortcuts.md](shortcuts.md#handheld-mode) lists the buttons.

## Build without root

`--rootless` builds without installing anything as root. Add it before `--run` or `--test`:

```sh
python3 tools/build_handheld.py --rootless
python3 tools/build_handheld.py --rootless --run smilebasic-check --bench --fixed 1200 DIR PROGRAM
python3 tools/build_handheld.py --rootless --test --no-fail-fast
```

You need `python3`, `rustup`, `cmake`, `make`, `pkg-config`, `readelf` and `file`, plus
`qemu-aarch64` or `qemu-arm` for `--run` and `--test`. The first run fetches zig 0.15.2,
cargo-zigbuild 0.24.1, and Debian 10's arm64 or armhf headers and libc into `local/cross`, or
`SB_CROSS_HOME` if you set it. The tool pins each download's checksum, and adds the Rust
target with rustup.

SDL's CMake cache remembers the headers' folder. If you delete or move `local/cross`, delete
`target/TRIPLE` too.
