#!/usr/bin/env python3
"""Cross-build smilebasic and smilebasic-check for aarch64 Linux handhelds.

By default this uses the tools on your PATH and the -devel headers in /usr/include. It
downloads nothing: when something is missing, it prints the commands to install it and
stops.

--rootless builds without installing anything as root. It fetches zig, cargo-zigbuild,
Debian 10's arm64 headers and Debian 10's libc, checksums pinned below, into local/aarch64
(or SB_AARCH64_HOME). It still needs python3, rustup, cmake, make and pkg-config.

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

# Versions that built both binaries and matched the x86-64 checksum. Others get a warning.
ZIG_TESTED = ("0.15.2", "0.16.0")

# --rootless fetches these.
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

# What SDL needs from each library it opens at start-up: headers, and the soname to open.
# (name, soname, headers, Cflags subfolder, Fedora package, apt package)
HOST_LIBRARIES = [
    ("libdrm", "libdrm.so.2", ("libdrm", "xf86drm.h", "xf86drmMode.h"), "/libdrm", "libdrm-devel", "libdrm-dev"),
    ("gbm", "libgbm.so.1", ("gbm.h",), "", "mesa-libgbm-devel", "libgbm-dev"),
    ("alsa", "libasound.so.2", ("alsa",), "", "alsa-lib-devel", "libasound2-dev"),
    ("libpulse", "libpulse.so.0", ("pulse",), "", "pulseaudio-libs-devel", "libpulse-dev"),
    ("libudev", "libudev.so.1", ("libudev.h",), "", "systemd-devel", "libudev-dev"),
]
HOST_INCLUDE = Path("/usr/include")
# Program on PATH, Fedora package, apt package.
HOST_PROGRAMS = [
    ("cmake", "cmake", "cmake"),
    ("make", "make", "make"),
    ("pkg-config", "pkgconf-pkg-config", "pkg-config"),
    ("readelf", "binutils", "binutils"),
    ("file", "file", "file"),
]
# Fedora's, then Debian's and Ubuntu's: an aarch64 libc for qemu-aarch64 to load.
HOST_RUNROOTS = ("/usr/aarch64-redhat-linux/sys-root/fc*", "/usr/aarch64-linux-gnu")

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


def stop_with_install_commands(missing: list[str], dnf: list[str], apt: list[str], other: list[str]) -> None:
    lines = ["Missing: " + ", ".join(missing), "", "Install it, then run this again:"]
    if dnf:
        lines += ["  Fedora:         sudo dnf install " + " ".join(dnf)]
    if apt:
        lines += ["  Debian, Ubuntu: sudo apt install " + " ".join(apt)]
    lines += ["  " + line for line in other]
    lines += ["", "To build without installing anything as root, add --rootless."]
    sys.exit("\n".join(lines))


def rust_target_installed() -> bool:
    reply = subprocess.run(["rustup", "target", "list", "--installed"], capture_output=True, text=True, check=True)
    return RUST_TARGET in reply.stdout.split()


def check_host() -> None:
    """Stops before building if anything is missing, and installs nothing."""
    missing, dnf, apt, other = [], [], [], []
    for program, fedora, debian in HOST_PROGRAMS:
        if not shutil.which(program):
            missing.append(program)
            dnf.append(fedora)
            apt.append(debian)
    for name, _, headers, _, fedora, debian in HOST_LIBRARIES:
        if not all((HOST_INCLUDE / header).exists() for header in headers):
            missing.append(f"{name} headers")
            dnf.append(fedora)
            apt.append(debian)
    if not shutil.which("zig"):
        missing.append("zig")
        dnf.append("zig")
        other.append("Ubuntu 26.04 or later: sudo apt install zig")
        other.append("Debian, older Ubuntu: unpack a release from https://ziglang.org/download/ and add it to PATH")
    if not shutil.which("cargo-zigbuild"):
        missing.append("cargo-zigbuild")
        other.append("cargo install --locked cargo-zigbuild")
    if not shutil.which("rustup"):
        missing.append("rustup")
        other.append("rustup: see https://rustup.rs")
    elif not rust_target_installed():
        missing.append(f"the Rust target {RUST_TARGET}")
        other.append(f"rustup target add {RUST_TARGET}")
    if missing:
        stop_with_install_commands(missing, dnf, apt, other)
    zig = subprocess.run(["zig", "version"], capture_output=True, text=True, check=True).stdout.strip()
    if zig not in ZIG_TESTED:
        print(f"warning: zig {zig} is untested; {' and '.join(ZIG_TESTED)} are known to work", file=sys.stderr)


def host_sysroot() -> Path:
    """Links the host's headers into a folder of their own, beside placeholder libraries.

    The host's /usr/include also holds an x86-64 libc, so it cannot go on the include path.
    SDL reads a soname off the library's file name and opens the library at start-up, so the
    placeholders only have to exist, and they are empty so that linking one fails.
    """
    root = OUTPUT.parent / "host-sysroot"
    shutil.rmtree(root, ignore_errors=True)
    include = root / "usr" / "include"
    libdir = root / "usr" / "lib" / "aarch64-linux-gnu"
    pkgconfig = libdir / "pkgconfig"
    include.mkdir(parents=True)
    pkgconfig.mkdir(parents=True)
    for name, soname, headers, subfolder, _, _ in HOST_LIBRARIES:
        for header in headers:
            (include / header).symlink_to(HOST_INCLUDE / header)
        (libdir / soname).touch()
        short = soname.split(".so")[0]
        (libdir / f"{short}.so").symlink_to(soname)
        # 99 satisfies SDL's only version test, libpulse>=0.9.15.
        (pkgconfig / f"{name}.pc").write_text(
            f"Name: {name}\nDescription: {name}\nVersion: 99\n"
            f"Cflags: -I/usr/include{subfolder}\nLibs: -L/usr/lib/aarch64-linux-gnu -l{short.removeprefix('lib')}\n"
        )
    return root


def rootless_env() -> tuple[dict[str, str], Path, Path]:
    """Fetches what it lacks into its own folder. Returns the environment, sysroot and folder."""
    base = home()
    base.mkdir(parents=True, exist_ok=True)
    zig = ensure_zig(base)
    env = dict(os.environ)
    env["PATH"] = os.pathsep.join([str(base / "cargo" / "bin"), str(zig), env["PATH"]])
    # Keeps the wrappers cargo-zigbuild writes and zig's cache out of the home folder.
    env["XDG_CACHE_HOME"] = str(base / "cache")
    env["ZIG_GLOBAL_CACHE_DIR"] = str(base / "cache" / "zig")
    ensure_zigbuild(base, env)
    subprocess.run(["rustup", "target", "add", RUST_TARGET], env=env, check=True)
    return env, ensure_root(base, "sysroot", SYSROOT_DEBS), base


def build_env(rootless: bool) -> dict[str, str]:
    if rootless:
        env, sysroot, folder = rootless_env()
    else:
        check_host()
        env, sysroot = dict(os.environ), host_sysroot()
        folder = sysroot
    if " " in str(sysroot):
        sys.exit(f"{sysroot}: C flags cannot carry a path with spaces; use a checkout or SB_AARCH64_HOME without any")
    toolchain = folder / "toolchain.cmake"
    toolchain.write_text(TOOLCHAIN.format(sysroot=sysroot))
    env["CMAKE_TOOLCHAIN_FILE_aarch64_unknown_linux_gnu"] = str(toolchain)
    env["PKG_CONFIG_SYSROOT_DIR"] = str(sysroot)
    env["PKG_CONFIG_LIBDIR"] = f"{sysroot}/usr/lib/aarch64-linux-gnu/pkgconfig:{sysroot}/usr/share/pkgconfig"
    env.pop("PKG_CONFIG_PATH", None)
    # For a check_include_file, which sees neither pkg-config nor the find root.
    env["CFLAGS_aarch64_unknown_linux_gnu"] = f"-isystem {sysroot}/usr/include"
    return env


def build(glibc: str, rootless: bool) -> None:
    subprocess.run(
        ["cargo", "zigbuild", "--profile", "dist", "--target", f"{RUST_TARGET}.{glibc}"]
        + ["-p", "sb-sdl", "-p", "sb-check"],
        cwd=REPO,
        env=build_env(rootless),
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


def host_runroot() -> Path:
    for pattern in HOST_RUNROOTS:
        for found in sorted(Path("/").glob(pattern.lstrip("/"))):
            if (found / "lib" / "ld-linux-aarch64.so.1").exists():
                return found
            if (found / "usr" / "lib" / "ld-linux-aarch64.so.1").exists():
                # Fedora's sysroot lacks the /lib and /lib64 links a real system has.
                root = OUTPUT.parent / "host-runroot"
                shutil.rmtree(root, ignore_errors=True)
                root.mkdir(parents=True)
                (root / "usr").symlink_to(found / "usr")
                (root / "lib").symlink_to(found / "usr" / "lib")
                (root / "lib64").symlink_to(found / "usr" / "lib64")
                return root
    stop_with_install_commands(
        ["an aarch64 libc for qemu-aarch64"],
        ["sysroot-aarch64-fc$(rpm -E %fedora)-glibc"],
        ["libc6-arm64-cross"],
        [],
    )


def qemu_env(env: dict[str, str], rootless: bool) -> dict[str, str]:
    """The libc to load, and the device's core: an instruction it lacks stops the run."""
    if not shutil.which("qemu-aarch64"):
        stop_with_install_commands(["qemu-aarch64"], ["qemu-user"], ["qemu-user"], [])
    if rootless:
        root = ensure_root(home(), "runroot", RUNROOT_DEBS)
    elif prefix := env.get("QEMU_LD_PREFIX"):
        root = Path(prefix)
    else:
        root = host_runroot()
    return env | {"QEMU_LD_PREFIX": str(root), "QEMU_CPU": "cortex-a35"}


