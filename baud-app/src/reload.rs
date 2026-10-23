//! **The config read again when its files change**: the config file, the
//! theme files it names and the fonts it names by path, watched by the
//! kernel, and a burst of changes read once, a settle after the last.
//!
//! **What is watched is every directory a file sits in, and each file
//! that exists.** An editor's save is often not a write to the file: vim
//! "rename[s] the file and write[s] a new one" (`[[source:
//! vim-9-1-options]]`, `'backupcopy'`), and `rename(2)` leaves "a directory
//! entry named new ... visible to other threads throughout the renaming
//! operation" (`[[source: posix-rename]]`), so the new file arrives as a
//! change to its directory and the old file's descriptor watches a file
//! nobody will write again. A file that does not exist yet, and a
//! directory that does not, are seen through the nearest directory that
//! does: `~/.config` while `~/.config/baud` is missing.
//!
//! * **macOS**: a kqueue, `EVFILT_VNODE` on a descriptor opened
//!   `O_EVTONLY` for each directory and file (`[[source:
//!   macos-man-kqueue]]` 310-334, `sys.rs`), heard on the main queue: a
//!   dispatch source on the kqueue's descriptor
//!   (`DISPATCH_SOURCE_TYPE_READ`, "monitors a file descriptor for pending
//!   bytes available to be read", `[[source: macos-sdk-dispatch-source-h]]`
//!   160-166) runs a handler on the main thread that drains it, marks the
//!   watch heard and wakes the window. No thread of baud's waits on it.
//! * **Linux**: one inotify watch a directory (`[[source:
//!   linux-man-inotify-7]]` 15-16), its descriptor in the loop's own
//!   `poll`, and the settle's deadline in its timeout.
//!
//! **A change is decided by reading, not by the event.** A vnode event
//! names no file, and a watched `~/.config` hears every other program's
//! files too, so after the settle every watched file is read again -- a
//! text file's bytes, a font's identity and times -- and only a difference
//! from the last reading is a change. That one comparison folds a burst
//! into one reading, ignores the neighbours, and sees a file swapped in at
//! a higher place of the lookup. The watch is armed again before each
//! reading, so a file replaced by a rename is watched by its new
//! descriptor and a change after the reading is heard.
//!
//! **At launch the watch comes first** (`Watch::around`): taken, and the
//! files read, before the config is, so an edit while the config is read
//! reads differently at the next reading and is applied a settle later.
#![allow(unsafe_code)]

use std::path::{Path, PathBuf};
use std::time::Duration;

/// **How long after the last event the files are read**: 10 ms, pushed
/// back by each event, so one save's rename, create and write are one
/// reading. alacritty's `DEBOUNCE_DELAY` is 10 ms, WezTerm sleeps 200 ms;
/// Jev's lean after a re-ask, MS_10, 0.72 and 0.73.
pub const SETTLE: Duration = Duration::from_millis(10);

/// How a watched file is compared: a config or theme file by its bytes, a
/// font, which can be megabytes, by its identity and times.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Text,
    Face,
}

/// One file a config pulls in.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Watched {
    pub path: PathBuf,
    pub kind: Kind,
}

/// **The files to watch**: each place the config is looked up, each path
/// a theme name is tried at, and each font named by path, each once, in
/// this order. A symbolic link is watched as itself, read through: the
/// kernel's watch on its path is a watch on the file it names (an
/// `O_EVTONLY` open follows it, and inotify does unless asked not to:
/// `IN_DONT_FOLLOW`, "Don't dereference pathname if it is a symbolic
/// link", `[[source: linux-man-inotify-7]]` 310-313), so an edit through
/// it is heard in place, and a rename over its target as that file's
/// delete: `NOTE_DELETE` on macOS, `IN_DELETE_SELF` on Linux, measured.
/// The target's directory is not watched: a home-manager link resolves to
/// a file directly in `/nix/store`, whose every write would be a settle
/// and a reading, and a switch replaces the link in its own directory, as
/// WezTerm watches a config's path and its directory and never its
/// target's (`[[source: wezterm-20240203-config-lib]]`; Jev's lean after a
/// re-ask, B_WEZTERM_SHAPE).
pub fn files(lookup: &[PathBuf], themes: &[PathBuf], faces: &[PathBuf]) -> Vec<Watched> {
    let mut out: Vec<Watched> = Vec::new();
    let tagged = lookup
        .iter()
        .chain(themes)
        .map(|p| (p, Kind::Text))
        .chain(faces.iter().map(|p| (p, Kind::Face)));
    for (path, kind) in tagged {
        if !out.iter().any(|w| w.path == *path) {
            out.push(Watched {
                path: path.clone(),
                kind,
            });
        }
    }
    out
}

/// **The directories that see the files**: each file's directory, or,
/// while it does not exist, the nearest one above it that does -- and, for
/// a directory that is itself a symbolic link, as home-manager links a
/// whole config directory, the link's own directory too, where the next
/// link is renamed over it. Each once.
pub fn dirs(files: &[Watched]) -> Vec<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    dirs_beside(files, home.as_deref())
}

/// [`dirs`], with `home` the home directory. The link's own directory is
/// added only for the directory a file sits in, and never when it is
/// `home` or the root: a missing config seen from a linked `~/.config`
/// would otherwise watch the home directory, whose every write is a
/// settle and a reading -- WezTerm never watches it, "so that we don't
/// keep reloading every time something in the home dir changes"
/// (`[[source: wezterm-20240203-config-lib]]`).
fn dirs_beside(files: &[Watched], home: Option<&Path>) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    let mut add = |d: &Path| {
        if !out.iter().any(|o| o == d) {
            out.push(d.to_path_buf());
        }
    };
    for w in files {
        let own = w.path.parent();
        let mut dir = own;
        while let Some(d) = dir {
            if d.is_dir() {
                add(d);
                let up = d
                    .parent()
                    .filter(|up| Some(d) == own && d.is_symlink() && Some(*up) != home)
                    .filter(|up| up.parent().is_some());
                if let Some(up) = up {
                    add(up);
                }
                break;
            }
            dir = d.parent();
        }
    }
    out
}

/// What a file held when it was read.
#[derive(Clone, PartialEq, Eq, Debug)]
enum Held {
    Absent,
    Text(Vec<u8>),
    /// A font's device, inode, length and modification and change times:
    /// a rename over it changes the inode, a write the times.
    Face([i64; 7]),
    /// A file that is there and does not read, by why.
    Unread(std::io::ErrorKind),
}

impl Held {
    fn of(w: &Watched) -> Held {
        use std::os::unix::fs::MetadataExt;
        let missing = |e: std::io::Error| match e.kind() {
            std::io::ErrorKind::NotFound => Held::Absent,
            k => Held::Unread(k),
        };
        match w.kind {
            Kind::Text => std::fs::read(&w.path).map_or_else(missing, Held::Text),
            Kind::Face => std::fs::metadata(&w.path).map_or_else(missing, |m| {
                let n = |v: u64| i64::try_from(v).unwrap_or(i64::MAX);
                Held::Face([
                    n(m.dev()),
                    n(m.ino()),
                    n(m.len()),
                    m.mtime(),
                    m.mtime_nsec(),
                    m.ctime(),
                    m.ctime_nsec(),
                ])
            }),
        }
    }
}

/// Every watched file as it was read, in the list's order.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
struct Reading(Vec<(PathBuf, Held)>);

impl Reading {
    fn take(files: &[Watched]) -> Reading {
        Reading(
            files
                .iter()
                .map(|w| (w.path.clone(), Held::of(w)))
                .collect(),
        )
    }

    /// Whether a file both readings hold reads differently in them.
    fn moved_from(&self, earlier: &Reading) -> bool {
        self.0
            .iter()
            .any(|(p, h)| earlier.0.iter().any(|(q, was)| q == p && was != h))
    }

    /// Each file both readings hold, put back as `earlier` read it: a file
    /// that reads differently now is compared with its earlier reading at
    /// the settle's end, and the rest read the same either way.
    fn keep_earlier(&mut self, earlier: &Reading) {
        for (p, h) in &mut self.0 {
            if let Some((_, was)) = earlier.0.iter().find(|(q, _)| q == p) {
                h.clone_from(was);
            }
        }
    }
}

