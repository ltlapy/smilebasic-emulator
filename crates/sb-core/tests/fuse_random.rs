//! Random programs from a fixed seed, run fused and one instruction at a
//! time: what they print, their errors, and where each time slice ends must
//! be the same. The statements are the kind the fuser's chains take, with
//! values of every type in the variables, so that each step of a chain meets
//! what it cannot do as well as what it can. DEFs of every form and a host's
//! commands are called from them, in expressions and chains, so that each
//! call's fast path meets errors in its callee and slices that end inside it.
//! Each program first runs slot 1 with EXEC, whose COMMON DEFs it calls, so
//! that the view of the running DEF's variables follows calls into another
//! slot and back; DEFs whose variables have the globals' names, and deep
//! recursion, move it too.

mod common;

use common::CmdHost;
use sb_core::fuse::FastOp;
use sb_core::vm::{Status, Vm};

/// xorshift64*, so that the programs are the same on every run.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }

    fn pick<'a>(&mut self, items: &[&'a str]) -> &'a str {
        items[self.below(items.len())]
    }
}

const NUMBERS: &[&str] = &[
    "0",
    "1",
    "2",
    "3",
    "7",
    "-1",
    "255",
    "65536",
    "46341",
    "2147483647",
    "-2147483648",
    "0.5",
    "-1.5",
    "2.25",
    "1E300",
    "2147483648",
];
const VARIABLES: &[&str] = &["A", "B", "C", "I", "X#", "K%", "S$"];
const OPERATORS: &[&str] = &[
    "+", "-", "*", "/", "DIV", "MOD", "AND", "OR", "XOR", "<<", ">>", ">>>", "<<+", ">>+", "==",
    "!=", "<", "<=", ">", ">=", "&&", "||",
];
const FUNCTIONS: &[&str] = &["ABS", "SQR", "FLOOR", "SGN", "INT", "SIN"];

/// An array element with an index that is mostly in range.
fn element(r: &mut Rng, depth: u32) -> String {
    let index = match r.below(3) {
        0 => r.pick(&["0", "1", "I", "A", "4", "-1"]).to_string(),
        _ => format!("({}) AND 3", expr(r, depth + 1)),
    };
    match r.below(4) {
        0 => format!("M[{index}]"),
        1 => format!("N%[{index}]"),
        2 => format!("R#[{index},{}]", r.pick(&["0", "1", "I AND 1", "2"])),
        _ => format!("T$[{index}]"),
    }
}

/// Function DEFs [`DEFS_SOURCE`] and [`SLOT1`] have, of one argument.
const DEFS: &[&str] = &["SQ", "LC", "TS", "OU", "GS", "GR", "GC", "SH", "CA"];

/// Slot 1, which each program runs with EXEC first: COMMON DEFs whose
/// variables have the names of slot 0's globals, and a global of its own.
const SLOT1: &str = "\
VAR A=7,Q
A=A+1
END
COMMON DEF CA(V)
 VAR A=V*2,T,S$=\"C\"
 T=A+Q:Q=Q+1
 RETURN T+A
END
COMMON DEF CB V OUT W
 W=V*3+A
END
COMMON DEF CR(N)
 IF N<1 THEN RETURN A
 RETURN CR(N-1)+N
END
";

/// DEFs the programs call: functions, commands, OUT variables, a string
/// local, a recursive one, a host command inside, GOSUBs inside and returns
/// from them, and errors on strings.
const DEFS_SOURCE: &str = "\
DEF T3(X,Y,Z)
 RETURN X*100+Y*10-Z
END
DEF ONE()
 RETURN 1
END
DEF GS(V)
 GOSUB @IN
 RETURN V+1
@IN
 A=A+V
 RETURN
END
DEF GR(V)
 GOSUB @IN
 RETURN 0
@IN
 RETURN V*2
END
DEF GC(V)
 VAR R
 GT V OUT R
 RETURN R
END
DEF GT X OUT Y
 GOSUB @IN
 Y=X
 RETURN
