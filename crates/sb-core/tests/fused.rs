//! Fused instructions must not be seen: a program runs the same with them as
//! one instruction at a time, up to its errors and where each time slice ends.

use sb_core::bytecode::{Compiled, Op};
use sb_core::fuse::FastOp;
use sb_core::host::TextHost;
use sb_core::vm::{Status, Step, Vm};
use sb_core::watch::Watch;
use std::rc::Rc;

fn compiled(source: &str, fused: bool) -> Compiled {
    let host = TextHost::default();
    let mut program = sb_core::compile(source, &host).unwrap_or_else(|e| panic!("{source}: {e}"));
    if !fused {
        program.fast.clear();
    }
    program
}

/// What can be seen of a run in slices of `budget` instructions: where each
/// slice ended, how it ended, and the output.
fn run(source: &str, budget: u32, fused: bool) -> (Vec<String>, String) {
    let mut host = TextHost::default();
    let mut vm = Vm::new(compiled(source, fused));
    let mut seen = Vec::new();
    for _ in 0..200_000 {
        let result = vm.run(&mut host, budget);
        // An error holds an address, so not all of it can be compared.
        let shown = match &result {
            Ok(status) => format!("{status:?}"),
            Err(e) => format!("{e} {:?} {} {} {:?}", e.kind, e.line, e.col, e.detail),
        };
        let (executed, line, col) = (vm.executed(), vm.line(), vm.col());
        seen.push(format!("{shown} executed {executed} line {line} col {col}"));
        match result {
            Ok(Status::Ended) | Err(_) => return (seen, host.output),
            Ok(_) => {}
        }
    }
    panic!("{source}: did not end");
}

