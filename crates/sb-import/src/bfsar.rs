//! SmileBASIC 4's sound archive, `petc4.bfsar` in the RomFS: Nintendo's
//! FSAR, holding sequences, one bank of instruments (FBNK) and one archive
//! of waveforms (FWAR). This turns the bank and its waveforms into the
//! emulator's own [`Bank`], keeping the waveforms compressed.

use sb_runtime::sound::bank::{Adshr, Bank, Encoding, Region, WaveInfo};
use sb_runtime::sound::seq::{Kind, Sequence, Sequences};

type Result<T> = std::result::Result<T, String>;

/// Ranges of keys an instrument's table may have: there are 128 keys.
const KEY_RANGES: usize = 128;
/// The most regions in the whole bank, because many instruments sharing
/// one table could otherwise multiply them. SmileBASIC 4's bank has about
/// 2,500.
const REGIONS: usize = 1 << 16;
/// Songs and effects of each kind: the numbers the file format can store.
const SEQUENCE_NUMBERS: usize = 1 << 16;
/// The most sequence code, as a multiple of the file's own size, because
/// many sounds may name one big file and each sound keeps a copy of its
/// file. SmileBASIC 4's sequences take less than the file.
const SEQUENCE_CODE_TIMES_FILE: usize = 2;

const DAMAGED: &str = "damaged: an offset points outside the file";

/// The FSAR block of sound names.
const FSAR_STRINGS: u16 = 0x2000;
/// The FSAR block of tables: sounds, files and more.
const FSAR_INFO: u16 = 0x2001;
/// The FSAR block that holds the files inside the archive.
const FSAR_FILES: u16 = 0x2002;
/// The FSEQ block of sequence code.
const FSEQ_CODE: u16 = 0x5000;
/// The FBNK block of waves and instruments.
const FBNK_INFO: u16 = 0x5800;
/// The FWAR block of wave positions.
const FWAR_INFO: u16 = 0x6800;
/// The FWAR block that holds the FWAV files.
const FWAR_FILES: u16 = 0x6801;
/// The FWAV block of the format and channels.
const FWAV_INFO: u16 = 0x7000;
/// The FWAV block of samples.
const FWAV_DATA: u16 = 0x7001;

/// The index of the file table among the FSAR info block's references.
const FILE_TABLE_REFERENCE: usize = 6;
/// The reference type of a file inside the archive.
const FILE_IN_ARCHIVE: u16 = 0x220C;
/// The reference type of a sound's sequence details.
const SEQUENCE_INFO: u16 = 0x2203;
/// The offset of a sound's flags.
const SOUND_FLAGS: usize = 20;
/// The flag of a sound whose first value after the flags is its name.
const SOUND_HAS_NAME: u32 = 1;

/// Offsets that read past the end report the file as damaged.
#[derive(Clone, Copy)]
struct Bytes<'a>(&'a [u8]);

impl<'a> Bytes<'a> {
    fn slice(self, at: usize, len: usize) -> Result<&'a [u8]> {
        at.checked_add(len)
            .and_then(|end| self.0.get(at..end))
            .ok_or_else(|| DAMAGED.to_string())
    }

    /// `base + offset`, if that is inside the file. Every place in the
    /// file passes through here or `slice` first, so a field or entry
    /// offset added to one afterwards cannot overflow, even where a usize
    /// has 32 bits.
    fn at(self, base: usize, offset: usize) -> Result<usize> {
        base.checked_add(offset)
            .filter(|&at| at <= self.0.len())
            .ok_or_else(|| DAMAGED.to_string())
    }

    fn u8(self, at: usize) -> Result<u8> {
        Ok(self.slice(at, 1)?[0])
    }

    fn u16(self, at: usize) -> Result<u16> {
        Ok(u16::from_le_bytes(
            self.slice(at, 2)?.try_into().expect("two bytes"),
        ))
    }

    fn i16(self, at: usize) -> Result<i16> {
        Ok(self.u16(at)? as i16)
    }

    fn u32(self, at: usize) -> Result<u32> {
        Ok(u32::from_le_bytes(
            self.slice(at, 4)?.try_into().expect("four bytes"),
        ))
    }

    fn offset(self, at: usize) -> Result<usize> {
        Ok(self.u32(at)? as usize)
    }

    /// A reference at `at`: its type, and its target counted from `base`,
    /// or `None` for a null reference.
    fn reference(self, at: usize, base: usize) -> Result<(u16, Option<usize>)> {
        let kind = self.u16(at)?;
        let offset = self.u32(at + 4)?;
        if offset == u32::MAX {
            return Ok((kind, None));
        }
        let offset = usize::try_from(offset).map_err(|_| DAMAGED)?;
        Ok((kind, Some(self.at(base, offset)?)))
    }

    /// The body of the block of type `kind` in a Nintendo file whose
    /// header is at `at` and has the magic `magic`.
    fn block(self, at: usize, magic: &[u8; 4], kind: u16) -> Result<usize> {
        if self.slice(at, 4)? != magic {
            return Err(format!("not an {}", String::from_utf8_lossy(magic)));
        }
        for i in 0..usize::from(self.u16(at + 0x10)?) {
            let entry = at + 0x14 + i * 12;
            if self.u16(entry)? == kind {
                return self.at(self.at(at, self.offset(entry + 4)?)?, 8);
            }
        }
        Err(format!(
            "{} has no block {kind:#06x}",
            String::from_utf8_lossy(magic)
        ))
    }
}

