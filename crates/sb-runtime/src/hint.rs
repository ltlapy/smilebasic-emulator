//! Argument hints: while the arguments of an instruction, a function or
//! one of the program's DEFs are typed, every form the help gives for it
//! shows above the cursor, with the argument being typed marked in each
//! form it fits.

use crate::complete::{BACK, BORDER, DIM, Item, Library, SYNTAX, TEXT, draw_syntax, fill};
use crate::extra_glyphs::Fonts;
use crate::framebuffer::Framebuffer;
use crate::syntax::{Role, highlight, is_ident_char, is_ident_start};
use crate::text::TextScreen;

/// Forms shown at most.
const FORMS: usize = 6;
const MARK: u32 = 0xFF20_2838;

/// What the cursor is typing: an argument of a command.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hint {
    pub name: String,
    pub forms: Vec<String>,
    /// The argument's place among those before OUT, or among the OUT
    /// variables.
    pub arg: usize,
    pub out: bool,
    /// Inside the parentheses of a function call.
    pub call: bool,
}

/// An open statement or parenthesis while reading a line.
#[derive(Clone, Debug, Default)]
struct Frame {
    name: Option<String>,
    /// Where the name ends, for a statement.
    name_end: usize,
    commas: usize,
    out: bool,
    call: bool,
}

/// The command whose arguments the cursor at `col` is in, with its forms
/// from the help or from the program's DEFs (`names`).
pub fn find(line: &[u16], col: usize, library: &Library, names: &[Item]) -> Option<Hint> {
    let col = col.min(line.len());
    let (roles, _) = highlight(line);
    let forms_of = |name: &str| -> Vec<String> {
        let upper = name.to_ascii_uppercase();
        let def = names
            .iter()
            .find(|i| i.word.to_ascii_uppercase() == upper)
            .and_then(Item::form);
        match def {
            Some(form) => vec![form],
            None => library.forms(&upper).to_vec(),
        }
    };
    let mut stack = vec![Frame::default()];
    // The word just read, for a `(` that follows it.
    let mut last_word: Option<String> = None;
    let mut i = 0;
    while i < col {
        let c = line[i];
        if matches!(roles[i], Role::String | Role::Comment) {
            last_word = None;
            i += 1;
            continue;
        }
        if is_ident_start(c) {
            let start = i;
            while i < col && is_ident_char(line[i]) {
                i += 1;
            }
            if i < col && matches!(line[i], 0x23..=0x25) {
                i += 1;
            }
            let word = String::from_utf16_lossy(&line[start..i]).to_ascii_uppercase();
            let statement = stack.len() == 1;
            let frame = stack.last_mut().expect("never empty");
            match word.as_str() {
                // What follows THEN and ELSE is a statement of its own.
                "THEN" | "ELSE" if statement => *frame = Frame::default(),
                "OUT" => {
                    frame.out = true;
                    frame.commas = 0;
                }
                _ if statement && frame.name.is_none() => {
                    frame.name = Some(word.clone());
                    frame.name_end = i;
                }
                _ => {}
            }
            last_word = Some(word);
            continue;
        }
        match c {
            0x28 => stack.push(Frame {
                name: last_word.take(),
                call: true,
                ..Frame::default()
            }),
            // An array's index has no hint.
            0x5B => stack.push(Frame::default()),
            0x29 | 0x5D if stack.len() > 1 => {
                stack.pop();
            }
            0x2C => stack.last_mut().expect("never empty").commas += 1,
            0x3A => stack = vec![Frame::default()],
            _ => {}
        }
        if c != 0x20 {
            last_word = None;
        }
        i += 1;
    }
    for (depth, frame) in stack.iter().enumerate().rev() {
        // Inside an index or a bracketed expression, no hint.
        let Some(name) = &frame.name else {
            if depth > 0 {
                return None;
            }
            continue;
        };
        // A statement's arguments start after a space.
        if depth == 0 && col <= frame.name_end {
            return None;
        }
        let forms = forms_of(name);
        if forms.is_empty() {
            if depth > 0 {
                return None;
            }
            continue;
        }
        return Some(Hint {
            name: name.clone(),
            forms,
            arg: frame.commas,
            out: frame.out,
            call: frame.call,
        });
    }
    None
}

/// Character ranges of a form.
type Spans = Vec<(usize, usize)>;

