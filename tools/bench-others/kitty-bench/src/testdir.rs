//! A scratch directory for a unit test, removed when it drops.

use std::path::{Path, PathBuf};

pub struct TestDir(PathBuf);

impl TestDir {
    pub fn new(name: &str) -> TestDir {
        let p = std::env::temp_dir().join(format!("kitty-bench-{}-{name}", std::process::id()));
        // A leftover from an earlier process with this id is not an error.
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).expect("create a test directory");
        TestDir(p)
    }

    pub fn path(&self) -> &Path {
        &self.0
    }

    /// Write `bytes` at `rel`, creating its parents.
    pub fn write(&self, rel: &str, bytes: &[u8]) -> PathBuf {
        let p = self.0.join(rel);
        std::fs::create_dir_all(p.parent().expect("a parent")).expect("create parents");
        std::fs::write(&p, bytes).expect("write a test file");
        p
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
