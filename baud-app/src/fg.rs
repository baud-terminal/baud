//! **The program in the foreground and the folder it is in**, read from
//! the kernel for the window's title when a program sets none (note AK,
//! section 2; `title.rs`).
//!
//! No standard covers any of it: POSIX's `tcgetpgrp` "shall fail if ... the
//! file is not the controlling terminal" (`[[source: posix-tcgetpgrp]]`),
//! and both kernels answer it on the master anyway (`sys.rs`). What is
//! read, and why that and not something cheaper:
//!
//! * **The group**: `TIOCGPGRP` on the master, the job the shell gave the
//!   terminal to, or the shell itself at its prompt. XNU's master answers 0
//!   when there is none, which is no group here.
//! * **Its name**: argv[0]'s last component, a login shell's leading `-`
//!   dropped -- what `ps` shows and what the user typed. macOS's
//!   `proc_name` is cheaper and answered `coreutils` for nix's `sleep`, a
//!   symlink to a multi-call binary (note AK 2.3, measured), where Linux's
//!   `comm` says `sleep`; argv[0] is the same on both. When argv[0] cannot
//!   be read -- a root process under `sudo` -- the executable's file name
//!   stands in.
//! * **Its directory**: macOS's `PROC_PIDVNODEPATHINFO`, Linux's
//!   `/proc/pid/cwd` (`[[source: linux-man-proc-pid-cwd]]`), each refused
//!   for a process the user cannot trace.
//!
//! On macOS the reads are Apple's "private interfaces ... subject to change
//! in future releases" (`[[source: macos-sdk-libproc-h]]` 41-44), the only
//! ones there are; a call that fails reads as nothing, and the title says
//! what it still knows.

use crate::sys::{Fd, Pid};
use std::os::unix::ffi::OsStrExt;
use std::path::PathBuf;

/// The pty's foreground process group, `None` when the kernel names none.
pub fn group(master: Fd) -> Option<Pid> {
    let mut g: Pid = 0;
    // SAFETY: `TIOCGPGRP` writes one `pid_t` through the pointer.
    let r = unsafe { crate::sys::ioctl(master, crate::sys::TIOCGPGRP, &raw mut g) };
    (r == 0 && g > 0).then_some(g)
}

/// The program `pid` runs, by argv[0], else by its executable's name.
pub fn name(pid: Pid) -> Option<String> {
    argv0(pid)
        .and_then(|a| program(&a))
        .or_else(|| exe(pid).and_then(|p| program(p.as_os_str().as_bytes())))
}

/// argv[0]'s last path component, a login shell's leading `-` dropped
/// (`fork_in` starts the shell as `-zsh`, as `login` does).
pub fn program(argv0: &[u8]) -> Option<String> {
    let last = argv0.rsplit(|&b| b == b'/').next()?;
    let last = last.strip_prefix(b"-").unwrap_or(last);
    (!last.is_empty()).then(|| String::from_utf8_lossy(last).into_owned())
}

/// The first argument out of a `KERN_PROCARGS2` answer: "argc as the
/// first word", then "the bare executable path", which "may have multiple
/// null bytes after it for alignment", then argv (`[[source:
/// xnu-kern-sysctl]]` 1619-1640, 1706-1709).
#[cfg(any(target_os = "macos", test))]
pub fn procargs2_argv0(buf: &[u8]) -> Option<&[u8]> {
    let (argc, rest) = buf.split_first_chunk::<4>()?;
    if core::ffi::c_int::from_ne_bytes(*argc) < 1 {
        return None;
    }
    let path_end = rest.iter().position(|&b| b == 0)?;
    let after = &rest[path_end..];
    let start = after.iter().position(|&b| b != 0)?;
    let arg = &after[start..];
    let end = arg.iter().position(|&b| b == 0).unwrap_or(arg.len());
    Some(&arg[..end])
}

/// The bytes before the first NUL: a C string out of a buffer.
fn c_string(buf: &[u8]) -> &[u8] {
    let end = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
    &buf[..end]
}

/// argv[0] of `pid`: `KERN_PROCARGS2` asked twice, once for the size it
/// needs and once with a buffer that size, because a shorter buffer gets
/// "copied out `buflen` bytes from the end" -- the environment's end, not
/// argv's (`[[source: xnu-kern-sysctl]]` 1532-1536 and 1661-1693).
#[cfg(target_os = "macos")]
fn argv0(pid: Pid) -> Option<Vec<u8>> {
    use crate::sys::{CTL_KERN, KERN_PROCARGS2, MapLen, sysctl};
    let mut mib = [CTL_KERN, KERN_PROCARGS2, pid];
    let mut len = MapLen(0);
    let null = std::ptr::null_mut();
    // SAFETY: a three-name MIB and an out-length; no buffer, so the call
    // writes only the length.
    if unsafe { sysctl(mib.as_mut_ptr(), 3, null, &raw mut len, null, MapLen(0)) } != 0 {
        return None;
    }
    let mut buf = vec![0u8; len.0];
    // SAFETY: a buffer of the length passed beside it.
    if unsafe {
        sysctl(
            mib.as_mut_ptr(),
            3,
            buf.as_mut_ptr().cast(),
            &raw mut len,
            null,
            MapLen(0),
        )
    } != 0
    {
        return None;
    }
    buf.truncate(len.0);
    procargs2_argv0(&buf).map(<[u8]>::to_vec)
}

