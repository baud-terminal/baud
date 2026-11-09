//! **What baud was started with stays out of its children.** A launcher
//! or shell that leaves a descriptor open without close-on-exec hands it
//! to baud, and every child baud starts -- each window's shell, a link's
//! opener, Linux's `-e` re-exec -- would inherit it in turn, for good.
//! Once, first thing in `main` while the process still has one thread, so
//! no other open races it, every descriptor from 3 up is marked
//! close-on-exec ([`keep_from_children`]). baud itself keeps each one
//! open: nothing hands baud a descriptor meant for its children.
//!
//! **What this does not reach**: a descriptor a library opens without
//! close-on-exec for a moment, later, on another thread -- macOS's C
//! library opens each `LC_*` file of a locale it loads so (measured: every
//! locale file seen open while `newlocale` ran on another thread lacked
//! `FD_CLOEXEC`). A window's child is guarded against that at its fork
//! (`pty`); a `std::process::Command` child, an opener that exits at once,
//! is not, and only `posix_spawn` closing what it is not told to keep
//! would close it exactly.

use crate::sys::Fd;

/// **Every descriptor `listed` marked close-on-exec**: run once, before any
/// thread starts, with [`open_from_three`], every descriptor from 3 up.
/// The listing is given so a test drives this body over its own
/// descriptors, never the test process's others.
pub(crate) fn keep_from_children(listed: impl FnOnce() -> Vec<Fd>) {
    close_on_exec(&listed());
}

/// The descriptors this process holds, from 3 up, as the kernel lists
/// them: macOS's `proc_pidinfo(PROC_PIDLISTFDS)`, Linux's `/proc/self/fd`.
/// Listed rather than tried one by one up to the descriptor limit, which
/// is 1,048,576 in a shell here.
#[cfg(target_os = "macos")]
pub(crate) fn open_from_three() -> Vec<Fd> {
    use crate::sys::{FDINFO_FD, FDINFO_SIZE, PROC_PIDLISTFDS, ProcArg, proc_pidinfo};
    let Ok(pid) = crate::sys::Pid::try_from(std::process::id()) else {
        return Vec::new();
    };
    // SAFETY: a null buffer of size 0 asks only how many bytes the list is.
    let need = unsafe { proc_pidinfo(pid, PROC_PIDLISTFDS, ProcArg(0), core::ptr::null_mut(), 0) };
    let Ok(need) = usize::try_from(need) else {
        return Vec::new();
    };
    // Room for a few more, though one thread opens nothing between the two
    // calls.
    let mut buf = vec![0u8; need + 16 * FDINFO_SIZE];
    let Ok(size) = core::ffi::c_int::try_from(buf.len()) else {
        return Vec::new();
    };
    // SAFETY: a buffer of the size passed beside it; the answer is the
    // bytes of `proc_fdinfo` written.
    let got = unsafe {
        proc_pidinfo(
            pid,
            PROC_PIDLISTFDS,
            ProcArg(0),
            buf.as_mut_ptr().cast(),
            size,
        )
    };
    let Ok(got) = usize::try_from(got) else {
        return Vec::new();
    };
    buf[..got.min(buf.len())]
        .as_chunks::<FDINFO_SIZE>()
        .0
        .iter()
        .filter_map(|e| {
            let n = e.get(FDINFO_FD..FDINFO_FD + 4)?;
            Some(Fd::from_ne_bytes(n.try_into().ok()?))
        })
        .filter(|fd| *fd >= 3)
        .collect()
}

#[cfg(target_os = "linux")]
pub(crate) fn open_from_three() -> Vec<Fd> {
    let Ok(dir) = std::fs::read_dir("/proc/self/fd") else {
        return Vec::new();
    };
    dir.filter_map(|e| e.ok()?.file_name().to_str()?.parse::<Fd>().ok())
        .filter(|fd| *fd >= 3)
        .collect()
}

/// Each of `fds` marked close-on-exec, its other flags kept; one that is
/// closed, or already so, is left as it is.
pub(crate) fn close_on_exec(fds: &[Fd]) {
    for &fd in fds {
        // SAFETY: `F_GETFD` reads a descriptor's flags and fails on one
        // that is not open.
        let flags = unsafe { crate::sys::fcntl(fd, crate::sys::F_GETFD) };
        if flags >= 0 && flags & crate::sys::FD_CLOEXEC == 0 {
            // SAFETY: as above; setting the flag leaves the descriptor open.
            unsafe { crate::sys::fcntl(fd, crate::sys::F_SETFD, flags | crate::sys::FD_CLOEXEC) };
        }
    }
}

