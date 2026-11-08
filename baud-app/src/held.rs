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

/// **A window's child, reaped**: the status `waitpid` gave it, and when.
/// No status when `waitpid` said there was no such child to reap -- one
/// something else reaped -- which closes the window as a shell's does.
#[derive(Clone, Copy, Debug)]
pub struct Reaped {
    pub status: Option<std::process::ExitStatus>,
    pub at: Instant,
}

/// **Where a window's watch puts its child's status**, set once by
/// whichever thread reaps it and read by the window's turns.
pub type Slot = std::sync::Arc<std::sync::OnceLock<Reaped>>;

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
    let Some(status) = r.status else {
        return Decided::Close;
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

/// **A held window's line as the frame draws it**: in baud's message box,
/// from the left edge of `row`, the cursor's row once [`written`] has
/// moved it, on a main display `cols` by `rows`.
pub fn look(line: &str, row: Row, cols: Cols, rows: Rows) -> Option<baud_render::TipLook> {
    baud_render::message_look(line, cols, rows, baud_render::MessageAt::Row(row))
}

/// **[`look`] on the view as it is scrolled**: the live screen's `row`
/// shown `back` rows down while the window looks back through the history,
/// and no line once that row is below the view.
pub fn on_view(
    line: &str,
    row: Row,
    back: Rows,
    cols: Cols,
    rows: Rows,
) -> Option<baud_render::TipLook> {
    let at = row.index() + back.index();
    (at < rows.index()).then(|| look(line, Row::from_index(at), cols, rows))?
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
/// fields after `tip`'s, so on a row they share the tip's words show for
/// their moment and the line again after. Either alone is itself.
pub fn with_line(
    tip: Option<baud_render::TipLook>,
    line: Option<baud_render::TipLook>,
) -> Option<baud_render::TipLook> {
    match (tip, line) {
        (Some(mut t), Some(l)) => {
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
        assert_eq!(
            Ended::of(run("ulimit -c 0; kill -SEGV $$")),
            Ended::Signal(SIGSEGV)
        );
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
            status: Some(failed),
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
            status: None,
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
        let shown = look(line, row, cols, n).expect("room");
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
            on_view("x", Row::from_index(4), back, cols, rows).map(|l| l.lines[0].row.index())
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
            let look = on_view(line, t.cursor().0, t.window_back(), cols, rows).expect("room");
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
        let line = look("x", Row::from_index(4), cols, rows);
        let both = with_line(words.clone(), line.clone()).expect("both");
        assert_eq!(both.lines.len(), 2);
        assert_eq!(both.lines[0], words.clone().expect("words").lines[0]);
        assert_eq!(both.lines[1], line.clone().expect("line").lines[0]);
        assert_eq!(with_line(None, line.clone()), line);
        assert_eq!(with_line(words.clone(), None), words);
    }
}
