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
    // ON's jump as a call with no arguments. LEN and MAX of two count as
    // no call.
    assert_eq!(work.builtin_calls, 1);
    assert_eq!(work.builtin_args, 0);
    // A%[I] written four times and read once, and M[1,2] written.
    assert_eq!(work.elements, 6);
    assert_eq!(work.extra_indices, 1);
    // FOR tests I five times: 0 to 3 and the end. F runs its RETURN four
    // times, each call setting up a parameter and a result.
    assert_eq!(work.for_tests, 5);
    assert_eq!((work.def_statements, work.def_values), (4, 8));
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

/// An IF or ELSEIF of an array element is counted apart; one of a
/// comparison or a variable is not.
#[test]
fn ifs_of_array_elements_count_apart() {
    let source = "DIM A%[2]:A%[1]=1:V=0\n\
        FOR I=0 TO 1\n IF A%[I] THEN V=V+1\n IF V THEN V=V+1\n IF A%[I]>0 THEN V=V+1\nNEXT\n\
        IF V>9 THEN\n ?0\nELSEIF A%[0] THEN\n ?1\nELSE\n ?V\nENDIF\n";
    let (work, out) = run(source, true);
    assert_eq!(out, "3\n");
    assert_eq!(work.element_tests, 3);
}

/// ASC and LEN are not counted as builtin calls, and CHR$ is counted on
/// its own too, when the work is reported, but not when it makes a code
/// one of the last four made.
#[test]
fn asc_and_chr_count_apart() {
    let source = "C=65:S$=CHR$(C)+CHR$(C+1)+CHR$(C+1):?ASC(S$);LEN(S$)\n";
    let (work, out) = run(source, true);
    assert_eq!(out, "653\n");
    assert_eq!(work.builtin_calls, 3);
    assert_eq!(work.chr_calls, 2);
    assert_eq!(run(source, false).0.chr_calls, 0);
}

/// SIN, COS, TAN and SQR are counted apart and as no call, and SIN, COS
/// and TAN of a number past a quarter of pi apart again. Statements in a
/// DEF are counted apart: the FOR and its NEXT three times.
#[test]
fn cheap_builtins_and_fors_in_defs_count_apart() {
    let source = "DEF F\n FOR I=1 TO 2:NEXT\nEND\n\
        A=SIN(0.5)+SIN(-2)+COS(1)+TAN(0)+SQR(4):F\n?A>0\n";
    let (work, out) = run(source, true);
    assert_eq!(out, "1\n");
    assert_eq!(work.builtin_calls, 0);
    let i = |name| sb_core::vm::CHEAP.iter().position(|&n| n == name).unwrap();
    assert_eq!(work.cheap_calls[i("SIN")], 2);
    assert_eq!(work.cheap_calls[i("SQR")], 1);
    assert_eq!(work.trig_reductions, 2);
    assert_eq!((work.for_tests, work.def_statements), (3, 4));
}

/// FILL counts the elements it writes, from its start to its count or
/// the array's end.
#[test]
fn fill_counts_its_elements() {
    let source = "DIM A%[100]:FILL A%,1:FILL A%,2,90:FILL A%,3,10,20:?A%[95]\n";
    let (work, out) = run(source, true);
    assert_eq!(out, "2\n");
    assert_eq!(work.filled, 100 + 10 + 20);
    assert_eq!(run(source, false).0.filled, 0);
}

/// The builtins that cost more than a call are counted apart, and those
/// that cost none, INT and MIN of two among them, are not calls.
#[test]
fn builtins_count_by_what_they_cost() {
    let source = "X=2.5:A$=STR$(X)+STR$(1):B=VAL(\"3\"):C=INT(X)+MIN(X,1)+MAX(X,1,2)\n?A$;B;C\n";
    let (work, out) = run(source, true);
    assert_eq!(out, "2.5135.5\n");
    let i = |name| sb_core::vm::COSTLY.iter().position(|&n| n == name).unwrap();
    assert_eq!(work.costly_calls[i("STR$")], 2);
    assert_eq!(work.costly_calls[i("VAL")], 1);
    // STR$ twice, VAL and MAX of three.
    assert_eq!(work.builtin_calls, 4);
}

/// A join counts the characters of what it makes, and comparing strings
/// is no join.
#[test]
fn joins_count_their_characters() {
    let source = "S$=\"AB\":T$=S$+\"CDE\"\nIF S$==T$ THEN S$=\"\"\n?T$;S$\n";
    let (work, out) = run(source, true);
    assert_eq!(out, "ABCDEAB\n");
    assert_eq!(work.joins, 1);
    assert_eq!(work.join_chars, 5);
    assert_eq!(work.str_compares, 1);
}
