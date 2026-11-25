//! Sound: BGM tracks playing MML on a SoundFont, mixed one frame at a time
//! into stereo samples for the frontend to play.

pub mod bank;
pub mod beep;
pub(crate) mod decoded;
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
use decoded::{Decoded, Shared};
use inspect::{BeepInfo, TrackInfo};
use mml::Song;
use pcm::Pcm;
use seq::{Kind, Sequences};
use synth::Instruments;
use talk::Talk;
use track::{AfterFade, Fresh, Track};
use wave::Waves;

/// Samples per second.
pub const RATE: u32 = 48_000;
/// Samples in one frame at 60 frames per second.
pub const FRAME_SAMPLES: usize = (RATE / 60) as usize;
/// BGM tracks 0 to 15.
pub const TRACKS: usize = 16;
/// Frames a song started on an idle sound system waits.
const WAKE_FRAMES: u32 = 2;
/// [`Track::linger`] for a song stopped with a fade in the frame it started
/// in, after a VSYNC that did not wait.
const LINGER_AFTER_SYNCED_FADE: u32 = 2;
/// [`Track::linger`] after BGMPLAY of a user song with nothing set.
const LINGER_AFTER_EMPTY_SONG: u32 = 3;
/// The most mixed samples kept when nobody takes them, as in tests: one
/// second, left and right.
const MOST_UNTAKEN: usize = 2 * RATE as usize;
/// The highest preset song number: 4.4.5 added 62 to 73, and 46 and up
/// play on hardware.
pub const PRESET_SONGS: i32 = 73;
/// The highest preset sound effect: 4.4.5 added 156.
pub const PRESET_BEEPS: i32 = 156;

/// A gain moving to a target in a straight line, one sample at a time.
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

    /// Moves to `target` over `frames` frames, or immediately for 0.
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

/// A level that SNDMVOL and SNDMSBAL move to a target over whole seconds,
/// one step per frame, in single precision like SmileBASIC 4. The time
/// passed grows by 1/60 per frame, and the level is the value of the
/// straight line at that time, until the time passed reaches the whole
/// time. From 0 to 1 over a second it is just under 1 at the 60th frame,
/// and 1 at the 61st.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Fade {
    moving: bool,
    seconds: f32,
    passed: f32,
    from: f32,
    to: f32,
    /// The level now, 0 to 1.
    value: f32,
}

impl Fade {
    pub(crate) const fn at(value: f32) -> Fade {
        Fade {
            moving: false,
            seconds: 0.0,
            passed: 0.0,
            from: value,
            to: value,
            value,
        }
    }

    /// Moves from the current level to `target` over `seconds`, or
    /// immediately for 0.
    pub(crate) fn to(&mut self, target: f32, seconds: f32) {
        self.moving = true;
        self.from = self.value;
        self.to = target;
        self.seconds = seconds;
        self.passed = 0.0;
        if seconds == 0.0 {
            self.value = target;
        }
    }

    /// A frame's step.
    pub(crate) fn step_frame(&mut self) {
        if !self.moving {
            return;
        }
        self.passed += 1.0 / 60.0;
        if self.seconds <= self.passed {
            self.value = self.to;
            self.moving = false;
        } else {
            self.value = self.from + (self.passed * (self.to - self.from)) / self.seconds;
        }
    }

    /// Returns the value SNDMVOL() and SNDMSBAL() read: 0 to 127, rounded
    /// down.
    pub(crate) fn read(&self) -> i32 {
        (self.value * 127.0) as i32
    }
}

/// Converts a volume or balance of 0 to 127 to the level [`Fade`] keeps.
fn level(n: i32) -> f32 {
    n as f32 / 127.0
}

/// SNDMVOL's level, and the per-sample gain that follows it from frame to
/// frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Master {
    pub(crate) level: Fade,
    gain: Ramp,
}

/// SNDMVOL's level for a gain of 1, which it starts at and returns to:
/// 0.7, which SNDMVOL() reads as 88. The output is linear in the level, so
/// 127 plays at 1.43 times the starting level.
const MASTER_UNITY: f32 = 0.7;

impl Master {
    pub(crate) const START: Master = Master {
        level: Fade::at(MASTER_UNITY),
        gain: Ramp {
            value: 1.0,
            target: 1.0,
            step: 0.0,
        },
    };

    /// Returns the gain for the level.
    fn level_gain(&self) -> f64 {
        f64::from(self.level.value / MASTER_UNITY)
    }

    /// A frame's step: the level moves, and the gain goes to it in a
    /// straight line through the frame.
    fn step_frame(&mut self, own: bool) {
        if own {
            self.level.step_frame();
        }
        let gain = self.level_gain();
        if gain != self.gain.target {
            self.gain.to(gain, 1);
        }
    }
}

