//! Bytecode interpreter.
//!
//! The VM runs in slices: [`Vm::run`] executes up to a budget of
//! instructions and returns, so the frontend can keep drawing frames.
//! VSYNC and WAIT return [`Status::Wait`] and resume on the next call.

use std::collections::HashMap;
use std::rc::Rc;

use crate::ast::{BinaryOp, UnaryOp};
use crate::builtins::{self, Form, Kind};
use crate::bytecode::*;
use crate::error::{Error, ErrorKind, Result};
use crate::fuse::{FOR_TEST_LEN, FastOp, Micro, Selector, Var};
use crate::host::{Exec, Host};
use crate::lexer::Suffix;
use crate::memory;
use crate::ops;
use crate::value::{Array, ArrayData, ElemType, Num, Str, Value, format_number, new_str};
use crate::watch::Watch;

/// The stack GOSUBs, DEF calls and RESTORE "PUSH" share, in half slots: 259072
/// slots. The values of expressions share it too, which is not counted: their
/// room is left aside, as two values take it.
const STACK_SIZE: usize = 518_136;
/// What is left aside for those values. A DEF call that reaches into it fails on
/// what follows it, a call that goes past it on the call itself. GOSUB and
/// RESTORE "PUSH" may fill it, and then the next command given arguments fails.
const VALUES_ROOM: usize = 8;
/// All of the stack, the values' room too.
const STACK_FULL: usize = STACK_SIZE + VALUES_ROOM;
/// What a GOSUB takes of it.
const GOSUB_COST: usize = 2;
/// What a place RESTORE "PUSH" keeps takes of it.
const RESTORE_COST: usize = 3;
/// What a DEF's call takes of it, without its variables.
const CALL_COST: usize = 12;
/// What a function's call takes on top, for its value.
const VALUE_COST: usize = 4;
/// What each of a DEF's arguments, OUTs and variables takes of it.
const VARIABLE_COST: usize = 4;

/// Counts of the work a [`Vm`] has done, from [`Vm::work`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Work {
    pub executed: u64,
    pub statements: u64,
    pub def_calls: u64,
    pub builtin_calls: u64,
    /// The arguments given to builtin calls.
    pub builtin_args: u64,
    /// Array elements read or written, and their indices past the first.
    pub elements: u64,
    pub extra_indices: u64,
    /// Of those elements, characters of a string read or written by index.
    pub str_elements: u64,
    /// Elements `FILL` wrote.
    pub filled: u64,
    /// Variables, labels and DEFs found by a name in a string at run time.
    pub lookups: u64,
    /// Strings joined.
    pub joins: u64,
    /// Tests of whether a FOR goes round again: each pass and the end.
    pub for_tests: u64,
    /// Of those, the tests run inside a DEF.
    pub def_for_tests: u64,
    /// `IF` and `ELSEIF` conditions that are an array element
    /// ([`crate::bytecode::Op::TestIfFalse`]).
    pub element_tests: u64,
    /// Calls of `CHR$` of a code other than the last call's, counted
    /// only for a host that hears of the work.
    pub chr_calls: u64,
    /// The calls of each builtin of [`COSTLY`], counted only for a host
    /// that hears of the work, as the rest of these.
    pub costly_calls: [u64; COSTLY.len()],
    /// The calls of each builtin of [`CHEAP`], counted as the rest of
    /// these, and those of `SIN`, `COS` and `TAN` of a number past a
    /// quarter of pi either way.
    pub cheap_calls: [u64; CHEAP.len()],
    pub trig_reductions: u64,
    /// The characters of the strings joins made.
    pub join_chars: u64,
    /// Comparisons and other operators but `+` with a string on the left.
    pub str_compares: u64,
}

/// Builtins that cost SmileBASIC 4 more than a call, timed alone by
/// TEST71 on both consoles. [`Work::costly_calls`] counts their calls in
/// this order.
pub const COSTLY: [&str; 6] = ["STR$", "FORMAT$", "VAL", "INSTR", "HEX$", "MID$"];

/// Builtins that cost SmileBASIC 4 about what reading a variable does,
/// or a little more, each timed alone by TEST71, TEST72 and TEST74 on
/// both consoles. [`Work::cheap_calls`] counts their calls in this order.
/// The first three cost more for a number past a quarter of pi either way
/// (TEST72's `SIN` of 1000000.5). UNVERIFIED: `TAN` as `SIN`, and where
/// between 0.5 and 1000000.5 that cost starts.
pub const CHEAP: [&str; 4] = ["SIN", "COS", "TAN", "SQR"];

/// Builtins that cost SmileBASIC 4 no more than reading a variable, so
/// that it seems to run them without a call (TEST69 to TEST71, both
/// consoles), and LEN (TEST71, TEST74). MIN and MAX of two count among
/// them too. UNVERIFIED: that MAX of two costs what MIN of two does.
const FREE: [&str; 5] = ["ASC", "INT", "FLOOR", "ABS", "LEN"];

/// How the work of a builtin's call is counted.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Charge {
    Call,
    Free,
    FreeWithTwo,
    Chr,
    Costly(u8),
    Cheap(u8),
}

