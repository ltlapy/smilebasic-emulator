"""Makes the bundled 16-pixel fallback font: Noto Sans Mono CJK JP, fixed at
the Regular weight, cut down to the characters SmileBASIC programs use,
turned into TrueType outlines, and renamed as the SIL OFL asks of modified
versions.

    python3 subset.py NotoSansMonoCJK-VF.ttc NotoSansMonoCJK-subset.ttf

The input is the variable font collection from Noto CJK 2.004; its first
font is the JP one. Needs fontTools.
"""

import sys

from fontTools import subset
from fontTools.pens.cu2quPen import Cu2QuPen
from fontTools.pens.ttGlyphPen import TTGlyphPen
from fontTools.ttLib import TTCollection, newTable
from fontTools.varLib import instancer

FAMILY = "SB Emulator Mono CJK"

RANGES = [
    (0x20, 0x7E),      # ASCII
    (0xA0, 0x24F),     # Latin-1 and Latin Extended
    (0x370, 0x4FF),    # Greek, Cyrillic
    (0x2000, 0x206F),  # General Punctuation
    (0x2100, 0x23FF),  # Letterlike symbols, arrows, math, technical
    (0x2460, 0x27BF),  # Enclosed, box drawing, blocks, shapes, symbols
    (0x3000, 0x30FF),  # CJK symbols, kana
    (0x3130, 0x318F),  # Hangul compatibility jamo
    (0x3200, 0x33FF),  # Enclosed CJK, CJK compatibility
    (0xAC00, 0xD7A3),  # Hangul syllables
    (0xFF00, 0xFFEF),  # Full-width and half-width forms
]


def double_byte(codec, leads, trails):
    """Every character of a double-byte encoding."""
    chars = set()
    for lead in leads:
        for trail in trails:
            try:
                chars.add(ord(bytes([lead, trail]).decode(codec)))
            except UnicodeDecodeError:
                pass
    return chars


def codes():
    chars = set()
    for lo, hi in RANGES:
        chars.update(range(lo, hi + 1))
    # JIS X 0208 (Shift_JIS) and KS X 1001 (EUC-KR), kanji and hanja included.
    sjis_leads = list(range(0x81, 0xA0)) + list(range(0xE0, 0xF0))
    chars |= double_byte("shift_jis", sjis_leads, range(0x40, 0xFD))
    chars |= double_byte("euc_kr", range(0xA1, 0xFF), range(0xA1, 0xFF))
    return sorted(chars)


def to_truetype(font):
    """Replaces the cubic CFF2 outlines with quadratic TrueType ones."""
    glyphs = font.getGlyphSet()
    glyf = newTable("glyf")
    glyf.glyphOrder = font.getGlyphOrder()
    glyf.glyphs = {}
    for name in glyf.glyphOrder:
        pen = TTGlyphPen(None)
        # 3 units of 1000 is under 0.05 pixels at 16 pixels.
        glyphs[name].draw(Cu2QuPen(pen, max_err=3.0, reverse_direction=True))
        glyf.glyphs[name] = pen.glyph()
    del font["CFF2"]
    font["glyf"] = glyf
    font["loca"] = newTable("loca")
    font.sfntVersion = "\0\1\0\0"
    font["head"].glyphDataFormat = 0
    maxp = font["maxp"]
    maxp.tableVersion = 0x00010000
    for field in (
        "maxZones", "maxTwilightPoints", "maxStorage", "maxFunctionDefs",
        "maxInstructionDefs", "maxStackElements", "maxSizeOfInstructions",
        "maxComponentElements",
    ):
        setattr(maxp, field, 0)
    maxp.maxZones = 1
    post = font["post"]
    post.formatType = 3.0
    post.extraNames = []
    post.mapping = {}


def rename(font):
    names = font["name"]
    for record in list(names.names):
        if record.nameID in (1, 3, 4, 6, 16, 17, 21, 22, 25):
            names.removeNames(nameID=record.nameID)
    names.setName(FAMILY, 1, 3, 1, 0x409)
    names.setName("Regular", 2, 3, 1, 0x409)
    names.setName(FAMILY + " Regular; subset of Noto Sans Mono CJK JP 2.004", 3, 3, 1, 0x409)
    names.setName(FAMILY, 4, 3, 1, 0x409)
    names.setName(FAMILY.replace(" ", "") + "-Regular", 6, 3, 1, 0x409)


def main(src, dst):
    font = TTCollection(src).fonts[0]
    font = instancer.instantiateVariableFont(font, {"wght": 400})
    options = subset.Options()
    options.layout_features = []
    options.hinting = False
    options.notdef_outline = True
    options.name_IDs = ["*"]
    options.drop_tables += [
        "DSIG", "VORG", "vhea", "vmtx", "BASE", "STAT", "GSUB", "GPOS", "GDEF",
    ]
    subsetter = subset.Subsetter(options)
    subsetter.populate(unicodes=codes())
    subsetter.subset(font)
    to_truetype(font)
    rename(font)
    font.save(dst)


if __name__ == "__main__":
    main(sys.argv[1], sys.argv[2])
