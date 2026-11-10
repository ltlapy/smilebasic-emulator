//! What tracks and BEEP play instruments on: a SoundFont through
//! rustysynth, or SmileBASIC 4's own instruments through the sampler.

use std::sync::Arc;

use std::panic::{AssertUnwindSafe, catch_unwind};

use rustysynth::{SoundFont, Synthesizer, SynthesizerSettings};

use super::RATE;
use super::bank::Bank;
use super::sampler::{Lfo, LfoTarget, Sampler};
use super::wave::Envelope;

/// Channels a synthesizer has: the tracks' 16 and BEEP's 16 handles.
pub const CHANNELS: usize = 16;

/// Where instruments come from.
#[derive(Clone, Debug)]
pub enum Instruments {
    Font(Arc<SoundFont>),
    Bank(Arc<Bank>),
}

impl Instruments {
    pub(crate) fn synth(&self) -> Box<dyn Synth> {
        match self {
            Instruments::Font(f) => Box::new(FontSynth::new(f)),
            Instruments::Bank(b) => Box::new(Sampler::new(b.clone())),
        }
    }
}

/// A synthesizer with [`CHANNELS`] channels. Values are MIDI's 0 to 127.
pub(crate) trait Synth {
    /// Whether `program` (MML's @ number) has an instrument here.
    fn plays(&self, program: i32) -> bool;
    fn program(&mut self, channel: usize, program: i32);
    fn volume(&mut self, channel: usize, value: i32);
    fn expression(&mut self, channel: usize, value: i32);
    fn pan(&mut self, channel: usize, value: i32);
    /// The modulation wheel: the vibrato's depth.
    fn modulation(&mut self, channel: usize, value: i32);
    /// MML's @MP, @MA or @ML. A SoundFont takes @MP's depth on its
    /// modulation wheel and ignores the rest.
    fn lfo(&mut self, channel: usize, target: LfoTarget, lfo: Lfo) {
        if target == LfoTarget::Pitch {
            self.modulation(channel, lfo.depth);
        }
    }
    fn bend(&mut self, channel: usize, cents: i32);
    /// MML's @E: the envelope the channel's notes start with from now on,
    /// in place of their instruments'; `None` for theirs. A SoundFont's
    /// instruments keep their own.
    fn envelope(&mut self, _channel: usize, _envelope: Option<Envelope>) {}
    fn note_on(&mut self, channel: usize, key: i32, velocity: i32);
    fn note_off(&mut self, channel: usize, key: i32);
    /// A portamento: the channel's notes that are held sound `cents`
    /// off their key.
    fn porta(&mut self, _channel: usize, _cents: f32) {}
    /// A portamento's end: the held note `from` goes on as `to`, without
    /// starting again where the synthesizer can.
    fn retune(&mut self, channel: usize, from: i32, to: i32) {
        self.note_off(channel, from);
        self.note_on(channel, to, 100);
    }
    /// BEEP's pan for a preset effect, 0 to 127: it moves every voice by
    /// as much as a channel's pan would from 64, so a mono effect at 127
    /// plays right only, and each side of a stereo effect moves on its
    /// own. Gives whether the synthesizer can; BEEP balances what it gives
    /// when not.
    fn pan_offset(&mut self, _pan: i32) -> bool {
        false
    }
    /// Every note on the channel stops at once.
    fn channel_off(&mut self, channel: usize);
    /// Every note stops at once and the channels go back to how they
    /// started.
    fn reset(&mut self);
    /// Writes `left.len()` samples over the buffers.
    fn render(&mut self, left: &mut [f32], right: &mut [f32]);
}

/// MIDI's channel for drums in rustysynth, which adds 128 to its bank.
const DRUM_CHANNEL: usize = 9;
/// SoundFonts keep drum kits in bank 128.
const DRUM_BANK: i32 = 128;
/// Semitones the pitch bend reaches either way.
const BEND_RANGE: i32 = 24;

/// SmileBASIC 4's drum sets @128 to @148 as GS drum kits, which General
/// MIDI SoundFonts such as GeneralUser GS have.
// These are the nearest GS kits by name, not the sets' own sounds (see
// docs/differences.md).
pub const DRUM_KITS: [i32; 21] = [
    0, 24, 25, 1, 2, 8, 25, 26, 25, 26, 16, 24, 25, 26, 25, 32, 40, 48, 56, 56, 0,
];

