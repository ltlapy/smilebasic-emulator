//! Files and projects, laid out as SmileBASIC 4 keeps them: one folder per
//! project, each file named by a one-letter type prefix and its name
//! (`TGAME.PRG` is the TXT file `GAME.PRG`).
//!
//! The workspace project (`PROJECT ""`) lives in the `@WORKSPACE` folder;
//! `@` cannot appear in project names, so it never clashes with one.
//!
//! Projects get copied in from elsewhere, so nothing here follows a symbolic
//! link found among the projects or in one: such a link could lead a read or
//! a write out of the storage folder. The storage folder itself may be one.
//!
//! One more project can come from a folder of the user's own, outside the
//! storage folder: the one that holds a program run from the command line
//! (see [`Storage::open_folder`]). Its files may have plain names too
//! (`HGLIB.PRG.txt`), and it takes no links either.

use std::cell::RefCell;
use std::collections::BTreeSet;
use std::fmt;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::SystemTime;

const WORKSPACE_DIR: &str = "@WORKSPACE";
/// SmileBASIC 4's limit on file and project name length.
pub const MAX_NAME: usize = 32;
/// The previous version of a program overwritten by SAVE.
pub const BACKUP_NAME: &str = "@BACKUP.PRG";
/// The largest file read or saved, looked at before any of it is read, so
/// that a huge file in a project cannot take all the memory. SmileBASIC 4
/// has about 128 MiB for a program, so an array filling that, in strings
/// that grow as UTF-8 too, still fits.
pub const MAX_FILE: u64 = crate::sbfile::MAX_BODY as u64;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum FileKind {
    Txt,
    Dat,
    Grp,
    Jpg,
}

const KINDS: [FileKind; 4] = [FileKind::Txt, FileKind::Dat, FileKind::Grp, FileKind::Jpg];

impl FileKind {
    pub fn name(self) -> &'static str {
        match self {
            FileKind::Txt => "TXT",
            FileKind::Dat => "DAT",
            FileKind::Grp => "GRP",
            FileKind::Jpg => "JPG",
        }
    }

    /// The previous version of a file overwritten by SAVEV or SAVEG.
    pub fn backup_name(self) -> &'static str {
        match self {
            FileKind::Txt => "@BACKUP.TXT",
            FileKind::Dat => "@BACKUP.DAT",
            FileKind::Grp => "@BACKUP.GRP",
            FileKind::Jpg => "@BACKUP.JPG",
        }
    }

    /// The letter that starts the names of this type's files.
    pub fn prefix(self) -> char {
        match self {
            FileKind::Txt => 'T',
            FileKind::Dat => 'B',
            FileKind::Grp => 'G',
            // SmileBASIC 4's prefix for JPG files.
            FileKind::Jpg => 'J',
        }
    }

    pub fn from_prefix(prefix: char) -> Option<FileKind> {
        KINDS.into_iter().find(|k| k.prefix() == prefix)
    }

    pub(crate) fn from_name(name: &str) -> Option<FileKind> {
        KINDS
            .into_iter()
            .find(|k| k.name().eq_ignore_ascii_case(name))
    }
}

#[derive(Debug)]
pub enum StorageError {
    /// No storage folder was configured.
    Unavailable,
    BadName(String),
    BadKind(String),
    NotFound(String),
    Exists(String),
    Io(io::Error),
}

impl fmt::Display for StorageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StorageError::Unavailable => write!(f, "no storage folder"),
            StorageError::BadName(n) => write!(f, "illegal name {n:?}"),
            StorageError::BadKind(k) => write!(f, "unknown file type {k:?}"),
            StorageError::NotFound(n) => write!(f, "{n} not found"),
            StorageError::Exists(n) => write!(f, "{n} already exists"),
            StorageError::Io(e) => write!(f, "{e}"),
        }
    }
}

impl From<io::Error> for StorageError {
    fn from(e: io::Error) -> Self {
        StorageError::Io(e)
    }
}

pub type Result<T> = std::result::Result<T, StorageError>;

fn file_label(path: &Path) -> String {
    path.file_name()
        .map_or_else(|| path.display().to_string(), |n| n.display().to_string())
}

fn link_refused(path: &Path) -> io::Error {
    let what = format!("{} is a symbolic link", file_label(path));
    io::Error::new(io::ErrorKind::PermissionDenied, what)
}

/// Whether `path` is itself a regular file, not a link to one.
fn is_real_file(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok_and(|m| m.is_file())
}

fn too_large(path: &Path, size: u64) -> io::Error {
    let what = format!(
        "{} is too large ({size} bytes, the most is {MAX_FILE})",
        file_label(path)
    );
    io::Error::new(io::ErrorKind::InvalidData, what)
}

/// Reads a file of a project, which must be a regular file, not a link, nor
/// a pipe that would wait forever, of [`MAX_FILE`] bytes at most.
pub fn read_regular(path: &Path) -> io::Result<Vec<u8>> {
    let meta = std::fs::symlink_metadata(path)?;
    if meta.file_type().is_symlink() {
        return Err(link_refused(path));
    }
    if !meta.is_file() {
        let what = format!("{} is not a file", file_label(path));
        return Err(io::Error::new(io::ErrorKind::InvalidInput, what));
    }
    if meta.len() > MAX_FILE {
        return Err(too_large(path, meta.len()));
    }
    std::fs::read(path)
}

/// Reads a file the user picked, from anywhere, so through a link too.
pub fn read_chosen(path: &Path) -> io::Result<Vec<u8>> {
    let meta = std::fs::metadata(path)?;
    if meta.is_file() && meta.len() > MAX_FILE {
        return Err(too_large(path, meta.len()));
    }
    std::fs::read(path)
}

/// Fails unless writing to `path` would make or replace a regular file.
fn check_writable(path: &Path) -> io::Result<()> {
    match std::fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_symlink() => Err(link_refused(path)),
        Ok(meta) if !meta.is_file() => {
            let what = format!("{} is not a file", file_label(path));
            Err(io::Error::new(io::ErrorKind::InvalidInput, what))
        }
        Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
        _ => Ok(()),
    }
}

/// Writes a file of a project. When it replaces one, the old contents
/// move to `backup` first, if given.
fn write_with_backup(path: &Path, bytes: &[u8], backup: Option<&Path>) -> Result<()> {
    // It could not be read back.
    if bytes.len() as u64 > MAX_FILE {
        return Err(too_large(path, bytes.len() as u64).into());
    }
    check_writable(path)?;
    std::fs::create_dir_all(path.parent().expect("file in a project folder"))?;
    // A link at the backup's name is replaced, not followed.
    if let Some(backup) = backup
        && is_real_file(path)
    {
        std::fs::rename(path, backup)?;
    }
    std::fs::write(path, bytes)?;
    Ok(())
}

/// Writes a regular file, refusing a link in its place, which would carry
/// the write to wherever it leads, even to a file not there yet.
pub fn write_regular(path: &Path, bytes: &[u8]) -> io::Result<()> {
    check_writable(path)?;
    std::fs::write(path, bytes)
}

/// Checks a file name and returns it in upper case: up to 32 letters,
/// digits, `_`, `-` and `.`, not starting with `.`. It may start with `@`,
/// as the backups' names do (`@BACKUP.PRG`); programs can read and save
/// such names (checked on hardware).
fn check_name(name: &str) -> Result<String> {
    if let Some(rest) = name.strip_prefix('@') {
        return check_plain_name(rest)
            .ok()
            .filter(|_| name.len() <= MAX_NAME)
            .map(|n| format!("@{n}"))
            .ok_or_else(|| StorageError::BadName(name.to_string()));
    }
    check_plain_name(name)
}

fn check_plain_name(name: &str) -> Result<String> {
    let ok = |c: char| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.');
    if name.is_empty()
        || name.len() > MAX_NAME
        || name.starts_with('.')
        || !name.chars().all(ok)
        || (cfg!(windows) && name.ends_with('.'))
    {
        return Err(StorageError::BadName(name.to_string()));
    }
    Ok(name.to_ascii_uppercase())
}

/// Whether Windows takes a name for a device, whatever follows its first
/// dot, rather than for a folder. Files escape this by their type letter;
/// only a project's folder can be named so.
///
/// UNVERIFIED: whether SmileBASIC 4 takes these names, and names ending in
/// a dot. It runs on no Windows disk, so a project called `CON` may well
/// exist; only Windows, which cannot hold them, refuses them.
fn is_windows_device(name: &str) -> bool {
    let stem = name.split('.').next().unwrap_or(name).to_ascii_uppercase();
    match stem.as_bytes() {
        b"CON" | b"PRN" | b"AUX" | b"NUL" => true,
        [b'C', b'O', b'M', digit] | [b'L', b'P', b'T', digit] => digit.is_ascii_digit(),
        _ => false,
    }
}

/// `name` made into one that Windows can hold as a folder, for
/// [`suggest_project_name`].
fn windows_folder_name(name: String) -> String {
    let trimmed = name.trim_end_matches('.');
    match trimmed {
        "" | "#" => format!("{trimmed}PROJECT"),
        _ if is_windows_device(trimmed) => {
            let kept = trimmed.chars().take(MAX_NAME - 1);
            format!("_{}", kept.collect::<String>())
        }
        _ => trimmed.to_string(),
    }
}

/// A file named for reading: `[TYPE:][PROJECT/]NAME`, where an empty
/// project (`"/NAME"`) is the workspace.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileRef {
    pub kind: FileKind,
    /// `None` for the current project.
    pub project: Option<String>,
    pub name: String,
}

