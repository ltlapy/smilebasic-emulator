//! Fused instructions: a second form of a program's code that runs faster.
//!
//! [`Compiled::code`] is kept as the compiler made it, and everything that
//! needs to see single instructions uses it: the debugger, TRACE, errors,
//! line numbers. Next to it, [`Compiled::fast`] has one entry for each
//! address. Each entry does what the instructions from its address do,
//! usually several instructions in one.
//!
//! An entry does its work only when nothing can go wrong and no partial work
//! can be seen: numbers and no failure. In any other case, the VM runs
//! `code` one instruction at a time, from the same address. So each
//! instruction has one meaning, and an error names what it always named.
//! An entry counts as the instructions it stands for, so that time slices
//! end where they would.
//!
//! Calls are the exception. A DEF call or return of the running slot is made
//! in the fused code, the same as in `code`, when the call fits the DEF. The
//! fuser checks that once. A call of a builtin or of the host,
//! [`FastOp::Call`] and [`FastOp::Host`], is made by the same code that the
//! instruction in `code` uses, so it has the same effects and errors as
//! that instruction. A chain may include the call of a function whose body
//! is one RETURN of numbers ([`FastOp::CallChain`]): its steps compute the
//! value of the body from the arguments in place, and anything else leaves
//! the call to `code`.
//!
//! Most entries combine a few stack instructions. A [`FastOp::Chain`]
//! stands for whole statements instead: the stack code of their expressions
//! is rewritten as three-address [`Micro`] steps. The steps read
//! variables and constants in place and keep their results in hidden places
//! after the constants, so that the values never go through the stack. They
//! write nothing the program can see until the last step of a statement. A
//! [`Micro::Commit`] after each statement except the last says where `code`
//! continues if a later step cannot do its work. Each address takes the
//! entry or chain that costs least up to the next jump, by rough
//! instruction counts.
//!
//! An operator with a constant number on its right, or on its left where
//! the sides can change places, carries the number in its entry or step
//! (`AddInt`, `JumpLessInt`, `AddStoreInt`, ...), because reading it from
//! its place costs as much as reading a variable. The result is what
//! reading the place of the constant would give: constants are never
//! written.
//!
//! [`check`] runs on every program the VM takes: it bounds the global
//! places that entries name, and computes the [`Fast::need`] of each entry,
//! the number of the running DEF's variables it needs. With the
//! `unchecked-vars` feature, the VM relies on both to read and write those
//! places without checking each against the length of its table
//! (`crate::vars`).

use std::collections::HashMap;

use crate::ast::{BinaryOp, UnaryOp};
use crate::bytecode::{Addr, Compiled, DefInfo, Literal, Op, Slot, Want};
use crate::value::Num;

/// A number's place, for [`FastOp`]s: a variable of the running DEF or of the
/// slot, or a constant. Constants follow the globals in the VM's table of
/// globals, so that reading any of the three is the same few instructions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Var(u16);

const LOCAL: u16 = 0x8000;

impl Var {
    fn global(i: usize) -> Option<Var> {
        u16::try_from(i).ok().filter(|&i| i < LOCAL).map(Var)
    }

    fn of(slot: Slot) -> Option<Var> {
        match slot {
            Slot::Global(i) => Var::global(i as usize),
            Slot::Local(i) => Var::global(i as usize).map(|v| Var(v.0 | LOCAL)),
        }
    }

    /// Returns the table: 0 for globals and constants, 1 for locals.
    #[inline(always)]
    pub fn table(self) -> usize {
        usize::from(self.0 >> 15)
    }

    #[inline(always)]
    pub fn index(self) -> usize {
        usize::from(self.0 & !LOCAL)
    }
}

/// The operation of an entry. Stack effects are written as in [`Op`], and a
/// [`Var`] stands for the `Int`, `Real` or `Load` that pushed it. `Slow` runs
/// from `code`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FastOp {
    Slow,
    Nop,
    Push(Var),
    /// `a, b`
    Push2(Var, Var),
    Store(Var),
    /// `from, Store(to)`
    Move {
        from: Var,
        to: Var,
    },
    Swap,
    Reverse(u32),
    Jump(Addr),
    JumpIfFalse(Addr),
    JumpIfTrue(Addr),
    /// ON GOTO and ON GOSUB, with the table of `count` `Jump`s after it.
    On {
        from: Selector,
        count: u32,
        gosub: bool,
    },
    /// GOSUB in the running slot.
    Gosub(Addr),
    /// RETURN from a GOSUB of the running slot, or from a DEF called there.
    Return,
    /// A call of a DEF of the running slot with fixed numbers of arguments
    /// and OUTs, that fits them: `cost` is the amount of the shared call
    /// stack that it takes. A call that does not fit is left to `code`,
    /// which fails.
    CallDef {
        def: u32,
        argc: u8,
        want: Want,
        cost: u32,
    },
    /// `CallDef` of one to three arguments that are each a variable or a
    /// number, `argc` of `args` in order, read in place instead of pushed:
    /// `a, CallDef`, `b, a, Swap, CallDef` or `c, b, a, Reverse(3),
    /// CallDef`.
    CallArgs {
        def: u16,
        argc: u8,
        want: Want,
        cost: u16,
        args: [Var; 3],
    },
    /// RETURN with a value from a DEF called in the running slot.
    ReturnValue,
    /// A string of [`Compiled::strings`] pushed as a new string, the same as
    /// the instruction in `code` does: a DEF's name for SPVAR, CALL or a
    /// callback lookup.
    Str(u32),
    /// CALLIDX().
    CallIdx,
    /// The next callback of CALL SPRITE or CALL TEXT, which
    /// [`crate::vm::Vm`] makes the same way as the instruction in `code`,
    /// outside the fused loop, because it asks the host.
    CallbackNext(Addr),
    /// A call of a builtin that [`FastOp::Builtin`] does not stand for, which
    /// [`crate::vm::Vm`] makes the same way as the instruction in `code`,
    /// without leaving the fused code.
    Call {
        id: u16,
        argc: u8,
        want: Want,
    },
    /// A call of a host's command or function, which [`crate::vm::Vm`]
    /// makes the same way as the instruction in `code`, without leaving
    /// the fused code.
    Host {
        id: u32,
        argc: u8,
        want: Want,
    },
    /// A call of one of the builtins [`crate::builtins::is_pure_call`]
    /// names, as a function.
    Builtin {
        id: u16,
        argc: u8,
    },
    Unary(UnaryOp),
    Truthy,
    Binary(BinaryOp),
    /// `Swap, Binary`
    Swapped(BinaryOp),
    /// `Swap, Binary, Store(to)`
    SwappedStore(BinaryOp, Var),
    /// `a, b, Binary`
    Vars {
        op: BinaryOp,
        a: Var,
        b: Var,
    },
    /// `a, b, Binary, Store(to)`
    VarsStore {
        op: BinaryOp,
        a: Var,
        b: Var,
        to: Var,
    },
    /// `VarsStore` of `+`, `-` and `*`, which programs use most, with the
    /// operator encoded in the entry kind.
    AddStore(Var, Var, Var),
    SubStore(Var, Var, Var),
    MulStore(Var, Var, Var),
    /// The same with a constant integer on the right, carried in the entry.
    AddStoreInt(Var, i32, Var),
    SubStoreInt(Var, i32, Var),
    MulStoreInt(Var, i32, Var),
    /// `AddStore` with a constant real on the right, in the form that
    /// [`real_bits`] returns: an `f64` would make every entry 4 bytes longer.
    AddStoreReal(Var, [u32; 2], Var),
    /// `VarsJump` of a comparison, with the operator encoded in the entry
    /// kind: a jump unless `a op b`.
    UnlessEq(Var, Var, Addr),
    UnlessNotEq(Var, Var, Addr),
    UnlessLess(Var, Var, Addr),
    UnlessLessEq(Var, Var, Addr),
    UnlessGreater(Var, Var, Addr),
    UnlessGreaterEq(Var, Var, Addr),
    /// The same with a constant integer on the right.
    UnlessEqInt(Var, i32, Addr),
    UnlessNotEqInt(Var, i32, Addr),
    UnlessLessInt(Var, i32, Addr),
    UnlessLessEqInt(Var, i32, Addr),
    UnlessGreaterInt(Var, i32, Addr),
    UnlessGreaterEqInt(Var, i32, Addr),
    /// `a, b, Binary, JumpIfFalse`
    VarsJump {
        op: BinaryOp,
        a: Var,
        b: Var,
        target: Addr,
    },
    /// `a, b, Binary, JumpIfTrue`: a WHEN's test.
    VarsJumpTrue {
        op: BinaryOp,
        a: Var,
        b: Var,
        target: Addr,
    },
    /// `a, Swap, Binary`: the right side is on the stack.
    Under {
        op: BinaryOp,
        a: Var,
    },
    /// `a, Swap, Binary, Store(to)`
    UnderStore {
        op: BinaryOp,
        a: Var,
        to: Var,
    },
    Index1,
    Index2,
    /// `base, i, Index(1)`
    Index1Vars {
        base: Var,
        i: Var,
    },
    /// `base, i, Index(1), Store(to)`
    Index1Store {
        base: Var,
        i: Var,
        to: Var,
    },
    /// `base, inner, i, Index(1), Index(1)`: `base[inner[i]]`.
    Index1Nested {
        base: Var,
        inner: Var,
        i: Var,
    },
    StoreIndex1,
    StoreIndex2,
    /// `base, i, StoreIndex(1)`
    StoreIndex1Vars {
        base: Var,
        i: Var,
    },
    /// `base, inner, i, Index(1), StoreIndex(1)`
    StoreIndex1Nested {
        base: Var,
        inner: Var,
        i: Var,
    },
    /// `var, to, step, ForTest, JumpIfFalse(exit)`
    ForCheck {
        var: Var,
        to: Var,
        step: Var,
        exit: Addr,
    },
    /// A [`Micro`] chain: `count` of [`Compiled::micro`] from `first`, then
    /// the code at `next` unless one of them jumps.
    Chain {
        first: u32,
        count: u16,
        next: Addr,
    },
    /// A `Chain` that makes DEF calls in its steps, with the values of the
    /// bodies computed from the arguments in place (see `inline`). It runs
    /// only when the stack has room for a call that takes `cost`, the most
    /// that any of them takes, because those calls would fail without that
    /// room. Otherwise `code` runs it.
    CallChain {
        first: u32,
        count: u16,
        next: Addr,
        cost: u16,
    },
    /// NEXT with a FOR whose TO and STEP are single values: `var, step_slot,
    /// Binary(Add), Store(var), Nop, step, Store(step_slot), to, Store(to_slot),
    /// var, to_slot, step_slot, ForTest, JumpIfFalse(exit)`.
    ForNext {
        var: Var,
        step: Var,
        step_slot: Var,
        to: Var,
        to_slot: Var,
        exit: Addr,
        /// Whether `step` and `to` are places separate from `var`,
        /// `step_slot` and `to_slot`, which is nearly always true, so that
        /// reading them before any of those is written gives the same values.
        apart: bool,
    },
    /// The start of a loop compiled to native code, the region of this
    /// number in [`Compiled::native`]. Where native code does not run,
    /// `step` runs the instruction, and the entries after it continue. Only
    /// the VM puts it there, after [`check`].
    #[cfg(feature = "jit")]
    Native(u32),
}

