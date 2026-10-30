//! `smilebasic-import`: copies SmileBASIC 4's own resources from the
//! user's extracted RomFS and ExeFS directories into the emulator's data
//! directory. It is kept apart from the emulator, which only reads what
//! this has imported.

mod bfsar;
mod import;
mod midi;
mod nso;
mod sf2;
mod spdef;

use std::path::PathBuf;

use sb_runtime::resources::{RESOURCE_DIR, data_dir};

const USAGE: &str = "usage:
  smilebasic-import [-o OUT] [--sf2 FILE] [--midi DIR] DIR...
      copy the font, sprite image, sprite definitions and instruments
      from your own extracted SmileBASIC 4 RomFS and ExeFS directories
      into the emulator's data directory, or into OUT. List the update's
      directories after the base game's, so the update's files win.
      With --sf2, also write the instruments as a SoundFont to FILE;
      with --midi, the preset songs as MIDI files into DIR.";

type AnyError = Box<dyn std::error::Error>;

fn main() {
    if let Err(e) = run(std::env::args().skip(1).collect()) {
        eprintln!("{e}");
        std::process::exit(1);
    }
}

/// Takes `flag` and the value after it out of `args`.
fn option(args: &mut Vec<String>, flag: &str) -> Result<Option<PathBuf>, AnyError> {
    match args.iter().position(|a| a == flag) {
        Some(at) if at + 1 < args.len() => {
            let value = args.remove(at + 1);
            args.remove(at);
            Ok(Some(PathBuf::from(value)))
        }
        Some(_) => Err(USAGE.into()),
        None => Ok(None),
    }
}

fn run(mut args: Vec<String>) -> Result<(), AnyError> {
    let dest = match option(&mut args, "-o")? {
        Some(dir) => dir,
        None => data_dir()?.join(RESOURCE_DIR),
    };
    let sf2 = option(&mut args, "--sf2")?;
    let midi = option(&mut args, "--midi")?;
    if args.is_empty() || args.iter().any(|a| a.starts_with('-')) {
        return Err(USAGE.into());
    }
    let dirs: Vec<PathBuf> = args.iter().map(PathBuf::from).collect();
    let imported = import::import(&dirs, &dest)?;
    for source in &imported.sources {
        println!("imported {}", source.display());
    }
    println!("into {}", dest.display());
    for missing in &imported.missing {
        println!("not found: {missing}");
    }
    if let Some(path) = sf2 {
        let bank = imported
            .instruments
            .as_ref()
            .ok_or("no instruments to write as a SoundFont")?;
        std::fs::File::create(&path)
            .and_then(|mut f| sf2::write(bank, &mut f))
            .map_err(|e| format!("{}: {e}", path.display()))?;
        println!("wrote the SoundFont {}", path.display());
    }
    if let Some(dir) = midi {
        let sequences = imported
            .sequences
            .as_ref()
            .ok_or("no songs to write as MIDI")?;
        std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        for (n, song) in sequences.songs.iter().enumerate() {
            let Some(song) = song else { continue };
            let path = dir.join(format!("BGM{n:02}.mid"));
            std::fs::write(&path, midi::write(song.clone()))
                .map_err(|e| format!("{}: {e}", path.display()))?;
        }
        println!("wrote the songs as MIDI into {}", dir.display());
    }
    Ok(())
}
