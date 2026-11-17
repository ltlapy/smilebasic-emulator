//! Native code for the fused entries, with the `jit` feature (off by
//! default): see docs/jit-design.md.
//!
//! When the VM takes a program, after [`crate::fuse::check`], [`attach`]
//! picks the loops whose entries all have a native form, compiles each with
//! `sb_jit` into one function, and puts [`FastOp::Native`] in the place of
//! the loop's first entry. `run_fused` calls the function there; it runs
//! entries until one it does not have, a step that cannot do its work, or
//! the end of the budget, and leaves `pc` and the budget as the fused
//! entries would have. A step that cannot do its work ends `run_fused` as a
//! chain's does, and `step` goes on from the statement's start.
//!
//! What each entry does natively is worked out here from the same entries
//! and chain steps the interpreter runs, and the operators' rules are
//! `ops::binary_numbers`'s: the tests in this file run every operator the
//! native code has on a table of values both ways.
//!
//! PROTOTYPE LIMITS, each needed by the full version:
//! - only loops whose every entry is a chain or a stack-free entry of
//!   numbers are compiled: no array elements, builtins, DEF calls, GOSUBs,
//!   host calls or stack entries;
//! - a FOR's NEXT has a native form for integers only, and a region that
//!   keeps failing is left to the interpreter for good, which then runs
//!   the loop's first instruction with `step` each time round: the full
//!   version needs the replaced entry back, without a test on every entry;
//! - native code is made for every loop when the VM takes the program, with
//!   no measure of which loops are hot.

use std::cell::Cell;
use std::collections::VecDeque;
use std::rc::Rc;
use std::time::Instant;

use sb_jit as j;

use crate::ast::{BinaryOp, UnaryOp};
use crate::bytecode::{Compiled, Literal};
use crate::fuse::{FastOp, Micro, Var};
use crate::value::Value;
use crate::vars::Vars;

/// The most entries a region takes. Others are left out of it and reached
/// through the interpreter.
const MOST_ENTRIES: usize = 64;

/// A region that has failed this often, and on at least one run in
/// [`FAILING_SHARE`], is left to the interpreter from then on.
const MOST_FAILURES: u64 = 16;
const FAILING_SHARE: u64 = 8;

/// Whether programs the VM takes from now on, on this thread, get native
/// code. On unless turned off, for measuring and testing the same build
/// both ways.
pub fn set_enabled(on: bool) {
    ENABLED.with(|e| e.set(on));
}

/// What making native code has cost on this thread.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Stats {
    /// Regions compiled, the entries in them, and the bytes of code.
    pub regions: u64,
    pub entries: u64,
    pub bytes: u64,
    /// Regions that did not compile.
    pub refused: u64,
    /// Time to work out the regions and compile them, in nanoseconds, and
    /// the longest for one region.
    pub nanos: u64,
    pub most_nanos: u64,
    /// Time to describe the machine to Cranelift, once a thread, before
    /// the first program: not in `nanos`.
    pub setup_nanos: u64,
}

thread_local! {
    static STATS: Cell<Stats> = Cell::new(Stats::default());
    static ENABLED: Cell<bool> = const { Cell::new(true) };
}

pub fn stats() -> Stats {
    STATS.with(Cell::get)
}

struct Region {
    code: j::Code,
    runs: Cell<u64>,
    failures: Cell<u64>,
}

/// A program's native code, kept with it in [`Compiled::native`]. Cloning
/// the program shares the code. Programs compare equal whatever their
/// native code.
#[derive(Clone, Default)]
pub struct Native {
    regions: Option<Rc<[Region]>>,
}

impl std::fmt::Debug for Native {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let n = self.regions.as_ref().map_or(0, |r| r.len());
        write!(f, "Native({n} regions)")
    }
}

impl PartialEq for Native {
    fn eq(&self, _: &Native) -> bool {
        true
    }
}

/// What came of entering native code.
pub(crate) enum Entered {
    /// It ran up to `pc`, which the interpreter goes on from.
    Ran { pc: usize, left: u64 },
    /// A step could not do its work: `step` goes on from `pc`.
    Failed { pc: usize, left: u64 },
    /// Not entered: `step` runs the instruction, as for a failure.
    Not,
}

