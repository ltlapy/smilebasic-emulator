//! Moving projects to and from SmileBASIC 4. Exporting writes a project's
//! files in SmileBASIC 4's container, signed, with a META file; importing
//! turns such a folder back into the emulator's plain files.
//! docs/file-formats.md describes both kinds of file.

use std::fs;
use std::path::{Path, PathBuf};

use crate::env::{self, Meta, SB4_META};
use crate::image;
use crate::sbfile::{self, Data, DataValues, Date, Kind, ProjectMeta, usage};
use crate::storage::{FileKind, Storage};

/// The folder SmileBASIC 4 keeps the workspace in.
pub const SB4_WORKSPACE: &str = "###";
/// The project name offered for an imported workspace.
const IMPORTED_WORKSPACE: &str = "WORKSPACE";
const JPEG_MAGIC: &[u8] = &[0xFF, 0xD8];

/// What an export or import did.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Summary {
    /// Files written, not counting the metadata.
    pub written: usize,
    /// Files left out, with why.
    pub skipped: Vec<(String, String)>,
}

impl Summary {
    /// One line for the project manager, after `done` ("Exported GAME").
    pub fn line(&self, done: &str) -> String {
        let files = match self.written {
            1 => "1 file".to_string(),
            n => format!("{n} files"),
        };
        match self.skipped.first() {
            None => format!("{done}: {files}."),
            Some((name, why)) => format!(
                "{done}: {files}; {} left out ({name}: {why}).",
                self.skipped.len()
            ),
        }
    }
}

/// The folder an export of `project` into `dest` writes.
pub fn export_target(project: &str, dest: &Path) -> PathBuf {
    dest.join(match project {
        "" => SB4_WORKSPACE,
        name => name,
    })
}

/// A name to offer for a project imported from `folder`.
pub fn import_name(folder: &Path) -> String {
    match folder.file_name().and_then(|n| n.to_str()) {
        Some(SB4_WORKSPACE) | None => IMPORTED_WORKSPACE.to_string(),
        Some(name) => crate::storage::suggest_project_name(name),
    }
}

/// A file of the emulator's as SmileBASIC 4.4.9 writes that type. A file
/// already in SmileBASIC 4's container is kept as it is.
pub fn export_file(kind: FileKind, bytes: &[u8], date: Date) -> Result<Vec<u8>, String> {
    if sbfile::is_container(bytes) && sbfile::parse(bytes).is_ok() {
        return Ok(bytes.to_vec());
    }
    Ok(match kind {
        // SAVEV's text files are compressed, with 0 at 0x06, but the
        // emulator cannot tell them from programs, so every text file goes
        // as a program.
        FileKind::Txt => {
            std::str::from_utf8(bytes).map_err(|_| "not UTF-8 text")?;
            sbfile::container(Kind::Text, usage::PROGRAM, date, bytes, false)
        }
        FileKind::Dat => {
            sbfile::parse_data(bytes).map_err(|e| e.to_string())?;
            sbfile::container(Kind::Data, usage::DAT, date, bytes, true)
        }
        FileKind::Grp => {
            let image = image::decode(bytes)?;
            let data = Data {
                dims: vec![image.width, image.height],
                values: DataValues::Int(image.pixels.iter().map(|&p| p as i32).collect()),
            };
            let body = sbfile::data_body(&data);
            sbfile::container(Kind::Grp, usage::GRP, date, &body, true)
        }
        FileKind::Jpg => {
            if !bytes.starts_with(JPEG_MAGIC) {
                return Err("not a JPEG file".to_string());
            }
            sbfile::container(Kind::Jpg, usage::JPG, date, bytes, false)
        }
    })
}

/// A file of SmileBASIC 4's as the emulator saves that type. A file that
/// is not in SmileBASIC 4's container is kept as it is.
pub fn import_file(kind: FileKind, bytes: &[u8]) -> Result<Vec<u8>, String> {
    if !sbfile::is_container(bytes) {
        return Ok(bytes.to_vec());
    }
    let file = sbfile::parse(bytes).map_err(|e| e.to_string())?;
    let expected = match kind {
        FileKind::Txt => file.kind == Kind::Text,
        FileKind::Dat => file.kind == Kind::Data && !file.is_image(),
        FileKind::Grp => file.is_image(),
        FileKind::Jpg => file.kind == Kind::Jpg,
    };
    if !expected {
        return Err(format!("not a {} file", kind.name()));
    }
    match kind {
        FileKind::Txt => file.text().map(drop).map_err(|e| e.to_string())?,
        FileKind::Dat => file.data().map(drop).map_err(|e| e.to_string())?,
        FileKind::Grp => return image::decode(bytes).map(|i| image::encode_png(&i)),
        FileKind::Jpg if !file.body.starts_with(JPEG_MAGIC) => {
            return Err("not a JPEG file".to_string());
        }
        FileKind::Jpg => {}
    };
    Ok(file.body)
}

