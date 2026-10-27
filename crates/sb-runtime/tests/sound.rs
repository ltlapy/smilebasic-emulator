//! BGM commands, played frame by frame.

use std::sync::Arc;

use sb_core::error::ErrorKind;
use sb_core::vm::{Status, Vm};
use sb_runtime::Runtime;
use sb_runtime::font::Font;
use sb_runtime::machine::Machine;
use sb_runtime::sound::SoundFont;

fn run_on(rt: &mut Runtime, source: &str) -> Result<String, sb_core::error::Error> {
    let mut vm = Vm::new(sb_core::compile(source, &*rt)?);
    while vm.run(rt, 1_000_000)? != Status::Ended {}
    let console = &rt.text[4];
    let codes: Vec<u16> = (0..console.size().0)
        .map(|x| console.cell(x, 0).code)
        .take_while(|&c| c != 0)
        .collect();
    rt.text[4].cls();
    Ok(String::from_utf16(&codes).unwrap())
}

fn runtime() -> Runtime {
    Runtime::new(400, 240, Font::blank())
}

fn frames(rt: &mut Runtime, n: usize) {
    for _ in 0..n {
        rt.sound.render_frame();
    }
}

#[test]
fn songs_play_for_their_length() {
    let mut rt = runtime();
    // A quarter note at 120 a minute lasts half a second: 30 frames.
    run_on(&mut rt, "BGMPLAY \"T120C4\"").unwrap();
    frames(&mut rt, 29);
    assert_eq!(run_on(&mut rt, "BGMCHK OUT A:?A").unwrap(), "1");
    frames(&mut rt, 2);
    assert_eq!(run_on(&mut rt, "BGMCHK OUT A:?A").unwrap(), "0");
}

#[test]
fn tracks_repeat_stop_and_pause() {
    let mut rt = runtime();
    run_on(&mut rt, "BGMSET 130,\"[C]\":BGMPLAY 3,130:BGMPLAY 2,0").unwrap();
    frames(&mut rt, 200);
    assert_eq!(
        run_on(&mut rt, "?BGMCHK(3);BGMCHK(2);BGMCHK(0)").unwrap(),
        "110"
    );
    run_on(&mut rt, "BGMPAUSE 3:BGMSTOP 2,0.5").unwrap();
    frames(&mut rt, 31);
    assert_eq!(run_on(&mut rt, "?BGMCHK(3);BGMCHK(2)").unwrap(), "10");
    run_on(&mut rt, "BGMCONT:BGMSTOP").unwrap();
    assert_eq!(run_on(&mut rt, "?BGMCHK(3)").unwrap(), "0");
}

#[test]
fn variables_and_data() {
    let mut m = Machine::new(runtime());
    m.boot();
    m.set_slot(
        0,
        "BGMSETD 128,\"@M\":BGMPLAY 1,128:BGMVAR 1,2,7:BGMVAR 1,2 OUT A:VSYNC:BGMVAR 1,2 OUT B:?A;B\n\
         @M:DATA \"$2=1 :0 C\",\"D\",0"
            .to_string(),
    );
    m.run_slot(0);
    for _ in 0..5 {
        m.frame();
    }
    assert!(!m.is_running());
    // The song set $2 to 1 on its first tick.
    let console = &m.rt.text[4];
    let rows: Vec<String> = (0..console.size().1)
        .map(|y| {
            let codes: Vec<u16> = (0..console.size().0)
                .map(|x| console.cell(x, y).code)
                .take_while(|&c| c != 0)
                .collect();
            String::from_utf16(&codes).unwrap()
        })
        .collect();
    assert!(rows.iter().any(|r| r == "71"), "{rows:?}");
}

#[test]
fn checking_mml() {
    let mut rt = runtime();
    assert_eq!(
        run_on(&mut rt, "CHKMML \"CDE\" OUT A:CHKMML \"CDX\" OUT B:?A;B").unwrap(),
        "-12"
    );
    assert_eq!(
        run_on(&mut rt, "BGMSET 128,\"C5\"").unwrap_err().kind,
        ErrorKind::IllegalMml
    );
    assert_eq!(
        run_on(&mut rt, "BGMPLAY 46").unwrap_err().kind,
        ErrorKind::OutOfRange
    );
    assert_eq!(
        run_on(&mut rt, "BGMSET 127,\"C\"").unwrap_err().kind,
        ErrorKind::OutOfRange
    );
}

