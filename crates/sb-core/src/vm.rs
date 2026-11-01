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
use crate::value::{Array, Str, Value, format_number, new_str};

/// UNVERIFIED: SmileBASIC's own nesting limits are not documented.
pub const MAX_CALL_DEPTH: usize = 4096;
pub const MAX_GOSUB_DEPTH: usize = 4096;
/// How many READ places RESTORE "PUSH" keeps.
const MAX_DATA_KEPT: usize = 65536;

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
    /// Waiting on the user or the host, such as INPUT or a confirmation:
    /// call [`Vm::run`] again on the next frame. Unlike WAIT, it leaves
    /// VSYNC's count alone (checked on hardware for INPUT).
    Retry,
    Ended,
    /// The + button stops the program after the statement on `line`.
    Break {
        line: u32,
    },
    /// STOP on `line`. [`Vm::run`] goes on after it.
    Stop {
        line: u32,
    },
    /// The statement [`Vm::trace`] asked for, on `line`, has run.
    Traced {
        line: u32,
    },
    /// A breakpoint, or a step [`Vm::step_by`] asked for, stops the
    /// program before the statement on `line`.
    Debug {
        line: u32,
    },
}

/// How far a debugger's step runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    /// To the next statement, into DEFs and GOSUBs.
    Into,
    /// To the next statement that is not in a DEF or GOSUB it calls.
    Over,
    /// Out of the DEF or GOSUB under way, to the statement after.
    Out,
}

/// A DEF or GOSUB under way, or the program's top level, as a debugger
/// lists them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Call {
    /// Where it is: the line of the call it made, or for the innermost
    /// one, the line about to run.
    pub slot: usize,
    pub line: u32,
    pub kind: CallKind,
    /// The DEF call whose locals it sees, as an index for
    /// [`Vm::frame_locals`]; `None` sees the globals only.
    pub frame: Option<usize>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CallKind {
    Top,
    Def(String),
    Gosub,
}

/// Commands and functions the compiler turns into instructions of their
/// own rather than calls, for CHKCALL.
const VM_CALLS: &[&str] = &[
    "CALLIDX", "DEFARG", "DEFARGC", "DEFOUTC", "CHKLABEL", "CHKCALL", "CHKVAR", "VSYNC", "WAIT",
    "FREEMEM",
];

/// Names CHKCALL does not count although they are commands here.
const NOT_CALLS: &[&str] = &[
    "INC",
    "DEC",
    "STOP",
    "RUN",
    "CONT",
    "NEW",
    "CLEAR",
    "TRACE",
    "BACKTRACE",
];

/// The characters of STOP's message that are shown.
const STOP_MESSAGE: usize = 255;

/// Waits a frame and runs the same instruction again.
const RETRY: Status = Status::Retry;

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
    data_from: Option<Rc<Compiled>>,
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
            data_from: None,
        }
    }
}

/// TRACE runs until the start of the statement after the first one it
/// runs into, and names that one's line. A debugger's step instead stops
/// at the first statement start after an instruction has run, within
/// `depth` calls, and names the line it stops before: it starts where
/// `executed` was `from`.
struct Trace {
    line: Option<u32>,
    depth: Option<usize>,
    from: u64,
}

/// Where breakpoints stop one slot's program: whether each instruction
/// is one, for the program they were worked out for.
struct Stops {
    program: Rc<Compiled>,
    at: Rc<[bool]>,
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
    /// Slot and address to return to, and how many READ places
    /// RESTORE "PUSH" had kept then.
    gosubs: Vec<(usize, usize, usize)>,
    data_pos: usize,
    /// Another slot's program, when RESTORE "n:@LABEL" sent READ there.
    data_from: Option<Rc<Compiled>>,
    /// READ places kept by RESTORE "PUSH".
    data_kept: Vec<(usize, Option<Rc<Compiled>>)>,
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
    /// ON BREAK GOTO's slot and address, until the + button jumps there.
    on_break: Option<(usize, usize)>,
    /// After a jump out of INPUT, the press is acted on again once a
    /// statement has run: the instruction count at the jump, and the line
    /// of the last instruction run since.
    break_pending: Option<(u64, u32)>,
    /// STOP's message, shown after the line it stopped on.
    stop_message: Option<Str>,
    /// TRACE's statement under way.
    trace: Option<Trace>,
    /// Lines with breakpoints, by slot, and the one a debugger runs to
    /// once.
    breakpoints: Vec<Vec<u32>>,
    run_to: Option<(usize, u32)>,
    /// Worked out from them by slot, and the running program's.
    stops: Vec<Option<Stops>>,
    stop_at: Option<Rc<[bool]>>,
    /// The slot and instruction the program goes on from, which a
    /// breakpoint there does not stop it at.
    passed: Option<(usize, usize)>,
}

