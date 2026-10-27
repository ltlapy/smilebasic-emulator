//! One BGM track: a song's channels stepped through tick by tick, played
//! on a SoundFont synthesizer of the track's own.

use std::rc::Rc;
use std::sync::Arc;

use rustysynth::{SoundFont, Synthesizer, SynthesizerSettings};

use super::mml::{ChordPart, Cmd, Link, Num, Pitch, Song, WHOLE, ticks};
use super::{RATE, Ramp};

/// Ticks in a quarter note; T counts quarter notes a minute.
const QUARTER: u32 = WHOLE / 4;
/// Commands a channel may run on one tick before it is taken to be stuck
/// in a loop that plays nothing.
const COMMANDS_PER_TICK: u32 = 10_000;
/// MIDI's channel for drums in rustysynth, which adds 128 to its bank.
const DRUM_CHANNEL: i32 = 9;
/// SoundFonts keep drum kits in bank 128.
const DRUM_BANK: i32 = 128;

/// SmileBASIC 4's drum sets @128 to @148 as GS drum kits, which General
/// MIDI SoundFonts such as GeneralUser GS have.
// UNVERIFIED: these are the nearest GS kits by name, not the sets'
// own sounds.
const DRUM_KITS: [i32; 21] = [
    0, 24, 25, 1, 2, 8, 25, 26, 25, 26, 16, 24, 25, 26, 25, 32, 40, 48, 56, 56, 0,
];

/// What a channel plays with, from @.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Voice {
    /// A General MIDI instrument or drum kit: bank and program.
    Font(i32, i32),
    /// PSG, user waveforms and sound effects, not played yet.
    Silent,
}

fn voice(program: i32) -> Voice {
    match program {
        0..=127 => Voice::Font(0, program),
        128..=148 => Voice::Font(DRUM_BANK, DRUM_KITS[(program - 128) as usize]),
        _ => Voice::Silent,
    }
}

#[derive(Clone, Debug)]
struct Channel {
    pc: usize,
    /// Ticks until the next command runs.
    wait: u32,
    /// Ticks until the sounding notes stop; 0 when held for a link.
    gate: u32,
    /// Open repeats: where they start and how many times are left (0 for
    /// ever).
    repeats: Vec<(usize, u32)>,
    octave: i32,
    length: u32,
    quantize: i32,
    key: i32,
    volume: i32,
    velocity: i32,
    detune: i32,
    voice: Voice,
    /// Notes sounding now, as MIDI keys.
    sounding: Vec<i32>,
    /// How the last note joins the next.
    link: Link,
    done: bool,
}

impl Default for Channel {
    fn default() -> Self {
        Channel {
            pc: 0,
            wait: 0,
            gate: 0,
            repeats: Vec::new(),
            octave: 4,
            length: QUARTER,
            // UNVERIFIED: notes sound for their whole length until Q.
            quantize: 32,
            key: 0,
            // UNVERIFIED: the starting volume and velocity.
            volume: 100,
            velocity: 100,
            detune: 0,
            voice: Voice::Font(0, 0),
            sounding: Vec::new(),
            link: Link::None,
            done: false,
        }
    }
}

pub(crate) struct Track {
    /// None without a SoundFont: the song goes on, silent.
    synth: Option<Synthesizer>,
    song: Option<Rc<Song>>,
    channels: Vec<Channel>,
    tempo: u32,
    song_key: i32,
    pub(crate) vars: [i32; 8],
    /// Samples until the next tick.
    until_tick: f64,
    /// Plays: a song that has not ended, or a preset that is not there.
    pub(crate) playing: bool,
    pub(crate) paused: bool,
    /// BGMVOL, 0 to 127.
    pub(crate) volume: i32,
    /// Fades in, out and around pauses.
    pub(crate) fade: Ramp,
    /// What happens when the fade ends.
    pub(crate) after_fade: AfterFade,
    /// BGMPITCH, in cents.
    pub(crate) pitch: i32,
    pub(crate) wet: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AfterFade {
    Nothing,
    Stop,
    Pause,
}

impl Track {
    pub(crate) fn new(font: Option<&Arc<SoundFont>>) -> Track {
        let mut settings = SynthesizerSettings::new(RATE as i32);
        settings.enable_reverb_and_chorus = false;
        Track {
            synth: font.map(|f| Synthesizer::new(f, &settings).expect("valid settings")),
            song: None,
            channels: Vec::new(),
            tempo: 120,
            song_key: 0,
            vars: [0; 8],
            until_tick: 0.0,
            playing: false,
            paused: false,
            volume: 127,
            fade: Ramp::at(1.0),
            after_fade: AfterFade::Nothing,
            pitch: 0,
            wet: 0,
        }
    }

