//! Converts the fallback font into 8x8 console cells.
//!
//! - `$OUT_DIR/fallback_font.bin`, from the GalmuriMono7 BDF: one 10-byte
//!   record per character, sorted by code: u16 code (little-endian), then
//!   8 row bytes with the leftmost pixel in bit 7.
//! - `$OUT_DIR/fallback_symbols.bin`, from `fonts/symbols.txt`: one 66-byte
//!   record per character: u16 code, then 64 palette indices in rows (see
//!   `SYMBOL_PALETTE` in `src/font.rs`).
//! - `$OUT_DIR/extra_hangul16.bin` and `extra_emoji{8,16}.bin`, the
//!   editor's extra glyphs (see `src/extra_glyphs.rs`), from
//!   `fonts/galmuri/Galmuri14-Hangul.bdf` and
//!   `fonts/notoemoji/SBEmulatorEmoji.ttf`: tables compressed with
//!   deflate, laid out as `extra_glyphs::Table` reads them.

use std::collections::BTreeMap;
use std::path::Path;

// The runtime uses parts of it the build does not.
#[allow(dead_code)]
#[path = "src/cell_glyph.rs"]
mod cell_glyph;

const BDF: &str = "fonts/galmuri/GalmuriMono7.bdf";
const OVERRIDES: &str = "fonts/galmuri/overrides.txt";
const SYMBOLS: &str = "fonts/symbols.txt";
const HANGUL: &str = "fonts/galmuri/Galmuri14-Hangul.bdf";
const EMOJI: &str = "fonts/notoemoji/SBEmulatorEmoji.ttf";
/// Pixel characters of `symbols.txt`, in palette order.
const SYMBOL_CHARS: &str = ".#kgb";
/// Cell row of the baseline's lowest pixel row (y = 0); y = -1 is row 7.
const BASELINE_ROW: i32 = 6;

type Cell = [u8; 8];

struct Glyph {
    code: u32,
    advance: i32,
    bbx: [i32; 4],
    rows: Vec<u32>,
}

fn parse_bdf(text: &str) -> Vec<Glyph> {
    let mut glyphs = Vec::new();
    let (mut code, mut advance, mut bbx) = (0, 0, [0; 4]);
    let mut bitmap: Option<Vec<u32>> = None;
    for line in text.lines() {
        let mut words = line.split_whitespace();
        let Some(key) = words.next() else { continue };
        let mut num = || {
            words
                .next()
                .and_then(|w| w.parse::<i32>().ok())
                .unwrap_or(0)
        };
        match key {
            "ENCODING" => code = num() as u32,
            "DWIDTH" => advance = num(),
            "BBX" => bbx = [num(), num(), num(), num()],
            "BITMAP" => bitmap = Some(Vec::new()),
            "ENDCHAR" => {
                let rows = bitmap.take().expect("BITMAP before ENDCHAR");
                glyphs.push(Glyph {
                    code,
                    advance,
                    bbx,
                    rows,
                });
            }
            hex => {
                if let Some(rows) = &mut bitmap {
                    rows.push(u32::from_str_radix(hex, 16).expect("hex bitmap row"));
                }
            }
        }
    }
    glyphs
}

/// Places a glyph in a cell: half-width glyphs are centered, and glyphs
/// that poke out above or below are moved inside when they fit.
fn place(g: &Glyph) -> Cell {
    let [w, h, xoff, yoff] = g.bbx;
    let left = if g.advance <= 4 { 2 } else { 0 };
    let first_row = BASELINE_ROW - (yoff + h - 1);
    let last_row = BASELINE_ROW - yoff;
    let shift = if h > 8 {
        0
    } else if first_row < 0 {
        -first_row
    } else if last_row > 7 {
        7 - last_row
    } else {
        0
    };
    let row_bits = (w + 7) / 8 * 8;
    let mut cell = [0u8; 8];
    for (i, &bits) in g.rows.iter().enumerate() {
        let row = first_row + i as i32 + shift;
        if !(0..8).contains(&row) {
            continue;
        }
        for c in 0..w {
            let x = left + xoff + c;
            if (0..8).contains(&x) && bits >> (row_bits - 1 - c) & 1 == 1 {
                cell[row as usize] |= 0x80 >> x;
            }
        }
    }
    cell
}

/// Reads entries of a "U+XXXX" line followed by 8 rows of 8 pixel
/// characters, each mapped to its index in `chars`.
fn parse_art(path: &str, text: &str, chars: &str) -> Vec<(u32, [u8; 64])> {
    let mut out = Vec::new();
    // Comments are only allowed before the first entry, since pixel rows
    // may start with '#' too.
    let mut lines = text
        .lines()
        .skip_while(|l| !l.starts_with("U+"))
        .filter(|l| !l.is_empty());
    while let Some(head) = lines.next() {
        let hex = head
            .strip_prefix("U+")
            .and_then(|h| h.split_whitespace().next())
            .unwrap_or_else(|| panic!("{path}: expected U+XXXX, found {head:?}"));
        let code = u32::from_str_radix(hex, 16).expect("hex code");
        let mut pixels = [0u8; 64];
        for y in 0..8 {
            let line = lines.next().expect("8 rows per entry");
            assert_eq!(
                line.chars().count(),
                8,
                "{path}: U+{code:04X} rows are 8 pixels"
            );
            for (x, ch) in line.chars().enumerate() {
                let i = chars
                    .find(ch)
                    .unwrap_or_else(|| panic!("{path}: U+{code:04X} has pixel {ch:?}"));
                pixels[y * 8 + x] = i as u8;
            }
        }
        out.push((code, pixels));
    }
    out
}

