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
    let mut rt = Runtime::new(400, 240, Font::blank());
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

    enter(&mut m, "load \"game.prg\",2:run 2");
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
fn projects_rename_delete() {
    let dir = TempDir::new("project");
    let mut m = machine(&dir);
    // PROJECT only switches; projects are made in the project manager.
    enter(&mut m, "project \"mygame\"");
    assert!(tail(&m, 2)[0].starts_with("Undefined project name"));
    enter(&mut m, "project \"/\"");
    assert!(tail(&m, 2)[0].starts_with("Illegal project name"));
    m.rt.storage.create_project("MYGAME").unwrap();
    enter(&mut m, "project \"mygame\":?project()");
    assert_eq!(tail(&m, 2), ["MYGAME", "OK"]);
    enter(&mut m, "save \"a\":rename \"a\",\"b\":files \"txt:\"");
    assert_eq!(tail(&m, 2), ["TXT:B", "OK"]);
    enter(&mut m, "delete \"b\":f$=files():?len(f$)");
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
    assert!(r[1].starts_with("Syntax error in 0:2"), "{r:?}");
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
            "To stop these questions, choose File access",
            "in the Options menu (top of the window).",
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
    // A longer title is cut; another project starts with its name.
    m.set_slot(
        0,
        "METAEDIT 0,\"0123456789012345678901234\":METAEDIT 0 OUT T$:?LEN(T$)".to_string(),
    );
    enter(&mut m, "run");
    assert_eq!(tail(&m, 2), ["24", "OK"]);
    m.rt.storage.create_project("GAME").unwrap();
    m.set_slot(0, "METAEDIT 0 OUT T$:?T$".to_string());
    enter(&mut m, "project \"game\":run");
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
    assert!(tail(&m, 2)[0].starts_with("Undefined project name"));
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

    // Programs read them too, with LOAD, LOADV and EXEC (checked on
    // hardware).
    m.set_slot(
        0,
        "LOAD \"GAME/SUB\",1:?RESULT();LOADV(\"TXT:GAME/NOTE\")\nEXEC \"GAME/SUB\",3".to_string(),
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
        "delete \"@backup.prg\":?result();chkfile(\"@backup.prg\")",
    );
    assert_eq!(tail(&m, 2)[0], "10");
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