impl FileRef {
    /// `TXT:NAME`, or `TXT:PROJECT/NAME` in another project.
    pub fn label(&self) -> String {
        match &self.project {
            None => format!("{}:{}", self.kind.name(), self.name),
            Some(p) => format!("{}:{p}/{}", self.kind.name(), self.name),
        }
    }
}

/// Splits `"[TYPE:][PROJECT/]NAME"` for reading; see [`FileRef`].
pub fn parse_read_name(spec: &str, default: FileKind) -> Result<FileRef> {
    let (kind, rest) = match spec.split_once(':') {
        Some((kind, rest)) => (
            FileKind::from_name(kind).ok_or_else(|| StorageError::BadKind(kind.to_string()))?,
            rest,
        ),
        None => (default, spec),
    };
    let (project, name) = match rest.split_once('/') {
        Some(("", name)) => (Some(String::new()), name),
        Some((project, name)) => (Some(check_project_name(project)?), name),
        None => (None, rest),
    };
    Ok(FileRef {
        kind,
        project,
        name: check_name(name)?,
    })
}

/// Checks a project name: the rules of a file name, after an optional `#`
/// that marks a system project (`#SYS`). SmileBASIC 4 cannot type `#`, but
/// system projects can be copied into the folder. The `#` counts toward the
/// 32 characters.
pub fn check_project_name(name: &str) -> Result<String> {
    let rest = name.strip_prefix('#').unwrap_or(name);
    // `@` stays out of project names, so `@WORKSPACE` never clashes.
    check_plain_name(rest)
        .ok()
        .filter(|_| name.len() <= MAX_NAME && !(cfg!(windows) && is_windows_device(name)))
        .ok_or(())
        .map(|n| format!("{}{n}", &name[..name.len() - rest.len()]))
        .map_err(|_| StorageError::BadName(name.to_string()))
}

/// A legal project name close to `name`, to offer when a folder copied in
/// by hand breaks the rules.
pub fn suggest_project_name(name: &str) -> String {
    let (hash, rest) = match name.strip_prefix('#') {
        Some(rest) => ("#", rest),
        None => ("", name),
    };
    let fixed: String = rest
        .chars()
        .map(|c| match c {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '_' | '-' | '.' => c.to_ascii_uppercase(),
            _ => '_',
        })
        .collect();
    let room = MAX_NAME - hash.len();
    let fixed: String = fixed.trim_start_matches('.').chars().take(room).collect();
    let suggested = match fixed.as_str() {
        "" => format!("{hash}PROJECT"),
        _ => format!("{hash}{fixed}"),
    };
    match cfg!(windows) {
        true => windows_folder_name(suggested),
        false => suggested,
    }
}

/// Splits `"[TYPE:]NAME"` into its type (or `default`) and checked name.
pub fn parse_file_name(spec: &str, default: FileKind) -> Result<(FileKind, String)> {
    let (kind, name) = match spec.split_once(':') {
        Some((kind, name)) => (
            FileKind::from_name(kind).ok_or_else(|| StorageError::BadKind(kind.to_string()))?,
            name,
        ),
        None => (default, spec),
    };
    Ok((kind, check_name(name)?))
}

/// Splits FILES' `"[TYPE:][PROJECT]"` into an optional type and project.
pub fn parse_listing(spec: &str) -> Result<(Option<FileKind>, Option<String>)> {
    let (kind, project) = match spec.split_once(':') {
        Some((kind, project)) => (
            Some(FileKind::from_name(kind).ok_or_else(|| StorageError::BadKind(kind.to_string()))?),
            project,
        ),
        None => (None, spec),
    };
    // A `/` may follow the project (`TXT:NAME/`); on its own it is the
    // workspace.
    let project = match project {
        "" => None,
        "/" => Some(String::new()),
        p => Some(check_project_name(p.strip_suffix('/').unwrap_or(p))?),
    };
    Ok((kind, project))
}

#[derive(Clone)]
pub struct Storage {
    root: Option<PathBuf>,
    project: String,
    /// The user's folder taken for a project, shared by the copies that
    /// subprograms get, so that they see the same files as written.
    folder: Option<Rc<Folder>>,
}

impl Storage {
    /// Storage under `root`, which holds one folder per project.
    pub fn new(root: PathBuf) -> Storage {
        Storage {
            root: Some(root),
            project: String::new(),
            folder: None,
        }
    }

    /// No folder: every file operation fails. For headless runs and tests.
    pub fn none() -> Storage {
        Storage {
            root: None,
            project: String::new(),
            folder: None,
        }
    }

    /// Takes `dir`, a folder outside the storage folder, for a project and
    /// switches to it; returns the project's name (see
    /// [`folder_project_name`]). A project of that name in the storage
    /// folder is out of reach meanwhile. Other projects and the workspace
    /// stay where they are.
    pub fn open_folder(&mut self, dir: &Path) -> io::Result<String> {
        let folder = Folder::open(dir)?;
        let name = folder.name.clone();
        self.folder = Some(Rc::new(folder));
        self.project = name.clone();
        Ok(name)
    }

    /// The folder [`Storage::open_folder`] opened.
    pub fn folder(&self) -> Option<&Path> {
        self.folder.as_deref().map(|f| f.dir.as_path())
    }

    /// `project`'s folder, if it is the user's folder.
    fn folder_of(&self, project: &str) -> Option<&Folder> {
        self.folder.as_deref().filter(|f| f.name == project)
    }

    /// Forgets that a file of the user's folder was there from the start,
    /// once it is written, deleted or renamed.
    fn touched(&self, path: &Path) {
        if let Some(folder) = &self.folder
            && path.parent() == Some(folder.dir.as_path())
            && let Some(name) = path.file_name().and_then(|n| n.to_str())
        {
            folder.untouched.borrow_mut().remove(name);
        }
    }

    /// The name on disk of a file of the user's folder that was there
    /// when it was opened and that nothing has written, deleted or renamed
    /// since; `None` for any other file. Changing such a file asks first.
    pub fn original(&self, file: &FileRef) -> Option<String> {
        let folder = self.folder_of(file.project.as_deref().unwrap_or(&self.project))?;
        let entry = folder.find(file.kind, &file.name).ok().flatten()?;
        folder.untouched.borrow().contains(&entry).then_some(entry)
    }

    /// [`Storage::original`] for a file of the current project's own, such
    /// as its metadata.
    pub fn original_own(&self, name: &str) -> Option<String> {
        let folder = self.folder_of(&self.project)?;
        let there = is_real_file(&folder.dir.join(name));
        (there && folder.untouched.borrow().contains(name)).then(|| name.to_string())
    }

    /// The type and name that a file of the user's folder goes by, given
    /// its name on disk, if it maps to one.
    pub fn folder_file(&self, entry: &str) -> Option<(FileKind, String)> {
        let files = self.folder.as_deref()?.files().ok()?;
        let found = files.into_iter().find(|(_, _, e)| e == entry);
        found.map(|(kind, name, _)| (kind, name))
    }

    /// The current project; empty for the workspace.
    pub fn project(&self) -> &str {
        &self.project
    }

    /// Whether `name` (checked) is the workspace or an existing project.
    pub fn project_exists(&self, name: &str) -> bool {
        name.is_empty() || self.dir(name).is_ok_and(|d| d.is_dir())
    }

    /// Switches to an existing project, or to the workspace (`""`).
    pub fn set_project(&mut self, name: &str) -> Result<()> {
        let name = match name {
            "" => String::new(),
            n => check_project_name(n)?,
        };
        if !self.project_exists(&name) {
            return Err(StorageError::NotFound(name));
        }
        self.project = name;
        Ok(())
    }

    /// The folder that holds the projects.
    pub fn root(&self) -> Option<&Path> {
        self.root.as_deref()
    }

    /// Creates an empty project and returns its checked name.
    pub fn create_project(&self, name: &str) -> Result<String> {
        let name = check_project_name(name)?;
        let dir = self.dir(&name)?;
        if dir.exists() {
            return Err(StorageError::Exists(name));
        }
        std::fs::create_dir_all(dir)?;
        Ok(name)
    }

    /// The folder of an existing project, named as it is on disk, which
    /// may break the naming rules if it was copied there by hand.
    fn existing_project(&self, name: &str) -> Result<PathBuf> {
        // The user's own folder is not the emulator's to rename, delete
        // or copy whole.
        if self.folder_of(name).is_some() {
            let what = format!("{name} is the folder of the program run from the command line");
            return Err(io::Error::new(io::ErrorKind::PermissionDenied, what).into());
        }
        let plain = !name.is_empty()
            && name != WORKSPACE_DIR
            && name != "."
            && name != ".."
            && !name.contains(['/', '\\']);
        let dir = self.dir(name)?;
        if !plain || !dir.is_dir() {
            return Err(StorageError::NotFound(name.to_string()));
        }
        Ok(dir)
    }

    /// Renames a project and returns the new, checked name. Renaming the
    /// current project keeps it current.
    pub fn rename_project(&mut self, from: &str, to: &str) -> Result<String> {
        let old = self.existing_project(from)?;
        let to = check_project_name(to)?;
        let new = self.dir(&to)?;
        // A change of case only finds itself on case-insensitive disks.
        if !to.eq_ignore_ascii_case(from) && new.exists() {
            return Err(StorageError::Exists(to));
        }
        std::fs::rename(old, new)?;
        if self.project == from {
            self.project = to.clone();
        }
        Ok(to)
    }

    /// Deletes a project and its files. Deleting the current one goes back
    /// to the workspace, as on SmileBASIC 4.
    pub fn delete_project(&mut self, name: &str) -> Result<()> {
        let dir = self.existing_project(name)?;
        std::fs::remove_dir_all(dir)?;
        if name == self.project {
            self.project = String::new();
        }
        Ok(())
    }

