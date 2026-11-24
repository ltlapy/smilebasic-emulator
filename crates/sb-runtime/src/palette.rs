//! The editor's colors: one for each part of the language and for the
//! backgrounds behind lines. Palettes are kept and shared as text, one
//! `part=#RRGGBB` per line. Lines starting with `#` are comments.

use crate::syntax::{LineKind, Role};

/// The parts of the editor a palette colors. The first thirteen are
/// SmileBASIC 4's editor colors; selection is the emulator's own.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Part {
    Character,
    Numeric,
    String,
    Constant,
    Label,
    Command,
    Reserved,
    Comment,
    DefLine,
    LabelLine,
    CursorLine,
    /// Behind text found by a search.
    Search,
    Background,
    Selection,
}

impl Part {
    pub const ALL: [Part; 14] = [
        Part::Character,
        Part::Numeric,
        Part::String,
        Part::Constant,
        Part::Label,
        Part::Command,
        Part::Reserved,
        Part::Comment,
        Part::DefLine,
        Part::LabelLine,
        Part::CursorLine,
        Part::Search,
        Part::Background,
        Part::Selection,
    ];

    /// The name in palette text.
    pub fn key(self) -> &'static str {
        match self {
            Part::Character => "character",
            Part::Numeric => "numeric",
            Part::String => "string",
            Part::Constant => "constant",
            Part::Label => "label",
            Part::Command => "command",
            Part::Reserved => "reserved",
            Part::Comment => "comment",
            Part::DefLine => "def_line",
            Part::LabelLine => "label_line",
            Part::CursorLine => "cursor_line",
            Part::Search => "search",
            Part::Background => "background",
            Part::Selection => "selection",
        }
    }

    /// The name shown to the user.
    pub fn label(self) -> &'static str {
        match self {
            Part::Character => "Character",
            Part::Numeric => "Numeric",
            Part::String => "String",
            Part::Constant => "Constant",
            Part::Label => "Label",
            Part::Command => "Command",
            Part::Reserved => "Reserved word",
            Part::Comment => "Comment",
            Part::DefLine => "DEF line",
            Part::LabelLine => "Label line",
            Part::CursorLine => "Cursor line",
            Part::Search => "Search match",
            Part::Background => "Background",
            Part::Selection => "Selection",
        }
    }
}

impl From<Role> for Part {
    fn from(role: Role) -> Part {
        match role {
            Role::Character => Part::Character,
            Role::Numeric => Part::Numeric,
            Role::String => Part::String,
            Role::Constant => Part::Constant,
            Role::Label => Part::Label,
            Role::Command => Part::Command,
            Role::Reserved => Part::Reserved,
            Role::Comment => Part::Comment,
        }
    }
}

/// Opaque `0xAARRGGBB` colors, in the order of [`Part::ALL`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Palette([u32; 14]);

/// Builds a palette from `0xRRGGBB` values in the order of [`Part::ALL`].
const fn rgb(colors: [u32; 14]) -> Palette {
    let mut out = [0; 14];
    let mut i = 0;
    while i < 14 {
        out[i] = 0xFF00_0000 | colors[i];
        i += 1;
    }
    Palette(out)
}

/// SmileBASIC 4's default editor colors.
pub const SMILEBASIC: Palette = rgb([
    0xFFFFFF, 0xF04090, 0x6080F0, 0xB0D0D0, 0xF0A000, 0x7B7BF7, 0x00C0F0, 0x20F010, 0x003030,
    0x402000, 0x606000, 0x807020, 0x000000, 0x305090,
]);

/// Palettes to start from: the SmileBASIC 4 palette and those of
/// well-known editor themes, mapped onto its parts.
pub const PRESETS: &[(&str, Palette)] = &[
    ("SmileBASIC 4", SMILEBASIC),
    (
        "Monokai",
        rgb([
            0xF8F8F2, 0xAE81FF, 0xE6DB74, 0x66D9EF, 0xFD971F, 0xA6E22E, 0xF92672, 0x75715E,
            0x1E3330, 0x3A2E1C, 0x3E3D32, 0x6B5F2A, 0x272822, 0x49483E,
        ]),
    ),
    (
        "Dracula",
        rgb([
            0xF8F8F2, 0xBD93F9, 0xF1FA8C, 0x8BE9FD, 0xFFB86C, 0x50FA7B, 0xFF79C6, 0x6272A4,
            0x223844, 0x40323A, 0x44475A, 0x6A6040, 0x282A36, 0x515A82,
        ]),
    ),
    (
        "One Dark",
        rgb([
            0xABB2BF, 0xD19A66, 0x98C379, 0x56B6C2, 0xE5C07B, 0x61AFEF, 0xC678DD, 0x5C6370,
            0x213A3E, 0x3A3328, 0x2C313C, 0x5A4C2E, 0x282C34, 0x3E4451,
        ]),
    ),
    (
        "Solarized Dark",
        rgb([
            0x839496, 0xD33682, 0x2AA198, 0x6C71C4, 0xCB4B16, 0x268BD2, 0x859900, 0x586E75,
            0x00403F, 0x2A3226, 0x073642, 0x4A4A20, 0x002B36, 0x274656,
        ]),
    ),
    (
        "Solarized Light",
        rgb([
            0x657B83, 0xD33682, 0x2AA198, 0x6C71C4, 0xCB4B16, 0x268BD2, 0x859900, 0x93A1A1,
            0xE3F0E0, 0xF6E6C8, 0xEEE8D5, 0xF0DC9C, 0xFDF6E3, 0xCFDCE6,
        ]),
    ),
];

