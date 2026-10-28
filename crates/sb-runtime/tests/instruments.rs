//! Plays SmileBASIC 4's own instruments from `$SB_INSTRUMENTS`, a bank the
//! importer wrote from your own copy. Skipped when the variable is unset.
//! With `$SB_INSTRUMENTS_WAV` set, also writes what played there as a WAV
//! file to listen to.
//!
//! The preset songs and effects play too, from `sequences.bin` next to
//! the bank; `$SB_PRESETS_WAV` writes them all to a WAV file.
//!
//! `$SB_SOUNDFONT` names a SoundFont the importer wrote with `--sf2`, to
//! play the same way.
//!
//! Run with `SB_INSTRUMENTS=/path/instruments.bin cargo test -p sb-runtime
//! --test instruments -- --nocapture`.

use std::sync::Arc;

use sb_core::vm::{Status, Vm};
use sb_runtime::Runtime;
use sb_runtime::font::Font;
use sb_runtime::sound::bank::Bank;
use sb_runtime::sound::seq::Sequences;
use sb_runtime::sound::synth::Instruments;
use sb_runtime::sound::{RATE, SoundFont};

fn bank() -> Option<Bank> {
    let path = std::env::var_os("SB_INSTRUMENTS")?;
    let bytes = std::fs::read(&path).expect("SB_INSTRUMENTS is readable");
    Some(Bank::from_bytes(&bytes).expect("SB_INSTRUMENTS is a bank"))
}

/// Runs `source` and plays `frames` frames, left and right in turn.
fn play(instruments: &Instruments, source: &str, frames: usize) -> Vec<f32> {
    play_with(instruments, None, source, frames)
}

fn play_with(
    instruments: &Instruments,
    sequences: Option<&Arc<Sequences>>,
    source: &str,
    frames: usize,
) -> Vec<f32> {
    let mut rt = Runtime::new(400, 240, Font::blank());
    rt.sound.set_instruments(Some(instruments.clone()));
    rt.sound.set_sequences(sequences.cloned());
    let mut vm = Vm::new(sb_core::compile(source, &rt).unwrap());
    while vm.run(&mut rt, 1_000_000).unwrap() != Status::Ended {}
    let mut samples = Vec::new();
    for _ in 0..frames {
        rt.sound.render_frame();
        samples.extend(rt.sound.take_samples());
    }
    samples
}

/// The pitch of the left channel: the first lag at which it nearly
/// repeats itself, between 50 and 2000 Hz.
fn pitch(samples: &[f32]) -> f64 {
    let left: Vec<f32> = samples.iter().step_by(2).take(8192).copied().collect();
    let n = left.len() / 2;
    let corr = |lag: usize| -> f32 { (0..n).map(|i| left[i] * left[i + lag]).sum() };
    let lags = (RATE as usize / 2000)..(RATE as usize / 50);
    let best = lags.clone().map(corr).fold(f32::MIN, f32::max);
    let lag = lags
        .into_iter()
        .find(|&l| corr(l) > 0.9 * best)
        .unwrap_or(1);
    f64::from(RATE) / lag as f64
}

fn write_wav(path: &std::ffi::OsStr, samples: &[f32]) {
    let mut out = Vec::new();
    let data = samples.len() as u32 * 2;
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    for v in [16u32, 0x0002_0001, RATE, RATE * 4, 0x0010_0004] {
        out.extend_from_slice(&v.to_le_bytes());
    }
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data.to_le_bytes());
    for s in samples {
        out.extend_from_slice(&((s * 32767.0) as i16).to_le_bytes());
    }
    std::fs::write(path, out).unwrap();
}

