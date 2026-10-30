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
    m.toggle_split();
    assert!(!m.is_split());
}

#[test]
fn direct_mode_completes_words() {
    use sb_runtime::runtime::keys;
    let mut m = machine();
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
    use sb_runtime::editor::FileRequest;
    use sb_runtime::runtime::keys;
    use sb_runtime::storage::Storage;
    let dir = std::env::temp_dir().join(format!("sb-simple-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let mut m = machine();
    m.rt.storage = Storage::new(dir.clone());
    m.set_slot(1, "?1".to_string());
    m.edit_slot(1);
    m.type_text("'");
    m.frame();
    // No name yet: typed in the status line.
    m.prompt_file(true);
    m.type_text("game");
    m.rt.key_press(keys::ENTER);
    m.frame();
    let Some(FileRequest::Save(name)) = m.take_file_request() else {
        panic!("no request");
    };
    assert_eq!(name, "GAME");
    m.save_edited(&name).unwrap();
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
