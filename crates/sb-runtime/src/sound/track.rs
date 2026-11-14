//! One BGM track: a song's channels stepped through tick by tick, played
//! on a synthesizer of the track's own.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use super::inspect::{ChannelInfo, State, TrackInfo};
use super::mml::{ChordPart, Cmd, Link, Num, Pitch, Song, WHOLE, ticks};
use super::sampler::{self, Lfo, LfoTarget};
use super::seq::{Player, Sequence};
use super::synth::{Instruments, Synth};
use super::talk::{Singer, key_hz};
use super::wave::{self, Envelope, Waves};
use super::{RATE, Ramp};

/// Ticks in a quarter note; T counts quarter notes a minute.
const QUARTER: u32 = WHOLE / 4;
/// Commands a channel may run on one tick before it is taken to be stuck
/// in a loop that plays nothing.
const COMMANDS_PER_TICK: u32 = 10_000;
/// A sung note's level, as TALK's.
// UNVERIFIED: how loud singing is beside the instruments.
const SUNG_GAIN: f32 = 0.7;
/// Samples in a unit of an LFO's delay: 40 ms, whatever the tempo.
const LFO_DELAY_UNIT: u32 = RATE / 25;
/// What a channel plays with, from @.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Voice {
    /// An instrument of the synthesizer.
    Synth(i32),
    /// The PSG, noise or a user waveform: @216 to @255.
    Wave(i32),
    /// An instrument the synthesizer does not have.
    Silent,
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
    pan: i32,
    /// @E's, over the waveform's own.
    envelope: Option<Envelope>,
    /// @MP's, @MA's and @ML's settings, which @MON turns back on after
    /// @MOF.
    lfo_set: [Lfo; 3],
    /// The LFOs now: as set, or off after @MOF or their own OF.
    lfo: [Lfo; 3],
    voice: Voice,
    /// @'s number, which `voice` plays or not.
    program: i32,
    /// Notes sounding now, as MIDI keys.
    sounding: Vec<i32>,
    /// Notes started, for the debugger.
    notes: u32,
    /// How the last note joins the next.
    link: Link,
    /// A portamento under way.
    glide: Option<Glide>,
    /// `*WA,`'s phonemes, for the next note to sing.
    lyric: Option<Rc<str>>,
    /// The notes sounding are sung.
    singing: bool,
    done: bool,
}

/// A note glides to the next one's key over its gate, in a straight line
/// in semitones, then the next note goes on from there without starting
/// again: `C2_G2` slides up from C for a second and holds G for a second
/// (TEST41).
#[derive(Clone, Copy, Debug)]
struct Glide {
    cents: f32,
    ticks: u32,
    done: u32,
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
            // Notes sound for their whole length until Q.
            quantize: 32,
            key: 0,
            // Both start full (TEST40).
            volume: 127,
            velocity: 127,
            detune: 0,
            pan: 64,
            envelope: None,
            lfo_set: [Lfo::default(); 3],
            lfo: [Lfo::default(); 3],
            voice: Voice::Synth(0),
            program: 0,
            sounding: Vec::new(),
            notes: 0,
            link: Link::None,
            glide: None,
            lyric: None,
            singing: false,
            done: false,
        }
    }
}

