# Building for an aarch64 handheld

For devices such as an RK3326 on an AmberELEC-like system: Cortex-A35, GLES2 over
KMSDRM, no X11 or Wayland.

```sh
python3 tools/build_aarch64.py
```

It needs `python3`, `rustup`, `cmake`, `make` and `pkg-config` on an x86-64 Linux PC,
and no root. It prints what it built in `target/aarch64-unknown-linux-gnu/dist/`:
both programs' size, the newest glibc they need and the libraries they link, and which
of SDL's backends are in.

The first run fetches these, checksums pinned in the tool, into `local/aarch64` (or
`SB_AARCH64_HOME`):

- Zig, as C compiler and linker, with `cargo-zigbuild` to point cargo, cc and cmake at it.
  Zig carries stubs of every glibc, so the result runs on any glibc since 2.17, aarch64's
  first, without an old system to build on. `--glibc` raises that.
- Debian 10's arm64 headers and sonames for libdrm, gbm, ALSA, PulseAudio and udev. SDL
  opens those libraries when it starts, so nothing of them is linked, and a device without
  one just lacks that backend. EGL and GLES2 headers come with SDL.

The binaries link only to libc, libm, libdl and libpthread, and SDL is inside.

## Checking it here

```sh
python3 tools/build_aarch64.py --run smilebasic-check --bench --fixed 1200 DIR PROGRAM
python3 tools/build_aarch64.py --test --no-fail-fast
```

These run under `qemu-aarch64` with a Cortex-A35's instruction set on Debian 10's libc. The
checksum has to be the one an x86-64 build prints for the same line: a difference is a
real finding, in floating point most likely. `--test` skips `sb-sdl`.
`a_busy_subprogram_keeps_to_the_frames_time` times a 2 ms limit and can fail on a busy
machine, natively too; run it alone to tell. Times under qemu say nothing about the device.

`smilebasic` itself runs too, with `SDL_VIDEO_DRIVER=dummy SDL_AUDIO_DRIVER=dummy`
and no window. It can only reach KMSDRM and GLES2 on the device.

## On the device

Find out what it has, over SSH:

```sh
uname -m                                        # aarch64; a 32-bit userland needs another build
ldd --version | head -1
ls /usr/lib/lib{EGL,GLESv2,gbm,drm,asound}*
```

Quit the front end, which holds the display: `systemctl stop emustation` on AmberELEC
and its forks. Copy `smilebasic`, `smilebasic-check` and a project, in a folder
`projects/NAME` under `$XDG_DATA_HOME/smilebasic-emulator` for `smilebasic`:

```sh
export XDG_DATA_HOME=/storage/sb

# Does the GPU path draw the same as the CPU? Exits 1 if a scene differs too much.
SB_GPU_GOLDEN=1 SDL_VIDEO_DRIVER=kmsdrm SDL_RENDER_DRIVER=opengles2 ./smilebasic

# Frame times on the GPU path, and who drew each frame.
SB_GPU=sync SB_UI_BENCH=frames=600,run=NAME/PROGRAM \
    SDL_VIDEO_DRIVER=kmsdrm SDL_RENDER_DRIVER=opengles2 ./smilebasic

# The CPU alone; the checksum has to match the PC's.
perf stat -e instructions:u,cycles:u -- \
    ./smilebasic-check --bench --fixed 400 "$XDG_DATA_HOME/smilebasic-emulator/projects/NAME" PROGRAM
```

Without `perf`, run the last line alone for times. SDL looks for `libEGL.so.1` and
`libGLESv2.so.2`. If the driver goes by other names, as a Mali blob may, set
`SDL_VIDEO_EGL_DRIVER` and `SDL_VIDEO_GL_DRIVER` to its paths.
[benchmarking.md](benchmarking.md) explains the lines' output.