/// The offset of each of the archive's files, by file number; `None` for
/// a file kept outside it.
fn files(d: Bytes) -> Result<Vec<Option<usize>>> {
    let info = d.block(0, b"FSAR", FSAR_INFO)?;
    let data = d.block(0, b"FSAR", FSAR_FILES)?;
    let (_, table) = d.reference(info + FILE_TABLE_REFERENCE * 8, info)?;
    let table = table.ok_or("no file table")?;
    let mut files = Vec::new();
    for i in 0..d.offset(table)? {
        let (_, entry) = d.reference(table + 4 + i * 8, table)?;
        let mut at = None;
        if let Some(entry) = entry {
            let (kind, location) = d.reference(entry, entry)?;
            if kind == FILE_IN_ARCHIVE {
                at = d.reference(location.ok_or("no file location")?, data)?.1;
            }
        }
        files.push(at);
    }
    Ok(files)
}

fn find(d: Bytes, files: &[Option<usize>], magic: &[u8; 4]) -> Result<usize> {
    for &at in files.iter().flatten() {
        if d.slice(at, 4)? == magic {
            return Ok(at);
        }
    }
    Err(format!("no {} inside", String::from_utf8_lossy(magic)))
}

/// The archive's sound names, by string number.
fn names(d: Bytes) -> Result<Vec<String>> {
    let strings = d.block(0, b"FSAR", FSAR_STRINGS)?;
    let (_, table) = d.reference(strings, strings)?;
    let table = table.ok_or("no string table")?;
    // Each name costs a copy, so many names pointing at the same big text
    // must not multiply it. Real names do not overlap, so they fit in the
    // file.
    let mut total = 0usize;
    (0..d.offset(table)?)
        .map(|i| {
            let entry = table + 4 + i * 12;
            let text = d.slice(d.at(table, d.offset(entry + 4)?)?, d.offset(entry + 8)?)?;
            total = total.saturating_add(text.len());
            if total > d.0.len() {
                return Err("damaged: the names overlap".to_string());
            }
            Ok(String::from_utf8_lossy(text.strip_suffix(&[0]).unwrap_or(text)).into_owned())
        })
        .collect()
}

/// The preset songs (sounds `Bnn`) and BEEP's effects (`Snnn`), which are
/// sequences.
pub fn sequences(archive: &[u8]) -> Result<Sequences> {
    let d = Bytes(archive);
    let files = files(d)?;
    let names = names(d)?;
    let info = d.block(0, b"FSAR", FSAR_INFO)?;
    let (_, sounds) = d.reference(info, info)?;
    let sounds = sounds.ok_or("no sound table")?;
    let mut out = Sequences::default();
    let mut code_bytes = 0usize;
    for i in 0..d.offset(sounds)? {
        let (_, sound) = d.reference(sounds + 4 + i * 8, sounds)?;
        let Some(sound) = sound else { continue };
        let flags = sound + SOUND_FLAGS;
        if d.u32(flags)? & SOUND_HAS_NAME == 0 {
            continue;
        }
        let Some(name) = names.get(d.offset(flags + 4)?) else {
            continue;
        };
        let (kind, number) = match name.split_at(1) {
            ("B", n) => (Kind::Song, n),
            ("S", n) => (Kind::Beep, n),
            _ => continue,
        };
        let Ok(number) = number.parse::<usize>() else {
            continue;
        };
        if number >= SEQUENCE_NUMBERS {
            return Err(format!("{name}'s number is too large"));
        }
        let (detail_kind, detail) = d.reference(sound + 12, sound)?;
        let (SEQUENCE_INFO, Some(detail)) = (detail_kind, detail) else {
            continue;
        };
        let file = files
            .get(d.offset(sound)?)
            .copied()
            .flatten()
            .ok_or_else(|| format!("{name}'s sequence is missing"))?;
        let code = d.block(file, b"FSEQ", FSEQ_CODE)?;
        let len = d
            .offset(code - 4)?
            .checked_sub(8)
            .ok_or("a damaged sequence")?;
        code_bytes = code_bytes.saturating_add(len);
        if code_bytes > d.0.len().saturating_mul(SEQUENCE_CODE_TIMES_FILE) {
            return Err("too many sounds name the same sequences".to_string());
        }
        let code = d.slice(code, len)?.to_vec();
        let start = d.offset(detail + 16)?;
        if start >= code.len() {
            return Err(format!("{name} starts outside its sequence"));
        }
        let volume = d.u8(sound + 8)?;
        out.insert(
            kind,
            number,
            Sequence {
                volume,
                start,
                code,
            },
        );
    }
    Ok(out)
}

