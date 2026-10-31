//! Sound: BGM tracks playing MML on a SoundFont, mixed a frame at a time
//! into stereo samples for the frontend to play.

pub mod bank;
pub mod beep;
pub mod mml;
pub mod pcm;
pub mod sampler;
pub mod seq;
pub mod synth;
pub mod talk;
mod track;
pub mod wave;

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

pub use rustysynth::SoundFont;

use beep::Beeps;
use mml::Song;
use pcm::Pcm;
use seq::{Kind, Sequences};
use synth::Instruments;
use talk::Talk;
use track::{AfterFade, Track};
use wave::Waves;

/// Samples a second.
pub const RATE: u32 = 48_000;
/// Samples in one frame at 60 frames a second.
pub const FRAME_SAMPLES: usize = (RATE / 60) as usize;
/// BGM tracks 0 to 15.
pub const TRACKS: usize = 16;
/// Frames a song started on an idle sound system waits.
const WAKE_FRAMES: u32 = 2;
/// Frames until a fading BGMSTOP shows in BGMCHK.
const STOP_SHOWN_FRAMES: u32 = 2;
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

/// Seconds, as BGMSTOP and friends take them, in frames.
pub(crate) fn frames(seconds: f64) -> u32 {
    (seconds * 60.0).round().clamp(0.0, f64::from(u32::MAX)) as u32
}

/// RECSTART's recording of what plays, as 16-bit samples.
#[derive(Clone, Debug, Default)]
pub(crate) struct Recorder {
    pub(crate) samples: Vec<[i16; 2]>,
    pub(crate) pos: usize,
    pub(crate) recording: bool,
    /// Without a time, recording goes round the buffer until RECSTOP.
    pub(crate) ring: bool,
}

impl Recorder {
    fn record(&mut self, left: f32, right: f32) {
        if !self.recording || self.samples.is_empty() {
            return;
        }
        let s = |x: f32| (x * 32767.0).round() as i16;
        self.samples[self.pos] = [s(left), s(right)];
        self.pos += 1;
        if self.pos == self.samples.len() {
            if self.ring {
                self.pos = 0;
            } else {
                self.recording = false;
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Effector {
    pub(crate) on: bool,
    pub(crate) preset: i32,
    /// BEEP, BGM, TALK and PCM's wetness.
    pub(crate) wet: [i32; 4],
}

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
    // UNVERIFIED: kept, but the effector does not change the sound yet.
    pub(crate) effector: Effector,
    pub(crate) recorder: Recorder,
    /// SNDMSBAL, which only subprograms set.
    pub(crate) balance: i32,
    /// SNDMVOL, as a gain.
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
            balance: 64,
            recorder: Recorder::default(),
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
        self.instruments = instruments;
        self.tracks.iter_mut().for_each(|t| *t = None);
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

    /// BGMCHK: a track fading out to stop says it has stopped two frames
    /// after the BGMSTOP, whatever the fade's length, and calling BGMSTOP
    /// again does not put that off (checked on hardware).
    // UNVERIFIED: that the fade goes on being heard.
    pub(crate) fn playing(&self, n: usize) -> bool {
        self.tracks[n]
            .as_ref()
            .is_some_and(|t| t.playing && t.stop_shown_in != Some(0))
    }

    pub(crate) fn play(&mut self, n: usize, song: Option<Rc<Song>>, volume: i32, fade: u32) {
        self.track(n).play(song, volume, fade);
    }

    pub(crate) fn stop(&mut self, n: usize, fade: u32) {
        if let Some(t) = self.tracks[n].as_mut() {
            if fade == 0 {
                t.stop();
            } else {
                t.fade.to(0.0, fade);
                t.after_fade = AfterFade::Stop;
                t.stop_shown_in.get_or_insert(STOP_SHOWN_FRAMES);
            }
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

    /// Whether no BGM track plays.
    pub(crate) fn idle(&self) -> bool {
        self.tracks.iter().flatten().all(|t| !t.playing)
    }

    /// Holds a song just started on an idle sound system back for the
    /// frames it takes to wake up: on hardware a song's first variable
    /// showed four frames after the first BGMPLAY of a run, but within
    /// two for later ones.
    // UNVERIFIED: whether the wait comes from the system being idle or
    // from the first BGMPLAY of a run, and that the sound waits with it.
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

    /// SNDSTOP: every sound stops at once.
    pub fn stop_all(&mut self) {
        for t in self.tracks() {
            t.stop();
        }
        self.beeps.stop_all();
        self.pcm.playing = false;
        self.talk.stop();
    }

    /// Mixes one frame of samples.
    pub fn render_frame(&mut self) {
        let n = FRAME_SAMPLES;
        let [left, right] = &mut self.mix;
        left.clear();
        left.resize(n, 0.0);
        right.clear();
        right.resize(n, 0.0);
        for t in self.tracks.iter_mut().flatten() {
            t.render(left, right, &mut self.scratch);
        }
        self.beeps.render(left, right);
        self.pcm.render(left, right);
        self.talk.render(left, right);
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
        for i in 0..n {
            let gain = self.master.value as f32;
            let (l, r) = (
                (left[i] * gain).clamp(-1.0, 1.0),
                (right[i] * gain).clamp(-1.0, 1.0),
            );
            self.recorder.record(l, r);
            self.out.push(l);
            self.out.push(r);
            self.master.step_sample();
        }
    }

    /// The samples mixed since the last call, left and right in turn.
    pub fn take_samples(&mut self) -> Vec<f32> {
        std::mem::take(&mut self.out)
    }
}
