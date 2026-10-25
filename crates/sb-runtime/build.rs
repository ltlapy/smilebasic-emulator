//! Converts the fallback font from BDF into 8x8 console cells.
//!
//! Output (`$OUT_DIR/fallback_font.bin`): one 10-byte record per character,
//! sorted by code: u16 code (little-endian), then 8 row bytes with the
//! leftmost pixel in bit 7.

use std::collections::BTreeMap;
use std::path::Path;

const BDF: &str = "fonts/galmuri/GalmuriMono7.bdf";
const OVERRIDES: &str = "fonts/galmuri/overrides.txt";
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

fn parse_overrides(text: &str) -> Vec<(u32, Cell)> {
    let mut out = Vec::new();
    let mut lines = text
        .lines()
        .filter(|l| !l.is_empty() && !l.starts_with('#'));
    while let Some(head) = lines.next() {
        let hex = head
            .strip_prefix("U+")
            .and_then(|h| h.split_whitespace().next())
            .unwrap_or_else(|| panic!("{OVERRIDES}: expected U+XXXX, found {head:?}"));
        let code = u32::from_str_radix(hex, 16).expect("hex code");
        let mut cell = [0u8; 8];
        for row in &mut cell {
            let line = lines.next().expect("8 rows per override");
            assert_eq!(line.len(), 8, "{OVERRIDES}: rows are 8 characters");
            for (x, ch) in line.chars().enumerate() {
                if ch == '#' {
                    *row |= 0x80 >> x;
                }
            }
        }
        out.push((code, cell));
    }
    out
}

fn main() {
    println!("cargo::rerun-if-changed={BDF}");
    println!("cargo::rerun-if-changed={OVERRIDES}");
    let bdf = std::fs::read_to_string(BDF).expect("read the fallback font");
    let overrides = std::fs::read_to_string(OVERRIDES).expect("read the overrides");

    let mut cells = BTreeMap::new();
    for g in parse_bdf(&bdf) {
        // Zero-width glyphs are combining marks, which the console cannot use.
        if g.code <= 0xFFFF && g.advance > 0 {
            cells.insert(g.code, place(&g));
        }
    }
    for (code, cell) in parse_overrides(&overrides) {
        assert!(
            cells.contains_key(&code),
            "override for missing U+{code:04X}"
        );
        cells.insert(code, cell);
    }

    let mut out = Vec::with_capacity(cells.len() * 10);
    for (code, cell) in cells {
        out.extend_from_slice(&(code as u16).to_le_bytes());
        out.extend_from_slice(&cell);
    }
    let dest = Path::new(&std::env::var("OUT_DIR").unwrap()).join("fallback_font.bin");
    std::fs::write(dest, out).expect("write the converted font");
}