/// Reads the sound archive's instruments and waveforms.
pub fn bank(archive: &[u8]) -> Result<Bank> {
    let d = Bytes(archive);
    let files = files(d)?;
    let fbnk = find(d, &files, b"FBNK")?;
    let fwar = find(d, &files, b"FWAR")?;

    let info = d.block(fbnk, b"FBNK", FBNK_INFO)?;
    let (_, wave_table) = d.reference(info, info)?;
    let (_, instrument_table) = d.reference(info + 8, info)?;
    let (wave_table, instrument_table) = (
        wave_table.ok_or("no wave table")?,
        instrument_table.ok_or("no instruments")?,
    );

    let mut bank = Bank::default();
    let archive_waves = war_waves(d, fwar)?;
    let add = |bank: &mut Bank, info: &WaveInfo, bytes: &[u8]| {
        let start = bank.data.len();
        // Waves that share or overlap their bytes would be copied again
        // each time, and a small table could then make many times the
        // file's size. Real waves are separate, so they fit in the file.
        if bytes.len() > archive.len() - start {
            return Err("damaged: the waveforms overlap".to_string());
        }
        bank.data.extend_from_slice(bytes);
        bank.waves.push(WaveInfo {
            data: start..bank.data.len(),
            ..info.clone()
        });
        Ok(())
    };
    // The bank names waves by archive and index; SmileBASIC 4 has one
    // archive. The right channels of stereo waves go after them all.
    let mut rights = Vec::new();
    for i in 0..d.offset(wave_table)? {
        let index = d.offset(wave_table + 4 + i * 8 + 4)?;
        let (info, bytes, right) = archive_waves
            .get(index)
            .ok_or("an instrument's waveform is missing")?;
        if let Some(right) = right {
            rights.push((bank.waves.len(), right));
        }
        add(&mut bank, info, bytes)?;
    }
    for (left, (info, bytes)) in rights {
        let at = u16::try_from(bank.waves.len()).map_err(|_| "too many waveforms")?;
        bank.waves[left].right = Some(at);
        add(&mut bank, info, bytes)?;
    }

    for i in 0..d.offset(instrument_table)? {
        let start = bank.regions.len();
        let (kind, at) = d.reference(instrument_table + 4 + i * 8, instrument_table)?;
        if let (0x5900, Some(at)) = (kind, at) {
            let (kind, keys) = d.reference(at, at)?;
            if let Some(keys) = keys {
                key_regions(d, kind, keys, &mut bank.regions)?;
                if bank.regions.len() > REGIONS {
                    return Err("too many key regions".to_string());
                }
            }
        }
        bank.instruments.push(start..bank.regions.len());
    }
    if bank
        .regions
        .iter()
        .any(|r| usize::from(r.wave) >= bank.waves.len())
    {
        return Err("an instrument names a waveform that is not there".into());
    }
    Ok(bank)
}

/// An instrument's regions, from its table of key ranges.
fn key_regions(d: Bytes, kind: u16, at: usize, out: &mut Vec<Region>) -> Result<()> {
    match kind {
        // One region for every key.
        0x6000 => {
            let (_, region) = d.reference(at, at)?;
            if let Some(r) = region {
                out.push(key_region(d, r, (0, 127))?);
            }
        }
        // Ranges, by their highest keys.
        0x6001 => {
            let n = d.offset(at)?;
            if n > KEY_RANGES {
                return Err(format!("{n} key ranges in an instrument"));
            }
            let refs = at + 4 + n.next_multiple_of(4);
            let mut low = 0u8;
            for i in 0..n {
                let high = d.u8(at + 4 + i)?;
                let (_, region) = d.reference(refs + i * 8, at)?;
                if let Some(r) = region {
                    out.push(key_region(d, r, (low, high))?);
                }
                low = high.saturating_add(1);
            }
        }
        // A region for each key, as in drum sets; neighboring keys that
        // share a region become one range.
        0x6002 => {
            let (min, max) = (d.u8(at)?, d.u8(at + 1)?);
            let mut last: Option<(usize, u8)> = None;
            for key in min..=max {
                let (kind, region) = d.reference(at + 4 + usize::from(key - min) * 8, at)?;
                let region = region.filter(|_| kind == 0x5901);
                match (region, last) {
                    (Some(r), Some((lr, _))) if r == lr => {
                        out.last_mut().expect("a region").keys.1 = key;
                    }
                    (Some(r), _) => out.push(key_region(d, r, (key, key))?),
                    (None, _) => {}
                }
                last = region.map(|r| (r, key));
            }
        }
        _ => return Err(format!("unknown key table {kind:#06x}")),
    }
    Ok(())
}

