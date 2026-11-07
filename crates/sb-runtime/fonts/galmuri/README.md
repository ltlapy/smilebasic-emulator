# Galmuri

- License: SIL Open Font License 1.1, see `LICENSE.txt`.

## GalmuriMono7

Fallback console font, used when no SmileBASIC 4 font has been imported,
and the editor's 8-pixel glyphs for characters SmileBASIC's font lacks.

- Source: [Galmuri](https://github.com/quiple/galmuri) v2.40.4,
  `dist/GalmuriMono7.bdf`, unchanged.
- The build script (`build.rs`) places each glyph in an 8x8 cell and applies
  the changes in `overrides.txt`, which tell apart characters that look
  identical in the original (for example U+FF10 and U+FF2F).

## Galmuri14-Hangul

The editor's 16-pixel Hangul, for SmileBASIC's font, which has none.

- Source: Galmuri v2.40.4, `Galmuri14.bdf`, with only the Hangul blocks
  kept by `subset14.py`. The glyphs are unchanged.
- The build script centers each glyph in a 16x16 cell.