const PROGRAMS: &[&str] = &[
    // FOR in integers and reals, up and down, with TO and STEP that change.
    "FOR I=1 TO 5:?I:NEXT:?I",
    "FOR I=5 TO 1 STEP -2:?I:NEXT:?I",
    "FOR I=0 TO 1 STEP 0.25:?I:NEXT:?I",
    "FOR I#=1 TO 3:?I#:NEXT:?I#",
    "N=3:S=1:FOR I=0 TO N STEP S:N=N-1:S=S+1:?I:NEXT",
    "FOR I=1 TO 4 STEP I+1:?I:NEXT",
    "FOR I=1 TO I+1:?I:IF I>4 THEN BREAK\nNEXT",
    "FOR I%=2147483646 TO 2147483647:?I%:IF I%<0 THEN BREAK\nNEXT:?I%",
    "DEF F N\nFOR I=0 TO N:?I:NEXT\nEND\nF 3",
    "Z=0:FOR I=0 TO 1/Z\nNEXT",
    "S$=\"A\":FOR I=0 TO S$:NEXT",
    "S$=\"A\":FOR I=0 TO 3 STEP S$:NEXT",
    "T=1:S$=\"A\":FOR I=0 TO T\n?I\nT=S$\nNEXT\n?\"AFTER\"",
    "T=1:S$=\"A\":FOR I=0 TO 1 STEP T\n?I\nT=S$\nNEXT\n?\"AFTER\"",
    "T=2\nS$=\"A\":FOR I=0 TO 3:T=T-1:NEXT\n?I:FOR J=0 TO T:J=S$\nNEXT",
    // Arithmetic and comparisons of every kind, where they fuse and not.
    "A=1:B=2:C=A+B:?C:C=A-B:?C:C=A*B:?C:C=A/B:?C:C=A DIV B:?C:C=A MOD B:?C",
    "A=7:B=2:?A<<B:?A>>B:?A AND B:?A OR B:?A XOR B:?(A*2)+(B*3):?(A*2)-(B*3)",
    "A#=1.5:B%=2:?A#+B%:?B%+A#:?A#*B%:?A#/B%:?A# DIV B%:?A# MOD B%:?A#<<B%:?A# AND B%",
    "A%=2147483647:A%=A%+1:?A%:B%=-2147483648:B%=B%-1:?B%:C%=65536:C%=C%*C%:?C%",
    "A=0/1:?A==0:?A!=0:?A<1:?A<=0:?A>0:?A>=0:IF A==0 THEN ?\"Z\"\nIF A THEN ?\"NZ\"",
    "N#=1E308*10:M#=N#-N#:?M#==M#:?M#!=M#:?M#<1:?M#>1:IF M# THEN ?\"T\" ELSE ?\"F\"",
    "A$=\"X\":B$=\"Y\":?A$+B$:C$=A$+B$:?C$:?A$<B$:?A$==B$:?A$*3:D=2:?A$*D:IF A$<B$ THEN ?1",
    "A=1:IF A==1 && A<2 THEN ?\"AND\"\nIF A==2 || A<2 THEN ?\"OR\"\nWHILE A<4:A=A+1:WEND:?A",
    "X=(1+2)*(3+4):?X:Y=2:Z=3-(Y*2):?Z:Z=(Y*2)-3:?Z:W=Y-(Z*2)-(Y+Z):?W",
    "REPEAT:A=A+1:UNTIL A>=3:?A",
    // Errors in each shape, not at the start of a line.
    "A=1:B=0:C=A/B",
    "A=1:B=0\n?1:C=A/B:?2",
    "A=1:B=0\n?1:C=A DIV B:?2",
    "A=1:B=0\n?1:C=(A+1) MOD B:?2",
    "A=1:B=0\n?1:IF A/B THEN ?2",
    "A=1:B=0:D=0:C=D+(A/B)",
    "A=1:B$=\"X\"\n?1:C=A+B$:?2",
    "A=1:B$=\"X\"\n?1:C=B$+A:?2",
    "A=1:B$=\"X\"\n?1:C=(A+1)+B$:?2",
    "A=1:B$=\"X\"\n?1:C=B$+(A+1):?2",
    "A=1:B$=\"X\"\n?1:C=A-B$:?2",
    "A=1:B$=\"X\"\n?1:IF A<B$ THEN ?2",
    "B$=\"X\"\n?1:IF B$ THEN ?2",
    "B$=\"X\"\n?1:WHILE B$:WEND",
    "A#=1E300\n?1:C=A# AND 1:?2",
    "A#=1E300\n?1:C=A#<<1:?2",
    "VAR A\nDEF F OUT X\nEND\nF OUT A:?1:B=A+1",
    // Arrays: kinds, shapes, ranges, conversions.
    "DIM A[4]:A[1]=3:A[2]=A[1]+1:?A[2]:I=2:J=A[I]:?J:A[I]=J+1:?A[2]:?A[I]+A[1]",
    "DIM A%[4]:A%[1]=3.75:?A%[1]:I#=1.75:?A%[I#]:A%[I#]=9:?A%[1]:A%[2]=-1.5:?A%[2]",
    "DIM A#[4]:A#[1]=3:?A#[1]:I=1:?A#[I]",
    "DIM A[2,3]:A[1,2]=5:?A[1,2]:I=1:J=2:?A[I,J]:A[I,J]=6:?A[5]:?A[I*1,J*1]:A[I*1,J*1]=7:?A[5]",
    "DIM A$[3]:A$[1]=\"X\":?A$[1]:I=1:B$=A$[I]:?B$:A$[I]=\"Y\":?A$[1]:?A$[I]+\"Z\"",
    "S$=\"ABC\":?S$[1]:I=2:?S$[I]:S$[I]=\"XY\":?S$:C$=S$[0]:?C$",
    "DIM A[2]\n?1:?A[2]",
    "DIM A[2]\n?1:I=-1:?A[I]",
    "DIM A[2]\n?1:I=2:B=A[I]:?2",
    "DIM A[2]\n?1:I=2:A[I]=1:?2",
    "DIM A[2]\n?1:I=1E300:A[I]=1:?2",
    "DIM A[2]\n?1:I=0/1:J=1:A[I,J]=1:?2",
    "DIM A[2,2]\n?1:I=2:J=0:?A[I,J]:?2",
    "DIM A[2,2]\n?1:I=0:J=2:A[I,J]=1:?2",
    "DIM A[2,2]\n?1:I=3:?A[I]:I=4:?A[I]",
    "DIM A[2,2,2]\n?1:I=0:J=0:?A[I,J]",
    "DIM A%[2]\n?1:X=1E300:A%[0]=X:?2",
    "DIM A%[2]\n?1:X$=\"S\":A%[0]=X$:?2",
    "DIM A$[2]\n?1:A$[0]=1:?2",
    "DIM A[2]\n?1:I$=\"X\":?A[I$]",
    "A=1\n?1:I=0:?A[I]",
    "A=1\n?1:I=0:A[I]=1",
    "VAR A[2],B[2]\nB=A:B[0]=1:?A[0]:C=A[0]+B[0]:?C",
    // DEFs, locals, GOSUB, READ, the values the stack holds meanwhile.
    "DEF ADD(A,B)\nVAR C=A+B\nRETURN C\nEND\n?ADD(1,2)+ADD(3,4):?ADD(1.5,2)*2",
    "DEF TWO A,B OUT X,Y\nX=A+B:Y=A*B\nEND\nTWO 2,3 OUT P,Q:?P:?Q",
    "DEF F(A)\nIF A<=1 THEN RETURN 1\nRETURN A*F(A-1)\nEND\n?F(10)",
    "DEF F(A)\nRETURN A+B$\nEND\nVAR B$=\"X\":?F(1)",
    "DEF F A$\nA$=A$+\"Y\":?A$\nEND\nS$=\"X\":F S$:?S$",
    "GOSUB @S:?A:END\n@S:A=A+1:IF A<3 THEN GOSUB @S\nRETURN",
    "READ A,B$,C:?A:?B$:?C:DATA 1,\"S\",2.5",
    "A=1:B=2:SWAP A,B:?A:?B:INC A:DEC B:?A:?B:INC A,2:?A",
    "S$=\"A\":INC S$,\"B\":?S$:T$=S$:INC T$,\"C\":?S$",
    "?ABS(-1)+1:?MAX(1,2)*2:?FLOOR(1.5)+CEIL(1.5):?LEN(\"AB\")+1:?SIN(0)",
    "DIM A[3]:?LEN(A)+1:A[1]=MAX(1,2):?A[1]:PUSH A,5:?A[3]:?POP(A)",
    "X=-1:?-X:?!X:?NOT X:Y=-X+1:?Y",
    "ON 1 GOTO @A,@B\n@A:?1\n@B:?2\nFOR I=0 TO 2:ON I GOSUB @C,@D:NEXT:END\n@C:?3:RETURN\n@D:?4:RETURN",
    "A=1:GOTO @L\nA=2\n@L:?A:B=A+1:?B",
    // ON GOTO and ON GOSUB: in range, out of range, from a variable, an
    // expression and an array, in a DEF, and each way the selector fails.
    ON_GOTO,
    ON_GOSUB,
    "FOR I=-1 TO 3:ON I+1 GOSUB @A,@B:NEXT:END\n@A:?\"A\":RETURN\n@B:?\"B\":RETURN",
    ON_ARRAY,
    ON_REAL,
    "DEF F N\nVAR I\nFOR I=0 TO N:ON I GOSUB @A,@B:NEXT\nEND\nF 3:END\n@A:?\"A\":RETURN\n@B:?\"B\":RETURN",
    ON_IN_DEF,
    "X$=\"A\"\n?1:ON X$ GOTO @A\n@A:?2",
    "X=1E300\n?1:ON X GOTO @A\n@A:?2",
    "DIM K%[2]\n?1:I=2:ON K%[I] GOTO @A\n@A:?2",
    "DIM K$[2]\n?1:I=0:ON K$[I] GOTO @A\n@A:?2",
    "?1:RETURN",
    "GOSUB @S:?\"OK\":END\n@S:RESTORE \"PUSH\":RETURN",
    "DEF F\nGOSUB @S\n?\"F\"\nEND\nF:?\"OUT\":END\n@S:?\"S\":RETURN",
    // CASE on numbers and strings, WHENs sharing a body, OTHERWISE.
    CASE_NUMBERS,
    "FOR A#=0 TO 2 STEP 0.5\nCASE A#\nWHEN 1:?\"ONE\"\nWHEN 1.5:?\"HALF\"\nENDCASE\nNEXT",
    CASE_STRINGS,
    "S$=\"B\"\n?1:CASE S$\nWHEN 1:?2\nENDCASE",
    "A=1\n?1:CASE A\nWHEN \"A\":?2\nENDCASE",
];

