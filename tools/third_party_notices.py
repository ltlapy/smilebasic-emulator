#!/usr/bin/env python3
"""Writes THIRD-PARTY-NOTICES.txt: the licenses of everything the binaries hold.

    python3 tools/third_party_notices.py          # write the file
    python3 tools/third_party_notices.py --check  # fail if it is out of date

It lists the crates that `smilebasic`, `smilebasic-import` and
`smilebasic-check` link, on the targets and with the features named below, with the
license files each crate ships, then SDL, the bundled fonts and the bundled
SoundFont. Run it after a change to Cargo.lock or to the bundled files, and
ship the file beside the binaries.
"""

import glob
import hashlib
import json
import os
import subprocess
import sys

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
OUT = os.path.join(REPO, "THIRD-PARTY-NOTICES.txt")
BINARIES = ("sb-sdl", "sb-import", "sb-check")
# The targets and the optional features a release may be built for.
TARGETS = (
    "x86_64-unknown-linux-gnu", "aarch64-unknown-linux-gnu", "armv7-unknown-linux-gnueabihf",
    "x86_64-pc-windows-gnu", "x86_64-pc-windows-msvc", "x86_64-apple-darwin", "aarch64-apple-darwin",
)
FEATURES = "sb-sdl/jit"
# Built from source and linked in, though Cargo calls it a build dependency.
LINKED_SOURCES = {"sdl3-sys": "sdl3-src"}
LICENSE_NAMES = ("licen", "copying", "notice", "unlicense", "copyright", "authors")
RULE = "=" * 78


def license_files(folder):
    found = []
    for path in sorted(glob.glob(os.path.join(folder, "*"))):
        name = os.path.basename(path)
        if os.path.isfile(path) and name.lower().startswith(LICENSE_NAMES):
            with open(path, encoding="utf-8", errors="replace") as f:
                found.append((name, f.read().replace("\r\n", "\n").strip("\n")))
    return found


def crates():
    meta = json.loads(
        subprocess.run(
            ["cargo", "metadata", "--format-version", "1", "--all-features", "--locked"],
            cwd=REPO, check=True, capture_output=True, text=True,
        ).stdout
    )
    by_key = {(p["name"], p["version"]): p for p in meta["packages"]}
    by_name = {p["name"]: p for p in meta["packages"]}
    ours = {by_key[k]["name"] for k in by_key if by_key[k]["id"] in meta["workspace_members"]}
    linked = {}
    for target in TARGETS:
        tree = subprocess.run(
            ["cargo", "tree", "--locked", "-e", "normal", "--prefix", "none", "--target", target,
             "--features", FEATURES, *[a for b in BINARIES for a in ("-p", b)]],
            cwd=REPO, check=True, capture_output=True, text=True,
        ).stdout
        for line in tree.splitlines():
            words = line.split()
            if len(words) >= 2 and words[0] not in ours:
                linked[(words[0], words[1].lstrip("v"))] = by_key[(words[0], words[1].lstrip("v"))]
    for user, source in LINKED_SOURCES.items():
        if any(name == user for name, _ in linked) and source in by_name:
            linked[(source, by_name[source]["version"])] = by_name[source]
    return [linked[k] for k in sorted(linked)]


def bundled():
    """The fonts and the SoundFont in the tree: (title, [(file, text)])."""
    parts = []
    fonts = os.path.join(REPO, "crates", "sb-runtime", "fonts")
    for folder in sorted(glob.glob(os.path.join(fonts, "*"))):
        files = license_files(folder)
        for extra in sorted(glob.glob(os.path.join(folder, "LICENSES", "*", "*"))):
            with open(extra, encoding="utf-8", errors="replace") as f:
                name = os.path.relpath(extra, folder).replace(os.sep, "/")
                files.append((name, f.read().replace("\r\n", "\n").strip("\n")))
        if files:
            parts.append((f"Font: crates/sb-runtime/fonts/{os.path.basename(folder)}", files))
    sounds = os.path.join(REPO, "crates", "sb-sdl", "sounds")
    files = [(os.path.basename(p), open(p, encoding="utf-8", errors="replace").read().strip("\n"))
             for p in sorted(glob.glob(os.path.join(sounds, "*LICENSE*")))]
    if files:
        parts.append(("SoundFont: GeneralUser GS (crates/sb-sdl/sounds)", files))
    return parts


def build():
    out = [
        "Third-party notices",
        "",
        "The SmileBASIC emulator's own code is under MIT OR Apache-2.0 (LICENSE-MIT,",
        "LICENSE-APACHE). Its binaries also hold the work listed here, each under",
        "its own license. tools/third_party_notices.py writes this file.",
        "",
        "The Rust standard library, linked into every binary, is under",
        "MIT OR Apache-2.0: https://www.rust-lang.org/policies/licenses",
        "",
        RULE, "Rust crates", RULE, "",
    ]
    texts = {}  # digest -> (text, [who])
    for p in crates():
        out.append(f"{p['name']} {p['version']}")
        out.append(f"  License: {p.get('license') or 'see its license file'}")
        if p.get("repository"):
            out.append(f"  Source: {p['repository']}")
        if p.get("authors"):
            out.append(f"  Authors: {', '.join(p['authors'])}")
        files = license_files(os.path.dirname(p["manifest_path"]))
        if not files:
            out.append("  The package holds no license file; its license is the one named above.")
        for name, text in files:
            digest = hashlib.sha256(text.encode()).hexdigest()
            texts.setdefault(digest, (text, []))[1].append(f"{p['name']} {p['version']} ({name})")
        out.append("")
    out += [RULE, "License texts of the crates", RULE, ""]
    for text, who in sorted(texts.values(), key=lambda t: t[1]):
        out.append("Used by: " + "; ".join(who))
        out += ["", text, "", "-" * 78, ""]
    for title, files in bundled():
        out += [RULE, title, RULE, ""]
        for name, text in files:
            out += [f"[{name}]", "", text, ""]
    return "\n".join(out).rstrip("\n") + "\n"


def main():
    text = build()
    if "--check" in sys.argv[1:]:
        with open(OUT, encoding="utf-8") as f:
            if f.read() != text:
                sys.exit("THIRD-PARTY-NOTICES.txt is out of date: run tools/third_party_notices.py")
        return
    with open(OUT, "w", encoding="utf-8") as f:
        f.write(text)


if __name__ == "__main__":
    main()
