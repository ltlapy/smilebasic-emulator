//! Turns SmileBASIC source text into tokens.
//!
//! Identifiers, keywords, labels and constants are case-insensitive and are
//! stored upper-cased. String literal contents keep their case and are stored
//! as UTF-16, the string representation SmileBASIC uses at runtime.

use std::fmt;

/// Position of a token in the source, both 1-based. Columns count characters.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Span {
    pub line: u32,
    pub col: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Token {
    pub kind: TokenKind,
    pub span: Span,
}

/// Type suffix on an identifier: `A%`, `A#`, `A$`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Suffix {
    Int,
    Real,
    Str,
}

#[derive(Clone, Debug, PartialEq)]
pub enum TokenKind {
    Int(i32),
    Real(f64),
    Str(Vec<u16>),
    Ident {
        name: String,
        suffix: Option<Suffix>,
    },
    /// `@NAME`, without the `@`.
    Label(String),
    /// `#NAME`, without the `#`.
    Constant(String),
    Keyword(Keyword),

    Plus,
    Minus,
    Star,
    Slash,
    /// `=`, assignment.
    Assign,
    Eq,
    NotEq,
    Less,
    LessEq,
    Greater,
    GreaterEq,
    /// `<<`, and `<<<`, which the reference defines as identical.
    Shl,
    /// `>>`, arithmetic.
    Shr,
    /// `>>>`, logical.
    UShr,
    /// `<<+`
    RotL,
    /// `>>+`
    RotR,
    /// `!`
    Bang,
    AndAnd,
    OrOr,
    LParen,
    RParen,
    LBracket,
    RBracket,
    Comma,
    Semicolon,
    Colon,

    Newline,
    Eof,
}

macro_rules! keywords {
    ($($name:ident),* $(,)?) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub enum Keyword {
            $($name),*
        }

        impl Keyword {
            /// Every reserved word, as the enum spells it.
            pub const WORDS: &[&str] = &[$(stringify!($name)),*];

            /// Looks up an upper-cased word.
            pub fn from_word(word: &str) -> Option<Keyword> {
                match word {
                    $(w if w.eq_ignore_ascii_case(stringify!($name)) => Some(Keyword::$name),)*
                    _ => None,
                }
            }
        }
    };
}

// The reserved words of the language specification, plus the DIV and MOD
// operators. Other words with special syntax (TO, STEP, CONST, ENUM, OPTION)
// are not reserved, so they stay identifiers and the parser recognizes them
// in context.
keywords!(
    And, Break, Call, Case, Common, Continue, Data, Def, Defout, Dim, Div, Else, Elseif, End,
    Endcase, Endif, Endloop, Exec, For, Gosub, Goto, If, Input, Linput, Loop, Mod, Next, Not, On,
    Or, Otherwise, Out, Print, Read, Repeat, Restore, Return, Swap, Then, Tprint, Until, Var, Wend,
    When, While, Xor,
);

/// Longest spellings first so that `<<<` wins over `<<` and `<`.
const OPERATORS: &[(&str, TokenKind)] = &[
    ("<<<", TokenKind::Shl),
    (">>>", TokenKind::UShr),
    ("<<+", TokenKind::RotL),
    (">>+", TokenKind::RotR),
    ("==", TokenKind::Eq),
    ("!=", TokenKind::NotEq),
    ("<=", TokenKind::LessEq),
    (">=", TokenKind::GreaterEq),
    ("<<", TokenKind::Shl),
    (">>", TokenKind::Shr),
    ("&&", TokenKind::AndAnd),
    ("||", TokenKind::OrOr),
    ("=", TokenKind::Assign),
    ("!", TokenKind::Bang),
    ("<", TokenKind::Less),
    (">", TokenKind::Greater),
    ("+", TokenKind::Plus),
    ("-", TokenKind::Minus),
    ("*", TokenKind::Star),
    ("/", TokenKind::Slash),
    ("(", TokenKind::LParen),
    (")", TokenKind::RParen),
    ("[", TokenKind::LBracket),
    ("]", TokenKind::RBracket),
    (",", TokenKind::Comma),
    (";", TokenKind::Semicolon),
    (":", TokenKind::Colon),
    ("?", TokenKind::Keyword(Keyword::Print)),
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LexErrorKind {
    UnexpectedChar(char),
    /// `&H`, `&B`, `@` or `#` with nothing valid after it.
    MissingDigits,
    /// A hexadecimal or binary literal wider than 32 bits.
    Overflow,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LexError {
    pub kind: LexErrorKind,
    pub span: Span,
}

impl fmt::Display for LexError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let what = match &self.kind {
            LexErrorKind::UnexpectedChar(c) => format!("Syntax error (unexpected {c:?})"),
            LexErrorKind::MissingDigits => "Syntax error".to_string(),
            LexErrorKind::Overflow => "Overflow".to_string(),
        };
        write!(f, "{what} in {}", self.span.line)
    }
}

