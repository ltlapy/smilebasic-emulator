//! Voices that are not in the SoundFont: the PSG's pulse waves (@216 to
//! @222), noise (@223) and the user waveforms of WAVSET and WAVSETA (@224
//! to @255), each with an envelope.

use std::rc::Rc;

use super::RATE;
use super::bank::Adshr;
use super::sampler;

/// A waveform: one cycle, or a sample with a loop.
#[derive(Clone, Debug, PartialEq)]
pub struct Wave {
    pub samples: Vec<f32>,
    /// The note the waveform plays at as it is.
    pub base_note: i32,
    /// Samples a second for a sample; `None` for a repeating wave, which
    /// steps through `cycle` samples in one cycle of its base note.
    pub rate: Option<f64>,
    pub cycle: f64,
    /// Where the loop starts and ends (exclusive).
    pub looped: (usize, usize),
    /// Whether BEEP loops it.
    pub beep_loop: bool,
    pub envelope: Envelope,
}

/// Attack, decay, sustain and release, each 0 to 127; larger times are
/// faster.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Envelope {
    pub attack: i32,
    pub decay: i32,
    pub sustain: i32,
    pub release: i32,
}

impl Envelope {
    pub const ORGAN: Envelope = Envelope {
        attack: 127,
        decay: 127,
        sustain: 127,
        release: 127,
    };
}

/// The values as SmileBASIC's own instruments take them: waveforms move
/// their level in the same steps.
// UNVERIFIED: that WAVSET's and @E's values mean what the sound archive's
// instruments' do.
impl From<Envelope> for Adshr {
    fn from(e: Envelope) -> Adshr {
        let v = |x: i32| x.clamp(0, 127) as u8;
        Adshr {
            attack: v(e.attack),
            decay: v(e.decay),
            sustain: v(e.sustain),
            hold: 0,
            release: v(e.release),
        }
    }
}

/// An 8-bit sample's unit, full scale as a 16-bit sample's.
// UNVERIFIED: that 8-bit and 16-bit samples share a scale.
const EIGHT_BIT: f32 = 1.0 / 128.0;

/// A user waveform from WAVSET or SmileBASIC 3's WAVSETA: 8-bit samples,
/// offset by 128. SmileBASIC 4 plays a pass of the samples as eight
/// cycles of the note, whatever their number: 32 samples of one cycle
/// sound three octaves down (TEST40), and songs written for SmileBASIC 4
/// repeat their cycles to fill eight. Past 512 samples the rate stays
/// that of 512, 28160 Hz at A, the 16-bit form's own default.
// UNVERIFIED: lengths other than 32, and the rate past 512 samples.
pub fn eight_bit(bytes: impl IntoIterator<Item = f64>, base_note: i32, envelope: Envelope) -> Wave {
    let samples: Vec<f32> = bytes
        .into_iter()
        .map(|b| (b as f32 - 128.0) * EIGHT_BIT)
        .collect();
    let len = samples.len();
    Wave {
        samples,
        base_note,
        rate: None,
        cycle: len.min(512) as f64 / 8.0,
        looped: (0, len),
        beep_loop: true,
        envelope,
    }
}

/// The PSG's pulse widths for @216 to @222, as eighths of a cycle.
// UNVERIFIED: SmileBASIC 4's PSG tones.
const PULSES: [usize; 7] = [1, 2, 3, 4, 5, 6, 7];

/// @216 to @222 as one cycle of a pulse wave, as loud as a full-scale
/// 8-bit waveform.
// UNVERIFIED: the PSG's and the noise's levels.
pub fn pulse(program: i32) -> Wave {
    let high = PULSES[(program - 216) as usize];
    Wave {
        samples: (0..8)
            .map(|i| if i < high { 127.0 } else { -128.0 } * EIGHT_BIT)
            .collect(),
        base_note: 69,
        rate: None,
        cycle: 8.0,
        looped: (0, 8),
        beep_loop: true,
        envelope: Envelope::ORGAN,
    }
}

/// @223: noise, read as one cycle of random samples so that higher notes
/// sound brighter.
pub fn noise() -> Wave {
    let mut x: u32 = 0x1234_5678;
    let samples = (0..4096)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            (x >> 16) as f32 / 32768.0 - 1.0
        })
        .map(|s| s * 128.0 * EIGHT_BIT)
        .collect();
    Wave {
        samples,
        base_note: 69,
        // The whole sound repeats too rarely to hear as a pitch.
        rate: Some(f64::from(RATE)),
        cycle: 4096.0,
        looped: (0, 4096),
        beep_loop: true,
        envelope: Envelope::ORGAN,
    }
}

/// The waveforms @216 to @255 play.
pub struct Waves {
    pulses: Vec<Rc<Wave>>,
    noise: Rc<Wave>,
    /// WAVSET's @224 to @255.
    pub user: Vec<Option<Rc<Wave>>>,
}

impl Default for Waves {
    fn default() -> Self {
        Waves {
            pulses: (216..=222).map(|p| Rc::new(pulse(p))).collect(),
            noise: Rc::new(noise()),
            user: vec![None; 32],
        }
    }
}

