//! PCMSTREAM: arrays of samples played as ring buffers, which the program
//! may write ahead of where they play.

use sb_core::value::{Array, ArrayData, ArrayRef};

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
    source: Option<Source>,
    /// Samples in each side's ring. PCMSTREAM takes it from the arrays and
    /// it stays, whatever RESIZE, PUSH and the like do to them afterwards.
    len: usize,
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
            len: 0,
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

/// The sample at `at` as PCMSTREAM plays it: the low 16 bits of an integer,
/// so that 65535 plays as -1, and a real cut to a whole number and held
/// within 16 bits. There is silence past the end of an array, which the
/// program may have made shorter since the stream started.
#[inline]
fn sample(a: &Array, at: usize) -> f64 {
    match &a.data {
        ArrayData::Int(v) => v.get(at).map_or(0.0, |&n| f64::from(n as i16)),
        ArrayData::Real(v) => v.get(at).map_or(0.0, |&x| f64::from(x as i16)),
        ArrayData::Str(_) => 0.0,
    }
}

impl Pcm {
    /// PCMSTREAM: plays `source` from its start at `rate` samples a
    /// second, at full volume on both sides whatever PCMVOL set before.
    pub fn start(&mut self, source: Source, rate: f64) {
        self.len = match &source {
            Source::Mono(a) | Source::Pair(a, _) => a.borrow().len(),
            Source::Rows(a) => a.borrow().dims.get(1).copied().unwrap_or(0),
        };
        self.source = Some(source);
        self.rate = rate;
        self.pos = 0.0;
        self.playing = true;
        self.volume = [Level::at(FULL); 2];
    }

    /// PCMCONT: goes on with the stream PCMSTOP stopped, if there is one.
    pub fn resume(&mut self) {
        self.playing = self.source.is_some();
    }

    /// The next playing position as an index in the arrays.
    pub fn index(&self) -> usize {
        match self.len {
            0 => 0,
            len => self.pos as usize % len,
        }
    }

    pub fn render(&mut self, left: &mut [f32], right: &mut [f32]) {
        if !self.playing || self.len == 0 {
            return;
        }
        let Some(source) = &self.source else {
            return;
        };
        // The right side's array, when it is not the left's, and where its
        // samples start. A 2D array's second row starts where the array
        // says now, as its first does.
        let (a, b, base) = match source {
            Source::Mono(a) => (a.borrow(), None, 0),
            Source::Rows(a) => {
                let a = a.borrow();
                let base = a.dims.get(1).copied().unwrap_or(0);
                (a, None, base)
            }
            Source::Pair(a, b) => (a.borrow(), Some(b.borrow()), 0),
        };
        let b = b.as_ref().unwrap_or(&a);
        let step = self.rate / f64::from(RATE);
        let len = self.len as f64;
        let [vl, vr] = &mut self.volume;
        for (l, r) in left.iter_mut().zip(right.iter_mut()) {
            let i = self.pos as usize;
            *l += (sample(&a, i) * vl.value * SCALE) as f32;
            *r += (sample(b, base + i) * vr.value * SCALE) as f32;
            vl.step();
            vr.step();
            self.pos = (self.pos + step) % len;
        }
    }
}
