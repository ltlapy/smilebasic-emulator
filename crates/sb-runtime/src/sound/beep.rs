//! BEEP's sounds: up to 16 at once, each on a handle that BEEPPIT,
//! BEEPVOL, BEEPPAN and BEEPSTOP change.

use std::rc::Rc;
use std::sync::Arc;

use rustysynth::{SoundFont, Synthesizer, SynthesizerSettings};

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
/// Semitones the pitch bend reaches either way, for BEEPPIT.
const BEND_RANGE: i32 = 24;
/// rustysynth's drum channel, which adds 128 to the bank.
const DRUM_CHANNEL: usize = 9;

#[derive(Clone, Debug)]
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
    Wave(wave::Voice),
}

#[derive(Clone, Debug)]
struct Beep {
    sounding: Sounding,
    /// The pitch at the start, in cents: what BEEPPIT bends from.
    start_pitch: i32,
    volume: i32,
    pan: i32,
    fade: Ramp,
    stopping: bool,
}

pub struct Beeps {
    synth: Option<Synthesizer>,
    beeps: Vec<Option<Beep>>,
    /// Which handle the next BEEP tries first, so that handles go round.
    next: usize,
    scratch: [Vec<f32>; 2],
}

impl Beeps {
    pub fn new(font: Option<&Arc<SoundFont>>) -> Beeps {
        let mut settings = SynthesizerSettings::new(RATE as i32);
        settings.enable_reverb_and_chorus = false;
        let mut synth = font.map(|f| Synthesizer::new(f, &settings).expect("valid settings"));
        if let Some(s) = &mut synth {
            for c in 0..HANDLES as i32 {
                // RPN 0, the pitch bend's range.
                s.process_midi_message(c, 0xB0, 0x65, 0);
                s.process_midi_message(c, 0xB0, 0x64, 0);
                s.process_midi_message(c, 0xB0, 0x06, BEND_RANGE);
            }
        }
        Beeps {
            synth,
            beeps: vec![None; HANDLES],
            next: 0,
            scratch: Default::default(),
        }
    }

    fn midi(&mut self, handle: usize, command: i32, a: i32, b: i32) {
        if let Some(s) = &mut self.synth {
            s.process_midi_message(handle as i32, command, a, b);
        }
    }

    /// A free handle, or the one taken longest ago.
    fn handle(&mut self) -> usize {
        let h = (0..HANDLES)
            .map(|i| (self.next + i) % HANDLES)
            .find(|&h| self.beeps[h].is_none())
            .unwrap_or(self.next);
        self.next = (h + 1) % HANDLES;
        self.stop_now(h);
        h
    }

    /// Applies a handle's volume and pan to what it plays.
    fn mix(&mut self, h: usize) {
        let Some(b) = &mut self.beeps[h] else {
            return;
        };
        let (volume, pan) = (b.volume, b.pan);
        match &mut b.sounding {
            Sounding::Wave(v) => v.gains = wave::gains(volume, 127, pan),
            Sounding::Instrument { .. } => {
                self.midi(h, 0xB0, 7, volume);
                self.midi(h, 0xB0, 10, pan);
            }
            Sounding::Preset => {}
        }
    }

    /// Plays an instrument (`program` 0 to 127) or a waveform, or holds a
    /// handle for a preset effect, and gives the handle.
    pub fn play(&mut self, what: BeepSound, pitch: i32, volume: i32, pan: i32) -> usize {
        let h = self.handle();
        let sounding = match what {
            BeepSound::Preset => Sounding::Preset,
            BeepSound::Instrument(program) => {
                let key = INSTRUMENT_NOTE + pitch.div_euclid(100);
                let bank = if h == DRUM_CHANNEL { -128 } else { 0 };
                self.midi(h, 0xB0, 0, bank);
                self.midi(h, 0xC0, program, 0);
                self.bend(h, pitch.rem_euclid(100));
                if (0..128).contains(&key) {
                    if let Some(s) = &mut self.synth {
                        s.note_on(h as i32, key, 127);
                    }
                }
                Sounding::Instrument {
                    key,
                    hold: INSTRUMENT_FRAMES,
                    release: RELEASE_FRAMES,
                }
            }
            BeepSound::Wave(w) => {
                let key = w.base_note;
                let looping = w.beep_loop;
                Sounding::Wave(wave::Voice::new(w, None, key, pitch, looping))
            }
        };
        self.beeps[h] = Some(Beep {
            sounding,
            start_pitch: pitch,
            volume,
            pan,
            fade: Ramp::at(1.0),
            stopping: false,
        });
        self.mix(h);
        h
    }

    fn bend(&mut self, h: usize, cents: i32) {
        let cents = cents.clamp(-BEND_RANGE * 100, BEND_RANGE * 100);
        let bend = (8192 + cents * 8192 / (BEND_RANGE * 100)).clamp(0, 16383);
        self.midi(h, 0xE0, bend & 0x7F, bend >> 7);
    }

    pub fn set_pitch(&mut self, h: usize, pitch: i32) {
        let Some(b) = &mut self.beeps[h] else {
            return;
        };
        match &mut b.sounding {
            Sounding::Wave(v) => v.set_pitch(pitch),
            Sounding::Instrument { .. } => {
                let from = b.start_pitch.div_euclid(100) * 100;
                self.bend(h, pitch - from);
            }
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

    fn stop_now(&mut self, h: usize) {
        if let Some(Beep {
            sounding: Sounding::Instrument { .. },
            ..
        }) = self.beeps[h]
        {
            if let Some(s) = &mut self.synth {
                s.note_off_all_channel(h as i32, true);
            }
        }
        self.beeps[h] = None;
    }

    /// Stops a handle's sound, fading over `frames`.
    pub fn stop(&mut self, h: usize, frames: u32) {
        match &mut self.beeps[h] {
            Some(b) if frames > 0 => {
                b.fade.to(0.0, frames);
                b.stopping = true;
            }
            _ => self.stop_now(h),
        }
    }

    pub fn stop_all(&mut self) {
        for h in 0..HANDLES {
            self.stop_now(h);
        }
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
                self.midi(h, 0xB0, 11, expression);
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
        let (mut wl, mut wr) = (vec![0.0; n], vec![0.0; n]);
        for h in 0..HANDLES {
            let Some(b) = &mut self.beeps[h] else {
                continue;
            };
            let ended = match &mut b.sounding {
                Sounding::Preset => true,
                Sounding::Wave(v) => {
                    wl.fill(0.0);
                    wr.fill(0.0);
                    v.render(&mut wl, &mut wr);
                    for i in 0..n {
                        let gain = b.fade.value as f32;
                        left[i] += wl[i] * gain;
                        right[i] += wr[i] * gain;
                        b.fade.step_sample();
                    }
                    v.done()
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
                            s.note_off(h as i32, *key);
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
    }
}

/// What BEEP plays.
pub enum BeepSound {
    Preset,
    Instrument(i32),
    Wave(Rc<Wave>),
}
