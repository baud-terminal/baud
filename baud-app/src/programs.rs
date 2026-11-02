//! **Real programs under the app's pointer**: vim, less, a shell, zellij
//! and herdr run in a pty, their output fed through `reads::feed` -- the
//! `reads::paced` both drivers call, with no delay held -- between the
//! seat's `before_read` and `after_read`, as the Linux driver's read is,
//! and the pointer driven through the Linux driver's seat
//! (`Seat::input`, and `Seat::input_beside` with the scrollbar's thumb),
//! the macOS driver's order, with every byte it sends and every answer the
//! core owes written back to the child. What is asserted is what the child
//! did, read off the screen -- the cell vim's cursor moved to, the line a
//! pager or a multiplexer's pane scrolled to -- and what the app did: the
//! window over the history, the text a copy takes, the thumb drawn.
//!
//! **Each program is optional.** One that is not installed is skipped with
//! the reason printed, so the suite passes on a machine without it; each
//! runs with its home, its configuration, its state and its sockets in a
//! directory of its own, made for the test and removed after it, so no
//! user's configuration reaches it and nothing it writes outlives it.
//!
//! **Waited on, never slept.** Every step waits for the screen to say what
//! the step did, with `poll` on the master, to a deadline that fails the
//! test with the screen dumped -- a wait for a condition, so a slow machine
//! takes longer and does not read differently.

use crate::gesture::Pressed;
use crate::seat::{Geometry, Input, Owed, Seat};
use baud_core::{Modifiers, MouseTracking, Owned, Term};
use baud_domain::EventMillis as Ms;
use baud_domain::{Col, Row, WinSize};
use baud_font::Px;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// The grid every program runs on, and the ten-pixel cells a point is
/// mapped through.
const WS: WinSize = WinSize {
    ws_col: 80,
    ws_row: 24,
};
const G: Geometry = Geometry {
    pad: (0, 0),
    cell: (10.0, 10.0),
    grid: (80, 24),
    scale: 1.0,
};

/// How long a step may take before the test fails: long, because it is a
/// bound on a wait for the screen, not a time anything sleeps.
const DEADLINE: Duration = Duration::from_secs(30);

const MS: Duration = Duration::from_millis(1);

/// A cell, `(row, col)` from indices.
macro_rules! cell {
    ($row:expr, $col:expr) => {
        (Row::from_index($row), Col::at_index($col))
    };
}

/// `name` on `PATH`, or `None`, said: the test that needed it is skipped.
pub(crate) fn found(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH").unwrap_or_default();
    let hit = std::env::split_paths(&path)
        .map(|d| d.join(name))
        .find(|p| p.is_file());
    if hit.is_none() {
        eprintln!("skipped: {name} is not installed");
    }
    hit
}

/// **A directory of the test's own**, under the system's temporary
/// directory and short enough for a socket path in it, removed on drop.
pub(crate) struct Scratch(PathBuf);

