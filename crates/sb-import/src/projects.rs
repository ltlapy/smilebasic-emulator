//! Copying SmileBASIC 4's own projects (`#SYS`, `#TOOL`, `#EXAMPLE`,
//! `#GUIDE`) out of the RomFS into the emulator's projects folder, each
//! file turned into the form the emulator saves (the same as importing a
//! project does), never replacing a file that is already there.

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use sb_runtime::storage::FileKind;
use sb_runtime::transfer;

use crate::import::ImportError;

/// The projects SmileBASIC 4 keeps in its RomFS, under `basic/LANG/`.
const PROJECTS: [&str; 4] = ["#SYS", "#TOOL", "#EXAMPLE", "#GUIDE"];

/// The languages to look in, most preferred first, the same as for the
/// font of `#SYS`.
const LANGUAGES: [&str; 2] = ["en", "jp"];

/// The result of [`import_projects`].
#[derive(Debug, Default)]
pub struct Projects {
    /// Each project found, with the folder it was copied from and how
    /// many files were written.
    pub copied: Vec<(&'static str, PathBuf, usize)>,
    /// Files already there and the same, so left unchanged.
    pub unchanged: usize,
    /// Files already there but different, so kept, for example
    /// `#TOOL/TSBLIB.PRG`.
    pub kept: Vec<String>,
    /// Files that could not be converted to the emulator's form, or that
    /// have a link in their place, with the reason.
    pub skipped: Vec<(String, String)>,
    /// Projects no directory has.
    pub missing: Vec<&'static str>,
}

/// A project's folder: from the last directory that has it, so that the
/// folder of an update wins as a whole, in English if there is one.
fn locate(project: &str, dirs: &[PathBuf]) -> Option<PathBuf> {
    dirs.iter().rev().find_map(|root| {
        LANGUAGES
            .iter()
            .map(|lang| root.join("basic").join(lang).join(project))
            .find(|p| p.is_dir())
    })
}

/// The files of a folder, sorted by name.
fn files(dir: &Path) -> Result<Vec<(String, PathBuf)>, ImportError> {
    let io = |e| ImportError::Io(dir.to_path_buf(), e);
    let mut files = Vec::new();
    for entry in fs::read_dir(dir).map_err(io)? {
        let entry = entry.map_err(io)?;
        if !entry.file_type().map_err(io)?.is_file() {
            continue;
        }
        if let Some(name) = entry.file_name().to_str() {
            files.push((name.to_string(), entry.path()));
        }
    }
    files.sort();
    Ok(files)
}

/// Writes a file that must not exist yet. `create_new` fails if anything,
/// a link included, took the name since it was checked, so nothing there
/// is overwritten. A file left half written is removed, or the next
/// import would keep it as the user's file.
fn write_new(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?;
    file.write_all(bytes).inspect_err(|_| {
        drop(fs::remove_file(path));
    })
}

/// Copies each of SmileBASIC 4's projects found in `dirs` into
/// `projects`, as `projects/#TOOL/TSBLIB.PRG` and so on. A file already
/// there is never replaced, so changes made to it in the emulator stay.
pub fn import_projects(dirs: &[PathBuf], projects: &Path) -> Result<Projects, ImportError> {
    let mut done = Projects::default();
    for project in PROJECTS {
        let Some(source) = locate(project, dirs) else {
            done.missing.push(project);
            continue;
        };
        let target = projects.join(project);
        fs::create_dir_all(&target).map_err(|e| ImportError::Io(target.clone(), e))?;
        let mut written = 0;
        for (name, path) in files(&source)? {
            let label = format!("{project}/{name}");
            let mut chars = name.chars();
            let kind = chars.next().and_then(FileKind::from_prefix);
            let Some(kind) = kind.filter(|_| !chars.as_str().is_empty()) else {
                done.skipped.push((label, "not a file type".to_string()));
                continue;
            };
            let bytes = fs::read(&path).map_err(|e| ImportError::Io(path.clone(), e))?;
            let converted = match transfer::import_file(kind, &bytes) {
                Ok(converted) => converted,
                Err(why) => {
                    done.skipped.push((label, why));
                    continue;
                }
            };
            let dest = target.join(&name);
            // Not `exists()`: that follows links, so a dangling one would
            // read as missing and the write would go to its target.
            match fs::symlink_metadata(&dest) {
                Ok(meta) if meta.file_type().is_symlink() => {
                    done.skipped
                        .push((label, "a link is in its place".to_string()));
                    continue;
                }
                Ok(_) => {
                    match fs::read(&dest) {
                        Ok(old) if old == converted => done.unchanged += 1,
                        _ => done.kept.push(label),
                    }
                    continue;
                }
                Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                Err(e) => return Err(ImportError::Io(dest, e)),
            }
            write_new(&dest, &converted).map_err(|e| ImportError::Io(dest, e))?;
            written += 1;
        }
        done.copied.push((project, source, written));
    }
    Ok(done)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::temp_dir::TempDir;
    use sb_runtime::sbfile::{self, Date, Kind, usage};

    fn program(text: &str) -> Vec<u8> {
        let date = Date {
            year: 2020,
            month: 4,
            day: 23,
            hour: 0,
            minute: 0,
            second: 0,
        };
        sbfile::container(Kind::Text, usage::PROGRAM, date, text.as_bytes(), false)
    }

    fn put(dir: &Path, name: &str, bytes: &[u8]) {
        fs::create_dir_all(dir).unwrap();
        fs::write(dir.join(name), bytes).unwrap();
    }

    #[test]
    fn copies_the_projects_as_plain_files() {
        let tmp = TempDir::new("projects");
        let romfs = tmp.0.join("romfs");
        put(&romfs.join("basic/en/#TOOL"), "TA.PRG", &program("?1"));
        put(
            &romfs.join("basic/en/#TOOL"),
            "GB.PNG",
            b"\x89PNG\r\n\x1a\n",
        );
        put(&romfs.join("basic/jp/#EXAMPLE"), "TEX1TEXT", &program("?2"));
        // Folders inside a project are not files.
        put(&romfs.join("basic/en/#TOOL/SUB"), "TX", b"");
        put(&romfs.join("basic/en/#SYS"), "ZODD", b"");
        let out = tmp.0.join("projects");
        let done = import_projects(&[romfs], &out).unwrap();

        assert_eq!(fs::read(out.join("#TOOL/TA.PRG")).unwrap(), b"?1");
        // A file that is not in a container is copied unchanged.
        assert_eq!(
            fs::read(out.join("#TOOL/GB.PNG")).unwrap(),
            b"\x89PNG\r\n\x1a\n"
        );
        // Only the Japanese folder has #EXAMPLE here, so it comes from
        // there; the English folder without it is not used.
        assert_eq!(fs::read(out.join("#EXAMPLE/TEX1TEXT")).unwrap(), b"?2");
        let copied: Vec<_> = done.copied.iter().map(|c| (c.0, c.2)).collect();
        assert_eq!(copied, [("#SYS", 0), ("#TOOL", 2), ("#EXAMPLE", 1)]);
        assert_eq!(done.missing, ["#GUIDE"]);
        assert_eq!(
            done.skipped,
            [("#SYS/ZODD".to_string(), "not a file type".to_string())]
        );
    }

    #[test]
    fn english_and_later_directories_win() {
        let tmp = TempDir::new("projects-order");
        let (base, update) = (tmp.0.join("base"), tmp.0.join("update"));
        put(&base.join("basic/jp/#TOOL"), "TOLD.PRG", &program("old"));
        put(&update.join("basic/jp/#TOOL"), "TA.PRG", &program("jp"));
        put(&update.join("basic/en/#TOOL"), "TA.PRG", &program("en"));
        let out = tmp.0.join("projects");
        import_projects(&[base, update], &out).unwrap();
        assert_eq!(fs::read(out.join("#TOOL/TA.PRG")).unwrap(), b"en");
        // The update's folder is taken whole, not merged with the base
        // folder.
        assert!(!out.join("#TOOL/TOLD.PRG").exists());
    }

    #[test]
    fn keeps_files_already_there() {
        let tmp = TempDir::new("projects-keep");
        let romfs = tmp.0.join("romfs");
        put(&romfs.join("basic/en/#TOOL"), "TA.PRG", &program("A"));
        put(&romfs.join("basic/en/#TOOL"), "TB.PRG", &program("B"));
        put(&romfs.join("basic/en/#TOOL"), "TC.PRG", &program("C"));
        let out = tmp.0.join("projects");
        put(&out.join("#TOOL"), "TA.PRG", b"A");
        put(&out.join("#TOOL"), "TB.PRG", b"mine");
        let done = import_projects(&[romfs], &out).unwrap();
        assert_eq!(fs::read(out.join("#TOOL/TB.PRG")).unwrap(), b"mine");
        assert_eq!(fs::read(out.join("#TOOL/TC.PRG")).unwrap(), b"C");
        assert_eq!(done.unchanged, 1);
        assert_eq!(done.kept, ["#TOOL/TB.PRG"]);
        assert_eq!(done.copied[0].2, 1);
    }

    #[cfg(unix)]
    #[test]
    fn leaves_links_alone() {
        use std::os::unix::fs::symlink;
        let tmp = TempDir::new("projects-links");
        let romfs = tmp.0.join("romfs");
        put(&romfs.join("basic/en/#TOOL"), "TA.PRG", &program("A"));
        put(&romfs.join("basic/en/#TOOL"), "TB.PRG", &program("B"));
        let out = tmp.0.join("projects");
        fs::create_dir_all(out.join("#TOOL")).unwrap();
        // A dangling link, and one to a file outside the folder.
        let nowhere = tmp.0.join("nowhere.prg");
        symlink(&nowhere, out.join("#TOOL/TA.PRG")).unwrap();
        let outside = tmp.0.join("outside.prg");
        fs::write(&outside, b"mine").unwrap();
        symlink(&outside, out.join("#TOOL/TB.PRG")).unwrap();

        let done = import_projects(&[romfs], &out).unwrap();
        assert!(!nowhere.exists());
        assert_eq!(fs::read(&outside).unwrap(), b"mine");
        for name in ["TA.PRG", "TB.PRG"] {
            let meta = fs::symlink_metadata(out.join("#TOOL").join(name)).unwrap();
            assert!(meta.file_type().is_symlink());
        }
        let reason = "a link is in its place".to_string();
        assert_eq!(
            done.skipped,
            [
                ("#TOOL/TA.PRG".to_string(), reason.clone()),
                ("#TOOL/TB.PRG".to_string(), reason)
            ]
        );
        assert_eq!(done.copied[0].2, 0);
    }

    #[test]
    fn leaves_out_files_of_the_wrong_type() {
        let tmp = TempDir::new("projects-bad");
        let romfs = tmp.0.join("romfs");
        // A program named as a DAT file.
        put(&romfs.join("basic/en/#TOOL"), "BA.DAT", &program("A"));
        let out = tmp.0.join("projects");
        let done = import_projects(&[romfs], &out).unwrap();
        assert!(!out.join("#TOOL/BA.DAT").exists());
        assert_eq!(done.skipped.len(), 1);
        assert_eq!(done.skipped[0].0, "#TOOL/BA.DAT");
    }
}
