//! Programs run against the runtime, checked through the console contents.

use sb_core::error::ErrorKind;
use sb_core::vm::{Status, Vm};
use sb_runtime::Runtime;
use sb_runtime::font::Font;
use sb_runtime::runtime::keys;

fn run(source: &str) -> Result<Runtime, sb_core::error::Error> {
    let mut rt = Runtime::for_tests(Font::blank());
    let mut vm = Vm::new(sb_core::compile(source, &rt)?);
    while vm.run(&mut rt, 100_000)? != Status::Ended {}
    Ok(rt)
}

fn row(rt: &Runtime, screen: usize, y: u32) -> String {
    let s = &rt.text[screen];
    let codes: Vec<u16> = (0..s.size().0)
        .map(|x| s.cell(x, y).code)
        .take_while(|&c| c != 0)
        .collect();
    String::from_utf16(&codes).unwrap()
}

#[test]
fn print_locate_color() {
    let rt = run("PRINT \"HI\"\nLOCATE 3,2:COLOR #C_RED:?\"X\";\nLOCATE OUT X,Y\n?X;Y").unwrap();
    assert_eq!(row(&rt, 4, 0), "HI");
    assert_eq!(rt.text[4].cell(3, 2).code, u16::from(b'X'));
    assert_eq!(rt.text[4].cell(3, 2).color, 0xFFFF_0000);
    assert_eq!(row(&rt, 4, 2), "");
    assert_eq!(rt.text[4].cell(4, 2).code, u16::from(b'4'));
}

#[test]
fn omitted_coordinates_and_screens() {
    let rt =
        run("LOCATE 5,7:LOCATE ,1:TPRINT 1,\"A\";:COLOR 1,&HFF00FF00\nC=COLOR(1)\n?C==&HFF00FF00")
            .unwrap();
    assert_eq!(rt.text[4].cell(5, 1).code, u16::from(b'1'));
    assert_eq!(row(&rt, 1, 0), "A");
    let rt = run("?\"A\":CLS:?\"B\"").unwrap();
    assert_eq!(row(&rt, 4, 0), "B");
}

#[test]
fn errors() {
    let kind = |src| run(src).err().map(|e| e.kind);
    assert_eq!(kind("LOCATE 50,0"), None);
    assert_eq!(kind("LOCATE 51,0"), Some(ErrorKind::OutOfRange));
    assert_eq!(kind("CLS 5"), Some(ErrorKind::OutOfRange));
    // No form takes that many arguments, as LOCATE and TOFS on hardware.
    assert_eq!(kind("COLOR 1,2,3"), Some(ErrorKind::WrongNumberOfArguments));
    assert_eq!(
        kind("LOCATE 1,2,3,4,5"),
        Some(ErrorKind::WrongNumberOfArguments)
    );
    assert_eq!(kind("COLOR \"A\""), Some(ErrorKind::TypeMismatch));
}

fn type_keys(rt: &mut Runtime, text: &str) {
    text.encode_utf16().for_each(|c| rt.key_press(c));
    rt.take_typed_keys();
}

fn press(rt: &mut Runtime, key: u16) {
    rt.key_press(key);
    rt.take_typed_keys();
}

#[test]
fn inkey_reads_queued_keys() {
    let mut rt = Runtime::for_tests(Font::blank());
    type_keys(&mut rt, "AB");
    press(&mut rt, keys::UP);
    let src = "A$=INKEY$()+INKEY$():B$=INKEY$(1):C$=INKEY$()\n?A$;ASC(B$);LEN(C$)";
    let mut vm = Vm::new(sb_core::compile(src, &rt).unwrap());
    assert_eq!(vm.run(&mut rt, 10_000).unwrap(), Status::Ended);
    // The right operand of + is worked out first, so it reads A.
    assert_eq!(row(&rt, 4, 0), "BA300");
}