/// The largest magnitude of a sample sent into the effector: well beyond
/// full scale. Only a sum that would clip anyway comes near it.
const MOST_SENT: f32 = 8.0;

/// Silences NaN samples, which `clamp` lets through. Checking for them
/// first keeps this out of the way of the mix's own loops.
#[inline(never)]
fn silence_nans(samples: &mut [f32]) {
    // All of them, without stopping at the first, so that several are
    // checked at a time.
    if samples.iter().fold(false, |nan, s| nan | s.is_nan()) {
        samples
            .iter_mut()
            .filter(|s| s.is_nan())
            .for_each(|s| *s = 0.0);
    }
}

/// Adds `from` times `gain` into `to`, sample by sample: the loop for a
/// sound with a constant gain, which the compiler can run on several
/// samples at a time.
#[inline(always)]
pub(crate) fn add_scaled(to: &mut [f32], from: &[f32], gain: f32) {
    for (t, &x) in to.iter_mut().zip(from) {
        *t += x * gain;
    }
}

/// Converts seconds, as BGMSTOP and similar commands take them, to frames.
pub(crate) fn frames(seconds: f64) -> u32 {
    (seconds * 60.0).round().clamp(0.0, f64::from(u32::MAX)) as u32
}

/// RECSTART's recording of what plays, as 16-bit samples.
#[derive(Clone, Debug, Default)]
pub(crate) struct Recorder {
    /// The buffer, which RECSTART leaves unchanged: the part a recording
    /// has not reached yet holds older recordings. It only grows.
    buffer: Vec<[i16; 2]>,
    /// The samples this recording takes, from the buffer's start.
    len: usize,
    pub(crate) pos: usize,
    pub(crate) recording: bool,
    /// Without a time, recording wraps around the buffer until RECSTOP.
    pub(crate) ring: bool,
    /// Whether it has taken a sample yet.
    pub(crate) begun: bool,
    /// The position of the last frame's samples in the buffer, and their
    /// number, for [`Sound::hear_sub`].
    frame_start: usize,
    taken: usize,
}

impl Recorder {
    /// RECSTART: `len` samples, looping with `ring`.
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

    /// The samples RECSTART asked for, with older samples in the part this
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

/// SNDMSBAL's balance, which moves to a new one, one step per frame, when
/// it is given a time.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Balance {
    /// The current balance, 0 to 1. A subprogram starts halfway, at 0.5,
    /// which SNDMSBAL reads as 63; the manual calls 64 even.
    level: Fade,
}

impl Balance {
    /// Halfway, where the sounds of the subprogram and the main program
    /// each play as loud as the main program's sound alone.
    pub(crate) const START: Balance = Balance {
        level: Fade::at(0.5),
    };

    /// SNDMSBAL: moves to `balance` over `seconds`, or immediately for 0.
    pub(crate) fn to(&mut self, balance: i32, seconds: f32) {
        self.level.to(level(balance.clamp(0, 127)), seconds);
    }

    fn step_frame(&mut self) {
        self.level.step_frame();
    }

    /// Returns the value SNDMSBAL reads: the current balance, rounded down.
    pub(crate) fn read(&self) -> i32 {
        self.level.read()
    }

    /// The gains of the main and the sub interpreter at this balance:
    /// b/127 and (127-b)/127 of the output for a balance b, doubled, so
    /// that halfway, where each subprogram starts, both play as loud as the
    /// main program alone.
    fn gains(&self) -> (f32, f32) {
        let b = self.level.value;
        (2.0 * b, 2.0 * (1.0 - b))
    }
}

/// EFCEN, EFCSET and EFCWET's settings, which the main program and the
/// subprogram share.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Effector {
    pub(crate) on: bool,
    /// EFCSET's reverb, which #EFCOFF leaves unchanged.
    pub(crate) settings: Option<reverb::Settings>,
    /// BEEP, BGM, TALK and PCM's wetness, all 127 to begin with.
    pub(crate) wet: [i32; 4],
}

impl Default for Effector {
    fn default() -> Self {
        Effector {
            on: false,
            // UNVERIFIED: that the effector starts with #EFCROOM, the same
            // as SNDSTOP leaves it.
            settings: reverb::Settings::preset(1),
            wet: [127; 4],
        }
    }
}

/// EFCWET's sends, in its order.
const BEEP_SEND: usize = 0;
const BGM_SEND: usize = 1;
const TALK_SEND: usize = 2;
const PCM_SEND: usize = 3;
/// The output gain for BEEP and BGM, and the share of them that EFCWET
/// 127 sends.
const HALF: f32 = 0.5;