/// Whether the kernel said anything since the last look.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Heard {
    Quiet,
    Moved,
}

/// What the settle's end found.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Due {
    /// The settle has not ended, or nothing was heard.
    Wait,
    /// The files read as they did: nothing to apply.
    Same,
    /// A file reads differently: read the config again.
    Changed,
}

/// **The settle**: the deadline each event pushes back, on whatever clock
/// the driver runs -- the display link's time on macOS, `Instant` on
/// Linux, a fixed origin in a test.
#[derive(Clone, Copy, Debug)]
pub struct Settle<T> {
    due: Option<T>,
}

impl<T: Copy + PartialOrd + core::ops::Add<Duration, Output = T>> Settle<T> {
    pub fn new() -> Settle<T> {
        Settle { due: None }
    }

    /// An event at `now`: the reading moves to [`SETTLE`] after it.
    pub fn heard(&mut self, now: T) {
        self.due = Some(now + SETTLE);
    }

    /// When the files are read, while an event is waiting on it.
    pub fn deadline(&self) -> Option<T> {
        self.due
    }

    /// Whether the files are to be read at `now`; once a burst.
    pub fn take(&mut self, now: T) -> bool {
        match self.due {
            Some(d) if d <= now => {
                self.due = None;
                true
            }
            _ => false,
        }
    }
}

/// **The kernel's side on macOS**: the kqueue, and each directory and file
/// open for its events, closed when the watch is armed again. The kqueue
/// is shared with a listening source's context, and closes when the last
/// of the two lets it go: the context goes in the source's cancel handler
/// (`Watch::listen`).
#[cfg(target_os = "macos")]
struct Kernel {
    kq: std::sync::Arc<std::os::fd::OwnedFd>,
    open: Vec<std::fs::File>,
}

#[cfg(target_os = "macos")]
impl Kernel {
    fn new() -> Option<Kernel> {
        use std::os::fd::FromRawFd;
        // SAFETY: no arguments; a descriptor or -1.
        let kq = unsafe { crate::sys::kqueue() };
        if kq < 0 {
            return None;
        }
        // SAFETY: a descriptor just made, owned by nothing else.
        let kq = unsafe { std::os::fd::OwnedFd::from_raw_fd(kq) };
        Some(Kernel {
            kq: std::sync::Arc::new(kq),
            open: Vec::new(),
        })
    }

    fn fd(&self) -> crate::sys::Fd {
        use std::os::fd::AsRawFd;
        self.kq.as_raw_fd()
    }

    /// Watch `paths` from now: the new ones open and registered before the
    /// old close, so no change falls between them. A path that does not
    /// open is not watched; its directory still is.
    fn arm(&mut self, paths: &[&Path]) {
        use crate::sys::{
            EV_ADD, EV_CLEAR, EVFILT_VNODE, Kevent, NOTE_ATTRIB, NOTE_DELETE, NOTE_EXTEND,
            NOTE_LINK, NOTE_RENAME, NOTE_REVOKE, NOTE_WRITE, O_EVTONLY,
        };
        use std::os::fd::AsRawFd;
        use std::os::unix::fs::OpenOptionsExt;
        let mut open = Vec::with_capacity(paths.len());
        for p in paths {
            let Ok(f) = std::fs::OpenOptions::new()
                .read(true)
                .custom_flags(O_EVTONLY)
                .open(p)
            else {
                continue;
            };
            let Ok(ident) = core::ffi::c_ulong::try_from(f.as_raw_fd()) else {
                continue;
            };
            let change = Kevent {
                ident,
                filter: EVFILT_VNODE,
                flags: EV_ADD | EV_CLEAR,
                fflags: NOTE_DELETE
                    | NOTE_WRITE
                    | NOTE_EXTEND
                    | NOTE_ATTRIB
                    | NOTE_LINK
                    | NOTE_RENAME
                    | NOTE_REVOKE,
                data: 0,
                udata: std::ptr::null_mut(),
            };
            let zero = crate::sys::Timespec {
                tv_sec: 0,
                tv_nsec: 0,
            };
            // SAFETY: one change, no event list, a zero timeout.
            let r = unsafe {
                crate::sys::kevent(
                    self.fd(),
                    &raw const change,
                    1,
                    std::ptr::null_mut(),
                    0,
                    &raw const zero,
                )
            };
            if r == 0 {
                open.push(f);
            }
        }
        self.open = open;
    }

    /// Take every event waiting, never waiting for one.
    fn drain(&self) -> Heard {
        drain_kqueue(self.fd())
    }
}

/// Every event a kqueue holds, taken with a zero timeout: whether there
/// was one. The main queue's handler and the driver's own look both call
/// it, on the main thread.
#[cfg(target_os = "macos")]
fn drain_kqueue(kq: crate::sys::Fd) -> Heard {
    let mut events = [crate::sys::Kevent {
        ident: 0,
        filter: 0,
        flags: 0,
        fflags: 0,
        data: 0,
        udata: std::ptr::null_mut(),
    }; 16];
    let zero = crate::sys::Timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    let mut heard = Heard::Quiet;
    loop {
        // SAFETY: no changes; an event list of its length; a zero timeout,
        // so the call returns at once.
        let n = unsafe {
            crate::sys::kevent(
                kq,
                std::ptr::null(),
                0,
                events.as_mut_ptr(),
                16,
                &raw const zero,
            )
        };
        if n > 0 {
            heard = Heard::Moved;
        }
        if n < 16 {
            return heard;
        }
    }
}

/// **The kernel's side on Linux**: the inotify descriptor, non-blocking,
/// and the watch on each directory.
#[cfg(target_os = "linux")]
struct Kernel {
    fd: std::os::fd::OwnedFd,
    wds: Vec<core::ffi::c_int>,
}

#[cfg(target_os = "linux")]
impl Kernel {
    fn new() -> Option<Kernel> {
        use crate::sys::{O_CLOEXEC, O_NONBLOCK};
        use std::os::fd::FromRawFd;
        // SAFETY: flags only; a descriptor or -1.
        let fd = unsafe { crate::sys::inotify_init1(O_NONBLOCK | O_CLOEXEC) };
        if fd < 0 {
            return None;
        }
        // SAFETY: a descriptor just made, owned by nothing else.
        let fd = unsafe { std::os::fd::OwnedFd::from_raw_fd(fd) };
        Some(Kernel {
            fd,
            wds: Vec::new(),
        })
    }

    fn fd(&self) -> crate::sys::Fd {
        use std::os::fd::AsRawFd;
        self.fd.as_raw_fd()
    }

    /// Watch the directories among `paths` from now, and each symbolic
    /// link, which the watch follows to the file it names; a watch no
    /// longer wanted is removed after the new ones are added. Any other
    /// file's events come through its directory's watch; a link's target's
    /// directory is not watched (`files`).
    fn arm(&mut self, paths: &[&Path]) {
        use crate::sys::{
            IN_ATTRIB, IN_CLOSE_WRITE, IN_CREATE, IN_DELETE, IN_DELETE_SELF, IN_MODIFY,
            IN_MOVE_SELF, IN_MOVED_FROM, IN_MOVED_TO,
        };
        use std::os::unix::ffi::OsStrExt;
        let mask = IN_MODIFY
            | IN_ATTRIB
            | IN_CLOSE_WRITE
            | IN_MOVED_FROM
            | IN_MOVED_TO
            | IN_CREATE
            | IN_DELETE
            | IN_DELETE_SELF
            | IN_MOVE_SELF;
        let mut wds = Vec::with_capacity(paths.len());
        for p in paths.iter().filter(|p| p.is_dir() || p.is_symlink()) {
            let Ok(c) = std::ffi::CString::new(p.as_os_str().as_bytes()) else {
                continue;
            };
            // SAFETY: a NUL-terminated path that lives across the call.
            let wd = unsafe {
                crate::sys::inotify_add_watch(self.fd(), c.as_ptr(), crate::sys::InotifyMask(mask))
            };
            if wd >= 0 && !wds.contains(&wd) {
                wds.push(wd);
            }
        }
        for &old in self.wds.iter().filter(|w| !wds.contains(w)) {
            // SAFETY: a watch this descriptor gave; a stale one is EINVAL.
            let _ = unsafe { crate::sys::inotify_rm_watch(self.fd(), old) };
        }
        self.wds = wds;
    }

