//! A small sampler for SmileBASIC 4's own instruments. Each note decodes
//! its waveform as it plays, so nothing is decoded ahead and memory stays
//! at the compressed bank's size.

use std::sync::Arc;

use super::RATE;
use super::bank::{Adshr, Bank, Reader, Region};
use super::synth::{CHANNELS, Synth};

/// Samples between envelope steps: 5 ms.
// UNVERIFIED: how often SmileBASIC 4 steps its envelopes.
const TICK: usize = RATE as usize / 200;
/// Notes sounding at once; the oldest makes way for a new one.
const MAX_VOICES: usize = 48;
/// The level of silence, in 1/128ths of 0.1 dB.
const SILENCE: i32 = -723 * 128;
/// Scales the mix so that a few loud notes do not clip.
// UNVERIFIED: SmileBASIC 4's output level.
const OUTPUT_GAIN: f32 = 0.5;
/// The vibrato's speed, and its depth at full modulation, in cents.
// UNVERIFIED: how SmileBASIC 4's vibrato maps to these.
const VIBRATO_HZ: f32 = 6.0;
const VIBRATO_CENTS: f32 = 50.0;

/// The attack's step for attack values 109 to 127: how much of the level
/// is left after each step, in 256ths. Below 109 it is 255 - attack.
// UNVERIFIED: these are the Nintendo DS sound engine's values, assumed
// to carry over to SmileBASIC 4.
const FAST_ATTACK: [i32; 19] = [
    0, 1, 5, 14, 26, 38, 51, 63, 73, 84, 92, 100, 109, 116, 123, 127, 132, 137, 143,
];

fn attack_factor(attack: u8) -> i32 {
    match attack.min(127) {
        a @ 109.. => FAST_ATTACK[usize::from(127 - a)],
        a => 255 - i32::from(a),
    }
}

/// How far the level falls each step for a decay or release value.
// UNVERIFIED: as attack_factor.
fn fall_rate(value: u8) -> i32 {
    match i32::from(value.min(127)) {
        127 => 0xFFFF,
        126 => 0x3C00,
        v @ 0..50 => v * 2 + 1,
        v => 0x1E00 / (126 - v),
    }
}

/// A 0-127 level as 1/128ths of 0.1 dB, squared as Nintendo's volumes are.
fn level(value: u8) -> i32 {
    if value == 0 {
        return SILENCE;
    }
    let db10 = 400.0 * (f32::from(value.min(127)) / 127.0).log10();
    (db10.round() as i32 * 128).max(SILENCE)
}