@IN
 Y=X+1:RETURN
END

DEF SQ(V)
 RETURN V*V
END
DEF LC(V)
 VAR T=V+1,U$=\"L\"
 RETURN T*2
END
DEF TS(V)
 PUT V
 RETURN GET()+V
END
DEF OU(V)
 VAR P,Q
 TW V,V+1 OUT P,Q
 RETURN P-Q
END
DEF TW X,Y OUT P,Q
 P=X*2:Q=Y-1
END
DEF CM X
 A=A+X:B=X
END
DEF RC(N)
 IF N<1 THEN RETURN 1
 RETURN RC(N-1)*2+N
END
DEF RD(N)
 VAR T=N*2,U$=STR$(N)
 IF N<1 THEN RETURN T
 RETURN RD(N-1)+T+LEN(U$)
END
DEF SH(V)
 VAR A=V*2,B=A+1,K%=3
 A=A+B:B=A*K%
 RETURN A+B+C
END
";

fn leaf(r: &mut Rng, depth: u32) -> String {
    match r.below(17) {
        0..=3 => r.pick(VARIABLES).to_string(),
        4..=6 => r.pick(NUMBERS).to_string(),
        7 if depth < 3 => element(r, depth),
        8 if depth < 3 => format!("{}({})", r.pick(FUNCTIONS), expr(r, depth + 1)),
        9 if depth < 3 => format!("MAX({},{})", expr(r, depth + 1), leaf(r, depth + 1)),
        10 if depth < 3 => format!("{}({})", r.pick(DEFS), expr(r, depth + 1)),
        11 => "GET()".to_string(),
        12 if depth < 2 => match r.below(3) {
            0 => format!("RD({})", r.pick(&["0", "3", "40", "I", "A AND 63"])),
            1 => format!("CR({})", r.pick(&["0", "5", "I", "B AND 7"])),
            _ => format!("RC({})", r.pick(&["0", "1", "2", "I", "A AND 3"])),
        },
        13 if depth < 3 => format!("LEN(STR$({}))", expr(r, depth + 1)),
        14 => "INSTR(S$,\"B\")".to_string(),
        15 if depth < 3 => format!(
            "T3({},{},{})",
            leaf(r, depth + 1),
            leaf(r, depth + 1),
            leaf(r, depth + 1)
        ),
        _ => r.pick(VARIABLES).to_string(),
    }
}

fn expr(r: &mut Rng, depth: u32) -> String {
    if depth >= 3 || r.below(3) == 0 {
        return leaf(r, depth);
    }
    match r.below(8) {
        0 => format!("-({})", expr(r, depth + 1)),
        1 => format!("{}({})", r.pick(&["NOT ", "!"]), expr(r, depth + 1)),
        _ => format!(
            "({}) {} ({})",
            expr(r, depth + 1),
            r.pick(OPERATORS),
            expr(r, depth + 1)
        ),
    }
}

fn target(r: &mut Rng) -> String {
    match r.below(5) {
        0 => element(r, 2),
        _ => r.pick(&["A", "B", "C", "X#", "K%", "S$"]).to_string(),
    }
}