impl Native {
    /// How many times native code has been entered.
    pub fn runs(&self) -> u64 {
        self.regions
            .iter()
            .flat_map(|r| r.iter())
            .map(|r| r.runs.get())
            .sum()
    }

    /// How many regions there are.
    pub fn regions(&self) -> usize {
        self.regions.as_ref().map_or(0, |r| r.len())
    }

    /// Runs region `region` from `pc`, with `left` of the budget, on the
    /// variables `vars` sees.
    #[inline]
    pub(crate) fn enter(&self, region: u32, vars: &mut Vars, pc: usize, left: u64) -> Entered {
        let Some(r) = (self.regions.as_deref()).and_then(|r| r.get(region as usize)) else {
            return Entered::Not;
        };
        let (runs, failures) = (r.runs.get(), r.failures.get());
        if failures >= MOST_FAILURES && failures * FAILING_SHARE >= runs {
            return Entered::Not;
        }
        let [(globals, globals_len), (locals, locals_len)] = vars.native_tables();
        let need = r.code.need();
        if globals_len < need[0] || locals_len < need[1] {
            return Entered::Not;
        }
        let mut ctx = j::Ctx {
            tables: [globals.cast(), locals.cast()],
            pc: pc as u64,
            left,
        };
        // SAFETY: the tables are the ones `vars` holds the only borrows of,
        // for as long as this call, and each has at least the places the
        // code names, checked just above; `Value` is laid out as `LAYOUT`
        // says, which the tests below check. The code touches nothing else
        // (sb_jit) and calls nothing, so nothing can unwind through it.
        let status = unsafe { r.code.run(&mut ctx) };
        r.runs.set(runs + 1);
        let (pc, left) = (ctx.pc as usize, ctx.left);
        match status {
            j::EXIT => Entered::Ran { pc, left },
            _ => {
                r.failures.set(failures + 1);
                Entered::Failed { pc, left }
            }
        }
    }
}

/// [`Value`] in memory, as `#[repr(u8)]` makes it: a tag of the variant's
/// number in declaration order at offset 0, then each variant's fields as
/// a `#[repr(C)]` struct after the tag would hold them.
fn layout() -> j::Layout {
    #[repr(C)]
    struct IntVariant {
        tag: u8,
        n: i32,
    }
    #[repr(C)]
    struct RealVariant {
        tag: u8,
        x: f64,
    }
    j::Layout {
        size: std::mem::size_of::<Value>() as u32,
        int_tag: 1,
        real_tag: 2,
        first_heap_tag: 3,
        int_offset: std::mem::offset_of!(IntVariant, n) as u32,
        real_offset: std::mem::offset_of!(RealVariant, x) as u32,
    }
}

/// An entry's native form, and the addresses it can go on to.
struct Lowered {
    entry: j::Entry,
    next: Vec<usize>,
}

/// Compiles the loops of `program` whose entries all have native forms,
/// and puts [`FastOp::Native`] at each one's start. Runs after
/// [`crate::fuse::check`], on a program not yet shared.
pub(crate) fn attach(program: &mut Compiled) {
    program.native = Native::default();
    if !ENABLED.with(Cell::get) || cfg!(miri) || !j::SUPPORTED || program.fast.is_empty() {
        return;
    }
    let mut stats = stats();
    if stats.setup_nanos == 0 {
        let at = Instant::now();
        if j::prepare().is_err() {
            return;
        }
        stats.setup_nanos = (at.elapsed().as_nanos() as u64).max(1);
    }
    let started = Instant::now();
    let layout = layout();
    let n = program.fast.len();
    let lowered: Vec<Option<Lowered>> = (0..n).map(|pc| lower(program, pc)).collect();
    // A loop starts where an entry goes back to, and runs to the last entry
    // that goes back to its start. A region that does not come back to its
    // start saves less than entering native code costs, and what follows
    // the loop is another loop's or the interpreter's. Found in one pass:
    // looking for each loop's end from the program's end took time in the
    // number of loops times the program's length, 27 seconds for a program
    // of 20000 short loops.
    let mut ends: Vec<Option<usize>> = vec![None; n];
    for (pc, l) in lowered.iter().enumerate() {
        for &to in l.iter().flat_map(|l| &l.next) {
            if to <= pc && lowered.get(to).is_some_and(Option::is_some) {
                ends[to] = Some(pc);
            }
        }
    }
    let mut regions = Vec::new();
    for (head, end) in (0..n).filter_map(|pc| Some((pc, ends[pc]?))) {
        let region = gather(head, end, &lowered);
        let closes = (region.entries.iter())
            .any(|e| (lowered[e.pc as usize].as_ref()).is_some_and(|l| l.next.contains(&head)));
        if !closes {
            continue;
        }
        let at = Instant::now();
        let compiled = j::compile(&region, &layout);
        let nanos = at.elapsed().as_nanos() as u64;
        stats.most_nanos = stats.most_nanos.max(nanos);
        let Ok(code) = compiled else {
            stats.refused += 1;
            continue;
        };
        let Ok(index) = u32::try_from(regions.len()) else {
            break;
        };
        stats.regions += 1;
        stats.entries += region.entries.len() as u64;
        stats.bytes += code.size() as u64;
        regions.push(Region {
            code,
            runs: Cell::new(0),
            failures: Cell::new(0),
        });
        program.fast[head].op = FastOp::Native(index);
    }
    stats.nanos += started.elapsed().as_nanos() as u64;
    STATS.with(|s| s.set(stats));
    if !regions.is_empty() {
        program.native = Native {
            regions: Some(regions.into()),
        };
    }
}