/// A 0-127 value as a gain, squared.
fn square(value: i32) -> f32 {
    let v = value.clamp(0, 127) as f32 / 127.0;
    v * v
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Stage {
    Attack,
    Hold(u8),
    Decay,
    Sustain,
    Release,
    Done,
}

#[derive(Clone, Debug)]
struct Envelope {
    adshr: Adshr,
    stage: Stage,
    level: i32,
}

impl Envelope {
    fn new(adshr: Adshr) -> Envelope {
        Envelope {
            adshr,
            stage: Stage::Attack,
            level: SILENCE,
        }
    }

    fn step(&mut self) {
        let e = self.adshr;
        match self.stage {
            Stage::Attack => {
                self.level = -((-self.level * attack_factor(e.attack)) >> 8);
                if self.level == 0 {
                    // UNVERIFIED: hold lasts a step for each unit.
                    self.stage = Stage::Hold(e.hold);
                }
            }
            Stage::Hold(0) => self.stage = Stage::Decay,
            Stage::Hold(n) => self.stage = Stage::Hold(n - 1),
            Stage::Decay => {
                let sustain = level(e.sustain);
                self.level -= fall_rate(e.decay);
                if self.level <= sustain {
                    self.level = sustain;
                    self.stage = if sustain <= SILENCE {
                        Stage::Done
                    } else {
                        Stage::Sustain
                    };
                }
            }
            Stage::Sustain | Stage::Done => {}
            Stage::Release => {
                self.level -= fall_rate(e.release);
                if self.level <= SILENCE {
                    self.level = SILENCE;
                    self.stage = Stage::Done;
                }
            }
        }
    }

    fn gain(&self) -> f32 {
        if self.level <= SILENCE {
            return 0.0;
        }
        10f32.powf(self.level as f32 / (128.0 * 200.0))
    }
}

#[derive(Clone, Copy, Debug)]
struct Channel {
    program: i32,
    volume: i32,
    expression: i32,
    pan: i32,
    modulation: i32,
    bend: i32,
}

impl Default for Channel {
    fn default() -> Self {
        Channel {
            program: 0,
            volume: 100,
            expression: 127,
            pan: 64,
            modulation: 0,
            bend: 0,
        }
    }
}

#[derive(Clone, Debug)]
struct Voice {
    channel: usize,
    key: i32,
    region: Region,
    reader: Reader,
    /// The samples either side of the position, and how far between.
    s0: f32,
    s1: f32,
    frac: f64,
    /// The step through the waveform before bend and vibrato.
    base_step: f64,
    step: f64,
    velocity: f32,
    envelope: Envelope,
    /// Samples into the current envelope step.
    in_tick: usize,
    gains: [f32; 2],
    deltas: [f32; 2],
    vibrato_phase: f32,
    /// The waveform ran out.
    wave_ended: bool,
    done: bool,
}

impl Voice {
    fn next_sample(&mut self, bank: &Bank) -> f32 {
        let out = self.s0 + (self.s1 - self.s0) * self.frac as f32;
        self.frac += self.step;
        while self.frac >= 1.0 {
            self.frac -= 1.0;
            if self.wave_ended {
                self.done = true;
            }
            self.s0 = self.s1;
            let wave = &bank.waves[usize::from(self.region.wave)];
            self.s1 = match self.reader.next(wave, &bank.data) {
                Some(s) => f32::from(s) / 32768.0,
                None => {
                    self.wave_ended = true;
                    0.0
                }
            };
        }
        out
    }

    /// Steps the envelope and aims the gains and pitch at where they
    /// should be by the end of the next step.
    fn tick(&mut self, ch: &Channel) {
        self.envelope.step();
        let gain = if self.envelope.stage == Stage::Done {
            0.0
        } else {
            OUTPUT_GAIN
                * self.envelope.gain()
                * square(ch.volume)
                * square(ch.expression)
                * square(i32::from(self.region.volume))
                * self.velocity
        };
        let pan = (ch.pan + i32::from(self.region.pan) - 64).clamp(0, 127) as f32 / 127.0;
        let angle = pan * std::f32::consts::FRAC_PI_2;
        let target = [gain * angle.cos(), gain * angle.sin()];
        self.deltas = [0, 1].map(|i| (target[i] - self.gains[i]) / TICK as f32);
        let mut cents = ch.bend as f32;
        if ch.modulation > 0 {
            self.vibrato_phase =
                (self.vibrato_phase + VIBRATO_HZ * TICK as f32 / RATE as f32).fract();
            let depth = VIBRATO_CENTS * ch.modulation as f32 / 127.0;
            cents += depth * (self.vibrato_phase * std::f32::consts::TAU).sin();
        }
        self.step = self.base_step * f64::from(2f32.powf(cents / 1200.0));
    }

    fn render(&mut self, bank: &Bank, ch: &Channel, left: &mut [f32], right: &mut [f32]) {
        for (l, r) in left.iter_mut().zip(right.iter_mut()) {
            if self.in_tick == 0 {
                if self.envelope.stage == Stage::Done && self.gains == [0.0; 2] {
                    self.done = true;
                }
                self.tick(ch);
            }
            if self.done {
                return;
            }
            self.in_tick = (self.in_tick + 1) % TICK;
            let s = self.next_sample(bank);
            self.gains = [0, 1].map(|i| self.gains[i] + self.deltas[i]);
            if self.in_tick == 0 {
                // Land exactly on the target, which may be silence.
                self.gains = self.gains.map(|g| if g.abs() < 1e-6 { 0.0 } else { g });
            }
            *l += s * self.gains[0];
            *r += s * self.gains[1];
        }
    }
}

pub(crate) struct Sampler {
    bank: Arc<Bank>,
    channels: [Channel; CHANNELS],
    voices: Vec<Voice>,
}

impl Sampler {
    pub(crate) fn new(bank: Arc<Bank>) -> Sampler {
        Sampler {
            bank,
            channels: [Channel::default(); CHANNELS],
            voices: Vec::new(),
        }
    }
}

impl Synth for Sampler {
    fn plays(&self, program: i32) -> bool {
        self.bank.has_instrument(program)
    }

    fn program(&mut self, channel: usize, program: i32) {
        self.channels[channel].program = program;
    }

    fn volume(&mut self, channel: usize, value: i32) {
        self.channels[channel].volume = value;
    }

    fn expression(&mut self, channel: usize, value: i32) {
        self.channels[channel].expression = value;
    }

    fn pan(&mut self, channel: usize, value: i32) {
        self.channels[channel].pan = value;
    }

    fn modulation(&mut self, channel: usize, value: i32) {
        self.channels[channel].modulation = value;
    }

    fn bend(&mut self, channel: usize, cents: i32) {
        self.channels[channel].bend = cents;
    }

    fn note_on(&mut self, channel: usize, key: i32, velocity: i32) {
        let Some(&region) = self.bank.region(self.channels[channel].program, key) else {
            return;
        };
        let wave = &self.bank.waves[usize::from(region.wave)];
        let mut reader = Reader::default();
        let mut sample = || {
            reader
                .next(wave, &self.bank.data)
                .map_or(0.0, |s| f32::from(s) / 32768.0)
        };
        let (s0, s1) = (sample(), sample());
        let semitones = (key - i32::from(region.original_key)) as f64;
        if self.voices.len() >= MAX_VOICES {
            let oldest = self
                .voices
                .iter()
                .position(|v| v.envelope.stage == Stage::Release)
                .unwrap_or(0);
            self.voices.remove(oldest);
        }
        self.voices.push(Voice {
            channel,
            key,
            region,
            reader,
            s0,
            s1,
            frac: 0.0,
            base_step: f64::from(wave.rate) / f64::from(RATE) * 2f64.powf(semitones / 12.0),
            step: 0.0,
            velocity: square(velocity),
            envelope: Envelope::new(region.envelope),
            in_tick: 0,
            gains: [0.0; 2],
            deltas: [0.0; 2],
            vibrato_phase: 0.0,
            wave_ended: false,
            done: false,
        });
    }

    fn note_off(&mut self, channel: usize, key: i32) {
        for v in &mut self.voices {
            if v.channel == channel
                && v.key == key
                && !v.region.ignore_note_off
                && v.envelope.stage != Stage::Done
            {
                v.envelope.stage = Stage::Release;
            }
        }
    }

    fn channel_off(&mut self, channel: usize) {
        self.voices.retain(|v| v.channel != channel);
    }

    fn reset(&mut self) {
        self.voices.clear();
        self.channels = [Channel::default(); CHANNELS];
    }

    fn render(&mut self, left: &mut [f32], right: &mut [f32]) {
        left.fill(0.0);
        right.fill(0.0);
        let bank = &*self.bank;
        for v in &mut self.voices {
            v.render(bank, &self.channels[v.channel], left, right);
        }
        self.voices.retain(|v| !v.done);
    }
}

/// An envelope's stages in seconds, as a SoundFont gives them: the decay
/// and release are how long a fall of 100 dB takes at their rate.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Times {
    pub attack: f64,
    pub hold: f64,
    pub decay: f64,
    /// The sustain level below full, in dB.
    pub sustain_db: f64,
    pub release: f64,
}

