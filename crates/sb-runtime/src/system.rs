//! Random numbers, time and the system's settings: RND, RNDF, RANDOMIZE,
//! MILLISEC, MAINCNT, DATE$, TIME$, DTREAD, TMREAD, SYSPARAM and FONTINFO.

use std::time::{Instant, SystemTime, UNIX_EPOCH};

use chrono::{Datelike, Local, NaiveDate, NaiveDateTime, NaiveTime, Timelike};
use sb_core::bytecode::Want;
use sb_core::error::{Error, ErrorKind, Result};
use sb_core::value::{Value, new_str};

use crate::font::FontSize;
use crate::runtime::{Command, Runtime, checking, no_form};
use crate::text::USER_CHARS;

/// RND's series 0 to 7.
const SERIES: usize = 8;

/// One series of random numbers: TinyMT32 with its standard parameters,
/// which gives SmileBASIC 4's numbers for the same seed (checked on
/// hardware with seeds 1, 2 and -1).
#[derive(Clone, Copy, Debug)]
struct Series([u32; 4]);

const MAT1: u32 = 0x8F70_11EE;
const MAT2: u32 = 0xFC78_FF1F;
const TMAT: u32 = 0x3793_FDFF;

impl Series {
    fn seeded(seed: u32) -> Series {
        let mut st = [seed, MAT1, MAT2, TMAT];
        for i in 1..8u32 {
            let prev = st[(i as usize - 1) & 3];
            st[i as usize & 3] ^=
                i.wrapping_add(1_812_433_253u32.wrapping_mul(prev ^ (prev >> 30)));
        }
        let mut s = Series(st);
        for _ in 0..8 {
            s.advance();
        }
        s
    }

    fn advance(&mut self) {
        let st = &mut self.0;
        let mut y = st[3];
        let mut x = (st[0] & 0x7FFF_FFFF) ^ st[1] ^ st[2];
        x ^= x << 1;
        y ^= (y >> 1) ^ x;
        st[0] = st[1];
        st[1] = st[2];
        st[2] = x ^ (y << 10);
        st[3] = y;
        if y & 1 != 0 {
            st[1] ^= MAT1;
            st[2] ^= MAT2;
        }
    }

    fn next(&mut self) -> u32 {
        self.advance();
        let st = &self.0;
        let t1 = st[0].wrapping_add(st[2] >> 8);
        let t0 = st[3] ^ t1;
        if t1 & 1 != 0 { t0 ^ TMAT } else { t0 }
    }
}

/// A seed from the clock, different for each call.
fn entropy() -> u32 {
    static CALLS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos() as u64);
    let n = nanos ^ CALLS.fetch_add(1, std::sync::atomic::Ordering::Relaxed) << 48;
    (n ^ n >> 32) as u32
}

pub struct System {
    series: [Series; SERIES],
    started: Instant,
    /// Frames since start-up, for MAINCNT.
    pub(crate) frames: u32,
    /// Time and chance come from the frame count and fixed seeds, for runs
    /// that must repeat exactly; see [`System::fix`].
    fixed: bool,
    /// Seeds given out since [`System::fix`].
    fixed_seeds: u32,
    /// SYSPARAM "SYSBEEP": the system's own sounds play. The emulator has
    /// none, so this is only kept.
    sysbeep: bool,
    /// SYSPARAM "MOUSECURSOR": programs show the mouse cursor.
    pub mouse_cursor: bool,
    /// When PERFBEGIN started each of its eight measurements.
    perf: [Option<Instant>; 8],
    /// What PERFEND has timed of each since RUN, for the debugger's
    /// panel.
    perf_stats: [PerfStats; 8],
}

/// One PERFBEGIN and PERFEND measurement's times since RUN, in
/// microseconds, which the emulator's debugger shows; programs never see
/// them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PerfStats {
    /// PERFENDs timed.
    pub count: u64,
    pub last: u64,
    pub max: u64,
    pub total: u64,
    /// The colour PERFBEGIN last gave, as a colour code.
    pub color: Option<u32>,
    /// PERFBEGIN started it, though PERFEND may not have ended it yet.
    pub begun: bool,
}

impl PerfStats {
    /// Kept cheap, as PERFEND may run many times a frame.
    fn add(&mut self, micros: u64) {
        self.count += 1;
        self.last = micros;
        self.total += micros;
        self.max = self.max.max(micros);
    }