pub(crate) struct FontSynth {
    synth: Synthesizer,
    /// Each channel's bend and portamento, in cents, which share MIDI's
    /// pitch bend.
    bends: [(i32, f32); CHANNELS],
    /// Each channel's last velocity, for a portamento's new key.
    velocities: [i32; CHANNELS],
}

impl FontSynth {
    fn new(font: &Arc<SoundFont>) -> FontSynth {
        let mut settings = SynthesizerSettings::new(RATE as i32);
        settings.enable_reverb_and_chorus = false;
        let mut s = FontSynth {
            synth: Synthesizer::new(font, &settings).expect("valid settings"),
            bends: [(0, 0.0); CHANNELS],
            velocities: [100; CHANNELS],
        };
        s.bend_range();
        s
    }

    fn midi(&mut self, channel: usize, command: i32, a: i32, b: i32) {
        self.synth
            .process_midi_message(channel as i32, command, a, b);
    }

    fn bend_range(&mut self) {
        for c in 0..CHANNELS {
            // RPN 0, the pitch bend's range.
            self.midi(c, 0xB0, 0x65, 0);
            self.midi(c, 0xB0, 0x64, 0);
            self.midi(c, 0xB0, 0x06, BEND_RANGE);
        }
    }

    fn controller(&mut self, channel: usize, number: i32, value: i32) {
        self.midi(channel, 0xB0, number, value.clamp(0, 127));
    }

    fn send_bend(&mut self, channel: usize) {
        let (bend, porta) = self.bends[channel];
        let cents = (bend + porta.round() as i32).clamp(-BEND_RANGE * 100, BEND_RANGE * 100);
        let bend = (8192 + cents * 8192 / (BEND_RANGE * 100)).clamp(0, 16383);
        self.midi(channel, 0xE0, bend & 0x7F, bend >> 7);
    }
}

impl Synth for FontSynth {
    fn plays(&self, program: i32) -> bool {
        (0..=148).contains(&program)
    }

    fn program(&mut self, channel: usize, program: i32) {
        let (bank, program) = match program {
            0..=127 => (0, program),
            128..=148 => (DRUM_BANK, DRUM_KITS[(program - 128) as usize]),
            _ => return,
        };
        // rustysynth's drum channel adds the drum bank itself.
        let bank = if channel == DRUM_CHANNEL {
            bank - DRUM_BANK
        } else {
            bank
        };
        self.midi(channel, 0xB0, 0, bank);
        self.midi(channel, 0xC0, program, 0);
    }

    fn volume(&mut self, channel: usize, value: i32) {
        self.controller(channel, 7, value);
    }

    fn expression(&mut self, channel: usize, value: i32) {
        self.controller(channel, 11, value);
    }

    fn pan(&mut self, channel: usize, value: i32) {
        self.controller(channel, 10, value);
    }

    fn modulation(&mut self, channel: usize, value: i32) {
        self.controller(channel, 1, value);
    }

    fn bend(&mut self, channel: usize, cents: i32) {
        self.bends[channel].0 = cents;
        self.send_bend(channel);
    }

    fn porta(&mut self, channel: usize, cents: f32) {
        self.bends[channel].1 = cents;
        self.send_bend(channel);
    }

    /// A SoundFont's notes cannot change key: the new key starts again.
    fn retune(&mut self, channel: usize, from: i32, to: i32) {
        self.note_off(channel, from);
        self.note_on(channel, to, self.velocities[channel]);
    }

    fn note_on(&mut self, channel: usize, key: i32, velocity: i32) {
        self.velocities[channel] = velocity;
        self.synth.note_on(channel as i32, key, velocity);
    }

    fn note_off(&mut self, channel: usize, key: i32) {
        self.synth.note_off(channel as i32, key);
    }

    fn channel_off(&mut self, channel: usize) {
        self.synth.note_off_all_channel(channel as i32, true);
    }

    fn reset(&mut self) {
        self.synth.reset();
        self.bends = [(0, 0.0); CHANNELS];
        self.bend_range();
    }

