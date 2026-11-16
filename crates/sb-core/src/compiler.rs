//! AST to bytecode.

use std::cell::Cell;
use std::collections::{HashMap, HashSet};

use crate::ast::*;
use crate::builtins::{self, Form, Kind};
use crate::bytecode::*;
use crate::constants::{self, ConstValue};
use crate::error::{Error, ErrorKind, Result};
use crate::host::Host;
use crate::lexer::{Span, Suffix};
use crate::ops;
use crate::value::{ElemType, Value, new_str};

type Key = (String, Option<Suffix>);

fn key(var: &Var) -> Key {
    (var.name.clone(), var.suffix)
}

/// Compiles `program`; `direct` for a line typed in direct mode, where
/// global declarations are checked as they run rather than here.
/// `unparsed` is the error of its [`StmtKind::Unparsed`], given when
/// compiling reaches it.
pub fn compile(
    program: &Program,
    host: &dyn Host,
    direct: bool,
    slot: i32,
    unparsed: Option<Error>,
) -> Result<Compiled> {
    let mut c = Compiler {
        host,
        direct,
        unparsed,
        first: program.stmts.first().map(|s| (s.line, s.col)),
        slot,
        out: Compiled::default(),
        global_slots: HashMap::new(),
        global_var_seen: HashSet::new(),
        global_used: HashSet::new(),
        global_made: Vec::new(),
        global_first: HashMap::new(),
        def_at: 0,
        strict_at: None,
        constants: HashMap::new(),
        pending: Vec::new(),
        defint_at: None,
        first_seen: HashMap::new(),
        def: None,
        loops: Vec::new(),
        patches: Vec::new(),
        line: 0,
        col: 0,
        temp_count: 0,
        constant_checked: false,
        constant_bytes: Cell::new(0),
        string_words: Cell::new(0),
    };
    c.prepare(program);
    // The first error in top-level code, and the statement it is in.
    let mut failed = None;
    for (i, stmt) in program.stmts.iter().enumerate() {
        if !matches!(stmt.kind, StmtKind::Def(_))
            && let Err(e) = c.stmt(stmt)
        {
            failed = Some((i, e));
            break;
        }
    }
    match failed {
        None => {
            c.emit(Op::End);
            c.resolve_patches(None)?;
        }
        // Only the DEFs before it still compile, to find an error of
        // theirs that comes first.
        Some(_) => {
            c.patches.clear();
            c.loops.clear();
        }
    }
    let before = failed.as_ref().map_or(program.stmts.len(), |(i, _)| *i);
    for (i, stmt) in program.stmts[..before].iter().enumerate() {
        if let StmtKind::Def(def) = &stmt.kind {
            if let Some(e) = c.take_pending(stmt) {
                return Err(e);
            }
            c.line = stmt.line;
            c.def_at = i;
            c.def(def, (stmt.line, stmt.col))?;
        }
    }
    if let Some((_, e)) = failed {
        return Err(e);
    }
    if let Some(e) = c.unparsed {
        return Err(e);
    }
    if let Some((_, e)) = c.pending.into_iter().next() {
        return Err(e);
    }
    crate::fuse::fuse(&mut c.out);
    Ok(c.out)
}

/// Compilation state for the DEF being compiled.
struct DefScope {
    index: u32,
    is_function: bool,
    locals: HashMap<Key, u32>,
    declared: HashSet<Key>,
    /// By local slot, whether the local was made ahead of time for a VAR
    /// further down that neither it nor a use has reached yet. Kept only
    /// with an OPTION STRICT, under which such a local does not exist yet.
    ahead: Vec<bool>,
}

struct Loop {
    breaks: Vec<usize>,
    continues: Vec<usize>,
}

/// A variable, or an element with its array and indices kept in
/// temporaries, for statements that read and then write it.
enum Place {
    Var(Slot),
    Index(Slot, Vec<Slot>),
    Dynamic(Slot),
}

struct Compiler<'h> {
    host: &'h dyn Host,
    out: Compiled,
    global_slots: HashMap<Key, u32>,
    /// VAR statements compiled so far at top level, to report duplicates.
    global_var_seen: HashSet<Key>,
    /// Globals that top-level code has used so far, in source order.
    global_used: HashSet<Key>,
    /// By global slot, whether a top-level VAR or use compiled so far made
    /// the global. Kept only with an OPTION STRICT.
    global_made: Vec<bool>,
    /// The top-level statement each global first appears in. A DEF sees
    /// only those from before it: SmileBASIC compiles in one pass.
    global_first: HashMap<Key, usize>,
    /// The statement the DEF being compiled is.
    def_at: usize,
    direct: bool,
    /// The error the program's [`StmtKind::Unparsed`] gives.
    unparsed: Option<Error>,
    /// Where the first statement starts: in DIRECT mode, the commands
    /// of DIRECT mode work only there, at the start of the line.
    first: Option<(u32, u32)>,
    /// The program slot compiled for, `#_SLOT`.
    slot: i32,
    /// Where the first OPTION STRICT stands. From there on, through and out
    /// of DEFs, a variable must exist before it is used, as SmileBASIC 4
    /// keeps the flag in its compile state.
    strict_at: Option<(u32, u32)>,
    /// The program's constants, each with where its name stands: a use
    /// before that place is Undefined constant, as SmileBASIC 4 compiles
    /// in one pass.
    constants: HashMap<String, (Literal, Span)>,
    /// Errors the pre-pass found, each given when compiling reaches its
    /// statement, so that the errors before it come first.
    pending: Vec<(*const Stmt, Error)>,
    /// Where the first OPTION DEFINT stands. The default type is int from
    /// there to the end of the program, DEFs included, as SmileBASIC 4
    /// keeps it in its compile state.
    defint_at: Option<(u32, u32)>,
    /// Where each variable of the scope being compiled is first seen,
    /// worked out only when there is a [`Self::defint_at`].
    first_seen: HashMap<Key, (u32, u32)>,
    def: Option<DefScope>,
    loops: Vec<Loop>,
    /// Jumps to labels, resolved when their scope is finished.
    patches: Vec<(usize, String, u32)>,
    line: u32,
    col: u32,
    temp_count: u32,
    /// The expression being compiled is part of a constant one already
    /// worked out without error, so its parts need no check of their own.
    constant_checked: bool,
    /// What the constant strings worked out in this statement have taken
    /// of [`CONSTANT_BYTES`].
    constant_bytes: Cell<usize>,
    /// What constant strings have taken of the code memory so far.
    string_words: Cell<usize>,
}

/// SmileBASIC 4 takes up to 254 arguments or OUT variables in a call,
/// and parameters or OUTs in a DEF.
const MOST_ARGUMENTS: usize = 254;

/// `Too many arguments` for more than [`MOST_ARGUMENTS`].
fn too_many(n: usize) -> Result<()> {
    match n > MOST_ARGUMENTS {
        true => Err(ErrorKind::TooManyArguments.into()),
        false => Ok(()),
    }
}

fn err(kind: ErrorKind, detail: impl Into<String>) -> Error {
    Error::with_detail(kind, detail)
}

/// The numbers of arguments of a function SmileBASIC 4 compiles in place
/// when it is called with brackets. Another number is `Wrong number of arguments`
/// as the program compiles. MIN and MAX are in the table too, but only
/// two arguments are compiled in place; other numbers are calls, which
/// check as they run.
fn in_place_counts(name: &str) -> Option<std::ops::RangeInclusive<usize>> {
    Some(match name {
        "DEFARGC" | "DEFOUTC" => 0..=0,
        "ABS" | "ACOS" | "ASC" | "ASIN" | "CEIL" | "CHKCALL" | "CHKVAR" | "CHR$" | "COS"
        | "COSH" | "DEFARG" | "DEG" | "FLOAT" | "FLOOR" | "INT" | "LAST" | "LEN" | "POP"
        | "RAD" | "ROUND" | "SGN" | "SHIFT" | "SIN" | "SINH" | "SQR" | "TAN" | "TANH"
        | "TYPEOF" | "VAL" => 1..=1,
        "LEFT$" | "POW" | "RIGHT$" => 2..=2,
        "ATAN" | "CHKLABEL" | "DIM" | "HEX$" | "LOG" | "RND" | "STR$" => 1..=2,
        "EXP" | "RNDF" => 0..=1,
        _ => return None,
    })
}

/// `Wrong number of arguments` for a function compiled in place called
/// with `argc` arguments it does not take, before they are compiled.
fn check_in_place(name: &str, argc: usize) -> Result<()> {
    match in_place_counts(name) {
        Some(takes) if !takes.contains(&argc) => Err(err(ErrorKind::WrongNumberOfArguments, name)),
        _ => Ok(()),
    }
}

fn elem_type(suffix: Option<Suffix>, default_int: bool) -> ElemType {
    match suffix {
        Some(Suffix::Int) => ElemType::Int,
        Some(Suffix::Real) => ElemType::Real,
        Some(Suffix::Str) => ElemType::Str,
        None if default_int => ElemType::Int,
        None => ElemType::Real,
    }
}

/// Whether an `IF` condition is an array element, which costs
/// SmileBASIC 4 more than a comparison (TEST65, TEST70) or a variable,
/// `!` or a function's value (TEST69's HGIMEUPDATE). UNVERIFIED: what
/// a string's character costs.
fn tests_element(cond: &Expr) -> bool {
    matches!(cond, Expr::Index { .. })
}

/// ENUM's value from a real, which must be a whole number in the 32-bit
/// range: a fraction (or a NaN) is `Type mismatch`, and a value outside
/// is `Out of range`.
fn enum_int(x: f64) -> std::result::Result<i32, ErrorKind> {
    // A NaN is not its own truncation either; an infinity is.
    if x != x.trunc() {
        return Err(ErrorKind::TypeMismatch);
    }
    if !(-2147483648.0..=2147483647.0).contains(&x) {
        return Err(ErrorKind::OutOfRange);
    }
    Ok(x as i32)
}

/// Upper-case name as calls look it up, `MID$` style.
fn call_name(var: &Var) -> String {
    full_name(&var.name, var.suffix)
}

/// Turns an argument of INC/DEC into the variable it modifies.
fn expr_to_lvalue(expr: &Expr) -> Option<LValue> {
    match expr {
        Expr::Var(v) => Some(LValue::Var(v.clone())),
        Expr::Index { base, indices } => Some(LValue::Index {
            base: Box::new(expr_to_lvalue(base)?),
            indices: indices.clone(),
        }),
        Expr::DynamicVar(name) => Some(LValue::Dynamic(name.clone())),
        _ => None,
    }
}