/// How a call of each builtin is counted, by its number. Made once, so
/// that a run that reports its work compares no names.
fn charges() -> &'static [Charge] {
    static TABLE: std::sync::OnceLock<Vec<Charge>> = std::sync::OnceLock::new();
    fn set(table: &mut Vec<Charge>, name: &str, charge: Charge) {
        if let Some(id) = builtins::lookup(name, Form::Function) {
            let i = usize::from(id);
            if table.len() <= i {
                table.resize(i + 1, Charge::Call);
            }
            table[i] = charge;
        }
    }
    TABLE.get_or_init(|| {
        let mut t = Vec::new();
        for name in FREE {
            set(&mut t, name, Charge::Free);
        }
        set(&mut t, "MIN", Charge::FreeWithTwo);
        set(&mut t, "MAX", Charge::FreeWithTwo);
        set(&mut t, "CHR$", Charge::Chr);
        for (i, name) in COSTLY.iter().enumerate() {
            set(&mut t, name, Charge::Costly(i as u8));
        }
        for (i, name) in CHEAP.iter().enumerate() {
            set(&mut t, name, Charge::Cheap(i as u8));
        }
        t
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    /// The instruction budget ran out, or the host's time
    /// ([`crate::host::Host::out_of_time`]); call [`Vm::run`] again.
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

/// Whether `name` is one of the system's commands or functions, as
/// CHKCALL counts them.
pub(crate) fn is_system_call(name: &str, host: &dyn Host) -> bool {
    !NOT_CALLS.contains(&name)
        && (builtins::lookup(name, Form::Function).is_some()
            || builtins::lookup(name, Form::Command).is_some()
            || host.lookup(name).is_some()
            || VM_CALLS.contains(&name))
}

/// The characters of STOP's message that are shown.
const STOP_MESSAGE: usize = 255;

/// Waits a frame and runs the same instruction again.
const RETRY: Status = Status::Retry;

/// A READ place RESTORE "PUSH" kept, with the numbers of DEF calls and
/// GOSUBs then.
struct Kept {
    pos: usize,
    from: Rc<Compiled>,
    calls: usize,
    gosubs: usize,
}

struct Frame {
    def: u32,
    /// The slot the DEF is in, and the one to return to.
    home: usize,
    caller: usize,
    /// Where its variables start in [`Vm::locals`]. They go on to the next
    /// frame's, or to the end.
    base: usize,
    /// Arguments of a `*` DEF.
    args: Vec<Value>,
    /// Results of an `OUT *` DEF.
    outs: Vec<Value>,
    want: Want,
    return_pc: usize,
    gosub_base: usize,
    stack_base: usize,
    /// How much of [`STACK_SIZE`] the DEF calls so far take, this one too.
    stack_to: usize,
}

/// A program slot's program with its variables. The VM keeps the running
/// slot's in its own fields, and the others here.
struct Unit {
    program: Rc<Compiled>,
    globals: Vec<Value>,
    declared: Vec<bool>,
}

impl Unit {
    fn new(program: Compiled) -> Unit {
        let globals = (program.globals.iter())
            .map(initial_value)
            .chain(program.constants.iter().map(constant_value))
            .collect();
        Unit {
            declared: vec![false; program.globals.len()],
            program: Rc::new(program),
            globals,
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

/// A line that stops the program before it runs, when its condition, if
/// it has one, is true.
pub type Breakpoint = (u32, Option<Rc<Watch>>);

/// A watch that stops the program when its value changes, with the value
/// it had last, when it could be worked out.
struct Change {
    watch: Rc<Watch>,
    last: Option<Value>,
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
    /// READ's place when EXEC started the slot.
    data_pos: usize,
    data_from: Rc<Compiled>,
}

pub struct Vm {
    /// This interpreter's share of [`memory`]'s estimate between slices.
    memory: memory::Estimate,
    /// `executed` when memory was last counted for an instruction to run
    /// again; it asking twice means there is no room after all.
    memory_checked: Option<u64>,
    program: Rc<Compiled>,
    pc: usize,
    stack: Vec<Value>,
    globals: Vec<Value>,
    /// Which globals exist, for direct mode's declarations.
    declared: Vec<bool>,
    frames: Vec<Frame>,
    /// The variables of the DEF calls under way, each call's after its
    /// caller's, so that a call and its return move no lists about.
    locals: Vec<Value>,
    /// Slot and address to return to, and how many READ places
    /// RESTORE "PUSH" had kept then.
    gosubs: Vec<(usize, usize, usize)>,
    /// READ's place: one for the interpreter, kept when a call goes on
    /// in another slot, so that a READ in another slot's COMMON DEF reads
    /// the DATA of the program that called it.
    data_pos: usize,
    /// The program whose DATA READ reads: the one RUN started, or the one
    /// RESTORE last named.
    data_from: Rc<Compiled>,
    /// READ places kept by RESTORE "PUSH".
    data_kept: Vec<Kept>,
    /// Whether a GOSUB or RESTORE "PUSH" has reached into the values' room.
    /// Until one has, commands need not look at the room; after, it stays
    /// set until ON BREAK empties the stack, and they look each time.
    values_room_reached: bool,
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
    /// What [`Vm::work`] counts. Only a run that reports it counts
    /// statements, as that takes the fused steps away.
    statements: u64,
    def_calls: u64,
    builtin_calls: u64,
    builtin_args: u64,
    elements: u64,
    extra_indices: u64,
    str_elements: u64,
    filled: u64,
    lookups: u64,
    joins: u64,
    for_tests: u64,
    def_for_tests: u64,
    element_tests: u64,
    chr_calls: u64,
    /// The code of the last `CHR$` counted, as a run that reports its
    /// work sees it.
    last_chr: Option<i32>,
    costly_calls: [u64; COSTLY.len()],
    cheap_calls: [u64; CHEAP.len()],
    trig_reductions: u64,
    join_chars: u64,
    str_compares: u64,
    /// Whether the host hears of [`Vm::work`] before each of its calls,
    /// and then [`charges`].
    report_work: bool,
    charges: &'static [Charge],
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
    /// Breakpoints, by slot, and the line a debugger runs to once.
    breakpoints: Vec<Vec<Breakpoint>>,
    run_to: Option<(usize, u32)>,
    /// Worked out from them by slot, and the running program's.
    stops: Vec<Option<Stops>>,
    stop_at: Option<Rc<[bool]>>,
    /// The slot and instruction the program goes on from, which a
    /// breakpoint there does not stop it at.
    passed: Option<(usize, usize)>,
    /// Watches that stop the program when their values change, looked at
    /// before each statement.
    changes: Vec<Change>,
    /// Why the debugger last stopped the program.
    debug_note: Option<DebugNote>,
}

/// Why the debugger stopped the program, as [`Vm::take_debug_note`]
/// tells.
#[derive(Clone, Debug)]
pub enum DebugNote {
    /// A step ended.
    Step,
    /// A breakpoint, with its condition when it has one.
    Breakpoint(Option<Rc<Watch>>),
    /// The line a debugger ran to.
    RanTo,
    /// The watch at this place in [`Vm::set_changes`]'s list changed
    /// from one value to the other.
    Changed(usize, Value, Value),
    /// A breakpoint's condition failed, so it stopped.
    Condition(Error),
}

/// Whether two values are alike: the same number, whether integer or
/// real, or the same characters or elements.
fn same(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Default, Value::Default) => true,
        (Value::Int(a), Value::Int(b)) => a == b,
        (Value::Int(_) | Value::Real(_), Value::Int(_) | Value::Real(_)) => {
            let (a, b) = (a.to_real(), b.to_real());
            a.ok().map(f64::to_bits) == b.ok().map(f64::to_bits)
        }
        (Value::Str(a), Value::Str(b)) => *a.borrow() == *b.borrow(),
        (Value::Array(a), Value::Array(b)) => *a.borrow() == *b.borrow(),
        _ => false,
    }
}

/// A copy of a value that changes to the string or array it came from
/// leave alone.
fn snapshot(value: &Value) -> Value {
    match value {
        Value::Str(s) => Value::Str(new_str(s.borrow().clone())),
        Value::Array(a) => {
            let mut copy = a.borrow().clone();
            if let ArrayData::Str(items) = &mut copy.data {
                for s in items {
                    let text = s.borrow().clone();
                    *s = new_str(text);
                }
            }
            Value::Array(Rc::new(std::cell::RefCell::new(copy)))
        }
        v => v.clone(),
    }
}

/// A variable's starting value. The suffix decides only this: variables
/// keep whatever value they are given (checked on hardware: `A%=1E10`
/// keeps the real, `A$` takes a number and `A%` a string).
fn initial_value(var: &VarInfo) -> Value {
    if var.array {
        return Value::Default;
    }
    match var.suffix {
        Some(Suffix::Int) => Value::Int(0),
        Some(Suffix::Real) => Value::Real(0.0),
        Some(Suffix::Str) => Value::Str(new_str(Vec::new())),
        None if var.int => Value::Int(0),
        None => Value::Real(0.0),
    }
}

/// A constant of [`Compiled::constants`] in its place after the globals.
fn constant_value(constant: &Literal) -> Value {
    match constant {
        Literal::Int(n) => Value::Int(*n),
        Literal::Real(x) => Value::Real(*x),
        Literal::Str(s) => Value::Str(new_str(s.clone())),
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
    // Extra fields are left out, whatever they hold.
    if fields.len() < kinds.len() {
        return None;
    }
    fields
        .iter()
        .zip(kinds)
        .map(|(field, kind)| match kind {
            InputKind::Str => Some(Value::Str(new_str(field.to_vec()))),
            // An empty field reads as 0.
            InputKind::Number if field.iter().all(|&c| c == u16::from(b' ')) => Some(Value::Int(0)),
            // Spaces around a number make it ask again (checked on
            // hardware), as VAL reads them as 0.
            InputKind::Number | InputKind::Named(_) => builtins::parse_number(field),
        })
        .collect()
}

/// "@NAME", "NAME" or "1:@NAME" to "NAME".
pub(crate) fn label_name(value: &Value) -> Result<String> {
    let s = String::from_utf16_lossy(&value.clone().into_str()?.borrow()).to_ascii_uppercase();
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

/// The program slots a label string may name.
const LAST_SLOT: usize = 5;

/// A label string for GOTO, GOSUB and RESTORE, `"@NAME"` or `"n:@NAME"`
/// with slot n of 0 to [`LAST_SLOT`], to the slot and `"NAME"`. Anything
/// else is `Illegal label string`.
pub fn label_string(value: &Value) -> Result<(Option<usize>, String)> {
    let text = value.clone().into_str()?;
    let s = String::from_utf16_lossy(&text.borrow()).to_ascii_uppercase();
    let illegal = || Error::with_detail(ErrorKind::IllegalLabelString, format!("{s:?}"));
    let (slot, rest) = match s.split_once(':') {
        Some((slot, rest)) => {
            let slot = slot.trim().parse().ok().filter(|&n| n <= LAST_SLOT);
            (Some(slot.ok_or_else(illegal)?), rest)
        }
        None => (None, s.as_str()),
    };
    let name = rest.strip_prefix('@').ok_or_else(illegal)?;
    Ok((slot, name.to_string()))
}

/// An error in argument `arg` of `command`, named as SmileBASIC 4 does.
#[cold]
fn arg_error(e: Error, arg: usize, command: &str) -> Error {
    e.of_arg(arg).of_command(command)
}

/// The name CHKLABEL, CHKCALL and CHKVAR look for: anything but a string
/// is `Type mismatch` naming it.
#[inline]
fn name_arg(value: &Value, command: &str) -> Result<()> {
    match value {
        Value::Str(_) => Ok(()),
        _ => Err(arg_error(ErrorKind::TypeMismatch.into(), 0, command)),
    }
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
                    .unwrap_or_else(|| initial_value(g))
            })
            .chain(program.constants.iter().map(constant_value))
            .collect();
        let program = Rc::new(program);
        Vm {
            memory: memory::Estimate::default(),
            memory_checked: None,
            data_from: Rc::clone(&program),
            program,
            pc: 0,
            stack: Vec::new(),
            globals,
            declared,
            frames: Vec::new(),
            locals: Vec::new(),
            gosubs: Vec::new(),
            data_pos: 0,
            data_kept: Vec::new(),
            values_room_reached: false,
            slot: 0,
            in_slot: false,
            units: Vec::new(),
            execs: Vec::new(),
            ended: false,
            reading_input: false,
            callbacks: Vec::new(),
            call_idx: 0,
            executed: 0,
            statements: 0,
            def_calls: 0,
            builtin_calls: 0,
            builtin_args: 0,
            elements: 0,
            extra_indices: 0,
            str_elements: 0,
            filled: 0,
            lookups: 0,
            joins: 0,
            for_tests: 0,
            def_for_tests: 0,
            element_tests: 0,
            chr_calls: 0,
            last_chr: None,
            costly_calls: [0; COSTLY.len()],
            cheap_calls: [0; CHEAP.len()],
            trig_reductions: 0,
            join_chars: 0,
            str_compares: 0,
            charges: &[],
            report_work: false,
            on_break: None,
            break_pending: None,
            stop_message: None,
            trace: None,
            breakpoints: Vec::new(),
            run_to: None,
            stops: Vec::new(),
            stop_at: None,
            passed: None,
            changes: Vec::new(),
            debug_note: None,
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
    pub fn set_breakpoints(&mut self, slot: usize, breakpoints: &[Breakpoint]) {
        if self.breakpoints.len() <= slot {
            self.breakpoints.resize_with(slot + 1, Vec::new);
        }
        self.breakpoints[slot] = breakpoints.to_vec();
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
        let mut lines: Vec<u32> = (self.breakpoints.get(slot).into_iter().flatten())
            .map(|b| b.0)
            .collect();
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

    /// Whether a breakpoint the program has come to stops it: the line it
    /// runs to does, and so does a condition that is true or fails.
    fn breakpoint_holds(&mut self) -> bool {
        let line = self.line();
        if self.run_to == Some((self.slot, line)) {
            self.debug_note = Some(DebugNote::RanTo);
            return true;
        }
        let breakpoint =
            (self.breakpoints.get(self.slot).into_iter().flatten()).find(|b| b.0 == line);
        let Some(watch) = breakpoint.and_then(|b| b.1.clone()) else {
            self.debug_note = Some(DebugNote::Breakpoint(None));
            return true;
        };
        match watch
            .eval(&|name| self.var_here(name))
            .and_then(|v| v.is_true())
        {
            Ok(holds) => {
                if holds {
                    self.debug_note = Some(DebugNote::Breakpoint(Some(watch)));
                }
                holds
            }
            Err(e) => {
                self.debug_note = Some(DebugNote::Condition(e));
                true
            }
        }
    }

    /// Sets the watches that stop the program, before the statement
    /// after the one that changed one of their values. Their values now
    /// are what changes are told from.
    pub fn set_changes(&mut self, watches: &[Rc<Watch>]) {
        self.changes = (watches.iter())
            .map(|w| Change {
                watch: Rc::clone(w),
                last: w
                    .eval(&|name| self.var_here(name))
                    .ok()
                    .as_ref()
                    .map(snapshot),
            })
            .collect();
    }

    /// Whether a watch's value changed since it was last looked at,
    /// noting the first that did. A watch that cannot be worked out
    /// here, such as one of another DEF's locals, keeps its last value.
    fn watch_changed(&mut self) -> bool {
        let mut changes = std::mem::take(&mut self.changes);
        let mut changed = None;
        for (i, change) in changes.iter_mut().enumerate() {
            let Ok(value) = change.watch.eval(&|name| self.var_here(name)) else {
                continue;
            };
            match change.last.replace(snapshot(&value)) {
                Some(last) if changed.is_none() && !same(&last, &value) => {
                    changed = Some(DebugNote::Changed(i, last, value));
                }
                _ => {}
            }
        }
        self.changes = changes;
        let stops = changed.is_some();
        if stops {
            self.debug_note = changed;
        }
        stops
    }

    /// Why the debugger stopped the program, beyond its line, once.
    pub fn take_debug_note(&mut self) -> Option<DebugNote> {
        self.debug_note.take()
    }

    /// A variable the running code sees, by full name: a local of the
    /// innermost DEF call, else a global of the running slot.
    fn var_here(&self, name: &str) -> Option<Value> {
        let named = |var: &VarInfo| {
            let suffix = match var.suffix {
                Some(Suffix::Int) => "%",
                Some(Suffix::Real) => "#",
                Some(Suffix::Str) => "$",
                None => "",
            };
            name.strip_prefix(var.name.as_str()) == Some(suffix)
        };
        if let Some(f) = self.frames.last()
            && let Some(def) = (self.program_in(f.home)).and_then(|p| p.defs.get(f.def as usize))
            && let Some(i) = def.locals.iter().position(named)
        {
            return self.locals[f.base..].get(i).cloned();
        }
        let i = self.program.globals.iter().position(named)?;
        self.globals.get(i).cloned()
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
            .zip(self.frame_vars(frame))
            .filter(|(info, _)| !info.name.starts_with('\0'))
            .map(|(info, value)| (info.full_name(), value.clone()))
            .collect()
    }

    /// Gives a local of a DEF call under way, by [`Call::frame`], a new
    /// value. False when it has no such local.
    pub fn set_local(&mut self, frame: usize, name: &str, value: Value) -> bool {
        let Some(f) = self.frames.get(frame) else {
            return false;
        };
        let def = (self.program_in(f.home)).and_then(|p| p.defs.get(f.def as usize));
        let Some(i) = def.and_then(|d| d.locals.iter().position(|l| l.is_named(name))) else {
            return false;
        };
        let base = self.frames[frame].base;
        self.locals[base + i] = value;
        true
    }

    /// The variables of DEF call `frame`, by [`Call::frame`].
    fn frame_vars(&self, frame: usize) -> &[Value] {
        let end = (self.frames.get(frame + 1)).map_or(self.locals.len(), |f| f.base);
        &self.locals[self.frames[frame].base..end]
    }

    /// Ends the DEF calls from `frames` on, with their variables.
    fn truncate_frames(&mut self, frames: usize) {
        if let Some(f) = self.frames.get(frames) {
            self.locals.truncate(f.base);
        }
        self.frames.truncate(frames);
    }

    /// Gives a global of a slot's program that has run a new value. False
    /// when it has no such global.
    pub fn set_slot_global(&mut self, slot: usize, name: &str, value: Value) -> bool {
        let (program, values) = if slot == self.slot {
            (&self.program, &mut self.globals)
        } else {
            match self.units.get_mut(slot) {
                Some(Some(unit)) => (&unit.program, &mut unit.globals),
                _ => return false,
            }
        };
        let Some(i) = program.globals.iter().position(|g| g.is_named(name)) else {
            return false;
        };
        values[i] = value;
        true
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

    /// Whether an INPUT or LINPUT waits for its line.
    pub fn is_reading_input(&self) -> bool {
        self.reading_input
    }

    pub fn is_ended(&self) -> bool {
        self.ended
    }

    /// Value of a global variable by its full name (`"A%"`), for tests and
    /// debuggers.
    pub fn global(&self, name: &str) -> Option<&Value> {
        let i = self.program.globals.iter().position(|g| g.is_named(name))?;
        Some(&self.globals[i])
    }

    /// Instructions run so far, for hosts that share out a budget.
    pub fn executed(&self) -> u64 {
        self.executed
    }

    /// Whether [`Host::work`] hears of the work done before each host
    /// call, for hosts that time it as a console would take.
    pub fn report_work(&mut self, on: bool) {
        self.report_work = on;
        if on && self.charges.is_empty() {
            self.charges = charges();
        }
    }

    /// The work done so far, for hosts that stand for a console's speed.
    pub fn work(&self) -> Work {
        Work {
            executed: self.executed,
            statements: self.statements,
            def_calls: self.def_calls,
            builtin_calls: self.builtin_calls,
            builtin_args: self.builtin_args,
            elements: self.elements,
            extra_indices: self.extra_indices,
            str_elements: self.str_elements,
            filled: self.filled,
            lookups: self.lookups,
            joins: self.joins,
            for_tests: self.for_tests,
            def_for_tests: self.def_for_tests,
            element_tests: self.element_tests,
            chr_calls: self.chr_calls,
            costly_calls: self.costly_calls,
            cheap_calls: self.cheap_calls,
            trig_reductions: self.trig_reductions,
            join_chars: self.join_chars,
            str_compares: self.str_compares,
        }
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
        // The jump leaves GOSUBs, DEFs and callbacks behind.
        self.stack.clear();
        self.frames.clear();
        self.locals.clear();
        self.gosubs.clear();
        self.data_kept.clear();
        self.values_room_reached = false;
        self.callbacks.clear();
        self.reading_input = false;
        self.switch_to(slot, host);
        self.pc = addr;
        true
    }

    /// Executes up to `budget` instructions.
    pub fn run(&mut self, host: &mut dyn Host, budget: u32) -> Result<Status> {
        memory::enter(self.memory);
        let status = self.run_counted(host, budget);
        self.memory = memory::leave();
        let status = status?;
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

    /// [`Vm::run_code`], after counting memory if enough has been made
    /// since the last time. That is where what small requests add up to is
    /// refused.
    fn run_counted(&mut self, host: &mut dyn Host, budget: u32) -> Result<Status> {
        if memory::count_due() {
            self.count_for_slice(host)?;
        }
        // Fused steps leave out what the counts of [`Vm::work`] need, so
        // a run that reports them goes one instruction at a time.
        match self.report_work {
            true => self.run_code::<false>(host, budget),
            false => self.run_code::<true>(host, budget),
        }
    }

    #[cold]
    fn count_for_slice(&mut self, host: &dyn Host) -> Result<()> {
        if self.ended {
            return Ok(());
        }
        let tally = self.count_memory(host);
        memory::counted(&tally, false);
        if tally.bytes > memory::TOTAL as usize {
            let pc = self.pc;
            return Err(self.fail(pc, ErrorKind::OutOfMemory.into()));
        }
        Ok(())
    }

    /// An error from instruction `pc`. Ends the program with it, unless
    /// the instruction only asked for memory to be counted: then it runs
    /// again, unless it has just done so.
    #[cold]
    fn failed(&mut self, pc: usize, e: Error, host: &dyn Host) -> Option<Error> {
        if e.kind != ErrorKind::MemoryCheck {
            return Some(self.fail(pc, e));
        }
        if self.memory_checked == Some(self.executed) {
            return Some(self.fail(pc, ErrorKind::OutOfMemory.into()));
        }
        self.memory_checked = Some(self.executed);
        let tally = self.count_memory(host);
        memory::counted(&tally, true);
        None
    }

    /// Puts back what an instruction took off the stack, if it is to run
    /// again.
    #[cold]
    fn run_again(&mut self, e: Error, taken: Vec<Value>) -> Error {
        if e.kind == ErrorKind::MemoryCheck {
            self.stack.extend(taken);
        }
        e
    }

    /// The next string of a chain of `+`, added to the one made so far.
    #[inline(never)]
    fn append(&mut self) -> Result<()> {
        let rhs = self.pop();
        let lhs = self
            .stack
            .last_mut()
            .expect("compiler keeps the stack balanced");
        match (&*lhs, &rhs) {
            (Value::Str(l), Value::Str(r)) => {
                self.joins += 1;
                let added = r.borrow().clone();
                let mut l = l.borrow_mut();
                self.join_chars += (l.len() + added.len()) as u64;
                memory::take(memory::string_bytes(l.len() + added.len()))?;
                l.extend_from_slice(&added);
            }
            _ => match ops::binary(BinaryOp::Add, lhs, &rhs) {
                Ok(v) => *lhs = v,
                Err(e) => return Err(self.run_again_with(e, rhs)),
            },
        }
        Ok(())
    }

    /// DIM's array of the `dims` sizes on the stack.
    #[inline(never)]
    fn new_array(&mut self, elem: ElemType, dims: usize) -> Result<()> {
        let sizes = self.pop_n(dims);
        let dims = sizes
            .iter()
            .map(|d| usize::try_from(d.to_int()?).map_err(|_| ErrorKind::OutOfRange.into()))
            .collect::<Result<Vec<_>>>()?;
        // Only a one-dimensional array may be empty: DIM R#[0,0] is
        // Subscript out of range (checked on hardware).
        // One zero among other sizes is, too.
        if dims.len() > 1 && dims.contains(&0) {
            return Err(ErrorKind::SubscriptOutOfRange.into());
        }
        let len = dims.iter().fold(1usize, |all, &d| all.saturating_mul(d));
        if let Err(e) = memory::request(memory::array_bytes(elem, len)) {
            return Err(self.run_again(e, sizes));
        }
        self.stack
            .push(Value::Array(Rc::new(Array::new(elem, dims).into())));
        Ok(())
    }

    /// [`Vm::run_again`] for arguments left on the stack from `at`.
    #[cold]
    fn failed_with_args(&mut self, e: Error, at: usize) -> Error {
        if e.kind != ErrorKind::MemoryCheck {
            self.stack.truncate(at);
        }
        e
    }

    /// [`Vm::run_again`] for CALL, with the name it took too.
    #[cold]
    fn run_call_again(&mut self, e: Error, name: Value, args: Vec<Value>) -> Error {
        if e.kind == ErrorKind::MemoryCheck {
            self.stack.push(name);
            self.stack.extend(args);
        }
        e
    }

    /// [`Vm::run_again`] with the one value a binary operator took.
    #[cold]
    fn run_again_with(&mut self, e: Error, taken: Value) -> Error {
        if e.kind == ErrorKind::MemoryCheck {
            self.stack.push(taken);
        }
        e
    }

    fn run_code<const FUSED: bool>(&mut self, host: &mut dyn Host, budget: u32) -> Result<Status> {
        let mut passed = self.passed.take();
        if self.ended {
            return Ok(Status::Ended);
        }
        let stop = self.executed + u64::from(budget);
        let mut program = Rc::clone(&self.program);
        while self.executed < stop {
            // None of what runs in larger steps changes these.
            if FUSED
                && self.break_pending.is_none()
                && self.trace.is_none()
                && self.changes.is_empty()
                && self.stop_at.is_none()
                && program
                    .fast
                    .get(self.pc)
                    // `!=` would call FastOp's PartialEq on every step.
                    .is_some_and(|f| !matches!(f.op, FastOp::Slow))
            {
                if let Some((pc, outcome)) = self.run_fast(&program, stop, host) {
                    if let Some(end) = self.settle(pc, outcome, host) {
                        return end;
                    }
                    continue;
                }
            }
            if self.executed >= stop {
                break;
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
            if !self.changes.is_empty()
                && self.program.starts.get(self.pc) == Some(&true)
                && self.watch_changed()
            {
                self.trace = None;
                return Ok(Status::Debug { line: self.line() });
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
                        self.debug_note = Some(DebugNote::Step);
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
                && self.breakpoint_holds()
            {
                self.trace = None;
                if self.run_to.is_some() {
                    self.run_to(None);
                }
                return Ok(Status::Debug { line: self.line() });
            }
            let pc = self.pc;
            self.executed += 1;
            if !FUSED && program.starts.get(pc) == Some(&true) {
                self.statements += 1;
            }
            let outcome = self.step(&program, host);
            // Calls into other slots change the running program.
            if !Rc::ptr_eq(&program, &self.program) {
                program = Rc::clone(&self.program);
            }
            if let Some(end) = self.settle(pc, outcome, host) {
                return end;
            }
        }
        Ok(Status::Running)
    }

    /// What [`Vm::run_code`] does with the outcome of instruction `pc`, which
    /// it counted as run: `Some` ends the slice.
    #[inline(always)]
    fn settle(
        &mut self,
        pc: usize,
        outcome: Result<Option<Status>>,
        host: &dyn Host,
    ) -> Option<Result<Status>> {
        match outcome {
            Ok(None) => None,
            Ok(Some(status)) => {
                if status == Status::Ended {
                    self.ended = true;
                }
                Some(Ok(status))
            }
            Err(e) => {
                self.executed -= 1;
                // The stack may run out on what follows a GOSUB or call.
                let pc = match e.kind {
                    ErrorKind::StackOverflow => self.pc,
                    _ => pc,
                };
                let e = self.failed(pc, e, host)?;
                self.executed += 1;
                Some(Err(e))
            }
        }
    }

    /// Runs [`Compiled::fast`] until the budget is spent or something needs
    /// [`Vm::step`]: anything other than plain work on numbers, DEF calls
    /// within the slot and host calls, every failure included. An entry that
    /// cannot do its work leaves everything as it was, so that `step` goes on
    /// from the same place. A host call is made here as `step` makes it, and
    /// what comes of it other than going on is handed back with the
    /// instruction's address, for [`Vm::settle`].
    fn run_fast(
        &mut self,
        program: &Compiled,
        stop: u64,
        host: &mut dyn Host,
    ) -> Option<(usize, Result<Option<Status>>)> {
        loop {
            let Fused::Host { id, argc, want } = self.run_fused(program, stop) else {
                return None;
            };
            // The Nops of a FOR may lead the call in its entry.
            let nops = usize::from(program.fast[self.pc].len) - 1;
            self.pc += nops;
            self.executed += nops as u64;
            let pc = self.pc;
            self.executed += 1;
            match self.host_call(host, id, argc, want) {
                Ok(None) => {}
                outcome => return Some((pc, outcome)),
            }
        }
    }

    /// [`Vm::run_fast`] up to a host call. DEF calls and returns within the
    /// slot are made here, as [`Vm::enter_def_on_stack`] and
    /// [`Vm::return_op`] make them.
    #[inline(always)]
    fn run_fused(&mut self, program: &Compiled, stop: u64) -> Fused {
        let Vm {
            stack,
            globals,
            frames,
            locals,
            gosubs,
            data_kept,
            slot,
            pc: at,
            executed,
            def_calls,
            ..
        } = self;
        let slot = *slot;
        // The running DEF's stack taken, GOSUBs and variables, which
        // GOSUB and RETURN look at.
        let (mut calls, mut gosub_base, mut base) = innermost(frames, locals);
        let mut vars: Vars = [&mut globals[..], &mut locals[base..]];
        let mut pc = *at;
        let mut left = stop.saturating_sub(*executed);
        let mut exit = Fused::Slow;
        while let Some(fast) = program.fast.get(pc) {
            let len = u64::from(fast.len);
            if len > left {
                break;
            }
            let size = usize::from(fast.len);
            let mut next = pc + size;
            // An operator the entry's kind names, worked out on its own.
            macro_rules! store {
                ($op:expr, $a:expr, $b:expr, $to:expr) => {{
                    let Some(n) = ops::binary_numbers($op, get(&vars, $a), get(&vars, $b)) else {
                        break;
                    };
                    put(at_var(&mut vars, $to), n);
                }};
            }
            macro_rules! unless {
                ($op:expr, $a:expr, $b:expr, $target:expr) => {{
                    let Some(n) = ops::binary_numbers($op, get(&vars, $a), get(&vars, $b)) else {
                        break;
                    };
                    if !num_truth(n) {
                        next = $target as usize;
                    }
                }};
            }
            match fast.op {
                FastOp::Slow => break,
                FastOp::Nop => {}
                FastOp::Push(a) => push_copy(stack, get(&vars, a)),
                FastOp::Push2(a, b) => {
                    push_copy(stack, get(&vars, a));
                    push_copy(stack, get(&vars, b));
                }
                FastOp::Store(to) => {
                    let slot = at_var(&mut vars, to);
                    let value = top(stack);
                    match *value {
                        Value::Int(n) => *slot = Value::Int(n),
                        Value::Real(x) => *slot = Value::Real(x),
                        _ => std::mem::swap(slot, value),
                    }
                    drop_top(stack);
                }
                FastOp::Move { from, to } => {
                    copy_var(&mut vars, from, to);
                }
                FastOp::Swap => {
                    let n = stack.len();
                    swap_values(stack, n - 1, n - 2);
                }
                FastOp::Reverse(n) => {
                    let (mut low, mut high) = (stack.len() - n as usize, stack.len());
                    while low + 1 < high {
                        high -= 1;
                        swap_values(stack, low, high);
                        low += 1;
                    }
                }
                FastOp::Jump(a) => next = a as usize,
                FastOp::JumpIfFalse(a) => {
                    let Some(yes) = stack.last().and_then(truth) else {
                        break;
                    };
                    drop_top(stack);
                    if !yes {
                        next = a as usize;
                    }
                }
                FastOp::JumpIfTrue(a) => {
                    let Some(yes) = stack.last().and_then(truth) else {
                        break;
                    };
                    drop_top(stack);
                    if yes {
                        next = a as usize;
                    }
                }
                FastOp::On { from, count, gosub } => {
                    let selector = match from {
                        Selector::Stack => stack.last().and_then(index_number),
                        Selector::Var(v) => index_number(get(&vars, v)),
                        Selector::Index(base, i) => {
                            element1(get(&vars, base), get(&vars, i)).and_then(num_index)
                        }
                    };
                    let Some(selector) = selector else {
                        break;
                    };
                    // The table follows the entry.
                    let after = next + count as usize;
                    // An out-of-range selector continues after ON.
                    if (0..count as i32).contains(&selector) {
                        let Some(&Op::Jump(target)) = program.code.get(next + selector as usize)
                        else {
                            unreachable!("ON is followed by its jump table");
                        };
                        if gosub {
                            if !gosub_fits(calls, gosubs.len(), data_kept.len()) {
                                break;
                            }
                            gosubs.push((slot, after, data_kept.len()));
                        }
                        next = target as usize;
                    } else {
                        next = after;
                    }
                    if from == Selector::Stack {
                        drop_top(stack);
                    }
                }
                FastOp::Gosub(target) => {
                    if !gosub_fits(calls, gosubs.len(), data_kept.len()) {
                        break;
                    }
                    gosubs.push((slot, pc + 1, data_kept.len()));
                    next = target as usize;
                }
                FastOp::Return if gosubs.len() <= gosub_base => {
                    // Out of a DEF called in this slot, with no GOSUB of the DEF
                    // pending: as `return_op`. Others are for `step`.
                    let Some(frame) = frames.last() else {
                        break;
                    };
                    if frame.caller != slot || frame.home != slot {
                        break;
                    }
                    let info = &program.defs[frame.def as usize];
                    // The OUT variables, or the one value of a function.
                    let (start, count) = match (info.is_function, info.outs) {
                        (true, _) => (0, 0),
                        (false, Some(n)) => (usize::from(info.params.unwrap_or(0)), usize::from(n)),
                        (false, None) => break,
                    };
                    let given = match (info.is_function, frame.want) {
                        (true, _) => 1,
                        (false, Want::Value) => count.min(1),
                        (false, Want::Outs(_)) => count,
                    };
                    if given != frame.want.count() {
                        break;
                    }
                    let frame = frames.pop().expect("checked above");
                    stack.truncate(frame.stack_base);
                    if info.is_function {
                        // A bare RETURN in a function gives no value.
                        stack.push(Value::Default);
                    } else {
                        let start = frame.base + start;
                        move_values(&mut locals[start..start + given], stack);
                    }
                    locals.truncate(frame.base);
                    next = frame.return_pc;
                    drop_frame(frame);
                    (calls, gosub_base, base) = innermost(frames, locals);
                    vars = [&mut globals[..], &mut locals[base..]];
                }
                FastOp::Return => {
                    // Back into another slot, or with RESTORE "PUSH" places
                    // left: for `step`.
                    let Some(&(to_slot, back, kept)) = gosubs.last() else {
                        break;
                    };
                    if to_slot != slot || data_kept.len() > kept {
                        break;
                    }
                    gosubs.pop();
                    next = back;
                }
                FastOp::CallDef {
                    def,
                    argc,
                    want,
                    cost,
                } => {
                    // As `enter_def_on_stack`, which the fuser has made sure
                    // the call fits, but for the stack's room.
                    let cost = cost as usize;
                    if room_for(calls, gosubs.len(), data_kept.len(), cost) > STACK_SIZE {
                        break;
                    }
                    let info = &program.defs[def as usize];
                    *def_calls += 1;
                    base = locals.len();
                    take_args(stack, locals, usize::from(argc));
                    start_locals(locals, base, info);
                    frames.push(Frame {
                        def,
                        home: slot,
                        caller: slot,
                        base,
                        args: Vec::new(),
                        outs: Vec::new(),
                        want,
                        return_pc: next,
                        gosub_base: gosubs.len(),
                        stack_base: stack.len(),
                        stack_to: calls + cost,
                    });
                    (calls, gosub_base) = (calls + cost, gosubs.len());
                    vars = [&mut globals[..], &mut locals[base..]];
                    next = info.entry as usize;
                }
                FastOp::ReturnValue => {
                    // From a function called in this slot: as `leave_def_with`.
                    let Some(frame) = frames.last() else {
                        break;
                    };
                    if frame.caller != slot || frame.want.count() != 1 {
                        break;
                    }
                    let frame = frames.pop().expect("checked above");
                    locals.truncate(frame.base);
                    gosubs.truncate(frame.gosub_base);
                    let len = stack.len();
                    if len - 1 != frame.stack_base {
                        stack.swap(len - 1, frame.stack_base);
                        stack.truncate(frame.stack_base + 1);
                    }
                    next = frame.return_pc;
                    drop_frame(frame);
                    (calls, gosub_base, base) = innermost(frames, locals);
                    vars = [&mut globals[..], &mut locals[base..]];
                }
                FastOp::Host { id, argc, want } => {
                    exit = Fused::Host { id, argc, want };
                    break;
                }
                FastOp::Builtin { id, argc } => {
                    let builtins::Kind::Function(f) = builtins::get(id).kind else {
                        unreachable!("only functions are fused");
                    };
                    let at = stack.len() - usize::from(argc);
                    // A failure is left to `step`, which calls it again and
                    // fails as it always does: these change nothing.
                    let n = match f(&stack[at..]) {
                        Ok(Value::Int(n)) => Num::Int(n),
                        Ok(Value::Real(x)) => Num::Real(x),
                        _ => break,
                    };
                    for _ in 0..argc {
                        drop_top(stack);
                    }
                    push_num(stack, n);
                }
                FastOp::Unary(op) => {
                    let value = top(stack);
                    let Some(n) = ops::unary_number(op, value) else {
                        break;
                    };
                    put(value, n);
                }
                FastOp::Truthy => {
                    let value = top(stack);
                    let Some(yes) = truth(value) else {
                        break;
                    };
                    put(value, Num::Int(i32::from(yes)));
                }
                FastOp::Binary(op) => {
                    let Some(n) = on_two(stack, |l, r| ops::binary_numbers(op, l, r)) else {
                        break;
                    };
                    drop_top(stack);
                    put(top(stack), n);
                }
                FastOp::Swapped(op) => {
                    let Some(n) = on_two(stack, |l, r| ops::binary_numbers(op, r, l)) else {
                        break;
                    };
                    drop_top(stack);
                    put(top(stack), n);
                }
                FastOp::SwappedStore(op, to) => {
                    let Some(n) = on_two(stack, |l, r| ops::binary_numbers(op, r, l)) else {
                        break;
                    };
                    drop_top(stack);
                    drop_top(stack);
                    put(at_var(&mut vars, to), n);
                }
                FastOp::Vars { op, a, b } => {
                    let Some(n) = ops::binary_numbers(op, get(&vars, a), get(&vars, b)) else {
                        break;
                    };
                    push_num(stack, n);
                }
                FastOp::VarsStore { op, a, b, to } => {
                    let Some(n) = ops::binary_numbers(op, get(&vars, a), get(&vars, b)) else {
                        break;
                    };
                    put(at_var(&mut vars, to), n);
                }
                FastOp::AddStore(a, b, to) => store!(BinaryOp::Add, a, b, to),
                FastOp::SubStore(a, b, to) => store!(BinaryOp::Sub, a, b, to),
                FastOp::MulStore(a, b, to) => store!(BinaryOp::Mul, a, b, to),
                FastOp::UnlessEq(a, b, t) => unless!(BinaryOp::Eq, a, b, t),
                FastOp::UnlessNotEq(a, b, t) => unless!(BinaryOp::NotEq, a, b, t),
                FastOp::UnlessLess(a, b, t) => unless!(BinaryOp::Less, a, b, t),
                FastOp::UnlessLessEq(a, b, t) => unless!(BinaryOp::LessEq, a, b, t),
                FastOp::UnlessGreater(a, b, t) => unless!(BinaryOp::Greater, a, b, t),
                FastOp::UnlessGreaterEq(a, b, t) => unless!(BinaryOp::GreaterEq, a, b, t),
                FastOp::VarsJump { op, a, b, target } => {
                    let Some(n) = ops::binary_numbers(op, get(&vars, a), get(&vars, b)) else {
                        break;
                    };
                    if !num_truth(n) {
                        next = target as usize;
                    }
                }
                FastOp::VarsJumpTrue { op, a, b, target } => {
                    let Some(n) = ops::binary_numbers(op, get(&vars, a), get(&vars, b)) else {
                        break;
                    };
                    if num_truth(n) {
                        next = target as usize;
                    }
                }
                FastOp::Under { op, a } => {
                    let rhs = top(stack);
                    let Some(n) = ops::binary_numbers(op, get(&vars, a), rhs) else {
                        break;
                    };
                    put(rhs, n);
                }
                FastOp::UnderStore { op, a, to } => {
                    let Some(n) = ops::binary_numbers(op, get(&vars, a), top(stack)) else {
                        break;
                    };
                    drop_top(stack);
                    put(at_var(&mut vars, to), n);
                }
                FastOp::Index1 => {
                    let Some(n) = on_two(stack, element1) else {
                        break;
                    };
                    drop_top(stack);
                    put(top(stack), n);
                }
                FastOp::Index2 => {
                    let n = stack.len();
                    let Some(v) = element2(&stack[n - 3], &stack[n - 2], &stack[n - 1]) else {
                        break;
                    };
                    drop_top(stack);
                    drop_top(stack);
                    put(top(stack), v);
                }
                FastOp::Index1Vars { base, i } => {
                    let Some(n) = element1(get(&vars, base), get(&vars, i)) else {
                        break;
                    };
                    push_num(stack, n);
                }
                FastOp::Index1Store { base, i, to } => {
                    let Some(n) = element1(get(&vars, base), get(&vars, i)) else {
                        break;
                    };
                    put(at_var(&mut vars, to), n);
                }
                FastOp::Index1Nested { base, inner, i } => {
                    let Some(k) = element1(get(&vars, inner), get(&vars, i)) else {
                        break;
                    };
                    let Some(n) = element1(get(&vars, base), &Value::from(k)) else {
                        break;
                    };
                    push_num(stack, n);
                }
                FastOp::StoreIndex1Nested { base, inner, i } => {
                    let Some(k) = element1(get(&vars, inner), get(&vars, i)) else {
                        break;
                    };
                    if !set_element1(get(&vars, base), &Value::from(k), top(stack)) {
                        break;
                    }
                    drop_top(stack);
                }
                FastOp::StoreIndex1 => {
                    let n = stack.len();
                    if !set_element1(&stack[n - 2], &stack[n - 1], &stack[n - 3]) {
                        break;
                    }
                    drop_top(stack);
                    drop_top(stack);
                    drop_top(stack);
                }
                FastOp::StoreIndex2 => {
                    let n = stack.len();
                    let (base, i, j) = (&stack[n - 3], &stack[n - 2], &stack[n - 1]);
                    if !set_element2(base, i, j, &stack[n - 4]) {
                        break;
                    }
                    drop_top(stack);
                    drop_top(stack);
                    drop_top(stack);
                    drop_top(stack);
                }
                FastOp::StoreIndex1Vars { base, i } => {
                    if !set_element1(get(&vars, base), get(&vars, i), top(stack)) {
                        break;
                    }
                    drop_top(stack);
                }
                FastOp::ForCheck {
                    var,
                    to,
                    step,
                    exit,
                } => {
                    let more = for_continues(get(&vars, var), get(&vars, to), get(&vars, step));
                    let Some(more) = more else { break };
                    if !more {
                        next = exit as usize;
                    }
                }
                FastOp::Chain {
                    first,
                    count,
                    next: to,
                } => {
                    next = to as usize;
                    let first = first as usize;
                    // Where `step` goes on from if a step cannot do its
                    // work, and the instructions run by then.
                    let mut done = (pc, 0);
                    let mut failed = false;
                    'micro: for &m in &program.micro[first..first + usize::from(count)] {
                        // A step that cannot do its work leaves the chain to
                        // `step` from its start: those before it wrote only
                        // the chain's own places.
                        macro_rules! fail {
                            () => {{
                                failed = true;
                                break 'micro;
                            }};
                        }
                        macro_rules! binary {
                            ($op:expr, $a:expr, $b:expr, $to:expr) => {{
                                let n = ops::binary_numbers($op, get(&vars, $a), get(&vars, $b));
                                let Some(n) = n else { fail!() };
                                put(at_var(&mut vars, $to), n);
                            }};
                        }
                        macro_rules! jump {
                            ($op:expr, $j:expr) => {{
                                let n =
                                    ops::binary_numbers($op, get(&vars, $j.a), get(&vars, $j.b));
                                let Some(n) = n else { fail!() };
                                if num_truth(n) == $j.when {
                                    next = $j.target as usize;
                                }
                            }};
                        }
                        use BinaryOp as B;
                        use Micro as M;
                        match m {
                            M::Add(a, b, to) => binary!(B::Add, a, b, to),
                            M::Sub(a, b, to) => binary!(B::Sub, a, b, to),
                            M::Mul(a, b, to) => binary!(B::Mul, a, b, to),
                            M::Div(a, b, to) => binary!(B::Div, a, b, to),
                            M::IntDiv(a, b, to) => binary!(B::IntDiv, a, b, to),
                            M::Mod(a, b, to) => binary!(B::Mod, a, b, to),
                            M::And(a, b, to) => binary!(B::And, a, b, to),
                            M::Or(a, b, to) => binary!(B::Or, a, b, to),
                            M::Xor(a, b, to) => binary!(B::Xor, a, b, to),
                            M::Shl(a, b, to) => binary!(B::Shl, a, b, to),
                            M::Shr(a, b, to) => binary!(B::Shr, a, b, to),
                            M::Eq(a, b, to) => binary!(B::Eq, a, b, to),
                            M::NotEq(a, b, to) => binary!(B::NotEq, a, b, to),
                            M::Less(a, b, to) => binary!(B::Less, a, b, to),
                            M::LessEq(a, b, to) => binary!(B::LessEq, a, b, to),
                            M::Greater(a, b, to) => binary!(B::Greater, a, b, to),
                            M::GreaterEq(a, b, to) => binary!(B::GreaterEq, a, b, to),
                            M::Binary(op, a, b, to) => binary!(op, a, b, to),
                            M::Neg(a, to) => {
                                let n = ops::unary_number(UnaryOp::Neg, get(&vars, a));
                                let Some(n) = n else { fail!() };
                                put(at_var(&mut vars, to), n);
                            }
                            M::Unary(op, a, to) => {
                                let Some(n) = ops::unary_number(op, get(&vars, a)) else {
                                    fail!();
                                };
                                put(at_var(&mut vars, to), n);
                            }
                            M::Index1 { base, i, to } => {
                                let Some(n) = element1(get(&vars, base), get(&vars, i)) else {
                                    fail!();
                                };
                                put(at_var(&mut vars, to), n);
                            }
                            M::Index2 { base, i, j, to } => {
                                let n = element2(get(&vars, base), get(&vars, i), get(&vars, j));
                                let Some(n) = n else { fail!() };
                                put(at_var(&mut vars, to), n);
                            }
                            M::Builtin { id, argc, args, to } => {
                                let builtins::Kind::Function(f) = builtins::get(id).kind else {
                                    unreachable!("only functions are fused");
                                };
                                let result = match argc {
                                    1 => f(std::slice::from_ref(get(&vars, args))),
                                    n => {
                                        let at = args.index();
                                        f(&vars[args.table()][at..at + usize::from(n)])
                                    }
                                };
                                let n = match result {
                                    Ok(Value::Int(n)) => Num::Int(n),
                                    Ok(Value::Real(x)) => Num::Real(x),
                                    _ => fail!(),
                                };
                                put(at_var(&mut vars, to), n);
                            }
                            M::Move { from, to } => {
                                let Some(n) = number(get(&vars, from)) else {
                                    fail!();
                                };
                                put(at_var(&mut vars, to), n);
                            }
                            M::StoreIndex1 { value, base, i } => {
                                if !set_element1(get(&vars, base), get(&vars, i), get(&vars, value))
                                {
                                    fail!();
                                }
                            }
                            M::StoreIndex2 { value, base, i, j } => {
                                let (b, v) = (get(&vars, base), get(&vars, value));
                                if !set_element2(b, get(&vars, i), get(&vars, j), v) {
                                    fail!();
                                }
                            }
                            M::JumpIfFalse { cond, target } => {
                                let Some(yes) = truth(get(&vars, cond)) else {
                                    fail!();
                                };
                                if !yes {
                                    next = target as usize;
                                }
                            }
                            M::JumpIfTrue { cond, target } => {
                                let Some(yes) = truth(get(&vars, cond)) else {
                                    fail!();
                                };
                                if yes {
                                    next = target as usize;
                                }
                            }
                            M::JumpEq(j) => jump!(B::Eq, j),
                            M::JumpNotEq(j) => jump!(B::NotEq, j),
                            M::JumpLess(j) => jump!(B::Less, j),
                            M::JumpLessEq(j) => jump!(B::LessEq, j),
                            M::JumpGreater(j) => jump!(B::Greater, j),
                            M::JumpGreaterEq(j) => jump!(B::GreaterEq, j),
                            M::Commit { to, charged } => done = (to as usize, u64::from(charged)),
                            M::Truthy(a, to) => {
                                let Some(yes) = truth(get(&vars, a)) else {
                                    fail!();
                                };
                                put(at_var(&mut vars, to), Num::Int(i32::from(yes)));
                            }
                            M::Push(a) => {
                                let Some(n) = number(get(&vars, a)) else {
                                    fail!();
                                };
                                push_num(stack, n);
                            }
                        }
                    }
                    if failed {
                        pc = done.0;
                        left -= done.1;
                        break;
                    }
                }
                FastOp::ForNext {
                    var,
                    step,
                    step_slot,
                    to,
                    to_slot,
                    exit,
                    apart,
                } => {
                    // Integers, as nearly all FORs count: what the general
                    // steps below do on them, each value read once.
                    if apart
                        && let (Value::Int(v), Value::Int(s0)) =
                            (get(&vars, var), get(&vars, step_slot))
                        && let Some(n) = v.checked_add(*s0)
                        && let (&Value::Int(s), &Value::Int(t)) = (get(&vars, step), get(&vars, to))
                    {
                        put(at_var(&mut vars, var), Num::Int(n));
                        put(at_var(&mut vars, step_slot), Num::Int(s));
                        put(at_var(&mut vars, to_slot), Num::Int(t));
                        if !(if s >= 0 { n <= t } else { n >= t }) {
                            next = exit as usize;
                        }
                        pc = next;
                        left -= len;
                        continue;
                    }
                    let n =
                        ops::binary_numbers(BinaryOp::Add, get(&vars, var), get(&vars, step_slot));
                    let Some(n) = n else { break };
                    put(at_var(&mut vars, var), n);
                    copy_var(&mut vars, step, step_slot);
                    copy_var(&mut vars, to, to_slot);
                    let more =
                        for_continues(get(&vars, var), get(&vars, to_slot), get(&vars, step_slot));
                    let Some(more) = more else {
                        // All but the test has run, and that must not.
                        pc += size - FOR_TEST_LEN;
                        left -= len - FOR_TEST_LEN as u64;
                        break;
                    };
                    if !more {
                        next = exit as usize;
                    }
                }
            }
            pc = next;
            left -= len;
        }
        *at = pc;
        *executed = stop - left;
        exit
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

    /// Whether a FOR goes on, from its variable, its end and its step on the
    /// stack. Nearly all of them count in integers.
    #[inline]
    fn for_test(&mut self) -> Result<()> {
        let step = self.pop();
        let to = self.pop();
        let var = self.pop();
        let more = if let (Value::Int(v), Value::Int(t), Value::Int(s)) = (&var, &to, &step) {
            Value::Int(i32::from(if *s >= 0 { v <= t } else { v >= t }))
        } else {
            let ordering = if step.to_real()? >= 0.0 {
                BinaryOp::LessEq
            } else {
                BinaryOp::GreaterEq
            };
            ops::binary(ordering, &var, &to)?
        };
        self.stack.push(more);
        Ok(())
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

    fn locals(&mut self) -> &mut [Value] {
        let f = (self.frames.last()).expect("compiler emits locals only in DEFs");
        &mut self.locals[f.base..]
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
    /// at the top level of a slot that EXEC has run, as `"1:@NAME"`. In a
    /// DEF, a label with a slot is never found (4.4.8).
    fn find_label(&self, value: &Value) -> Result<(usize, usize)> {
        let (slot, name) = label_string(value)?;
        let found = match slot {
            Some(_) if self.own_frame().is_some() => None,
            Some(slot) if slot == self.slot => {
                (self.program.labels.get(&name)).map(|&a| (slot, a as usize))
            }
            Some(slot) => (self.program_in(slot))
                .and_then(|p| p.labels.get(&name))
                .map(|&a| (slot, a as usize)),
            None => self.labels().get(&name).map(|&a| (self.slot, a as usize)),
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
            // A slot that EXEC has not run reports 0 rather than an error.
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
        let tally = self.count_memory(host);
        memory::counted(&tally, false);
        tally.free()
    }

    fn count_memory(&self, host: &dyn Host) -> memory::Tally {
        let mut tally = memory::Tally::default();
        for array in host.own_arrays() {
            tally.skip(&array);
        }
        let frames = (self.locals.iter()).chain(
            self.frames
                .iter()
                .flat_map(|f| f.args.iter().chain(&f.outs)),
        );
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
        tally
    }

    /// CHKCALL: whether CALL could call the name, as a DEF of this slot,
    /// a COMMON DEF of another, or a command or function of the system.
    // Reserved statements such as PRINT and GOTO report 0.
    fn has_call(&self, value: &Value, host: &dyn Host) -> Result<bool> {
        let (slot, name) = slot_and_name(value)?;
        // With another slot, its DEFs, and the system's commands and
        // functions still: SmileBASIC Debugger finds those with
        // CHKCALL("n:NAME") of a slot it leaves empty.
        let other = slot.filter(|&s| s != self.slot);
        let program = match other {
            Some(slot) => self.program_in(slot),
            None => Some(&self.program),
        };
        if program.is_some_and(|p| p.def_names.contains_key(&name)) {
            return Ok(true);
        }
        if NOT_CALLS.contains(&name.as_str()) {
            return Ok(false);
        }
        Ok(self.find_common(&name).is_some() || is_system_call(&name, host))
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
        self.truncate_frames(back.frames);
        self.gosubs.truncate(back.gosubs);
        self.stack.truncate(back.stack);
        self.callbacks.truncate(back.callbacks);
        (self.data_pos, self.data_from) = (back.data_pos, back.data_from);
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
            self.truncate_frames(frames);
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
                data_pos: self.data_pos,
                data_from: Rc::clone(&self.data_from),
            });
            let old = self.take_unit();
            self.park(self.slot, old);
            if let Some(stale) = self.units.get_mut(slot) {
                *stale = None;
            }
        }
        self.put_unit(unit, slot, host);
        // The program EXEC starts reads its own DATA, and the one that
        // ran EXEC reads where it was once it goes on. UNVERIFIED.
        (self.data_pos, self.data_from) = (0, Rc::clone(&self.program));
        self.pc = 0;
        // Back to the host, which may give the new code its breakpoints
        // before it runs.
        Ok(Some(Status::Running))
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
                if !self.stack_has_full_room(RESTORE_COST) {
                    return Err(ErrorKind::StackOverflow.into());
                }
                self.data_kept.push(Kept {
                    pos: self.data_pos,
                    from: Rc::clone(&self.data_from),
                    calls: self.frames.len(),
                    gosubs: self.gosubs.len(),
                });
                self.note_values_room();
            }
            "POP" => {
                // GOSUBs, DEF calls and kept places share one stack, and
                // only a kept place on top can come off it.
                let (calls, gosubs) = match self.data_kept.last() {
                    Some(k) => (k.calls, k.gosubs),
                    None => (0, 0),
                };
                if self.frames.len() > calls || self.gosubs.len() > gosubs {
                    return Err(ErrorKind::UseRestorePush.into());
                }
                let kept = self.data_kept.pop().ok_or(ErrorKind::StackUnderflow)?;
                (self.data_pos, self.data_from) = (kept.pos, kept.from);
            }
            "BEGIN" => (self.data_pos, self.data_from) = (0, Rc::clone(&self.program)),
            _ => {
                let Some((slot, "BEGIN")) = s.split_once(':') else {
                    return Ok(false);
                };
                let slot: usize = slot.trim().parse().map_err(|_| {
                    Error::with_detail(ErrorKind::IllegalFunctionCall, format!("slot {slot:?}"))
                })?;
                // It moves the one READ place to the start of that slot's
                // DATA. UNVERIFIED.
                let Some(program) = self.program_in(slot) else {
                    return Err(Error::with_detail(
                        ErrorKind::OutOfRange,
                        format!("slot {slot}"),
                    ));
                };
                (self.data_pos, self.data_from) = (0, Rc::clone(program));
            }
        }
        Ok(true)
    }

    /// Whether `cost` more of the stack GOSUBs, DEF calls and RESTORE "PUSH"
    /// share is free.
    fn stack_has_room(&self, cost: usize) -> bool {
        let calls = self.frames.last().map_or(0, |f| f.stack_to);
        room_for(calls, self.gosubs.len(), self.data_kept.len(), cost) <= STACK_SIZE
    }

    /// [`Vm::stack_has_room`] for GOSUB and RESTORE "PUSH", which may take
    /// the values' room too.
    fn stack_has_full_room(&self, cost: usize) -> bool {
        let calls = self.frames.last().map_or(0, |f| f.stack_to);
        room_for(calls, self.gosubs.len(), self.data_kept.len(), cost) <= STACK_FULL
    }

    /// A command given arguments when GOSUBs or RESTORE "PUSH" have taken
    /// the values' room: Stack overflow on it. PRINT goes on while the
    /// room has space for a value. UNVERIFIED: that a command without
    /// arguments, and a builtin function, go on.
    #[inline(always)]
    fn values_room_taken(&self, argc: usize) -> bool {
        argc > 0 && self.values_room_reached && !self.stack_has_room(0)
    }

    /// Keeps [`Vm::values_room_reached`] after a GOSUB or RESTORE "PUSH";
    /// DEF calls never go past the room.
    fn note_values_room(&mut self) {
        self.values_room_reached |= !self.stack_has_room(0);
    }

    /// GOSUB to `target` of `slot`. One that reaches into the values' room
    /// goes, and the first command given arguments after it stops. GOSUBs
    /// go on until the values' room is full, as RESTORE "PUSH" does, and
    /// the one past it stops on itself.
    fn gosub(&mut self, slot: usize, target: usize, host: &mut dyn Host) -> Result<()> {
        if !self.stack_has_full_room(GOSUB_COST) {
            return Err(ErrorKind::StackOverflow.into());
        }
        self.gosubs
            .push((self.slot, self.pc + 1, self.data_kept.len()));
        self.note_values_room();
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
            Op::Str(i) => {
                let s = program.strings[*i as usize].clone();
                memory::charge(memory::string_bytes(s.len()));
                self.stack.push(Value::Str(new_str(s)));
            }
            Op::ShiftConstant { first, len } => {
                memory::request(memory::string_bytes(*len as usize))
                    .map_err(|e| builtin_error(e, "SHIFT", &[]))?;
                let first = program.strings[*first as usize].clone();
                self.stack.push(Value::Str(new_str(first)));
            }
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
                    false => initial_value(&self.program.globals[i]),
                };
            }
            Op::Store(slot) => {
                let v = self.pop();
                self.store(*slot, v);
            }
            Op::Index(n) => {
                self.elements += 1;
                self.extra_indices += u64::from(n.saturating_sub(1));
                let idx = self.indices(*n)?;
                let base = self.pop();
                self.str_elements += u64::from(matches!(base, Value::Str(_)));
                self.stack.push(index(&base, &idx)?);
            }
            Op::StoreIndex(n) => {
                self.elements += 1;
                self.extra_indices += u64::from(n.saturating_sub(1));
                let idx = self.indices(*n)?;
                let base = self.pop();
                self.str_elements += u64::from(matches!(base, Value::Str(_)));
                let value = self.pop();
                store_index(&base, &idx, value)?;
            }
            Op::NewArray { elem, dims } => self.new_array(*elem, (*dims).into())?,
            Op::ArrayLit { elem, len } => {
                let values = self.pop_n(*len as usize);
                memory::take(memory::array_bytes(*elem, values.len()))?;
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
                    // More values than elements is an error.
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
                if let Value::Str(l) = &*lhs {
                    match op {
                        BinaryOp::Add => {
                            self.joins += 1;
                            if let Value::Str(r) = &rhs {
                                self.join_chars += (l.borrow().len() + r.borrow().len()) as u64;
                            }
                        }
                        _ => self.str_compares += 1,
                    }
                }
                match ops::binary(*op, lhs, &rhs) {
                    Ok(v) => *lhs = v,
                    Err(e) => return Err(self.run_again_with(e, rhs)),
                }
            }
            Op::Append => self.append()?,
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
            Op::TestIfFalse(a) => {
                self.element_tests += 1;
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
            Op::UndefinedLabel(name) => {
                let name = String::from_utf16_lossy(&program.strings[*name as usize]);
                return Err(Error::with_detail(ErrorKind::UndefinedLabel, name));
            }
            Op::GotoDyn => {
                self.lookups += 1;
                let label = self.pop();
                let (slot, target) = self.find_label(&label)?;
                self.switch_to(slot, host);
                next = target;
            }
            Op::GosubDyn => {
                self.lookups += 1;
                let label = self.pop();
                let (slot, target) = self.find_label(&label)?;
                self.gosub(slot, target, host)?;
                return Ok(None);
            }
            Op::On { count, gosub } => {
                // Its table costs about what a builtin's call with no
                // arguments does.
                self.builtin_calls += 1;
                let selector = self.pop().to_int()?;
                let after = self.pc + 1 + *count as usize;
                next = after;
                // An out-of-range selector continues after ON.
                if (0..*count as i32).contains(&selector) {
                    let Op::Jump(target) = program.code[self.pc + 1 + selector as usize] else {
                        unreachable!("ON is followed by its jump table");
                    };
                    if *gosub {
                        // As in `gosub`.
                        if !self.stack_has_full_room(GOSUB_COST) {
                            return Err(ErrorKind::StackOverflow.into());
                        }
                        self.gosubs.push((self.slot, after, self.data_kept.len()));
                        self.note_values_room();
                    }
                    next = target as usize;
                }
            }
            Op::Return => next = self.return_op(host)?,
            Op::ReturnValue => {
                let value = self.pop();
                next = self.leave_def_with(value, host)?;
            }
            Op::CallDef { def, argc, want } => {
                self.enter_def_on_stack(*def, (*argc).into(), *want, self.slot)?;
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
                let b = builtins::get(*id);
                // CHR$ costs more than a call (TEST68, TEST69).
                // UNVERIFIED: that the builtins not timed cost what the
                // meter charges a call.
                let charge = match self.report_work {
                    true => self.charges.get(usize::from(*id)).copied(),
                    false => None,
                };
                let call = match charge {
                    None | Some(Charge::Call) => true,
                    Some(Charge::Free) => false,
                    Some(Charge::FreeWithTwo) => *argc != 2,
                    // A CHR$ of the code the last one made costs only its
                    // call (TEST71 to TEST74 time U$=CHR$(C) so).
                    // UNVERIFIED: that SmileBASIC 4 keeps the last string
                    // CHR$ made, as HGCNV$ and TPERF's VSTR, which make a
                    // new code each time, need CHR$'s cost.
                    Some(Charge::Chr) => {
                        let code = self.stack.last().and_then(|v| v.to_int().ok());
                        if code.is_none() || std::mem::replace(&mut self.last_chr, code) != code {
                            self.chr_calls += 1;
                        }
                        true
                    }
                    Some(Charge::Costly(i)) => {
                        self.costly_calls[usize::from(i)] += 1;
                        true
                    }
                    Some(Charge::Cheap(i)) => {
                        self.cheap_calls[usize::from(i)] += 1;
                        let far = |v: &Value| {
                            v.to_real()
                                .is_ok_and(|x| x.abs() > std::f64::consts::FRAC_PI_4)
                        };
                        if i < 3 && self.stack.last().is_some_and(far) {
                            self.trig_reductions += 1;
                        }
                        false
                    }
                };
                if call {
                    self.builtin_calls += 1;
                    self.builtin_args += u64::from(*argc);
                }
                let argc = usize::from(*argc);
                if let (Kind::Function(f), Want::Value, true) = (b.kind, *want, b.accepts(argc)) {
                    // Most calls: no need to take the arguments off the stack
                    // for the call, or to make a list of one result.
                    let at = self.stack.len() - argc;
                    let result = f(&self.stack[at..]);
                    let result = result.map_err(|e| builtin_error(e, b.name, &self.stack[at..]));
                    match result {
                        Ok(v) => {
                            self.stack.truncate(at);
                            self.stack.push(v);
                        }
                        Err(e) => return Err(self.failed_with_args(e, at)),
                    }
                } else {
                    let args = self.pop_n(argc);
                    if self.report_work && b.name == "FILL" {
                        self.filled += fill_count(&args);
                    }
                    if let Err(e) = self.call_builtin(program, *id, &args, *want) {
                        return Err(self.run_again(e, args));
                    }
                }
            }
            Op::CallHost { id, argc, want } => return self.host_call(host, *id, *argc, *want),
            Op::CallDyn { argc, want } => {
                self.lookups += 1;
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
                } else if let ("VSYNC" | "WAIT", Want::Outs(0), 0..=1) =
                    (name.as_str(), *want, args.len())
                {
                    // Commands the VM runs itself, which SmileBASIC
                    // Debugger calls by name too.
                    let frames = args.first().map(Value::to_int).transpose()?.unwrap_or(1);
                    self.pc = next;
                    return Ok(Some(Status::Wait {
                        frames: frames.max(0) as u32,
                        vsync: name == "VSYNC",
                    }));
                } else if let ("FREEMEM", Want::Value | Want::Outs(1 | 2), 0) =
                    (name.as_str(), *want, args.len())
                {
                    let free = Value::Int(self.free_memory(host));
                    if *want == Want::Outs(2) {
                        self.stack.push(free.clone());
                    }
                    self.stack.push(free);
                } else if let Some(id) = builtins::lookup(&name, form(*want)) {
                    if let Err(e) = self.call_builtin(program, id, &args, *want) {
                        return Err(self.run_call_again(e, name_value, args));
                    }
                } else if *want == Want::Outs(1)
                    && let Some(id) = builtins::lookup(&name, Form::Function)
                {
                    if let Err(e) = self.call_builtin(program, id, &args, Want::Value) {
                        return Err(self.run_call_again(e, name_value, args));
                    }
                } else if let Some(id) = host.lookup(&name) {
                    match self.call_host(host, id, &args, *want) {
                        Ok(true) => {}
                        Ok(false) => {
                            self.stack.push(name_value);
                            self.stack.extend(args);
                            return Ok(Some(RETRY));
                        }
                        Err(e) => return Err(self.run_call_again(e, name_value, args)),
                    }
                } else {
                    return Err(Error::with_detail(ErrorKind::UndefinedFunction, name));
                }
            }
            Op::LoadDyn => {
                self.lookups += 1;
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
                self.lookups += 1;
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
                name_arg(&name, "CHKLABEL")?;
                let found = self.has_label(&name, outer)?;
                self.stack.push(Value::Int(found.into()));
            }
            Op::ChkCall => {
                let name = self.pop();
                name_arg(&name, "CHKCALL")?;
                let found = self.has_call(&name, host)?;
                self.stack.push(Value::Int(found.into()));
            }
            Op::ChkVar => {
                let target = self.pop();
                name_arg(&target, "CHKVAR")?;
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
                        .then(|| self.locals[f.base + n as usize].clone()),
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
                        self.locals[frame.base + slot] = value;
                    }
                    (_, None) if n < frame.outs.len() => frame.outs[n] = value,
                    _ => return Err(ErrorKind::OutOfRange.into()),
                }
            }
            Op::ForTest => {
                self.for_tests += 1;
                // A NEXT in a DEF costs SmileBASIC 4 more (TPERF, TEST70, TEST71).
                self.def_for_tests += u64::from(!self.frames.is_empty());
                self.for_test()?;
            }
            Op::Print { screen } => {
                if self.values_room_reached && !self.stack_has_full_room(VALUE_COST) {
                    return Err(ErrorKind::StackOverflow.into());
                }
                let value = self.pop();
                let screen = self.screen_arg(*screen)?;
                host.print(screen, &text_of(&value)?)?;
                if self.report_work && host.out_of_time() {
                    self.pc = next;
                    return Ok(Some(Status::Running));
                }
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
                let frames = match *has_arg {
                    true => (self.pop().to_int()).map_err(|e| {
                        let name = if *vsync { "VSYNC" } else { "WAIT" };
                        arg_error(e, 0, name)
                    })?,
                    false => 1,
                };
                // Zero or negative counts do not wait.
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
                let mut known = Vec::with_capacity(kinds.len());
                for &kind in kinds.iter() {
                    known.push(match kind {
                        InputKind::Named(slot) => {
                            let name = self.load(slot).into_str()?;
                            match name.borrow().last() == Some(&u16::from(b'$')) {
                                true => InputKind::Str,
                                false => InputKind::Number,
                            }
                        }
                        kind => kind,
                    });
                }
                match parse_input(&line, &known, *whole_line) {
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
            Op::Restore(pos) => {
                (self.data_pos, self.data_from) = (*pos as usize, Rc::clone(&self.program));
            }
            Op::RestoreDyn => {
                let target = self.pop();
                if !self.restore_special(&target)? {
                    // "n:@LABEL" sends READ to another slot's DATA, as
                    // FINITI does to read models from the program that
                    // calls it, also from a DEF: the top level of that
                    // slot. A slot EXEC has not run gives Undefined label.
                    // "@LABEL" looks in the DEF running, then the top level.
                    let (slot, name) = label_string(&target)?;
                    let from = slot
                        .filter(|&s| s != self.slot)
                        .map(|s| self.program_in(s).cloned());
                    let undefined =
                        || Error::with_detail(ErrorKind::UndefinedLabel, format!("@{name}"));
                    let own = match slot {
                        None => self.own_frame().and_then(|f| {
                            self.program.def_data_labels.get(f.def as usize)?.get(&name)
                        }),
                        Some(_) => None,
                    };
                    let from = match from {
                        Some(None) => return Err(undefined()),
                        Some(Some(p)) => p,
                        None => Rc::clone(&self.program),
                    };
                    let pos = *own
                        .or_else(|| from.data_labels.get(&name))
                        .ok_or_else(undefined)?;
                    (self.data_pos, self.data_from) = (pos as usize, from);
                }
            }
            Op::Read => {
                let item = (self.data_from.data.get(self.data_pos)).ok_or(ErrorKind::OutOfData)?;
                self.data_pos += 1;
                self.stack.push(match item {
                    Literal::Int(n) => Value::Int(*n),
                    Literal::Real(x) => Value::Real(*x),
                    Literal::Str(s) => Value::Str(new_str(s.clone())),
                });
            }
            Op::End => return Ok(self.end(host)),
            Op::Nop => {}
            Op::Fail(kind, None) => return Err((*kind).into()),
            Op::Fail(kind, Some(name)) => {
                let name = String::from_utf16_lossy(&program.strings[*name as usize]);
                return Err(Error::new(*kind).of_command(&name));
            }
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
    // A slot EXEC has not run gives Undefined variable.
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
            .position(|v| v.is_named(&name))
            .ok_or_else(undefined)?;
        Ok(Some(&mut unit.globals[i]))
    }

    fn dynamic_slot(&self, name: &str) -> Result<Slot> {
        if let Some(f) = self.own_frame() {
            let locals = &self.program.defs[f.def as usize].locals;
            if let Some(i) = locals.iter().position(|v| v.is_named(name)) {
                return Ok(Slot::Local(i as u32));
            }
        }
        self.program
            .globals
            .iter()
            .position(|v| v.is_named(name))
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
            && b.form() == Form::Command
            && let Some(f) = builtins::lookup(b.name, Form::Function)
            && (b.outs.first || !b.accepts(args.len()) && builtins::get(f).accepts(args.len()))
        {
            // `HSV H,S,V OUT C`: arguments that only the function form
            // takes, with one OUT variable for its result. A builtin that
            // takes its form from the OUT count has only that one then.
            return self.call_builtin(program, f, args, Want::Value);
        }
        let outs = want.count();
        let outs_taken = b.outs.takes(outs);
        if !outs_taken && b.outs.first && !(b.outs.none_first && args.is_empty()) {
            return Err(Error::new(ErrorKind::WrongNumberOfOutArguments).of_command(b.name));
        }
        if !b.accepts(args.len()) {
            // Arguments that fit the other form make the results the
            // wrong ones: HSV(120) is read as HSV's OUT form (checked on
            // hardware).
            let other = match b.form() {
                Form::Function => Form::Command,
                Form::Command => Form::Function,
            };
            let fits_other = !b.outs.first
                && builtins::lookup(b.name, other)
                    .is_some_and(|o| builtins::get(o).accepts(args.len()));
            let kind = match fits_other {
                true => ErrorKind::WrongNumberOfOutArguments,
                false => ErrorKind::WrongNumberOfArguments,
            };
            return Err(Error::new(kind).of_command(b.name));
        }
        if !outs_taken {
            return Err(Error::new(ErrorKind::WrongNumberOfOutArguments).of_command(b.name));
        }
        let results = match b.kind {
            Kind::Function(f) => vec![f(args).map_err(|e| builtin_error(e, b.name, args))?],
            Kind::Command(f) => f(args, outs).map_err(|e| builtin_error(e, b.name, args))?,
            Kind::Data(f) => {
                let def = self.own_frame().map(|f| f.def);
                let mut data = builtins::Data {
                    program,
                    def,
                    pos: &mut self.data_pos,
                    moved: false,
                };
                let results = f(args, outs, &mut data);
                if data.moved {
                    self.data_from = Rc::clone(&self.program);
                }
                results.map_err(|e| builtin_error(e, b.name, args))?
            }
        };
        self.push_results(results, want)
    }

    /// Takes READ's place after a host call. A command that read DATA
    /// from a `"@label"` moved it, into this slot's program.
    #[inline(always)]
    fn data_moved(&mut self, host: &dyn Host) {
        if let Some(pos) = host.data_pos()
            && pos != self.data_pos
        {
            (self.data_pos, self.data_from) = (pos, Rc::clone(&self.program));
        }
    }

    /// [`Op::CallHost`] at `pc`, for [`Vm::step`] and [`Vm::run_fast`]: the
    /// arguments stay on the stack for the call, and an error takes them off.
    #[inline(never)]
    fn host_call(
        &mut self,
        host: &mut dyn Host,
        id: u32,
        argc: u8,
        want: Want,
    ) -> Result<Option<Status>> {
        let at = self.stack.len() - usize::from(argc);
        if self.values_room_taken(usize::from(argc)) {
            return Err(self.failed_with_args(ErrorKind::StackOverflow.into(), at));
        }
        if self.report_work {
            host.work(self.work());
        }
        host.set_data_pos(self.data_pos, self.own_frame().map(|f| f.def));
        let results = host.call(id, &self.stack[at..], want);
        self.data_moved(host);
        let results = match results {
            Ok(results) => results,
            Err(e) => {
                let e = e.in_args(&self.stack[at..]);
                return Err(self.failed_with_args(e, at));
            }
        };
        if host.retry() {
            return Ok(Some(RETRY));
        }
        self.stack.truncate(at);
        // As `push_results`, without going through a list for none or one.
        if results.len() != want.count() {
            return Err(ErrorKind::IllegalFunctionCall.into());
        }
        let mut results = results;
        match results.len() {
            0 => {}
            1 => self.stack.push(results.pop().expect("one value")),
            _ => self.stack.extend(results),
        }
        self.pc += 1;
        if self.report_work && host.out_of_time() {
            return Ok(Some(Status::Running));
        }
        Ok(None)
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
        if self.values_room_taken(args.len()) {
            return Err(ErrorKind::StackOverflow.into());
        }
        host.set_data_pos(self.data_pos, self.own_frame().map(|f| f.def));
        let results = host.call(id, args, want);
        self.data_moved(host);
        let results = results.map_err(|e| e.in_args(args))?;
        if host.retry() {
            return Ok(false);
        }
        self.push_results(results, want)?;
        Ok(true)
    }

    /// Calls DEF `def` of the running slot, returning to slot `caller`.
    fn enter_def(&mut self, def: u32, args: Vec<Value>, want: Want, caller: usize) -> Result<()> {
        let argc = args.len();
        self.stack.extend(args);
        self.enter_def_on_stack(def, argc, want, caller)
    }

    /// [`Vm::enter_def`] with the arguments on top of the stack, which it takes.
    fn enter_def_on_stack(
        &mut self,
        def: u32,
        argc: usize,
        want: Want,
        caller: usize,
    ) -> Result<()> {
        let at = self.stack.len() - argc;
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
        let args_ok = info.params.is_none_or(|p| usize::from(p) == argc);
        // A DEF given the wrong number of arguments is the same error as a
        // builtin.
        // Several OUTs for one value is the wrong number of them.
        let too_many_outs = want == Want::Value && info.outs.is_some_and(|m| m > 1);
        if !call_ok || !args_ok || too_many_outs {
            let kind = match (call_ok, too_many_outs) {
                (_, true) => ErrorKind::WrongNumberOfOutArguments,
                (true, false) => ErrorKind::WrongNumberOfArguments,
                (false, false) => ErrorKind::IllegalFunctionCall,
            };
            let e = Error::with_detail(kind, info.name.clone());
            self.stack.truncate(at);
            return Err(e);
        }
        let cost = call_cost(info, argc);
        if !self.stack_has_room(cost) {
            if self.stack_has_room(cost - VALUES_ROOM.min(cost)) {
                self.pc = info.entry as usize;
            }
            self.stack.truncate(at);
            return Err(ErrorKind::StackOverflow.into());
        }
        let program = Rc::clone(&self.program);
        self.push_frame(&program.defs[def as usize], def, argc, want, caller, cost);
        Ok(())
    }

    /// Starts a call of DEF `def` of the running slot, `info`, which has room,
    /// taking its arguments off the stack.
    #[inline(always)]
    fn push_frame(
        &mut self,
        info: &DefInfo,
        def: u32,
        argc: usize,
        want: Want,
        caller: usize,
        cost: usize,
    ) {
        self.def_calls += 1;
        let at = self.stack.len() - argc;
        let stack_to = self.frames.last().map_or(0, |f| f.stack_to) + cost;
        let base = self.locals.len();
        // Arguments of a `*` DEF are kept apart from its variables.
        let variadic_args = match info.params {
            Some(_) => {
                take_args(&mut self.stack, &mut self.locals, argc);
                Vec::new()
            }
            None => self.stack.split_off(at),
        };
        start_locals(&mut self.locals, base, info);
        let outs = match info.outs {
            None => vec![Value::Default; want.count()],
            Some(_) => Vec::new(),
        };
        self.frames.push(Frame {
            def,
            home: self.slot,
            caller,
            base,
            args: variadic_args,
            outs,
            want,
            return_pc: self.pc + 1,
            gosub_base: self.gosubs.len(),
            stack_base: self.stack.len(),
            stack_to,
        });
        self.pc = info.entry as usize;
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
            // A bare RETURN in a function gives no value.
            (true, _) => vec![Value::Default],
            (false, Some(n)) => {
                let start = frame.base + usize::from(info.params.unwrap_or(0));
                self.locals[start..start + usize::from(n)].to_vec()
            }
            (false, None) => frame.outs.clone(),
        };
        if frame.want == Want::Value {
            results.truncate(1);
        }
        self.leave_def(results, host)
    }

    fn leave_def(&mut self, results: Vec<Value>, host: &mut dyn Host) -> Result<usize> {
        let frame = self.pop_frame().ok_or(ErrorKind::IllegalStatement)?;
        self.gosubs.truncate(frame.gosub_base);
        self.stack.truncate(frame.stack_base);
        self.push_results(results, frame.want)?;
        self.leave_frame(frame, host)
    }

    /// [`Vm::leave_def`] with one value, as a function returns: no list of it.
    fn leave_def_with(&mut self, result: Value, host: &mut dyn Host) -> Result<usize> {
        let frame = self.pop_frame().ok_or(ErrorKind::IllegalStatement)?;
        self.gosubs.truncate(frame.gosub_base);
        self.stack.truncate(frame.stack_base);
        if frame.want.count() != 1 {
            return Err(ErrorKind::IllegalFunctionCall.into());
        }
        self.stack.push(result);
        self.leave_frame(frame, host)
    }

    /// Ends the innermost DEF call, with its variables.
    #[inline(always)]
    fn pop_frame(&mut self) -> Option<Frame> {
        let frame = self.frames.pop()?;
        self.locals.truncate(frame.base);
        Some(frame)
    }

    /// Goes back to the caller of a frame that has ended.
    #[inline]
    fn leave_frame(&mut self, frame: Frame, host: &mut dyn Host) -> Result<usize> {
        let (caller, back) = (frame.caller, frame.return_pc);
        drop_frame(frame);
        self.switch_to(caller, host);
        Ok(back)
    }
}

/// Drops a frame that has ended, without a call for the lists most frames
/// leave empty.
#[inline(always)]
fn drop_frame(frame: Frame) {
    let Frame { args, outs, .. } = frame;
    drop_values(args);
    drop_values(outs);
}

/// How fused entries end, for [`Vm::run_fast`].
enum Fused {
    /// At an entry for `step`, or with the budget spent.
    Slow,
    Host {
        id: u32,
        argc: u8,
        want: Want,
    },
}

/// What a call of `info` with `argc` arguments, wanting `want`, takes of
/// [`STACK_SIZE`], when the DEF has fixed numbers of arguments and OUTs and
/// the call fits them as [`Vm::enter_def_on_stack`] checks. `None` for the
/// others, which the fuser leaves to it.
pub(crate) fn fixed_call_cost(info: &DefInfo, argc: u8, want: Want) -> Option<u32> {
    let (Some(params), Some(outs)) = (info.params, info.outs) else {
        return None;
    };
    let fits = match want {
        Want::Value => (info.is_function || outs > 0) && outs <= 1,
        Want::Outs(n) if info.is_function => n == 1,
        Want::Outs(n) => outs == n,
    };
    match params == argc && fits {
        true => u32::try_from(call_cost(info, argc.into())).ok(),
        false => None,
    }
}

/// The innermost DEF call's stack taken, first GOSUB and first variable, or
/// the top level's.
#[inline(always)]
fn innermost(frames: &[Frame], locals: &[Value]) -> (usize, usize, usize) {
    match frames.last() {
        Some(f) => (f.stack_to, f.gosub_base, f.base),
        None => (0, 0, locals.len()),
    }
}

/// Moves the `argc` arguments on top of the stack on to the end of a call's
/// variables.
#[inline(always)]
fn take_args(stack: &mut Vec<Value>, locals: &mut Vec<Value>, argc: usize) {
    let at = stack.len() - argc;
    move_values(&mut stack[at..], locals);
    // SAFETY: what is left from `at` on is numbers, which need no dropping.
    unsafe { stack.set_len(at) };
}

/// Gives a call's variables after its arguments, from `base` on, their
/// starting values. OUT variables hold nothing until set: a DEF that never
/// sets one gives the omitted value, as SmileBASIC Debugger's EMPTY() relies
/// on.
#[inline(always)]
fn start_locals(locals: &mut Vec<Value>, base: usize, info: &DefInfo) {
    let count = info.locals.len();
    locals.reserve(count - (locals.len() - base));
    let first_out = usize::from(info.params.unwrap_or(0));
    let out_end = first_out + usize::from(info.outs.unwrap_or(0));
    for i in locals.len() - base..count {
        locals.push(match i < out_end {
            true => Value::Default,
            false => initial_value(&info.locals[i]),
        });
    }
}

/// How much of [`STACK_SIZE`] a call of `info` takes.
#[inline(always)]
fn call_cost(info: &DefInfo, argc: usize) -> usize {
    let values = info.locals.len() + argc * usize::from(info.params.is_none());
    CALL_COST + values * VARIABLE_COST + if info.is_function { VALUE_COST } else { 0 }
}

/// Drops a list of values, without a call when it was never given room, as
/// with the `args` and `outs` of most frames.
#[inline(always)]
fn drop_values(values: Vec<Value>) {
    if values.capacity() == 0 {
        std::mem::forget(values);
    }
}

/// Moves values on to the end of `to`, numbers in parts, leaving numbers in
/// their places.
#[inline(always)]
fn move_values(from: &mut [Value], to: &mut Vec<Value>) {
    to.reserve(from.len());
    for value in from {
        match *value {
            Value::Int(n) => push_num(to, Num::Int(n)),
            Value::Real(x) => push_num(to, Num::Real(x)),
            _ => to.push(std::mem::replace(value, Value::Int(0))),
        }
    }
}

/// The variables [`Vm::run_fast`] reads and writes, by [`Var::table`].
type Vars<'a> = [&'a mut [Value]; 2];

/// The stack in use with `cost` more, from the stack the DEF calls take and
/// the GOSUBs and RESTORE "PUSH" places held.
#[inline(always)]
fn room_for(calls: usize, gosubs: usize, kept: usize, cost: usize) -> usize {
    calls + gosubs * GOSUB_COST + kept * RESTORE_COST + cost
}

/// Whether a GOSUB of [`Vm::run_fused`] fits without the values' room, which
/// it leaves to [`Vm::gosub`].
#[inline(always)]
fn gosub_fits(calls: usize, gosubs: usize, kept: usize) -> bool {
    room_for(calls, gosubs, kept, GOSUB_COST) <= STACK_SIZE
}

#[inline(always)]
fn get<'a>(vars: &'a Vars, var: Var) -> &'a Value {
    &vars[var.table()][var.index()]
}

#[inline(always)]
fn at_var<'a>(vars: &'a mut Vars, var: Var) -> &'a mut Value {
    &mut vars[var.table()][var.index()]
}

