//! Programs reading and changing other slots: the PRG commands.

use sb_runtime::Runtime;
use sb_runtime::font::Font;
use sb_runtime::machine::Machine;
use sb_runtime::runtime::keys;

fn machine() -> Machine {
    let mut m = Machine::new(Runtime::new(400, 240, Font::blank()));
    m.boot();
    m
}

fn enter(m: &mut Machine, line: &str) {
    m.type_text(line);
    m.rt.key_press(keys::ENTER);
    for _ in 0..100 {
        m.frame();
        if !m.is_running() {
            return;
        }
    }
    panic!("{line:?} did not finish");
}

/// The console's last `n` lines, with PRINT's tab stops as single spaces.
fn tail(m: &Machine, n: usize) -> Vec<String> {
    let console = &m.rt.text[4];
    let (w, h) = console.size();
    let rows: Vec<String> = (0..h)
        .map(|y| {
            let codes: Vec<u16> = (0..w)
                .map(|x| console.cell(x, y).code)
                .take_while(|&c| c != 0)
                .collect();
            let row = String::from_utf16(&codes).unwrap();
            row.split_whitespace().collect::<Vec<_>>().join(" ")
        })
        .filter(|r| !r.is_empty())
        .collect();
    rows[rows.len() - n..].to_vec()
}

fn run(m: &mut Machine, program: &[&str]) {
    m.set_slot(0, program.join("\n"));
    enter(m, "run");
}

#[test]
fn reads_lines_in_turn() {
    let mut m = machine();
    m.set_slot(1, "A\nB\nC".to_string());
    run(
        &mut m,
        &[
            "PRGEDIT 1:?PRGSIZE(1),PRGGET$(),PRGGET$(),PRGEDIT()",
            "?PRGGET$(0),PRGGET$(),\"[\"+PRGGET$()+\"]\",PRGSEEK(1)",
            "PRGEDIT 1,-1:PRGEDIT OUT S,L:?S,L,PRGSEEK(1),PRGSEEK(-1),PRGGET$(0)",
            "?PRGSIZE(1,1),PRGNAME$(1)==\"\"",
        ],
    );
    assert_eq!(
        tail(&m, 5),
        ["3 A B 3", "C C [] 0", "1 3 0 1 B", "5 1", "OK"]
    );
}

#[test]
fn changes_lines() {
    let mut m = machine();
    m.set_slot(2, "1\n2\n3".to_string());
    run(
        &mut m,
        &[
            "PRGEDIT 2,2:PRGSET \"TWO\"",
            "PRGINS \"X\"+CHR$(10)+\"Y\":PRGINS \"Z\",1",
            "PRGEDIT 2,1:PRGDEL",
            "PRGEDIT 2,-1:A$=PRGGET$():PRGSET \"END\"",
        ],
    );
    assert_eq!(m.slot(2), "X\nY\nTWO\nZ\n3\nEND");
    assert!(m.unsaved_slots().contains(&2));
    run(&mut m, &["PRGEDIT 2,3:PRGDEL -1:PRGEDIT 2:PRGDEL 9"]);
    assert_eq!(m.slot(2), "");
}

#[test]
fn errors() {
    let mut m = machine();
    run(&mut m, &["?PRGGET$()"]);
    assert!(tail(&m, 2)[0].starts_with("Illegal function call"));
    run(&mut m, &["PRGEDIT 0"]);
    assert!(tail(&m, 2)[0].starts_with("Illegal function call"));
    run(&mut m, &["PRGEDIT 1,2"]);
    assert!(tail(&m, 2)[0].starts_with("Out of range"));
    // Running a program ends the editing.
    enter(&mut m, "prgedit 1");
    run(&mut m, &["?PRGEDIT()"]);
    assert!(tail(&m, 2)[0].starts_with("Illegal function call"));
}

#[test]
fn surroundings_of_the_main_interpreter() {
    let mut m = machine();
    enter(&mut m, "a=1");
    run(
        &mut m,
        &[
            "ENVTYPE OUT T,P:?ENVTYPE(),T,P,ENVSTAT(),ENVFOCUS()",
            "ENVSTAT OUT S,E,D:?S,E,D",
            "?ENVINPUT$(1),ENVINPUT$(2),\"[\"+ENVINPUT$(32)+\"]\"",
            "CLIPBOARD \"COPIED\":?CLIPBOARD()",
            "ENVPROJECT:ENVSTAT 0:ENVFOCUS 1",
        ],
    );
    assert_eq!(
        tail(&m, 5),
        ["0 0 -1 3 1", "3 0 0", "RUN A=1 []", "COPIED", "OK"]
    );
    enter(&mut m, "?envstat()");
    assert_eq!(tail(&m, 2), ["0", "OK"]);
    run(&mut m, &["ENVLOAD 0,1,1"]);
    assert!(tail(&m, 2)[0].starts_with("Illegal function call"));
}

/// Starts a program and runs frames until it waits on a dialog.
fn until_dialog(m: &mut Machine, program: &str) -> sb_runtime::dialog::ProgramDialog {
    m.set_slot(0, program.to_string());
    m.type_text("run");
    m.rt.key_press(keys::ENTER);
    for _ in 0..10 {
        m.frame();
        if let Some(d) = m.rt.program_dialog() {
            return d.clone();
        }
    }
    panic!("no dialog");
}

fn settle(m: &mut Machine) {
    for _ in 0..10 {
        m.frame();
    }
    assert!(!m.is_running());
}

#[test]
fn dialogs_wait_for_an_answer() {
    use sb_runtime::dialog::{DialogAnswer, DialogKind};
    let mut m = machine();
    let d = until_dialog(&mut m, "DIALOG \"Hello\"+CHR$(10)+\"there\",1:?RESULT()");
    assert_eq!(d.caption, "DIALOG");
    assert_eq!(d.lines, ["Hello", "there"]);
    assert_eq!(d.kind, DialogKind::Choice(vec!["No", "Yes"]));
    m.rt.answer_dialog(DialogAnswer::Choice(0));
    settle(&mut m);
    assert_eq!(tail(&m, 2), ["-1", "OK"]);

    // A timeout in frames.
    until_dialog(&mut m, "DIALOG \"X\",0,\"Cap\",-3:?RESULT()");
    settle(&mut m);
    assert_eq!(tail(&m, 2), ["0", "OK"]);

    // Waiting for buttons: B is pressed, but only A and B count.
    until_dialog(&mut m, "?DIALOG(\"Press\",&B1010)");
    m.rt.input.live.pads[0].buttons = 1 << 1;
    settle(&mut m);
    assert_eq!(tail(&m, 2), ["2", "OK"]);
    m.rt.input.live.pads[0].buttons = 0;

    let d = until_dialog(&mut m, "N$=DIALOG(\"NAME0\",\"SAVE\",14):?N$,RESULT()");
    assert_eq!(
        d.kind,
        DialogKind::Name {
            initial: "NAME0".to_string(),
            max: 14
        }
    );
    m.rt.answer_dialog(DialogAnswer::Name(Some("GAME".to_string())));
    settle(&mut m);
    assert_eq!(tail(&m, 2), ["GAME 1", "OK"]);

    run(&mut m, &["?DIALOG(\"A B\",\"X\")"]);
    assert!(tail(&m, 2)[0].starts_with("Illegal function call"));
    run(&mut m, &["?DIALOG(\"A\",0)"]);
    assert!(tail(&m, 2)[0].starts_with("Out of range"));
}
