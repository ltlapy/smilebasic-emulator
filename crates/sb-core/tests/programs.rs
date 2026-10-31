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
            Status::Stop { .. } | Status::Traced { .. } => {}
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
    assert_eq!(output("?HEX$(RGBF(2,-1,0.5))"), "FFFF007F\n");
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
