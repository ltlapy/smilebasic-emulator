# SB Emulator SC Bold

The second font of the antialiased set of 16-pixel glyphs: glyphs for the
GB2312 characters M PLUS 1p Bold lacks, for the editor, the menus and
windows, and the fallback font. In the pixel set it comes last, and adds
nothing, because Unifont JP draws every GB2312 character.

- Source: [Noto Sans CJK](https://github.com/notofonts/noto-cjk) tag
  Sans2.004, `Sans/SubsetOTF/SC/NotoSansSC-Bold.otf`.
  © 2014-2021 Adobe (http://www.adobe.com/).
- License: SIL Open Font License 1.1, see `LICENSE.txt`.
- Changes, made by `subset.py`: cut down to the GB2312 character set
  (7445 characters plus ASCII); hinting, layout tables and vertical
  metrics removed; renamed, as the license asks of modified versions.
- It draws CJK characters shared with Japanese in their Simplified Chinese
  forms, so it comes after M PLUS 1p Bold, whose Japanese forms win.
- The build script (`build.rs`) draws each glyph into a 16x16 cell, the
  same way as the M PLUS 1p Bold glyphs (an em of 15 pixels), in 16 steps
  of gray. Only the cells of characters M PLUS 1p Bold lacks go into the
  program: the others are never drawn.
