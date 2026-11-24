#!/usr/bin/env python3
"""Writes the order of SmileBASIC 4's font page, from a font.map, for the fallback font.

    python3 tools/font_order.py FONT_MAP [OUT]
    python3 tools/font_order.py --dump [TABLE]

FONT_MAP is the font.map from your own copy of SmileBASIC 4 (smilebasic-import puts it in
the data directory's resources folder). OUT defaults to
crates/sb-runtime/fonts/sb4-order/order.bin. --dump prints a table as text, one
"U+XXXX glyph" line per code, to review it.

The table keeps only each character code's glyph number, the place of its glyph on the
font page (GRP5). It drops the metrics and attribute bits. The glyph pictures are in
GDEFFONT, which is not read here. Codes that font.map maps to the missing glyph (703),
U+FFFD among them, are excluded.

Format: u16 words, little-endian, one entry per code in ascending code order. Bits 0-11 of a
word are the glyph number. Bits 12-15 are the gap g to the previous entry's code: the code
is the previous one plus g + 1, the previous one being -1 before the first entry. For
g = 15 the code is the next word instead. Needs only the Python standard library.
"""

import struct
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
TABLE = REPO / "crates/sb-runtime/fonts/sb4-order/order.bin"
CODES = 0x10000
MISSING = 703
FAR = 15


def read_map(path):
    """Each code's glyph number, from a 4.x (u32 entries) or 1.x (u16 entries) font.map."""
    data = Path(path).read_bytes()
    if len(data) == CODES * 4:
        entries = struct.unpack(f"<{CODES}I", data)
    elif len(data) == CODES * 2:
        entries = struct.unpack(f"<{CODES}H", data)
    else:
        sys.exit(f"{path}: not a font.map ({len(data)} bytes)")
    return [e & 0x0FFF for e in entries]


def encode(glyphs):
    words = []
    prev = -1
    for code, n in enumerate(glyphs):
        if n == MISSING:
            continue
        gap = code - prev - 1
        if gap < FAR:
            words.append(gap << 12 | n)
        else:
            words += [FAR << 12 | n, code]
        prev = code
    return struct.pack(f"<{len(words)}H", *words)


def decode(data):
    """(code, glyph number) pairs, in code order."""
    words = iter(struct.unpack(f"<{len(data) // 2}H", data))
    prev = -1
    for w in words:
        gap = w >> 12
        code = next(words) if gap == FAR else prev + gap + 1
        yield code, w & 0x0FFF
        prev = code


def main(argv):
    if argv[:1] == ["--dump"]:
        path = argv[1] if len(argv) > 1 else TABLE
        for code, n in decode(Path(path).read_bytes()):
            print(f"U+{code:04X} {n}")
        return
    if len(argv) not in (1, 2) or argv[0].startswith("-"):
        sys.exit(__doc__)
    glyphs = read_map(argv[0])
    data = encode(glyphs)
    expected = [(c, n) for c, n in enumerate(glyphs) if n != MISSING]
    assert list(decode(data)) == expected
    out = Path(argv[1]) if len(argv) > 1 else TABLE
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_bytes(data)
    print(f"{out.name}: {len(expected)} codes, {len(data)} bytes")


if __name__ == "__main__":
    main(sys.argv[1:])