impl Scratch {
    pub(crate) fn new(tag: &str) -> Scratch {
        let dir = std::env::temp_dir().join(format!("bp-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        for sub in ["h", "c", "d", "s", "k", "r", "t"] {
            std::fs::create_dir_all(dir.join(sub)).expect("scratch");
        }
        Scratch(dir)
    }

    pub(crate) fn path(&self) -> &Path {
        &self.0
    }

    /// The environment a child is handed: `PATH` kept, and home, the XDG
    /// directories and `TMPDIR` all in here, so a program's configuration,
    /// state, caches and sockets are the test's.
    pub(crate) fn env(&self, extra: &[(&str, &str)]) -> Vec<(OsString, OsString)> {
        let at = |s: &str| self.0.join(s).into_os_string();
        let mut env = vec![
            (
                OsString::from("PATH"),
                std::env::var_os("PATH").unwrap_or_default(),
            ),
            ("HOME".into(), at("h")),
            ("XDG_CONFIG_HOME".into(), at("c")),
            ("XDG_DATA_HOME".into(), at("d")),
            ("XDG_STATE_HOME".into(), at("s")),
            ("XDG_CACHE_HOME".into(), at("k")),
            ("XDG_RUNTIME_DIR".into(), at("r")),
            ("TMPDIR".into(), at("t")),
            ("LANG".into(), "en_US.UTF-8".into()),
        ];
        env.extend(
            extra
                .iter()
                .map(|(k, v)| (OsString::from(k), OsString::from(v))),
        );
        env
    }

    /// A file of 99 numbered lines, `line 01` to `line 99`, for a program
    /// to show.
    pub(crate) fn numbered(&self, name: &str) -> PathBuf {
        let p = self.0.join(name);
        let text: String = (1..=99).map(|i| format!("line {i:02}\n")).collect();
        std::fs::write(&p, text).expect("numbered");
        p
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// **A program on a pty and the app's side of it**: the terminal its
/// output is read into, the seat the pointer goes through, and every byte
/// the seat sent, kept for the report a test reads.
pub(crate) struct Under<'t, 's> {
    pub(crate) term: &'t mut Term<'s>,
    pub(crate) seat: Seat,
    /// The child this harness forked, the one its drop waits for.
    pid: core::ffi::c_int,
    master: crate::sys::Fd,
    pub(crate) sent: Vec<u8>,
    /// Every byte written to the child: the seat's, the core's answers and
    /// the keys typed, in order.
    pub(crate) wrote: Vec<u8>,
    /// The pointer's clock, for the stamps a click is counted by.
    clock: Duration,
    /// What a point is mapped through.
    geometry: Geometry,
}

/// What one wait on the child's side of the pty found.
pub(crate) enum Heard {
    Bytes(Vec<u8>),
    /// Nothing by the wait's end.
    Quiet,
    /// The child's side closed: it has exited, and so has every process
    /// that held its terminal.
    Closed,
}

impl<'t, 's> Under<'t, 's> {
    /// `cmd` forked under `/bin/sh -c` on a pty of the grid, in `env`.
    pub(crate) fn start(
        term: &'t mut Term<'s>,
        cmd: &str,
        env: Vec<(OsString, OsString)>,
    ) -> Under<'t, 's> {
        let ws = crate::pty::text_area(&WS, Px::of(10), Px::of(10));
        Under::start_on(term, cmd, env, ws, G)
    }

    /// [`Under::start`] on a pty of `ws`, its points mapped through
    /// `geometry`: a grid and a cell of the caller's.
    pub(crate) fn start_on(
        term: &'t mut Term<'s>,
        cmd: &str,
        env: Vec<(OsString, OsString)>,
        ws: crate::sys::Winsize,
        geometry: Geometry,
    ) -> Under<'t, 's> {
        let env = crate::pty::child_env(env, None);
        let (pid, master) =
            crate::pty::Pty::fork_child(ws, Some(cmd.into()), env).expect("forkpty");
        Under {
            term,
            seat: Seat::new(),
            pid,
            master,
            sent: Vec::new(),
            wrote: Vec::new(),
            clock: Duration::from_secs(1),
            geometry,
        }
    }

    /// Bytes to the child, all of them.
    fn send(&mut self, mut b: &[u8]) {
        self.wrote.extend_from_slice(b);
        while !b.is_empty() {
            // SAFETY: an open descriptor and a buffer valid for its length.
            let n = unsafe { crate::sys::write(self.master, b.as_ptr().cast(), b.len()) };
            let n = usize::try_from(n).expect("the child's pty takes the write");
            b = &b[n..];
        }
    }

    /// The answers the core owes, to the child.
    fn answer(&mut self) {
        let reply = self.term.reply().to_vec();
        self.term.clear_reply();
        self.send(&reply);
    }

    /// One wait on the master of up to `ms`, and what it read fed as the
    /// module's doc says; whether anything arrived.
    fn pump(&mut self, wait: Duration) -> bool {
        match self.read(wait) {
            Heard::Bytes(b) => {
                self.take(&b);
                true
            }
            Heard::Quiet | Heard::Closed => false,
        }
    }

    /// **One wait on the master of up to `wait`**, and what it read, not
    /// yet fed.
    pub(crate) fn read(&mut self, wait: Duration) -> Heard {
        let ms = i32::try_from(wait.as_millis()).unwrap_or(i32::MAX);
        let mut fd = crate::sys::PollFd {
            fd: self.master,
            events: crate::sys::POLLIN,
            revents: 0,
        };
        // SAFETY: one live `PollFd`.
        let ready = unsafe { crate::sys::poll(&mut fd, 1, ms) };
        if ready <= 0 {
            return Heard::Quiet;
        }
        let mut buf = vec![0u8; 1 << 16];
        // SAFETY: an open descriptor and a buffer valid for its length.
        let n = unsafe { crate::sys::read(self.master, buf.as_mut_ptr().cast(), buf.len()) };
        // Ready and nothing to read, or the error a master reads once the
        // last holder of the child's side has closed it (`EIO`).
        match usize::try_from(n) {
            Ok(n) if n > 0 => {
                buf.truncate(n);
                Heard::Bytes(buf)
            }
            _ => Heard::Closed,
        }
    }

    /// **A read fed as the module's doc says**, between the seat's
    /// `before_read` and `after_read`, and every answer it owed written to
    /// the child.
    pub(crate) fn take(&mut self, read: &[u8]) {
        let mut out = Vec::new();
        let before = self.seat.before_read(self.term);
        crate::reads::feed(
            self.term,
            [read],
            |b| out.extend_from_slice(b),
            |_| {},
            &mut crate::hostlog::NoLog,
        );
        self.seat.after_read(self.term, before);
        self.send(&out);
        self.answer();
    }

    /// **One step of a kitty picture's decode** (`reads::decode_step`), as
    /// the Linux loop takes one a turn, and the answer it owed to the
    /// child: whether more is left to decode.
    pub(crate) fn decode(&mut self) -> baud_domain::DecodeProgress {
        let (progress, _) = crate::reads::decode_step(self.term);
        self.answer();
        progress
    }

    /// **Waits until `done` holds of the terminal and the seat**, reading
    /// the child meanwhile; fails with the screen when the deadline passes.
    pub(crate) fn until(&mut self, what: &str, done: impl Fn(&Term<'_>, &Seat) -> bool) {
        let end = Instant::now() + DEADLINE;
        while !done(self.term, &self.seat) {
            assert!(
                Instant::now() < end,
                "{what}: not by the deadline; the screen:\n{}",
                screen(self.term).join("\n")
            );
            self.pump(MS * 50);
        }
    }

    /// Reads until the child has been quiet for `ms`: what a test does
    /// before it asserts that something did *not* happen.
    pub(crate) fn settle(&mut self, quiet: Duration) {
        while self.pump(quiet) {}
    }

    /// Keystrokes, as typed.
    pub(crate) fn typed(&mut self, b: &[u8]) {
        self.send(b);
    }

    /// One pointer input through the seat, at the next millisecond stamp;
    /// what it sent goes to the child and into [`Under::sent`].
    pub(crate) fn input(&mut self, e: Input, mods: Modifiers) -> Owed {
        let mut out = Vec::new();
        let owed = self.seat.input(self.term, e, mods, self.geometry, |b| {
            out.extend_from_slice(b)
        });
        self.sent.extend_from_slice(&out);
        self.send(&out);
        let reply = self.term.reply().to_vec();
        self.sent.extend_from_slice(&reply);
        self.answer();
        owed
    }

    /// The pointer moved to the middle of `(row, col)`.
    pub(crate) fn at(&mut self, (row, col): (Row, Col), mods: Modifiers) -> Owed {
        let mid = |i: usize| i as f64 * 10.0 + 5.0;
        let (x, y) = (mid(col.index()), mid(row.index()));
        self.input(Input::Moved(x, y), mods)
    }

    fn stamp(&mut self) -> Ms {
        self.clock += MS * 400;
        Ms::of_millis(u32::try_from(self.clock.as_millis()).unwrap_or(u32::MAX))
    }

    pub(crate) fn press(&mut self, button: Pressed, mods: Modifiers) -> Owed {
        let time = self.stamp();
        self.input(Input::Pressed { button, time }, mods)
    }

    pub(crate) fn release(&mut self, button: Pressed, mods: Modifiers) -> Owed {
        let time = self.stamp();
        self.input(Input::Released { button, time }, mods)
    }

    /// A left click at `(row, col)`.
    pub(crate) fn click(&mut self, at: (Row, Col), mods: Modifiers) -> Owed {
        self.at(at, mods);
        self.press(Pressed::Left, mods);
        self.release(Pressed::Left, mods)
    }

    /// The bytes the seat has sent since the last take.
    pub(crate) fn take_sent(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.sent)
    }
}

impl Drop for Under<'_, '_> {
    fn drop(&mut self) {
        // SAFETY: the master this harness opened, closed once: the child's
        // session hears the hangup.
        drop(unsafe { <std::os::fd::OwnedFd as std::os::fd::FromRawFd>::from_raw_fd(self.master) });
        // And the child is waited for, so the test ends with no program of
        // its own still running: the hangup ends it. Its pid and no other:
        // a wait for any child would take one another test in this process
        // started, and that test's wait would fail.
        let mut status = 0;
        // SAFETY: a live out-pointer and the pid this harness forked.
        unsafe { crate::sys::waitpid(self.pid, &mut status, 0) };
    }
}

/// The live screen's rows as text, blanks as spaces, trailing ones cut.
pub(crate) fn screen(t: &Term<'_>) -> Vec<String> {
    let v = t.window();
    (0..v.rows().index())
        .map(|y| row_text(v.row(Row::from_index(y))))
        .collect()
}

fn row_text(cells: &[baud_domain::PackedCell]) -> String {
    cells
        .iter()
        .map(|c| {
            c.content
                .as_codepoint()
                .and_then(|cp| char::from_u32(cp.value()))
                .unwrap_or(' ')
        })
        .collect::<String>()
        .trim_end()
        .to_string()
}

const NONE: Modifiers = Modifiers::NONE;

/// **The harness waits for its own child and for no other**: libtest runs
/// the tests as threads of one process, so a harness that waited for any
/// child would take a child another test started. A child of the test's
/// own, which exits before a harness starts, is still the test's to wait
/// for once the harness has ended, with its exit status.
#[test]
fn the_harness_waits_for_its_own_child_alone() {
    let mut other = std::process::Command::new("/bin/sh")
        .args(["-c", "exit 3"])
        .stdout(std::process::Stdio::piped())
        .spawn()
        .expect("a child of the test's own");
    let mut out = Vec::new();
    let mut pipe = other.stdout.take().expect("its output");
    std::io::Read::read_to_end(&mut pipe, &mut out).expect("its output ends as it exits");
    let dir = Scratch::new("wo");
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    let mut u = Under::start(&mut t, "printf ready; exec cat", dir.env(&[]));
    u.until("the harness's child", |t, _| screen(t)[0] == "ready");
    drop(u);
    let status = other
        .wait()
        .expect("the test's own child, still to wait for");
    assert_eq!(status.code(), Some(3));
}

/// vim on `file` with the mouse on, no user configuration and no viminfo,
/// and `more` arguments.
fn vim_on(file: &Path, more: &str) -> String {
    format!(
        "exec vim -u NONE -i NONE -N -n -c 'set mouse=a showmode' {more} {}",
        file.display()
    )
}

/// **A click in vim moves vim's cursor to the clicked cell**, in SGR's
/// reports and in X10's: vim turns the mouse on under `mouse=a`, the seat
/// sends the press and the release to it at the cell the pointer is over,
/// in the encoding vim's `ttymouse` asked for, and vim's cursor lands
/// there -- row 5 col 2, then row 12 col 4. vim asks for SGR whatever
/// `ttymouse` says where the terminfo's `XM` names `?1006`, and once
/// baud's `DA2` answer names an xterm that has it, so the X10 run clears
/// both (`t_XM`, `t_RV`) and vim asks for `?1000` alone.
#[test]
fn a_click_in_vim_moves_its_cursor_to_the_cell() {
    let Some(_) = found("vim") else { return };
    for (ttymouse, press) in [("xterm", &b"\x1b[M #&"[..]), ("sgr", b"\x1b[<0;3;6M")] {
        let dir = Scratch::new("vc");
        let file = dir.numbered("n.txt");
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        let rv = match ttymouse {
            "xterm" => "--cmd 'set t_RV= t_XM='",
            _ => "",
        };
        let cmd = vim_on(&file, &format!("{rv} -c 'set ttymouse={ttymouse}'"));
        let mut u = Under::start(&mut t, &cmd, dir.env(&[]));
        u.until("vim shows the file with the mouse on", |t, _| {
            screen(t)[0] == "line 01" && t.mouse_tracking() != MouseTracking::Off
        });
        u.click(cell!(5, 2), NONE);
        let sent = u.take_sent();
        assert!(
            sent.starts_with(press),
            "{ttymouse}: the press at row 5 col 2: {:?}",
            String::from_utf8_lossy(&sent)
        );
        u.until(&format!("{ttymouse}: vim's cursor at the click"), |t, _| {
            t.cursor() == cell!(5, 2)
        });
        u.click(cell!(12, 4), NONE);
        u.until(&format!("{ttymouse}: and at the next"), |t, _| {
            t.cursor() == cell!(12, 4)
        });
    }
}

const SHIFT: Modifiers = Modifiers::SHIFT;
const CTRL: Modifiers = Modifiers::CTRL;

/// vim with the mouse on in SGR's reports, set rather than left to what
/// vim reads of the terminal, on a 99-line file, shown.
fn vim<'t, 's>(term: &'t mut Term<'s>, dir: &Scratch) -> Under<'t, 's> {
    let file = dir.numbered("n.txt");
    let cmd = vim_on(&file, "-c 'set ttymouse=sgr'");
    let mut u = Under::start(term, &cmd, dir.env(&[]));
    u.until("vim shows the file with the mouse on", |t, _| {
        screen(t)[0] == "line 01" && t.mouse_tracking() != MouseTracking::Off
    });
    u
}

/// **A drag in vim is vim's visual selection**: the press, the motion
/// with the button held, under `?1002`, and the release reach vim, which
/// shows `-- VISUAL --` with its cursor at the drag's end.
#[test]
fn a_drag_in_vim_is_its_visual_selection() {
    let Some(_) = found("vim") else { return };
    let dir = Scratch::new("vd");
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    let mut u = vim(&mut t, &dir);
    u.at(cell!(2, 1), NONE);
    u.press(Pressed::Left, NONE);
    u.at(cell!(3, 3), NONE);
    u.at(cell!(6, 4), NONE);
    u.release(Pressed::Left, NONE);
    let sent = String::from_utf8_lossy(&u.take_sent()).into_owned();
    assert_eq!(
        sent, "\x1b[<0;2;3M\x1b[<32;4;4M\x1b[<32;5;7M\x1b[<0;5;7m",
        "the press, two drags, the release"
    );
    u.until("vim's visual selection to the drag's end", |t, _| {
        screen(t)[23].starts_with("-- VISUAL --") && t.cursor() == cell!(6, 4)
    });
}

/// **The wheel scrolls vim, a report a line**: three detents down are
/// three presses of button 5 at the pointer, which vim scrolls three
/// lines each (`mousescroll`'s default), and three up bring it back.
#[test]
fn the_wheel_scrolls_vim() {
    let Some(_) = found("vim") else { return };
    let dir = Scratch::new("vw");
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    let mut u = vim(&mut t, &dir);
    u.at(cell!(4, 10), NONE);
    u.take_sent();
    assert_eq!(u.input(Input::Steps(3.0), NONE), Owed::Nothing);
    assert_eq!(
        String::from_utf8_lossy(&u.take_sent()),
        "\x1b[<65;11;5M".repeat(3),
        "three presses of button 5"
    );
    u.until("vim scrolled nine lines", |t, _| screen(t)[0] == "line 10");
    u.input(Input::Steps(-3.0), NONE);
    assert_eq!(
        String::from_utf8_lossy(&u.take_sent()),
        "\x1b[<64;11;5M".repeat(3)
    );
    u.until("and back", |t, _| screen(t)[0] == "line 01");
}

/// **A modifier reaches vim in the report, and Shift is baud's**: a
/// Ctrl-click is button 0 with 16 added, which vim takes as `:tag` for
/// the word under it and answers with its error for no tags file; a
/// Shift-drag is the selection's under XTSHIFTESCAPE's default, sends
/// vim nothing and leaves its cursor; a Shift-wheel on the alternate
/// screen is the cursor keys under `?1007`, in the application mode vim
/// set (`SS3 B`), which move vim's cursor.
#[test]
fn a_modifier_reaches_vim_and_shift_is_bauds() {
    let Some(_) = found("vim") else { return };
    let dir = Scratch::new("vm");
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    let mut u = vim(&mut t, &dir);
    u.click(cell!(3, 2), CTRL);
    assert!(u.take_sent().starts_with(b"\x1b[<16;3;4M"), "Ctrl adds 16");
    u.until("vim's :tag answers", |t, _| {
        screen(t)[21] == "E433: No tags file" && screen(t)[23].starts_with("Press ENTER")
    });
    u.typed(b"\r");
    u.until("the prompt gone", |t, _| screen(t)[21] == "line 22");
    u.click(cell!(1, 1), NONE);
    u.until("vim's cursor at row 1", |t, _| t.cursor() == cell!(1, 1));
    u.take_sent();
    u.at(cell!(8, 0), SHIFT);
    u.press(Pressed::Left, SHIFT);
    u.at(cell!(8, 6), SHIFT);
    let owed = u.release(Pressed::Left, SHIFT);
    assert_eq!(owed, Owed::Selected, "Shift's drag is a selection");
    assert_eq!(u.take_sent(), b"", "and vim hears none of it");
    assert_eq!(u.seat.selected_text(u.term).as_deref(), Some("line 09"));
    u.input(Input::Steps(2.0), SHIFT);
    assert_eq!(u.take_sent(), b"\x1bOB\x1bOB", "Shift's wheel is the keys");
    u.until("vim's cursor two rows down", |t, _| {
        t.cursor().0 == Row::from_index(3)
    });
}

/// less with the mouse on, on `file`, with no history file and no
/// `LESS` from the user.
fn less_on(file: &Path) -> String {
    format!("exec less --mouse {}", file.display())
}

/// **The wheel scrolls less**: `less --mouse` tracks the mouse, the
/// seat's wheel reaches it as buttons 4 and 5 at the pointer, and less
/// scrolls a line a report, down and back up.
#[test]
fn the_wheel_scrolls_less() {
    let Some(_) = found("less") else { return };
    let dir = Scratch::new("lw");
    let file = dir.numbered("n.txt");
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    let env = dir.env(&[("LESSHISTFILE", "-")]);
    let mut u = Under::start(&mut t, &less_on(&file), env);
    u.until("less shows the file with the mouse on", |t, _| {
        screen(t)[0] == "line 01" && t.mouse_tracking() != MouseTracking::Off
    });
    u.at(cell!(4, 10), NONE);
    u.take_sent();
    u.input(Input::Steps(3.0), NONE);
    assert_eq!(
        String::from_utf8_lossy(&u.take_sent()),
        "\x1b[<65;11;5M".repeat(3),
        "three presses of button 5"
    );
    u.until("less scrolled a line a report", |t, _| {
        screen(t)[0] == "line 04"
    });
    u.input(Input::Steps(-2.0), NONE);
    u.until("and back two", |t, _| screen(t)[0] == "line 02");
}

/// The rows a pane prints and then holds, waiting on its terminal, for a
/// multiplexer to show: `row 001` to `row 200`.
const ROWS: &str =
    "i=1; while [ $i -le 200 ]; do printf 'row %03d\\n' $i; i=$((i+1)); done; exec cat";

/// **zellij, its session the test's own**: its configuration in the
/// scratch home -- no tips, no release notes, no frames, quit on a hangup
/// -- one pane running [`ROWS`], and the session killed when the test
/// ends, through zellij in the same environment.
struct Zellij<'d> {
    dir: &'d Scratch,
    name: String,
}

impl<'d> Zellij<'d> {
    fn new(dir: &'d Scratch) -> (Zellij<'d>, String) {
        let conf = dir.path().join("c/zellij");
        std::fs::create_dir_all(&conf).expect("zellij's config dir");
        std::fs::write(
            conf.join("config.kdl"),
            concat!(
                "show_startup_tips false\n",
                "show_release_notes false\n",
                "pane_frames false\n",
                "on_force_close \"quit\"\n",
                "session_serialization false\n",
                "mouse_hover_effects false\n",
                "default_shell \"/bin/sh\"\n",
            ),
        )
        .expect("zellij's config");
        let layout = dir.path().join("layout.kdl");
        let script = ROWS.replace('\\', "\\\\").replace('"', "\\\"");
        std::fs::write(
            &layout,
            format!(
                "layout {{\n  pane command=\"/bin/sh\" {{\n    args \"-c\" \"{script}\"\n  }}\n}}\n"
            ),
        )
        .expect("the layout");
        let name = format!("bp{}", std::process::id());
        let cmd = format!(
            "ZELLIJ_CONFIG_DIR={} exec zellij -s {name} -n {}",
            conf.display(),
            layout.display()
        );
        (Zellij { dir, name }, cmd)
    }
}

impl Drop for Zellij<'_> {
    fn drop(&mut self) {
        let _ = std::process::Command::new("zellij")
            .args(["kill-session", &self.name])
            .env_clear()
            .envs(self.dir.env(&[]))
            .output();
    }
}

/// **The wheel scrolls a zellij pane**: zellij tracks the mouse, the
/// seat's wheel reaches it as buttons 4 and 5, and zellij moves its pane
/// back over the rows the pane printed, and forward again.
#[test]
fn the_wheel_scrolls_a_zellij_pane() {
    let Some(_) = found("zellij") else { return };
    let dir = Scratch::new("zw");
    let (z, cmd) = Zellij::new(&dir);
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    let mut u = Under::start(&mut t, &cmd, dir.env(&[]));
    u.until(
        "zellij shows the pane's last rows with the mouse on",
        |t, _| {
            screen(t).iter().any(|r| r.contains("row 200"))
                && t.mouse_tracking() != MouseTracking::Off
        },
    );
    u.until("at the last rows", |t, _| screen(t)[0] == "row 178");
    u.at(cell!(10, 10), NONE);
    u.take_sent();
    u.input(Input::Steps(-3.0), NONE);
    assert_eq!(
        String::from_utf8_lossy(&u.take_sent()),
        "\x1b[<64;11;11M".repeat(3),
        "three presses of button 4"
    );
    u.until("the pane back nine rows", |t, _| screen(t)[0] == "row 169");
    u.input(Input::Steps(3.0), NONE);
    u.until("and forward again", |t, _| screen(t)[0] == "row 178");
    drop(u);
    drop(z);
}

/// **herdr, its server the test's own**: its configuration, state and
/// socket in the scratch home -- `herdr status` there reports no server
/// and a socket inside it before anything starts -- a shell of `/bin/sh`,
/// and the server stopped when the test ends, through herdr in the same
/// environment.
struct Herdr<'d> {
    dir: &'d Scratch,
}

