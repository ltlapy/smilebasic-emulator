//! PCMSTREAM: arrays of samples played as ring buffers, which the program
//! may write ahead of where they play.

use sb_core::value::ArrayRef;

use super::RATE;

/// Full scale for PCM samples and PCMVOL.
pub const FULL: f64 = 32767.0;

/// PCMSTREAM's level in the mix: a full-scale sample at full PCMVOL plays
/// at 0.35 of full scale, against the other sounds at their levels.
const GAIN: f64 = 0.35;

/// A sample times its side's PCMVOL, to the mix's scale.
const SCALE: f64 = GAIN / (FULL * FULL);

/// Where the samples are: one array (a 2D one has the left channel in its
/// first row and the right in its second), or one for each side.
pub enum Source {
    Mono(ArrayRef),
    Rows(ArrayRef),
    Pair(ArrayRef, ArrayRef),
}

/// PCMVOL's volume for a side, moving to a target over some samples.
#[derive(Clone, Copy, Debug)]
pub struct Level {
    pub value: f64,
    target: f64,
    step: f64,
}

impl Level {
    pub fn at(value: f64) -> Level {
        Level {
            value,
            target: value,
            step: 0.0,
        }
    }

    pub fn to(&mut self, target: f64, samples: u32) {
        self.target = target;
        if samples == 0 {
            self.value = target;
            self.step = 0.0;
        } else {
            self.step = (target - self.value) / f64::from(samples);
        }
    }

    fn step(&mut self) {
        if self.step != 0.0 {
            self.value += self.step;
            if (self.target - self.value) * self.step <= 0.0 {
                self.value = self.target;
                self.step = 0.0;
            }
        }
    }
}

pub struct Pcm {
    pub source: Option<Source>,
    /// Samples a second of the source.
    pub rate: f64,
    /// The next sample to play, with the fraction between samples.
    pub pos: f64,
    pub playing: bool,
    pub volume: [Level; 2],
}

impl Default for Pcm {
    fn default() -> Self {
        Pcm {
            source: None,
            rate: f64::from(RATE),
            pos: 0.0,
            playing: false,
            // PCMVOL reads 0 0 until the first PCMSTREAM sets both sides
            // to full volume; RUN leaves it as it is. PCMVOL without a
            // stream is kept and read back.
            // UNVERIFIED: the 0 0 at start-up.
            volume: [Level::at(0.0); 2],
        }
    }
}

fn read(a: &ArrayRef, row: usize, i: usize) -> (f64, usize) {
    let a = a.borrow();
    let (rows, len) = match a.dims[..] {
        [_, len] => (2, len),
        _ => (1, a.len()),
    };
    if len == 0 {
        return (0.0, 0);
    }
    let at = row.min(rows - 1) * len + i % len;
    (a.get(at).to_real().unwrap_or(0.0), len)
}

impl Pcm {
    /// Samples in each side's ring.
    fn len(&self) -> usize {
        match &self.source {
            None => 0,
            Some(Source::Mono(a) | Source::Pair(a, _)) => a.borrow().len(),
            Some(Source::Rows(a)) => a.borrow().dims.get(1).copied().unwrap_or(0),
        }
    }

    /// The next playing position as an index in the arrays.
    pub fn index(&self) -> usize {
        match self.len() {
            0 => 0,
            len => self.pos as usize % len,
        }
    }

    pub fn render(&mut self, left: &mut [f32], right: &mut [f32]) {
        if !self.playing {
            return;
        }
        let Some(source) = &self.source else {
            return;
        };
        let step = self.rate / f64::from(RATE);
        for (l, r) in left.iter_mut().zip(right.iter_mut()) {
            let i = self.pos as usize;
            let (a, b, len) = match source {
                Source::Mono(a) => {
                    let (s, len) = read(a, 0, i);
                    (s, s, len)
                }
                Source::Rows(a) => {
                    let (s, len) = read(a, 0, i);
                    (s, read(a, 1, i).0, len)
                }
                Source::Pair(a, b) => {
                    let (s, len) = read(a, 0, i);
                    (s, read(b, 0, i).0, len)
                }
            };
            if len == 0 {
                return;
            }
            let [vl, vr] = &mut self.volume;
            *l += (a * vl.value * SCALE) as f32;
            *r += (b * vr.value * SCALE) as f32;
            vl.step();
            vr.step();
            self.pos = (self.pos + step) % len as f64;
        }
    }
}
