//! AST to bytecode.

use std::collections::{HashMap, HashSet};

use crate::ast::*;
use crate::builtins::{self, Form, Kind};
use crate::bytecode::*;
use crate::constants::{self, ConstValue};
use crate::error::{Error, ErrorKind, Result};
use crate::host::Host;
use crate::lexer::Suffix;
use crate::ops;
use crate::value::{ElemType, Value, new_str};

type Key = (String, Option<Suffix>);

fn key(var: &Var) -> Key {
    (var.name.clone(), var.suffix)
}

/// Compiles `program`; `direct` for a line typed in direct mode, where
/// global declarations are checked as they run rather than here.
pub fn compile(program: &Program, host: &dyn Host, direct: bool) -> Result<Compiled> {
    let mut c = Compiler {
        host,
        direct,
        out: Compiled::default(),
        global_slots: HashMap::new(),
        global_declared: HashSet::new(),
        global_var_seen: HashSet::new(),
        global_used: HashSet::new(),
        strict: false,
        constants: HashMap::new(),
        def: None,
        loops: Vec::new(),
        patches: Vec::new(),
        line: 0,
        col: 0,
        temp_count: 0,
    };
    c.prepare(program)?;
    for stmt in &program.stmts {
        if !matches!(stmt.kind, StmtKind::Def(_)) {
            c.stmt(stmt)?;
        }
    }
    c.emit(Op::End);
    c.resolve_patches(None)?;
    for stmt in &program.stmts {
        if let StmtKind::Def(def) = &stmt.kind {
            c.line = stmt.line;
            c.def(def)?;
        }
    }
    Ok(c.out)
}

/// Compilation state for the DEF being compiled.
struct DefScope {
    index: u32,
    is_function: bool,
    locals: HashMap<Key, u32>,
    declared: HashSet<Key>,
}

struct Loop {
    breaks: Vec<usize>,
    continues: Vec<usize>,
}

struct Compiler<'h> {
    host: &'h dyn Host,
    out: Compiled,
    global_slots: HashMap<Key, u32>,
    /// Names declared by VAR at top level (used by OPTION STRICT).
    global_declared: HashSet<Key>,
    /// VAR statements compiled so far at top level, to report duplicates.
    global_var_seen: HashSet<Key>,
    /// Globals that top-level code has used so far, in source order.
    global_used: HashSet<Key>,
    direct: bool,
    strict: bool,
    constants: HashMap<String, Literal>,
    def: Option<DefScope>,
    loops: Vec<Loop>,
    /// Jumps to labels, resolved when their scope is finished.
    patches: Vec<(usize, String, u32)>,
    line: u32,
    col: u32,
    temp_count: u32,
}

