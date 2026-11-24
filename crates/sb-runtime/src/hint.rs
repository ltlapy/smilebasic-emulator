//! Argument hints. While the arguments of an instruction, a function or
//! one of the program's DEFs are typed, every form that the help gives for
//! it shows above the cursor. The argument being typed is marked in each
//! form it fits.

use crate::complete::{BACK, BORDER, DIM, Item, Library, SYNTAX, TEXT, draw_syntax, fill};
use crate::extra_glyphs::Fonts;
use crate::framebuffer::Framebuffer;
use crate::syntax::{Role, highlight, is_ident_char, is_ident_start};
use crate::text::TextScreen;

/// The maximum number of forms shown.
const FORMS: usize = 6;
const MARK: u32 = 0xFF20_2838;

/// The argument of a command that the cursor is typing.
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
    /// The end of the name, for a statement.
    name_end: usize,
    /// Commas and the other separators read so far.
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
                // The text after THEN and ELSE is a separate statement.
                "THEN" | "ELSE" if statement => *frame = Frame::default(),
                "OUT" => {
                    frame.out = true;
                    frame.commas = 0;
                }
                // These separate arguments, the same as a comma does: FOR's
                // end value and step, the labels after ON or IF's GOTO.
                "TO" | "STEP" | "GOTO" | "GOSUB" if frame.name.is_some() => frame.commas += 1,
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
            // PRINT and INPUT separate arguments with `;` too.
            0x2C | 0x3B => stack.last_mut().expect("never empty").commas += 1,
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
pub type Spans = Vec<(usize, usize)>;

/// The arguments of a form after the command's name, as character ranges
/// of the form.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Arguments {
    /// Those before OUT.
    pub ins: Spans,
    /// Those after OUT.
    pub outs: Spans,
    /// Whether the last of `ins` repeats, which a form writes as `...`.
    pub ins_repeat: bool,
    /// Whether the last of `outs` repeats.
    pub outs_repeat: bool,
    /// Whether the help writes the form as a function call.
    pub call: bool,
}

/// Words of a form that separate its arguments, the same as a comma does.
const SEPARATORS: &[&str] = &["TO", "STEP", "GOTO", "GOSUB", "THEN", "ELSE", "ENDIF"];

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

fn is_suffix(c: char) -> bool {
    matches!(c, '$' | '#' | '%')
}

/// Returns the end of the command's name in a form: after the name as a
/// whole word, or else after the first word of the form, as for `ARRAY%(`
/// among the forms of ARRAY# and `VAR` among the forms of DIM.
fn name_end(chars: &[char], name: &str) -> usize {
    let name: Vec<char> = name.chars().collect();
    let n = name.len();
    if n > 0 && n <= chars.len() {
        for at in 0..=chars.len() - n {
            let fits =
                (chars[at..at + n].iter().zip(&name)).all(|(a, b)| a.eq_ignore_ascii_case(b));
            let before = at == 0 || !is_word_char(chars[at - 1]);
            let after = (chars.get(at + n)).is_none_or(|&c| !is_word_char(c) && !is_suffix(c));
            if fits && before && after {
                return at + n;
            }
        }
    }
    let start = chars.iter().take_while(|c| c.is_whitespace()).count();
    let mut i = start
        + chars[start..]
            .iter()
            .take_while(|&&c| is_word_char(c))
            .count();
    if i == start {
        // Not a command's form, such as REM's `'`.
        return chars.len();
    }
    if chars.get(i).copied().is_some_and(is_suffix) {
        i += 1;
    }
    i
}

/// Whether a character of a form is not part of an argument: a space, or
/// one of the icons of SmileBASIC 4's font that the help puts after some
/// forms (in Unicode's private use area).
fn is_blank(c: char) -> bool {
    c.is_whitespace() || ('\u{E000}'..='\u{F8FF}').contains(&c)
}

/// Whether the `[` at `at` is part of an argument rather than around an
/// optional one: an array's size (`Name[[Size]]`) or an array literal
/// (`=[Value1,Value2]`).
fn is_literal_bracket(chars: &[char], at: usize) -> bool {
    let Some(&before) = at.checked_sub(1).and_then(|b| chars.get(b)) else {
        return false;
    };
    before == '='
        || (is_word_char(before)
            && chars.get(at + 1) == Some(&'[')
            && chars.get(at + 2).is_some_and(|c| c.is_alphabetic()))
}

