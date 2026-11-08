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
        "2102"
    );
    // TEST63: no colors on one not connected.
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
fn a_controllers_key_repeats_after_20_frames_then_every_3() {
    let down = vec![pressed(1 << 5); 27];
    // At the press, then on frames 20, 23 and 26 after it.
    let counts: Vec<usize> = [1, 20, 21, 23, 24, 26, 27]
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

/// A button held when a program ends types nothing into direct mode, however
/// long it stays down, and a fresh press does.
#[test]
fn a_button_held_past_the_end_of_a_program_types_nothing() {
    let a = pressed(1 << 3);
    let mut rt = Runtime::new(400, 240, Font::blank());
    rt.input.live = a;
    rt.input.forget();
    for _ in 0..40 {
        rt.begin_frame();
    }
    let mut vm = Vm::new(sb_core::compile("K$=INKEY$():?LEN(K$)", &rt).unwrap());
    while vm.run(&mut rt, 100_000).unwrap() != Status::Ended {}
    assert_eq!(rt.text[4].cell(0, 0).code, u16::from(b'0'));
}

/// TEST64: the A that answers a DIALOG doesn't reach INKEY$ after it as
/// Enter, but its repeats do while it stays down, and BUTTON sees it held
/// with no press.
#[test]
fn the_press_that_answers_a_dialog_types_only_its_repeats() {
    let a = pressed(1 << 3);
    let mut rt = Runtime::new(400, 240, Font::blank());
    let source = "R=DIALOG(\"Q\",-1):P=BUTTON(0,-1,2):H=BUTTON(0):N=0:REPEAT:VSYNC:INC N:UNTIL N==22\n\
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

/// TEST64: the release of the A that closes an OK dialog is no release for
/// BUTTON after it.
#[test]
fn the_release_that_closes_a_dialog_is_hidden() {
    let a = pressed(1 << 3);
    let mut rt = Runtime::new(400, 240, Font::blank());
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
    assert!(frames > 6, "closed before A was let go");
    assert_eq!(rt.text[4].cell(0, 0).code, u16::from(b'0'));
}

/// Runs a program that prints each key INKEY$ gives with the frame it came
/// in, counted from the first, through `frames` frames; `at` sets up the
/// keyboard before each frame.
fn keys_by_frame(frames: usize, mut at: impl FnMut(usize, &mut Runtime)) -> String {
    use sb_runtime::machine::Machine;
    let mut m = Machine::new(Runtime::new(400, 240, Font::blank()));
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

/// TEST69K: another key going down takes over the repeat, and the first,
/// still held, does not repeat again.
#[test]
fn another_key_stops_a_held_keys_repeat() {
    let typed = keys_by_frame(40, |i, rt| {
        match i {
            0 => {
                rt.input.key_down(A_KEY);
                rt.type_held(u16::from(b'A'));
            }
            // B types nothing itself here, as a key whose text has not come.
            23 => rt.input.key_down(B_KEY),
            _ => {}
        }
        rt.input.live = match i {
            0..23 => holding(&[A_KEY]),
            23..26 => holding(&[A_KEY, B_KEY]),
            _ => holding(&[A_KEY]),
        };
    });
    // Repeats at 19 and 22, and none after B went down.
    assert_eq!(typed, "0A 19A 22A");
}

const ONE_KEY: u8 = 30;
const LEFT_CTRL: u8 = 224;
const RIGHT_CTRL: u8 = 228;
const LEFT_ALT: u8 = 226;
const RIGHT_ALT: u8 = 230;

/// What a key types by the modifiers, as the frontend gives it: `plain`
/// and `shift`, and with Alt SmileBASIC 4's own character.
fn key_text(usage: u8, plain: u8, shift: u8) -> KeyText {
    let alt = alt_key_code(usage).unwrap_or(0);
    KeyText {
        plain: plain.into(),
        shift: shift.into(),
        alt,
        right_alt: alt,
    }
}

/// Presses `usage` as the frontend does: it types `typed`, and its repeats
/// type what `text` gives by the modifiers held then.
fn press_key(rt: &mut Runtime, usage: u8, text: KeyText, typed: u16) {
    rt.input.key_down(usage);
    rt.input.key_text(text);
    rt.type_held(typed);
}

/// The A key with SmileBASIC 4's Caps Lock on.
fn a_text() -> KeyText {
    key_text(A_KEY, b'A', b'a')
}

/// TEST69K: Shift going down while a letter is held holds its repeat for 19
/// frames, and then it repeats in the other case.
#[test]
fn shift_turns_a_held_letters_repeats_round() {
    let typed = keys_by_frame(52, |i, rt| {
        if i == 0 {
            press_key(rt, A_KEY, a_text(), u16::from(b'A'));
        }
        // A modifier going down leaves the repeat going.
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

/// Shift going up does the same the other way, here before the first
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

/// Every key's repeats follow Shift, not only letters': 1 held, then Shift,
/// types "!" after 19 frames.
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

/// Ctrl going down holds the repeat back and types nothing while it is
/// held; going up, it starts the count again, and the key types 19 frames
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

/// A key pressed with Ctrl held types nothing, but held on once Ctrl is let
/// go it types 19 frames later.
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

/// Another key let go while the held one repeats starts its count again.
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

/// Alt going down while a key repeats: after 19 frames it repeats what Alt
/// gives with it, SmileBASIC 4's own character, either Alt alike.
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

/// Keys such as Enter repeat as they typed at the press, with Ctrl held too.
#[test]
fn keys_that_type_a_code_repeat_it_with_ctrl() {
    let typed = keys_by_frame(30, |i, rt| {
        if i == 0 {
            rt.input.key_down(ONE_KEY);
            rt.type_held(u16::from(b'1'));
        }
        rt.input.live = holding(&[ONE_KEY, LEFT_CTRL]);
    });
    // The Ctrl held from the start is no change.
    assert_eq!(typed, "01 191 221 251 281");
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
    // Either Ctrl key, as it is held.
    assert!(holding(&[LEFT_CTRL]).keys_type_nothing());
    assert!(holding(&[RIGHT_CTRL]).keys_type_nothing());
    let shift = State {
        modifiers: 1 << 1,
        ..State::default()
    };
    assert!(!shift.keys_type_nothing());
}

/// L or R held while a key repeats keeps the repeats from typing, without
/// a keyboard key going down or up to start the count again.
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

/// Let go before 19 frames, a key types once, and pressing it again starts
/// its count over.
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

/// What a key typed before a program ends does not repeat after it.
#[test]
fn a_program_that_ends_leaves_no_repeat() {
    use sb_runtime::machine::Machine;
    let mut m = Machine::new(Runtime::new(400, 240, Font::blank()));
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
    let ended = screen(&m);
    for _ in 0..40 {
        m.frame();
    }
    // Direct mode's line has nothing typed into it.
    assert!(screen(&m) == ended);
}

const BACKSPACE_KEY: u8 = 42;
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
    let mut m = Machine::new(Runtime::new(400, 240, Font::blank()));
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

/// A key held in the editor types as it does for a program: at the press,
/// 19 frames later and then every 3, in the machine's frames, though
/// programs see nothing held while the editor has the keys.
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
    // Let go, it types no more.
    let changed = editor_changes(&mut m, 30, |_, _| {});
    assert!(changed.is_empty());
}

/// The editor's repeats follow the modifiers as a program's do.
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
/// the keys, rather than typing into the prompt.
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

/// The prompt repeats a held key in frames too, and the Enter that runs a
/// program goes on repeating into it, 19 frames after the press and then
/// every 3.
#[test]
fn a_key_held_at_the_prompt_repeats_into_the_program_it_runs() {
    use sb_runtime::machine::Machine;
    use sb_runtime::runtime::keys;
    let mut m = Machine::new(Runtime::new(400, 240, Font::blank()));
    m.boot();
    // As TEST70K logs them: the frame from the program's start where
    // INKEY$ gives Enter.
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
    // The prompt takes the line in the frame of the press, and RUN runs in
    // the next and starts the program in it: 19 frames after the press is
    // the program's frame 18, as on SmileBASIC 4.
    let rows = console_rows(&m);
    let at: Vec<&str> = rows.iter().filter_map(|r| r.strip_prefix('K')).collect();
    assert_eq!(at, ["18", "21", "24", "27"], "{rows:?}");
}
