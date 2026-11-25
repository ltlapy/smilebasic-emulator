//! BEEP's sounds: up to 16 at the same time, each on a handle that
//! BEEPPIT, BEEPVOL, BEEPPAN and BEEPSTOP change.

use std::rc::Rc;
use std::sync::Arc;

use super::decoded::{Decoded, Shared};
use super::inspect::{BeepInfo, BeepKind};
use super::sampler::{MAX_VOICES, STEP_SAMPLES};
use super::seq::{Player, Sequence};
use super::synth::{Instruments, Synth};
use super::wave::{self, Wave};
use super::{RATE, Ramp, add_scaled};

pub const HANDLES: usize = 16;
/// The note BEEP plays instruments at.
// UNVERIFIED: SmileBASIC's note for BEEP 256 to 383.
const INSTRUMENT_NOTE: i32 = 69;
/// The frames an instrument sounds before it is released.
// UNVERIFIED: how long BEEP holds an instrument's note.
const INSTRUMENT_FRAMES: u32 = 30;
/// Frames an instrument's handle stays taken after the release, for the
/// note to die away.
const RELEASE_FRAMES: u32 = 60;
/// The BEEP volume at which a waveform or preset effect plays unchanged,
/// the same as BGMPLAY's volume: it is a linear gain, not squared like
/// MML's `V`.
const WAVE_VOLUME: f32 = 64.0;
/// The level of an instrument's note at BEEP's default, relative to the
/// note at `V127`.
const INSTRUMENT_LEVEL: f32 = 70.0 / 127.0;

/// The channel volume for BEEP's `volume` on an instrument, which squares it:
/// BEEP's is linear, up to all the channel has.
pub(crate) fn instrument_volume(volume: i32) -> i32 {
    let level = volume as f32 / WAVE_VOLUME * INSTRUMENT_LEVEL;
    (127.0 * level.sqrt()).round().min(127.0) as i32
}

/// Samples from the start of a waveform's sound to the end of its note, at
/// `ticks` of [`wave::BEEP_TICKS_PER_SECOND`]. The sound engine advances one
/// step at a time, 0.96 of these ticks per step, the first step included.
/// The note's level then takes one more step to fall.
fn note_end(ticks: u64) -> usize {
    // A waveform WAVSETA gives 1 Hz holds its note for billions of ticks, more samples
    // than a 32-bit usize counts.
    let samples = ((ticks * 25).div_ceil(24) - 1).saturating_mul(STEP_SAMPLES as u64);
    usize::try_from(samples).unwrap_or(usize::MAX)
}

/// Adds `voice` to `notes`, which holds [`MAX_VOICES`] at most: the oldest
/// is removed. The list allocates its memory once, when the first note is
/// added.
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

    /// Ends the note. With the repeat flag, the next pass starts in the same
    /// place while the old notes die away over it, however long their
    /// release takes, and this returns the samples until that pass ends.
    /// With `full`, set when all the sounds have as many old notes as they
    /// may, the oldest of this note's old notes is removed.
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
    /// A preset effect: a sequence on its own synthesizer, because it may
    /// take several channels. `tail` counts the frames left after it
    /// ends, for its notes to die away.
    Sequence {
        player: Box<Player>,
        synth: Box<dyn Synth>,
        until_tick: f64,
        tail: u32,
        /// The synthesizer pans the voices itself.
        own_pan: bool,
    },
}

struct Beep {
    sounding: Sounding,
    /// BEEP's sound number, and the pitch BEEPPIT last set, for the
    /// debugger.
    sound: i32,
    pitch: i32,
    /// The pitch at the start, in cents: the pitch BEEPPIT bends from.
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
    /// The waveforms kept decoded, which the sound system's synthesizers
    /// share.
    decoded: Shared,
    synth: Option<Box<dyn Synth>>,
    beeps: Vec<Option<Beep>>,
    /// The handle the next BEEP tries first. It keeps rising, whether the
    /// last has ended or not, and wraps around after the last handle.
    next: usize,
    /// Notes of waveforms whose handles have gone, falling as the releases
    /// of their envelopes say, at the gain they had.
    falling: Vec<wave::Voice>,
    scratch: [Vec<f32>; 2],
    wave_scratch: [Vec<f32>; 2],
}

impl Beeps {
    pub fn new(instruments: Option<&Instruments>) -> Beeps {
        Beeps::sharing(instruments, &Decoded::new())
    }

