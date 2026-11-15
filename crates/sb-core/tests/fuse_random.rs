//! Random programs from a fixed seed, run fused and one instruction at a
//! time: what they print, their errors, and where each time slice ends must
//! be the same. The statements are the kind the fuser's chains take, with
//! values of every type in the variables, so that each step of a chain meets
//! what it cannot do as well as what it can.

use sb_core::fuse::FastOp;
use sb_core::host::TextHost;
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

fn leaf(r: &mut Rng, depth: u32) -> String {
    match r.below(10) {
        0..=3 => r.pick(VARIABLES).to_string(),
        4..=6 => r.pick(NUMBERS).to_string(),
        7 if depth < 3 => element(r, depth),
        8 if depth < 3 => format!("{}({})", r.pick(FUNCTIONS), expr(r, depth + 1)),
        9 if depth < 3 => format!("MAX({},{})", expr(r, depth + 1), leaf(r, depth + 1)),
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
    match r.below(10) {
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
        _ => format!("?{}", expr(r, 0)),
    }
}

fn setup(r: &mut Rng) -> String {
    let mut s = String::from("VAR A,B,C,X#,K%,S$,J\nDIM M[4],N%[4],R#[4,2],T$[4]\n");
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
    match r.below(3) {
        0 => s += &format!("FOR I=0 TO 2\n{body}\nNEXT\n"),
        1 => s += &format!("DEF F I\n{body}\nEND\nFOR Q=0 TO 2:F Q:NEXT\n"),
        _ => s += &format!("I=1\n{body}\n"),
    }
    s + "?A,B,C,X#,K%,S$,M[0],M[1],N%[0],R#[0,0],T$[0]\n"
}

/// What can be seen of a run in slices of `budget` instructions.
fn run(source: &str, budget: u32, fused: bool) -> Option<Vec<String>> {
    let host = TextHost::default();
    let mut program = sb_core::compile(source, &host).ok()?;
    if !fused {
        program.fast.clear();
    }
    let mut host = TextHost::default();
    let mut vm = Vm::new(program);
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
                seen.push(host.output);
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
    let (mut compiled, mut chains) = (0, 0);
    for _ in 0..1500 {
        let source = program(&mut r);
        let host = TextHost::default();
        let Ok(p) = sb_core::compile(&source, &host) else {
            continue;
        };
        compiled += 1;
        chains += p
            .fast
            .iter()
            .filter(|f| matches!(f.op, FastOp::Chain { .. }))
            .count();
        for budget in [1, 2, 5, 13, 1_000_000] {
            assert_eq!(
                run(&source, budget, true),
                run(&source, budget, false),
                "{source}\nin slices of {budget}"
            );
        }
    }
    // The generator must make programs that compile and that chains take.
    assert!(compiled > 1000, "{compiled} compiled");
    assert!(chains > 8000, "{chains} chains");
}
