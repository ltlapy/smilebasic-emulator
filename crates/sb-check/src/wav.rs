//! `--wav`: the sound a run mixed, as a 16-bit PCM stereo WAV file at the
//! mixer's rate.

use std::fs::File;
use std::io::{BufWriter, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use sb_runtime::sound::RATE;

const CHANNELS: u16 = 2;
const BYTES_PER_SAMPLE: u16 = 2;
const BLOCK: u16 = CHANNELS * BYTES_PER_SAMPLE;
const HEADER: u32 = 44;
/// The most data the header's 32-bit sizes can count, in whole frames.
const MOST_DATA: u32 = (u32::MAX - HEADER) / BLOCK as u32 * BLOCK as u32;

/// A WAV file being written. Its header gets the final length when it is
/// finished or dropped, so a run that fails partway still leaves a file
/// that plays.
pub struct Wav {
    file: BufWriter<File>,
    path: PathBuf,
    /// Data bytes written so far.
    bytes: u32,
    /// Whether samples were dropped because the file was full.
    dropped: bool,
    finished: bool,
}

impl Wav {
    pub fn create(path: &Path) -> std::io::Result<Wav> {
        let mut file = BufWriter::new(File::create(path)?);
        file.write_all(&header(0))?;
        Ok(Wav {
            file,
            path: path.to_path_buf(),
            bytes: 0,
            dropped: false,
            finished: false,
        })
    }

    /// Adds samples, left and right in turn, from -1 to 1.
    pub fn write(&mut self, samples: &[f32]) -> std::io::Result<()> {
        let room = ((MOST_DATA - self.bytes) / u32::from(BYTES_PER_SAMPLE)) as usize;
        let room = room - room % usize::from(CHANNELS);
        let take = samples.len().min(room);
        self.dropped |= take < samples.len();
        for &s in &samples[..take] {
            // Rounded, and saturating at the ends.
            let v = (s * 32767.0).round() as i16;
            self.file.write_all(&v.to_le_bytes())?;
        }
        self.bytes += take as u32 * u32::from(BYTES_PER_SAMPLE);
        Ok(())
    }

    /// Writes the header's sizes and closes the file.
    pub fn finish(mut self) -> Result<(), String> {
        self.close()
            .map_err(|e| format!("{}: {e}", self.path.display()))
    }

    fn close(&mut self) -> std::io::Result<()> {
        self.finished = true;
        self.file.seek(SeekFrom::Start(0))?;
        self.file.write_all(&header(self.bytes))?;
        self.file.flush()?;
        if self.dropped {
            eprintln!(
                "{}: the WAV file is full; the rest of the sound is left out",
                self.path.display()
            );
        }
        Ok(())
    }
}

impl Drop for Wav {
    fn drop(&mut self) {
        if !self.finished {
            let _ = self.close();
        }
    }
}

/// A canonical 44-byte header for `data` bytes of samples.
fn header(data: u32) -> [u8; HEADER as usize] {
    let mut h = [0; HEADER as usize];
    let byte_rate = RATE * u32::from(BLOCK);
    h[0..4].copy_from_slice(b"RIFF");
    h[4..8].copy_from_slice(&(HEADER - 8 + data).to_le_bytes());
    h[8..16].copy_from_slice(b"WAVEfmt ");
    h[16..20].copy_from_slice(&16u32.to_le_bytes());
    h[20..22].copy_from_slice(&1u16.to_le_bytes());
    h[22..24].copy_from_slice(&CHANNELS.to_le_bytes());
    h[24..28].copy_from_slice(&RATE.to_le_bytes());
    h[28..32].copy_from_slice(&byte_rate.to_le_bytes());
    h[32..34].copy_from_slice(&BLOCK.to_le_bytes());
    h[34..36].copy_from_slice(&(BYTES_PER_SAMPLE * 8).to_le_bytes());
    h[36..40].copy_from_slice(b"data");
    h[40..44].copy_from_slice(&data.to_le_bytes());
    h
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_and_samples() {
        let path = std::env::temp_dir().join(format!("sb-check-wav-{}.wav", std::process::id()));
        let mut w = Wav::create(&path).unwrap();
        w.write(&[0.0, 1.0, -1.0, 0.5]).unwrap();
        w.finish().unwrap();
        let bytes = std::fs::read(&path).unwrap();
        let _ = std::fs::remove_file(&path);
        assert_eq!(bytes.len(), 44 + 8);
        assert_eq!(&bytes[4..8], &44u32.to_le_bytes());
        assert_eq!(&bytes[40..44], &8u32.to_le_bytes());
        let samples: Vec<i16> = (bytes[44..].chunks(2))
            .map(|c| i16::from_le_bytes([c[0], c[1]]))
            .collect();
        assert_eq!(samples, [0, 32767, -32767, 16384]);
    }

    #[test]
    fn a_dropped_file_gets_its_length() {
        let path =
            std::env::temp_dir().join(format!("sb-check-wav-drop-{}.wav", std::process::id()));
        {
            let mut w = Wav::create(&path).unwrap();
            w.write(&[0.25; 6]).unwrap();
        }
        let bytes = std::fs::read(&path).unwrap();
        let _ = std::fs::remove_file(&path);
        assert_eq!(&bytes[40..44], &12u32.to_le_bytes());
    }
}
