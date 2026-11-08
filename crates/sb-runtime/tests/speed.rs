//! Programs run at a console's speed: as much work a frame as on the
//! console, with frames missed when a program needs more.

use sb_runtime::Runtime;
use sb_runtime::confirm::FileConfirm;
use sb_runtime::font::Font;
use sb_runtime::machine::Machine;
use sb_runtime::speed::Speed;
use sb_runtime::storage::Storage;

/// A busy loop's passes in three frames, from the start of one.
const BUSY: &str = "VSYNC:T=MAINCNT():WHILE MAINCNT()-T<3:INC N:WEND:?N\n";

/// An empty FOR's time for 10000 passes.
const TIMED: &str = "PERFBEGIN 0:FOR I=1 TO 10000:NEXT:?PERFEND(0)\n";

/// Runs `source` at `speed` until it ends, and gives the last line it
/// printed.
fn run(speed: Speed, source: &str) -> String {
    run_on(speed, Runtime::new(400, 240, Font::blank()), &[(0, source)])
}

/// Runs slot 0 of `slots` on `rt` at `speed`, with the other slots
/// filled, and gives the last line printed.
fn run_on(speed: Speed, rt: Runtime, slots: &[(usize, &str)]) -> String {
    let mut m = Machine::new(rt);
    m.boot();
    m.rt.settings.speed = speed;
    for &(slot, source) in slots {
        m.set_slot(slot, source.to_string());
    }
    m.run_slot(0);
    for _ in 0..600 {
        m.frame();
        if !m.is_running() {
            break;
        }
    }
    assert!(!m.is_running(), "{slots:?} still runs");
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

/// Host commands count toward the frame as the VM's work does: 100 GCLS
/// of a whole page, 0.4 s on a Switch 1, take 24 frames of MAINCNT, not
/// none.
#[test]
fn host_commands_use_up_frames() {
    let source = "VSYNC:T=MAINCNT():PERFBEGIN 0\n\
                  FOR I=1 TO 100:GCLS:NEXT\n\
                  ?MAINCNT()-T;\" \";PERFEND(0)\n";
    let line = run(Speed::Switch1, source);
    let (frames, us) = line.split_once(' ').unwrap();
    let (frames, us): (u64, u64) = (frames.parse().unwrap(), us.trim().parse().unwrap());
    assert_eq!(frames, us / 16_667, "{line}");
    assert!(frames >= 2, "{line}");
}

/// GPAINT costs the pixels it fills, not the whole GCLIP rectangle.
#[test]
fn a_small_fill_costs_less_than_a_large_one() {
    let fill = |size: u32| {
        format!(
            "GCLS:GBOX 0,0,{e},{e},#C_RED:PERFBEGIN 0\n\
             GPAINT 1,1,#C_WHITE,#C_RED:?PERFEND(0)\n",
            e = size + 1
        )
    };
    let small = passes(Speed::Switch1, &fill(16));
    let large = passes(Speed::Switch1, &fill(256));
    // A fill of the whole page would cost 68 ms.
    assert!(
        small * 5.0 < large && large < 5_000.0,
        "{small} us and {large} us"
    );
}

/// SPSET's search for a free sprite costs each sprite it passes.
#[test]
fn a_long_search_for_a_free_sprite_costs_more() {
    let search = |used: u32| {
        format!(
            "FOR I=0 TO {used}:SPSET I,0:NEXT:PERFBEGIN 0\n\
             S=SPSET(0,4095,0):?PERFEND(0)\n"
        )
    };
    let short = passes(Speed::Switch1, &search(0));
    let long = passes(Speed::Switch1, &search(3000));
    assert!(short < 5.0 && long > 10.0, "{short} us and {long} us");
}

/// SPCLR of a range pays for each sprite in it, in use or not: 4096 free
/// ones took 1.50 ms on a Switch 1 (TEST69).
#[test]
fn spclr_of_a_range_costs_each_sprite() {
    let source = "PERFBEGIN 0:SPCLR 0,4095:T=PERFEND(0)\n\
                  PERFBEGIN 0:SPCLR 5:?T;\" \";PERFEND(0)\n";
    let (range, one) = pair(&run(Speed::Switch1, source));
    assert!((1_400.0..1_600.0).contains(&range), "{range} us");
    assert!(one < 5.0, "{one} us");
}

/// Two numbers a program printed on one line.
fn pair(line: &str) -> (f64, f64) {
    let mut words = line.split_whitespace().map(|w| w.parse().unwrap());
    let pair = (words.next(), words.next());
    pair.0.zip(pair.1).unwrap_or_else(|| panic!("{line:?}"))
}

/// MILLISEC keeps the console's clock, as PERFEND does, not the
/// computer's.
#[test]
fn millisec_follows_the_consoles_clock() {
    let source = "VSYNC:M=MILLISEC():PERFBEGIN 0\n\
                  FOR I=1 TO 100:GCLS:NEXT\n\
                  ?MILLISEC()-M;\" \";PERFEND(0)\n";
    let (ms, us) = pair(&run(Speed::Switch1, source));
    assert!(
        us > 100_000.0 && (ms - us / 1000.0).abs() <= 1.0,
        "{ms} ms, {us} us"
    );
}

/// EXEC costs a base and the characters it compiles, code more than
/// comments and strings: on a Switch 2 about 0.4 ms, and 2.3 ms and
/// 0.9 ms for 10000 characters.
#[test]
fn exec_costs_the_source_it_compiles() {
    let exec = |source: &str| {
        let rt = Runtime::new(400, 240, Font::blank());
        let slot1 = format!("{source}?PERFEND(0)\n");
        let line = run_on(
            Speed::Switch2,
            rt,
            &[(0, "PERFBEGIN 0:EXEC 1\n"), (1, &slot1)],
        );
        line.trim()
            .parse::<f64>()
            .unwrap_or_else(|_| panic!("{line:?}"))
    };
    let base = exec("");
    let code = exec(&"A=1\n".repeat(2_500)) - base;
    let comment = exec(&format!("'{}\n", "X".repeat(9_998))) - base;
    assert!((350.0..450.0).contains(&base), "{base} us");
    assert!((2_200.0..2_500.0).contains(&code), "{code} us");
    assert!((800.0..1_000.0).contains(&comment), "{comment} us");
}

/// LOADG costs a base, each pixel of the image and each byte of its
/// file: TEST68's 1024x624 font took 10.4 ms on a Switch 2. One that
/// loads nothing costs little.
#[test]
fn loadg_costs_the_pixels_it_decodes() {
    let dir = std::env::temp_dir().join(format!("sb-speed-loadg-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let load = |w: u32, h: u32| {
        let mut rt = Runtime::new(400, 240, Font::blank());
        rt.storage = Storage::new(dir.clone());
        rt.settings.file_confirm = FileConfirm::Silent;
        let source = match w {
            0 => "PERFBEGIN 0:LOADG \"NONE\",1:?PERFEND(0)\n".to_string(),
            _ => format!(
                "SAVEG \"PIC\",0,0,0,{w},{h}\n\
                 PERFBEGIN 0:LOADG \"PIC\",1:?PERFEND(0)\n"
            ),
        };
        let line = run_on(Speed::Switch2, rt, &[(0, &source)]);
        line.trim()
            .parse::<f64>()
            .unwrap_or_else(|_| panic!("{line:?}"))
    };
    let (none, small, large) = (load(0, 0), load(8, 8), load(1024, 624));
    let _ = std::fs::remove_dir_all(&dir);
    assert!(none < 10.0, "{none} us");
    assert!((300.0..450.0).contains(&small), "{small} us");
    assert!((9_500.0..11_000.0).contains(&large), "{large} us");
}

/// PRINT costs the lines it scrolls the screen by: 20 lines of a 50x30
/// console on top of 1000 characters.
#[test]
fn printing_that_scrolls_costs_more() {
    let source = "A$=\"X\"*1000:LOCATE 0,0:PERFBEGIN 0:PRINT A$;:T=PERFEND(0)\n\
                  LOCATE 0,29:PERFBEGIN 0:PRINT A$;:U=PERFEND(0):?T;\" \";U\n";
    let (still, scrolled) = pair(&run(Speed::Switch2, source));
    assert!((15.0..30.0).contains(&still), "{still} us");
    assert!(
        (60.0..90.0).contains(&(scrolled - still)),
        "{still} us, {scrolled} us"
    );
}
