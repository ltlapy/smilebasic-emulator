//! Random numbers, time and the system's settings.

use sb_core::error::ErrorKind;
use sb_core::vm::{Status, Vm};
use sb_runtime::Runtime;
use sb_runtime::font::Font;

fn run(source: &str) -> Result<String, sb_core::error::Error> {
    let mut rt = Runtime::for_tests(Font::blank());
    rt.begin_frame();
    rt.begin_frame();
    let mut vm = Vm::new(sb_core::compile(source, &rt)?);
    loop {
        match vm.run(&mut rt, 1_000_000)? {
            Status::Ended => break,
            Status::Retry => rt.begin_frame(),
            _ => {}
        }
    }
    let console = &rt.text[4];
    let codes: Vec<u16> = (0..console.size().0)
        .map(|x| console.cell(x, 0).code)
        .take_while(|&c| c != 0)
        .collect();
    Ok(String::from_utf16(&codes).unwrap())
}

/// A number where a string goes names the argument, `(CLIPBOARD:1)`.
#[test]
fn a_number_for_a_string_is_numbered() {
    for source in [
        "CLIPBOARD 0",
        "METALOAD 0",
        "?SYSPARAM(0)",
        "SYSPARAM 0,0",
        "DTREAD 0 OUT A,B,C",
        "TMREAD 0 OUT A,B,C",
        "?CHKMML(0)",
        "?LOADV(0)",
        "DIM A[1]:SAVEV 0,A",
    ] {
        let e = run(source).expect_err(source);
        assert_eq!(e.kind, ErrorKind::TypeMismatch, "{source}");
        assert_eq!(e.arg, Some(0), "{source}");
    }
}

#[test]
fn uistate_numbers_stop_at_5() {
    assert!(run("?UISTATE(5)").is_ok());
    assert_eq!(
        run("?UISTATE(6)").err().map(|e| e.kind),
        Some(ErrorKind::OutOfRange)
    );
}

#[test]
fn random_numbers() {
    assert_eq!(
        run("OK=1:FOR I=0 TO 999:R=RND(7):F#=RNDF(3):IF R<0 OR R>6 OR F#<0 OR F#>=1 THEN OK=0\nNEXT:?OK").unwrap(),
        "1"
    );
    assert_eq!(
        run("RANDOMIZE 2,99:A=RND(2,1000):B=RND(2,1000):RANDOMIZE 2,99:C=RND(2,1000):D=RND(2,1000):?C==A AND D==B").unwrap(),
        "1"
    );
    assert_eq!(run("?TYPEOF(RND(5));TYPEOF(RNDF())").unwrap(), "12");
    assert_eq!(run("?RND(0)").unwrap(), "0");
    assert_eq!(run("?RND(-1)").unwrap_err().kind, ErrorKind::OutOfRange);
    assert_eq!(run("?RND(8,5)").unwrap_err().kind, ErrorKind::OutOfRange);
    // SmileBASIC 4's own numbers (TEST12 and TEST13 on hardware). PRINT
    // works its items out last first, so these take them one by one.
    let seq = "RANDOMIZE 0,1:A=RND(&H7FFFFFFF):B=RND(65536):C=RND(100):?A;\" \";B;\" \";C";
    assert_eq!(run(seq).unwrap(), "397858342 58081 33");
    assert_eq!(
        run("RANDOMIZE 3,-1:A=RND(3,&H7FFFFFFF):?A").unwrap(),
        "1579374114"
    );
    // RND(0) takes no number; RNDF takes two.
    assert_eq!(
        run("RANDOMIZE 0,1:A=RND(0):B=RND(&H7FFFFFFF):RANDOMIZE 0,1:C=RNDF():D=RND(&H7FFFFFFF):?B;\" \";C;\" \";D").unwrap(),
        "397858342 0.59263361 1567819186"
    );
}

#[test]
fn counters_and_clock() {
    assert_eq!(run("?MAINCNT()").unwrap(), "2");
    assert_eq!(run("?MILLISEC()>=0").unwrap(), "1");
    assert_eq!(
        run("D$=DATE$():T$=TIME$():?LEN(D$);MID$(D$,4,1);LEN(T$);MID$(T$,2,1)").unwrap(),
        "10/8:"
    );
}

#[test]
fn reading_dates_and_times() {
    assert_eq!(
        run("DTREAD \"2014/10/12\" OUT Y,M,D,W:?Y;M;D;W;:TMREAD \"12:59:31\" OUT H,M,S:?H;M;S")
            .unwrap(),
        "201410120125931"
    );
    assert_eq!(
        run("DTREAD \"2014/02/30\" OUT Y,M,D").unwrap_err().kind,
        ErrorKind::IllegalFunctionCall
    );
    assert_eq!(
        run("TMREAD \"1:00:00\" OUT H,M,S").unwrap_err().kind,
        ErrorKind::IllegalFunctionCall
    );
}

