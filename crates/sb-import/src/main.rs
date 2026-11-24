//! `smilebasic-import`: copies SmileBASIC 4's own resources from the
//! user's extracted RomFS and ExeFS directories into the emulator's data
//! directory, and its own projects into the emulator's projects folder.
//! It is separate from the emulator, which only reads what this tool has
//! imported.

mod bfsar;
mod import;
mod midi;
mod nso;
mod projects;
mod sf2;
mod spdef;
#[cfg(test)]
mod temp_dir;

use std::path::PathBuf;

use sb_runtime::resources::{RESOURCE_DIR, data_dir};

/// The folder of the emulator's data directory that holds the projects.
const PROJECTS_DIR: &str = "projects";

const USAGE: &str = "usage: smilebasic-import [options] DIR...

Imports the font, sprites and sounds from your own extracted SmileBASIC 4
RomFS and ExeFS directories, and copies its projects #SYS, #TOOL, #EXAMPLE
and #GUIDE into the emulator's projects folder, keeping files already
there. List the update's directories after the base game's so that its
files win.

options:
  -o OUT       import into OUT instead of the emulator's data directory
  --projects DIR
               copy the projects into DIR instead of the emulator's
               projects folder
  --sf2 FILE   also write the instruments as a SoundFont
  --midi DIR   also write the preset songs as MIDI files
  --gm         with --sf2 and --midi, number @0-@127 and the drum sets as
               General MIDI and Roland GS do (drums on channel 10), not by
               SmileBASIC 4's own banks. Other instruments keep theirs.";

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
    let layout = match args.iter().position(|a| a == "--gm") {
        Some(at) => {
            args.remove(at);
            sf2::Layout::Gm
        }
        None => sf2::Layout::Own,
    };
    let projects_dir = match option(&mut args, "--projects")? {
        Some(dir) => dir,
        None => data_dir()?.join(PROJECTS_DIR),
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
    let projects = projects::import_projects(&dirs, &projects_dir)?;
    for (project, source, written) in &projects.copied {
        let files = match written {
            1 => "1 file".to_string(),
            n => format!("{n} files"),
        };
        println!("copied {project} ({files}) from {}", source.display());
    }
    if !projects.copied.is_empty() {
        println!("into {}", projects_dir.display());
    }
    if projects.unchanged > 0 {
        println!("already there: {} files", projects.unchanged);
    }
    for file in &projects.kept {
        println!("kept {file}: it differs from SmileBASIC 4's");
    }
    for (file, why) in &projects.skipped {
        println!("left out {file}: {why}");
    }
    for missing in &projects.missing {
        println!("not found: {missing} (a project, in the RomFS)");
    }
    if let Some(path) = sf2 {
        let bank = imported
            .instruments
            .as_ref()
            .ok_or("no instruments to write as a SoundFont")?;
        std::fs::File::create(&path)
            .and_then(|mut f| sf2::write(bank, layout, &mut f))
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
            std::fs::write(&path, midi::write(song.clone(), layout))
                .map_err(|e| format!("{}: {e}", path.display()))?;
        }
        println!("wrote the songs as MIDI into {}", dir.display());
    }
    Ok(())
}
