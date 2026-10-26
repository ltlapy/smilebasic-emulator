//! System resources copied from the user's own copy of SmileBASIC 4.
//!
//! The emulator does not ship SmileBASIC's font, graphics or tables. Users
//! extract the RomFS (and, for sprite definitions, the ExeFS) of their own
//! copy and run the importer on it, which validates the needed files and
//! copies them into the emulator's data directory.

use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

use crate::font::{Font, FontError};
use crate::graphics::GRP_SIZE;
use crate::nso;
use crate::sbfile::{self, DataValues};
use crate::sprites::{self, SpriteDef};

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

/// The default sprite image, GRP4 at start-up. Optional.
const SPRITE_PAGE: Resource = Resource {
    name: "GDEFGRP",
    romfs_paths: &["basic/en/#SYS/GDEFGRP", "basic/jp/#SYS/GDEFGRP"],
};

/// The executable in the ExeFS, which holds the default SPDEF table.
const EXECUTABLE: Resource = Resource {
    name: "main",
    romfs_paths: &["main"],
};

/// The SPDEF table taken from the executable, as its raw records.
const SPRITE_DEFS: &str = "spdef.bin";

/// What [`import`] did.
#[derive(Debug, Default)]
pub struct Imported {
    /// The source of each copied resource.
    pub sources: Vec<PathBuf>,
    /// Optional resources that were not found, with what they are for.
    pub missing: Vec<&'static str>,
}

#[derive(Debug)]
pub enum ResourceError {
    /// No source directory contains the file.
    Missing(&'static str),
    Io(PathBuf, io::Error),
    Font(FontError),
    /// An optional resource is there but not as expected.
    Bad(PathBuf, String),
}

impl fmt::Display for ResourceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ResourceError::Missing(name) => write!(f, "{name} not found"),
            ResourceError::Io(path, e) => write!(f, "{}: {e}", path.display()),
            ResourceError::Font(e) => write!(f, "{e}"),
            ResourceError::Bad(path, why) => write!(f, "{}: {why}", path.display()),
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

/// Finds a resource in the RomFS or ExeFS directories. Later directories
/// win, so an update's can be listed after the base game's.
fn locate(resource: &Resource, romfs: &[PathBuf]) -> Option<PathBuf> {
    romfs.iter().rev().find_map(|root| {
        resource
            .romfs_paths
            .iter()
            .map(|p| root.join(p))
            .find(|p| p.is_file())
    })
}

/// A page image: a 2048x2048 integer DAT in a SmileBASIC 4 container.
fn page_pixels(bytes: &[u8]) -> Option<Vec<u32>> {
    let data = sbfile::parse(bytes).ok()?.data().ok()?;
    let side = GRP_SIZE as usize;
    match data.values {
        DataValues::Int(px) if data.dims == [side, side] => {
            Some(px.into_iter().map(|p| p as u32).collect())
        }
        _ => None,
    }
}

/// Copies every resource from the RomFS and ExeFS directories into
/// `dest`, checking that each one parses first. The font is required; the
/// sprite image and definitions are taken when found.
pub fn import(romfs: &[PathBuf], dest: &Path) -> Result<Imported, ResourceError> {
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
    let write = |name: &str, bytes: &[u8]| {
        let path = dest.join(name);
        std::fs::write(&path, bytes).map_err(|e| ResourceError::Io(path, e))
    };
    for (resource, bytes) in RESOURCES.iter().zip(&files) {
        write(resource.name, bytes)?;
    }
    let mut imported = Imported {
        sources,
        missing: Vec::new(),
    };

    match locate(&SPRITE_PAGE, romfs) {
        Some(path) => {
            let bytes = read(&path)?;
            if page_pixels(&bytes).is_none() {
                return Err(ResourceError::Bad(path, "not a graphic page".into()));
            }
            write(SPRITE_PAGE.name, &bytes)?;
            imported.sources.push(path);
        }
        None => imported
            .missing
            .push("GDEFGRP (the default sprite image, in the RomFS)"),
    }
    match locate(&EXECUTABLE, romfs) {
        Some(path) => {
            let bytes = read(&path)?;
            let [_, ro, _] = nso::segments(&bytes)
                .map_err(|e| ResourceError::Bad(path.clone(), e.to_string()))?;
            let table = sprites::find_default_defs(&ro).ok_or_else(|| {
                ResourceError::Bad(path.clone(), "no sprite definitions found".into())
            })?;
            write(SPRITE_DEFS, table)?;
            imported.sources.push(path);
        }
        None => imported
            .missing
            .push("main (sprite definitions, in the ExeFS)"),
    }
    Ok(imported)
}

/// The default sprite image and definitions from a directory filled by
/// [`import`], each `None` if it was not imported or is damaged.
pub fn load_sprites(dir: &Path) -> (Option<Vec<u32>>, Option<Vec<SpriteDef>>) {
    let page = std::fs::read(dir.join(SPRITE_PAGE.name))
        .ok()
        .and_then(|b| page_pixels(&b));
    let defs = std::fs::read(dir.join(SPRITE_DEFS))
        .ok()
        .and_then(|b| sprites::defs_from_bytes(&b));
    (page, defs)
}

/// Loads the font from a directory filled by [`import`].
pub fn load_font(dir: &Path) -> Result<Font, ResourceError> {
    let map = read(&dir.join(FONT_MAP.name))?;
    let page = read(&dir.join(FONT_PAGE.name))?;
    Ok(Font::from_files(&map, &page)?)
}
