# GalmuriMono7

Fallback console font, used when no SmileBASIC 4 font has been imported.

- Source: [Galmuri](https://github.com/quiple/galmuri) v2.40.4,
  `dist/GalmuriMono7.bdf`, unchanged.
- License: SIL Open Font License 1.1, see `LICENSE.txt`.
- The build script (`build.rs`) places each glyph in an 8x8 cell and applies
  the changes in `overrides.txt`, which tell apart characters that look
  identical in the original (for example U+FF10 and U+FF2F).
