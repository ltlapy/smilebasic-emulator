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

use std::cell::RefCell;
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
    /// Whether it has taken a sample yet.
    pub(crate) begun: bool,
}

impl Recorder {
    fn record(&mut self, left: f32, right: f32) {
        if !self.recording || self.samples.is_empty() {
            return;
        }
        self.begun = true;
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
    pub(crate) recorder: Recorder,
    /// SNDMSBAL, which only subprograms set. It starts at 63, though the
    /// manual calls 64 even (checked on hardware).
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
            reverb: None,
            aux: Default::default(),
            part: Default::default(),
            balance: 63,
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

    /// EFCSET: the effector's reverb, or none.
    pub(crate) fn set_reverb(&mut self, settings: Option<reverb::Settings>) {
        if self.reverb.as_ref().map(|r| r.settings()) != settings {
            self.reverb = settings.map(reverb::Reverb::new);
        }
    }

    /// Mixes one frame of samples.
    pub fn render_frame(&mut self) {
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
        // sound system to wake, and so begins with it.
        let waking = self.tracks.iter().flatten().any(|t| t.hold > 0);
        let record = self.recorder.begun || !waking;
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
        mix(sends[PCM_SEND], &mut |l, r| self.pcm.render(l, r));
        mix(sends[TALK_SEND], &mut |l, r| self.talk.render(l, r));
        // What the effector adds: RECSTART records the sound before it.
        if let Some(r) = self.reverb.as_mut().filter(|_| effect) {
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
        let [aux_l, aux_r] = &self.aux;
        for i in 0..n {
            let gain = self.master.value as f32;
            let (l, r) = (left[i] * gain, right[i] * gain);
            if record {
                self.recorder.record(l.clamp(-1.0, 1.0), r.clamp(-1.0, 1.0));
            }
            let (l, r) = match (aux_l.get(i), aux_r.get(i)) {
                (Some(al), Some(ar)) => (l + al * gain, r + ar * gain),
                _ => (l, r),
            };
            self.out.push(l.clamp(-1.0, 1.0));
            self.out.push(r.clamp(-1.0, 1.0));
            self.master.step_sample();
        }
    }

    /// The samples mixed since the last call, left and right in turn.
    pub fn take_samples(&mut self) -> Vec<f32> {
        std::mem::take(&mut self.out)
    }
}