    /// The folder of a project (checked, or `""`), which may not exist yet.
    pub fn project_dir(&self, project: &str) -> Result<PathBuf> {
        self.dir(project)
    }

    /// Every way to a project goes through here, so that a link in the
    /// storage folder posing as a project, the workspace included, leads
    /// nowhere; [`Storage::projects`] does not list one either.
    fn dir(&self, project: &str) -> Result<PathBuf> {
        if let Some(folder) = self.folder_of(project) {
            return Ok(folder.dir.clone());
        }
        let root = self.root.as_ref().ok_or(StorageError::Unavailable)?;
        let dir = root.join(if project.is_empty() {
            WORKSPACE_DIR
        } else {
            project
        });
        if std::fs::symlink_metadata(&dir).is_ok_and(|m| m.file_type().is_symlink()) {
            return Err(link_refused(&dir).into());
        }
        Ok(dir)
    }

    /// The path of a typed file of a project: its type letter and name,
    /// or in the user's folder, the name it has there or, for a new file,
    /// the one it takes (see [`Folder::file_name`]).
    fn typed_path(&self, project: &str, kind: FileKind, name: &str) -> Result<PathBuf> {
        let dir = self.dir(project)?;
        let file = match self.folder_of(project) {
            Some(folder) => folder.file_name(kind, name)?,
            None => format!("{}{name}", kind.prefix()),
        };
        Ok(dir.join(file))
    }

    fn path(&self, kind: FileKind, name: &str) -> Result<PathBuf> {
        self.typed_path(&self.project, kind, name)
    }

    /// The path of a file named for reading, in its project.
    fn ref_path(&self, file: &FileRef) -> Result<PathBuf> {
        let project = file.project.as_deref().unwrap_or(&self.project);
        if !self.project_exists(project) {
            return Err(StorageError::NotFound(project.to_string()));
        }
        self.typed_path(project, file.kind, &file.name)
    }

    /// Reads a typed file of any project, by the name the lists show.
    pub fn read_in(&self, project: &str, kind: FileKind, name: &str) -> Result<Vec<u8>> {
        let path = self.typed_path(project, kind, name)?;
        read_regular(&path).map_err(|e| match e.kind() {
            io::ErrorKind::NotFound => StorageError::NotFound(format!("{}:{name}", kind.name())),
            _ => e.into(),
        })
    }

    pub fn exists_ref(&self, file: &FileRef) -> bool {
        self.ref_path(file).is_ok_and(|p| is_real_file(&p))
    }

    pub fn read_ref(&self, file: &FileRef) -> Result<Vec<u8>> {
        let path = self.ref_path(file)?;
        read_regular(&path).map_err(|e| match e.kind() {
            io::ErrorKind::NotFound => StorageError::NotFound(file.label()),
            _ => e.into(),
        })
    }

    pub fn exists(&self, kind: FileKind, name: &str) -> bool {
        self.path(kind, name).is_ok_and(|p| is_real_file(&p))
    }

    pub fn read(&self, kind: FileKind, name: &str) -> Result<Vec<u8>> {
        let path = self.path(kind, name)?;
        read_regular(&path).map_err(|e| match e.kind() {
            io::ErrorKind::NotFound => StorageError::NotFound(format!("{}:{name}", kind.name())),
            _ => e.into(),
        })
    }

    /// Writes a file. When it replaces one, the old contents become the
    /// file `backup` of the same type, if given.
    pub fn write(
        &self,
        kind: FileKind,
        name: &str,
        bytes: &[u8],
        backup: Option<&str>,
    ) -> Result<()> {
        let backup = match backup {
            Some(backup) if name != backup => Some(self.path(kind, backup)?),
            _ => None,
        };
        let path = self.path(kind, name)?;
        self.write_marked(&path, bytes, backup.as_deref())
    }

    /// [`write_with_backup`], noting the files it changes.
    fn write_marked(&self, path: &Path, bytes: &[u8], backup: Option<&Path>) -> Result<()> {
        let written = write_with_backup(path, bytes, backup);
        self.touched(path);
        if let Some(backup) = backup {
            self.touched(backup);
        }
        written
    }

    /// [`Storage::write`] of a file named with a project, which need not
    /// be the current one but must exist.
    pub fn write_ref(&self, file: &FileRef, bytes: &[u8], backup: Option<&str>) -> Result<()> {
        let backup = match backup {
            Some(backup) if file.name != backup => Some(self.ref_path(&FileRef {
                name: backup.to_string(),
                ..file.clone()
            })?),
            _ => None,
        };
        let path = self.ref_path(file)?;
        self.write_marked(&path, bytes, backup.as_deref())
    }

    /// A file of the current project's own, outside the typed files (whose
    /// names start with a type letter), such as its metadata.
    pub fn read_project_file(&self, name: &str) -> Result<Vec<u8>> {
        self.read_file_in(&self.project, name)
    }

    pub fn write_project_file(&self, name: &str, bytes: &[u8]) -> Result<()> {
        self.write_file_in(&self.project, name, bytes)
    }

    /// Like [`Storage::read_project_file`], from any project.
    pub fn read_file_in(&self, project: &str, name: &str) -> Result<Vec<u8>> {
        let path = self.dir(project)?.join(name);
        read_regular(&path).map_err(|e| match e.kind() {
            io::ErrorKind::NotFound => StorageError::NotFound(name.to_string()),
            _ => e.into(),
        })
    }

    /// Writes a file of a project's own, such as its metadata, whole or
    /// not at all: into a temporary file first, which then replaces it.
    pub fn write_file_in(&self, project: &str, name: &str, bytes: &[u8]) -> Result<()> {
        let dir = self.dir(project)?;
        std::fs::create_dir_all(&dir)?;
        let target = dir.join(name);
        check_writable(&target)?;
        // A leading `.` keeps the temporary file out of the listings.
        let temporary = dir.join(format!(".{name}.tmp"));
        // One left by a crash goes; anything else there is refused, and
        // `create_new` does not follow a link that appears meanwhile.
        check_writable(&temporary)?;
        if is_real_file(&temporary) {
            std::fs::remove_file(&temporary)?;
        }
        let mut file =
            (std::fs::OpenOptions::new().write(true).create_new(true)).open(&temporary)?;
        let written = file.write_all(bytes);
        // Windows cannot rename a file that is open.
        drop(file);
        self.touched(&target);
        if let Err(e) = written.and_then(|()| std::fs::rename(&temporary, target)) {
            let _ = std::fs::remove_file(&temporary);
            return Err(e.into());
        }
        Ok(())
    }

    pub fn delete(&self, kind: FileKind, name: &str) -> Result<()> {
        self.delete_in(&self.project, kind, name)
    }

    /// Whether a file is in a project, which need not be the current one.
    pub fn exists_in(&self, project: &str, kind: FileKind, name: &str) -> bool {
        (self.typed_path(project, kind, name)).is_ok_and(|p| is_real_file(&p))
    }

    /// Deletes a file of a project, which need not be the current one.
    pub fn delete_in(&self, project: &str, kind: FileKind, name: &str) -> Result<()> {
        if !self.exists_in(project, kind, name) {
            return Err(StorageError::NotFound(format!("{}:{name}", kind.name())));
        }
        let path = self.typed_path(project, kind, name)?;
        std::fs::remove_file(&path)?;
        self.touched(&path);
        Ok(())
    }

    /// Copies a file from one project to another (or the same one) under
    /// the name `to.1`, replacing a file of that name. Returns the name
    /// as checked, in upper case.
    pub fn copy_file(
        &self,
        kind: FileKind,
        (from_project, name): (&str, &str),
        (to_project, to_name): (&str, &str),
    ) -> Result<String> {
        let to_name = check_name(to_name)?;
        if !self.project_exists(to_project) {
            return Err(StorageError::NotFound(to_project.to_string()));
        }
        let from = self.typed_path(from_project, kind, name)?;
        if !is_real_file(&from) {
            return Err(StorageError::NotFound(format!("{}:{name}", kind.name())));
        }
        let to = self.typed_path(to_project, kind, &to_name)?;
        if from != to {
            check_writable(&to)?;
            std::fs::create_dir_all(to.parent().expect("file in a project folder"))?;
            std::fs::copy(from, &to)?;
            self.touched(&to);
        }
        Ok(to_name)
    }

    pub fn rename(&self, kind: FileKind, from: &str, to: &str) -> Result<()> {
        if !self.exists(kind, from) {
            return Err(StorageError::NotFound(format!("{}:{from}", kind.name())));
        }
        let (from, to) = (self.path(kind, from)?, self.path(kind, to)?);
        std::fs::rename(&from, &to)?;
        self.touched(&from);
        self.touched(&to);
        Ok(())
    }

    /// Renames a file of a project, which need not be the current one,
    /// and returns the new name as checked, in upper case. A file of that
    /// name already there is not replaced.
    pub fn rename_in(&self, project: &str, kind: FileKind, from: &str, to: &str) -> Result<String> {
        let to = check_name(to)?;
        if !self.exists_in(project, kind, from) {
            return Err(StorageError::NotFound(format!("{}:{from}", kind.name())));
        }
        // A change of case only finds itself on case-insensitive disks.
        if !to.eq_ignore_ascii_case(from) && self.exists_in(project, kind, &to) {
            return Err(StorageError::Exists(format!("{}:{to}", kind.name())));
        }
        let old = self.typed_path(project, kind, from)?;
        let new = self.typed_path(project, kind, &to)?;
        std::fs::rename(&old, &new)?;
        self.touched(&old);
        self.touched(&new);
        Ok(to)
    }