/// The region of the loop from `head` to `end`: the entries with native
/// forms it reaches there, up to [`MOST_ENTRIES`].
fn gather(head: usize, end: usize, lowered: &[Option<Lowered>]) -> j::Region {
    let mut taken = vec![false; end + 1 - head];
    let mut queue = VecDeque::from([head]);
    let mut entries = Vec::new();
    taken[0] = true;
    while let Some(pc) = queue.pop_front() {
        let Some(l) = &lowered[pc] else { continue };
        entries.push(l.entry.clone());
        for &to in &l.next {
            if (head..=end).contains(&to)
                && !taken[to - head]
                && lowered[to].is_some()
                && entries.len() + queue.len() < MOST_ENTRIES
            {
                taken[to - head] = true;
                queue.push_back(to);
            }
        }
    }
    j::Region { entries }
}

/// The operands and results of one program's entries, as native code names
/// them.
struct Places<'a> {
    globals: usize,
    constants: &'a [Literal],
}

impl Places<'_> {
    /// A place to read: a variable, a chain's hidden place, or the number
    /// a constant's place holds, which is never written.
    fn operand(&self, v: Var) -> Option<j::Operand> {
        let index = v.index();
        if v.table() == 1 {
            return Some(j::Operand::Place(place(1, index)?));
        }
        let Some(k) = index.checked_sub(self.globals) else {
            return Some(j::Operand::Place(place(0, index)?));
        };
        if k < crate::fuse::TEMPS {
            return Some(j::Operand::Temp(k as u8));
        }
        match self.constants.get(k)? {
            Literal::Int(n) => Some(j::Operand::Int(*n)),
            Literal::Real(x) => Some(j::Operand::Real(*x)),
            Literal::Str(_) => None,
        }
    }

    fn dest(&self, v: Var) -> Option<j::Dest> {
        match self.operand(v)? {
            j::Operand::Place(p) => Some(j::Dest::Place(p)),
            j::Operand::Temp(k) => Some(j::Dest::Temp(k)),
            _ => None,
        }
    }
}

fn place(table: u8, index: usize) -> Option<j::Place> {
    let index = u32::try_from(index).ok()?;
    Some(j::Place { table, index })
}

fn bin_op(op: BinaryOp) -> Option<j::BinOp> {
    use BinaryOp as B;
    use j::BinOp as J;
    Some(match op {
        B::Add => J::Add,
        B::Sub => J::Sub,
        B::Mul => J::Mul,
        B::Div => J::Div,
        B::IntDiv => J::IntDiv,
        B::Mod => J::Mod,
        B::And => J::And,
        B::Or => J::Or,
        B::Xor => J::Xor,
        B::Shl => J::Shl,
        B::Shr => J::Shr,
        B::UShr => J::UShr,
        B::Eq => J::Eq,
        B::NotEq => J::NotEq,
        B::Less => J::Less,
        B::LessEq => J::LessEq,
        B::Greater => J::Greater,
        B::GreaterEq => J::GreaterEq,
        B::RotL | B::RotR | B::LogicalAnd | B::LogicalOr => return None,
    })
}

