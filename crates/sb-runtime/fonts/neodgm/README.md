# SB Emulator Half

16-pixel half-width glyphs for the emulator's menus and windows, and for the
editor when it draws half-width characters half a cell wide. Its ASCII is
drawn for 8x16 cells, so it stays sharp there.

- Source: [NeoDunggeunmo](https://github.com/neodgm/neodgm) v1.601,
  `neodgm.ttf`, by Dalgona, from Jungtae Kim's public domain Dunggeunmo.
- License: SIL Open Font License 1.1, see `LICENSE.txt`.
- Changes, made by `subset.py`: only ASCII, the won sign and the four
  half-width punctuation marks it has are kept; hinting and layout tables
  removed; renamed, as the license asks of modified versions.
- Half-width katakana are not in the font. They come from GalmuriMono7,
  doubled.
