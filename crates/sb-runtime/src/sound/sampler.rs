//! A small sampler for SmileBASIC 4's own instruments. Each note decodes
//! its waveform as it plays, so nothing is decoded ahead. The chunks a note
//! decodes are kept for later notes, up to a budget (see [`Decoded`]).

use std::sync::Arc;

use super::RATE;
use super::bank::{Adshr, Bank, Reader, Region, WaveInfo};
use super::decoded::{Decoded, Shared};
use super::synth::{CHANNELS, Synth};
use super::wave::Envelope as MmlEnvelope;

/// Samples between envelope steps: 5 ms.
pub(crate) const TICK: usize = RATE as usize / 200;
/// Milliseconds in a step: the envelope advances one millisecond at a time.
const TICK_MS: u32 = 5;
/// Notes sounding at the same time. A new one stops the oldest released
/// note, or the oldest note when none is released.
pub(crate) const MAX_VOICES: usize = 48;
/// The level the attack starts at, in 0.1 dB: the note is silent below it.
const FLOOR: f32 = -904.0;
/// The lowest sustain level, in 0.1 dB.
const SUSTAIN_FLOOR: f32 = -723.0;
/// The instruments' level: @0's A at V127 in the middle records at an
/// RMS of 850 (TEST40), with a region volume of 88. A full-scale sample
/// at region volume 127 then plays about as loud as a full-scale WAVSET
/// waveform, the same as the PSG's waveforms do (TEST41).
pub(crate) const OUTPUT_GAIN: f32 = 0.1774;

/// Steps of [`TAPS`] between two samples of a waveform. Fewer steps add
/// their own noise: a 1 kHz sine at 8000 Hz gets a component at 15 kHz
/// 64 dB down with 256, 84 dB down with 1024.
const PHASES: usize = 1024;

/// The curve that notes of instruments and waveforms are read through, one
/// side of it: its value and slope at 0, 0.5, 1, 1.5 and 2 samples from
/// the position, a cubic between each two. It keeps what is well below
/// half the waveform's rate and removes most of the images above it, from
/// four samples. PCMSTREAM is not filtered.
// UNVERIFIED: a curve fitted to recordings of SB4's images and harmonics,
// not SB4's own; SB4 may use another for notes played well above their
// rate.
const KNOTS: [(f64, f64); 5] = [
    (1.0, 0.0),
    (0.6412, -1.1478),
    (0.029, -0.6635),
    (-0.0914, 0.1061),
    (0.0, 0.1756),
];

const fn kernel(d: f64) -> f64 {
    let a = if d < 0.0 { -d } else { d };
    if a >= 2.0 {
        return 0.0;
    }
    let x = a * 2.0;
    let i = x as usize;
    let t = x - i as f64;
    let ((v0, m0), (v1, m1)) = (KNOTS[i], KNOTS[i + 1]);
    let (t2, t3) = (t * t, t * t * t);
    (2.0 * t3 - 3.0 * t2 + 1.0) * v0
        + (t3 - 2.0 * t2 + t) * 0.5 * m0
        + (3.0 * t2 - 2.0 * t3) * v1
        + (t3 - t2) * 0.5 * m1
}

/// The weights of the samples before, at and the two after the position,
/// for each step of [`PHASES`] and the next sample: in 32768ths, so that
/// they are the same on every machine, and adding up to 1 so that no step
/// is louder than another.
const fn taps() -> [[f32; 4]; PHASES + 1] {
    const ONE: f64 = 32768.0;
    let mut out = [[0.0; 4]; PHASES + 1];
    let mut p = 0;
    while p <= PHASES {
        let frac = p as f64 / PHASES as f64;
        let w = [
            kernel(frac + 1.0),
            kernel(frac),
            kernel(frac - 1.0),
            kernel(frac - 2.0),
        ];
        let sum = w[0] + w[1] + w[2] + w[3];
        let mut q = [0i32; 4];
        let mut total = 0;
        let mut k = 0;
        while k < 4 {
            let x = w[k] / sum * ONE;
            q[k] = if x < 0.0 {
                (x - 0.5) as i32
            } else {
                (x + 0.5) as i32
            };
            total += q[k];
            k += 1;
        }
        // The remainder of the rounding goes to the nearer sample.
        q[if 2 * p < PHASES { 1 } else { 2 }] += ONE as i32 - total;
        let mut k = 0;
        while k < 4 {
            out[p][k] = q[k] as f32 / ONE as f32;
            k += 1;
        }
        p += 1;
    }
    out
}