/// Pushes a number. Pushing a whole [`Value`] made just now would make it first
/// and then move it, which is what [`Num`] is there to avoid.
#[inline(always)]
fn push_num(stack: &mut Vec<Value>, n: Num) {
    if stack.len() == stack.capacity() {
        stack.reserve(1);
    }
    let len = stack.len();
    let free = &mut stack.spare_capacity_mut()[0];
    match n {
        Num::Int(n) => free.write(Value::Int(n)),
        Num::Real(x) => free.write(Value::Real(x)),
    };
    // SAFETY: the element at `len` was written just above. This is what
    // `Vec::push` does, without the move: measured at a quarter of the cycles of
    // the VREAL scene of TPERF, in the wait for a value written in parts to be
    // read whole.
    unsafe { stack.set_len(len + 1) };
}

#[inline(always)]
fn push_copy(stack: &mut Vec<Value>, value: &Value) {
    match *value {
        Value::Int(n) => push_num(stack, Num::Int(n)),
        Value::Real(x) => push_num(stack, Num::Real(x)),
        _ => stack.push(value.clone()),
    }
}

/// Copies a variable to another.
#[inline(always)]
fn copy_var(vars: &mut Vars, from: Var, to: Var) {
    match *get(vars, from) {
        Value::Int(n) => put(at_var(vars, to), Num::Int(n)),
        Value::Real(x) => put(at_var(vars, to), Num::Real(x)),
        ref value => {
            let value = value.clone();
            *at_var(vars, to) = value;
        }
    }
}

