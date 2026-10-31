//! Sound commands: BGMPLAY, BGMSET, BGMSETD, BGMSTOP, BGMCHK, BGMVOL,
//! BGMPITCH, BGMPAUSE, BGMCONT, BGMVAR, BGMCLEAR, BGMWET, CHKMML, SNDSTOP,
//! SNDMVOL, WAVSET, WAVSETA, BEEP, BEEPPIT, BEEPVOL, BEEPPAN, BEEPSTOP,
//! PCMSTREAM, PCMSTOP, PCMCONT, PCMPOS, PCMVOL, EFCEN, EFCSET, EFCWET,
//! SNDMSBAL, TALK, TALKCHK, TALKSTOP, and RECSTART and the commands that
//! read its recording.

use std::rc::Rc;

use sb_core::bytecode::{Literal, Want};
use sb_core::error::{Error, ErrorKind, Result};
use sb_core::value::{Array, Value};

use crate::runtime::{Command, Runtime, illegal, no_form};
use crate::sound::beep::{BeepSound, HANDLES};
use crate::sound::mml::{self, Song};
use crate::sound::pcm::{FULL, Source};
use crate::sound::seq::Kind;
use crate::sound::wave::{Envelope, Wave};
use crate::sound::{PRESET_BEEPS, PRESET_SONGS, RATE, Recorder, TRACKS, frames};

/// The preset songs that end rather than loop, and their lengths in
/// frames, worked out by playing SmileBASIC 4's own sequences through the
/// emulator's player; every other preset loops until stopped.
// UNVERIFIED: whether BGMCHK also waits for the last notes to die away.
const JINGLES: [(i32, u32); 3] = [(4, 167), (5, 111), (6, 342)];

fn int(v: &Value) -> Result<i32> {
    v.to_int()
}

fn out_of_range(what: &str, n: i32) -> Error {
    Error::with_detail(ErrorKind::OutOfRange, format!("{what} {n}"))
}

fn in_range(what: &str, v: &Value, range: std::ops::RangeInclusive<i32>) -> Result<i32> {
    let n = int(v)?;
    if range.contains(&n) {
        Ok(n)
    } else {
        Err(out_of_range(what, n))
    }
}

fn track_id(v: &Value) -> Result<usize> {
    in_range("track", v, 0..=TRACKS as i32 - 1).map(|n| n as usize)
}

fn user_song(v: &Value) -> Result<i32> {
    in_range("song", v, 128..=255)
}

/// Seconds, real allowed; 0 when left out.
fn seconds(v: Option<&Value>) -> Result<u32> {
    match v {
        None | Some(Value::Default) => Ok(0),
        Some(v) => {
            let s = v.to_real()?;
            if s < 0.0 {
                return Err(Error::with_detail(ErrorKind::OutOfRange, "time"));
            }
            Ok(frames(s))
        }
    }
}

fn handle(v: &Value) -> Result<usize> {
    in_range("handle", v, 0..=HANDLES as i32 - 1).map(|n| n as usize)
}

/// An omittable argument in `range`, `default` when left out.
fn opt_in_range(
    what: &str,
    v: Option<&Value>,
    range: std::ops::RangeInclusive<i32>,
    default: i32,
) -> Result<i32> {
    match v {
        None | Some(Value::Default) => Ok(default),
        Some(v) => in_range(what, v, range),
    }
}

/// WAVSET's instrument number and envelope.
fn wave_head(args: &[Value]) -> Result<(usize, Envelope)> {
    let n = in_range("instrument", &args[0], 224..=255)? as usize - 224;
    let [attack, decay, sustain, release] =
        [1, 2, 3, 4].map(|i| in_range("envelope", &args[i], 0..=127));
    Ok((
        n,
        Envelope {
            attack: attack?,
            decay: decay?,
            sustain: sustain?,
            release: release?,
        },
    ))
}