/// Keys queued before INPUT, which nothing read, are not typed into its
/// line, PUSHKEY's included.
#[test]
fn input_drops_keys_queued_before_it() {
    let mut rt = Runtime::for_tests(Font::blank());
    type_keys(&mut rt, "ZSX");
    let src = "PUSHKEY \"7\":INPUT A$\n?\"[\";A$;\"]\"";
    let mut vm = Vm::new(sb_core::compile(src, &rt).unwrap());
    // PUSHKEY holds for a frame, and then INPUT waits.
    assert_eq!(vm.run(&mut rt, 10_000).unwrap(), Status::Retry);
    rt.begin_frame();
    assert_eq!(vm.run(&mut rt, 10_000).unwrap(), Status::Retry);
    press(&mut rt, keys::ENTER);
    assert_eq!(vm.run(&mut rt, 10_000).unwrap(), Status::Ended);
    assert_eq!(row(&rt, 4, 1), "[]");
}

/// Keys typed while INPUT waits are typed into its line.
#[test]
fn input_takes_keys_typed_while_it_waits() {
    let mut rt = Runtime::for_tests(Font::blank());
    let src = "INPUT A$\n?\"[\";A$;\"]\"";
    let mut vm = Vm::new(sb_core::compile(src, &rt).unwrap());
    assert_eq!(vm.run(&mut rt, 10_000).unwrap(), Status::Retry);
    type_keys(&mut rt, "8");
    press(&mut rt, keys::ENTER);
    assert_eq!(vm.run(&mut rt, 10_000).unwrap(), Status::Ended);
    assert_eq!(row(&rt, 4, 1), "[8]");
}

#[test]
fn input_waits_for_enter_and_edits() {
    let mut rt = Runtime::for_tests(Font::blank());
    let src = "INPUT \"N\";A\n?A*2";
    let mut vm = Vm::new(sb_core::compile(src, &rt).unwrap());
    let waiting = Status::Retry;
    assert_eq!(vm.run(&mut rt, 10_000).unwrap(), waiting);
    type_keys(&mut rt, "12");
    press(&mut rt, keys::BACKSPACE);
    type_keys(&mut rt, "3");
    assert_eq!(vm.run(&mut rt, 10_000).unwrap(), waiting);
    assert_eq!(row(&rt, 4, 0), "N? 13");
    press(&mut rt, keys::ENTER);
    assert_eq!(vm.run(&mut rt, 10_000).unwrap(), Status::Ended);
    assert_eq!(row(&rt, 4, 1), "26");
}

/// What a program waiting in INPUT shows a frame on.
fn next_frame(rt: &mut Runtime) -> sb_runtime::framebuffer::Framebuffer {
    rt.begin_frame();
    rt.render().clone()
}

#[test]
fn input_cursor_is_a_white_bar_that_blinks() {
    let mut rt = Runtime::for_tests(Font::blank());
    let src = "BACKCOLOR &HFF102030:COLOR &H80FFFF00:INPUT \"N\";A";
    let mut vm = Vm::new(sb_core::compile(src, &rt).unwrap());
    assert_eq!(vm.run(&mut rt, 10_000).unwrap(), Status::Retry);
    // "N? " leaves it in the fourth cell. It is 2 by 8 at the cell's left,
    // white whatever COLOR is, and leaves the rest of the cell alone.
    let at = |fb: &sb_runtime::framebuffer::Framebuffer, x: usize, y: usize| {
        fb.pixels()[y * fb.width() as usize + x]
    };
    const BACK: u32 = 0xFF10_2030;
    let mut shown = Vec::new();
    for _ in 0..60 {
        let fb = next_frame(&mut rt);
        let on = at(&fb, 24, 0) == 0xFFFF_FFFF;
        shown.push(on);
        for y in 0..8 {
            let bar = if on { 0xFFFF_FFFF } else { BACK };
            assert_eq!((at(&fb, 24, y), at(&fb, 25, y)), (bar, bar));
            assert_eq!(at(&fb, 26, y), BACK);
            assert_eq!(at(&fb, 31, y), BACK);
        }
        assert_eq!(at(&fb, 24, 8), BACK);
        assert_eq!(at(&fb, 23, 0), BACK);
    }
    // 20 frames on and 20 off, starting on, the first run a frame short
    // for the frame INPUT came in (this loop starts one frame after it).
    assert!(shown[0]);
    let flips: Vec<usize> = (1..60).filter(|&i| shown[i] != shown[i - 1]).collect();
    assert_eq!(flips, [18, 38, 58]);
}

