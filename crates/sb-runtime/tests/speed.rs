//! Programs run at a console's speed: as much work a frame as on the
//! console, with frames missed when a program needs more.

use sb_runtime::Runtime;
use sb_runtime::font::Font;
use sb_runtime::machine::Machine;
use sb_runtime::speed::Speed;

/// A busy loop's passes in three frames, from the start of one.
const BUSY: &str = "VSYNC:T=MAINCNT():WHILE MAINCNT()-T<3:INC N:WEND:?N\n";

/// An empty FOR's time for 10000 passes.
const TIMED: &str = "PERFBEGIN 0:FOR I=1 TO 10000:NEXT:?PERFEND(0)\n";

/// Runs `source` at `speed` until it ends, and gives the last line it
/// printed.
fn run(speed: Speed, source: &str) -> String {
    let mut m = Machine::new(Runtime::new(400, 240, Font::blank()));
    m.boot();
    m.rt.settings.speed = speed;
    m.set_slot(0, source.to_string());
    m.run_slot(0);
    for _ in 0..600 {
        m.frame();
        if !m.is_running() {
            break;
        }
    }
    assert!(!m.is_running(), "{source} still runs");
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
        .rfind(|r| !r.is_empty() && r != "OK")
        .unwrap_or_default()
}

fn passes(speed: Speed, source: &str) -> f64 {
    let line = run(speed, source);
    line.trim().parse().unwrap_or_else(|_| panic!("{line:?}"))
}

/// `WHILE MAINCNT()-T<60:INC N:WEND` went round 2338972 times on a
/// Switch 1 and 6317043 times on a Switch 2 (TEST65, 4.4.9). The costs
/// are fitted to many kinds of work at once, so one loop comes within a
/// few percent: 3 on a Switch 1, 7 on a Switch 2.
#[test]
fn a_busy_loop_goes_round_as_often_as_on_the_console() {
    let consoles = [
        (Speed::Switch1, 2_338_972.0, 0.05),
        (Speed::Switch2, 6_317_043.0, 0.1),
    ];
    for (speed, in_60_frames, within) in consoles {
        let expected = in_60_frames / 20.0;
        let got = passes(speed, BUSY);
        assert!(
            (got / expected - 1.0).abs() < within,
            "{speed:?}: {got} passes, the console {expected}"
        );
    }
}

/// Unlimited runs a million instructions a frame without a time limit,
/// as before there was a choice.
#[test]
fn unlimited_runs_as_before() {
    assert_eq!(passes(Speed::Unlimited, BUSY), 214_286.0);
}

/// A frame's work that takes 1.5 frames on the console makes a VSYNC loop
/// lose a frame in two, as SmileBASIC 4 does.
#[test]
fn a_slow_frame_misses_vsyncs() {
    let source = |work: u32| {
        format!(
            "VSYNC:T=MAINCNT()\n\
             FOR F=1 TO 20:FOR I=1 TO {work}:NEXT:VSYNC:NEXT\n\
             ?MAINCNT()-T\n"
        )
    };
    // The FOR's passes in a frame, from what 10000 take.
    let per_frame = 16_667.0 * 10_000.0 / passes(Speed::Switch1, TIMED);
    let fits = passes(Speed::Switch1, &source((per_frame * 0.5) as u32));
    assert_eq!(fits, 20.0);
    let slow = passes(Speed::Switch1, &source((per_frame * 1.5) as u32));
    assert!((29.0..=31.0).contains(&slow), "{slow} frames");
}

/// PERFEND times the console's clock, the same on every run.
#[test]
fn perfend_gives_the_consoles_time() {
    let us = passes(Speed::Switch1, TIMED);
    assert_eq!(us, passes(Speed::Switch1, TIMED));
    // FOR...NEXT's pass is a few instructions and a statement.
    assert!((500.0..3000.0).contains(&us), "{us} us");
    assert!(passes(Speed::Switch2, TIMED) < us);
}
