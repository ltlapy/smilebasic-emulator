//! Fused instructions: a second form of a program's code that runs faster.
//!
//! [`Compiled::code`] stays as the compiler made it, and everything that needs
//! to see single instructions uses it: the debugger, TRACE, errors, line
//! numbers. Beside it, [`Compiled::fast`] has one entry for each address, which
//! does what the instructions from there do, usually several at once.
//!
//! An entry does its work only when nothing can go wrong and nothing can be
//! seen to happen part way: numbers and no failure. Anything else, the VM runs
//! from `code` one instruction at a time, from the same address. So there is
//! one meaning for each instruction, and an error names what it always named.
//! An entry counts as the instructions it stands for, so that time slices end
//! where they would.
//!
//! Most entries are a few stack instructions in one. A [`FastOp::Chain`]
//! stands for whole statements instead: the stack code of their expressions
//! worked out again as three-address [`Micro`] steps, which read variables
//! and constants where they are and keep what they make in hidden places
//! after the constants, so that the values never go through the stack. Its
//! steps write nothing the program can see until a statement's last, and a
//! [`Micro::Commit`] after each statement but the last says where `code`
//! goes on from if a later one cannot do its work. Each address takes the
//! entry or chain that costs least, by rough instruction counts, on to the
//! next jump.

use std::collections::HashMap;

use crate::ast::{BinaryOp, UnaryOp};
use crate::bytecode::{Addr, Compiled, Literal, Op, Slot, Want};

/// A number's place, for [`FastOp`]s: a variable of the running DEF or of the
/// slot, or a constant. Constants follow the globals in the VM's table of
/// them, so that reading any of the three is the same few instructions.
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

    /// Which table: 0 for globals and constants, 1 for locals.
    #[inline(always)]
    pub fn table(self) -> usize {
        usize::from(self.0 >> 15)
    }

    #[inline(always)]
    pub fn index(self) -> usize {
        usize::from(self.0 & !LOCAL)
    }
}

/// What an entry does. Stack effects as in [`Op`]; a [`Var`] stands for the
/// `Int`, `Real` or `Load` that pushed it. `Slow` runs from `code`.
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
    /// A call of a DEF of the running slot.
    CallDef {
        def: u32,
        argc: u8,
        want: Want,
    },
    /// RETURN with a value from a DEF called in the running slot.
    ReturnValue,
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
    /// operator in the entry's own kind.
    AddStore(Var, Var, Var),
    SubStore(Var, Var, Var),
    MulStore(Var, Var, Var),
    /// `VarsJump` of a comparison, with the operator in the entry's kind:
    /// a jump unless `a op b`.
    UnlessEq(Var, Var, Addr),
    UnlessNotEq(Var, Var, Addr),
    UnlessLess(Var, Var, Addr),
    UnlessLessEq(Var, Var, Addr),
    UnlessGreater(Var, Var, Addr),
    UnlessGreaterEq(Var, Var, Addr),
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
    /// A [`Micro`] chain: `count` of [`Compiled::micro`] from `first`, then on
    /// to `next` unless one of them jumps.
    Chain {
        first: u32,
        count: u16,
        next: Addr,
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
        /// Whether `step` and `to` are places apart from `var`, `step_slot`
        /// and `to_slot`, as nearly always, so that reading them before
        /// any of those is written reads the same.
        apart: bool,
    },
}

/// One three-address step of a [`FastOp::Chain`]. A chain stands for one or
/// more statements of stack code, each ending in one thing that can be seen:
/// a store, a jump, or a value left on the stack. Its steps read their
/// operands from variables and constants, and before a statement's last step
/// they write only the chain's own places, hidden variables after the
/// constants. So a step that cannot do its work leaves the statement as it
/// was, and `code` runs it from its start. Each common operator has its own
/// kind, `a op b` with `a` the left side and the result last.
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
    /// The statements before are done: a later step that cannot do its
    /// work leaves the rest to `step` from `to`, with `charged` of the
    /// chain's instructions run.
    Commit {
        to: Addr,
        charged: u8,
    },
    /// `a` as 0 or 1, as [`Op::Truthy`].
    Truthy(Var, Var),
    /// The chain's last step: `a` left on the stack.
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
    /// How many instructions of `code` it stands for.
    pub len: u8,
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

