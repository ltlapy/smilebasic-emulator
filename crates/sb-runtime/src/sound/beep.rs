//! BEEP's sounds: up to 16 at once, each on a handle that BEEPPIT,
//! BEEPVOL, BEEPPAN and BEEPSTOP change.

use std::rc::Rc;
use std::sync::Arc;

use super::inspect::{BeepInfo, BeepKind};
use super::sampler::{MAX_VOICES, TICK};
use super::seq::{Player, Sequence};
use super::synth::{Instruments, Synth};
use super::wave::{self, Wave};
use super::{RATE, Ramp};

pub const HANDLES: usize = 16;
/// The note BEEP plays instruments at.
// UNVERIFIED: SmileBASIC's note for BEEP 256 to 383.
const INSTRUMENT_NOTE: i32 = 69;
/// How long an instrument sounds before it is let go.
// UNVERIFIED: how long BEEP holds an instrument's note.
const INSTRUMENT_FRAMES: u32 = 30;
/// Frames an instrument's handle stays taken after letting go, for the
/// note to die away.
const RELEASE_FRAMES: u32 = 60;
/// BEEP's volume a waveform or preset effect plays at as it is, as BGMPLAY's
/// is: it is a linear gain, not squared as MML's `V` is.
const WAVE_VOLUME: f32 = 64.0;
/// How loud BEEP's default leaves an instrument's note, by the note at `V127`.
const INSTRUMENT_LEVEL: f32 = 70.0 / 127.0;

/// The channel volume for BEEP's `volume` on an instrument, which squares it:
/// BEEP's is linear, up to all the channel has.
pub(crate) fn instrument_volume(volume: i32) -> i32 {
    let level = volume as f32 / WAVE_VOLUME * INSTRUMENT_LEVEL;
    (127.0 * level.sqrt()).round().min(127.0) as i32
}

/// Samples from the start of a waveform's sound to the end of its note, at
/// `ticks` of [`wave::BEEP_TICKS_A_SECOND`]. The sound engine moves on a
/// step at a time, 0.96 of these ticks in each, the first step included;
/// the note's level then takes one more step to fall.
fn note_end(ticks: u64) -> usize {
    ((ticks * 25).div_ceil(24) - 1) as usize * TICK
}

/// Adds `voice` to `notes`, which holds [`MAX_VOICES`] at most: the oldest
/// makes way. The list takes its memory once, when the first note joins it.
fn pile(notes: &mut Vec<wave::Voice>, voice: wave::Voice) {
    if notes.capacity() == 0 {
        notes.reserve_exact(MAX_VOICES);
    }
    if notes.len() == MAX_VOICES {
        notes.remove(0);
    }
    notes.push(voice);
}

/// A waveform's note, and those before it while they die away.
struct WaveNote {
    voice: wave::Voice,
    old: Vec<wave::Voice>,
    wave: Rc<Wave>,
    /// The ticks of the passes so far, for those that repeat.
    ticks: u64,
}

impl WaveNote {
    fn voice(wave: &Rc<Wave>, pitch: i32, pan: i32) -> wave::Voice {
        let mut v = wave::Voice::new(wave.clone(), None, wave.base_note, pitch, wave.beep_loop);
        v.set_mix(127, 127, pan);
        v
    }

    fn new(wave: Rc<Wave>, pitch: i32, pan: i32) -> WaveNote {
        WaveNote {
            voice: WaveNote::voice(&wave, pitch, pan),
            old: Vec::new(),
            ticks: wave.beep_ticks.map_or(0, u64::from),
            wave,
        }
    }

    /// Ends the note. With the repeat flag the next pass starts in the same
    /// place, the old notes dying away over it however long their release
    /// takes, and this gives the samples until that one ends. With `full`,
    /// when all the sounds have as many old notes as they may, this note's
    /// oldest makes way.
    fn end_note(&mut self, pitch: i32, pan: i32, full: bool) -> Option<usize> {
        if !self.wave.beep_repeats {
            self.voice.release();
            return None;
        }
        let new = WaveNote::voice(&self.wave, pitch, pan);
        let mut old = std::mem::replace(&mut self.voice, new);
        old.release();
        if full && !self.old.is_empty() {
            self.old.remove(0);
        }
        pile(&mut self.old, old);
        let pass = self.wave.beep_ticks.map_or(0, u64::from);
        let hold = note_end(self.ticks + pass) - note_end(self.ticks);
        self.ticks += pass;
        Some(hold)
    }