const ON_GOTO: &str = "\
FOR I=-2 TO 3
 ON I GOTO @A,@B,@C
 ?\"NONE\";I:GOTO @E
 @A:?\"A\":GOTO @E
 @B:?\"B\":GOTO @E
 @C:?\"C\"
 @E
NEXT
";
const ON_GOSUB: &str =
    "FOR I=-1 TO 3:ON I GOSUB @A,@B:?\"N\";I:NEXT:END\n@A:?\"A\":RETURN\n@B:?\"B\":RETURN";
const ON_ARRAY: &str = "DIM K%[3]:K%[1]=1:K%[2]=7:FOR I=0 TO 2:ON K%[I] GOTO @A,@B\n?\"N\":GOTO @E\n@A:?\"A\":GOTO @E\n@B:?\"B\"\n@E:NEXT";
const ON_REAL: &str = "X#=1.75:ON X# GOTO @A,@B\n@A:?\"A\"\n@B:?\"B\"";
const ON_IN_DEF: &str = "DEF F N\nON N GOTO @A\n?\"N\":RETURN\n@A:?\"A\"\nEND\nF 0:F 1";
const CASE_NUMBERS: &str = "FOR A=0 TO 4\nCASE A\nWHEN 0:WHEN 1:?\"A\"\nWHEN 2:?\"B\"\nWHEN 3.0:?\"C\"\nOTHERWISE:?\"X\"\nENDCASE\nNEXT";
const CASE_STRINGS: &str = "S$=\"B\"\nCASE S$\nWHEN \"A\":?1\nWHEN \"B\":?2\nENDCASE\nCASE S$+\"C\"\nWHEN \"BC\":?3\nENDCASE";

