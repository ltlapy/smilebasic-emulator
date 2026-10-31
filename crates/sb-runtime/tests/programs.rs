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

/// Runs `ops` on an emptied slot 3 and gives PRGSIZE's lines and
/// characters, PRGEDIT() and the source, as sb4_test07 printed them on
/// hardware.
fn prg_state(m: &mut Machine, ops: &str) -> (String, String) {
    run(
        m,
        &[
            &format!("PRGEDIT 3:PRGDEL -1:{ops}"),
            "A=PRGSIZE(3):B=PRGSIZE(3,1):C=PRGEDIT():?A,B,C",
        ],
    );
    (tail(m, 2)[0].clone(), m.slot(3).to_string())
}

#[test]
fn lines_as_on_hardware() {
    let mut m = machine();
    let state = |m: &mut Machine, ops: &str| prg_state(m, ops);
    let cases: &[(&str, &str, &str)] = &[
        ("", "0 1 1", ""),
        ("PRGSET \"A\"", "2 3 2", "A\n\n"),
        ("PRGINS \"A\"", "2 3 2", "A\n\n"),
        ("PRGINS \"A\",1", "2 3 2", "\nA\n"),
        ("PRGSET \"A\":PRGSET \"B\"", "3 5 3", "A\nB\n\n"),
        ("PRGINS \"A\"+CHR$(10)+\"B\"", "3 5 3", "A\nB\n\n"),
        ("PRGSET \"A\"+CHR$(10)", "3 4 3", "A\n\n\n"),
        (
            "PRGINS \"A\":PRGINS \"B\":PRGINS \"C\"",
            "4 7 4",
            "A\nB\nC\n\n",
        ),
        (
            "PRGINS \"A\",1:PRGINS \"B\",1:PRGINS \"C\",1",
            "4 7 4",
            "\nA\nB\nC\n",
        ),
        (
            "PRGINS \"1\",1:PRGINS \"2\",1:PRGINS \"3\",1:PRGEDIT 3,1:PRGINS \"ZERO\"",
            "5 12 2",
            "ZERO\n\n1\n2\n3\n",
        ),
    ];
    for &(ops, numbers, text) in cases {
        let (got, source) = state(&mut m, ops);
        assert_eq!((got.as_str(), source.as_str()), (numbers, text), "{ops}");
    }
    // Replacing a line that has text, deleting, and reading past the end.
    let build = "PRGINS \"A\",1:PRGINS \"B\",1:PRGINS \"C\",1:PRGEDIT 3,2:PRGSET \"X\"";
    assert_eq!(state(&mut m, build), ("4 7 3".into(), "\nX\nB\nC\n".into()));
    let build = format!("{build}:PRGEDIT 3,2:PRGDEL");
    assert_eq!(state(&mut m, &build), ("3 5 2".into(), "\nB\nC\n".into()));
    run(
        &mut m,
        &[
            "PRGEDIT 3,2:A$=PRGGET$():B$=PRGGET$():C=PRGEDIT()",
            "?LEN(A$),ASC(B$),C,\"[\"+PRGGET$(0)+\"]\"",
            "PRGEDIT 3,-1:S=PRGSEEK(1):?PRGEDIT(),S",
        ],
    );
    assert_eq!(tail(&m, 3), ["2 67 3 []", "3 0", "OK"]);
    // An empty slot reads as one empty line, then nothing.
    run(
        &mut m,
        &["PRGEDIT 3:PRGDEL -1:A$=PRGGET$():B$=PRGGET$():?ASC(A$),LEN(B$),PRGEDIT()"],
    );
    assert_eq!(tail(&m, 2), ["10 0 1", "OK"]);
    assert!(m.unsaved_slots().contains(&3));
}

