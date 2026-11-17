//! Animation of sprites and text screens (SPANIM, TANIM, ANIMDEF):
//! keyframes that set or slide one property, played one frame at a time.

use sb_core::error::{Error, ErrorKind, Result};
use sb_core::value::Value;

use crate::runtime::checking;

/// Keyframes in one animation, at most.
pub const MAX_KEYS: usize = 32;
/// ANIMDEF numbers 0-1023.
pub const DEFINITIONS: usize = 1024;
/// One animation per target can run on a sprite at a time.
pub const TARGETS: usize = 8;
/// The variable (SPVAR, TVAR) that target "V" animates.
pub const ANIM_VAR: i32 = 7;

/// What an animation changes, in SPCHK's bit order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    Xy,
    Z,
    R,
    S,
    C,
    V,
    Uv,
    I,
}

const NAMES: [(&str, Target); TARGETS] = [
    ("XY", Target::Xy),
    ("Z", Target::Z),
    ("R", Target::R),
    ("S", Target::S),
    ("C", Target::C),
    ("V", Target::V),
    ("UV", Target::Uv),
    ("I", Target::I),
];

impl Target {
    pub fn index(self) -> usize {
        self as usize
    }

    /// Values per keyframe besides the time.
    pub fn items(self) -> usize {
        match self {
            Target::Xy | Target::S | Target::Uv => 2,
            _ => 1,
        }
    }
}

/// The target argument: a number (8 adds "relative", 16 "delete at the
/// end") or a name such as `"XY"`, `"XY+"` or `"XY+."`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mode {
    pub target: Target,
    pub relative: bool,
    pub delete: bool,
}

fn bad_target(detail: String) -> Error {
    Error::with_detail(ErrorKind::OutOfRange, detail)
}

/// A name that is no target, such as `"X"`.
// UNVERIFIED: that a known name with flags it does not take (`"XY++"`)
// is refused the same way.
fn bad_name(detail: String) -> Error {
    Error::with_detail(ErrorKind::AnimationTypeMismatch, detail)
}

