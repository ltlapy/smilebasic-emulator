//! Plays SmileBASIC 4's own instruments from `$SB_INSTRUMENTS`, a bank the
//! importer wrote from your own copy. Skipped when the variable is unset.
//! With `$SB_INSTRUMENTS_WAV` set, also writes what played there as a WAV
//! file to listen to.
//!
//! Run with `SB_INSTRUMENTS=/path/instruments.bin cargo test -p sb-runtime
//! --test instruments -- --nocapture`.

use std::sync::Arc;

use sb_core::vm::{Status, Vm};
use sb_runtime::Runtime;
use sb_runtime::font::Font;
use sb_runtime::sound::RATE;
use sb_runtime::sound::bank::Bank;
use sb_runtime::sound::synth::Instruments;

fn bank() -> Option<Bank> {
    let path = std::env::var_os("SB_INSTRUMENTS")?;
    let bytes = std::fs::read(&path).expect("SB_INSTRUMENTS is readable");
    Some(Bank::from_bytes(&bytes).expect("SB_INSTRUMENTS is a bank"))
}

/// Runs `source` and plays `frames` frames, left and right in turn.
fn play(bank: &Arc<Bank>, source: &str, frames: usize) -> Vec<f32> {
    let mut rt = Runtime::new(400, 240, Font::blank());
    rt.sound
        .set_instruments(Some(Instruments::Bank(bank.clone())));
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
    let bank = Arc::new(bank);
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