/// Puts a number where a value was.
#[inline(always)]
fn put(place: &mut Value, n: Num) {
    match n {
        Num::Int(n) => *place = Value::Int(n),
        Num::Real(x) => *place = Value::Real(x),
    }
}

/// Takes the top value off the stack, without moving a number to do it.
#[inline(always)]
fn drop_top(stack: &mut Vec<Value>) {
    let len = stack.len();
    if let Some(Value::Int(_) | Value::Real(_)) = stack.last() {
        // SAFETY: the stack has this element, and a number needs no dropping.
        // `Vec::pop` would read 16 bytes just written in parts, and wait for them:
        // with the same for swapping and negating, cycles fell 17% on the VARRAY
        // scene of TPERF, 6% on S51_MEDAL4 and 3% on VINT.
        unsafe { stack.set_len(len - 1) };
    } else {
        stack.pop();
    }
}

/// Swaps two values of the stack, writing numbers in parts.
#[inline(always)]
fn swap_values(stack: &mut [Value], a: usize, b: usize) {
    let numbers = |v: &Value| match *v {
        Value::Int(n) => Some(Num::Int(n)),
        Value::Real(x) => Some(Num::Real(x)),
        _ => None,
    };
    if let (Some(x), Some(y)) = (numbers(&stack[a]), numbers(&stack[b])) {
        put(&mut stack[a], y);
        put(&mut stack[b], x);
    } else {
        stack.swap(a, b);
    }
}

