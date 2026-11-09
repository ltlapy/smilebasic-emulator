# SB Emulator Dot

16-pixel half-width glyphs for the emulator's menus and windows, and for the
editor when it draws half-width characters half a cell wide. Its glyphs are
drawn for 8x16 and 16x16 cells, so they stay sharp there.

- Source: [NeoDunggeunmo](https://github.com/neodgm/neodgm) v1.601,
  `neodgm.ttf`, by Dalgona, from Jungtae Kim's public domain Dunggeunmo.
- License: SIL Open Font License 1.1, see `LICENSE.txt`.
- Changes, made by `subset.py`: control characters, hinting and layout
  tables removed; renamed, as the license asks of modified versions. It
  keeps every other character.
- The build script (`build.rs`) compresses the file, and the emulator
  unpacks it on first use.
- The font has no half-width katakana. They come from Unifont JP
  (`fonts/unifont`).