impl Mode {
    pub fn parse(v: &Value) -> Result<Mode> {
        checking(v, || {
            match v {
                Value::Str(s) => {
                    // Spaces after the name are left out (`"S "`).
                    let text = String::from_utf16_lossy(&s.borrow()).to_ascii_uppercase();
                    let text = text.trim_end_matches(' ');
                    let name = text.trim_end_matches(['+', '.']);
                    let flags = &text[name.len()..];
                    let target = NAMES
                        .iter()
                        .find(|(n, _)| *n == name)
                        .map(|&(_, t)| t)
                        .filter(|_| {
                            flags.matches('+').count() <= 1 && flags.matches('.').count() <= 1
                        })
                        .ok_or_else(|| bad_name(format!("animation target \"{text}\"")))?;
                    Ok(Mode {
                        target,
                        relative: flags.contains('+'),
                        delete: flags.contains('.'),
                    })
                }
                Value::Array(_) => Err(ErrorKind::AnimationTypeMismatch.into()),
                v => {
                    let n = v.to_int()?;
                    if !(0..32).contains(&n) {
                        return Err(bad_target(format!("animation target {n}")));
                    }
                    Ok(Mode {
                        target: NAMES[(n & 7) as usize].1,
                        relative: n & 8 != 0,
                        delete: n & 16 != 0,
                    })
                }
            }
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Key {
    /// Frames the key lasts; negative slides from the previous value.
    pub time: i32,
    pub values: [f64; 2],
}

#[derive(Clone, Debug, PartialEq)]
pub struct Anim {
    pub mode: Mode,
    pub keys: Vec<Key>,
    /// 0 repeats forever.
    pub loops: i32,
}

impl Anim {
    /// An animation from `time, item[, item]` groups.
    pub fn new(mode: Mode, data: &[f64], loops: i32) -> Result<Anim> {
        let per = 1 + mode.target.items();
        if data.is_empty() || !data.len().is_multiple_of(per) {
            return Err(Error::with_detail(
                ErrorKind::IllegalFunctionCall,
                format!("{per} values per key"),
            ));
        }
        if data.len() / per > MAX_KEYS {
            return Err(Error::with_detail(
                ErrorKind::AnimationTooLong,
                format!("{} keys (32 at most)", data.len() / per),
            ));
        }
        if loops < 0 {
            return Err(Error::with_detail(
                ErrorKind::OutOfRange,
                format!("loop count {loops}"),
            ));
        }
        // Whole-number targets are kept in 16 bits, and refuse more.
        if whole(mode.target)
            && let Some(v) = data
                .chunks_exact(per)
                .flat_map(|k| &k[1..])
                .find(|&&v| i16::try_from(v as i32).is_err())
        {
            return Err(Error::with_detail(
                ErrorKind::OutOfRange,
                format!("animation value {v}"),
            ));
        }
        let value = |v: f64| match whole(mode.target) {
            true => whole_value(v),
            false => v,
        };
        let keys: Vec<Key> = data
            .chunks_exact(per)
            .map(|k| Key {
                time: k[0] as i32,
                values: [value(k[1]), value(k.get(2).copied().unwrap_or(0.0))],
            })
            .collect();
        // Checked on hardware: a key of time 0 is refused.
        if let Some(i) = keys.iter().position(|k| k.time == 0) {
            return Err(Error::with_detail(
                ErrorKind::IllegalAnimationData,
                format!("key {} lasts 0 frames", i + 1),
            ));
        }
        Ok(Anim { mode, keys, loops })
    }
}

/// Something animations play on.
pub trait Animated {
    /// The current value of a target.
    fn value(&self, target: Target) -> [f64; 2];
    fn set_value(&mut self, target: Target, v: [f64; 2]);
}

fn channels(c: f64) -> [f64; 4] {
    (c as i64 as u32).to_be_bytes().map(f64::from)
}

fn from_channels(c: [f64; 4]) -> f64 {
    f64::from(u32::from_be_bytes(c.map(|x| x.clamp(0.0, 255.0) as u8)))
}

/// Whether a target's values are whole numbers, kept in 16 bits while it
/// plays.
fn whole(target: Target) -> bool {
    matches!(target, Target::Xy | Target::Z | Target::Uv | Target::I)
}

fn whole_value(v: f64) -> f64 {
    f64::from(v as i32 as i16)
}

/// `a` moved towards `b` on frame `elapsed` of `frames`, the two ends
/// weighted by the frames left and gone. Whole-number targets divide
/// their sum and drop the fraction: 0 to 3 over three frames goes 1, 2, 3.
/// Colors, rotation and scale mix in single precision, so that &HFF to
/// &HFF over three frames passes &HFF and &HFE, and colors round down.
/// "V" mixes in double precision.
fn mix(target: Target, a: [f64; 2], b: [f64; 2], (elapsed, frames): (u32, u32)) -> [f64; 2] {
    if whole(target) {
        let (left, gone) = (i64::from(frames - elapsed), i64::from(elapsed));
        return std::array::from_fn(|i| {
            ((a[i] as i64 * left + b[i] as i64 * gone) / i64::from(frames)) as f64
        });
    }
    if target == Target::V {
        let left = f64::from(frames - elapsed) / f64::from(frames);
        return [a[0] * left + b[0] * (1.0 - left), 0.0];
    }
    let left = (frames - elapsed) as f32 / frames as f32;
    let weigh = |a: f64, b: f64| a as f32 * left + b as f32 * (1.0 - left);
    if target == Target::C {
        let (a, b) = (channels(a[0]), channels(b[0]));
        let c = std::array::from_fn(|i| f64::from(weigh(a[i], b[i]).floor()));
        return [from_channels(c), 0.0];
    }
    [weigh(a[0], b[0]), weigh(a[1], b[1])].map(f64::from)
}

/// A relative key's value: the start value plus the key's, or for colors
/// the two multiplied per channel and divided by 256, rounding down
/// (checked on hardware: &HFF times &HFF gives &HFE).
fn offset(target: Target, base: [f64; 2], by: [f64; 2]) -> [f64; 2] {
    if target == Target::C {
        let (a, b) = (channels(base[0]), channels(by[0]));
        let c = std::array::from_fn(|i| (a[i] * b[i] / 256.0).floor());
        return [from_channels(c), 0.0];
    }
    let sum = [base[0] + by[0], base[1] + by[1]];
    match whole(target) {
        true => sum.map(whole_value),
        false => sum,
    }
}

/// An animation playing on a sprite or text screen.
#[derive(Clone, Debug, PartialEq)]
pub struct Track {
    anim: Anim,
    /// The value when the animation was set, which relative keys add to.
    base: [f64; 2],
    /// Where a sliding key starts: the value the previous key ended on.
    from: [f64; 2],
    key: usize,
    /// Frames of the current key played.
    elapsed: u32,
    loops_done: i32,
    /// Whether the last frame played was part of a slide.
    slid: bool,
}

impl Track {
    pub fn new(anim: Anim, on: &impl Animated) -> Track {
        let mut base = on.value(anim.mode.target);
        if whole(anim.mode.target) {
            base = base.map(whole_value);
        }
        Track {
            anim,
            base,
            from: base,
            key: 0,
            elapsed: 0,
            loops_done: 0,
            slid: false,
        }
    }

    pub fn mode(&self) -> Mode {
        self.anim.mode
    }

    pub fn slid(&self) -> bool {
        self.slid
    }

    fn key_value(&self) -> [f64; 2] {
        let values = self.anim.keys[self.key].values;
        if self.anim.mode.relative {
            offset(self.anim.mode.target, self.base, values)
        } else {
            values
        }
    }

    /// Moves to the next key; false once the last loop has ended.
    fn next_key(&mut self, ended_on: [f64; 2]) -> bool {
        self.from = ended_on;
        self.elapsed = 0;
        self.key += 1;
        if self.key == self.anim.keys.len() {
            self.key = 0;
            self.loops_done += 1;
            if self.anim.loops != 0 && self.loops_done >= self.anim.loops {
                return false;
            }
            // Each loop's first key slides from the value the animation
            // started on, not from where the last key ended.
            self.from = self.base;
        }
        true
    }

    /// Plays one frame; false once the animation has ended.
    // A key of time T shows its value (or the slide's steps) on T frames,
    // starting on the frame after SPANIM.
    pub fn step(&mut self, on: &mut impl Animated) -> bool {
        let target = self.anim.mode.target;
        let time = self.anim.keys[self.key].time;
        let frames = time.unsigned_abs();
        let to = self.key_value();
        self.elapsed += 1;
        self.slid = time < 0;
        let value = if time < 0 {
            mix(target, self.from, to, (self.elapsed, frames))
        } else {
            to
        };
        on.set_value(target, value);
        self.elapsed < frames || self.next_key(to)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sprites::{DEFS, Sprite, SpriteDef, WithDefs};

    fn mode(target: Target) -> Mode {
        Mode {
            target,
            relative: false,
            delete: false,
        }
    }

    fn sprite() -> Sprite {
        let mut s = crate::sprites::Sprites::default().list()[0].clone();
        s.used = true;
        s
    }

    fn play(track: &mut Track, s: &mut Sprite, frames: usize) -> Vec<(f64, f64, bool)> {
        let defs = vec![SpriteDef::default(); DEFS];
        (0..frames)
            .map(|_| {
                let going = track.step(&mut WithDefs::new(s, &defs));
                (s.pos.0, s.pos.1, going)
            })
            .collect()
    }

    #[test]
    fn parses_targets() {
        let s = |t: &str| Value::Str(sb_core::value::new_str(t.encode_utf16().collect()));
        let m = Mode::parse(&s("xy+.")).unwrap();
        assert_eq!((m.target, m.relative, m.delete), (Target::Xy, true, true));
        let m = Mode::parse(&Value::Int(6 + 16)).unwrap();
        assert_eq!((m.target, m.relative, m.delete), (Target::Uv, false, true));
        assert!(Mode::parse(&s("XY++")).is_err());
        assert!(Mode::parse(&s("XY..")).is_err());
        assert!(Mode::parse(&s("Q")).is_err());
        assert!(Mode::parse(&Value::Int(32)).is_err());
    }

    #[test]
    fn checks_the_data() {
        assert!(Anim::new(mode(Target::Xy), &[1.0, 2.0], 1).is_err());
        assert!(Anim::new(mode(Target::Z), &[], 1).is_err());
        assert!(Anim::new(mode(Target::Z), &[1.0; 66], 1).is_err());
        assert!(Anim::new(mode(Target::Z), &[1.0; 64], 1).is_ok());
        assert!(Anim::new(mode(Target::Z), &[1.0, 1.0], -1).is_err());
        let zero = Anim::new(mode(Target::Z), &[1.0, 1.0, 0.0, 2.0], 1).unwrap_err();
        assert_eq!(zero.kind, ErrorKind::IllegalAnimationData);
    }

    #[test]
    fn holds_then_slides() {
        let anim = Anim::new(mode(Target::Xy), &[2.0, 10.0, 20.0, -2.0, 20.0, 40.0], 1).unwrap();
        let mut s = sprite();
        let mut t = Track::new(anim, &WithDefs::new(&mut s, &[]));
        assert_eq!(
            play(&mut t, &mut s, 4),
            [
                (10.0, 20.0, true),
                (10.0, 20.0, true),
                (15.0, 30.0, true),
                (20.0, 40.0, false),
            ]
        );
    }

    #[test]
    fn relative_keys_add_to_the_start_and_loops_repeat() {
        let anim = Anim::new(
            Mode {
                relative: true,
                ..mode(Target::Xy)
            },
            &[-2.0, 10.0, 0.0, 1.0, 0.0, 0.0],
            2,
        )
        .unwrap();
        let mut s = sprite();
        s.pos = (100.0, 50.0);
        let mut t = Track::new(anim, &WithDefs::new(&mut s, &[]));
        // The second key snaps back for a frame, and the next loop slides
        // from the start again.
        assert_eq!(
            play(&mut t, &mut s, 6),
            [
                (105.0, 50.0, true),
                (110.0, 50.0, true),
                (100.0, 50.0, true),
                (105.0, 50.0, true),
                (110.0, 50.0, true),
                (100.0, 50.0, false),
            ]
        );
    }

    /// TEST74: on every loop, the first key slides again from the value
    /// the animation started on.
    #[test]
    fn loops_slide_from_the_start_value() {
        let anim = Anim::new(mode(Target::I), &[-4.0, 1.0, -4.0, 2.0, -4.0, 3.0], 0).unwrap();
        let defs = vec![SpriteDef::default(); DEFS];
        let mut s = sprite();
        let mut t = Track::new(anim, &WithDefs::new(&mut s, &defs));
        let seen: Vec<i32> = (0..24)
            .map(|_| {
                t.step(&mut WithDefs::new(&mut s, &defs));
                WithDefs::new(&mut s, &defs).value(Target::I)[0] as i32
            })
            .collect();
        let samples: Vec<i32> = seen.iter().skip(1).step_by(2).copied().collect();
        assert_eq!(samples, [0, 1, 1, 2, 2, 3, 0, 1, 1, 2, 2, 3]);
    }

    #[test]
    fn colors_slide_per_channel() {
        let anim = Anim::new(mode(Target::C), &[-2.0, f64::from(0xFF00_0000u32)], 0).unwrap();
        let mut s = sprite();
        s.color = 0x80FF_FF00;
        let defs = vec![SpriteDef::default(); DEFS];
        let mut t = Track::new(anim, &WithDefs::new(&mut s, &defs));
        assert!(t.step(&mut WithDefs::new(&mut s, &defs)));
        assert_eq!(s.color, 0xBF7F_7F00);
    }

    /// TEST56: whole-number targets divide, so a slide that lands on whole
    /// numbers shows them, and "V" keeps its fractions.
    #[test]
    fn whole_targets_slide_in_whole_numbers() {
        let anim = Anim::new(mode(Target::Xy), &[-3.0, 3.0, 7.0], 0).unwrap();
        let defs = vec![SpriteDef::default(); DEFS];
        let mut s = sprite();
        let mut t = Track::new(anim, &WithDefs::new(&mut s, &defs));
        let seen: Vec<_> = play(&mut t, &mut s, 3).iter().map(|p| (p.0, p.1)).collect();
        assert_eq!(seen, [(1.0, 2.0), (2.0, 4.0), (3.0, 7.0)]);
        assert_eq!(
            mix(Target::V, [0.0, 0.0], [1.0, 0.0], (1, 3))[0],
            1.0 - 2.0 / 3.0
        );
        assert_eq!(
            mix(Target::R, [0.0, 0.0], [3.0, 0.0], (1, 3))[0],
            f64::from(3.0f32 * (1.0 - 2.0f32 / 3.0))
        );
        // Keys and the start are cut to whole numbers.
        let anim = Anim::new(mode(Target::Xy), &[1.0, 10.75, -2.5], 1).unwrap();
        assert_eq!(anim.keys[0].values, [10.0, -2.0]);
    }

    /// TEST47 part 20, read back on hardware: over three frames, X goes
    /// 0, -3, -6, -10 and the colours' alpha dips to &HFE on the second.
    #[test]
    fn slides_round_like_hardware() {
        let anim = Anim::new(mode(Target::Xy), &[-3.0, -10.0, -10.0], 0).unwrap();
        let defs = vec![SpriteDef::default(); DEFS];
        let mut s = sprite();
        let mut t = Track::new(anim, &WithDefs::new(&mut s, &defs));
        let xs: Vec<f64> = play(&mut t, &mut s, 3).iter().map(|p| p.0).collect();
        assert_eq!(xs, [-3.0, -6.0, -10.0]);
        for (from, to, seen) in [
            (
                0xFF00_0000u32,
                0xFF0A_0A0Au32,
                [0xFF03_0303u32, 0xFE06_0606, 0xFF0A_0A0A],
            ),
            (
                0xFF0A_0A0A,
                0xFF00_0000,
                [0xFF06_0606, 0xFE03_0303, 0xFF00_0000],
            ),
        ] {
            let anim = Anim::new(mode(Target::C), &[-3.0, f64::from(to)], 0).unwrap();
            let mut s = sprite();
            s.color = from;
            let mut t = Track::new(anim, &WithDefs::new(&mut s, &defs));
            for expected in seen {
                t.step(&mut WithDefs::new(&mut s, &defs));
                assert_eq!(s.color, expected, "{from:X}");
            }
        }
    }
}