#[test]
fn input_cursor_starts_again_on_each_key_that_does_something() {
    let mut rt = Runtime::for_tests(Font::blank());
    let mut vm = Vm::new(sb_core::compile("INPUT A$", &rt).unwrap());
    assert_eq!(vm.run(&mut rt, 10_000).unwrap(), Status::Retry);
    let flips_after = |rt: &mut Runtime, from: u32| -> usize {
        rt.begin_frame();
        let bar = |fb: &sb_runtime::framebuffer::Framebuffer, x: u32| {
            fb.pixels()[x as usize] == 0xFFFF_FFFF
        };
        let first = bar(rt.render(), from);
        (1..)
            .find(|_| {
                rt.begin_frame();
                bar(rt.render(), from) != first
            })
            .unwrap()
    };
    // The cursor stands in the first cell after "? ".
    for _ in 0..10 {
        rt.begin_frame();
    }
    type_keys(&mut rt, "7");
    assert_eq!(vm.run(&mut rt, 10_000).unwrap(), Status::Retry);
    assert_eq!(flips_after(&mut rt, 24), 18);
    // Off now. Left to the first character moves, and shows it again.
    press(&mut rt, keys::LEFT);
    assert_eq!(vm.run(&mut rt, 10_000).unwrap(), Status::Retry);
    assert_eq!(flips_after(&mut rt, 16), 18);
    for _ in 0..5 {
        rt.begin_frame();
    }
    // Off, and it stays so: Left at the line's start moves nothing.
    press(&mut rt, keys::LEFT);
    assert_eq!(vm.run(&mut rt, 10_000).unwrap(), Status::Retry);
    assert_eq!(flips_after(&mut rt, 16), 14);
}

#[test]
fn input_cursor_is_a_block_after_insert() {
    let mut rt = Runtime::for_tests(Font::blank());
    let src = "BACKCOLOR &HFF102030:INPUT A$:INPUT B$";
    let mut vm = Vm::new(sb_core::compile(src, &rt).unwrap());
    assert_eq!(vm.run(&mut rt, 10_000).unwrap(), Status::Retry);
    let width = |fb: &sb_runtime::framebuffer::Framebuffer, y: usize| {
        let row = &fb.pixels()[y * fb.width() as usize..];
        (row[16..32].iter()).filter(|&&p| p == 0xFFFF_FFFF).count()
    };
    assert_eq!(width(&next_frame(&mut rt), 0), 2);
    press(&mut rt, keys::INSERT);
    assert_eq!(vm.run(&mut rt, 10_000).unwrap(), Status::Retry);
    assert_eq!(width(&next_frame(&mut rt), 0), 8);
    // The next line goes on overwriting.
    press(&mut rt, keys::ENTER);
    assert_eq!(vm.run(&mut rt, 10_000).unwrap(), Status::Retry);
    assert_eq!(width(&next_frame(&mut rt), 8), 8);
}

#[test]
fn input_cursor_scales_with_the_cell() {
    let mut rt = Runtime::for_tests(Font::blank());
    let src = "BACKCOLOR &HFF102030:TSCREEN 4,16,16:INPUT A$";
    let mut vm = Vm::new(sb_core::compile(src, &rt).unwrap());
    assert_eq!(vm.run(&mut rt, 10_000).unwrap(), Status::Retry);
    let fb = next_frame(&mut rt);
    let lit = |x: usize, y: usize| fb.pixels()[y * fb.width() as usize + x] == 0xFFFF_FFFF;
    assert!(lit(32, 0) && lit(35, 15));
    assert!(!lit(36, 0) && !lit(32, 16));
}

