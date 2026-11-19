#!/usr/bin/env python3
"""Cross-build smilebasic and smilebasic-check for Linux handhelds, aarch64 or armhf.

By default this uses the tools on your PATH and the -devel headers in /usr/include. It
downloads nothing: when something is missing, it prints the commands to install it and
stops.

--rootless builds without installing anything as root. It fetches zig, cargo-zigbuild,
Debian 10's headers and Debian 10's libc for the architecture, checksums pinned below,
into local/cross (or SB_CROSS_HOME). It still needs python3, rustup, cmake, make and
pkg-config.

See docs/building/building-handheld.md.
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
from dataclasses import dataclass
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
# The oldest glibc Rust's std supports on Linux, and aarch64's first, so any system has it
# or newer.
GLIBC = "2.17"
BINARIES = ("smilebasic", "smilebasic-check")

# Versions that built both binaries and matched the x86-64 checksum. Others get a warning.
ZIG_TESTED = ("0.15.2", "0.16.0")

# --rootless fetches these.
ZIG_VERSION = "0.15.2"
ZIG_SHA256 = "02aa270f183da276e5b5920b1dac44a63f1a49e55050ebde3aecc9eb82f93239"
ZIG_URL = f"https://ziglang.org/download/{ZIG_VERSION}/zig-x86_64-linux-{ZIG_VERSION}.tar.xz"
ZIGBUILD_VERSION = "0.24.1"

DEBIAN = "https://archive.debian.org/debian/pool/main/"
# Path under the pool without the architecture, then sha256 for arm64 and for armhf.
# Debian 10 "buster" has glibc 2.28.
SYSROOT_DEBS = [
    (
        "a/alsa-lib/libasound2_1.1.8-1",
        "af663d07d10b085f590c539772dbf70b279df187210ea818446dfd896487adee",
        "644a76a9e3bb138a45aa49419f73dd7dfd560e3ae2f290c490e33eca924b7618",
    ),
    (
        "a/alsa-lib/libasound2-dev_1.1.8-1",
        "617ed035bd7caaab64c7f68e5b673bd60160eb536b9e7ca3c748cc9c46949ffe",
        "9afa05ad5a79d98a656342c078499629eb821ed1721babad930a49f9e40ae388",
    ),
    (
        "libd/libdrm/libdrm-dev_2.4.97-1",
        "21bd19c7e4ad86898e3613f45683630f5f42cd696b978ebced0b93a1d943104a",
        "af5af3d311b389d54215253001fbdc710692569ee81d0adc1c25c8e21241421b",
    ),
    (
        "libd/libdrm/libdrm2_2.4.97-1",
        "c1521eb6b63dc794e9487e6fd6956201b97fc230d514ccf5bf767111e0f86efb",
        "023ca34adf90684a0d82d571980fc87522feac852f515d7054a873e4b746dc60",
    ),
    (
        "m/mesa/libgbm-dev_18.3.6-2+deb10u1",
        "cc87a516b71e7260d50583f60baf91415c8253d5f27882c6e7f1d5fa379e6f75",
        "81f0b49c9c54969f2da1527a1af35308e9d14701284a3f0ecaead34901fd2591",
    ),
    (
        "m/mesa/libgbm1_18.3.6-2+deb10u1",
        "c9c9a3fbc0c7187f28f0ad71f459a3a364803326d2e9175e458b043aa392478c",
        "26da92cfa972761840ac32854a498cbdd9ec991f59135ccea869a7603b3ee197",
    ),
    (
        "p/pulseaudio/libpulse-dev_12.2-4+deb10u1",
        "ada3da8f3245ce82fda28f2ef494029213c8a4acb16bf66bb9c6664138749182",
        "f56949743b50a40b003800bff4c92d3cd6f0370565bb4c1d250b9dd87fda7a6b",
    ),
    (
        "p/pulseaudio/libpulse0_12.2-4+deb10u1",
        "543c2329b85893045e10b7a3f59801851c92267f1f17a2cbb27473066b8eecb3",
        "f9ee14702c63135406684ee8685c61c0603ef6ae94094cce6d4352913c1a3f1b",
    ),
    (
        "s/systemd/libudev-dev_241-7~deb10u8",
        "77039dc5dba0950eea903e3af1a58f0973731738513d02db36f7c9c16d1ade71",
        "437001a74a6c7e288fb9bb45ab95e2317eb394c8ab070a32fba9472daf65bd91",
    ),
    (
        "s/systemd/libudev1_241-7~deb10u8",
        "4a673e524d3d83d33be9488d972e608404fc9e4638b4d867fc42ccff5ddb195d",
        "56b49a75f13959850db5f258c54bcddded325e87131ca7e3cd7070c34a30431e",
    ),
]
RUNROOT_DEBS = [
    (
        "g/glibc/libc6_2.28-10+deb10u1",
        "09f5e92b1527cdc06b757499bb4d97ddf1b064fc3d163f978e5de18b131cd330",
        "c10a2ee12efdb773c1c626ed318f0d74108fce0aa11d66350b1c23ddbc8d5711",
    ),
    (
        "g/gcc-8/libgcc1_8.3.0-6",
        "2851ac25d12958586c035de5ec4f2fc17272dec48f776dd0dd24c62f62674fd9",
        "2303bab1cc54099a8de5ced6601c10d4abedc62d3c12addc097f03cba1355d2d",
    ),
]


@dataclass(frozen=True)
class Arch:
    rust_target: str
    # The GNU triple, which is also Debian's library folder.
    multiarch: str
    cmake_processor: str
    debian: str
    loader: str
    qemu: str
    pointer_bits: int
    # Added to the Rust target's own features.
    rustflags: str
    # The core qemu runs: an instruction it lacks stops the run.
    qemu_cpu: str
    # Fedora's, then Debian's and Ubuntu's: a libc for qemu to load.
    host_runroots: tuple[str, ...]
    runroot_dnf: tuple[str, ...]
    runroot_apt: tuple[str, ...]
    runroot_other: tuple[str, ...]

    @property
    def output(self) -> Path:
        return REPO / "target" / self.rust_target / "dist"

    @property
    def env_suffix(self) -> str:
        return self.rust_target.replace("-", "_")

    def debs(self, table: list[tuple[str, str, str]]) -> list[tuple[str, str]]:
        column = 1 if self.debian == "arm64" else 2
        return [(f"{row[0]}_{self.debian}.deb", row[column]) for row in table]


# Named as PortMaster names them.
ARCHES = {
    "aarch64": Arch(
        rust_target="aarch64-unknown-linux-gnu",
        multiarch="aarch64-linux-gnu",
        cmake_processor="aarch64",
        debian="arm64",
        loader="ld-linux-aarch64.so.1",
        qemu="qemu-aarch64",
        pointer_bits=64,
        rustflags="",
        qemu_cpu="cortex-a35",
        host_runroots=("/usr/aarch64-redhat-linux/sys-root/fc*", "/usr/aarch64-linux-gnu"),
        runroot_dnf=("sysroot-aarch64-fc$(rpm -E %fedora)-glibc",),
        runroot_apt=("libc6-arm64-cross",),
        runroot_other=(),
    ),
    # A 32-bit userland on the same cores. Rust's armv7 target asks for ARMv7-A, Thumb-2
    # and VFPv3 with 32 double registers. NEON, which every such core has, takes 3 to 15%
    # off TPERF's instructions and leaves its checksum as it was. A Cortex-A8 has all of
    # that, without the integer divide and VFPv4 of later cores.
    "armhf": Arch(
        rust_target="armv7-unknown-linux-gnueabihf",
        multiarch="arm-linux-gnueabihf",
        cmake_processor="armv7l",
        debian="armhf",
        loader="ld-linux-armhf.so.3",
        qemu="qemu-arm",
        pointer_bits=32,
        rustflags="-C target-feature=+neon",
        qemu_cpu="cortex-a8",
        host_runroots=("/usr/arm-linux-gnueabihf",),
        runroot_dnf=(),
        runroot_apt=("libc6-armhf-cross",),
        runroot_other=("Fedora packages no armhf libc: set QEMU_LD_PREFIX to the root of one, or add --rootless",),
    ),
}

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

# Read by cmake through the cmake crate. The compiler comes from the wrappers
# cargo-zigbuild puts in the environment; the sysroot holds headers and sonames only,
# so libc's own headers and libraries stay zig's.
TOOLCHAIN = """\
set(CMAKE_SYSTEM_NAME Linux)
set(CMAKE_SYSTEM_PROCESSOR {processor})
set(CMAKE_C_COMPILER "$ENV{{CC_{suffix}}}")
set(CMAKE_CXX_COMPILER "$ENV{{CXX_{suffix}}}")
set(CMAKE_RANLIB "$ENV{{RANLIB_{suffix}}}")
set(CMAKE_C_LINKER_DEPFILE_SUPPORTED FALSE)
set(CMAKE_LIBRARY_ARCHITECTURE {multiarch})
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
    if env := os.environ.get("SB_CROSS_HOME"):
        return Path(env).resolve()
    # From a git worktree under the checkout, the checkout's local/ is the one to share.
    for parent in REPO.parents:
        if (parent / "local").is_dir() and (parent / ".git").exists():
            return parent / "local" / "cross"
    return REPO / "local" / "cross"


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
        unpack_deb(download(DEBIAN + path, sha, home() / "debs"), root)
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


