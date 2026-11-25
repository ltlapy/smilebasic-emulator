//! SmileBASIC 4's in-game help: reading it from the `HelpData.idx` and
//! `HelpData.dat` of the user's own copy, finding the command that the
//! editor's cursor is on, and HELPGET and HELPINFO, which give the help
//! to programs.
//!
//! The index holds a count, then for each topic `{name_off, name_len,
//! list_off, list_count}` (all `u32`): the topic's ASCII name and a list of
//! `u64` page references. A reference's low half is the page's offset in
//! the data file and its high half the byte length of an example program
//! stored right after the page header (the Japanese help only). A page is
//! a `u32` line count, the example, and the lines: `{type, span_count,
//! byte_len}` (`u32`), `span_count` color spans of 16 bytes (`i32`
//! position, `i32` length, `u32` color and an `i32` that is always -1),
//! and the UTF-16LE text.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use sb_core::bytecode::Want;
use sb_core::error::{Error, ErrorKind, Result};
use sb_core::value::{Array, ArrayData, ElemType, Value, new_str};

use crate::runtime::{Command, Runtime, no_form};

/// The topic that the help opens on when nothing is asked for.
pub const INTRO: &str = "DEFAULTHELPKEY";
/// The most values in HELPGET's array of colored runs. A real page has about a
/// thousand; only a damaged file asks for more.
const MOST_RUN_VALUES: usize = 1 << 24;
/// A character that the help puts at some line ends, removed when a page
/// is read.
const ZERO_WIDTH_SPACE: u16 = 0x200B;

/// Words that are not separate topics, and the topic that covers them.
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

/// A run of a line in another color.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Span {
    pub pos: i32,
    pub len: i32,
    pub color: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Line {
    pub style: Style,
    /// The type as stored, which HELPGET reports: 0 to 8.
    pub kind: u32,
    pub spans: Vec<Span>,
    pub text: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Page {
    pub lines: Vec<Line>,
    /// The example program, if the page stores one.
    pub example: String,
}

fn utf16le(raw: &[u8]) -> Vec<u16> {
    raw.chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .collect()
}

pub struct Help {
    data: Vec<u8>,
    /// Each topic's page references, by name.
    topics: HashMap<String, Vec<u64>>,
}

/// The help's result for the cursor's place in a line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Lookup {
    Topic(String),
    /// A word that is not a command in the help.
    Unknown(String),
    /// The cursor is not on a word.
    Nothing,
}

fn u32_at(bytes: &[u8], at: usize) -> Option<usize> {
    let raw = bytes.get(at..at.checked_add(4)?)?;
    Some(u32::from_le_bytes(raw.try_into().ok()?) as usize)
}

fn page_at(data: &[u8], reference: u64) -> Option<Page> {
    page_and_size(data, reference).map(|(page, _)| page)
}

/// Returns a page and the number of bytes of the data file it takes.
fn page_and_size(data: &[u8], reference: u64) -> Option<(Page, usize)> {
    let start = (reference & 0xFFFF_FFFF) as usize;
    let mut at = (reference & 0xFFFF_FFFF) as usize;
    let example_len = (reference >> 32) as usize;
    let count = u32_at(data, at)?;
    at = at.checked_add(4)?;
    let example = data.get(at..at.checked_add(example_len)?)?;
    let example = String::from_utf16_lossy(&utf16le(example))
        .trim_end_matches('\0')
        .to_string();
    at += example_len;
    let mut lines = Vec::new();
    for _ in 0..count {
        let kind = u32_at(data, at)?;
        let span_count = u32_at(data, at + 4)?;
        let len = u32_at(data, at + 8)?;
        at = at.checked_add(12)?;
        let spans = (0..span_count)
            .map(|i| {
                let s = at.checked_add(i.checked_mul(16)?)?;
                Some(Span {
                    pos: u32_at(data, s)? as i32,
                    len: u32_at(data, s + 4)? as i32,
                    color: u32_at(data, s + 8)? as u32,
                })
            })
            .collect::<Option<Vec<_>>>()?;
        at = at.checked_add(span_count.checked_mul(16)?)?;
        let raw = data.get(at..at.checked_add(len)?)?;
        at += len;
        let units: Vec<u16> = utf16le(raw)
            .into_iter()
            .filter(|&u| u != ZERO_WIDTH_SPACE)
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
        lines.push(Line {
            style,
            kind: kind as u32,
            spans,
            text,
        });
    }
    Some((Page { lines, example }, at - start))
}