pub(crate) struct Track {
    /// None without instruments: the song goes on, silent.
    synth: Option<Box<dyn Synth>>,
    waves: Rc<RefCell<Waves>>,
    /// Waveform notes sounding, with their channels.
    voices: Vec<(usize, i32, wave::Voice)>,
    /// The singing voices of the channels that have sung.
    singers: Vec<(usize, Singer)>,
    song: Option<Rc<Song>>,
    /// A preset song, played instead of `song`.
    seq: Option<Player>,
    channels: Vec<Channel>,
    tempo: u32,
    song_key: i32,
    pub(crate) vars: [i32; 8],
    /// BGMPLAY's song number, and ticks played of it, for the debugger.
    pub(crate) number: i32,
    ticks: u64,
    /// Frames before the song starts, while the sound system wakes up.
    pub(crate) hold: u32,
    /// Frames left of a preset that plays silent but would end.
    silent_left: Option<u32>,
    /// Samples until the next tick.
    until_tick: f64,
    /// Plays: a song that has not ended, or a preset that is not there.
    pub(crate) playing: bool,
    /// How far a song started since the last frame was mixed has got.
    pub(crate) fresh: Fresh,
    /// Frames BGMCHK still says 1 after a fade out ended.
    pub(crate) linger: u32,
    pub(crate) paused: bool,
    /// BGMVOL, 0 to 127, as a linear gain: BGMPLAY's 64 unless given.
    pub(crate) volume: i32,
    /// Fades in, out and around pauses.
    pub(crate) fade: Ramp,
    /// What happens when the fade ends.
    pub(crate) after_fade: AfterFade,
    /// BGMPITCH, in cents.
    pub(crate) pitch: i32,
    /// BGMWET, once set.
    pub(crate) wet: Option<i32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Fresh {
    No,
    Started,
    /// A VSYNC has run since, without waiting.
    Synced,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AfterFade {
    Nothing,
    Stop,
    Pause,
}

impl Track {
    pub(crate) fn new(instruments: Option<&Instruments>, waves: Rc<RefCell<Waves>>) -> Track {
        Track {
            synth: instruments.map(Instruments::synth),
            waves,
            voices: Vec::new(),
            singers: Vec::new(),
            song: None,
            seq: None,
            channels: Vec::new(),
            tempo: 120,
            song_key: 0,
            vars: [0; 8],
            number: 0,
            ticks: 0,
            hold: 0,
            silent_left: None,
            until_tick: 0.0,
            playing: false,
            fresh: Fresh::No,
            linger: 0,
            paused: false,
            volume: 64,
            fade: Ramp::at(1.0),
            after_fade: AfterFade::Nothing,
            pitch: 0,
            wet: None,
        }
    }

    /// Starts `song`, or plays silence for a song that is not there.
    pub(crate) fn play(&mut self, song: Option<Rc<Song>>, volume: i32, fade_frames: u32) {
        self.start(volume, fade_frames);
        self.song = song;
        for c in 0..self.channels.len() {
            self.apply_voice(c);
            if let Some(s) = &mut self.synth {
                s.volume(c, 127);
            }
        }
    }

    /// Starts a preset song.
    pub(crate) fn play_sequence(&mut self, seq: Arc<Sequence>, volume: i32, fade_frames: u32) {
        self.start(volume, fade_frames);
        let mut player = Player::new(seq);
        if let Some(s) = &mut self.synth {
            player.set_pitch(self.pitch, s.as_mut());
        }
        self.seq = Some(player);
    }

    fn start(&mut self, volume: i32, fade_frames: u32) {
        if let Some(s) = &mut self.synth {
            s.reset();
        }
        self.voices.clear();
        self.singers.iter_mut().for_each(|(_, s)| s.stop());
        self.channels = vec![Channel::default(); super::mml::CHANNELS];
        self.tempo = 120;
        self.song_key = 0;
        // Variables the song has not set read -1 (checked on hardware).
        self.vars = [-1; 8];
        self.ticks = 0;
        self.hold = 0;
        self.silent_left = None;
        self.until_tick = 0.0;
        self.song = None;
        self.seq = None;
        self.playing = true;
        self.fresh = Fresh::Started;
        self.linger = 0;
        self.paused = false;
        self.volume = volume;
        self.fade = Ramp::at(if fade_frames > 0 { 0.0 } else { 1.0 });
        self.fade.to(1.0, fade_frames);
        self.after_fade = AfterFade::Nothing;
    }

    pub(crate) fn stop(&mut self) {
        self.playing = false;
        self.linger = 0;
        self.paused = false;
        self.song = None;
        self.seq = None;
        self.silence();
    }

    /// Every note stops at once.
    fn silence(&mut self) {
        if let Some(s) = &mut self.synth {
            (0..self.channels.len()).for_each(|c| s.channel_off(c));
        }
        self.voices.clear();
        self.singers.iter_mut().for_each(|(_, s)| s.stop());
        self.channels.iter_mut().for_each(|ch| ch.singing = false);
    }

    /// Channel `c`'s singing voice.
    fn singer(&mut self, c: usize) -> &mut Singer {
        let i = match self.singers.iter().position(|(sc, _)| *sc == c) {
            Some(i) => i,
            None => {
                self.singers.push((c, Singer::new(c as u32 + 1)));
                self.singers.len() - 1
            }
        };
        &mut self.singers[i].1
    }

    /// Starts singing `letters` at `key` for `ticks`; false when there
    /// is nothing to sing.
    fn sing(&mut self, c: usize, letters: &str, key: i32, ticks: u32) -> bool {
        let hz = key_hz(key, self.cents(c).rem_euclid(100));
        let ms = f64::from(ticks) * 60_000.0 / f64::from(self.tempo * QUARTER);
        let sung = self.singer(c).sing(letters, hz, ms);
        self.channels[c].singing = sung;
        sung
    }

    /// Runs `f` on the synthesizer, if there is one.
    fn synth(&mut self, f: impl FnOnce(&mut dyn Synth)) {
        if let Some(s) = &mut self.synth {
            f(s.as_mut());
        }
    }

    fn set_program(&mut self, c: usize, program: i32) {
        self.channels[c].program = program;
        self.channels[c].voice = match program {
            // SmileBASIC 4's own PSG and noise, from its sound archive,
            // when the instruments came from there.
            216..=223 if self.synth.as_ref().is_some_and(|s| s.plays(program)) => {
                Voice::Synth(program)
            }
            216..=255 => Voice::Wave(program),
            _ if self.synth.as_ref().is_some_and(|s| s.plays(program)) => Voice::Synth(program),
            _ => Voice::Silent,
        };
        self.apply_voice(c);
    }

    fn apply_voice(&mut self, c: usize) {
        if let Voice::Synth(program) = self.channels[c].voice {
            self.synth(|s| s.program(c, program));
        }
    }

    /// The channel's pitch bend for the part of BGMPITCH and @D below a
    /// semitone; whole semitones move the notes themselves.
    fn cents(&self, c: usize) -> i32 {
        self.pitch + self.channels[c].detune * 100 / 64
    }

    fn apply_bend(&mut self, c: usize) {
        let fraction = self.cents(c).rem_euclid(100);
        self.synth(|s| s.bend(c, fraction));
        for (.., v) in self.voices.iter_mut().filter(|(vc, ..)| *vc == c) {
            v.set_pitch(fraction);
        }
    }

    pub(crate) fn set_pitch(&mut self, cents: i32) {
        self.pitch = cents;
        if let (Some(p), Some(s)) = (&mut self.seq, &mut self.synth) {
            p.set_pitch(cents, s.as_mut());
            return;
        }
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
        let keys = std::mem::take(&mut self.channels[c].sounding);
        self.keys_off(c, &keys);
    }

    fn keys_off(&mut self, c: usize, keys: &[i32]) {
        if self.channels[c].singing && !keys.is_empty() {
            self.channels[c].singing = false;
            self.singer(c).release();
        }
        for &key in keys {
            if let Some(s) = &mut self.synth {
                s.note_off(c, key);
            }
        }
        for (.., v) in self
            .voices
            .iter_mut()
            .filter(|(vc, _, v)| *vc == c && keys.contains(&v.key))
        {
            v.release();
        }
    }

    /// Moves the held notes of channel `c` by a portamento's `cents`.
    fn porta(&mut self, c: usize, cents: f32) {
        self.synth(|s| s.porta(c, cents));
        if self.channels[c].singing {
            self.singer(c).bend(cents);
        }
        for (.., v) in self.voices.iter_mut().filter(|(vc, ..)| *vc == c) {
            if !v.released() {
                v.porta = cents;
            }
        }
    }

    /// The end of a portamento: the held notes `from` go on as `to`, the
    /// first to the first and any left over to the last.
    fn retune(&mut self, c: usize, from: &[i32], to: &[i32]) {
        if let (true, Some(&key)) = (self.channels[c].singing, to.first()) {
            let hz = key_hz(key, self.cents(c).rem_euclid(100));
            self.singer(c).retune(hz);
        }
        for (i, &key) in to.iter().enumerate() {
            let Some(&old) = from.get(i).or(from.last()) else {
                return;
            };
            if i >= from.len() {
                self.keys_on(c, &[key]);
                continue;
            }
            self.synth(|s| s.retune(c, old, key));
            for (.., v) in self.voices.iter_mut().filter(|(vc, ..)| *vc == c) {
                if v.key == old && !v.released() {
                    v.retune(key);
                }
            }
        }
        if from.len() > to.len() {
            self.keys_off(c, &from[to.len()..]);
        }
        self.porta(c, 0.0);
    }

    /// The keys the next note of channel `c` from command `pc` on plays,
    /// none for a rest.
    fn next_keys(&self, c: usize, pc: usize) -> Vec<i32> {
        let Some(song) = &self.song else {
            return Vec::new();
        };
        let up = if song.reversed { -1 } else { 1 };
        let mut octave = self.channels[c].octave;
        for cmd in &song.channels[c][pc..] {
            match cmd {
                Cmd::Octave(n) => octave = self.var(*n),
                Cmd::Up => octave += up,
                Cmd::Down => octave -= up,
                Cmd::Note { pitch, .. } => {
                    return match *pitch {
                        Pitch::Rest => Vec::new(),
                        Pitch::Name(s) => vec![self.midi_key(c, octave, s)],
                        Pitch::Number(n) => vec![self.var(n) + self.midi_key(c, -1, 0)],
                    };
                }
                Cmd::Chord { parts, .. } => {
                    let mut keys = Vec::new();
                    for part in parts {
                        match *part {
                            ChordPart::Note(s) => keys.push(self.midi_key(c, octave, s)),
                            ChordPart::Octave(n) => octave = self.var(n),
                            ChordPart::Up => octave += up,
                            ChordPart::Down => octave -= up,
                        }
                    }
                    return keys;
                }
                _ => {}
            }
        }
        Vec::new()
    }

    fn keys_on(&mut self, c: usize, keys: &[i32]) {
        let ch = &self.channels[c];
        match ch.voice {
            Voice::Synth(_) => {
                let velocity = ch.velocity.max(1);
                if let Some(s) = &mut self.synth {
                    for &key in keys {
                        s.note_on(c, key, velocity);
                    }
                }
            }
            Voice::Wave(program) => {
                let Some(w) = self.waves.borrow().get(program) else {
                    return;
                };
                let (envelope, cents) = (ch.envelope, self.cents(c).rem_euclid(100));
                for &key in keys {
                    // As many as the sampler keeps, so that long releases
                    // (and a release of 0, which never ends) cannot pile
                    // up notes: the oldest let go makes way.
                    if self.voices.len() >= sampler::MAX_VOICES {
                        let oldest = self.voices.iter().position(|(.., v)| v.released());
                        self.voices.remove(oldest.unwrap_or(0));
                    }
                    // A 16-bit sample loops between its loop points, the
                    // whole of it by default; BEEP's flag is BEEP's alone.
                    // Drivers such as DAW_MUSIC's give a 128-sample cycle
                    // that way, which played once was a 2 ms click.
                    let v = wave::Voice::new(w.clone(), envelope, key, cents, true);
                    self.voices.push((c, program, v));
                }
            }
            Voice::Silent => {}
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
        let keys: Vec<i32> = keys.into_iter().filter(|k| (0..128).contains(k)).collect();
        // A rest takes the phonemes too.
        let lyric = self.channels[c].lyric.take().filter(|_| !keys.is_empty());
        let gate = (ticks * (self.channels[c].quantize as u32 + 1) / 33).max(1);
        if let Some(letters) = lyric {
            // A sung note starts again whatever joins it to the last; the
            // instrument does not play it.
            let old = std::mem::take(&mut self.channels[c].sounding);
            self.keys_off(c, &old);
            let length = if link == Link::None { gate } else { ticks };
            if !self.sing(c, &letters, keys[0], length) {
                self.keys_on(c, &keys);
            }
            let ch = &mut self.channels[c];
            ch.notes = ch.notes.wrapping_add(1);
            ch.sounding = keys;
        } else if tie && self.channels[c].sounding == keys {
            // A tie: the notes go on.
        } else if tie && !keys.is_empty() && !self.channels[c].sounding.is_empty() {
            // A slur or the end of a portamento: the notes go on at the
            // new keys without striking again.
            let old = std::mem::take(&mut self.channels[c].sounding);
            self.retune(c, &old, &keys);
            self.channels[c].sounding = keys;
        } else {
            let old = std::mem::take(&mut self.channels[c].sounding);
            self.keys_on(c, &keys);
            self.keys_off(c, &old);
            if self.channels[c].glide.is_some() {
                // A glide that did not reach its note: the new notes
                // start at their own key.
                self.porta(c, 0.0);
            }
            let ch = &mut self.channels[c];
            if !keys.is_empty() {
                ch.notes = ch.notes.wrapping_add(1);
            }
            ch.sounding = keys;
        }
        let next = match link {
            Link::Porta => self.next_keys(c, self.channels[c].pc),
            _ => Vec::new(),
        };
        let ch = &mut self.channels[c];
        ch.glide = match (ch.sounding.first(), next.first()) {
            (Some(&from), Some(&to)) if from != to => Some(Glide {
                cents: ((to - from) * 100) as f32,
                ticks: gate,
                done: 0,
            }),
            _ => None,
        };
        ch.link = link;
        ch.wait = ticks;
        ch.gate = match link {
            // (Q+1)/33 of the note, cut to whole ticks: Q32 is all of it.
            Link::None => gate,
            // Held until the next note.
            _ => 0,
        };
    }

    /// Sets @MP's, @MA's or @ML's depth, range, speed and delay.
    fn set_lfo(&mut self, c: usize, target: LfoTarget, v: [Num; 4]) {
        let [depth, range, speed, delay] = v.map(|n| self.var(n).clamp(0, 128));
        let lfo = Lfo {
            depth,
            range,
            speed,
            delay: delay as u32 * LFO_DELAY_UNIT,
        };
        self.channels[c].lfo_set[target as usize] = lfo;
        self.apply_lfo(c, target, true);
    }

    /// Turns an LFO on as set, or off.
    fn apply_lfo(&mut self, c: usize, target: LfoTarget, on: bool) {
        let lfo = if on {
            self.channels[c].lfo_set[target as usize]
        } else {
            Lfo::default()
        };
        self.channels[c].lfo[target as usize] = lfo;
        self.synth(|s| s.lfo(c, target, lfo));
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
                    // The chord's own octave changes end with it.
                    let mut octave = self.channels[c].octave;
                    let mut keys = Vec::new();
                    for part in parts {
                        match *part {
                            ChordPart::Note(s) => keys.push(self.midi_key(c, octave, s)),
                            ChordPart::Octave(n) => octave = self.var(n),
                            ChordPart::Up => octave += up,
                            ChordPart::Down => octave -= up,
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
                Cmd::Program(n) => self.set_program(c, self.var(*n)),
                Cmd::Volume(n) => {
                    let v = self.var(*n).clamp(0, 127);
                    self.channels[c].volume = v;
                    self.synth(|s| s.volume(c, v));
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
                    self.synth(|s| s.volume(c, v));
                }
                Cmd::Velocity(n) => self.channels[c].velocity = self.var(*n).clamp(0, 127),
                Cmd::Pan(n) => {
                    let pan = self.var(*n).clamp(0, 127);
                    self.channels[c].pan = pan;
                    self.synth(|s| s.pan(c, pan));
                }
                Cmd::Gate(n) => self.channels[c].quantize = self.var(*n).clamp(0, 32),
                Cmd::Key(n) => self.channels[c].key = self.var(*n),
                Cmd::SongKey(n) => self.song_key = self.var(*n),
                Cmd::Detune(n) => {
                    self.channels[c].detune = self.var(*n);
                    self.apply_bend(c);
                }
                Cmd::Vibrato(v) => self.set_lfo(c, LfoTarget::Pitch, *v),
                Cmd::Tremolo(v) => self.set_lfo(c, LfoTarget::Volume, *v),
                Cmd::PanWave(v) => self.set_lfo(c, LfoTarget::Pan, *v),
                Cmd::VibratoOff => self.apply_lfo(c, LfoTarget::Pitch, false),
                Cmd::TremoloOff => self.apply_lfo(c, LfoTarget::Volume, false),
                Cmd::PanWaveOff => self.apply_lfo(c, LfoTarget::Pan, false),
                // @MPON, @MAON and @MLON send the last settings of one LFO
                // again; a `:` forgets them, so `@MP64,1,20,0@MPOF:0@MPON`
                // sends the default settings, which do nothing.
                // UNVERIFIED: an LFO that sounds goes on past the `:`.
                Cmd::ForgetLfos => self.channels[c].lfo_set = [Lfo::default(); 3],
                Cmd::VibratoOn => self.apply_lfo(c, LfoTarget::Pitch, true),
                Cmd::TremoloOn => self.apply_lfo(c, LfoTarget::Volume, true),
                Cmd::PanWaveOn => self.apply_lfo(c, LfoTarget::Pan, true),
                // @MON sends the last settings of all three again, and @MOF
                // turns all three off.
                Cmd::ModulationOn | Cmd::ModulationOff => {
                    let on = matches!(cmds[pc], Cmd::ModulationOn);
                    for target in [LfoTarget::Pitch, LfoTarget::Volume, LfoTarget::Pan] {
                        self.apply_lfo(c, target, on);
                    }
                }
                Cmd::Envelope(e) => {
                    let [attack, decay, sustain, release] = e.map(|n| self.var(n));
                    let envelope = Envelope {
                        attack,
                        decay,
                        sustain,
                        release,
                    };
                    self.channels[c].envelope = Some(envelope);
                    self.synth(|s| s.envelope(c, Some(envelope)));
                }
                Cmd::EnvelopeOff => {
                    self.channels[c].envelope = None;
                    self.synth(|s| s.envelope(c, None));
                }
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
                Cmd::Sing(letters) => self.channels[c].lyric = Some(letters.clone()),
            }
        }
    }

    /// One tick: notes whose time is up stop, and channels go on.
    fn tick(&mut self) {
        self.ticks += 1;
        if let Some(p) = &mut self.seq {
            // Without instruments a preset goes on, silent, until stopped.
            if let Some(s) = &mut self.synth {
                p.tick(s.as_mut());
                if p.ended() {
                    self.playing = false;
                }
            }
            return;
        }
        for c in 0..self.channels.len() {
            if let Some(g) = &mut self.channels[c].glide
                && g.done < g.ticks
            {
                g.done += 1;
                let cents = g.cents * g.done as f32 / g.ticks as f32;
                self.porta(c, cents);
            }
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

    /// What the track plays, while it plays or is paused.
    pub(crate) fn inspect(&self, track: usize) -> Option<TrackInfo> {
        if !self.playing {
            return None;
        }
        let state = match (self.after_fade, self.fade.done()) {
            _ if self.hold > 0 => State::Waking,
            _ if self.paused => State::Paused,
            (AfterFade::Stop, _) => State::FadingOut,
            (AfterFade::Pause, _) => State::Pausing,
            (AfterFade::Nothing, false) => State::FadingIn,
            (AfterFade::Nothing, true) => State::Playing,
        };
        let (tempo, quarter, channels) = match (&self.seq, &self.song) {
            (Some(p), _) => (p.tempo, p.timebase, p.channels()),
            (None, Some(song)) => {
                let used = song.used().map(|(c, _)| (c, &self.channels[c]));
                let channels = used.map(|(c, ch)| ChannelInfo {
                    channel: c,
                    program: ch.program,
                    octave: Some(ch.octave),
                    volume: ch.volume,
                    velocity: ch.velocity,
                    pan: ch.pan,
                    keys: ch.sounding.clone(),
                    notes: ch.notes,
                    ended: ch.done,
                });
                (self.tempo, QUARTER, channels.collect())
            }
            (None, None) => (self.tempo, QUARTER, Vec::new()),
        };
        Some(TrackInfo {
            track,
            song: self.number,
            source: self.song.as_ref().map(|s| s.source.clone()),
            silent: self.seq.is_none() && self.song.is_none(),
            state,
            fade: self.fade.value,
            volume: self.volume,
            pitch: self.pitch,
            tempo,
            quarter,
            ticks: self.ticks,
            vars: self.vars,
            channels,
        })
    }

    /// A preset that ends after `frames` but plays silent here, without
    /// its sequence or instruments, still ends then.
    pub(crate) fn end_silent_after(&mut self, frames: u32) {
        if self.seq.is_none() || self.synth.is_none() {
            self.silent_left = Some(frames);
        }
    }

    /// Adds `left.len()` samples of the track to the buffers, and to the
    /// effector's as much as it sends there.
    pub(crate) fn render(
        &mut self,
        left: &mut [f32],
        right: &mut [f32],
        scratch: &mut [Vec<f32>; 2],
        mut aux: Option<(&mut [f32], &mut [f32], f32)>,
    ) {
        self.fresh = Fresh::No;
        self.linger = self.linger.saturating_sub(1);
        if self.hold > 0 {
            self.hold -= 1;
            // A fade out goes on while the song waits to begin.
            if self.after_fade == AfterFade::Stop {
                (0..left.len()).for_each(|_| self.fade.step_sample());
                self.end_fade();
            }
            return;
        }
        if let (Some(n), true, false) = (&mut self.silent_left, self.playing, self.paused) {
            *n = n.saturating_sub(1);
            if *n == 0 {
                self.silent_left = None;
                self.playing = false;
            }
        }
        let n = left.len();
        let [l, r] = scratch;
        l.resize(n, 0.0);
        r.resize(n, 0.0);
        if let Some(p) = &mut self.seq {
            p.begin_block();
        }
        let mut done = 0;
        while done < n {
            if !self.paused && self.playing && self.until_tick <= 0.0 {
                self.tick();
                self.until_tick += match &self.seq {
                    Some(p) => p.tick_samples(RATE),
                    None => f64::from(RATE) * 60.0 / f64::from(self.tempo * QUARTER),
                };
            }
            let chunk = if self.paused || !self.playing {
                n - done
            } else {
                ((self.until_tick.ceil() as usize).max(1)).min(n - done)
            };
            let (cl, cr) = (&mut l[done..done + chunk], &mut r[done..done + chunk]);
            match &mut self.synth {
                Some(s) => s.render(cl, cr),
                None => cl.iter_mut().chain(cr.iter_mut()).for_each(|x| *x = 0.0),
            }
            let waves = self.waves.borrow();
            for (vc, program, v) in &mut self.voices {
                // A waveform WAVSET sets again changes the notes that
                // sound with it, as programs that shape their waveforms
                // each frame expect.
                waves.follow(*program, v);
                let ch = &self.channels[*vc];
                v.set_mix(ch.volume, ch.velocity, ch.pan);
                v.lfo = ch.lfo;
                v.render(cl, cr);
            }
            drop(waves);
            self.voices.retain(|(.., v)| !v.done());
            for (sc, s) in &mut self.singers {
                if s.sounding() {
                    let ch = &self.channels[*sc];
                    let gain =
                        SUNG_GAIN * sampler::square(ch.volume) * sampler::square(ch.velocity);
                    let [gl, gr] = sampler::pan_gains(sampler::pan_position(ch.pan));
                    s.render(cl, cr, (gain * gl, gain * gr));
                }
            }
            if !self.paused && self.playing {
                self.until_tick -= chunk as f64;
            }
            done += chunk;
        }
        // Levels are measured at BGMPLAY's own volume of 64; BGMVOL goes
        // on up to twice that.
        let volume = self.volume as f32 / 64.0 * self.seq.as_ref().map_or(1.0, Player::volume);
        for i in 0..n {
            let gain = volume * self.fade.value as f32;
            let (x, y) = (l[i] * gain, r[i] * gain);
            left[i] += x;
            right[i] += y;
            if let Some((al, ar, send)) = &mut aux {
                al[i] += x * *send;
                ar[i] += y * *send;
            }
            self.fade.step_sample();
        }
        self.end_fade();
    }

    fn end_fade(&mut self) {
        if self.fade.done() {
            match std::mem::replace(&mut self.after_fade, AfterFade::Nothing) {
                AfterFade::Nothing => {}
                // BGMCHK says 0 a frame after the fade ends (checked on
                // hardware).
                AfterFade::Stop => {
                    self.stop();
                    self.linger = 1;
                }
                AfterFade::Pause => {
                    self.paused = true;
                    self.silence();
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sound::FRAME_SAMPLES;
    use crate::sound::mml::parse;

    #[test]
    fn samples_loop_in_songs() {
        // WAVSETA's 16-bit form with its loop over the whole sample, as
        // drivers that build one cycle of 128 samples give it.
        let waves = Rc::new(RefCell::new(Waves::default()));
        waves.borrow_mut().user[0] = Some(Rc::new(wave::Wave {
            samples: (0..128).map(|i| if i < 64 { 0.5 } else { -0.5 }).collect(),
            rate: Some(56_000.0),
            looped: (0, 128),
            beep_loop: false,
            beep_repeats: false,
            ..wave::pulse(219)
        }));
        let mut t = Track::new(None, waves);
        t.play(Some(Rc::new(parse("@224 A1").unwrap())), 127, 0);
        let (mut l, mut r) = (vec![0.0; FRAME_SAMPLES], vec![0.0; FRAME_SAMPLES]);
        let mut scratch = Default::default();
        for _ in 0..30 {
            t.render(&mut l, &mut r, &mut scratch, None);
        }
        l.fill(0.0);
        t.render(&mut l, &mut r, &mut scratch, None);
        assert!(l.iter().any(|&s| s.abs() > 0.01));
    }

    #[test]
    fn waveforms_never_set_play_the_archives_sine() {
        let sounds = |bank: bool| {
            let waves = Rc::new(RefCell::new(Waves::default()));
            if bank {
                let bank = wave::tests::sine_bank();
                waves.borrow_mut().set_bank(Some(std::sync::Arc::new(bank)));
            }
            let mut t = Track::new(None, waves);
            t.play(Some(Rc::new(parse("@250 V127 A1").unwrap())), 127, 0);
            let (mut l, mut r) = (vec![0.0; FRAME_SAMPLES], vec![0.0; FRAME_SAMPLES]);
            let mut scratch = Default::default();
            for _ in 0..30 {
                t.render(&mut l, &mut r, &mut scratch, None);
            }
            l.iter().any(|&s| s.abs() > 0.01)
        };
        assert!(sounds(true));
        assert!(!sounds(false));
    }

    #[test]
    fn portamento_glides_over_the_first_note_and_holds_the_next() {
        let waves = Rc::new(RefCell::new(Waves::default()));
        let mut t = Track::new(None, waves);
        t.play(Some(Rc::new(parse("T120@219O4C2_G2").unwrap())), 127, 0);
        let (mut l, mut r) = (vec![0.0; FRAME_SAMPLES], vec![0.0; FRAME_SAMPLES]);
        let mut scratch = Default::default();
        // Half way through C2, half way to G.
        for _ in 0..30 {
            t.render(&mut l, &mut r, &mut scratch, None);
        }
        assert_eq!(t.voices.len(), 1);
        assert!((t.voices[0].2.porta - 350.0).abs() < 15.0);
        // G2: the same note, at G.
        for _ in 0..60 {
            t.render(&mut l, &mut r, &mut scratch, None);
        }
        assert_eq!(t.voices.len(), 1);
        assert_eq!((t.voices[0].2.key, t.voices[0].2.porta), (67, 0.0));
    }

    /// SmileBASIC 4 plays `@MP64,1,20,0@MPOF:0@MPON` with no vibrato:
    /// the `:` forgot what @MPON sends again.
    #[test]
    fn a_colon_forgets_the_lfo_settings() {
        let vibrato = |mml: &str| {
            let waves = Rc::new(RefCell::new(Waves::default()));
            let mut t = Track::new(None, waves);
            t.play(Some(Rc::new(parse(mml).unwrap())), 127, 0);
            let (mut l, mut r) = (vec![0.0; FRAME_SAMPLES], vec![0.0; FRAME_SAMPLES]);
            let mut scratch = Default::default();
            t.render(&mut l, &mut r, &mut scratch, None);
            t.voices[0].2.lfo[0]
        };
        assert_eq!(vibrato("@219@MP64,1,20,0@MPOF:0@MPONA1").depth, 0);
        assert_eq!(vibrato("@219@MP64,1,20,0@MPOF@MPONA1").depth, 64);
        assert_eq!(vibrato("@219@MP64,1,20,0@MOF:0@MONA1").depth, 0);
        assert_eq!(vibrato(":0@219@MP64,1,20,0@MPOF@MPONA1").depth, 64);
    }

    #[test]
    fn a_slur_goes_on_at_the_new_key_without_striking_again() {
        let waves = Rc::new(RefCell::new(Waves::default()));
        let mut t = Track::new(None, waves);
        t.play(Some(Rc::new(parse("T120@219O4C2&G2").unwrap())), 127, 0);
        let (mut l, mut r) = (vec![0.0; FRAME_SAMPLES], vec![0.0; FRAME_SAMPLES]);
        let mut scratch = Default::default();
        for _ in 0..75 {
            t.render(&mut l, &mut r, &mut scratch, None);
        }
        assert_eq!(t.voices.len(), 1);
        assert_eq!((t.voices[0].2.key, t.voices[0].2.porta), (67, 0.0));
    }

    #[test]
    fn waveform_notes_that_never_end_do_not_pile_up() {
        let waves = Rc::new(RefCell::new(Waves::default()));
        waves.borrow_mut().user[0] = Some(Rc::new(wave::Wave {
            envelope: wave::Envelope {
                release: 0,
                ..wave::Envelope::ORGAN
            },
            ..wave::pulse(219)
        }));
        let mut t = Track::new(None, waves);
        t.play(
            Some(Rc::new(parse("T240 @224 L32 [CDEFGAB]").unwrap())),
            127,
            0,
        );
        let (mut l, mut r) = (vec![0.0; FRAME_SAMPLES], vec![0.0; FRAME_SAMPLES]);
        let mut scratch = Default::default();
        for _ in 0..600 {
            t.render(&mut l, &mut r, &mut scratch, None);
        }
        assert_eq!(t.voices.len(), sampler::MAX_VOICES);
    }

    /// The lag, in samples, at which the signal looks most like itself,
    /// between `from` and `to`.
    fn period(s: &[f32], from: usize, to: usize) -> usize {
        let score = |lag: usize| -> f32 { (0..s.len() - lag).map(|i| s[i] * s[i + lag]).sum() };
        (from..to)
            .max_by(|&a, &b| score(a).total_cmp(&score(b)))
            .unwrap()
    }

    /// `*A,` sings: without instruments the note is heard all the same,
    /// at the note's pitch, and it ends with the note.
    #[test]
    fn sung_notes_sing_at_their_pitch() {
        let waves = Rc::new(RefCell::new(Waves::default()));
        let mut scratch = Default::default();
        let (mut l, mut r) = (vec![0.0; FRAME_SAMPLES], vec![0.0; FRAME_SAMPLES]);
        for (mml, hz) in [("T120O3*A,A2", 220.0), ("T120O4*KA,A2", 440.0)] {
            let mut t = Track::new(None, waves.clone());
            t.play(Some(Rc::new(parse(mml).unwrap())), 127, 0);
            let mut sound = Vec::new();
            for _ in 0..50 {
                l.fill(0.0);
                t.render(&mut l, &mut r, &mut scratch, None);
                sound.extend_from_slice(&l);
            }
            let late = &sound[RATE as usize / 2..];
            assert!(late.iter().any(|s| s.abs() > 0.02), "{mml} is heard");
            let expected = (RATE as f32 / hz) as usize;
            let p = period(&late[..4096], expected / 2 + 10, expected * 3 / 2);
            assert!(p.abs_diff(expected) <= 2, "{mml}: {p} for {expected}");
            // A half note at T120 is a second.
            for _ in 0..30 {
                l.fill(0.0);
                t.render(&mut l, &mut r, &mut scratch, None);
            }
            assert!(l.iter().all(|s| s.abs() < 1e-3), "{mml} ends");
        }
        // Without the phonemes, there is no instrument to hear.
        let mut t = Track::new(None, waves);
        t.play(Some(Rc::new(parse("T120O4A2").unwrap())), 127, 0);
        l.fill(0.0);
        for _ in 0..20 {
            t.render(&mut l, &mut r, &mut scratch, None);
        }
        assert!(l.iter().all(|&s| s == 0.0));
    }
}
