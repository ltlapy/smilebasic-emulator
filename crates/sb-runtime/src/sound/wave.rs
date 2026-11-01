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

/// Samples a second 8-bit waveforms play at their reference pitch.
pub const EIGHT_BIT_RATE: f64 = 28160.0;

/// A user waveform from WAVSET or WAVSETA's 8-bit form: samples offset
/// by 128, stretched to `pass` samples by taking the one at or before
/// each, and played at [`EIGHT_BIT_RATE`] at the base note. WAVSET
/// stretches to 512 samples, so a pass of any length is 55 Hz; WAVSETA
/// to the next power of two.
pub fn eight_bit(bytes: &[f64], pass: usize, base_note: i32, envelope: Envelope) -> Wave {
    let n = bytes.len();
    let samples: Vec<f32> = (0..pass)
        .map(|i| (bytes[i * n / pass] as f32 - 128.0) * EIGHT_BIT)
        .collect();
    Wave {
        samples,
        base_note,
        rate: Some(EIGHT_BIT_RATE),
        cycle: 0.0,
        looped: (0, pass),
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
    /// The channel's @MP, @MA and @ML, and how far round they have gone.
    pub(crate) lfo: [sampler::Lfo; 3],
    lfos: sampler::Lfos,
    /// The gain from volume and velocity, and the pan's position.
    gain: f32,
    pan: f32,
    /// Left and right gains, envelope and all, moving in a straight line
    /// to the next step's.
    levels: [f32; 2],
    steps: [f32; 2],
    targets: [f32; 2],
    /// Samples into the envelope's step.
    in_tick: usize,
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
            lfo: [sampler::Lfo::default(); 3],
            lfos: sampler::Lfos::default(),
            gain: WAVE_GAIN,
            pan: 0.0,
            levels: [0.0; 2],
            steps: [0.0; 2],
            targets: [0.0; 2],
            in_tick: 0,
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
            // A cycle at the base note is A=440 Hz: a waveform with base
            // note 57 sounds an octave higher.
            None => self.wave.cycle * 440.0 * ratio / f64::from(RATE),
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

    /// Sets the volume, velocity and pan, each 0 to 127. Volume and
    /// velocity are squared, as the instruments' are.
    pub fn set_mix(&mut self, volume: i32, velocity: i32, pan: i32) {
        self.gain = WAVE_GAIN * sampler::square(volume) * sampler::square(velocity);
        self.pan = sampler::pan_position(pan);
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
                let (cents, tremolo, pan) = self.lfos.step(&self.lfo);
                let level = if self.envelope.done() {
                    0.0
                } else {
                    self.envelope.gain() * self.gain * tremolo
                };
                let [gl, gr] = sampler::pan_gains(self.pan + pan);
                self.targets = [level * gl, level * gr];
                if self.pos == 0.0 {
                    // The note starts at its first step's level: a square
                    // with attack 127 is at full level from its first
                    // sample (TEST40).
                    self.levels = self.targets;
                }
                let tick = sampler::TICK as f32;
                self.steps = [0, 1].map(|i| (self.targets[i] - self.levels[i]) / tick);
                self.step = match cents {
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
            let s = samples[(self.pos as usize).min(samples.len() - 1)];
            *l += s * self.levels[0];
            *r += s * self.levels[1];
            self.pos += self.step;
            self.levels[0] += self.steps[0];
            self.levels[1] += self.steps[1];
            if self.in_tick == 0 {
                // Land exactly on the step's level, which may be silence.
                self.levels = self.targets;
                if self.envelope.done() && self.levels == [0.0; 2] {
                    self.done = true;
                    return;
                }
            }
        }
    }
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
        assert!(l[959] > 0.02);
        v.follow(&wave(-0.5));
        let (mut l, mut r) = (vec![0.0; 960], vec![0.0; 960]);
        v.render(&mut l, &mut r);
        assert!(l[0] < -0.02);
    }

    #[test]
    fn vibrato_moves_the_pitch_and_stops() {
        let mut v = Voice::new(Rc::new(pulse(219)), None, 69, 0, false);
        let steady = v.step;
        // @MP32,1,20,0: ±25 cents, 7.8 times a second.
        v.lfo[0] = sampler::Lfo {
            depth: 32,
            range: 1,
            speed: 20,
            delay: 0,
        };
        let mut cents = Vec::new();
        for _ in 0..26 {
            peak(&mut v, sampler::TICK);
            cents.push(1200.0 * (v.step / steady).log2());
        }
        let top = cents.iter().cloned().fold(f64::MIN, f64::max);
        let bottom = cents.iter().cloned().fold(f64::MAX, f64::min);
        assert!((top - 25.0).abs() < 0.5 && (bottom + 25.0).abs() < 0.5);
        v.lfo[0] = sampler::Lfo::default();
        peak(&mut v, sampler::TICK);
        assert_eq!(v.step, steady);
    }

    /// The RMS of the left and right sides over `n` samples.
    fn rms(v: &mut Voice, n: usize) -> (f32, f32) {
        let (mut l, mut r) = (vec![0.0; n], vec![0.0; n]);
        v.render(&mut l, &mut r);
        let rms = |x: &[f32]| (x.iter().map(|s| s * s).sum::<f32>() / n as f32).sqrt();
        (rms(&l), rms(&r))
    }

    #[test]
    fn tremolo_and_pan_waves_swing_the_level() {
        let lfo = sampler::Lfo {
            depth: 24,
            range: 2,
            speed: 7,
            delay: 0,
        };
        let swing = |target: usize| {
            let mut v = Voice::new(Rc::new(pulse(219)), None, 69, 0, false);
            v.set_mix(127, 127, 64);
            let (middle, _) = rms(&mut v, 480);
            v.lfo[target] = lfo;
            let sides: Vec<_> = (0..80).map(|_| rms(&mut v, 480)).collect();
            let l = sides.iter().map(|s| s.0 / middle);
            let r = sides.iter().map(|s| s.1 / middle);
            (
                l.clone().fold(f32::MAX, f32::min),
                l.fold(0f32, f32::max),
                r.fold(0f32, f32::max),
            )
        };
        // @MA24,2,7: ±2.25 dB on both sides.
        let (low, high, right) = swing(1);
        assert!((low - 0.772).abs() < 0.02 && (high - 1.296).abs() < 0.02);
        assert!((right - high).abs() < 0.01);
        // @ML24,2,7: each side between about 0.79 and 1.17 of the middle.
        let (low, high, _) = swing(2);
        assert!((low - 0.79).abs() < 0.02 && (high - 1.17).abs() < 0.02);
    }

    #[test]
    fn lfos_wait_out_their_delay() {
        let mut v = Voice::new(Rc::new(pulse(219)), None, 69, 0, false);
        let steady = v.step;
        v.lfo[0] = sampler::Lfo {
            depth: 127,
            range: 1,
            speed: 127,
            delay: 4800,
        };
        peak(&mut v, 4800 - sampler::TICK);
        assert_eq!(v.step, steady);
        peak(&mut v, 4 * sampler::TICK);
        assert_ne!(v.step, steady);
    }

    #[test]
    fn waveforms_are_as_loud_as_measured() {
        // A full-scale square at V127 in the middle: an eighth of full
        // scale on each side.
        let bytes: Vec<f64> = (0..32).map(|i| if i < 16 { 0.0 } else { 256.0 }).collect();
        let w = eight_bit(&bytes, 512, 69, Envelope::ORGAN);
        let mut v = Voice::new(Rc::new(w), None, 69, 0, true);
        v.set_mix(127, 127, 64);
        let (l, r) = rms(&mut v, 4800);
        assert!((l - 0.125).abs() < 0.001 && l == r);
        // Volume and velocity are squared.
        v.set_mix(127, 64, 0);
        rms(&mut v, 480);
        let (l, r) = rms(&mut v, 4800);
        let hard_left = 0.125 * std::f32::consts::SQRT_2 * sampler::square(64);
        assert!((l - hard_left).abs() < 0.001 && r == 0.0);
    }

    #[test]
    fn eight_bit_waveforms_play_a_pass_at_their_rate() {
        // WAVSET's 32 samples at A, stretched to 512: a pass at 55 Hz,
        // three octaves down.
        let bytes: Vec<f64> = (0..32).map(f64::from).collect();
        let w = eight_bit(&bytes, 512, 69, Envelope::ORGAN);
        assert_eq!(w.samples[15], w.samples[0]);
        assert_ne!(w.samples[16], w.samples[0]);
        let v = Voice::new(Rc::new(w), None, 69, 0, false);
        assert!((v.step - 512.0 * 55.0 / 48000.0).abs() < 1e-9);
        // The base note moves it: at 57, the same A sounds an octave up.
        let w = eight_bit(&bytes, 512, 57, Envelope::ORGAN);
        let v = Voice::new(Rc::new(w), None, 69, 0, false);
        assert!((v.step - 512.0 * 110.0 / 48000.0).abs() < 1e-9);
        // WAVSETA's 32 samples stay 32: a pass at 880 Hz.
        let w = eight_bit(&bytes, 32, 69, Envelope::ORGAN);
        let v = Voice::new(Rc::new(w), None, 69, 0, false);
        assert!((v.step - 32.0 * 880.0 / 48000.0).abs() < 1e-9);
    }

    #[test]
    fn a_cycle_plays_at_the_note() {
        // 8 samples a cycle at 440 Hz: a step of 8 * 440 / 48000.
        let v = Voice::new(Rc::new(pulse(219)), None, 81, 0, false);
        assert!((v.step - 2.0 * 8.0 * 440.0 / 48000.0).abs() < 1e-9);
    }
}
