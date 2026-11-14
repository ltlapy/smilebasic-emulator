//! Writes the imported instruments as a SoundFont 2 file, for playing them
//! elsewhere. The waveforms are decoded to 16-bit samples, so the file is
//! several times the size of the bank.
//!
//! Instruments become presets: @0-@127 in bank 0, the drum sets @128-@148
//! in bank 128 (programs 0-20), and the rest in bank @/128, program @%128.
//! With [`Layout::Gm`] @0-@127 and the drum sets follow General MIDI and
//! Roland GS: bank 0, and bank 128 at GS's kit programs. The rest, which
//! GM has no sound for, keep the numbers above.

use std::io::{self, BufWriter, Write};

use sb_runtime::sound::bank::{Bank, Reader};
use sb_runtime::sound::sampler::times;
use sb_runtime::sound::synth::DRUM_KITS;

/// Zero samples after each waveform, as the format asks.
const GAP: usize = 46;

/// The preset bank and program for MML's instrument `program`.
pub fn preset(program: usize) -> (u16, u16) {
    match program {
        128..=148 => (128, program as u16 - 128),
        _ => ((program / 128) as u16, (program % 128) as u16),
    }
}

/// How instruments are numbered.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Layout {
    /// SmileBASIC 4's own numbers, see [`preset`].
    Own,
    /// General MIDI and Roland GS where they have the sound.
    Gm,
}

/// The GS kit program of each drum set @128-@148, every one its own so
/// that none hides another. A set takes the GS kit nearest by name
/// ([`DRUM_KITS`]) if no earlier set has it, else the next program up.
pub fn gm_drum_programs() -> [u8; 21] {
    let mut taken = [false; 128];
    DRUM_KITS.map(|kit| {
        let program = (kit as usize..128).find(|&p| !taken[p]).unwrap_or(0);
        taken[program] = true;
        program as u8
    })
}

impl Layout {
    /// The preset bank and program for MML's instrument `program`.
    pub fn preset(self, program: usize) -> (u16, u16) {
        match (self, program) {
            (Layout::Gm, 0..=127) => (0, program as u16),
            (Layout::Gm, 128..=148) => (128, gm_drum_programs()[program - 128].into()),
            (_, _) => preset(program),
        }
    }
}

/// A 20-byte name.
fn name(text: &str) -> [u8; 20] {
    let mut out = [0u8; 20];
    let bytes = text.as_bytes();
    out[..bytes.len().min(19)].copy_from_slice(&bytes[..bytes.len().min(19)]);
    out
}

/// A chunk, padded inside to an even size: SoundFont strings are even,
/// and some readers do not skip RIFF's padding byte.
fn chunk(id: &[u8; 4], body: &[u8]) -> Vec<u8> {
    let size = body.len().next_multiple_of(2);
    let mut out = id.to_vec();
    out.extend_from_slice(&(size as u32).to_le_bytes());
    out.extend_from_slice(body);
    out.resize(8 + size, 0);
    out
}

fn list(kind: &[u8; 4], chunks: &[Vec<u8>]) -> Vec<u8> {
    let mut body = kind.to_vec();
    chunks.iter().for_each(|c| body.extend_from_slice(c));
    chunk(b"LIST", &body)
}

/// Seconds as timecents, the format's unit for times.
fn timecents(seconds: f64) -> i16 {
    if seconds <= 0.001 {
        return -12000;
    }
    (1200.0 * seconds.log2()).round().clamp(-12000.0, 8000.0) as i16
}

/// Generators: an operator and its amount.
#[derive(Default)]
struct Gens(Vec<u8>);

impl Gens {
    fn add(&mut self, operator: u16, amount: i16) {
        self.0.extend_from_slice(&operator.to_le_bytes());
        self.0.extend_from_slice(&amount.to_le_bytes());
    }

    fn count(&self) -> u16 {
        (self.0.len() / 4) as u16
    }
}