#[inline(always)]
fn top(stack: &mut [Value]) -> &mut Value {
    stack.last_mut().expect("compiler keeps the stack balanced")
}

/// What `f` makes of the two values on top of the stack.
#[inline(always)]
fn on_two<T>(stack: &[Value], f: impl FnOnce(&Value, &Value) -> T) -> T {
    let [.., under, top] = stack else {
        unreachable!("compiler keeps the stack balanced");
    };
    f(under, top)
}

/// A number's value.
#[inline(always)]
fn number(value: &Value) -> Option<Num> {
    match *value {
        Value::Int(n) => Some(Num::Int(n)),
        Value::Real(x) => Some(Num::Real(x)),
        _ => None,
    }
}

/// [`Value::is_true`] on a number.
#[inline(always)]
fn truth(value: &Value) -> Option<bool> {
    match *value {
        Value::Int(n) => Some(n != 0),
        Value::Real(x) => Some(x != 0.0),
        _ => None,
    }
}

#[inline(always)]
fn num_truth(n: Num) -> bool {
    match n {
        Num::Int(n) => n != 0,
        Num::Real(x) => x != 0.0,
    }
}

/// [`Vm::for_test`] on numbers that cannot fail.
#[inline(always)]
fn for_continues(var: &Value, to: &Value, step: &Value) -> Option<bool> {
    if let (Value::Int(v), Value::Int(t), Value::Int(s)) = (var, to, step) {
        return Some(if *s >= 0 { v <= t } else { v >= t });
    }
    let up = match *step {
        Value::Int(s) => s >= 0,
        Value::Real(s) => s >= 0.0,
        _ => return None,
    };
    let order = match up {
        true => BinaryOp::LessEq,
        false => BinaryOp::GreaterEq,
    };
    ops::binary_numbers(order, var, to).map(num_truth)
}

