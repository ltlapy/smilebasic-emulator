//! Running programs about as fast as on a real Switch, for checking that
//! a program written on a fast computer keeps up on the console.
//!
//! A [`Meter`] charges the work a program does in picoseconds of the
//! console's time: the counts the VM keeps in [`Work`], each host command
//! by the work it is given (pixels for drawing, cells for text) and each
//! character printed. A frame gives the program one sixtieth of a second
//! of that time. A program that needs more continues into the next frame
//! and misses the frame's `VSYNC`, the same as on the console, so
//! `MAINCNT`, `VSYNC` and the frames a program skips match the console.
//! `PERFBEGIN`, `PERFEND` and `MILLISEC` measure the same time.
//!
//! The costs come from TPERF's scenes and the loops of TEST65, TEST67,
//! TEST69 to TEST76, timed on both consoles running SmileBASIC 4.4.9, and
//! from TEST68 on a Switch 2 (docs/performance/benchmarking.md). TEST70
//! finds a Switch 2 as fast handheld as docked.
//!
//! A `GFILL` or `GCOPY` whose left edge is not on a 16-pixel boundary
//! costs more on each row, by its width and the edge's column, and less
//! on a 4-pixel one (TEST69 to TEST72, most of them on a Switch 1). A
//! misaligned or blended `GFILL`, or a `GLINE`, at a place not among the
//! last 64 such places costs more on top of that. Changing sprites in turn
//! costs more once there are more than 64 of them, so the meter remembers
//! the last 64 sprites changed, and some sprite commands cost less while
//! the last 64 are for a few sprites. The first read of each file costs
//! more than the next ones. UNVERIFIED: that these are the console's
//! memory and storage at work, and the number of sprites between 64 and
//! 1000 at which the extra sprite cost starts.
//! Drawing sprites, layers and text screens happens in the frame job, which
//! TPERF and TEST67 show costs a program nothing, so only the commands are
//! charged.
//!
//! A subprogram's interpreter gets a separate frame's time.

use std::cell::Cell;

use sb_core::value::Value;
use sb_core::vm::{CHEAP, COSTLY, Work};

use crate::graphics::PAGES;
use crate::page::Page;
use crate::runtime::{Command, Runtime};
use crate::sprites::SPRITES;
use crate::text::USER_CHARS;

/// The speed programs run at: as fast as the computer allows, or about as
/// fast as on a console.
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

/// The most instructions run between checks of the time left in a frame,
/// so that a slice full of costly commands cannot run far past its end.
const SLICE: u64 = 20_000;

/// The most sprites that the last 64 sprite commands can be for while
/// some of those commands cost less: for 10 sprites in turn they do, for
/// 100 they do not. UNVERIFIED: where between.
const FEW_SPRITES: u32 = 12;

