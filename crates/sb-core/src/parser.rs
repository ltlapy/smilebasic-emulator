//! Recursive-descent parser from tokens to [`ast::Program`].

use std::fmt;

use crate::ast::*;
use crate::lexer::{self, Keyword, LexError, LexErrorKind, Span, Token, TokenKind};

#[derive(Clone, Debug, PartialEq)]
pub enum ParseErrorKind {
    Lex(LexErrorKind),
    Expected {
        expected: &'static str,
        found: TokenKind,
    },
    /// A keyword that is only valid somewhere else, such as `WEND` without `WHILE`.
    Misplaced(Keyword),
    DefInsideBlock,
    /// Nesting deeper than [`MAX_DEPTH`].
    TooDeep,
}

/// Limit on nested expressions and blocks, so hostile input cannot overflow
/// the stack. UNVERIFIED: SmileBASIC's own limit is not documented.
pub const MAX_DEPTH: u32 = 256;

#[derive(Clone, Debug, PartialEq)]
pub struct ParseError {
    pub kind: ParseErrorKind,
    pub span: Span,
    /// Where the innermost statement around the error starts.
    pub stmt: Option<Span>,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let line = self.span.line;
        match &self.kind {
            ParseErrorKind::Lex(kind) => LexError {
                kind: kind.clone(),
                span: self.span,
            }
            .fmt(f),
            ParseErrorKind::Expected { expected, found } => {
                write!(
                    f,
                    "Syntax error in {line} (expected {expected}, found {found:?})"
                )
            }
            ParseErrorKind::Misplaced(keyword) => {
                write!(f, "Syntax error in {line} (unexpected {keyword:?})")
            }
            ParseErrorKind::DefInsideBlock => {
                write!(f, "Syntax error in {line} (DEF inside a block)")
            }
            ParseErrorKind::TooDeep => write!(f, "Expression too complex in {line}"),
        }
    }
}

impl std::error::Error for ParseError {}

impl ParseError {
    /// Context for developers, without the error message and line.
    pub fn detail(&self) -> Option<String> {
        match &self.kind {
            ParseErrorKind::Lex(LexErrorKind::UnexpectedChar(c)) => {
                Some(format!("unexpected {c:?}"))
            }
            ParseErrorKind::Lex(_) => None,
            ParseErrorKind::Expected { expected, found } => {
                Some(format!("expected {expected}, found {found:?}"))
            }
            ParseErrorKind::Misplaced(keyword) => Some(format!("unexpected {keyword:?}")),
            ParseErrorKind::DefInsideBlock => Some("DEF inside a block".to_string()),
            ParseErrorKind::TooDeep => Some("expression too complex".to_string()),
        }
    }
}

impl From<LexError> for ParseError {
    fn from(e: LexError) -> Self {
        ParseError {
            kind: ParseErrorKind::Lex(e.kind),
            span: e.span,
            stmt: None,
        }
    }
}

pub fn parse(source: &str) -> Result<Program, ParseError> {
    let tokens = lexer::tokenize(source)?;
    Parser {
        tokens,
        pos: 0,
        in_def: false,
        loops: Vec::new(),
        depth: 0,
    }
    .program()
}

/// Parses one expression and nothing after it, for a debugger's watch.
pub fn parse_expression(source: &str) -> Result<Expr, ParseError> {
    let tokens = lexer::tokenize(source)?;
    let mut parser = Parser {
        tokens,
        pos: 0,
        in_def: false,
        loops: Vec::new(),
        depth: 0,
    };
    let expr = parser.expr()?;
    while matches!(parser.peek(), TokenKind::Newline) {
        parser.pos += 1;
    }
    if !matches!(parser.peek(), TokenKind::Eof) {
        return parser.expected("the end of the expression");
    }
    Ok(expr)
}

type PResult<T> = Result<T, ParseError>;

struct Parser {
    tokens: Vec<Token>,
    pos: usize,
    in_def: bool,
    depth: u32,
    /// The closing keywords of the loops being parsed, innermost last.
    loops: Vec<Keyword>,
}

/// Operator and its precedence level, 0 binding loosest.
fn binary_op(kind: &TokenKind) -> Option<(BinaryOp, u8)> {
    use BinaryOp as B;
    use TokenKind as T;
    // Lowest precedence first, as in the operator table of the reference.
    let op = match kind {
        T::OrOr => (B::LogicalOr, 0),
        T::AndAnd => (B::LogicalAnd, 1),
        T::Keyword(Keyword::Or) => (B::Or, 2),
        T::Keyword(Keyword::Xor) => (B::Xor, 2),
        T::Keyword(Keyword::And) => (B::And, 3),
        T::Eq => (B::Eq, 4),
        T::NotEq => (B::NotEq, 4),
        T::Less => (B::Less, 4),
        T::LessEq => (B::LessEq, 4),
        T::Greater => (B::Greater, 4),
        T::GreaterEq => (B::GreaterEq, 4),
        T::Shl => (B::Shl, 5),
        T::Shr => (B::Shr, 5),
        T::UShr => (B::UShr, 5),
        T::RotL => (B::RotL, 5),
        T::RotR => (B::RotR, 5),
        T::Plus => (B::Add, 6),
        T::Minus => (B::Sub, 6),
        T::Star => (B::Mul, 7),
        T::Slash => (B::Div, 7),
        T::Keyword(Keyword::Div) => (B::IntDiv, 7),
        T::Keyword(Keyword::Mod) => (B::Mod, 7),
        _ => return None,
    };
    Some(op)
}

impl Parser {
    // ---- token access ----

    fn peek(&self) -> &TokenKind {
        &self.tokens[self.pos].kind
    }

    fn peek_at(&self, offset: usize) -> &TokenKind {
        let i = (self.pos + offset).min(self.tokens.len() - 1);
        &self.tokens[i].kind
    }

    fn span(&self) -> Span {
        self.tokens[self.pos].span
    }

    fn advance(&mut self) -> TokenKind {
        let kind = self.tokens[self.pos].kind.clone();
        if self.pos + 1 < self.tokens.len() {
            self.pos += 1;
        }
        kind
    }

    fn eat(&mut self, kind: &TokenKind) -> bool {
        if self.peek() == kind {
            self.advance();
            true
        } else {
            false
        }
    }

    fn eat_keyword(&mut self, keyword: Keyword) -> bool {
        self.eat(&TokenKind::Keyword(keyword))
    }

    fn is_keyword(&self, keyword: Keyword) -> bool {
        *self.peek() == TokenKind::Keyword(keyword)
    }

    /// TO, STEP, CONST, ... are plain identifiers; match them by name.
    fn is_word(&self, word: &str) -> bool {
        matches!(self.peek(), TokenKind::Ident { name, suffix: None } if name == word)
    }