/// [`Value::to_int`] on a number in range.
#[inline(always)]
fn index_number(value: &Value) -> Option<i32> {
    match *value {
        Value::Int(n) => Some(n),
        Value::Real(x) if x > -2147483649.0 && x < 2147483648.0 => Some(x as i32),
        _ => None,
    }
}

/// [`index_number`] on a number.
#[inline(always)]
fn num_index(n: Num) -> Option<i32> {
    match n {
        Num::Int(n) => Some(n),
        Num::Real(x) => index_number(&Value::Real(x)),
    }
}

/// An array's number by flat position, which may be outside it. Strings are
/// left to [`index`].
#[inline(always)]
fn element_at(array: &Array, at: usize) -> Option<Num> {
    match &array.data {
        ArrayData::Int(v) => v.get(at).map(|n| Num::Int(*n)),
        ArrayData::Real(v) => v.get(at).map(|x| Num::Real(*x)),
        ArrayData::Str(_) => None,
    }
}

/// What [`index`] gives on one index, when that is a number in an array.
#[inline(always)]
fn element1(base: &Value, i: &Value) -> Option<Num> {
    let Value::Array(a) = base else {
        return None;
    };
    // A negative index goes far beyond any array here.
    element_at(&a.borrow(), index_number(i)? as u32 as usize)
}

