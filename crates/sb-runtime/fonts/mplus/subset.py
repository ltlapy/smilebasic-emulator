"""Makes the bundled 16-pixel fallback font: M PLUS 1p Bold, the face
SmileBASIC 4's own 16-pixel font is drawn from, without hinting, layout
tables or vertical metrics, and renamed as the SIL OFL asks of modified
versions.

    python3 subset.py MPLUS1p-Bold.ttf SBEmulator1p-Bold.ttf

The input is MPLUS1p-Bold.ttf from Google Fonts (ofl/mplus1p). Needs
fontTools.
"""

import sys

from fontTools import subset
from fontTools.ttLib import TTFont

FAMILY = "SB Emulator 1p"


def rename(font):
    names = font["name"]
    for record in list(names.names):
        if record.nameID in (1, 2, 3, 4, 6, 16, 17, 21, 22, 25):
            names.removeNames(nameID=record.nameID)
    names.setName(FAMILY, 1, 3, 1, 0x409)
    names.setName("Bold", 2, 3, 1, 0x409)
    names.setName(FAMILY + " Bold; subset of M PLUS 1p Bold", 3, 3, 1, 0x409)
    names.setName(FAMILY + " Bold", 4, 3, 1, 0x409)
    names.setName(FAMILY.replace(" ", "") + "-Bold", 6, 3, 1, 0x409)


def main(src, dst):
    font = TTFont(src)
    options = subset.Options()
    options.layout_features = []
    options.hinting = False
    options.notdef_outline = True
    options.name_IDs = ["*"]
    options.drop_tables += ["DSIG", "vhea", "vmtx", "GSUB", "GPOS", "GDEF", "gasp"]
    subsetter = subset.Subsetter(options)
    subsetter.populate(unicodes=font.getBestCmap().keys())
    subsetter.subset(font)
    rename(font)
    font.save(dst)


if __name__ == "__main__":
    main(sys.argv[1], sys.argv[2])
