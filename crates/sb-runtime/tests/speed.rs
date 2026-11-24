//! Programs run at a console's speed: as much work per frame as on the
//! console, with frames missed when a program needs more.

use sb_runtime::Runtime;
use sb_runtime::confirm::FileConfirm;
use sb_runtime::font::Font;
use sb_runtime::machine::Machine;
use sb_runtime::speed::Speed;
use sb_runtime::storage::Storage;

/// The passes of a busy loop in three frames, from the start of a frame.
const BUSY: &str = "VSYNC:T=MAINCNT():WHILE MAINCNT()-T<3:INC N:WEND:?N\n";

/// An empty FOR's time for 10000 passes.
const TIMED: &str = "PERFBEGIN 0:FOR I=1 TO 10000:NEXT:?PERFEND(0)\n";

/// Runs `source` at `speed` until it ends, and gives the last line it
/// printed.
fn run(speed: Speed, source: &str) -> String {
    run_on(speed, Runtime::for_tests(Font::blank()), &[(0, source)])
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

/// `WHILE MAINCNT()-T<60:INC N:WEND` repeated 2338972 times on a Switch 1
/// and 6317043 times on a Switch 2 (TEST65, 4.4.9). The costs are fitted
/// to many kinds of work at the same time, so one loop comes within a few
/// percent: 3 on a Switch 1, 7 on a Switch 2.
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

/// Unlimited runs a million instructions per frame without a time limit,
/// as it did before there was a choice.
#[test]
fn unlimited_runs_as_before() {
    assert_eq!(passes(Speed::Unlimited, BUSY), 214_286.0);
}

/// A frame's work that takes 1.5 frames on the console makes a VSYNC loop
/// lose a frame in two, the same as SmileBASIC 4 does.
#[test]
fn a_slow_frame_misses_vsyncs() {
    let source = |work: u32| {
        format!(
            "VSYNC:T=MAINCNT()\n\
             FOR F=1 TO 20:FOR I=1 TO {work}:NEXT:VSYNC:NEXT\n\
             ?MAINCNT()-T\n"
        )
    };
    // The passes of the FOR in a frame, computed from the time 10000 take.
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

/// Host commands count toward the frame the same as the VM's work does:
/// 100 GCLS of a whole page, 0.4 s on a Switch 1, take 24 frames of
/// MAINCNT, not none.
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

/// A GFILL whose left edge is off a 16-pixel boundary costs more on each
/// row, less on a 4-pixel one, as TEST70 and TEST71 find on a Switch 1.
/// Its row costs nothing, but one of the last 64 places costs less than
/// a new one.
#[test]
fn drawing_off_a_16_pixel_boundary_costs_more() {
    let fill = |x: u32, y: &str| {
        format!("PERFBEGIN 0:FOR I=0 TO 99:GFILL {x},{y},{x}+31,{y}+31,-1:NEXT:?PERFEND(0)\n")
    };
    let at = |x, y| passes(Speed::Switch1, &fill(x, y));
    let (aligned, on4, off) = (at(16, "I"), at(20, "I"), at(17, "I"));
    assert!(
        off > aligned * 2.0 && aligned < on4 && on4 < off,
        "{aligned} us, {on4} us, {off} us"
    );
    assert_eq!(at(16, "0"), aligned);
    let same = at(17, "0");
    assert!(aligned < same && same < off, "{same} us, {off} us");
}

/// Of the columns 2 pixels off a 4-pixel boundary, a 32-pixel GFILL at
/// x 22 costs the most on a Switch 1 and one at x 26 the least, as TEST72
/// finds: 8.3, 5.2 and 4.1 microseconds at x 22, 18 and 26.
#[test]
fn the_column_of_a_fills_edge_sets_its_cost() {
    let fill =
        |x: u32| format!("PERFBEGIN 0:FOR I=0 TO 99:GFILL {x},0,{x}+31,31,-1:NEXT:?PERFEND(0)\n");
    let at = |x| passes(Speed::Switch1, &fill(x));
    let (x18, x22, x26) = (at(18), at(22), at(26));
    assert!(x26 < x18 && x18 < x22, "{x18} us, {x22} us, {x26} us");
    assert!((780.0..880.0).contains(&x22), "{x22} us");
}

/// Changing 64 sprites in turn costs the same as changing one; changing
/// 1024 in turn costs more on a Switch 1 (TEST69, TEST70).
#[test]
fn changing_many_sprites_in_turn_costs_more() {
    let spofs = |mask: u32| {
        format!(
            "FOR I=0 TO 1023:SPSET I,0:NEXT:PERFBEGIN 0\n\
             FOR I=0 TO 9999:SPOFS I AND {mask},I,0:NEXT:?PERFEND(0)\n"
        )
    };
    let n = |mask| passes(Speed::Switch1, &spofs(mask));
    let (one, few, many) = (n(0), n(63), n(1023));
    assert!((few - one).abs() < 20.0, "{one} us, {few} us");
    assert!(many > one + 1_000.0, "{one} us, {many} us");
}

/// Two numbers a program printed on one line.
fn pair(line: &str) -> (f64, f64) {
    let mut words = line.split_whitespace().map(|w| w.parse().unwrap());
    let pair = (words.next(), words.next());
    pair.0.zip(pair.1).unwrap_or_else(|| panic!("{line:?}"))
}

/// MILLISEC keeps the console's clock, the same as PERFEND does, not the
/// clock of the computer.
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

/// EXEC of a file costs reading it and compiling it, code more than
/// comments and strings: on a Switch 2 about 0.4 ms, and 2.3 ms and 1.0
/// ms for 10000 characters (TEST69, TEST76, a file EXEC has read before).
#[test]
fn exec_costs_the_source_it_compiles() {
    let dir = std::env::temp_dir().join(format!("sb-speed-exec-{}", std::process::id()));
    let exec = |source: &str| {
        let _ = std::fs::remove_dir_all(&dir);
        let mut rt = Runtime::for_tests(Font::blank());
        rt.storage = Storage::new(dir.clone());
        rt.settings.file_confirm = FileConfirm::Silent;
        let slot1 = format!("{source}?PERFEND(0)\n");
        let slot0 = "SAVE \"P\",1:PERFBEGIN 0:EXEC \"P\",1:PERFBEGIN 0:EXEC \"P\",1\n";
        let line = run_on(Speed::Switch2, rt, &[(0, slot0), (1, &slot1)]);
        line.trim()
            .parse::<f64>()
            .unwrap_or_else(|_| panic!("{line:?}"))
    };
    let base = exec("");
    let code = exec(&"A=1\n".repeat(2_500)) - base;
    let comment = exec(&format!("'{}\n", "X".repeat(9_998))) - base;
    let _ = std::fs::remove_dir_all(&dir);
    assert!((350.0..450.0).contains(&base), "{base} us");
    assert!((2_200.0..2_500.0).contains(&code), "{code} us");
    assert!((900.0..1_150.0).contains(&comment), "{comment} us");
}

/// EXEC of the program already in a slot only compiles it: TEST76's EXEC
/// 3 of 16000 characters of code after LOAD took 2.6 ms on a Switch 2,
/// and EXEC of the file 4.1 ms.
#[test]
fn exec_of_a_slot_costs_less_than_of_a_file() {
    let rt = Runtime::for_tests(Font::blank());
    let slot1 = format!("{}?PERFEND(0)\n", "A=1\n".repeat(2_500));
    let line = run_on(
        Speed::Switch2,
        rt,
        &[(0, "PERFBEGIN 0:EXEC 1\n"), (1, &slot1)],
    );
    let us: f64 = line.trim().parse().unwrap_or_else(|_| panic!("{line:?}"));
    assert!((1_500.0..1_850.0).contains(&us), "{us} us");
}

/// GPUTCHR of user-defined characters costs by drawing mode: one of 16
/// pixels took 4.1, 12.2 and 16.0 us in modes 1, 2 and 3 on a Switch 1
/// (TEST73). SmileBASIC 4's own characters in mode 1 cost as before.
#[test]
fn gputchr_of_user_characters_costs_by_mode() {
    let put = |text: &str, mode: u32| {
        format!(
            "PERFBEGIN 0:FOR I=1 TO 100:GPUTCHR 16,16,{text},16,1,1,-1,{mode}:NEXT\n\
             ?PERFEND(0)/100\n"
        )
    };
    let at = |text, mode| passes(Speed::Switch1, &put(text, mode));
    let user = "CHR$(&HE800)";
    let (m1, m2, m3) = (at(user, 1), at(user, 2), at(user, 3));
    assert!((3.5..4.6).contains(&m1), "{m1} us");
    assert!((11.0..13.5).contains(&m2), "{m2} us");
    assert!((14.5..17.5).contains(&m3), "{m3} us");
    // TEST71's GPUTCHR of "A", 16 pixels, in mode 0.
    let own = at("\"A\"", 0);
    assert!((6.0..7.2).contains(&own), "{own} us");
}

/// GCOPY with a color code costs more per pixel than without, and more in
/// the blending modes: 16x16 took 5.97 us in mode 1 and 15.2 us in mode
/// 3 on a Switch 1 (TEST73). GPUTCHRP's width costs its call.
#[test]
fn gcopy_with_a_color_costs_by_mode() {
    let copy = |tail: &str| {
        format!(
            "PERFBEGIN 0:FOR I=1 TO 100:GCOPY 0,0,0,15,15,16,16{tail}:NEXT\n\
             ?PERFEND(0)/100\n"
        )
    };
    let at = |tail| passes(Speed::Switch1, &copy(tail));
    let (plain, m1, m3) = (at(",1"), at(",-1,1"), at(",-1,3"));
    assert!(plain < 2.0, "{plain} us");
    assert!((5.4..6.6).contains(&m1), "{m1} us");
    assert!((13.7..16.7).contains(&m3), "{m3} us");
    let width = "PERFBEGIN 0:FOR I=1 TO 127:W=GPUTCHRP(CHR$(I),8):NEXT:?PERFEND(0)\n";
    let us = passes(Speed::Switch1, width);
    assert!(us < 150.0, "{us} us");
}

/// The first LOADG of a file costs more than the next ones, because the
/// console reads a file faster once it has read it (TEST69 to TEST71).
#[test]
fn a_file_read_the_first_time_costs_more() {
    let dir = std::env::temp_dir().join(format!("sb-speed-first-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let mut rt = Runtime::for_tests(Font::blank());
    rt.storage = Storage::new(dir.clone());
    rt.settings.file_confirm = FileConfirm::Silent;
    let source = "SAVEG \"PIC\",0,0,0,64,64\n\
        PERFBEGIN 0:LOADG \"PIC\",1:A=PERFEND(0)\n\
        PERFBEGIN 0:LOADG \"PIC\",1:?A;\" \";PERFEND(0)\n";
    let line = run_on(Speed::Switch1, rt, &[(0, source)]);
    let _ = std::fs::remove_dir_all(&dir);
    let (first, next) = pair(&line);
    assert!(first > next + 500.0, "{first} us, {next} us");
}

/// LOADG costs a base, each pixel of the image and each byte of its
/// file: TEST68's 1024x624 font took 10.4 ms on a Switch 2. One that
/// loads nothing costs little.
#[test]
fn loadg_costs_the_pixels_it_decodes() {
    let dir = std::env::temp_dir().join(format!("sb-speed-loadg-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let load = |w: u32, h: u32| {
        let mut rt = Runtime::for_tests(Font::blank());
        rt.storage = Storage::new(dir.clone());
        rt.settings.file_confirm = FileConfirm::Silent;
        let source = match w {
            0 => "PERFBEGIN 0:LOADG \"NONE\",1:?PERFEND(0)\n".to_string(),
            _ => format!(
                "SAVEG \"PIC\",0,0,0,{w},{h}:LOADG \"PIC\",1\n\
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

/// PRINT costs the lines it scrolls the screen by: 19 lines of a 50x30
/// console took 56 us on a Switch 2 (TEST69) on top of 1000 characters.
#[test]
fn printing_that_scrolls_costs_more() {
    let source = "A$=\"X\"*1000:LOCATE 0,0:PERFBEGIN 0:PRINT A$;:T=PERFEND(0)\n\
                  LOCATE 0,29:PERFBEGIN 0:PRINT A$;:U=PERFEND(0):?T;\" \";U\n";
    let (still, scrolled) = pair(&run(Speed::Switch2, source));
    assert!((15.0..30.0).contains(&still), "{still} us");
    assert!(
        (45.0..70.0).contains(&(scrolled - still)),
        "{still} us, {scrolled} us"
    );
}

/// A statement costs a little more inside a DEF than at the top level: a
/// FOR's pass 7 to 12 ns on both consoles (TEST76).
#[test]
fn a_for_in_a_def_costs_more() {
    let top = "PERFBEGIN 0:FOR I=1 TO 10000:NEXT:?PERFEND(0)\n";
    let def = "DEF L:VAR I:FOR I=1 TO 10000:NEXT:END\nPERFBEGIN 0:L:?PERFEND(0)\n";
    for (speed, more) in [(Speed::Switch1, 100.0), (Speed::Switch2, 110.0)] {
        let (top, def) = (passes(speed, top), passes(speed, def));
        let extra = def - top;
        assert!(
            (more * 0.8..more * 1.4).contains(&extra),
            "{speed:?}: {top} us, {def} us"
        );
    }
}

/// CHR$ of one of the codes the last four CHR$ made costs only its call,
/// and CHR$ of another code costs more. TEST76 times CHR$ of one to four
/// codes in turn at 163 to 177 ns on a Switch 1, while TPERF's VSTR, which
/// makes 16 codes in turn, and HGLIB's conversions need more.
#[test]
fn chr_of_a_new_code_costs_more() {
    let chr = |mask: u32| {
        format!("PERFBEGIN 0:FOR I=1 TO 1000:U$=CHR$(65+(I AND {mask})):NEXT:?PERFEND(0)\n")
    };
    let (same, four, new) = (
        passes(Speed::Switch1, &chr(0)),
        passes(Speed::Switch1, &chr(3)),
        passes(Speed::Switch1, &chr(15)),
    );
    // Except the first four, which are new.
    assert!((four - same).abs() < 3.0, "{same} us, {four} us");
    assert!(new > same + 300.0, "{same} us, {new} us");
}

/// LEN costs the same as reading a variable, like ABS. SIN of a number
/// past a quarter of pi costs more than SIN of a small one: TEST76's SIN
/// of 1 to 1000000.5 took 152 to 197 ns on a Switch 1, and of 0.5 110 ns.
#[test]
fn cheap_builtins_cost_no_call() {
    let time = |body: &str| {
        passes(
            Speed::Switch1,
            &format!(
                "S$=\"ABC\":X=-3:R#=0.5:B#=1000000.5\n\
                 PERFBEGIN 0:FOR I=1 TO 10000:{body}:NEXT:?PERFEND(0)\n"
            ),
        )
    };
    assert_eq!(time("A=LEN(S$)"), time("A=ABS(X)"));
    let (small, large) = (time("S#=SIN(R#)"), time("S#=SIN(B#)"));
    assert!(
        (600.0..800.0).contains(&(large - small)),
        "{small} us, {large} us"
    );
}

/// RGB of constants is a number when the program is compiled: TEST74's
/// A=RGB(1,2,3) costs less than A=FLOOR(R#).
#[test]
fn rgb_of_constants_costs_a_number() {
    let time = |body: &str| {
        passes(
            Speed::Switch1,
            &format!("R=1:PERFBEGIN 0:FOR I=1 TO 1000:{body}:NEXT:?PERFEND(0)\n"),
        )
    };
    assert_eq!(time("A=RGB(1,2,3)"), time("A=1"));
    assert!(time("A=RGB(R,2,3)") > time("A=1") + 100.0);
}

/// GLOAD of palette indices costs more per pixel than of colors: TEST74's
/// 400x216 took 0.24 ms on a Switch 1.
#[test]
fn gload_of_palette_indices_costs_more() {
    let load = |pal: &str| {
        format!(
            "DIM IMG%[400*216],PAL%[256]:PERFBEGIN 0\n\
             GLOAD 0,24,400,216,IMG%,{pal}0:?PERFEND(0)\n"
        )
    };
    let (colors, indices) = (
        passes(Speed::Switch1, &load("")),
        passes(Speed::Switch1, &load("PAL%,")),
    );
    assert!((220.0..260.0).contains(&indices), "{indices} us");
    assert!(colors < indices * 0.75, "{colors} us, {indices} us");
}

/// SPROT among commands for a few sprites costs less: TEST76's SPROT of
/// one, two or ten sprites in turn takes 285 ns each on a Switch 1, while
/// TEST70's sprite loop over a hundred sprites needs more.
#[test]
fn sprot_in_a_row_costs_less() {
    let rot = |other: &str| {
        format!(
            "FOR J=0 TO 99:SPSET J,0:NEXT:PERFBEGIN 0\n\
             FOR I=1 TO 1000:SPROT 0,I:SPROT {other},I:NEXT:?PERFEND(0)\n"
        )
    };
    let (row, two, many) = (
        passes(Speed::Switch1, &rot("0")),
        passes(Speed::Switch1, &rot("1")),
        passes(Speed::Switch1, &rot("I MOD 100")),
    );
    // Except the first, while the SPSETs of a hundred sprites are among the
    // last commands.
    assert!((two - row).abs() < 3.0, "{row} us, {two} us");
    assert!(row + 400.0 < many, "{row} us, {many} us");
}
