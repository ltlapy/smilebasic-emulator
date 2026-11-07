# SB Emulator SC Bold

The editor's last 16-pixel source: glyphs for GB2312 characters that
M PLUS 1p Bold, Galmuri14 and Noto Emoji lack, mostly Simplified Chinese.
The smooth font of the menus and windows uses the same glyphs.

- Source: [Noto Sans CJK](https://github.com/notofonts/noto-cjk) tag
  Sans2.004, `Sans/SubsetOTF/SC/NotoSansSC-Bold.otf`.
  © 2014-2021 Adobe (http://www.adobe.com/).
- License: SIL Open Font License 1.1, see `LICENSE.txt`.
- Changes, made by `subset.py`: cut down to the GB2312 character set
  (7445 characters plus ASCII); hinting, layout tables and vertical
  metrics removed; renamed, as the license asks of modified versions.
- It draws CJK characters shared with Japanese in their Simplified Chinese
  forms, so it comes after M PLUS 1p Bold, whose Japanese forms win.
- The build script (`build.rs`) draws each glyph into a 16x16 cell, as
  the M PLUS 1p Bold glyphs are drawn (an em of 15 pixels), in 16 steps of
  gray. Only the cells of characters the fonts before it lack go into the
  program: the editor never draws the others.