/// How far past the highest descriptor the parent saw a window's child
/// closes when it cannot list its own: descriptors are given lowest-first,
/// so as many opens by other threads between the parent's look and the
/// fork stay under it.
pub(crate) const MARGIN: Fd = 256;

/// How many descriptors a window's child can list, in a fixed array on its
/// stack, 8 bytes each (`proc_fdinfo`): 8 KiB, and no allocation.
#[cfg(target_os = "macos")]
pub(crate) const LIST_ROOM: usize = 1024;

/// **The highest descriptor open in the parent just before a fork**: the
/// child's bound when it cannot list its own.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Highest(Fd);

impl Highest {
    /// Looked up now, in the parent, where listing may allocate.
    pub(crate) fn scanned() -> Highest {
        Highest(open_from_three().into_iter().max().unwrap_or(2))
    }
}

/// **How much a window's child lists**: all it can, [`LIST_ROOM`]
/// entries, or one, which only a test asks, so the list fills and the
/// child takes the bound instead -- on Linux too, in place of
/// `close_range`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Room {
    Full,
    #[cfg_attr(not(test), allow(dead_code))]
    One,
}

/// How much this thread's spawns list: [`Room::Full`], unless a test set
/// otherwise.
pub(crate) fn list_room() -> Room {
    #[cfg(test)]
    {
        ROOM.with(core::cell::Cell::get)
    }
    #[cfg(not(test))]
    {
        Room::Full
    }
}

#[cfg(test)]
thread_local! {
    static ROOM: core::cell::Cell<Room> = const { core::cell::Cell::new(Room::Full) };
}

/// Sets how much this thread's spawns list ([`list_room`]).
#[cfg(test)]
pub(crate) fn set_list_room(room: Room) {
    ROOM.with(|r| r.set(room));
}

