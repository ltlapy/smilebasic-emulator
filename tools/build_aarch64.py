#!/usr/bin/env python3
"""Cross-build smilebasic and smilebasic-check for aarch64 Linux handhelds.

Needs python3, rustup, cmake, make and pkg-config, and no root. Everything else is
fetched once, with its checksum pinned, into local/aarch64 (or SB_AARCH64_HOME):

  zig             the C compiler and linker, which carries stubs of every glibc
  cargo-zigbuild  points cargo, cc and cmake at zig
  sysroot         Debian 10 arm64 packages: the headers and sonames SDL's KMSDRM,
                  ALSA, PulseAudio and udev backends need; none of it gets linked
  runroot         Debian 10's libc, to run the result under qemu-aarch64 (--run, --test)

See docs/building-handheld.md.
"""

import argparse
import hashlib
import io
import os
import re
import shutil
import subprocess
import sys
import tarfile
import urllib.request
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
RUST_TARGET = "aarch64-unknown-linux-gnu"
# The first glibc for aarch64, so any system has it or newer.
GLIBC = "2.17"
OUTPUT = REPO / "target" / RUST_TARGET / "dist"
BINARIES = ("smilebasic", "smilebasic-check")

ZIG_VERSION = "0.15.2"
ZIG_SHA256 = "02aa270f183da276e5b5920b1dac44a63f1a49e55050ebde3aecc9eb82f93239"
ZIG_URL = f"https://ziglang.org/download/{ZIG_VERSION}/zig-x86_64-linux-{ZIG_VERSION}.tar.xz"
ZIGBUILD_VERSION = "0.24.1"

DEBIAN = "https://archive.debian.org/debian/pool/main/"
# Path under the pool, sha256. Debian 10 "buster" has glibc 2.28.
SYSROOT_DEBS = [
    ("a/alsa-lib/libasound2_1.1.8-1_arm64.deb", "af663d07d10b085f590c539772dbf70b279df187210ea818446dfd896487adee"),
    ("a/alsa-lib/libasound2-dev_1.1.8-1_arm64.deb", "617ed035bd7caaab64c7f68e5b673bd60160eb536b9e7ca3c748cc9c46949ffe"),
    ("libd/libdrm/libdrm-dev_2.4.97-1_arm64.deb", "21bd19c7e4ad86898e3613f45683630f5f42cd696b978ebced0b93a1d943104a"),
    ("libd/libdrm/libdrm2_2.4.97-1_arm64.deb", "c1521eb6b63dc794e9487e6fd6956201b97fc230d514ccf5bf767111e0f86efb"),
    ("m/mesa/libgbm-dev_18.3.6-2+deb10u1_arm64.deb", "cc87a516b71e7260d50583f60baf91415c8253d5f27882c6e7f1d5fa379e6f75"),
    ("m/mesa/libgbm1_18.3.6-2+deb10u1_arm64.deb", "c9c9a3fbc0c7187f28f0ad71f459a3a364803326d2e9175e458b043aa392478c"),
    ("p/pulseaudio/libpulse-dev_12.2-4+deb10u1_arm64.deb", "ada3da8f3245ce82fda28f2ef494029213c8a4acb16bf66bb9c6664138749182"),
    ("p/pulseaudio/libpulse0_12.2-4+deb10u1_arm64.deb", "543c2329b85893045e10b7a3f59801851c92267f1f17a2cbb27473066b8eecb3"),
    ("s/systemd/libudev-dev_241-7~deb10u8_arm64.deb", "77039dc5dba0950eea903e3af1a58f0973731738513d02db36f7c9c16d1ade71"),
    ("s/systemd/libudev1_241-7~deb10u8_arm64.deb", "4a673e524d3d83d33be9488d972e608404fc9e4638b4d867fc42ccff5ddb195d"),
]
RUNROOT_DEBS = [
    ("g/glibc/libc6_2.28-10+deb10u1_arm64.deb", "09f5e92b1527cdc06b757499bb4d97ddf1b064fc3d163f978e5de18b131cd330"),
    ("g/gcc-8/libgcc1_8.3.0-6_arm64.deb", "2851ac25d12958586c035de5ec4f2fc17272dec48f776dd0dd24c62f62674fd9"),
]

