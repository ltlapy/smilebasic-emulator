//! Running programs about as fast as a real Switch does, for checking that
//! a program written on a fast computer keeps up on the console.
//!
//! A [`Meter`] charges the work a program does in picoseconds of the
//! console's time: the VM's counts from [`Work`], each host command by
//! what it is given (pixels for drawing, cells for text) and each
//! character printed. A frame gives the program one sixtieth of a second
//! of that time. A program that needs more goes on into the next frame and
//! misses the frame's `VSYNC` as it would on the console, so `MAINCNT`,
//! `VSYNC` and the frames a program skips come out as there. `PERFBEGIN`,
//! `PERFEND` and `MILLISEC` measure the same time.
//!
//! The costs come from TPERF's scenes and the loops of TEST65, TEST67,
//! TEST69, TEST70 and TEST71, timed on both consoles running SmileBASIC
//! 4.4.9, and from TEST68 on a Switch 2 (docs/benchmarking.md). TEST70
//! finds a Switch 2 as fast in the hand as docked.
//!
//! A `GFILL` or `GCOPY` whose left edge is not on a 16-pixel boundary
//! costs more on each row, by its width, and less on a 4-pixel one
//! (TEST69 to TEST71, most on a Switch 1). A misaligned or blended
//! `GFILL` at a place not among the last 64 such places costs more
//! again. Changing sprites in turn costs more once there are more of them
//! than 64, so the meter remembers the last 64 sprites changed. The first
//! read of each file costs more than the next ones. UNVERIFIED: that
//! these are the console's memory and storage at work, and where between
//! 64 and 1000 sprites the sprites' cost starts.
//! Drawing sprites, layers and text screens happens in the frame job, which
//! TPERF and TEST67 show costs a program nothing, so only the commands are
//! charged.
//!
//! A subprogram's interpreter gets a frame's time of its own.

use std::cell::Cell;

use sb_core::value::Value;
use sb_core::vm::{COSTLY, Work};

use crate::runtime::{Command, Runtime};
use crate::sprites::SPRITES;

/// How fast programs run: as fast as the computer allows, or about as fast
/// as on a console.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Speed {
    #[default]
    Unlimited,
    Switch2,
    Switch1,
}

impl Speed {
    pub const ALL: [Speed; 3] = [Speed::Unlimited, Speed::Switch2, Speed::Switch1];

    /// The name in the settings file and on the command line.
    pub fn name(self) -> &'static str {
        match self {
            Speed::Unlimited => "unlimited",
            Speed::Switch2 => "switch2",
            Speed::Switch1 => "switch1",
        }
    }

    pub fn from_name(name: &str) -> Option<Speed> {
        Speed::ALL.into_iter().find(|s| s.name() == name)
    }

    /// The name people read.
    pub fn label(self) -> &'static str {
        match self {
            Speed::Unlimited => "Unlimited",
            Speed::Switch2 => "Switch 2",
            Speed::Switch1 => "Switch 1",
        }
    }

    fn costs(self) -> Option<&'static Costs> {
        match self {
            Speed::Unlimited => None,
            Speed::Switch2 => Some(&SWITCH2),
            Speed::Switch1 => Some(&SWITCH1),
        }
    }
}

/// A frame of the console's time, in picoseconds.
pub const FRAME_PS: u64 = 1_000_000_000_000 / 60;

/// The most instructions run between looks at what is left of a frame, so
/// that a slice full of costly commands cannot run far past its end.
const SLICE: u64 = 20_000;

