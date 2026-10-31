//! Animation of sprites and text screens (SPANIM, TANIM, ANIMDEF):
//! keyframes that set or slide one property, played one frame at a time.

use sb_core::error::{Error, ErrorKind, Result};
use sb_core::value::Value;

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

impl Mode {
    pub fn parse(v: &Value) -> Result<Mode> {
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
                    .filter(|_| flags.matches('+').count() <= 1 && flags.matches('.').count() <= 1)
                    .ok_or_else(|| bad_target(format!("animation target \"{text}\"")))?;
                Ok(Mode {
                    target,
                    relative: flags.contains('+'),
                    delete: flags.contains('.'),
                })
            }
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
                ErrorKind::OutOfRange,
                format!("{} keys (32 at most)", data.len() / per),
            ));
        }
        if loops < 0 {
            return Err(Error::with_detail(
                ErrorKind::OutOfRange,
                format!("loop count {loops}"),
            ));
        }
        let keys: Vec<Key> = data
            .chunks_exact(per)
            .map(|k| Key {
                time: k[0] as i32,
                values: [k[1], k.get(2).copied().unwrap_or(0.0)],
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

/// `a` moved towards `b` by the fraction `f`; colors move per channel.
fn mix(target: Target, a: [f64; 2], b: [f64; 2], f: f64) -> [f64; 2] {
    if target == Target::C {
        let (a, b) = (channels(a[0]), channels(b[0]));
        let c = std::array::from_fn(|i| a[i] + (b[i] - a[i]) * f);
        return [from_channels(c), 0.0];
    }
    [a[0] + (b[0] - a[0]) * f, a[1] + (b[1] - a[1]) * f]
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
    [base[0] + by[0], base[1] + by[1]]
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
        let base = on.value(anim.mode.target);
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
        }
        true
    }

    /// Plays one frame; false once the animation has ended.
    // UNVERIFIED: a key of time T shows its value (or the slide's steps)
    // on T frames, starting on the frame after SPANIM; looping slides
    // from where the last key ended.
    pub fn step(&mut self, on: &mut impl Animated) -> bool {
        let target = self.anim.mode.target;
        let time = self.anim.keys[self.key].time;
        let frames = time.unsigned_abs();
        let to = self.key_value();
        self.elapsed += 1;
        self.slid = time < 0;
        let value = if time < 0 {
            mix(
                target,
                self.from,
                to,
                f64::from(self.elapsed) / f64::from(frames),
            )
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
        let mut s = crate::sprites::Sprites::default().list[0].clone();
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
        // from there.
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
}
