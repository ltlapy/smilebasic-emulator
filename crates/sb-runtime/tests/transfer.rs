//! Exporting projects into SmileBASIC 4's files and importing them back.

use std::fs;
use std::path::{Path, PathBuf};

use sb_runtime::image::{self, Image};
use sb_runtime::sbfile::{self, Data, DataValues, FOOTER_SIZE, Kind, usage};
use sb_runtime::storage::{self, FileKind, Storage};
use sb_runtime::transfer::{self, Summary};

struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> TempDir {
        let dir = std::env::temp_dir().join(format!("sb-transfer-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        TempDir(dir)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Checks the footer and returns the container's contents.
fn signed(path: &Path) -> sbfile::File {
    let bytes = fs::read(path).unwrap();
    let (rest, footer) = bytes.split_at(bytes.len() - FOOTER_SIZE);
    assert_eq!(footer, sbfile::sign(rest), "{}", path.display());
    sbfile::parse(&bytes).unwrap()
}

fn picture() -> Image {
    Image {
        width: 3,
        height: 2,
        pixels: vec![
            0xFFFF0000, 0x8000FF00, 0, 0xFF0000FF, 0xFFFFFFFF, 0x01020304,
        ],
    }
}

#[test]
fn export_then_import() {
    let tmp = TempDir::new("both");
    let mut s = Storage::new(tmp.0.join("projects"));
    s.create_project("GAME").unwrap();
    s.set_project("GAME").unwrap();
    let program = "PRINT \"あ\"\n";
    s.write(FileKind::Txt, "MAIN.PRG", program.as_bytes(), None)
        .unwrap();
    let data = sbfile::data_body(&Data {
        dims: vec![2, 3],
        values: DataValues::Int(vec![1, 2, 3, 4, 5, 6]),
    });
    s.write(FileKind::Dat, "MAP", &data, None).unwrap();
    s.write(
        FileKind::Grp,
        "PIC",
        &image::encode_png(&picture()).unwrap(),
        None,
    )
    .unwrap();
    let jpeg = image::encode_jpeg(&picture(), 90).unwrap();
    s.write(FileKind::Jpg, "PHOTO", &jpeg, None).unwrap();
    s.write(FileKind::Grp, "BAD", b"not an image", None)
        .unwrap();
    s.write_project_file("@META.TXT", "My game\nIt is a game.".as_bytes())
        .unwrap();
    s.write_project_file("NOTES", b"stray").unwrap();

    let out = tmp.0.join("out");
    let summary = transfer::export_project(&s, "GAME", &out).unwrap();
    assert_eq!(summary.written, 4);
    let skipped: Vec<&str> = summary.skipped.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(skipped, ["GBAD", "NOTES"]);

    let game = out.join("GAME");
    let f = signed(&game.join("TMAIN.PRG"));
    assert_eq!(
        (f.kind, f.usage, f.text().unwrap()),
        (Kind::Text, usage::PROGRAM, program)
    );
    let f = signed(&game.join("BMAP"));
    assert_eq!((f.kind, f.usage, &f.body), (Kind::Data, usage::DAT, &data));
    let f = signed(&game.join("GPIC"));
    assert_eq!((f.kind, f.usage), (Kind::Grp, usage::GRP));
    assert_eq!(f.data().unwrap().dims, [3, 2], "width first");
    let f = signed(&game.join("JPHOTO"));
    assert_eq!((f.kind, f.usage, &f.body), (Kind::Jpg, usage::JPG, &jpeg));
    let meta = sbfile::parse(&fs::read(game.join("META")).unwrap()).unwrap();
    let meta = sbfile::parse_meta(&meta.body).unwrap();
    assert_eq!(String::from_utf16(&meta.title).unwrap(), "My game");
    assert_eq!(
        String::from_utf16(&meta.description).unwrap(),
        "It is a game."
    );

    assert_eq!(transfer::import_name(&game), "GAME");
    s.create_project("COPY").unwrap();
    let summary = transfer::import_project(&s, &game, "COPY").unwrap();
    assert_eq!(
        summary,
        Summary {
            written: 4,
            skipped: Vec::new()
        }
    );
    s.set_project("COPY").unwrap();
    assert_eq!(
        s.read(FileKind::Txt, "MAIN.PRG").unwrap(),
        program.as_bytes()
    );
    assert_eq!(s.read(FileKind::Dat, "MAP").unwrap(), data);
    let pic = image::decode(&s.read(FileKind::Grp, "PIC").unwrap()).unwrap();
    assert_eq!(pic, picture());
    assert!(
        s.read(FileKind::Grp, "PIC")
            .unwrap()
            .starts_with(b"\x89PNG")
    );
    assert_eq!(s.read(FileKind::Jpg, "PHOTO").unwrap(), jpeg);
    assert_eq!(
        s.read_project_file("@META.TXT").unwrap(),
        "My game\nIt is a game.".as_bytes()
    );
    assert!(
        s.read_project_file("META").is_err(),
        "kept as our own files"
    );
}

#[test]
fn the_workspace_and_wrong_files() {
    let tmp = TempDir::new("workspace");
    let s = Storage::new(tmp.0.join("projects"));
    let out = tmp.0.join("out");
    let summary = transfer::export_project(&s, "", &out).unwrap();
    assert_eq!(summary.written, 0);
    let meta = sbfile::parse(&fs::read(out.join("###/META")).unwrap()).unwrap();
    let meta = sbfile::parse_meta(&meta.body).unwrap();
    assert_eq!(String::from_utf16(&meta.title).unwrap(), "[WORKSPACE]");
    assert_eq!(transfer::import_name(&out.join("###")), "WORKSPACE");
    assert_eq!(transfer::import_name(Path::new("my game")), "MY_GAME");

    // A DAT file whose container holds text is refused.
    let text = sbfile::container(Kind::Text, 2, sbfile::Date::now(), b"X", false);
    assert!(transfer::import_file(FileKind::Dat, &text).is_err());
    assert_eq!(
        transfer::import_file(FileKind::Txt, &text).unwrap(),
        b"X",
        "and taken as text"
    );
    let summary = Summary {
        written: 1,
        skipped: vec![("GBAD".to_string(), "why".to_string())],
    };
    assert_eq!(
        summary.line("Exported A"),
        "Exported A: 1 file; 1 left out (GBAD: why)."
    );
}

/// A file reading as `size` bytes, taking no room.
fn sparse(path: &Path, size: u64) {
    fs::File::create(path).unwrap().set_len(size).unwrap();
}

#[test]
fn files_too_large_stay_behind_and_the_others_go() {
    let tmp = TempDir::new("large");
    let s = Storage::new(tmp.0.join("projects"));
    s.create_project("GAME").unwrap();
    let source = tmp.0.join("source");
    fs::create_dir_all(&source).unwrap();
    sparse(&source.join("TA"), storage::MAX_FILE + 1);
    fs::write(source.join("TB"), "?1").unwrap();
    let summary = transfer::import_project(&s, &source, "GAME").unwrap();
    assert_eq!(summary.written, 1);
    assert_eq!(summary.skipped.len(), 1);
    assert!(summary.skipped[0].1.contains("too large"));
    assert!(s.exists_in("GAME", FileKind::Txt, "B"));
    assert!(!s.exists_in("GAME", FileKind::Txt, "A"));

    sparse(&tmp.0.join("projects/GAME/TC"), storage::MAX_FILE + 1);
    let summary = transfer::export_project(&s, "GAME", &tmp.0.join("dest")).unwrap();
    assert_eq!(summary.written, 1);
    assert_eq!(summary.skipped.len(), 1);
    assert!(summary.skipped[0].1.contains("too large"));
    assert!(tmp.0.join("dest/GAME/TB").exists());
    assert!(!tmp.0.join("dest/GAME/TC").exists());
}

#[cfg(unix)]
mod links {
    use super::*;
    use std::os::unix::fs::symlink;

    #[test]
    fn importing_writes_through_no_link_in_the_project() {
        let tmp = TempDir::new("link-import");
        let s = Storage::new(tmp.0.join("projects"));
        s.create_project("GAME").unwrap();
        let outside = tmp.0.join("outside");
        fs::write(&outside, "outside").unwrap();
        symlink(&outside, tmp.0.join("projects/GAME/TMAIN.PRG")).unwrap();
        let source = tmp.0.join("source");
        fs::create_dir_all(&source).unwrap();
        fs::write(source.join("TMAIN.PRG"), "?1").unwrap();
        assert!(transfer::import_project(&s, &source, "GAME").is_err());
        assert_eq!(fs::read(&outside).unwrap(), b"outside");
    }

    #[test]
    fn importing_reads_no_link_in_the_source() {
        let tmp = TempDir::new("link-source");
        let s = Storage::new(tmp.0.join("projects"));
        s.create_project("GAME").unwrap();
        let outside = tmp.0.join("outside");
        fs::write(&outside, "secret").unwrap();
        let source = tmp.0.join("source");
        fs::create_dir_all(&source).unwrap();
        symlink(&outside, source.join("TLEAK.PRG")).unwrap();
        let summary = transfer::import_project(&s, &source, "GAME").unwrap();
        assert_eq!(summary.written, 0);
        assert!(!s.exists_in("GAME", FileKind::Txt, "LEAK.PRG"));
    }

    #[test]
    fn exporting_carries_no_linked_file_out() {
        let tmp = TempDir::new("link-export");
        let s = Storage::new(tmp.0.join("projects"));
        s.create_project("GAME").unwrap();
        let outside = tmp.0.join("outside");
        fs::write(&outside, "secret title\n").unwrap();
        symlink(&outside, tmp.0.join("projects/GAME/TLEAK.PRG")).unwrap();
        symlink(&outside, tmp.0.join("projects/GAME/@META.TXT")).unwrap();
        let dest = tmp.0.join("dest");
        let summary = transfer::export_project(&s, "GAME", &dest).unwrap();
        assert_eq!(summary.written, 0);
        assert!(!dest.join("GAME/TLEAK.PRG").exists());
        let meta = sbfile::parse(&fs::read(dest.join("GAME/META")).unwrap()).unwrap();
        let meta = sbfile::parse_meta(&meta.body).unwrap();
        assert_eq!(String::from_utf16_lossy(&meta.title), "GAME");
    }

    #[test]
    fn a_linked_project_is_not_exported() {
        let tmp = TempDir::new("link-export-project");
        let s = Storage::new(tmp.0.join("projects"));
        s.create_project("GAME").unwrap();
        let away = tmp.0.join("away");
        fs::create_dir_all(&away).unwrap();
        fs::write(away.join("TMAIN.PRG"), "secret").unwrap();
        symlink(&away, tmp.0.join("projects/LINK")).unwrap();
        assert!(transfer::export_project(&s, "LINK", &tmp.0.join("dest")).is_err());
        assert!(!tmp.0.join("dest/LINK/TMAIN.PRG").exists());
    }
}
