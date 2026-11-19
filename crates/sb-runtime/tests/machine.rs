//! Direct mode and program slots, driven through typed keys.

use sb_runtime::Runtime;
use sb_runtime::font::Font;
use sb_runtime::machine::{Machine, StopReason};
use sb_runtime::speed::Speed;

#[test]
fn recovering_from_a_panic_keeps_the_editors_text() {
    let mut m = machine();
    m.set_slot(0, "A=1\n".to_string());
    m.toggle_editor();
    m.frame();
    m.type_text("?2\n");
    m.frame();
    m.toggle_editor();
    m.frame();
    let edited = m.rt.slots[0].clone();
    assert_ne!(edited, "A=1\n", "typed in the editor");

    m.set_slot(1, "WHILE 1:VSYNC:WEND\n".to_string());
    m.run_slot(1);
    m.frame();
    assert!(m.is_running());
    m.recover_from_panic("a bug");
    assert!(!m.is_running());
    assert_eq!(m.rt.slots[0], edited);
    assert_eq!(m.rt.slots[1], "WHILE 1:VSYNC:WEND\n");
    assert!(
        rows(&m).iter().any(|r| r == "Internal Error (a bug)"),
        "{:?}",
        rows(&m)
    );
    // Direct mode takes lines again.
    enter(&mut m, "?\"back\"");
    assert!(rows(&m).iter().any(|r| r == "BACK"), "{:?}", rows(&m));

    // With the editor open, it stays open and keeps its place.
    m.toggle_editor();
    m.frame();
    m.recover_from_panic("a bug");
    assert!(m.is_editing());
    m.frame();
    assert_eq!(m.rt.slots[0], edited);
}

fn machine() -> Machine {
    let mut m = Machine::new(Runtime::for_tests(Font::blank()));
    m.boot();
    m
}

/// Types a line, presses Enter and runs frames until direct mode waits
/// for input again.
fn enter(m: &mut Machine, line: &str) {
    m.type_text(line);
    m.rt.key_press(sb_runtime::runtime::keys::ENTER);
    for _ in 0..100 {
        m.frame();
        if !m.is_running() {
            return;
        }
    }
    panic!("{line:?} did not finish");
}