impl Drop for Herdr<'_> {
    fn drop(&mut self) {
        let _ = std::process::Command::new("herdr")
            .args(["server", "stop"])
            .env_clear()
            .envs(self.dir.env(&[]))
            .output();
    }
}

/// **The wheel scrolls a herdr pane**: past herdr's welcome and its
/// settings, its shell prints [`ROWS`], herdr tracks the mouse, the seat's
/// wheel over the pane reaches it as buttons 4 and 5, and herdr moves the
/// pane back over the rows and forward again.
#[test]
fn the_wheel_scrolls_a_herdr_pane() {
    let Some(_) = found("herdr") else { return };
    let dir = Scratch::new("hw");
    let status = std::process::Command::new("herdr")
        .arg("status")
        .env_clear()
        .envs(dir.env(&[]))
        .output()
        .expect("herdr status");
    let status = String::from_utf8_lossy(&status.stdout).into_owned();
    let sock = dir.path().join("c").display().to_string();
    assert!(
        status.contains("status: not running") && status.contains(&sock),
        "herdr is not isolated, so it is not started: {status}"
    );
    let h = Herdr { dir: &dir };
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    let env = dir.env(&[("SHELL", "/bin/sh")]);
    let mut u = Under::start(&mut t, "exec herdr", env);
    u.until("herdr's welcome", |t, _| {
        screen(t).iter().any(|r| r.contains("continue"))
    });
    u.typed(b"\r");
    u.until("herdr's settings", |t, _| {
        screen(t).iter().any(|r| r.contains("esc close"))
    });
    u.typed(b"\x1b");
    u.until("herdr's shell", |t, _| {
        let s = screen(t);
        !s.iter().any(|r| r.contains("esc close")) && s.iter().any(|r| r.contains("$"))
    });
    u.typed(format!("{ROWS}\r").as_bytes());
    u.until("the pane's last rows", |t, _| {
        screen(t)[1].contains("row 179")
    });
    u.at(cell!(10, 50), NONE);
    u.take_sent();
    u.input(Input::Steps(-3.0), NONE);
    assert_eq!(
        String::from_utf8_lossy(&u.take_sent()),
        "\x1b[<64;51;11M".repeat(3),
        "three presses of button 4"
    );
    u.until("the pane back nine rows", |t, _| {
        screen(t)[1].contains("row 170")
    });
    u.input(Input::Steps(3.0), NONE);
    u.until("and forward again", |t, _| screen(t)[1].contains("row 179"));
    drop(u);
    drop(h);
}

