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
    while vm.run(&mut rt, 100_000)? != Status::Ended {}
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
    assert_eq!(run(&[], "PUSHKEY \"AB\":?INKEY$();INKEY$()").unwrap(), "AB");
    let mut rt = Runtime::new(400, 240, Font::blank());
    let id = rt.lookup("KEY").unwrap();
    let long = sb_core::value::Value::Str(sb_core::value::new_str(vec![65; 257]));
    let err = rt
        .call(
            id,
            &[sb_core::value::Value::Int(1), long],
            sb_core::bytecode::Want::Outs(0),
        )
        .unwrap_err();
    assert_eq!(err.kind, ErrorKind::StringTooLong);
}
