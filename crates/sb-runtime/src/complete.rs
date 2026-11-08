//! Completion of the word being typed, in the editor and in direct mode:
//! a list of SmileBASIC's words, and if asked for the program's own
//! names, that start with it, below the cursor. SmileBASIC 4's editor
//! lists its words from the first letter; the settings can change that.
//!
//! Each entry carries a tag for what it is (an instruction, a function, a
//! DEF, a label...). After GOTO, GOSUB and RESTORE only labels are
//! offered, and after `#` only constants.

use std::collections::HashMap;

use sb_core::constants::{self, ConstValue};
use sb_core::lexer::Keyword;

use crate::extra_glyphs::Fonts;
use crate::framebuffer::Framebuffer;
use crate::help::{Help, Style};
use crate::palette::{Palette, Part};
use crate::runtime::keys;
use crate::settings::Settings;
use crate::syntax::{ALSO_RESERVED, COMMANDS, Role, highlight, is_ident_char};
use crate::text::TextScreen;

/// Entries shown at once; the list scrolls through the rest.
pub const ROWS: usize = 6;
/// Cells before each entry for its tag.
const TAG_CELLS: usize = 2;

pub(crate) const BACK: u32 = 0xFF20_2838;
const CHOSEN: u32 = 0xFF2F_5FAF;
pub(crate) const TEXT: u32 = 0xFFFF_FFFF;
pub(crate) const BORDER: u32 = 0xFF80_8080;
pub(crate) const DIM: u32 = 0xFF90_98A8;
pub(crate) const SYNTAX: u32 = 0xFFE0_E090;
const DEF: u32 = 0xFF40_C8A0;

/// What goes in the list, and when it shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Options {
    /// Letters typed before the list shows.
    pub after: usize,
    /// SmileBASIC's instructions, functions and reserved words.
    pub commands: bool,
    /// Variables, labels, constants and DEFs found in the program.
    pub names: bool,
    /// Enter takes the chosen entry, as Tab always does.
    pub enter: bool,
    /// How commands are used shows under the list and while typing
    /// their arguments.
    pub hints: bool,
    /// Hints show at the bottom of the screen, or at the top while the
    /// cursor is down there, instead of by the cursor.
    pub bottom: bool,
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
            hints: settings.complete_hints,
            bottom: !settings.complete_above,
        })
    }
}

/// What an entry is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    /// An instruction, used as a statement.
    Command,
    Function,
    /// Both an instruction and a function, such as RGB.
    Both,
    Keyword,
    /// A DEF that returns a value, and one used as a statement.
    Def,
    Sub,
    Label,
    Constant,
    Variable,
}

impl Kind {
    /// The tag drawn before an entry.
    fn tag(self) -> &'static str {
        match self {
            Kind::Command => "CMD",
            Kind::Function => "FN",
            Kind::Both => "C/F",
            Kind::Keyword => "KW",
            Kind::Def => "DEF",
            Kind::Sub => "SUB",
            Kind::Label => "@",
            Kind::Constant => "#",
            Kind::Variable => "VAR",
        }
    }

    fn color(self, colors: &Palette) -> u32 {
        match self {
            Kind::Command | Kind::Function | Kind::Both => colors[Part::Command],
            Kind::Keyword => colors[Part::Reserved],
            Kind::Def | Kind::Sub => DEF,
            Kind::Label => colors[Part::Label],
            Kind::Constant => colors[Part::Constant],
            Kind::Variable => colors[Part::Character],
        }
    }
}

/// An entry of the list.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Item {
    pub word: String,
    pub kind: Kind,
    /// Shown after the word: a DEF's arguments, an array's brackets or a
    /// constant's value.
    pub detail: String,
}

impl Item {
    fn new(word: impl Into<String>, kind: Kind, detail: impl Into<String>) -> Item {
        Item {
            word: word.into(),
            kind,
            detail: detail.into(),
        }
    }

