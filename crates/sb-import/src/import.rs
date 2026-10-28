//! Finding the resources in the extracted RomFS and ExeFS directories,
//! checking them, and copying them into the emulator's resource directory.

use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

use sb_runtime::font::{Font, FontError};
use sb_runtime::resources::{self, FONT_MAP, FONT_PAGE, SPRITE_DEFS, SPRITE_PAGE};

use crate::{nso, spdef};

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

/// The executable in the ExeFS, which holds the default SPDEF table.
const EXECUTABLE: Source = Source {
    name: "main",
    paths: &["main"],
};

/// What [`import`] did.
#[derive(Debug, Default)]
pub struct Imported {
    /// The source of each copied resource.
    pub sources: Vec<PathBuf>,
    /// Optional resources that were not found, with what they are for.
    pub missing: Vec<&'static str>,
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
/// so an update's can be listed after the base game's.
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
    let write = |name: &str, bytes: &[u8]| {
        let path = dest.join(name);
        std::fs::write(&path, bytes).map_err(|e| ImportError::Io(path, e))
    };
    for (source, bytes) in font.iter().zip(&files) {
        write(source.name, bytes)?;
    }
    let mut imported = Imported {
        sources,
        missing: Vec::new(),
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
    Ok(imported)
}