impl Compiler<'_> {
    // ---- pre-pass ----

    /// The errors it finds wait in [`Compiler::pending`]: the first of the
    /// DEF lines, and the first of [`Self::walk_source_order`].
    fn prepare(&mut self, program: &Program) {
        let mut failed = None;
        for stmt in &program.stmts {
            (self.line, self.col) = (stmt.line, stmt.col);
            let mut fail = |e: Error| _ = failed.get_or_insert((stmt as *const Stmt, e));
            let StmtKind::Def(def) = &stmt.kind else {
                continue;
            };
            let name = call_name(&def.name);
            let index = self.out.defs.len() as u32;
            // A DEF may not take the name of a system function, such as
            // LAST. UNVERIFIED: that a command's name is refused too.
            // The DEFs after an error are still named, for the code
            // before it.
            if crate::vm::is_system_call(&name, self.host) || self.out.def_names.contains_key(&name)
            {
                fail(err(ErrorKind::DuplicateFunction, name).at_pos(stmt.line, stmt.col));
                continue;
            }
            self.out.def_names.insert(name.clone(), index);
            for list in [&def.params, &def.outs] {
                if let Params::Fixed(list) = list
                    && let Err(e) = too_many(list.len())
                {
                    fail(e.at_pos(stmt.line, stmt.col));
                }
            }
            self.out.defs.push(DefInfo {
                name,
                entry: 0,
                end: 0,
                is_function: def.is_function,
                common: def.common,
                params: match &def.params {
                    Params::Fixed(p) => Some(p.len() as u8),
                    Params::Variadic => None,
                },
                outs: match &def.outs {
                    Params::Fixed(o) => Some(o.len() as u8),
                    Params::Variadic => None,
                },
                locals: Vec::new(),
                labels: HashMap::new(),
            });
        }
        self.pending.extend(failed);
        self.out.def_data_labels = vec![HashMap::new(); self.out.defs.len()];
        if let Err(failed) = self.walk_source_order(&program.stmts, None) {
            self.pending.push(failed);
        }
        self.out.default_int = self.defint_at.is_some();
        if self.defint_at.is_some() {
            let top = program
                .stmts
                .iter()
                .filter(|s| !matches!(s.kind, StmtKind::Def(_)));
            for stmt in top {
                first_seen(std::slice::from_ref(stmt), &mut self.first_seen);
            }
        }

        // Every variable named in top-level code is global, and DEFs see it.
        let mut used = Vec::new();
        let mut declared = Vec::new();
        for (i, stmt) in program.stmts.iter().enumerate() {
            if !matches!(stmt.kind, StmtKind::Def(_)) {
                let (u, d) = (used.len(), declared.len());
                collect_vars(std::slice::from_ref(stmt), &mut used, &mut declared);
                for k in used[u..].iter().chain(&declared[d..]) {
                    self.global_first.entry(k.clone()).or_insert(i);
                }
            }
        }
        for k in used.into_iter().chain(declared) {
            self.global_slot(k);
        }
    }

    /// CONST/ENUM definitions, DATA and its labels, in source order across
    /// DEFs, up to the first statement with an error, which it gives.
    /// `def` is the index of the DEF `stmts` are in.
    fn walk_source_order(
        &mut self,
        stmts: &[Stmt],
        def: Option<u32>,
    ) -> std::result::Result<(), (*const Stmt, Error)> {
        for stmt in stmts {
            self.walk_stmt(stmt, def)
                .map_err(|e| (stmt as *const Stmt, e))?;
            let def = match &stmt.kind {
                StmtKind::Def(d) => self.out.def_names.get(&call_name(&d.name)).copied(),
                _ => def,
            };
            for body in child_blocks(&stmt.kind) {
                self.walk_source_order(body, def)?;
            }
        }
        Ok(())
    }

    /// [`Self::walk_source_order`] of `stmt` without the blocks in it.
    fn walk_stmt(&mut self, stmt: &Stmt, def: Option<u32>) -> Result<()> {
        (self.line, self.col) = (stmt.line, stmt.col);
        self.constant_bytes.set(0);
        let at = |e: Error| e.at_pos(stmt.line, stmt.col);
        // A value that is not worked out as the program compiles is a
        // Syntax error in DATA, but its own error in CONST and ENUM.
        let illegal = |mut e: Error| {
            if e.kind == ErrorKind::SyntaxError {
                e.kind = ErrorKind::IllegalConstantExpression;
            }
            e
        };
        match &stmt.kind {
            StmtKind::Label(name) => {
                let labels = match def {
                    Some(d) => &mut self.out.def_data_labels[d as usize],
                    None => &mut self.out.data_labels,
                };
                labels
                    .entry(name.clone())
                    .or_insert(self.out.data.len() as u32);
            }
            StmtKind::Option(o) => self.option(o, stmt)?,
            // SmileBASIC 4 checks a CONST name after working out its
            // value, and an ENUM name before.
            StmtKind::Const(items) => {
                for (name, span, expr) in items {
                    let value = self.const_eval(expr).map_err(illegal).map_err(at)?;
                    self.check_new_constant(name, *span)?;
                    self.constants.insert(name.clone(), (value, *span));
                }
            }
            StmtKind::Enum(items) => {
                let mut next = 0i32;
                for (name, span, expr) in items {
                    self.check_new_constant(name, *span)?;
                    let n = match expr {
                        None => next,
                        Some((e, place)) => {
                            let value = match self.const_eval(e).map_err(illegal).map_err(at)? {
                                Literal::Int(n) => Ok(n),
                                Literal::Real(x) => enum_int(x),
                                Literal::Str(_) => Err(ErrorKind::TypeMismatch),
                            };
                            value.map_err(|kind| Error::from(kind).at_pos(place.line, place.col))?
                        }
                    };
                    self.constants
                        .insert(name.clone(), (Literal::Int(n), *span));
                    next = n.wrapping_add(1);
                }
            }
            StmtKind::Data(items) => {
                for e in items {
                    let value = self.const_eval(e).map_err(at)?;
                    self.out.data.push(value);
                }
            }
            _ => {}
        }
        Ok(())
    }

    /// OPTION, which may stand anywhere, in a DEF too, and holds from
    /// there on. Another name is a Syntax error.
    #[cold]
    fn option(&mut self, name: &str, stmt: &Stmt) -> Result<()> {
        let at = Some((stmt.line, stmt.col));
        match name {
            "DEFINT" => self.defint_at = self.defint_at.or(at),
            "STRICT" => self.strict_at = self.strict_at.or(at),
            _ => {
                return Err(err(ErrorKind::SyntaxError, format!("OPTION {name}"))
                    .at_pos(stmt.line, stmt.col));
            }
        }
        Ok(())
    }

    /// `Duplicate constant` if CONST or ENUM may not define `#name`, which
    /// stands at `at`: a built-in constant, or one defined before anywhere
    /// in this program, in a DEF or not. Other slots have their own.
    fn check_new_constant(&self, name: &str, at: Span) -> Result<()> {
        let builtin =
            matches!(name, "_LINE" | "_SLOT" | "_FILENAME") || constants::lookup(name).is_some();
        if builtin || self.constants.contains_key(name) {
            return Err(
                err(ErrorKind::DuplicateConstant, format!("#{name}")).at_pos(at.line, at.col)
            );
        }
        Ok(())
    }

    /// The value of constant `#name`, which stands at `at`. Undefined
    /// constant is placed there, not at the statement or its `=`.
    // UNVERIFIED: for constants outside an assignment's value.
    fn constant(&self, name: &str, at: Span) -> Result<Literal> {
        match self.constants.get(name) {
            Some((v, defined)) if (defined.line, defined.col) < (at.line, at.col) => {
                return Ok(v.clone());
            }
            // Defined later on: no built-in has its name.
            Some(_) => {
                return Err(
                    err(ErrorKind::UndefinedConstant, format!("#{name}")).at_pos(at.line, at.col)
                );
            }
            None => {}
        }
        match name {
            // 0 in DIRECT mode.
            "_LINE" if self.direct => return Ok(Literal::Int(0)),
            "_LINE" => return Ok(Literal::Int(self.line as i32)),
            "_SLOT" => return Ok(Literal::Int(self.slot)),
            // PRGNAME$'s name in a program, empty in direct mode.
            "_FILENAME" if self.direct => return Ok(Literal::Str(Vec::new())),
            "_FILENAME" => {
                let name = self.host.program_name(self.slot as usize);
                return Ok(Literal::Str(name));
            }
            _ => {}
        }
        match constants::lookup(name) {
            Some(ConstValue::Int(n)) => Ok(Literal::Int(n)),
            Some(ConstValue::Real(x)) => Ok(Literal::Real(x)),
            None => {
                Err(err(ErrorKind::UndefinedConstant, format!("#{name}")).at_pos(at.line, at.col))
            }
        }
    }

    /// Evaluates a constant expression (CONST, ENUM, DATA).
    fn const_eval(&self, expr: &Expr) -> Result<Literal> {
        let value = self.const_value(expr)?;
        Ok(match value {
            Value::Int(n) => Literal::Int(n),
            Value::Real(x) => Literal::Real(x),
            Value::Str(s) => Literal::Str(s.borrow().clone()),
            _ => return Err(ErrorKind::TypeMismatch.into()),
        })
    }

    /// Works out a constant expression: its first operand, then each
    /// operator up the chain in turn (see [`Expr::chain_operand`]).
    fn const_value(&self, expr: &Expr) -> Result<Value> {
        let (links, first) = chain(expr, false);
        let mut value = self.const_operand(first)?;
        for link in links.iter().rev() {
            value = match link {
                Expr::Unary { op, .. } => ops::unary(*op, &value)?,
                Expr::Binary { op, rhs, .. } => {
                    self.const_binary(*op, value, &self.const_value(rhs)?)?
                }
                _ => unreachable!("chain(_, false) stops at indexing"),
            };
        }
        Ok(value)
    }

    /// [`Compiler::const_value`] of anything but an operator.
    fn const_operand(&self, expr: &Expr) -> Result<Value> {
        Ok(match expr {
            Expr::Int(n) => {
                self.take_constant_node();
                Value::Int(*n)
            }
            Expr::Real(x) => {
                self.take_constant_node();
                Value::Real(*x)
            }
            Expr::Str(s) => {
                self.take_constant_node();
                Value::Str(new_str(s.clone()))
            }
            Expr::Label(name) => Value::Str(new_str(format!("@{name}").encode_utf16().collect())),
            Expr::Constant(name, at) => {
                self.take_constant_node();
                literal_value(&self.constant(name, *at)?)
            }
            Expr::Call { name, args } if builtins::is_constant_function(&call_name(name)) => {
                let b = builtins::get(
                    builtins::lookup(&call_name(name), Form::Function)
                        .expect("constant functions are builtins"),
                );
                let args = args
                    .iter()
                    .map(|a| {
                        a.as_ref()
                            .map_or(Ok(Value::Default), |a| self.const_value(a))
                    })
                    .collect::<Result<Vec<_>>>()?;
                builtins::check_constant_args(b.name, &args)?;
                match b.kind {
                    Kind::Function(f) if b.accepts(args.len()) => f(&args)?,
                    Kind::Function(_) => return Err(ErrorKind::WrongNumberOfArguments.into()),
                    _ => return Err(ErrorKind::IllegalFunctionCall.into()),
                }
            }
            _ => return Err(err(ErrorKind::SyntaxError, "not a constant expression")),
        })
    }

    // ---- emitting ----

    fn emit(&mut self, op: Op) -> usize {
        self.out.code.push(op);
        self.out.lines.push(self.line);
        self.out.cols.push(self.col);
        self.out.starts.push(false);
        self.out.code.len() - 1
    }

    /// Marks `at` as the start of a statement, if anything was emitted
    /// there.
    fn mark_start(&mut self, at: usize) {
        if let Some(s) = self.out.starts.get_mut(at) {
            *s = true;
        }
    }

    /// Puts the two values pushed from `at`, the second one on top, back
    /// in order. Where each was a single push that cannot be seen to
    /// happen early, such as a constant or a variable, the two pushes
    /// trade places instead, which saves a Swap each time it runs.
    fn put_back_two(&mut self, at: usize) {
        let simple = |op: &Op| matches!(op, Op::Int(_) | Op::Real(_) | Op::Str(_) | Op::Load(_));
        let code = &mut self.out.code;
        if code.len() == at + 2 && simple(&code[at]) && simple(&code[at + 1]) {
            code.swap(at, at + 1);
            self.out.lines.swap(at, at + 1);
            self.out.cols.swap(at, at + 1);
            self.out.starts.swap(at, at + 1);
        } else {
            self.emit(Op::Swap);
        }
    }

    fn here(&self) -> Addr {
        self.out.code.len() as Addr
    }

    fn patch(&mut self, at: usize, target: Addr) {
        match &mut self.out.code[at] {
            Op::Jump(a)
            | Op::JumpIfFalse(a)
            | Op::TestIfFalse(a)
            | Op::JumpIfTrue(a)
            | Op::Gosub(a)
            | Op::OnBreak(a)
            | Op::CallbackNext(a) => *a = target,
            other => unreachable!("patching non-jump {other:?}"),
        }
    }

    fn string_const(&mut self, s: Vec<u16>) -> u32 {
        self.take_string_words(s.len());
        self.out.strings.push(s);
        (self.out.strings.len() - 1) as u32
    }

    /// POP or SHIFT of what proved a string constant, compiled from `start`
    /// on: as a constant takes no block, it is not made for them to work on.
    /// POP's result is known now. An empty constant is an error as it runs.
    fn take_from_constant(&mut self, start: usize, arg: &Expr, shift: bool) -> bool {
        let bytes = self.constant_bytes.get();
        let value = self.const_value(arg);
        self.constant_bytes.set(bytes);
        let Ok(Value::Str(constant)) = value else {
            return false;
        };
        let constant = constant.borrow();
        let (Some(&first), Some(&last)) = (constant.first(), constant.last()) else {
            return false;
        };
        self.out.code.truncate(start);
        self.out.lines.truncate(start);
        self.out.cols.truncate(start);
        self.out.starts.truncate(start);
        let op = if shift {
            Op::ShiftConstant {
                first: self.string_const(vec![first]),
                len: constant.len() as u32,
            }
        } else {
            Op::Str(self.string_const(vec![last]))
        };
        self.emit(op);
        true
    }

    fn scope_index(&self) -> u32 {
        self.def.as_ref().map_or(u32::MAX, |d| d.index)
    }

    fn labels_mut(&mut self) -> &mut HashMap<String, Addr> {
        match &self.def {
            Some(d) => &mut self.out.defs[d.index as usize].labels,
            None => &mut self.out.labels,
        }
    }

    fn jump_to_label(&mut self, op: Op, label: &str) {
        let at = self.emit(op);
        let scope = self.scope_index();
        self.patches.push((at, label.to_string(), scope));
    }

    fn resolve_patches(&mut self, def: Option<u32>) -> Result<()> {
        let scope = def.unwrap_or(u32::MAX);
        let patches = std::mem::take(&mut self.patches);
        for (at, label, s) in patches {
            debug_assert_eq!(s, scope);
            let labels = match def {
                Some(d) => &self.out.defs[d as usize].labels,
                None => &self.out.labels,
            };
            // ON BREAK GOTO in a DEF may name a label outside it, all of
            // which are known by then. One outside comes before a label of
            // the DEF's own.
            let outside = matches!(self.out.code[at], Op::OnBreak(_))
                .then(|| self.out.labels.get(&label))
                .flatten();
            // A label that is not there is an error only when the jump
            // runs: SmileBASIC Debugger's generated code jumps to labels
            // it writes only when needed.
            let target = match outside.or_else(|| labels.get(&label)) {
                Some(&target) => target,
                None => {
                    (self.line, self.col) = (self.out.lines[at], self.out.cols[at]);
                    let name = self.string_const(format!("@{label}").encode_utf16().collect());
                    self.emit(Op::UndefinedLabel(name)) as Addr
                }
            };
            self.patch(at, target);
        }
        Ok(())
    }

    // ---- variables ----

    /// Whether variable `k`, made now, starts as an int: it has no suffix
    /// and OPTION DEFINT stands before where it is first seen, or before
    /// the statement compiling if it is not seen in the source.
    fn starts_int(&self, k: &Key) -> bool {
        let Some(defint) = self.defint_at else {
            return false;
        };
        let seen = self.first_seen.get(k).copied();
        k.1.is_none() && seen.unwrap_or((self.line, self.col)) > defint
    }

    fn var_info(&self, slot: Slot) -> &VarInfo {
        match slot {
            Slot::Global(n) => &self.out.globals[n as usize],
            Slot::Local(n) => {
                let def = self.def.as_ref().expect("a local is in a DEF").index;
                &self.out.defs[def as usize].locals[n as usize]
            }
        }
    }

    fn global_slot(&mut self, k: Key) -> u32 {
        if let Some(&slot) = self.global_slots.get(&k) {
            return slot;
        }
        let slot = self.out.globals.len() as u32;
        if self.strict_at.is_some() {
            self.global_made.push(false);
        }
        let int = self.starts_int(&k);
        self.out.globals.push(VarInfo {
            name: k.0.clone(),
            suffix: k.1,
            array: false,
            int,
        });
        self.global_slots.insert(k, slot);
        slot
    }

    fn local_slot(&mut self, k: Key) -> u32 {
        if let Some(&slot) = self.def.as_ref().expect("inside a DEF").locals.get(&k) {
            return slot;
        }
        let int = self.starts_int(&k);
        let def = self.def.as_mut().expect("inside a DEF");
        let info = &mut self.out.defs[def.index as usize];
        let slot = info.locals.len() as u32;
        info.locals.push(VarInfo {
            name: k.0.clone(),
            suffix: k.1,
            array: false,
            int,
        });
        def.locals.insert(k, slot);
        slot
    }

    /// The variable `var` names. A use makes one that does not exist yet,
    /// except under OPTION STRICT.
    fn slot(&mut self, var: &Var) -> Result<Slot> {
        if self.strict_at.is_some() {
            return self.strict_slot(var);
        }
        let k = key(var);
        if let Some(def) = &self.def {
            if let Some(&slot) = def.locals.get(&k) {
                return Ok(Slot::Local(slot));
            }
            if let Some(&slot) = self.global_slots.get(&k) {
                // A DEF sees the globals top-level code above it made.
                if self.global_first.get(&k).is_some_and(|&i| i < self.def_at) {
                    return Ok(Slot::Global(slot));
                }
            }
        } else if let Some(&slot) = self.global_slots.get(&k) {
            if !self.global_used.contains(&k) {
                self.global_used.insert(k);
            }
            return Ok(Slot::Global(slot));
        }
        Ok(match self.def {
            Some(_) => Slot::Local(self.local_slot(k)),
            None => Slot::Global(self.global_slot(k)),
        })
    }

    /// [`Self::slot`] in a program with an OPTION STRICT. From where it
    /// stands, a use of a variable that does not exist yet is `Undefined
    /// variable`. SmileBASIC 4 looks in the DEF's variables, then in the
    /// globals made so far, all in one pass.
    #[inline(never)]
    fn strict_slot(&mut self, var: &Var) -> Result<Slot> {
        let k = key(var);
        let strict = self.strict_at.is_some_and(|at| (self.line, self.col) >= at);
        let found = match &self.def {
            Some(def) => match def.locals.get(&k) {
                Some(&n) => Some(Slot::Local(n)),
                // A DEF sees the globals top-level code above it made, by
                // now compiled past this DEF.
                None => match self.global_first.get(&k) {
                    Some(&i) if i < self.def_at => {
                        self.global_slots.get(&k).map(|&n| Slot::Global(n))
                    }
                    _ => None,
                },
            },
            None => self.global_slots.get(&k).map(|&n| Slot::Global(n)),
        };
        if let Some(slot) = found {
            if self.exists(slot) {
                return Ok(slot);
            }
            if !strict {
                match slot {
                    // A DEF makes no global.
                    Slot::Global(_) if self.def.is_some() => {}
                    Slot::Global(_) => {
                        self.global_used.insert(k);
                        self.made(slot);
                    }
                    Slot::Local(_) => self.made(slot),
                }
                return Ok(slot);
            }
            // A local made ahead for a VAR further down, where SmileBASIC 4
            // takes a global of the name if top-level code above made one.
            // This keeps the local, as it does without OPTION STRICT.
            if let Slot::Local(_) = slot {
                let global = match self.global_first.get(&k) {
                    Some(&i) if i < self.def_at => self.global_slots.get(&k).copied(),
                    _ => None,
                };
                if global.is_some_and(|n| self.exists(Slot::Global(n))) {
                    return Ok(slot);
                }
            }
        }
        if strict {
            return Err(err(
                ErrorKind::UndefinedVariable,
                full_name(&var.name, var.suffix),
            ));
        }
        let slot = match self.def {
            Some(_) => Slot::Local(self.local_slot(k)),
            None => {
                let n = self.global_slot(k.clone());
                self.global_used.insert(k);
                Slot::Global(n)
            }
        };
        self.made(slot);
        Ok(slot)
    }

    /// Whether `slot` exists by now, in a program with an OPTION STRICT:
    /// a global that a top-level VAR or use above made, or a local that is
    /// not waiting for its VAR.
    fn exists(&self, slot: Slot) -> bool {
        match slot {
            Slot::Global(n) => self.global_made[n as usize],
            Slot::Local(n) => {
                let ahead = &self.def.as_ref().expect("a local is in a DEF").ahead;
                !ahead.get(n as usize).copied().unwrap_or(false)
            }
        }
    }

    /// Records that `slot` exists from here on (see [`Self::exists`]).
    fn made(&mut self, slot: Slot) {
        match slot {
            Slot::Global(n) => self.global_made[n as usize] = true,
            Slot::Local(n) => {
                let ahead = &mut self.def.as_mut().expect("a local is in a DEF").ahead;
                if let Some(a) = ahead.get_mut(n as usize) {
                    *a = false;
                }
            }
        }
    }

    /// A hidden variable for FOR limits, CASE subjects and TPRINT screens.
    fn temp(&mut self) -> Slot {
        self.temp_count += 1;
        // A NUL cannot appear in source names, so VAR("...") never finds it.
        let k = (format!("\0T{}", self.temp_count), None);
        match self.def {
            Some(_) => Slot::Local(self.local_slot(k)),
            None => Slot::Global(self.global_slot(k)),
        }
    }

    // ---- statements ----

    /// The pre-pass's error in `stmt`, if any.
    fn take_pending(&mut self, stmt: &Stmt) -> Option<Error> {
        if self.pending.is_empty() {
            return None;
        }
        let i = self
            .pending
            .iter()
            .position(|(s, _)| std::ptr::eq(*s, stmt))?;
        Some(self.pending.swap_remove(i).1)
    }

    fn block(&mut self, stmts: &[Stmt]) -> Result<()> {
        stmts.iter().try_for_each(|s| self.stmt(s))
    }

    fn stmt(&mut self, stmt: &Stmt) -> Result<()> {
        if let Some(e) = self.take_pending(stmt) {
            return Err(e);
        }
        (self.line, self.col) = (stmt.line, stmt.col);
        let start = self.out.code.len();
        self.constant_bytes.set(0);
        self.stmt_kind(&stmt.kind)
            .map_err(|e| e.at_pos(stmt.line, stmt.col))?;
        self.check_code_memory()
            .map_err(|e| e.at_pos(stmt.line, stmt.col))?;
        // A FOR's own start is no statement for TRACE: it runs on into
        // the body (checked on hardware). A label emits nothing.
        if !matches!(stmt.kind, StmtKind::For { .. } | StmtKind::Label(_)) {
            self.mark_start(start);
        }
        Ok(())
    }

    fn stmt_kind(&mut self, kind: &StmtKind) -> Result<()> {
        match kind {
            StmtKind::Label(name) => {
                let here = self.here();
                if self.labels_mut().insert(name.clone(), here).is_some() {
                    return Err(err(ErrorKind::DuplicateLabel, format!("@{name}")));
                }
            }
            StmtKind::Assign {
                target,
                value,
                eq_col,
            } => {
                // An error found as the value compiles, as a division by
                // zero of constants, is placed at the `=`, as one found as
                // it parses is.
                // UNVERIFIED: for errors other than Divide by zero.
                self.expr(value).map_err(|e| e.at_pos(self.line, *eq_col))?;
                self.store(target)?;
            }
            StmtKind::Var(decls) => {
                for d in decls {
                    self.var_decl(d)?;
                }
            }
            StmtKind::Const(_) | StmtKind::Enum(_) | StmtKind::Option(_) | StmtKind::Data(_) => {}
            StmtKind::Command { name, args, outs } => self.command(name, args, outs)?,
            StmtKind::Call { name, args, outs } => {
                too_many(outs.len())?;
                // The arguments before the name (checked on hardware).
                self.args(args)?;
                self.expr(name)?;
                // [args.., name] -> [name, args..]
                self.reorder(args.len() + 1);
                self.reorder(args.len());
                let want = Want::Outs(outs.len() as u8);
                self.emit(Op::CallDyn {
                    argc: args.len() as u8,
                    want,
                });
                self.store_outs(outs)?;
            }
            StmtKind::CallSprite | StmtKind::CallText => {
                self.emit(Op::CallbacksStart(match kind {
                    StmtKind::CallSprite => Callbacks::Sprite,
                    _ => Callbacks::Text,
                }));
                let top = self.here();
                let next = self.emit(Op::CallbackNext(0));
                self.emit(Op::Jump(top));
                let end = self.here();
                self.patch(next, end);
            }
            StmtKind::Print { screen, items } => self.print(screen.as_ref(), items)?,
            StmtKind::Input {
                prompt,
                question,
                targets,
                whole_line,
            } => {
                // VAR's name, when only running it tells what it holds.
                let mut named = Vec::with_capacity(targets.len());
                for target in targets {
                    named.push(match target {
                        LValue::Dynamic(name) if !matches!(&**name, Expr::Str(_)) => {
                            self.expr(name)?;
                            let tmp = self.temp();
                            self.emit(Op::Store(tmp));
                            Some(tmp)
                        }
                        _ => None,
                    });
                }
                match prompt {
                    Some(p) => self.expr(p)?,
                    None => _ = self.emit(Op::Default),
                }
                let kinds = targets
                    .iter()
                    .zip(&named)
                    .map(|(target, &tmp)| tmp.map_or_else(|| input_kind(target), InputKind::Named))
                    .collect();
                self.emit(Op::Input {
                    kinds,
                    question: *question,
                    whole_line: *whole_line,
                });
                for (target, tmp) in targets.iter().zip(&named).rev() {
                    match tmp {
                        Some(tmp) => self.store_place(&Place::Dynamic(*tmp)),
                        None => self.store(target)?,
                    }
                }
            }
            StmtKind::If {
                branches,
                otherwise,
            } => {
                let mut ends = Vec::new();
                for (cond, body) in branches {
                    self.expr(cond)?;
                    let skip = match tests_element(cond) {
                        true => self.emit(Op::TestIfFalse(0)),
                        false => self.emit(Op::JumpIfFalse(0)),
                    };
                    self.block(body)?;
                    ends.push(self.emit(Op::Jump(0)));
                    let next = self.here();
                    self.patch(skip, next);
                }
                if let Some(body) = otherwise {
                    self.block(body)?;
                }
                let end = self.here();
                ends.into_iter().for_each(|j| self.patch(j, end));
            }
            StmtKind::Case {
                subject,
                arms,
                otherwise,
            } => {
                let tmp = self.temp();
                self.expr(subject)?;
                self.emit(Op::Store(tmp));
                let mut ends = Vec::new();
                for arm in arms {
                    let mut to_body = Vec::new();
                    for value in &arm.values {
                        self.emit(Op::Load(tmp));
                        self.expr(value)?;
                        self.emit(Op::Binary(BinaryOp::Eq));
                        to_body.push(self.emit(Op::JumpIfTrue(0)));
                    }
                    let skip = self.emit(Op::Jump(0));
                    let body = self.here();
                    to_body.into_iter().for_each(|j| self.patch(j, body));
                    self.block(&arm.body)?;
                    ends.push(self.emit(Op::Jump(0)));
                    let next = self.here();
                    self.patch(skip, next);
                }
                if let Some(body) = otherwise {
                    self.block(body)?;
                }
                let end = self.here();
                ends.into_iter().for_each(|j| self.patch(j, end));
            }
            StmtKind::For {
                var,
                from,
                to,
                step,
                body,
            } => {
                let (to_slot, step_slot) = (self.temp(), self.temp());
                self.expr(from)?;
                self.store(var)?;
                // NEXT goes back to the step and then the test, each a
                // statement on the FOR's line for TRACE; FOR itself goes
                // straight to the test (checked on hardware).
                let enter = self.emit(Op::Jump(0));
                let step_at = self.emit(Op::Nop);
                self.mark_start(step_at);
                self.load_lvalue(var)?;
                self.emit(Op::Load(step_slot));
                self.emit(Op::Binary(BinaryOp::Add));
                self.store(var)?;
                let test_at = self.emit(Op::Nop);
                self.mark_start(test_at);
                let check = self.here();
                self.patch(enter, check);
                // STEP, then TO, are worked out again before each test
                // (checked on hardware).
                match step {
                    Some(s) => self.expr(s)?,
                    None => _ = self.emit(Op::Int(1)),
                }
                self.emit(Op::Store(step_slot));
                self.expr(to)?;
                self.emit(Op::Store(to_slot));
                self.load_lvalue(var)?;
                self.emit(Op::Load(to_slot));
                self.emit(Op::Load(step_slot));
                self.emit(Op::ForTest);
                let exit = self.emit(Op::JumpIfFalse(0));
                let lp = self.loop_body(body)?;
                self.emit(Op::Jump(step_at as Addr));
                let end = self.here();
                self.patch(exit, end);
                self.finish_loop(lp, step_at as Addr, end);
            }
            StmtKind::While { cond, body } => {
                let check = self.here();
                self.expr(cond)?;
                let exit = self.emit(Op::JumpIfFalse(0));
                let lp = self.loop_body(body)?;
                let wend = self.emit(Op::Jump(check));
                self.mark_start(wend);
                let end = self.here();
                self.patch(exit, end);
                self.finish_loop(lp, check, end);
            }
            StmtKind::Repeat { body, cond } => {
                let top = self.here();
                let lp = self.loop_body(body)?;
                let check = self.here();
                self.expr(cond)?;
                self.mark_start(check as usize);
                self.emit(Op::JumpIfFalse(top));
                let end = self.here();
                self.finish_loop(lp, check, end);
            }
            StmtKind::Loop { body } => {
                let top = self.here();
                let lp = self.loop_body(body)?;
                let endloop = self.emit(Op::Jump(top));
                self.mark_start(endloop);
                let end = self.here();
                self.finish_loop(lp, top, end);
            }
            StmtKind::Break | StmtKind::Continue => {
                let j = self.emit(Op::Jump(0));
                // Outside a loop, a Syntax error as it compiles.
                let lp = self.loops.last_mut().ok_or(ErrorKind::SyntaxError)?;
                match kind {
                    StmtKind::Break => lp.breaks.push(j),
                    _ => lp.continues.push(j),
                }
            }
            StmtKind::Goto(Target::Label(l)) => self.jump_to_label(Op::Jump(0), l),
            StmtKind::Gosub(Target::Label(l)) => self.jump_to_label(Op::Gosub(0), l),
            StmtKind::Goto(Target::Expr(e)) => {
                self.expr(e)?;
                self.emit(Op::GotoDyn);
            }
            StmtKind::Gosub(Target::Expr(e)) => {
                self.expr(e)?;
                self.emit(Op::GosubDyn);
            }
            StmtKind::Goto(Target::Missing) => {
                self.emit(Op::Default);
                self.emit(Op::GotoDyn);
            }
            StmtKind::Gosub(Target::Missing) => {
                self.emit(Op::Default);
                self.emit(Op::GosubDyn);
            }
            StmtKind::On {
                selector,
                labels,
                gosub,
            } => {
                self.expr(selector)?;
                self.emit(Op::On {
                    count: labels.len() as u32,
                    gosub: *gosub,
                });
                for l in labels {
                    self.jump_to_label(Op::Jump(0), l);
                }
            }
            StmtKind::OnBreakGoto(l) if l.is_empty() => _ = self.emit(Op::NoBreak),
            StmtKind::OnBreakGoto(l) => self.jump_to_label(Op::OnBreak(0), l),
            StmtKind::Return(None) => _ = self.emit(Op::Return),
            StmtKind::Return(Some(e)) => {
                if self.def.as_ref().is_some_and(|d| d.is_function) {
                    self.expr(e)?;
                    self.emit(Op::ReturnValue);
                } else {
                    // The value of a RETURN in a command DEF is ignored,
                    // and not even compiled.
                    self.emit(Op::Return);
                }
            }
            StmtKind::End => _ = self.emit(Op::End),
            StmtKind::Def(_) => unreachable!("DEFs are compiled separately"),
            StmtKind::Read(targets) => {
                for t in targets {
                    self.emit(Op::Read);
                    self.store(t)?;
                }
            }
            // A DEF's own labels first, then the top level's, all known by
            // now. A label that is not there is an error only when the
            // RESTORE runs.
            StmtKind::Restore(Target::Label(l)) => match (self.def.as_ref())
                .and_then(|d| self.out.def_data_labels[d.index as usize].get(l))
                .or_else(|| self.out.data_labels.get(l))
            {
                Some(&pos) => _ = self.emit(Op::Restore(pos)),
                None => {
                    let name = self.string_const(format!("@{l}").encode_utf16().collect());
                    self.emit(Op::Str(name));
                    self.emit(Op::RestoreDyn);
                }
            },
            StmtKind::Restore(Target::Expr(e)) => {
                self.expr(e)?;
                self.emit(Op::RestoreDyn);
            }
            StmtKind::Restore(Target::Missing) => unreachable!("RESTORE's target is optional"),
            StmtKind::Unparsed => {
                let e = self.unparsed.take();
                return Err(e.unwrap_or_else(|| ErrorKind::SyntaxError.into()));
            }
            StmtKind::Swap(a, b) => {
                // Each place is worked out once, the second first (checked
                // on hardware).
                let b = self.place(b)?;
                let a = self.place(a)?;
                self.load_place(&a);
                self.load_place(&b);
                self.store_place(&a);
                self.store_place(&b);
            }
        }
        Ok(())
    }

    fn loop_body(&mut self, body: &[Stmt]) -> Result<Loop> {
        self.loops.push(Loop {
            breaks: Vec::new(),
            continues: Vec::new(),
        });
        let result = self.block(body);
        let lp = self.loops.pop().expect("pushed above");
        result.map(|()| lp)
    }

    fn finish_loop(&mut self, lp: Loop, continue_to: Addr, break_to: Addr) {
        lp.continues
            .into_iter()
            .for_each(|j| self.patch(j, continue_to));
        lp.breaks.into_iter().for_each(|j| self.patch(j, break_to));
    }

    fn var_decl(&mut self, d: &VarDecl) -> Result<()> {
        let k = key(&d.var);
        // Using a variable before its DIM or VAR also makes the DIM a
        // duplicate (checked on hardware).
        let first = match &mut self.def {
            Some(def) => def.declared.insert(k.clone()),
            None => {
                (self.global_var_seen.insert(k.clone()) && !self.global_used.contains(&k))
                    || self.direct
            }
        };
        if !first {
            return Err(err(
                ErrorKind::DuplicateVariable,
                full_name(&d.var.name, d.var.suffix),
            ));
        }
        let slot = match &self.def {
            Some(_) => Slot::Local(self.local_slot(k)),
            None => Slot::Global(self.global_slot(k)),
        };
        if self.strict_at.is_some() {
            self.made(slot);
        }
        if d.dims.is_some() {
            let info = match slot {
                Slot::Global(n) => &mut self.out.globals[n as usize],
                Slot::Local(n) => {
                    let def = self.def.as_ref().expect("a local is in a DEF").index;
                    &mut self.out.defs[def as usize].locals[n as usize]
                }
            };
            info.array = true;
        }
        if let (true, None, Slot::Global(slot)) = (self.direct, &self.def, slot) {
            let array = d.dims.is_some();
            self.emit(Op::Declare { slot, array });
        }
        let elem = elem_type(d.var.suffix, self.var_info(slot).int);
        match (&d.dims, &d.init) {
            // With sizes, they decide the array's shape and the values fill
            // it from the start (as the reference's DIM POS$[10,5]=["X",...]).
            (Some(dims), Some(Init::Array(values))) if !dims.is_empty() => {
                if dims.len() > crate::value::MAX_DIMS {
                    return Err(err(ErrorKind::SyntaxError, "more than 4 dimensions"));
                }
                self.exprs(dims)?;
                self.emit(Op::NewArray {
                    elem,
                    dims: dims.len() as u8,
                });
                self.exprs(values)?;
                self.emit(Op::FillArray(values.len() as u32));
            }
            (Some(_), Some(Init::Array(values))) => {
                self.exprs(values)?;
                self.emit(Op::ArrayLit {
                    elem,
                    len: values.len() as u32,
                });
            }
            (Some(dims), None) => {
                if dims.is_empty() {
                    self.emit(Op::Int(0));
                } else if dims.len() > crate::value::MAX_DIMS {
                    return Err(err(ErrorKind::SyntaxError, "more than 4 dimensions"));
                }
                self.exprs(dims)?;
                self.emit(Op::NewArray {
                    elem,
                    dims: dims.len().max(1) as u8,
                });
            }
            (None, Some(Init::Expr(e))) => self.expr(e)?,
            // VAR without a value only declares; it does not reset.
            (None, None) => return Ok(()),
            (Some(_), Some(Init::Expr(_))) | (None, Some(Init::Array(_))) => {
                return Err(err(ErrorKind::SyntaxError, "array initializer"));
            }
        }
        self.emit(Op::Store(slot));
        Ok(())
    }

    fn print(&mut self, screen: Option<&Expr>, items: &[PrintItem]) -> Result<()> {
        let screen_slot = match screen {
            Some(e) => {
                let tmp = self.temp();
                self.expr(e)?;
                self.emit(Op::Store(tmp));
                Some(tmp)
            }
            None => None,
        };
        let has_screen = screen_slot.is_some();
        let load_screen = |c: &mut Self| {
            if let Some(s) = screen_slot {
                c.emit(Op::Load(s));
            }
        };
        // Every item is worked out before anything prints, the last
        // first (checked on hardware), so the first ends up on top.
        for item in items.iter().rev() {
            if let PrintItem::Expr(e) = item {
                self.expr(e)?;
            }
        }
        for item in items {
            match item {
                PrintItem::Expr(_) => {
                    if has_screen {
                        load_screen(self);
                        self.emit(Op::Swap);
                    }
                    self.emit(Op::Print { screen: has_screen });
                }
                PrintItem::Semicolon => {}
                PrintItem::Comma => {
                    load_screen(self);
                    self.emit(Op::PrintTab { screen: has_screen });
                }
            }
        }
        if !matches!(items.last(), Some(PrintItem::Semicolon | PrintItem::Comma)) {
            load_screen(self);
            self.emit(Op::PrintNewline { screen: has_screen });
        }
        Ok(())
    }

    fn command(
        &mut self,
        name: &Var,
        args: &[Option<Expr>],
        outs: &[Option<LValue>],
    ) -> Result<()> {
        let full = call_name(name);
        too_many(args.len().max(outs.len()))?;
        // A DIRECT mode line runs its commands only as its first word.
        // Anywhere else, after a `:`, THEN or ELSE, they are a Syntax
        // error, and nothing on the line runs.
        if self.direct
            && DIRECT_COMMANDS.contains(&full.as_str())
            && (self.col != 1 || self.first != Some((self.line, self.col)))
        {
            return Err(err(ErrorKind::SyntaxError, "only at the start of the line"));
        }
        let no_outs = |c: &Self| -> Result<()> {
            if outs.is_empty() {
                Ok(())
            } else {
                Err(c.illegal_call(&full))
            }
        };
        match full.as_str() {
            "INC" | "DEC" => {
                no_outs(self)?;
                let target = match args {
                    [Some(t)] | [Some(t), _] => expr_to_lvalue(t).ok_or(ErrorKind::SyntaxError)?,
                    _ => return Err(self.illegal_call(&full)),
                };
                // The amount first, then the place, once (checked on
                // hardware).
                match args.get(1) {
                    Some(Some(e)) => self.expr(e)?,
                    _ => _ = self.emit(Op::Int(1)),
                }
                let place = self.place(&target)?;
                self.load_place(&place);
                self.emit(Op::Swap);
                self.emit(match full == "INC" {
                    true => Op::Append,
                    false => Op::Binary(BinaryOp::Sub),
                });
                self.store_place(&place);
                return Ok(());
            }
            "CALLIDX" | "CHKLABEL" | "CHKCALL" | "CHKVAR" | "DEFARG" | "DEFARGC" | "DEFOUTC" => {
                return self.handler_command(&full, args, outs);
            }
            // The handlers check for no OUT as they run, and read the
            // first argument only, past any others.
            "VSYNC" | "WAIT" => {
                if !outs.is_empty() {
                    return self.fail_after(ErrorKind::WrongNumberOfOutArguments, &full, args);
                }
                self.args(args)?;
                for _ in 1..args.len() {
                    self.emit(Op::Pop);
                }
                self.emit(Op::Wait {
                    vsync: full == "VSYNC",
                    has_arg: !args.is_empty(),
                });
                return Ok(());
            }
            "EXEC" => {
                no_outs(self)?;
                let (1 | 2) = args.len() else {
                    return Err(err(ErrorKind::WrongNumberOfArguments, full));
                };
                if args.iter().any(Option::is_none) {
                    return Err(self.illegal_call(&full));
                }
                self.args(args)?;
                self.emit(Op::Exec(args.len() as u8));
                return Ok(());
            }
            "STOP" => {
                no_outs(self)?;
                let has_message = match args {
                    [] => false,
                    [Some(message)] => {
                        self.expr(message)?;
                        true
                    }
                    _ => return Err(self.illegal_call(&full)),
                };
                self.emit(Op::Stop { has_message });
                return Ok(());
            }
            // One OUT variable takes what FREEMEM() gives, two take that
            // and the largest free block. UNVERIFIED: the errors for
            // arguments and for more OUT variables.
            "FREEMEM" => {
                if !args.is_empty() {
                    return Err(err(ErrorKind::WrongNumberOfArguments, full));
                }
                let (1 | 2) = outs.len() else {
                    return Err(err(ErrorKind::WrongNumberOfOutArguments, full));
                };
                self.emit(Op::FreeMem {
                    both: outs.len() == 2,
                });
                return self.store_outs(outs);
            }
            "DEFOUT" => {
                no_outs(self)?;
                self.require_def()?;
                let [Some(n), Some(v)] = args else {
                    return Err(self.illegal_call(&full));
                };
                self.expr(n)?;
                self.expr(v)?;
                self.emit(Op::DefOut);
                return Ok(());
            }
            _ => {}
        }
        self.args(args)?;
        let argc = args.len() as u8;
        let want = Want::Outs(outs.len() as u8);
        if let Some(&def) = self.out.def_names.get(&full) {
            self.emit(Op::CallDef { def, argc, want });
        } else if let Some(id) = builtins::lookup(&full, Form::Command) {
            // The VM checks the argument count when the call runs, as
            // SmileBASIC 4 does (checked on hardware).
            self.emit(Op::CallBuiltin { id, argc, want });
        } else if let Some(id) = (outs.len() == 1)
            .then(|| builtins::lookup(&full, Form::Function))
            .flatten()
        {
            // A function with one OUT variable: `ABS -3 OUT A` is A=ABS(-3).
            let want = Want::Value;
            self.emit(Op::CallBuiltin { id, argc, want });
        } else if let Some(id) = self.host.lookup(&full) {
            self.emit(Op::CallHost { id, argc, want });
        } else if let Some(id) = builtins::lookup(&full, Form::Function) {
            // A function with no OUT variable or more than one compiles,
            // and the call finds the count wrong as it runs, as SmileBASIC
            // 4's handlers look at it themselves.
            self.emit(Op::CallBuiltin { id, argc, want });
        } else {
            self.call_common(&full, argc, want)?;
        }
        self.store_outs(outs)
    }

    fn illegal_call(&self, name: &str) -> Error {
        err(ErrorKind::IllegalFunctionCall, name.to_string())
    }

    fn require_def(&self) -> Result<()> {
        if self.def.is_none() {
            return Err(err(ErrorKind::CantUseOutsideDef, "only valid inside DEF"));
        }
        Ok(())
    }

    /// Works out `args` and fails with `kind` as the call runs, as the
    /// handler of builtin `name` does when it checks them.
    fn fail_after(&mut self, kind: ErrorKind, name: &str, args: &[Option<Expr>]) -> Result<()> {
        self.args(args)?;
        let name = self.string_const(name.encode_utf16().collect());
        self.emit(Op::Fail(kind, Some(name)));
        Ok(())
    }

    /// A call of DEFARG, DEFARGC, DEFOUTC, CHKLABEL, CHKCALL or CHKVAR
    /// with brackets. SmileBASIC 4 compiles these in place, not as calls
    /// of their handlers, and checks as the program compiles how many
    /// arguments they have, then that the DEF ones are inside a DEF.
    fn in_place(&mut self, name: &str, args: &[Option<Expr>]) -> Result<()> {
        check_in_place(name, args.len())?;
        if name.starts_with("DEF") && self.def.is_none() {
            return Err(err(ErrorKind::CantUseOutsideDef, name));
        }
        for arg in args {
            match arg {
                Some(e) => self.expr(e)?,
                None => _ = self.emit(Op::Default),
            }
        }
        self.emit(match name {
            "DEFARGC" => Op::DefArgc,
            "DEFOUTC" => Op::DefOutc,
            "DEFARG" => Op::DefArg,
            "CHKCALL" => Op::ChkCall,
            "CHKVAR" => Op::ChkVar,
            _ => Op::ChkLabel {
                has_flag: args.len() == 2,
            },
        });
        Ok(())
    }

    /// CALLIDX, CHKLABEL, CHKCALL, CHKVAR and the DEF functions as
    /// commands, `CHKVAR "A" OUT F`: these call their handlers, which
    /// check the counts as they run, each in its own order.
    fn handler_command(
        &mut self,
        name: &str,
        args: &[Option<Expr>],
        outs: &[Option<LValue>],
    ) -> Result<()> {
        use ErrorKind::{WrongNumberOfArguments, WrongNumberOfOutArguments};
        let argc_fits = match name {
            "CALLIDX" | "DEFARGC" | "DEFOUTC" => args.is_empty(),
            "CHKLABEL" => matches!(args.len(), 1 | 2),
            _ => args.len() == 1,
        };
        let outc_fits = outs.len() == 1;
        let fail = match name {
            "DEFARG" if self.def.is_none() => Some(ErrorKind::CantUseOutsideDef),
            "CHKLABEL" if !outc_fits => Some(WrongNumberOfOutArguments),
            _ if !argc_fits => Some(WrongNumberOfArguments),
            _ if !outc_fits => Some(WrongNumberOfOutArguments),
            _ => None,
        };
        if let Some(kind) = fail {
            return self.fail_after(kind, name, args);
        }
        match name {
            "CALLIDX" => _ = self.emit(Op::CallIdx),
            // Outside a DEF, the handlers give 0.
            "DEFARGC" | "DEFOUTC" if self.def.is_none() => _ = self.emit(Op::Int(0)),
            _ => self.in_place(name, args)?,
        }
        self.store_outs(outs)
    }

    fn store_outs(&mut self, outs: &[Option<LValue>]) -> Result<()> {
        // Results are on the stack in order, so the last one is stored first.
        for out in outs.iter().rev() {
            match out {
                Some(target) => self.store(target)?,
                None => _ = self.emit(Op::Pop),
            }
        }
        Ok(())
    }

    fn args(&mut self, args: &[Option<Expr>]) -> Result<()> {
        too_many(args.len())?;
        // The last argument first, as SmileBASIC does (checked on
        // hardware for builtins, DEFs and commands).
        for a in args.iter().rev() {
            match a {
                Some(e) => self.expr(e)?,
                None => _ = self.emit(Op::Default),
            }
        }
        self.reorder(args.len());
        Ok(())
    }

    /// Pushes values, the last one first, and puts them back in order.
    fn exprs(&mut self, exprs: &[Expr]) -> Result<()> {
        exprs.iter().rev().try_for_each(|e| self.expr(e))?;
        self.reorder(exprs.len());
        Ok(())
    }

    /// Puts the last `n` values, pushed last first, back in order.
    fn reorder(&mut self, n: usize) {
        match n {
            0 | 1 => {}
            2 => _ = self.emit(Op::Swap),
            n => _ = self.emit(Op::Reverse(n as u32)),
        }
    }

    // ---- lvalues ----

    fn store(&mut self, target: &LValue) -> Result<()> {
        match target {
            LValue::Var(v) => {
                let slot = self.slot(v)?;
                self.emit(Op::Store(slot));
            }
            LValue::Index { base, indices } => {
                self.load_lvalue(base)?;
                self.indices(indices)?;
                self.emit(Op::StoreIndex(indices.len() as u8));
            }
            LValue::Dynamic(name) => {
                self.expr(name)?;
                self.emit(Op::StoreDyn);
            }
        }
        Ok(())
    }

    /// Works out where `target` is once, into temporaries, for statements
    /// that read and write it.
    fn place(&mut self, target: &LValue) -> Result<Place> {
        let keep = |c: &mut Self| {
            let tmp = c.temp();
            c.emit(Op::Store(tmp));
            tmp
        };
        Ok(match target {
            LValue::Var(v) => Place::Var(self.slot(v)?),
            LValue::Index { base, indices } => {
                self.load_lvalue(base)?;
                let base = keep(self);
                if indices.len() > crate::value::MAX_DIMS {
                    return Err(ErrorKind::SubscriptOutOfRange.into());
                }
                let mut slots: Vec<Slot> = Vec::new();
                for i in indices.iter().rev() {
                    self.expr(i)?;
                    slots.push(keep(self));
                }
                slots.reverse();
                Place::Index(base, slots)
            }
            LValue::Dynamic(name) => {
                self.expr(name)?;
                Place::Dynamic(keep(self))
            }
        })
    }

    fn load_place(&mut self, place: &Place) {
        match place {
            Place::Var(slot) => _ = self.emit(Op::Load(*slot)),
            Place::Index(base, indices) => {
                self.emit(Op::Load(*base));
                indices.iter().for_each(|&i| _ = self.emit(Op::Load(i)));
                self.emit(Op::Index(indices.len() as u8));
            }
            Place::Dynamic(name) => {
                self.emit(Op::Load(*name));
                self.emit(Op::LoadDyn);
            }
        }
    }

    fn store_place(&mut self, place: &Place) {
        match place {
            Place::Var(slot) => _ = self.emit(Op::Store(*slot)),
            Place::Index(base, indices) => {
                self.emit(Op::Load(*base));
                indices.iter().for_each(|&i| _ = self.emit(Op::Load(i)));
                self.emit(Op::StoreIndex(indices.len() as u8));
            }
            Place::Dynamic(name) => {
                self.emit(Op::Load(*name));
                self.emit(Op::StoreDyn);
            }
        }
    }

    fn load_lvalue(&mut self, target: &LValue) -> Result<()> {
        match target {
            LValue::Var(v) => {
                let slot = self.slot(v)?;
                self.emit(Op::Load(slot));
            }
            LValue::Index { base, indices } => {
                self.load_lvalue(base)?;
                self.indices(indices)?;
                self.emit(Op::Index(indices.len() as u8));
            }
            LValue::Dynamic(name) => {
                self.expr(name)?;
                self.emit(Op::LoadDyn);
            }
        }
        Ok(())
    }

    fn indices(&mut self, indices: &[Expr]) -> Result<()> {
        if indices.len() > crate::value::MAX_DIMS {
            return Err(ErrorKind::SubscriptOutOfRange.into());
        }
        // The last index first (checked on hardware).
        indices.iter().rev().try_for_each(|i| self.expr(i))?;
        self.reorder(indices.len());
        Ok(())
    }

    // ---- expressions ----

    fn expr(&mut self, expr: &Expr) -> Result<()> {
        match expr {
            Expr::Int(n) => _ = self.emit(Op::Int(*n)),
            Expr::Real(x) => _ = self.emit(Op::Real(*x)),
            Expr::Str(s) => {
                let id = self.string_const(s.clone());
                self.emit(Op::Str(id));
            }
            Expr::Label(name) => {
                let id = self.string_const(format!("@{name}").encode_utf16().collect());
                self.emit(Op::Str(id));
            }
            Expr::Constant(name, at) => match self.constant(name, *at)? {
                Literal::Int(n) => _ = self.emit(Op::Int(n)),
                Literal::Real(x) => _ = self.emit(Op::Real(x)),
                Literal::Str(s) => {
                    let id = self.string_const(s);
                    self.emit(Op::Str(id));
                }
            },
            Expr::Var(v) => {
                let slot = self.slot(v)?;
                self.emit(Op::Load(slot));
            }
            Expr::Index { .. } | Expr::Unary { .. } | Expr::Binary { .. } => {
                let outer = self.constant_checked;
                let result = self.chain_expr(expr);
                self.constant_checked = outer;
                result?;
            }
            Expr::Call { name, args } => self.call_expr(name, args)?,
            Expr::DynamicCall { name, args } => {
                self.expr(name)?;
                self.args(args)?;
                self.emit(Op::CallDyn {
                    argc: args.len() as u8,
                    want: Want::Value,
                });
            }
            Expr::DynamicVar(name) => {
                self.expr(name)?;
                self.emit(Op::LoadDyn);
            }
        }
        Ok(())
    }

    /// Compiles an operator, or indexing, down its chain in a loop (see
    /// [`Expr::chain_operand`]): first the work before each link's chain
    /// operand, outermost link first, then the first operand, then the
    /// work after it, innermost link first. Leaves
    /// [`Compiler::constant_checked`] for the caller to put back.
    fn chain_expr(&mut self, expr: &Expr) -> Result<()> {
        let (links, first) = chain(expr, true);
        let mut checks = match self.constant_checked {
            true => Vec::new(),
            false => self.check_chain(&links, first),
        };
        // Where each link's right operand starts, and whether constants
        // were checked outside the link.
        let mut down = Vec::with_capacity(links.len());
        for (i, link) in links.iter().enumerate() {
            let outer = self.constant_checked;
            let mut at = 0;
            match link {
                Expr::Binary {
                    op: BinaryOp::LogicalAnd | BinaryOp::LogicalOr,
                    ..
                }
                | Expr::Index { .. } => {}
                Expr::Unary { .. } => {
                    if !outer {
                        self.constant_checked = std::mem::replace(&mut checks[i], Ok(false))?;
                    }
                }
                // The right operand first (checked on hardware:
                // F(1)+F(2)*F(3) calls F(3), F(2), then F(1)).
                Expr::Binary { rhs, .. } => {
                    if !outer {
                        self.constant_checked = std::mem::replace(&mut checks[i], Ok(false))?;
                    }
                    at = self.out.code.len();
                    self.expr(rhs)?;
                }
                _ => unreachable!("chain links are operators and indexing"),
            }
            down.push((at, outer));
        }
        self.expr(first)?;
        for (link, (at, outer)) in links.iter().zip(down).rev() {
            match link {
                Expr::Index { indices, .. } => {
                    self.constant_checked = outer;
                    self.indices(indices)?;
                    self.emit(Op::Index(indices.len() as u8));
                }
                Expr::Unary { op, .. } => _ = self.emit(Op::Unary(*op)),
                Expr::Binary {
                    op: op @ (BinaryOp::LogicalAnd | BinaryOp::LogicalOr),
                    rhs,
                    ..
                } => {
                    self.constant_checked = outer;
                    let (jump, otherwise) = match op {
                        BinaryOp::LogicalAnd => (Op::JumpIfFalse(0), 0),
                        _ => (Op::JumpIfTrue(0), 1),
                    };
                    let short = self.emit(jump);
                    self.expr(rhs)?;
                    self.emit(Op::Truthy);
                    let end = self.emit(Op::Jump(0));
                    let here = self.here();
                    self.patch(short, here);
                    self.emit(Op::Int(otherwise));
                    let here = self.here();
                    self.patch(end, here);
                }
                Expr::Binary { op, .. } => {
                    self.put_back_two(at);
                    self.emit(Op::Binary(*op));
                }
                _ => unreachable!("chain links are operators and indexing"),
            }
        }
        Ok(())
    }

    /// SmileBASIC 4 works out operations on constants when it compiles,
    /// so their errors stop the program before it runs (checked on
    /// hardware with `"A"*-1`).
    ///
    /// For each link of a chain from [`chain`], whether all of it is a
    /// constant that worked out (then every operation in it did too, and
    /// need not be checked again), or the error of its own operation on
    /// constant operands. `&&`, `||` and indexing are never checked.
    ///
    /// The chain is worked out once from its first operand up, where
    /// checking each link anew would take time growing with the square of
    /// the chain's length, both in a constant chain such as
    /// `"A"+#LF+"B"+...` and in one led by a variable.
    fn check_chain(&self, links: &[&Expr], first: &Expr) -> Vec<Result<bool>> {
        let mut checks: Vec<Result<bool>> = links.iter().map(|_| Ok(false)).collect();
        let mut value = self.const_value(first).ok();
        // How long the string worked out last is: the program holds it
        // when what comes next is not a constant too.
        let mut made = None;
        for (link, check) in links.iter().zip(&mut checks).rev() {
            let result = match (link, value) {
                (_, None) | (Expr::Index { .. }, _) => None,
                (Expr::Unary { op, .. }, Some(v)) => Some(ops::unary(*op, &v)),
                (Expr::Binary { op, rhs, .. }, Some(l)) => {
                    // A right side that is no constant is compiled on its
                    // own later, and takes what it takes then.
                    let before = self.constant_bytes.get();
                    let right = self.const_value(rhs);
                    if right.is_err() {
                        self.constant_bytes.set(before);
                    }
                    right.ok().map(|r| self.const_binary(*op, l, &r))
                }
                _ => unreachable!("chain links are operators and indexing"),
            };
            let logical = matches!(
                link,
                Expr::Binary {
                    op: BinaryOp::LogicalAnd | BinaryOp::LogicalOr,
                    ..
                }
            );
            value = match result {
                Some(Ok(v)) => {
                    *check = Ok(!logical);
                    made = match &v {
                        Value::Str(s) => Some(s.borrow().len()),
                        _ => None,
                    };
                    Some(v)
                }
                // Too large to work out for what is free now: it is the
                // program's to run into, if it ever gets there.
                Some(Err(e)) if e.kind == ErrorKind::MemoryCheck => None,
                Some(Err(e)) => {
                    if !logical {
                        *check = Err(e);
                    }
                    None
                }
                None => None,
            };
            if value.is_none()
                && let Some(chars) = made.take()
            {
                self.take_string_words(chars);
            }
        }
        if let Some(chars) = made {
            self.take_string_words(chars);
        }
        checks
    }

    /// Each constant of an expression has its place in [`CONSTANT_BYTES`],
    /// though only strings worked out are refused for it: a literal string's
    /// characters stay where they are in the source.
    fn take_constant_node(&self) {
        self.constant_bytes.set(
            self.constant_bytes
                .get()
                .saturating_add(CONSTANT_NODE_BYTES),
        );
    }

    /// A constant string in the code takes its characters and its length.
    fn take_string_words(&self, chars: usize) {
        let length = if chars >> 15 == 0 { 1 } else { 2 };
        self.string_words
            .set(self.string_words.get() + chars + length);
    }

    /// Whether what has been compiled so far fits in the code memory.
    // The rest of the code is taken as a word an instruction. Ours are not
    // SmileBASIC 4's, but it is constant strings that come near.
    fn check_code_memory(&self) -> Result<()> {
        let most = if self.direct {
            DIRECT_CODE_WORDS
        } else {
            SLOT_CODE_WORDS
        };
        if self.string_words.get() + self.out.code.len() > most {
            return Err(ErrorKind::OutOfCodeMemory.into());
        }
        Ok(())
    }

    /// [`ops::binary`] for working out constants, adding a string to the end
    /// of one no one else holds rather than copying it: a chain such as
    /// `"A"+"B"+...` then takes time in proportion to its length.
    ///
    /// Each string worked out stays until the statement has compiled, in
    /// [`CONSTANT_BYTES`], beside the constants it came from. The rest of
    /// the statement takes from there too, which is not counted: it is
    /// little, and this way nothing is refused that SmileBASIC 4 compiles.
    fn const_binary(&self, op: BinaryOp, lhs: Value, rhs: &Value) -> Result<Value> {
        let chars = match (op, &lhs, rhs) {
            (BinaryOp::Add, Value::Str(a), Value::Str(b)) => {
                Some(a.borrow().len() as i64 + b.borrow().len() as i64)
            }
            (BinaryOp::Mul, Value::Str(a), _) => {
                Some((a.borrow().len() as i64).saturating_mul(i64::from(rhs.to_int()?)))
            }
            _ => None,
        };
        if let Some(chars) = chars.filter(|&c| c >= 0) {
            let bytes = usize::try_from(chars)
                .unwrap_or(usize::MAX)
                .saturating_mul(2)
                .saturating_add(7 + CONSTANT_NODE_BYTES)
                & !7;
            let total = self.constant_bytes.get().saturating_add(bytes);
            if total > CONSTANT_BYTES {
                return Err(ErrorKind::OutOfMemory.into());
            }
            self.constant_bytes.set(total);
        }
        if let (BinaryOp::Add, Value::Str(a), Value::Str(b)) = (op, &lhs, rhs)
            && std::rc::Rc::strong_count(a) == 1
        {
            a.borrow_mut().extend_from_slice(&b.borrow());
            return Ok(lhs);
        }
        ops::binary(op, &lhs, rhs)
    }

    fn call_expr(&mut self, name: &Var, args: &[Option<Expr>]) -> Result<()> {
        let full = call_name(name);
        match (full.as_str(), args) {
            ("CALLIDX", []) => {
                self.emit(Op::CallIdx);
                return Ok(());
            }
            // Its handler checks the arguments as it runs.
            ("CALLIDX", _) => {
                return self.fail_after(ErrorKind::WrongNumberOfArguments, &full, args);
            }
            ("DEFARGC" | "DEFOUTC" | "DEFARG" | "CHKLABEL" | "CHKCALL" | "CHKVAR", _) => {
                return self.in_place(&full, args);
            }
            ("FREEMEM", []) => {
                self.emit(Op::FreeMem { both: false });
                return Ok(());
            }
            // They exist, so these fail as they run, not as unknown names.
            ("FREEMEM", _) => {
                self.args(args)?;
                let name = self.string_const(full.encode_utf16().collect());
                self.emit(Op::Fail(ErrorKind::WrongNumberOfArguments, Some(name)));
                return Ok(());
            }
            // `Wrong number of OUT arguments (VSYNC)` (TEST62).
            ("VSYNC" | "WAIT", _) => {
                return self.fail_after(ErrorKind::WrongNumberOfOutArguments, &full, args);
            }
            _ => {}
        }
        check_in_place(&full, args.len())?;
        let start = self.out.code.len();
        self.args(args)?;
        let argc = args.len() as u8;
        let want = Want::Value;
        if let Some(&def) = self.out.def_names.get(&full) {
            self.emit(Op::CallDef { def, argc, want });
        } else if let ("POP" | "SHIFT", [Some(arg)]) = (full.as_str(), args)
            && self.take_from_constant(start, arg, full == "SHIFT")
        {
        } else if let Some(id) = builtins::lookup(&full, Form::Function) {
            let constants = args
                .iter()
                .map(|a| a.as_ref().and_then(|a| self.const_value(a).ok()))
                .collect::<Option<Vec<_>>>();
            if let Some(constants) = constants {
                builtins::check_constant_args(&full, &constants)?;
                // SmileBASIC 4 works out a number from constants as it
                // compiles: TEST74's A=RGB(1,2,3) costs less than
                // A=FLOOR(R#). A call that fails still fails as it runs.
                let b = builtins::get(id);
                let folded = match b.kind {
                    Kind::Function(f) if builtins::is_constant_function(&full) => {
                        b.accepts(constants.len()).then(|| f(&constants).ok())
                    }
                    _ => None,
                };
                let op = match folded.flatten() {
                    Some(Value::Int(n)) => Some(Op::Int(n)),
                    Some(Value::Real(x)) => Some(Op::Real(x)),
                    _ => None,
                };
                if let Some(op) = op {
                    self.out.code.truncate(start);
                    self.out.lines.truncate(start);
                    self.out.cols.truncate(start);
                    self.out.starts.truncate(start);
                    self.emit(op);
                    return Ok(());
                }
            }
            // The VM checks the argument count when the call runs, as
            // SmileBASIC 4 does (checked on hardware).
            self.emit(Op::CallBuiltin { id, argc, want });
        } else if let Some(id) = builtins::lookup(&full, Form::Command) {
            // `Q=PUSH(A,1)` calls the command with one OUT variable, and
            // its handler finds the counts wrong as it runs.
            self.emit(Op::CallBuiltin { id, argc, want });
        } else if let Some(id) = self.host.lookup(&full) {
            self.emit(Op::CallHost { id, argc, want });
        } else {
            self.call_common(&full, argc, want)?;
        }
        Ok(())
    }

    /// A call to a name defined nowhere in this program: a COMMON DEF of
    /// another slot, found when it runs (it may be EXEC'd only then). A
    /// name found nowhere is a Syntax error only when the call is reached
    /// (checked on hardware).
    fn call_common(&mut self, name: &str, argc: u8, want: Want) -> Result<()> {
        // A DIRECT mode line leaves no name for later: SmileBASIC 4 gives
        // the Syntax error as the call compiles, after its arguments.
        if self.direct {
            return Err(Error::with_detail(ErrorKind::SyntaxError, name));
        }
        let name = self.string_const(name.encode_utf16().collect());
        self.emit(Op::CallCommon { name, argc, want });
        Ok(())
    }

    // ---- DEF ----

    /// Compiles `def`, which stands at `at`.
    fn def(&mut self, def: &Def, at: (u32, u32)) -> Result<()> {
        let index = self.out.def_names[&call_name(&def.name)];
        self.out.defs[index as usize].entry = self.here();
        self.def = Some(DefScope {
            index,
            is_function: def.is_function,
            locals: HashMap::new(),
            declared: HashSet::new(),
            ahead: Vec::new(),
        });
        let params = match &def.params {
            Params::Fixed(p) => p.as_slice(),
            Params::Variadic => &[],
        };
        let outs = match &def.outs {
            Params::Fixed(o) => o.as_slice(),
            Params::Variadic => &[],
        };
        if self.defint_at.is_some() {
            self.first_seen.clear();
            for v in params.iter().chain(outs) {
                self.first_seen.entry(key(v)).or_insert(at);
            }
            first_seen(&def.body, &mut self.first_seen);
        }
        for v in params.iter().chain(outs) {
            let k = key(v);
            if self
                .def
                .as_ref()
                .expect("set above")
                .locals
                .contains_key(&k)
            {
                return Err(
                    err(ErrorKind::DuplicateVariable, full_name(&v.name, v.suffix))
                        .at_pos(self.line, self.col),
                );
            }
            self.local_slot(k.clone());
            self.def.as_mut().expect("set above").declared.insert(k);
        }
        // Names declared with VAR anywhere in the body are local.
        let mut used = Vec::new();
        let mut declared = Vec::new();
        collect_vars(&def.body, &mut used, &mut declared);
        let strict = self.strict_at.is_some();
        for k in declared {
            let made = self.out.defs[index as usize].locals.len();
            let slot = self.local_slot(k) as usize;
            let ahead = &mut self.def.as_mut().expect("set above").ahead;
            if strict && slot >= made {
                ahead.resize(slot, false);
                ahead.push(true);
            }
        }
        self.block(&def.body)?;
        if def.is_function {
            // A function that ends without RETURN gives no value.
            self.emit(Op::Default);
            self.emit(Op::ReturnValue);
        } else {
            self.emit(Op::Return);
        }
        self.resolve_patches(Some(index))?;
        self.out.defs[index as usize].end = self.here();
        self.def = None;
        Ok(())
    }
}