impl std::error::Error for LexError {}

pub fn tokenize(source: &str) -> Result<Vec<Token>, LexError> {
    Lexer::new(source).run()
}

struct Lexer {
    chars: Vec<char>,
    pos: usize,
    line: u32,
    col: u32,
    tokens: Vec<Token>,
}

fn is_ident_start(c: char) -> bool {
    c.is_ascii_alphabetic() || c == '_'
}

fn is_ident_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

impl Lexer {
    fn new(source: &str) -> Self {
        Self {
            chars: source.chars().collect(),
            pos: 0,
            line: 1,
            col: 1,
            tokens: Vec::new(),
        }
    }

    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    fn peek_at(&self, offset: usize) -> Option<char> {
        self.chars.get(self.pos + offset).copied()
    }

    fn bump(&mut self) -> Option<char> {
        let c = self.peek()?;
        self.pos += 1;
        self.col += 1;
        Some(c)
    }

    fn span(&self) -> Span {
        Span {
            line: self.line,
            col: self.col,
        }
    }

    fn error(&self, kind: LexErrorKind, span: Span) -> LexError {
        LexError { kind, span }
    }

    fn push(&mut self, kind: TokenKind, span: Span) {
        self.tokens.push(Token { kind, span });
    }

    fn skip_to_line_end(&mut self) {
        while !matches!(self.peek(), None | Some('\n' | '\r')) {
            self.bump();
        }
    }

    /// A `\\` joins its line with the next one, and whatever follows it on
    /// the line is left out. The reference does not describe this, but
    /// SmileBASIC 4's bundled programs use it.
    fn continues_line(&mut self) -> bool {
        let mut i = 1;
        while !matches!(self.peek_at(i), None | Some('\r' | '\n')) {
            i += 1;
        }
        let newline_len = match (self.peek_at(i), self.peek_at(i + 1)) {
            (Some('\r'), Some('\n')) => 2,
            (Some('\r' | '\n'), _) => 1,
            _ => return false,
        };
        self.pos += i + newline_len;
        self.line += 1;
        self.col = 1;
        // Blank lines after the backslash are skipped too; a bundled program
        // relies on it. UNVERIFIED: whether comment-only lines are skipped.
        loop {
            let mut j = 0;
            while matches!(self.peek_at(j), Some(' ' | '\t')) {
                j += 1;
            }
            let len = match (self.peek_at(j), self.peek_at(j + 1)) {
                (Some('\r'), Some('\n')) => 2,
                (Some('\r' | '\n'), _) => 1,
                _ => return true,
            };
            self.pos += j + len;
            self.line += 1;
        }
    }