impl Help {
    /// Reads the index and data files. Returns `None` if they are not the
    /// help files or are damaged.
    pub fn from_bytes(index: &[u8], data: Vec<u8>) -> Option<Help> {
        let mut topics = HashMap::new();
        // Topics share pages, and each page is read once. Pages do not
        // overlap in a real help, so their total size is at most the size of
        // the data file. A damaged index could make every reference read
        // most of the file, here and again each time a topic is shown.
        let mut sizes: HashMap<u64, usize> = HashMap::new();
        let mut read = 0usize;
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
            let mut shown = 0usize;
            for &reference in &refs {
                let size = match sizes.get(&reference) {
                    Some(&size) => size,
                    None => {
                        let (_, size) = page_and_size(&data, reference)?;
                        sizes.insert(reference, size);
                        read = read.checked_add(size)?;
                        size
                    }
                };
                shown = shown.checked_add(size)?;
                if read > data.len() || shown > data.len() {
                    return None;
                }
            }
            topics.insert(name, refs);
        }
        if topics.is_empty() {
            return None;
        }
        Some(Help { data, topics })
    }

    pub fn topics(&self) -> impl Iterator<Item = &str> {
        self.topics.keys().map(String::as_str)
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

    /// Returns the topic for the command at the cursor, or just left of it,
    /// in a line; `col` is the cursor's column in UTF-16 units. Elsewhere in
    /// a command's arguments, up to the end of the line, it is the function
    /// whose brackets the cursor is in, or else the statement's command, the
    /// same way SmileBASIC 4's F1 finds it.
    pub fn lookup(&self, line: &[u16], col: usize) -> Lookup {
        let word = word_at(line, col);
        let around = word.iter().cloned().chain(enclosing(line, col));
        for name in around {
            if let Some(topic) = self.topic(&name) {
                return Lookup::Topic(topic);
            }
        }
        match word {
            Some(word) => Lookup::Unknown(word),
            None => Lookup::Nothing,
        }
    }

    /// The topic a word names, if any.
    fn topic(&self, word: &str) -> Option<String> {
        let alias = ALIASES.iter().find(|(w, _)| *w == word).map(|&(_, t)| t);
        // A name with a type suffix (`LEFT$`) is a separate topic, and so
        // is `ARRAY%`; for other words the suffix belongs to a variable of
        // the same name.
        let bare = word.trim_end_matches(['$', '%', '#']);
        [Some(word), alias, Some(bare)]
            .into_iter()
            .flatten()
            .find(|t| self.has(t))
            .map(String::from)
    }
}

