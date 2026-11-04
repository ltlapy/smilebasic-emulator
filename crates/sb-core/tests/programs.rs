//! Whole programs run headless, checked against their PRINT output.
//! Most programs are the examples from the SmileBASIC 4 reference.

use sb_core::error::ErrorKind;
use sb_core::host::TextHost;
use sb_core::vm::{Status, Vm};

fn run(source: &str) -> Result<String, sb_core::error::Error> {
    run_with_input(source, &[])
}

fn run_with_input(source: &str, input: &[&str]) -> Result<String, sb_core::error::Error> {
    let mut host = TextHost {
        input: input.iter().map(|s| s.to_string()).collect(),
        ..TextHost::default()
    };
    let compiled = sb_core::compile(source, &host)?;
    let mut vm = Vm::new(compiled);
    for _ in 0..10_000 {
        match vm.run(&mut host, 100_000)? {
            Status::Ended => return Ok(host.output),
            Status::Running | Status::Wait { .. } | Status::Retry => {}
            Status::Break { .. } => unreachable!("nothing presses the + button"),
            Status::Stop { .. } | Status::Traced { .. } | Status::Debug { .. } => {}
        }
    }
    panic!("program did not end; output so far: {:?}", host.output);
}

fn output(source: &str) -> String {
    run(source).unwrap_or_else(|e| panic!("{e}"))
}

fn error(source: &str) -> (ErrorKind, u32) {
    let e = run(source).unwrap_err();
    (e.kind, e.line)
}

#[test]
fn bqparam_checks_the_gain_count_first() {
    let kind = |src: &str| run(src).err().map(|e| e.kind);
    assert_eq!(
        kind("DIM F[13]:BQPARAM F,5,0,1,1"),
        Some(ErrorKind::WrongNumberOfArguments)
    );
    assert_eq!(
        kind("DIM F[13]:BQPARAM F,0,44100,1,0"),
        Some(ErrorKind::IllegalFunctionCall)
    );
}

#[test]
fn print_separators() {
    assert_eq!(output("PRINT 1;2\n?\"A\",\"B\"\nPRINT"), "12\nA   B\n\n");
    assert_eq!(output("?1;:?2"), "12\n");
    assert_eq!(output("? ?1"), "\n1\n");
}

#[test]
fn arithmetic_and_types() {
    assert_eq!(output("?7/2;\" \";7 DIV 2;\" \";7 MOD 2"), "3.5 3 1\n");
    assert_eq!(output("A%=2147483647:B=A%+1:?B"), "2147483648\n");
    // The suffix gives only the starting value (checked on hardware).
    assert_eq!(output("A%=3.9:?A%"), "3.9\n");
    assert_eq!(output("?\"AB\"*3+\"!\""), "ABABAB!\n");
    assert_eq!(output("?1==1, 1<0"), "1   0\n");
    assert_eq!(output("A$=1:B%=\"X\":?A$;B%"), "1X\n");
    assert_eq!(error("\n?1/0"), (ErrorKind::DivideByZero, 2));
}

/// The order SmileBASIC 4 works things out in, as TEST08 showed on
/// hardware: right to left nearly everywhere.
#[test]
fn evaluation_order() {
    let defs = "\nDEF F(N)\nO$=O$+STR$(N)\nRETURN N\nEND\nDEF G(A,B,C)\nRETURN 0\nEND\n\
                DEF H A,B\nEND\nDEF P(N)\n?\"<\";N;\">\";\nRETURN N\nEND";
    let order = |code: &str| output(&format!("O$=\"\":{code}:?O${defs}"));
    for (code, want) in [
        ("X=F(1)+F(2)*F(3)", "321"),
        ("X=MAX(F(1),F(2),F(3))", "321"),
        ("X=G(F(1),F(2),F(3))", "321"),
        ("H F(1),F(2)", "21"),
        ("DIM A[5,5]:A[F(1),F(2)]=F(3)", "321"),
        ("X=F(F(1)+F(2))", "213"),
        ("FOR I=F(1) TO F(2) STEP F(3):NEXT", "13232"),
        ("X=F(1)+F(2)+F(3)", "321"),
        ("X=-F(1)*(F(2)-F(3))", "321"),
        ("X=F(1) AND F(2)", "21"),
        // TEST12: literals and sizes last first, SWAP and INC working
        // each place out once, CALL's arguments before its name.
        ("DIM A[]=[F(1),F(2)]", "21"),
        ("DIM A[F(2),F(3)]", "32"),
        ("DIM C[4]:SWAP C[F(0)],C[F(1)]", "10"),
        ("DIM C[4]:INC C[F(0)],F(5)", "50"),
        ("CALL MID$(\"HX\",F(0),1),F(1),F(2)", "210"),
    ] {
        assert_eq!(order(code), format!("{want}\n"), "{code}");
    }
    // PRINT works every item out, last first, before printing any.
    assert_eq!(output(&format!("?P(1);P(2);P(3){defs}")), "<3><2><1>123\n");
    // && and || still go left to right, stopping early.
    assert_eq!(order("X=F(0) && F(1)"), "0\n");
}

#[test]
fn on_break_in_a_def_names_outer_labels() {
    // TEST15: it compiles, and + would jump there.
    assert_eq!(
        output("W:?\"OK\"\nEND\nDEF W\nON BREAK GOTO @B\nEND\n@B:?\"B\""),
        "OK\n"
    );
}

/// RESTORE's special strings from SmileBASIC 4.4, as TEST12 showed them.
#[test]
fn restore_push_and_pop() {
    let src = "RESTORE @B:READ A:RESTORE \"PUSH\":READ B\nRESTORE @A:READ C:RESTORE \"POP\":READ D\n\
               RESTORE \"BEGIN\":READ E:?A;B;C;D;E\n@A:DATA 10\n@B:DATA 1,2,3";
    assert_eq!(output(src), "1210210\n");
    assert_eq!(error("RESTORE \"POP\"").0, ErrorKind::StackUnderflow);
    assert_eq!(
        error("GOSUB @S:END\n@S:RESTORE \"PUSH\":RETURN").0,
        ErrorKind::UseRestorePop
    );
    assert_eq!(
        output("GOSUB @S:?\"OK\":END\n@S:RESTORE \"PUSH\":RESTORE \"POP\":RETURN"),
        "OK\n"
    );
}

#[test]
fn read_modify_write() {
    assert_eq!(
        output(
            "DIM C[]=[1,2]:SWAP C[0],C[1]:INC C[0],10:A$=\"X\":B$=\"Y\":SWAP A$,B$:INC A$,\"Z\":?C[0];C[1];A$;B$"
        ),
        "121YZX\n"
    );
    assert_eq!(
        output("A=1:INC VAR(\"A\"):DIM S$[1]:S$[0]=\"AB\":INC S$[0],\"C\":?A;S$[0]"),
        "2ABC\n"
    );
}

#[test]
fn short_circuit() {
    let src = "DEF T(X)\n?\"T\";\nRETURN X\nEND\n?T(0)&&T(1)\n?T(1)||T(0)\n?T(1)&&T(2)";
    assert_eq!(output(src), "T0\nT1\nTT1\n");
}

#[test]
fn string_reference_semantics() {
    // The reference type example from the language specification.
    let src = r#"
A$="ABC"
B$=A$
?A$,B$
A$[1]="Z"
?A$,B$
B$=B$+"X"
?A$,B$
C$=B$
PUSH C$,"W"
?A$,B$,C$
"#;
    assert_eq!(
        output(src),
        "ABC ABC\nAZC AZC\nAZC AZCX\nAZC AZCXW   AZCXW\n"
    );
    assert_eq!(
        output("A$=\"12345\":A$[3]=\"ABC\":?A$;\" \";A$[0]"),
        "123ABC5 1\n"
    );
}

#[test]
fn if_forms() {
    let src = r#"
FOR A=1 TO 3
IF A==1 THEN
 PRINT "ONE"
ELSEIF A==2 THEN
 PRINT "TWO"
ELSE
 PRINT "MANY"
ENDIF
NEXT
IF 0 THEN ?"X" ELSE ?"Y":?"Z"
IF 1 GOTO @SKIP
?"NOT HERE"
@SKIP
?"END"
"#;
    assert_eq!(output(src), "ONE\nTWO\nMANY\nY\nZ\nEND\n");
}

#[test]
fn case_when() {
    let src = "FOR A=0 TO 3\nCASE A\nWHEN 0:WHEN 1: PRINT\"A\";\nWHEN 2: PRINT\"B\";\nOTHERWISE: PRINT\"X\";\nENDCASE\nNEXT";
    assert_eq!(output(src), "AABX");
}

