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

#[test]
fn the_main_interpreter_tells_why_a_subprogram_stopped() {
    let dir = TempDir::new("stopped");
    let mut m = machine(&dir, "XSUBSCREEN 0,0,100,100\nSTOP\n");
    enter(&mut m, "subrun \"sub\"");
    frames(&mut m, 3);
    let rule = "=".repeat(27);
    let block = ["=== From subinterpreter ===", "Break on 0:2", rule.as_str()];
    assert_eq!(last_rows(&m, 3), block);
    assert!(!m.sub_has_focus());

    m.set_slot(1, "ENVFOCUS 0:LOOP:VSYNC:ENDLOOP\n".to_string());
    enter(&mut m, "save \"sub\",1");
    enter(&mut m, "subrun \"sub\"");
    frames(&mut m, 2);
    enter(&mut m, "substop");
    let block = [
        "=== From subinterpreter ===",
        "User break on 0:1",
        rule.as_str(),
        "OK",
    ];
    assert_eq!(last_rows(&m, 4), block);
}

#[test]
fn envstat_0_drops_the_main_programs_stop() {
    let dir = TempDir::new("envstat");
    let mut m = machine(&dir, "ENVSTAT 0\n");
    m.set_slot(0, "STOP\n?\"GONE ON\"\n".to_string());
    m.run_slot(0);
    frames(&mut m, 2);
    enter(&mut m, "subrun \"sub\"");
    frames(&mut m, 2);
    enter(&mut m, "cont");
    assert!(last_rows(&m, 2)[0].starts_with("Can't continue"));
}

#[test]
fn the_side_without_the_focus_shows_darker() {
    let dir = TempDir::new("dim");
    let mut m = machine(
        &dir,
        "ENVTYPE OUT T,P:XSUBSCREEN OUT X,Y,W,H\n\
         ENVFOCUS 0\n\
         PUSHKEY \"?\"+STR$(P)+\",\"+STR$(W)+CHR$(13)\n\
         VSYNC 5:ENVFOCUS 1\n\
         LOOP:VSYNC:ENDLOOP\n",
    );
    enter(&mut m, "backcolor #c_white");
    enter(&mut m, "subrun \"sub\"");
    frames(&mut m, 2);
    // ENVFOCUS 0: the subprogram's own screen goes darker, not the main.
    assert_eq!(m.render().get(399, 239), Some(0xFFFF_FFFF));
    frames(&mut m, 8);
    // A subprogram's ENVTYPE parameter, and its XSUBSCREEN before one
    // is set.
    assert!(
        rows(&m)
            .iter()
            .any(|r| r.starts_with("-1") && r.ends_with("1280"))
    );
    assert_eq!(m.render().get(399, 239), Some(0xFFE0_E0E0));
}

#[test]
fn errors_carry_their_number_and_a_line_under_them() {
    let dir = TempDir::new("errnum");
    let mut m = machine(
        &dir,
        "ENVSTAT OUT S,N,A,SL,C,L\n\
         ENVFOCUS 0\n\
         PUSHKEY \"?\"+STR$(N)+\",\"+STR$(C)+CHR$(13)\n\
         LOOP:VSYNC:ENDLOOP\n",
    );
    m.rt.settings.localized_errors = true;
    m.set_slot(0, "'\nX=0:?1/X\n".to_string());
    m.run_slot(0);
    frames(&mut m, 2);
    assert_eq!(
        last_rows(&m, 3)[..2],
        ["Divide by zero in 0:2", "ゼロで除算しました"]
    );
    enter(&mut m, "subrun \"sub\"");
    frames(&mut m, 5);
    // Divide by zero is 7, in the statement at character 6.
    assert!(rows(&m).iter().any(|r| r == "7   6"), "{:?}", rows(&m));
}