/// One three-address step of a [`FastOp::Chain`]. A chain stands for one or
/// more statements of stack code, each ending in one effect that can be seen:
/// a store, a jump, or a value left on the stack. Its steps read their
/// operands from variables and constants, and before a statement's last step
/// they write only the chain's own places, hidden variables after the
/// constants. So when a step cannot do its work, the statement has had no
/// visible effect, and `code` runs it from its start. Each common operator
/// has its own kind, `a op b` with `a` the left side and the result last.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Micro {
    Add(Var, Var, Var),
    Sub(Var, Var, Var),
    Mul(Var, Var, Var),
    Div(Var, Var, Var),
    IntDiv(Var, Var, Var),
    Mod(Var, Var, Var),
    And(Var, Var, Var),
    Or(Var, Var, Var),
    Xor(Var, Var, Var),
    Shl(Var, Var, Var),
    Shr(Var, Var, Var),
    Eq(Var, Var, Var),
    NotEq(Var, Var, Var),
    Less(Var, Var, Var),
    LessEq(Var, Var, Var),
    Greater(Var, Var, Var),
    GreaterEq(Var, Var, Var),
    /// The other operators.
    Binary(BinaryOp, Var, Var, Var),
    /// `a op k` with `k` a constant integer, carried in the step.
    AddInt(Var, i32, Var),
    SubInt(Var, i32, Var),
    MulInt(Var, i32, Var),
    AndInt(Var, i32, Var),
    BinaryInt(BinaryOp, Var, i32, Var),
    /// `a op x` with `x` a constant real.
    AddReal(Var, f64, Var),
    SubReal(Var, f64, Var),
    MulReal(Var, f64, Var),
    BinaryReal(BinaryOp, Var, f64, Var),
    Neg(Var, Var),
    Unary(UnaryOp, Var, Var),
    /// `base[i]` to `to`.
    Index1 {
        base: Var,
        i: Var,
        to: Var,
    },
    Index2 {
        base: Var,
        i: Var,
        j: Var,
        to: Var,
    },
    /// A builtin [`crate::builtins::is_pure_call`] names, on `argc`
    /// places in a row from `args`.
    Builtin {
        id: u16,
        argc: u8,
        args: Var,
        to: Var,
    },
    /// A number copied, or a statement's last step that stores one.
    Move {
        from: Var,
        to: Var,
    },
    /// A statement's last step, `base[i]=value`.
    StoreIndex1 {
        value: Var,
        base: Var,
        i: Var,
    },
    StoreIndex2 {
        value: Var,
        base: Var,
        i: Var,
        j: Var,
    },
    /// A statement's last step: a jump to `target` unless `cond` is true.
    JumpIfFalse {
        cond: Var,
        target: Addr,
    },
    JumpIfTrue {
        cond: Var,
        target: Addr,
    },
    /// A statement's last step: a jump to `target` when `a op b` is `when`.
    JumpEq(Jump),
    JumpNotEq(Jump),
    JumpLess(Jump),
    JumpLessEq(Jump),
    JumpGreater(Jump),
    JumpGreaterEq(Jump),
    /// The same with a constant integer on the right.
    JumpEqInt(JumpInt),
    JumpNotEqInt(JumpInt),
    JumpLessInt(JumpInt),
    JumpLessEqInt(JumpInt),
    JumpGreaterInt(JumpInt),
    JumpGreaterEqInt(JumpInt),
    /// The statements before it are done: a later step that cannot do its
    /// work leaves the rest to `step` from `to`, with `charged` of the
    /// chain's instructions run.
    Commit {
        to: Addr,
        charged: u8,
    },
    /// `a` as 0 or 1, like [`Op::Truthy`].
    Truthy(Var, Var),
    /// One of the chain's last steps: `a`, a hidden place, left on the
    /// stack.
    Push(Var),
}

