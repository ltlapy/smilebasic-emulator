//! Running programs about as fast as a real Switch does, for checking that
//! a program written on a fast computer keeps up on the console.
//!
//! A [`Meter`] charges the work a program does in picoseconds of the
//! console's time: the VM's counts from [`Work`], each host command by
//! what it is given (pixels for drawing, cells for text) and each
//! character printed. A frame gives the program one sixtieth of a second
//! of that time. A program that needs more goes on into the next frame and
//! misses the frame's `VSYNC` as it would on the console, so `MAINCNT`,
//! `VSYNC` and the frames a program skips come out as there. `PERFBEGIN`
//! and `PERFEND` measure the same time.
//!
//! The costs come from TPERF's scenes and TEST65's loops, timed on both
//! consoles running SmileBASIC 4.4.9 (docs/benchmarking.md). Drawing
//! sprites, layers and text screens happens in the frame job, which TPERF
//! shows costs a program nothing, so only the commands are charged.
//!
//! UNVERIFIED: a subprogram's interpreter gets a frame's time of its own,
//! where the console shares one between the two.

use sb_core::value::Value;
use sb_core::vm::Work;

use crate::runtime::{Command, Runtime};

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
    builtin: u64,
    element: u64,
    lookup: u64,
    join: u64,
    /// `MAINCNT` and `MILLISEC`.
    clock: u64,
    /// Host functions not timed, such as `RND` and `BUTTON`.
    query: u64,
    /// Host commands not timed, and those that set one thing, as
    /// `SPROT`, `SPSCALE`, `SPCOLOR` and `LOCATE` do.
    setter: u64,
    spofs: u64,
    spset: u64,
    spclr: u64,
    sphitsp: u64,
    spanim: u64,
    print_char: u64,
    tput: u64,
    tfill_cell: u64,
    scroll_cell: u64,
    /// Every GRP command, before its pixels.
    grp: u64,
    fill_px: u64,
    alpha_px: u64,
    tri_px: u64,
    /// `GLINE` and `GBOX`, and `GCIRCLE`'s outline, before their pixels.
    line: u64,
    line_px: u64,
    circle: u64,
    circle_px: u64,
    copy_px: u64,
    load_px: u64,
    char_px: u64,
    gcls: u64,
    /// `LFILTER` and `LMATRIX`.
    layer: u64,
}

// From TPERF and TEST65 on a Switch 2 and a Switch 1. Where a scene times
// one size of a command, how its time splits between a base and its pixels
// is a guess.
const SWITCH2: Costs = Costs {
    instruction: 10_100,
    statement: 6_400,
    for_credit: 77_600,
    def_call: 145_200,
    builtin: 31_800,
    element: 17_300,
    lookup: 139_500,
    join: 183_500,
    clock: 9_500,
    // UNVERIFIED: about three builtins.
    query: 40_000,
    setter: 140_000,
    spofs: 130_000,
    // UNVERIFIED: how SPCHURN's time splits between SPSET and SPCLR.
    spset: 2_400_000,
    spclr: 1_200_000,
    sphitsp: 600_000,
    spanim: 540_000,
    print_char: 22_000,
    tput: 128_000,
    tfill_cell: 22_500,
    scroll_cell: 2_500,
    grp: 400_000,
    fill_px: 523,
    alpha_px: 8_910,
    tri_px: 1_380,
    line: 250_000,
    line_px: 5_300,
    circle: 227_000,
    circle_px: 2_100,
    copy_px: 1_260,
    load_px: 780,
    char_px: 3_560,
    // UNVERIFIED: what the GAME scene leaves.
    gcls: 300_000_000,
    layer: 8_000_000,
};

