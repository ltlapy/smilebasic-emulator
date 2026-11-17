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

/// Samples in the ring a stream plays from, on each side.
const RING: usize = 4096;
/// The feed tops the ring up once it holds fewer samples than this...
const LOW: f32 = 1024.0;
/// ...until it holds more than this.
const HIGH: f32 = 3072.0;
/// Output samples between feeds: the mixer takes the stream in blocks of
/// 5 ms.
const BLOCK: usize = (RATE / 200) as usize;

/// PCMSTREAM's player. The arrays are not played where they are: a feed
/// copies their samples into a ring, at the start of each 5 ms block of
/// output, whenever the ring holds fewer than 1024 samples, until it holds
/// more than 3072; the output plays the ring at the stream's rate. So
/// PCMPOS, where the next sample will be fed from, runs up to 3073 samples
/// ahead of what plays and moves in jumps of a little over 2048, and a
/// sample the program writes after it has been fed does not play until the
/// next time round.
pub struct Pcm {
    source: Option<Source>,
    /// Samples in each side's array. PCMSTREAM takes it from the arrays and
    /// it stays, whatever RESIZE, PUSH and the like do to them afterwards.
    len: usize,
    /// What the ring holds, left and right.
    ring: Vec<[i16; 2]>,
    /// Where the next fed sample goes.
    write: usize,
    /// Samples the ring holds, and where in it the output plays, in single
    /// precision as SmileBASIC 4 counts them.
    held: f32,
    read: f32,
    /// What a sample of output takes from the ring: the rate over 48000.
    step: f32,
    /// For a rate that divides 48000, how many output samples each sample
    /// plays for instead, and how many it has left.
    repeat: u32,
    repeats_left: u32,
    /// The ring is being topped up.
    filling: bool,
    /// The array index the next fed sample comes from: what PCMPOS reads.
    fed: usize,
    /// Output samples until the next block starts.
    to_block: usize,
    pub playing: bool,
    pub volume: [Level; 2],
}

