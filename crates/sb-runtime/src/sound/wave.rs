//! Voices that are not in the SoundFont: the PSG's pulse waves (@216 to
//! @222), noise (@223) and the user waveforms of WAVSET and WAVSETA (@224
//! to @255), each with an envelope.

use std::rc::Rc;

use super::RATE;

/// A waveform: one cycle, or a sample with a loop.
#[derive(Clone, Debug, PartialEq)]
pub struct Wave {
    pub samples: Vec<f32>,
    /// The note the waveform plays at as it is.
    pub base_note: i32,
    /// Samples a second for a sample; `None` for one cycle of a wave.
    pub rate: Option<f64>,
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

/// Seconds a stage takes for its value.
// UNVERIFIED: SmileBASIC's curve; 127 is 2 ms, every 10 down twice as long,
// so 0 is about 13 s.
fn stage_seconds(value: i32) -> f64 {
    0.002 * 2f64.powf(f64::from(127 - value.clamp(0, 127)) / 10.0)
}

/// The change in level a sample for a stage of `value`.
fn stage_step(value: i32) -> f64 {
    1.0 / (stage_seconds(value) * f64::from(RATE))
}

/// The PSG's pulse widths for @216 to @222, as eighths of a cycle.
// UNVERIFIED: SmileBASIC 4's PSG tones.
const PULSES: [usize; 7] = [1, 2, 3, 4, 5, 6, 7];

/// @216 to @222 as one cycle of a pulse wave.
pub fn pulse(program: i32) -> Wave {
    let high = PULSES[(program - 216) as usize];
    Wave {
        samples: (0..8).map(|i| if i < high { 0.5 } else { -0.5 }).collect(),
        base_note: 69,
        rate: None,
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
        .map(|s| s * 0.5)
        .collect();
    Wave {
        samples,
        base_note: 69,
        // The whole sound repeats too rarely to hear as a pitch.
        rate: Some(f64::from(RATE)),
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Stage {
    Attack,
    Decay,
    Sustain,
    Release,
    Done,
}

/// One note of a waveform.
#[derive(Clone, Debug)]
pub struct Voice {
    wave: Rc<Wave>,
    envelope: Envelope,
    pub key: i32,
    pos: f64,
    step: f64,
    stage: Stage,
    level: f64,
    /// Left and right gains.
    pub gains: (f32, f32),
    /// Plays the loop for ever, as BEEP does when asked.
    looping: bool,
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
        let envelope = envelope.unwrap_or(wave.envelope);
        let mut v = Voice {
            wave,
            envelope,
            key,
            pos: 0.0,
            step: 0.0,
            stage: Stage::Attack,
            level: 0.0,
            gains: (0.5, 0.5),
            looping,
        };
        v.set_pitch(cents);
        v
    }

    pub fn set_pitch(&mut self, cents: i32) {
        let ratio = 2f64
            .powf((f64::from(self.key - self.wave.base_note) * 100.0 + f64::from(cents)) / 1200.0);
        self.step = match self.wave.rate {
            Some(rate) => rate * ratio / f64::from(RATE),
            // One cycle at the base note is A=440 Hz shifted to the note.
            None => {
                let freq = 440.0 * 2f64.powf(f64::from(self.wave.base_note - 69) / 12.0);
                self.wave.samples.len() as f64 * freq * ratio / f64::from(RATE)
            }
        };
    }

    pub fn release(&mut self) {
        if self.stage != Stage::Done {
            self.stage = Stage::Release;
        }
    }

    pub fn done(&self) -> bool {
        self.stage == Stage::Done
    }

    fn envelope_step(&mut self) {
        let e = self.envelope;
        let sustain = f64::from(e.sustain.clamp(0, 127)) / 127.0;
        match self.stage {
            Stage::Attack => {
                self.level += stage_step(e.attack);
                if self.level >= 1.0 {
                    self.level = 1.0;
                    self.stage = Stage::Decay;
                }
            }
            Stage::Decay => {
                self.level -= stage_step(e.decay);
                if self.level <= sustain {
                    self.level = sustain;
                    self.stage = Stage::Sustain;
                }
            }
            Stage::Sustain => {}
            // Release 0 does not stop the note; only SNDSTOP does.
            Stage::Release if e.release == 0 => {}
            Stage::Release => {
                self.level -= stage_step(e.release);
                if self.level <= 0.0 {
                    self.level = 0.0;
                    self.stage = Stage::Done;
                }
            }
            Stage::Done => {}
        }
        if self.stage == Stage::Sustain && sustain == 0.0 {
            self.stage = Stage::Done;
        }
    }

    /// Adds the voice into the buffers.
    pub fn render(&mut self, left: &mut [f32], right: &mut [f32]) {
        let wave = self.wave.clone();
        let samples = &wave.samples;
        if samples.is_empty() {
            self.stage = Stage::Done;
            return;
        }
        let (start, end) = self.wave.looped;
        let end = end.clamp(1, samples.len());
        let start = start.min(end - 1);
        let loops =
            self.wave.rate.is_none() || self.looping || self.wave.looped != (0, samples.len());
        for (l, r) in left.iter_mut().zip(right.iter_mut()) {
            if self.stage == Stage::Done {
                return;
            }
            let i = self.pos as usize;
            if i >= end {
                if loops {
                    self.pos = start as f64 + (self.pos - end as f64) % (end - start) as f64;
                } else {
                    self.stage = Stage::Done;
                    return;
                }
            }
            let s = samples[(self.pos as usize).min(samples.len() - 1)] * self.level as f32;
            *l += s * self.gains.0;
            *r += s * self.gains.1;
            self.pos += self.step;
            self.envelope_step();
        }
    }
}

/// The gains for a volume, velocity and pan, each 0 to 127.
pub fn gains(volume: i32, velocity: i32, pan: i32) -> (f32, f32) {
    let v = (volume.clamp(0, 127) * velocity.clamp(0, 127)) as f32 / (127.0 * 127.0);
    let pan = pan.clamp(0, 127) as f32 / 127.0;
    let angle = pan * std::f32::consts::FRAC_PI_2;
    (v * angle.cos(), v * angle.sin())
}

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

    #[test]
    fn a_cycle_plays_at_the_note() {
        // 8 samples a cycle at 440 Hz: a step of 8 * 440 / 48000.
        let v = Voice::new(Rc::new(pulse(219)), None, 81, 0, false);
        assert!((v.step - 2.0 * 8.0 * 440.0 / 48000.0).abs() < 1e-9);
    }
}
