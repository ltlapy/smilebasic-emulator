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
