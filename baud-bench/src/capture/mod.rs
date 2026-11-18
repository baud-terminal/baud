//! The tools that capture a program's bytes through a pty, and the three
//! that write what such a capture is run over. Each is an example, a thin
//! `main` over a module here, so `baud` stays the only binary and a bare
//! `cargo bench` launches none of them:
//!
//! | example | module | what it writes |
//! |---|---|---|
//! | `capture-tui` | [`tui`] | a program's bytes, and with `--steps` each read's length and each key |
//! | `capture-resize` | [`resize`] | a shell's bytes across window sizes, and the size each arrived at |
//! | `capture-yazi-walk` | [`yazi_walk`] | yazi walking its pictures, as a terminal with Sixel sees it |
//! | `record-kitten-icat` | [`kitten`] | what `kitten icat` sends, by input, window and mode |
//! | `record-shell-start` | [`shell_start`] | a login shell's start, each read with its time |
//! | `gen-marks-payload` | [`marks`] | scrolling text under combining marks |
//! | `gen-vim-unicode` | [`vim_unicode`] | the file `tests/recordings/utf8/vim-unicode.raw` pages |
//! | `gen-yazi-files` | [`yazi_files`] | the seven files `tests/recordings/sixel/yazi-walk.raw` walks |
//!
//! **A committed recording is evidence and none of these rewrites one.** A
//! capture is not reproducible -- btop draws live figures -- so a run goes to
//! a scratch path, and `tests/recordings/README.md` says what each recording
//! was made with.
//!
//! The tools keep the flags, the output and the exit codes of the scripts
//! they replaced; `MEASUREMENTS.md`, "The capture tools are examples", lists
//! what differs.

pub mod answers;
pub mod args;
pub mod deflate;
pub mod draws;
pub mod kitten;
pub mod marks;
pub mod pty;
pub mod py;
pub mod resize;
pub mod shell_start;
pub mod tui;
pub mod vim_unicode;
pub mod yazi_files;
pub mod yazi_walk;

use std::ffi::{OsStr, OsString};
use std::io::Write;

/// This process's arguments, past its name.
pub fn cli() -> Vec<OsString> {
    std::env::args_os().skip(1).collect()
}

/// A line on stdout, a path's bytes as they are.
pub fn say(line: &[u8]) {
    let mut out = std::io::stdout().lock();
    // Nothing to do about a closed stdout but finish the work.
    let _ = out.write_all(line);
    let _ = out.write_all(b"\n");
}

/// The bytes of a path or an argument.
pub fn raw(s: &OsStr) -> &[u8] {
    std::os::unix::ffi::OsStrExt::as_bytes(s)
}

/// What a script's uncaught exception came to: one line on stderr, exit 1.
pub fn died(what: &str) -> i32 {
    eprintln!("{what}");
    1
}

/// `prefix` and then `suffix`, for the two paths `capture-resize` writes.
pub fn with_suffix(prefix: &OsStr, suffix: &str) -> OsString {
    let mut p = prefix.to_os_string();
    p.push(suffix);
    p
}
