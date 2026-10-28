//! Bytecode interpreter.
//!
//! The VM runs in slices: [`Vm::run`] executes up to a budget of
//! instructions and returns, so the frontend can keep drawing frames.
//! VSYNC and WAIT return [`Status::Wait`] and resume on the next call.

use std::collections::HashMap;
use std::rc::Rc;

use crate::ast::BinaryOp;
use crate::builtins::{self, Form, Kind};
use crate::bytecode::*;
use crate::error::{Error, ErrorKind, Result};
use crate::host::{Exec, Host};
use crate::lexer::Suffix;
use crate::ops;
use crate::value::{Array, Value, format_number, new_str};

/// UNVERIFIED: SmileBASIC's own nesting limits are not documented.
pub const MAX_CALL_DEPTH: usize = 4096;
pub const MAX_GOSUB_DEPTH: usize = 4096;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    /// The instruction budget ran out; call [`Vm::run`] again.
    Running,
    /// VSYNC or WAIT: let `frames` frames pass, then call [`Vm::run`] again.
    /// For VSYNC the frames count from the previous VSYNC.
    Wait {
        frames: u32,
        vsync: bool,
    },
    Ended,
}

/// Waits a frame and runs the same instruction again.
const RETRY: Status = Status::Wait {
    frames: 1,
    vsync: false,
};

struct Frame {
    def: u32,
    /// The slot the DEF is in, and the one to return to.
    home: usize,
    caller: usize,
    locals: Vec<Value>,
    /// Arguments of a `*` DEF.
    args: Vec<Value>,
    /// Results of an `OUT *` DEF.
    outs: Vec<Value>,
    want: Want,
    return_pc: usize,
    gosub_base: usize,
    stack_base: usize,
}

/// A program slot's program with its variables and READ's place. The
/// VM keeps the running slot's in its own fields, and the others here.
struct Unit {
    program: Rc<Compiled>,
    globals: Vec<Value>,
    declared: Vec<bool>,
    data_pos: usize,
}

impl Unit {
    fn new(program: Compiled) -> Unit {
        let globals = (program.globals.iter())
            .map(|g| initial_value(g, program.default_int))
            .collect();
        Unit {
            declared: vec![false; program.globals.len()],
            program: Rc::new(program),
            globals,
            data_pos: 0,
        }
    }
}

/// Where EXEC goes back to when the slot it started ends.
struct ExecReturn {
    slot: usize,
    pc: usize,
    frames: usize,
    gosubs: usize,
    stack: usize,
    callbacks: usize,
}

pub struct Vm {
    program: Rc<Compiled>,
    pc: usize,
    stack: Vec<Value>,
    globals: Vec<Value>,
    /// Which globals exist, for direct mode's declarations.
    declared: Vec<bool>,
    frames: Vec<Frame>,
    /// Slot and address to return to.
    gosubs: Vec<(usize, usize)>,
    data_pos: usize,
    /// The program slot running, and whether errors name it (not in
    /// direct mode).
    slot: usize,
    in_slot: bool,
    /// The other slots EXEC has run, by slot.
    units: Vec<Option<Unit>>,
    execs: Vec<ExecReturn>,
    ended: bool,
    /// An INPUT or LINPUT has shown its prompt and waits for a line.
    reading_input: bool,
    /// Each CALL SPRITE or CALL TEXT under way, with the number of the
    /// callback it last ran (-1 before the first).
    callbacks: Vec<(Callbacks, i32)>,
    call_idx: i32,
    /// Instructions run so far.
    executed: u64,
}

/// A variable's starting value. The suffix decides only this: variables
/// keep whatever value they are given (checked on hardware: `A%=1E10`
/// keeps the real, `A$` takes a number and `A%` a string).
fn initial_value(var: &VarInfo, default_int: bool) -> Value {
    match var.suffix {
        Some(Suffix::Int) => Value::Int(0),
        Some(Suffix::Real) => Value::Real(0.0),
        Some(Suffix::Str) => Value::Str(new_str(Vec::new())),
        None if default_int => Value::Int(0),
        None => Value::Real(0.0),
    }
}

fn form(want: Want) -> Form {
    match want {
        Want::Value => Form::Function,
        Want::Outs(_) => Form::Command,
    }
}

fn text_of(value: &Value) -> Result<Vec<u16>> {
    match value {
        Value::Str(s) => Ok(s.borrow().clone()),
        Value::Int(_) | Value::Real(_) => Ok(format_number(value)),
        _ => Err(ErrorKind::TypeMismatch.into()),
    }
}

/// Splits an INPUT line into one value per target. `None` asks the user
/// to type the line again.
fn parse_input(line: &[u16], kinds: &[InputKind], whole_line: bool) -> Option<Vec<Value>> {
    if whole_line {
        return Some(vec![Value::Str(new_str(line.to_vec()))]);
    }
    let fields: Vec<&[u16]> = line.split(|&c| c == u16::from(b',')).collect();
    // UNVERIFIED: extra fields are rejected like missing ones.
    if fields.len() != kinds.len() {
        return None;
    }
    fields
        .iter()
        .zip(kinds)
        .map(|(field, kind)| match kind {
            InputKind::Str => Some(Value::Str(new_str(field.to_vec()))),
            // UNVERIFIED: an empty field reads as 0.
            InputKind::Number if field.iter().all(|&c| c == u16::from(b' ')) => Some(Value::Int(0)),
            // Spaces around a number make it ask again (checked on
            // hardware), as VAL reads them as 0.
            InputKind::Number => builtins::parse_number(field),
        })
        .collect()
}