# Read by cmake through the cmake crate. The compiler comes from the wrappers
# cargo-zigbuild puts in the environment; the sysroot holds headers and sonames only,
# so libc's own headers and libraries stay zig's.
TOOLCHAIN = """\
set(CMAKE_SYSTEM_NAME Linux)
set(CMAKE_SYSTEM_PROCESSOR aarch64)
set(CMAKE_C_COMPILER "$ENV{{CC_aarch64_unknown_linux_gnu}}")
set(CMAKE_CXX_COMPILER "$ENV{{CXX_aarch64_unknown_linux_gnu}}")
set(CMAKE_RANLIB "$ENV{{RANLIB_aarch64_unknown_linux_gnu}}")
set(CMAKE_C_LINKER_DEPFILE_SUPPORTED FALSE)
set(CMAKE_LIBRARY_ARCHITECTURE aarch64-linux-gnu)
set(CMAKE_FIND_ROOT_PATH "{sysroot}")
set(CMAKE_FIND_ROOT_PATH_MODE_PROGRAM NEVER)
set(CMAKE_FIND_ROOT_PATH_MODE_LIBRARY ONLY)
set(CMAKE_FIND_ROOT_PATH_MODE_INCLUDE ONLY)
set(CMAKE_FIND_ROOT_PATH_MODE_PACKAGE ONLY)
set(SDL_UNIX_CONSOLE_BUILD ON CACHE BOOL "No X11 or Wayland: KMSDRM only")
set(SDL_X11 OFF CACHE BOOL "")
set(SDL_WAYLAND OFF CACHE BOOL "")
set(SDL_VULKAN OFF CACHE BOOL "")
set(SDL_OPENGL OFF CACHE BOOL "")
set(SDL_PIPEWIRE OFF CACHE BOOL "")
set(SDL_JACK OFF CACHE BOOL "")
set(SDL_SNDIO OFF CACHE BOOL "")
set(SDL_DBUS OFF CACHE BOOL "")
set(SDL_IBUS OFF CACHE BOOL "")
set(SDL_LIBURING OFF CACHE BOOL "")
set(SDL_HIDAPI_LIBUSB OFF CACHE BOOL "")
set(SDL_KMSDRM ON CACHE BOOL "")
set(SDL_OPENGLES ON CACHE BOOL "")
"""


def home() -> Path:
    if env := os.environ.get("SB_AARCH64_HOME"):
        return Path(env).resolve()
    # From a git worktree under the checkout, the checkout's local/ is the one to share.
    for parent in REPO.parents:
        if (parent / "local").is_dir() and (parent / ".git").exists():
            return parent / "local" / "aarch64"
    return REPO / "local" / "aarch64"


def download(url: str, sha256: str, folder: Path) -> Path:
    folder.mkdir(parents=True, exist_ok=True)
    path = folder / url.rsplit("/", 1)[1]
    if not path.exists():
        print(f"fetching {url}", flush=True)
        with urllib.request.urlopen(url, timeout=120) as reply:
            data = reply.read()
        path.write_bytes(data)
    if hashlib.sha256(path.read_bytes()).hexdigest() != sha256:
        path.unlink()
        sys.exit(f"{url}: checksum differs from the one pinned here; removed the download")
    return path


def extract_filter(root: Path):
    """Keeps a package inside root, turning its absolute links into relative ones."""

    def keep(member: tarfile.TarInfo, dest: str):
        name = member.name.removeprefix("./")
        if name.startswith(("usr/share/doc", "usr/share/man", "usr/share/lintian", "usr/bin")):
            return None
        if member.issym() and member.linkname.startswith("/"):
            link = root / member.linkname.lstrip("/")
            target = os.path.relpath(link, (root / name).parent)
            member = member.replace(linkname=target, deep=False)
        return tarfile.data_filter(member, dest)

    return keep


def unpack_deb(deb: Path, root: Path) -> None:
    """A .deb is an ar archive, and dpkg is not on every distribution."""
    data = deb.read_bytes()
    if not data.startswith(b"!<arch>\n"):
        sys.exit(f"{deb}: not a Debian package")
    at = 8
    while at + 60 <= len(data):
        name = data[at : at + 16].decode().strip().rstrip("/")
        size = int(data[at + 48 : at + 58])
        if name.startswith("data.tar"):
            with tarfile.open(fileobj=io.BytesIO(data[at + 60 : at + 60 + size])) as tar:
                tar.extractall(root, filter=extract_filter(root))
            return
        at += 60 + size + (size & 1)
    sys.exit(f"{deb}: no data in the package")


def ensure_root(base: Path, name: str, debs: list[tuple[str, str]]) -> Path:
    root = base / name
    stamp = base / f"{name}.done"
    wanted = "\n".join(sha for _, sha in debs)
    if stamp.exists() and stamp.read_text() == wanted:
        return root
    shutil.rmtree(root, ignore_errors=True)
    root.mkdir(parents=True)
    for path, sha in debs:
        unpack_deb(download(DEBIAN + path, sha, base / "debs"), root)
    stamp.write_text(wanted)
    return root


def ensure_zig(base: Path) -> Path:
    folder = base / f"zig-x86_64-linux-{ZIG_VERSION}"
    if not (folder / "zig").exists():
        tarball = download(ZIG_URL, ZIG_SHA256, base / "dl-zig")
        with tarfile.open(tarball) as tar:
            tar.extractall(base, filter="data")
    return folder


def ensure_zigbuild(base: Path, env: dict[str, str]) -> None:
    tool = base / "cargo" / "bin" / "cargo-zigbuild"
    if tool.exists():
        return
    subprocess.run(
        ["cargo", "install", "cargo-zigbuild", "--version", ZIGBUILD_VERSION, "--locked", "--root", base / "cargo"],
        env=env,
        check=True,
    )