/// The flat position of two indices, when they are in range; for an array of
/// another shape, `None`.
#[inline(always)]
fn offset2(array: &Array, i: &Value, j: &Value) -> Option<usize> {
    let &[rows, columns] = array.dims.as_slice() else {
        return None;
    };
    let (i, j) = (
        index_number(i)? as u32 as usize,
        index_number(j)? as u32 as usize,
    );
    (i < rows && j < columns).then_some(i * columns + j)
}

/// What [`index`] gives on two indices, when that is a number in an array.
#[inline(always)]
fn element2(base: &Value, i: &Value, j: &Value) -> Option<Num> {
    let Value::Array(a) = base else {
        return None;
    };
    let a = a.borrow();
    element_at(&a, offset2(&a, i, j)?)
}

/// [`Array::set`] when it cannot fail. False leaves the element alone.
#[inline(always)]
fn set_element(array: &mut Array, at: usize, value: &Value) -> bool {
    match (&mut array.data, value) {
        (ArrayData::Int(v), Value::Int(n)) => v.get_mut(at).map(|e| *e = *n).is_some(),
        (ArrayData::Real(v), Value::Real(x)) => v.get_mut(at).map(|e| *e = *x).is_some(),
        (ArrayData::Real(v), Value::Int(n)) => v.get_mut(at).map(|e| *e = f64::from(*n)).is_some(),
        (ArrayData::Int(v), Value::Real(x)) => match crate::value::stored_int(*x) {
            Some(n) => v.get_mut(at).map(|e| *e = n).is_some(),
            None => false,
        },
        _ => false,
    }
}

