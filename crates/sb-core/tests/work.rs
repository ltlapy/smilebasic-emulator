//! What a VM counts of its work for hosts that time it as a console would.

use sb_core::host::TextHost;
use sb_core::vm::{Status, Vm, Work};

/// Runs `source` to its end, reporting the work or not, and gives the
/// counts and the output.
fn run(source: &str, report: bool) -> (Work, String) {
    let mut host = TextHost::default();
    let program = sb_core::compile(source, &host).unwrap_or_else(|e| panic!("{source}: {e}"));
    let mut vm = Vm::new(program);
    vm.report_work(report);
    loop {
        match vm.run(&mut host, 1000) {
            Ok(Status::Ended) => return (vm.work(), host.output),
            Ok(_) => {}
            Err(e) => panic!("{source}: {e}"),
        }
    }
}

const PROGRAM: &str = "DIM A%[4]\n\
    DEF F(X)\n RETURN X+1\nEND\n\
    FOR I=0 TO 3\n A%[I]=F(I)+LEN(\"AB\")\nNEXT\n\
    S$=\"A\"+\"B\":INC S$,\"C\"\n\
    V=VAR(\"I\")\n\
    DIM M[2,3]:M[1,2]=MAX(1,2)\n\
    ON 0 GOTO @L\n@L\n\
    ?A%[3];S$;V\n";

#[test]
fn a_reported_run_counts_its_work() {
    let (work, out) = run(PROGRAM, true);
    assert_eq!(out, "6ABC4\n");
    assert_eq!(work.def_calls, 4);
    // LEN four times, MAX once, and ON's jump as a call with no
    // arguments.
    assert_eq!(work.builtin_calls, 6);
    assert_eq!(work.builtin_args, 6);
    // A%[I] written four times and read once, and M[1,2] written.
    assert_eq!(work.elements, 6);
    assert_eq!(work.extra_indices, 1);
    // FOR tests I five times: 0 to 3 and the end.
    assert_eq!(work.for_tests, 5);
    assert_eq!(work.joins, 2);
    assert_eq!(work.lookups, 1);
    assert!(work.statements > 10);
}

/// Reporting takes the fused steps away, which changes nothing a program
/// or a frame's budget sees.
#[test]
fn reporting_changes_nothing_else() {
    let (counted, out) = run(PROGRAM, true);
    let (plain, plain_out) = run(PROGRAM, false);
    assert_eq!(out, plain_out);
    assert_eq!(counted.executed, plain.executed);
    assert_eq!(plain.statements, 0);
}

/// Characters of a string read or written by index are counted apart
/// from the other elements.
#[test]
fn string_characters_by_index_count_apart() {
    let source = "S$=\"ABC\":DIM A$[1]:A$[0]=\"DE\"\nS$[1]=\"X\":?ASC(S$[0]);A$[0];S$[1]\n";
    let (work, out) = run(source, true);
    assert_eq!(out, "65DEX\n");
    // A$[0] twice, and S$ three times.
    assert_eq!(work.elements, 5);
    assert_eq!(work.str_elements, 3);
}
