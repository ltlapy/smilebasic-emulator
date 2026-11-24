# Unifont JP

The second bundled 16-pixel dot font, after Izumi Gothic 16
(`fonts/izumi`): glyphs for everything else in Unifont JP's range, the
whole Basic Multilingual Plane plus the JIS X 0213 kanji of planes 2 and 3.
It also draws the 16-pixel half-width forms that NeoDunggeunmo lacks, such
as half-width katakana.

- Source: [GNU Unifont](https://unifoundry.com/unifont/) 18.0.01,
  `font-builds/unifont_jp-18.0.01.hex.gz`, unchanged (SHA-256
  `c9c3af339e1f86076c95e4d3c34806a0cf6f0d37a4e168d697a04c1ea948736b`; its
  `.sig` checks against Paul Hardy's key
  `95D2E9AB8740D8046387FD151A09227B1F435A33`). Its JIS
  kanji come from Izumi Gothic 16.
- License: the glyphs are dual-licensed; this project uses them under the
  SIL Open Font License 1.1, see `LICENSE.txt`.
- The build script (`build.rs`) reads the unchanged file and makes the
  full-width table's glyphs bold: each stroke grows one pixel toward the
  side the glyph leaves free, keeping one-pixel gaps open. A glyph drawn
  against column 0 with column 15 empty first moves one pixel right, so it
  sits in the same position as Izumi's kanji. The half-width forms stay
  unchanged (see `src/dot_bold.rs` and `BOLD_EDGE`).