/// Splits `expr` into the links of its operator chain, outermost first,
/// and the operand the chain starts from (see [`Expr::chain_operand`]).
/// Indexing is a link only `with_index`.
fn chain(expr: &Expr, with_index: bool) -> (Vec<&Expr>, &Expr) {
    let mut links = Vec::new();
    let mut first = expr;
    while let Some(operand) = first.chain_operand() {
        if !with_index && matches!(first, Expr::Index { .. }) {
            break;
        }
        links.push(first);
        first = operand;
    }
    (links, first)
}

/// The commands of DIRECT mode that compile as commands; LIST is read
/// before the line compiles.
const DIRECT_COMMANDS: &[&str] = &[
    "RUN",
    "CONT",
    "NEW",
    "CLEAR",
    "TRACE",
    "BACKTRACE",
    "SUBRUN",
    "SUBSTOP",
    "SUBSHOW",
    "SUBHIDE",
];

/// What one statement's constant strings can take as it compiles, and
/// what each takes beside its characters.
const CONSTANT_BYTES: usize = 4 << 20;
const CONSTANT_NODE_BYTES: usize = 24;
/// The code memory, of 16-bit words, of a program slot and of direct mode.
const SLOT_CODE_WORDS: usize = 4 << 20;
const DIRECT_CODE_WORDS: usize = 32768;