def rust_target_installed(arch: Arch) -> bool:
    reply = subprocess.run(["rustup", "target", "list", "--installed"], capture_output=True, text=True, check=True)
    return arch.rust_target in reply.stdout.split()


def check_host(arch: Arch) -> None:
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
    elif not rust_target_installed(arch):
        missing.append(f"the Rust target {arch.rust_target}")
        other.append(f"rustup target add {arch.rust_target}")
    if missing:
        stop_with_install_commands(missing, dnf, apt, other)
    zig = subprocess.run(["zig", "version"], capture_output=True, text=True, check=True).stdout.strip()
    if zig not in ZIG_TESTED:
        print(f"warning: zig {zig} is untested; {' and '.join(ZIG_TESTED)} are known to work", file=sys.stderr)


def host_sysroot(arch: Arch) -> Path:
    """Links the host's headers into a folder of their own, beside placeholder libraries.

    The host's /usr/include also holds an x86-64 libc, so it cannot go on the include path.
    SDL reads a soname off the library's file name and opens the library at start-up, so the
    placeholders only have to exist, and they are empty so that linking one fails.
    """
    root = arch.output.parent / "host-sysroot"
    shutil.rmtree(root, ignore_errors=True)
    include = root / "usr" / "include"
    libdir = root / "usr" / "lib" / arch.multiarch
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
            f"Cflags: -I/usr/include{subfolder}\nLibs: -L/usr/lib/{arch.multiarch} -l{short.removeprefix('lib')}\n"
        )
    return root


