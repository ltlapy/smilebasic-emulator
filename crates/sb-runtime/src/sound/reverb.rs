//! The effector: a reverb on a bus of its own that EFCWET and BGMWET send
//! part of each sound to. SmileBASIC 4 hands this to the console's audio
//! library; this is a small reverb of the emulator's own with the same
//! settings, early reflections from a few taps and a tail from four comb
//! filters and two all-pass filters a side, in the manner of Freeverb.
//! It sounds like a room, a hall or a cave, but not like the console's.

use super::RATE;

/// Settings as SmileBASIC 4 gives them to the console's reverb.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Settings {
    /// EFCSET's reflection mode, 0 to 4 (4 for none).
    pub(crate) early_mode: i32,
    pub(crate) early_gain: f32,
    /// Seconds before the reflections.
    pub(crate) predelay: f32,
    /// EFCSET's echo mode, 0 to 4.
    pub(crate) late_mode: i32,
    pub(crate) late_gain: f32,
    /// Seconds the tail takes to fall by 60 dB.
    pub(crate) decay: f32,
    /// How much of the treble each pass of the tail keeps, 0.1 to 1.
    pub(crate) hf_decay: f32,
    /// How dense the tail is, 0 to 1.
    pub(crate) coloration: f32,
    /// The reverb's input, its output, and how much of the bus passes by
    /// it unchanged.
    pub(crate) input: f32,
    pub(crate) output: f32,
    pub(crate) dry: f32,
}

impl Settings {
    /// What every preset starts from.
    const BASE: Settings = Settings {
        early_mode: 0,
        early_gain: 0.6,
        predelay: 0.0,
        late_mode: 0,
        late_gain: 0.4,
        decay: 1.0,
        hf_decay: 0.4,
        coloration: 0.6,
        input: 1.0,
        output: 0.5,
        dry: 0.1,
    };

    /// EFCSET's presets: #EFCBATH, #EFCCAVE and #EFCSPACE; #EFCOFF is none.
    pub(crate) fn preset(n: i32) -> Option<Settings> {
        let b = Settings::BASE;
        match n {
            1 => Some(Settings {
                decay: 0.5,
                hf_decay: 0.6,
                ..b
            }),
            2 => Some(Settings {
                early_mode: 3,
                late_mode: 2,
                decay: 2.0,
                early_gain: 0.3,
                hf_decay: 0.1,
                ..b
            }),
            3 => Some(Settings {
                early_mode: 3,
                late_mode: 3,
                predelay: 0.1,
                decay: 10.0,
                early_gain: 0.15,
                coloration: 0.8,
                ..b
            }),
            _ => None,
        }
    }

    /// EFCSET's nine values: reflection mode, volume and time (ms), echo
    /// mode, volume and time (ms), treble decay, tone and input volume.
    pub(crate) fn detailed(v: [f64; 9]) -> Settings {
        let f = |i: usize| v[i] as f32;
        Settings {
            early_mode: v[0] as i32,
            early_gain: f(1),
            predelay: f(2) / 1000.0,
            late_mode: v[3] as i32,
            late_gain: f(4),
            decay: f(5) / 1000.0,
            hf_decay: f(6),
            coloration: f(7),
            input: f(8),
            output: 0.5,
            dry: 0.0,
        }
    }
}

/// Reflections after the predelay, in ms, with their left and right gains.
const TAPS: [(f32, f32, f32); 6] = [
    (0.0, 0.84, 0.60),
    (4.3, 0.55, 0.78),
    (8.9, 0.62, 0.45),
    (13.1, 0.40, 0.52),
    (19.7, 0.33, 0.29),
    (27.2, 0.22, 0.26),
];
/// Freeverb's comb and all-pass delays, in ms, and how much the right side's
/// are longer.
const COMBS_MS: [f32; 4] = [25.3, 26.9, 29.0, 30.7];
const ALLPASSES_MS: [f32; 2] = [12.6, 5.1];
const SPREAD: usize = 23;
/// The tail's input, so that four combs ringing together stay near the
/// level of what goes in.
const COMB_INPUT: f32 = 0.06;
/// The longest predelay EFCSET takes, and the reflections after it.
const MAX_PREDELAY: f32 = 0.3;
const MAX_TAP: f32 = 0.08;

/// How far apart each reflection mode spreads its taps.
fn early_scale(mode: i32) -> Option<f32> {
    match mode {
        0 => Some(0.4),
        1 => Some(0.8),
        2 => Some(1.4),
        3 => Some(2.2),
        _ => None,
    }
}

/// How long each echo mode's combs are.
fn late_scale(mode: i32) -> f32 {
    match mode {
        0 => 0.7,
        1 => 1.0,
        2 => 0.5,
        3 => 1.4,
        _ => 0.85,
    }
}

fn samples(seconds: f32) -> usize {
    (seconds * RATE as f32) as usize
}

#[derive(Clone, Debug)]
struct Comb {
    buf: Vec<f32>,
    pos: usize,
    feedback: f32,
    damp: f32,
    store: f32,
}

impl Comb {
    fn new(len: usize, decay: f32, damp: f32) -> Comb {
        // A pass of `len` samples loses 60 dB over `decay` seconds.
        let feedback = 10f32.powf(-3.0 * len as f32 / (decay.max(0.01) * RATE as f32));
        Comb {
            buf: vec![0.0; len.max(1)],
            pos: 0,
            feedback,
            damp,
            store: 0.0,
        }
    }

    #[inline]
    fn process(&mut self, x: f32) -> f32 {
        let y = self.buf[self.pos];
        self.store = y + (self.store - y) * self.damp;
        self.buf[self.pos] = x + self.store * self.feedback;
        self.pos += 1;
        if self.pos == self.buf.len() {
            self.pos = 0;
        }
        y
    }
}