/// What work costs on one console, in picoseconds.
#[derive(Debug)]
pub(crate) struct Costs {
    /// Each VM instruction, and each statement on top.
    instruction: u64,
    statement: u64,
    /// Taken off each test of a FOR: a NEXT is many of our instructions
    /// but one step of SmileBASIC 4's.
    for_credit: u64,
    def_call: u64,
    /// Each builtin call and each argument it is given. ON's jump counts
    /// as a call with none.
    builtin: u64,
    builtin_arg: u64,
    /// Each array element read or written, and each index past its first.
    element: u64,
    extra_index: u64,
    /// A character of a string read or written by index, on top of the
    /// element.
    str_element: u64,
    /// An `IF` or `ELSEIF` whose condition is an array element: TEST70's
    /// `IF MB%[ASC(S$[I%])] THEN BREAK` costs that much more than
    /// `C%=MB%[ASC(S$[I%])]`, where TEST65's `IF V>K THEN` costs what
    /// `V=A+B` does and TEST69's HGIMEUPDATE, which tests variables,
    /// `!` and `KEYBOARD()`, needs nothing on top.
    element_test: u64,
    /// Each element `FILL` writes.
    fill_element: u64,
    lookup: u64,
    join: u64,
    /// `CHR$`, on top of the call, for TEST68's and TEST69's HGCNV$ and
    /// HGREV$, which make 488 characters with it in 1000. UNVERIFIED:
    /// that it is making a string that costs, which keeps TPERF's VSTR
    /// where it was, where TEST71's `U$=CHR$(C)` of an ASCII code costs
    /// no more than a call.
    chr: u64,
    /// Each builtin of [`COSTLY`] on top of its call, in that order.
    costly: [u64; COSTLY.len()],
    /// Each character of the string a join makes.
    join_char: u64,
    /// A comparison of strings, or another operator but `+` with a
    /// string on its left.
    str_compare: u64,
    /// `MAINCNT` and `MILLISEC`.
    clock: u64,
    /// Host functions, such as `BUTTON` and `SPCHK`, and `RND`.
    query: u64,
    rnd: u64,
    /// Host commands not timed, and those that set one thing, as
    /// `SPROT`, `SPCOLOR` and `LOCATE` do.
    setter: u64,
    /// Commands TEST71 timed alone: `LOCATE`, `SPSHOW`, `SPVAR`, `SPCHR`,
    /// `STICK` and `SPOFS` with `OUT`, and `GPSET` and `GPGET`.
    locate: u64,
    spshow: u64,
    spvar: u64,
    spchr: u64,
    stick: u64,
    spofs_out: u64,
    pixel: u64,
    /// `CHKCHR` and `BGMCHK`, which TEST71 times at less than `BUTTON`.
    peek: u64,
    spofs: u64,
    spscale: u64,
    /// `SPSET`: the call and each argument, a sprite not in use on top,
    /// and the search of the function form with each sprite it passes.
    spset: u64,
    spset_arg: u64,
    spset_new: u64,
    spset_search: u64,
    spset_scan: u64,
    /// `SPCLR` of one sprite, and of a range of 1, 100, 500, 1000, 2000
    /// and 4096 sprites, in use or not, with straight lines between.
    spclr: u64,
    spclr_range: [u64; 6],
    sphitsp: u64,
    spanim: u64,
    /// Each thing `PRINT` prints, and each character of it.
    print: u64,
    print_char: u64,
    /// Each time `PRINT` scrolls the screen, and each cell it moves.
    print_scroll: u64,
    print_scroll_cell: u64,
    tput: u64,
    tfill: u64,
    tfill_cell: u64,
    scroll_cell: u64,
    /// Every GRP command but `GTRI` and `GCOPY`, before its rows and
    /// pixels, and `GCOPY`'s. `GFILL`, `GCLS` and `GCOPY` also pay each row.
    grp: u64,
    copy: u64,
    row: u64,
    /// `GCLS` before its rows and pixels, and `GCLIP` with arguments and
    /// without.
    gcls: u64,
    clip: [u64; 2],
    /// Pixels of `GFILL` and `GCLS`, of `GCOPY` and of blended drawing.
    /// TEST69 finds the same rates with `GCLIP` set smaller than the
    /// page.
    fill_px: u64,
    copy_px: u64,
    /// A pixel of a `GCOPY` that leaves out the transparent ones.
    copy_key_px: u64,
    alpha_px: u64,
    copy_alpha_px: u64,
    /// `GTRI`: the call, each row and each pixel.
    tri: u64,
    tri_row: u64,
    tri_px: u64,
    /// `GLINE` and `GBOX`, and `GCIRCLE`'s outline, before their pixels.
    line: u64,
    line_px: u64,
    circle: u64,
    circle_px: u64,
    load_px: u64,
    /// `LOADG` of an image, each pixel it decodes and each byte of its
    /// file, which a busy image has more of.
    loadg: u64,
    loadg_px: u64,
    loadg_byte: u64,
    /// A pixel of `GPUTCHR` in the 8-pixel font, and in the 16-pixel one
    /// (TPERF's 8 characters of 8 pixels, TEST71's one of 16).
    char_px: u64,
    char16_px: u64,
    /// `GPAINT`, and each pixel it fills.
    paint: u64,
    paint_px: u64,
    /// Each row of a `GFILL` whose left edge is not on a 16-pixel
    /// boundary, 8, 16, 32, 64 and 128 pixels wide, with straight lines
    /// between, in proportion below 8 and the same past 128: first for
    /// an edge off a 4-pixel boundary, then for one on it. Then the share
    /// of it a blended `GFILL` and a `GCOPY` pay, in percent, `GCOPY` by
    /// where it draws. UNVERIFIED: the widths `GCOPY` and blending were
    /// not timed at, other than 32, and an edge 2 pixels off a 4-pixel
    /// boundary, which TEST69's and TEST70's mixes of places only
    /// suggest costs what an odd one does.
    place: [[u64; 5]; 2],
    place_alpha_pct: u64,
    place_copy_pct: u64,
    /// On top, at a place not among the last 64 a misaligned or blended
    /// `GFILL` drew at: each row of a misaligned one and each pixel of a
    /// blended one. TPERF's fills at random places cost that much more
    /// than fills at one place or at 10 or 64 places in turn (TEST69 to
    /// TEST71). UNVERIFIED: that it is the console's memory cache, and
    /// that it holds 64 places: TEST71's 100 fills a call, 37 places on
    /// from the call before, cost as much again as TPERF's.
    cold_row: u64,
    cold_alpha_px: u64,
    /// A sprite command for a sprite not among the last 64 changed.
    /// TEST70 finds SPOFS of 1, 10 and 64 sprites in turn costs the
    /// same, TEST69 SPOFS over 1000 sprites in order more.
    far_sprite: u64,
    /// `LFILTER` and `LMATRIX`.
    layer: u64,
    /// `EXEC`: the call, the file's reading included, and each character
    /// it compiles, those of code apart from those of strings and
    /// comments.
    exec: u64,
    compile_char: u64,
    compile_text_char: u64,
    /// On top: the first `EXEC` of a file since the program started, and
    /// the first read by `EXEC` or `LOADG` of each file since the
    /// emulator started, and each of its bytes. TEST69 to TEST71: the
    /// console reads a file faster once it has read it.
    first_exec: u64,
    first_file: u64,
    first_byte: u64,
}

