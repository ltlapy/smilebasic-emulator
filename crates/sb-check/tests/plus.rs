//! The + button and Ctrl+C in a `--play` script, `F plus N` and
//! `F ctrlc N`, run on small projects in a temporary folder.

use std::process::Command;

/// Runs `main` as project `P`'s MAIN for at most `frames` frames with
/// `script`, keeping the tool from the user's data folder. Gives the exit
/// status and what the tool printed.
fn play(test: &str, main: &str, script: &str, frames: u32) -> (i32, String) {
    let dir = std::env::temp_dir().join(format!(
        "smilebasic-check-plus-{}-{test}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("P")).unwrap();
    std::fs::write(dir.join("P").join("TMAIN"), main).unwrap();
    let script_file = dir.join("script.txt");
    std::fs::write(&script_file, script).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_smilebasic-check"))
        .arg("--play")
        .arg(frames.to_string())
        .arg(dir.join("P"))
        .arg("MAIN")
        .arg(&script_file)
        .arg(dir.join("shots"))
        .env("XDG_DATA_HOME", &dir)
        .env("APPDATA", &dir)
        .env("HOME", &dir)
        .output()
        .unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    let stdout = String::from_utf8(out.stdout).unwrap();
    (out.status.code().unwrap(), stdout)
}

/// The console's rows after `START`, as the tool printed them at the end.
fn after_start(stdout: &str) -> Vec<&str> {
    let rows: Vec<&str> = stdout
        .lines()
        .filter_map(|l| l.strip_prefix("  |"))
        .collect();
    let start = rows.iter().rposition(|&r| r == "START").expect(stdout);
    rows[start + 1..].to_vec()
}

const LOOP_WITH_BREAK: &str =
    "PRINT \"START\"\nON BREAK GOTO @B\n@L\nVSYNC\nGOTO @L\n@B\nPRINT \"BROKE\"\n";

#[test]
fn a_short_press_jumps_to_on_break() {
    let (status, out) = play("short", LOOP_WITH_BREAK, "10 plus 5\n", 100);
    assert_eq!(status, 0);
    // It acts when released, before frame 15 runs.
    assert!(out.starts_with("ended at frame 16\n"), "{out}");
    assert_eq!(after_start(&out), ["BROKE", "OK"]);
}

#[test]
fn a_short_press_counts_in_input_and_dialog() {
    let input = "PRINT \"START\"\nON BREAK GOTO @B\nINPUT A\nEND\n@B\nPRINT \"BROKE\"\n";
    let (_, out) = play("input", input, "10 plus 5\n", 100);
    assert_eq!(after_start(&out), ["? BROKE", "OK"]);
    let dialog = "PRINT \"START\"\nON BREAK GOTO @B\nDIALOG \"HI\"\nEND\n@B\nPRINT \"BROKE\"\n";
    let (_, out) = play("dialog", dialog, "10 plus 5\n", 100);
    assert_eq!(after_start(&out), ["BROKE", "OK"]);
}

#[test]
fn a_short_press_without_on_break_is_a_user_break() {
    let program = "PRINT \"START\"\n@L\nVSYNC\nGOTO @L\n";
    // 59 frames is still a short press.
    let (status, out) = play("no-break", program, "10 plus 59\n", 100);
    assert_eq!(status, 0);
    assert!(out.starts_with("ended at frame 70\n"), "{out}");
    assert_eq!(after_start(&out), ["User break on 0:4", "OK"]);
}

#[test]
fn a_one_second_hold_stops_past_on_break() {
    // The release at frame 130 would run PRG0 again from DIRECT mode,
    // printing START twice, if it still acted.
    let script = "10 plus 120\n150 end\n";
    let (status, out) = play("hold", LOOP_WITH_BREAK, script, 300);
    assert_eq!(status, 0);
    assert!(out.starts_with("ended at frame 150\n"), "{out}");
    assert_eq!(out.matches("|START\n").count(), 1, "{out}");
    assert_eq!(after_start(&out), ["User break on 0:5", "OK"]);
    // It stops at frame 70, one second into the hold.
    let (_, out) = play("hold-60", LOOP_WITH_BREAK, "10 plus 60\n", 300);
    assert!(out.starts_with("ended at frame 71\n"), "{out}");
}

#[test]
fn ctrl_c_jumps_to_on_break_at_the_press() {
    let (status, out) = play("ctrlc", LOOP_WITH_BREAK, "10 ctrlc 30\n", 100);
    assert_eq!(status, 0);
    assert!(out.starts_with("ended at frame 11\n"), "{out}");
    assert_eq!(after_start(&out), ["BROKE", "OK"]);
}

#[test]
fn ctrl_c_without_on_break_is_a_user_break() {
    let program = "PRINT \"START\"\n@L\nVSYNC\nGOTO @L\n";
    let (status, out) = play("ctrlc-stop", program, "10 ctrlc 5\n", 100);
    assert_eq!(status, 0);
    // It stops at the press, before frame 10 runs.
    assert!(out.starts_with("ended at frame 11\n"), "{out}");
    assert_eq!(after_start(&out), ["User break on 0:4", "OK"]);
}

#[test]
fn ctrl_c_types_nothing_and_hides_cs_press() {
    // Ctrl and C held for 80 frames: long enough for a key to repeat.
    let program = "PRINT \"START\"\nON BREAK GOTO @B\n@L\nVSYNC\nGOTO @L\n@B\n\
                   FOR I=1 TO 100\nK$=INKEY$()\nIF K$!=\"\" THEN PRINT \"KEY\";ASC(K$)\n\
                   IF KEYBOARD(6,2) THEN PRINT \"PRESS\"\n\
                   IF KEYBOARD(6,3) THEN PRINT \"RELEASE\"\nVSYNC\nNEXT\n";
    let (_, out) = play("ctrlc-keys", program, "10 ctrlc 80\n", 200);
    assert_eq!(after_start(&out), ["RELEASE", "OK"]);
}
