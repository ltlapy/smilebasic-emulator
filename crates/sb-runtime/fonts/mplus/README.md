# SB Emulator 1p

Fallback 16-pixel console font, used when no SmileBASIC 4 font has been
imported. The 8-pixel font stays GalmuriMono7.

- Source: [M PLUS 1p](https://github.com/google/fonts/tree/main/ofl/mplus1p)
  Bold, `MPLUS1p-Bold.ttf`, by the [M+ FONTS project](https://mplusfonts.github.io/).
  SmileBASIC 4's 16-pixel kana and kanji come closest to this face.
- License: SIL Open Font License 1.1, see `LICENSE.txt`.
- Changes, made by `subset.py`: hinting, layout tables and vertical
  metrics removed; renamed, as the license asks of modified versions.

# M PLUS Code Latin 50 Bold

Half-width glyphs of the smooth font for the emulator's menus and windows
(`ui_font=smooth`, see `src/ui_glyphs.rs`).

- Source: `fonts/MPLUSCodeLatin/ttf/MPLUSCodeLatin50-Bold.ttf` from
  [MPLUS_FONTS](https://github.com/coz-m/MPLUS_FONTS), by the M+ FONTS
  project, unchanged.
- License: SIL Open Font License 1.1, see `LICENSE.txt`.