    /// Read every record waiting, never waiting for one: whether there was
    /// one. The records are not parsed.
    fn drain(&self) -> Heard {
        let mut buf = [0u8; 4096];
        let mut heard = Heard::Quiet;
        loop {
            // SAFETY: a buffer of the length passed beside it.
            let n = unsafe { crate::sys::read(self.fd(), buf.as_mut_ptr().cast(), buf.len()) };
            if n <= 0 {
                return heard;
            }
            heard = Heard::Moved;
        }
    }
}

/// **The watch**: the kernel's side, the files and how they last read,
/// and the settle.
pub struct Watch<T> {
    kernel: Option<Kernel>,
    files: Vec<Watched>,
    last: Reading,
    settle: Settle<T>,
    /// Whether a font file read differently at the last settle's end.
    faces_moved: crate::live::FaceFiles,
    /// Set by the main queue's handler on macOS, which drained the kqueue
    /// itself (`Watch::listen`), and who drains: once a handler listens,
    /// the look takes the mark alone and makes no `kevent` call of its
    /// own, so a turn of the loop costs no system call.
    #[cfg(target_os = "macos")]
    mark: std::sync::Arc<std::sync::atomic::AtomicBool>,
    #[cfg(target_os = "macos")]
    drains: std::cell::Cell<Drains>,
}

/// Who drains the kqueue on macOS: the look itself, or the main queue's
/// handler once one listens.
#[cfg(target_os = "macos")]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Drains {
    Look,
    Handler,
}

impl<T: Copy + PartialOrd + core::ops::Add<Duration, Output = T>> Watch<T> {
    /// Watch `files` from now, as they read now.
    pub fn new(files: Vec<Watched>) -> Watch<T> {
        let mut w = Watch {
            kernel: Kernel::new(),
            files: Vec::new(),
            last: Reading::default(),
            settle: Settle::new(),
            faces_moved: crate::live::FaceFiles::Same,
            #[cfg(target_os = "macos")]
            mark: std::sync::Arc::default(),
            #[cfg(target_os = "macos")]
            drains: std::cell::Cell::new(Drains::Look),
        };
        w.files = files;
        w.arm();
        w.last = Reading::take(&w.files);
        w
    }

    fn arm(&mut self) {
        let dirs = dirs(&self.files);
        let Some(k) = self.kernel.as_mut() else {
            return;
        };
        let existing = self.files.iter().map(|w| w.path.as_path());
        let paths: Vec<&Path> = dirs.iter().map(PathBuf::as_path).chain(existing).collect();
        k.arm(&paths);
    }

    /// The descriptor the kernel's events arrive on, for a dispatch source
    /// or a test; none when the kernel gave no watch.
    #[cfg(target_os = "macos")]
    pub fn fd(&self) -> Option<crate::sys::Fd> {
        self.kernel.as_ref().map(Kernel::fd)
    }

    /// The descriptor, borrowed, for the Linux loop's `poll`.
    #[cfg(target_os = "linux")]
    pub fn as_fd(&self) -> Option<std::os::fd::BorrowedFd<'_>> {
        use std::os::fd::AsFd;
        self.kernel.as_ref().map(|k| k.fd.as_fd())
    }

    /// Take what the kernel said, never waiting: an event at `now` moves
    /// the reading to a settle after it.
    pub fn heard(&mut self, now: T) -> Heard {
        #[cfg(target_os = "macos")]
        let drained = match (
            self.mark.swap(false, std::sync::atomic::Ordering::AcqRel),
            self.drains.get(),
        ) {
            (true, _) => Heard::Moved,
            (false, Drains::Handler) => Heard::Quiet,
            (false, Drains::Look) => self.kernel.as_ref().map_or(Heard::Quiet, Kernel::drain),
        };
        #[cfg(not(target_os = "macos"))]
        let drained = self.kernel.as_ref().map_or(Heard::Quiet, Kernel::drain);
        if drained == Heard::Moved {
            self.settle.heard(now);
        }
        drained
    }

    /// When the files are read, while an event waits on it.
    pub fn deadline(&self) -> Option<T> {
        self.settle.deadline()
    }

    /// **At the settle's end, the files read again**: the watch armed anew
    /// first -- a file a rename replaced is watched by its new descriptor,
    /// a directory just made is watched -- then every file read and
    /// compared with the last reading.
    pub fn due(&mut self, now: T) -> Due {
        if !self.settle.take(now) {
            return Due::Wait;
        }
        self.arm();
        let now_reads = Reading::take(&self.files);
        let due = match now_reads == self.last {
            true => Due::Same,
            false => Due::Changed,
        };
        let moved = now_reads.0.iter().zip(&self.files).any(|((p, h), w)| {
            w.kind == Kind::Face && !self.last.0.iter().any(|(q, was)| q == p && was == h)
        });
        self.faces_moved = match moved {
            true => crate::live::FaceFiles::Moved,
            false => crate::live::FaceFiles::Same,
        };
        self.last = now_reads;
        due
    }

    /// Whether a font file read differently at the last settle's end: a
    /// font the config names by the same path, replaced.
    pub fn faces_moved(&self) -> crate::live::FaceFiles {
        self.faces_moved
    }

    /// **The files a config just read pulls in**, watched from now. A file
    /// the last reading also held that reads differently now changed
    /// between that reading and this one, so it is heard at `now`, and
    /// kept as it read before: the settle's end compares with that, and
    /// reads the change.
    pub fn rewatch(&mut self, files: Vec<Watched>, now: T) {
        self.files = files;
        self.arm();
        let mut reading = Reading::take(&self.files);
        if reading.moved_from(&self.last) {
            self.settle.heard(now);
            reading.keep_earlier(&self.last);
        }
        self.last = reading;
    }

    /// **The watch around the launch's read of the config**: taken on the
    /// `lookup`, and its files read, before `read` reads the config, then
    /// the files `read` says the config names watched from then on
    /// ([`Watch::rewatch`]). An edit that lands after the launch's read and
    /// before the watch's first reading is a file that reads differently
    /// at the rewatch, so it is heard at `now`; a theme or font file the
    /// config names is read after this returns, under the watch.
    pub fn around<R>(
        lookup: &[PathBuf],
        now: T,
        read: impl FnOnce() -> (R, Vec<Watched>),
    ) -> (R, Watch<T>) {
        let mut w = Watch::new(
            lookup
                .iter()
                .map(|p| Watched {
                    path: p.clone(),
                    kind: Kind::Text,
                })
                .collect(),
        );
        let (r, files) = read();
        w.rewatch(files, now);
        (r, w)
    }

    /// The files watched, in the order they were given.
    #[cfg(test)]
    pub fn files(&self) -> &[Watched] {
        &self.files
    }
}

/// **The kqueue heard on the main queue**: a dispatch source on its
/// descriptor whose handler, on the main thread, drains it, marks the
/// watch heard and calls `wake`.
#[cfg(target_os = "macos")]
pub struct Listening {
    source: dispatch2::DispatchRetained<dispatch2::DispatchSource>,
}

/// **Cancelled when dropped**, so no handler runs after the watch it marks
/// is gone: the setting turned off. The cancel is asynchronous, and
/// "Closing the descriptor or port before the cancellation handler is
/// invoked may result in a race condition" (`[[source:
/// macos-sdk-dispatch-source-h]]` 461-468), so the kqueue is not closed
/// here: the cancel handler frees the context, and with it the context's
/// share of the descriptor.
#[cfg(target_os = "macos")]
impl Drop for Listening {
    fn drop(&mut self) {
        self.source.cancel();
    }
}

/// What the handler needs, behind its one context pointer: the kqueue
/// held open until the cancel handler has run.
#[cfg(target_os = "macos")]
struct Context {
    kq: std::sync::Arc<std::os::fd::OwnedFd>,
    mark: std::sync::Arc<std::sync::atomic::AtomicBool>,
    wake: Box<dyn Fn()>,
}

