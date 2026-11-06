//! `--wav`: the sound a run mixed, written as a WAV file, run on a small
//! project in a temporary folder.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// A pulse-wave tune, which plays without the sound archive.
const SONG: &str = "BGMPLAY \"T150@216L8CDEFGAB<C\"\nWAIT 600\n";

/// A project folder `P` holding `MAIN`, in a fresh folder of its own.
fn project(test: &str, main: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "smilebasic-check-wav-{}-{test}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("P")).unwrap();
    std::fs::write(dir.join("P").join("TMAIN"), main).unwrap();
    dir
}

/// Runs the tool on `dir`'s project, kept from the user's data folder.
fn run(dir: &Path, args: &[&str], after: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_smilebasic-check"))
        .args(args)
        .arg(dir.join("P"))
        .arg("MAIN")
        .args(after)
        .env("XDG_DATA_HOME", dir)
        .env("APPDATA", dir)
        .env("HOME", dir)
        .output()
        .unwrap()
}

fn u16_at(b: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([b[at], b[at + 1]])
}

fn u32_at(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(b[at..at + 4].try_into().unwrap())
}

/// Checks the header of a 48 kHz 16-bit stereo file and gives its samples.
fn samples(bytes: &[u8]) -> Vec<i16> {
    assert_eq!(&bytes[0..4], b"RIFF");
    assert_eq!(u32_at(bytes, 4) as usize, bytes.len() - 8);
    assert_eq!(&bytes[8..16], b"WAVEfmt ");
    assert_eq!(u16_at(bytes, 20), 1, "PCM");
    assert_eq!(u16_at(bytes, 22), 2, "channels");
    assert_eq!(u32_at(bytes, 24), 48_000, "rate");
    assert_eq!(u32_at(bytes, 28), 48_000 * 4, "byte rate");
    assert_eq!(u16_at(bytes, 32), 4, "block");
    assert_eq!(u16_at(bytes, 34), 16, "bits");
    assert_eq!(&bytes[36..40], b"data");
    assert_eq!(u32_at(bytes, 40) as usize, bytes.len() - 44);
    (bytes[44..].chunks(2))
        .map(|c| i16::from_le_bytes([c[0], c[1]]))
        .collect()
}

#[test]
fn play_writes_the_frames_sound() {
    let dir = project("play", SONG);
    let (wav, shots) = (dir.join("song.wav"), dir.join("shots"));
    let args = ["-", shots.to_str().unwrap(), "--wav", wav.to_str().unwrap()];
    let out = run(&dir, &["--play", "90"], &args);
    assert!(out.status.success(), "{out:?}");
    let first = std::fs::read(&wav).unwrap();
    let s = samples(&first);
    // 800 stereo samples for each of the 90 frames at 60 a second.
    assert_eq!(s.len(), 90 * 800 * 2);
    assert!(s.iter().any(|&v| v != 0));
    let out = run(&dir, &["--play", "90"], &args);
    assert!(out.status.success(), "{out:?}");
    let second = std::fs::read(&wav).unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    assert!(first == second, "a second run wrote different bytes");
}

#[test]
fn bench_fixed_writes_the_same_sound() {
    let dir = project("bench", SONG);
    let (played, benched) = (dir.join("play.wav"), dir.join("bench.wav"));
    let shots = dir.join("shots");
    let out = run(
        &dir,
        &["--play", "--wav", played.to_str().unwrap(), "60"],
        &["-", shots.to_str().unwrap()],
    );
    assert!(out.status.success(), "{out:?}");
    let out = run(
        &dir,
        &[
            "--bench",
            "--fixed",
            "--wav",
            benched.to_str().unwrap(),
            "60",
        ],
        &[],
    );
    assert!(out.status.success(), "{out:?}");
    let (played, benched) = (
        std::fs::read(played).unwrap(),
        std::fs::read(benched).unwrap(),
    );
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(samples(&benched).len(), 60 * 800 * 2);
    assert!(played == benched);
}

#[test]
fn a_run_without_it_is_the_same() {
    let dir = project("without", SONG);
    let wav = dir.join("song.wav");
    let without = run(&dir, &["--bench", "--fixed", "60"], &[]);
    let with = run(
        &dir,
        &["--bench", "--fixed", "--wav", wav.to_str().unwrap(), "60"],
        &[],
    );
    let _ = std::fs::remove_dir_all(&dir);
    let checksum = |o: &Output| {
        let text = String::from_utf8(o.stdout.clone()).unwrap();
        (text.lines())
            .find(|l| l.starts_with("checksum "))
            .map(String::from)
    };
    assert!(checksum(&without).is_some(), "{without:?}");
    assert_eq!(checksum(&without), checksum(&with));
    assert_eq!(without.status.code(), Some(0));
    assert_eq!(with.status.code(), Some(0));
}

#[test]
fn an_error_leaves_a_whole_file() {
    let dir = project("error", "BGMPLAY \"@216CDE\"\nWAIT 10\nB=0\nA=1/B\n");
    let (wav, shots) = (dir.join("song.wav"), dir.join("shots"));
    let out = run(
        &dir,
        &["--play", "60"],
        &["-", shots.to_str().unwrap(), "--wav", wav.to_str().unwrap()],
    );
    let bytes = std::fs::read(&wav).unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(out.status.code(), Some(2));
    let s = samples(&bytes);
    assert!(s.len() < 60 * 800 * 2 && s.len().is_multiple_of(800 * 2));
    assert!(s.iter().any(|&v| v != 0));
}
