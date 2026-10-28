//! BEEP's sounds: up to 16 at once, each on a handle that BEEPPIT,
//! BEEPVOL, BEEPPAN and BEEPSTOP change.

use std::rc::Rc;

use super::Ramp;
use super::synth::{Instruments, Synth};
use super::wave::{self, Wave};

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
    synth: Option<Box<dyn Synth>>,
    beeps: Vec<Option<Beep>>,
    /// Which handle the next BEEP tries first, so that handles go round.
    next: usize,
    scratch: [Vec<f32>; 2],
}

impl Beeps {
    pub fn new(instruments: Option<&Instruments>) -> Beeps {
        Beeps {
            synth: instruments.map(Instruments::synth),
            beeps: vec![None; HANDLES],
            next: 0,
            scratch: Default::default(),
        }
    }

    /// Runs `f` on the synthesizer, if there is one.
    fn synth(&mut self, f: impl FnOnce(&mut dyn Synth)) {
        if let Some(s) = &mut self.synth {
            f(s.as_mut());
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
            Sounding::Instrument { .. } => self.synth(|s| {
                s.volume(h, volume);
                s.pan(h, pan);
            }),
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

    pub fn set_pitch(&mut self, h: usize, pitch: i32) {
        let Some(b) = &mut self.beeps[h] else {
            return;
        };
        match &mut b.sounding {
            Sounding::Wave(v) => v.set_pitch(pitch),
            Sounding::Instrument { .. } => {
                let from = b.start_pitch.div_euclid(100) * 100;
                self.synth(|s| s.bend(h, pitch - from));
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
            self.synth(|s| s.channel_off(h));
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
    }
}

/// What BEEP plays.
pub enum BeepSound {
    Preset,
    Instrument(i32),
    Wave(Rc<Wave>),
}
