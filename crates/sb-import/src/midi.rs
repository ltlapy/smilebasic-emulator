//! Writes the preset songs as Standard MIDI Files, for keeping. A song
//! that loops is written up to the point where every track has looped once.
//!
//! Instruments are chosen the same way as in the SoundFont that `--sf2`
//! writes: bank select (MSB then LSB, together the bank) and program, so
//! the files play with SmileBASIC 4's instruments in a player that takes
//! both bytes. The pitch bend reaches 24 semitones either way.
//!
//! With [`Layout::Gm`] the file is General MIDI and Roland GS: every drum
//! set on channel 10 (a channel that is not a drum set is never put there),
//! and GM System On and GS Reset at the start. Instruments from @149 have
//! no GM sound, so they are chosen by bank select (the MSB alone, as the
//! SoundFont's bank number), and only then; a player without SmileBASIC
//! 4's SoundFont plays them with the GM sound at that program.

use std::collections::HashMap;
use std::sync::Arc;

use sb_runtime::sound::seq::{Event, Sequence, record};

use crate::sf2::Layout;

/// Semitones the pitch bend reaches, set on every channel at the start.
const BEND_RANGE: i32 = 24;

fn varlen(out: &mut Vec<u8>, mut v: u32) {
    let mut bytes = vec![(v & 0x7F) as u8];
    v >>= 7;
    while v > 0 {
        bytes.push((v & 0x7F) as u8 | 0x80);
        v >>= 7;
    }
    out.extend(bytes.iter().rev());
}

/// The channel each of the sequence's channels plays on under General
/// MIDI: drum sets on channel 10, the rest on the other channels.
#[derive(Default)]
struct Channels {
    /// Whether each channel's instrument is a drum set now.
    drums: [bool; 16],
    melodic: HashMap<usize, u8>,
    /// The bank last selected on each channel.
    banks: [u16; 16],
    /// The MIDI channel each sounding note started on, by the note's
    /// sequence channel and key.
    sounding: HashMap<(usize, i32), u8>,
}

const DRUM_CHANNEL: u8 = 9;

impl Channels {
    fn target(&mut self, channel: usize) -> u8 {
        if self.drums[channel & 15] {
            return DRUM_CHANNEL;
        }
        let next = self.melodic.len() as u8;
        // The fifteen channels other than 10; a sixteenth shares the last.
        *self
            .melodic
            .entry(channel)
            .or_insert_with(|| next.min(14) + u8::from(next >= DRUM_CHANNEL))
    }
}