fn to_cell(pixels: &[u8; 64]) -> Cell {
    std::array::from_fn(|y| (0..8).fold(0, |row, x| row | (pixels[y * 8 + x] << (7 - x))))
}

fn main() {
    println!("cargo::rerun-if-changed={BDF}");
    println!("cargo::rerun-if-changed={OVERRIDES}");
    println!("cargo::rerun-if-changed={SYMBOLS}");
    println!("cargo::rerun-if-changed={HANGUL}");
    println!("cargo::rerun-if-changed={EMOJI}");
    println!("cargo::rerun-if-changed=src/cell_glyph.rs");
    let read = |path| std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{path}: {e}"));
    let out_dir = std::env::var("OUT_DIR").unwrap();

    let mut cells = BTreeMap::new();
    for g in parse_bdf(&read(BDF)) {
        // Zero-width glyphs are combining marks, which the console cannot use.
        if g.code <= 0xFFFF && g.advance > 0 {
            cells.insert(g.code, place(&g));
        }
    }
    for (code, pixels) in parse_art(OVERRIDES, &read(OVERRIDES), ".#") {
        assert!(
            cells.contains_key(&code),
            "override for missing U+{code:04X}"
        );
        cells.insert(code, to_cell(&pixels));
    }

    let mut out = Vec::with_capacity(cells.len() * 10);
    for (code, cell) in cells {
        out.extend_from_slice(&(code as u16).to_le_bytes());
        out.extend_from_slice(&cell);
    }
    std::fs::write(Path::new(&out_dir).join("fallback_font.bin"), out).expect("write the font");

    let mut out = Vec::new();
    let mut symbols = parse_art(SYMBOLS, &read(SYMBOLS), SYMBOL_CHARS);
    symbols.sort_by_key(|&(code, _)| code);
    for (code, pixels) in symbols {
        out.extend_from_slice(&(code as u16).to_le_bytes());
        out.extend_from_slice(&pixels);
    }
    std::fs::write(Path::new(&out_dir).join("fallback_symbols.bin"), out)
        .expect("write the symbols");

    extra_glyphs(&read, Path::new(&out_dir));
}

/// Writes a table of cells: u32 count, u8 cell size, u8 bits per pixel
/// (1 or 8), the codes as u32 in order, then the cells, deflated.
fn write_table(path: &Path, cell: u32, bits: u8, glyphs: &BTreeMap<u32, Vec<u8>>) {
    let mut out = Vec::new();
    out.extend_from_slice(&(glyphs.len() as u32).to_le_bytes());
    out.extend_from_slice(&[cell as u8, bits]);
    for &code in glyphs.keys() {
        out.extend_from_slice(&code.to_le_bytes());
    }
    for alpha in glyphs.values() {
        match bits {
            1 => {
                for byte in alpha.chunks(8) {
                    out.push(byte.iter().fold(0, |b, &a| b << 1 | u8::from(a >= 128)));
                }
            }
            _ => out.extend_from_slice(alpha),
        }
    }
    let packed = miniz_oxide::deflate::compress_to_vec(&out, 10);
    std::fs::write(path, packed).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
}

fn extra_glyphs(read: &dyn Fn(&'static str) -> String, out_dir: &Path) {
    let bdf = cell_glyph::parse_bdf(&read(HANGUL));
    let layout = cell_glyph::BdfLayout::new(&bdf);
    let hangul = bdf
        .glyphs
        .iter()
        .map(|g| (g.code, cell_glyph::bdf_cell(g, &layout, 16)))
        .collect();
    write_table(&out_dir.join("extra_hangul16.bin"), 16, 1, &hangul);

    let bytes = std::fs::read(EMOJI).unwrap_or_else(|e| panic!("{EMOJI}: {e}"));
    let font = ab_glyph::FontRef::try_from_slice(&bytes).expect("emoji font");
    let codes: Vec<char> = {
        use ab_glyph::Font as _;
        let mut codes: Vec<char> = font.codepoint_ids().map(|(_, c)| c).collect();
        codes.sort_unstable();
        codes.dedup();
        codes
    };
    for cell in [8, 16] {
        let glyphs = codes
            .iter()
            .filter_map(|&c| Some((c as u32, cell_glyph::outline_cell(&font, c, cell, 1.0)?)))
            .collect();
        let name = format!("extra_emoji{cell}.bin");
        write_table(&out_dir.join(name), cell, 8, &glyphs);
    }
}
