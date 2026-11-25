//! A [`Region`] in Cranelift's IR, compiled.
//!
//! Each value a step works on is an integer, a real, or one whose kind is
//! known only when the code runs ([`Val::Dyn`]). Such a value has its tag
//! and both an `i32` and an `f64` reading of its payload, and the tag says
//! which reading counts. An operator on values of unknown kind tests the
//! tags and takes an integer or a real path, which meet again after it.

use std::collections::HashMap;
use std::mem::{offset_of, size_of};

use cranelift_codegen::control::ControlPlane;
use cranelift_codegen::ir::condcodes::{FloatCC, IntCC};
use cranelift_codegen::ir::{
    self, AbiParam, Block, BlockArg, InstBuilder, MemFlags, Signature, UserFuncName, Value, types,
};
use cranelift_codegen::isa::{CallConv, OwnedTargetIsa};
use cranelift_codegen::settings::{self, Configurable};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext, Variable};

use crate::{
    BinOp, Body, Code, Cond, Ctx, Dest, Entry, Layout, Operand, Place, Region, SUPPORTED, Step,
    UnOp, exec,
};

thread_local! {
    /// Making the target's description takes longer than compiling a small
    /// region, so each thread makes it once.
    static ISA: Result<OwnedTargetIsa, String> = make_isa();
}

fn make_isa() -> Result<OwnedTargetIsa, String> {
    let mut flags = settings::builder();
    let set = |flags: &mut settings::Builder, name: &str, value: &str| {
        flags.set(name, value).map_err(|e| format!("{name}: {e}"))
    };
    set(&mut flags, "opt_level", "speed")?;
    set(&mut flags, "unwind_info", "false")?;
    let verify = if cfg!(debug_assertions) {
        "true"
    } else {
        "false"
    };
    set(&mut flags, "enable_verifier", verify)?;
    // The host's baseline: the same instructions on every machine of the
    // architecture, regardless of the extensions it has.
    let builder = cranelift_codegen::isa::lookup(target_lexicon::Triple::host())
        .map_err(|e| format!("no Cranelift backend for this machine: {e}"))?;
    builder
        .finish(settings::Flags::new(flags))
        .map_err(|e| format!("{e}"))
}

/// Makes this thread's description of the machine, which the first
/// [`compile`] would make otherwise, so that it can be timed separately.
pub fn prepare() -> Result<(), String> {
    if !SUPPORTED {
        return Err("no native code on this platform".into());
    }
    ISA.with(|isa| isa.as_ref().map(|_| ()).map_err(Clone::clone))
}

/// Compiles `region` for this machine, for values laid out as `layout`.
pub fn compile(region: &Region, layout: &Layout) -> Result<Code, String> {
    if !SUPPORTED {
        return Err("no native code on this platform".into());
    }
    check_layout(layout)?;
    ISA.with(|isa| {
        let isa = isa.as_ref().map_err(Clone::clone)?;
        let (func, need) = build(region, layout, isa)?;
        let mut ctx = cranelift_codegen::Context::for_function(func);
        let compiled = ctx
            .compile(&**isa, &mut ControlPlane::default())
            .map_err(|e| format!("{:?}", e.inner))?;
        // Code with relocations would need patching to run where it lands.
        if !compiled.buffer.relocs().is_empty() {
            return Err("code with relocations".into());
        }
        let mem = exec::Mem::new(compiled.code_buffer())?;
        Ok(Code { mem, need })
    })
}

fn check_layout(l: &Layout) -> Result<(), String> {
    let fits = |offset: u32, width: u32| offset.is_multiple_of(width) && offset + width <= l.size;
    let tags = [l.int_tag, l.real_tag];
    if !fits(l.int_offset, 4)
        || !fits(l.real_offset, 8)
        || l.size == 0
        || l.size > 64
        || tags.iter().any(|&t| t >= l.first_heap_tag)
        || l.int_tag == l.real_tag
    {
        return Err(format!("layout {l:?} not supported"));
    }
    Ok(())
}

/// A value a step works on.
#[derive(Clone, Copy)]
enum Val {
    Int(Value),
    Real(Value),
    /// Known only at run time. `numeric` once the tag is checked to be that
    /// of an integer or a real.
    Dyn {
        tag: Value,
        int: Value,
        real: Value,
        numeric: bool,
    },
}

/// Whether a value is an integer: known now, or at run time.
#[derive(Clone, Copy)]
enum Known {
    Yes,
    No,
    RunTime(Value),
}

