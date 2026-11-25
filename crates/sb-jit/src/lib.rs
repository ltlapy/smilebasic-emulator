//! Native code for regions of the interpreter's fused entries.
//!
//! This crate has no knowledge of SmileBASIC. It takes a [`Region`] and
//! compiles it with Cranelift into one function. A region holds entries, each
//! standing for some instructions of the program and made of three-address
//! [`Step`]s on numbered places in two tables of values. The caller (sb-core's
//! `jit` module) says how a value is laid out ([`Layout`]) and what each entry
//! does. Only the caller holds the language's rules, and it checks the native
//! code against them in tests.
//!
//! The function runs entries until one of these, and then returns:
//!
//! - the next entry is not in the region, or costs more than the budget
//!   left: [`EXIT`], with `pc` the next entry's address;
//! - a step cannot do its work (a value that is not a number, an integer
//!   that overflows into a real where the step leaves that to the
//!   interpreter, a string in the place a number is written to): [`FAIL`],
//!   with `pc` and `left` set to the values the interpreter continues
//!   from, the same values the interpreter's own fused code would give.
//!
//! Native code reads and writes only places below [`Code::need`] of each
//! table, and calls nothing. The caller must hand it tables at least that
//! long, and nothing else may use them while the code runs.
//!
//! PROTOTYPE LIMITS, each needed by the full version:
//! - executable memory on Linux only ([`exec`]); macOS needs `MAP_JIT` and
//!   `pthread_jit_write_protect_np`, Windows `VirtualAlloc`/`VirtualProtect`;
//! - one mapping per region, never shared, freed with the program;
//! - no calls out of native code (DEF calls, builtins, host commands and
//!   array elements end the region instead);
//! - x86-64 and aarch64 only, which Cranelift's backends cover.

mod codegen;
mod exec;

pub use codegen::{compile, prepare};

/// The memory layout of a value: a tag byte at offset 0, an `i32` at
/// `int_offset` for an integer, an `f64` at `real_offset` for a real.
/// Tags from `first_heap_tag` on own memory that native code cannot drop,
/// so a write over one of them fails instead.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Layout {
    pub size: u32,
    pub int_tag: u8,
    pub real_tag: u8,
    pub first_heap_tag: u8,
    pub int_offset: u32,
    pub real_offset: u32,
}

/// A place in one of the two tables: 0 the globals and constants, 1 the
/// running DEF's variables.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Place {
    pub table: u8,
    pub index: u32,
}

/// What a step reads: a place, one of the entry's hidden values, or a
/// number the step carries.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Operand {
    Place(Place),
    Hidden(u8),
    Int(i32),
    Real(f64),
}

/// Where a step's result goes. A hidden value lives only while its entry
/// runs, so native code keeps it in registers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dest {
    Place(Place),
    Hidden(u8),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    IntDiv,
    Mod,
    And,
    Or,
    Xor,
    Shl,
    Shr,
    UShr,
    Eq,
    NotEq,
    Less,
    LessEq,
    Greater,
    GreaterEq,
}

impl BinOp {
    pub fn compares(self) -> bool {
        use BinOp as B;
        matches!(
            self,
            B::Eq | B::NotEq | B::Less | B::LessEq | B::Greater | B::GreaterEq
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnOp {
    /// Wraps on integers.
    Neg,
    /// Integers only.
    Not,
    /// 1 for zero, 0 otherwise.
    LogicalNot,
    /// 0 for zero, 1 otherwise.
    Truthy,
}

/// What a jump tests.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Cond {
    /// `a op b`, a comparison.
    Compare { op: BinOp, a: Operand, b: Operand },
    /// Whether `a` is not zero.
    Truth(Operand),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Step {
    Binary {
        op: BinOp,
        a: Operand,
        b: Operand,
        to: Dest,
    },
    Unary {
        op: UnOp,
        a: Operand,
        to: Dest,
    },
    /// A number copied; anything else fails.
    Move {
        from: Operand,
        to: Dest,
    },
    /// To `target` when `cond` is `when`, ending the entry.
    Jump {
        cond: Cond,
        target: u32,
        when: bool,
    },
    /// The statements so far are done: after a later failure, the
    /// interpreter continues from `to`, with `charged` of the entry's cost
    /// spent.
    Commit {
        to: u32,
        charged: u32,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub enum Body {
    /// Steps in order, then on to `next` unless one jumps.
    Steps { steps: Vec<Step>, next: u32 },
    /// An integer FOR's NEXT: `var` plus `step_slot`, then `step` and `to`
    /// into their slots, then on to `cont` while within `to`, else `exit`.
    /// Anything but integers fails.
    ForNext {
        var: Place,
        step: Operand,
        step_slot: Place,
        to: Operand,
        to_slot: Place,
        exit: u32,
        cont: u32,
    },
}

/// An entry at address `pc`, costing `len` of the budget.
#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub pc: u32,
    pub len: u32,
    pub body: Body,
}

/// Entries compiled together; the first is where the code starts. A jump
/// to an address of another entry goes there directly.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Region {
    pub entries: Vec<Entry>,
}

/// The input and output of native code. `tables` point to the two tables,
/// `pc` is where it stopped, `left` the budget left.
#[repr(C)]
#[derive(Debug)]
pub struct Ctx {
    pub tables: [*mut u8; 2],
    pub pc: u64,
    pub left: u64,
}

/// [`Code::run`]'s result when it stopped before an entry it does not run.
pub const EXIT: u32 = 0;
/// [`Code::run`]'s result when a step could not do its work.
pub const FAIL: u32 = 1;

/// A compiled region.
pub struct Code {
    mem: exec::Mem,
    need: [usize; 2],
}

impl Code {
    /// Returns the number of places of each table that the code reads or
    /// writes: one more than the highest index it names.
    pub fn need(&self) -> [usize; 2] {
        self.need
    }

    /// Bytes of machine code.
    pub fn size(&self) -> usize {
        self.mem.len()
    }

    /// Runs the code from its first entry, with `ctx.left` of the budget.
    ///
    /// # Safety
    ///
    /// `ctx.tables[t]` must point to at least `need()[t]` values laid out
    /// as the [`Layout`] the code was compiled with said, valid to read and
    /// write and used by nothing else until this returns.
    pub unsafe fn run(&self, ctx: &mut Ctx) -> u32 {
        // SAFETY: the memory holds a function compiled for this machine's
        // C calling convention with this signature (`codegen::compile`), and
        // it stays mapped while `self` lives. The caller must vouch for the
        // memory the function touches, as above.
        unsafe { (self.mem.entry())(ctx) }
    }
}

impl std::fmt::Debug for Code {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Code({} bytes, need {:?})", self.size(), self.need)
    }
}

/// Whether this build can make native code at all.
pub const SUPPORTED: bool = cfg!(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
));