def rootless_env(arch: Arch) -> tuple[dict[str, str], Path, Path]:
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
    subprocess.run(["rustup", "target", "add", arch.rust_target], env=env, check=True)
    folder = base / arch.debian
    return env, ensure_root(folder, "sysroot", arch.debs(SYSROOT_DEBS)), folder


def build_env(arch: Arch, glibc: str, rootless: bool) -> dict[str, str]:
    if rootless:
        env, sysroot, folder = rootless_env(arch)
    else:
        check_host(arch)
        env, sysroot = dict(os.environ), host_sysroot(arch)
        folder = sysroot
    if " " in str(sysroot):
        sys.exit(f"{sysroot}: C flags cannot carry a path with spaces; use a checkout or SB_CROSS_HOME without any")
    suffix = arch.env_suffix
    toolchain = folder / "toolchain.cmake"
    toolchain.write_text(
        TOOLCHAIN.format(processor=arch.cmake_processor, suffix=suffix, multiarch=arch.multiarch, sysroot=sysroot)
    )
    env[f"CMAKE_TOOLCHAIN_FILE_{suffix}"] = str(toolchain)
    env["PKG_CONFIG_SYSROOT_DIR"] = str(sysroot)
    env["PKG_CONFIG_LIBDIR"] = f"{sysroot}/usr/lib/{arch.multiarch}/pkgconfig:{sysroot}/usr/share/pkgconfig"
    env.pop("PKG_CONFIG_PATH", None)
    # For a check_include_file, which sees neither pkg-config nor the find root.
    cflags = f"-isystem {sysroot}/usr/include"
    if arch.pointer_bits == 32 and version(glibc) < (2, 34):
        # SDL asks for a 64-bit time_t on 32-bit glibc, whose functions for it came in 2.34.
        # CMake puts these flags after SDL's definitions, so this one wins.
        cflags += " -U_TIME_BITS"
    env[f"CFLAGS_{suffix}"] = cflags
    if arch.rustflags:
        rustflags = f"CARGO_TARGET_{suffix.upper()}_RUSTFLAGS"
        env[rustflags] = f"{env.get(rustflags, '')} {arch.rustflags}".strip()
    return env