fn literal_value(l: &Literal) -> Value {
    match l {
        Literal::Int(n) => Value::Int(*n),
        Literal::Real(x) => Value::Real(*x),
        Literal::Str(s) => Value::Str(new_str(s.clone())),
    }
}

/// Statement lists nested directly inside a statement.
fn child_blocks(kind: &StmtKind) -> Vec<&[Stmt]> {
    match kind {
        StmtKind::If {
            branches,
            otherwise,
        } => {
            let mut v: Vec<&[Stmt]> = branches.iter().map(|(_, b)| b.as_slice()).collect();
            v.extend(otherwise.as_deref());
            v
        }
        StmtKind::Case {
            arms, otherwise, ..
        } => {
            let mut v: Vec<&[Stmt]> = arms.iter().map(|a| a.body.as_slice()).collect();
            v.extend(otherwise.as_deref());
            v
        }
        StmtKind::For { body, .. }
        | StmtKind::While { body, .. }
        | StmtKind::Repeat { body, .. }
        | StmtKind::Loop { body } => vec![body.as_slice()],
        StmtKind::Def(def) => vec![def.body.as_slice()],
        _ => Vec::new(),
    }
}

/// Collects variable names used and VAR-declared in `stmts`, not descending
/// into DEFs.
fn collect_vars(stmts: &[Stmt], used: &mut Vec<Key>, declared: &mut Vec<Key>) {
    for stmt in stmts {
        stmt_vars(stmt, used, declared);
        if !matches!(stmt.kind, StmtKind::Def(_)) {
            for body in child_blocks(&stmt.kind) {
                collect_vars(body, used, declared);
            }
        }
    }
}