impl Waves {
    pub fn get(&self, program: i32) -> Option<Rc<Wave>> {
        match program {
            216..=222 => Some(self.pulses[(program - 216) as usize].clone()),
            223 => Some(self.noise.clone()),
            224..=255 => self.user[(program - 224) as usize].clone(),
            _ => None,
        }
    }
}

/// One note of a waveform.
#[derive(Clone, Debug)]
pub struct Voice {
    wave: Rc<Wave>,
    envelope: sampler::Envelope,
    pub key: i32,
    cents: i32,
    pos: f64,
    step: f64,
    /// The step before vibrato.
    base_step: f64,
    /// MML's vibrato depth, 0 to 127, and how far round it has gone.
    pub modulation: i32,
    vibrato_phase: f32,
    /// The envelope's gain, moving in a straight line to the next step's.
    level: f32,
    level_step: f32,
    target: f32,
    /// Samples into the envelope's step.
    in_tick: usize,
    /// Left and right gains.
    pub gains: (f32, f32),
    /// Plays the loop for ever: always in a song, and in BEEP when asked.
    looping: bool,
    done: bool,
}

impl Voice {
    /// `cents` moves the note's pitch.
    pub fn new(
        wave: Rc<Wave>,
        envelope: Option<Envelope>,
        key: i32,
        cents: i32,
        looping: bool,
    ) -> Voice {
        let envelope = sampler::Envelope::new(envelope.unwrap_or(wave.envelope).into());
        let mut v = Voice {
            wave,
            envelope,
            key,
            cents,
            pos: 0.0,
            step: 0.0,
            base_step: 0.0,
            modulation: 0,
            vibrato_phase: 0.0,
            level: 0.0,
            level_step: 0.0,
            target: 0.0,
            in_tick: 0,
            gains: (0.5, 0.5),
            looping,
            done: false,
        };
        v.set_pitch(cents);
        v
    }

    pub fn set_pitch(&mut self, cents: i32) {
        self.cents = cents;
        let ratio = 2f64
            .powf((f64::from(self.key - self.wave.base_note) * 100.0 + f64::from(cents)) / 1200.0);
        self.base_step = match self.wave.rate {
            Some(rate) => rate * ratio / f64::from(RATE),
            // A cycle at the base note is A=440 Hz shifted to the note.
            None => {
                let freq = 440.0 * 2f64.powf(f64::from(self.wave.base_note - 69) / 12.0);
                self.wave.cycle * freq * ratio / f64::from(RATE)
            }
        };
        self.step = self.base_step;
    }

    /// Goes on with `wave` in place of the one the note started with, as
    /// when WAVSET sets the instrument again while the note sounds.
    pub fn follow(&mut self, wave: &Rc<Wave>) {
        if !Rc::ptr_eq(&self.wave, wave) {
            self.wave = wave.clone();
            self.set_pitch(self.cents);
        }
    }

    pub fn release(&mut self) {
        self.envelope.release();
    }

    pub fn done(&self) -> bool {
        self.done
    }

    pub fn released(&self) -> bool {
        self.envelope.released()
    }

    /// Adds the voice into the buffers.
    pub fn render(&mut self, left: &mut [f32], right: &mut [f32]) {
        let wave = self.wave.clone();
        let samples = &wave.samples;
        if samples.is_empty() {
            self.done = true;
            return;
        }
        let (start, end) = wave.looped;
        let end = end.clamp(1, samples.len());
        let start = start.min(end - 1);
        let loops = wave.rate.is_none() || self.looping || wave.looped != (0, samples.len());
        for (l, r) in left.iter_mut().zip(right.iter_mut()) {
            if self.in_tick == 0 {
                self.envelope.step();
                self.target = if self.envelope.done() {
                    0.0
                } else {
                    self.envelope.gain()
                };
                self.level_step = (self.target - self.level) / sampler::TICK as f32;
                self.step = match sampler::vibrato(&mut self.vibrato_phase, self.modulation) {
                    0.0 => self.base_step,
                    cents => self.base_step * f64::from(2f32.powf(cents / 1200.0)),
                };
            }
            self.in_tick = (self.in_tick + 1) % sampler::TICK;
            if self.pos >= end as f64 {
                if loops {
                    self.pos = start as f64 + (self.pos - end as f64) % (end - start) as f64;
                } else {
                    self.done = true;
                    return;
                }
            }
            let s = samples[(self.pos as usize).min(samples.len() - 1)] * self.level;
            *l += s * self.gains.0;
            *r += s * self.gains.1;
            self.pos += self.step;
            self.level += self.level_step;
            if self.in_tick == 0 {
                // Land exactly on the step's level, which may be silence.
                self.level = self.target;
                if self.envelope.done() && self.level == 0.0 {
                    self.done = true;
                    return;
                }
            }
        }
    }
}

/// The gains for a volume, velocity and pan, each 0 to 127. Volume and
/// velocity are squared, as the instruments' are.
pub fn gains(volume: i32, velocity: i32, pan: i32) -> (f32, f32) {
    let v = WAVE_GAIN * sampler::square(volume) * sampler::square(velocity);
    let [l, r] = sampler::pan_gains(pan);
    (v * l, v * r)
}