/// A key region: a table of velocity ranges, of which SmileBASIC 4 always
/// has one.
fn key_region(d: Bytes, at: usize, keys: (u8, u8)) -> Result<Region> {
    let (kind, velocities) = d.reference(at, at)?;
    let velocities = velocities.ok_or("empty key region")?;
    let region = match kind {
        0x6000 => d.reference(velocities, velocities)?.1,
        // Ranges: take the first, because the loudest notes are rare.
        0x6001 => {
            let n = d.offset(velocities)?;
            let refs = n
                .checked_next_multiple_of(4)
                .ok_or(DAMAGED)
                .and_then(|n| d.at(velocities + 4, n).map_err(|_| DAMAGED))?;
            d.reference(refs, velocities)?.1
        }
        _ => return Err(format!("unknown velocity table {kind:#06x}")),
    };
    velocity_region(d, region.ok_or("empty velocity region")?, keys)
}

/// The flag of a velocity region that has an original key.
const REGION_ORIGINAL_KEY: usize = 0;
/// The flag of a velocity region that has a volume.
const REGION_VOLUME: usize = 1;
/// The flag of a velocity region that has a pan.
const REGION_PAN: usize = 2;
/// The flag of a velocity region that may ignore note-off.
const REGION_IGNORE_NOTE_OFF: usize = 4;
/// The flag of a velocity region that has an envelope.
const REGION_ENVELOPE: usize = 9;

fn velocity_region(d: Bytes, at: usize, keys: (u8, u8)) -> Result<Region> {
    let wave = d.u32(at)?;
    let flags = d.u32(at + 4)?;
    // Each flag present adds one 32-bit value, in order of the flags.
    let mut values = [0u32; 32];
    let mut next = at + 8;
    for (bit, v) in values.iter_mut().enumerate() {
        if flags >> bit & 1 != 0 {
            *v = d.u32(next)?;
            next += 4;
        }
    }
    let has = |bit: usize| flags >> bit & 1 != 0;
    let envelope = if has(REGION_ENVELOPE) {
        // An offset to a reference to the curve.
        let curve_ref = d.at(at, values[REGION_ENVELOPE] as usize)?;
        let (_, curve) = d.reference(curve_ref, curve_ref)?;
        let c = d.slice(curve.ok_or("no envelope")?, 5)?;
        Adshr {
            attack: c[0],
            decay: c[1],
            sustain: c[2],
            hold: c[3],
            release: c[4],
        }
    } else {
        Adshr {
            attack: 127,
            decay: 127,
            sustain: 127,
            hold: 0,
            release: 127,
        }
    };
    // The tuning (flag 3) is 1.0 throughout SmileBASIC 4's bank, so it is
    // not kept.
    Ok(Region {
        keys,
        wave: u16::try_from(wave).map_err(|_| "too many waveforms")?,
        original_key: if has(REGION_ORIGINAL_KEY) {
            values[REGION_ORIGINAL_KEY] as u8
        } else {
            60
        },
        volume: if has(REGION_VOLUME) {
            values[REGION_VOLUME] as u8
        } else {
            127
        },
        pan: if has(REGION_PAN) {
            values[REGION_PAN] as u8
        } else {
            64
        },
        envelope,
        ignore_note_off: has(REGION_IGNORE_NOTE_OFF) && values[REGION_IGNORE_NOTE_OFF] & 0xFF != 0,
    })
}

/// A waveform's left or only channel, with its bytes, and the right
/// channel of a stereo one.
type Channels<'a> = (WaveInfo, &'a [u8], Option<(WaveInfo, &'a [u8])>);

/// The waveforms of an FWAR, each with its bytes.
fn war_waves<'a>(d: Bytes<'a>, at: usize) -> Result<Vec<Channels<'a>>> {
    let info = d.block(at, b"FWAR", FWAR_INFO)?;
    let files = d.block(at, b"FWAR", FWAR_FILES)?;
    (0..d.offset(info)?)
        .map(|i| wave(d, d.at(files, d.offset(info + 4 + i * 12 + 4)?)?))
        .collect()
}

/// One FWAV: its first channel, and its second for the few SmileBASIC 4
/// waves that are stereo. More channels are not kept.
fn wave<'a>(d: Bytes<'a>, at: usize) -> Result<Channels<'a>> {
    let info = d.block(at, b"FWAV", FWAV_INFO)?;
    let channels = info + 0x14;
    let (left, bytes) = match d.offset(channels)? {
        0 => return Err("a waveform has no channels".into()),
        _ => channel(d, at, 0)?,
    };
    let right = match d.offset(channels)? {
        1 => None,
        _ => Some(channel(d, at, 1)?),
    };
    Ok((left, bytes, right))
}

