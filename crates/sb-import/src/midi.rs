//! Writes the preset songs as Standard MIDI Files, for keeping. A song
//! that loops is written up to where every track has gone round once.
//!
//! Instruments are chosen as in the SoundFont that `--sf2` writes: bank
//! select (MSB then LSB, together the bank) and program, so the files
//! play with SmileBASIC 4's instruments in a player that takes both bytes.
//! The pitch bend reaches 24 semitones either way.

use std::sync::Arc;

use sb_runtime::sound::seq::{Event, Sequence, record};

use crate::sf2::preset;

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

pub fn write(seq: Arc<Sequence>) -> Vec<u8> {
    let (events, timebase) = record(seq);
    let end = events.last().map_or(0, |e| e.tick);
    let mut track = Vec::new();
    let mut last = 0;
    let mut at = |track: &mut Vec<u8>, tick: u32, bytes: &[u8]| {
        varlen(track, tick - last);
        last = tick;
        track.extend_from_slice(bytes);
    };
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
    for e in events {
        let c = e.channel as u8 & 15;
        let byte = |v: i32| v.clamp(0, 127) as u8;
        match e.event {
            Event::Program(p) => {
                let (bank, program) = preset(usize::try_from(p).unwrap_or(0));
                at(&mut track, e.tick, &[0xB0 | c, 0, (bank >> 7) as u8]);
                at(&mut track, e.tick, &[0xB0 | c, 32, (bank & 127) as u8]);
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
    // Format 0: one track; ticks a quarter note.
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
        let midi = write(Arc::new(Sequence {
            volume: 127,
            start: 0,
            code,
        }));
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
    }
}