def features(unchecked_vars: bool, jit: bool) -> list[str]:
    """Cargo's arguments for the optional features asked for."""
    asked = [name for name, on in (("unchecked-vars", unchecked_vars), ("jit", jit)) if on]
    return ["--features", ",".join(asked)] if asked else []


def build(arch: Arch, glibc: str, rootless: bool, unchecked_vars: bool, jit: bool) -> None:
    subprocess.run(
        ["cargo", "zigbuild", "--profile", "dist", "--target", f"{arch.rust_target}.{glibc}"]
        + ["-p", "sb-sdl", "-p", "sb-check"]
        + features(unchecked_vars, jit),
        cwd=REPO,
        env=build_env(arch, glibc, rootless),
        check=True,
    )


def version(text: str) -> tuple[int, ...]:
    return tuple(int(part) for part in text.split("."))


def tool_output(*args: str) -> str:
    return subprocess.run(args, capture_output=True, text=True, check=True).stdout


def report(arch: Arch) -> None:
    for name in BINARIES:
        path = arch.output / name
        print(f"{path.relative_to(REPO)}: {path.stat().st_size / 1e6:.2f} MB")
        print(" ", tool_output("file", "-b", str(path)).split(", BuildID")[0].strip())
        versions = set(re.findall(r"GLIBC_([0-9.]+)", tool_output("readelf", "-W", "-V", str(path))))
        newest = max(versions, key=version)
        print(f"  needs glibc {newest}")
        needed = re.findall(r"\(NEEDED\).*\[(.*)\]", tool_output("readelf", "-W", "-d", str(path)))
        print("  needed:", " ".join(needed))
    data = (arch.output / "smilebasic").read_bytes()
    for what in (b"kmsdrm", b"opengles2", b"alsa", b"pulseaudio", b"libdrm.so.2", b"libgbm.so.1", b"libudev.so.1"):
        state = "in" if what + b"\0" in data else "NOT in"
        print(f"  {what.decode()}: {state} smilebasic")


def host_runroot(arch: Arch) -> Path:
    for pattern in arch.host_runroots:
        for found in sorted(Path("/").glob(pattern.lstrip("/"))):
            if (found / "lib" / arch.loader).exists():
                return found
            if (found / "usr" / "lib" / arch.loader).exists():
                # Fedora's sysroot lacks the /lib and /lib64 links a real system has.
                root = arch.output.parent / "host-runroot"
                shutil.rmtree(root, ignore_errors=True)
                root.mkdir(parents=True)
                (root / "usr").symlink_to(found / "usr")
                (root / "lib").symlink_to(found / "usr" / "lib")
                (root / "lib64").symlink_to(found / "usr" / "lib64")
                return root
    stop_with_install_commands(
        [f"an {arch.debian} libc for {arch.qemu}"],
        list(arch.runroot_dnf),
        list(arch.runroot_apt),
        list(arch.runroot_other),
    )


