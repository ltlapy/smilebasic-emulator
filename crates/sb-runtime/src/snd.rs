//! Sound commands: BGMPLAY, BGMSET, BGMSETD, BGMSTOP, BGMCHK, BGMVOL,
//! BGMPITCH, BGMPAUSE, BGMCONT, BGMVAR, BGMCLEAR, BGMWET, CHKMML, SNDSTOP,
//! SNDMVOL, WAVSET, WAVSETA, BEEP, BEEPPIT, BEEPVOL, BEEPPAN, BEEPSTOP,
//! PCMSTREAM, PCMSTOP, PCMCONT, PCMPOS, PCMVOL, EFCEN, EFCSET, EFCWET,
//! SNDMSBAL, TALK, TALKCHK, TALKSTOP, and RECSTART and the commands that
//! read its recording.

use std::rc::Rc;

use sb_core::bytecode::{Literal, Want};
use sb_core::error::{Error, ErrorKind, Result};
use sb_core::value::{Array, ArrayRef, ElemType, Value};

use crate::runtime::{Command, Runtime, checking, illegal, no_form};
use crate::sound::beep::{BeepSound, HANDLES};
use crate::sound::mml::{self, Song};
use crate::sound::pcm::{FULL, Source};
use crate::sound::reverb::Settings;
use crate::sound::seq::Kind;
use crate::sound::wave::{
    BEEP_TICKS_PER_SECOND, Envelope, SIXTEEN_BIT, Wave, beep_ticks, eight_bit,
};
use crate::sound::{PRESET_BEEPS, PRESET_SONGS, RATE, TRACKS, frames};

/// The preset songs that end rather than loop, and their lengths in
/// frames, computed by playing SmileBASIC 4's own sequences through the
/// emulator's player; every other preset loops until stopped. BGMCHK says
/// 0 a frame or two after the last notes have died away.
const JINGLES: [(i32, u32); 3] = [(4, 167), (5, 111), (6, 342)];

fn int(v: &Value) -> Result<i32> {
    v.to_int()
}

fn out_of_range(what: &str, n: i32) -> Error {
    Error::with_detail(ErrorKind::OutOfRange, format!("{what} {n}"))
}

fn in_range(what: &str, v: &Value, range: std::ops::RangeInclusive<i32>) -> Result<i32> {
    checking(v, || {
        let n = int(v)?;
        if range.contains(&n) {
            Ok(n)
        } else {
            Err(out_of_range(what, n))
        }
    })
}

fn track_id(v: &Value) -> Result<usize> {
    in_range("track", v, 0..=TRACKS as i32 - 1).map(|n| n as usize)
}

/// A track that may be omitted, also with a comma after it
/// (`BGMSTOP ,1`).
fn opt_track(v: Option<&Value>) -> Result<Option<usize>> {
    match v {
        None | Some(Value::Default) => Ok(None),
        Some(v) => track_id(v).map(Some),
    }
}

fn user_song(v: &Value) -> Result<i32> {
    in_range("song", v, 128..=255)
}

/// Seconds, real allowed; 0 when omitted.
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

/// An omittable argument in `range`, `default` when omitted.
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

/// PCMSTREAM's array in argument `i`: integers or reals, at least one of
/// them, in at most `dims` dimensions.
fn pcm_array(args: &[Value], i: usize, dims: usize) -> Result<ArrayRef> {
    let mismatch = || Error::new(ErrorKind::TypeMismatch).of_arg(i);
    let Value::Array(a) = &args[i] else {
        return Err(mismatch());
    };
    let array = a.borrow();
    if array.elem_type() == ElemType::Str {
        return Err(mismatch());
    }
    if array.is_empty() {
        return Err(illegal().of_arg(i));
    }
    if array.dims.len() > dims {
        return Err(mismatch());
    }
    Ok(a.clone())
}

/// BGMPLAY's volume when it is omitted: half of BGMVOL's 127, which
/// plays twice as loud (TEST41).
const BGM_VOLUME: i32 = 64;