/// "@NAME", "NAME" or "1:@NAME" to "NAME".
pub(crate) fn label_name(value: &Value) -> Result<String> {
    let s = String::from_utf16_lossy(&value.clone().into_str()?.borrow()).to_ascii_uppercase();
    // UNVERIFIED: slot prefixes are ignored until there are program slots.
    let s = s.split_once(':').map_or(s.as_str(), |(_, rest)| rest);
    Ok(s.strip_prefix('@').unwrap_or(s).to_string())
}

/// `"1:@NAME"` or `"1:NAME"` to slot 1 and `"NAME"`; without a slot
/// prefix, `None` and the name.
fn slot_and_name(value: &Value) -> Result<(Option<usize>, String)> {
    let s = String::from_utf16_lossy(&value.clone().into_str()?.borrow()).to_ascii_uppercase();
    let (slot, rest) = match s.split_once(':') {
        Some((slot, rest)) => {
            let slot = slot.trim().parse().map_err(|_| {
                Error::with_detail(ErrorKind::IllegalFunctionCall, format!("slot {slot:?}"))
            })?;
            (Some(slot), rest)
        }
        None => (None, s.as_str()),
    };
    Ok((slot, rest.strip_prefix('@').unwrap_or(rest).to_string()))
}

fn name_of(value: &Value) -> Result<String> {
    let s = String::from_utf16_lossy(&value.clone().into_str()?.borrow()).to_ascii_uppercase();
    Ok(s.split_once(':')
        .map_or(s.clone(), |(_, rest)| rest.to_string()))
}

impl Vm {
    pub fn new(program: Compiled) -> Self {
        Vm::with_globals(program, &HashMap::new())
    }

    /// Starts with the globals in `values`, keyed by full name (`A$`), set
    /// to those values. Direct mode uses it to keep variables between lines.
    pub fn with_globals(program: Compiled, values: &HashMap<String, Value>) -> Self {
        // Variables from earlier lines exist already, for Op::Declare.
        let declared = (program.globals.iter())
            .map(|g| values.contains_key(&g.full_name()))
            .collect();
        let globals = program
            .globals
            .iter()
            .map(|g| {
                values
                    .get(&g.full_name())
                    .cloned()
                    .unwrap_or_else(|| initial_value(g, program.default_int))
            })
            .collect();
        Vm {
            program: Rc::new(program),
            pc: 0,
            stack: Vec::new(),
            globals,
            declared,
            frames: Vec::new(),
            gosubs: Vec::new(),
            data_pos: 0,
            slot: 0,
            in_slot: false,
            units: Vec::new(),
            execs: Vec::new(),
            ended: false,
            reading_input: false,
            callbacks: Vec::new(),
            call_idx: 0,
            executed: 0,
        }
    }

    /// Runs as the program of `slot`, which errors name.
    pub fn in_slot(mut self, slot: usize) -> Self {
        self.slot = slot;
        self.in_slot = true;
        self
    }

    /// The program slot running, or `None` in direct mode.
    pub fn slot(&self) -> Option<usize> {
        self.in_slot.then_some(self.slot)
    }

    /// The program being run.
    pub fn program(&self) -> Rc<Compiled> {
        Rc::clone(&self.program)
    }