struct Gen<'f, 'c> {
    b: FunctionBuilder<'f>,
    layout: &'c Layout,
    tables: [Value; 2],
    left: Variable,
    blocks: HashMap<u32, Block>,
    exit: Block,
    fail: Block,
    need: [usize; 2],
    hidden: HashMap<u8, Val>,
    /// The budget before the running entry.
    left_before: Value,
    restart: Restart,
}

/// The point the interpreter continues from after a failure.
#[derive(Clone, Copy)]
struct Restart {
    pc: u32,
    /// The part of the entry's cost that is charged.
    charged: u32,
}

const FLAGS: MemFlags = MemFlags::trusted();

// The offsets of the fields of `Ctx` that native code reads and writes.
const CTX_TABLES: [i32; 2] = [
    offset_of!(Ctx, tables) as i32,
    (offset_of!(Ctx, tables) + size_of::<*mut u8>()) as i32,
];
const CTX_PC: i32 = offset_of!(Ctx, pc) as i32;
const CTX_LEFT: i32 = offset_of!(Ctx, left) as i32;

fn build(
    region: &Region,
    layout: &Layout,
    isa: &OwnedTargetIsa,
) -> Result<(ir::Function, [usize; 2]), String> {
    let Some(head) = region.entries.first() else {
        return Err("empty region".into());
    };
    let ptr = isa.pointer_type();
    if ptr != types::I64 {
        return Err("64-bit targets only".into());
    }
    let mut sig = Signature::new(CallConv::triple_default(isa.triple()));
    sig.params.push(AbiParam::new(ptr));
    sig.returns.push(AbiParam::new(types::I32));
    let mut func = ir::Function::with_name_signature(UserFuncName::default(), sig);
    let mut fctx = FunctionBuilderContext::new();
    let mut b = FunctionBuilder::new(&mut func, &mut fctx);
    let start = b.create_block();
    b.append_block_params_for_function_params(start);
    b.switch_to_block(start);
    let ctx = b.block_params(start)[0];
    let tables = [
        b.ins().load(ptr, FLAGS, ctx, CTX_TABLES[0]),
        b.ins().load(ptr, FLAGS, ctx, CTX_TABLES[1]),
    ];
    let left0 = b.ins().load(types::I64, FLAGS, ctx, CTX_LEFT);
    let left = b.declare_var(types::I64);
    b.def_var(left, left0);
    let mut blocks = HashMap::new();
    for e in &region.entries {
        if blocks.insert(e.pc, b.create_block()).is_some() {
            return Err(format!("two entries at {}", e.pc));
        }
    }
    // The two ways out: stopped before an entry, or failed.
    let way_out = |b: &mut FunctionBuilder, status: u32| {
        let block = b.create_block();
        let pc = b.append_block_param(block, types::I64);
        let left = b.append_block_param(block, types::I64);
        (block, pc, left, status)
    };
    let exit = way_out(&mut b, crate::EXIT);
    let fail = way_out(&mut b, crate::FAIL);
    let first = blocks[&head.pc];
    b.ins().jump(first, &[]);
    let mut g = Gen {
        b,
        layout,
        tables,
        left,
        blocks,
        exit: exit.0,
        fail: fail.0,
        need: [0; 2],
        hidden: HashMap::new(),
        left_before: left0,
        restart: Restart {
            pc: head.pc,
            charged: 0,
        },
    };
    for e in &region.entries {
        g.entry(e)?;
    }
    for (block, pc, left, status) in [exit, fail] {
        g.b.switch_to_block(block);
        g.b.ins().store(FLAGS, pc, ctx, CTX_PC);
        g.b.ins().store(FLAGS, left, ctx, CTX_LEFT);
        let status = g.b.ins().iconst(types::I32, i64::from(status));
        g.b.ins().return_(&[status]);
    }
    g.b.seal_all_blocks();
    let need = g.need;
    g.b.finalize();
    Ok((func, need))
}

fn int_cc(op: BinOp) -> IntCC {
    match op {
        BinOp::Eq => IntCC::Equal,
        BinOp::NotEq => IntCC::NotEqual,
        BinOp::Less => IntCC::SignedLessThan,
        BinOp::LessEq => IntCC::SignedLessThanOrEqual,
        BinOp::Greater => IntCC::SignedGreaterThan,
        _ => IntCC::SignedGreaterThanOrEqual,
    }
}

