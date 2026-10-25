//! File commands in direct mode and in programs, on a temporary folder.

use std::path::PathBuf;

use sb_runtime::Runtime;
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
    enter(&mut m, "files");
    assert_eq!(tail(&m, 3), ["TXT:@BACKUP.PRG", "TXT:GAME.PRG", "OK"]);
    enter(&mut m, "?chkfile(\"game.prg\");chkfile(\"grp:game.prg\")");
    assert_eq!(tail(&m, 2), ["10", "OK"]);
}

#[test]
fn projects_rename_delete() {
    let dir = TempDir::new("project");
    let mut m = machine(&dir);
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