/// A comparison's jump in a chain.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Jump {
    pub a: Var,
    pub b: Var,
    pub target: Addr,
    pub when: bool,
}

/// [`Jump`] with a constant integer for `b`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct JumpInt {
    pub a: Var,
    pub k: i32,
    pub target: Addr,
    pub when: bool,
}

/// Splits a real into the form that [`FastOp::AddStoreReal`] keeps.
pub fn real_bits(x: f64) -> [u32; 2] {
    let bits = x.to_bits();
    [bits as u32, (bits >> 32) as u32]
}

#[inline(always)]
pub fn real_of(bits: [u32; 2]) -> f64 {
    f64::from_bits(u64::from(bits[0]) | u64::from(bits[1]) << 32)
}

/// Where [`FastOp::On`] finds the number that picks its label.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Selector {
    /// On the stack: `On`.
    Stack,
    /// `v, On`
    Var(Var),
    /// `base, i, Index(1), On`
    Index(Var, Var),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Fast {
    pub op: FastOp,
    /// The number of instructions of `code` it stands for.
    pub len: u8,
    /// The number of the running DEF's variables it needs: one more than the
    /// highest index of the locals it and its chain name, 0 for none. With
    /// the `unchecked-vars` feature, the VM reads and writes an entry's
    /// places without checking each, after checking this against the DEF's
    /// variables, so only [`check`] sets it.
    pub need: u16,
}

/// The instructions `var, to_slot, step_slot, ForTest, JumpIfFalse`, which [`FastOp::ForNext`] leaves to
/// `code` when its values are not numbers.
pub const FOR_TEST_LEN: usize = 5;

/// Interns constants for [`Var`].
struct Constants<'a> {
    first: usize,
    found: HashMap<(bool, u64), Var>,
    list: &'a mut Vec<Literal>,
}

impl Constants<'_> {
    fn number(&mut self, key: (bool, u64), literal: Literal) -> Option<Var> {
        if let Some(&var) = self.found.get(&key) {
            return Some(var);
        }
        let var = Var::global(self.first + self.list.len())?;
        self.list.push(literal);
        self.found.insert(key, var);
        Some(var)
    }

    /// The number `v` holds, if it is a constant: never written, so that a
    /// step may carry it in itself instead of reading it.
    fn literal(&self, v: Var) -> Option<Num> {
        let k = v.index().checked_sub(self.first)?;
        if v.table() != 0 || k < TEMPS {
            return None;
        }
        match self.list.get(k)? {
            Literal::Int(n) => Some(Num::Int(*n)),
            Literal::Real(x) => Some(Num::Real(*x)),
            Literal::Str(_) => None,
        }
    }

    fn int(&self, v: Var) -> Option<i32> {
        match self.literal(v)? {
            Num::Int(n) => Some(n),
            Num::Real(_) => None,
        }
    }

    /// The place of what `op` pushes, if it is a variable or a number.
    fn source(&mut self, op: Option<&Op>) -> Option<Var> {
        match op? {
            Op::Int(n) => self.number((false, *n as u32 as u64), Literal::Int(*n)),
            Op::Real(x) => self.number((true, x.to_bits()), Literal::Real(*x)),
            Op::Load(slot) => Var::of(*slot),
            _ => None,
        }
    }
}

