//! SmileBASIC 4's own instruments, as the importer stores them: a table of
//! instruments and key regions, and the waveforms still compressed, so
//! that they take little memory and are decoded only as notes play.
//!
//! The file is the emulator's own format, not a SmileBASIC format:
//!
//! ```text
//! "SBBANK01"
//! u32 instruments, then per instrument: u32 first region, u32 regions
//! u32 regions, then per region REGION_BYTES as in Region::write
//! u32 waves, then per wave WAVE_BYTES as in WaveInfo::write (a bank
//! from before stereo waves has 0 where the right channel goes, and reads
//! as all mono)
//! the waves' data
//! ```
//!
//! Numbers are little-endian.

use std::ops::Range;

use super::decoded::Cached;

const MAGIC: &[u8; 8] = b"SBBANK01";
const REGION_BYTES: usize = 12;
const WAVE_BYTES: usize = 64;
/// Samples in one DSP-ADPCM frame of eight bytes.
pub const FRAME_SAMPLES: u32 = 14;

/// The envelope of a note's level: SmileBASIC's attack, decay, sustain,
/// hold and release, each 0 to 127.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Adshr {
    pub attack: u8,
    pub decay: u8,
    pub sustain: u8,
    pub hold: u8,
    pub release: u8,
}

/// The waveform an instrument plays for a range of keys.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Region {
    /// The lowest and highest key, inclusive.
    pub keys: (u8, u8),
    pub wave: u16,
    /// The key the waveform sounds at when played unchanged.
    pub original_key: u8,
    pub volume: u8,
    pub pan: u8,
    pub envelope: Adshr,
    /// The note plays to its end however soon it is released, the same as
    /// a drum note.
    pub ignore_note_off: bool,
}

impl Region {
    fn write(&self, out: &mut Vec<u8>) {
        let e = self.envelope;
        out.extend_from_slice(&[self.keys.0, self.keys.1]);
        out.extend_from_slice(&self.wave.to_le_bytes());
        out.extend_from_slice(&[
            self.original_key,
            self.volume,
            self.pan,
            e.attack,
            e.decay,
            e.sustain,
            e.hold,
            e.release | u8::from(self.ignore_note_off) << 7,
        ]);
    }

