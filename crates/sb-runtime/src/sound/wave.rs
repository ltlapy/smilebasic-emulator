//! Voices that are not in the SoundFont: the PSG's pulse waves (@216 to
//! @222), noise (@223) and the user waveforms of WAVSET and WAVSETA (@224
//! to @255), each with an envelope.

use std::cell::OnceCell;
use std::rc::Rc;
use std::sync::Arc;

use super::RATE;
use super::bank::{Adshr, Bank, Reader};
use super::sampler;

/// A waveform: one cycle, or a sample with a loop.
#[derive(Clone, Debug, PartialEq)]
pub struct Wave {
    /// The instrument's wave memory: this definition's samples, up to the
    /// end of its loop, then what earlier ones left past that.
    pub samples: Vec<f32>,
    /// The note the waveform plays at as it is.
    pub base_note: i32,
    /// Samples a second for a sample; `None` for a repeating wave, which
    /// steps through `cycle` samples in one cycle of its base note.
    pub rate: Option<f64>,
    pub cycle: f64,
    /// Where the loop starts and ends (exclusive); the end is also where
    /// the definition ends.
    pub looped: (usize, usize),
    /// Whether BEEP's note loops it.
    pub beep_loop: bool,
    /// How long BEEP holds its note, in [`BEEP_TICKS_A_SECOND`]; `None` for
    /// as long as it is let. 0 plays nothing.
    pub beep_ticks: Option<u32>,
    /// Whether BEEP plays one such note after another until stopped.
    pub beep_repeats: bool,
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
/// their level in the same steps, with no hold.
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

/// What BEEP's notes of a user waveform are counted in.
pub const BEEP_TICKS_A_SECOND: u32 = 192;

/// BEEP's note for a waveform of `samples` at `rate`: as long as a pass of
/// it, cut to whole ticks, so too short a waveform has no note.
pub fn beep_ticks(samples: usize, rate: u32) -> u32 {
    (samples as u64 * u64::from(BEEP_TICKS_A_SECOND) / u64::from(rate.max(1)))
        .min(u64::from(u32::MAX)) as u32
}

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
        beep_ticks: Some(beep_ticks(pass, EIGHT_BIT_RATE as u32)),
        beep_repeats: false,
        envelope,
    }
}

/// The PSG's pulse widths for @216 to @222, as eighths of a cycle.
// UNVERIFIED: the widths of SmileBASIC 4's PSG tones.
const PULSES: [usize; 7] = [1, 2, 3, 4, 5, 6, 7];

/// @216 to @222 as one cycle of a pulse wave, as loud as a full-scale
/// 8-bit waveform, which SmileBASIC 4's own are near (TEST41). They play
/// when the instruments do not come from SmileBASIC 4's sound archive.
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
        beep_ticks: None,
        beep_repeats: false,
        envelope: Envelope::ORGAN,
    }
}

/// @223: noise, read as one cycle of random samples so that higher notes
/// sound brighter. SmileBASIC 4's own noise steps between silence and
/// full scale, never below (TEST41), so this does too.
pub fn noise() -> Wave {
    let mut x: u32 = 0x1234_5678;
    let samples = (0..4096)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            if x & 0x8000 != 0 { 1.0 } else { 0.0 }
        })
        .collect();
    Wave {
        samples,
        base_note: 69,
        // The whole sound repeats too rarely to hear as a pitch.
        rate: Some(f64::from(RATE)),
        cycle: 4096.0,
        looped: (0, 4096),
        beep_loop: true,
        beep_ticks: None,
        beep_repeats: false,
        envelope: Envelope::ORGAN,
    }
}

/// What @224 to @255 play until WAVSET or WAVSETA sets them: the sound
/// archive holds a sine for @224, and SmileBASIC 4 starts all of them on that
/// one, with a second's note for BEEP at the sine's own pitch. `None` without
/// the sound archive's bank. The archive also keeps a shorter copy of the sine
/// for notes from key 96 up, which sounds the same at the same pitch.
pub fn unset(bank: &Bank) -> Option<Wave> {
    let region = bank.region(224, 57)?;
    let info = bank.waves.get(usize::from(region.wave))?;
    let mut reader = Reader::default();
    let samples: Vec<f32> = (0..info.len)
        .map_while(|_| reader.next(info, &bank.data))
        .map(|s| f32::from(s) / 32768.0)
        .collect();
    let end = samples.len();
    let e = region.envelope;
    let v = i32::from;
    Some(Wave {
        samples,
        base_note: i32::from(region.original_key),
        rate: Some(f64::from(info.rate)),
        cycle: 0.0,
        looped: (info.loop_start.map_or(0, |s| s as usize), end),
        beep_loop: info.loop_start.is_some(),
        beep_ticks: Some(BEEP_TICKS_A_SECOND),
        beep_repeats: false,
        envelope: Envelope {
            attack: v(e.attack),
            decay: v(e.decay),
            sustain: v(e.sustain),
            release: v(e.release),
        },
    })
}

