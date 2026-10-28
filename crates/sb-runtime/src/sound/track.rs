//! One BGM track: a song's channels stepped through tick by tick, played
//! on a synthesizer of the track's own.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use super::mml::{ChordPart, Cmd, Link, Num, Pitch, Song, WHOLE, ticks};
use super::seq::{Player, Sequence};
use super::synth::{Instruments, Synth};
use super::wave::{self, Envelope, Waves};
use super::{RATE, Ramp};

/// Ticks in a quarter note; T counts quarter notes a minute.
const QUARTER: u32 = WHOLE / 4;
/// Commands a channel may run on one tick before it is taken to be stuck
/// in a loop that plays nothing.
const COMMANDS_PER_TICK: u32 = 10_000;
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
            pan: 64,
            envelope: None,
            voice: Voice::Synth(0),
            sounding: Vec::new(),
            link: Link::None,
            done: false,
        }
    }
}

pub(crate) struct Track {
    /// None without instruments: the song goes on, silent.
    synth: Option<Box<dyn Synth>>,
    waves: Rc<RefCell<Waves>>,
    /// Waveform notes sounding, with their channels.
    voices: Vec<(usize, wave::Voice)>,
    song: Option<Rc<Song>>,
    /// A preset song, played instead of `song`.
    seq: Option<Player>,
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
    pub(crate) fn new(instruments: Option<&Instruments>, waves: Rc<RefCell<Waves>>) -> Track {
        Track {
            synth: instruments.map(Instruments::synth),
            waves,
            voices: Vec::new(),
            song: None,
            seq: None,
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
        self.start(volume, fade_frames);
        self.song = song;
        for c in 0..self.channels.len() {
            self.apply_voice(c);
            if let Some(s) = &mut self.synth {
                s.volume(c, 100);
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
        self.channels = vec![Channel::default(); super::mml::CHANNELS];
        self.tempo = 120;
        self.song_key = 0;
        self.vars = [0; 8];
        self.until_tick = 0.0;
        self.song = None;
        self.seq = None;
        self.playing = true;
        self.paused = false;
        self.volume = volume;
        self.fade = Ramp::at(if fade_frames > 0 { 0.0 } else { 1.0 });
        self.fade.to(1.0, fade_frames);
        self.after_fade = AfterFade::Nothing;
    }

    pub(crate) fn stop(&mut self) {
        self.playing = false;
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
    }

    /// Runs `f` on the synthesizer, if there is one.
    fn synth(&mut self, f: impl FnOnce(&mut dyn Synth)) {
        if let Some(s) = &mut self.synth {
            f(s.as_mut());
        }
    }

    fn set_program(&mut self, c: usize, program: i32) {
        self.channels[c].voice = match program {
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
        for (_, v) in self.voices.iter_mut().filter(|(vc, _)| *vc == c) {
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
        for &key in keys {
            if let Some(s) = &mut self.synth {
                s.note_off(c, key);
            }
        }
        for (_, v) in self
            .voices
            .iter_mut()
            .filter(|(vc, v)| *vc == c && keys.contains(&v.key))
        {
            v.release();
        }
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
                    let v = wave::Voice::new(w.clone(), envelope, key, cents, false);
                    self.voices.push((c, v));
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
        if tie && self.channels[c].sounding == keys {
            // A tie: the notes go on.
        } else {
            let old = std::mem::take(&mut self.channels[c].sounding);
            let keys: Vec<i32> = keys.into_iter().filter(|k| (0..128).contains(k)).collect();
            // A slur starts the new notes before ending the old.
            self.keys_on(c, &keys);
            self.keys_off(c, &old);
            self.channels[c].sounding = keys;
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
                // The synthesizer's vibrato, through the modulation wheel.
                // UNVERIFIED: how depth, range, speed and delay map.
                Cmd::Vibrato([depth, ..]) => {
                    let depth = self.var(*depth).clamp(0, 127);
                    self.synth(|s| s.modulation(c, depth));
                }
                Cmd::VibratoOff | Cmd::ModulationOff => self.synth(|s| s.modulation(c, 0)),
                Cmd::Envelope(e) => {
                    let [attack, decay, sustain, release] = e.map(|n| self.var(n));
                    self.channels[c].envelope = Some(Envelope {
                        attack,
                        decay,
                        sustain,
                        release,
                    });
                }
                Cmd::EnvelopeOff => self.channels[c].envelope = None,
                // UNVERIFIED: envelopes only shape waveforms, not the
                // synthesizer's instruments; tremolo and pan modulation are read but
                // not played.
                Cmd::Tremolo(_) | Cmd::TremoloOff | Cmd::PanWave(_) | Cmd::PanWaveOff => {}
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
            for (vc, v) in &mut self.voices {
                let ch = &self.channels[*vc];
                v.gains = wave::gains(ch.volume, ch.velocity, ch.pan);
                v.render(cl, cr);
            }
            self.voices.retain(|(_, v)| !v.done());
            if !self.paused && self.playing {
                self.until_tick -= chunk as f64;
            }
            done += chunk;
        }
        let volume = self.volume as f32 / 127.0 * self.seq.as_ref().map_or(1.0, Player::volume);
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
                    self.silence();
                }
            }
        }
    }
}
