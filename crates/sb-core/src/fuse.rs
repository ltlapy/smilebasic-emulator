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
    match code.get(2) {
        Some(Op::Binary(op)) => {
            let op = *op;
            add(F::Vars { op, a, b }, 3);
            if let Some(to) = store(code.get(3)) {
                add(F::VarsStore { op, a, b, to }, 4);
            }
            if let Some(Op::JumpIfFalse(target) | Op::TestIfFalse(target)) = code.get(3) {
                let target = *target;
                add(F::VarsJump { op, a, b, target }, 4);
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

/// Works out [`Compiled::fast`] and [`Compiled::constants`]. Each address takes
/// the form that leaves the fewest entries to run on to the next jump.
pub fn fuse(program: &mut Compiled) {
    let code = &program.code;
    let mut constants = Vec::new();
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
    let mut runs = vec![0u32; n + 1];
    for pc in (0..n).rev() {
        let mut best = slow;
        let mut fewest = runs[pc + 1] + 1;
        forms(&code[pc..], &mut c, &mut |op, len| {
            let after = runs[pc + len] + 1;
            if after < fewest || (after == fewest && len >= usize::from(best.len)) {
                fewest = after;
                best = Fast { op, len: len as u8 };
            }
        });
        // A Nop goes along with what follows it.
        if best.op == FastOp::Nop && fast.get(pc + 1).is_some_and(|f| f.op != FastOp::Slow) {
            best = fast[pc + 1];
            best.len += 1;
            fewest = runs[pc + 1];
        }
        fast[pc] = best;
        runs[pc] = fewest;
    }
    program.fast = fast;
    program.constants = constants;
}