/// The `]` that closes the `[` at `at`.
fn closing_bracket(chars: &[char], at: usize) -> Option<usize> {
    let mut depth = 0usize;
    for (i, &c) in chars.iter().enumerate().skip(at) {
        match c {
            '[' => depth += 1,
            ']' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
    }
    None
}

/// One argument being read: the text between two separators, without the
/// brackets of optional groups and the `...` around it.
#[derive(Default)]
struct Piece {
    start: Option<usize>,
    end: usize,
    /// The last `[` before the argument's text.
    lead: Option<usize>,
    /// Brackets opened within the text and not yet closed.
    inner: usize,
    /// A `]` within the text closed a group opened before it, as in
    /// `[Slot:]Line`; if more text follows, the group belongs to it.
    closed_lead: bool,
    dots: bool,
}

impl Piece {
    fn text(&mut self, from: usize, to: usize) {
        if self.closed_lead {
            self.start = self.lead.take().or(self.start);
            self.closed_lead = false;
        }
        self.start.get_or_insert(from);
        self.end = to;
    }

    fn open(&mut self, at: usize) {
        if self.start.is_none() {
            self.lead = Some(at);
        } else {
            self.inner += 1;
        }
    }

    fn close(&mut self, at: usize) {
        if self.inner > 0 {
            self.inner -= 1;
            self.end = at + 1;
        } else if self.start.is_some() {
            self.closed_lead = true;
        }
    }
}

impl Arguments {
    /// Ends the argument being read, adding it to the list it is in.
    fn finish(&mut self, piece: &mut Piece, chars: &[char], out: bool) {
        let (list, repeat) = match out {
            true => (&mut self.outs, &mut self.outs_repeat),
            false => (&mut self.ins, &mut self.ins_repeat),
        };
        if let Some(start) = piece.start {
            // PRINT's `[;Or,Expression]` means either separator is allowed.
            if !matches!(chars[start..piece.end], ['O' | 'o', 'r']) {
                list.push((start, piece.end));
            }
        }
        *repeat |= piece.dots;
        *piece = Piece::default();
    }
}

/// The arguments of a form after the command's name. Commas and `;`
/// separate them, and so do OUT and the words of [`SEPARATORS`]. The
/// brackets of optional arguments and `...` are not part of an argument;
/// a quoted argument includes everything between its quotes.
pub fn arguments(form: &str, name: &str) -> Arguments {
    let chars: Vec<char> = form.chars().collect();
    let len = chars.len();
    let mut args = Arguments::default();
    let mut i = name_end(&chars, name);
    let blank = chars[i..].iter().take_while(|c| c.is_whitespace()).count();
    if chars.get(i + blank) == Some(&'(') {
        args.call = true;
        i += blank + 1;
    }
    let mut piece = Piece::default();
    let mut out = false;
    // Parentheses opened within the arguments.
    let mut depth = 0usize;
    while i < len {
        let c = chars[i];
        match c {
            '"' => {
                // A quote the help never closes is not a string.
                let close = chars[i + 1..].iter().position(|&c| c == '"');
                let to = close.map_or(i + 1, |at| i + at + 2);
                piece.text(i, to);
                i = to;
                continue;
            }
            '[' if is_literal_bracket(&chars, i) => match closing_bracket(&chars, i) {
                Some(close) => {
                    piece.text(i, close + 1);
                    i = close + 1;
                    continue;
                }
                None => piece.open(i),
            },
            '[' => piece.open(i),
            ']' => piece.close(i),
            '(' => {
                depth += 1;
                piece.text(i, i + 1);
            }
            ')' if depth == 0 && args.call => break,
            ')' => {
                depth = depth.saturating_sub(1);
                piece.text(i, i + 1);
            }
            ',' | ';' if depth == 0 => args.finish(&mut piece, &chars, out),
            '.' | '…' if depth == 0 => piece.dots = true,
            c if is_blank(c) => {}
            c if is_word_char(c) => {
                let start = i;
                while i < len && is_word_char(chars[i]) {
                    i += 1;
                }
                if i < len && is_suffix(chars[i]) {
                    i += 1;
                }
                let word: String = chars[start..i].iter().collect();
                if depth == 0 && word == "OUT" {
                    args.finish(&mut piece, &chars, out);
                    out = true;
                } else if depth == 0 && SEPARATORS.contains(&word.as_str()) {
                    args.finish(&mut piece, &chars, out);
                } else {
                    piece.text(start, i);
                }
                continue;
            }
            _ => piece.text(i, i + 1),
        }
        i += 1;
    }
    args.finish(&mut piece, &chars, out);
    args
}

/// The band where the hints are drawn when they show at the bottom: pixel
/// columns from `left`, rows from `top` to `bottom`, and the cursor's row at
/// `cursor`.
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
        let args = arguments(form, &self.name);
        if args.call != self.call {
            return None;
        }
        let (list, repeats) = match self.out {
            true => (&args.outs, args.outs_repeat),
            false => (&args.ins, args.ins_repeat),
        };
        // `...` repeats the last argument.
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

    /// The argument marked in `form`, of the help's topic `name`, with
    /// `typed` up to the cursor.
    fn marked(name: &str, form: &str, typed: &str) -> Option<String> {
        let library = Library::with_forms(&[(name, &[form])]);
        let line = u(typed);
        let (a, b) = find(&line, line.len(), &library, &[])?.mark(form)?;
        Some(form.chars().skip(a).take(b - a).collect())
    }

    #[test]
    fn marks_the_argument_of_each_kind_of_form() {
        let load = "LOAD \"[FileType:]FileName\"[,Slot]";
        let beep = "BEEP [SoundEffectNumber[,[Pitch][,[Volume][,[Panpot]]]]]";
        let call = "CALL CommandName[,Parameter...][ OUT Variable1[,Variable2...]]";
        let toy = "XCTRLSTYLE MaxNumberOfConnections,100,Toy-ConTypeLeft1,Toy-ConTypeRight1";
        #[rustfmt::skip]
        let cases: &[(&str, &str, &str, Option<&str>)] = &[
            // A quoted argument with brackets inside, and an optional one.
            ("LOAD", load, "LOAD", None),
            ("LOAD", load, "LOAD ", Some("\"[FileType:]FileName\"")),
            ("LOAD", load, "LOAD \"AB", Some("\"[FileType:]FileName\"")),
            ("LOAD", load, "LOAD \"A,B", Some("\"[FileType:]FileName\"")),
            ("LOAD", load, "LOAD \"AB\"", Some("\"[FileType:]FileName\"")),
            ("LOAD", load, "LOAD \"AB\",", Some("Slot")),
            ("LOAD", load, "LOAD \"AB\",1,", None),
            ("RENAME", "RENAME \"[FileType1:]FileName1\",\"[FileType2:]FileName2\"",
                "RENAME \"A\",", Some("\"[FileType2:]FileName2\"")),
            // Nested optional groups, and a leading one.
            ("BEEP", beep, "BEEP ", Some("SoundEffectNumber")),
            ("BEEP", beep, "BEEP 1,,", Some("Volume")),
            ("BEEP", beep, "BEEP 1,,,", Some("Panpot")),
            ("ATTR", "ATTR [ScreenID,]HorizontalFlip,VerticalFlip,Rotation",
                "ATTR 0,", Some("HorizontalFlip")),
            ("COPY", "COPY Destination,Source[[,SourceOffset],Count]",
                "COPY A,B,", Some("SourceOffset")),
            ("LIST", "LIST [ProgramSlot:]LineNumber", "LIST ", Some("[ProgramSlot:]LineNumber")),
            // Functions, and the forms a call does not fit.
            ("RGB", "RGB(A,R,G,B)", "C=RGB(", Some("A")),
            ("RGB", "RGB(A,R,G,B)", "C=RGB(1,2,", Some("G")),
            ("RGB", "RGB ColorCode OUT A,R,G,B", "C=RGB(", None),
            ("RND", "RND([SeedID,]MaximumValue)", "X=RND(0,", Some("MaximumValue")),
            ("ATTR", "ATTR( [ScreenID])", "A=ATTR(", Some("ScreenID")),
            ("FORMAT$", "FORMAT$(FormatString,Value,...)", "S$=FORMAT$(F$,1,2,", Some("Value")),
            ("BIQUAD", "BIQUAD Output(OUT),Input(IN),Coefficients(FP)",
                "BIQUAD A,", Some("Input(IN)")),
            // OUT forms.
            ("RGB", "RGB ColorCode OUT A,R,G,B", "RGB C OUT ", Some("A")),
            ("RGB", "RGB ColorCode OUT A,R,G,B", "RGB C OUT A,", Some("R")),
            ("RGB", "RGB ColorCode OUT A,R,G,B", "RGB C,", None),
            ("CALL", call, "CALL \"F\",1,2,", Some("Parameter")),
            ("CALL", call, "CALL \"F\" OUT A,", Some("Variable2")),
            ("TCSUBM", "TCSUBM ControllerID, OUT RecognitionFlag[,Accelerator]",
                "TCSUBM 0 OUT ", Some("RecognitionFlag")),
            ("DTREAD", "DTREAD OUT Y,M,D[,W]", "DTREAD OUT Y,M,D,", Some("W")),
            // Arguments the help writes with more than a name.
            ("CONST", "CONST #Name=Value[,#Name=Value]", "CONST #A=1,", Some("#Name=Value")),
            ("ENUM", "ENUM #Name[=Value][,#Name[=Value]]", "ENUM ", Some("#Name[=Value]")),
            ("XCTRLSTYLE", toy, "XCTRLSTYLE 1,100,", Some("Toy-ConTypeLeft1")),
            ("ON", "ON Expression GOTO @Label0,@Label1…", "ON X GOTO @A,@B,", Some("@Label1")),
            // Separators other than commas.
            ("PRINT", "PRINT [Expression[;Or,Expression...]]", "PRINT 1;", Some("Expression")),
            ("INPUT", "INPUT [\"GuideString\";]Variable[,Variable2...]",
                "INPUT \"Q\";", Some("Variable")),
            ("FOR", "FOR LoopVariable=InitialValue TO EndValue STEP Increment",
                "FOR I=0 TO ", Some("EndValue")),
            ("FOR", "FOR LoopVariable=InitialValue TO EndValue STEP Increment",
                "FOR I=0 TO 9 STEP ", Some("Increment")),
            ("FOR", "FOR LoopVariable=InitialValue TO EndValue",
                "FOR I=0 TO 9 STEP ", None),
            // An array's size is part of the argument, and has no hint.
            ("DIM", "DIM Name[[Size[,Size...]]]", "DIM ", Some("Name[[Size[,Size...]]]")),
            ("DIM", "DIM Name[[Size[,Size...]]]", "DIM A[", None),
            // A form named differently from the topic, and one with an icon.
            ("DIM", "VAR Variable[=Expression][,Variable[=Expression]...]",
                "DIM A,", Some("Variable[=Expression]")),
            ("ARRAY#", "ARRAY%(Size[,Size...])", "A=ARRAY#(", Some("Size")),
            ("PCMSTREAM", "PCMSTREAM Array[,Rate] \u{E2CB}", "PCMSTREAM A,", Some("Rate")),
            // A quote the help leaves open.
            ("FILES", "FILES \"[FileType:]ProjectName,StringArray",
                "FILES \"A\",", Some("StringArray")),
        ];
        for &(name, form, typed, want) in cases {
            let got = marked(name, form, typed);
            assert_eq!(got.as_deref(), want, "{form} with {typed:?}");
        }
    }

    #[test]
    fn splits_a_form_into_arguments() {
        let form = "SPOFS SpriteNumber[,Flag] OUT X,Y[,Z...]";
        let args = arguments(form, "SPOFS");
        assert_eq!(args.ins, [(6, 18), (20, 24)]);
        assert_eq!(args.outs, [(30, 31), (32, 33), (35, 36)]);
        assert!(!args.ins_repeat && args.outs_repeat && !args.call);
        assert_eq!(arguments("'", "REM"), Arguments::default());
        assert!(arguments("DATE$()", "DATE$").call);
    }
}