// From TPERF and TEST65 on a Switch 2 and a Switch 1, from TEST67 and
// TEST69, which time single commands at several sizes, and from TEST68's
// Hangul library on a Switch 2. The Switch 2 ran TEST67 to TEST69
// docked and TPERF handheld; the Switch 1 ran the others handheld.
const SWITCH2: Costs = Costs {
    instruction: 9_700,
    statement: 6_500,
    for_credit: 78_900,
    def_call: 153_100,
    builtin: 26_400,
    builtin_arg: 11_500,
    element: 17_400,
    extra_index: 43_000,
    // TEST69's and TEST70's rows of S$[I] and ASC, with ASC charged as
    // no call, and TEST70's IF..THEN rows and HGCNV$'s search loop,
    // within 10% but T$=S$[I] and C%=MS%[ASC(S$[I%])], 14% apart.
    str_element: 43_000,
    element_test: 73_000,
    // TEST69's FILL of 1000 and 100000 elements.
    fill_element: 914,
    lookup: 146_400,
    // TEST71's joins of two short strings and of one growing to 2000
    // characters. HGCNV$ and HGREV$ within 7%, and VSTR as before.
    join: 85_000,
    chr: 125_000,
    // TEST71's STR$, FORMAT$, VAL, INSTR, HEX$ and MID$, each alone.
    costly: [210_900, 136_700, 89_300, 83_400, 45_900, 67_200],
    join_char: 447,
    // What a join cost before TEST71: TEST69's HGIMEUPDATE, which tests
    // K$!="", within 7%. UNVERIFIED: TEST71's IF S$[I]==K$ costs no
    // more than a comparison of numbers.
    str_compare: 121_000,
    clock: 5_000,
    query: 51_700,
    rnd: 21_000,
    setter: 108_800,
    locate: 23_600,
    spshow: 57_700,
    spvar: 44_300,
    spchr: 87_300,
    stick: 67_100,
    spofs_out: 66_300,
    pixel: 51_000,
    peek: 40_500,
    spofs: 102_100,
    // TEST69's SPSCALE, which costs less than the other setters.
    spscale: 94_200,
    spset: 111_100,
    spset_arg: 28_400,
    spset_new: 65_900,
    spset_search: 60_500,
    spset_scan: 2_140,
    spclr: 116_700,
    // TEST70's SPCLR of 1, 100, 1000 and 4096 free sprites. TEST69's
    // SPCLR of 1000 sprites in use comes within 8%.
    spclr_range: [
        123_300,
        4_253_000,
        25_646_000,
        52_388_000,
        107_241_000,
        222_217_000,
    ],
    sphitsp: 612_300,
    spanim: 540_000,
    // TEST71's PRINT of one character.
    print: 67_000,
    print_char: 22_000,
    // TEST69's PRINT that scrolls a 50x30 console 19 times, 56 us, and
    // TEST68's and TEST69's that scroll a 25x15 one 25 times.
    print_scroll: 329_000,
    print_scroll_cell: 1_737,
    tput: 128_000,
    tfill: 156_000,
    tfill_cell: 18_480,
    scroll_cell: 2_500,
    // TEST67's GFILL of 8, 32 and 128 pixels.
    grp: 125_000,
    // TEST71's GCOPY 16 to 64 pixels wide at x 16; 128 wide costs 10%
    // more a pixel.
    copy: 180_000,
    row: 1_000,
    // TEST70's and TEST71's GCLS under a 100x100 GCLIP, and TEST71's
    // GCLIP alone.
    gcls: 1_525_000,
    clip: [6_700, 47_000],
    fill_px: 458,
    copy_px: 1_120,
    // TPERF's GCOPY of 48x48 at random places.
    copy_key_px: 1_350,
    alpha_px: 8_900,
    copy_alpha_px: 9_290,
    tri: 180_000,
    tri_row: 14_000,
    tri_px: 480,
    line: 250_000,
    line_px: 5_300,
    circle: 227_000,
    circle_px: 2_100,
    load_px: 780,
    // TEST68's and TEST69's LOADG and HGLOAD of fonts from 512x312 to
    // 2048x1376 and TEST69's of a 512x512 image of noise (13.9 ms, twice
    // an empty one): all within 10%. UNVERIFIED: TEST69's empty image,
    // which SmileBASIC 4 saved itself, took 6.9 ms, half again what the
    // size of the file the emulator saves for it gives.
    loadg: 319_000_000,
    loadg_px: 15_150,
    // TEST69's and TEST70's images of noise load as fast as each other on
    // the console, but the emulator saves them 901070 and 725652 bytes
    // long: the cost a byte between them leaves them 5% slow and 9% fast.
    loadg_byte: 11_500,
    char_px: 4_150,
    char16_px: 8_540,
    // TEST67's GPAINT of 128x128 and 512x512 and TEST69's of 4x4 and
    // 16x16, all within 7%.
    paint: 33_500_000,
    paint_px: 7_531,
    // TEST71's GFILL 8 to 128 pixels wide at x 16, 17, 20 and 24, and its
    // GCOPY 32 and 128 at x 17. TEST69's blended GFILL 32 costs no more.
    // TEST70's 128 wide at 10 places in turn costs 30% more.
    place: [
        [0, 3_850, 1_200, 740, 23_000],
        [0, 3_850, 1_550, 890, 14_000],
    ],
    place_alpha_pct: 0,
    place_copy_pct: 60,
    // TEST71's GFILL and blended GFILL at random places.
    cold_row: 10_100,
    cold_alpha_px: 100,
    // TEST69's SPOFS over 1000 sprites needs none.
    far_sprite: 0,
    layer: 8_000_000,
    // TEST69's EXEC of a 3-line file (0.24 and 0.52 ms) and of two of
    // 15343 characters, one of code (3.7 ms) and one of DATA strings
    // (1.8 ms), and TEST68's and TEST69's of HGLIB.PRG with its set-up
    // code: the costs that fit all five with the least relative error.
    // HGLIB's rows and the second and third EXEC of each file come
    // within 9%, but the 3-line file's, which differ by half on the
    // console. UNVERIFIED: what else in a file costs, as HGLIB lies 16%
    // above the line through the other three. The first EXEC of a file,
    // 2.3 ms for the 3-line one, is not charged apart.
    exec: 379_000_000,
    compile_char: 231_800,
    compile_text_char: 90_600,
    // TEST69's to TEST71's first EXEC and LOADG of each file, TEST70's
    // and TEST71's of 725652 bytes of noise as the emulator saves it.
    first_exec: 1_200_000_000,
    first_file: 300_000_000,
    first_byte: 9_250,
};