/// The console's non-empty rows.
fn rows(m: &Machine) -> Vec<String> {
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

#[test]
fn boot_message() {
    let m = machine();
    let r = rows(&m);
    assert!(r[0].starts_with("ＳｍｉｌｅＢＡＳＩＣ　ｅｍｕｌａｔｏｒ"));
    assert_eq!(r[2], "１３４２１６７２０　ｂｙｔｅｓ　ｆｒｅｅ");
    assert_eq!(r[3], "OK");
    // A blank line separates the message from OK.
    assert_eq!(m.rt.text[4].cell(0, 3).code, 0);
    assert_eq!(m.rt.text[4].cell(0, 4).code, u16::from(b'O'));
}

#[test]
fn direct_mode_keeps_variables() {
    let mut m = machine();
    // Unshifted letters type capitals; shifted ones lower case.
    enter(&mut m, "a=5:b$=\"Hi\"");
    enter(&mut m, "print a*2;b$");
    let r = rows(&m);
    assert_eq!(
        &r[4..],
        ["A=5:B$=\"hI\"", "OK", "PRINT A*2;B$", "10hI", "OK"]
    );
    enter(&mut m, "clear:?a");
    enter(&mut m, "?a");
    assert_eq!(rows(&m).last().unwrap(), "OK");
    assert_eq!(rows(&m)[rows(&m).len() - 2], "0");
}

#[test]
fn direct_mode_declarations() {
    let mut m = machine();
    let last = |m: &Machine| rows(m).iter().rev().nth(1).unwrap().clone();
    // Variables stay from line to line, also those a line did not use.
    enter(&mut m, "a=1");
    enter(&mut m, "b=2");
    enter(&mut m, "?a");
    assert_eq!(last(&m), "1");
    // Any variable from an earlier line is a duplicate, however it came.
    for line in ["dim a[3]", "var b=3"] {
        enter(&mut m, line);
        assert!(last(&m).starts_with("Duplicate variable"), "{line}");
    }
    // Checked as the line runs: the first DIM of the two declares.
    enter(&mut m, "?\"x\";:dim h[2]:dim h[2]");
    // The error goes on from the X.
    assert!(last(&m).starts_with("XDuplicate variable"));
    enter(&mut m, "?len(h)");
    assert_eq!(last(&m), "2");
    // A DIM that fails leaves its array omitted, and a VAR zero.
    enter(&mut m, "dim d[0,3]");
    enter(&mut m, "?typeof(d)");
    assert_eq!(last(&m), "0");
    enter(&mut m, "var g=sqr(-1)");
    enter(&mut m, "?typeof(g);g");
    assert_eq!(last(&m), "20");
    enter(&mut m, "dim d[2]");
    assert!(last(&m).starts_with("Duplicate variable"));
    // A line that does not compile declares nothing; CLEAR forgets all.
    enter(&mut m, "dim k[2]:?(");
    enter(&mut m, "dim k[2]:?len(k)");
    assert_eq!(last(&m), "2");
    enter(&mut m, "clear");
    enter(&mut m, "dim a[3]:?len(a)");
    assert_eq!(last(&m), "3");
}

#[test]
fn slots_run_and_new() {
    let mut m = machine();
    m.set_slot(1, "X=X+1:?\"RAN\";X".to_string());
    enter(&mut m, "x=41");
    enter(&mut m, "run 1");
    enter(&mut m, "?x");
    let r = rows(&m);
    // RUN starts with fresh variables and leaves its own for direct mode.
    assert_eq!(&r[r.len() - 5..], ["RAN1", "OK", "?X", "1", "OK"]);
    enter(&mut m, "new 1");
    assert_eq!(m.slot(1), "");
}

#[test]
fn perfend_times_add_up_until_run() {
    let mut m = machine();
    m.set_slot(
        1,
        "FOR I=0 TO 2:PERFBEGIN 3,&HFF00FF00:PERFEND 3:NEXT:PERFBEGIN 5".to_string(),
    );
    enter(&mut m, "run 1");
    let stats = *m.rt.perf_stats();
    assert_eq!(stats[3].count, 3);
    assert_eq!(stats[3].color, Some(0xFF00_FF00));
    assert!(stats[3].max >= stats[3].average() && stats[3].total >= stats[3].last);
    // Begun but not ended, a measurement shows without times.
    assert!(stats[5].used() && stats[5].count == 0);
    assert!(!stats[0].used());
    // PERFEND in direct mode adds on; RUN starts again.
    enter(&mut m, "perfend 3");
    assert_eq!(m.rt.perf_stats()[3].count, 4);
    m.set_slot(1, String::new());
    enter(&mut m, "run 1");
    assert!(m.rt.perf_stats().iter().all(|s| !s.used()));
    // Without PERFBEGIN, PERFEND still counts; the panel's Reset forgets it.
    enter(&mut m, "perfend 2");
    assert_eq!(m.rt.perf_stats()[2].count, 1);
    m.rt.reset_perf_stats();
    assert!(!m.rt.perf_stats()[2].used());
}

#[test]
fn errors_return_to_direct_mode() {
    let mut m = machine();
    enter(&mut m, "?1/0");
    enter(&mut m, "?(");
    m.set_slot(0, "RUN".to_string());
    enter(&mut m, "run");
    let r = rows(&m);
    // A direct-mode line names no place (TEST70), a program's line does.
    assert_eq!(r[5], "Divide by zero", "{r:?}");
    assert_eq!(r[6], "OK");
    assert_eq!(
        r[8], "Syntax error (expected expression, found Eof)",
        "{r:?}"
    );
    assert_eq!(r[10], "RUN");
    assert!(r[11].starts_with("Syntax error in 0:1"), "{r:?}");
    assert_eq!(r[12], "OK");
}

/// An error message goes on from where the program left the cursor (TEST65).
#[test]
fn errors_follow_the_cursor() {
    let mut m = machine();
    m.set_slot(0, "?\"V\";\nZ=0:X=1/Z".to_string());
    enter(&mut m, "run");
    let r = rows(&m);
    let at = r.iter().position(|l| l.starts_with('V'));
    let at = at.unwrap_or_else(|| panic!("{r:?}"));
    assert_eq!(r[at], "VDivide by zero in 0:2", "{r:?}");
    assert_eq!(r[at + 1], "OK", "{r:?}");
}

#[test]
fn stop_breaks_out_of_a_loop() {
    let mut m = machine();
    m.type_text("@l:i=i+1:goto @l");
    m.rt.key_press(sb_runtime::runtime::keys::ENTER);
    m.frame();
    m.frame();
    assert!(m.is_running());
    m.stop();
    assert!(!m.is_running());
    let r = rows(&m);
    assert_eq!(&r[r.len() - 2..], ["User break on 0:1", "OK"]);
    // Variables of the stopped code stay for direct mode.
    enter(&mut m, "?i>0");
    assert_eq!(rows(&m)[rows(&m).len() - 2], "1");
    // Stopping in direct mode does nothing.
    m.stop();
    assert_eq!(rows(&m).last().unwrap(), "OK");
}

#[test]
fn sysreq_runs_prg0_and_stops() {
    let mut m = machine();
    m.set_slot(0, "?\"A\"\n@l:goto @l".to_string());
    enter(&mut m, "list");
    assert!(m.is_editing());
    m.sysreq();
    assert!(!m.is_editing());
    m.frame();
    m.frame();
    assert!(m.is_running());
    m.sysreq();
    assert!(!m.is_running());
    let r = rows(&m);
    assert_eq!(&r[r.len() - 3..], ["A", "User break on 0:2", "OK"]);
}

#[test]
fn sysreq_jumps_to_on_break_once() {
    let mut m = machine();
    m.set_slot(
        0,
        "ON BREAK GOTO @B\n@L:VSYNC:GOTO @L\n@B:?\"B\"\n@M:VSYNC:GOTO @M".to_string(),
    );
    m.run_slot(0);
    m.frame();
    m.frame();
    m.sysreq();
    m.frame();
    assert!(m.is_running());
    assert_eq!(rows(&m).last().unwrap(), "B");
    // The label is used once; the next press stops.
    m.sysreq();
    assert!(!m.is_running());
    assert_eq!(rows(&m).last().unwrap(), "OK");
}

/// Ctrl+C is a break ON BREAK GOTO takes, as the + button is, but it
/// never starts PRG0.
#[test]
fn keyboard_break_jumps_to_on_break() {
    let mut m = machine();
    m.set_slot(
        0,
        "ON BREAK GOTO @B\n@L:VSYNC:GOTO @L\n@B:?\"B\"\n@M:VSYNC:GOTO @M".to_string(),
    );
    m.run_slot(0);
    m.frame();
    m.keyboard_break();
    m.frame();
    assert!(m.is_running());
    assert_eq!(rows(&m).last().unwrap(), "B");
    m.keyboard_break();
    assert!(!m.is_running());
    m.keyboard_break();
    m.frame();
    assert!(!m.is_running());
    assert_eq!(rows(&m).last().unwrap(), "OK");
}

/// Without ON BREAK GOTO, Ctrl+C stops the program as the + button does.
#[test]
fn keyboard_break_stops_without_on_break() {
    let mut m = machine();
    m.set_slot(0, "@L:VSYNC:GOTO @L".to_string());
    m.run_slot(0);
    m.frame();
    m.keyboard_break();
    assert!(!m.is_running());
    let r = rows(&m);
    assert_eq!(&r[r.len() - 2..], ["User break on 0:1", "OK"]);
}

/// After Ctrl+C's break, the program sees Ctrl held, but C's press
/// neither reads as a press for KEYBOARD nor types for INKEY$, not even
/// held on after Ctrl is released, long enough to repeat. Its release
/// shows.
#[test]
fn keyboard_break_hides_c_from_the_program() {
    let mut m = machine();
    m.set_slot(
        0,
        "ON BREAK GOTO @B\n@L:VSYNC:GOTO @L\n@B\n\
         @W:K$=INKEY$():IF K$!=\"\" THEN ?\"K\";ASC(K$)\n\
         IF KEYBOARD(6,2) THEN ?\"P\"\nIF KEYBOARD(6,3) THEN ?\"R\"\n\
         IF KEYBOARD(224,2) THEN ?\"CTRL\"\nVSYNC:GOTO @W"
            .to_string(),
    );
    m.run_slot(0);
    m.frame();
    // What the frontend does as Ctrl+C goes down: C would repeat 'c'.
    m.rt.input.key_down(0x06);
    m.rt.input.key_text(sb_runtime::input::KeyText {
        plain: u16::from(b'c'),
        shift: u16::from(b'C'),
        ..Default::default()
    });
    m.keyboard_break();
    m.rt.input.live.set_key(0xE0, true);
    m.rt.input.live.set_key(0x06, true);
    for _ in 0..10 {
        m.frame();
    }
    // Ctrl released, C held on: it would repeat now.
    m.rt.input.live.set_key(0xE0, false);
    for _ in 0..60 {
        m.frame();
    }
    m.rt.input.live.set_key(0x06, false);
    m.frame();
    m.frame();
    assert!(m.is_running());
    let r = rows(&m);
    assert_eq!(&r[r.len() - 2..], ["CTRL", "R"]);
}

/// A break jump in code EXEC'd from a DEF leaves the DEF behind, and the
/// EXEC with it: the program ends where that slot ends.
#[test]
fn on_break_in_code_execed_from_a_def_ends_with_the_slot() {
    for exec in ["EXEC 3", "GOSUB @G\n@G:EXEC 3"] {
        let mut m = machine();
        m.set_slot(
            0,
            format!("DEF RUNIT\n VAR Q=5\n {exec}\n ?\"BACK\";Q\nEND\nRUNIT\n?\"DONE\""),
        );
        m.set_slot(
            3,
            "ON BREAK GOTO @B\n@L:VSYNC:GOTO @L\n@B:?\"BRK\"".to_string(),
        );
        m.run_slot(0);
        m.frame();
        m.frame();
        m.sysreq();
        m.frame();
        m.frame();
        assert!(!m.is_running(), "{exec}");
        let r = rows(&m);
        assert_eq!(&r[r.len() - 2..], ["BRK", "OK"], "{exec}");
    }
    // An EXEC from the top level goes back as ever.
    let mut m = machine();
    m.set_slot(0, "EXEC 3\n?\"BACK\"".to_string());
    m.set_slot(
        3,
        "ON BREAK GOTO @B\n@L:VSYNC:GOTO @L\n@B:?\"BRK\"".to_string(),
    );
    m.run_slot(0);
    m.frame();
    m.frame();
    m.sysreq();
    m.frame();
    m.frame();
    let r = rows(&m);
    assert_eq!(&r[r.len() - 3..], ["BRK", "BACK", "OK"]);
}

/// A COMMON DEF may not share its name with a DEF of another slot, nor a
/// DEF with another slot's COMMON DEF: Duplicate function as EXEC
/// compiles the second.
#[test]
fn exec_refuses_a_def_named_as_another_slots_common_def() {
    for (two, three, out) in [
        (
            "COMMON DEF F\nEND",
            "\nCOMMON DEF F\nEND",
            "Duplicate function in 3:2",
        ),
        (
            "COMMON DEF F\nEND",
            "?1\nDEF F\nEND",
            "Duplicate function in 3:2",
        ),
        (
            "DEF F\nEND",
            "COMMON DEF F\nEND",
            "Duplicate function in 3:1",
        ),
        ("DEF F\nEND", "DEF F\nEND\n?\"FINE\"", "FINE"),
    ] {
        let mut m = machine();
        m.set_slot(0, "EXEC 2:EXEC 3".to_string());
        m.set_slot(2, two.to_string());
        m.set_slot(3, three.to_string());
        enter(&mut m, "RUN");
        let r = rows(&m);
        assert!(r[r.len() - 2].starts_with(out), "{two:?} {three:?}: {r:?}");
    }
    // The same slot again replaces its own.
    let mut m = machine();
    m.set_slot(0, "EXEC 3:EXEC 3:?FA()".to_string());
    m.set_slot(3, "COMMON DEF FA()\nRETURN 3\nEND".to_string());
    enter(&mut m, "RUN");
    let r = rows(&m);
    assert_eq!(r[r.len() - 2], "3");
}

#[test]
fn sysreq_out_of_input_also_stops() {
    let mut m = machine();
    m.set_slot(
        0,
        "ON BREAK GOTO @B\nINPUT A\n@B\n?\"B\"\n?\"C\"".to_string(),
    );
    m.run_slot(0);
    m.frame();
    m.frame();
    m.sysreq();
    m.frame();
    // Checked on hardware: the jump goes on after the prompt, and the
    // program stops after the statement it jumped to.
    assert!(!m.is_running());
    let r = rows(&m);
    assert_eq!(&r[r.len() - 3..], ["? B", "User break on 0:4", "OK"]);
}

#[test]
fn sysreq_out_of_input_acts_after_one_statement() {
    // Checked on hardware: a FOR loop first stops after its first
    // statement, with the variable at 1.
    let mut m = machine();
    m.set_slot(
        0,
        "ON BREAK GOTO @B\nINPUT A\n@B\nFOR I=1 TO 99999:NEXT".to_string(),
    );
    m.run_slot(0);
    m.frame();
    m.frame();
    m.sysreq();
    m.frame();
    assert!(!m.is_running());
    enter(&mut m, "?I");
    assert_eq!(rows(&m)[rows(&m).len() - 2], "1");

    // A label set in that statement is jumped to instead.
    let mut m = machine();
    m.set_slot(
        0,
        "ON BREAK GOTO @B\nINPUT A\n@B\nON BREAK GOTO @C\n?\"B\"\nEND\n@C\n?\"C\"".to_string(),
    );
    m.run_slot(0);
    m.frame();
    m.frame();
    m.sysreq();
    m.frame();
    assert!(!m.is_running());
    let r = rows(&m);
    assert_eq!(&r[r.len() - 2..], ["? C", "OK"]);
}

/// Presses keys, one frame each.
fn press(m: &mut Machine, codes: &[u16]) {
    for &code in codes {
        m.rt.key_press(code);
        m.frame();
    }
}

#[test]
fn list_edits_a_slot_and_f4_returns() {
    use sb_runtime::runtime::keys;
    let mut m = machine();
    m.set_slot(0, "?1\r\n?2".to_string());
    enter(&mut m, "list 2");
    assert!(m.is_editing());
    assert_eq!(m.editor().cursor(), (2, 0));
    press(&mut m, &[keys::END]);
    m.type_text("3");
    m.frame();
    m.toggle_editor();
    assert!(!m.is_editing());
    assert_eq!(m.slot(0), "?1\n?23");
    enter(&mut m, "run");
    assert_eq!(&rows(&m)[rows(&m).len() - 3..], ["1", "23", "OK"]);

    // An untouched program keeps its line endings.
    m.set_slot(2, "?1\r\n".to_string());
    enter(&mut m, "list 2:1");
    assert_eq!(m.editor().slot(), 2);
    m.toggle_editor();
    assert_eq!(m.slot(2), "?1\r\n");
}

#[test]
fn f4_and_list_err_show_the_error() {
    let mut m = machine();
    // The cursor goes to the start of the statement that failed.
    m.set_slot(1, "?1\n?2\nA=0:?1/A".to_string());
    enter(&mut m, "run 1");
    m.toggle_editor();
    assert_eq!((m.editor().slot(), m.editor().cursor()), (1, (3, 4)));
    // The error is shown once; later, the editor opens where it was left.
    m.toggle_editor();
    // So do syntax errors (checked on hardware: before the `?`).
    m.set_slot(0, "A=1:?(".to_string());
    enter(&mut m, "run");
    enter(&mut m, "list err");
    assert_eq!((m.editor().slot(), m.editor().cursor()), (0, (1, 4)));
    m.toggle_editor();
    m.toggle_editor();
    assert_eq!(m.editor().slot(), 0);
}

#[test]
fn list_arguments() {
    let mut m = machine();
    enter(&mut m, "list x");
    assert!(!m.is_editing());
    assert!(rows(&m)[rows(&m).len() - 2].starts_with("Syntax error"));
    enter(&mut m, "list 6:1");
    assert!(rows(&m)[rows(&m).len() - 2].starts_with("Out of range"));
    // A name that only starts with LIST is a variable.
    enter(&mut m, "lista=3:?lista");
    assert_eq!(rows(&m)[rows(&m).len() - 2], "3");
    enter(&mut m, "list");
    assert!(m.is_editing());
}

#[test]
fn ctrl_digits_switch_slots() {
    let mut m = machine();
    m.set_slot(3, "?3".to_string());
    // From direct mode, opens the editor on the slot.
    m.edit_slot(3);
    assert!(m.is_editing());
    assert_eq!(m.editor().slot(), 3);
    m.type_text("'");
    m.frame();
    // Switching keeps the edit and opens the other slot.
    m.edit_slot(1);
    assert_eq!((m.editor().slot(), m.slot(3)), (1, "'?3"));
    assert_eq!(m.unsaved_slots(), [3]);
    m.edit_slot(6);
    assert_eq!(m.editor().slot(), 1, "no such slot");
}

/// TEST14 on hardware: a VSYNC that is behind goes straight on once and
/// counts from then; WAIT, WAIT 0 too, starts the count anew.
#[test]
fn vsync_after_falling_behind() {
    let frames = |between: &str| {
        let mut m = machine();
        m.set_slot(
            0,
            format!(
                "VSYNC:FOR I=1 TO 2000000:NEXT{between}\n\
                 M=MAINCNT():FOR I=1 TO 10:VSYNC:NEXT:?MAINCNT()-M"
            ),
        );
        m.run_slot(0);
        for _ in 0..2000 {
            m.frame();
            if !m.is_running() {
                break;
            }
        }
        rows(&m).iter().rev().nth(1).cloned().unwrap_or_default()
    };
    assert_eq!(frames(""), "9");
    assert_eq!(frames(":WAIT 1"), "10");
    assert_eq!(frames(":WAIT 0"), "10");
}

/// A line typed in DIRECT mode runs in the frame its Enter is
/// taken in, and so does CONT's program; RUN's program starts in the
/// frame after the line's, and two frames later at a Switch 1's speed.
#[test]
fn typed_lines_run_and_cont_start_frames() {
    // The frames from the Enter until the console shows `tag`.
    let frames = |m: &mut Machine, line: &str, tag: &str| {
        m.type_text(line);
        m.frame();
        m.rt.key_press(sb_runtime::runtime::keys::ENTER);
        for n in 1..10 {
            m.frame();
            if rows(m).iter().any(|r| r.starts_with(tag)) {
                return n;
            }
        }
        panic!("{line:?}: no {tag:?} in {:?}", rows(m));
    };
    for speed in [Speed::Unlimited, Speed::Switch1, Speed::Switch2] {
        let mut m = machine();
        m.rt.settings.speed = speed;
        m.set_slot(0, "?\"P\":STOP\n?\"Q\"\n".to_string());
        // Typed letters come out in lower case.
        assert_eq!(frames(&mut m, "?\"D\"", "d"), 1, "{speed:?}");
        let run = if speed == Speed::Switch1 { 4 } else { 2 };
        assert_eq!(frames(&mut m, "RUN", "P"), run, "{speed:?}");
        assert_eq!(frames(&mut m, "CONT", "Q"), 1, "{speed:?}");
    }
}

/// Under a time limit, a frame run to catch up can leave a program at its
/// VSYNC, so that the frame drawn next shows a whole trip round its loop.
#[test]
fn a_program_can_be_held_at_its_vsync() {
    let mut m = machine();
    m.time_limit = Some(std::time::Duration::from_secs(1));
    m.set_slot(0, "@L:VSYNC:GOTO @L".to_string());
    m.run_slot(0);
    m.frame();
    assert!(m.at_sync() && !m.mid_loop());
    let (executed, vsyncs) = (m.executed(), m.vsyncs());

    m.hold_at_sync = true;
    m.frame();
    m.frame();
    assert!(m.at_sync());
    assert_eq!((m.executed(), m.vsyncs()), (executed, vsyncs));

    m.hold_at_sync = false;
    m.frame();
    assert!(m.at_sync());
    assert!(m.vsyncs() > vsyncs);
}

/// Runs `program` to its end, or for `frames` calls of [`Machine::frame`],
/// with [`Machine::keep_frames`] as given, and gives what it printed last.
fn last_line_kept(program: &str, keep: bool, frames: u32) -> String {
    let mut m = machine();
    m.keep_frames = keep;
    m.set_slot(0, program.to_string());
    m.run_slot(0);
    for _ in 0..frames {
        m.frame();
        if !m.is_running() {
            break;
        }
    }
    let rows = rows(&m);
    let last = (rows.iter().rev()).find(|r| !r.trim().is_empty() && r.as_str() != "OK");
    last.cloned().unwrap_or_default()
}

/// A trip round the main loop that takes more than a frame's
/// instructions misses frames, as on SmileBASIC 4; kept frames give each
/// trip one frame.
#[test]
fn kept_frames_give_each_trip_one_frame() {
    let program = "VSYNC:M=MAINCNT()\n\
                   FOR J=1 TO 5:FOR I=1 TO 100000:NEXT:VSYNC:NEXT\n\
                   ?MAINCNT()-M\n";
    let missed: u32 = last_line_kept(program, false, 1000).trim().parse().unwrap();
    assert!(missed > 5, "{missed}");
    assert_eq!(last_line_kept(program, true, 1000).trim(), "5");
}

/// Kept frames leave alone a program that does not VSYNC and a trip too
/// long to keep, such as loading: their frames go by as usual.
#[test]
fn kept_frames_let_long_trips_and_busy_programs_be() {
    let busy = "M=MAINCNT():FOR I=1 TO 600000:NEXT:?MAINCNT()-M\n";
    assert_eq!(
        last_line_kept(busy, true, 1000),
        last_line_kept(busy, false, 1000)
    );
    // One frame for the calls it keeps, and then one each.
    let long = "VSYNC:M=MAINCNT():FOR I=1 TO 600000:NEXT:VSYNC:?MAINCNT()-M\n";
    let frames = |keep| -> u32 { last_line_kept(long, keep, 1000).trim().parse().unwrap() };
    let kept = sb_runtime::machine::KEEP_MOST_CALLS - 1;
    assert!(frames(false) > 2 * kept, "{}", frames(false));
    assert_eq!(frames(true), frames(false) - kept);
}

/// OTAKA_GAME_COLLECTION00's pellets: a relative SPANIM of 32 frames,
/// started again every 32 trips by a counter in the main loop. A trip of
/// more than two frames' work misses a frame even though the VSYNC after
/// it goes straight on, and each missed frame moves the place the next
/// start counts from, so the sprite wanders off. Kept frames keep it
/// where it was.
#[test]
fn kept_frames_keep_a_restarted_relative_animation_in_place() {
    let program = "SPSET 0,0:SPOFS 0,100,100:C=0:VSYNC\n\
                   FOR J=0 TO 95\n\
                   IF C==0 THEN SPANIM 0,\"XY+\",-8,0,-1,-8,0,0,-8,0,1,-8,0,0,0\n\
                   C=(C+1) MOD 32\n\
                   IF J MOD 5==2 THEN FOR I=1 TO 160000:NEXT\n\
                   VSYNC\n\
                   NEXT\n\
                   SPOFS 0 OUT X,Y:?Y\n";
    let wandered: f64 = last_line_kept(program, false, 1000).trim().parse().unwrap();
    assert!((wandered - 100.0).abs() > 0.5, "{wandered}");
    assert_eq!(last_line_kept(program, true, 1000).trim(), "100");
}

/// The presses and releases of A that a program counts with BUTTON once
/// a trip, each trip taking `work` FOR steps, when A is held for `hold`
/// calls of [`Machine::frame`] every 20, 15 times.
fn presses_counted(work: u32, hold: u32, keep: bool) -> String {
    let mut m = machine();
    m.keep_frames = keep;
    m.set_slot(
        0,
        format!(
            "CLS:N=0:R=0:WHILE 1:FOR I=1 TO {work}:NEXT\n\
             IF BUTTON(0,#B_RRIGHT,2) THEN INC N\n\
             IF BUTTON(0,#B_RRIGHT,3) THEN INC R\n\
             LOCATE 0,0:?N;\" \";R;\" \";:VSYNC:WEND\n"
        ),
    );
    m.run_slot(0);
    for call in 0..20 * 15 + 40 {
        let pressed = (20..20 * 16).contains(&call) && call % 20 < hold;
        m.rt.input.live.pads[0].buttons = u32::from(pressed) << sb_runtime::input::A;
        m.frame();
    }
    rows(&m)[0].trim().to_string()
}

/// A trip longer than a frame's work reads BUTTON in one of the frames it
/// takes, and a press or release shows only on the frame it comes in. A
/// frame that ends part-way through the trip, with nothing read, passes
/// its presses and releases on to the next, so that none go missing, also
/// those that start and end in it; kept frames give the trip one frame.
#[test]
fn presses_survive_trips_longer_than_a_frame() {
    for hold in [1, 3] {
        for keep in [false, true] {
            assert_eq!(presses_counted(1000, hold, keep), "15 15", "{hold} {keep}");
            assert_eq!(
                presses_counted(120_000, hold, keep),
                "15 15",
                "{hold} {keep}"
            );
        }
    }
}

/// Kept frames do not spend a frame that would leave the program at its
/// VSYNC: MAINCNT and the animations wait for the program.
#[test]
fn kept_frames_spend_no_frame_held_at_vsync() {
    let mut m = machine();
    m.keep_frames = true;
    m.time_limit = Some(std::time::Duration::from_secs(1));
    m.set_slot(0, "@L:VSYNC:GOTO @L".to_string());
    m.run_slot(0);
    m.frame();
    let count = m.rt.main_count();
    m.hold_at_sync = true;
    m.frame();
    m.frame();
    assert_eq!(m.rt.main_count(), count);
    m.hold_at_sync = false;
    m.frame();
    assert_eq!(m.rt.main_count(), count.wrapping_add(1));
}

/// A VSYNC that is behind and waits for the next frame is counted once.
/// The VSYNCs after it come a frame late only by that wait, each in the
/// frame after the last, and lose nothing.
#[test]
fn a_deferred_vsync_is_counted_once() {
    let mut m = machine();
    // No time is left at any VSYNC, so one that is behind cannot go on.
    m.time_limit = Some(std::time::Duration::ZERO);
    // VSYNC 0 is behind by itself.
    m.set_slot(0, "VSYNC 0:@L:VSYNC:GOTO @L".to_string());
    m.run_slot(0);
    m.frame();
    assert_eq!(m.missed.deferred, 1);
    let count = m.rt.main_count();
    for _ in 0..5 {
        m.frame();
    }
    assert_eq!(m.rt.main_count(), count.wrapping_add(5));
    assert_eq!(m.missed.deferred, 1, "a trip every frame, and none behind");
    assert!(m.at_sync());
}

/// A program out of time between VSYNCs has only part of a scene made.
#[test]
fn a_program_out_of_time_is_mid_loop() {
    let mut m = machine();
    m.time_limit = Some(std::time::Duration::from_micros(100));
    m.set_slot(0, "@L:GOTO @L".to_string());
    m.run_slot(0);
    m.frame();
    assert!(m.is_running() && m.mid_loop() && !m.at_sync());
}

/// The line typed after the last prompt, as Enter would run it.
fn typed(m: &mut Machine, keys: &[u16]) -> String {
    press(m, keys);
    let console = &m.rt.text[4];
    let (w, _) = console.size();
    let (_, y) = console.cursor();
    let codes: Vec<u16> = (0..w)
        .map(|x| console.cell(x, y).code)
        .take_while(|&c| c != 0)
        .collect();
    String::from_utf16(&codes).unwrap()
}

#[test]
fn up_and_down_walk_the_history() {
    use sb_runtime::runtime::keys::{BACKSPACE, DOWN, UP};
    let mut m = machine();
    enter(&mut m, "a=1");
    enter(&mut m, "a=22");
    m.type_text("b");
    assert_eq!(typed(&mut m, &[UP]), "A=22");
    assert_eq!(typed(&mut m, &[UP]), "A=1");
    // The oldest line stays when there is nothing older.
    assert_eq!(typed(&mut m, &[UP]), "A=1");
    assert_eq!(typed(&mut m, &[DOWN]), "A=22");
    // Down past the newest brings back what was being typed.
    assert_eq!(typed(&mut m, &[DOWN]), "B");
    assert_eq!(typed(&mut m, &[DOWN]), "B");
    // A recalled line can be edited and run.
    press(&mut m, &[UP, BACKSPACE]);
    enter(&mut m, "3");
    enter(&mut m, "print a");
    assert!(rows(&m).iter().any(|r| r == "23"));
}

#[test]
fn the_same_line_twice_is_one_history_entry() {
    use sb_runtime::runtime::keys::UP;
    let mut m = machine();
    enter(&mut m, "a=1");
    enter(&mut m, "a=2");
    press(&mut m, &[UP]);
    enter(&mut m, "");
    assert_eq!(typed(&mut m, &[UP, UP]), "A=1");
}

#[test]
fn split_editor_sides() {
    use sb_runtime::runtime::keys;
    let mut m = machine();
    m.set_slot(0, "?0".to_string());
    m.set_slot(1, "?1".to_string());
    m.edit_slot(0);
    m.toggle_split();
    assert!(m.is_split());
    // Both sides show PRG0 (letters typed swap case); typing on one shows on the other.
    m.type_text("A");
    m.frame();
    m.render();
    m.switch_side();
    assert_eq!((m.active_side(), m.editor().text()), (1, "a?0".to_string()));
    // The undo history came along.
    m.edit().undo();
    assert_eq!(m.editor().text(), "?0");
    // The right side opens PRG1; the left keeps PRG0.
    m.edit_slot(1);
    press(&mut m, &[keys::END]);
    m.type_text("1");
    m.frame();
    m.switch_side();
    assert_eq!(
        (m.editor().slot(), m.editor().text()),
        (0, "?0".to_string())
    );
    // Asking for PRG1 here moves to the side showing it.
    m.edit_slot(1);
    assert_eq!(m.active_side(), 1);
    assert_eq!(m.unsaved_slots(), [0, 1]);
    // Leaving keeps both sides' edits.
    m.toggle_editor();
    assert_eq!((m.slot(0), m.slot(1)), ("?0", "?11"));
    m.toggle_editor();
    assert!(m.is_split());
    assert_eq!(m.editor().slot(), 1);
    // Side by side, then one above the other, then one side.
    m.toggle_split();
    assert!(m.is_split_rows());
    let h = m.render().height() as i32;
    // A press on the upper side gives it the keys.
    m.editor_mouse_down(100, h / 4, false);
    assert_eq!(m.active_side(), 0);
    m.editor_mouse_down(100, h - h / 4, false);
    assert_eq!(m.active_side(), 1);
    m.toggle_split();
    assert!(!m.is_split());
}

#[test]
fn split_editor_shows_the_side_with_the_keys() {
    let mut m = machine();
    m.edit_slot(0);
    m.toggle_split();
    let fb = m.render();
    let (w, h) = (fb.width(), fb.height());
    let second = w - (w - 2) / 2;
    let edges = |m: &mut Machine| {
        let fb = m.render();
        (
            fb.get(1, h / 2).unwrap(),
            fb.get(second + 1, h / 2).unwrap(),
        )
    };
    // A frame shows around the side that gets the keys, then fades out.
    let (left, right) = edges(&mut m);
    assert_ne!(left, right);
    m.switch_side();
    let (left2, right2) = edges(&mut m);
    assert_eq!((left2, right2), (right, left));
    for _ in 0..60 {
        m.render();
    }
    let (left3, right3) = edges(&mut m);
    // Once it is gone, blank margins match on both sides...
    assert_eq!(left3, right3);
    // ...but the status line of the side without the keys is dimmer.
    let fb = m.render();
    let bar = |x| fb.get(x, h - 1).unwrap() & 0xFF;
    assert!(bar(w / 4) < bar(second + w / 4));
}

#[test]
fn direct_mode_completes_words() {
    use sb_runtime::runtime::keys;
    let mut m = machine();
    // The program's names are offered only when asked for.
    m.rt.settings.complete_names = true;
    m.set_slot(0, "SCORE_TOTAL=5".to_string());
    // Typed letters swap case, so these come out upper case.
    m.type_text("?score_t");
    m.rt.key_press(keys::TAB);
    m.rt.key_press(keys::ENTER);
    for _ in 0..10 {
        m.frame();
    }
    // The variable is unset in direct mode, but the line read it.
    assert_eq!(&rows(&m)[rows(&m).len() - 3..], ["?SCORE_TOTAL", "0", "OK"]);
    // Up still goes through the history without a list.
    m.rt.key_press(keys::UP);
    m.frame();
    assert_eq!(rows(&m).last().map(String::as_str), Some("?SCORE_TOTAL"));
}

#[test]
fn completion_offers_other_slots_names_when_asked() {
    use sb_runtime::runtime::keys;
    let typed = |all: bool| {
        let mut m = machine();
        m.rt.settings.complete_names = true;
        m.rt.settings.complete_all_slots = all;
        m.set_slot(2, "HIGH_SCORE=5".to_string());
        m.type_text("?high_s");
        m.rt.key_press(keys::TAB);
        m.frame();
        m.rt.line_and_cursor()
            .map(|(t, _)| String::from_utf16_lossy(t))
    };
    // With nothing to take, Tab is a space.
    assert_eq!(typed(false).as_deref(), Some("?HIGH_S "));
    assert_eq!(typed(true).as_deref(), Some("?HIGH_SCORE"));
}

#[test]
fn simple_save_and_load() {
    use sb_runtime::storage::Storage;
    let dir = std::env::temp_dir().join(format!("sb-simple-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let mut m = machine();
    m.rt.storage = Storage::new(dir.clone());
    m.set_slot(1, "?1".to_string());
    m.edit_slot(1);
    m.type_text("'");
    m.frame();
    let name = "GAME";
    m.save_edited(name).unwrap();
    assert!(m.unsaved_slots().is_empty());
    assert!(m.rt.txt_exists("GAME"));
    // Loading drops the edits.
    m.type_text("X");
    m.frame();
    m.load_edited("GAME").unwrap();
    assert_eq!(m.editor().text(), "'?1");
    assert!(m.unsaved_slots().is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn keys_left_by_a_program_are_dropped() {
    let mut m = machine();
    // Stopped, and ending by itself.
    m.set_slot(0, "@L:VSYNC:GOTO @L".to_string());
    m.set_slot(1, "FOR I=0 TO 4:VSYNC:NEXT".to_string());
    m.type_text("RUN");
    m.rt.key_press(sb_runtime::runtime::keys::ENTER);
    m.frame();
    m.type_text("XYZ");
    m.frame();
    m.stop();
    enter(&mut m, "?1");
    assert_eq!(rows(&m)[rows(&m).len() - 2], "1");
    m.type_text("RUN 1");
    m.rt.key_press(sb_runtime::runtime::keys::ENTER);
    m.frame();
    m.type_text("ABC");
    for _ in 0..20 {
        m.frame();
    }
    assert!(!m.is_running());
    enter(&mut m, "?2");
    assert_eq!(rows(&m)[rows(&m).len() - 2], "2");
}

/// After a `:`, RUN and CONT are a Syntax error, and nothing on
/// the line runs.
#[test]
fn run_and_cont_only_start_a_direct_line() {
    let mut m = machine();
    m.set_slot(0, "?\"P\"\nSTOP\n?\"Q\"".to_string());
    let last = |m: &Machine| rows(m)[rows(m).len() - 2..].to_vec();
    enter(&mut m, "?\"BEFORE\":RUN");
    assert!(last(&m)[0].starts_with("Syntax error"), "{:?}", rows(&m));
    enter(&mut m, "RUN");
    assert_eq!(last(&m), ["Break on 0:2", "OK"]);
    enter(&mut m, "?\"BEFORE\":CONT");
    assert!(last(&m)[0].starts_with("Syntax error"), "{:?}", rows(&m));
    assert!(!rows(&m).iter().any(|r| r == "before"), "{:?}", rows(&m));
    enter(&mut m, "  CONT");
    assert_eq!(last(&m), ["Q", "OK"]);
}

/// The other commands of DIRECT mode are a Syntax error after a `:` or
/// THEN too, and nothing on the line runs.
#[test]
fn direct_commands_only_start_a_line() {
    let mut m = machine();
    let last = |m: &Machine| rows(m)[rows(m).len() - 2].clone();
    for line in [
        "?1:CLEAR",
        "?2:TRACE",
        "?3:NEW",
        "?4:BACKTRACE",
        "IF 1 THEN RUN",
        ":RUN",
    ] {
        enter(&mut m, line);
        assert!(
            last(&m).starts_with("Syntax error"),
            "{line}: {:?}",
            rows(&m)
        );
    }
    for n in ["1", "2", "3", "4"] {
        assert!(!rows(&m).iter().any(|r| r == n), "{n}: {:?}", rows(&m));
    }
}

/// RUN reads one number literal and nothing after it; CONT and CLEAR
/// read nothing.
#[test]
fn direct_commands_ignore_the_rest_of_the_line() {
    let mut m = machine();
    for slot in 0..4 {
        m.set_slot(slot, format!("?\"SLOT{slot}\""));
    }
    let last = |m: &Machine| rows(m)[rows(m).len() - 2].clone();
    for (line, out) in [
        ("RUN 1:?\"NO\"", "SLOT1"),
        ("RUN 1+2", "SLOT1"),
        ("run &h2", "SLOT2"),
        ("RUN &B11", "SLOT3"),
        ("RUN", "SLOT0"),
        ("RUN 6", "Out of range (RUN: slot 6)"),
        (
            "RUN:?\"NO\"",
            "Illegal function call (RUN: a slot number literal)",
        ),
        (
            "RUN A",
            "Illegal function call (RUN: a slot number literal)",
        ),
        (
            "RUN 1.5",
            "Illegal function call (RUN: a slot number literal)",
        ),
        // A minus reads as slot 0 (checked on hardware with RUN -3).
        ("RUN -1", "SLOT0"),
        ("RUN -3", "SLOT0"),
        ("SUBRUN", "Syntax error (SUBRUN: a file name literal)"),
    ] {
        enter(&mut m, line);
        assert_eq!(last(&m), out, "{line}");
    }
    assert!(!rows(&m).iter().any(|r| r == "NO"), "{:?}", rows(&m));

    m.set_slot(0, "?\"P\"\nSTOP\n?\"Q\"".to_string());
    enter(&mut m, "RUN");
    enter(&mut m, "CONT:?\"NO\"");
    assert_eq!(last(&m), "Q");
    enter(&mut m, "RUN");
    enter(&mut m, "CONT 7");
    assert_eq!(last(&m), "Q");
    enter(&mut m, "X=5");
    enter(&mut m, "CLEAR 5:X=1");
    enter(&mut m, "?X");
    assert_eq!(last(&m), "0");
}

/// LIST reads literals: a line, or a slot, a `:` and an optional line.
#[test]
fn list_reads_literals() {
    let mut m = machine();
    m.set_slot(1, "?1\n?2\n?3".to_string());
    let last = |m: &Machine| rows(m)[rows(m).len() - 2].clone();
    for (line, out) in [
        // Checked on hardware: a real first is a Syntax error, and a
        // word after the `:` a Type mismatch.
        ("LIST 1.5", "Syntax error (LIST: a line, a slot or ERR)"),
        ("LIST 1:2.5", "Type mismatch (LIST: a line number)"),
        ("LIST 1:A", "Type mismatch (LIST: a line number)"),
        ("LIST 1000000", "Out of range (LIST: line 1000000)"),
        ("LIST 6:", "Out of range (LIST: slot 6)"),
        ("LIST 10+1", "Syntax error (LIST: a line, a slot or ERR)"),
    ] {
        enter(&mut m, line);
        assert!(!m.is_editing(), "{line}");
        assert_eq!(last(&m), out, "{line}");
    }
    enter(&mut m, "LIST 1:");
    assert_eq!((m.editor().slot(), m.editor().cursor().0), (1, 1));
    m.toggle_editor();
    enter(&mut m, "LIST 1:&H3");
    assert_eq!((m.editor().slot(), m.editor().cursor().0), (1, 3));
    m.toggle_editor();
    enter(&mut m, "LIST &B10");
    assert_eq!((m.editor().slot(), m.editor().cursor().0), (1, 2));
}

#[test]
fn stop_and_cont() {
    let mut m = machine();
    m.set_slot(0, "A=1\nSTOP\n?A\nSTOP \"HI\"\n?\"END\"".to_string());
    enter(&mut m, "RUN");
    let r = rows(&m);
    assert_eq!(&r[r.len() - 2..], ["Break on 0:2", "OK"]);
    // The main interpreter is running whatever asks.
    enter(&mut m, "?ENVSTAT():A=5");
    assert_eq!(rows(&m)[rows(&m).len() - 2], "3");
    // Direct mode's changes reach the program; the message follows the
    // line.
    enter(&mut m, "CONT");
    let r = rows(&m);
    assert_eq!(&r[r.len() - 5..], ["cont", "5", "Break on 0:4", "HI", "OK"]);
    enter(&mut m, "CONT");
    let r = rows(&m);
    assert_eq!(&r[r.len() - 2..], ["END", "OK"]);
    enter(&mut m, "CONT");
    let r = rows(&m);
    assert_eq!(r[r.len() - 2], "Can't continue");
    enter(&mut m, "STOP");
    let r = rows(&m);
    assert!(
        r[r.len() - 2].starts_with("Can't use from direct mode"),
        "{r:?}"
    );
}

#[test]
fn cont_after_edits_and_input_but_not_errors() {
    let mut m = machine();
    m.set_slot(0, "STOP\n?1".to_string());
    enter(&mut m, "RUN");
    m.set_slot(0, "STOP\n?2".to_string());
    enter(&mut m, "CONT");
    let r = rows(&m);
    assert_eq!(&r[r.len() - 2..], ["1", "OK"]);

    // + at INPUT stops after the prompt; CONT goes on after the INPUT.
    m.set_slot(0, "X=7\nINPUT \"IN\";X\n?\"AFTER\";X".to_string());
    m.run_slot(0);
    m.frame();
    m.frame();
    m.stop();
    let r = rows(&m);
    assert_eq!(&r[r.len() - 2..], ["IN? User break on 0:2", "OK"]);
    enter(&mut m, "CONT");
    let r = rows(&m);
    assert_eq!(&r[r.len() - 2..], ["AFTER7", "OK"]);

    m.set_slot(0, "A=0\n?1/A".to_string());
    enter(&mut m, "RUN");
    enter(&mut m, "CONT");
    let r = rows(&m);
    assert_eq!(r[r.len() - 2], "Can't continue");
}

#[test]
fn inspect_pages_long_arrays() {
    let mut m = machine();
    enter(&mut m, "??1.5:?? \"AB\"");
    let r = rows(&m);
    assert_eq!(
        &r[r.len() - 3..],
        ["REAL: 1.50000000", "STRING: (2)\"ab\"", "OK"]
    );
    // TTEST19 P5 on hardware: 28 lines a page, the last row kept for the
    // prompt, and the screen it left.
    let src = "ACLS:?\"PART? 5\"\nFOR I=0 TO 9:?\"LINE\";I:NEXT\nDIM LB%[60]\n??LB%\nWAIT 9999";
    m.set_slot(0, src.to_string());
    m.run_slot(0);
    for _ in 0..5 {
        m.frame();
    }
    let screen = |m: &Machine| -> Vec<String> {
        let console = &m.rt.text[4];
        let (w, h) = console.size();
        (0..h)
            .map(|y| {
                let codes: Vec<u16> = (0..w).map(|x| console.cell(x, y).code).collect();
                let row = String::from_utf16(&codes).unwrap();
                row.trim_end_matches(['\0', ' ']).to_string()
            })
            .collect()
    };
    let s = screen(&m);
    assert_eq!(s[0], "INT[60]:");
    assert_eq!(s[27], " [26]: 0");
    assert_eq!(s[28], "");
    assert_eq!(s[29], "=== Press ENTER ===");
    m.rt.key_press(sb_runtime::runtime::keys::ENTER);
    for _ in 0..3 {
        m.frame();
    }
    let s = screen(&m);
    assert_eq!(s[27], " [54]: 0");
    assert_eq!(s[29], "=== Press ENTER ===");
    m.rt.key_press(sb_runtime::runtime::keys::ENTER);
    for _ in 0..3 {
        m.frame();
    }
    let s = screen(&m);
    assert_eq!(s[0], " [33]: 0");
    assert_eq!(s[26], " [59]: 0");
    assert_eq!(s[27], "");
}

/// INSPECT shows each type of array, in one to four dimensions, page by
/// page: after each page the screen holds the last lines shown, all of
/// them in order.
#[test]
fn inspect_pages_show_every_line_in_order() {
    // The program, the dimensions, and each element's text.
    type Element = fn(usize) -> String;
    let cases: [(&str, &[usize], &str, Element); 4] = [
        (
            "DIM A%[100]:FOR I=0 TO 99:A%[I]=I*I-500:NEXT:INSPECT A%",
            &[100],
            "INT",
            |i| (i as i64 * i as i64 - 500).to_string(),
        ),
        (
            "DIM A#[7,9]:FOR I=0 TO 62:A#[I DIV 9,I MOD 9]=I/8-3:NEXT:INSPECT A#",
            &[7, 9],
            "REAL",
            |i| format!("{:.8}", i as f64 / 8.0 - 3.0),
        ),
        (
            "DIM A$[3,4,5]:FOR I=0 TO 59:A$[I DIV 20,(I DIV 5) MOD 4,I MOD 5]=\"S\"*(I MOD 9):NEXT:INSPECT A$",
            &[3, 4, 5],
            "STRING",
            |i| format!("({})\"{}\"", i % 9, "S".repeat(i % 9)),
        ),
        (
            "DIM A%[2,3,4,5]:FOR I=0 TO 119:A%[I DIV 60,(I DIV 20) MOD 3,(I DIV 5) MOD 4,I MOD 5]=-I:NEXT:INSPECT A%",
            &[2, 3, 4, 5],
            "INT",
            |i| (-(i as i64)).to_string(),
        ),
    ];
    for (setup, dims, kind, element) in cases {
        let mut m = machine();
        m.set_slot(0, format!("ACLS\n{setup}\nWAIT 9999"));
        m.run_slot(0);
        for _ in 0..5 {
            m.frame();
        }
        let names: Vec<String> = dims.iter().map(usize::to_string).collect();
        let mut expected = vec![format!("{kind}[{}]:", names.join(","))];
        let len: usize = dims.iter().product();
        for i in 0..len {
            let mut index = Vec::new();
            let mut flat = i;
            for &n in dims.iter().rev() {
                index.push((flat % n).to_string());
                flat /= n;
            }
            index.reverse();
            expected.push(format!(" [{}]: {}", index.join(","), element(i)));
        }
        expected.push(String::new());
        let page = m.rt.text[4].size().1 as usize - 2;
        let mut shown = 0;
        loop {
            shown = (shown + page).min(expected.len());
            let screen: Vec<String> = (0..page as u32)
                .map(|y| {
                    let console = &m.rt.text[4];
                    let codes: Vec<u16> = (0..console.size().0)
                        .map(|x| console.cell(x, y).code)
                        .collect();
                    let row = String::from_utf16(&codes).unwrap();
                    row.trim_end_matches(['\0', ' ']).to_string()
                })
                .collect();
            let from = shown.saturating_sub(page);
            let want = &expected[from..shown];
            assert_eq!(&screen[..want.len()], want, "{setup}, {shown} lines");
            let prompt = (m.rt.text[4].cell(0, page as u32 + 1).code) == u16::from(b'=');
            assert_eq!(prompt, shown < expected.len(), "{setup}, {shown} lines");
            if !prompt {
                break;
            }
            m.rt.key_press(sb_runtime::runtime::keys::ENTER);
            for _ in 0..3 {
                m.frame();
            }
        }
    }
}

#[test]
fn every_line_of_a_paste_runs_though_the_prompt_drops_keys() {
    let mut m = machine();
    m.frame();
    m.paste("?11\n?22\n?33\n");
    for _ in 0..20 {
        m.frame();
    }
    let rows = rows(&m);
    for line in ["11", "22", "33"] {
        assert!(rows.iter().any(|r| r.trim() == line), "{rows:?}");
    }
}

#[test]
fn paste_that_does_not_fit_the_line_is_left_out_whole() {
    let mut m = machine();
    m.frame();
    m.paste(&"A".repeat(250));
    for _ in 0..8 {
        m.frame();
    }
    assert_eq!(m.rt.line_and_cursor().unwrap().0.len(), 250);
    m.paste("1234567");
    m.frame();
    assert_eq!(m.rt.line_and_cursor().unwrap().0.len(), 250);
    m.paste("123456");
    m.frame();
    assert_eq!(m.rt.line_and_cursor().unwrap().0.len(), 256);
    // Typed on, it stays as it is, and the cursor at its end.
    m.rt.key_press(0x42);
    m.frame();
    assert_eq!(m.rt.line_and_cursor().unwrap().1, 256);
    assert_eq!(m.rt.line_and_cursor().unwrap().0.len(), 256);
}

#[test]
fn direct_mode_keeps_the_consoles_last_row_free() {
    let mut m = machine();
    for _ in 0..40 {
        enter(&mut m, "");
    }
    let console = &m.rt.text[4];
    assert_eq!(console.cursor().1 + 2, console.size().1);
    // A narrower, shorter console too.
    enter(&mut m, "TSCREEN 4,8,26,12");
    for _ in 0..20 {
        enter(&mut m, "");
    }
    assert_eq!(m.rt.text[4].cursor().1, 10);
}

#[test]
fn backtrace_and_trace() {
    let mut m = machine();
    let def = "DEF DF N\n STOP\n ?\"IN DF\";N\nEND\nDEF DE N\n ?1/N\nEND";
    m.set_slot(0, format!("GOSUB @G\nEND\n@G\nDF 1\nRETURN\n{def}"));
    enter(&mut m, "RUN");
    enter(&mut m, "BACKTRACE");
    let r = rows(&m);
    assert_eq!(
        &r[r.len() - 4..],
        ["0:     7", "0:     4", "0:     1", "OK"]
    );
    // Unlike SmileBASIC 4, the program goes on after BACKTRACE.
    enter(&mut m, "TRACE");
    let r = rows(&m);
    assert_eq!(&r[r.len() - 3..], ["IN DF1", "Break on 0:8", "OK"]);
    enter(&mut m, "CONT");
    assert_eq!(rows(&m).last().unwrap(), "OK");

    // After an error, BACKTRACE still works.
    m.set_slot(0, format!("GOSUB @E\nEND\n@E\nDE 0\nRETURN\n{def}"));
    enter(&mut m, "RUN");
    enter(&mut m, "BACKTRACE");
    let r = rows(&m);
    assert_eq!(
        &r[r.len() - 4..],
        ["0:    11", "0:     4", "0:     1", "OK"]
    );
}

/// TRACE on hardware (TTEST19T, TTEST20): `(output, line)` for each
/// TRACE after the program's STOP.
fn traces(src: &str, part: Option<&str>, expected: &[(&str, u32)]) {
    let mut m = machine();
    m.set_slot(0, src.to_string());
    match part {
        Some(part) => {
            m.run_slot(0);
            m.frame();
            enter(&mut m, part);
        }
        None => enter(&mut m, "RUN"),
    }
    for &(out, at) in expected {
        enter(&mut m, "TRACE");
        let r = rows(&m);
        let mut want = vec![format!("Break on 0:{at}"), "OK".to_string()];
        if !out.is_empty() {
            want.insert(0, out.to_string());
        }
        assert_eq!(r[r.len() - want.len()..], want, "{r:?}");
    }
}

#[test]
fn trace_statement_by_statement() {
    // A FOR runs on into the first statement of its body.
    let src = "STOP\n?\"A\":?\"B\"\n?\"C\"\nFOR I=0 TO 1\n?I\nNEXT\nX=0\n?1/X";
    traces(src, None, &[("A", 2), ("B", 2), ("C", 3), ("0", 5)]);
    let mut m = machine();
    m.set_slot(0, src.to_string());
    enter(&mut m, "RUN");
    enter(&mut m, "CONT");
    enter(&mut m, "TRACE");
    assert_eq!(rows(&m)[rows(&m).len() - 2], "Can't trace");

    // TTEST20: a DEF's END runs with what follows the call; RETURN names
    // the GOSUB's line. NEXT goes round the FOR's step and test, each on
    // the FOR's line.
    let src = "INPUT P\nON P-1 GOSUB @P1,@P2\nEND\n@P1\nDT 5\n?\"BACK\"\nRETURN\n\
               DEF DT N\n STOP\n ?\"X\";N\n ?\"Y\"\nEND\n\
               @P2\nSTOP\nFOR I=0 TO 1\n?\"I\";I\n?\"J\"\nNEXT\n?\"DONE\"\nRETURN";
    traces(
        src,
        Some("1"),
        &[("X5", 10), ("Y", 11), ("BACK", 6), ("", 2)],
    );
    let expected = [
        ("I0", 16),
        ("J", 17),
        ("", 15),
        ("", 15),
        ("I1", 16),
        ("J", 17),
        ("", 15),
    ];
    traces(src, Some("2"), &expected);
}

/// A machine whose debugger stops leave the editor closed, for tests of
/// where programs stop.
fn debug_machine() -> Machine {
    let mut m = machine();
    m.rt.settings.debug_open_editor = false;
    m
}

/// Goes on with the stopped program from the debugger and runs until it
/// stops or ends.
fn debug(m: &mut Machine, how: sb_runtime::machine::Resume) {
    assert!(m.debug(how));
    for _ in 0..100 {
        m.frame();
        if !m.is_running() {
            break;
        }
    }
}

/// A global of the stopped program's slot 0, as PRINT shows it.
fn global(m: &Machine, name: &str) -> Option<String> {
    let globals = m.slot_globals(0);
    let (_, value) = globals.iter().find(|(n, _)| n == name)?;
    Some(format!("{value:?}"))
}

#[test]
fn breakpoints_stop_before_their_line() {
    use sb_core::vm::CallKind;
    use sb_runtime::machine::Resume;
    let mut m = debug_machine();
    m.set_slot(
        0,
        "A=1\nFOR I=0 TO 2:B=I:NEXT\nFOR J=0 TO 1\n X=J\nNEXT\nGOSUB @S\nEND\n@S\nD=4\nRETURN"
            .to_string(),
    );
    assert!(m.toggle_breakpoint(0, 2));
    assert!(m.toggle_breakpoint(0, 4));
    assert!(m.toggle_breakpoint(0, 9));
    assert!(!m.toggle_breakpoint(0, 9));
    assert!(m.toggle_breakpoint(0, 9));
    assert_eq!(m.breakpoints(0), [2, 4, 9]);
    enter(&mut m, "RUN");
    assert_eq!(rows(&m).last().unwrap(), "OK");
    assert!(rows(&m).contains(&"Break on 0:2".to_string()));
    assert_eq!(m.paused_at(), Some((0, 2)));
    assert_eq!(global(&m, "A"), Some("Int(1)".to_string()));
    assert_eq!(global(&m, "B"), Some("Real(0.0)".to_string()));
    // A loop within the line stops there once; one over lines, each time.
    debug(&mut m, Resume::Cont);
    assert_eq!(m.paused_at(), Some((0, 4)));
    debug(&mut m, Resume::Cont);
    assert_eq!(m.paused_at(), Some((0, 4)));
    debug(&mut m, Resume::Cont);
    assert_eq!(m.paused_at(), Some((0, 9)));
    let calls = m.calls();
    assert_eq!(calls.len(), 2);
    assert_eq!(
        (calls[0].line, calls[1].line, &calls[1].kind),
        (6, 9, &CallKind::Gosub)
    );
    // CONT typed in direct mode passes the breakpoint too.
    enter(&mut m, "CONT");
    assert!(!m.debug(Resume::Cont));
    m.clear_breakpoints();
    assert!(m.breakpoints(0).is_empty());
    enter(&mut m, "RUN");
    assert_eq!(m.paused_at(), None);
}

#[test]
fn debugger_steps() {
    use sb_core::vm::{CallKind, Step};
    use sb_runtime::machine::Resume;
    let src = "STOP\nF 1\nY=2\nEND\nDEF F N\n X=N\n Z=3\nEND";
    let mut m = debug_machine();
    m.set_slot(0, src.to_string());
    enter(&mut m, "RUN");
    assert_eq!(m.paused_at(), Some((0, 2)));
    // Over the call; the line it stops before is named.
    debug(&mut m, Resume::Step(Step::Over));
    assert_eq!(m.paused_at(), Some((0, 3)));
    assert_eq!(rows(&m)[rows(&m).len() - 2], "Break on 0:3");

    enter(&mut m, "RUN");
    debug(&mut m, Resume::Step(Step::Into));
    assert_eq!(m.paused_at(), Some((0, 6)));
    let calls = m.calls();
    assert_eq!(calls.len(), 2);
    assert_eq!((calls[0].line, calls[0].frame), (2, None));
    assert_eq!(calls[1].kind, CallKind::Def("F".to_string()));
    assert_eq!((calls[1].line, calls[1].frame), (6, Some(0)));
    let locals = m.frame_locals(0);
    assert_eq!(locals[0].0, "N");
    assert_eq!(format!("{:?}", locals[0].1), "Int(1)");
    debug(&mut m, Resume::Step(Step::Over));
    assert_eq!(m.paused_at(), Some((0, 7)));
    debug(&mut m, Resume::Step(Step::Out));
    assert_eq!(m.paused_at(), Some((0, 3)));
    // Out of the top level, the program runs on to its end.
    debug(&mut m, Resume::Step(Step::Out));
    assert_eq!(m.paused_at(), None);
    assert_eq!(rows(&m).last().unwrap(), "OK");
}

#[test]
fn run_to_a_line_once() {
    use sb_runtime::machine::Resume;
    let src = "STOP\nF 1\nSTOP\nF 2\nEND\nDEF F N\n X=N\nEND";
    let mut m = debug_machine();
    m.set_slot(0, src.to_string());
    enter(&mut m, "RUN");
    assert_eq!(m.stop_reason(), Some(&StopReason::Stop));
    debug(&mut m, Resume::RunTo(0, 7));
    assert_eq!(m.paused_at(), Some((0, 7)));
    assert_eq!(m.stop_reason(), Some(&StopReason::RanTo));
    // The line is forgotten once reached.
    debug(&mut m, Resume::Cont);
    assert_eq!(m.paused_at(), Some((0, 4)));
    // ...or once something else stops the program first.
    enter(&mut m, "RUN");
    debug(&mut m, Resume::RunTo(0, 5));
    assert_eq!(m.paused_at(), Some((0, 4)));
    debug(&mut m, Resume::Cont);
    assert_eq!(m.paused_at(), None);
}

#[test]
fn conditional_breakpoints_stop_when_true() {
    use sb_core::vm::Step;
    use sb_runtime::machine::Resume;
    let mut m = debug_machine();
    let src = "FOR I=0 TO 9\n A=I\nNEXT\nF 3\nF 7\nEND\nDEF F N\n X=N\nEND";
    m.set_slot(0, src.to_string());
    m.set_condition(0, 2, "I==4 || I==8").unwrap();
    assert_eq!(m.condition(0, 2), Some("I==4 || I==8".to_string()));
    assert_eq!(m.condition(0, 3), None);
    enter(&mut m, "RUN");
    assert_eq!(m.paused_at(), Some((0, 2)));
    assert_eq!(global(&m, "I"), Some("Int(4)".to_string()));
    let condition = Some("I==4 || I==8".to_string());
    assert_eq!(m.stop_reason(), Some(&StopReason::Breakpoint(condition)));
    debug(&mut m, Resume::Cont);
    assert_eq!(global(&m, "I"), Some("Int(8)".to_string()));
    // A DEF's local, in a condition on one of its lines.
    m.toggle_breakpoint(0, 2);
    m.set_condition(0, 8, "N>5").unwrap();
    m.toggle_breakpoint(0, 4);
    debug(&mut m, Resume::Cont);
    assert_eq!(m.paused_at(), Some((0, 4)));
    // A step over a call whose breakpoint does not stop goes on as a step.
    assert_eq!(m.stop_reason(), Some(&StopReason::Breakpoint(None)));
    debug(&mut m, Resume::Step(Step::Over));
    assert_eq!(m.paused_at(), Some((0, 5)));
    assert_eq!(m.stop_reason(), Some(&StopReason::Stepped));
    debug(&mut m, Resume::Cont);
    assert_eq!(m.paused_at(), Some((0, 8)));
    assert_eq!(format!("{:?}", m.frame_locals(0)[0].1), "Int(7)");
    // An empty condition stops always; a failing one stops and says why.
    m.set_condition(0, 8, "").unwrap();
    assert_eq!(m.condition(0, 8), Some(String::new()));
    assert!(m.set_condition(0, 8, "N+").is_err());
    m.set_condition(0, 8, "Q$").unwrap();
    m.toggle_breakpoint(0, 4);
    enter(&mut m, "RUN");
    assert_eq!(m.paused_at(), Some((0, 8)));
    let r = rows(&m);
    assert!(
        r[r.len() - 2].starts_with("Condition failed: Undefined variable"),
        "{r:?}"
    );
    assert!(matches!(
        m.stop_reason(),
        Some(StopReason::ConditionFailed(_))
    ));
    // A jump leaves what stopped it behind; the end, any reason.
    assert!(m.jump_to(8));
    assert_eq!(m.stop_reason(), None);
    m.clear_breakpoints();
    debug(&mut m, Resume::Cont);
    assert_eq!(m.stop_reason(), None);
}

#[test]
fn watches_stop_where_their_value_changed() {
    use sb_runtime::machine::Resume;
    let mut m = debug_machine();
    m.set_slot(
        0,
        "DIM D[3]\nA=1\nFOR I=0 TO 9:B=I DIV 4:NEXT\nD[1]=5\nC$=\"X\"\nEND".to_string(),
    );
    m.set_changes(&["B", "D[1]"]);
    enter(&mut m, "RUN");
    // The loop within one line stops at the statement after the change.
    assert_eq!(m.paused_at(), Some((0, 3)));
    assert_eq!(global(&m, "I"), Some("Int(4)".to_string()));
    let r = rows(&m);
    assert_eq!(r[r.len() - 2], "B changed: 0 -> 1");
    let changed = StopReason::Changed {
        watch: "B".to_string(),
        from: "0".to_string(),
        to: "1".to_string(),
    };
    assert_eq!(m.stop_reason(), Some(&changed));
    debug(&mut m, Resume::Cont);
    assert_eq!(global(&m, "I"), Some("Int(8)".to_string()));
    debug(&mut m, Resume::Cont);
    assert_eq!(m.paused_at(), Some((0, 5)));
    let r = rows(&m);
    assert_eq!(r[r.len() - 2], "D[1] changed: 0 -> 5");
    // Direct mode's lines are not stopped by them.
    enter(&mut m, "B=7");
    assert_eq!(rows(&m).last().unwrap(), "OK");
    assert!(!rows(&m).iter().any(|r| r == "Break on 0:1"));
    // Taking the watches away lets the program run to its end.
    m.set_changes(&[]);
    debug(&mut m, Resume::Cont);
    assert_eq!(m.paused_at(), None);
    assert_eq!(rows(&m).last().unwrap(), "OK");
}

#[test]
fn steps_from_the_editor_leave_the_console_to_the_program() {
    use sb_core::vm::Step;
    use sb_runtime::machine::Resume;
    let mut m = debug_machine();
    m.set_slot(0, "STOP\nPRINT \"A\";\nPRINT \"B\";\nEND".to_string());
    enter(&mut m, "RUN");
    assert_eq!(&rows(&m)[rows(&m).len() - 2..], ["Break on 0:1", "OK"]);
    m.toggle_editor();
    debug(&mut m, Resume::Step(Step::Over));
    assert_eq!(m.paused_at(), Some((0, 3)));
    debug(&mut m, Resume::Step(Step::Over));
    assert_eq!(m.paused_at(), Some((0, 4)));
    // No Break line or OK between what the program printed.
    assert_eq!(rows(&m).last().unwrap(), "AB");
    // Back in direct mode, OK shows on a line of its own.
    m.toggle_editor();
    m.frame();
    assert_eq!(&rows(&m)[rows(&m).len() - 2..], ["AB", "OK"]);
    enter(&mut m, "CONT");
    assert_eq!(&rows(&m)[rows(&m).len() - 4..], ["AB", "OK", "cont", "OK"]);
}

#[test]
fn the_panel_shows_before_a_program_runs() {
    let mut m = debug_machine();
    enter(&mut m, "Q=5");
    m.set_slot(0, "A=1".to_string());
    m.toggle_editor();
    assert!(!m.panel_open());
    m.show_panel(true);
    assert!(m.panel_open());
    assert!(!m.panel_screens());
    // Direct mode's variables, as slot 0's.
    assert_eq!(global(&m, "Q"), Some("Int(5)".to_string()));
    m.set_condition(0, 1, "Q>1").unwrap();
    m.toggle_breakpoint(1, 3);
    let all = m.all_breakpoints();
    assert_eq!(all, [(0, 1, Some("Q>1".to_string())), (1, 3, None)]);
    m.show_panel(false);
    assert!(!m.panel_open());
}

#[test]
fn breakpoints_follow_edited_lines() {
    use sb_runtime::runtime::keys;
    let mut m = debug_machine();
    m.set_slot(0, "A=1\nB=2\nC=3\nD=4".to_string());
    m.toggle_breakpoint(0, 2);
    m.toggle_breakpoint(0, 4);
    enter(&mut m, "LIST 2");
    // A line broken at its start moves down with its text.
    press(&mut m, &[keys::HOME, keys::ENTER]);
    m.edit();
    assert_eq!(m.breakpoints(0), [3, 5]);
    // A line taken out takes its breakpoint along.
    m.edit().delete_line();
    assert_eq!(m.breakpoints(0), [4]);
    m.toggle_editor();
    enter(&mut m, "RUN");
    assert_eq!(m.paused_at(), Some((0, 4)));
}

#[test]
fn breakpoints_follow_prgins_and_prgdel() {
    let mut m = debug_machine();
    m.set_slot(1, "A=1\nB=2\nC=3\nD=4\nE=5".to_string());
    m.toggle_breakpoint(1, 2);
    m.set_condition(1, 3, "A>0").unwrap();
    m.toggle_breakpoint(1, 4);
    m.toggle_breakpoint(1, 5);
    // Two lines put in before line 2, line 6 (D) taken out, and line 1
    // typed over with two lines, which keeps its place.
    m.set_slot(
        0,
        "PRGEDIT 1,2:PRGINS \"X=0\"+CHR$(10)+\"W=0\"\nPRGEDIT 1,6:PRGDEL\n\
         PRGEDIT 1,1:PRGSET \"Y=1\"+CHR$(10)+\"Z=2\"\nSTOP"
            .to_string(),
    );
    enter(&mut m, "RUN");
    assert_eq!(m.slot(1), "Y=1\nZ=2\nX=0\nW=0\nB=2\nC=3\nE=5\n");
    let all = m.all_breakpoints();
    assert_eq!(
        all,
        [(1, 5, None), (1, 6, Some("A>0".to_string())), (1, 7, None)]
    );
    // The stopped program goes on with the code it had in slot 1.
    assert!(m.lines_moved(1));
    enter(&mut m, "CONT");
    enter(&mut m, "RUN 1");
    assert_eq!(m.paused_at(), Some((1, 5)));
    // A negative count takes every line, and their breakpoints.
    enter(&mut m, "CONT");
    m.set_slot(0, "PRGEDIT 1:PRGDEL -1".to_string());
    enter(&mut m, "RUN");
    assert!(m.breakpoints(1).is_empty());
}

#[test]
fn exec_gives_the_new_code_its_breakpoints() {
    let mut m = debug_machine();
    m.set_slot(1, "A=1\nB=2".to_string());
    m.toggle_breakpoint(1, 2);
    // A line put in before slot 1's first moves its breakpoint to line 3,
    // which EXEC runs at once.
    m.set_slot(0, "PRGEDIT 1:PRGINS \"Z=0\"\nEXEC 1".to_string());
    enter(&mut m, "RUN");
    assert_eq!(m.paused_at(), Some((1, 3)));
}

#[test]
fn breakpoints_go_with_the_program_new_replaces() {
    let mut m = debug_machine();
    m.set_slot(1, "A=1\nB=2".to_string());
    m.set_slot(2, "A=1\nB=2".to_string());
    m.toggle_breakpoint(1, 2);
    m.toggle_breakpoint(2, 2);
    enter(&mut m, "NEW 1");
    assert_eq!(m.all_breakpoints(), [(2, 2, None)]);
}

#[test]
fn jump_to_a_line_in_the_same_def() {
    use sb_runtime::machine::Resume;
    let src = "STOP\n?\"A\"\n?\"B\"\nF\nEND\nDEF F\n ?\"C\"\n STOP\n ?\"D\"\nEND";
    let mut m = debug_machine();
    m.set_slot(0, src.to_string());
    enter(&mut m, "RUN");
    assert!(m.jump_to(3));
    assert_eq!(m.paused_at(), Some((0, 3)));
    debug(&mut m, Resume::Cont);
    let r = rows(&m);
    assert_eq!(&r[r.len() - 4..], ["B", "C", "Break on 0:8", "OK"]);
    // Not out of the DEF, nor onto a line without code.
    assert!(!m.jump_to(3));
    assert!(!m.jump_to(6));
    assert!(m.jump_to(7));
    debug(&mut m, Resume::Cont);
    let r = rows(&m);
    assert_eq!(&r[r.len() - 3..], ["C", "Break on 0:8", "OK"]);
    // Nor once the program has ended.
    debug(&mut m, Resume::Cont);
    assert!(!m.jump_to(2));
}

#[test]
fn stopped_program_keeps_its_lines_after_edits() {
    use sb_runtime::machine::Resume;
    use sb_runtime::runtime::keys;
    let mut m = debug_machine();
    m.set_slot(0, "STOP\nA=1\nB=2\nC=3\nD=4".to_string());
    m.toggle_breakpoint(0, 4);
    enter(&mut m, "RUN");
    enter(&mut m, "LIST 1");
    press(&mut m, &[keys::HOME, keys::ENTER]);
    m.toggle_editor();
    // The text's breakpoint moved; the old code CONT goes on with keeps
    // its own, and lines of the text are not its lines.
    assert_eq!(m.breakpoints(0), [5]);
    assert!(m.lines_moved(0));
    assert!(!m.jump_to(3));
    assert!(!m.debug(Resume::RunTo(0, 3)));
    debug(&mut m, Resume::Cont);
    assert_eq!(m.paused_at(), Some((0, 4)));
    // A new run takes the text's.
    enter(&mut m, "CONT");
    enter(&mut m, "RUN");
    assert!(!m.lines_moved(0));
    debug(&mut m, Resume::Cont);
    assert_eq!(m.paused_at(), Some((0, 5)));
}

#[test]
fn steps_stop_at_the_next_line_from_anywhere() {
    use sb_core::vm::Step;
    use sb_runtime::machine::Resume;
    // From a breakpoint on a FOR, whose set-up starts no statement.
    let mut m = debug_machine();
    m.set_slot(0, "FOR I=0 TO 2\n A=I\n B=I\nNEXT".to_string());
    m.toggle_breakpoint(0, 1);
    enter(&mut m, "RUN");
    assert_eq!(m.paused_at(), Some((0, 1)));
    debug(&mut m, Resume::Step(Step::Into));
    assert_eq!(m.paused_at(), Some((0, 2)));
    // From a STOP within an IF.
    m.clear_breakpoints();
    m.set_slot(0, "IF 1 THEN STOP\nA=1\nB=2".to_string());
    enter(&mut m, "RUN");
    debug(&mut m, Resume::Step(Step::Into));
    assert_eq!(m.paused_at(), Some((0, 2)));
}

#[test]
fn breakpoints_where_the_program_goes_on_are_passed() {
    use sb_runtime::machine::Resume;
    let mut m = debug_machine();
    m.set_slot(0, "STOP\nA=1\n?\"X\"\nC=3".to_string());
    m.toggle_breakpoint(0, 3);
    enter(&mut m, "RUN");
    assert!(m.jump_to(3));
    debug(&mut m, Resume::Cont);
    let r = rows(&m);
    assert_eq!(&r[r.len() - 2..], ["X", "OK"]);
    // STOP just before one.
    m.set_slot(0, "STOP\n?\"Y\"".to_string());
    m.toggle_breakpoint(0, 2);
    enter(&mut m, "RUN");
    debug(&mut m, Resume::Cont);
    let r = rows(&m);
    assert_eq!(&r[r.len() - 2..], ["Y", "OK"]);
    // TRACE names the line it ran, as without breakpoints.
    m.set_slot(0, "STOP\nA=1\nB=2\nC=3".to_string());
    m.clear_breakpoints();
    m.toggle_breakpoint(0, 3);
    enter(&mut m, "RUN");
    enter(&mut m, "TRACE");
    assert_eq!(rows(&m)[rows(&m).len() - 2], "Break on 0:2");
    enter(&mut m, "TRACE");
    assert_eq!(rows(&m)[rows(&m).len() - 2], "Break on 0:3");
}

#[test]
fn undo_gives_breakpoints_back() {
    use sb_runtime::runtime::keys;
    let mut m = debug_machine();
    m.set_slot(0, "A=1\nB=2\nC=3".to_string());
    m.toggle_breakpoint(0, 2);
    enter(&mut m, "LIST 2");
    press(&mut m, &[keys::HOME]);
    m.edit();
    m.paste("X=1\nY=2");
    assert_eq!(m.breakpoints(0), [3]);
    m.edit().undo();
    assert_eq!(m.breakpoints(0), [2]);
}

#[test]
fn debugger_stops_open_the_editor() {
    use sb_core::vm::Step;
    use sb_runtime::machine::Resume;
    let mut m = machine();
    m.set_slot(0, "A=1\nB=2\nC=3\nD=4".to_string());
    m.toggle_breakpoint(0, 2);
    enter(&mut m, "RUN");
    // At the line it stopped before, with room for the panel.
    assert!(m.is_editing());
    assert_eq!(m.editor().cursor().0, 2);
    let h = m.render().height();
    assert!(m.panel_at().is_some_and(|at| at < h));
    // A step keeps the editor, which follows it.
    debug(&mut m, Resume::Step(Step::Over));
    assert!(m.is_editing());
    assert_eq!((m.paused_at(), m.editor().cursor().0), (Some((0, 3)), 3));
    // CONT shows the screen again.
    debug(&mut m, Resume::Cont);
    assert!(!m.is_editing());
    assert_eq!(rows(&m).last().unwrap(), "OK");
    assert_eq!(m.panel_at(), None);
    // Off, the stop stays in direct mode.
    m.rt.settings.debug_open_editor = false;
    enter(&mut m, "RUN");
    assert!(!m.is_editing());
    assert_eq!(m.paused_at(), Some((0, 2)));
    assert!(m.restart());
    for _ in 0..10 {
        m.frame();
    }
    assert_eq!(m.paused_at(), Some((0, 2)));
}

#[test]
fn steps_waiting_for_input_show_the_screen() {
    use sb_core::vm::Step;
    use sb_runtime::machine::Resume;
    use sb_runtime::runtime::keys;
    let mut m = machine();
    m.set_slot(
        0,
        "STOP\nINPUT X\n?X*2\nWAITKEY\n?\"K\"\nDEF WAITKEY\n REPEAT VSYNC UNTIL BUTTON(0)\nEND"
            .to_string(),
    );
    enter(&mut m, "RUN");
    m.edit_slot(0);
    assert!(m.debug(Resume::Step(Step::Into)));
    for _ in 0..3 {
        m.frame();
    }
    // INPUT takes its line on the screen; the stop after brings the
    // editor back.
    assert!(!m.is_editing());
    m.type_text("21");
    m.rt.key_press(keys::ENTER);
    for _ in 0..3 {
        m.frame();
    }
    assert!(m.is_editing());
    assert_eq!(m.paused_at(), Some((0, 3)));
    debug(&mut m, Resume::Step(Step::Into));
    assert_eq!(m.paused_at(), Some((0, 4)));
    // Over a DEF that waits for a button, once a frame has gone by.
    assert!(m.debug(Resume::Step(Step::Over)));
    for _ in 0..3 {
        m.frame();
    }
    assert!(!m.is_editing() && m.is_running());
    // A step into such a loop stays in the editor.
    m.stop();
    m.edit_slot(0);
    assert!(m.debug(Resume::Step(Step::Into)));
    for _ in 0..3 {
        m.frame();
    }
    assert!(m.is_editing());
}

#[test]
fn editor_marks_breakpoints_and_the_stopped_line() {
    let mut m = machine();
    m.set_slot(0, "A=1\nB=2\nC=3".to_string());
    m.toggle_breakpoint(0, 3);
    m.edit_slot(0);
    let plain: Vec<u32> = m.render().pixels().to_vec();
    m.toggle_editor();
    enter(&mut m, "RUN");
    let marked = m.render().pixels().to_vec();
    assert_ne!(plain, marked);
    assert_eq!(m.editor().marks.breakpoints, [2]);
    assert_eq!(m.editor().marks.stopped, Some(2));
}

#[test]
fn watches_and_changed_variables() {
    use sb_core::value::Value;
    use sb_runtime::machine::Resume;
    let mut m = debug_machine();
    m.set_slot(
        0,
        "A=1\nSTOP\n?A\nF 2\nDEF F N\n STOP\n ?N*10\nEND".to_string(),
    );
    enter(&mut m, "RUN");
    let shown = |v: Result<Value, _>| format!("{:?}", v.unwrap());
    assert_eq!(shown(m.watch("A*3", None, 0)), "Int(3)");
    assert!(m.set_variable("A", Value::Int(7), None, 0));
    assert!(!m.set_variable("NOPE", Value::Int(7), None, 0));
    debug(&mut m, Resume::Cont);
    assert!(rows(&m).contains(&"7".to_string()));
    // In the DEF, its locals come before the globals.
    let frame = m.calls().last().unwrap().frame;
    assert_eq!(shown(m.watch("N+A", frame, 0)), "Int(9)");
    assert!(m.set_variable("N", Value::Int(5), frame, 0));
    debug(&mut m, Resume::Cont);
    assert!(rows(&m).contains(&"50".to_string()));
}

#[test]
fn the_panel_closes_until_the_next_stop() {
    use sb_core::vm::Step;
    use sb_runtime::machine::Resume;
    let mut m = machine();
    m.set_slot(0, "A=1\nB=2\nC=3".to_string());
    m.toggle_breakpoint(0, 1);
    enter(&mut m, "RUN");
    m.render();
    assert!(m.panel_at().is_some());
    m.show_panel(false);
    m.render();
    assert_eq!((m.panel_at(), m.editor().marks.stopped), (None, None));
    // The program is still stopped, and the next stop shows them again.
    debug(&mut m, Resume::Step(Step::Into));
    m.render();
    assert!(m.panel_at().is_some());
    assert_eq!(m.editor().marks.stopped, Some(1));
}

/// A machine whose editor shows apart from the screens.
fn detached_machine() -> Machine {
    let mut m = debug_machine();
    m.set_detached(true);
    m
}

#[test]
fn detached_editor_fills_the_area_it_is_given() {
    use sb_runtime::editor::Area;
    use sb_runtime::runtime::keys;
    let mut m = detached_machine();
    let text: String = (1..=200).map(|i| format!("A={i}\n")).collect();
    m.set_slot(0, text);
    m.edit_slot(0);
    // Page Down goes down from line 1 by the rows shown.
    let page = |m: &mut Machine, area: Area| {
        m.set_editor_area(Some(area));
        m.edit().file_top();
        let fb = m.render_editor();
        assert_eq!((fb.width(), fb.height()), area.size);
        m.edit().key(keys::PAGE_DOWN);
        m.editor().cursor().0
    };
    let small = page(
        &mut m,
        Area {
            size: (400, 300),
            top: 12,
        },
    );
    let tall = page(
        &mut m,
        Area {
            size: (400, 600),
            top: 12,
        },
    );
    // 16-pixel rows, less the menu bar and the ruler and status line.
    assert_eq!(
        (small, tall),
        (1 + (300 - 12) / 16 - 2, 1 + (600 - 12) / 16 - 2)
    );
    // A frame too small for any row draws nothing, split or not.
    m.toggle_split();
    for size in [(1, 1), (3, 40), (40, 3)] {
        m.set_editor_area(Some(Area { size, top: 30 }));
        m.render_editor();
    }
    // In place of the screens, it has SmileBASIC 4's rows again.
    m.set_detached(false);
    let fb = m.render_editor();
    assert_ne!((fb.width(), fb.height()), (40, 3));
}

#[test]
fn detached_editor_opens_while_a_program_runs() {
    let mut m = detached_machine();
    m.set_slot(0, "@L\nVSYNC\nGOTO @L".to_string());
    enter_no_wait(&mut m, "RUN");
    m.edit_slot(0);
    assert!(m.is_editing() && m.is_running());
    m.frame();
    assert!(m.is_running());
    // Put back in place of the screens, it closes.
    m.set_detached(false);
    assert!(!m.is_editing() && m.is_running());
    // ...and cannot open again until the program stops.
    m.toggle_editor();
    assert!(!m.is_editing());
}

#[test]
fn detached_editor_takes_only_its_own_keys() {
    let mut m = detached_machine();
    m.set_slot(0, "A=1".to_string());
    m.edit_slot(0);
    // Without the keys, typing goes to direct mode.
    enter(&mut m, "?6*7");
    assert!(rows(&m).iter().any(|r| r == "42"));
    assert_eq!(m.editor().text(), "A=1");
    m.focus_editor(true);
    // Unshifted letters type capitals.
    m.type_text("b");
    m.frame();
    assert_eq!(m.editor().text(), "BA=1");
}

#[test]
fn detached_editor_edits_run_without_closing_it() {
    let mut m = detached_machine();
    m.set_slot(0, "?\"OLD\"".to_string());
    m.edit_slot(0);
    m.focus_editor(true);
    m.edit().select_all();
    m.paste("?\"NEW\"");
    m.focus_editor(false);
    enter(&mut m, "RUN");
    let r = rows(&m);
    assert_eq!(&r[r.len() - 2..], ["NEW", "OK"]);
    assert!(m.is_editing());
    assert_eq!(m.slot(0), "?\"NEW\"");
    assert!(m.unsaved_slots().contains(&0));
}

#[test]
fn detached_editor_takes_a_programs_changes_to_its_slot() {
    let mut m = detached_machine();
    m.set_slot(0, "A=1".to_string());
    m.set_slot(1, "PRGEDIT 0\nPRGSET \"B=2\"".to_string());
    m.edit_slot(0);
    enter(&mut m, "RUN 1");
    m.frame();
    assert!(m.slot(0).starts_with("B=2"));
    assert_eq!(m.editor().text(), m.slot(0));
}

#[test]
fn detached_editor_stays_open_on_cont() {
    use sb_runtime::machine::Resume;
    let mut m = detached_machine();
    m.set_slot(0, "STOP\n?\"GO\"".to_string());
    enter(&mut m, "RUN");
    m.edit_slot(0);
    debug(&mut m, Resume::Cont);
    let r = rows(&m);
    assert_eq!(&r[r.len() - 2..], ["GO", "OK"]);
    assert!(m.is_editing());
}

#[test]
fn detached_editor_stays_open_while_a_step_waits_for_input() {
    use sb_core::vm::Step;
    use sb_runtime::machine::Resume;
    use sb_runtime::runtime::keys;
    let mut m = detached_machine();
    m.set_slot(0, "STOP\nINPUT X\n?X*2".to_string());
    enter(&mut m, "RUN");
    m.edit_slot(0);
    assert!(!m.step_wants_keys());
    assert!(m.debug(Resume::Step(Step::Into)));
    for _ in 0..3 {
        m.frame();
    }
    assert!(m.is_editing() && m.step_wants_keys());
    m.type_text("21");
    m.rt.key_press(keys::ENTER);
    for _ in 0..3 {
        m.frame();
    }
    assert_eq!(m.paused_at(), Some((0, 3)));
    assert!(!m.step_wants_keys());
    // The next step does not wait.
    debug(&mut m, Resume::Step(Step::Into));
    assert!(!m.step_wants_keys());
}

/// Types a line and presses Enter, running one frame for it.
fn enter_no_wait(m: &mut Machine, line: &str) {
    m.type_text(line);
    m.rt.key_press(sb_runtime::runtime::keys::ENTER);
    m.frame();
}

#[test]
fn a_program_that_ends_can_leave_a_clean_screen_and_silence() {
    let ended = |tidy: bool| {
        let mut m = machine();
        m.rt.settings.tidy_on_end = tidy;
        m.set_slot(0, "BACKCOLOR #C_RED\nBEEP 1\nBGMPLAY 1\n".to_string());
        enter(&mut m, "RUN");
        let playing = !m.rt.sound.inspect_tracks().is_empty();
        (m.rt.back_color, playing)
    };
    let (color, playing) = ended(false);
    assert_ne!(color, 0xFF00_0000);
    assert!(playing);
    assert_eq!(ended(true), (0xFF00_0000, false));
}

/// What follows RETURN in a command DEF is a statement; a list of values is
/// none (TEST62), while `RETURN X,Y` is a command X (KRB_OGIRI_SMBOMB).
#[test]
fn return_values_in_a_command_def_when_compiling() {
    let run = |source: &str| {
        let mut m = machine();
        m.set_slot(0, source.to_string());
        enter(&mut m, "RUN");
        rows(&m)
    };
    let r = run("?\"RUNS\"\nDEF F OUT A,B\n RETURN 1,2\nEND\nF OUT X,Y\n");
    assert!(!r.iter().any(|l| l == "RUNS"), "{r:?}");
    assert!(
        r.iter().any(|l| l.starts_with("Syntax error in 0:3")),
        "{r:?}"
    );
    let r = run("DEF G CODE OUT X,Y\n RETURN X,Y\nEND\nG 1 OUT A,B\n?\"RUNS\"\n");
    assert!(r.iter().any(|l| l == "RUNS"), "{r:?}");
}

#[test]
fn a_prompt_drops_the_keys_of_the_line_before_it() {
    let mut m = machine();
    enter(&mut m, "PUSHKEY \"?1\"+CHR$(13)+\"?2\"+CHR$(13)");
    for _ in 0..10 {
        m.frame();
    }
    let r = rows(&m);
    assert!(!r.iter().any(|l| l == "?1"), "{r:?}");
}

/// SNDSTOP right after VSYNC takes a frame and puts SNDMVOL back at 88, as
/// RUN does.
#[test]
fn sndstop_takes_a_frame_and_run_resets_sndmvol() {
    let mut m = machine();
    m.set_slot(
        0,
        "SNDMVOL 32:VSYNC:T=MAINCNT():SNDSTOP:?MAINCNT()-T;SNDMVOL():SNDMVOL 40\n".to_string(),
    );
    enter(&mut m, "RUN");
    assert!(rows(&m).iter().any(|l| l == "188"), "{:?}", rows(&m));
    enter(&mut m, "?SNDMVOL()");
    assert!(rows(&m).iter().any(|l| l == "40"), "{:?}", rows(&m));
    m.set_slot(0, "?SNDMVOL()+1000\n".to_string());
    enter(&mut m, "RUN");
    assert!(rows(&m).iter().any(|l| l == "1088"), "{:?}", rows(&m));
}

/// TEST62 on a Switch 2: RUN leaves VSYNC's count where it was, so a first
/// VSYNC of any size finds its frames gone by.
#[test]
fn a_first_vsync_does_not_wait() {
    for n in [5, 2, 1] {
        let mut m = machine();
        enter(&mut m, "?1");
        for _ in 0..10 {
            m.frame();
        }
        m.set_slot(0, format!("T=MAINCNT():VSYNC {n}:?MAINCNT()-T\n"));
        enter(&mut m, "RUN");
        let r = rows(&m);
        assert!(r.iter().any(|l| l == "0"), "VSYNC {n}: {r:?}");
    }
    // At the very start too, with no VSYNC before it.
    let mut m = machine();
    m.set_slot(0, "T=MAINCNT():VSYNC 5:?MAINCNT()-T\n".to_string());
    m.run_slot(0);
    for _ in 0..10 {
        m.frame();
    }
    assert!(rows(&m).iter().any(|l| l == "0"), "{:?}", rows(&m));
}

/// TEST62 and TEST56 on real SB4: a program goes on from XSCREEN while the frame
/// job runs, the job's update behind it and MAINCNT's rise ahead of it. So an
/// animation started there shows on the third VSYNC 1, with a first VSYNC from
/// long ago; with one just before, on the second (TEST60).
#[test]
fn an_animation_started_after_xscreen() {
    let rows_of = |source: &str| {
        let mut m = machine();
        enter(&mut m, "?1");
        for _ in 0..10 {
            m.frame();
        }
        m.set_slot(0, source.to_string());
        enter(&mut m, "ACLS");
        enter(&mut m, "RUN");
        rows(&m)
    };
    const LOOP: &str = "D=ARRAY#(3):D[0]=-3:D[1]=3:D[2]=7:SPANIM 0,\"XY\",D\n\
        FOR I=1 TO 6:VSYNC 1:SPOFS 0 OUT K,M:?\"F\";I;\" \";K;\" \";M:NEXT\n";
    let r = rows_of(&format!(
        "ACLS\nXSCREEN 400,240,2\nSPSET 0,0,0,16,16,0\n{LOOP}"
    ));
    for row in ["F1 0 0", "F2 0 0", "F3 1 2", "F4 2 4", "F5 3 7", "F6 3 7"] {
        assert!(r.iter().any(|l| l == row), "{row}: {r:?}");
    }
    let r = rows_of(&format!(
        "SPSET 0,0,0,16,16,0\nVSYNC 1\nXSCREEN 400,240,2\nSPSET 0,0,0,16,16,0\n{LOOP}"
    ));
    for row in ["F1 0 0", "F2 1 2", "F3 2 4", "F4 3 7"] {
        assert!(r.iter().any(|l| l == row), "{row}: {r:?}");
    }
}

/// TEST63 on a Switch 2: MAINCNT has yet to rise when XSCREEN or ACLS gives the
/// program back, and WAIT 1 or VSYNC 1 then only sees it rise.
#[test]
fn maincnt_reads_inside_the_frame_job() {
    let printed = |source: &str| {
        let mut m = machine();
        m.set_slot(0, format!("{source}\n"));
        m.run_slot(0);
        for _ in 0..100 {
            m.frame();
            if !m.is_running() {
                break;
            }
        }
        rows(&m)
            .into_iter()
            .find(|row| !row.is_empty())
            .unwrap_or_default()
    };
    for (source, want) in [
        ("T=MAINCNT():ACLS:?MAINCNT()-T", "0"),
        ("VSYNC:T=MAINCNT():ACLS:?MAINCNT()-T", "0"),
        ("VSYNC:T=MAINCNT():ACLS:ACLS:?MAINCNT()-T", "1"),
        ("VSYNC:ACLS:T=MAINCNT():WAIT 1:?MAINCNT()-T", "1"),
        ("T=MAINCNT():XSCREEN 400,240,2:?MAINCNT()-T", "0"),
        ("VSYNC:T=MAINCNT():XSCREEN 400,240,2:?MAINCNT()-T", "0"),
        (
            "VSYNC:XSCREEN 400,240,2:T=MAINCNT():WAIT 1:U=MAINCNT():WAIT 1:?U-T;MAINCNT()-U",
            "11",
        ),
        (
            "VSYNC:XSCREEN 400,240,2:T=MAINCNT():VSYNC 1:U=MAINCNT():VSYNC 1:?U-T;MAINCNT()-U",
            "11",
        ),
    ] {
        assert_eq!(printed(source), want, "{source}");
    }
}

/// How often the screens were drawn again over `frames` frames of the
/// program, and whether the last frame given matches one drawn afresh.
fn redraws(program: &str, frames: u32) -> (u32, bool) {
    let mut m = machine();
    m.set_slot(0, program.to_string());
    m.run_slot(0);
    // The first frames set the screens up.
    for _ in 0..3 {
        m.frame();
        m.render();
    }
    let mut drawn = 0;
    for _ in 0..frames {
        let before = m.screen_revision();
        m.frame();
        m.render();
        drawn += u32::from(m.screen_revision() != before);
    }
    assert!(m.is_running());
    let kept = m.screen_frame().pixels().to_vec();
    m.rt.touch();
    m.render();
    (drawn, m.screen_frame().pixels() == &kept[..])
}

#[test]
fn screens_are_drawn_again_only_when_changed() {
    // Writing what is already there changes nothing.
    let same = "SPSET 0,1:SPOFS 0,10,10\n\
                @L:CLS:LOCATE 2,3:COLOR 7:PRINT \"SAME\"\n\
                SPOFS 0,10,10:SPCOLOR 0,-1:X=SPROT(0):VSYNC:GOTO @L";
    assert_eq!(redraws(same, 10), (0, true));
    let moving = "SPSET 0,1\n@L:SPOFS 0,MAINCNT() MOD 100,10:VSYNC:GOTO @L";
    assert_eq!(redraws(moving, 10), (10, true));
    let printing = "@L:LOCATE 0,0:PRINT MAINCNT():VSYNC:GOTO @L";
    assert_eq!(redraws(printing, 10), (10, true));
    let drawing = "@L:GPSET MAINCNT() MOD 100,0:VSYNC:GOTO @L";
    assert_eq!(redraws(drawing, 10), (10, true));
}

/// What a fixed run prints, a few frames in.
fn fixed_run(source: &str) -> Vec<String> {
    let mut rt = Runtime::for_tests(Font::blank());
    rt.fix_time_and_chance();
    let mut m = Machine::new(rt);
    m.boot();
    m.set_slot(0, source.to_string());
    m.run_slot(0);
    for _ in 0..300 {
        m.frame();
    }
    let console = &m.rt.text[sb_runtime::text::CONSOLE];
    let (w, h) = console.size();
    (0..h)
        .map(|y| {
            let codes: Vec<u16> = (0..w)
                .map(|x| console.cell(x, y).code)
                .take_while(|&c| c != 0)
                .collect();
            String::from_utf16_lossy(&codes)
        })
        .filter(|row| !row.is_empty())
        .collect()
}

/// Runs that are to be compared must do the same work every time.
#[test]
fn fixed_runs_repeat_time_and_chance() {
    let source = "VSYNC:VSYNC 120\n\
        ?RND(1000);\" \";RND(1,1000);\" \";RNDF()\n\
        RANDOMIZE 2,0\n\
        ?RND(2,1000)\n\
        ?MILLISEC();\" \";MAINCNT();\" \";PERFEND(0)\n\
        ?DATE$();\" \";TIME$()\n\
        WHILE 1:VSYNC:WEND\n";
    let first = fixed_run(source);
    assert_eq!(first, fixed_run(source));
    assert!(
        first.iter().any(|r| r == "2026/01/01 00:00:02"),
        "{first:?}"
    );
    assert!(
        first.iter().any(|r| r.starts_with("2016 121 0")),
        "{first:?}"
    );
}
