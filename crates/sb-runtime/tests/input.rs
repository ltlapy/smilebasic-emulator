//! Controller, mouse and keyboard commands, with the input set by hand.

use sb_core::error::ErrorKind;
use sb_core::host::Host;
use sb_core::vm::{Status, Vm};
use sb_runtime::Runtime;
use sb_runtime::font::Font;
use sb_runtime::input::{KeyText, State, alt_key_code};

/// Runs `source` after one frame each of `frames`, returning what it
/// printed on the console's first row.
fn run(frames: &[State], source: &str) -> Result<String, sb_core::error::Error> {
    let mut rt = Runtime::for_tests(Font::blank());
    for state in frames {
        rt.input.live = *state;
        rt.input.latch();
    }
    let mut vm = Vm::new(sb_core::compile(source, &rt)?);
    loop {
        match vm.run(&mut rt, 100_000)? {
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

fn pressed(buttons: u32) -> State {
    let mut s = State::default();
    s.pads[0].buttons = buttons;
    s
}

#[test]
fn buttons() {
    let a = 1 << 3;
    let frames = [pressed(0), pressed(a)];
    assert_eq!(
        run(
            &frames,
            "?BUTTON(0);BUTTON(1,3);BUTTON(1,#B_RANY);BUTTON(2,3);BUTTON(1,3,2)"
        )
        .unwrap(),
        format!("{}1101", a | 1 << 14 | 1 << 16)
    );
    let frames = [pressed(a), pressed(0)];
    assert_eq!(run(&frames, "?BUTTON(1,3);BUTTON(1,3,3)").unwrap(), "01");
}

#[test]
fn bad_ids() {
    let err = |s| run(&[], s).unwrap_err().kind;
    assert_eq!(err("?BUTTON(5)"), ErrorKind::OutOfRange);
    assert_eq!(err("?BUTTON(0,17)"), ErrorKind::OutOfRange);
    assert_eq!(err("?BUTTON(0,0,4)"), ErrorKind::OutOfRange);
    assert_eq!(err("ACCEL 0 OUT X,Y,Z"), ErrorKind::OutOfRange);
    assert_eq!(err("XCTRLSTYLE 1"), ErrorKind::OutOfRange);
    assert_eq!(err("XCTRLSTYLE 1,2"), ErrorKind::OutOfRange);
}

/// XCTRLSTYLE's errors and the argument each names, in SmileBASIC 4's
/// order; PUSHKEY's too long string (TEST60) and key code past &H10FFF.
#[test]
fn control_style_errors_name_the_argument() {
    for (source, kind, arg) in [
        ("XCTRLSTYLE 5,0", ErrorKind::OutOfRange, 0),
        ("XCTRLSTYLE 3,0", ErrorKind::OutOfRange, 0),
        ("XCTRLSTYLE 1,4", ErrorKind::OutOfRange, 1),
        (
            "XCTRLSTYLE 1,0,0,1",
            ErrorKind::IllegalControlStyleForIrCamera,
            1,
        ),
        ("XCTRLSTYLE 1,101,0,0,0,0,0", ErrorKind::OutOfRange, 0),
        ("XCTRLSTYLE 2,101,0,0,0,0,0", ErrorKind::OutOfRange, 1),
        ("XCTRLSTYLE 2,100,11,0,0,0", ErrorKind::OutOfRange, 2),
        ("XCTRLSTYLE 2,100,0,0,0,-1", ErrorKind::OutOfRange, 5),
        ("XCTRLSTYLE 2,100,0,0,0,0", ErrorKind::IllegalToyConType, 5),
        ("XCTRLSTYLE 2,100,1,0,0,0", ErrorKind::IllegalToyConType, 2),
        (
            "XCTRLSTYLE 2,100,10,10,6,5",
            ErrorKind::IllegalToyConType,
            3,
        ),
        ("XCTRLSTYLE 1,100,0,0", ErrorKind::IllegalToyConType, 3),
        ("XCTRLSTYLE 1,100,3,4", ErrorKind::IllegalToyConType, 3),
        ("XCTRLSTYLE 1,100,6,6", ErrorKind::IllegalToyConType, 3),
        ("XCTRLSTYLE 2,100,0,1", ErrorKind::OutOfRange, 0),
        ("PUSHKEY \"A\"*64", ErrorKind::StringTooLong, 0),
        ("PUSHKEY 70000", ErrorKind::OutOfRange, 0),
    ] {
        let e = run(&[], source).unwrap_err();
        assert_eq!((e.kind, e.arg), (kind, Some(arg)), "{source}");
    }
    run(
        &[],
        "XCTRLSTYLE 1,100,6,5:XCTRLSTYLE 1,100,3,3:XCTRLSTYLE 2,100,6,1,0,2",
    )
    .unwrap();
}

/// DIALOG checks each form's arguments in order and names the one that
/// was wrong, the same as SmileBASIC 4's handler does (4.4.8).
#[test]
fn dialog_errors_name_the_argument() {
    for (source, kind, arg) in [
        ("DIALOG 0,0,\"C\",0", ErrorKind::TypeMismatch, 0),
        ("DIALOG \"S\",6", ErrorKind::OutOfRange, 1),
        ("DIALOG \"S\",-1", ErrorKind::OutOfRange, 1),
        ("DIALOG \"S\",0,0", ErrorKind::TypeMismatch, 2),
        ("Q=DIALOG(0,\"S\",1)", ErrorKind::TypeMismatch, 0),
        ("Q=DIALOG(0,\"S\",40)", ErrorKind::TypeMismatch, 0),
        ("Q=DIALOG(\"S\",0,\"C\",0)", ErrorKind::OutOfRange, 1),
        ("Q=DIALOG(\"S\",&H40000)", ErrorKind::OutOfRange, 1),
        ("Q=DIALOG(\"S\",8,0)", ErrorKind::TypeMismatch, 2),
        ("Q$=DIALOG(\"A\",\"X\",33)", ErrorKind::OutOfRange, 2),
        // One OUT variable is the function form.
        ("DIALOG \"S\",0,\"C\",0 OUT Q", ErrorKind::OutOfRange, 1),
    ] {
        let e = run(&[], source).unwrap_err();
        assert_eq!((e.kind, e.arg), (kind, Some(arg)), "{source}");
    }
}

/// IRSTART's settings by mode, as SmileBASIC 4's handler checks them
/// (4.4.8).
#[test]
fn ir_camera_settings_depend_on_the_mode() {
    let style = "XCTRLSTYLE 1,1,0,1:";
    for (source, kind, arg) in [
        (
            "IRSTART 1,0,0,1,7",
            ErrorKind::WrongArgumentsForIrMode,
            None,
        ),
        (
            "IRSTART 1,1,0,1,7",
            ErrorKind::WrongArgumentsForIrMode,
            None,
        ),
        (
            "IRSTART 1,2,0,1,7,0",
            ErrorKind::WrongArgumentsForIrMode,
            None,
        ),
        (
            "IRSTART 1,2,0,1,7,0,0,0",
            ErrorKind::WrongArgumentsForIrMode,
            None,
        ),
        ("IRSTART 1,0,0", ErrorKind::WrongNumberOfArguments, None),
        (
            "IRSTART 1,0,0,1,7,0,0,0,0",
            ErrorKind::WrongNumberOfArguments,
            None,
        ),
        ("IRSTART 1,3", ErrorKind::OutOfRange, Some(1)),
        ("IRSTART 2,1,0,1,7,0", ErrorKind::OutOfRange, Some(0)),
        ("IRSTART 1,2,4,1,7", ErrorKind::OutOfRange, Some(2)),
        ("IRSTART 1,2,0,17,7", ErrorKind::OutOfRange, Some(3)),
        ("IRSTART 1,1,0,1,601,0", ErrorKind::OutOfRange, Some(4)),
        ("IRSTART 1,1,0,1,7,5", ErrorKind::OutOfRange, Some(5)),
        (
            "IRSTART 1,0,0,1,7,76801,0,0",
            ErrorKind::OutOfRange,
            Some(5),
        ),
        ("IRSTART 1,0,0,1,7,0,0,256", ErrorKind::OutOfRange, Some(7)),
    ] {
        let e = run(&[], &format!("{style}{source}")).unwrap_err();
        assert_eq!((e.kind, e.arg.map(usize::from)), (kind, arg), "{source}");
    }
    for source in [
        "IRSTART 1,2,3,16,600",
        "IRSTART 1,1,,,,4",
        "IRSTART 1,0,0,1,7,76800,0,255",
    ] {
        run(&[], &format!("{style}{source}")).expect(source);
    }
}

#[test]
fn repeat_settings() {
    assert_eq!(
        run(&[], "BREPEAT 5,0,3:BREPEAT 5 OUT A,B:?A;B").unwrap(),
        "13"
    );
    assert_eq!(
        run(&[], "BREPEAT 4,20,5:BREPEAT 4 OUT A,B:?A;B").unwrap(),
        "205"
    );
}

#[test]
fn sticks() {
    let mut s = State::default();
    s.pads[1].connected = true;
    s.pads[1].sticks[1] = (0.5, -0.25);
    assert_eq!(
        run(
            &[s],
            "STICK 2,1 OUT X,Y:?X;Y;:STICK 0,1 OUT X,Y:?X;Y;:STICK 1 OUT X,Y:?X;Y"
        )
        .unwrap(),
        "0.5-0.250.5-0.2500"
    );
}

#[test]
fn controllers() {
    let mut s = State::default();
    s.pads[2].connected = true;
    assert_eq!(
        run(
            &[s],
            "?CONTROLLER(0);CONTROLLER(1);CONTROLLER(2);CONTROLLER(3)"
        )
        .unwrap(),
        "2102"
    );
    // TEST63: no colors on a controller that is not connected.
    assert_eq!(run(&[], "CONTROLLER 2 OUT A,B,C:?A;B;C").unwrap(), "000");
    // TEST64, a Pro Controller alone: the default controller has no colors.
    let mut s = State::default();
    s.pads[0].gamepad = true;
    let colors = "FOR I=0 TO 1:CONTROLLER I OUT A,B,C:?A;\" \";HEX$(B);\" \";HEX$(C);\" \";:NEXT";
    assert_eq!(run(&[s], colors).unwrap(), "2 0 0 2 FF232323 FFA0A0A0 ");
    assert_eq!(
        run(
            &[],
            "XCTRLSTYLE 4,3:XCTRLSTYLE OUT A,B:?A;B;RESULT();:XCTRLSTYLE 0:XCTRLSTYLE OUT A,B:?A;B"
        )
        .unwrap(),
        "43110"
    );
}

#[test]
fn mouse_and_touch() {
    let mut s = State::default();
    s.mouse.x = 10;
    s.mouse.y = 20;
    s.mouse.display = (30, 40);
    s.mouse.wheel = -2;
    s.mouse.buttons = 1;
    s.mouse.on_screen = true;
    let frames = [s, s, s];
    assert_eq!(
        run(
            &frames,
            "MOUSE OUT X,Y,W:?X;Y;W;:MOUSE 1 OUT X,Y:?X;Y;MBUTTON();MBUTTON(0,2)"
        )
        .unwrap(),
        "1020-240304010"
    );
    assert_eq!(
        run(&frames, "TOUCH OUT T,X,Y:?T;X;Y;:TOUCH 1 OUT T,X,Y:?T").unwrap(),
        "310200"
    );
    let off = State {
        mouse: sb_runtime::input::Mouse {
            on_screen: false,
            ..s.mouse
        },
        ..s
    };
    assert_eq!(
        run(&[off], "TOUCH 0,0 OUT T,X,Y:?T;:TOUCH 0,1 OUT T,X,Y:?T").unwrap(),
        "01"
    );
    assert_eq!(run(&[s], "MOUSE 5,6:MOUSE OUT X,Y:?X;Y").unwrap(), "56");
    // MOUSE keeps the pointer on the screen, or on the display with flag 1.
    let past = "XSCREEN 640,360:MOUSE -5,400:MOUSE OUT X,Y:?X;Y;:\
                MOUSE 700,-1:MOUSE OUT X,Y:?X;Y;:\
                MOUSE 2000,800,1:MOUSE 1 OUT X,Y:?X;Y";
    assert_eq!(run(&[s], past).unwrap(), "035963901279719");
}

#[test]
fn keyboard() {
    let mut s = State::default();
    s.set_key(4, true);
    s.modifiers = 2;
    assert_eq!(
        run(
            &[State::default(), s],
            "?KEYBOARD(4);KEYBOARD(4,2);KEYBOARD(5);:KEYBOARD 4 OUT R,M:?R;M"
        )
        .unwrap(),
        "11012"
    );
}

/// KEYBOARD 57 is the Caps Lock key itself, down while it is held, not
/// the lock.
#[test]
fn keyboard_reads_the_caps_lock_key_itself() {
    let mut caps = State::default();
    caps.set_key(57, true);
    let up = State::default();
    let read = "?KEYBOARD(57);KEYBOARD(57,2);KEYBOARD(57,1)";
    assert_eq!(run(&[up, caps], read).unwrap(), "110");
    assert_eq!(
        run(&[caps, up], "?KEYBOARD(57);KEYBOARD(57,3)").unwrap(),
        "01"
    );
}

#[test]
fn function_keys_and_pushkey() {
    assert_eq!(
        run(&[], "KEY 2,\"LIST\":?KEY(2);KEY(1);\"|\";").unwrap(),
        "LIST|"
    );
    assert_eq!(
        run(&[], "PUSHKEY \"AB\":A$=INKEY$():?A$;INKEY$()").unwrap(),
        "AB"
    );
    let mut rt = Runtime::for_tests(Font::blank());
    let id = rt.lookup("PUSHKEY").unwrap();
    let long = sb_core::value::Value::Str(sb_core::value::new_str(vec![65; 65]));
    let err = rt
        .call(id, &[long], sb_core::bytecode::Want::Outs(0))
        .unwrap_err();
    assert_eq!(err.kind, ErrorKind::StringTooLong);
}

/// Runs each state of `frames` as a frame, then reads with `read` and
/// prints the code of each key with `?`.
fn typed(frames: &[State], read: &str) -> String {
    let mut rt = Runtime::for_tests(Font::blank());
    for state in frames {
        rt.input.live = *state;
        rt.begin_frame();
    }
    let source = format!("REPEAT:K$={read}:IF K$!=\"\" THEN ?ASC(K$);\" \";\nUNTIL K$==\"\"");
    let mut vm = Vm::new(sb_core::compile(&source, &rt).unwrap());
    while vm.run(&mut rt, 100_000).unwrap() != Status::Ended {}
    let console = &rt.text[4];
    let codes: Vec<u16> = (0..console.size().0)
        .map(|x| console.cell(x, 0).code)
        .take_while(|&c| c != 0)
        .collect();
    String::from_utf16(&codes).unwrap().trim_end().to_string()
}

#[test]
fn controllers_type_keys_for_inkey() {
    // #B_RRIGHT, #B_RLEFT and #B_LUP to #B_LRIGHT, as the manual lists them.
    for (button, code) in [
        (3, "13"),
        (2, "8"),
        (4, "30"),
        (5, "31"),
        (6, "29"),
        (7, "28"),
    ] {
        let down = [State::default(), pressed(1 << button)];
        assert_eq!(typed(&down, "INKEY$()"), code, "button {button}");
        assert_eq!(typed(&down, "INKEY$(0)"), code, "button {button}");
        assert_eq!(typed(&down, "INKEY$(1)"), "", "button {button}");
    }
    // INKEY$ has no character for B, nor for X and the others.
    for button in [0, 1, 8, 9, 10, 11, 12, 13] {
        assert_eq!(typed(&[pressed(1 << button)], "INKEY$()"), "");
    }
    // L or R as Ctrl.
    assert_eq!(typed(&[pressed(1 << 8 | 1 << 6)], "INKEY$()"), "2");
    assert_eq!(typed(&[pressed(1 << 9 | 1 << 7)], "INKEY$()"), "3");
    assert_eq!(typed(&[pressed(1 << 8 | 1 << 4)], "INKEY$()"), "30");
}

#[test]
fn a_controllers_key_repeats_after_20_frames_then_every_3() {
    let down = vec![pressed(1 << 5); 27];
    // At the press, then on frames 20, 23 and 26 after it.
    let counts: Vec<usize> = [1, 20, 21, 23, 24, 26, 27]
        .iter()
        .map(|&n| typed(&down[..n], "INKEY$()").split_whitespace().count())
        .collect();
    assert_eq!(counts, [1, 1, 2, 2, 3, 3, 4]);
    // A press again after a release.
    let again = [pressed(1 << 5), State::default(), pressed(1 << 5)];
    assert_eq!(typed(&again, "INKEY$()"), "31 31");
    // Another button while the first is held types nothing until all are
    // released.
    let both = [pressed(1 << 5), pressed(1 << 5 | 1 << 4), pressed(1 << 5)];
    assert_eq!(typed(&both, "INKEY$()"), "31");
}

#[test]
fn a_controllers_stick_types_keys_for_inkey() {
    let tilted = |x, y| {
        let mut s = State::default();
        s.pads[0].sticks[0] = (x, y);
        s
    };
    assert_eq!(typed(&[tilted(0.0, -0.31)], "INKEY$()"), "17");
    assert_eq!(typed(&[tilted(0.0, 0.31)], "INKEY$()"), "18");
    assert_eq!(typed(&[tilted(-0.31, 0.0)], "INKEY$()"), "2");
    assert_eq!(typed(&[tilted(0.31, 0.0)], "INKEY$()"), "3");
    assert_eq!(typed(&[tilted(0.3, -0.3)], "INKEY$()"), "");
}

#[test]
fn buttons_keys_press_type_nothing_more() {
    let mut s = pressed(1 << 3);
    s.key_pressed_buttons = 1 << 3;
    assert_eq!(typed(&[s], "INKEY$()"), "");
    assert_eq!(run(&[s, s], "?BUTTON(1,3)").unwrap(), "1");
}

#[test]
fn function_keys_start_as_smilebasic_4s() {
    // TEST15 on hardware; a string longer than 256 is ignored.
    let mut m = sb_runtime::machine::Machine::new(Runtime::for_tests(Font::blank()));
    m.boot();
    m.set_slot(
        0,
        "A$=\"\":FOR I=1 TO 257:A$=A$+\"A\":NEXT:KEY 1,A$:?KEY(1);\"|\";KEY(2);\"|\";KEY(4)"
            .to_string(),
    );
    m.run_slot(0);
    for _ in 0..10 {
        m.frame();
    }
    let console = &m.rt.text[4];
    let rows: Vec<String> = (0..console.size().1)
        .map(|y| {
            let codes: Vec<u16> = (0..console.size().0)
                .map(|x| console.cell(x, y).code)
                .take_while(|&c| c != 0)
                .collect();
            String::from_utf16(&codes).unwrap()
        })
        .collect();
    assert!(
        rows.iter().any(|r| r == "FILES|LOAD\"|LIST ERR"),
        "{rows:?}"
    );
}

#[test]
fn hardware_the_emulator_lacks() {
    let ok = |s| run(&[], s).unwrap();
    let err = |s| run(&[], s).unwrap_err().kind;
    // Nothing vibrates, but VIBRATE() reads what was asked (TTEST21).
    assert_eq!(
        ok(
            "VIBRATE 1,40:?VIBRATE(1);VIBRATE(1,0);VIBRATE(1,1);\nVIBRATE 1,440,127,220,64:?VIBRATE(1);:VIBRATE:?VIBRATE(1)"
        ),
        "26214804040-10"
    );
    assert_eq!(ok("VIBRATE 2,1,7:?VIBRATE(2);VIBRATE(2,0)"), "4587520");
    assert_eq!(err("VIBRATE 0,40"), ErrorKind::OutOfRange);
    assert_eq!(err("VIBRATE 1,63"), ErrorKind::OutOfRange);
    // A 0 frequency or one 0 amplitude still vibrates.
    for zero in ["0,100,320,100", "160,0,320,100", "160,100,0,100"] {
        let source = format!("VIBRATE 1,{zero}:?VIBRATE(1)");
        assert_eq!(run(&[], &source).unwrap(), "-1", "{zero}");
    }
    assert_eq!(ok("VIBRATE 1,160,0,320,0:?VIBRATE(1)"), "0");
    // The IR camera runs once allowed, and sees nothing.
    assert_eq!(err("IRSTART 1,0"), ErrorKind::IrCameraNotAvailable);
    assert_eq!(
        ok(
            "XCTRLSTYLE 1,1,0,1:?IRSTATE(1);:IRSTART 1,0:?IRSTATE(1);\nIRREAD 1 OUT S,N,C,D:?S;N;C;DIM(D,1);:IRSTOP 1:?IRSTATE(1)"
        ),
        "0200081"
    );
    assert_eq!(
        ok("XCTRLSTYLE 1,1,0,1:IRSTART 2,2:IRREAD 2 OUT S,N,C,D:?C;DIM(D,0)"),
        "4848"
    );
    // The array of clustering mode is as tall as that of moment mode.
    assert_eq!(
        ok("XCTRLSTYLE 1,1,0,1:IRSTART 1,0:IRREAD 1 OUT S,N,C,D:?C;DIM(D,0)"),
        "048"
    );
    // Toy-Con are set up but never recognized.
    assert_eq!(err("TCPIANO 1 OUT F"), ErrorKind::IllegalControlStyle);
    // SmileBASIC 4 names the controller, in each of the ten.
    for source in [
        "TCPIANO 1 OUT F",
        "TCHOUSE 1 OUT F,A",
        "TCBIKE 1 OUT F",
        "TCFISHING 1 OUT F",
        "TCROBOT 1 OUT F,A",
        "TCVISOR 1 OUT F,A,B,C",
        "TCCAR 1 OUT F",
        "TCPLANE 1 OUT F",
        "TCSUBM 1 OUT F",
        "Q=TCVEHICLE(1)",
    ] {
        let e = run(&[], source).unwrap_err();
        assert_eq!(
            (e.kind, e.arg),
            (ErrorKind::IllegalControlStyle, Some(0)),
            "{source}"
        );
    }
    // TCHOUSE and TCROBOT look for the Toy-Con before the OUT count,
    // TCVISOR after it.
    assert_eq!(err("TCHOUSE 1 OUT F"), ErrorKind::IllegalControlStyle);
    assert_eq!(err("TCROBOT 1 OUT F"), ErrorKind::IllegalControlStyle);
    assert_eq!(err("TCVISOR 1 OUT F"), ErrorKind::WrongNumberOfOutArguments);
    assert_eq!(
        err("XCTRLSTYLE 2,100,6,5,10,10:TCROBOT 1 OUT F"),
        ErrorKind::WrongNumberOfOutArguments
    );
    assert_eq!(
        ok("XCTRLSTYLE 1,100,0,1:TCPIANO 1 OUT F,K,D,V:?F;K;D;V;TCVEHICLE(1)"),
        "00000"
    );
    assert_eq!(
        err("XCTRLSTYLE 1,100,0,1:TCHOUSE 1 OUT F,A"),
        ErrorKind::IllegalControlStyle
    );
    assert_eq!(
        ok("XCTRLSTYLE 2,100,6,5,10,10:TCCAR 2 OUT F,A,W:TCROBOT 1 OUT R,L:?F;R;DIM(L,0)"),
        "004"
    );
    assert_eq!(
        err("XCTRLSTYLE 1,100,0,1:TCPIANO 1 OUT F,K,D,V,X"),
        ErrorKind::WrongNumberOfOutArguments
    );
}

#[test]
fn vibration_patterns_end() {
    // Pattern 57 lasts 3 frames (TTEST24); the input frame comes with the
    // machine frame.
    let mut m = sb_runtime::machine::Machine::new(Runtime::for_tests(Font::blank()));
    m.boot();
    m.set_slot(0, "VSYNC:VIBRATE 1,57:VSYNC:VSYNC:A=VIBRATE(1,0):VSYNC:B=VIBRATE(1,0):VIBRATE 1,40:VSYNC 200:CLS:?A;B;VIBRATE(1,0)".into());
    m.run_slot(0);
    for _ in 0..300 {
        m.frame();
    }
    let console = &m.rt.text[4];
    let codes: Vec<u16> = (0..5).map(|x| console.cell(x, 0).code).collect();
    assert_eq!(String::from_utf16_lossy(&codes), "57040");
}

/// A button held when a program ends types nothing into DIRECT mode, however
/// long it stays down, and a new press does.
#[test]
fn a_button_held_past_the_end_of_a_program_types_nothing() {
    let a = pressed(1 << 3);
    let mut rt = Runtime::for_tests(Font::blank());
    rt.input.live = a;
    rt.input.forget();
    for _ in 0..40 {
        rt.begin_frame();
    }
    let mut vm = Vm::new(sb_core::compile("K$=INKEY$():?LEN(K$)", &rt).unwrap());
    while vm.run(&mut rt, 100_000).unwrap() != Status::Ended {}
    assert_eq!(rt.text[4].cell(0, 0).code, u16::from(b'0'));
}

/// TEST64: the A that answers a DIALOG does not reach INKEY$ after it as
/// Enter, but its repeats do while it stays down, and BUTTON sees it held
/// with no press.
#[test]
fn the_press_that_answers_a_dialog_types_only_its_repeats() {
    let a = pressed(1 << 3);
    let mut rt = Runtime::for_tests(Font::blank());
    let source = "R=DIALOG(\"Q\",&H3FFFF):P=BUTTON(0,-1,2):H=BUTTON(0):N=0:REPEAT:VSYNC:INC N:UNTIL N==22\n\
                  K=0:WHILE INKEY$()==CHR$(13):INC K:WEND:?R;\" \";P;\" \";H;\" \";K";
    let mut vm = Vm::new(sb_core::compile(source, &rt).unwrap());
    let mut frames = 0;
    loop {
        rt.input.live = if frames > 2 { a } else { State::default() };
        rt.begin_frame();
        frames += 1;
        assert!(frames < 200, "the dialog was not answered");
        if vm.run(&mut rt, 100_000).unwrap() == Status::Ended {
            break;
        }
    }
    let row: String = (0..20)
        .map(|x| char::from_u32(u32::from(rt.text[4].cell(x, 0).code)).unwrap_or(' '))
        .collect();
    // The first repeat, 20 frames after the press, and not the next.
    assert_eq!(row.trim_end_matches(['\0', ' ']), "81928 0 81928 1");
}

/// A wait for buttons answers with the #B_RANY and #B_ANY bits that turn on:
/// A with L held gives A and #B_RANY, because L had turned #B_ANY on
/// already, and does not include L.
#[test]
fn a_wait_for_buttons_answers_with_the_any_bits_going_on() {
    let mut rt = Runtime::for_tests(Font::blank());
    let source = "?DIALOG(\"Q\",8)";
    let mut vm = Vm::new(sb_core::compile(source, &rt).unwrap());
    let mut frames = 0;
    loop {
        rt.input.live = pressed(if frames > 2 { 1 << 8 | 1 << 3 } else { 1 << 8 });
        rt.begin_frame();
        frames += 1;
        assert!(frames < 200, "the dialog was not answered");
        if vm.run(&mut rt, 100_000).unwrap() == Status::Ended {
            break;
        }
    }
    let row: String = (0..8)
        .map(|x| char::from_u32(u32::from(rt.text[4].cell(x, 0).code)).unwrap_or(' '))
        .collect();
    assert_eq!(row.trim_end_matches(['\0', ' ']), "16392");
}

/// TEST64: the release of the A that closes an OK dialog is no release for
/// BUTTON after it.
#[test]
fn the_release_that_closes_a_dialog_is_hidden() {
    let a = pressed(1 << 3);
    let mut rt = Runtime::for_tests(Font::blank());
    let source = "DIALOG \"Q\":?BUTTON(0,-1,3)";
    let mut vm = Vm::new(sb_core::compile(source, &rt).unwrap());
    let mut frames = 0;
    loop {
        rt.input.live = if (3..6).contains(&frames) {
            a
        } else {
            State::default()
        };
        rt.begin_frame();
        frames += 1;
        assert!(frames < 200, "the dialog was not answered");
        if vm.run(&mut rt, 100_000).unwrap() == Status::Ended {
            break;
        }
    }
    assert!(frames > 6, "closed before A was released");
    assert_eq!(rt.text[4].cell(0, 0).code, u16::from(b'0'));
}

/// Runs a program that prints each key INKEY$ gives with the frame it came
/// in, counted from the first, through `frames` frames; `at` sets up the
/// keyboard before each frame.
fn keys_by_frame(frames: usize, mut at: impl FnMut(usize, &mut Runtime)) -> String {
    use sb_runtime::machine::Machine;
    let mut m = Machine::new(Runtime::for_tests(Font::blank()));
    m.boot();
    let source = "CLS:F=-1\nWHILE 1\nVSYNC\nK$=INKEY$()\nIF K$!=\"\" && F<0 THEN F=MAINCNT()\n\
                  IF K$!=\"\" THEN ?MAINCNT()-F;K$;\" \";\nWEND\n";
    m.set_slot(0, source.to_string());
    m.run_slot(0);
    m.frame();
    for i in 0..frames {
        at(i, &mut m.rt);
        m.frame();
    }
    assert!(m.is_running());
    let console = &m.rt.text[4];
    let codes: Vec<u16> = (0..console.size().0)
        .map(|x| console.cell(x, 0).code)
        .take_while(|&c| c != 0)
        .collect();
    String::from_utf16(&codes).unwrap().trim_end().to_string()
}

/// Like [`keys_by_frame`], with each key as its code in hexadecimal.
fn codes_by_frame(frames: usize, at: impl FnMut(usize, &mut Runtime)) -> String {
    use sb_runtime::machine::Machine;
    let mut m = Machine::new(Runtime::for_tests(Font::blank()));
    m.boot();
    let source = "CLS:F=-1\nWHILE 1\nVSYNC\nK$=INKEY$()\nIF K$!=\"\" && F<0 THEN F=MAINCNT()\n\
                  IF K$!=\"\" THEN ?MAINCNT()-F;HEX$(ASC(K$));\" \";\nWEND\n";
    m.set_slot(0, source.to_string());
    m.run_slot(0);
    m.frame();
    let mut at = at;
    for i in 0..frames {
        at(i, &mut m.rt);
        m.frame();
    }
    assert!(m.is_running());
    let console = &m.rt.text[4];
    let codes: Vec<u16> = (0..console.size().0 * 2)
        .map(|i| {
            console
                .cell(i % console.size().0, i / console.size().0)
                .code
        })
        .take_while(|&c| c != 0)
        .collect();
    String::from_utf16(&codes).unwrap().trim_end().to_string()
}

const A_KEY: u8 = 4;
const B_KEY: u8 = 5;
const LEFT_SHIFT: u8 = 225;
const RIGHT_SHIFT: u8 = 229;

fn holding(keys: &[u8]) -> State {
    let mut s = State::default();
    for &k in keys {
        s.set_key(k, true);
    }
    s
}

/// TEST68: a key held for a program types at the press, 19 frames later and
/// then every 3, in the program's frames.
#[test]
fn a_held_key_types_again_after_19_frames_then_every_3() {
    let typed = keys_by_frame(40, |i, rt| {
        if i == 5 {
            rt.input.key_down(A_KEY);
            rt.type_held(u16::from(b'A'));
        }
        rt.input.live = if (5..30).contains(&i) {
            holding(&[A_KEY])
        } else {
            State::default()
        };
    });
    assert_eq!(typed, "0A 19A 22A");
}

/// TEST69K: pressing another key takes over the repeat, and the first key,
/// still held, does not repeat again.
#[test]
fn another_key_stops_a_held_keys_repeat() {
    let typed = keys_by_frame(40, |i, rt| {
        match i {
            0 => {
                rt.input.key_down(A_KEY);
                rt.type_held(u16::from(b'A'));
            }
            // B itself types nothing here, like a key whose text has not come.
            23 => rt.input.key_down(B_KEY),
            _ => {}
        }
        rt.input.live = match i {
            0..23 => holding(&[A_KEY]),
            23..26 => holding(&[A_KEY, B_KEY]),
            _ => holding(&[A_KEY]),
        };
    });
    // Repeats at 19 and 22, and none after B was pressed.
    assert_eq!(typed, "0A 19A 22A");
}

const ONE_KEY: u8 = 30;
const LEFT_CTRL: u8 = 224;
const RIGHT_CTRL: u8 = 228;
const LEFT_ALT: u8 = 226;
const RIGHT_ALT: u8 = 230;

/// The text a key types for each modifier, as the frontend gives it:
/// `plain` and `shift`, and with Alt SmileBASIC 4's own character.
fn key_text(usage: u8, plain: u8, shift: u8) -> KeyText {
    let alt = alt_key_code(usage).unwrap_or(0);
    KeyText {
        plain: plain.into(),
        shift: shift.into(),
        alt,
        right_alt: alt,
    }
}

/// Presses `usage` the same way the frontend does: it types `typed`, and its
/// repeats type what `text` gives for the modifiers held at that time.
fn press_key(rt: &mut Runtime, usage: u8, text: KeyText, typed: u16) {
    rt.input.key_down(usage);
    rt.input.key_text(text);
    rt.type_held(typed);
}

/// The A key with SmileBASIC 4's Caps Lock on.
fn a_text() -> KeyText {
    key_text(A_KEY, b'A', b'a')
}

/// TEST69K: pressing Shift while a letter is held holds back its repeat for
/// 19 frames, and then the letter repeats in the other case.
#[test]
fn shift_turns_a_held_letters_repeats_round() {
    let typed = keys_by_frame(52, |i, rt| {
        if i == 0 {
            press_key(rt, A_KEY, a_text(), u16::from(b'A'));
        }
        // Pressing a modifier keeps the repeat going.
        if i == 26 {
            rt.input.key_down(LEFT_SHIFT);
        }
        rt.input.live = match i {
            0..26 => holding(&[A_KEY]),
            _ => holding(&[A_KEY, LEFT_SHIFT]),
        };
    });
    assert_eq!(typed, "0A 19A 22A 25A 45a 48a 51a");
}

/// Releasing Shift does the same the other way, here before the first
/// repeat, and the other Shift counts as well.
#[test]
fn shift_let_go_turns_a_held_letter_back() {
    let typed = keys_by_frame(40, |i, rt| {
        if i == 0 {
            press_key(rt, A_KEY, a_text(), u16::from(b'a'));
        }
        rt.input.live = match i {
            0..10 => holding(&[A_KEY, RIGHT_SHIFT]),
            _ => holding(&[A_KEY]),
        };
    });
    assert_eq!(typed, "0a 29A 32A 35A 38A");
}

/// The repeats of every key follow Shift, not only those of letters: 1
/// held, then Shift, types "!" after 19 frames.
#[test]
fn shift_turns_a_held_digit_into_its_symbol() {
    let typed = keys_by_frame(36, |i, rt| {
        if i == 0 {
            press_key(rt, ONE_KEY, key_text(ONE_KEY, b'1', b'!'), u16::from(b'1'));
        }
        rt.input.live = match i {
            0..10 => holding(&[ONE_KEY]),
            _ => holding(&[ONE_KEY, LEFT_SHIFT]),
        };
    });
    assert_eq!(typed, "01 29! 32! 35!");
}

/// Pressing Ctrl holds the repeat back and types nothing while Ctrl is
/// held. Releasing Ctrl starts the count again, and the key types 19 frames
/// later.
#[test]
fn ctrl_down_and_up_start_the_count_again() {
    let typed = keys_by_frame(60, |i, rt| {
        if i == 0 {
            press_key(rt, A_KEY, a_text(), u16::from(b'A'));
        }
        rt.input.live = match i {
            0..21 => holding(&[A_KEY]),
            21..50 => holding(&[A_KEY, LEFT_CTRL]),
            _ => holding(&[A_KEY]),
        };
    });
    // Repeats at 19, none with Ctrl, then 50+19.
    assert_eq!(typed, "0A 19A");
    let typed = keys_by_frame(60, |i, rt| {
        if i == 0 {
            press_key(rt, A_KEY, a_text(), u16::from(b'A'));
        }
        rt.input.live = match i {
            0..21 => holding(&[A_KEY]),
            21..30 => holding(&[A_KEY, RIGHT_CTRL]),
            _ => holding(&[A_KEY]),
        };
    });
    assert_eq!(typed, "0A 19A 49A 52A 55A 58A");
}

/// A key pressed with Ctrl held types nothing, but if it stays held after
/// Ctrl is released, it types 19 frames later.
#[test]
fn a_key_pressed_with_ctrl_types_once_ctrl_goes_up() {
    let typed = keys_by_frame(40, |i, rt| {
        if i == 0 {
            // The frontend types nothing for it, but says what it gives.
            rt.input.key_down(A_KEY);
            rt.input.key_text(a_text());
            rt.key_press(u16::from(b'X'));
        }
        rt.input.live = match i {
            0..10 => holding(&[A_KEY, LEFT_CTRL]),
            _ => holding(&[A_KEY]),
        };
    });
    assert_eq!(typed, "0X 29A 32A 35A 38A");
}

/// Releasing another key while the held one repeats starts its count again.
#[test]
fn another_key_let_go_starts_the_count_again() {
    let typed = keys_by_frame(50, |i, rt| {
        if i == 0 {
            press_key(rt, A_KEY, a_text(), u16::from(b'A'));
        }
        rt.input.live = match i {
            0..24 => holding(&[A_KEY, B_KEY]),
            _ => holding(&[A_KEY]),
        };
    });
    assert_eq!(typed, "0A 19A 22A 43A 46A 49A");
}

/// Pressing Alt while a key repeats: after 19 frames the key repeats what
/// Alt gives with it, SmileBASIC 4's own character. Both Alt keys work the
/// same way.
#[test]
fn alt_turns_a_held_keys_repeats_into_its_own_character() {
    for alt in [LEFT_ALT, RIGHT_ALT] {
        let typed = keys_by_frame(35, |i, rt| {
            if i == 0 {
                press_key(rt, A_KEY, a_text(), u16::from(b'A'));
            }
            rt.input.live = match i {
                0..10 => holding(&[A_KEY]),
                _ => holding(&[A_KEY, alt, LEFT_SHIFT]),
            };
        });
        assert_eq!(typed, "0A 29\u{E2EC} 32\u{E2EC}");
    }
    // 3 has none, and types nothing with Alt.
    let three = 32;
    let typed = keys_by_frame(35, |i, rt| {
        if i == 0 {
            press_key(rt, three, key_text(three, b'3', b'#'), u16::from(b'3'));
        }
        rt.input.live = match i {
            0..10 => holding(&[three]),
            _ => holding(&[three, LEFT_ALT]),
        };
    });
    assert_eq!(typed, "03");
}

/// Keys such as Enter repeat the code they typed at the press, with Ctrl
/// held too.
#[test]
fn keys_that_type_a_code_repeat_it_with_ctrl() {
    let typed = keys_by_frame(30, |i, rt| {
        if i == 0 {
            rt.input.key_down(ONE_KEY);
            rt.type_held(u16::from(b'1'));
        }
        rt.input.live = holding(&[ONE_KEY, LEFT_CTRL]);
    });
    // Ctrl held from the start does not count as a change.
    assert_eq!(typed, "01 191 221 251 281");
}

/// Enter and Backspace repeat no more while Ctrl is held, and continue once
/// it is released; the arrow keys repeat all along.
#[test]
fn enter_and_backspace_type_nothing_with_ctrl() {
    use sb_runtime::runtime::keys::{BACKSPACE, ENTER, LEFT};
    for (key, code, typed) in [
        (ENTER_KEY, ENTER, "0D 19D 69D 72D"),
        (BACKSPACE_KEY, BACKSPACE, "08 198 698 728"),
        (
            LEFT_ARROW_KEY,
            LEFT,
            "01D 191D 401D 431D 461D 491D 691D 721D",
        ),
    ] {
        let codes = codes_by_frame(75, |i, rt| {
            if i == 0 {
                rt.input.key_down(key);
                rt.type_held(code);
            }
            rt.input.live = match i {
                21..50 => holding(&[key, LEFT_CTRL]),
                _ => holding(&[key]),
            };
        });
        assert_eq!(codes, typed, "{code:X}");
    }
}

/// The table of what Alt types with each key.
#[test]
fn alt_types_smilebasic_4s_own_characters() {
    assert_eq!(alt_key_code(A_KEY), Some(0xE2EC));
    assert_eq!(alt_key_code(25), Some(0x2606), "V");
    assert_eq!(alt_key_code(ONE_KEY), Some(0xE2EB));
    assert_eq!(alt_key_code(31), Some(0xE2EA), "2");
    for digit in 32..=38 {
        assert_eq!(alt_key_code(digit), None, "3 to 9");
    }
    assert_eq!(alt_key_code(39), Some(0xE29C), "0");
    // The keypad's + to 9, 0 and the point.
    assert_eq!(alt_key_code(0x57), Some(0xE28F));
    assert_eq!(alt_key_code(0x61), Some(0xE299));
    assert_eq!(alt_key_code(0x62), Some(0xE295));
    assert_eq!(alt_key_code(0x63), Some(0xE296));
    assert_eq!(alt_key_code(40), None, "Enter");
    assert_eq!(alt_key_code(0), None);
}

/// TEST69K: keys pressed with Ctrl type nothing, nor with the controller's
/// L or R, either of which counts as Ctrl. L and R pressed by keys do not
/// count.
#[test]
fn ctrl_or_l_or_r_keep_keys_from_typing() {
    const L_AND_R: u32 = 0b11 << 8;
    assert!(pressed(L_AND_R).keys_type_nothing());
    assert!(pressed(1 << 8).keys_type_nothing(), "L");
    assert!(pressed(1 << 9).keys_type_nothing(), "R");
    assert!(!pressed(1 << 3).keys_type_nothing());
    assert!(!pressed(1 << 10 | 1 << 11).keys_type_nothing(), "ZL, ZR");
    let mut by_keys = pressed(L_AND_R);
    by_keys.key_pressed_buttons = L_AND_R;
    assert!(!by_keys.keys_type_nothing());
    let ctrl = State {
        modifiers: 1 << 2,
        ..State::default()
    };
    assert!(ctrl.keys_type_nothing());
    // Either Ctrl key, while it is held.
    assert!(holding(&[LEFT_CTRL]).keys_type_nothing());
    assert!(holding(&[RIGHT_CTRL]).keys_type_nothing());
    let shift = State {
        modifiers: 1 << 1,
        ..State::default()
    };
    assert!(!shift.keys_type_nothing());
}

/// L or R held while a key repeats keeps the repeats from typing, without
/// a press or release of a keyboard key to start the count again.
#[test]
fn l_held_keeps_a_held_key_from_repeating() {
    let typed = keys_by_frame(40, |i, rt| {
        if i == 0 {
            press_key(rt, A_KEY, a_text(), u16::from(b'A'));
        }
        let mut state = holding(&[A_KEY]);
        if (21..30).contains(&i) {
            state.pads[0].buttons = 1 << 8;
        }
        rt.input.live = state;
    });
    assert_eq!(typed, "0A 19A 31A 34A 37A");
}

/// Released before 19 frames, a key types once, and pressing it again
/// starts its count over.
#[test]
fn a_key_let_go_types_once() {
    let typed = keys_by_frame(40, |i, rt| {
        if i == 0 || i == 10 {
            rt.input.key_down(A_KEY);
            rt.type_held(u16::from(b'A'));
        }
        rt.input.live = if (0..8).contains(&i) || (10..30).contains(&i) {
            holding(&[A_KEY])
        } else {
            State::default()
        };
    });
    assert_eq!(typed, "0A 10A 29A");
}

/// A key held when a program ends continues repeating in DIRECT mode.
#[test]
fn a_key_held_when_a_program_ends_repeats_in_direct_mode() {
    use sb_runtime::machine::Machine;
    let mut m = Machine::new(Runtime::for_tests(Font::blank()));
    m.boot();
    m.set_slot(0, "WHILE INKEY$()==\"\":VSYNC:WEND\n".to_string());
    m.run_slot(0);
    m.frame();
    m.rt.input.key_down(A_KEY);
    m.rt.type_held(u16::from(b'A'));
    m.rt.input.live = holding(&[A_KEY]);
    for _ in 0..5 {
        m.frame();
    }
    assert!(!m.is_running());
    let screen = |m: &Machine| {
        let console = &m.rt.text[4];
        let (w, h) = console.size();
        let cells = (0..h).flat_map(|y| (0..w).map(move |x| (x, y)));
        cells
            .map(|(x, y)| console.cell(x, y).code)
            .collect::<Vec<_>>()
    };
    let a_cells = |m: &Machine| screen(m).iter().filter(|&&c| c == u16::from(b'A')).count();
    let ended = a_cells(&m);
    for _ in 0..40 {
        m.frame();
    }
    // The A key types into DIRECT mode's line.
    assert!(a_cells(&m) > ended);
}

const BACKSPACE_KEY: u8 = 42;
const LEFT_ARROW_KEY: u8 = 80;
const ENTER_KEY: u8 = 40;

/// Frames, counted from the first, on which the editor's text changed;
/// `at` sets up the keyboard before each frame.
fn editor_changes(
    m: &mut sb_runtime::machine::Machine,
    frames: usize,
    mut at: impl FnMut(usize, &mut Runtime),
) -> Vec<usize> {
    let mut text = m.editor().text();
    let mut changed = Vec::new();
    for i in 0..frames {
        at(i, &mut m.rt);
        m.frame();
        let now = m.editor().text();
        if now != text {
            changed.push(i);
            text = now;
        }
    }
    changed
}

fn editing() -> sb_runtime::machine::Machine {
    use sb_runtime::machine::Machine;
    let mut m = Machine::new(Runtime::for_tests(Font::blank()));
    m.boot();
    m.toggle_editor();
    m.frame();
    m
}

/// The console's rows that have text.
fn console_rows(m: &sb_runtime::machine::Machine) -> Vec<String> {
    let console = &m.rt.text[4];
    let (w, h) = console.size();
    (0..h)
        .map(|y| {
            let codes: Vec<u16> = (0..w)
                .map(|x| console.cell(x, y).code)
                .take_while(|&c| c != 0)
                .collect();
            String::from_utf16(&codes).unwrap()
        })
        .filter(|r| !r.is_empty())
        .collect()
}

/// A key held in the editor types the same as it does for a program: at
/// the press, 19 frames later and then every 3, in the machine's frames,
/// though programs see nothing held while the editor has the keys.
#[test]
fn a_key_held_in_the_editor_repeats_as_for_programs() {
    let mut m = editing();
    let changed = editor_changes(&mut m, 30, |i, rt| {
        if i == 0 {
            rt.input.key_down(A_KEY);
            rt.type_held(u16::from(b'A'));
        }
        rt.input.editor_held = if i < 26 {
            holding(&[A_KEY])
        } else {
            State::default()
        };
    });
    assert_eq!(changed, [0, 19, 22, 25]);
    assert_eq!(m.editor().text(), "AAAA");
    // Released, it types no more.
    let changed = editor_changes(&mut m, 30, |_, _| {});
    assert!(changed.is_empty());
}

/// Repeats in the editor follow the modifiers, the same as repeats in a
/// program do.
#[test]
fn a_key_held_in_the_editor_follows_shift_and_l() {
    let mut m = editing();
    let changed = editor_changes(&mut m, 45, |i, rt| {
        if i == 0 {
            press_key(rt, ONE_KEY, key_text(ONE_KEY, b'1', b'!'), u16::from(b'1'));
        }
        let mut held = match i {
            0..10 => holding(&[ONE_KEY]),
            _ => holding(&[ONE_KEY, LEFT_SHIFT]),
        };
        // L blocks 32, 35 and 38.
        if (30..40).contains(&i) {
            held.pads[0].buttons = 1 << 8;
        }
        rt.input.editor_held = held;
    });
    assert_eq!(changed, [0, 29, 41, 44]);
    assert_eq!(m.editor().text(), "1!!!");
}

/// Backspace and the other keys that edit repeat the same way.
#[test]
fn an_editing_key_held_in_the_editor_repeats_as_for_programs() {
    use sb_runtime::runtime::keys;
    let mut m = editing();
    m.edit().paste("ABCDEFGH");
    let changed = editor_changes(&mut m, 23, |i, rt| {
        if i == 0 {
            rt.input.key_down(BACKSPACE_KEY);
            rt.type_held(keys::BACKSPACE);
        }
        rt.input.editor_held = holding(&[BACKSPACE_KEY]);
    });
    assert_eq!(changed, [0, 19, 22]);
    assert_eq!(m.editor().text(), "ABCDE");
}

/// A key held in the editor stops repeating once the editor no longer has
/// the keys, rather than typing into DIRECT mode.
#[test]
fn a_key_held_in_the_editor_stops_when_it_closes() {
    let mut m = editing();
    m.rt.input.key_down(A_KEY);
    m.rt.type_held(u16::from(b'A'));
    m.rt.input.editor_held = holding(&[A_KEY]);
    m.frame();
    assert_eq!(m.editor().text(), "A");
    m.toggle_editor();
    m.rt.input.editor_held = State::default();
    m.rt.input.live = holding(&[A_KEY]);
    for _ in 0..30 {
        m.frame();
    }
    let rows = console_rows(&m);
    assert!(!rows.iter().any(|r| r.contains('A')), "{rows:?}");
}

/// DIRECT mode repeats a held key in frames too, and the Enter that runs a
/// program continues repeating into it, 19 frames after the press and then
/// every 3.
#[test]
fn a_key_held_at_the_prompt_repeats_into_the_program_it_runs() {
    use sb_runtime::machine::Machine;
    use sb_runtime::runtime::keys;
    let mut m = Machine::new(Runtime::for_tests(Font::blank()));
    m.boot();
    // In the form TEST70K logs them: the frame, counted from the start of
    // the program, in which INKEY$ gives Enter.
    let source = "T=MAINCNT()\nWHILE 1\nK$=INKEY$()\n\
                  IF K$==CHR$(13) THEN ?\"K\";MAINCNT()-T ELSEIF K$!=\"\" THEN ?K$;\n\
                  VSYNC\nWEND\n";
    m.set_slot(0, source.to_string());
    m.frame();
    m.rt.input.key_down(A_KEY);
    m.rt.type_held(u16::from(b'A'));
    m.rt.input.live = holding(&[A_KEY]);
    for _ in 0..23 {
        m.frame();
    }
    m.rt.input.live = State::default();
    m.frame();
    // At the press, and again 19 and 22 frames later.
    assert_eq!(console_rows(&m).last().map(String::as_str), Some("AAA"));
    for _ in 0..3 {
        m.rt.key_press(keys::BACKSPACE);
    }
    m.type_text("RUN");
    m.rt.input.key_down(ENTER_KEY);
    m.rt.type_held(keys::ENTER);
    m.rt.input.live = holding(&[ENTER_KEY]);
    for _ in 0..30 {
        m.frame();
    }
    assert!(m.is_running());
    // DIRECT mode takes the line in the frame of the press, and RUN runs in
    // the next and starts the program in it: 19 frames after the press is
    // the program's frame 18, as on SmileBASIC 4.
    let rows = console_rows(&m);
    let at: Vec<&str> = rows.iter().filter_map(|r| r.strip_prefix('K')).collect();
    assert_eq!(at, ["18", "21", "24", "27"], "{rows:?}");
}

/// Runs `source`, which opens a dialog, with `input` giving what is held
/// in each frame, counted from 0, and the dialog's focus as each frame
/// ends. Returns the first row printed and the focus by frame.
fn dialog_frames(source: &str, input: impl Fn(usize) -> State) -> (String, Vec<Option<usize>>) {
    let mut rt = Runtime::for_tests(Font::blank());
    let mut vm = Vm::new(sb_core::compile(source, &rt).unwrap());
    let mut focus = Vec::new();
    for frame in 0.. {
        rt.input.live = input(frame);
        rt.begin_frame();
        assert!(frame < 200, "the dialog was not answered");
        let ended = vm.run(&mut rt, 100_000).unwrap() == Status::Ended;
        focus.push(rt.dialog_focus());
        if ended {
            break;
        }
    }
    let row: String = (0..20)
        .map(|x| char::from_u32(u32::from(rt.text[4].cell(x, 0).code)).unwrap_or(' '))
        .collect();
    (row.trim_end_matches(['\0', ' ']).to_string(), focus)
}

/// Holds the buttons of `at` in the frames given with them.
fn pad_at(frame: usize, at: &[(std::ops::Range<usize>, u32)]) -> State {
    let held = at.iter().filter(|(r, _)| r.contains(&frame));
    pressed(held.fold(0, |bits, (_, b)| bits | b))
}

const LEFT: u32 = 1 << 6;
const RIGHT: u32 = 1 << 7;
const A_BUTTON: u32 = 1 << 3;
const B_BUTTON: u32 = 1 << 1;

/// The focus starts on the right button of two, and A chooses it.
#[test]
fn a_chooses_the_right_button_at_first() {
    let (row, focus) = dialog_frames("DIALOG \"Q\",1:?RESULT()", |f| {
        pad_at(f, &[(3..5, A_BUTTON)])
    });
    assert_eq!(row, "1");
    assert_eq!(focus[1], Some(1));
}

/// The D-pad's Left and Right move the focus, one button per press and no
/// further than the ends, and A chooses the button with the focus.
#[test]
fn the_d_pad_moves_the_focus_that_a_chooses() {
    let presses = [
        (2..4, LEFT),
        (5..7, LEFT),
        (8..10, RIGHT),
        (11..13, LEFT),
        (14..16, A_BUTTON),
    ];
    let (row, focus) = dialog_frames("DIALOG \"Q\",3:?RESULT()", |f| pad_at(f, &presses));
    assert_eq!(row, "-1");
    let at = |f: usize| focus[f];
    assert_eq!(
        (at(1), at(2), at(5), at(8), at(11)),
        (Some(1), Some(0), Some(0), Some(1), Some(0))
    );
    // Held, Left moves no more.
    assert_eq!(at(3), Some(0));
}

/// B chooses the left button of two, regardless of the focus.
#[test]
fn b_chooses_the_left_button_whatever_the_focus() {
    let source = "DIALOG \"Q\",2:?RESULT()";
    let (row, _) = dialog_frames(source, |f| pad_at(f, &[(3..5, B_BUTTON)]));
    assert_eq!(row, "-1");
    let (row, _) = dialog_frames(source, |f| pad_at(f, &[(2..3, LEFT), (5..7, B_BUTTON)]));
    assert_eq!(row, "-1");
}

/// The left stick moves the focus once each time it is pushed past halfway,
/// and not at all if it was already past halfway when the dialog opened.
#[test]
fn the_left_stick_moves_the_focus() {
    let stick = |x: f64| {
        let mut s = State::default();
        s.pads[0].sticks[0] = (x, 0.0);
        s
    };
    let source = "DIALOG \"Q\",4:?RESULT()";
    let (row, focus) = dialog_frames(source, |f| match f {
        3 => stick(-0.4),
        4..=6 => stick(-0.8),
        8..=9 => stick(0.9),
        10..=11 => stick(-0.6),
        13..=14 => pad_at(f, &[(13..15, A_BUTTON)]),
        _ => State::default(),
    });
    assert_eq!(row, "-1");
    assert_eq!(
        (focus[3], focus[4], focus[8], focus[10]),
        (Some(1), Some(0), Some(1), Some(0))
    );

    // Pushed to the left from before the dialog opened, it moves nothing.
    let (row, _) = dialog_frames(source, |f| match f {
        0..=9 => stick(-1.0),
        10..=11 => pad_at(f, &[(10..12, A_BUTTON)]),
        _ => State::default(),
    });
    assert_eq!(row, "1");
}

/// A dialog with one button keeps the focus on it, and one that waits for
/// buttons has none.
#[test]
fn only_a_choice_of_two_moves_its_focus() {
    let (row, focus) = dialog_frames("DIALOG \"Q\",5:?RESULT()", |f| {
        pad_at(f, &[(2..3, LEFT), (4..5, RIGHT), (6..8, A_BUTTON)])
    });
    assert_eq!(row, "1");
    let open = &focus[..focus.len() - 1];
    assert!(open.iter().all(|&f| f == Some(0)), "{focus:?}");
    let (_, focus) = dialog_frames("?DIALOG(\"Q\",8)", |f| {
        pad_at(f, &[(2..3, LEFT), (4..6, A_BUTTON)])
    });
    assert!(focus.iter().all(Option::is_none), "{focus:?}");
}

/// The frontend's keys move the same focus, no further than the last
/// button.
#[test]
fn the_frontend_sets_the_focus() {
    let mut rt = Runtime::for_tests(Font::blank());
    let mut vm = Vm::new(sb_core::compile("DIALOG \"Q\",1:?RESULT()", &rt).unwrap());
    vm.run(&mut rt, 100_000).unwrap();
    assert_eq!(rt.dialog_focus(), Some(1));
    rt.set_dialog_focus(0);
    assert_eq!(rt.dialog_focus(), Some(0));
    rt.set_dialog_focus(5);
    assert_eq!(rt.dialog_focus(), Some(1));
}

/// The D-pad presses that move the focus are no presses for BUTTON or
/// INKEY$ after the dialog closes.
#[test]
fn moving_the_focus_leaves_nothing_for_the_program() {
    let source = "DIALOG \"Q\",1:P=BUTTON(0,-1,2):N=0:REPEAT:VSYNC:INC N:UNTIL N==30\n\
                  K$=INKEY$():?RESULT();\" \";P;\" \";LEN(K$)";
    // Left is still held as A closes the dialog, and released after it.
    let (row, _) = dialog_frames(source, |f| pad_at(f, &[(2..8, LEFT), (5..7, A_BUTTON)]));
    assert_eq!(row, "-1 0 0");
}