/// **On the alternate screen, with nothing tracking the mouse, the wheel
/// is the cursor keys under `?1007`**, which is set at power-on: less
/// without `--mouse` is on the alternate screen, the seat sends a Down a
/// line, in the application mode less set (`SS3 B`), and less scrolls a
/// line each. With `?1007` reset before less
/// starts the wheel sends nothing and less stays where it is.
#[test]
fn the_alternate_screens_wheel_is_the_cursor_keys_to_less() {
    let Some(_) = found("less") else { return };
    let down = &b"\x1bOB\x1bOB\x1bOB"[..];
    for (before, sent, top) in [
        ("", down, "line 04"),
        ("printf '\\033[?1007l'; ", b"", "line 01"),
    ] {
        let dir = Scratch::new("la");
        let file = dir.numbered("n.txt");
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        let env = dir.env(&[("LESSHISTFILE", "-")]);
        let cmd = format!("{before}exec less {}", file.display());
        let mut u = Under::start(&mut t, &cmd, env);
        u.until("less shows the file on the alternate screen", |t, _| {
            screen(t)[0] == "line 01" && t.screen() == baud_core::Screen::Alternate
        });
        assert_eq!(u.term.mouse_tracking(), MouseTracking::Off);
        u.at(cell!(4, 10), NONE);
        let owed = u.input(Input::Steps(3.0), NONE);
        assert_eq!(u.take_sent(), sent, "{before:?}");
        match sent.is_empty() {
            false => {
                assert_eq!(owed, Owed::Keys);
                u.until("less scrolled a line a key", |t, _| screen(t)[0] == top);
            }
            true => {
                u.settle(MS * 300);
                assert_eq!(screen(u.term)[0], top, "less did not move");
            }
        }
    }
}