/// IEEE comparisons: false when either side is NaN, but for `!=`, which is
/// true (Cranelift's `ne` is "unordered or not equal").
fn float_cc(op: BinOp) -> FloatCC {
    match op {
        BinOp::Eq => FloatCC::Equal,
        BinOp::NotEq => FloatCC::NotEqual,
        BinOp::Less => FloatCC::LessThan,
        BinOp::LessEq => FloatCC::LessThanOrEqual,
        BinOp::Greater => FloatCC::GreaterThan,
        _ => FloatCC::GreaterThanOrEqual,
    }
}

impl Gen<'_, '_> {
    fn entry(&mut self, e: &Entry) -> Result<(), String> {
        let block = self.blocks[&e.pc];
        self.b.switch_to_block(block);
        // The same as in the interpreter: an entry that costs more than is
        // left is not started.
        let left = self.b.use_var(self.left);
        let len = self.b.ins().iconst(types::I64, i64::from(e.len));
        let over = self.b.ins().icmp(IntCC::UnsignedGreaterThan, len, left);
        let body = self.b.create_block();
        let pc = self.b.ins().iconst(types::I64, i64::from(e.pc));
        self.b.ins().brif(
            over,
            self.exit,
            &[BlockArg::Value(pc), BlockArg::Value(left)],
            body,
            &[],
        );
        self.b.switch_to_block(body);
        let after = self.b.ins().isub(left, len);
        self.b.def_var(self.left, after);
        self.left_before = left;
        self.restart = Restart {
            pc: e.pc,
            charged: 0,
        };
        self.hidden.clear();
        match &e.body {
            Body::Steps { steps, next } => {
                for step in steps {
                    self.step(step)?;
                }
                self.goto(*next);
            }
            Body::ForNext {
                var,
                step,
                step_slot,
                to,
                to_slot,
                exit,
                cont,
            } => self.for_next(*var, *step, *step_slot, *to, *to_slot, *exit, *cont)?,
        }
        Ok(())
    }

    /// Returns the destination of a jump to `pc`: its entry's block, or out.
    fn target(&mut self, pc: u32) -> (Block, Vec<BlockArg>) {
        if let Some(&block) = self.blocks.get(&pc) {
            return (block, Vec::new());
        }
        let pc = self.b.ins().iconst(types::I64, i64::from(pc));
        let left = self.b.use_var(self.left);
        (self.exit, vec![BlockArg::Value(pc), BlockArg::Value(left)])
    }

    fn goto(&mut self, pc: u32) {
        let (block, args) = self.target(pc);
        self.b.ins().jump(block, &args);
    }

    /// Fails when `cond` is true, and continues in a new block when not.
    fn fail_if(&mut self, cond: Value) {
        let Restart { pc, charged } = self.restart;
        let pc = self.b.ins().iconst(types::I64, i64::from(pc));
        let left = self.b.ins().iadd_imm(self.left_before, -i64::from(charged));
        let on = self.b.create_block();
        let args = [BlockArg::Value(pc), BlockArg::Value(left)];
        self.b.ins().brif(cond, self.fail, &args, on, &[]);
        self.b.switch_to_block(on);
    }

    fn fail_unless(&mut self, cond: Value) {
        let not = self.b.ins().icmp_imm(IntCC::Equal, cond, 0);
        self.fail_if(not);
    }

    fn fail_always(&mut self) {
        let yes = self.b.ins().iconst(types::I8, 1);
        self.fail_if(yes);
    }

    /// The address of a place, noting what the code needs of its table.
    fn place(&mut self, p: Place) -> Result<(Value, i32), String> {
        let table = usize::from(p.table);
        if table > 1 {
            return Err(format!("no table {table}"));
        }
        let index = usize::try_from(p.index).map_err(|e| e.to_string())?;
        let offset = (index.checked_mul(self.layout.size as usize))
            .and_then(|o| i32::try_from(o).ok())
            .filter(|o| *o <= i32::MAX - 64)
            .ok_or("place too far")?;
        self.need[table] = self.need[table].max(index + 1);
        Ok((self.tables[table], offset))
    }

