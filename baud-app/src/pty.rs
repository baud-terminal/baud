//! The pty: a login shell on the slave, the master read on a thread on
//! macOS and polled beside the compositor's socket on Linux.
//!
//! The one place this binary calls libc: `forkpty`, `tcgetattr` and
//! `tcsetattr` in the child and on the subsidiary `ptsname` names,
//! `execve`, `read`, `write`, `poll`, `fcntl`, the `TIOCSWINSZ` ioctl,
//! `mmap` for the font files, CoreGraphics for the display's scale before
//! AppKit exists, and
//! CoreFoundation and `newlocale` for a shell whose environment names no
//! locale, and on Linux `newlocale` for the Compose search. On macOS the
//! reader thread
//! owns the master's read side for the life of the process and pushes
//! every `read(2)` -- 4096 bytes at a time, which is what `N_TTY_BUF_SIZE`
//! and the Darwin clist hand back anyway -- to the main thread over a
//! channel; on Linux the main loop waits on the master itself and takes
//! each read where it is polled.
//!
//! **The master is non-blocking**, because the main thread writes it. A
//! write the slave's input queue cannot hold -- Darwin's takes 1022 bytes
//! and no more until the child reads -- waited in `write(2)` until the child
//! read, which held the window's thread for as long as the child did not:
//! a stopped job, or one busy elsewhere. Now the kernel takes what fits, an
//! [`Outbox`] keeps the rest in order, and the next pump offers it again.
//! The read side waits in `poll` instead of in `read`, on whichever thread
//! reads.
#![deny(unsafe_op_in_unsafe_fn)]

#[cfg(target_os = "macos")]
use baud_domain::HostTime;
use baud_domain::{InputEncoding, WinSize};
use baud_font::Px;
use std::ffi::{CString, OsString};
use std::os::fd::AsRawFd;
#[cfg(target_os = "linux")]
use std::os::fd::BorrowedFd;
use std::os::unix::ffi::OsStrExt;
#[cfg(target_os = "macos")]
use std::sync::Arc;
#[cfg(target_os = "macos")]
use std::sync::atomic::{AtomicBool, Ordering};
#[cfg(target_os = "macos")]
use std::sync::mpsc::{Receiver, Sender, channel};
#[cfg(target_os = "linux")]
use std::time::Duration;

/// The master's file descriptor, owned here.
pub struct Pty {
    master: crate::sys::Fd,
    /// The process the pty was spawned with, the session's root: the
    /// shell, or what `BAUD_CMD` names. `None` for a test's pty over a
    /// child it forked itself.
    child: Option<crate::sys::Pid>,
    /// The size the kernel was last told, so a `resize` that changes nothing
    /// sends no `SIGWINCH`: the app calls it on every re-lay, including the
    /// ones where only the pixels could have moved.
    told: crate::sys::Winsize,
    /// `IUTF8`, kept with the input encoding after the spawn.
    line: LineDiscipline,
    #[cfg(target_os = "macos")]
    rx: Receiver<Read>,
    /// Set by the reader when it has woken the main thread and not yet
    /// been drained, so a flood of reads sends one wake, not one a read.
    #[cfg(target_os = "macos")]
    signalled: Arc<AtomicBool>,
    /// The write end of the pipe the reader waits on beside the master:
    /// dropped with the window's `Pty`, it ends the reader, which closes
    /// its copy of the master (`Reading`). `None` when the reader shares this
    /// `Pty`'s descriptor, which then stays open for the life of the
    /// process.
    #[cfg(target_os = "macos")]
    closing: Option<std::io::PipeWriter>,
}

/// **A window's close hangs up its shell**, as the process's exit always
/// has: the master's last close is a lost carrier, `ptcclose` calls the
/// line discipline's modem routine with the carrier off (XNU's
/// `tty_dev.c` 581, `[[source: xnu-tty-dev]]`), and `ttymodem` sends
/// `SIGHUP` to the session's leader, the shell (`tty.c` 2041-2042,
/// `[[source: xnu-tty]]`). On macOS the reader's copy is the other
/// reference, and the pipe this `Pty` holds ends it after this.
impl Drop for Pty {
    fn drop(&mut self) {
        #[cfg(target_os = "macos")]
        if self.closing.is_none() {
            return;
        }
        // SAFETY: the descriptor is this `Pty`'s, open since the fork, and
        // closed only here: the reader holds a copy of its own.
        drop(unsafe { <std::os::fd::OwnedFd as std::os::fd::FromRawFd>::from_raw_fd(self.master) });
    }
}

/// Whether the child is still on the other end of the pty.
#[cfg(target_os = "macos")]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Stream {
    Open,
    /// The reader saw end of file: the child exited.
    Closed,
}

/// One `read(2)` of the master, and when it returned.
///
/// The stamp is taken on the reader thread the moment the read returns,
/// before the bytes are copied or the main thread is woken, so it is the
/// earliest moment this process knows the child answered -- the `echo` leg
/// of the keystroke instrument (`latency.rs`), which is why it rides in the
/// channel beside the bytes rather than being read on the main thread when
/// the chunk is drained. Named `Read` rather than `Chunk`, which is the Linux
/// loop's enum for the same read without the stamp.
#[cfg(target_os = "macos")]
pub struct Read {
    pub at: HostTime,
    pub bytes: Vec<u8>,
}

/// What a drain found: the chunks that arrived, and whether more can.
#[cfg(target_os = "macos")]
pub struct Drained {
    pub chunks: Vec<Read>,
    pub stream: Stream,
}

/// What `read(2)` delivers at most per call, the tty's own quantum.
const READ_CHUNK: usize = 4096;

/// Whether a write reached the child whole, in part, or not at all.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Wrote {
    /// The kernel took every byte owed, and the outbox is empty.
    All,
    /// The kernel took some of it and not all: the slave's input queue
    /// filled because the child is not reading. The rest is still in the
    /// outbox, first in line for the next write. It was lost until
    /// 2026-10-09.
    Part,
    /// The kernel took none of it: the queue is full, or the child is gone,
    /// and everything owed is still owed. The activity lights' SD stays
    /// dark for it, since nothing was sent.
    Nothing,
}

/// Bytes owed to the child that the kernel has not taken yet, oldest first.
///
/// **Every write to the child goes through one**, so nothing overtakes what
/// is already owed: a key cannot land between the halves of a mouse report
/// the kernel split, and a paste cannot jump the tail of a wheel burst. It is
/// unbounded, and only what the user does fills it -- keys, a paste, the
/// wheel, and the core's answers to the child's own queries -- so a child
/// that stops reading costs memory in proportion to what was typed at it,
/// where it used to cost the window.
#[derive(Default)]
pub struct Outbox {
    bytes: Vec<u8>,
}

impl Outbox {
    /// Owe `b` too, after everything already owed.
    pub fn push(&mut self, b: &[u8]) {
        self.bytes.extend_from_slice(b);
    }

    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }

    /// Whether a wait should wake for room to write.
    #[cfg(target_os = "linux")]
    pub fn owed(&self) -> Owed {
        match self.bytes.is_empty() {
            true => Owed::Nothing,
            false => Owed::Bytes,
        }
    }
}

/// What one `read(2)` of the master found.
enum Got<'a> {
    /// What the child wrote since the last read.
    Bytes(&'a [u8]),
    /// Nothing yet: the master is non-blocking and the child has written
    /// nothing since the last read, or a signal cut the read short. Neither
    /// is the end of the stream.
    Nothing,
    /// The child has gone: end of file on Darwin, `EIO` on Linux.
    Closed,
}

/// One `read(2)` of the master into `buf`, shared by the macOS reader thread
/// and the Linux loop so the two cannot disagree about which answer ends the
/// stream.
fn read_master(fd: crate::sys::Fd, buf: &mut [u8]) -> Got<'_> {
    // SAFETY: an open descriptor and a buffer valid for its length.
    let n = unsafe { crate::sys::read(fd, buf.as_mut_ptr().cast(), buf.len()) };
    match usize::try_from(n) {
        Ok(0) => Got::Closed,
        Ok(n) => Got::Bytes(&buf[..n.min(buf.len())]),
        // Every failed read was `Closed` while the master blocked, which
        // could not answer `EAGAIN`; a non-blocking one answers it whenever
        // the child is quiet.
        Err(_) => match std::io::Error::last_os_error().kind() {
            std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted => Got::Nothing,
            _ => Got::Closed,
        },
    }
}

/// What a wait on the master for something to read ended with.
#[cfg(any(target_os = "macos", test))]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Waited {
    /// Something to read, or a hang-up -- the read that follows says which.
    Ready,
    /// `poll` answered `POLLNVAL`: it cannot watch this descriptor, and every
    /// wait would return at once, so waiting again would spin.
    Unwatchable,
    /// `poll` itself failed, which a retry would only repeat.
    Failed,
    /// The window's `Pty` dropped the pipe's write end: its window closed.
    #[cfg(target_os = "macos")]
    Closing,
}

/// Waits, as long as it takes, until the master has something to read or
/// has hung up: the macOS reader's wait, now that a read returns at once.
#[cfg(any(target_os = "macos", test))]
fn readable(fd: crate::sys::Fd) -> Waited {
    let mut p = crate::sys::PollFd {
        fd,
        events: crate::sys::POLLIN,
        revents: 0,
    };
    loop {
        // SAFETY: one initialised record and its count; the kernel writes
        // only `revents`, and the record outlives the call.
        if unsafe { crate::sys::poll(&mut p, 1, -1) } >= 0 {
            return match p.revents & crate::sys::POLLNVAL {
                0 => Waited::Ready,
                _ => Waited::Unwatchable,
            };
        }
        // A signal, or `EAGAIN` -- which Darwin's page gives for "Allocation
        // of internal data structures fails. A subsequent request may
        // succeed." -- is waited again; anything else would only repeat.
        match std::io::Error::last_os_error().kind() {
            std::io::ErrorKind::Interrupted | std::io::ErrorKind::WouldBlock => {}
            _ => return Waited::Failed,
        }
    }
}

/// [`readable`], or until `closing` -- the read end of the pipe a
/// window's [`Pty`] holds the other end of -- is readable or hung up,
/// which only the window's close makes it.
#[cfg(target_os = "macos")]
fn readable_unless(fd: crate::sys::Fd, closing: Option<crate::sys::Fd>) -> Waited {
    let Some(closing) = closing else {
        return readable(fd);
    };
    let mut p = [
        crate::sys::PollFd {
            fd,
            events: crate::sys::POLLIN,
            revents: 0,
        },
        crate::sys::PollFd {
            fd: closing,
            events: crate::sys::POLLIN,
            revents: 0,
        },
    ];
    loop {
        // SAFETY: two initialised records and their count; the kernel
        // writes only `revents`, and the records outlive the call.
        if unsafe { crate::sys::poll(p.as_mut_ptr(), 2, -1) } >= 0 {
            return match (p[1].revents, p[0].revents & crate::sys::POLLNVAL) {
                (0, 0) => Waited::Ready,
                (0, _) => Waited::Unwatchable,
                _ => Waited::Closing,
            };
        }
        match std::io::Error::last_os_error().kind() {
            std::io::ErrorKind::Interrupted | std::io::ErrorKind::WouldBlock => {}
            _ => return Waited::Failed,
        }
    }
}