    /// Starts `song`, or plays silence for a song that is not there.
    pub(crate) fn play(&mut self, song: Option<Rc<Song>>, volume: i32, fade_frames: u32) {
        if let Some(s) = &mut self.synth {
            s.reset();
        }
        self.channels = vec![Channel::default(); super::mml::CHANNELS];
        self.tempo = 120;
        self.song_key = 0;
        self.vars = [0; 8];
        self.until_tick = 0.0;
        self.song = song;
        self.playing = true;
        self.paused = false;
        self.volume = volume;
        self.fade = Ramp::at(if fade_frames > 0 { 0.0 } else { 1.0 });
        self.fade.to(1.0, fade_frames);
        self.after_fade = AfterFade::Nothing;
        for c in 0..self.channels.len() {
            self.apply_voice(c);
            self.controller(c, 7, 100);
        }
    }

    pub(crate) fn stop(&mut self) {
        self.playing = false;
        self.paused = false;
        self.song = None;
        if let Some(s) = &mut self.synth {
            s.note_off_all(true);
        }
    }

    fn midi(&mut self, c: usize, command: i32, a: i32, b: i32) {
        if let Some(s) = &mut self.synth {
            s.process_midi_message(c as i32, command, a, b);
        }
    }

    fn controller(&mut self, c: usize, number: i32, value: i32) {
        self.midi(c, 0xB0, number, value.clamp(0, 127));
    }

    fn apply_voice(&mut self, c: usize) {
        let Voice::Font(bank, program) = self.channels[c].voice else {
            return;
        };
        // rustysynth's drum channel adds the drum bank itself.
        let bank = if c as i32 == DRUM_CHANNEL {
            bank - DRUM_BANK
        } else {
            bank
        };
        self.midi(c, 0xB0, 0, bank);
        self.midi(c, 0xC0, program, 0);
    }

    /// The channel's pitch bend for the part of BGMPITCH and @D below a
    /// semitone; whole semitones move the notes themselves.
    fn cents(&self, c: usize) -> i32 {
        self.pitch + self.channels[c].detune * 100 / 64
    }

    fn apply_bend(&mut self, c: usize) {
        let fraction = self.cents(c).rem_euclid(100);
        // Two semitones either way span the bend's 14 bits.
        let bend = 8192 + fraction * 8192 / 200;
        self.midi(c, 0xE0, bend & 0x7F, bend >> 7);
    }

    pub(crate) fn set_pitch(&mut self, cents: i32) {
        self.pitch = cents;
        for c in 0..self.channels.len() {
            self.apply_bend(c);
        }
    }

    fn var(&self, n: Num) -> i32 {
        match n {
            Num::Lit(v) => v,
            Num::Var(v) => self.vars[usize::from(v)],
        }
    }

    fn notes_off(&mut self, c: usize) {
        for key in std::mem::take(&mut self.channels[c].sounding) {
            if let Some(s) = &mut self.synth {
                s.note_off(c as i32, key);
            }
        }
    }

    fn midi_key(&self, c: usize, octave: i32, semitone: i32) -> i32 {
        let ch = &self.channels[c];
        let shift = self.song_key + ch.key + self.cents(c).div_euclid(100);
        (octave + 1) * 12 + semitone + shift
    }