static TAPS: [[f32; 4]; PHASES + 1] = taps();

/// The waveform at `frac` of the way from `h[1]` to `h[2]`, `h` being four
/// samples in a row.
#[inline(always)]
pub(crate) fn interpolate(h: &[f32; 4], frac: f64) -> f32 {
    // The nearest step; past the last, the next sample's weights.
    let w = &TAPS[((frac * PHASES as f64 + 0.5) as usize).min(PHASES)];
    h[0] * w[0] + h[1] * w[1] + h[2] * w[2] + h[3] * w[3]
}

/// The attack for attack values 109 to 127: the share of the level left
/// after 5 ms, in 256ths. Below 109 it is 255 - attack.
// These are the Nintendo DS sound engine's values; SmileBASIC 4's own
// steps of a millisecond are their fifth roots.
const FAST_ATTACK: [i32; 19] = [
    0, 1, 5, 14, 26, 38, 51, 63, 73, 84, 92, 100, 109, 116, 123, 127, 132, 137, 143,
];

/// The share of the level left after each millisecond of the attack.
fn attack_factor(attack: u8) -> f32 {
    let per_5ms = match attack.min(127) {
        a @ 109.. => FAST_ATTACK[usize::from(127 - a)],
        a => 255 - i32::from(a),
    };
    (per_5ms as f32 / 256.0).powf(0.2)
}

/// The fall of the level per millisecond for a decay or release value,
/// in 0.1 dB.
fn fall_rate(value: u8) -> f32 {
    match value.min(127) {
        127 => 65535.0,
        126 => 24.0,
        v @ 0..50 => f32::from(v * 2 + 1) / 640.0,
        v => 12.0 / f32::from(126 - v),
    }
}

/// A 0-127 sustain level in 0.1 dB, squared like Nintendo's volumes,
/// and no lower than -72.3 dB (-72.2 for 1).
fn level(value: u8) -> f32 {
    match value.min(127) {
        0 => SUSTAIN_FLOOR,
        1 => SUSTAIN_FLOOR + 1.0,
        v => (400.0 * (f32::from(v) / 127.0).log10())
            .round()
            .max(SUSTAIN_FLOOR),
    }
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

/// The target of an LFO: MML's @MP, @MA and @ML.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LfoTarget {
    Pitch = 0,
    Volume = 1,
    Pan = 2,
}

/// An LFO's settings, as MML's @MP, @MA and @ML give them: depth,
/// range, speed and delay. They act the same as the Nintendo sound
/// engine's LFOs: the wave is a sine of depth / 128 times range units at
/// speed / 2.56 Hz, a unit being a semitone of pitch, 6 dB of volume or
/// the whole way from the middle to one side of the pan. TEST40 measured
/// @MP7,1,7 at about ±6 cents every 360 ms, @MP32,1,20 at ±25 cents every
/// 130 ms, @MA24,2,7 at ±2.3 dB and @ML24,2,7 swinging each side between
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

/// The state of a note's LFO: its phase and the delay waited so far.
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
        // The wave starts from 0 at the step after the delay, and moves
        // from the next.
        let value = (self.phase * std::f32::consts::TAU).sin();
        let hz = lfo.speed as f32 * 0.390_625;
        self.phase = (self.phase + hz * TICK as f32 / RATE as f32).fract();
        value * lfo.depth as f32 / 128.0 * lfo.range as f32
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
        let gain = if v == 0.0 { 1.0 } else { volume_ratio(v * 6.0) };
        (p * 100.0, gain, l)
    }
}

/// Decibels as a gain, as the sound engine reads them from its table of
/// tenths of a dB: truncated, no louder than +6 dB, and silent from
/// -90.4 dB down. Without these limits a tremolo of a wide range reaches
/// beyond the range of a float.
#[inline(never)]
fn volume_ratio(db: f32) -> f32 {
    match (db * 10.0).clamp(-904.0, 60.0) as i32 {
        -904 => 0.0,
        tenths => 10f32.powf(tenths as f32 / 200.0),
    }
}

