//! **When a link's window stays open after its program ends.** A window a
//! link's `action = "window"` opened runs a program in place of a shell,
//! and closes when it ends, as a shell window does. A program that fails at
//! once -- a missing file, a bad argument, a connection refused -- would
//! make its window flash and vanish with whatever it said, so one that
//! exits other than with status 0 within [`HOLD_FOR`] of its start keeps
//! its window, with one line saying how it ended, until a key closes it;
//! so does one a signal ended, other than an interrupt or a hangup, the
//! two a user sends to end a program on purpose. A shell window never
//! holds. Pure over how the child ended and how long it lived, so its
//! tests run on every platform.
#![forbid(unsafe_code)]
// The Linux driver holds no window yet; its tests run there all the same.
#![cfg_attr(not(any(target_os = "macos", test)), allow(dead_code))]

use std::time::Duration;

/// **How long a program may live and still hold its window when it exits
/// with a failure**: long enough for one that fails at its start, short
/// enough that a program the user used and quit with an error closes as
/// it would elsewhere.
pub const HOLD_FOR: Duration = Duration::from_secs(2);

/// `SIGHUP`, the same number on every platform baud runs on (POSIX.1's
/// table of signals, and the `kill -s` numbers it fixes).
const SIGHUP: i32 = 1;
/// `SIGINT`, as [`SIGHUP`].
const SIGINT: i32 = 2;

/// **How a child ended**, from the status `waitpid` gave: its exit code,
/// or the signal that ended it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Ended {
    Code(i32),
    Signal(i32),
}

impl Ended {
    /// **`s` as `waitpid` reported it**, read through the standard
    /// library's `ExitStatus`, which applies the platform's own
    /// `WIFEXITED`, `WEXITSTATUS` and `WTERMSIG`: POSIX names the macros and
    /// leaves the layout to each system. A status that is neither, a
    /// stopped child's, reads as the signal that stopped it.
    pub fn of(s: std::process::ExitStatus) -> Ended {
        use std::os::unix::process::ExitStatusExt;
        match (s.code(), s.signal(), s.stopped_signal()) {
            (Some(code), _, _) => Ended::Code(code),
            (None, Some(signal), _) | (None, None, Some(signal)) => Ended::Signal(signal),
            (None, None, None) => Ended::Signal(0),
        }
    }
}

/// What a window does once its child has ended.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum AfterExit {
    /// It closes, as a shell window does.
    Close,
    /// It stays, this line written below what the program printed, until
    /// a key closes it; the key goes to no program.
    Hold(String),
}

/// **What a window does once its child has ended**: `program` is the
/// program a link's window ran, none for a shell window; `ended` how it
/// ended, and `lived` how long after its start.
/// - Status 0, `SIGINT` or `SIGHUP`, at any time: it closes.
/// - Any other status: it holds within [`HOLD_FOR`], and closes after.
/// - Any other signal, at any time: it holds, since the program did not
///   end by itself and a crash says so only here.
pub fn after_exit(program: Option<&std::ffi::OsStr>, ended: Ended, lived: Duration) -> AfterExit {
    let Some(program) = program else {
        return AfterExit::Close;
    };
    let holds = match ended {
        Ended::Code(0) | Ended::Signal(SIGINT | SIGHUP) => false,
        Ended::Code(_) => lived < HOLD_FOR,
        Ended::Signal(_) => true,
    };
    if !holds {
        return AfterExit::Close;
    }
    let name = std::path::Path::new(program)
        .file_name()
        .unwrap_or(program)
        .to_string_lossy();
    AfterExit::Hold(match ended {
        Ended::Code(c) => format!("{name} exited {c}; any key closes"),
        Ended::Signal(s) => format!("{name} ended on signal {s}; any key closes"),
    })
}

