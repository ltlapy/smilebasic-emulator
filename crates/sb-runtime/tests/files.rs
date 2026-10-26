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
    assert_eq!(tail(&m, 2), ["B", "OK"]);
    enter(&mut m, "delete \"b\":f$=files():?len(f$)");
    assert_eq!(tail(&m, 2), ["0", "OK"]);
    enter(&mut m, "files \"//\"");
    assert_eq!(tail(&m, 2), ["MYGAME", "OK"]);
    enter(&mut m, "dim n$[0]:files \"//\",n$:?n$[0]");
    assert_eq!(tail(&m, 2), ["MYGAME", "OK"]);
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
    m.set_slot(0, "LOAD \"NOPE\":?RESULT()\nPROJECT \"X\"".to_string());
    enter(&mut m, "run");
    let r = tail(&m, 3);
    assert_eq!(r[0], "0");
    assert!(r[1].starts_with("Illegal statement in 2"), "{r:?}");
    enter(&mut m, "new");
    assert_eq!(m.slot(0), "");
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
