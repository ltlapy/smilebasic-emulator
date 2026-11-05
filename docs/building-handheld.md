# Building for an aarch64 handheld

This builds for devices such as an RK3326 on an AmberELEC-like system: Cortex-A35, GLES2
over KMSDRM, no X11 or Wayland. You build on an x86-64 Linux PC.

## Install

On Fedora:

```sh
sudo dnf install zig cmake make pkgconf-pkg-config binutils file \
    libdrm-devel mesa-libgbm-devel alsa-lib-devel pulseaudio-libs-devel systemd-devel
cargo install --locked cargo-zigbuild
rustup target add aarch64-unknown-linux-gnu
```

On Debian or Ubuntu, install `cmake make pkg-config binutils file libdrm-dev libgbm-dev
libasound2-dev libpulse-dev libudev-dev` with apt, then run the last two lines above. Ubuntu
26.04 and later package `zig`. On Debian and older Ubuntu, unpack a release from
[ziglang.org](https://ziglang.org/download/) and add it to `PATH`.

Zig 0.15.2 and 0.16.0 work. The tool warns about other versions.

Why not Fedora's `gcc-aarch64-linux-gnu`? It links against the glibc 2.43 in its sysroot, and
an older handheld system refuses those binaries. Zig carries stubs of every glibc, so you
can target 2.17, aarch64's first, from any PC.

## Build

```sh
python3 tools/build_aarch64.py
```

If something is missing, the tool lists the commands to install it and stops. It downloads
nothing.

## What you get

`target/aarch64-unknown-linux-gnu/dist/` holds `smilebasic` and `smilebasic-check`. The tool
prints their sizes, the newest glibc they need, the libraries they link and SDL's backends.

Both programs link only libc, libm, libdl and libpthread, with SDL inside. SDL opens libdrm,
gbm, ALSA, PulseAudio and udev when it starts, so a device without one of them lacks that
backend and nothing more. The build needs only those libraries' headers from your `-devel`
packages, and no x86-64 library reaches the linker. EGL and GLES2 headers come with SDL.

`--glibc` raises the oldest glibc to run on. Delete `target/aarch64-unknown-linux-gnu` after
changing it.

## Check it on the PC

Install qemu and an aarch64 libc for it:

```sh
sudo dnf install qemu-user "sysroot-aarch64-fc$(rpm -E %fedora)-glibc"
```

On Debian or Ubuntu, install `qemu-user libc6-arm64-cross`. To use
another libc, set `QEMU_LD_PREFIX` to its root.

```sh
python3 tools/build_aarch64.py --run smilebasic-check --bench --fixed 1200 DIR PROGRAM
python3 tools/build_aarch64.py --test --no-fail-fast
```

Both run under `qemu-aarch64` with a Cortex-A35's instruction set. An x86-64 build of the
same line must print the same checksum. A difference is a real finding, most likely in
floating point. Times under qemu say nothing about the device.

`--test` skips `sb-sdl`. `a_busy_subprogram_keeps_to_the_frames_time` times a 2 ms limit and
can fail on a busy machine, natively too. Run it alone to tell.

`smilebasic` runs too, with `SDL_VIDEO_DRIVER=dummy SDL_AUDIO_DRIVER=dummy` and no window.
Only the device can test KMSDRM and GLES2.

## On the device

Find out what it has, over SSH:

```sh
uname -m                                        # aarch64; a 32-bit userland needs another build
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

## Build without root

`--rootless` builds without installing anything as root. Add it before any other option:

```sh
python3 tools/build_aarch64.py --rootless
python3 tools/build_aarch64.py --rootless --run smilebasic-check --bench --fixed 1200 DIR PROGRAM
python3 tools/build_aarch64.py --rootless --test --no-fail-fast
```

You need `python3`, `rustup`, `cmake`, `make`, `pkg-config`, `readelf` and `file`, plus
`qemu-aarch64` for `--run` and `--test`. The first run fetches zig 0.15.2, cargo-zigbuild
0.24.1, and Debian 10's arm64 headers and libc into `local/aarch64`, or `SB_AARCH64_HOME` if
you set it. The tool pins each download's checksum, and adds the Rust target with rustup.

SDL's CMake cache remembers the headers' folder. If you delete `local/aarch64`, delete
`target/aarch64-unknown-linux-gnu` too.