    /// Copies a project, the workspace included, to a new project named
    /// after it with the first free number on the end (`GAME2`, `GAME3`),
    /// and returns the new name.
    pub fn duplicate_project(&self, name: &str) -> Result<String> {
        let from = match name {
            "" => self.dir("")?,
            name => self.existing_project(name)?,
        };
        let base = match name {
            "" => "WORKSPACE".to_string(),
            name => suggest_project_name(name),
        };
        let free = (2..1000)
            .map(|n| {
                let n = n.to_string();
                let kept: String = base.chars().take(MAX_NAME - n.len()).collect();
                format!("{kept}{n}")
            })
            // `COM` and 2 are a device on Windows.
            .filter(|to| check_project_name(to).is_ok())
            .find(|to| self.dir(to).is_ok_and(|d| !d.exists()))
            .ok_or_else(|| StorageError::Exists(base.clone()))?;
        let to = self.dir(&free)?;
        std::fs::create_dir_all(&to)?;
        if let Err(e) = copy_folder(&from, &to) {
            let _ = std::fs::remove_dir_all(&to);
            return Err(e.into());
        }
        Ok(free)
    }

    /// A file's size, and when it last changed in local time as
    /// `YYYY/MM/DD hh:mm:ss`, as SmileBASIC 4's file list shows them.
    pub fn file_info(&self, project: &str, kind: FileKind, name: &str) -> Option<(u64, String)> {
        let path = self.typed_path(project, kind, name).ok()?;
        let meta = std::fs::symlink_metadata(path)
            .ok()
            .filter(|m| m.is_file())?;
        let changed = chrono::DateTime::<chrono::Local>::from(meta.modified().ok()?);
        Some((meta.len(), changed.format("%Y/%m/%d %H:%M:%S").to_string()))
    }

    /// Files of a project (the current one if `None`), optionally of one
    /// type, sorted by type and name.
    pub fn list(
        &self,
        kind: Option<FileKind>,
        project: Option<&str>,
    ) -> Result<Vec<(FileKind, String)>> {
        let project = project.unwrap_or(&self.project);
        if let Some(folder) = self.folder_of(project) {
            let files = folder.files()?.into_iter();
            let files = files.filter(|(k, _, _)| kind.is_none_or(|want| want == *k));
            return Ok(files.map(|(k, name, _)| (k, name)).collect());
        }
        let dir = self.dir(project)?;
        let entries = match std::fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(e.into()),
        };
        let mut files = Vec::new();
        for entry in entries {
            let entry = entry?;
            if entry.file_type()?.is_symlink() {
                continue;
            }
            let file_name = entry.file_name();
            let Some(file_name) = file_name.to_str() else {
                continue;
            };
            let mut chars = file_name.chars();
            let Some(prefix) = chars.next() else { continue };
            let Some(k) = FileKind::from_prefix(prefix) else {
                continue;
            };
            if kind.is_none_or(|want| want == k) {
                files.push((k, chars.as_str().to_string()));
            }
        }
        files.sort();
        Ok(files)
    }

    /// When the storage folder (`None`) or a project's folder last had a
    /// file or folder added, removed or renamed in it; `None` while it is
    /// not there. The file lists compare it to see changes that other
    /// programs made. A file written over in place leaves it as it was.
    pub fn changed_at(&self, project: Option<&str>) -> Option<SystemTime> {
        let dir = match project {
            None => self.root.clone()?,
            Some(project) => self.dir(project).ok()?,
        };
        std::fs::metadata(dir).ok()?.modified().ok()
    }

    /// Project names, without the workspace. The user's folder is one of
    /// them, in place of a project of its name in the storage folder.
    pub fn projects(&self) -> Result<Vec<String>> {
        let root = self.root.as_ref().ok_or(StorageError::Unavailable)?;
        let mut projects: Vec<String> = self.folder.iter().map(|f| f.name.clone()).collect();
        let entries = match std::fs::read_dir(root) {
            Ok(entries) => entries,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(projects),
            Err(e) => return Err(e.into()),
        };
        for entry in entries {
            let entry = entry?;
            if entry.file_type()?.is_dir()
                && let Some(name) = entry.file_name().to_str()
                && name != WORKSPACE_DIR
                && self.folder_of(name).is_none()
            {
                projects.push(name.to_string());
            }
        }
        projects.sort();
        Ok(projects)
    }
}

/// The project name that a folder of the user's goes by: its own name in
/// upper case when that is a legal project name, or else the name that
/// the Projects window offers for a folder copied in by hand (each
/// character that is not allowed becomes `_`). A `#` at the start, which
/// marks SmileBASIC 4's own projects, becomes `_` too.
pub fn folder_project_name(folder: &str) -> String {
    let name = match folder.strip_prefix('#') {
        Some(rest) => format!("_{rest}"),
        None => folder.to_string(),
    };
    check_project_name(&name).unwrap_or_else(|_| suggest_project_name(&name))
}

/// The forms of a name in the user's folder, in the order they win when
/// two files map to the same type and name.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Form {
    /// SmileBASIC 4's: the type letter, then the name (`THGLIB.PRG`).
    Sb4,
    /// A text file's name and `.txt` (`HGLIB.PRG.txt`).
    Txt,
    /// A text file's name alone (`HGLIB.PRG`).
    Bare,
}

/// A folder of the user's, outside the storage folder, used as a project
/// (see [`Storage::open_folder`]). Its files keep the names they have on
/// disk, which can be SmileBASIC 4's or plain ones. Names compare without
/// regard to case, and each file maps to one type and name at most:
///
/// - SmileBASIC 4's form, when the folder uses it (see [`Folder::open`])
///   or the file's contents are of its type (SmileBASIC 4's container, or
///   the DAT, PNG or JPEG that the emulator writes);
/// - otherwise a text file: the name before `.txt`, or the whole name.
///
/// Names that break the rules map to nothing and stay out of the lists,
/// as do the project's metadata files. Of two files that map to one name,
/// the one in SmileBASIC 4's form wins, then the one ending in `.txt`.
#[derive(Debug)]
struct Folder {
    dir: PathBuf,
    name: String,
    /// Whether new files take SmileBASIC 4's names; otherwise plain names,
    /// for text files only.
    sb4: bool,
    /// Whether new plain text files end in `.txt`.
    txt: bool,
    /// The files there when the folder was opened, by their names on
    /// disk, that nothing has written, deleted or renamed since.
    untouched: RefCell<BTreeSet<String>>,
}

/// The names of the regular files in `dir`, links left out, sorted.
fn folder_entries(dir: &Path) -> io::Result<Vec<String>> {
    let mut names = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        if entry.file_type()?.is_file()
            && let Ok(name) = entry.file_name().into_string()
        {
            names.push(name);
        }
    }
    names.sort();
    Ok(names)
}

/// A name ending in `.txt`, in any case, without it.
fn txt_stem(entry: &str) -> Option<&str> {
    let cut = entry.len().checked_sub(4)?;
    let suffix = entry.get(cut..)?;
    suffix.eq_ignore_ascii_case(".txt").then(|| &entry[..cut])
}

/// A name in SmileBASIC 4's form split into its type and checked name.
fn sb4_name(entry: &str) -> Option<(FileKind, String)> {
    let mut chars = entry.chars();
    let kind = FileKind::from_prefix(chars.next()?.to_ascii_uppercase())?;
    Some((kind, check_name(chars.as_str()).ok()?))
}

/// Whether a file starts as SmileBASIC 4's container does, or for DAT,
/// GRP and JPG files, as the emulator writes them.
fn starts_as(path: &Path, kind: FileKind) -> bool {
    let mut start = Vec::with_capacity(8);
    let read = std::fs::File::open(path).and_then(|f| f.take(8).read_to_end(&mut start));
    read.is_ok()
        && (start.starts_with(&[4, 0])
            || match kind {
                FileKind::Txt => false,
                FileKind::Dat => start.starts_with(crate::sbfile::DATA_MAGIC),
                FileKind::Grp => start.starts_with(b"\x89PNG"),
                FileKind::Jpg => start.starts_with(&[0xFF, 0xD8]),
            })
}

impl Folder {
    /// Opens `dir` and settles how its new files are named, from the
    /// files there. It uses SmileBASIC 4's names when it holds a `META`
    /// file, a backup such as `T@BACKUP.PRG`, or a text file in
    /// SmileBASIC 4's container: these come from SmileBASIC 4 or the
    /// emulator's projects. Otherwise new text files get plain names,
    /// ending in `.txt` when at least half of the plain text files there
    /// do, and DAT, GRP and JPG files take SmileBASIC 4's names.
    fn open(dir: &Path) -> io::Result<Folder> {
        // The folder itself may be a link, as the storage folder may.
        let dir = std::fs::canonicalize(dir)?;
        if !dir.is_dir() {
            let what = format!("{} is not a folder", file_label(&dir));
            return Err(io::Error::new(io::ErrorKind::InvalidInput, what));
        }
        let entries = folder_entries(&dir)?;
        let name = dir.file_name().and_then(|n| n.to_str()).unwrap_or("");
        let sb4 = entries.iter().any(|entry| {
            entry.eq_ignore_ascii_case(crate::env::SB4_META)
                || matches!(sb4_name(entry), Some((FileKind::Txt, name))
                    if name.starts_with('@') || starts_as(&dir.join(entry), FileKind::Txt))
        });
        let mut folder = Folder {
            name: folder_project_name(name),
            dir,
            sb4,
            txt: true,
            untouched: RefCell::new(entries.iter().cloned().collect()),
        };
        if !sb4 {
            let forms: Vec<Form> = (entries.iter())
                .filter_map(|entry| folder.reading(entry).map(|(_, _, form)| form))
                .collect();
            let count = |form| forms.iter().filter(|&&f| f == form).count();
            folder.txt = count(Form::Txt) >= count(Form::Bare);
        }
        Ok(folder)
    }

