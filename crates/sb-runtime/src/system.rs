//! Random numbers, time and the system's settings: RND, RNDF, RANDOMIZE,
//! MILLISEC, MAINCNT, DATE$, TIME$, DTREAD, TMREAD and SYSPARAM.

use std::time::{Instant, SystemTime, UNIX_EPOCH};

use chrono::{Datelike, Local, NaiveDate, NaiveTime, Timelike};
use sb_core::bytecode::Want;
use sb_core::error::{Error, ErrorKind, Result};
use sb_core::value::{Value, new_str};

use crate::runtime::{Command, Runtime, illegal};

/// RND's series 0 to 7.
const SERIES: usize = 8;

/// One series of random numbers.
// UNVERIFIED: SmileBASIC 4's generator, so seeded series differ from the
// hardware's.
#[derive(Clone, Copy, Debug)]
struct Series(u64);

impl Series {
    fn seeded(seed: u64) -> Series {
        // SplitMix64 spreads close seeds apart; zero would stay zero.
        let mut z = seed.wrapping_add(0x9E37_79B9_7F4A_7C15);
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        Series((z ^ (z >> 31)) | 1)
    }

    /// xorshift64*.
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
}

/// A seed from the clock, different for each call.
fn entropy() -> u64 {
    static CALLS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos() as u64);
    nanos ^ CALLS.fetch_add(1, std::sync::atomic::Ordering::Relaxed) << 48
}

pub struct System {
    series: [Series; SERIES],
    started: Instant,
    /// Frames since start-up, for MAINCNT.
    pub(crate) frames: u32,
    /// SYSPARAM "SYSBEEP": the system's own sounds play. The emulator has
    /// none, so this is only kept.
    sysbeep: bool,
    /// SYSPARAM "MOUSECURSOR": programs show the mouse cursor.
    pub mouse_cursor: bool,
}

/// SYSPARAM "TOOL1" to "TOOL3": the programs F9 to F11 start, as
/// SmileBASIC 4's settings start.
const TOOLS: [&str; 3] = ["#TOOL/SBSMILE.PRG", "#TOOL/GAHAKU.PRG", ""];

/// SYSPARAM "KEYREPEAT": frames before a held key repeats, and between
/// repeats, as SmileBASIC 4's settings start.
const KEY_REPEAT: [i32; 2] = [20, 3];

impl Default for System {
    fn default() -> Self {
        System {
            series: std::array::from_fn(|_| Series::seeded(entropy())),
            started: Instant::now(),
            frames: 0,
            sysbeep: true,
            mouse_cursor: false,
        }
    }
}

fn int(v: &Value) -> Result<i32> {
    v.to_int()
}

fn series_id(v: &Value) -> Result<usize> {
    let n = int(v)?;
    usize::try_from(n)
        .ok()
        .filter(|&n| n < SERIES)
        .ok_or_else(|| Error::with_detail(ErrorKind::OutOfRange, format!("series {n}")))
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
                // RND(0) is 0 and a negative maximum Out of range, as on
                // hardware.
                // UNVERIFIED: RND(0) still takes a number from the series.
                if max < 0 {
                    return Err(Error::with_detail(
                        ErrorKind::OutOfRange,
                        format!("maximum {max}"),
                    ));
                }
                let r = ((sys.series[id].next() >> 32) * max as u64) >> 32;
                Ok(vec![Value::Int(r as i32)])
            }
            (Command::Rndf, Want::Value, 0..=1) => {
                let id = args.first().map_or(Ok(0), series_id)?;
                // 53 random bits: 0 up to but not including 1.
                let r = (sys.series[id].next() >> 11) as f64 / (1u64 << 53) as f64;
                Ok(vec![Value::Real(r)])
            }
            (Command::Randomize, Want::Outs(0), 1..=2) => {
                let id = series_id(&args[0])?;
                let seed = match args.get(1) {
                    None | Some(Value::Default) => 0,
                    Some(v) => int(v)?,
                };
                sys.series[id] = Series::seeded(match seed {
                    0 => entropy(),
                    n => n as u32 as u64,
                });
                Ok(vec![])
            }
            (Command::Millisec, Want::Value, 0) => {
                Ok(vec![Value::Int(sys.started.elapsed().as_millis() as i32)])
            }
            (Command::Maincnt, Want::Value, 0) => Ok(vec![Value::Int(sys.frames as i32)]),
            (Command::Date, Want::Value, 0) => {
                Ok(vec![string(Local::now().format("%Y/%m/%d").to_string())])
            }
            (Command::Time, Want::Value, 0) => {
                Ok(vec![string(Local::now().format("%H:%M:%S").to_string())])
            }
            (Command::Dtread, Want::Outs(3 | 4), 0..=1) => {
                let date = match args.first() {
                    None => Local::now().date_naive(),
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
                    None => Local::now().time(),
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
                        vec![string(TOOLS[n].to_string())]
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
            _ => Err(illegal()),
        }
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
    fn seeds_repeat_and_differ() {
        let take = |seed| {
            let mut s = Series::seeded(seed);
            (0..4).map(|_| s.next()).collect::<Vec<_>>()
        };
        assert_eq!(take(1), take(1));
        assert_ne!(take(1), take(2));
    }
}
