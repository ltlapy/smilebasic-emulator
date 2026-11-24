# Izumi Gothic 16

The first of the bundled 16-pixel dot fonts: full-width glyphs for the
editor, the menus and windows, and the fallback font. It comes first in
the pixel set of 16-pixel glyphs, and in the antialiased set after M PLUS 1p
Bold and Noto Sans SC Bold, for the characters they lack. It covers JIS
X 0213:2004, so it adds the kanji of JIS levels 3 and 4 and many symbols.
Unifont JP (`fonts/unifont`) comes after it.

- Source: [Izumi Bitmap Font Series](http://izumilib.web.fc2.com/izumi-bf/dl-eng.html)
  by Yu Izumi, Izumi Gothic 16 final beta (beta 6, 2007-03-01),
  `izmg16.beta6.bdf.tar.gz`, files `izmg16-2004-1.bdf` and
  `izmg16-2004-2.bdf` (JIS X 0213 planes 1 and 2). The same file is in the
  Internet Archive's copy of that page.
- License: the author declares it equivalent to public domain; anyone may
  use, change and redistribute it, commercially or not, without credit.
  `LICENSE.txt` holds the author's texts. The author asks, without
  requiring it, to be told about changes to the unfinished font.
- File: `izumi16.hex`, made by `convert.py`. The BDF files number their
  glyphs by JIS code, and this one by Unicode, in GNU Unifont's .hex
  format. The glyphs are unchanged. The 25 JIS characters that Unicode
  writes as two code points, such as kana with a semi-voiced mark, are
  excluded, because a cell table has no place for them. The half-width set
  (`izumi-uv16.bdf`, JIS X 0201 with `\` as the yen sign) is not used.
- The build script (`build.rs`) makes the glyphs bold with the rule of
  Izumi Gothic 16 Bold, which grows each stroke one pixel to the left
  while keeping one-pixel gaps open. Where the regular glyph leaves
  column 0 of the cell empty, the bold glyph does too, so neighbors do not
  touch; a stroke in column 1 grows into column 2 instead (see
  `src/dot_bold.rs` and `BOLD_EDGE`).
