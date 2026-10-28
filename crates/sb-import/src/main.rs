//! `smilebasic-import`: copies SmileBASIC 4's own resources from the
//! user's extracted RomFS and ExeFS directories into the emulator's data
//! directory. It is kept apart from the emulator, which only reads what
//! this has imported.

mod bfsar;
mod import;
mod nso;
mod sf2;
mod spdef;

use std::path::PathBuf;

use sb_runtime::resources::{DATA_DIR, RESOURCE_DIR};

const USAGE: &str = "usage:
  smilebasic-import [-o OUT] [--sf2 FILE] DIR...
      copy the font, sprite image, sprite definitions and instruments
      from your own extracted SmileBASIC 4 RomFS and ExeFS directories
      into the emulator's data directory, or into OUT. List the update's
      directories after the base game's, so the update's files win.
      With --sf2, also write the instruments as a SoundFont to FILE.";

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
    Ok(())
}

/// The directory SDL's `SDL_GetPrefPath("", DATA_DIR)` gives the emulator,
/// worked out the same way so the importer needs no SDL.
fn data_dir() -> Result<PathBuf, AnyError> {
    let env = |name: &str| std::env::var_os(name).filter(|v| !v.is_empty());
    let base = if cfg!(windows) {
        env("APPDATA")
            .map(PathBuf::from)
            .ok_or("APPDATA is not set")?
    } else if cfg!(target_os = "macos") {
        env("HOME")
            .map(|h| PathBuf::from(h).join("Library/Application Support"))
            .ok_or("HOME is not set")?
    } else {
        env("XDG_DATA_HOME")
            .map(PathBuf::from)
            .or_else(|| env("HOME").map(|h| PathBuf::from(h).join(".local/share")))
            .ok_or("neither XDG_DATA_HOME nor HOME is set")?
    };
    Ok(base.join(DATA_DIR))
}
