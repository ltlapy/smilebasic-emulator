//! Sound: BGM tracks playing MML on a SoundFont, mixed a frame at a time
//! into stereo samples for the frontend to play.

pub mod bank;
pub mod beep;
pub mod inspect;
pub mod mml;
pub mod output;
pub mod pcm;
pub(crate) mod reverb;
pub mod sampler;
pub mod seq;
pub mod synth;
pub mod talk;
mod track;
pub mod wave;

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

pub use rustysynth::SoundFont;

use beep::Beeps;
use inspect::{BeepInfo, TrackInfo};
use mml::Song;
use pcm::Pcm;
use seq::{Kind, Sequences};
use synth::Instruments;
use talk::Talk;
use track::{AfterFade, Fresh, Track};
use wave::Waves;

/// Samples a second.
pub const RATE: u32 = 48_000;
/// Samples in one frame at 60 frames a second.
pub const FRAME_SAMPLES: usize = (RATE / 60) as usize;
/// BGM tracks 0 to 15.
pub const TRACKS: usize = 16;
/// Frames a song started on an idle sound system waits.
const WAKE_FRAMES: u32 = 2;
/// The highest preset song number: 4.4.5 added 62 to 73, and 46 and up
/// play on hardware.
pub const PRESET_SONGS: i32 = 73;
/// The highest preset sound effect: 4.4.5 added 156.
pub const PRESET_BEEPS: i32 = 156;

/// A gain moving to a target in a straight line, a sample at a time.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Ramp {
    pub(crate) value: f64,
    target: f64,
    step: f64,
}

impl Ramp {
    pub(crate) fn at(value: f64) -> Ramp {
        Ramp {
            value,
            target: value,
            step: 0.0,
        }
    }

    /// Moves to `target` over `frames` frames, at once for 0.
    pub(crate) fn to(&mut self, target: f64, frames: u32) {
        self.target = target;
        if frames == 0 {
            self.value = target;
            self.step = 0.0;
        } else {
            self.step = (target - self.value) / (f64::from(frames) * FRAME_SAMPLES as f64);
        }
    }

    pub(crate) fn step_sample(&mut self) {
        if self.step != 0.0 {
            self.value += self.step;
            if (self.step > 0.0 && self.value >= self.target)
                || (self.step < 0.0 && self.value <= self.target)
            {
                self.value = self.target;
                self.step = 0.0;
            }
        }
    }

    pub(crate) fn done(&self) -> bool {
        self.step == 0.0
    }
}

/// The furthest a sample going into the effector may reach: well beyond
/// full scale, which only a sum that would clip anyway comes near.
const MOST_SENT: f32 = 8.0;

/// Silences samples that are not numbers, which `clamp` lets through.
/// Looking first keeps this out of the way of the mix's own loops.
#[inline(never)]
fn silence_nans(samples: &mut [f32]) {
    if samples.iter().any(|s| s.is_nan()) {
        samples
            .iter_mut()
            .filter(|s| s.is_nan())
            .for_each(|s| *s = 0.0);
    }
}

/// Seconds, as BGMSTOP and friends take them, in frames.
pub(crate) fn frames(seconds: f64) -> u32 {
    (seconds * 60.0).round().clamp(0.0, f64::from(u32::MAX)) as u32
}

/// SNDMVOL's volume for a gain of 1, which it starts at and goes back to:
/// 0.7 of 127, which SNDMVOL() reads as 88. The output is linear in the
/// volume, so 127 plays at 1.43 times the start's level.
const MASTER_UNITY: f64 = 0.7 * 127.0;

/// RECSTART's recording of what plays, as 16-bit samples.
#[derive(Clone, Debug, Default)]
pub(crate) struct Recorder {
    /// The buffer, which RECSTART leaves as it was: what a recording has
    /// not reached yet holds older recordings. It only grows.
    buffer: Vec<[i16; 2]>,
    /// The samples this recording takes, from the buffer's start.
    len: usize,
    pub(crate) pos: usize,
    pub(crate) recording: bool,
    /// Without a time, recording goes round the buffer until RECSTOP.
    pub(crate) ring: bool,
    /// Whether it has taken a sample yet.
    pub(crate) begun: bool,
    /// Where the last frame's samples went in the buffer and how many it
    /// took, for [`Sound::hear_sub`].
    frame_start: usize,
    taken: usize,
}