#[test]
fn loops() {
    assert_eq!(
        output("FOR I=0 TO 9 STEP 2\n PRINT I;\",\";\nNEXT"),
        "0,2,4,6,8,"
    );
    assert_eq!(output("FOR I=3 TO 1 STEP -1:?I;:NEXT"), "321");
    assert_eq!(output("FOR I=1 TO 0:?I:NEXT:?\"DONE\""), "DONE\n");
    assert_eq!(
        output("FOR I=0 TO 9\n IF I==1 THEN CONTINUE\n IF I==7 THEN BREAK\n PRINT I;\",\";\nNEXT"),
        "0,2,3,4,5,6,"
    );
    assert_eq!(output("A=0:B=4\nWHILE A<B\n A=A+1\nWEND\n?A"), "4\n");
    assert_eq!(output("A=0:B=4\nREPEAT\n A=A+1\nUNTIL A>B\n?A"), "5\n");
    assert_eq!(
        output("I=0\nLOOP\n I=I+1\n IF I>100 THEN BREAK\nENDLOOP\n?I"),
        "101\n"
    );
    assert_eq!(
        output("I=0:REPEAT:I=I+1:IF I<3 THEN CONTINUE\n?I;:UNTIL I>=5"),
        "345"
    );
}

#[test]
fn labels_may_start_with_a_digit_or_underscore() {
    let src = "GOSUB @1ST\nGOSUB \"@2_ND\"\nGOTO @_3\n@1ST\n?1:RETURN\n@2_ND\n?2:RETURN\n@_3\n?3";
    assert_eq!(output(src), "1\n2\n3\n");
}

#[test]
fn goto_gosub_on() {
    let src = r#"
GOSUB @SUB
GOSUB "@SUB"
FOR IDX=0 TO 2
ON IDX GOSUB @A,@B
NEXT
END
@SUB
?"SUB"
RETURN
@A
?"A":RETURN
@B
?"B":RETURN
"#;
    assert_eq!(output(src), "SUB\nSUB\nA\nB\n");
    assert_eq!(error("RETURN").0, ErrorKind::ReturnWithoutGosub);
    assert_eq!(error("GOTO @NOWHERE"), (ErrorKind::UndefinedLabel, 1));
    assert_eq!(error("L$=\"@X\"\nGOTO L$"), (ErrorKind::UndefinedLabel, 2));
    // RESTORE finds out only when it runs.
    assert_eq!(output("IF 0 THEN RESTORE @NOWHERE\n?1"), "1\n");
    assert_eq!(
        error("?1\nRESTORE @NOWHERE"),
        (ErrorKind::UndefinedLabel, 2)
    );
}

#[test]
fn user_functions() {
    let src = r#"
DEF ADD(X,Y)
RETURN X+Y
END
DEF FACTORIAL(N)
IF N==1 THEN RETURN N
RETURN N*FACTORIAL(N-1)
END
DEF REVERSE$(T$)
VAR A$=""
VAR L=LEN(T$)
WHILE L>0
 A$=A$+MID$(T$,L-1,1)
 DEC L
WEND
RETURN A$
END
PRINT ADD(10,5)
PRINT FACTORIAL(4)
PRINT REVERSE$("BASIC")
"#;
    assert_eq!(output(src), "15\n24\nCISAB\n");
}

#[test]
fn user_commands_with_out() {
    let src = r#"
DEF CALCPM A,B OUT OP,OM
OP=A+B
OM=A*B
END
CALCPM 5,10 OUT P,M
PRINT P,M
DEF FUNC2 X,Y
PRINT X*Y
END
FUNC2 3,4
CALL "CALCPM",2,3 OUT P,M
?P;M
"#;
    assert_eq!(output(src), "15  50\n12\n56\n");
}

#[test]
fn variadic_defs() {
    let src = "DEF VARFUNC * OUT *\n FOR I=0 TO DEFOUTC()-1\n  DEFOUT I, DEFARG(I)*2\n NEXT\nEND\nVARFUNC 1,2,3 OUT A,B\n?A;B;DEFC()\nDEF DEFC()\nRETURN 0\nEND";
    assert_eq!(output(src), "240\n");
}

#[test]
fn variadic_outs_as_a_function() {
    let src = "DEF F * OUT *\nDEFOUT 0,DEFARG(0)*2+DEFOUTC()\nEND\n?F(20)";
    assert_eq!(output(src), "41\n");
}

#[test]
fn array_parameters_may_say_so() {
    let src = "DEF F(A[],B)\nRETURN A[1]+B\nEND\nDEF G A[] OUT C\nC=A[0]\nEND\nDIM X[]=[1,2]\nG X OUT D\n?F(X,3);D";
    assert_eq!(output(src), "51\n");
}

#[test]
fn locals_and_globals() {
    // G is used at top level, so the DEF sees the global; L stays local.
    let src = "G=1\nDEF F\nG=G+1\nL=5\nEND\nF:F\n?G";
    assert_eq!(output(src), "3\n");
    let src = "DEF F\nL=5\nEND\nF\n?VAR(\"L\")";
    assert_eq!(error(src).0, ErrorKind::UndefinedVariable);
    let src = "DEF F\nVAR G=7\nEND\nG=1\nF\n?G";
    assert_eq!(output(src), "1\n");
}

#[test]
fn arrays() {
    let src = "VAR A[3]\nA[1]=5.5\n?A[1];LEN(A)\nDIM B%[2,3]\nB%[1,2]=9\n?B%[5]\nVAR C$[]=[\"X\",\"Y\"]\n?C$[1];LAST(C$)";
    assert_eq!(output(src), "5.53\n9\nY1\n");
    assert_eq!(
        error("VAR A[2]\n?A[2]"),
        (ErrorKind::SubscriptOutOfRange, 2)
    );
    assert_eq!(error("VAR A:VAR A").0, ErrorKind::DuplicateVariable);
}

#[test]
fn dynamic_access() {
    let src = "FOO=1\nVAR(\"FOO\")=3\n?FOO;VAR(\"FOO\")\nA=CALL(\"ADD\",2,3)\n?A\nDEF ADD(X,Y)\nRETURN X+Y\nEND";
    assert_eq!(output(src), "33\n5\n");
    assert_eq!(error("?VAR(\"NOPE\")").0, ErrorKind::UndefinedVariable);
    assert_eq!(error("?CALL(\"NOPE\")").0, ErrorKind::UndefinedFunction);
}

#[test]
fn chklabel_chkcall_chkvar() {
    let src = "@MAIN\nVAR COUNTX$\n\
        ?CHKLABEL(\"@MAIN\");CHKLABEL(\"MAIN\");CHKLABEL(\"@NONE\")\n\
        ?CHKCALL(\"KEYCHECK\");CHKCALL(\"SIN\");CHKCALL(\"DEFARGC\");CHKCALL(\"NONE\")\n\
        ?CHKVAR(\"COUNTX$\");CHKVAR(\"COUNTX\");CHKVAR(\"L\")\n\
        KEYCHECK\n\
        DEF KEYCHECK\nVAR L\n@INNER\n\
        ?CHKLABEL(\"@INNER\");CHKLABEL(\"@MAIN\");CHKLABEL(\"@MAIN\",1);CHKVAR(\"L\");CHKVAR(\"COUNTX$\")\n\
        END";
    assert_eq!(output(src), "100\n1110\n100\n10111\n");
    assert_eq!(error("?CHKVAR(1)").0, ErrorKind::TypeMismatch);
    assert_eq!(
        output("?CHKCALL(\"VSYNC\");CHKCALL(\"INC\");CHKCALL(\"STOP\")"),
        "100\n"
    );
}

#[test]
fn constants_and_data() {
    let src = "CONST #C1=1,#C2=\"ABC\",#C3=RAD(180)\n?#C1;#C2;#C3==#PI\nENUM #TYPE1=2,#TYPE2,#TYPE3\n?#TYPE1;#TYPE2;#TYPE3";
    assert_eq!(output(src), "1ABC1\n234\n");
    let src = "READ A,B$\n?A;B$\nRESTORE @D2\nREAD C\n?C\n@D1\nDATA 1,\"X\"\n@D2\nDATA -2.5";
    assert_eq!(output(src), "1X\n-2.5\n");
    assert_eq!(error("READ A").0, ErrorKind::OutOfData);
    assert_eq!(output("?#_LINE\n?#_LINE"), "1\n2\n");
}

#[test]
fn option_strict() {
    assert_eq!(
        error("OPTION STRICT\nVAR A\nA=1\nB=2").0,
        ErrorKind::UndefinedVariable
    );
    // It holds from where it stands; a variable used before it exists.
    assert_eq!(output("A=1\nOPTION STRICT\nVAR B=A+1\n?B"), "2\n");
    assert_eq!(
        error("A=1\nOPTION STRICT\nB=2").0,
        ErrorKind::UndefinedVariable
    );
    assert_eq!(output("OPTION DEFINT\nA=7/2\n?A,TYPEOF(A)"), "3.5 2\n");
    assert_eq!(output("OPTION DEFINT\n?TYPEOF(A)"), "1\n");
}

#[test]
fn return_value_in_a_command_def_is_ignored() {
    // Not even compiled: OPTION STRICT does not see the undeclared name.
    let src =
        "OPTION STRICT\nVAR A\nT OUT A\n?A\nDEF T OUT Y\n Y=\"OK\"\n RETURN NOT_DECLARED\nEND";
    assert_eq!(output(src), "OK\n");
}