/// The numbers in a one-dimensional array.
fn samples(v: &Value) -> Result<Vec<f64>> {
    let a = v.clone().into_array()?;
    let a = a.borrow();
    if a.dims.len() != 1 {
        return Err(Error::with_detail(
            ErrorKind::IllegalFunctionCall,
            "1D array",
        ));
    }
    (0..a.len()).map(|i| a.get(i).to_real()).collect()
}

fn text(v: &Value) -> Result<String> {
    Ok(String::from_utf16_lossy(&v.clone().into_str()?.borrow()))
}

fn parse(mml: &str) -> Result<Song> {
    mml::parse(mml).map_err(|at| Error::with_detail(ErrorKind::IllegalMml, format!("at {at}")))
}

/// An omittable track, then the arguments after it: `[track,] rest...`
/// takes `rest` arguments, the first of them required.
fn split_track(args: &[Value], rest: usize) -> Result<(Option<usize>, &[Value])> {
    if args.len() > rest {
        Ok((Some(track_id(&args[0])?), &args[1..]))
    } else {
        Ok((None, args))
    }
}

impl Runtime {
    /// Every track `track` names: one, or all of them.
    fn tracks_named(track: Option<usize>) -> std::ops::Range<usize> {
        match track {
            Some(t) => t..t + 1,
            None => 0..TRACKS,
        }
    }

