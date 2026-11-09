//! The bold of the bundled 16-pixel dot glyphs (Izumi Gothic 16 and
//! Unifont JP, see `fonts/izumi` and `fonts/unifont`), which are drawn in
//! one-pixel strokes. `build.rs` includes this file and makes the glyphs
//! bold once, when it builds their table; the crate only tests it.
//!
//! The rule is the one Izumi Gothic 16 Bold was made with: each set pixel
//! also sets its neighbor on one side, but only where the pixel beyond that
//! neighbor is empty, so one-pixel gaps between strokes stay open. Setting
//! every neighbor would close them and clog dense kanji. No row or column
//! is repeated, so nothing is stretched.

/// A 16x16 glyph: one row per element from the top, bit `x` set when
/// column `x` (0 is the left) is inked.
pub type Cell16 = [u16; 16];

const LEFT: u16 = 1;
const RIGHT: u16 = 1 << 15;

/// Which font a glyph comes from, which decides the side it grows to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Face {
    /// Izumi Gothic 16: always to the left, as Izumi Gothic 16 Bold does.
    /// Its kanji leave column 0 empty and mostly touch column 15.
    Izumi,
    /// Unifont JP: to the left when column 0 is empty, else to the right.
    Unifont,
}

/// How bolding treats the cell's side columns, where a glyph meets its
/// neighbors.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EdgeRule {
    /// The rule alone, as Izumi Gothic 16 Bold ships: many kanji then
    /// touch both sides of the cell, and the next character.
    Plain,
    /// No pixel is added to the side column the glyph grows toward when
    /// the regular glyph leaves that column empty. A stroke in the column
    /// next to it stays one pixel wide.
    KeepEdge,
    /// [`EdgeRule::KeepEdge`], but a stroke kept out of the side column
    /// grows inward instead, under the same gap rule, so it still ends up
    /// two pixels wide.
    KeepEdgeInward,
    /// [`EdgeRule::KeepEdgeInward`], after a Unifont glyph that touches
    /// column 0 and leaves column 15 empty moves one pixel right, so that
    /// it sits as Izumi's kanji do and grows left like them. The move
    /// removes no pixel. Without it, an Izumi kanji followed by a hanzi
    /// only Unifont JP has touches it about a third of the time.
    KeepEdgeInwardShifted,
}

/// The columns any row of `rows` inks.
fn columns(rows: &Cell16) -> u16 {
    rows.iter().fold(0, |m, &r| m | r)
}

/// The pixels the rule adds to row `r`, toward the left or the right.
fn grow(r: u16, left: bool) -> u16 {
    match left {
        true => (r >> 1) & !(r << 1),
        false => (r << 1) & !(r >> 1),
    }
}

