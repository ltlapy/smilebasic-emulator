//! BGM commands, played frame by frame.

use std::sync::Arc;

use sb_core::error::ErrorKind;
use sb_core::vm::{Status, Vm};
use sb_runtime::Runtime;
use sb_runtime::font::Font;
use sb_runtime::machine::Machine;
use sb_runtime::sound::SoundFont;
use sb_runtime::sound::synth::Instruments;

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
    // A quarter note at 120 a minute lasts half a second: 30 frames, after
    // the 2 an idle sound system takes to start.
    run_on(&mut rt, "BGMPLAY \"T120C4\"").unwrap();
    frames(&mut rt, 31);
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
        "BGMSETD 128,\"@M\":BGMPLAY 1,128:BGMVAR 1,2,7:BGMVAR 1,2 OUT A:VSYNC:BGMVAR 1,2 OUT B\n\
         VSYNC 3:BGMVAR 1,2 OUT C:?A;B;C\n\
         @M:DATA \"$2=1 :0 C\",\"D\",0"
            .to_string(),
    );
    m.run_slot(0);
    for _ in 0..10 {
        m.frame();
    }
    assert!(!m.is_running());
    // BGMVAR's value shows a frame later, and the song sets $2 to 1 once
    // it starts, two frames after BGMPLAY.
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
    assert!(rows.iter().any(|r| r == "-171"), "{rows:?}");
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
        run_on(&mut rt, "BGMPLAY 74").unwrap_err().kind,
        ErrorKind::OutOfRange
    );
    assert_eq!(
        run_on(&mut rt, "BGMSET 127,\"C\"").unwrap_err().kind,
        ErrorKind::OutOfRange
    );
}

#[test]
fn song_variables_read_minus_one_while_nothing_plays() {
    let mut rt = runtime();
    assert_eq!(
        run_on(&mut rt, "BGMVAR 0,1,7:BGMVAR 0,1 OUT A:?BGMVAR(0,0);A").unwrap(),
        "-1-1"
    );
}

#[test]
fn preset_ranges_follow_4_4_9() {
    let mut rt = runtime();
    // 4.4.5 added song 62 to 73 and sound 156.
    assert_eq!(run_on(&mut rt, "BEEP 156:BGMPLAY 73:?1").unwrap(), "1");
    assert_eq!(
        run_on(&mut rt, "BEEP 157").unwrap_err().kind,
        ErrorKind::OutOfRange
    );
}

#[test]
fn unset_user_songs_play_nothing() {
    let mut rt = runtime();
    assert_eq!(run_on(&mut rt, "BGMPLAY 128:?BGMCHK()").unwrap(), "0");
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
    rt.sound
        .set_instruments(Some(Instruments::Font(Arc::new(font))));
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
    // PRINT works out its items right to left, so BEEP() takes handle 0.
    assert_eq!(run_on(&mut rt, "?BEEP(152);BEEP();").unwrap(), "10");
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

#[test]
fn pcm_streams_from_arrays() {
    let mut rt = runtime();
    run_on(
        &mut rt,
        "DIM W[4800]:FOR I=0 TO 4799:W[I]=SIN(I/4)*16000:NEXT:PCMSTREAM W",
    )
    .unwrap();
    rt.sound.take_samples();
    frames(&mut rt, 1);
    assert!(peak(&mut rt) > 0.3);
    assert_eq!(run_on(&mut rt, "PCMPOS OUT P:?P").unwrap(), "800");
    run_on(&mut rt, "PCMSTOP:PCMPOS 10").unwrap();
    frames(&mut rt, 1);
    assert_eq!(peak(&mut rt), 0.0);
    assert_eq!(
        run_on(&mut rt, "PCMCONT:PCMVOL 0,-100:PCMVOL OUT L,R:?L;R").unwrap(),
        "-10032767"
    );
    assert_eq!(
        run_on(&mut rt, "DIM S[3,4]:PCMSTREAM S").unwrap_err().kind,
        ErrorKind::IllegalFunctionCall
    );
    run_on(
        &mut rt,
        "DIM S[2,4]:PCMSTREAM S,24000:DIM A[9],B[9]:PCMSTREAM A,B",
    )
    .unwrap();
}

#[test]
fn effector_balance_and_talk_are_accepted() {
    let mut rt = runtime();
    assert_eq!(
        run_on(&mut rt, "EFCEN #EFCON:EFCSET #EFCCAVE:EFCWET 1,2,3,4:EFCEN OUT E:SNDMSBAL 10:SNDMSBAL OUT B:TALK \"A\":TALKCHK OUT T:TALKSTOP:?E;B;T").unwrap(),
        "1101"
    );
    assert_eq!(run_on(&mut rt, "?TALKCHK()").unwrap(), "0");
    assert_eq!(
        run_on(&mut rt, "?EFCEN();SNDMSBAL();SNDMVOL();PCMPOS()").unwrap(),
        "1101270"
    );
    run_on(&mut rt, "TALK \"こんにちは\"").unwrap();
    frames(&mut rt, 60);
    assert_eq!(run_on(&mut rt, "?TALKCHK()").unwrap(), "0");
    run_on(&mut rt, "EFCSET 0,0.5,100,1,0.5,1000,0.5,0.5,1").unwrap();
    assert_eq!(
        run_on(&mut rt, "EFCSET 4").unwrap_err().kind,
        ErrorKind::OutOfRange
    );
}

#[test]
fn recording_what_plays() {
    let mut rt = runtime();
    run_on(
        &mut rt,
        "DIM W[480]:FOR I=0 TO 479:W[I]=16000:NEXT:PCMSTREAM W:RECSTART 0.1",
    )
    .unwrap();
    // 0.1 seconds is 4800 samples, 6 frames: half are in after 3.
    frames(&mut rt, 3);
    assert_eq!(
        run_on(
            &mut rt,
            "RECCHK OUT C:RECDATA 5 OUT L,R:?C;RECLEN();RECPOS();L;R"
        )
        .unwrap(),
        "1480024001600016000"
    );
    assert_eq!(
        run_on(
            &mut rt,
            "DIM A[1]:RECSAVE A,10,2:DIM B[1,1]:RECSAVE B:?LEN(A);A[0];DIM(B,1);B[1,0]"
        )
        .unwrap(),
        "1016000240016000"
    );
    frames(&mut rt, 3);
    assert_eq!(run_on(&mut rt, "?RECCHK();RECPOS()").unwrap(), "04800");
    run_on(&mut rt, "RECSTART:RECSTOP").unwrap();
    assert_eq!(run_on(&mut rt, "?RECLEN()").unwrap(), "240000");
    assert_eq!(
        run_on(&mut rt, "RECSTART 61").unwrap_err().kind,
        ErrorKind::OutOfRange
    );
}

#[test]
fn silent_jingles_still_end() {
    // Without SmileBASIC's own songs, preset 5 ends after its length and
    // preset 0, which loops, goes on.
    let mut rt = runtime();
    run_on(&mut rt, "BGMPLAY 0,5:BGMPLAY 1,0").unwrap();
    frames(&mut rt, 100);
    assert_eq!(run_on(&mut rt, "?BGMCHK(0);BGMCHK(1)").unwrap(), "11");
    frames(&mut rt, 20);
    assert_eq!(run_on(&mut rt, "?BGMCHK(0);BGMCHK(1)").unwrap(), "01");
}