/// Makes the master's reads and writes return rather than wait.
fn nonblocking(fd: crate::sys::Fd) -> std::io::Result<()> {
    // SAFETY: `F_GETFL` takes no third argument and `F_SETFL` an `int`; the
    // descriptor is open.
    let flags = unsafe { crate::sys::fcntl(fd, crate::sys::F_GETFL) };
    if flags < 0 {
        return Err(std::io::Error::last_os_error());
    }
    // SAFETY: as above.
    if unsafe { crate::sys::fcntl(fd, crate::sys::F_SETFL, flags | crate::sys::O_NONBLOCK) } < 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

/// Makes the master non-blocking, or says why it stays blocking -- in which
/// case a write the queue cannot hold waits in the kernel again, as every
/// write did before this.
fn make_nonblocking(fd: crate::sys::Fd) {
    if let Err(e) = nonblocking(fd) {
        crate::warn!("pty: the master stays blocking, so a full queue waits: {e}");
    }
}

/// Makes the master's reads and writes wait again: the reader's way back
/// when `poll` cannot watch the master, to the blocking reads and writes
/// every version before this one had.
#[cfg(target_os = "macos")]
fn blocking(fd: crate::sys::Fd) -> std::io::Result<()> {
    // SAFETY: as in `nonblocking`.
    let flags = unsafe { crate::sys::fcntl(fd, crate::sys::F_GETFL) };
    if flags < 0 {
        return Err(std::io::Error::last_os_error());
    }
    // SAFETY: as above.
    if unsafe { crate::sys::fcntl(fd, crate::sys::F_SETFL, flags & !crate::sys::O_NONBLOCK) } < 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

/// Writes as much of `out` to `fd` as the kernel takes now, takes that off
/// the front, and says whether any is left.
fn write_some(fd: crate::sys::Fd, out: &mut Outbox) -> Wrote {
    let mut at = 0usize;
    while at < out.bytes.len() {
        let rest = &out.bytes[at..];
        // SAFETY: the slice is valid for its length and the fd is ours.
        let n = unsafe { crate::sys::write(fd, rest.as_ptr().cast(), rest.len()) };
        match usize::try_from(n) {
            Ok(0) => break,
            Ok(n) => at += n,
            Err(_) => match std::io::Error::last_os_error().kind() {
                // A signal cut it short: write again.
                std::io::ErrorKind::Interrupted => {}
                // `EAGAIN`: the queue is full until the child reads. Anything
                // else: the child is gone, and the reader is about to say so.
                _ => break,
            },
        }
    }
    out.bytes.drain(..at);
    match (out.bytes.is_empty(), at) {
        (true, _) => Wrote::All,
        (false, 0) => Wrote::Nothing,
        (false, _) => Wrote::Part,
    }
}

/// The shell when `$SHELL` is unset: the system's.
#[cfg(target_os = "macos")]
const DEFAULT_SHELL: &str = "/bin/zsh";
#[cfg(not(target_os = "macos"))]
const DEFAULT_SHELL: &str = "/bin/sh";

/// The variables a terminal sets to name itself to what runs inside it,
/// so a program can pick the terminal's own features -- and the ones a
/// multiplexer sets to say it sits in between. baud's shell is in none of
/// them: launched from Terminal.app it would otherwise tell every child it
/// *is* Terminal.app, and on 2026-09-22 that is what happened. yazi read
/// the inherited `TERM_PROGRAM=Apple_Terminal`, decided the terminal was
/// Apple's -- a brand with no image support in its table -- and fell back
/// to chafa without ever reading baud's own device attributes; chafa read
/// the same variable and drew in 240 colours rather than truecolour.
///
/// Three entries are baud's own: shell integration's
/// [`crate::shell::HELPERS`].
///
/// The list is every variable yazi 26.5.6's `Brand::from_env` reads
/// (`yazi-emulator/src/brand.rs`), the rest of each named terminal's own
/// family, the multiplexers yazi's `Mux` consults, and the three that
/// describe the outer terminal to any program: `COLORFGBG` (its colours,
/// not baud's), `WINDOWID` (its X window) and `TERMINFO` (its terminfo
/// directory, which ncurses searches *alone* when the variable is set, so
/// a Ghostty- or kitty-provided one hides the `xterm-256color` entry baud
/// asks for). A variable a user sets in their own rc is set after this and
/// is theirs.
const OUTER: &[&str] = &[
    // Apple Terminal.
    "TERM_SESSION_ID",
    // iTerm2.
    "ITERM_SESSION_ID",
    "ITERM_PROFILE",
    "LC_TERMINAL",
    "LC_TERMINAL_VERSION",
    "TERM_FEATURES",
    // kitty.
    "KITTY_WINDOW_ID",
    "KITTY_PID",
    "KITTY_PUBLIC_KEY",
    "KITTY_INSTALLATION_DIR",
    "KITTY_LISTEN_ON",
    // WezTerm.
    "WEZTERM_EXECUTABLE",
    "WEZTERM_EXECUTABLE_DIR",
    "WEZTERM_PANE",
    "WEZTERM_UNIX_SOCKET",
    "WEZTERM_CONFIG_DIR",
    "WEZTERM_CONFIG_FILE",
    // Ghostty.
    "GHOSTTY_RESOURCES_DIR",
    "GHOSTTY_BIN_DIR",
    "GHOSTTY_SHELL_FEATURES",
    // Alacritty.
    "ALACRITTY_WINDOW_ID",
    "ALACRITTY_SOCKET",
    "ALACRITTY_LOG",
    // Konsole.
    "KONSOLE_VERSION",
    "KONSOLE_DBUS_SERVICE",
    "KONSOLE_DBUS_SESSION",
    "KONSOLE_DBUS_WINDOW",
    "SHELL_SESSION_ID",
    // VTE-based terminals.
    "VTE_VERSION",
    // Windows Terminal.
    "WT_SESSION",
    "WT_PROFILE_ID",
    // Warp.
    "WARP_HONOR_PS1",
    "WARP_IS_LOCAL_SHELL_SESSION",
    "WARP_USE_SSH_WRAPPER",
    // VS Code's integrated terminal.
    "VSCODE_INJECTION",
    "VSCODE_SHELL_INTEGRATION",
    // Tabby.
    "TABBY_CONFIG_DIRECTORY",
    // JetBrains' terminal.
    "TERMINAL_EMULATOR",
    // Multiplexers.
    "TMUX",
    "TMUX_PANE",
    "STY",
    "WINDOW",
    "ZELLIJ",
    "ZELLIJ_SESSION_NAME",
    "ZELLIJ_PANE_ID",
    // baud's own: what shell integration hands a shell's startup to put
    // back, and its marker, none of them a parent's to pass on.
    crate::shell::HELPERS[0],
    crate::shell::HELPERS[1],
    crate::shell::HELPERS[2],
    // The outer terminal's colours, window and terminfo.
    "COLORFGBG",
    "WINDOWID",
    "TERMINFO",
];

/// What baud says about itself, replacing whatever the parent said.
/// `TERM_PROGRAM` is the convention every macOS terminal keeps --
/// `Apple_Terminal`, `iTerm.app`, `ghostty`, `WezTerm` -- and a name no
/// program's table knows is the honest answer: yazi then decides from
/// baud's own replies, and chafa from `COLORTERM`.
const OURS: [(&str, &str); 4] = [
    // The entry whose `flash` length the bell reads, spelled once there.
    ("TERM", crate::bell::TERM),
    ("COLORTERM", "truecolor"),
    ("TERM_PROGRAM", "baud"),
    ("TERM_PROGRAM_VERSION", env!("CARGO_PKG_VERSION")),
];

/// The variables a shell reads its locale from: `LC_ALL` outranks every
/// category, `LC_CTYPE` sets the one that decides how input bytes become
/// characters, and `LANG` stands under both.
const LOCALE: [&str; 3] = ["LC_ALL", "LC_CTYPE", "LANG"];

/// **Whether the parent names a locale**: one of [`LOCALE`] set and not
/// empty -- POSIX reads an empty one as unset. Other `LC_` names do not
/// count: each sets one category and leaves the character type to `LANG`,
/// and iTerm2's `LC_TERMINAL` is a terminal's name, not a locale's. The one
/// test both [`spawn_env`] and [`child_env`] ask, so the two cannot
/// disagree about when a locale is filled.
fn names_a_locale(parent: &[(OsString, OsString)]) -> bool {
    parent
        .iter()
        .any(|(k, v)| !v.is_empty() && LOCALE.iter().any(|l| l.as_bytes() == k.as_bytes()))
}

/// The child's environment: the parent's, less every variable in
/// [`OUTER`] and every one baud sets, then baud's own in [`OURS`] -- and
/// `LANG` set to `locale` when the parent names no locale
/// ([`names_a_locale`]), replacing an empty one rather than adding a second
/// that `getenv`, which reads the first, would never see. Built in the
/// parent, before the fork, so the child runs no `putenv` between `fork`
/// and `exec`: it may reallocate the environment array in a process that
/// was multi-threaded a moment ago, and it is not async-signal-safe. The
/// child runs only [`utf8_erase`], the `chdir` [`start_in`] decided, a
/// test of what it returned, the exec and, if the exec fails, `_exit`.
pub(crate) fn child_env(parent: Vec<(OsString, OsString)>, locale: Option<&str>) -> Vec<CString> {
    let fill = locale.filter(|_| !names_a_locale(&parent));
    let dropped = |k: &[u8]| {
        OUTER.iter().any(|o| o.as_bytes() == k)
            || OURS.iter().any(|(o, _)| o.as_bytes() == k)
            || (fill.is_some() && k == b"LANG")
    };
    let mut env: Vec<CString> = parent
        .into_iter()
        .filter(|(k, _)| !k.is_empty() && !dropped(k.as_bytes()))
        .filter_map(|(k, v)| {
            let mut entry = k.as_bytes().to_vec();
            entry.push(b'=');
            entry.extend_from_slice(v.as_bytes());
            CString::new(entry).ok()
        })
        .collect();
    env.extend(
        OURS.iter()
            .filter_map(|(k, v)| CString::new(format!("{k}={v}")).ok()),
    );
    env.extend(fill.and_then(|l| CString::new(format!("LANG={l}")).ok()));
    env
}

/// **The environment a shell is spawned with**: [`child_env`], with a
/// locale asked of `locale` only when the parent names none. A Dock launch
/// inherits launchd's environment, which has no locale, and zsh in the C
/// locale reads each byte of a UTF-8 character as a character of its own:
/// typed Hebrew came out as `U+FFFD` and `<0099>`
/// (`docs/research/notes/aa-shell-locale.md`). On macOS the answer is
/// CoreFoundation's, and a launch from a terminal, which passes its `LANG`
/// on, never pays for it.
fn spawn_env(
    parent: Vec<(OsString, OsString)>,
    locale: impl FnOnce() -> Option<String>,
) -> Vec<CString> {
    let found = (!names_a_locale(&parent)).then(locale).flatten();
    child_env(parent, found.as_deref())
}

/// **The names a shell's locale is tried as**, in order, each as UTF-8 and
/// each once: the user's identifier without its `@` keywords (a region
/// override, `en_US@rg=ilzzzz`, is no part of a POSIX name), their language
/// and region (`zh_CN` for `zh-Hans_CN`), then `en_US`, then `C`. Language
/// and region can name a locale the system lacks -- English in Israel,
/// `en_IL` -- which is why each name is checked rather than trusted.
#[cfg(target_os = "macos")]
fn locale_candidates(
    identifier: Option<&str>,
    language: Option<&str>,
    region: Option<&str>,
) -> Vec<String> {
    fn given(s: Option<&str>) -> Option<&str> {
        s.filter(|s| !s.is_empty())
    }
    let own = given(identifier)
        .map(|id| id.split_once('@').map_or(id, |(base, _)| base))
        .filter(|base| !base.is_empty())
        .map(str::to_string);
    let pair = given(language)
        .zip(given(region))
        .map(|(l, r)| format!("{l}_{r}"));
    let mut names: Vec<String> = Vec::new();
    for base in own
        .into_iter()
        .chain(pair)
        .chain(["en_US".to_string(), "C".to_string()])
    {
        let name = format!("{base}.UTF-8");
        if !names.contains(&name) {
            names.push(name);
        }
    }
    names
}

/// The first of `candidates` that `valid` accepts.
#[cfg(target_os = "macos")]
fn first_valid(candidates: &[String], valid: impl Fn(&str) -> bool) -> Option<String> {
    candidates.iter().find(|c| valid(c)).cloned()
}

/// **Whether the system has `name` as a UTF-8 locale**: `newlocale` loads
/// it for every category and its character set reads `UTF-8`. Every
/// category, because `LANG` feeds them all and a shell's
/// `setlocale(LC_ALL, "")` fails whole when one cannot load -- the
/// character type alone accepts `UTF-8`, which the whole mask refuses. The
/// character set, because `C`, `POSIX` and a bare `en_US` all load and are
/// not UTF-8. An empty name is refused first: `newlocale` reads it as the
/// environment's own locale, the one being replaced. Measured on macOS 27
/// (`scripts/locale/validate_probe.c`).
#[cfg(target_os = "macos")]
fn is_utf8_locale(name: &str) -> bool {
    use crate::sys::{CODESET, LC_ALL_MASK, freelocale, newlocale, nl_langinfo_l};
    if name.is_empty() {
        return false;
    }
    let Ok(name) = CString::new(name) else {
        return false;
    };
    // SAFETY: `name` is NUL-terminated and outlives the call; a null base
    // asks for a new object.
    let locale = unsafe { newlocale(LC_ALL_MASK, name.as_ptr(), core::ptr::null_mut()) };
    if locale.is_null() {
        return false;
    }
    // SAFETY: `locale` is the live object just made, freed once here after
    // the string `nl_langinfo_l` returns for it -- which lives as long as
    // the object does -- has been read.
    unsafe {
        let set = nl_langinfo_l(CODESET, locale);
        let utf8 = !set.is_null() && std::ffi::CStr::from_ptr(set).to_bytes() == b"UTF-8";
        freelocale(locale);
        utf8
    }
}

/// **Whether the system has the locale `name`**: `newlocale` loads it
/// for every category, as libxkbcommon asks before it gives a locale its
/// Compose registry does not list the `en_US.UTF-8` table
/// (`[[source: libxkbcommon-compose-paths-c]]`), so a misspelt or
/// uninstalled one makes none.
#[cfg(target_os = "linux")]
pub fn locale_installed(name: &str) -> bool {
    use crate::sys::{LC_ALL_MASK, freelocale, newlocale};
    let Ok(name) = CString::new(name) else {
        return false;
    };
    // SAFETY: `name` is NUL-terminated and outlives the call; a null base
    // asks for a new object.
    let locale = unsafe { newlocale(LC_ALL_MASK, name.as_ptr(), core::ptr::null_mut()) };
    if locale.is_null() {
        return false;
    }
    // SAFETY: `locale` is the live object just made, freed once here.
    unsafe { freelocale(locale) };
    true
}

/// A CoreFoundation string as text: `None` for null, or for one longer than
/// 63 bytes -- a locale's identifier, language or region is a few letters.
#[cfg(target_os = "macos")]
pub(crate) fn cf_text(s: crate::sys::CfString) -> Option<String> {
    if s.is_null() {
        return None;
    }
    let mut buf = [0u8; 64];
    let size = crate::sys::CfIndex::try_from(buf.len()).ok()?;
    // SAFETY: `s` is a live string, owned by a locale that outlives the
    // call or a framework constant, and `buf` is writable for the `size`
    // bytes passed.
    let ok = unsafe {
        crate::sys::CFStringGetCString(
            s,
            buf.as_mut_ptr().cast(),
            size,
            crate::sys::CF_STRING_ENCODING_UTF8,
        )
    };
    if ok == 0 {
        return None;
    }
    Some(
        std::ffi::CStr::from_bytes_until_nul(&buf)
            .ok()?
            .to_str()
            .ok()?
            .to_string(),
    )
}

/// The user's locale identifier, language and region, as CoreFoundation
/// reads them from their preferences -- `en_US`, `en` and `US` on this Mac
/// -- whatever the environment says.
#[cfg(target_os = "macos")]
fn users_locale() -> (Option<String>, Option<String>, Option<String>) {
    use crate::sys::{
        CF_LOCALE_COUNTRY_CODE, CF_LOCALE_LANGUAGE_CODE, CFLocaleCopyCurrent,
        CFLocaleGetIdentifier, CFLocaleGetValue, CFRelease,
    };
    // SAFETY: `CFLocaleCopyCurrent` has no preconditions and returns a
    // reference this function owns, checked for null and released once;
    // the strings the `Get` calls return belong to the locale and are
    // copied out before it is released. The keys are framework constants,
    // read as the pointers they are.
    unsafe {
        let locale = CFLocaleCopyCurrent();
        if locale.is_null() {
            return (None, None, None);
        }
        let got = (
            cf_text(CFLocaleGetIdentifier(locale)),
            cf_text(CFLocaleGetValue(locale, CF_LOCALE_LANGUAGE_CODE)),
            cf_text(CFLocaleGetValue(locale, CF_LOCALE_COUNTRY_CODE)),
        );
        CFRelease(locale.object());
        got
    }
}

/// **The locale a shell is given when its environment names none**: the
/// user's, as CoreFoundation reads their preferences, if the system has it
/// as UTF-8, else the first fallback that is ([`locale_candidates`]);
/// `None` when none is, which leaves the environment as it was. What it
/// cost, and that it was asked at all, print under `BAUD_STARTUP`.
#[cfg(target_os = "macos")]
fn shell_locale() -> Option<String> {
    let asked = std::time::Instant::now();
    let (identifier, language, region) = users_locale();
    let found = first_valid(
        &locale_candidates(
            identifier.as_deref(),
            language.as_deref(),
            region.as_deref(),
        ),
        is_utf8_locale,
    );
    crate::host::startup_note(&format!(
        "locale: {} in {:.3} ms",
        found.as_deref().unwrap_or("none"),
        asked.elapsed().as_secs_f64() * 1e3
    ));
    found
}

/// On Linux the parent's environment passes on as it is, as it does from
/// every terminal surveyed (note AA, section 3); whether baud should fill
/// an unset `LANG` there is open.
#[cfg(target_os = "linux")]
fn shell_locale() -> Option<String> {
    None
}

/// **What the child runs**: the path `execve` is given and the argument
/// list, decided in the parent before the fork, with what shell
/// integration changes in the environment.
struct Exec {
    path: CString,
    argv: Vec<CString>,
    injection: Option<crate::shell::Injection>,
}

impl Exec {
    /// `/bin/sh -c cmd` for a command, as `BAUD_CMD` runs one; otherwise
    /// a login shell, `$SHELL` as `parent` has it (else the system's) with
    /// argv[0] its name after a `-`, as `login` starts one, and with
    /// whatever `integration` adds to its start
    /// ([`crate::shell::inject`]). `None` when an argument cannot be a C
    /// string.
    fn of(
        cmd: Option<String>,
        parent: &[(OsString, OsString)],
        integration: crate::shell::Integration,
    ) -> Option<Exec> {
        if let Some(c) = cmd {
            return Some(Exec {
                path: CString::new("/bin/sh").ok()?,
                argv: vec![
                    CString::new("sh").ok()?,
                    CString::new("-c").ok()?,
                    CString::new(c).ok()?,
                ],
                injection: None,
            });
        }
        let shell = login_shell(parent);
        let path = resolve(&shell).unwrap_or_else(|| DEFAULT_SHELL.to_string());
        let argv0 = CString::new(format!("-{}", shell.rsplit('/').next().unwrap_or("sh"))).ok()?;
        let injection = crate::shell::inject(integration, &shell, &path, parent);
        let argv = core::iter::once(argv0)
            .chain(injection.iter().flat_map(|i| i.args.iter().cloned()))
            .collect();
        Some(Exec {
            path: CString::new(path).ok()?,
            argv,
            injection,
        })
    }
}

/// **The shell a session starts**, `$SHELL` as `parent` has it, else the
/// system's: what [`Exec::of`] runs when no command is given.
fn login_shell(parent: &[(OsString, OsString)]) -> String {
    parent
        .iter()
        .find(|(k, _)| k.as_bytes() == b"SHELL")
        .and_then(|(_, v)| v.to_str())
        .unwrap_or(DEFAULT_SHELL)
        .to_string()
}

/// **The name of the shell a session starts**, [`login_shell`]'s last
/// path component from this process's environment: the badge's `shell`,
/// as iTerm2 names the user's shell by its last path component
/// (`bestGuessAtUserShellWithPath:NO`, `[[source:
/// iterm2-964a75d-ptysession]]`).
pub(crate) fn shell_name() -> String {
    let shell = login_shell(&std::env::vars_os().collect::<Vec<_>>());
    shell.rsplit('/').next().unwrap_or(&shell).to_string()
}

/// `prog` as a path `execve` can take: itself when it names a path, else
/// the first executable of that name on `PATH` -- the search `execvp` would
/// have done, done here because the child must not allocate.
fn resolve(prog: &str) -> Option<String> {
    use std::os::unix::fs::PermissionsExt;
    if prog.contains('/') {
        return Some(prog.to_string());
    }
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join(prog))
        .find(|p| {
            std::fs::metadata(p).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
        })
        .and_then(|p| p.into_os_string().into_string().ok())
}

/// The `winsize` for a grid of `cells` whose cells are `cell_w` by `cell_h`
/// device pixels: the cell counts, and **the text area** in pixels.
///
/// The text area and not the window, because a child reads `ws_xpixel /
/// ws_col` as the width of a cell -- `[[source: yazi-dimension]]` does exactly
/// that -- and the window's width carries the padding and the part of a cell
/// the grid does not fill. `[[source: inband-resize]]` defines the same
/// quantity for the in-band report, "Text area does not include any padding
/// the terminal applies to the window", and `CSI 14 t` answers with it, so
/// all three agree. Device pixels, because that is what every render path
/// draws a Sixel pixel as. Saturating at the `u16` the kernel's field holds.
pub fn text_area(cells: &WinSize, cell_w: Px, cell_h: Px) -> crate::sys::Winsize {
    let span = |cell: Px, n: u16| {
        u16::try_from(u64::from(cell.value()) * u64::from(n)).unwrap_or(u16::MAX)
    };
    crate::sys::Winsize {
        ws_row: cells.ws_row,
        ws_col: cells.ws_col,
        ws_xpixel: span(cell_w, cells.ws_col),
        ws_ypixel: span(cell_h, cells.ws_row),
    }
}

/// Sends `ws` to the kernel through `master`, unless `told` says the kernel
/// already holds it -- a `TIOCSWINSZ` is a `SIGWINCH` to the child, and a
/// shell woken for a size it already has redraws its prompt for nothing.
fn tell(master: crate::sys::Fd, told: &mut crate::sys::Winsize, ws: crate::sys::Winsize) {
    if ws == *told {
        return;
    }
    // SAFETY: TIOCSWINSZ takes a pointer to a winsize.
    unsafe {
        crate::sys::ioctl(master, crate::sys::TIOCSWINSZ, &ws);
    }
    *told = ws;
}

/// **Backspace erases a whole UTF-8 character**: `IUTF8` set on the
/// terminal `fd`, which the kernel's default leaves clear on Darwin and on
/// Linux. Without it a line read in canonical mode -- `cat`, `read` -- kept
/// a Hebrew letter's lead byte `d7` after Backspace
/// (`docs/research/notes/aa-shell-locale.md` section 5).
///
/// Called in the child, on descriptor 0, before the exec, so the shell's
/// first look at its terminal already has the flag: Terminal.app 2.15's
/// route (`[[source: macos-terminal-2-15-binary]]`), and foot's on the
/// subsidiary before it dups it to 0. The route only: Terminal.app
/// writes a whole `c_iflag` of its own, with `IUTF8` only when a
/// `CharacterEncoding` value is 4, `NSUTF8StringEncoding`, where this
/// changes one bit (note AA). A failed call leaves the kernel's default
/// and the shell still starts: the parent is not told (an open
/// question). After the spawn the parent keeps the bit with the input
/// encoding ([`LineDiscipline`]).
///
/// The attributes set are the ones `tcgetattr` read, with one bit changed,
/// and a failed read sets nothing: `tcsetattr`'s effect "is undefined if
/// the value of the termios structure pointed to by termios_p was not
/// derived from the result of a call to tcgetattr() on fildes"
/// (`[[source: posix-tcsetattr]]`).
///
/// # Safety
/// Only async-signal-safe calls, over the child's stack: `tcgetattr` and
/// `tcsetattr` are in XSH 2.4.3's table (`[[source: posix-xsh-chap02]]`).
/// It allocates, formats and panics nowhere, and reads no `errno`.
unsafe fn utf8_erase(fd: crate::sys::Fd) {
    let mut t = core::mem::MaybeUninit::<crate::sys::Termios>::uninit();
    let p = t.as_mut_ptr();
    // SAFETY: `p` is a termios-sized, termios-aligned block, which the
    // call fills when it succeeds.
    if unsafe { crate::sys::tcgetattr(fd, p) } != 0 {
        return;
    }
    // SAFETY: the call above succeeded, so `*p` is initialised; the flag
    // word is changed in place and nothing else is touched.
    unsafe {
        (*p).c_iflag |= crate::sys::IUTF8;
        crate::sys::tcsetattr(fd, crate::sys::TCSANOW, p);
    }
}

/// The child's terminal after `forkpty`: its `login_tty` makes the
/// subsidiary descriptors 0, 1 and 2, and 0 is the one Terminal.app asks.
const CHILD_TERMINAL: crate::sys::Fd = 0;

/// **Who started baud**, as far as where its shell starts goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Parent {
    /// launchd, which is the parent of an app `open` starts (measured), and
    /// so, inferred, of one the Dock or Finder starts, which LaunchServices
    /// starts the same way. Never read on Linux.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    Launchd,
    /// A shell, a debugger, anything else.
    Other,
}

/// launchd's process ID. launchd(8): "During boot launchd is invoked by
/// the kernel to run as the first process on the system"
/// (`[[source: macos-man-launchd]]`), and `ps -p 1` names it
/// (`[[source: aa-obs-cwdprobe-run]]`).
#[cfg(target_os = "macos")]
const LAUNCHD: u32 = 1;

impl Parent {
    /// **This process's parent, read once, in the parent, at the spawn**:
    /// launchd when `getppid()` is 1 -- Ghostty's `getppid()` test on macOS
    /// (`[[source: ghostty-d9840f3-desktop]]`). An app `open` starts has
    /// parent 1, and one run from a shell has that shell
    /// (`[[source: ai-obs-fdprobe-run]]`, and again for this change,
    /// `[[source: aa-obs-cwdprobe-run]]`).
    ///
    /// One misreading is known and kept, as Ghostty keeps it: an exiting
    /// process's children are given to "an implementation-defined system
    /// process" (`[[source: posix-exit]]`), on macOS launchd
    /// (`[[source: aa-obs-cwdprobe-run]]`), so
    /// a baud started from a shell that exited before the spawn reads as
    /// launchd's too, and its shell starts home.
    #[cfg(target_os = "macos")]
    fn of_this_process() -> Parent {
        match std::os::unix::process::parent_id() {
            LAUNCHD => Parent::Launchd,
            _ => Parent::Other,
        }
    }

    /// Not on Linux, where process 1 is init and launchd does not exist:
    /// the shell starts where baud is.
    #[cfg(not(target_os = "macos"))]
    fn of_this_process() -> Parent {
        Parent::Other
    }
}

/// **Where the child starts.**
#[derive(Debug, PartialEq, Eq)]
enum StartIn {
    /// Where baud is: the exec keeps the working directory
    /// (`[[source: posix-exec]]`), and the child changes nothing.
    Inherited,
    /// A directory changed to in the child before the exec: `HOME`, or
    /// the directory a new window inherits (`windows::inherited`).
    Dir(CString),
}

/// **Where a shell baud spawns starts**: in `HOME` when baud's parent is
/// launchd, and otherwise where baud is -- so a Dock or Finder launch,
/// whose working directory is `/`, starts in the home directory, and a
/// launch from a shell starts in that shell's directory. `HOME` is the
/// first in `parent_env`, which is the one `getenv` would read, and is
/// used only when it is absolute: an empty one names nothing, and a
/// relative one would be read against `/`, so either leaves the child where
/// baud is. Decided in the parent, before the fork, so the child calls
/// `chdir` on a string built already.
fn start_in(started_by: Parent, parent_env: &[(OsString, OsString)]) -> StartIn {
    match started_by {
        Parent::Other => StartIn::Inherited,
        Parent::Launchd => parent_env
            .iter()
            .find(|(k, _)| k.as_bytes() == b"HOME")
            .map(|(_, v)| v.as_bytes())
            .filter(|v| v.first() == Some(&b'/'))
            .and_then(|v| CString::new(v).ok())
            .map_or(StartIn::Inherited, StartIn::Dir),
    }
}

/// **The directory the shell starts in**, [`start_in`]'s answer for this
/// process: `HOME` on a Dock or Finder launch, else baud's own working
/// directory. A program's log with no name, or a relative one, goes here
/// (`hostlog.rs`), from the shell's point of view at its start.
pub fn start_dir() -> std::path::PathBuf {
    let env: Vec<(OsString, OsString)> = std::env::vars_os().collect();
    match start_in(Parent::of_this_process(), &env) {
        StartIn::Dir(home) => {
            std::path::PathBuf::from(std::ffi::OsStr::from_bytes(home.as_bytes()))
        }
        StartIn::Inherited => {
            std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("/"))
        }
    }
}

/// **The child's `PWD` names where it starts.** `PWD` "shall represent an
/// absolute pathname of the current working directory"
/// (`[[source: posix-xbd-8]]`). A child sent home would inherit a `PWD`
/// naming somewhere else -- `open` run from a shell passes that shell's on
/// to the app it starts, while the app's directory is `/`
/// (`[[source: aa-obs-cwdprobe-run]]`) -- so it is replaced with `HOME`,
/// as Ghostty sets it (note AA section 9). A
/// child that stays keeps its environment as it is.
fn with_pwd(mut env: Vec<CString>, start: &StartIn) -> Vec<CString> {
    if let StartIn::Dir(dir) = start {
        env.retain(|e| !e.as_bytes().starts_with(b"PWD="));
        let mut entry = b"PWD=".to_vec();
        entry.extend_from_slice(dir.as_bytes());
        env.extend(CString::new(entry).ok());
    }
    env
}

/// **`IUTF8` follows the input encoding after the spawn.** Under `ESC % @`
/// the keyboard sends ISO 8859-1, one byte a character (`Term::key`), and
/// with `IUTF8` still set the line discipline's erase reads a byte
/// `80`-`BF` as a continuation and takes the character before it too: `a £`
/// then Backspace left an empty line, where ISO 8859-1 leaves `a`. So the
/// parent clears the bit when the core's encoding becomes ISO 8859-1 and
/// sets it when it becomes UTF-8 again, by whatever changed it -- `ESC % @`,
/// and `ESC % G` or `RIS`, which returns the core to UTF-8.
///
/// **Through the subsidiary, not the manager.** `tcgetattr` takes "an open
/// file descriptor associated with a terminal" (`[[source:
/// posix-tcgetattr]]`), which the subsidiary is; no text read says the
/// manager is one. So the parent opens the path `ptsname` gave at the
/// spawn, reads the attributes, changes one bit, writes them back and
/// closes it before returning: it never holds the subsidiary, so the
/// child's last close is still the one that hangs the manager up. Opened
/// with `O_NOCTTY`, so the parent never takes it as its controlling
/// terminal, and `O_NONBLOCK`, so the open "shall return without blocking
/// for the device to be ready or available" (`[[source: posix-open]]`).
/// The parent runs none of this between `fork` and `exec`, so it may
/// allocate, and it says a failure rather than swallowing it.
pub struct LineDiscipline {
    /// The subsidiary's path, from `ptsname` at the spawn; `None` if it
    /// gave none, and then no change can reach the line discipline.
    subsidiary: Option<std::path::PathBuf>,
    /// The encoding the bit was last set for: UTF-8 at the spawn, because
    /// the child set it ([`utf8_erase`]).
    told: InputEncoding,
}