fn statement(r: &mut Rng) -> String {
    match r.below(22) {
        // A variable takes another kind of value: a string, an array, a
        // real or an integer, whatever its suffix.
        19 => format!(
            "{}={}",
            r.pick(&["A", "B", "C", "X#", "K%"]),
            r.pick(&["S$", "\"Z\"", "M", "1.5", "7", "-2147483648", "N%"])
        ),
        20 => format!("CB {} OUT {}", expr(r, 1), r.pick(&["A", "B", "C", "X#"])),
        0..=5 => format!("{}={}", target(r), expr(r, 0)),
        6 => format!(
            "{0}={0}{1}{2}",
            target(r),
            r.pick(&["+", "-", "*"]),
            leaf(r, 1)
        ),
        7 => format!(
            "IF {} THEN {} ELSE {}",
            expr(r, 0),
            format_args!("{}={}", target(r), expr(r, 1)),
            format_args!("{}={}", target(r), leaf(r, 1))
        ),
        8 => format!("WHILE ({}) AND (J<3):J=J+1:WEND", expr(r, 1)),
        9 => format!("?{}", expr(r, 0)),
        10 => format!("CM {}", expr(r, 1)),
        11 => format!(
            "TW {},{} OUT {},{}",
            leaf(r, 1),
            expr(r, 1),
            r.pick(&["A", "B"]),
            r.pick(&["C", "X#"])
        ),
        12 => format!("PUT {}", expr(r, 1)),
        13 => format!(
            "PAIR {} OUT {},{}",
            expr(r, 1),
            r.pick(&["A", "B"]),
            r.pick(&["C", "X#"])
        ),
        14 => format!("{}=GET()+{}", target(r), leaf(r, 1)),
        // Builtins of strings and arrays, functions and commands.
        15 => format!("S$=LEFT$(S$+STR$({}),3)", expr(r, 1)),
        16 => format!("A=POP(M):PUSH M,{}", expr(r, 1)),
        17 => "SORT N%".to_string(),
        18 => format!("?MID$(S$,{},2)", expr(r, 1)),
        _ => format!("?{}", expr(r, 0)),
    }
}

fn setup(r: &mut Rng) -> String {
    let mut s = String::from("VAR A,B,C,X#,K%,S$,J\nDIM M[4],N%[4],R#[4,2],T$[4]\nEXEC 1\n");
    for v in ["A", "B", "C", "X#", "K%"] {
        s += &format!("{v}={}:", r.pick(NUMBERS));
    }
    // Sometimes a string where a number would be.
    if r.below(6) == 0 {
        s += &format!("{}=\"Q\":", r.pick(&["A", "B", "C"]));
    }
    s += "S$=\"AB\":T$[1]=\"Z\":M[1]=3:N%[2]=-5:R#[1,1]=0.25\n";
    s
}

fn program(r: &mut Rng) -> String {
    let mut s = setup(r);
    let body: Vec<String> = (0..1 + r.below(3)).map(|_| statement(r)).collect();
    let body = body.join("\n");
    match r.below(8) {
        0 => s += &format!("FOR I=0 TO 2\n{body}\nNEXT\n"),
        // Calls that STEP makes, which the FOR's Nop leads.
        5 => s += &format!("FOR I=0 TO 3 STEP ONE()\n{body}\nNEXT\n"),
        6 => s += &format!("FOR I=0 TO 9 STEP GET()\n{body}\nNEXT\n"),
        1 => s += &format!("DEF F I\n{body}\nEND\nFOR Q=0 TO 2:F Q:NEXT\n"),
        2 => s += &format!("DEF F(I)\n{body}\nRETURN A+I\nEND\nFOR Q=0 TO 2:B=F(Q)+F(Q+1):NEXT\n"),
        3 => s += &format!("DEF F I OUT Z\n{body}\nZ=I\nEND\nFOR Q=0 TO 2:F Q OUT C:NEXT\n"),
        4 => s += &format!("DEF F I\n{body}\nIF I>0 THEN F I-1\nEND\nF 2\n"),
        _ => s += &format!("I=1\n{body}\n"),
    }
    s + "?A,B,C,X#,K%,S$,M[0],M[1],N%[0],R#[0,0],T$[0]\nEND\n" + DEFS_SOURCE
}