#[test]
fn errors() {
    let mut m = machine();
    run(&mut m, &["?PRGGET$()"]);
    assert!(tail(&m, 2)[0].starts_with("Use PRGEDIT before any PRG function"));
    run(&mut m, &["PRGEDIT 0"]);
    assert!(tail(&m, 2)[0].starts_with("Can't edit running program"));
    run(&mut m, &["PRGEDIT 1,2"]);
    assert!(tail(&m, 2)[0].starts_with("Out of range"));
    // Running a program ends the editing.
    enter(&mut m, "prgedit 1");
    run(&mut m, &["?PRGEDIT()"]);
    assert!(tail(&m, 2)[0].starts_with("Use PRGEDIT"));
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
        ["0 0 0 3 1", "3 0 0", "RUN A=1 []", "COPIED", "OK"]
    );
    // Typed in direct mode, it is running too.
    enter(&mut m, "?envstat()");
    assert_eq!(tail(&m, 2), ["3", "OK"]);
    // Only subprograms copy; here it does nothing.
    run(&mut m, &["ENVLOAD 0,1,1"]);
    assert_eq!(tail(&m, 2), ["RUN", "OK"]);
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

    // A timeout in frames; a long caption is cut.
    let d = until_dialog(
        &mut m,
        &format!("DIALOG \"X\",0,\"{}\",-3:?RESULT()", "C".repeat(50)),
    );
    assert_eq!(d.caption, format!("{}……", "C".repeat(45)));
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
    assert!(tail(&m, 2)[0].starts_with("Illegal file name"));
    run(&mut m, &["?DIALOG(\"A\",0)"]);
    assert!(tail(&m, 2)[0].starts_with("Out of range"));
}

#[test]
fn other_slots_data_and_variables() {
    // FINITI reads models from the DATA of the slot that calls it, and
    // its demos write other slots' variables with VAR("n:NAME").
    let mut m = machine();
    m.set_slot(
        1,
        "VAR D0=5\nCOMMON DEF R()\n RESTORE \"0:@M\"\n READ A$\n RETURN A$\nEND\n\
         COMMON DEF L(N)\n RETURN CHKLABEL(STR$(N)+\":@OWN\")\nEND\n@OWN:DATA 1"
            .to_string(),
    );
    run(
        &mut m,
        &[
            "EXEC 1",
            "?R()",
            "VAR(\"1:D0\")=VAR(\"1:D0\")+2:?VAR(\"1:D0\");CHKVAR(\"2:D0\")",
            // A slot's top level, also from a DEF in it.
            "?L(0);L(1)",
            "@M:DATA \"MODEL\"",
        ],
    );
    assert_eq!(tail(&m, 4), ["MODEL", "70", "01", "OK"]);
}

#[test]
fn exec_runs_other_slots() {
    let mut m = machine();
    m.set_slot(
        1,
        [
            "X=5:?\"S1\";X",
            "END",
            "COMMON DEF ADD(A,B)",
            " RETURN A+B+X",
            "END",
            "DEF HIDDEN",
            " ?\"HID\"",
            "END",
            "@L",
            "?\"L1\":RETURN",
        ]
        .join("\n"),
    );
    run(
        &mut m,
        &[
            "?\"A\":EXEC 1:?\"B\";X",
            "?ADD(2,3)",
            "GOSUB \"1:@L\"",
            "CALL \"1:HIDDEN\"",
            "?\"END\"",
        ],
    );
    let shown = ["A", "S15", "B0", "10", "L1", "HID", "END", "OK"];
    assert_eq!(tail(&m, 8), shown, "slot 1 has its own X");

    // Only COMMON DEFs are found by name, and only once their slot ran.
    // A name found nowhere is a Syntax error when reached (checked on
    // hardware).
    run(&mut m, &["?1:HIDDEN"]);
    assert_eq!(tail(&m, 3)[..2], ["1", "Syntax error in 0:1 (HIDDEN)"]);
    run(&mut m, &["?ADD(1,2)"]);
    assert!(tail(&m, 2)[0].starts_with("Syntax error in 0:1"));
    // The running slot starts again; one a DEF returns to cannot be
    // EXEC'd, nor can EXEC be used in direct mode (checked on hardware).
    run(&mut m, &["?\"R\";:LOCATE OUT X,Y:IF X<3 THEN EXEC 0"]);
    assert_eq!(tail(&m, 2), ["RRR", "OK"]);
    m.set_slot(3, "COMMON DEF BACK\nEXEC 0\nEND".to_string());
    run(&mut m, &["EXEC 3:BACK"]);
    assert!(tail(&m, 2)[0].starts_with("Can't use the specified slot in 3:2"));
    enter(&mut m, "exec 1");
    assert_eq!(tail(&m, 2), ["Can't use from direct mode", "OK"]);

    // An error in slot 1 names it, and F4 goes there.
    m.set_slot(2, "?\"S2\"\nZ=0:?1/Z".to_string());
    run(&mut m, &["EXEC 2"]);
    assert_eq!(tail(&m, 3)[..2], ["S2", "Divide by zero in 2:2"]);
    m.toggle_editor();
    assert_eq!(m.edit().slot(), 2);
}

