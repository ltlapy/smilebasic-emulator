//! The exit status and stderr line `smilebasic-check` gives for how a
//! program ends, run on small projects in a temporary folder.

use std::path::{Path, PathBuf};
use std::process::Command;

/// A project folder `P` holding `MAIN`, in a fresh folder of its own.
fn project(test: &str, files: &[(&str, &str)]) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "smilebasic-check-test-{}-{test}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("P")).unwrap();
    for (name, text) in files {
        std::fs::write(dir.join("P").join(format!("T{name}")), text).unwrap();
    }
    dir
}

/// Runs the tool with `args` before the project, and `after` after it,
/// keeping it from the user's data folder. Gives the status and stderr.
fn run(dir: &Path, args: &[&str], after: &[&str]) -> (i32, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_smilebasic-check"))
        .args(args)
        .arg(dir.join("P"))
        .arg("MAIN")
        .args(after)
        .env("XDG_DATA_HOME", dir)
        .env("APPDATA", dir)
        .env("HOME", dir)
        .output()
        .unwrap();
    let _ = std::fs::remove_dir_all(dir);
    let stderr = String::from_utf8(out.stderr).unwrap();
    (out.status.code().unwrap(), stderr)
}

fn play(test: &str, main: &str) -> (i32, String) {
    let dir = project(test, &[("MAIN", main)]);
    let shots = dir.join("shots");
    run(&dir, &["--play", "30"], &["-", shots.to_str().unwrap()])
}

fn bench(test: &str, main: &str) -> (i32, String) {
    let dir = project(test, &[("MAIN", main)]);
    run(&dir, &["--bench", "30"], &[])
}

#[test]
fn an_error_is_2() {
    let program = "DIM A[3]\nPRINT 1\nA[4]=1\n";
    let expected = "error 31 at 0:3: Subscript out of range\n";
    assert_eq!(play("error", program), (2, expected.into()));
    // The frame it stops in once took the instructions run below zero.
    assert_eq!(bench("bench-error", program), (2, expected.into()));
}

#[test]
fn a_syntax_error_is_2() {
    let (status, stderr) = play("syntax", "PRINT 1\nFOR I=\n");
    assert_eq!(status, 2);
    assert!(
        stderr.starts_with("error 3 at 0:2: Syntax error"),
        "{stderr}"
    );
}

#[test]
fn an_error_in_an_exec_slot_is_2() {
    let program = "PRGEDIT 1\nPRGSET \"A=1/0\"\nEXEC 1\n";
    let expected = "error 7 at 1:1: Divide by zero\n";
    assert_eq!(play("exec", program), (2, expected.into()));
}

#[test]
fn an_error_in_a_subprogram_is_2() {
    let dir = project("sub", &[("MAIN", "END\n"), ("SUB", "VSYNC\nA=1/0\n")]);
    let script = dir.join("script.txt");
    std::fs::write(&script, "5 type SUBRUN \"SUB\"\n6 key ENTER\n").unwrap();
    let shots = dir.join("shots");
    let after = [script.to_str().unwrap(), shots.to_str().unwrap()];
    let expected = "subprogram error 7 at 0:2: Divide by zero\n";
    assert_eq!(run(&dir, &["--play", "60"], &after), (2, expected.into()));
}

#[test]
fn an_end_is_0() {
    assert_eq!(play("end", "PRINT 1\nEND\n"), (0, String::new()));
    assert_eq!(play("last-line", "PRINT 1\n"), (0, String::new()));
    assert_eq!(bench("bench-end", "PRINT 1\n"), (0, String::new()));
}

#[test]
fn the_frame_limit_is_0() {
    let program = "WHILE 1\nVSYNC\nWEND\n";
    assert_eq!(play("limit", program), (0, String::new()));
    assert_eq!(bench("bench-limit", program), (0, String::new()));
}

#[test]
fn stop_is_3() {
    let program = "PRINT 1\nSTOP\nPRINT 2\n";
    assert_eq!(play("stop", program), (3, "stop at 0:2\n".into()));
    assert_eq!(bench("bench-stop", program), (3, "stop at 0:2\n".into()));
}

#[test]
fn a_missing_program_is_1() {
    let dir = project("missing", &[]);
    let shots = dir.join("shots");
    let (status, _) = run(&dir, &["--play", "30"], &["-", shots.to_str().unwrap()]);
    assert_eq!(status, 1);
}