    pub fn average(&self) -> u64 {
        self.total.checked_div(self.count).unwrap_or(0)
    }

    /// Whether PERFBEGIN or PERFEND used it since RUN.
    pub fn used(&self) -> bool {
        self.begun || self.count > 0
    }
}

/// SYSPARAM "KEYREPEAT": frames before a held key repeats, and between
/// repeats, as SmileBASIC 4's settings start.
const KEY_REPEAT: [i32; 2] = [20, 3];

impl Default for System {
    fn default() -> Self {
        System {
            series: std::array::from_fn(|_| Series::seeded(entropy())),
            started: Instant::now(),
            frames: 0,
            fixed: false,
            fixed_seeds: 0,
            sysbeep: true,
            mouse_cursor: false,
            perf: [None; 8],
            perf_stats: [PerfStats::default(); 8],
        }
    }
}

impl System {
    /// Makes time and chance the same on every run: MILLISEC, DATE$ and
    /// TIME$ follow the frame count from a set date, PERFEND gives 0, and
    /// the random series start from set seeds. For measuring and comparing
    /// runs, never for playing.
    pub fn fix(&mut self) {
        self.fixed = true;
        self.frames = 0;
        self.series = std::array::from_fn(|i| Series::seeded(i as u32 + 1));
    }

    pub fn is_fixed(&self) -> bool {
        self.fixed
    }

    fn millis(&self) -> i32 {
        match self.fixed {
            true => (u64::from(self.frames) * 1000 / 60) as i32,
            false => self.started.elapsed().as_millis() as i32,
        }
    }

    fn now(&self) -> NaiveDateTime {
        if !self.fixed {
            return Local::now().naive_local();
        }
        let start = NaiveDate::from_ymd_opt(2026, 1, 1)
            .and_then(|d| d.and_hms_opt(0, 0, 0))
            .expect("a date that exists");
        start + chrono::TimeDelta::seconds(i64::from(self.frames / 60))
    }

    fn next_seed(&mut self) -> u32 {
        if !self.fixed {
            return entropy();
        }
        self.fixed_seeds += 1;
        self.fixed_seeds.wrapping_mul(0x9E37_79B1)
    }
}

fn int(v: &Value) -> Result<i32> {
    v.to_int()
}

/// PERFBEGIN's and PERFEND's measurement number, 0 to 7.
fn perf_id(v: &Value) -> Result<usize> {
    checking(v, || {
        let n = int(v)?;
        usize::try_from(n)
            .ok()
            .filter(|&n| n < 8)
            .ok_or_else(|| Error::with_detail(ErrorKind::OutOfRange, format!("measurement {n}")))
    })
}

fn series_id(v: &Value) -> Result<usize> {
    checking(v, || {
        let n = int(v)?;
        usize::try_from(n)
            .ok()
            .filter(|&n| n < SERIES)
            .ok_or_else(|| Error::with_detail(ErrorKind::OutOfRange, format!("series {n}")))
    })
}

fn string(s: String) -> Value {
    Value::Str(new_str(s.encode_utf16().collect()))
}

fn text(v: &Value) -> Result<String> {
    Ok(String::from_utf16_lossy(&v.clone().into_str()?.borrow()))
}

/// Two-digit (or four, for the year) fields between `sep`s, as DTREAD and
/// TMREAD take them.
fn fields(s: &str, sep: char, widths: [usize; 3]) -> Result<[u32; 3]> {
    let parts: Vec<&str> = s.split(sep).collect();
    let bad = || Error::with_detail(ErrorKind::IllegalFunctionCall, s.to_string());
    if parts.len() != 3 {
        return Err(bad());
    }
    let mut out = [0; 3];
    for (i, part) in parts.iter().enumerate() {
        if part.len() != widths[i] || !part.bytes().all(|b| b.is_ascii_digit()) {
            return Err(bad());
        }
        out[i] = part.parse().map_err(|_| bad())?;
    }
    Ok(out)
}

