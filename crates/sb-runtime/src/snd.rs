//! Sound commands: BGMPLAY, BGMSET, BGMSETD, BGMSTOP, BGMCHK, BGMVOL,
//! BGMPITCH, BGMPAUSE, BGMCONT, BGMVAR, BGMCLEAR, BGMWET, CHKMML, SNDSTOP
//! and SNDMVOL.

use std::rc::Rc;

use sb_core::bytecode::{Literal, Want};
use sb_core::error::{Error, ErrorKind, Result};
use sb_core::value::Value;

use crate::runtime::{Command, Runtime, illegal};
use crate::sound::mml::{self, Song};
use crate::sound::{PRESET_SONGS, TRACKS, frames};

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
                self.sound.play(0, Some(song), 127, 0);
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
                    // UNVERIFIED: without SmileBASIC's own songs, presets play
                    // silence until stopped, so programs waiting on BGMCHK
                    // still wait.
                    0..=PRESET_SONGS => None,
                    // UNVERIFIED: a song number with nothing set plays
                    // silence too.
                    128..=255 => self.sound.songs.get(&music).cloned(),
                    n => return Err(out_of_range("song", n)),
                };
                self.sound.play(track, song, volume, fade);
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
                loop {
                    match data.literal()? {
                        Literal::Str(s) => mml.push_str(&String::from_utf16_lossy(&s)),
                        Literal::Int(0) => break,
                        Literal::Real(0.0) => break,
                        _ => return Err(ErrorKind::TypeMismatch.into()),
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
                self.sound.vars(track)[n] = v;
                Ok(vec![])
            }
            (Command::Bgmvar, Want::Outs(1) | Want::Value, 2) => {
                let track = track_id(&args[0])?;
                let n = in_range("variable", &args[1], 0..=7)? as usize;
                Ok(vec![Value::Int(self.sound.vars(track)[n])])
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
            _ => Err(illegal()),
        }
    }
}
