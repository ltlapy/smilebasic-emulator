"""Makes the bundled half-width font: NeoDunggeunmo's ASCII and the few
other half-width characters it has, without hinting or layout tables, and
renamed as the SIL OFL asks of modified versions.

    python3 subset.py neodgm.ttf SBEmulatorHalf.ttf

The input is neodgm.ttf from the NeoDunggeunmo release. Needs fontTools.
"""

import sys

from fontTools import subset
from fontTools.ttLib import TTFont

FAMILY = "SB Emulator Half"
# ASCII, half-width katakana and forms, and the won sign: the characters
# the emulator draws half a cell wide.
WANTED = [*range(0x20, 0x7F), *range(0xFF61, 0xFFA0), 0xFFE8, 0xFFED, 0xFFEE, 0x20A9]


def rename(font):
    names = font["name"]
    for record in list(names.names):
        if record.nameID in (1, 2, 3, 4, 6, 16, 17, 21, 22, 25):
            names.removeNames(nameID=record.nameID)
    names.setName(FAMILY, 1, 3, 1, 0x409)
    names.setName("Regular", 2, 3, 1, 0x409)
    names.setName(FAMILY + "; subset of NeoDunggeunmo", 3, 3, 1, 0x409)
    names.setName(FAMILY, 4, 3, 1, 0x409)
    names.setName(FAMILY.replace(" ", ""), 6, 3, 1, 0x409)


def main(src, dst):
    font = TTFont(src)
    options = subset.Options()
    options.layout_features = []
    options.hinting = False
    options.notdef_outline = True
    options.name_IDs = ["*"]
    options.drop_tables += ["DSIG", "vhea", "vmtx", "GSUB", "GPOS", "GDEF", "gasp", "STAT"]
    subsetter = subset.Subsetter(options)
    subsetter.populate(unicodes=[c for c in WANTED if c in font.getBestCmap()])
    subsetter.subset(font)
    rename(font)
    font.save(dst)


if __name__ == "__main__":
    main(sys.argv[1], sys.argv[2])