/// Offers each form the code from its start can take, with the number of
/// instructions each stands for.
fn forms(code: &[Op], defs: &[DefInfo], c: &mut Constants, add: &mut dyn FnMut(FastOp, usize)) {
    use FastOp as F;
    let store = |op: Option<&Op>| match op {
        Some(Op::Store(slot)) => Var::of(*slot),
        _ => None,
    };
    match code[0] {
        Op::Nop => add(F::Nop, 1),
        Op::Store(slot) => {
            if let Some(v) = Var::of(slot) {
                add(F::Store(v), 1);
            }
        }
        Op::Swap => {
            add(F::Swap, 1);
            if let Some(Op::Binary(op)) = code.get(1) {
                add(F::Swapped(*op), 2);
                if let Some(to) = store(code.get(2)) {
                    add(F::SwappedStore(*op, to), 3);
                }
            }
        }
        Op::Reverse(n) => add(F::Reverse(n), 1),
        Op::Jump(a) => add(F::Jump(a), 1),
        Op::JumpIfFalse(a) | Op::TestIfFalse(a) => add(F::JumpIfFalse(a), 1),
        Op::JumpIfTrue(a) => add(F::JumpIfTrue(a), 1),
        Op::On { count, gosub } => add(
            F::On {
                from: Selector::Stack,
                count,
                gosub,
            },
            1,
        ),
        Op::Gosub(a) => add(F::Gosub(a), 1),
        Op::Return => add(F::Return, 1),
        Op::ReturnValue => add(F::ReturnValue, 1),
        Op::Str(i) => add(F::Str(i), 1),
        Op::CallIdx => add(F::CallIdx, 1),
        Op::CallbackNext(end) => add(F::CallbackNext(end), 1),
        Op::CallDef { def, argc, want } => {
            let info = defs.get(def as usize);
            if let Some(cost) = info.and_then(|info| crate::vm::fixed_call_cost(info, argc, want)) {
                add(
                    F::CallDef {
                        def,
                        argc,
                        want,
                        cost,
                    },
                    1,
                );
            }
        }
        Op::CallHost { id, argc, want } => add(F::Host { id, argc, want }, 1),
        Op::CallBuiltin {
            id,
            argc,
            want: Want::Value,
        } if crate::builtins::is_pure_call(id, argc.into()) => add(F::Builtin { id, argc }, 1),
        Op::CallBuiltin { id, argc, want } => add(F::Call { id, argc, want }, 1),
        Op::Unary(op) => add(F::Unary(op), 1),
        Op::Truthy => add(F::Truthy, 1),
        Op::Binary(op) => add(F::Binary(op), 1),
        Op::Index(1) => add(F::Index1, 1),
        Op::Index(2) => add(F::Index2, 1),
        Op::StoreIndex(1) => add(F::StoreIndex1, 1),
        Op::StoreIndex(2) => add(F::StoreIndex2, 1),
        _ => {}
    }
    let Some(a) = c.source(code.first()) else {
        return;
    };
    add(F::Push(a), 1);
    call_args(code, defs, &[a], 1, add);
    if let Some(to) = store(code.get(1)) {
        add(F::Move { from: a, to }, 2);
    }
    if let Some(&Op::On { count, gosub }) = code.get(1) {
        let from = Selector::Var(a);
        add(F::On { from, count, gosub }, 2);
    }
    if let Some([Op::Swap, Op::Binary(op)]) = code.get(1..3) {
        let op = *op;
        add(F::Under { op, a }, 3);
        if let Some(to) = store(code.get(3)) {
            add(F::UnderStore { op, a, to }, 4);
        }
    }
    let Some(b) = c.source(code.get(1)) else {
        return;
    };
    add(F::Push2(a, b), 2);
    // Two arguments, pushed last first and put back in order.
    if let Some(Op::Swap) = code.get(2) {
        add(F::Push2(b, a), 3);
        call_args(code, defs, &[b, a], 3, add);
    }
    match code.get(2) {
        Some(Op::Binary(op)) => {
            let op = *op;
            add(F::Vars { op, a, b }, 3);
            // A constant on the left of `+` or `*` goes to the right.
            let (l, r) = match commutes(op) && c.literal(a).is_some() && c.literal(b).is_none() {
                true => (b, a),
                false => (a, b),
            };
            if let Some(to) = store(code.get(3)) {
                add(
                    match (op, c.literal(r)) {
                        (BinaryOp::Add, Some(Num::Int(k))) => F::AddStoreInt(l, k, to),
                        (BinaryOp::Sub, Some(Num::Int(k))) => F::SubStoreInt(l, k, to),
                        (BinaryOp::Mul, Some(Num::Int(k))) => F::MulStoreInt(l, k, to),
                        (BinaryOp::Add, Some(Num::Real(x))) => F::AddStoreReal(l, real_bits(x), to),
                        (BinaryOp::Add, _) => F::AddStore(a, b, to),
                        (BinaryOp::Sub, _) => F::SubStore(a, b, to),
                        (BinaryOp::Mul, _) => F::MulStore(a, b, to),
                        (op, _) => F::VarsStore { op, a, b, to },
                    },
                    4,
                );
            }
            if let Some(Op::JumpIfFalse(t) | Op::TestIfFalse(t)) = code.get(3) {
                let t = *t;
                let int = c.int(b);
                add(
                    match (op, int) {
                        (BinaryOp::Eq, Some(k)) => F::UnlessEqInt(a, k, t),
                        (BinaryOp::NotEq, Some(k)) => F::UnlessNotEqInt(a, k, t),
                        (BinaryOp::Less, Some(k)) => F::UnlessLessInt(a, k, t),
                        (BinaryOp::LessEq, Some(k)) => F::UnlessLessEqInt(a, k, t),
                        (BinaryOp::Greater, Some(k)) => F::UnlessGreaterInt(a, k, t),
                        (BinaryOp::GreaterEq, Some(k)) => F::UnlessGreaterEqInt(a, k, t),
                        (BinaryOp::Eq, _) => F::UnlessEq(a, b, t),
                        (BinaryOp::NotEq, _) => F::UnlessNotEq(a, b, t),
                        (BinaryOp::Less, _) => F::UnlessLess(a, b, t),
                        (BinaryOp::LessEq, _) => F::UnlessLessEq(a, b, t),
                        (BinaryOp::Greater, _) => F::UnlessGreater(a, b, t),
                        (BinaryOp::GreaterEq, _) => F::UnlessGreaterEq(a, b, t),
                        (op, _) => F::VarsJump {
                            op,
                            a,
                            b,
                            target: t,
                        },
                    },
                    4,
                );
            }
            if let Some(Op::JumpIfTrue(target)) = code.get(3) {
                let target = *target;
                add(F::VarsJumpTrue { op, a, b, target }, 4);
            }
        }
        Some(Op::Index(1)) => {
            add(F::Index1Vars { base: a, i: b }, 3);
            if let Some(to) = store(code.get(3)) {
                add(F::Index1Store { base: a, i: b, to }, 4);
            }
            if let Some(&Op::On { count, gosub }) = code.get(3) {
                let from = Selector::Index(a, b);
                add(F::On { from, count, gosub }, 4);
            }
        }
        Some(Op::StoreIndex(1)) => add(F::StoreIndex1Vars { base: a, i: b }, 3),
        _ => {}
    }
    let third = c.source(code.get(2));
    if let (Some(c3), Some(Op::Reverse(3))) = (third, code.get(3)) {
        call_args(code, defs, &[c3, b, a], 4, add);
    }
    if let Some(i) = third {
        let (base, inner) = (a, b);
        match code.get(3..5) {
            Some([Op::Index(1), Op::Index(1)]) => add(F::Index1Nested { base, inner, i }, 5),
            Some([Op::Index(1), Op::StoreIndex(1)]) => {
                add(F::StoreIndex1Nested { base, inner, i }, 5)
            }
            _ => {}
        }
    }
    if let Some(step) = third
        && let Some([Op::ForTest, Op::JumpIfFalse(exit)]) = code.get(3..5)
    {
        let (var, to, exit) = (a, b, *exit);
        add(
            F::ForCheck {
                var,
                to,
                step,
                exit,
            },
            5,
        );
    }
    if let Some(
        &[
            Op::Load(var),
            Op::Load(step_slot),
            Op::Binary(BinaryOp::Add),
            Op::Store(var2),
            Op::Nop,
            ref step,
            Op::Store(step_slot2),
            ref to,
            Op::Store(to_slot),
            Op::Load(var3),
            Op::Load(to_slot2),
            Op::Load(step_slot3),
            Op::ForTest,
            Op::JumpIfFalse(exit),
        ],
    ) = code.get(..14)
        && var == var2
        && var == var3
        && step_slot == step_slot2
        && step_slot == step_slot3
        && to_slot == to_slot2
        && let (Some(step), Some(to)) = (c.source(Some(step)), c.source(Some(to)))
        && let (Some(var), Some(step_slot), Some(to_slot)) =
            (Var::of(var), Var::of(step_slot), Var::of(to_slot))
    {
        let apart = [step, to]
            .iter()
            .all(|v| ![var, step_slot, to_slot].contains(v));
        add(
            F::ForNext {
                var,
                step,
                step_slot,
                to,
                to_slot,
                exit,
                apart,
            },
            14,
        );
    }
}

/// Offers [`FastOp::CallArgs`] for a call at `code[at]` of `args`, which the
/// code before it pushes.
fn call_args(
    code: &[Op],
    defs: &[DefInfo],
    args: &[Var],
    at: usize,
    add: &mut dyn FnMut(FastOp, usize),
) {
    let Some(&Op::CallDef { def, argc, want }) = code.get(at) else {
        return;
    };
    let info = defs.get(def as usize);
    let cost = info.and_then(|info| crate::vm::fixed_call_cost(info, argc, want));
    if let (Ok(def), Some(Ok(cost)), true) = (
        u16::try_from(def),
        cost.map(u16::try_from),
        usize::from(argc) == args.len(),
    ) {
        let mut all = [args[0]; 3];
        all[..args.len()].copy_from_slice(args);
        let args = all;
        add(
            FastOp::CallArgs {
                def,
                argc,
                want,
                cost,
                args,
            },
            at + 1,
        );
    }
}

/// The number of hidden places a chain has for the values it makes.
pub(crate) const TEMPS: usize = 8;

/// A chain being built: the places of the values a stack machine would
/// have, and the hidden places that hold a value still to be used.
#[derive(Clone)]
struct Chain {
    micro: Vec<Micro>,
    stack: Vec<Var>,
    /// The first hidden place.
    temps: usize,
    used: u16,
    /// Hidden places that stay used when taken off the stack: those of
    /// the arguments of a call made in the chain, which its body may read
    /// more than once.
    pinned: u16,
}

impl Chain {
    fn temp(&self, v: Var) -> Option<usize> {
        let k = v.index().wrapping_sub(self.temps);
        (v.table() == 0 && k < TEMPS).then_some(k)
    }

    fn pop(&mut self) -> Option<Var> {
        let v = self.stack.pop()?;
        if let Some(k) = self.temp(v)
            && self.pinned & (1 << k) == 0
        {
            self.used &= !(1 << k);
        }
        Some(v)
    }

    /// A free hidden place for a value made now, once its operands are taken.
    fn alloc(&mut self) -> Option<Var> {
        let k = (!self.used).trailing_zeros() as usize;
        if k >= TEMPS {
            return None;
        }
        self.used |= 1 << k;
        Var::global(self.temps + k)
    }

    /// `n` free hidden places in a row: none when `n` is more than there
    /// are, such as for `MAX` of nine arguments.
    fn alloc_run(&mut self, n: usize) -> Option<Var> {
        if n > TEMPS {
            return None;
        }
        let run = (1u16 << n) - 1;
        let k = (0..=TEMPS - n).find(|k| self.used & (run << k) == 0)?;
        self.used |= run << k;
        Var::global(self.temps + k)
    }

