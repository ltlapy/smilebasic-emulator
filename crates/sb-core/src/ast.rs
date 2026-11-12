//! Syntax tree produced by the parser.

use crate::lexer::{Span, Suffix};

#[derive(Clone, Debug, PartialEq)]
pub struct Program {
    pub stmts: Vec<Stmt>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Stmt {
    pub kind: StmtKind,
    /// Where the statement starts, for error messages and for the editor
    /// to show: the 1-based line, and column in characters.
    pub line: u32,
    pub col: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Var {
    pub name: String,
    pub suffix: Option<Suffix>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum LValue {
    Var(Var),
    Index {
        base: Box<LValue>,
        indices: Vec<Expr>,
    },
    /// `VAR("name")`, resolved at run time.
    Dynamic(Box<Expr>),
}

/// Where GOTO, GOSUB and RESTORE go: a label, or a string computed at run time.
#[derive(Clone, Debug, PartialEq)]
pub enum Target {
    Label(String),
    Expr(Expr),
    /// GOTO or GOSUB with nothing after it, a Type mismatch when it runs.
    Missing,
}

#[derive(Clone, Debug, PartialEq)]
pub struct VarDecl {
    pub var: Var,
    /// `VAR A[10,2]` gives `Some([10, 2])`, and `VAR A[]` gives `Some([])`.
    pub dims: Option<Vec<Expr>>,
    pub init: Option<Init>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Init {
    Expr(Expr),
    /// `VAR A[]=[1,2,3]`
    Array(Vec<Expr>),
}

#[derive(Clone, Debug, PartialEq)]
pub enum PrintItem {
    Expr(Expr),
    /// `;`: no separator and, at the end, no newline.
    Semicolon,
    /// `,`: a tab-like gap.
    Comma,
}

#[derive(Clone, Debug, PartialEq)]
pub struct WhenArm {
    /// Consecutive `WHEN`s with nothing between them share one body.
    pub values: Vec<Expr>,
    pub body: Vec<Stmt>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Params {
    Fixed(Vec<Var>),
    /// `*`: count and values are read with DEFARGC/DEFARG or DEFOUTC/DEFOUT.
    Variadic,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Def {
    pub name: Var,
    pub common: bool,
    /// `DEF F(X)` returns one value with RETURN; `DEF F X OUT Y` is a command.
    pub is_function: bool,
    pub params: Params,
    pub outs: Params,
    pub body: Vec<Stmt>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum StmtKind {
    Label(String),
    Assign {
        target: LValue,
        value: Expr,
        /// The column of the `=`, where an error in the value is placed.
        eq_col: u32,
    },
    Var(Vec<VarDecl>),
    Const(Vec<(String, Expr)>),
    Enum(Vec<(String, Option<Expr>)>),
    Option(String),
    /// A built-in or user-defined command. Omitted arguments (`CMD ,1`) and
    /// OUT variables (`OUT A,,B`) are `None`.
    Command {
        name: Var,
        args: Vec<Option<Expr>>,
        outs: Vec<Option<LValue>>,
    },
    /// `CALL name$, args OUT vars`
    Call {
        name: Expr,
        args: Vec<Option<Expr>>,
        outs: Vec<Option<LValue>>,
    },
    CallSprite,
    CallText,
    Print {
        screen: Option<Expr>,
        items: Vec<PrintItem>,
    },
    /// INPUT, or LINPUT when `whole_line` is set.
    Input {
        prompt: Option<Expr>,
        /// Whether a `?` follows the prompt.
        question: bool,
        targets: Vec<LValue>,
        whole_line: bool,
    },
    If {
        branches: Vec<(Expr, Vec<Stmt>)>,
        otherwise: Option<Vec<Stmt>>,
    },
    Case {
        subject: Expr,
        arms: Vec<WhenArm>,
        otherwise: Option<Vec<Stmt>>,
    },
    For {
        var: LValue,
        from: Expr,
        to: Expr,
        step: Option<Expr>,
        body: Vec<Stmt>,
    },
    While {
        cond: Expr,
        body: Vec<Stmt>,
    },
    Repeat {
        body: Vec<Stmt>,
        cond: Expr,
    },
    Loop {
        body: Vec<Stmt>,
    },
    Break,
    Continue,
    Goto(Target),
    Gosub(Target),
    On {
        selector: Expr,
        labels: Vec<String>,
        gosub: bool,
    },
    OnBreakGoto(String),
    Return(Option<Expr>),
    End,
    Def(Box<Def>),
    Data(Vec<Expr>),
    Read(Vec<LValue>),
    Restore(Target),
    Swap(LValue, LValue),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnaryOp {
    Neg,
    /// `NOT`, bitwise.
    Not,
    /// `!`, logical.
    LogicalNot,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BinaryOp {
    Mul,
    Div,
    IntDiv,
    Mod,
    Add,
    Sub,
    Shl,
    Shr,
    UShr,
    RotL,
    RotR,
    Eq,
    NotEq,
    Less,
    LessEq,
    Greater,
    GreaterEq,
    And,
    Or,
    Xor,
    LogicalAnd,
    LogicalOr,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Expr {
    Int(i32),
    Real(f64),
    Str(Vec<u16>),
    /// A label in an expression is the string `"@NAME"`.
    Label(String),
    /// A `#` constant and where it is, for an Undefined constant error.
    Constant(String, Span),
    Var(Var),
    Index {
        base: Box<Expr>,
        indices: Vec<Expr>,
    },
    /// Omitted arguments (`F(,1)`) are `None`.
    Call {
        name: Var,
        args: Vec<Option<Expr>>,
    },
    /// `CALL(name$, args)`
    DynamicCall {
        name: Box<Expr>,
        args: Vec<Option<Expr>>,
    },
    /// `VAR("name")`
    DynamicVar(Box<Expr>),
    Unary {
        op: UnaryOp,
        expr: Box<Expr>,
    },
    Binary {
        op: BinaryOp,
        lhs: Box<Expr>,
        rhs: Box<Expr>,
    },
}

impl Expr {
    /// The operand that a chain of operators continues down: the left
    /// operand of a binary operator, the operand of a unary one, the base
    /// of an index. Binary operators are left-associative, so a line such
    /// as `"A"+"B"+...` nests thousands deep here. Walks over expressions
    /// follow this operand in a loop rather than recursing, which keeps
    /// the stack they need bounded by the parser's nesting limit.
    pub fn chain_operand(&self) -> Option<&Expr> {
        match self {
            Expr::Binary { lhs, .. } => Some(lhs),
            Expr::Unary { expr, .. } => Some(expr),
            Expr::Index { base, .. } => Some(base),
            _ => None,
        }
    }

    /// Takes out the chain operand when it is a link of the chain itself,
    /// leaving a placeholder.
    fn take_chain(&mut self) -> Option<Expr> {
        let operand = match self {
            Expr::Binary { lhs, .. } => lhs,
            Expr::Unary { expr, .. } => expr,
            Expr::Index { base, .. } => base,
            _ => return None,
        };
        operand.chain_operand()?;
        Some(std::mem::replace(&mut **operand, Expr::Int(0)))
    }
}

/// Drops a chain one link at a time, where the derived drop would
/// recurse down all of it.
impl Drop for Expr {
    fn drop(&mut self) {
        let mut next = self.take_chain();
        while let Some(mut link) = next {
            next = link.take_chain();
        }
    }
}
