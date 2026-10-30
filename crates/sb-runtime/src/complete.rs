//! Completion of the word being typed, in the editor and in direct mode:
//! a list of SmileBASIC's words, and if asked for the program's own
//! names, that start with it, below the cursor. SmileBASIC 4's editor
//! lists its words from the first letter; the settings can change that.

use sb_core::lexer::Keyword;

use crate::font::Font;
use crate::framebuffer::Framebuffer;
use crate::runtime::keys;
use crate::settings::Settings;
use crate::syntax::{ALSO_RESERVED, COMMANDS, Role, highlight, is_ident_char};
use crate::text::TextScreen;

/// Entries shown at once; the list scrolls through the rest.
pub const ROWS: usize = 8;

const BACK: u32 = 0xFF20_2838;
const CHOSEN: u32 = 0xFF2F_5FAF;
const TEXT: u32 = 0xFFFF_FFFF;
const BORDER: u32 = 0xFF80_8080;

/// What goes in the list, and when it shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Options {
    /// Letters typed before the list shows.
    pub after: usize,
    /// SmileBASIC's instructions, functions and reserved words.
    pub commands: bool,
    /// Variables, labels, constants and DEFs found in the program.
    pub names: bool,
    /// Enter takes the chosen entry, as Tab always does.
    pub enter: bool,
}

impl Options {
    /// The settings' options for the editor, or for direct mode; `None`
    /// if completion is off there.
    pub fn from_settings(settings: &Settings, editor: bool) -> Option<Options> {
        let on = if editor {
            settings.complete_editor
        } else {
            settings.complete_direct
        };
        on.then_some(Options {
            after: settings.complete_after as usize,
            commands: settings.complete_commands,
            names: settings.complete_names,
            enter: settings.complete_enter,
        })
    }
}

/// What a key did to an open list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// The list took the key.
    Used,
    /// Put the chosen entry in place of the word.
    Accept,
    /// The list closed and took the key.
    Close,
    /// The key goes on to the text; the list is looked at again after.
    Pass,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Completion {
    /// Where the word starts in the line.
    pub start: usize,
    pub items: Vec<String>,
    /// The entry picked with Up or Down; none until then.
    pub chosen: Option<usize>,
    /// The first entry shown.
    top: usize,
}

/// The word that ends at `col`: where it starts, and its text with a
/// leading `@` or `#`. Nothing inside strings and comments.
pub fn word_before(line: &[u16], col: usize) -> Option<(usize, String)> {
    let col = col.min(line.len());
    let mut start = col;
    while start > 0 && is_ident_char(line[start - 1]) {
        start -= 1;
    }
    if start > 0 && matches!(line[start - 1], 0x40 | 0x23) {
        start -= 1;
    }
    if start == col {
        return None;
    }
    let (roles, _) = highlight(line);
    if matches!(roles[start], Role::String | Role::Comment) {
        return None;
    }
    let word = String::from_utf16_lossy(&line[start..col]);
    // Numbers are no names, but labels may start with a digit.
    if word.starts_with(|c: char| c.is_ascii_digit()) {
        return None;
    }
    Some((start, word))
}

/// The names a program uses: variables, DEFs, labels and constants, as
/// first written.
pub fn names<'a>(lines: impl IntoIterator<Item = &'a [u16]>) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for line in lines {
        let (roles, _) = highlight(line);
        let mut i = 0;
        while i < line.len() {
            let role = roles[i];
            let start = i;
            let named = matches!(role, Role::Character | Role::Label | Role::Constant);
            let starts = is_ident_char(line[i]) || matches!(line[i], 0x40 | 0x23);
            if !named || !starts {
                i += 1;
                continue;
            }
            i += 1;
            while i < line.len() && roles[i] == role && is_ident_char(line[i]) {
                i += 1;
            }
            // A type suffix (`#`, `$` or `%`) belongs to the name.
            if i < line.len() && roles[i] == role && matches!(line[i], 0x23..=0x25) {
                i += 1;
            }
            let word = String::from_utf16_lossy(&line[start..i]);
            let first = word.chars().next().unwrap_or('0');
            if word.len() > 1 && !first.is_ascii_digit() && seen.insert(word.to_ascii_uppercase()) {
                out.push(word);
            }
        }
    }
    out
}