/// WAVSET's string with `[..]` groups, which the manual does not describe,
/// as the 512 samples it gives; `Some(None)` if it has no groups, and `None`
/// for anything SmileBASIC 4 refuses: text before the first group, an
/// empty group, one left open or nested, an odd number of digits, anything
/// other than digits in a group, more than 1024 digits in all, or a group
/// of more samples than 512 divided by the number of groups.
///
/// Each group is stretched to that many samples, but every group is written
/// from the start of the buffer: the last one is all that stays, and the
/// rest of the 512 keeps what the buffer held before.
// The rest of the buffer is left silent here, while SmileBASIC 4 does not
// clear its buffer: see differences.md.
fn wave_groups(text: &str) -> Option<Option<Vec<u8>>> {
    let units: Vec<u16> = text.encode_utf16().collect();
    let (open, close) = (u16::from(b'['), u16::from(b']'));
    let (mut groups, mut digits) = (0usize, 0usize);
    let mut inside: Option<usize> = None;
    for (i, &u) in units.iter().enumerate() {
        match inside {
            Some(n) if u == close => {
                if n == 0 || !n.is_multiple_of(2) {
                    return None;
                }
                digits += n;
                groups += 1;
                inside = None;
            }
            Some(_) if u == open => return None,
            Some(n) => inside = Some(n + 1),
            None if u == open => {
                if groups == 0 && i != 0 {
                    return None;
                }
                inside = Some(0);
            }
            None if u == close => return None,
            None => {}
        }
    }
    if inside.is_some() || digits > 1024 {
        return None;
    }
    if groups == 0 {
        return Some(None);
    }
    if units.len() < 4 || !units.len().is_multiple_of(2) {
        return None;
    }
    let digit = |u: u16| char::from_u32(u32::from(u)).and_then(|c| c.to_digit(16));
    let each = 512 / groups;
    let mut out = vec![0x80u8; 512];
    let mut bytes: Vec<u8> = Vec::new();
    let mut at = 0;
    for _ in 0..groups {
        if units[at] == open {
            bytes.clear();
            at += 1;
            while units[at] != close {
                let high = digit(units[at])?;
                let low = digit(units[at + 1])?;
                bytes.push((high << 4 | low) as u8);
                at += 2;
            }
            at += 1;
        }
        if bytes.len() > each {
            return None;
        }
        // A 16.16 fixed point step, the same as SmileBASIC 4 uses.
        let step = ((bytes.len() as i32) << 16) / each as i32;
        for (i, sample) in out[..each].iter_mut().enumerate() {
            *sample = bytes[((i as i32 * step) >> 16) as usize];
        }
    }
    Some(Some(out))
}

