# JIT design

This document plans native code for the interpreter's hot loops. It covers
the choice of code generator, what gets compiled and from what, the
contract between native code and the VM, the memory safety argument,
determinism, the cost to the code base, and a staged plan. The numbers come
from a prototype (stage 1), behind the `jit` feature of `sb-core`, measured
under qemu and on an RK3326 handheld.

[benchmark-results.md](benchmark-results.md) has the interpreter's own
numbers, and [security.md](../development/security.md) the rules for `unsafe` code.

## Summary

**Go, with Cranelift, in stages, the feature off by default.** The
prototype compiles loops of integer and real arithmetic, comparisons and
integer FORs, and leaves everything else to the interpreter. On the
handheld, TPERF's VINT scene takes 1.80 ms of program time per frame instead
of 7.99 (4.4 times as fast) and 3.9 times as fast as the `unchecked-vars`
build; the covered loops run 2.2 to 3.7 times as fast, in step with
aarch64 instruction counts that fall by 48 to 72%. Scenes it does not
cover move by 5% or less, within the noise of code layout, and every
checksum is the same.

Three findings limit what it is worth in its current form:

- **Coverage.** GAME, VREAL, VARRAY and VDEF gain nothing: their loops read
  array elements, call builtins or DEFs, which the prototype does not
  compile. Array elements (stage 3) are what games loop over.
- **Compile time.** Cranelift takes about 1 ms per entry on the Cortex-A35,
  6.4 ms for a loop of six. Compiling every loop at load cannot ship;
  compiling only hot loops (stage 2) must come first.
- **Size and trust.** 1.6 MB more in the aarch64 binary, about twice the
  CPU time for a clean build, and generated code that no Rust check or
  Miri covers. The operator tests found a Cranelift miscompilation.

The boundary with the VM is clean: a region is entered at one entry, returns
an address and the budget left, and fails back to `step` the same as a chain
does. The VM needed one `match` arm, two calls after `fuse::check`, and the
value layout fixed with `#[repr(u8)]`.

## Where the time goes now