/// The names whose arguments the cursor at `col` is among, innermost
/// first: the functions whose brackets are still open, then the
/// statement's command. Strings and comments are skipped.
fn enclosing(line: &[u16], col: usize) -> Vec<String> {
    let upper = |r: &[u16]| String::from_utf16_lossy(r).to_ascii_uppercase();
    let mut statement = None;
    // The word before each open bracket.
    let mut calls: Vec<Option<String>> = Vec::new();
    let mut word_start = None;
    let mut last_word: Option<(usize, usize)> = None;
    let mut i = 0;
    let end = col.min(line.len());
    while i < end {
        let c = line[i];
        if is_word(c) || (word_start.is_some() && is_suffix(c)) {
            word_start.get_or_insert(i);
            i += 1;
            continue;
        }
        if let Some(start) = word_start.take() {
            let word = upper(&line[start..i]);
            last_word = Some((start, i));
            match word.as_str() {
                // A new statement starts after THEN and ELSE.
                "THEN" | "ELSE" => (statement, last_word) = (None, None),
                "REM" => return Vec::new(),
                _ if statement.is_none() && !word.starts_with(|c: char| c.is_ascii_digit()) => {
                    statement = Some(word)
                }
                _ => {}
            }
        }
        match u8::try_from(c) {
            Ok(b'"') => {
                // A string runs to its closing quote.
                i += 1;
                while i < end && line[i] != u16::from(b'"') {
                    i += 1;
                }
            }
            // In a comment, nothing.
            Ok(b'\'') => return Vec::new(),
            Ok(b':') => {
                (statement, last_word) = (None, None);
                calls.clear();
            }
            Ok(b'?') if statement.is_none() => statement = Some("?".into()),
            Ok(b'(') => {
                let before = last_word
                    .filter(|&(_, e)| e == i)
                    .map(|(s, e)| upper(&line[s..e]));
                calls.push(before);
            }
            Ok(b')') => _ = calls.pop(),
            _ => {}
        }
        i += 1;
    }
    // The cursor in a word that started a statement counts for it.
    if statement.is_none()
        && let Some(start) = word_start
    {
        statement = Some(upper(&line[start..end]));
    }
    calls.into_iter().rev().flatten().chain(statement).collect()
}

fn array(elem: ElemType, dims: Vec<usize>, data: ArrayData) -> Value {
    let mut array = Array::new(elem, dims);
    array.data = data;
    Value::Array(Rc::new(RefCell::new(array)))
}

fn string(text: &str) -> Value {
    Value::Str(new_str(text.encode_utf16().collect()))
}

/// HELPINFO's array: each page's number of lines.
fn line_counts(pages: &[Page]) -> Value {
    let counts = pages.iter().map(|p| p.lines.len() as i32).collect();
    array(ElemType::Int, vec![pages.len()], ArrayData::Int(counts))
}

impl Runtime {
    /// HELPINFO OUT: the topic at the editor's cursor, for SmileBASIC 4's
    /// own help, which runs from #SYS (`system`). Other programs get the
    /// first page's topic, as on hardware, unless the settings let them see
    /// the cursor.
    pub(crate) fn helpinfo_out(&self, system: bool) -> Vec<Value> {
        let allowed = system || self.settings.helpinfo_cursor;
        let topic = match self.help.as_ref().filter(|_| allowed).map(|h| {
            let (line, col) = &self.help_cursor;
            h.lookup(line, *col)
        }) {
            Some(Lookup::Topic(topic)) => topic,
            _ => INTRO.to_string(),
        };
        let pages = self
            .help_pages(&string(&topic))
            .map_or_else(|_| Vec::new(), |p| p.1);
        vec![string(&topic), line_counts(&pages)]
    }

    fn help_pages(&self, name: &Value) -> Result<(String, Vec<Page>)> {
        let help = self.help.as_ref().ok_or_else(|| {
            Error::with_detail(ErrorKind::IllegalFunctionCall, "no help imported")
        })?;
        let name =
            String::from_utf16_lossy(&name.clone().into_str()?.borrow()).to_ascii_uppercase();
        if !help.has(&name) {
            return Err(Error::with_detail(ErrorKind::IllegalFunctionCall, name));
        }
        let pages = help.pages(&name);
        Ok((name, pages))
    }

