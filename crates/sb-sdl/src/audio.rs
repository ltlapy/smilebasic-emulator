//! Playing the runtime's sound, and loading the SoundFont it plays with.
//!
//! The device's own thread takes the sound when it needs it, from an
//! [`Output`] the main loop fills a frame at a time, so a late frame eats
//! into a margin rather than leaving a gap, and the device's clock and the
//! frames' never drift apart.

use std::path::Path;
use std::sync::{Arc, Mutex};

use sb_runtime::sound::output::{Output, Stats};
use sb_runtime::sound::{RATE, SoundFont};
use sdl3::audio::{AudioCallback, AudioFormat, AudioSpec, AudioStream, AudioStreamWithCallback};

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

    /// What the device's buffer went through, if there is a device.
    pub fn stats(&self) -> Option<Stats> {
        self._stream.as_ref()?;
        self.output.lock().ok().map(|o| o.stats)
    }
}

/// The SoundFont at `path`, or the bundled one for `None`.
pub fn load_font(path: Option<&Path>) -> Result<Arc<SoundFont>, String> {
    let font = match path {
        None => SoundFont::new(&mut std::io::Cursor::new(BUNDLED_FONT)),
        Some(path) => {
            let mut file =
                std::fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
            SoundFont::new(&mut std::io::BufReader::new(&mut file))
        }
    };
    font.map(Arc::new)
        .map_err(|e| format!("not a SoundFont: {e}"))
}