pub fn write(seq: Arc<Sequence>, layout: Layout) -> Vec<u8> {
    let (events, timebase) = record(seq);
    let end = events.last().map_or(0, |e| e.tick);
    let mut track = Vec::new();
    let mut last = 0;
    let mut at = |track: &mut Vec<u8>, tick: u32, bytes: &[u8]| {
        varlen(track, tick - last);
        last = tick;
        track.extend_from_slice(bytes);
    };
    if layout == Layout::Gm {
        // GM System On, then GS Reset (a GM-only device skips the latter).
        at(&mut track, 0, &[0xF0, 5, 0x7E, 0x7F, 9, 1, 0xF7]);
        at(
            &mut track,
            0,
            &[
                0xF0, 10, 0x41, 0x10, 0x42, 0x12, 0x40, 0, 0x7F, 0, 0x41, 0xF7,
            ],
        );
    }
    for c in 0..16u8 {
        // RPN 0, the bend range, then no RPN.
        for (cc, v) in [
            (101, 0),
            (100, 0),
            (6, BEND_RANGE as u8),
            (101, 127),
            (100, 127),
        ] {
            at(&mut track, 0, &[0xB0 | c, cc, v]);
        }
    }
    let mut channels = Channels::default();
    for e in events {
        let byte = |v: i32| v.clamp(0, 127) as u8;
        let c = match layout {
            Layout::Own => e.channel as u8 & 15,
            Layout::Gm => {
                if let Event::Program(p) = e.event {
                    channels.drums[e.channel & 15] = (128..=148).contains(&p);
                }
                match e.event {
                    // A note ends on the channel it began on.
                    Event::NoteOff(key) => channels
                        .sounding
                        .remove(&(e.channel, key))
                        .unwrap_or_else(|| channels.target(e.channel)),
                    Event::NoteOn(key, _) => {
                        let c = channels.target(e.channel);
                        channels.sounding.insert((e.channel, key), c);
                        c
                    }
                    _ => channels.target(e.channel),
                }
            }
        };
        match e.event {
            Event::Program(p) => {
                let (bank, program) = layout.preset(usize::try_from(p).unwrap_or(0));
                // For GM and GS the bank is selected only when it changes.
                let drums = c == DRUM_CHANNEL && layout == Layout::Gm;
                let select = layout == Layout::Own || channels.banks[usize::from(c)] != bank;
                if !drums && select {
                    channels.banks[usize::from(c)] = bank;
                    // GM and GS read the MSB alone, as the SoundFont's bank
                    // number; only SmileBASIC 4's own layout uses both.
                    let (msb, lsb) = match layout {
                        Layout::Own => ((bank >> 7) as u8, (bank & 127) as u8),
                        Layout::Gm => (bank as u8, 0),
                    };
                    at(&mut track, e.tick, &[0xB0 | c, 0, msb]);
                    at(&mut track, e.tick, &[0xB0 | c, 32, lsb]);
                }
                at(&mut track, e.tick, &[0xC0 | c, program as u8]);
            }
            Event::Volume(v) => at(&mut track, e.tick, &[0xB0 | c, 7, byte(v)]),
            Event::Expression(v) => at(&mut track, e.tick, &[0xB0 | c, 11, byte(v)]),
            Event::Pan(v) => at(&mut track, e.tick, &[0xB0 | c, 10, byte(v)]),
            Event::Modulation(v) => at(&mut track, e.tick, &[0xB0 | c, 1, byte(v)]),
            Event::Bend(cents) => {
                let v = (8192 + cents * 8192 / (BEND_RANGE * 100)).clamp(0, 16383);
                at(
                    &mut track,
                    e.tick,
                    &[0xE0 | c, (v & 127) as u8, (v >> 7) as u8],
                );
            }
            Event::NoteOn(key, velocity) => {
                at(
                    &mut track,
                    e.tick,
                    &[0x90 | c, byte(key), byte(velocity).max(1)],
                );
            }
            Event::NoteOff(key) => at(&mut track, e.tick, &[0x80 | c, byte(key), 0]),
            Event::Tempo(t) => {
                let micros = (60_000_000 / t.max(1)).min(0xFF_FFFF);
                let [_, a, b, d] = micros.to_be_bytes();
                at(&mut track, e.tick, &[0xFF, 0x51, 3, a, b, d]);
            }
        }
    }
    at(&mut track, end, &[0xFF, 0x2F, 0]);

    let mut out = b"MThd".to_vec();
    out.extend_from_slice(&6u32.to_be_bytes());
    // Format 0: one track; ticks per quarter note.
    out.extend_from_slice(&[0, 0, 0, 1]);
    out.extend_from_slice(&(timebase.min(0x7FFF) as u16).to_be_bytes());
    out.extend_from_slice(b"MTrk");
    out.extend_from_slice(&(track.len() as u32).to_be_bytes());
    out.extend_from_slice(&track);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_a_song() {
        // @130 (a drum set), then a note that loops.
        let code = vec![0x81, 0x81, 0x02, 60, 90, 48, 0x89, 3, 0, 0];
        let seq = Arc::new(Sequence {
            volume: 127,
            start: 0,
            code,
        });
        let midi = write(seq.clone(), Layout::Own);
        assert_eq!(&midi[..14], b"MThd\0\0\0\x06\0\0\0\x01\0\x30");
        let track = &midi[22..];
        // Bank 128 (MSB 1, LSB 0), program 2, the note, a tempo of 120.
        let program = [0, 0xB0, 0, 1, 0, 0xB0, 32, 0, 0, 0xC0, 2];
        let start = 16 * 5 * 4;
        assert_eq!(&track[start..start + program.len()], program);
        assert!(track.windows(3).any(|w| w == [0x90, 60, 90]));
        assert!(track.windows(3).any(|w| w == [0x80, 60, 0]));
        assert!(track.ends_with(&[0xFF, 0x2F, 0]));
        let mut v = Vec::new();
        varlen(&mut v, 200);
        assert_eq!(v, [0x81, 0x48]);

        // The same in GM: the drum set is GS kit 25 on channel 10, with
        // no bank select, and the system messages come first.
        let midi = write(seq, Layout::Gm);
        let track = &midi[22..];
        assert!(track.starts_with(&[0, 0xF0, 5, 0x7E, 0x7F, 9, 1, 0xF7]));
        assert!(track.windows(2).any(|w| w == [0xC9, 25]));
        assert!(track.windows(3).any(|w| w == [0x99, 60, 90]));
        assert!(track.windows(3).any(|w| w == [0x89, 60, 0]));
        assert!(!track.windows(2).any(|w| w == [0xB9, 0]));
    }

    #[test]
    fn gm_selects_other_banks_by_msb() {
        // @300 (bank 2, program 44), then @5 (back to bank 0).
        let code = vec![0x81, 0x82, 0x2C, 0x81, 5, 0, 0];
        let seq = Arc::new(Sequence {
            volume: 127,
            start: 0,
            code,
        });
        let midi = write(seq, Layout::Gm);
        let track = &midi[22..];
        assert!(track.windows(7).any(|w| w == [0xB0, 0, 2, 0, 0xB0, 32, 0]));
        assert!(track.windows(2).any(|w| w == [0xC0, 44]));
    }

    #[test]
    fn gm_keeps_drums_off_other_channels() {
        let mut channels = Channels::default();
        channels.drums[0] = true;
        assert_eq!(channels.target(0), DRUM_CHANNEL);
        // Channel 9 is not a drum set here, so it moves to another channel.
        assert_eq!(channels.target(9), 0);
        assert_eq!(channels.target(3), 1);
        for c in 10..16 {
            assert_ne!(channels.target(c), DRUM_CHANNEL);
        }
    }
}