fn bag(out: &mut Vec<u8>, generators: u16) {
    out.extend_from_slice(&generators.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
}

/// Writes the SoundFont to `out`, decoding the waveforms as it goes so
/// that they are never all in memory.
pub fn write(bank: &Bank, layout: Layout, out: &mut impl Write) -> io::Result<()> {
    // Where each waveform starts among the samples.
    let mut shdr = Vec::new();
    let mut start = 0u32;
    for (i, wave) in bank.waves.iter().enumerate() {
        let end = start + wave.len;
        let (loop_start, loop_end) = match wave.loop_start {
            Some(l) => (start + l, end),
            None => (start, end),
        };
        shdr.extend_from_slice(&name(&format!("wave {i}")));
        for v in [start, end, loop_start, loop_end, wave.rate] {
            shdr.extend_from_slice(&v.to_le_bytes());
        }
        // Root key (overridden by each zone), correction, link, mono.
        shdr.extend_from_slice(&[60, 0, 0, 0, 1, 0]);
        start = end + GAP as u32;
    }
    let sample_bytes = start * 2;
    shdr.extend_from_slice(&name("EOS"));
    shdr.resize(shdr.len() + 26, 0);

    let (mut phdr, mut pbag, mut pgen) = (Vec::new(), Vec::new(), Gens::default());
    let (mut inst, mut ibag, mut igen) = (Vec::new(), Vec::new(), Gens::default());
    let mut presets = 0u16;
    for (program, range) in bank.instruments.iter().enumerate() {
        if range.is_empty() {
            continue;
        }
        let (b, p) = layout.preset(program);
        let instrument = (inst.len() / 22) as i16;
        inst.extend_from_slice(&name(&format!("@{program}")));
        inst.extend_from_slice(&((ibag.len() / 4) as u16).to_le_bytes());
        for region in &bank.regions[range.clone()] {
            bag(&mut ibag, igen.count());
            let t = times(region.envelope);
            let volume = f64::from(region.volume.max(1)) / 127.0;
            igen.add(43, i16::from_le_bytes([region.keys.0, region.keys.1]));
            igen.add(34, timecents(t.attack));
            igen.add(35, timecents(t.hold));
            igen.add(36, timecents(t.decay));
            igen.add(37, (t.sustain_db * 10.0).round().min(1440.0) as i16);
            // A note that ignores being let go keeps decaying.
            let release = if region.ignore_note_off {
                t.decay
            } else {
                t.release
            };
            igen.add(38, timecents(release));
            igen.add(
                17,
                ((i16::from(region.pan) - 64) * 500 / 64).clamp(-500, 500),
            );
            // A region's volume is a linear gain.
            igen.add(48, (-200.0 * volume.log10()).round() as i16);
            let looped = bank.waves[usize::from(region.wave)].loop_start.is_some();
            igen.add(54, i16::from(looped));
            igen.add(58, i16::from(region.original_key));
            igen.add(53, region.wave as i16);
        }
        phdr.extend_from_slice(&name(&format!("SmileBASIC 4 @{program}")));
        phdr.extend_from_slice(&p.to_le_bytes());
        phdr.extend_from_slice(&b.to_le_bytes());
        phdr.extend_from_slice(&presets.to_le_bytes());
        phdr.resize(phdr.len() + 12, 0);
        bag(&mut pbag, pgen.count());
        pgen.add(41, instrument);
        presets += 1;
    }
    phdr.extend_from_slice(&name("EOP"));
    phdr.extend_from_slice(&[0; 4]);
    phdr.extend_from_slice(&presets.to_le_bytes());
    phdr.resize(phdr.len() + 12, 0);
    bag(&mut pbag, pgen.count());
    pgen.add(0, 0);
    inst.extend_from_slice(&name("EOI"));
    inst.extend_from_slice(&((ibag.len() / 4) as u16).to_le_bytes());
    bag(&mut ibag, igen.count());
    igen.add(0, 0);

    let info = list(
        b"INFO",
        &[
            chunk(b"ifil", &[2, 0, 1, 0]),
            chunk(b"isng", b"EMU8000\0"),
            chunk(b"INAM", b"SmileBASIC 4 instruments\0"),
            chunk(
                b"ICMT",
                b"Converted from the user's own copy by smilebasic-import\0",
            ),
        ],
    );
    let pdta = list(
        b"pdta",
        &[
            chunk(b"phdr", &phdr),
            chunk(b"pbag", &pbag),
            chunk(b"pmod", &[0; 10]),
            chunk(b"pgen", &pgen.0),
            chunk(b"inst", &inst),
            chunk(b"ibag", &ibag),
            chunk(b"imod", &[0; 10]),
            chunk(b"igen", &igen.0),
            chunk(b"shdr", &shdr),
        ],
    );
    // The sample chunk's list: its kind, then the chunk's header.
    let sdta_size = 4 + 8 + sample_bytes;
    let riff_size = 4 + info.len() as u32 + 8 + sdta_size + pdta.len() as u32;
    let mut out = BufWriter::new(out);
    out.write_all(b"RIFF")?;
    out.write_all(&riff_size.to_le_bytes())?;
    out.write_all(b"sfbk")?;
    out.write_all(&info)?;
    out.write_all(b"LIST")?;
    out.write_all(&sdta_size.to_le_bytes())?;
    out.write_all(b"sdtasmpl")?;
    out.write_all(&sample_bytes.to_le_bytes())?;
    for wave in &bank.waves {
        let mut reader = Reader::default();
        for _ in 0..wave.len {
            let s = reader.next(wave, &bank.data).unwrap_or(0);
            out.write_all(&s.to_le_bytes())?;
        }
        out.write_all(&[0; GAP * 2])?;
    }
    out.write_all(&pdta)?;
    out.flush()
}

#[cfg(test)]
mod tests {
    use super::*;
    use sb_runtime::sound::SoundFont;
    use sb_runtime::sound::bank::{Adshr, Encoding, Region, WaveInfo};

    #[test]
    fn rustysynth_reads_it() {
        let samples: Vec<u8> = (0..100i16).flat_map(|s| (s * 300).to_le_bytes()).collect();
        let region = Region {
            keys: (0, 127),
            wave: 0,
            original_key: 60,
            volume: 100,
            pan: 64,
            envelope: Adshr {
                attack: 127,
                decay: 100,
                sustain: 90,
                hold: 0,
                release: 110,
            },
            ignore_note_off: false,
        };
        let bank = Bank {
            instruments: vec![0..1, 1..1, 1..2],
            regions: vec![region, region],
            waves: vec![WaveInfo {
                right: None,
                rate: 32000,
                len: 100,
                loop_start: Some(50),
                encoding: Encoding::Pcm16,
                data: 0..200,
            }],
            data: samples,
        };
        let mut bytes = Vec::new();
        write(&bank, Layout::Own, &mut bytes).unwrap();
        let font = SoundFont::new(&mut std::io::Cursor::new(bytes)).unwrap();
        let presets: Vec<_> = font
            .get_presets()
            .iter()
            .map(|p| (p.get_bank_number(), p.get_patch_number()))
            .collect();
        assert_eq!(presets, [(0, 0), (0, 2)]);
        assert_eq!(font.get_sample_headers().len(), 1);
        assert_eq!(preset(140), (128, 12));
        assert_eq!(preset(300), (2, 44));
    }

    #[test]
    fn gm_layout_keeps_to_gm() {
        assert_eq!(Layout::Gm.preset(5), (0, 5));
        assert_eq!(Layout::Gm.preset(128), (128, 0));
        assert_eq!(Layout::Gm.preset(129), (128, 24));
        // GM has no sound for these, so they keep their own numbers.
        assert_eq!(Layout::Gm.preset(300), (2, 44));
        assert_eq!(Layout::Own.preset(129), (128, 1));
        // No two drum sets share a program.
        let mut kits = gm_drum_programs().to_vec();
        kits.sort_unstable();
        kits.dedup();
        assert_eq!(kits.len(), 21);
    }
}