    /// The type and name that the file `entry` maps to, if any, with the
    /// form of its name.
    fn reading(&self, entry: &str) -> Option<(FileKind, String, Form)> {
        if crate::env::is_meta_file(&entry.to_ascii_uppercase()) {
            return None;
        }
        if let Some((kind, name)) = sb4_name(entry)
            && (self.sb4 || starts_as(&self.dir.join(entry), kind))
        {
            return Some((kind, name, Form::Sb4));
        }
        match txt_stem(entry) {
            Some(stem) => check_name(stem).ok().map(|n| (FileKind::Txt, n, Form::Txt)),
            None => check_name(entry)
                .ok()
                .map(|n| (FileKind::Txt, n, Form::Bare)),
        }
    }

    /// The files there by type and name, sorted, with their names on disk.
    fn files(&self) -> io::Result<Vec<(FileKind, String, String)>> {
        let entries = match folder_entries(&self.dir) {
            Ok(entries) => entries,
            Err(e) if e.kind() == io::ErrorKind::NotFound => Vec::new(),
            Err(e) => return Err(e),
        };
        let mut found: Vec<_> = (entries.into_iter())
            .filter_map(|entry| {
                let (kind, name, form) = self.reading(&entry)?;
                Some((kind, name, form, entry))
            })
            .collect();
        found.sort();
        found.dedup_by(|a, b| a.0 == b.0 && a.1 == b.1);
        Ok(found.into_iter().map(|(k, n, _, e)| (k, n, e)).collect())
    }

    /// The name on disk of the file of this type and name, if there is one.
    fn find(&self, kind: FileKind, name: &str) -> io::Result<Option<String>> {
        let files = self.files()?.into_iter();
        Ok((files.filter(|(k, n, _)| *k == kind && n == name))
            .map(|(_, _, e)| e)
            .next())
    }

    /// The name on disk of a file, or, for a new file, the name it takes.
    fn file_name(&self, kind: FileKind, name: &str) -> io::Result<String> {
        Ok(self
            .find(kind, name)?
            .unwrap_or_else(|| self.new_name(kind, name)))
    }

    /// The name a new file takes, in the folder's way. A plain name that
    /// would map back to another name (`README.TXT` saved as is would be
    /// `README`), or to the metadata, takes the other plain form.
    fn new_name(&self, kind: FileKind, name: &str) -> String {
        let sb4 = format!("{}{name}", kind.prefix());
        if self.sb4 || kind != FileKind::Txt {
            return sb4;
        }
        let (txt, bare) = (format!("{name}.txt"), name.to_string());
        let forms = match self.txt {
            true => [txt, bare],
            false => [bare, txt],
        };
        let maps_back =
            |entry: &String| (self.reading(entry)).is_some_and(|(k, n, _)| k == kind && n == name);
        forms.into_iter().find(maps_back).unwrap_or(sb4)
    }
}