After four passes over the fused code, on aarch64 a three-address chain
step costs 50 to 60 instructions, choosing an entry 16 to 19, reading a
variable 5 to 10 and a DEF call with its return about 300
([benchmark-results.md](benchmark-results.md#variables-and-constants-from-d081e7a)).
Of a step's instructions, the operator's own work is 2 to 5. The rest is
dispatch, tag tests that repeat for every operand, the bounds check and
address of each place, and spilling the loop's state around a large
`match`. A JIT removes the dispatch, keeps the loop state in registers,
folds constants and tag tests the code generator can see, and computes
every address at compile time.

## Backend

| | Cranelift | dynasm-rs or hand-written templates | Copy-and-patch |
|---|---|---|---|
| Architectures | x86-64, aarch64, riscv64, s390x | dynasm-rs: x86-64, x86, aarch64, riscv; templates: what we write | what the stencil compiler (LLVM) targets |
| arm32, 32-bit x86 | no | dynasm-rs: x86 yes, arm32 no | possible, more stencils |
| Binary size added (aarch64, `dist`, stripped `smilebasic-check`) | 1.57 MB (6.13 to 7.70 MB, measured) | under 100 KB (estimate) | stencils only, under 100 KB (estimate) |
| Build time added | a clean x86-64 `release` build of `smilebasic-check`: 105 to 224 s of CPU, 40 to 67 s wall (measured) | small | an LLVM step at build time, or checked-in stencils |
| Compile speed on a Cortex-A35 | 1.06 ms per entry, 6.4 ms for a loop of six entries (measured); x86-64 6.7 times as fast | tens of µs per region (estimate: one pass, no register allocation) | fastest: memcpy and patch |
| Code quality | register allocation, constant folding, GVN, instruction selection | what the templates give; values stay in memory between operations unless we write an allocator | stencils' quality, values pass through fixed registers or memory |
| Work per architecture | none: the IR is the same | all of it: templates, encodings, calling convention, tests | one stencil set per architecture, built by a tool we own |
| License | Apache-2.0 WITH LLVM-exception (Cranelift, regalloc2, target-lexicon), MIT/Apache (the rest) | dynasm-rs MPL-2.0 (file-level copyleft); templates ours | ours, but the build needs LLVM |
| Cross-build with `tools/build_handheld.py` | works unchanged (pure Rust; the `dist` cross-build of the prototype took 71 to 137 s after a change in `sb-core`, against 40 to 70 s without the feature) | works | needs clang for the target at build time |

The interpreter's semantics are in Rust (`ops::binary_numbers`). With
Cranelift, each operator is lowered once to an architecture-neutral IR, and
every target gets the same lowering. Templates and copy-and-patch repeat
that lowering per architecture, which doubles the places an operator's
rules live and the tests they need.

Cranelift's cost is size and compile speed. Both are measured below. The
compile speed problem has a known fix (compile only hot loops, stage 2); the
size is the price of the register allocator and instruction selector,
which the other two would need us to write.

**Recommendation: Cranelift**, pinned to the newest release that supports
the workspace's `rust-version` (0.124 for Rust 1.87). arm32 and 32-bit x86
builds stay interpreted.

## What is compiled, and from what

The input is the fuser's output, [`Compiled::fast`](../../crates/sb-core/src/bytecode.rs)
and [`Compiled::micro`](../../crates/sb-core/src/fuse.rs), not the stack
bytecode or the AST. The fuser has already found statements, turned their
expressions into three-address steps with hidden places, and folded
constants into steps. Its entries also fix the boundaries where a time
slice can end. Native code that follows the same entries ends slices at
the same instructions, which the tests compare.

### Regions

A region is a loop: an entry that a later entry jumps back to (the head),
and the entries with native forms reachable from it up to the last entry
that jumps back, at most 64. Native code is one function per region. It
starts at the head, runs entries and jumps between them directly, and
returns when the next entry is outside the region, costs more than the
budget left, or cannot do its work. A region that does not come back to its
head is not compiled, because entering native code costs a call.

The VM puts `FastOp::Native(n)` in place of the head's entry. `run_fused`
calls region `n` there and continues from the `pc` it returns. Every other
address keeps its interpreter entry, so the interpreter can continue from
any address where native code stops.

### Entry kinds

The prototype compiles:

- `Chain` entries whose steps are operators, moves, jumps and commits: the
  integer and real forms of `+ - * /`, `DIV`, `MOD`, `AND OR XOR << >> >>>`,
  the six comparisons, unary `-`, `NOT`, `!`, truthiness, and the
  comparison and truth jumps;
- the stack-free entries of numbers: `Nop`, `Jump`, `Move`, `AddStore` and
  the related entries with or without a constant, `VarsStore`, the `Unless*`
  jumps, `VarsJump`, `VarsJumpTrue`;
- `ForNext` when its TO and STEP are separate from the counter (`apart`) and
  integers or integer constants.

With the feature, the fuser weighs a chain's step at 5 instead of 6 tenths
of an entry, so that a statement of two operators becomes a chain rather
than two stack entries (`X#=X#+U#*T#`). Every form is the same program;
`tests/fused.rs` checks this for both weights.

Next, in order of the time loops spend on them: array elements (`Index1`,
`Index2`, `StoreIndex*`), pure builtins, real FOR loops, then calls (see the
plan).

### Entering and leaving

```rust
#[repr(C)]
pub struct Ctx {
    pub tables: [*mut u8; 2], // globals and constants; the running DEF's variables
    pub pc: u64,              // out: where it stopped
    pub left: u64,            // in and out: the budget
}
extern "C" fn(*mut Ctx) -> u32 // EXIT or FAIL
```

The function loads both table pointers and the budget once, keeps them in
registers, and stores `pc` and `left` back when it returns.

### Values and guards

A variable may hold any kind of value regardless of its suffix
([sb4-pitfalls.md](../smilebasic4/sb4-pitfalls.md)), so every read tests the tag. The code
generator tracks, for each value it works with, whether it is known to be
an integer, known to be a real, or known only at run time (a variable
read, or the result of an operator on such a value). An operator on values
of known kinds is straight-line code; on a value known at run time, it
tests the tags and takes an integer or a real path, which meet again after
it. Constants, which fused steps carry, are known, and comparisons and bit
operators always give integers, so most tests fold away. A read loads the
tag and both payload readings; the tag decides which one is used.

A guard that fails ends the region with `FAIL` and the place to continue
from: the statement's start, or the last `Commit`'s target with the
instructions charged to it, the same as a failing chain step gives them now.
`run_fused` then stops the same way as for a failing chain, and `step` runs
the statement one instruction at a time, with the interpreter's own rules
and errors. Native code fails on:

- a value that is not a number where one is needed (a string, an array, an
  omitted value);
- `DIV` and `MOD` by 0, by -1 and by -2147483648, and `/` by zero: the
  interpreter raises the error or computes the wrapped value;
- a real operand of `DIV`, `MOD`, `NOT` or a bit operator, which the
  interpreter truncates or checks;
- a write over a string or an array, which must be dropped;
- in `ForNext`, anything but integers, and a counter that overflows.

An integer `+ - *` that overflows does not fail: it gives the real that
`ops::binary_numbers` gives, inline.

### One place for the rules

The operators' rules stay in `ops.rs`. Native code implements only the
cases that are a single machine operation on each type (`binary_numbers`'
fast paths), and fails on everything else. `crate::jit`'s tests run every
native operator, with the right side from a place and carried as a
constant, on 29 values (integers at the edges, ±0, NaN, infinities, reals
outside the 32-bit range, an omitted value, a string), and require the
same bits as `binary_numbers` or a failure where `binary_numbers` fails.
This test found a Cranelift 0.124 miscompilation: `-2147483648 DIV
-2147483648` with the divisor a constant gave -1. That divisor fails now.

Every change to an operator's rules in `ops.rs` must also change its
lowering in `sb-jit`, or make it fail there. The operator tests fail until
both agree.

## The contract with the VM

### Memory layout

With the feature, `Value` is `#[repr(u8)]`. The language then fixes its
layout: a tag byte at offset 0 numbered by the variants' order (`Default`
0, `Int` 1, `Real` 2, `Str` 3, `Array` 4), and each variant's fields as a
`#[repr(C)]` struct after the tag would hold them, so an `i32` at 4 and an
`f64` at 8, in 16 bytes on 64-bit targets. `crate::jit::layout` derives the
offsets from mirror structs with `offset_of!`, and a test checks the tags
and payloads of real values. Reordering `Value`'s variants breaks that
test.

The tables are the slices `run_fused`'s `Vars` view holds: the globals'
table (globals, then the chains' eight hidden places, then constants) and
the running DEF's variables. Native code names places by table and index,
the same as `fuse::Var` does, and turns them into constant offsets at
compile time. The hidden places are kept in registers: they live only
within an entry.

### Budget, `pc` and `executed`

Each entry block starts with the interpreter's test, `len > left`, and
leaves before the entry if `len` is more than `left`; otherwise it subtracts
`len`. A failure returns the budget from before the entry, minus what the
chain charged before its last `Commit`. `run_fused` writes `pc` and
`executed` from what native code returns, the same as from its own loop, so
errors and resumption see the same values. `tests/fused.rs` runs its
programs in slices of 1, 2, 3, 5, 7, 13, 100 and 1,000,000 instructions, and
`tests/fuse_random.rs` its random programs in slices of 1, 2, 5, 13 and
1,000,000, fused (with native code, in a `jit` build) and unfused, and both
compare where each slice ends, how it ends and what is printed.

### What turns it off

`run_code` runs fused code only when no debugger step, TRACE, watch,
breakpoint, pending ON BREAK or work report (the speed model) is active.
Native code runs inside `run_fused`, so the same tests turn it off.
`SB_JIT=0` in `smilebasic-check` (and `sb_core::jit::set_enabled`) makes no
native code for programs loaded after it, to compare the same binary with
and without it.

### Calls

The prototype ends a region at any call. The full version:

- **DEF calls and returns within the slot**: the region calls the callee's
  region, or returns to `run_fused`, which makes the call the way it does
  now and enters the callee's native code at its first loop. `run_fused`
  handles the frame either way.
- **GOSUB and RETURN**: as now, in `run_fused`.
- **Pure builtins** (`ABS`, `SQR`, `SIN`, `MIN` and the others
  `builtins::is_pure_call` names): native code calls `extern "C"`
  wrappers that take their arguments in a buffer, run the same Rust
  function `step` runs, and report failure as a status. Calling the same
  Rust functions keeps their results the same to the bit; Cranelift's own
  `sqrt` would match, but no other function of the C library or `libm` is
  required to.
- **Host commands and the other builtins**: native code returns to
  `run_fast`, which calls `host_call` and `builtin_call` as now.

### Panics and unwinding

Native code calls nothing in the prototype, so nothing can unwind through
it. Helpers called from native code in later stages must be `extern "C"`
functions, which abort on an escaping panic since Rust 1.81. To keep
`Internal Error` rather than an abort, each helper wraps its body in
`catch_unwind` and returns a status that makes the region fail. `run_fused`
then re-raises the panic outside native code.

## Memory safety

Native code is outside Rust's checks and outside Miri. Its memory accesses
are bounded by construction: every address it computes is a table pointer
from `Ctx` plus a constant offset that `sb-jit`'s one address function
emits, and that function records the highest index per table (`need`).

| Invariant | Established by |
|---|---|
| `Value`'s layout matches `Layout` | `#[repr(u8)]`; `layout()` from `offset_of!`; a test per target |
| Every place native code touches is below `need` | `sb-jit`'s `place()`, the only address computation |
| The tables are at least `need` long | `Native::enter` compares `need` with the `Vars` view's lengths before each call |
| Nothing else uses the tables during the call | `Vars::native_tables` takes `&mut self` of the view that holds the borrows |
| Native code writes only valid numbers, and never over a string or array | tags written are the integer or real tag; the write guard fails on tags from 3 |
| `Ctx` is laid out as native code reads it | `#[repr(C)]`; a test of its offsets |
| Code is mapped while it runs | `Code::run` borrows the `Code`, which the program's `Rc` owns |
| No unwinding through native frames | native code calls nothing |
| Native code matches the calling convention | Cranelift's `CallConv::triple_default`, Rust's `extern "C"` |

`fuse::check` still bounds the interpreter's own reads; a program with
`FastOp::Native` entries when the VM takes it (one cloned from another VM)
runs without fused code, because `check` refuses them.

Checks that stay in native code, in aarch64 instructions: a tag test is a
compare and a branch (2), a write's string test a load, compare and branch
(3), the budget test of an entry a compare, branch and subtract (3), an
overflow test one branch. There is no bounds check per access: `need` is
compared once per call. Keeping one on each access (a compare with a length
in a register, and a branch) would add 2 instructions to each of the 14
reads and writes in an iteration of VINT's loop (6 for the NEXT, 8 for the
two statements). That is 28 instructions, a quarter of the 120 that the
iteration runs natively on aarch64.

### W^X

| OS | How |
|---|---|
| Linux (prototype) | `mmap` read-write, copy, `mprotect` read-execute; on aarch64, `dc cvau`/`ic ivau` by `CTR_EL0`'s line sizes, then `isb` |
| Windows | `VirtualAlloc` read-write, `VirtualProtect` to `PAGE_EXECUTE_READ`, `FlushInstructionCache` |
| macOS on Apple Silicon | `mmap` with `MAP_JIT`, `pthread_jit_write_protect_np(0)` to write and `(1)` after, `sys_icache_invalidate`; signed builds need the `com.apple.security.cs.allow-jit` entitlement |
| macOS on Intel | the same as Linux |
| Systems that refuse executable memory (SELinux `execmem` denied, iOS, consoles) | `mmap` or `mprotect` fails, the region is not compiled, the program runs interpreted |

Pages are never writable and executable at the same time. The handheld's
kernel, 4.4, does not restrict mapping memory a process wrote as executable.

### Code lifetime

A program the VM holds is behind an `Rc` and never changes. Its native code
lives in `Compiled::native` and is dropped with the program: the program of
a slot, of EXEC, or of a line typed in DIRECT mode. The prototype maps a
page per region and frees it when the program is dropped; the full version
puts a program's regions in one arena. Code is never patched after it is
made.

### Tests

- Differential runs three ways: unfused, fused without native code, fused
  with it, at every slice budget. The prototype's default build compares
  fused against unfused on `tests/fused.rs`'s programs and the 1,500
  random programs of `tests/fuse_random.rs`; its `jit` build compares
  native against unfused on both, and the loops of `NATIVE_LOOPS` all three
  ways, and asserts that native code ran. Stage 2 runs every set all three
  ways.
- The operator tests above, on every target the JIT builds for.
- aarch64 in the test suite: `tools/build_handheld.py --jit --test` runs
  the tests under qemu-user, which runs generated code (and gives the same
  TPERF checksums).
- Fuzzing (stage 2): programs from the random generator and from a grammar
  fuzzer, run with and without native code, comparing output, errors and
  slice ends.
- AddressSanitizer on the Rust side (nightly `-Zsanitizer=address`): finds
  native writes that land in a redzone, not all out-of-bounds writes.
- Under Miri, `attach` makes no native code (`cfg(miri)`), so Miri can
  check the glue with the feature on, but not the native code; Miri has
  not been run with the feature on yet.

Two things stay unprovable: that Cranelift compiles our IR correctly (once
it did not; see above), and that the lowering is right for values the tests
do not try. A code generator bug means wrong results in a program, which the
user sees as SmileBASIC 4 behaving differently. If the bug reaches an
address, it means memory corruption that an untrusted program can cause. The
design leaves only the address function and `need` able to cause memory
corruption: a wrong operator or a wrong index within `need` gives wrong
values, not unsafety.
Like `unchecked-vars`, the feature is for builds whose users accept that.

## Determinism

- **Floating point.** Cranelift emits IEEE double operations, one
  instruction each (SSE2 on x86-64, scalar FP on aarch64), the same as rustc
  does for the interpreter. It never fuses a multiply and an add. Integer to
  real is `fcvt_from_sint`, exact, like `f64::from(i32)`. Comparisons use
  ordered conditions and `!=` the unordered one, the same as Rust's
  operators. NaN bits come from the same hardware instructions as in the
  interpreter on the same machine; Cranelift's NaN canonicalization is off.
- **Integers.** Overflow is tested with Cranelift's `sadd_overflow` and the
  related instructions; shifts take the count modulo 32, like
  `wrapping_shl`; `DIV` and `MOD` run natively only where they cannot trap.
- **The ISA.** The code generator targets each architecture's baseline,
  not the host's extensions, so every machine of an architecture runs the
  same instructions.

TPERF's checksums at 400 and 3600 frames are the same with the feature on
x86-64 and on aarch64 under qemu, and every device run's checksums match
those of the default build.

## Costs to the code base

- **A crate and a feature.** `crates/sb-jit` (Cranelift, executable memory,
  cache maintenance; it has no knowledge of SmileBASIC) and
  `crates/sb-core/src/jit.rs` (which loops, the lowering of entries and
  steps, the call, the operator tests). The `jit` feature of `sb-core` is
  forwarded by `sb-runtime`, `sb-check` and `sb-sdl`, like `unchecked-vars`,
  and `tools/build_handheld.py --jit` builds and tests it.
- **Changes elsewhere, all behind `cfg(feature = "jit")`:** `Value` gets
  `#[repr(u8)]` (`value.rs`); `Compiled` a `native` field
  (`bytecode.rs`); `FastOp` a `Native` variant, `check` a refusal of it and
  the chain weighting a lower step cost (`fuse.rs`, which also makes
  `TEMPS` visible to the crate); `Vars` a `native_tables` method
  (`vars.rs`); `vm.rs` two calls of `jit::attach` after `fuse::check` and
  one arm of `run_fused`'s match. A default build compiles the same code
  as before, with no new `unsafe` and no new dependency in its binary.
- **What every later change must also touch.** An operator's rules in
  `ops.rs`: its lowering in `sb-jit`, or a failure there; the operator
  tests catch a mismatch. A new `FastOp` kind: nothing, because `jit::lower`
  leaves kinds it does not know to the interpreter; a new `Micro` kind: an
  arm in `jit::lower_micro`, whose match has no wildcard. A
  change to how `run_fused` charges the budget or where it stops: the same
  in `sb-jit`'s entry blocks. `Value`'s variants: their order, which the
  layout test checks.
- **Build time and size.** Cranelift adds about 120 s of CPU to a clean
  build (x86-64 `release`, 105 to 224 s) and 1.6 MB to an aarch64
  `smilebasic-check` (6.13 to 7.70 MB, `dist`, stripped).
- **CI.** Clippy and the tests with `--features jit` on x86-64 (and
  aarch64 hosts), `tools/build_handheld.py --jit --test` under qemu for
  aarch64, the operator tests on both, and later the fuzzer.

## The prototype's numbers

The prototype is in `crates/sb-jit` and `crates/sb-core/src/jit.rs`, on
`main` at `9c1b55a`. It runs on Linux on x86-64 and aarch64.

### Method

The method is that of
[benchmark-results.md](benchmark-results.md#variables-and-constants-from-d081e7a):
x86-64 counts with `perf stat -e instructions:u` on the `release` build,
aarch64 with qemu's Cortex-A35 model on the `dist` build cross-built by
`tools/build_handheld.py`, from whole blocks under `setarch -R`. A loop's
count is the difference between programs that run it 3200 and 6400 times,
in a DEF with its own locals. A scene's count comes from TPERF, running the
scene alone with `#ONLY`, over frames 3 to 83 (x86-64) and 3 to 6
(aarch64), and covers the program and the CPU drawing.
Four builds: the default, `unchecked-vars` (the interpreter's best),
`jit`, and the `jit` binary with `SB_JIT=0` (no native code: what the
feature costs where it does not apply). qemu runs the generated code:
TPERF's checksums at 400 and 3600 frames are `e8e5f6dc467db861` and
`a6e1ed07824b1ad8` with the feature, on x86-64 and on aarch64 under qemu,
the same as without it.

The device is the RK3326 handheld of the earlier sets, front end stopped,
benchmark pinned to core 3, `--bench --fixed 80 --log`, the median of
`run_us` over frames 10 to 69, best of three runs, the four builds
alternating in rotated order. The loops ran 3000 times per frame with a
`VSYNC` after, in a program that clears the console, so that every build's
checksum is the same; it was.

### Loops

The covered loops are VINT's (`A%=(A%+I*3) AND 1023`, `B%=B% XOR (A%<<1)`),
the counters (`C=C+1:IF C>=60 THEN C=0:T=T+1`, `IF S<100 THEN S=S+2 ELSE
S=S-100`, `L=L+(I AND 3)`), the IF chain (`K=I MOD 5` and an `IF`/`ELSEIF`
chain on it) and the scalar reals (`X#=X#+U#*T#:Y#=Y#+W#*T#`, `IF X#>100
THEN X#=X#-100`, `W#=W#+0.01`). The others are controls the prototype
does not compile: a GOSUB, a function DEF call, VREAL's loop (`SQR`,
`SIN`) and VARRAY's (array elements).

Instructions per iteration of the loop:

| Loop | aarch64 default | aarch64 unchecked | aarch64 jit | aarch64 jit off | x86-64 default | x86-64 unchecked | x86-64 jit | x86-64 jit off |
|---|---|---|---|---|---|---|---|---|
| Empty FOR | 130.8 | 118.9 | 43.9 | 134.1 | 156.6 | 135.9 | 39.6 | 160.1 |
| VINT's | 429.0 | 374.8 | 119.6 | 443.7 | 478.2 | 369.7 | 97.8 | 492.3 |
| Counters | 466.3 | 428.5 | 204.9 | 478.3 | 542.5 | 459.4 | 178.8 | 557.8 |
| IF chain | 388.1 | 355.6 | 156.3 | 395.7 | 449.4 | 395.9 | 137.1 | 456.5 |
| Scalar reals | 536.9 | 492.3 | 280.5 | 540.4 | 618.9 | 564.8 | 237.8 | 615.4 |
| GOSUB | 261.8 | 259.2 | 265.0 | 264.9 | 303.1 | 281.0 | 309.1 | 309.4 |
| Function DEF | 623.6 | 626.9 | 653.6 | 653.8 | 728.9 | 711.0 | 764.2 | 763.9 |
| VREAL's | 720.8 | 634.3 | 730.7 | 730.3 | 763.9 | 625.9 | 781.1 | 780.8 |
| VARRAY's | 1143.9 | 973.0 | 1163.5 | 1163.0 | 1229.0 | 922.3 | 1265.4 | 1267.1 |

With native code off, the jit build runs 2 to 4% more instructions than
the default on the loops that native code would cover (VINT's 443.7 on
aarch64, 492.3 on x86-64), and the same as with it on the others.

Device, ms of program time per frame:

| Loop | Default | Unchecked | Jit | Jit off | Jit against default | Jit against unchecked |
|---|---|---|---|---|---|---|
| VINT's | 1.42 | 1.23 | 0.38 | 1.39 | -73.0% | -68.9% |
| Counters | 1.51 | 1.34 | 0.57 | 1.59 | -62.3% | -57.5% |
| IF chain | 1.27 | 1.14 | 0.47 | 1.28 | -62.7% | -58.4% |
| Scalar reals | 1.69 | 1.46 | 0.75 | 1.78 | -55.5% | -48.3% |
| Function DEF | 1.91 | 1.79 | 2.03 | 2.02 | +6.0% | +13.1% |
| VARRAY's | 3.85 | 3.19 | 3.77 | 3.76 | -2.1% | +18.1% |

The covered loops run 2.2 to 3.7 times as fast as the default build and 1.9
to 3.2 times as fast as `unchecked-vars`, on the device as in the counts.
The empty FOR, 44 aarch64 instructions natively, shows what is left:
the loop reads its counter and its STEP slot from memory each iteration,
tests their tags, and writes the counter and both slots back.

### Scenes

Instructions per frame, in millions:

| Scene | aarch64 default | aarch64 unchecked | aarch64 jit | aarch64 jit off | x86-64 default | x86-64 unchecked | x86-64 jit | x86-64 jit off |
|---|---|---|---|---|---|---|---|---|
| VINT | 8.14 | 7.17 | 2.58 | 8.40 | 8.74 | 6.81 | 2.02 | 9.01 |
| VREAL | 6.87 | 6.12 | 6.95 | 6.96 | 7.00 | 5.76 | 7.16 | 7.16 |
| VSTR | 12.82 | 13.03 | 12.96 | 12.93 | 11.53 | 11.60 | 11.81 | 11.81 |
| VARRAY | 11.85 | 10.14 | 12.04 | 12.04 | 12.44 | 9.37 | 12.82 | 12.82 |
| VDEF | 5.36 | 5.36 | 5.59 | 5.58 | 5.85 | 5.79 | 6.17 | 6.17 |
| GAME | 19.94 | 19.42 | 19.97 | 19.97 | 28.22 | 27.45 | 28.36 | 28.36 |

Device, ms of program time per frame:

| Scene | Default | Unchecked | Jit | Jit off | Jit against default | Jit against unchecked |
|---|---|---|---|---|---|---|
| VINT | 7.99 | 7.07 | 1.80 | 7.88 | -77.5% | -74.6% |
| VREAL | 7.05 | 6.10 | 7.41 | 7.43 | +5.2% | +21.5% |
| VSTR | 15.62 | 14.94 | 15.34 | 15.25 | -1.8% | +2.7% |
| VARRAY | 12.72 | 10.45 | 12.39 | 12.23 | -2.6% | +18.6% |
| VDEF | 5.33 | 5.04 | 5.59 | 5.58 | +4.8% | +10.8% |
| SPMOVE | 9.06 | 8.63 | 9.49 | 9.49 | +4.7% | +10.0% |
| GAME | 8.47 | 7.95 | 8.52 | 8.53 | +0.5% | +7.2% |

VINT, the one scene the prototype covers, takes 1.80 ms instead of 7.99,
4.4 times as fast, and 3.9 times as fast as `unchecked-vars`. Its
program runs 3.6 times fewer aarch64 instructions (7.7 against 2.2 million
per frame, not counting the 0.4 million of CPU drawing in the counts
above), and native code runs about a fifth more of them per cycle on the
in-order core: 0.83 ms per million against 1.03. The other scenes move by
5% or less against the default build, within the noise of code layout.
Their counts move by +0.2% (GAME) to +4.3% (VDEF) on aarch64 and +0.5% to
+5.5% on x86-64, the same with native code off. That is what the feature
costs where it does not apply, from `Value`'s fixed layout, the lower chain
weight and where LLVM lays out `run_fused`. `unchecked-vars` stays ahead of
the jit build on every uncovered scene, because the jit build does not take
its unchecked reads; both features build together.

An earlier version tested every entry for `FastOp::Native` before the
interpreter's `match` (to run the replaced entry when native code
declined): on the device, VDEF took 15% and a function DEF loop 20% longer
with native code off, and aarch64 counts of the DEF loop rose by 12%. The
`Native` arm inside the `match` costs nothing on other entries; the price is
that a region native code declines runs its head's first instruction
through `step`.

### Compiling

| Program | Regions | Entries | Code (aarch64) | Device | x86-64 |
|---|---|---|---|---|---|
| 40 loops of four statements | 40 | 240 | 55,356 bytes | 254 ms | 38 ms |
| TPERF (6262 entries) | 3 | 7 | 1,564 bytes | 28 ms | 5.9 ms |

On the device a region of six entries takes 6.4 ms to compile, 1.06 ms per
entry, and the largest TPERF region (three entries) 3.8 ms; x86-64 is 6.7
times as fast. Making Cranelift's description of the machine takes 35 to 80
µs once per thread. TPERF's other 20 ms on the device go to computing every
entry's native form, which allocates for each of its 6262 entries. TPERF
loads in 62 ms with the feature against 35 ms without. Native code is
about 230 bytes of aarch64 per entry, 270 of x86-64.

Compiling every loop at load is too slow for large programs on the device:
a program of 400 loops would wait two and a half seconds. Stage 2 compiles
a loop only when it has run often, and computes native forms only for the
entries of such loops.

### Size and build time

| | Default | Jit |
|---|---|---|
| aarch64 `smilebasic-check`, `dist`, stripped | 6.13 MB | 7.70 MB |
| x86-64 `smilebasic-check`, `release`, stripped | 7.29 MB | 11.63 MB |
| aarch64 `run_fast` | 45,436 bytes | 47,300 bytes |
| Clean x86-64 `release` build of `smilebasic-check`, CPU | 105 s | 224 s |
| The same, wall, 16 threads | 40 s | 67 s |
| `dist` cross-build for aarch64 after a change in `sb-core` | 40 to 70 s | 71 to 137 s |

The clean builds ran while the machine also ran qemu; the CPU times are the
better guide.

## Plan

Each stage ends with every test of CONTRIBUTING.md passing in the default,
`unchecked-vars` and `jit` builds, `--jit --test` passing under qemu, and
TPERF's checksums unchanged on both architectures. Device times follow
[benchmarking.md](benchmarking.md#measuring-on-a-handheld): a change under
about 10% counts only where the instruction counts move with it.

1. **Prototype (done).** Loops of chains and stack-free entries of numbers,
   integer FOR. Exit: the numbers in this document.
2. **A base to ship behind the option.** Compile a loop when its head has
   run some hundreds of times, not every loop at load (start-up cost
   below); one code arena per program; Windows and macOS executable
   memory; the replaced entry kept so that a region native code gives up
   on runs interpreted rather than a `step` each iteration; real FOR
   loops; counters, step and limit slots kept in registers through a
   loop and written back at its exits (the empty FOR is 44 aarch64
   instructions natively, mostly loads and stores of the three slots);
   the fuzzer. Exit: VINT and the scalar loops as measured here; no
   scene slower by more than the noise with instruction counts behind it;
   start-up under 5 ms for TPERF on the device.
3. **Array elements.** `Index1`, `Index2` and `StoreIndex*`, through a
   fixed layout for an array's header (dimensions, element type, data
   pointer and length) or through `extern "C"` helpers for the first
   version. VARRAY, GARRAY, SPMOVE and the sprite loops spend most of their
   steps there. Expected: VARRAY's loop at the speed of VINT's per step, so
   VARRAY 12.7 to about 4 to 5 ms (an estimate); GAME, whose interpreter is about a
   quarter of its frame and mostly reads elements into reals, 8.5 to about
   7 ms (an estimate). Exit: those, with the `need` argument extended to an array's
   length checked per access (2 instructions, kept).
4. **Pure builtins** through helpers that run the same Rust functions:
   VREAL (`SQR`, `SIN`), `ABS`, `MIN`, `MAX` in game loops. Expected:
   VREAL 7.1 to about 3 to 4 ms (an estimate); the helpers' calls cost more than the
   operators, so less than VINT's factor.
5. **Calls.** DEF calls and returns within a slot entering the callee's
   native code, GOSUB, and returning to `run_fast` for host commands
   without leaving the loop's state in memory. VDEF, game loops of DEF
   calls and SPOFS. The gain here is the smallest per instruction
   compiled: a DEF call is about 300 aarch64 instructions of frame work
   that native code does the same way as the interpreter.

Sensible places to stop: after stage 2, if programs people run are not
the integer and real loops it covers (its gain on GAME is nil); after stage
3, which covers what games loop over; stage 5 only if game loops of calls
dominate measured projects.


## Dependencies

The crates that the `jit` feature adds, at the versions Cargo picks for Rust
1.87. None is in a default build.

| Crate | Version | License |
|---|---|---|
| cranelift-codegen, -frontend, -bforest, -bitset, -codegen-shared, -control, -entity, -assembler-x64 | 0.124.3 | Apache-2.0 WITH LLVM-exception |
| cranelift-isle, -codegen-meta, -assembler-x64-meta, -srcgen (build time only) | 0.124.3 | Apache-2.0 WITH LLVM-exception |
| regalloc2 | 0.13.5 | Apache-2.0 WITH LLVM-exception |
| target-lexicon | 0.13.6 | Apache-2.0 WITH LLVM-exception |
| wasmtime-internal-math | 37.0.3 | Apache-2.0 WITH LLVM-exception |
| gimli | 0.32.3 | MIT OR Apache-2.0 |
| hashbrown | 0.15.5, 0.17.1 | MIT OR Apache-2.0 |
| bumpalo | 3.20.3 | MIT OR Apache-2.0 |
| allocator-api2 | 0.2.21 | MIT OR Apache-2.0 |
| arbitrary | 1.5.0 | MIT OR Apache-2.0 |
| indexmap | 2.14.2 | Apache-2.0 OR MIT |
| equivalent | 1.0.3 | Apache-2.0 OR MIT |
| rustc-hash | 2.1.3 | Apache-2.0 OR MIT |
| smallvec | 1.16.5 | MIT OR Apache-2.0 |
| log | 0.4.34 | MIT OR Apache-2.0 |
| libm | 0.2.16 | MIT |
| heck (build time only) | 0.5.0 | MIT OR Apache-2.0 |

All allow use in an `MIT OR Apache-2.0` project. Binaries that include
them must carry the Apache-2.0 notice; the LLVM exception waives the
attribution requirement for object code. Cranelift's default features
(`unwind`, `timing`) are off; `host-arch` builds only the target's own
backend. Newer Cranelift releases need a newer Rust (0.125 needs 1.88,
0.137 needs 1.97).

## Recommendation

Go, with Cranelift, staged as above, the `jit` feature off by default and
out of every build handed to others until stage 2's fuzzing and stage 3's
array checks are in.

The gain where native code applies is far outside the device's noise: 4.4
times on VINT and 2.2 to 3.7 times on the scalar loops, against 1.1 to 1.2
times for `unchecked-vars`, the interpreter's best. The boundary with the
VM needed no redesign. It does not yet help a game: GAME's
loops read array elements and call DEFs and builtins. Stage 2 is needed
before the feature is usable at all (compile time), and stage 3 decides
whether it is worth keeping: if GAME and the sprite scenes do not gain 10%
or more on the device with arrays compiled, stop there and keep the
feature as an experiment.
