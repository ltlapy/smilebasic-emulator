//! A small sampler for SmileBASIC 4's own instruments. Each note decodes
//! its waveform as it plays, so nothing is decoded ahead and memory stays
//! at the compressed bank's size.

use std::sync::Arc;

use super::RATE;
use super::bank::{Adshr, Bank, Reader, Region};
use super::synth::{CHANNELS, Synth};
use super::wave::Envelope as MmlEnvelope;

/// Samples between envelope steps: 5 ms.
// UNVERIFIED: how often SmileBASIC 4 steps its envelopes.
pub(crate) const TICK: usize = RATE as usize / 200;
/// Notes sounding at once; the oldest makes way for a new one.
pub(crate) const MAX_VOICES: usize = 48;
/// The level of silence, in 1/128ths of 0.1 dB.
const SILENCE: i32 = -723 * 128;
/// The instruments' level: @0's A at V127 in the middle records at an
/// RMS of 850 (TEST40), its regions' volume being 88. A full-scale sample
/// at region volume 127 then plays about as loud as a full-scale WAVSET
/// waveform, as the PSG's do (TEST41).
pub(crate) const OUTPUT_GAIN: f32 = 0.1774;

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

/// A pan of 0 to 127 as -1 (left) to 1 (right), 64 in the middle:
/// (P - 64) / 63, so that 0 and 1 are both hard left.
pub(crate) fn pan_position(pan: i32) -> f32 {
    ((pan.clamp(0, 127) - 64) as f32 / 63.0).max(-1.0)
}

/// Left and right gains for a pan position of -1 to 1, at equal power on
/// a square-root curve: P64 is even on both sides, and @ML's swing fits
/// this curve rather than a sine and cosine (TEST40).
pub(crate) fn pan_gains(position: f32) -> [f32; 2] {
    let p = position.clamp(-1.0, 1.0);
    [((1.0 - p) * 0.5).sqrt(), ((1.0 + p) * 0.5).sqrt()]
}

/// What an LFO moves: MML's @MP, @MA and @ML.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LfoTarget {
    Pitch = 0,
    Volume = 1,
    Pan = 2,
}

/// An LFO's settings, as MML's @MP, @MA and @ML give them: depth,
/// range, speed and delay. They act as the Nintendo sound engine's:
/// the wave is a sine of depth / 128 times range units at speed / 2.56
/// Hz, a unit being a semitone of pitch, 6 dB of volume or the whole
/// way from the middle to one side of the pan. TEST40 measured @MP7,1,7
/// at about ±6 cents every 360 ms, @MP32,1,20 at ±25 cents every 130
/// ms, @MA24,2,7 at ±2.3 dB and @ML24,2,7 swinging each side between
/// 0.77 and 1.2 of the middle's level, both every 360 ms.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Lfo {
    pub(crate) depth: i32,
    pub(crate) range: i32,
    pub(crate) speed: i32,
    /// Samples from the note's start until it moves.
    pub(crate) delay: u32,
}

impl Lfo {
    /// The modulation wheel's vibrato, as the sound engine's sequences
    /// set it, with their default range and speed.
    pub(crate) fn wheel(depth: i32) -> Lfo {
        Lfo {
            depth,
            range: 1,
            speed: 16,
            delay: 0,
        }
    }

    pub(crate) fn off(&self) -> bool {
        self.depth == 0 || self.range == 0 || self.speed == 0
    }
}

/// How far round a note's LFO has gone.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct LfoState {
    phase: f32,
    waited: u32,
}

impl LfoState {
    /// The LFO's value at the next envelope step, in its units.
    pub(crate) fn step(&mut self, lfo: &Lfo) -> f32 {
        if lfo.off() {
            return 0.0;
        }
        if self.waited < lfo.delay {
            self.waited += TICK as u32;
            return 0.0;
        }
        let hz = lfo.speed as f32 * 0.390_625;
        self.phase = (self.phase + hz * TICK as f32 / RATE as f32).fract();
        (self.phase * std::f32::consts::TAU).sin() * lfo.depth as f32 / 128.0 * lfo.range as f32
    }
}

/// The three LFOs of a note.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Lfos {
    states: [LfoState; 3],
}

impl Lfos {
    /// Steps them: the pitch in cents, the gain as a factor and the pan
    /// as a move of its position.
    pub(crate) fn step(&mut self, lfos: &[Lfo; 3]) -> (f32, f32, f32) {
        let [p, v, l] = [0, 1, 2].map(|i| self.states[i].step(&lfos[i]));
        let gain = if v == 0.0 {
            1.0
        } else {
            10f32.powf(v * 6.0 / 20.0)
        };
        (p * 100.0, gain, l)
    }
}