fn un_op(op: UnaryOp) -> j::UnOp {
    match op {
        UnaryOp::Neg => j::UnOp::Neg,
        UnaryOp::Not => j::UnOp::Not,
        UnaryOp::LogicalNot => j::UnOp::LogicalNot,
    }
}

/// The native form of the entry at `pc`, if it has one.
fn lower(program: &Compiled, pc: usize) -> Option<Lowered> {
    use FastOp as F;
    let p = Places {
        globals: program.globals.len(),
        constants: &program.constants,
    };
    let fast = program.fast[pc];
    let len = u32::from(fast.len);
    let after = pc + usize::from(fast.len);
    let at = |pc: usize| u32::try_from(pc).ok();
    let binary = |op: BinaryOp, a: Var, b: j::Operand, to: Var| {
        Some(j::Step::Binary {
            op: bin_op(op)?,
            a: p.operand(a)?,
            b,
            to: p.dest(to)?,
        })
    };
    // A jump unless (or when) `a op b`.
    let compare = |op: BinaryOp, a: Var, b: j::Operand, target: u32, when: bool| {
        let op = bin_op(op).filter(|op| op.compares())?;
        Some(j::Step::Jump {
            cond: j::Cond::Compare {
                op,
                a: p.operand(a)?,
                b,
            },
            target,
            when,
        })
    };
    let int = j::Operand::Int;
    let (steps, next) = match fast.op {
        F::Nop => (vec![], after),
        F::Jump(to) => (vec![], to as usize),
        F::Move { from, to } => (
            vec![j::Step::Move {
                from: p.operand(from)?,
                to: p.dest(to)?,
            }],
            after,
        ),
        F::AddStore(a, b, to) => (vec![binary(BinaryOp::Add, a, p.operand(b)?, to)?], after),
        F::SubStore(a, b, to) => (vec![binary(BinaryOp::Sub, a, p.operand(b)?, to)?], after),
        F::MulStore(a, b, to) => (vec![binary(BinaryOp::Mul, a, p.operand(b)?, to)?], after),
        F::AddStoreInt(a, k, to) => (vec![binary(BinaryOp::Add, a, int(k), to)?], after),
        F::SubStoreInt(a, k, to) => (vec![binary(BinaryOp::Sub, a, int(k), to)?], after),
        F::MulStoreInt(a, k, to) => (vec![binary(BinaryOp::Mul, a, int(k), to)?], after),
        F::AddStoreReal(a, x, to) => {
            let x = j::Operand::Real(crate::fuse::real_of(x));
            (vec![binary(BinaryOp::Add, a, x, to)?], after)
        }
        F::VarsStore { op, a, b, to } => (vec![binary(op, a, p.operand(b)?, to)?], after),
        F::UnlessEq(a, b, t) => (
            vec![compare(BinaryOp::Eq, a, p.operand(b)?, t, false)?],
            after,
        ),
        F::UnlessNotEq(a, b, t) => (
            vec![compare(BinaryOp::NotEq, a, p.operand(b)?, t, false)?],
            after,
        ),
        F::UnlessLess(a, b, t) => (
            vec![compare(BinaryOp::Less, a, p.operand(b)?, t, false)?],
            after,
        ),
        F::UnlessLessEq(a, b, t) => (
            vec![compare(BinaryOp::LessEq, a, p.operand(b)?, t, false)?],
            after,
        ),
        F::UnlessGreater(a, b, t) => (
            vec![compare(BinaryOp::Greater, a, p.operand(b)?, t, false)?],
            after,
        ),
        F::UnlessGreaterEq(a, b, t) => (
            vec![compare(BinaryOp::GreaterEq, a, p.operand(b)?, t, false)?],
            after,
        ),
        F::UnlessEqInt(a, k, t) => (vec![compare(BinaryOp::Eq, a, int(k), t, false)?], after),
        F::UnlessNotEqInt(a, k, t) => (vec![compare(BinaryOp::NotEq, a, int(k), t, false)?], after),
        F::UnlessLessInt(a, k, t) => (vec![compare(BinaryOp::Less, a, int(k), t, false)?], after),
        F::UnlessLessEqInt(a, k, t) => {
            (vec![compare(BinaryOp::LessEq, a, int(k), t, false)?], after)
        }
        F::UnlessGreaterInt(a, k, t) => (
            vec![compare(BinaryOp::Greater, a, int(k), t, false)?],
            after,
        ),
        F::UnlessGreaterEqInt(a, k, t) => (
            vec![compare(BinaryOp::GreaterEq, a, int(k), t, false)?],
            after,
        ),
        F::VarsJump { op, a, b, target } => {
            (vec![compare(op, a, p.operand(b)?, target, false)?], after)
        }
        F::VarsJumpTrue { op, a, b, target } => {
            (vec![compare(op, a, p.operand(b)?, target, true)?], after)
        }
        F::ForNext {
            var,
            step,
            step_slot,
            to,
            to_slot,
            exit,
            apart: true,
        } => {
            let slot = |v: Var| match p.operand(v)? {
                j::Operand::Place(place) => Some(place),
                _ => None,
            };
            // A real STEP or TO, or a hidden place, leaves it to the
            // interpreter.
            let value = |v: Var| match p.operand(v)? {
                op @ (j::Operand::Place(_) | j::Operand::Int(_)) => Some(op),
                _ => None,
            };
            let body = j::Body::ForNext {
                var: slot(var)?,
                step: value(step)?,
                step_slot: slot(step_slot)?,
                to: value(to)?,
                to_slot: slot(to_slot)?,
                exit,
                cont: at(after)?,
            };
            let entry = j::Entry {
                pc: at(pc)?,
                len,
                body,
            };
            return Some(Lowered {
                entry,
                next: vec![after, exit as usize],
            });
        }
        F::Chain { first, count, next } => {
            let first = first as usize;
            let micro = program.micro.get(first..first + usize::from(count))?;
            let steps = (micro.iter().map(|m| lower_micro(&p, m))).collect::<Option<Vec<_>>>()?;
            (steps, next as usize)
        }
        _ => return None,
    };
    let mut ahead = vec![next];
    ahead.extend(steps.iter().filter_map(|s| match s {
        j::Step::Jump { target, .. } => Some(*target as usize),
        _ => None,
    }));
    let body = j::Body::Steps {
        steps,
        next: at(next)?,
    };
    let entry = j::Entry {
        pc: at(pc)?,
        len,
        body,
    };
    Some(Lowered { entry, next: ahead })
}

