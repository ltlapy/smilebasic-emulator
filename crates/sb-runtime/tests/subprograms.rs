//! SUBRUN's subprograms on the sub interpreter, beside the main one.

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
        let dir = std::env::temp_dir().join(format!("sb-sub-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        TempDir(dir)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A machine whose workspace holds the program `SUB` with `source`.
fn machine(dir: &TempDir, source: &str) -> Machine {
    let mut rt = Runtime::new(400, 240, Font::blank());
    rt.storage = Storage::new(dir.0.clone());
    rt.settings.file_confirm = FileConfirm::Silent;
    let mut m = Machine::new(rt);
    m.boot();
    m.set_slot(1, source.to_string());
    enter(&mut m, "save \"sub\",1");
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

fn frames(m: &mut Machine, n: usize) {
    for _ in 0..n {
        m.frame();
    }
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

fn last_rows(m: &Machine, n: usize) -> Vec<String> {
    let rows = rows(m);
    rows[rows.len().saturating_sub(n)..].to_vec()
}

#[test]
fn a_subprogram_runs_beside_direct_mode_and_reaches_the_main_one() {
    let dir = TempDir::new("beside");
    let mut m = machine(
        &dir,
        "XSUBSCREEN 10,20,300,200\n\
         ENVSTAT OUT S,P,E\n\
         ENVLOAD 0,1,2\n\
         GPSET 0,0,#C_RED\n\
         ENVSAVE 1,0,3\n\
         ENVFOCUS 0\n\
         PUSHKEY \"?ENVTYPE(),\"+STR$(S)+\",\"+STR$(LEN(PRGNAME$(2)))+CHR$(13)\n\
         WHILE 1:VSYNC:WEND\n",
    );
    enter(&mut m, "subrun \"sub\"");
    assert!(m.sub_running(), "{:?} {:?}", m.rt.take_notices(), rows(&m));
    frames(&mut m, 5);
    // The main interpreter ran the line the subprogram typed: it is the
    // main one, it was in direct mode, and the slot came across by name.
    assert_eq!(last_rows(&m, 3), ["?ENVTYPE(),0,3", "0   0   3", "OK"]);
    let (fb, rect) = m.sub_screen().expect("shown");
    assert_eq!(rect, [10, 20, 300, 200]);
    assert_eq!((fb.width(), fb.height()), (400, 240));
    // ENVSAVE copied the subprogram's page 0 onto the main one's page 3.
    enter(&mut m, "gtarget 3:?hex$(gpget(0,0))");
    assert_eq!(last_rows(&m, 2)[0], "FFFF0000");
    enter(&mut m, "substop");
    frames(&mut m, 1);
    assert!(!m.sub_running());
    assert!(m.sub_screen().is_none());
}

#[test]
fn the_focus_takes_the_keys_and_comes_back_when_the_subprogram_ends() {
    let dir = TempDir::new("focus");
    let mut m = machine(
        &dir,
        "WHILE INKEY$()!=\"Q\":VSYNC:WEND\n\
         ?ENVFOCUS();\n",
    );
    enter(&mut m, "subrun \"sub\"");
    assert!(m.sub_has_focus());
    m.type_text("q");
    frames(&mut m, 3);
    assert!(!m.sub_running());
    assert!(!m.sub_has_focus());
    // Nothing reached the main interpreter's direct mode.
    assert_eq!(last_rows(&m, 1), ["OK"]);
}

#[test]
fn subprogram_commands_are_for_direct_mode_and_a_missing_file_fails() {
    let dir = TempDir::new("direct");
    let mut m = machine(&dir, "END\n");
    enter(&mut m, "?envtype():xsubscreen out x,y:?x;y");
    assert_eq!(last_rows(&m, 3)[..2], ["0", "00"]);
    m.set_slot(0, "SUBRUN \"SUB\"\n".to_string());
    m.run_slot(0);
    frames(&mut m, 2);
    assert!(
        rows(&m)
            .iter()
            .any(|r| r.starts_with("Syntax error in 0:1"))
    );
    enter(&mut m, "subrun \"nothing\"");
    assert!(rows(&m).iter().any(|r| r.starts_with("Load failed")));
    assert!(!m.sub_running());
}

#[test]
fn a_busy_subprogram_keeps_to_the_frames_time() {
    let dir = TempDir::new("busy");
    let mut m = machine(&dir, "WHILE 1:WEND\n");
    enter(&mut m, "subrun \"sub\"");
    m.set_slot(0, "WHILE 1:WEND\n".to_string());
    m.run_slot(0);
    let limit = std::time::Duration::from_millis(2);
    m.time_limit = Some(limit);
    let start = std::time::Instant::now();
    frames(&mut m, 5);
    // The sides share the limit, give or take the instructions between
    // looks at the clock; a subprogram with no limit of its own took
    // three times as long.
    let per_frame = start.elapsed() / 5;
    assert!(per_frame < limit * 2, "{per_frame:?}");
    assert!(m.sub_running() && m.is_running());
}