    fn read(&mut self, op: Operand) -> Result<Val, String> {
        Ok(match op {
            // Cranelift takes a narrow constant's bits zero-extended.
            Operand::Int(n) => Val::Int(self.b.ins().iconst(types::I32, i64::from(n as u32))),
            Operand::Real(x) => Val::Real(self.b.ins().f64const(x)),
            Operand::Hidden(k) => *self.hidden.get(&k).ok_or("hidden value read before made")?,
            Operand::Place(p) => {
                let (base, at) = self.place(p)?;
                let l = self.layout;
                let tag = self.b.ins().load(types::I8, FLAGS, base, at);
                let int = (self.b.ins()).load(types::I32, FLAGS, base, at + l.int_offset as i32);
                let real = (self.b.ins()).load(types::F64, FLAGS, base, at + l.real_offset as i32);
                Val::Dyn {
                    tag,
                    int,
                    real,
                    numeric: false,
                }
            }
        })
    }

    fn is_tag(&mut self, tag: Value, want: u8) -> Value {
        self.b.ins().icmp_imm(IntCC::Equal, tag, i64::from(want))
    }

    /// The value, failing if it is not a number.
    fn expect_number(&mut self, v: Val) -> Val {
        match v {
            Val::Dyn {
                tag,
                int,
                real,
                numeric: false,
            } => {
                let i = self.is_tag(tag, self.layout.int_tag);
                let r = self.is_tag(tag, self.layout.real_tag);
                let either = self.b.ins().bor(i, r);
                self.fail_unless(either);
                Val::Dyn {
                    tag,
                    int,
                    real,
                    numeric: true,
                }
            }
            v => v,
        }
    }

    fn known_int(&mut self, v: Val) -> Known {
        match v {
            Val::Int(_) => Known::Yes,
            Val::Real(_) => Known::No,
            Val::Dyn { tag, .. } => Known::RunTime(self.is_tag(tag, self.layout.int_tag)),
        }
    }

    /// The integer, failing if it is not one.
    fn expect_int(&mut self, v: Val) -> Value {
        match v {
            Val::Int(x) => x,
            Val::Real(_) => {
                self.fail_always();
                self.b.ins().iconst(types::I32, 0)
            }
            Val::Dyn { tag, int, .. } => {
                let yes = self.is_tag(tag, self.layout.int_tag);
                self.fail_unless(yes);
                int
            }
        }
    }

    /// A number as a real, the same as `f64::from` converts an integer. Only
    /// for a value checked by [`Gen::expect_number`].
    fn real(&mut self, v: Val) -> Value {
        match v {
            Val::Int(x) => self.b.ins().fcvt_from_sint(types::F64, x),
            Val::Real(x) => x,
            Val::Dyn { tag, int, real, .. } => {
                let is = self.is_tag(tag, self.layout.int_tag);
                let from = self.b.ins().fcvt_from_sint(types::F64, int);
                self.b.ins().select(is, from, real)
            }
        }
    }

    /// A block where an integer and a real path meet, and its value.
    fn meeting(&mut self) -> (Block, Val) {
        let block = self.b.create_block();
        let tag = self.b.append_block_param(block, types::I8);
        let int = self.b.append_block_param(block, types::I32);
        let real = self.b.append_block_param(block, types::F64);
        let v = Val::Dyn {
            tag,
            int,
            real,
            numeric: true,
        };
        (block, v)
    }

    fn jump_real(&mut self, to: Block, x: Value) {
        let tag = self
            .b
            .ins()
            .iconst(types::I8, i64::from(self.layout.real_tag));
        let int = self.b.ins().iconst(types::I32, 0);
        let args = [tag, int, x].map(BlockArg::Value);
        self.b.ins().jump(to, &args);
    }

    fn real_op(&mut self, op: BinOp, x: Value, y: Value) -> Value {
        match op {
            BinOp::Add => self.b.ins().fadd(x, y),
            BinOp::Sub => self.b.ins().fsub(x, y),
            _ => self.b.ins().fmul(x, y),
        }
    }