/// TEST60: `RETURN F()` in a command DEF is refused when the program compiles.
#[test]
fn a_call_with_no_arguments_after_return_in_a_command_def() {
    let source = "?1\nDEF A\n RETURN F()\nEND\nDEF F()\nEND";
    assert!(sb_core::parser::parse(source).is_err());
    let source = "?1\nDEF A()\n RETURN F()\nEND\nDEF F()\nEND";
    assert!(sb_core::parser::parse(source).is_ok());
}

#[test]
fn inc_dec_swap() {
    assert_eq!(output("A=1:INC A:INC A,5:DEC A,2\n?A"), "5\n");
    assert_eq!(output("VAR X[2]:INC X[1],3:?X[1]"), "3\n");
    assert_eq!(output("A$=\"A\":B$=\"B\":SWAP A$,B$:?A$;B$"), "BA\n");
}

#[test]
fn vsync_yields() {
    let mut host = TextHost::default();
    let mut vm = Vm::new(sb_core::compile("VSYNC\nWAIT 3\n?\"X\"", &host).unwrap());
    assert_eq!(
        vm.run(&mut host, 1000).unwrap(),
        Status::Wait {
            frames: 1,
            vsync: true
        }
    );
    assert_eq!(
        vm.run(&mut host, 1000).unwrap(),
        Status::Wait {
            frames: 3,
            vsync: false
        }
    );
    assert_eq!(vm.run(&mut host, 1000).unwrap(), Status::Ended);
    assert_eq!(host.output, "X\n");
}

#[test]
fn compile_errors() {
    // An unknown name fails only when reached, as a Syntax error (checked
    // on hardware), since another slot may define it.
    assert_eq!(error("?1\nFOO 1").0, ErrorKind::SyntaxError);
    assert_eq!(error("?1\n?(").0, ErrorKind::SyntaxError);
    assert_eq!(error("LEN 1").0, ErrorKind::IllegalFunctionCall);
    assert_eq!(error("@A\n@A").0, ErrorKind::DuplicateLabel);
    assert_eq!(error("BREAK").0, ErrorKind::IllegalStatement);
}

#[test]
fn deep_recursion_is_an_error() {
    let src = "DEF F(N)\nRETURN F(N+1)\nEND\n?F(0)";
    assert_eq!(error(src).0, ErrorKind::StackOverflow);
}

#[test]
fn input_and_linput() {
    let src = "INPUT \"NAME AND AGE\";N$,A\n?N$;A+1";
    assert_eq!(
        run_with_input(src, &["BOB,41"]).unwrap(),
        "NAME AND AGE? BOB,41\nBOB42\n"
    );
    // Missing values and non-numbers ask again.
    assert_eq!(
        run_with_input("INPUT A,B\n?A*B", &["3", "X,2", "3,&H10"]).unwrap(),
        "? 3\n?Redo from start\nX,2\n?Redo from start\n3,&H10\n48\n"
    );
    assert_eq!(
        run_with_input("INPUT \"N\",N%:?N%", &["7.9"]).unwrap(),
        "N7.9\n7.9\n"
    );
    assert_eq!(
        run_with_input("LINPUT \"ADDR:\";L$:?LEN(L$)", &["1, MAIN ST"]).unwrap(),
        "ADDR:1, MAIN ST\n10\n"
    );
    // Extra values are left out (checked on hardware).
    assert_eq!(
        run_with_input("INPUT A\n?A", &["1,2"]).unwrap(),
        "? 1,2\n1\n"
    );
    assert_eq!(
        run_with_input("VAR A$[2]:INPUT A$[1]:?A$[1]", &["HI"]).unwrap(),
        "? HI\nHI\n"
    );
}

/// A host whose ASK waits one frame for each call before answering.
struct Asking {
    text: TextHost,
    waited: bool,
    retry: bool,
}

impl sb_core::host::Host for Asking {
    fn lookup(&self, name: &str) -> Option<u32> {
        (name == "ASK").then_some(0)
    }

    fn call(
        &mut self,
        _id: u32,
        args: &[sb_core::value::Value],
        want: sb_core::bytecode::Want,
    ) -> sb_core::error::Result<Vec<sb_core::value::Value>> {
        assert_eq!(args.len(), 1, "arguments survive a retry");
        self.waited = !self.waited;
        self.retry = self.waited;
        let answer = sb_core::value::Value::Int(args[0].to_int()? + 6);
        Ok(vec![answer; want.count()])
    }

    fn print(&mut self, s: Option<i32>, t: &[u16]) -> sb_core::error::Result<()> {
        self.text.print(s, t)
    }

    fn print_tab(&mut self, s: Option<i32>) -> sb_core::error::Result<()> {
        self.text.print_tab(s)
    }

    fn print_newline(&mut self, s: Option<i32>) -> sb_core::error::Result<()> {
        self.text.print_newline(s)
    }

    fn input_start(&mut self) -> sb_core::error::Result<()> {
        Ok(())
    }

    fn input_poll(&mut self) -> sb_core::error::Result<Option<Vec<u16>>> {
        Ok(None)
    }

    fn retry(&mut self) -> bool {
        std::mem::take(&mut self.retry)
    }
}

#[test]
fn host_calls_can_wait_for_the_user() {
    let mut host = Asking {
        text: TextHost::default(),
        waited: false,
        retry: false,
    };
    let program = sb_core::compile("?1;ASK(1);CALL \"ASK\",0", &host).unwrap();
    let mut vm = Vm::new(program);
    let mut frames = 0;
    while vm.run(&mut host, 1000).unwrap() != Status::Ended {
        frames += 1;
    }
    assert_eq!((host.text.output.as_str(), frames), ("17", 2));
}

#[test]
fn color_functions() {
    assert_eq!(
        output(
            "?HEX$(RGBF(1,0.5,0));\" \";HEX$(RGBF(0.5,1,1,1));\" \";HEX$(RGBF(&HFF000000,,1,,))"
        ),
        "FFFF7F00 7FFFFFFF FFFF0000\n"
    );
    // Numbers printed with ; follow each other without spaces.
    assert_eq!(
        output("RGBF &H80FF0000 OUT A,R,G,B:?A>0.5;A<0.51;R;G"),
        "1110\n"
    );
    assert_eq!(
        output("?HEX$(HSV(0,255,255));\" \";HEX$(HSV(480,255,255,128));\" \";HEX$(HSVF(0.5,1,1))"),
        "FFFF0000 8000FF00 FF00FFFF\n"
    );
    assert_eq!(
        output("HSV RGB(0,255,0) OUT H,S,V,A:?H;\",\";S;\",\";V;\",\";A"),
        "120,255,255,255\n"
    );
    assert_eq!(output("HSVF RGB(0,0,255) OUT H,S,V:?H*3"), "2\n");
    assert_eq!(error("?RGBF(2,0,0)").0, ErrorKind::OutOfRange);
    // Only constants are checked, before the program runs; values from
    // variables are clamped.
    assert!(sb_core::compile("?1\n?RGBF(1.1,0,0)", &TextHost::default()).is_err());
    assert_eq!(
        output("R=1.1:A=2:?HEX$(RGBF(R,0.5,0));HEX$(RGBF(1,1,1,A))"),
        "FFFF7F00FFFFFFFF\n"
    );
    assert_eq!(output("CONST #C=HSV(120,255,255):?HEX$(#C)"), "FF00FF00\n");
    // Below 0 too, as constants; a variable is clamped.
    assert!(sb_core::compile("?RGBF(1,-1,0)", &TextHost::default()).is_err());
    assert_eq!(output("I=-1:?HEX$(RGBF(1,I,0.5))"), "FFFF007F\n");
    assert_eq!(output("?HEX$(RGB(584,-1,9))"), "FFFF0009\n");
    assert_eq!(error("?HSV(0,256,0)").0, ErrorKind::OutOfRange);
}

/// A host with fixed callbacks for CALL SPRITE and CALL TEXT.
struct Callbacking {
    text: TextHost,
}

impl sb_core::host::Host for Callbacking {
    fn lookup(&self, _name: &str) -> Option<u32> {
        None
    }

    fn call(
        &mut self,
        _id: u32,
        _args: &[sb_core::value::Value],
        _want: sb_core::bytecode::Want,
    ) -> sb_core::error::Result<Vec<sb_core::value::Value>> {
        unreachable!("no host commands")
    }

    fn print(&mut self, s: Option<i32>, t: &[u16]) -> sb_core::error::Result<()> {
        self.text.print(s, t)
    }

    fn print_tab(&mut self, s: Option<i32>) -> sb_core::error::Result<()> {
        self.text.print_tab(s)
    }

    fn print_newline(&mut self, s: Option<i32>) -> sb_core::error::Result<()> {
        self.text.print_newline(s)
    }

    fn input_start(&mut self) -> sb_core::error::Result<()> {
        Ok(())
    }

    fn input_poll(&mut self) -> sb_core::error::Result<Option<Vec<u16>>> {
        Ok(None)
    }