/// **The bytes that write a held window's line** below what the program
/// printed: DECSTR first, the soft reset `CSI ! p`, which puts the
/// rendition, the margins, origin and insert mode and the character sets
/// back to their defaults and leaves the screen's text where it is
/// (`[[source: vt510-decstr]]`), so a program that ended in reverse video
/// or inside a scroll region cannot hide the line; then a new line, and
/// the line.
#[cfg_attr(not(any(target_os = "macos", test)), allow(dead_code))]
pub fn written(line: &str) -> Vec<u8> {
    let mut b = b"\x1b[!p\r\n".to_vec();
    b.extend_from_slice(line.as_bytes());
    b
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsStr;

    const SOON: Duration = Duration::from_millis(300);
    const LATE: Duration = Duration::from_secs(60);
    fn hx() -> Option<&'static OsStr> {
        Some(OsStr::new("/usr/bin/hx"))
    }
    /// `SIGSEGV`: 11 on Darwin and on Linux, the two this test runs on.
    const SIGSEGV: i32 = 11;

    /// **Status 0 closes the window, however soon.** Named break: hold on
    /// 0 -- a program that did its job leaves its window open.
    #[test]
    fn a_program_that_exits_0_closes_its_window() {
        assert_eq!(after_exit(hx(), Ended::Code(0), SOON), AfterExit::Close);
        assert_eq!(after_exit(hx(), Ended::Code(0), LATE), AfterExit::Close);
    }

    /// **A failure within [`HOLD_FOR`] holds the window with one line**,
    /// naming the program by its file name; a shell window never holds.
    /// Named break: close on every exit -- a missing file's error flashes
    /// and is gone.
    #[test]
    fn a_program_that_fails_within_2s_holds_its_window() {
        assert_eq!(
            after_exit(hx(), Ended::Code(1), SOON),
            AfterExit::Hold("hx exited 1; any key closes".into())
        );
        assert_eq!(
            after_exit(None, Ended::Code(1), SOON),
            AfterExit::Close,
            "a shell window"
        );
    }

    /// **A failure after [`HOLD_FOR`] closes the window**, as a program the
    /// user used and quit closes elsewhere. Named break: no bound -- every
    /// failure holds, however long the program ran.
    #[test]
    fn a_program_that_fails_after_2s_closes_its_window() {
        assert_eq!(after_exit(hx(), Ended::Code(1), HOLD_FOR), AfterExit::Close);
        assert_eq!(after_exit(hx(), Ended::Code(1), LATE), AfterExit::Close);
    }

    /// **An interrupt or a hangup closes the window, at any time**: the
    /// user ended the program, or closed what it ran in. Named break: hold
    /// on `SIGINT` -- a Control-C within 2 s leaves the window open.
    #[test]
    fn a_program_interrupted_or_hung_up_closes_its_window() {
        for s in [SIGINT, SIGHUP] {
            assert_eq!(
                after_exit(hx(), Ended::Signal(s), SOON),
                AfterExit::Close,
                "{s}"
            );
            assert_eq!(
                after_exit(hx(), Ended::Signal(s), LATE),
                AfterExit::Close,
                "{s}"
            );
        }
    }

    /// **Any other signal holds the window, at any time**, naming the
    /// signal: a crash says so only here. Named break: close on a signal
    /// after [`HOLD_FOR`] -- a crash after a minute vanishes with its
    /// window.
    #[test]
    fn a_program_a_signal_ended_holds_its_window() {
        for lived in [SOON, LATE] {
            assert_eq!(
                after_exit(Some(OsStr::new("w3m")), Ended::Signal(SIGSEGV), lived),
                AfterExit::Hold("w3m ended on signal 11; any key closes".into())
            );
        }
    }

    /// **The status is decoded as this platform's `waitpid` gives it**,
    /// against real children's: `exit 3` and a `kill -9` of itself; and the
    /// two signal numbers the rule names are this platform's. Named break:
    /// take every status as an exit code -- the signal reads as a code.
    #[test]
    fn a_status_is_decoded_as_waitpid_gives_it() {
        let run = |script: &str| {
            std::process::Command::new("/bin/sh")
                .args(["-c", script])
                .status()
                .expect("sh")
        };
        assert_eq!(Ended::of(run("exit 3")), Ended::Code(3));
        assert_eq!(Ended::of(run("exit 0")), Ended::Code(0));
        assert_eq!(Ended::of(run("kill -9 $$")), Ended::Signal(9));
        assert_eq!(Ended::of(run("kill -HUP $$")), Ended::Signal(SIGHUP));
        assert_eq!(Ended::of(run("kill -INT $$")), Ended::Signal(SIGINT));
        assert_eq!(Ended::of(run("kill -SEGV $$")), Ended::Signal(SIGSEGV));
    }

    /// **A held window's line shows below what the program printed, in the
    /// default rendition, whatever state the program left**: one that ended
    /// in reverse video, in insert mode and inside a scroll region with the
    /// cursor at its foot. The program's text stays where it is, the line is
    /// on the row after it, and every cell of the line has the default
    /// rendition.
    /// Named break: write the line with no soft reset -- the region scrolls
    /// the text up, and the line comes out reversed.
    #[test]
    fn a_held_windows_line_shows_whatever_the_program_left() {
        use baud_core::{BytePos, Owned, ReadLen, Term};
        use baud_domain::{Row, StyleId, WinSize};
        let ws = WinSize {
            ws_col: 40,
            ws_row: 6,
        };
        let mut store = Owned::new(&ws);
        let mut t = Term::new(store.storage(), &ws);
        let feed = |t: &mut Term<'_>, b: &[u8]| {
            t.write_at(BytePos::ZERO, b);
            t.feed(BytePos::ZERO, ReadLen::from_index(b.len()));
        };
        // A scroll region of rows 2-3, the program's last words on its foot,
        // and reverse video and insert mode left on.
        feed(&mut t, b"\x1b[2;3r\x1b[3;1Hhx: no such file\x1b[7m\x1b[4h");
        let line = "hx exited 1; any key closes";
        feed(&mut t, &written(line));
        let rows = crate::programs::screen(&t);
        assert_eq!(rows[2], "hx: no such file", "the text stays: {rows:?}");
        assert_eq!(rows[3], line, "on the row after it: {rows:?}");
        let cells = t.window().row(Row::from_index(3)).to_vec();
        assert!(
            cells[..line.len()]
                .iter()
                .all(|c| c.style == StyleId::DEFAULT),
            "the default rendition"
        );
    }
}