/// What ON and CASE print, fused.
#[test]
fn on_and_case_print_what_they_should() {
    for (source, expected) in [
        (ON_GOTO, "NONE-2\nNONE-1\nA\nB\nC\nNONE3\n"),
        (ON_GOSUB, "N-1\nA\nN0\nB\nN1\nN2\nN3\n"),
        (ON_ARRAY, "A\nB\nN\n"),
        (ON_REAL, "B\n"),
        (ON_IN_DEF, "A\nN\n"),
        (CASE_NUMBERS, "A\nA\nB\nC\nX\n"),
        (CASE_STRINGS, "2\n3\n"),
    ] {
        let mut host = TextHost::default();
        let mut vm = Vm::new(compiled(source, true));
        assert_eq!(
            vm.run(&mut host, 1_000_000).unwrap(),
            Status::Ended,
            "{source}"
        );
        assert_eq!(host.output, expected, "{source}");
    }
}

/// GOSUBs fill the stack as without fusing, which takes longer slices than
/// [`programs_run_the_same_fused_or_not`] uses.
#[test]
fn gosubs_overflow_as_without_fusing() {
    for program in [
        "?1:GOSUB @S\n@S:GOSUB @S",
        "?1:I=0:ON I GOSUB @S\n@S:ON I GOSUB @S",
    ] {
        for budget in [5, 7, 1_000_000] {
            let fused = run(program, budget, true);
            assert!(
                fused.0.last().unwrap().contains("StackOverflow"),
                "{fused:?}"
            );
            assert_eq!(fused, run(program, budget, false), "{program} in {budget}");
        }
    }
}

