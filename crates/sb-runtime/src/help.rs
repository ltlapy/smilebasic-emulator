//! SmileBASIC 4's in-game help, read from the `HelpData.idx` and
//! `HelpData.dat` of the user's own copy, and finding the command that the
//! editor's cursor is on.
//!
//! The index holds a count, then for each topic `{name_off, name_len,
//! list_off, list_count}` (all `u32`): the topic's ASCII name and a list of
//! `u64` page references. A reference's low half is the page's offset in
//! the data file and its high half the byte length of an example program
//! stored right after the page header (the Japanese help only). A page is
//! a `u32` line count, the example, and the lines: `{type, span_count,
//! byte_len}` (`u32`), `span_count` color spans of 16 bytes, and the
//! UTF-16LE text.

use std::collections::HashMap;

/// The topic that the help opens on when nothing is asked for.
pub const INTRO: &str = "DEFAULTHELPKEY";

/// Words that are not topics of their own, and the topic that covers them.
const ALIASES: &[(&str, &str)] = &[
    ("?", "PRINT"),
    ("THEN", "IF"),
    ("ELSE", "IF"),
    ("ENDIF", "IF"),
    ("TO", "FOR"),
    ("STEP", "FOR"),
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Style {
    Body,
    /// A page's first line: the form of the command.
    Title,
    /// The name of an argument or of a form of the result.
    Argument,
    /// A heading such as "Arguments" or "Example".
    Section,
    /// The description under the title.
    Summary,
    /// A horizontal rule.
    Rule,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Line {
    pub style: Style,
    pub text: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Page {
    pub lines: Vec<Line>,
}

pub struct Help {
    data: Vec<u8>,
    /// Each topic's page references, by name.
    topics: HashMap<String, Vec<u64>>,
}

/// What the help has for the cursor's place in a line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Lookup {
    Topic(String),
    /// A word that is no command of the help.
    Unknown(String),
    /// The cursor is not on a word.
    Nothing,
}

fn u32_at(bytes: &[u8], at: usize) -> Option<usize> {
    let raw = bytes.get(at..at.checked_add(4)?)?;
    Some(u32::from_le_bytes(raw.try_into().ok()?) as usize)
}

fn page_at(data: &[u8], reference: u64) -> Option<Page> {
    let mut at = (reference & 0xFFFF_FFFF) as usize;
    let example = (reference >> 32) as usize;
    let count = u32_at(data, at)?;
    at = at.checked_add(4)?.checked_add(example)?;
    let mut lines = Vec::new();
    for _ in 0..count {
        let kind = u32_at(data, at)?;
        let spans = u32_at(data, at + 4)?;
        let len = u32_at(data, at + 8)?;
        at = at.checked_add(12)?.checked_add(spans.checked_mul(16)?)?;
        let raw = data.get(at..at.checked_add(len)?)?;
        at += len;
        let units: Vec<u16> = raw
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            // The help marks some line ends with zero-width spaces.
            .filter(|&u| u != 0x200B)
            .collect();
        let text = String::from_utf16_lossy(&units);
        let style = match kind {
            1 => Style::Title,
            2 => Style::Argument,
            3 | 4 => Style::Section,
            5 => Style::Summary,
            6..=8 => Style::Rule,
            _ => Style::Body,
        };
        lines.push(Line { style, text });
    }
    Some(Page { lines })
}

impl Help {
    /// Reads the index and data files, `None` if they are not the help's
    /// or are damaged.
    pub fn from_bytes(index: &[u8], data: Vec<u8>) -> Option<Help> {
        let mut topics = HashMap::new();
        for i in 0..u32_at(index, 0)? {
            let entry = 4 + 16 * i;
            let (name, len) = (u32_at(index, entry)?, u32_at(index, entry + 4)?);
            let (list, count) = (u32_at(index, entry + 8)?, u32_at(index, entry + 12)?);
            let name = String::from_utf8(index.get(name..name.checked_add(len)?)?.to_vec()).ok()?;
            let refs = (0..count)
                .map(|k| {
                    let raw = index.get(list.checked_add(8 * k)?..)?.get(..8)?;
                    Some(u64::from_le_bytes(raw.try_into().ok()?))
                })
                .collect::<Option<Vec<_>>>()?;
            // Only whole pages count as a help.
            if refs.iter().any(|&r| page_at(&data, r).is_none()) {
                return None;
            }
            topics.insert(name, refs);
        }
        if topics.is_empty() {
            return None;
        }
        Some(Help { data, topics })
    }

    pub fn has(&self, topic: &str) -> bool {
        self.topics.contains_key(topic)
    }

    /// The pages of a topic.
    pub fn pages(&self, topic: &str) -> Vec<Page> {
        let refs = self.topics.get(topic).map_or(&[][..], Vec::as_slice);
        refs.iter()
            .filter_map(|&r| page_at(&self.data, r))
            .collect()
    }

    /// The topic for the command at the cursor, or just left of it, in a
    /// line; `col` is the cursor's column in UTF-16 units.
    pub fn lookup(&self, line: &[u16], col: usize) -> Lookup {
        let Some(word) = word_at(line, col) else {
            return Lookup::Nothing;
        };
        let alias = ALIASES.iter().find(|(w, _)| *w == word).map(|&(_, t)| t);
        // A name with a type suffix (`LEFT$`) is a topic of its own, as
        // are `ARRAY%`; for others the suffix belongs to a variable of
        // the same name.
        let bare = word.trim_end_matches(['$', '%', '#']);
        [Some(word.as_str()), alias, Some(bare)]
            .into_iter()
            .flatten()
            .find(|t| self.has(t))
            .map_or_else(
                || Lookup::Unknown(word.clone()),
                |t| Lookup::Topic(t.into()),
            )
    }
}

fn is_word(c: u16) -> bool {
    c == u16::from(b'_') || (c < 0x80 && (c as u8).is_ascii_alphanumeric())
}

fn is_suffix(c: u16) -> bool {
    c < 0x80 && b"$%#".contains(&(c as u8))
}

/// The upper-cased word under the cursor, else the one ending just left
/// of it. Labels and constants (`@NAME`, `#NAME`) are not commands.
pub fn word_at(line: &[u16], col: usize) -> Option<String> {
    let at = |i: usize| line.get(i).copied();
    let on = |i: usize| at(i).is_some_and(|c| is_word(c) || c == u16::from(b'?'));
    let mut start = if on(col) {
        col
    } else if col > 0 && (on(col - 1) || at(col - 1).is_some_and(is_suffix)) {
        col - 1
    } else {
        return None;
    };
    if at(start) == Some(u16::from(b'?')) {
        return Some("?".into());
    }
    // A suffix the cursor is just past belongs to the word before it.
    if at(start).is_some_and(is_suffix) {
        start -= 1;
    }
    while start > 0 && is_word(line[start - 1]) {
        start -= 1;
    }
    if start > 0 && matches!(line[start - 1], 0x40 | 0x23) {
        return None;
    }
    let mut end = start;
    while end < line.len() && is_word(line[end]) {
        end += 1;
    }
    if end < line.len() && is_suffix(line[end]) {
        end += 1;
    }
    let word = String::from_utf16_lossy(&line[start..end]).to_ascii_uppercase();
    // Names do not start with a digit: `10` is a number.
    (!word.starts_with(|c: char| c.is_ascii_digit())).then_some(word)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn utf16(s: &str) -> Vec<u16> {
        s.encode_utf16().collect()
    }

    fn word(line: &str, col: usize) -> Option<String> {
        word_at(&utf16(line), col)
    }

    /// A help with the topics DEFAULTHELPKEY, IF and LEFT$, one page each.
    fn help() -> Help {
        let mut data = Vec::new();
        let mut refs = Vec::new();
        for text in ["intro", "if form", "left form"] {
            refs.push(data.len() as u64);
            data.extend(1u32.to_le_bytes());
            data.extend([1, 0, 0, 0, 0, 0, 0, 0]);
            data.extend((text.len() as u32 * 2).to_le_bytes());
            data.extend(text.encode_utf16().flat_map(u16::to_le_bytes));
        }
        let names = [INTRO, "IF", "LEFT$"];
        let mut index = (names.len() as u32).to_le_bytes().to_vec();
        let mut tail = Vec::new();
        let base = 4 + 16 * names.len();
        for (name, reference) in names.iter().zip(&refs) {
            let name_at = base + tail.len();
            tail.extend(name.bytes());
            let list_at = base + tail.len();
            tail.extend(reference.to_le_bytes());
            for n in [name_at, name.len(), list_at, 1] {
                index.extend((n as u32).to_le_bytes());
            }
        }
        index.extend(tail);
        Help::from_bytes(&index, data).expect("valid help")
    }

    #[test]
    fn reads_topics_and_pages() {
        let help = help();
        assert!(help.has("IF") && !help.has("PRINT"));
        let pages = help.pages("LEFT$");
        assert_eq!(pages.len(), 1);
        assert_eq!(
            pages[0].lines,
            [Line {
                style: Style::Title,
                text: "left form".into()
            }]
        );
        assert!(help.pages("PRINT").is_empty());
    }

    #[test]
    fn damaged_help_is_refused() {
        assert!(Help::from_bytes(&[], Vec::new()).is_none());
        assert!(Help::from_bytes(&[1, 0, 0, 0, 4, 0], Vec::new()).is_none());
    }

    #[test]
    fn finds_the_word_at_or_left_of_the_cursor() {
        assert_eq!(word("PRINT A", 0).as_deref(), Some("PRINT"));
        assert_eq!(word("print a", 3).as_deref(), Some("PRINT"));
        // Just past the word.
        assert_eq!(word("PRINT A", 5).as_deref(), Some("PRINT"));
        assert_eq!(word("PRINT  A", 6), None);
        assert_eq!(word("A=LEFT$(B$,2)", 8).as_deref(), Some("B$"));
        assert_eq!(word("A=LEFT$(B$,2)", 7).as_deref(), Some("LEFT$"));
        assert_eq!(word("?A", 0).as_deref(), Some("?"));
        assert_eq!(word("GOTO @LOOP", 8), None);
        assert_eq!(word("X=#TRUE", 5), None);
        assert_eq!(word("X=10", 3), None);
        assert_eq!(word("", 0), None);
    }

    #[test]
    fn looks_up_topics() {
        let help = help();
        let at = |line: &str, col| help.lookup(&utf16(line), col);
        assert_eq!(at("IF A THEN B", 0), Lookup::Topic("IF".into()));
        assert_eq!(at("IF A THEN B", 7), Lookup::Topic("IF".into()));
        assert_eq!(at("X=LEFT$(A$,1)", 4), Lookup::Topic("LEFT$".into()));
        // A suffix that is no part of a topic's name is dropped.
        assert_eq!(at("IF% 1", 1), Lookup::Topic("IF".into()));
        assert_eq!(at("FOO=1", 1), Lookup::Unknown("FOO".into()));
        assert_eq!(at("  ", 1), Lookup::Nothing);
    }
}