    /// `+`, `-` and `*`: integers while the result fits, else reals, the same
    /// as `ops::binary_numbers` gives them.
    fn arith(&mut self, op: BinOp, a: Val, b: Val) -> Val {
        let (ka, kb) = (self.known_int(a), self.known_int(b));
        if matches!(ka, Known::No) || matches!(kb, Known::No) {
            let (a, b) = (self.expect_number(a), self.expect_number(b));
            let (x, y) = (self.real(a), self.real(b));
            return Val::Real(self.real_op(op, x, y));
        }
        let (meet, v) = self.meeting();
        let both = match (ka, kb) {
            (Known::RunTime(x), Known::RunTime(y)) => Some(self.b.ins().band(x, y)),
            (Known::RunTime(x), _) | (_, Known::RunTime(x)) => Some(x),
            _ => None,
        };
        let ints = self.b.create_block();
        let reals = both.map(|both| {
            let reals = self.b.create_block();
            self.b.ins().brif(both, ints, &[], reals, &[]);
            reals
        });
        if reals.is_none() {
            self.b.ins().jump(ints, &[]);
        }
        self.b.switch_to_block(ints);
        let (x, y) = (self.int_payload(a), self.int_payload(b));
        let (n, overflow) = match op {
            BinOp::Add => self.b.ins().sadd_overflow(x, y),
            BinOp::Sub => self.b.ins().ssub_overflow(x, y),
            _ => self.b.ins().smul_overflow(x, y),
        };
        let wide = self.b.create_block();
        let tag = self
            .b
            .ins()
            .iconst(types::I8, i64::from(self.layout.int_tag));
        let zero = self.b.ins().f64const(0.0);
        let args = [tag, n, zero].map(BlockArg::Value);
        self.b.ins().brif(overflow, wide, &[], meet, &args);
        self.b.switch_to_block(wide);
        let (fx, fy) = (
            self.b.ins().fcvt_from_sint(types::F64, x),
            self.b.ins().fcvt_from_sint(types::F64, y),
        );
        let r = self.real_op(op, fx, fy);
        self.jump_real(meet, r);
        if let Some(reals) = reals {
            self.b.switch_to_block(reals);
            let (a, b) = (self.expect_number(a), self.expect_number(b));
            let (fx, fy) = (self.real(a), self.real(b));
            let r = self.real_op(op, fx, fy);
            self.jump_real(meet, r);
        }
        self.b.switch_to_block(meet);
        v
    }

    /// The `i32` reading of a value on a path where it is an integer.
    fn int_payload(&mut self, v: Val) -> Value {
        match v {
            Val::Int(x) => x,
            Val::Dyn { int, .. } => int,
            Val::Real(_) => self.b.ins().iconst(types::I32, 0),
        }
    }

    /// A comparison's truth, 0 or 1 as an `i8`.
    fn compare(&mut self, op: BinOp, a: Val, b: Val) -> Value {
        let (ka, kb) = (self.known_int(a), self.known_int(b));
        let cc = int_cc(op);
        let fcc = float_cc(op);
        match (ka, kb) {
            (Known::Yes, Known::Yes) => {
                let (x, y) = (self.int_payload(a), self.int_payload(b));
                return self.b.ins().icmp(cc, x, y);
            }
            (Known::No, _) | (_, Known::No) => {
                let (a, b) = (self.expect_number(a), self.expect_number(b));
                let (x, y) = (self.real(a), self.real(b));
                return self.b.ins().fcmp(fcc, x, y);
            }
            _ => {}
        }
        let both = match (ka, kb) {
            (Known::RunTime(x), Known::RunTime(y)) => self.b.ins().band(x, y),
            (Known::RunTime(x), _) | (_, Known::RunTime(x)) => x,
            _ => unreachable!("taken above"),
        };
        let meet = self.b.create_block();
        let truth = self.b.append_block_param(meet, types::I8);
        let (ints, reals) = (self.b.create_block(), self.b.create_block());
        self.b.ins().brif(both, ints, &[], reals, &[]);
        self.b.switch_to_block(ints);
        let (x, y) = (self.int_payload(a), self.int_payload(b));
        let c = self.b.ins().icmp(cc, x, y);
        self.b.ins().jump(meet, &[BlockArg::Value(c)]);
        self.b.switch_to_block(reals);
        let (a, b) = (self.expect_number(a), self.expect_number(b));
        let (x, y) = (self.real(a), self.real(b));
        let c = self.b.ins().fcmp(fcc, x, y);
        self.b.ins().jump(meet, &[BlockArg::Value(c)]);
        self.b.switch_to_block(meet);
        truth
    }

    /// Whether a number is not zero, as an `i8`; a NaN is not zero.
    fn truth(&mut self, v: Val) -> Value {
        match self.expect_number(v) {
            Val::Int(x) => self.b.ins().icmp_imm(IntCC::NotEqual, x, 0),
            Val::Real(x) => {
                let zero = self.b.ins().f64const(0.0);
                self.b.ins().fcmp(FloatCC::NotEqual, x, zero)
            }
            Val::Dyn { tag, int, real, .. } => {
                let is = self.is_tag(tag, self.layout.int_tag);
                let i = self.b.ins().icmp_imm(IntCC::NotEqual, int, 0);
                let zero = self.b.ins().f64const(0.0);
                let r = self.b.ins().fcmp(FloatCC::NotEqual, real, zero);
                self.b.ins().select(is, i, r)
            }
        }
    }