/// What [`LineDiscipline::follow`] did.
#[derive(Debug)]
pub enum Followed {
    /// The encoding is the one the bit was last set for.
    Same,
    /// The bit now says the new encoding.
    Told,
    /// The change did not reach the line discipline, and why.
    Failed(std::io::Error),
}

impl LineDiscipline {
    /// The line discipline of the pty whose manager is `master`, the bit as
    /// the child set it.
    fn of(master: crate::sys::Fd) -> LineDiscipline {
        LineDiscipline {
            subsidiary: subsidiary_of(master),
            told: InputEncoding::Utf8,
        }
    }

    /// Sets `IUTF8` for `enc` when `enc` is not what it was last set for,
    /// and does nothing when it is. The encoding counts as told even when
    /// the change fails, so a failure is said once, not once a read.
    pub fn follow(&mut self, enc: InputEncoding) -> Followed {
        if enc == self.told {
            return Followed::Same;
        }
        self.told = enc;
        let Some(path) = &self.subsidiary else {
            return Followed::Failed(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "ptsname named no subsidiary at the spawn",
            ));
        };
        match utf8_erase_at(path, enc) {
            Ok(()) => Followed::Told,
            Err(e) => Followed::Failed(e),
        }
    }
}

/// The subsidiary's path for the manager `master`, or `None` if
/// `ptsname` gives none.
///
/// **Under a lock**, because `ptsname` answers in a buffer of libc's that
/// the next call overwrites, and two spawns -- or two tests, which
/// `cargo test` runs as threads of one process -- could otherwise read
/// each other's name. The lock covers every call baud makes; a library in
/// this process calling `ptsname` at the same moment is not covered, and
/// none in `baud-app`'s dependency tree does.
fn subsidiary_of(master: crate::sys::Fd) -> Option<std::path::PathBuf> {
    static PTSNAME: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _held = PTSNAME
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    // SAFETY: `master` is a descriptor; the answer is null or a string
    // libc keeps until the next call, which the lock holds off until the
    // copy below is made.
    let name = unsafe { crate::sys::ptsname(master) };
    if name.is_null() {
        return None;
    }
    // SAFETY: non-null, so a NUL-terminated string, read before the guard
    // drops.
    let name = unsafe { std::ffi::CStr::from_ptr(name) };
    Some(std::path::PathBuf::from(std::ffi::OsStr::from_bytes(
        name.to_bytes(),
    )))
}

/// `IUTF8` set on the terminal at `subsidiary` for UTF-8 and cleared for
/// ISO 8859-1, every other attribute as `tcgetattr` read it; the
/// descriptor is closed before this returns, whatever happened.
fn utf8_erase_at(subsidiary: &std::path::Path, enc: InputEncoding) -> std::io::Result<()> {
    use std::os::unix::fs::OpenOptionsExt;
    let file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(crate::sys::O_NOCTTY | crate::sys::O_NONBLOCK)
        .open(subsidiary)?;
    let fd = file.as_raw_fd();
    let mut t = core::mem::MaybeUninit::<crate::sys::Termios>::uninit();
    // SAFETY: a termios-sized, termios-aligned block, which the call fills
    // when it succeeds.
    if unsafe { crate::sys::tcgetattr(fd, t.as_mut_ptr()) } != 0 {
        return Err(std::io::Error::last_os_error());
    }
    // SAFETY: the call above succeeded, so the block is initialised.
    let mut t = unsafe { t.assume_init() };
    match enc {
        InputEncoding::Utf8 => t.c_iflag |= crate::sys::IUTF8,
        InputEncoding::Latin1 => t.c_iflag &= !crate::sys::IUTF8,
    }
    // SAFETY: `t` is what `tcgetattr` read on `fd`, one bit changed.
    if unsafe { crate::sys::tcsetattr(fd, crate::sys::TCSANOW, &t) } != 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

/// **A forked child's first act is libSystem's, and a thread can be in the
/// middle of what it reads.** On macOS `fork` runs libnotify's handler in
/// the child, which reads the library's globals through a once; when
/// another thread of this process was making the process's first libnotify
/// call at the moment of the fork, the child waits on a once whose owner
/// did not cross the fork and is killed before it reaches `execve` -- the
/// shell never starts and the master reads nothing. `std::env::temp_dir()`
/// with `TMPDIR` unset is one such first call. Making one here, in the
/// parent, settles the once before every fork, or waits for the thread
/// that is settling it. Elsewhere `fork` runs no such handler.
fn settle_before_fork() {
    // SAFETY: the token check takes any int and reads libnotify's table
    // under its own lock.
    #[cfg(target_os = "macos")]
    unsafe {
        crate::sys::notify_is_valid_token(0);
    }
}

/// How many times a pty open is made before its failure stands. Each
/// `EREDRIVEOPEN` means another opener took the terminal first, and the
/// kernel has already redone the open ten times; measured with 64 threads
/// opening at once, no open took more than 4 calls. Each call that loses
/// waits out the kernel's sleeps, about 0.4 s.
const REDRIVES: usize = 16;

/// `open` -- a `forkpty` or a `posix_openpt` -- made again, at once, while
/// it fails with [`crate::sys::EREDRIVEOPEN`]: the kernel's redrive
/// continued past its own ten, as its comment has the value mean, "we've
/// been raced" (`[[source: xnu-tty-ptmx-c]]`). The race is system-wide: four
/// processes of eight threads each lost it where one alone never did. No
/// wait of baud's own: the kernel sleeps inside the call before its sixth
/// redo. A `forkpty` that fails this way has not forked, since Darwin's
/// opens the pty before it forks (`[[source: apple-libc-pty-c]]`), and any
/// other failure stands.
fn redriven(mut open: impl FnMut() -> core::ffi::c_int) -> core::ffi::c_int {
    let mut calls = 1;
    loop {
        let got = open();
        if got >= 0 || calls == REDRIVES || !raced() {
            return got;
        }
        calls += 1;
    }
}

/// Whether the last call lost the kernel's pty allocation race.
#[cfg(target_os = "macos")]
fn raced() -> bool {
    std::io::Error::last_os_error().raw_os_error() == Some(crate::sys::EREDRIVEOPEN)
}

/// Linux's pty open has no such race to lose.
#[cfg(not(target_os = "macos"))]
fn raced() -> bool {
    false
}

/// `forkpty` on a fresh pty of size `ws`, made again while it loses the
/// kernel's race ([`redriven`]): its pid, zero in the child and negative
/// when it failed, and the master in the parent. libnotify is settled
/// before it forks ([`settle_before_fork`]).
fn fork_terminal(ws: crate::sys::Winsize) -> (core::ffi::c_int, crate::sys::Fd) {
    // Darwin's `forkpty` takes the size by mutable pointer and
    // glibc's by const, so the binding is only mutated on one.
    #[cfg_attr(target_os = "linux", allow(unused_mut))]
    let mut ws = ws;
    let mut master: crate::sys::Fd = -1;
    #[cfg(target_os = "linux")]
    let ws_arg: *const crate::sys::Winsize = &raw const ws;
    #[cfg(not(target_os = "linux"))]
    let ws_arg: *mut crate::sys::Winsize = &raw mut ws;
    settle_before_fork();
    // SAFETY: forkpty is given a valid out-pointer and window size, both
    // live for the call. The caller decides what the child runs.
    let pid = redriven(|| unsafe {
        crate::sys::forkpty(
            &mut master,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            ws_arg,
        )
    });
    (pid, master)
}

impl Pty {
    /// Forks a login shell -- `$SHELL`, else the system's -- on a fresh
    /// pty of `size`, and returns the master. With `BAUD_CMD` set, forks
    /// `/bin/sh -c "$BAUD_CMD"` instead, which is how a benchmark drives
    /// the terminal: the window closes when the command ends.
    ///
    /// **The pixels go in at the fork**, not at the first resize: a program
    /// that asks the kernel how big a cell is before the window has settled
    /// -- yazi does, at startup -- would otherwise read zero, and the first
    /// resize after launch usually computes the grid the pty was spawned at,
    /// so it had nothing to send and the zero stood until the window changed.
    ///
    /// `at` is the directory a new window inherits, which wins over the
    /// launch rule ([`start_in`]) when it is absolute. `integration` is
    /// `[shell] integration`, which marks the shell's prompts
    /// (`shell.rs`).
    fn fork(
        ws: crate::sys::Winsize,
        at: Option<&std::path::Path>,
        integration: crate::shell::Integration,
    ) -> Option<(crate::sys::Pid, crate::sys::Fd)> {
        let cmd = std::env::var("BAUD_CMD").ok();
        Pty::fork_given(
            ws,
            cmd,
            std::env::vars_os().collect(),
            shell_locale,
            Parent::of_this_process(),
            at,
            integration,
        )
    }

    /// [`Pty::fork`] with every input given but the integration, which is
    /// off: the command, the parent's environment, where a locale comes
    /// from when the parent names none, and who started baud -- so a test
    /// runs the whole spawn, the locale and the start directory included.
    #[cfg(test)]
    fn fork_from(
        ws: crate::sys::Winsize,
        cmd: Option<String>,
        parent: Vec<(OsString, OsString)>,
        locale: impl FnOnce() -> Option<String>,
        started_by: Parent,
        at: Option<&std::path::Path>,
    ) -> Option<crate::sys::Fd> {
        Pty::fork_given(
            ws,
            cmd,
            parent,
            locale,
            started_by,
            at,
            crate::shell::Integration::Off,
        )
        .map(|(_, master)| master)
    }

    /// [`Pty::fork`] with every input given, the integration too.
    fn fork_given(
        ws: crate::sys::Winsize,
        cmd: Option<String>,
        parent: Vec<(OsString, OsString)>,
        locale: impl FnOnce() -> Option<String>,
        started_by: Parent,
        at: Option<&std::path::Path>,
        integration: crate::shell::Integration,
    ) -> Option<(crate::sys::Pid, crate::sys::Fd)> {
        let start = match at
            .filter(|d| d.is_absolute())
            .and_then(|d| CString::new(d.as_os_str().as_bytes()).ok())
        {
            Some(dir) => StartIn::Dir(dir),
            None => start_in(started_by, &parent),
        };
        let exec = Exec::of(cmd, &parent, integration)?;
        let mut stay = spawn_env(parent, locale);
        if let Some(i) = &exec.injection {
            i.apply(&mut stay);
        }
        let home = with_pwd(stay.clone(), &start);
        Pty::fork_in(ws, &exec, &home, &stay, &start)
    }

    /// [`Pty::fork_in`] where baud is, for the tests that hand the child a
    /// parent environment of their own and read back what it saw or the
    /// terminal it was given.
    #[cfg(test)]
    pub(crate) fn fork_with(
        ws: crate::sys::Winsize,
        cmd: Option<String>,
        env: Vec<CString>,
    ) -> Option<crate::sys::Fd> {
        Pty::fork_child(ws, cmd, env).map(|(_, master)| master)
    }

    /// [`Pty::fork_with`] with the child's pid beside the master, for a
    /// test that waits for that child: libtest runs the tests as threads of
    /// one process, so a wait for any child (`waitpid(-1)`) can take a
    /// child of a test running beside it, whose own wait then fails with
    /// `ECHILD`. The shell, when no command is given, is `$SHELL` as this
    /// process has it, started with integration off.
    #[cfg(test)]
    pub(crate) fn fork_child(
        ws: crate::sys::Winsize,
        cmd: Option<String>,
        env: Vec<CString>,
    ) -> Option<(core::ffi::c_int, crate::sys::Fd)> {
        let parent: Vec<(OsString, OsString)> = std::env::vars_os().collect();
        let exec = Exec::of(cmd, &parent, crate::shell::Integration::Off)?;
        Pty::fork_in(ws, &exec, &env, &env, &StartIn::Inherited)
    }

    /// [`Pty::fork`] with what the child runs ([`Exec`]), the environments
    /// and the start directory given rather than read: the child's pid and
    /// the master. `home` is the environment of a child whose `chdir` to
    /// `start`'s directory succeeded, and `stay` that of a child left where
    /// baud is -- because `start` said so, or because the `chdir` failed --
    /// so a `PWD` naming `HOME` reaches only a shell that is in it, as XBD 8
    /// has `PWD` name "the current working directory"
    /// (`[[source: posix-xbd-8]]`).
    fn fork_in(
        ws: crate::sys::Winsize,
        exec: &Exec,
        home: &[CString],
        stay: &[CString],
        start: &StartIn,
    ) -> Option<(core::ffi::c_int, crate::sys::Fd)> {
        let path_c = &exec.path;
        let argv: Vec<*const core::ffi::c_char> = exec
            .argv
            .iter()
            .map(|a| a.as_ptr())
            .chain(std::iter::once(std::ptr::null()))
            .collect();
        let envp_of = |env: &[CString]| -> Vec<*const core::ffi::c_char> {
            env.iter()
                .map(|e| e.as_ptr())
                .chain(std::iter::once(std::ptr::null()))
                .collect()
        };
        let envp_home = envp_of(home);
        let envp_stay = envp_of(stay);
        // `fork_terminal` returns in the child having called nothing but
        // `forkpty`: a pid of zero ends the redrive before it reads errno.
        let (pid, master) = fork_terminal(ws);
        if pid < 0 {
            return None;
        }
        if pid == 0 {
            // SAFETY: in the child only `tcgetattr`, `tcsetattr`, `chdir`,
            // `execve` and `_exit` run, and a branch on `chdir`'s result,
            // over the child's stack and pointers into memory built before
            // the fork: after a multi-threaded process forks, POSIX has "the
            // child process only executes async-signal-safe operations until
            // such time as one of the exec functions is successful"
            // (`[[source: posix-fork]]`), and XSH 2.4.3's table lists all
            // five (`[[source: posix-xsh-chap02]]`). The rule was `execve`
            // and `_exit` alone until 2026-10-13, when it was widened for
            // the two attribute calls, and then for `chdir`, once the child
            // was to change directory.
            unsafe {
                utf8_erase(CHILD_TERMINAL);
                // A failed change leaves the directory as it was (`[[source:
                // posix-chdir]]`), and the shell still starts, as Ghostty's
                // does (`[[source: ghostty-d9840f3-command]]`), with the
                // environment of a child that stayed: the choice is a test
                // of the `int` `chdir` returns, and allocates nothing.
                let envp = match start {
                    StartIn::Dir(dir) if crate::sys::chdir(dir.as_ptr()) == 0 => &envp_home,
                    _ => &envp_stay,
                };
                crate::sys::execve(path_c.as_ptr(), argv.as_ptr(), envp.as_ptr());
                crate::sys::_exit(127);
            }
        }
        // **The master closes on exec**: a program this process starts --
        // the opener a link is handed to, and what that opener starts --
        // never holds the terminal's other end. The child closed its copy
        // in `forkpty`.
        // SAFETY: an open descriptor; `F_SETFD` takes the flags as an int.
        if unsafe { crate::sys::fcntl(master, crate::sys::F_SETFD, crate::sys::FD_CLOEXEC) } < 0 {
            crate::warn!(
                "pty: the master stays open across exec: {}",
                std::io::Error::last_os_error()
            );
        }
        Some((pid, master))
    }

    /// The pty with a thread reading it: each chunk crosses a channel and
    /// `wake` runs once per batch, so a main thread that cannot wait on a
    /// descriptor -- the macOS run loop -- learns there is something to
    /// drain.
    ///
    /// `at` is the directory a new window's shell starts in (`fork`).
    #[cfg(target_os = "macos")]
    pub fn spawn(
        size: &WinSize,
        cell_w: Px,
        cell_h: Px,
        at: Option<&std::path::Path>,
        integration: crate::shell::Integration,
        wake: impl Fn() + Send + 'static,
    ) -> Option<Pty> {
        let told = text_area(size, cell_w, cell_h);
        let (child, master) = Pty::fork(told, at, integration)?;
        Some(Pty::reading(master, Some(child), told, wake))
    }

    /// The pty over `master`, a child's manager the fork told `told`,
    /// with the thread that reads it.
    #[cfg(target_os = "macos")]
    fn reading(
        master: crate::sys::Fd,
        child: Option<crate::sys::Pid>,
        told: crate::sys::Winsize,
        wake: impl Fn() + Send + 'static,
    ) -> Pty {
        // Before the reader starts, so it never sees the master block.
        make_nonblocking(master);
        let (tx, rx) = channel();
        let signalled = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&signalled);
        let (held, closing) = Reading::of(master);
        std::thread::spawn(move || reader(held, tx, flag, wake));
        Pty {
            master,
            child,
            told,
            line: LineDiscipline::of(master),
            rx,
            signalled,
            closing,
        }
    }

    /// **The app's pty over a child a test names**: [`Pty::spawn`] with the
    /// command, the parent's environment and who started baud given rather
    /// than read from this process, the locale asked as a launch asks it
    /// ([`shell_locale`]), and the reader thread, which calls `wake` once a
    /// batch -- so a test runs a real program through the spawn a Dock
    /// launch runs.
    #[cfg(all(test, target_os = "macos"))]
    pub(crate) fn spawn_given(
        size: &WinSize,
        (cell_w, cell_h): (Px, Px),
        cmd: &str,
        parent: Vec<(OsString, OsString)>,
        started_by: Parent,
        wake: impl Fn() + Send + 'static,
    ) -> Option<Pty> {
        let told = text_area(size, cell_w, cell_h);
        let master = Pty::fork_from(
            told,
            Some(cmd.into()),
            parent,
            shell_locale,
            started_by,
            None,
        )?;
        Some(Pty::reading(master, None, told, wake))
    }

    /// The pty with no thread: the caller waits on `as_fd` beside whatever
    /// else it waits on and takes each read with `read_chunk`, which is
    /// the Linux loop's shape -- one wait over the compositor's socket and
    /// the master, and nothing crossing a channel.
    #[cfg(target_os = "linux")]
    pub fn spawn_polled(
        size: &WinSize,
        cell_w: Px,
        cell_h: Px,
        integration: crate::shell::Integration,
    ) -> Option<Pty> {
        let told = text_area(size, cell_w, cell_h);
        let (child, master) = Pty::fork(told, None, integration)?;
        make_nonblocking(master);
        Some(Pty {
            master,
            child: Some(child),
            told,
            line: LineDiscipline::of(master),
        })
    }

    /// The master, to wait on.
    #[cfg(target_os = "linux")]
    pub fn as_fd(&self) -> BorrowedFd<'_> {
        // SAFETY: the master is open for as long as `self` is, and the
        // borrow cannot outlive `self`.
        unsafe { BorrowedFd::borrow_raw(self.master) }
    }

    /// **The tty's special characters**, which a paste replaces as xterm's
    /// `STTY` does (`paste::FILTER`): the `c_cc` entries
    /// [`crate::sys::STTY_CHARS`] names, as `tcgetattr` reads them on the
    /// manager; none when they cannot be read.
    pub fn specials(&self) -> crate::paste::Specials {
        let mut t = core::mem::MaybeUninit::<crate::sys::Termios>::uninit();
        // SAFETY: a termios-sized, termios-aligned block, which the call
        // fills when it succeeds.
        if unsafe { crate::sys::tcgetattr(self.master, t.as_mut_ptr()) } != 0 {
            return crate::paste::Specials::default();
        }
        // SAFETY: the call above succeeded, so the block is initialised.
        let t = unsafe { t.assume_init() };
        crate::paste::Specials::of(
            &t.c_cc,
            crate::sys::STTY_CHARS,
            crate::paste::Disabled(crate::sys::POSIX_VDISABLE),
        )
    }

    /// The tty's literal-next character, which a `?2005` paste puts
    /// before each byte: `c_cc[VLNEXT]` as `tcgetattr` reads it on the
    /// manager, or Control-V when it cannot be read or is switched off, as
    /// xterm's `get_tty_lnext` falls back to `XTERM_LNEXT`.
    pub fn literal_next(&self) -> crate::readline::Quote {
        let mut t = core::mem::MaybeUninit::<crate::sys::Termios>::uninit();
        // SAFETY: a termios-sized, termios-aligned block, which the call
        // fills when it succeeds.
        if unsafe { crate::sys::tcgetattr(self.master, t.as_mut_ptr()) } != 0 {
            return crate::readline::Quote::LNEXT;
        }
        // SAFETY: the call above succeeded, so the block is initialised.
        let t = unsafe { t.assume_init() };
        match t.c_cc[crate::sys::VLNEXT] {
            crate::sys::POSIX_VDISABLE => crate::readline::Quote::LNEXT,
            b => crate::readline::Quote(b),
        }
    }

    /// One read of the master, taken after a wait said it was ready: at
    /// most `READ_CHUNK` bytes, empty when a signal interrupted the read or
    /// the wait woke for room to write rather than for bytes, and `Closed`
    /// once the child has gone -- a Linux master answers a read with an
    /// error then, not with zero bytes.
    #[cfg(target_os = "linux")]
    pub fn read_chunk(&self) -> Chunk {
        let mut buf = vec![0u8; READ_CHUNK];
        match read_master(self.master, &mut buf) {
            Got::Bytes(b) => {
                let n = b.len();
                buf.truncate(n);
                Chunk::Bytes(buf)
            }
            Got::Nothing => Chunk::Bytes(Vec::new()),
            Got::Closed => Chunk::Closed,
        }
    }

    /// Chunks the shell wrote since the last call, in order.
    #[cfg(target_os = "macos")]
    pub fn drain(&self) -> Drained {
        // Cleared before draining, so a chunk that lands after the drain
        // begins earns its own wake.
        self.signalled.store(false, Ordering::Release);
        let mut chunks = Vec::new();
        let stream = loop {
            match self.rx.try_recv() {
                Ok(chunk) => chunks.push(chunk),
                Err(std::sync::mpsc::TryRecvError::Empty) => break Stream::Open,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => break Stream::Closed,
            }
        };
        Drained { chunks, stream }
    }

    /// Writes what `out` owes the shell, as much as the kernel takes now,
    /// and says whether all of it went. **It never waits**: what the slave's
    /// input queue cannot hold stays in `out`, first in line, and the caller
    /// offers it again on its next turn. A write that left bytes owed asks
    /// for no echo, and the keystroke instrument must not wait for one.
    pub fn write(&self, out: &mut Outbox) -> Wrote {
        write_some(self.master, out)
    }

    /// Tells the kernel, and through it the shell, the geometry in cells
    /// and the text area in pixels -- **the cell size, not the window's**, so
    /// a caller cannot hand it the padding; see [`text_area`].
    ///
    /// Called on every re-lay, and a no-op when nothing moved: a zoom that
    /// keeps the grid still changes the pixels and is sent, a drag step that
    /// lands on the same grid and cell is not, so the shell is not woken
    /// for nothing.
    pub fn resize(&mut self, size: &WinSize, cell_w: Px, cell_h: Px) {
        tell(self.master, &mut self.told, text_area(size, cell_w, cell_h));
    }

    /// **The session's root process**, as the badge's `pid` names it:
    /// "The process ID of the root process in this session"
    /// (`[[source: iterm2-doc-variables]]`).
    pub fn child(&self) -> Option<crate::sys::Pid> {
        self.child
    }

    /// **The pty's device**, as the badge's `tty` names it: "The path to
    /// the local TTY device", `ptsname`'s answer at the spawn.
    pub fn tty(&self) -> Option<&std::path::Path> {
        self.line.subsidiary.as_deref()
    }

    /// What the derived title reads of the foreground (`fg.rs`).
    pub fn reads(&self) -> crate::fg::Live {
        crate::fg::Live {
            master: self.master,
        }
    }

    /// Keeps `IUTF8` with the core's input encoding ([`LineDiscipline`]).
    /// Called after every feed and before any key is encoded, so a key
    /// `Term::key` sends as ISO 8859-1 reaches a line discipline with the
    /// bit clear. A change that fails is said once.
    pub fn follow(&mut self, enc: InputEncoding) {
        if let Followed::Failed(e) = self.line.follow(enc) {
            crate::warn!("the pty's IUTF8 stayed as it was for {enc:?}: {e}");
        }
    }
}