#[cfg(target_os = "macos")]
extern "C" fn on_ready(ctx: *mut core::ffi::c_void) {
    use std::os::fd::AsRawFd;
    // SAFETY: the context `listen` boxed for this source, freed only by
    // `on_cancel`, after which no event handler runs ("Cancellation
    // prevents any further invocation of the event handler block",
    // `[[source: macos-sdk-dispatch-source-h]]` 519-521).
    let ctx = unsafe { &*ctx.cast::<Context>() };
    if drain_kqueue(ctx.kq.as_raw_fd()) == Heard::Moved {
        ctx.mark.store(true, std::sync::atomic::Ordering::Release);
        (ctx.wake)();
    }
}

/// **The source's cancel handler**: submitted "once the source's event
/// handler has finished, indicating it is now safe to close the source's
/// handle" (`[[source: macos-sdk-dispatch-source-h]]` 523-525), it frees
/// the context; the kqueue closes here unless the watch still holds it.
#[cfg(target_os = "macos")]
extern "C" fn on_cancel(ctx: *mut core::ffi::c_void) {
    // SAFETY: the context `listen` boxed for this source, which runs this
    // once, after its last event handler.
    drop(unsafe { Box::from_raw(ctx.cast::<Context>()) });
}

#[cfg(target_os = "macos")]
impl<T: Copy + PartialOrd + core::ops::Add<Duration, Output = T>> Watch<T> {
    /// Hear the kernel on `queue` -- the main queue in the app -- calling
    /// `wake` after each batch. None when the kernel gave no watch.
    pub fn listen(
        &self,
        queue: &dispatch2::DispatchQueue,
        wake: impl Fn() + 'static,
    ) -> Option<Listening> {
        use dispatch2::DispatchObject;
        let kq = std::sync::Arc::clone(&self.kernel.as_ref()?.kq);
        let handle = usize::try_from(self.fd()?).ok()?;
        let ctx = Box::into_raw(Box::new(Context {
            kq,
            mark: std::sync::Arc::clone(&self.mark),
            wake: Box::new(wake),
        }));
        // SAFETY: the read source's type, a descriptor the context holds
        // open until the cancel handler has run, no mask ("unused (pass
        // zero for now)"), and a queue the source retains.
        let source = unsafe {
            dispatch2::DispatchSource::new(
                (&raw const dispatch2::_dispatch_source_type_read).cast_mut(),
                handle,
                0,
                Some(queue),
            )
        };
        // SAFETY: the context is the box `on_ready` reads and `on_cancel`
        // frees, valid until then.
        unsafe { source.set_context(ctx.cast()) };
        source.set_event_handler_f(on_ready);
        source.set_cancel_handler_f(on_cancel);
        source.activate();
        self.drains.set(Drains::Handler);
        Some(Listening { source })
    }
}

/// The temp directories the tests work in.
#[cfg(test)]
mod tests_support {
    use std::path::PathBuf;

    /// A directory of the test's own under the temp directory, removed
    /// when it ends.
    pub(super) struct Dir(pub(super) PathBuf);

    impl Dir {
        pub(super) fn new(name: &str) -> Dir {
            static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
            let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let d =
                std::env::temp_dir().join(format!("baud-reload-{name}-{}-{n}", std::process::id()));
            let _ = std::fs::remove_dir_all(&d);
            std::fs::create_dir_all(&d).expect("a temp dir");
            Dir(std::fs::canonicalize(&d).expect("canonical"))
        }

        pub(super) fn at(&self, p: &str) -> PathBuf {
            self.0.join(p)
        }
    }

    impl Drop for Dir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::tests_support::Dir;
    use super::*;
    use std::time::Instant;

    fn text(p: &Path) -> Watched {
        Watched {
            path: p.to_path_buf(),
            kind: Kind::Text,
        }
    }

    /// The settle's end after an event at `t`.
    fn after(t: Instant) -> Instant {
        t + SETTLE
    }

    /// What the watch makes of one change made by `change`: whether the
    /// kernel said so, and what the settle's end then found.
    fn one(w: &mut Watch<Instant>, change: impl FnOnce()) -> (Heard, Due) {
        let t = Instant::now();
        change();
        let heard = w.heard(t);
        (heard, w.due(after(t)))
    }

    #[test]
    fn an_edit_in_place_is_heard_and_reads_as_a_change() {
        let d = Dir::new("in-place");
        let cfg = d.at("config.toml");
        std::fs::write(&cfg, "font_size = 13\n").unwrap();
        let mut w = Watch::new(vec![text(&cfg)]);
        let (heard, due) = one(&mut w, || {
            std::fs::write(&cfg, "font_size = 15\n").unwrap();
        });
        assert_eq!((heard, due), (Heard::Moved, Due::Changed));
        // And the next edit, on the same file, as well.
        let (heard, due) = one(&mut w, || {
            std::fs::write(&cfg, "font_size = 16\n").unwrap();
        });
        assert_eq!((heard, due), (Heard::Moved, Due::Changed));
    }

    #[test]
    fn an_atomic_save_is_heard_through_the_directory() {
        let d = Dir::new("atomic");
        let cfg = d.at("config.toml");
        std::fs::write(&cfg, "font_size = 13\n").unwrap();
        let mut w = Watch::new(vec![text(&cfg)]);
        let (heard, due) = one(&mut w, || {
            let tmp = d.at(".config.toml.swp");
            std::fs::write(&tmp, "font_size = 15\n").unwrap();
            std::fs::rename(&tmp, &cfg).unwrap();
        });
        assert_eq!((heard, due), (Heard::Moved, Due::Changed));
    }

    /// The file a rename put in place is a new one: after the reading the
    /// watch is armed on it, so an edit to it in place is heard too.
    #[test]
    fn an_edit_after_an_atomic_save_is_heard() {
        let d = Dir::new("after-rename");
        let cfg = d.at("config.toml");
        std::fs::write(&cfg, "font_size = 13\n").unwrap();
        let mut w = Watch::new(vec![text(&cfg)]);
        let (_, due) = one(&mut w, || {
            let tmp = d.at("new");
            std::fs::write(&tmp, "font_size = 15\n").unwrap();
            std::fs::rename(&tmp, &cfg).unwrap();
        });
        assert_eq!(due, Due::Changed);
        let (heard, due) = one(&mut w, || {
            use std::io::Write;
            let mut f = std::fs::OpenOptions::new().append(true).open(&cfg).unwrap();
            f.write_all(b"padding = 4\n").unwrap();
        });
        assert_eq!((heard, due), (Heard::Moved, Due::Changed));
    }

    #[test]
    fn a_file_that_does_not_exist_yet_is_heard_when_it_is_made() {
        let d = Dir::new("created");
        let cfg = d.at("config.toml");
        let mut w = Watch::new(vec![text(&cfg)]);
        let (heard, due) = one(&mut w, || {
            std::fs::write(&cfg, "font_size = 15\n").unwrap();
        });
        assert_eq!((heard, due), (Heard::Moved, Due::Changed));
    }

    /// `~/.config/baud` missing: `~/.config` is watched, and once the
    /// directory is made it is watched itself, so the file made in it is
    /// heard.
    #[test]
    fn a_directory_that_does_not_exist_yet_is_heard_and_then_watched() {
        let d = Dir::new("dir-created");
        let cfg = d.at("baud/config.toml");
        let mut w = Watch::new(vec![text(&cfg)]);
        assert_eq!(dirs(w.files()), std::slice::from_ref(&d.0));
        let (heard, due) = one(&mut w, || {
            std::fs::create_dir(d.at("baud")).unwrap();
        });
        // The directory is not a watched file: nothing reads differently.
        assert_eq!((heard, due), (Heard::Moved, Due::Same));
        let (heard, due) = one(&mut w, || {
            std::fs::write(&cfg, "font_size = 15\n").unwrap();
        });
        assert_eq!((heard, due), (Heard::Moved, Due::Changed));
    }

    #[test]
    fn a_deleted_file_is_heard_and_reads_as_a_change() {
        let d = Dir::new("deleted");
        let cfg = d.at("config.toml");
        std::fs::write(&cfg, "font_size = 13\n").unwrap();
        let mut w = Watch::new(vec![text(&cfg)]);
        let (heard, due) = one(&mut w, || std::fs::remove_file(&cfg).unwrap());
        assert_eq!((heard, due), (Heard::Moved, Due::Changed));
    }