const SWITCH1: Costs = Costs {
    instruction: 28_000,
    statement: 6_000,
    for_credit: 245_600,
    def_call: 534_900,
    builtin: 28_300,
    builtin_arg: 55_000,
    element: 54_300,
    extra_index: 138_000,
    // As for the Switch 2.
    str_element: 38_000,
    element_test: 215_000,
    fill_element: 1_990,
    lookup: 318_800,
    // As for the Switch 2.
    join: 124_000,
    chr: 380_000,
    costly: [526_500, 291_700, 204_900, 197_600, 69_400, 123_300],
    join_char: 910,
    str_compare: 212_000,
    clock: 5_000,
    query: 131_200,
    rnd: 48_200,
    setter: 392_900,
    locate: 48_500,
    spshow: 160_200,
    spvar: 86_200,
    spchr: 194_000,
    stick: 179_300,
    spofs_out: 168_000,
    pixel: 165_000,
    peek: 90_000,
    // TEST67's and TEST69's SPOFS of one sprite.
    spofs: 306_000,
    spscale: 280_200,
    spset: 383_500,
    spset_arg: 57_000,
    spset_new: 261_000,
    spset_search: 248_000,
    spset_scan: 4_320,
    spclr: 300_600,
    // As for the Switch 2.
    spclr_range: [
        358_000,
        16_603_000,
        88_300_000,
        191_728_000,
        501_600_000,
        1_499_745_000,
    ],
    sphitsp: 1_373_700,
    spanim: 840_000,
    print: 158_000,
    print_char: 40_300,
    // TEST69's two PRINTs that scroll, as for the Switch 2.
    print_scroll: 752_000,
    print_scroll_cell: 4_076,
    tput: 440_000,
    tfill: 374_000,
    tfill_cell: 32_700,
    scroll_cell: 4_900,
    // As for the Switch 2.
    grp: 193_000,
    copy: 340_000,
    row: 23_000,
    gcls: 193_000,
    clip: [23_000, 153_000],
    fill_px: 955,
    copy_px: 2_100,
    copy_key_px: 2_100,
    alpha_px: 17_500,
    copy_alpha_px: 22_400,
    tri: 0,
    tri_row: 96_000,
    tri_px: 980,
    line: 500_000,
    line_px: 10_500,
    circle: 500_000,
    circle_px: 5_000,
    load_px: 1_860,
    // As for the Switch 2, from TEST69's rows.
    loadg: 1_142_000_000,
    loadg_px: 24_600,
    // As for the Switch 2.
    loadg_byte: 22_600,
    char_px: 15_760,
    char16_px: 24_360,
    paint: 93_500_000,
    paint_px: 16_110,
    // As for the Switch 2: 150 ns a row of 32 pixels off a 4-pixel
    // boundary, 61 ns on one. TEST69's blended GFILL pays 18% of it, and
    // its GCOPY at 10 places and TPERF's at random ones 60%, where
    // TEST71's at x 17 pays 138%.
    place: [
        [6_800, 0, 150_000, 235_000, 182_000],
        [1_500, 0, 61_500, 235_000, 182_000],
    ],
    place_alpha_pct: 18,
    place_copy_pct: 60,
    cold_row: 38_500,
    cold_alpha_px: 3_290,
    // TEST69's SPOFS over 1000 sprites in order, and SPSET and SPCLR.
    far_sprite: 150_000,
    layer: 5_000_000,
    // As for the Switch 2, from TEST69's files (1.0 and 1.8 ms, 10.3 ms
    // and 5.5 ms) and its EXEC of HGLIB.PRG.
    exec: 1_401_000_000,
    compile_char: 564_700,
    compile_text_char: 220_400,
    first_exec: 2_150_000_000,
    first_file: 800_000_000,
    first_byte: 30_500,
};