pub struct Sound {
    instruments: Option<Instruments>,
    /// The preset songs and sound effects, when imported.
    sequences: Option<Arc<Sequences>>,
    tracks: Vec<Option<Track>>,
    /// BGMSET's songs, 128 to 255.
    pub(crate) songs: HashMap<i32, Rc<Song>>,
    /// The PSG and WAVSET's waveforms, shared with the tracks.
    pub(crate) waves: Rc<RefCell<Waves>>,
    /// The instruments' waveforms kept decoded, shared by the tracks and
    /// BEEP.
    decoded: Shared,
    pub(crate) beeps: Beeps,
    pub(crate) pcm: Pcm,
    pub(crate) talk: Talk,
    /// EFCEN, EFCSET and EFCWET's settings.
    effector: Rc<Cell<Effector>>,
    /// The reverb for the effector's settings.
    reverb: Option<reverb::Reverb>,
    /// What PCMSTREAM sends to the effector this frame, and then what
    /// the effector adds to the mix.
    pcm_aux: [Vec<f32>; 2],
    /// What BEEP and BGM send to the effector this frame.
    beep_bgm_aux: [Vec<f32>; 2],
    /// PCMSTREAM and TALK this frame.
    unhalved: [Vec<f32>; 2],
    /// One kind of sound at a time, to send part of it.
    part: [Vec<f32>; 2],
    /// PCMSTREAM's samples this frame, kept out of a recording.
    pcm_part: [Vec<f32>; 2],
    /// RECSTART's recording, which the sub interpreter shares with the main
    /// one.
    pub(crate) recorder: Rc<RefCell<Recorder>>,
    /// The sub interpreter's sound never records.
    pub(crate) is_sub_interpreter: bool,
    /// SNDMSBAL, which only subprograms set. The main and the sub
    /// interpreter share one from the sub's first frame on.
    balance: Rc<Cell<Balance>>,
    /// SNDMVOL: 0.7 at its start, which SNDMVOL() reads as 88, and the
    /// gain for it. In the sub interpreter it follows the main
    /// interpreter's SNDMVOL.
    pub(crate) master: Master,
    /// SNDSTOPs so far, shared by the main and the sub interpreter: either
    /// one's SNDSTOP stops every sound of both.
    stops: Rc<Cell<u32>>,
    /// The SNDSTOPs this interpreter's sound has seen.
    stops_seen: u32,
    /// Mixed samples, left and right in turn, not yet taken.
    out: Vec<f32>,
    scratch: [Vec<f32>; 2],
    mix: [Vec<f32>; 2],
    /// Each track's MML variables as the program sees them: their values
    /// when the last frame's sound was mixed.
    shown: [[i32; 8]; TRACKS],
}

impl Default for Sound {
    fn default() -> Self {
        let decoded = Decoded::new();
        Sound {
            instruments: None,
            sequences: None,
            tracks: (0..TRACKS).map(|_| None).collect(),
            songs: HashMap::new(),
            waves: Rc::default(),
            beeps: Beeps::sharing(None, &decoded),
            decoded,
            pcm: Pcm::default(),
            talk: Talk::default(),
            effector: Rc::default(),
            reverb: None,
            pcm_aux: Default::default(),
            beep_bgm_aux: Default::default(),
            unhalved: Default::default(),
            part: Default::default(),
            pcm_part: Default::default(),
            balance: Rc::new(Cell::new(Balance::START)),
            recorder: Rc::default(),
            is_sub_interpreter: false,
            master: Master::START,
            stops: Rc::default(),
            stops_seen: 0,
            out: Vec::new(),
            scratch: Default::default(),
            mix: Default::default(),
            shown: [[-1; 8]; TRACKS],
        }
    }
}

impl Sound {
    /// Sets the source of instruments; without any, BGM is silent.
    pub fn set_instruments(&mut self, instruments: Option<Instruments>) {
        self.beeps = Beeps::sharing(instruments.as_ref(), &self.decoded);
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
        let (waves, decoded) = (&self.waves, &self.decoded);
        self.tracks[n].get_or_insert_with(|| Track::new(instruments, waves.clone(), decoded))
    }

    /// The tracks that exist, which are all that can be playing.
    pub(crate) fn tracks(&mut self) -> impl Iterator<Item = &mut Track> {
        self.tracks.iter_mut().flatten()
    }