/// Where each variable in `stmts` is first seen: the statement it is
/// in, innermost, not descending into DEFs.
fn first_seen(stmts: &[Stmt], seen: &mut HashMap<Key, (u32, u32)>) {
    let (mut used, mut declared) = (Vec::new(), Vec::new());
    for stmt in stmts {
        stmt_vars(stmt, &mut used, &mut declared);
        for k in used.drain(..).chain(declared.drain(..)) {
            seen.entry(k).or_insert((stmt.line, stmt.col));
        }
        if !matches!(stmt.kind, StmtKind::Def(_)) {
            for body in child_blocks(&stmt.kind) {
                first_seen(body, seen);
            }
        }
    }
}

/// [`collect_vars`] of `stmt` without the blocks in it.
fn stmt_vars(stmt: &Stmt, used: &mut Vec<Key>, declared: &mut Vec<Key>) {
    /// In the order they appear, following a chain in a loop.
    fn expr(e: &Expr, used: &mut Vec<Key>) {
        let (links, first) = chain(e, true);
        match first {
            Expr::Var(v) => used.push(key(v)),
            Expr::Call { args, .. } => args.iter().flatten().for_each(|a| expr(a, used)),
            Expr::DynamicCall { name, args } => {
                expr(name, used);
                args.iter().flatten().for_each(|a| expr(a, used));
            }
            Expr::DynamicVar(n) => expr(n, used),
            Expr::Index { .. } | Expr::Unary { .. } | Expr::Binary { .. } => {
                unreachable!("chain(_, true) follows operators and indexing")
            }
            Expr::Int(_) | Expr::Real(_) | Expr::Str(_) | Expr::Label(_) | Expr::Constant(..) => {}
        }
        for link in links.iter().rev() {
            match link {
                Expr::Index { indices, .. } => indices.iter().for_each(|i| expr(i, used)),
                Expr::Binary { rhs, .. } => expr(rhs, used),
                _ => {}
            }
        }
    }
    fn lvalue(l: &LValue, used: &mut Vec<Key>) {
        match l {
            LValue::Var(v) => used.push(key(v)),
            LValue::Index { base, indices } => {
                lvalue(base, used);
                indices.iter().for_each(|i| expr(i, used));
            }
            LValue::Dynamic(n) => expr(n, used),
        }
    }
    {
        match &stmt.kind {
            StmtKind::Assign { target, value, .. } => {
                lvalue(target, used);
                expr(value, used);
            }
            StmtKind::Var(decls) => {
                for d in decls {
                    declared.push(key(&d.var));
                    d.dims.iter().flatten().for_each(|e| expr(e, used));
                    match &d.init {
                        Some(Init::Expr(e)) => expr(e, used),
                        Some(Init::Array(values)) => values.iter().for_each(|e| expr(e, used)),
                        None => {}
                    }
                }
            }
            StmtKind::Command { args, outs, .. } => {
                args.iter().flatten().for_each(|a| expr(a, used));
                outs.iter().flatten().for_each(|o| lvalue(o, used));
            }
            StmtKind::Call { name, args, outs } => {
                expr(name, used);
                args.iter().flatten().for_each(|a| expr(a, used));
                outs.iter().flatten().for_each(|o| lvalue(o, used));
            }
            StmtKind::Print { screen, items } => {
                screen.iter().for_each(|s| expr(s, used));
                for item in items {
                    if let PrintItem::Expr(e) = item {
                        expr(e, used);
                    }
                }
            }
            StmtKind::Input {
                prompt, targets, ..
            } => {
                prompt.iter().for_each(|p| expr(p, used));
                targets.iter().for_each(|t| lvalue(t, used));
            }
            StmtKind::If { branches, .. } => branches.iter().for_each(|(c, _)| expr(c, used)),
            StmtKind::Case { subject, arms, .. } => {
                expr(subject, used);
                arms.iter()
                    .flat_map(|a| &a.values)
                    .for_each(|v| expr(v, used));
            }
            StmtKind::For {
                var,
                from,
                to,
                step,
                ..
            } => {
                lvalue(var, used);
                expr(from, used);
                expr(to, used);
                step.iter().for_each(|s| expr(s, used));
            }
            StmtKind::While { cond, .. } | StmtKind::Repeat { cond, .. } => expr(cond, used),
            StmtKind::Goto(Target::Expr(e))
            | StmtKind::Gosub(Target::Expr(e))
            | StmtKind::Restore(Target::Expr(e)) => expr(e, used),
            StmtKind::On { selector, .. } => expr(selector, used),
            StmtKind::Return(Some(e)) => expr(e, used),
            StmtKind::Read(targets) => targets.iter().for_each(|t| lvalue(t, used)),
            StmtKind::Swap(a, b) => {
                lvalue(a, used);
                lvalue(b, used);
            }
            _ => {}
        }
    }
}

/// Whether an INPUT target takes a string or a number, from its suffix.
fn input_kind(target: &LValue) -> InputKind {
    match target {
        LValue::Var(v) if v.suffix == Some(Suffix::Str) => InputKind::Str,
        LValue::Var(_) => InputKind::Number,
        LValue::Index { base, .. } => input_kind(base),
        LValue::Dynamic(name) => match &**name {
            Expr::Str(s) if s.last() == Some(&u16::from(b'$')) => InputKind::Str,
            _ => InputKind::Number,
        },
    }
}
