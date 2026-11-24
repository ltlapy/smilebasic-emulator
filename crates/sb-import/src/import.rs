//! Finding the resources in the extracted RomFS and ExeFS directories,
//! checking them, and copying them into the emulator's resource directory.

use std::fmt;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use sb_runtime::font::{Font, FontError};
use sb_runtime::help::Help;
use sb_runtime::resources::{
    self, FONT_MAP, FONT_PAGE, HELP_DATA, HELP_INDEX, INSTRUMENTS, SEQUENCES, SPRITE_DEFS,
    SPRITE_PAGE,
};
use sb_runtime::sound::bank::Bank;
use sb_runtime::sound::seq::Sequences;

use crate::{bfsar, nso, spdef};

/// A file to import, and where it lives inside a RomFS or ExeFS.
struct Source {
    name: &'static str,
    /// Candidate paths relative to the directory's root, most preferred
    /// first.
    paths: &'static [&'static str],
}

const FONT_MAP_SOURCE: Source = Source {
    name: FONT_MAP,
    paths: &["font.map"],
};

const FONT_PAGE_SOURCE: Source = Source {
    name: FONT_PAGE,
    paths: &["basic/en/#SYS/GDEFFONT", "basic/jp/#SYS/GDEFFONT"],
};

const SPRITE_PAGE_SOURCE: Source = Source {
    name: SPRITE_PAGE,
    paths: &["basic/en/#SYS/GDEFGRP", "basic/jp/#SYS/GDEFGRP"],
};

const HELP_INDEX_SOURCE: Source = Source {
    name: HELP_INDEX,
    paths: &["help/en/HelpData.idx"],
};

const HELP_DATA_SOURCE: Source = Source {
    name: HELP_DATA,
    paths: &["help/en/HelpData.dat"],
};

/// The sound archive, which holds the instruments, preset songs and
/// effects.
const SOUND_ARCHIVE: Source = Source {
    name: "petc4.bfsar",
    paths: &["petc4.bfsar"],
};

/// The executable in the ExeFS, which holds the default SPDEF table.
const EXECUTABLE: Source = Source {
    name: "main",
    paths: &["main"],
};

/// The result of [`import`].
#[derive(Debug, Default)]
pub struct Imported {
    /// The source of each copied resource.
    pub sources: Vec<PathBuf>,
    /// Optional resources that were not found, with what they are for.
    pub missing: Vec<&'static str>,
    /// The instruments, for writing out as a SoundFont too.
    pub instruments: Option<Bank>,
    /// The preset songs and effects, for writing out as MIDI.
    pub sequences: Option<Sequences>,
}

#[derive(Debug)]
pub enum ImportError {
    /// No source directory contains the file.
    Missing(&'static str),
    Io(PathBuf, io::Error),
    Font(FontError),
    /// A resource is there but not as expected.
    Bad(PathBuf, String),
}

impl fmt::Display for ImportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ImportError::Missing(name) => write!(f, "{name} not found"),
            ImportError::Io(path, e) => write!(f, "{}: {e}", path.display()),
            ImportError::Font(e) => write!(f, "{e}"),
            ImportError::Bad(path, why) => write!(f, "{}: {why}", path.display()),
        }
    }
}

impl std::error::Error for ImportError {}

impl From<FontError> for ImportError {
    fn from(e: FontError) -> Self {
        ImportError::Font(e)
    }
}

fn read(path: &Path) -> Result<Vec<u8>, ImportError> {
    std::fs::read(path).map_err(|e| ImportError::Io(path.to_path_buf(), e))
}

/// Finds a file in the RomFS or ExeFS directories. Later directories win,
/// so the directories of an update can be listed after those of the base
/// game.
fn locate(source: &Source, dirs: &[PathBuf]) -> Option<PathBuf> {
    dirs.iter().rev().find_map(|root| {
        source
            .paths
            .iter()
            .map(|p| root.join(p))
            .find(|p| p.is_file())
    })
}