/// WAVSET's waveform: 512 samples, with a one-second note for BEEP
/// regardless of their length.
fn wavset_wave(bytes: &[u8], base_note: i32, envelope: Envelope) -> Wave {
    Wave {
        beep_ticks: Some(BEEP_TICKS_PER_SECOND),
        ..eight_bit(bytes, 512, base_note, envelope)
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

/// One of WAVSETA's arguments after the array: these can be omitted at the
/// end, by passing fewer arguments, but none can be omitted in between.
fn wave_arg(
    what: &str,
    args: &[Value],
    i: usize,
    range: std::ops::RangeInclusive<i32>,
    default: i32,
) -> Result<i32> {
    match args.get(i) {
        None => Ok(default),
        Some(Value::Default) => {
            let omitted = format!("argument {} omitted", i + 1);
            Err(Error::with_detail(ErrorKind::TypeMismatch, omitted).of_arg(i))
        }
        Some(v) => in_range(what, v, range),
    }
}

/// The numbers `at` in a one-dimensional array, each through `cut`. Only
/// those are read, so a waveform from a part of a huge array does not
/// copy all of it first.
fn samples<T>(
    v: &Value,
    at: std::ops::RangeInclusive<usize>,
    cut: impl Fn(f64) -> T,
) -> Result<Vec<T>> {
    checking(v, || {
        let a = v.clone().into_array()?;
        let a = a.borrow();
        if a.dims.len() != 1 {
            return Err(Error::with_detail(
                ErrorKind::IllegalFunctionCall,
                "1D array",
            ));
        }
        at.map(|i| a.get(i).to_real().map(&cut)).collect()
    })
}

fn text(v: &Value) -> Result<String> {
    checking(v, || {
        Ok(String::from_utf16_lossy(&v.clone().into_str()?.borrow()))
    })
}

/// The most MML that BGMPLAY, BGMSET and BGMSETD take, in characters.
const MML_MOST: usize = 1 << 20;

/// MML for BGMPLAY, BGMSET or BGMSETD. An error in it names the first
/// argument regardless of the command, even when the MML is not the first
/// argument; MML that is too long names none.
fn parse(mml: &str) -> Result<Song> {
    if mml.len() > MML_MOST && mml.encode_utf16().count() > MML_MOST {
        return Err(Error::new(ErrorKind::MmlTooLong));
    }
    mml::parse(mml).map_err(|e| Error::with_detail(e.kind, format!("at {}", e.at)).of_arg(0))
}

/// SNDMVOL's volume and its time in whole seconds. The time is checked
/// first, and a subprogram's SNDMVOL checks both before it does nothing.
pub(crate) fn sndmvol_args(args: &[Value]) -> Result<(i32, f32)> {
    let seconds = match args.get(1) {
        None => 0,
        Some(v) => in_range("time", v, 0..=32767)?,
    };
    let volume = in_range("volume", &args[0], 0..=127)?;
    Ok((volume, seconds as f32))
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
                self.sound.play(0, Some(song), BGM_VOLUME, 0);
                self.sound.track(0).number = 255;
                if idle {
                    self.sound.wake(0);
                }
                Ok(vec![])
            }
            (Command::Bgmplay, Want::Outs(0), 1..=4) => self.bgmplay(args),
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
                let track = opt_track(args.first())?;
                let fade = seconds(args.get(1))?;
                for t in Self::tracks_named(track) {
                    self.sound.stop(t, fade);
                }
                Ok(vec![])
            }
            (Command::Bgmpause, Want::Outs(0), 0..=2) => {
                let track = opt_track(args.first())?;
                let fade = seconds(args.get(1))?;
                for t in Self::tracks_named(track) {
                    self.sound.pause(t, fade);
                }
                Ok(vec![])
            }
            (Command::Bgmcont, Want::Outs(0), 0..=2) => {
                let track = opt_track(args.first())?;
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
                let wet = in_range("wet", &rest[0], 0..=127)?;
                self.sound.track(track.unwrap_or(0)).wet = Some(wet);
                Ok(vec![])
            }
            // Without a track, BGMPITCH sets the pitch of every track.
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
                let at = match mml::check(&text(&args[0])?) {
                    Ok(()) => -1,
                    Err(e) => e.at as i32,
                };
                Ok(vec![Value::Int(at)])
            }
            // SNDSTOP holds the program until the next frame when it synced
            // in this one (VSYNC, WAIT or SNDSTOP), and continues
            // immediately when a frame has passed since. VSYNC then counts
            // from where SNDSTOP left off, the same as from a VSYNC.
            (Command::Sndstop, Want::Outs(0), 0) => {
                if !self.frame_wait_over() {
                    self.sound.sndstop();
                    let now = self.sync.now;
                    if self.sync.mark.is_some_and(|mark| now <= mark) {
                        self.sync.moved = Some(now + 1);
                        self.hold_for_a_frame();
                    } else {
                        self.sync.moved = Some(now);
                    }
                }
                Ok(vec![])
            }
            (Command::Sndmvol, Want::Outs(0), 1..=2) => {
                let (volume, seconds) = sndmvol_args(args)?;
                self.sound.set_master(volume, seconds);
                Ok(vec![])
            }
            (Command::Sndmvol, Want::Outs(1) | Want::Value, 0) => {
                Ok(vec![Value::Int(self.sound.master_volume())])
            }
            // SmileBASIC 4 checks the reference pitch before it
            // reads the waveform, and names the waveform when it is wrong.
            (Command::Wavset, Want::Outs(0), 6..=7) => {
                let (n, envelope) = wave_head(args)?;
                let base_note = opt_in_range("pitch", args.get(6), 0..=127, 69)?;
                let hex = text(&args[5])?;
                let illegal =
                    || Error::with_detail(ErrorKind::IllegalFunctionCall, "waveform").of_arg(5);
                if let Some(bytes) = wave_groups(&hex).ok_or_else(illegal)? {
                    let wave = wavset_wave(&bytes, base_note, envelope);
                    self.sound.waves.borrow_mut().define(n, wave);
                    return Ok(vec![]);
                }
                let bytes: Option<Vec<u8>> = (0..hex.len() / 2)
                    .map(|i| {
                        hex.get(2 * i..2 * i + 2)
                            .and_then(|h| u8::from_str_radix(h, 16).ok())
                    })
                    .collect();
                // 16, 32, 64, 128, 256 or 512 samples; any other length is
                // an error, not cut short.
                let bytes = bytes
                    .filter(|b| {
                        hex.len() % 2 == 0 && [16, 32, 64, 128, 256, 512].contains(&b.len())
                    })
                    .ok_or_else(illegal)?;
                let wave = wavset_wave(&bytes, base_note, envelope);
                self.sound.waves.borrow_mut().define(n, wave);
                Ok(vec![])
            }
            // Ten arguments or more make a 16-bit sample with a loop and its
            // own sample rate, fewer the 8-bit form of SmileBASIC 3.
            (Command::Wavseta, Want::Outs(0), 6..=12) => self.wavseta(args),
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
                    // Without SmileBASIC's own sounds, presets are silent.
                    0..=PRESET_BEEPS => BeepSound::Preset,
                    224..=255 => match self.sound.waves.borrow().get(n) {
                        Some(w) if w.beep_ticks != Some(0) => BeepSound::Wave(w),
                        _ => BeepSound::Preset,
                    },
                    256..=383 => BeepSound::Instrument(n - 256),
                    n => return Err(out_of_range("sound", n)),
                };
                let keep = matches!(want, Want::Value);
                let h = self.sound.beeps.play(n, what, pitch, volume, pan, keep);
                Ok(match want {
                    Want::Value => vec![Value::Int(h.map_or(-1, |h| h as i32))],
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
                    None | Some(Value::Default) => {
                        (0..HANDLES).for_each(|h| self.sound.beeps.stop(h, fade))
                    }
                    Some(v) => self.sound.beeps.stop(handle(v)?, fade),
                }
                Ok(vec![])
            }
            (Command::Pcmstream, Want::Outs(0), 1..=3) => {
                let is_array = |v: &Value| matches!(v, Value::Array(_));
                let frequency =
                    |v: Option<&Value>| opt_in_range("frequency", v, 1..=192_000, 48_000);
                let (source, rate) = match args {
                    // A rate alone sets the new rate of the current stream.
                    [rate] if !is_array(rate) => {
                        self.sound.pcm.set_rate(f64::from(frequency(Some(rate))?));
                        return Ok(vec![]);
                    }
                    [_] | [_, _] if !args.get(1).is_some_and(is_array) => {
                        let a = pcm_array(args, 0, 2)?;
                        let rate = frequency(args.get(1))?;
                        // A 2D array has a row for each side.
                        let rows = a.borrow().dims.len() == 2;
                        let source = if rows {
                            Source::Rows(a)
                        } else {
                            Source::Mono(a)
                        };
                        (source, rate)
                    }
                    _ => {
                        let l = pcm_array(args, 0, 1)?;
                        let r = pcm_array(args, 1, 1)?;
                        if l.borrow().len() != r.borrow().len() {
                            return Err(illegal().of_arg(1));
                        }
                        (Source::Pair(l, r), frequency(args.get(2))?)
                    }
                };
                self.sound.pcm.start(source, f64::from(rate));
                Ok(vec![])
            }
            (Command::Pcmstop, Want::Outs(0), 0) => {
                self.sound.pcm.playing = false;
                Ok(vec![])
            }
            (Command::Pcmcont, Want::Outs(0), 0) => {
                self.sound.pcm.resume();
                Ok(vec![])
            }
            (Command::Pcmpos, Want::Outs(0), 1) => {
                let at = in_range("position", &args[0], 0..=i32::MAX)?;
                self.sound.pcm.restart(at as usize);
                Ok(vec![])
            }
            (Command::Pcmpos, Want::Outs(1) | Want::Value, 0) => {
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
                let on = in_range("state", &args[0], 0..=1)? == 1;
                self.sound.set_effector_on(on);
                Ok(vec![])
            }
            (Command::Efcen, Want::Outs(1) | Want::Value, 0) => {
                Ok(vec![Value::Int(i32::from(self.sound.effector().on))])
            }
            (Command::Efcset, Want::Outs(0), 1) => {
                let preset = in_range("preset", &args[0], 0..=3)?;
                self.sound.set_reverb(Settings::preset(preset));
                Ok(vec![])
            }
            (Command::Efcset, Want::Outs(0), 9) => {
                in_range("reflections", &args[0], 0..=4)?;
                in_range("echo", &args[3], 0..=4)?;
                let mut v = [0.0; 9];
                for (v, a) in v.iter_mut().zip(args) {
                    *v = a.to_real()?;
                }
                self.sound.set_reverb(Some(Settings::detailed(v)));
                Ok(vec![])
            }
            (Command::Efcwet, Want::Outs(0), 4) => {
                let mut wet = [0; 4];
                for (w, a) in wet.iter_mut().zip(args) {
                    *w = in_range("wet", a, 0..=127)?;
                }
                self.sound.set_sends(wet);
                Ok(vec![])
            }
            // The balance moves over the time in seconds, one step per frame.
            (Command::Sndmsbal, Want::Outs(0), 1..=2) => {
                let balance = in_range("balance", &args[0], 0..=127)?;
                let seconds = opt_in_range("time", args.get(1), 0..=32767, 0)? as f32;
                self.sound.set_balance(balance, seconds);
                Ok(vec![])
            }
            (Command::Sndmsbal, Want::Outs(1) | Want::Value, 0) => {
                Ok(vec![Value::Int(self.sound.balance())])
            }
            // A new TALK cuts off what is being said.
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
            // The subprogram's RECSTART and RECSTOP do nothing, regardless
            // of their arguments: its RECSAVE gets what the main program
            // recorded last, and the main program's recording continues.
            (Command::Recstart | Command::Recstop, Want::Outs(0), _)
                if self.sound.is_sub_interpreter =>
            {
                Ok(vec![])
            }
            (Command::Recstart, Want::Outs(0), 0..=1) => {
                let (len, ring) = match args.first() {
                    // Five seconds, looping.
                    None => (5 * RATE as usize, true),
                    Some(v) => {
                        let s = v.to_real()?;
                        if !(0.1..=60.0).contains(&s) {
                            return Err(Error::with_detail(ErrorKind::OutOfRange, "time"));
                        }
                        ((s * f64::from(RATE)).round() as usize, false)
                    }
                };
                self.sound.recorder.borrow_mut().start(len, ring);
                // RECSTART sends nothing to the effector until EFCWET.
                self.sound.set_sends([0; 4]);
                Ok(vec![])
            }
            (Command::Recstop, Want::Outs(0), 0) => {
                self.sound.recorder.borrow_mut().recording = false;
                Ok(vec![])
            }
            (Command::Recchk, Want::Outs(1) | Want::Value, 0) => Ok(vec![Value::Int(i32::from(
                self.sound.recorder.borrow().recording,
            ))]),
            (Command::Reclen, Want::Value, 0) => Ok(vec![Value::Int(
                self.sound.recorder.borrow().samples().len() as i32,
            )]),
            (Command::Recpos, Want::Value, 0) => {
                Ok(vec![Value::Int(self.sound.recorder.borrow().pos as i32)])
            }
            (Command::Recdata, Want::Outs(2), 1) => {
                let rec = self.sound.recorder.borrow();
                let samples = rec.samples();
                let len = samples.len() as i32;
                // When recording loops around the buffer, positions wrap
                // around it too: NK8_SOUNDVISUAL reads RECPOS()-I-1 from
                // the start. With a time, a position outside the recording
                // reads 0s with no error, and so does -1 (TEST32).
                let at = args[0].to_int()?;
                let at = match rec.ring && len > 0 {
                    true => Some(at.rem_euclid(len)),
                    false => (0..len).contains(&at).then_some(at),
                };
                let [l, r] = at.map_or([0, 0], |at| samples[at as usize]);
                Ok(vec![Value::Int(i32::from(l)), Value::Int(i32::from(r))])
            }
            (Command::Recsave, Want::Outs(0), 1..=3) => self.recsave(args),
            _ => Err(no_form()),
        }
    }

    /// BGMPLAY with a song number: `[track,] song [,volume [,time]]`.
    fn bgmplay(&mut self, args: &[Value]) -> Result<Vec<Value>> {
        let (track, music) = match args.len() {
            1 => (0, int(&args[0])?),
            _ => (opt_track(args.first())?.unwrap_or(0), int(&args[1])?),
        };
        let volume = match args.get(2) {
            None | Some(Value::Default) => BGM_VOLUME,
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
                self.sound.track(track).number = music;
                if let Some(&(_, n)) = JINGLES.iter().find(|j| j.0 == music) {
                    self.sound.track(track).end_silent_after(n);
                }
                if idle {
                    self.sound.wake(track);
                }
                return Ok(vec![]);
            }
            // A song number with nothing set plays nothing, so
            // BGMCHK says 0, and it stops what the track played:
            // BGMCHK still said 1 two frames later, and 0 five
            // frames later.
            // UNVERIFIED: that the sound stops immediately, and that
            // BGMCHK says 0 from the third frame.
            128..=255 => match self.sound.songs.get(&music) {
                Some(song) => Some(Rc::clone(song)),
                None => {
                    self.sound.stop_for_nothing(track);
                    return Ok(vec![]);
                }
            },
            n => return Err(out_of_range("song", n)),
        };
        let idle = self.sound.idle();
        self.sound.play(track, song, volume, fade);
        self.sound.track(track).number = music;
        if idle {
            self.sound.wake(track);
        }
        Ok(vec![])
    }

    /// WAVSETA: an instrument from part of an array.
    fn wavseta(&mut self, args: &[Value]) -> Result<Vec<Value>> {
        let (n, envelope) = wave_head(args)?;
        let Value::Array(array) = &args[5] else {
            return Err(Error::from(ErrorKind::TypeMismatch).of_arg(5));
        };
        let last = array.borrow().len() as i32 - 1;
        let base_note = wave_arg("pitch", args, 6, 0..=127, 69)?;
        let start = wave_arg("start", args, 7, 0..=last, 0)?;
        let end = wave_arg("end", args, 8, 0..=last, last)?;
        let rate = wave_arg("frequency", args, 9, 1..=96000, 28160)?;
        let offset = wave_arg("offset", args, 10, 0..=last, 0)?;
        let beep_loop = wave_arg("flag", args, 11, i32::MIN..=i32::MAX, 0)? != 0;
        if start > end {
            let detail = if last < 0 {
                "empty array"
            } else {
                "start past end"
            };
            return Err(Error::with_detail(ErrorKind::IllegalFunctionCall, detail));
        }
        if start == end {
            return Ok(vec![]);
        }
        let (start, end) = (start as usize, end as usize);
        if args.len() < 10 {
            // Each value is cut to an integer toward zero and then
            // to its low 8 bits, so 256 is 0 and -1 is 255.
            let bytes = samples(&args[5], start..=end, |x| x as i32 as u8)?;
            // The pass is the next power of two, at most 1M samples.
            let pass = bytes.len().next_power_of_two().min(1 << 20);
            let wave = eight_bit(&bytes, pass, base_note, envelope);
            self.sound.waves.borrow_mut().define(n, wave);
            return Ok(vec![]);
        }
        let offset = offset as usize;
        // The sample is the loop's end + 1 values from the offset
        // on, and the loop counts from the offset: what comes after
        // the loop is dropped.
        if end > (last as usize) - offset {
            let e = Error::with_detail(ErrorKind::OutOfRange, "offset leaves too little");
            return Err(e.of_arg(10));
        }
        // Values are cut to 16 bits, reals toward zero first and
        // held to 32 bits.
        let samples = samples(&args[5], offset..=offset + end, |x| x as i32 as i16)?;
        let looped = (start, end + 1);
        // The flag repeats BEEP's note forever, if it has one.
        let beep_ticks = Some(beep_ticks(end + 1, rate as u32));
        self.sound.waves.borrow_mut().define(
            n,
            Wave {
                samples,
                unit: SIXTEEN_BIT,
                base_note,
                rate: Some(f64::from(rate)),
                cycle: 0.0,
                looped,
                beep_loop,
                beep_ticks,
                beep_repeats: beep_loop,
                envelope,
            },
        );
        Ok(vec![])
    }

    /// RECSAVE: copies the recording into an array, which takes its size.
    fn recsave(&mut self, args: &[Value]) -> Result<Vec<Value>> {
        // SmileBASIC 4 names the first argument when it
        // is not an array.
        if !matches!(args[0], Value::Array(_)) {
            return Err(Error::new(ErrorKind::TypeMismatch).of_arg(0));
        }
        let rec = self.sound.recorder.borrow();
        let samples = rec.samples();
        let len = samples.len() as i32;
        let n = opt_in_range("count", args.get(1), 0..=len, rec.pos as i32)? as usize;
        // When recording loops around the buffer, positions wrap
        // around it too, with no error: before 0 or past what this
        // recording reached, they read the older recordings there.
        let taken: Vec<[i16; 2]> = if rec.ring && len > 0 {
            let from = match args.get(2) {
                Some(v) => v.to_int()?.rem_euclid(len) as usize,
                None => 0,
            };
            (0..n).map(|i| samples[(from + i) % len as usize]).collect()
        } else {
            // UNVERIFIED: that a recording with a time refuses
            // positions past its end.
            let from = opt_in_range("position", args.get(2), 0..=len, 0)? as usize;
            if from + n > samples.len() {
                return Err(Error::with_detail(ErrorKind::OutOfRange, "count"));
            }
            samples[from..from + n].to_vec()
        };
        let a = args[0].clone().into_array()?;
        let mut a = a.borrow_mut();
        let elem = a.elem_type();
        // SmileBASIC 4 reads the count and the position first, then
        // refuses a string array or one of over two dimensions as
        // the first argument's Type mismatch.
        if elem == ElemType::Str || a.dims.len() > 2 {
            return Err(Error::new(ErrorKind::TypeMismatch).of_arg(0));
        }
        if a.system {
            return Err(Error::new(ErrorKind::CantResizeThisArray).of_arg(0));
        }
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wavset_groups_keep_the_last_one() {
        // TEST56's string is taken.
        let out = wave_groups("[FF00][80][4080C0]").unwrap().unwrap();
        assert_eq!(out.len(), 512);
        // 512 / 3 = 170 samples from three bytes; then silence.
        assert_eq!(
            (out[0], out[56], out[57], out[113], out[114]),
            (0x40, 0x40, 0x80, 0x80, 0xC0)
        );
        assert_eq!((out[169], out[170]), (0xC0, 0x80));
        let one = wave_groups("[0080]").unwrap().unwrap();
        assert_eq!((one[0], one[255], one[256], one[511]), (0, 0, 0x80, 0x80));
        // No groups: the plain form handles it.
        assert_eq!(wave_groups("FF00"), Some(None));
        for bad in ["FF[00]", "[]", "[F]", "[00", "00]", "[[00]]", "[0G]"] {
            assert_eq!(wave_groups(bad), None, "{bad}");
        }
        // More than 512/2 samples in one of two groups.
        let long = format!("[{}][00]", "00".repeat(257));
        assert_eq!(wave_groups(&long), None);
    }

    #[test]
    fn wavseta_reads_its_part_of_the_array_cut_to_size() {
        let mut a = sb_core::value::Array::new(ElemType::Real, vec![6]);
        for (i, x) in [1.0, 70000.5, -1.9, 255.9, 256.0, -40000.0]
            .into_iter()
            .enumerate()
        {
            a.set(i, Value::Real(x)).unwrap();
        }
        let v = Value::Array(std::rc::Rc::new(std::cell::RefCell::new(a)));
        let sixteen = samples(&v, 1..=4, |x| x as i32 as i16).unwrap();
        assert_eq!(sixteen, [4464, -1, 255, 256]);
        let eight = samples(&v, 2..=5, |x| x as i32 as u8).unwrap();
        assert_eq!(eight, [255, 255, 0, 192]);
        let strings = sb_core::value::Array::new(ElemType::Str, vec![2]);
        let s = Value::Array(std::rc::Rc::new(std::cell::RefCell::new(strings)));
        let e = samples(&s, 0..=1, |x| x as i32 as i16).unwrap_err();
        assert_eq!(e.kind, ErrorKind::TypeMismatch);
    }
}
