"""Keeps the Hangul of Galmuri14 for the editor's 16-pixel extra glyphs.

    python3 subset14.py Galmuri14.bdf Galmuri14-Hangul.bdf

The input is dist/Galmuri14.bdf of the Galmuri release. Glyphs outside
HANGUL are dropped; everything else is copied as it is.
"""

import sys

HANGUL = [
    (0x1100, 0x11FF),  # Jamo
    (0x3130, 0x318F),  # Compatibility Jamo
    (0xA960, 0xA97F),  # Jamo Extended-A
    (0xAC00, 0xD7A3),  # Syllables
    (0xD7B0, 0xD7FF),  # Jamo Extended-B
    (0xFFA0, 0xFFDF),  # Half-width forms
]


def wanted(code):
    return any(a <= code <= b for a, b in HANGUL)


def main(src, dst):
    header, glyphs, current = [], [], None
    with open(src, encoding="utf-8") as f:
        for line in f:
            if line.startswith("STARTCHAR"):
                current = [line]
            elif current is not None:
                current.append(line)
                if line.startswith("ENDCHAR"):
                    code = next(int(l.split()[1]) for l in current if l.startswith("ENCODING"))
                    if wanted(code):
                        glyphs.append(current)
                    current = None
            elif not line.startswith(("CHARS ", "ENDFONT")):
                header.append(line)
    with open(dst, "w", encoding="utf-8", newline="\n") as f:
        f.writelines(header)
        f.write(f"CHARS {len(glyphs)}\n")
        for glyph in glyphs:
            f.writelines(glyph)
        f.write("ENDFONT\n")


if __name__ == "__main__":
    main(sys.argv[1], sys.argv[2])
