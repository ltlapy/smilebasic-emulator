//! SmileBASIC 4's own instruments, as the importer stores them: a table of
//! instruments and key regions, and the waveforms still compressed, so
//! that they take little memory and are decoded only as notes play.
//!
//! The file is the emulator's own format, not SmileBASIC's:
//!
//! ```text
//! "SBBANK01"
//! u32 instruments, then per instrument: u32 first region, u32 regions
//! u32 regions, then per region REGION_BYTES as in Region::write
//! u32 waves, then per wave WAVE_BYTES as in WaveInfo::write
//! the waves' data
//! ```
//!
//! Numbers are little-endian.

use std::ops::Range;

const MAGIC: &[u8; 8] = b"SBBANK01";
const REGION_BYTES: usize = 12;
const WAVE_BYTES: usize = 64;
/// Samples in one DSP-ADPCM frame of eight bytes.
pub const FRAME_SAMPLES: u32 = 14;

/// How a note's level moves: SmileBASIC's attack, decay, sustain, hold and
/// release, each 0 to 127.
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
    /// The key the waveform sounds at as it is.
    pub original_key: u8,
    pub volume: u8,
    pub pan: u8,
    pub envelope: Adshr,
    /// The note plays to its end however soon it is let go, as drums do.
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
    /// before the loop start, which decoding the loop again starts from.
    Adpcm {
        coefs: [i16; 16],
        loop_history: [i16; 2],
    },
}

/// One waveform, mono.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WaveInfo {
    pub rate: u32,
    /// Samples in the waveform.
    pub len: u32,
    /// Where the loop starts, for a waveform that loops to its end.
    pub loop_start: Option<u32>,
    pub encoding: Encoding,
    /// The bytes in the bank's data.
    pub data: Range<usize>,
}

impl WaveInfo {
    /// Bytes `len` samples take, if that fits in memory at all (a 32-bit
    /// machine's usize can be too small for a damaged file's `len`).
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
        Some(WaveInfo {
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
            && waves.iter().all(|w| w.data.end <= data.len());
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

/// Reads a waveform's samples one after another, going round its loop.
#[derive(Clone, Debug, Default)]
pub struct Reader {
    pos: u32,
    history: [i16; 2],
}

impl Reader {
    /// The next sample, or `None` at the end of a waveform that does not
    /// loop.
    pub fn next(&mut self, wave: &WaveInfo, data: &[u8]) -> Option<i16> {
        if self.pos >= wave.len {
            let start = wave.loop_start?;
            self.pos = start;
            if let Encoding::Adpcm { loop_history, .. } = wave.encoding {
                self.history = loop_history;
            }
        }
        let data = &data[wave.data.clone()];
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
                // Sign-extend the four bits.
                let nibble = i32::from((nibble << 4) as i8 >> 4);
                let predictor = usize::from(header >> 4 & 7);
                let [c1, c2] = [coefs[2 * predictor], coefs[2 * predictor + 1]].map(i32::from);
                let [h1, h2] = self.history.map(i32::from);
                // A damaged bank's coefficients can overflow: it wraps, as
                // it would unchecked. Real waveforms stay far inside i32.
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
                    rate: 32000,
                    len: 3,
                    loop_start: Some(1),
                    encoding: Encoding::Pcm16,
                    data: 0..6,
                },
                WaveInfo {
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

    /// What the decoder's sum was before it wrapped: the same in every
    /// frame of valid data, whatever the build.
    #[test]
    fn adpcm_output_is_unchanged_inside_the_range() {
        let mut coefs = [0; 16];
        for (i, c) in coefs.iter_mut().enumerate() {
            *c = (i as i16 - 8) * 256;
        }
        let w = WaveInfo {
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

    #[test]
    fn wave_sizes_stay_in_range() {
        // Past what fits, a wave is refused rather than wrapped round.
        let mut b = vec![0u8; WAVE_BYTES];
        b[4..8].copy_from_slice(&u32::MAX.to_le_bytes());
        b[8..12].copy_from_slice(&u32::MAX.to_le_bytes());
        b[12..16].copy_from_slice(&u32::MAX.to_le_bytes());
        let read = WaveInfo::read(&b);
        if usize::BITS == 64 {
            assert_eq!(read.unwrap().data.end, (1 << 32) - 1 + (1 << 33) - 2);
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