/// On a composed layer the cursor adds its light without covering what is
/// behind, so a layer color dims it and leaves the frame showing.
#[test]
fn input_cursor_adds_its_light_on_a_composed_layer() {
    let mut rt = Runtime::for_tests(Font::blank());
    let src = "BACKCOLOR &HFF102030:LAYER 0,1,&HFF808080:INPUT A$";
    let mut vm = Vm::new(sb_core::compile(src, &rt).unwrap());
    assert_eq!(vm.run(&mut rt, 10_000).unwrap(), Status::Retry);
    let fb = next_frame(&mut rt);
    assert_eq!(fb.pixels()[16], 0xFF90_A0B0);
    assert_eq!(fb.pixels()[18], 0xFF10_2030);
}

#[test]
fn input_cursor_redraws_only_when_it_changes() {
    let mut rt = Runtime::for_tests(Font::blank());
    let mut vm = Vm::new(sb_core::compile("INPUT A", &rt).unwrap());
    assert_eq!(vm.run(&mut rt, 10_000).unwrap(), Status::Retry);
    next_frame(&mut rt);
    let before = rt.revision();
    for _ in 0..60 {
        next_frame(&mut rt);
    }
    assert_eq!(rt.revision() - before, 3);
}

#[test]
fn input_clears_the_cells_it_typed_over_and_deleted() {
    let mut rt = Runtime::for_tests(Font::blank());
    let src = "?\"@@@@@@\";:LOCATE 0,0:INPUT \"\",A$";
    let mut vm = Vm::new(sb_core::compile(src, &rt).unwrap());
    assert_eq!(vm.run(&mut rt, 10_000).unwrap(), Status::Retry);
    type_keys(&mut rt, "XY");
    press(&mut rt, keys::BACKSPACE);
    press(&mut rt, keys::BACKSPACE);
    assert_eq!(vm.run(&mut rt, 10_000).unwrap(), Status::Retry);
    let s = &rt.text[4];
    let codes: Vec<u16> = (0..6).map(|x| s.cell(x, 0).code).collect();
    assert_eq!(codes, [0, 0, 64, 64, 64, 64]);
}

/// TEST64 and TEST67: RESTORE "PUSH" may fill the stack further than a
/// command given arguments or a DEF call can then go, while PRINT and
/// plain expressions still run.
#[test]
fn a_full_stack_stops_the_next_command() {
    const STK: &str = "K=172713:N=5:B=1:C=2\nFOR I=1 TO K:RESTORE \"PUSH\":NEXT\n";
    const DEF: &str = "\n?\"PAST\"\nEND\nDEF F(P)\nRETURN P\nEND";
    for (statement, line) in [
        ("?N", None),
        ("X=B+C", None),
        ("PRINT", None),
        ("A=F(B)", Some(3)),
        ("LOCATE 0,0", Some(3)),
    ] {
        let source = format!("{STK}{statement}{DEF}");
        match (run(&source), line) {
            (Ok(rt), None) => {
                assert!((0..3).any(|y| row(&rt, 4, y) == "PAST"), "{statement}");
            }
            (Err(e), Some(line)) => {
                assert_eq!(
                    (e.kind, e.line),
                    (ErrorKind::StackOverflow, line),
                    "{statement}"
                );
            }
            (r, _) => panic!("{statement}: {:?}", r.err()),
        }
    }
    let e = run("FOR I=1 TO 180000:RESTORE \"PUSH\":NEXT\n?\"PAST\"").err();
    assert_eq!(
        e.map(|e| (e.kind, e.line)),
        Some((ErrorKind::StackOverflow, 1))
    );
    // 172712 leave room for LOCATE.
    assert!(run("FOR I=1 TO 172712:RESTORE \"PUSH\":NEXT\nLOCATE 0,0").is_ok());
}