    /// Adds samples of the notes into the buffers.
    fn render(&mut self, left: &mut [f32], right: &mut [f32]) {
        self.voice.render(left, right);
        if !self.old.is_empty() {
            for old in &mut self.old {
                old.render(left, right);
            }
            self.old.retain(|old| !old.done() && old.audible());
        }
    }

    fn set_pitch(&mut self, pitch: i32) {
        self.voice.set_pitch(pitch);
        self.old.iter_mut().for_each(|v| v.set_pitch(pitch));
    }

    fn set_pan(&mut self, pan: i32) {
        self.voice.set_mix(127, 127, pan);
        self.old.iter_mut().for_each(|v| v.set_mix(127, 127, pan));
    }
}

enum Sounding {
    /// A preset effect, which there is nothing to play for.
    Preset,
    /// A note on the beep synthesizer's channel of the same number: held
    /// for `hold` frames, then left `release` frames to die away.
    Instrument {
        key: i32,
        hold: u32,
        release: u32,
    },
    Wave(Box<WaveNote>),
    /// A preset effect: a sequence on a synthesizer of its own, as it may
    /// take several channels. `tail` counts the frames left after it
    /// ends, for its notes to die away.
    Sequence {
        player: Box<Player>,
        synth: Box<dyn Synth>,
        until_tick: f64,
        tail: u32,
    },
}

struct Beep {
    sounding: Sounding,
    /// BEEP's sound number, and the pitch BEEPPIT last set, for the
    /// debugger.
    sound: i32,
    pitch: i32,
    /// The pitch at the start, in cents: what BEEPPIT bends from.
    start_pitch: i32,
    volume: i32,
    pan: i32,
    fade: Ramp,
    stopping: bool,
    /// Samples until a waveform's note is released; `None` for never.
    hold: Option<usize>,
    /// BEEP gave its handle to the program: until the sound ends or is
    /// stopped, a new BEEP does not take this one's place.
    kept: bool,
}

pub struct Beeps {
    instruments: Option<Instruments>,
    synth: Option<Box<dyn Synth>>,
    beeps: Vec<Option<Beep>>,
    /// Which handle the next BEEP tries first: it goes on rising, whether
    /// the last has ended or not, and round after the last handle.
    next: usize,
    /// Notes of waveforms whose handles have gone, falling as their
    /// envelopes' releases say, at the gain they had.
    falling: Vec<wave::Voice>,
    scratch: [Vec<f32>; 2],
    wave_scratch: [Vec<f32>; 2],
}

impl Beeps {
    pub fn new(instruments: Option<&Instruments>) -> Beeps {
        Beeps {
            instruments: instruments.cloned(),
            synth: instruments.map(Instruments::synth),
            beeps: (0..HANDLES).map(|_| None).collect(),
            next: 0,
            falling: Vec::new(),
            scratch: Default::default(),
            wave_scratch: Default::default(),
        }
    }

    /// Runs `f` on the synthesizer, if there is one.
    fn synth(&mut self, f: impl FnOnce(&mut dyn Synth)) {
        if let Some(s) = &mut self.synth {
            f(s.as_mut());
        }
    }

    /// The next handle that no program holds to a sound still going, which
    /// ends whatever played on it before; `None` if all are held. A handle
    /// nobody took back goes to the next BEEP that gets there.
    fn handle(&mut self) -> Option<usize> {
        let h = (0..HANDLES)
            .map(|i| (self.next + i) % HANDLES)
            .find(|&h| !self.beeps[h].as_ref().is_some_and(|b| b.kept))?;
        self.next = (h + 1) % HANDLES;
        self.stop_now(h);
        Some(h)
    }

    /// Applies a handle's volume and pan to what it plays.
    fn mix(&mut self, h: usize) {
        let Some(b) = &mut self.beeps[h] else {
            return;
        };
        let (volume, pan) = (instrument_volume(b.volume), b.pan);
        match &mut b.sounding {
            Sounding::Wave(note) => note.set_pan(pan),
            Sounding::Instrument { .. } => self.synth(|s| {
                s.volume(h, volume);
                s.pan(h, pan);
            }),
            Sounding::Preset | Sounding::Sequence { .. } => {}
        }
    }