/// The cost of work on one console, in picoseconds.
#[derive(Debug)]
pub(crate) struct Costs {
    /// Each VM instruction.
    instruction: u64,
    /// Each statement, on top of its instructions.
    statement: u64,
    /// Taken off each test of a FOR: a NEXT is many of our instructions
    /// but one step on SmileBASIC 4.
    for_credit: u64,
    /// On top, each statement run inside a DEF of the running slot: a
    /// FOR, a WHILE and `A=A+1` cost a little more per pass in a DEF than
    /// at the top level, but code that EXEC runs from a DEF costs the same
    /// as at the top level. UNVERIFIED: that it is each statement.
    def_statement: u64,
    // A DEF call, and each value it sets up: parameters, OUT variables,
    // other variables and a result. An empty DEF costs much less than
    // the calls of TPERF's VDEF, and TEST69's HGIMEUPDATE, with four
    // VARs, costs more. UNVERIFIED: that each of these costs the same.
    def_call: u64,
    def_value: u64,
    /// Each builtin call. ON's jump counts as a call with no arguments.
    builtin: u64,
    /// Each argument a builtin call is given.
    builtin_arg: u64,
    /// Each array element read or written.
    element: u64,
    /// Each index of an element past its first.
    extra_index: u64,
    /// A character of a string read or written by index, on top of the
    /// element.
    str_element: u64,
    /// An `IF` or `ELSEIF` whose condition is an array element: TEST70's
    /// `IF MB%[ASC(S$[I%])] THEN BREAK` costs that much more than
    /// `C%=MB%[ASC(S$[I%])]`, whereas TEST65's `IF V>K THEN` costs the
    /// same as `V=A+B`, and TEST69's HGIMEUPDATE, which tests variables,
    /// `!` and `KEYBOARD()`, needs nothing on top.
    element_test: u64,
    /// Each element `FILL` writes.
    fill_element: u64,
    /// Each variable, label or DEF found by a name in a string at run time.
    lookup: u64,
    /// Each string join, before its characters.
    join: u64,
    /// `CHR$`, on top of the call, for TEST68's and TEST69's HGCNV$ and
    /// HGREV$, which make 488 characters with it in 1000. UNVERIFIED:
    /// that the cost is in making a string, which keeps the cost of
    /// TPERF's VSTR unchanged, whereas TEST71's `U$=CHR$(C)` of an ASCII
    /// code costs no more than a call.
    chr: u64,
    /// Each builtin of [`COSTLY`] on top of its call, in that order.
    /// `SORT` costs the same as a `SORT` of 16 elements, and `FREEMEM` the
    /// same as with the memory a small program leaves: UNVERIFIED how they
    /// grow.
    costly: [u64; COSTLY.len()],
    /// Each builtin of [`CHEAP`], which costs no call.
    cheap: [u64; CHEAP.len()],
    /// On top, `SIN`, `COS` and `TAN` of a number past a quarter of pi
    /// either way (TEST71, TEST72, TEST74). UNVERIFIED: the console's cost
    /// grows with the number, by half again from about 1 to 1000000 on a
    /// Switch 1; this is the cost of angles up to a full turn.
    trig_reduce: u64,
    /// Each character of the string a join makes.
    join_char: u64,
    /// A comparison of strings, or any other operator except `+` with a
    /// string on its left.
    str_compare: u64,
    /// `MAINCNT` and `MILLISEC`.
    clock: u64,
    /// Host functions, such as `BUTTON` and `SPCHK`.
    query: u64,
    /// `RND`.
    rnd: u64,
    /// `RNDF`.
    rndf: u64,
    /// Host commands not timed, and those that set one thing, such as
    /// `SPROT`, `SPCOLOR` and `LOCATE`.
    setter: u64,
    /// `LOCATE`, which TEST71 timed alone.
    locate: u64,
    /// `LOCATE` with a screen, which TEST72 times.
    locate_screen: u64,
    /// `COLOR` of a text color alone.
    color: u64,
    /// `COLOR` of a screen and a color.
    color_screen: u64,
    /// `TOFS`.
    tofs: u64,
    // `SPROT`, `SPCOLOR`, `SPHOME`, `SPLINK`, `SPHIDE`, `SPUNLINK`,
    // `SPSTOP`, `SPCOL` of a sprite alone and `SPCOLVEC` among sprite
    // commands for a few sprites ([`FEW_SPRITES`]), regardless of the
    // order. Among commands for more sprites each costs `setter`, which
    // TPERF's sprite scenes and TEST70's sprite loop over a hundred
    // sprites and more need.
    sprot: u64,
    spcolor: u64,
    sphome: u64,
    splink: u64,
    sphide: u64,
    spunlink: u64,
    spstop: u64,
    spcol: u64,
    spcolvec: u64,
    // `TROT`, `TSCALE`, `THOME` and `TCOLOR` as commands, `TCOPY` of 4x4
    // cells (UNVERIFIED: by the cells), `LCLIP` with a rectangle, and
    // `TOUCH` and `DTREAD` with `OUT`.
    trot: u64,
    tscale: u64,
    thome: u64,
    tcolor: u64,
    tcopy: u64,
    lclip: u64,
    touch: u64,
    dtread: u64,
    // Commands TEST71 timed alone: `SPSHOW`, `SPVAR`, `SPCHR`, `STICK` and
    // `SPOFS` with `OUT`, and `GPSET` and `GPGET`.
    spshow: u64,
    spvar: u64,
    spchr: u64,
    stick: u64,
    spofs_out: u64,
    pixel: u64,
    /// `GPGET`, which TEST74 finds cheaper than `GPSET` on a Switch 1.
    pixel_get: u64,
    /// `CHKCHR` and `BGMCHK`, which TEST71 times at less than `BUTTON`.
    peek: u64,
    /// `SPOFS` without `OUT`.
    spofs: u64,
    /// `SPSCALE`.
    spscale: u64,
    // `SPSET`: the call and each argument, a sprite not in use on top,
    // and the search of the function form with each sprite it passes.
    spset: u64,
    spset_arg: u64,
    spset_new: u64,
    spset_search: u64,
    spset_scan: u64,
    /// `SPCLR` of one sprite.
    spclr: u64,
    /// `SPCLR` of a range of 1, 100, 500, 1000, 2000 and 4096 sprites, in
    /// use or not, with straight lines between.
    spclr_range: [u64; 6],
    /// `SPHITSP` and the like.
    sphitsp: u64,
    /// `SPHITSP` of two sprites, which looks at no others.
    sphitsp_pair: u64,
    /// `SPANIM`.
    spanim: u64,
    /// `SPANIM` of a sprite among the last 64 changed (TEST74's three
    /// `SPANIM`s on a sprite just set, compared with TPERF's `SPANIM` over
    /// 3000 sprites).
    spanim_near: u64,
    /// Each thing `PRINT` prints.
    print: u64,
    /// Each character `PRINT` prints.
    print_char: u64,
    /// Each time `PRINT` scrolls the screen.
    print_scroll: u64,
    /// Each cell a scroll of `PRINT` moves.
    print_scroll_cell: u64,
    /// `TPUT`.
    tput: u64,
    /// `TFILL` before its cells.
    tfill: u64,
    /// Each cell `TFILL` writes.
    tfill_cell: u64,
    /// Each cell of the screen that `SCROLL` moves, on top of `setter`.
    scroll_cell: u64,
    /// Every GRP command but `GTRI` and `GCOPY`, before its rows and
    /// pixels.
    grp: u64,
    /// `GCOPY` before its rows and pixels.
    copy: u64,
    /// Each row of `GFILL`, `GCLS` and `GCOPY`.
    row: u64,
    /// `GCLS` before its rows and pixels.
    gcls: u64,
    /// `GCLIP` with arguments and without.
    clip: [u64; 2],
    // Pixels of `GFILL` and `GCLS`, of `GCOPY` and of blended drawing.
    // TEST69 finds the same rates with `GCLIP` set smaller than the
    // page.
    fill_px: u64,
    copy_px: u64,
    // A pixel of a `GCOPY` that skips the transparent ones. When
    // `key_pattern` is set, such a copy pays by its source's pattern
    // rather than by where it draws: each opaque pixel, each row with
    // both kinds, and each change from one kind to the other along a
    // row, at one rate when the run it ends is as long as the run
    // before it (stripes, a checkerboard) and at another otherwise.
    // UNVERIFIED: the console's sprite images cost a little more than
    // that gives, and a pattern of lines a little less.
    copy_key_px: u64,
    key_pattern: bool,
    key_ink_px: u64,
    key_mixed_row: u64,
    key_regular: u64,
    key_irregular: u64,
    /// A pixel of a blended `GFILL` or `GTRI`.
    alpha_px: u64,
    /// A pixel of a `GCOPY` in drawing mode 2 or 3.
    copy_alpha_px: u64,
    // `GTRI`: the call, each row and each pixel, more for one whose left
    // side is not one upright edge. UNVERIFIED: the console's cost also
    // depends on which way the left side leans.
    tri: u64,
    tri_row: u64,
    tri_px: u64,
    tri_sloped_px: u64,
    // `GLINE` before its pixels, each pixel along its longer side and
    // each row it goes down, and each pixel of a horizontal one. A steep
    // line pays on top for each pixel past four in a column.
    // UNVERIFIED: that it is the console's memory cache, of four rows.
    line: u64,
    line_px: u64,
    line_row: u64,
    line_steep: u64,
    line_flat_px: u64,
    /// `GCIRCLE`'s outline before its pixels.
    circle: u64,
    /// Each pixel of the outline.
    circle_px: u64,
    /// A pixel of `GLOAD` and of `GSAVE`.
    load_px: u64,
    /// A pixel of `GLOAD` of an image of palette indices (TEST74).
    load_index_px: u64,
    /// `LOADG` of an image.
    loadg: u64,
    /// Each pixel `LOADG` decodes.
    loadg_px: u64,
    /// Each byte of the file `LOADG` reads, which a busy image has more of.
    loadg_byte: u64,
    /// A pixel of `GPUTCHR` in the 8-pixel font (TPERF's 8 characters of 8
    /// pixels).
    char_px: u64,
    /// A pixel of `GPUTCHR` in the 16-pixel font (TEST71's one character of
    /// 16 pixels).
    char16_px: u64,
    // A user-defined character of `GPUTCHR` or `GPUTCHRP`, which comes
    // from the `TPAGE` page, for each row of its font's cell, and each
    // of its pixels, by drawing mode 0 to 4 (TEST73). The modes that
    // blend charge SmileBASIC 4's own characters this way too.
    user_row: [u64; 5],
    user_char_px: [u64; 5],
    // A row and a pixel of SmileBASIC 4's own characters in modes 2 to
    // 4, letters and spaces alike, from those of 16 pixels, and in mode 3
    // of 8 pixels too. UNVERIFIED: modes 2 and 4 of 8 pixels.
    own_blend_row: [u64; 3],
    own_blend_px: [u64; 3],
    /// A pixel of a `GCOPY` with a color code, by drawing mode, in place
    /// of the rates above (TEST73).
    copy_color_px: [u64; 5],
    /// `GPAINT`.
    paint: u64,
    /// Each pixel `GPAINT` fills.
    paint_px: u64,
    // Each row of a `GFILL` whose left edge is not on a 16-pixel
    // boundary, 8, 16, 32, 64 and 128 pixels wide, with straight lines
    // between, in proportion below 8 and the same past 128, by the
    // edge's column in its 16 ([`by_column`]). Then the share of it a
    // blended `GFILL` and a `GCOPY` pay, in percent, `GCOPY` by where
    // it draws. UNVERIFIED: the widths `GCOPY` and blending were not
    // timed at, other than 32, and the columns TEST71 and TEST72 did not
    // time.
    place: [[u64; 5]; 16],
    place_alpha_pct: u64,
    place_copy_pct: u64,
    // On top, at a place not among the last 64 a misaligned or blended
    // `GFILL` drew at: each row of a misaligned one and each pixel of a
    // blended one. TPERF's fills at random places cost that much more
    // than fills at one place or at 10 or 64 places in turn (TEST69 to
    // TEST71). UNVERIFIED: that it is the console's memory cache, and
    // that it holds 64 places: TEST71's 100 fills per call, 37 places on
    // from the call before, cost as much again as TPERF's fills.
    cold_row: u64,
    cold_alpha_px: u64,
    /// The same for each row a `GLINE` goes down: TPERF's lines at random
    /// places cost more than lines drawn again at one place. UNVERIFIED:
    /// that it is the same cache.
    cold_line_row: u64,
    /// A sprite command for a sprite not among the last 64 changed.
    /// TEST70 finds that SPOFS of 1, 10 and 64 sprites in turn costs the
    /// same, and TEST69 that SPOFS over 1000 sprites in order costs more.
    far_sprite: u64,
    /// `LFILTER` and `LMATRIX`.
    layer: u64,
    // Compiling for `EXEC`, of a file or of a slot: the call, each token
    // of code (a name, a number, a string or a sign) and each character
    // of strings and comments. Code of short names compiles faster per
    // character than code of long ones.
    exec: u64,
    compile_token: u64,
    compile_text_char: u64,
    // Reading a file for `LOAD`, `LOADV` or `EXEC`: the call and each
    // byte. `EXEC` of a file reads and compiles it, `LOAD` only reads
    // it, `EXEC` of a slot only compiles it.
    read: u64,
    read_byte: u64,
    // `LOADV`, on top of reading: each character of a TXT file's string,
    // and each element of a GRP or DAT file's array, after a GRP file
    // pays what its `LOADG` costs. UNVERIFIED: DAT as GRP.
    loadv_char: u64,
    loadv_element: u64,
    // On top: the first read of each file since the emulator started,
    // by any of those or `LOADG`, and each of its bytes, and with it the
    // first `EXEC` of a file since the program started. The console
    // reads a file faster once it has read it.
    first_exec: u64,
    first_file: u64,
    first_byte: u64,
    /// Starting a program with RUN: a Switch 1 takes two frames longer
    /// than a Switch 2, for a program of 5 lines or of 2002. UNVERIFIED:
    /// that SUBRUN does too.
    run: u64,
}