/// `rows` made bold. Pixels pushed out of the cell are dropped; the
/// regular glyph's pixels all stay.
pub fn embolden(rows: &Cell16, face: Face, edge: EdgeRule) -> Cell16 {
    let mut rows = *rows;
    let mut face = face;
    if edge == EdgeRule::KeepEdgeInwardShifted && face == Face::Unifont {
        let m = columns(&rows);
        if m & LEFT != 0 && m & RIGHT == 0 {
            rows = rows.map(|r| r << 1);
            face = Face::Izumi;
        }
    }
    let m = columns(&rows);
    let left = face == Face::Izumi || m & LEFT == 0;
    let side = if left { LEFT } else { RIGHT };
    let block = match edge {
        EdgeRule::Plain => 0,
        _ if m & side != 0 => 0,
        _ => side,
    };
    let inward = matches!(
        edge,
        EdgeRule::KeepEdgeInward | EdgeRule::KeepEdgeInwardShifted
    );
    rows.map(|r| {
        let added = grow(r, left);
        let refused = added & block;
        // From column 1 to column 2 (or 14 to 13), where the pixel beyond
        // must be empty too.
        let inner = match (inward, left) {
            (false, _) => 0,
            (true, true) => (refused << 2) & !(r >> 1),
            (true, false) => (refused >> 2) & !(r << 1),
        };
        r | (added & !block) | inner
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A glyph from rows of `#` and `.`, the rest of the cell empty.
    fn cell(art: &[&str]) -> Cell16 {
        let mut rows = [0; 16];
        for (y, line) in art.iter().enumerate() {
            for (x, ch) in line.chars().enumerate() {
                if ch == '#' {
                    rows[y] |= 1 << x;
                }
            }
        }
        rows
    }

    fn art(rows: &Cell16, n: usize) -> Vec<String> {
        rows[..n]
            .iter()
            .map(|&r| {
                (0..16)
                    .map(|x| if r >> x & 1 == 1 { '#' } else { '.' })
                    .collect()
            })
            .collect()
    }

    #[test]
    fn gaps_of_one_pixel_stay_open() {
        let regular = cell(&["...#.#....#....#"]);
        let bold = embolden(&regular, Face::Izumi, EdgeRule::Plain);
        // Column 4 stays empty: 3 is set, so 5 does not grow into it.
        assert_eq!(art(&bold, 1), ["..##.#...##...##"]);
    }

    #[test]
    fn edges() {
        // A stroke in column 1 with column 0 empty, and one at the right.
        let regular = cell(&[".#.............#", ".#.#...........#"]);
        let grown = |edge| art(&embolden(&regular, Face::Izumi, edge), 2);
        assert_eq!(
            grown(EdgeRule::Plain),
            ["##............##", "##.#..........##"]
        );
        assert_eq!(
            grown(EdgeRule::KeepEdge),
            [".#............##", ".#.#..........##"]
        );
        // Kept out of column 0, column 1 grows into 2, unless that would
        // close the gap to a stroke in 3.
        assert_eq!(
            grown(EdgeRule::KeepEdgeInward),
            [".##...........##", ".#.#..........##"]
        );
        // Original pixels in an edge column let the glyph grow into it.
        let inked = cell(&["#..#"]);
        assert_eq!(
            art(&embolden(&inked, Face::Izumi, EdgeRule::KeepEdge), 1),
            ["#.##............"]
        );
    }

    #[test]
    fn inward_growth_keeps_gaps() {
        // Strokes in columns 1 and 3: neither may grow into 2.
        let regular = cell(&[".#.#"]);
        let bold = embolden(&regular, Face::Izumi, EdgeRule::KeepEdgeInward);
        assert_eq!(art(&bold, 1), [".#.#............"]);
        // A gap of two pixels is not kept.
        let regular = cell(&[".#..#"]);
        let bold = embolden(&regular, Face::Izumi, EdgeRule::KeepEdgeInward);
        assert_eq!(art(&bold, 1), [".####..........."]);
    }

    #[test]
    fn unifont_grows_toward_its_free_side() {
        let left_free = cell(&[".#......"]);
        let against_left = cell(&["#......#"]);
        let bold = |rows, edge| art(&embolden(rows, Face::Unifont, edge), 1);
        assert_eq!(bold(&left_free, EdgeRule::Plain), ["##.............."]);
        assert_eq!(bold(&against_left, EdgeRule::Plain), ["##.....##......."]);
        // Izumi grows left even then, and loses what leaves the cell.
        let izumi = embolden(&against_left, Face::Izumi, EdgeRule::Plain);
        assert_eq!(art(&izumi, 1), ["#.....##........"]);
    }

    #[test]
    fn shifted_unifont_glyphs_sit_as_izumi_does() {
        let against_left = cell(&["#..#", "#..."]);
        let bold = embolden(
            &against_left,
            Face::Unifont,
            EdgeRule::KeepEdgeInwardShifted,
        );
        // Moved right, column 0 kept empty, and the strokes in column 1
        // grow inward.
        assert_eq!(art(&bold, 2), [".####...........", ".##............."]);
        // Without the move, the same glyph grows right.
        let bold = embolden(&against_left, Face::Unifont, EdgeRule::KeepEdgeInward);
        assert_eq!(art(&bold, 2), ["##.##...........", "##.............."]);
        // A glyph that also touches column 15 stays where it is.
        let both = cell(&["#..............#"]);
        let bold = embolden(&both, Face::Unifont, EdgeRule::KeepEdgeInwardShifted);
        assert_eq!(art(&bold, 1), ["##.............#"]);
    }

    #[test]
    fn empty_glyphs_stay_empty() {
        for edge in [
            EdgeRule::Plain,
            EdgeRule::KeepEdge,
            EdgeRule::KeepEdgeInward,
            EdgeRule::KeepEdgeInwardShifted,
        ] {
            for face in [Face::Izumi, Face::Unifont] {
                assert_eq!(embolden(&[0; 16], face, edge), [0; 16]);
            }
        }
    }
}
