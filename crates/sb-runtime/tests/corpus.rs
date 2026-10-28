//! Reads every SmileBASIC 4 file under `$SB_CORPUS_DIR` (for example a
//! directory of files extracted from your own copy) and compiles the
//! programs among them. Skipped when the variable is unset.
//!
//! Run with `SB_CORPUS_DIR=/path cargo test -p sb-runtime --test corpus -- --nocapture`.

use std::path::{Path, PathBuf};

use sb_core::bytecode::Want;
use sb_core::error::Result;
use sb_core::host::Host;
use sb_core::value::Value;
use sb_runtime::sbfile::{self, Kind};

/// Resolves every command name, so that only language errors are reported
/// while the host commands are still missing.
struct AnyHost;

impl Host for AnyHost {
    fn lookup(&self, _name: &str) -> Option<u32> {
        Some(0)
    }
    fn call(&mut self, _id: u32, _args: &[Value], _want: Want) -> Result<Vec<Value>> {
        unreachable!("corpus programs are compiled, not run")
    }
    fn print(&mut self, _screen: Option<i32>, _text: &[u16]) -> Result<()> {
        Ok(())
    }
    fn print_tab(&mut self, _screen: Option<i32>) -> Result<()> {
        Ok(())
    }
    fn print_newline(&mut self, _screen: Option<i32>) -> Result<()> {
        Ok(())
    }
    fn input_start(&mut self) -> Result<()> {
        Ok(())
    }
    fn input_poll(&mut self) -> Result<Option<Vec<u16>>> {
        Ok(None)
    }
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk(&path, out);
        } else {
            out.push(path);
        }
    }
}

/// Text files that hold programs rather than data: `T` prefix (text) and
/// not a `.TXT` document.
fn is_program(path: &Path) -> bool {
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    name.starts_with('T') && !name.to_ascii_uppercase().ends_with(".TXT")
}

#[test]
fn corpus() {
    let Some(root) = std::env::var_os("SB_CORPUS_DIR") else {
        eprintln!("SB_CORPUS_DIR not set; skipping");
        return;
    };
    let mut files = Vec::new();
    walk(Path::new(&root), &mut files);
    files.sort();
    let (mut containers, mut programs, mut failures) = (0, 0, Vec::new());
    for path in &files {
        let bytes = std::fs::read(path).unwrap();
        if bytes.get(0..2) != Some(&[4, 0]) {
            continue; // PNG and other plain files
        }
        let file = match sbfile::parse(&bytes) {
            Ok(f) => f,
            Err(e) => {
                failures.push(format!("{}: {e}", path.display()));
                continue;
            }
        };
        containers += 1;
        match file.kind {
            Kind::Data | Kind::Grp => {
                if let Err(e) = file.data() {
                    failures.push(format!("{}: {e}", path.display()));
                }
            }
            Kind::Text if is_program(path) => {
                programs += 1;
                let result = file.text().map_err(|e| e.to_string()).and_then(|src| {
                    sb_core::compile(src, &AnyHost)
                        .map(|_| ())
                        .map_err(|e| e.to_string())
                });
                if let Err(e) = result {
                    failures.push(format!("{}: {e}", path.display()));
                }
            }
            Kind::Text | Kind::Meta | Kind::Jpg => {}
        }
    }
    eprintln!(
        "{containers} files read, {programs} programs compiled, {} failures",
        failures.len()
    );
    for f in &failures {
        eprintln!("  {f}");
    }
    assert!(failures.is_empty());
}
