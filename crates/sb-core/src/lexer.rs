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
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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
    Shl,
    Shr,
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

// Words with their own syntax. Ordinary commands and functions (PRINT
// excepted, because of its `?` alias) stay identifiers and are resolved by
// the parser.
keywords!(
    And, Break, Call, Case, Common, Const, Continue, Data, Def, Dim, Div, Else, Elseif, End,
    Endcase, Endif, Endloop, Enum, For, Gosub, Goto, If, Loop, Mod, Next, Not, On, Option, Or,
    Otherwise, Out, Print, Repeat, Return, Step, Then, To, Until, Var, Wend, When, While, Xor,
);

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
                '"' => self.string(start),
                '0'..='9' => self.number(start),
                '.' if self.peek_at(1).is_some_and(|c| c.is_ascii_digit()) => self.number(start),
                '&' => self.ampersand(start)?,
                '@' => {
                    self.bump();
                    let name = self
                        .word()
                        .ok_or(self.error(LexErrorKind::MissingDigits, start))?;
                    self.push(TokenKind::Label(name), start);
                }
                '#' => {
                    self.bump();
                    let name = self
                        .word()
                        .ok_or(self.error(LexErrorKind::MissingDigits, start))?;
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
        if !self.peek().is_some_and(is_ident_start) {
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
        let mut text = String::new();
        while let Some(c) = self.peek() {
            if matches!(c, '\n' | '\r') {
                break;
            }
            self.bump();
            if c == '"' {
                break;
            }
            text.push(c);
        }
        self.push(TokenKind::Str(text.encode_utf16().collect()), start);
    }

    fn number(&mut self, start: Span) {
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
        // UNVERIFIED: a decimal literal outside the 32-bit range becomes real.
        let kind = match text.parse::<i32>() {
            Ok(n) if !is_real => TokenKind::Int(n),
            _ => TokenKind::Real(text.parse().expect("lexed digits form a valid float")),
        };
        self.push(kind, start);
    }

    fn take_digits(&mut self, text: &mut String) {
        while let Some(c) = self.peek().filter(char::is_ascii_digit) {
            text.push(c);
            self.bump();
        }
    }

    /// `&H..`, `&B..` or `&&`.
    fn ampersand(&mut self, start: Span) -> Result<(), LexError> {
        self.bump();
        let radix = match self.peek() {
            Some('&') => {
                self.bump();
                self.push(TokenKind::AndAnd, start);
                return Ok(());
            }
            Some('h' | 'H') => 16,
            Some('b' | 'B') => 2,
            other => {
                return Err(self.error(LexErrorKind::UnexpectedChar(other.unwrap_or('&')), start));
            }
        };
        self.bump();
        let mut value: u64 = 0;
        let mut digits = 0;
        while let Some(d) = self.peek().and_then(|c| c.to_digit(radix)) {
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
        self.bump();
        let next = self.peek();
        let (kind, two_chars) = match (c, next) {
            ('=', Some('=')) => (TokenKind::Eq, true),
            ('!', Some('=')) => (TokenKind::NotEq, true),
            ('<', Some('=')) => (TokenKind::LessEq, true),
            ('>', Some('=')) => (TokenKind::GreaterEq, true),
            ('<', Some('<')) => (TokenKind::Shl, true),
            ('>', Some('>')) => (TokenKind::Shr, true),
            ('|', Some('|')) => (TokenKind::OrOr, true),
            ('=', _) => (TokenKind::Assign, false),
            ('!', _) => (TokenKind::Bang, false),
            ('<', _) => (TokenKind::Less, false),
            ('>', _) => (TokenKind::Greater, false),
            ('+', _) => (TokenKind::Plus, false),
            ('-', _) => (TokenKind::Minus, false),
            ('*', _) => (TokenKind::Star, false),
            ('/', _) => (TokenKind::Slash, false),
            ('(', _) => (TokenKind::LParen, false),
            (')', _) => (TokenKind::RParen, false),
            ('[', _) => (TokenKind::LBracket, false),
            (']', _) => (TokenKind::RBracket, false),
            (',', _) => (TokenKind::Comma, false),
            (';', _) => (TokenKind::Semicolon, false),
            (':', _) => (TokenKind::Colon, false),
            ('?', _) => (TokenKind::Keyword(Keyword::Print), false),
            _ => return Err(self.error(LexErrorKind::UnexpectedChar(c), start)),
        };
        if two_chars {
            self.bump();
        }
        self.push(kind, start);
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
    }

    #[test]
    fn labels_and_constants() {
        assert_eq!(
            kinds("@main_loop ?#C_WHITE"),
            [
                T::Label("MAIN_LOOP".into()),
                T::Keyword(Keyword::Print),
                T::Constant("C_WHITE".into())
            ]
        );
        assert_eq!(error("@ X"), LexErrorKind::MissingDigits);
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
            kinds("= == != < <= > >= << >> ! && || + - * / ( ) [ ] , ; :"),
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