/// None of these leaves anything to `step` but DIM, the end of the program
/// and the FOR test that only stands in a larger entry.
#[test]
fn on_and_case_are_fused() {
    for source in [
        "FOR I=0 TO 3:ON I GOTO @A,@B\nA=1\n@A:B=1\n@B:NEXT",
        "FOR I=0 TO 9:ON I GOSUB @S:NEXT:END\n@S:RETURN",
        "FOR A=0 TO 4\nCASE A\nWHEN 1:B=1\nWHEN 2:B=2\nENDCASE\nNEXT",
    ] {
        let program = compiled(source, true);
        let slow: Vec<_> = program
            .fast
            .iter()
            .zip(&program.code)
            .filter(|(f, op)| {
                f.op == FastOp::Slow && !matches!(op, Op::End | Op::ForTest | Op::NewArray { .. })
            })
            .map(|(_, op)| op)
            .collect();
        assert!(slow.is_empty(), "{source}: {slow:?}");
    }
}

#[test]
fn programs_run_the_same_fused_or_not() {
    for program in PROGRAMS {
        for budget in [1, 2, 3, 5, 7, 13, 100, 1_000_000] {
            let fused = run(program, budget, true);
            let plain = run(program, budget, false);
            assert_eq!(fused, plain, "{program} in slices of {budget}");
        }
    }
}

/// Calls keep their arguments on the stack while they run. Whatever comes of a
/// call, the stack and the error must be as if they had been taken off first.
#[test]
fn calls_leave_the_stack_as_it_should_be() {
    for (source, expected) in [
        ("A=1+ABS(-2)*MAX(3,4):?A", "9\n"),
        (
            "DEF F(A,B$)\nVAR C$=B$*A\nRETURN C$\nEND\n?F(2,\"X\")+F(1,\"Y\")",
            "XXY\n",
        ),
        ("DEF F(A)\nRETURN A+1\nEND\n?1+F(F(1))*2:?F(2)", "7\n3\n"),
        ("DEF G *\n?DEFARGC()\n?DEFARG(1)\nEND\nG 1,2,3", "3\n2\n"),
        (
            "DEF H * OUT *\nFOR I=0 TO DEFOUTC()-1:DEFOUT I,I+DEFARG(0):NEXT\nEND\nH 5 OUT A,B:?A:?B",
            "5\n6\n",
        ),
        ("DEF K OUT A,B\nA=1\nEND\nK OUT P,Q:?P:?TYPEOF(Q)", "1\n0\n"),
        (
            "DEF M A,B$ OUT C\nC=A+LEN(B$)\nEND\nM 1,\"AB\" OUT D:?D",
            "3\n",
        ),
        (
            "DEF R(N)\nVAR L$=\"S\":L$=L$*N\nRETURN LEN(L$)\nEND\n?R(2)+R(3)",
            "5\n",
        ),
    ] {
        let mut host = TextHost::default();
        let mut vm = Vm::new(compiled(source, true));
        assert_eq!(
            vm.run(&mut host, 1_000_000).unwrap(),
            Status::Ended,
            "{source}"
        );
        assert_eq!(host.output, expected, "{source}");
    }
}

#[test]
fn failed_calls_name_what_they_always_named() {
    use sb_core::error::ErrorKind as E;
    for (source, kind, line) in [
        ("A=1\n?ABS()", E::WrongNumberOfArguments, 2),
        ("A=1\n?ABS(\"X\")", E::TypeMismatch, 2),
        ("A=1\nB=SQR(-1)", E::OutOfRange, 2),
        (
            "DEF F(A)\nRETURN A\nEND\nC=1\nB=F(1,2)",
            E::WrongNumberOfArguments,
            5,
        ),
        ("DEF F A\nEND\nC=1\nB=F(1)", E::IllegalFunctionCall, 4),
        (
            "DEF F(A)\nRETURN A\nEND\nC=1\nF 1 OUT P,Q",
            E::IllegalFunctionCall,
            5,
        ),
        (
            "DEF F OUT A,B\nEND\nC=1\nB=F()",
            E::WrongNumberOfOutArguments,
            4,
        ),
        (
            "DEF F(A)\nRETURN F(A+1)\nEND\nC=1\nB=F(1)",
            E::StackOverflow,
            2,
        ),
    ] {
        let mut host = TextHost::default();
        let mut vm = Vm::new(compiled(source, true));
        let e = vm.run(&mut host, 100_000_000).unwrap_err();
        assert_eq!((e.kind, e.line), (kind, line), "{source}");
    }
}

