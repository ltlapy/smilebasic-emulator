//! A scratch folder for the tests.

use std::fs;
use std::path::PathBuf;

/// A fresh folder under the system temp directory, removed on drop.
pub struct TempDir(pub PathBuf);

impl TempDir {
    /// `tag` must differ between tests, which run at the same time.
    pub fn new(tag: &str) -> TempDir {
        let dir = std::env::temp_dir().join(format!("sb-import-{tag}-{}", std::process::id()));
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
