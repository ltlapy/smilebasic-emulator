//! Direct mode and program slots, driven through typed keys.

use sb_runtime::Runtime;
use sb_runtime::font::Font;
use sb_runtime::machine::Machine;

fn machine() -> Machine {
    let mut m = Machine::new(Runtime::new(400, 240, Font::blank()));
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
    let r = rows(&m);
    assert_eq!(r[r.len() - 3], "X");
    assert!(last(&m).starts_with("Duplicate variable"));
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
    enter(&mut m, "x=41:run 1");
    enter(&mut m, "?x");
    let r = rows(&m);
    // RUN starts with fresh variables and leaves its own for direct mode.
    assert_eq!(&r[r.len() - 5..], ["RAN1", "OK", "?X", "1", "OK"]);
    enter(&mut m, "new 1");
    assert_eq!(m.slot(1), "");
}

#[test]
fn errors_return_to_direct_mode() {
    let mut m = machine();
    enter(&mut m, "?1/0");
    enter(&mut m, "?(");
    m.set_slot(0, "RUN".to_string());
    enter(&mut m, "run");
    let r = rows(&m);
    assert!(r[5].starts_with("Divide by zero"), "{r:?}");
    assert_eq!(r[6], "OK");
    assert_eq!(
        r[8], "Syntax error in 1 (expected expression, found Eof)",
        "{r:?}"
    );
    assert_eq!(r[10], "RUN");
    assert!(r[11].starts_with("Syntax error"), "{r:?}");
    assert_eq!(r[12], "OK");
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
    let src = "ACLS:?\"PART? 5\"\nFOR I=0 TO 9:?\"LINE\";I:NEXT\nDIM LB%[60]\n??LB%";
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
    debug(&mut m, Resume::RunTo(0, 7));
    assert_eq!(m.paused_at(), Some((0, 7)));
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