/// **What the reader holds of the master.** A copy of its own and the
/// read end of a pipe whose write end the window's [`Pty`] holds, so the
/// window can close without closing a descriptor the reader is waiting
/// on: a closed number can be handed to the next window's master, and the
/// reader would read that window's bytes. The pipe's close is the
/// reader's end, and the copy closes with it.
#[cfg(target_os = "macos")]
enum Reading {
    Own {
        copy: std::os::fd::OwnedFd,
        closing: std::io::PipeReader,
    },
    /// The window's own descriptor, when no copy or pipe could be made:
    /// it is never closed, as every master was before windows could close.
    Shared(crate::sys::Fd),
}

#[cfg(target_os = "macos")]
impl Reading {
    /// The reader's hold on `master`, and the write end the window keeps.
    fn of(master: crate::sys::Fd) -> (Reading, Option<std::io::PipeWriter>) {
        // SAFETY: open for as long as the window's `Pty`, which outlives
        // this call; the copy is a descriptor of its own.
        let copy = unsafe { std::os::fd::BorrowedFd::borrow_raw(master) }.try_clone_to_owned();
        match (copy, std::io::pipe()) {
            (Ok(copy), Ok((closing, close))) => (Reading::Own { copy, closing }, Some(close)),
            (Err(e), _) | (_, Err(e)) => {
                crate::warn!("pty: the master stays open until baud quits: {e}");
                (Reading::Shared(master), None)
            }
        }
    }

    fn fd(&self) -> crate::sys::Fd {
        match self {
            Reading::Own { copy, .. } => copy.as_raw_fd(),
            Reading::Shared(fd) => *fd,
        }
    }

    fn closing(&self) -> Option<crate::sys::Fd> {
        match self {
            Reading::Own { closing, .. } => Some(closing.as_raw_fd()),
            Reading::Shared(_) => None,
        }
    }
}

#[cfg(target_os = "macos")]
fn reader(held: Reading, tx: Sender<Read>, signalled: Arc<AtomicBool>, wake: impl Fn()) {
    let fd = held.fd();
    let mut buf = vec![0u8; READ_CHUNK];
    // Debug tee: with BAUD_PTYLOG set, every byte the child writes is appended
    // to that file, so a scrambled Sixel can be captured as the exact input
    // baud received and replayed through a decoder test.
    let mut log = std::env::var_os("BAUD_PTYLOG").and_then(|p| {
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(p)
            .ok()
    });
    loop {
        let bytes = match read_master(fd, &mut buf) {
            Got::Bytes(b) => b,
            // **The wait is here, in `poll`, not in the read.** The master is
            // non-blocking for the main thread's writes, and the flag is the
            // open file's, so this read returns at once with nothing. Read as
            // the end of the stream -- the rule every empty answer had while
            // the master blocked -- it would end the reader, and with it the
            // window, the first time the child paused.
            Got::Nothing => match readable_unless(fd, held.closing()) {
                Waited::Ready => continue,
                // The window closed: `held` drops here, and its copy of the
                // master with it.
                Waited::Closing => return,
                // Darwin's `poll(2)` says it does not support devices; it
                // watched the master on the Mac this was written on, and
                // where it cannot, the master goes back to blocking -- the
                // reads wait in `read` and the writes in `write`, as before
                // -- rather than this loop spinning on `POLLNVAL`.
                Waited::Unwatchable => match blocking(fd) {
                    Ok(()) => {
                        crate::warn!("pty: poll cannot watch the master; it blocks again");
                        continue;
                    }
                    Err(_) => return,
                },
                Waited::Failed => return,
            },
            Got::Closed => return,
        };
        // Stamped before anything else happens to the bytes: this is the
        // moment the child's answer reached the process.
        let at = host_now();
        if let Some(f) = log.as_mut() {
            use std::io::Write;
            let _ = f.write_all(bytes);
        }
        if tx
            .send(Read {
                at,
                bytes: bytes.to_vec(),
            })
            .is_err()
        {
            return;
        }
        // The chunk is in the channel; wake the main thread once for
        // however many arrive before it drains.
        if !signalled.swap(true, Ordering::AcqRel) {
            wake();
        }
    }
}

/// What a wait said about a descriptor. A hang-up counts as readable: the
/// read that follows is what reports it. So does room to write, when the
/// wait was told bytes are owed: the read that follows finds nothing, and
/// the flush after it writes.
#[cfg(target_os = "linux")]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Ready {
    Readable,
    Idle,
}

/// Whether the second descriptor of a [`wait`] is owed bytes it has not
/// taken, so room to write should end the wait too: the retry is scheduled
/// on the event that makes it possible, not on a timer.
#[cfg(target_os = "linux")]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Owed {
    Nothing,
    Bytes,
}

/// One read of the master.
#[cfg(target_os = "linux")]
pub enum Chunk {
    Bytes(Vec<u8>),
    Closed,
}

/// What else a [`wait`] watches a descriptor for: a clipboard transfer's
/// pipe, read from or written to a piece at a time.
#[cfg(target_os = "linux")]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Interest {
    Read,
    Write,
}

/// Waits until either descriptor is readable, one of `also` is ready for
/// what it is watched for, or `timeout` passes -- `None` waits as long as
/// it takes -- and says which of the two, and which of `also`, in its
/// order: a descriptor the kernel says nothing of is one the caller need
/// not read. A signal that cuts the wait short answers `Idle` for all,
/// so the caller simply waits again; `poll` reports a descriptor that is
/// still ready on the next wait.
#[cfg(target_os = "linux")]
pub fn wait(
    a: BorrowedFd<'_>,
    b: BorrowedFd<'_>,
    b_owed: Owed,
    also: &[(BorrowedFd<'_>, Interest)],
    timeout: Option<Duration>,
) -> std::io::Result<(Ready, Ready, Vec<Ready>)> {
    let mut fds = vec![
        crate::sys::PollFd {
            fd: a.as_raw_fd(),
            events: crate::sys::POLLIN,
            revents: 0,
        },
        crate::sys::PollFd {
            fd: b.as_raw_fd(),
            events: match b_owed {
                Owed::Nothing => crate::sys::POLLIN,
                Owed::Bytes => crate::sys::POLLIN | crate::sys::POLLOUT,
            },
            revents: 0,
        },
    ];
    fds.extend(also.iter().map(|(fd, i)| crate::sys::PollFd {
        fd: fd.as_raw_fd(),
        events: match i {
            Interest::Read => crate::sys::POLLIN,
            Interest::Write => crate::sys::POLLOUT,
        },
        revents: 0,
    }));
    // Rounded up to whole milliseconds, not down, from the nanoseconds: a
    // deadline 0.8 ms -- or 0.8 us -- ahead truncated to `poll(0)` returns
    // at once, and a caller waiting for that deadline asks again, spinning
    // until the clock reaches it.
    let ms = match timeout {
        None => -1,
        Some(d) => i32::try_from(d.as_nanos().div_ceil(1_000_000)).unwrap_or(i32::MAX),
    };
    // SAFETY: initialised records and their count; the kernel writes only
    // `revents`, and the vector outlives the call.
    let n = unsafe {
        crate::sys::poll(
            fds.as_mut_ptr(),
            crate::sys::Nfds::try_from(fds.len()).unwrap_or(2),
            ms,
        )
    };
    if n < 0 {
        let e = std::io::Error::last_os_error();
        return match e.kind() {
            std::io::ErrorKind::Interrupted => {
                Ok((Ready::Idle, Ready::Idle, vec![Ready::Idle; also.len()]))
            }
            _ => Err(e),
        };
    }
    let ready = |p: &crate::sys::PollFd| match p.revents {
        0 => Ready::Idle,
        _ => Ready::Readable,
    };
    let extra = fds[2..].iter().map(ready).collect();
    Ok((ready(&fds[0]), ready(&fds[1]), extra))
}

/// **The font bytes and parsed faces a stack is made of, held**: each file
/// mapping and each parsed slice lent out as `'static`, kept so a font
/// reload or a window's close can give them back once nothing borrows
/// them ([`Held::release`]). Dropped without that, they stay for the
/// process. It lives here beside [`map_readonly`], the mapping it gives
/// back, which is private to this file.
///
/// **A font file is mapped once for the process** ([`MAPS`]), however
/// many windows' stacks are made of it: a second [`Held::map`] of the
/// same file takes the first's mapping and counts itself, and only the
/// last [`Held::release`] of it unmaps it. Two windows drawing one font
/// share its pages rather than each holding a copy, and a release in one
/// window never unmaps what another still reads.
///
/// A parsed slice is kept as the pointer `Box::into_raw` gave, not as the
/// shared reference lent from it: freeing needs write permission, which a
/// pointer taken back from a shared reference does not carry.
#[derive(Default)]
pub struct Held {
    maps: Vec<&'static [u8]>,
    parsed: Vec<*mut [baud_font::Face<'static>]>,
}

/// What [`Held::release`] gave back: mappings the kernel unmapped, and
/// parsed slices freed.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Released {
    pub unmapped: usize,
    pub freed: usize,
}

impl Held {
    /// `path` mapped ([`map_readonly`]) and held: the mapping the process
    /// already has of this file when it has one, counted once more.
    pub fn map(&mut self, path: &str) -> std::io::Result<&'static [u8]> {
        let bytes = map_shared(path)?;
        self.maps.push(bytes);
        Ok(bytes)
    }

    /// `faces` lent for as long as they are held.
    pub fn leak(
        &mut self,
        faces: Vec<baud_font::Face<'static>>,
    ) -> &'static [baud_font::Face<'static>] {
        let raw = Box::into_raw(faces.into_boxed_slice());
        self.parsed.push(raw);
        // SAFETY: a live allocation `Box::into_raw` just gave up, valid
        // until `release` frees it, whose caller says nothing borrows it
        // by then; nothing writes through `raw` before that.
        unsafe { &*raw }
    }

    /// **Every parsed slice freed, then every mapping given back**: each
    /// one's count in [`MAPS`] taken down, and the file unmapped when no
    /// other holder counts it.
    ///
    /// # Safety
    ///
    /// Nothing may borrow a slice this lent -- a face, or the bytes under
    /// one -- once it is called: every `Stack` and `Fonts` made of them
    /// dropped, and no frame drawing from them. Another holder's borrows
    /// of a shared mapping stay valid: it is unmapped only once no holder
    /// counts it.
    pub unsafe fn release(self) -> Released {
        let mut given = Released::default();
        for raw in self.parsed {
            // SAFETY: the pointer `Box::into_raw` gave `leak`, given back
            // once; the caller says nothing borrows the slice.
            drop(unsafe { Box::from_raw(raw) });
            given.freed += 1;
        }
        for bytes in self.maps {
            if let Gone::Unmapped = unshare(bytes) {
                given.unmapped += 1;
            }
        }
        given
    }
}

/// **The font files the process has mapped**, each with how many holders
/// count it: one entry a file, keyed by what the file is rather than by
/// its path ([`FileId`]), so a font replaced at the same path -- a reload
/// after an edit -- is mapped afresh rather than handed the old bytes.
static MAPS: std::sync::Mutex<Vec<Mapping>> = std::sync::Mutex::new(Vec::new());

/// What a font file is: its device, inode, length and last change, from
/// `fstat`. A file written in place keeps its inode and changes its time
/// and, mostly, its length; one replaced by a rename has another inode.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct FileId {
    dev: Device,
    ino: Inode,
    len: FileBytes,
    changed: Changed,
}

/// The device a file is on, `st_dev`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Device(u64);

/// A file's inode number, `st_ino`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Inode(u64);

/// A file's length in bytes, `st_size`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct FileBytes(u64);

/// When a file's data last changed, `st_mtime` and its nanoseconds.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Changed(i64, i64);

impl FileId {
    fn of(m: &std::fs::Metadata) -> FileId {
        use std::os::unix::fs::MetadataExt;
        FileId {
            dev: Device(m.dev()),
            ino: Inode(m.ino()),
            len: FileBytes(m.len()),
            changed: Changed(m.mtime(), m.mtime_nsec()),
        }
    }
}

/// How many holders count a mapping: at least one while it is in the
/// table.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Holders(usize);

impl Holders {
    pub const ONE: Holders = Holders(1);

    pub fn more(self) -> Holders {
        Holders(self.0 + 1)
    }

    /// One fewer, or `None` when this was the last.
    fn fewer(self) -> Option<Holders> {
        self.0.checked_sub(1).filter(|&n| n > 0).map(Holders)
    }
}

/// One mapped file: what it is, its bytes, and how many holders count it.
struct Mapping {
    id: FileId,
    bytes: &'static [u8],
    holders: Holders,
}

/// What [`unshare`] did with a mapping.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Gone {
    /// Another holder still counts it.
    Kept,
    /// It was the last, and the kernel unmapped it.
    Unmapped,
    /// It was the last and `munmap` failed, or it was never in the table.
    Failed,
}

/// How many holders count the mapping that begins at `at`, `None` when
/// the process has no such mapping.
#[cfg(test)]
pub fn holders(at: *const u8) -> Option<Holders> {
    MAPS.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .iter()
        .find(|m| core::ptr::eq(m.bytes.as_ptr(), at))
        .map(|m| m.holders)
}

/// **`path`'s bytes, mapped once for the process**: the mapping [`MAPS`]
/// has of the same file, counted once more, else a new one entered there.
fn map_shared(path: &str) -> std::io::Result<&'static [u8]> {
    let file = std::fs::File::open(path)?;
    let id = FileId::of(&file.metadata()?);
    let mut maps = MAPS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(m) = maps.iter_mut().find(|m| m.id == id) {
        // It stays mapped while any holder counts it, this one now among
        // them.
        m.holders = m.holders.more();
        return Ok(m.bytes);
    }
    let bytes = map_readonly(file)?;
    maps.push(Mapping {
        id,
        bytes,
        holders: Holders::ONE,
    });
    Ok(bytes)
}

/// **One holder's count of `bytes`' mapping taken down**, and the file
/// unmapped when it was the last.
fn unshare(bytes: &'static [u8]) -> Gone {
    let mut maps = MAPS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let Some(i) = maps
        .iter()
        .position(|m| core::ptr::eq(m.bytes.as_ptr(), bytes.as_ptr()))
    else {
        return Gone::Failed;
    };
    if let Some(left) = maps[i].holders.fewer() {
        maps[i].holders = left;
        return Gone::Kept;
    }
    let m = maps.swap_remove(i);
    // SAFETY: a whole mapping `map_readonly` made, unmapped once, when the
    // last holder that counted it gives it back: its caller says nothing
    // borrows it any more, and no other holder counts it.
    let r = unsafe {
        crate::sys::munmap(
            m.bytes.as_ptr().cast_mut().cast(),
            crate::sys::MapLen(m.bytes.len()),
        )
    };
    match r {
        0 => Gone::Unmapped,
        _ => Gone::Failed,
    }
}

/// An open font file as a read-only, private mapping, for the life of
/// the process unless the last [`Held`] counting it gives it back. A face is a bounds-checked
/// view over the file's bytes, so a mapping is the same thing a read into
/// memory is -- without the read: Apple Color Emoji is 192 MB, and reading
/// it whole was 17 ms of every launch for the few strikes a screen uses.
/// The pages a face touches are the pages that get paged in.
fn map_readonly(file: std::fs::File) -> std::io::Result<&'static [u8]> {
    // **`File` is `open`, `fstat` and `close`**, and it opens
    // close-on-exec, so none of those and none of their flags has to be
    // declared in `sys`. That is most of what used to make this function
    // the largest consumer of a `libc` dependency.
    let len = usize::try_from(file.metadata()?.len()).unwrap_or(0);
    if len == 0 {
        return Err(std::io::Error::other("an empty font file"));
    }
    // SAFETY: a private read-only mapping of the whole file at offset
    // zero; the kernel validates the length against the file. The mapping
    // is unmapped only by the last `Held::release` that counts it
    // (`unshare`), whose caller says nothing borrows the slice any more.
    let ptr = unsafe {
        crate::sys::mmap(
            std::ptr::null_mut(),
            len,
            crate::sys::PROT_READ,
            crate::sys::MAP_PRIVATE,
            file.as_raw_fd(),
            0,
        )
    };
    // The mapping holds its own reference to the file, so closing the
    // descriptor -- which dropping `file` does -- leaves the pages.
    drop(file);
    if ptr == crate::sys::MAP_FAILED {
        return Err(std::io::Error::last_os_error());
    }
    // SAFETY: `ptr` is a live, page-aligned, read-only mapping of exactly
    // `len` bytes that outlives every borrow of it.
    Ok(unsafe { std::slice::from_raw_parts(ptr.cast::<u8>(), len) })
}

#[cfg(target_os = "macos")]
/// A display, as CoreGraphics names one.
#[repr(transparent)]
struct DisplayId(u32);

#[cfg(target_os = "macos")]
/// A count of pixels, or of points, as a display mode reports a width.
#[repr(transparent)]
struct Pixels(usize);

#[cfg(target_os = "macos")]
/// A display mode, as CoreGraphics hands one out: an opaque reference the
/// caller releases.
#[repr(transparent)]
#[derive(Clone, Copy)]
struct DisplayMode(*const core::ffi::c_void);

#[cfg(target_os = "macos")]
#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGMainDisplayID() -> DisplayId;
    fn CGDisplayCopyDisplayMode(display: DisplayId) -> DisplayMode;
    fn CGDisplayModeGetPixelWidth(mode: DisplayMode) -> Pixels;
    fn CGDisplayModeGetWidth(mode: DisplayMode) -> Pixels;
    fn CGDisplayModeRelease(mode: DisplayMode);
}

/// The main display's backing scale -- pixels over points -- from
/// CoreGraphics, which answers without starting AppKit. Asking
/// `NSScreen` for the same number costs AppKit's whole initialisation,
/// 42 ms measured, and the grid the shell is spawned at needs it first.
/// `None` when the display reports no width, which is no display.
#[cfg(target_os = "macos")]
pub fn display_scale() -> Option<f64> {
    // `CGDisplayPixelsWide` answers in points on a Retina display, so the
    // first version of this read 1 on a 2x panel and the first frame was
    // built at half its size. The display *mode* carries both widths.
    // SAFETY: four CoreGraphics calls with no preconditions; the mode is a
    // reference this function owns until it releases it, and a null one
    // is checked before use.
    let (pixels, points) = unsafe {
        let mode = CGDisplayCopyDisplayMode(CGMainDisplayID());
        if mode.0.is_null() {
            return None;
        }
        let both = (
            CGDisplayModeGetPixelWidth(mode),
            CGDisplayModeGetWidth(mode),
        );
        CGDisplayModeRelease(mode);
        both
    };
    if points.0 == 0 || pixels.0 == 0 {
        return None;
    }
    // Counts of pixels and points a display has are exact in an f64.
    Some(pixels.0 as f64 / points.0 as f64)
}