    fn next_callback(
        &mut self,
        kind: sb_core::bytecode::Callbacks,
        after: i32,
    ) -> Option<(i32, Vec<u16>)> {
        let list: &[(i32, &str)] = match kind {
            sb_core::bytecode::Callbacks::Sprite => &[(5, "@A"), (7, "B")],
            sb_core::bytecode::Callbacks::Text => &[(2, "@T")],
        };
        list.iter()
            .find(|&&(i, _)| i > after)
            .map(|&(i, t)| (i, t.encode_utf16().collect()))
    }
    fn callbacks_end(&self, kind: sb_core::bytecode::Callbacks) -> i32 {
        match kind {
            sb_core::bytecode::Callbacks::Sprite => 4096,
            sb_core::bytecode::Callbacks::Text => 5,
        }
    }
}

#[test]
fn call_sprite_and_text_run_the_host_callbacks() {
    let mut host = Callbacking {
        text: TextHost::default(),
    };
    let program = sb_core::compile(
        "CALL SPRITE:?\"/\";CALLIDX():END\n\
         @A:?CALLIDX();:CALL TEXT:?CALLIDX();:RETURN\n\
         @T:?\"t\";CALLIDX();:RETURN\n\
         DEF B:?\"b\";CALLIDX();:END",
        &host,
    )
    .unwrap();
    let mut vm = Vm::new(program);
    while vm.run(&mut host, 1000).unwrap() != Status::Ended {}
    // Once the list ends, CALLIDX() is one past the last number.
    assert_eq!(host.text.output, "5t25b7/4096\n");
    assert_eq!(output("CALL SPRITE:CALL TEXT:?1"), "1\n");
}

#[test]
fn constant_operations_fail_when_compiled() {
    let compiled = |src: &str| {
        sb_core::compile(src, &TextHost::default())
            .err()
            .map(|e| e.kind)
    };
    assert_eq!(compiled("IF 0 THEN ?\"A\"*-1"), Some(ErrorKind::Overflow));
    assert_eq!(compiled("?-2147483648 DIV -1"), None);
    assert_eq!(output("?VAL(\" 12 \");VAL(\"12\")"), "012\n");
    // Inside an expression that is not constant, and deep in one that is.
    assert_eq!(compiled("A=1:?A+(\"A\"*-1)"), Some(ErrorKind::Overflow));
    assert_eq!(
        compiled("?\"B\"+(\"A\"*-1)+\"C\""),
        Some(ErrorKind::Overflow)
    );
    // A long constant chain is worked out once, not once for each part:
    // programs that write programs build such lines.
    let chain = vec!["\"ABCDEFGHIJ\"+CHR$(10)"; 200].join("+");
    assert_eq!(compiled(&format!("A$={chain}")), None);
    // The right operand is compiled first, so its error comes first.
    assert_eq!(compiled("?#NOPE+\"A\"*-1"), Some(ErrorKind::Overflow));
    assert_eq!(
        compiled("?\"A\"*-1+#NOPE"),
        Some(ErrorKind::UndefinedConstant)
    );
    assert_eq!(
        compiled("?-(\"A\"*-1)+#NOPE"),
        Some(ErrorKind::UndefinedConstant)
    );
    assert_eq!(
        compiled("?#NOPE&&\"A\"*-1"),
        Some(ErrorKind::UndefinedConstant)
    );
}

/// Program generators write lines of thousands of terms, such as
/// `"A"+"B"+...`. Parsing, checking, compiling and dropping such a chain
/// must not take stack in proportion to its length.
#[test]
fn long_operator_chains_fit_a_small_stack() {
    const TERMS: usize = 20_000;
    let chain = |first: &str, op: &str, term: &str| {
        let mut s = first.to_string();
        for _ in 1..TERMS {
            s.push_str(op);
            s.push_str(term);
        }
        s
    };
    let strings = chain("\"A\"", "+", "\"B\"");
    let led_by_var = chain("A$", "+", "\"B\"");
    let ints = chain("1", "+", "1");
    let logic = chain("1", "&&", "A");
    let mixed = chain("2", "*", "1+0");
    let indexed = chain("A", "", "[0]");
    let run = std::thread::Builder::new()
        .stack_size(256 << 10)
        .spawn(move || {
            let b = "B".repeat(TERMS - 1);
            // Each string on the way stays until the statement has compiled,
            // and this many do not fit.
            assert_eq!(error(&format!("?LEN({strings})")).0, ErrorKind::OutOfMemory);
            assert_eq!(
                output(&format!("A$=\"A\":?{led_by_var}")),
                format!("A{b}\n")
            );
            assert_eq!(
                output(&format!("CONST #N={ints}:?#N")),
                format!("{TERMS}\n")
            );
            assert_eq!(output(&format!("A=1:?{logic}")), "1\n");
            assert_eq!(output(&format!("?{mixed}")), "2\n");
            assert_eq!(
                output(&format!("READ X:?X\nDATA {ints}")),
                format!("{TERMS}\n")
            );
            // A failing constant part deep in the chain is still found
            // before the program runs.
            let bad = format!("?1\n?A$+\"X\"*-1+{strings}");
            assert_eq!(error(&bad), (ErrorKind::Overflow, 2));
            // Compiles; SmileBASIC 4 has no arrays of arrays, so it fails
            // when it runs.
            assert!(sb_core::compile(&format!("?{indexed}"), &TextHost::default()).is_ok());
        });
    run.unwrap().join().unwrap();
    // Nesting that goes the other way is bounded by the parser's limit,
    // and must fit the 1 MiB main-thread stack of Windows.
    let depth = sb_core::parser::MAX_DEPTH as usize - 2;
    let nested = format!("?{}1{}", "1+(".repeat(depth), ")".repeat(depth));
    let negated = format!("?{}1{}", "1+(-".repeat(depth / 2), ")".repeat(depth / 2));
    let run = std::thread::Builder::new()
        .stack_size(1 << 20)
        .spawn(move || (output(&nested), output(&negated)));
    let (nested, negated) = run.unwrap().join().unwrap();
    assert_eq!(nested, format!("{}\n", depth + 1));
    assert_eq!(negated, format!("{}\n", 1 - (depth / 2) as i32));
}