fn lower_micro(p: &Places, m: &Micro) -> Option<j::Step> {
    use BinaryOp as B;
    use Micro as M;
    let binary = |op: BinaryOp, a: Var, b: j::Operand, to: Var| {
        Some(j::Step::Binary {
            op: bin_op(op)?,
            a: p.operand(a)?,
            b,
            to: p.dest(to)?,
        })
    };
    let vars = |op: BinaryOp, a: Var, b: Var, to: Var| binary(op, a, p.operand(b)?, to);
    let unary = |op: j::UnOp, a: Var, to: Var| {
        Some(j::Step::Unary {
            op,
            a: p.operand(a)?,
            to: p.dest(to)?,
        })
    };
    let jump = |op: BinaryOp, a: Var, b: j::Operand, target: u32, when: bool| {
        let op = bin_op(op)?;
        Some(j::Step::Jump {
            cond: j::Cond::Compare {
                op,
                a: p.operand(a)?,
                b,
            },
            target,
            when,
        })
    };
    let int = j::Operand::Int;
    let real = j::Operand::Real;
    match *m {
        M::Add(a, b, to) => vars(B::Add, a, b, to),
        M::Sub(a, b, to) => vars(B::Sub, a, b, to),
        M::Mul(a, b, to) => vars(B::Mul, a, b, to),
        M::Div(a, b, to) => vars(B::Div, a, b, to),
        M::IntDiv(a, b, to) => vars(B::IntDiv, a, b, to),
        M::Mod(a, b, to) => vars(B::Mod, a, b, to),
        M::And(a, b, to) => vars(B::And, a, b, to),
        M::Or(a, b, to) => vars(B::Or, a, b, to),
        M::Xor(a, b, to) => vars(B::Xor, a, b, to),
        M::Shl(a, b, to) => vars(B::Shl, a, b, to),
        M::Shr(a, b, to) => vars(B::Shr, a, b, to),
        M::Eq(a, b, to) => vars(B::Eq, a, b, to),
        M::NotEq(a, b, to) => vars(B::NotEq, a, b, to),
        M::Less(a, b, to) => vars(B::Less, a, b, to),
        M::LessEq(a, b, to) => vars(B::LessEq, a, b, to),
        M::Greater(a, b, to) => vars(B::Greater, a, b, to),
        M::GreaterEq(a, b, to) => vars(B::GreaterEq, a, b, to),
        M::Binary(op, a, b, to) => vars(op, a, b, to),
        M::AddInt(a, k, to) => binary(B::Add, a, int(k), to),
        M::SubInt(a, k, to) => binary(B::Sub, a, int(k), to),
        M::MulInt(a, k, to) => binary(B::Mul, a, int(k), to),
        M::AndInt(a, k, to) => binary(B::And, a, int(k), to),
        M::BinaryInt(op, a, k, to) => binary(op, a, int(k), to),
        M::AddReal(a, x, to) => binary(B::Add, a, real(x), to),
        M::SubReal(a, x, to) => binary(B::Sub, a, real(x), to),
        M::MulReal(a, x, to) => binary(B::Mul, a, real(x), to),
        M::BinaryReal(op, a, x, to) => binary(op, a, real(x), to),
        M::Neg(a, to) => unary(j::UnOp::Neg, a, to),
        M::Unary(op, a, to) => unary(un_op(op), a, to),
        M::Truthy(a, to) => unary(j::UnOp::Truthy, a, to),
        M::Move { from, to } => Some(j::Step::Move {
            from: p.operand(from)?,
            to: p.dest(to)?,
        }),
        M::JumpIfFalse { cond, target } => Some(j::Step::Jump {
            cond: j::Cond::Truth(p.operand(cond)?),
            target,
            when: false,
        }),
        M::JumpIfTrue { cond, target } => Some(j::Step::Jump {
            cond: j::Cond::Truth(p.operand(cond)?),
            target,
            when: true,
        }),
        M::JumpEq(j) => jump(B::Eq, j.a, p.operand(j.b)?, j.target, j.when),
        M::JumpNotEq(j) => jump(B::NotEq, j.a, p.operand(j.b)?, j.target, j.when),
        M::JumpLess(j) => jump(B::Less, j.a, p.operand(j.b)?, j.target, j.when),
        M::JumpLessEq(j) => jump(B::LessEq, j.a, p.operand(j.b)?, j.target, j.when),
        M::JumpGreater(j) => jump(B::Greater, j.a, p.operand(j.b)?, j.target, j.when),
        M::JumpGreaterEq(j) => jump(B::GreaterEq, j.a, p.operand(j.b)?, j.target, j.when),
        M::JumpEqInt(j) => jump(B::Eq, j.a, int(j.k), j.target, j.when),
        M::JumpNotEqInt(j) => jump(B::NotEq, j.a, int(j.k), j.target, j.when),
        M::JumpLessInt(j) => jump(B::Less, j.a, int(j.k), j.target, j.when),
        M::JumpLessEqInt(j) => jump(B::LessEq, j.a, int(j.k), j.target, j.when),
        M::JumpGreaterInt(j) => jump(B::Greater, j.a, int(j.k), j.target, j.when),
        M::JumpGreaterEqInt(j) => jump(B::GreaterEq, j.a, int(j.k), j.target, j.when),
        M::Commit { to, charged } => Some(j::Step::Commit {
            to,
            charged: u32::from(charged),
        }),
        // PROTOTYPE LIMIT: array elements and builtins need calls out of
        // native code, and a pushed value the stack.
        M::Index1 { .. }
        | M::Index2 { .. }
        | M::Builtin { .. }
        | M::StoreIndex1 { .. }
        | M::StoreIndex2 { .. }
        | M::Push(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ops;

    /// What `LAYOUT` says of `Value`, checked on the values themselves.
    #[test]
    fn values_are_laid_out_as_native_code_reads_them() {
        let l = layout();
        let tag = |v: &Value| {
            // SAFETY: test only. A `#[repr(u8)]` enum's first byte is its
            // tag, which is always initialized.
            unsafe { *(v as *const Value).cast::<u8>() }
        };
        assert_eq!(tag(&Value::Default), 0);
        assert_eq!(tag(&Value::Int(-7)), l.int_tag);
        assert_eq!(tag(&Value::Real(0.5)), l.real_tag);
        assert_eq!(tag(&Value::str("A")), 3);
        assert!(l.first_heap_tag <= 3);
        let int = Value::Int(-123_456_789);
        let real = Value::Real(-1.25e300);
        // SAFETY: test only: the payloads where the layout says they are,
        // initialized in the variants they belong to.
        let (n, x) = unsafe {
            let n = *(&int as *const Value)
                .cast::<u8>()
                .add(l.int_offset as usize)
                .cast::<i32>();
            let x = *(&real as *const Value)
                .cast::<u8>()
                .add(l.real_offset as usize)
                .cast::<f64>();
            (n, x)
        };
        assert_eq!((n, x), (-123_456_789, -1.25e300));
        assert_eq!(l.size as usize, std::mem::size_of::<Value>());
    }

    const OPS: [BinaryOp; 18] = [
        BinaryOp::Add,
        BinaryOp::Sub,
        BinaryOp::Mul,
        BinaryOp::Div,
        BinaryOp::IntDiv,
        BinaryOp::Mod,
        BinaryOp::And,
        BinaryOp::Or,
        BinaryOp::Xor,
        BinaryOp::Shl,
        BinaryOp::Shr,
        BinaryOp::UShr,
        BinaryOp::Eq,
        BinaryOp::NotEq,
        BinaryOp::Less,
        BinaryOp::LessEq,
        BinaryOp::Greater,
        BinaryOp::GreaterEq,
    ];

    fn values() -> Vec<Value> {
        let mut v: Vec<Value> = [
            i32::MIN,
            i32::MIN + 1,
            -65536,
            -2,
            -1,
            0,
            1,
            2,
            3,
            31,
            32,
            46341,
            65536,
            i32::MAX - 1,
            i32::MAX,
        ]
        .into_iter()
        .map(Value::Int)
        .collect();
        v.extend(
            [
                0.0,
                -0.0,
                0.5,
                -1.5,
                3.0,
                1e300,
                -1e300,
                2147483648.0,
                f64::INFINITY,
                f64::NEG_INFINITY,
                f64::NAN,
                -f64::NAN,
            ]
            .map(Value::Real),
        );
        v.push(Value::Default);
        v.push(Value::str("S"));
        v
    }

    /// What native code made of a value: its kind and bits, or `None`
    /// when it failed.
    fn seen(v: &Value) -> Option<(u8, u64)> {
        match *v {
            Value::Int(n) => Some((1, n as u32 as u64)),
            Value::Real(x) => Some((2, x.to_bits())),
            _ => None,
        }
    }

    /// Runs one entry on `table`: `Ok` with the place the entry wrote
    /// last, `Err` with where it failed.
    fn run_entry(steps: Vec<j::Step>, table: &mut [Value]) -> Result<u32, (u64, u64)> {
        let region = j::Region {
            entries: vec![j::Entry {
                pc: 0,
                len: 4,
                body: j::Body::Steps { steps, next: 100 },
            }],
        };
        let code = j::compile(&region, &layout()).expect("compiles");
        assert!(code.need()[0] <= table.len());
        let mut empty: [Value; 0] = [];
        let mut ctx = j::Ctx {
            tables: [table.as_mut_ptr().cast(), empty.as_mut_ptr().cast()],
            pc: 0,
            left: 10,
        };
        // SAFETY: test only: the table holds every place the steps name.
        let status = unsafe { code.run(&mut ctx) };
        match status {
            j::EXIT => {
                assert_eq!((ctx.pc, ctx.left), (100, 6));
                Ok(ctx.pc as u32)
            }
            _ => Err((ctx.pc, ctx.left)),
        }
    }

    fn at(index: u32) -> j::Place {
        j::Place { table: 0, index }
    }

    /// Every operator the native code has, on every pair of values from
    /// places and with the right side carried in the step: what it gives is
    /// what `ops::binary_numbers` gives, to the bit, or it leaves the
    /// statement to the interpreter where `binary_numbers` does.
    #[test]
    fn native_operators_agree_with_the_interpreter() {
        if !j::SUPPORTED {
            return;
        }
        let values = values();
        for op in OPS {
            let jop = bin_op(op).expect("supported");
            for a in &values {
                for b in &values {
                    let want = ops::binary_numbers(op, a, b).map(Value::from);
                    let forms = [
                        j::Operand::Place(at(1)),
                        match *b {
                            Value::Int(n) => j::Operand::Int(n),
                            Value::Real(x) => j::Operand::Real(x),
                            _ => j::Operand::Place(at(1)),
                        },
                    ];
                    for b_form in forms {
                        let mut table = [a.clone(), b.clone(), Value::Int(0)];
                        let steps = vec![j::Step::Binary {
                            op: jop,
                            a: j::Operand::Place(at(0)),
                            b: b_form,
                            to: j::Dest::Place(at(2)),
                        }];
                        let got = run_entry(steps, &mut table).ok().map(|_| table[2].clone());
                        let what = format!("{op:?} {a:?} {b:?} as {b_form:?}");
                        match (&want, &got) {
                            (Some(w), Some(g)) => assert_eq!(seen(w), seen(g), "{what}"),
                            (None, None) => {}
                            // Native code may leave more to the
                            // interpreter, never less.
                            (Some(_), None) => {}
                            (None, Some(g)) => panic!("{what}: native {g:?}, interpreter fails"),
                        }
                        // A comparison's jump goes where its value says.
                        if jop.compares() {
                            let mut table = [a.clone(), b.clone(), Value::Int(0)];
                            let steps = vec![j::Step::Jump {
                                cond: j::Cond::Compare {
                                    op: jop,
                                    a: j::Operand::Place(at(0)),
                                    b: b_form,
                                },
                                target: 50,
                                when: true,
                            }];
                            let region = j::Region {
                                entries: vec![j::Entry {
                                    pc: 0,
                                    len: 4,
                                    body: j::Body::Steps { steps, next: 100 },
                                }],
                            };
                            let code = j::compile(&region, &layout()).unwrap();
                            let mut ctx = j::Ctx {
                                tables: [table.as_mut_ptr().cast(), std::ptr::null_mut()],
                                pc: 0,
                                left: 10,
                            };
                            // SAFETY: test only, as in `run_entry`.
                            let status = unsafe { code.run(&mut ctx) };
                            let jumped = match status {
                                j::EXIT => Some(ctx.pc == 50),
                                _ => None,
                            };
                            let truth = want.as_ref().map(|w| matches!(w, Value::Int(1)));
                            match (truth, jumped) {
                                (Some(t), Some(j)) => assert_eq!(t, j, "{what} jump"),
                                (None, Some(_)) => panic!("{what}: native jumps"),
                                _ => {}
                            }
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn native_unary_operators_agree_with_the_interpreter() {
        if !j::SUPPORTED {
            return;
        }
        for op in [UnaryOp::Neg, UnaryOp::Not, UnaryOp::LogicalNot] {
            for a in values() {
                let want = ops::unary_number(op, &a).map(Value::from);
                let mut table = [a.clone(), Value::Int(0)];
                let steps = vec![j::Step::Unary {
                    op: un_op(op),
                    a: j::Operand::Place(at(0)),
                    to: j::Dest::Place(at(1)),
                }];
                let got = run_entry(steps, &mut table).ok().map(|_| table[1].clone());
                match (&want, &got) {
                    (Some(w), Some(g)) => assert_eq!(seen(w), seen(g), "{op:?} {a:?}"),
                    (None, Some(g)) => panic!("{op:?} {a:?}: native {g:?}"),
                    _ => {}
                }
            }
        }
    }

    /// A write over a string or an array is left to the interpreter, which
    /// drops it; nothing is written.
    #[test]
    fn native_code_does_not_write_over_strings() {
        if !j::SUPPORTED {
            return;
        }
        let mut table = [Value::Int(1), Value::str("KEEP")];
        let steps = vec![j::Step::Move {
            from: j::Operand::Place(at(0)),
            to: j::Dest::Place(at(1)),
        }];
        assert_eq!(run_entry(steps, &mut table), Err((0, 10)));
        assert!(matches!(&table[1], Value::Str(s) if s.borrow().len() == 4));
    }
}