    /// A config that is a link, as a home-manager one is: the link swapped
    /// for one to another file, by a rename in its directory, is heard.
    #[test]
    fn a_swapped_symbolic_link_is_heard() {
        let d = Dir::new("symlink");
        let (a, b, cfg) = (d.at("a.toml"), d.at("b.toml"), d.at("config.toml"));
        std::fs::write(&a, "font_size = 13\n").unwrap();
        std::fs::write(&b, "font_size = 15\n").unwrap();
        std::os::unix::fs::symlink(&a, &cfg).unwrap();
        let mut w = Watch::new(files(std::slice::from_ref(&cfg), &[], &[]));
        assert_eq!(w.files(), [text(&cfg)], "the link, read through");
        let (heard, due) = one(&mut w, || {
            let next = d.at("next");
            std::os::unix::fs::symlink(&b, &next).unwrap();
            std::fs::rename(&next, &cfg).unwrap();
        });
        assert_eq!((heard, due), (Heard::Moved, Due::Changed));
    }

    /// **A link into a store is watched at the link, not in the store**:
    /// a home-manager config resolves to a file directly in `/nix/store`,
    /// whose every write would be a settle and a reading. The link's own
    /// directory is watched, where a switch puts the next link, and the
    /// link, read through; another file written beside the target is not
    /// heard.
    #[test]
    fn a_link_into_a_store_is_watched_at_the_link_not_in_the_store() {
        let d = Dir::new("store");
        std::fs::create_dir_all(d.at("store")).unwrap();
        std::fs::create_dir_all(d.at("cfg")).unwrap();
        let (target, cfg) = (d.at("store/h-baud-config.toml"), d.at("cfg/config.toml"));
        std::fs::write(&target, "font_size = 13\n").unwrap();
        std::os::unix::fs::symlink(&target, &cfg).unwrap();
        let mut w = Watch::new(files(std::slice::from_ref(&cfg), &[], &[]));
        assert_eq!(dirs(w.files()), [d.at("cfg")]);
        let (heard, due) = one(&mut w, || {
            std::fs::write(d.at("store/h2-other"), "x").unwrap();
        });
        assert_eq!(
            (heard, due),
            (Heard::Quiet, Due::Wait),
            "the store is not watched"
        );
        // The control: a switch, the next link renamed over this one.
        let (heard, due) = one(&mut w, || {
            let next = d.at("store/h3-baud-config.toml");
            std::fs::write(&next, "font_size = 15\n").unwrap();
            let link = d.at("cfg/.config.toml.new");
            std::os::unix::fs::symlink(&next, &link).unwrap();
            std::fs::rename(&link, &cfg).unwrap();
        });
        assert_eq!((heard, due), (Heard::Moved, Due::Changed));
    }

    /// **An edit through a link is heard where it lands**, with its
    /// directory unwatched: in place, and a save by rename over the target,
    /// which the target's own watch hears; armed again at the reading, the
    /// file the rename put there is watched in turn.
    #[test]
    fn an_edit_through_a_link_is_heard_in_place_and_by_rename() {
        let d = Dir::new("through");
        std::fs::create_dir_all(d.at("dot")).unwrap();
        std::fs::create_dir_all(d.at("cfg")).unwrap();
        let (target, cfg) = (d.at("dot/baud.toml"), d.at("cfg/config.toml"));
        std::fs::write(&target, "font_size = 13\n").unwrap();
        std::os::unix::fs::symlink(&target, &cfg).unwrap();
        let mut w = Watch::new(files(std::slice::from_ref(&cfg), &[], &[]));
        let (heard, due) = one(&mut w, || {
            std::fs::write(&target, "font_size = 14\n").unwrap();
        });
        assert_eq!((heard, due), (Heard::Moved, Due::Changed), "in place");
        for n in [15, 16] {
            let (heard, due) = one(&mut w, || {
                let tmp = d.at("dot/.baud.toml.swp");
                std::fs::write(&tmp, format!("font_size = {n}\n")).unwrap();
                std::fs::rename(&tmp, &target).unwrap();
            });
            assert_eq!((heard, due), (Heard::Moved, Due::Changed), "by rename, {n}");
        }
    }

    /// **A config directory that is a link**, as home-manager makes one
    /// for a whole directory: the switch renames the next link over it in
    /// its own directory, which is watched as well.
    #[test]
    fn a_linked_config_directory_swapped_is_heard() {
        let d = Dir::new("dir-link");
        for g in ["gen1", "gen2", "home"] {
            std::fs::create_dir_all(d.at(g)).unwrap();
        }
        std::fs::write(d.at("gen1/config.toml"), "font_size = 13\n").unwrap();
        std::fs::write(d.at("gen2/config.toml"), "font_size = 15\n").unwrap();
        let linked = d.at("home/baud");
        std::os::unix::fs::symlink(d.at("gen1"), &linked).unwrap();
        let cfg = linked.join("config.toml");
        let mut w = Watch::new(files(std::slice::from_ref(&cfg), &[], &[]));
        assert_eq!(dirs(w.files()), [linked.clone(), d.at("home")]);
        let (heard, due) = one(&mut w, || {
            let next = d.at("home/.baud.new");
            std::os::unix::fs::symlink(d.at("gen2"), &next).unwrap();
            std::fs::rename(&next, &linked).unwrap();
        });
        assert_eq!((heard, due), (Heard::Moved, Due::Changed));
    }

    /// **Only the file's own directory brings its link's directory**: a
    /// missing config seen from a directory above it that is a link -- a
    /// `~/.config` linked into dotfiles, `/tmp` on macOS -- is watched at
    /// the link alone, and the home directory and `/` are never added, as
    /// WezTerm never watches the home directory (`[[source:
    /// wezterm-20240203-config-lib]]`).
    #[test]
    fn a_link_above_a_missing_config_brings_no_directory_above_it() {
        let d = Dir::new("link-above");
        std::fs::create_dir_all(d.at("dots/config")).unwrap();
        std::fs::create_dir_all(d.at("home")).unwrap();
        let linked = d.at("home/.config");
        std::os::unix::fs::symlink(d.at("dots/config"), &linked).unwrap();
        let missing = text(&linked.join("baud/config.toml"));
        assert_eq!(
            dirs(std::slice::from_ref(&missing)),
            std::slice::from_ref(&linked)
        );
        // A linked config directory directly under the home directory:
        // the link, and not the home directory.
        let beside = d.at("home/baud");
        std::os::unix::fs::symlink(d.at("dots/config"), &beside).unwrap();
        let cfg = text(&beside.join("config.toml"));
        assert_eq!(
            dirs_beside(std::slice::from_ref(&cfg), Some(&d.at("home"))),
            std::slice::from_ref(&beside)
        );
        // The control: the same link anywhere else brings its directory.
        assert_eq!(
            dirs_beside(std::slice::from_ref(&cfg), None),
            [beside, d.at("home")]
        );
    }

    /// Another program's file in the same directory is heard -- a vnode
    /// event names no file -- and reads as no change.
    #[test]
    fn a_neighbour_is_heard_and_changes_nothing() {
        let d = Dir::new("neighbour");
        let cfg = d.at("config.toml");
        std::fs::write(&cfg, "font_size = 13\n").unwrap();
        let mut w = Watch::new(vec![text(&cfg)]);
        let (heard, due) = one(&mut w, || {
            std::fs::write(d.at("other.txt"), "x").unwrap();
        });
        assert_eq!((heard, due), (Heard::Moved, Due::Same));
    }

    #[test]
    fn nothing_heard_is_nothing_read() {
        let d = Dir::new("quiet");
        let cfg = d.at("config.toml");
        std::fs::write(&cfg, "font_size = 13\n").unwrap();
        let mut w: Watch<Instant> = Watch::new(vec![text(&cfg)]);
        let t = Instant::now();
        assert_eq!(w.heard(t), Heard::Quiet);
        assert_eq!(w.deadline(), None);
        assert_eq!(w.due(t + Duration::from_secs(1)), Due::Wait);
    }

    /// **A burst is one reading**, a settle after its last event: each
    /// event pushes the deadline back.
    #[test]
    fn a_burst_settles_into_one_reading_after_its_last_event() {
        let t0 = Instant::now();
        let ms = Duration::from_millis;
        let mut s = Settle::new();
        s.heard(t0);
        s.heard(t0 + ms(6));
        assert_eq!(s.deadline(), Some(t0 + ms(6) + SETTLE));
        assert!(!s.take(t0 + ms(12)), "the second event pushed it back");
        assert!(s.take(t0 + ms(16)));
        assert!(!s.take(t0 + ms(30)), "once a burst");
    }