fn err(kind: ErrorKind, detail: impl Into<String>) -> Error {
    Error::with_detail(kind, detail)
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

    fn prepare(&mut self, program: &Program) -> Result<()> {
        for stmt in &program.stmts {
            (self.line, self.col) = (stmt.line, stmt.col);
            match &stmt.kind {
                StmtKind::Option(o) if o == "STRICT" => self.strict = true,
                StmtKind::Option(o) if o == "DEFINT" => self.out.default_int = true,
                StmtKind::Option(o) => {
                    return Err(err(ErrorKind::SyntaxError, format!("OPTION {o}"))
                        .at_pos(stmt.line, stmt.col));
                }
                StmtKind::Def(def) => {
                    let name = call_name(&def.name);
                    let index = self.out.defs.len() as u32;
                    if self.out.def_names.insert(name.clone(), index).is_some() {
                        return Err(
                            err(ErrorKind::DuplicateFunction, name).at_pos(stmt.line, stmt.col)
                        );
                    }
                    self.out.defs.push(DefInfo {
                        name,
                        entry: 0,
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
                _ => {}
            }
        }
        let mut data_labels = HashMap::new();
        self.walk_source_order(&program.stmts, &mut data_labels)?;
        self.out.data_labels = data_labels;

        // Every variable named in top-level code is global, and DEFs see it.
        let mut used = Vec::new();
        let mut declared = Vec::new();
        for stmt in &program.stmts {
            if !matches!(stmt.kind, StmtKind::Def(_)) {
                collect_vars(std::slice::from_ref(stmt), &mut used, &mut declared);
            }
        }
        for k in used.into_iter().chain(declared.iter().cloned()) {
            self.global_slot(k);
        }
        self.global_declared.extend(declared);
        Ok(())
    }

    /// CONST/ENUM definitions and DATA, in source order across DEFs.
    fn walk_source_order(
        &mut self,
        stmts: &[Stmt],
        data_labels: &mut HashMap<String, u32>,
    ) -> Result<()> {
        for stmt in stmts {
            (self.line, self.col) = (stmt.line, stmt.col);
            let at = |e: Error| e.at_pos(stmt.line, stmt.col);
            match &stmt.kind {
                StmtKind::Label(name) => {
                    data_labels
                        .entry(name.clone())
                        .or_insert(self.out.data.len() as u32);
                }
                StmtKind::Const(items) => {
                    for (name, expr) in items {
                        let value = self.const_eval(expr).map_err(at)?;
                        self.define_constant(name, value).map_err(at)?;
                    }
                }
                StmtKind::Enum(items) => {
                    let mut next = 0i32;
                    for (name, expr) in items {
                        let n = match expr {
                            None => next,
                            Some(e) => match self.const_eval(e).map_err(at)? {
                                Literal::Int(n) => n,
                                // The reference: a real value is truncated.
                                Literal::Real(x) => crate::value::real_to_int(x).map_err(at)?,
                                Literal::Str(_) => {
                                    return Err(at(ErrorKind::TypeMismatch.into()));
                                }
                            },
                        };
                        self.define_constant(name, Literal::Int(n)).map_err(at)?;
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
            for body in child_blocks(&stmt.kind) {
                self.walk_source_order(body, data_labels)?;
            }
        }
        Ok(())
    }

    fn define_constant(&mut self, name: &str, value: Literal) -> Result<()> {
        if self.constants.insert(name.to_string(), value).is_some() {
            return Err(err(ErrorKind::DuplicateVariable, format!("#{name}")));
        }
        Ok(())
    }

    fn constant(&self, name: &str) -> Result<Literal> {
        if let Some(v) = self.constants.get(name) {
            return Ok(v.clone());
        }
        match name {
            "_LINE" => return Ok(Literal::Int(self.line as i32)),
            // Only slot 0 exists so far, and programs are not loaded from files yet.
            "_SLOT" => return Ok(Literal::Int(0)),
            "_FILENAME" => return Ok(Literal::Str(Vec::new())),
            _ => {}
        }
        match constants::lookup(name) {
            Some(ConstValue::Int(n)) => Ok(Literal::Int(n)),
            Some(ConstValue::Real(x)) => Ok(Literal::Real(x)),
            None => Err(err(
                ErrorKind::SyntaxError,
                format!("undefined constant #{name}"),
            )),
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

    fn const_value(&self, expr: &Expr) -> Result<Value> {
        Ok(match expr {
            Expr::Int(n) => Value::Int(*n),
            Expr::Real(x) => Value::Real(*x),
            Expr::Str(s) => Value::Str(new_str(s.clone())),
            Expr::Label(name) => Value::Str(new_str(format!("@{name}").encode_utf16().collect())),
            Expr::Constant(name) => literal_value(&self.constant(name)?),
            Expr::Unary { op, expr } => ops::unary(*op, &self.const_value(expr)?)?,
            Expr::Binary { op, lhs, rhs } => {
                ops::binary(*op, &self.const_value(lhs)?, &self.const_value(rhs)?)?
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
        self.out.code.len() - 1
    }

    fn here(&self) -> Addr {
        self.out.code.len() as Addr
    }

    fn patch(&mut self, at: usize, target: Addr) {
        match &mut self.out.code[at] {
            Op::Jump(a)
            | Op::JumpIfFalse(a)
            | Op::JumpIfTrue(a)
            | Op::Gosub(a)
            | Op::OnBreak(a)
            | Op::CallbackNext(a) => *a = target,
            other => unreachable!("patching non-jump {other:?}"),
        }
    }

    fn string_const(&mut self, s: Vec<u16>) -> u32 {
        self.out.strings.push(s);
        (self.out.strings.len() - 1) as u32
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
            let target = *labels.get(&label).ok_or_else(|| {
                err(ErrorKind::UndefinedLabel, format!("@{label}"))
                    .at_pos(self.out.lines[at], self.out.cols[at])
            })?;
            self.patch(at, target);
        }
        Ok(())
    }

    // ---- variables ----

    fn global_slot(&mut self, k: Key) -> u32 {
        if let Some(&slot) = self.global_slots.get(&k) {
            return slot;
        }
        let slot = self.out.globals.len() as u32;
        self.out.globals.push(VarInfo {
            name: k.0.clone(),
            suffix: k.1,
        });
        self.global_slots.insert(k, slot);
        slot
    }

    fn local_slot(&mut self, k: Key) -> u32 {
        let def = self.def.as_mut().expect("inside a DEF");
        if let Some(&slot) = def.locals.get(&k) {
            return slot;
        }
        let info = &mut self.out.defs[def.index as usize];
        let slot = info.locals.len() as u32;
        info.locals.push(VarInfo {
            name: k.0.clone(),
            suffix: k.1,
        });
        def.locals.insert(k, slot);
        slot
    }

    fn slot(&mut self, var: &Var) -> Result<Slot> {
        let k = key(var);
        if let Some(def) = &self.def {
            if let Some(&slot) = def.locals.get(&k) {
                return Ok(Slot::Local(slot));
            }
            if let Some(&slot) = self.global_slots.get(&k) {
                if !self.strict || self.global_declared.contains(&k) {
                    return Ok(Slot::Global(slot));
                }
            }
        } else if let Some(&slot) = self.global_slots.get(&k) {
            self.global_used.insert(k.clone());
            if self.strict && !self.global_declared.contains(&k) {
                return Err(err(
                    ErrorKind::UndefinedVariable,
                    full_name(&var.name, var.suffix),
                ));
            }
            return Ok(Slot::Global(slot));
        }
        if self.strict {
            return Err(err(
                ErrorKind::UndefinedVariable,
                full_name(&var.name, var.suffix),
            ));
        }
        Ok(match self.def {
            Some(_) => Slot::Local(self.local_slot(k)),
            None => Slot::Global(self.global_slot(k)),
        })
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

    fn block(&mut self, stmts: &[Stmt]) -> Result<()> {
        stmts.iter().try_for_each(|s| self.stmt(s))
    }

    fn stmt(&mut self, stmt: &Stmt) -> Result<()> {
        (self.line, self.col) = (stmt.line, stmt.col);
        self.stmt_kind(&stmt.kind)
            .map_err(|e| e.at_pos(stmt.line, stmt.col))
    }

    fn stmt_kind(&mut self, kind: &StmtKind) -> Result<()> {
        match kind {
            StmtKind::Label(name) => {
                let here = self.here();
                if self.labels_mut().insert(name.clone(), here).is_some() {
                    return Err(err(ErrorKind::DuplicateLabel, format!("@{name}")));
                }
            }
            StmtKind::Assign { target, value } => {
                self.expr(value)?;
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
                self.expr(name)?;
                self.args(args)?;
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
                match prompt {
                    Some(p) => self.expr(p)?,
                    None => _ = self.emit(Op::Default),
                }
                let kinds = targets.iter().map(input_kind).collect();
                self.emit(Op::Input {
                    kinds,
                    question: *question,
                    whole_line: *whole_line,
                });
                for target in targets.iter().rev() {
                    self.store(target)?;
                }
            }
            StmtKind::If {
                branches,
                otherwise,
            } => {
                let mut ends = Vec::new();
                for (cond, body) in branches {
                    self.expr(cond)?;
                    let skip = self.emit(Op::JumpIfFalse(0));
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
                // STEP, then TO, are worked out again before each test
                // (checked on hardware).
                let check = self.here();
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
                let next = self.here();
                self.load_lvalue(var)?;
                self.emit(Op::Load(step_slot));
                self.emit(Op::Binary(BinaryOp::Add));
                self.store(var)?;
                self.emit(Op::Jump(check));
                let end = self.here();
                self.patch(exit, end);
                self.finish_loop(lp, next, end);
            }
            StmtKind::While { cond, body } => {
                let check = self.here();
                self.expr(cond)?;
                let exit = self.emit(Op::JumpIfFalse(0));
                let lp = self.loop_body(body)?;
                self.emit(Op::Jump(check));
                let end = self.here();
                self.patch(exit, end);
                self.finish_loop(lp, check, end);
            }
            StmtKind::Repeat { body, cond } => {
                let top = self.here();
                let lp = self.loop_body(body)?;
                let check = self.here();
                self.expr(cond)?;
                self.emit(Op::JumpIfFalse(top));
                let end = self.here();
                self.finish_loop(lp, check, end);
            }
            StmtKind::Loop { body } => {
                let top = self.here();
                let lp = self.loop_body(body)?;
                self.emit(Op::Jump(top));
                let end = self.here();
                self.finish_loop(lp, top, end);
            }
            StmtKind::Break | StmtKind::Continue => {
                let j = self.emit(Op::Jump(0));
                let lp = self.loops.last_mut().ok_or(ErrorKind::IllegalStatement)?;
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
            StmtKind::OnBreakGoto(l) => self.jump_to_label(Op::OnBreak(0), l),
            StmtKind::Return(None) => _ = self.emit(Op::Return),
            StmtKind::Return(Some(e)) => {
                self.expr(e)?;
                if self.def.as_ref().is_some_and(|d| d.is_function) {
                    self.emit(Op::ReturnValue);
                } else {
                    // A bundled program returns a value from a command DEF.
                    // UNVERIFIED: the value is discarded.
                    self.emit(Op::Pop);
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
            StmtKind::Restore(Target::Label(l)) => {
                let pos = *self
                    .out
                    .data_labels
                    .get(l)
                    .ok_or_else(|| err(ErrorKind::UndefinedLabel, format!("@{l}")))?;
                self.emit(Op::Restore(pos));
            }
            StmtKind::Restore(Target::Expr(e)) => {
                self.expr(e)?;
                self.emit(Op::RestoreDyn);
            }
            StmtKind::Swap(a, b) => {
                self.load_lvalue(a)?;
                self.load_lvalue(b)?;
                self.store(a)?;
                self.store(b)?;
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
        if let (true, None, Slot::Global(slot)) = (self.direct, &self.def, slot) {
            let array = d.dims.is_some();
            self.emit(Op::Declare { slot, array });
        }
        let elem = elem_type(d.var.suffix, self.out.default_int);
        match (&d.dims, &d.init) {
            // With sizes, they decide the array's shape and the values fill
            // it from the start (as the reference's DIM POS$[10,5]=["X",...]).
            (Some(dims), Some(Init::Array(values))) if !dims.is_empty() => {
                if dims.len() > crate::value::MAX_DIMS {
                    return Err(err(ErrorKind::SyntaxError, "more than 4 dimensions"));
                }
                for e in dims {
                    self.expr(e)?;
                }
                self.emit(Op::NewArray {
                    elem,
                    dims: dims.len() as u8,
                });
                for v in values {
                    self.expr(v)?;
                }
                self.emit(Op::FillArray(values.len() as u32));
            }
            (Some(_), Some(Init::Array(values))) => {
                for v in values {
                    self.expr(v)?;
                }
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
                for e in dims {
                    self.expr(e)?;
                }
                self.emit(Op::NewArray {
                    elem,
                    dims: dims.len().max(1) as u8,
                });
            }
            (None, Some(Init::Expr(e))) => self.expr(e)?,
            // UNVERIFIED: VAR without a value only declares; it does not reset.
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
                self.load_lvalue(&target)?;
                match args.get(1) {
                    Some(Some(e)) => self.expr(e)?,
                    _ => _ = self.emit(Op::Int(1)),
                }
                let op = if full == "INC" {
                    BinaryOp::Add
                } else {
                    BinaryOp::Sub
                };
                self.emit(Op::Binary(op));
                return self.store(&target);
            }
            "VSYNC" | "WAIT" => {
                no_outs(self)?;
                let has_arg = match args {
                    [] => false,
                    [Some(e)] => {
                        self.expr(e)?;
                        true
                    }
                    _ => return Err(self.illegal_call(&full)),
                };
                self.emit(Op::Wait {
                    vsync: full == "VSYNC",
                    has_arg,
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
            let b = builtins::get(id);
            // Checked on hardware: ?ABS(1,2) is Wrong number of arguments.
            if !b.accepts(args.len()) {
                return Err(err(ErrorKind::WrongNumberOfArguments, full.clone()));
            }
            self.emit(Op::CallBuiltin { id, argc, want });
        } else if let Some(id) = self.host.lookup(&full) {
            self.emit(Op::CallHost { id, argc, want });
        } else {
            self.call_common(&full, argc, want)?;
        }
        self.store_outs(outs)
    }

    /// A name that resolves to nothing in the form it is called in: a builtin
    /// used in its other form is an illegal call, anything else is undefined.
    fn unknown_call(&self, name: &str) -> Error {
        let exists = builtins::lookup(name, Form::Function).is_some()
            || builtins::lookup(name, Form::Command).is_some();
        if exists {
            self.illegal_call(name)
        } else {
            err(ErrorKind::UndefinedFunction, name.to_string())
        }
    }

    fn illegal_call(&self, name: &str) -> Error {
        err(ErrorKind::IllegalFunctionCall, name.to_string())
    }

    fn require_def(&self) -> Result<()> {
        if self.def.is_none() {
            return Err(err(ErrorKind::IllegalStatement, "only valid inside DEF"));
        }
        Ok(())
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
        if args.len() > u8::MAX as usize {
            return Err(err(ErrorKind::SyntaxError, "too many arguments"));
        }
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

    /// Puts the last `n` values, pushed last first, back in order.
    fn reorder(&mut self, n: usize) {
        match n {
            0 | 1 => {}
            2 => _ = self.emit(Op::Swap),
            n => _ = self.emit(Op::Reverse(n as u8)),
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
            Expr::Constant(name) => match self.constant(name)? {
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
            Expr::Index { base, indices } => {
                self.expr(base)?;
                self.indices(indices)?;
                self.emit(Op::Index(indices.len() as u8));
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
            Expr::Unary { op, expr: operand } => {
                self.check_constant(expr)?;
                self.expr(operand)?;
                self.emit(Op::Unary(*op));
            }
            Expr::Binary {
                op: BinaryOp::LogicalAnd,
                lhs,
                rhs,
            } => {
                self.expr(lhs)?;
                let short = self.emit(Op::JumpIfFalse(0));
                self.expr(rhs)?;
                self.emit(Op::Truthy);
                let end = self.emit(Op::Jump(0));
                let here = self.here();
                self.patch(short, here);
                self.emit(Op::Int(0));
                let here = self.here();
                self.patch(end, here);
            }
            Expr::Binary {
                op: BinaryOp::LogicalOr,
                lhs,
                rhs,
            } => {
                self.expr(lhs)?;
                let short = self.emit(Op::JumpIfTrue(0));
                self.expr(rhs)?;
                self.emit(Op::Truthy);
                let end = self.emit(Op::Jump(0));
                let here = self.here();
                self.patch(short, here);
                self.emit(Op::Int(1));
                let here = self.here();
                self.patch(end, here);
            }
            // The right operand first (checked on hardware: F(1)+F(2)*F(3)
            // calls F(3), F(2), then F(1)).
            Expr::Binary { op, lhs, rhs } => {
                self.check_constant(expr)?;
                self.expr(rhs)?;
                self.expr(lhs)?;
                self.emit(Op::Swap);
                self.emit(Op::Binary(*op));
            }
        }
        Ok(())
    }

    /// SmileBASIC 4 works out operations on constants when it compiles,
    /// so their errors stop the program before it runs (checked on
    /// hardware with `"A"*-1`).
    fn check_constant(&self, expr: &Expr) -> Result<()> {
        let constant = |e: &Expr| self.const_value(e).is_ok();
        let operands = match expr {
            Expr::Unary { expr, .. } => constant(expr),
            Expr::Binary { lhs, rhs, .. } => constant(lhs) && constant(rhs),
            _ => false,
        };
        if operands {
            self.const_value(expr)?;
        }
        Ok(())
    }

    fn call_expr(&mut self, name: &Var, args: &[Option<Expr>]) -> Result<()> {
        let full = call_name(name);
        match (full.as_str(), args) {
            ("CALLIDX", []) => {
                self.emit(Op::CallIdx);
                return Ok(());
            }
            ("DEFARGC", []) | ("DEFOUTC", []) => {
                self.require_def()?;
                self.emit(if full == "DEFARGC" {
                    Op::DefArgc
                } else {
                    Op::DefOutc
                });
                return Ok(());
            }
            ("DEFARG", [Some(n)]) => {
                self.require_def()?;
                self.expr(n)?;
                self.emit(Op::DefArg);
                return Ok(());
            }
            _ => {}
        }
        self.args(args)?;
        let argc = args.len() as u8;
        let want = Want::Value;
        if let Some(&def) = self.out.def_names.get(&full) {
            self.emit(Op::CallDef { def, argc, want });
        } else if let Some(id) = builtins::lookup(&full, Form::Function) {
            let b = builtins::get(id);
            // Checked on hardware: ?ABS(1,2) is Wrong number of arguments.
            if !b.accepts(args.len()) {
                return Err(err(ErrorKind::WrongNumberOfArguments, full.clone()));
            }
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
    /// (checked on hardware). A builtin used in its other form is still
    /// an illegal call.
    fn call_common(&mut self, name: &str, argc: u8, want: Want) -> Result<()> {
        let err = self.unknown_call(name);
        if err.kind != ErrorKind::UndefinedFunction {
            return Err(err);
        }
        let name = self.string_const(name.encode_utf16().collect());
        self.emit(Op::CallCommon { name, argc, want });
        Ok(())
    }

    // ---- DEF ----

    fn def(&mut self, def: &Def) -> Result<()> {
        let index = self.out.def_names[&call_name(&def.name)];
        self.out.defs[index as usize].entry = self.here();
        self.def = Some(DefScope {
            index,
            is_function: def.is_function,
            locals: HashMap::new(),
            declared: HashSet::new(),
        });
        let params = match &def.params {
            Params::Fixed(p) => p.as_slice(),
            Params::Variadic => &[],
        };
        let outs = match &def.outs {
            Params::Fixed(o) => o.as_slice(),
            Params::Variadic => &[],
        };
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
        for k in declared {
            self.local_slot(k);
        }
        self.block(&def.body)?;
        if def.is_function {
            // UNVERIFIED: a function that ends without RETURN gives no value.
            self.emit(Op::Default);
            self.emit(Op::ReturnValue);
        } else {
            self.emit(Op::Return);
        }
        self.resolve_patches(Some(index))?;
        self.def = None;
        Ok(())
    }
}

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
    fn expr(e: &Expr, used: &mut Vec<Key>) {
        match e {
            Expr::Var(v) => used.push(key(v)),
            Expr::Index { base, indices } => {
                expr(base, used);
                indices.iter().for_each(|i| expr(i, used));
            }
            Expr::Call { args, .. } => args.iter().flatten().for_each(|a| expr(a, used)),
            Expr::DynamicCall { name, args } => {
                expr(name, used);
                args.iter().flatten().for_each(|a| expr(a, used));
            }
            Expr::DynamicVar(n) => expr(n, used),
            Expr::Unary { expr: inner, .. } => expr(inner, used),
            Expr::Binary { lhs, rhs, .. } => {
                expr(lhs, used);
                expr(rhs, used);
            }
            Expr::Int(_) | Expr::Real(_) | Expr::Str(_) | Expr::Label(_) | Expr::Constant(_) => {}
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
    for stmt in stmts {
        match &stmt.kind {
            StmtKind::Assign { target, value } => {
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
        if !matches!(stmt.kind, StmtKind::Def(_)) {
            for body in child_blocks(&stmt.kind) {
                collect_vars(body, used, declared);
            }
        }
    }
}

/// Whether an INPUT target takes a string or a number, from its suffix.
fn input_kind(target: &LValue) -> InputKind {
    match target {
        LValue::Var(v) if v.suffix == Some(Suffix::Str) => InputKind::Str,
        LValue::Var(_) => InputKind::Number,
        LValue::Index { base, .. } => input_kind(base),
        // UNVERIFIED: VAR("name$") targets; the name is only known at run
        // time unless it is a literal.
        LValue::Dynamic(name) => match &**name {
            Expr::Str(s) if s.last() == Some(&u16::from(b'$')) => InputKind::Str,
            _ => InputKind::Number,
        },
    }
}
