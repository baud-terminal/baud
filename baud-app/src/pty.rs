//! The pty: a login shell on the slave, the master read on a thread on
//! macOS and polled beside the compositor's socket on Linux.
//!
//! The one place this binary calls libc: `forkpty`, `execvp`, `read`,
//! `write`, `poll`, the `TIOCSWINSZ` ioctl, and `mmap` for the font files. On macOS the reader thread
//! owns the master's read side for the life of the process and pushes
//! every `read(2)` -- 4096 bytes at a time, which is what `N_TTY_BUF_SIZE`
//! and the Darwin clist hand back anyway -- to the main thread over a
//! channel; on Linux the main loop waits on the master itself and takes
//! each read where it is polled.
#![deny(unsafe_op_in_unsafe_fn)]

use baud_domain::WinSize;
use baud_font::Px;
use std::ffi::CString;
#[cfg(target_os = "linux")]
use std::os::fd::{AsRawFd, BorrowedFd};
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
    master: libc::c_int,
    #[cfg(target_os = "macos")]
    rx: Receiver<Vec<u8>>,
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

/// What a drain found: the chunks that arrived, and whether more can.
#[cfg(target_os = "macos")]
pub struct Drained {
    pub chunks: Vec<Vec<u8>>,
    pub stream: Stream,
}

/// What `read(2)` delivers at most per call, the tty's own quantum.
const READ_CHUNK: usize = 4096;

/// The shell when `$SHELL` is unset: the system's.
#[cfg(target_os = "macos")]
const DEFAULT_SHELL: &str = "/bin/zsh";
#[cfg(not(target_os = "macos"))]
const DEFAULT_SHELL: &str = "/bin/sh";

impl Pty {
    /// Forks a login shell -- `$SHELL`, else the system's -- on a fresh
    /// pty of `size`, and returns the master. With `BAUD_CMD` set, forks
    /// `/bin/sh -c "$BAUD_CMD"` instead, which is how a benchmark drives
    /// the terminal: the window closes when the command ends.
    fn fork(size: &WinSize) -> Option<libc::c_int> {
        let cmd = std::env::var("BAUD_CMD").ok();
        let shell = match &cmd {
            Some(_) => "/bin/sh".to_string(),
            None => std::env::var("SHELL").unwrap_or_else(|_| DEFAULT_SHELL.to_string()),
        };
        let shell_c = CString::new(shell.clone()).ok()?;
        let argv0 = match &cmd {
            Some(_) => CString::new("sh").ok()?,
            None => CString::new(format!("-{}", shell.rsplit('/').next().unwrap_or("sh"))).ok()?,
        };
        let dash_c = CString::new("-c").ok()?;
        let cmd_c = cmd.and_then(|c| CString::new(c).ok());
        let term = CString::new("TERM=xterm-256color").ok()?;
        let colorterm = CString::new("COLORTERM=truecolor").ok()?;
        let mut ws: libc::winsize = unsafe { std::mem::zeroed() };
        ws.ws_col = size.ws_col;
        ws.ws_row = size.ws_row;
        let mut master: libc::c_int = -1;
        // Linux's forkpty takes the window size by const pointer and
        // Darwin's by mutable pointer; the size is the same struct.
        #[cfg(target_os = "linux")]
        let ws_arg: *const libc::winsize = &raw const ws;
        #[cfg(not(target_os = "linux"))]
        let ws_arg: *mut libc::winsize = &raw mut ws;
        // SAFETY: forkpty is given a valid out-pointer and window size; in
        // the child only async-signal-safe calls run before exec.
        let pid = unsafe {
            libc::forkpty(
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
                libc::putenv(term.into_raw());
                libc::putenv(colorterm.into_raw());
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
                libc::execvp(shell_c.as_ptr(), argv.as_ptr());
                libc::_exit(127);
            }
        }
        Some(master)
    }