/// TEST69: after 172713 RESTORE "PUSH", commands without arguments and
/// builtin functions run; a DEF call fails once the pushes leave no room
/// for it, and pushes alone stop on their own line.
#[test]
fn a_full_stack_lets_commands_without_arguments_run() {
    const DEF: &str = "\n?\"PAST\"\nEND\nDEF F(P)\nRETURN P\nEND";
    let pushes = |k: u32| format!("K={k}:B=-3:C=2\nFOR I=1 TO K:RESTORE \"PUSH\":NEXT\n");
    for (k, statement, line) in [
        (172_713, "VSYNC", None),
        (172_713, "CLS", None),
        (172_713, "X=ABS(B)", None),
        (172_713, "LOCATE 0,1", Some(3)),
        (172_000, "A=F(C)", None),
        (172_500, "A=F(C)", None),
        (172_712, "A=F(C)", Some(3)),
    ] {
        let source = format!("{}{statement}{DEF}", pushes(k));
        match (run(&source), line) {
            (Ok(rt), None) => {
                assert!((0..3).any(|y| row(&rt, 4, y) == "PAST"), "{k} {statement}");
            }
            (Err(e), Some(line)) => {
                assert_eq!(
                    (e.kind, e.line),
                    (ErrorKind::StackOverflow, line),
                    "{k} {statement}"
                );
            }
            (r, _) => panic!("{k} {statement}: {:?}", r.err()),
        }
    }
    for k in [400_000, 1_000_000] {
        let e = run(&format!("{}?\"PAST\"", pushes(k))).err();
        assert_eq!(
            e.map(|e| (e.kind, e.line)),
            Some((ErrorKind::StackOverflow, 2)),
            "{k}"
        );
    }
}

/// TEST63, TEST64 and TEST69: a GOSUB that does not fit goes, and the stack
/// overflow comes on the first command given arguments after it: past
/// `D=D+1`, on the `LOCATE`.
#[test]
fn a_gosub_without_room_stops_the_next_command() {
    let deepest = |source: &str| {
        let mut rt = Runtime::for_tests(Font::blank());
        let mut vm = Vm::new(sb_core::compile(source, &rt).unwrap());
        let e = loop {
            match vm.run(&mut rt, 1_000_000) {
                Ok(Status::Ended) => panic!("{source} ended"),
                Ok(_) => {}
                Err(e) => break e,
            }
        };
        (e.kind, e.line, e.col, row(&rt, 4, 1))
    };
    const PUSHES: &str = "FOR I=1 TO 100000:RESTORE \"PUSH\":NEXT\n";
    for (source, line, printed) in [
        (
            format!("{PUSHES}GOSUB @S\nEND\n@S\nD=D+1\nLOCATE 0,1:?D;\nGOSUB @S"),
            6,
            "109068",
        ),
        (
            format!("{PUSHES}GOSUB @S\nEND\n@S\nD=D+1\nLOCATE 0,1:?D;\nON 0 GOSUB @S"),
            6,
            "109068",
        ),
        ("@L\nN=N+1:LOCATE 0,1:?N\nGOSUB @L".to_string(), 2, "259069"),
        (
            format!("{PUSHES}@L\nN=N+1:LOCATE 0,1:?N\nGOSUB @L"),
            3,
            "109069",
        ),
    ] {
        let (kind, at, col, shown) = deepest(&source);
        assert_eq!(
            (kind, at, shown.as_str()),
            (ErrorKind::StackOverflow, line, printed),
            "{source}"
        );
        // The LOCATE's own place, counted from 1.
        let locate = 1 + source
            .lines()
            .nth(line as usize - 1)
            .unwrap()
            .find("LOCATE")
            .unwrap();
        assert_eq!(col as usize, locate, "{source}");
    }
}
