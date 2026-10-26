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
    assert!(r[11].starts_with("Illegal statement"), "{r:?}");
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
    assert_eq!(&r[r.len() - 2..], ["Break in 1", "OK"]);
    // Variables of the stopped code stay for direct mode.
    enter(&mut m, "?i>0");
    assert_eq!(rows(&m)[rows(&m).len() - 2], "1");
    // Stopping in direct mode does nothing.
    m.stop();
    assert_eq!(rows(&m).last().unwrap(), "OK");
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
    assert_eq!(m.editor.cursor(), (2, 0));
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
    assert_eq!(m.editor.slot(), 2);
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
    assert_eq!((m.editor.slot(), m.editor.cursor()), (1, (3, 4)));
    // The error is shown once; later, the editor opens where it was left.
    m.toggle_editor();
    // So do syntax errors (checked on hardware: before the `?`).
    m.set_slot(0, "A=1:?(".to_string());
    enter(&mut m, "run");
    enter(&mut m, "list err");
    assert_eq!((m.editor.slot(), m.editor.cursor()), (0, (1, 4)));
    m.toggle_editor();
    m.toggle_editor();
    assert_eq!(m.editor.slot(), 0);
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
    assert_eq!(m.editor.slot(), 3);
    m.type_text("'");
    m.frame();
    // Switching keeps the edit and opens the other slot.
    m.edit_slot(1);
    assert_eq!((m.editor.slot(), m.slot(3)), (1, "'?3"));
    assert_eq!(m.unsaved_slots(), [3]);
    m.edit_slot(6);
    assert_eq!(m.editor.slot(), 1, "no such slot");
}