impl Recorder {
    /// RECSTART: `len` samples, round and round with `ring`.
    pub(crate) fn start(&mut self, len: usize, ring: bool) {
        if self.buffer.len() < len {
            self.buffer.resize(len, [0, 0]);
        }
        self.len = len;
        self.pos = 0;
        self.recording = true;
        self.ring = ring;
        self.begun = false;
    }

    /// The samples RECSTART asked for, the older ones in what this
    /// recording has not reached.
    pub(crate) fn samples(&self) -> &[[i16; 2]] {
        &self.buffer[..self.len]
    }

    fn record(&mut self, left: f32, right: f32) {
        if !self.recording || self.len == 0 {
            return;
        }
        self.begun = true;
        let s = |x: f32| (x * 32767.0).round() as i16;
        self.buffer[self.pos] = [s(left), s(right)];
        self.pos += 1;
        self.taken += 1;
        if self.pos == self.len {
            if self.ring {
                self.pos = 0;
            } else {
                self.recording = false;
            }
        }
    }
}

/// The main and the sub interpreter's gains for a balance SNDMSBAL set:
/// b/127 and (127-b)/127 of the output, twice over, so that halfway, where
/// each subprogram starts, both play as loud as the main program alone.
pub(crate) fn balance_gains(balance: i32) -> (f32, f32) {
    let b = balance.clamp(0, 127) as f32;
    (2.0 * b / 127.0, 2.0 * (127.0 - b) / 127.0)
}

/// SNDMSBAL's balance, which moves to a new one in steps of a frame
/// when it is given a time.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Balance {
    /// Where it is, 0 to 127. A subprogram starts halfway, at 63.5, which
    /// SNDMSBAL reads as 63; the manual calls 64 even.
    at: f32,
    /// The main and the sub interpreter's gains there.
    gains: (f32, f32),
    /// Where it goes, and the frames left to get there.
    target: f32,
    target_gains: (f32, f32),
    left: u32,
}

impl Balance {
    /// Halfway, where the subprogram's sound and the main program's are
    /// each as loud as the main program's alone.
    pub(crate) const START: Balance = Balance {
        at: 63.5,
        gains: (1.0, 1.0),
        target: 63.5,
        target_gains: (1.0, 1.0),
        left: 0,
    };

    /// SNDMSBAL: moves to `balance` over `frames` frames, at once for 0.
    pub(crate) fn to(&mut self, balance: i32, frames: u32) {
        self.target = balance as f32;
        self.target_gains = balance_gains(balance);
        self.left = frames;
        if frames == 0 {
            self.at = self.target;
            self.gains = self.target_gains;
        }
    }

    /// One frame's step of the move: the same share of what is left each
    /// frame, which moves in a straight line.
    fn step_frame(&mut self) {
        if self.left == 0 {
            return;
        }
        let k = 1.0 / self.left as f32;
        self.left -= 1;
        if self.left == 0 {
            self.at = self.target;
            self.gains = self.target_gains;
        } else {
            self.at += (self.target - self.at) * k;
            self.gains.0 += (self.target_gains.0 - self.gains.0) * k;
            self.gains.1 += (self.target_gains.1 - self.gains.1) * k;
        }
    }

