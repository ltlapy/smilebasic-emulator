# Importing and exporting

`smilebasic-import` copies SmileBASIC 4's resources from your own copy into
the emulator's data directory. It can also write the instruments and preset
songs for use in other programs. It is a separate program, and the emulator
only reads what it has imported.

## Without importing

Without any setup, text uses bundled fonts under the SIL Open Font License
1.1: [GalmuriMono7](https://github.com/quiple/galmuri) at 8 pixels, and
[M PLUS 1p](https://mplusfonts.github.io/) Bold, the face SmileBASIC 4's own
font is drawn from, at 16 pixels. SmileBASIC's own characters (number tiles,
buttons, box drawing and other symbols at U+E100-E17F and U+E200-E2FF) are
redrawn for this emulator in `crates/sb-runtime/fonts/symbols.txt`.

Music plays with [GeneralUser GS](https://www.schristiancollins.com/generaluser)
by S. Christian Collins, bundled in `crates/sb-sdl/sounds`, until SmileBASIC's
own instruments are imported. File > Settings > Sound picks between them and
another SoundFont (`.sf2`).

## Characters the font lacks

SmileBASIC 4's font has no Hangul or emoji and shows them as a box. The
editor draws such characters from another font instead:

1. the file set in File > Settings > Editor > "Font for characters SmileBASIC
   lacks": a BDF, TTF or OTF font, read at startup and when chosen;
2. bundled fonts, all under the SIL Open Font License 1.1:
   - 8 pixels: GalmuriMono7, then [Noto Emoji](https://fonts.google.com/noto/specimen/Noto+Emoji),
     then M PLUS 1p Bold;
   - 16 pixels: Galmuri14's Hangul, M PLUS 1p Bold, Noto Emoji, then
     GalmuriMono7 doubled.

A character none of them has keeps the box. Each glyph is fitted to one cell:
bitmap fonts are scaled by whole steps, or down when larger than the cell,
and narrow glyphs are drawn twice as wide, as SmileBASIC draws Latin. An
outline font's em fills the cell. Emoji are monochrome.

If the file cannot be read, the editor goes back to the bundled fonts, clears
the setting and says why. In `settings.txt` the setting is
`editor_extra_font=PATH`; leave it empty for the bundled fonts.

Running programs never see these glyphs: see
[differences.md](differences.md#editor).

## Half-width characters

The emulator's own menus, windows and dialogs draw ASCII and half-width kana
half a cell wide. The editor does too when File > Settings > Editor > "Narrow
half-width characters" is on (`editor_half_width=true` in `settings.txt`). The
console, direct mode, the help's text and the debugger's panel keep one
square cell per character.

SmileBASIC 4's ASCII fills its square cell, so it cannot be cut in half. The
half-width glyphs come from:

- 8 pixels: GalmuriMono7, drawn for 4x8 cells;
- 16 pixels: the font's own glyph squeezed to half its width, which keeps
  its look. Characters the font lacks, such as half-width kana, take
  GalmuriMono7 doubled.

The half-width characters are U+0020-U+007E, U+FF61-U+FF9F, U+FFE8, U+FFED,
U+FFEE and U+20A9. Latin-1 and the rest stay full-width, as in SmileBASIC 4.
The editor's font file from the section above is not used for them.

## Importing

Extract the RomFS and ExeFS of the game, and of its update if you have one.
Then run:

```sh
smilebasic-import BASE_ROMFS BASE_EXEFS UPDATE_ROMFS UPDATE_EXEFS
```

List the update's directories last so its files take precedence. Only the font
is required.

The importer copies these into the emulator's data directory:

- `font.map` and `GDEFFONT`
- the default sprite image `GDEFGRP`
- the English help (`HelpData.idx`, `HelpData.dat`)

It takes these out of the game's files:

- the default sprite definitions (SPDEF), from the executable `main`
- the instruments, preset songs and sound effects, from the sound archive
  `petc4.bfsar`

Without the ExeFS, sprite definitions are a plain grid of 16x16 cells. The first
release's ExeFS has only definitions 0-4095; the others are empty.

It also copies SmileBASIC 4's own projects `#SYS`, `#TOOL`, `#EXAMPLE` and
`#GUIDE` into the projects folder, as plain files like the project manager's
Import. It never replaces a file already there.

| Option | Effect |
|---|---|
| `-o OUT` | Import into OUT instead of the emulator's data directory |
| `--projects DIR` | Copy the projects into DIR instead of the projects folder |

## Exporting instruments and songs

| Option | Effect |
|---|---|
| `--sf2 FILE` | Also write the instruments as a SoundFont, to play them in other programs |
| `--midi DIR` | Also write the preset songs into DIR as MIDI files |
| `--gm` | With `--sf2` and `--midi`, number instruments as General MIDI and Roland GS do |

Each MIDI file runs up to where the song has gone round its loop once. Its pitch
bend range is 24 semitones.

### Banks

| Instruments | Without `--gm` | With `--gm` |
|---|---|---|
| `@0`-`@127` | bank 0 | bank 0 |
| Drum sets `@128`-`@148` | bank 128, programs 0-20 | bank 128, one GS kit program each (the nearest by name where free) |
| The rest | bank `@/128`, program `@%128` | the same banks |

MIDI files choose instruments by the same banks. Without `--gm` they use bank
select MSB and LSB.

With `--gm`, the songs put drums on channel 10 and start with GM System On and
GS Reset. Instruments from `@149`, such as the tones of `@256` and up that many
songs use, have no GM sound and keep the banks above, selected by the MSB alone.
Without the SoundFont, a player plays them as GM instruments.