impl Default for Palette {
    fn default() -> Self {
        SMILEBASIC
    }
}

impl std::ops::Index<Part> for Palette {
    type Output = u32;

    fn index(&self, part: Part) -> &u32 {
        &self.0[part as usize]
    }
}

impl std::ops::IndexMut<Part> for Palette {
    fn index_mut(&mut self, part: Part) -> &mut u32 {
        &mut self.0[part as usize]
    }
}

/// Reads `#RRGGBB` or `RRGGBB` as an opaque color.
pub fn parse_color(text: &str) -> Option<u32> {
    let hex = text.trim();
    let hex = hex.strip_prefix('#').unwrap_or(hex);
    if hex.len() != 6 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    u32::from_str_radix(hex, 16).ok().map(|c| 0xFF00_0000 | c)
}

pub fn format_color(color: u32) -> String {
    format!("#{:06X}", color & 0xFF_FFFF)
}

impl Palette {
    /// The name of the preset with exactly these colors.
    pub fn preset_name(&self) -> Option<&'static str> {
        PRESETS
            .iter()
            .find(|(_, p)| p == self)
            .map(|(name, _)| *name)
    }

    /// Sets a part from a `key` and a `#RRGGBB` value; false if either is
    /// not understood.
    pub fn set(&mut self, key: &str, value: &str) -> bool {
        let part = Part::ALL.into_iter().find(|p| p.key() == key);
        match (part, parse_color(value)) {
            (Some(part), Some(color)) => {
                self[part] = color;
                true
            }
            _ => false,
        }
    }

    /// Reads palette text over SmileBASIC 4's colors, so that files
    /// missing a part still load. Fails if no line sets a color.
    pub fn from_text(text: &str) -> Result<Palette, String> {
        let mut palette = SMILEBASIC;
        let mut read = 0;
        for line in text.lines() {
            let line = line.trim();
            if line.starts_with('#') {
                continue;
            }
            if let Some((key, value)) = line.split_once('=')
                && palette.set(key.trim(), value)
            {
                read += 1;
            }
        }
        if read == 0 {
            return Err("no editor colors in the file".to_string());
        }
        Ok(palette)
    }

    /// Lines of `part=#RRGGBB`, each key after `prefix`.
    pub fn lines(&self, prefix: &str) -> String {
        Part::ALL
            .iter()
            .map(|&p| format!("{prefix}{}={}\n", p.key(), format_color(self[p])))
            .collect()
    }

    pub fn to_text(&self) -> String {
        format!("# SmileBASIC emulator editor colors\n{}", self.lines(""))
    }

    /// The background behind a line other than the cursor line.
    pub fn line_background(&self, kind: LineKind) -> u32 {
        match kind {
            LineKind::Plain => self[Part::Background],
            LineKind::Def => self[Part::DefLine],
            LineKind::Label => self[Part::LabelLine],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_round_trips() {
        for (name, palette) in PRESETS {
            assert_eq!(Palette::from_text(&palette.to_text()).as_ref(), Ok(palette));
            assert_eq!(palette.preset_name(), Some(*name));
        }
        assert_eq!(SMILEBASIC[Part::Numeric], 0xFFF0_4090);
        assert!(
            SMILEBASIC
                .to_text()
                .contains("\nlabel_line=#402000\ncursor_line=#606000\n")
        );
    }

    #[test]
    fn reads_partial_and_odd_text() {
        let p =
            Palette::from_text("# mine\n comment = 00ff00\nnumeric=#12\nfuture=#FFFFFF\n").unwrap();
        assert_eq!(p[Part::Comment], 0xFF00_FF00);
        assert_eq!(p[Part::Numeric], SMILEBASIC[Part::Numeric]);
        assert_eq!(p.preset_name(), None);
        assert!(Palette::from_text("hello").is_err());
        assert_eq!(parse_color("#a0B0c0"), Some(0xFFA0_B0C0));
        assert_eq!(format_color(0xFFA0_B0C0), "#A0B0C0");
    }
}