/// Offers each form the code from its start can take, with the instructions
/// each stands for.
fn forms(code: &[Op], c: &mut Constants, add: &mut dyn FnMut(FastOp, usize)) {
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
        Op::CallDef { def, argc, want } => add(F::CallDef { def, argc, want }, 1),
        Op::CallBuiltin {
            id,
            argc,
            want: Want::Value,
        } if crate::builtins::is_pure_call(id, argc.into()) => add(F::Builtin { id, argc }, 1),
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
    }
    match code.get(2) {
        Some(Op::Binary(op)) => {
            let op = *op;
            add(F::Vars { op, a, b }, 3);
            if let Some(to) = store(code.get(3)) {
                add(
                    match op {
                        BinaryOp::Add => F::AddStore(a, b, to),
                        BinaryOp::Sub => F::SubStore(a, b, to),
                        BinaryOp::Mul => F::MulStore(a, b, to),
                        op => F::VarsStore { op, a, b, to },
                    },
                    4,
                );
            }
            if let Some(Op::JumpIfFalse(t) | Op::TestIfFalse(t)) = code.get(3) {
                let t = *t;
                add(
                    match op {
                        BinaryOp::Eq => F::UnlessEq(a, b, t),
                        BinaryOp::NotEq => F::UnlessNotEq(a, b, t),
                        BinaryOp::Less => F::UnlessLess(a, b, t),
                        BinaryOp::LessEq => F::UnlessLessEq(a, b, t),
                        BinaryOp::Greater => F::UnlessGreater(a, b, t),
                        BinaryOp::GreaterEq => F::UnlessGreaterEq(a, b, t),
                        op => F::VarsJump {
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

/// How many hidden places a chain has for the values it makes.
const TEMPS: usize = 8;

/// A chain being built: the places of the values a stack machine would
/// have, and which hidden places hold one still to be used.
struct Chain {
    micro: Vec<Micro>,
    stack: Vec<Var>,
    /// The first hidden place.
    temps: usize,
    used: u16,
}

impl Chain {
    fn temp(&self, v: Var) -> Option<usize> {
        let k = v.index().wrapping_sub(self.temps);
        (v.table() == 0 && k < TEMPS).then_some(k)
    }

    fn pop(&mut self) -> Option<Var> {
        let v = self.stack.pop()?;
        if let Some(k) = self.temp(v) {
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

    /// `n` free hidden places in a row.
    fn alloc_run(&mut self, n: usize) -> Option<Var> {
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

    /// Where the last step puts its result, if that is `v`.
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

fn binary_micro(op: BinaryOp, a: Var, b: Var, to: Var) -> Micro {
    use BinaryOp as B;
    use Micro as M;
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

/// A chain the code from an address can take: its steps, how many
/// instructions it stands for, and where it goes on.
type Candidate = (Vec<Micro>, usize, Addr);

/// The longest chain a chain may be, in instructions: [`Fast::len`].
const MAX_CHAIN: usize = 200;

/// The chains for the code from `code[0]` at `pc`, each a statement longer
/// than the one before. `temps` is the first of its hidden places.
fn chains(code: &[Op], pc: usize, c: &mut Constants, temps: usize) -> Vec<Candidate> {
    let mut found = Vec::new();
    chain(code, pc, c, temps, &mut found);
    found
}

fn chain(
    code: &[Op],
    pc: usize,
    c: &mut Constants,
    temps: usize,
    found: &mut Vec<Candidate>,
) -> Option<()> {
    let mut ch = Chain {
        micro: Vec::new(),
        stack: Vec::new(),
        temps,
        used: 0,
    };
    let addr = |at: usize| Addr::try_from(pc + at).ok();
    // A store or a jump ends it. A Jump right after a store goes with it.
    let after_store = |at: usize| match code.get(at + 1) {
        Some(&Op::Jump(target)) => Some((at + 2, target)),
        _ => Some((at + 1, addr(at + 1)?)),
    };
    // After a store, the chain may go on to the next statement.
    let mut stored = |ch: &mut Chain, at: usize| -> Option<bool> {
        let (len, next) = after_store(at)?;
        if len > MAX_CHAIN {
            return None;
        }
        found.push((ch.micro.clone(), len, next));
        if len == at + 2 {
            return Some(false);
        }
        let charged = u8::try_from(len).ok()?;
        ch.micro.push(Micro::Commit { to: next, charged });
        Some(true)
    };
    let mut end = code.len();
    for (at, op) in code.iter().enumerate() {
        match *op {
            Op::Nop => {}
            Op::Int(_) | Op::Real(_) | Op::Load(_) => {
                let v = c.source(Some(op))?;
                ch.stack.push(v);
            }
            Op::Swap => {
                let n = ch.stack.len();
                if n < 2 {
                    return None;
                }
                ch.stack.swap(n - 1, n - 2);
            }
            Op::Reverse(n) => {
                let from = ch.stack.len().checked_sub(n as usize)?;
                ch.stack[from..].reverse();
            }
            Op::Binary(op) => {
                let b = ch.pop()?;
                let a = ch.pop()?;
                ch.make(|to| binary_micro(op, a, b, to))?;
            }
            Op::Unary(op) => {
                let a = ch.pop()?;
                ch.make(|to| match op {
                    UnaryOp::Neg => Micro::Neg(a, to),
                    op => Micro::Unary(op, a, to),
                })?;
            }
            Op::Truthy => {
                let a = ch.pop()?;
                ch.make(|to| Micro::Truthy(a, to))?;
            }
            Op::Index(1) => {
                let i = ch.pop()?;
                let base = ch.pop()?;
                ch.make(|to| Micro::Index1 { base, i, to })?;
            }
            Op::Index(2) => {
                let j = ch.pop()?;
                let i = ch.pop()?;
                let base = ch.pop()?;
                ch.make(|to| Micro::Index2 { base, i, j, to })?;
            }
            Op::CallBuiltin {
                id,
                argc,
                want: Want::Value,
            } if crate::builtins::is_pure_call(id, argc.into()) => {
                let n = usize::from(argc);
                let from = ch.stack.len().checked_sub(n)?;
                let args = match n {
                    0 => Var::global(temps)?,
                    // One place is a slice of one already.
                    1 => ch.stack[from],
                    _ => {
                        let args = ch.alloc_run(n)?;
                        for (k, &from) in ch.stack[from..].iter().enumerate() {
                            let to = Var::global(args.index() + k)?;
                            ch.micro.push(Micro::Move { from, to });
                        }
                        args
                    }
                };
                for _ in 0..n {
                    ch.pop()?;
                }
                if n > 1 {
                    ch.used &= !(((1u16 << n) - 1) << (args.index() - temps));
                }
                ch.make(|to| Micro::Builtin { id, argc, args, to })?;
            }
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
                if !stored(&mut ch, at)? {
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
                if !stored(&mut ch, at)? {
                    return Some(());
                }
            }
            Op::JumpIfFalse(target) | Op::TestIfFalse(target) | Op::JumpIfTrue(target) => {
                let cond = ch.pop()?;
                if !ch.stack.is_empty() {
                    return None;
                }
                let when = matches!(op, Op::JumpIfTrue(_));
                // A comparison made just now jumps on its own.
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
                let last = match (compared, ch.micro.last()) {
                    (Some(j), Some(Micro::Eq(..))) => Micro::JumpEq(j),
                    (Some(j), Some(Micro::NotEq(..))) => Micro::JumpNotEq(j),
                    (Some(j), Some(Micro::Less(..))) => Micro::JumpLess(j),
                    (Some(j), Some(Micro::LessEq(..))) => Micro::JumpLessEq(j),
                    (Some(j), Some(Micro::Greater(..))) => Micro::JumpGreater(j),
                    (Some(j), Some(Micro::GreaterEq(..))) => Micro::JumpGreaterEq(j),
                    _ if when => Micro::JumpIfTrue { cond, target },
                    _ => Micro::JumpIfFalse { cond, target },
                };
                if compared.is_some() {
                    ch.micro.pop();
                }
                ch.micro.push(last);
                if at < MAX_CHAIN {
                    found.push((ch.micro, at + 1, addr(at + 1)?));
                }
                return Some(());
            }
            _ => {
                end = at;
                break;
            }
        }
    }
    // A value made for what follows.
    match ch.stack[..] {
        [v] if ch.temp(v).is_some() && end <= MAX_CHAIN => {
            ch.micro.push(Micro::Push(v));
            found.push((ch.micro, end, addr(end)?));
        }
        _ => {}
    }
    Some(())
}

/// What an entry costs, in tens of aarch64 instructions: about 70 with the
/// 20 that choose it.
const ENTRY_COST: u32 = 7;

/// A chain costs an entry's choosing and setting out, and about 60 a step,
/// but 20 for a [`Micro::Commit`].
fn chain_cost(steps: &[Micro]) -> u32 {
    let commits = steps
        .iter()
        .filter(|m| matches!(m, Micro::Commit { .. }))
        .count() as u32;
    3 + 6 * (steps.len() as u32 - commits) + 2 * commits
}

/// Works out [`Compiled::fast`], [`Compiled::micro`] and
/// [`Compiled::constants`]. Each address takes the form that costs least to
/// run on to the next jump.
pub fn fuse(program: &mut Compiled) {
    let code = &program.code;
    let mut constants = Vec::new();
    // The chains' hidden places come first, as places of their own.
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
    };
    let mut fast = vec![slow; n];
    let mut micro = Vec::new();
    let mut runs = vec![0u32; n + 1];
    for pc in (0..n).rev() {
        let mut best = slow;
        let mut least = runs[pc + 1] + ENTRY_COST;
        forms(&code[pc..], &mut c, &mut |op, len| {
            let after = runs[pc + len] + ENTRY_COST;
            if after < least || (after == least && len >= usize::from(best.len)) {
                least = after;
                best = Fast { op, len: len as u8 };
            }
        });
        let mut chosen = None;
        for (steps, len, next) in chains(&code[pc..], pc, &mut c, temps) {
            let cost = runs[pc + len] + chain_cost(&steps);
            if let (Ok(short), Ok(count)) = (u8::try_from(len), u16::try_from(steps.len()))
                && cost < least
            {
                least = cost;
                let first = u32::try_from(micro.len()).unwrap_or(u32::MAX);
                best = Fast {
                    op: FastOp::Chain { first, count, next },
                    len: short,
                };
                chosen = Some(steps);
            }
        }
        if let Some(steps) = chosen {
            micro.extend(steps);
        }
        // A Nop goes along with what follows it.
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
}