pub fn times(adshr: Adshr) -> Times {
    let step = TICK as f64 / f64::from(RATE);
    let mut e = Envelope::new(adshr);
    let mut steps = 0;
    while e.stage == Stage::Attack && steps < 100_000 {
        e.step();
        steps += 1;
    }
    let fall = |value: u8| f64::from(-SILENCE) / f64::from(fall_rate(value)) * step;
    Times {
        attack: f64::from(steps) * step,
        hold: f64::from(adshr.hold) * step,
        decay: fall(adshr.decay),
        sustain_db: f64::from(-level(adshr.sustain)) / 1280.0,
        release: fall(adshr.release),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sound::bank::{Encoding, WaveInfo};

    /// One instrument: a looping square wave at 48 kHz, eight samples a
    /// cycle, sounding at key 60.
    fn sampler(adshr: Adshr, ignore_note_off: bool) -> Sampler {
        let samples: Vec<i16> = (0..8).map(|i| if i < 4 { 16384 } else { -16384 }).collect();
        let bank = Bank {
            instruments: std::iter::once(0..1).collect(),
            regions: vec![Region {
                keys: (0, 127),
                wave: 0,
                original_key: 60,
                volume: 127,
                pan: 64,
                envelope: adshr,
                ignore_note_off,
            }],
            waves: vec![WaveInfo {
                rate: RATE,
                len: 8,
                loop_start: Some(0),
                encoding: Encoding::Pcm16,
                data: 0..16,
            }],
            data: samples.iter().flat_map(|s| s.to_le_bytes()).collect(),
        };
        Sampler::new(Arc::new(bank))
    }

    const ORGAN: Adshr = Adshr {
        attack: 127,
        decay: 127,
        sustain: 127,
        hold: 0,
        release: 120,
    };

    fn peak(s: &mut Sampler, samples: usize) -> f32 {
        let (mut l, mut r) = (vec![0.0; samples], vec![0.0; samples]);
        s.render(&mut l, &mut r);
        l.iter().chain(&r).fold(0f32, |m, x| m.max(x.abs()))
    }

    #[test]
    fn a_note_sounds_and_releases() {
        let mut s = sampler(ORGAN, false);
        s.note_on(0, 60, 127);
        assert!(peak(&mut s, 960) > 0.1);
        s.note_off(0, 60);
        peak(&mut s, 48_000);
        assert!(s.voices.is_empty());
        assert_eq!(peak(&mut s, 960), 0.0);
    }

    #[test]
    fn drums_ignore_note_off() {
        let mut s = sampler(ORGAN, true);
        s.note_on(0, 60, 127);
        s.note_off(0, 60);
        peak(&mut s, 4800);
        assert_eq!(s.voices.len(), 1);
    }

    #[test]
    fn an_octave_up_steps_twice_as_fast() {
        let mut s = sampler(ORGAN, false);
        s.note_on(0, 72, 127);
        peak(&mut s, 1);
        assert!((s.voices[0].step - 2.0).abs() < 1e-6);
        s.bend(0, -1200);
        peak(&mut s, TICK);
        assert!((s.voices[0].step - 1.0).abs() < 1e-6);
    }

    #[test]
    fn decay_to_silence_ends_the_note() {
        let mut s = sampler(
            Adshr {
                decay: 120,
                sustain: 0,
                ..ORGAN
            },
            false,
        );
        s.note_on(0, 60, 127);
        peak(&mut s, 48_000);
        assert!(s.voices.is_empty());
    }

    #[test]
    fn levels_follow_the_curves() {
        assert_eq!(level(127), 0);
        assert_eq!(level(0), SILENCE);
        // Half the value is about -12 dB, squared.
        assert_eq!(level(64) / 128, -119);
        assert_eq!(attack_factor(127), 0);
        assert_eq!(attack_factor(0), 255);
        assert_eq!(fall_rate(127), 0xFFFF);
        assert_eq!(fall_rate(10), 21);
    }

    #[test]
    fn times_follow_the_steps() {
        let t = times(Adshr {
            attack: 127,
            decay: 126,
            sustain: 0,
            hold: 2,
            release: 127,
        });
        assert_eq!(t.attack, 0.005);
        assert_eq!(t.hold, 0.01);
        assert!((t.decay - 92544.0 / 15360.0 * 0.005).abs() < 1e-9);
        assert_eq!(t.sustain_db, 72.3);
        assert!(times(Adshr::default()).attack > 1.0);
    }

    #[test]
    fn voices_are_capped() {
        let mut s = sampler(ORGAN, false);
        for k in 0..MAX_VOICES as i32 + 5 {
            s.note_on(0, k, 127);
        }
        assert_eq!(s.voices.len(), MAX_VOICES);
    }
}