    /// Every named global with its current value, keyed by full name.
    pub fn globals(&self) -> impl Iterator<Item = (String, Value)> + '_ {
        self.program
            .globals
            .iter()
            .zip(&self.globals)
            .filter(|(info, _)| !info.name.starts_with('\0'))
            .map(|(info, value)| (info.full_name(), value.clone()))
    }

    /// Source line of the next instruction.
    pub fn line(&self) -> u32 {
        self.program.lines.get(self.pc).copied().unwrap_or(0)
    }

    /// Column of the statement the next instruction belongs to.
    pub fn col(&self) -> u32 {
        self.program.cols.get(self.pc).copied().unwrap_or(0)
    }

    pub fn is_ended(&self) -> bool {
        self.ended
    }

    /// Value of a global variable by its full name (`"A%"`), for tests and
    /// debuggers.
    pub fn global(&self, name: &str) -> Option<&Value> {
        let i = self
            .program
            .globals
            .iter()
            .position(|g| g.full_name() == name)?;
        Some(&self.globals[i])
    }

    /// Instructions run so far, for hosts that share out a budget.
    pub fn executed(&self) -> u64 {
        self.executed
    }

    /// Executes up to `budget` instructions.
    pub fn run(&mut self, host: &mut dyn Host, budget: u32) -> Result<Status> {
        if self.ended {
            return Ok(Status::Ended);
        }
        for _ in 0..budget {
            let pc = self.pc;
            self.executed += 1;
            match self.step(host) {
                Ok(None) => {}
                Ok(Some(status)) => {
                    if status == Status::Ended {
                        self.ended = true;
                    }
                    return Ok(status);
                }
                Err(e) => {
                    self.ended = true;
                    let line = self.program.lines.get(pc).copied().unwrap_or(0);
                    let col = self.program.cols.get(pc).copied().unwrap_or(0);
                    let slot = self.slot().map(|s| s as u32);
                    return Err(e.at_pos(line, col).in_slot(slot));
                }
            }
        }
        Ok(Status::Running)
    }

    fn pop(&mut self) -> Value {
        self.stack.pop().expect("compiler keeps the stack balanced")
    }

    fn pop_n(&mut self, n: usize) -> Vec<Value> {
        let at = self.stack.len() - n;
        self.stack.split_off(at)
    }

    fn frame(&self) -> Result<&Frame> {
        self.frames
            .last()
            .ok_or_else(|| ErrorKind::IllegalStatement.into())
    }

    fn locals(&mut self) -> &mut Vec<Value> {
        &mut self
            .frames
            .last_mut()
            .expect("compiler emits locals only in DEFs")
            .locals
    }

    fn load(&mut self, slot: Slot) -> Value {
        match slot {
            Slot::Global(i) => self.globals[i as usize].clone(),
            Slot::Local(i) => self.locals()[i as usize].clone(),
        }
    }

    fn store(&mut self, slot: Slot, value: Value) {
        match slot {
            Slot::Global(i) => self.globals[i as usize] = value,
            Slot::Local(i) => self.locals()[i as usize] = value,
        }
    }

    fn indices(&mut self, n: u8) -> Result<Vec<i32>> {
        self.pop_n(n.into()).iter().map(Value::to_int).collect()
    }

    /// The DEF running in this slot, if any.
    fn own_frame(&self) -> Option<&Frame> {
        self.frames.last().filter(|f| f.home == self.slot)
    }

    fn labels(&self) -> &std::collections::HashMap<String, Addr> {
        match self.own_frame() {
            Some(f) => &self.program.defs[f.def as usize].labels,
            None => &self.program.labels,
        }
    }

    /// A label's slot and address: in this slot (or the DEF running), or
    /// at the top level of another slot that EXEC has run, as `"1:@NAME"`.
    fn find_label(&self, value: &Value) -> Result<(usize, usize)> {
        let (slot, name) = slot_and_name(value)?;
        let found = match slot {
            Some(slot) if slot != self.slot => (self.program_in(slot))
                .and_then(|p| p.labels.get(&name))
                .map(|&a| (slot, a as usize)),
            _ => self.labels().get(&name).map(|&a| (self.slot, a as usize)),
        };
        found.ok_or_else(|| Error::with_detail(ErrorKind::UndefinedLabel, format!("@{name}")))
    }

    /// The program of a slot that runs or that EXEC has run.
    fn program_in(&self, slot: usize) -> Option<&Rc<Compiled>> {
        match slot == self.slot {
            true => Some(&self.program),
            false => self.units.get(slot)?.as_ref().map(|u| &u.program),
        }
    }

    /// Goes on in another slot that EXEC has run, keeping this one's
    /// program and variables for later.
    fn switch_to(&mut self, slot: usize, host: &mut dyn Host) {
        if slot == self.slot {
            return;
        }
        let unit = self.units[slot].take().expect("checked by program_in");
        let old = self.take_unit();
        self.park(self.slot, old);
        self.put_unit(unit, slot, host);
    }

    fn take_unit(&mut self) -> Unit {
        Unit {
            program: Rc::clone(&self.program),
            globals: std::mem::take(&mut self.globals),
            declared: std::mem::take(&mut self.declared),
            data_pos: self.data_pos,
        }
    }

    fn park(&mut self, slot: usize, unit: Unit) {
        if self.units.len() <= slot {
            self.units.resize_with(slot + 1, || None);
        }
        self.units[slot] = Some(unit);
    }

    fn put_unit(&mut self, unit: Unit, slot: usize, host: &mut dyn Host) {
        self.program = unit.program;
        self.globals = unit.globals;
        self.declared = unit.declared;
        self.data_pos = unit.data_pos;
        self.slot = slot;
        host.entered_slot(slot, &self.program);
    }

    /// A COMMON DEF of another slot that EXEC has run, the lowest first.
    fn find_common(&self, name: &str) -> Option<(usize, u32)> {
        (self.units.iter().enumerate()).find_map(|(slot, unit)| {
            let program = &unit.as_ref()?.program;
            let &def = program.def_names.get(name)?;
            program.defs[def as usize].common.then_some((slot, def))
        })
    }

    /// Calls a DEF of another slot (or this one), coming back here.
    fn call_in(
        &mut self,
        slot: usize,
        def: u32,
        args: Vec<Value>,
        want: Want,
        host: &mut dyn Host,
    ) -> Result<()> {
        let caller = self.slot;
        self.switch_to(slot, host);
        let entered = self.enter_def(def, args, want, caller);
        if entered.is_err() {
            self.switch_to(caller, host);
        }
        entered
    }

    /// END, or the end of the code: back to the EXEC that started this
    /// slot, if any.
    fn end(&mut self, host: &mut dyn Host) -> Option<Status> {
        let Some(back) = self.execs.pop() else {
            return Some(Status::Ended);
        };
        self.frames.truncate(back.frames);
        self.gosubs.truncate(back.gosubs);
        self.stack.truncate(back.stack);
        self.callbacks.truncate(back.callbacks);
        self.switch_to(back.slot, host);
        self.pc = back.pc;
        None
    }

    /// EXEC: runs a slot from its start, after loading a file into it if
    /// given. EXEC of another slot comes back when it ends; loading into
    /// the running slot replaces its program.
    fn exec(&mut self, args: Vec<Value>, host: &mut dyn Host) -> Result<Option<Status>> {
        let (file, slot) = match (&args[0], args.get(1)) {
            (Value::Str(s), slot) => {
                let slot = match slot {
                    Some(v) => v.to_int()?,
                    None => self.slot as i32,
                };
                (Some(s.borrow().clone()), slot)
            }
            (v, None) => (None, v.to_int()?),
            (_, Some(_)) => return Err(ErrorKind::TypeMismatch.into()),
        };
        let running =
            |s: i32| s == self.slot as i32 || self.execs.iter().any(|r| r.slot as i32 == s);
        // EXEC of a slot must not start one that is running, and loading
        // may replace only the slot running now.
        let own = file.is_some() && slot == self.slot as i32;
        if running(slot) && !own {
            return Err(Error::with_detail(
                ErrorKind::IllegalFunctionCall,
                format!("slot {slot} is running"),
            ));
        }
        let source = match host.exec_source(file.as_deref(), slot)? {
            Exec::Run(source) => source,
            Exec::Wait => {
                self.stack.extend(args);
                return Ok(Some(RETRY));
            }
            Exec::Skip => {
                self.pc += 1;
                return Ok(None);
            }
        };
        let slot = slot as usize;
        let program = crate::compile(&source, &*host).map_err(|e| e.in_slot(Some(slot as u32)))?;
        let unit = Unit::new(program);
        if own {
            let base = self.execs.last();
            let frames = base.map_or(0, |b| b.frames);
            let gosubs = base.map_or(0, |b| b.gosubs);
            let stack = base.map_or(0, |b| b.stack);
            let callbacks = base.map_or(0, |b| b.callbacks);
            self.frames.truncate(frames);
            self.gosubs.truncate(gosubs);
            self.stack.truncate(stack);
            self.callbacks.truncate(callbacks);
        } else {
            self.execs.push(ExecReturn {
                slot: self.slot,
                pc: self.pc + 1,
                frames: self.frames.len(),
                gosubs: self.gosubs.len(),
                stack: self.stack.len(),
                callbacks: self.callbacks.len(),
            });
            let old = self.take_unit();
            self.park(self.slot, old);
            if let Some(stale) = self.units.get_mut(slot) {
                *stale = None;
            }
        }
        self.put_unit(unit, slot, host);
        self.pc = 0;
        Ok(None)
    }

    /// Calls one callback of CALL SPRITE or CALL TEXT: GOSUB to a
    /// `"@label"`, or a DEF command without arguments.
    fn run_callback(
        &mut self,
        program: &Compiled,
        target: &Value,
        host: &mut dyn Host,
    ) -> Result<()> {
        if target.clone().into_str()?.borrow().first() == Some(&u16::from(b'@')) {
            let (slot, label) = self.find_label(target)?;
            return self.gosub(slot, label, host);
        }
        let name = name_of(target)?;
        let def = *program
            .def_names
            .get(&name)
            .ok_or_else(|| Error::with_detail(ErrorKind::UndefinedFunction, name))?;
        self.enter_def(def, Vec::new(), Want::Outs(0), self.slot)
    }

    fn gosub(&mut self, slot: usize, target: usize, host: &mut dyn Host) -> Result<()> {
        if self.gosubs.len() >= MAX_GOSUB_DEPTH {
            return Err(ErrorKind::StackOverflow.into());
        }
        self.gosubs.push((self.slot, self.pc + 1));
        self.switch_to(slot, host);
        self.pc = target;
        Ok(())
    }

    /// Runs one instruction. `Some` stops the current slice.
    fn step(&mut self, host: &mut dyn Host) -> Result<Option<Status>> {
        let program = Rc::clone(&self.program);
        let Some(op) = program.code.get(self.pc) else {
            return Ok(self.end(host));
        };
        let mut next = self.pc + 1;
        match op {
            Op::Int(n) => self.stack.push(Value::Int(*n)),
            Op::Real(x) => self.stack.push(Value::Real(*x)),
            Op::Str(i) => self
                .stack
                .push(Value::Str(new_str(program.strings[*i as usize].clone()))),
            Op::Default => self.stack.push(Value::Default),
            Op::Load(slot) => {
                let v = self.load(*slot);
                self.stack.push(v);
            }
            Op::Declare { slot, array } => {
                let i = *slot as usize;
                if std::mem::replace(&mut self.declared[i], true) {
                    let name = self.program.globals[i].full_name();
                    return Err(Error::with_detail(ErrorKind::DuplicateVariable, name));
                }
                self.globals[i] = match array {
                    true => Value::Default,
                    false => initial_value(&self.program.globals[i], self.program.default_int),
                };
            }
            Op::Store(slot) => {
                let v = self.pop();
                self.store(*slot, v);
            }
            Op::Index(n) => {
                let idx = self.indices(*n)?;
                let base = self.pop();
                self.stack.push(index(&base, &idx)?);
            }
            Op::StoreIndex(n) => {
                let idx = self.indices(*n)?;
                let base = self.pop();
                let value = self.pop();
                store_index(&base, &idx, value)?;
            }
            Op::NewArray { elem, dims } => {
                let dims = self.pop_n((*dims).into());
                let dims = dims
                    .iter()
                    .map(|d| usize::try_from(d.to_int()?).map_err(|_| ErrorKind::OutOfRange.into()))
                    .collect::<Result<Vec<_>>>()?;
                // Only a one-dimensional array may be empty: DIM R#[0,0] is
                // Subscript out of range (checked on hardware).
                // UNVERIFIED: that one zero among other sizes is, too.
                if dims.len() > 1 && dims.contains(&0) {
                    return Err(ErrorKind::SubscriptOutOfRange.into());
                }
                // UNVERIFIED: the cap stands in for "out of memory".
                if dims
                    .iter()
                    .try_fold(1usize, |a, &d| a.checked_mul(d))
                    .is_none_or(|t| t > 1 << 28)
                {
                    return Err(ErrorKind::OutOfRange.into());
                }
                self.stack
                    .push(Value::Array(Rc::new(Array::new(*elem, dims).into())));
            }
            Op::ArrayLit { elem, len } => {
                let values = self.pop_n(*len as usize);
                let mut array = Array::new(*elem, vec![values.len()]);
                for (i, v) in values.into_iter().enumerate() {
                    array.set(i, v)?;
                }
                self.stack.push(Value::Array(Rc::new(array.into())));
            }
            Op::FillArray(len) => {
                let values = self.pop_n(*len as usize);
                let array = self.pop().into_array()?;
                {
                    let mut a = array.borrow_mut();
                    // UNVERIFIED: more values than elements is an error.
                    if values.len() > a.len() {
                        return Err(ErrorKind::SubscriptOutOfRange.into());
                    }
                    for (i, v) in values.into_iter().enumerate() {
                        a.set(i, v)?;
                    }
                }
                self.stack.push(Value::Array(array));
            }
            Op::Unary(op) => {
                let v = self.pop();
                self.stack.push(ops::unary(*op, &v)?);
            }
            Op::Binary(op) => {
                let rhs = self.pop();
                let lhs = self.pop();
                self.stack.push(ops::binary(*op, &lhs, &rhs)?);
            }
            Op::Truthy => {
                let b = self.pop().is_true()?;
                self.stack.push(Value::Int(i32::from(b)));
            }
            Op::Pop => _ = self.pop(),
            Op::Swap => {
                let n = self.stack.len();
                self.stack.swap(n - 1, n - 2);
            }
            Op::Reverse(n) => {
                let from = self.stack.len() - usize::from(*n);
                self.stack[from..].reverse();
            }
            Op::Jump(a) => next = *a as usize,
            Op::JumpIfFalse(a) => {
                if !self.pop().is_true()? {
                    next = *a as usize;
                }
            }
            Op::JumpIfTrue(a) => {
                if self.pop().is_true()? {
                    next = *a as usize;
                }
            }
            Op::Gosub(a) => {
                self.gosub(self.slot, *a as usize, host)?;
                return Ok(None);
            }
            Op::GotoDyn => {
                let label = self.pop();
                let (slot, target) = self.find_label(&label)?;
                self.switch_to(slot, host);
                next = target;
            }
            Op::GosubDyn => {
                let label = self.pop();
                let (slot, target) = self.find_label(&label)?;
                self.gosub(slot, target, host)?;
                return Ok(None);
            }
            Op::On { count, gosub } => {
                let selector = self.pop().to_int()?;
                let after = self.pc + 1 + *count as usize;
                next = after;
                // UNVERIFIED: an out-of-range selector continues after ON.
                if (0..*count as i32).contains(&selector) {
                    let Op::Jump(target) = program.code[self.pc + 1 + selector as usize] else {
                        unreachable!("ON is followed by its jump table");
                    };
                    if *gosub {
                        if self.gosubs.len() >= MAX_GOSUB_DEPTH {
                            return Err(ErrorKind::StackOverflow.into());
                        }
                        self.gosubs.push((self.slot, after));
                    }
                    next = target as usize;
                }
            }
            Op::Return => next = self.return_op(host)?,
            Op::ReturnValue => {
                let value = self.pop();
                next = self.leave_def(vec![value], host)?;
            }
            Op::CallDef { def, argc, want } => {
                let args = self.pop_n((*argc).into());
                self.enter_def(*def, args, *want, self.slot)?;
                return Ok(None);
            }
            Op::CallCommon { name, argc, want } => {
                let name = String::from_utf16_lossy(&program.strings[*name as usize]);
                let (slot, def) = self
                    .find_common(&name)
                    .ok_or_else(|| Error::with_detail(ErrorKind::UndefinedFunction, name))?;
                let args = self.pop_n((*argc).into());
                self.call_in(slot, def, args, *want, host)?;
                return Ok(None);
            }
            Op::CallBuiltin { id, argc, want } => {
                let args = self.pop_n((*argc).into());
                self.call_builtin(&program, *id, &args, *want)?;
            }
            Op::CallHost { id, argc, want } => {
                let args = self.pop_n((*argc).into());
                if !self.call_host(host, *id, &args, *want)? {
                    self.stack.extend(args);
                    return Ok(Some(RETRY));
                }
            }
            Op::CallDyn { argc, want } => {
                let args = self.pop_n((*argc).into());
                let name_value = self.pop();
                let (slot, name) = slot_and_name(&name_value)?;
                if let Some(slot) = slot.filter(|&s| s != self.slot) {
                    // CALL "1:NAME": a DEF of a slot that EXEC has run.
                    let def = (self.program_in(slot))
                        .and_then(|p| p.def_names.get(&name).copied())
                        .ok_or_else(|| Error::with_detail(ErrorKind::UndefinedFunction, name))?;
                    self.call_in(slot, def, args, *want, host)?;
                    return Ok(None);
                }
                if let Some(&def) = program.def_names.get(&name) {
                    self.enter_def(def, args, *want, self.slot)?;
                    return Ok(None);
                } else if let Some((slot, def)) = self.find_common(&name) {
                    self.call_in(slot, def, args, *want, host)?;
                    return Ok(None);
                } else if let Some(id) = builtins::lookup(&name, form(*want)) {
                    self.call_builtin(&program, id, &args, *want)?;
                } else if let Some(id) = host.lookup(&name) {
                    if !self.call_host(host, id, &args, *want)? {
                        self.stack.push(name_value);
                        self.stack.extend(args);
                        return Ok(Some(RETRY));
                    }
                } else {
                    return Err(Error::with_detail(ErrorKind::UndefinedFunction, name));
                }
            }
            Op::LoadDyn => {
                let name = name_of(&self.pop())?;
                let slot = self.dynamic_slot(&name)?;
                let v = self.load(slot);
                self.stack.push(v);
            }
            Op::StoreDyn => {
                let name = name_of(&self.pop())?;
                let slot = self.dynamic_slot(&name)?;
                let v = self.pop();
                self.store(slot, v);
            }
            Op::CallbacksStart(kind) => self.callbacks.push((*kind, -1)),
            Op::CallbackNext(end) => {
                let (kind, at) = self
                    .callbacks
                    .last_mut()
                    .ok_or(ErrorKind::IllegalStatement)?;
                match host.next_callback(*kind, *at) {
                    None => {
                        self.call_idx = host.callbacks_end(*kind);
                        self.callbacks.pop();
                        next = *end as usize;
                    }
                    Some((idx, target)) => {
                        *at = idx;
                        self.call_idx = idx;
                        self.run_callback(&program, &Value::Str(new_str(target)), host)?;
                        return Ok(None);
                    }
                }
            }
            Op::CallIdx => self.stack.push(Value::Int(self.call_idx)),
            Op::DefArgc => {
                let f = self.frame()?;
                let n = match program.defs[f.def as usize].params {
                    Some(n) => usize::from(n),
                    None => f.args.len(),
                };
                self.stack.push(Value::Int(n as i32));
            }
            Op::DefArg => {
                let n = self.pop().to_int()?;
                let f = self.frame()?;
                let v = match program.defs[f.def as usize].params {
                    Some(p) => (0..i32::from(p))
                        .contains(&n)
                        .then(|| f.locals[n as usize].clone()),
                    None => usize::try_from(n).ok().and_then(|n| f.args.get(n).cloned()),
                };
                self.stack.push(v.ok_or(ErrorKind::OutOfRange)?);
            }
            Op::DefOutc => {
                let n = self.frame()?.want.count();
                self.stack.push(Value::Int(n as i32));
            }
            Op::DefOut => {
                let value = self.pop();
                let n = self.pop().to_int()?;
                let info = &program.defs[self.frame()?.def as usize];
                let frame = self.frames.last_mut().expect("checked by frame()");
                let n = usize::try_from(n).map_err(|_| ErrorKind::OutOfRange)?;
                match (info.params, info.outs) {
                    (params, Some(outs)) if n < usize::from(outs) => {
                        let slot = usize::from(params.unwrap_or(0)) + n;
                        frame.locals[slot] = value;
                    }
                    (_, None) if n < frame.outs.len() => frame.outs[n] = value,
                    _ => return Err(ErrorKind::OutOfRange.into()),
                }
            }
            Op::ForTest => {
                let step = self.pop();
                let to = self.pop();
                let var = self.pop();
                let ordering = if step.to_real()? >= 0.0 {
                    BinaryOp::LessEq
                } else {
                    BinaryOp::GreaterEq
                };
                self.stack.push(ops::binary(ordering, &var, &to)?);
            }
            Op::Print { screen } => {
                let value = self.pop();
                let screen = self.screen_arg(*screen)?;
                host.print(screen, &text_of(&value)?)?;
            }
            Op::PrintTab { screen } => {
                let screen = self.screen_arg(*screen)?;
                host.print_tab(screen)?;
            }
            Op::PrintNewline { screen } => {
                let screen = self.screen_arg(*screen)?;
                host.print_newline(screen)?;
            }
            Op::Wait { vsync, has_arg } => {
                let frames = if *has_arg { self.pop().to_int()? } else { 1 };
                // UNVERIFIED: zero or negative counts do not wait.
                self.pc = next;
                return Ok(Some(Status::Wait {
                    frames: frames.max(0) as u32,
                    vsync: *vsync,
                }));
            }
            Op::Input {
                kinds,
                question,
                whole_line,
            } => {
                if !self.reading_input {
                    let prompt = self.stack.last().expect("INPUT prompt");
                    if !matches!(prompt, Value::Default) {
                        host.print(None, &text_of(prompt)?)?;
                    }
                    if *question {
                        host.print(None, &[u16::from(b'?')])?;
                    }
                    host.input_start()?;
                    self.reading_input = true;
                }
                // Until the line is complete, run this instruction again
                // on the next frame.
                let Some(line) = host.input_poll()? else {
                    return Ok(Some(RETRY));
                };
                self.reading_input = false;
                match parse_input(&line, kinds, *whole_line) {
                    Some(values) => {
                        self.pop();
                        self.stack.extend(values);
                    }
                    None => {
                        let redo: Vec<u16> = "?Redo from start".encode_utf16().collect();
                        host.print(None, &redo)?;
                        host.print_newline(None)?;
                        return Ok(None);
                    }
                }
            }
            Op::Restore(pos) => self.data_pos = *pos as usize,
            Op::RestoreDyn => {
                let name = label_name(&self.pop())?;
                let pos = program.data_labels.get(&name).ok_or_else(|| {
                    Error::with_detail(ErrorKind::UndefinedLabel, format!("@{name}"))
                })?;
                self.data_pos = *pos as usize;
            }
            Op::Read => {
                let item = program
                    .data
                    .get(self.data_pos)
                    .ok_or(ErrorKind::OutOfData)?;
                self.data_pos += 1;
                self.stack.push(match item {
                    Literal::Int(n) => Value::Int(*n),
                    Literal::Real(x) => Value::Real(*x),
                    Literal::Str(s) => Value::Str(new_str(s.clone())),
                });
            }
            Op::End => return Ok(self.end(host)),
            Op::Exec(argc) => {
                let args = self.pop_n((*argc).into());
                return self.exec(args, host);
            }
        }
        self.pc = next;
        Ok(None)
    }

    fn screen_arg(&mut self, present: bool) -> Result<Option<i32>> {
        if present {
            Ok(Some(self.pop().to_int()?))
        } else {
            Ok(None)
        }
    }

    fn dynamic_slot(&self, name: &str) -> Result<Slot> {
        if let Some(f) = self.own_frame() {
            let locals = &self.program.defs[f.def as usize].locals;
            if let Some(i) = locals.iter().position(|v| v.full_name() == name) {
                return Ok(Slot::Local(i as u32));
            }
        }
        self.program
            .globals
            .iter()
            .position(|v| v.full_name() == name)
            .map(|i| Slot::Global(i as u32))
            .ok_or_else(|| Error::with_detail(ErrorKind::UndefinedVariable, name.to_string()))
    }

    fn push_results(&mut self, results: Vec<Value>, want: Want) -> Result<()> {
        if results.len() != want.count() {
            return Err(ErrorKind::IllegalFunctionCall.into());
        }
        self.stack.extend(results);
        Ok(())
    }

    fn call_builtin(
        &mut self,
        program: &Compiled,
        id: u16,
        args: &[Value],
        want: Want,
    ) -> Result<()> {
        let b = builtins::get(id);
        if !b.accepts(args.len()) {
            return Err(Error::with_detail(
                ErrorKind::WrongNumberOfArguments,
                b.name,
            ));
        }
        let results = match (b.kind, want) {
            (Kind::Function(f), Want::Value) => vec![f(args)?],
            (Kind::Command(f), Want::Outs(n)) => f(args, n.into())?,
            (Kind::Data(f), Want::Outs(n)) => {
                let mut data = builtins::Data {
                    program,
                    pos: &mut self.data_pos,
                };
                f(args, n.into(), &mut data)?
            }
            _ => return Err(Error::with_detail(ErrorKind::IllegalFunctionCall, b.name)),
        };
        self.push_results(results, want)
    }

    /// Returns false, leaving the stack alone, if the host asked for the
    /// call to be made again on the next frame.
    fn call_host(
        &mut self,
        host: &mut dyn Host,
        id: u32,
        args: &[Value],
        want: Want,
    ) -> Result<bool> {
        host.set_data_pos(self.data_pos);
        let results = host.call(id, args, want);
        if let Some(pos) = host.data_pos() {
            self.data_pos = pos;
        }
        for slot in host.take_reloaded() {
            if let Some(unit) = self.units.get_mut(slot) {
                *unit = None;
            }
        }
        let results = results?;
        if host.retry() {
            return Ok(false);
        }
        self.push_results(results, want)?;
        Ok(true)
    }

    /// Calls DEF `def` of the running slot, returning to slot `caller`.
    fn enter_def(&mut self, def: u32, args: Vec<Value>, want: Want, caller: usize) -> Result<()> {
        let info = &self.program.defs[def as usize];
        let call_ok = match want {
            Want::Value => info.is_function,
            Want::Outs(n) => !info.is_function && info.outs.is_none_or(|m| m == n),
        };
        let args_ok = info.params.is_none_or(|p| usize::from(p) == args.len());
        // UNVERIFIED: that a DEF given the wrong number of arguments is
        // the same error as a builtin.
        if !call_ok || !args_ok {
            let kind = match call_ok {
                true => ErrorKind::WrongNumberOfArguments,
                false => ErrorKind::IllegalFunctionCall,
            };
            return Err(Error::with_detail(kind, info.name.clone()));
        }
        if self.frames.len() >= MAX_CALL_DEPTH {
            return Err(ErrorKind::StackOverflow.into());
        }
        let default_int = self.program.default_int;
        let mut locals: Vec<Value> = info
            .locals
            .iter()
            .map(|v| initial_value(v, default_int))
            .collect();
        let variadic_args = match info.params {
            Some(_) => {
                for (i, a) in args.into_iter().enumerate() {
                    locals[i] = a;
                }
                Vec::new()
            }
            None => args,
        };
        let outs = match info.outs {
            None => vec![Value::Default; want.count()],
            Some(_) => Vec::new(),
        };
        self.frames.push(Frame {
            def,
            home: self.slot,
            caller,
            locals,
            args: variadic_args,
            outs,
            want,
            return_pc: self.pc + 1,
            gosub_base: self.gosubs.len(),
            stack_base: self.stack.len(),
        });
        self.pc = info.entry as usize;
        Ok(())
    }

    /// RETURN: from GOSUB if one is pending in this scope, else from the DEF.
    fn return_op(&mut self, host: &mut dyn Host) -> Result<usize> {
        let base = self.frames.last().map_or(0, |f| f.gosub_base);
        if self.gosubs.len() > base {
            let (slot, pc) = self.gosubs.pop().expect("length checked");
            self.switch_to(slot, host);
            return Ok(pc);
        }
        let Some(frame) = self.frames.last() else {
            return Err(ErrorKind::ReturnWithoutGosub.into());
        };
        let info = &self.program.defs[frame.def as usize];
        let results = match (info.is_function, info.outs) {
            // UNVERIFIED: a bare RETURN in a function gives no value.
            (true, _) => vec![Value::Default],
            (false, Some(n)) => {
                let start = usize::from(info.params.unwrap_or(0));
                frame.locals[start..start + usize::from(n)].to_vec()
            }
            (false, None) => frame.outs.clone(),
        };
        self.leave_def(results, host)
    }

    fn leave_def(&mut self, results: Vec<Value>, host: &mut dyn Host) -> Result<usize> {
        let frame = self.frames.pop().ok_or(ErrorKind::IllegalStatement)?;
        self.gosubs.truncate(frame.gosub_base);
        self.stack.truncate(frame.stack_base);
        self.push_results(results, frame.want)?;
        self.switch_to(frame.caller, host);
        Ok(frame.return_pc)
    }
}