#[test]
fn format_strings() {
    assert_eq!(
        output(r#"?FORMAT$("%06D|%-4D|%+D|% D|%4X|%B|%%",42,7,3,5,255,5)"#),
        "000042|7   |+3| 5|  FF|101|%\n"
    );
    assert_eq!(
        output(r#"?FORMAT$("%8.2F|%F|%S!|%.2S|%5S",3.14159,-1.5,"AB","XYZ","Q")"#),
        "    3.14|-1.500000|AB!|XY|    Q\n"
    );
    assert_eq!(output(r#"?FORMAT$("%05D|%X",-42,-1)"#), "-0042|FFFFFFFF\n");
    assert_eq!(error(r#"?FORMAT$("%D","A")"#).0, ErrorKind::TypeMismatch);
    // These errors, lower case and the value left over are as TEST12
    // showed on hardware.
    assert_eq!(error(r#"?FORMAT$("%S",5)"#).0, ErrorKind::TypeMismatch);
    assert_eq!(
        error(r#"?FORMAT$("%D %D",1)"#).0,
        ErrorKind::WrongNumberOfArguments
    );
    assert_eq!(error(r#"?FORMAT$("%Q",1)"#).0, ErrorKind::IllegalFormat);
    // Halves go toward zero; the rest round to the nearest (TEST13).
    let rounded = "FOR I=0 TO 7:READ X:?FORMAT$(\"%.0F \",X);:NEXT:READ X:?FORMAT$(\"%.1F\",X)\n\
                   DATA 0.5,1.5,2.5,3.5,4.5,2.7,-2.5,-3.5,0.25";
    assert_eq!(output(rounded), "0 1 2 3 4 3 -2 -3 0.2\n");
    assert_eq!(
        output(r#"?FORMAT$("%d|%x|%s|%f",10,255,"Q",1.5);FORMAT$("%D",1,2)"#),
        "10|FF|Q|1.5000001\n"
    );
}

#[test]
fn copying() {
    assert_eq!(
        output(
            "DIM S[5]=[1,2,3,4,5]:DIM D[2]:COPY D,S:?LEN(D);D[4]\nCOPY D,1,S,3,2:?D[0];D[1];D[2];D[3]"
        ),
        "55\n1454\n"
    );
    assert_eq!(
        output("DIM S[3]=[7,8,9]:A=COPY(S,1):?LEN(A);A[0];A[1]:?COPY(\"HELLO\",1,3)"),
        "289\nELL\n"
    );
    assert_eq!(
        output("A$=\"12345\":COPY A$,2,\"AB\":?A$:B$=\"\":COPY B$,\"XYZ\",1,2:?B$"),
        "12AB5\nYZ\n"
    );
    assert_eq!(
        output("DIM D[2]:COPY D,1,\"@N\",3:READ X:?LEN(D);D[1];D[3];X\n@N:DATA 5,6,7,8"),
        "4578\n"
    );
}

#[test]
fn copy_errors() {
    assert_eq!(
        error("DIM D[2,2]:DIM S[5]:COPY D,S").0,
        ErrorKind::SubscriptOutOfRange
    );
    assert_eq!(error("DIM S[2]:A=COPY(S,1,5)").0, ErrorKind::OutOfRange);
    assert_eq!(
        error("DIM D[3]:COPY D,\"@N\"\n@N:DATA 1").0,
        ErrorKind::OutOfData
    );
}

#[test]
fn sorting() {
    assert_eq!(
        output(
            "DIM K[]=[3,1,2,1]:DIM V$[]=[\"C\",\"A\",\"B\",\"D\"]:SORT K,V$:?K[0];K[1];K[2];K[3];V$[0];V$[1];V$[2];V$[3]"
        ),
        "1123ADBC\n"
    );
    assert_eq!(
        output("DIM K[]=[3,1,2,5]:RSORT 1,2,K:?K[0];K[1];K[2];K[3]"),
        "3215\n"
    );
    // Equal keys keep their order in SORT and swap it in RSORT, as on
    // hardware.
    let ties = "DIM K[]=[3,1,3,2,1],V[]=[0,1,2,3,4]:{}K,V:FOR I=0 TO 4:?V[I];:NEXT";
    assert_eq!(output(&ties.replace("{}", "SORT ")), "14302");
    assert_eq!(output(&ties.replace("{}", "RSORT ")), "20341");
}

#[test]
fn inserting_and_removing() {
    assert_eq!(
        output(
            "DIM A[]=[1,2,3]:INSERT A,1,2,9:?LEN(A);A[0];A[1];A[2];A[3]\nREMOVE A,0,3:?LEN(A);A[0];A[1]"
        ),
        "51992\n223\n"
    );
    assert_eq!(
        output("DIM A$[1]:INSERT A$,1:?LEN(A$);\"[\";A$[1];\"]\""),
        "2[]\n"
    );
    assert_eq!(error("DIM A[3]:INSERT A,4").0, ErrorKind::OutOfRange);
    assert_eq!(error("DIM A[3]:REMOVE A,2,2").0, ErrorKind::OutOfRange);
    assert_eq!(output("DIM A[0]:REMOVE A,0,LEN(A):?LEN(A)"), "0\n");
}

#[test]
fn initialized_arrays_keep_their_sizes() {
    assert_eq!(
        output("DIM P$[3,2]=[\"X\",\"Y\",\"Z\"]:?LEN(P$);DIM(P$);P$[1,0];\"|\";P$[2,1];\"|\""),
        "62Z||\n"
    );
    assert_eq!(error("DIM A[2]=[1,2,3]").0, ErrorKind::SubscriptOutOfRange);
    assert_eq!(error("?ABS(1,2)").0, ErrorKind::WrongNumberOfArguments);
    // Only when the call runs: the lines before it still run.
    assert_eq!(run("?1\n?HSV(120,100)").unwrap_err().line, 2);
    assert_eq!(error("?HSV(120)").0, ErrorKind::WrongNumberOfOutArguments);
    assert_eq!(
        error("HSV &HFF00FF00 OUT H,S").0,
        ErrorKind::WrongNumberOfOutArguments
    );
    assert_eq!(
        error("IF 0 THEN ?ABS(1,2)\n?ABS(1,2)"),
        (ErrorKind::WrongNumberOfArguments, 2)
    );
    assert_eq!(error("?ABS(#NOSUCH)").0, ErrorKind::UndefinedConstant);
    // A function with one OUT variable runs as the function.
    assert_eq!(output("ABS -3 OUT A:?A"), "3\n");
    assert_eq!(output("HSV 120,255,255 OUT C:?HEX$(C,8)"), "FF00FF00\n");
    assert_eq!(output("CALL \"LEN\",\"AB\" OUT L:?L"), "2\n");
    // DEFs too: one with an OUT as a function, a function with one OUT.
    assert_eq!(output("?F(3)\nDEF F A OUT R\n R=A*2\nEND"), "6\n");
    assert_eq!(output("T OUT X:?X\nDEF T()\n RETURN 5\nEND"), "5\n");
    // An OUT variable never set gives the omitted value.
    assert_eq!(
        output("A=E():?TYPEOF(A)\nE OUT B:?TYPEOF(B)\nDEF E OUT X\nEND"),
        "0\n0\n"
    );
    // Not one with two OUTs, at run time: checked on hardware.
    assert_eq!(
        error("?\"S\"\n?G()\nEND\nDEF G OUT X,Y\nEND"),
        (ErrorKind::WrongNumberOfOutArguments, 2)
    );
    // A variable used before its DIM is a duplicate, checked on hardware.
    assert_eq!(
        error("S$=\"A\"\nDIM S$[4]"),
        (ErrorKind::DuplicateVariable, 2)
    );
    assert_eq!(output("DIM S$[4]:S$[0]=\"A\":?S$[0]"), "A\n");
    // Only one-dimensional arrays may be empty.
    assert_eq!(error("DIM R#[0,0]").0, ErrorKind::SubscriptOutOfRange);
    assert_eq!(output("DIM A[0]:?LEN(A)"), "0\n");
}

#[test]
fn smilebasic_debugger_forms() {
    // A bare label after THEN or ELSE jumps, and the rest of the line
    // stays in the branch.
    assert_eq!(
        output("A=0:IF A THEN @X:?\"AFTER\"\n?\"E\":END\n@X:?\"X\""),
        "E\n"
    );
    assert_eq!(
        output("A=1:IF A THEN ?\"T\"; ELSE @X:?\"AFTER\"\n?\"E\":END\n@X"),
        "TE\n"
    );
    assert_eq!(
        output("A=0:IF A THEN ELSE @X:ENDIF\n?\"E\":END\n@X:?\"X\""),
        "X\n"
    );
    // ELSEIF takes GOTO in place of THEN.
    assert_eq!(
        output("IF 0 THEN ?1:ELSEIF 1 GOTO @X\nEND\n@X:?\"X\""),
        "X\n"
    );
    // A label that is not there fails only when the jump runs.
    assert_eq!(output("IF 0 THEN GOTO @NONE\n?\"OK\""), "OK\n");
    assert_eq!(error("?1\nGOSUB @NONE").0, ErrorKind::UndefinedLabel);
    assert_eq!(error("?1\nGOSUB @NONE").1, 2);
    // CHKCALL with another slot still finds the system's names.
    assert_eq!(
        output("?CHKCALL(\"1:ABS\");CHKCALL(\"1:SIN\");CHKCALL(\"1:NONE\")"),
        "110\n"
    );
    // CALL reaches the commands the VM runs itself.
    assert_eq!(
        output("CALL \"WAIT\",1:CALL \"VSYNC\":?CALL(\"FREEMEM\")>0"),
        "1\n"
    );
}

#[test]
fn defs_see_only_globals_from_before_them() {
    // I is first used at top level after F, so F's I is its own.
    assert_eq!(output("A=1\nDEF F\n I=5:A=2\nEND\nI=1:F:?I;A"), "12\n");
    assert_eq!(output("I=1\nDEF F\n I=5\nEND\nF:?I"), "5\n");
}

#[test]
fn loop_ends_inside_blocks_continue() {
    assert_eq!(
        output("FOR I=0 TO 4\n IF I==2 THEN NEXT\n ?I;\nNEXT"),
        "0134"
    );
    assert_eq!(
        output("FOR I=0 TO 3\n IF I<2 THEN\n  NEXT I\n ENDIF\n ?I;\nNEXT"),
        "23"
    );
    assert_eq!(
        output("I=0\nWHILE I<4\n INC I:IF I==2 THEN WEND\n ?I;\nWEND"),
        "134"
    );
    assert_eq!(
        output("I=0\nLOOP\n INC I:IF I<3 THEN ENDLOOP\n ?I:BREAK\nENDLOOP"),
        "3\n"
    );
    // Only the innermost loop's.
    assert_eq!(
        error("FOR I=0 TO 1\nWHILE 1\nIF 1 THEN NEXT\nWEND\nNEXT").0,
        ErrorKind::SyntaxError
    );
}

#[test]
fn finding() {
    assert_eq!(output("DIM A[]=[1,2,3,4,5]:?FIND(A,3);FIND(A,9)"), "2-1\n");
    assert_eq!(
        output(
            "DIM A$[3,3]=[\"1\",\"2\",\"3\",\"4\",\"5\",\"6\",\"7\",\"8\",\"9\"]:FIND A$,\"6\" OUT Y,X:?Y;X;A$[Y,X]"
        ),
        "126\n"
    );
    assert_eq!(error("DIM A[3]:?FIND(A,\"X\")").0, ErrorKind::TypeMismatch);
}

#[test]
fn resizing() {
    assert_eq!(
        output("DIM A[2,3]:A[1,2]=7:RESIZE A,3,3:?LEN(A);A[1,2];A[2,2]:RESIZE A,1,3:?LEN(A)"),
        "970\n3\n"
    );
    assert_eq!(
        error("DIM A[2,3]:RESIZE A,3,4").0,
        ErrorKind::IllegalFunctionCall
    );
    assert_eq!(
        error("DIM A[2,3]:RESIZE A,3").0,
        ErrorKind::WrongNumberOfArguments
    );
}

#[test]
fn array_operations() {
    assert_eq!(
        output(
            "DIM R[4]:DIM P[]=[1,2]:ARYOP #AOPADD,R,P,10:?R[0];R[1];R[2];R[3]\nARYOP #AOPCLP,R,P,0,1.5:?R[0];R[1]\nARYOP #AOPLIP,R,10,20,0.25:?R[0]"
        ),
        "11121112\n11.5\n17.5\n"
    );
    // Crossed bounds: below the lower one gives it, else above the upper.
    assert_eq!(
        output("DIM R[3]:DIM X[]=[0,5,9]:ARYOP #AOPCLP,R,X,2,X:?R[0];R[1];R[2]"),
        "259\n"
    );
    assert_eq!(
        error("DIM R[2]:ARYOP 4,R,1,2").0,
        ErrorKind::IllegalFunctionCall
    );
}

#[test]
fn ringcopy_on_an_array_with_no_channels_does_nothing() {
    assert_eq!(
        output("DIM A[1,3]:RESIZE A,0,3:RINGCOPY A,0,A:?LEN(A)"),
        "0\n"
    );
    assert_eq!(
        output("DIM A[1,3],S[]=[1,2]:RESIZE A,0,3:RINGCOPY A,1,S OUT E:?LEN(A);E"),
        "00\n"
    );
}

#[test]
fn ring_copies() {
    assert_eq!(
        output("DIM D[4]:DIM S[]=[1,2,3]:RINGCOPY D,3,S OUT E:?D[0];D[1];D[2];D[3];E"),
        "23012\n"
    );
    assert_eq!(
        output("DIM D[2,3]:DIM S[]=[5,6]:RINGCOPY D,2,S:?D[0,2];D[0,0];D[1,2];D[1,0]"),
        "5656\n"
    );
    assert_eq!(
        error("DIM D[4],S[6]:RINGCOPY D,1,S,4,4").0,
        ErrorKind::IllegalFunctionCall
    );
}

#[test]
fn signal_processing() {
    assert_eq!(
        output(
            "DIM R[4]=[1,2,3,4],I[4],W[4]:FFTWFN W,#WFRECT:FFT R,I,R,I,W:?R[0];I[1]\nIFFT R,I,R,I:?R[3];\",\";I[3]"
        ),
        "102\n4,0\n"
    );
    assert_eq!(
        output("?CLASSIFY(0.5);CLASSIFY(POW(10,400));CLASSIFY(POW(10,400)-POW(10,400))"),
        "012\n"
    );
    assert_eq!(
        output(
            "DIM OD[2,4],ID[2,4],FP[13]:ID[0,0]=1:BQPARAM FP,#BQLPF,48000,8000,1/SQR(2)\nBIQUAD OD,ID,FP:?OD[0,0]>0;OD[1,0]"
        ),
        "10\n"
    );
}

#[test]
fn free_memory() {
    // Sizes from SmileBASIC 4 (TTEST21 and TTEST23).
    assert_eq!(
        output(
            "FREEMEM OUT T,C:?T;\" \";C\nB=FREEMEM():DIM A[100000]:?B-FREEMEM()\nB=FREEMEM():S$=\"X\"*1000:T$=S$:?B-FREEMEM()\nB=FREEMEM():Q=ARRAY%(10):?B-FREEMEM()"
        ),
        "134216720 134216720\n800064\n2112\n0\n"
    );
    assert_eq!(
        output(
            "DIM A$[3]:A$[1]=\"X\"*100:B=FREEMEM():A$[1]=\"\":?FREEMEM()-B;CHKCALL(\"FREEMEM\")"
        ),
        "3201\n"
    );
    assert_eq!(
        error("FREEMEM 1 OUT A,B").0,
        ErrorKind::WrongNumberOfArguments
    );
}

#[test]
fn statements_run_together_without_colons() {
    assert_eq!(output("?\"A\" B=1:?B"), "A\n1\n");
    assert_eq!(output("FOR I=1 TO 2:NEXT?I"), "3\n");
    assert_eq!(output("CONST #2P=7:?#2P"), "7\n");
    assert_eq!(output("A=0#:?A;1.5#"), "01.5\n");
    assert_eq!(output("?1\\ XYZ\n+2"), "3\n");
}

#[test]
fn input_guides_may_be_expressions() {
    let run = |s: &str| run_with_input(s, &["ABC"]).unwrap();
    assert_eq!(run("INPUT \"A\"+\"B\";A$:?A$"), "AB? ABC\nABC\n");
    assert_eq!(run("LINPUT@G;A$:?A$\n@G"), "@GABC\nABC\n");
    assert_eq!(run("P$=\"Q\":INPUT P$;A$:?A$"), "Q? ABC\nABC\n");
}

#[test]
fn inc_adds_to_a_string_where_it_is() {
    assert_eq!(
        output("VAR B$=\"Y\":P B$,\"X\":?B$\nDEF P T$,C$\n INC T$,C$\nEND"),
        "YX\n"
    );
    assert_eq!(output("A=1:INC A,2:A$=\"Q\":INC A$,A$:?A;A$"), "3QQ\n");
}

#[test]
fn labels_keep_their_case_as_strings() {
    assert_eq!(output("A$=@Lp:?A$"), "@Lp\n");
    assert_eq!(output("GOTO @lp:?1\n@Lp\n?2"), "2\n");
    assert_eq!(output("GOSUB \"@lP\":END\n@LP\n?3:RETURN"), "3\n");
    assert_eq!(output("RESTORE @dd:READ X:?X\n@DD\nDATA 5"), "5\n");
    assert_eq!(output("ON BREAK GOTO \"\":?4"), "4\n");
}

#[test]
fn arrays_hold_nothing_until_their_dim_runs() {
    assert_eq!(
        output("OPTION STRICT\nGOTO @M\nDIM B[]\nDEF F\n ?TYPEOF(B)\nEND\n@M\nF"),
        "0\n"
    );
    assert_eq!(output("DIM B[2]:?TYPEOF(B)"), "6\n");
    // Empty sizes compile, and are no numbers when the DIM runs.
    assert_eq!(error("?1\nDIM A[,]"), (ErrorKind::TypeMismatch, 2));
}

/// Sizes from TEST51: what fits and what does not, and how the errors read.
#[test]
fn out_of_memory_at_the_sizes_seen_on_hardware() {
    let oom = |source: &str| {
        let (kind, ..) = error(source);
        assert_eq!(kind, ErrorKind::OutOfMemory, "{source}");
    };
    assert_eq!(output("N=33554432:?LEN(\"A\"*N)"), "33554432\n");
    oom("N=67108864:S$=\"A\"*N");
    assert_eq!(
        output("N=16777216:S$=\"A\"*N:S$=S$+S$:?LEN(S$)"),
        "33554432\n"
    );
    oom("N=33554432:S$=\"A\"*N:S$=S$+S$");
    assert_eq!(output("A=ARRAY#(8388608):?LEN(A)"), "8388608\n");
    oom("A=ARRAY#(16777216)");
    assert_eq!(
        output("DIM A%[8000000],B%[8000000],C%[8000000],D%[8000000]:?FREEMEM()"),
        "6216464\n"
    );
    oom("DIM A%[8000000],B%[8000000],C%[8000000],D%[8000000],E%[8000000]");
    assert_eq!(
        output("DIM A%[8388608]:RESIZE A%,16777216:?LEN(A%)"),
        "16777216\n"
    );
    oom("DIM A%[1]:RESIZE A%,33554432");
    oom("DIM A%[1],B%[1]:COPY A%,33554432,B%");
    // Growing needs the new block beside the old, though it would fit
    // alone.
    oom("DIM A%[15728641]:INSERT A%,0,16777216");
    assert_eq!(
        output("DIM A%[7340033]:INSERT A%,0,8388608:?LEN(A%)"),
        "15728641\n"
    );
    for (source, command) in [
        ("A=ARRAY#(16777216)", "ARRAY#"),
        ("DIM A%[1]:RESIZE A%,33554432", "RESIZE"),
        ("DIM A%[15728641]:INSERT A%,0,16777216", "INSERT"),
        ("DIM A%[1],B%[1]:COPY A%,33554432,B%", "COPY"),
    ] {
        let e = run(source).unwrap_err();
        assert_eq!(e.detail.as_deref(), Some(command), "{source}");
    }
    // The error's number, as ENVSTAT gives it.
    assert_eq!(ErrorKind::OutOfMemory.code(), 11);
}

/// Requests over absurd sizes end at once.
#[test]
fn absurd_requests_are_out_of_memory() {
    for source in [
        "?\"A\"*2000000000",
        "N=2000000000:?\"A\"*N",
        "A$=\"X\":FOR I=1 TO 31:A$=A$+A$:NEXT",
        "A$=\"XX\":FOR I=1 TO 40:A$[0]=A$:NEXT",
        "A$=\"XX\":FOR I=1 TO 40:PUSH A$,A$:NEXT",
        "A$=\"XX\":FOR I=1 TO 40:A$=SUBST$(A$,0,0,A$):NEXT",
        "A=ARRAY#(2000000000)",
        "A=ARRAY$(2000000000,2000000000,2000000000,2000000000)",
        "DIM A$[268435455]",
        "DIM A%[2147483647,2147483647]",
        "DIM A[1]:RESIZE A,2000000000",
        "DIM A[1]:INSERT A,0,2000000000",
        "DIM A[1],B[1]:COPY A,2000000000,B",
        "DIM A$[1]:RESIZE A$,200000000",
    ] {
        assert_eq!(error(source).0, ErrorKind::OutOfMemory, "{source}");
    }
}

/// Values dropped since the estimate was last right are not held against a
/// program.
#[test]
fn garbage_causes_no_out_of_memory() {
    assert_eq!(
        output("VAR S$,I,N=16777216:FOR I=1 TO 20:S$=\"\":S$=\"A\"*N:NEXT:?LEN(S$)"),
        "16777216\n"
    );
    assert_eq!(
        output("VAR A,I:FOR I=1 TO 20:A=ARRAY#(1):A=ARRAY#(8388608):NEXT:?LEN(A)"),
        "8388608\n"
    );
    assert_eq!(
        output("VAR I:DIM A%[16000000]:FOR I=1 TO 20:RESIZE A%,1:RESIZE A%,16000000:NEXT:?LEN(A%)"),
        "16000000\n"
    );
    assert_eq!(
        output("DEF F:DIM B%[16000000]:END\nVAR I:FOR I=1 TO 20:F:NEXT:?1"),
        "1\n"
    );
}

/// An instruction that stops to have memory counted runs again exactly as
/// it would have: everything it took off the stack is back.
#[test]
fn counting_memory_leaves_the_instruction_as_it_was() {
    let big = "VAR S$,I,N=16777216:FOR I=1 TO 8:S$=\"\":S$=\"A\"*N:NEXT:S$=\"\"\n";
    let run = |line: &str| output(&format!("{big}{line}"));
    assert_eq!(run("?7+LEN(\"A\"*(N*2))"), "33554439\n");
    assert_eq!(run("T$=\"A\"*N:U$=T$+T$:?1;LEN(U$);2"), "1335544322\n");
    assert_eq!(
        run("T$=\"A\"*N:T$=T$+\"B\"+\"C\":?LEN(T$);RIGHT$(T$,2)"),
        "16777218BC\n"
    );
    assert_eq!(run("A=ARRAY%(1,16000000):?1;DIM(A,1);2"), "1160000002\n");
    assert_eq!(run("DIM Q%[16000000]:?LEN(Q%)"), "16000000\n");
    assert_eq!(run("DIM Q%[1]:RESIZE Q%,16000000:?LEN(Q%)"), "16000000\n");
    assert_eq!(run("A=CALL(\"ARRAY%\",16000000):?LEN(A)"), "16000000\n");
    assert_eq!(
        run("DIM Q%[1],R%[16000000]:COPY Q%,R%:?LEN(Q%)"),
        "16000000\n"
    );
}

/// Filling a string array asks for little at a time, and is refused when
/// the VM next counts.
#[test]
fn what_small_strings_add_up_to_is_out_of_memory() {
    let source = "DIM A$[1000000]:VAR I:FOR I=0 TO 999999:A$[I]=\"X\"*100:NEXT:?I";
    assert_eq!(error(source).0, ErrorKind::OutOfMemory);
    // Up to a hundred thousand strings of a hundred characters (32 MB).
    let source = "DIM A$[100000]:VAR I:FOR I=0 TO 99999:A$[I]=\"X\"*100:NEXT:?I";
    assert_eq!(output(source), "100000\n");
}

/// Constant strings stay in memory for the compiler as a statement compiles,
/// and then in the program's code (TEST57).
/// A program of TEST58 with hardly anything free, from which memory falls
/// short of the block a smaller array needs.
const LITTLE_FREE: &str = "DIM A%[1000001]
DIM F[8000000]
VAR N=((FREEMEM()-1000000-64) DIV 64)*16
DIM G%[N]
";

#[test]
fn pop_down_one_block_and_back_with_little_free() {
    assert_eq!(
        output(&format!(
            "{LITTLE_FREE}?LEN(A%);\" \";FREEMEM()
PUSH A%,1
?LEN(A%);\" \";FREEMEM()
X=POP(A%)
?LEN(A%);\" \";FREEMEM()
X=POP(A%)
?LEN(A%);\" \";FREEMEM()
PUSH A%,1
?LEN(A%);\" \";FREEMEM()"
        )),
        "1000001 1000016\n\
         1000002 1000016\n\
         1000001 1000016\n\
         1000001 1000016\n\
         1000002 1000016\n"
    );
}

#[test]
fn pop_over_and_over_with_little_free_stays() {
    assert_eq!(
        output(&format!(
            "{LITTLE_FREE}FOR I=1 TO 20:X=POP(A%):NEXT
?LEN(A%);\" \";FREEMEM();\" \";X"
        )),
        "1000001 1000016 0\n"
    );
}

/// The VM counts and runs POP again; it must take one element, once.
#[test]
fn pop_that_has_to_count_takes_one_element() {
    assert_eq!(
        output(
            "DIM A%[7500001]
A%[7500000]=7
A%[7499999]=6
FOR I=1 TO 3:X=LEN(ARRAY%(15000000)):NEXT
X=POP(A%)
?X;\" \";LEN(A%)
X=POP(A%)
?X;\" \";LEN(A%)"
        ),
        "7 7500000\n6 7499999\n"
    );
}

#[test]
fn long_constant_strings_stop_the_program_compiling() {
    let refused = |source: &str, kind: ErrorKind| {
        let e = sb_core::compile(source, &TextHost::default()).unwrap_err();
        assert_eq!(e.kind, kind, "{source}");
    };
    assert_eq!(output("?LEN(\"A\"*(1<<20))"), "1048576\n");
    // Its two operands take from the same 4 MiB.
    assert_eq!(output("?LEN(\"A\"*2097116)"), "2097116\n");
    refused("?1\n?LEN(\"A\"*2097117)", ErrorKind::OutOfMemory);
    // TEST58.
    refused("A=LEN(\"A\"*2097140)", ErrorKind::OutOfMemory);
    refused("A=LEN(\"A\"*2097141)", ErrorKind::OutOfMemory);
    // The first line never runs.
    for source in [
        "?1\n?LEN(\"A\"*(1<<21))",
        "?1\n?\"A\"*2097152",
        "IF 0 THEN ?\"A\"*2000000000",
        "?1\nA$=\"A\"*(1<<24)",
        "CONST #A$=\"A\"*(1<<21)",
        // Those worked out on the way stay too.
        "?1\nA$=\"A\"*1000000+\"B\"*1000000",
        "?1\nA$=(\"A\"*1500000)*1",
        "?1\n?\"A\"*1500000,\"B\"*1500000",
    ] {
        refused(source, ErrorKind::OutOfMemory);
    }
    // For the next statement, all of that is free again.
    assert_eq!(
        output("?LEN(\"A\"*1500000)\n?LEN(\"B\"*1500000)"),
        "1500000\n1500000\n"
    );
    // A slot's code holds four million characters.
    assert_eq!(
        output("A=LEN(\"A\"*2000000)\nA=LEN(\"A\"*2000000)\n?A"),
        "2000000\n"
    );
    refused(
        "A=LEN(\"A\"*2000000)\nA=LEN(\"A\"*2000000)\nA=LEN(\"A\"*2000000)",
        ErrorKind::OutOfCodeMemory,
    );
    assert_eq!(ErrorKind::OutOfCodeMemory.code(), 12);
    assert_eq!(ErrorKind::OutOfCodeMemory.message(), "Out of code memory");
    // Through a variable it is the program's to run into, if it gets there.
    assert_eq!(output("N=2000000000:IF 0 THEN ?\"A\"*N\n?1"), "1\n");
    assert_eq!(output("N=1<<21:A$=\"A\"*N:?LEN(A$)"), "2097152\n");
}

/// Direct mode has far less code memory than a slot (TEST57).
#[test]
fn direct_mode_has_little_code_memory() {
    let compile = |line: &str| sb_core::compile_direct(line, &TextHost::default(), 0).map(|_| ());
    compile("?LEN(\"A\"*(1<<14))").unwrap();
    let e = compile("?LEN(\"A\"*(1<<15))").unwrap_err();
    assert_eq!(e.kind, ErrorKind::OutOfCodeMemory);
}

/// PUSH and UNSHIFT need a block's new size beside the old whenever it has
/// to grow, for arrays and strings; within a block they need nothing
/// (TEST57).
#[test]
fn growing_by_one_needs_the_new_block_beside_the_old() {
    let pushed = "DIM A%[15999999],F[8000000]:VAR B=FREEMEM()\nPUSH A%,1\n";
    assert_eq!(
        output(&format!("{pushed}?LEN(A%);\" \";B-FREEMEM();\" \";B")),
        "16000000 0 6216592\n"
    );
    let e = run(&format!("{pushed}PUSH A%,1")).unwrap_err();
    assert_eq!((e.kind, e.line), (ErrorKind::OutOfMemory, 3));
    assert_eq!(e.detail.as_deref(), Some("PUSH"));
    let e = run(&format!("{pushed}UNSHIFT A%,1")).unwrap_err();
    assert_eq!(e.detail.as_deref(), Some("UNSHIFT"));
    // Reals and strings go in eights.
    let filled = |dim: &str, push: &str| {
        run(&format!(
            "DIM A{dim}[8000000],F%[9000000]:VAR I,M=0\nFOR I=1 TO 9:{push}:M=I:NEXT"
        ))
        .unwrap_err()
    };
    for (dim, push) in [("", "PUSH A,1"), ("$", "PUSH A$,\"\"")] {
        let e = filled(dim, push);
        assert_eq!(e.kind, ErrorKind::OutOfMemory, "{push}");
    }
    // A string's block grows each 64 characters.
    let text = "VAR N=40000000:VAR S$=\"A\"*N\nPUSH S$,\"B\"\n";
    let e = run(text).unwrap_err();
    assert_eq!(e.kind, ErrorKind::OutOfMemory);
    assert_eq!(e.detail.as_deref(), Some("PUSH"));
    let text = "VAR N=39999999:VAR S$=\"A\"*N\nPUSH S$,\"B\":?LEN(S$)";
    assert_eq!(output(text), "40000000\n");
    // Growth that fits goes on, and what was dropped does not count.
    assert_eq!(
        output(
            "VAR I:DIM A%[0],B[0],C$[0]\nFOR I=1 TO 1000:PUSH A%,I:UNSHIFT B,I:PUSH C$,\"X\":NEXT\n?LEN(A%);LEN(B);LEN(C$)"
        ),
        "100010001000\n"
    );
}

/// Which argument errors name, as TEST58 saw them.
#[test]
fn errors_name_their_arguments() {
    let wrong = |source: &str, kind: ErrorKind, arg: u8| {
        let e = run(source).unwrap_err();
        assert_eq!((e.kind, e.arg), (kind, Some(arg)), "{source}");
    };
    let too_long = ErrorKind::StringTooLong;
    wrong("VAR F$=\"%.1022F\",X=1.5:?FORMAT$(F$,X)", too_long, 1);
    wrong("VAR F$=\"%1024F\",X=1.5:?FORMAT$(F$,X)", too_long, 1);
    let illegal = ErrorKind::IllegalFunctionCall;
    let mismatch = ErrorKind::TypeMismatch;
    let arrays = "VAR E,X=1:DIM A[4],B[3]:";
    wrong(&format!("{arrays}RINGCOPY A,0,B,-1,1 OUT E"), illegal, 3);
    wrong(&format!("{arrays}RINGCOPY A,0,B,4,0 OUT E"), illegal, 4);
    wrong(&format!("{arrays}RINGCOPY A,0,B,1,3 OUT E"), illegal, 4);
    wrong(&format!("{arrays}RINGCOPY A,0,B,0,-1 OUT E"), illegal, 4);
    wrong(&format!("{arrays}RINGCOPY X,0,B"), mismatch, 0);
    wrong(&format!("{arrays}RINGCOPY A,0,X"), mismatch, 2);
}

/// What RINGCOPY looks at and in what order (TEST57).
#[test]
fn ringcopy_checks_in_order() {
    let wrong = |source: &str, kind: ErrorKind, arg: Option<u8>| {
        let e = run(source).unwrap_err();
        assert_eq!((e.kind, e.arg), (kind, arg), "{source}");
    };
    let mismatch = ErrorKind::TypeMismatch;
    let illegal = ErrorKind::IllegalFunctionCall;
    wrong("S$=\"\":DIM B[3]:RINGCOPY S$,0,B", mismatch, Some(0));
    wrong("S$=\"\":DIM A[3]:RINGCOPY A,0,S$", mismatch, Some(2));
    wrong("DIM A[3],B$[3]:RINGCOPY A,0,B$", mismatch, Some(2));
    wrong("DIM A$[3],B[3]:RINGCOPY A$,0,B", mismatch, Some(2));
    // Before the count, and with nothing to copy.
    wrong("DIM A[2,2,2],B[2]:RINGCOPY A,0,B", mismatch, None);
    wrong("DIM A[2],B[2,2,2]:RINGCOPY A,0,B,0", mismatch, None);
    wrong("DIM A[3],B[3]:RINGCOPY A,0,B,9", illegal, Some(3));
    wrong("DIM A[3],B[3]:RINGCOPY A,0,B,-1", illegal, Some(3));
    wrong("DIM A[3],B[3]:RINGCOPY A,0,B,-1,1", illegal, Some(3));
    wrong("DIM A[3],B[3]:RINGCOPY A,0,B,1,-1", illegal, Some(4));
    wrong("DIM A[3],B[3]:RINGCOPY A,0,B,1,3", illegal, Some(4));
    wrong("DIM A[3],B[3]:RINGCOPY A,0,B,4,0", illegal, Some(4));
    // Nothing to copy, whatever else there is to find.
    for (source, out) in [
        ("DIM A[4],B[3]:RINGCOPY A,2,B,0 OUT E:?E", "0\n"),
        ("DIM A[0],B[3]:RINGCOPY A,2,B OUT E:?E", "0\n"),
        ("DIM A[2,4],B[3,4]:RINGCOPY A,0,B,0:?1", "1\n"),
        ("DIM A$[3],B$[3]:RINGCOPY A$,0,B$,0:?1", "1\n"),
    ] {
        assert_eq!(output(source), out, "{source}");
    }
    wrong("DIM A[2,4],B[3,4]:RINGCOPY A,0,B", mismatch, None);
    wrong("DIM A$[3],B$[3]:RINGCOPY A$,0,B$", mismatch, None);
    // Int and Real mix, and a negative offset comes round from the end.
    assert_eq!(
        output("DIM A[4]:DIM B[]=[1,2,3]:RINGCOPY A,-1,B OUT E:?A[0];A[1];A[2];A[3];E"),
        "23012\n"
    );
    assert_eq!(
        output("DIM A%[4]:DIM B[]=[1,2,3]:RINGCOPY A%,-5,B,1,2 OUT E:?A%[3];A%[0];E"),
        "231\n"
    );
}

/// FOR's test takes integers by a shorter way than other numbers.
#[test]
fn for_counts_the_same_in_integers_and_reals() {
    let count = |head: &str| output(&format!("N=0:FOR I={head}:N=N+1:NEXT:?N"));
    for (head, n) in [
        ("1 TO 3", "3"),
        ("3 TO 1", "0"),
        ("3 TO 1 STEP -1", "3"),
        ("1 TO 3 STEP 0.5", "5"),
        ("1.5 TO 3", "2"),
        ("1 TO 3.5", "3"),
        ("3 TO 1 STEP -0.5", "5"),
        ("2147483646 TO 2147483647", "2"),
    ] {
        assert_eq!(count(head), format!("{n}\n"), "{head}");
    }
}

/// A string constant takes no block until it is stored: POP gives its last
/// character whatever is free.
#[test]
fn pop_of_a_constant_takes_no_block() {
    let got = run(
        "DIM F[8000000]:N=((FREEMEM()-100000-64) DIV 64)*16:DIM G%[N]:\
         M=FREEMEM():X$=POP(\"ABC\"+\"X\"*100027+\"XYZ\"):?X$;M==FREEMEM();",
    );
    assert_eq!(got.unwrap(), "Z1");
    assert_eq!(run("A$=\"PQ\":B$=POP(A$):?B$;LEN(A$);").unwrap(), "Q1");
    // An empty one is an error as it runs, and not before.
    let empty = run("?1:?POP(\"\")").unwrap_err();
    assert_eq!(empty.kind, ErrorKind::SubscriptOutOfRange);
}

/// SHIFT of a string constant needs a copy of it to work on.
#[test]
fn shift_of_a_constant_needs_a_block() {
    let e = run(
        "DIM F[8000000]:N=((FREEMEM()-100000-64) DIV 64)*16:DIM G%[N]:\
         X$=SHIFT(\"ABC\"+\"X\"*100027+\"XYZ\")",
    )
    .unwrap_err();
    assert_eq!(e.kind, ErrorKind::OutOfMemory);
    assert_eq!(e.detail.as_deref(), Some("SHIFT"));
    assert_eq!(
        run("?SHIFT(\"ABC\"+\"X\"*100027+\"XYZ\");POP(\"PQ\");").unwrap(),
        "AQ"
    );
    let empty = run("?SHIFT(\"\"+\"\")").unwrap_err();
    assert_eq!(empty.kind, ErrorKind::SubscriptOutOfRange);
}