/// The entries for a word: those that start with it but are longer,
/// SmileBASIC's words in the case it is typed in.
pub fn candidates(word: &str, options: &Options, names: &[String]) -> Vec<String> {
    let upper = word.to_ascii_uppercase();
    let lower = word.chars().any(|c| c.is_ascii_lowercase())
        && !word.chars().any(|c| c.is_ascii_uppercase());
    let fits = |w: &str| w.len() > word.len() && w.to_ascii_uppercase().starts_with(&upper);
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    if options.commands && !word.starts_with(['@', '#']) {
        let words = COMMANDS.iter().chain(ALSO_RESERVED).copied();
        let keywords = Keyword::WORDS.iter().map(|w| w.to_ascii_uppercase());
        for w in words.map(String::from).chain(keywords) {
            if fits(&w) && seen.insert(w.clone()) {
                out.push(if lower { w.to_ascii_lowercase() } else { w });
            }
        }
    }
    if options.names {
        for w in names {
            if fits(w) && seen.insert(w.to_ascii_uppercase()) {
                out.push(w.clone());
            }
        }
    }
    out.sort_by_key(|w| w.to_ascii_uppercase());
    out
}

impl Completion {
    /// The list for the word ending at `col`, if it is long enough and
    /// anything fits it.
    pub fn find(line: &[u16], col: usize, options: &Options, names: &[String]) -> Option<Self> {
        let (start, word) = word_before(line, col)?;
        let letters = word.trim_start_matches(['@', '#']).len();
        if letters < options.after.max(1) {
            return None;
        }
        let items = candidates(&word, options, names);
        (!items.is_empty()).then_some(Completion {
            start,
            items,
            chosen: None,
            top: 0,
        })
    }

    /// The entry to put in: the one picked, or else the first.
    pub fn chosen(&self) -> &str {
        &self.items[self.chosen.unwrap_or(0)]
    }

    /// A key while the list is open. Until Up or Down picks an entry,
    /// Enter goes on to the text, so that a list showing after one letter
    /// does not take the Enter that ends a line; Tab takes the first.
    // UNVERIFIED: that SmileBASIC 4's Enter also needs an entry picked.
    pub fn key(&mut self, code: u16, enter: bool) -> Outcome {
        let n = self.items.len();
        let chosen = match (code, self.chosen) {
            (keys::DOWN, None) => 0,
            (keys::UP, None) => n - 1,
            (keys::DOWN, Some(i)) => (i + 1) % n,
            (keys::UP, Some(i)) => (i + n - 1) % n,
            (keys::TAB, _) | (keys::ENTER, Some(_)) if code == keys::TAB || enter => {
                return Outcome::Accept;
            }
            (keys::ESCAPE, _) => return Outcome::Close,
            _ => return Outcome::Pass,
        };
        self.chosen = Some(chosen);
        self.top = self.top.min(chosen);
        if chosen >= self.top + ROWS {
            self.top = chosen + 1 - ROWS;
        }
        Outcome::Used
    }