    /// The pty with a thread reading it: each chunk crosses a channel and
    /// `wake` runs once per batch, so a main thread that cannot wait on a
    /// descriptor -- the macOS run loop -- learns there is something to
    /// drain.
    #[cfg(target_os = "macos")]
    pub fn spawn(size: &WinSize, wake: impl Fn() + Send + 'static) -> Option<Pty> {
        let master = Pty::fork(size)?;
        let (tx, rx) = channel();
        let fd = master;
        let signalled = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&signalled);
        std::thread::spawn(move || reader(fd, tx, flag, wake));
        Some(Pty {
            master,
            rx,
            signalled,
        })
    }

    /// The pty with no thread: the caller waits on `as_fd` beside whatever
    /// else it waits on and takes each read with `read_chunk`, which is
    /// the Linux loop's shape -- one wait over the compositor's socket and
    /// the master, and nothing crossing a channel.
    #[cfg(target_os = "linux")]
    pub fn spawn_polled(size: &WinSize) -> Option<Pty> {
        Some(Pty {
            master: Pty::fork(size)?,
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
        let n = unsafe { libc::read(self.master, buf.as_mut_ptr().cast(), READ_CHUNK) };
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

    /// Writes `bytes` to the shell, all of them.
    pub fn write(&self, bytes: &[u8]) {
        let mut at = 0usize;
        while at < bytes.len() {
            // SAFETY: the slice is valid for its length and the fd is ours.
            let n =
                unsafe { libc::write(self.master, bytes[at..].as_ptr().cast(), bytes.len() - at) };
            if n <= 0 {
                return;
            }
            at += usize::try_from(n).unwrap_or(0);
        }
    }

    /// Tells the kernel, and through it the shell, the new geometry in
    /// cells and in pixels.
    pub fn resize(&self, size: &WinSize, px_w: Px, px_h: Px) {
        let ws = libc::winsize {
            ws_col: size.ws_col,
            ws_row: size.ws_row,
            ws_xpixel: u16::try_from(px_w.value()).unwrap_or(u16::MAX),
            ws_ypixel: u16::try_from(px_h.value()).unwrap_or(u16::MAX),
        };
        // SAFETY: TIOCSWINSZ takes a pointer to a winsize.
        unsafe {
            libc::ioctl(self.master, libc::TIOCSWINSZ, &ws);
        }
    }
}

#[cfg(target_os = "macos")]
fn reader(fd: libc::c_int, tx: Sender<Vec<u8>>, signalled: Arc<AtomicBool>, wake: impl Fn()) {
    let mut buf = vec![0u8; READ_CHUNK];
    loop {
        // SAFETY: the buffer is valid for READ_CHUNK bytes.
        let n = unsafe { libc::read(fd, buf.as_mut_ptr().cast(), READ_CHUNK) };
        if n <= 0 {
            return;
        }
        let n = usize::try_from(n).unwrap_or(0);
        if tx.send(buf[..n].to_vec()).is_err() {
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
        libc::pollfd {
            fd: a.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        },
        libc::pollfd {
            fd: b.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        },
    ];
    let ms = match timeout {
        None => -1,
        Some(d) => i32::try_from(d.as_millis()).unwrap_or(i32::MAX),
    };
    // SAFETY: two initialised records and their count; the kernel writes
    // only `revents`, and the array outlives the call.
    let n = unsafe { libc::poll(fds.as_mut_ptr(), 2, ms) };
    if n < 0 {
        let e = std::io::Error::last_os_error();
        return match e.kind() {
            std::io::ErrorKind::Interrupted => Ok((Ready::Idle, Ready::Idle)),
            _ => Err(e),
        };
    }
    let ready = |p: &libc::pollfd| match p.revents {
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
    let c = CString::new(path).map_err(|_| std::io::Error::other("a NUL in a font path"))?;
    // SAFETY: a NUL-terminated path and flags; the descriptor returned is
    // ours until `close` below.
    let fd = unsafe { libc::open(c.as_ptr(), libc::O_RDONLY | libc::O_CLOEXEC) };
    if fd < 0 {
        return Err(std::io::Error::last_os_error());
    }
    // SAFETY: `st` is a zeroed stat buffer the kernel fills for an open fd.
    let mut st: libc::stat = unsafe { std::mem::zeroed() };
    // SAFETY: an open descriptor and a pointer to a stat buffer of the
    // right layout.
    if unsafe { libc::fstat(fd, &mut st) } < 0 {
        let e = std::io::Error::last_os_error();
        // SAFETY: the descriptor is ours and open.
        unsafe { libc::close(fd) };
        return Err(e);
    }
    let len = usize::try_from(st.st_size).unwrap_or(0);
    if len == 0 {
        // SAFETY: as above.
        unsafe { libc::close(fd) };
        return Err(std::io::Error::other("an empty font file"));
    }
    // SAFETY: a private read-only mapping of the whole file at offset
    // zero; the kernel validates the length against the file. The mapping
    // is never unmapped, so the slice below lives as long as the process.
    let ptr = unsafe {
        libc::mmap(
            std::ptr::null_mut(),
            len,
            libc::PROT_READ,
            libc::MAP_PRIVATE,
            fd,
            0,
        )
    };
    // SAFETY: the mapping holds its own reference to the file; the
    // descriptor is no longer needed.
    unsafe { libc::close(fd) };
    if ptr == libc::MAP_FAILED {
        return Err(std::io::Error::last_os_error());
    }
    // SAFETY: `ptr` is a live, page-aligned, read-only mapping of exactly
    // `len` bytes that outlives every borrow of it.
    Ok(unsafe { std::slice::from_raw_parts(ptr.cast::<u8>(), len) })
}