    /// Makes a value into a free hidden place, once its operands are taken.
    fn make(&mut self, f: impl FnOnce(Var) -> Micro) -> Option<()> {
        let to = self.alloc()?;
        self.micro.push(f(to));
        self.stack.push(to);
        Some(())
    }

    /// Returns the place where the last step puts its result, if that place
    /// is `v`.
    fn made_last(&mut self, v: Var) -> Option<&mut Var> {
        use Micro as M;
        let to = match self.micro.last_mut()? {
            M::Add(_, _, to)
            | M::Sub(_, _, to)
            | M::Mul(_, _, to)
            | M::Div(_, _, to)
            | M::IntDiv(_, _, to)
            | M::Mod(_, _, to)
            | M::And(_, _, to)
            | M::Or(_, _, to)
            | M::Xor(_, _, to)
            | M::Shl(_, _, to)
            | M::Shr(_, _, to)
            | M::Eq(_, _, to)
            | M::NotEq(_, _, to)
            | M::Less(_, _, to)
            | M::LessEq(_, _, to)
            | M::Greater(_, _, to)
            | M::GreaterEq(_, _, to)
            | M::Binary(_, _, _, to)
            | M::AddInt(_, _, to)
            | M::SubInt(_, _, to)
            | M::MulInt(_, _, to)
            | M::AndInt(_, _, to)
            | M::BinaryInt(_, _, _, to)
            | M::AddReal(_, _, to)
            | M::SubReal(_, _, to)
            | M::MulReal(_, _, to)
            | M::BinaryReal(_, _, _, to)
            | M::Neg(_, to)
            | M::Unary(_, _, to)
            | M::Truthy(_, to)
            | M::Index1 { to, .. }
            | M::Index2 { to, .. }
            | M::Builtin { to, .. } => to,
            _ => return None,
        };
        (*to == v).then_some(to)
    }
}

/// The operators whose sides can change places with the same result, to
/// bring a constant to the right: for reals too, because only one side can
/// be NaN. A constant never is.
fn commutes(op: BinaryOp) -> bool {
    use BinaryOp as B;
    matches!(
        op,
        B::Add | B::Mul | B::And | B::Or | B::Xor | B::Eq | B::NotEq
    )
}

/// `a op b` to `to`, with a constant `b` carried in the step. Comparisons
/// keep their places, so that a jump after them can include the comparison.
fn binary_micro(c: &Constants, op: BinaryOp, a: Var, b: Var, to: Var) -> Micro {
    use BinaryOp as B;
    use Micro as M;
    let (a, b) = match commutes(op) && c.literal(a).is_some() && c.literal(b).is_none() {
        true => (b, a),
        false => (a, b),
    };
    let compares = matches!(
        op,
        B::Eq | B::NotEq | B::Less | B::LessEq | B::Greater | B::GreaterEq
    );
    match (c.literal(b), compares) {
        (Some(Num::Int(k)), false) => {
            return match op {
                B::Add => M::AddInt(a, k, to),
                B::Sub => M::SubInt(a, k, to),
                B::Mul => M::MulInt(a, k, to),
                B::And => M::AndInt(a, k, to),
                op => M::BinaryInt(op, a, k, to),
            };
        }
        (Some(Num::Real(x)), false) => {
            return match op {
                B::Add => M::AddReal(a, x, to),
                B::Sub => M::SubReal(a, x, to),
                B::Mul => M::MulReal(a, x, to),
                op => M::BinaryReal(op, a, x, to),
            };
        }
        _ => {}
    }
    match op {
        B::Add => M::Add(a, b, to),
        B::Sub => M::Sub(a, b, to),
        B::Mul => M::Mul(a, b, to),
        B::Div => M::Div(a, b, to),
        B::IntDiv => M::IntDiv(a, b, to),
        B::Mod => M::Mod(a, b, to),
        B::And => M::And(a, b, to),
        B::Or => M::Or(a, b, to),
        B::Xor => M::Xor(a, b, to),
        B::Shl => M::Shl(a, b, to),
        B::Shr => M::Shr(a, b, to),
        B::Eq => M::Eq(a, b, to),
        B::NotEq => M::NotEq(a, b, to),
        B::Less => M::Less(a, b, to),
        B::LessEq => M::LessEq(a, b, to),
        B::Greater => M::Greater(a, b, to),
        B::GreaterEq => M::GreaterEq(a, b, to),
        op => M::Binary(op, a, b, to),
    }
}

/// A chain the code from an address can take.
struct Candidate {
    steps: Vec<Micro>,
    /// The number of instructions of `code` from the address it stands for,
    len: usize,
    /// and the number it counts as run: more when it makes calls.
    charge: usize,
    /// The address where it continues.
    next: Addr,
    calls: Calls,
}

/// What the calls a chain makes in its steps add to it.
#[derive(Clone, Copy, Default)]
struct Calls {
    /// The instructions of the DEFs' bodies, their RETURNs included.
    run: usize,
    /// The most stack that any one of them takes, 0 for none.
    cost: u16,
    /// Their saving against the entries that would make them, in the units
    /// of [`chain_cost`].
    saving: u32,
}

impl Calls {
    fn and(self, other: Calls) -> Calls {
        Calls {
            run: self.run + other.run,
            cost: self.cost.max(other.cost),
            saving: self.saving + other.saving,
        }
    }
}

/// The longest a chain may be, in instructions: [`Fast::len`].
const MAX_CHAIN: usize = 200;

/// The most values a chain's stack holds. Expressions need few. A deeper
/// stack is most likely a run of pushes such as those of an array literal,
/// which no chain takes, and looking for a chain from each of its addresses
/// would cost that many steps each.
const MAX_CHAIN_STACK: usize = 32;

/// The chains for the code from `code[0]` at `pc`, each a statement longer
/// than the one before. `temps` is the first of its hidden places. `whole`
/// is all of the code, which holds the bodies of the DEFs it calls.
fn chains(
    code: &[Op],
    pc: usize,
    whole: &[Op],
    defs: &[DefInfo],
    c: &mut Constants,
    temps: usize,
) -> Vec<Candidate> {
    let mut found = Vec::new();
    chain(code, pc, whole, defs, c, temps, &mut found);
    found
}

/// The result of [`Chain::expression`] for an instruction.
enum Worked {
    Done,
    /// An instruction of an expression that the chain cannot compute.
    Failed,
    /// Not an instruction of an expression.
    Other,
}

impl Chain {
    /// Translates an instruction of an expression into the chain's steps.
    fn expression(&mut self, op: &Op, c: &mut Constants) -> Worked {
        match self.expression_or_fail(op, c) {
            Some(worked) => worked,
            None => Worked::Failed,
        }
    }