impl Runtime {
    pub(crate) fn system_command(
        &mut self,
        command: Command,
        args: &[Value],
        want: Want,
    ) -> Result<Vec<Value>> {
        let sys = &mut self.system;
        match (command, want, args.len()) {
            (Command::Rnd, Want::Value, 1..=2) => {
                let (id, max) = match args {
                    [max] => (0, int(max)?),
                    [id, max] => (series_id(id)?, int(max)?),
                    _ => unreachable!("matched above"),
                };
                // RND(0) is 0 without taking a number, and a negative
                // maximum Out of range, as on hardware.
                if max < 0 {
                    return Err(Error::with_detail(
                        ErrorKind::OutOfRange,
                        format!("maximum {max}"),
                    ));
                }
                if max == 0 {
                    return Ok(vec![Value::Int(0)]);
                }
                let r = sys.series[id].next() % max as u32;
                Ok(vec![Value::Int(r as i32)])
            }
            (Command::Rndf, Want::Value, 0..=1) => {
                let id = args.first().map_or(Ok(0), series_id)?;
                // 53 bits from two numbers, as TinyMT's and Mersenne
                // Twister's res53: 0 up to but not including 1.
                let a = u64::from(sys.series[id].next() >> 5);
                let b = u64::from(sys.series[id].next() >> 6);
                let r = (a << 26 | b) as f64 / (1u64 << 53) as f64;
                Ok(vec![Value::Real(r)])
            }
            (Command::Randomize, Want::Outs(0), 1..=2) => {
                let id = series_id(&args[0])?;
                let seed = match args.get(1) {
                    None | Some(Value::Default) => 0,
                    Some(v) => int(v)?,
                };
                sys.series[id] = Series::seeded(match seed {
                    0 => sys.next_seed(),
                    n => n as u32,
                });
                Ok(vec![])
            }
            (Command::Millisec, Want::Value, 0) => Ok(vec![Value::Int(sys.millis())]),
            // Past XSCREEN or ACLS, the frame job has yet to raise it.
            (Command::Maincnt, Want::Value, 0) => Ok(vec![Value::Int(
                sys.frames.wrapping_sub(self.in_frame_job.into()) as i32,
            )]),
            // PERFBEGIN and PERFEND measure time; SmileBASIC 4.4.9 shows no
            // gauge for them, nor does the emulator on its screens. Only
            // the debugger's panel lists the times kept here.
            (Command::Perfbegin, Want::Outs(0), 1 | 2) => {
                let n = perf_id(&args[0])?;
                let color = args.get(1).map(int).transpose()?;
                let stats = &mut sys.perf_stats[n];
                stats.begun = true;
                if let Some(color) = color {
                    stats.color = Some(color as u32);
                }
                sys.perf[n] = Some(Instant::now());
                Ok(vec![])
            }
            (Command::Perfend, Want::Outs(0) | Want::Value, 1) => {
                let n = perf_id(&args[0])?;
                // Without PERFBEGIN, the time since SmileBASIC started.
                let since = sys.perf[n].unwrap_or(sys.started);
                let micros = match sys.fixed {
                    true => 0,
                    false => since.elapsed().as_micros() as u64,
                };
                sys.perf_stats[n].add(micros);
                Ok(match want {
                    Want::Value => vec![Value::Int(micros as i32)],
                    _ => vec![],
                })
            }
            (Command::Date, Want::Value, 0) => {
                Ok(vec![string(sys.now().format("%Y/%m/%d").to_string())])
            }
            (Command::Time, Want::Value, 0) => {
                Ok(vec![string(sys.now().format("%H:%M:%S").to_string())])
            }
            (Command::Dtread, Want::Outs(3 | 4), 0..=1) => {
                let date = match args.first() {
                    None => sys.now().date(),
                    Some(v) => {
                        let s = text(v)?;
                        let [y, m, d] = fields(&s, '/', [4, 2, 2])?;
                        NaiveDate::from_ymd_opt(y as i32, m, d).ok_or_else(|| {
                            Error::with_detail(ErrorKind::IllegalFunctionCall, s.clone())
                        })?
                    }
                };
                let mut out = [
                    date.year(),
                    date.month() as i32,
                    date.day() as i32,
                    date.weekday().num_days_from_sunday() as i32,
                ]
                .map(Value::Int)
                .to_vec();
                out.truncate(want.count());
                Ok(out)
            }
            (Command::Tmread, Want::Outs(3), 0..=1) => {
                let time = match args.first() {
                    None => sys.now().time(),
                    Some(v) => {
                        let s = text(v)?;
                        let [h, m, sec] = fields(&s, ':', [2, 2, 2])?;
                        NaiveTime::from_hms_opt(h, m, sec).ok_or_else(|| {
                            Error::with_detail(ErrorKind::IllegalFunctionCall, s.clone())
                        })?
                    }
                };
                Ok([time.hour(), time.minute(), time.second()]
                    .map(|n| Value::Int(n as i32))
                    .to_vec())
            }
            (Command::Sysparam, _, 1..=2) => {
                // Names may be in lower case (checked on hardware).
                let name = text(&args[0])?.to_ascii_uppercase();
                let flag = |on: bool| Value::Int(on.into());
                let values = match name.as_str() {
                    "LANG" => vec![string(self.settings.language.code().to_string())],
                    "SYSBEEP" => vec![flag(sys.sysbeep)],
                    "TABSTEP" => vec![Value::Int(self.settings.editor_tab_step as i32)],
                    "KEYREPEAT" => KEY_REPEAT.map(Value::Int).to_vec(),
                    "MOUSECURSOR" => vec![flag(sys.mouse_cursor)],
                    // The emulator's window stands for the Switch on a TV.
                    "TVMODE" => vec![Value::Int(1)],
                    "TOOL1" | "TOOL2" | "TOOL3" => {
                        let n = usize::from(name.as_bytes()[4] - b'1');
                        vec![string(self.settings.tools[n].clone())]
                    }
                    _ => {
                        return Err(Error::with_detail(
                            ErrorKind::IllegalSystemArgumentName,
                            name,
                        ));
                    }
                };
                // Setting an item that can only be read, or asking for
                // another number of values than it has, is Wrong number of
                // OUT arguments (checked on hardware).
                let wrong =
                    || Error::with_detail(ErrorKind::WrongNumberOfOutArguments, name.clone());
                match (want, args.get(1)) {
                    (Want::Outs(0), Some(v)) => {
                        let on = int(v)? != 0;
                        match name.as_str() {
                            "SYSBEEP" => sys.sysbeep = on,
                            "MOUSECURSOR" => sys.mouse_cursor = on,
                            _ => return Err(wrong()),
                        }
                        Ok(vec![])
                    }
                    (_, None) if want.count() == values.len() => Ok(values),
                    _ => Err(wrong()),
                }
            }
            (Command::Fontinfo, Want::Outs(2), 2) => {
                let code = match &args[0] {
                    Value::Str(s) => {
                        (s.borrow().first().copied()).ok_or(ErrorKind::CantUseEmptyString)?
                    }
                    v => u16::try_from(int(v)?)
                        .map_err(|_| Error::with_detail(ErrorKind::OutOfRange, "character code"))?,
                };
                let size = match int(&args[1])? {
                    8 => FontSize::Small,
                    16 => FontSize::Large,
                    n => {
                        return Err(Error::with_detail(
                            ErrorKind::OutOfRange,
                            format!("font type {n}"),
                        ));
                    }
                };
                if USER_CHARS.contains(&code) {
                    return Err(ErrorKind::CantUseUserDefinedCharacter.into());
                }
                let (x, y) = self.font.origin(code, size).ok_or_else(|| {
                    Error::with_detail(ErrorKind::IllegalFunctionCall, "no glyph on GRPF")
                })?;
                Ok(vec![Value::Int(x as i32), Value::Int(y as i32)])
            }
            _ => Err(no_form()),
        }
    }

    /// What PERFEND has timed of each measurement since RUN.
    pub fn perf_stats(&self) -> &[PerfStats; 8] {
        &self.system.perf_stats
    }

    /// Forgets what PERFEND has timed, as RUN does.
    pub fn reset_perf_stats(&mut self) {
        self.system.perf_stats = [PerfStats::default(); 8];
    }

    /// Whether the running program shows the mouse cursor.
    pub fn mouse_cursor(&self) -> bool {
        self.system.mouse_cursor
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seeds_give_smilebasic_4s_numbers() {
        let take = |seed| {
            let mut s = Series::seeded(seed);
            (0..3).map(|_| s.next()).collect::<Vec<_>>()
        };
        assert_eq!(take(1), [2_545_341_989, 981_918_433, 3_715_302_833]);
        assert_ne!(take(1), take(2));
    }

    #[test]
    fn perf_stats_add_up() {
        let mut s = PerfStats::default();
        assert!(!s.used());
        assert_eq!(s.average(), 0);
        for micros in [10, 30, 20] {
            s.add(micros);
        }
        assert!(s.used());
        assert_eq!((s.count, s.last, s.max, s.total), (3, 20, 30, 60));
        assert_eq!(s.average(), 20);
    }
}
