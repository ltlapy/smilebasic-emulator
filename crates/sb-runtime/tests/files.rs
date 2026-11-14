//! File commands in direct mode and in programs, on a temporary folder.

use std::path::PathBuf;

use sb_runtime::Runtime;
use sb_runtime::confirm::FileConfirm;
use sb_runtime::font::Font;
use sb_runtime::machine::Machine;
use sb_runtime::runtime::keys;
use sb_runtime::storage::Storage;

struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> TempDir {
        let dir = std::env::temp_dir().join(format!("sb-files-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        TempDir(dir)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn machine(dir: &TempDir) -> Machine {
    let mut rt = Runtime::for_tests(Font::blank());
    rt.storage = Storage::new(dir.0.clone());
    // Tests of file questions turn them back on.
    rt.settings.file_confirm = FileConfirm::Silent;
    let mut m = Machine::new(rt);
    m.boot();
    m
}

/// Types a line in lower case (it arrives in upper case) and runs it.
fn enter(m: &mut Machine, line: &str) {
    m.type_text(line);
    m.rt.key_press(keys::ENTER);
    // Keys go on 32 a frame.
    for _ in 0..=line.len() / 32 {
        m.frame();
    }
    for _ in 0..100 {
        m.frame();
        if !m.is_running() {
            return;
        }
    }
    panic!("{line:?} did not finish");
}

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

fn tail(m: &Machine, n: usize) -> Vec<String> {
    let r = rows(m);
    r[r.len() - n..].to_vec()
}

#[test]
fn the_project_manager_loads_and_runs() {
    use sb_runtime::storage::FileKind;
    let dir = TempDir::new("manager");
    let mut m = machine(&dir);
    let storage = &m.rt.storage;
    storage
        .write(FileKind::Txt, "MAIN.PRG", b"?42", None)
        .unwrap();
    storage
        .write(FileKind::Txt, "@BACKUP.PRG", b"?7", None)
        .unwrap();
    m.load_slot(1, "@BACKUP.PRG").unwrap();
    assert_eq!(m.slot(1), "?7");
    assert!(m.load_slot(1, "NONE").is_err());
    m.run_file("MAIN.PRG").unwrap();
    for _ in 0..10 {
        m.frame();
    }
    assert_eq!(m.slot(0), "?42");
    assert_eq!(tail(&m, 2), ["42", "OK"]);
}

#[test]
fn listing_pages_follow_the_console_size() {
    let dir = TempDir::new("small-pages");
    let mut m = machine(&dir);
    for i in 0..40 {
        m.rt.storage
            .write(
                sb_runtime::storage::FileKind::Dat,
                &format!("D{i:02}"),
                b"",
                None,
            )
            .unwrap();
    }
    // A 160-pixel-high screen holds 20 rows of 8-pixel characters.
    m.type_text("xscreen 400,160:files");
    m.rt.key_press(keys::ENTER);
    for _ in 0..10 {
        m.frame();
    }
    assert!(m.is_running(), "waits after a screen: {:?}", rows(&m));
    let shown = rows(&m);
    assert_eq!(m.rt.text[4].size().1, 20);
    assert_eq!(shown.len(), 20);
    assert_eq!(shown[19], "=== Press ENTER ===");
}

#[test]
fn long_listings_wait_for_a_key() {
    let dir = TempDir::new("pages");
    let mut m = machine(&dir);
    for i in 0..40 {
        m.rt.storage
            .write(
                sb_runtime::storage::FileKind::Dat,
                &format!("D{i:02}"),
                b"",
                None,
            )
            .unwrap();
    }
    m.type_text("cls:files");
    m.rt.key_press(keys::ENTER);
    for _ in 0..10 {
        m.frame();
    }
    assert!(m.is_running(), "waits after a screen");
    let shown = rows(&m);
    assert_eq!(shown.len(), 30);
    assert_eq!(shown[0], "-- 40 files --");
    assert_eq!(shown[28], "DAT:D27");
    assert_eq!(shown[29], "=== Press ENTER ===");
    m.rt.key_press(u16::from(b' '));
    for _ in 0..10 {
        m.frame();
    }
    assert!(!m.is_running());
    assert_eq!(tail(&m, 2), ["DAT:D39", "OK"]);
}

#[test]
fn save_load_and_list() {
    let dir = TempDir::new("save");
    let mut m = machine(&dir);
    m.set_slot(1, "?\"ＨＩ\"".to_string());
    enter(&mut m, "save \"game.prg\",1");
    enter(&mut m, "save \"game.prg\",1:?result()");
    assert_eq!(tail(&m, 2), ["1", "OK"]);
    // Full-width letters were saved as ASCII, and the old file was kept.
    let saved = dir.0.join("@WORKSPACE").join("TGAME.PRG");
    assert_eq!(std::fs::read_to_string(saved).unwrap(), "?\"HI\"");
    assert!(dir.0.join("@WORKSPACE").join("T@BACKUP.PRG").is_file());

    enter(&mut m, "load \"game.prg\",2");
    enter(&mut m, "run 2");
    assert_eq!(tail(&m, 2), ["HI", "OK"]);
    // The editor's status line names the file each slot came from.
    assert_eq!(m.rt.slot_names[1..3], ["GAME.PRG", "GAME.PRG"]);
    enter(&mut m, "new 2");
    assert_eq!(m.rt.slot_names[2], "");
    enter(&mut m, "files");
    assert_eq!(
        tail(&m, 4),
        ["-- 2 files --", "TXT:@BACKUP.PRG", "TXT:GAME.PRG", "OK"]
    );
    enter(&mut m, "?chkfile(\"game.prg\");chkfile(\"grp:game.prg\")");
    assert_eq!(tail(&m, 2), ["10", "OK"]);
}

#[test]
fn breakpoints_go_and_come_back_with_their_file() {
    let dir = TempDir::new("breakpoints");
    let mut m = machine(&dir);
    m.set_slot(1, "A=1\nB=2".to_string());
    enter(&mut m, "save \"b\",1");
    m.set_slot(3, "C=1".to_string());
    enter(&mut m, "save \"c\",3");
    m.toggle_breakpoint(1, 2);
    m.toggle_breakpoint(2, 1);
    enter(&mut m, "load \"b\",1");
    assert_eq!(m.all_breakpoints(), [(1, 2, None), (2, 1, None)]);
    enter(&mut m, "load \"c\",1");
    assert_eq!(m.all_breakpoints(), [(2, 1, None)]);
    enter(&mut m, "load \"b\",1");
    assert_eq!(m.all_breakpoints(), [(1, 2, None), (2, 1, None)]);
    // Lines past the end of the file as it is now go.
    m.toggle_breakpoint(1, 1);
    enter(&mut m, "load \"c\",1");
    m.set_slot(3, "A=1".to_string());
    enter(&mut m, "save \"b\",3");
    enter(&mut m, "load \"b\",1");
    assert_eq!(m.all_breakpoints(), [(1, 1, None), (2, 1, None)]);
}

#[test]
fn projects_rename_delete() {
    let dir = TempDir::new("project");
    let mut m = machine(&dir);
    // PROJECT only switches; projects are made in the project manager.
    enter(&mut m, "project \"mygame\"");
    // The message names the command and goes on to a second row.
    assert!(tail(&m, 3).concat().contains("Undefined project name"));
    enter(&mut m, "project \"/\"");
    assert!(tail(&m, 2)[0].starts_with("Illegal project name"));
    m.rt.storage.create_project("MYGAME").unwrap();
    enter(&mut m, "project \"mygame\":?project()");
    assert_eq!(tail(&m, 2), ["MYGAME", "OK"]);
    enter(&mut m, "save \"a\":rename \"a\",\"b\":files \"txt:\"");
    assert_eq!(tail(&m, 2), ["TXT:B", "OK"]);
    // Another type for the second name is File type mismatch, before the
    // file is looked for; no type takes the first's.
    m.set_slot(0, "A=1\nRENAME \"TXT:B\",\"DAT:C\"".to_string());
    enter(&mut m, "run");
    let (e, _) = m.take_program_error().expect("an error");
    let message = "File type mismatch in 0:2(RENAME) (TXT file to DAT)";
    assert_eq!(
        (e.to_string().as_str(), e.kind.code(), e.arg),
        (message, 77, None)
    );
    enter(&mut m, "rename \"txt:none\",\"dat:c\"");
    assert_eq!(
        tail(&m, 2)[0],
        "File type mismatch (RENAME) (TXT file to DAT)"
    );
    enter(&mut m, "rename \"txt:b\",\"c\":files \"txt:\"");
    assert_eq!(tail(&m, 2), ["TXT:C", "OK"]);
    enter(&mut m, "delete \"c\":f$=files():?len(f$)");
    assert_eq!(tail(&m, 2), ["0", "OK"]);
    // Projects end in `/`, after the workspace's own `/`.
    enter(&mut m, "files \"//\"");
    assert_eq!(tail(&m, 3), ["/", "MYGAME/", "OK"]);
    enter(&mut m, "dim n$[0]:files \"//\",n$:?n$[1]");
    assert_eq!(tail(&m, 2), ["MYGAME/", "OK"]);
    enter(&mut m, "?len(files(\"txt:mygame/\"))");
    assert_eq!(tail(&m, 2), ["0", "OK"]);
}

#[test]
fn failures_set_result_and_explain() {
    let dir = TempDir::new("fail");
    let mut m = machine(&dir);
    enter(&mut m, "load \"nope\"");
    assert_eq!(tail(&m, 2), ["LOAD failed: TXT:NOPE not found", "OK"]);
    enter(&mut m, "?result()");
    assert_eq!(tail(&m, 2), ["0", "OK"]);
    // In a program, failures are silent and PROJECT is direct-mode only.
    m.set_slot(0, "LOAD \"NOPE\",1:?RESULT()\nPROJECT \"X\"".to_string());
    enter(&mut m, "run");
    let r = tail(&m, 3);
    assert_eq!(r[0], "0");
    assert!(r[1].starts_with("Can't use in program in 0:2"), "{r:?}");
    enter(&mut m, "new");
    assert_eq!(m.slot(0), "");
    // Unless file access is silent, a program's failures show as notices.
    m.rt.take_notices();
    m.rt.settings.file_confirm = FileConfirm::Notify;
    m.set_slot(0, "LOAD \"NOPE\",1".to_string());
    enter(&mut m, "run");
    assert_eq!(m.rt.take_notices(), ["LOAD failed: TXT:NOPE not found"]);
    // A program can LOAD over itself, and goes on (checked on hardware).
    (m.rt.storage)
        .write(sb_runtime::storage::FileKind::Txt, "X", b"?2", None)
        .unwrap();
    m.set_slot(0, "LOAD \"X\":?1".to_string());
    enter(&mut m, "run");
    assert_eq!(
        (tail(&m, 2), m.slot(0)),
        (vec!["1".to_string(), "OK".to_string()], "?2")
    );
}

#[test]
fn edits_are_unsaved_until_save() {
    use sb_runtime::runtime::keys;
    let dir = TempDir::new("unsaved");
    let mut m = machine(&dir);
    m.set_slot(1, "?1".to_string());
    enter(&mut m, "list 1:1");
    m.toggle_editor();
    assert_eq!(m.unsaved_slots(), [] as [usize; 0], "nothing was changed");
    enter(&mut m, "list 1:1");
    m.type_text("?2");
    m.rt.key_press(keys::ENTER);
    m.frame();
    assert_eq!(m.unsaved_slots(), [1], "counted while still editing");
    m.toggle_editor();
    assert_eq!(m.unsaved_slots(), [1]);
    enter(&mut m, "save \"a\",1");
    assert_eq!(m.unsaved_slots(), [] as [usize; 0]);
    enter(&mut m, "list 2:1");
    m.type_text("X");
    m.frame();
    m.toggle_editor();
    ask(&mut m, "new 2");
    m.rt.answer(true);
    settle(&mut m);
    assert_eq!(m.unsaved_slots(), [] as [usize; 0]);
}

/// Edits a slot so that it has unsaved changes.
fn touch(m: &mut Machine, slot: usize) {
    enter(m, &format!("list {slot}:1"));
    m.type_text("'");
    m.frame();
    m.toggle_editor();
}

/// Types a line and runs frames until it asks a question.
fn ask(m: &mut Machine, line: &str) -> sb_runtime::confirm::Confirm {
    m.type_text(line);
    m.rt.key_press(sb_runtime::runtime::keys::ENTER);
    for _ in 0..10 {
        m.frame();
        if let Some(c) = m.rt.confirmation() {
            return c.clone();
        }
    }
    panic!("{line:?} did not ask");
}

/// Runs frames until direct mode waits for a line again.
fn settle(m: &mut Machine) {
    for _ in 0..10 {
        m.frame();
    }
    assert!(!m.is_running());
}

#[test]
fn load_and_new_ask_before_losing_edits() {
    let dir = TempDir::new("ask");
    let mut m = machine(&dir);
    m.set_slot(0, "?1".to_string());
    enter(&mut m, "save \"a\"");
    touch(&mut m, 0);
    // A missing file fails without asking.
    enter(&mut m, "load \"nope\":?result()");
    assert_eq!(tail(&m, 2), ["0", "OK"]);

    let c = ask(&mut m, "load \"a\":?result()");
    assert_eq!(c.title, "Slot 0: Load file");
    assert_eq!(
        c.lines[5..],
        ["Slot 0 has unsaved changes,", "which will be lost."]
    );
    m.rt.answer(false);
    settle(&mut m);
    assert_eq!(tail(&m, 2), ["-1", "OK"]);
    assert_eq!(m.slot(0), "'?1");

    ask(&mut m, "load \"a\":?result()");
    m.rt.answer(true);
    settle(&mut m);
    assert_eq!(tail(&m, 2), ["1", "OK"]);
    assert_eq!(m.slot(0), "?1");

    touch(&mut m, 3);
    enter(&mut m, "new 2");
    let c = ask(&mut m, "new");
    assert_eq!(c.lines[0], "Erase the programs in all slots.");
    assert_eq!(c.lines[2], "Slot 3 has unsaved changes,");
    m.rt.answer(false);
    settle(&mut m);
    assert_eq!(m.slot(3), "'");
    // Stopping drops the question.
    ask(&mut m, "new 3");
    m.stop();
    assert!(m.rt.confirmation().is_none());
    ask(&mut m, "new 3");
    m.rt.answer(true);
    settle(&mut m);
    assert_eq!((m.slot(3), m.unsaved_slots()), ("", vec![]));
}

#[test]
fn direct_file_commands_ask_notify_or_stay_silent() {
    let dir = TempDir::new("confirm");
    let mut m = machine(&dir);
    m.rt.settings.file_confirm = FileConfirm::Ask;
    m.set_slot(0, "?1".to_string());
    let c = ask(&mut m, "save \"a\"");
    assert_eq!(c.title, "Slot 0: Save file");
    assert_eq!(
        c.lines,
        [
            "Save slot 0 as this file.",
            "",
            "  Project:  [WORKSPACE]",
            "  File:     TXT:A",
            "",
            "To stop these questions, go to",
            "File > Settings > Files.",
        ]
    );
    m.rt.answer(true);
    settle(&mut m);
    assert!(dir.0.join("@WORKSPACE").join("TA").is_file());

    // The hint is given once; LOAD asks even with nothing unsaved.
    let c = ask(&mut m, "load \"a\",1");
    assert_eq!(c.lines.len(), 4);
    assert_eq!(c.accept, "Load");
    m.rt.answer(true);
    settle(&mut m);
    assert_eq!(m.slot(1), "?1");

    m.rt.settings.file_confirm = FileConfirm::Notify;
    enter(&mut m, "rename \"a\",\"b\"");
    assert_eq!(m.rt.take_notices(), ["Renaming TXT:A to B"]);
    assert!(dir.0.join("@WORKSPACE").join("TB").is_file());
}

#[test]
fn programs_ask_except_for_a_safe_load() {
    let dir = TempDir::new("program");
    let mut m = machine(&dir);
    m.rt.settings.file_confirm = FileConfirm::Ask;
    std::fs::create_dir_all(dir.0.join("@WORKSPACE")).unwrap();
    std::fs::write(dir.0.join("@WORKSPACE").join("TA"), "?2").unwrap();
    m.set_slot(0, "LOAD \"A\",1:?RESULT()".to_string());
    enter(&mut m, "run");
    assert_eq!(
        (tail(&m, 2), m.slot(1)),
        (vec!["1".into(), "OK".into()], "?2")
    );

    // SAVE in a program asks, as typed ones do.
    m.set_slot(0, "SAVE \"B\",1:?RESULT()".to_string());
    let c = ask(&mut m, "run");
    assert_eq!(c.title, "Slot 1: Save file");
    m.rt.answer(true);
    settle(&mut m);
    assert_eq!(tail(&m, 2), ["1", "OK"]);
    m.rt.settings.file_confirm = FileConfirm::Notify;
    enter(&mut m, "run");
    assert_eq!(
        m.rt.take_notices(),
        ["Saving slot 1 as TXT:B"; 1],
        "and follows the setting"
    );

    m.rt.settings.file_confirm = FileConfirm::Silent;
    m.set_slot(0, "LOAD \"A\",1:?RESULT()".to_string());
    touch(&mut m, 1);
    let c = ask(&mut m, "run");
    assert_eq!(c.lines[0], "Load this file into slot 1.");
    m.rt.answer(false);
    settle(&mut m);
    assert_eq!(tail(&m, 2), ["-1", "OK"]);
}

#[test]
fn switching_projects_warns_of_unsaved_edits() {
    let dir = TempDir::new("switch");
    let mut m = machine(&dir);
    m.rt.storage.create_project("X").unwrap();
    enter(&mut m, "project \"x\"");
    touch(&mut m, 2);
    let c = ask(&mut m, "project \"\":?result()");
    assert_eq!(
        c.lines,
        [
            "Switch to project [WORKSPACE].",
            "",
            "Unsaved changes in slot 2",
            "are not saved in project X.",
        ]
    );
    m.rt.answer(false);
    settle(&mut m);
    assert_eq!(tail(&m, 2), ["-1", "OK"]);
    assert_eq!(m.rt.storage.project(), "X");
}

/// Rows with PRINT's tab stops as single spaces.
fn words(rows: &[String]) -> Vec<String> {
    rows.iter()
        .map(|r| r.split_whitespace().collect::<Vec<_>>().join(" "))
        .collect()
}

/// LOADV's function form makes a new value beside what it will replace,
/// while the statement form fills the array where it is (TEST57).
#[test]
fn loadv_as_a_function_needs_room_for_the_new_value() {
    let dir = TempDir::new("loadv_room");
    let mut m = machine(&dir);
    let setup = "DIM A%[2500000]:SAVEV \"DAT:Q\",A%:DIM F[7000000],G[7000000],H[1000000]";
    m.set_slot(0, format!("{setup}\nA%=LOADV(\"DAT:Q\")"));
    enter(&mut m, "run");
    assert!(
        tail(&m, 2)[0].starts_with("Out of memory in 0:2 (LOADV)"),
        "{:?}",
        tail(&m, 2)
    );
    m.set_slot(
        0,
        format!("{setup}\nVAR B=FREEMEM():LOADV \"DAT:Q\",A%:?LEN(A%);\" \";B-FREEMEM()"),
    );
    enter(&mut m, "run");
    assert_eq!(tail(&m, 2)[0], "2500000 0");
    // A string the same.
    let setup = "VAR N=2000000:VAR S$=\"A\"*N:SAVEV \"TXT:Q\",S$:DIM F[16000000]";
    m.set_slot(0, format!("{setup}\nS$=LOADV(\"TXT:Q\")"));
    enter(&mut m, "run");
    assert!(
        tail(&m, 2)[0].starts_with("Out of memory in 0:2 (LOADV)"),
        "{:?}",
        tail(&m, 2)
    );
    // Nor does a dropped value count against it.
    m.set_slot(
        0,
        "VAR N=2000000:VAR S$=\"A\"*N:SAVEV \"TXT:Q\",S$:S$=\"\":DIM F[16000000]\n\
         S$=LOADV(\"TXT:Q\"):?LEN(S$)"
            .to_string(),
    );
    enter(&mut m, "run");
    assert_eq!(tail(&m, 2)[0], "2000000");
    // An array that has to grow needs its new block beside the old.
    let grown = |filler: u32| {
        format!(
            "DEF MK:DIM B%[9000000]:SAVEV \"DAT:R\",B%:END\n\
             MK:DIM A%[8000000],F[{filler}]\n\
             LOADV \"DAT:R\",A%:?LEN(A%)"
        )
    };
    m.set_slot(0, grown(8000000));
    enter(&mut m, "run");
    assert_eq!(tail(&m, 2)[0], "9000000");
    m.set_slot(0, grown(8500000));
    enter(&mut m, "run");
    assert!(
        tail(&m, 2)[0].starts_with("Out of memory in 0:3 (LOADV)"),
        "{:?}",
        tail(&m, 2)
    );
}

#[test]
fn values_save_and_load() {
    let dir = TempDir::new("values");
    let mut m = machine(&dir);
    let program = [
        "SAVEV \"TXT:MEMO\",\"Hi\"+CHR$(10)+\"あ\"",
        "?LOADV(\"TXT:MEMO\")==\"Hi\"+CHR$(10)+\"あ\"",
        "DIM A[2,3]:A[1,2]=1.5:SAVEV \"DAT:A\",A",
        "B=LOADV(\"DAT:A\"):?DIM(B),LEN(B),B[1,2]",
        "DIM C%[2,2]:LOADV \"DAT:A\",C%:?LEN(C%),C%[1,2]",
        "DIM S$[1]:S$[0]=\"X\":SAVEV \"DAT:S\",S$:DIM T$[5]:LOADV \"DAT:S\",T$:?LEN(T$),T$[0]",
        "SAVEV \"DAT:A\",A",
        "?TYPEOF(LOADV(\"DAT:NONE\")),RESULT(),TYPEOF(LOADV(\"TXT:NONE\"))",
    ];
    m.set_slot(0, program.join("\n"));
    enter(&mut m, "run");
    assert_eq!(
        words(&tail(&m, 6)),
        ["1", "2 6 1.5", "6 1", "1 X", "0 0 3", "OK"]
    );
    assert!(dir.0.join("@WORKSPACE").join("B@BACKUP.DAT").is_file());
    // Text is stored as UTF-8, as SAVE does.
    let memo = dir.0.join("@WORKSPACE").join("TMEMO");
    assert_eq!(std::fs::read_to_string(memo).unwrap(), "Hi\nあ");

    m.set_slot(0, "DIM D[2]:LOADV \"DAT:A\",D".to_string());
    enter(&mut m, "run");
    assert!(
        tail(&m, 2)[0].starts_with("Type mismatch"),
        "{:?}",
        tail(&m, 2)
    );
    m.set_slot(0, "SAVEV \"TXT:X\",1".to_string());
    enter(&mut m, "run");
    assert!(tail(&m, 2)[0].starts_with("Type mismatch"));
    m.set_slot(0, "DIM Z$[0]:LOADV \"TXT:MEMO\",Z$".to_string());
    enter(&mut m, "run");
    // The message wraps onto a second row.
    assert!(tail(&m, 3)[0].starts_with("Wrong number of arguments in 0:1"));
    m.set_slot(0, "LOADG \"DAT:S\",0".to_string());
    enter(&mut m, "run");
    assert!(
        tail(&m, 3)[0].starts_with("Illegal file format"),
        "{:?}",
        tail(&m, 3)
    );
}

/// What TEST67 saw of LOADV, CHKFILE, LOAD and SAVE in a program.
#[test]
fn loadv_chkfile_and_program_types() {
    let dir = TempDir::new("test67");
    let mut m = machine(&dir);
    let err = |m: &mut Machine| {
        let (e, _) = m.take_program_error().expect("an error");
        (e.to_string(), e.kind.code(), e.arg)
    };
    // A real file into an int array: NaN as 0, the rest truncated.
    m.set_slot(
        0,
        "A=1E308:B=10:F=A*B:N=F-F:DIM R#[3]:R#[0]=N:R#[1]=1.9:R#[2]=-1.9\n\
         SAVEV \"DAT:NAN\",R#:DIM M%[1]:LOADV \"DAT:NAN\",M%:?LEN(M%);M%[0];M%[1];M%[2]"
            .to_string(),
    );
    enter(&mut m, "run");
    assert_eq!(tail(&m, 2), ["301-1", "OK"]);
    // The command form takes no TXT file, into a string either.
    m.set_slot(0, "S$=\"\"\nLOADV \"TXT:NONE\",S$".to_string());
    enter(&mut m, "run");
    assert_eq!(
        err(&mut m),
        (
            "Wrong number of arguments in 0:2(LOADV) (TXT in the command form)".to_string(),
            52,
            None
        )
    );
    // JPG:, PNG: and no type are File type mismatch in both forms, before
    // the file is looked for.
    for (source, detail) in [
        ("S$=LOADV(\"JPG:NONE\")", "JPG file"),
        ("S$=LOADV(\"NONE\")", "no type"),
        ("LOADV \"JPG:NONE\",A%", "JPG file"),
        ("LOADV \"png:NONE\",A%", "PNG file"),
        ("LOADV \"NONE\",A%", "no type"),
    ] {
        m.set_slot(0, format!("DIM A%[1]\n{source}"));
        enter(&mut m, "run");
        let message = format!("File type mismatch in 0:2(LOADV) ({detail})");
        assert_eq!(err(&mut m), (message, 77, None), "{source}");
    }
    // RESULT is 0 after CHKFILE of a missing file in a program that runs
    // after one that left it 1: RUN set it to 0.
    m.set_slot(0, "SAVEV \"TXT:Y\",\"1\":?RESULT()".to_string());
    enter(&mut m, "run");
    assert_eq!(tail(&m, 2), ["1", "OK"]);
    m.set_slot(0, "B=CHKFILE(\"TXT:NONE\"):?B;RESULT()".to_string());
    enter(&mut m, "run");
    assert_eq!(tail(&m, 2), ["00", "OK"]);
    for (source, command) in [("LOAD \"GRP0:X\"", "LOAD"), ("SAVE \"GRP0:X\"", "SAVE")] {
        m.set_slot(0, format!("A=1\n{source}"));
        enter(&mut m, "run");
        let message = format!("Illegal format in 0:2({command}:1) (\"GRP0:X\")");
        assert_eq!(err(&mut m), (message, 71, Some(0)), "{source}");
    }
}

/// A file name may start with `#`; it is the project's only when a `/`
/// follows.
#[test]
fn file_names_starting_with_a_hash() {
    use sb_runtime::storage::FileKind;
    let dir = TempDir::new("hash-names");
    let mut m = machine(&dir);
    let program = [
        "SAVEV \"TXT:#NEWFILE\",\"HI\":?RESULT();CHKFILE(\"TXT:#NEWFILE\")",
        "?LOADV(\"TXT:#NEWFILE\")",
        "DELETE \"TXT:#NEWFILE\":?RESULT();CHKFILE(\"TXT:#NEWFILE\")",
    ];
    m.set_slot(0, program.join("\n"));
    enter(&mut m, "run");
    assert_eq!(tail(&m, 4), ["11", "HI", "10", "OK"]);
    m.rt.storage
        .write(FileKind::Txt, "#UPDATE-META", b"?\"META\"", None)
        .unwrap();
    assert!(dir.0.join("@WORKSPACE").join("T#UPDATE-META").is_file());
    enter(&mut m, "load \"#update-meta\"");
    assert_eq!(m.slot(0), "?\"META\"");
}

/// Names as SmileBASIC 4's parser takes them: `@` anywhere, `.` after a
/// first `@`, and a type only before a `:` at index 3. A name it refuses
/// is Illegal format, and a part of over 32 characters File name is too
/// long, both numbering the argument.
#[test]
fn file_names_follow_sb4s_parser() {
    let dir = TempDir::new("name-parser");
    let mut m = machine(&dir);
    let program = [
        "SAVEV \"TXT:A@B\",\"AT\":?RESULT();LOADV(\"TXT:A@B\");CHKFILE(\"txt:a@b\")",
        "SAVEV \"TXT:@.X\",\"DOT\":?RESULT();LOADV(\"TXT:@.X\")",
        "SAVEV \"TXT:@\",\"LONE\":?RESULT();LOADV(\"TXT:@\")",
    ];
    m.set_slot(0, program.join("\n"));
    enter(&mut m, "run");
    assert_eq!(tail(&m, 4), ["1AT1", "1DOT", "1LONE", "OK"]);
    assert!(dir.0.join("@WORKSPACE").join("TA@B").is_file());

    let (max, long) = ("A".repeat(32), "A".repeat(33));
    m.set_slot(0, format!("SAVEV \"TXT:{max}\",\"X\":?RESULT()"));
    enter(&mut m, "run");
    assert_eq!(tail(&m, 2), ["1", "OK"]);
    for (source, message, code) in [
        ("?CHKFILE(\"TXT:A:B\")", "Illegal format", 71),
        ("?CHKFILE(\"AB:C\")", "Illegal format", 71),
        ("?CHKFILE(\"TXT:\")", "Illegal format", 71),
        ("SAVEV \"TXT:A/B/C\",\"X\"", "Illegal format", 71),
        ("LOADV \"DAT:A*\",A%", "Illegal format", 71),
        ("DELETE \"TXT:A:B\"", "Illegal format", 71),
        (
            &format!("SAVEV \"TXT:{long}\",\"X\""),
            "File name is too long",
            73,
        ),
        (
            &format!("?CHKFILE(\"{long}/A\")"),
            "File name is too long",
            73,
        ),
    ] {
        m.set_slot(0, format!("DIM A%[1]\n{source}"));
        enter(&mut m, "run");
        let (said, number, arg) = program_error(&mut m);
        let command = source.trim_start_matches('?');
        let command = &command[..command.find([' ', '(']).unwrap()];
        let start = format!("{message} in 0:2({command}:1) (");
        assert!(said.starts_with(&start), "{source}: {said}");
        assert_eq!((number, arg), (code, Some(0)), "{source}");
    }
    assert!(!dir.0.join("@WORKSPACE").join(format!("T{long}")).exists());

    for (line, message) in [
        ("?chkfile(\"\")", "Illegal format (CHKFILE:1) (\"\")"),
        (
            "load \"grp:a:b\",1",
            "Illegal format (LOAD:1) (\"GRP:A:B\")",
        ),
        (
            "rename \"a@b\",\"b:c\"",
            "Illegal format (RENAME:2) (\"B:C\")",
        ),
        (
            "files \"txt:a/b\"",
            "Illegal format (FILES:1) (\"TXT:A/B\")",
        ),
        ("files \"\"", "Illegal format (FILES:1) (\"\")"),
        ("load \"png:x\"", "File type mismatch (LOAD) (PNG file)"),
    ] {
        assert_eq!(direct_error(&mut m, line), message, "{line}");
    }
    let line = format!("loadg \"{}/a\",0", "b".repeat(33));
    let message = format!("File name is too long (LOADG:1) (\"{}/A\")", "B".repeat(33));
    assert_eq!(direct_error(&mut m, &line), message);
    // EXEC numbers no argument.
    m.set_slot(0, "EXEC \"TXT:A:B\",1".to_string());
    enter(&mut m, "run");
    let (said, number, arg) = program_error(&mut m);
    assert_eq!(
        (said.as_str(), number, arg),
        ("Illegal format in 0:1 (\"TXT:A:B\")", 71, None)
    );
    // The name is fine and the file there: FILES lists it, RENAME and
    // DELETE take it.
    enter(&mut m, "cls:dim l$[0]:files \"txt:\",l$:?l$[0]");
    assert_eq!(tail(&m, 2), ["TXT:@", "OK"]);
    enter(
        &mut m,
        "rename \"txt:a@b\",\"@a.b\":?result();chkfile(\"txt:@a.b\")",
    );
    assert_eq!(tail(&m, 2), ["11", "OK"]);
    enter(
        &mut m,
        "delete \"txt:@a.b\":?result();chkfile(\"txt:@a.b\")",
    );
    assert_eq!(tail(&m, 2), ["10", "OK"]);
}

#[test]
fn graphic_pages_save_and_load() {
    let dir = TempDir::new("images");
    let mut m = machine(&dir);
    let program = [
        "GPSET 1,2,&HFF102030:GPSET 0,0,&H80FFFFFF",
        "SAVEG \"PIC\",0,0,0,4,3:?RESULT()",
        "GCLS:LOADG \"GRP:PIC\",1,10,20 OUT W,H:?W,H",
        "GTARGET 1:?HEX$(GPGET(11,22)),HEX$(GPGET(10,20))",
        "IMG=LOADV(\"GRP:PIC\"):?DIM(IMG),LEN(IMG),HEX$(IMG[2,1])",
        "SAVEV \"GRP:IMG\",IMG:LOADG \"IMG\",2:GTARGET 2:?HEX$(GPGET(1,2))",
        "SAVEG \"JPG:PHOTO\",1,8,16,8,8,90:LOADG \"JPG:PHOTO\",3 OUT W,H:?W,H,RESULT()",
        "LOADG \"NOPE\",0:?RESULT()",
    ];
    m.set_slot(0, program.join("\n"));
    enter(&mut m, "run");
    assert_eq!(
        words(&tail(&m, 8)),
        [
            "1",
            "4 3",
            "FF102030 80FFFFFF",
            "2 12 FF102030",
            "FF102030",
            "8 8 1",
            "0",
            "OK"
        ]
    );
    // GRP files are PNG images.
    let png = std::fs::read(dir.0.join("@WORKSPACE").join("GPIC")).unwrap();
    assert!(png.starts_with(b"\x89PNG"));
    m.set_slot(0, "SAVEG \"PIC\",0,2000,0,100,1".to_string());
    enter(&mut m, "run");
    assert!(tail(&m, 2)[0].starts_with("Out of range"));
}

/// A bare `PCBN` integer array of these sizes, with `count` values of 0.
fn array_file(dims: &[u32], count: usize) -> Vec<u8> {
    let mut body = b"PCBN0004".to_vec();
    body.extend_from_slice(&4u16.to_le_bytes());
    body.extend_from_slice(&(dims.len() as u16).to_le_bytes());
    for i in 0..4 {
        body.extend_from_slice(&dims.get(i).copied().unwrap_or(0).to_le_bytes());
    }
    body.resize(body.len() + 4 * count, 0);
    body
}

#[test]
fn crafted_arrays_fail_as_a_bad_file_does() {
    let dir = TempDir::new("crafted");
    let mut m = machine(&dir);
    let workspace = dir.0.join("@WORKSPACE");
    std::fs::create_dir_all(&workspace).unwrap();
    // Their products are 0, but RINGCOPY would go through four thousand
    // million rows, and an image of no pixels ended the process when it was
    // encoded.
    std::fs::write(workspace.join("BHUGE"), array_file(&[u32::MAX, 0], 0)).unwrap();
    std::fs::write(workspace.join("GFLAT"), array_file(&[0, 16], 0)).unwrap();
    let program = [
        "DIM A[1,1]",
        "LOADV \"DAT:HUGE\",A:?RESULT(),DIM(A,0),DIM(A,1)",
        "LOADV \"GRP:FLAT\",A:?RESULT()",
        "LOADG \"FLAT\",0 OUT W,H:?W,H,RESULT()",
        // RESIZE can make what DIM cannot, and that has to come back.
        "DIM B[1,3]:RESIZE B,0,3:SAVEV \"DAT:E\",B:?RESULT()",
        "DIM C[1,1]:LOADV \"DAT:E\",C:?RESULT(),DIM(C,0),DIM(C,1)",
    ];
    m.set_slot(0, program.join("\n"));
    enter(&mut m, "run");
    assert_eq!(
        words(&tail(&m, 6)),
        ["0 1 1", "0", "0 0 0", "1", "1 0 3", "OK"]
    );
    m.set_slot(0, "SAVEG \"PIC\",0,0,0,0,3".to_string());
    enter(&mut m, "run");
    assert!(tail(&m, 2)[0].starts_with("Out of range"));
    m.set_slot(0, "SAVEG \"JPG:PIC\",0,0,0,3,0".to_string());
    enter(&mut m, "run");
    assert!(tail(&m, 2)[0].starts_with("Out of range"));
}

/// A zero in any size loads and keeps the sizes, and whatever is then done
/// with so empty an array ends, with an error or without.
#[test]
fn arrays_with_a_zero_in_any_size_stay_harmless() {
    let statements = [
        "?LEN(B),DIM(B),DIM(B,0),DIM(B,1)",
        "RESIZE B,5,DIM(B,1)",
        "RESIZE B,0,DIM(B,1)",
        "COPY B,B",
        "COPY B,0,B,0,0",
        "SORT B",
        "RSORT B",
        "FILL B,1",
        "?FIND(B,0)",
        "FIND B,0 OUT X,Y",
        "ARYOP 0,B,1,2",
        "ARYOP 0,B,B,B",
        "RINGCOPY B,0,B",
        "RINGCOPY B,0,B OUT E",
        "RINGCOPY D,0,B",
        "BIQUAD B,B,D",
        "FFT B,B,B,B",
        "FFTWFN B,0",
        "?POP(B)",
        "PUSH B,1",
        "INSERT B,0,1",
        "REMOVE B,0",
        "INSPECT B",
        "SAVEV \"DAT:OUT\",B",
        "SAVEV \"GRP:OUT\",B",
        "LOADV \"DAT:OUT\",B",
        "GLOAD 0,0,4,4,B,1,0",
        "GSAVE 0,0,0,4,4,B,1",
        "GLOAD B,1",
        "TLOAD 0,0,0,4,4,B",
        "WAVSET 224,0,0,0,0,\"\":WAVSETA 224,0,0,0,0,B",
        "PCMSTREAM B",
        "LFILTER 0,3,B,0",
        "SPPAGE 0:SPDEF B",
        "ANIMDEF 0,B",
        "SPANIM 0,\"XY\",B",
        "?B[0]",
        "B[0,0]=1",
        "?MAX(B)",
        "?MIN(B)",
    ];
    let dir = TempDir::new("zero-sizes");
    let mut m = machine(&dir);
    let workspace = dir.0.join("@WORKSPACE");
    std::fs::create_dir_all(&workspace).unwrap();
    for (name, dims) in [
        ("Z0", [0u32, 3]),
        ("Z1", [3, 0]),
        ("Z2", [1 << 28, 0]),
        ("Z3", [0, 1 << 28]),
        ("Z4", [2, 0]),
    ] {
        std::fs::write(workspace.join(format!("B{name}")), array_file(&dims, 0)).unwrap();
        std::fs::write(workspace.join(format!("G{name}")), array_file(&dims, 0)).unwrap();
        m.set_slot(
            0,
            format!("DIM B[1,1]:LOADV \"DAT:{name}\",B:?RESULT();\" \";DIM(B,0);\" \";DIM(B,1)"),
        );
        enter(&mut m, "run");
        let shown = format!("1 {} {}", dims[0], dims[1]);
        assert_eq!(tail(&m, 2)[0], shown);
        for statement in statements {
            let head = format!("DIM D[2,2],B[1,1]:LOADV \"DAT:{name}\",B\n");
            m.set_slot(0, format!("{head}{statement}\n"));
            enter(&mut m, "run");
            m.set_slot(0, format!("{head}LOADV \"GRP:{name}\",B\n{statement}\n"));
            enter(&mut m, "run");
        }
        m.set_slot(0, format!("LOADG \"{name}\",0 OUT W,H:?W,H,RESULT()"));
        enter(&mut m, "run");
        m.set_slot(0, format!("LOAD \"GRP0:{name}\""));
        enter(&mut m, "run");
    }
}

/// A file's array that fills memory can replace another of its size: 9
/// million reals twice would not fit side by side. IRCAMERA loads its files
/// this way.
#[test]
fn loadv_replaces_an_array_that_fills_memory() {
    let dir = TempDir::new("loadv-big");
    let mut m = machine(&dir);
    let workspace = dir.0.join("@WORKSPACE");
    std::fs::create_dir_all(&workspace).unwrap();
    std::fs::write(
        workspace.join("BBIG"),
        array_file(&[3, 3_000_000], 9_000_000),
    )
    .unwrap();
    let program = [
        "DIM A[3,3000000]",
        "FOR I=1 TO 3:LOADV \"DAT:BIG\",A:?RESULT();\" \";LEN(A):VSYNC:NEXT",
        // Beside another of its size it does not fit, which shows once the
        // VM counts.
        "DIM B[1,1]:LOADV \"DAT:BIG\",B:VSYNC:?\"NOT HERE\"",
    ];
    m.set_slot(0, program.join("\n"));
    enter(&mut m, "run");
    for _ in 0..5 {
        m.frame();
    }
    let console = rows(&m);
    assert_eq!(console.iter().filter(|r| *r == "1 9000000").count(), 3);
    assert!(
        console.iter().any(|r| r.starts_with("Out of memory")),
        "{console:?}"
    );
    assert!(!console.iter().any(|r| r == "NOT HERE"), "{console:?}");

    // A real array of an integer file twice over could never fit.
    std::fs::write(
        workspace.join("BINTS"),
        array_file(&[17_000_000], 17_000_000),
    )
    .unwrap();
    m.set_slot(0, "DIM A[1]:LOADV \"DAT:INTS\",A".to_string());
    enter(&mut m, "run");
    assert!(
        tail(&m, 2)[0].starts_with("Out of memory"),
        "{:?}",
        tail(&m, 2)
    );
    m.set_slot(0, "DIM A%[1]:LOADV \"DAT:INTS\",A%:?LEN(A%)".to_string());
    enter(&mut m, "run");
    assert_eq!(tail(&m, 2)[0], "17000000");
}

#[test]
fn a_file_too_large_to_read_fails_as_a_bad_file_does() {
    let dir = TempDir::new("huge");
    let mut m = machine(&dir);
    let workspace = dir.0.join("@WORKSPACE");
    std::fs::create_dir_all(&workspace).unwrap();
    for name in ["TA", "BA", "GA"] {
        let file = std::fs::File::create(workspace.join(name)).unwrap();
        file.set_len(sb_runtime::storage::MAX_FILE + 1).unwrap();
    }
    let program = [
        "LOAD \"A\",1:?RESULT()",
        "A$=LOADV(\"TXT:A\"):?RESULT(),LEN(A$)",
        "DIM A[1]:LOADV \"DAT:A\",A:?RESULT()",
        "LOADG \"A\",0:?RESULT()",
    ];
    m.set_slot(0, program.join("\n"));
    enter(&mut m, "run");
    assert_eq!(words(&tail(&m, 5)), ["0", "0 0", "0", "0", "OK"]);
}

#[test]
fn metadata_saves_with_the_project() {
    let dir = TempDir::new("meta");
    let mut m = machine(&dir);
    let program = [
        "DIM I[1600]:I[41]=&HFF00FF00",
        "METAEDIT 0,\"My game\":METAEDIT 1,\"Line 1\"+CHR$(10)+\"Line 2\":METAEDIT 2,I",
        "METASAVE:METAEDIT 0,\"\":METALOAD",
        "METAEDIT 0 OUT T$:METAEDIT 1 OUT D$:METAEDIT 2 OUT J",
        "?T$,LEN(D$),DIM(J),HEX$(J[1,1])",
    ];
    m.set_slot(0, program.join("\n"));
    enter(&mut m, "run");
    assert_eq!(words(&tail(&m, 2)), ["My game 13 2 FF00FF00", "OK"]);
    let text = std::fs::read_to_string(dir.0.join("@WORKSPACE").join("@META.TXT")).unwrap();
    assert_eq!(text, "My game\nLine 1\nLine 2");
    // METASAVE leaves RESULT() as it was.
    m.set_slot(0, "LOAD \"NOPE\",1:METASAVE:?RESULT()".to_string());
    enter(&mut m, "run");
    assert_eq!(tail(&m, 2), ["0", "OK"]);
    // A longer title is cut; another project starts with its name.
    m.set_slot(
        0,
        "METAEDIT 0,\"0123456789012345678901234\":METAEDIT 0 OUT T$:?LEN(T$)".to_string(),
    );
    enter(&mut m, "run");
    assert_eq!(tail(&m, 2), ["24", "OK"]);
    m.rt.storage.create_project("GAME").unwrap();
    m.set_slot(0, "METAEDIT 0 OUT T$:?T$".to_string());
    enter(&mut m, "project \"game\"");
    enter(&mut m, "run");
    assert_eq!(tail(&m, 2), ["GAME", "OK"]);
    // METALOAD reads another project's, here the workspace's.
    m.set_slot(0, "METALOAD \"\":METAEDIT 0 OUT T$:?T$".to_string());
    enter(&mut m, "run");
    assert_eq!(tail(&m, 2), ["My game", "OK"]);
    m.set_slot(0, "?METAEDIT(0)".to_string());
    enter(&mut m, "run");
    assert_eq!(tail(&m, 2), ["My game", "OK"]);
    m.set_slot(0, "METALOAD \"NONE\"".to_string());
    enter(&mut m, "run");
    // The message names the command and goes on to a second row.
    assert!(tail(&m, 3).concat().contains("Undefined project name"));
    let (_, code, arg) = program_error(&mut m);
    assert_eq!((code, arg), (76, Some(0)), "METALOAD:1");
}

#[test]
fn metadata_from_smilebasic() {
    use sb_runtime::sbfile::{self, Date, Kind, ProjectMeta};
    let dir = TempDir::new("sb4meta");
    let mut m = machine(&dir);
    let meta = ProjectMeta {
        title: "てすと05".encode_utf16().collect(),
        description: "Hi\n".encode_utf16().collect(),
        icon: vec![0xFFD2_D6CA; 1600],
    };
    let date = Date {
        year: 2026,
        month: 10,
        day: 28,
        hour: 9,
        minute: 2,
        second: 28,
    };
    let bytes = sbfile::container(Kind::Meta, 0, date, &sbfile::meta_body(&meta), false);
    m.rt.storage.write_project_file("META", &bytes).unwrap();
    let program =
        "METALOAD:METAEDIT 0 OUT T$:METAEDIT 1 OUT D$:METAEDIT 2 OUT I:?T$,LEN(D$),HEX$(I[39,39])";
    m.set_slot(0, program.to_string());
    enter(&mut m, "run");
    assert_eq!(words(&tail(&m, 2)), ["てすと05 3 FFD2D6CA", "OK"]);
}

#[test]
fn other_projects_and_backups() {
    use sb_runtime::storage::FileKind;
    let dir = TempDir::new("paths");
    let mut m = machine(&dir);
    let s = &mut m.rt.storage;
    s.create_project("GAME").unwrap();
    s.set_project("GAME").unwrap();
    s.write(FileKind::Txt, "SUB", b"?\"SUB\"", None).unwrap();
    s.write(FileKind::Txt, "NOTE", b"HI", None).unwrap();
    s.set_project("").unwrap();
    s.write(FileKind::Txt, "W", b"?\"W\"", None).unwrap();

    // Direct mode reads any project's files; "/NAME" is the workspace.
    enter(&mut m, "load \"game/sub\",1:?loadv(\"txt:game/note\")");
    assert_eq!(
        (m.slot(1), tail(&m, 2)),
        ("?\"SUB\"", vec!["HI".to_string(), "OK".to_string()])
    );
    enter(&mut m, "project \"game\":load \"/w\",2:?result()");
    assert_eq!(m.slot(2), "?\"W\"");
    enter(&mut m, "project \"\"");

    // Programs read them too with LOAD and LOADV (checked on hardware),
    // though not with EXEC.
    m.set_slot(
        0,
        "LOAD \"GAME/SUB\",1:?RESULT();LOADV(\"TXT:GAME/NOTE\")\nEXEC 1".to_string(),
    );
    enter(&mut m, "run");
    assert_eq!(tail(&m, 3), ["1HI", "SUB", "OK"]);

    // ...or name the current project (checked on hardware).
    enter(&mut m, "project \"game\"");
    m.set_slot(0, "LOAD \"GAME/SUB\",1:?RESULT()".to_string());
    enter(&mut m, "run");
    assert_eq!(tail(&m, 2), ["1", "OK"]);
    enter(&mut m, "project \"\"");

    // Names may start with @, as the backups' do (checked on hardware).
    enter(&mut m, "save \"w\",3:load \"@backup.prg\",4:?result()");
    assert_eq!((m.slot(4), tail(&m, 2)[0].as_str()), ("?\"W\"", "1"));
    enter(&mut m, "save \"@x\",3:?result()");
    assert_eq!(tail(&m, 2)[0], "1");
    enter(
        &mut m,
        "delete \"@backup.prg\":r=result():?r;chkfile(\"@backup.prg\")",
    );
    assert_eq!(tail(&m, 2)[0], "10");
}

/// A program may not EXEC a file named with a project, its own included:
/// EXEC stops with error 74 (TEST68). LOADG takes a name with a project,
/// the program's own or another, with or without `GRP:`, in a program and
/// in direct mode. LOAD into a slot and then EXEC of the slot runs another
/// project's program.
#[test]
fn exec_refuses_project_names_and_loadg_takes_them() {
    use sb_runtime::storage::FileKind;
    let dir = TempDir::new("project-names");
    let mut m = machine(&dir);
    let save = |m: &mut Machine, project: &str, color: &str| {
        m.rt.storage.create_project(project).unwrap();
        m.rt.storage.set_project(project).unwrap();
        let program = format!("GFILL 0,0,3,3,&H{color}:SAVEG \"PIC\",0,0,0,4,4");
        m.set_slot(0, program);
        enter(m, "run");
    };
    save(&mut m, "ART", "FF445566");
    save(&mut m, "GAME", "FF112233");
    (m.rt.storage)
        .write(FileKind::Txt, "SUB", b"?\"SUB\"", None)
        .unwrap();

    m.set_slot(0, "EXEC \"GAME/SUB\",1\n?\"PAST\"".to_string());
    enter(&mut m, "run");
    assert_eq!(
        tail(&m, 2),
        ["Can't access other project in this mode in 0:1", "OK"]
    );

    let names = [
        ("GAME/PIC", "FF112233"),
        ("GRP:GAME/PIC", "FF112233"),
        ("GRP:PIC", "FF112233"),
        ("ART/PIC", "FF445566"),
        ("GRP:ART/PIC", "FF445566"),
    ];
    for (name, color) in names {
        let program =
            format!("GCLS &HFF00FF00:LOADG \"{name}\",0 OUT W,H\n?RESULT();W;H;HEX$(GPGET(0,0))");
        m.set_slot(0, program);
        enter(&mut m, "run");
        let want = [format!("144{color}"), "OK".to_string()];
        assert_eq!(tail(&m, 2), want, "{name}");

        let name = name.to_ascii_lowercase();
        let line = format!("gcls:loadg \"{name}\",0:?result();hex$(gpget(0,0))");
        enter(&mut m, &line);
        let want = [format!("1{color}"), "OK".to_string()];
        assert_eq!(tail(&m, 2), want, "{name}");
    }

    // LOAD names it, then EXEC runs the slot.
    m.set_slot(0, "LOAD \"GAME/SUB\",1:EXEC 1".to_string());
    enter(&mut m, "run");
    assert_eq!(tail(&m, 2), ["SUB", "OK"]);
}

/// What a program's error says, its number and its argument.
fn program_error(m: &mut Machine) -> (String, i32, Option<u8>) {
    let (e, _) = m.take_program_error().expect("an error");
    (e.to_string(), e.kind.code(), e.arg)
}

/// TEST69: LOAD and SAVE of another type of file, and LOADG `PNG:`.
#[test]
fn load_and_save_refuse_other_types_of_file() {
    let dir = TempDir::new("file-types");
    let mut m = machine(&dir);
    enter(&mut m, "dim a%[3]:savev \"dat:d\",a%");
    // Whether or not there is such a file.
    for (source, command, detail) in [
        ("LOAD \"DAT:D\",1", "LOAD", "DAT"),
        ("LOAD \"GRP:NONE\",1", "LOAD", "GRP"),
        ("LOAD \"JPG:NONE\",1", "LOAD", "JPG"),
        ("SAVE \"DAT:X\",1", "SAVE", "DAT"),
    ] {
        m.set_slot(0, format!("A=1\n{source}"));
        enter(&mut m, "run");
        let message = format!("File type mismatch in 0:2({command}) ({detail} file)");
        assert_eq!(program_error(&mut m), (message, 77, None), "{source}");
    }
    assert!(!dir.0.join("@WORKSPACE").join("BX").exists());
    m.set_slot(0, "A=1\nLOAD \"PRG:X\",1".to_string());
    enter(&mut m, "run");
    let message = "Illegal format in 0:2(LOAD:1) (\"PRG:X\")".to_string();
    assert_eq!(program_error(&mut m), (message, 71, Some(0)));

    m.set_slot(0, "A=1\nLOADG \"PNG:X\",0".to_string());
    enter(&mut m, "run");
    let message = "Can't access other project in this mode in 0:2(LOADG)".to_string();
    assert_eq!(program_error(&mut m), (message, 74, None));
    m.set_slot(0, "LOADG \"DAT:NONE\",0:?RESULT()".to_string());
    enter(&mut m, "run");
    assert_eq!(tail(&m, 2), ["0", "OK"]);
}

/// EXEC into a slot with unsaved edits asks first; Back sets RESULT to
/// -1 and stops the program with Load failed.
#[test]
fn exec_over_unsaved_edits_asks_and_back_fails() {
    let dir = TempDir::new("exec-edits");
    let mut m = machine(&dir);
    m.set_slot(1, "?1".to_string());
    enter(&mut m, "save \"a\",1");
    touch(&mut m, 1);
    m.set_slot(0, "EXEC \"A\",1\n?\"PAST\"".to_string());
    ask(&mut m, "run");
    m.rt.answer(false);
    settle(&mut m);
    assert!(
        tail(&m, 2)[0].starts_with("Load failed in 0:1"),
        "{:?}",
        tail(&m, 3)
    );
    enter(&mut m, "?result()");
    assert_eq!(tail(&m, 2), ["-1", "OK"]);
}

/// TEST69: a main program's SAVEV, RENAME and DELETE refuse a project
/// name, their own project's and the workspace's `/NAME` too, while
/// SAVEV at the prompt saves in the current project named so. EXEC
/// refuses `/NAME` as it refuses a project's name.
#[test]
fn programs_write_no_file_named_with_a_project() {
    let dir = TempDir::new("write-projects");
    let mut m = machine(&dir);
    m.rt.storage.create_project("GAME").unwrap();
    enter(&mut m, "savev \"txt:a\",\"1\"");
    let workspace = dir.0.join("@WORKSPACE");
    for (project, own) in [("", "/"), ("GAME", "GAME/")] {
        enter(&mut m, &format!("project \"{project}\""));
        for (source, command) in [
            (format!("SAVEV \"TXT:{own}X\",\"1\""), "SAVEV"),
            ("SAVEV \"TXT:GAME/X\",\"1\"".to_string(), "SAVEV"),
            ("SAVEV \"DAT:/X\",A%".to_string(), "SAVEV"),
            (format!("SAVE \"TXT:{own}X\",1"), "SAVE"),
            ("SAVE \"GAME/X\",1".to_string(), "SAVE"),
            (format!("SAVEG \"{own}X\",0"), "SAVEG"),
            ("SAVEG \"JPG:GAME/X\",0".to_string(), "SAVEG"),
            (format!("RENAME \"TXT:{own}A\",\"B\""), "RENAME:1"),
            (format!("RENAME \"TXT:A\",\"{own}B\""), "RENAME:2"),
            // The first name is checked first.
            (format!("RENAME \"TXT:{own}A\",\"/B\""), "RENAME:1"),
            (format!("DELETE \"TXT:{own}A\""), "DELETE:1"),
        ] {
            m.set_slot(0, format!("DIM A%[1]\n{source}"));
            enter(&mut m, "run");
            let message = format!("Can't access other project in this mode in 0:2({command})");
            let arg = command.ends_with(":1").then_some(0);
            let arg = arg.or(command.ends_with(":2").then_some(1));
            assert_eq!(program_error(&mut m), (message, 74, arg), "{source}");
        }
    }
    assert!(workspace.join("TA").is_file());
    for project in [&workspace, &dir.0.join("GAME")] {
        for file in ["TB", "TX", "GX", "JX"] {
            assert!(!project.join(file).exists(), "{file}");
        }
    }

    m.set_slot(0, "EXEC \"/A\",1\n?\"PAST\"".to_string());
    enter(&mut m, "run");
    assert_eq!(
        tail(&m, 2),
        ["Can't access other project in this mode in 0:1", "OK"]
    );

    // At the prompt, SAVEV asks as set and saves.
    m.rt.settings.file_confirm = FileConfirm::Ask;
    enter(&mut m, "project \"\"");
    let c = ask(&mut m, "savev \"txt:/x\",\"1\":?result()");
    assert_eq!(c.lines[3], "  File:     TXT:X");
    m.rt.answer(true);
    settle(&mut m);
    assert_eq!(tail(&m, 2), ["1", "OK"]);
    assert!(workspace.join("TX").is_file());
    m.rt.settings.file_confirm = FileConfirm::Silent;
    enter(&mut m, "project \"game\"");
    enter(&mut m, "savev \"txt:game/x\",\"1\":?result()");
    assert_eq!(tail(&m, 2), ["1", "OK"]);
    assert!(dir.0.join("GAME").join("TX").is_file());
    // Another project's name, the workspace's here, at the prompt.
    enter(&mut m, "savev \"txt:/y\",\"1\":?result()");
    assert_eq!(tail(&m, 2), ["1", "OK"]);
    assert!(workspace.join("TY").is_file());
}

/// SAVE, SAVEV and SAVEG at the prompt and in a subprogram write to any
/// project named, which must exist, and keep a backup there.
#[test]
fn the_prompt_and_subprograms_save_in_any_project() {
    let dir = TempDir::new("save-projects");
    let mut m = machine(&dir);
    m.rt.storage.create_project("GAME").unwrap();
    let (workspace, game) = (dir.0.join("@WORKSPACE"), dir.0.join("GAME"));
    m.set_slot(1, "?1".to_string());
    enter(
        &mut m,
        "save \"game/p\",1:a=result():save \"txt:game/p\",1:savev \"txt:game/v\",\"1\"\
         :b=result():saveg \"game/g\",0,0,0,4,4:?a;b;result()",
    );
    assert_eq!(tail(&m, 2), ["111", "OK"]);
    for file in ["TP", "T@BACKUP.PRG", "TV", "GG"] {
        assert!(game.join(file).is_file(), "{file}");
    }
    enter(&mut m, "save \"none/p\",1:?result()");
    assert_eq!(tail(&m, 2), ["0", "OK"]);
    assert!(!dir.0.join("NONE").exists());

    // The subprogram runs in project GAME and saves in the workspace.
    enter(&mut m, "project \"game\"");
    m.set_slot(
        1,
        "SAVE \"/P\",1:SAVEV \"TXT:/V\",\"1\":SAVEG \"JPG:/G\",0,0,0,4,4\nEND".to_string(),
    );
    enter(&mut m, "save \"sub\",1");
    enter(&mut m, "subrun \"sub\"");
    for _ in 0..10 {
        m.frame();
    }
    assert!(m.take_program_error().is_none());
    for file in ["TP", "TV", "JG"] {
        assert!(workspace.join(file).is_file(), "{file}");
    }
}

/// Runs a direct-mode line on a clear console, and gives what it printed
/// after itself, put together again from the rows it wraps on.
fn direct_error(m: &mut Machine, line: &str) -> String {
    enter(m, "cls");
    enter(m, line);
    let rows = rows(m);
    // CLS's OK, the line, what it printed, and OK.
    let text = rows[1..rows.len() - 1].concat();
    assert_eq!(rows.last().map(String::as_str), Some("OK"));
    let echo = line.to_ascii_uppercase();
    let printed = text.strip_prefix(&echo);
    printed.unwrap_or_else(|| panic!("{text:?}")).to_string()
}

/// SAVE, SAVEV and SAVEG into a `#` project are error 75 in a main
/// program (before its 74), in a subprogram and in DIRECT mode.
#[test]
fn saves_into_a_system_project_are_refused_everywhere() {
    let dir = TempDir::new("system-project");
    let mut m = machine(&dir);
    m.rt.storage.create_project("#SYS").unwrap();
    let commands = [
        ("SAVE \"#SYS/X\",1", "SAVE"),
        ("SAVEV \"TXT:#SYS/X\",\"1\"", "SAVEV"),
        ("SAVEG \"GRP:#SYS/X\",0", "SAVEG"),
    ];
    for (source, command) in commands {
        m.set_slot(0, format!("A=1\n{source}"));
        enter(&mut m, "run");
        let message = format!("Can't SAVE to system project in 0:2({command})");
        assert_eq!(program_error(&mut m), (message, 75, None), "{source}");

        let message = format!("Can't SAVE to system project ({command})");
        assert_eq!(direct_error(&mut m, &source.to_ascii_lowercase()), message);

        m.set_slot(1, format!("A=1\n{source}"));
        enter(&mut m, "save \"sub\",1");
        enter(&mut m, "subrun \"sub\"");
        for _ in 0..10 {
            m.frame();
        }
        let (e, sub) = m.take_program_error().expect("an error");
        let message = format!("Can't SAVE to system project in 0:2({command})");
        assert!(sub);
        assert_eq!((e.to_string(), e.kind.code()), (message, 75), "{source}");
    }
    let files = std::fs::read_dir(dir.0.join("#SYS")).map_or(0, Iterator::count);
    assert_eq!(files, 0);
}

/// RENAME and DELETE refuse a name with a project at the prompt and in a
/// subprogram too.
#[test]
fn rename_and_delete_refuse_projects_everywhere() {
    let dir = TempDir::new("rename-projects");
    let mut m = machine(&dir);
    enter(&mut m, "savev \"txt:a\",\"1\"");
    for (line, command) in [
        ("rename \"txt:/a\",\"b\"", "RENAME:1"),
        ("rename \"a\",\"/b\"", "RENAME:2"),
        ("delete \"/a\"", "DELETE:1"),
    ] {
        let message = format!("Can't access other project in this mode ({command})");
        assert_eq!(direct_error(&mut m, line), message, "{line}");
    }
    m.set_slot(1, "DELETE \"TXT:/A\"".to_string());
    enter(&mut m, "save \"sub\",1");
    enter(&mut m, "subrun \"sub\"");
    for _ in 0..10 {
        m.frame();
    }
    let (e, sub) = m.take_program_error().expect("an error");
    assert!(sub);
    let message = "Can't access other project in this mode in 0:1(DELETE:1)";
    assert_eq!((e.to_string().as_str(), e.kind.code()), (message, 74));
    assert!(dir.0.join("@WORKSPACE").join("TA").is_file());
}

/// LOADG's types of file, the same in every mode: `PNG:` is error 74,
/// a type that is none Illegal format, `TXT:` File type mismatch, and a
/// JPG file loads.
#[test]
fn loadg_types_of_file() {
    let dir = TempDir::new("loadg-types");
    let mut m = machine(&dir);
    enter(
        &mut m,
        "gcls &hff112233:saveg \"jpg:p\",0,0,0,4,4:savev \"txt:t\",\"1\"",
    );
    for (line, message) in [
        (
            "loadg \"png:p\",0",
            "Can't access other project in this mode (LOADG)",
        ),
        ("loadg \"xyz:p\",0", "Illegal format (LOADG:1) (\"XYZ:P\")"),
        ("loadg \"txt:t\",0", "File type mismatch (LOADG) (TXT file)"),
        ("load \"dat:x\",1", "File type mismatch (LOAD) (DAT file)"),
    ] {
        assert_eq!(direct_error(&mut m, line), message, "{line}");
    }
    enter(
        &mut m,
        "gcls:loadg \"jpg:p\",0 out w,h:?result();w;h;gpget(0,0)!=0",
    );
    assert_eq!(tail(&m, 2), ["1441", "OK"]);

    for (source, message, code, arg) in [
        (
            "LOADG \"XYZ:P\",0",
            "Illegal format in 0:2(LOADG:1) (\"XYZ:P\")",
            71,
            Some(0),
        ),
        (
            "LOADG \"TXT:T\",0",
            "File type mismatch in 0:2(LOADG) (TXT file)",
            77,
            None,
        ),
        // Before the file is looked for.
        (
            "S$=\"\"\nLOADV \"GRP:NONE\",S$",
            "Type mismatch in 0:2(LOADV:2)",
            8,
            Some(1),
        ),
    ] {
        let source = match source.contains('\n') {
            true => source.to_string(),
            false => format!("A=1\n{source}"),
        };
        m.set_slot(0, source.clone());
        enter(&mut m, "run");
        assert_eq!(
            program_error(&mut m),
            (message.to_string(), code, arg),
            "{source}"
        );
    }

    m.set_slot(1, "LOADG \"PNG:P\",0".to_string());
    enter(&mut m, "save \"sub\",1");
    enter(&mut m, "subrun \"sub\"");
    for _ in 0..10 {
        m.frame();
    }
    let (e, sub) = m.take_program_error().expect("an error");
    assert!(sub);
    assert_eq!(e.kind.code(), 74);
}

/// ENVSAVE in a main program copies nothing and gives no error, for a
/// kind it takes.
#[test]
fn envsave_runs_in_a_main_program() {
    let dir = TempDir::new("envsave");
    let mut m = machine(&dir);
    m.set_slot(
        0,
        "ENVSAVE 1,0,3:ENVSAVE 3,0,0:ENVSAVE 4,0,0:?\"PAST\"".to_string(),
    );
    enter(&mut m, "run");
    assert_eq!(tail(&m, 2), ["PAST", "OK"]);
    // Its kind is checked: 0 is Illegal function call, 5 Out of range.
    for (kind, message, code) in [
        (
            0,
            "Illegal function call in 0:2(ENVSAVE:1) (program slot)",
            4,
        ),
        (5, "Out of range in 0:2(ENVSAVE:1) (kind 5)", 10),
    ] {
        m.set_slot(0, format!("A=1\nENVSAVE {kind},0,0"));
        enter(&mut m, "run");
        assert_eq!(program_error(&mut m), (message.to_string(), code, Some(0)));
    }
}

/// PROJECT in a program stops it with error 44 when it runs, while
/// CLEAR stays a Syntax error.
#[test]
fn project_in_a_program_cannot_be_used() {
    let dir = TempDir::new("project-in-program");
    let mut m = machine(&dir);
    m.set_slot(0, "?\"A\"\nPROJECT \"\"".to_string());
    enter(&mut m, "run");
    let message = "Can't use in program in 0:2(PROJECT)".to_string();
    assert_eq!(program_error(&mut m), (message, 44, None));
    m.set_slot(0, "?\"A\"\nCLEAR".to_string());
    enter(&mut m, "run");
    assert_eq!(program_error(&mut m).1, 3);
}

/// TEST69: LOADV of a DAT file into a string, and RESULT 0 after CHKFILE
/// of a file that is there.
#[test]
fn loadv_into_a_string_and_chkfile_result() {
    let dir = TempDir::new("test69-loadv");
    let mut m = machine(&dir);
    enter(&mut m, "dim a%[3]:savev \"dat:d\",a%");
    m.set_slot(0, "S$=\"\"\nLOADV \"DAT:D\",S$".to_string());
    enter(&mut m, "run");
    let message = "Type mismatch in 0:2(LOADV:2)".to_string();
    assert_eq!(program_error(&mut m), (message, 8, Some(1)));
    m.set_slot(0, "S$=LOADV(\"DAT:D\"):?LEN(S$);RESULT()".to_string());
    enter(&mut m, "run");
    assert_eq!(tail(&m, 2), ["31", "OK"]);

    enter(&mut m, "savev \"txt:y\",\"1\"");
    m.set_slot(0, "B=CHKFILE(\"TXT:Y\"):?B;RESULT()".to_string());
    enter(&mut m, "run");
    assert_eq!(tail(&m, 2), ["10", "OK"]);
}

/// RUN sets RESULT to 0 once the program compiles, and CHKFILE leaves it
/// as it was, whether the file is there or not.
#[test]
fn run_sets_result_and_chkfile_leaves_it() {
    let dir = TempDir::new("run-result");
    let mut m = machine(&dir);
    enter(&mut m, "savev \"txt:y\",\"1\":?result()");
    assert_eq!(tail(&m, 2), ["1", "OK"]);
    m.set_slot(0, "?RESULT()".to_string());
    enter(&mut m, "run");
    assert_eq!(tail(&m, 2), ["0", "OK"]);

    let program = "SAVEV \"TXT:Y\",\"1\":A=RESULT()\n\
                   B=CHKFILE(\"TXT:Y\"):C=CHKFILE(\"TXT:NONE\"):?A;B;C;RESULT()";
    m.set_slot(0, program.to_string());
    enter(&mut m, "run");
    assert_eq!(tail(&m, 2), ["1101", "OK"]);
    enter(&mut m, "?chkfile(\"txt:none\");result()");
    assert_eq!(tail(&m, 2), ["01", "OK"]);

    // A program that does not compile leaves RESULT as it was.
    m.set_slot(0, "?RESULT(\n".to_string());
    enter(&mut m, "run");
    enter(&mut m, "?result()");
    assert_eq!(tail(&m, 2), ["1", "OK"]);
}

/// LOADG of a file named with a project asks at the prompt as LOAD does,
/// and loads without asking in a program, for the program's own project
/// and another, with or without `GRP:`.
#[test]
fn loadg_of_a_project_name_asks_only_at_the_prompt() {
    use sb_runtime::storage::FileKind;
    let dir = TempDir::new("loadg-ask");
    let mut m = machine(&dir);
    let s = &mut m.rt.storage;
    s.create_project("GAME").unwrap();
    s.set_project("GAME").unwrap();
    s.write(FileKind::Txt, "SUB", b"", None).unwrap();
    enter(&mut m, "saveg \"pic\",0,0,0,4,4");

    m.rt.settings.file_confirm = FileConfirm::Ask;
    let program = "LOADG \"GAME/PIC\",0:?RESULT()\nLOADG \"GRP:GAME/PIC\",0:?RESULT()";
    m.set_slot(0, program.to_string());
    for project in ["game", ""] {
        enter(&mut m, &format!("project \"{project}\""));
        enter(&mut m, "run");
        assert_eq!(tail(&m, 3), ["1", "1", "OK"], "{project:?}");
        assert!(m.rt.take_notices().is_empty(), "{project:?}");
    }

    for line in [
        "loadg \"game/pic\",0:?result()",
        "loadg \"grp:game/pic\",0:?result()",
    ] {
        let c = ask(&mut m, line);
        assert_eq!(c.title, "Load file");
        assert_eq!(
            c.lines[..4],
            [
                "Load this file.",
                "",
                "  Project:  [WORKSPACE]",
                "  File:     GRP:GAME/PIC",
            ]
        );
        m.rt.answer(true);
        settle(&mut m);
    }
    assert_eq!(tail(&m, 2), ["1", "OK"]);
    let c = ask(&mut m, "load \"game/sub\",1");
    assert_eq!(c.title, "Slot 1: Load file");
    m.rt.answer(true);
    settle(&mut m);

    enter(&mut m, "project \"game\"");
    let c = ask(&mut m, "loadg \"grp:game/pic\",0:?result()");
    assert_eq!(c.title, "Load file");
    m.rt.answer(true);
    settle(&mut m);
    assert_eq!(tail(&m, 2), ["1", "OK"]);

    m.rt.settings.file_confirm = FileConfirm::Notify;
    enter(&mut m, "project \"\"");
    enter(&mut m, "loadg \"game/pic\",0");
    assert_eq!(m.rt.take_notices(), ["Loading GRP:GAME/PIC"]);
}

/// Checked on hardware, although the manual says that loading frees them.
#[test]
fn loading_a_slot_keeps_its_definitions() {
    use sb_runtime::storage::FileKind;
    let dir = TempDir::new("reload");
    let mut m = machine(&dir);
    (m.rt.storage)
        .write(FileKind::Txt, "OTHER", b"?\"O\"", None)
        .unwrap();
    m.set_slot(1, "END\nCOMMON DEF F()\nRETURN 7\nEND".to_string());
    m.set_slot(0, "EXEC 1:?F()\nLOAD \"OTHER\",1:?F()".to_string());
    enter(&mut m, "run");
    assert_eq!(tail(&m, 3), ["7", "7", "OK"]);
}

/// Files and projects that another program adds, replaces or removes
/// between commands: each command reads the folder as it is now.
#[test]
fn changes_made_outside_show_at_the_next_command() {
    let dir = TempDir::new("outside");
    let mut m = machine(&dir);
    let workspace = dir.0.join("@WORKSPACE");
    enter(&mut m, "save \"a\"");

    std::fs::write(workspace.join("TNEW"), "?\"NEW\"").unwrap();
    std::fs::write(workspace.join("BVALUES"), "").unwrap();
    // No type has the letter X: it stays out, as at the start.
    std::fs::write(workspace.join("XSTRAY"), "").unwrap();
    enter(&mut m, "files \"txt:\"");
    assert_eq!(tail(&m, 3), ["TXT:A", "TXT:NEW", "OK"]);
    enter(&mut m, "?chkfile(\"new\");len(files())");
    assert_eq!(tail(&m, 2), ["13", "OK"]);
    enter(&mut m, "load \"new\",1");
    assert_eq!(m.slot(1), "?\"NEW\"");

    // Replaced, and renamed over: LOAD and LOADV read the new contents.
    std::fs::write(workspace.join("TNEW"), "?\"NEWER\"").unwrap();
    enter(&mut m, "load \"new\",1");
    assert_eq!(m.slot(1), "?\"NEWER\"");
    std::fs::write(workspace.join("TOTHER"), "?\"OTHER\"").unwrap();
    std::fs::rename(workspace.join("TOTHER"), workspace.join("TNEW")).unwrap();
    enter(&mut m, "?loadv(\"txt:new\")");
    assert_eq!(tail(&m, 2), ["?\"OTHER\"", "OK"]);

    // Removed: gone from the list, and missing to the commands.
    std::fs::remove_file(workspace.join("TNEW")).unwrap();
    enter(&mut m, "files \"txt:\"");
    assert_eq!(tail(&m, 2), ["TXT:A", "OK"]);
    enter(&mut m, "load \"new\"");
    assert_eq!(tail(&m, 2), ["LOAD failed: TXT:NEW not found", "OK"]);
    enter(&mut m, "?chkfile(\"new\")");
    assert_eq!(tail(&m, 2), ["0", "OK"]);
    enter(&mut m, "rename \"new\",\"x\"");
    assert!(
        tail(&m, 2)[0].starts_with("RENAME failed"),
        "{:?}",
        tail(&m, 2)
    );
    enter(&mut m, "delete \"new\"");
    assert!(
        tail(&m, 2)[0].starts_with("DELETE failed"),
        "{:?}",
        tail(&m, 2)
    );

    // SAVE asks about replacing a file only while one is there.
    m.rt.settings.file_confirm = FileConfirm::Ask;
    std::fs::write(workspace.join("TB"), "").unwrap();
    let replaced = "The file there now will be replaced.".to_string();
    assert!(ask(&mut m, "save \"b\"").lines.contains(&replaced));
    m.rt.answer(false);
    settle(&mut m);
    std::fs::remove_file(workspace.join("TB")).unwrap();
    assert!(!ask(&mut m, "save \"b\"").lines.contains(&replaced));
    m.rt.answer(false);
    settle(&mut m);
    m.rt.settings.file_confirm = FileConfirm::Silent;

    // Project folders, listed and switched to.
    std::fs::create_dir(dir.0.join("GAME")).unwrap();
    std::fs::write(dir.0.join("GAME").join("TMAIN"), "?1").unwrap();
    enter(&mut m, "files \"//\"");
    assert_eq!(tail(&m, 3), ["/", "GAME/", "OK"]);
    enter(&mut m, "project \"game\":files");
    assert_eq!(tail(&m, 2), ["TXT:MAIN", "OK"]);
    enter(&mut m, "project \"\"");
    std::fs::remove_dir_all(dir.0.join("GAME")).unwrap();
    enter(&mut m, "files \"//\"");
    assert_eq!(tail(&m, 2), ["/", "OK"]);
    enter(&mut m, "project \"game\"");
    assert!(tail(&m, 3).concat().contains("Undefined project name"));
}

/// A save to a project that is not there asks first, as SmileBASIC 4
/// does, and then fails with RESULT 0.
#[test]
fn saves_to_a_missing_project_ask_and_then_fail() {
    let dir = TempDir::new("missing-project");
    let mut m = machine(&dir);
    m.rt.settings.file_confirm = FileConfirm::Ask;
    for (line, command) in [
        ("savev \"txt:none/x\",\"1\":?result()", "SAVEV"),
        ("save \"none/x\":?result()", "SAVE"),
        ("saveg \"none/x\",0:?result()", "SAVEG"),
    ] {
        ask(&mut m, line);
        m.rt.answer(true);
        settle(&mut m);
        let failed = format!("{command} failed: NONE not found");
        assert_eq!(tail(&m, 3), [failed.as_str(), "0", "OK"], "{line}");
    }
    assert!(!dir.0.join("NONE").exists());
}

/// Runs the program in slot 0 until it waits on a dialog.
fn run_to_dialog(m: &mut Machine) -> sb_runtime::dialog::ProgramDialog {
    m.type_text("run");
    m.rt.key_press(keys::ENTER);
    for _ in 0..10 {
        m.frame();
        if let Some(d) = m.program_dialog() {
            return d.clone();
        }
    }
    panic!("no dialog: {:?}", rows(m));
}

#[test]
fn a_programs_file_failure_waits_on_a_dialog() {
    use sb_runtime::dialog::DialogAnswer;
    let dir = TempDir::new("failure-dialog");
    let mut m = machine(&dir);
    m.rt.settings.file_confirm = FileConfirm::Ask;
    std::fs::create_dir_all(dir.0.join("@WORKSPACE")).unwrap();
    m.set_slot(0, "LOADG \"DAT:NONE\",1\n?\"PAST\";RESULT()".to_string());
    let d = run_to_dialog(&mut m);
    assert_eq!(d.caption, "LOADG failed");
    assert_eq!(d.lines, ["DAT:NONE not found"]);
    // It waits for an answer, however long.
    for _ in 0..300 {
        m.frame();
    }
    assert!(m.is_running() && m.program_dialog().is_some());
    m.answer_dialog(DialogAnswer::Choice(0));
    settle(&mut m);
    assert_eq!(tail(&m, 2), ["PAST0", "OK"]);

    // The A button closes it too.
    m.set_slot(
        0,
        "DIM A%[1]:LOADV \"DAT:NONE\",A%\n?\"PAST\";RESULT()".to_string(),
    );
    run_to_dialog(&mut m);
    let a = 1 << sb_runtime::input::A;
    for buttons in [a, a, 0] {
        m.rt.input.live.pads[0].buttons = buttons;
        m.frame();
    }
    settle(&mut m);
    assert_eq!(tail(&m, 2), ["PAST0", "OK"]);

    // A function returns its value once the dialog closes.
    m.set_slot(
        0,
        "A$=LOADV(\"TXT:NONE\")\n?\"PAST\";LEN(A$)\n?RESULT()".to_string(),
    );
    run_to_dialog(&mut m);
    m.answer_dialog(DialogAnswer::Choice(0));
    settle(&mut m);
    assert_eq!(tail(&m, 3), ["PAST0", "0", "OK"]);

    // EXEC of a missing file is Load failed once it closes.
    m.set_slot(0, "EXEC \"NONE\",1\n?\"PAST\"".to_string());
    let d = run_to_dialog(&mut m);
    assert_eq!(d.caption, "EXEC failed");
    m.answer_dialog(DialogAnswer::Choice(0));
    settle(&mut m);
    assert!(tail(&m, 2)[0].starts_with("Load failed"), "{:?}", rows(&m));

    // A question before the command is not asked again.
    std::fs::remove_dir_all(dir.0.join("@WORKSPACE")).unwrap();
    std::fs::write(dir.0.join("@WORKSPACE"), b"").unwrap();
    m.set_slot(0, "SAVEV \"TXT:X\",\"A\"\n?\"PAST\";RESULT()".to_string());
    let c = ask(&mut m, "run");
    assert_eq!(c.title, "Save file");
    m.rt.answer(true);
    let d = run_to_dialog_now(&mut m);
    assert_eq!(d.caption, "SAVEV failed");
    m.answer_dialog(DialogAnswer::Choice(0));
    settle(&mut m);
    assert!(m.rt.confirmation().is_none());
    assert_eq!(tail(&m, 2), ["PAST0", "OK"]);

    // When file access notifies, a failure is a notice and nothing waits.
    m.rt.settings.file_confirm = FileConfirm::Notify;
    std::fs::remove_file(dir.0.join("@WORKSPACE")).unwrap();
    m.set_slot(0, "LOADG \"DAT:NONE\",1:?RESULT()".to_string());
    m.rt.take_notices();
    enter(&mut m, "run");
    assert_eq!(tail(&m, 2), ["0", "OK"]);
    assert_eq!(m.rt.take_notices(), ["LOADG failed: DAT:NONE not found"]);
}

/// Runs frames of a running program until it waits on a dialog.
fn run_to_dialog_now(m: &mut Machine) -> sb_runtime::dialog::ProgramDialog {
    for _ in 0..10 {
        m.frame();
        if let Some(d) = m.program_dialog() {
            return d.clone();
        }
    }
    panic!("no dialog: {:?}", rows(m));
}

#[test]
fn files_of_projects_and_of_a_files_name() {
    let dir = TempDir::new("files-forms");
    let mut m = machine(&dir);
    m.set_slot(0, "?1".to_string());
    enter(&mut m, "save \"a\"");
    m.rt.storage.create_project("-DASH").unwrap();
    m.rt.storage.create_project("GAME").unwrap();
    // The count first, and the workspace's `/` after a `-` name.
    enter(&mut m, "cls:files \"//\"");
    assert_eq!(rows(&m), ["-- 3 projects --", "-DASH/", "/", "GAME/", "OK"]);
    // A file's name is taken for a project's, which is not there.
    enter(&mut m, "cls:files \"txt:a\"");
    assert_eq!(rows(&m), ["OK"]);
    enter(&mut m, "cls:files \"txt:\"");
    assert_eq!(rows(&m), ["-- 1 files --", "TXT:A", "OK"]);
}

/// The folder of a program run from the command line, as the current
/// project, with plain names.
fn user_folder(dir: &TempDir) -> std::path::PathBuf {
    let folder = dir.0.join("hgtest");
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(folder.join("HGLIB.PRG.txt"), "'LIB").unwrap();
    std::fs::write(folder.join("HGTEST.PRG.txt"), "'TEST").unwrap();
    folder
}

#[test]
fn a_folder_opened_as_the_project_loads_and_saves_beside_the_program() {
    let dir = TempDir::new("folder");
    let folder = user_folder(&dir);
    let mut m = machine(&dir);
    assert_eq!(m.rt.storage.open_folder(&folder).unwrap(), "HGTEST");
    let program = "LOAD \"HGLIB.PRG\",1\nSAVEV \"TXT:OUT\",\"SAVED\"\n?RESULT()\n?PROJECT()";
    m.set_slot(0, program.to_string());
    enter(&mut m, "run");
    assert_eq!(tail(&m, 3), ["1", "HGTEST", "OK"]);
    assert_eq!(m.slot(1), "'LIB");
    assert_eq!(std::fs::read(folder.join("OUT.txt")).unwrap(), b"SAVED");
    enter(&mut m, "cls:files");
    assert_eq!(
        rows(&m),
        [
            "-- 3 files --",
            "TXT:HGLIB.PRG",
            "TXT:HGTEST.PRG",
            "TXT:OUT",
            "OK"
        ]
    );
    // Away to the workspace and back by the folder's name.
    enter(&mut m, "project \"\":?chkfile(\"hglib.prg\")");
    assert_eq!(tail(&m, 2), ["0", "OK"]);
    enter(&mut m, "project \"hgtest\":?chkfile(\"hglib.prg\")");
    assert_eq!(tail(&m, 2), ["1", "OK"]);
}

#[test]
fn changing_a_file_that_was_in_the_folder_asks_once_whatever_the_setting() {
    let dir = TempDir::new("folder-ask");
    let folder = user_folder(&dir);
    let mut m = machine(&dir);
    m.rt.storage.open_folder(&folder).unwrap();
    m.set_slot(0, "SAVEV \"TXT:HGLIB.PRG\",\"'NEW\"\n?RESULT()".to_string());
    let c = ask(&mut m, "run");
    assert_eq!(
        c.lines[c.lines.len() - 2..],
        [
            "This changes the original file HGLIB.PRG.txt",
            "in the folder the program was run from."
        ]
    );
    assert!(!c.accept_first);
    m.rt.answer(true);
    settle(&mut m);
    assert_eq!(tail(&m, 2), ["1", "OK"]);
    assert_eq!(
        std::fs::read(folder.join("HGLIB.PRG.txt")).unwrap(),
        b"'NEW"
    );
    let backup = std::fs::read(folder.join("@BACKUP.TXT.txt")).unwrap();
    assert_eq!(backup, b"'LIB");
    // Once is enough; the setting says the rest.
    enter(&mut m, "run");
    assert_eq!(tail(&m, 2), ["1", "OK"]);

    // In DIRECT mode with notices too, and for DELETE.
    m.rt.settings.file_confirm = FileConfirm::Notify;
    let c = ask(&mut m, "delete \"hgtest.prg\":?result()");
    assert_eq!(c.title, "Delete file");
    m.rt.answer(false);
    settle(&mut m);
    assert_eq!(tail(&m, 2), ["-1", "OK"]);
    assert!(folder.join("HGTEST.PRG.txt").is_file());
    // Ctrl+S asks by this too.
    assert_eq!(m.rt.original_txt("HGTEST.PRG"), ["HGTEST.PRG.txt"]);
    assert!(m.rt.original_txt("NEW.PRG").is_empty());
}

/// CON, PRN, NUL, AUX, COM1 to COM4 and LPT1 to LPT4 are no file's or
/// project's name, on every system: Illegal format. COM5 and the like are
/// names like any other.
#[test]
fn reserved_names_are_illegal_format() {
    let dir = TempDir::new("reserved-names");
    let mut m = machine(&dir);
    for (line, message) in [
        (
            "?chkfile(\"txt:con\")",
            "Illegal format (CHKFILE:1) (\"TXT:CON\")",
        ),
        (
            "?chkfile(\"nul/a\")",
            "Illegal format (CHKFILE:1) (\"NUL/A\")",
        ),
        (
            "savev \"txt:com4\",\"x\"",
            "Illegal format (SAVEV:1) (\"TXT:COM4\")",
        ),
        ("save \"lpt1/a\"", "Illegal format (SAVE:1) (\"LPT1/A\")"),
        (
            "rename \"a\",\"aux\"",
            "Illegal format (RENAME:2) (\"AUX\")",
        ),
        (
            "files \"txt:prn\"",
            "Illegal format (FILES:1) (\"TXT:PRN\")",
        ),
    ] {
        assert_eq!(direct_error(&mut m, line), message, "{line}");
    }
    enter(
        &mut m,
        "cls:savev \"txt:com5\",\"x\":?result():?loadv(\"txt:com5\")",
    );
    assert_eq!(rows(&m), ["1", "X", "OK"]);
    enter(
        &mut m,
        "cls:savev \"txt:con.a\",\"y\":?chkfile(\"txt:con.a\")",
    );
    assert_eq!(rows(&m), ["1", "OK"]);
    m.rt.storage.create_project("LPT9").unwrap();
    enter(
        &mut m,
        "cls:savev \"txt:lpt9/b\",\"z\":?loadv(\"txt:lpt9/b\")",
    );
    assert_eq!(rows(&m), ["Z", "OK"]);
    assert!(
        m.rt.storage
            .projects()
            .unwrap()
            .contains(&"LPT9".to_string())
    );
}
