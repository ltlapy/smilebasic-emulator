//! Playing the runtime's sound, and loading the SoundFont it plays with.

use std::path::Path;
use std::sync::Arc;

use sb_runtime::sound::{FRAME_SAMPLES, RATE, SoundFont};
use sdl3::audio::{AudioFormat, AudioSpec, AudioStreamOwner};

/// GeneralUser GS by S. Christian Collins; see `sounds/` for its license.
static BUNDLED_FONT: &[u8] = include_bytes!("../sounds/GeneralUser-GS.sf2");

/// Frames of sound queued ahead, so a late frame does not leave a gap.
const AHEAD: usize = 3;
/// More than this queued and the frame's sound is dropped, so that the
/// sound does not fall behind the picture.
const MOST_AHEAD: usize = 8;

const BYTES_PER_FRAME: usize = FRAME_SAMPLES * 2 * std::mem::size_of::<f32>();

pub struct Audio {
    stream: Option<AudioStreamOwner>,
}

impl Audio {
    /// Opens the default playback device; without one, sound is dropped.
    pub fn open(sdl: &sdl3::Sdl) -> Audio {
        let stream = (|| -> Result<AudioStreamOwner, sdl3::Error> {
            let spec = AudioSpec {
                freq: Some(RATE as i32),
                channels: Some(2),
                format: Some(AudioFormat::F32LE),
            };
            let stream = sdl
                .audio()?
                .default_playback_device()
                .open_device_stream(Some(&spec))?;
            stream.put_data_f32(&vec![0.0; FRAME_SAMPLES * 2 * AHEAD])?;
            stream.resume()?;
            Ok(stream)
        })();
        Audio {
            stream: stream.map_err(|e| eprintln!("no sound: {e}")).ok(),
        }
    }

    /// Queues a frame's samples, left and right in turn.
    pub fn play(&self, samples: &[f32]) {
        let Some(stream) = &self.stream else {
            return;
        };
        let queued = stream.queued_bytes().unwrap_or(0).max(0) as usize;
        if queued > BYTES_PER_FRAME * MOST_AHEAD {
            return;
        }
        if queued < BYTES_PER_FRAME {
            // Ran dry: queue silence ahead again so that it does not keep
            // running dry.
            let _ = stream.put_data_f32(&vec![0.0; FRAME_SAMPLES * 2 * (AHEAD - 1)]);
        }
        if let Err(e) = stream.put_data_f32(samples) {
            eprintln!("sound: {e}");
        }
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
