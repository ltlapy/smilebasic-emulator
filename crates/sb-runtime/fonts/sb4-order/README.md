# SmileBASIC 4 font page order

`order.bin` gives each character code the glyph number SmileBASIC 4's
`font.map` gives it: the place of the code's glyph on the font page (GRP5).
Glyph `n` of the 8-pixel font is at `((n % 64) * 8, (n / 64) * 8)`, and of
the 16-pixel font at `((n % 64) * 16, 1024 + (n / 64) * 16)`. The fallback
font (`src/font.rs`) puts its own glyphs at these places, so programs that
draw their own font page for SmileBASIC 4 show the right characters without
an imported font.

- Source: `font.map` of SmileBASIC 4 version 4.4.x, from the user's own copy
  (imported by `smilebasic-import`).
- Contents: the order only, 4217 codes. No glyph pictures (those are in
  `GDEFFONT`, which stays with the user), and none of font.map's metrics or
  attribute bits. Codes that font.map sends to the missing glyph, number
  703, are left out; U+FFFD is one of them.
- Made by `tools/font_order.py`, which also prints the table as text
  (`--dump`) and describes the format: u16 words, one per code, holding the
  glyph number and the gap to the previous code.