fn index(base: &Value, idx: &[i32]) -> Result<Value> {
    match base {
        Value::Array(a) => {
            let a = a.borrow();
            let at = a.offset(idx)?;
            Ok(a.get(at))
        }
        Value::Str(s) => {
            let s = s.borrow();
            let [i] = idx else {
                return Err(ErrorKind::SubscriptOutOfRange.into());
            };
            let c = usize::try_from(*i)
                .ok()
                .and_then(|i| s.get(i))
                .ok_or(ErrorKind::SubscriptOutOfRange)?;
            Ok(Value::Str(new_str(vec![*c])))
        }
        _ => Err(ErrorKind::TypeMismatch.into()),
    }
}

fn store_index(base: &Value, idx: &[i32], value: Value) -> Result<()> {
    match base {
        Value::Array(a) => {
            let mut a = a.borrow_mut();
            let at = a.offset(idx)?;
            a.set(at, value)
        }
        Value::Str(s) => {
            // `A$[3]="ABC"` replaces one character with the whole string.
            let piece = value.into_str()?.borrow().clone();
            let mut s = s.borrow_mut();
            let [i] = idx else {
                return Err(ErrorKind::SubscriptOutOfRange.into());
            };
            let i = usize::try_from(*i)
                .ok()
                .filter(|&i| i < s.len())
                .ok_or(ErrorKind::SubscriptOutOfRange)?;
            s.splice(i..=i, piece);
            Ok(())
        }
        _ => Err(ErrorKind::TypeMismatch.into()),
    }
}