// From TPERF and TEST65 on a Switch 2 and a Switch 1, from TEST67 and
// TEST69, which time single commands at several sizes, and from TEST68's
// Hangul library on a Switch 2. The Switch 2 ran TEST67 to TEST69
// docked and TPERF handheld; the Switch 1 ran the others handheld.
const SWITCH2: Costs = Costs {
    instruction: 9_700,
    statement: 6_500,
    for_credit: 78_900,
    def_statement: 5_500,
    def_call: 72_000,
    def_value: 19_400,
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
    // TEST71's STR$, FORMAT$, VAL, INSTR, HEX$ and MID$, each alone, and
    // the rest of COSTLY, each alone.
    costly: [
        210_900, 136_700, 89_300, 83_400, 45_900, 67_200, 93_500, 125_500, 6_500, 313_000, 95_000,
        95_000, 83_200, 83_200, 85_300_000,
    ],
    // SIN, COS and SQR alone in TEST71, TEST72 and TEST74, TEST72's SIN
    // of 1000000.5, and the rest of CHEAP, each alone. UNVERIFIED: TAN as
    // SIN.
    cheap: [
        5_300, 6_800, 5_300, 16_000, 0, 9_000, 22_400, 26_000, 38_700, 38_700,
    ],
    trig_reduce: 39_600,
    join_char: 447,
    // The cost of a join before TEST71: TEST69's HGIMEUPDATE, which tests
    // K$!="", within 7%. UNVERIFIED: TEST71's IF S$[I]==K$ costs no
    // more than a comparison of numbers.
    str_compare: 121_000,
    clock: 5_000,
    query: 48_500,
    rnd: 21_000,
    rndf: 21_000,
    setter: 108_800,
    locate: 23_600,
    locate_screen: 35_700,
    color: 20_700,
    color_screen: 28_100,
    tofs: 54_400,
    sprot: 75_000,
    spcolor: 73_900,
    sphome: 89_400,
    splink: 92_200,
    sphide: 58_200,
    spunlink: 58_100,
    spstop: 57_100,
    spcol: 35_000,
    spcolvec: 55_500,
    trot: 29_000,
    tscale: 45_000,
    thome: 42_600,
    tcolor: 26_000,
    tcopy: 360_000,
    lclip: 69_500,
    touch: 114_400,
    dtread: 114_600,
    spshow: 57_700,
    spvar: 44_300,
    spchr: 87_300,
    stick: 67_100,
    spofs_out: 66_300,
    pixel: 51_000,
    pixel_get: 51_000,
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
    sphitsp_pair: 48_700,
    spanim: 540_000,
    spanim_near: 168_700,
    // TEST71's PRINT of one character, and TEST69's PRINT of 1000
    // characters and TPERF's TPRINT, which TEST72's LOCATE with a screen
    // leaves 3% fast with 22 ns per character.
    print: 67_000,
    print_char: 22_500,
    // TEST69's PRINT that scrolls a 50x30 console 19 times, 56 us, and
    // TEST68's and TEST69's PRINTs that scroll a 25x15 one 25 times.
    print_scroll: 329_000,
    print_scroll_cell: 1_737,
    tput: 128_000,
    tfill: 156_000,
    tfill_cell: 18_480,
    scroll_cell: 2_500,
    // TEST67's GFILL of 8, 32 and 128 pixels.
    grp: 125_000,
    // TEST71's GCOPY 16 to 64 pixels wide at x 16; 128 wide costs 10%
    // more per pixel.
    copy: 180_000,
    row: 1_000,
    // TEST70's and TEST71's GCLS under a 100x100 GCLIP, and TEST71's
    // GCLIP alone.
    gcls: 1_525_000,
    clip: [6_700, 47_000],
    fill_px: 458,
    copy_px: 1_120,
    // TPERF's GCOPY of 48x48 at random places. On a Switch 2 it costs the
    // same for any pattern.
    copy_key_px: 1_350,
    key_pattern: false,
    key_ink_px: 0,
    key_mixed_row: 0,
    key_regular: 0,
    key_irregular: 0,
    alpha_px: 8_900,
    copy_alpha_px: 9_290,
    tri: 166_700,
    tri_row: 15_600,
    tri_px: 426,
    tri_sloped_px: 895,
    // Lines 20x10 to 300x300 every way. Steep ones cost no more.
    line: 76_000,
    line_px: 4_580,
    line_row: 2_830,
    line_steep: 0,
    line_flat_px: 1_515,
    circle: 227_000,
    circle_px: 2_100,
    load_px: 780,
    load_index_px: 1_233,
    // TEST68's and TEST69's LOADG and HGLOAD of fonts from 512x312 to
    // 2048x1376 and TEST69's LOADG of a 512x512 image of noise (13.9 ms,
    // twice an empty one): all within 10%. UNVERIFIED: TEST69's empty
    // image, which SmileBASIC 4 saved itself, took 6.9 ms, half again what
    // the size of the file the emulator saves for it gives.
    loadg: 319_000_000,
    loadg_px: 15_150,
    // TEST69's and TEST70's images of noise load as fast as each other on
    // the console, but the emulator saves them 901070 and 725652 bytes
    // long: the cost per byte between them leaves them 5% slow and 9% fast.
    loadg_byte: 11_500,
    char_px: 4_150,
    char16_px: 8_540,
    // TEST73's GPUTCHR of 1 and 20 user-defined characters of 16 pixels
    // and of 1 at twice the size, in modes 1 to 3, and its screens of 8
    // lines of 16 and of 8 pixels in modes 0 to 4, with spaces. Modes 1
    // and 3 cost by the ink drawn, which the meter does not count: in
    // mode 1 NotoSans' lines cost 25% more than Galmuri11's lines, and
    // in mode 3 Galmuri11's lines cost 15% less than its single Hangul
    // syllables give.
    user_row: [45_000, 28_000, 51_800, 13_200, 25_100],
    user_char_px: [6_830, 1_620, 15_540, 21_330, 16_850],
    own_blend_row: [51_800, 52_800, 25_100],
    own_blend_px: [15_540, 14_500, 16_930],
    // TEST73's GCOPY with a color of 11x11, 12x16 and 16x16 pixels in
    // modes 1 to 3, and of a box of 11x11 and 16x16 in modes 0 and 4.
    copy_color_px: [5_900, 6_600, 15_860, 20_010, 15_860],
    // TEST67's GPAINT of 128x128 and 512x512 and TEST69's GPAINT of 4x4
    // and 16x16, all within 7%.
    paint: 33_500_000,
    paint_px: 7_531,
    // TEST71's GFILL 8 to 128 pixels wide at x 16, 17, 20 and 24, and its
    // GCOPY 32 and 128 at x 17. TEST69's blended GFILL 32 costs no more.
    // TEST70's GFILL 128 wide at 10 places in turn costs 30% more.
    // TEST72's GFILL 32 at x 18, 22 and 26 cost the same as one at x 17.
    place: by_column(
        [0, 3_850, 1_200, 740, 23_000],
        [0, 3_850, 1_550, 890, 14_000],
        &[],
    ),
    place_alpha_pct: 0,
    place_copy_pct: 60,
    // TEST71's GFILL and blended GFILL at random places.
    cold_row: 10_100,
    cold_alpha_px: 100,
    cold_line_row: 3_800,
    // TEST69's SPOFS over 1000 sprites needs none.
    far_sprite: 0,
    layer: 8_000_000,
    // EXEC of slots of 600 to 16000 characters of code after LOAD, and of
    // HGLIB.PRG with its set-up code; TEST69's EXEC of a file of DATA
    // strings.
    exec: 65_000_000,
    compile_token: 208_200,
    compile_text_char: 28_000,
    // LOAD of files of 600 to 73914 bytes, and EXEC of files taken as LOAD
    // and EXEC of the slot together.
    read: 305_000_000,
    read_byte: 74_700,
    // LOADV of programs of 2648 to 101357 bytes, and of a 512x352 GRP.
    loadv_char: 55_000,
    loadv_element: 65_000,
    // TEST69's to TEST71's first EXEC and LOADG of each file, TEST70's
    // and TEST71's first LOADG of 725652 bytes of noise as the emulator
    // saves it, and the first LOADV and EXEC of files of 48 to 101357
    // bytes.
    first_exec: 1_200_000_000,
    first_file: 350_000_000,
    first_byte: 9_250,
    run: 0,
};