#[derive(Clone, Debug)]
struct Allpass {
    buf: Vec<f32>,
    pos: usize,
    gain: f32,
}

impl Allpass {
    #[inline]
    fn process(&mut self, x: f32) -> f32 {
        let b = self.buf[self.pos];
        self.buf[self.pos] = x + b * self.gain;
        self.pos += 1;
        if self.pos == self.buf.len() {
            self.pos = 0;
        }
        b - x
    }
}

#[derive(Clone, Debug)]
pub(crate) struct Reverb {
    settings: Settings,
    /// What went in, for the predelay and reflections.
    history: Vec<f32>,
    at: usize,
    /// The reflections' delays from now, in samples, and gains.
    taps: Vec<(usize, f32, f32)>,
    predelay: usize,
    combs: [Vec<Comb>; 2],
    allpasses: [Vec<Allpass>; 2],
}

impl Reverb {
    pub(crate) fn new(settings: Settings) -> Reverb {
        let s = settings;
        let predelay = samples(s.predelay.clamp(0.0, MAX_PREDELAY));
        let taps = match early_scale(s.early_mode) {
            Some(scale) => TAPS
                .iter()
                .map(|&(ms, l, r)| (predelay + samples(ms * scale / 1000.0), l, r))
                .collect(),
            None => Vec::new(),
        };
        let late = late_scale(s.late_mode);
        let damp = (1.0 - s.hf_decay.clamp(0.0, 1.0)) * 0.6;
        let combs = [0, SPREAD].map(|spread| {
            (COMBS_MS.iter())
                .map(|&ms| Comb::new(samples(ms * late / 1000.0) + spread, s.decay, damp))
                .collect()
        });
        let gain = 0.3 + 0.4 * s.coloration.clamp(0.0, 1.0);
        let allpasses = [0, SPREAD].map(|spread| {
            (ALLPASSES_MS.iter())
                .map(|&ms| Allpass {
                    buf: vec![0.0; samples(ms / 1000.0) + spread],
                    pos: 0,
                    gain,
                })
                .collect()
        });
        Reverb {
            settings,
            history: vec![0.0; samples(MAX_PREDELAY + MAX_TAP * early_scale(3).unwrap_or(1.0)) + 1],
            at: 0,
            taps,
            predelay,
            combs,
            allpasses,
        }
    }

    pub(crate) fn settings(&self) -> Settings {
        self.settings
    }

    /// Turns the bus's samples into what the effect adds to the mix: its
    /// output less what was sent, since the sound sent stays in the mix
    /// as it was.
    pub(crate) fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        let s = self.settings;
        let n = self.history.len();
        let (early, late) = (s.early_gain, s.late_gain);
        for (l, r) in left.iter_mut().zip(right.iter_mut()) {
            let input = (*l + *r) * 0.5 * s.input;
            self.history[self.at] = input;
            let back = |d: usize| (self.at + n - d % n) % n;
            let (mut el, mut er) = (0.0, 0.0);
            for &(d, gl, gr) in &self.taps {
                let x = self.history[back(d)];
                el += x * gl;
                er += x * gr;
            }
            let into = self.history[back(self.predelay)] * COMB_INPUT;
            let mut sides = [0.0f32; 2];
            for (side, out) in sides.iter_mut().enumerate() {
                let mut y = 0.0;
                for c in &mut self.combs[side] {
                    y += c.process(into);
                }
                for a in &mut self.allpasses[side] {
                    y = a.process(y);
                }
                *out = y;
            }
            self.at = (self.at + 1) % n;
            let wet_l = early * el + late * sides[0];
            let wet_r = early * er + late * sides[1];
            *l = *l * (s.dry - 1.0) + wet_l * s.output;
            *r = *r * (s.dry - 1.0) + wet_r * s.output;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_click_rings_on_and_dies_away() {
        let mut r = Reverb::new(Settings::preset(2).unwrap());
        let n = RATE as usize;
        let (mut l, mut rt) = (vec![0.0; n], vec![0.0; n]);
        l[0] = 1.0;
        rt[0] = 1.0;
        r.process(&mut l, &mut rt);
        let energy = |x: &[f32]| x.iter().map(|s| s * s).sum::<f32>();
        // Something after the click, well after it too.
        assert!(energy(&l[1..4800]) > 1e-4);
        assert!(energy(&l[24000..]) > 1e-7);
        // And less later than sooner: the tail falls.
        assert!(energy(&l[24000..]) < energy(&l[1..24000]));
        // A long silence later, it has died away.
        for _ in 0..10 {
            l.fill(0.0);
            rt.fill(0.0);
            r.process(&mut l, &mut rt);
        }
        assert!(energy(&l) < 1e-9);
    }

    #[test]
    fn a_steady_tone_stays_near_its_level() {
        for preset in 1..=3 {
            let mut r = Reverb::new(Settings::preset(preset).unwrap());
            let n = RATE as usize * 2;
            let tone: Vec<f32> = (0..n).map(|i| (i as f32 * 0.06).sin() * 0.5).collect();
            let (mut l, mut rt) = (tone.clone(), tone.clone());
            r.process(&mut l, &mut rt);
            let out: Vec<f32> = l.iter().zip(&tone).map(|(d, t)| d + t).collect();
            let rms = |x: &[f32]| (x.iter().map(|s| s * s).sum::<f32>() / x.len() as f32).sqrt();
            let ratio = rms(&out[n / 2..]) / rms(&tone[n / 2..]);
            assert!((0.05..2.0).contains(&ratio), "{preset} {ratio}");
        }
    }
}
