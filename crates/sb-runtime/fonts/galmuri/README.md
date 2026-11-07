# Galmuri

- License: SIL Open Font License 1.1, see `LICENSE.txt`.
- Source: [Galmuri](https://github.com/quiple/galmuri) v2.40.4, files from
  `dist/`, unchanged.

## GalmuriMono7

Fallback console font, used when no SmileBASIC 4 font has been imported,
and the editor's 8-pixel glyphs for characters SmileBASIC's font lacks.

- File: `GalmuriMono7.bdf`.
- The build script (`build.rs`) places each glyph in an 8x8 cell and applies
  the changes in `overrides.txt`, which tell apart characters that look
  identical in the original (for example U+FF10 and U+FF2F).

## Galmuri14

The editor's 16-pixel glyphs for characters that SmileBASIC's font and
M PLUS 1p Bold lack, the fallback font's 16-pixel glyphs for the same, and
the 16-pixel half-width glyphs NeoDunggeunmo lacks.

- File: `Galmuri14.bdf`, with every character it draws.
- The build script centers each glyph in a 16x16 cell at the size it was
  drawn, and its half-width glyphs in 8x16 cells.