    fn run(mut self) -> Result<Vec<Token>, LexError> {
        while let Some(c) = self.peek() {
            let start = self.span();
            match c {
                ' ' | '\t' => {
                    self.bump();
                }
                '\r' | '\n' => {
                    self.bump();
                    if c == '\r' && self.peek() == Some('\n') {
                        self.bump();
                    }
                    self.push(TokenKind::Newline, start);
                    self.line += 1;
                    self.col = 1;
                }
                '\'' => self.skip_to_line_end(),
                '\\' if self.continues_line() => {}
                '"' => self.string(start),
                '0'..='9' => self.number(start)?,
                '.' if self.peek_at(1).is_some_and(|c| c.is_ascii_digit()) => self.number(start)?,
                '&' if self.peek_at(1) != Some('&') => self.ampersand(start)?,
                '@' => {
                    self.bump();
                    // Unlike other names, a label may start with a digit.
                    // Its case is kept for `A$=@Lp`; as a place to go to,
                    // the parser compares it in upper case.
                    let from = self.pos;
                    self.word_from(is_ident_char)
                        .ok_or(self.error(LexErrorKind::MissingDigits, start))?;
                    let name = self.chars[from..self.pos].iter().collect();
                    self.push(TokenKind::Label(name), start);
                }
                '#' => {
                    self.bump();
                    // Like a label, a constant may start with a digit (`#2P`).
                    let mut name = self
                        .word_from(is_ident_char)
                        .ok_or(self.error(LexErrorKind::MissingDigits, start))?;
                    // Constants can carry a type suffix too (`#LF$`).
                    if let Some(c @ ('$' | '%' | '#')) = self.peek() {
                        name.push(c);
                        self.bump();
                    }
                    self.push(TokenKind::Constant(name), start);
                }
                c if is_ident_start(c) => self.ident(start),
                _ => self.operator(c, start)?,
            }
        }
        let end = self.span();
        self.push(TokenKind::Eof, end);
        Ok(self.tokens)
    }

    /// Reads `[A-Z_][A-Z0-9_]*` and returns it upper-cased.
    fn word(&mut self) -> Option<String> {
        self.word_from(is_ident_start)
    }

    /// Like [`Lexer::word`], with `first` deciding which characters may
    /// start it.
    fn word_from(&mut self, first: fn(char) -> bool) -> Option<String> {
        if !self.peek().is_some_and(first) {
            return None;
        }
        let mut name = String::new();
        while let Some(c) = self.peek().filter(|&c| is_ident_char(c)) {
            name.push(c.to_ascii_uppercase());
            self.bump();
        }
        Some(name)
    }

    fn ident(&mut self, start: Span) {
        let name = self.word().expect("caller checked is_ident_start");
        // `T?` is short for TPRINT, as `?` is for PRINT.
        if name == "T" && self.peek() == Some('?') && self.peek_at(1) != Some('?') {
            self.bump();
            self.push(TokenKind::Keyword(Keyword::Tprint), start);
            return;
        }
        let suffix = match self.peek() {
            Some('%') => Some(Suffix::Int),
            Some('#') => Some(Suffix::Real),
            Some('$') => Some(Suffix::Str),
            _ => None,
        };
        if suffix.is_some() {
            self.bump();
        } else if let Some(keyword) = Keyword::from_word(&name) {
            self.push(TokenKind::Keyword(keyword), start);
            return;
        } else if name == "REM" {
            self.skip_to_line_end();
            return;
        }
        self.push(TokenKind::Ident { name, suffix }, start);
    }

    // UNVERIFIED: a string missing its closing quote ends at the end of the
    // line, and there is no escape for `"` inside a literal (CHR$(34) is the
    // usual workaround).
    fn string(&mut self, start: Span) {
        self.bump();
        let mut text = Vec::new();
        while let Some(c) = self.peek() {
            if matches!(c, '\n' | '\r') {
                break;
            }
            self.bump();
            if c == '"' {
                break;
            }
            // Only the Basic Multilingual Plane is supported; the specification
            // says surrogate pairs are ignored. Lone surrogates the file held
            // come back as themselves.
            if let Some(unit) = crate::wtf8::unit_of(c) {
                text.push(unit);
            } else if c <= '\u{FFFF}' {
                text.extend_from_slice(c.encode_utf16(&mut [0; 2]));
            }
        }
        self.push(TokenKind::Str(text), start);
    }