/// [`store_index`] on one index, for numbers in an array's range.
#[inline(always)]
fn set_element1(base: &Value, i: &Value, value: &Value) -> bool {
    let (Value::Array(a), Some(i)) = (base, index_number(i)) else {
        return false;
    };
    set_element(&mut a.borrow_mut(), i as u32 as usize, value)
}

/// [`store_index`] on two indices, for numbers in an array's range.
#[inline(always)]
fn set_element2(base: &Value, i: &Value, j: &Value, value: &Value) -> bool {
    let Value::Array(a) = base else {
        return false;
    };
    let mut a = a.borrow_mut();
    match offset2(&a, i, j) {
        Some(at) => set_element(&mut a, at, value),
        None => false,
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

/// An error from builtin `name`, which names it, with the argument that
/// was wrong when that is one of `args`. Out of memory says which it was.
#[cold]
fn builtin_error(e: Error, name: &str, args: &[Value]) -> Error {
    match e.kind {
        ErrorKind::OutOfMemory if e.detail.is_none() => Error::with_detail(e.kind, name),
        ErrorKind::MemoryCheck => e.in_args(args),
        _ => match name {
            // LEFT$'s Overflow names no argument, even its second, and
            // ENVSTAT gives none. A negative count is (LEFT$:2).
            "LEFT$" if e.kind == ErrorKind::Overflow => {
                let mut e = e.in_args(args);
                e.arg = None;
                e.of_command(name)
            }
            // SQR of a negative names neither SQR nor its argument.
            "SQR" if e.kind == ErrorKind::OutOfRange => Error::new(e.kind),
            _ => e.in_args(args).of_command(name),
        },
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
            if piece.len() > 1 {
                memory::take(memory::string_bytes(s.len() + piece.len() - 1))?;
            }
            s.splice(i..=i, piece);
            Ok(())
        }
        _ => Err(ErrorKind::TypeMismatch.into()),
    }
}

/// The elements `FILL array, value[, start[, count]]` writes.
fn fill_count(args: &[Value]) -> u64 {
    let Some(Value::Array(a)) = args.first() else {
        return 0;
    };
    let arg = |i: usize| args.get(i).and_then(|v| v.to_int().ok());
    let start = arg(2).map_or(0, |n| n.max(0) as usize);
    let count = arg(3).map_or(usize::MAX, |n| n.max(0) as usize);
    let end = start.saturating_add(count).min(a.borrow().len());
    end.saturating_sub(start) as u64
}