/// A call that asks for more memory than the last count allowed runs again after
/// counting, and finds its arguments still there.
#[test]
fn a_call_runs_again_after_counting_memory() {
    let source = "FOR I=0 TO 40:A%=ARRAY%(4000000):A%[0]=I:NEXT:?LEN(A%):?A%[0]";
    let mut host = TextHost::default();
    let mut vm = Vm::new(compiled(source, true));
    loop {
        match vm.run(&mut host, 1_000_000).unwrap() {
            Status::Ended => break,
            _ => continue,
        }
    }
    assert_eq!(host.output, "4000000\n40\n");
}

/// Variables the fused code reads are the same ones the rest of the program and
/// the debugger see, constants being kept out of their view.
#[test]
fn constants_are_not_variables() {
    let mut host = TextHost::default();
    let mut vm = Vm::new(compiled("A=5:B=A+7:C#=B*2.5", true));
    vm.run(&mut host, 1_000_000).unwrap();
    let mut names: Vec<String> = vm.globals().map(|(name, _)| name).collect();
    names.sort();
    assert_eq!(names, ["A", "B", "C#"]);
    assert!(matches!(vm.global("C#"), Some(sb_core::value::Value::Real(x)) if *x == 30.0));
    assert!(!vm.program().constants.is_empty());
}

#[test]
fn most_loops_are_fused() {
    let program = compiled("FOR I=1 TO 9:A=A+I*2:NEXT", true);
    assert_eq!(program.fast.len(), program.code.len());
    assert!(program.fast.iter().any(|f| f.len > 3), "{:?}", program.fast);
}

/// Budgets that end in the middle of what would be fused, for each thing that
/// can stop a program.
const BUDGETS: [u32; 5] = [1, 4, 9, 23, 41];

const LOOP: &str = "\
A=0:DIM R%[8],K%[8]
FOR I=1 TO 6
 A=A+I*2
 B=A-1:C=B+1
 IF A>9 THEN D=1
 ON I MOD 3 GOSUB @S,@T
 CASE I
 WHEN 2:E=1
 WHEN 4:E=2
 ENDCASE
 K%[I]=I+1:R%[I]=R%[I]+1
NEXT
?A
END
@S:F=F+1:RETURN
@T
RETURN
";

/// Each line a run stops at, given that it first ran `budget` instructions
/// fused, and how `stop` asks for the stops.
fn stops(fused: bool, budget: u32, stop: impl Fn(&mut Vm)) -> Vec<(String, u64)> {
    let mut host = TextHost::default();
    let mut vm = Vm::new(compiled(LOOP, fused)).in_slot(0);
    vm.run(&mut host, budget).unwrap();
    let mut seen = Vec::new();
    for _ in 0..1000 {
        stop(&mut vm);
        let status = vm.run(&mut host, 1_000_000).unwrap();
        seen.push((format!("{status:?} line {}", vm.line()), vm.executed()));
        if status == Status::Ended {
            return seen;
        }
    }
    panic!("did not end");
}

#[test]
fn debugger_steps_stop_as_without_fusing() {
    for step in [Step::Into, Step::Over, Step::Out] {
        for budget in BUDGETS {
            let ask = |vm: &mut Vm| vm.step_by(step);
            assert_eq!(stops(true, budget, ask), stops(false, budget, ask));
        }
    }
}

#[test]
fn trace_stops_as_without_fusing() {
    for budget in BUDGETS {
        let ask = |vm: &mut Vm| vm.trace();
        let fused = stops(true, budget, ask);
        assert!(fused.len() > 6, "{fused:?}");
        assert_eq!(fused, stops(false, budget, ask));
    }
}

#[test]
fn breakpoints_stop_as_without_fusing() {
    for budget in BUDGETS {
        let set = |vm: &mut Vm| vm.set_breakpoints(0, &[(4, None)]);
        let fused = stops(true, budget, set);
        // The line in the loop, once each time round from wherever it began.
        assert!(fused.len() >= 5, "{fused:?}");
        assert_eq!(fused, stops(false, budget, set));
    }
}