def qemu_env(arch: Arch, env: dict[str, str], rootless: bool) -> dict[str, str]:
    """The libc to load, and the core to run."""
    if not shutil.which(arch.qemu):
        stop_with_install_commands([arch.qemu], ["qemu-user"], ["qemu-user"], [])
    if rootless:
        root = ensure_root(home() / arch.debian, "runroot", arch.debs(RUNROOT_DEBS))
    elif prefix := env.get("QEMU_LD_PREFIX"):
        root = Path(prefix)
    else:
        root = host_runroot(arch)
    return env | {"QEMU_LD_PREFIX": str(root), "QEMU_CPU": arch.qemu_cpu}


def run(arch: Arch, command: list[str], rootless: bool) -> int:
    """Runs a built binary under qemu. Shows no window."""
    binary = arch.output / command[0]
    env = qemu_env(arch, dict(os.environ), rootless)
    return subprocess.run([arch.qemu, str(binary), *command[1:]], env=env).returncode


def test(arch: Arch, extra: list[str], rootless: bool, unchecked_vars: bool, jit: bool) -> int:
    """Runs the tests that need no SDL under qemu."""
    env = qemu_env(arch, build_env(arch, GLIBC, rootless), rootless)
    env[f"CARGO_TARGET_{arch.env_suffix.upper()}_RUNNER"] = arch.qemu
    crates = ["-p", "sb-core", "-p", "sb-runtime", "-p", "sb-profiles", "-p", "sb-check", "-p", "sb-import"]
    command = ["cargo-zigbuild", "test", "--release", "--target", f"{arch.rust_target}.{GLIBC}"]
    command += crates + features(unchecked_vars, jit) + extra
    return subprocess.run(command, cwd=REPO, env=env).returncode


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument(
        "--arch",
        choices=ARCHES,
        default="aarch64",
        help="aarch64 (the default) for a 64-bit userland, armhf for a 32-bit one; put this before --run or --test",
    )
    parser.add_argument(
        "--rootless",
        action="store_true",
        help="build without installing anything as root: fetch zig, cargo-zigbuild and Debian 10's "
        "headers and libc into local/cross or SB_CROSS_HOME; put this before --run or --test",
    )
    parser.add_argument(
        "--unchecked-vars",
        action="store_true",
        help="build with sb-core's unchecked-vars feature: faster arithmetic, but no bounds check on "
        "each variable access (see docs/development/security.md); off by default; put this before --test",
    )
    parser.add_argument(
        "--jit",
        action="store_true",
        help="build with sb-core's jit feature: native code for hot loops, aarch64 only (see "
        "docs/performance/jit-design.md); off by default; put this before --test",
    )
    parser.add_argument(
        "--glibc",
        default=GLIBC,
        help=f"oldest glibc to run on, default {GLIBC}; after changing it, delete target/TRIPLE",
    )
    parser.add_argument(
        "--run",
        nargs=argparse.REMAINDER,
        metavar="BINARY ARGS",
        help="do not build: run a built binary under qemu with the rest of the line",
    )
    parser.add_argument(
        "--test",
        nargs=argparse.REMAINDER,
        metavar="ARGS",
        help="do not build: run the tests that need no SDL under qemu, with these cargo test arguments",
    )
    args = parser.parse_args()
    arch = ARCHES[args.arch]
    if args.run:
        return run(arch, args.run, args.rootless)
    if args.test is not None:
        return test(arch, args.test, args.rootless, args.unchecked_vars, args.jit)
    build(arch, args.glibc, args.rootless, args.unchecked_vars, args.jit)
    report(arch)
    return 0


if __name__ == "__main__":
    sys.exit(main())