    /// Starts `keys` for `ticks`, joined to what sounds as the last note
    /// asked.
    fn sound(&mut self, c: usize, keys: Vec<i32>, ticks: u32, link: Link) {
        let tie = self.channels[c].link != Link::None;
        if tie && self.channels[c].sounding == keys {
            // A tie: the notes go on.
        } else {
            let old = std::mem::take(&mut self.channels[c].sounding);
            if !keys.is_empty() && self.channels[c].voice != Voice::Silent {
                let velocity = self.channels[c].velocity;
                for &key in &keys {
                    if (0..128).contains(&key) {
                        if let Some(s) = &mut self.synth {
                            s.note_on(c as i32, key, velocity.max(1));
                        }
                    }
                }
            }
            // A slur starts the new notes before ending the old.
            for key in old {
                if let Some(s) = &mut self.synth {
                    s.note_off(c as i32, key);
                }
            }
            self.channels[c].sounding = keys.into_iter().filter(|k| (0..128).contains(k)).collect();
        }
        let ch = &mut self.channels[c];
        ch.link = link;
        ch.wait = ticks;
        ch.gate = match link {
            Link::None => (ticks * ch.quantize as u32 / 32).max(1),
            // Held until the next note.
            _ => 0,
        };
    }

    /// Runs channel `c`'s commands until one takes time.
    fn run_channel(&mut self, c: usize) {
        let Some(song) = self.song.clone() else {
            return;
        };
        let cmds = &song.channels[c];
        let up = if song.reversed { -1 } else { 1 };
        let mut budget = COMMANDS_PER_TICK;
        while self.channels[c].wait == 0 && !self.channels[c].done {
            budget -= 1;
            let pc = self.channels[c].pc;
            if budget == 0 || pc >= cmds.len() {
                self.channels[c].done = true;
                break;
            }
            self.channels[c].pc += 1;
            let length = self.channels[c].length;
            let vars = self.vars;
            let lookup = |v: u8| vars[usize::from(v)];
            match &cmds[pc] {
                Cmd::Note { pitch, len, link } => {
                    let Some(t) = ticks(*len, length, lookup) else {
                        continue;
                    };
                    let keys = match *pitch {
                        Pitch::Rest => Vec::new(),
                        Pitch::Name(s) => vec![self.midi_key(c, self.channels[c].octave, s)],
                        Pitch::Number(n) => {
                            let shift = self.midi_key(c, -1, 0);
                            vec![self.var(n) + shift]
                        }
                    };
                    self.sound(c, keys, t, *link);
                }
                Cmd::Chord { parts, len, link } => {
                    let Some(t) = ticks(*len, length, lookup) else {
                        continue;
                    };
                    let mut keys = Vec::new();
                    for part in parts {
                        match *part {
                            ChordPart::Note(s) => {
                                let octave = self.channels[c].octave;
                                keys.push(self.midi_key(c, octave, s));
                            }
                            ChordPart::Octave(n) => self.channels[c].octave = self.var(n),
                            ChordPart::Up => self.channels[c].octave += up,
                            ChordPart::Down => self.channels[c].octave -= up,
                        }
                    }
                    self.sound(c, keys, t, *link);
                }
                Cmd::Length(len) => {
                    if let Some(t) = ticks(*len, length, lookup) {
                        self.channels[c].length = t;
                    }
                }
                Cmd::Octave(n) => self.channels[c].octave = self.var(*n),
                Cmd::Up => self.channels[c].octave += up,
                Cmd::Down => self.channels[c].octave -= up,
                Cmd::Tempo(n) => self.tempo = self.var(*n).clamp(1, 1023) as u32,
                Cmd::Program(n) => {
                    self.channels[c].voice = voice(self.var(*n));
                    self.apply_voice(c);
                }
                Cmd::Volume(n) => {
                    self.channels[c].volume = self.var(*n);
                    self.controller(c, 7, self.channels[c].volume);
                }
                Cmd::Louder(n) | Cmd::Softer(n) => {
                    let step = self.var(*n);
                    let step = if matches!(cmds[pc], Cmd::Softer(_)) {
                        -step
                    } else {
                        step
                    };
                    let v = (self.channels[c].volume + step).clamp(0, 127);
                    self.channels[c].volume = v;
                    self.controller(c, 7, v);
                }
                Cmd::Velocity(n) => self.channels[c].velocity = self.var(*n).clamp(0, 127),
                Cmd::Pan(n) => self.controller(c, 10, self.var(*n)),
                Cmd::Gate(n) => self.channels[c].quantize = self.var(*n).clamp(0, 32),
                Cmd::Key(n) => self.channels[c].key = self.var(*n),
                Cmd::SongKey(n) => self.song_key = self.var(*n),
                Cmd::Detune(n) => {
                    self.channels[c].detune = self.var(*n);
                    self.apply_bend(c);
                }
                // The SoundFont's vibrato, through the modulation wheel.
                // UNVERIFIED: how depth, range, speed and delay map.
                Cmd::Vibrato([depth, ..]) => self.controller(c, 1, self.var(*depth)),
                Cmd::VibratoOff | Cmd::ModulationOff => self.controller(c, 1, 0),
                // UNVERIFIED: envelopes, tremolo and pan modulation are
                // read but not played on SoundFont instruments.
                Cmd::Envelope(_)
                | Cmd::EnvelopeOff
                | Cmd::Tremolo(_)
                | Cmd::TremoloOff
                | Cmd::PanWave(_)
                | Cmd::PanWaveOff => {}
                Cmd::RepeatStart => {
                    // Find the count at the matching end.
                    let mut depth = 0;
                    let mut count = 0;
                    for cmd in &cmds[pc..] {
                        match cmd {
                            Cmd::RepeatStart => depth += 1,
                            Cmd::RepeatEnd(n) => {
                                depth -= 1;
                                if depth == 0 {
                                    count = self.var(*n).max(0) as u32;
                                    break;
                                }
                            }
                            _ => {}
                        }
                    }
                    self.channels[c].repeats.push((pc + 1, count));
                }
                Cmd::RepeatEnd(_) => {
                    let ch = &mut self.channels[c];
                    if let Some((start, left)) = ch.repeats.last_mut() {
                        match *left {
                            0 => ch.pc = *start,
                            1 => {
                                ch.repeats.pop();
                            }
                            _ => {
                                *left -= 1;
                                ch.pc = *start;
                            }
                        }
                    }
                }
                Cmd::SetVar(n, v) => self.vars[usize::from(*n)] = *v,
            }
        }
    }