def build_env() -> dict[str, str]:
    base = home()
    if " " in str(base):
        sys.exit(f"{base}: C flags cannot carry a path with spaces; set SB_AARCH64_HOME")
    base.mkdir(parents=True, exist_ok=True)
    zig = ensure_zig(base)
    env = dict(os.environ)
    env["PATH"] = os.pathsep.join([str(base / "cargo" / "bin"), str(zig), env["PATH"]])
    # Keeps the wrappers cargo-zigbuild writes and zig's cache out of the home folder.
    env["XDG_CACHE_HOME"] = str(base / "cache")
    env["ZIG_GLOBAL_CACHE_DIR"] = str(base / "cache" / "zig")
    ensure_zigbuild(base, env)
    subprocess.run(["rustup", "target", "add", RUST_TARGET], env=env, check=True)

    sysroot = ensure_root(base, "sysroot", SYSROOT_DEBS)
    toolchain = base / "toolchain.cmake"
    toolchain.write_text(TOOLCHAIN.format(sysroot=sysroot))
    env["CMAKE_TOOLCHAIN_FILE_aarch64_unknown_linux_gnu"] = str(toolchain)
    env["PKG_CONFIG_SYSROOT_DIR"] = str(sysroot)
    env["PKG_CONFIG_LIBDIR"] = f"{sysroot}/usr/lib/aarch64-linux-gnu/pkgconfig:{sysroot}/usr/share/pkgconfig"
    env.pop("PKG_CONFIG_PATH", None)
    # For a check_include_file, which sees neither pkg-config nor the find root.
    env["CFLAGS_aarch64_unknown_linux_gnu"] = f"-isystem {sysroot}/usr/include"
    return env


def build(glibc: str) -> None:
    subprocess.run(
        ["cargo", "zigbuild", "--profile", "dist", "--target", f"{RUST_TARGET}.{glibc}"]
        + ["-p", "sb-sdl", "-p", "sb-check"],
        cwd=REPO,
        env=build_env(),
        check=True,
    )


def tool_output(*args: str) -> str:
    return subprocess.run(args, capture_output=True, text=True, check=True).stdout


def report() -> None:
    for name in BINARIES:
        path = OUTPUT / name
        print(f"{path.relative_to(REPO)}: {path.stat().st_size / 1e6:.2f} MB")
        print(" ", tool_output("file", "-b", str(path)).split(", BuildID")[0])
        versions = set(re.findall(r"GLIBC_([0-9.]+)", tool_output("readelf", "-W", "-V", str(path))))
        newest = max(versions, key=lambda v: tuple(int(p) for p in v.split(".")))
        print(f"  needs glibc {newest}")
        needed = re.findall(r"\(NEEDED\).*\[(.*)\]", tool_output("readelf", "-W", "-d", str(path)))
        print("  needed:", " ".join(needed))
    data = (OUTPUT / "smilebasic").read_bytes()
    for what in (b"kmsdrm", b"opengles2", b"alsa", b"pulseaudio", b"libdrm.so.2", b"libgbm.so.1", b"libudev.so.1"):
        state = "in" if what + b"\0" in data else "NOT in"
        print(f"  {what.decode()}: {state} smilebasic")


def qemu_env(env: dict[str, str]) -> dict[str, str]:
    """Debian 10's libc, and the device's core: an instruction it lacks stops the run."""
    root = ensure_root(home(), "runroot", RUNROOT_DEBS)
    return env | {"QEMU_LD_PREFIX": str(root), "QEMU_CPU": "cortex-a35"}


def run(command: list[str]) -> int:
    """Runs a built binary under qemu-aarch64. Shows no window."""
    binary = OUTPUT / command[0]
    return subprocess.run(["qemu-aarch64", str(binary), *command[1:]], env=qemu_env(dict(os.environ))).returncode


def test(extra: list[str]) -> int:
    """Runs the tests that need no SDL under qemu-aarch64."""
    env = qemu_env(build_env())
    env["CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_RUNNER"] = "qemu-aarch64"
    crates = ["-p", "sb-core", "-p", "sb-runtime", "-p", "sb-profiles", "-p", "sb-check"]
    command = ["cargo-zigbuild", "test", "--release", "--target", f"{RUST_TARGET}.{GLIBC}"]
    return subprocess.run(command + crates + extra, cwd=REPO, env=env).returncode


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument(
        "--glibc",
        default=GLIBC,
        help=f"oldest glibc to run on, default {GLIBC}; after changing it, delete target/{RUST_TARGET}",
    )
    parser.add_argument(
        "--run",
        nargs=argparse.REMAINDER,
        metavar="BINARY ARGS",
        help="do not build: run a built binary under qemu-aarch64 with the rest of the line",
    )
    parser.add_argument(
        "--test",
        nargs=argparse.REMAINDER,
        metavar="ARGS",
        help="do not build: run the tests that need no SDL under qemu-aarch64, with these cargo test arguments",
    )
    args = parser.parse_args()
    if args.run:
        return run(args.run)
    if args.test is not None:
        return test(args.test)
    build(args.glibc)
    report()
    return 0


if __name__ == "__main__":
    sys.exit(main())