/// The argument names of a form after the command's name: those before
/// OUT and those after, as character ranges of the form.
fn arguments(form: &str, name: &str) -> (Spans, Spans) {
    let chars: Vec<char> = form.chars().collect();
    let upper: String = form.to_ascii_uppercase();
    let after_name = upper
        .find(name)
        .map_or(0, |at| form[..at + name.len()].chars().count());
    let (mut ins, mut outs) = (Vec::new(), Vec::new());
    let mut out = false;
    let mut quoted = false;
    let mut i = after_name;
    while i < chars.len() {
        let c = chars[i];
        if c == '"' {
            quoted = !quoted;
        }
        if quoted || !(c.is_alphanumeric() || c == '_') {
            i += 1;
            continue;
        }
        let start = i;
        while i < chars.len() && (chars[i].is_alphanumeric() || "_$%#".contains(chars[i])) {
            i += 1;
        }
        let word: String = chars[start..i].iter().collect();
        if word == "OUT" {
            out = true;
        } else if out {
            outs.push((start, i));
        } else {
            ins.push((start, i));
        }
    }
    (ins, outs)
}

/// Whether the help writes this form as a function call.
fn is_call(form: &str, name: &str) -> bool {
    let upper = form.to_ascii_uppercase();
    upper
        .find(name)
        .is_some_and(|at| upper[at + name.len()..].trim_start().starts_with('('))
}

/// Where the hints go when they show at the bottom: pixel columns from
/// `left`, rows from `top` to `bottom`, and the cursor's row at `cursor`.
#[derive(Clone, Copy, Debug)]
pub struct Band {
    pub left: i64,
    pub top: i64,
    pub bottom: i64,
    pub cursor: i64,
}

/// Draws the hint for the command being typed over the form of
/// completion's chosen entry, at the bottom of the band, or at its top
/// while the cursor's row is down there.
pub fn draw_band(
    fb: &mut Framebuffer,
    fonts: Fonts,
    band: Band,
    (hint, syntax): (Option<&Hint>, Option<&str>),
    cell: u32,
) {
    let c = i64::from(cell);
    let hint_h = hint.map_or(0, |h| h.forms.len().min(FORMS) as i64 * c + 2);
    let syntax_h = if syntax.is_some() { c + 2 } else { 0 };
    let mut y = band.bottom - hint_h - syntax_h;
    if band.cursor + c > y && band.cursor < band.bottom {
        y = band.top;
    }
    if let Some(hint) = hint {
        hint.draw(fb, fonts, band.left + 1, (y + hint_h, y + 1), cell);
    }
    if let Some(syntax) = syntax {
        let cols = (syntax.encode_utf16().count() as i64 + 2).min(i64::from(fb.width()) / c);
        let x = (band.left + 1)
            .min(i64::from(fb.width()) - cols * c - 1)
            .max(1);
        let at = (x, y + hint_h + 1);
        draw_syntax(fb, fonts, at, cols, cell, syntax);
    }
}

impl Hint {
    /// The marked argument in a form, as a character range, or `None` if
    /// the form does not fit what is typed.
    pub fn mark(&self, form: &str) -> Option<(usize, usize)> {
        if is_call(form, &self.name) != self.call {
            return None;
        }
        let (ins, outs) = arguments(form, &self.name);
        let list = if self.out { outs } else { ins };
        // `...` repeats the last argument.
        let repeats = form.contains("...");
        list.get(self.arg)
            .or_else(|| list.last().filter(|_| repeats))
            .copied()
    }