    /// An editor that truncates and then writes: written before the settle
    /// ends, only the written bytes are read; the settle passing between
    /// the two, the empty file is read -- an empty config is the defaults,
    /// Jev's lean EMPTY_IS_DEFAULTS, 0.80 and 0.80 -- and the write after
    /// it is a second change.
    #[test]
    fn a_truncate_then_write_reads_once_inside_the_settle() {
        let d = Dir::new("truncate");
        let cfg = d.at("config.toml");
        std::fs::write(&cfg, "font_size = 13\n").unwrap();
        let mut w = Watch::new(vec![text(&cfg)]);
        let t0 = Instant::now();
        let ms = Duration::from_millis;
        std::fs::write(&cfg, "").unwrap();
        assert_eq!(w.heard(t0), Heard::Moved);
        std::fs::write(&cfg, "font_size = 15\n").unwrap();
        assert_eq!(w.heard(t0 + ms(3)), Heard::Moved);
        assert_eq!(w.due(t0 + ms(11)), Due::Wait);
        assert_eq!(w.due(t0 + ms(13)), Due::Changed);
        assert_eq!(w.last.0[0].1, Held::Text(b"font_size = 15\n".to_vec()));

        let t1 = t0 + ms(100);
        std::fs::write(&cfg, "").unwrap();
        assert_eq!(w.heard(t1), Heard::Moved);
        assert_eq!(w.due(after(t1)), Due::Changed);
        assert_eq!(w.last.0[0].1, Held::Text(Vec::new()));
        std::fs::write(&cfg, "font_size = 16\n").unwrap();
        let t2 = after(t1) + ms(1);
        assert_eq!(w.heard(t2), Heard::Moved);
        assert_eq!(w.due(after(t2)), Due::Changed);
    }

    /// **A file the config names is watched once the config names it**:
    /// a theme file named by a reload is heard from then on, and a change
    /// that landed between the reading and the new watch is heard too.
    #[test]
    fn a_file_a_reload_names_is_watched_from_then_on() {
        let d = Dir::new("rewatch");
        let (cfg, theme) = (d.at("config.toml"), d.at("themes/day.yaml"));
        std::fs::write(&cfg, "font_size = 13\n").unwrap();
        let mut w: Watch<Instant> = Watch::new(vec![text(&cfg)]);
        let t = Instant::now();
        std::fs::create_dir(d.at("themes")).unwrap();
        std::fs::write(&theme, "a").unwrap();
        w.rewatch(vec![text(&cfg), text(&theme)], t);
        assert_eq!(w.deadline(), None, "nothing both readings hold moved");
        let (heard, due) = one(&mut w, || std::fs::write(&theme, "b").unwrap());
        assert_eq!((heard, due), (Heard::Moved, Due::Changed));
        // The config changed after the reading the reload made, before
        // the watch was armed again: the rewatch hears it.
        let last = w.last.clone();
        std::fs::write(&cfg, "font_size = 15\n").unwrap();
        let _ = w.heard(t);
        w.last = last;
        w.settle = Settle::new();
        w.rewatch(vec![text(&cfg), text(&theme)], t);
        assert_eq!(w.deadline(), Some(after(t)));
        assert_eq!(w.due(after(t)), Due::Changed, "and read as a change");
    }

    /// **An edit between the launch's read of the config and the watch's
    /// first reading is heard**: the watch is taken, and the files read,
    /// before the config is, so the reading after it differs from the one
    /// before and the settle's end reads a change.
    #[test]
    fn an_edit_between_the_launch_read_and_the_watch_is_heard() {
        let d = Dir::new("launch-race");
        let cfg = d.at("config.toml");
        std::fs::write(&cfg, "font_size = 13\n").unwrap();
        let t = Instant::now();
        let (read, mut w) = Watch::around(std::slice::from_ref(&cfg), t, || {
            let read = std::fs::read(&cfg).unwrap();
            // The edit lands after the launch read the file.
            std::fs::write(&cfg, "font_size = 15\n").unwrap();
            (read, vec![text(&cfg)])
        });
        assert_eq!(read, b"font_size = 13\n", "the launch read the old file");
        let _ = w.heard(t);
        assert_eq!(w.deadline(), Some(after(t)), "the edit is heard");
        assert_eq!(w.due(after(t)), Due::Changed, "and reads as a change");
    }

    /// The control: nothing changed around the launch's read, nothing is
    /// due.
    #[test]
    fn a_launch_with_no_edit_owes_no_reading() {
        let d = Dir::new("launch-quiet");
        let cfg = d.at("config.toml");
        std::fs::write(&cfg, "font_size = 13\n").unwrap();
        let t = Instant::now();
        let (_, mut w) = Watch::around(std::slice::from_ref(&cfg), t, || ((), vec![text(&cfg)]));
        assert_eq!(w.heard(t), Heard::Quiet);
        assert_eq!(w.deadline(), None);
        assert_eq!(w.due(t + Duration::from_secs(1)), Due::Wait);
    }

    #[test]
    fn a_font_is_compared_by_identity_not_bytes() {
        let d = Dir::new("face");
        let face = d.at("Mono.ttf");
        std::fs::write(&face, [0u8; 64]).unwrap();
        let mut w = Watch::new(files(&[], &[], std::slice::from_ref(&face)));
        assert_eq!(w.files()[0].kind, Kind::Face);
        let (heard, due) = one(&mut w, || {
            let tmp = d.at("Mono.ttf.new");
            std::fs::write(&tmp, [1u8; 64]).unwrap();
            std::fs::rename(&tmp, &face).unwrap();
        });
        assert_eq!((heard, due), (Heard::Moved, Due::Changed));
    }

    #[test]
    fn each_file_once_and_each_directory_once() {
        let d = Dir::new("lists");
        let a = d.at("config.toml");
        let t = d.at("themes/day");
        let got = files(&[a.clone(), a.clone()], std::slice::from_ref(&t), &[]);
        assert_eq!(got, [text(&a), text(&t)]);
        // `themes` does not exist: its file is seen from the directory
        // above it, which is the config's own, listed once.
        assert_eq!(dirs(&got), std::slice::from_ref(&d.0));
    }

    /// **The kernel heard on a dispatch queue, with no thread of baud's
    /// waiting**: the handler drains the kqueue on the queue and wakes;
    /// the mark it leaves is what the driver's look takes.
    #[cfg(target_os = "macos")]
    #[test]
    fn a_dispatch_source_hears_the_kqueue_and_marks_the_watch() {
        let d = Dir::new("dispatch");
        let cfg = d.at("config.toml");
        std::fs::write(&cfg, "font_size = 13\n").unwrap();
        let mut w: Watch<Instant> = Watch::new(vec![text(&cfg)]);
        let queue = dispatch2::DispatchQueue::new("baud.reload.test", None);
        let (tx, rx) = std::sync::mpsc::channel();
        let tx = std::sync::Mutex::new(tx);
        let _heard = w
            .listen(&queue, move || {
                let _ = tx.lock().map(|t| t.send(()));
            })
            .expect("a source");
        std::fs::write(&cfg, "font_size = 15\n").unwrap();
        rx.recv_timeout(Duration::from_secs(10))
            .expect("the handler ran");
        // The handler drained the kqueue; the mark carries the event.
        let t = Instant::now();
        assert_eq!(w.heard(t), Heard::Moved);
        assert_eq!(w.due(after(t)), Due::Changed);
    }