    /// HELPGET and HELPINFO, which SmileBASIC 4's own help viewer uses.
    pub(crate) fn help_command(
        &mut self,
        command: Command,
        args: &[Value],
        want: Want,
    ) -> Result<Vec<Value>> {
        match (command, want, args) {
            // The lines, their types, their colored runs as position,
            // length and color, and the example.
            // Checked on hardware: pages count from 0; the array of runs is
            // one run wide when no line has more.
            (Command::Helpget, Want::Outs(4), [name, page]) => {
                let (_, pages) = match self.help_pages(name) {
                    // A name the help does not have is not an error.
                    Err(e) if self.help.is_some() && e.kind == ErrorKind::IllegalFunctionCall => {
                        let none = || array(ElemType::Str, vec![0], ArrayData::Str(Vec::new()));
                        return Ok(vec![none(), none(), none(), none()]);
                    }
                    pages => pages?,
                };
                let n = page.to_int()?;
                let page = usize::try_from(n)
                    .ok()
                    .and_then(|i| pages.get(i))
                    .ok_or_else(|| {
                        Error::with_detail(ErrorKind::OutOfRange, format!("page {n}"))
                    })?;
                let rows = page.lines.len();
                let width = page
                    .lines
                    .iter()
                    .map(|l| l.spans.len())
                    .max()
                    .unwrap_or(0)
                    .max(1);
                let values = rows
                    .checked_mul(width)
                    .and_then(|cells| cells.checked_mul(3))
                    .filter(|&values| values <= MOST_RUN_VALUES)
                    .ok_or_else(|| {
                        let detail = format!("help page of {rows}x{width} runs");
                        Error::with_detail(ErrorKind::IllegalFileFormat, detail)
                    })?;
                let mut runs = vec![0.0; values];
                for (r, line) in page.lines.iter().enumerate() {
                    for k in 0..width {
                        let at = (r * width + k) * 3;
                        let run = match line.spans.get(k) {
                            // The color comes opaque (checked on hardware).
                            Some(s) => [
                                f64::from(s.pos),
                                f64::from(s.len),
                                f64::from((s.color | 0xFF00_0000) as i32),
                            ],
                            None => [-1.0, 0.0, 0.0],
                        };
                        runs[at..at + 3].copy_from_slice(&run);
                    }
                }
                let texts = page
                    .lines
                    .iter()
                    .map(|l| new_str(l.text.encode_utf16().collect()));
                Ok(vec![
                    array(ElemType::Str, vec![rows], ArrayData::Str(texts.collect())),
                    array(
                        ElemType::Int,
                        vec![rows],
                        ArrayData::Int(page.lines.iter().map(|l| l.kind as i32).collect()),
                    ),
                    array(ElemType::Real, vec![rows, width, 3], ArrayData::Real(runs)),
                    string(&page.example),
                ])
            }
            // The topic where the editor's cursor was when the program
            // started, or for a subprogram, where the cursor is now.
            (Command::Helpinfo, Want::Outs(2), []) => {
                let system = self.storage.project().eq_ignore_ascii_case("#SYS");
                Ok(self.helpinfo_out(system))
            }
            // Also as a function (checked on hardware).
            (Command::Helpinfo, Want::Outs(1) | Want::Value, [name]) => {
                let pages = match self.help_pages(name) {
                    // A name the help does not have is not an error.
                    Err(e) if self.help.is_some() && e.kind == ErrorKind::IllegalFunctionCall => {
                        Vec::new()
                    }
                    pages => pages?.1,
                };
                Ok(vec![line_counts(&pages)])
            }
            _ => Err(no_form()),
        }
    }
}

fn is_word(c: u16) -> bool {
    c == u16::from(b'_') || (c < 0x80 && (c as u8).is_ascii_alphanumeric())
}

fn is_suffix(c: u16) -> bool {
    c < 0x80 && b"$%#".contains(&(c as u8))
}