/// A 0-127 value as a gain, squared.
pub(crate) fn square(value: i32) -> f32 {
    let v = value.clamp(0, 127) as f32 / 127.0;
    v * v
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Stage {
    Attack,
    /// Milliseconds of the hold left.
    Hold(u32),
    Decay,
    Sustain,
    Release,
    Done,
}

/// A note's level as SmileBASIC's instruments move it, in 0.1 dB, one
/// millisecond at a time, stepped every [`TICK`] samples. The PSG and
/// user waveforms move the same way. The attack multiplies the level,
/// which starts at -90.4 dB; the hold lasts (hold + 1)^2 / 4 ms; decay and
/// release take the level down in a straight line, and the note ends
/// below -90.4 dB.
#[derive(Clone, Debug)]
pub(crate) struct Envelope {
    stage: Stage,
    level: f32,
    attack: f32,
    hold: u32,
    decay: f32,
    sustain: f32,
    release: f32,
}

impl Envelope {
    pub(crate) fn new(adshr: Adshr) -> Envelope {
        let hold = (u32::from(adshr.hold.min(127)) + 1).pow(2) / 4;
        Envelope {
            stage: Stage::Attack,
            level: FLOOR,
            attack: attack_factor(adshr.attack),
            hold,
            decay: fall_rate(adshr.decay),
            sustain: level(adshr.sustain),
            release: fall_rate(adshr.release),
        }
    }

    pub(crate) fn release(&mut self) {
        if self.stage != Stage::Done {
            self.stage = Stage::Release;
        }
    }

    /// Whether the note has been released.
    pub(crate) fn released(&self) -> bool {
        matches!(self.stage, Stage::Release | Stage::Done)
    }

    /// Whether the note has fallen silent for good.
    pub(crate) fn done(&self) -> bool {
        self.stage == Stage::Done
    }

    /// Advances by one step's milliseconds.
    pub(crate) fn step(&mut self) {
        for _ in 0..TICK_MS {
            self.step_ms();
        }
    }

    fn step_ms(&mut self) {
        match self.stage {
            Stage::Attack => {
                self.level *= self.attack;
                if self.level > -0.03125 {
                    self.level = 0.0;
                    self.stage = Stage::Hold(self.hold);
                }
            }
            Stage::Hold(0) => {
                self.stage = Stage::Decay;
                self.step_ms();
            }
            Stage::Hold(n) => self.stage = Stage::Hold(n - 1),
            Stage::Decay => {
                self.level -= self.decay;
                if self.level <= self.sustain {
                    self.level = self.sustain;
                    // SmileBASIC 4 holds a sustain of 0 at -72.3 dB until
                    // the note is released; here the note ends there, so
                    // that drums that ignore the release cannot pile up
                    // notes nobody hears.
                    // self.stage = Stage::Sustain;
                    self.stage = if self.sustain <= SUSTAIN_FLOOR {
                        Stage::Done
                    } else {
                        Stage::Sustain
                    };
                }
            }
            Stage::Sustain | Stage::Done => {}
            Stage::Release => {
                self.level -= self.release;
                if self.level < FLOOR {
                    self.level = FLOOR;
                    self.stage = Stage::Done;
                }
            }
        }
    }

    pub(crate) fn gain(&self) -> f32 {
        if self.stage == Stage::Done {
            return 0.0;
        }
        10f32.powf(self.level / 200.0)
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
    /// MML's @E, in place of the instruments' own envelopes.
    envelope: Option<MmlEnvelope>,
    /// BEEP's pan as a position, added to every voice's position: see
    /// [`Synth::pan_offset`].
    pan_offset: f32,
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
            pan_offset: 0.0,
        }
    }
}

#[derive(Clone, Debug)]
struct Voice {
    channel: usize,
    key: i32,
    region: Region,
    /// The position of a channel of a stereo waveform before the pan: -1
    /// for the left, 1 for the right; 0 for a mono waveform.
    side: f32,
    reader: Reader,
    /// The samples on either side of the position, and the fraction
    /// between them.
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
    /// Whether the first step has been taken.
    begun: bool,
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
    #[cfg(test)]
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

