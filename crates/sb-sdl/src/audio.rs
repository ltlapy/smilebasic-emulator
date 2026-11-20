//! Playing the runtime's sound, and loading the SoundFont it plays with.
//!
//! The device's own thread takes the sound when it needs it, from an
//! [`Output`] the main loop fills a frame at a time, so a late frame eats
//! into a margin rather than leaving a gap, and the device's clock and the
//! frames' never drift apart.

use std::io::{Read, Seek, SeekFrom};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::Path;
use std::sync::{Arc, Mutex};

use sb_runtime::sound::output::{Output, Stats};
use sb_runtime::sound::{RATE, SoundFont};
use sdl3::audio::{AudioCallback, AudioFormat, AudioSpec, AudioStream, AudioStreamWithCallback};
use sdl3::sys::audio::SDL_SetAudioStreamGetCallback;

/// GeneralUser GS by S. Christian Collins; see `sounds/` for its license.
static BUNDLED_FONT: &[u8] = include_bytes!("../sounds/GeneralUser-GS.sf2");

/// The device's side: takes what it asks for from the shared output.
struct Pull {
    output: Arc<Mutex<Output>>,
    buf: Vec<f32>,
}

impl AudioCallback<f32> for Pull {
    fn callback(&mut self, stream: &mut AudioStream, requested: i32) {
        // Whole stereo samples.
        let n = (requested.max(0) as usize).next_multiple_of(2);
        self.buf.resize(n, 0.0);
        match self.output.lock() {
            Ok(mut o) => o.pull(&mut self.buf),
            Err(_) => self.buf.fill(0.0),
        }
        let _ = stream.put_data_f32(&self.buf);
    }
}

pub struct Audio {
    output: Arc<Mutex<Output>>,
    /// Kept for the device to go on calling back.
    _stream: Option<AudioStreamWithCallback<Pull>>,
}

impl Audio {
    /// Opens the default playback device; without one, sound is dropped.
    pub fn open(sdl: &sdl3::Sdl) -> Audio {
        let output = Arc::new(Mutex::new(Output::default()));
        let pull = Pull {
            output: output.clone(),
            buf: Vec::new(),
        };
        let stream = (|| -> Result<AudioStreamWithCallback<Pull>, sdl3::Error> {
            let spec = AudioSpec {
                freq: Some(RATE as i32),
                channels: Some(2),
                format: Some(AudioFormat::F32LE),
            };
            let stream = sdl
                .audio()?
                .default_playback_device()
                .open_playback_stream_with_callback(&spec, pull)?;
            stream.resume()?;
            Ok(stream)
        })();
        let stream = stream.map_err(|e| eprintln!("no sound: {e}")).ok();
        Audio {
            output,
            _stream: stream,
        }
    }

    /// Queues a frame's samples, left and right in turn.
    pub fn play(&self, samples: &[f32]) {
        if self._stream.is_none() || samples.is_empty() {
            return;
        }
        if let Ok(mut o) = self.output.lock() {
            o.push(samples);
        }
    }

    /// Where another thread can queue samples, as [`Audio::play`] does.
    pub fn sink(&self) -> Sink {
        Sink {
            output: self._stream.as_ref().map(|_| self.output.clone()),
        }
    }

    /// What the device's buffer went through and the samples waiting in
    /// it now, if there is a device.
    pub fn stats(&self) -> Option<(Stats, usize)> {
        self._stream.as_ref()?;
        self.output.lock().ok().map(|o| (o.stats, o.queued()))
    }
}

impl Drop for Audio {
    /// The sdl3 crate (0.20) frees the callback before it destroys the
    /// stream, while the device's thread may be calling it or about to:
    /// a `Pull` freed under it then writes into freed memory. Taking the
    /// callback off the stream first ends that, as SDL takes the stream's
    /// lock both to call the callback and to change it.
    fn drop(&mut self) {
        if let Some(stream) = &self._stream {
            // SAFETY: the stream lives until `_stream` is dropped, after
            // this; SDL runs no callback once the call returns.
            unsafe {
                SDL_SetAudioStreamGetCallback(stream.raw(), None, std::ptr::null_mut());
            }
        }
    }
}

/// The device's output for another thread; without a device, sound is dropped.
pub struct Sink {
    output: Option<Arc<Mutex<Output>>>,
}

impl Sink {
    #[cfg(test)]
    pub fn to(output: Option<Arc<Mutex<Output>>>) -> Sink {
        Sink { output }
    }

    /// Queues a frame's samples, left and right in turn.
    pub fn play(&self, samples: &[f32]) {
        if let (Some(output), false) = (&self.output, samples.is_empty())
            && let Ok(mut o) = output.lock()
        {
            o.push(samples);
        }
    }
}

/// Chunks looked at in a SoundFont; real ones have about 25.
const FONT_CHUNKS: usize = 256;