    fn expected<T>(&self, expected: &'static str) -> PResult<T> {
        Err(ParseError {
            kind: ParseErrorKind::Expected {
                expected,
                found: self.peek().clone(),
            },
            span: self.span(),
            stmt: None,
        })
    }

    fn expect(&mut self, kind: &TokenKind, expected: &'static str) -> PResult<()> {
        if self.eat(kind) {
            Ok(())
        } else {
            self.expected(expected)
        }
    }

    fn expect_keyword(&mut self, keyword: Keyword, expected: &'static str) -> PResult<()> {
        self.expect(&TokenKind::Keyword(keyword), expected)
    }

    fn expect_word(&mut self, word: &str, expected: &'static str) -> PResult<()> {
        if self.is_word(word) {
            self.advance();
            Ok(())
        } else {
            self.expected(expected)
        }
    }

    fn at_stmt_end(&self) -> bool {
        // UNVERIFIED: ENDIF may close a one-line IF on the same line.
        matches!(
            self.peek(),
            TokenKind::Newline
                | TokenKind::Colon
                | TokenKind::Eof
                | TokenKind::Keyword(Keyword::Else | Keyword::Elseif | Keyword::Endif)
        )
    }

    // ---- statement lists ----

    fn program(mut self) -> PResult<Program> {
        // With no terminators, block() only stops at end of input; stray
        // closing keywords are reported by statement().
        let stmts = self.block(&[])?;
        Ok(Program { stmts })
    }

    /// Parses statements across lines until one of `until` (not consumed) or
    /// end of input. Reaching the end is left for the caller to report.
    fn block(&mut self, until: &[Keyword]) -> PResult<Vec<Stmt>> {
        let mut stmts = Vec::new();
        loop {
            while matches!(self.peek(), TokenKind::Newline | TokenKind::Colon) {
                self.advance();
            }
            match self.peek() {
                TokenKind::Eof => return Ok(stmts),
                TokenKind::Keyword(k) if until.contains(k) => return Ok(stmts),
                // END closes a DEF; at top level it is the END statement.
                TokenKind::Keyword(Keyword::End) if self.in_def => return Ok(stmts),
                _ => {}
            }
            // No separator is needed between statements: `A=1B=2` is two.
            stmts.push(self.statement(until.is_empty() && !self.in_def)?);
        }
    }

    /// Like [`Parser::block`] but the closing keyword must be present and is consumed.
    fn block_until(&mut self, end: Keyword, expected: &'static str) -> PResult<Vec<Stmt>> {
        let body = self.block(&[end])?;
        self.expect_keyword(end, expected)?;
        Ok(body)
    }

    /// A loop's body, during which its closing keyword inside another
    /// block means CONTINUE.
    fn loop_body(&mut self, end: Keyword, expected: &'static str) -> PResult<Vec<Stmt>> {
        self.loops.push(end);
        let body = self.block_until(end, expected);
        self.loops.pop();
        body
    }

    /// Statements of a one-line IF branch, up to the end of the line or ELSE.
    fn line_stmts(&mut self) -> PResult<Vec<Stmt>> {
        let mut stmts = Vec::new();
        loop {
            if self.eat(&TokenKind::Colon) {
                continue;
            }
            if self.at_stmt_end() {
                return Ok(stmts);
            }
            stmts.push(self.statement(false)?);
        }
    }

    // ---- statements ----

    fn statement(&mut self, top_level: bool) -> PResult<Stmt> {
        let start = self.span();
        let kind = self
            .nested(|p| p.statement_kind(top_level))
            .map_err(|e| ParseError {
                stmt: e.stmt.or(Some(start)),
                ..e
            })?;
        let Span { line, col } = start;
        Ok(Stmt { kind, line, col })
    }

    fn statement_kind(&mut self, top_level: bool) -> PResult<StmtKind> {
        use Keyword as K;
        let span = self.span();
        let keyword = match self.peek().clone() {
            TokenKind::Label(name) => {
                self.advance();
                return Ok(StmtKind::Label(name.to_ascii_uppercase()));
            }
            TokenKind::Ident { .. } => return self.ident_statement(),
            TokenKind::Keyword(k) => k,
            _ => return self.expected("statement"),
        };
        // VAR(...) = ... is an assignment, not a declaration.
        if keyword == K::Var && *self.peek_at(1) == TokenKind::LParen {
            return self.assignment();
        }
        self.advance();
        Ok(match keyword {
            K::Var | K::Dim => StmtKind::Var(self.var_decls()?),
            K::Print => StmtKind::Print {
                screen: None,
                items: self.print_items()?,
            },
            K::Tprint => {
                let screen = self.expr()?;
                let items = if self.eat(&TokenKind::Comma) {
                    self.print_items()?
                } else {
                    Vec::new()
                };
                StmtKind::Print {
                    screen: Some(screen),
                    items,
                }
            }
            K::Input => self.input(false)?,
            K::Linput => self.input(true)?,
            K::If => self.if_statement()?,
            K::Case => self.case_statement()?,
            K::For => self.for_statement()?,
            K::While => {
                let cond = self.expr()?;
                StmtKind::While {
                    cond,
                    body: self.loop_body(K::Wend, "WEND")?,
                }
            }
            K::Repeat => {
                let body = self.block_until(K::Until, "UNTIL")?;
                StmtKind::Repeat {
                    body,
                    cond: self.expr()?,
                }
            }
            K::Loop => StmtKind::Loop {
                body: self.loop_body(K::Endloop, "ENDLOOP")?,
            },
            K::Break => StmtKind::Break,
            K::Continue => StmtKind::Continue,
            // Inside an IF or another block of the loop, its closing
            // keyword goes on with the next turn (`IF A THEN NEXT`), as
            // SmileBASIC Debugger describes.
            K::Next | K::Wend | K::Endloop if self.loops.last() == Some(&keyword) => {
                if keyword == K::Next && matches!(self.peek(), TokenKind::Ident { .. }) {
                    self.lvalue()?;
                }
                StmtKind::Continue
            }
            K::Goto => StmtKind::Goto(self.jump_target()?),
            K::Gosub => StmtKind::Gosub(self.jump_target()?),
            K::On => self.on_statement()?,
            K::Return => StmtKind::Return(if self.at_stmt_end() {
                None
            } else {
                Some(self.expr()?)
            }),
            K::End => StmtKind::End,
            K::Def | K::Common => {
                if !top_level {
                    return Err(ParseError {
                        kind: ParseErrorKind::DefInsideBlock,
                        span,
                        stmt: None,
                    });
                }
                let common = keyword == K::Common;
                if common {
                    self.expect_keyword(K::Def, "DEF")?;
                }
                StmtKind::Def(Box::new(self.def(common)?))
            }
            K::Call => self.call_statement()?,
            K::Data => StmtKind::Data(self.comma_list(Self::expr)?),
            K::Read => StmtKind::Read(self.comma_list(Self::lvalue)?),
            K::Restore => StmtKind::Restore(self.target()?),
            K::Swap => {
                let a = self.lvalue()?;
                self.expect(&TokenKind::Comma, "','")?;
                StmtKind::Swap(a, self.lvalue()?)
            }
            // Reserved, but called like ordinary commands.
            K::Exec | K::Defout => {
                let name = Var {
                    name: format!("{keyword:?}").to_ascii_uppercase(),
                    suffix: None,
                };
                self.command(name)?
            }
            other => {
                return Err(ParseError {
                    kind: ParseErrorKind::Misplaced(other),
                    span,
                    stmt: None,
                });
            }
        })
    }