thread_local! {
    /// How many times native code ran, with the `jit` feature.
    static NATIVE: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

/// What can be seen of a run in slices of `budget` instructions.
fn run(source: &str, budget: u32, fused: bool) -> Option<Vec<String>> {
    let host = CmdHost::default();
    let mut program = sb_core::compile(source, &host).ok()?;
    if !fused {
        program.fast.clear();
    }
    let mut host = CmdHost::default();
    host.slots = vec![String::new(), SLOT1.to_string()];
    let mut vm = Vm::new(program);
    // Slot 1's program is fused when EXEC compiles it: a run that reports
    // its work goes one instruction at a time everywhere.
    vm.report_work(!fused);
    let mut seen = Vec::new();
    for _ in 0..100_000 {
        let result = vm.run(&mut host, budget);
        let shown = match &result {
            Ok(status) => format!("{status:?}"),
            Err(e) => format!("{e} {:?} {} {} {:?}", e.kind, e.line, e.col, e.detail),
        };
        seen.push(format!(
            "{shown} {} {} {}",
            vm.executed(),
            vm.line(),
            vm.col()
        ));
        match result {
            Ok(Status::Ended) | Err(_) => {
                NATIVE.with(|n| n.set(n.get() + common::native_runs(&vm)));
                seen.push(format!("{} {}", host.text.output, host.count));
                return Some(seen);
            }
            Ok(_) => {}
        }
    }
    panic!("{source}: did not end");
}

#[test]
fn random_programs_run_the_same_fused_or_not() {
    let mut r = Rng(0x5EED_1234_ABCD_0001);
    let (mut compiled, mut chains, mut calls, mut hosts, mut builtins) = (0, 0, 0, 0, 0);
    // Chains that make calls of DEFs in their steps.
    let mut call_chains = 0;
    // Programs that run to their end, past the EXEC and every call.
    let mut ended = 0;
    // Miri runs a program thousands of times slower, so there it runs a few
    // unless SB_RANDOM_PROGRAMS says how many. The counts below hold for
    // all of them.
    const ALL: usize = 1500;
    let count = (std::env::var("SB_RANDOM_PROGRAMS").ok())
        .and_then(|n| n.parse().ok())
        .unwrap_or(if cfg!(miri) { 5 } else { ALL });
    for _ in 0..count {
        let source = program(&mut r);
        let host = CmdHost::default();
        let Ok(p) = sb_core::compile(&source, &host) else {
            continue;
        };
        compiled += 1;
        common::check_needs(&p, &source);
        chains += p
            .fast
            .iter()
            .filter(|f| matches!(f.op, FastOp::Chain { .. }))
            .count();
        calls += (p.fast.iter())
            .filter(|f| matches!(f.op, FastOp::CallDef { .. }))
            .count();
        call_chains += (p.fast.iter())
            .filter(|f| matches!(f.op, FastOp::CallChain { .. }))
            .count();
        hosts += (p.fast.iter())
            .filter(|f| matches!(f.op, FastOp::Host { .. }))
            .count();
        builtins += (p.fast.iter())
            .filter(|f| matches!(f.op, FastOp::Call { .. }))
            .count();
        for budget in [1, 2, 5, 13, 1_000_000] {
            let fused = run(&source, budget, true);
            assert_eq!(
                fused,
                run(&source, budget, false),
                "{source}\nin slices of {budget}"
            );
            let end = fused.as_ref().map(|seen| &seen[seen.len() - 2]);
            ended += usize::from(budget == 1 && end.is_some_and(|e| e.starts_with("Ended")));
        }
    }
    // The generator must make programs that compile, that chains take, and
    // that call DEFs and the host.
    // With the `jit` feature, native code ran in the fused runs.
    let native = NATIVE.with(|n| n.get());
    assert!(
        !cfg!(feature = "jit") || native > 0,
        "native code never ran"
    );
    if count < ALL {
        println!("{count} programs, {compiled} compiled, {ended} ended, native {native}");
        return;
    }
    assert!(compiled > 1000, "{compiled} compiled");
    assert!(chains > 8000, "{chains} chains");
    assert!(calls > 4000, "{calls} DEF calls");
    assert!(call_chains > 500, "{call_chains} chains that call DEFs");
    assert!(hosts > 3000, "{hosts} host calls");
    assert!(builtins > 1000, "{builtins} builtin calls");
    assert!(ended > 500, "{ended} ended");
}