#[test]
fn master_volume() {
    let mut rt = runtime();
    assert_eq!(
        run_on(&mut rt, "SNDMVOL 64:SNDMVOL OUT V:?V").unwrap(),
        "64"
    );
    run_on(&mut rt, "SNDMVOL 0,1").unwrap();
    frames(&mut rt, 30);
    assert_eq!(run_on(&mut rt, "SNDMVOL OUT V:?V").unwrap(), "32");
}

#[test]
fn a_soundfont_makes_sound() {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../sb-sdl/sounds/GeneralUser-GS.sf2"
    );
    let font = SoundFont::new(&mut std::fs::File::open(path).unwrap()).unwrap();
    let mut rt = runtime();
    rt.sound.set_font(Some(Arc::new(font)));
    rt.sound.take_samples();
    run_on(&mut rt, "BGMPLAY \"T120 @0 O4 C4\"").unwrap();
    frames(&mut rt, 10);
    let loud = rt
        .sound
        .take_samples()
        .iter()
        .fold(0f32, |m, s| m.max(s.abs()));
    assert!(loud > 0.01, "peak {loud}");
    run_on(&mut rt, "SNDSTOP").unwrap();
    frames(&mut rt, 5);
    rt.sound.take_samples();
    frames(&mut rt, 1);
    let left = rt
        .sound
        .take_samples()
        .iter()
        .fold(0f32, |m, s| m.max(s.abs()));
    assert!(left < 0.001, "peak after SNDSTOP {left}");
}

fn peak(rt: &mut Runtime) -> f32 {
    rt.sound
        .take_samples()
        .iter()
        .fold(0f32, |m, s| m.max(s.abs()))
}

#[test]
fn user_waveforms_and_the_psg_play_without_a_soundfont() {
    let mut rt = runtime();
    run_on(
        &mut rt,
        "WAVSET 224,127,0,0,127,\"FF00\"*8,69:BGMPLAY \"@224O4A1\"",
    )
    .unwrap();
    frames(&mut rt, 5);
    assert!(peak(&mut rt) > 0.05);
    run_on(&mut rt, "BGMPLAY \"@219 V127 C4\"").unwrap();
    rt.sound.take_samples();
    frames(&mut rt, 5);
    assert!(peak(&mut rt) > 0.05);
    run_on(&mut rt, "DIM W[512]:FOR I=0 TO 511:W[I]=SIN(I/8)*20000:NEXT\nWAVSETA 225,127,127,127,127,W,69,0,511,48000:BGMPLAY \"@225C\"").unwrap();
    rt.sound.take_samples();
    frames(&mut rt, 5);
    assert!(peak(&mut rt) > 0.05);
}

#[test]
fn beeps_take_handles() {
    let mut rt = runtime();
    assert_eq!(run_on(&mut rt, "?BEEP(152);BEEP();").unwrap(), "01");
    run_on(&mut rt, "WAVSET 230,127,127,127,127,\"FF00\"*8:H=BEEP(230,0,127):BEEPPIT H,1200:BEEPVOL H,64:BEEPPAN H,0").unwrap();
    rt.sound.take_samples();
    frames(&mut rt, 2);
    assert!(peak(&mut rt) > 0.01);
    run_on(&mut rt, "BEEPSTOP").unwrap();
    frames(&mut rt, 1);
    rt.sound.take_samples();
    frames(&mut rt, 1);
    assert_eq!(peak(&mut rt), 0.0);
    assert_eq!(
        run_on(&mut rt, "BEEP 200").unwrap_err().kind,
        ErrorKind::OutOfRange
    );
    assert_eq!(
        run_on(&mut rt, "BEEPVOL 16,1").unwrap_err().kind,
        ErrorKind::OutOfRange
    );
    assert_eq!(
        run_on(&mut rt, "WAVSET 224,0,0,0,0,\"FF0\"")
            .unwrap_err()
            .kind,
        ErrorKind::IllegalFunctionCall
    );
}