/// The console's time a program has used, kept by the runtime.
#[derive(Debug)]
pub struct Meter {
    pub(crate) speed: Speed,
    costs: &'static Costs,
    /// What host commands and printing have cost since the meter started.
    host: u64,
    /// The VM's counts, as last heard before a host call.
    work: Work,
    /// The counts and `host` when the frame's time began, and the clock
    /// then.
    base_work: Work,
    base_host: u64,
    start: u64,
    /// The clock at the end of the frame, and where the program's own
    /// time had got to when the last frame ended.
    frame_end: u64,
    busy_until: u64,
    /// The last sprites changed, each in the place its number's low six
    /// bits give.
    sprites_seen: [Cell<i16>; 64],
    /// The last places misaligned or blended drawing drew at, in turn,
    /// and where the next goes.
    places_seen: [Cell<u64>; 64],
    next_place: Cell<usize>,
    /// The files read since the meter started, and whether the program
    /// has run an `EXEC` of a file.
    files_read: std::collections::HashSet<String>,
    exec_run: bool,
}

impl Meter {
    pub fn new(speed: Speed) -> Option<Meter> {
        let costs = speed.costs()?;
        Some(Meter {
            speed,
            costs,
            host: 0,
            work: Work::default(),
            base_work: Work::default(),
            base_host: 0,
            start: 0,
            frame_end: FRAME_PS,
            busy_until: 0,
            sprites_seen: std::array::from_fn(|_| Cell::new(-1)),
            places_seen: std::array::from_fn(|_| Cell::new(u64::MAX)),
            next_place: Cell::new(0),
            files_read: std::collections::HashSet::new(),
            exec_run: false,
        })
    }

    /// What `work` costs.
    fn vm_cost(&self, work: &Work) -> u64 {
        let c = self.costs;
        let b = &self.base_work;
        let n = |now: u64, then: u64| now.saturating_sub(then);
        let cost = n(work.executed, b.executed) * c.instruction
            + n(work.statements, b.statements) * c.statement
            + n(work.def_calls, b.def_calls) * c.def_call
            + n(work.builtin_calls, b.builtin_calls) * c.builtin
            + n(work.builtin_args, b.builtin_args) * c.builtin_arg
            + n(work.elements, b.elements) * c.element
            + n(work.extra_indices, b.extra_indices) * c.extra_index
            + n(work.str_elements, b.str_elements) * c.str_element
            + n(work.filled, b.filled) * c.fill_element
            + n(work.lookups, b.lookups) * c.lookup
            + n(work.joins, b.joins) * c.join
            + n(work.element_tests, b.element_tests) * c.element_test
            + n(work.chr_calls, b.chr_calls) * c.chr
            + n(work.join_chars, b.join_chars) * c.join_char
            + n(work.str_compares, b.str_compares) * c.str_compare;
        let costly: u64 = (work.costly_calls.iter().zip(&b.costly_calls))
            .zip(&c.costly)
            .map(|((&now, &then), &each)| n(now, then) * each)
            .sum();
        (cost + costly).saturating_sub(n(work.for_tests, b.for_tests) * c.for_credit)
    }

    /// The console's clock in picoseconds, for a program that has done
    /// `work`.
    pub fn clock(&self, work: &Work) -> u64 {
        self.start + self.vm_cost(work) + (self.host - self.base_host)
    }

    /// The clock as of the last host call.
    pub(crate) fn now(&self) -> u64 {
        self.clock(&self.work)
    }

    /// Starts frame `frame`'s time for a VM that has done `work`. A program
    /// still busy from the frame before starts late.
    pub fn begin_frame(&mut self, frame: u64, work: Work) {
        // A program that has done nothing has just started.
        if work.executed == 0 {
            self.exec_run = false;
        }
        let start = frame.saturating_mul(FRAME_PS);
        self.start = start.max(self.busy_until);
        self.frame_end = start + FRAME_PS;
        self.base_work = work;
        self.work = work;
        self.base_host = self.host;
    }

    /// Ends the frame's time, where the VM got to.
    pub fn end_frame(&mut self, work: &Work) {
        self.busy_until = self.clock(work);
    }

    /// Whether the frame has time left for a VM that has done `work`.
    pub fn has_time(&self, work: &Work) -> bool {
        self.clock(work) < self.frame_end
    }

    /// The instructions to run before looking again.
    pub fn slice(&self, work: &Work) -> u32 {
        let left = self.frame_end.saturating_sub(self.clock(work));
        (left / self.costs.instruction).clamp(1, SLICE) as u32
    }

    /// Whether the frame's time is used up as of the last host call.
    pub(crate) fn spent(&self) -> bool {
        self.now() >= self.frame_end
    }

    pub(crate) fn heard(&mut self, work: Work) {
        self.work = work;
    }

    pub(crate) fn charge(&mut self, cost: u64) {
        self.host += cost;
    }

    /// Charges printing `chars` characters, and the times the screen
    /// scrolled up with the cells that moved, as [`scrolling`] gives them.
    pub(crate) fn charge_print(&mut self, chars: usize, (scrolls, cells): (u64, u64)) {
        let c = self.costs;
        if chars > 0 {
            self.host += c.print;
        }
        self.host += chars as u64 * c.print_char + scrolls * c.print_scroll;
        self.host += cells * c.print_scroll_cell;
    }

