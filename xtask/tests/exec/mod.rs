//! **A file a test runs is written by another process.** A file this
//! process writes is open for writing in every child another test thread
//! forks while the write is open, until that child's own exec closes it
//! (`O_CLOEXEC` closes a descriptor at exec, not at fork), and Linux
//! refuses to run a file open for writing: the exec is ETXTBSY. `sh`
//! writes the file here and `cp` copies it, so no descriptor of it is ever
//! open in this process. A module directory, so cargo does not build it as
//! a test target of its own.

#![allow(dead_code)]

use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

/// `body` at `path`, mode 755, written by `sh`.
pub fn script(path: &Path, body: &str) {
    let mut sh = Command::new("/bin/sh")
        .args(["-c", "cat > \"$1\" && chmod 755 \"$1\"", "sh"])
        .arg(path)
        .stdin(Stdio::piped())
        .spawn()
        .expect("sh");
    sh.stdin
        .take()
        .expect("its input")
        .write_all(body.as_bytes())
        .expect("the script");
    let status = sh.wait().expect("sh");
    assert!(status.success(), "{}: {status}", path.display());
}

/// `from` copied to `to` by `cp`, which keeps a runnable file runnable:
/// `/bin/cp` where there is one, else the `cp` this process's `PATH`
/// finds, on a system that keeps no `/bin/cp`.
pub fn copy(from: &Path, to: &Path) {
    let cp = if Path::new("/bin/cp").is_file() {
        "/bin/cp"
    } else {
        "cp"
    };
    let status = Command::new(cp).arg(from).arg(to).status().expect("cp");
    assert!(
        status.success(),
        "{} to {}: {status}",
        from.display(),
        to.display()
    );
}
