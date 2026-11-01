//! What tracks and BEEP play instruments on: a SoundFont through
//! rustysynth, or SmileBASIC 4's own instruments through the sampler.

use std::sync::Arc;

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
// UNVERIFIED: these are the nearest GS kits by name, not the sets'
// own sounds.
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

    fn render(&mut self, left: &mut [f32], right: &mut [f32]) {
        self.synth.render(left, right);
    }
}