/// Whether the chunks in a SoundFont's three lists fit in the file.
/// rustysynth sets aside each chunk's size (up to 4 GB) before reading it,
/// so this stops a few damaged bytes taking all the memory. It goes over
/// what rustysynth reads and no more, so it refuses nothing that would
/// have loaded: anything else odd is left for rustysynth to find.
fn chunks_fit<R: Read + Seek>(reader: &mut R) -> std::io::Result<bool> {
    let len = reader.seek(SeekFrom::End(0))?;
    // After "RIFF", its size and "sfbk".
    let mut at = 12;
    let (mut end, mut lists) = (len, 0);
    for _ in 0..FONT_CHUNKS {
        if at + 8 > end {
            break;
        }
        let mut head = [0; 8];
        reader.seek(SeekFrom::Start(at))?;
        reader.read_exact(&mut head)?;
        let size = u64::from(u32::from_le_bytes([head[4], head[5], head[6], head[7]]));
        if at + 8 + size > len {
            return Ok(false);
        }
        if &head[..4] == b"LIST" {
            lists += 1;
            if lists == 3 {
                end = at + 8 + size;
            }
            // Into the list, past its type.
            at += 12;
        } else {
            at += 8 + size;
        }
    }
    Ok(true)
}

/// Reads a SoundFont. rustysynth takes the indices between its tables on
/// trust, and a file that gets them wrong makes it panic while loading;
/// that is a damaged file like any other, not the end of the program.
fn read_font<R: Read + Seek>(reader: &mut R) -> Result<SoundFont, String> {
    let damaged = |e: String| format!("not a SoundFont: {e}");
    if !chunks_fit(reader).map_err(|e| damaged(e.to_string()))? {
        return Err(damaged("a chunk is larger than the file".into()));
    }
    reader
        .seek(SeekFrom::Start(0))
        .map_err(|e| damaged(e.to_string()))?;
    catch_unwind(AssertUnwindSafe(|| SoundFont::new(reader)))
        .map_err(|_| damaged("its tables do not agree".into()))?
        .map_err(|e| damaged(e.to_string()))
}

/// The SoundFont at `path`, or the bundled one for `None`.
pub fn load_font(path: Option<&Path>) -> Result<Arc<SoundFont>, String> {
    let font = match path {
        None => read_font(&mut std::io::Cursor::new(BUNDLED_FONT)),
        Some(path) => {
            let file = std::fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
            read_font(&mut std::io::BufReader::new(file))
        }
    };
    font.map(Arc::new)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn chunk(id: &[u8; 4], body: &[u8]) -> Vec<u8> {
        let mut out = id.to_vec();
        out.extend_from_slice(&(body.len() as u32).to_le_bytes());
        out.extend_from_slice(body);
        out
    }

    fn list(kind: &[u8; 4], chunks: &[Vec<u8>]) -> Vec<u8> {
        let mut body = kind.to_vec();
        chunks.iter().for_each(|c| body.extend_from_slice(c));
        chunk(b"LIST", &body)
    }

    /// A SoundFont with no sounds, whose first zone starts at generator
    /// `generator`.
    fn font(generator: u16) -> Vec<u8> {
        let info = list(
            b"INFO",
            &[
                chunk(b"ifil", &[2, 0, 1, 0]),
                chunk(b"isng", b"EMU8000\0"),
                chunk(b"INAM", b"test\0\0"),
            ],
        );
        let sdta = list(b"sdta", &[chunk(b"smpl", &[0; 64])]);
        let bag = |g: u16| {
            let mut b = vec![];
            for g in [g, g.saturating_add(100)] {
                b.extend_from_slice(&g.to_le_bytes());
                b.extend_from_slice(&[0, 0]);
            }
            b
        };
        let pdta = list(
            b"pdta",
            &[
                chunk(b"phdr", &[0; 38 * 2]),
                chunk(b"pbag", &bag(generator)),
                chunk(b"pmod", &[0; 10]),
                chunk(b"pgen", &[0; 4]),
                chunk(b"inst", &[0; 22 * 2]),
                chunk(b"ibag", &bag(0)),
                chunk(b"imod", &[0; 10]),
                chunk(b"igen", &[0; 4]),
                chunk(b"shdr", &[0; 46 * 2]),
            ],
        );
        let mut body = b"sfbk".to_vec();
        for l in [info, sdta, pdta] {
            body.extend_from_slice(&l);
        }
        chunk(b"RIFF", &body)
    }

    #[test]
    fn tables_that_disagree_are_an_error_not_a_panic() {
        let e = read_font(&mut Cursor::new(font(1000))).unwrap_err();
        assert!(e.contains("not a SoundFont"), "{e}");
    }

    #[test]
    fn the_bundled_font_loads() {
        assert!(load_font(None).is_ok());
        assert!(chunks_fit(&mut Cursor::new(BUNDLED_FONT)).unwrap());
    }

    #[test]
    fn a_chunk_larger_than_the_file_is_refused_unread() {
        let mut bytes = font(0);
        assert!(chunks_fit(&mut Cursor::new(&bytes)).unwrap());
        let smpl = bytes.windows(4).position(|w| w == b"smpl").unwrap();
        bytes[smpl + 4..smpl + 8].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(!chunks_fit(&mut Cursor::new(&bytes)).unwrap());
        let e = read_font(&mut Cursor::new(bytes)).unwrap_err();
        assert!(e.contains("larger than the file"), "{e}");
    }

    #[test]
    fn what_follows_the_lists_is_not_looked_at() {
        // rustysynth stops after the third list.
        let mut bytes = font(0);
        bytes.extend_from_slice(b"junk");
        bytes.extend_from_slice(&u32::MAX.to_le_bytes());
        assert!(chunks_fit(&mut Cursor::new(bytes)).unwrap());
    }

    #[test]
    fn a_short_file_is_left_to_rustysynth() {
        assert!(chunks_fit(&mut Cursor::new(b"RIFF")).unwrap());
        assert!(read_font(&mut Cursor::new(b"RIFF")).is_err());
    }
}
