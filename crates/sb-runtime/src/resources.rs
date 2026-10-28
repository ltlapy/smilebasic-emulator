//! System resources copied from the user's own copy of SmileBASIC 4.
//!
//! The emulator does not ship SmileBASIC's font, graphics or tables. Users
//! extract the RomFS (and, for sprite definitions, the ExeFS) of their own
//! copy and run the separate importer, `smilebasic-import`, on it. It
//! checks the files and copies them into the emulator's data directory
//! under the names below, where the loaders here read them.

use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

use crate::font::{Font, FontError};
use crate::graphics::GRP_SIZE;
use crate::sbfile::{self, DataValues};
use crate::sound::bank::Bank;
use crate::sprites::{self, SpriteDef};

/// The emulator's data directory in the user's application data, which
/// both the emulator and the importer find on their own.
pub const DATA_DIR: &str = "smilebasic-emulator";
/// The resource directory, inside the data directory.
pub const RESOURCE_DIR: &str = "resources";

/// The font's code-to-glyph table, from the RomFS root.
pub const FONT_MAP: &str = "font.map";
/// The font page, from `#SYS` in the RomFS.
pub const FONT_PAGE: &str = "GDEFFONT";
/// The default sprite image, GRP4 at start-up, from `#SYS` in the RomFS.
pub const SPRITE_PAGE: &str = "GDEFGRP";
/// The SPDEF table taken from the executable, as its raw records.
pub const SPRITE_DEFS: &str = "spdef.bin";
/// The instruments and waveforms from the sound archive, as a [`Bank`].
pub const INSTRUMENTS: &str = "instruments.bin";

#[derive(Debug)]
pub enum ResourceError {
    Io(PathBuf, io::Error),
    Font(FontError),
}

impl fmt::Display for ResourceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
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

/// A page image: a 2048x2048 integer DAT in a SmileBASIC 4 container.
pub fn page_pixels(bytes: &[u8]) -> Option<Vec<u32>> {
    let data = sbfile::parse(bytes).ok()?.data().ok()?;
    let side = GRP_SIZE as usize;
    match data.values {
        DataValues::Int(px) if data.dims == [side, side] => {
            Some(px.into_iter().map(|p| p as u32).collect())
        }
        _ => None,
    }
}

/// The default sprite image and definitions from the resource directory,
/// each `None` if it was not imported or is damaged.
pub fn load_sprites(dir: &Path) -> (Option<Vec<u32>>, Option<Vec<SpriteDef>>) {
    let page = std::fs::read(dir.join(SPRITE_PAGE))
        .ok()
        .and_then(|b| page_pixels(&b));
    let defs = std::fs::read(dir.join(SPRITE_DEFS))
        .ok()
        .and_then(|b| sprites::defs_from_bytes(&b));
    (page, defs)
}

/// Loads the font from the resource directory.
pub fn load_font(dir: &Path) -> Result<Font, ResourceError> {
    let map = read(&dir.join(FONT_MAP))?;
    let page = read(&dir.join(FONT_PAGE))?;
    Ok(Font::from_files(&map, &page)?)
}

/// SmileBASIC 4's instruments from the resource directory, `None` if they
/// were not imported or are damaged.
pub fn load_instruments(dir: &Path) -> Option<Bank> {
    Bank::from_bytes(&std::fs::read(dir.join(INSTRUMENTS)).ok()?)
}