    fn ident_statement(&mut self) -> PResult<StmtKind> {
        let TokenKind::Ident { name, suffix } = self.peek().clone() else {
            unreachable!("caller checked for an identifier");
        };
        let next = self.peek_at(1).clone();
        if matches!(next, TokenKind::Assign | TokenKind::LBracket) {
            return self.assignment();
        }
        if suffix.is_none() {
            match (name.as_str(), &next) {
                ("CONST", TokenKind::Constant(_)) => {
                    self.advance();
                    return Ok(StmtKind::Const(self.comma_list(|p| {
                        let name = p.constant_name()?;
                        p.expect(&TokenKind::Assign, "'='")?;
                        Ok((name, p.expr()?))
                    })?));
                }
                ("ENUM", TokenKind::Constant(_)) => {
                    self.advance();
                    return Ok(StmtKind::Enum(self.comma_list(|p| {
                        let name = p.constant_name()?;
                        let value = if p.eat(&TokenKind::Assign) {
                            Some(p.expr()?)
                        } else {
                            None
                        };
                        Ok((name, value))
                    })?));
                }
                ("OPTION", TokenKind::Ident { name: feature, .. }) => {
                    let feature = feature.clone();
                    self.advance();
                    self.advance();
                    return Ok(StmtKind::Option(feature));
                }
                _ => {}
            }
        }
        self.advance();
        self.command(Var { name, suffix })
    }

    fn assignment(&mut self) -> PResult<StmtKind> {
        let target = self.lvalue()?;
        self.expect(&TokenKind::Assign, "'='")?;
        Ok(StmtKind::Assign {
            target,
            value: self.expr()?,
        })
    }

    fn command(&mut self, name: Var) -> PResult<StmtKind> {
        let args = self.args(Self::at_args_end)?;
        let outs = self.outs()?;
        Ok(StmtKind::Command { name, args, outs })
    }

    /// End of a command's arguments: the statement ends, OUT follows, or
    /// the next token starts another statement (`CLS PRINT 1`).
    fn at_args_end(&self) -> bool {
        self.at_stmt_end()
            || self.is_keyword(Keyword::Out)
            || !(*self.peek() == TokenKind::Comma || self.can_start_expr())
    }

    fn can_start_expr(&self) -> bool {
        match self.peek() {
            TokenKind::Int(_)
            | TokenKind::Real(_)
            | TokenKind::Str(_)
            | TokenKind::Label(_)
            | TokenKind::Constant(_)
            | TokenKind::Ident { .. }
            | TokenKind::LParen
            | TokenKind::Minus
            | TokenKind::Bang
            | TokenKind::Keyword(Keyword::Not) => true,
            TokenKind::Keyword(Keyword::Var | Keyword::Call | Keyword::Dim) => {
                *self.peek_at(1) == TokenKind::LParen
            }
            _ => false,
        }
    }

    /// OUT variables; like arguments, any of them may be left out (`OUT A,,B`).
    fn outs(&mut self) -> PResult<Vec<Option<LValue>>> {
        let mut outs = Vec::new();
        if !self.eat_keyword(Keyword::Out) {
            return Ok(outs);
        }
        loop {
            if *self.peek() == TokenKind::Comma || self.at_stmt_end() {
                outs.push(None);
            } else {
                outs.push(Some(self.lvalue()?));
            }
            if !self.eat(&TokenKind::Comma) {
                return Ok(outs);
            }
        }
    }

    /// Comma-separated arguments where any of them may be left out.
    fn args(&mut self, at_end: impl Fn(&Self) -> bool) -> PResult<Vec<Option<Expr>>> {
        let mut args = Vec::new();
        if at_end(self) {
            return Ok(args);
        }
        loop {
            if *self.peek() == TokenKind::Comma || at_end(self) {
                args.push(None);
            } else {
                args.push(Some(self.expr()?));
            }
            if !self.eat(&TokenKind::Comma) {
                return Ok(args);
            }
        }
    }

    fn comma_list<T>(&mut self, mut item: impl FnMut(&mut Self) -> PResult<T>) -> PResult<Vec<T>> {
        let mut items = vec![item(self)?];
        while self.eat(&TokenKind::Comma) {
            items.push(item(self)?);
        }
        Ok(items)
    }

    fn constant_name(&mut self) -> PResult<String> {
        match self.peek().clone() {
            TokenKind::Constant(name) => {
                self.advance();
                Ok(name)
            }
            _ => self.expected("constant name"),
        }
    }

    fn var_name(&mut self) -> PResult<Var> {
        match self.peek().clone() {
            TokenKind::Ident { name, suffix } => {
                self.advance();
                Ok(Var { name, suffix })
            }
            _ => self.expected("variable name"),
        }
    }

    fn var_decls(&mut self) -> PResult<Vec<VarDecl>> {
        let mut decls = Vec::new();
        loop {
            decls.push(self.var_decl()?);
            // SmileBASIC 4's own example programs end a VAR list with a comma.
            if !self.eat(&TokenKind::Comma) || self.at_stmt_end() {
                return Ok(decls);
            }
        }
    }

    fn var_decl(&mut self) -> PResult<VarDecl> {
        let var = self.var_name()?;
        let dims = if !self.eat(&TokenKind::LBracket) {
            None
        } else if *self.peek() == TokenKind::Comma {
            // `DIM A[,]` compiles; its sizes are no numbers when it runs,
            // which is Type mismatch.
            let mut n = 1;
            while self.eat(&TokenKind::Comma) {
                n += 1;
            }
            self.expect(&TokenKind::RBracket, "']'")?;
            Some(vec![Expr::Str(Vec::new()); n])
        } else {
            Some(self.bracket_list()?)
        };
        let init = if !self.eat(&TokenKind::Assign) {
            None
        } else if dims.is_some() && self.eat(&TokenKind::LBracket) {
            Some(Init::Array(self.bracket_list()?))
        } else {
            Some(Init::Expr(self.expr()?))
        };
        Ok(VarDecl { var, dims, init })
    }