const SWITCH1: Costs = Costs {
    instruction: 28_000,
    statement: 6_000,
    for_credit: 245_600,
    def_statement: 5_000,
    def_call: 166_000,
    def_value: 106_400,
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
    costly: [
        526_500,
        291_700,
        204_900,
        197_600,
        69_400,
        123_300,
        136_000,
        276_000,
        31_700,
        715_000,
        195_500,
        195_500,
        178_000,
        178_000,
        164_500_000,
    ],
    cheap: [
        14_000, 39_000, 14_000, 0, 0, 54_000, 78_000, 130_000, 64_000, 62_500,
    ],
    // SIN of 1 to 1000000.5 costs 42 to 87 ns more than of 0.5.
    trig_reduce: 70_000,
    join_char: 910,
    str_compare: 212_000,
    clock: 5_000,
    query: 121_000,
    rnd: 48_200,
    // Between TEST71's RNDF() and TEST76's Q#=RNDF(), 117 and 159 ns.
    rndf: 72_000,
    setter: 392_900,
    locate: 48_500,
    locate_screen: 126_900,
    color: 32_900,
    color_screen: 79_100,
    tofs: 136_800,
    sprot: 166_600,
    spcolor: 215_200,
    sphome: 254_600,
    splink: 230_400,
    sphide: 157_700,
    spunlink: 161_900,
    spstop: 150_800,
    spcol: 76_400,
    spcolvec: 149_800,
    trot: 56_600,
    tscale: 121_700,
    thome: 130_600,
    tcolor: 57_300,
    tcopy: 880_000,
    lclip: 195_000,
    touch: 317_700,
    dtread: 330_900,
    spshow: 160_200,
    spvar: 86_200,
    spchr: 194_000,
    stick: 179_300,
    spofs_out: 168_000,
    // TEST71's and TEST74's GPSET and GPGET.
    pixel: 180_000,
    pixel_get: 135_000,
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
    sphitsp_pair: 111_000,
    spanim: 840_000,
    spanim_near: 615_000,
    print: 158_000,
    print_char: 41_300,
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
    // GCOPY of 48x48 boxes of stripes, a checkerboard and sprite images
    // at x 16, which TPERF's GCOPY of a pattern of lines at random places
    // fits. It pays nothing for where it draws.
    copy_key_px: 2_977,
    key_pattern: true,
    key_ink_px: 282,
    key_mixed_row: 20_700,
    key_regular: 965,
    key_irregular: 20_000,
    alpha_px: 17_500,
    copy_alpha_px: 22_400,
    // Triangles 40x36 of each shape, thin ones, and TEST71's triangles of
    // 8 rows.
    tri: 260_000,
    tri_row: 49_100,
    tri_px: 3_040,
    tri_sloped_px: 3_950,
    // Lines 20x10 to 300x300 every way.
    line: 325_000,
    line_px: 10_210,
    line_row: 1_770,
    line_steep: 18_000,
    line_flat_px: 2_320,
    circle: 500_000,
    circle_px: 5_000,
    load_px: 1_860,
    load_index_px: 2_810,
    // As for the Switch 2, from TEST69's rows.
    loadg: 1_142_000_000,
    loadg_px: 24_600,
    // As for the Switch 2.
    loadg_byte: 22_600,
    char_px: 15_760,
    char16_px: 24_360,
    // As for the Switch 2.
    user_row: [140_700, 133_800, 198_800, 36_600, 35_900],
    user_char_px: [19_600, 3_340, 33_060, 54_360, 45_710],
    own_blend_row: [198_800, 230_700, 35_900],
    own_blend_px: [33_060, 29_500, 40_100],
    copy_color_px: [13_400, 19_000, 35_740, 51_800, 32_500],
    paint: 93_500_000,
    paint_px: 16_110,
    // As for the Switch 2: 168 ns per row of 32 pixels at an odd column,
    // such as x 17, and 61 ns on a 4-pixel boundary. At 32 pixels each
    // column off a 4-pixel boundary costs the average of GFILL 32 at the
    // four places of that column from x 0 to 63, and column 3 also
    // includes TEST74's GFILL 32 at x 3, 19 and 35: the odd columns move
    // by up to a sixth from one run to the next. TEST69's blended GFILL
    // pays 18% of it, and its GCOPY at 10 places and TEST70's GCOPY at 64
    // places pay 65%, whereas TEST71's GCOPY at x 17 pays 138%.
    place: by_column(
        [6_800, 0, 168_000, 235_000, 182_000],
        [1_500, 0, 61_500, 235_000, 182_000],
        &[
            (1, 165_400),
            (2, 98_700),
            (3, 123_900),
            (5, 175_900),
            (6, 171_500),
            (7, 136_700),
            (9, 148_700),
            (10, 60_700),
            (11, 154_800),
            (13, 160_300),
            (14, 47_800),
            (15, 139_100),
        ],
    ),
    place_alpha_pct: 18,
    place_copy_pct: 65,
    cold_row: 38_500,
    cold_alpha_px: 3_290,
    cold_line_row: 5_700,
    // TEST69's SPOFS over 1000 sprites in order, and SPSET and SPCLR.
    far_sprite: 150_000,
    layer: 5_000_000,
    // As for the Switch 2, except HGLIB.PRG, which this console held in
    // another version.
    exec: 110_000_000,
    compile_token: 594_000,
    compile_text_char: 80_000,
    read: 1_300_000_000,
    read_byte: 190_000,
    loadv_char: 180_000,
    loadv_element: 247_000,
    first_exec: 2_150_000_000,
    first_file: 1_000_000_000,
    first_byte: 30_500,
    // Three or four frames from RUN to a program's first line, whereas a
    // Switch 2 and the emulator take two.
    // Whole frames, so that the program starts at the start of a frame.
    run: FRAME_PS * 2,
};