/// A full-scale waveform at V127 in the middle records at an eighth of
/// full scale on each side (TEST40), the pan taking 3 dB.
const WAVE_GAIN: f32 = 0.125 * std::f32::consts::SQRT_2;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn envelopes_rise_hold_and_end() {
        let wave = Rc::new(pulse(219));
        let mut v = Voice::new(wave, None, 69, 0, false);
        let (mut l, mut r) = (vec![0.0; 480], vec![0.0; 480]);
        v.render(&mut l, &mut r);
        assert!(l.iter().any(|&s| s.abs() > 0.1));
        v.release();
        let (mut l, mut r) = (vec![0.0; 480], vec![0.0; 480]);
        v.render(&mut l, &mut r);
        assert!(v.done());
    }

    /// The loudest sample of `n` more of the voice.
    fn peak(v: &mut Voice, n: usize) -> f32 {
        let (mut l, mut r) = (vec![0.0; n], vec![0.0; n]);
        v.render(&mut l, &mut r);
        l.iter().fold(0f32, |m, x| m.max(x.abs()))
    }

    #[test]
    fn releases_take_as_long_as_the_instruments_do() {
        // @E127,116,111,32 rings on long after its note ends; a release
        // of 122, as songs give notes cut short with Q0, still leaves a
        // tail of a few dozen milliseconds.
        for (release, ms, sounding) in [(32, 500, true), (122, 30, true), (122, 400, false)] {
            let e = Envelope {
                attack: 127,
                decay: 116,
                sustain: 111,
                release,
            };
            let mut v = Voice::new(Rc::new(pulse(219)), Some(e), 69, 0, false);
            peak(&mut v, 960);
            v.release();
            peak(&mut v, ms * 48 - 480);
            assert_eq!(peak(&mut v, 480) > 0.001, sounding, "{release} {ms}");
        }
    }

    #[test]
    fn notes_follow_a_waveform_set_again() {
        let wave = |level: f32| {
            Rc::new(Wave {
                samples: vec![level; 16],
                ..pulse(219)
            })
        };
        let mut v = Voice::new(wave(0.5), None, 69, 0, false);
        let (mut l, mut r) = (vec![0.0; 960], vec![0.0; 960]);
        v.render(&mut l, &mut r);
        assert!(l[959] > 0.1);
        v.follow(&wave(-0.5));
        let (mut l, mut r) = (vec![0.0; 960], vec![0.0; 960]);
        v.render(&mut l, &mut r);
        assert!(l[0] < -0.1);
    }

    #[test]
    fn vibrato_moves_the_pitch_and_stops() {
        let mut v = Voice::new(Rc::new(pulse(219)), None, 69, 0, false);
        let steady = v.step;
        v.modulation = 127;
        let mut steps = Vec::new();
        for _ in 0..40 {
            peak(&mut v, sampler::TICK);
            steps.push(v.step);
        }
        assert!(steps.iter().any(|&s| s > steady * 1.01));
        assert!(steps.iter().any(|&s| s < steady * 0.99));
        v.modulation = 0;
        peak(&mut v, sampler::TICK);
        assert_eq!(v.step, steady);
    }

    #[test]
    fn waveforms_are_as_loud_as_instruments() {
        // Volume and velocity are squared, as the instruments' are.
        let (l, r) = gains(127, 64, 0);
        assert_eq!(r, 0.0);
        assert!((l - WAVE_GAIN * sampler::square(64)).abs() < 1e-6);
        let (l, r) = gains(127, 127, 64);
        assert!((l - 0.125).abs() < 1e-6 && (r - 0.125).abs() < 1e-6);
    }

    #[test]
    fn eight_bit_waveforms_play_eight_cycles_a_pass() {
        // 32 samples at A: a pass at 55 Hz, three octaves down.
        let w = eight_bit((0..32).map(f64::from), 69, Envelope::ORGAN);
        let v = Voice::new(Rc::new(w), None, 69, 0, false);
        assert!((v.step - 32.0 * 55.0 / 48000.0).abs() < 1e-9);
        // Past 512 samples, 28160 samples a second at the base note.
        let w = eight_bit((0..2048).map(|_| 128.0), 60, Envelope::ORGAN);
        let v = Voice::new(Rc::new(w), None, 60, 0, false);
        let rate = 28160.0 * 2f64.powf(-9.0 / 12.0);
        assert!((v.step - rate / 48000.0).abs() < 1e-9);
    }

    #[test]
    fn the_middle_is_even() {
        let (l, r) = gains(127, 127, 64);
        assert_eq!(l, r);
    }

    #[test]
    fn a_cycle_plays_at_the_note() {
        // 8 samples a cycle at 440 Hz: a step of 8 * 440 / 48000.
        let v = Voice::new(Rc::new(pulse(219)), None, 81, 0, false);
        assert!((v.step - 2.0 * 8.0 * 440.0 / 48000.0).abs() < 1e-9);
    }
}