    fn bool_int(&mut self, c: Value) -> Val {
        Val::Int(self.b.ins().uextend(types::I32, c))
    }

    fn binary(&mut self, op: BinOp, a: Val, b: Val) -> Val {
        use BinOp as B;
        match op {
            B::Add | B::Sub | B::Mul => self.arith(op, a, b),
            B::Div => {
                let (a, b) = (self.expect_number(a), self.expect_number(b));
                let (x, y) = (self.real(a), self.real(b));
                // Dividing by zero, or by -0, is an error for the
                // interpreter to raise.
                let zero = self.b.ins().f64const(0.0);
                let by_zero = self.b.ins().fcmp(FloatCC::Equal, y, zero);
                self.fail_if(by_zero);
                Val::Real(self.b.ins().fdiv(x, y))
            }
            B::IntDiv | B::Mod => {
                let (x, y) = (self.expect_int(a), self.expect_int(b));
                // Division by zero is an error, and division by -1 wraps, but
                // Cranelift's division traps: both are left to the interpreter.
                let zero = self.b.ins().icmp_imm(IntCC::Equal, y, 0);
                self.fail_if(zero);
                let minus = self
                    .b
                    .ins()
                    .icmp_imm(IntCC::Equal, y, i64::from(-1i32 as u32));
                self.fail_if(minus);
                // Cranelift 0.124 divides by a constant -2147483648 wrongly
                // (-2147483648 DIV it gives -1), which the operator test
                // found: that divisor is left to the interpreter too.
                let min = self
                    .b
                    .ins()
                    .icmp_imm(IntCC::Equal, y, i64::from(i32::MIN as u32));
                self.fail_if(min);
                Val::Int(match op {
                    B::IntDiv => self.b.ins().sdiv(x, y),
                    _ => self.b.ins().srem(x, y),
                })
            }
            B::And | B::Or | B::Xor | B::Shl | B::Shr | B::UShr => {
                let (x, y) = (self.expect_int(a), self.expect_int(b));
                Val::Int(match op {
                    B::And => self.b.ins().band(x, y),
                    B::Or => self.b.ins().bor(x, y),
                    B::Xor => self.b.ins().bxor(x, y),
                    // Cranelift's shifts take the count modulo 32, the same as
                    // the interpreter's shifts do.
                    B::Shl => self.b.ins().ishl(x, y),
                    B::Shr => self.b.ins().sshr(x, y),
                    _ => self.b.ins().ushr(x, y),
                })
            }
            _ => {
                let c = self.compare(op, a, b);
                self.bool_int(c)
            }
        }
    }

    fn unary(&mut self, op: UnOp, a: Val) -> Val {
        match op {
            UnOp::Neg => match self.expect_number(a) {
                Val::Int(x) => Val::Int(self.b.ins().ineg(x)),
                Val::Real(x) => Val::Real(self.b.ins().fneg(x)),
                Val::Dyn { tag, int, real, .. } => Val::Dyn {
                    tag,
                    int: self.b.ins().ineg(int),
                    real: self.b.ins().fneg(real),
                    numeric: true,
                },
            },
            UnOp::Not => {
                let x = self.expect_int(a);
                Val::Int(self.b.ins().bnot(x))
            }
            UnOp::LogicalNot => {
                let t = self.truth(a);
                let c = self.b.ins().icmp_imm(IntCC::Equal, t, 0);
                self.bool_int(c)
            }
            UnOp::Truthy => {
                let t = self.truth(a);
                self.bool_int(t)
            }
        }
    }

    fn write(&mut self, to: Dest, v: Val) -> Result<(), String> {
        let p = match to {
            Dest::Hidden(k) => {
                self.hidden.insert(k, v);
                return Ok(());
            }
            Dest::Place(p) => p,
        };
        self.fail_if_heap(p)?;
        self.store(p, v)
    }

