//! The pty: a login shell on the slave, the master read on a thread on
//! macOS and polled beside the compositor's socket on Linux.
//!
//! The one place this binary calls libc: `forkpty`, `execve`, `read`,
//! `write`, `poll`, the `TIOCSWINSZ` ioctl, `mmap` for the font files, and
//! CoreGraphics for the display's scale before AppKit exists. On macOS the reader thread
//! owns the master's read side for the life of the process and pushes
//! every `read(2)` -- 4096 bytes at a time, which is what `N_TTY_BUF_SIZE`
//! and the Darwin clist hand back anyway -- to the main thread over a
//! channel; on Linux the main loop waits on the master itself and takes
//! each read where it is polled.
#![deny(unsafe_op_in_unsafe_fn)]

#[cfg(target_os = "macos")]
use baud_domain::HostTime;
use baud_domain::WinSize;
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
    /// The size the kernel was last told, so a `resize` that changes nothing
    /// sends no `SIGWINCH`: the app calls it on every re-lay, including the
    /// ones where only the pixels could have moved.
    told: crate::sys::Winsize,
    #[cfg(target_os = "macos")]
    rx: Receiver<Read>,
    /// Set by the reader when it has woken the main thread and not yet
    /// been drained, so a flood of reads sends one wake, not one a read.
    #[cfg(target_os = "macos")]
    signalled: Arc<AtomicBool>,
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

/// Whether a write reached the child whole.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Wrote {
    All,
    /// The kernel took none of it, or not all of it, and the rest is lost.
    Short,
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