    /// What SNDMSBAL reads: where it is now, rounded down.
    pub(crate) fn read(&self) -> i32 {
        self.at as i32
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Effector {
    pub(crate) on: bool,
    pub(crate) preset: i32,
    /// BEEP, BGM, TALK and PCM's wetness.
    pub(crate) wet: [i32; 4],
}

/// EFCWET's sends, in its order.
const BEEP_SEND: usize = 0;
const BGM_SEND: usize = 1;
const TALK_SEND: usize = 2;
const PCM_SEND: usize = 3;

pub struct Sound {
    instruments: Option<Instruments>,
    /// The preset songs and sound effects, when imported.
    sequences: Option<Arc<Sequences>>,
    tracks: Vec<Option<Track>>,
    /// BGMSET's songs, 128 to 255.
    pub(crate) songs: HashMap<i32, Rc<Song>>,
    /// The PSG and WAVSET's waveforms, shared with the tracks.
    pub(crate) waves: Rc<RefCell<Waves>>,
    pub(crate) beeps: Beeps,
    pub(crate) pcm: Pcm,
    pub(crate) talk: Talk,
    /// EFCEN, EFCSET and EFCWET's settings.
    pub(crate) effector: Effector,
    /// EFCSET's reverb, none for #EFCOFF.
    reverb: Option<reverb::Reverb>,
    /// What the sounds send to the effector this frame.
    aux: [Vec<f32>; 2],
    /// One kind of sound at a time, to send part of it.
    part: [Vec<f32>; 2],
    /// PCMSTREAM's samples this frame, kept out of a recording.
    pcm_part: [Vec<f32>; 2],
    /// RECSTART's recording: the sub interpreter shares the main one's.
    pub(crate) recorder: Rc<RefCell<Recorder>>,
    /// This is the sub interpreter's sound, which never records.
    pub(crate) sub: bool,
    /// SNDMSBAL, which only subprograms set. The main and the sub
    /// interpreter share one from the sub's first frame on.
    balance: Rc<Cell<Balance>>,
    /// SNDMVOL, as a gain: 1 at its start, which SNDMVOL() reads as 88.
    /// The sub interpreter's follows the main one's.
    pub(crate) master: Ramp,
    /// Mixed samples, left and right in turn, not yet taken.
    out: Vec<f32>,
    scratch: [Vec<f32>; 2],
    mix: [Vec<f32>; 2],
    /// Each track's MML variables as the program sees them: as they were
    /// when the last frame's sound was made.
    shown: [[i32; 8]; TRACKS],
}

impl Default for Sound {
    fn default() -> Self {
        Sound {
            instruments: None,
            sequences: None,
            tracks: (0..TRACKS).map(|_| None).collect(),
            songs: HashMap::new(),
            waves: Rc::default(),
            beeps: Beeps::new(None),
            pcm: Pcm::default(),
            talk: Talk::default(),
            effector: Effector::default(),
            reverb: None,
            aux: Default::default(),
            part: Default::default(),
            pcm_part: Default::default(),
            balance: Rc::new(Cell::new(Balance::START)),
            recorder: Rc::default(),
            sub: false,
            master: Ramp::at(1.0),
            out: Vec::new(),
            scratch: Default::default(),
            mix: Default::default(),
            shown: [[-1; 8]; TRACKS],
        }
    }
}

impl Sound {
    /// Where instruments come from; without any, BGM is silent.
    pub fn set_instruments(&mut self, instruments: Option<Instruments>) {
        self.beeps = Beeps::new(instruments.as_ref());
        self.waves.borrow_mut().set_bank(match &instruments {
            Some(Instruments::Bank(bank)) => Some(bank.clone()),
            _ => None,
        });
        self.instruments = instruments;
        self.tracks.iter_mut().for_each(|t| *t = None);
    }

    pub(crate) fn instruments(&self) -> Option<&Instruments> {
        self.instruments.as_ref()
    }

    pub(crate) fn sequences(&self) -> Option<Arc<Sequences>> {
        self.sequences.clone()
    }

    pub fn set_sequences(&mut self, sequences: Option<Arc<Sequences>>) {
        self.sequences = sequences;
    }

    /// SmileBASIC 4's own preset song or effect `n`, if imported.
    pub(crate) fn sequence(&self, kind: Kind, n: i32) -> Option<Arc<seq::Sequence>> {
        self.sequences.as_ref()?.get(kind, n)
    }

    pub(crate) fn track(&mut self, n: usize) -> &mut Track {
        let instruments = self.instruments.as_ref();
        let waves = &self.waves;
        self.tracks[n].get_or_insert_with(|| Track::new(instruments, waves.clone()))
    }

    /// The tracks that exist, which are all that can be playing.
    pub(crate) fn tracks(&mut self) -> impl Iterator<Item = &mut Track> {
        self.tracks.iter_mut().flatten()
    }

    /// A VSYNC that went straight on.
    pub fn synced(&mut self) {
        for t in self.tracks.iter_mut().flatten() {
            if t.fresh == Fresh::Started {
                t.fresh = Fresh::Synced;
            }
        }
    }

    pub(crate) fn playing(&self, n: usize) -> bool {
        self.tracks[n]
            .as_ref()
            .is_some_and(|t| t.playing || t.linger > 0)
    }

    pub(crate) fn play(&mut self, n: usize, song: Option<Rc<Song>>, volume: i32, fade: u32) {
        self.track(n).play(song, volume, fade);
    }

    pub(crate) fn stop(&mut self, n: usize, fade: u32) {
        if let Some(t) = self.tracks[n].as_mut() {
            // Stopped with a fade in the frame it started in, a song
            // never sounds. BGMCHK says 0 a frame on, or two after a
            // VSYNC that did not wait (checked on hardware).
            if fade == 0 || t.fresh != Fresh::No {
                let synced = t.fresh == Fresh::Synced;
                t.stop();
                if fade > 0 && synced {
                    t.linger = 2;
                }
            } else {
                t.fade.to(0.0, fade);
                t.after_fade = AfterFade::Stop;
            }
        }
    }

    /// BGMPLAY of a user song with nothing set: the track stops, and
    /// BGMCHK says 1 for two more frames.
    pub(crate) fn stop_for_nothing(&mut self, n: usize) {
        if let Some(t) = self.tracks[n].as_mut().filter(|t| t.playing) {
            t.stop();
            t.linger = 3;
        }
    }

    pub(crate) fn pause(&mut self, n: usize, fade: u32) {
        if let Some(t) = self.tracks[n].as_mut().filter(|t| t.playing && !t.paused) {
            t.fade.to(0.0, fade);
            t.after_fade = AfterFade::Pause;
            if fade == 0 {
                t.paused = true;
            }
        }
    }

    pub(crate) fn resume(&mut self, n: usize, fade: u32) {
        if let Some(t) = self.tracks[n].as_mut().filter(|t| t.paused) {
            t.paused = false;
            t.after_fade = AfterFade::Nothing;
            t.fade.to(1.0, fade);
        }
    }

    /// BGMVAR: a track's MML variable, -1 while the track is not playing
    /// (checked on hardware: before any song, after BGMSTOP and after the
    /// song ended). It is read as the sound was at the start of the frame,
    /// so values set by the song, by BGMVAR or by stopping show a frame
    /// later, as on hardware.
    pub(crate) fn var(&self, n: usize, var: usize) -> i32 {
        self.shown[n][var]
    }

    /// What the tracks that play or are paused play, for the debugger.
    pub fn inspect_tracks(&self) -> Vec<TrackInfo> {
        (self.tracks.iter().enumerate())
            .filter_map(|(n, t)| t.as_ref()?.inspect(n))
            .collect()
    }

    /// The BEEP handles that sound, for the debugger.
    pub fn inspect_beeps(&self) -> Vec<BeepInfo> {
        self.beeps.inspect()
    }

    /// Whether no BGM track plays.
    pub(crate) fn idle(&self) -> bool {
        self.tracks.iter().flatten().all(|t| !t.playing)
    }

    /// Holds a song just started on an idle sound system back for the
    /// frames it takes to wake up: on hardware a song's first variable
    /// showed four frames after the first BGMPLAY of a run, but within
    /// two for later ones. Any BGMPLAY to an idle system waits, the first
    /// of a run or not, and its sound waits with it; the program does not.
    /// On hardware the sound began about 1276 samples after the call, 1.6
    /// frames, where here it waits two whole frames: the variables' timing
    /// above, which rhythm games follow, matches with two.
    pub(crate) fn wake(&mut self, n: usize) {
        self.track(n).hold = WAKE_FRAMES;
    }

    /// BGMVAR setting a variable, which a track that is not playing
    /// ignores (checked on hardware).
    pub(crate) fn set_var(&mut self, n: usize, var: usize, value: i32) {
        let t = self.track(n);
        if t.playing {
            t.vars[var] = value;
        }
    }

    /// SNDMVOL: moves to `volume` over `frames` frames, at once for 0.
    pub(crate) fn set_master(&mut self, volume: i32, frames: u32) {
        self.master.to(f64::from(volume) / MASTER_UNITY, frames);
    }

    /// What SNDMVOL() reads: the volume now, rounded down.
    pub(crate) fn master_volume(&self) -> i32 {
        (self.master.value * MASTER_UNITY + 1e-6).floor() as i32
    }

    /// RUN puts SNDMVOL back at its start.
    pub(crate) fn run_started(&mut self) {
        self.master = Ramp::at(1.0);
    }

    /// SNDSTOP's reset of the mixer: SNDMVOL and the shared SNDMSBAL go
    /// back to their start.
    // UNVERIFIED: that a subprogram's SNDSTOP resets the balance too.
    pub(crate) fn reset_mixer(&mut self) {
        self.master = Ramp::at(1.0);
        self.balance.set(Balance::START);
    }

    /// SNDMSBAL, which does nothing in the main program.
    pub(crate) fn set_balance(&mut self, balance: i32, frames: u32) {
        if self.sub {
            let mut b = self.balance.get();
            b.to(balance, frames);
            self.balance.set(b);
        }
    }

    /// What SNDMSBAL() reads, the same in either program.
    pub(crate) fn balance(&self) -> i32 {
        self.balance.get().read()
    }

    /// SNDSTOP: every sound stops at once.
    pub fn stop_all(&mut self) {
        for t in self.tracks() {
            t.stop();
        }
        self.beeps.stop_all();
        self.pcm.playing = false;
        self.talk.stop();
    }

    /// The main and the sub interpreter's gains for this subprogram's
    /// balance.
    pub(crate) fn balance_gains(&self) -> (f32, f32) {
        self.balance.get().gains
    }

    /// A subprogram starts: the balance goes back to halfway.
    pub fn sub_started(&mut self) {
        self.balance.set(Balance::START);
    }

    /// EFCSET: the effector's reverb, or none.
    pub(crate) fn set_reverb(&mut self, settings: Option<reverb::Settings>) {
        if self.reverb.as_ref().map(|r| r.settings()) != settings {
            self.reverb = settings.map(reverb::Reverb::new);
        }
    }

    /// Mixes one frame of samples.
    pub fn render_frame(&mut self) {
        // The sub interpreter moves the shared balance, once a frame.
        if self.sub {
            let mut b = self.balance.get();
            b.step_frame();
            self.balance.set(b);
        }
        let n = FRAME_SAMPLES;
        let [left, right] = &mut self.mix;
        left.clear();
        left.resize(n, 0.0);
        right.clear();
        right.resize(n, 0.0);
        // With the effector on, each kind of sound sends part of itself to
        // it, as EFCWET (or a track's BGMWET) asks.
        let effect = self.effector.on && self.reverb.is_some();
        let send = |n: usize| self.effector.wet[n] as f32 / 127.0;
        let sends = self.effector.wet.map(|_| 0.0);
        let sends = if effect {
            [BEEP_SEND, BGM_SEND, TALK_SEND, PCM_SEND].map(send)
        } else {
            sends
        };
        let [aux_l, aux_r] = &mut self.aux;
        aux_l.clear();
        aux_r.clear();
        if effect {
            aux_l.resize(n, 0.0);
            aux_r.resize(n, 0.0);
        }
        // A recording not yet begun waits with a song that waits for the
        // sound system to wake, and so begins with it. The sub
        // interpreter's sound reaches the main one's recording through
        // `hear_sub`.
        let record = !self.sub && {
            let waking = self.tracks.iter().flatten().any(|t| t.hold > 0);
            let mut rec = self.recorder.borrow_mut();
            rec.frame_start = rec.pos;
            rec.taken = 0;
            rec.recording && (rec.begun || !waking)
        };
        // RECSTART does not hear PCMSTREAM, which goes in after the
        // recording, still sending to the effector.
        let pcm_apart = record && self.pcm.playing;
        if pcm_apart {
            let [l, r] = &mut self.pcm_part;
            l.clear();
            l.resize(n, 0.0);
            r.clear();
            r.resize(n, 0.0);
            self.pcm.render(l, r);
            let s = sends[PCM_SEND];
            if s != 0.0 {
                for i in 0..n {
                    aux_l[i] += l[i] * s;
                    aux_r[i] += r[i] * s;
                }
            }
        }
        for t in self.tracks.iter_mut().flatten() {
            // UNVERIFIED: that BGMWET takes the place of EFCWET's BGM for
            // its track once set.
            let s = match t.wet {
                Some(w) if effect => w as f32 / 127.0,
                _ => sends[BGM_SEND],
            };
            let aux = (s > 0.0).then_some((aux_l.as_mut_slice(), aux_r.as_mut_slice(), s));
            t.render(left, right, &mut self.scratch, aux);
        }
        let [part_l, part_r] = &mut self.part;
        let mut mix = |s: f32, render: &mut dyn FnMut(&mut [f32], &mut [f32])| {
            if s == 0.0 {
                render(left, right);
                return;
            }
            part_l.clear();
            part_l.resize(n, 0.0);
            part_r.clear();
            part_r.resize(n, 0.0);
            render(part_l, part_r);
            for i in 0..n {
                left[i] += part_l[i];
                right[i] += part_r[i];
                aux_l[i] += part_l[i] * s;
                aux_r[i] += part_r[i] * s;
            }
        };
        mix(sends[BEEP_SEND], &mut |l, r| self.beeps.render(l, r));
        if !pcm_apart {
            mix(sends[PCM_SEND], &mut |l, r| self.pcm.render(l, r));
        }
        mix(sends[TALK_SEND], &mut |l, r| self.talk.render(l, r));
        // What the effector adds: RECSTART records the sound before it.
        if let Some(r) = self.reverb.as_mut().filter(|_| effect) {
            // A tail remembers what goes in: one bad sample would ring on.
            for aux in [&mut *aux_l, &mut *aux_r] {
                silence_nans(aux);
                aux.iter_mut()
                    .for_each(|s| *s = s.clamp(-MOST_SENT, MOST_SENT));
            }
            r.process(aux_l, aux_r);
        }
        for (i, shown) in self.shown.iter_mut().enumerate() {
            *shown = match &self.tracks[i] {
                Some(t) if t.playing => t.vars,
                _ => [-1; 8],
            };
        }
        // Nobody is taking them, as in tests: keep a second at most.
        let most = 2 * RATE as usize;
        if self.out.len() > most {
            self.out.drain(..self.out.len() - most);
        }
        self.out.reserve(2 * n);
        let start = self.out.len();
        let [aux_l, aux_r] = &self.aux;
        let [pcm_l, pcm_r] = &self.pcm_part;
        let mut rec = self.recorder.borrow_mut();
        for i in 0..n {
            let gain = self.master.value as f32;
            let (mut l, mut r) = (left[i] * gain, right[i] * gain);
            if record {
                rec.record(l.clamp(-1.0, 1.0), r.clamp(-1.0, 1.0));
            }
            if pcm_apart {
                l += pcm_l[i] * gain;
                r += pcm_r[i] * gain;
            }
            let (l, r) = match (aux_l.get(i), aux_r.get(i)) {
                (Some(al), Some(ar)) => (l + al * gain, r + ar * gain),
                _ => (l, r),
            };
            self.out.push(l.clamp(-1.0, 1.0));
            self.out.push(r.clamp(-1.0, 1.0));
            self.master.step_sample();
        }
        silence_nans(&mut self.out[start..]);
    }

    /// What the sub interpreter mixed this frame, which RECSTART in the
    /// main program hears with the main program's own sound, as the
    /// subprogram's SNDMSBAL balances them. The sub interpreter shares the
    /// main one's recorder: its RECSAVE reads what the main program
    /// recorded last.
    pub(crate) fn hear_sub(&mut self, sub: &mut Sound) {
        if !Rc::ptr_eq(&sub.recorder, &self.recorder) {
            sub.recorder = Rc::clone(&self.recorder);
            sub.sub = true;
            self.balance.set(sub.balance.get());
            sub.balance = Rc::clone(&self.balance);
        }
        let rec = &mut *self.recorder.borrow_mut();
        if rec.taken == 0 {
            return;
        }
        let (own, theirs) = sub.balance_gains();
        let theirs = theirs * sub.master.value as f32;
        let [left, right] = &sub.mix;
        let mixed = |s: i16, t: Option<&f32>| {
            let x = f32::from(s) / 32767.0 * own + t.copied().unwrap_or(0.0) * theirs;
            (x.clamp(-1.0, 1.0) * 32767.0).round() as i16
        };
        for i in 0..rec.taken {
            let p = (rec.frame_start + i) % rec.len;
            let [l, r] = rec.buffer[p];
            rec.buffer[p] = [mixed(l, left.get(i)), mixed(r, right.get(i))];
        }
    }

    /// The samples mixed since the last call, left and right in turn.
    pub fn take_samples(&mut self) -> Vec<f32> {
        std::mem::take(&mut self.out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The main program's recording hears the subprogram's sound too, both
    /// as SNDMSBAL balances them.
    #[test]
    fn recording_hears_the_subprogram_as_sndmsbal_balances_it() {
        let heard = |balance: i32| {
            let (mut main, mut sub) = (Sound::default(), Sound::default());
            sub.sub = true;
            sub.set_balance(balance, 0);
            main.recorder.borrow_mut().start(FRAME_SAMPLES, false);
            main.render_frame();
            sub.render_frame();
            sub.mix = [vec![0.25; FRAME_SAMPLES], vec![-0.25; FRAME_SAMPLES]];
            main.hear_sub(&mut sub);
            main.recorder.borrow().samples()[FRAME_SAMPLES - 1]
        };
        assert_eq!(heard(0), [16384, -16384]);
        assert_eq!(heard(127), [0, 0]);
        let [l, r] = heard(64);
        assert!((l - 8127).abs() <= 1 && (r + 8127).abs() <= 1, "{l} {r}");
        assert_eq!(balance_gains(127), (2.0, 0.0));
        assert_eq!(Balance::START.gains, (1.0, 1.0));
    }

    /// The main program's SNDMSBAL does nothing, and both programs read
    /// the subprogram's balance once it has had a frame; SNDSTOP in either
    /// puts it back halfway, and SNDMVOL back at 88.
    #[test]
    fn the_balance_is_shared_and_sndstop_resets_the_mixer() {
        let (mut main, mut sub) = (Sound::default(), Sound::default());
        main.set_balance(127, 0);
        assert_eq!(main.balance(), 63);
        main.render_frame();
        sub.render_frame();
        main.hear_sub(&mut sub);
        sub.set_balance(127, 0);
        assert_eq!((main.balance(), sub.balance()), (127, 127));
        main.set_master(32, 0);
        assert_eq!(main.master_volume(), 32);
        main.reset_mixer();
        assert_eq!((main.balance(), sub.balance()), (63, 63));
        assert_eq!(main.master_volume(), 88);
        main.set_master(127, 0);
        assert_eq!(main.master_volume(), 127);
        assert!((main.master.value - 127.0 / 88.9).abs() < 1e-9);
    }

    /// The sub interpreter shares the main one's recorder and records
    /// nothing into it itself.
    #[test]
    fn the_subprogram_records_nothing_of_its_own() {
        let (mut main, mut sub) = (Sound::default(), Sound::default());
        main.recorder.borrow_mut().start(3 * FRAME_SAMPLES, false);
        main.render_frame();
        sub.render_frame();
        main.hear_sub(&mut sub);
        assert!(sub.sub && Rc::ptr_eq(&sub.recorder, &main.recorder));
        sub.render_frame();
        assert_eq!(main.recorder.borrow().pos, FRAME_SAMPLES);
        main.render_frame();
        assert_eq!(main.recorder.borrow().pos, 2 * FRAME_SAMPLES);
    }
}
