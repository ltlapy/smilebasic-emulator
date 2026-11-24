//! Builds the font tables and the version string that the crate embeds.
//!
//! - `$OUT_DIR/fallback_font.bin`, from the GalmuriMono7 BDF: one 10-byte
//!   record per character, sorted by code: u16 code (little-endian), then
//!   8 row bytes with the leftmost pixel in bit 7.
//! - `$OUT_DIR/fallback_symbols.bin`, from `fonts/symbols.txt`: one 66-byte
//!   record per character: u16 code, then 64 palette indices in rows (see
//!   `SYMBOL_PALETTE` in `src/font.rs`).
//! - `$OUT_DIR/extra_dot16.bin`, `extra_misaki8.bin`, `extra_fusion8.bin`
//!   and `extra_sc16.bin`, the editor's extra glyphs (see
//!   `src/extra_glyphs.rs`), from `fonts/izumi`, `fonts/unifont`,
//!   `fonts/misaki`, `fonts/fusionpixel` and `fonts/notosanssc`, and
//!   `extra_half16.bin`, Unifont JP's half-width forms in the left half of
//!   16x16 cells (see `src/half_width.rs`): tables compressed with
//!   deflate, laid out as `extra_glyphs::Table` reads them. The 16-pixel
//!   dot glyphs are made bold here (see `src/dot_bold.rs` and
//!   `BOLD_EDGE`).
//! - `$OUT_DIR/dot_font.bin`, `fonts/neodgm/SBEmulatorDot.ttf` compressed
//!   with deflate (see `src/half_width.rs`).
//! - `SB_VERSION`, the package version followed by the commit the build
//!   comes from (see `commit_id`), for `sb_runtime::VERSION`.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

// The runtime uses parts of it the build does not.
#[allow(dead_code)]
#[path = "src/cell_glyph.rs"]
mod cell_glyph;
// The edge choices BOLD_EDGE does not pick.
#[allow(dead_code)]
#[path = "src/dot_bold.rs"]
mod dot_bold;

const BDF: &str = "fonts/galmuri/GalmuriMono7.bdf";
const OVERRIDES: &str = "fonts/galmuri/overrides.txt";
const SYMBOLS: &str = "fonts/symbols.txt";
const IZUMI: &str = "fonts/izumi/izumi16.hex";
const UNIFONT: &str = "fonts/unifont/unifont_jp-18.0.01.hex.gz";
/// The bolding rule for the cell's side columns of the 16-pixel dot
/// glyphs: `Plain`, `KeepEdge`, `KeepEdgeInward` or
/// `KeepEdgeInwardShifted` (see `dot_bold::EdgeRule`).
const BOLD_EDGE: dot_bold::EdgeRule = dot_bold::EdgeRule::KeepEdgeInwardShifted;
const MISAKI: &str = "fonts/misaki/misaki_gothic_2nd.bdf";
const FUSION: &str = "fonts/fusionpixel/fusion-pixel-8px-monospaced-zh_hans.bdf";
const NOTO_SC: &str = "fonts/notosanssc/SBEmulatorSC-Bold.otf";
const MPLUS: &str = "fonts/mplus/SBEmulator1p-Bold.ttf";
const DOT: &str = "fonts/neodgm/SBEmulatorDot.ttf";
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