/// A variable's starting value. The suffix decides only this: variables
/// keep whatever value they are given (checked on hardware: `A%=1E10`
/// keeps the real, `A$` takes a number and `A%` a string).
fn initial_value(var: &VarInfo, default_int: bool) -> Value {
    if var.array {
        return Value::Default;
    }
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
    // Extra fields are left out (checked on hardware). UNVERIFIED:
    // whether they are checked at all.
    if fields.len() < kinds.len() {
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
            data_from: None,
            data_kept: Vec::new(),
            slot: 0,
            in_slot: false,
            units: Vec::new(),
            execs: Vec::new(),
            ended: false,
            reading_input: false,
            callbacks: Vec::new(),
            call_idx: 0,
            executed: 0,
            on_break: None,
            break_pending: None,
            stop_message: None,
            trace: None,
            breakpoints: Vec::new(),
            run_to: None,
            stops: Vec::new(),
            stop_at: None,
            passed: None,
        }
    }

    /// Runs as the program of `slot`, which errors name.
    pub fn in_slot(mut self, slot: usize) -> Self {
        self.slot = slot;
        self.in_slot = true;
        self.update_stops();
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

    /// Gives up an INPUT or LINPUT that waits for its line, so that CONT
    /// goes on with the next statement.
    pub fn leave_input(&mut self) {
        if !self.reading_input {
            return;
        }
        self.reading_input = false;
        self.pop();
        self.pc += 1;
        while self.program.starts.get(self.pc) == Some(&false) {
            self.pc += 1;
        }
        self.pass_here();
    }

    /// TRACE: the next [`Vm::run`] runs one statement and returns
    /// [`Status::Traced`].
    pub fn trace(&mut self) {
        self.trace = Some(Trace {
            line: None,
            depth: None,
            from: self.executed,
        });
    }

    /// A debugger's step: the next [`Vm::run`] runs at least one
    /// statement and returns [`Status::Debug`] before the one it stops
    /// at. Stepping out of the top level runs on as CONT does.
    pub fn step_by(&mut self, step: Step) {
        let depth = self.depth();
        let depth = match step {
            Step::Into => usize::MAX,
            Step::Over => depth,
            Step::Out => match depth.checked_sub(1) {
                Some(depth) => depth,
                None => return,
            },
        };
        self.trace = Some(Trace {
            line: None,
            depth: Some(depth),
            from: self.executed,
        });
    }

    /// How many DEFs, GOSUBs and EXECs are under way.
    fn depth(&self) -> usize {
        self.frames.len() + self.gosubs.len() + self.execs.len()
    }

    /// Lets the program go on from here without a breakpoint here
    /// stopping it again at once.
    pub fn pass_here(&mut self) {
        self.passed = Some((self.slot, self.pc));
    }

    /// Sets the lines of `slot` that stop the program before they run.
    /// A breakpoint stops where the line's code follows another line's,
    /// so a loop within the line stops there once.
    pub fn set_breakpoints(&mut self, slot: usize, lines: &[u32]) {
        if self.breakpoints.len() <= slot {
            self.breakpoints.resize_with(slot + 1, Vec::new);
        }
        self.breakpoints[slot] = lines.to_vec();
        self.stops.clear();
        self.update_stops();
    }

    /// Stops the program once before `line` of `slot` runs, with the
    /// breakpoints, or not with `None`.
    pub fn run_to(&mut self, to: Option<(usize, u32)>) {
        self.run_to = to;
        self.stops.clear();
        self.update_stops();
    }

    /// Works out where the running program's breakpoints stop it.
    fn update_stops(&mut self) {
        self.stop_at = None;
        if !self.in_slot {
            return;
        }
        let slot = self.slot;
        if let Some(Some(stops)) = self.stops.get(slot)
            && Rc::ptr_eq(&stops.program, &self.program)
        {
            self.stop_at = Some(Rc::clone(&stops.at));
            return;
        }
        let mut lines: Vec<u32> = self.breakpoints.get(slot).cloned().unwrap_or_default();
        lines.extend(self.run_to.filter(|to| to.0 == slot).map(|to| to.1));
        if lines.is_empty() {
            return;
        }
        let program = &self.program;
        let at: Rc<[bool]> = (program.lines.iter().enumerate())
            .map(|(pc, line)| lines.contains(line) && (pc == 0 || program.lines[pc - 1] != *line))
            .collect();
        if self.stops.len() <= slot {
            self.stops.resize_with(slot + 1, || None);
        }
        self.stops[slot] = Some(Stops {
            program: Rc::clone(program),
            at: Rc::clone(&at),
        });
        self.stop_at = Some(at);
    }

    /// Goes on from `line` of the running slot instead, as GOTO would, if
    /// that is in the same DEF, or outside DEFs as the program is. False
    /// when it is not.
    pub fn jump_to(&mut self, line: u32) -> bool {
        let program = &self.program;
        let def_at = |pc: usize| {
            (program.defs.iter()).position(|d| (d.entry as usize..d.end as usize).contains(&pc))
        };
        let here = def_at(self.pc);
        let to = (program.lines.iter().enumerate()).position(|(pc, &l)| {
            l == line && (pc == 0 || program.lines[pc - 1] != l) && def_at(pc) == here
        });
        let Some(to) = to else {
            return false;
        };
        self.pc = to;
        self.trace = None;
        self.pass_here();
        true
    }

    /// The program's top level and the DEFs and GOSUBs under way, with
    /// where each one is; the innermost last.
    pub fn calls(&self) -> Vec<Call> {
        let mut calls = vec![Call {
            slot: 0,
            line: 0,
            kind: CallKind::Top,
            frame: None,
        }];
        let mut gosubs = 0;
        let mut frame = None;
        for (i, f) in self.frames.iter().enumerate() {
            let base = f.gosub_base.max(gosubs);
            for &(s, r, _) in &self.gosubs[gosubs..base] {
                self.called_from(&mut calls, s, r, CallKind::Gosub, frame);
            }
            let name = (self.program_in(f.home))
                .and_then(|p| p.defs.get(f.def as usize))
                .map_or_else(String::new, |d| d.name.clone());
            frame = Some(i);
            self.called_from(
                &mut calls,
                f.caller,
                f.return_pc,
                CallKind::Def(name),
                frame,
            );
            gosubs = base;
        }
        for &(s, r, _) in &self.gosubs[gosubs..] {
            self.called_from(&mut calls, s, r, CallKind::Gosub, frame);
        }
        let last = calls.last_mut().expect("the top level is there");
        last.slot = self.slot;
        last.line = self.line();
        calls
    }

    /// Adds a call made from `slot`, returning to `ret`: the caller gets
    /// the call's line.
    fn called_from(
        &self,
        calls: &mut Vec<Call>,
        slot: usize,
        ret: usize,
        kind: CallKind,
        frame: Option<usize>,
    ) {
        // The call is the instruction before the one it returns to.
        let line = (self.program_in(slot))
            .and_then(|p| p.lines.get(ret.wrapping_sub(1)).copied())
            .unwrap_or(0);
        let caller = calls.last_mut().expect("the top level is there");
        (caller.slot, caller.line) = (slot, line);
        calls.push(Call {
            slot,
            line: 0,
            kind,
            frame,
        });
    }

    /// The locals of a DEF call under way, by [`Call::frame`], with
    /// their values.
    pub fn frame_locals(&self, frame: usize) -> Vec<(String, Value)> {
        let Some(f) = self.frames.get(frame) else {
            return Vec::new();
        };
        let Some(def) = self
            .program_in(f.home)
            .and_then(|p| p.defs.get(f.def as usize))
        else {
            return Vec::new();
        };
        (def.locals.iter())
            .zip(&f.locals)
            .filter(|(info, _)| !info.name.starts_with('\0'))
            .map(|(info, value)| (info.full_name(), value.clone()))
            .collect()
    }

    /// The globals of a slot's program that has run, with their values.
    pub fn slot_globals(&self, slot: usize) -> Vec<(String, Value)> {
        let (program, values) = if slot == self.slot {
            (&self.program, &self.globals)
        } else {
            match self.units.get(slot) {
                Some(Some(unit)) => (&unit.program, &unit.globals),
                _ => return Vec::new(),
            }
        };
        (program.globals.iter())
            .zip(values)
            .filter(|(info, _)| !info.name.starts_with('\0'))
            .map(|(info, value)| (info.full_name(), value.clone()))
            .collect()
    }

    /// Where the running code was called from, outermost first: the slot
    /// and line of each GOSUB and DEF call under way, as BACKTRACE lists
    /// them.
    pub fn call_sites(&self) -> Vec<(usize, u32)> {
        let calls = self.calls();
        let n = calls.len() - 1;
        calls[..n].iter().map(|c| (c.slot, c.line)).collect()
    }

    /// The message of the STOP that stopped the program, once.
    pub fn take_stop_message(&mut self) -> Option<Str> {
        self.stop_message.take()
    }

    /// Gives this slot's globals the values of the same names, as direct
    /// mode left them before CONT.
    pub fn set_globals(&mut self, values: &HashMap<String, Value>) {
        for (var, global) in self.program.globals.iter().zip(&mut self.globals) {
            if let Some(v) = values.get(&var.full_name()) {
                *global = v.clone();
            }
        }
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

    /// The + button: jumps to ON BREAK GOTO's label, which is then
    /// forgotten. False when no label is set, and the program should stop.
    pub fn take_break(&mut self, host: &mut dyn Host) -> bool {
        let input = self.reading_input;
        if !self.jump_on_break(host) {
            return false;
        }
        // Out of INPUT, the press is acted on again once the statement
        // there has run: it stops the program, or jumps to a label set
        // meanwhile (checked on hardware).
        if input {
            self.break_pending = Some((self.executed, self.line()));
        }
        true
    }

    /// Jumps to ON BREAK GOTO's label, if one is set, and forgets it.
    fn jump_on_break(&mut self, host: &mut dyn Host) -> bool {
        let Some((slot, addr)) = self.on_break.take() else {
            return false;
        };
        // The jump leaves GOSUBs behind (checked on hardware). UNVERIFIED:
        // that DEFs and callbacks go too.
        self.stack.clear();
        self.frames.clear();
        self.gosubs.clear();
        self.data_kept.clear();
        self.callbacks.clear();
        self.reading_input = false;
        self.switch_to(slot, host);
        self.pc = addr;
        true
    }

    /// Executes up to `budget` instructions.
    pub fn run(&mut self, host: &mut dyn Host, budget: u32) -> Result<Status> {
        let status = self.run_code(host, budget)?;
        // Where it stopped, or an instruction to be run again.
        if matches!(
            status,
            Status::Break { .. }
                | Status::Stop { .. }
                | Status::Traced { .. }
                | Status::Debug { .. }
                | Status::Retry
        ) {
            self.pass_here();
        }
        Ok(status)
    }

    fn run_code(&mut self, host: &mut dyn Host, budget: u32) -> Result<Status> {
        let mut passed = self.passed.take();
        if self.ended {
            return Ok(Status::Ended);
        }
        let stop = self.executed + u64::from(budget);
        let mut program = Rc::clone(&self.program);
        while self.executed < stop {
            if self.break_pending.is_none()
                && self.trace.is_none()
                && !self
                    .stop_at
                    .as_ref()
                    .is_some_and(|at| at.get(self.pc) == Some(&true))
                && let Some(op) = program.code.get(self.pc)
            {
                // The commonest instructions, run here without the rest of
                // step's machinery; they are what most loops are made of.
                let pc = self.pc;
                let mut next = pc + 1;
                let done: Result<bool> = match op {
                    Op::Int(n) => {
                        self.stack.push(Value::Int(*n));
                        Ok(true)
                    }
                    Op::Real(x) => {
                        self.stack.push(Value::Real(*x));
                        Ok(true)
                    }
                    Op::Load(slot) => {
                        let v = self.load(*slot);
                        self.stack.push(v);
                        Ok(true)
                    }
                    Op::Store(slot) => {
                        let v = self.pop();
                        self.store(*slot, v);
                        Ok(true)
                    }
                    Op::Swap => {
                        let n = self.stack.len();
                        self.stack.swap(n - 1, n - 2);
                        Ok(true)
                    }
                    Op::Binary(op) => {
                        let rhs = self.pop();
                        let lhs = self
                            .stack
                            .last_mut()
                            .expect("compiler keeps the stack balanced");
                        ops::binary(*op, lhs, &rhs).map(|v| {
                            *lhs = v;
                            true
                        })
                    }
                    Op::Jump(a) => {
                        next = *a as usize;
                        Ok(true)
                    }
                    Op::JumpIfFalse(a) => self.pop().is_true().map(|t| {
                        if !t {
                            next = *a as usize;
                        }
                        true
                    }),
                    Op::Nop => Ok(true),
                    _ => Ok(false),
                };
                match done {
                    Ok(true) => {
                        self.executed += 1;
                        self.pc = next;
                        continue;
                    }
                    Ok(false) => {}
                    Err(e) => return Err(self.fail(pc, e)),
                }
            }
            if let Some((at, line)) = self.break_pending {
                if self.executed > at && self.program.starts.get(self.pc) == Some(&true) {
                    self.break_pending = None;
                    if !self.jump_on_break(host) {
                        return Ok(Status::Break { line });
                    }
                } else {
                    self.break_pending = Some((at, self.line()));
                }
            }
            let (depth, executed) = (self.depth(), self.executed);
            if let Some(trace) = &mut self.trace
                && self.program.starts.get(self.pc) == Some(&true)
            {
                match (trace.depth, trace.line) {
                    (None, Some(line)) => {
                        self.trace = None;
                        return Ok(Status::Traced { line });
                    }
                    (None, None) => trace.line = self.program.lines.get(self.pc).copied(),
                    (Some(most), _) if executed > trace.from && depth <= most => {
                        self.trace = None;
                        return Ok(Status::Debug { line: self.line() });
                    }
                    (Some(_), _) => {}
                }
            }
            // The first breakpoint looked at is passed over if the program
            // goes on from it.
            if let Some(at) = &self.stop_at
                && at.get(self.pc) == Some(&true)
                && passed.take() != Some((self.slot, self.pc))
            {
                self.trace = None;
                if self.run_to.is_some() {
                    self.run_to(None);
                }
                return Ok(Status::Debug { line: self.line() });
            }
            let pc = self.pc;
            self.executed += 1;
            let outcome = self.step(&program, host);
            // Calls into other slots change the running program.
            if !Rc::ptr_eq(&program, &self.program) {
                program = Rc::clone(&self.program);
            }
            match outcome {
                Ok(None) => {}
                Ok(Some(status)) => {
                    if status == Status::Ended {
                        self.ended = true;
                    }
                    return Ok(status);
                }
                Err(e) => return Err(self.fail(pc, e)),
            }
        }
        Ok(Status::Running)
    }

    /// Ends the program with an error at instruction `pc`.
    #[cold]
    fn fail(&mut self, pc: usize, e: Error) -> Error {
        self.ended = true;
        let line = self.program.lines.get(pc).copied().unwrap_or(0);
        let col = self.program.cols.get(pc).copied().unwrap_or(0);
        let slot = self.slot().map(|s| s as u32);
        e.at_pos(line, col).in_slot(slot)
    }

    #[inline]
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

    /// The `n` indices on top of the stack, taken off it. Arrays have at
    /// most four dimensions, so they fit without allocating.
    fn indices(&mut self, n: u8) -> Result<Indices> {
        let n = usize::from(n);
        let at = self.stack.len() - n;
        let mut idx = Indices {
            buf: [0; 4],
            len: n,
            more: Vec::new(),
        };
        let taken: Result<()> = match n {
            0..=4 => (idx.buf.iter_mut().zip(&self.stack[at..]))
                .try_for_each(|(i, v)| v.to_int().map(|v| *i = v)),
            _ => self.stack[at..]
                .iter()
                .map(Value::to_int)
                .collect::<Result<Vec<_>>>()
                .map(|v| idx.more = v),
        };
        self.stack.truncate(at);
        taken.map(|()| idx)
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

    /// CHKLABEL: whether a label exists where GOTO would look for it.
    /// In a DEF, `outer` also looks at the top level.
    fn has_label(&self, value: &Value, outer: bool) -> Result<bool> {
        let text = value.clone().into_str()?;
        let text = text.borrow();
        let name_at = text
            .iter()
            .position(|&c| c == u16::from(b':'))
            .map_or(0, |i| i + 1);
        if text.get(name_at) != Some(&u16::from(b'@')) {
            return Ok(false);
        }
        let (slot, name) = slot_and_name(value)?;
        Ok(match slot {
            // With a slot, the top level of that slot's program, also in
            // a DEF of its own: FINITI looks for its own models so.
            Some(slot) if slot == self.slot => self.program.labels.contains_key(&name),
            // UNVERIFIED: a slot that EXEC has not run reports 0 rather
            // than an error.
            Some(slot) => (self.program_in(slot)).is_some_and(|p| p.labels.contains_key(&name)),
            None => {
                self.labels().contains_key(&name)
                    || (outer && self.program.labels.contains_key(&name))
            }
        })
    }

    /// FREEMEM: the memory left after every value the program can reach,
    /// in every slot.
    fn free_memory(&self, host: &dyn Host) -> i32 {
        let mut tally = crate::memory::Tally::default();
        for array in host.own_arrays() {
            tally.skip(&array);
        }
        let frames = self
            .frames
            .iter()
            .flat_map(|f| f.locals.iter().chain(&f.args).chain(&f.outs));
        let units = self.units.iter().flatten().flat_map(|u| &u.globals);
        for v in self
            .globals
            .iter()
            .chain(&self.stack)
            .chain(frames)
            .chain(units)
        {
            tally.add(v);
        }
        tally.free()
    }

    /// CHKCALL: whether CALL could call the name, as a DEF of this slot,
    /// a COMMON DEF of another, or a command or function of the system.
    // UNVERIFIED: reserved statements such as PRINT and GOTO report 0.
    fn has_call(&self, value: &Value, host: &dyn Host) -> Result<bool> {
        let (slot, name) = slot_and_name(value)?;
        if let Some(slot) = slot.filter(|&s| s != self.slot) {
            return Ok((self.program_in(slot)).is_some_and(|p| p.def_names.contains_key(&name)));
        }
        if NOT_CALLS.contains(&name.as_str()) {
            return Ok(false);
        }
        Ok(self.program.def_names.contains_key(&name)
            || self.find_common(&name).is_some()
            || builtins::lookup(&name, Form::Function).is_some()
            || builtins::lookup(&name, Form::Command).is_some()
            || host.lookup(&name).is_some()
            || VM_CALLS.contains(&name.as_str()))
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
            data_from: self.data_from.take(),
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
        self.data_from = unit.data_from;
        self.slot = slot;
        self.update_stops();
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
        // A slot with code that may still run: the one running, one EXEC
        // goes back to, or one a DEF or GOSUB returns to.
        let running = |s: i32| {
            let s = s as usize;
            s == self.slot
                || self.execs.iter().any(|r| r.slot == s)
                || self.frames.iter().any(|f| f.caller == s || f.home == s)
                || self.gosubs.iter().any(|g| g.0 == s)
        };
        // EXEC of a slot must not start one that is running, but the slot
        // running now may be started again, from its file or as it is
        // (checked on hardware: EXEC 0 in a DEF of slot 1 called from slot
        // 0 is Can't use the specified slot).
        let own = slot == self.slot as i32;
        if running(slot) && !own {
            return Err(Error::with_detail(
                ErrorKind::CantUseSpecifiedSlot,
                format!("slot {slot}"),
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
        let program =
            crate::compile_in(&source, &*host, slot).map_err(|e| e.in_slot(Some(slot as u32)))?;
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
        let text = target.clone().into_str()?;
        let is_label = {
            let text = text.borrow();
            let name_at = text
                .iter()
                .position(|&c| c == u16::from(b':'))
                .map_or(0, |i| i + 1);
            text.get(name_at) == Some(&u16::from(b'@'))
        };
        if is_label {
            let (slot, label) = self.find_label(target)?;
            return self.gosub(slot, label, host);
        }
        // A DEF of the slot the callback was set in, or a COMMON DEF.
        let (slot, name) = slot_and_name(target)?;
        let slot = slot.unwrap_or(self.slot);
        let own = match slot == self.slot {
            true => Some(program),
            false => self.program_in(slot).map(|p| &**p),
        };
        if let Some(&def) = own.and_then(|p| p.def_names.get(&name)) {
            return match slot == self.slot {
                true => self.enter_def(def, Vec::new(), Want::Outs(0), self.slot),
                false => self.call_in(slot, def, Vec::new(), Want::Outs(0), host),
            };
        }
        let (slot, def) = self
            .find_common(&name)
            .ok_or_else(|| Error::with_detail(ErrorKind::UndefinedFunction, name))?;
        self.call_in(slot, def, Vec::new(), Want::Outs(0), host)
    }

    /// RESTORE "PUSH", "POP", "BEGIN" and "n:BEGIN" (SmileBASIC 4.4):
    /// whether `target` was one of them.
    fn restore_special(&mut self, target: &Value) -> Result<bool> {
        let Value::Str(s) = target else {
            return Ok(false);
        };
        let s = String::from_utf16_lossy(&s.borrow()).to_ascii_uppercase();
        match s.as_str() {
            "PUSH" => {
                // UNVERIFIED: how many places it keeps; 1000 were fine.
                if self.data_kept.len() >= MAX_DATA_KEPT {
                    return Err(ErrorKind::StackOverflow.into());
                }
                self.data_kept.push((self.data_pos, self.data_from.clone()));
            }
            "POP" => {
                (self.data_pos, self.data_from) =
                    self.data_kept.pop().ok_or(ErrorKind::StackUnderflow)?;
            }
            "BEGIN" => (self.data_pos, self.data_from) = (0, None),
            _ => {
                let Some((slot, "BEGIN")) = s.split_once(':') else {
                    return Ok(false);
                };
                let slot: usize = slot.trim().parse().map_err(|_| {
                    Error::with_detail(ErrorKind::IllegalFunctionCall, format!("slot {slot:?}"))
                })?;
                // UNVERIFIED: another slot's READ place is its own, used
                // once the program goes on there.
                match self.units.get_mut(slot) {
                    _ if slot == self.slot => self.data_pos = 0,
                    Some(Some(unit)) => unit.data_pos = 0,
                    _ => {
                        return Err(Error::with_detail(
                            ErrorKind::OutOfRange,
                            format!("slot {slot}"),
                        ));
                    }
                }
            }
        }
        Ok(true)
    }

    fn gosub(&mut self, slot: usize, target: usize, host: &mut dyn Host) -> Result<()> {
        if self.gosubs.len() >= MAX_GOSUB_DEPTH {
            return Err(ErrorKind::StackOverflow.into());
        }
        self.gosubs
            .push((self.slot, self.pc + 1, self.data_kept.len()));
        self.switch_to(slot, host);
        self.pc = target;
        Ok(())
    }

    /// Runs one instruction. `Some` stops the current slice.
    /// Runs the instruction at the program counter; `program` is the
    /// running program, which the caller keeps rather than this cloning
    /// it for every instruction.
    fn step(&mut self, program: &Rc<Compiled>, host: &mut dyn Host) -> Result<Option<Status>> {
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
                let lhs = self
                    .stack
                    .last_mut()
                    .expect("compiler keeps the stack balanced");
                *lhs = ops::binary(*op, lhs, &rhs)?;
            }
            Op::Append => {
                let rhs = self.pop();
                let lhs = self
                    .stack
                    .last_mut()
                    .expect("compiler keeps the stack balanced");
                match (&*lhs, &rhs) {
                    (Value::Str(l), Value::Str(r)) => {
                        let added = r.borrow().clone();
                        l.borrow_mut().extend_from_slice(&added);
                    }
                    _ => *lhs = ops::binary(BinaryOp::Add, lhs, &rhs)?,
                }
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
                let from = self.stack.len() - *n as usize;
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
            Op::OnBreak(a) => self.on_break = Some((self.slot, *a as usize)),
            Op::NoBreak => self.on_break = None,
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
                        self.gosubs.push((self.slot, after, self.data_kept.len()));
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
                // Checked on hardware: a name found nowhere is a Syntax error.
                let (slot, def) = self
                    .find_common(&name)
                    .ok_or_else(|| Error::with_detail(ErrorKind::SyntaxError, name))?;
                let args = self.pop_n((*argc).into());
                self.call_in(slot, def, args, *want, host)?;
                return Ok(None);
            }
            Op::CallBuiltin { id, argc, want } => {
                let args = self.pop_n((*argc).into());
                self.call_builtin(program, *id, &args, *want)?;
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
                    self.call_builtin(program, id, &args, *want)?;
                } else if *want == Want::Outs(1)
                    && let Some(id) = builtins::lookup(&name, Form::Function)
                {
                    self.call_builtin(program, id, &args, Want::Value)?;
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
                let target = self.pop();
                let v = match self.other_global(&target)? {
                    Some(v) => v.clone(),
                    None => {
                        let slot = self.dynamic_slot(&name_of(&target)?)?;
                        self.load(slot)
                    }
                };
                self.stack.push(v);
            }
            Op::StoreDyn => {
                let target = self.pop();
                let v = self.pop();
                match self.other_global(&target)? {
                    Some(place) => *place = v,
                    None => {
                        let slot = self.dynamic_slot(&name_of(&target)?)?;
                        self.store(slot, v);
                    }
                }
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
                        self.run_callback(program, &Value::Str(new_str(target)), host)?;
                        return Ok(None);
                    }
                }
            }
            Op::CallIdx => self.stack.push(Value::Int(self.call_idx)),
            Op::ChkLabel { has_flag } => {
                let outer = match has_flag {
                    true => self.pop().to_int()? != 0,
                    false => false,
                };
                let name = self.pop();
                let found = self.has_label(&name, outer)?;
                self.stack.push(Value::Int(found.into()));
            }
            Op::ChkCall => {
                let name = self.pop();
                let found = self.has_call(&name, host)?;
                self.stack.push(Value::Int(found.into()));
            }
            Op::ChkVar => {
                let target = self.pop();
                let name = name_of(&target)?;
                // Another slot's, as VAR("n:NAME") would find it.
                let found = match self.other_global(&target) {
                    Ok(Some(_)) => true,
                    Ok(None) => self.dynamic_slot(&name).is_ok(),
                    Err(_) => false,
                };
                self.stack.push(Value::Int(found.into()));
            }
            Op::FreeMem { both } => {
                let free = Value::Int(self.free_memory(host));
                // The largest free block is all of it (checked on
                // hardware).
                if *both {
                    self.stack.push(free.clone());
                }
                self.stack.push(free);
            }
            Op::Stop { has_message } => {
                let message = match has_message {
                    true => Some(text_of(&self.pop())?),
                    false => None,
                };
                if !self.in_slot {
                    return Err(ErrorKind::CantUseFromDirectMode.into());
                }
                // UNVERIFIED: how 255 characters are kept to.
                self.stop_message = message.map(|mut m| {
                    m.truncate(STOP_MESSAGE);
                    m
                });
                let line = self.line();
                self.pc = next;
                return Ok(Some(Status::Stop { line }));
            }
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
                    // Checked on hardware: a space follows the question mark.
                    if *question {
                        host.print(None, &[u16::from(b'?'), u16::from(b' ')])?;
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
                        // The line is typed again on the next line, without
                        // the prompt (checked on hardware).
                        let redo: Vec<u16> = "?Redo from start".encode_utf16().collect();
                        host.print(None, &redo)?;
                        host.print_newline(None)?;
                        host.input_start()?;
                        self.reading_input = true;
                        return Ok(Some(RETRY));
                    }
                }
            }
            Op::Restore(pos) => (self.data_pos, self.data_from) = (*pos as usize, None),
            Op::RestoreDyn => {
                let target = self.pop();
                if !self.restore_special(&target)? {
                    // "n:@LABEL" sends READ to another slot's DATA, as
                    // FINITI does to read models from the program that
                    // calls it.
                    // UNVERIFIED: that a slot EXEC has not run gives
                    // Undefined label.
                    let (slot, name) = slot_and_name(&target)?;
                    let from = slot
                        .filter(|&s| s != self.slot)
                        .map(|s| self.program_in(s).cloned());
                    let undefined =
                        || Error::with_detail(ErrorKind::UndefinedLabel, format!("@{name}"));
                    let from = match from {
                        Some(None) => return Err(undefined()),
                        Some(Some(p)) => Some(p),
                        None => None,
                    };
                    let source = from.as_deref().unwrap_or(program);
                    let pos = *source.data_labels.get(&name).ok_or_else(undefined)?;
                    (self.data_pos, self.data_from) = (pos as usize, from);
                }
            }
            Op::Read => {
                let source = self.data_from.as_deref().unwrap_or(program);
                let item = source.data.get(self.data_pos).ok_or(ErrorKind::OutOfData)?;
                self.data_pos += 1;
                self.stack.push(match item {
                    Literal::Int(n) => Value::Int(*n),
                    Literal::Real(x) => Value::Real(*x),
                    Literal::Str(s) => Value::Str(new_str(s.clone())),
                });
            }
            Op::End => return Ok(self.end(host)),
            Op::Nop => {}
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

    /// VAR("n:NAME") of another slot that EXEC has run: its global, or
    /// `None` for a name without another slot.
    // UNVERIFIED: the error for a slot EXEC has not run.
    fn other_global(&mut self, target: &Value) -> Result<Option<&mut Value>> {
        let (slot, name) = slot_and_name(target)?;
        let Some(slot) = slot.filter(|&s| s != self.slot) else {
            return Ok(None);
        };
        let undefined =
            || Error::with_detail(ErrorKind::UndefinedVariable, format!("{slot}:{name}"));
        let unit = self
            .units
            .get_mut(slot)
            .and_then(Option::as_mut)
            .ok_or_else(undefined)?;
        let i = (unit.program.globals.iter())
            .position(|v| v.full_name() == name)
            .ok_or_else(undefined)?;
        Ok(Some(&mut unit.globals[i]))
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
        if want == Want::Outs(1)
            && !b.accepts(args.len())
            && let Some(f) = builtins::lookup(b.name, Form::Function)
            && builtins::get(f).accepts(args.len())
        {
            // `HSV H,S,V OUT C`: arguments that only the function form
            // takes, with one OUT variable for its result.
            return self.call_builtin(program, f, args, Want::Value);
        }
        if !b.accepts(args.len()) {
            // Arguments that fit the other form make the results the
            // wrong ones: HSV(120) is read as HSV's OUT form (checked on
            // hardware).
            let other = match b.form() {
                Form::Function => Form::Command,
                Form::Command => Form::Function,
            };
            let fits_other = builtins::lookup(b.name, other)
                .is_some_and(|o| builtins::get(o).accepts(args.len()));
            let kind = match fits_other {
                true => ErrorKind::WrongNumberOfOutArguments,
                false => ErrorKind::WrongNumberOfArguments,
            };
            return Err(Error::with_detail(kind, b.name));
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
            // A DEF with one OUT may be called as a function, giving it.
            Want::Value => info.is_function || info.outs.is_none_or(|m| m > 0),
            // A function may be called with one OUT for its value.
            Want::Outs(n) => match info.is_function {
                true => n == 1,
                false => info.outs.is_none_or(|m| m == n),
            },
        };
        let args_ok = info.params.is_none_or(|p| usize::from(p) == args.len());
        // UNVERIFIED: that a DEF given the wrong number of arguments is
        // the same error as a builtin.
        // Several OUTs for one value is the wrong number of them.
        let too_many_outs = want == Want::Value && info.outs.is_some_and(|m| m > 1);
        if !call_ok || !args_ok || too_many_outs {
            let kind = match (call_ok, too_many_outs) {
                (_, true) => ErrorKind::WrongNumberOfOutArguments,
                (true, false) => ErrorKind::WrongNumberOfArguments,
                (false, false) => ErrorKind::IllegalFunctionCall,
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
        // OUT variables hold nothing until set: a DEF that never sets
        // one gives the omitted value, as SmileBASIC Debugger's EMPTY()
        // relies on.
        let first_out = usize::from(info.params.unwrap_or(0));
        let out_count = usize::from(info.outs.unwrap_or(0));
        for v in locals.iter_mut().skip(first_out).take(out_count) {
            *v = Value::Default;
        }
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
            let (slot, pc, kept) = *self.gosubs.last().expect("length checked");
            // Places RESTORE "PUSH" kept in the subroutine must be taken
            // back first (checked on hardware).
            if self.data_kept.len() > kept {
                return Err(ErrorKind::UseRestorePop.into());
            }
            self.gosubs.pop();
            self.switch_to(slot, host);
            // TRACE names the line of the GOSUB it came back to (checked
            // on hardware).
            if let Some(Trace {
                line: Some(line), ..
            }) = &mut self.trace
            {
                *line = self
                    .program
                    .lines
                    .get(pc.wrapping_sub(1))
                    .copied()
                    .unwrap_or(*line);
            }
            return Ok(pc);
        }
        let Some(frame) = self.frames.last() else {
            return Err(ErrorKind::ReturnWithoutGosub.into());
        };
        let info = &self.program.defs[frame.def as usize];
        let mut results = match (info.is_function, info.outs) {
            // UNVERIFIED: a bare RETURN in a function gives no value.
            (true, _) => vec![Value::Default],
            (false, Some(n)) => {
                let start = usize::from(info.params.unwrap_or(0));
                frame.locals[start..start + usize::from(n)].to_vec()
            }
            (false, None) => frame.outs.clone(),
        };
        if frame.want == Want::Value {
            results.truncate(1);
        }
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

/// Indices popped by [`Vm::indices`].
struct Indices {
    buf: [i32; 4],
    len: usize,
    more: Vec<i32>,
}

impl std::ops::Deref for Indices {
    type Target = [i32];
    fn deref(&self) -> &[i32] {
        match self.len {
            0..=4 => &self.buf[..self.len],
            _ => &self.more,
        }
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
