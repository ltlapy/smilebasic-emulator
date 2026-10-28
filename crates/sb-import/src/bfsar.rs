//! SmileBASIC 4's sound archive, `petc4.bfsar` in the RomFS: Nintendo's
//! FSAR, holding sequences, one bank of instruments (FBNK) and one archive
//! of waveforms (FWAR). This turns the bank and its waveforms into the
//! emulator's own [`Bank`], keeping the waveforms compressed.

use sb_runtime::sound::bank::{Adshr, Bank, Encoding, Region, WaveInfo};
use sb_runtime::sound::seq::{Kind, Sequence, Sequences};

type Result<T> = std::result::Result<T, String>;

/// Offsets that read past the end report the file as damaged.
#[derive(Clone, Copy)]
struct Bytes<'a>(&'a [u8]);

impl<'a> Bytes<'a> {
    fn slice(self, at: usize, len: usize) -> Result<&'a [u8]> {
        at.checked_add(len)
            .and_then(|end| self.0.get(at..end))
            .ok_or_else(|| "damaged: an offset points outside the file".to_string())
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
        Ok((kind, (offset != u32::MAX).then(|| base + offset as usize)))
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
                return Ok(at + self.offset(entry + 4)? + 8);
            }
        }
        Err(format!(
            "{} has no block {kind:#06x}",
            String::from_utf8_lossy(magic)
        ))
    }
}