    /// Steps the envelope and aims the gains and pitch at their values for
    /// the end of the next step.
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
        let at = pan_position(ch.pan + i32::from(self.region.pan) - 64)
            + pan
            + ch.pan_offset
            + self.side;
        let [l, r] = pan_gains(at);
        let target = [gain * l, gain * r];
        if !self.begun {
            // The note starts at its first step's level, the same as waveforms
            // do: @0's C4 sounded from its 11th sample (TEST40).
            self.begun = true;
            self.gains = target;
        }
        self.deltas = [0, 1].map(|i| (target[i] - self.gains[i]) / TICK as f32);
        let cents = ch.bend as f32 + cents + self.porta;
        self.step = self.base_step * f64::from(2f32.powf(cents / 1200.0));
    }

    /// A step of the envelope lasts [`TICK`] samples, which are rendered as
    /// one run. This is the heaviest part of a voice's work, and in a run
    /// nothing but the waveform can change.
    fn render(&mut self, bank: &Bank, ch: &Channel, left: &mut [f32], right: &mut [f32]) {
        let wave = &bank.waves[usize::from(self.region.wave)];
        let data = &bank.data[wave.data.clone()];
        let mut at = 0;
        while at < left.len() {
            if self.in_tick == 0 {
                if self.envelope.stage == Stage::Done && self.gains == [0.0; 2] {
                    self.done = true;
                }
                self.tick(ch);
            }
            if self.done {
                return;
            }
            let run = (TICK - self.in_tick).min(left.len() - at);
            let ends_step = self.in_tick + run == TICK;
            self.in_tick = (self.in_tick + run) % TICK;
            let (ls, rs) = (&mut left[at..at + run], &mut right[at..at + run]);
            at += run;
            // Land exactly on the target, which may be silence: for the last
            // sample of a step, which is rendered separately.
            let (plain, last) = if ends_step { (run - 1, 1) } else { (run, 0) };
            let (mut g0, mut g1) = (self.gains[0], self.gains[1]);
            let [d0, d1] = self.deltas;
            let (mut s0, mut s1, mut frac) = (self.s0, self.s1, self.frac);
            let step = self.step;
            let (lp, ll) = ls.split_at_mut(plain);
            let (rp, rl) = rs.split_at_mut(plain);
            let reader = &mut self.reader;
            let (mut ended, mut done) = (self.wave_ended, false);
            let mut i = 0;
            while i < lp.len() {
                // Samples continue from what the reader has already decoded,
                // with no call in the loop: around a call, every float would
                // have to leave the registers. A sample's reads come after
                // its output, which does not depend on them, so that the loop
                // can stop between two reads for the reader to decode more.
                let ahead = if ended { &[][..] } else { reader.ahead() };
                let mut taken = 0;
                let mut more = false;
                for (l, r) in lp[i..].iter_mut().zip(rp[i..].iter_mut()) {
                    // SmileBASIC 4 reads instruments through the waveforms'
                    // filter too, over the sample before and the two after:
                    // let s = interpolate(&[before, s0, s1, after], frac);
                    // Here they stay linear: the filter made songs of many
                    // notes take a quarter longer to mix, and the
                    // instruments' samples are 32 kHz and more, where the
                    // two sound nearly the same.
                    let s = s0 + (s1 - s0) * frac as f32;
                    frac += step;
                    g0 += d0;
                    g1 += d1;
                    *l += s * g0;
                    *r += s * g1;
                    i += 1;
                    while frac >= 1.0 {
                        let Some(&next) = ahead.get(taken) else {
                            more = true;
                            break;
                        };
                        frac -= 1.0;
                        s0 = s1;
                        s1 = f32::from(next) / 32768.0;
                        taken += 1;
                    }
                    if more {
                        break;
                    }
                }
                reader.skip(taken);
                if more {
                    let at = (s0, s1, frac);
                    (s0, s1, frac) = read(reader, wave, data, at, &mut ended, &mut done);
                    if done {
                        break;
                    }
                }
            }
            if !done && last == 1 {
                let s = s0 + (s1 - s0) * frac as f32;
                frac += step;
                if frac >= 1.0 {
                    let at = (s0, s1, frac);
                    (s0, s1, frac) = read(reader, wave, data, at, &mut ended, &mut done);
                }
                g0 += d0;
                g1 += d1;
                let snap = |g: f32| if g.abs() < 1e-6 { 0.0 } else { g };
                (g0, g1) = (snap(g0), snap(g1));
                ll[0] += s * g0;
                rl[0] += s * g1;
            }
            (self.s0, self.s1, self.frac, self.gains) = (s0, s1, frac, [g0, g1]);
            (self.wave_ended, self.done) = (ended, done);
            if done {
                return;
            }
        }
    }

    /// The first version of [`Voice::render`], one sample at a time, to
    /// compare with.
    #[cfg(test)]
    fn render_by_sample(&mut self, bank: &Bank, ch: &Channel, left: &mut [f32], right: &mut [f32]) {
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

/// The work done when the position passes a sample of the waveform: the
/// samples on either side advance, and the note is done once the position
/// passes the silence after the waveform's end.
#[inline(always)]
fn read(
    reader: &mut Reader,
    wave: &WaveInfo,
    data: &[u8],
    (mut s0, mut s1, mut frac): (f32, f32, f64),
    ended: &mut bool,
    done: &mut bool,
) -> (f32, f32, f64) {
    while frac >= 1.0 {
        frac -= 1.0;
        if *ended {
            *done = true;
        }
        s0 = s1;
        s1 = match reader.next_of(wave, data) {
            Some(s) => f32::from(s) / 32768.0,
            None => {
                *ended = true;
                0.0
            }
        };
    }
    (s0, s1, frac)
}

pub(crate) struct Sampler {
    bank: Arc<Bank>,
    /// The waveforms kept decoded, which the sound system's synthesizers
    /// share.
    decoded: Shared,
    channels: [Channel; CHANNELS],
    voices: Vec<Voice>,
}

impl Sampler {
    pub(crate) fn new(bank: Arc<Bank>, decoded: Shared) -> Sampler {
        Sampler {
            bank,
            decoded,
            channels: [Channel::default(); CHANNELS],
            voices: Vec::new(),
        }
    }

    /// Starts a note of `region` on `channel`, its pan moved by `side`.
    fn start(&mut self, channel: usize, key: i32, velocity: i32, region: Region, side: f32) {
        let wave = &self.bank.waves[usize::from(region.wave)];
        let mut reader = Decoded::reader(&self.decoded, &self.bank, usize::from(region.wave));
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
            side,
            reader,
            s0,
            s1,
            frac: 0.0,
            base_step: f64::from(wave.rate) / f64::from(RATE) * 2f64.powf(semitones / 12.0),
            step: 0.0,
            velocity: square(velocity),
            envelope: Envelope::new(match self.channels[channel].envelope {
                // @E keeps the instrument's hold.
                Some(e) => Adshr {
                    hold: region.envelope.hold,
                    ..e.into()
                },
                None => region.envelope,
            }),
            in_tick: 0,
            begun: false,
            gains: [0.0; 2],
            deltas: [0.0; 2],
            lfos: Lfos::default(),
            porta: 0.0,
            wave_ended: false,
            done: false,
        });
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
        // A stereo waveform plays as two notes, its left channel on the left
        // and its right on the right, and the pan moves both: at P0 the
        // right channel sounds in the middle.
        match self.bank.waves[usize::from(region.wave)].right {
            Some(right) => {
                self.start(channel, key, velocity, region, -1.0);
                let region = Region {
                    wave: right,
                    ..region
                };
                self.start(channel, key, velocity, region, 1.0);
            }
            None => self.start(channel, key, velocity, region, 0.0),
        }
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
        let pan_offset = self.channels[0].pan_offset;
        self.channels = [Channel {
            pan_offset,
            ..Channel::default()
        }; CHANNELS];
    }

    fn pan_offset(&mut self, pan: i32) -> bool {
        let at = pan_position(pan);
        self.channels.iter_mut().for_each(|c| c.pan_offset = at);
        true
    }

    fn silent(&self) -> bool {
        self.voices.is_empty()
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
/// and release are the time a fall of 100 dB takes at their rate.
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
    let mut e = Envelope::new(adshr);
    let mut ms = 0;
    while e.stage == Stage::Attack && ms < 1_000_000 {
        e.step_ms();
        ms += 1;
    }
    // 100 dB, a thousand tenths, at so many tenths per millisecond.
    let fall = |value: u8| 1.0 / f64::from(fall_rate(value));
    Times {
        attack: f64::from(ms) / 1000.0,
        hold: f64::from(e.hold) / 1000.0,
        decay: fall(adshr.decay),
        sustain_db: f64::from(-level(adshr.sustain)) / 10.0,
        release: fall(adshr.release),
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::sound::bank::Encoding;

    #[test]
    fn decibels_as_the_engine_reads_them() {
        let near = |db: f32, gain: f32| (volume_ratio(db) - gain).abs() < gain * 1e-3;
        assert_eq!(volume_ratio(0.0), 1.0);
        assert!(near(-6.0, 0.5012) && near(6.0, 1.9953));
        // Truncated to tenths of a dB, toward 0.
        assert!(near(0.19, 1.0116) && near(-0.19, 0.9886));
        // No higher than +6 dB, however far it asks.
        assert!(near(6.1, 1.9953) && near(756.0, 1.9953) && near(f32::MAX, 1.9953));
        // Silent from -90.4 dB down.
        assert!(near(-90.3, 3.055e-5));
        assert_eq!(volume_ratio(-90.4), 0.0);
        assert_eq!(volume_ratio(-756.0), 0.0);
    }

    /// One instrument: a looping square wave at 48 kHz, eight samples per
    /// cycle, sounding at key 60.
    fn sampler(adshr: Adshr, ignore_note_off: bool) -> Sampler {
        Sampler::new(Arc::new(bank(adshr, ignore_note_off)), Decoded::new())
    }

    pub(crate) fn bank(adshr: Adshr, ignore_note_off: bool) -> Bank {
        let samples: Vec<i16> = (0..8).map(|i| if i < 4 { 16384 } else { -16384 }).collect();
        Bank {
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
                right: None,
                rate: RATE,
                len: 8,
                loop_start: Some(0),
                encoding: Encoding::Pcm16,
                data: 0..16,
            }],
            data: samples.iter().flat_map(|s| s.to_le_bytes()).collect(),
        }
    }

    pub(crate) const ORGAN: Adshr = Adshr {
        attack: 127,
        decay: 127,
        sustain: 127,
        hold: 0,
        release: 120,
    };

    /// A long note on instrument 0, as a preset effect.
    fn effect() -> std::sync::Arc<crate::sound::seq::Sequence> {
        Arc::new(crate::sound::seq::Sequence {
            volume: 127,
            start: 0,
            code: vec![60, 127, 0x83, 0x60, 0xFF],
        })
    }

    /// The peak of a stretch of `render` once the note is playing.
    fn settled_peak(mut render: impl FnMut(&mut [f32], &mut [f32])) -> f32 {
        let (mut l, mut r) = (vec![0.0; 800], vec![0.0; 800]);
        for _ in 0..6 {
            l.fill(0.0);
            r.fill(0.0);
            render(&mut l, &mut r);
        }
        r.iter().fold(0f32, |m, x| m.max(x.abs()))
    }

    #[test]
    fn beep_plays_a_preset_as_it_is_at_its_default_volume_and_scales_linearly() {
        use crate::sound::beep::{BeepSound, Beeps};
        use crate::sound::seq::Player;
        use crate::sound::synth::Instruments;
        let instruments = Instruments::Bank(Arc::new(bank(ORGAN, false)));
        let preset = |volume| {
            let mut beeps = Beeps::new(Some(&instruments));
            let what = BeepSound::Sequence(effect());
            beeps.play(0, what, 0, volume, 64, false).unwrap();
            settled_peak(|l, r| beeps.render(l, r))
        };
        let mut synth = instruments.synth(&Decoded::new());
        let mut player = Player::new(effect());
        player.begin_block();
        player.tick(synth.as_mut());
        let plain = settled_peak(|l, r| synth.render(l, r));
        assert!(plain > 0.01);
        assert!((preset(64) / plain - 1.0).abs() < 0.01);
        assert!((preset(127) / plain - 127.0 / 64.0).abs() < 0.02);
        assert!((preset(32) / plain - 0.5).abs() < 0.01);
    }

    /// A mono effect panned to one side by BEEP plays there at equal
    /// power, 1.41 times as loud as in the middle, and BEEPPAN moves it.
    #[test]
    fn beep_pans_a_preset_at_equal_power() {
        use crate::sound::beep::{BeepSound, Beeps};
        use crate::sound::synth::Instruments;
        let instruments = Instruments::Bank(Arc::new(bank(ORGAN, false)));
        let sides = |pan, moved: Option<i32>| {
            let mut beeps = Beeps::new(Some(&instruments));
            let h = beeps.play(0, BeepSound::Sequence(effect()), 0, 64, pan, false);
            if let Some(to) = moved {
                beeps.set_pan(h.unwrap(), to);
            }
            let (mut l, mut r) = (vec![0.0; 800], vec![0.0; 800]);
            for _ in 0..6 {
                l.fill(0.0);
                r.fill(0.0);
                beeps.render(&mut l, &mut r);
            }
            let peak = |v: &[f32]| v.iter().fold(0f32, |m, x| m.max(x.abs()));
            (peak(&l), peak(&r))
        };
        let (cl, cr) = sides(64, None);
        assert!(cl > 0.01 && (cl - cr).abs() < 1e-4);
        for (l, r) in [sides(0, None), sides(64, Some(0))] {
            assert!((l / cl - 2f32.sqrt()).abs() < 0.01 && r < 1e-4, "{l} {r}");
        }
        let (l, r) = sides(127, None);
        assert!(l < 1e-4 && (r / cr - 2f32.sqrt()).abs() < 0.01);
    }

    /// A stereo waveform's channels play on their own sides, and the pan
    /// moves both: hard left, the right channel sounds in the middle.
    #[test]
    fn stereo_waves_play_each_channel_on_its_side() {
        let mut b = bank(ORGAN, false);
        let right = WaveInfo {
            right: None,
            ..b.waves[0].clone()
        };
        b.waves[0].right = Some(1);
        b.waves.push(right);
        let sides = |pan: i32| {
            let mut s = Sampler::new(Arc::new(b.clone()), Decoded::new());
            s.pan(0, pan);
            s.note_on(0, 60, 127);
            let (mut l, mut r) = (vec![0.0; 960], vec![0.0; 960]);
            s.render(&mut l, &mut r);
            s.render(&mut l, &mut r);
            let peak = |v: &[f32]| v.iter().fold(0f32, |m, x| m.max(x.abs()));
            (peak(&l), peak(&r), s.voices.len())
        };
        let (l, r, voices) = sides(64);
        assert_eq!(voices, 2);
        assert!(l > 0.01 && (l - r).abs() < 1e-6);
        // Each channel alone on its side is as loud as a mono note on that
        // side.
        let mono = {
            let mut s = sampler(ORGAN, false);
            s.pan(0, 0);
            s.note_on(0, 60, 127);
            peak(&mut s, 960);
            peak(&mut s, 960)
        };
        assert!((l - mono).abs() < 1e-6);
        let (l, r, _) = sides(0);
        let half = std::f32::consts::FRAC_1_SQRT_2;
        assert!((l - mono * (1.0 + half)).abs() < 1e-4 && (r - mono * half).abs() < 1e-4);
    }

    #[test]
    fn beep_plays_an_instrument_at_70_of_127_of_its_note_by_default() {
        use crate::sound::beep::instrument_volume;
        let level = |volume| square(instrument_volume(volume));
        assert!((level(64) - 70.0 / 127.0).abs() < 0.01);
        assert!((level(32) - 35.0 / 127.0).abs() < 0.01);
        assert_eq!(level(0), 0.0);
        assert_eq!(instrument_volume(127), 127);
    }

    fn peak(s: &mut Sampler, samples: usize) -> f32 {
        let (mut l, mut r) = (vec![0.0; samples], vec![0.0; samples]);
        s.render(&mut l, &mut r);
        l.iter().chain(&r).fold(0f32, |m, x| m.max(x.abs()))
    }

    /// A bank of four waves: ADPCM, looping and not, and PCM.
    fn mixed_bank(adshr: Adshr) -> Bank {
        let mut b = bank(adshr, false);
        let mut data = b.data.clone();
        let start = data.len();
        for i in 0..640u32 {
            data.push((i.wrapping_mul(2_654_435_761) >> 13) as u8);
        }
        let coefs = [
            2048, -1024, 1024, 512, -512, 256, 300, -200, 0, 0, 1800, -800, 100, 50, 4, 8,
        ];
        let adpcm = |loop_start| WaveInfo {
            right: None,
            rate: RATE,
            len: 1000,
            loop_start,
            encoding: Encoding::Adpcm {
                coefs,
                loop_history: [10, -10],
            },
            data: start..start + 640,
        };
        b.waves.push(adpcm(Some(100)));
        b.waves.push(adpcm(None));
        b.waves.push(WaveInfo {
            right: None,
            rate: RATE / 2,
            len: 8,
            loop_start: None,
            encoding: Encoding::Pcm16,
            data: 0..16,
        });
        let region = b.regions[0];
        for wave in 1..4 {
            b.regions.push(Region { wave, ..region });
        }
        b.instruments = (0..4).map(|i| i..i + 1).collect();
        b.data = data;
        b
    }

    #[test]
    fn runs_of_samples_give_what_each_sample_gave() {
        let mut seed = 0xACE1_2345u32;
        let mut next = move |n: u32| {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            seed % n
        };
        for round in 0..300 {
            let adshr = Adshr {
                attack: next(128) as u8,
                decay: next(128) as u8,
                sustain: next(128) as u8,
                hold: next(20) as u8,
                release: next(128) as u8,
            };
            let mut a = Sampler::new(Arc::new(mixed_bank(adshr)), Decoded::new());
            for c in 0..3 {
                a.program(c, next(4) as i32);
                a.pan(c, next(128) as i32);
                a.modulation(c, next(128) as i32);
                a.note_on(c, 20 + next(100) as i32, 1 + next(127) as i32);
            }
            let (mut l, mut r) = (vec![0.0; 800], vec![0.0; 800]);
            let (mut lw, mut rw) = (vec![0.0; 800], vec![0.0; 800]);
            for frame in 0..30 {
                let bank = a.bank.clone();
                let channels = a.channels;
                let mut plain = a.voices.clone();
                // Blocks of any length, as well as of a frame's length.
                let n = if frame % 3 == 0 {
                    1 + next(800) as usize
                } else {
                    800
                };
                l[..n].fill(0.0);
                r[..n].fill(0.0);
                lw[..n].fill(0.0);
                rw[..n].fill(0.0);
                for (v, w) in a.voices.iter_mut().zip(plain.iter_mut()) {
                    v.render(&bank, &channels[v.channel], &mut l[..n], &mut r[..n]);
                    w.render_by_sample(&bank, &channels[w.channel], &mut lw[..n], &mut rw[..n]);
                    assert_eq!(v.done, w.done, "round {round}");
                    if !v.done {
                        assert_eq!((v.s0, v.s1, v.frac), (w.s0, w.s1, w.frac));
                        assert_eq!((v.gains, v.in_tick), (w.gains, w.in_tick));
                    }
                }
                let bits = |v: &[f32]| v.iter().map(|s| s.to_bits()).collect::<Vec<_>>();
                assert_eq!(bits(&l[..n]), bits(&lw[..n]), "round {round}");
                assert_eq!(bits(&r[..n]), bits(&rw[..n]), "round {round}");
                a.voices.retain(|v| !v.done);
                if frame == 10 {
                    a.note_off(0, 0);
                    a.bend(1, next(800) as i32 - 400);
                }
            }
        }
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
    fn decay_to_a_sustain_of_0_ends_the_note() {
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
        assert_eq!(level(127), 0.0);
        assert_eq!(level(0), -723.0);
        assert_eq!(level(1), -722.0);
        assert_eq!(level(3), -651.0);
        // Half the value is about -12 dB, squared.
        assert_eq!(level(64), -119.0);
        assert_eq!(attack_factor(127), 0.0);
        assert!((attack_factor(0) - 0.999_217_5).abs() < 1e-6);
        assert!((attack_factor(109) - 0.890_06).abs() < 1e-4);
        assert_eq!(fall_rate(127), 65535.0);
        assert_eq!(fall_rate(10), 21.0 / 640.0);
        assert_eq!(fall_rate(116), 1.2);
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
        assert_eq!(t.attack, 0.001);
        // (2 + 1)^2 / 4 ms.
        assert_eq!(t.hold, 0.002);
        assert!((t.decay - 1.0 / 24.0).abs() < 1e-9);
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
