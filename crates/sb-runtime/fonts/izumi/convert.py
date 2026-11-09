"""Makes izumi16.hex, Izumi Gothic 16 by Unicode code, from the original
BDF files, which are numbered by JIS X 0213 code.

    python3 convert.py izmg16-2004-1.bdf izmg16-2004-2.bdf izumi16.hex

The inputs are the two planes of `izmg16.beta6.bdf.tar.gz`. The output is
in GNU Unifont's .hex format: one line per character, its code in hex, a
colon, then 16 rows of 16 pixels as 64 hex digits, the leftmost pixel in
the highest bit. The glyphs stay as drawn: the build script makes them
bold. JIS X 0213 characters that Unicode spells with two code points (kana
with a semi-voiced mark, for example) have no place in a one-code table
and are left out. Needs only the Python standard library.
"""

import sys


def read_bdf(path):
    """The glyphs of a BDF file: {JIS code: rows}, each glyph 16x16."""
    glyphs = {}
    code = rows = None
    with open(path, encoding="ascii") as f:
        for line in f:
            words = line.split()
            if not words:
                continue
            key = words[0]
            if key == "ENCODING":
                code = int(words[1])
            elif key == "BBX":
                if words[1:] != ["16", "16", "0", "-2"]:
                    sys.exit(f"{path}: JIS {code:04X} is not a 16x16 cell")
            elif key == "BITMAP":
                rows = []
            elif key == "ENDCHAR":
                if len(rows) != 16:
                    sys.exit(f"{path}: JIS {code:04X} has {len(rows)} rows")
                glyphs[code] = rows
                rows = None
            elif rows is not None:
                rows.append(int(key, 16))
    return glyphs


def to_unicode(plane, code):
    """The Unicode text of a JIS X 0213 code, through EUC-JIS-2004."""
    euc = bytes([code >> 8 | 0x80, code & 0xFF | 0x80])
    if plane == 2:
        euc = b"\x8f" + euc
    return euc.decode("euc_jis_2004")


def main(plane1, plane2, out):
    glyphs = {}
    for plane, path in ((1, plane1), (2, plane2)):
        for code, rows in read_bdf(path).items():
            text = to_unicode(plane, code)
            if len(text) != 1:
                continue
            if ord(text) in glyphs:
                sys.exit(f"U+{ord(text):04X} appears twice")
            glyphs[ord(text)] = rows
    with open(out, "w", encoding="ascii", newline="\n") as f:
        for code in sorted(glyphs):
            digits = 4 if code <= 0xFFFF else 6
            pixels = "".join(f"{row:04X}" for row in glyphs[code])
            f.write(f"{code:0{digits}X}:{pixels}\n")


if __name__ == "__main__":
    main(*sys.argv[1:4])