/// A 0-127 value as a gain, squared.
pub(crate) fn square(value: i32) -> f32 {
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

/// A note's level as SmileBASIC's instruments move it, stepped every
/// [`TICK`] samples. The PSG and user waveforms move the same way.
#[derive(Clone, Debug)]
pub(crate) struct Envelope {
    adshr: Adshr,
    stage: Stage,
    level: i32,
}

impl Envelope {
    pub(crate) fn new(adshr: Adshr) -> Envelope {
        Envelope {
            adshr,
            stage: Stage::Attack,
            level: SILENCE,
        }
    }

    pub(crate) fn release(&mut self) {
        if self.stage != Stage::Done {
            self.stage = Stage::Release;
        }
    }

    /// Whether the note has been let go.
    pub(crate) fn released(&self) -> bool {
        matches!(self.stage, Stage::Release | Stage::Done)
    }

    /// Whether the note has fallen silent for good.
    pub(crate) fn done(&self) -> bool {
        self.stage == Stage::Done
    }

    pub(crate) fn step(&mut self) {
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

    pub(crate) fn gain(&self) -> f32 {
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
    lfos: [Lfo; 3],
    bend: i32,
    /// MML's @E, over the instruments' own.
    envelope: Option<MmlEnvelope>,
}

impl Default for Channel {
    fn default() -> Self {
        Channel {
            program: 0,
            volume: 100,
            expression: 127,
            pan: 64,
            lfos: [Lfo::default(); 3],
            bend: 0,
            envelope: None,
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
    lfos: Lfos,
    /// A portamento's glide, in cents.
    porta: f32,
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
        let (cents, tremolo, pan) = self.lfos.step(&ch.lfos);
        let gain = if self.envelope.stage == Stage::Done {
            0.0
        } else {
            OUTPUT_GAIN
                * tremolo
                * self.envelope.gain()
                * square(ch.volume)
                * square(ch.expression)
                // Unlike V and @V, the region's volume is linear: with it
                // squared the PSG played 45% too loud next to @0.
                * f32::from(self.region.volume)
                / 127.0
                * self.velocity
        };
        let [l, r] = pan_gains(pan_position(ch.pan + i32::from(self.region.pan) - 64) + pan);
        let target = [gain * l, gain * r];
        self.deltas = [0, 1].map(|i| (target[i] - self.gains[i]) / TICK as f32);
        let cents = ch.bend as f32 + cents + self.porta;
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
        self.channels[channel].lfos[LfoTarget::Pitch as usize] = Lfo::wheel(value);
    }

    fn lfo(&mut self, channel: usize, target: LfoTarget, lfo: Lfo) {
        self.channels[channel].lfos[target as usize] = lfo;
    }

    fn bend(&mut self, channel: usize, cents: i32) {
        self.channels[channel].bend = cents;
    }

    fn envelope(&mut self, channel: usize, envelope: Option<MmlEnvelope>) {
        self.channels[channel].envelope = envelope;
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
            envelope: Envelope::new(match self.channels[channel].envelope {
                // UNVERIFIED: @E keeps the instrument's hold.
                Some(e) => Adshr {
                    hold: region.envelope.hold,
                    ..e.into()
                },
                None => region.envelope,
            }),
            in_tick: 0,
            gains: [0.0; 2],
            deltas: [0.0; 2],
            lfos: Lfos::default(),
            porta: 0.0,
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

    fn porta(&mut self, channel: usize, cents: f32) {
        let held = |v: &&mut Voice| v.channel == channel && !v.envelope.released();
        self.voices
            .iter_mut()
            .filter(held)
            .for_each(|v| v.porta = cents);
    }

    fn retune(&mut self, channel: usize, from: i32, to: i32) {
        for v in &mut self.voices {
            if v.channel == channel && v.key == from && !v.envelope.released() {
                v.base_step *= 2f64.powf(f64::from(to - from) / 12.0);
                v.key = to;
                v.porta = 0.0;
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
        assert!(peak(&mut s, 960) > 0.02);
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
    fn mml_envelopes_take_over_from_the_instruments() {
        let quick = Adshr {
            release: 127,
            ..ORGAN
        };
        let mut s = sampler(quick, false);
        // A slow release, as songs give with @E127,116,111,32.
        let slow = MmlEnvelope {
            attack: 127,
            decay: 116,
            sustain: 111,
            release: 32,
        };
        s.envelope(0, Some(slow));
        s.note_on(0, 60, 127);
        peak(&mut s, 960);
        s.note_off(0, 60);
        peak(&mut s, 4800);
        assert!(peak(&mut s, 960) > 0.01);
        // @ER: the instrument's quick release again.
        s.envelope(0, None);
        s.note_on(0, 62, 127);
        peak(&mut s, 960);
        s.note_off(0, 62);
        peak(&mut s, 4800);
        assert_eq!(s.voices.iter().filter(|v| v.key == 62).count(), 0);
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