#[test]
fn instruments_play() {
    let Some(bank) = bank() else {
        eprintln!("SB_INSTRUMENTS is not set; skipped");
        return;
    };
    let bank = Instruments::Bank(Arc::new(bank));
    // A held A4 on a few instruments: piano, organ, strings.
    for program in [0, 19, 48] {
        let samples = play(&bank, &format!("BGMPLAY \"T60 @{program} O4 A1\""), 60);
        let peak = samples.iter().fold(0f32, |m, s| m.max(s.abs()));
        let hz = pitch(&samples[samples.len() / 2..]);
        eprintln!("@{program}: peak {peak:.3}, about {hz:.0} Hz");
        assert!(peak > 0.01, "@{program} is silent");
        assert!((400.0..490.0).contains(&hz), "@{program} is out of tune");
    }
    // A drum set and a BEEP of an instrument.
    let drums = play(&bank, "BGMPLAY \"T120 @128 O2 C4D4\"", 60);
    assert!(drums.iter().any(|s| s.abs() > 0.01), "drums are silent");
    let beep = play(&bank, "BEEP 256", 30);
    assert!(beep.iter().any(|s| s.abs() > 0.01), "BEEP 256 is silent");

    // The worst case: every channel holding a three-note chord, which the
    // voice cap trims. Ten seconds should take a small part of that.
    let busy: String = (0..16)
        .map(|c| format!(":{c} @{} O4 L1 [|CEG|]", c * 8))
        .collect();
    let start = std::time::Instant::now();
    play(&bank, &format!("BGMPLAY \"T60 {busy}\""), 600);
    eprintln!("ten busy seconds took {:?}", start.elapsed());

    if let Some(path) = std::env::var_os("SB_INSTRUMENTS_WAV") {
        let song = "BGMPLAY \"T120 @0 O4 L8 CDEFGAB<C2 @48 >CEG<C2 @128 O2 L8 CCDCCCDC @19 O4 C1\"";
        write_wav(&path, &play(&bank, song, 60 * 12));
    }
}

#[test]
fn the_soundfont_plays() {
    let Some(path) = std::env::var_os("SB_SOUNDFONT") else {
        eprintln!("SB_SOUNDFONT is not set; skipped");
        return;
    };
    let mut file = std::fs::File::open(path).expect("SB_SOUNDFONT is readable");
    let font = SoundFont::new(&mut file).expect("SB_SOUNDFONT is a SoundFont");
    let font = Instruments::Font(Arc::new(font));
    let samples = play(&font, "BGMPLAY \"T60 @0 O4 A1\"", 60);
    let hz = pitch(&samples[samples.len() / 2..]);
    eprintln!("SoundFont @0: about {hz:.0} Hz");
    assert!((400.0..490.0).contains(&hz), "out of tune");
    let drums = play(&font, "BGMPLAY \"T120 @128 O2 C4D4\"", 60);
    assert!(drums.iter().any(|s| s.abs() > 0.01), "drums are silent");
}

#[test]
fn presets_play() {
    let Some(path) = std::env::var_os("SB_INSTRUMENTS") else {
        eprintln!("SB_INSTRUMENTS is not set; skipped");
        return;
    };
    let bank = Instruments::Bank(Arc::new(bank().unwrap()));
    let dir = std::path::Path::new(&path).parent().unwrap();
    let bytes = std::fs::read(dir.join("sequences.bin")).expect("sequences.bin is there");
    let seqs = Arc::new(Sequences::from_bytes(&bytes).expect("sequences.bin is whole"));
    let peak = |s: &[f32]| s.iter().fold(0f32, |m, x| m.max(x.abs()));
    let mut all = Vec::new();
    let start = std::time::Instant::now();
    let mut quiet = Vec::new();
    for n in 0..=45 {
        let samples = play_with(&bank, Some(&seqs), &format!("BGMPLAY {n}"), 60 * 8);
        if peak(&samples) < 0.01 {
            quiet.push(format!("BGM {n}"));
        }
        all.extend(samples);
    }
    eprintln!("46 songs, 8 seconds each, took {:?}", start.elapsed());
    for n in (0..=155).chain(256..=383) {
        let samples = play_with(&bank, Some(&seqs), &format!("BEEP {n}"), 60);
        // Some effects are meant to be quiet.
        if peak(&samples) < 0.001 {
            quiet.push(format!("BEEP {n}"));
        }
        if n < 156 {
            all.extend(samples);
        }
    }
    eprintln!("silent: {quiet:?}");
    if let Some(path) = std::env::var_os("SB_PRESETS_WAV") {
        write_wav(&path, &all);
    }
    assert!(quiet.is_empty(), "silent presets: {quiet:?}");
}