    fn number(&mut self, start: Span) -> Result<(), LexError> {
        let mut text = String::new();
        let mut is_real = false;
        self.take_digits(&mut text);
        if self.peek() == Some('.') {
            is_real = true;
            text.push('.');
            self.bump();
            self.take_digits(&mut text);
        }
        // Only an exponent when digits follow, so `1ELSE` stays `1` `ELSE`.
        if matches!(self.peek(), Some('e' | 'E')) {
            let digits_at = match self.peek_at(1) {
                Some('+' | '-') => 2,
                _ => 1,
            };
            if self.peek_at(digits_at).is_some_and(|c| c.is_ascii_digit()) {
                is_real = true;
                text.push('E');
                self.bump();
                if digits_at == 2 {
                    text.push(self.bump().expect("sign was peeked"));
                }
                self.take_digits(&mut text);
            }
        }
        // `1_000` is a Syntax error: only `&H` and `&B` take `_`.
        if self.peek() == Some('_') {
            return Err(self.error(LexErrorKind::MissingDigits, start));
        }
        // A `#` after the digits makes the number a real (`0#`).
        if self.peek() == Some('#') && !self.peek_at(1).is_some_and(is_ident_char) {
            is_real = true;
            self.bump();
        }
        // Integer literals go up to 2147483647.
        // A larger decimal literal becomes real instead of an error.
        let kind = match text.parse::<i32>() {
            Ok(n) if !is_real => TokenKind::Int(n),
            _ => TokenKind::Real(text.parse().expect("lexed digits form a valid float")),
        };
        self.push(kind, start);
        Ok(())
    }

    fn take_digits(&mut self, text: &mut String) {
        while let Some(c) = self.peek().filter(char::is_ascii_digit) {
            text.push(c);
            self.bump();
        }
    }

    /// `&H..` or `&B..`.
    fn ampersand(&mut self, start: Span) -> Result<(), LexError> {
        self.bump();
        let radix = match self.peek() {
            Some('h' | 'H') => 16,
            Some('b' | 'B') => 2,
            other => {
                return Err(self.error(LexErrorKind::UnexpectedChar(other.unwrap_or('&')), start));
            }
        };
        self.bump();
        let mut value: u64 = 0;
        let mut digits = 0;
        loop {
            // `_` separates digit groups, as in `&B0000_0001`, and may
            // also lead and repeat, as in `&H_3F__BF_BF`.
            if self.peek() == Some('_') {
                self.bump();
                // It must lead to a digit: `&HFF_` is a Syntax error.
                let mut next = 0;
                while self.peek_at(next) == Some('_') {
                    next += 1;
                }
                if !self.peek_at(next).is_some_and(|c| c.is_digit(radix)) {
                    return Err(self.error(LexErrorKind::MissingDigits, start));
                }
                continue;
            }
            let Some(d) = self.peek().and_then(|c| c.to_digit(radix)) else {
                break;
            };
            self.bump();
            value = value * u64::from(radix) + u64::from(d);
            digits += 1;
            if value > u64::from(u32::MAX) {
                return Err(self.error(LexErrorKind::Overflow, start));
            }
        }
        if digits == 0 {
            return Err(self.error(LexErrorKind::MissingDigits, start));
        }
        // `&HFFFFFFFF` is -1: the 32 bits are reinterpreted as signed.
        self.push(TokenKind::Int(value as u32 as i32), start);
        Ok(())
    }