    /// Plays BEEP's `sound`, an instrument (`program` 0 to 127) or a
    /// waveform, or holds a handle for a preset effect, and gives the
    /// handle, which `keep` makes the program's while the sound goes on.
    /// When the program holds all of them, nothing plays and there is no
    /// handle.
    pub fn play(
        &mut self,
        sound: i32,
        what: BeepSound,
        pitch: i32,
        volume: i32,
        pan: i32,
        keep: bool,
    ) -> Option<usize> {
        let h = self.handle()?;
        let hold = match &what {
            BeepSound::Wave(w) => w.beep_ticks.map(|ticks| note_end(u64::from(ticks))),
            _ => None,
        };
        let sounding = match what {
            BeepSound::Preset => Sounding::Preset,
            BeepSound::Sequence(seq) => match &self.instruments {
                Some(instruments) => {
                    let mut synth = instruments.synth();
                    let mut player = Box::new(Player::new(seq));
                    player.set_pitch(pitch, synth.as_mut());
                    Sounding::Sequence {
                        player,
                        synth,
                        until_tick: 0.0,
                        tail: RELEASE_FRAMES,
                    }
                }
                None => Sounding::Preset,
            },
            BeepSound::Instrument(program) => {
                let key = INSTRUMENT_NOTE + pitch.div_euclid(100);
                self.synth(|s| {
                    s.expression(h, 127);
                    s.program(h, program);
                    s.bend(h, pitch.rem_euclid(100));
                    if (0..128).contains(&key) {
                        s.note_on(h, key, 127);
                    }
                });
                Sounding::Instrument {
                    key,
                    hold: INSTRUMENT_FRAMES,
                    release: RELEASE_FRAMES,
                }
            }
            BeepSound::Wave(w) => Sounding::Wave(Box::new(WaveNote::new(w, pitch, pan))),
        };
        self.beeps[h] = Some(Beep {
            sounding,
            sound,
            pitch,
            start_pitch: pitch,
            volume,
            pan,
            fade: Ramp::at(1.0),
            stopping: false,
            hold,
            kept: keep,
        });
        self.mix(h);
        Some(h)
    }

    pub fn set_pitch(&mut self, h: usize, pitch: i32) {
        let Some(b) = &mut self.beeps[h] else {
            return;
        };
        b.pitch = pitch;
        match &mut b.sounding {
            Sounding::Wave(note) => note.set_pitch(pitch),
            Sounding::Instrument { .. } => {
                let from = b.start_pitch.div_euclid(100) * 100;
                self.synth(|s| s.bend(h, pitch - from));
            }
            Sounding::Sequence { player, synth, .. } => player.set_pitch(pitch, synth.as_mut()),
            Sounding::Preset => {}
        }
    }

    pub fn set_volume(&mut self, h: usize, volume: i32) {
        if let Some(b) = &mut self.beeps[h] {
            b.volume = volume;
        }
        self.mix(h);
    }

    pub fn set_pan(&mut self, h: usize, pan: i32) {
        if let Some(b) = &mut self.beeps[h] {
            b.pan = pan;
        }
        self.mix(h);
    }

    /// Ends what plays on a handle, which frees it. A waveform's notes are
    /// let go and fall on their own, as their releases say: at release 127
    /// that is the sound engine's one step.
    fn stop_now(&mut self, h: usize) {
        let Some(b) = self.beeps[h].take() else {
            return;
        };
        match b.sounding {
            Sounding::Instrument { .. } => self.synth(|s| s.channel_off(h)),
            Sounding::Wave(note) => {
                let gain = b.fade.value as f32 * b.volume as f32 / WAVE_VOLUME;
                if gain > 0.0 {
                    let WaveNote { mut voice, old, .. } = *note;
                    voice.release();
                    for mut v in old.into_iter().chain([voice]).filter(|v| !v.done()) {
                        v.scale(gain);
                        pile(&mut self.falling, v);
                    }
                }
            }
            Sounding::Preset | Sounding::Sequence { .. } => {}
        }
    }

    /// Stops a handle's sound, fading over `frames`, and lets the next BEEP
    /// have the handle.
    pub fn stop(&mut self, h: usize, frames: u32) {
        match &mut self.beeps[h] {
            Some(b) if frames > 0 => {
                b.fade.to(0.0, frames);
                b.stopping = true;
                b.kept = false;
            }
            _ => self.stop_now(h),
        }
    }

    /// The handles that sound, for the debugger.
    pub fn inspect(&self) -> Vec<BeepInfo> {
        (self.beeps.iter().enumerate())
            .filter_map(|(handle, b)| {
                let b = b.as_ref()?;
                let kind = match &b.sounding {
                    Sounding::Preset => BeepKind::Silent,
                    Sounding::Sequence { .. } => BeepKind::Effect,
                    Sounding::Instrument { .. } => BeepKind::Instrument(b.sound - 256),
                    Sounding::Wave(_) => BeepKind::Wave,
                };
                Some(BeepInfo {
                    handle,
                    sound: b.sound,
                    kind,
                    pitch: b.pitch,
                    volume: b.volume,
                    pan: b.pan,
                    stopping: b.stopping,
                })
            })
            .collect()
    }