/// **The system's non-blinking-caret switch**, macOS 15's "prefer
/// non-blinking cursor": `AXPrefersNonBlinkingTextInsertionIndicator`,
/// looked up rather than linked, since it does not exist before 15 (its
/// header's `API_AVAILABLE(macos(15.0))`). The caller asks only on 15 or
/// later, so an older system loads no framework for a symbol it lacks.
///
/// The framework is asked for with `RTLD_NOLOAD` first -- AppKit has
/// loaded it by the first tick -- and opened only if it has not. The
/// function is looked up once and called fresh on every read: Chromium
/// calls it fresh on each query too. Any failure reads as the switch off,
/// which draws the caret as it asks, as before the switch existed.
#[cfg(target_os = "macos")]
pub fn prefers_steady_caret() -> crate::caret::NonBlinking {
    use crate::caret::NonBlinking;
    match steady_caret_symbol() {
        Some(f) if f().as_bool() => NonBlinking::On,
        _ => NonBlinking::Off,
    }
}

/// `AXPrefersNonBlinkingTextInsertionIndicator`: `BOOL (void)`.
#[cfg(target_os = "macos")]
type AskSteady = extern "C" fn() -> macos_liquid_glass::objc2::runtime::Bool;

/// The switch's function, looked up once, or `None` where the framework or
/// the symbol is missing.
#[cfg(target_os = "macos")]
fn steady_caret_symbol() -> Option<AskSteady> {
    use crate::sys::{RTLD_LAZY, RTLD_LOCAL, RTLD_NOLOAD, dlopen, dlsym};
    use std::sync::OnceLock;
    static ASK: OnceLock<Option<AskSteady>> = OnceLock::new();
    *ASK.get_or_init(|| {
        let path = c"/System/Library/Frameworks/Accessibility.framework/Accessibility";
        // SAFETY: `dlopen` and `dlsym` take NUL-terminated strings, which
        // `c""` literals are, and answer null on any failure, which is
        // checked. A non-null symbol is the function the SDK header
        // declares, `BOOL (void)`, and `Bool` is `repr(transparent)` over
        // the platform's `BOOL`, so the pointer is that function's.
        unsafe {
            let mut handle = dlopen(path.as_ptr(), RTLD_NOLOAD | RTLD_LAZY);
            if handle.is_null() {
                handle = dlopen(path.as_ptr(), RTLD_LAZY | RTLD_LOCAL);
            }
            if handle.is_null() {
                return None;
            }
            let sym = dlsym(
                handle,
                c"AXPrefersNonBlinkingTextInsertionIndicator".as_ptr(),
            );
            (!sym.is_null()).then(|| core::mem::transmute::<*mut core::ffi::c_void, AskSteady>(sym))
        }
    })
}

#[cfg(target_os = "macos")]
unsafe extern "C" {
    /// libSystem's `clock_gettime_nsec_np`, which libc 0.2 does not bind
    /// (it binds the `CLOCK_UPTIME_RAW` constant and `clock_gettime`, whose
    /// `timespec` would need a multiply and an add to become nanoseconds).
    /// It answers a `uint64_t`, and [`HostTime`] is `repr(transparent)`
    /// over that width, so the return crosses the ABI as the type the
    /// instrument uses rather than as a bare integer -- the same shape as
    /// the CoreGraphics declarations above.
    fn clock_gettime_nsec_np(clock_id: core::ffi::c_uint) -> HostTime;
}

/// Now, on the clock every stamp in the keystroke instrument shares:
/// `CLOCK_UPTIME_RAW`, which is `mach_absolute_time` in nanoseconds -- the
/// base `NSEvent.timestamp`, `CADisplayLink.targetTimestamp` and Metal's
/// `GPUEndTime` are expressed in (see `baud_domain::time`). The one place
/// this binary reads it, so every caller agrees on the clock by
/// construction. Zero if the call fails, which it does not for a valid
/// clock id.
#[cfg(target_os = "macos")]
pub fn host_now() -> HostTime {
    // SAFETY: a libSystem call with no preconditions beyond a valid clock
    // id, which `CLOCK_UPTIME_RAW` is.
    unsafe { clock_gettime_nsec_np(crate::sys::CLOCK_UPTIME_RAW) }
}

#[cfg(test)]
mod shell_tests;

#[cfg(test)]
mod tests {
    use super::*;

