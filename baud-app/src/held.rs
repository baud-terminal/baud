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
//!
//! **A window decides once it has both ends of its child** ([`decide`]):
//! the stream closed, which the pty's read says, and the status the child
//! was reaped with, which its watch says (macOS's `fg::Watch`, Linux's
//! pidfd in the loop's poll). Either can come first -- a child that closed
//! its terminal and went on, or one whose terminal a child of its own
//! still holds -- and nothing waits for the second: the window asks again
//! on the turn that brings it.
//!
//! **The line is drawn in baud's message box** ([`look`]), the pair every
//! message of baud's is drawn in, on the row below the blank one under what
//! the program printed; nothing of it reaches a cell, so a copy of the
//! window's text is the program's alone. The choice is this file's two
//! functions: [`written`] moves the cursor to the line's row, and [`look`]
//! draws the box there. Writing the line into the cells instead is
//! [`written`] carrying the text and [`look`] giving none.
//!
//! **`[message.held]` changes both** ([`crate::config::MessageHeld`]): its
//! `look` draws the line as the program's own text instead, in the window's
//! default colours, as text written after a soft reset (`DECSTR`) is, and
//! from the left edge, still never in a cell; its `place` puts it on the
//! window's bottom line, where `[message.copy] place = "bar"` puts a copy's
//! words -- and, as a copy's words do, it gives that line to a link's
//! address while the preview shows one, since the tip slot is drawn over
//! the preview's bar and would cover where the link goes; and to a copy's
//! words said there, until they go ([`with_line`]).
#![forbid(unsafe_code)]

use baud_domain::{Cols, Row, Rows};
use std::time::{Duration, Instant};

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

/// **How a window's child ended, as the window learned it.** Only an
/// [`Ending::Exited`] carries a status; each of the others closes the window
/// as a shell's does.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Ending {
    /// `waitpid` answered with the child and the status it ended with.
    Exited(std::process::ExitStatus),
    /// `waitpid` answered `ECHILD`: the pid is no child of this process to
    /// wait for, one something else reaped.
    NotOurs,
    /// `waitpid` failed with anything else. For a pid above 0 with
    /// `WNOHANG` POSIX leaves only `EINVAL`, an options value it does not
    /// take: a fault in baud, said where it is read.
    WaitFailed(crate::sys::Errno),
    /// The window stopped waiting for a child still running: with no pidfd,
    /// the hung-up master's child was looked for twice and was still there
    /// (`pty::Pty::give_up`).
    #[cfg(target_os = "linux")]
    Abandoned,
}

/// **A window's child's end**, and when the window learned it. Written
/// into a window's `pty::Slot` only by the reap path in `pty.rs`.
#[derive(Clone, Copy, Debug)]
pub struct Reaped {
    pub ending: Ending,
    pub at: Instant,
}

/// What a window does once its child has ended.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum AfterExit {
    /// It closes, as a shell window does.
    Close,
    /// It stays, this line shown below what the program printed, until a
    /// key closes it; the key goes to no program.
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
        Ended::Code(c) => format!("{name} stopped with error {c}. Press any key to close."),
        Ended::Signal(s) => format!("{name} crashed (signal {s}). Press any key to close."),
    })
}

/// **Whether a window is held open** after its program ended, and the line
/// it shows until a key closes it. Sticky: once held, a turn asks nothing
/// more of the child.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub enum Holding {
    #[default]
    No,
    Yes(String),
}

/// Whether the pty's stream has closed: the read that says the child has
/// let go of its terminal.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Stream {
    /// macOS's driver asks only once its drain says the stream closed.
    #[cfg_attr(not(any(target_os = "linux", test)), allow(dead_code))]
    Open,
    Closed,
}

/// What a window's turn does about its child's end.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Decided {
    /// One of the two ends is still to come: nothing yet.
    Wait,
    Close,
    Hold(String),
}

/// The line a window says when it closes on [`Ending::WaitFailed`]: the
/// errno, which no window should ever see.
pub fn wait_failed(e: crate::sys::Errno) -> String {
    format!("held: waitpid failed ({e}); the window closes without its child's status")
}

/// **What a window does about its child's end, once both ends are in**:
/// the stream closed, and the child reaped (`reaped`), in either order;
/// until then it waits. `born` is when the child was started, for
/// [`after_exit`]'s bound, and `program` as there.
pub fn decide(
    program: Option<&std::ffi::OsStr>,
    born: Instant,
    stream: Stream,
    reaped: Option<&Reaped>,
) -> Decided {
    let (Stream::Closed, Some(r)) = (stream, reaped) else {
        return Decided::Wait;
    };
    let status = match r.ending {
        Ending::Exited(status) => status,
        Ending::NotOurs => return Decided::Close,
        Ending::WaitFailed(e) => {
            crate::warn!("{}", wait_failed(e));
            return Decided::Close;
        }
        #[cfg(target_os = "linux")]
        Ending::Abandoned => return Decided::Close,
    };
    match after_exit(
        program,
        Ended::of(status),
        r.at.saturating_duration_since(born),
    ) {
        AfterExit::Close => Decided::Close,
        AfterExit::Hold(line) => Decided::Hold(line),
    }
}

/// **The bytes that move a held window's cursor to the line's row**,
/// written through the core below what the program printed: DECSTR first,
/// the soft reset `CSI ! p`, which puts the rendition, the margins, origin
/// and insert mode and the character sets back to their defaults and leaves
/// the screen's text where it is (`[[source: vt510-decstr]]`), so a program
/// that ended inside a scroll region cannot keep the line's row inside it;
/// then the cursor hidden (`DECTCEM`, which DECSTR sets), and a new line. A
/// program that ended its output with a new line leaves the row between
/// blank.
pub fn written() -> &'static [u8] {
    b"\x1b[!p\x1b[?25l\r\n"
}