    /// Draws the list with its top left at a pixel of `fb`, in cells of
    /// `cell` pixels, or above `y` with its bottom at `above` if it would
    /// not fit below.
    pub fn draw(
        &self,
        fb: &mut Framebuffer,
        font: &Font,
        (x, y): (i64, i64),
        above: i64,
        cell: u32,
    ) {
        let rows = self.items.len().min(ROWS);
        let cols = self
            .items
            .iter()
            .map(|w| w.encode_utf16().count())
            .max()
            .unwrap_or(0)
            + 2;
        let (w, h) = (cols as i64 * i64::from(cell), rows as i64 * i64::from(cell));
        let y = if y + h + 2 > i64::from(fb.height()) {
            above - h
        } else {
            y
        };
        let x = x.min(i64::from(fb.width()) - w - 1).max(1);
        let fill = |fb: &mut Framebuffer, x0: i64, y0: i64, w: i64, h: i64, color: u32| {
            for py in y0.max(0)..(y0 + h).max(0) {
                for px in x0.max(0)..(x0 + w).max(0) {
                    fb.set(px as u32, py as u32, color);
                }
            }
        };
        fill(fb, x - 1, y - 1, w + 2, h + 2, BORDER);
        fill(fb, x, y, w, h, BACK);
        let mut screen = TextScreen::new(0, 0);
        screen.set_font(cell, cell, cols as u32, rows as u32);
        screen.pos = (x as f64, y as f64);
        for (row, item) in self.items.iter().enumerate().skip(self.top).take(rows) {
            let r = (row - self.top) as i64;
            if Some(row) == self.chosen {
                fill(fb, x, y + r * i64::from(cell), w, i64::from(cell), CHOSEN);
            }
            for (i, c) in item.encode_utf16().enumerate() {
                screen.put_at(i as u32 + 1, r as u32, c, TEXT);
            }
        }
        screen.draw(fb, font);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn u(text: &str) -> Vec<u16> {
        text.encode_utf16().collect()
    }

    const ALL: Options = Options {
        after: 2,
        commands: true,
        names: true,
        enter: true,
    };

    #[test]
    fn words_before_the_cursor() {
        let line = u("A=SPOFS 0:GOTO @lo:?\"PRI");
        assert_eq!(word_before(&line, 7), Some((2, "SPOFS".into())));
        assert_eq!(word_before(&line, 18), Some((15, "@lo".into())));
        assert_eq!(word_before(&line, 9), None, "a number");
        assert_eq!(word_before(&line, line.len()), None, "in a string");
        assert_eq!(word_before(&line, 8), None);
    }

    #[test]
    fn names_of_a_program() {
        let lines = [
            u("@MAIN:VAR SCORE%=0"),
            u("DEF ADD(A,B)"),
            u("PRINT score%,#RED,NAME$ 'X"),
        ];
        let names = names(lines.iter().map(Vec::as_slice));
        assert_eq!(names, ["@MAIN", "SCORE%", "ADD", "#RED", "NAME$"]);
    }

    #[test]
    fn lists_what_starts_with_the_word() {
        let names = vec!["SPRITE_X".to_string(), "@SPIN".to_string()];
        let c = Completion::find(&u("SPO"), 3, &ALL, &names).unwrap();
        assert_eq!(c.items, ["SPOFS"]);
        let c = Completion::find(&u("sp"), 2, &ALL, &names).unwrap();
        assert!(c.items.contains(&"spofs".to_string()));
        assert!(c.items.contains(&"SPRITE_X".to_string()));
        assert!(!c.items.contains(&"@SPIN".to_string()));
        let c = Completion::find(&u("GOTO @S"), 7, &ALL, &names);
        assert_eq!(c.map(|c| c.items), None, "one letter after the @");
        let c = Completion::find(&u("GOTO @SP"), 8, &ALL, &names).unwrap();
        assert_eq!((c.start, c.items), (5, vec!["@SPIN".to_string()]));
        // Keywords too, and nothing for a whole word alone.
        let c = Completion::find(&u("ENDI"), 4, &ALL, &[]).unwrap();
        assert_eq!(c.items, ["ENDIF"]);
        assert_eq!(Completion::find(&u("ENDIF"), 5, &ALL, &[]), None);
        let only_names = Options {
            commands: false,
            ..ALL
        };
        assert_eq!(Completion::find(&u("SPO"), 3, &only_names, &names), None);
    }

    #[test]
    fn keys_move_through_the_list() {
        let mut c = Completion::find(&u("BGMS"), 4, &ALL, &[]).unwrap();
        assert_eq!(c.items, ["BGMSET", "BGMSETD", "BGMSTOP"]);
        assert_eq!(c.key(keys::ENTER, true), Outcome::Pass, "nothing picked");
        assert_eq!(c.chosen(), "BGMSET");
        assert_eq!(c.key(keys::UP, true), Outcome::Used);
        assert_eq!(c.chosen(), "BGMSTOP");
        c.key(keys::DOWN, true);
        assert_eq!(c.chosen(), "BGMSET");
        assert_eq!(c.key(keys::ENTER, false), Outcome::Pass);
        assert_eq!(c.key(keys::ENTER, true), Outcome::Accept);
        assert_eq!(c.key(keys::TAB, false), Outcome::Accept);
        assert_eq!(c.key(keys::ESCAPE, true), Outcome::Close);
        assert_eq!(c.key(keys::LEFT, true), Outcome::Pass);
    }
}
