# Importing and exporting

`smilebasic-import` copies SmileBASIC 4's resources from your own copy into
the emulator's data directory. It can also write the instruments and preset
songs for use in other programs. It is a separate program, and the emulator
only reads what it has imported.

## Without importing

Without any setup, text uses bundled fonts under the SIL Open Font License
1.1: [GalmuriMono7](https://github.com/quiple/galmuri) at 8 pixels, and
[M PLUS 1p](https://mplusfonts.github.io/) Bold at 16 pixels. SmileBASIC 4's
16-pixel kana and kanji come closest to that face; its ASCII is SmileBASIC's
own drawing (slashed zero, broken bar), which no M+ font matches. The
16-pixel font follows "Bundled 16-pixel glyphs" (see below), so with the
pixel set it is drawn from the dot fonts instead. SmileBASIC's
own characters (number tiles, buttons, box drawing and other symbols at
U+E100-E17F and U+E200-E2FF) are redrawn for this emulator in
`crates/sb-runtime/fonts/symbols.txt` at 8 pixels, and drawn at twice that
size in the 16-pixel font.

The stand-in font's glyphs sit on the font page (GRP5) where SmileBASIC 4's
own font has them, so programs that draw their own font page work without
importing. For this the emulator carries the order of SmileBASIC 4's font
page, which glyph number each character code has, taken from its font.map
(`crates/sb-runtime/fonts/sb4-order`). It holds none of SmileBASIC 4's
glyphs or metrics.

Music plays with [GeneralUser GS](https://www.schristiancollins.com/generaluser)
by S. Christian Collins, bundled in `crates/sb-sdl/sounds`, until SmileBASIC's
own instruments are imported. File > Settings > Sound picks between them and
another SoundFont (`.sf2`).

## Characters the font lacks

SmileBASIC 4's font covers ASCII, Japanese and some symbols, and shows other
Unicode characters as a box. The editor draws those from another font
instead. It has three sources:

1. SmileBASIC's font, imported or the bundled stand-in;
2. the file set in File > Settings > Fonts > "Font file": a BDF, TTF or OTF
   font, read at startup and when chosen;
3. bundled fonts:
   - 8 pixels: GalmuriMono7, [Misaki Gothic 2nd](https://littlelimit.net/misaki.htm),
     M PLUS 1p Bold, then [Fusion Pixel](https://github.com/TakWolf/fusion-pixel-font)
     8px monospaced (zh_hans);
   - 16 pixels, one of two sets, picked by File > Settings > Fonts >
     "Bundled 16-pixel glyphs":
     - Antialiased (the default): M PLUS 1p Bold, then
       [Noto Sans SC](https://github.com/notofonts/noto-cjk) Bold, cut down
       to the GB2312 characters;
     - Pixel: [Izumi Gothic 16](http://izumilib.web.fc2.com/izumi-bf/dl-eng.html)
       (JIS X 0213), then [Unifont JP](https://unifoundry.com/unifont/) (the
       whole Basic Multilingual Plane, and the JIS X 0213 kanji above it).

     Each set ends with the other set's fonts, for the characters it lacks,
     so both draw the same characters: the antialiased set takes Hangul,
     most symbols and the rarer kanji from the dot fonts, and the pixel set
     takes the few characters above U+FFFF that only M PLUS 1p Bold has,
     such as double-struck letters.

   Izumi Gothic 16 and Unifont JP are dot fonts drawn in one-pixel
   strokes. The build makes them bold: each stroke grows one pixel to the
   side, but one-pixel gaps between strokes stay open, and so does the
   side column of the cell where the glyph leaves it empty, so neighbors
   do not touch.

   Misaki Gothic 2nd is free to use, copy and change under its author's
   license, and Izumi Gothic 16 is equivalent to public domain by its
   author's declaration; the others are under the SIL Open Font License
   1.1. Each font's folder in `crates/sb-runtime/fonts` holds its license.

   Fusion Pixel and Noto Sans SC draw the CJK characters they share with
   Japanese in their Simplified Chinese forms, so they come after the
   Japanese fonts: they only fill in characters the fonts before them lack,
   and kana and Japanese kanji keep the glyphs of GalmuriMono7, M PLUS 1p
   Bold and Izumi Gothic 16. Unifont JP draws every GB2312 character, so in
   the pixel set Noto Sans SC adds none.

The editor looks in them in this order. "Editor's first font" on the same page
moves one source to the front; the other two keep their order. With the file
first, the order is file, SmileBASIC, bundled; with the bundled fonts first, it
is bundled, SmileBASIC, file. A character none of them has keeps the box. The
order covers the code, line numbers, ruler, status line, suggestions and hints.
SmileBASIC's own symbols (U+E000-U+F8FF) always come from SmileBASIC's font.

Each glyph takes one cell. SmileBASIC draws ASCII as wide as kana, so for
ASCII (U+0021-U+007E) the editor takes the font's full-width form
(U+FF01-U+FF5E) when the font has one: those forms are drawn for a square
cell. A glyph with no full-width form keeps its own width, centered in the
cell. No glyph is stretched or made larger:

- A bitmap glyph keeps the size it was drawn at. An 8-pixel font in a
  16-pixel cell stays 8 pixels, centered; a glyph larger than the cell is
  made smaller to fit.
- An outline font is drawn at the cell's size: its em fills the cell, or 15
  of 16 pixels for M PLUS 1p Bold, as SmileBASIC 4 draws that face. At 8
  pixels a pixel is on or off; at 16 pixels the edges keep their gray, as on
  SmileBASIC 4's 16-pixel font page.

No bundled font is an emoji font. At 16 pixels the emoji-range characters of
the Basic Multilingual Plane (such as ☀, ★ and ✅) come from M PLUS 1p Bold
or Unifont JP, in one color; those above U+FFFF keep the box, as do most
emoji at 8 pixels. A font file can supply them.

If the file cannot be read, the editor goes back to the bundled fonts, clears
the setting and says why. In `settings.txt` the settings are
`editor_extra_font=PATH` (empty for none),
`editor_font_first=smilebasic`, `file` or `bundled`, and
`bundled_glyphs16=antialiased` or `pixel`. The set takes effect when chosen,
everywhere the bundled 16-pixel glyphs are drawn: the editor, the menus and
windows, the stand-in font and the programs' font.

### Programs' font

By default programs see SmileBASIC's font alone. "Programs' 8-pixel font" and
"Programs' 16-pixel font" on the same page can put the file's or the bundled
fonts' glyphs on the font page (GRP5) instead, for the characters SmileBASIC's
font has a place for. Programs then draw them with `PRINT`, `GPUTCHR` and
everything else that reads the page. Characters with no place on the page
keep the box. In `settings.txt` these are `program_font8` and
`program_font16`, each `smilebasic`, `file` or `bundled`. See
[differences.md](differences.md#rendering-and-gpu) for what a program can
notice.

## Half-width characters

The emulator's own menus, windows and dialogs draw ASCII and half-width kana
half a cell wide. The editor does too when File > Settings > Editor > "Narrow
half-width characters" is on (`editor_half_width=true` in `settings.txt`). The
console, DIRECT mode, the help's text and the debugger's panel keep one
square cell per character.

SmileBASIC 4's ASCII fills its square cell, so it cannot be cut in half. The
half-width glyphs come from:

- 8 pixels: GalmuriMono7, drawn for 4x8 cells;
- 16 pixels: NeoDunggeunmo, drawn for 8x16 cells
  (`crates/sb-runtime/fonts/neodgm`), then Unifont JP for what it lacks,
  such as half-width kana.

In the editor, the font file comes before these when it draws a character
half-width (a BDF glyph at most half as wide as the font's full-width ones,
or an outline glyph at most 0.6 em wide), unless "Editor's first font" puts
the bundled fonts first. SmileBASIC's own glyph squeezed to half its width is
the last resort. The menus and windows use the bundled glyphs.

## Font of the menus and windows

File > Settings > Fonts > "Menus and windows" picks the font of the menu bar,
the settings window and the emulator's other windows:

- 16-pixel (the default): SmileBASIC's 16-pixel font with its gray edges,
  as SmileBASIC 4 draws it, and the half-width glyphs above. With an
  imported font, Unicode characters it lacks come from the editor's bundled
  16-pixel glyphs, in the set "Bundled 16-pixel glyphs" picks, instead of
  the box. A character beyond U+FFFF takes two cells, with its
  glyph on the first, as in the editor;
- Smooth: outlines with soft edges, drawn for a 16-pixel cell. Half-width
  characters come from [M PLUS Code Latin](https://github.com/coz-m/MPLUS_FONTS)
  50 Bold, a monospaced Latin face half an em wide, then M PLUS 1p Bold's
  half-width forms. Other characters come from the editor's bundled 16-pixel
  fonts. SmileBASIC's own symbols and what none of them has come from
  SmileBASIC's font.

In `settings.txt` this is `ui_font=16` or `smooth`. Earlier versions also
had `ui_font=8`, the 8-pixel fonts doubled; it now loads as `16`. Text the
emulator draws at an odd scale, such as the debugger's small panel text,
keeps the 8-pixel font, in solid pixels. With an imported font, the
characters it lacks there come from the editor's bundled 8-pixel fonts,
drawn with their gray as in the editor.

The half-width characters are U+0020-U+007E, U+FF61-U+FF9F, U+FFE8, U+FFED,
U+FFEE and U+20A9. Latin-1 and the rest stay full-width, as in SmileBASIC 4.

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

Each file is written in full under a temporary name, then takes its place.
A link in the output folder is replaced by the file, and the file it
pointed to is left as it was.

Without the ExeFS, sprite definitions are a plain grid of 16x16 cells. The first
release's ExeFS has only definitions 0-4095; the others are empty.

A sound archive imported by an older version of the importer kept one channel
of each waveform, so its stereo waveforms, such as `BEEP 152`'s, play in mono.
Import again to get both channels.

It also copies SmileBASIC 4's own projects `#SYS`, `#TOOL`, `#EXAMPLE` and
`#GUIDE` into the projects folder, as plain files like the project manager's
Import. It never replaces a file already there, and leaves out a file that
has a link in its place.

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