/// **A held window's line as the frame draws it**, on a main display `cols`
/// by `rows`, as `held` says: from the left edge of `row`, the cursor's row
/// once [`written`] has moved it, or on the bottom line; in baud's message
/// box, or as the program's own text ([`plain`]).
pub fn look(
    line: &str,
    row: Row,
    cols: Cols,
    rows: Rows,
    held: crate::config::MessageHeld,
) -> Option<baud_render::TipLook> {
    use crate::config::{HeldLook, MessagePlace};
    use baud_render::MessageAt;
    let at = match held.place {
        MessagePlace::Tip => MessageAt::Row(row),
        MessagePlace::Bar => MessageAt::Bar,
    };
    match held.look {
        HeldLook::Box => baud_render::message_look(line, cols, rows, at),
        HeldLook::Plain => plain(line, at, cols, rows),
    }
}

/// **The line as the program's own text**: wrapped at the window's width as
/// the terminal wraps a line written to it, from the left edge, in the
/// window's default rendition ([`baud_render::Hints::DEFAULT`]), as the
/// same text written after a soft reset (`DECSTR`) is drawn -- not the
/// rendition of the cell under it, which a program may have left in a
/// ground (`SGR 41`) or concealed (`SGR 8`) -- and no space either side.
/// Placed as [`baud_render::message_look`] places a message: on `at`'s
/// row moved up as far as its lines need, or on the last rows for the bar;
/// with too few rows the first lines that fit, and none with no room.
fn plain(
    line: &str,
    at: baud_render::MessageAt,
    cols: Cols,
    rows: Rows,
) -> Option<baud_render::TipLook> {
    use baud_domain::{Codepoint, Col};
    let (cols, rows) = (cols.index(), rows.index());
    if cols == 0 || rows == 0 {
        return None;
    }
    let chars: Vec<char> = line.chars().collect();
    let lines: Vec<&[char]> = chars.chunks(cols).collect();
    let n = lines.len();
    if n == 0 {
        return None;
    }
    let top = match at {
        baud_render::MessageAt::Row(r) => r.index(),
        _ => rows.saturating_sub(n),
    }
    .min(rows.saturating_sub(n));
    let shown = n.min(rows - top);
    let width = lines.iter().map(|l| l.len()).max().unwrap_or(0);
    let fields = lines[..shown]
        .iter()
        .enumerate()
        .map(|(i, l)| baud_render::Field {
            row: Row::from_index(top + i),
            col: Col::ZERO,
            text: l.iter().map(|&c| Codepoint::from_char(c)).collect(),
            hints: vec![baud_render::Hints::DEFAULT; l.len()],
            caret: Col::at_index(l.len()),
        })
        .collect();
    Some(baud_render::TipLook {
        lines: fields,
        area: baud_render::TipArea {
            top: Row::from_index(top),
            bottom: Row::from_index(top + shown),
            left: Col::ZERO,
            right: Col::at_index(width),
        },
    })
}

/// **Where a link's address is while a held line is drawn**: the link
/// preview's ([`crate::copied::Preview`]) at `[message.link]`'s place.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Address {
    /// No preview up.
    Down,
    /// Up at the window's bottom line, `[message.link] place = "bar"`.
    AtBar,
    /// Up by the link, `[message.link] place = "tip"`.
    ByLink,
}

impl Address {
    /// The preview's state, its address at `place`.
    pub fn of(preview: crate::copied::Preview, place: crate::config::MessagePlace) -> Address {
        match (preview, place) {
            (crate::copied::Preview::None, _) => Address::Down,
            (crate::copied::Preview::Up, crate::config::MessagePlace::Bar) => Address::AtBar,
            (crate::copied::Preview::Up, crate::config::MessagePlace::Tip) => Address::ByLink,
        }
    }
}

/// **[`look`] on the view as it is scrolled**: at its tip, the live
/// screen's `row` shown `back` rows down while the window looks back
/// through the history, and no line once that row is below the view; in the
/// bar, the bottom line however the view is scrolled. None while a link's
/// address is up where the line could be ([`Address`]): by the link,
/// wherever the line is, or at the bar with the line there too; the line
/// comes back once the address goes.
pub fn on_view(
    line: &str,
    row: Row,
    back: Rows,
    cols: Cols,
    rows: Rows,
    held: crate::config::MessageHeld,
    address: Address,
) -> Option<baud_render::TipLook> {
    use crate::config::MessagePlace;
    let at = row.index() + back.index();
    let hidden = match address {
        Address::Down => false,
        Address::ByLink => true,
        Address::AtBar => held.place == MessagePlace::Bar,
    };
    if hidden {
        return None;
    }
    match held.place {
        MessagePlace::Bar => look(line, row, cols, rows, held),
        MessagePlace::Tip => {
            (at < rows.index()).then(|| look(line, Row::from_index(at), cols, rows, held))?
        }
    }
}

/// **Whether a key closes a held window**: any key but a modifier's own,
/// which kitty's "report all keys" sends alone (`[[source:
/// kitty-keyboard]]`) and which is only the start of a chord. macOS hands
/// a modifier alone to its own callback, never to the key's, so only the
/// Linux driver asks.
#[cfg_attr(not(any(target_os = "linux", test)), allow(dead_code))]
pub fn closes(key: baud_domain::KeyCode) -> bool {
    use baud_domain::{KeyCode, PuaKey};
    !matches!(
        key,
        KeyCode::Pua(
            PuaKey::LeftShift
                | PuaKey::LeftControl
                | PuaKey::LeftAlt
                | PuaKey::LeftSuper
                | PuaKey::LeftHyper
                | PuaKey::LeftMeta
                | PuaKey::RightShift
                | PuaKey::RightControl
                | PuaKey::RightAlt
                | PuaKey::RightSuper
                | PuaKey::RightHyper
                | PuaKey::RightMeta
                | PuaKey::IsoLevel3Shift
                | PuaKey::IsoLevel5Shift
        )
    )
}