    fn expression_or_fail(&mut self, op: &Op, c: &mut Constants) -> Option<Worked> {
        match *op {
            Op::Nop => {}
            Op::Int(_) | Op::Real(_) | Op::Load(_) => {
                if self.stack.len() >= MAX_CHAIN_STACK {
                    return None;
                }
                let v = c.source(Some(op))?;
                self.stack.push(v);
            }
            Op::Swap => {
                let n = self.stack.len();
                if n < 2 {
                    return None;
                }
                self.stack.swap(n - 1, n - 2);
            }
            Op::Reverse(n) => {
                let from = self.stack.len().checked_sub(n as usize)?;
                self.stack[from..].reverse();
            }
            Op::Binary(op) => {
                let b = self.pop()?;
                let a = self.pop()?;
                self.make(|to| binary_micro(c, op, a, b, to))?;
            }
            Op::Unary(op) => {
                let a = self.pop()?;
                self.make(|to| match op {
                    UnaryOp::Neg => Micro::Neg(a, to),
                    op => Micro::Unary(op, a, to),
                })?;
            }
            Op::Truthy => {
                let a = self.pop()?;
                self.make(|to| Micro::Truthy(a, to))?;
            }
            Op::Index(1) => {
                let i = self.pop()?;
                let base = self.pop()?;
                self.make(|to| Micro::Index1 { base, i, to })?;
            }
            Op::Index(2) => {
                let j = self.pop()?;
                let i = self.pop()?;
                let base = self.pop()?;
                self.make(|to| Micro::Index2 { base, i, j, to })?;
            }
            Op::CallBuiltin {
                id,
                argc,
                want: Want::Value,
            } if crate::builtins::is_pure_call(id, argc.into()) => {
                let n = usize::from(argc);
                let from = self.stack.len().checked_sub(n)?;
                let args = match n {
                    0 => Var::global(self.temps)?,
                    // One place is a slice of one already.
                    1 => self.stack[from],
                    _ => {
                        let args = self.alloc_run(n)?;
                        for (k, &from) in self.stack[from..].iter().enumerate() {
                            let to = Var::global(args.index() + k)?;
                            self.micro.push(Micro::Move { from, to });
                        }
                        args
                    }
                };
                for _ in 0..n {
                    self.pop()?;
                }
                if n > 1 {
                    self.used &= !(((1u16 << n) - 1) << (args.index() - self.temps));
                }
                self.make(|to| Micro::Builtin { id, argc, args, to })?;
            }
            _ => return Some(Worked::Other),
        }
        Some(Worked::Done)
    }
}

/// The most instructions of a DEF's body that [`inline`] accepts.
const MOST_INLINED: usize = 48;

/// The saving of a call made in a chain, in the units of [`chain_cost`],
/// besides the steps of its body: the entry of the call, the entry of its
/// RETURN, and the frames made and ended, about 300 aarch64 instructions.
const CALL_SAVING: u32 = 30;

/// Returns `ch` with the call of DEF `def` made in its steps, on the `argc`
/// values on top of its stack, and what that call adds to the chain. It is
/// for a function whose body is one RETURN of a value computed from its
/// arguments, the slot's globals and constants, the same way a chain
/// computes an expression. Under these conditions, nothing of a call can be
/// seen except its value:
///
/// - The arguments are read in place. Before the last step of its
///   statement, a chain's steps write only its hidden places, so the
///   arguments still hold the values the call would have taken.
/// - A step of the body that cannot do its work leaves the statement to
///   `code`, which makes the call: errors in the body come from there,
///   naming its lines.
/// - The chain counts the instructions of the body and its RETURN as run,
///   and runs whole or not at all, so time slices end where they would.
/// - It runs only when the stack has room for the call
///   ([`FastOp::CallChain`]); otherwise `code` fails on the call.
/// - The DEF has no variables except its arguments, so there are none to
///   initialize, and it calls nothing, so it makes no further calls.
fn inline(
    ch: &Chain,
    c: &mut Constants,
    code: &[Op],
    defs: &[DefInfo],
    def: u32,
    argc: u8,
) -> Option<(Chain, Calls)> {
    let info = defs.get(def as usize)?;
    let cost = crate::vm::fixed_call_cost(info, argc, Want::Value)?;
    let cost = u16::try_from(cost).ok()?;
    if !info.is_function || info.locals.len() != usize::from(argc) {
        return None;
    }
    let from = ch.stack.len().checked_sub(usize::from(argc))?;
    let mut ch = ch.clone();
    let args = ch.stack[from..].to_vec();
    let pinned = ch.pinned;
    for &a in &args {
        if let Some(k) = ch.temp(a) {
            ch.pinned |= 1 << k;
        }
    }
    let steps = ch.micro.len();
    let body = code.get(info.entry as usize..)?;
    for (k, op) in body.iter().enumerate().take(MOST_INLINED) {
        match *op {
            Op::Load(Slot::Local(i)) => ch.stack.push(*args.get(i as usize)?),
            Op::ReturnValue => {
                if ch.stack.len() != from + args.len() + 1 {
                    return None;
                }
                let value = ch.stack.pop()?;
                ch.stack.truncate(from);
                ch.pinned = pinned;
                for &a in &args {
                    if let Some(k) = ch.temp(a)
                        && ch.temp(value) != Some(k)
                        && pinned & (1 << k) == 0
                    {
                        ch.used &= !(1 << k);
                    }
                }
                ch.stack.push(value);
                let body_steps = u32::try_from(ch.micro.len() - steps).ok()?;
                let calls = Calls {
                    run: k + 1,
                    cost,
                    saving: CALL_SAVING + STEP_COST * body_steps,
                };
                return Some((ch, calls));
            }
            ref op => match ch.expression(op, c) {
                Worked::Done => {}
                Worked::Failed | Worked::Other => return None,
            },
        }
    }
    None
}

