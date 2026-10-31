//! Compiled program: a flat instruction list for a stack machine.

use std::collections::HashMap;

use crate::ast::{BinaryOp, UnaryOp};
use crate::lexer::Suffix;
use crate::value::{ElemType, Str};

pub type Addr = u32;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Slot {
    Global(u32),
    /// Index into the current DEF call's locals.
    Local(u32),
}

/// Which callbacks CALL runs: SPFUNC's or TFUNC's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Callbacks {
    Sprite,
    Text,
}

/// What a call leaves on the stack.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Want {
    /// Called as a function: exactly one value.
    Value,
    /// Called as a command: one value per OUT variable.
    Outs(u8),
}

impl Want {
    pub fn count(self) -> usize {
        match self {
            Want::Value => 1,
            Want::Outs(n) => n.into(),
        }
    }
}

/// What an INPUT target accepts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InputKind {
    Number,
    Str,
}

/// Stack effects are written as `[inputs] -> [outputs]`, top of stack last.
#[derive(Clone, Debug, PartialEq)]
pub enum Op {
    Int(i32),
    Real(f64),
    /// Pushes a fresh copy of string constant `n`, so that modifying it in
    /// place cannot change the constant.
    Str(u32),
    Default,
    Load(Slot),
    /// `[value] -> []`
    Store(Slot),
    /// Direct mode's DIM and VAR of a global: Duplicate variable if it
    /// exists already, from this line or an earlier one; otherwise it
    /// exists from now on, even if what follows fails. An array starts as
    /// the omitted value (TYPEOF 0) until its DIM finishes, a variable as
    /// zero (checked on hardware).
    Declare {
        slot: u32,
        array: bool,
    },
    /// `[base, i1..in] -> [element]`
    Index(u8),
    /// `[value, base, i1..in] -> []`
    StoreIndex(u8),
    /// `[d1..dn] -> [array]`
    NewArray {
        elem: ElemType,
        dims: u8,
    },
    /// `[v1..vn] -> [array]`
    ArrayLit {
        elem: ElemType,
        len: u32,
    },
    Unary(UnaryOp),
    Binary(BinaryOp),
    /// `[array, v1..vn] -> [array]`: sets the first elements.
    FillArray(u32),
    /// `[v] -> [0 or 1]`
    Truthy,
    Pop,
    /// `[a, b] -> [b, a]`. SmileBASIC evaluates right to left, so the
    /// compiler pushes values in reverse and puts them back in order.
    Swap,
    /// `[v1..vn] -> [vn..v1]`
    Reverse(u32),
    Jump(Addr),
    /// `[cond] -> []`
    JumpIfFalse(Addr),
    JumpIfTrue(Addr),
    Gosub(Addr),
    /// ON BREAK GOTO: the + button jumps here instead of stopping.
    OnBreak(Addr),
    /// `[label string] -> []`
    GotoDyn,
    GosubDyn,
    /// `[selector] -> []`. Followed by `count` `Jump` entries forming the table.
    On {
        count: u32,
        gosub: bool,
    },
    /// Returns from GOSUB, or from a DEF when no GOSUB is pending in it.
    Return,
    /// `[value] -> []`, returns from a function DEF.
    ReturnValue,
    /// `[args..] -> [results..]`
    CallDef {
        def: u32,
        argc: u8,
        want: Want,
    },
    CallBuiltin {
        id: u16,
        argc: u8,
        want: Want,
    },
    CallHost {
        id: u32,
        argc: u8,
        want: Want,
    },
    /// `[args..] -> [results..]`: a COMMON DEF of another slot, by
    /// string constant `name`.
    CallCommon {
        name: u32,
        argc: u8,
        want: Want,
    },
    /// `[name, args..] -> [results..]`
    CallDyn {
        argc: u8,
        want: Want,
    },
    /// `[name] -> [value]`
    LoadDyn,
    /// `[value, name] -> []`
    StoreDyn,
    DefArgc,
    /// CALL SPRITE or CALL TEXT: starts going through the host's
    /// callbacks.
    CallbacksStart(Callbacks),
    /// Runs the next callback of the innermost CALL SPRITE or CALL TEXT,
    /// or jumps to the address when none are left.
    CallbackNext(Addr),
    /// `CALLIDX()`
    CallIdx,
    /// `[name, flag?] -> [found]`: CHKLABEL.
    ChkLabel {
        has_flag: bool,
    },
    /// `[name] -> [found]`: CHKCALL.
    ChkCall,
    /// `[name] -> [found]`: CHKVAR.
    ChkVar,
    /// `[message?] -> []`: STOP.
    Stop {
        has_message: bool,
    },
    /// `[n] -> [arg n]`
    DefArg,
    DefOutc,
    /// `[n, value] -> []`
    DefOut,
    /// `[var, to, step] -> [continue?]`
    ForTest,
    /// `[screen?, value] -> []`
    Print {
        screen: bool,
    },
    PrintTab {
        screen: bool,
    },
    PrintNewline {
        screen: bool,
    },
    /// `[frames?] -> []`
    Wait {
        vsync: bool,
        has_arg: bool,
    },
    /// `[prompt] -> [v1..vn]`: reads a line from the keyboard for INPUT
    /// (one value per kind, split at commas) or LINPUT (`whole_line`, one
    /// string). Yields until the line is complete. The prompt may be
    /// `Default` for none.
    Input {
        kinds: Box<[InputKind]>,
        question: bool,
        whole_line: bool,
    },
    /// Moves the DATA read position.
    Restore(u32),
    /// `[label string] -> []`
    RestoreDyn,
    /// `[] -> [next DATA value]`
    Read,
    End,
    /// Does nothing: marks where a statement starts.
    Nop,
    /// `[file or slot, slot?] -> []`: EXEC.
    Exec(u8),
}

