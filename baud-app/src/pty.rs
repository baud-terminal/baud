//! The pty: a login shell on the slave, the master read on a thread.
//!
//! The one place this binary calls libc: `forkpty`, `execvp`, `read`,
//! `write` and the `TIOCSWINSZ` ioctl. The reader thread owns the master's
//! read side for the life of the process and pushes every `read(2)` --
//! 4096 bytes at a time, which is what `N_TTY_BUF_SIZE` and the Darwin
//! clist hand back anyway -- to the main thread over a channel.
#![deny(unsafe_op_in_unsafe_fn)]

use baud_domain::WinSize;
use baud_font::Px;
use std::ffi::CString;
use std::sync::mpsc::{Receiver, Sender, channel};

/// The master's file descriptor, owned here.
pub struct Pty {
    master: libc::c_int,
    rx: Receiver<Vec<u8>>,
}

/// What `read(2)` delivers at most per call, the tty's own quantum.
const READ_CHUNK: usize = 4096;

impl Pty {
    /// Forks a login shell -- `$SHELL`, else `/bin/zsh` -- on a fresh pty
    /// of `size`, and starts reading it.
    pub fn spawn(size: &WinSize) -> Option<Pty> {
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".to_string());
        let shell_c = CString::new(shell.clone()).ok()?;
        let argv0 = CString::new(format!("-{}", shell.rsplit('/').next().unwrap_or("zsh"))).ok()?;
        let term = CString::new("TERM=xterm-256color").ok()?;
        let colorterm = CString::new("COLORTERM=truecolor").ok()?;
        let mut ws: libc::winsize = unsafe { std::mem::zeroed() };
        ws.ws_col = size.ws_col;
        ws.ws_row = size.ws_row;
        let mut master: libc::c_int = -1;
        // SAFETY: forkpty is given a valid out-pointer and window size; in
        // the child only async-signal-safe calls run before exec.
        let pid = unsafe {
            libc::forkpty(
                &mut master,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                &mut ws,
            )
        };
        if pid < 0 {
            return None;
        }
        if pid == 0 {
            unsafe {
                libc::putenv(term.into_raw());
                libc::putenv(colorterm.into_raw());
                let argv = [argv0.as_ptr(), std::ptr::null()];
                libc::execvp(shell_c.as_ptr(), argv.as_ptr());
                libc::_exit(127);
            }
        }
        let (tx, rx) = channel();
        let fd = master;
        std::thread::spawn(move || reader(fd, tx));
        Some(Pty { master, rx })
    }

    /// Chunks the shell wrote since the last call, in order.
    pub fn drain(&self) -> Vec<Vec<u8>> {
        let mut out = Vec::new();
        while let Ok(chunk) = self.rx.try_recv() {
            out.push(chunk);
        }
        out
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

fn reader(fd: libc::c_int, tx: Sender<Vec<u8>>) {
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
    }
}