    fn operator(&mut self, c: char, start: Span) -> Result<(), LexError> {
        let rest = &self.chars[self.pos..];
        // `??` is short for the INSPECT command, as `?` is for PRINT.
        if rest.starts_with(&['?', '?']) {
            self.bump();
            self.bump();
            let name = "INSPECT".to_string();
            self.push(TokenKind::Ident { name, suffix: None }, start);
            return Ok(());
        }
        let (spelling, kind) = OPERATORS
            .iter()
            .find(|(op, _)| op.len() <= rest.len() && op.chars().zip(rest).all(|(a, &b)| a == b))
            .ok_or(self.error(LexErrorKind::UnexpectedChar(c), start))?;
        for _ in 0..spelling.len() {
            self.bump();
        }
        self.push(kind.clone(), start);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use TokenKind as T;

    fn kinds(source: &str) -> Vec<TokenKind> {
        let mut kinds: Vec<_> = tokenize(source)
            .unwrap()
            .into_iter()
            .map(|t| t.kind)
            .collect();
        assert_eq!(kinds.pop(), Some(T::Eof));
        kinds
    }

    fn ident(name: &str) -> TokenKind {
        T::Ident {
            name: name.into(),
            suffix: None,
        }
    }

    fn utf16(s: &str) -> Vec<u16> {
        s.encode_utf16().collect()
    }

    fn error(source: &str) -> LexErrorKind {
        tokenize(source).unwrap_err().kind
    }

    #[test]
    fn integers_and_reals() {
        assert_eq!(
            kinds("0 42 1.5 .5 3. 1E3 2.5e-2"),
            [
                T::Int(0),
                T::Int(42),
                T::Real(1.5),
                T::Real(0.5),
                T::Real(3.0),
                T::Real(1000.0),
                T::Real(0.025)
            ]
        );
    }

    #[test]
    fn decimal_beyond_int_range_is_real() {
        assert_eq!(
            kinds("2147483647 2147483648"),
            [T::Int(i32::MAX), T::Real(2147483648.0)]
        );
    }

    #[test]
    fn hex_and_binary() {
        assert_eq!(
            kinds("&HFF &hff &B101 &HFFFFFFFF"),
            [T::Int(255), T::Int(255), T::Int(5), T::Int(-1)]
        );
        assert_eq!(error("&H100000000"), LexErrorKind::Overflow);
        assert_eq!(error("&H"), LexErrorKind::MissingDigits);
        assert_eq!(error("&X1"), LexErrorKind::UnexpectedChar('X'));
    }

    #[test]
    fn number_followed_by_keyword() {
        assert_eq!(
            kinds("IF A==1THEN B=1ELSE B=2"),
            [
                T::Keyword(Keyword::If),
                ident("A"),
                T::Eq,
                T::Int(1),
                T::Keyword(Keyword::Then),
                ident("B"),
                T::Assign,
                T::Int(1),
                T::Keyword(Keyword::Else),
                ident("B"),
                T::Assign,
                T::Int(2),
            ]
        );
    }

    #[test]
    fn identifiers_are_case_insensitive_with_suffixes() {
        assert_eq!(
            kinds("abc A% x# Name$ _t1"),
            [
                ident("ABC"),
                T::Ident {
                    name: "A".into(),
                    suffix: Some(Suffix::Int)
                },
                T::Ident {
                    name: "X".into(),
                    suffix: Some(Suffix::Real)
                },
                T::Ident {
                    name: "NAME".into(),
                    suffix: Some(Suffix::Str)
                },
                ident("_T1"),
            ]
        );
    }

    #[test]
    fn keywords() {
        assert_eq!(
            kinds("for Endif wend ENDCASE"),
            [
                T::Keyword(Keyword::For),
                T::Keyword(Keyword::Endif),
                T::Keyword(Keyword::Wend),
                T::Keyword(Keyword::Endcase),
            ]
        );
        // Not reserved, so the parser decides from context.
        assert_eq!(kinds("TO STEP"), [ident("TO"), ident("STEP")]);
        assert_eq!(
            kinds("tprint DEFOUT"),
            [T::Keyword(Keyword::Tprint), T::Keyword(Keyword::Defout)]
        );
        // A suffix makes it an ordinary identifier.
        assert_eq!(
            kinds("END$"),
            [T::Ident {
                name: "END".into(),
                suffix: Some(Suffix::Str)
            }]
        );
    }

    #[test]
    fn strings_keep_case_and_end_at_line_end() {
        assert_eq!(
            kinds("\"Hello\" \"한글\""),
            [T::Str(utf16("Hello")), T::Str(utf16("한글"))]
        );
        assert_eq!(
            kinds("\"open\nX"),
            [T::Str(utf16("open")), T::Newline, ident("X")]
        );
        assert_eq!(kinds("\"A😀B\""), [T::Str(utf16("AB"))]);
    }

    #[test]
    fn underscores_in_radix_literals() {
        assert_eq!(
            kinds("&B000_00000_0000_0001 &HFF_FF"),
            [T::Int(1), T::Int(0xFFFF)]
        );
        assert_eq!(
            kinds("&h_FFFF7FFF &h_3F__BF_BF_BF &b____1 &h1___00"),
            [
                T::Int(-32769),
                T::Int(0x3FBF_BFBF),
                T::Int(1),
                T::Int(0x100)
            ]
        );
        assert_eq!(error("&H_"), LexErrorKind::MissingDigits);
        assert_eq!(error("&HFF_"), LexErrorKind::MissingDigits);
        assert_eq!(error("&HFF_:A=1"), LexErrorKind::MissingDigits);
        assert_eq!(error("1_000"), LexErrorKind::MissingDigits);
    }

    #[test]
    fn labels_and_constants_may_start_with_a_digit() {
        assert_eq!(
            kinds("@1ST @_2 #2P"),
            [
                T::Label("1ST".into()),
                T::Label("_2".into()),
                T::Constant("2P".into())
            ]
        );
        assert_eq!(error("@"), LexErrorKind::MissingDigits);
        assert_eq!(error("#"), LexErrorKind::MissingDigits);
    }

    #[test]
    fn short_forms_and_real_suffix() {
        assert_eq!(
            kinds("T?0 0# 1.5#"),
            [
                T::Keyword(Keyword::Tprint),
                T::Int(0),
                T::Real(0.0),
                T::Real(1.5)
            ]
        );
        // Text after a backslash is left out and the lines are joined.
        assert_eq!(kinds("1\\ XYZ\n+2"), [T::Int(1), T::Plus, T::Int(2)]);
    }

    #[test]
    fn labels_and_constants() {
        assert_eq!(
            kinds("@main_loop ?#C_WHITE"),
            [
                T::Label("main_loop".into()),
                T::Keyword(Keyword::Print),
                T::Constant("C_WHITE".into())
            ]
        );
        assert_eq!(error("@ X"), LexErrorKind::MissingDigits);
        assert_eq!(
            kinds("#LF$=#A%"),
            [
                T::Constant("LF$".into()),
                T::Assign,
                T::Constant("A%".into())
            ]
        );
    }

    #[test]
    fn backslash_continues_the_line() {
        assert_eq!(
            kinds("VAR A[]=[1,\\\n2]\nB"),
            [
                T::Keyword(Keyword::Var),
                ident("A"),
                T::LBracket,
                T::RBracket,
                T::Assign,
                T::LBracket,
                T::Int(1),
                T::Comma,
                T::Int(2),
                T::RBracket,
                T::Newline,
                ident("B"),
            ]
        );
        // Line numbers still count physical lines.
        let tokens = tokenize("A \\ \r\n B\nC").unwrap();
        let lines: Vec<_> = tokens.iter().map(|t| t.span.line).collect();
        assert_eq!(lines, [1, 2, 2, 3, 3]);
        assert_eq!(error("A\\B"), LexErrorKind::UnexpectedChar('\\'));
        assert_eq!(kinds("1,\\ 'note\n2"), [T::Int(1), T::Comma, T::Int(2)]);
        assert_eq!(
            kinds("[\\\n\n  \n 1]"),
            [T::LBracket, T::Int(1), T::RBracket]
        );
    }

    #[test]
    fn comments() {
        assert_eq!(
            kinds("A=1 'note\nREM all of this\nREMARK"),
            [
                ident("A"),
                T::Assign,
                T::Int(1),
                T::Newline,
                T::Newline,
                ident("REMARK"),
            ]
        );
    }

    #[test]
    fn operators() {
        assert_eq!(
            kinds("= == != < <= > >= << >> <<< >>> <<+ >>+ ! && || + - * / ( ) [ ] , ; :"),
            [
                T::Assign,
                T::Eq,
                T::NotEq,
                T::Less,
                T::LessEq,
                T::Greater,
                T::GreaterEq,
                T::Shl,
                T::Shr,
                T::Shl,
                T::UShr,
                T::RotL,
                T::RotR,
                T::Bang,
                T::AndAnd,
                T::OrOr,
                T::Plus,
                T::Minus,
                T::Star,
                T::Slash,
                T::LParen,
                T::RParen,
                T::LBracket,
                T::RBracket,
                T::Comma,
                T::Semicolon,
                T::Colon,
            ]
        );
        assert_eq!(error("A=1|2"), LexErrorKind::UnexpectedChar('|'));
    }

    #[test]
    fn spans_track_lines_and_columns() {
        let tokens = tokenize("A=1\r\n  B").unwrap();
        let spans: Vec<_> = tokens.iter().map(|t| (t.span.line, t.span.col)).collect();
        assert_eq!(spans, [(1, 1), (1, 2), (1, 3), (1, 4), (2, 3), (2, 4)]);
    }
}