#[test]
fn exec_loads_files() {
    use sb_runtime::confirm::FileConfirm;
    use sb_runtime::storage::{FileKind, Storage};
    let dir = std::env::temp_dir().join(format!("sb-exec-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let mut m = machine();
    m.rt.storage = Storage::new(dir.clone());
    m.rt.settings.file_confirm = FileConfirm::Silent;
    let s = &m.rt.storage;
    s.write(FileKind::Txt, "SUB", b"?\"SUB\"", None).unwrap();
    s.write(FileKind::Txt, "NEXT", b"?\"NEXT\";X", None)
        .unwrap();
    run(
        &mut m,
        &[
            "X=1:EXEC \"SUB\",3:?\"BACK\";RESULT()",
            "EXEC \"NEXT\"",
            "?\"NOT HERE\"",
        ],
    );
    assert_eq!(tail(&m, 4), ["SUB", "BACK1", "NEXT0", "OK"]);
    assert_eq!(m.slot(0), "?\"NEXT\";X");
    // A file that cannot be loaded is an error (checked on hardware).
    run(&mut m, &["EXEC \"NONE\",3:?\"NOT HERE\""]);
    assert_eq!(tail(&m, 2), ["Load failed in 0:1", "OK"]);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn slot_constant_follows_the_slot() {
    // FINITI, run with EXEC into slot 5, tells its caller's slot so.
    let mut m = machine();
    m.set_slot(3, "?\"S\";#_SLOT;#_FILENAME==PRGNAME$()".to_string());
    m.rt.slot_names[3] = "LIB".to_string();
    run(&mut m, &["EXEC 3", "?#_SLOT;#_FILENAME"]);
    assert_eq!(tail(&m, 3), ["S31", "0", "OK"]);
}

#[test]
fn commands_read_data_in_slots_exec_ran() {
    let mut m = machine();
    m.set_slot(1, "END\n@M\nDATA \"T120CDE\",0".to_string());
    run(&mut m, &["EXEC 1:BGMSETD 128,\"1:@M\":?\"OK\""]);
    assert_eq!(tail(&m, 2), ["OK", "OK"]);
    run(&mut m, &["BGMSETD 128,\"2:@M\""]);
    assert!(tail(&m, 2)[0].starts_with("Undefined label"));
}

#[test]
fn callbacks_are_found_in_the_slot_that_set_them() {
    let mut m = machine();
    m.set_slot(
        1,
        "END\nCOMMON DEF INIT\n SPSET 0,0:SPFUNC 0,\"CB\"\nEND\nDEF CB\n ?\"CB\";CALLIDX()\nEND"
            .to_string(),
    );
    run(&mut m, &["EXEC 1:INIT:CALL SPRITE"]);
    assert_eq!(tail(&m, 2), ["CB0", "OK"]);
}