#[test]
fn system_settings() {
    assert_eq!(
        run("SYSPARAM \"KEYREPEAT\" OUT S,I:?SYSPARAM(\"LANG\");S;I;SYSPARAM(\"TABSTEP\")")
            .unwrap(),
        "en-US2031"
    );
    assert_eq!(
        run("A=SYSPARAM(\"MOUSECURSOR\"):SYSPARAM \"MOUSECURSOR\",1:?A;SYSPARAM(\"MOUSECURSOR\")")
            .unwrap(),
        "01"
    );
    assert_eq!(
        run("SYSPARAM \"lang\" OUT L$:?L$;SYSPARAM(\"TOOL2\")").unwrap(),
        "en-US#TOOL/GAHAKU.PRG"
    );
    for (source, error) in [
        ("?SYSPARAM(\"COLOR\")", ErrorKind::IllegalSystemArgumentName),
        ("SYSPARAM \"LANG\",1", ErrorKind::WrongNumberOfOutArguments),
        (
            "?SYSPARAM(\"KEYREPEAT\")",
            ErrorKind::WrongNumberOfOutArguments,
        ),
    ] {
        assert_eq!(run(source).unwrap_err().kind, error, "{source}");
    }
}

#[test]
fn free_memory_and_font_places() {
    assert_eq!(
        run("FREEMEM OUT T,C:?FREEMEM()==T;T==C;FREEMEM()>0").unwrap(),
        "111"
    );
    assert_eq!(run("FONTINFO \"AB\",8 OUT X,Y:?X;\",\";Y").unwrap(), "8,8");
    assert_eq!(run("FONTINFO 65,8 OUT X,Y:?X;\",\";Y").unwrap(), "8,8");
    assert_eq!(
        run("FONTINFO 65,12 OUT X,Y").unwrap_err().kind,
        ErrorKind::OutOfRange
    );
    assert_eq!(
        run("FONTINFO &HE800,8 OUT X,Y").unwrap_err().kind,
        ErrorKind::CantUseUserDefinedCharacter
    );
    assert_eq!(
        run("FONTINFO \"\",8 OUT X,Y").unwrap_err().kind,
        ErrorKind::CantUseEmptyString
    );
}

#[test]
fn what_only_subprograms_use() {
    assert_eq!(
        run("XSUBSCREEN 0,0,400,240:XSUBSCREEN OUT X,Y:?UISTATE(0);X;Y").unwrap(),
        "000"
    );
    // Outside a subprogram nothing is looked at.
    run("XSUBSCREEN 0,0,1281,0:XSUBSCREEN \"A\":XSUBSCREEN 1,2").unwrap();
    assert_eq!(run("PUSHKEY 65622:PUSHKEY 65:?INKEY$()").unwrap(), "A");
}

/// TEST60: keys go to where INKEY$ reads them 32 a frame, and PUSHKEY waits a
/// frame. Both buffers keep the newest 128.
#[test]
fn the_key_buffers() {
    let keys = "VAR I,J,N,F,L,K$,S$\n\
         FOR J=0 TO 4\n\
         S$=\"\":FOR I=0 TO 62:S$=S$+CHR$(&H4E00+J*63+I):NEXT:PUSHKEY S$\n\
         NEXT\n\
         F=-1:REPEAT:K$=INKEY$()\n\
         IF K$!=\"\" THEN N=N+1:L=ASC(K$)-&H4E00:IF F<0 THEN F=L\n\
         UNTIL K$==\"\"\n\
         ?N;\" \";F;\" \";L";
    assert_eq!(run(keys).unwrap(), "128 32 218");
    // INKEY$ skips a code it has no value for, and gives Home's and Tab's.
    assert_eq!(
        run("PUSHKEY &H10013:PUSHKEY &H10009:PUSHKEY &H10008:VSYNC 0:A$=INKEY$():B$=INKEY$():?LEN(A$);LEN(B$);LEN(INKEY$());\" \";ASC(A$);ASC(B$)")
            .unwrap(),
        "110 29"
    );
    assert_eq!(
        run("PUSHKEY \"A\"*64").unwrap_err().kind,
        sb_core::error::ErrorKind::StringTooLong
    );
    assert!(run("PUSHKEY \"A\"*63").is_ok());
    assert_eq!(
        run("PUSHKEY &H11000+1").unwrap_err().kind,
        sb_core::error::ErrorKind::OutOfRange
    );
}