    fn read(b: &[u8]) -> Region {
        Region {
            keys: (b[0], b[1]),
            wave: u16::from_le_bytes([b[2], b[3]]),
            original_key: b[4],
            volume: b[5],
            pan: b[6],
            envelope: Adshr {
                attack: b[7],
                decay: b[8],
                sustain: b[9],
                hold: b[10],
                release: b[11] & 0x7F,
            },
            ignore_note_off: b[11] & 0x80 != 0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Encoding {
    Pcm16,
    /// Nintendo's DSP-ADPCM: eight predictor pairs, and the two samples
    /// before the loop start, which decoding starts from each time the loop
    /// repeats.
    Adpcm {
        coefs: [i16; 16],
        loop_history: [i16; 2],
    },
}

/// One waveform, mono, or the left channel of a stereo one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WaveInfo {
    /// For the left channel of a stereo waveform, the wave that holds its
    /// right channel.
    pub right: Option<u16>,
    pub rate: u32,
    /// Samples in the waveform.
    pub len: u32,
    /// The loop start, for a waveform that loops to its end.
    pub loop_start: Option<u32>,
    pub encoding: Encoding,
    /// The bytes in the bank's data.
    pub data: Range<usize>,
}

impl WaveInfo {
    /// The number of bytes `len` samples take, if it fits in memory at all
    /// (on a 32-bit machine, usize can be too small for the `len` of a
    /// damaged file).
    pub fn bytes_for(encoding: &Encoding, len: u32) -> Option<usize> {
        match encoding {
            Encoding::Pcm16 => usize::try_from(len).ok()?.checked_mul(2),
            Encoding::Adpcm { .. } => usize::try_from(len.div_ceil(FRAME_SAMPLES))
                .ok()?
                .checked_mul(8),
        }
    }

    fn write(&self, out: &mut Vec<u8>) {
        let start = out.len();
        out.extend_from_slice(&self.rate.to_le_bytes());
        out.extend_from_slice(&self.len.to_le_bytes());
        out.extend_from_slice(&self.loop_start.unwrap_or(u32::MAX).to_le_bytes());
        out.extend_from_slice(&(self.data.start as u32).to_le_bytes());
        let (kind, coefs, history) = match self.encoding {
            Encoding::Pcm16 => (0u32, [0; 16], [0; 2]),
            Encoding::Adpcm {
                coefs,
                loop_history,
            } => (1, coefs, loop_history),
        };
        out.extend_from_slice(&kind.to_le_bytes());
        for c in coefs.iter().chain(&history) {
            out.extend_from_slice(&c.to_le_bytes());
        }
        let right = self.right.map_or(0, |r| u32::from(r) + 1);
        out.extend_from_slice(&right.to_le_bytes());
        out.resize(start + WAVE_BYTES, 0);
    }

    fn read(b: &[u8]) -> Option<WaveInfo> {
        let u = |i: usize| u32::from_le_bytes(b[i..i + 4].try_into().expect("four bytes"));
        let i16s = |from: usize, n: usize| {
            (0..n).map(move |k| i16::from_le_bytes([b[from + 2 * k], b[from + 2 * k + 1]]))
        };
        let encoding = match u(16) {
            0 => Encoding::Pcm16,
            1 => {
                let mut coefs = [0; 16];
                coefs.iter_mut().zip(i16s(20, 16)).for_each(|(c, v)| *c = v);
                let mut loop_history = [0; 2];
                loop_history
                    .iter_mut()
                    .zip(i16s(52, 2))
                    .for_each(|(c, v)| *c = v);
                Encoding::Adpcm {
                    coefs,
                    loop_history,
                }
            }
            _ => return None,
        };
        let (len, start) = (u(4), usize::try_from(u(12)).ok()?);
        let loop_start = Some(u(8)).filter(|&s| s != u32::MAX);
        if loop_start.is_some_and(|s| s >= len) {
            return None;
        }
        let end = start.checked_add(WaveInfo::bytes_for(&encoding, len)?)?;
        let right = match u(56) {
            0 => None,
            r => Some(u16::try_from(r - 1).ok()?),
        };
        Some(WaveInfo {
            right,
            rate: u(0),
            len,
            loop_start,
            data: start..end,
            encoding,
        })
    }
}

/// The instruments @0 upwards; an instrument with no regions is silent.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Bank {
    pub instruments: Vec<Range<usize>>,
    pub regions: Vec<Region>,
    pub waves: Vec<WaveInfo>,
    pub data: Vec<u8>,
}

impl Bank {
    /// The region instrument `program` plays `key` with.
    pub fn region(&self, program: i32, key: i32) -> Option<&Region> {
        let range = self.instruments.get(usize::try_from(program).ok()?)?;
        self.regions[range.clone()]
            .iter()
            .find(|r| (i32::from(r.keys.0)..=i32::from(r.keys.1)).contains(&key))
    }

    pub fn has_instrument(&self, program: i32) -> bool {
        usize::try_from(program)
            .ok()
            .and_then(|p| self.instruments.get(p))
            .is_some_and(|r| !r.is_empty())
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = MAGIC.to_vec();
        let count = |out: &mut Vec<u8>, n: usize| out.extend_from_slice(&(n as u32).to_le_bytes());
        count(&mut out, self.instruments.len());
        for r in &self.instruments {
            count(&mut out, r.start);
            count(&mut out, r.len());
        }
        count(&mut out, self.regions.len());
        for r in &self.regions {
            r.write(&mut out);
        }
        count(&mut out, self.waves.len());
        for w in &self.waves {
            w.write(&mut out);
        }
        out.extend_from_slice(&self.data);
        out
    }