/// SmileBASIC 4's META file for `meta`.
pub fn export_meta(meta: &Meta, date: Date) -> Vec<u8> {
    let meta = ProjectMeta {
        title: meta.title.clone(),
        description: meta.description.clone(),
        icon: meta.icon.iter().map(|&p| p as u32).collect(),
    };
    sbfile::container(Kind::Meta, 0, date, &sbfile::meta_body(&meta), false)
}

/// The files of a folder, sorted by name, with their contents and when
/// they last changed.
fn files(dir: &Path) -> Result<Vec<(String, Vec<u8>, Date)>, String> {
    let entries = fs::read_dir(dir).map_err(|e| e.to_string())?;
    let mut files = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|e| e.to_string())?;
        let meta = entry.metadata().map_err(|e| e.to_string())?;
        let Some(name) = entry.file_name().to_str().map(str::to_string) else {
            continue;
        };
        if !meta.is_file() {
            continue;
        }
        let bytes = fs::read(entry.path()).map_err(|e| format!("{name}: {e}"))?;
        let date = meta.modified().map_or_else(|_| Date::now(), Date::at);
        files.push((name, bytes, date));
    }
    files.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(files)
}

/// A file's type, from the letter its name starts with.
fn kind_of(name: &str) -> Option<FileKind> {
    let mut chars = name.chars();
    let kind = FileKind::from_prefix(chars.next()?)?;
    (!chars.as_str().is_empty()).then_some(kind)
}

fn write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    fs::write(path, bytes).map_err(|e| e.to_string())
}

/// Writes `project` into [`export_target`] of `dest`, replacing files of
/// the same name there.
pub fn export_project(storage: &Storage, project: &str, dest: &Path) -> Result<Summary, String> {
    let dir = storage.project_dir(project).map_err(|e| e.to_string())?;
    let target = export_target(project, dest);
    fs::create_dir_all(&target).map_err(|e| e.to_string())?;
    let mut summary = Summary::default();
    let present = match dir.is_dir() {
        true => files(&dir)?,
        false => Vec::new(),
    };
    for (name, bytes, date) in &present {
        let Some(kind) = kind_of(name) else {
            // The metadata's files go into META below.
            if !env::is_meta_file(name) {
                summary
                    .skipped
                    .push((name.clone(), "not a file type".to_string()));
            }
            continue;
        };
        match export_file(kind, bytes, *date) {
            Ok(out) => {
                write(&target.join(name), &out)?;
                summary.written += 1;
            }
            Err(why) => summary.skipped.push((name.clone(), why)),
        }
    }
    let read = |name: &str| fs::read(dir.join(name)).ok();
    let meta = env::project_meta(project, read);
    write(&target.join(SB4_META), &export_meta(&meta, Date::now()))?;
    Ok(summary)
}

/// Reads SmileBASIC 4's project folder `source` into `project`, which
/// must exist, replacing files of the same name.
pub fn import_project(storage: &Storage, source: &Path, project: &str) -> Result<Summary, String> {
    let dir = storage.project_dir(project).map_err(|e| e.to_string())?;
    let mut summary = Summary::default();
    for (name, bytes, _) in files(source)? {
        if name == SB4_META {
            let meta = sbfile::parse(&bytes)
                .ok()
                .filter(|f| f.kind == Kind::Meta)
                .and_then(|f| sbfile::parse_meta(&f.body).ok());
            let Some(meta) = meta else {
                summary.skipped.push((name, "not a META file".to_string()));
                continue;
            };
            let meta = Meta {
                project: project.to_string(),
                home: project.to_string(),
                title: meta.title,
                description: meta.description,
                icon: meta.icon.into_iter().map(|p| p as i32).collect(),
            };
            for (file, bytes) in env::meta_files(&meta) {
                write(&dir.join(file), &bytes)?;
            }
            continue;
        }
        let Some(kind) = kind_of(&name) else {
            summary.skipped.push((name, "not a file type".to_string()));
            continue;
        };
        match import_file(kind, &bytes) {
            Ok(out) => {
                write(&dir.join(&name), &out)?;
                summary.written += 1;
            }
            Err(why) => summary.skipped.push((name, why)),
        }
    }
    Ok(summary)
}