impl Default for Pcm {
    fn default() -> Self {
        Pcm {
            source: None,
            len: 0,
            ring: Vec::new(),
            write: 0,
            held: 0.0,
            read: 0.0,
            step: 1.0,
            repeat: 1,
            repeats_left: 1,
            filling: false,
            fed: 0,
            to_block: 0,
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
fn sample(a: &Array, at: usize) -> i16 {
    match &a.data {
        ArrayData::Int(v) => v.get(at).map_or(0, |&n| n as i16),
        ArrayData::Real(v) => v.get(at).map_or(0, |&x| x as i16),
        ArrayData::Str(_) => 0,
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
        self.set_rate(rate);
        self.restart(0);
        self.ring.clear();
        self.ring.resize(RING, [0, 0]);
        self.playing = true;
        self.volume = [Level::at(FULL); 2];
    }

    /// `PCMSTREAM rate` alone: the stream goes on from where it is at the
    /// new rate.
    pub fn set_rate(&mut self, rate: f64) {
        self.step = (rate / f64::from(RATE)) as f32;
        let rate = rate as u32;
        // A rate that divides 48000 holds each sample for the same number
        // of output samples; any other moves by the step.
        self.repeat = match rate {
            1..=RATE if RATE.is_multiple_of(rate) => RATE / rate,
            _ => 0,
        };
        self.repeats_left = self.repeat;
    }

    /// PCMCONT: goes on with the stream PCMSTOP stopped, if there is one.
    pub fn resume(&mut self) {
        self.playing = self.source.is_some();
    }

    /// PCMPOS: where the next sample will be fed from, an index in the
    /// arrays.
    pub fn index(&self) -> usize {
        self.fed
    }

    /// `PCMPOS n`: the ring empties and fills again from `at`.
    pub fn restart(&mut self, at: usize) {
        self.held = 0.0;
        self.read = 0.0;
        self.write = 0;
        self.repeats_left = self.repeat;
        self.filling = true;
        self.fed = match self.len {
            0 => at,
            len => at % len,
        };
    }

    /// Tops the ring up from the arrays.
    fn feed(&mut self) {
        let Some(source) = &self.source else {
            self.filling = false;
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
        let len = self.len.max(1);
        while self.filling {
            let i = self.fed;
            self.ring[self.write] = [sample(&a, i), sample(b, base + i)];
            self.write = (self.write + 1) % RING;
            self.held += 1.0;
            self.filling = self.held <= HIGH;
            self.fed = (i + 1) % len;
        }
    }

    pub fn render(&mut self, left: &mut [f32], right: &mut [f32]) {
        // PCMSTOP holds the stream where it is, silent.
        // UNVERIFIED: SmileBASIC 4 may go on sending the sample it stopped
        // at, a level that does not move.
        if !self.playing || self.len == 0 || self.source.is_none() {
            // The blocks go on whether a stream plays or not.
            self.to_block = (self.to_block + BLOCK - left.len() % BLOCK) % BLOCK;
            return;
        }
        let mut done = 0;
        while done < left.len() {
            if self.to_block == 0 {
                if self.filling {
                    self.feed();
                }
                self.to_block = BLOCK;
            }
            let n = self.to_block.min(left.len() - done);
            let [vl, vr] = &mut self.volume;
            for (l, r) in left[done..done + n]
                .iter_mut()
                .zip(&mut right[done..done + n])
            {
                let [sl, sr] = if self.held > 0.0 {
                    let s = self.ring[self.read as usize % RING];
                    if self.repeat == 0 {
                        self.read += self.step;
                        self.held -= self.step;
                    } else {
                        self.repeats_left = self.repeats_left.saturating_sub(1);
                        if self.repeats_left == 0 {
                            self.read += 1.0;
                            self.held -= 1.0;
                            self.repeats_left = self.repeat;
                        }
                    }
                    if self.read >= RING as f32 {
                        self.read -= RING as f32;
                    }
                    if self.held < 0.0 {
                        self.held = 0.0;
                    }
                    self.filling |= self.held < LOW;
                    s
                } else {
                    [0, 0]
                };
                *l += (f64::from(sl) * vl.value * SCALE) as f32;
                *r += (f64::from(sr) * vr.value * SCALE) as f32;
                vl.step();
                vr.step();
            }
            self.to_block -= n;
            done += n;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sb_core::value::{ElemType, Value};
    use std::cell::RefCell;
    use std::rc::Rc;

    /// PCMPOS read once a frame: the feed's jumps, round the array.
    fn jumps(rate: f64, len: usize, frames: usize) -> Vec<usize> {
        let a: ArrayRef = Rc::new(RefCell::new(Array::new(ElemType::Int, vec![len])));
        a.borrow_mut().set(0, Value::Int(1000)).unwrap();
        let mut pcm = Pcm::default();
        pcm.start(Source::Mono(a), rate);
        let (mut l, mut r) = (vec![0.0; 800], vec![0.0; 800]);
        let mut last = pcm.index();
        let mut out = Vec::new();
        for _ in 0..frames {
            pcm.render(&mut l, &mut r);
            let now = pcm.index();
            if now != last {
                out.push((now + len - last) % len);
                last = now;
            }
        }
        out
    }

    /// The feed takes 3073 samples at once, then tops the ring up to more
    /// than 3072 whenever fewer than 1024 are left at a block's start: 2160
    /// at a time at 48 kHz. At 32 kHz single precision makes one top-up in
    /// 16 a sample longer.
    #[test]
    fn the_feed_moves_in_jumps() {
        let j = jumps(48000.0, 48000, 120);
        assert_eq!(j[0], 3073);
        assert!(j[1..].iter().all(|&n| n == 2160), "{j:?}");
        let j = jumps(32000.0, 32000, 120);
        assert_eq!(&j[..3], [3073, 2080, 2080]);
        assert_eq!(j[16], 2081);
        let j = jumps(8000.0, 8000, 120);
        assert!(j[1..].iter().all(|&n| n == 2080), "{j:?}");
    }
}