const SWITCH1: Costs = Costs {
    instruction: 29_600,
    statement: 2_800,
    for_credit: 256_500,
    def_call: 519_400,
    builtin: 71_900,
    element: 53_200,
    lookup: 304_600,
    join: 399_800,
    clock: 19_800,
    // UNVERIFIED: about three builtins.
    query: 100_000,
    setter: 450_000,
    spofs: 720_000,
    // UNVERIFIED: as for the Switch 2.
    spset: 5_000_000,
    spclr: 2_500_000,
    sphitsp: 1_330_000,
    spanim: 1_040_000,
    print_char: 40_300,
    tput: 440_000,
    tfill_cell: 36_600,
    scroll_cell: 4_900,
    grp: 900_000,
    fill_px: 5_815,
    alpha_px: 22_560,
    tri_px: 5_070,
    line: 500_000,
    line_px: 10_500,
    circle: 500_000,
    circle_px: 5_000,
    copy_px: 4_480,
    load_px: 1_860,
    char_px: 14_200,
    // UNVERIFIED: as for the Switch 2.
    gcls: 500_000_000,
    layer: 5_000_000,
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
            + n(work.elements, b.elements) * c.element
            + n(work.lookups, b.lookups) * c.lookup
            + n(work.joins, b.joins) * c.join;
        cost.saturating_sub(n(work.for_tests, b.for_tests) * c.for_credit)
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

    pub(crate) fn heard(&mut self, work: Work) {
        self.work = work;
    }

    pub(crate) fn charge(&mut self, cost: u64) {
        self.host += cost;
    }

    pub(crate) fn charge_print(&mut self, chars: usize) {
        self.host += chars as u64 * self.costs.print_char;
    }
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
        // Pixels inside GCLIP's rectangle.
        let clip = self.grp.clip;
        let area = |x0: i64, y0: i64, x1: i64, y1: i64| {
            let (x0, x1) = (
                x0.min(x1).max(clip.x0.into()),
                x0.max(x1).min(clip.x1.into()),
            );
            let (y0, y1) = (
                y0.min(y1).max(clip.y0.into()),
                y0.max(y1).min(clip.y1.into()),
            );
            ((x1 - x0 + 1).max(0) * (y1 - y0 + 1).max(0)) as u64
        };
        let page = area(
            clip.x0.into(),
            clip.y0.into(),
            clip.x1.into(),
            clip.y1.into(),
        );
        let px = |n: u64, rate: u64| c.grp + n * rate;
        match command {
            Maincnt | Millisec => c.clock,
            Spofs => c.spofs,
            Spset => c.spset,
            Spclr => c.spclr,
            Sphitsp | Sphitrc | Sphitinfo => c.sphitsp,
            Spanim | Tanim => c.spanim,
            Tput => c.tput,
            Tfill => {
                let cells = (int(args, 3) - int(args, 1)).abs() + 1;
                let rows = (int(args, 4) - int(args, 2)).abs() + 1;
                c.setter + (cells * rows) as u64 * c.tfill_cell
            }
            Scroll => {
                let screen = usize::try_from(int(args, 0)).ok();
                let (w, h) = (screen.and_then(|s| self.text.get(s))).map_or((50, 30), |t| t.size());
                c.setter + u64::from(w * h) * c.scroll_cell
            }
            Gfill => {
                let n = area(int(args, 0), int(args, 1), int(args, 2), int(args, 3));
                px(
                    n,
                    if blends(args, 5) {
                        c.alpha_px
                    } else {
                        c.fill_px
                    },
                )
            }
            Gbox => {
                let w = (int(args, 2) - int(args, 0)).unsigned_abs() + 1;
                let h = (int(args, 3) - int(args, 1)).unsigned_abs() + 1;
                c.line + 2 * (w + h) * c.line_px
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
                let rate = if blends(args, 7) {
                    c.alpha_px
                } else {
                    c.tri_px
                };
                px(n.min(page), rate)
            }
            Gcopy => {
                let at = usize::from(args.len() > 7);
                let w = (int(args, at + 2) - int(args, at)).unsigned_abs() + 1;
                let h = (int(args, at + 3) - int(args, at + 1)).unsigned_abs() + 1;
                px((w * h).min(page), c.copy_px)
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
                let size = if int(args, 3) == 8 { 8 } else { 16 };
                let scale = match args.len() >= 7 {
                    true => (int(args, 4).max(1) * int(args, 5).max(1)) as u64,
                    false => 1,
                };
                px(chars * size * size * scale, c.char_px)
            }
            // UNVERIFIED: that a fill takes as long as filling GCLIP's
            // rectangle.
            Gpaint => px(page, c.fill_px),
            Gcls => c.gcls,
            Lfilter | Lmatrix => c.layer,
            Gpset | Gpget | Gtarget | Gcolor | Gclip => c.grp,
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
