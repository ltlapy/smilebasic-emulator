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
    /// end of its loop, then the samples earlier ones left past that, each
    /// in `unit`s. 8-bit waveforms are kept as 16-bit ones, exactly, so a
    /// sample takes two bytes: WAVSETA can make one as long as an array.
    pub samples: Vec<i16>,
    /// The value a sample of 1 plays as: [`SIXTEEN_BIT`] for all waveforms
    /// but the noise, so that one written over another's memory keeps the
    /// rest of that memory sounding the same.
    pub unit: f32,
    /// The note at which the waveform plays unchanged.
    pub base_note: i32,
    /// Samples per second for a sample; `None` for a repeating wave, which
    /// steps through `cycle` samples in one cycle of its base note.
    pub rate: Option<f64>,
    pub cycle: f64,
    /// The start and end (exclusive) of the loop; the end is also the end
    /// of the definition.
    pub looped: (usize, usize),
    /// Whether BEEP's note loops it.
    pub beep_loop: bool,
    /// The length of BEEP's note, in [`BEEP_TICKS_A_SECOND`]; `None` for
    /// as long as nothing stops it. 0 plays nothing.
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

/// The values in the form SmileBASIC's own instruments use: waveforms move
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

/// A 16-bit sample's unit, a power of two, so that each sample times it
/// is exact.
pub const SIXTEEN_BIT: f32 = 1.0 / 32768.0;

/// An 8-bit sample `b`, offset by 128, as a 16-bit sample, with the same
/// full scale as a 16-bit sample.
fn from_eight_bit(b: u8) -> i16 {
    (i16::from(b) - 128) << 8
}

/// The samples per second of 8-bit waveforms at their reference pitch.
pub const EIGHT_BIT_RATE: f64 = 28160.0;

