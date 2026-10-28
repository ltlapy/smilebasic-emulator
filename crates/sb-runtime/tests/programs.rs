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
