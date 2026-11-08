//! Record a login shell's start through a pty, with each read's time: the
//! `record-shell-start` example.
//!
//! The shell is started much as baud starts it: `$SHELL` as a login shell
//! (argv[0] `-zsh` for zsh) in the home directory, on a 120 by 36 pty, with
//! `TERM` xterm-256color, `COLORTERM` truecolor and `TERM_PROGRAM` baud. The
//! rest of the caller's environment is kept, including the outer terminal's
//! own variables, which baud's spawn removes (`OUTER` in
//! `baud-app/src/pty.rs`). It is read for the given seconds and then sent
//! `exit`.
//!
//! ```text
//! record-shell-start <out> [seconds]
//! ```
//!
//! `<out>` holds each read as a little-endian f64 of seconds since the fork,
//! a u32 length and the bytes: what
//! `cargo bench -p baud-bench --bench splash_shell -- <out>` replays under
//! the launch logo. Each read's time and length is printed; the bytes are
//! not, and the file should stay on the machine it was made on, because a
//! shell's output names the machine and the user.

use std::ffi::{OsStr, OsString};
use std::os::unix::ffi::{OsStrExt, OsStringExt};

use super::pty::{Child, Environ, Lookup, Spawn, clock};
use super::py;

const DOC: &str = "Record a login shell's start through a pty, with each read's time.

Usage: record-shell-start <out> [seconds]

<out> holds each read as a little-endian f64 of seconds since the fork, a
u32 length and the bytes: what `cargo bench -p baud-bench --bench
splash_shell -- <out>` replays under the launch logo.";

/// The file's bytes: each read as its time, its length and itself.
pub fn file_bytes(reads: &[(f64, Vec<u8>)]) -> Vec<u8> {
    let mut out = Vec::new();
    for (at, b) in reads {
        out.extend_from_slice(&at.to_le_bytes());
        let len = u32::try_from(b.len()).expect("one read fits a u32");
        out.extend_from_slice(&len.to_le_bytes());
        out.extend_from_slice(b);
    }
    out
}

/// What a login shell is called: its last path word after a dash.
pub fn login_name(shell: &OsStr) -> OsString {
    let bytes = shell.as_bytes();
    let base = bytes.rsplit(|b| *b == b'/').next().unwrap_or(bytes);
    OsString::from_vec([b"-", base].concat())
}

/// The example.
pub fn run(args: &[OsString]) -> i32 {
    let Some(out) = args.first() else {
        return super::died(DOC);
    };
    let seconds = match args.get(1) {
        None => 3.0,
        Some(s) => match py::float(&s.to_string_lossy()) {
            Some(v) => v,
            None => {
                return super::died(&format!(
                    "could not convert string to float: {}",
                    py::str_repr(&s.to_string_lossy())
                ));
            }
        },
    };
    match record(out, seconds) {
        Ok(()) => 0,
        Err(e) => super::died(&e),
    }
}

/// Everything past the arguments.
fn record(out: &OsString, seconds: f64) -> Result<(), String> {
    let mut env = Environ::current();
    let shell = env
        .get("SHELL")
        .map_or_else(|| OsString::from("/bin/zsh"), OsStr::to_os_string);
    let home = env
        .get("HOME")
        .map_or_else(|| OsString::from("/"), OsStr::to_os_string);
    env.set("TERM", "xterm-256color");
    env.set("COLORTERM", "truecolor");
    env.set("TERM_PROGRAM", "baud");
    let login = login_name(&shell);
    let start = clock();
    let spawn = Spawn::new(
        Lookup::Direct,
        std::slice::from_ref(&shell),
        Some(&login),
        &env,
        1,
    )?
    .in_dir(&home)?;
    let child = Child::fork(&spawn)?;
    child.set_size(120, 36, 0, 0)?;

    let mut reads: Vec<(f64, Vec<u8>)> = Vec::new();
    let end = start + seconds;
    let mut gone = false;
    loop {
        let left = end - clock();
        if left.is_nan() || left <= 0.0 {
            break;
        }
        if !child.readable(left)? {
            continue;
        }
        match child.read(4096) {
            Ok(b) if !b.is_empty() => reads.push((clock() - start, b)),
            _ => {
                gone = true;
                break;
            }
        }
    }
    // **A shell that ended before it was sent `exit` fails the recording**:
    // a `SHELL` whose exec or `chdir` was refused said why on the terminal
    // and exited with the spawn's code. Its reads end before the time,
    // and only once the shell's side of the pty is gone: on Linux after
    // `readable` saw it exit and let go of the slave, on Darwin when its
    // exit hung the terminal up, which comes before its parent can see the
    // exit. So the wait is for a shell that is ending, and returns. One that
    // hung its terminal up and goes on running is given 2 s, as the shell is
    // after `exit`, and then killed. The write below is no test of it: Darwin
    // refuses a write to a pty no process holds, and Linux takes it into the
    // buffer and answers success.
    if gone || child.exited() {
        if !child.exited_within(2.0) {
            child.close();
            match child.kill(libc::SIGKILL) {
                Err(e) if e.raw_os_error() != Some(libc::ESRCH) => {
                    return Err(format!("kill: {e}"));
                }
                _ => {}
            }
            child.wait()?;
            return Err("start: the shell left its terminal without exiting".to_string());
        }
        let code = child.wait()?;
        return Err(format!(
            "start: the shell ended before it was sent exit, with {code}"
        ));
    }
    // The shell's last output is read and dropped: a shell exits only once
    // the tty has drained what it wrote, and closing the pty first ends it.
    child.write(b"exit\r")?;
    let end = clock() + 2.0;
    loop {
        let left = end - clock();
        if left <= 0.0 {
            break;
        }
        if child.readable(left)? {
            match child.read(4096) {
                Ok(b) if !b.is_empty() => {}
                _ => break,
            }
        }
    }
    child.close();
    match child.kill(libc::SIGKILL) {
        Err(e) if e.raw_os_error() != Some(libc::ESRCH) => return Err(format!("kill: {e}")),
        _ => {}
    }
    child.wait()?;
    std::fs::write(out, file_bytes(&reads)).map_err(|e| format!("open: {e}"))?;
    for (at, b) in &reads {
        println!("{:9.1} ms {:6} bytes", at * 1e3, b.len());
    }
    Ok(())
}