/// Where each of the archive's files is, by file number; `None` for a
/// file kept outside it.
fn files(d: Bytes) -> Result<Vec<Option<usize>>> {
    let info = d.block(0, b"FSAR", 0x2001)?;
    let data = d.block(0, b"FSAR", 0x2002)?;
    // The info block's seventh reference is the file table.
    let (_, table) = d.reference(info + 6 * 8, info)?;
    let table = table.ok_or("no file table")?;
    let mut files = Vec::new();
    for i in 0..d.offset(table)? {
        let (_, entry) = d.reference(table + 4 + i * 8, table)?;
        let mut at = None;
        if let Some(entry) = entry {
            let (kind, location) = d.reference(entry, entry)?;
            // 0x220C: a file inside the archive.
            if kind == 0x220C {
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
    let strings = d.block(0, b"FSAR", 0x2000)?;
    let (_, table) = d.reference(strings, strings)?;
    let table = table.ok_or("no string table")?;
    (0..d.offset(table)?)
        .map(|i| {
            let entry = table + 4 + i * 12;
            let text = d.slice(table + d.offset(entry + 4)?, d.offset(entry + 8)?)?;
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
    let info = d.block(0, b"FSAR", 0x2001)?;
    let (_, sounds) = d.reference(info, info)?;
    let sounds = sounds.ok_or("no sound table")?;
    let mut out = Sequences::default();
    for i in 0..d.offset(sounds)? {
        let (_, sound) = d.reference(sounds + 4 + i * 8, sounds)?;
        let Some(sound) = sound else { continue };
        // The flags' first value is the name.
        if d.u32(sound + 20)? & 1 == 0 {
            continue;
        }
        let Some(name) = names.get(d.offset(sound + 24)?) else {
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
        // 0x2203: a sequence.
        let (detail_kind, detail) = d.reference(sound + 12, sound)?;
        let (0x2203, Some(detail)) = (detail_kind, detail) else {
            continue;
        };
        let file = files
            .get(d.offset(sound)?)
            .copied()
            .flatten()
            .ok_or_else(|| format!("{name}'s sequence is missing"))?;
        let code = d.block(file, b"FSEQ", 0x5000)?;
        let len = d
            .offset(code - 4)?
            .checked_sub(8)
            .ok_or("a damaged sequence")?;
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

    let info = d.block(fbnk, b"FBNK", 0x5800)?;
    let (_, wave_table) = d.reference(info, info)?;
    let (_, instrument_table) = d.reference(info + 8, info)?;
    let (wave_table, instrument_table) = (
        wave_table.ok_or("no wave table")?,
        instrument_table.ok_or("no instruments")?,
    );

    let mut bank = Bank::default();
    let archive_waves = war_waves(d, fwar)?;
    // The bank names waves by archive and index; SmileBASIC 4 has one
    // archive.
    for i in 0..d.offset(wave_table)? {
        let index = d.offset(wave_table + 4 + i * 8 + 4)?;
        let &(ref info, bytes) = archive_waves
            .get(index)
            .ok_or("an instrument's waveform is missing")?;
        let start = bank.data.len();
        bank.data.extend_from_slice(bytes);
        bank.waves.push(WaveInfo {
            data: start..bank.data.len(),
            ..info.clone()
        });
    }

    for i in 0..d.offset(instrument_table)? {
        let start = bank.regions.len();
        let (kind, at) = d.reference(instrument_table + 4 + i * 8, instrument_table)?;
        if let (0x5900, Some(at)) = (kind, at) {
            let (kind, keys) = d.reference(at, at)?;
            if let Some(keys) = keys {
                key_regions(d, kind, keys, &mut bank.regions)?;
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
        // A region for each key, as drum sets have; neighbouring keys
        // that share a region become one range.
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
        // Ranges: take the first, for the loudest notes being rare.
        0x6001 => {
            let n = d.offset(velocities)?;
            d.reference(velocities + 4 + n.next_multiple_of(4), velocities)?
                .1
        }
        _ => return Err(format!("unknown velocity table {kind:#06x}")),
    };
    velocity_region(d, region.ok_or("empty velocity region")?, keys)
}

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
    let has = |bit: u32| flags >> bit & 1 != 0;
    let envelope = if has(9) {
        // An offset to a reference to the curve.
        let curve_ref = at + values[9] as usize;
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
        original_key: if has(0) { values[0] as u8 } else { 60 },
        volume: if has(1) { values[1] as u8 } else { 127 },
        pan: if has(2) { values[2] as u8 } else { 64 },
        envelope,
        ignore_note_off: has(4) && values[4] & 0xFF != 0,
    })
}

/// The waveforms of an FWAR, each with its bytes.
fn war_waves<'a>(d: Bytes<'a>, at: usize) -> Result<Vec<(WaveInfo, &'a [u8])>> {
    let info = d.block(at, b"FWAR", 0x6800)?;
    let files = d.block(at, b"FWAR", 0x6801)?;
    (0..d.offset(info)?)
        .map(|i| wave(d, files + d.offset(info + 4 + i * 12 + 4)?))
        .collect()
}

/// One FWAV. Only its first channel is kept: SmileBASIC 4's are mono.
fn wave<'a>(d: Bytes<'a>, at: usize) -> Result<(WaveInfo, &'a [u8])> {
    let info = d.block(at, b"FWAV", 0x7000)?;
    let data = d.block(at, b"FWAV", 0x7001)?;
    let (format, looped) = (d.u8(info)?, d.u8(info + 1)? != 0);
    let rate = d.u32(info + 4)?;
    let loop_start = d.u32(info + 8)?;
    let len = d.u32(info + 12)?;
    let channels = info + 0x14;
    if d.offset(channels)? == 0 {
        return Err("a waveform has no channels".into());
    }
    let (_, channel) = d.reference(channels + 4, channels)?;
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
            // After the coefficients: the start's context, then the
            // loop's, each a predictor and scale, then two samples.
            let loop_context = adpcm + 32 + 6;
            Encoding::Adpcm {
                coefs,
                loop_history: [d.i16(loop_context + 2)?, d.i16(loop_context + 4)?],
            }
        }
        _ => return Err(format!("waveform encoding {format} is not supported")),
    };
    let bytes = d.slice(samples, WaveInfo::bytes_for(&encoding, len))?;
    let info = WaveInfo {
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
        // The channel: samples at 0 in the data, coefficients 0x14 on.
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

    /// A sequence file whose code is `FF 80 01 FF`.
    fn fseq() -> Vec<u8> {
        file(b"FSEQ", &[(0x5000, b"DATA", vec![0xFF, 0x80, 1, 0xFF])])
    }

    /// An archive of the bank, the waves and one sequence, sound `B07`,
    /// next to a sound `X1` that is not a preset.
    fn fsar() -> Vec<u8> {
        let mut data = Vec::new();
        let mut at = Vec::new();
        for f in [fbnk(), fwar(), fseq()] {
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
        put(&mut info, &[2]);
        put(&mut info, &r(0, 20));
        put(&mut info, &r(0, 68));
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

    #[test]
    fn damage_is_an_error() {
        let mut a = fsar();
        a.truncate(a.len() / 2);
        assert!(bank(&a).is_err());
        assert!(sequences(&a).is_err());
        assert!(bank(b"FSAR").is_err());
    }
}