    /// **A font file two windows' fonts map is mapped once, and unmapped
    /// only when the last gives it back**: the second map is the first's
    /// bytes counted twice, the first release keeps them mapped and
    /// readable, the second unmaps them. A file replaced at the same path
    /// is mapped afresh, so a reload after an edit reads the new bytes.
    #[test]
    fn a_font_file_is_mapped_once_and_unmapped_by_its_last_holder() {
        let dir = std::env::temp_dir().join(format!("baud-shared-map-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("Face.ttf");
        std::fs::write(&path, b"first bytes").unwrap();
        let p = path.to_str().unwrap();
        let (mut a, mut b) = (Held::default(), Held::default());
        let one = a.map(p).unwrap();
        let two = b.map(p).unwrap();
        assert!(core::ptr::eq(one.as_ptr(), two.as_ptr()), "one mapping");
        let at = one.as_ptr();
        assert_eq!(holders(at), Some(Holders(2)));
        // SAFETY: nothing borrows `a`'s bytes but `one`, not read again.
        let given = unsafe { a.release() };
        assert_eq!(given.unmapped, 0, "b still counts it");
        assert_eq!(holders(at), Some(Holders(1)));
        assert_eq!(two, b"first bytes", "and reads it");
        // A rename over the path: another inode, so another mapping.
        let next = dir.join("Face.ttf.new");
        std::fs::write(&next, b"second, longer bytes").unwrap();
        std::fs::rename(&next, &path).unwrap();
        let mut c = Held::default();
        let three = c.map(p).unwrap();
        assert_eq!(three, b"second, longer bytes", "the file as it is now");
        assert!(!core::ptr::eq(two.as_ptr(), three.as_ptr()));
        // SAFETY: nothing borrows `b`'s bytes after this line.
        let given = unsafe { b.release() };
        assert_eq!(given.unmapped, 1, "the last holder unmaps it");
        assert_eq!(holders(at), None);
        // SAFETY: nothing borrows `c`'s bytes after this line.
        let given = unsafe { c.release() };
        assert_eq!(given.unmapped, 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// **`wait` says which of its extra descriptors is ready**, so the
    /// loop reads only those: one with a byte waiting is readable, one
    /// with none is idle, whichever end of the list each is at.
    #[cfg(target_os = "linux")]
    #[test]
    fn wait_says_which_extra_descriptor_is_ready() {
        use std::io::Write;
        use std::os::fd::AsFd;
        let (a, _a) = std::os::unix::net::UnixStream::pair().unwrap();
        let (b, _b) = std::os::unix::net::UnixStream::pair().unwrap();
        let (quiet, _q) = std::os::unix::net::UnixStream::pair().unwrap();
        let (heard, mut to_heard) = std::os::unix::net::UnixStream::pair().unwrap();
        to_heard.write_all(b"x").unwrap();
        for also in [
            [
                (quiet.as_fd(), Interest::Read),
                (heard.as_fd(), Interest::Read),
            ],
            [
                (heard.as_fd(), Interest::Read),
                (quiet.as_fd(), Interest::Read),
            ],
        ] {
            let (ra, rb, extra) = wait(
                a.as_fd(),
                b.as_fd(),
                Owed::Nothing,
                &also,
                Some(Duration::ZERO),
            )
            .unwrap();
            assert_eq!((ra, rb), (Ready::Idle, Ready::Idle));
            let want: Vec<Ready> = also
                .iter()
                .map(|(fd, _)| match fd.as_raw_fd() == heard.as_raw_fd() {
                    true => Ready::Readable,
                    false => Ready::Idle,
                })
                .collect();
            assert_eq!(extra, want);
        }
    }

    /// **What the environment says baud is, XTVERSION says too.** A
    /// program reads `TERM_PROGRAM` and `TERM_PROGRAM_VERSION` before it
    /// starts and asks `CSI > q` once running (yazi does both); the two
    /// answers come from two crates, so this holds them to one text:
    /// `DCS > | baud(<workspace version>) ST`.
    #[test]
    fn the_environment_names_what_xtversion_reports() {
        let get = |k: &str| OURS.iter().find(|(o, _)| *o == k).map(|(_, v)| *v);
        let (name, version) = (get("TERM_PROGRAM"), get("TERM_PROGRAM_VERSION"));
        let want = format!("\x1bP>|{}({})\x1b\\", name.unwrap(), version.unwrap());
        let got = baud_domain::Report::Version.encode(baud_domain::C1Transmit::SevenBit);
        assert_eq!(
            String::from_utf8_lossy(got.as_bytes()),
            want,
            "XTVERSION against TERM_PROGRAM and TERM_PROGRAM_VERSION"
        );
    }

    /// **The positive control for the switch.** A misspelt symbol, or a
    /// framework path that moved, reads as the switch off forever with every
    /// other test green. So the lookup is held to the OS version, asked of
    /// `sw_vers` -- not of `NSProcessInfo`, since this test binary links no
    /// Foundation (`-dead_strip_dylibs` keeps only what it names): the
    /// symbol resolves on macOS 15 and later and is absent before. Its value
    /// is the user's setting and is not asserted.
    #[cfg(target_os = "macos")]
    #[test]
    fn the_steady_switch_resolves_on_macos_fifteen_and_later() {
        let out = std::process::Command::new("sw_vers")
            .arg("-productVersion")
            .output()
            .expect("sw_vers runs");
        let version = String::from_utf8_lossy(&out.stdout);
        let major: u32 = version
            .trim()
            .split('.')
            .next()
            .and_then(|m| m.parse().ok())
            .unwrap_or_else(|| panic!("sw_vers said {version:?}"));
        assert_eq!(
            steady_caret_symbol().is_some(),
            major >= 15,
            "macOS {version}"
        );
        // And the read answers, whichever way the user has set it.
        let _ = prefers_steady_caret();
    }

    /// A parent environment as Terminal.app, kitty and tmux leave it, with
    /// two variables that are nobody's terminal and must pass through.
    fn outer_parent() -> Vec<(OsString, OsString)> {
        [
            ("TERM_PROGRAM", "Apple_Terminal"),
            ("TERM_PROGRAM_VERSION", "455"),
            ("TERM_SESSION_ID", "w0t0p0:0"),
            ("KITTY_WINDOW_ID", "3"),
            ("TMUX", "/tmp/tmux-501/default,1,0"),
            ("TERM", "dumb"),
            ("COLORTERM", "no"),
            ("HOME", "/nowhere"),
            ("BAUD_KEPT", "yes"),
        ]
        .into_iter()
        .map(|(k, v)| (OsString::from(k), OsString::from(v)))
        .collect()
    }

    fn strings(env: &[CString]) -> Vec<String> {
        let mut v: Vec<String> = env
            .iter()
            .map(|e| e.to_string_lossy().into_owned())
            .collect();
        v.sort();
        v
    }

    fn ours() -> Vec<String> {
        OURS.iter().map(|(k, v)| format!("{k}={v}")).collect()
    }

    /// **The child names baud and no outer terminal**: every identity
    /// variable the parent carried is gone, each of baud's appears once
    /// with baud's value, and a variable that names no terminal survives.
    #[test]
    fn the_child_names_baud_and_no_outer_terminal() {
        let mut want = ours();
        want.push("BAUD_KEPT=yes".into());
        want.push("HOME=/nowhere".into());
        want.sort();
        assert_eq!(strings(&child_env(outer_parent(), None)), want);
    }

    /// The list is what it says: no entry is one of baud's own, which
    /// would drop a variable and then set it -- harmless, but a sign the
    /// two lists were not read together -- and none is repeated.
    #[test]
    fn the_outer_list_is_disjoint_from_ours_and_has_no_repeat() {
        for o in OUTER {
            assert!(!OURS.iter().any(|(k, _)| k == o), "{o} is in both lists");
        }
        let mut seen: Vec<&str> = OUTER.to_vec();
        seen.sort_unstable();
        seen.dedup();
        assert_eq!(seen.len(), OUTER.len(), "OUTER repeats an entry");
    }

    /// **The master closes on exec**, so a program the app starts never
    /// holds it. Perturbation: drop the `F_SETFD` -- this fails.
    #[test]
    fn the_master_closes_on_exec() {
        let size = WinSize {
            ws_col: 80,
            ws_row: 24,
        };
        let master = Pty::fork_with(
            text_area(&size, Px::of(8), Px::of(16)),
            Some("exit 0".into()),
            child_env(outer_parent(), None),
        )
        .expect("forkpty");
        // SAFETY: an open descriptor; `F_GETFD` takes no argument.
        let flags = unsafe { crate::sys::fcntl(master, crate::sys::F_GETFD) };
        let _ = read_all(master);
        assert!(flags >= 0);
        assert_eq!(flags & crate::sys::FD_CLOEXEC, crate::sys::FD_CLOEXEC);
    }

    /// **And the child a pty forks actually sees it**: the same parent
    /// environment handed to a real `forkpty`, with the child running
    /// `env`, read back off the master. This is the gate on the fork's
    /// plumbing -- the `execve` and the pointer array -- rather than on the
    /// filter, which the test above holds.
    #[test]
    fn a_forked_child_sees_the_environment_baud_gives_it() {
        let size = WinSize {
            ws_col: 80,
            ws_row: 24,
        };
        let master = Pty::fork_with(
            text_area(&size, Px::of(8), Px::of(16)),
            Some("exec /usr/bin/env".into()),
            child_env(outer_parent(), None),
        )
        .expect("forkpty");
        let out = read_all(master);
        let text = String::from_utf8_lossy(&out);
        let lines: Vec<&str> = text.lines().map(|l| l.trim_end_matches('\r')).collect();
        assert!(
            lines.contains(&"BAUD_KEPT=yes"),
            "the child's env never arrived, so nothing below is evidence: {text:?}"
        );
        for want in ours() {
            assert_eq!(
                lines.iter().filter(|l| **l == want).count(),
                1,
                "{want} is not in the child's env exactly once: {text:?}"
            );
        }
        for gone in [
            "TERM_SESSION_ID=",
            "KITTY_WINDOW_ID=",
            "TMUX=",
            "TERM_PROGRAM=Apple",
        ] {
            assert!(
                !lines.iter().any(|l| l.starts_with(gone)),
                "the child inherited {gone}: {text:?}"
            );
        }
    }

    /// A child of a bare `forkpty` that runs only `execve`, as every spawn
    /// did before 2026-10-13: the kernel's own terminal, for a control.
    fn bare_fork(ws: crate::sys::Winsize, cmd: &str, env: &[CString]) -> crate::sys::Fd {
        let path = CString::new("/bin/sh").expect("a path");
        let argv0 = CString::new("sh").expect("argv0");
        let dash = CString::new("-c").expect("-c");
        let cmd = CString::new(cmd).expect("a command");
        let argv = [
            argv0.as_ptr(),
            dash.as_ptr(),
            cmd.as_ptr(),
            std::ptr::null(),
        ];
        let envp: Vec<*const core::ffi::c_char> = env
            .iter()
            .map(|e| e.as_ptr())
            .chain(std::iter::once(std::ptr::null()))
            .collect();
        let (pid, master) = fork_terminal(ws);
        assert!(pid >= 0, "forkpty");
        if pid == 0 {
            // SAFETY: as in `fork_with`; the child runs only `execve` and
            // `_exit`.
            unsafe {
                crate::sys::execve(path.as_ptr(), argv.as_ptr(), envp.as_ptr());
                crate::sys::_exit(127);
            }
        }
        master
    }

    /// **Terminals opened at once all open.** Darwin's `/dev/ptmx` can hand
    /// two openers the same terminal, and the kernel redoes the loser's
    /// open ten times before it fails it with `EREDRIVEOPEN`; forty-eight
    /// threads forking at once lose that often enough to fail it. Each
    /// thread forks a child through [`fork_terminal`], as a window does,
    /// that exits at once, reads its master to the end and reaps it.
    #[test]
    fn terminals_opened_at_once_all_open() {
        const THREADS: usize = 48;
        const ROUNDS: usize = 10;
        let size = WinSize {
            ws_col: 80,
            ws_row: 24,
        };
        let ws = text_area(&size, Px::of(8), Px::of(16));
        let start = std::sync::Barrier::new(THREADS);
        let failed: Vec<String> = std::thread::scope(|s| {
            let opened: Vec<_> =
                (0..THREADS)
                    .map(|_| {
                        s.spawn(|| {
                        start.wait();
                        let mut failed = Vec::new();
                        for _ in 0..ROUNDS {
                            let (pid, master) = fork_terminal(ws);
                            if pid < 0 {
                                failed.push(std::io::Error::last_os_error().to_string());
                                continue;
                            }
                            if pid == 0 {
                                // SAFETY: the child calls only `_exit`.
                                unsafe { crate::sys::_exit(0) }
                            }
                            read_all(master);
                            // SAFETY: `master` is the descriptor forkpty
                            // returned and nothing else owns it; `pid` is
                            // this thread's own child.
                            unsafe {
                                drop(<std::os::fd::OwnedFd as std::os::fd::FromRawFd>::from_raw_fd(
                                    master,
                                ));
                                crate::sys::waitpid(pid, std::ptr::null_mut(), 0);
                            }
                        }
                        failed
                    })
                    })
                    .collect();
            opened
                .into_iter()
                .flat_map(|t| t.join().expect("a thread"))
                .collect()
        });
        assert!(
            failed.is_empty(),
            "of {} opens, {} failed: {failed:?}",
            THREADS * ROUNDS,
            failed.len()
        );
    }

    /// What `/bin/stty -a` printed on `master`, word by word, once the child
    /// has gone; the descriptor is closed after.
    fn stty_words(master: crate::sys::Fd) -> Vec<String> {
        let out = read_all(master);
        // SAFETY: `master` is the descriptor forkpty returned and nothing
        // else owns it.
        drop(unsafe { <std::os::fd::OwnedFd as std::os::fd::FromRawFd>::from_raw_fd(master) });
        String::from_utf8_lossy(&out)
            .split_whitespace()
            .map(str::to_string)
            .collect()
    }

    /// **The child's terminal has `IUTF8`, and nothing else differs from
    /// the kernel's.** `stty -a` run through baud's spawn against the same
    /// command under a bare `forkpty`: the two print the same words but
    /// `iutf8` for `-iutf8`. The bare one printing `-iutf8` is the control
    /// -- `stty` names the flag on this system and the default leaves it
    /// clear, so the probe can fire -- and the comparison holds the route to
    /// one bit, every other flag, character and speed the kernel's.
    #[test]
    fn a_spawned_childs_terminal_has_iutf8_and_otherwise_the_kernels_default() {
        let size = WinSize {
            ws_col: 80,
            ws_row: 24,
        };
        let ws = text_area(&size, Px::of(8), Px::of(16));
        let env = child_env(outer_parent(), None);
        let bare = stty_words(bare_fork(ws, "exec /bin/stty -a", &env));
        assert!(
            bare.iter().any(|w| w == "-iutf8"),
            "the control: a bare forkpty's terminal reads -iutf8: {bare:?}"
        );
        let ours =
            stty_words(Pty::fork_with(ws, Some("exec /bin/stty -a".into()), env).expect("forkpty"));
        let want: Vec<String> = bare
            .iter()
            .map(|w| match w.as_str() {
                "-iutf8" => "iutf8".to_string(),
                _ => w.clone(),
            })
            .collect();
        assert_eq!(ours, want, "baud's spawn against the kernel's default");
    }

    /// The bytes a line read in canonical mode holds after `typed`, in hex:
    /// the child turns echo off, says it is ready -- after which its terminal
    /// is set, so the line discipline reads `typed` under the flags the
    /// spawn left -- reads one line and prints it between markers.
    fn line_after(typed: &[u8]) -> String {
        let size = WinSize {
            ws_col: 80,
            ws_row: 24,
        };
        let master = Pty::fork_with(
            text_area(&size, Px::of(8), Px::of(16)),
            Some(
                r#"/bin/stty -echo; echo ready; IFS= read -r l; printf 'GOT[%s]\n' "$(printf '%s' "$l" | /usr/bin/od -An -tx1 | /usr/bin/tr -d ' \n')""#
                    .into(),
            ),
            child_env(outer_parent(), None),
        )
        .expect("forkpty");
        read_until(master, "ready\r\n");
        // SAFETY: `typed` is a live buffer of the length passed.
        let n = unsafe { crate::sys::write(master, typed.as_ptr().cast(), typed.len()) };
        assert_eq!(
            usize::try_from(n).ok(),
            Some(typed.len()),
            "the pty took it"
        );
        let out = read_all(master);
        // SAFETY: as in `stty_words`.
        drop(unsafe { <std::os::fd::OwnedFd as std::os::fd::FromRawFd>::from_raw_fd(master) });
        let text = String::from_utf8_lossy(&out).into_owned();
        text.split_once("GOT[")
            .and_then(|(_, rest)| rest.split_once(']'))
            .map(|(line, _)| line.to_string())
            .unwrap_or_else(|| panic!("the child printed no line: {text:?}"))
    }

    /// **Backspace after a Hebrew letter leaves an empty line**, where the
    /// kernel's default left its lead byte `d7` (note AA section 5, measured
    /// by `scripts/locale/iutf8_probe.py`). The control, `ab` then
    /// Backspace, leaves `61` with the flag or without it, so a probe that
    /// erased everything would fail it.
    #[test]
    fn backspace_after_a_hebrew_letter_erases_the_whole_letter() {
        assert_eq!(line_after(b"ab\x7f\n"), "61", "the control");
        assert_eq!(line_after(b"\xd7\x99\x7f\n"), "");
    }

    /// Whether a test lets the line discipline follow the core's encoding,
    /// or leaves the bit as the child set it -- the control, and what the
    /// app did before the bit followed.
    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    enum Follow {
        Encoding,
        Never,
    }

    /// A text key, as `key_encode.rs` spells one.
    fn ch(c: char) -> baud_core::KeyCode {
        baud_core::KeyCode::Char(baud_core::Codepoint::from_char(c))
    }

    /// The bytes `keys` send through `Term::key`, in the encoding `t` is in.
    fn typed(t: &mut baud_core::Term<'_>, keys: &[baud_core::KeyCode]) -> Vec<u8> {
        t.clear_reply();
        for &k in keys {
            assert!(
                t.key(baud_core::KeyEvent::press(k, baud_core::Modifiers::NONE)),
                "{k:?} sends something"
            );
        }
        let out = t.reply().to_vec();
        t.clear_reply();
        out
    }

    /// `bytes` into the core, as `reads::feed` hands it a read.
    fn feed(t: &mut baud_core::Term<'_>, bytes: &[u8]) {
        t.write_at(baud_domain::BytePos::ZERO, bytes);
        t.feed(
            baud_domain::BytePos::ZERO,
            baud_domain::ReadLen::from_index(bytes.len()),
        );
    }

    /// `bytes` to the child, all of them.
    fn send(master: crate::sys::Fd, bytes: &[u8]) {
        // SAFETY: `bytes` is a live buffer of the length passed.
        let n = unsafe { crate::sys::write(master, bytes.as_ptr().cast(), bytes.len()) };
        assert_eq!(
            usize::try_from(n).ok(),
            Some(bytes.len()),
            "the pty took it"
        );
    }

    /// The child's half of a line test: echo off, then for each marker, the
    /// marker (after which its terminal is as the parent leaves it), one
    /// line read in canonical mode, and that line in hex between `GOT[` and
    /// `]`. `before` is printed ahead of each marker -- `ESC % @` or
    /// `ESC % G`, which the core reads in the same output.
    fn line_script(steps: &[(&str, &str)]) -> String {
        let mut s = String::from("/bin/stty -echo");
        for (before, marker) in steps {
            s.push_str(&format!(
                r#"; printf '{before}{marker}\n'; IFS= read -r l; printf 'GOT[%s]\n' "$(printf '%s' "$l" | /usr/bin/od -An -tx1 | /usr/bin/tr -d ' \n')""#
            ));
        }
        s
    }

    /// The lines a child read in canonical mode after the keys of each
    /// step, typed through a `Term` that read everything the child wrote,
    /// with the line discipline following its encoding or not: the whole
    /// path from the child's `ESC % @` to the line discipline, but the
    /// driver's pump. Each step is the bytes printed before its marker and
    /// the keys typed after it.
    fn lines_typed(follow: Follow, steps: &[(&str, &[baud_core::KeyCode])]) -> Vec<String> {
        let size = WinSize {
            ws_col: 80,
            ws_row: 24,
        };
        let markers: Vec<(String, String)> = steps
            .iter()
            .enumerate()
            .map(|(i, (before, _))| (before.to_string(), format!("ready{i}")))
            .collect();
        let script = line_script(
            &markers
                .iter()
                .map(|(b, m)| (b.as_str(), m.as_str()))
                .collect::<Vec<_>>(),
        );
        let master = Pty::fork_with(
            text_area(&size, Px::of(8), Px::of(16)),
            Some(script),
            child_env(outer_parent(), None),
        )
        .expect("forkpty");
        let mut line = LineDiscipline::of(master);
        let mut s = baud_core::Owned::new(&size);
        let mut t = baud_core::Term::new(s.storage(), &size);
        let mut out = Vec::new();
        for ((_, keys), (_, marker)) in steps.iter().zip(&markers) {
            let read = read_until(master, &format!("{marker}\r\n"));
            out.extend_from_slice(&read);
            feed(&mut t, &read);
            if follow == Follow::Encoding {
                if let Followed::Failed(e) = line.follow(t.input_encoding()) {
                    panic!("the line discipline did not follow: {e}");
                }
            }
            let bytes = typed(&mut t, keys);
            send(master, &bytes);
        }
        out.extend_from_slice(&read_all(master));
        // SAFETY: as in `stty_words`.
        drop(unsafe { <std::os::fd::OwnedFd as std::os::fd::FromRawFd>::from_raw_fd(master) });
        let text = String::from_utf8_lossy(&out).into_owned();
        let got: Vec<String> = text
            .split("GOT[")
            .skip(1)
            .map(|rest| rest.split_once(']').expect("a closed GOT").0.to_string())
            .collect();
        assert_eq!(got.len(), steps.len(), "one line a step: {text:?}");
        got
    }

    /// **Under `ESC % @` Backspace erases one ISO 8859-1 byte.** The child
    /// switches the core to ISO 8859-1; `a`, `£`, Backspace and Return
    /// through `Term::key` send `61 a3 7f 0d`, and the line holds `61`.
    /// The control leaves the bit as the child set it: the erase reads
    /// `a3` as a continuation and takes the `a` with it, the empty line a
    /// spawn that only sets the bit in the child leaves, so the probe can
    /// fire.
    #[test]
    fn under_latin1_backspace_erases_one_byte() {
        use baud_core::KeyCode::{Backspace, Enter};
        let keys = [ch('a'), ch('£'), Backspace, Enter];
        {
            let size = WinSize {
                ws_col: 80,
                ws_row: 24,
            };
            let mut s = baud_core::Owned::new(&size);
            let mut t = baud_core::Term::new(s.storage(), &size);
            feed(&mut t, b"\x1b%@");
            assert_eq!(
                typed(&mut t, &keys),
                b"a\xa3\x7f\r",
                "the keyboard sends ISO 8859-1"
            );
        }
        let step: &[(&str, &[baud_core::KeyCode])] = &[(r"\033%%@", &keys)];
        assert_eq!(lines_typed(Follow::Never, step), [""], "the control");
        assert_eq!(lines_typed(Follow::Encoding, step), ["61"]);
    }

    /// **Back under `ESC % G`, Backspace erases the whole letter again.**
    /// The same child switches to ISO 8859-1, reads a line, switches back
    /// and reads another: `a £` Backspace leaves `61`, then `י` Backspace,
    /// `d7 99 7f`, leaves the line empty. Without the bit set again the
    /// erase would take `99` and leave `d7`.
    #[test]
    fn back_under_utf8_backspace_erases_the_whole_letter() {
        use baud_core::KeyCode::{Backspace, Enter};
        let latin1 = [ch('a'), ch('£'), Backspace, Enter];
        let utf8 = [ch('י'), Backspace, Enter];
        assert_eq!(
            lines_typed(
                Follow::Encoding,
                &[(r"\033%%@", &latin1), (r"\033%%G", &utf8)]
            ),
            ["61", ""]
        );
    }

    /// **The line discipline is told only when the encoding changes**: the
    /// app calls [`LineDiscipline::follow`] after every read, and an open,
    /// two attribute calls and a close a read would be work for nothing.
    /// UTF-8 at the spawn, as the child left the bit.
    #[test]
    fn the_line_discipline_is_told_only_when_the_encoding_changes() {
        let size = WinSize {
            ws_col: 80,
            ws_row: 24,
        };
        let master = Pty::fork_with(
            text_area(&size, Px::of(8), Px::of(16)),
            Some("IFS= read -r l".into()),
            child_env(outer_parent(), None),
        )
        .expect("forkpty");
        let mut line = LineDiscipline::of(master);
        let told = |f: Followed| match f {
            Followed::Same => false,
            Followed::Told => true,
            Followed::Failed(e) => panic!("{e}"),
        };
        assert!(!told(line.follow(InputEncoding::Utf8)), "as spawned");
        assert!(told(line.follow(InputEncoding::Latin1)));
        assert!(!told(line.follow(InputEncoding::Latin1)));
        assert!(told(line.follow(InputEncoding::Utf8)));
        assert!(!told(line.follow(InputEncoding::Utf8)));
        send(master, b"\r");
        drop(read_all(master));
        // SAFETY: as in `stty_words`.
        drop(unsafe { <std::os::fd::OwnedFd as std::os::fd::FromRawFd>::from_raw_fd(master) });
    }

    /// What `stty -a < /dev/tty` printed after the child switched the core
    /// to ISO 8859-1 and the parent did or did not follow: the child's
    /// controlling terminal, opened by that name after the parent's open,
    /// change and close.
    fn stty_after_latin1(follow: Follow) -> Vec<String> {
        let size = WinSize {
            ws_col: 80,
            ws_row: 24,
        };
        let master = Pty::fork_with(
            text_area(&size, Px::of(8), Px::of(16)),
            Some(
                r#"/bin/stty -echo; printf '\033%%@ready\n'; IFS= read -r l; exec /bin/stty -a < /dev/tty"#
                    .into(),
            ),
            child_env(outer_parent(), None),
        )
        .expect("forkpty");
        let mut s = baud_core::Owned::new(&size);
        let mut t = baud_core::Term::new(s.storage(), &size);
        feed(&mut t, &read_until(master, "ready\r\n"));
        assert_eq!(
            t.input_encoding(),
            InputEncoding::Latin1,
            "the core switched"
        );
        if follow == Follow::Encoding {
            let mut line = LineDiscipline::of(master);
            assert!(matches!(line.follow(t.input_encoding()), Followed::Told));
        }
        send(master, b"\r");
        stty_words(master)
    }

    /// **The parent's change is one bit, on the child's own terminal.**
    /// `stty -a` on the child's `/dev/tty` -- which opens only while the
    /// child still has its controlling terminal -- prints the same words
    /// with and without the parent's change, but `-iutf8` for `iutf8`.
    #[test]
    fn the_parents_change_is_iutf8_alone_on_the_childs_terminal() {
        let before = stty_after_latin1(Follow::Never);
        assert!(
            before.iter().any(|w| w == "iutf8"),
            "the control: the child set the bit: {before:?}"
        );
        let want: Vec<String> = before
            .iter()
            .map(|w| match w.as_str() {
                "iutf8" => "-iutf8".to_string(),
                _ => w.clone(),
            })
            .collect();
        assert_eq!(stty_after_latin1(Follow::Encoding), want);
    }

    /// **A full reset sets `IUTF8` again.** The child switches the core to
    /// ISO 8859-1 and the parent follows, clearing the bit; then the child
    /// sends `RIS`, which returns the core to UTF-8, and the parent follows
    /// again. `stty -a` on the child's `/dev/tty` prints `iutf8`. Each
    /// switch arrives in a read of its own, so the parent follows both,
    /// as the app does after every read; the first is the control, which
    /// prints `-iutf8` once the bit is cleared.
    #[test]
    fn a_full_reset_sets_iutf8_again() {
        let size = WinSize {
            ws_col: 80,
            ws_row: 24,
        };
        let master = Pty::fork_with(
            text_area(&size, Px::of(8), Px::of(16)),
            Some(
                r#"/bin/stty -echo; printf '\033%%@latin\n'; IFS= read -r l; /bin/stty -a < /dev/tty; printf 'cleared\n'; IFS= read -r l; printf '\033creset\n'; IFS= read -r l; exec /bin/stty -a < /dev/tty"#
                    .into(),
            ),
            child_env(outer_parent(), None),
        )
        .expect("forkpty");
        let mut line = LineDiscipline::of(master);
        let mut s = baud_core::Owned::new(&size);
        let mut t = baud_core::Term::new(s.storage(), &size);
        let told = |f: Followed| match f {
            Followed::Same => false,
            Followed::Told => true,
            Followed::Failed(e) => panic!("{e}"),
        };
        feed(&mut t, &read_until(master, "latin\r\n"));
        assert_eq!(t.input_encoding(), InputEncoding::Latin1);
        assert!(told(line.follow(t.input_encoding())), "cleared");
        send(master, b"\r");
        let cleared = read_until(master, "cleared\r\n");
        let words = String::from_utf8_lossy(&cleared).into_owned();
        assert!(
            words.split_whitespace().any(|w| w == "-iutf8"),
            "the control: the bit is clear under ISO 8859-1: {words:?}"
        );
        send(master, b"\r");
        feed(&mut t, &read_until(master, "reset\r\n"));
        assert_eq!(t.input_encoding(), InputEncoding::Utf8, "RIS");
        assert!(told(line.follow(t.input_encoding())), "set again");
        send(master, b"\r");
        let after = stty_words(master);
        assert!(
            after.iter().any(|w| w == "iutf8"),
            "the bit is set again: {after:?}"
        );
    }

    /// Whether `master` reads as hung up within two seconds -- a bound, so
    /// a subsidiary held open fails the test rather than hanging it.
    fn hung_up(master: crate::sys::Fd) -> bool {
        make_nonblocking(master);
        let mut p = crate::sys::PollFd {
            fd: master,
            events: crate::sys::POLLIN,
            revents: 0,
        };
        // SAFETY: one initialised record and its count.
        let ready = unsafe { crate::sys::poll(&mut p, 1, 2000) };
        let mut buf = [0u8; 64];
        ready > 0 && matches!(read_master(master, &mut buf), Got::Closed)
    }

    /// **The parent never holds the subsidiary, so the child's going still
    /// hangs the manager up.** The child prints `ESC % @` and exits; the
    /// parent sees the end, reads the switch and follows it after the child
    /// has gone, and the manager still reads as hung up. The control holds
    /// the subsidiary open across the same read, and the manager then reads
    /// as waiting, so the probe can fire.
    #[test]
    fn following_after_the_child_has_gone_leaves_the_manager_hung_up() {
        let size = WinSize {
            ws_col: 80,
            ws_row: 24,
        };
        let spawn = || {
            Pty::fork_with(
                text_area(&size, Px::of(8), Px::of(16)),
                Some(r"printf '\033%%@'".into()),
                child_env(outer_parent(), None),
            )
            .expect("forkpty")
        };
        let master = spawn();
        let mut line = LineDiscipline::of(master);
        let mut s = baud_core::Owned::new(&size);
        let mut t = baud_core::Term::new(s.storage(), &size);
        feed(&mut t, &read_all(master));
        assert_eq!(
            t.input_encoding(),
            InputEncoding::Latin1,
            "the core switched"
        );
        let followed = line.follow(t.input_encoding());
        assert!(
            matches!(followed, Followed::Told),
            "the subsidiary opens after the child has gone: {followed:?}"
        );
        assert!(hung_up(master), "the manager reads as hung up");
        // SAFETY: as in `stty_words`.
        drop(unsafe { <std::os::fd::OwnedFd as std::os::fd::FromRawFd>::from_raw_fd(master) });

        let master = spawn();
        drop(read_all(master));
        let held = std::fs::OpenOptions::new()
            .read(true)
            .custom_flags_noctty()
            .open(subsidiary_of(master).expect("a subsidiary"))
            .expect("the subsidiary opens");
        assert!(!hung_up(master), "the control: held open, it waits");
        drop(held);
        // SAFETY: as in `stty_words`.
        drop(unsafe { <std::os::fd::OwnedFd as std::os::fd::FromRawFd>::from_raw_fd(master) });
    }

    /// `O_NOCTTY` on an `OpenOptions`, for the controls that open the
    /// subsidiary as the parent does.
    trait NoCtty {
        fn custom_flags_noctty(&mut self) -> &mut Self;
    }
    impl NoCtty for std::fs::OpenOptions {
        fn custom_flags_noctty(&mut self) -> &mut Self {
            use std::os::unix::fs::OpenOptionsExt;
            self.custom_flags(crate::sys::O_NOCTTY)
        }
    }

    /// The variable that turns [`ctty_helper`] on: the subsidiary's path,
    /// then `follow` or `plain` after a tab.
    const CTTY_HELPER: &str = "BAUD_TEST_CTTY_HELPER";

    /// **Run only by [`the_parent_never_takes_the_subsidiary_as_its_terminal`]**,
    /// as a process of its own; a no-op otherwise. It starts a session --
    /// no controlling terminal, the state of an app the Dock started --
    /// opens the subsidiary the way it is told, and says whether `/dev/tty`
    /// opens after: whether it took the subsidiary as its terminal.
    #[test]
    fn ctty_helper() {
        let Ok(arg) = std::env::var(CTTY_HELPER) else {
            return;
        };
        let (path, how) = arg.split_once('\t').expect("a path and a mode");
        // SAFETY: no arguments; a process that is not a group leader.
        assert!(unsafe { crate::sys::setsid() } >= 0, "a new session");
        assert!(
            std::fs::File::open("/dev/tty").is_err(),
            "a new session has no terminal"
        );
        match how {
            "follow" => {
                let mut line = LineDiscipline {
                    subsidiary: Some(path.into()),
                    told: InputEncoding::Utf8,
                };
                let followed = line.follow(InputEncoding::Latin1);
                assert!(matches!(followed, Followed::Told), "{followed:?}");
            }
            _ => drop(std::fs::File::open(path).expect("the subsidiary opens")),
        }
        let took = std::fs::File::open("/dev/tty").is_ok();
        println!("CTTY[{took}]");
    }

    fn env_of(pairs: &[(&str, &str)]) -> Vec<(OsString, OsString)> {
        pairs
            .iter()
            .map(|(k, v)| (OsString::from(k), OsString::from(v)))
            .collect()
    }

    /// **The child goes home only when launchd started baud, and only to
    /// an absolute `HOME`**: under any other parent it stays where baud
    /// is, and so it does under launchd when `HOME` is missing, empty or
    /// relative. The first `HOME` is the one used, as `getenv` reads it.
    #[test]
    fn the_child_starts_home_only_under_launchd_and_an_absolute_home() {
        let home = |v: &str| StartIn::Dir(CString::new(v).expect("no NUL"));
        let with_home = env_of(&[("HOME", "/Users/u"), ("HOME", "/second")]);
        assert_eq!(start_in(Parent::Launchd, &with_home), home("/Users/u"));
        assert_eq!(start_in(Parent::Other, &with_home), StartIn::Inherited);
        for env in [
            env_of(&[]),
            env_of(&[("HOME", "")]),
            env_of(&[("HOME", "relative/home")]),
        ] {
            assert_eq!(
                start_in(Parent::Launchd, &env),
                StartIn::Inherited,
                "{env:?}"
            );
        }
    }

    /// **A child sent home is told so by its `PWD`**, which replaces any
    /// it would inherit; a child that stays keeps its environment exactly.
    #[test]
    fn a_child_sent_home_has_home_as_its_pwd() {
        let env = || {
            ["PWD=/where/baud/is", "BAUD_KEPT=yes", "OLDPWD=/before"]
                .map(|e| CString::new(e).expect("no NUL"))
                .to_vec()
        };
        let home = StartIn::Dir(CString::new("/Users/u").expect("no NUL"));
        assert_eq!(
            strings(&with_pwd(env(), &home)),
            ["BAUD_KEPT=yes", "OLDPWD=/before", "PWD=/Users/u"]
        );
        assert_eq!(with_pwd(env(), &StartIn::Inherited), env());
    }

    /// **The spawn changes the child's directory**, through
    /// [`Pty::fork_from`] on any platform with the parent given: told that
    /// launchd started baud, the shell starts in the `HOME` it was handed;
    /// told otherwise, where this test is. The control is that the two
    /// directories differ. `HOME` is a symbolic link, so the `PWD` the
    /// shell reports tells baud's `PWD` (the link's spelling) from one the
    /// shell found itself (the resolved one): a POSIX shell keeps a `PWD`
    /// from its environment that is "an absolute pathname of the current
    /// working directory", and otherwise sets it as `pwd -P` would print
    /// it (`[[source: posix-xcu-2]]`, 2.5.3).
    #[test]
    fn the_spawn_starts_the_child_where_its_parent_says() {
        let real = scratch_dir("given-home");
        let link = scratch_dir("given-link").join("home");
        std::os::unix::fs::symlink(&real, &link).expect("a link to home");
        let here = std::env::current_dir()
            .expect("a working directory")
            .canonicalize()
            .expect("it resolves");
        assert_ne!(real, here, "the control: two directories to tell apart");
        let size = WinSize {
            ws_col: 80,
            ws_row: 24,
        };
        let run = |started_by: Parent| -> Vec<String> {
            let parent = env_of(&[
                ("HOME", link.to_str().expect("a UTF-8 path")),
                ("PWD", "/where/baud/is"),
                ("LANG", "C"),
            ]);
            let master = Pty::fork_from(
                text_area(&size, Px::of(8), Px::of(16)),
                Some("/bin/pwd -P; printf 'PWD=%s\\n' \"$PWD\"".into()),
                parent,
                || None,
                started_by,
                None,
            )
            .expect("forkpty");
            String::from_utf8_lossy(&read_all(master))
                .lines()
                .map(|l| l.trim_end_matches('\r').to_string())
                .collect()
        };
        assert_eq!(
            run(Parent::Launchd),
            [
                real.display().to_string(),
                format!("PWD={}", link.display())
            ]
        );
        assert_eq!(
            run(Parent::Other)[0],
            here.display().to_string(),
            "where this test is"
        );
        for d in [real, link.parent().expect("its directory").into()] {
            let _ = std::fs::remove_dir_all(d);
        }
    }

    /// **A new window's shell starts in the directory it inherits**
    /// (`windows::inherited`), whoever started baud, with `PWD` naming it;
    /// a relative one is not taken, and the launch rule stands. The
    /// directory is a symbolic link, so the `PWD` the shell reports tells
    /// baud's spelling from the one `pwd -P` resolves, and the control is
    /// that the inherited directory is not where this test is.
    #[test]
    fn a_new_windows_shell_starts_in_the_directory_it_inherits() {
        let real = scratch_dir("inherited");
        let link = scratch_dir("inherited-link").join("dir");
        std::os::unix::fs::symlink(&real, &link).expect("a link to it");
        let here = std::env::current_dir()
            .expect("a working directory")
            .canonicalize()
            .expect("it resolves");
        assert_ne!(real, here, "the control: two directories to tell apart");
        let size = WinSize {
            ws_col: 80,
            ws_row: 24,
        };
        let run = |started_by: Parent, at: &std::path::Path| -> Vec<String> {
            let parent = env_of(&[
                ("HOME", "/nowhere/home"),
                ("PWD", "/where/baud/is"),
                ("LANG", "C"),
            ]);
            let master = Pty::fork_from(
                text_area(&size, Px::of(8), Px::of(16)),
                Some("/bin/pwd -P; printf 'PWD=%s\\n' \"$PWD\"".into()),
                parent,
                || None,
                started_by,
                Some(at),
            )
            .expect("forkpty");
            String::from_utf8_lossy(&read_all(master))
                .lines()
                .map(|l| l.trim_end_matches('\r').to_string())
                .collect()
        };
        let want = [
            real.display().to_string(),
            format!("PWD={}", link.display()),
        ];
        assert_eq!(run(Parent::Other, &link), want);
        assert_eq!(run(Parent::Launchd, &link), want, "over HOME");
        assert_eq!(
            run(Parent::Other, std::path::Path::new("relative"))[0],
            here.display().to_string(),
            "a relative directory is not inherited"
        );
        for d in [real, link.parent().expect("its directory").into()] {
            let _ = std::fs::remove_dir_all(d);
        }
    }

    /// **A window's close hangs up its shell**, as baud's exit does: the
    /// window's `Pty` drops, its descriptor closes and the pipe it holds
    /// ends the reader, which closes the other -- and the master's last
    /// close sends the shell `SIGHUP`. The shell traps it and writes to a
    /// FIFO, whose open waits until the trap runs: a pass is never timed,
    /// and the bound on each wait only turns a regression's hang into a
    /// failure. The control is the shell answering through the reader
    /// before the close.
    #[cfg(target_os = "macos")]
    #[test]
    fn a_closed_windows_pty_hangs_up_its_shell() {
        let scratch = scratch_dir("hang-up");
        let fifo = scratch.join("hup");
        let made = std::process::Command::new("/usr/bin/mkfifo")
            .arg(&fifo)
            .status()
            .expect("mkfifo runs");
        assert!(made.success(), "a FIFO");
        let size = WinSize {
            ws_col: 80,
            ws_row: 24,
        };
        let told = text_area(&size, Px::of(8), Px::of(16));
        let master = Pty::fork_with(
            told,
            Some(format!(
                "/bin/stty -echo; trap 'printf hup > {}; exit 0' HUP; echo ready; while :; do read x; echo \"got $x\"; done",
                fifo.display()
            )),
            child_env(outer_parent(), None),
        )
        .expect("forkpty");
        read_until(master, "ready\r\n");
        make_nonblocking(master);
        let (tx, rx) = channel();
        let signalled = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&signalled);
        let (held, closing) = Reading::of(master);
        assert!(
            closing.is_some(),
            "the control: the reader has its own copy"
        );
        let (ended, reader_ended) = channel();
        std::thread::spawn(move || {
            reader(held, tx, flag, || {});
            let _ = ended.send(());
        });
        let opened = fifo.clone();
        let (trapped, heard) = channel();
        std::thread::spawn(move || {
            let _ = trapped.send(std::fs::read_to_string(&opened));
        });
        let bound = std::time::Duration::from_secs(60);
        let pty = Pty {
            master,
            child: None,
            told,
            line: LineDiscipline::of(master),
            rx,
            signalled,
            closing,
        };
        // The control: the reader's copy alone keeps the line up, so the
        // shell still answers through it once a write reaches it.
        let mut out = Outbox::default();
        out.push(b"one\n");
        assert_eq!(write_some(pty.master, &mut out), Wrote::All);
        let mut echoed = Vec::new();
        while !echoed.ends_with(b"got one\r\n") {
            let read = pty
                .rx
                .recv_timeout(bound)
                .expect("the reader passes the answer on");
            echoed.extend_from_slice(&read.bytes);
        }
        assert_eq!(String::from_utf8_lossy(&echoed), "got one\r\n");
        drop(pty);
        reader_ended
            .recv_timeout(bound)
            .expect("the reader ends with the window");
        let heard = heard.recv_timeout(bound).expect("the shell was hung up");
        assert_eq!(heard.expect("the trap writes the FIFO"), "hup");
        let _ = std::fs::remove_dir_all(scratch);
    }

    /// **A child whose `chdir` fails keeps the environment of one that
    /// stayed**, so its `PWD` never names a `HOME` it is not in: told that
    /// launchd started baud, with an absolute `HOME` that does not exist,
    /// the shell starts where this test is and is handed the parent's
    /// `PWD`. That `PWD` is a symbolic link to this directory, and a POSIX
    /// shell keeps a `PWD` from its environment only when it is "an
    /// absolute pathname of the current working directory" (`[[source:
    /// posix-xcu-2]]`, 2.5.3), so the shell prints the link's spelling only
    /// if it was given the stay environment; given `PWD=$HOME` it would
    /// find its own and print the resolved one. The control is that the
    /// two spellings differ.
    #[test]
    fn a_failed_chdir_hands_the_shell_the_environment_of_one_that_stayed() {
        let scratch = scratch_dir("failed-chdir");
        let missing = scratch.join("missing-home");
        assert!(!missing.exists(), "the control: HOME names nothing");
        let here = std::env::current_dir()
            .expect("a working directory")
            .canonicalize()
            .expect("it resolves");
        let link = scratch.join("here");
        std::os::unix::fs::symlink(&here, &link).expect("a link to here");
        assert_ne!(link, here, "the control: two spellings to tell apart");
        let size = WinSize {
            ws_col: 80,
            ws_row: 24,
        };
        let parent = env_of(&[
            ("HOME", missing.to_str().expect("a UTF-8 path")),
            ("PWD", link.to_str().expect("a UTF-8 path")),
            ("LANG", "C"),
        ]);
        let master = Pty::fork_from(
            text_area(&size, Px::of(8), Px::of(16)),
            Some("/bin/pwd -P; printf 'PWD=%s\\n' \"$PWD\"".into()),
            parent,
            || None,
            Parent::Launchd,
            None,
        )
        .expect("forkpty");
        let lines: Vec<String> = String::from_utf8_lossy(&read_all(master))
            .lines()
            .map(|l| l.trim_end_matches('\r').to_string())
            .collect();
        assert_eq!(
            lines,
            [
                here.display().to_string(),
                format!("PWD={}", link.display())
            ],
            "where baud is, with the PWD it would have had"
        );
        let _ = std::fs::remove_dir_all(scratch);
    }

    /// The variable that turns [`spawn_helper`] on: `orphan` to wait until
    /// launchd is its parent first, anything else to spawn at once.
    const SPAWN_HELPER: &str = "BAUD_TEST_SPAWN_HELPER";

    /// **Run only by [`a_shell_spawned_under_launchd_starts_home`]**, as a
    /// process of its own; a no-op otherwise. It spawns a shell the way
    /// the app does, [`Pty::fork`] with the process's own environment and
    /// parent, and prints its parent's ID and what the shell printed.
    ///
    /// As an orphan it first waits for its parent to be launchd: the `sh`
    /// that starts it exits at once, and POSIX gives the children of a
    /// process that exits to "an implementation-defined system process"
    /// (`[[source: posix-exit]]`), which on macOS is launchd, process 1
    /// (measured with `ps`).
    #[test]
    fn spawn_helper() {
        let Ok(how) = std::env::var(SPAWN_HELPER) else {
            return;
        };
        if how == "orphan" {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
            while std::os::unix::process::parent_id() != 1 {
                assert!(
                    std::time::Instant::now() < deadline,
                    "never adopted by launchd"
                );
                std::thread::yield_now();
            }
        }
        println!("PPID[{}]", std::os::unix::process::parent_id());
        let size = WinSize {
            ws_col: 80,
            ws_row: 24,
        };
        let (_, master) = Pty::fork(
            text_area(&size, Px::of(8), Px::of(16)),
            None,
            crate::shell::Integration::Off,
        )
        .expect("forkpty");
        for line in String::from_utf8_lossy(&read_all(master)).lines() {
            println!("CHILD[{}]", line.trim_end_matches('\r'));
        }
    }

    /// A fresh directory of this test's own, by its resolved path.
    fn scratch_dir(tag: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("baud-start-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).expect("a scratch directory");
        d.canonicalize().expect("it resolves")
    }

    /// **A shell spawned under launchd starts in the home directory**, and one
    /// spawned under any other parent starts where baud was started: Ghostty's
    /// `getppid()` test. The whole spawn, [`Pty::fork`], runs in
    /// [`spawn_helper`] twice, from the same directory with the same `HOME`:
    /// once as this test's child, and once as an orphan, whose parent is then
    /// launchd for real -- which is what a Dock or Finder launch's parent is
    /// (`[[source: ai-obs-fdprobe-run]]`, ppid 1 under `open`). The two
    /// directories differ, so either answer can be told from the other.
    ///
    /// `HOME` is a symbolic link to the directory, as `/var` is on macOS,
    /// so the shell's `PWD` -- which a POSIX shell keeps from its
    /// environment only when it names the working directory (`[[source:
    /// posix-xcu-2]]`, 2.5.3) -- says whether baud gave the shell `HOME` as
    /// its `PWD`, by the link's spelling, or the shell found its own, by
    /// the resolved one.
    #[cfg(target_os = "macos")]
    #[test]
    fn a_shell_spawned_under_launchd_starts_home() {
        let real = scratch_dir("home");
        let link = scratch_dir("link").join("home");
        std::os::unix::fs::symlink(&real, &link).expect("a link to home");
        let launch = scratch_dir("launch");
        assert_ne!(real, launch, "the control: two directories to tell apart");
        let exe = std::env::current_exe().expect("this test");
        let run = |how: &str| -> (String, Vec<String>) {
            let mut cmd = match how {
                "orphan" => {
                    let mut c = std::process::Command::new("/bin/sh");
                    c.args([
                        "-c",
                        "\"$0\" --exact pty::tests::spawn_helper --nocapture & exit 0",
                    ])
                    .arg(&exe);
                    c
                }
                _ => {
                    let mut c = std::process::Command::new(&exe);
                    c.args(["--exact", "pty::tests::spawn_helper", "--nocapture"]);
                    c
                }
            };
            cmd.current_dir(&launch)
                .env("HOME", &link)
                .env("PWD", &launch)
                .env("BAUD_CMD", "/bin/pwd -P; printf 'PWD=%s\\n' \"$PWD\"")
                .env(SPAWN_HELPER, how);
            let out = cmd.output().expect("the helper runs");
            let text = String::from_utf8_lossy(&out.stdout).into_owned();
            assert!(out.status.success(), "the helper passed: {text}");
            let ppid = text
                .lines()
                .find_map(|l| l.strip_prefix("PPID[")?.strip_suffix(']'))
                .unwrap_or_else(|| panic!("the helper spawned nothing: {text}"))
                .to_string();
            let child = text
                .lines()
                .filter_map(|l| l.strip_prefix("CHILD[")?.strip_suffix(']'))
                .map(str::to_string)
                .collect();
            (ppid, child)
        };
        let (ppid, child) = run("child");
        assert_eq!(ppid, std::process::id().to_string(), "the control's parent");
        assert_eq!(
            child,
            [
                launch.display().to_string(),
                format!("PWD={}", launch.display())
            ],
            "under this test, the launch directory"
        );
        let (ppid, child) = run("orphan");
        assert_eq!(ppid, "1", "the orphan's parent is launchd");
        assert_eq!(
            child,
            [
                real.display().to_string(),
                format!("PWD={}", link.display())
            ],
            "under launchd, home"
        );
        for d in [real, link.parent().expect("its directory").into(), launch] {
            let _ = std::fs::remove_dir_all(d);
        }
    }

    /// **Both drivers follow the encoding straight after they feed the
    /// child's output.** The pumps are bound to AppKit and to Wayland, so
    /// the source is held: in each, the statement after the one
    /// `reads::feed` of a read is the follow, so no key can be encoded
    /// between a switch and the bit that follows it.
    #[test]
    fn both_drivers_follow_the_encoding_straight_after_the_feed() {
        let next_after = |src: &str, feed: &str, follow: &str| {
            assert_eq!(src.matches(feed).count(), 1, "one feed: {feed}");
            assert_eq!(src.matches(follow).count(), 1, "one follow: {follow}");
            let rest = &src[src.find(feed).expect("found") + feed.len()..];
            let next = rest
                .lines()
                .map(str::trim)
                .find(|l| !l.is_empty() && !l.starts_with("//"))
                .expect("a statement after the feed");
            assert_eq!(next, follow);
        };
        // Each callback ends taking the notifications, and the log is the
        // feed's last argument.
        let end = "t.clear_notices();\n            },\n";
        next_after(
            include_str!("app.rs"),
            &format!("{end}            hostlog,\n        );\n"),
            "self.app.pty.follow(self.app.term.input_encoding());",
        );
        next_after(
            include_str!("linux.rs"),
            &format!("{end}            &mut self.hostlog,\n        );\n"),
            "self.pty.follow(self.term.input_encoding());",
        );
    }

    /// A pseudo-terminal no session has held: its manager, which the
    /// caller closes. `posix_openpt` with `O_NOCTTY`, so this process does
    /// not take it either.
    fn unheld_pty() -> crate::sys::Fd {
        // SAFETY: plain calls on the descriptor the first returns.
        unsafe {
            let m =
                redriven(|| crate::sys::posix_openpt(crate::sys::O_RDWR | crate::sys::O_NOCTTY));
            assert!(m >= 0, "posix_openpt");
            assert_eq!(crate::sys::grantpt(m), 0, "grantpt");
            assert_eq!(crate::sys::unlockpt(m), 0, "unlockpt");
            m
        }
    }

    /// **The parent never takes the subsidiary as its controlling
    /// terminal.** A session leader with none -- an app the Dock started --
    /// takes a terminal it opens without `O_NOCTTY` when no session holds
    /// that terminal, and with it the terminal's hang-up. A process in a
    /// new session ([`ctty_helper`]) follows the encoding on the subsidiary
    /// of a pty no session holds and has no terminal after. The control
    /// opens the same path plainly and has it as its terminal after, so the
    /// probe can fire. This process keeps the subsidiary open across both,
    /// with `O_NOCTTY`, as the child keeps it in the app: on Darwin the
    /// helper's close is otherwise the last, and the last close lets the
    /// terminal go, so a control without the hold read nothing taken. Not
    /// a `forkpty` pty: its child's session holds the subsidiary from the
    /// start, and on Darwin that hold outlives the child -- a plain open
    /// after the child had exited, reaped or not, took nothing.
    #[test]
    fn the_parent_never_takes_the_subsidiary_as_its_terminal() {
        let took = |how: &str| {
            let master = unheld_pty();
            let path = subsidiary_of(master).expect("a subsidiary");
            let held = std::fs::OpenOptions::new()
                .read(true)
                .custom_flags_noctty()
                .open(&path)
                .expect("the subsidiary opens");
            let out = std::process::Command::new(std::env::current_exe().expect("this test"))
                .args(["--exact", "pty::tests::ctty_helper", "--nocapture"])
                .env(CTTY_HELPER, format!("{}\t{how}", path.display()))
                .output()
                .expect("the helper runs");
            drop(held);
            // SAFETY: `master` is the descriptor `posix_openpt` returned
            // and nothing else owns it.
            drop(unsafe { <std::os::fd::OwnedFd as std::os::fd::FromRawFd>::from_raw_fd(master) });
            let text = String::from_utf8_lossy(&out.stdout).into_owned();
            assert!(out.status.success(), "the helper passed: {text}");
            match (text.contains("CTTY[true]"), text.contains("CTTY[false]")) {
                (true, false) => (true, text),
                (false, true) => (false, text),
                _ => panic!("the helper ran once and said: {text}"),
            }
        };
        let (plain, said) = took("plain");
        assert!(plain, "the control: a plain open takes it: {said}");
        let (follow, said) = took("follow");
        assert!(!follow, "{said}");
    }

    /// `outer_parent()` with `extra` added.
    fn parent_with(extra: &[(&str, &str)]) -> Vec<(OsString, OsString)> {
        let mut parent = outer_parent();
        parent.extend(
            extra
                .iter()
                .map(|(k, v)| (OsString::from(k), OsString::from(v))),
        );
        parent
    }

    fn langs(env: &[CString]) -> Vec<String> {
        strings(env)
            .into_iter()
            .filter(|e| e.starts_with("LANG="))
            .collect()
    }

    /// **A shell that inherited no locale is given the one baud found**,
    /// once, beside everything else as before. The name is `he_IL.UTF-8` so
    /// that neither this Mac's locale nor a test runner's own `LANG` can pass
    /// it by accident.
    #[test]
    fn a_child_with_no_locale_is_given_the_one_baud_found() {
        let mut want = ours();
        want.push("BAUD_KEPT=yes".into());
        want.push("HOME=/nowhere".into());
        want.push("LANG=he_IL.UTF-8".into());
        want.sort();
        assert_eq!(
            strings(&child_env(outer_parent(), Some("he_IL.UTF-8"))),
            want
        );
    }

    /// A locale the parent named wins: its `LANG`, or an `LC_ALL` or
    /// `LC_CTYPE`, which outrank `LANG`, leaves the environment as given.
    #[test]
    fn a_locale_the_parent_set_is_kept() {
        let env = child_env(parent_with(&[("LANG", "fr_FR.UTF-8")]), Some("he_IL.UTF-8"));
        assert_eq!(langs(&env), ["LANG=fr_FR.UTF-8"]);
        for (k, v) in [("LC_ALL", "he_IL.UTF-8"), ("LC_CTYPE", "UTF-8")] {
            let env = child_env(parent_with(&[(k, v)]), Some("he_IL.UTF-8"));
            assert!(
                langs(&env).is_empty(),
                "{k} named a locale: {:?}",
                strings(&env)
            );
            assert!(strings(&env).contains(&format!("{k}={v}")), "{k} kept");
        }
    }

    /// An empty variable is no locale: an empty `LANG` is replaced, never
    /// doubled -- `getenv` reads the first entry, which would be the empty
    /// one -- and empty `LC_ALL` and `LC_CTYPE` do not stop the fill.
    #[test]
    fn an_empty_locale_variable_is_replaced_not_duplicated() {
        let env = child_env(parent_with(&[("LANG", "")]), Some("he_IL.UTF-8"));
        assert_eq!(langs(&env), ["LANG=he_IL.UTF-8"]);
        let env = child_env(
            parent_with(&[("LC_ALL", ""), ("LC_CTYPE", "")]),
            Some("he_IL.UTF-8"),
        );
        assert_eq!(langs(&env), ["LANG=he_IL.UTF-8"]);
    }

    /// iTerm2's `LC_TERMINAL` and `LC_TERMINAL_VERSION` name a terminal, not
    /// a locale: they go as outer identity and do not stop the fill.
    #[test]
    fn lc_terminal_is_a_terminal_not_a_locale() {
        let env = child_env(
            parent_with(&[("LC_TERMINAL", "iTerm2"), ("LC_TERMINAL_VERSION", "3.5")]),
            Some("he_IL.UTF-8"),
        );
        assert_eq!(langs(&env), ["LANG=he_IL.UTF-8"]);
        assert!(!strings(&env).iter().any(|e| e.starts_with("LC_TERMINAL")));
    }

    /// The spawn asks for a locale only when the parent named none -- the
    /// answer is CoreFoundation's, read from the user's preferences -- fills
    /// it then, and leaves the environment as before when none is found. An
    /// empty `LANG` and `LC_TERMINAL` alone name none; `LC_ALL` and
    /// `LC_CTYPE` do.
    #[test]
    fn the_spawn_asks_for_a_locale_only_when_the_parent_named_none() {
        let asked = |parent: Vec<(OsString, OsString)>| {
            let called = std::cell::Cell::new(false);
            let env = spawn_env(parent, || {
                called.set(true);
                Some("he_IL.UTF-8".into())
            });
            (called.get(), langs(&env))
        };
        let filled = vec!["LANG=he_IL.UTF-8".to_string()];
        assert_eq!(
            asked(parent_with(&[("LANG", "fr_FR.UTF-8")])),
            (false, vec!["LANG=fr_FR.UTF-8".to_string()])
        );
        assert_eq!(
            asked(parent_with(&[("LC_ALL", "he_IL.UTF-8")])),
            (false, vec![])
        );
        assert_eq!(
            asked(parent_with(&[("LC_CTYPE", "UTF-8")])),
            (false, vec![])
        );
        assert_eq!(asked(outer_parent()), (true, filled.clone()));
        assert_eq!(asked(parent_with(&[("LANG", "")])), (true, filled.clone()));
        assert_eq!(
            asked(parent_with(&[("LC_TERMINAL", "iTerm2")])),
            (true, filled)
        );
        assert_eq!(
            strings(&spawn_env(outer_parent(), || None)),
            strings(&child_env(outer_parent(), None))
        );
    }

    /// The names a shell's locale is tried as: the user's identifier
    /// without its `@` keywords, their language and region, then `en_US`
    /// and `C`, each as UTF-8 and each once. The first the system has wins;
    /// none, nothing.
    #[cfg(target_os = "macos")]
    #[test]
    fn the_candidates_run_from_the_users_locale_to_the_fallbacks() {
        assert_eq!(
            locale_candidates(Some("he_IL"), Some("he"), Some("IL")),
            ["he_IL.UTF-8", "en_US.UTF-8", "C.UTF-8"]
        );
        assert_eq!(
            locale_candidates(Some("en_US@rg=ilzzzz"), Some("en"), Some("IL")),
            ["en_US.UTF-8", "en_IL.UTF-8", "C.UTF-8"]
        );
        assert_eq!(
            locale_candidates(None, None, None),
            ["en_US.UTF-8", "C.UTF-8"]
        );
        let he_us = locale_candidates(Some("he_US"), Some("he"), Some("US"));
        assert_eq!(
            first_valid(&he_us, |n| n != "he_US.UTF-8"),
            Some("en_US.UTF-8".into())
        );
        assert_eq!(first_valid(&he_us, |_| false), None);
    }

    /// Only a UTF-8 locale the system has is handed over. Refused: an empty
    /// name (which `newlocale` reads as the environment's), C and POSIX
    /// (ASCII), names the system lacks, a bare `en_US` (no codeset), and
    /// `UTF-8`, which the character type alone would accept. The positive
    /// controls are names every Mac has.
    #[cfg(target_os = "macos")]
    #[test]
    fn only_a_utf8_locale_the_system_has_is_accepted() {
        for bad in [
            "",
            "C",
            "POSIX",
            "bogus_XX.UTF-8",
            "en_IL.UTF-8",
            "en_US",
            "UTF-8",
            "en_US.utf8",
        ] {
            assert!(!is_utf8_locale(bad), "{bad:?} was accepted");
        }
        for good in ["en_US.UTF-8", "he_IL.UTF-8", "C.UTF-8"] {
            assert!(is_utf8_locale(good), "{good} was refused");
        }
        let he_us = locale_candidates(Some("he_US"), Some("he"), Some("US"));
        assert_eq!(
            first_valid(&he_us, is_utf8_locale),
            Some("en_US.UTF-8".into())
        );
    }

    /// On this Mac the user's locale is found, and it is one the system has.
    #[cfg(target_os = "macos")]
    #[test]
    fn the_users_locale_is_found_and_is_utf8() {
        let found = shell_locale().expect("en_US.UTF-8 and C.UTF-8 are on every Mac");
        assert!(is_utf8_locale(&found), "{found}");
    }

    /// Everything a forked child writes, until it exits.
    fn read_all(master: crate::sys::Fd) -> Vec<u8> {
        let mut out = Vec::new();
        let mut buf = [0u8; 4096];
        loop {
            // SAFETY: `buf` is a live, writable buffer of the length passed.
            let n = unsafe { crate::sys::read(master, buf.as_mut_ptr().cast(), buf.len()) };
            let Ok(n) = usize::try_from(n) else { break };
            if n == 0 {
                break;
            }
            out.extend_from_slice(&buf[..n]);
        }
        out
    }

    /// **A shell a pty forks counts a Hebrew letter as one character**, and
    /// names the locale it was given -- through the whole spawn,
    /// [`Pty::fork_from`], from a parent that named none. Without a locale
    /// (the control) the same command counts the letter's two bytes, so the
    /// probe can fire.
    #[cfg(target_os = "macos")]
    #[test]
    fn a_shell_spawned_without_a_locale_counts_a_hebrew_letter_as_one() {
        let size = WinSize {
            ws_col: 80,
            ws_row: 24,
        };
        let run = |locale: Option<&str>| {
            let locale = locale.map(str::to_string);
            let master = Pty::fork_from(
                text_area(&size, Px::of(8), Px::of(16)),
                Some(r#"printf '%s|' "$LANG"; printf '\327\231' | /usr/bin/wc -m"#.into()),
                outer_parent(),
                move || locale,
                Parent::Other,
                None,
            )
            .expect("forkpty");
            String::from_utf8_lossy(&read_all(master)).replace(char::is_whitespace, "")
        };
        assert_eq!(run(Some("he_IL.UTF-8")), "he_IL.UTF-8|1");
        assert_eq!(
            run(None),
            "|2",
            "the control: with no locale the letter is two bytes"
        );
    }

    #[cfg(target_os = "macos")]
    fn holds(haystack: &[u8], needle: &[u8]) -> bool {
        haystack.windows(needle.len()).any(|w| w == needle)
    }

    /// **zsh echoes a typed Hebrew letter whole** -- the reported symptom at
    /// its source. zsh runs on the pty with no rc file; once its line editor
    /// is reading (it turns bracketed paste on, `CSI ? 2004 h`, as it starts
    /// each line -- zsh takes no prompt from the environment, so the prompt
    /// cannot be the marker), the letter's two bytes are typed, then Return
    /// (zsh answers that no such command exists) and `exit`. With a locale
    /// the echo carries `d7 99` and no `<0099>`; without one (the control)
    /// zsh prints `<0099>`, as it did on the screen it was reported from.
    #[cfg(target_os = "macos")]
    #[test]
    fn zsh_echoes_a_hebrew_letter_whole() {
        let echo = |locale: Option<&str>| -> Vec<u8> {
            let size = WinSize {
                ws_col: 80,
                ws_row: 24,
            };
            let locale = locale.map(str::to_string);
            let master = Pty::fork_from(
                text_area(&size, Px::of(8), Px::of(16)),
                Some("exec /bin/zsh -f -i".into()),
                outer_parent(),
                move || locale,
                Parent::Other,
                None,
            )
            .expect("forkpty");
            let (tx, rx) = channel::<Vec<u8>>();
            let reader = std::thread::spawn(move || {
                let mut buf = [0u8; 4096];
                loop {
                    // SAFETY: `buf` is a live, writable buffer of the length passed.
                    let n = unsafe { crate::sys::read(master, buf.as_mut_ptr().cast(), buf.len()) };
                    let Ok(n) = usize::try_from(n) else { break };
                    if n == 0 || tx.send(buf[..n].to_vec()).is_err() {
                        break;
                    }
                }
            });
            let mut seen = Vec::new();
            let wait_for = |seen: &mut Vec<u8>, needle: &[u8], from: usize| {
                let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
                while !holds(&seen[from.min(seen.len())..], needle) {
                    let left = deadline.saturating_duration_since(std::time::Instant::now());
                    match rx.recv_timeout(left) {
                        Ok(chunk) => seen.extend(chunk),
                        Err(_) => panic!(
                            "zsh never wrote {:?}: {:?}",
                            String::from_utf8_lossy(needle),
                            String::from_utf8_lossy(seen)
                        ),
                    }
                }
            };
            let typed = |bytes: &[u8]| {
                // SAFETY: `bytes` is a live buffer of the length passed.
                let n = unsafe { crate::sys::write(master, bytes.as_ptr().cast(), bytes.len()) };
                assert_eq!(
                    usize::try_from(n).ok(),
                    Some(bytes.len()),
                    "the pty took the bytes"
                );
            };
            const READING: &[u8] = b"\x1b[?2004h";
            wait_for(&mut seen, READING, 0);
            let first_line = seen.len();
            typed(b"\xd7\x99\r");
            wait_for(&mut seen, READING, first_line);
            typed(b"exit\r");
            for chunk in rx {
                seen.extend(chunk);
            }
            reader.join().expect("the reader");
            seen
        };
        let with = echo(Some("he_IL.UTF-8"));
        assert!(
            holds(&with, b"\xd7\x99"),
            "the letter's bytes: {:?}",
            String::from_utf8_lossy(&with)
        );
        assert!(
            !holds(&with, b"<0099>"),
            "zsh escaped the letter: {:?}",
            String::from_utf8_lossy(&with)
        );
        let without = echo(None);
        assert!(
            holds(&without, b"<0099>"),
            "the control: with no locale zsh prints <0099>: {:?}",
            String::from_utf8_lossy(&without)
        );
    }

    /// **The foreground is read off the master: the program and its
    /// folder**, from another session, as the app reads its own child
    /// (`fg.rs`). The child `cd`s into a fresh folder, says it is ready
    /// and waits for a line: the group is its own, `sh` by argv[0], in
    /// that folder -- canonical, as the kernel answers it. On macOS a
    /// line lets it exec `sleep`: the exec is a kqueue event (`fg::Watch`),
    /// waited on with a bound and never a sleep, after which the same
    /// group reads `sleep`, and a [`crate::title::Derive`] that heard the
    /// event reads the new name where one that did not keeps the old.
    #[test]
    fn the_foreground_reads_name_the_program_and_its_folder() {
        let size = WinSize {
            ws_col: 80,
            ws_row: 24,
        };
        let made = std::env::temp_dir().join(format!("baud-fg-{}", std::process::id()));
        std::fs::create_dir_all(&made).expect("a folder");
        let dir = made.canonicalize().expect("canonical");
        let cmd = format!(
            "cd '{}' && /bin/stty -echo && echo ready && read x && exec /bin/sleep 30",
            dir.display()
        );
        let master = Pty::fork_with(
            text_area(&size, Px::of(8), Px::of(16)),
            Some(cmd),
            child_env(outer_parent(), None),
        )
        .expect("forkpty");
        read_until(master, "ready\r\n");
        let g = crate::fg::group(master).expect("the child's group");
        assert_eq!(crate::fg::name(g).as_deref(), Some("sh"));
        assert_eq!(crate::fg::dir(g).as_deref(), Some(dir.as_path()));
        let reads = crate::fg::Live { master };
        let (mut heard, mut deaf) = (crate::title::Derive::new(), crate::title::Derive::new());
        heard.check(&reads);
        deaf.check(&reads);
        let home = Some(&b"/nowhere"[..]);
        let folder = crate::title::folder_text(
            std::os::unix::ffi::OsStrExt::as_bytes(dir.as_os_str()),
            home,
        );
        assert_eq!(
            heard.title(None, &[], home),
            Some(format!("{folder} \u{2014} sh"))
        );
        #[cfg(target_os = "macos")]
        {
            let (tx, rx) = std::sync::mpsc::channel();
            let w = crate::fg::Watch::start(move || {
                let _ = tx.send(());
            })
            .expect("a kqueue");
            w.add(g);
            send(master, b"\n");
            rx.recv_timeout(std::time::Duration::from_secs(10))
                .expect("the exec is an event");
            assert_eq!(w.take(), crate::fg::Moved::Moved);
            assert_eq!(crate::fg::group(master), Some(g), "the same group");
            assert_eq!(crate::fg::name(g).as_deref(), Some("sleep"));
            heard.moved();
            deaf.owe();
            assert_eq!(heard.check(&reads), crate::title::Changed::Yes);
            assert_eq!(
                heard.title(None, &[], home),
                Some(format!("{folder} \u{2014} sleep"))
            );
            deaf.check(&reads);
            assert_eq!(
                deaf.title(None, &[], home),
                Some(format!("{folder} \u{2014} sh")),
                "the control: no event, no new name"
            );
        }
        // SAFETY: `master` is the descriptor forkpty returned and nothing
        // else owns it; closing it hangs the child up.
        drop(unsafe { <std::os::fd::OwnedFd as std::os::fd::FromRawFd>::from_raw_fd(master) });
        let _ = std::fs::remove_dir(&made);
    }

    /// Blocking reads of `master` until what arrived ends with `want`, and
    /// what arrived. The master must still be blocking.
    fn read_until(master: crate::sys::Fd, want: &str) -> Vec<u8> {
        let mut seen = Vec::new();
        let mut buf = [0u8; 256];
        while !seen.ends_with(want.as_bytes()) {
            match read_master(master, &mut buf) {
                Got::Bytes(b) => seen.extend_from_slice(b),
                Got::Nothing => {}
                Got::Closed => panic!(
                    "the child left before {want:?}: {:?}",
                    String::from_utf8_lossy(&seen)
                ),
            }
        }
        seen
    }

    /// **An idle child's empty master is not the end of the stream.** The
    /// reader took a read that returned no bytes as the child gone. Once the
    /// master is non-blocking -- for the main thread's writes, and the flag
    /// is the open file's -- an idle child's master answers `EAGAIN` at once,
    /// and the reader, and with it the window, would end the first time the
    /// child paused. Then the wait that replaces the blocking read: `poll`
    /// returns when the child writes, and the read after it has the bytes.
    #[test]
    fn an_idle_childs_master_reads_as_nothing_and_its_next_write_wakes_the_wait() {
        let size = WinSize {
            ws_col: 80,
            ws_row: 24,
        };
        let master = Pty::fork_with(
            text_area(&size, Px::of(8), Px::of(16)),
            Some("/bin/stty -echo; echo ready; read x; echo hi".into()),
            child_env(outer_parent(), None),
        )
        .expect("forkpty");
        // Parked in `read`, with nothing behind its `ready`.
        read_until(master, "ready\r\n");
        nonblocking(master).expect("O_NONBLOCK");
        let mut buf = [0u8; READ_CHUNK];
        assert!(
            matches!(read_master(master, &mut buf), Got::Nothing),
            "an idle child's empty master read as the end of the stream"
        );
        let mut out = Outbox::default();
        out.push(b"go\n");
        assert_eq!(write_some(master, &mut out), Wrote::All);
        let mut seen = Vec::new();
        loop {
            match read_master(master, &mut buf) {
                Got::Bytes(b) => seen.extend_from_slice(b),
                Got::Nothing => assert_eq!(readable(master), Waited::Ready),
                Got::Closed => break,
            }
        }
        assert_eq!(String::from_utf8_lossy(&seen), "hi\r\n");
        // SAFETY: `master` is the descriptor forkpty returned and nothing
        // else owns it.
        drop(unsafe { <std::os::fd::OwnedFd as std::os::fd::FromRawFd>::from_raw_fd(master) });
    }

    /// **A descriptor `poll` cannot watch is reported, not waited on again.**
    /// `POLLNVAL` returns at once, so a reader that took it as readiness
    /// would read, find nothing and wait again, forever, at full speed. A
    /// number no process opens is the descriptor here: `poll` answers it
    /// `POLLNVAL` on both platforms, and no other test thread can be handed
    /// it.
    #[test]
    fn a_descriptor_poll_cannot_watch_is_reported_rather_than_waited_on() {
        assert_eq!(readable(1 << 20), Waited::Unwatchable);
    }

    /// **A write the full queue takes nothing of says so**, so the activity
    /// lights' SD stays dark for bytes still owed (note AH, section 7): a
    /// raw child that never reads lets Darwin's master take 1022 bytes of
    /// 8192, a part, and the next write finds no room and takes none, every
    /// byte still owed.
    #[cfg(target_os = "macos")]
    #[test]
    fn a_write_the_full_queue_takes_nothing_of_says_so() {
        let size = WinSize {
            ws_col: 80,
            ws_row: 24,
        };
        let master = Pty::fork_with(
            text_area(&size, Px::of(8), Px::of(16)),
            Some("/bin/stty raw -echo; echo ready; exec /bin/sleep 30".into()),
            child_env(outer_parent(), None),
        )
        .expect("forkpty");
        read_until(master, "ready\n");
        nonblocking(master).expect("O_NONBLOCK");
        let mut out = Outbox::default();
        out.push(&[b'x'; 8192]);
        assert_eq!(write_some(master, &mut out), Wrote::Part);
        let owed = out.bytes.len();
        assert!(owed > 0 && owed < 8192, "{owed} owed");
        assert_eq!(write_some(master, &mut out), Wrote::Nothing);
        assert_eq!(out.bytes.len(), owed, "nothing taken, nothing lost");
        // SAFETY: `master` is the descriptor forkpty returned and nothing
        // else owns it; closing it hangs the sleeping child up.
        drop(unsafe { <std::os::fd::OwnedFd as std::os::fd::FromRawFd>::from_raw_fd(master) });
    }

    /// **A write the queue cannot hold returns, and what it could not take
    /// stays owed, in order.** A raw child that never reads let Darwin's
    /// master take 1022 bytes and then answered `EAGAIN` (measured
    /// 2026-10-09), and the blocking master waited in `write(2)` for the rest
    /// -- on the window's thread. Here 8192 bytes go to a raw child that
    /// echoes what it reads, through the outbox, offered again whenever
    /// `poll` says there is room: every byte comes back once and in order,
    /// and on Darwin the first write is short, so the owed path is the one
    /// taken.
    #[test]
    fn a_full_queue_keeps_the_rest_owed_in_order_and_never_waits() {
        let size = WinSize {
            ws_col: 80,
            ws_row: 24,
        };
        let master = Pty::fork_with(
            text_area(&size, Px::of(8), Px::of(16)),
            Some("/bin/stty raw -echo; echo ready; exec /usr/bin/head -c 8192".into()),
            child_env(outer_parent(), None),
        )
        .expect("forkpty");
        // Raw: no output processing, so no carriage return.
        read_until(master, "ready\n");
        nonblocking(master).expect("O_NONBLOCK");
        // Printable, so no line discipline setting can read a byte as a
        // control.
        let payload: Vec<u8> = (b' '..=b'~').cycle().take(8192).collect();
        let mut out = Outbox::default();
        out.push(&payload);
        let first = write_some(master, &mut out);
        #[cfg(target_os = "macos")]
        assert_eq!(
            first,
            Wrote::Part,
            "Darwin's queue holds 1022 bytes, so 8192 do not go in one write; {} were taken",
            payload.len() - out.bytes.len()
        );
        let _ = first;
        let mut echoed = Vec::new();
        let mut buf = [0u8; READ_CHUNK];
        loop {
            let mut p = crate::sys::PollFd {
                fd: master,
                events: match out.is_empty() {
                    true => crate::sys::POLLIN,
                    false => crate::sys::POLLIN | crate::sys::POLLOUT,
                },
                revents: 0,
            };
            // A bound on a hang, not a timing: a pass never comes near it.
            // SAFETY: one initialised record and its count.
            let n = unsafe { crate::sys::poll(&mut p, 1, 20_000) };
            assert!(
                n > 0,
                "nothing moved in 20 s: {} echoed, {} owed",
                echoed.len(),
                out.bytes.len()
            );
            if p.revents & crate::sys::POLLOUT != 0 {
                let _ = write_some(master, &mut out);
            }
            match read_master(master, &mut buf) {
                Got::Bytes(b) => echoed.extend_from_slice(b),
                Got::Nothing => {}
                Got::Closed => break,
            }
        }
        assert!(out.is_empty(), "{} bytes still owed", out.bytes.len());
        assert_eq!(echoed.len(), payload.len());
        assert!(echoed == payload, "every byte, once, in order");
        // SAFETY: as above.
        drop(unsafe { <std::os::fd::OwnedFd as std::os::fd::FromRawFd>::from_raw_fd(master) });
    }

    /// What the kernel holds for the pty behind `master` -- read back rather
    /// than trusted from `told`, so the tests that use it are about the
    /// kernel.
    fn ask(master: crate::sys::Fd) -> crate::sys::Winsize {
        let mut ws = crate::sys::Winsize::default();
        // SAFETY: TIOCGWINSZ writes one winsize through the pointer.
        let rc = unsafe { crate::sys::ioctl(master, crate::sys::TIOCGWINSZ, &mut ws) };
        assert_eq!(rc, 0, "TIOCGWINSZ failed");
        ws
    }

    /// **The pixels are the text area, from the cell -- not the window.** A
    /// child reads `ws_xpixel / ws_col` as a cell's width, so a window's
    /// width, which carries the padding and the part of a cell the grid does
    /// not fill, would inflate it.
    #[test]
    fn the_winsize_is_the_text_area_in_pixels() {
        let ws = text_area(
            &WinSize {
                ws_col: 80,
                ws_row: 24,
            },
            Px::of(17),
            Px::of(34),
        );
        assert_eq!(
            ws,
            crate::sys::Winsize {
                ws_row: 24,
                ws_col: 80,
                ws_xpixel: 1360,
                ws_ypixel: 816,
            }
        );
        // Saturating, not wrapping: 2048 columns of a 72-pixel cell is
        // 147,456 pixels, which the kernel's `u16` cannot hold.
        let wide = text_area(
            &WinSize {
                ws_col: 2048,
                ws_row: 10,
            },
            Px::of(72),
            Px::of(144),
        );
        assert_eq!((wide.ws_xpixel, wide.ws_ypixel), (u16::MAX, 1440));
    }

    /// **The kernel holds the pixels from the fork on, and hears a zoom that
    /// keeps the grid.** Before this, the fork carried zero pixels and the
    /// first resize after launch computed the grid the pty was spawned at,
    /// so nothing was sent and a child asking at startup -- yazi does --
    /// read zero until the window changed. Read back through `TIOCGWINSZ`
    /// on a real master, so it is the kernel's answer and not baud's
    /// bookkeeping.
    #[test]
    fn the_kernel_holds_the_text_area_from_the_fork_and_after_a_zoom() {
        let size = WinSize {
            ws_col: 80,
            ws_row: 24,
        };
        let first = text_area(&size, Px::of(17), Px::of(34));
        let master = Pty::fork_with(
            first,
            Some("exec /bin/cat".into()),
            child_env(outer_parent(), None),
        )
        .expect("forkpty");
        assert_eq!(ask(master), first, "the fork did not carry the pixels");

        // A zoom: the same grid, a bigger cell. Told.
        let mut told = first;
        let zoomed = text_area(&size, Px::of(20), Px::of(40));
        tell(master, &mut told, zoomed);
        assert_eq!(
            ask(master),
            zoomed,
            "a zoom that kept the grid was not sent"
        );
        assert_eq!(told, zoomed);

        // **The same size again sends nothing**, which is the property that
        // lets the app call this on every re-lay. The kernel's size is moved
        // out of band first, so a `tell` that did send would put it back.
        let elsewhere = text_area(&size, Px::of(1), Px::of(1));
        // SAFETY: TIOCSWINSZ takes a pointer to a winsize.
        unsafe {
            crate::sys::ioctl(master, crate::sys::TIOCSWINSZ, &elsewhere);
        }
        tell(master, &mut told, zoomed);
        assert_eq!(ask(master), elsewhere, "an unchanged size was sent again");

        // SAFETY: `master` is the descriptor forkpty returned and nothing
        // else owns it; closing it hangs up the child.
        drop(unsafe { <std::os::fd::OwnedFd as std::os::fd::FromRawFd>::from_raw_fd(master) });
    }
}