    pub(crate) fn sound_command(
        &mut self,
        command: Command,
        args: &[Value],
        want: Want,
    ) -> Result<Vec<Value>> {
        match (command, want, args.len()) {
            // BGMPLAY "MML" plays on track 0 as song 255.
            (Command::Bgmplay, Want::Outs(0), 1) if matches!(args[0], Value::Str(_)) => {
                let song = Rc::new(parse(&text(&args[0])?)?);
                self.sound.songs.insert(255, song.clone());
                let idle = self.sound.idle();
                self.sound.play(0, Some(song), 127, 0);
                if idle {
                    self.sound.wake(0);
                }
                Ok(vec![])
            }
            (Command::Bgmplay, Want::Outs(0), 1..=4) => {
                let (track, music) = match args.len() {
                    1 => (0, int(&args[0])?),
                    _ => (track_id(&args[0])?, int(&args[1])?),
                };
                let volume = match args.get(2) {
                    None | Some(Value::Default) => 127,
                    Some(v) => in_range("volume", v, 0..=127)?,
                };
                let fade = match args.get(3) {
                    None => 0,
                    Some(v) => frames(f64::from(in_range("time", v, 0..=32767)?)),
                };
                let song = match music {
                    0..=PRESET_SONGS => {
                        let idle = self.sound.idle();
                        match self.sound.sequence(Kind::Song, music) {
                            Some(seq) => self.sound.track(track).play_sequence(seq, volume, fade),
                            // Without SmileBASIC's own songs, presets play
                            // silence, so programs waiting on BGMCHK still
                            // wait.
                            None => self.sound.play(track, None, volume, fade),
                        }
                        if let Some(&(_, n)) = JINGLES.iter().find(|j| j.0 == music) {
                            self.sound.track(track).end_silent_after(n);
                        }
                        if idle {
                            self.sound.wake(track);
                        }
                        return Ok(vec![]);
                    }
                    // A song number with nothing set plays nothing, so
                    // BGMCHK says 0 (checked on hardware).
                    // UNVERIFIED: whether it stops what the track played.
                    128..=255 => match self.sound.songs.get(&music) {
                        Some(song) => Some(Rc::clone(song)),
                        None => return Ok(vec![]),
                    },
                    n => return Err(out_of_range("song", n)),
                };
                let idle = self.sound.idle();
                self.sound.play(track, song, volume, fade);
                if idle {
                    self.sound.wake(track);
                }
                Ok(vec![])
            }
            (Command::Bgmset, Want::Outs(0), 2) => {
                let n = user_song(&args[0])?;
                let song = parse(&text(&args[1])?)?;
                self.sound.songs.insert(n, Rc::new(song));
                Ok(vec![])
            }
            // The MML is strings of DATA up to a 0.
            (Command::Bgmsetd, Want::Outs(0), 2) => {
                let n = user_song(&args[0])?;
                let mut mml = String::new();
                let mut data = self.label_data(&args[1])?;
                // The MML ends at a number, or after the program's last
                // DATA.
                while !data.at_end() {
                    match data.literal()? {
                        Literal::Str(s) => mml.push_str(&String::from_utf16_lossy(&s)),
                        Literal::Int(_) | Literal::Real(_) => break,
                    }
                }
                let song = parse(&mml)?;
                self.sound.songs.insert(n, Rc::new(song));
                Ok(vec![])
            }
            (Command::Bgmstop, Want::Outs(0), 0..=2) => {
                let track = args.first().map(track_id).transpose()?;
                let fade = seconds(args.get(1))?;
                for t in Self::tracks_named(track) {
                    self.sound.stop(t, fade);
                }
                Ok(vec![])
            }
            (Command::Bgmpause, Want::Outs(0), 0..=2) => {
                let track = args.first().map(track_id).transpose()?;
                let fade = seconds(args.get(1))?;
                for t in Self::tracks_named(track) {
                    self.sound.pause(t, fade);
                }
                Ok(vec![])
            }
            (Command::Bgmcont, Want::Outs(0), 0..=2) => {
                let track = args.first().map(track_id).transpose()?;
                let fade = seconds(args.get(1))?;
                for t in Self::tracks_named(track) {
                    self.sound.resume(t, fade);
                }
                Ok(vec![])
            }
            (Command::Bgmchk, Want::Outs(1) | Want::Value, 0..=1) => {
                let track = args.first().map(track_id).transpose()?.unwrap_or(0);
                Ok(vec![Value::Int(i32::from(self.sound.playing(track)))])
            }
            (Command::Bgmvol, Want::Outs(0), 1..=2) => {
                let (track, rest) = split_track(args, 1)?;
                let volume = in_range("volume", &rest[0], 0..=127)?;
                self.sound.track(track.unwrap_or(0)).volume = volume;
                Ok(vec![])
            }
            (Command::Bgmwet, Want::Outs(0), 1..=2) => {
                let (track, rest) = split_track(args, 1)?;
                // UNVERIFIED: kept for the effector, which does not play.
                let wet = in_range("wet", &rest[0], 0..=127)?;
                self.sound.track(track.unwrap_or(0)).wet = wet;
                Ok(vec![])
            }
            // Without a track, BGMPITCH sets every track's.
            (Command::Bgmpitch, Want::Outs(0), 1..=2) => {
                let (track, rest) = split_track(args, 1)?;
                let cents = in_range("pitch", &rest[0], -32768..=32767)?;
                for t in Self::tracks_named(track) {
                    self.sound.track(t).set_pitch(cents);
                }
                Ok(vec![])
            }
            (Command::Bgmvar, Want::Outs(0), 3) => {
                let track = track_id(&args[0])?;
                let n = in_range("variable", &args[1], 0..=7)? as usize;
                let v = in_range("value", &args[2], -32768..=32767)?;
                self.sound.set_var(track, n, v);
                Ok(vec![])
            }
            (Command::Bgmvar, Want::Outs(1) | Want::Value, 2) => {
                let track = track_id(&args[0])?;
                let n = in_range("variable", &args[1], 0..=7)? as usize;
                Ok(vec![Value::Int(self.sound.var(track, n))])
            }
            (Command::Bgmclear, Want::Outs(0), 0..=1) => {
                match args.first() {
                    None => self.sound.songs.clear(),
                    Some(v) => {
                        self.sound.songs.remove(&user_song(v)?);
                    }
                }
                Ok(vec![])
            }
            (Command::Chkmml, Want::Outs(1) | Want::Value, 1) => {
                let at = match mml::parse(&text(&args[0])?) {
                    Ok(_) => -1,
                    Err(at) => at as i32,
                };
                Ok(vec![Value::Int(at)])
            }
            (Command::Sndstop, Want::Outs(0), 0) => {
                self.sound.stop_all();
                Ok(vec![])
            }
            (Command::Sndmvol, Want::Outs(0), 1..=2) => {
                let volume = in_range("volume", &args[0], 0..=127)?;
                let fade = match args.get(1) {
                    None => 0,
                    Some(v) => frames(f64::from(in_range("time", v, 0..=32767)?)),
                };
                self.sound.master.to(f64::from(volume) / 127.0, fade);
                Ok(vec![])
            }
            (Command::Sndmvol, Want::Outs(1), 0) => Ok(vec![Value::Int(
                (self.sound.master.value * 127.0).round() as i32,
            )]),
            (Command::Wavset, Want::Outs(0), 6..=7) => {
                let (n, envelope) = wave_head(args)?;
                let hex = text(&args[5])?;
                let bytes: Option<Vec<u8>> = (0..hex.len() / 2)
                    .map(|i| {
                        hex.get(2 * i..2 * i + 2)
                            .and_then(|h| u8::from_str_radix(h, 16).ok())
                    })
                    .collect();
                let bytes = bytes
                    .filter(|b| {
                        hex.len() % 2 == 0 && [16, 32, 64, 128, 256, 512].contains(&b.len())
                    })
                    .ok_or_else(|| {
                        Error::with_detail(ErrorKind::IllegalFunctionCall, "waveform")
                    })?;
                let base_note = opt_in_range("pitch", args.get(6), 0..=127, 69)?;
                let samples: Vec<f32> = bytes
                    .iter()
                    .map(|&b| (f32::from(b) - 128.0) / 128.0)
                    .collect();
                let len = samples.len();
                self.sound.waves.borrow_mut().user[n] = Some(Rc::new(Wave {
                    samples,
                    base_note,
                    rate: None,
                    looped: (0, len),
                    beep_loop: true,
                    envelope,
                }));
                Ok(vec![])
            }
            // SmileBASIC 3's form: one cycle of 8-bit samples.
            (Command::Wavseta, Want::Outs(0), 6..=9) => {
                let (n, envelope) = wave_head(args)?;
                let all = samples(&args[5])?;
                let base_note = opt_in_range("pitch", args.get(6), 0..=127, 69)?;
                let last = all.len() as i32 - 1;
                let start = opt_in_range("start", args.get(7), 0..=last, 0)? as usize;
                let end = opt_in_range("end", args.get(8), start as i32..=last, last)? as usize;
                let samples: Vec<f32> = all[start..=end]
                    .iter()
                    .map(|&x| (x as f32 - 128.0) / 128.0)
                    .collect();
                let len = samples.len();
                self.sound.waves.borrow_mut().user[n] = Some(Rc::new(Wave {
                    samples,
                    base_note,
                    rate: None,
                    looped: (0, len),
                    beep_loop: true,
                    envelope,
                }));
                Ok(vec![])
            }
            // A 16-bit sample with a loop and its own sample rate.
            (Command::Wavseta, Want::Outs(0), 10..=12) => {
                let (n, envelope) = wave_head(args)?;
                let all = samples(&args[5])?;
                let base_note = opt_in_range("pitch", args.get(6), 0..=127, 69)?;
                let last = all.len() as i32 - 1;
                let offset = opt_in_range("offset", args.get(10), 0..=last.max(0), 0)? as usize;
                let loop_start = opt_in_range("loop start", args.get(7), 0..=last, 0)? as usize;
                let loop_end = opt_in_range("loop end", args.get(8), 0..=last, last)? as usize;
                let rate = opt_in_range("frequency", args.get(9), 1..=96000, 28160)?;
                let beep_loop = opt_in_range("flag", args.get(11), 0..=1, 0)? == 1;
                let samples: Vec<f32> = all[offset..].iter().map(|&x| x as f32 / 32768.0).collect();
                // UNVERIFIED: the loop's indices count from the array's start,
                // offset included, and its end is the last sample played.
                let looped = (
                    loop_start.saturating_sub(offset),
                    (loop_end + 1).saturating_sub(offset).max(1),
                );
                self.sound.waves.borrow_mut().user[n] = Some(Rc::new(Wave {
                    samples,
                    base_note,
                    rate: Some(f64::from(rate)),
                    looped,
                    beep_loop,
                    envelope,
                }));
                Ok(vec![])
            }
            (Command::Beep, Want::Outs(0) | Want::Value, 0..=4) => {
                let n = opt_in_range("sound", args.first(), 0..=383, 0)?;
                let pitch = opt_in_range("pitch", args.get(1), -32768..=32767, 0)?;
                let volume = opt_in_range("volume", args.get(2), 0..=127, 64)?;
                let pan = opt_in_range("pan", args.get(3), 0..=127, 64)?;
                let what = match n {
                    0..=PRESET_BEEPS | 256..=383
                        if let Some(seq) = self.sound.sequence(Kind::Beep, n) =>
                    {
                        BeepSound::Sequence(seq)
                    }
                    // UNVERIFIED: without SmileBASIC's own sounds, presets are
                    // silent.
                    0..=PRESET_BEEPS => BeepSound::Preset,
                    224..=255 => match self.sound.waves.borrow().get(n) {
                        Some(w) => BeepSound::Wave(w),
                        None => BeepSound::Preset,
                    },
                    256..=383 => BeepSound::Instrument(n - 256),
                    n => return Err(out_of_range("sound", n)),
                };
                let h = self.sound.beeps.play(what, pitch, volume, pan);
                Ok(match want {
                    Want::Value => vec![Value::Int(h as i32)],
                    _ => vec![],
                })
            }
            (Command::Beeppit, Want::Outs(0), 2) => {
                let h = handle(&args[0])?;
                let pitch = in_range("pitch", &args[1], -32768..=32767)?;
                self.sound.beeps.set_pitch(h, pitch);
                Ok(vec![])
            }
            (Command::Beepvol, Want::Outs(0), 2) => {
                let h = handle(&args[0])?;
                let volume = in_range("volume", &args[1], 0..=127)?;
                self.sound.beeps.set_volume(h, volume);
                Ok(vec![])
            }
            (Command::Beeppan, Want::Outs(0), 2) => {
                let h = handle(&args[0])?;
                let pan = in_range("pan", &args[1], 0..=127)?;
                self.sound.beeps.set_pan(h, pan);
                Ok(vec![])
            }
            (Command::Beepstop, Want::Outs(0), 0..=2) => {
                let fade = seconds(args.get(1))?;
                match args.first() {
                    None => (0..HANDLES).for_each(|h| self.sound.beeps.stop(h, fade)),
                    Some(v) => self.sound.beeps.stop(handle(v)?, fade),
                }
                Ok(vec![])
            }
            (Command::Pcmstream, Want::Outs(0), 1..=3) => {
                let arrays: Vec<_> = args
                    .iter()
                    .take_while(|a| matches!(a, Value::Array(_)))
                    .map(|a| a.clone().into_array())
                    .collect::<Result<_>>()?;
                let source = match &arrays[..] {
                    [a] => match a.borrow().dims.len() {
                        1 => Source::Mono(a.clone()),
                        // UNVERIFIED: a 2D array has a row for each side.
                        2 if a.borrow().dims[0] == 2 => Source::Rows(a.clone()),
                        _ => return Err(illegal()),
                    },
                    [l, r] => Source::Pair(l.clone(), r.clone()),
                    _ => return Err(ErrorKind::TypeMismatch.into()),
                };
                let rate = opt_in_range("frequency", args.get(arrays.len()), 1..=192_000, 48_000)?;
                let pcm = &mut self.sound.pcm;
                pcm.source = Some(source);
                pcm.rate = f64::from(rate);
                pcm.pos = 0.0;
                pcm.playing = true;
                Ok(vec![])
            }
            (Command::Pcmstop, Want::Outs(0), 0) => {
                self.sound.pcm.playing = false;
                Ok(vec![])
            }
            (Command::Pcmcont, Want::Outs(0), 0) => {
                let pcm = &mut self.sound.pcm;
                pcm.playing = pcm.source.is_some();
                Ok(vec![])
            }
            (Command::Pcmpos, Want::Outs(0), 1) => {
                let at = in_range("position", &args[0], 0..=i32::MAX)?;
                let pcm = &mut self.sound.pcm;
                pcm.pos = f64::from(at);
                let i = pcm.index();
                pcm.pos = i as f64;
                Ok(vec![])
            }
            (Command::Pcmpos, Want::Outs(1), 0) => {
                Ok(vec![Value::Int(self.sound.pcm.index() as i32)])
            }
            (Command::Pcmvol, Want::Outs(0), 1..=3) => {
                let (sides, volume, samples) = match args {
                    [v] => (0..2, v, None),
                    [c, v] => {
                        let c = in_range("channel", c, 0..=1)? as usize;
                        (c..c + 1, v, None)
                    }
                    [c, v, n] => {
                        let c = in_range("channel", c, 0..=1)? as usize;
                        (c..c + 1, v, Some(n))
                    }
                    _ => unreachable!("matched above"),
                };
                let volume = in_range("volume", volume, -32767..=32767)?;
                let samples = opt_in_range("samples", samples, 0..=i32::MAX, 0)? as u32;
                for side in sides {
                    self.sound.pcm.volume[side].to(f64::from(volume), samples);
                }
                Ok(vec![])
            }
            (Command::Pcmvol, Want::Outs(n @ 1..=2), 0) => Ok(self.sound.pcm.volume
                [..usize::from(n)]
                .iter()
                .map(|l| Value::Int(l.value.round().clamp(-FULL, FULL) as i32))
                .collect()),
            (Command::Efcen, Want::Outs(0), 1) => {
                self.sound.effector.on = in_range("state", &args[0], 0..=1)? == 1;
                Ok(vec![])
            }
            (Command::Efcen, Want::Outs(1), 0) => {
                Ok(vec![Value::Int(i32::from(self.sound.effector.on))])
            }
            (Command::Efcset, Want::Outs(0), 1) => {
                self.sound.effector.preset = in_range("preset", &args[0], 0..=3)?;
                Ok(vec![])
            }
            (Command::Efcset, Want::Outs(0), 9) => {
                in_range("reflections", &args[0], 0..=4)?;
                in_range("echo", &args[3], 0..=4)?;
                for a in args {
                    a.to_real()?;
                }
                Ok(vec![])
            }
            (Command::Efcwet, Want::Outs(0), 4) => {
                let mut wet = [0; 4];
                for (w, a) in wet.iter_mut().zip(args) {
                    *w = in_range("wet", a, 0..=127)?;
                }
                self.sound.effector.wet = wet;
                Ok(vec![])
            }
            // UNVERIFIED: in the main program, SNDMSBAL is kept but changes
            // nothing, as there are no subprograms yet.
            (Command::Sndmsbal, Want::Outs(0), 1..=2) => {
                self.sound.balance = in_range("balance", &args[0], 0..=127)?;
                opt_in_range("time", args.get(1), 0..=32767, 0)?;
                Ok(vec![])
            }
            (Command::Sndmsbal, Want::Outs(1), 0) => Ok(vec![Value::Int(self.sound.balance)]),
            // UNVERIFIED: a new TALK cuts off what is being said.
            (Command::Talk, Want::Outs(0), 1) => {
                self.sound.talk.say(&text(&args[0])?);
                Ok(vec![])
            }
            (Command::Talkchk, Want::Outs(1) | Want::Value, 0) => {
                Ok(vec![Value::Int(i32::from(self.sound.talk.speaking()))])
            }
            (Command::Talkstop, Want::Outs(0), 0) => {
                self.sound.talk.stop();
                Ok(vec![])
            }
            (Command::Recstart, Want::Outs(0), 0..=1) => {
                let (len, ring) = match args.first() {
                    // Five seconds, round and round.
                    None => (5 * RATE as usize, true),
                    Some(v) => {
                        let s = v.to_real()?;
                        if !(0.1..=60.0).contains(&s) {
                            return Err(Error::with_detail(ErrorKind::OutOfRange, "time"));
                        }
                        ((s * f64::from(RATE)).round() as usize, false)
                    }
                };
                self.sound.recorder = Recorder {
                    samples: vec![[0, 0]; len],
                    pos: 0,
                    recording: true,
                    ring,
                };
                Ok(vec![])
            }
            (Command::Recstop, Want::Outs(0), 0) => {
                self.sound.recorder.recording = false;
                Ok(vec![])
            }
            (Command::Recchk, Want::Outs(1) | Want::Value, 0) => {
                Ok(vec![Value::Int(i32::from(self.sound.recorder.recording))])
            }
            (Command::Reclen, Want::Value, 0) => {
                Ok(vec![Value::Int(self.sound.recorder.samples.len() as i32)])
            }
            (Command::Recpos, Want::Value, 0) => {
                Ok(vec![Value::Int(self.sound.recorder.pos as i32)])
            }
            (Command::Recdata, Want::Outs(2), 1) => {
                let rec = &self.sound.recorder;
                let at = in_range("position", &args[0], 0..=rec.samples.len() as i32 - 1)?;
                let [l, r] = rec.samples[at as usize];
                Ok(vec![Value::Int(i32::from(l)), Value::Int(i32::from(r))])
            }
            (Command::Recsave, Want::Outs(0), 1..=3) => {
                let rec = &self.sound.recorder;
                let len = rec.samples.len() as i32;
                let n = opt_in_range("count", args.get(1), 0..=len, rec.pos as i32)? as usize;
                // Recording round the buffer, positions go round it too.
                // UNVERIFIED: which samples a position past either end
                // takes (it is no error).
                let taken: Vec<[i16; 2]> = if rec.ring && len > 0 {
                    let from = match args.get(2) {
                        Some(v) => v.to_int()?.rem_euclid(len) as usize,
                        None => 0,
                    };
                    (0..n)
                        .map(|i| rec.samples[(from + i) % len as usize])
                        .collect()
                } else {
                    let from = opt_in_range("position", args.get(2), 0..=len, 0)? as usize;
                    if from + n > rec.samples.len() {
                        return Err(Error::with_detail(ErrorKind::OutOfRange, "count"));
                    }
                    rec.samples[from..from + n].to_vec()
                };
                let a = args[0].clone().into_array()?;
                let mut a = a.borrow_mut();
                let elem = a.elem_type();
                // The array takes the size of what it gets.
                match a.dims.len() {
                    1 => {
                        *a = Array::new(elem, vec![n]);
                        for (i, [l, r]) in taken.iter().enumerate() {
                            a.set(i, Value::Int((i32::from(*l) + i32::from(*r)) / 2))?;
                        }
                    }
                    2 => {
                        *a = Array::new(elem, vec![2, n]);
                        for (i, [l, r]) in taken.iter().enumerate() {
                            a.set(i, Value::Int(i32::from(*l)))?;
                            a.set(n + i, Value::Int(i32::from(*r)))?;
                        }
                    }
                    _ => return Err(illegal()),
                }
                Ok(vec![])
            }
            _ => Err(no_form()),
        }
    }
}