fn chain(
    code: &[Op],
    pc: usize,
    whole: &[Op],
    defs: &[DefInfo],
    c: &mut Constants,
    temps: usize,
    found: &mut Vec<Candidate>,
) -> Option<()> {
    let mut ch = Chain {
        micro: Vec::new(),
        stack: Vec::new(),
        temps,
        used: 0,
        pinned: 0,
    };
    let mut calls = Calls::default();
    let addr = |at: usize| Addr::try_from(pc + at).ok();
    // A store or a jump ends it. A Jump right after a store is included
    // with it.
    let after_store = |at: usize| match code.get(at + 1) {
        Some(&Op::Jump(target)) => Some((at + 2, target)),
        _ => Some((at + 1, addr(at + 1)?)),
    };
    // After a store, the chain may continue with the next statement.
    let mut stored = |ch: &mut Chain, at: usize, calls: Calls| -> Option<bool> {
        let (len, next) = after_store(at)?;
        if len > MAX_CHAIN {
            return None;
        }
        found.push(Candidate {
            steps: ch.micro.clone(),
            len,
            charge: len + calls.run,
            next,
            calls,
        });
        if len == at + 2 {
            return Some(false);
        }
        let charged = u8::try_from(len + calls.run).ok()?;
        ch.micro.push(Micro::Commit { to: next, charged });
        Some(true)
    };
    let mut end = code.len();
    // No candidate is longer than MAX_CHAIN, so there is nothing to find
    // past it. Without this limit, a run of pushes with no store or jump
    // between, such as an array literal of thousands of numbers, was
    // walked to its end from each of its addresses: time proportional to
    // the square of its length, two seconds for 16000 numbers.
    for (at, op) in code.iter().enumerate().take(MAX_CHAIN + 1) {
        match ch.expression(op, c) {
            Worked::Done => continue,
            Worked::Failed => return None,
            Worked::Other => {}
        }
        match *op {
            Op::CallDef {
                def,
                argc,
                want: Want::Value,
            } => match inline(&ch, c, whole, defs, def, argc) {
                Some((made, more)) => {
                    ch = made;
                    calls = calls.and(more);
                }
                None => {
                    end = at;
                    break;
                }
            },
            Op::Store(slot) => {
                let to = Var::of(slot)?;
                let v = ch.pop()?;
                if !ch.stack.is_empty() {
                    return None;
                }
                let made = ch.temp(v).is_some();
                match ch.made_last(v) {
                    Some(last) if made => *last = to,
                    _ => ch.micro.push(Micro::Move { from: v, to }),
                }
                if !stored(&mut ch, at, calls)? {
                    return Some(());
                }
            }
            Op::StoreIndex(n @ (1 | 2)) => {
                let j = if n == 2 { Some(ch.pop()?) } else { None };
                let i = ch.pop()?;
                let base = ch.pop()?;
                let value = ch.pop()?;
                if !ch.stack.is_empty() {
                    return None;
                }
                ch.micro.push(match j {
                    Some(j) => Micro::StoreIndex2 { value, base, i, j },
                    None => Micro::StoreIndex1 { value, base, i },
                });
                if !stored(&mut ch, at, calls)? {
                    return Some(());
                }
            }
            Op::JumpIfFalse(target) | Op::TestIfFalse(target) | Op::JumpIfTrue(target) => {
                let cond = ch.pop()?;
                if !ch.stack.is_empty() {
                    return None;
                }
                let when = matches!(op, Op::JumpIfTrue(_));
                // A comparison made by the last step becomes the jump.
                let compared = match ch.micro.last() {
                    Some(
                        &(Micro::Eq(a, b, to)
                        | Micro::NotEq(a, b, to)
                        | Micro::Less(a, b, to)
                        | Micro::LessEq(a, b, to)
                        | Micro::Greater(a, b, to)
                        | Micro::GreaterEq(a, b, to)),
                    ) if to == cond => Some(Jump { a, b, target, when }),
                    _ => None,
                };
                // With a constant integer on the right, the jump carries it.
                let int = compared.and_then(|j| {
                    let k = c.int(j.b)?;
                    Some(JumpInt {
                        a: j.a,
                        k,
                        target,
                        when,
                    })
                });
                let last = match (int, compared, ch.micro.last()) {
                    (Some(j), _, Some(Micro::Eq(..))) => Micro::JumpEqInt(j),
                    (Some(j), _, Some(Micro::NotEq(..))) => Micro::JumpNotEqInt(j),
                    (Some(j), _, Some(Micro::Less(..))) => Micro::JumpLessInt(j),
                    (Some(j), _, Some(Micro::LessEq(..))) => Micro::JumpLessEqInt(j),
                    (Some(j), _, Some(Micro::Greater(..))) => Micro::JumpGreaterInt(j),
                    (Some(j), _, Some(Micro::GreaterEq(..))) => Micro::JumpGreaterEqInt(j),
                    (_, Some(j), Some(Micro::Eq(..))) => Micro::JumpEq(j),
                    (_, Some(j), Some(Micro::NotEq(..))) => Micro::JumpNotEq(j),
                    (_, Some(j), Some(Micro::Less(..))) => Micro::JumpLess(j),
                    (_, Some(j), Some(Micro::LessEq(..))) => Micro::JumpLessEq(j),
                    (_, Some(j), Some(Micro::Greater(..))) => Micro::JumpGreater(j),
                    (_, Some(j), Some(Micro::GreaterEq(..))) => Micro::JumpGreaterEq(j),
                    _ if when => Micro::JumpIfTrue { cond, target },
                    _ => Micro::JumpIfFalse { cond, target },
                };
                if compared.is_some() {
                    ch.micro.pop();
                }
                ch.micro.push(last);
                if at < MAX_CHAIN {
                    found.push(Candidate {
                        steps: ch.micro,
                        len: at + 1,
                        charge: at + 1 + calls.run,
                        next: addr(at + 1)?,
                        calls,
                    });
                }
                return Some(());
            }
            _ => {
                end = at;
                break;
            }
        }
    }
    // A value made for what follows, or the values of calls made in the
    // chain, such as the arguments of a builtin call that the chain cannot
    // make. Each is in a hidden place, which holds a number, so none of the
    // pushes fails.
    let values = ch.stack.clone();
    let made = !values.is_empty() && values.iter().all(|&v| ch.temp(v).is_some());
    if made && end <= MAX_CHAIN && (values.len() == 1 || calls.cost > 0) {
        ch.micro.extend(values.into_iter().map(Micro::Push));
        found.push(Candidate {
            steps: ch.micro,
            len: end,
            charge: end + calls.run,
            next: addr(end)?,
            calls,
        });
    }
    Some(())
}

/// The cost of an entry, in tens of aarch64 instructions: about 70,
/// including the 20 that choose it.
const ENTRY_COST: u32 = 7;

/// A chain costs the choosing and setup of an entry, plus about 60 per
/// step, but 20 for a [`Micro::Commit`].
fn chain_cost(steps: &[Micro]) -> u32 {
    let commits = steps
        .iter()
        .filter(|m| matches!(m, Micro::Commit { .. }))
        .count() as u32;
    3 + STEP_COST * (steps.len() as u32 - commits) + 2 * commits
}

/// The cost of a chain's step, as [`chain_cost`] weighs it. With the `jit`
/// feature, a chain that stands for two or more entries wins: native code
/// has the forms of chains, but not those of entries that work on the stack.
const STEP_COST: u32 = if cfg!(feature = "jit") { 5 } else { 6 };

/// Computes [`Compiled::fast`], [`Compiled::micro`] and
/// [`Compiled::constants`]. Each address takes the form that costs least to
/// run up to the next jump.
pub fn fuse(program: &mut Compiled) {
    let code = &program.code;
    let mut constants = Vec::new();
    // The hidden places of the chains come first, as separate places.
    let temps = program.globals.len();
    constants.extend((0..TEMPS).map(|_| Literal::Int(0)));
    let mut c = Constants {
        first: program.globals.len(),
        found: HashMap::new(),
        list: &mut constants,
    };
    let n = code.len();
    let slow = Fast {
        op: FastOp::Slow,
        len: 1,
        need: 0,
    };
    let mut fast = vec![slow; n];
    let mut micro = Vec::new();
    let mut runs = vec![0u32; n + 1];
    for pc in (0..n).rev() {
        let mut best = slow;
        let mut least = runs[pc + 1] + ENTRY_COST;
        forms(&code[pc..], &program.defs, &mut c, &mut |op, len| {
            let after = runs[pc + len] + ENTRY_COST;
            if after < least || (after == least && len >= usize::from(best.len)) {
                least = after;
                best = Fast {
                    op,
                    len: len as u8,
                    need: 0,
                };
            }
        });
        let mut chosen = None;
        let found = chains(&code[pc..], pc, code, &program.defs, &mut c, temps);
        for Candidate {
            steps,
            len,
            charge,
            next,
            calls,
        } in found
        {
            let cost = runs[pc + len] + chain_cost(&steps).saturating_sub(calls.saving);
            if let (Ok(charge), Ok(count)) = (u8::try_from(charge), u16::try_from(steps.len()))
                && cost < least
            {
                least = cost;
                let first = u32::try_from(micro.len()).unwrap_or(u32::MAX);
                let op = match calls.cost {
                    0 => FastOp::Chain { first, count, next },
                    cost => FastOp::CallChain {
                        first,
                        count,
                        next,
                        cost,
                    },
                };
                best = Fast {
                    op,
                    len: charge,
                    need: 0,
                };
                chosen = Some(steps);
            }
        }
        if let Some(steps) = chosen {
            micro.extend(steps);
        }
        // A Nop joins the entry that follows it.
        if best.op == FastOp::Nop
            && fast
                .get(pc + 1)
                .is_some_and(|f| f.op != FastOp::Slow && f.len < u8::MAX)
        {
            best = fast[pc + 1];
            best.len += 1;
            least = runs[pc + 1];
        }
        fast[pc] = best;
        runs[pc] = least;
    }
    program.fast = fast;
    program.micro = micro;
    program.constants = constants;
    check(program);
}

/// The number of places in the VM's table of globals for `program`: its
/// globals, then its constants, the chains' hidden places among them.
pub fn global_places(program: &Compiled) -> usize {
    program.globals.len() + program.constants.len()
}

