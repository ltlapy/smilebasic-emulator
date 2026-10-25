//! Files and projects, laid out as SmileBASIC 4 keeps them: one folder per
//! project, each file named by a one-letter type prefix and its name
//! (`TGAME.PRG` is the TXT file `GAME.PRG`).
//!
//! The workspace project (`PROJECT ""`) lives in the `@WORKSPACE` folder;
//! `@` cannot appear in project names, so it never clashes with one.

use std::fmt;
use std::io;
use std::path::PathBuf;

const WORKSPACE_DIR: &str = "@WORKSPACE";
/// UNVERIFIED: SmileBASIC 4's limit on file and project name length.
pub const MAX_NAME: usize = 32;
/// The previous version of a file overwritten by SAVE.
pub const BACKUP_NAME: &str = "@BACKUP.PRG";

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum FileKind {
    Txt,
    Dat,
    Grp,
}

const KINDS: [FileKind; 3] = [FileKind::Txt, FileKind::Dat, FileKind::Grp];

impl FileKind {
    pub fn name(self) -> &'static str {
        match self {
            FileKind::Txt => "TXT",
            FileKind::Dat => "DAT",
            FileKind::Grp => "GRP",
        }
    }

    fn prefix(self) -> char {
        match self {
            FileKind::Txt => 'T',
            FileKind::Dat => 'B',
            FileKind::Grp => 'G',
        }
    }

    fn from_name(name: &str) -> Option<FileKind> {
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
    Io(io::Error),
}

impl fmt::Display for StorageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StorageError::Unavailable => write!(f, "no storage folder"),
            StorageError::BadName(n) => write!(f, "illegal name {n:?}"),
            StorageError::BadKind(k) => write!(f, "unknown file type {k:?}"),
            StorageError::NotFound(n) => write!(f, "{n} not found"),
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

/// Checks a file or project name and returns it in upper case.
// UNVERIFIED: which characters SmileBASIC 4 accepts, and that names are
// case-insensitive.
fn check_name(name: &str, allow_dot: bool) -> Result<String> {
    let ok = |c: char| c.is_ascii_alphanumeric() || c == '_' || c == '-' || (allow_dot && c == '.');
    if name.is_empty() || name.len() > MAX_NAME || !name.chars().all(ok) {
        return Err(StorageError::BadName(name.to_string()));
    }
    Ok(name.to_ascii_uppercase())
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
    Ok((kind, check_name(name, true)?))
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
    let project = match project {
        "" => None,
        p => Some(check_name(p, false)?),
    };
    Ok((kind, project))
}

pub struct Storage {
    root: Option<PathBuf>,
    project: String,
}

impl Storage {
    /// Storage under `root`, which holds one folder per project.
    pub fn new(root: PathBuf) -> Storage {
        Storage {
            root: Some(root),
            project: String::new(),
        }
    }

    /// No folder: every file operation fails. For headless runs and tests.
    pub fn none() -> Storage {
        Storage {
            root: None,
            project: String::new(),
        }
    }

    /// The current project; empty for the workspace.
    pub fn project(&self) -> &str {
        &self.project
    }

    /// Switches project, creating its folder if needed.
    // UNVERIFIED: whether SmileBASIC 4 creates a missing project.
    pub fn set_project(&mut self, name: &str) -> Result<()> {
        let name = match name {
            "" => String::new(),
            n => check_name(n, false)?,
        };
        std::fs::create_dir_all(self.dir(&name)?)?;
        self.project = name;
        Ok(())
    }

    fn dir(&self, project: &str) -> Result<PathBuf> {
        let root = self.root.as_ref().ok_or(StorageError::Unavailable)?;
        Ok(root.join(if project.is_empty() {
            WORKSPACE_DIR
        } else {
            project
        }))
    }

    fn path(&self, kind: FileKind, name: &str) -> Result<PathBuf> {
        Ok(self
            .dir(&self.project)?
            .join(format!("{}{name}", kind.prefix())))
    }

    pub fn exists(&self, kind: FileKind, name: &str) -> bool {
        self.path(kind, name).is_ok_and(|p| p.is_file())
    }