/// **In a window's child, between `fork` and `exec`: every descriptor from
/// 3 closed**, so the program it runs holds its terminal on 0, 1 and 2 and
/// nothing else of baud's -- not one a library on another thread held open
/// without close-on-exec at the moment of the fork (macOS's C library
/// opens each `LC_*` file of a locale it loads so). On macOS the child
/// lists its own descriptors into an array on its stack,
/// `proc_pidinfo(PROC_PIDLISTFDS)`, which is one `__proc_info` call with
/// no allocation and no lock (`[[source: xnu-libproc-c]]` 101-110), and
/// closes exactly those; on Linux `close_range(3, ~0U, 0)` closes them
/// (`[[source: linux-man-close-range]]`). When the list fails or fills
/// its `room`, or `close_range` fails (before Linux 5.9), it closes
/// every number from 3 to [`MARGIN`] past the highest the parent saw or
/// the list named. `close` and `getpid` are on XSH 2.4.3's table
/// (`[[source: posix-xsh-chap02]]`); the list and the bound are
/// arithmetic on the child's stack.
///
/// # Safety
///
/// Called only in the child of a fork, before its `exec`: it closes
/// descriptors the rest of this process may hold.
pub(crate) unsafe fn close_from_three(highest: Highest, room: Room) {
    #[cfg(target_os = "macos")]
    let top = {
        use crate::sys::{FDINFO_FD, FDINFO_SIZE, PROC_PIDLISTFDS, ProcArg, proc_pidinfo};
        let mut list = [0u8; LIST_ROOM * FDINFO_SIZE];
        let entries = match room {
            Room::Full => LIST_ROOM,
            Room::One => 1,
        };
        let len = entries * FDINFO_SIZE;
        let size = core::ffi::c_int::try_from(len).unwrap_or(0);
        // SAFETY: a buffer of at least the size passed beside it; the
        // answer is the bytes of `proc_fdinfo` written, 0 on failure.
        let got = unsafe {
            proc_pidinfo(
                crate::sys::getpid(),
                PROC_PIDLISTFDS,
                ProcArg(0),
                list.as_mut_ptr().cast(),
                size,
            )
        };
        let got = usize::try_from(got).unwrap_or(0).min(len);
        let listed = list[..got]
            .as_chunks::<FDINFO_SIZE>()
            .0
            .iter()
            .filter_map(|e| {
                let n = e.get(FDINFO_FD..FDINFO_FD + 4)?;
                Some(Fd::from_ne_bytes(n.try_into().ok()?))
            });
        // The kernel lists at most as many as fit and answers with the bytes
        // written (`[[source: xnu-proc-info-c]]` 514-552): an answer under
        // the room is the whole list, and one that fills it may not be.
        if got > 0 && got < len {
            for fd in listed.filter(|fd| *fd >= 3) {
                // SAFETY: a descriptor the child holds; closing it is the point.
                unsafe { crate::sys::close(fd) };
            }
            return;
        }
        listed.fold(highest.0, Fd::max)
    };
    #[cfg(target_os = "linux")]
    let top = {
        // A room of one, which only a test asks, takes the bound here too,
        // so the bound is tested on Linux as well.
        if room == Room::Full {
            // SAFETY: the call takes two unsigned ints and flags, by number.
            let r = unsafe {
                crate::sys::syscall(
                    crate::sys::SYS_CLOSE_RANGE,
                    3 as core::ffi::c_uint,
                    core::ffi::c_uint::MAX,
                    0 as core::ffi::c_uint,
                )
            };
            if r == 0 {
                return;
            }
        }
        highest.0
    };
    for fd in 3..=top.saturating_add(MARGIN) {
        // SAFETY: a number the child may hold; closing one it does not
        // hold fails harmlessly.
        unsafe { crate::sys::close(fd) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::fd::AsRawFd;

    fn flags(fd: Fd) -> core::ffi::c_int {
        // SAFETY: reads the flags of a descriptor this test holds open.
        unsafe { crate::sys::fcntl(fd, crate::sys::F_GETFD) }
    }

    /// **A descriptor open without close-on-exec comes out with it, and one
    /// with it is unchanged**: the start-up pass, [`keep_from_children`],
    /// over two descriptors this test opened, listed by number so no other
    /// descriptor of the test process is touched. Both stay open. The list
    /// `main` gives the pass names both, the control that it would reach
    /// them. Named break: an empty pass, and the bare one stays bare.
    #[test]
    fn an_inherited_descriptor_is_marked_close_on_exec_and_stays_open() {
        let bare = std::fs::File::open("/dev/null").expect("a descriptor");
        let kept = std::fs::File::open("/dev/null").expect("a second");
        let (b, k) = (bare.as_raw_fd(), kept.as_raw_fd());
        // SAFETY: `bare` is open; clearing its flags leaves it open.
        unsafe { crate::sys::fcntl(b, crate::sys::F_SETFD, 0) };
        assert_eq!(flags(b) & crate::sys::FD_CLOEXEC, 0, "the control: none");
        let before = flags(k);
        assert_ne!(before & crate::sys::FD_CLOEXEC, 0, "the control: set");
        let listed = open_from_three();
        assert!(listed.contains(&b) && listed.contains(&k), "{listed:?}");
        keep_from_children(|| vec![b, k]);
        assert_ne!(flags(b) & crate::sys::FD_CLOEXEC, 0, "marked");
        assert_eq!(flags(k), before, "unchanged");
        assert!(flags(b) >= 0 && flags(k) >= 0, "both open");
    }

    /// **`main` runs the pass first, over every descriptor from 3 up**,
    /// right after the rig door and before anything that could start a
    /// thread, so no other open races it. Named break: `main` gives the
    /// pass an empty listing.
    #[test]
    fn main_marks_what_it_inherited_first() {
        let main = include_str!("main.rs");
        let door = main
            .find("let admitted = rig_door::admitted();")
            .expect("the rig door");
        let pass = main
            .find("inherited::keep_from_children(inherited::open_from_three);")
            .expect("the pass");
        let next = main[door..]
            .find(";\n")
            .map(|i| door + i + 2)
            .expect("the door's end");
        assert!(
            main[next..pass]
                .trim()
                .lines()
                .all(|l| l.trim_start().starts_with("//")
                    || l.trim_start().starts_with("#[cfg")
                    || l.trim().is_empty()),
            "only comments between the door and the pass"
        );
    }
}