    /// **An event the kqueue holds before the source listens is heard**:
    /// the launch's watch is taken before the window, and a change in
    /// between waits in the kqueue until the source's handler drains it.
    #[cfg(target_os = "macos")]
    #[test]
    fn an_event_before_the_source_listens_is_heard_once_it_does() {
        let d = Dir::new("dispatch-early");
        let cfg = d.at("config.toml");
        std::fs::write(&cfg, "font_size = 13\n").unwrap();
        let mut w: Watch<Instant> = Watch::new(vec![text(&cfg)]);
        std::fs::write(&cfg, "font_size = 15\n").unwrap();
        let queue = dispatch2::DispatchQueue::new("baud.reload.early", None);
        let (tx, rx) = std::sync::mpsc::channel();
        let tx = std::sync::Mutex::new(tx);
        let _heard = w
            .listen(&queue, move || {
                let _ = tx.lock().map(|t| t.send(()));
            })
            .expect("a source");
        rx.recv_timeout(Duration::from_secs(10))
            .expect("the handler ran for the event before it");
        let t = Instant::now();
        assert_eq!(w.heard(t), Heard::Moved);
        assert_eq!(w.due(after(t)), Due::Changed);
    }

    /// **The kqueue stays open until the cancel handler has run**, as
    /// `dispatch_source_set_cancel_handler()` requires of a descriptor's
    /// source: dropped with its source cancelled and the queue held, the
    /// watch leaves the descriptor open; let go, the handler frees the
    /// context, and its `wake` with it.
    #[cfg(target_os = "macos")]
    #[test]
    fn a_dropped_watch_keeps_its_kqueue_open_until_the_cancel_handler_has_run() {
        use dispatch2::DispatchObject;
        let d = Dir::new("cancel");
        let cfg = d.at("config.toml");
        std::fs::write(&cfg, "font_size = 13\n").unwrap();
        let w: Watch<Instant> = Watch::new(vec![text(&cfg)]);
        let kq = w.fd().expect("a kqueue");
        let open = |fd| {
            let zero = crate::sys::Timespec {
                tv_sec: 0,
                tv_nsec: 0,
            };
            // SAFETY: no changes, no event list, a zero timeout.
            let r = unsafe {
                crate::sys::kevent(
                    fd,
                    std::ptr::null(),
                    0,
                    std::ptr::null_mut(),
                    0,
                    &raw const zero,
                )
            };
            r == 0
        };
        assert!(open(kq), "the control: a live watch's kqueue answers");
        let queue = dispatch2::DispatchQueue::new("baud.reload.cancel", None);
        let (tx, rx) = std::sync::mpsc::channel::<()>();
        let tx = std::sync::Mutex::new(tx);
        let heard = w
            .listen(&queue, move || {
                let _ = tx.lock().map(|t| t.send(()));
            })
            .expect("a source");
        // Nothing on the queue runs while it is suspended, the cancel
        // handler included.
        queue.suspend();
        drop(heard);
        drop(w);
        let held = open(kq);
        queue.resume();
        assert!(held, "closed before the cancel handler ran");
        assert_eq!(
            rx.recv_timeout(Duration::from_secs(10)),
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected),
            "the cancel handler freed the context"
        );
    }
}

/// **A config file edited, saved, broken, emptied, deleted and made, read
/// again and applied**: the watch hears each change, the reading decides
/// what is applied, and what reaches the core is read back from it. Every
/// driver applies through these same steps (`live::term_options`,
/// `Term::reconfigure_cursor_style`, `live::between`); what only a window
/// can show -- the palette on screen, the font's pixels -- is the drivers'.
#[cfg(test)]
mod flow {
    use super::tests_support::Dir;
    use super::*;
    use crate::config::{Config, Reread};
    use crate::live::{FaceFiles, Notice, Part, between};
    use std::time::Instant;

    /// One change made by `change`, heard and read: the reading, when the
    /// files read differently.
    fn change(
        w: &mut Watch<Instant>,
        paths: &[PathBuf],
        in_force: Option<&Path>,
        change: impl FnOnce(),
    ) -> Reread {
        let t = Instant::now();
        change();
        assert_eq!(w.heard(t), Heard::Moved, "the kernel heard it");
        assert_eq!(w.due(t + SETTLE), Due::Changed, "it reads differently");
        Config::reread_at(paths, in_force)
    }

    fn read(r: Reread) -> crate::config::Reading {
        match r {
            Reread::Read(r) => *r,
            Reread::Broken(s) => panic!("broken: {s}"),
            Reread::Gone(p) => panic!("gone: {}", p.display()),
        }
    }

    /// The config `path` holds now, read as a launch reads it.
    fn launch(path: &Path) -> Config {
        read(Config::reread_at(&[path.to_path_buf()], None)).config
    }

    #[test]
    fn a_saved_config_is_read_again_and_its_settings_reach_the_core() {
        let d = Dir::new("flow-save");
        let cfg = d.at("config.toml");
        std::fs::write(
            &cfg,
            "font_size = 13\n[screen]\nchecksum = \"on\"\n[cursor]\nshape = \"block\"\n",
        )
        .unwrap();
        let old = launch(&cfg);
        let mut w = Watch::new(files(std::slice::from_ref(&cfg), &[], &[]));
        let paths = [cfg.clone()];
        let new = read(change(&mut w, &paths, Some(&cfg), || {
            let tmp = d.at(".config.toml.tmp");
            std::fs::write(
                &tmp,
                "font_size = 15\n[screen]\nchecksum = \"off\"\n[cursor]\nshape = \"bar\"\n",
            )
            .unwrap();
            std::fs::rename(&tmp, &cfg).unwrap();
        }));
        assert!(new.said.is_empty(), "{:?}", new.said);
        let parts = between(&old, &new.config, FaceFiles::Same);
        for p in [Part::FontSize, Part::TermOptions, Part::CursorStyle] {
            assert!(parts.contains(&p), "{p:?} in {parts:?}");
        }
        assert_eq!(new.config.font_size, 15.0);
        // What reaches the core, read back from it.
        let ws = baud_domain::WinSize {
            ws_col: 20,
            ws_row: 4,
        };
        let mut owned = baud_core::Owned::new(&ws);
        let mut term = baud_core::Term::new(owned.storage(), &ws);
        let start = baud_core::CursorStyle::of(old.cursor_shape, old.cursor_blink.power_on());
        term.set_initial_cursor_style(start);
        crate::live::term_options(&mut term, &old, crate::winops::mac_abilities());
        assert_eq!(
            term.checksum_report(),
            baud_domain::ChecksumReport::On,
            "the control"
        );
        crate::live::term_options(&mut term, &new.config, crate::winops::mac_abilities());
        term.reconfigure_cursor_style(baud_core::CursorStyle::of(
            new.config.cursor_shape,
            new.config.cursor_blink.power_on(),
        ));
        assert_eq!(term.checksum_report(), baud_domain::ChecksumReport::Off);
        assert_eq!(
            term.cursor_style(),
            baud_core::CursorStyle::of(
                baud_domain::CursorShape::Bar,
                new.config.cursor_blink.power_on()
            )
        );
        assert_ne!(term.cursor_style(), start, "the cursor changed");
    }

    /// **A file that does not parse keeps the last good config**, and says
    /// which line, until a good save, whose notice is empty.
    #[test]
    fn a_file_that_does_not_parse_keeps_the_last_settings_until_a_good_save() {
        let d = Dir::new("flow-broken");
        let cfg = d.at("config.toml");
        std::fs::write(&cfg, "font_size = 13\n").unwrap();
        let mut w = Watch::new(files(std::slice::from_ref(&cfg), &[], &[]));
        let paths = [cfg.clone()];
        let broken = change(&mut w, &paths, Some(&cfg), || {
            std::fs::write(&cfg, "font_size = 15\nfont_size 16\n").unwrap();
        });
        match broken {
            Reread::Broken(s) => {
                assert!(s.contains("line 2"), "{s}");
                assert!(s.contains("\"font_size 16\" is not key = value"), "{s}");
                assert!(s.ends_with("the last good settings stay"), "{s}");
            }
            _ => panic!("a line that is not key = value is a broken file"),
        }
        let unclosed = change(&mut w, &paths, Some(&cfg), || {
            std::fs::write(&cfg, "fonts = [\"Menlo\",\n\"Monaco\"\n").unwrap();
        });
        assert!(
            matches!(unclosed, Reread::Broken(s) if s.contains("line 1: a list opened here is never closed"))
        );
        let not_text = change(&mut w, &paths, Some(&cfg), || {
            std::fs::write(&cfg, [0x66, 0xff, 0x0a]).unwrap();
        });
        assert!(matches!(not_text, Reread::Broken(s) if s.contains("is not UTF-8 text")));
        let good = read(change(&mut w, &paths, Some(&cfg), || {
            std::fs::write(&cfg, "font_size = 16\n").unwrap();
        }));
        assert_eq!(good.config.font_size, 16.0);
        assert_eq!(
            Notice::of(&good.said),
            Notice::NONE,
            "a good save clears the row"
        );
    }

