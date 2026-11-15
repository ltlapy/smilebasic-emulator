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
    Runtime::for_tests(Font::blank())
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
fn bgmchk_after_a_fading_stop() {
    // Stopped in the frame it started in, the song never sounds.
    let mut rt = runtime();
    run_on(&mut rt, "BGMPLAY 0,1:BGMSTOP 0,1").unwrap();
    frames(&mut rt, 1);
    assert_eq!(run_on(&mut rt, "?BGMCHK(0)").unwrap(), "0");
    // After a VSYNC that did not wait, BGMCHK says 0 two frames on.
    run_on(&mut rt, "BGMPLAY 0,1").unwrap();
    rt.sound.synced();
    run_on(&mut rt, "BGMSTOP 0,1").unwrap();
    frames(&mut rt, 1);
    assert_eq!(run_on(&mut rt, "?BGMCHK(0)").unwrap(), "1");
    frames(&mut rt, 1);
    assert_eq!(run_on(&mut rt, "?BGMCHK(0)").unwrap(), "0");
    // A frame later it fades, and BGMCHK says 0 a frame after the fade.
    run_on(&mut rt, "BGMPLAY 0,1").unwrap();
    frames(&mut rt, 1);
    run_on(&mut rt, "BGMSTOP 0,0.5").unwrap();
    frames(&mut rt, 30);
    assert_eq!(run_on(&mut rt, "?BGMCHK(0)").unwrap(), "1");
    frames(&mut rt, 1);
    assert_eq!(run_on(&mut rt, "?BGMCHK(0)").unwrap(), "0");
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

/// A user song with nothing set stops what the track plays, though
/// BGMCHK still says 1 two frames on.
#[test]
fn an_unset_song_stops_the_track() {
    let mut rt = runtime();
    run_on(&mut rt, "BGMPLAY 0,200").unwrap();
    assert_eq!(run_on(&mut rt, "?BGMCHK(0)").unwrap(), "0");
    run_on(&mut rt, "BGMPLAY 0,0").unwrap();
    frames(&mut rt, 30);
    run_on(&mut rt, "BGMPLAY 0,200").unwrap();
    frames(&mut rt, 2);
    assert_eq!(run_on(&mut rt, "?BGMCHK(0)").unwrap(), "1");
    frames(&mut rt, 3);
    assert_eq!(run_on(&mut rt, "?BGMCHK(0)").unwrap(), "0");
    // Another track goes on.
    run_on(&mut rt, "BGMPLAY 1,0:BGMPLAY 0,200").unwrap();
    frames(&mut rt, 5);
    assert_eq!(run_on(&mut rt, "?BGMCHK(0);BGMCHK(1)").unwrap(), "01");
}

#[test]
fn variables_and_data() {
    let mut m = Machine::new(runtime());
    m.boot();
    m.set_slot(
        0,
        "VSYNC:BGMSETD 128,\"@M\":BGMPLAY 1,128:BGMVAR 1,2,7:BGMVAR 1,2 OUT A:VSYNC:BGMVAR 1,2 OUT B\n\
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
        ErrorKind::MmlLengthOutOfRange
    );
    // SmileBASIC 4's MML errors: 81 and the parser's own number.
    for (mml, code) in [
        ("CDX", 81),
        ("V200C", 94),
        ("C200", 93),
        ("{AB}C", 87),
        ("[C", 85),
        ("C]", 84),
        ("C:1D:0E", 83),
        ("@MP128,1,1,1C", 102),
        ("@E0,0,0,128C", 101),
        ("O9C<C", 92),
        ("$8=1", 103),
    ] {
        let e = run_on(&mut rt, &format!("BGMPLAY \"{mml}\"")).unwrap_err();
        assert_eq!(e.kind.code(), code, "{mml}");
    }
    // A `:` naming the channel in use is no new channel (TEST74).
    run_on(&mut rt, "BGMPLAY \":0C:0D\":BGMSTOP").unwrap();
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

/// Both of WAVSETA's forms count their ranges from the last sample, and an
/// empty array has none.
#[test]
fn wavseta_refuses_an_empty_array() {
    for source in [
        "DIM A[0]:WAVSETA 224,0,0,0,0,A",
        "DIM A[0]:WAVSETA 224,0,0,0,0,A,69",
    ] {
        let e = run_on(&mut runtime(), source).unwrap_err();
        assert_eq!(e.kind, ErrorKind::IllegalFunctionCall, "{source}");
        assert_eq!(
            e.detail.as_deref(),
            Some("(WAVSETA) (empty array)"),
            "{source}"
        );
    }
    for source in [
        "DIM A[0]:WAVSETA 224,0,0,0,0,A,69,0,0,28160",
        "DIM A[0]:WAVSETA 224,0,0,0,0,A,69,0,0,28160,0,1",
    ] {
        let e = run_on(&mut runtime(), source).unwrap_err();
        assert_eq!(
            (e.kind, e.arg),
            (ErrorKind::OutOfRange, Some(7)),
            "{source}"
        );
    }
}

/// WAVSETA looks at its arguments in order, then at the range: one with
/// nothing in it defines nothing and is no error, in both forms (TEST57).
#[test]
fn wavseta_checks_in_order() {
    let wrong = |source: &str, kind: ErrorKind, arg: Option<u8>| {
        let source = format!("DIM A%[16]:WAVSETA 224,0,0,0,0,A%,{source}");
        let e = run_on(&mut runtime(), &source).unwrap_err();
        assert_eq!((e.kind, e.arg), (kind, arg), "{source}");
    };
    let range = ErrorKind::OutOfRange;
    let illegal = ErrorKind::IllegalFunctionCall;
    wrong("128", range, Some(6));
    wrong("69,16", range, Some(7));
    wrong("69,-1", range, Some(7));
    wrong("69,0,16", range, Some(8));
    wrong("69,0,15,0", range, Some(9));
    wrong("69,0,15,96001", range, Some(9));
    wrong("69,0,15,28160,16", range, Some(10));
    // An argument left out counts where it is.
    wrong("128,,15", range, Some(6));
    wrong("69,0,15,28160,", ErrorKind::TypeMismatch, Some(10));
    wrong("69,0,15,28160,0,\"X\"", ErrorKind::TypeMismatch, Some(11));
    // Then the range, which comes before the offset's check.
    wrong("69,10,5", illegal, None);
    wrong("69,10,5,28160", illegal, None);
    wrong("69,10,5,28160,15", illegal, None);
    wrong("69,0,15,28160,4", range, Some(10));
    wrong("69,0,12,28160,4", range, Some(10));
    for (source, shows) in [
        ("69,5,5", "0"),
        ("69,5,5,28160", "0"),
        ("69,5,5,28160,15", "0"),
        ("69,0,11,28160,4", "1"),
        // The flag can be any number.
        ("69,0,11,28160,4,-7", "1"),
        ("69,0,1", "1"),
    ] {
        let mut rt = runtime();
        let program = format!(
            "DIM A%[16]:FILL A%,20000:WAVSETA 224,0,0,0,0,A%,{source}:BGMPLAY \"@224 V127 C4\""
        );
        run_on(&mut rt, &program).unwrap();
        rt.sound.take_samples();
        frames(&mut rt, 5);
        assert_eq!(u8::from(peak(&mut rt) > 0.0).to_string(), shows, "{source}");
    }
    // One sample has no range, and an array of it is no error either.
    for source in [
        "DIM A[1]:WAVSETA 224,0,0,0,0,A",
        "DIM A[1]:WAVSETA 224,0,0,0,0,A,69,0,0,28160",
        "DIM A[1]:WAVSETA 224,0,0,0,0,A,69,0,0,28160,0",
    ] {
        run_on(&mut runtime(), source).unwrap();
    }
}

/// Arguments can be left off the end but not out in between, in either
/// form, and that is looked at before the array is.
#[test]
fn wavseta_refuses_omitted_arguments() {
    for (source, arg) in [
        ("DIM A%[16]:WAVSETA 224,0,0,0,0,A%,,0,15", 6),
        ("DIM A%[16]:WAVSETA 224,0,0,0,0,A%,69,,15", 7),
        ("DIM A%[0]:WAVSETA 224,0,0,0,0,A%,,0,15", 6),
        ("DIM A[1]:WAVSETA 224,0,0,0,0,A,69,0,0,28160,,1", 10),
        ("DIM A[0]:WAVSETA 224,0,0,0,0,A,,0,0,28160", 6),
        ("DIM A[0]:WAVSETA 224,0,0,0,0,A,69,,,28160", 7),
        ("DIM A[1]:WAVSETA 224,0,0,0,0,A,69,0,,28160", 8),
        ("DIM A[1]:WAVSETA 224,0,0,0,0,A,69,0,0,,0", 9),
    ] {
        let e = run_on(&mut runtime(), source).unwrap_err();
        assert_eq!(e.kind, ErrorKind::TypeMismatch, "{source}");
        assert_eq!(e.arg, Some(arg), "{source}");
    }
}

/// CHKMML reads macros that BGMPLAY would refuse for how much they copy.
#[test]
fn chkmml_takes_macros_bgmplay_refuses() {
    let mut rt = runtime();
    let bomb = "VAR L,M$=\"{M0=CCCCCCCCCC}\"\n\
                FOR L=1 TO 7\n\
                M$=M$+\"{M\"+STR$(L)+\"=\"+(\"{M\"+STR$(L-1)+\"}\")*10+\"}\"\n\
                NEXT\n\
                M$=M$+\"{M7}\"\n";
    let chk = format!("{bomb}?CHKMML(M$)\n");
    assert_eq!(run_on(&mut rt, &chk).unwrap(), "-1");
    let play = format!("{bomb}BGMPLAY M$\n");
    let e = run_on(&mut rt, &play).unwrap_err();
    assert_eq!(e.kind, ErrorKind::IllegalMml);
}

/// BEEP of a user waveform holds a note as long as a pass of it, which for a
/// short waveform is none at all (TEST58's silence).
#[test]
fn beep_of_a_waveform_lasts_as_long_as_it() {
    let beep = |source: &str, at: usize| {
        let mut rt = runtime();
        run_on(&mut rt, source).unwrap();
        frames(&mut rt, at);
        rt.sound.take_samples();
        frames(&mut rt, 1);
        peak(&mut rt)
    };
    let fill = "VAR I:FOR I=0 TO LEN(W%)-1:W%[I]=228:IF I MOD 16>=8 THEN W%[I]=28\nNEXT\n";
    // 32 samples: a millisecond, less than the shortest note.
    let short = format!("DIM W%[32]\n{fill}WAVSETA 224,127,127,127,127,W%,69:H=BEEP(224,0,127)");
    assert_eq!(beep(&short, 2), 0.0);
    // TEST58 went on with a start equal to the end, which changes nothing.
    assert_eq!(
        beep(
            &format!("{short}:WAVSETA 224,127,127,127,127,W%,69,5,5:H=BEEP(224,0,127)"),
            2
        ),
        0.0
    );
    // WAVSET's string: a second, whatever its length.
    let wavset = "WAVSET 224,127,127,127,127,\"E020\"*8:H=BEEP(224,0,127)";
    assert!(beep(wavset, 5) > 0.01);
    assert!(beep(wavset, 55) > 0.01);
    assert_eq!(beep(wavset, 65), 0.0);
    // A longer array, by the 8-bit form, is 16384 samples once rounded up to
    // a power of two: 0.58 seconds, 35 frames.
    let long = format!("DIM W%[14080]\n{fill}WAVSETA 224,127,127,127,127,W%,69:H=BEEP(224,0,127)");
    assert!(beep(&long, 30) > 0.01);
    assert_eq!(beep(&long, 40), 0.0);
    // By 16-bit form, at its own rate: a quarter of a second, or for ever.
    let sixteen = "DIM W%[1000]:FILL W%,20000:WAVSETA 224,127,127,127,127,W%,69,0,999,4000";
    assert!(beep(&format!("{sixteen}:H=BEEP(224,0,127)"), 10) > 0.01);
    assert_eq!(beep(&format!("{sixteen}:H=BEEP(224,0,127)"), 20), 0.0);
    let flagged = format!("{sixteen},0,1:H=BEEP(224,0,127)");
    assert!(beep(&flagged, 60) > 0.01);
    // The same instrument is not cut short in a song.
    let song = format!("{short}:BEEPSTOP:BGMPLAY \"@224O4A1\"");
    assert!(beep(&song, 30) > 0.01);
}

/// The peak of `frames` frames of what `source` starts, and how many samples
/// up to the last that is not silent.
fn beep_recording(source: &str, n: usize) -> (i32, usize) {
    let mut rt = runtime();
    run_on(&mut rt, &format!("RECSTART {}:{source}", n as f64 / 60.0)).unwrap();
    frames(&mut rt, n);
    let got = run_on(
        &mut rt,
        "DIM R[1]:RECSAVE R:K=-1:FOR I=0 TO LEN(R)-1:P=MAX(P,ABS(R[I])):IF R[I] THEN K=I\nNEXT:?P;\",\";K+1;",
    )
    .unwrap();
    let (peak, end) = got.split_once(',').unwrap();
    (peak.parse().unwrap(), end.parse().unwrap())
}

const WAVSET_SQUARE: &str = "WAVSET 224,127,127,127,127,\"FFFF0000\"*8,69";

/// BEEP's volume is a linear gain, by which 64 is as loud as BGMPLAY's
/// default: an eighth of full scale on each side for a full-scale waveform.
/// The square holds at 4096; the filter waveforms are read through rings a
/// little past that at each edge, to 4814.
#[test]
fn beep_of_a_waveform_is_as_loud_as_a_song() {
    let beep = |volume: i32| beep_recording(&format!("{WAVSET_SQUARE}:H=BEEP(224,0,{volume})"), 30);
    let (at_64, _) = beep(64);
    let (song, _) = beep_recording(&format!("{WAVSET_SQUARE}:BGMPLAY \"@224O4A1\""), 30);
    assert_eq!(at_64, 4814);
    assert_eq!(at_64, song);
    let (at_127, _) = beep(127);
    assert!((at_127 - 4814 * 127 / 64).abs() <= 1, "{at_127}");
    let (at_32, _) = beep(32);
    assert!((at_32 - 2407).abs() <= 1, "{at_32}");
    let (default, _) = beep_recording(&format!("{WAVSET_SQUARE}:H=BEEP(224)"), 30);
    assert_eq!(default, at_64);
}

/// The last step of a note, of 5 ms, is its fall.
#[test]
fn beep_of_a_waveform_ends_on_the_sound_engines_steps() {
    let ms = |source: &str, n: usize| beep_recording(source, n).1 * 1000 / 48000;
    assert_eq!(ms(&format!("{WAVSET_SQUARE}:H=BEEP(224)"), 90), 1000);
    let array = |n: usize, a: &str| {
        format!(
            "DIM W%[{n}]:FILL W%,228:FOR I=8 TO {n}-1 STEP 16:W%[I]=28:NEXT:WAVSETA 225,127,127,127,127,W%,69{a}:H=BEEP(225)"
        )
    };
    assert_eq!(ms(&array(256, ""), 10), 10);
    assert_eq!(ms(&array(4096, ""), 20), 145);
    let sixteen =
        "DIM W%[28160]:FILL W%,30000:WAVSETA 225,127,127,127,127,W%,69,0,28159,28160:H=BEEP(225)";
    assert_eq!(ms(sixteen, 90), 1000);
}

/// With the repeat flag each pass is a new note, over the last one's fall.
#[test]
fn a_repeating_beep_doubles_for_a_moment_at_each_pass() {
    let mut rt = runtime();
    run_on(
        &mut rt,
        "RECSTART 2:DIM W%[28160]:FILL W%,30000:\
         WAVSETA 225,127,127,127,127,W%,69,0,28159,28160,0,1:H=BEEP(225)",
    )
    .unwrap();
    frames(&mut rt, 120);
    let got = run_on(
        &mut rt,
        "DIM S[1]:RECSAVE S:FOR I=0 TO LEN(S)-1:IF ABS(S[I])>5000 THEN B=I:IF A==0 THEN A=I\n\
         NEXT:?A DIV 48;\",\";B DIV 48;\",\";ABS(S[47000]);",
    )
    .unwrap();
    // Loud from 5 ms before the first second ends, and last before the second
    // second's end; between them, one note's level.
    assert_eq!(got, "995,1998,3750");
}

/// `source`'s peak and the millisecond of its last sample that is not silent,
/// with `then` run after `first` frames and `rest` frames more.
fn stopped_recording(source: &str, first: usize, then: &str, rest: usize) -> (i32, usize) {
    let mut rt = runtime();
    let seconds = (first + rest) as f64 / 60.0;
    run_on(&mut rt, &format!("RECSTART {seconds}:{source}")).unwrap();
    frames(&mut rt, first);
    run_on(&mut rt, then).unwrap();
    frames(&mut rt, rest);
    let got = run_on(
        &mut rt,
        "DIM R[1]:RECSAVE R:K=-1:FOR I=0 TO LEN(R)-1:P=MAX(P,ABS(R[I])):IF R[I] THEN K=I\nNEXT:?P;\",\";K DIV 48;",
    )
    .unwrap();
    let (peak, end) = got.split_once(',').unwrap();
    (peak.parse().unwrap(), end.parse().unwrap())
}

const SLOW_SQUARE: &str = "DIM W%[2816]:FOR I=0 TO 2815:W%[I]=20000*(I MOD 64<32)-10000:NEXT";

/// With a slow release the notes of a repeating BEEP pile up, and BEEPSTOP
/// lets them all fall as they would.
#[test]
fn a_repeating_beeps_notes_pile_up_and_fall_after_beepstop() {
    let flagged =
        format!("{SLOW_SQUARE}:WAVSETA 225,127,127,127,32,W%,69,0,2815,28160,0,1:H=BEEP(225)");
    let (peak, end) = stopped_recording(&flagged, 60, "BEEPSTOP H", 30);
    // One note peaks at about 1400.
    assert!((8500..10500).contains(&peak), "{peak}");
    assert_eq!(end, 1499);
    // With a fade the notes fade too, and nothing is left at its end.
    let (_, end) = stopped_recording(&flagged, 60, "BEEPSTOP H,0.25", 30);
    assert!((1240..1255).contains(&end), "{end}");
}

/// BEEPSTOP of a note with a slow release and no flag does the same.
#[test]
fn beepstop_lets_a_slow_release_run_without_the_flag() {
    let slow = "WAVSET 224,127,127,127,32,\"FFFF0000\"*8,69:H=BEEP(224)";
    let (_, end) = stopped_recording(slow, 6, "BEEPSTOP H", 54);
    assert_eq!(end, 999);
    // The handle is free at once, and BEEPVOL of it does not reach the fall.
    let (quiet, _) = stopped_recording(slow, 6, "BEEPSTOP H:BEEPVOL H,0", 54);
    assert!(quiet > 3000, "{quiet}");
}

/// A BEEP that takes over a statement's handle lets its note fall too, a step
/// for release 127, and SNDSTOP stops everything at once.
#[test]
fn a_note_taken_over_falls_and_sndstop_cuts() {
    let quiet = "FOR I=1 TO 16:BEEP 225,0,0:NEXT";
    let square = format!("{WAVSET_SQUARE}:WAVSET 225,127,127,127,127,\"FFFF0000\"*8,69");
    let (_, end) = stopped_recording(&format!("{square}:BEEP 224"), 24, quiet, 36);
    assert_eq!(end, 404);
    let slow = "WAVSET 224,127,127,127,32,\"FFFF0000\"*8,69:H=BEEP(224)";
    let (_, end) = stopped_recording(slow, 6, "BEEPSTOP H:SNDSTOP", 54);
    assert_eq!(end, 99);
}

/// BEEPSTOP takes a step to fall, and frees the handle at once.
#[test]
fn beepstop_of_a_waveform_takes_a_step() {
    let mut rt = runtime();
    run_on(&mut rt, &format!("RECSTART 1:{WAVSET_SQUARE}:H=BEEP(224)")).unwrap();
    frames(&mut rt, 6);
    run_on(&mut rt, "BEEPSTOP H").unwrap();
    frames(&mut rt, 54);
    let got = run_on(
        &mut rt,
        "DIM S[1]:RECSAVE S:FOR I=0 TO LEN(S)-1:IF S[I] THEN B=I\nNEXT:?B DIV 48;",
    )
    .unwrap();
    assert_eq!(got, "104");
    assert_eq!(run_on(&mut rt, "?BEEP(0);").unwrap(), "1");
}

/// BEEP's handles go on rising and round, whether the last has ended or not.
#[test]
fn beep_handles_go_round() {
    let mut rt = runtime();
    let handle = |rt: &mut Runtime, sound: i32| {
        run_on(rt, &format!("?BEEP({sound});"))
            .unwrap()
            .parse::<i32>()
            .unwrap()
    };
    // 0 to 15, ended or not: a preset without the sounds ends at once, and
    // one sounding still only holds its own number.
    run_on(&mut rt, WAVSET_SQUARE).unwrap();
    for n in 0..8 {
        assert_eq!(handle(&mut rt, 0), n);
        frames(&mut rt, 1);
    }
    assert_eq!(handle(&mut rt, 224), 8);
    // A BEEP with no use for its handle takes a number too.
    run_on(&mut rt, "BEEP 0").unwrap();
    assert_eq!(handle(&mut rt, 0), 10);
    frames(&mut rt, 1);
    for n in 11..16 {
        assert_eq!(handle(&mut rt, 0), n);
        frames(&mut rt, 1);
    }
    // Round again, past the one still held.
    assert_eq!(handle(&mut rt, 0), 0);
    frames(&mut rt, 1);
    for n in 1..8 {
        assert_eq!(handle(&mut rt, 0), n);
        frames(&mut rt, 1);
    }
    assert_eq!(handle(&mut rt, 0), 9);
    // BEEPSTOP lets it go.
    run_on(&mut rt, "BEEPSTOP 8").unwrap();
    frames(&mut rt, 1);
    for n in 10..16 {
        assert_eq!(handle(&mut rt, 0), n);
        frames(&mut rt, 1);
    }
    for n in 0..9 {
        assert_eq!(handle(&mut rt, 0), n);
        frames(&mut rt, 1);
    }
}

/// With all 16 held there is no handle, and no error; then BEEPSTOP's number
/// is out of range, and one held but not stopped still reaches its sound.
#[test]
fn with_all_beep_handles_held_there_is_none() {
    let mut rt = runtime();
    run_on(&mut rt, WAVSET_SQUARE).unwrap();
    let all = "FOR I=0 TO 15:H=BEEP(224):NEXT:";
    assert_eq!(
        run_on(&mut rt, &format!("{all}?H;BEEP(224);")).unwrap(),
        "15-1"
    );
    run_on(&mut rt, "BEEP 224").unwrap();
    assert_eq!(
        run_on(&mut rt, "BEEPSTOP -1").unwrap_err().kind,
        ErrorKind::OutOfRange
    );
    run_on(&mut rt, "BEEPSTOP 3").unwrap();
    assert_eq!(run_on(&mut rt, "?BEEP(224);").unwrap(), "3");
    // Once they end the numbers are free again, from where they were.
    frames(&mut rt, 70);
    assert_eq!(run_on(&mut rt, "?BEEP(224);").unwrap(), "4");
}

/// A handle's number reaches whatever plays on it now, as it is the number
/// of a place to play in: an old one has no sound of its own to tell apart.
#[test]
fn an_old_beep_handle_reaches_what_plays_there() {
    let mut rt = runtime();
    run_on(&mut rt, &format!("{WAVSET_SQUARE}:H=BEEP(224)")).unwrap();
    frames(&mut rt, 70);
    // It ended: this does nothing, and is no error.
    run_on(&mut rt, "BEEPVOL 0,1:BEEPPIT 0,100:BEEPPAN 0,1:BEEPSTOP 0").unwrap();
    for _ in 1..16 {
        run_on(&mut rt, "BEEP 0").unwrap();
    }
    frames(&mut rt, 1);
    run_on(&mut rt, "H=BEEP(224)").unwrap();
    rt.sound.take_samples();
    frames(&mut rt, 1);
    assert!(peak(&mut rt) > 0.05);
    run_on(&mut rt, "BEEPVOL 0,0").unwrap();
    frames(&mut rt, 1);
    rt.sound.take_samples();
    frames(&mut rt, 1);
    assert_eq!(peak(&mut rt), 0.0);
}

/// BEEP of a waveform too short for a note is no error, and gives a handle.
#[test]
fn a_silent_beep_still_gives_a_handle() {
    let mut rt = runtime();
    assert_eq!(
        run_on(
            &mut rt,
            "DIM W%[32]:WAVSETA 224,127,127,127,127,W%,69:?BEEP(224)"
        )
        .unwrap(),
        "0"
    );
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
    // 16, 32, ... 512 samples, and no other length.
    run_on(&mut rt, "WAVSET 224,0,0,0,0,\"FF00\"*64").unwrap();
    for bad in ["\"FF00\"*257", "\"FF\"*17"] {
        assert_eq!(
            run_on(&mut rt, &format!("WAVSET 224,0,0,0,0,{bad}"))
                .unwrap_err()
                .kind,
            ErrorKind::IllegalFunctionCall
        );
    }
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
    // 16000 of 32767, at 0.35 of full scale for a full sample.
    assert!(peak(&mut rt) > 0.16);
    assert_eq!(run_on(&mut rt, "PCMPOS OUT P:?P").unwrap(), "800");
    run_on(&mut rt, "PCMSTOP:PCMPOS 10").unwrap();
    frames(&mut rt, 1);
    assert_eq!(peak(&mut rt), 0.0);
    assert_eq!(
        run_on(&mut rt, "PCMCONT:PCMVOL 0,-100:PCMVOL OUT L,R:?L;R").unwrap(),
        "-10032767"
    );
    run_on(
        &mut rt,
        "DIM S[2,4]:PCMSTREAM S,24000:DIM A[9],B[9]:PCMSTREAM A,B",
    )
    .unwrap();
}

/// PCMSTREAM takes integers or reals in one or two dimensions, or two 1D
/// arrays as long as each other, and none that is empty.
#[test]
fn pcm_stream_checks_its_arrays() {
    let mut rt = runtime();
    for (bad, kind, arg) in [
        ("DIM S$[4]:PCMSTREAM S$", ErrorKind::TypeMismatch, 0),
        ("DIM S[2,2,2]:PCMSTREAM S", ErrorKind::TypeMismatch, 0),
        ("DIM S[0]:PCMSTREAM S", ErrorKind::IllegalFunctionCall, 0),
        (
            "DIM S[2,4]:RESIZE S,0,4:PCMSTREAM S,8000",
            ErrorKind::IllegalFunctionCall,
            0,
        ),
        (
            "DIM A[4],B[5]:PCMSTREAM A,B",
            ErrorKind::IllegalFunctionCall,
            1,
        ),
        ("DIM A[4],B[2,2]:PCMSTREAM A,B", ErrorKind::TypeMismatch, 1),
        ("DIM A[2,2],B[4]:PCMSTREAM A,B", ErrorKind::TypeMismatch, 0),
        ("DIM A[4]:PCMSTREAM A,8000,8000", ErrorKind::TypeMismatch, 1),
        ("DIM A[4]:PCMSTREAM A,0", ErrorKind::OutOfRange, 1),
        ("PCMSTREAM 192001", ErrorKind::OutOfRange, 0),
    ] {
        let e = run_on(&mut rt, bad).unwrap_err();
        assert_eq!((e.kind, e.arg), (kind, Some(arg)), "{bad}");
    }
    // Rows past the second are not played.
    run_on(&mut rt, "DIM S[3,4]:PCMSTREAM S").unwrap();
}

/// An integer plays as its low 16 bits, so a program may store samples as
/// 0 to 65535, and a real as a whole number held within 16 bits.
#[test]
fn pcm_samples_are_16_bits() {
    let mut rt = runtime();
    for (array, sample, level) in [
        ("W%", "65535-16000", -16000.0),
        ("W%", "&H10000+16000", 16000.0),
        ("W%", "-16000", -16000.0),
        ("W#", "1E308", 32767.0),
        ("W#", "-40000", -32768.0),
        ("W#", "0.9", 0.0),
        ("W#", "-16000.9", -16000.0),
    ] {
        let fill = format!("DIM {array}[480]:FILL {array},{sample}:PCMSTREAM {array}");
        run_on(&mut rt, &fill).unwrap();
        rt.sound.take_samples();
        frames(&mut rt, 1);
        let samples = rt.sound.take_samples();
        // A full sample plays at 0.35 of full scale.
        let want = (level / 32767.0 * 0.35) as f32;
        assert!(
            samples.iter().all(|s| (s - want).abs() < 1e-4),
            "{fill}: {} for {want}",
            samples[0]
        );
        run_on(&mut rt, "PCMSTOP").unwrap();
    }
}

/// A stream keeps the length its arrays had at PCMSTREAM. Made shorter
/// since, they play silence where they have nothing, also once PCMCONT
/// goes on with a stream whose array was emptied.
#[test]
fn a_pcm_stream_outlives_its_array() {
    let mut rt = runtime();
    run_on(
        &mut rt,
        "DIM S%[2,480]:FILL S%,16000:PCMSTREAM S%:PCMSTOP:RESIZE S%,0,480:PCMCONT",
    )
    .unwrap();
    rt.sound.take_samples();
    frames(&mut rt, 2);
    assert_eq!(peak(&mut rt), 0.0);
    assert_eq!(run_on(&mut rt, "?PCMPOS()").unwrap(), "160");
    // The left row is still there, the right one is not.
    run_on(
        &mut rt,
        "DIM T%[2,480]:FILL T%,16000:PCMSTREAM T%:RESIZE T%,1,480",
    )
    .unwrap();
    rt.sound.take_samples();
    frames(&mut rt, 1);
    let samples = rt.sound.take_samples();
    assert!(samples.chunks(2).all(|s| s[0] > 0.16 && s[1] == 0.0));
    // A rate alone changes the rate and nothing else.
    run_on(&mut rt, "PCMPOS 0:PCMSTREAM 24000").unwrap();
    frames(&mut rt, 1);
    assert_eq!(run_on(&mut rt, "?PCMPOS()").unwrap(), "400");
    // An array grown since goes round at its old length.
    assert_eq!(
        run_on(
            &mut rt,
            "DIM M%[100]:PCMSTREAM M%:RESIZE M%,48000:PCMPOS 150:?PCMPOS()"
        )
        .unwrap(),
        "50"
    );
}

#[test]
fn effector_balance_and_talk_are_accepted() {
    let mut rt = runtime();
    assert_eq!(run_on(&mut rt, "?SNDMSBAL()").unwrap(), "63");
    assert_eq!(
        run_on(&mut rt, "EFCEN #EFCON:EFCSET #EFCCAVE:EFCWET 1,2,3,4:EFCEN OUT E:SNDMSBAL 10:SNDMSBAL OUT B:TALK \"A\":TALKCHK OUT T:TALKSTOP:?E;B;T").unwrap(),
        "1631"
    );
    assert_eq!(run_on(&mut rt, "?TALKCHK()").unwrap(), "0");
    // The main program's SNDMSBAL does nothing; SNDMVOL starts at 88.
    assert_eq!(
        run_on(&mut rt, "?EFCEN();SNDMSBAL();SNDMVOL();PCMPOS()").unwrap(),
        "163880"
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

/// TALK and a sung note do not cut each other, whichever starts first;
/// a sung note's letters must make syllables.
#[test]
fn talk_and_sung_notes_sound_together() {
    let mut rt = runtime();
    for src in [
        "TALK \"あいうえお\":BGMPLAY \"T120@0O4*U,C1\"",
        "BGMPLAY \"T120@0O4*U,C1\":TALK \"あいうえお\"",
    ] {
        run_on(&mut rt, src).unwrap();
        frames(&mut rt, 10);
        assert_eq!(
            run_on(&mut rt, "?TALKCHK();BGMCHK()").unwrap(),
            "11",
            "{src}"
        );
        run_on(&mut rt, "SNDSTOP").unwrap();
    }
    assert_eq!(
        run_on(&mut rt, "BGMPLAY \"T60@0O4*KONNICHIWA,C1\"")
            .unwrap_err()
            .kind,
        ErrorKind::IllegalMml
    );
    // Three letters at most to a note; letters that are no phoneme pass.
    run_on(&mut rt, "BGMPLAY \"T60@0O4*KO,C*Q,C*NI,C*CHI,C*WA,C1\"").unwrap();
    run_on(&mut rt, "BGMPLAY \"T60@0O4*XA,C*KXA,C\"").unwrap();
    frames(&mut rt, 10);
}

#[test]
fn the_effector_rings_on_after_a_note_but_is_not_recorded() {
    let tail = |setup: &str| {
        let mut rt = runtime();
        let program = format!(
            "WAVSET 224,127,0,127,127,\"E020\"*16,69:RECSTART 1:{setup}:BGMPLAY \"T120@224O4A16\""
        );
        run_on(&mut rt, &program).unwrap();
        // A sixteenth lasts 125 ms; listen from 300 ms to 600 ms.
        frames(&mut rt, 20);
        rt.sound.take_samples();
        frames(&mut rt, 18);
        let heard = peak(&mut rt);
        let recorded = run_on(&mut rt, "RECDATA 24000 OUT L,R:?ABS(L)+ABS(R)").unwrap();
        (heard, recorded)
    };
    assert_eq!(tail("EFCEN 0"), (0.0, "0".to_string()));
    let (heard, recorded) = tail("EFCSET #EFCCAVE:EFCWET 0,127,0,0:EFCEN 1");
    assert!(heard > 0.001, "{heard}");
    assert_eq!(recorded, "0");
    // Sending nothing, nothing rings: RECSTART sends nothing until EFCWET.
    assert_eq!(tail("EFCSET #EFCCAVE:EFCWET 127,0,127,127:EFCEN 1").0, 0.0);
    assert_eq!(tail("EFCSET #EFCCAVE:EFCEN 1").0, 0.0);
}

/// What the effector hears of each sound. The checks read what plays:
/// a full-scale square BEEP or BGM note at 64 records at an eighth of full
/// scale, and plays as loud with EFCWET 127.
#[test]
fn the_effector_sends_and_resets() {
    let heard = |setup: &str, sound: &str| {
        let mut rt = runtime();
        run_on(&mut rt, &format!("{WAVSET_SQUARE}:{setup}:{sound}")).unwrap();
        frames(&mut rt, 10);
        rt.sound.take_samples();
        frames(&mut rt, 2);
        peak(&mut rt)
    };
    let beep = "H=BEEP(224)";
    let song = "BGMPLAY \"@224O4A1\"";
    let near = |a: f32, b: f32| (a - b).abs() < 0.02 * b;
    for sound in [beep, song] {
        // EFCWET starts at 127, and with the effector off what is sent
        // passes it: a full-scale square at 64 plays at an eighth of full
        // scale, its waveform filter's ringing aside.
        let full = heard("", sound);
        assert!((0.12..0.15).contains(&full), "{sound} {full}");
        // Sending nothing, half as loud.
        let quiet = heard("EFCWET 0,0,0,0", sound);
        assert!(near(quiet, full / 2.0), "{sound} {quiet}");
        // SNDSTOP sends all of it again, and RECSTART nothing.
        let reset = heard("EFCWET 0,0,0,0:SNDSTOP", sound);
        assert!(near(reset, full), "{sound} {reset}");
        let recording = heard("RECSTART", sound);
        assert!(near(recording, full / 2.0), "{sound} {recording}");
        // With the effector on, the sound is not lowered by what it sends.
        let on = heard("EFCSET #EFCBATH:EFCWET 0,0,0,0:EFCEN 1", sound);
        assert!(near(on, quiet), "{sound} {on}");
    }
    // TALK with all of it sent and the effector on says nothing.
    let talk = "TALK \"あいうえおあいうえお\"";
    assert!(heard("", talk) > 0.001);
    assert!(heard("EFCSET #EFCBATH:EFCEN 1", talk) < 1e-6);
    let mut rt = runtime();
    let got = run_on(&mut rt, "EFCEN 1:A=EFCEN():SNDSTOP:?A;EFCEN()").unwrap();
    assert_eq!(got, "10");
}

/// BGMPLAY starts a track with EFCWET's send, whatever BGMWET said before.
#[test]
fn bgmplay_forgets_bgmwet() {
    let quiet = |setup: &str| {
        let mut rt = runtime();
        run_on(&mut rt, &format!("{WAVSET_SQUARE}:{setup}")).unwrap();
        frames(&mut rt, 10);
        rt.sound.take_samples();
        frames(&mut rt, 2);
        peak(&mut rt)
    };
    let song = "BGMPLAY \"@224O4A1\"";
    let full = quiet(song);
    assert_eq!(quiet(&format!("BGMWET 0,0:{song}")), full);
    // During the song, BGMWET sends that track as it says.
    let set = quiet(&format!("{song}:BGMWET 0,0"));
    assert!((set - full / 2.0).abs() < 0.002, "{set}");
}

#[test]
fn a_recording_waits_for_a_song_waking_the_sound_system() {
    let mut rt = runtime();
    run_on(
        &mut rt,
        "WAVSET 224,127,0,127,100,\"E020\"*16,69:RECSTART 0.5:BGMPLAY \"T120@224V127O4A1\"",
    )
    .unwrap();
    frames(&mut rt, 10);
    // The first sample already sounds, as on hardware.
    let first = run_on(&mut rt, "RECDATA 0 OUT L,R:?ABS(L)+ABS(R)>64").unwrap();
    assert_eq!(first, "1");
}

/// RECSTART keeps what older recordings left in the buffer, and a
/// recording round the buffer reads it before 0 and past its end.
#[test]
fn recordings_keep_what_older_ones_left() {
    let mut rt = runtime();
    run_on(
        &mut rt,
        "WAVSET 224,127,0,127,100,\"E020\"*16,69:BGMPLAY \"T120@224V127O4[A1]\":RECSTART",
    )
    .unwrap();
    // Over five seconds: the whole buffer.
    frames(&mut rt, 310);
    run_on(&mut rt, "SNDSTOP:RECSTART").unwrap();
    frames(&mut rt, 10);
    let mut sum = |from: &str| {
        let program = format!(
            "DIM A[1]:RECSAVE A,1000,{from}:VAR I,S:FOR I=0 TO 999:S=S+ABS(A[I]):NEXT:?S>100000"
        );
        run_on(&mut rt, &program).unwrap()
    };
    assert_eq!(sum("-1000"), "1");
    assert_eq!(sum("RECPOS()+1000"), "1");
    // What this recording took: silence.
    assert_eq!(sum("4000"), "0");
}

#[test]
fn recording_what_plays() {
    let mut rt = runtime();
    run_on(
        &mut rt,
        "DIM W%[28160]:FILL W%,30000:\
         WAVSETA 225,127,127,127,127,W%,69,0,28159,28160:H=BEEP(225):RECSTART 0.1",
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
        "14800240037503750"
    );
    assert_eq!(
        run_on(
            &mut rt,
            "DIM A[1]:RECSAVE A,10,2:DIM B[1,1]:RECSAVE B:?LEN(A);A[0];DIM(B,1);B[1,0]"
        )
        .unwrap(),
        // The note's first sample is a little below the rest: the filter
        // reads the silence before it.
        "10375024003647"
    );
    frames(&mut rt, 3);
    assert_eq!(run_on(&mut rt, "?RECCHK();RECPOS()").unwrap(), "04800");
    run_on(&mut rt, "RECSTART:RECSTOP").unwrap();
    assert_eq!(run_on(&mut rt, "?RECLEN()").unwrap(), "240000");
    assert_eq!(
        run_on(&mut rt, "RECSTART 61").unwrap_err().kind,
        ErrorKind::OutOfRange
    );
    // Recording round the buffer, a position below 0 is from its end.
    assert_eq!(
        run_on(&mut rt, "RECSTART:RECDATA -1 OUT L,R:?\"OK\"").unwrap(),
        "OK"
    );
    assert!(run_on(&mut rt, "RECSTART 1:RECDATA -1 OUT L,R").is_err());
}

/// RECSTART does not hear PCMSTREAM, which still plays.
#[test]
fn recording_leaves_out_the_pcm_stream() {
    let mut rt = runtime();
    run_on(
        &mut rt,
        "DIM W[480]:FOR I=0 TO 479:W[I]=16000:NEXT:PCMSTREAM W:RECSTART 0.1",
    )
    .unwrap();
    rt.sound.take_samples();
    frames(&mut rt, 3);
    assert!(peak(&mut rt) > 0.16);
    assert_eq!(
        run_on(&mut rt, "RECDATA 5 OUT L,R:?RECPOS();L;R").unwrap(),
        "240000"
    );
}

/// PCMVOL reads 0 0 before the first stream and keeps what it is given
/// without one; PCMSTREAM sets both sides back to full volume.
#[test]
fn a_pcm_stream_starts_at_full_volume() {
    let mut rt = runtime();
    assert_eq!(
        run_on(&mut rt, "PCMVOL OUT L,R:PCMVOL 0,4096:PCMVOL OUT A:?L;R;A").unwrap(),
        "004096"
    );
    assert_eq!(
        run_on(
            &mut rt,
            "DIM W[480]:PCMVOL 0,4096:PCMVOL 1,4096:PCMSTREAM W:PCMVOL OUT L,R:?L;R"
        )
        .unwrap(),
        "3276732767"
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

/// An octave set inside a chord ends with the chord, and a bare length
/// after a tied chord goes on with it. Without the first, the `<` after
/// the chord would leave octave 9.
#[test]
fn a_chords_octave_ends_with_the_chord() {
    let mut rt = runtime();
    run_on(&mut rt, "BGMPLAY \"T240O8|A<CE|96&96<C1\"").unwrap();
    frames(&mut rt, 20);
    let tracks = rt.sound.inspect_tracks();
    let ch = &tracks[0].channels[0];
    assert_eq!((ch.octave, ch.keys.as_slice()), (Some(9), [120].as_slice()));
}

#[test]
fn the_debugger_sees_what_tracks_and_beeps_play() {
    use sb_runtime::sound::inspect::{BeepKind, State};
    let mut rt = runtime();
    let mml = "T90$2=7:1@3O5V90P20[C4]:4R1";
    run_on(&mut rt, &format!("BGMSET 130,\"{mml}\":BGMPLAY 3,130,100")).unwrap();
    run_on(&mut rt, "BGMPLAY 1,0,,1").unwrap();
    // Waking, then a tick or so a frame at 90 quarters a minute.
    assert_eq!(rt.sound.inspect_tracks()[1].state, State::Waking);
    frames(&mut rt, 6);
    let tracks = rt.sound.inspect_tracks();
    assert_eq!(tracks.len(), 2);
    let (preset, user) = (&tracks[0], &tracks[1]);
    assert_eq!((preset.track, preset.song, preset.silent), (1, 0, true));
    assert_eq!(preset.state, State::FadingIn);
    assert_eq!((user.track, user.song, user.volume), (3, 130, 100));
    assert_eq!(user.source.as_deref(), Some(mml));
    assert_eq!(
        (user.state, user.tempo, user.quarter),
        (State::Playing, 90, 48)
    );
    assert_eq!((user.ticks, user.vars[2]), (5, 7));
    // The channels the song uses, with what they last set.
    let c: Vec<usize> = user.channels.iter().map(|c| c.channel).collect();
    assert_eq!(c, [0, 1, 4]);
    let one = &user.channels[1];
    assert_eq!(
        (one.program, one.octave, one.volume, one.pan),
        (3, Some(5), 90, 20)
    );
    assert_eq!((one.keys.as_slice(), one.notes), ([72].as_slice(), 1));
    assert!(user.channels[0].ended && !one.ended);
    // BGMPLAY with MML is song 255; paused tracks still show.
    run_on(&mut rt, "BGMPLAY \"CDE\":BGMPAUSE 3:BGMSTOP 1").unwrap();
    let tracks = rt.sound.inspect_tracks();
    assert_eq!(tracks.len(), 2);
    assert_eq!((tracks[0].song, tracks[1].state), (255, State::Paused));
    assert_eq!(tracks[0].source.as_deref(), Some("CDE"));

    run_on(&mut rt, "BEEP 300,-50,90,10:BEEP 2").unwrap();
    run_on(&mut rt, "BEEPPIT 0,120").unwrap();
    let beeps = rt.sound.inspect_beeps();
    assert_eq!(beeps.len(), 2);
    assert_eq!(
        (beeps[0].sound, beeps[0].kind),
        (300, BeepKind::Instrument(44))
    );
    assert_eq!(
        (beeps[0].pitch, beeps[0].volume, beeps[0].pan),
        (120, 90, 10)
    );
    assert_eq!((beeps[1].sound, beeps[1].kind), (2, BeepKind::Silent));
    // Nothing for a silent preset to play, it ends at once.
    frames(&mut rt, 1);
    assert_eq!(rt.sound.inspect_beeps().len(), 1);
}

/// `@MA`'s depth and range together ask for ±756 dB here, which is beyond
/// a float: at full scale and as not-a-number, the song was a blast. The
/// level stops at +6 dB, and at silence below -90.4 dB.
#[test]
fn a_wide_tremolo_stops_at_the_sound_engines_limits() {
    let square = "WAVSET 224,127,0,127,127,\"FF00\"*8,69:";
    let mut rt = runtime();
    run_on(&mut rt, &format!("{square}BGMPLAY \"V127@224O4A1\"")).unwrap();
    frames(&mut rt, 30);
    let plain = peak(&mut rt);
    assert!(plain > 0.05 && plain < 0.5, "{plain}");

    for ma in ["@MA127,127,12,0", "@MA127,110,127,0"] {
        let mut rt = runtime();
        run_on(&mut rt, &format!("{square}BGMPLAY \"V127@224{ma}O4A1\"")).unwrap();
        let (mut loudest, mut silent_frames) = (0f32, 0);
        for _ in 0..80 {
            frames(&mut rt, 1);
            let samples = rt.sound.take_samples();
            assert!(samples.iter().all(|s| s.is_finite()), "{ma}");
            let frame = samples.iter().fold(0f32, |m, s| m.max(s.abs()));
            loudest = loudest.max(frame);
            silent_frames += usize::from(frame == 0.0);
        }
        let ratio = loudest / plain;
        assert!((1.9..2.05).contains(&ratio), "{ma}: {ratio}");
        // The slow one has whole frames in the bottom of its swing.
        assert!(ma.ends_with("127,0") || silent_frames > 3, "{ma}");
    }
}

/// PCMSTREAM holds a real within 16 bits, however large it is, so that it
/// stays out of the output and of the effector's tail, which would ring on
/// with it.
#[test]
fn samples_out_of_all_range_stay_out_of_the_mix() {
    let mut rt = runtime();
    run_on(
        &mut rt,
        "EFCSET #EFCCAVE:EFCWET 0,0,0,127:EFCEN 1:DIM W[480]:FOR I=0 TO 479:W[I]=1E308*(I MOD 2*2-1):NEXT:PCMSTREAM W",
    )
    .unwrap();
    frames(&mut rt, 5);
    let during = rt.sound.take_samples();
    assert!(during.iter().all(|s| s.is_finite() && s.abs() <= 1.0));
    run_on(&mut rt, "PCMSTOP").unwrap();
    // The tail goes on, and dies away, rather than being stuck for good.
    for _ in 0..40 {
        frames(&mut rt, 60);
        let after = rt.sound.take_samples();
        assert!(after.iter().all(|s| s.is_finite()));
    }
    frames(&mut rt, 1);
    assert!(peak(&mut rt) < 0.01);
}