    /// BEEP's sounds on `instruments`, keeping the waveforms they decode in
    /// `decoded`.
    pub(crate) fn sharing(instruments: Option<&Instruments>, decoded: &Shared) -> Beeps {
        Beeps {
            instruments: instruments.cloned(),
            decoded: decoded.clone(),
            synth: instruments.map(|i| i.synth(decoded)),
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

    /// The next handle that no program holds to a sound still playing,
    /// which ends anything that played on it before; `None` if all are
    /// held. A handle nobody took back goes to the next BEEP that gets
    /// there.
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
            Sounding::Sequence { synth, own_pan, .. } => *own_pan = synth.pan_offset(pan),
            Sounding::Preset => {}
        }
    }

    /// Plays BEEP's `sound`, an instrument (`program` 0 to 127) or a
    /// waveform, or holds a handle for a preset effect, and returns the
    /// handle. With `keep`, the program holds it while the sound continues.
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
                    let mut synth = instruments.synth(&self.decoded);
                    let mut player = Box::new(Player::new(seq));
                    player.set_pitch(pitch, synth.as_mut());
                    Sounding::Sequence {
                        player,
                        synth,
                        until_tick: 0.0,
                        tail: RELEASE_FRAMES,
                        own_pan: false,
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

    /// Ends the sound on a handle, which frees the handle. A waveform's
    /// notes are released and fall on their own, as their releases say: at
    /// release 127 that is one step of the sound engine.
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
        // A synthesizer with nothing sounding would add zeros, which leave
        // the buffers unchanged: they start at 0 and only take sums, and so
        // never hold -0.
        if let Some(s) = self.synth.as_mut().filter(|s| !s.silent()) {
            let [l, r] = &mut self.scratch;
            l.clear();
            l.resize(n, 0.0);
            r.clear();
            r.resize(n, 0.0);
            s.render(l, r);
            for (to, from) in [(&mut *left, &*l), (&mut *right, &*r)] {
                to.iter_mut().zip(from).for_each(|(t, &x)| *t += x);
            }
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
                        let until = b.hold.map_or(n, |hold| at.saturating_add(hold).min(n));
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
                    if b.fade.done() {
                        let gain = b.fade.value as f32 * volume;
                        add_scaled(left, &wl, gain);
                        add_scaled(right, &wr, gain);
                    } else {
                        for i in 0..n {
                            let gain = b.fade.value as f32 * volume;
                            left[i] += wl[i] * gain;
                            right[i] += wr[i] * gain;
                            b.fade.step_sample();
                        }
                    }
                    note.voice.done() && note.old.is_empty()
                }
                Sounding::Sequence {
                    player,
                    synth,
                    until_tick,
                    tail,
                    own_pan,
                } => {
                    player.begin_block();
                    // Whether a note may have sounded.
                    let mut sounded = false;
                    let mut done = 0;
                    while done < n {
                        let ended = player.ended();
                        if *until_tick <= 0.0 && !ended {
                            player.tick(synth.as_mut());
                            *until_tick += player.tick_samples(RATE);
                        }
                        // Once the sequence has ended, no tick comes, and the
                        // notes die away in one run: rendered one sample at a
                        // time, the tail's 60 frames cost more than the effect
                        // did.
                        let chunk = match ended {
                            true => n - done,
                            false => ((until_tick.ceil() as usize).max(1)).min(n - done),
                        };
                        sounded |= !synth.silent();
                        synth.render(&mut wl[done..done + chunk], &mut wr[done..done + chunk]);
                        *until_tick -= chunk as f64;
                        done += chunk;
                    }
                    let volume = b.volume as f32 / WAVE_VOLUME * player.volume();
                    let (pl, pr) = match own_pan {
                        true => (1.0, 1.0),
                        false => balance(b.pan),
                    };
                    if !sounded {
                        // Zeros, which leave the buffers unchanged.
                        if !b.fade.done() {
                            (0..n).for_each(|_| b.fade.step_sample());
                        }
                    } else if b.fade.done() {
                        let gain = volume * b.fade.value as f32;
                        for (to, from, side) in [(&mut *left, &wl, pl), (&mut *right, &wr, pr)] {
                            for (t, &x) in to.iter_mut().zip(from) {
                                *t += x * gain * side;
                            }
                        }
                    } else {
                        for i in 0..n {
                            let gain = volume * b.fade.value as f32;
                            left[i] += wl[i] * gain * pl;
                            right[i] += wr[i] * gain * pr;
                            b.fade.step_sample();
                        }
                    }
                    if player.ended() {
                        *tail = tail.saturating_sub(1);
                    }
                    *tail == 0
                }
                Sounding::Instrument { key, hold, release } => {
                    if !b.fade.done() {
                        (0..n).for_each(|_| b.fade.step_sample());
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

/// Left and right gains that move a stereo sound toward one side: 64
/// keeps both, 0 and 127 silence the other side. Only for SoundFonts,
/// whose synthesizer cannot pan an effect's voices.
fn balance(pan: i32) -> (f32, f32) {
    let p = pan.clamp(0, 127) as f32;
    (((127.0 - p) / 63.5).min(1.0), (p / 63.5).min(1.0))
}

/// The sound BEEP plays.
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
    fn long_notes_do_not_wrap_round() {
        let ticks = u64::from(u32::MAX);
        let samples = ((ticks * 25).div_ceil(24) - 1) * STEP_SAMPLES as u64;
        assert_eq!(note_end(ticks) as u64, samples.min(usize::MAX as u64));
        assert!(note_end(ticks) >= note_end(ticks / 2));
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
        // A second set pushes the first out, and the memory stays unchanged.
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

    /// A preset effect that ends dies away in one run of samples per frame,
    /// bit for bit the same as it did one sample at a time.
    #[test]
    fn an_effect_dies_away_as_it_did_a_sample_at_a_time() {
        use crate::sound::sampler::tests::{ORGAN, bank};
        let instruments = Instruments::Bank(Arc::new(bank(ORGAN, false)));
        // A note of 16 ticks, then the end.
        let seq = Arc::new(Sequence {
            volume: 100,
            start: 0,
            code: vec![60, 127, 0x10, 0xFF],
        });
        let n = 800;
        let mut beeps = Beeps::new(Some(&instruments));
        beeps.play(0, BeepSound::Sequence(seq.clone()), 0, 64, 64, false);
        // The old loop, which rendered the tail one sample at a time.
        let mut synth = instruments.synth(&Decoded::new());
        let mut player = Player::new(seq);
        player.set_pitch(0, synth.as_mut());
        let own_pan = synth.pan_offset(64);
        assert!(own_pan);
        let mut until_tick = 0.0;
        let (mut wl, mut wr) = (vec![0.0; n], vec![0.0; n]);
        let mut heard = false;
        for frame in 0.. {
            assert!(frame < 200, "the effect ends");
            if beeps.beeps[0].is_none() {
                break;
            }
            let (mut l, mut r) = (vec![0.0; n], vec![0.0; n]);
            beeps.render(&mut l, &mut r);
            heard |= l.iter().any(|&s| s != 0.0);
            player.begin_block();
            let mut done = 0;
            while done < n {
                if until_tick <= 0.0 && !player.ended() {
                    player.tick(synth.as_mut());
                    until_tick += player.tick_samples(RATE);
                }
                let chunk = ((until_tick.ceil() as usize).max(1)).min(n - done);
                synth.render(&mut wl[done..done + chunk], &mut wr[done..done + chunk]);
                until_tick -= chunk as f64;
                done += chunk;
            }
            let gain = 64.0 / WAVE_VOLUME * player.volume() * 1.0f32;
            let bits = |v: &[f32]| v.iter().map(|s| s.to_bits()).collect::<Vec<_>>();
            let expect = |w: &[f32]| w.iter().map(|s| 0.0 + s * gain * 1.0).collect::<Vec<_>>();
            assert_eq!(bits(&l), bits(&expect(&wl)), "frame {frame}");
            assert_eq!(bits(&r), bits(&expect(&wr)), "frame {frame}");
        }
        assert!(heard && player.ended());
    }

    /// With nothing sounding, the instruments' synthesizer is not run: its
    /// buffers stay untouched.
    #[test]
    fn a_silent_synthesizer_is_left_out() {
        use crate::sound::sampler::tests::{ORGAN, bank};
        let instruments = Instruments::Bank(Arc::new(bank(ORGAN, false)));
        let mut beeps = Beeps::new(Some(&instruments));
        render(&mut beeps, 3);
        assert!(beeps.scratch.iter().all(|b| b.capacity() == 0));
        beeps.play(256, BeepSound::Instrument(0), 0, 64, 64, false);
        render(&mut beeps, 1);
        assert!(beeps.scratch.iter().all(|b| b.len() == 800));
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