    /// Expressions up to `]`, which may be empty. The `[` is already consumed.
    fn bracket_list(&mut self) -> PResult<Vec<Expr>> {
        let items = if *self.peek() == TokenKind::RBracket {
            Vec::new()
        } else {
            self.comma_list(Self::expr)?
        };
        self.expect(&TokenKind::RBracket, "']'")?;
        Ok(items)
    }

    /// The items end at anything but a separator after an expression, so
    /// that `?"A" B=1` prints A and then assigns B.
    fn print_items(&mut self) -> PResult<Vec<PrintItem>> {
        let mut items = Vec::new();
        let mut after_expr = false;
        while !self.at_stmt_end() {
            if self.eat(&TokenKind::Semicolon) {
                items.push(PrintItem::Semicolon);
                after_expr = false;
            } else if self.eat(&TokenKind::Comma) {
                items.push(PrintItem::Comma);
                after_expr = false;
            } else if !after_expr && self.can_start_expr() {
                items.push(PrintItem::Expr(self.expr()?));
                after_expr = true;
            } else {
                break;
            }
        }
        Ok(items)
    }

    fn input(&mut self, whole_line: bool) -> PResult<StmtKind> {
        // A `,` after the guide string hides the `?`. The guide may be any
        // string expression, a label among them; a variable is one only
        // when a `;` follows it, as `INPUT A,B` reads two values.
        let start = self.pos;
        let guide = match self.peek() {
            TokenKind::Str(_) | TokenKind::Label(_) | TokenKind::LParen => {
                Some((self.expr()?, true))
            }
            TokenKind::Ident { .. } => self.expr().ok().map(|e| (e, false)),
            _ => None,
        };
        let (prompt, semicolon) = match (guide, self.peek()) {
            (Some((prompt, _)), TokenKind::Semicolon) => {
                self.advance();
                (Some(prompt), true)
            }
            (Some((prompt, true)), TokenKind::Comma) => {
                self.advance();
                (Some(prompt), false)
            }
            _ => {
                self.pos = start;
                (None, true)
            }
        };
        let targets = self.comma_list(Self::lvalue)?;
        Ok(StmtKind::Input {
            prompt,
            // UNVERIFIED: LINPUT never shows `?`, and INPUT without a guide does.
            question: semicolon && !whole_line,
            targets,
            whole_line,
        })
    }

    fn if_statement(&mut self) -> PResult<StmtKind> {
        let mut branches = Vec::new();
        let cond = self.expr()?;
        // `IF X GOTO @L` jumps without THEN, and is always one line.
        if self.eat_keyword(Keyword::Goto) {
            let Span { line, col } = self.span();
            branches.push((
                cond,
                vec![Stmt {
                    kind: StmtKind::Goto(self.jump_target()?),
                    line,
                    col,
                }],
            ));
            let otherwise = self.one_line_else()?;
            return Ok(StmtKind::If {
                branches,
                otherwise,
            });
        }
        self.expect_keyword(Keyword::Then, "THEN")?;
        if *self.peek() != TokenKind::Newline {
            branches.push((cond, self.one_line_branch()?));
            // A one-line IF can chain ELSEIF too.
            while self.eat_keyword(Keyword::Elseif) {
                let cond = self.expr()?;
                let mut body = self.then_or_goto()?;
                body.extend(self.one_line_branch()?);
                branches.push((cond, body));
            }
            let otherwise = self.one_line_else()?;
            return Ok(StmtKind::If {
                branches,
                otherwise,
            });
        }

        branches.push((
            cond,
            self.block(&[Keyword::Elseif, Keyword::Else, Keyword::Endif])?,
        ));
        let mut otherwise = None;
        loop {
            match self.advance() {
                TokenKind::Keyword(Keyword::Elseif) => {
                    let cond = self.expr()?;
                    let mut body = self.then_or_goto()?;
                    body.extend(self.block(&[Keyword::Elseif, Keyword::Else, Keyword::Endif])?);
                    branches.push((cond, body));
                }
                TokenKind::Keyword(Keyword::Else) => {
                    otherwise = Some(self.block_until(Keyword::Endif, "ENDIF")?);
                    break;
                }
                TokenKind::Keyword(Keyword::Endif) => break,
                _ => return self.expected("ENDIF"),
            }
        }
        Ok(StmtKind::If {
            branches,
            otherwise,
        })
    }

    /// ELSEIF's THEN, or a GOTO in its place (`ELSEIF B GOTO @L`), which
    /// starts the branch.
    fn then_or_goto(&mut self) -> PResult<Vec<Stmt>> {
        if !self.eat_keyword(Keyword::Goto) {
            self.expect_keyword(Keyword::Then, "THEN")?;
            return Ok(Vec::new());
        }
        let Span { line, col } = self.span();
        let kind = StmtKind::Goto(self.jump_target()?);
        Ok(vec![Stmt { kind, line, col }])
    }

    /// Statements after a one-line THEN or ELSE; a bare label first means
    /// GOTO, and the rest of the line still belongs to the branch
    /// (`THEN @L:ENDIF`, as SmileBASIC Debugger writes it).
    fn one_line_branch(&mut self) -> PResult<Vec<Stmt>> {
        let mut stmts = Vec::new();
        if let TokenKind::Label(name) = self.peek().clone() {
            let Span { line, col } = self.span();
            self.advance();
            stmts.push(Stmt {
                kind: StmtKind::Goto(Target::Label(name.to_ascii_uppercase())),
                line,
                col,
            });
        }
        stmts.extend(self.line_stmts()?);
        Ok(stmts)
    }

    fn one_line_else(&mut self) -> PResult<Option<Vec<Stmt>>> {
        let otherwise = if !self.eat_keyword(Keyword::Else) {
            None
        } else if *self.peek() == TokenKind::Newline {
            // `ELSE` followed by a newline opens a block that ENDIF closes.
            return Ok(Some(self.block_until(Keyword::Endif, "ENDIF")?));
        } else {
            Some(self.one_line_branch()?)
        };
        self.eat_keyword(Keyword::Endif);
        Ok(otherwise)
    }

    fn case_statement(&mut self) -> PResult<StmtKind> {
        use Keyword as K;
        let subject = self.expr()?;
        let mut arms: Vec<WhenArm> = Vec::new();
        let leading = self.block(&[K::When, K::Otherwise, K::Endcase])?;
        if !leading.is_empty() {
            return self.expected("WHEN");
        }
        let mut otherwise = None;
        loop {
            match self.advance() {
                TokenKind::Keyword(K::When) => {
                    let value = self.expr()?;
                    let body = self.block(&[K::When, K::Otherwise, K::Endcase])?;
                    match arms.last_mut() {
                        Some(arm) if arm.body.is_empty() => {
                            arm.values.push(value);
                            arm.body = body;
                        }
                        _ => arms.push(WhenArm {
                            values: vec![value],
                            body,
                        }),
                    }
                }
                TokenKind::Keyword(K::Otherwise) => {
                    otherwise = Some(self.block_until(K::Endcase, "ENDCASE")?);
                    break;
                }
                TokenKind::Keyword(K::Endcase) => break,
                _ => return self.expected("ENDCASE"),
            }
        }
        Ok(StmtKind::Case {
            subject,
            arms,
            otherwise,
        })
    }