    /// Reads a bank, checking that every reference stays inside it.
    pub fn from_bytes(bytes: &[u8]) -> Option<Bank> {
        if bytes.get(..MAGIC.len())? != MAGIC {
            return None;
        }
        let mut c = Cursor {
            bytes,
            at: MAGIC.len(),
        };
        let instruments = (0..c.count()?)
            .map(|_| {
                let (start, len) = (c.count()?, c.count()?);
                Some(start..start.checked_add(len)?)
            })
            .collect::<Option<Vec<_>>>()?;
        let regions = (0..c.count()?)
            .map(|_| c.take(REGION_BYTES).map(Region::read))
            .collect::<Option<Vec<_>>>()?;
        let waves = (0..c.count()?)
            .map(|_| c.take(WAVE_BYTES).and_then(WaveInfo::read))
            .collect::<Option<Vec<_>>>()?;
        let data = bytes[c.at..].to_vec();
        let fits = instruments.iter().all(|r| r.end <= regions.len())
            && regions
                .iter()
                .all(|r| usize::from(r.wave) < waves.len() && r.keys.0 <= r.keys.1)
            && waves.iter().all(|w| w.data.end <= data.len())
            && waves
                .iter()
                .filter_map(|w| w.right)
                .all(|r| usize::from(r) < waves.len());
        fits.then_some(Bank {
            instruments,
            regions,
            waves,
            data,
        })
    }
}

struct Cursor<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Cursor<'a> {
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let b = self.bytes.get(self.at..self.at.checked_add(n)?)?;
        self.at += n;
        Some(b)
    }

    fn count(&mut self) -> Option<usize> {
        self.take(4)
            .map(|b| u32::from_le_bytes(b.try_into().expect("four bytes")) as usize)
    }
}

/// Reads a waveform's samples one after another, going around its loop.
///
/// It decodes the rest of a DSP-ADPCM frame in one step, up to 14 samples,
/// and returns them from a small buffer: decoding one sample at a time took
/// about as long as all the rest of a voice's work. A 16-bit waveform goes
/// through the same buffer, so that taking a sample is the same few
/// instructions for both. A reader of a waveform kept decoded
/// ([`super::decoded`]) takes its samples from there instead.
#[derive(Clone, Debug, Default)]
pub struct Reader {
    /// The position of the next sample, which `ahead[next]` holds while
    /// `next < end`.
    pos: u32,
    /// The last two samples decoded, the latest first, which the next is
    /// predicted from.
    history: [i16; 2],
    /// Samples of the frame `pos` is in, decoded ahead: `next..end` are
    /// those still to come.
    ahead: [i16; FRAME_SAMPLES as usize],
    next: u8,
    end: u8,
    /// The waveform kept decoded, while this reader takes from there.
    cached: Option<Cached>,
}

impl Reader {
    /// The next sample, or `None` at the end of a waveform that does not
    /// loop.
    pub fn next(&mut self, wave: &WaveInfo, data: &[u8]) -> Option<i16> {
        self.next_of(wave, &data[wave.data.clone()])
    }

    /// [`Reader::next`] with the waveform's own bytes already found. It runs
    /// for each sample of each voice.
    #[inline(always)]
    pub fn next_of(&mut self, wave: &WaveInfo, data: &[u8]) -> Option<i16> {
        if self.next < self.end {
            let s = self.ahead[usize::from(self.next)];
            self.next += 1;
            self.pos += 1;
            return Some(s);
        }
        if let Some(c) = &mut self.cached
            && let Some(&s) = c.chunk.get(c.at)
        {
            c.at += 1;
            self.pos += 1;
            return Some(s);
        }
        self.decode(wave, data)
    }

    /// A reader that takes a kept waveform's samples.
    pub(super) fn from_cache(cached: Cached) -> Reader {
        Reader {
            cached: Some(cached),
            ..Reader::default()
        }
    }

    /// The samples already decoded that [`Reader::next_of`] returns next,
    /// for a loop that takes them itself, with [`Reader::skip`].
    #[inline(always)]
    pub(crate) fn ahead(&self) -> &[i16] {
        match &self.cached {
            Some(c) => c.chunk.get(c.at..).unwrap_or_default(),
            None => &self.ahead[usize::from(self.next)..usize::from(self.end.max(self.next))],
        }
    }

    /// Moves past `n` of the samples that [`Reader::ahead`] returns.
    #[inline(always)]
    pub(crate) fn skip(&mut self, n: usize) {
        debug_assert!(n <= self.ahead().len());
        match &mut self.cached {
            Some(c) => c.at += n,
            None => self.next += n as u8,
        }
        self.pos += n as u32;
    }