    /// Charges an `EXEC` that compiles `source`.
    pub(crate) fn charge_compile(&mut self, source: &str) {
        let (code, text) = source_chars(source);
        let c = self.costs;
        self.host += c.exec + code * c.compile_char + text * c.compile_text_char;
    }

    /// Charges reading `bytes` from file `name`, as `EXEC` (`exec`) or
    /// `LOADG` does.
    pub(crate) fn charge_read(&mut self, name: &str, bytes: u64, exec: bool) {
        let c = self.costs;
        if exec && !std::mem::replace(&mut self.exec_run, true) {
            self.host += c.first_exec;
        }
        let kind = if exec { 'X' } else { 'G' };
        if self
            .files_read
            .insert(format!("{kind}{}", name.to_ascii_uppercase()))
        {
            self.host += c.first_file + bytes * c.first_byte;
        }
    }

    /// Charges a `LOADG` that decoded an image of `pixels` from a file of
    /// `bytes`. UNVERIFIED: one that loads nothing costs what an untimed
    /// command does.
    pub(crate) fn charge_loadg(&mut self, pixels: u64, bytes: u64) {
        let c = self.costs;
        self.host += c.loadg + pixels * c.loadg_px + bytes * c.loadg_byte;
    }

    /// Whether a sprite command for sprite `id` is for one not among the
    /// last 64 changed, which it now is.
    fn far_sprite(&self, id: i64) -> bool {
        let Ok(id) = i16::try_from(id) else {
            return false;
        };
        id >= 0 && self.sprites_seen[id as usize & 63].replace(id) != id
    }

    /// Whether drawing `w` by `rows` pixels at `x`, `y` is at one of the
    /// last 64 places misaligned or blended drawing drew at, which it now
    /// is.
    fn place_seen(&self, x: i64, y: i64, w: u64, rows: u64) -> bool {
        let key = (x as u64 & 0xFFFF) | (y as u64 & 0xFFFF) << 16 | (w & 0xFFFF) << 32 | rows << 48;
        if self.places_seen.iter().any(|p| p.get() == key) {
            return true;
        }
        let i = self.next_place.get();
        self.places_seen[i].set(key);
        self.next_place.set((i + 1) % self.places_seen.len());
        false
    }

    /// What `rows` rows of a `GFILL` or `GCOPY` `w` pixels wide cost on
    /// top for starting at column `x` and row `y`, at `pct` percent of a
    /// `GFILL`'s. A `GFILL`, blended or not as `fill` says, also pays
    /// for a place not drawn at lately. UNVERIFIED: that a `GCOPY` does
    /// not, as TPERF's GCOPY at random places suggests.
    fn placed(&self, (x, y): (i64, i64), w: u64, rows: u64, pct: u64, fill: Option<bool>) -> u64 {
        let c = self.costs;
        let off = match x.rem_euclid(16) {
            0 => None,
            v => Some(usize::from(v % 4 == 0)),
        };
        let blended = fill == Some(true);
        if off.is_none() && !blended {
            return 0;
        }
        let cold = fill.is_some() && !self.place_seen(x, y, w, rows);
        let mut cost = 0;
        if let Some(k) = off {
            let t = c.place[k];
            let row = match w {
                ..=8 => t[0] * w / 8,
                9..=16 => lerp(t[0], t[1], w - 8, 8),
                17..=32 => lerp(t[1], t[2], w - 16, 16),
                33..=64 => lerp(t[2], t[3], w - 32, 32),
                65..=128 => lerp(t[3], t[4], w - 64, 64),
                _ => t[4],
            };
            let row = row + if cold && !blended { c.cold_row } else { 0 };
            cost += row * rows * pct / 100;
        }
        if cold && blended {
            cost += c.cold_alpha_px * w * rows;
        }
        cost
    }

    /// Charges the pixels a `GPAINT` filled.
    pub(crate) fn charge_paint(&mut self, pixels: u64) {
        self.host += pixels * self.costs.paint_px;
    }
}

/// The characters of `source` that are code, and those in strings and
/// comments, which cost less to compile.
fn source_chars(source: &str) -> (u64, u64) {
    let (mut code, mut text) = (0u64, 0u64);
    for line in source.split('\n') {
        let mut quoted = false;
        let mut prev = ' ';
        for (i, c) in line.char_indices() {
            if quoted {
                text += 1;
                quoted = c != '"';
            } else if c == '"' {
                text += 1;
                quoted = true;
            } else if c == '\'' || is_rem(line, i, prev) {
                text += line[i..].chars().count() as u64;
                break;
            } else {
                code += 1;
            }
            prev = c;
        }
        code += 1;
    }
    (code.saturating_sub(1), text)
}

/// Whether `REM` starts a comment at byte `i` of `line`, after `prev`.
fn is_rem(line: &str, i: usize, prev: char) -> bool {
    let word = |c: char| c.is_ascii_alphanumeric() || c == '_';
    line.get(i..i + 3)
        .is_some_and(|w| w.eq_ignore_ascii_case("REM"))
        && !word(prev)
        && !line[i + 3..].starts_with(word)
}