    fn for_statement(&mut self) -> PResult<StmtKind> {
        let var = self.lvalue()?;
        self.expect(&TokenKind::Assign, "'='")?;
        let from = self.expr()?;
        self.expect_word("TO", "TO")?;
        let to = self.expr()?;
        let step = if self.is_word("STEP") {
            self.advance();
            Some(self.expr()?)
        } else {
            None
        };
        let body = self.loop_body(Keyword::Next, "NEXT")?;
        // UNVERIFIED: NEXT may name the loop variable, as in SB3. It is not checked.
        // Anything else starts the next statement (`NEXT?I`).
        if matches!(self.peek(), TokenKind::Ident { .. }) {
            self.lvalue()?;
        }
        Ok(StmtKind::For {
            var,
            from,
            to,
            step,
            body,
        })
    }

    /// GOTO's and GOSUB's target, which may be missing until the jump
    /// runs (checked on hardware: a bare GOTO is a Type mismatch there).
    fn jump_target(&mut self) -> PResult<Target> {
        match self.at_stmt_end() {
            true => Ok(Target::Missing),
            false => self.target(),
        }
    }

    fn target(&mut self) -> PResult<Target> {
        if let TokenKind::Label(name) = self.peek().clone() {
            self.advance();
            Ok(Target::Label(name.to_ascii_uppercase()))
        } else {
            Ok(Target::Expr(self.expr()?))
        }
    }

    fn label(&mut self) -> PResult<String> {
        match self.peek().clone() {
            TokenKind::Label(name) => {
                self.advance();
                Ok(name.to_ascii_uppercase())
            }
            _ => self.expected("label"),
        }
    }

    fn on_statement(&mut self) -> PResult<StmtKind> {
        if self.eat_keyword(Keyword::Break) {
            self.expect_keyword(Keyword::Goto, "GOTO")?;
            // An empty string takes the handler away.
            if let TokenKind::Str(s) = self.peek()
                && s.is_empty()
            {
                self.advance();
                return Ok(StmtKind::OnBreakGoto(String::new()));
            }
            return Ok(StmtKind::OnBreakGoto(self.label()?));
        }
        let selector = self.expr()?;
        let gosub = match self.advance() {
            TokenKind::Keyword(Keyword::Goto) => false,
            TokenKind::Keyword(Keyword::Gosub) => true,
            _ => return self.expected("GOTO or GOSUB"),
        };
        // Labels only: the reference says ON GOTO does not take strings.
        let labels = self.comma_list(Self::label)?;
        Ok(StmtKind::On {
            selector,
            labels,
            gosub,
        })
    }

    fn call_statement(&mut self) -> PResult<StmtKind> {
        let next_ends = matches!(
            self.peek_at(1),
            TokenKind::Newline | TokenKind::Colon | TokenKind::Eof
        );
        if next_ends && self.is_word("SPRITE") {
            self.advance();
            return Ok(StmtKind::CallSprite);
        }
        if next_ends && self.is_word("TEXT") {
            self.advance();
            return Ok(StmtKind::CallText);
        }
        let name = self.expr()?;
        let args = if self.eat(&TokenKind::Comma) {
            self.args(Self::at_args_end)?
        } else {
            Vec::new()
        };
        let outs = self.outs()?;
        Ok(StmtKind::Call { name, args, outs })
    }

    fn params(&mut self, at_end: impl Fn(&Self) -> bool) -> PResult<Params> {
        if self.eat(&TokenKind::Star) {
            return Ok(Params::Variadic);
        }
        if at_end(self) {
            return Ok(Params::Fixed(Vec::new()));
        }
        Ok(Params::Fixed(self.comma_list(Self::param)?))
    }

    /// A DEF's parameter: a name, which may end in `[]` to say that it
    /// takes an array.
    fn param(&mut self) -> PResult<Var> {
        let name = self.var_name()?;
        if self.eat(&TokenKind::LBracket) {
            self.expect(&TokenKind::RBracket, "']'")?;
        }
        Ok(name)
    }

    fn def(&mut self, common: bool) -> PResult<Def> {
        let name = self.var_name()?;
        let is_function = self.eat(&TokenKind::LParen);
        let (params, outs) = if is_function {
            let params = self.params(|p| *p.peek() == TokenKind::RParen)?;
            self.expect(&TokenKind::RParen, "')'")?;
            (params, Params::Fixed(Vec::new()))
        } else {
            let params = self.params(|p| p.at_stmt_end() || p.is_keyword(Keyword::Out))?;
            let outs = if self.eat_keyword(Keyword::Out) {
                self.params(Self::at_stmt_end)?
            } else {
                Params::Fixed(Vec::new())
            };
            (params, outs)
        };
        self.in_def = true;
        let body = self.block(&[]);
        self.in_def = false;
        let body = body?;
        self.expect_keyword(Keyword::End, "END")?;
        Ok(Def {
            name,
            common,
            is_function,
            params,
            outs,
            body,
        })
    }

    fn lvalue(&mut self) -> PResult<LValue> {
        let mut target = if self.is_keyword(Keyword::Var) {
            self.advance();
            self.expect(&TokenKind::LParen, "'('")?;
            let name = self.expr()?;
            self.expect(&TokenKind::RParen, "')'")?;
            LValue::Dynamic(Box::new(name))
        } else {
            LValue::Var(self.var_name()?)
        };
        while self.eat(&TokenKind::LBracket) {
            let indices = self.comma_list(Self::expr)?;
            self.expect(&TokenKind::RBracket, "']'")?;
            target = LValue::Index {
                base: Box::new(target),
                indices,
            };
        }
        Ok(target)
    }

    // ---- expressions ----

    pub fn expr(&mut self) -> PResult<Expr> {
        self.nested(|p| p.binary(0))
    }

    fn nested<T>(&mut self, f: impl FnOnce(&mut Self) -> PResult<T>) -> PResult<T> {
        if self.depth >= MAX_DEPTH {
            return Err(ParseError {
                kind: ParseErrorKind::TooDeep,
                span: self.span(),
                stmt: None,
            });
        }
        self.depth += 1;
        // Debug builds use ~20 KiB of stack per nested block, so grow the
        // stack on demand instead of relying on the caller's thread size.
        let result = stacker::maybe_grow(64 * 1024, 1024 * 1024, || f(self));
        self.depth -= 1;
        result
    }