/// **The tip slot's look with a held window's line in it**: the line's
/// fields after `tip`'s, unless the tip's words take a row of the line's:
/// then the words hide the whole line for their moment -- a copy's words at
/// the bar with `[message.held] place = "bar"`, the line wrapped onto the
/// rows above included -- as a link's preview hides it, and the line comes
/// back once they go, when the slot is the line alone. So every path draws
/// one field a row, and no line is drawn cut short. The area is the tip's
/// and the line's together, or the tip's alone while the line is hidden.
/// Either alone is itself.
pub fn with_line(
    tip: Option<baud_render::TipLook>,
    line: Option<baud_render::TipLook>,
) -> Option<baud_render::TipLook> {
    match (tip, line) {
        (Some(mut t), Some(l)) => {
            let taken = |row: Row| t.lines.iter().any(|f| f.row == row);
            if l.lines.iter().any(|f| taken(f.row)) {
                return Some(t);
            }
            let a = &mut t.area;
            a.top = a.top.min(l.area.top);
            a.bottom = a.bottom.max(l.area.bottom);
            a.left = a.left.min(l.area.left);
            a.right = a.right.max(l.area.right);
            t.lines.extend(l.lines);
            Some(t)
        }
        (t, l) => t.or(l),
    }
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
    /// No link preview on screen.
    /// `[message.held]` at its defaults: the box, at the tip.
    const BOX_TIP: crate::config::MessageHeld = crate::config::MessageHeld {
        look: crate::config::HeldLook::Box,
        place: crate::config::MessagePlace::Tip,
    };

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
            AfterExit::Hold("hx stopped with error 1. Press any key to close.".into())
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
                AfterExit::Hold("w3m crashed (signal 11). Press any key to close.".into())
            );
        }
    }

    /// **The status is decoded as this platform's `waitpid` gives it**,
    /// against real children's: `exit 3` and a `kill -9` of itself; and the
    /// two signal numbers the rule names are this platform's. Each child
    /// starts with the signals it sends itself at their default action: a
    /// runner that ignores `SIGINT` or `SIGHUP`, as a background job of a
    /// non-interactive shell ignores `SIGINT`, passes that on through `exec`,
    /// and the child's `kill -INT $$` then ended nothing, read `Code(0)`.
    /// Named break: take every status as an exit code -- the signal reads as
    /// a code.
    #[test]
    fn a_status_is_decoded_as_waitpid_gives_it() {
        let run = |script: &str| {
            let mut sh = std::process::Command::new("/bin/sh");
            crate::sys::default_signals(sh.args(["-c", script]), &[SIGHUP, SIGINT, SIGSEGV])
                .status()
                .expect("sh")
        };
        assert_eq!(Ended::of(run("exit 3")), Ended::Code(3));
        assert_eq!(Ended::of(run("exit 0")), Ended::Code(0));
        assert_eq!(Ended::of(run("kill -9 $$")), Ended::Signal(9));
        assert_eq!(Ended::of(run("kill -HUP $$")), Ended::Signal(SIGHUP));
        assert_eq!(Ended::of(run("kill -INT $$")), Ended::Signal(SIGINT));
        assert_eq!(
            Ended::of(run("ulimit -c 0; kill -SEGV $$")),
            Ended::Signal(SIGSEGV)
        );
    }

    /// **A wait that failed for any reason but `ECHILD` closes the window
    /// and says the errno**, and is not taken for a child something else
    /// reaped: `EINVAL`, the one other failure POSIX gives `waitpid` here,
    /// closes a link's window that a failure would hold, and its line
    /// names the error. Named break: fold every errno into
    /// [`Ending::NotOurs`] (`pty::reap_now`).
    #[test]
    fn a_failed_wait_closes_and_names_its_errno() {
        let born = Instant::now();
        let e = crate::sys::Errno(crate::sys::EINVAL);
        let failed = Reaped {
            ending: Ending::WaitFailed(e),
            at: born + SOON,
        };
        assert_eq!(
            decide(hx(), born, Stream::Closed, Some(&failed)),
            Decided::Close
        );
        let line = wait_failed(e);
        assert!(line.contains("Invalid argument"), "{line}");
        assert_ne!(Ending::WaitFailed(e), Ending::NotOurs);
    }

    /// **A window decides only once both ends of its child are in, in
    /// either order**: the stream closed with no status waits, a status
    /// with the stream open waits, and both decide by the rule -- a failure
    /// within [`HOLD_FOR`] of `born` holds, and a child no status was
    /// reaped for closes. Named break: decide on the stream alone -- a
    /// failure that closed its terminal first closes its window, unheld.
    #[test]
    fn a_window_decides_once_both_ends_are_in() {
        let born = Instant::now();
        let failed = std::process::Command::new("/bin/sh")
            .args(["-c", "exit 1"])
            .status()
            .expect("sh");
        let reaped = Reaped {
            ending: Ending::Exited(failed),
            at: born + SOON,
        };
        assert_eq!(decide(hx(), born, Stream::Closed, None), Decided::Wait);
        assert_eq!(
            decide(hx(), born, Stream::Open, Some(&reaped)),
            Decided::Wait
        );
        assert_eq!(
            decide(hx(), born, Stream::Closed, Some(&reaped)),
            Decided::Hold("hx stopped with error 1. Press any key to close.".into())
        );
        let gone = Reaped {
            ending: Ending::NotOurs,
            at: born + SOON,
        };
        assert_eq!(
            decide(hx(), born, Stream::Closed, Some(&gone)),
            Decided::Close
        );
        assert_eq!(
            decide(None, born, Stream::Closed, Some(&reaped)),
            Decided::Close,
            "a shell window"
        );
    }

    /// **A held window's line goes on the row below the blank one under
    /// what the program printed, whatever state the program left**: one
    /// that ended in reverse video, in insert mode and inside a scroll
    /// region with the cursor at its foot, its last line ended. The text
    /// stays where it is, the cursor is on the second row after it and
    /// hidden, the row between is blank, and the line's box starts there,
    /// at the left edge, every cell in the message pair. Named break: move
    /// the cursor with no soft reset -- the region scrolls the text up.
    #[test]
    fn a_held_windows_line_goes_below_a_blank_row() {
        use baud_core::{BytePos, Owned, ReadLen, Term};
        use baud_domain::{Col, WinSize};
        let ws = WinSize {
            ws_col: 60,
            ws_row: 8,
        };
        let mut store = Owned::new(&ws);
        let mut t = Term::new(store.storage(), &ws);
        let feed = |t: &mut Term<'_>, b: &[u8]| {
            t.write_at(BytePos::ZERO, b);
            t.feed(BytePos::ZERO, ReadLen::from_index(b.len()));
        };
        // A scroll region of rows 2-3, the program's last words on its foot
        // and a new line after them, and reverse video and insert mode left
        // on.
        feed(
            &mut t,
            b"\x1b[2;3r\x1b[3;1Hhx: no such file\r\n\x1b[7m\x1b[4h",
        );
        feed(&mut t, written());
        let rows = crate::programs::screen(&t);
        assert_eq!(rows[1], "hx: no such file", "the text stays: {rows:?}");
        assert_eq!(rows[2], "", "the blank row: {rows:?}");
        let (row, col) = t.cursor();
        assert_eq!((row.index(), col.index()), (3, 0), "{rows:?}");
        assert!(!t.cursor_visible(), "the cursor is hidden");
        let line = "hx stopped with error 1. Press any key to close.";
        let (cols, n) = t.geometry();
        let shown = look(line, row, cols, n, BOX_TIP).expect("room");
        assert_eq!(shown.lines.len(), 1);
        let field = &shown.lines[0];
        assert_eq!((field.row, field.col), (row, Col::ZERO));
        assert!(
            field
                .hints
                .iter()
                .all(|h| *h == baud_render::Hints::MESSAGE),
            "the message pair"
        );
        let text: String = field
            .text
            .iter()
            .filter_map(|c| char::from_u32(c.value()))
            .collect();
        assert_eq!(text.trim(), line);
    }

    /// **The line follows the view through the history, and goes below
    /// it**: scrolled back three rows, the live screen's row 4 is the
    /// view's 7, and scrolled back far enough it is no row of the view.
    /// Named break: draw it at the live row whatever the view -- it covers
    /// the history's text.
    #[test]
    fn the_held_line_follows_the_view() {
        let (cols, rows) = (Cols::of_index(80), Rows::of_index(24));
        let at = |back: usize| {
            let back = match back {
                0 => Rows::ZERO,
                n => Rows::of_index(n),
            };
            on_view(
                "x",
                Row::from_index(4),
                back,
                cols,
                rows,
                BOX_TIP,
                Address::Down,
            )
            .map(|l| l.lines[0].row.index())
        };
        assert_eq!(at(0), Some(4));
        assert_eq!(at(3), Some(7));
        assert_eq!(at(19), Some(23));
        assert_eq!(at(20), None);
    }

    /// **Any key closes a held window but a modifier's own**: a letter,
    /// Escape and a function key close it; Shift or Control pressed alone,
    /// on the way to a chord, does not. Named break: close on every key --
    /// the Control of Control-Shift-C closes the window before the copy.
    #[test]
    fn a_modifier_alone_does_not_close_a_held_window() {
        use baud_domain::{Codepoint, KeyCode, PuaKey};
        assert!(closes(KeyCode::Char(Codepoint::from_char('q'))));
        assert!(closes(KeyCode::Escape));
        assert!(closes(KeyCode::Pua(PuaKey::F21)));
        for m in [PuaKey::LeftShift, PuaKey::RightControl, PuaKey::LeftSuper] {
            assert!(!closes(KeyCode::Pua(m)), "{m:?}");
        }
    }

    /// **A held window's line is drawn in baud's pair on every page, never
    /// darkened**: the window as a hold leaves it -- the program's last
    /// words, [`written`], and [`look`] in the tip slot -- drawn on the CPU
    /// path, the Linux driver's, on the dark page, the light one and a
    /// dimmed glass page: the box's first cell is `#F4A32C` in every pixel,
    /// alpha included, and its text's pixels lie between that and
    /// `#15171B`, which they reach. The row above it is the page. With
    /// `BAUD_HELD_RENDERS` naming a folder, each page is written there as a
    /// PPM to look at. Named break: build the look in the selection's
    /// colours -- the box is the selection's, which is no page's amber.
    #[test]
    fn a_held_line_is_drawn_in_bauds_pair_on_every_page() {
        use baud_core::{BytePos, Owned, ReadLen, Term};
        use baud_domain::{Chan, WinSize};
        use baud_render::{Blend, Damage, Pair, PxSize, Renderer, Stack, Target, Theme};
        let ws = WinSize {
            ws_col: 64,
            ws_row: 8,
        };
        let line = "hx stopped with error 1. Press any key to close.";
        let faces = [baud_font::Face::parse(crate::embedded::REGULAR.bytes).expect("the face")];
        let mut glass = Theme::dark();
        glass.bg_alpha = Chan::from_u8(0x38);
        for (name, theme) in [
            ("dark", Theme::dark()),
            ("light", Theme::light()),
            ("glass", glass),
        ] {
            let mut store = Owned::new(&ws);
            let mut t = Term::new(store.storage(), &ws);
            let feed = |t: &mut Term<'_>, b: &[u8]| {
                t.write_at(BytePos::ZERO, b);
                t.feed(BytePos::ZERO, ReadLen::from_index(b.len()));
            };
            feed(
                &mut t,
                b"$ hx missing.md\r\nhx: missing.md: No such file or directory\r\n",
            );
            feed(&mut t, written());
            let (cols, rows) = t.geometry();
            let look = on_view(
                line,
                t.cursor().0,
                t.window_back(),
                cols,
                rows,
                BOX_TIP,
                Address::Down,
            )
            .expect("room");
            let mut r = Renderer::new(
                Stack::new(&faces).expect("a stack"),
                PxSize::of(14.0).expect("a size"),
                theme,
                Blend::Native,
                cols,
                rows,
            )
            .expect("cell metrics");
            r.fit(&t);
            r.set_tip(Some(look.clone()));
            let (w, h) = r.pixel_size();
            let (wu, hu) = (
                usize::try_from(w.value()).expect("a width"),
                usize::try_from(h.value()).expect("a height"),
            );
            let mut px = vec![0u32; wu * hu];
            {
                let mut target = Target::new(&mut px, w, w, h).expect("a target");
                r.render(&mut t, &mut target, Damage::Full);
            }
            let m = r.metrics();
            let (cw, ch) = (
                usize::try_from(m.cell_w.value()).expect("a cell"),
                usize::try_from(m.cell_h.value()).expect("a cell"),
            );
            let cell = |row: usize, col: usize| {
                (row * ch..(row + 1) * ch)
                    .flat_map(move |y| (col * cw..(col + 1) * cw).map(move |x| y * wu + x))
            };
            let field = &look.lines[0];
            let (row, left) = (field.row.index(), field.col.index());
            assert!(
                cell(row, left).all(|i| px[i] == Pair::BAUD.page.0),
                "{name}: the box is #F4A32C, found 0x{:08x}",
                px[cell(row, left).next().expect("a pixel")]
            );
            let between = |a: u32, b: u32, v: u32| (a.min(b)..=a.max(b)).contains(&v);
            let mut reached = false;
            for col in left + 1..left + field.text.len() - 1 {
                for i in cell(row, col) {
                    let p = px[i];
                    reached |= p == Pair::BAUD.text.0;
                    for s in [0, 8, 16] {
                        assert!(
                            between(
                                (Pair::BAUD.page.0 >> s) & 0xFF,
                                (Pair::BAUD.text.0 >> s) & 0xFF,
                                (p >> s) & 0xFF
                            ),
                            "{name}: 0x{p:08x} is outside the pair at column {col}"
                        );
                    }
                }
            }
            assert!(reached, "{name}: the text is #15171B");
            assert!(
                cell(row - 1, left).all(|i| px[i] != Pair::BAUD.page.0),
                "{name}: the blank row above is the page"
            );
            if let Some(dir) = std::env::var_os("BAUD_HELD_RENDERS") {
                let mut ppm = format!("P6\n{wu} {hu}\n255\n").into_bytes();
                for p in &px {
                    ppm.extend_from_slice(&p.to_be_bytes()[1..]);
                }
                let path = std::path::Path::new(&dir).join(format!("held-{name}.ppm"));
                std::fs::write(&path, ppm).expect("the render written");
            }
        }
    }

    /// **The plain line is drawn as the program's text written after a
    /// soft reset**: over a row a program left in `SGR 41`, a red ground,
    /// and one it left in `SGR 8`, concealed, the line drawn plain, on the
    /// CPU path, is pixel for pixel the same text written into those cells
    /// after `DECSTR` (`CSI ! p`), which sets the default rendition. Named
    /// break: draw it in the rendition of the cells under it, and it takes
    /// the red ground on one row and vanishes on the other.
    #[test]
    fn a_plain_held_line_is_drawn_in_the_default_rendition() {
        use crate::config::{HeldLook, MessageHeld, MessagePlace};
        use baud_core::{BytePos, Owned, ReadLen, Term};
        use baud_domain::WinSize;
        use baud_render::{Blend, Damage, PxSize, Renderer, Stack, Target, Theme};
        let ws = WinSize {
            ws_col: 40,
            ws_row: 6,
        };
        let line = "hx stopped with error 1.";
        let plain = MessageHeld {
            look: HeldLook::Plain,
            place: MessagePlace::Tip,
        };
        let faces = [baud_font::Face::parse(crate::embedded::REGULAR.bytes).expect("the face")];
        let screen: &[u8] = b"\x1b[?25l\x1b[2;1H\x1b[41mred ground under the line\x1b[K\x1b[0m\
            \x1b[3;1H\x1b[8mconcealed under the line\x1b[K\x1b[0m";
        // The frame's pixels, `then` fed after the screen and `tip` drawn.
        let draw = |then: &[u8], tip: Option<baud_render::TipLook>| {
            let mut store = Owned::new(&ws);
            let mut t = Term::new(store.storage(), &ws);
            for b in [screen, then] {
                t.write_at(BytePos::ZERO, b);
                t.feed(BytePos::ZERO, ReadLen::from_index(b.len()));
            }
            let (cols, rows) = t.geometry();
            let mut r = Renderer::new(
                Stack::new(&faces).expect("a stack"),
                PxSize::of(14.0).expect("a size"),
                Theme::dark(),
                Blend::Native,
                cols,
                rows,
            )
            .expect("cell metrics");
            r.fit(&t);
            r.set_tip(tip);
            let (w, h) = r.pixel_size();
            let wu = usize::try_from(w.value()).expect("a width");
            let hu = usize::try_from(h.value()).expect("a height");
            let mut px = vec![0u32; wu * hu];
            {
                let mut target = Target::new(&mut px, w, w, h).expect("a target");
                r.render(&mut t, &mut target, Damage::Full);
            }
            let m = r.metrics();
            let cw = usize::try_from(m.cell_w.value()).expect("a cell");
            let ch = usize::try_from(m.cell_h.value()).expect("a cell");
            (px, wu, cw, ch)
        };
        let (cols, rows) = (Cols::of_index(40), Rows::of_index(6));
        for (row, under) in [(1, "SGR 41"), (2, "SGR 8")] {
            let tip = look(line, Row::from_index(row), cols, rows, plain).expect("room");
            let (shown, wu, cw, ch) = draw(b"", Some(tip));
            let written = format!("\x1b[!p\x1b[?25l\x1b[{};1H{line}", row + 1);
            let (want, ..) = draw(written.as_bytes(), None);
            let cells = (row * ch..(row + 1) * ch)
                .flat_map(|y| (0..line.len() * cw).map(move |x| y * wu + x));
            let differ = cells.filter(|&i| shown[i] != want[i]).count();
            assert_eq!(
                differ, 0,
                "over {under}: {differ} pixels differ from the line written after DECSTR"
            );
        }
    }

    /// **The tip slot keeps both**: a copy's words and the held line each
    /// show, the words first on a row they share, and either alone is
    /// itself. Named break: let the tip replace the line -- Command-C in a
    /// held window takes the line away.
    #[test]
    fn the_held_line_shares_the_tip_slot() {
        let cols = Cols::of_index(80);
        let rows = Rows::of_index(24);
        let words = baud_render::message_look(
            "Copied 3 characters",
            cols,
            rows,
            baud_render::MessageAt::Bar,
        );
        let line = look("x", Row::from_index(4), cols, rows, BOX_TIP);
        let both = with_line(words.clone(), line.clone()).expect("both");
        assert_eq!(both.lines.len(), 2);
        assert_eq!(both.lines[0], words.clone().expect("words").lines[0]);
        assert_eq!(both.lines[1], line.clone().expect("line").lines[0]);
        assert_eq!(with_line(None, line.clone()), line);
        assert_eq!(with_line(words.clone(), None), words);
    }

    /// **At the bar a copy's words hide the line until they go, and then
    /// it comes back**: with `place = "bar"` both are on the bottom row,
    /// and the slot carries only the words there, so every path draws the
    /// words alone -- on the CPU path the bar row is pixel for pixel the
    /// words' frame -- and once they go the line alone is the slot again,
    /// in both looks. Named break: keep the line's field on the words'
    /// row, and the CPU path draws it over them.
    #[test]
    fn a_copys_words_at_the_bar_hide_the_line_until_they_go() {
        use crate::config::{HeldLook, MessageHeld, MessagePlace};
        use baud_core::{BytePos, Owned, ReadLen, Term};
        use baud_domain::WinSize;
        use baud_render::{Blend, Damage, PxSize, Renderer, Stack, Target, Theme};
        let ws = WinSize {
            ws_col: 64,
            ws_row: 8,
        };
        let (cols, rows) = (Cols::of_index(64), Rows::of_index(8));
        let words = baud_render::message_look(
            "Copied 3 characters",
            cols,
            rows,
            baud_render::MessageAt::Bar,
        );
        let faces = [baud_font::Face::parse(crate::embedded::REGULAR.bytes).expect("the face")];
        let draw = |tip: Option<baud_render::TipLook>| {
            let mut store = Owned::new(&ws);
            let mut t = Term::new(store.storage(), &ws);
            let b = b"\x1b[?25l$ hx missing.md\r\n";
            t.write_at(BytePos::ZERO, b);
            t.feed(BytePos::ZERO, ReadLen::from_index(b.len()));
            let mut r = Renderer::new(
                Stack::new(&faces).expect("a stack"),
                PxSize::of(14.0).expect("a size"),
                Theme::dark(),
                Blend::Native,
                cols,
                rows,
            )
            .expect("cell metrics");
            r.fit(&t);
            r.set_tip(tip);
            let (w, h) = r.pixel_size();
            let wu = usize::try_from(w.value()).expect("a width");
            let hu = usize::try_from(h.value()).expect("a height");
            let mut px = vec![0u32; wu * hu];
            {
                let mut target = Target::new(&mut px, w, w, h).expect("a target");
                r.render(&mut t, &mut target, Damage::Full);
            }
            let ch = usize::try_from(r.metrics().cell_h.value()).expect("a cell");
            px[(rows.index() - 1) * ch * wu..rows.index() * ch * wu].to_vec()
        };
        let alone = draw(words.clone());
        for look_is in [HeldLook::Box, HeldLook::Plain] {
            let held = MessageHeld {
                look: look_is,
                place: MessagePlace::Bar,
            };
            let line = look(
                "hx stopped with error 1. Press any key to close.",
                Row::from_index(2),
                cols,
                rows,
                held,
            );
            let both = with_line(words.clone(), line.clone()).expect("both");
            let bar = Row::from_index(rows.index() - 1);
            let on_bar: Vec<_> = both.lines.iter().filter(|f| f.row == bar).collect();
            assert_eq!(
                on_bar,
                vec![&words.clone().expect("words").lines[0]],
                "{look_is:?}: the words alone on the bar"
            );
            let differ = draw(Some(both))
                .iter()
                .zip(&alone)
                .filter(|(a, b)| a != b)
                .count();
            assert_eq!(differ, 0, "{look_is:?}: the bar row is the words' alone");
            assert_eq!(with_line(None, line.clone()), line, "{look_is:?}: back");
        }
    }

    /// **A link's address by the link hides the held line wherever it is**,
    /// and the line comes back once it goes; an address at the bar hides
    /// only a line at the bar, as before. [`Address::of`] reads the
    /// preview's state at `[message.link]`'s place. Named break: keep the
    /// line at its tip under an address by the link -- the two can share a
    /// row.
    #[test]
    fn an_address_by_the_link_hides_the_held_line_until_it_goes() {
        use crate::config::{MessageHeld, MessagePlace};
        use crate::copied::Preview;
        let (cols, rows) = (Cols::of_index(80), Rows::of_index(24));
        assert_eq!(Address::of(Preview::None, MessagePlace::Tip), Address::Down);
        assert_eq!(Address::of(Preview::None, MessagePlace::Bar), Address::Down);
        assert_eq!(Address::of(Preview::Up, MessagePlace::Bar), Address::AtBar);
        assert_eq!(Address::of(Preview::Up, MessagePlace::Tip), Address::ByLink);
        for place in [MessagePlace::Tip, MessagePlace::Bar] {
            let held = MessageHeld { place, ..BOX_TIP };
            let at = |address| {
                on_view(
                    "hx stopped with error 1. Press any key to close.",
                    Row::from_index(4),
                    Rows::ZERO,
                    cols,
                    rows,
                    held,
                    address,
                )
                .map(|l| l.lines[0].row.index())
            };
            let shown = match place {
                MessagePlace::Tip => 4,
                MessagePlace::Bar => 23,
            };
            assert_eq!(at(Address::Down), Some(shown), "{place:?}: no address");
            assert_eq!(at(Address::ByLink), None, "{place:?}: by the link");
            let at_bar = match place {
                MessagePlace::Tip => Some(shown),
                MessagePlace::Bar => None,
            };
            assert_eq!(at(Address::AtBar), at_bar, "{place:?}: at the bar");
            assert_eq!(at(Address::Down), Some(shown), "{place:?}: back");
        }
    }

    /// **A line wrapped at the bar is hidden whole by a copy's words**: a
    /// held line longer than the width, at the bar, wraps onto the rows
    /// above it; a copy's words said there take the bottom row, and no
    /// field of the line is kept, in either look, so its upper rows are not
    /// left drawn with its tail cut off. The slot is the words' alone, area
    /// included. Named break: drop only the line's fields on the words'
    /// row.
    #[test]
    fn a_line_wrapped_at_the_bar_is_hidden_whole_by_a_copys_words() {
        use crate::config::{HeldLook, MessageHeld, MessagePlace};
        let (cols, rows) = (Cols::of_index(32), Rows::of_index(8));
        let words = baud_render::message_look(
            "Copied 3 characters",
            cols,
            rows,
            baud_render::MessageAt::Bar,
        )
        .expect("words");
        for look_is in [HeldLook::Box, HeldLook::Plain] {
            let held = MessageHeld {
                look: look_is,
                place: MessagePlace::Bar,
            };
            let line = look(
                "hx stopped with error 1. Press any key to close.",
                Row::from_index(2),
                cols,
                rows,
                held,
            )
            .expect("line");
            let line_rows: Vec<Row> = line.lines.iter().map(|f| f.row).collect();
            assert!(
                line_rows
                    .iter()
                    .any(|&r| words.lines.iter().all(|w| w.row != r)),
                "{look_is:?}: the line wraps above the words' row: {line_rows:?}"
            );
            let both = with_line(Some(words.clone()), Some(line.clone())).expect("both");
            assert_eq!(both, words, "{look_is:?}: the words alone, area too");
            assert_eq!(
                with_line(None, Some(line.clone())),
                Some(line),
                "{look_is:?}: back"
            );
        }
    }

    /// **The slot's area spans the words and the line**: the rows from the
    /// higher's top to the lower's bottom and the columns from the leftmost
    /// to the rightmost, so the area the pointer may cross is the whole slot
    /// drawn. Named break: keep the words' area alone, which leaves the
    /// line's row out.
    #[test]
    fn the_slots_area_spans_the_words_and_the_line() {
        let (cols, rows) = (Cols::of_index(80), Rows::of_index(24));
        let words = baud_render::message_look(
            "Copied 3 characters",
            cols,
            rows,
            baud_render::MessageAt::Bar,
        )
        .expect("words");
        let line = look(
            "hx stopped with error 1. Press any key to close.",
            Row::from_index(4),
            cols,
            rows,
            BOX_TIP,
        )
        .expect("line");
        let both = with_line(Some(words.clone()), Some(line.clone())).expect("both");
        let (w, l) = (words.area, line.area);
        assert_eq!(
            both.area,
            baud_render::TipArea {
                top: w.top.min(l.top),
                bottom: w.bottom.max(l.bottom),
                left: w.left.min(l.left),
                right: w.right.max(l.right),
            }
        );
    }

    /// **The line takes `[message.held]`'s look and place**: `"box"` in
    /// the message pair with a space either side, `"plain"` the line alone
    /// in the default rendition; `"tip"` on the row below
    /// the blank one, following the view back through the history and gone
    /// once its row is below the view, and `"bar"` on the bottom line
    /// however far the view is scrolled, and gone while a link's preview
    /// shows its address there, as a copy's words go, the tip staying; and a
    /// plain line wraps at the width as a written line does, moved up from
    /// the last row as far as its lines need. Named breaks: draw `"plain"`
    /// in the box; put `"bar"` at the tip's row; take the bar off the view
    /// with the tip's row; draw the bar over the preview's address.
    #[test]
    fn the_held_line_takes_its_look_and_its_place() {
        use crate::config::{HeldLook, MessageHeld, MessagePlace};
        use baud_domain::Col;
        let (cols, rows) = (Cols::of_index(80), Rows::of_index(24));
        let line = "hx stopped with error 1. Press any key to close.";
        let text = |f: &baud_render::Field| -> String {
            f.text
                .iter()
                .filter_map(|c| char::from_u32(c.value()))
                .collect()
        };
        let held = |look, place| MessageHeld { look, place };
        let row = Row::from_index(4);
        for (look_is, hints, shown) in [
            (
                HeldLook::Box,
                baud_render::Hints::MESSAGE,
                format!(" {line} "),
            ),
            (
                HeldLook::Plain,
                baud_render::Hints::DEFAULT,
                line.to_string(),
            ),
        ] {
            for (place, at) in [(MessagePlace::Tip, 4), (MessagePlace::Bar, 23)] {
                let l = look(line, row, cols, rows, held(look_is, place)).expect("room");
                assert_eq!(l.lines.len(), 1, "{look_is:?} {place:?}");
                let f = &l.lines[0];
                assert_eq!(
                    (f.row.index(), f.col),
                    (at, Col::ZERO),
                    "{look_is:?} {place:?}"
                );
                assert!(
                    f.hints.iter().all(|h| *h == hints),
                    "{look_is:?} {place:?}: {:?}",
                    f.hints
                );
                assert_eq!(text(f), shown, "{look_is:?} {place:?}");
                let back = on_view(
                    line,
                    row,
                    Rows::of_index(3),
                    cols,
                    rows,
                    held(look_is, place),
                    Address::Down,
                )
                .expect("on the view");
                let want = match place {
                    MessagePlace::Tip => 7,
                    MessagePlace::Bar => 23,
                };
                assert_eq!(back.lines[0].row.index(), want, "{look_is:?} {place:?}");
                let far = on_view(
                    line,
                    row,
                    Rows::of_index(20),
                    cols,
                    rows,
                    held(look_is, place),
                    Address::Down,
                )
                .map(|l| l.lines[0].row.index());
                let want = match place {
                    MessagePlace::Tip => None,
                    MessagePlace::Bar => Some(23),
                };
                assert_eq!(far, want, "{look_is:?} {place:?}: scrolled back 20");
                let under = on_view(
                    line,
                    row,
                    Rows::ZERO,
                    cols,
                    rows,
                    held(look_is, place),
                    Address::AtBar,
                )
                .map(|l| l.lines[0].row.index());
                let want = match place {
                    MessagePlace::Tip => Some(4),
                    MessagePlace::Bar => None,
                };
                assert_eq!(under, want, "{look_is:?} {place:?}: under a link's preview");
            }
        }
        let narrow = Cols::of_index(20);
        let wrapped = look(
            line,
            Row::from_index(23),
            narrow,
            rows,
            held(HeldLook::Plain, MessagePlace::Tip),
        )
        .expect("room");
        let chars: Vec<char> = line.chars().collect();
        let want: Vec<(usize, String)> = chars
            .chunks(20)
            .enumerate()
            .map(|(i, c)| (24 - chars.len().div_ceil(20) + i, c.iter().collect()))
            .collect();
        let got: Vec<(usize, String)> = wrapped
            .lines
            .iter()
            .map(|f| (f.row.index(), text(f)))
            .collect();
        assert_eq!(got, want);
    }

    /// **`"plain"` draws exactly what the line written into the cells
    /// drew**, as the hold first wrote it: on the CPU path, on the dark page
    /// and the light one, the line's row of a window holding the plain line
    /// over its blank cells is, pixel for pixel, that row of one where the
    /// line was written into the cells after [`written`]'s DECSTR -- and the
    /// first window's cells on that row stay blank, so a copy of its text is
    /// the program's alone. Named breaks: build the plain line in the
    /// message pair, or in the selection's colours.
    #[test]
    fn a_plain_held_line_draws_as_the_line_written_into_the_cells() {
        use crate::config::{HeldLook, MessageHeld, MessagePlace};
        use baud_core::{BytePos, Owned, ReadLen, Term};
        use baud_domain::WinSize;
        use baud_render::{Blend, Damage, PxSize, Renderer, Stack, Target, Theme};
        let ws = WinSize {
            ws_col: 64,
            ws_row: 8,
        };
        let line = "hx stopped with error 1. Press any key to close.";
        let plain = MessageHeld {
            look: HeldLook::Plain,
            place: MessagePlace::Tip,
        };
        let faces = [baud_font::Face::parse(crate::embedded::REGULAR.bytes).expect("the face")];
        let feed = |t: &mut Term<'_>, b: &[u8]| {
            t.write_at(BytePos::ZERO, b);
            t.feed(BytePos::ZERO, ReadLen::from_index(b.len()));
        };
        for (name, theme) in [("dark", Theme::dark()), ("light", Theme::light())] {
            let draw = |into_cells: bool| -> (Vec<u32>, usize, usize, usize) {
                let mut store = Owned::new(&ws);
                let mut t = Term::new(store.storage(), &ws);
                feed(
                    &mut t,
                    b" missing.md\r\n\x1b[7mhx: missing.md: No such file\r\n",
                );
                feed(&mut t, written());
                let (row, _) = t.cursor();
                let (cols, rows) = t.geometry();
                let tip = match into_cells {
                    true => {
                        feed(&mut t, line.as_bytes());
                        None
                    }
                    false => on_view(line, row, t.window_back(), cols, rows, plain, Address::Down),
                };
                if !into_cells {
                    let screen = crate::programs::screen(&t);
                    assert_eq!(screen[row.index()], "", "{name}: nothing in a cell");
                }
                let mut r = Renderer::new(
                    Stack::new(&faces).expect("a stack"),
                    PxSize::of(14.0).expect("a size"),
                    theme.clone(),
                    Blend::Native,
                    cols,
                    rows,
                )
                .expect("cell metrics");
                r.fit(&t);
                r.set_tip(tip);
                let (w, h) = r.pixel_size();
                let wu = usize::try_from(w.value()).expect("a width");
                let hu = usize::try_from(h.value()).expect("a height");
                let mut px = vec![0u32; wu * hu];
                {
                    let mut target = Target::new(&mut px, w, w, h).expect("a target");
                    r.render(&mut t, &mut target, Damage::Full);
                }
                let ch = usize::try_from(r.metrics().cell_h.value()).expect("a cell");
                (px, wu, ch, row.index())
            };
            let (overlay, wu, ch, row) = draw(false);
            let (cells, _, _, _) = draw(true);
            let band = row * ch * wu..(row + 1) * ch * wu;
            let differ = overlay[band.clone()]
                .iter()
                .zip(&cells[band.clone()])
                .filter(|(a, b)| a != b)
                .count();
            assert_eq!(
                differ, 0,
                "{name}: {differ} pixels differ on the line's row"
            );
            let page = cells[band.start];
            assert!(
                cells[band].iter().any(|&p| p != page),
                "{name}: the control, the written line draws ink"
            );
        }
    }
}