/// The commit to name after the version: `SB_COMMIT` when it is set (empty
/// for none), otherwise the short hash of git's `HEAD`, or none outside a
/// git checkout.
fn commit_id() -> Option<String> {
    println!("cargo::rerun-if-env-changed=SB_COMMIT");
    if let Ok(commit) = std::env::var("SB_COMMIT") {
        return Some(commit.trim().to_string()).filter(|c| !c.is_empty());
    }
    let git = |args: &[&str]| {
        let out = std::process::Command::new("git").args(args).output().ok()?;
        let text = String::from_utf8(out.stdout).ok()?;
        out.status.success().then(|| text.trim().to_string())
    };
    // A new commit or checkout changes HEAD, the branch's ref or packed-refs.
    // Cargo reruns the script on every build for a file that does not exist.
    let watch = |path: &str| {
        let file = git(&["rev-parse", "--path-format=absolute", "--git-path", path])?;
        Path::new(&file)
            .exists()
            .then(|| println!("cargo::rerun-if-changed={file}"))
    };
    watch("HEAD");
    watch("packed-refs");
    if let Some(branch) = git(&["symbolic-ref", "-q", "HEAD"]) {
        watch(&branch);
    }
    git(&["rev-parse", "--short=7", "HEAD"]).filter(|c| !c.is_empty())
}

