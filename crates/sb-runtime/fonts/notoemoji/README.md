# SB Emulator Emoji

Monochrome emoji the editor draws for characters SmileBASIC's font lacks.

- Source: [Noto Emoji](https://github.com/google/fonts/tree/main/ofl/notoemoji),
  `NotoEmoji[wght].ttf` from Google Fonts, version 3.002.
- License: SIL Open Font License 1.1, see `LICENSE.txt`.
- Changes, made by `subset.py`: fixed at weight 600; control characters,
  hinting, layout tables and vertical metrics removed; renamed, as the license asks of
  modified versions. Without the layout tables, flags, skin tones and other
  sequences are gone.
- The build script (`build.rs`) draws each glyph into 8x8 and 16x16 cells.
  Only those cells go into the program.