    /// A key the reader cannot act on is applied as a launch applies it,
    /// that key at its default, and said on the row.
    #[test]
    fn a_complaint_is_applied_and_said() {
        let d = Dir::new("flow-said");
        let cfg = d.at("config.toml");
        std::fs::write(&cfg, "font_size = 15\n").unwrap();
        let mut w = Watch::new(files(std::slice::from_ref(&cfg), &[], &[]));
        let paths = [cfg.clone()];
        let r = read(change(&mut w, &paths, Some(&cfg), || {
            std::fs::write(&cfg, "font_size = big\npadding = 3\n").unwrap();
        }));
        assert_eq!(r.config.font_size, Config::defaults().font_size);
        assert_eq!(r.config.padding, 3.0, "the rest applied");
        let notice = Notice::of(&r.said);
        assert!(
            notice
                .text()
                .is_some_and(|t| t.contains("font_size \"big\"")),
            "{notice:?}"
        );
    }

    /// **The file in force deleted keeps the last settings**, said, and
    /// does not fall through to a file below it in the lookup; made again,
    /// it is read.
    #[test]
    fn a_deleted_file_keeps_the_last_settings_until_it_is_back() {
        let d = Dir::new("flow-gone");
        let (cfg, low) = (d.at("config.toml"), d.at("low.toml"));
        std::fs::write(&cfg, "font_size = 15\n").unwrap();
        std::fs::write(&low, "font_size = 9\n").unwrap();
        let paths = [cfg.clone(), low.clone()];
        let mut w = Watch::new(files(&paths, &[], &[]));
        let gone = change(&mut w, &paths, Some(&cfg), || {
            std::fs::remove_file(&cfg).unwrap()
        });
        assert!(matches!(&gone, Reread::Gone(p) if *p == cfg));
        assert!(crate::config::gone(&cfg).ends_with("the last settings stay until it is back"));
        let back = read(change(&mut w, &paths, Some(&cfg), || {
            std::fs::write(&cfg, "font_size = 17\n").unwrap();
        }));
        assert_eq!(back.config.font_size, 17.0);
        assert_eq!(back.config.source.as_deref(), Some(cfg.as_path()));
    }

    /// An emptied file is the defaults, as at launch.
    #[test]
    fn an_emptied_file_is_the_defaults() {
        let d = Dir::new("flow-empty");
        let cfg = d.at("config.toml");
        std::fs::write(&cfg, "font_size = 15\n").unwrap();
        let old = launch(&cfg);
        let mut w = Watch::new(files(std::slice::from_ref(&cfg), &[], &[]));
        let paths = [cfg.clone()];
        let r = read(change(&mut w, &paths, Some(&cfg), || {
            std::fs::write(&cfg, "").unwrap()
        }));
        assert_eq!(r.config.font_size, Config::defaults().font_size);
        assert!(between(&old, &r.config, FaceFiles::Same).contains(&Part::FontSize));
    }

    /// **A file made above the one in force takes over**, as a launch
    /// would read it first; and one made where none was is read.
    #[test]
    fn a_file_made_above_the_one_in_force_takes_over() {
        let d = Dir::new("flow-above");
        let (high, low) = (d.at("baud/config.toml"), d.at("low.toml"));
        std::fs::write(&low, "font_size = 9\n").unwrap();
        let paths = [high.clone(), low.clone()];
        let mut w = Watch::new(files(&paths, &[], &[]));
        let r = read(change(&mut w, &paths, Some(&low), || {
            std::fs::create_dir(d.at("baud")).unwrap();
            std::fs::write(&high, "font_size = 21\n").unwrap();
        }));
        assert_eq!(r.config.source.as_deref(), Some(high.as_path()));
        assert_eq!(r.config.font_size, 21.0);
    }

    /// **`[text] ambiguous_width` changes at once**: the core takes it for
    /// the next character (`Term::set_ambiguous_width`), and `DECRQM
    /// ?1021` answers set or reset, never permanently, so no earlier answer
    /// is made false. The text already on the screen keeps its widths.
    #[test]
    fn a_saved_ambiguous_width_reaches_the_core_at_once() {
        use baud_domain::{AmbiguousWidth, BytePos, ReadLen};
        let d = Dir::new("flow-ambiguous");
        let cfg = d.at("config.toml");
        std::fs::write(&cfg, "font_size = 13\n").unwrap();
        let old = launch(&cfg);
        let mut w = Watch::new(files(std::slice::from_ref(&cfg), &[], &[]));
        let paths = [cfg.clone()];
        let new = read(change(&mut w, &paths, Some(&cfg), || {
            std::fs::write(&cfg, "font_size = 13\n[text]\nambiguous_width = \"wide\"\n").unwrap();
        }));
        assert_eq!(new.config.ambiguous_width, AmbiguousWidth::Wide);
        let parts = between(&old, &new.config, FaceFiles::Same);
        assert!(
            parts.iter().any(|p| p.name() == "[text] ambiguous_width"),
            "{parts:?}"
        );
        assert_eq!(crate::live::later(&parts), None, "nothing waits");
        let ws = baud_domain::WinSize {
            ws_col: 20,
            ws_row: 4,
        };
        let mut owned = baud_core::Owned::new(&ws);
        let mut term = baud_core::Term::new(owned.storage(), &ws);
        crate::host::set_widths(&mut term, old.vs16_width, old.ambiguous_width);
        let feed = |t: &mut baud_core::Term<'_>, b: &[u8]| {
            t.write_at(BytePos::ZERO, b);
            t.feed(BytePos::ZERO, ReadLen::from_index(b.len()));
        };
        // U+00B1 is East Asian Ambiguous.
        feed(&mut term, "\u{b1}".as_bytes());
        assert_eq!(term.cursor().1.index(), 1, "the control: narrow");
        term.set_ambiguous_width(new.config.ambiguous_width);
        assert_eq!(term.ambiguous_width(), AmbiguousWidth::Wide);
        feed(&mut term, "\u{b1}".as_bytes());
        assert_eq!(term.cursor().1.index(), 3, "the next one is wide");
    }

    /// `config_reload = "off"` is read and named as a change of its own.
    #[test]
    fn the_setting_turned_off_is_read_as_its_own_change() {
        let d = Dir::new("flow-off");
        let cfg = d.at("config.toml");
        std::fs::write(&cfg, "font_size = 13\n").unwrap();
        let old = launch(&cfg);
        assert_eq!(
            old.config_reload,
            crate::config::ConfigReload::On,
            "on by default"
        );
        let mut w = Watch::new(files(std::slice::from_ref(&cfg), &[], &[]));
        let paths = [cfg.clone()];
        let r = read(change(&mut w, &paths, Some(&cfg), || {
            std::fs::write(&cfg, "config_reload = \"off\"\n").unwrap();
        }));
        assert_eq!(r.config.config_reload, crate::config::ConfigReload::Off);
        assert!(between(&old, &r.config, FaceFiles::Same).contains(&Part::Reload));
    }

    /// **Both drivers watch what a config read again names before they
    /// read it**, as the launch does: the rewatch comes before the apply in
    /// each `reload_config`, so a theme or font file named for the first
    /// time and edited while it is applied is heard. The drivers start
    /// AppKit or Wayland, so the order is held in their source.
    #[test]
    fn both_drivers_rewatch_before_they_apply() {
        for (driver, src) in [
            ("macOS", include_str!("app.rs")),
            ("Linux", include_str!("linux.rs")),
        ] {
            let body = &src[src.find("    fn reload_config(").expect(driver)..];
            let body = &body[..body.find("\n    }\n").expect(driver)];
            let at = |needle: &str| {
                assert_eq!(body.matches(needle).count(), 1, "{driver}: {needle}");
                body.find(needle).unwrap_or(usize::MAX)
            };
            assert!(
                at("r.watch.rewatch(") < at("self.apply_config(&config, &parts)"),
                "{driver}: the rewatch before the apply"
            );
        }
    }
}