/// The waveforms @216 to @255 play.
pub struct Waves {
    pulses: Vec<Rc<Wave>>,
    noise: Rc<Wave>,
    /// WAVSET's @224 to @255.
    pub user: Vec<Option<Rc<Wave>>>,
    /// The sound archive's bank, for what they play until set.
    bank: Option<Arc<Bank>>,
    /// What they play until set, decoded once it is needed.
    unset: OnceCell<Option<Rc<Wave>>>,
}

impl Default for Waves {
    fn default() -> Self {
        Waves {
            pulses: (216..=222).map(|p| Rc::new(pulse(p))).collect(),
            noise: Rc::new(noise()),
            user: vec![None; 32],
            bank: None,
            unset: OnceCell::new(),
        }
    }
}

impl Waves {
    /// Where @224 to @255 find the sine they play until set, if anywhere.
    pub(crate) fn set_bank(&mut self, bank: Option<Arc<Bank>>) {
        self.bank = bank;
        self.unset = OnceCell::new();
    }

    fn unset(&self) -> Option<&Rc<Wave>> {
        self.unset
            .get_or_init(|| unset(self.bank.as_ref()?).map(Rc::new))
            .as_ref()
    }

    /// Sets WAVSET's instrument `n` (0 for @224) to `wave`. Its samples
    /// overwrite the instrument's memory from the start, and notes that
    /// sound go on reading it with the length, loop and rate they started
    /// with: a 32-sample sine written while a 512-sample square sounds
    /// plays as the sine's cycle and then the rest of the square (TEST40,
    /// TEST41).
    pub fn define(&mut self, n: usize, mut wave: Wave) {
        if let Some(old) = &self.user[n]
            && old.samples.len() > wave.samples.len()
        {
            wave.samples
                .extend_from_slice(&old.samples[wave.samples.len()..]);
        }
        self.user[n] = Some(Rc::new(wave));
    }

    pub fn get(&self, program: i32) -> Option<Rc<Wave>> {
        match program {
            216..=222 => Some(self.pulses[(program - 216) as usize].clone()),
            223 => Some(self.noise.clone()),
            224..=255 => match &self.user[(program - 224) as usize] {
                Some(w) => Some(w.clone()),
                None => self.unset().cloned(),
            },
            _ => None,
        }
    }