#[test]
fn conditional_breakpoints_and_watches_stop_as_without_fusing() {
    let condition = Rc::new(Watch::new("A>10").unwrap());
    let watched = Rc::new(Watch::new("B").unwrap());
    for budget in BUDGETS {
        let set = |vm: &mut Vm| vm.set_breakpoints(0, &[(4, Some(condition.clone()))]);
        assert_eq!(stops(true, budget, set), stops(false, budget, set));
        let run = |fused: bool| {
            let mut host = TextHost::default();
            let mut vm = Vm::new(compiled(LOOP, fused)).in_slot(0);
            vm.run(&mut host, budget).unwrap();
            vm.set_changes(std::slice::from_ref(&watched));
            let mut seen = Vec::new();
            loop {
                let status = vm.run(&mut host, 1_000_000).unwrap();
                seen.push((format!("{status:?}"), vm.line(), vm.executed()));
                if status == Status::Ended {
                    return seen;
                }
            }
        };
        let fused = run(true);
        assert!(fused.len() >= 5, "{fused:?}");
        assert_eq!(fused, run(false));
    }
}

/// The + button comes between slices: wherever a slice ends in fused code, it
/// finds the program where it would have been.
#[test]
fn on_break_jumps_as_without_fusing() {
    let source = "ON BREAK GOTO @B\nA=0\nWHILE 1\n A=A+1:C=A*2\nWEND\n@B\n?A\n?\"OUT\"";
    for budget in [1, 2, 3, 5, 9, 17, 40, 100, 333] {
        let run = |fused: bool| {
            let mut host = TextHost::default();
            let mut vm = Vm::new(compiled(source, fused));
            let first = vm.run(&mut host, budget).unwrap();
            let taken = vm.take_break(&mut host);
            let line = vm.line();
            let status = vm.run(&mut host, 10_000).unwrap();
            (first, taken, line, status, vm.executed(), host.output)
        };
        assert_eq!(run(true), run(false), "after {budget}");
    }
}

/// Without ON BREAK the host stops the program between slices, and the place
/// it stopped at goes on as if nothing had happened.
#[test]
fn a_stopped_slice_goes_on_as_without_fusing() {
    let source = "A=0\nFOR I=0 TO 50\n A=A+I:B=A*2\n STOP\nNEXT\n?A";
    let run = |fused: bool| {
        let mut host = TextHost::default();
        let mut vm = Vm::new(compiled(source, fused)).in_slot(0);
        let mut seen = Vec::new();
        loop {
            let status = vm.run(&mut host, 1_000_000).unwrap();
            seen.push((status, vm.line(), vm.executed()));
            if status == Status::Ended {
                return (seen, host.output);
            }
        }
    };
    assert_eq!(run(true), run(false));
}

/// A statement that fails names its own line and statement, whichever
/// instruction of a fused group it was.
#[test]
fn errors_name_their_statement() {
    let at = |source: &str| {
        let mut host = TextHost::default();
        let mut vm = Vm::new(compiled(source, true));
        let e = vm.run(&mut host, 1_000_000).unwrap_err();
        (e.kind.code(), e.line, e.col)
    };
    let plain = |source: &str| {
        let mut host = TextHost::default();
        let mut vm = Vm::new(compiled(source, false));
        let e = vm.run(&mut host, 1_000_000).unwrap_err();
        (e.kind.code(), e.line, e.col)
    };
    for source in [
        "A=1:B=0\nD=1:C=A/B:E=2",
        "DIM X[2]\nI=5\nD=1:E=X[I]:F=2",
        "DIM X[2]\nI=5\nD=1:X[I]=1:F=2",
        "S$=\"A\"\nFOR I=0 TO 1\nA=1:A=A+S$\nNEXT",
        "S$=\"A\"\nA=1\nFOR I=0 TO S$\nA=2\nNEXT",
        "S$=\"A\"\nT=1\nFOR I=0 TO T:T=S$\nNEXT",
        "DEF F(A)\nB=1:RETURN A/0\nEND\nC=2:D=F(1)",
    ] {
        let fused = at(source);
        assert_eq!(fused, plain(source), "{source}");
        assert!(fused.1 > 0, "{source}: {fused:?}");
    }
}
