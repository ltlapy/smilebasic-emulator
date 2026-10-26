# SB Emulator Mono CJK

Fallback 16-pixel console font, used when no SmileBASIC 4 font has been
imported. The 8-pixel font stays GalmuriMono7.

- Source: [Noto Sans Mono CJK JP](https://github.com/notofonts/noto-cjk)
  2.004, the first font of `NotoSansMonoCJK-VF.ttc`.
- License: SIL Open Font License 1.1, see `LICENSE.txt`. Noto is a
  trademark of Google Inc.
- Changes, made by `subset.py`: fixed at the Regular weight; cut down to
  ASCII, Latin, Greek, Cyrillic, common symbols, kana, Hangul, and the
  characters of JIS X 0208 and KS X 1001; outlines turned into TrueType
  curves; renamed, as the license asks of modified versions.