/// A shell that prints [`ROWS`] on the primary screen and then waits.
fn rows<'t, 's>(term: &'t mut Term<'s>, dir: &Scratch, then: &str) -> Under<'t, 's> {
    let cmd = ROWS.replace("exec cat", then);
    let mut u = Under::start(term, &cmd, dir.env(&[]));
    u.until("the rows printed", |t, _| screen(t)[22] == "row 200");
    u
}

/// **With no program tracking the mouse the wheel moves baud's own window
/// over the history**: three detents up show the rows three lines back,
/// three down the live screen again, and a key snaps a window that is
/// back to the live screen.
#[test]
fn the_wheel_moves_bauds_window_over_a_shells_history() {
    let dir = Scratch::new("sh");
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    let mut u = rows(&mut t, &dir, "exec cat");
    assert_eq!(screen(u.term)[0], "row 178");
    u.at(cell!(4, 10), NONE);
    assert_eq!(u.input(Input::Steps(-3.0), NONE), Owed::Frame);
    assert_eq!(u.take_sent(), b"", "the child hears nothing");
    assert_eq!(u.term.window_back().index(), 3);
    let s = screen(u.term);
    assert_eq!((s[0].as_str(), s[23].as_str()), ("row 175", "row 198"));
    u.input(Input::Steps(3.0), NONE);
    assert_eq!(u.term.window_back().index(), 0);
    assert_eq!(screen(u.term)[0], "row 178");
    u.input(Input::Steps(-5.0), NONE);
    assert!(u.seat.key(u.term), "a key snaps it live");
    assert_eq!(screen(u.term)[0], "row 178");
}