    pub fn stop_all(&mut self) {
        for h in 0..HANDLES {
            self.stop_now(h);
        }
        self.falling.clear();
    }

    /// Adds a frame of the sounds into the buffers.
    pub fn render(&mut self, left: &mut [f32], right: &mut [f32]) {
        let n = left.len();
        // Instruments fade through their channel's expression.
        for h in 0..HANDLES {
            if let Some(Beep {
                sounding: Sounding::Instrument { .. },
                fade,
                ..
            }) = &self.beeps[h]
            {
                let expression = (fade.value * 127.0).round() as i32;
                self.synth(|s| s.expression(h, expression));
            }
        }
        let [l, r] = &mut self.scratch;
        l.clear();
        l.resize(n, 0.0);
        r.clear();
        r.resize(n, 0.0);
        if let Some(s) = &mut self.synth {
            s.render(l, r);
        }
        for i in 0..n {
            left[i] += l[i];
            right[i] += r[i];
        }
        let old_notes: usize = (self.beeps.iter().flatten())
            .map(|b| match &b.sounding {
                Sounding::Wave(note) => note.old.len(),
                _ => 0,
            })
            .sum();
        let full = old_notes >= MAX_VOICES;
        let [mut wl, mut wr] = std::mem::take(&mut self.wave_scratch);
        wl.resize(n, 0.0);
        wr.resize(n, 0.0);
        for h in 0..HANDLES {
            let Some(b) = &mut self.beeps[h] else {
                continue;
            };
            let ended = match &mut b.sounding {
                Sounding::Preset => true,
                Sounding::Wave(note) => {
                    wl.fill(0.0);
                    wr.fill(0.0);
                    let mut at = 0;
                    while at < n {
                        let until = b.hold.map_or(n, |hold| (at + hold).min(n));
                        note.render(&mut wl[at..until], &mut wr[at..until]);
                        if let Some(hold) = &mut b.hold {
                            *hold -= until - at;
                            if *hold == 0 {
                                b.hold = note.end_note(b.pitch, b.pan, full);
                            }
                        }
                        at = until;
                    }
                    let volume = b.volume as f32 / WAVE_VOLUME;
                    for i in 0..n {
                        let gain = b.fade.value as f32 * volume;
                        left[i] += wl[i] * gain;
                        right[i] += wr[i] * gain;
                        b.fade.step_sample();
                    }
                    note.voice.done() && note.old.is_empty()
                }
                Sounding::Sequence {
                    player,
                    synth,
                    until_tick,
                    tail,
                } => {
                    player.begin_block();
                    let mut done = 0;
                    while done < n {
                        if *until_tick <= 0.0 && !player.ended() {
                            player.tick(synth.as_mut());
                            *until_tick += player.tick_samples(RATE);
                        }
                        let chunk = ((until_tick.ceil() as usize).max(1)).min(n - done);
                        synth.render(&mut wl[done..done + chunk], &mut wr[done..done + chunk]);
                        *until_tick -= chunk as f64;
                        done += chunk;
                    }
                    let volume = b.volume as f32 / WAVE_VOLUME * player.volume();
                    let (pl, pr) = balance(b.pan);
                    for i in 0..n {
                        let gain = volume * b.fade.value as f32;
                        left[i] += wl[i] * gain * pl;
                        right[i] += wr[i] * gain * pr;
                        b.fade.step_sample();
                    }
                    if player.ended() {
                        *tail = tail.saturating_sub(1);
                    }
                    *tail == 0
                }
                Sounding::Instrument { key, hold, release } => {
                    for _ in 0..n {
                        b.fade.step_sample();
                    }
                    if *hold > 0 {
                        *hold -= 1;
                        if *hold == 0
                            && let Some(s) = &mut self.synth
                        {
                            s.note_off(h, *key);
                        }
                        false
                    } else {
                        *release = release.saturating_sub(1);
                        *release == 0
                    }
                }
            };
            if ended || (b.stopping && b.fade.done()) {
                self.stop_now(h);
            }
        }
        self.wave_scratch = [wl, wr];
        if !self.falling.is_empty() {
            for v in &mut self.falling {
                v.render(left, right);
            }
            self.falling.retain(|v| !v.done() && v.audible());
        }
    }
}