/// The row costs of [`Costs::place`] for each column of a left edge in its
/// 16: `off` for one off a 4-pixel boundary, `on` for one on it, and the
/// cost at 32 pixels wide for the columns `w32` names.
const fn by_column(off: [u64; 5], on: [u64; 5], w32: &[(usize, u64)]) -> [[u64; 5]; 16] {
    let mut table = [[0; 5]; 16];
    let mut x = 1;
    while x < 16 {
        table[x] = if x % 4 == 0 { on } else { off };
        x += 1;
    }
    let mut i = 0;
    while i < w32.len() {
        table[w32[i].0][2] = w32[i].1;
        i += 1;
    }
    table
}

/// The console's time a program has used, kept by the runtime.
#[derive(Debug)]
pub struct Meter {
    pub(crate) speed: Speed,
    costs: &'static Costs,
    /// The cost of host commands and printing since the meter started.
    host: u64,
    /// The VM's counts, as last reported before a host call.
    work: Work,
    /// The counts and `host` when the frame's time began, and the clock
    /// then.
    base_work: Work,
    base_host: u64,
    start: u64,
    /// The clock at the end of the frame, and the program's own time
    /// when the last frame ended.
    frame_end: u64,
    busy_until: u64,
    /// The last sprites changed, each in the place its number's low six
    /// bits give.
    sprites_seen: [Cell<i16>; 64],
    /// The sprites of the last 64 sprite commands, in turn, and the index
    /// of the next; the number of those commands for each sprite, and the
    /// number of sprites they name.
    recent_sprites: [Cell<i16>; 64],
    next_sprite: Cell<usize>,
    sprite_uses: Box<[Cell<u8>]>,
    sprites_in_use: Cell<u32>,
    /// The last places misaligned or blended drawing drew at, in turn,
    /// and the index of the next.
    places_seen: [Cell<u64>; 64],
    next_place: Cell<usize>,
    /// The files read since the meter started, and whether the program
    /// has run an `EXEC` of a file.
    files_read: std::collections::HashSet<String>,
    exec_run: bool,
    /// Whether the code to run was typed in DIRECT mode, as the runtime
    /// last reported, and whether the program that has done nothing yet has
    /// paid for its start.
    direct: bool,
    started: bool,
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
            recent_sprites: std::array::from_fn(|_| Cell::new(-1)),
            next_sprite: Cell::new(0),
            sprite_uses: (0..SPRITES).map(|_| Cell::new(0)).collect(),
            sprites_in_use: Cell::new(0),
            places_seen: std::array::from_fn(|_| Cell::new(u64::MAX)),
            next_place: Cell::new(0),
            files_read: std::collections::HashSet::new(),
            exec_run: false,
            direct: false,
            started: false,
        })
    }

    /// The cost of `work`.
    fn vm_cost(&self, work: &Work) -> u64 {
        let c = self.costs;
        let b = &self.base_work;
        let n = |now: u64, then: u64| now.saturating_sub(then);
        let cost = n(work.executed, b.executed) * c.instruction
            + n(work.statements, b.statements) * c.statement
            + n(work.def_calls, b.def_calls) * c.def_call
            + n(work.def_values, b.def_values) * c.def_value
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
            + n(work.str_compares, b.str_compares) * c.str_compare
            + n(work.def_statements, b.def_statements) * c.def_statement
            + n(work.trig_reductions, b.trig_reductions) * c.trig_reduce;
        let costly: u64 = (work.costly_calls.iter().zip(&b.costly_calls))
            .zip(&c.costly)
            .map(|((&now, &then), &each)| n(now, then) * each)
            .sum();
        let cheap: u64 = (work.cheap_calls.iter().zip(&b.cheap_calls))
            .zip(&c.cheap)
            .map(|((&now, &then), &each)| n(now, then) * each)
            .sum();
        (cost + costly + cheap).saturating_sub(n(work.for_tests, b.for_tests) * c.for_credit)
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
        let start = frame.saturating_mul(FRAME_PS);
        // A program that has done nothing has just started. One that RUN
        // started is busy starting for a while.
        if work.executed == 0 {
            self.exec_run = false;
            if !self.direct && !std::mem::replace(&mut self.started, true) {
                self.busy_until = self.busy_until.max(start + self.costs.run);
            }
        } else {
            self.started = false;
        }
        self.start = start.max(self.busy_until);
        self.frame_end = start + FRAME_PS;
        self.base_work = work;
        self.work = work;
        self.base_host = self.host;
    }

    /// Ends the frame's time at the point the VM reached.
    pub fn end_frame(&mut self, work: &Work) {
        self.busy_until = self.clock(work);
    }

    /// Whether the frame has time left for a VM that has done `work`.
    pub fn has_time(&self, work: &Work) -> bool {
        self.clock(work) < self.frame_end
    }

    /// The instructions to run before checking again.
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
        self.started &= work.executed == 0;
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

    /// Charges an `EXEC` that compiles `source`, read from a file, which
    /// [`Meter::charge_read`] charges, or already in its slot.
    pub(crate) fn charge_compile(&mut self, source: &str, _from_file: bool) {
        let (tokens, text) = source_tokens(source);
        let c = self.costs;
        self.host += c.exec + tokens * c.compile_token + text * c.compile_text_char;
    }

    /// Charges reading `bytes` from file `name` for an `EXEC` (`exec`) or
    /// a `LOADG`. `LOADG`'s own costs include its reading.
    pub(crate) fn charge_read(&mut self, name: &str, bytes: u64, exec: bool) {
        let c = self.costs;
        let first_exec = exec && !std::mem::replace(&mut self.exec_run, true);
        let kind = if exec { "TXT" } else { "GRP" };
        if exec {
            self.host += c.read + bytes * c.read_byte;
        }
        if self.first_read(name, kind, bytes) && first_exec {
            self.host += c.first_exec;
        }
    }

    /// Charges reading `bytes` from file `name`, of `kind` unless the name
    /// gives one, for `LOAD` and `LOADV`, and `more` on top.
    pub(crate) fn charge_file(&mut self, name: &str, kind: &str, bytes: u64, more: u64) {
        let c = self.costs;
        self.host += c.read + bytes * c.read_byte + more;
        self.first_read(name, kind, bytes);
    }

    /// Charges a read of `bytes` from file `name`, of `kind` unless the
    /// name gives one, if it is the first read of that file since the
    /// meter started. Returns whether it was.
    fn first_read(&mut self, name: &str, kind: &str, bytes: u64) -> bool {
        let name = name.to_ascii_uppercase();
        let key = match name.split_once(':') {
            Some(_) => name,
            None => format!("{kind}:{name}"),
        };
        let first = self.files_read.insert(key);
        if first {
            self.host += self.costs.first_file + bytes * self.costs.first_byte;
        }
        first
    }

    /// Charges a `LOADG` that decoded an image of `pixels` from a file of
    /// `bytes`. UNVERIFIED: one that loads nothing costs the same as an
    /// untimed command.
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

    /// Whether the last 64 sprite commands, with this one for sprite
    /// `id`, are for no more than [`FEW_SPRITES`] sprites.
    fn few_sprites(&self, id: i64) -> bool {
        let Some(id) = i16::try_from(id).ok().filter(|&id| (id as usize) < SPRITES) else {
            return false;
        };
        let (uses, count) = (&self.sprite_uses, &self.sprites_in_use);
        let at = self.next_sprite.replace((self.next_sprite.get() + 1) % 64);
        let old = self.recent_sprites[at].replace(id);
        if let Ok(old) = usize::try_from(old) {
            uses[old].set(uses[old].get() - 1);
            count.set(count.get() - u32::from(uses[old].get() == 0));
        }
        count.set(count.get() + u32::from(uses[id as usize].get() == 0));
        uses[id as usize].set(uses[id as usize].get() + 1);
        count.get() <= FEW_SPRITES
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

    /// The cost on top for `rows` rows of a `GFILL` or `GCOPY` `w` pixels
    /// wide that start at column `x` and row `y`, at `pct` percent of the
    /// cost for a `GFILL`. A `GFILL`, blended or not as `fill` says, also
    /// pays for a place not drawn at lately. UNVERIFIED: that a `GCOPY`
    /// does not, as TPERF's GCOPY at random places suggests.
    fn placed(&self, (x, y): (i64, i64), w: u64, rows: u64, pct: u64, fill: Option<bool>) -> u64 {
        let c = self.costs;
        let column = x.rem_euclid(16) as usize;
        let blended = fill == Some(true);
        if column == 0 && !blended {
            return 0;
        }
        let cold = fill.is_some() && !self.place_seen(x, y, w, rows);
        let mut cost = 0;
        if column > 0 {
            let t = c.place[column];
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

/// The tokens of `source`'s code: each name or number, each string and
/// each other sign but spaces; and the characters in strings and
/// comments, which cost less to compile.
fn source_tokens(source: &str) -> (u64, u64) {
    let word =
        |c: char| c.is_ascii_alphanumeric() || matches!(c, '_' | '#' | '$' | '%' | '.' | '&');
    let (mut tokens, mut text) = (0u64, 0u64);
    for line in source.split('\n') {
        let mut quoted = false;
        let mut prev = ' ';
        for (i, c) in line.char_indices() {
            if quoted {
                text += 1;
                quoted = c != '"';
            } else if c == '"' {
                text += 1;
                tokens += 1;
                quoted = true;
            } else if c == '\'' || is_rem(line, i, prev) {
                text += line[i..].chars().count() as u64;
                break;
            } else if c != ' ' && !(word(c) && word(prev)) {
                tokens += 1;
            }
            prev = c;
        }
    }
    (tokens, text)
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

/// The pattern a `GCOPY` that skips transparent pixels meets in its source:
/// opaque pixels, rows with both kinds, and changes from one kind to the
/// other along a row, `regular` where the run that ends is as long as the
/// run before it in the row.
#[derive(Default)]
struct Pattern {
    ink: u64,
    mixed: u64,
    regular: u64,
    irregular: u64,
}

/// The [`Pattern`] of `w` by `h` pixels of `page` from `(x, y)`.
fn key_pattern(page: &Page, (x, y): (i64, i64), (w, h): (u64, u64)) -> Pattern {
    let mut p = Pattern::default();
    for row in 0..h as i64 {
        let (mut last, mut run, mut before, mut first, mut ink) = (None, 0u64, 0u64, true, 0u64);
        for col in 0..w as i64 {
            let opaque = page.pixel((x + col) as i32, (y + row) as i32) >> 24 != 0;
            ink += u64::from(opaque);
            if last.is_some_and(|l| l != opaque) {
                match first || run != before {
                    true => p.irregular += 1,
                    false => p.regular += 1,
                }
                (first, before, run) = (false, run, 0);
            }
            run += 1;
            last = Some(opaque);
        }
        p.ink += ink;
        p.mixed += u64::from(ink > 0 && ink < w);
    }
    p
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

/// Whether the blend argument at `i` blends with the pixels already there.
fn blends(args: &[Value], i: usize) -> bool {
    matches!(int(args, i), 2 | 3)
}

impl Runtime {
    /// The cost of `command` on the console the meter stands for.
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
        let far = sprite && meter.far_sprite(int(args, 0));
        let other = match far {
            true => c.far_sprite,
            false => 0,
        };
        // A sprite command among others for a few sprites.
        let near = sprite && meter.few_sprites(int(args, 0)) && !value;
        let px = |n: u64, rate: u64| c.grp + n * rate;
        let rect = |(w, h): (u64, u64), rate: u64| c.grp + h * c.row + w * h * rate;
        other
            + match command {
                Maincnt | Millisec => c.clock,
                Rnd if value => c.rnd,
                Rndf if value => c.rndf,
                Spofs if args.len() == 1 => c.spofs_out,
                Spofs => c.spofs,
                Spscale => c.spscale,
                Locate if args.len() == 2 => c.locate,
                Locate if args.len() == 3 => c.locate_screen,
                Spshow => c.spshow,
                Sprot if near => c.sprot,
                Spcolor if near => c.spcolor,
                Sphome if near => c.sphome,
                Splink if near => c.splink,
                Sphide if near => c.sphide,
                Spunlink if near => c.spunlink,
                Spstop if near => c.spstop,
                Spcol if near && args.len() == 1 => c.spcol,
                Spcolvec if near => c.spcolvec,
                Trot if !value => c.trot,
                Tscale if !value => c.tscale,
                Thome if !value => c.thome,
                Tcolor if !value => c.tcolor,
                Tcopy => c.tcopy,
                Lclip if args.len() >= 5 => c.lclip,
                Touch if !value => c.touch,
                Dtread if !value => c.dtread,
                Color if args.len() == 1 => c.color,
                Color if args.len() == 2 => c.color_screen,
                Tofs => c.tofs,
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
                    // SPCLR with no arguments costs the same as a range of
                    // every sprite.
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
                Sphitsp if args.len() == 2 => c.sphitsp_pair,
                Sphitsp | Sphitrc | Sphitinfo => c.sphitsp,
                Spanim if !far => c.spanim_near,
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
                    let steep = (dy + 1).saturating_sub(4 * (dx + 1));
                    let (x, y) = (int(args, 0), int(args, 1));
                    let cold = match meter.place_seen(x, y, dx, dy) {
                        true => 0,
                        false => (dy + 1) * c.cold_line_row,
                    };
                    cold + match dy {
                        0 => c.line + (dx + 1) * c.line_flat_px,
                        _ => {
                            c.line
                                + (dx.max(dy) + 1) * c.line_px
                                + dy * c.line_row
                                + steep * c.line_steep
                        }
                    }
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
                    // Two corners on the leftmost column make the left
                    // side one upright edge.
                    let left = x0.min(x1).min(x2);
                    let upright = [x0, x1, x2].iter().filter(|&&x| x == left).count() > 1;
                    let rate = match (blends(args, 7), upright) {
                        (true, _) => c.alpha_px,
                        (false, true) => c.tri_px,
                        (false, false) => c.tri_sloped_px,
                    };
                    c.tri + rows * c.tri_row + n.min(page) * rate
                }
                Gcopy => {
                    let at = usize::from(args.len() > 7);
                    let w = (int(args, at + 2) - int(args, at)).unsigned_abs() + 1;
                    let h = (int(args, at + 3) - int(args, at + 1)).unsigned_abs() + 1;
                    let mode = int(args, args.len().saturating_sub(1));
                    let rate = match mode {
                        _ if args.len() < 7 => c.copy_px,
                        // With a color code.
                        _ if args.len() == 9 => c.copy_color_px[mode.clamp(0, 4) as usize],
                        1 => c.copy_key_px,
                        2 | 3 => c.copy_alpha_px,
                        _ => c.copy_px,
                    };
                    let (w, h) = (w.min(clip_w), h.min(clip_h));
                    if c.key_pattern && mode == 1 && args.len() < 9 {
                        let src = match at {
                            1 => int(args, 0).clamp(0, PAGES as i64 - 1) as usize,
                            _ => self.grp.target,
                        };
                        let sx = int(args, at).min(int(args, at + 2));
                        let sy = int(args, at + 1).min(int(args, at + 3));
                        let p = key_pattern(self.grp.page(src), (sx, sy), (w, h));
                        return c.copy
                            + h * c.row
                            + w * h * rate
                            + p.ink * c.key_ink_px
                            + p.mixed * c.key_mixed_row
                            + p.regular * c.key_regular
                            + p.irregular * c.key_irregular;
                    }
                    let x = int(args, at + 4).max(clip.x0.into());
                    let y = int(args, at + 5).max(clip.y0.into());
                    let placed = meter.placed((x, y), w, h, c.place_copy_pct, None);
                    c.copy + h * c.row + w * h * rate + placed
                }
                // An image of palette indices, to a rectangle.
                Gload if matches!(args.get(5), Some(Value::Array(_))) => px(
                    (int(args, 2) * int(args, 3)).unsigned_abs(),
                    c.load_index_px,
                ),
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
                // The function form only measures: it costs its call.
                Gputchrp if value => 0,
                Gputchr | Gputchrp => {
                    // SmileBASIC 4's own characters, and user-defined ones.
                    let (chars, user) = match args.get(2) {
                        Some(Value::Str(s)) => {
                            let s = s.borrow();
                            let user = s.iter().filter(|c| USER_CHARS.contains(c)).count();
                            (s.len() as u64, user as u64)
                        }
                        _ => (1, u64::from(USER_CHARS.contains(&(int(args, 2) as u16)))),
                    };
                    let (size, rate) = match int(args, 3) == 8 {
                        true => (8, c.char_px),
                        false => (16, c.char16_px),
                    };
                    let scale = match args.len() >= 7 {
                        true => (int(args, 4).max(1) * int(args, 5).max(1)) as u64,
                        false => 1,
                    };
                    // The drawing mode: GPUTCHRP's mode is 1 when omitted.
                    let at = if args.len() >= 7 { 7 } else { 5 };
                    let mode = match args.get(at) {
                        None | Some(Value::Default) if command == Gputchrp => 1,
                        _ => int(args, at).clamp(0, 4) as usize,
                    };
                    // In the modes that blend, SmileBASIC 4's own
                    // characters cost by rows and pixels the same way as
                    // user-defined ones, at their own rate.
                    let px_each = size * size * scale;
                    let own = match mode {
                        2.. => {
                            size * c.own_blend_row[mode - 2] + px_each * c.own_blend_px[mode - 2]
                        }
                        _ => px_each * rate,
                    };
                    c.grp
                        + (chars - user) * own
                        + user * (size * c.user_row[mode] + px_each * c.user_char_px[mode])
                }
                // The pixels it fills are charged as it fills them
                // (Meter::charge_paint).
                Gpaint => c.paint,
                Lfilter | Lmatrix => c.layer,
                Gclip => c.clip[usize::from(!args.is_empty())],
                Gpset => c.pixel,
                Gpget => c.pixel_get,
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
        if let Some(meter) = &mut self.meter {
            meter.direct = self.direct;
        }
    }

    /// Charges a `LOAD` (`what`) of `spec` into `slot`, if it `loaded`.
    /// `EXEC` charges its own reading.
    pub(crate) fn charge_load(&mut self, what: &str, spec: &str, slot: usize, loaded: bool) {
        if let Some(meter) = self.meter.as_mut().filter(|_| loaded && what == "LOAD") {
            meter.charge_file(spec, "TXT", self.slots[slot].len() as u64, 0);
        }
    }

    /// Charges a `LOADV` of `spec` that gave `value`.
    pub(crate) fn charge_loadv<E>(&mut self, spec: &str, value: &std::result::Result<Value, E>) {
        match value {
            Ok(Value::Str(s)) => {
                if let Some(meter) = &mut self.meter {
                    let s = s.borrow();
                    let bytes = s
                        .iter()
                        .map(|&u| 1 + u64::from(u >= 0x80) + u64::from(u >= 0x800));
                    let more = s.len() as u64 * meter.costs.loadv_char;
                    meter.charge_file(spec, "TXT", bytes.sum(), more);
                }
            }
            Ok(Value::Array(a)) => self.charge_loadv_array(spec, Some(a.borrow().len())),
            _ => {}
        }
    }

    /// Charges a `LOADV` of `spec` into an array that it gave `elements`.
    pub(crate) fn charge_loadv_array(&mut self, spec: &str, elements: Option<usize>) {
        let (Some(meter), Some(elements)) = (&mut self.meter, elements) else {
            return;
        };
        let c = meter.costs;
        let (n, more) = (elements as u64, elements as u64 * c.loadv_element);
        match spec
            .get(..4)
            .is_some_and(|k| k.eq_ignore_ascii_case("GRP:"))
        {
            // The image is decoded the same way as by LOADG.
            true => {
                meter.host += c.loadg + n * c.loadg_px + more;
                meter.first_read(spec, "GRP", 0);
            }
            false => meter.charge_file(spec, "DAT", n * 4, more),
        }
    }
}