    /// One tick: notes whose time is up stop, and channels go on.
    fn tick(&mut self) {
        for c in 0..self.channels.len() {
            let ch = &mut self.channels[c];
            if ch.gate > 0 {
                ch.gate -= 1;
                if ch.gate == 0 {
                    self.notes_off(c);
                }
            }
            let ch = &mut self.channels[c];
            ch.wait = ch.wait.saturating_sub(1);
            self.run_channel(c);
            let ch = &self.channels[c];
            if ch.done && ch.gate == 0 && ch.link != Link::None {
                // A link at the very end has nothing to join.
                self.notes_off(c);
            }
        }
        if self.song.is_some() && self.channels.iter().all(|c| c.done && c.wait == 0) {
            self.playing = false;
        }
    }

    /// Adds `left.len()` samples of the track to the buffers.
    pub(crate) fn render(
        &mut self,
        left: &mut [f32],
        right: &mut [f32],
        scratch: &mut [Vec<f32>; 2],
    ) {
        let n = left.len();
        let [l, r] = scratch;
        l.resize(n, 0.0);
        r.resize(n, 0.0);
        let mut done = 0;
        while done < n {
            if !self.paused && self.playing && self.until_tick <= 0.0 {
                self.tick();
                self.until_tick += f64::from(RATE) * 60.0 / f64::from(self.tempo * QUARTER);
            }
            let chunk = if self.paused || !self.playing {
                n - done
            } else {
                ((self.until_tick.ceil() as usize).max(1)).min(n - done)
            };
            match &mut self.synth {
                Some(s) => s.render(&mut l[done..done + chunk], &mut r[done..done + chunk]),
                None => l[done..done + chunk]
                    .iter_mut()
                    .chain(&mut r[done..done + chunk])
                    .for_each(|x| *x = 0.0),
            }
            if !self.paused && self.playing {
                self.until_tick -= chunk as f64;
            }
            done += chunk;
        }
        let volume = self.volume as f32 / 127.0;
        for i in 0..n {
            let gain = volume * self.fade.value as f32;
            left[i] += l[i] * gain;
            right[i] += r[i] * gain;
            self.fade.step_sample();
        }
        if self.fade.done() {
            match std::mem::replace(&mut self.after_fade, AfterFade::Nothing) {
                AfterFade::Nothing => {}
                AfterFade::Stop => self.stop(),
                AfterFade::Pause => {
                    self.paused = true;
                    if let Some(s) = &mut self.synth {
                        s.note_off_all(true);
                    }
                }
            }
        }
    }
}