def run(command: list[str], rootless: bool) -> int:
    """Runs a built binary under qemu-aarch64. Shows no window."""
    binary = OUTPUT / command[0]
    env = qemu_env(dict(os.environ), rootless)
    return subprocess.run(["qemu-aarch64", str(binary), *command[1:]], env=env).returncode


def test(extra: list[str], rootless: bool) -> int:
    """Runs the tests that need no SDL under qemu-aarch64."""
    env = qemu_env(build_env(rootless), rootless)
    env["CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_RUNNER"] = "qemu-aarch64"
    crates = ["-p", "sb-core", "-p", "sb-runtime", "-p", "sb-profiles", "-p", "sb-check"]
    command = ["cargo-zigbuild", "test", "--release", "--target", f"{RUST_TARGET}.{GLIBC}"]
    return subprocess.run(command + crates + extra, cwd=REPO, env=env).returncode


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument(
        "--rootless",
        action="store_true",
        help="build without installing anything as root: fetch zig, cargo-zigbuild and Debian 10's "
        "headers and libc into local/aarch64 or SB_AARCH64_HOME; put this before --run or --test",
    )
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
        return run(args.run, args.rootless)
    if args.test is not None:
        return test(args.test, args.rootless)
    build(args.glibc, args.rootless)
    report()
    return 0


if __name__ == "__main__":
    sys.exit(main())