/// The child's environment: the parent's, less every variable in
/// [`OUTER`] and every one baud sets, then baud's own in [`OURS`]. Built in
/// the parent, before the fork, so the child does nothing between `fork`
/// and `exec` but the exec -- no `putenv`, which may reallocate the
/// environment array in a process that was multi-threaded a moment ago.
fn child_env(parent: impl IntoIterator<Item = (OsString, OsString)>) -> Vec<CString> {
    let dropped = |k: &[u8]| {
        OUTER.iter().any(|o| o.as_bytes() == k) || OURS.iter().any(|(o, _)| o.as_bytes() == k)
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
    env
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
    fn fork(ws: crate::sys::Winsize) -> Option<crate::sys::Fd> {
        let cmd = std::env::var("BAUD_CMD").ok();
        Pty::fork_with(ws, cmd, child_env(std::env::vars_os()))
    }

    /// [`Pty::fork`] with the command and the environment given rather
    /// than read, so a test can hand the child a parent environment of its
    /// own and read back what the child saw.
    fn fork_with(
        ws: crate::sys::Winsize,
        cmd: Option<String>,
        env: Vec<CString>,
    ) -> Option<crate::sys::Fd> {
        let shell = match &cmd {
            Some(_) => "/bin/sh".to_string(),
            None => std::env::var("SHELL").unwrap_or_else(|_| DEFAULT_SHELL.to_string()),
        };
        let path = resolve(&shell).unwrap_or_else(|| DEFAULT_SHELL.to_string());
        let path_c = CString::new(path).ok()?;
        let argv0 = match &cmd {
            Some(_) => CString::new("sh").ok()?,
            None => CString::new(format!("-{}", shell.rsplit('/').next().unwrap_or("sh"))).ok()?,
        };
        let dash_c = CString::new("-c").ok()?;
        let cmd_c = cmd.and_then(|c| CString::new(c).ok());
        let argv = match &cmd_c {
            Some(c) => [
                argv0.as_ptr(),
                dash_c.as_ptr(),
                c.as_ptr(),
                std::ptr::null(),
            ],
            None => [
                argv0.as_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
            ],
        };
        let envp: Vec<*const core::ffi::c_char> = env
            .iter()
            .map(|e| e.as_ptr())
            .chain(std::iter::once(std::ptr::null()))
            .collect();
        // Darwin's `forkpty` takes the size by mutable pointer and
        // glibc's by const, so the binding is only mutated on one.
        #[cfg_attr(target_os = "linux", allow(unused_mut))]
        let mut ws = ws;
        let mut master: crate::sys::Fd = -1;
        // Linux's forkpty takes the window size by const pointer and
        // Darwin's by mutable pointer; the size is the same struct.
        #[cfg(target_os = "linux")]
        let ws_arg: *const crate::sys::Winsize = &raw const ws;
        #[cfg(not(target_os = "linux"))]
        let ws_arg: *mut crate::sys::Winsize = &raw mut ws;
        // SAFETY: forkpty is given a valid out-pointer and window size; in
        // the child only `execve` and `_exit` run, both async-signal-safe,
        // over pointers into memory built before the fork.
        let pid = unsafe {
            crate::sys::forkpty(
                &mut master,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                ws_arg,
            )
        };
        if pid < 0 {
            return None;
        }
        if pid == 0 {
            unsafe {
                crate::sys::execve(path_c.as_ptr(), argv.as_ptr(), envp.as_ptr());
                crate::sys::_exit(127);
            }
        }
        Some(master)
    }

    /// The pty with a thread reading it: each chunk crosses a channel and
    /// `wake` runs once per batch, so a main thread that cannot wait on a
    /// descriptor -- the macOS run loop -- learns there is something to
    /// drain.
    #[cfg(target_os = "macos")]
    pub fn spawn(
        size: &WinSize,
        cell_w: Px,
        cell_h: Px,
        wake: impl Fn() + Send + 'static,
    ) -> Option<Pty> {
        let told = text_area(size, cell_w, cell_h);
        let master = Pty::fork(told)?;
        let (tx, rx) = channel();
        let fd = master;
        let signalled = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&signalled);
        std::thread::spawn(move || reader(fd, tx, flag, wake));
        Some(Pty {
            master,
            told,
            rx,
            signalled,
        })
    }

    /// The pty with no thread: the caller waits on `as_fd` beside whatever
    /// else it waits on and takes each read with `read_chunk`, which is
    /// the Linux loop's shape -- one wait over the compositor's socket and
    /// the master, and nothing crossing a channel.
    #[cfg(target_os = "linux")]
    pub fn spawn_polled(size: &WinSize, cell_w: Px, cell_h: Px) -> Option<Pty> {
        let told = text_area(size, cell_w, cell_h);
        Some(Pty {
            master: Pty::fork(told)?,
            told,
        })
    }

    /// The master, to wait on.
    #[cfg(target_os = "linux")]
    pub fn as_fd(&self) -> BorrowedFd<'_> {
        // SAFETY: the master is open for as long as `self` is, and the
        // borrow cannot outlive `self`.
        unsafe { BorrowedFd::borrow_raw(self.master) }
    }

    /// One read of the master, taken after a wait said it was readable: at
    /// most `READ_CHUNK` bytes, empty when a signal interrupted the read,
    /// and `Closed` once the child has gone -- a Linux master answers a
    /// read with an error then, not with zero bytes.
    #[cfg(target_os = "linux")]
    pub fn read_chunk(&self) -> Chunk {
        let mut buf = vec![0u8; READ_CHUNK];
        // SAFETY: an open descriptor and a buffer of the length named.
        let n = unsafe { crate::sys::read(self.master, buf.as_mut_ptr().cast(), READ_CHUNK) };
        if n < 0 && std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted {
            return Chunk::Bytes(Vec::new());
        }
        if n <= 0 {
            return Chunk::Closed;
        }
        buf.truncate(usize::try_from(n).unwrap_or(0));
        Chunk::Bytes(buf)
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

    /// Writes `bytes` to the shell, all of them, and says whether they
    /// all went: a write the kernel refuses -- the slave's input queue full
    /// because the child stopped reading, or the child gone -- asks for no
    /// echo, and the keystroke instrument must not wait for one.
    pub fn write(&self, bytes: &[u8]) -> Wrote {
        let mut at = 0usize;
        while at < bytes.len() {
            // SAFETY: the slice is valid for its length and the fd is ours.
            let n = unsafe {
                crate::sys::write(self.master, bytes[at..].as_ptr().cast(), bytes.len() - at)
            };
            if n <= 0 {
                return Wrote::Short;
            }
            at += usize::try_from(n).unwrap_or(0);
        }
        Wrote::All
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
}

#[cfg(target_os = "macos")]
fn reader(fd: crate::sys::Fd, tx: Sender<Read>, signalled: Arc<AtomicBool>, wake: impl Fn()) {
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
        // SAFETY: the buffer is valid for READ_CHUNK bytes.
        let n = unsafe { crate::sys::read(fd, buf.as_mut_ptr().cast(), READ_CHUNK) };
        // Stamped before anything else happens to the bytes: this is the
        // moment the child's answer reached the process.
        let at = host_now();
        if n <= 0 {
            return;
        }
        let n = usize::try_from(n).unwrap_or(0);
        if let Some(f) = log.as_mut() {
            use std::io::Write;
            let _ = f.write_all(&buf[..n]);
        }
        if tx
            .send(Read {
                at,
                bytes: buf[..n].to_vec(),
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
/// read that follows is what reports it.
#[cfg(target_os = "linux")]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Ready {
    Readable,
    Idle,
}

/// One read of the master.
#[cfg(target_os = "linux")]
pub enum Chunk {
    Bytes(Vec<u8>),
    Closed,
}

/// Waits until either descriptor is readable or `timeout` passes -- `None`
/// waits as long as it takes -- and says which. A signal that cuts the
/// wait short answers `Idle` for both, so the caller simply waits again.
#[cfg(target_os = "linux")]
pub fn wait(
    a: BorrowedFd<'_>,
    b: BorrowedFd<'_>,
    timeout: Option<Duration>,
) -> std::io::Result<(Ready, Ready)> {
    let mut fds = [
        crate::sys::PollFd {
            fd: a.as_raw_fd(),
            events: crate::sys::POLLIN,
            revents: 0,
        },
        crate::sys::PollFd {
            fd: b.as_raw_fd(),
            events: crate::sys::POLLIN,
            revents: 0,
        },
    ];
    // Rounded up to whole milliseconds, not down, from the nanoseconds: a
    // deadline 0.8 ms -- or 0.8 us -- ahead truncated to `poll(0)` returns
    // at once, and a caller waiting for that deadline asks again, spinning
    // until the clock reaches it.
    let ms = match timeout {
        None => -1,
        Some(d) => i32::try_from(d.as_nanos().div_ceil(1_000_000)).unwrap_or(i32::MAX),
    };
    // SAFETY: two initialised records and their count; the kernel writes
    // only `revents`, and the array outlives the call.
    let n = unsafe { crate::sys::poll(fds.as_mut_ptr(), 2, ms) };
    if n < 0 {
        let e = std::io::Error::last_os_error();
        return match e.kind() {
            std::io::ErrorKind::Interrupted => Ok((Ready::Idle, Ready::Idle)),
            _ => Err(e),
        };
    }
    let ready = |p: &crate::sys::PollFd| match p.revents {
        0 => Ready::Idle,
        _ => Ready::Readable,
    };
    Ok((ready(&fds[0]), ready(&fds[1])))
}

/// A font file as a read-only, private mapping, for the life of the
/// process. A face is a bounds-checked view over the file's bytes, so a
/// mapping is the same thing a read into memory is -- without the read:
/// Apple Color Emoji is 192 MB, and reading it whole was 17 ms of every
/// launch for the few strikes a screen uses. The pages a face touches are
/// the pages that get paged in.
pub fn map_readonly(path: &str) -> std::io::Result<&'static [u8]> {
    // **`File` is `open`, `fstat` and `close`**, and it opens
    // close-on-exec, so none of those and none of their flags has to be
    // declared in `sys`. That is most of what used to make this function
    // the largest consumer of a `libc` dependency.
    let file = std::fs::File::open(path)?;
    let len = usize::try_from(file.metadata()?.len()).unwrap_or(0);
    if len == 0 {
        return Err(std::io::Error::other("an empty font file"));
    }
    // SAFETY: a private read-only mapping of the whole file at offset
    // zero; the kernel validates the length against the file. The mapping
    // is never unmapped, so the slice below lives as long as the process.
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
mod tests {
    use super::*;

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
        assert_eq!(strings(&child_env(outer_parent())), want);
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
            child_env(outer_parent()),
        )
        .expect("forkpty");
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

    /// What the kernel holds for the pty behind `master` -- read back rather
    /// than trusted from `told`, so the tests below are about the kernel.
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
            child_env(outer_parent()),
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