    /// How a DEF is used, as the help writes a command's forms.
    pub fn form(&self) -> Option<String> {
        match self.kind {
            Kind::Def => Some(format!("{}{}", self.word, self.detail)),
            Kind::Sub if self.detail.is_empty() => Some(self.word.clone()),
            Kind::Sub => Some(format!("{} {}", self.word, self.detail)),
            _ => None,
        }
    }
}

/// SmileBASIC's own words: the forms the help gives for each, from which
/// instructions are told from functions.
#[derive(Default)]
pub struct Library {
    forms: HashMap<String, Vec<String>>,
}

/// Whether a form of the help (`ABS(Number)`, `SPOFS SpriteNumber,X,Y`)
/// is a function's: the name followed by a parenthesis.
fn is_function_form(form: &str, name: &str) -> bool {
    let upper = form.to_ascii_uppercase();
    upper
        .find(name)
        .is_some_and(|at| upper[at + name.len()..].trim_start().starts_with('('))
}

/// A help title without the page it is of, as `(➡1/2)` at its end.
fn without_page_mark(title: &str) -> &str {
    let Some(open) = title.strip_suffix(')').and_then(|t| t.rfind('(')) else {
        return title;
    };
    let mark = &title[open + 1..title.len() - 1];
    let page = mark.contains('/')
        && mark
            .chars()
            .all(|c| c.is_ascii_digit() || c == '/' || !c.is_ascii());
    if page {
        title[..open].trim_end()
    } else {
        title
    }
}

impl Library {
    /// The forms from the help's page titles, if the help is imported.
    pub fn new(help: Option<&Help>) -> Library {
        let mut forms = HashMap::new();
        for topic in help.into_iter().flat_map(Help::topics) {
            let titles: Vec<String> = (help.expect("has topics").pages(topic).into_iter())
                .flat_map(|page| page.lines)
                .filter(|line| line.style == Style::Title)
                .map(|line| without_page_mark(line.text.trim()).to_string())
                .collect();
            if !titles.is_empty() {
                forms.insert(topic.to_ascii_uppercase(), titles);
            }
        }
        Library { forms }
    }

    /// A library with the given forms, as the help would give them.
    #[cfg(test)]
    pub(crate) fn with_forms(forms: &[(&str, &[&str])]) -> Library {
        let forms = (forms.iter())
            .map(|(name, f)| (name.to_string(), f.iter().map(|s| s.to_string()).collect()))
            .collect();
        Library { forms }
    }

    /// The help's forms of a word, if any.
    pub fn forms(&self, word: &str) -> &[String] {
        self.forms
            .get(&word.to_ascii_uppercase())
            .map_or(&[], Vec::as_slice)
    }

