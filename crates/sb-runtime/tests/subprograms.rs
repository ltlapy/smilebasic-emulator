//! SUBRUN's subprograms on the sub interpreter, beside the main one.

use std::path::PathBuf;

use sb_runtime::Runtime;
use sb_runtime::confirm::FileConfirm;
use sb_runtime::dialog::DialogAnswer;
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
    let sub = m.sub_screen().expect("shown");
    assert_eq!(sub.area, [10, 20, 300, 200]);
    assert_eq!((sub.frame.width(), sub.frame.height()), (400, 240));
    // ENVSAVE copied the subprogram's page 0 onto the main one's page 3.
    enter(&mut m, "gtarget 3:?hex$(gpget(0,0))");
    assert_eq!(last_rows(&m, 2)[0], "FFFF0000");
    enter(&mut m, "substop");
    frames(&mut m, 1);
    assert!(!m.sub_running());
    assert!(m.sub_screen().is_none());
}

#[test]
fn the_debugger_sees_the_subprograms_own_sprites() {
    let dir = TempDir::new("sprites");
    let mut m = machine(
        &dir,
        "XSUBSCREEN 10,20,300,200\n\
         SPSET 3,0:SPOFS 3,50,60\n\
         WHILE 1:VSYNC:WEND\n",
    );
    assert!(m.sub_runtime().is_none());
    enter(&mut m, "subrun \"sub\"");
    frames(&mut m, 2);
    let (rt, place) = m.sub_runtime().expect("running");
    // 400x240 at its aspect ratio in that rectangle.
    assert_eq!(place, Some([10, 30, 300, 180]));
    assert!(rt.sprites.list()[3].used);
    let corners = rt.sprite_corners(3).expect("set");
    assert_eq!(corners[0], (50.0, 60.0));
    // The main interpreter's sprites are its own.
    assert!(!m.rt.sprites.list()[3].used);
    m.stop_sub();
    frames(&mut m, 1);
    assert!(m.sub_runtime().is_none());
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
    // About 1.6 million instructions: 80 slices, or two frames for a
    // subprogram with no time limit of its own.
    let mut m = machine(&dir, "FOR I=1 TO 100000:NEXT\n");
    // With no time left in the frame, each side runs one slice a frame
    // whatever the computer's speed, so the count of frames tells.
    m.time_limit = Some(std::time::Duration::ZERO);
    enter(&mut m, "subrun \"sub\"");
    m.set_slot(0, "WHILE 1:WEND\n".to_string());
    m.run_slot(0);
    frames(&mut m, 10);
    assert!(m.sub_running() && m.is_running());
    for _ in 0..200 {
        if !m.sub_running() {
            break;
        }
        m.frame();
    }
    assert!(!m.sub_running() && m.is_running());
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
         WAIT 5:ENVFOCUS 1\n\
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

/// TEST62: a compile error leaves the status at direct mode's, and the rest
/// as an error that ran.
#[test]
fn envstat_tells_of_a_compile_error() {
    let dir = TempDir::new("errcompile");
    let mut m = machine(
        &dir,
        "ENVSTAT OUT S,N,A,SL,C,L\n\
         ENVFOCUS 0\n\
         PUSHKEY \"?\"+STR$(S)+\",\"+STR$(N)+\",\"+STR$(A)+\",\"+STR$(C)+\",\"+STR$(L)+CHR$(13)\n\
         LOOP:VSYNC:ENDLOOP\n",
    );
    m.set_slot(2, "?\"S\"\nDEF B\nRETURN A+1\nEND\n".to_string());
    enter(&mut m, "run 2");
    enter(&mut m, "subrun \"sub\"");
    frames(&mut m, 5);
    assert!(
        rows(&m).iter().any(|r| r == "0   3   -1  19  3"),
        "{:?}",
        rows(&m)
    );
}

/// TEST64: the subprogram's MAINCNT is the main interpreter's.
#[test]
fn a_subprogram_reads_the_main_frame_count() {
    let dir = TempDir::new("maincnt");
    let mut m = machine(
        &dir,
        "ENVFOCUS 0\n\
         PUSHKEY \"?\"+STR$(MAINCNT())+\"-MAINCNT()\"+CHR$(13)\n\
         LOOP:VSYNC:ENDLOOP\n",
    );
    frames(&mut m, 30);
    enter(&mut m, "subrun \"sub\"");
    frames(&mut m, 5);
    // The line runs a frame or two after the sub read the count.
    let rows = rows(&m);
    assert!(
        rows.iter().any(|r| matches!(r.trim(), "-1" | "-2" | "-3")),
        "{rows:?}"
    );
}

/// TEST64: at the start the error's argument is 0, and clean runs keep the
/// argument and line of the error before.
#[test]
fn envstat_keeps_the_argument_through_clean_runs() {
    let dir = TempDir::new("errclean");
    let tell = "ENVSTAT OUT S,N,A,SL,C,L\n\
         ENVFOCUS 0\n\
         PUSHKEY \"?\"+STR$(S)+\",\"+STR$(N)+\",\"+STR$(A)+\",\"+STR$(C)+\",\"+STR$(L)+CHR$(13)\n\
         LOOP:VSYNC:ENDLOOP\n";
    let told = |setup: &[&str], expected: &str| {
        let mut m = machine(&dir, tell);
        for program in setup {
            m.set_slot(2, program.to_string());
            enter(&mut m, "run 2");
        }
        enter(&mut m, "subrun \"sub\"");
        frames(&mut m, 5);
        assert!(rows(&m).iter().any(|r| r == expected), "{:?}", rows(&m));
    };
    told(&[], "0   0   0   -1  0");
    told(&["?\"A\"\n"], "0   0   0   -1  0");
    told(&["\n\nX=0:?1/X\n", "?\"A\"\n"], "0   0   -1  -1  3");
}

#[test]
fn envstat_tells_of_a_bare_goto() {
    let dir = TempDir::new("errsyntax");
    let mut m = machine(
        &dir,
        "ENVSTAT OUT S,N,A,SL,C,L\n\
         ENVFOCUS 0\n\
         PUSHKEY \"?\"+STR$(N)+\",\"+STR$(A)+\",\"+STR$(SL)+\",\"+STR$(L)+CHR$(13)\n\
         LOOP:VSYNC:ENDLOOP\n",
    );
    m.set_slot(2, "'\n?\"A\"\nGOTO\n".to_string());
    enter(&mut m, "run 2");
    assert!(
        rows(&m)
            .iter()
            .any(|r| r.starts_with("Type mismatch in 2:3"))
    );
    enter(&mut m, "subrun \"sub\"");
    frames(&mut m, 5);
    // Type mismatch is 8, in slot 2's third line.
    assert!(
        rows(&m).iter().any(|r| r == "8   -1  2   3"),
        "{:?}",
        rows(&m)
    );
}

#[test]
fn envstat_tells_which_argument_was_wrong() {
    for (program, expected) in [
        // As on hardware: Out of range (10), Y counted from 0.
        ("LOCATE 0,999", "10  1"),
        ("BGMPLAY 0,1,200", "10  2"),
        ("X=0:?1/X", "7   -1"),
    ] {
        let dir = TempDir::new("errarg");
        let mut m = machine(
            &dir,
            "ENVSTAT OUT S,N,A,SL,C,L\n\
             ENVFOCUS 0\n\
             PUSHKEY \"?\"+STR$(N)+\",\"+STR$(A)+CHR$(13)\n\
             LOOP:VSYNC:ENDLOOP\n",
        );
        m.set_slot(0, format!("'\n{program}\n"));
        m.run_slot(0);
        frames(&mut m, 2);
        enter(&mut m, "subrun \"sub\"");
        frames(&mut m, 5);
        assert!(
            rows(&m).iter().any(|r| r == expected),
            "{program}: {:?}",
            rows(&m)
        );
    }
}

#[test]
fn the_focus_moves_a_few_frames_after_envfocus() {
    let dir = TempDir::new("delay");
    let mut m = machine(
        &dir,
        "VSYNC:ENVFOCUS 0:N=0\n\
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
    let mut m = machine(&dir, "XSUBSCREEN 10,20,300,200\nWAIT 30\n");
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
    // A running tool stops with User break, even when the next cannot
    // start.
    assert!(m.run_tool(0).unwrap());
    frames(&mut m, 2);
    assert!(m.run_tool(1).is_err());
    frames(&mut m, 2);
    assert!(!m.sub_running());
    assert!(
        rows(&m).iter().any(|r| r.starts_with("User break on 0:2")),
        "{:?}",
        rows(&m)
    );
}

#[test]
fn every_form_of_a_subprograms_name_finds_its_file() {
    let dir = TempDir::new("forms");
    let mut m = machine(&dir, "END\n");
    let storage = m.rt.storage.clone();
    storage.create_project("GAME").unwrap();
    storage.create_project("#TOOL").unwrap();
    let source = "WHILE 1:VSYNC:WEND\n";
    storage
        .write_file_in("", "TBARE", source.as_bytes())
        .unwrap();
    storage
        .write_file_in("", "T@KEPT.PRG", source.as_bytes())
        .unwrap();
    storage
        .write_file_in("GAME", "TIN.PRG", source.as_bytes())
        .unwrap();
    storage
        .write_file_in("#TOOL", "TT.PRG", source.as_bytes())
        .unwrap();
    for (form, project) in [
        ("BARE", ""),
        ("bare", ""),
        ("/BARE", ""),
        ("@KEPT.PRG", ""),
        ("/@KEPT.PRG", ""),
        ("TXT:BARE", ""),
        ("GAME/IN.PRG", "GAME"),
        ("game/in.prg", "GAME"),
        ("TXT:GAME/IN.PRG", "GAME"),
        ("#TOOL/T.PRG", "#TOOL"),
    ] {
        m.rt.settings.tools[0] = form.to_string();
        assert!(m.run_tool(0).unwrap(), "{form}");
        let (sub, _) = m.sub_runtime().unwrap();
        assert_eq!(sub.storage.project(), project, "{form}");
        assert_eq!(sub.slots[0], source, "{form}");
        m.stop_sub();
        frames(&mut m, 1);
    }
    // A bare name is in the project the main interpreter is in.
    m.rt.storage.set_project("GAME").unwrap();
    m.rt.settings.tools[0] = "IN.PRG".to_string();
    assert!(m.run_tool(0).unwrap());
    assert_eq!(m.sub_runtime().unwrap().0.storage.project(), "GAME");
}

#[test]
fn a_subprograms_name_cannot_leave_the_storage_folder() {
    let outer = TempDir::new("traversal");
    let dir = TempDir(outer.0.join("root"));
    let mut m = machine(&dir, "END\n");
    // What the names below would reach if they went through.
    std::fs::create_dir_all(dir.0.join("@WORKSPACE/TA")).unwrap();
    std::fs::create_dir_all(dir.0.join("GAME/TA")).unwrap();
    std::fs::write(outer.0.join("secret"), "WHILE 1:VSYNC:WEND\n").unwrap();
    for bad in [
        "/A/../../../secret",
        "A/../../../secret",
        "GAME/A/../../../secret",
        "../secret",
        "/../secret",
        "/:\\x",
        ":\\x",
        "\\..\\x",
        "DAT:BARE",
    ] {
        m.rt.settings.tools[0] = bad.to_string();
        assert!(m.run_tool(0).is_err(), "{bad}");
        assert!(!m.sub_running(), "{bad}");
    }
    enter(&mut m, "subrun \"/A/../../../secret\"");
    frames(&mut m, 1);
    let rows_now = last_rows(&m, 4);
    assert_eq!(rows_now[0], "OK");
    assert_eq!(rows_now[1], "=== From subinterpreter ===");
    assert!(rows_now[2].starts_with("Illegal format"), "{rows_now:?}");
    assert_eq!(rows_now[3], "=".repeat(27));
    assert!(!m.sub_running());
    // So is a file of a type it cannot run (TEST57).
    enter(&mut m, "subrun \"DAT:BARE\"");
    frames(&mut m, 1);
    let rows_now = last_rows(&m, 4);
    assert_eq!(rows_now[1], "=== From subinterpreter ===");
    assert!(rows_now[2].starts_with("Illegal format"), "{rows_now:?}");
    assert!(!m.sub_running());
    // The keys stayed with the main interpreter.
    enter(&mut m, "?\"here\"");
    assert!(rows(&m).iter().any(|r| r == "HERE"), "{:?}", rows(&m));
}

#[test]
fn recovering_from_a_panic_ends_a_subprogram_too() {
    let dir = TempDir::new("panic");
    let mut m = machine(&dir, "WHILE 1:VSYNC:WEND\n");
    enter(&mut m, "subrun \"sub\"");
    frames(&mut m, 2);
    assert!(m.sub_running() && m.sub_has_focus());
    m.recover_from_panic("a bug");
    assert!(!m.sub_running() && !m.sub_has_focus());
    frames(&mut m, 2);
    enter(&mut m, "?\"here\"");
    assert!(rows(&m).iter().any(|r| r == "HERE"), "{:?}", rows(&m));
    enter(&mut m, "subrun \"sub\"");
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

/// A subprogram that types the main interpreter's ENVSTAT and ENVLOCATE
/// where the keys go.
const TELL_STATUS: &str = "ENVSTAT OUT S,N,A,SL,C,L\n\
     ENVLOCATE OUT X,Y\n\
     ENVFOCUS 0\n\
     VSYNC 4\n\
     PUSHKEY STR$(S)+\",\"+STR$(N)+\",\"+STR$(A)+\",\"+STR$(SL)+\",\"+STR$(C)+\",\"+STR$(L)+\" \"+STR$(X)+\",\"+STR$(Y)\n\
     LOOP:VSYNC:ENDLOOP\n";

#[test]
fn an_error_in_direct_mode_leaves_the_main_interpreter_suspended() {
    let dir = TempDir::new("direrr");
    let mut m = machine(&dir, TELL_STATUS);
    m.set_slot(0, "?\"A\"\nGOTO\n".to_string());
    enter(&mut m, "run");
    enter(&mut m, "?1/0");
    enter(&mut m, "subrun \"sub\"");
    frames(&mut m, 12);
    m.rt.key_press(keys::ENTER);
    frames(&mut m, 2);
    assert!(
        rows(&m).iter().any(|r| r.starts_with("1,0,-1,-1,-1,2 ")),
        "{:?}",
        rows(&m)
    );
}

#[test]
fn the_editor_hides_the_last_error_and_the_cursor_from_subprograms() {
    let dir = TempDir::new("editing");
    let mut m = machine(&dir, TELL_STATUS);
    m.set_slot(0, "?1/0\n".to_string());
    enter(&mut m, "run");
    m.set_slot(0, String::new());
    m.toggle_editor();
    enter_sub_from_editor(&mut m);
    m.edit();
    frames(&mut m, 12);
    assert_eq!(m.editor().text(), "4,0,0,0,0,0 0,0");
}

/// Each interpreter has all of memory to itself, as FREEMEM counts it.
#[test]
fn the_sub_interpreter_has_a_memory_budget_of_its_own() {
    let dir = TempDir::new("budget");
    let mut m = machine(&dir, "DIM A%[30000000]:WHILE 1:VSYNC:WEND\n");
    enter(&mut m, "dim a%[30000000]");
    enter(&mut m, "subrun \"sub\"");
    frames(&mut m, 5);
    assert!(m.sub_running(), "{:?}", rows(&m));
}

/// Both forms give no error on hardware, and start the file here.
#[test]
fn subrun_takes_lower_case_and_a_file_type() {
    let dir = TempDir::new("cases");
    let mut m = machine(&dir, "WHILE 1:VSYNC:WEND\n");
    for form in ["sub", "TXT:SUB", "txt:sub"] {
        enter(&mut m, &format!("subrun \"{form}\""));
        frames(&mut m, 1);
        assert!(m.sub_running(), "{form}");
        assert!(
            !rows(&m).iter().any(|r| r.starts_with("=== From")),
            "{form}"
        );
        m.stop_sub();
        frames(&mut m, 1);
        m.rt.text[4].cls();
    }
}

/// The sub interpreter's console, as [`rows`] gives the main one's.
fn sub_rows(m: &Machine) -> Vec<String> {
    let console = &m.sub_runtime().expect("running").0.text[4];
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

const THEN_WAIT: &str = "WHILE 1:VSYNC:WEND\n";

#[test]
fn a_subprograms_dialog_reaches_the_frontend_and_its_answer_the_subprogram() {
    let dir = TempDir::new("sub-dialog");
    let source = format!("DIALOG \"ASKED\",1,\"CAP\"\n?RESULT()\n{THEN_WAIT}");
    let mut m = machine(&dir, &source);
    enter(&mut m, "subrun \"sub\"");
    frames(&mut m, 3);
    let shown = m.program_dialog().expect("the subprogram's dialog");
    assert_eq!(shown.caption, "CAP");
    assert!(m.rt.program_dialog().is_none(), "not the main one's");
    m.answer_dialog(DialogAnswer::Choice(0));
    frames(&mut m, 2);
    assert!(m.program_dialog().is_none());
    assert_eq!(sub_rows(&m), ["-1"]);
}

#[test]
fn the_controls_answer_a_subprograms_dialog_without_the_focus() {
    let dir = TempDir::new("sub-dialog-pad");
    let source = format!("ENVFOCUS 0\nVSYNC 5\nDIALOG \"ASKED\",1\n?RESULT()\n{THEN_WAIT}");
    let mut m = machine(&dir, &source);
    enter(&mut m, "subrun \"sub\"");
    frames(&mut m, 10);
    assert!(!m.sub_has_focus() && m.program_dialog().is_some());
    let before = rows(&m);
    let a = 1 << sb_runtime::input::A;
    for buttons in [a, a, 0] {
        m.rt.input.live.pads[0].buttons = buttons;
        m.frame();
        // TEST64: the main interpreter, with the focus, sees it too.
        assert_eq!(m.rt.input.live.pads[0].buttons, buttons);
    }
    frames(&mut m, 1);
    assert!(m.program_dialog().is_none());
    assert_eq!(sub_rows(&m), ["1"]);
    // But it types no Enter there (TEST64: no key for INKEY$).
    assert_eq!(rows(&m), before);
}

/// TEST64: with a dialog on each side, one press answers both, in the same
/// frame.
#[test]
fn one_press_answers_the_dialogs_of_both_sides() {
    let dir = TempDir::new("both-dialogs");
    let ask = "DIALOG \"ASKED\",1\n?RESULT();\" \";MAINCNT()\n";
    let mut m = machine(&dir, &format!("{ask}{THEN_WAIT}"));
    enter(&mut m, "subrun \"sub\"");
    m.set_slot(0, format!("{ask}{THEN_WAIT}"));
    m.run_slot(0);
    frames(&mut m, 3);
    assert!(m.rt.program_dialog().is_some());
    assert!(
        m.sub_runtime()
            .expect("running")
            .0
            .program_dialog()
            .is_some()
    );
    let a = 1 << sb_runtime::input::A;
    for buttons in [a, a, 0] {
        m.rt.input.live.pads[0].buttons = buttons;
        m.frame();
    }
    frames(&mut m, 1);
    let main = rows(&m).into_iter().find(|r| r.starts_with("1 "));
    let sub = sub_rows(&m).into_iter().find(|r| r.starts_with("1 "));
    assert!(main.is_some() && main == sub, "{main:?} {sub:?}");
}

#[test]
fn the_controls_answer_the_main_dialog_while_a_subprogram_has_the_focus() {
    let dir = TempDir::new("main-dialog-pad");
    let mut m = machine(&dir, THEN_WAIT);
    enter(&mut m, "subrun \"sub\"");
    assert!(m.sub_has_focus());
    m.set_slot(0, format!("DIALOG \"ASKED\",1\n?RESULT()\n{THEN_WAIT}"));
    m.run_slot(0);
    frames(&mut m, 2);
    assert!(m.program_dialog().is_some() && m.rt.program_dialog().is_some());
    let a = 1 << sb_runtime::input::A;
    for buttons in [a, a, 0] {
        m.rt.input.live.pads[0].buttons = buttons;
        m.frame();
        let sub = m.sub_runtime().expect("running").0;
        assert_eq!(sub.input.live.pads[0].buttons, 0, "not for the subprogram");
    }
    frames(&mut m, 1);
    assert!(m.program_dialog().is_none());
    assert!(rows(&m).iter().any(|r| r == "1"), "{:?}", rows(&m));
    assert!(m.sub_has_focus(), "the focus stays where it was");
    // Past the dialog, the subprogram has the controls again.
    m.rt.input.live.pads[0].buttons = a;
    m.frame();
    assert_eq!(m.rt.input.live.pads[0].buttons, 0);
    let sub = m.sub_runtime().expect("running").0;
    assert_eq!(sub.input.live.pads[0].buttons, a);
}

#[test]
fn a_subprograms_question_reaches_the_frontend_and_its_answer_the_subprogram() {
    let dir = TempDir::new("sub-confirm");
    let source = format!("SAVEV \"TXT:Q\",\"HELLO\"\n?RESULT()\n{THEN_WAIT}");
    let mut m = machine(&dir, &source);
    m.rt.settings.file_confirm = FileConfirm::Ask;
    enter(&mut m, "subrun \"sub\"");
    frames(&mut m, 3);
    assert!(m.confirmation().is_some() && m.rt.confirmation().is_none());
    m.answer(false);
    frames(&mut m, 2);
    assert!(m.confirmation().is_none());
    assert_eq!(sub_rows(&m), ["-1"]);

    m.stop_sub();
    enter(&mut m, "subrun \"sub\"");
    frames(&mut m, 3);
    assert!(m.confirmation().is_some());
    m.answer(true);
    frames(&mut m, 2);
    assert_eq!(sub_rows(&m).last().map(String::as_str), Some("1"));
}

#[test]
fn with_both_interpreters_asking_each_gets_its_own_answer() {
    let dir = TempDir::new("both-ask");
    let source = format!("DIALOG \"SUB\",1\n?RESULT()\n{THEN_WAIT}");
    let mut m = machine(&dir, &source);
    enter(&mut m, "subrun \"sub\"");
    m.set_slot(0, format!("DIALOG \"MAIN\",1\n?RESULT()\n{THEN_WAIT}"));
    m.run_slot(0);
    frames(&mut m, 2);
    assert_eq!(m.program_dialog().unwrap().lines, ["SUB"]);
    m.answer_dialog(DialogAnswer::Choice(0));
    frames(&mut m, 2);
    assert_eq!(sub_rows(&m), ["-1"]);
    assert_eq!(m.program_dialog().unwrap().lines, ["MAIN"]);
    m.answer_dialog(DialogAnswer::Choice(1));
    frames(&mut m, 2);
    assert!(m.program_dialog().is_none());
    assert!(rows(&m).iter().any(|r| r == "1"));
    assert_eq!(sub_rows(&m), ["-1"]);
}

#[test]
fn stopping_a_subprogram_takes_its_dialog_and_question_with_it() {
    let dir = TempDir::new("sub-dialog-stop");
    let mut m = machine(&dir, "DIALOG \"ASKED\",1\n");
    enter(&mut m, "subrun \"sub\"");
    frames(&mut m, 2);
    assert!(m.program_dialog().is_some());
    m.stop_sub();
    assert!(m.program_dialog().is_none());
}

#[test]
fn a_subprograms_dialog_is_answered_with_the_editor_open() {
    let dir = TempDir::new("sub-dialog-editor");
    let source = format!(
        "XSUBSCREEN 0,0,100,100\nENVFOCUS 0\nVSYNC 5\nDIALOG \"ASKED\",1\n?RESULT()\n{THEN_WAIT}"
    );
    let mut m = machine(&dir, &source);
    m.set_slot(0, "PRINT".to_string());
    m.toggle_editor();
    enter_sub_from_editor(&mut m);
    frames(&mut m, 10);
    assert!(m.is_editing() && !m.sub_has_focus());
    assert!(m.program_dialog().is_some());
    assert!(m.sub_screen().is_some(), "it still shows over the editor");
    m.answer_dialog(DialogAnswer::Choice(1));
    frames(&mut m, 2);
    assert!(m.program_dialog().is_none());
    assert_eq!(sub_rows(&m), ["1"]);
    assert_eq!(m.editor().text(), "PRINT");
}

#[test]
fn input_in_a_subprogram_takes_the_keys_with_the_focus() {
    let dir = TempDir::new("sub-input");
    let mut m = machine(&dir, &format!("INPUT A$\n?\"GOT \";A$\n{THEN_WAIT}"));
    enter(&mut m, "subrun \"sub\"");
    frames(&mut m, 2);
    m.type_text("hi");
    m.rt.key_press(keys::ENTER);
    frames(&mut m, 3);
    let got = sub_rows(&m);
    assert!(got.iter().any(|r| r == "GOT HI"), "{got:?}");
}

#[test]
fn a_subprograms_notices_reach_the_frontend() {
    let dir = TempDir::new("sub-notice");
    let mut m = machine(&dir, &format!("SAVEV \"TXT:Q\",\"HELLO\"\n{THEN_WAIT}"));
    m.rt.settings.file_confirm = FileConfirm::Notify;
    enter(&mut m, "subrun \"sub\"");
    frames(&mut m, 2);
    let notices = m.take_notices();
    assert!(notices.iter().any(|n| n.contains("TXT:Q")), "{notices:?}");
    assert!(m.take_notices().is_empty());
}

/// A subprogram's LOADG takes a name with a project, unlike a main
/// program's, but its EXEC refuses one as a main program's does.
#[test]
fn a_subprograms_exec_refuses_a_project_name_and_its_loadg_does_not() {
    let dir = TempDir::new("sub-project-names");
    let source = format!("LOADG \"GRP:GAME/PIC\",0:?RESULT()\n{THEN_WAIT}");
    let mut m = machine(&dir, &source);
    let storage = m.rt.storage.clone();
    storage.create_project("GAME").unwrap();
    storage
        .write_file_in("GAME", "TIN.PRG", THEN_WAIT.as_bytes())
        .unwrap();
    enter(
        &mut m,
        "project \"game\":saveg \"pic\",0,0,0,4,4:project \"\"",
    );
    enter(&mut m, "subrun \"sub\"");
    frames(&mut m, 3);
    assert_eq!(sub_rows(&m), ["1"]);
    m.stop_sub();
    frames(&mut m, 1);

    m.set_slot(1, "EXEC \"GAME/IN.PRG\",2\n".to_string());
    enter(&mut m, "save \"sub\",1");
    enter(&mut m, "subrun \"sub\"");
    frames(&mut m, 3);
    let rule = "=".repeat(27);
    let block = [
        "=== From subinterpreter ===",
        "Can't access other project in this mode in 0:1",
        rule.as_str(),
    ];
    assert_eq!(last_rows(&m, 3), block);
}