    pub fn read(&self, kind: FileKind, name: &str) -> Result<Vec<u8>> {
        let path = self.path(kind, name)?;
        std::fs::read(&path).map_err(|e| match e.kind() {
            io::ErrorKind::NotFound => StorageError::NotFound(format!("{}:{name}", kind.name())),
            _ => e.into(),
        })
    }

    /// Writes a file. When it replaces one and `backup` is set, the old
    /// contents become `@BACKUP.PRG` of the same type.
    pub fn write(&self, kind: FileKind, name: &str, bytes: &[u8], backup: bool) -> Result<()> {
        let path = self.path(kind, name)?;
        std::fs::create_dir_all(path.parent().expect("file in a project folder"))?;
        if backup && path.is_file() {
            std::fs::rename(&path, self.path(kind, BACKUP_NAME)?)?;
        }
        std::fs::write(path, bytes)?;
        Ok(())
    }

    pub fn delete(&self, kind: FileKind, name: &str) -> Result<()> {
        if !self.exists(kind, name) {
            return Err(StorageError::NotFound(format!("{}:{name}", kind.name())));
        }
        std::fs::remove_file(self.path(kind, name)?)?;
        Ok(())
    }

    pub fn rename(&self, kind: FileKind, from: &str, to: &str) -> Result<()> {
        if !self.exists(kind, from) {
            return Err(StorageError::NotFound(format!("{}:{from}", kind.name())));
        }
        std::fs::rename(self.path(kind, from)?, self.path(kind, to)?)?;
        Ok(())
    }

    /// Files of a project (the current one if `None`), optionally of one
    /// type, sorted by type and name.
    pub fn list(
        &self,
        kind: Option<FileKind>,
        project: Option<&str>,
    ) -> Result<Vec<(FileKind, String)>> {
        let dir = self.dir(project.unwrap_or(&self.project))?;
        let entries = match std::fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(e.into()),
        };
        let mut files = Vec::new();
        for entry in entries {
            let file_name = entry?.file_name();
            let Some(file_name) = file_name.to_str() else {
                continue;
            };
            let mut chars = file_name.chars();
            let Some(prefix) = chars.next() else { continue };
            let Some(k) = KINDS.into_iter().find(|k| k.prefix() == prefix) else {
                continue;
            };
            if kind.is_none_or(|want| want == k) {
                files.push((k, chars.as_str().to_string()));
            }
        }
        files.sort();
        Ok(files)
    }

    /// Project names, without the workspace.
    pub fn projects(&self) -> Result<Vec<String>> {
        let root = self.root.as_ref().ok_or(StorageError::Unavailable)?;
        let entries = match std::fs::read_dir(root) {
            Ok(entries) => entries,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(e.into()),
        };
        let mut projects = Vec::new();
        for entry in entries {
            let entry = entry?;
            if entry.file_type()?.is_dir()
                && let Some(name) = entry.file_name().to_str()
                && name != WORKSPACE_DIR
            {
                projects.push(name.to_string());
            }
        }
        projects.sort();
        Ok(projects)
    }
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
        assert_eq!(parse_listing("").unwrap(), (None, None));
        assert_eq!(parse_listing("TXT:").unwrap(), (Some(FileKind::Txt), None));
        assert_eq!(
            parse_listing("mygame").unwrap(),
            (None, Some("MYGAME".to_string()))
        );
    }

    #[test]
    fn files_and_projects() {
        let tmp = TempDir::new("files");
        let mut s = Storage::new(tmp.0.clone());
        s.write(FileKind::Txt, "A", b"1", true).unwrap();
        s.write(FileKind::Txt, "A", b"2", true).unwrap();
        s.write(FileKind::Dat, "D", b"x", false).unwrap();
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

        s.set_project("game").unwrap();
        assert_eq!(s.project(), "GAME");
        assert!(s.list(None, None).unwrap().is_empty());
        assert_eq!(s.list(None, Some("")).unwrap().len(), 2);
        assert_eq!(s.projects().unwrap(), ["GAME"]);
        assert!(s.set_project("@WORKSPACE").is_err());

        assert!(matches!(
            Storage::none().read(FileKind::Txt, "A"),
            Err(StorageError::Unavailable)
        ));
    }
}