/// argv[0] of `pid`: `/proc/pid/cmdline`'s "set of strings separated by
/// null bytes", up to the first (`[[source: linux-man-proc-pid-cmdline]]`).
/// A process that rewrote its arguments as one line with no NULs (sshd's
/// `sshd: user@pts/0`) gives that whole line, and its last `/` component
/// is shown.
#[cfg(target_os = "linux")]
fn argv0(pid: Pid) -> Option<Vec<u8>> {
    let all = std::fs::read(format!("/proc/{pid}/cmdline")).ok()?;
    Some(c_string(&all).to_vec())
}

/// The executable `pid` runs: `proc_pidpath`, which still answers for a
/// root process (note AK 2.5, measured against pid 1).
#[cfg(target_os = "macos")]
fn exe(pid: Pid) -> Option<PathBuf> {
    use crate::sys::{PROC_PIDPATHINFO_MAXSIZE, PathSize, proc_pidpath};
    let mut buf = vec![0u8; PROC_PIDPATHINFO_MAXSIZE];
    let size = PathSize(u32::try_from(buf.len()).ok()?);
    // SAFETY: a buffer of the size passed beside it.
    let n = unsafe { proc_pidpath(pid, buf.as_mut_ptr().cast(), size) };
    let path = c_string(&buf[..usize::try_from(n).ok()?]);
    (!path.is_empty()).then(|| PathBuf::from(std::ffi::OsStr::from_bytes(path)))
}

/// The executable `pid` runs: `/proc/pid/exe`, "a symbolic link containing
/// the actual pathname of the executed command" (`[[source:
/// linux-man-proc-pid-exe]]`).
#[cfg(target_os = "linux")]
fn exe(pid: Pid) -> Option<PathBuf> {
    std::fs::read_link(format!("/proc/{pid}/exe")).ok()
}

/// The directory `pid` is in: `pvi_cdir`'s path out of
/// `PROC_PIDVNODEPATHINFO`, symlinks resolved by the kernel (`/tmp` reads
/// `/private/tmp`). `proc_pidinfo` answers the bytes it wrote, 0 on a
/// failure (`[[source: xnu-libproc-c]]` 100-110).
#[cfg(target_os = "macos")]
pub fn dir(pid: Pid) -> Option<PathBuf> {
    use crate::sys::{
        MAXPATHLEN, PROC_PIDVNODEPATHINFO, ProcArg, VNODEPATHINFO_CDIR_PATH, VNODEPATHINFO_SIZE,
        proc_pidinfo,
    };
    let mut buf = vec![0u8; VNODEPATHINFO_SIZE];
    let size = core::ffi::c_int::try_from(buf.len()).ok()?;
    // SAFETY: a buffer of the size passed beside it.
    let n = unsafe {
        proc_pidinfo(
            pid,
            PROC_PIDVNODEPATHINFO,
            ProcArg(0),
            buf.as_mut_ptr().cast(),
            size,
        )
    };
    if n != size {
        return None;
    }
    let path = c_string(&buf[VNODEPATHINFO_CDIR_PATH..VNODEPATHINFO_CDIR_PATH + MAXPATHLEN]);
    (!path.is_empty()).then(|| PathBuf::from(std::ffi::OsStr::from_bytes(path)))
}

/// The directory `pid` is in: `/proc/pid/cwd`, "a symbolic link to the
/// current working directory of the process".
#[cfg(target_os = "linux")]
pub fn dir(pid: Pid) -> Option<PathBuf> {
    std::fs::read_link(format!("/proc/{pid}/cwd")).ok()
}

/// The live reads, for [`crate::title::Derive::check`].
pub struct Live {
    pub master: Fd,
}

impl crate::title::Reads for Live {
    fn group(&self) -> Option<Pid> {
        group(self.master)
    }

    fn name(&self, pid: Pid) -> Option<String> {
        name(pid)
    }

    fn dir(&self, pid: Pid) -> Option<PathBuf> {
        dir(pid)
    }
}

/// Whether a watched process forked, exec'd or exited since the last look.
#[cfg(target_os = "macos")]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Moved {
    Still,
    Moved,
}