/// Copies the files and the folders in `from` into `to`, which exists.
/// Anything else, a link above all, stays behind, as it does on import.
fn copy_folder(from: &Path, to: &Path) -> io::Result<()> {
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        let kind = entry.file_type()?;
        if kind.is_dir() {
            std::fs::create_dir_all(&target)?;
            copy_folder(&entry.path(), &target)?;
        } else if kind.is_file() {
            std::fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fresh folder under the system temp directory, removed on drop.
    struct TempDir(PathBuf);

    impl TempDir {
        fn new(tag: &str) -> TempDir {
            let dir = std::env::temp_dir().join(format!("sb-storage-{tag}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            TempDir(dir)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn names() {
        assert_eq!(
            parse_file_name("game.prg", FileKind::Txt).unwrap(),
            (FileKind::Txt, "GAME.PRG".to_string())
        );
        assert_eq!(
            parse_file_name("grp:Pic", FileKind::Txt).unwrap(),
            (FileKind::Grp, "PIC".to_string())
        );
        assert!(parse_file_name("A/B", FileKind::Txt).is_err());
        assert!(parse_file_name("", FileKind::Txt).is_err());
        assert!(parse_file_name("XYZ:A", FileKind::Txt).is_err());
        assert!(parse_file_name(".A", FileKind::Txt).is_err());
        assert!(parse_file_name("#A", FileKind::Txt).is_err());
        assert_eq!(check_project_name("#sys").unwrap(), "#SYS");
        assert!(check_project_name("#").is_err());
        assert!(check_project_name("A#").is_err());
        assert!(check_project_name("A.B").is_ok());
        assert!(check_project_name(&"A".repeat(32)).is_ok());
        assert!(check_project_name(&format!("#{}", "A".repeat(31))).is_ok());
        assert!(check_project_name(&format!("#{}", "A".repeat(32))).is_err());
        assert_eq!(
            suggest_project_name(&format!("#{}", "a".repeat(40))).len(),
            32
        );
        assert_eq!(suggest_project_name("my game"), "MY_GAME");
        assert_eq!(suggest_project_name("#.ゲーム"), "#___");
        assert_eq!(suggest_project_name(".."), "PROJECT");
        let file = parse_read_name("dat:game/@backup.dat", FileKind::Txt).unwrap();
        assert_eq!(
            (file.kind, file.project.as_deref(), file.name.as_str()),
            (FileKind::Dat, Some("GAME"), "@BACKUP.DAT")
        );
        assert_eq!(file.label(), "DAT:GAME/@BACKUP.DAT");
        let file = parse_read_name("/MAIN.PRG", FileKind::Txt).unwrap();
        assert_eq!(file.project.as_deref(), Some(""), "the workspace");
        assert!(parse_read_name("A/B/C", FileKind::Txt).is_err());
        assert!(parse_read_name("@", FileKind::Txt).is_err());
        assert!(parse_read_name("A@", FileKind::Txt).is_err());
        assert!(parse_file_name("@X", FileKind::Txt).is_ok());
        assert!(check_project_name("@WORKSPACE").is_err());
        assert_eq!(parse_listing("").unwrap(), (None, None));
        assert_eq!(parse_listing("TXT:").unwrap(), (Some(FileKind::Txt), None));
        assert_eq!(
            parse_listing("mygame").unwrap(),
            (None, Some("MYGAME".to_string()))
        );
    }

    #[test]
    fn names_windows_cannot_hold() {
        for device in [
            "CON", "con", "PRN", "AUX", "NUL", "COM1", "COM9", "LPT1", "LPT9", "NUL.TXT", "CON.A.B",
        ] {
            assert!(is_windows_device(device), "{device}");
            assert_eq!(check_project_name(device).is_err(), cfg!(windows));
        }
        for fine in ["CONSOLE", "COM", "COM10", "#CON", "ACON", "LPTX", "NULL"] {
            assert!(!is_windows_device(fine), "{fine}");
            assert!(check_project_name(fine).is_ok(), "{fine}");
        }
        // A file's type letter keeps its name from being a device.
        assert!(check_name("CON").is_ok());
        for dotted in ["A.", "A..", "@A."] {
            assert_eq!(check_name(dotted).is_err(), cfg!(windows), "{dotted}");
        }
        assert_eq!(check_project_name("A.").is_err(), cfg!(windows));
        assert_eq!(windows_folder_name("CON".to_string()), "_CON");
        assert_eq!(windows_folder_name("NUL.TXT".to_string()), "_NUL.TXT");
        assert_eq!(windows_folder_name("#CON".to_string()), "#CON");
        assert_eq!(windows_folder_name("GAME.".to_string()), "GAME");
        assert_eq!(windows_folder_name("#.".to_string()), "#PROJECT");
        assert_eq!(windows_folder_name("GAME".to_string()), "GAME");
        // Spaces never were legal, trailing or not.
        assert!(check_project_name("A ").is_err());
    }

    #[test]
    fn files_and_projects() {
        let tmp = TempDir::new("files");
        let mut s = Storage::new(tmp.0.clone());
        s.write(FileKind::Txt, "A", b"1", Some(BACKUP_NAME))
            .unwrap();
        s.write(FileKind::Txt, "A", b"2", Some(BACKUP_NAME))
            .unwrap();
        s.write(FileKind::Dat, "D", b"x", None).unwrap();
        assert_eq!(s.read(FileKind::Txt, "A").unwrap(), b"2");
        assert_eq!(s.read(FileKind::Txt, BACKUP_NAME).unwrap(), b"1");
        assert_eq!(
            s.list(Some(FileKind::Txt), None).unwrap(),
            [
                (FileKind::Txt, BACKUP_NAME.to_string()),
                (FileKind::Txt, "A".to_string())
            ]
        );
        assert_eq!(s.list(None, None).unwrap().len(), 3);

        s.rename(FileKind::Txt, "A", "B").unwrap();
        assert!(!s.exists(FileKind::Txt, "A") && s.exists(FileKind::Txt, "B"));
        s.delete(FileKind::Txt, "B").unwrap();
        assert!(matches!(
            s.read(FileKind::Txt, "B"),
            Err(StorageError::NotFound(_))
        ));

        assert!(matches!(
            s.set_project("game"),
            Err(StorageError::NotFound(_))
        ));
        s.create_project("game").unwrap();
        s.set_project("game").unwrap();
        assert_eq!(s.project(), "GAME");
        assert!(s.list(None, None).unwrap().is_empty());
        assert_eq!(s.list(None, Some("")).unwrap().len(), 2);
        assert_eq!(s.projects().unwrap(), ["GAME"]);
        assert!(s.set_project("@WORKSPACE").is_err());
        s.create_project("#sys").unwrap();
        s.set_project("#sys").unwrap();
        assert_eq!(s.project(), "#SYS");
    }

    #[test]
    fn folder_times_for_the_lists() {
        let tmp = TempDir::new("times");
        let s = Storage::new(tmp.0.clone());
        assert_eq!(s.changed_at(None), None, "no folder yet");
        s.create_project("GAME").unwrap();
        let time = |dir: &Path| std::fs::metadata(dir).unwrap().modified().ok();
        assert_eq!(s.changed_at(None), time(&tmp.0));
        assert_eq!(s.changed_at(Some("GAME")), time(&tmp.0.join("GAME")));
        assert_eq!(s.changed_at(Some("NONE")), None);
        assert_eq!(Storage::none().changed_at(None), None);
    }

    #[test]
    fn copying_between_projects() {
        let tmp = TempDir::new("copy");
        let s = Storage::new(tmp.0.clone());
        s.create_project("B").unwrap();
        s.write(FileKind::Txt, "A", b"?1", None).unwrap();
        let name = s
            .copy_file(FileKind::Txt, ("", "A"), ("B", "c.prg"))
            .unwrap();
        assert_eq!(name, "C.PRG");
        assert!(s.exists_in("B", FileKind::Txt, "C.PRG"));
        assert!(s.copy_file(FileKind::Txt, ("", "A"), ("B", "a/b")).is_err());
        assert!(
            s.copy_file(FileKind::Txt, ("", "A"), ("NONE", "A"))
                .is_err()
        );
        assert!(s.copy_file(FileKind::Dat, ("", "A"), ("B", "A")).is_err());
        s.delete_in("B", FileKind::Txt, "C.PRG").unwrap();
        assert!(!s.exists_in("B", FileKind::Txt, "C.PRG"));
        assert!(s.delete_in("B", FileKind::Txt, "C.PRG").is_err());
    }

    #[test]
    fn managing_projects() {
        let tmp = TempDir::new("projects");
        let mut s = Storage::new(tmp.0.clone());
        assert_eq!(s.create_project("game").unwrap(), "GAME");
        assert!(matches!(
            s.create_project("GAME"),
            Err(StorageError::Exists(_))
        ));
        assert!(s.create_project("a b").is_err());

        s.set_project("GAME").unwrap();
        s.write(FileKind::Txt, "MAIN", b"1", None).unwrap();
        assert_eq!(s.rename_project("GAME", "#Demo").unwrap(), "#DEMO");
        assert_eq!(s.project(), "#DEMO", "the current project follows");
        assert_eq!(s.read(FileKind::Txt, "MAIN").unwrap(), b"1");

        // A folder copied in by hand keeps its name until renamed.
        std::fs::create_dir_all(tmp.0.join("my game")).unwrap();
        assert!(s.projects().unwrap().contains(&"my game".to_string()));
        assert_eq!(s.rename_project("my game", "MY_GAME").unwrap(), "MY_GAME");
        s.delete_project("#DEMO").unwrap();
        assert_eq!(s.project(), "", "back to the workspace");
        assert_eq!(s.projects().unwrap(), ["MY_GAME"]);
        for bad in ["", "@WORKSPACE", "..", "NONE"] {
            assert!(s.delete_project(bad).is_err(), "{bad:?}");
        }

        assert!(matches!(
            Storage::none().read(FileKind::Txt, "A"),
            Err(StorageError::Unavailable)
        ));
    }

    #[test]
    fn duplicating_and_renaming_in_projects() {
        let tmp = TempDir::new("duplicate");
        let s = Storage::new(tmp.0.clone());
        s.create_project("GAME").unwrap();
        s.write_file_in("GAME", "TMAIN.PRG", b"?1").unwrap();
        s.write_file_in("GAME", "@META.TXT", b"Game\n").unwrap();
        assert_eq!(s.duplicate_project("GAME").unwrap(), "GAME2");
        assert_eq!(s.duplicate_project("GAME").unwrap(), "GAME3");
        assert_eq!(s.read_file_in("GAME3", "TMAIN.PRG").unwrap(), b"?1");
        assert_eq!(s.read_file_in("GAME3", "@META.TXT").unwrap(), b"Game\n");
        s.write(FileKind::Dat, "D", b"", None).unwrap();
        assert_eq!(s.duplicate_project("").unwrap(), "WORKSPACE2");
        assert!(s.exists_in("WORKSPACE2", FileKind::Dat, "D"));
        assert!(s.duplicate_project("NONE").is_err());
        let long = "L".repeat(MAX_NAME);
        s.create_project(&long).unwrap();
        let copy = s.duplicate_project(&long).unwrap();
        assert_eq!(copy.len(), MAX_NAME);
        assert!(copy.ends_with('2'));

        assert_eq!(
            s.rename_in("GAME", FileKind::Txt, "MAIN.PRG", "start.prg")
                .unwrap(),
            "START.PRG"
        );
        assert!(s.exists_in("GAME", FileKind::Txt, "START.PRG"));
        assert!(!s.exists_in("GAME", FileKind::Txt, "MAIN.PRG"));
        s.write_file_in("GAME", "TB", b"").unwrap();
        assert!(matches!(
            s.rename_in("GAME", FileKind::Txt, "START.PRG", "B"),
            Err(StorageError::Exists(_))
        ));
        assert!(s.rename_in("GAME", FileKind::Txt, "B", "a/b").is_err());
        // The metadata is written whole, leaving no temporary file.
        s.write_file_in("GAME", "@META.TXT", b"New\n").unwrap();
        let names: Vec<_> = std::fs::read_dir(tmp.0.join("GAME"))
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        assert!(names.iter().all(|n| !n.ends_with(".tmp")), "{names:?}");
    }

    #[test]
    fn a_file_over_the_limit_is_refused_without_reading_it() {
        let tmp = TempDir::new("large");
        let mut s = Storage::new(tmp.0.clone());
        s.create_project("GAME").unwrap();
        s.set_project("GAME").unwrap();
        // Sparse, so that the test takes no room, and reading it whole would
        // show as the time and memory the check is there to save.
        let path = tmp.0.join("GAME/THUGE");
        let file = std::fs::File::create(&path).unwrap();
        file.set_len(MAX_FILE + 1).unwrap();
        drop(file);
        let too_large = |e: StorageError| e.to_string().contains("too large");
        assert!(s.read(FileKind::Txt, "HUGE").is_err_and(too_large));
        assert!(s.read_project_file("THUGE").is_err_and(too_large));
        let file = parse_read_name("HUGE", FileKind::Txt).unwrap();
        assert!(s.read_ref(&file).is_err_and(too_large));
        assert!(read_chosen(&path).is_err());
        assert!(s.exists(FileKind::Txt, "HUGE"), "it is still listed");

        let file = std::fs::File::create(&path).unwrap();
        file.set_len(16).unwrap();
        drop(file);
        assert_eq!(s.read(FileKind::Txt, "HUGE").unwrap().len(), 16);
        assert_eq!(read_chosen(&path).unwrap().len(), 16);
    }

    #[test]
    fn a_stale_temporary_file_goes_when_the_next_write_comes() {
        let tmp = TempDir::new("stale");
        let s = Storage::new(tmp.0.clone());
        s.create_project("GAME").unwrap();
        std::fs::write(tmp.0.join("GAME/.@META.TXT.tmp"), "old").unwrap();
        s.write_file_in("GAME", "@META.TXT", b"new").unwrap();
        assert_eq!(s.read_file_in("GAME", "@META.TXT").unwrap(), b"new");
        assert!(!tmp.0.join("GAME/.@META.TXT.tmp").exists());
    }

    #[test]
    fn something_other_than_a_file_is_not_read_or_written_as_one() {
        let tmp = TempDir::new("folder");
        let s = Storage::new(tmp.0.clone());
        std::fs::create_dir_all(tmp.0.join("@WORKSPACE/TA")).unwrap();
        assert!(s.read(FileKind::Txt, "A").is_err());
        assert!(s.write(FileKind::Txt, "A", b"", None).is_err());
        assert!(!s.exists(FileKind::Txt, "A"));
    }

    /// A folder of the user's holding `files` (name, contents), opened
    /// as the current project beside an empty storage folder.
    fn user_folder(tmp: &TempDir, folder: &str, files: &[(&str, &[u8])]) -> (Storage, PathBuf) {
        let dir = tmp.0.join(folder);
        std::fs::create_dir_all(&dir).unwrap();
        for (name, bytes) in files {
            std::fs::write(dir.join(name), bytes).unwrap();
        }
        let mut s = Storage::new(tmp.0.join("projects"));
        s.open_folder(&dir).unwrap();
        (s, dir)
    }

    fn on_disk(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = (std::fs::read_dir(dir).unwrap())
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        names.sort();
        names
    }

    fn txt(name: &str) -> (FileKind, String) {
        (FileKind::Txt, name.to_string())
    }

    #[test]
    fn folder_names_for_projects() {
        assert_eq!(folder_project_name("hgtest"), "HGTEST");
        assert_eq!(folder_project_name("hangul lib"), "HANGUL_LIB");
        assert_eq!(folder_project_name("#sys"), "_SYS");
        assert_eq!(folder_project_name("@WORKSPACE"), "_WORKSPACE");
        assert_eq!(folder_project_name("ハングル"), "____");
        assert_eq!(folder_project_name(""), "PROJECT");
    }

    #[test]
    fn a_plain_folder_maps_both_forms_and_leaves_out_the_rest() {
        let tmp = TempDir::new("folder-plain");
        let (s, _) = user_folder(
            &tmp,
            "hgtest",
            &[
                ("HGLIB.PRG.txt", b"lib"),
                ("hgtest.prg.txt", b"test"),
                ("TEST.PRG", b"plain"),
                ("BSCORES", b"PCBN0004 and the rest"),
                ("GPIC", b"\x89PNG..."),
                ("GAME.PRG", b"not a GRP"),
                ("bad name.txt", b""),
                ("@META.TXT", b"Title\n"),
                (".hidden", b""),
            ],
        );
        assert_eq!(s.project(), "HGTEST");
        assert_eq!(
            s.list(None, None).unwrap(),
            [
                txt("GAME.PRG"),
                txt("HGLIB.PRG"),
                txt("HGTEST.PRG"),
                txt("TEST.PRG"),
                (FileKind::Dat, "SCORES".to_string()),
                (FileKind::Grp, "PIC".to_string()),
            ]
        );
        assert_eq!(s.read(FileKind::Txt, "HGTEST.PRG").unwrap(), b"test");
        assert_eq!(s.read(FileKind::Txt, "TEST.PRG").unwrap(), b"plain");
        assert!(!s.exists(FileKind::Txt, "EST.PRG"), "a plain name it is");
        assert!(s.exists(FileKind::Dat, "SCORES"));
        assert!(!s.exists(FileKind::Txt, "BSCORES"));
        // Named with the project, as another project's files are.
        let file = parse_read_name("HGTEST/HGLIB.PRG", FileKind::Txt).unwrap();
        assert_eq!(s.read_ref(&file).unwrap(), b"lib");
        assert_eq!(s.folder_file("hgtest.prg.txt"), Some(txt("HGTEST.PRG")));
        assert_eq!(s.folder_file("@META.TXT"), None);
        assert_eq!(s.read_project_file("@META.TXT").unwrap(), b"Title\n");
    }

    #[test]
    fn of_two_files_for_one_name_the_sb4_form_wins_then_txt() {
        let tmp = TempDir::new("folder-conflict");
        let (s, _) = user_folder(
            &tmp,
            "GAME",
            &[
                ("META", b""),
                ("THGLIB.PRG", b"sb4"),
                ("HGLIB.PRG.txt", b"txt"),
                ("MAIN.PRG.txt", b"txt"),
                ("MAIN.PRG", b"bare"),
            ],
        );
        assert_eq!(s.read(FileKind::Txt, "HGLIB.PRG").unwrap(), b"sb4");
        assert_eq!(s.read(FileKind::Txt, "MAIN.PRG").unwrap(), b"txt");
        assert_eq!(
            s.list(None, None).unwrap(),
            [txt("HGLIB.PRG"), txt("MAIN.PRG")]
        );
    }

    #[test]
    fn a_folder_with_sb4_names_reads_them_so() {
        let tmp = TempDir::new("folder-sb4");
        let (s, dir) = user_folder(
            &tmp,
            "GAME",
            &[
                ("TMAIN.PRG", b"main"),
                ("T@BACKUP.PRG", b"old"),
                ("BDATA", b"no header"),
            ],
        );
        assert_eq!(
            s.list(None, None).unwrap(),
            [
                txt("@BACKUP.PRG"),
                txt("MAIN.PRG"),
                (FileKind::Dat, "DATA".to_string())
            ]
        );
        s.write(FileKind::Txt, "NEW", b"1", None).unwrap();
        s.write(FileKind::Dat, "D", b"1", None).unwrap();
        assert!(dir.join("TNEW").is_file() && dir.join("BD").is_file());
    }

    #[test]
    fn new_files_follow_the_folders_names_and_old_ones_keep_theirs() {
        let tmp = TempDir::new("folder-new");
        let (s, dir) = user_folder(
            &tmp,
            "LIB",
            &[
                ("hglib.prg.txt", b"1"),
                ("HGTEST.PRG.txt", b""),
                ("MAIN.PRG", b""),
            ],
        );
        s.write(FileKind::Txt, "HGLIB.PRG", b"2", Some(BACKUP_NAME))
            .unwrap();
        s.write(FileKind::Txt, "NEW.PRG", b"", Some(BACKUP_NAME))
            .unwrap();
        s.write(FileKind::Txt, "README.TXT", b"", None).unwrap();
        s.write(FileKind::Txt, "META", b"", None).unwrap();
        s.write(FileKind::Dat, "SCORES", b"PCBN0004", None).unwrap();
        assert_eq!(
            on_disk(&dir),
            [
                "@BACKUP.PRG.txt",
                "BSCORES",
                "HGTEST.PRG.txt",
                "MAIN.PRG",
                "META.txt",
                "NEW.PRG.txt",
                "README.TXT.txt",
                "hglib.prg.txt",
            ]
        );
        assert_eq!(s.read(FileKind::Txt, "HGLIB.PRG").unwrap(), b"2");
        assert_eq!(s.read(FileKind::Txt, BACKUP_NAME).unwrap(), b"1");
        assert_eq!(s.read(FileKind::Txt, "README.TXT").unwrap(), b"");
        assert!(s.exists(FileKind::Dat, "SCORES"));

        s.rename(FileKind::Txt, "MAIN.PRG", "START.PRG").unwrap();
        assert!(dir.join("START.PRG.txt").is_file() && !dir.join("MAIN.PRG").exists());
        s.delete(FileKind::Txt, "HGTEST.PRG").unwrap();
        assert!(!dir.join("HGTEST.PRG.txt").exists());

        // Without `.txt` on most of them, new ones go without.
        let tmp = TempDir::new("folder-bare");
        let plain: [(&str, &[u8]); 3] = [("MAIN.PRG", b""), ("OTHER.PRG", b""), ("A.txt", b"")];
        let (s, dir) = user_folder(&tmp, "BARE", &plain);
        s.write(FileKind::Txt, "NEW.PRG", b"", None).unwrap();
        s.write(FileKind::Txt, "README.TXT", b"", None).unwrap();
        assert_eq!(
            on_disk(&dir),
            [
                "A.txt",
                "MAIN.PRG",
                "NEW.PRG",
                "OTHER.PRG",
                "README.TXT.txt"
            ]
        );
    }

    #[test]
    fn files_there_from_the_start_are_originals_until_changed() {
        let tmp = TempDir::new("folder-originals");
        let (s, _) = user_folder(&tmp, "LIB", &[("HGLIB.PRG.txt", b"1"), ("MAIN.PRG", b"")]);
        let file = |name: &str| parse_read_name(name, FileKind::Txt).unwrap();
        assert_eq!(
            s.original(&file("HGLIB.PRG")).as_deref(),
            Some("HGLIB.PRG.txt")
        );
        assert_eq!(s.original(&file("NEW.PRG")), None);
        s.write(FileKind::Txt, "HGLIB.PRG", b"2", None).unwrap();
        assert_eq!(s.original(&file("HGLIB.PRG")), None, "asked once");
        // A copy for a subprogram sees the same.
        let copy = s.clone();
        copy.delete(FileKind::Txt, "MAIN.PRG").unwrap();
        s.write(FileKind::Txt, "MAIN.PRG", b"new", None).unwrap();
        assert_eq!(s.original(&file("MAIN.PRG")), None);
        // Files elsewhere are no originals.
        let mut s = s;
        s.set_project("").unwrap();
        s.write(FileKind::Txt, "HGLIB.PRG", b"", None).unwrap();
        assert_eq!(s.original(&file("HGLIB.PRG")), None);
        assert_eq!(s.original(&file("LIB/MAIN.PRG")), None);
    }

    #[test]
    fn the_folder_stands_among_the_projects_but_stays_the_users() {
        let tmp = TempDir::new("folder-projects");
        let (mut s, dir) = user_folder(&tmp, "lib", &[("MAIN.PRG", b"?1")]);
        s.create_project("OTHER").unwrap();
        std::fs::create_dir_all(tmp.0.join("projects/LIB")).unwrap();
        assert_eq!(s.projects().unwrap(), ["LIB", "OTHER"]);
        assert!(s.create_project("LIB").is_err());
        s.set_project("OTHER").unwrap();
        s.write(FileKind::Txt, "A", b"", None).unwrap();
        assert!(tmp.0.join("projects/OTHER/TA").is_file());
        s.set_project("LIB").unwrap();
        assert_eq!(s.read(FileKind::Txt, "MAIN.PRG").unwrap(), b"?1");
        for refused in [
            s.delete_project("LIB").err(),
            s.rename_project("LIB", "X").err(),
            s.duplicate_project("LIB").err(),
        ] {
            assert!(refused.is_some());
        }
        assert!(dir.join("MAIN.PRG").is_file());
        assert_eq!(s.project(), "LIB");
        assert_eq!(
            s.copy_file(FileKind::Txt, ("LIB", "MAIN.PRG"), ("OTHER", "M"))
                .unwrap(),
            "M"
        );
        assert_eq!(s.read_in("OTHER", FileKind::Txt, "M").unwrap(), b"?1");
        assert!(s.file_info("LIB", FileKind::Txt, "MAIN.PRG").is_some());
    }

    /// Planted links, each leading to a file outside the storage folder.
    #[cfg(unix)]
    mod links {
        use super::*;
        use std::os::unix::fs::symlink;

        /// A storage folder inside a temporary folder that also holds the
        /// file `outside`, which must never change or be read.
        fn arena(tag: &str) -> (TempDir, Storage, PathBuf) {
            let tmp = TempDir::new(tag);
            let root = tmp.0.join("root");
            std::fs::create_dir_all(root.join("@WORKSPACE")).unwrap();
            std::fs::create_dir_all(root.join("GAME")).unwrap();
            let outside = tmp.0.join("outside");
            std::fs::write(&outside, "outside").unwrap();
            (tmp, Storage::new(root), outside)
        }

        fn is_link(path: &Path) -> bool {
            std::fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_symlink())
        }

        #[test]
        fn a_link_at_a_temporary_files_name_takes_no_write() {
            let (tmp, s, outside) = arena("link-temp");
            symlink(&outside, tmp.0.join("root/GAME/.@META.TXT.tmp")).unwrap();
            assert!(s.write_file_in("GAME", "@META.TXT", b"new").is_err());
            assert_eq!(std::fs::read(&outside).unwrap(), b"outside");
            assert!(s.read_file_in("GAME", "@META.TXT").is_err());
        }

        #[test]
        fn a_link_at_a_metadata_files_name_stays_as_it_is() {
            let (tmp, s, outside) = arena("link-meta");
            let link = tmp.0.join("root/GAME/@META.TXT");
            symlink(&outside, &link).unwrap();
            assert!(s.write_file_in("GAME", "@META.TXT", b"new").is_err());
            assert_eq!(std::fs::read(&outside).unwrap(), b"outside");
            assert!(is_link(&link));
        }

        #[test]
        fn saving_over_the_backup_refuses_a_link_there() {
            let (tmp, s, outside) = arena("link-backup");
            symlink(&outside, tmp.0.join("root/@WORKSPACE/T@BACKUP.PRG")).unwrap();
            let saved = s.write(FileKind::Txt, BACKUP_NAME, b"new", Some(BACKUP_NAME));
            assert!(saved.is_err());
            assert_eq!(std::fs::read(&outside).unwrap(), b"outside");
        }

        #[test]
        fn a_backup_made_over_a_link_replaces_the_link() {
            let (tmp, s, outside) = arena("link-backup2");
            let link = tmp.0.join("root/@WORKSPACE/T@BACKUP.PRG");
            symlink(&outside, &link).unwrap();
            s.write(FileKind::Txt, "A", b"1", Some(BACKUP_NAME))
                .unwrap();
            s.write(FileKind::Txt, "A", b"2", Some(BACKUP_NAME))
                .unwrap();
            assert_eq!(std::fs::read(&outside).unwrap(), b"outside");
            assert!(!is_link(&link));
            assert_eq!(s.read(FileKind::Txt, BACKUP_NAME).unwrap(), b"1");
        }

        #[test]
        fn a_dangling_link_makes_no_file_where_it_points() {
            let (tmp, s, _) = arena("link-dangling");
            let target = tmp.0.join("created");
            symlink(&target, tmp.0.join("root/@WORKSPACE/TX")).unwrap();
            let saved = s.write(FileKind::Txt, "X", b"new", Some(BACKUP_NAME));
            assert!(saved.is_err());
            assert!(s.write(FileKind::Txt, "X", b"new", None).is_err());
            assert!(!target.exists());
        }

        #[test]
        fn copying_does_not_write_through_a_link_or_read_from_one() {
            let (tmp, s, outside) = arena("link-copy");
            s.write(FileKind::Txt, "A", b"mine", None).unwrap();
            symlink(&outside, tmp.0.join("root/GAME/TB")).unwrap();
            let copied = s.copy_file(FileKind::Txt, ("", "A"), ("GAME", "B"));
            assert!(copied.is_err());
            assert_eq!(std::fs::read(&outside).unwrap(), b"outside");
            let copied = s.copy_file(FileKind::Txt, ("GAME", "B"), ("", "C"));
            assert!(matches!(copied, Err(StorageError::NotFound(_))));
            assert!(!s.exists(FileKind::Txt, "C"));
        }

        #[test]
        fn a_linked_file_reads_as_nothing() {
            let (tmp, mut s, outside) = arena("link-read");
            symlink(&outside, tmp.0.join("root/@WORKSPACE/TLEAK")).unwrap();
            symlink(&outside, tmp.0.join("root/GAME/TLEAK")).unwrap();
            assert!(!s.exists(FileKind::Txt, "LEAK"));
            assert!(s.read(FileKind::Txt, "LEAK").is_err());
            assert!(s.read_file_in("GAME", "TLEAK").is_err());
            assert!(s.file_info("GAME", FileKind::Txt, "LEAK").is_none());
            assert!(!s.exists_in("GAME", FileKind::Txt, "LEAK"));
            assert!(s.list(None, None).unwrap().is_empty());
            let file = parse_read_name("GAME/LEAK", FileKind::Txt).unwrap();
            assert!(!s.exists_ref(&file));
            assert!(s.read_ref(&file).is_err());
            s.set_project("GAME").unwrap();
            assert!(s.read(FileKind::Txt, "LEAK").is_err());
        }

        #[test]
        fn a_linked_project_is_no_project() {
            let (tmp, mut s, _) = arena("link-project");
            let away = tmp.0.join("away");
            std::fs::create_dir_all(&away).unwrap();
            std::fs::write(away.join("TA"), "away").unwrap();
            symlink(&away, tmp.0.join("root/LINK")).unwrap();
            assert_eq!(s.projects().unwrap(), ["GAME"]);
            assert!(!s.project_exists("LINK"));
            assert!(matches!(
                s.set_project("LINK"),
                Err(StorageError::NotFound(_))
            ));
            let file = parse_read_name("LINK/A", FileKind::Txt).unwrap();
            assert!(s.read_ref(&file).is_err());
            assert!(s.read_file_in("LINK", "TA").is_err());
            assert!(s.write_file_in("LINK", "TB", b"").is_err());
            assert!(s.list(None, Some("LINK")).is_err());
            assert!(s.create_project("LINK").is_err());
            assert!(s.duplicate_project("LINK").is_err());
            assert!(s.delete_project("LINK").is_err());
            assert_eq!(std::fs::read_dir(&away).unwrap().count(), 1);
        }

        #[test]
        fn a_project_that_turns_into_a_link_takes_no_more_files() {
            let (tmp, mut s, _) = arena("link-current");
            s.set_project("GAME").unwrap();
            let away = tmp.0.join("away");
            std::fs::create_dir_all(&away).unwrap();
            std::fs::remove_dir(tmp.0.join("root/GAME")).unwrap();
            symlink(&away, tmp.0.join("root/GAME")).unwrap();
            assert!(s.write(FileKind::Txt, "A", b"", None).is_err());
            assert_eq!(std::fs::read_dir(&away).unwrap().count(), 0);
        }

        #[test]
        fn a_linked_workspace_is_refused_but_can_be_returned_to() {
            let (tmp, mut s, _) = arena("link-workspace");
            let away = tmp.0.join("away");
            std::fs::create_dir_all(&away).unwrap();
            std::fs::remove_dir(tmp.0.join("root/@WORKSPACE")).unwrap();
            symlink(&away, tmp.0.join("root/@WORKSPACE")).unwrap();
            s.set_project("").unwrap();
            assert!(s.write(FileKind::Txt, "A", b"", None).is_err());
            assert!(s.list(None, None).is_err());
            assert_eq!(std::fs::read_dir(&away).unwrap().count(), 0);
        }

        #[test]
        fn duplicating_leaves_links_behind() {
            let (tmp, s, outside) = arena("link-duplicate");
            s.write_file_in("GAME", "TMAIN.PRG", b"?1").unwrap();
            symlink(&outside, tmp.0.join("root/GAME/TLEAK")).unwrap();
            assert_eq!(s.duplicate_project("GAME").unwrap(), "GAME2");
            assert!(s.exists_in("GAME2", FileKind::Txt, "MAIN.PRG"));
            let leak = tmp.0.join("root/GAME2/TLEAK");
            assert!(std::fs::symlink_metadata(leak).is_err());
        }

        #[test]
        fn the_users_folder_takes_no_links_either() {
            let (tmp, _, outside) = arena("link-folder");
            let dir = tmp.0.join("mine");
            std::fs::create_dir_all(&dir).unwrap();
            symlink(&outside, dir.join("LEAK.PRG.txt")).unwrap();
            symlink(&outside, dir.join("TLEAK")).unwrap();
            let mut s = Storage::new(tmp.0.join("root"));
            s.open_folder(&dir).unwrap();
            assert!(s.list(None, None).unwrap().is_empty());
            assert!(s.read(FileKind::Txt, "LEAK.PRG").is_err());
            assert!(s.write(FileKind::Txt, "LEAK.PRG", b"new", None).is_err());
            assert!(s.write(FileKind::Txt, "LEAK", b"new", None).is_ok());
            assert_eq!(std::fs::read(&outside).unwrap(), b"outside");
            assert!(is_link(&dir.join("TLEAK")));
            // The folder itself may be a link, as the storage folder may.
            let linked = tmp.0.join("linked");
            symlink(&dir, &linked).unwrap();
            let mut s = Storage::new(tmp.0.join("root"));
            assert_eq!(s.open_folder(&linked).unwrap(), "MINE");
            assert_eq!(s.read(FileKind::Txt, "LEAK").unwrap(), b"new");
        }

        #[test]
        fn the_storage_folder_itself_may_be_a_link() {
            let (tmp, _, _) = arena("link-root");
            let moved = tmp.0.join("moved");
            symlink(tmp.0.join("root"), &moved).unwrap();
            let mut s = Storage::new(moved);
            s.write(FileKind::Txt, "A", b"1", Some(BACKUP_NAME))
                .unwrap();
            assert_eq!(s.read(FileKind::Txt, "A").unwrap(), b"1");
            s.set_project("GAME").unwrap();
            s.write_project_file("@META.TXT", b"t").unwrap();
            assert_eq!(s.read_project_file("@META.TXT").unwrap(), b"t");
            assert_eq!(s.projects().unwrap(), ["GAME"]);
        }
    }
}
