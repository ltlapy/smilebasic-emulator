//! Controller, mouse and keyboard commands, with the input set by hand.

use sb_core::error::ErrorKind;
use sb_core::host::Host;
use sb_core::vm::{Status, Vm};
use sb_runtime::Runtime;
use sb_runtime::font::Font;
use sb_runtime::input::State;

/// Runs `source` after one frame each of `frames`, returning what it
/// printed on the console's first row.
fn run(frames: &[State], source: &str) -> Result<String, sb_core::error::Error> {
    let mut rt = Runtime::new(400, 240, Font::blank());
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
        "1102"
    );
    // TEST63: no colors on one not connected.
    assert_eq!(run(&[], "CONTROLLER 2 OUT A,B,C:?A;B;C").unwrap(), "000");
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
        "1020-2304010"
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
    let mut rt = Runtime::new(400, 240, Font::blank());
    let id = rt.lookup("PUSHKEY").unwrap();
    let long = sb_core::value::Value::Str(sb_core::value::new_str(vec![65; 65]));
    let err = rt
        .call(id, &[long], sb_core::bytecode::Want::Outs(0))
        .unwrap_err();
    assert_eq!(err.kind, ErrorKind::StringTooLong);
}

/// Goes through `frames` as frames do, then reads what INKEY$ has for it:
/// `?` each key's code.
fn typed(frames: &[State], read: &str) -> String {
    let mut rt = Runtime::new(400, 240, Font::blank());
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
    // B's key is none INKEY$ has a character for, as are X's and the others.
    for button in [0, 1, 8, 9, 10, 11, 12, 13] {
        assert_eq!(typed(&[pressed(1 << button)], "INKEY$()"), "");
    }
    // L or R as Ctrl.
    assert_eq!(typed(&[pressed(1 << 8 | 1 << 6)], "INKEY$()"), "2");
    assert_eq!(typed(&[pressed(1 << 9 | 1 << 7)], "INKEY$()"), "3");
    assert_eq!(typed(&[pressed(1 << 8 | 1 << 4)], "INKEY$()"), "30");
}

#[test]
fn a_controllers_key_repeats_after_25_frames_then_every_3() {
    let down = vec![pressed(1 << 5); 32];
    // At the press, then on frames 25, 28 and 31 after it.
    let counts: Vec<usize> = [1, 25, 26, 28, 29, 31, 32]
        .iter()
        .map(|&n| typed(&down[..n], "INKEY$()").split_whitespace().count())
        .collect();
    assert_eq!(counts, [1, 1, 2, 2, 3, 3, 4]);
    // A press again after letting go.
    let again = [pressed(1 << 5), State::default(), pressed(1 << 5)];
    assert_eq!(typed(&again, "INKEY$()"), "31 31");
    // Another button while the first is held types nothing until all are up.
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
    let mut m = sb_runtime::machine::Machine::new(Runtime::new(400, 240, Font::blank()));
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
    // Toy-Con are set up but never recognized.
    assert_eq!(err("TCPIANO 1 OUT F"), ErrorKind::IllegalControlStyle);
    assert_eq!(
        ok("XCTRLSTYLE 1,100,0,1:TCPIANO 1 OUT F,K,D,V:?F;K;D;V;TCVEHICLE(1)"),
        "00000"
    );
    assert_eq!(
        err("XCTRLSTYLE 1,100,0,1:TCHOUSE 1 OUT F,A"),
        ErrorKind::IllegalControlStyle
    );
    assert_eq!(
        ok("XCTRLSTYLE 2,100,10,10,6,5:TCCAR 1 OUT F,A,W:TCROBOT 2 OUT R,L:?F;R;DIM(L,0)"),
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
    // machine's.
    let mut m = sb_runtime::machine::Machine::new(Runtime::new(400, 240, Font::blank()));
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