/// **The process events a job starts and ends with** (note AK 2.7): a
/// kqueue watching the shell and each foreground leader for
/// `NOTE_FORK`, `NOTE_EXEC` and `NOTE_EXIT`, waited on by a thread of its
/// own, as the pty's reader waits on the master, which wakes the main
/// thread once per batch. A job that prints nothing, `sleep 60`, starts
/// and ends with no read, and these are what say it did; nothing is
/// checked on a timer. "NOTE_TRACK, NOTE_TRACKERR, and NOTE_CHILD are no
/// longer supported as of 10.5" (`[[source: macos-sdk-sys-event-h]]`
/// 357), so a fork names no child: after one the shell's children are
/// listed and watched, before the group is read, so a child that takes
/// the terminal and execs after the read is seen by its `NOTE_EXEC`.
#[cfg(target_os = "macos")]
pub struct Watch {
    kq: Fd,
    moved: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

#[cfg(target_os = "macos")]
impl Watch {
    /// A kqueue and the thread that waits on it; `wake` runs after each
    /// batch of events.
    pub fn start(wake: impl Fn() + Send + 'static) -> Option<Watch> {
        use std::sync::atomic::{AtomicBool, Ordering};
        // SAFETY: no arguments; a descriptor or -1.
        let kq = unsafe { crate::sys::kqueue() };
        if kq < 0 {
            return None;
        }
        let moved = std::sync::Arc::new(AtomicBool::new(false));
        let flag = std::sync::Arc::clone(&moved);
        let spawned = std::thread::Builder::new()
            .name("baud-fg-watch".into())
            .spawn(move || {
                let mut events = [crate::sys::Kevent {
                    ident: 0,
                    filter: 0,
                    flags: 0,
                    fflags: 0,
                    data: 0,
                    udata: std::ptr::null_mut(),
                }; 8];
                loop {
                    // SAFETY: no changes; an event list of its length; no
                    // timeout, so the call waits for an event.
                    let n = unsafe {
                        crate::sys::kevent(
                            kq,
                            std::ptr::null(),
                            0,
                            events.as_mut_ptr(),
                            8,
                            std::ptr::null(),
                        )
                    };
                    if n < 0
                        && std::io::Error::last_os_error().kind() != std::io::ErrorKind::Interrupted
                    {
                        return;
                    }
                    if n > 0 {
                        flag.store(true, Ordering::Release);
                        wake();
                    }
                }
            });
        spawned.ok()?;
        Some(Watch { kq, moved })
    }

    /// Watch `pid` for a fork, an exec or an exit. A process gone, or one
    /// the user may not watch, is not watched, and says nothing.
    pub fn add(&self, pid: Pid) {
        use crate::sys::{EV_ADD, EV_CLEAR, EVFILT_PROC, Kevent, NOTE_EXEC, NOTE_EXIT, NOTE_FORK};
        let Ok(ident) = core::ffi::c_ulong::try_from(pid) else {
            return;
        };
        let change = Kevent {
            ident,
            filter: EVFILT_PROC,
            flags: EV_ADD | EV_CLEAR,
            fflags: NOTE_FORK | NOTE_EXEC | NOTE_EXIT,
            data: 0,
            udata: std::ptr::null_mut(),
        };
        let zero = crate::sys::Timespec {
            tv_sec: 0,
            tv_nsec: 0,
        };
        // SAFETY: one change, no event list, a zero timeout.
        let _ = unsafe {
            crate::sys::kevent(
                self.kq,
                &raw const change,
                1,
                std::ptr::null_mut(),
                0,
                &raw const zero,
            )
        };
    }

    /// Watch every child of `parent`: the job a fork made, before it
    /// takes the terminal and execs. `proc_listchildpids` answers a count
    /// of pids (`[[source: xnu-libproc-c]]` 87-97).
    pub fn add_children(&self, parent: Pid) {
        let mut kids = [0 as Pid; 64];
        let Ok(size) = core::ffi::c_int::try_from(core::mem::size_of_val(&kids)) else {
            return;
        };
        // SAFETY: a buffer of the size passed beside it.
        let n = unsafe { crate::sys::proc_listchildpids(parent, kids.as_mut_ptr().cast(), size) };
        for &kid in kids.iter().take(usize::try_from(n).unwrap_or(0)) {
            self.add(kid);
        }
    }

    /// Whether anything watched moved since the last call.
    pub fn take(&self) -> Moved {
        match self.moved.swap(false, std::sync::atomic::Ordering::AcqRel) {
            true => Moved::Moved,
            false => Moved::Still,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_program_is_argv0s_last_part_without_a_login_dash() {
        assert_eq!(program(b"-zsh").as_deref(), Some("zsh"));
        assert_eq!(program(b"/bin/sleep").as_deref(), Some("sleep"));
        assert_eq!(program(b"vim").as_deref(), Some("vim"));
        assert_eq!(program(b""), None);
        assert_eq!(program(b"/usr/bin/"), None);
    }

    #[test]
    fn procargs2_gives_argv0_past_the_path_and_its_padding() {
        let mut buf = 2i32.to_ne_bytes().to_vec();
        buf.extend_from_slice(b"/nix/store/x/bin/coreutils\0\0\0\0sleep\x0060\0PATH=/bin\0");
        assert_eq!(procargs2_argv0(&buf), Some(&b"sleep"[..]));
        let mut none = 0i32.to_ne_bytes().to_vec();
        none.extend_from_slice(b"/bin/x\0\0y\0");
        assert_eq!(procargs2_argv0(&none), None, "no argc, no argv");
        assert_eq!(procargs2_argv0(&[1, 0]), None, "short");
    }
}