    /// [`Reader::next_of`] past the end of the chunk that the reader of a
    /// kept waveform is in: it takes the next chunk, goes around the loop,
    /// or continues decoding alone where the waveform is not kept.
    #[inline(never)]
    fn next_kept(&mut self, wave: &WaveInfo, data: &[u8]) -> Option<i16> {
        let c = self.cached.as_mut()?;
        if self.pos >= wave.len {
            let start = wave.loop_start?;
            if let Encoding::Adpcm { loop_history, .. } = wave.encoding
                && !c.loops_same(start as usize, loop_history)
            {
                // Later passes start from the loop's own history.
                self.cached = None;
                self.pos = start;
                self.history = loop_history;
                return self.decode(wave, data);
            }
            self.pos = start;
        }
        let pos = self.pos as usize;
        match c.chunk_at(pos, wave, data) {
            Some((chunk, at)) => {
                let s = chunk[at];
                (c.chunk, c.at) = (chunk, at + 1);
                self.pos += 1;
                Some(s)
            }
            None => {
                // Not kept: decoding continues from here, from the two
                // samples before, which are kept.
                let history = c.history_at(pos);
                self.cached = None;
                // Without them, decoding stopped before here at missing
                // bytes, where decoding alone stops too.
                self.history = history?;
                self.decode(wave, data)
            }
        }
    }

    /// Decodes from `pos` to the end of its frame, or of the waveform, and
    /// returns the first of those samples.
    #[inline(never)]
    fn decode(&mut self, wave: &WaveInfo, data: &[u8]) -> Option<i16> {
        if self.cached.is_some() {
            return self.next_kept(wave, data);
        }
        if self.pos >= wave.len {
            let start = wave.loop_start?;
            self.pos = start;
            if let Encoding::Adpcm { loop_history, .. } = wave.encoding {
                self.history = loop_history;
            }
        }
        let pos = self.pos as usize;
        let from = pos % FRAME_SAMPLES as usize;
        let frame = pos - from;
        let to = (FRAME_SAMPLES as usize).min(wave.len as usize - frame);
        let to = match &wave.encoding {
            Encoding::Pcm16 => {
                // Only the samples whose bytes are present. A wave that
                // passed the bank's checks always has all of them.
                let bytes = data.get(2 * frame..).unwrap_or_default();
                let to = to.min(bytes.len() / 2);
                for n in from..to {
                    self.ahead[n] = i16::from_le_bytes([bytes[2 * n], bytes[2 * n + 1]]);
                }
                to
            }
            Encoding::Adpcm { coefs, .. } => {
                let bytes = data
                    .get(frame / FRAME_SAMPLES as usize * 8..)
                    .unwrap_or_default();
                let bytes = &bytes[..bytes.len().min(8)];
                let to = to.min(bytes.len().saturating_sub(1) * 2);
                let whole = <[u8; 8]>::try_from(bytes).unwrap_or_else(|_| {
                    let mut short = [0u8; 8];
                    short[..bytes.len()].copy_from_slice(bytes);
                    short
                });
                let header = whole[0];
                let predictor = usize::from(header >> 4 & 7);
                let [c1, c2] = [coefs[2 * predictor], coefs[2 * predictor + 1]].map(i32::from);
                // The amount each nibble adds, which does not depend on the
                // samples before. It is computed first, separately from the
                // chain of predictions. The sums wrap, the same as unchecked
                // arithmetic (the coefficients of a damaged bank can
                // overflow; real waveforms stay far inside i32), so their
                // order does not matter.
                let mut add = [0i32; FRAME_SAMPLES as usize];
                for (pair, &byte) in add.chunks_exact_mut(2).zip(&whole[1..]) {
                    // Sign-extend each four bits.
                    let [high, low] = [byte as i8 >> 4, ((byte << 4) as i8) >> 4];
                    for (a, nibble) in pair.iter_mut().zip([high, low]) {
                        *a = ((i32::from(nibble) << (header & 15)) << 11).wrapping_add(1024);
                    }
                }
                let [mut h1, mut h2] = self.history.map(i32::from);
                let run = from..to.max(from);
                for (out, &a) in self.ahead[run.clone()].iter_mut().zip(&add[run]) {
                    let value = a
                        .wrapping_add(c1.wrapping_mul(h1))
                        .wrapping_add(c2.wrapping_mul(h2));
                    let s = (value >> 11).clamp(-32768, 32767) as i16;
                    *out = s;
                    (h1, h2) = (i32::from(s), h1);
                }
                self.history = [h1 as i16, h2 as i16];
                to
            }
        };
        if from >= to {
            // Bytes missing from a waveform the checks would refuse: it
            // ends here rather than read outside it.
            self.end = 0;
            return None;
        }
        // The buffer is indexed by the place in the frame.
        (self.next, self.end) = (from as u8 + 1, to as u8);
        self.pos += 1;
        Some(self.ahead[from])
    }

