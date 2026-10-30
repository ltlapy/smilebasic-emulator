//! Predefined `#` constants from the SmileBASIC 4 reference.
//!
//! `#_LINE`, `#_SLOT` and `#_FILENAME` depend on where they appear and are
//! handled by the compiler.

/// A constant's value. Colors are stored as their 32-bit pattern.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ConstValue {
    Int(i32),
    Real(f64),
}

/// 4.X.Y is 4000000 + X*10000 + Y*100. This emulator follows 4.4.9.
pub const VERSION: i32 = 4_040_900;

const fn color(argb: u32) -> ConstValue {
    ConstValue::Int(argb as i32)
}

const fn int(n: i32) -> ConstValue {
    ConstValue::Int(n)
}

static CONSTANTS: &[(&str, ConstValue)] = &[
    ("ON", int(1)),
    ("OFF", int(0)),
    ("YES", int(1)),
    ("NO", int(0)),
    ("TRUE", int(1)),
    ("FALSE", int(0)),
    ("PI", ConstValue::Real(std::f64::consts::PI)),
    ("EXP", ConstValue::Real(std::f64::consts::E)),
    ("C_CLEAR", color(0x0000_0000)),
    ("C_AQUA", color(0xFF00_FFFF)),
    ("C_BLACK", color(0xFF00_0000)),
    ("C_BLUE", color(0xFF00_00FF)),
    ("C_CYAN", color(0xFF00_FFFF)),
    ("C_FUCHSIA", color(0xFFFF_00FF)),
    ("C_GRAY", color(0xFF80_8080)),
    ("C_GREEN", color(0xFF00_8000)),
    ("C_LIME", color(0xFF00_FF00)),
    ("C_MAGENTA", color(0xFFFF_00FF)),
    ("C_MAROON", color(0xFF80_0000)),
    ("C_NAVY", color(0xFF00_0080)),
    ("C_OLIVE", color(0xFF80_8000)),
    ("C_PURPLE", color(0xFF80_0080)),
    ("C_RED", color(0xFFFF_0000)),
    ("C_SILVER", color(0xFFC0_C0C0)),
    ("C_TEAL", color(0xFF00_8080)),
    ("C_WHITE", color(0xFFFF_FFFF)),
    ("C_YELLOW", color(0xFFFF_FF00)),
    ("B_RUP", int(0)),
    ("B_RDOWN", int(1)),
    ("B_RLEFT", int(2)),
    ("B_RRIGHT", int(3)),
    ("B_LUP", int(4)),
    ("B_LDOWN", int(5)),
    ("B_LLEFT", int(6)),
    ("B_LRIGHT", int(7)),
    ("B_L1", int(8)),
    ("B_R1", int(9)),
    ("B_L2", int(10)),
    ("B_R2", int(11)),
    ("B_SL", int(8)),
    ("B_SR", int(9)),
    ("B_S1", int(10)),
    ("B_S2", int(11)),
    ("B_LSTICK", int(12)),
    ("B_RSTICK", int(13)),
    ("B_RANY", int(14)),
    ("B_LANY", int(15)),
    ("B_ANY", int(16)),
    // Not in the reference's table but present in SmileBASIC 4 itself.
    // UNVERIFIED: values follow the BUTTON table (A is the right face button).
    ("B_X", int(0)),
    ("B_B", int(1)),
    ("B_Y", int(2)),
    ("B_A", int(3)),
    ("A_ROT0", int(0)),
    ("A_ROT90", int(1)),
    ("A_ROT180", int(2)),
    ("A_ROT270", int(3)),
    ("A_REVH", int(4)),
    ("A_REVV", int(8)),
    ("A_ADD", int(16)),
    ("CHKXY", int(1)),
    ("CHKZ", int(2)),
    ("CHKR", int(4)),
    ("CHKS", int(8)),
    ("CHKC", int(16)),
    ("CHKV", int(32)),
    ("CHKUV", int(64)),
    ("CHKI", int(128)),
    ("T_DEFAULT", int(0)),
    ("T_INT", int(1)),
    ("T_REAL", int(2)),
    ("T_STR", int(3)),
    ("T_INTARRAY", int(5)),
    ("T_REALARRAY", int(6)),
    ("T_STRARRAY", int(7)),
    ("G_NORMAL", int(0)),
    ("G_NORMAL2", int(1)),
    ("G_ALPHA", int(2)),
    ("G_ALPHA2", int(3)),
    ("G_ADD", int(4)),
    ("BQAPF", int(0)),
    ("BQLPF", int(1)),
    ("BQHPF", int(2)),
    ("BQBPF", int(3)),
    ("BQBSF", int(4)),
    ("BQLSF", int(5)),
    ("BQHSF", int(6)),
    ("BQPEQ", int(7)),
    ("WFRECT", int(0)),
    ("WFHAMM", int(1)),
    ("WFHANN", int(2)),
    ("WFBLKM", int(3)),
    ("AOPADD", int(0)),
    ("AOPSUB", int(1)),
    ("AOPMUL", int(2)),
    ("AOPDIV", int(3)),
    ("AOPMAD", int(4)),
    ("AOPLIP", int(5)),
    ("AOPCLP", int(6)),
    ("EFCOFF", int(0)),
    ("EFCON", int(1)),
    ("EFCBATH", int(1)),
    ("EFCCAVE", int(2)),
    ("EFCSPACE", int(3)),
    ("EFCREFSROOM", int(0)),
    ("EFCREFLROOM", int(1)),
    ("EFCREFHALL", int(2)),
    ("EFCREFCAVE", int(3)),
    ("EFCREFNONE", int(4)),
    ("EFCREVROOM", int(0)),
    ("EFCREVHALL", int(1)),
    ("EFCREVMETAL", int(2)),
    ("EFCREVCAVE", int(3)),
    ("EFCREVREV", int(4)),
    ("PVLEFT", int(0)),
    ("PVRIGHT", int(1)),
    ("GRPWIDTH", int(2048)),
    ("GRPHEIGHT", int(2048)),
    ("GRPF", int(5)),
    ("GSPRITE", int(4095)),
    ("TCONSOLE", int(4)),
    ("MAXT", int(3)),
    ("MAXSP", int(4094)),
    ("MAXGRP", int(4)),
    // First user-defined character. Not in the reference's table.
    // UNVERIFIED: value taken from a comment in a bundled program.
    ("TUSRCHR", int(0xE800)),
    ("HARDWARE", int(10)),
    ("VERSION", int(VERSION)),
];

/// Every predefined constant, by its name without `#`.
pub fn all() -> &'static [(&'static str, ConstValue)] {
    CONSTANTS
}

/// Looks up a predefined constant by its upper-cased name without `#`.
pub fn lookup(name: &str) -> Option<ConstValue> {
    CONSTANTS.iter().find(|(n, _)| *n == name).map(|&(_, v)| v)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lookup_finds_constants() {
        assert_eq!(lookup("C_WHITE"), Some(ConstValue::Int(-1)));
        assert_eq!(lookup("TRUE"), Some(ConstValue::Int(1)));
        assert_eq!(lookup("NOPE"), None);
    }

    #[test]
    fn names_are_unique() {
        let mut names: Vec<_> = CONSTANTS.iter().map(|(n, _)| n).collect();
        names.sort();
        let len = names.len();
        names.dedup();
        assert_eq!(names.len(), len);
    }
}