/// Returns the upper-cased word under the cursor, or else the word ending
/// just left of it. Labels and constants (`@NAME`, `#NAME`) are not commands.
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

    /// An index of one topic, with these page references.
    fn index_of(name: &str, refs: &[u64]) -> Vec<u8> {
        let mut index = Vec::new();
        index.extend(1u32.to_le_bytes());
        index.extend(20u32.to_le_bytes());
        index.extend((name.len() as u32).to_le_bytes());
        index.extend((20 + name.len() as u32).to_le_bytes());
        index.extend((refs.len() as u32).to_le_bytes());
        index.extend(name.as_bytes());
        index.extend(refs.iter().flat_map(|r| r.to_le_bytes()));
        index
    }

    /// A page with `rows` lines, the first of them with `spans` runs.
    fn page_of(rows: u32, spans: u32) -> Vec<u8> {
        let mut data = Vec::new();
        data.extend(rows.to_le_bytes());
        for row in 0..rows {
            let spans = if row == 0 { spans } else { 0 };
            data.extend(0u32.to_le_bytes());
            data.extend(spans.to_le_bytes());
            data.extend(0u32.to_le_bytes());
            data.resize(data.len() + 16 * spans as usize, 0);
        }
        data
    }

    #[test]
    fn helpget_refuses_a_page_that_asks_for_too_many_runs() {
        let (index, data) = (index_of("PRINT", &[0]), page_of(5000, 5000));
        let help = Help::from_bytes(&index, data).unwrap();
        let mut rt = Runtime::for_tests(crate::font::Font::blank());
        rt.help = Some(Rc::new(help));
        let args = [Value::str("PRINT"), Value::Int(0)];
        let got = rt.help_command(Command::Helpget, &args, Want::Outs(4));
        assert_eq!(got.unwrap_err().kind, ErrorKind::IllegalFileFormat);

        let (index, data) = (index_of("PRINT", &[0]), page_of(10, 2));
        rt.help = Some(Rc::new(Help::from_bytes(&index, data).unwrap()));
        let got = rt.help_command(Command::Helpget, &args, Want::Outs(4));
        assert_eq!(got.unwrap().len(), 4);
    }

    /// TEST62: no error, and four empty arrays.
    #[test]
    fn a_name_the_help_does_not_have() {
        let (index, data) = (index_of("PRINT", &[0]), page_of(10, 2));
        let mut rt = Runtime::for_tests(crate::font::Font::blank());
        rt.help = Some(Rc::new(Help::from_bytes(&index, data).unwrap()));
        let args = [Value::str("NOSUCHNAME"), Value::Int(0)];
        let got = rt.help_command(Command::Helpget, &args, Want::Outs(4));
        assert_eq!(got.unwrap().len(), 4);
    }

    #[test]
    fn a_help_that_reads_its_pages_over_and_over_is_refused() {
        let data = page_of(100, 0);
        let (one, twice) = (index_of("A", &[0]), index_of("A", &[0; 2]));
        assert!(Help::from_bytes(&one, data.clone()).is_some());
        assert!(Help::from_bytes(&twice, data.clone()).is_none());
        // The whole data file, read as pages at many offsets.
        let repeating: Vec<u8> = (0..1000u32)
            .flat_map(|_| [100u32, 0, 0])
            .flat_map(u32::to_le_bytes)
            .collect();
        let refs: Vec<u64> = (0..500).map(|i| i * 12).collect();
        assert!(Help::from_bytes(&index_of("A", &refs), repeating).is_none());
    }

    #[test]
    fn topics_may_share_a_page() {
        let mut index = Vec::new();
        index.extend(2u32.to_le_bytes());
        for (name, list) in [(36u32, 38u32), (37, 38)] {
            index.extend(name.to_le_bytes());
            index.extend(1u32.to_le_bytes());
            index.extend(list.to_le_bytes());
            index.extend(1u32.to_le_bytes());
        }
        index.extend(b"AB");
        index.extend(0u64.to_le_bytes());
        let help = Help::from_bytes(&index, page_of(3, 1)).unwrap();
        assert_eq!(help.pages("A"), help.pages("B"));
        assert_eq!(help.pages("A")[0].lines.len(), 3);
    }

    /// A help with the topics DEFAULTHELPKEY, IF and LEFT$, one page each;
    /// IF's page has an example and a line with an orange run.
    fn help() -> Help {
        let mut data = Vec::new();
        let mut refs = Vec::new();
        let utf16le = |s: &str| {
            s.encode_utf16()
                .flat_map(u16::to_le_bytes)
                .collect::<Vec<_>>()
        };
        for text in ["intro", "if form", "left form"] {
            let example = if text == "if form" {
                utf16le("IF 1 THEN")
            } else {
                vec![]
            };
            refs.push(data.len() as u64 | (example.len() as u64) << 32);
            data.extend(1u32.to_le_bytes());
            data.extend(&example);
            let spans: &[[i32; 4]] = match text {
                "if form" => &[[3, 4, 0xF39800, -1]],
                _ => &[],
            };
            data.extend(1u32.to_le_bytes());
            data.extend((spans.len() as u32).to_le_bytes());
            data.extend((text.len() as u32 * 2).to_le_bytes());
            data.extend(spans.iter().flatten().flat_map(|n| n.to_le_bytes()));
            data.extend(utf16le(text));
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
                kind: 1,
                spans: vec![],
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
        // A suffix that is not part of a topic's name is dropped.
        assert_eq!(at("IF% 1", 1), Lookup::Topic("IF".into()));
        assert_eq!(at("FOO=1", 1), Lookup::Unknown("FOO".into()));
        assert_eq!(at("  ", 1), Lookup::Nothing);
    }

    #[test]
    fn looks_up_the_command_whose_arguments_the_cursor_is_in() {
        let help = help();
        let at = |line: &str, col| help.lookup(&utf16(line), col);
        let topic = |t: &str| Lookup::Topic(t.into());
        // Up to the end of the line, past the arguments.
        assert_eq!(at("IF A==1 ", 8), topic("IF"));
        assert_eq!(at("X=LEFT$(A$, 1)", 11), topic("LEFT$"));
        assert_eq!(at("X=LEFT$(A$, 1)", 14), Lookup::Nothing);
        // Not within strings, nor across statements or a comment.
        assert_eq!(at("IF \"(\" ", 8), topic("IF"));
        assert_eq!(at("IF A THEN B=1 ", 14), Lookup::Nothing);
        assert_eq!(at("IF A:B=1 ", 9), Lookup::Nothing);
        assert_eq!(at("IF A '", 6), Lookup::Nothing);
        // For a word that is not a topic, the command around it is used.
        assert_eq!(at("IF FOO", 6), topic("IF"));
    }

    #[test]
    fn only_allowed_programs_see_the_topic_at_the_cursor() {
        let mut rt = Runtime::for_tests(crate::font::Font::blank());
        rt.help = Some(Rc::new(help()));
        rt.help_cursor = (utf16("IF A"), 4);
        let topic = |out: Vec<Value>| {
            String::from_utf16_lossy(&out[0].clone().into_str().unwrap().borrow())
        };
        assert_eq!(topic(rt.helpinfo_out(false)), INTRO);
        assert_eq!(topic(rt.helpinfo_out(true)), "IF");
        rt.settings.helpinfo_cursor = true;
        assert_eq!(topic(rt.helpinfo_out(false)), "IF");
    }

    #[test]
    fn programs_read_the_help() {
        let mut rt = Runtime::for_tests(crate::font::Font::blank());
        rt.help = Some(Rc::new(help()));
        let source = "HELPGET \"if\",0 OUT L$,T,A,E$\n\
            ?L$[0];T[0];A[0,0,0];A[0,0,1];HEX$(A[0,0,2]);E$\n\
            HELPINFO \"LEFT$\" OUT P:?LEN(P);P[0]\n\
            HELPINFO OUT N$,P:?N$;LEN(P)";
        let mut vm = sb_core::vm::Vm::new(sb_core::compile(source, &rt).unwrap());
        while vm.run(&mut rt, 100_000).unwrap() != sb_core::vm::Status::Ended {}
        let console = &rt.text[crate::text::CONSOLE];
        let row = |y| {
            let codes: Vec<u16> = (0..console.size().0)
                .map(|x| console.cell(x, y).code)
                .take_while(|&c| c != 0)
                .collect();
            String::from_utf16(&codes).unwrap()
        };
        assert_eq!(row(0), "if form134FFF39800IF 1 THEN");
        assert_eq!(row(1), "11");
        assert_eq!(row(2), "DEFAULTHELPKEY1");

        // A name the help does not have: an empty array, no error.
        let source = "HELPINFO \"PRINT\" OUT P\nLOCATE 0,3:?LEN(P)";
        let mut vm = sb_core::vm::Vm::new(sb_core::compile(source, &rt).unwrap());
        while vm.run(&mut rt, 100_000).unwrap() != sb_core::vm::Status::Ended {}
        let console = &rt.text[crate::text::CONSOLE];
        assert_eq!(console.cell(0, 3).code, u16::from(b'0'));
    }
}