    /// Keeps a note of `program` reading what WAVSET last wrote there; see
    /// [`Waves::define`]. The sine an unset waveform plays is not that
    /// memory, so a note of it goes on with the sine.
    pub(crate) fn follow(&self, program: i32, voice: &mut Voice) {
        if self
            .unset
            .get()
            .and_then(Option::as_ref)
            .is_some_and(|u| voice.reads(u))
        {
            return;
        }
        if let Some(w) = self.get(program) {
            voice.follow(&w);
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
    /// The step at the base note, and that note, as the note started.
    rate: f64,
    base_note: i32,
    /// The loop, and whether it plays for ever, as the note started.
    looped: (usize, usize),
    loops: bool,
    /// The channel's @MP, @MA and @ML, and how far round they have gone.
    pub(crate) lfo: [sampler::Lfo; 3],
    lfos: sampler::Lfos,
    /// A portamento's glide, in cents.
    pub(crate) porta: f32,
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
        let rate = match wave.rate {
            Some(rate) => rate / f64::from(RATE),
            // A cycle at the base note is A=440 Hz: a waveform with base
            // note 57 sounds an octave higher.
            None => wave.cycle * 440.0 / f64::from(RATE),
        };
        let end = wave.looped.1.clamp(1, wave.samples.len().max(1));
        let looped = (wave.looped.0.min(end - 1), end);
        let mut v = Voice {
            rate,
            base_note: wave.base_note,
            looped,
            loops: wave.rate.is_none() || looping || looped.0 > 0,
            wave,
            envelope,
            key,
            cents,
            pos: 0.0,
            step: 0.0,
            base_step: 0.0,
            lfo: [sampler::Lfo::default(); 3],
            lfos: sampler::Lfos::default(),
            porta: 0.0,
            gain: WAVE_GAIN,
            pan: 0.0,
            levels: [0.0; 2],
            steps: [0.0; 2],
            targets: [0.0; 2],
            in_tick: 0,
            done: false,
        };
        v.set_pitch(cents);
        v
    }

    pub fn set_pitch(&mut self, cents: i32) {
        self.cents = cents;
        let ratio =
            2f64.powf((f64::from(self.key - self.base_note) * 100.0 + f64::from(cents)) / 1200.0);
        self.base_step = self.rate * ratio;
        self.step = self.base_step;
    }

    /// Goes on reading `wave`'s memory in place of the one the note
    /// started with, as when WAVSET sets the instrument again while the
    /// note sounds; see [`Waves::define`].
    pub fn follow(&mut self, wave: &Rc<Wave>) {
        if !Rc::ptr_eq(&self.wave, wave) {
            self.wave = wave.clone();
        }
    }

    fn reads(&self, wave: &Rc<Wave>) -> bool {
        Rc::ptr_eq(&self.wave, wave)
    }

    /// A portamento's end: the note goes on as `key`.
    pub fn retune(&mut self, key: i32) {
        self.key = key;
        self.porta = 0.0;
        self.set_pitch(self.cents);
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
        let (start, end) = self.looped;
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
                self.step = match cents + self.porta {
                    0.0 => self.base_step,
                    cents => self.base_step * f64::from(2f32.powf(cents / 1200.0)),
                };
            }
            self.in_tick = (self.in_tick + 1) % sampler::TICK;
            if self.pos >= end as f64 {
                if self.loops {
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
pub(crate) mod tests {
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
    fn notes_keep_their_length_when_a_shorter_waveform_is_written() {
        let mut waves = Waves::default();
        let square = |n: usize, level: f32| Wave {
            samples: (0..n)
                .map(|i| if i < n / 2 { level } else { -level })
                .collect(),
            rate: Some(EIGHT_BIT_RATE),
            looped: (0, n),
            ..pulse(219)
        };
        waves.define(0, square(512, 0.5));
        let mut v = Voice::new(waves.get(224).unwrap(), None, 69, 0, true);
        waves.define(0, square(32, 0.25));
        let w = waves.get(224).unwrap();
        // The memory keeps the old square past the new 32 samples, and
        // new notes play 32 of it.
        assert_eq!((w.samples.len(), w.looped), (512, (0, 32)));
        assert_eq!((w.samples[31], w.samples[300]), (-0.25, -0.5));
        // The note that sounded goes on over all 512 at its old rate.
        let steady = v.step;
        v.follow(&w);
        assert_eq!((v.looped, v.step), ((0, 512), steady));
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

    /// A bank with a four-sample cycle as its @224.
    pub(crate) fn sine_bank() -> Bank {
        let samples: [i16; 4] = [0, 16384, 0, -16384];
        let adshr = Adshr {
            attack: 127,
            decay: 127,
            sustain: 127,
            hold: 0,
            release: 127,
        };
        Bank {
            instruments: (0..225)
                .map(|p| if p == 224 { 0..1 } else { 0..0 })
                .collect(),
            regions: vec![crate::sound::bank::Region {
                keys: (0, 95),
                wave: 0,
                original_key: 57,
                volume: 127,
                pan: 64,
                envelope: adshr,
                ignore_note_off: false,
            }],
            waves: vec![crate::sound::bank::WaveInfo {
                rate: 28160,
                len: 4,
                loop_start: Some(0),
                encoding: crate::sound::bank::Encoding::Pcm16,
                data: 0..8,
            }],
            data: samples.iter().flat_map(|s| s.to_le_bytes()).collect(),
        }
    }

    #[test]
    fn what_an_unset_waveform_plays_comes_from_the_archives_224() {
        let bank = sine_bank();
        let w = unset(&bank).unwrap();
        assert_eq!(w.samples, [0.0, 0.5, 0.0, -0.5]);
        assert_eq!(w.base_note, 57);
        assert_eq!(w.rate, Some(28160.0));
        assert_eq!(w.looped, (0, 4));
        assert_eq!(w.beep_ticks, Some(BEEP_TICKS_A_SECOND));
        assert!(w.beep_loop && !w.beep_repeats);
        let none = Bank {
            instruments: Vec::new(),
            regions: Vec::new(),
            waves: Vec::new(),
            data: Vec::new(),
        };
        assert_eq!(unset(&none), None);

        // All 32 play it until set, each number on its own, and nothing
        // plays without the bank.
        let mut waves = Waves::default();
        assert_eq!(waves.get(224), None);
        waves.set_bank(Some(Arc::new(bank)));
        assert!(Rc::ptr_eq(
            &waves.get(224).unwrap(),
            &waves.get(255).unwrap()
        ));
        waves.define(16, pulse(219));
        assert_eq!(*waves.get(240).unwrap(), pulse(219));
        assert_eq!(waves.get(250).unwrap().samples, [0.0, 0.5, 0.0, -0.5]);
        // WAVSET of a number does not reach a note of the sine.
        let mut v = Voice::new(waves.get(250).unwrap(), None, 57, 0, true);
        waves.define(26, pulse(219));
        waves.follow(250, &mut v);
        assert_eq!(v.wave.samples, [0.0, 0.5, 0.0, -0.5]);
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
