//! Files and projects, laid out as SmileBASIC 4 keeps them: one folder per
//! project, each file named by a one-letter type prefix and its name
//! (`TGAME.PRG` is the TXT file `GAME.PRG`).
//!
//! The workspace project (`PROJECT ""`) lives in the `@WORKSPACE` folder;
//! `@` cannot appear in project names, so it never clashes with one.

use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

const WORKSPACE_DIR: &str = "@WORKSPACE";
/// UNVERIFIED: SmileBASIC 4's limit on file and project name length.
pub const MAX_NAME: usize = 32;
/// The previous version of a program overwritten by SAVE.
pub const BACKUP_NAME: &str = "@BACKUP.PRG";

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
            // UNVERIFIED: SmileBASIC 4's prefix for JPG files.
            FileKind::Jpg => 'J',
        }
    }

    pub fn from_prefix(prefix: char) -> Option<FileKind> {
        KINDS.into_iter().find(|k| k.prefix() == prefix)
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

/// Checks a file name and returns it in upper case: up to 32 letters,
/// digits, `_`, `-` and `.`, not starting with `.`.
fn check_name(name: &str) -> Result<String> {
    let ok = |c: char| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.');
    if name.is_empty() || name.len() > MAX_NAME || name.starts_with('.') || !name.chars().all(ok) {
        return Err(StorageError::BadName(name.to_string()));
    }
    Ok(name.to_ascii_uppercase())
}

/// Checks a project name: the rules of a file name, after an optional `#`
/// that marks a system project (`#SYS`). SmileBASIC 4 cannot type `#`, but
/// system projects can be copied into the folder. The `#` counts toward the
/// 32 characters.
pub fn check_project_name(name: &str) -> Result<String> {
    let rest = name.strip_prefix('#').unwrap_or(name);
    check_name(rest)
        .ok()
        .filter(|_| name.len() <= MAX_NAME)
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
    match fixed.as_str() {
        "" => format!("{hash}PROJECT"),
        _ => format!("{hash}{fixed}"),
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
    let project = match project {
        "" => None,
        p => Some(check_project_name(p)?),
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

    /// Writes a file. When it replaces one, the old contents become the
    /// file `backup` of the same type, if given.
    pub fn write(
        &self,
        kind: FileKind,
        name: &str,
        bytes: &[u8],
        backup: Option<&str>,
    ) -> Result<()> {
        let path = self.path(kind, name)?;
        std::fs::create_dir_all(path.parent().expect("file in a project folder"))?;
        if let Some(backup) = backup
            && path.is_file()
            && name != backup
        {
            std::fs::rename(&path, self.path(kind, backup)?)?;
        }
        std::fs::write(path, bytes)?;
        Ok(())
    }

    /// A file of the current project's own, outside the typed files (whose
    /// names start with a type letter), such as its metadata.
    pub fn read_project_file(&self, name: &str) -> Result<Vec<u8>> {
        let path = self.dir(&self.project)?.join(name);
        std::fs::read(&path).map_err(|e| match e.kind() {
            io::ErrorKind::NotFound => StorageError::NotFound(name.to_string()),
            _ => e.into(),
        })
    }

    pub fn write_project_file(&self, name: &str, bytes: &[u8]) -> Result<()> {
        let dir = self.dir(&self.project)?;
        std::fs::create_dir_all(&dir)?;
        std::fs::write(dir.join(name), bytes)?;
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
}