/// **An Option-drag selects the rectangle and a copy takes it**, the
/// block the drag spans and not the lines between its ends: Alt is
/// Option on the Linux seat, as the macOS driver's `press_of` has it, and
/// what Cmd-C or Ctrl+Shift+C hands the pasteboard is what the copy rule
/// says (`keep::copied`, through `Seat::selected_text`).
#[test]
fn an_option_drag_copies_the_rectangle() {
    let dir = Scratch::new("ob");
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    let mut u = rows(&mut t, &dir, "exec cat");
    const ALT: Modifiers = Modifiers::ALT;
    u.at(cell!(2, 2), ALT);
    u.press(Pressed::Left, ALT);
    u.at(cell!(3, 4), ALT);
    u.at(cell!(4, 5), ALT);
    assert_eq!(u.release(Pressed::Left, ALT), Owed::Selected);
    assert_eq!(u.take_sent(), b"", "the child hears none of it");
    assert_eq!(
        u.seat.selected_text(u.term).as_deref(),
        Some("w 18\nw 18\nw 18"),
        "columns 2 to 5 of rows 2 to 4: the 'w 18' of rows 180 to 182"
    );
    u.at(cell!(2, 2), NONE);
    u.press(Pressed::Left, NONE);
    u.at(cell!(4, 5), NONE);
    u.release(Pressed::Left, NONE);
    assert_eq!(
        u.seat.selected_text(u.term).as_deref(),
        Some("w 180\nrow 181\nrow 18"),
        "the plain drag's control: the lines between"
    );
}

