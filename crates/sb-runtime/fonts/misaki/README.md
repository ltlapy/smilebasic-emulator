# Misaki Gothic 2nd

The editor's 8-pixel glyphs for characters SmileBASIC's font and
GalmuriMono7 lack, such as Greek and Cyrillic, and the same glyphs in the
menus and windows when they draw the 8-pixel fonts doubled.

- Source: [Misaki font](https://littlelimit.net/misaki.htm) by Num Kadoma,
  BDF edition of 2021-05-05 (`misaki_bdf_2021-05-05.zip`), file
  `misaki_gothic_2nd.bdf`, unchanged.
- License: free to use, copy and redistribute, with or without changes,
  commercially or not, with no warranty. `LICENSE.txt` is the font's
  manual, `misaki.txt`, which holds the license in English and Japanese.
- The build script (`build.rs`) places each glyph in an 8x8 cell at the
  size it was drawn. Only the cells of characters GalmuriMono7 lacks go
  into the program: the editor never draws the others.