    /// The first version of [`Reader::next_of`], which decodes one sample
    /// at a time, to compare with.
    #[cfg(test)]
    fn next_by_sample(&mut self, wave: &WaveInfo, data: &[u8]) -> Option<i16> {
        if self.pos >= wave.len {
            let start = wave.loop_start?;
            self.pos = start;
            if let Encoding::Adpcm { loop_history, .. } = wave.encoding {
                self.history = loop_history;
            }
        }
        let pos = self.pos as usize;
        self.pos += 1;
        let sample = match &wave.encoding {
            Encoding::Pcm16 => i16::from_le_bytes([data[2 * pos], data[2 * pos + 1]]),
            Encoding::Adpcm { coefs, .. } => {
                let frame = pos / FRAME_SAMPLES as usize * 8;
                let n = pos % FRAME_SAMPLES as usize;
                let header = data[frame];
                let byte = data[frame + 1 + n / 2];
                let nibble = if n.is_multiple_of(2) {
                    byte >> 4
                } else {
                    byte & 15
                };
                let nibble = i32::from((nibble << 4) as i8 >> 4);
                let predictor = usize::from(header >> 4 & 7);
                let [c1, c2] = [coefs[2 * predictor], coefs[2 * predictor + 1]].map(i32::from);
                let [h1, h2] = self.history.map(i32::from);
                let value = ((nibble << (header & 15)) << 11)
                    .wrapping_add(1024)
                    .wrapping_add(c1.wrapping_mul(h1))
                    .wrapping_add(c2.wrapping_mul(h2));
                let s = (value >> 11).clamp(-32768, 32767) as i16;
                self.history = [s, self.history[0]];
                s
            }
        };
        Some(sample)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bank() -> Bank {
        let adpcm = Encoding::Adpcm {
            coefs: [0; 16],
            loop_history: [0; 2],
        };
        Bank {
            instruments: vec![0..2, 2..2],
            regions: vec![
                Region {
                    keys: (0, 59),
                    wave: 0,
                    original_key: 60,
                    volume: 127,
                    pan: 64,
                    envelope: Adshr {
                        attack: 127,
                        decay: 10,
                        sustain: 0,
                        hold: 0,
                        release: 100,
                    },
                    ignore_note_off: true,
                },
                Region {
                    keys: (60, 127),
                    wave: 1,
                    ..Region::default()
                },
            ],
            waves: vec![
                WaveInfo {
                    right: None,
                    rate: 32000,
                    len: 3,
                    loop_start: Some(1),
                    encoding: Encoding::Pcm16,
                    data: 0..6,
                },
                WaveInfo {
                    right: None,
                    rate: 44100,
                    len: 15,
                    loop_start: None,
                    encoding: adpcm,
                    data: 6..22,
                },
            ],
            data: (0..22).collect(),
        }
    }

    #[test]
    fn a_bank_reads_back() {
        let b = bank();
        assert_eq!(Bank::from_bytes(&b.to_bytes()), Some(b.clone()));
        assert_eq!(b.region(0, 59).unwrap().wave, 0);
        assert_eq!(b.region(0, 60).unwrap().wave, 1);
        assert!(b.region(1, 60).is_none() && !b.has_instrument(1));
        assert!(b.region(2, 60).is_none() && b.region(-1, 60).is_none());
        let mut bad = b.clone();
        bad.regions[0].wave = 2;
        assert_eq!(Bank::from_bytes(&bad.to_bytes()), None);
        // A stereo wave names its right channel, which must be there.
        let mut stereo = b.clone();
        stereo.waves[0].right = Some(1);
        assert_eq!(Bank::from_bytes(&stereo.to_bytes()), Some(stereo.clone()));
        stereo.waves[0].right = Some(2);
        assert_eq!(Bank::from_bytes(&stereo.to_bytes()), None);
        let bytes = b.to_bytes();
        assert_eq!(Bank::from_bytes(&bytes[..bytes.len() - 1]), None);
    }

    #[test]
    fn pcm_loops() {
        let b = bank();
        let w = &b.waves[0];
        let mut r = Reader::default();
        let got: Vec<i16> = (0..5).map(|_| r.next(w, &b.data).unwrap()).collect();
        let s = |lo: i16| lo | (lo + 1) << 8;
        assert_eq!(got, [s(0), s(2), s(4), s(2), s(4)]);
    }

    /// The most a predictor can ask for: both coefficients and both
    /// samples before at their extreme. It must neither panic in a build
    /// that checks overflow nor read outside the wave.
    #[test]
    fn adpcm_with_extreme_coefficients_does_not_overflow() {
        let w = WaveInfo {
            right: None,
            rate: 32000,
            len: 14,
            loop_start: None,
            encoding: Encoding::Adpcm {
                coefs: [i16::MIN; 16],
                loop_history: [0; 2],
            },
            data: 0..8,
        };
        // Scale 2^15 and the largest nibble, to bring the history to the
        // bottom and then keep it at extremes.
        let data = [0x0F, 0x88, 0x88, 0x88, 0x77, 0x77, 0x77, 0x77];
        let mut r = Reader::default();
        for _ in 0..14 {
            assert!(r.next(&w, &data).is_some());
        }
        assert_eq!(r.next(&w, &data), None);
    }

    /// The output the decoder gave before its sums wrapped stays the same
    /// in every frame of valid data, regardless of the build.
    #[test]
    fn adpcm_output_is_unchanged_inside_the_range() {
        let mut coefs = [0; 16];
        for (i, c) in coefs.iter_mut().enumerate() {
            *c = (i as i16 - 8) * 256;
        }
        let w = WaveInfo {
            right: None,
            rate: 32000,
            len: 14 * 64,
            loop_start: None,
            encoding: Encoding::Adpcm {
                coefs,
                loop_history: [0; 2],
            },
            data: 0..8 * 64,
        };
        let mut seed = 12345u32;
        let data: Vec<u8> = (0..8 * 64)
            .map(|i| {
                seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                let b = (seed >> 24) as u8;
                // Frame headers: a low scale, so no sum leaves i32.
                if i % 8 == 0 { b & 0x73 } else { b }
            })
            .collect();
        let (mut r, mut h) = (Reader::default(), [0i64; 2]);
        for pos in 0..14 * 64 {
            let frame = pos / 14 * 8;
            let n = pos % 14;
            let byte = data[frame + 1 + n / 2];
            let nibble = if n % 2 == 0 { byte >> 4 } else { byte & 15 };
            let nibble = i64::from((nibble << 4) as i8 >> 4);
            let header = data[frame];
            let p = usize::from(header >> 4 & 7);
            let value = ((nibble << (header & 15)) << 11)
                + 1024
                + i64::from(coefs[2 * p]) * h[0]
                + i64::from(coefs[2 * p + 1]) * h[1];
            let s = (value >> 11).clamp(-32768, 32767);
            h = [s, h[0]];
            assert_eq!(r.next(&w, &data), Some(s as i16), "sample {pos}");
        }
    }

    /// Decoding one frame at a time gives the same samples as decoding one
    /// sample at a time did, across frames, around loops that start
    /// anywhere in a frame, and at the end of waveforms that do not loop,
    /// of any length.
    #[test]
    fn frames_decode_as_samples_did() {
        let mut seed = 0x1234_5678u32;
        let mut next = move |n: u32| {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            seed % n
        };
        for round in 0..400 {
            let len = 1 + next(200);
            let loop_start = match next(4) {
                0 => None,
                1 => Some(0),
                2 => Some(len - 1),
                _ => Some(next(len)),
            };
            let encoding = if round % 4 == 3 {
                Encoding::Pcm16
            } else {
                let mut coefs = [0i16; 16];
                coefs.iter_mut().for_each(|c| *c = next(4096) as i16 - 1024);
                Encoding::Adpcm {
                    coefs,
                    loop_history: [next(2000) as i16 - 1000, next(2000) as i16 - 1000],
                }
            };
            let bytes = WaveInfo::bytes_for(&encoding, len).unwrap();
            let data: Vec<u8> = (0..bytes)
                .map(|i| {
                    let b = next(256) as u8;
                    // Frame headers: scales up to 2^11.
                    match (&encoding, i % 8) {
                        (Encoding::Adpcm { .. }, 0) => (b & 0x70) | ((b & 15) % 12),
                        _ => b,
                    }
                })
                .collect();
            let w = WaveInfo {
                right: None,
                rate: 32000,
                len,
                loop_start,
                encoding,
                data: 0..bytes,
            };
            let (mut block, mut plain) = (Reader::default(), Reader::default());
            for i in 0..3 * len + 30 {
                let got = block.next_of(&w, &data);
                assert_eq!(
                    got,
                    plain.next_by_sample(&w, &data),
                    "round {round} sample {i}"
                );
                if got.is_none() {
                    assert_eq!(block.next_of(&w, &data), None);
                    break;
                }
            }
        }
    }

    /// A waveform whose bytes stop short of its length, which the bank's
    /// checks refuse, ends where its bytes stop.
    #[test]
    fn missing_bytes_end_the_waveform() {
        let w = WaveInfo {
            right: None,
            rate: 32000,
            len: 28,
            loop_start: Some(0),
            encoding: Encoding::Adpcm {
                coefs: [0; 16],
                loop_history: [0; 2],
            },
            data: 0..16,
        };
        let data = [0x01; 11];
        let mut r = Reader::default();
        let got: Vec<_> = (0..30).map(|_| r.next_of(&w, &data)).collect();
        // Eleven bytes: a whole frame, and four samples of the next.
        assert!(got[..18].iter().all(Option::is_some));
        assert!(got[18..].iter().all(Option::is_none));
        let pcm = WaveInfo {
            encoding: Encoding::Pcm16,
            loop_start: None,
            ..w
        };
        let mut r = Reader::default();
        let got: Vec<_> = (0..8).map(|_| r.next_of(&pcm, &data)).collect();
        assert!(got[..5].iter().all(Option::is_some) && got[5..].iter().all(Option::is_none));
    }

    #[test]
    fn wave_sizes_stay_in_range() {
        // Past what fits, a wave is refused rather than wrapped around.
        let mut b = vec![0u8; WAVE_BYTES];
        b[4..8].copy_from_slice(&u32::MAX.to_le_bytes());
        b[8..12].copy_from_slice(&u32::MAX.to_le_bytes());
        b[12..16].copy_from_slice(&u32::MAX.to_le_bytes());
        let read = WaveInfo::read(&b);
        if usize::BITS == 64 {
            let end = read.unwrap().data.end as u64;
            assert_eq!(end, (1 << 32) - 1 + (1 << 33) - 2);
        } else {
            assert_eq!(read, None);
        }
        assert_eq!(WaveInfo::bytes_for(&Encoding::Pcm16, 3), Some(6),);
    }

    #[test]
    fn adpcm_decodes_and_ends() {
        // Predictor 0 with coefficients 2048 and 0 repeats the last
        // sample and adds the nibble times the scale.
        let mut coefs = [0; 16];
        coefs[0] = 2048;
        let w = WaveInfo {
            right: None,
            rate: 32000,
            len: 4,
            loop_start: None,
            encoding: Encoding::Adpcm {
                coefs,
                loop_history: [0; 2],
            },
            data: 0..8,
        };
        // Scale 2^2; nibbles 1, 1, -1, 7.
        let data = [0x02, 0x11, 0xF7, 0, 0, 0, 0, 0];
        let mut r = Reader::default();
        let got: Vec<Option<i16>> = (0..5).map(|_| r.next(&w, &data)).collect();
        assert_eq!(got, [Some(4), Some(8), Some(4), Some(32), None]);
    }
}