/// **A window over the history comes back when a program has had the
/// alternate screen**: the window is moved back five lines, a program
/// takes the alternate screen -- vim, started by a write to a pipe and not
/// a key -- keys are typed into it, which leave the primary's window
/// where it is, and when vim leaves the window shows the same five-back
/// rows as before.
#[test]
fn the_window_comes_back_after_vim_has_the_alternate_screen() {
    let Some(_) = found("vim") else { return };
    let Some(_) = found("mkfifo") else { return };
    let dir = Scratch::new("aw");
    let file = dir.numbered("n.txt");
    let fifo = dir.path().join("go");
    let made = std::process::Command::new("mkfifo").arg(&fifo).status();
    assert!(made.is_ok_and(|s| s.success()), "mkfifo");
    let then = format!(
        "read x < {}; exec vim -u NONE -i NONE -N -n {}",
        fifo.display(),
        file.display()
    );
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    let mut u = rows(&mut t, &dir, &then);
    u.at(cell!(4, 10), NONE);
    u.input(Input::Steps(-5.0), NONE);
    assert_eq!(u.term.window_back().index(), 5);
    let before = screen(u.term);
    assert_eq!(before[0], "row 173");
    std::fs::write(&fifo, "go\n").expect("the pipe");
    u.until("vim on the alternate screen", |t, _| {
        t.screen() == baud_core::Screen::Alternate && screen(t)[0] == "line 01"
    });
    assert_eq!(
        u.term.window_back().index(),
        0,
        "the alternate screen is drawn live"
    );
    assert!(!u.seat.key(u.term), "a key there moves no window");
    u.typed(b"Ohello\x1b");
    u.until("vim took the keys", |t, _| screen(t)[0] == "hello");
    assert_eq!(
        u.term.primary_window_back().index(),
        5,
        "kept for the primary"
    );
    assert!(!u.seat.key(u.term));
    u.typed(b":q!\r");
    u.until("vim gone and the window back", |t, _| {
        t.screen() == baud_core::Screen::Normal && screen(t) == before
    });
    assert_eq!(u.term.window_back().index(), 5);
}

/// **The Linux driver's pointer dispatch with its thumb**: the seat asked
/// with the scrollbar beside it, the event handed to the thumb as the
/// driver's `thumb` does (`bar::event_beside`), a window the input moved
/// revealing the thumb, and then the loop's turn and the frame it draws,
/// whose look the next press is tested against.
struct Thumbed {
    bar: crate::bar::Bar,
    frame: crate::bar::Frame,
    t0: baud_domain::HostTime,
}

impl Thumbed {
    fn new() -> Thumbed {
        Thumbed {
            bar: crate::bar::Bar::new(),
            frame: crate::bar::Frame {
                size: (Px::of(800), Px::of(240)),
                cell_h: Px::of(10),
                widths: crate::reveal::widths(crate::reveal::Scale(1.0)),
            },
            t0: baud_domain::HostTime::of_secs(100.0).expect("a time"),
        }
    }

    fn now(&self, at: Duration) -> baud_domain::HostTime {
        self.t0.plus(at)
    }

    /// One input at `at`, through the seat and the thumb, and the frame the
    /// loop then draws; what the seat owed and whether the thumb took it.
    fn input(
        &mut self,
        u: &mut Under<'_, '_>,
        e: Input,
        mods: Modifiers,
        at: Duration,
    ) -> (Owed, crate::bar::Took) {
        let now = self.now(at);
        let back = u.term.primary_window_back();
        let (bar, f) = (&mut self.bar, self.frame);
        let mut took = crate::bar::Took::No;
        let mut out = Vec::new();
        let owed = u.seat.input_beside(
            u.term,
            e,
            mods,
            G,
            |b| out.extend_from_slice(b),
            |term, beside| {
                let Some(e) = crate::bar::event_beside(beside, 0.0, 1.0) else {
                    return crate::bar::Took::No;
                };
                took = bar.input(term, e, f, now).0;
                took
            },
        );
        u.sent.extend_from_slice(&out);
        u.send(&out);
        if u.term.primary_window_back() != back {
            self.bar.reveal.reveal(now);
        }
        self.draw(u.term, at);
        (owed, took)
    }

    /// The loop's turn at `at` and the frame it draws.
    fn draw(&mut self, term: &Term<'_>, at: Duration) {
        let now = self.now(at);
        self.bar.turn(now, term);
        self.bar.drawn = self.bar.look(now);
    }

    /// The knob the renderer draws for the look last drawn.
    fn knob(&self, term: &Term<'_>) -> Option<baud_render::Thumb> {
        let f = self.frame;
        let win = crate::effects::window_of(term);
        baud_render::thumb(win, f.size, f.cell_h, f.widths, self.bar.drawn.hover)
    }
}