#[test]
fn the_focus_moves_a_few_frames_after_envfocus() {
    let dir = TempDir::new("delay");
    let mut m = machine(
        &dir,
        "ENVFOCUS 0:N=0\n\
         WHILE ENVFOCUS():VSYNC:INC N:WEND\n\
         ENVFOCUS 1:M=0\n\
         WHILE !ENVFOCUS():VSYNC:INC M:WEND\n\
         ENVFOCUS 0:ENVFOCUS 1\n\
         PUSHKEY \"?\"+STR$(N)+STR$(M)+CHR$(13)\n\
         VSYNC 10\n\
         ENVFOCUS 1:VSYNC 5\n",
    );
    enter(&mut m, "subrun \"sub\"");
    frames(&mut m, 30);
    // Giving it away took two frames and taking it one. The ENVFOCUS 1
    // right after an ENVFOCUS 0 was lost, and the keys the subprogram
    // pushed went with the focus to the main interpreter.
    assert!(rows(&m).iter().any(|r| r == "21"), "{:?}", rows(&m));
}

#[test]
fn a_yellow_border_marks_where_the_focus_goes() {
    let dir = TempDir::new("border");
    let mut m = machine(&dir, "XSUBSCREEN 10,20,300,200\nVSYNC 30\n");
    enter(&mut m, "subrun \"sub\"");
    frames(&mut m, 3);
    assert_eq!(m.focus_border(), Some(([10, 20, 300, 200], 255)));
    // Long after, it has faded out; when the subprogram ends, the main
    // interpreter's whole display gets it.
    frames(&mut m, 28);
    assert!(!m.sub_running());
    assert_eq!(m.focus_border(), Some(([0, 0, 1280, 720], 255)));
    frames(&mut m, 14 + 8);
    assert!(m.focus_border().is_some_and(|(_, a)| a < 255));
    frames(&mut m, 10);
    assert_eq!(m.focus_border(), None);
}

#[test]
fn the_tool_keys_start_the_tools_set_in_the_settings() {
    let dir = TempDir::new("tools");
    let mut m = machine(&dir, "END\n");
    let storage = m.rt.storage.clone();
    storage.create_project("#TOOL").unwrap();
    let tool = "?PROJECT();ENVSTAT()\nWHILE INKEY$()!=\"Q\":VSYNC:WEND\n";
    storage
        .write_file_in("#TOOL", "TT.PRG", tool.as_bytes())
        .unwrap();
    m.rt.settings.tools = ["#TOOL/T.PRG", "#TOOL/NONE.PRG", ""].map(String::from);
    let before = rows(&m);
    // An empty setting does nothing, and a missing file only says so.
    assert!(!m.run_tool(2).unwrap());
    assert!(m.run_tool(1).is_err());
    assert!(!m.sub_running());
    assert_eq!(rows(&m), before);
    // From the editor too, where the main interpreter is editing.
    m.toggle_editor();
    assert!(m.run_tool(0).unwrap());
    assert!(m.sub_running() && m.sub_has_focus());
    frames(&mut m, 2);
    m.type_text("q");
    frames(&mut m, 3);
    assert!(!m.sub_running());
    assert!(m.is_editing());
    // A failed start leaves a running tool going.
    assert!(m.run_tool(0).unwrap());
    assert!(m.run_tool(1).is_err());
    assert!(m.sub_running());
}

#[test]
fn a_subprogram_types_into_the_editor_beside_it() {
    let dir = TempDir::new("editor");
    // As SmileBASIC 4's SMILETOOL types what was picked, on a key.
    let mut m = machine(
        &dir,
        "XSUBSCREEN 0,0,1280,720\n\
         WHILE INKEY$()!=\"Q\":VSYNC:WEND\n\
         IF ENVSTAT()==4 THEN M$=\"PRINT\" ELSE M$=\"?\"\n\
         ENVFOCUS 0\n\
         PUSHKEY M$\n\
         VSYNC 4\n\
         ENVFOCUS 1\n\
         LOOP:VSYNC:ENDLOOP\n",
    );
    m.set_slot(0, String::new());
    m.toggle_editor();
    enter_sub_from_editor(&mut m);
    // The keys go to the subprogram, not the editor, while it has the
    // focus.
    m.type_text("q");
    m.edit();
    frames(&mut m, 12);
    assert_eq!(m.editor().text(), "PRINT");
    assert!(m.sub_running() && m.sub_has_focus());
    assert!(!m.editor_has_keys());
    // Its screen shows over the editor.
    assert!(m.sub_screen().is_some());
}

/// Starts `SUB` as tool 1, with the editor open.
fn enter_sub_from_editor(m: &mut Machine) {
    m.rt.settings.tools[0] = "SUB".to_string();
    assert!(m.run_tool(0).unwrap());
    frames(m, 2);
}