    /// Fails if `p` holds a string or an array, which needs dropping: that
    /// is for the interpreter.
    fn fail_if_heap(&mut self, p: Place) -> Result<(), String> {
        let (base, at) = self.place(p)?;
        let old = self.b.ins().load(types::I8, FLAGS, base, at);
        let heap = (self.b.ins()).icmp_imm(
            IntCC::UnsignedGreaterThanOrEqual,
            old,
            i64::from(self.layout.first_heap_tag),
        );
        self.fail_if(heap);
        Ok(())
    }

    /// Writes a number over a number or nothing.
    fn store(&mut self, p: Place, v: Val) -> Result<(), String> {
        let (base, at) = self.place(p)?;
        let l = *self.layout;
        let (int_at, real_at) = (at + l.int_offset as i32, at + l.real_offset as i32);
        let (tag, int, real) = match v {
            Val::Int(x) => (
                self.b.ins().iconst(types::I8, i64::from(l.int_tag)),
                Some(x),
                None,
            ),
            Val::Real(x) => (
                self.b.ins().iconst(types::I8, i64::from(l.real_tag)),
                None,
                Some(x),
            ),
            Val::Dyn { tag, int, real, .. } => (tag, Some(int), Some(real)),
        };
        self.b.ins().store(FLAGS, tag, base, at);
        if let Some(int) = int {
            self.b.ins().store(FLAGS, int, base, int_at);
        }
        if let Some(real) = real {
            self.b.ins().store(FLAGS, real, base, real_at);
        }
        Ok(())
    }

    fn step(&mut self, step: &Step) -> Result<(), String> {
        match *step {
            Step::Binary { op, a, b, to } => {
                let (a, b) = (self.read(a)?, self.read(b)?);
                let v = self.binary(op, a, b);
                self.write(to, v)?;
            }
            Step::Unary { op, a, to } => {
                let a = self.read(a)?;
                let v = self.unary(op, a);
                self.write(to, v)?;
            }
            Step::Move { from, to } => {
                let v = self.read(from)?;
                let v = self.expect_number(v);
                self.write(to, v)?;
            }
            Step::Jump { cond, target, when } => {
                let c = match cond {
                    Cond::Compare { op, a, b } if op.compares() => {
                        let (a, b) = (self.read(a)?, self.read(b)?);
                        self.compare(op, a, b)
                    }
                    Cond::Compare { op, .. } => return Err(format!("{op:?} is no comparison")),
                    Cond::Truth(a) => {
                        let a = self.read(a)?;
                        self.truth(a)
                    }
                };
                let on = self.b.create_block();
                let (to, args) = self.target(target);
                match when {
                    true => self.b.ins().brif(c, to, &args, on, &[]),
                    false => self.b.ins().brif(c, on, &[], to, &args),
                };
                self.b.switch_to_block(on);
            }
            Step::Commit { to, charged } => self.restart = Restart { pc: to, charged },
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn for_next(
        &mut self,
        var: Place,
        step: Operand,
        step_slot: Place,
        to: Operand,
        to_slot: Place,
        exit: u32,
        cont: u32,
    ) -> Result<(), String> {
        let v = self.read(Operand::Place(var))?;
        let s0 = self.read(Operand::Place(step_slot))?;
        let (v, s0) = (self.expect_int(v), self.expect_int(s0));
        let (n, overflow) = self.b.ins().sadd_overflow(v, s0);
        self.fail_if(overflow);
        let int_operand = |g: &mut Self, op: Operand| -> Result<Value, String> {
            match op {
                Operand::Real(_) => Err("a real STEP or TO".into()),
                op => {
                    let v = g.read(op)?;
                    Ok(g.expect_int(v))
                }
            }
        };
        let s = int_operand(self, step)?;
        let t = int_operand(self, to)?;
        // `var` and `step_slot` hold integers, checked above; `to_slot`
        // may hold anything. Nothing is written until all is checked.
        self.fail_if_heap(to_slot)?;
        for (place, value) in [(var, n), (step_slot, s), (to_slot, t)] {
            self.store(place, Val::Int(value))?;
        }
        let up = self.b.ins().icmp_imm(IntCC::SignedGreaterThanOrEqual, s, 0);
        let below = self.b.ins().icmp(IntCC::SignedLessThanOrEqual, n, t);
        let above = self.b.ins().icmp(IntCC::SignedGreaterThanOrEqual, n, t);
        let more = self.b.ins().select(up, below, above);
        let (yes, yes_args) = self.target(cont);
        let (no, no_args) = self.target(exit);
        self.b.ins().brif(more, yes, &yes_args, no, &no_args);
        Ok(())
    }
}