/// **The scrollbar over a shell's history, as the pointer finds it**: the
/// wheel moves the window and reveals the knob at rest; the pointer over
/// the strip widens it, and the knob the renderer draws is the wider one;
/// a press on the knob and a drag up move the window further back, a
/// press in the strip above the knob jumps to the top of the history, and
/// none of it reaches the shell or makes a selection; the pointer leaving
/// and the hold and fade running out hide it again.
#[test]
fn the_scrollbar_takes_the_wheel_the_hover_a_drag_and_a_jump() {
    use crate::bar::Took;
    use baud_render::Hover;
    let dir = Scratch::new("sb");
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    let mut u = rows(&mut t, &dir, "exec cat");
    let mut b = Thumbed::new();
    let widths = b.frame.widths;
    let retained = crate::effects::window_of(u.term).retained.index();
    assert!(retained > 100, "the history behind the screen: {retained}");
    b.input(&mut u, Input::Entered(400.0, 100.0), NONE, Duration::ZERO);
    assert_eq!(
        b.bar.drawn.shown.get(),
        0,
        "nothing drawn at the live screen"
    );
    b.input(&mut u, Input::Steps(-3.0), NONE, MS * 10);
    assert_eq!(u.term.window_back().index(), 3);
    assert!(b.bar.drawn.shown.get() > 0, "the wheel revealed it");
    assert_eq!(b.bar.drawn.hover, Hover::Rest);
    let rest = b.knob(u.term).expect("a knob");
    assert_eq!(rest.width, widths.rest);
    b.input(&mut u, Input::Moved(795.0, 100.0), NONE, MS * 20);
    assert_eq!(b.bar.drawn.hover, Hover::Hovered, "over the strip");
    let wide = b.knob(u.term).expect("a knob");
    assert_eq!(wide.width, widths.hovered, "the renderer draws it widened");
    assert!(wide.top.value() > 200, "near the bottom, three lines back");
    let grab = f64::from(wide.top.value() + wide.height.value() / 2);
    b.input(&mut u, Input::Moved(795.0, grab), NONE, MS * 30);
    let (_, took) = b.input(
        &mut u,
        Input::Pressed {
            button: Pressed::Left,
            time: Ms::of_millis(40),
        },
        NONE,
        MS * 40,
    );
    assert_eq!(took, Took::Yes, "the knob's press");
    b.input(&mut u, Input::Moved(795.0, grab - 60.0), NONE, MS * 50);
    let dragged = u.term.window_back().index();
    assert!(dragged > 3, "the drag up moved the window back: {dragged}");
    let (_, took) = b.input(
        &mut u,
        Input::Released {
            button: Pressed::Left,
            time: Ms::of_millis(60),
        },
        NONE,
        MS * 60,
    );
    assert_eq!(took, Took::Yes, "and its release");
    b.input(&mut u, Input::Moved(795.0, 2.0), NONE, MS * 70);
    let (_, took) = b.input(
        &mut u,
        Input::Pressed {
            button: Pressed::Left,
            time: Ms::of_millis(500),
        },
        NONE,
        MS * 80,
    );
    assert_eq!(took, Took::Yes, "the strip's press off the knob");
    b.input(
        &mut u,
        Input::Released {
            button: Pressed::Left,
            time: Ms::of_millis(520),
        },
        NONE,
        MS * 90,
    );
    assert_eq!(u.term.window_back().index(), retained, "jumped to the top");
    assert_eq!(screen(u.term)[0], "row 001", "the history's first line");
    assert_eq!(u.take_sent(), b"", "the shell heard none of it");
    assert_eq!(
        u.seat.selected_text(u.term),
        None,
        "and nothing is selected"
    );
    b.input(&mut u, Input::Left, NONE, MS * 100);
    b.draw(u.term, MS * 10_000);
    assert_eq!(b.bar.drawn.shown.get(), 0, "held, faded and gone");
}

/// **The legacy scroller's gutter is never cells**, the macOS driver's
/// rule (`bar::press`, which its `scrollbar` handler calls with AppKit's
/// events): under vim, which tracks the mouse, a right press, a left one
/// with no history and a move in the gutter are all the gutter's
/// (`bar::press` answers `Took::Yes`), so the driver passes none of them
/// on to vim, where a left press over the cells is not taken. The test
/// sends nothing to vim and reads nothing back from it.
#[test]
fn the_legacy_gutter_keeps_a_press_from_a_tracking_vim() {
    use crate::bar::{At, Ctx, Held, Hold, Kind, Left, Took, press};
    let Some(_) = found("vim") else { return };
    let dir = Scratch::new("lg");
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    let u = vim(&mut t, &dir);
    let b = Thumbed::new();
    let c = Ctx {
        scroller: baud_render::Scroller::Legacy,
        shown: u.term.scrollbar_shown(),
        frame: b.frame,
        drawn: baud_render::Look::HIDDEN,
        now: Some(b.now(Duration::ZERO)),
    };
    let (mut hold, mut reveal, mut place) = (
        Hold::FREE,
        crate::reveal::Reveal::HIDDEN,
        baud_render::Place::Grid,
    );
    let mut at = |term: &mut Term<'_>, x: u32, kind: Kind| {
        let e = At {
            x: Px::of(x),
            y: Px::of(100),
            kind,
        };
        press(&mut hold, &mut reveal, &mut place, term, e, c).took
    };
    for kind in [
        Kind::Pressed { left: Left::No },
        Kind::Released,
        Kind::Pressed { left: Left::Yes },
        Kind::Released,
        Kind::Moved { held: Held::No },
    ] {
        assert_eq!(at(u.term, 795, kind), Took::Yes, "{kind:?} in the gutter");
    }
    assert_eq!(
        at(u.term, 700, Kind::Pressed { left: Left::Yes }),
        Took::No,
        "off the gutter, the cells'"
    );
}