/// BEEP counts its notes of a user waveform in ticks; this many make a
/// second.
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
///
/// The stretch steps in 8.8 fixed point, the same as on SmileBASIC 4, so
/// a length just under a power of two drops its last few samples.
pub fn eight_bit(bytes: &[u8], pass: usize, base_note: i32, envelope: Envelope) -> Wave {
    // SmileBASIC 4 counts in 32 bits, which overflow past 8M samples; 64
    // bits keep those in the array.
    let step = (bytes.len() as i64 * 256) / pass as i64;
    let samples: Vec<i16> = (0..pass as i64)
        .map(|i| from_eight_bit(bytes[((i * step) >> 8) as usize]))
        .collect();
    Wave {
        samples,
        unit: SIXTEEN_BIT,
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
/// 8-bit waveform, which SmileBASIC 4's own pulse waves are near
/// (TEST41). They play when the instruments do not come from SmileBASIC
/// 4's sound archive.
pub fn pulse(program: i32) -> Wave {
    let high = PULSES[(program - 216) as usize];
    Wave {
        samples: (0..8)
            .map(|i| from_eight_bit(if i < high { 255 } else { 0 }))
            .collect(),
        unit: SIXTEEN_BIT,
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
            i16::from(x & 0x8000 != 0)
        })
        .collect();
    Wave {
        samples,
        // Full scale, which a 16-bit sample cannot reach.
        unit: 1.0,
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

/// The waveform @224 to @255 play until WAVSET or WAVSETA sets them: the
/// sound archive holds a sine for @224, and SmileBASIC 4 starts all of them
/// on that one, with a one-second note for BEEP at the sine's own pitch.
/// `None` without the sound archive's bank. The archive also keeps a
/// shorter copy of the sine for notes from key 96 up, which sounds the same
/// at the same pitch.
pub fn unset(bank: &Bank) -> Option<Wave> {
    let region = bank.region(224, 57)?;
    let info = bank.waves.get(usize::from(region.wave))?;
    let mut reader = Reader::default();
    let samples: Vec<i16> = (0..info.len)
        .map_while(|_| reader.next(info, &bank.data))
        .collect();
    let end = samples.len();
    let e = region.envelope;
    let v = i32::from;
    Some(Wave {
        samples,
        unit: SIXTEEN_BIT,
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
    /// The sound archive's bank, for the waveform they play until set.
    bank: Option<Arc<Bank>>,
    /// The waveform they play until set, decoded when it is first needed.
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
    /// Sets the bank where @224 to @255 find the sine they play until set,
    /// or none.
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
    /// are sounding continue to read it with the length, loop and rate
    /// they started with: a 32-sample sine written while a 512-sample
    /// square sounds plays as the sine's cycle and then the rest of the
    /// square (TEST40, TEST41).
    pub fn define(&mut self, n: usize, mut wave: Wave) {
        if let Some(old) = &self.user[n]
            && old.samples.len() > wave.samples.len()
        {
            debug_assert_eq!(old.unit, wave.unit);
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

    /// Keeps a note of `program` reading the samples WAVSET last wrote
    /// there; see [`Waves::define`]. The sine an unset waveform plays is
    /// not that memory, so a note of it continues with the sine.
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
    /// The step at the base note, and that note, taken when the note
    /// started.
    rate: f64,
    base_note: i32,
    /// The loop, and whether it plays forever, taken when the note
    /// started.
    looped: (usize, usize),
    loops: bool,
    /// The channel's @MP, @MA and @ML, and how far around they have gone.
    pub(crate) lfo: [sampler::Lfo; 3],
    lfos: sampler::Lfos,
    /// A portamento's glide, in cents.
    pub(crate) porta: f32,
    /// The gain from volume and velocity, and the pan's position.
    gain: f32,
    pan: f32,
    /// Left and right gains, envelope included, moving in a straight line
    /// to the gains of the next step.
    levels: [f32; 2],
    steps: [f32; 2],
    targets: [f32; 2],
    /// Samples into the envelope's step.
    in_tick: usize,
    /// The position has gone around the loop: the sample before its start
    /// is the last sample of the loop.
    wrapped: bool,
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
            wrapped: false,
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

    /// Reads `wave`'s memory from now on in place of the one the note
    /// started with, as happens when WAVSET sets the instrument again
    /// while the note sounds; see [`Waves::define`].
    pub fn follow(&mut self, wave: &Rc<Wave>) {
        if !Rc::ptr_eq(&self.wave, wave) {
            self.wave = wave.clone();
        }
    }

    fn reads(&self, wave: &Rc<Wave>) -> bool {
        Rc::ptr_eq(&self.wave, wave)
    }

    /// A portamento's end: the note continues as `key`.
    pub fn retune(&mut self, key: i32) {
        self.key = key;
        self.porta = 0.0;
        self.set_pitch(self.cents);
    }

    pub fn release(&mut self) {
        self.envelope.release();
    }

    /// Sets the volume, velocity and pan, each 0 to 127. Volume and
    /// velocity are squared, the same as for the instruments.
    pub fn set_mix(&mut self, volume: i32, velocity: i32, pan: i32) {
        self.gain = WAVE_GAIN * sampler::square(volume) * sampler::square(velocity);
        self.pan = sampler::pan_position(pan);
    }

    /// Multiplies the gain [`Voice::set_mix`] set by `by`, and also the
    /// levels reached so far.
    pub(crate) fn scale(&mut self, by: f32) {
        self.gain *= by;
        for x in [&mut self.levels, &mut self.steps, &mut self.targets] {
            x.iter_mut().for_each(|v| *v *= by);
        }
    }

    pub fn done(&self) -> bool {
        self.done
    }

    /// Whether the note can still reach a 16-bit sample, because a
    /// released note takes much longer than that to end.
    pub(crate) fn audible(&self) -> bool {
        const HALF_A_STEP: f32 = 1.0 / 65536.0;
        self.levels[0].abs().max(self.levels[1].abs()) >= HALF_A_STEP
    }

    pub fn released(&self) -> bool {
        self.envelope.released()
    }

    /// The waveform at the position, read through the instruments' filter
    /// ([`sampler::interpolate`]).
    #[cfg(test)]
    fn sample(&self, samples: &[i16], unit: f32) -> f32 {
        sample(
            samples,
            unit,
            self.pos,
            self.looped,
            self.loops,
            self.wrapped,
        )
    }

    /// Steps the envelope and the LFOs, and aims the levels and pitch at
    /// their values for the end of the next step.
    fn tick(&mut self) {
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

    /// Adds the voice into the buffers. A step of the envelope is rendered
    /// as a run, in which only the position moves and the levels slide,
    /// kept in locals; each sample's sums are the same as in
    /// [`Voice::render_by_sample`], in the same order.
    pub fn render(&mut self, left: &mut [f32], right: &mut [f32]) {
        let wave = self.wave.clone();
        let (samples, unit) = (&wave.samples, wave.unit);
        if samples.is_empty() {
            self.done = true;
            return;
        }
        let (start, end) = self.looped;
        let n = left.len().min(right.len());
        let mut at = 0;
        while at < n {
            if self.in_tick == 0 {
                self.tick();
            }
            let run = (sampler::TICK - self.in_tick).min(n - at);
            let (mut pos, mut levels, mut wrapped) = (self.pos, self.levels, self.wrapped);
            let (step, steps, loops) = (self.step, self.steps, self.loops);
            let outs = left[at..at + run].iter_mut().zip(&mut right[at..at + run]);
            for (k, (l, r)) in outs.enumerate() {
                if pos >= end as f64 {
                    if loops {
                        pos = start as f64 + (pos - end as f64) % (end - start) as f64;
                        wrapped = true;
                    } else {
                        (self.pos, self.levels, self.wrapped) = (pos, levels, wrapped);
                        self.in_tick = (self.in_tick + k + 1) % sampler::TICK;
                        self.done = true;
                        return;
                    }
                }
                let s = sample(samples, unit, pos, self.looped, loops, wrapped);
                *l += s * levels[0];
                *r += s * levels[1];
                pos += step;
                levels[0] += steps[0];
                levels[1] += steps[1];
            }
            (self.pos, self.levels, self.wrapped) = (pos, levels, wrapped);
            self.in_tick = (self.in_tick + run) % sampler::TICK;
            at += run;
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

    /// [`Voice::render`] one sample at a time, the way it was first
    /// written, for comparison.
    #[cfg(test)]
    fn render_by_sample(&mut self, left: &mut [f32], right: &mut [f32]) {
        let wave = self.wave.clone();
        let samples = &wave.samples;
        if samples.is_empty() {
            self.done = true;
            return;
        }
        let (start, end) = self.looped;
        for (l, r) in left.iter_mut().zip(right.iter_mut()) {
            if self.in_tick == 0 {
                self.tick();
            }
            self.in_tick = (self.in_tick + 1) % sampler::TICK;
            if self.pos >= end as f64 {
                if self.loops {
                    self.pos = start as f64 + (self.pos - end as f64) % (end - start) as f64;
                    self.wrapped = true;
                } else {
                    self.done = true;
                    return;
                }
            }
            let s = self.sample(samples, wave.unit);
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

/// The waveform at `pos`, read through the instruments' filter
/// ([`sampler::interpolate`]): the sample before and the two after,
/// around the loop when it `loops`; after `wrapped`, the loop also gives
/// the sample before its start. Each sample times `unit` is exact.
#[inline(always)]
fn sample(
    samples: &[i16],
    unit: f32,
    pos: f64,
    (start, end): (usize, usize),
    loops: bool,
    wrapped: bool,
) -> f32 {
    let i = pos as usize;
    let frac = pos - i as f64;
    let h = match samples.get(i.wrapping_sub(1)..i + 3) {
        Some(h) if i + 2 < end => [h[0], h[1], h[2], h[3]],
        _ => {
            let at = |j: usize| {
                let j = if j >= end && loops {
                    start + (j - end) % (end - start)
                } else {
                    j
                };
                samples.get(j).copied().filter(|_| j < end).unwrap_or(0)
            };
            let before = if i == start && wrapped {
                at(end - 1)
            } else if i == 0 {
                0
            } else {
                at(i - 1)
            };
            [before, at(i), at(i + 1), at(i + 2)]
        }
    };
    sampler::interpolate(&h.map(|s| f32::from(s) * unit), frac)
}

/// A full-scale waveform at V127 in the middle records at an eighth of
/// full scale on each side (TEST40), the pan taking 3 dB.
const WAVE_GAIN: f32 = 0.125 * std::f32::consts::SQRT_2;

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// Runs of a step give the same result as one sample at a time, to the
    /// bit: over loops that start anywhere, waveforms that end, samples and
    /// cycles at any pitch, LFOs, portamento and release, in blocks of any
    /// length.
    #[test]
    fn runs_of_samples_give_what_each_sample_gave() {
        let mut seed = 0x2545_F491u32;
        let mut next = move |n: u32| {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            seed % n
        };
        for round in 0..300 {
            let len = 1 + next(600) as usize;
            let looped = match next(3) {
                0 => (0, len),
                1 => (next(len as u32) as usize, len),
                _ => (len, len),
            };
            let wave = Rc::new(Wave {
                samples: (0..len)
                    .map(|_| (next(65536) as i32 - 32768) as i16)
                    .collect(),
                rate: (next(2) == 0).then(|| f64::from(8000 + next(40000))),
                cycle: f64::from(1 + next(64)),
                looped,
                envelope: Envelope {
                    attack: next(128) as i32,
                    decay: next(128) as i32,
                    sustain: next(128) as i32,
                    release: next(128) as i32,
                },
                ..pulse(219)
            });
            let mut a = Voice::new(wave, None, 40 + next(60) as i32, 0, next(2) == 0);
            a.set_mix(next(128) as i32, 1 + next(127) as i32, next(128) as i32);
            a.lfo[0] = sampler::Lfo {
                depth: next(64) as i32,
                range: 1,
                speed: next(40) as i32,
                delay: 0,
            };
            let mut b = a.clone();
            for block in 0..40 {
                let n = 1 + next(900) as usize;
                let (mut l, mut r) = (vec![0.0; n], vec![0.0; n]);
                let (mut lw, mut rw) = (vec![0.0; n], vec![0.0; n]);
                a.render(&mut l, &mut r);
                b.render_by_sample(&mut lw, &mut rw);
                let bits = |v: &[f32]| v.iter().map(|s| s.to_bits()).collect::<Vec<_>>();
                assert_eq!(bits(&l), bits(&lw), "round {round} block {block}");
                assert_eq!(bits(&r), bits(&rw), "round {round} block {block}");
                assert_eq!((a.pos, a.in_tick, a.done), (b.pos, b.in_tick, b.done));
                assert_eq!((a.levels, a.wrapped), (b.levels, b.wrapped));
                for v in [&mut a, &mut b] {
                    match block {
                        10 => v.porta = 250.0,
                        20 => v.release(),
                        _ => {}
                    }
                }
            }
        }
    }

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
        // @E127,116,111,32 keeps ringing long after its note ends; a
        // release of 122, the release that songs give to notes cut short
        // with Q0, still leaves a tail of a few dozen milliseconds.
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
        let wave = |level: i16| {
            Rc::new(Wave {
                samples: vec![level; 16],
                ..pulse(219)
            })
        };
        let mut v = Voice::new(wave(16384), None, 69, 0, false);
        let (mut l, mut r) = (vec![0.0; 960], vec![0.0; 960]);
        v.render(&mut l, &mut r);
        assert!(l[959] > 0.02);
        v.follow(&wave(-16384));
        let (mut l, mut r) = (vec![0.0; 960], vec![0.0; 960]);
        v.render(&mut l, &mut r);
        assert!(l[0] < -0.02);
    }

    #[test]
    fn notes_keep_their_length_when_a_shorter_waveform_is_written() {
        let mut waves = Waves::default();
        let square = |n: usize, level: i16| Wave {
            samples: (0..n)
                .map(|i| if i < n / 2 { level } else { -level })
                .collect(),
            rate: Some(EIGHT_BIT_RATE),
            looped: (0, n),
            ..pulse(219)
        };
        waves.define(0, square(512, 16384));
        let mut v = Voice::new(waves.get(224).unwrap(), None, 69, 0, true);
        waves.define(0, square(32, 8192));
        let w = waves.get(224).unwrap();
        // The memory keeps the old square past the new 32 samples, and
        // new notes play 32 of it.
        assert_eq!((w.samples.len(), w.looped), (512, (0, 32)));
        assert_eq!((w.samples[31], w.samples[300]), (-8192, -16384));
        // The note that was sounding continues over all 512 at its old rate.
        let steady = v.step;
        v.follow(&w);
        assert_eq!((v.looped, v.step), ((0, 512), steady));
    }

    #[test]
    fn vibrato_moves_the_pitch_and_stops() {
        let mut v = Voice::new(Rc::new(pulse(219)), None, 69, 0, false);
        let steady = v.step;
        // @MP32,1,20,0: ±25 cents, 7.8 times per second.
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
        let bytes: Vec<u8> = (0..32).map(|i| if i < 16 { 0 } else { 255 }).collect();
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
                right: None,
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
        assert_eq!(
            (&w.samples[..], w.unit),
            (&[0, 16384, 0, -16384][..], SIXTEEN_BIT)
        );
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

        // All 32 play it until set, each number separately, and nothing
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
        assert_eq!(waves.get(250).unwrap().samples, [0, 16384, 0, -16384]);
        // WAVSET of a number does not reach a note of the sine.
        let mut v = Voice::new(waves.get(250).unwrap(), None, 57, 0, true);
        waves.define(26, pulse(219));
        waves.follow(250, &mut v);
        assert_eq!(v.wave.samples, [0, 16384, 0, -16384]);
    }

    #[test]
    fn eight_bit_waveforms_play_a_pass_at_their_rate() {
        // WAVSET's 32 samples at A, stretched to 512: a pass at 55 Hz,
        // three octaves down.
        let bytes: Vec<u8> = (0..32).collect();
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
    fn eight_bit_waveforms_stretch_in_fixed_point() {
        // 513 samples over 1024 step by 128/256 of a sample, short of
        // 513/1024, so the pass never reaches the last sample.
        let bytes: Vec<u8> = (0..513).map(|i| (i % 256) as u8).collect();
        let w = eight_bit(&bytes, 1024, 69, Envelope::ORGAN);
        let at = |i: usize| ((w.samples[i] >> 8) + 128) as u8;
        assert_eq!((at(1), at(2), at(1023)), (0, 1, (511 % 256) as u8));
    }

    /// A 1 kHz sine at 8000 Hz: the images at 7 and 9 kHz are about 38 and
    /// 62 dB below the tone, whereas holding each sample left them at 17
    /// and 19.
    #[test]
    fn waveforms_are_read_through_a_filter() {
        let samples: Vec<i16> = (0..8000)
            .map(|i| ((i as f32 * std::f32::consts::TAU / 8.0).sin() * 16384.0) as i16)
            .collect();
        let wave = Wave {
            samples,
            rate: Some(8000.0),
            looped: (0, 8000),
            ..pulse(219)
        };
        let mut v = Voice::new(Rc::new(wave), None, 69, 0, true);
        let (mut l, mut r) = (vec![0.0; 48000], vec![0.0; 48000]);
        v.render(&mut l, &mut r);
        let level = |hz: f32| {
            let w = std::f32::consts::TAU * hz / 48000.0;
            let (c, s) = l[4800..]
                .iter()
                .enumerate()
                .fold((0.0, 0.0), |(c, s), (i, x)| {
                    (c + x * (w * i as f32).cos(), s + x * (w * i as f32).sin())
                });
            f32::hypot(c, s)
        };
        let db = |hz: f32| 20.0 * (level(hz) / level(1000.0)).log10();
        assert!((-41.0..-35.0).contains(&db(7000.0)), "{}", db(7000.0));
        assert!(db(9000.0) < -55.0, "{}", db(9000.0));
    }

    #[test]
    fn a_cycle_plays_at_the_note() {
        // 8 samples per cycle at key 81, an octave above 440 Hz: a step of
        // 2 * 8 * 440 / 48000.
        let v = Voice::new(Rc::new(pulse(219)), None, 81, 0, false);
        assert!((v.step - 2.0 * 8.0 * 440.0 / 48000.0).abs() < 1e-9);
    }
}