/// Copies every resource from the RomFS and ExeFS directories into
/// `dest`, checking that each one parses first. The font is required; the
/// sprite image and definitions are taken when found.
pub fn import(dirs: &[PathBuf], dest: &Path) -> Result<Imported, ImportError> {
    let font = [FONT_MAP_SOURCE, FONT_PAGE_SOURCE];
    let sources = font
        .iter()
        .map(|s| locate(s, dirs).ok_or(ImportError::Missing(s.name)))
        .collect::<Result<Vec<_>, _>>()?;
    let files = sources
        .iter()
        .map(|p| read(p))
        .collect::<Result<Vec<_>, _>>()?;
    Font::from_files(&files[0], &files[1])?;

    std::fs::create_dir_all(dest).map_err(|e| ImportError::Io(dest.to_path_buf(), e))?;
    clear_temp_files(dest);
    let write = |name: &str, bytes: &[u8]| {
        let path = dest.join(name);
        if write_resource(&path, bytes)? {
            println!("replaced the link {} with a file", path.display());
        }
        Ok::<_, ImportError>(())
    };
    for (source, bytes) in font.iter().zip(&files) {
        write(source.name, bytes)?;
    }
    let mut imported = Imported {
        sources,
        ..Imported::default()
    };

    match locate(&SPRITE_PAGE_SOURCE, dirs) {
        Some(path) => {
            let bytes = read(&path)?;
            if resources::page_pixels(&bytes).is_none() {
                return Err(ImportError::Bad(path, "not a graphic page".into()));
            }
            write(SPRITE_PAGE, &bytes)?;
            imported.sources.push(path);
        }
        None => imported
            .missing
            .push("GDEFGRP (the default sprite image, in the RomFS)"),
    }
    match locate(&EXECUTABLE, dirs) {
        Some(path) => {
            let bytes = read(&path)?;
            let [_, ro, _] =
                nso::segments(&bytes).map_err(|e| ImportError::Bad(path.clone(), e.to_string()))?;
            let table = spdef::find_default_defs(&ro).ok_or_else(|| {
                ImportError::Bad(path.clone(), "no sprite definitions found".into())
            })?;
            write(SPRITE_DEFS, &table)?;
            imported.sources.push(path);
        }
        None => imported
            .missing
            .push("main (sprite definitions, in the ExeFS)"),
    }
    match (
        locate(&HELP_INDEX_SOURCE, dirs),
        locate(&HELP_DATA_SOURCE, dirs),
    ) {
        (Some(index), Some(data)) => {
            let (index_bytes, data_bytes) = (read(&index)?, read(&data)?);
            if Help::from_bytes(&index_bytes, data_bytes.clone()).is_none() {
                return Err(ImportError::Bad(data, "not a help".into()));
            }
            write(HELP_INDEX, &index_bytes)?;
            write(HELP_DATA, &data_bytes)?;
            imported.sources.extend([index, data]);
        }
        _ => imported.missing.push("HelpData (the help, in the RomFS)"),
    }
    match locate(&SOUND_ARCHIVE, dirs) {
        Some(path) => {
            let archive = read(&path)?;
            let bad = |e| ImportError::Bad(path.clone(), e);
            let bank = bfsar::bank(&archive).map_err(bad)?;
            let sequences = bfsar::sequences(&archive).map_err(bad)?;
            write(INSTRUMENTS, &bank.to_bytes())?;
            write(SEQUENCES, &sequences.to_bytes())?;
            imported.sources.push(path);
            imported.instruments = Some(bank);
            imported.sequences = Some(sequences);
        }
        None => imported
            .missing
            .push("petc4.bfsar (the instruments, songs and effects, in the RomFS)"),
    }
    Ok(imported)
}

/// Every name [`import`] writes in the output folder.
const OUTPUTS: [&str; 8] = [
    FONT_MAP,
    FONT_PAGE,
    SPRITE_PAGE,
    SPRITE_DEFS,
    HELP_INDEX,
    HELP_DATA,
    INSTRUMENTS,
    SEQUENCES,
];

/// How many times to try the rename, and how long to wait between tries.
/// On Windows an antivirus or indexer can hold a file it just saw written
/// for a moment, and the rename fails until it releases the file.
const RENAME_TRIES: u32 = 5;
const RENAME_WAIT: Duration = Duration::from_millis(80);

/// The temporary file a resource is written to before it takes the
/// resource's name: `NAME.tmp`, in the same folder so the rename stays on
/// one file system.
fn temp_path(path: &Path) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(".tmp");
    path.with_file_name(name)
}

