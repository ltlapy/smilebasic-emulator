//! System resources copied from the user's own copy of SmileBASIC 4.
//!
//! The emulator does not ship SmileBASIC's font or graphics. Users extract
//! the RomFS of their own copy and run the importer on it, which validates
//! the needed files and copies them into the emulator's data directory.

use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

use crate::font::{Font, FontError};

/// A file the emulator needs, and where it lives inside a RomFS.
struct Resource {
    name: &'static str,
    /// Candidate paths relative to the RomFS root, most preferred first.
    romfs_paths: &'static [&'static str],
}

const FONT_MAP: Resource = Resource {
    name: "font.map",
    romfs_paths: &["font.map"],
};

const FONT_PAGE: Resource = Resource {
    name: "GDEFFONT",
    romfs_paths: &["basic/en/#SYS/GDEFFONT", "basic/jp/#SYS/GDEFFONT"],
};

const RESOURCES: &[Resource] = &[FONT_MAP, FONT_PAGE];

#[derive(Debug)]
pub enum ResourceError {
    /// No source directory contains the file.
    Missing(&'static str),
    Io(PathBuf, io::Error),
    Font(FontError),
}

impl fmt::Display for ResourceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ResourceError::Missing(name) => write!(f, "{name} not found"),
            ResourceError::Io(path, e) => write!(f, "{}: {e}", path.display()),
            ResourceError::Font(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for ResourceError {}

impl From<FontError> for ResourceError {
    fn from(e: FontError) -> Self {
        ResourceError::Font(e)
    }
}

fn read(path: &Path) -> Result<Vec<u8>, ResourceError> {
    std::fs::read(path).map_err(|e| ResourceError::Io(path.to_path_buf(), e))
}

/// Finds a resource in the RomFS directories. Later directories win, so
/// an update's RomFS can be listed after the base game's.
fn locate(resource: &Resource, romfs: &[PathBuf]) -> Option<PathBuf> {
    romfs.iter().rev().find_map(|root| {
        resource
            .romfs_paths
            .iter()
            .map(|p| root.join(p))
            .find(|p| p.is_file())
    })
}

/// Copies every resource from the RomFS directories into `dest`, checking
/// that each one parses first. Returns the source of each copied file.
pub fn import(romfs: &[PathBuf], dest: &Path) -> Result<Vec<PathBuf>, ResourceError> {
    let sources = RESOURCES
        .iter()
        .map(|r| locate(r, romfs).ok_or(ResourceError::Missing(r.name)))
        .collect::<Result<Vec<_>, _>>()?;
    let files = sources
        .iter()
        .map(|p| read(p))
        .collect::<Result<Vec<_>, _>>()?;
    Font::from_files(&files[0], &files[1])?;

    std::fs::create_dir_all(dest).map_err(|e| ResourceError::Io(dest.to_path_buf(), e))?;
    for (resource, bytes) in RESOURCES.iter().zip(&files) {
        let path = dest.join(resource.name);
        std::fs::write(&path, bytes).map_err(|e| ResourceError::Io(path, e))?;
    }
    Ok(sources)
}

/// Loads the font from a directory filled by [`import`].
pub fn load_font(dir: &Path) -> Result<Font, ResourceError> {
    let map = read(&dir.join(FONT_MAP.name))?;
    let page = read(&dir.join(FONT_PAGE.name))?;
    Ok(Font::from_files(&map, &page)?)
}