/// The times the screen scrolls when `text` is printed from `cursor` on
/// a screen of `size`, once for each line past the last, and the cells
/// that move, the whole screen each time.
pub(crate) fn scrolling(cursor: (u32, u32), (w, h): (u32, u32), text: &[u16]) -> (u64, u64) {
    let (mut x, mut lines) = (cursor.0, 0u64);
    for &c in text {
        if matches!(c, 10 | 13) {
            (x, lines) = (0, lines + 1);
            continue;
        }
        if x >= w {
            (x, lines) = (0, lines + 1);
        }
        x += 1;
    }
    let scrolls = (u64::from(cursor.1) + lines).saturating_sub(u64::from(h.saturating_sub(1)));
    (scrolls, scrolls * u64::from(w) * u64::from(h))
}

/// The point `at` of `span` along a straight line from `a` to `b`.
fn lerp(a: u64, b: u64, at: u64, span: u64) -> u64 {
    let (a, b) = (a as i64, b as i64);
    (a + (b - a) * at as i64 / span as i64).max(0) as u64
}

/// An argument as a whole number, 0 when it is not one.
fn int(args: &[Value], i: usize) -> i64 {
    args.get(i)
        .and_then(|v| v.to_int().ok())
        .map_or(0, i64::from)
}

/// Whether the blend argument at `i` blends with what is there.
fn blends(args: &[Value], i: usize) -> bool {
    matches!(int(args, i), 2 | 3)
}