    /// Draws the forms in a box whose bottom is at pixel row `bottom`, or
    /// whose top is at `below` if there is no room above, starting at
    /// column `x`, in cells of `cell` pixels. Forms that do not fit what
    /// is typed are dimmed.
    pub fn draw(
        &self,
        fb: &mut Framebuffer,
        fonts: Fonts,
        x: i64,
        (bottom, below): (i64, i64),
        cell: u32,
    ) {
        let c = i64::from(cell);
        let forms: Vec<&String> = self.forms.iter().take(FORMS).collect();
        let longest = forms.iter().map(|f| f.chars().count()).max().unwrap_or(0);
        let cols = (longest as i64 + 2).min(i64::from(fb.width()) / c);
        let (w, h) = (cols * c, forms.len() as i64 * c);
        let y = if bottom - h - 2 < 0 {
            below
        } else {
            bottom - h - 1
        };
        let x = x.min(i64::from(fb.width()) - w - 1).max(1);
        fill(fb, x - 1, y - 1, w + 2, h + 2, BORDER);
        fill(fb, x, y, w, h, BACK);
        let mut screen = TextScreen::new(0, 0);
        screen.set_font(cell, cell, cols as u32, forms.len() as u32);
        screen.pos = (x as f64, y as f64);
        for (row, form) in forms.iter().enumerate() {
            let mark = self.mark(form);
            for (i, ch) in form.encode_utf16().enumerate() {
                let marked = mark.is_some_and(|(a, b)| a <= i && i < b);
                let color = match (mark, marked) {
                    (None, _) => DIM,
                    (_, true) => SYNTAX,
                    (_, false) => TEXT,
                };
                if marked {
                    let (px, py) = (x + (i as i64 + 1) * c, y + row as i64 * c);
                    fill(fb, px, py + c - c / 8, c, (c / 8).max(1), SYNTAX);
                    fill(fb, px, py, c, c - c / 8, MARK);
                }
                screen.put_at(i as u32 + 1, row as u32, ch, color);
            }
        }
        fonts.draw_screen(&mut screen, fb);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::complete::names;

    fn u(text: &str) -> Vec<u16> {
        text.encode_utf16().collect()
    }

    fn library() -> Library {
        Library::with_forms(&[
            (
                "SPOFS",
                &[
                    "SPOFS SpriteNumber,[X],[Y][,Z]",
                    "SPOFS SpriteNumber[,Flag] OUT X,Y[,Z]",
                ],
            ),
            (
                "RGB",
                &["RGB(R,G,B)", "RGB(A,R,G,B)", "RGB Color OUT R,G,B"],
            ),
            ("PRINT", &["PRINT [Expression][;or,Expression...]"]),
        ])
    }

    fn hint(text: &str) -> Option<(String, usize, bool, bool)> {
        let line = u(text);
        let names = names([line.as_slice()]);
        find(&line, line.len(), &library(), &names).map(|h| (h.name, h.arg, h.out, h.call))
    }

    #[test]
    fn finds_the_argument_being_typed() {
        let s = |n: &str, a, o, c| Some((n.to_string(), a, o, c));
        assert_eq!(hint("SPOFS 0,"), s("SPOFS", 1, false, false));
        assert_eq!(hint("SPOFS"), None, "no argument yet");
        assert_eq!(hint("SPOFS 0 OUT X,"), s("SPOFS", 1, true, false));
        assert_eq!(hint("C=RGB(255,"), s("RGB", 1, false, true));
        assert_eq!(hint("SPOFS 0,RGB(1,2),"), s("SPOFS", 2, false, false));
        assert_eq!(hint("SPOFS A[1,"), None, "in an index");
        assert_eq!(hint("IF X THEN SPOFS 1,"), s("SPOFS", 1, false, false));
        assert_eq!(hint("SPOFS 1:?"), None);
        assert_eq!(hint("SPOFS \"A,B\","), s("SPOFS", 1, false, false));
        // The program's DEFs, by their own line.
        assert_eq!(hint("DEF MOVE X,Y:END:MOVE 1,"), s("MOVE", 1, false, false));
    }

    #[test]
    fn marks_the_argument_in_each_form() {
        let lib = library();
        let line = u("SPOFS 0,");
        let h = find(&line, line.len(), &lib, &[]).unwrap();
        let forms = &h.forms;
        // [X] is the second argument; the OUT form has only one before OUT.
        assert_eq!(h.mark(&forms[0]), Some((20, 21)));
        assert_eq!(h.mark(&forms[1]), Some((20, 24)), "the flag");
        let line = u("RGB(1,2,3,");
        let h = find(&line, line.len(), &lib, &[]).unwrap();
        assert_eq!(h.mark("RGB(R,G,B)"), None, "too many");
        assert_eq!(h.mark("RGB(A,R,G,B)"), Some((10, 11)));
        assert_eq!(h.mark("RGB Color OUT R,G,B"), None, "not a call");
        let line = u("PRINT 1;2;3,");
        let h = find(&line, line.len(), &lib, &[]).unwrap();
        assert!(h.mark(&h.forms[0]).is_some(), "... repeats");
    }
}