/// Channel `i` of the FWAV at `at`.
fn channel<'a>(d: Bytes<'a>, at: usize, i: usize) -> Result<(WaveInfo, &'a [u8])> {
    let info = d.block(at, b"FWAV", FWAV_INFO)?;
    let data = d.block(at, b"FWAV", FWAV_DATA)?;
    let (format, looped) = (d.u8(info)?, d.u8(info + 1)? != 0);
    let rate = d.u32(info + 4)?;
    let loop_start = d.u32(info + 8)?;
    let len = d.u32(info + 12)?;
    let channels = info + 0x14;
    let (_, channel) = d.reference(channels + 4 + i * 8, channels)?;
    let channel = channel.ok_or("no channel")?;
    let (_, samples) = d.reference(channel, data)?;
    let samples = samples.ok_or("no samples")?;
    let encoding = match format {
        1 => Encoding::Pcm16,
        2 => {
            let (_, adpcm) = d.reference(channel + 8, channel)?;
            let adpcm = adpcm.ok_or("no ADPCM coefficients")?;
            let mut coefs = [0; 16];
            for (i, c) in coefs.iter_mut().enumerate() {
                *c = d.i16(adpcm + 2 * i)?;
            }
            // After the coefficients: the start context, then the loop
            // context, each a predictor and scale, then two samples.
            let loop_context = adpcm + 32 + 6;
            Encoding::Adpcm {
                coefs,
                loop_history: [d.i16(loop_context + 2)?, d.i16(loop_context + 4)?],
            }
        }
        _ => return Err(format!("waveform encoding {format} is not supported")),
    };
    let size = WaveInfo::bytes_for(&encoding, len).ok_or(DAMAGED)?;
    let bytes = d.slice(samples, size)?;
    let info = WaveInfo {
        right: None,
        rate,
        len,
        loop_start: (looped && loop_start < len).then_some(loop_start),
        encoding,
        data: 0..bytes.len(),
    };
    Ok((info, bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A Nintendo file: header, then blocks of the given types and bodies.
    fn file(magic: &[u8; 4], blocks: &[(u16, &[u8; 4], Vec<u8>)]) -> Vec<u8> {
        let header = (0x14 + blocks.len() * 12).next_multiple_of(0x20);
        let mut out = vec![0u8; header];
        out[..4].copy_from_slice(magic);
        out[0x10..0x12].copy_from_slice(&(blocks.len() as u16).to_le_bytes());
        for (i, (kind, name, body)) in blocks.iter().enumerate() {
            let at = out.len() as u32;
            let e = 0x14 + i * 12;
            out[e..e + 2].copy_from_slice(&kind.to_le_bytes());
            out[e + 4..e + 8].copy_from_slice(&at.to_le_bytes());
            out.extend_from_slice(*name);
            out.extend_from_slice(&(body.len() as u32 + 8).to_le_bytes());
            out.extend_from_slice(body);
        }
        out
    }

    fn put(out: &mut Vec<u8>, words: &[u32]) {
        words
            .iter()
            .for_each(|w| out.extend_from_slice(&w.to_le_bytes()));
    }

    /// A reference: type and offset.
    fn r(kind: u32, offset: u32) -> [u32; 2] {
        [kind, offset]
    }

    fn fwav() -> Vec<u8> {
        let mut info = Vec::new();
        // ADPCM, looping, 32 kHz, loop at 14 of 28 samples.
        put(&mut info, &[0x0102, 32000, 14, 28, 14]);
        put(&mut info, &[1]);
        put(&mut info, &r(0x7100, 12));
        // The channel: samples at 0 in the data, coefficients from 0x14.
        put(&mut info, &r(0x1F00, 0));
        put(&mut info, &r(0x0300, 0x14));
        put(&mut info, &[0]);
        info.extend((1..=16i16).flat_map(|c| c.to_le_bytes()));
        info.extend([0i16, 0, 0, 0, 7, -7].iter().flat_map(|c| c.to_le_bytes()));
        file(
            b"FWAV",
            &[
                (0x7000, b"INFO", info),
                (0x7001, b"DATA", (0..16).collect()),
            ],
        )
    }

    /// `fwav` with two channels: the samples of the second follow those
    /// of the first.
    fn stereo_fwav() -> Vec<u8> {
        let mut info = Vec::new();
        put(&mut info, &[0x0102, 32000, 14, 28, 14]);
        put(&mut info, &[2]);
        put(&mut info, &r(0x7100, 20));
        put(&mut info, &r(0x7100, 84));
        for (samples, first) in [(0, 1i16), (16, 101)] {
            put(&mut info, &r(0x1F00, samples));
            put(&mut info, &r(0x0300, 0x14));
            put(&mut info, &[0]);
            info.extend((first..first + 16).flat_map(|c| c.to_le_bytes()));
            info.extend([0i16, 0, 0, 0, 7, -7].iter().flat_map(|c| c.to_le_bytes()));
        }
        file(
            b"FWAV",
            &[
                (0x7000, b"INFO", info),
                (0x7001, b"DATA", (0..32).collect()),
            ],
        )
    }

    #[test]
    fn stereo_waves_keep_their_second_channel() {
        let wav = stereo_fwav();
        let (left, bytes, right) = wave(Bytes(&wav), 0).unwrap();
        let (right, right_bytes) = right.unwrap();
        assert_eq!(bytes, (0..16).collect::<Vec<u8>>());
        assert_eq!(right_bytes, (16..32).collect::<Vec<u8>>());
        let coefs = |e: &Encoding| match e {
            Encoding::Adpcm { coefs, .. } => coefs[0],
            Encoding::Pcm16 => 0,
        };
        assert_eq!((coefs(&left.encoding), coefs(&right.encoding)), (1, 101));
        assert_eq!(
            (right.rate, right.len, right.loop_start),
            (32000, 28, Some(14))
        );
        assert!(wave(Bytes(&fwav()), 0).unwrap().2.is_none());
    }

    fn fwar() -> Vec<u8> {
        let wav = fwav();
        let mut info = Vec::new();
        put(&mut info, &[1, 0x1F00, 0, wav.len() as u32]);
        file(b"FWAR", &[(0x6800, b"INFO", info), (0x6801, b"FILE", wav)])
    }

    fn fbnk() -> Vec<u8> {
        let mut info = Vec::new();
        put(&mut info, &r(0x0100, 0x10));
        put(&mut info, &r(0x0101, 0x1C));
        // Wave table at 0x10: one wave, archive 0 index 0.
        put(&mut info, &[1, 0x0500_0000, 0]);
        // Instruments at 0x1C: one empty, one at 0x14 from the table.
        put(&mut info, &[2]);
        put(&mut info, &[0, u32::MAX]);
        put(&mut info, &r(0x5900, 0x14));
        // The instrument: two key ranges, up to 59 and up to 127.
        put(&mut info, &r(0x6001, 8));
        put(&mut info, &[2, 0x7F3B]);
        put(&mut info, &r(0x5901, 0x18));
        put(&mut info, &r(0x5901, 0x18));
        // The key region, shared: a direct velocity table.
        put(&mut info, &r(0x6000, 8));
        put(&mut info, &r(0x5902, 8));
        // The velocity region.
        put(
            &mut info,
            &[0, 0x21F, 57, 100, 0x14, 1.0f32.to_bits(), 1, 0x20],
        );
        put(&mut info, &r(0, 8));
        info.extend_from_slice(&[127, 20, 0, 0, 30, 0, 0, 0]);
        file(b"FBNK", &[(0x5800, b"INFO", info)])
    }

    /// A sequence file whose code is `FF 80 01 FF`, then zeros up to `len`.
    fn fseq(len: usize) -> Vec<u8> {
        let mut code = vec![0xFF, 0x80, 1, 0xFF];
        code.resize(len, 0);
        file(b"FSEQ", &[(0x5000, b"DATA", code)])
    }

    /// An archive of the bank, the waves and one sequence, sound `B07`,
    /// next to a sound `X1` that is not a preset.
    fn fsar() -> Vec<u8> {
        fsar_with(0, 4)
    }

    /// `fsar` with `extra` more references to the sound `B07`, and code of
    /// `code` bytes.
    fn fsar_with(extra: u32, code: usize) -> Vec<u8> {
        let mut data = Vec::new();
        let mut at = Vec::new();
        for f in [fbnk(), fwar(), fseq(code)] {
            data.resize(data.len().next_multiple_of(32), 0);
            at.push((data.len() as u32, f.len() as u32));
            data.extend_from_slice(&f);
        }
        let mut info = Vec::new();
        // The sound table at 0x98, five unused references, and the file
        // table at 0x40.
        put(&mut info, &r(0x2100, 0x98));
        for _ in 0..5 {
            put(&mut info, &r(0, u32::MAX));
        }
        put(&mut info, &r(0x2106, 0x40));
        put(&mut info, &r(0, u32::MAX));
        // The file table: three entries, each a reference to a location.
        put(&mut info, &[3]);
        for i in 0..3 {
            put(&mut info, &r(0, 0x1C + 20 * i));
        }
        for (offset, len) in at {
            put(&mut info, &r(0x220C, 8));
            put(&mut info, &[0x1F00, offset, len]);
        }
        // The sounds: file, player, volume, the sequence's details, the
        // flags and the name; the details give the start, 1.
        put(&mut info, &[2 + extra]);
        put(&mut info, &r(0, 20 + 8 * extra));
        put(&mut info, &r(0, 68 + 8 * extra));
        for _ in 0..extra {
            put(&mut info, &r(0, 20 + 8 * extra));
        }
        for (file, name) in [(2, 0), (2, 1)] {
            put(&mut info, &[file, 0, 59]);
            put(&mut info, &r(0x2203, 28));
            put(&mut info, &[1, name]);
            put(&mut info, &[0x0100, 0x18, 1, 3, 1]);
        }
        let mut strings = Vec::new();
        put(&mut strings, &r(0x2400, 8));
        put(&mut strings, &[2, 0x1F01, 28, 4, 0x1F01, 32, 3]);
        strings.extend_from_slice(b"B07\0X1\0");
        file(
            b"FSAR",
            &[
                (0x2000, b"STRG", strings),
                (0x2001, b"INFO", info),
                (0x2002, b"FILE", data),
            ],
        )
    }

    #[test]
    fn reads_sequences() {
        let s = sequences(&fsar()).unwrap();
        let expected = Sequence {
            volume: 59,
            start: 1,
            code: vec![0xFF, 0x80, 1, 0xFF],
        };
        assert_eq!(s.get(Kind::Song, 7).as_deref(), Some(&expected));
        assert_eq!(s.songs.iter().flatten().count(), 1);
        assert!(s.beeps.is_empty());
    }

    #[test]
    fn reads_a_bank() {
        let bank = bank(&fsar()).unwrap();
        assert_eq!(bank.instruments, [0..0, 0..2]);
        let region = Region {
            keys: (0, 59),
            wave: 0,
            original_key: 57,
            volume: 100,
            pan: 0x14,
            envelope: Adshr {
                attack: 127,
                decay: 20,
                sustain: 0,
                hold: 0,
                release: 30,
            },
            ignore_note_off: true,
        };
        assert_eq!(bank.regions[0], region);
        assert_eq!(bank.regions[1].keys, (60, 127));
        let mut coefs = [0; 16];
        coefs.iter_mut().zip(1..).for_each(|(c, v)| *c = v);
        assert_eq!(
            bank.waves,
            [WaveInfo {
                right: None,
                rate: 32000,
                len: 28,
                loop_start: Some(14),
                encoding: Encoding::Adpcm {
                    coefs,
                    loop_history: [7, -7],
                },
                data: 0..16,
            }]
        );
        assert_eq!(bank.data, (0..16).collect::<Vec<u8>>());
        assert!(Bank::from_bytes(&bank.to_bytes()).is_some());
    }

    /// One more wave-table entry naming the first wave, `extra` times:
    /// the bank's info block gets longer, so the table moves to its end.
    fn fsar_with_repeated_waves(extra: u32) -> Vec<u8> {
        let mut info = Vec::new();
        let table = 0x10u32;
        put(&mut info, &r(0x0100, table));
        put(&mut info, &r(0x0101, table + 4 + (extra + 1) * 8));
        put(&mut info, &[extra + 1]);
        for _ in 0..=extra {
            put(&mut info, &[0x0500_0000, 0]);
        }
        put(&mut info, &[0]);
        let fbnk = file(b"FBNK", &[(0x5800, b"INFO", info)]);
        archive_of(&[fbnk, fwar()])
    }

    /// An archive of these files, without sounds.
    fn archive_of(files: &[Vec<u8>]) -> Vec<u8> {
        let mut data = Vec::new();
        let mut at = Vec::new();
        for f in files {
            data.resize(data.len().next_multiple_of(32), 0);
            at.push((data.len() as u32, f.len() as u32));
            data.extend_from_slice(f);
        }
        let mut info = Vec::new();
        for _ in 0..6 {
            put(&mut info, &r(0, u32::MAX));
        }
        put(&mut info, &r(0x2106, 0x40));
        put(&mut info, &r(0, u32::MAX));
        put(&mut info, &[at.len() as u32]);
        for i in 0..at.len() as u32 {
            put(&mut info, &r(0, 4 + at.len() as u32 * 8 + 20 * i));
        }
        for (offset, len) in at {
            put(&mut info, &r(0x220C, 8));
            put(&mut info, &[0x1F00, offset, len]);
        }
        file(b"FSAR", &[(0x2001, b"INFO", info), (0x2002, b"FILE", data)])
    }

    #[test]
    fn waves_may_not_multiply_the_file() {
        // A little repetition still fits in the file, and is kept.
        let a = fsar_with_repeated_waves(1);
        assert_eq!(bank(&a).unwrap().data.len(), 32);
        // A table many times the wave's size would make many times the
        // file's size in bytes.
        let a = fsar_with_repeated_waves(2000);
        let e = bank(&a).unwrap_err();
        assert!(e.contains("overlap"), "{e}");
    }

    #[test]
    fn regions_may_not_multiply_the_file() {
        // Many instruments naming one table of many regions.
        let (instruments, ranges) = (2000u32, 100u32);
        let mut info = Vec::new();
        put(&mut info, &r(0x0100, 0x10));
        put(&mut info, &r(0x0101, 0x14));
        put(&mut info, &[0]);
        put(&mut info, &[instruments]);
        let after = 4 + instruments * 8;
        for _ in 0..instruments {
            put(&mut info, &r(0x5900, after));
        }
        // Each entry counts from the table, so all reach the instrument.
        let at_instrument = info.len();
        assert_eq!(at_instrument as u32, 0x14 + after);
        put(&mut info, &r(0x6001, 8));
        let keys = ranges.next_multiple_of(4);
        put(&mut info, &[ranges]);
        info.extend((0..keys).map(|k| k.min(127) as u8));
        for _ in 0..ranges {
            put(&mut info, &r(0x5901, 4 + keys + ranges * 8));
        }
        put(&mut info, &r(0x6000, 8));
        put(&mut info, &r(0x5902, 8));
        put(&mut info, &[0, 0]);
        // Refer from the instrument itself: reference base is its start.
        let mut fixed = info;
        for i in 0..instruments as usize {
            let e = 0x14 + 4 + i * 8 + 4;
            let to = (at_instrument - 0x14) as u32;
            fixed[e..e + 4].copy_from_slice(&to.to_le_bytes());
        }
        let fbnk = file(b"FBNK", &[(0x5800, b"INFO", fixed)]);
        let a = archive_of(&[fbnk, fwar()]);
        let e = bank(&a).unwrap_err();
        assert!(e.contains("too many key regions"), "{e}");
    }

    #[test]
    fn too_many_key_ranges_are_an_error() {
        let mut info = Vec::new();
        put(&mut info, &r(0x0100, 0x10));
        put(&mut info, &r(0x0101, 0x14));
        put(&mut info, &[0]);
        put(&mut info, &[1]);
        put(&mut info, &r(0x5900, 12));
        put(&mut info, &r(0x6001, 8));
        put(&mut info, &[KEY_RANGES as u32 + 1]);
        let fbnk = file(b"FBNK", &[(0x5800, b"INFO", info)]);
        let e = bank(&archive_of(&[fbnk, fwar()])).unwrap_err();
        assert!(e.contains("key ranges"), "{e}");
    }

    /// `fsar`'s sound `B07` under another name.
    fn fsar_named(name: &[u8; 3]) -> Vec<u8> {
        let mut a = fsar();
        let at = a.windows(4).position(|w| w == b"B07\0").unwrap();
        a[at..at + 3].copy_from_slice(name);
        a
    }

    #[test]
    fn a_sequence_number_stays_in_the_format() {
        assert!(
            sequences(&fsar_named(b"B99"))
                .unwrap()
                .get(Kind::Song, 99)
                .is_some()
        );
        // A name as long as both names of `fsar`, its number too large for
        // the file format, and a list as long as the number says would be
        // huge.
        let mut a = fsar();
        let strings = a.windows(7).position(|w| w == b"B07\0X1\0").unwrap();
        a[strings..strings + 7].copy_from_slice(b"B99999\0");
        // The first string's size, in the table before the strings.
        let size = strings - 2 * 12 + 8;
        a[size..size + 4].copy_from_slice(&7u32.to_le_bytes());
        let e = sequences(&a).unwrap_err();
        assert!(e.contains("too large"), "{e}");
    }

    #[test]
    fn sequence_code_may_not_multiply_the_file() {
        // Many references to one sound, whose file is long.
        let extra = 200;
        let a = fsar_with(extra, 4096);
        let e = sequences(&a).unwrap_err();
        assert!(e.contains("too many sounds"), "{e}");
        // Without so many references it reads, keeping the file's code.
        let a = fsar_with(0, 4096);
        let code = sequences(&a).unwrap().get(Kind::Song, 7).unwrap();
        assert_eq!(code.code.len(), 4096);
    }

    /// Past the end of the file, and with sums that would wrap where a
    /// usize is small: errors, not panics.
    #[test]
    fn offsets_past_the_file_are_errors() {
        let d = Bytes(&[0, 0, 0, 0, 0xFF, 0xFF, 0xFF, 0x7F]);
        assert!(d.reference(0, 0).is_err());
        assert!(d.reference(0, usize::MAX).is_err());
        assert!(d.at(usize::MAX, 1).is_err());
        assert_eq!(d.at(4, 4), Ok(8));
        assert!(d.at(4, 5).is_err());
        let null = Bytes(&[1, 0, 0, 0, 0xFF, 0xFF, 0xFF, 0xFF]);
        assert_eq!(null.reference(0, usize::MAX), Ok((1, None)));
    }

    #[test]
    fn damage_is_an_error() {
        let mut a = fsar();
        a.truncate(a.len() / 2);
        assert!(bank(&a).is_err());
        assert!(sequences(&a).is_err());
        assert!(bank(b"FSAR").is_err());
    }
}