    /// A VSYNC that continued without waiting.
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
        let t = self.track(n);
        t.play(song, volume, fade);
        // UNVERIFIED: that BGMPLAY sends the track to the effector by the
        // last EFCWET and forgets an earlier BGMWET. On SmileBASIC 4 a
        // BGMWET before BGMPLAY made no audible change.
        t.wet = None;
    }

    pub(crate) fn stop(&mut self, n: usize, fade: u32) {
        if let Some(t) = self.tracks[n].as_mut() {
            // A song stopped with a fade in the frame it started in never
            // sounds. BGMCHK says 0 one frame later, or two frames later
            // after a VSYNC that did not wait (checked on hardware).
            if fade == 0 || t.fresh != Fresh::No {
                let synced = t.fresh == Fresh::Synced;
                t.stop();
                if fade > 0 && synced {
                    t.linger = LINGER_AFTER_SYNCED_FADE;
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
            t.linger = LINGER_AFTER_EMPTY_SONG;
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

    /// Returns what each playing or paused track plays, for the debugger.
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

    /// Holds back a song just started on an idle sound system for the
    /// frames the system takes to wake up. On hardware a song's first
    /// variable showed four frames after the first BGMPLAY of a run, but
    /// within two frames for later ones. Any BGMPLAY to an idle system
    /// waits, whether it is the first of a run or not, and its sound waits
    /// with it; the program does not wait. On hardware the sound began
    /// about 1276 samples after the call, 1.6 frames, while here it waits
    /// two whole frames: the timing of the variables above, which rhythm
    /// games follow, matches with two.
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

    /// SNDMVOL: moves to `volume` over `seconds`, or immediately for 0.
    pub(crate) fn set_master(&mut self, volume: i32, seconds: f32) {
        self.master.level.to(level(volume), seconds);
        if seconds == 0.0 {
            self.master.gain = Ramp::at(self.master.level_gain());
        }
    }

    /// Returns the value SNDMVOL() reads: the current volume, rounded down.
    pub(crate) fn master_volume(&self) -> i32 {
        self.master.level.read()
    }

    /// RUN puts SNDMVOL back at its start value.
    pub(crate) fn run_started(&mut self) {
        self.master = Master::START;
    }

    /// SNDSTOP's reset of the mixer: SNDMVOL and the shared SNDMSBAL go
    /// back to their start values, from either program.
    pub(crate) fn reset_mixer(&mut self) {
        self.master = Master::START;
        self.balance.set(Balance::START);
    }

    /// SNDSTOP: every sound of both programs stops, and the mixer and the
    /// effector go back to their start values.
    pub(crate) fn sndstop(&mut self) {
        self.stop_all();
        self.reset_mixer();
        self.reset_effector();
        self.stops.set(self.stops.get().wrapping_add(1));
        self.stops_seen = self.stops.get();
    }

    /// SNDMSBAL, which does nothing in the main program.
    pub(crate) fn set_balance(&mut self, balance: i32, seconds: f32) {
        if self.is_sub_interpreter {
            let mut b = self.balance.get();
            b.to(balance, seconds);
            self.balance.set(b);
        }
    }

    /// Returns the value SNDMSBAL() reads, the same in either program.
    pub(crate) fn balance(&self) -> i32 {
        self.balance.get().read()
    }

    /// SNDSTOP: every sound stops immediately.
    pub fn stop_all(&mut self) {
        for t in self.tracks() {
            t.stop();
        }
        self.beeps.stop_all();
        self.pcm.playing = false;
        self.talk.stop();
    }

    /// The gains of the main and the sub interpreter for this subprogram's
    /// balance.
    pub(crate) fn balance_gains(&self) -> (f32, f32) {
        self.balance.get().gains()
    }

    /// A subprogram starts: the balance goes back to halfway.
    pub fn sub_started(&mut self) {
        self.balance.set(Balance::START);
    }

    /// EFCWET's sends, which RECSTART sets to 0 and SNDSTOP to 127. New
    /// values send every track as EFCWET sets, in place of its BGMWET.
    pub(crate) fn set_sends(&mut self, wet: [i32; 4]) {
        let mut e = self.effector.get();
        if e.wet != wet {
            e.wet = wet;
            self.effector.set(e);
            for t in self.tracks() {
                t.wet = None;
            }
        }
    }

    /// EFCEN and EFCSET's settings and EFCWET's sends.
    pub(crate) fn effector(&self) -> Effector {
        self.effector.get()
    }

    /// EFCEN.
    pub(crate) fn set_effector_on(&mut self, on: bool) {
        let e = self.effector.get();
        self.effector.set(Effector { on, ..e });
    }

    /// SNDSTOP: the effector goes off with #EFCROOM set, and every send
    /// back to 127.
    pub(crate) fn reset_effector(&mut self) {
        self.set_reverb(reverb::Settings::preset(1));
        self.set_effector_on(false);
        self.set_sends([127; 4]);
    }

    /// EFCSET: the effector's reverb, which also turns the effector on.
    /// #EFCOFF turns it off and keeps the reverb set before, which a later
    /// EFCEN 1 plays.
    pub(crate) fn set_reverb(&mut self, settings: Option<reverb::Settings>) {
        let e = self.effector.get();
        self.effector.set(match settings {
            Some(_) => Effector {
                on: true,
                settings,
                ..e
            },
            None => Effector { on: false, ..e },
        });
    }

    /// Mixes one frame of samples.
    pub fn render_frame(&mut self) {
        // The other program's SNDSTOP.
        if self.stops_seen != self.stops.get() {
            self.stops_seen = self.stops.get();
            self.stop_all();
            self.master = Master::START;
        }
        // The sub interpreter moves the shared balance once per frame, and
        // the main interpreter moves SNDMVOL, which the sub interpreter
        // follows.
        if self.is_sub_interpreter {
            let mut b = self.balance.get();
            b.step_frame();
            self.balance.set(b);
        }
        self.master.step_frame(!self.is_sub_interpreter);
        let n = FRAME_SAMPLES;
        let [left, right] = &mut self.mix;
        left.clear();
        left.resize(n, 0.0);
        right.clear();
        right.resize(n, 0.0);
        // Each kind of sound sends part of itself to the effector, as set
        // by EFCWET (or a track's BGMWET). With the effector off, the sent
        // part passes through it unchanged. BEEP and BGM keep all of their
        // sound and send a copy, so EFCWET 127 with the effector off plays
        // them twice as loud as EFCWET 0, while RECSTART records only the
        // part they keep. The part PCMSTREAM and TALK send leaves their own
        // sound, so they play the same with the effector off. With it on,
        // PCMSTREAM keeps the reverb's own dry share, and the part TALK
        // sends never comes out. The output plays BEEP and BGM at half: at
        // the starting EFCWET 127 they play as loud as their sound.
        // UNVERIFIED: that EFCEN 1 with #EFCOFF passes the sends unchanged.
        let e = self.effector.get();
        if self.reverb.as_ref().map(reverb::Reverb::settings) != e.settings {
            self.reverb = e.settings.map(reverb::Reverb::new);
        }
        let effect = e.on && self.reverb.is_some();
        let sends = e.wet.map(|w| w as f32 / 127.0);
        let dry = self.reverb.as_ref().map_or(1.0, reverb::Reverb::dry);
        let (pcm_send, pcm_keep, talk_keep) = match effect {
            true => {
                let s = sends[PCM_SEND];
                (s, 1.0 - s * (1.0 - dry), 1.0 - sends[TALK_SEND])
            }
            false => (0.0, 1.0, 1.0),
        };
        for aux in &mut self.pcm_aux {
            aux.clear();
            if effect {
                aux.resize(n, 0.0);
            }
        }
        // With the effector off and BEEP and BGM sending the same share,
        // as they do at the start, the sent part passes as a gain on them.
        let passed = (!effect
            && sends[BEEP_SEND] == sends[BGM_SEND]
            && self.tracks.iter().flatten().all(|t| t.wet.is_none()))
        .then(|| HALF * (1.0 + sends[BGM_SEND]));
        let bus = if passed.is_some() { 0.0 } else { HALF };
        for aux in &mut self.beep_bgm_aux {
            aux.clear();
            if passed.is_none() {
                aux.resize(n, 0.0);
            }
        }
        for aux in &mut self.unhalved {
            aux.clear();
            aux.resize(n, 0.0);
        }
        let [pcm_aux_l, pcm_aux_r] = &mut self.pcm_aux;
        let [beep_bgm_l, beep_bgm_r] = &mut self.beep_bgm_aux;
        let [unhalved_l, unhalved_r] = &mut self.unhalved;
        // A recording not yet begun waits with a song that waits for the
        // sound system to wake, and so begins with the song. The sub
        // interpreter's sound reaches the main one's recording through
        // `hear_sub`.
        let record = !self.is_sub_interpreter && {
            let waking = self.tracks.iter().flatten().any(|t| t.hold > 0);
            let mut rec = self.recorder.borrow_mut();
            rec.frame_start = rec.pos;
            rec.taken = 0;
            rec.recording && (rec.begun || !waking)
        };
        // RECSTART does not hear PCMSTREAM, which is mixed in after the
        // recording and still sends to the effector.
        let pcm_apart = record && self.pcm.playing;
        // Whether PCMSTREAM or TALK may write into `unhalved` this frame.
        let unhalved_used = self.pcm.playing && !pcm_apart || self.talk.sounding();
        if pcm_apart {
            let [l, r] = &mut self.pcm_part;
            l.clear();
            l.resize(n, 0.0);
            r.clear();
            r.resize(n, 0.0);
            self.pcm.render(l, r);
            if pcm_send != 0.0 {
                for i in 0..n {
                    pcm_aux_l[i] += l[i] * pcm_send;
                    pcm_aux_r[i] += r[i] * pcm_send;
                    l[i] *= pcm_keep;
                    r[i] *= pcm_keep;
                }
            }
        }
        for t in self.tracks.iter_mut().flatten() {
            // UNVERIFIED: that BGMWET takes the place of EFCWET's BGM for
            // its track until the next BGMPLAY, a new EFCWET, RECSTART or
            // SNDSTOP.
            let s = t.wet.map_or(sends[BGM_SEND], |w| w as f32 / 127.0) * bus;
            let aux =
                (s > 0.0).then_some((beep_bgm_l.as_mut_slice(), beep_bgm_r.as_mut_slice(), s));
            t.render(left, right, &mut self.scratch, aux);
        }
        let [part_l, part_r] = &mut self.part;
        // A sound that sends part of itself: the part it keeps goes to
        // `to`, and the part it sends to `bus`.
        let mut mix = |s: f32,
                       keep: f32,
                       to: [&mut Vec<f32>; 2],
                       bus: [&mut Vec<f32>; 2],
                       render: &mut dyn FnMut(&mut [f32], &mut [f32])| {
            let [to_l, to_r] = to;
            if s == 0.0 && keep == 1.0 {
                render(to_l, to_r);
                return;
            }
            part_l.clear();
            part_l.resize(n, 0.0);
            part_r.clear();
            part_r.resize(n, 0.0);
            render(part_l, part_r);
            let [bus_l, bus_r] = bus;
            for i in 0..n {
                to_l[i] += part_l[i] * keep;
                to_r[i] += part_r[i] * keep;
                bus_l[i] += part_l[i] * s;
                bus_r[i] += part_r[i] * s;
            }
        };
        mix(
            sends[BEEP_SEND] * bus,
            1.0,
            [&mut *left, &mut *right],
            [&mut *beep_bgm_l, &mut *beep_bgm_r],
            &mut |l, r| self.beeps.render(l, r),
        );
        if !pcm_apart {
            mix(
                pcm_send,
                pcm_keep,
                [&mut *unhalved_l, &mut *unhalved_r],
                [&mut *pcm_aux_l, &mut *pcm_aux_r],
                &mut |l, r| self.pcm.render(l, r),
            );
        }
        mix(
            0.0,
            talk_keep,
            [&mut *unhalved_l, &mut *unhalved_r],
            [&mut *pcm_aux_l, &mut *pcm_aux_r],
            &mut |l, r| self.talk.render(l, r),
        );
        // The effector adds its output in place of what was sent to it.
        // RECSTART records the sound before the effector.
        if let Some(r) = self.reverb.as_mut().filter(|_| effect) {
            // A tail keeps its input: one bad sample would keep ringing.
            for aux in [
                &mut *pcm_aux_l,
                &mut *pcm_aux_r,
                &mut *beep_bgm_l,
                &mut *beep_bgm_r,
            ] {
                silence_nans(aux);
                aux.iter_mut()
                    .for_each(|s| *s = s.clamp(-MOST_SENT, MOST_SENT));
            }
            r.process(pcm_aux_l, pcm_aux_r, [beep_bgm_l, beep_bgm_r]);
        }
        // The mix, as RECSTART and the main program hear it. Without
        // PCMSTREAM or TALK this adds zeros, which leave it unchanged.
        if unhalved_used {
            for (to, from) in [(&mut *left, &*unhalved_l), (&mut *right, &*unhalved_r)] {
                to.iter_mut().zip(from).for_each(|(t, &x)| *t += x);
            }
        }
        for (i, shown) in self.shown.iter_mut().enumerate() {
            *shown = match &self.tracks[i] {
                Some(t) if t.playing => t.vars,
                _ => [-1; 8],
            };
        }
        if self.out.len() > MOST_UNTAKEN {
            self.out.drain(..self.out.len() - MOST_UNTAKEN);
        }
        self.out.reserve(2 * n);
        let start = self.out.len();
        let [wet_l, wet_r] = &self.pcm_aux;
        let [beep_bgm_l, beep_bgm_r] = &self.beep_bgm_aux;
        let [unhalved_l, unhalved_r] = &self.unhalved;
        let [pcm_l, pcm_r] = &self.pcm_part;
        let sent = match effect {
            true => [wet_l, wet_r],
            false => [beep_bgm_l, beep_bgm_r],
        };
        // BEEP and BGM at half, or with the gain they pass; the rest
        // unchanged.
        let keep = passed.unwrap_or(HALF);
        let rest = 1.0 - keep;
        let mut rec = self.recorder.borrow_mut();
        if !record && self.master.gain.done() {
            // The loop below with its gain fixed and nothing to record:
            // the same sums, in loops the compiler can run on several
            // samples at a time.
            let gain = self.master.gain.value as f32;
            self.out.resize(start + 2 * n, 0.0);
            let out = &mut self.out[start..];
            for (o, (&l, &w)) in out.iter_mut().step_by(2).zip(left.iter().zip(unhalved_l)) {
                *o = l * gain * keep + w * gain * rest;
            }
            for (o, (&r, &w)) in out
                .iter_mut()
                .skip(1)
                .step_by(2)
                .zip(right.iter().zip(unhalved_r))
            {
                *o = r * gain * keep + w * gain * rest;
            }
            let both = |side: [&Vec<f32>; 2], out: &mut [f32]| {
                if side[0].len() == n && side[1].len() == n {
                    let pairs = out.chunks_exact_mut(2).zip(side[0].iter().zip(side[1]));
                    for (o, (&l, &r)) in pairs {
                        o[0] += l * gain;
                        o[1] += r * gain;
                    }
                }
            };
            both(sent, out);
            if pcm_apart {
                both([pcm_l, pcm_r], out);
            }
            out.iter_mut().for_each(|s| *s = s.clamp(-1.0, 1.0));
            silence_nans(out);
            return;
        }
        for i in 0..n {
            let gain = self.master.gain.value as f32;
            let (l, r) = (left[i] * gain, right[i] * gain);
            if record {
                rec.record(l.clamp(-1.0, 1.0), r.clamp(-1.0, 1.0));
            }
            let (mut l, mut r) = (
                l * keep + unhalved_l[i] * gain * rest,
                r * keep + unhalved_r[i] * gain * rest,
            );
            if let (Some(sl), Some(sr)) = (sent[0].get(i), sent[1].get(i)) {
                l += sl * gain;
                r += sr * gain;
            }
            if pcm_apart {
                l += pcm_l[i] * gain;
                r += pcm_r[i] * gain;
            }
            self.out.push(l.clamp(-1.0, 1.0));
            self.out.push(r.clamp(-1.0, 1.0));
            self.master.gain.step_sample();
        }
        silence_nans(&mut self.out[start..]);
    }

    /// Lets RECSTART in the main program hear what the sub interpreter
    /// mixed this frame, together with the main program's own sound,
    /// balanced by the subprogram's SNDMSBAL. The sub interpreter shares
    /// the main interpreter's recorder: its RECSAVE reads what the main
    /// program recorded last.
    pub(crate) fn hear_sub(&mut self, sub: &mut Sound) {
        if !Rc::ptr_eq(&sub.recorder, &self.recorder) {
            sub.recorder = Rc::clone(&self.recorder);
            sub.is_sub_interpreter = true;
            self.balance.set(sub.balance.get());
            sub.balance = Rc::clone(&self.balance);
            // UNVERIFIED: that the two programs share the effector. On
            // SmileBASIC 4, after the main program's EFCWET 0, the
            // subprogram's tones played at the main program's level.
            sub.effector = Rc::clone(&self.effector);
            sub.stops = Rc::clone(&self.stops);
            sub.stops_seen = self.stops.get();
        }
        let rec = &mut *self.recorder.borrow_mut();
        if rec.taken == 0 {
            return;
        }
        let (own, theirs) = sub.balance_gains();
        let theirs = theirs * sub.master.gain.value as f32;
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
    /// balanced by SNDMSBAL.
    #[test]
    fn recording_hears_the_subprogram_as_sndmsbal_balances_it() {
        let heard = |balance: i32| {
            let (mut main, mut sub) = (Sound::default(), Sound::default());
            sub.is_sub_interpreter = true;
            sub.set_balance(balance, 0.0);
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
        let mut b = Balance::START;
        b.to(127, 0.0);
        assert_eq!(b.gains(), (2.0, 0.0));
        assert_eq!(Balance::START.gains(), (1.0, 1.0));
    }

    /// The main program's SNDMSBAL does nothing, and both programs read
    /// the subprogram's balance once it has had a frame; SNDSTOP in either
    /// program puts it back to halfway, and SNDMVOL back to 88.
    #[test]
    fn the_balance_is_shared_and_sndstop_resets_the_mixer() {
        let (mut main, mut sub) = (Sound::default(), Sound::default());
        main.set_balance(127, 0.0);
        assert_eq!(main.balance(), 63);
        main.render_frame();
        sub.render_frame();
        main.hear_sub(&mut sub);
        sub.set_balance(127, 0.0);
        assert_eq!((main.balance(), sub.balance()), (127, 127));
        main.set_master(32, 0.0);
        assert_eq!(main.master_volume(), 32);
        main.reset_mixer();
        assert_eq!((main.balance(), sub.balance()), (63, 63));
        assert_eq!(main.master_volume(), 88);
        main.set_master(127, 0.0);
        assert_eq!(main.master_volume(), 127);
        assert!((main.master.gain.value - 127.0 / 88.9).abs() < 1e-6);
    }

    /// A fade of SNDMVOL or SNDMSBAL over a second from 0 reads 126 at the
    /// 60th frame and 127 at the 61st, because single precision steps of
    /// 1/60 add up to just under 1; it moves in a straight line before
    /// that.
    #[test]
    fn fades_reach_their_target_a_frame_after_the_last_step() {
        let (mut main, mut sub) = (Sound::default(), Sound::default());
        main.render_frame();
        sub.render_frame();
        main.hear_sub(&mut sub);
        main.set_master(0, 0.0);
        main.set_master(127, 1.0);
        sub.set_balance(0, 0.0);
        sub.set_balance(127, 1.0);
        let mut read = vec![(main.master_volume(), sub.balance())];
        for _ in 0..62 {
            main.render_frame();
            sub.render_frame();
            read.push((main.master_volume(), sub.balance()));
        }
        let levels: Vec<i32> = read.iter().map(|r| r.0).collect();
        assert_eq!(read.iter().map(|r| r.1).collect::<Vec<_>>(), levels);
        assert_eq!(&levels[..10], [0, 2, 4, 6, 8, 10, 12, 14, 16, 19]);
        assert_eq!(&levels[58..], [122, 124, 126, 127, 127]);
    }

    /// Either program's SNDSTOP stops the other's sound too.
    #[test]
    fn sndstop_in_either_program_stops_both() {
        let song = || Some(Rc::new(mml::parse("@219C1").unwrap()));
        let (mut main, mut sub) = (Sound::default(), Sound::default());
        main.render_frame();
        sub.render_frame();
        main.hear_sub(&mut sub);
        main.play(0, song(), 100, 0);
        sub.play(0, song(), 100, 0);
        main.set_master(32, 0.0);
        sub.sndstop();
        assert!(main.playing(0) && !sub.playing(0));
        main.render_frame();
        assert!(!main.playing(0));
        assert_eq!(main.master_volume(), 88);
        sub.play(0, song(), 100, 0);
        main.sndstop();
        sub.render_frame();
        assert!(!sub.playing(0));
    }

    /// The mix's loops for a constant gain give the same samples as its
    /// per-sample loop, which a recording takes, to the bit: through fades
    /// of the song and of SNDMVOL, sends to the effector and the silence
    /// after the song.
    #[test]
    fn the_mix_is_the_same_recorded_or_not() {
        let song = Rc::new(mml::parse("T150@219L8O4CDE@220GAB<C>R2").unwrap());
        let mixed = |record: bool| {
            let mut s = Sound::default();
            if record {
                s.recorder.borrow_mut().start(100 * FRAME_SAMPLES, false);
            }
            s.play(0, Some(song.clone()), 100, 20);
            s.play(1, Some(song.clone()), 64, 0);
            let mut out = Vec::new();
            for frame in 0..90 {
                match frame {
                    10 => s.set_master(40, 1.0),
                    30 => s.set_sends([127, 60, 127, 127]),
                    40 => s.stop(1, 12),
                    _ => {}
                }
                s.render_frame();
                out.extend(s.take_samples().iter().map(|x| x.to_bits()));
            }
            out
        };
        let plain = mixed(false);
        assert!(plain.iter().any(|&x| f32::from_bits(x) != 0.0));
        assert_eq!(plain, mixed(true));
    }

    /// A track that has played and stopped, and BEEP with nothing to play,
    /// do no per-sample work: they leave their buffers untouched.
    #[test]
    fn idle_sounds_do_nothing_a_sample_at_a_time() {
        let mut s = Sound::default();
        s.play(0, Some(Rc::new(mml::parse("@219C64").unwrap())), 100, 0);
        for _ in 0..5 {
            s.render_frame();
        }
        assert!(s.take_samples().iter().any(|&x| x != 0.0));
        s.stop(0, 0);
        s.scratch = Default::default();
        s.render_frame();
        assert!(s.scratch.iter().all(|b| b.capacity() == 0));
        assert!(s.mix[0].iter().chain(&s.mix[1]).all(|&x| x.to_bits() == 0));
        assert!(s.take_samples().iter().all(|&x| x.to_bits() == 0));
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
        assert!(sub.is_sub_interpreter && Rc::ptr_eq(&sub.recorder, &main.recorder));
        sub.render_frame();
        assert_eq!(main.recorder.borrow().pos, FRAME_SAMPLES);
        main.render_frame();
        assert_eq!(main.recorder.borrow().pos, 2 * FRAME_SAMPLES);
    }
}