#[derive(Clone, Debug, PartialEq)]
pub enum Literal {
    Int(i32),
    Real(f64),
    Str(Str),
}

#[derive(Clone, Debug, PartialEq)]
pub struct VarInfo {
    pub name: String,
    pub suffix: Option<Suffix>,
}

impl VarInfo {
    /// Name as VAR("...") spells it, suffix included.
    pub fn full_name(&self) -> String {
        full_name(&self.name, self.suffix)
    }
}

pub fn full_name(name: &str, suffix: Option<Suffix>) -> String {
    let mut s = name.to_string();
    match suffix {
        Some(Suffix::Int) => s.push('%'),
        Some(Suffix::Real) => s.push('#'),
        Some(Suffix::Str) => s.push('$'),
        None => {}
    }
    s
}

#[derive(Clone, Debug, PartialEq)]
pub struct DefInfo {
    pub name: String,
    pub entry: Addr,
    pub is_function: bool,
    /// COMMON DEF: callable from other program slots.
    pub common: bool,
    /// `None` when declared with `*`.
    pub params: Option<u8>,
    pub outs: Option<u8>,
    /// Parameters first, then OUT variables, then other locals.
    pub locals: Vec<VarInfo>,
    pub labels: HashMap<String, Addr>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Compiled {
    pub code: Vec<Op>,
    /// Source line of each instruction.
    pub lines: Vec<u32>,
    /// Column of the statement each instruction belongs to.
    pub cols: Vec<u32>,
    /// Whether each instruction starts a statement, NEXT and the other
    /// ends of loops included.
    pub starts: Vec<bool>,
    pub strings: Vec<Str>,
    pub globals: Vec<VarInfo>,
    pub labels: HashMap<String, Addr>,
    pub defs: Vec<DefInfo>,
    pub def_names: HashMap<String, u32>,
    pub data: Vec<Literal>,
    /// DATA position each label refers to for RESTORE.
    pub data_labels: HashMap<String, u32>,
    /// OPTION DEFINT: variables without a suffix start as integer 0.
    pub default_int: bool,
}
