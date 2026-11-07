"""Makes the bundled emoji font: Noto Emoji at one weight, without hinting,
layout tables or vertical metrics, and renamed as the SIL OFL asks of
modified versions.

    python3 subset.py NotoEmoji[wght].ttf SBEmulatorEmoji.ttf

The input is NotoEmoji[wght].ttf from Google Fonts (ofl/notoemoji). The
editor draws one character per cell, so sequences (flags, skin tones, ZWJ)
go with the layout tables. Needs fontTools.
"""

import sys

from fontTools import subset
from fontTools.ttLib import TTFont
from fontTools.varLib import instancer

FAMILY = "SB Emulator Emoji"
# Heavy enough to stay legible at 8 and 16 pixels.
WEIGHT = 600


def rename(font):
    names = font["name"]
    for record in list(names.names):
        if record.nameID in (1, 2, 3, 4, 6, 16, 17, 21, 22, 25):
            names.removeNames(nameID=record.nameID)
    names.setName(FAMILY, 1, 3, 1, 0x409)
    names.setName("Regular", 2, 3, 1, 0x409)
    names.setName(FAMILY + "; subset of Noto Emoji", 3, 3, 1, 0x409)
    names.setName(FAMILY, 4, 3, 1, 0x409)
    names.setName(FAMILY.replace(" ", ""), 6, 3, 1, 0x409)


def main(src, dst):
    font = instancer.instantiateVariableFont(TTFont(src), {"wght": WEIGHT})
    options = subset.Options()
    options.layout_features = []
    options.hinting = False
    options.notdef_outline = True
    options.name_IDs = ["*"]
    options.drop_tables += ["DSIG", "vhea", "vmtx", "GSUB", "GPOS", "GDEF", "gasp", "STAT"]
    subsetter = subset.Subsetter(options)
    # ASCII (digits, # and *) is left to the other fonts.
    subsetter.populate(unicodes=[c for c in font.getBestCmap() if c > 0x7F])
    subsetter.subset(font)
    rename(font)
    font.save(dst)


if __name__ == "__main__":
    main(sys.argv[1], sys.argv[2])