    /// Precedence climbing: one stack frame per operand rather than one per
    /// precedence level, which keeps deep nesting affordable.
    fn binary(&mut self, min_level: u8) -> PResult<Expr> {
        let mut lhs = self.unary()?;
        while let Some((op, level)) = binary_op(self.peek()).filter(|&(_, l)| l >= min_level) {
            self.advance();
            let rhs = self.binary(level + 1)?;
            lhs = Expr::Binary {
                op,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            };
        }
        Ok(lhs)
    }

    fn unary(&mut self) -> PResult<Expr> {
        let op = match self.peek() {
            TokenKind::Minus => UnaryOp::Neg,
            TokenKind::Keyword(Keyword::Not) => UnaryOp::Not,
            TokenKind::Bang => UnaryOp::LogicalNot,
            _ => return self.postfix(),
        };
        self.advance();
        let expr = self.nested(Self::unary)?;
        Ok(Expr::Unary {
            op,
            expr: Box::new(expr),
        })
    }

    fn postfix(&mut self) -> PResult<Expr> {
        let mut expr = self.primary()?;
        while self.eat(&TokenKind::LBracket) {
            let indices = self.comma_list(Self::expr)?;
            self.expect(&TokenKind::RBracket, "']'")?;
            expr = Expr::Index {
                base: Box::new(expr),
                indices,
            };
        }
        Ok(expr)
    }

    fn call_args(&mut self) -> PResult<Vec<Option<Expr>>> {
        let args = self.args(|p| *p.peek() == TokenKind::RParen)?;
        self.expect(&TokenKind::RParen, "')'")?;
        Ok(args)
    }