/// Removes the temporary files an interrupted import left in `dir`. The
/// folder itself may be a link: it is followed, since that is where the
/// user put it.
fn clear_temp_files(dir: &Path) {
    for name in OUTPUTS {
        // A file that cannot be removed fails its write, with the reason.
        let _ = fs::remove_file(temp_path(&dir.join(name)));
    }
}

/// Writes a resource to a temporary file and renames it over `path`. A
/// link at `path` is replaced and what it points to is left alone, and an
/// interrupted import leaves the old file whole. Returns whether a link
/// was replaced.
fn write_resource(path: &Path, bytes: &[u8]) -> Result<bool, ImportError> {
    let temp = temp_path(path);
    let was_link = fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_symlink());
    // `create_new` does not follow a link at the temporary name either.
    let written = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp)
        .and_then(|mut file| {
            file.write_all(bytes)?;
            file.sync_all()
        });
    if let Err(e) = written {
        let _ = fs::remove_file(&temp);
        return Err(ImportError::Io(temp, e));
    }
    let mut tries = 1;
    while let Err(e) = fs::rename(&temp, path) {
        if tries == RENAME_TRIES {
            let _ = fs::remove_file(&temp);
            return Err(ImportError::Io(path.to_path_buf(), e));
        }
        tries += 1;
        std::thread::sleep(RENAME_WAIT);
    }
    Ok(was_link)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::temp_dir::TempDir;

    #[test]
    fn writes_through_a_temporary_file() {
        let tmp = TempDir::new("output");
        let dir = tmp.0.join("out");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(FONT_MAP), b"old").unwrap();
        // Left by an interrupted import.
        fs::write(temp_path(&dir.join(FONT_PAGE)), b"half").unwrap();
        clear_temp_files(&dir);
        assert!(!temp_path(&dir.join(FONT_PAGE)).exists());
        assert!(!write_resource(&dir.join(FONT_MAP), b"new").unwrap());
        assert!(!write_resource(&dir.join(FONT_PAGE), b"page").unwrap());
        assert_eq!(fs::read(dir.join(FONT_MAP)).unwrap(), b"new");
        assert_eq!(fs::read(dir.join(FONT_PAGE)).unwrap(), b"page");
        let mut names: Vec<_> = fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        names.sort();
        assert_eq!(names, [FONT_PAGE, FONT_MAP]);
    }

    #[cfg(unix)]
    #[test]
    fn replaces_links_and_leaves_their_files_alone() {
        use std::os::unix::fs::symlink;
        let tmp = TempDir::new("output-links");
        let dir = tmp.0.join("out");
        fs::create_dir_all(&dir).unwrap();
        let outside = tmp.0.join("real-font.map");
        fs::write(&outside, b"real").unwrap();
        symlink(&outside, dir.join(FONT_MAP)).unwrap();
        symlink(tmp.0.join("nowhere"), dir.join(FONT_PAGE)).unwrap();
        assert!(write_resource(&dir.join(FONT_MAP), b"new").unwrap());
        assert!(write_resource(&dir.join(FONT_PAGE), b"page").unwrap());

        assert_eq!(fs::read(&outside).unwrap(), b"real");
        assert!(!tmp.0.join("nowhere").exists());
        for (name, bytes) in [(FONT_MAP, &b"new"[..]), (FONT_PAGE, b"page")] {
            let path = dir.join(name);
            assert!(fs::symlink_metadata(&path).unwrap().file_type().is_file());
            assert_eq!(fs::read(&path).unwrap(), bytes);
        }
    }

    #[cfg(unix)]
    #[test]
    fn follows_a_linked_output_folder() {
        use std::os::unix::fs::symlink;
        let tmp = TempDir::new("output-folder-link");
        let real = tmp.0.join("real");
        fs::create_dir_all(&real).unwrap();
        let dir = tmp.0.join("out");
        symlink(&real, &dir).unwrap();
        assert!(!write_resource(&dir.join(FONT_MAP), b"new").unwrap());
        assert_eq!(fs::read(real.join(FONT_MAP)).unwrap(), b"new");
        assert!(fs::symlink_metadata(&dir).unwrap().file_type().is_symlink());
    }
}