/// Left and right gains that move a stereo sound towards one side: 64
/// keeps both, 0 and 127 silence the other side.
// UNVERIFIED: how BEEPPAN pans SmileBASIC 4's stereo effects.
fn balance(pan: i32) -> (f32, f32) {
    let p = pan.clamp(0, 127) as f32;
    (((127.0 - p) / 63.5).min(1.0), (p / 63.5).min(1.0))
}

/// What BEEP plays.
pub enum BeepSound {
    Preset,
    /// SmileBASIC 4's own effect, imported.
    Sequence(Arc<Sequence>),
    Instrument(i32),
    Wave(Rc<Wave>),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sound::wave::Envelope;

    /// A waveform that repeats a tick's note, whose release never ends.
    fn endless() -> BeepSound {
        BeepSound::Wave(Rc::new(Wave {
            beep_ticks: Some(1),
            beep_repeats: true,
            envelope: Envelope {
                release: 0,
                ..Envelope::ORGAN
            },
            ..wave::pulse(219)
        }))
    }

    fn render(beeps: &mut Beeps, frames: usize) {
        let (mut l, mut r) = (vec![0.0; 800], vec![0.0; 800]);
        for _ in 0..frames {
            beeps.render(&mut l, &mut r);
        }
    }

    fn pile_of(beeps: &Beeps, h: usize) -> usize {
        match &beeps.beeps[h].as_ref().unwrap().sounding {
            Sounding::Wave(note) => note.old.len(),
            _ => panic!("a waveform's note"),
        }
    }

    #[test]
    fn notes_that_never_end_stay_within_the_lists() {
        let mut beeps = Beeps::new(None);
        let h = beeps.play(224, endless(), 0, 64, 64, true).unwrap();
        render(&mut beeps, 120);
        assert_eq!(pile_of(&beeps, h), MAX_VOICES);
        beeps.stop(h, 0);
        assert!(beeps.beeps[h].is_none());
        assert_eq!(beeps.falling.len(), MAX_VOICES);
        // A second lot pushes the first out, and the memory stays as it was.
        let h = beeps.play(224, endless(), 0, 64, 64, true).unwrap();
        render(&mut beeps, 120);
        beeps.stop(h, 0);
        assert_eq!(beeps.falling.len(), MAX_VOICES);
        assert_eq!(beeps.falling.capacity(), MAX_VOICES);
        beeps.stop_all();
        assert!(beeps.falling.is_empty());
    }

    #[test]
    fn all_the_sounds_share_how_many_old_notes_they_may_have() {
        let mut beeps = Beeps::new(None);
        let handles: Vec<usize> = (0..16)
            .map(|_| beeps.play(224, endless(), 0, 64, 64, true).unwrap())
            .collect();
        render(&mut beeps, 600);
        let old: usize = handles.iter().map(|&h| pile_of(&beeps, h)).sum();
        assert!((MAX_VOICES..=MAX_VOICES + HANDLES).contains(&old), "{old}");
    }

    #[test]
    fn a_note_too_quiet_to_record_is_dropped() {
        let mut beeps = Beeps::new(None);
        let mut slow = wave::pulse(219);
        slow.envelope.release = 32;
        let h = beeps
            .play(224, BeepSound::Wave(Rc::new(slow)), 0, 64, 64, true)
            .unwrap();
        render(&mut beeps, 6);
        beeps.stop(h, 0);
        render(&mut beeps, 60);
        assert_eq!(beeps.falling.len(), 1);
        render(&mut beeps, 1800);
        assert!(beeps.falling.is_empty());
    }

    #[test]
    fn a_sound_without_the_flag_takes_no_list() {
        let mut beeps = Beeps::new(None);
        let wave = Rc::new(wave::pulse(219));
        let h = beeps
            .play(224, BeepSound::Wave(wave), 0, 64, 64, true)
            .unwrap();
        render(&mut beeps, 10);
        assert_eq!(pile_of(&beeps, h), 0);
        assert!(beeps.falling.capacity() == 0);
    }

    #[test]
    fn nothing_is_left_of_a_silent_sound() {
        let mut beeps = Beeps::new(None);
        let wave = Rc::new(wave::pulse(219));
        let h = beeps
            .play(224, BeepSound::Wave(wave), 0, 0, 64, false)
            .unwrap();
        beeps.stop(h, 0);
        assert_eq!(beeps.falling.capacity(), 0);
    }
}
