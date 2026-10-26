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
            Status::Running | Status::Wait { .. } => {}
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
}

#[test]
fn arithmetic_and_types() {
    assert_eq!(output("?7/2;\" \";7 DIV 2;\" \";7 MOD 2"), "3.5 3 1\n");
    assert_eq!(output("A%=2147483647:B=A%+1:?B"), "2147483648\n");
    assert_eq!(output("A%=3.9:?A%"), "3\n");
    assert_eq!(output("?\"AB\"*3+\"!\""), "ABABAB!\n");
    assert_eq!(output("?1==1, 1<0"), "1   0\n");
    assert_eq!(error("A$=1"), (ErrorKind::TypeMismatch, 1));
    assert_eq!(error("\n?1/0"), (ErrorKind::DivideByZero, 2));
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
    assert_eq!(error("?1\nFOO 1").0, ErrorKind::UndefinedFunction);
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
        "NAME AND AGE?BOB,41\nBOB42\n"
    );
    // Missing values and non-numbers ask again.
    assert_eq!(
        run_with_input("INPUT A,B\n?A*B", &["3", "X,2", "3,&H10"]).unwrap(),
        "?3\n?Redo from start\n?X,2\n?Redo from start\n?3,&H10\n48\n"
    );
    assert_eq!(
        run_with_input("INPUT \"N\",N%:?N%", &["7.9"]).unwrap(),
        "N7.9\n7\n"
    );
    assert_eq!(
        run_with_input("LINPUT \"ADDR:\";L$:?LEN(L$)", &["1, MAIN ST"]).unwrap(),
        "ADDR:1, MAIN ST\n10\n"
    );
    assert_eq!(
        run_with_input("VAR A$[2]:INPUT A$[1]:?A$[1]", &["HI"]).unwrap(),
        "?HI\nHI\n"
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
        "FFFF8000 80FFFFFF FFFF0000\n"
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

    fn callbacks(&mut self, kind: sb_core::bytecode::Callbacks) -> Vec<(i32, Vec<u16>)> {
        let list: &[(i32, &str)] = match kind {
            sb_core::bytecode::Callbacks::Sprite => &[(5, "@A"), (7, "B")],
            sb_core::bytecode::Callbacks::Text => &[(2, "@T")],
        };
        list.iter()
            .map(|&(i, t)| (i, t.encode_utf16().collect()))
            .collect()
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