impl Runtime {
    /// What `command` costs on the console the meter stands for.
    pub(crate) fn command_cost(&self, command: Command, args: &[Value], value: bool) -> u64 {
        use Command::*;
        let Some(meter) = &self.meter else {
            return 0;
        };
        let c = meter.costs;
        // Columns and rows inside GCLIP's rectangle.
        let clip = self.grp.clip;
        let size = |x0: i64, y0: i64, x1: i64, y1: i64| {
            let (x0, x1) = (
                x0.min(x1).max(clip.x0.into()),
                x0.max(x1).min(clip.x1.into()),
            );
            let (y0, y1) = (
                y0.min(y1).max(clip.y0.into()),
                y0.max(y1).min(clip.y1.into()),
            );
            ((x1 - x0 + 1).max(0) as u64, (y1 - y0 + 1).max(0) as u64)
        };
        let (clip_w, clip_h) = size(
            clip.x0.into(),
            clip.y0.into(),
            clip.x1.into(),
            clip.y1.into(),
        );
        let page = clip_w * clip_h;
        // Commands for one sprite, given as the first argument.
        let sprite = match command {
            Spofs | Sprot | Spscale | Spcolor | Sphome | Spchr | Spvar | Sphide | Spshow
            | Spanim | Spcol | Spcolvec | Splink | Spunlink | Spfunc | Spchk | Spstart | Spstop
            | Spused => !args.is_empty(),
            Spset => !value,
            Spclr => args.len() == 1,
            _ => false,
        };
        let other = match sprite && meter.far_sprite(int(args, 0)) {
            true => c.far_sprite,
            false => 0,
        };
        let px = |n: u64, rate: u64| c.grp + n * rate;
        let rect = |(w, h): (u64, u64), rate: u64| c.grp + h * c.row + w * h * rate;
        other
            + match command {
                Maincnt | Millisec => c.clock,
                Rnd | Rndf if value => c.rnd,
                Spofs if args.len() == 1 => c.spofs_out,
                Spofs => c.spofs,
                Spscale => c.spscale,
                // UNVERIFIED: that LOCATE with a screen or a depth, as
                // TPERF's TPRINT has it, costs what a setter does.
                Locate if args.len() == 2 => c.locate,
                Spshow => c.spshow,
                Spvar => c.spvar,
                Spchr => c.spchr,
                Stick => c.stick,
                Chkchr | Bgmchk => c.peek,
                Spset => {
                    let mut cost = c.spset + args.len() as u64 * c.spset_arg;
                    if value {
                        let (lo, hi) = match args.len() {
                            3 | 4 | 7 | 8 => (int(args, 0), int(args, 1)),
                            _ => (0, SPRITES as i64 - 1),
                        };
                        let (lo, hi) = (
                            lo.clamp(0, SPRITES as i64 - 1),
                            hi.clamp(0, SPRITES as i64 - 1),
                        );
                        let passed = match self.free_sprite(lo as usize, hi as usize) {
                            Some(id) => id.abs_diff(lo as usize) as u64,
                            None => lo.abs_diff(hi) + 1,
                        };
                        cost += c.spset_search + c.spset_new + passed * c.spset_scan;
                    } else {
                        let id = usize::try_from(int(args, 0)).ok();
                        let used = id
                            .and_then(|i| self.sprites.list().get(i))
                            .is_some_and(|s| s.used);
                        if !used {
                            cost += c.spset_new;
                        }
                    }
                    cost
                }
                Spclr => {
                    // SPCLR with no arguments costs what a range of every
                    // sprite does.
                    let n = match args.len() {
                        0 => SPRITES as u64,
                        2 => {
                            let at = |i| int(args, i).clamp(0, SPRITES as i64 - 1);
                            at(0).abs_diff(at(1)) + 1
                        }
                        _ => return other + c.spclr,
                    };
                    const AT: [u64; 6] = [1, 100, 500, 1000, 2000, SPRITES as u64];
                    let i = AT[1..5].iter().take_while(|&&a| n > a).count();
                    let (n0, n1) = (AT[i], AT[i + 1]);
                    let (c0, c1) = (c.spclr_range[i], c.spclr_range[i + 1]);
                    c0 + (c1 - c0) * (n - n0) / (n1 - n0)
                }
                Sphitsp | Sphitrc | Sphitinfo => c.sphitsp,
                Spanim | Tanim => c.spanim,
                Tput => c.tput,
                Tfill => {
                    let cells = (int(args, 3) - int(args, 1)).abs() + 1;
                    let rows = (int(args, 4) - int(args, 2)).abs() + 1;
                    c.tfill + (cells * rows) as u64 * c.tfill_cell
                }
                Scroll => {
                    let screen = usize::try_from(int(args, 0)).ok();
                    let (w, h) =
                        (screen.and_then(|s| self.text.get(s))).map_or((50, 30), |t| t.size());
                    c.setter + u64::from(w * h) * c.scroll_cell
                }
                Gfill => {
                    let blended = blends(args, 5);
                    let (rate, pct) = match blended {
                        true => (c.alpha_px, c.place_alpha_pct),
                        false => (c.fill_px, 100),
                    };
                    let (w, h) = size(int(args, 0), int(args, 1), int(args, 2), int(args, 3));
                    let x = int(args, 0).min(int(args, 2)).max(clip.x0.into());
                    let y = int(args, 1).min(int(args, 3)).max(clip.y0.into());
                    rect((w, h), rate) + meter.placed((x, y), w, h, pct, Some(blended))
                }
                Gcls => c.gcls + clip_h * c.row + page * c.fill_px,
                // As a GFILL of its four sides (TEST71's 21x11 box).
                Gbox => {
                    let w = (int(args, 2) - int(args, 0)).unsigned_abs() + 1;
                    let h = (int(args, 3) - int(args, 1)).unsigned_abs() + 1;
                    let px = (2 * (w + h)).saturating_sub(4).min(w * h);
                    c.grp + h * c.row + px * c.fill_px
                }
                Gline => {
                    let dx = (int(args, 2) - int(args, 0)).unsigned_abs();
                    let dy = (int(args, 3) - int(args, 1)).unsigned_abs();
                    c.line + (dx.max(dy) + 1) * c.line_px
                }
                Gcircle => {
                    let r = int(args, 2).unsigned_abs();
                    // A sector is filled.
                    match args.len() >= 6 && int(args, 5) == 1 {
                        true => px((r * r * 355 / 113).min(page), c.fill_px),
                        false => c.circle + (r * 710 / 113 + 1) * c.circle_px,
                    }
                }
                Gtri => {
                    let [x0, y0, x1, y1, x2, y2] = std::array::from_fn(|i| int(args, i));
                    let n = ((x1 - x0) * (y2 - y0) - (x2 - x0) * (y1 - y0)).unsigned_abs() / 2;
                    let (_, rows) = size(0, y0.min(y1).min(y2), 0, y0.max(y1).max(y2));
                    let rate = if blends(args, 7) {
                        c.alpha_px
                    } else {
                        c.tri_px
                    };
                    c.tri + rows * c.tri_row + n.min(page) * rate
                }
                Gcopy => {
                    let at = usize::from(args.len() > 7);
                    let w = (int(args, at + 2) - int(args, at)).unsigned_abs() + 1;
                    let h = (int(args, at + 3) - int(args, at + 1)).unsigned_abs() + 1;
                    let rate = match int(args, args.len().saturating_sub(1)) {
                        _ if args.len() < 7 => c.copy_px,
                        1 => c.copy_key_px,
                        2 | 3 => c.copy_alpha_px,
                        _ => c.copy_px,
                    };
                    let (w, h) = (w.min(clip_w), h.min(clip_h));
                    let x = int(args, at + 4).max(clip.x0.into());
                    let y = int(args, at + 5).max(clip.y0.into());
                    let placed = meter.placed((x, y), w, h, c.place_copy_pct, None);
                    c.copy + h * c.row + w * h * rate + placed
                }
                Gload | Gsave => {
                    let n = match args.len() {
                        5..=8 => {
                            let at = usize::from(command == Gsave && args.len() == 6);
                            (int(args, at + 2) * int(args, at + 3)).unsigned_abs()
                        }
                        _ => page,
                    };
                    px(n, c.load_px)
                }
                Gputchr | Gputchrp => {
                    let chars = match args.get(2) {
                        Some(Value::Str(s)) => s.borrow().len() as u64,
                        _ => 1,
                    };
                    let (size, rate) = match int(args, 3) == 8 {
                        true => (8, c.char_px),
                        false => (16, c.char16_px),
                    };
                    let scale = match args.len() >= 7 {
                        true => (int(args, 4).max(1) * int(args, 5).max(1)) as u64,
                        false => 1,
                    };
                    px(chars * size * size * scale, rate)
                }
                // The pixels it fills are charged as it fills them
                // (Meter::charge_paint).
                Gpaint => c.paint,
                Lfilter | Lmatrix => c.layer,
                Gclip => c.clip[usize::from(!args.is_empty())],
                Gpset | Gpget => c.pixel,
                Gtarget | Gcolor => c.grp,
                // UNVERIFIED: commands and functions not timed.
                _ if value => c.query,
                _ => c.setter,
            }
    }

    /// Makes the meter stand for `speed`, a new one if it changed.
    pub(crate) fn set_speed(&mut self, speed: Speed) {
        if self.meter.as_ref().map_or(Speed::Unlimited, |m| m.speed) != speed {
            self.meter = Meter::new(speed);
        }
    }
}