fn main() {
    let version = std::env::var("CARGO_PKG_VERSION").unwrap();
    match commit_id() {
        Some(commit) => println!("cargo::rustc-env=SB_VERSION={version}+{commit}"),
        None => println!("cargo::rustc-env=SB_VERSION={version}"),
    }
    println!("cargo::rerun-if-changed={BDF}");
    println!("cargo::rerun-if-changed={OVERRIDES}");
    println!("cargo::rerun-if-changed={SYMBOLS}");
    println!("cargo::rerun-if-changed={IZUMI}");
    println!("cargo::rerun-if-changed={UNIFONT}");
    println!("cargo::rerun-if-changed={MISAKI}");
    println!("cargo::rerun-if-changed={FUSION}");
    println!("cargo::rerun-if-changed={NOTO_SC}");
    println!("cargo::rerun-if-changed={MPLUS}");
    println!("cargo::rerun-if-changed={DOT}");
    println!("cargo::rerun-if-changed=src/cell_glyph.rs");
    println!("cargo::rerun-if-changed=src/dot_bold.rs");
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

    let galmuri7: BTreeSet<u32> = cells.keys().copied().collect();
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

    extra_glyphs(&read, Path::new(&out_dir), &galmuri7);

    // NeoDunggeunmo, read whole on first use: a fifth of its size packed.
    let dot = std::fs::read(DOT).unwrap_or_else(|e| panic!("{DOT}: {e}"));
    let packed = miniz_oxide::deflate::compress_to_vec(&dot, 10);
    std::fs::write(Path::new(&out_dir).join("dot_font.bin"), packed).expect("write the dot font");
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

/// Writes a table of 1-bit 16x16 cells, laid out the same way as the
/// tables of [`write_table`].
fn write_cells(path: &Path, glyphs: &BTreeMap<u32, dot_bold::Cell16>) {
    let mut out = Vec::new();
    out.extend_from_slice(&(glyphs.len() as u32).to_le_bytes());
    out.extend_from_slice(&[16, 1]);
    for &code in glyphs.keys() {
        out.extend_from_slice(&code.to_le_bytes());
    }
    for rows in glyphs.values() {
        for row in rows {
            // Column 0 goes to the top bit.
            out.extend_from_slice(&row.reverse_bits().to_be_bytes());
        }
    }
    let packed = miniz_oxide::deflate::compress_to_vec(&out, 10);
    std::fs::write(path, packed).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
}

/// The data of a gzip file with one member.
fn gunzip(path: &str, bytes: &[u8]) -> Vec<u8> {
    let bad = || -> ! { panic!("{path}: not a gzip file") };
    if bytes.len() < 18 || bytes[..3] != [0x1F, 0x8B, 8] {
        bad();
    }
    let flags = bytes[3];
    let mut at = 10;
    if flags & 4 != 0 {
        at += 2 + usize::from(u16::from_le_bytes([bytes[10], bytes[11]]));
    }
    // The file name and the comment end with a zero byte.
    for flag in [8, 16] {
        if flags & flag != 0 {
            let len = bytes[at..].iter().position(|&b| b == 0);
            at += len.unwrap_or_else(|| bad()) + 1;
        }
    }
    if flags & 2 != 0 {
        at += 2;
    }
    let data = miniz_oxide::inflate::decompress_to_vec(&bytes[at..])
        .unwrap_or_else(|e| panic!("{path}: {e:?}"));
    let size = u32::from_le_bytes(bytes[bytes.len() - 4..].try_into().unwrap());
    assert_eq!(size, data.len() as u32, "{path}: size");
    data
}

/// The glyphs of a GNU Unifont .hex file (a code in hex, a colon, then
/// 16 rows of 8 or 16 pixels in hex): each code, whether it is
/// half-width, and its rows with column `x` in bit `x`, half-width ones in
/// columns 0 to 7. Glyphs of other widths are skipped.
fn parse_hex(path: &str, text: &str) -> Vec<(u32, bool, dot_bold::Cell16)> {
    let mut glyphs = Vec::new();
    for (n, line) in text.lines().enumerate() {
        let bad = || -> ! { panic!("{path}:{}: not a .hex line", n + 1) };
        let (code, pixels) = line.split_once(':').unwrap_or_else(|| bad());
        let code = u32::from_str_radix(code, 16).unwrap_or_else(|_| bad());
        let digits = match pixels.len() {
            32 => 2,
            64 => 4,
            _ => continue,
        };
        let mut rows = [0; 16];
        for (y, row) in rows.iter_mut().enumerate() {
            let hex = &pixels[y * digits..(y + 1) * digits];
            let bits = u16::from_str_radix(hex, 16).unwrap_or_else(|_| bad());
            *row = match digits {
                2 => bits.reverse_bits() >> 8,
                _ => bits.reverse_bits(),
            };
        }
        glyphs.push((code, digits == 2, rows));
    }
    glyphs
}

/// Whether a character has a use in a cell: controls do not.
fn drawn(code: u32) -> bool {
    code >= 0x20 && !(0x7F..0xA0).contains(&code)
}

/// 16x16 cells by code.
type Cells = BTreeMap<u32, dot_bold::Cell16>;

/// The 16-pixel dot glyphs, made bold: Izumi Gothic 16, then Unifont JP
/// for the characters it lacks, half-width glyphs centered the same way
/// `bdf_cell` places them. Then Unifont JP's half-width forms
/// (U+FF00-U+FFEF) unchanged, in columns 0 to 7: the half-width kana and
/// the few symbols NeoDunggeunmo lacks are all among them.
fn dot_cells(read: &dyn Fn(&'static str) -> String) -> (Cells, Cells) {
    use dot_bold::{Face, embolden};
    let mut cells = BTreeMap::new();
    let mut halves = BTreeMap::new();
    let bytes = std::fs::read(UNIFONT).unwrap_or_else(|e| panic!("{UNIFONT}: {e}"));
    let text = String::from_utf8(gunzip(UNIFONT, &bytes)).expect("Unifont JP is ASCII");
    for (code, half, rows) in parse_hex(UNIFONT, &text) {
        if !drawn(code) {
            continue;
        }
        if half && (0xFF00..=0xFFEF).contains(&code) {
            halves.insert(code, rows);
        }
        let rows = if half { rows.map(|r| r << 4) } else { rows };
        cells.insert(code, embolden(&rows, Face::Unifont, BOLD_EDGE));
    }
    for (code, half, rows) in parse_hex(IZUMI, &read(IZUMI)) {
        assert!(!half, "{IZUMI}: U+{code:04X} is half-width");
        if drawn(code) {
            cells.insert(code, embolden(&rows, Face::Izumi, BOLD_EDGE));
        }
    }
    (cells, halves)
}

/// Writes the editor's tables. `galmuri7` is the set of codes
/// GalmuriMono7 draws.
fn extra_glyphs(read: &dyn Fn(&'static str) -> String, out_dir: &Path, galmuri7: &BTreeSet<u32>) {
    let (cells, halves) = dot_cells(read);
    write_cells(&out_dir.join("extra_dot16.bin"), &cells);
    write_cells(&out_dir.join("extra_half16.bin"), &halves);

    let bytes = std::fs::read(MPLUS).unwrap_or_else(|e| panic!("{MPLUS}: {e}"));
    let mplus = font_codes(&ab_glyph::FontRef::try_from_slice(&bytes).expect("M PLUS 1p"));

    // The fonts that come after others in extra_glyphs.rs keep only the
    // glyphs those lack: the rest would never be drawn.
    let misaki = bdf_cells(&read(MISAKI), 8, |c| !galmuri7.contains(&c));
    let earlier = |c: &u32| galmuri7.contains(c) || misaki.contains_key(c);
    let fusion = bdf_cells(&read(FUSION), 8, |c| !earlier(&c) && !mplus.contains(&c));
    write_table(&out_dir.join("extra_misaki8.bin"), 8, 1, &misaki);
    write_table(&out_dir.join("extra_fusion8.bin"), 8, 1, &fusion);

    // Drawn the same way as the 16-pixel M PLUS 1p Bold glyphs, in the 16
    // steps of gray of SmileBASIC 4's font page, which also packs smaller.
    // The antialiased set takes Noto Sans SC right after M PLUS 1p Bold;
    // the pixel set has every GB2312 character before it.
    let bytes = std::fs::read(NOTO_SC).unwrap_or_else(|e| panic!("{NOTO_SC}: {e}"));
    let font = ab_glyph::FontRef::try_from_slice(&bytes).expect("Noto Sans SC");
    let lacking = |c: u32| !mplus.contains(&c);
    let steps = |a: u8| ((u16::from(a) * 15 + 127) / 255) as u8 * 17;
    let glyphs = outline_cells(&font, 16, cell_glyph::SMILEBASIC_EM, lacking, steps);
    write_table(&out_dir.join("extra_sc16.bin"), 16, 8, &glyphs);
}

/// The characters a font has a glyph for.
fn font_codes(font: &ab_glyph::FontRef) -> BTreeSet<u32> {
    use ab_glyph::Font as _;
    font.codepoint_ids()
        .filter(|&(id, _)| id.0 != 0)
        .map(|(_, c)| c as u32)
        .collect()
}

/// The glyphs of a BDF font with a use in a cell that `keep` wants, in
/// `cell`-pixel cells.
fn bdf_cells(text: &str, cell: u32, keep: impl Fn(u32) -> bool) -> BTreeMap<u32, Vec<u8>> {
    let bdf = cell_glyph::parse_bdf(text);
    let layout = cell_glyph::BdfLayout::new(&bdf);
    bdf.glyphs
        .iter()
        .filter(|g| g.code >= 0x20 && !(0x7F..0xA0).contains(&g.code) && g.advance > 0)
        .filter(|g| keep(g.code))
        .map(|g| (g.code, cell_glyph::bdf_cell(g, &layout, cell)))
        .collect()
}

/// The characters of an outline font that `keep` wants, drawn with gray
/// edges in `cell`-pixel cells with an em of `em_share` of the cell, each
/// alpha byte passed through `alpha`.
fn outline_cells(
    font: &ab_glyph::FontRef,
    cell: u32,
    em_share: f32,
    keep: impl Fn(u32) -> bool,
    alpha: impl Fn(u8) -> u8,
) -> BTreeMap<u32, Vec<u8>> {
    use ab_glyph::Font as _;
    let mut codes: Vec<char> = font.codepoint_ids().map(|(_, c)| c).collect();
    codes.sort_unstable();
    codes.dedup();
    codes
        .into_iter()
        .filter(|&c| c >= ' ' && !('\u{7F}'..'\u{A0}').contains(&c) && keep(c as u32))
        .filter_map(|c| {
            let glyph = cell_glyph::outline_cell(font, c, cell, em_share, cell_glyph::Ink::Gray)?;
            Some((c as u32, glyph.into_iter().map(&alpha).collect()))
        })
        .collect()
}