    fn primary(&mut self) -> PResult<Expr> {
        let expr = match self.peek().clone() {
            TokenKind::Int(n) => Expr::Int(n),
            TokenKind::Real(x) => Expr::Real(x),
            TokenKind::Str(s) => Expr::Str(s),
            TokenKind::Label(name) => Expr::Label(name),
            TokenKind::Constant(name) => Expr::Constant(name),
            TokenKind::Ident { name, suffix } => {
                self.advance();
                let var = Var { name, suffix };
                if self.eat(&TokenKind::LParen) {
                    return Ok(Expr::Call {
                        name: var,
                        args: self.call_args()?,
                    });
                }
                return Ok(Expr::Var(var));
            }
            TokenKind::LParen => {
                self.advance();
                let inner = self.expr()?;
                self.expect(&TokenKind::RParen, "')'")?;
                return Ok(inner);
            }
            TokenKind::Keyword(Keyword::Var) if *self.peek_at(1) == TokenKind::LParen => {
                self.advance();
                self.advance();
                let name = self.expr()?;
                self.expect(&TokenKind::RParen, "')'")?;
                return Ok(Expr::DynamicVar(Box::new(name)));
            }
            TokenKind::Keyword(Keyword::Call) if *self.peek_at(1) == TokenKind::LParen => {
                self.advance();
                self.advance();
                let name = self.expr()?;
                let args = if self.eat(&TokenKind::Comma) {
                    self.call_args()?
                } else {
                    self.expect(&TokenKind::RParen, "')'")?;
                    Vec::new()
                };
                return Ok(Expr::DynamicCall {
                    name: Box::new(name),
                    args,
                });
            }
            // DIM(array) is a function despite DIM being reserved.
            TokenKind::Keyword(Keyword::Dim) if *self.peek_at(1) == TokenKind::LParen => {
                self.advance();
                self.advance();
                let name = Var {
                    name: "DIM".into(),
                    suffix: None,
                };
                return Ok(Expr::Call {
                    name,
                    args: self.call_args()?,
                });
            }
            _ => return self.expected("expression"),
        };
        self.advance();
        Ok(expr)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use BinaryOp as B;

    fn stmts(source: &str) -> Vec<StmtKind> {
        parse(source)
            .unwrap()
            .stmts
            .into_iter()
            .map(|s| s.kind)
            .collect()
    }

    fn one(source: &str) -> StmtKind {
        let mut all = stmts(source);
        assert_eq!(all.len(), 1, "{all:?}");
        all.pop().unwrap()
    }

    fn expr(source: &str) -> Expr {
        match one(&format!("X={source}")) {
            StmtKind::Assign { value, .. } => value,
            other => panic!("{other:?}"),
        }
    }

    fn var(name: &str) -> Var {
        Var {
            name: name.into(),
            suffix: None,
        }
    }

    fn v(name: &str) -> Expr {
        Expr::Var(var(name))
    }

    fn bin(op: BinaryOp, lhs: Expr, rhs: Expr) -> Expr {
        Expr::Binary {
            op,
            lhs: Box::new(lhs),
            rhs: Box::new(rhs),
        }
    }

    fn cmd(name: &str, args: Vec<Option<Expr>>) -> StmtKind {
        StmtKind::Command {
            name: var(name),
            args,
            outs: vec![],
        }
    }

    fn err(source: &str) -> ParseErrorKind {
        parse(source).unwrap_err().kind
    }

    #[test]
    fn precedence_follows_the_reference_table() {
        // A || B && C OR D AND E == F << G + H * -I
        let e = expr("A||B&&C OR D AND E==F<<G+H*-I");
        let neg_i = Expr::Unary {
            op: UnaryOp::Neg,
            expr: Box::new(v("I")),
        };
        let mul = bin(B::Mul, v("H"), neg_i);
        let add = bin(B::Add, v("G"), mul);
        let shl = bin(B::Shl, v("F"), add);
        let eq = bin(B::Eq, v("E"), shl);
        let and = bin(B::And, v("D"), eq);
        let or = bin(B::Or, v("C"), and);
        let land = bin(B::LogicalAnd, v("B"), or);
        assert_eq!(e, bin(B::LogicalOr, v("A"), land));
    }

    #[test]
    fn binary_operators_are_left_associative() {
        assert_eq!(
            expr("A-B-C"),
            bin(B::Sub, bin(B::Sub, v("A"), v("B")), v("C"))
        );
        assert_eq!(
            expr("A DIV B MOD C"),
            bin(B::Mod, bin(B::IntDiv, v("A"), v("B")), v("C"))
        );
    }

    #[test]
    fn not_binds_tighter_than_comparison() {
        let not_a = Expr::Unary {
            op: UnaryOp::Not,
            expr: Box::new(v("A")),
        };
        assert_eq!(expr("NOT A==B"), bin(B::Eq, not_a, v("B")));
    }

    #[test]
    fn calls_indexing_and_dynamic_forms() {
        assert_eq!(
            expr("MID$(A$,,2)[0]"),
            Expr::Index {
                base: Box::new(Expr::Call {
                    name: Var {
                        name: "MID".into(),
                        suffix: Some(lexer::Suffix::Str)
                    },
                    args: vec![
                        Some(Expr::Var(Var {
                            name: "A".into(),
                            suffix: Some(lexer::Suffix::Str)
                        })),
                        None,
                        Some(Expr::Int(2)),
                    ],
                }),
                indices: vec![Expr::Int(0)],
            }
        );
        assert_eq!(
            expr("VAR(\"A\")"),
            Expr::DynamicVar(Box::new(Expr::Str("A".encode_utf16().collect())))
        );
        assert!(matches!(expr("CALL(F$,1)"), Expr::DynamicCall { args, .. } if args.len() == 1));
        assert!(matches!(expr("DIM(A)"), Expr::Call { name, .. } if name.name == "DIM"));
        assert_eq!(expr("@L"), Expr::Label("L".into()));
    }

    #[test]
    fn assignments() {
        assert!(matches!(
            one("A[1,2]=3"),
            StmtKind::Assign {
                target: LValue::Index { .. },
                ..
            }
        ));
        assert!(matches!(
            one("VAR(\"X\")=1"),
            StmtKind::Assign {
                target: LValue::Dynamic(_),
                ..
            }
        ));
        // Not reserved, so usable as a variable.
        assert!(matches!(one("CONST=1"), StmtKind::Assign { .. }));
    }

    #[test]
    fn commands_with_omitted_args_and_out() {
        assert_eq!(one("CLS"), cmd("CLS", vec![]));
        assert_eq!(
            one("LOCATE ,5"),
            cmd("LOCATE", vec![None, Some(Expr::Int(5))])
        );
        assert_eq!(
            one("CALCPM 5,10 OUT P,M"),
            StmtKind::Command {
                name: var("CALCPM"),
                args: vec![Some(Expr::Int(5)), Some(Expr::Int(10))],
                outs: vec![Some(LValue::Var(var("P"))), Some(LValue::Var(var("M")))],
            }
        );
        assert_eq!(
            one("DEFOUT 0,10"),
            cmd("DEFOUT", vec![Some(Expr::Int(0)), Some(Expr::Int(10))])
        );
    }

    #[test]
    fn declarations() {
        let StmtKind::Var(decls) = one("VAR A, B=1, C[10,2], D[]=[1,2]") else {
            panic!()
        };
        assert_eq!(decls.len(), 4);
        assert_eq!(decls[2].dims, Some(vec![Expr::Int(10), Expr::Int(2)]));
        assert_eq!(
            decls[3].init,
            Some(Init::Array(vec![Expr::Int(1), Expr::Int(2)]))
        );
        assert_eq!(
            one("CONST #C1=1,#C2=\"A\""),
            StmtKind::Const(vec![
                ("C1".into(), Expr::Int(1)),
                ("C2".into(), Expr::Str("A".encode_utf16().collect())),
            ])
        );
        assert_eq!(
            one("ENUM #A=2,#B"),
            StmtKind::Enum(vec![("A".into(), Some(Expr::Int(2))), ("B".into(), None)])
        );
        assert_eq!(one("OPTION STRICT"), StmtKind::Option("STRICT".into()));
    }

    #[test]
    fn print_and_input() {
        assert_eq!(
            one("?\"A\";X,"),
            StmtKind::Print {
                screen: None,
                items: vec![
                    PrintItem::Expr(Expr::Str("A".encode_utf16().collect())),
                    PrintItem::Semicolon,
                    PrintItem::Expr(v("X")),
                    PrintItem::Comma,
                ],
            }
        );
        assert!(
            matches!(one("TPRINT 1,X"), StmtKind::Print { screen: Some(_), items } if items.len() == 1)
        );
        assert!(matches!(
            one("INPUT \"NAME\";N$,A"),
            StmtKind::Input { prompt: Some(_), question: true, targets, whole_line: false }
                if targets.len() == 2
        ));
        assert!(matches!(
            one("INPUT \"NAME\",N$"),
            StmtKind::Input {
                question: false,
                ..
            }
        ));
        assert!(matches!(
            one("LINPUT L$"),
            StmtKind::Input {
                prompt: None,
                whole_line: true,
                ..
            }
        ));
    }

    #[test]
    fn one_line_if() {
        let StmtKind::If {
            branches,
            otherwise,
        } = one("IF A>1 THEN @J ELSE PRINT:B=1")
        else {
            panic!()
        };
        assert_eq!(
            branches[0].1[0].kind,
            StmtKind::Goto(Target::Label("J".into()))
        );
        assert_eq!(otherwise.unwrap().len(), 2);

        let StmtKind::If {
            branches,
            otherwise,
        } = one("IF X GOTO \"@A\" ELSE @B")
        else {
            panic!()
        };
        assert!(matches!(
            branches[0].1[0].kind,
            StmtKind::Goto(Target::Expr(_))
        ));
        assert_eq!(
            otherwise.unwrap()[0].kind,
            StmtKind::Goto(Target::Label("B".into()))
        );

        // The one-line IF ends at the newline.
        assert_eq!(stmts("IF A THEN B=1\nC=2").len(), 2);
    }

    #[test]
    fn one_line_elseif() {
        let src = "IF OX<0 THEN OX=0 ELSEIF OX>9 THEN OX=9 ELSE OX=5";
        let StmtKind::If {
            branches,
            otherwise,
        } = one(src)
        else {
            panic!()
        };
        assert_eq!(branches.len(), 2);
        assert!(otherwise.is_some());
    }

    #[test]
    fn nested_one_line_if_binds_else_to_inner() {
        let StmtKind::If {
            branches,
            otherwise: None,
        } = one("IF A THEN IF B THEN X ELSE Y")
        else {
            panic!()
        };
        assert!(matches!(
            &branches[0].1[0].kind,
            StmtKind::If {
                otherwise: Some(_),
                ..
            }
        ));
    }

    #[test]
    fn block_if_with_elseif() {
        let src = "IF A==1 THEN\n PRINT 1\nELSEIF A==2 THEN\n PRINT 2\nELSE\n PRINT 3\nENDIF";
        let StmtKind::If {
            branches,
            otherwise,
        } = one(src)
        else {
            panic!()
        };
        assert_eq!(branches.len(), 2);
        assert_eq!(otherwise.unwrap().len(), 1);
        assert!(matches!(
            err("IF A THEN\nPRINT"),
            ParseErrorKind::Expected {
                expected: "ENDIF",
                ..
            }
        ));
    }

    #[test]
    fn one_line_then_with_block_else() {
        let StmtKind::If { otherwise, .. } = one("IF A THEN B=1 ELSE\n C=1\n D=1\nENDIF") else {
            panic!()
        };
        assert_eq!(otherwise.unwrap().len(), 2);
    }

    #[test]
    fn case_groups_consecutive_when() {
        let src =
            "CASE A\nWHEN 0:WHEN 1: PRINT\"A\"\nWHEN 2: PRINT\"B\"\nOTHERWISE: PRINT\"X\"\nENDCASE";
        let StmtKind::Case {
            arms, otherwise, ..
        } = one(src)
        else {
            panic!()
        };
        assert_eq!(arms.len(), 2);
        assert_eq!(arms[0].values, vec![Expr::Int(0), Expr::Int(1)]);
        assert!(otherwise.is_some());
    }

    #[test]
    fn loops() {
        let StmtKind::For { step, body, .. } = one("FOR I=0 TO 9 STEP 2\n PRINT I\nNEXT") else {
            panic!()
        };
        assert_eq!(step, Some(Expr::Int(2)));
        assert_eq!(body.len(), 1);
        assert!(matches!(one("FOR I=0TO 9:NEXT I"), StmtKind::For { .. }));
        assert!(
            matches!(one("WHILE A<B:A=A+1:WEND"), StmtKind::While { body, .. } if body.len() == 1)
        );
        assert!(matches!(
            one("REPEAT\nA=A+1\nUNTIL A>B"),
            StmtKind::Repeat { .. }
        ));
        assert!(
            matches!(one("LOOP\nBREAK\nENDLOOP"), StmtKind::Loop { body } if body[0].kind == StmtKind::Break)
        );
        assert_eq!(err("WEND"), ParseErrorKind::Misplaced(Keyword::Wend));
    }

    #[test]
    fn jumps() {
        assert_eq!(
            one("ON IDX GOSUB @A,@B"),
            StmtKind::On {
                selector: v("IDX"),
                labels: vec!["A".into(), "B".into()],
                gosub: true
            }
        );
        assert_eq!(one("ON BREAK GOTO @Q"), StmtKind::OnBreakGoto("Q".into()));
        assert!(matches!(
            one("GOTO \"@\"+L$"),
            StmtKind::Goto(Target::Expr(_))
        ));
        assert_eq!(one("RETURN"), StmtKind::Return(None));
    }

    #[test]
    fn calls() {
        assert_eq!(one("CALL SPRITE"), StmtKind::CallSprite);
        assert!(matches!(
            one("CALL \"USERCD\",X,Y OUT A,B"),
            StmtKind::Call { args, outs, .. } if args.len() == 2 && outs.len() == 2
        ));
        // SPRITE as a variable holding the name.
        assert!(matches!(one("CALL SPRITE,1"), StmtKind::Call { .. }));
    }

    #[test]
    fn defs() {
        let src = "DEF ADD(X,Y)\nRETURN X+Y\nEND\nCOMMON DEF CALCPM A,B OUT OP,OM\nOP=A+B\nEND\nDEF V * OUT *\nEND";
        let all = stmts(src);
        let StmtKind::Def(add) = &all[0] else {
            panic!()
        };
        assert!(add.is_function && !add.common);
        assert_eq!(add.params, Params::Fixed(vec![var("X"), var("Y")]));
        let StmtKind::Def(calc) = &all[1] else {
            panic!()
        };
        assert!(!calc.is_function && calc.common);
        assert_eq!(calc.outs, Params::Fixed(vec![var("OP"), var("OM")]));
        let StmtKind::Def(variadic) = &all[2] else {
            panic!()
        };
        assert_eq!(
            (&variadic.params, &variadic.outs),
            (&Params::Variadic, &Params::Variadic)
        );

        assert_eq!(
            err("IF A THEN\nDEF F\nEND\nENDIF"),
            ParseErrorKind::DefInsideBlock
        );
        // END outside DEF ends the program.
        assert_eq!(one("END"), StmtKind::End);
    }

    #[test]
    fn data_read_restore_swap() {
        assert!(matches!(one("DATA 1,\"A\",-2"), StmtKind::Data(v) if v.len() == 3));
        assert!(matches!(one("READ A,B$[0]"), StmtKind::Read(v) if v.len() == 2));
        assert_eq!(
            one("RESTORE @D"),
            StmtKind::Restore(Target::Label("D".into()))
        );
        assert!(matches!(one("SWAP A,B[1]"), StmtKind::Swap(..)));
    }

    #[test]
    fn var_list_may_end_with_comma() {
        let StmtKind::Var(decls) = one("VAR I,J,TONE,") else {
            panic!()
        };
        assert_eq!(decls.len(), 3);
    }

    #[test]
    fn separators_are_optional() {
        assert_eq!(stmts("A=1B=2").len(), 2);
        assert_eq!(stmts("CLS PRINT 1 PRINT 2").len(), 3);
        assert_eq!(stmts("DATA 1,2 DATA 3").len(), 2);
        assert_eq!(stmts("@L:A=1:B=2\n\nC=3").len(), 4);
        let StmtKind::Command { outs, .. } = one("RGB C OUT A,,,") else {
            panic!()
        };
        assert_eq!(outs.len(), 4);
        assert!(outs[1].is_none());
    }

    #[test]
    fn deep_nesting_is_an_error_not_a_crash() {
        // 1 MiB is the default main-thread stack on Windows; the parser must
        // reach MAX_DEPTH within it even in debug builds.
        let run = std::thread::Builder::new().stack_size(1 << 20).spawn(|| {
            let parens = format!("X={}1{}", "(".repeat(100_000), ")".repeat(100_000));
            assert_eq!(err(&parens), ParseErrorKind::TooDeep);
            assert_eq!(
                err(&format!("X={}1", "-".repeat(100_000))),
                ParseErrorKind::TooDeep
            );
            assert_eq!(err(&"WHILE 1\n".repeat(100_000)), ParseErrorKind::TooDeep);
            assert_eq!(err(&"IF 1 THEN ".repeat(100_000)), ParseErrorKind::TooDeep);
            let depth = MAX_DEPTH as usize - 2;
            assert!(parse(&format!("X={}1{}", "(".repeat(depth), ")".repeat(depth))).is_ok());
        });
        run.unwrap().join().unwrap();
    }

    #[test]
    fn never_panics_on_truncated_or_garbled_input() {
        let src = "DEF F(A)\nIF A THEN\n?\"X\";A,:RETURN A*2\nELSEIF A<0 THEN @L ELSE\nCASE A\nWHEN 1:GOTO \"@L\"\nENDCASE\nENDIF\nEND\n@L:VAR B[]=[1,2]:FOR I=0 TO 9 STEP 2:NEXT:ON A GOSUB @L,@L\nCALL \"F\",1 OUT X:INPUT \"Q\";Q$";
        let chars: Vec<char> = src.chars().collect();
        for end in 0..=chars.len() {
            let prefix: String = chars[..end].iter().collect();
            let _ = parse(&prefix);
            // Drop one character as well.
            for skip in 0..end {
                let mut s: String = chars[..skip].iter().collect();
                s.extend(&chars[skip + 1..end]);
                let _ = parse(&s);
            }
        }
    }

    #[test]
    fn errors_carry_line_numbers() {
        let e = parse("A=1\nB=(").unwrap_err();
        assert_eq!(e.span.line, 2);
        assert_eq!(
            e.to_string(),
            "Syntax error in 2 (expected expression, found Eof)"
        );
    }
}