/// Computes each entry's [`Fast::need`] from the places it names, and
/// checks that every global place is within [`global_places`]. This runs on
/// every program before the VM runs it, regardless of what made it. If any
/// check fails, the program runs without its fused code. A build with the
/// `unchecked-vars` feature relies on both to read and write places
/// unchecked.
pub fn check(program: &mut Compiled) {
    let globals = global_places(program);
    let mut fits = true;
    for i in 0..program.fast.len() {
        let mut need = 0usize;
        let mut place = |v: Var, more: usize| match v.table() {
            0 => fits &= v.index() + more <= globals,
            _ => need = need.max(v.index() + more),
        };
        fits &= places(&program.fast[i].op, &program.micro, &mut place);
        match u16::try_from(need) {
            Ok(need) => program.fast[i].need = need,
            Err(_) => fits = false,
        }
    }
    if !fits {
        program.fast.clear();
    }
}

/// Calls `place` with each place `op` reads or writes, and with the number
/// of places from there on that it takes (more than one for a builtin's
/// arguments). Returns false if some of a chain's steps are missing.
fn places(op: &FastOp, micro: &[Micro], place: &mut dyn FnMut(Var, usize)) -> bool {
    use FastOp as F;
    let mut one = |v: Var| place(v, 1);
    match *op {
        F::Slow
        | F::Nop
        | F::Swap
        | F::Reverse(_)
        | F::Jump(_)
        | F::JumpIfFalse(_)
        | F::JumpIfTrue(_)
        | F::Gosub(_)
        | F::Return
        | F::CallDef { .. }
        | F::ReturnValue
        | F::Str(_)
        | F::CallIdx
        | F::CallbackNext(_)
        | F::Call { .. }
        | F::Host { .. }
        | F::Builtin { .. }
        | F::Unary(_)
        | F::Truthy
        | F::Binary(_)
        | F::Swapped(_)
        | F::Index1
        | F::Index2
        | F::StoreIndex1
        | F::StoreIndex2 => {}
        F::Push(a) | F::Store(a) | F::SwappedStore(_, a) | F::Under { a, .. } => one(a),
        F::On { from, .. } => match from {
            Selector::Stack => {}
            Selector::Var(v) => one(v),
            Selector::Index(base, i) => {
                one(base);
                one(i);
            }
        },
        F::Push2(a, b)
        | F::Move { from: a, to: b }
        | F::Vars { a, b, .. }
        | F::UnderStore { a, to: b, .. }
        | F::VarsJump { a, b, .. }
        | F::VarsJumpTrue { a, b, .. }
        | F::Index1Vars { base: a, i: b }
        | F::StoreIndex1Vars { base: a, i: b }
        | F::AddStoreInt(a, _, b)
        | F::SubStoreInt(a, _, b)
        | F::MulStoreInt(a, _, b)
        | F::AddStoreReal(a, _, b)
        | F::UnlessEq(a, b, _)
        | F::UnlessNotEq(a, b, _)
        | F::UnlessLess(a, b, _)
        | F::UnlessLessEq(a, b, _)
        | F::UnlessGreater(a, b, _)
        | F::UnlessGreaterEq(a, b, _) => {
            one(a);
            one(b);
        }
        F::UnlessEqInt(a, ..)
        | F::UnlessNotEqInt(a, ..)
        | F::UnlessLessInt(a, ..)
        | F::UnlessLessEqInt(a, ..)
        | F::UnlessGreaterInt(a, ..)
        | F::UnlessGreaterEqInt(a, ..) => one(a),
        F::VarsStore { a, b, to, .. }
        | F::AddStore(a, b, to)
        | F::SubStore(a, b, to)
        | F::MulStore(a, b, to)
        | F::Index1Store { base: a, i: b, to }
        | F::Index1Nested {
            base: a,
            inner: b,
            i: to,
        }
        | F::StoreIndex1Nested {
            base: a,
            inner: b,
            i: to,
        } => {
            one(a);
            one(b);
            one(to);
        }
        F::CallArgs { argc, args, .. } => {
            for &a in args.iter().take(usize::from(argc)) {
                one(a);
            }
        }
        F::ForCheck { var, to, step, .. } => {
            one(var);
            one(to);
            one(step);
        }
        F::ForNext {
            var,
            step,
            step_slot,
            to,
            to_slot,
            ..
        } => {
            for v in [var, step, step_slot, to, to_slot] {
                one(v);
            }
        }
        F::Chain { first, count, .. } | F::CallChain { first, count, .. } => {
            let first = first as usize;
            let Some(steps) = micro.get(first..first + usize::from(count)) else {
                return false;
            };
            for m in steps {
                micro_places(m, place);
            }
        }
        // Native code that belongs to another VM: this program runs unfused.
        #[cfg(feature = "jit")]
        F::Native(_) => return false,
    }
    true
}

fn micro_places(m: &Micro, place: &mut dyn FnMut(Var, usize)) {
    use Micro as M;
    if let M::Builtin { argc, args, to, .. } = *m {
        place(args, usize::from(argc).max(1));
        place(to, 1);
        return;
    }
    let mut one = |v: Var| place(v, 1);
    match *m {
        M::Add(a, b, to)
        | M::Sub(a, b, to)
        | M::Mul(a, b, to)
        | M::Div(a, b, to)
        | M::IntDiv(a, b, to)
        | M::Mod(a, b, to)
        | M::And(a, b, to)
        | M::Or(a, b, to)
        | M::Xor(a, b, to)
        | M::Shl(a, b, to)
        | M::Shr(a, b, to)
        | M::Eq(a, b, to)
        | M::NotEq(a, b, to)
        | M::Less(a, b, to)
        | M::LessEq(a, b, to)
        | M::Greater(a, b, to)
        | M::GreaterEq(a, b, to)
        | M::Binary(_, a, b, to)
        | M::Index1 { base: a, i: b, to }
        | M::StoreIndex1 {
            value: a,
            base: b,
            i: to,
        } => {
            one(a);
            one(b);
            one(to);
        }
        M::AddInt(a, _, to)
        | M::SubInt(a, _, to)
        | M::MulInt(a, _, to)
        | M::AndInt(a, _, to)
        | M::BinaryInt(_, a, _, to)
        | M::AddReal(a, _, to)
        | M::SubReal(a, _, to)
        | M::MulReal(a, _, to)
        | M::BinaryReal(_, a, _, to)
        | M::Neg(a, to)
        | M::Unary(_, a, to)
        | M::Move { from: a, to }
        | M::Truthy(a, to) => {
            one(a);
            one(to);
        }
        M::Index2 { base, i, j, to }
        | M::StoreIndex2 {
            value: to,
            base,
            i,
            j,
        } => {
            for v in [base, i, j, to] {
                one(v);
            }
        }
        M::Builtin { .. } => unreachable!("taken above"),
        M::JumpIfFalse { cond, .. } | M::JumpIfTrue { cond, .. } | M::Push(cond) => one(cond),
        M::JumpEq(j)
        | M::JumpNotEq(j)
        | M::JumpLess(j)
        | M::JumpLessEq(j)
        | M::JumpGreater(j)
        | M::JumpGreaterEq(j) => {
            one(j.a);
            one(j.b);
        }
        M::JumpEqInt(j)
        | M::JumpNotEqInt(j)
        | M::JumpLessInt(j)
        | M::JumpLessEqInt(j)
        | M::JumpGreaterInt(j)
        | M::JumpGreaterEqInt(j) => one(j.a),
        M::Commit { .. } => {}
    }
}