    /// What one of SmileBASIC's words is: from the help's forms, or
    /// without the help from the language's own table, where the screen,
    /// sound and file commands count as instructions.
    pub fn kind(&self, word: &str) -> Kind {
        let upper = word.to_ascii_uppercase();
        if Keyword::from_word(&upper).is_some() || ALSO_RESERVED.contains(&upper.as_str()) {
            return Kind::Keyword;
        }
        let forms = self.forms(&upper);
        if !forms.is_empty() {
            let functions = forms.iter().filter(|f| is_function_form(f, &upper)).count();
            return match functions {
                0 => Kind::Command,
                n if n == forms.len() => Kind::Function,
                _ => Kind::Both,
            };
        }
        use sb_core::builtins::{Form, lookup};
        match (
            lookup(&upper, Form::Function).is_some(),
            lookup(&upper, Form::Command).is_some(),
        ) {
            (true, true) => Kind::Both,
            (true, false) => Kind::Function,
            _ => Kind::Command,
        }
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

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Completion {
    /// Where the word starts in the line.
    pub start: usize,
    pub items: Vec<Item>,
    /// The entry picked with Up or Down; none until then.
    pub chosen: Option<usize>,
    /// The first entry shown.
    top: usize,
    /// How the first form of the entry is used, under the list.
    syntax: Vec<Option<String>>,
    /// The form goes under the list; otherwise the caller shows it with
    /// the hints at the bottom.
    syntax_below: bool,
}

/// The word that ends at `col`: where it starts, and its text with a
/// leading `@` or `#`. Nothing inside strings and comments, or for a
/// type suffix.
pub fn word_before(line: &[u16], col: usize) -> Option<(usize, String)> {
    let col = col.min(line.len());
    let mut start = col;
    while start > 0 && is_ident_char(line[start - 1]) {
        start -= 1;
    }
    if start > 0 && matches!(line[start - 1], 0x40 | 0x23) {
        // `A#` is a real variable, not a constant.
        if start > 1 && start == col && is_ident_char(line[start - 2]) {
            return None;
        }
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

/// Whether the word starting at `start` follows GOTO, GOSUB or RESTORE,
/// also as one of ON's labels, so that it names a label.
fn wants_label(line: &[u16], start: usize) -> bool {
    let before = String::from_utf16_lossy(&line[..start]).to_ascii_uppercase();
    let mut rest = before.trim_end();
    // ON X GOTO @A,@B: back over the labels before this one.
    while let Some(head) = rest.strip_suffix(',') {
        let head = head.trim_end();
        rest = head
            .trim_end_matches(|c: char| c.is_ascii_alphanumeric() || c == '_')
            .trim_end_matches('@')
            .trim_end();
    }
    ["GOTO", "GOSUB", "RESTORE"].iter().any(|k| {
        rest.ends_with(k)
            && !rest[..rest.len() - k.len()]
                .ends_with(|c: char| c.is_ascii_alphanumeric() || c == '_')
    })
}

/// The text after `=` in a CONST up to its end, for a constant's value.
fn const_value(text: &str) -> String {
    let text = text.trim_start();
    let Some(value) = text.strip_prefix('=') else {
        return String::new();
    };
    let end = value.find([',', ':', '\'']).unwrap_or(value.len());
    format!("= {}", value[..end].trim())
}

/// `names` followed by those of `more` it does not have.
pub fn merge(mut names: Vec<Item>, more: &[Item]) -> Vec<Item> {
    let have: std::collections::HashSet<String> = names.iter().map(|i| i.word.clone()).collect();
    names.extend(more.iter().filter(|i| !have.contains(&i.word)).cloned());
    names
}

/// The names a program uses: variables, DEFs, labels and constants, as
/// first written. A DEF or an array keeps what its line says of it.
pub fn names<'a>(lines: impl IntoIterator<Item = &'a [u16]>) -> Vec<Item> {
    let mut index: HashMap<String, usize> = HashMap::new();
    let mut out: Vec<Item> = Vec::new();
    for line in lines {
        let (roles, _) = highlight(line);
        // Within DIM or VAR, or CONST, and the reserved word before this
        // one.
        let mut declaring = false;
        let mut constants = false;
        let mut last_reserved = String::new();
        let mut i = 0;
        while i < line.len() {
            let role = roles[i];
            let start = i;
            if line[i] == u16::from(b':') && role == Role::Character {
                declaring = false;
                constants = false;
            }
            if role == Role::Reserved && is_ident_char(line[i]) {
                while i < line.len() && roles[i] == role && is_ident_char(line[i]) {
                    i += 1;
                }
                last_reserved = String::from_utf16_lossy(&line[start..i]).to_ascii_uppercase();
                declaring |= matches!(last_reserved.as_str(), "DIM" | "VAR");
                constants |= last_reserved == "CONST";
                continue;
            }
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
            if word.len() < 2 || first.is_ascii_digit() {
                continue;
            }
            // Columns of the UTF-16 line are those of the text for the
            // ASCII that names and signatures are written in.
            let after = String::from_utf16_lossy(&line[i..]);
            let (kind, detail) = match role {
                Role::Label => (Kind::Label, String::new()),
                Role::Constant if constants => (Kind::Constant, const_value(&after)),
                Role::Constant => (Kind::Constant, String::new()),
                _ if last_reserved == "DEF" => {
                    let end = after.find([':', '\'']).unwrap_or(after.len());
                    let signature = after[..end].trim();
                    if signature.starts_with('(') {
                        let close = signature.find(')').map_or(signature.len(), |c| c + 1);
                        (Kind::Def, signature[..close].to_string())
                    } else {
                        (Kind::Sub, signature.to_string())
                    }
                }
                _ if declaring && after.starts_with('[') => {
                    let close = after.find(']').unwrap_or(after.len());
                    let commas = after[..close].matches(',').count();
                    (Kind::Variable, format!("[{}]", ",".repeat(commas)))
                }
                _ => (Kind::Variable, String::new()),
            };
            last_reserved.clear();
            let key = word.to_ascii_uppercase();
            match index.get(&key) {
                // What a DEF, an array or a CONST says wins over a use.
                Some(&at) if !detail.is_empty() || kind != Kind::Variable => {
                    if out[at].detail.is_empty()
                        && (out[at].kind == Kind::Variable || out[at].kind == kind)
                    {
                        out[at].kind = kind;
                        out[at].detail = detail;
                    }
                }
                Some(_) => {}
                None => {
                    index.insert(key, out.len());
                    out.push(Item::new(word, kind, detail));
                }
            }
        }
    }
    out
}

/// A predefined constant's value as written in a program: colors
/// (`#C_RED`) in hexadecimal.
fn constant_text(name: &str, value: ConstValue) -> String {
    match value {
        ConstValue::Int(n) if name.starts_with("C_") => format!("= &H{:08X}", n as u32),
        ConstValue::Int(n) => format!("= {n}"),
        ConstValue::Real(x) => format!("= {x}"),
    }
}

/// The entries for a word: those that start with it but are longer,
/// SmileBASIC's words in the case it is typed in. `label` asks for labels
/// only, as after GOTO.
pub fn candidates(
    word: &str,
    label: bool,
    options: &Options,
    library: &Library,
    names: &[Item],
) -> Vec<Item> {
    let lower = word.chars().any(|c| c.is_ascii_lowercase())
        && !word.chars().any(|c| c.is_ascii_uppercase());
    let fits = |w: &str, typed: &str| {
        w.len() > typed.len()
            && w.to_ascii_uppercase()
                .starts_with(&typed.to_ascii_uppercase())
    };
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    let mut add = |item: Item, out: &mut Vec<Item>| {
        if seen.insert(item.word.to_ascii_uppercase()) {
            out.push(item);
        }
    };
    if label {
        let typed = format!("@{}", word.trim_start_matches('@'));
        for item in names.iter().filter(|i| i.kind == Kind::Label) {
            if fits(&item.word, &typed) || typed == "@" {
                add(item.clone(), &mut out);
            }
        }
    } else if word.starts_with('#') {
        for item in names.iter().filter(|i| i.kind == Kind::Constant) {
            if fits(&item.word, word) {
                add(item.clone(), &mut out);
            }
        }
        for &(name, value) in constants::all() {
            let w = format!("#{name}");
            if fits(&w, word) || word == "#" {
                add(
                    Item::new(w, Kind::Constant, constant_text(name, value)),
                    &mut out,
                );
            }
        }
    } else if !word.starts_with('@') {
        if options.commands {
            let words = COMMANDS.iter().chain(ALSO_RESERVED).copied();
            let keywords = Keyword::WORDS.iter().map(|w| w.to_ascii_uppercase());
            for w in words.map(String::from).chain(keywords) {
                if fits(&w, word) {
                    let kind = library.kind(&w);
                    let w = if lower { w.to_ascii_lowercase() } else { w };
                    add(Item::new(w, kind, ""), &mut out);
                }
            }
        }
        if options.names {
            for item in names {
                if !matches!(item.kind, Kind::Label | Kind::Constant) && fits(&item.word, word) {
                    add(item.clone(), &mut out);
                }
            }
        }
    } else if options.names {
        for item in names.iter().filter(|i| i.kind == Kind::Label) {
            if fits(&item.word, word) {
                add(item.clone(), &mut out);
            }
        }
    }
    out.sort_by_key(|i| i.word.to_ascii_uppercase());
    out
}

impl Completion {
    /// The list for the word ending at `col`, if it is long enough and
    /// anything fits it. After GOTO and its kind, or at a lone `@` or `#`,
    /// it shows at once.
    pub fn find(
        line: &[u16],
        col: usize,
        options: &Options,
        library: &Library,
        names: &[Item],
    ) -> Option<Self> {
        let (start, word) = word_before(line, col)?;
        let label = wants_label(line, start);
        let letters = word.trim_start_matches(['@', '#']).len();
        let context = label || word == "@" || word == "#";
        if !context && letters < options.after.max(1) {
            return None;
        }
        let items = candidates(&word, label, options, library, names);
        let syntax = (items.iter())
            .map(|item| match options.hints {
                false => None,
                true => (item.form()).or_else(|| library.forms(&item.word).first().cloned()),
            })
            .collect();
        (!items.is_empty()).then_some(Completion {
            start,
            items,
            chosen: None,
            top: 0,
            syntax,
            syntax_below: !options.bottom,
        })
    }

    /// How the chosen entry, or else the first, is used.
    pub fn syntax(&self) -> Option<&str> {
        self.syntax[self.chosen.unwrap_or(0)].as_deref()
    }

    /// The word to put in: the one picked, or else the first.
    pub fn chosen(&self) -> &str {
        &self.items[self.chosen.unwrap_or(0)].word
    }

    /// A key while the list is open. Until Up or Down picks an entry,
    /// Enter goes on to the text, so that a list showing after one letter
    /// does not take the Enter that ends a line; Tab takes the first.
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
    /// not fit below. Under it goes how the entry is used.
    pub fn draw(
        &self,
        fb: &mut Framebuffer,
        fonts: Fonts,
        (x, y): (i64, i64),
        above: i64,
        cell: u32,
        colors: &Palette,
    ) {
        let c = i64::from(cell);
        let rows = self.items.len().min(ROWS);
        let entry = |item: &Item| {
            let detail = match item.detail.is_empty() {
                true => 0,
                false => item.detail.encode_utf16().count() + 1,
            };
            item.word.encode_utf16().count() + detail
        };
        let cols = (self.items.iter().map(entry).max().unwrap_or(0) + TAG_CELLS + 2) as i64;
        let syntax = self.syntax().filter(|_| self.syntax_below);
        let syntax_cols = syntax.map_or(0, |s| s.encode_utf16().count() as i64 + 2);
        let width_cells = cols.max(syntax_cols).min(i64::from(fb.width()) / c);
        let (w, h) = (width_cells * c, rows as i64 * c);
        let total = h + if syntax.is_some() { c + 2 } else { 0 };
        let y = if y + total + 2 > i64::from(fb.height()) {
            above - total
        } else {
            y
        };
        let x = x.min(i64::from(fb.width()) - w - 1).max(1);
        fill(fb, x - 1, y - 1, w + 2, h + 2, BORDER);
        fill(fb, x, y, w, h, BACK);
        let mut screen = TextScreen::new(0, 0);
        screen.set_font(cell, cell, width_cells as u32, rows as u32 + 2);
        screen.pos = (x as f64, y as f64);
        // `@` and `#` are drawn whole, half a cell in, centered in the tag.
        let mut symbols = TextScreen::new(0, 0);
        symbols.set_font(cell, cell, 1, rows as u32);
        symbols.pos = (x as f64 + f64::from(cell) / 2.0, y as f64);
        for (row, item) in self.items.iter().enumerate().skip(self.top).take(rows) {
            let r = (row - self.top) as i64;
            if Some(row) == self.chosen {
                fill(fb, x, y + r * c, w, c, CHOSEN);
            }
            let color = item.kind.color(colors);
            let tag = item.kind.tag();
            if tag.len() == 1 {
                let code = u16::from(tag.as_bytes()[0]);
                symbols.put_at(0, r as u32, code, color);
            } else {
                draw_tag(fb, x + 1, y + r * c, cell, tag, color);
            }
            let mut col = TAG_CELLS as u32 + 1;
            for ch in item.word.encode_utf16() {
                screen.put_at(col, r as u32, ch, TEXT);
                col += 1;
            }
            col += 1;
            for ch in item.detail.encode_utf16() {
                screen.put_at(col, r as u32, ch, DIM);
                col += 1;
            }
        }
        if let Some(syntax) = syntax {
            draw_syntax(fb, fonts, (x, y + h + 2), width_cells, cell, syntax);
        }
        fonts.draw_screen(&mut screen, fb);
        fonts.draw_screen(&mut symbols, fb);
    }
}

/// How an entry is used, on a line `cols` cells wide with its top left
/// at a pixel.
pub fn draw_syntax(
    fb: &mut Framebuffer,
    fonts: Fonts,
    (x, y): (i64, i64),
    cols: i64,
    cell: u32,
    syntax: &str,
) {
    let w = cols * i64::from(cell);
    fill(fb, x - 1, y - 1, w + 2, i64::from(cell) + 2, BORDER);
    fill(fb, x, y, w, i64::from(cell), BACK);
    let mut line = TextScreen::new(0, 0);
    line.set_font(cell, cell, cols as u32, 1);
    line.pos = (x as f64, y as f64);
    for (i, ch) in syntax.encode_utf16().enumerate() {
        line.put_at(i as u32 + 1, 0, ch, SYNTAX);
    }
    fonts.draw_screen(&mut line, fb);
}

pub(crate) fn fill(fb: &mut Framebuffer, x0: i64, y0: i64, w: i64, h: i64, color: u32) {
    for py in y0.max(0)..(y0 + h).max(0) {
        for px in x0.max(0)..(x0 + w).max(0) {
            fb.set(px as u32, py as u32, color);
        }
    }
}

/// Letters three pixels wide and five high for the tags, row by row.
fn tiny_glyph(c: char) -> [u8; 5] {
    match c {
        'C' => [0b111, 0b100, 0b100, 0b100, 0b111],
        'M' => [0b101, 0b111, 0b111, 0b101, 0b101],
        'D' => [0b110, 0b101, 0b101, 0b101, 0b110],
        'F' => [0b111, 0b100, 0b110, 0b100, 0b100],
        'N' => [0b110, 0b101, 0b101, 0b101, 0b101],
        '/' => [0b001, 0b001, 0b010, 0b100, 0b100],
        'K' => [0b101, 0b101, 0b110, 0b101, 0b101],
        'W' => [0b101, 0b101, 0b111, 0b111, 0b101],
        'E' => [0b111, 0b100, 0b110, 0b100, 0b111],
        'S' => [0b011, 0b100, 0b010, 0b001, 0b110],
        'U' => [0b101, 0b101, 0b101, 0b101, 0b111],
        'B' => [0b110, 0b101, 0b110, 0b101, 0b110],
        'V' => [0b101, 0b101, 0b101, 0b101, 0b010],
        'A' => [0b010, 0b101, 0b111, 0b101, 0b101],
        'R' => [0b110, 0b101, 0b110, 0b101, 0b101],
        _ => [0; 5],
    }
}

/// A tag on a dark pill of its color, in the tiny letters scaled to the
/// cell (twice their size in 16-pixel cells), filling two cells.
fn draw_tag(fb: &mut Framebuffer, x: i64, y: i64, cell: u32, tag: &str, color: u32) {
    let s = i64::from((cell / 8).max(1));
    let c = i64::from(cell);
    let dark = 0xFF00_0000 | ((color >> 2) & 0x003F_3F3F);
    fill(fb, x, y + s, 2 * c - 2, c - 2 * s, dark);
    let n = tag.len() as i64;
    let width = n * 3 * s + (n - 1) * s;
    let mut gx = x + (2 * c - 2 - width) / 2;
    let gy = y + (c - 5 * s) / 2;
    for ch in tag.chars() {
        for (row, bits) in tiny_glyph(ch).iter().enumerate() {
            for col in 0..3 {
                if bits & (0b100 >> col) != 0 {
                    fill(fb, gx + col * s, gy + row as i64 * s, s, s, color);
                }
            }
        }
        gx += 4 * s;
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
        hints: true,
        bottom: false,
    };

    fn words(c: &Option<Completion>) -> Vec<&str> {
        c.iter()
            .flat_map(|c| c.items.iter().map(|i| i.word.as_str()))
            .collect()
    }

    fn library() -> Library {
        let mut forms = HashMap::new();
        forms.insert("SPOFS".into(), vec!["SPOFS SpriteNumber,X,Y".into()]);
        forms.insert(
            "RGB".into(),
            vec!["RGB(R,G,B)".into(), "RGB Color OUT R,G,B".into()],
        );
        forms.insert("ABS".into(), vec!["ABS(Number)".into()]);
        Library { forms }
    }

    #[test]
    fn words_before_the_cursor() {
        let line = u("A=SPOFS 0:GOTO @lo:?\"PRI");
        assert_eq!(word_before(&line, 7), Some((2, "SPOFS".into())));
        assert_eq!(word_before(&line, 18), Some((15, "@lo".into())));
        assert_eq!(word_before(&line, 9), None, "a number");
        assert_eq!(word_before(&line, line.len()), None, "in a string");
        assert_eq!(word_before(&line, 8), None);
        assert_eq!(word_before(&u("A#"), 2), None, "a type suffix");
        assert_eq!(word_before(&u("?#"), 2), Some((1, "#".into())));
    }

    #[test]
    fn names_of_a_program() {
        let lines = [
            u("@MAIN:VAR SCORE%=0,MAP[10,20]"),
            u("DEF ADD(A,B)"),
            u("DEF MOVE X,Y OUT Z 'move"),
            u("CONST #LIVES=3,#NAME$=\"X\""),
            u("PRINT score%,#RED,NAME$,ADD(1,2)"),
        ];
        let names = names(lines.iter().map(Vec::as_slice));
        let shown: Vec<(&str, Kind, &str)> = (names.iter())
            .map(|i| (i.word.as_str(), i.kind, i.detail.as_str()))
            .collect();
        assert_eq!(
            shown,
            [
                ("@MAIN", Kind::Label, ""),
                ("SCORE%", Kind::Variable, ""),
                ("MAP", Kind::Variable, "[,]"),
                ("ADD", Kind::Def, "(A,B)"),
                ("MOVE", Kind::Sub, "X,Y OUT Z"),
                ("#LIVES", Kind::Constant, "= 3"),
                ("#NAME$", Kind::Constant, "= \"X\""),
                ("#RED", Kind::Constant, ""),
                ("NAME$", Kind::Variable, ""),
            ]
        );
        assert_eq!(names[4].form().as_deref(), Some("MOVE X,Y OUT Z"));
    }

    #[test]
    fn page_marks_leave_the_titles() {
        assert_eq!(
            without_page_mark("RGB(A,R,G,B)(\u{27A1}1/3)"),
            "RGB(A,R,G,B)"
        );
        assert_eq!(without_page_mark("RGB(R,G,B)"), "RGB(R,G,B)");
        assert_eq!(without_page_mark("GOTO @Label (2/2)"), "GOTO @Label");
    }

    #[test]
    fn kinds_from_the_help_or_the_language() {
        let lib = library();
        assert_eq!(lib.kind("SPOFS"), Kind::Command);
        assert_eq!(lib.kind("abs"), Kind::Function);
        assert_eq!(lib.kind("RGB"), Kind::Both);
        assert_eq!(lib.kind("ENDIF"), Kind::Keyword);
        assert_eq!(lib.kind("STEP"), Kind::Keyword);
        // Without the help: the language's table, else an instruction.
        let none = Library::default();
        assert_eq!(none.kind("LEN"), Kind::Function);
        assert_eq!(none.kind("BGMPLAY"), Kind::Command);
    }

    #[test]
    fn lists_what_starts_with_the_word() {
        let lib = library();
        let names = names([u("SPRITE_X=0:@SPIN").as_slice()]);
        let c = Completion::find(&u("SPO"), 3, &ALL, &lib, &names);
        assert_eq!(words(&c), ["SPOFS"]);
        assert_eq!(
            c.unwrap().syntax[0].as_deref(),
            Some("SPOFS SpriteNumber,X,Y")
        );
        let c = Completion::find(&u("sp"), 2, &ALL, &lib, &names);
        assert!(words(&c).contains(&"spofs"));
        assert!(words(&c).contains(&"SPRITE_X"));
        assert!(!words(&c).contains(&"@SPIN"));
        let c = Completion::find(&u("?@S"), 3, &ALL, &lib, &names);
        assert_eq!(words(&c), Vec::<&str>::new(), "one letter after the @");
        let c = Completion::find(&u("?@SP"), 4, &ALL, &lib, &names);
        assert_eq!(words(&c), ["@SPIN"]);
        // Keywords too, and nothing for a whole word alone.
        let c = Completion::find(&u("ENDI"), 4, &ALL, &lib, &[]);
        assert_eq!(c.unwrap().items, [Item::new("ENDIF", Kind::Keyword, "")]);
        assert_eq!(Completion::find(&u("ENDIF"), 5, &ALL, &lib, &[]), None);
        let only_names = Options {
            commands: false,
            ..ALL
        };
        assert_eq!(
            Completion::find(&u("SPO"), 3, &only_names, &lib, &names),
            None
        );
    }

    #[test]
    fn labels_after_goto_and_constants_after_hash() {
        let lib = library();
        let names = names([u("@MAIN:@MENU:@END:MOUSE=1").as_slice()]);
        let commands_only = Options {
            names: false,
            ..ALL
        };
        // Labels whatever the settings, from the first letter or none.
        let c = Completion::find(&u("GOTO M"), 6, &commands_only, &lib, &names);
        assert_eq!(
            (c.as_ref().unwrap().start, words(&c)),
            (5, vec!["@MAIN", "@MENU"])
        );
        let c = Completion::find(&u("ON X GOSUB @MAIN,@"), 18, &commands_only, &lib, &names);
        assert_eq!(words(&c), ["@END", "@MAIN", "@MENU"]);
        let c = Completion::find(&u("RESTORE @E"), 10, &commands_only, &lib, &names);
        assert_eq!(words(&c), ["@END"]);
        let c = Completion::find(&u("MYGOTO MI"), 9, &commands_only, &lib, &names);
        assert!(words(&c).contains(&"MID$"), "not GOTO itself");
        // Predefined constants, with their values.
        let c = Completion::find(&u("?#TR"), 4, &commands_only, &lib, &[]).unwrap();
        assert_eq!(c.items, [Item::new("#TRUE", Kind::Constant, "= 1")]);
        let c = Completion::find(&u("?#C_W"), 5, &commands_only, &lib, &[]).unwrap();
        assert_eq!(c.items[0].detail, "= &HFFFFFFFF");
    }

    #[test]
    fn keys_move_through_the_list() {
        let lib = library();
        let mut c = Completion::find(&u("BGMS"), 4, &ALL, &lib, &[]).unwrap();
        assert_eq!(words(&Some(c.clone())), ["BGMSET", "BGMSETD", "BGMSTOP"]);
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