    /// rustysynth reads outside its samples, and panics, when a note's
    /// steps are longer than its loop: a damaged file can ask for that, and
    /// it finds out only as the note plays. The notes stop; the song goes
    /// on. This is once a tick at most, not per sample.
    fn render(&mut self, left: &mut [f32], right: &mut [f32]) {
        let synth = &mut self.synth;
        let rendered = catch_unwind(AssertUnwindSafe(|| synth.render(left, right)));
        if rendered.is_err() {
            self.synth.note_off_all(true);
            left.fill(0.0);
            right.fill(0.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chunk(id: &[u8; 4], body: &[u8]) -> Vec<u8> {
        let mut out = id.to_vec();
        out.extend_from_slice(&(body.len() as u32).to_le_bytes());
        out.extend_from_slice(body);
        out
    }

    fn list(kind: &[u8; 4], chunks: &[Vec<u8>]) -> Vec<u8> {
        let mut body = kind.to_vec();
        chunks.iter().for_each(|c| body.extend_from_slice(c));
        chunk(b"LIST", &body)
    }

    /// A SoundFont of one sound: 1000 samples at `rate`, looping one.
    fn font(rate: u32) -> Arc<SoundFont> {
        let named = |name: &[u8], rest: &[u8]| {
            let mut out = name.to_vec();
            out.resize(20, 0);
            out.extend_from_slice(rest);
            out
        };
        let words = |w: &[u16]| -> Vec<u8> { w.iter().flat_map(|v| v.to_le_bytes()).collect() };
        let phdr = [
            named(b"P", &[0; 18]),
            named(b"EOP", &[&[0, 0, 0, 0, 1, 0][..], &[0; 12]].concat()),
        ]
        .concat();
        let shdr = [
            (0u32, 900u32, 100u32, 101u32, rate, b"S".as_slice()),
            (0, 0, 0, 0, 0, b"EOS"),
        ]
        .iter()
        .flat_map(|&(s, e, ls, le, r, n)| {
            let mut rest: Vec<u8> = [s, e, ls, le, r]
                .iter()
                .flat_map(|v| v.to_le_bytes())
                .collect();
            rest.extend_from_slice(&[60, 0, 0, 0, 1, 0]);
            named(n, &rest)
        })
        .collect::<Vec<u8>>();
        let samples: Vec<u8> = (0..1000i16).flat_map(|i| (i * 30).to_le_bytes()).collect();
        let info = list(
            b"INFO",
            &[
                chunk(b"ifil", &[2, 0, 1, 0]),
                chunk(b"isng", b"EMU8000\0"),
                chunk(b"INAM", b"test\0\0"),
            ],
        );
        let pdta = list(
            b"pdta",
            &[
                chunk(b"phdr", &phdr),
                chunk(b"pbag", &words(&[0, 0, 1, 0])),
                chunk(b"pmod", &[0; 10]),
                // The instrument.
                chunk(b"pgen", &words(&[41, 0, 0, 0])),
                chunk(
                    b"inst",
                    &[named(b"I", &[0, 0]), named(b"EOI", &[1, 0])].concat(),
                ),
                chunk(b"ibag", &words(&[0, 0, 2, 0])),
                chunk(b"imod", &[0; 10]),
                // It loops, on the sample.
                chunk(b"igen", &words(&[54, 1, 53, 0, 0, 0])),
                chunk(b"shdr", &shdr),
            ],
        );
        let sdta = list(b"sdta", &[chunk(b"smpl", &samples)]);
        let bytes = chunk(b"RIFF", &[b"sfbk".to_vec(), info, sdta, pdta].concat());
        Arc::new(SoundFont::new(&mut std::io::Cursor::new(bytes)).expect("loads"))
    }

    fn render(s: &mut FontSynth) -> f32 {
        let (mut l, mut r) = (vec![0.0; 800], vec![0.0; 800]);
        s.render(&mut l, &mut r);
        l.iter().chain(&r).fold(0.0, |m, x| m.max(x.abs()))
    }

    /// Steps of 100 samples round a loop of 1 run off the end of the data.
    #[test]
    fn a_note_that_reads_past_its_samples_stops_and_the_rest_goes_on() {
        let mut s = FontSynth::new(&font(4_800_000));
        s.note_on(0, 60, 127);
        for _ in 0..4 {
            render(&mut s);
        }
        // Stopped: silence, and no voices left to do it again.
        assert_eq!(render(&mut s), 0.0);
        // A note at a sane rate on the same synthesizer still sounds.
        let mut sane = FontSynth::new(&font(48000));
        sane.note_on(0, 60, 127);
        assert!(render(&mut sane) > 0.0);
    }
}
