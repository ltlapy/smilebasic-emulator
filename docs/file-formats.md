# SmileBASIC 4 files

How SmileBASIC 4.4.9 stores projects and files, worked out from files it
saved and from its own projects. The emulator reads all of these, and
writes them when exporting; its everyday files are simpler (see the end).

## Projects and file names

Each project is a folder. SmileBASIC 4 names the workspace's folder
`###`; the emulator uses `@WORKSPACE`. A file's name is a one-letter
type prefix and the name the program gives:

| Type | Prefix | Example | Backup when overwritten |
|---|---|---|---|
| TXT | `T` | `TMAIN.PRG` for `MAIN.PRG` | `@BACKUP.PRG` (SAVE), `@BACKUP.TXT` (SAVEV) |
| DAT | `B` | `BMAP` | `@BACKUP.DAT` |
| GRP | `G` | `GSPRITES` | `@BACKUP.GRP` |
| JPG | `J` | `JPHOTO` | `@BACKUP.JPG` |

A project also has a `META` file with its title, description and icon.
A new project starts with `META` and a `MAIN.PRG`.

## The container

Every file but a few PNG images in SmileBASIC 4's own projects starts
with this header. Numbers are little-endian.

| Offset | Size | Field |
|---|---|---|
| 0x00 | 2 | format version, 4 |
| 0x02 | 2 | kind: 0 TXT, 1 DAT, 2 GRP, 4 META, 5 JPG |
| 0x04 | 2 | 1 if the body is zlib-compressed, else 0 |
| 0x06 | 2 | a number that depends on the kind (below) |
| 0x08 | 4 | size of the body as stored |
| 0x0C | 2 | year saved |
| 0x0E | 5 | month, day, hour, minute, second |
| 0x13 | 1 | 0xFF |
| 0x14 | 0x5C | zero in files saved by programs; author names in SmileBASIC 4's own |
| 0x70 | size | body |
| end | 20 | signature, except in META |

The signature is the HMAC-SHA1 of the header and body, with the key that
SmileBASIC 3 used too (`SIGNING_KEY` in `crates/sb-runtime/src/sbfile.rs`).
SmileBASIC 4 loads files signed this way; whether it refuses unsigned
ones has not been tried.

What SmileBASIC 4.4.9 writes:

| File | Kind | Compressed | 0x06 | Body |
|---|---|---|---|---|
| Program (SAVE) | 0 | no | 2 | UTF-8, lines ending in LF |
| Text (SAVEV) | 0 | yes | 0 | UTF-8 |
| DAT | 1 | yes | 1 | `PCBN` array |
| GRP | 2 | yes | 3 | `PCBN` array of pixels |
| META | 4 | no | 0 | `PCPM` metadata, no signature |
| JPG | 5 | no | 3 | a JPEG file |

SmileBASIC 4's own projects are older: programs have 1 at 0x06, and
their GRP files are kind 1 (DAT) with 2 at 0x06. Some of their GRP files
are plain PNG images with no container.

## `PCBN` arrays

DAT files and GRP images hold one array:

| Offset | Size | Field |
|---|---|---|
| 0x00 | 8 | `PCBN0004` |
| 0x08 | 2 | element type: 4 integer, 5 real, 6 string |
| 0x0A | 2 | number of dimensions, 1 to 4 |
| 0x0C | 16 | four u32 sizes; unused ones may hold anything |
| 0x1C | | the elements |

- Integers are i32 and reals f64. Pixels are integers in `&HAARRGGBB`.
- Each string is its length in UTF-8 bytes, then its UTF-8 text. A
  length under 128 takes one byte; a longer one takes two, big-endian
  with the top bit set (300 is `81 2C`).
- In a DAT file the sizes are as `DIM` gave them, outermost first:
  `DIM A%[2,3]` stores 2, then 3. The elements are in the order
  `A%[0,0]`, `A%[0,1]`, and so on.
- A GRP image stores its width first, then its height. Its pixels go row
  by row from the top left. `LOADV "GRP:..."` turns it into an array of
  `[height, width]`.

## META

The container's kind is 4, it is not compressed and it has no signature.
The body is 0x2B20 bytes:

| Offset | Size | Field |
|---|---|---|
| 0x0000 | 8 | `PCPM0005` |
| 0x0008 | 48 | title, UTF-16, up to 24 characters, zero-padded |
| 0x0038 | | description, UTF-16, zero-padded; lines end in LF |
| 0x1218 | 4 | the icon's size, 40 |
| 0x121C | 6400 | the icon, 40x40 pixels in `&HAARRGGBB`, row by row |
| 0x2B1C | 4 | zero |

The bytes between the description and 0x1218 were all zero in the files
seen. A description saved on SmileBASIC 4 had LF added to make ten
lines. A new project's title is its folder's name; the workspace's is
`[WORKSPACE]` (in the Japanese version, `[ワークスペース]`).

## The emulator's own files

For files that can be opened on a computer, the emulator saves:

- programs and text as plain UTF-8, without a container;
- GRP files as PNG images, and JPG files as plain JPEG;
- DAT files as the bare `PCBN` array, without a container;
- metadata in `@META.TXT` (the title, a line break, then the
  description) and `@ICON.PNG`.

It reads both these and SmileBASIC 4's files.
