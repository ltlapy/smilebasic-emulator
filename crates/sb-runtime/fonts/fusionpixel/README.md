# Fusion Pixel 8px monospaced (zh_hans)

The editor's last 8-pixel source: glyphs for characters no other bundled
font has, mostly the Simplified Chinese characters of GB2312 and beyond.
The menus and windows use the same glyphs when they draw the 8-pixel fonts
doubled.

- Source: [Fusion Pixel Font](https://github.com/TakWolf/fusion-pixel-font)
  by TakWolf, release 2026.10.25,
  `fusion-pixel-font-8px-monospaced-bdf-v2026.10.25.zip`, file
  `fusion-pixel-8px-monospaced-zh_hans.bdf`, unchanged.
- License: SIL Open Font License 1.1, see `LICENSE.txt`. The font takes
  glyphs from other fonts; their licenses, as the release ships them, are
  in `LICENSES/` (Boutique Bitmap 7x7, Galmuri and Miseki Bitmap under the
  SIL OFL 1.1, Misaki under its author's license).
- The zh_hans build draws CJK characters shared with Japanese in their
  Simplified Chinese forms, so it comes after every other 8-pixel source.
- The build script (`build.rs`) places each glyph in an 8x8 cell at the
  size it was drawn. Only the cells of characters the 8-pixel sources
  before it lack go into the program: the editor never draws the others.
