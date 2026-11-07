"""Makes the bundled 16-pixel font for the GB2312 characters: Noto Sans SC
Bold cut down to the GB2312 character set, without hinting, layout tables
or vertical metrics, and renamed as the SIL OFL asks of modified versions.

    python3 subset.py NotoSansSC-Bold.otf SBEmulatorSC-Bold.otf

The input is Sans/SubsetOTF/SC/NotoSansSC-Bold.otf from the noto-cjk
repository (tag Sans2.004). Needs fontTools.
"""

import sys

from fontTools import subset
from fontTools.ttLib import TTFont

FAMILY = "SB Emulator SC"


def gb2312():
    """Every character of GB2312: ASCII and the 7445 two-byte characters."""
    codes = set(range(0x20, 0x7F))
    for high in range(0xA1, 0xF8):
        for low in range(0xA1, 0xFF):
            try:
                codes.add(ord(bytes([high, low]).decode("gb2312")))
            except UnicodeDecodeError:
                pass
    return codes


def rename(font):
    names = font["name"]
    for record in list(names.names):
        if record.nameID in (1, 2, 3, 4, 6, 16, 17, 21, 22, 25):
            names.removeNames(nameID=record.nameID)
    names.setName(FAMILY, 1, 3, 1, 0x409)
    names.setName("Bold", 2, 3, 1, 0x409)
    names.setName(FAMILY + " Bold; subset of Noto Sans SC Bold", 3, 3, 1, 0x409)
    names.setName(FAMILY + " Bold", 4, 3, 1, 0x409)
    names.setName(FAMILY.replace(" ", "") + "-Bold", 6, 3, 1, 0x409)


def main(src, dst):
    font = TTFont(src)
    options = subset.Options()
    options.layout_features = []
    options.hinting = False
    options.desubroutinize = False
    options.notdef_outline = True
    options.name_IDs = ["*"]
    options.drop_tables += ["DSIG", "vhea", "vmtx", "VORG", "GSUB", "GPOS", "GDEF", "BASE", "gasp"]
    subsetter = subset.Subsetter(options)
    subsetter.populate(unicodes=sorted(gb2312() & set(font.getBestCmap())))
    subsetter.subset(font)
    rename(font)
    font.save(dst)


if __name__ == "__main__":
    main(sys.argv[1], sys.argv[2])
