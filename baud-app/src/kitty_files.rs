//! **The file mediums of the terminal graphics protocol (APC G), read for
//! the core** (`[[source: kitty-graphics]]`, "The transmission medium"):
//! `t=f` a
//! file, `t=t` a temporary file deleted once read, `t=s` a POSIX
//! shared-memory object unlinked once read. The core hands out the decoded
//! path, `O`, `S` and the most bytes the transmission can use
//! ([`KittyFileRequest`]) and never opens a file; this is the
//! [`KittyFiles`] both drivers lend it.
//!
//! **Every check the text asks for, made on the path before the open**,
//! because "merely opening a file can have side-effects":
//!
//! 1. The path is resolved, symbolic links followed ("the terminal emulator
//!    must follow symlinks"), and a loop is an error like any other.
//! 2. A resolved path under `/proc`, `/sys` or `/dev` is refused ("may
//!    refuse to read files in sensitive parts of the filesystem"), except
//!    a file under `/dev/shm`, which the text names a temporary directory.
//! 3. It must be a regular file: "only regular files are allowed", so a
//!    directory, a FIFO, a device or a socket is refused unopened.
//! 4. **The open cannot be swapped under the check**, and in the same
//!    order on Darwin and Linux: the resolved path, which has no link in
//!    it, is walked from `/` a directory at a time, each opened in the one
//!    before it to search, following no link (`openat`, `O_NOFOLLOW`), and
//!    the last must have the device and inode the path resolved to -- so a
//!    directory swapped for a link, or renamed in, is refused before the
//!    file is opened. The file is then opened in that directory following
//!    no link and with `O_NONBLOCK`, so a FIFO put there cannot block the
//!    open, and the descriptor's own `fstat` must be a regular file with
//!    the device and inode the check saw, so any other swap is refused
//!    before a byte is read. kitty checks the path again only after its
//!    open (`[[source: kitty-utils-py]]`). Linux is type-checked, not run.
//!
//! The length is decided from that `fstat` before reading
//! ([`KittyFileRequest::read_len`]), so no object is read past what the
//! image store was given, and the read is `pread` in 64 KiB pieces.
//!
//! **Where the read runs.** [`LocalFiles`] reads where it is called: lent
//! to a terminal, at the command's terminator, inside the read. A terminal
//! that holds the file mediums (`Term::hold_kitty_files`) stops its read at
//! the terminator instead, and [`FileReader`] runs every check and the
//! read on a thread of its own -- resolving the path is the first call a
//! stalled mount holds -- handing the pieces over through a channel the
//! thread feeding the terminal takes from without waiting
//! (`reads::paced`), which gives the read up past its deadline. At most
//! [`READERS`] reads run at once in a process.
//!
//! **Every failure is one error**, kitty's `EBADF:Failed to read image
//! file`, so "the responses ... learn [nothing] about files it cannot read
//! itself"; the reason goes to the log at `warn`, which only the local
//! user reads, as kitty writes it to its own. The core answers the same
//! for whatever refuses the command once this was asked -- a PNG that
//! does not decode, a placement refused -- so a reply never says the file
//! was there.
//!
//! **What is deleted.** A `t=t` file is deleted only once it was opened and
//! read as the file the path named, from the directory it was read from
//! (`unlinkat` on the walk's descriptor, so no link swapped in above it
//! moves the deletion elsewhere), and only when its resolved path lies
//! in a known temporary directory -- `/tmp`, `/dev/shm`, `TMPDIR` and the
//! platform's own -- compared component by component, where kitty's
//! temporary-directory test compares strings and lets `/tmpfoo` pass for
//! `/tmp` (`[[source: kitty-utils-py]]`) -- and contains
//! `tty-graphics-protocol`; kitty deletes a file it opened even when the
//! load then fails, so it does not accumulate (`[[source:
//! kitty-graphics-c]]` `load_image_data`), and so does this. A `t=s`
//! object is unlinked once opened, read or not.
//!
//! `unsafe` here is the walk, the deletion and the shared-memory read:
//! `openat`, `unlinkat`, `shm_open` and `shm_unlink`, a descriptor taken
//! into a `File`, and on Darwin the mapping its shared memory is read
//! through.

#![allow(unsafe_code)]

use baud_domain::{
    KittyFileFailed, KittyFileRequest, KittyFiles, KittyFlow, KittyMedium, KittyPastLimit,
};
use std::ffi::{CString, OsStr};
use std::fs::{File, Metadata};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{FileExt, MetadataExt};
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, TryRecvError};

/// The piece a file is read and handed to the core in.
const PIECE: usize = 1 << 16;

/// What a `t=t` path must contain to be deleted.
const MARKER: &[u8] = b"tty-graphics-protocol";

/// **Why a read was refused**, for the log alone: the client is answered
/// the same error for each.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Refusal {
    /// A relative path names a file against baud's own working directory,
    /// which the client cannot know.
    NotAbsolute,
    /// A NUL inside the path.
    Nul,
    /// Resolving the path failed: it does not exist, a loop, a component
    /// that is no directory, no permission.
    Resolve,
    /// Under `/proc`, `/sys` or `/dev`.
    Sensitive,
    /// Not a regular file before the open.
    NotRegular,
    /// The open failed: no permission, or a link swapped in.
    Open,
    /// A directory above the file is a link, or not the directory the
    /// path resolved to.
    Directory,
    /// The descriptor is not the file the check saw, or not regular.
    Swapped,
    /// Past the store's bound where the request refuses it
    /// ([`KittyPastLimit::Refuse`]); `S` past the object is a bound, not
    /// this.
    Size,
    /// The read failed, or ended short: the file shrank while read.
    Read,
    /// A shared-memory name that does not start with `/`, has another
    /// `/`, or is too long.
    ShmName,
    /// `shm_open` failed.
    ShmOpen,
    /// Mapping the object failed: Darwin, whose shared memory is mapped.
    #[cfg(target_os = "macos")]
    ShmMap,
    /// Not a file medium.
    Medium,
}

/// **The reader both drivers lend the core**: the temporary directories it
/// may delete in, resolved once, and the buffer a file is read through.
pub struct LocalFiles {
    temp: Arc<[PathBuf]>,
    buf: Vec<u8>,
    /// Run between the check and the open, so a test can swap the file
    /// there and need no race.
    #[cfg(test)]
    between: Option<Between>,
    /// Run between resolving the path and taking its directory's device
    /// and inode.
    #[cfg(test)]
    canonical: Option<Between>,
    /// Run between taking them and checking the path.
    #[cfg(test)]
    resolved: Option<Between>,
    /// Run between the read and a `t=t` file's deletion.
    #[cfg(test)]
    unlinking: Option<Between>,
}

/// What a test runs between the check and the open.
#[cfg(test)]
type Between = Box<dyn FnMut(&Path)>;

impl LocalFiles {
    /// The temporary directories from this process's environment: `/tmp`,
    /// `/dev/shm`, `TMPDIR` if set, and what `std` takes for the
    /// platform's own (Darwin's per-user directory under `TMPDIR`), each
    /// resolved, those that do not resolve left out.
    pub fn new() -> LocalFiles {
        LocalFiles::of(resolved(&temp_dirs()))
    }

    /// The reader with `dirs` as the temporary directories.
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "the drivers name the temporary directories")
    )]
    pub fn with_temp(dirs: &[PathBuf]) -> LocalFiles {
        LocalFiles::of(resolved(dirs))
    }

    /// The reader with `temp`, resolved, as the temporary directories.
    fn of(temp: Arc<[PathBuf]>) -> LocalFiles {
        LocalFiles {
            temp,
            buf: Vec::new(),
            #[cfg(test)]
            between: None,
            #[cfg(test)]
            canonical: None,
            #[cfg(test)]
            resolved: None,
            #[cfg(test)]
            unlinking: None,
        }
    }

    /// A `t=f` or `t=t` read: the checks, the open, the read, and the
    /// deletion `t=t` asks for.
    fn file(
        &mut self,
        req: &KittyFileRequest<'_>,
        sink: &mut dyn FnMut(&[u8]) -> KittyFlow,
    ) -> Result<(), Refusal> {
        if req.path.contains(&0) {
            return Err(Refusal::Nul);
        }
        let path = Path::new(OsStr::from_bytes(req.path));
        if !path.is_absolute() {
            return Err(Refusal::NotAbsolute);
        }
        let real = std::fs::canonicalize(path).map_err(|_| Refusal::Resolve)?;
        #[cfg(test)]
        if let Some(f) = self.canonical.as_mut() {
            f(&real);
        }
        // The directory the path resolved to, as it was resolved: `real`
        // has no link in it, so this is that directory itself.
        let parent = real.parent().unwrap_or(Path::new("/"));
        let resolved = std::fs::symlink_metadata(parent).map_err(|_| Refusal::Resolve)?;
        #[cfg(test)]
        if let Some(f) = self.resolved.as_mut() {
            f(&real);
        }
        if sensitive(&real) {
            return Err(Refusal::Sensitive);
        }
        // `real` has no link in it, so this is the file itself.
        let before = std::fs::symlink_metadata(&real).map_err(|_| Refusal::Resolve)?;
        if !before.file_type().is_file() {
            return Err(Refusal::NotRegular);
        }
        let name = real.file_name().ok_or(Refusal::NotRegular)?;
        let name = CString::new(name.as_bytes()).map_err(|_| Refusal::Nul)?;
        let dir = open_dir(parent)?;
        let opened_dir = dir.metadata().map_err(|_| Refusal::Directory)?;
        if !same_directory(&resolved, &opened_dir) {
            return Err(Refusal::Directory);
        }
        #[cfg(test)]
        if let Some(f) = self.between.as_mut() {
            f(&real);
        }
        let file = open_at(&dir, &name, FILE_FLAGS).map_err(|()| Refusal::Open)?;
        let opened = file.metadata().map_err(|_| Refusal::Swapped)?;
        if !same_regular_file(&before, &opened) {
            return Err(Refusal::Swapped);
        }
        let read = self.pread(&file, req, &opened, sink);
        #[cfg(test)]
        if let Some(f) = self.unlinking.as_mut() {
            f(&real);
        }
        if req.medium == KittyMedium::TempFile && self.deletable(&real) {
            use std::os::fd::AsRawFd;
            // SAFETY: `dir` is open and `name` NUL-terminated. The name is
            // deleted from the directory it was read from, wherever a
            // rename or a link has put that directory's path since.
            if unsafe { crate::sys::unlinkat(dir.as_raw_fd(), name.as_ptr(), 0) } != 0 {
                let e = std::io::Error::last_os_error();
                crate::warn!("kitty graphics: could not delete {}: {e}", real.display());
            }
        }
        read
    }

    /// Read what `req` asks of `file`, whose `fstat` is `meta`, in pieces.
    fn pread(
        &mut self,
        file: &File,
        req: &KittyFileRequest<'_>,
        meta: &Metadata,
        sink: &mut dyn FnMut(&[u8]) -> KittyFlow,
    ) -> Result<(), Refusal> {
        let n = req
            .read_len(req.available(meta.len()))
            .map_err(|_| Refusal::Size)?;
        self.buf.resize(PIECE.min(n), 0);
        let mut at = u64::from(req.offset);
        let mut left = n;
        while left > 0 {
            let k = left.min(PIECE);
            let piece = &mut self.buf[..k];
            file.read_exact_at(piece, at).map_err(|_| Refusal::Read)?;
            if sink(piece) == KittyFlow::Stop {
                break;
            }
            at += u64::try_from(k).map_err(|_| Refusal::Read)?;
            left -= k;
        }
        // A large read does not keep its buffer.
        self.buf = Vec::new();
        Ok(())
    }

    /// Whether a `t=t` file at `real`, resolved, may be deleted: in a
    /// known temporary directory, and named for the protocol.
    fn deletable(&self, real: &Path) -> bool {
        let in_temp = self
            .temp
            .iter()
            .any(|d| real.starts_with(d) && real != d.as_path());
        in_temp && contains(real.as_os_str().as_bytes(), MARKER)
    }

    /// A `t=s` read: the name checked, the object opened read-only, read,
    /// unlinked and closed.
    fn shm(
        &mut self,
        req: &KittyFileRequest<'_>,
        sink: &mut dyn FnMut(&[u8]) -> KittyFlow,
    ) -> Result<(), Refusal> {
        let name = req.path;
        let ok = name.first() == Some(&b'/')
            && !name[1..].contains(&b'/')
            && name.len() > 1
            && name.len() <= crate::sys::SHM_NAME_MAX;
        if !ok {
            return Err(Refusal::ShmName);
        }
        let c = CString::new(name).map_err(|_| Refusal::Nul)?;
        let fd = open_shm(&c);
        if fd < 0 {
            return Err(Refusal::ShmOpen);
        }
        // SAFETY: `fd` was just returned by `shm_open` and nothing else
        // holds it; the `File` closes it on drop, after the unlink.
        let file = unsafe { <File as std::os::fd::FromRawFd>::from_raw_fd(fd) };
        let read = match file.metadata() {
            Ok(m) => self.shm_read(&file, req, &m, sink),
            Err(_) => Err(Refusal::ShmOpen),
        };
        // SAFETY: `c` is a NUL-terminated name.
        unsafe { crate::sys::shm_unlink(c.as_ptr()) };
        drop(file);
        read
    }

    /// Glibc's shared memory is a file under `/dev/shm`, read as one, and
    /// that directory is writable by anyone: a FIFO or a device there is
    /// refused as `file` refuses one ("Only regular files are allowed").
    #[cfg(not(target_os = "macos"))]
    fn shm_read(
        &mut self,
        file: &File,
        req: &KittyFileRequest<'_>,
        meta: &Metadata,
        sink: &mut dyn FnMut(&[u8]) -> KittyFlow,
    ) -> Result<(), Refusal> {
        if !meta.file_type().is_file() {
            return Err(Refusal::NotRegular);
        }
        self.pread(file, req, meta, sink)
    }

    /// **Darwin's shared memory cannot be `read`, only mapped**, and its
    /// `fstat` size is rounded up to a page: `[0, O + n)` is mapped, the
    /// `n` bytes from `O` handed over, and the mapping released. An object
    /// on Darwin cannot be resized once made, so it cannot shrink under the
    /// mapping.
    #[cfg(target_os = "macos")]
    fn shm_read(
        &mut self,
        file: &File,
        req: &KittyFileRequest<'_>,
        meta: &Metadata,
        sink: &mut dyn FnMut(&[u8]) -> KittyFlow,
    ) -> Result<(), Refusal> {
        use std::os::fd::AsRawFd;
        let n = req
            .read_len(req.available(meta.len()))
            .map_err(|_| Refusal::Size)?;
        if n == 0 {
            return Ok(());
        }
        let from = usize::try_from(req.offset).map_err(|_| Refusal::Size)?;
        let span = from.checked_add(n).ok_or(Refusal::Size)?;
        // SAFETY: a fresh read-only shared mapping of `span` bytes, which
        // `read_len` holds within the object's size; released below.
        let at = unsafe {
            crate::sys::mmap(
                core::ptr::null_mut(),
                span,
                crate::sys::PROT_READ,
                crate::sys::MAP_SHARED,
                file.as_raw_fd(),
                0,
            )
        };
        if at == crate::sys::MAP_FAILED {
            return Err(Refusal::ShmMap);
        }
        // SAFETY: `at` maps `span` readable bytes until the `munmap`.
        let bytes = unsafe { core::slice::from_raw_parts(at.cast::<u8>(), span) };
        for piece in bytes[from..].chunks(PIECE) {
            if sink(piece) == KittyFlow::Stop {
                break;
            }
        }
        // SAFETY: the mapping made above, with its length.
        unsafe { crate::sys::munmap(at, crate::sys::MapLen(span)) };
        Ok(())
    }
}

impl KittyFiles for LocalFiles {
    fn read(
        &mut self,
        req: &KittyFileRequest<'_>,
        sink: &mut dyn FnMut(&[u8]) -> KittyFlow,
    ) -> Result<(), KittyFileFailed> {
        let r = match req.medium {
            KittyMedium::File | KittyMedium::TempFile => self.file(req, sink),
            KittyMedium::SharedMemory => self.shm(req, sink),
            KittyMedium::Direct => Err(Refusal::Medium),
        };
        r.map_err(|why| {
            crate::warn!(
                "kitty graphics: refused to read {:?} ({:?}): {why:?}",
                String::from_utf8_lossy(req.path),
                req.medium
            );
            KittyFileFailed
        })
    }
}

/// **The file iTerm2's `image` names, read with every check a `t=f` file
/// gets** (the module's list): an absolute path, resolved, outside
/// `/proc`, `/sys` and `/dev`, a regular file opened as the one checked,
/// read whole up to [`crate::notify::PICTURE_MAX`] and refused past it.
impl crate::notify::PictureReader for LocalFiles {
    fn picture(&mut self, path: &[u8]) -> Result<Vec<u8>, String> {
        let req = KittyFileRequest {
            medium: KittyMedium::File,
            path,
            offset: 0,
            size: 0,
            limit: crate::notify::PICTURE_MAX,
            past: KittyPastLimit::Refuse,
        };
        let mut out = Vec::new();
        self.file(&req, &mut |b| {
            out.extend_from_slice(b);
            KittyFlow::More
        })
        .map(|()| out)
        .map_err(|why| format!("{why:?}"))
    }
}

/// The temporary directories from this process's environment, unresolved.
fn temp_dirs() -> Vec<PathBuf> {
    let mut dirs = vec![PathBuf::from("/tmp"), PathBuf::from("/dev/shm")];
    if let Some(t) = std::env::var_os("TMPDIR") {
        dirs.push(PathBuf::from(t));
    }
    dirs.push(std::env::temp_dir());
    dirs
}

/// `dirs` resolved, sorted and each once, those that do not resolve left
/// out.
fn resolved(dirs: &[PathBuf]) -> Arc<[PathBuf]> {
    let mut temp: Vec<PathBuf> = dirs
        .iter()
        .filter_map(|d| std::fs::canonicalize(d).ok())
        .collect();
    temp.sort();
    temp.dedup();
    temp.into()
}

/// **The most file medium reads running at once in a process.** A window
/// has one at most while its read is owed, and a read given up past its
/// deadline keeps its place until its thread ends -- which, on a mount
/// that never answers, it never does. Past it a file medium is refused at
/// once, unread, with the one file error, and counted
/// (`FileReader::refused`), for as long as those reads run; a `t=t` file
/// refused there is never opened, so it is left where it is.
pub const READERS: usize = 4;

/// The pieces a read queues ahead of the terminal taking them.
const QUEUE: usize = 4;

/// The pieces one turn of the feeding thread hands the core: 4 MiB.
const STEP: usize = 64;

/// **What wakes the thread that feeds the terminal** when a read has a
/// piece or its end for it.
pub type Wake = Arc<dyn Fn() + Send + Sync>;

/// **The file medium reads running**, counted against [`READERS`], and
/// those refused at it.
pub struct Running {
    now: AtomicUsize,
    refused: AtomicUsize,
}

impl Running {
    pub const fn new() -> Running {
        Running {
            now: AtomicUsize::new(0),
            refused: AtomicUsize::new(0),
        }
    }
}

/// The process's own count, which every [`FileReader::new`] shares.
static PROCESS: Running = Running::new();

/// A read's place among the [`READERS`], given back when its thread ends.
struct Seat(&'static Running);

impl Seat {
    /// A place, if fewer than [`READERS`] reads run.
    fn take(running: &'static Running) -> Option<Seat> {
        let took = running
            .now
            .try_update(Ordering::AcqRel, Ordering::Acquire, |n| {
                (n < READERS).then_some(n + 1)
            });
        match took {
            Ok(_) => Some(Seat(running)),
            Err(_) => {
                running.refused.fetch_add(1, Ordering::AcqRel);
                None
            }
        }
    }
}

impl Drop for Seat {
    fn drop(&mut self) {
        self.0.now.fetch_sub(1, Ordering::AcqRel);
    }
}

/// How many file mediums were refused at [`READERS`].
#[cfg(test)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RefusedAtCap(usize);

/// **The reader that runs off the thread feeding the terminal**: each read
/// on a thread of its own, with every check [`LocalFiles`] makes, its
/// pieces through a channel of [`QUEUE`] pieces, and the deletion `t=t`
/// and the unlink `t=s` ask for made on that thread on every path -- read
/// whole, refused, or given up by dropping its [`Reading`], when the next
/// piece goes unsent and the read stops where it is.
pub struct FileReader {
    temp: Arc<[PathBuf]>,
    running: &'static Running,
    /// Run on the read's thread once the path is resolved, so a test can
    /// hold the read there as a stalled mount does.
    #[cfg(test)]
    stall: Option<Stall>,
}

/// What a test runs where a read stalls.
#[cfg(test)]
pub type Stall = Arc<dyn Fn(&Path) + Send + Sync>;

/// One piece of a read, or its end.
enum Piece {
    Bytes(Vec<u8>),
    End(Result<(), KittyFileFailed>),
}

/// **A file medium's read under way**: dropped, its thread stops at its
/// next piece, and a thread a stalled mount holds ends on its own.
pub struct Reading {
    rx: Option<Receiver<Piece>>,
    ended: Option<Result<(), KittyFileFailed>>,
}

/// What a turn found of a [`Reading`].
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Took {
    /// More is to come.
    Running,
    /// Read whole, or failed; or refused, its pieces past the refusal not
    /// taken.
    Ended(Result<(), KittyFileFailed>),
}

/// How much of what a read queued a turn takes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Drain {
    /// [`STEP`] pieces at most.
    Step,
    /// Everything queued.
    All,
}

impl FileReader {
    /// The reader for this process's temporary directories, counted in
    /// the process's [`READERS`].
    pub fn new() -> FileReader {
        FileReader {
            temp: resolved(&temp_dirs()),
            running: &PROCESS,
            #[cfg(test)]
            stall: None,
        }
    }

    /// A reader with `dirs` as the temporary directories, counted in
    /// `running`, a read held at `stall` once its path is resolved.
    #[cfg(test)]
    pub fn for_test(
        dirs: &[PathBuf],
        running: &'static Running,
        stall: Option<Stall>,
    ) -> FileReader {
        FileReader {
            temp: resolved(dirs),
            running,
            stall,
        }
    }

    /// The file mediums refused at [`READERS`].
    #[cfg(test)]
    pub fn refused(&self) -> RefusedAtCap {
        RefusedAtCap(self.running.refused.load(Ordering::Acquire))
    }

    /// **Start reading what `req` names** on a thread of its own, `wake`
    /// called at each piece and at the end. Past [`READERS`], or with no
    /// thread to be had, the read has already ended in the one file error,
    /// nothing opened.
    pub fn start(&self, req: &KittyFileRequest<'_>, wake: Option<Wake>) -> Reading {
        let Some(seat) = Seat::take(self.running) else {
            crate::warn!(
                "kitty graphics: refused to read {:?} ({:?}): {READERS} reads still running, {} refused so far",
                String::from_utf8_lossy(req.path),
                req.medium,
                self.running.refused.load(Ordering::Acquire)
            );
            return Reading::ended(Err(KittyFileFailed));
        };
        let (medium, offset, size, limit, past) =
            (req.medium, req.offset, req.size, req.limit, req.past);
        let path = req.path.to_vec();
        let temp = self.temp.clone();
        #[cfg(test)]
        let stall = self.stall.clone();
        let (tx, rx) = std::sync::mpsc::sync_channel(QUEUE);
        let spawned = std::thread::Builder::new()
            .name("kitty-file".into())
            .spawn(move || {
                let mut files = LocalFiles::of(temp);
                #[cfg(test)]
                if let Some(s) = stall {
                    files.canonical = Some(Box::new(move |p: &Path| s(p)));
                }
                let req = KittyFileRequest {
                    medium,
                    path: &path,
                    offset,
                    size,
                    limit,
                    past,
                };
                let woken = |w: &Option<Wake>| {
                    if let Some(w) = w {
                        w();
                    }
                };
                let mut sink = |b: &[u8]| {
                    let sent = tx.send(Piece::Bytes(b.to_vec()));
                    woken(&wake);
                    match sent {
                        Ok(()) => KittyFlow::More,
                        Err(_) => KittyFlow::Stop,
                    }
                };
                let read = files.read(&req, &mut sink);
                // The place is given back before the end is told, so a
                // read the end was taken from is no longer counted.
                drop(seat);
                let _ = tx.send(Piece::End(read));
                woken(&wake);
            });
        match spawned {
            Ok(_) => Reading {
                rx: Some(rx),
                ended: None,
            },
            Err(e) => {
                crate::warn!("kitty graphics: no thread to read a file medium on: {e}");
                Reading::ended(Err(KittyFileFailed))
            }
        }
    }
}

impl Reading {
    /// A read that ended before it began.
    fn ended(read: Result<(), KittyFileFailed>) -> Reading {
        Reading {
            rx: None,
            ended: Some(read),
        }
    }

    /// **The pieces queued, to `piece`, in order, never waiting**: as
    /// `drain` allows, and until `piece` refuses one, which ends the read
    /// and lets its thread stop.
    pub fn take(&mut self, drain: Drain, piece: &mut dyn FnMut(&[u8]) -> KittyFlow) -> Took {
        if let Some(read) = self.ended {
            return Took::Ended(read);
        }
        let mut left = match drain {
            Drain::Step => STEP,
            Drain::All => usize::MAX,
        };
        while left > 0 {
            let got = match self.rx.as_ref() {
                Some(rx) => rx.try_recv(),
                None => Err(TryRecvError::Disconnected),
            };
            match got {
                Ok(Piece::Bytes(b)) => {
                    left -= 1;
                    if piece(&b) == KittyFlow::Stop {
                        return self.end(Ok(()));
                    }
                }
                Ok(Piece::End(read)) => return self.end(read),
                Err(TryRecvError::Empty) => return Took::Running,
                Err(TryRecvError::Disconnected) => return self.end(Err(KittyFileFailed)),
            }
        }
        Took::Running
    }

    /// The read ended in `read`, its channel let go.
    fn end(&mut self, read: Result<(), KittyFileFailed>) -> Took {
        self.rx = None;
        self.ended = Some(read);
        Took::Ended(read)
    }
}

/// Open the shared-memory object `name` read-only.
fn open_shm(name: &CString) -> crate::sys::Fd {
    // SAFETY: `name` is NUL-terminated; a read-only open takes no mode.
    #[cfg(target_os = "macos")]
    let fd = unsafe { crate::sys::shm_open(name.as_ptr(), crate::sys::O_RDONLY) };
    // SAFETY: as above; glibc's `mode_t` is fixed, and ignored without
    // `O_CREAT`. Glibc's `shm_open` opens `/dev/shm/<name>` with the flags
    // passed, so `O_NONBLOCK` keeps a FIFO planted there from blocking the
    // open until a writer comes.
    #[cfg(not(target_os = "macos"))]
    let fd = unsafe {
        crate::sys::shm_open(
            name.as_ptr(),
            crate::sys::O_RDONLY | crate::sys::O_NONBLOCK,
            0,
        )
    };
    fd
}

/// **Whether `real`, resolved, lies in a sensitive part of the
/// filesystem**: under `/proc`, `/sys` or `/dev`, but for a file under
/// `/dev/shm`, which the text lists as a temporary directory (kitty's
/// path check draws the same line, `[[source:
/// kitty-utils-py]]`).
pub fn sensitive(real: &Path) -> bool {
    let mut parts = real.components().filter_map(|c| match c {
        Component::Normal(s) => Some(s.as_bytes()),
        _ => None,
    });
    match parts.next() {
        Some(b"proc") | Some(b"sys") => true,
        Some(b"dev") => !(parts.next() == Some(b"shm") && parts.next().is_some()),
        _ => false,
    }
}

/// Whether the descriptor's `fstat` is the regular file the path's check
/// saw: the same device and inode.
fn same_regular_file(before: &Metadata, opened: &Metadata) -> bool {
    opened.file_type().is_file() && opened.dev() == before.dev() && opened.ino() == before.ino()
}

/// Whether the descriptor's `fstat` is the directory the path resolved
/// to: the same device and inode.
fn same_directory(resolved: &Metadata, opened: &Metadata) -> bool {
    opened.is_dir() && opened.dev() == resolved.dev() && opened.ino() == resolved.ino()
}

/// How each directory on the way is opened: a directory, to search it,
/// following no link, closed on exec (`O_SEARCH` holds `O_DIRECTORY`
/// already, named for the reader).
#[cfg(target_os = "macos")]
const DIR_FLAGS: core::ffi::c_int =
    crate::sys::O_SEARCH | crate::sys::O_DIRECTORY | crate::sys::O_NOFOLLOW | crate::sys::O_CLOEXEC;
#[cfg(not(target_os = "macos"))]
const DIR_FLAGS: core::ffi::c_int =
    crate::sys::O_PATH | crate::sys::O_DIRECTORY | crate::sys::O_NOFOLLOW | crate::sys::O_CLOEXEC;

/// How the file is opened in its directory: read-only, following no link,
/// not blocking on a FIFO swapped in, closed on exec.
const FILE_FLAGS: core::ffi::c_int =
    crate::sys::O_RDONLY | crate::sys::O_NOFOLLOW | crate::sys::O_NONBLOCK | crate::sys::O_CLOEXEC;

/// `name`, opened in the directory `dir` holds open with `flags`.
fn open_at(dir: &File, name: &CString, flags: core::ffi::c_int) -> Result<File, ()> {
    use std::os::fd::AsRawFd;
    // SAFETY: `dir` is open, `name` NUL-terminated, and no flag creates,
    // so no mode is passed.
    let fd = unsafe { crate::sys::openat(dir.as_raw_fd(), name.as_ptr(), flags) };
    if fd < 0 {
        return Err(());
    }
    // SAFETY: `fd` was just returned by `openat` and nothing else holds it.
    Ok(unsafe { <File as std::os::fd::FromRawFd>::from_raw_fd(fd) })
}

/// **The directory `dir` names, opened a component at a time from `/`,
/// following no link**: each component is opened in the one before it,
/// held open, so a directory swapped for a link anywhere on the way is
/// refused, on Darwin and Linux alike, before the file below it is
/// opened. `dir` is resolved, so it has no `.`, `..` or link.
fn open_dir(dir: &Path) -> Result<File, Refusal> {
    let root = c"/";
    // SAFETY: an absolute path, NUL-terminated, opened with no mode.
    let fd = unsafe { crate::sys::openat(crate::sys::AT_FDCWD, root.as_ptr(), DIR_FLAGS) };
    if fd < 0 {
        return Err(Refusal::Directory);
    }
    // SAFETY: `fd` was just returned by `openat` and nothing else holds it.
    let mut at = unsafe { <File as std::os::fd::FromRawFd>::from_raw_fd(fd) };
    for c in dir.components() {
        let name = match c {
            Component::RootDir => continue,
            Component::Normal(s) => CString::new(s.as_bytes()).map_err(|_| Refusal::Nul)?,
            _ => return Err(Refusal::Directory),
        };
        at = open_at(&at, &name, DIR_FLAGS).map_err(|()| Refusal::Directory)?;
    }
    Ok(at)
}

/// Whether `hay` contains `needle`.
fn contains(hay: &[u8], needle: &[u8]) -> bool {
    hay.windows(needle.len()).any(|w| w == needle)
}

#[cfg(test)]
mod tests {
    use super::*;
    use baud_core::{BytePos, Owned, ReadLen, Term, WinSize};
    use baud_domain::{ImageBudget, ImagePx, KittyPastLimit};
    use std::os::fd::AsRawFd;
    use std::sync::atomic::{AtomicUsize, Ordering};

    const WS: WinSize = WinSize {
        ws_row: 24,
        ws_col: 80,
    };

    /// The single error, for image 1.
    const FILE_ERR: &[u8] = b"\x1b_Gi=1;EBADF:Failed to read image file\x1b\\";

    /// A non-interlaced 8-bit RGBA PNG, 33x17.
    const PNG: &[u8] = include_bytes!("../../baud-font/tests/png/rgba8-33x17-n.png");

    /// **A directory of the test's own** under the system's temporary
    /// directory, removed on drop: `tmp` stands for a temporary directory
    /// (the reader is given it, and only it), `tmpx` beside it shares its
    /// name's prefix, and `home` is anywhere else.
    struct Dir {
        base: PathBuf,
    }

    impl Dir {
        fn new() -> Dir {
            static N: AtomicUsize = AtomicUsize::new(0);
            let n = N.fetch_add(1, Ordering::Relaxed);
            let base = std::fs::canonicalize(std::env::temp_dir())
                .unwrap()
                .join(format!("baud-kf-{}-{n}", std::process::id()));
            for d in ["tmp", "tmpx", "home"] {
                std::fs::create_dir_all(base.join(d)).unwrap();
            }
            Dir { base }
        }

        fn at(&self, rel: &str) -> PathBuf {
            self.base.join(rel)
        }

        fn put(&self, rel: &str, data: &[u8]) -> PathBuf {
            let p = self.at(rel);
            std::fs::write(&p, data).unwrap();
            p
        }

        fn files(&self) -> LocalFiles {
            LocalFiles::with_temp(&[self.at("tmp")])
        }
    }

    impl Drop for Dir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.base);
        }
    }

    /// RFC 4648 base64, padded.
    fn b64(data: &[u8]) -> Vec<u8> {
        const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut out = Vec::new();
        for q in data.chunks(3) {
            let mut b = [0u8; 3];
            b[..q.len()].copy_from_slice(q);
            let n = u32::from_be_bytes([0, b[0], b[1], b[2]]);
            for k in 0..4 {
                match k <= q.len() {
                    true => out.push(A[usize::try_from((n >> (18 - 6 * k)) & 63).unwrap()]),
                    false => out.push(b'='),
                }
            }
        }
        out
    }

    fn apc(ctrl: &str, payload: &[u8]) -> Vec<u8> {
        let mut v = b"\x1b_G".to_vec();
        v.extend_from_slice(ctrl.as_bytes());
        v.push(b';');
        v.extend_from_slice(&b64(payload));
        v.extend_from_slice(b"\x1b\\");
        v
    }

    /// Feed `bytes` to a terminal reading through `files`, settle any
    /// decode, and answer the replies and the pixels of the one image
    /// placed, if any.
    fn run(files: Option<LocalFiles>, bytes: &[u8]) -> (Vec<u8>, Option<Vec<u32>>) {
        let mut owned = Owned::new(&WS).with_images(ImageBudget::of(4096));
        let mut t = Term::new(owned.storage(), &WS);
        t.set_cell_size(ImagePx::of(10), ImagePx::of(20));
        if let Some(f) = files {
            t.set_kitty_files(Box::leak(Box::new(f)));
        }
        t.write_at(BytePos::ZERO, bytes);
        t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
        while t.decode_step(baud_domain::DecodeBudget::ALL) == baud_domain::DecodeProgress::More {}
        let r = t.reply().to_vec();
        let imgs = t.images();
        let px = imgs.placements().next().map(|(_, p)| {
            let v = imgs.image(p.image).expect("decoded");
            v.pixels.iter().map(|p| p.bits()).collect()
        });
        (r, px)
    }

    /// One request straight to the reader: what it handed over, or why
    /// it refused.
    fn ask(f: &mut LocalFiles, req: &KittyFileRequest<'_>) -> Result<Vec<u8>, Refusal> {
        let mut got = Vec::new();
        let r = match req.medium {
            KittyMedium::SharedMemory => f.shm(req, &mut |c| {
                got.extend_from_slice(c);
                KittyFlow::More
            }),
            _ => f.file(req, &mut |c| {
                got.extend_from_slice(c);
                KittyFlow::More
            }),
        };
        r.map(|()| got)
    }

    /// A request for the whole of `path`, up to `limit`.
    fn whole(medium: KittyMedium, path: &[u8]) -> KittyFileRequest<'_> {
        KittyFileRequest {
            medium,
            path,
            offset: 0,
            size: 0,
            limit: 1 << 20,
            past: KittyPastLimit::Refuse,
        }
    }

    fn bytes_of(p: &Path) -> &[u8] {
        p.as_os_str().as_bytes()
    }

    /// **A real PNG file decodes and places as its payload would**, and a
    /// raw one is read from `O` for `S` bytes.
    #[test]
    fn a_real_file_decodes_and_places_as_its_payload_would() {
        let d = Dir::new();
        let png = d.put("home/a.png", PNG);
        let direct = run(None, &apc("a=T,i=1,f=100", PNG));
        assert_eq!(direct.0, b"\x1b_Gi=1;OK\x1b\\");
        assert!(direct.1.is_some(), "the oracle placed it");
        let got = run(Some(d.files()), &apc("a=T,i=1,t=f,f=100", bytes_of(&png)));
        assert_eq!(got, direct, "the file decodes as its payload does");
        assert!(png.exists(), "t=f deletes nothing");
        let rgba: Vec<u8> = (0..3 * 2 * 4)
            .map(|i| u8::try_from(i * 9).unwrap())
            .collect();
        let mut padded = vec![0xEE; 10];
        padded.extend_from_slice(&rgba);
        padded.extend_from_slice(&[0xDD; 5]);
        let raw = d.put("home/a.raw", &padded);
        let direct = run(None, &apc("a=T,i=1,f=32,s=3,v=2", &rgba));
        let got = run(
            Some(d.files()),
            &apc("a=T,i=1,t=f,f=32,s=3,v=2,O=10,S=24", bytes_of(&raw)),
        );
        assert_eq!(got, direct, "O and S choose the bytes");
    }

    /// **Every refusal is one identical reply, and the log says why**: a
    /// missing file, a relative path, a NUL, a symlink loop, a directory,
    /// a FIFO, a socket, a device under `/dev`, and an object smaller than
    /// `S` claims.
    #[test]
    fn every_refusal_is_the_one_reply_and_the_log_says_why() {
        let d = Dir::new();
        let fifo = d.at("home/fifo");
        let made = std::process::Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .unwrap();
        assert!(made.success(), "mkfifo");
        let sock = d.at("home/sock");
        let _listener = std::os::unix::net::UnixListener::bind(&sock).unwrap();
        let a = d.at("home/loop-a");
        let b = d.at("home/loop-b");
        std::os::unix::fs::symlink(&b, &a).unwrap();
        std::os::unix::fs::symlink(&a, &b).unwrap();
        let short = d.put("home/short", &[1, 2, 3]);
        let dir = d.at("home");
        let missing = d.at("home/missing");
        let cases: [(&[u8], Result<(), Refusal>); 9] = [
            (bytes_of(&missing), Err(Refusal::Resolve)),
            (b"relative/a.png", Err(Refusal::NotAbsolute)),
            (b"/tmp/a\0b", Err(Refusal::Nul)),
            (bytes_of(&a), Err(Refusal::Resolve)),
            (bytes_of(&dir), Err(Refusal::NotRegular)),
            (bytes_of(&fifo), Err(Refusal::NotRegular)),
            (bytes_of(&sock), Err(Refusal::NotRegular)),
            (b"/dev/null", Err(Refusal::Sensitive)),
            // `S` past the object reads the three bytes there, as kitty
            // reads it; the image they leave short is the one reply below.
            (bytes_of(&short), Ok(())),
        ];
        let mut f = d.files();
        for (path, why) in cases {
            let name = String::from_utf8_lossy(path).into_owned();
            let req = KittyFileRequest {
                medium: KittyMedium::File,
                path,
                offset: 0,
                size: 24,
                limit: 24,
                past: KittyPastLimit::Stop,
            };
            let got = f.file(&req, &mut |_| KittyFlow::More);
            assert_eq!(got, why, "{name}");
            let (r, px) = run(Some(d.files()), &apc("a=T,i=1,t=f,f=32,s=3,v=2,S=24", path));
            assert_eq!(r, FILE_ERR, "{name}: {:?}", String::from_utf8_lossy(&r));
            assert_eq!(px, None, "{name}");
        }
        assert!(short.exists() && fifo.exists(), "nothing was deleted");
    }

    /// **The sensitive parts of the filesystem**, on the resolved path:
    /// `/proc`, `/sys` and `/dev` refused whole, a file under `/dev/shm`
    /// allowed, and nothing matched by a name's prefix.
    #[test]
    fn proc_sys_and_dev_are_refused_by_component() {
        for (p, refused) in [
            ("/proc/self/status", true),
            ("/proc", true),
            ("/sys/kernel/notes", true),
            ("/dev/null", true),
            ("/dev/zero", true),
            ("/dev/shm", true),
            ("/dev/shm/tty-graphics-protocol-a", false),
            ("/procfs/a", false),
            ("/devices/a", false),
            ("/tmp/proc/a", false),
            ("/home/dev/a", false),
        ] {
            assert_eq!(sensitive(Path::new(p)), refused, "{p}");
        }
    }

    /// **A symlink is followed, and a swap under the check is refused**: a
    /// link to a PNG decodes; the checked file replaced, between the check
    /// and the open, by a link to `/etc/passwd`, by another regular file,
    /// or by a FIFO, is refused each by its own mechanism, and the FIFO's
    /// open does not block.
    #[test]
    fn a_symlink_is_followed_and_a_swap_under_the_check_is_refused() {
        let d = Dir::new();
        let png = d.put("home/real.png", PNG);
        let link = d.at("home/link.png");
        std::os::unix::fs::symlink(&png, &link).unwrap();
        let mut f = d.files();
        let direct = run(None, &apc("a=T,i=1,f=100", PNG));
        let got = run(Some(d.files()), &apc("a=T,i=1,t=f,f=100", bytes_of(&link)));
        assert_eq!(got, direct, "the link is followed");
        let victim = d.put("home/victim.png", PNG);
        let other = d.put("home/other.png", PNG);
        let swaps: [(&str, Refusal); 3] = [
            ("passwd", Refusal::Open),
            ("rename", Refusal::Swapped),
            ("fifo", Refusal::Swapped),
        ];
        for (how, why) in swaps {
            std::fs::write(&victim, PNG).unwrap();
            std::fs::write(&other, PNG).unwrap();
            let (v, o) = (victim.clone(), other.clone());
            f.between = Some(Box::new(move |real: &Path| {
                assert_eq!(real, v.as_path());
                std::fs::remove_file(real).unwrap();
                match how {
                    "passwd" => std::os::unix::fs::symlink("/etc/passwd", real).unwrap(),
                    "rename" => std::fs::rename(&o, real).unwrap(),
                    _ => {
                        let s = std::process::Command::new("mkfifo").arg(real).status();
                        assert!(s.unwrap().success());
                    }
                }
            }));
            let got = ask(&mut f, &whole(KittyMedium::File, bytes_of(&victim)));
            assert_eq!(got, Err(why), "{how}");
            f.between = None;
            let _ = std::fs::remove_file(&victim);
        }
    }

    /// **A directory above the file swapped for a link after the path is
    /// resolved is refused before the file is opened**, on Darwin and on
    /// Linux alike: the directories are opened a component at a time,
    /// following no link, before the file is. The link points at a
    /// directory holding a regular file of the same name, standing for a
    /// sensitive one: every other check passes on it.
    #[test]
    fn a_directory_swapped_for_a_link_after_resolving_is_refused() {
        let d = Dir::new();
        std::fs::create_dir_all(d.at("home/dir")).unwrap();
        std::fs::create_dir_all(d.at("home/elsewhere")).unwrap();
        let real = d.put("home/dir/x.png", PNG);
        d.put("home/elsewhere/x.png", PNG);
        let mut f = d.files();
        let (dir, old, there) = (
            d.at("home/dir"),
            d.at("home/dir-old"),
            d.at("home/elsewhere"),
        );
        f.resolved = Some(Box::new(move |_: &Path| {
            std::fs::rename(&dir, &old).unwrap();
            std::os::unix::fs::symlink(&there, &dir).unwrap();
        }));
        let opened = std::rc::Rc::new(std::cell::Cell::new(false));
        let seen = opened.clone();
        f.between = Some(Box::new(move |_: &Path| seen.set(true)));
        let got = ask(&mut f, &whole(KittyMedium::File, bytes_of(&real)));
        assert_eq!(got, Err(Refusal::Directory));
        assert!(!opened.get(), "refused before the file's open");
    }

    /// **A directory further up swapped for a link before anything is
    /// checked is refused**: the directory's device and inode are taken
    /// through the link, and agree with a walk that followed it, so only
    /// the walk's refusal to follow one stands between the path checked
    /// and a file of the same name in the tree the link points into.
    #[test]
    fn an_ancestor_swapped_for_a_link_before_the_checks_is_refused() {
        let d = Dir::new();
        std::fs::create_dir_all(d.at("home/a/dir")).unwrap();
        std::fs::create_dir_all(d.at("home/b/dir")).unwrap();
        let real = d.put("home/a/dir/x.png", PNG);
        d.put("home/b/dir/x.png", PNG);
        let mut f = d.files();
        let (a, old, b) = (d.at("home/a"), d.at("home/a-old"), d.at("home/b"));
        f.canonical = Some(Box::new(move |_: &Path| {
            std::fs::rename(&a, &old).unwrap();
            std::os::unix::fs::symlink(&b, &a).unwrap();
        }));
        let got = ask(&mut f, &whole(KittyMedium::File, bytes_of(&real)));
        assert_eq!(got.map(|v| v.len()), Err(Refusal::Directory));
    }

    /// **The directory opened is the one resolved**: the file's directory
    /// renamed away after resolving and another put in its place, holding
    /// a hard link to the same file, so the path has no link in it and the
    /// file is the one checked -- only the directory's device and inode
    /// tell them apart.
    #[test]
    fn a_directory_renamed_in_after_resolving_is_refused() {
        let d = Dir::new();
        std::fs::create_dir_all(d.at("home/dir")).unwrap();
        std::fs::create_dir_all(d.at("home/twin")).unwrap();
        let real = d.put("home/dir/x.png", PNG);
        std::fs::hard_link(&real, d.at("home/twin/x.png")).unwrap();
        let mut f = d.files();
        let (dir, old, twin) = (d.at("home/dir"), d.at("home/dir-old"), d.at("home/twin"));
        f.resolved = Some(Box::new(move |_: &Path| {
            std::fs::rename(&dir, &old).unwrap();
            std::fs::rename(&twin, &dir).unwrap();
        }));
        let got = ask(&mut f, &whole(KittyMedium::File, bytes_of(&real)));
        assert_eq!(got.map(|v| v.len()), Err(Refusal::Directory));
    }

    /// **Every descriptor the walk and the file's open make closes on
    /// exec**: they are `openat`'s, not `std`'s, and the shell baud forks
    /// would otherwise inherit them.
    #[test]
    fn the_directories_and_the_file_are_opened_close_on_exec() {
        use std::os::fd::AsRawFd;
        let d = Dir::new();
        let p = d.put("home/c.png", PNG);
        let real = std::fs::canonicalize(&p).unwrap();
        let dir = open_dir(real.parent().unwrap()).expect("the walk");
        let name = CString::new(real.file_name().unwrap().as_bytes()).unwrap();
        let file = open_at(&dir, &name, FILE_FLAGS).expect("the open");
        for (what, fd) in [("dir", dir.as_raw_fd()), ("file", file.as_raw_fd())] {
            // SAFETY: an open descriptor; `F_GETFD` takes no argument.
            let flags = unsafe { crate::sys::fcntl(fd, crate::sys::F_GETFD) };
            assert!(flags >= 0, "{what}");
            assert_eq!(
                flags & crate::sys::FD_CLOEXEC,
                crate::sys::FD_CLOEXEC,
                "{what}"
            );
        }
    }

    /// **A `t=t` file is deleted from the directory it was read from**: a
    /// directory above it swapped, after the read, for a link to another
    /// holding a file of the same name, does not move the deletion there.
    #[test]
    fn a_directory_swapped_before_the_unlink_does_not_move_it() {
        let d = Dir::new();
        let rgba = [7u8; 3 * 2 * 4];
        std::fs::create_dir_all(d.at("tmp/sub")).unwrap();
        std::fs::create_dir_all(d.at("home/sub")).unwrap();
        let p = d.put("tmp/sub/tty-graphics-protocol-u", &rgba);
        let victim = d.put("home/sub/tty-graphics-protocol-u", &rgba);
        let mut f = d.files();
        let (sub, old, there) = (d.at("tmp/sub"), d.at("tmp/sub-old"), d.at("home/sub"));
        f.unlinking = Some(Box::new(move |_: &Path| {
            std::fs::rename(&sub, &old).unwrap();
            std::os::unix::fs::symlink(&there, &sub).unwrap();
        }));
        let req = KittyFileRequest {
            medium: KittyMedium::TempFile,
            ..whole(KittyMedium::TempFile, bytes_of(&p))
        };
        assert_eq!(ask(&mut f, &req), Ok(rgba.to_vec()));
        assert!(victim.exists(), "the file behind the link is not deleted");
        assert!(
            !d.at("tmp/sub-old/tty-graphics-protocol-u").exists(),
            "the file read is deleted"
        );
    }

    /// **`t=t` deletes only inside a temporary directory and with the
    /// marker**: every file is read, and only the one that has both goes,
    /// even when the load then fails; a directory that shares the
    /// temporary one's prefix is not inside it.
    #[test]
    fn a_temporary_file_is_deleted_only_in_temp_with_the_marker() {
        let d = Dir::new();
        let rgba = [7u8; 3 * 2 * 4];
        let cases = [
            ("tmp/tty-graphics-protocol-a", true),
            ("tmp/plain-a", false),
            ("home/tty-graphics-protocol-a", false),
            ("tmpx/tty-graphics-protocol-a", false),
        ];
        for (rel, goes) in cases {
            let p = d.put(rel, &rgba);
            let (r, px) = run(
                Some(d.files()),
                &apc("a=T,i=1,t=t,f=32,s=3,v=2", bytes_of(&p)),
            );
            assert_eq!(r, b"\x1b_Gi=1;OK\x1b\\", "{rel}: read");
            assert!(px.is_some(), "{rel}");
            assert_eq!(!p.exists(), goes, "{rel}: deleted");
        }
        // Opened and then short of what was claimed: the one error, and
        // the file still goes.
        let p = d.put("tmp/tty-graphics-protocol-short", &rgba[..5]);
        let (r, _) = run(
            Some(d.files()),
            &apc("a=T,i=1,t=t,f=32,s=3,v=2", bytes_of(&p)),
        );
        assert_eq!(r, FILE_ERR);
        assert!(!p.exists(), "deleted though the load failed");
    }

    /// A shared-memory object of the test's own, holding `data`.
    fn make_shm(name: &str, data: &[u8]) {
        let c = CString::new(name).unwrap();
        // SAFETY: a NUL-terminated name; the mode is `0600`.
        #[cfg(target_os = "macos")]
        let fd = unsafe {
            crate::sys::shm_open(
                c.as_ptr(),
                crate::sys::O_CREAT | crate::sys::O_RDWR,
                0o600 as core::ffi::c_uint,
            )
        };
        // SAFETY: as above.
        #[cfg(not(target_os = "macos"))]
        let fd = unsafe {
            crate::sys::shm_open(c.as_ptr(), crate::sys::O_CREAT | crate::sys::O_RDWR, 0o600)
        };
        assert!(fd >= 0, "shm_open {name}");
        // SAFETY: `fd` is ours alone.
        let file = unsafe { <File as std::os::fd::FromRawFd>::from_raw_fd(fd) };
        file.set_len(u64::try_from(data.len()).unwrap()).unwrap();
        use std::os::fd::AsRawFd;
        // SAFETY: a fresh writable shared mapping of the object's length.
        let at = unsafe {
            crate::sys::mmap(
                core::ptr::null_mut(),
                data.len(),
                crate::sys::PROT_READ | crate::sys::PROT_WRITE,
                crate::sys::MAP_SHARED,
                file.as_raw_fd(),
                0,
            )
        };
        assert!(at != crate::sys::MAP_FAILED, "mmap");
        // SAFETY: `at` maps `data.len()` writable bytes.
        unsafe {
            core::ptr::copy_nonoverlapping(data.as_ptr(), at.cast::<u8>(), data.len());
            crate::sys::munmap(at, crate::sys::MapLen(data.len()));
        }
    }

    /// Whether a shared-memory object `name` exists.
    fn shm_exists(name: &str) -> bool {
        let c = CString::new(name).unwrap();
        let fd = open_shm(&c);
        if fd >= 0 {
            // SAFETY: ours to close.
            drop(unsafe { <File as std::os::fd::FromRawFd>::from_raw_fd(fd) });
        }
        fd >= 0
    }

    /// **`t=s` reads a POSIX shared-memory object from `O` for `S` bytes
    /// and unlinks it**; a name the text forbids is refused unopened.
    #[test]
    fn shared_memory_round_trips_and_is_unlinked() {
        static N: AtomicUsize = AtomicUsize::new(0);
        let name = format!(
            "/bkf{}-{}",
            std::process::id() % 100_000,
            N.fetch_add(1, Ordering::Relaxed)
        );
        let rgba: Vec<u8> = (0..3 * 2 * 4)
            .map(|i| u8::try_from(i * 5).unwrap())
            .collect();
        let mut data = vec![0xAB; 6];
        data.extend_from_slice(&rgba);
        make_shm(&name, &data);
        assert!(shm_exists(&name), "the control: the object is there");
        let d = Dir::new();
        let mut f = d.files();
        let direct = run(None, &apc("a=T,i=1,f=32,s=3,v=2", &rgba));
        let got = run(
            Some(d.files()),
            &apc("a=T,i=1,t=s,f=32,s=3,v=2,O=6,S=24", name.as_bytes()),
        );
        assert_eq!(got, direct, "read from O for S");
        assert!(!shm_exists(&name), "unlinked once read");
        let long = format!("/{}", "a".repeat(crate::sys::SHM_NAME_MAX));
        for (bad, why) in [
            ("noslash", Refusal::ShmName),
            ("/a/b", Refusal::ShmName),
            ("/", Refusal::ShmName),
            (long.as_str(), Refusal::ShmName),
            ("/bkf-missing", Refusal::ShmOpen),
        ] {
            assert_eq!(
                ask(&mut f, &whole(KittyMedium::SharedMemory, bad.as_bytes())),
                Err(why),
                "{bad}"
            );
        }
    }

    /// **mpv's `t=s` command, as each mpv writes it**: mpv names the object
    /// it creates `/mpv-kitty-%p`, and 0.41.0 sent the name without its
    /// slash, which the text forbids ("On POSIX SHM names must start with a
    /// /", `[[source: kitty-graphics-rst]]`) and Darwin's `shm_open` does
    /// not take: refused unopened, nothing placed, the object left for mpv
    /// to unlink. The same command with the slash, as mpv writes it since
    /// its upstream fix, is read, placed and unlinked. mpv's keys exactly:
    /// `a=T,t=s,f=24,s=,v=,C=1,q=2,m=1`, no id and `q=2`, so no reply
    /// either way, as kitty sends none; the same refusal for a command with
    /// an id is answered `EBADF:Failed to read image file`, kitty's answer.
    #[test]
    fn mpvs_shared_memory_name_needs_its_slash() {
        static N: AtomicUsize = AtomicUsize::new(0);
        let name = format!(
            "/mpv-kitty-0x{:x}",
            u64::from(std::process::id()) << 8
                | u64::try_from(N.fetch_add(1, Ordering::Relaxed)).unwrap()
        );
        let rgb: Vec<u8> = (0..4 * 2 * 3)
            .map(|i| u8::try_from(i * 7).unwrap())
            .collect();
        let keys = "a=T,t=s,f=24,s=4,v=2,C=1,q=2,m=1";
        let d = Dir::new();
        make_shm(&name, &rgb);
        let stripped = run(Some(d.files()), &apc(keys, &name.as_bytes()[1..]));
        assert_eq!(
            stripped,
            (Vec::new(), None),
            "0.41.0's name: refused, silent"
        );
        assert!(shm_exists(&name), "not opened, so not unlinked");
        // A command that names an id and does not silence errors is told,
        // in kitty's one file error, as kitty answers a name its
        // `shm_open` cannot open (`IMAGE_FILE_ERROR_CODE`,
        // `[[source: kitty-graphics-c]]`).
        let told = run(
            Some(d.files()),
            &apc("a=T,t=s,f=24,s=4,v=2,i=1", &name.as_bytes()[1..]),
        );
        assert_eq!(
            told,
            (FILE_ERR.to_vec(), None),
            "an id's refusal is answered"
        );
        assert!(shm_exists(&name), "and not opened");
        let whole = run(Some(d.files()), &apc(keys, name.as_bytes()));
        let direct = run(None, &apc("a=T,f=24,s=4,v=2,C=1,q=2", &rgb));
        assert!(whole.1.is_some(), "the slash's name: placed");
        assert_eq!(whole, direct, "read as the direct payload would be");
        assert!(!shm_exists(&name), "unlinked once read");
    }

    /// **A FIFO planted under `/dev/shm` is refused, not waited on**:
    /// glibc's `shm_open` opens it as a file, so without `O_NONBLOCK` the
    /// open blocks until a writer comes, and without the regular-file check
    /// the read does.
    #[cfg(not(target_os = "macos"))]
    #[test]
    fn a_fifo_under_dev_shm_is_refused_at_once() {
        let name = format!("bkf-fifo-{}", std::process::id());
        let path = format!("/dev/shm/{name}");
        let made = std::process::Command::new("mkfifo")
            .arg(&path)
            .status()
            .unwrap();
        assert!(made.success(), "mkfifo");
        let d = Dir::new();
        let mut f = d.files();
        let got = ask(
            &mut f,
            &whole(KittyMedium::SharedMemory, format!("/{name}").as_bytes()),
        );
        let _ = std::fs::remove_file(&path);
        assert_eq!(got, Err(Refusal::NotRegular));
    }

    /// **An oversized file is refused before a byte is read**, and raw
    /// data is read to `s * v` and no further.
    #[test]
    fn an_oversized_file_is_refused_before_reading() {
        let d = Dir::new();
        let big = d.put("home/big", &[9u8; 1000]);
        let mut f = d.files();
        let mut handed = 0usize;
        let req = KittyFileRequest {
            medium: KittyMedium::File,
            path: bytes_of(&big),
            offset: 0,
            size: 0,
            limit: 999,
            past: KittyPastLimit::Refuse,
        };
        let got = f.file(&req, &mut |c| {
            handed += c.len();
            KittyFlow::More
        });
        assert_eq!((got, handed), (Err(Refusal::Size), 0));
        let req = KittyFileRequest {
            past: KittyPastLimit::Stop,
            ..req
        };
        let got = f.file(&req, &mut |c| {
            handed += c.len();
            KittyFlow::More
        });
        assert_eq!((got, handed), (Ok(()), 999), "read to the limit");
        // Through the core: a PNG past the scratch is the one error.
        let mut owned = Owned::new(&WS).with_images(ImageBudget::of(64));
        let mut t = Term::new(owned.storage(), &WS);
        t.set_kitty_files(Box::leak(Box::new(d.files())));
        let s = apc("a=T,i=1,t=f,f=100", bytes_of(&big));
        t.write_at(BytePos::ZERO, &s);
        t.feed(BytePos::ZERO, ReadLen::from_index(s.len()));
        assert_eq!(t.reply(), FILE_ERR);
    }

    /// **`kitten icat` through the reader, live**: `--transfer-mode=file`,
    /// `memory` and `detect` each run against a terminal reading through
    /// [`LocalFiles`], its replies written back, and the picture is placed,
    /// by `t=f` and `t=s` where those were asked for. Under `detect`
    /// kitten's `t=t` query carries `S` as its path's length, a bound, as
    /// kitty reads it, so the probe answers `OK` and kitten sends the PNG
    /// it holds as a file by `t=f` (`[[source: kitty-icat-transmit]]`
    /// `transmit_image`: shared memory only for bytes it made itself).
    /// kitten runs on a pty of 24x80 cells of 10x20 pixels, and this
    /// test holds the manager. Not in the suite: it needs kitten, named by
    /// `BAUD_KITTEN`. Prints the control data kitten sent.
    #[test]
    #[ignore = "needs kitten: BAUD_KITTEN=<path to kitten>"]
    fn kitten_icat_reads_through_the_reader_live() {
        let kitten = std::env::var("BAUD_KITTEN").expect("BAUD_KITTEN");
        let d = Dir::new();
        let image = d.put("home/icat.png", PNG);
        for mode in ["file", "memory", "detect"] {
            let size = baud_domain::WinSize {
                ws_col: 80,
                ws_row: 24,
            };
            let ws = crate::pty::text_area(&size, baud_font::Px::of(10), baud_font::Px::of(20));
            let env: Vec<std::ffi::CString> = std::env::vars()
                .filter(|(k, _)| k != "TERM")
                .chain([("TERM".to_string(), "xterm-256color".to_string())])
                .filter_map(|(k, v)| std::ffi::CString::new(format!("{k}={v}")).ok())
                .collect();
            let cmd = format!(
                "exec '{kitten}' icat --transfer-mode={mode} '{}'",
                image.display()
            );
            let (pid, master) = crate::pty::Pty::fork_child(ws, Some(cmd), env).expect("forkpty");
            // The reader owns the master and closes it when the child ends;
            // the replies borrow its number until then.
            let raw = master.as_raw_fd();
            let (tx, rx) = std::sync::mpsc::channel::<Vec<u8>>();
            std::thread::spawn(move || {
                let mut b = vec![0u8; 65536];
                loop {
                    // SAFETY: `b` is a live, writable buffer of the length
                    // passed. A read of a child's terminal that has closed
                    // answers 0 or `EIO`, and both end this.
                    let n = unsafe {
                        crate::sys::read(master.as_raw_fd(), b.as_mut_ptr().cast(), b.len())
                    };
                    let n = usize::try_from(n).unwrap_or(0);
                    if n == 0 || tx.send(b[..n].to_vec()).is_err() {
                        break;
                    }
                }
            });
            let mut owned = Owned::new(&WS).with_images(ImageBudget::of(1 << 20));
            let mut t = Term::new(owned.storage(), &WS);
            t.set_cell_size(ImagePx::of(10), ImagePx::of(20));
            t.set_kitty_files(Box::leak(Box::new(LocalFiles::new())));
            // kitten's detection waited ten seconds on `CSI ? 6 c`, baud's
            // default until 2026-11-08, and goes on at once on
            // `CSI ? 6 ; 4 c`, the answer under the Sixel claim.
            t.set_sixel_claim(baud_domain::SixelClaim::Yes);
            let mut seen = Vec::new();
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
            while let Ok(chunk) = rx.recv_timeout(deadline - std::time::Instant::now()) {
                seen.extend_from_slice(&chunk);
                t.write_at(BytePos::ZERO, &chunk);
                t.feed(BytePos::ZERO, ReadLen::from_index(chunk.len()));
                while t.decode_step(baud_domain::DecodeBudget::ALL)
                    == baud_domain::DecodeProgress::More
                {}
                if !t.reply().is_empty() {
                    // SAFETY: the reply is a live buffer of the length passed.
                    let _ = unsafe {
                        crate::sys::write(raw, t.reply().as_ptr().cast(), t.reply().len())
                    };
                    t.clear_reply();
                }
            }
            // SAFETY: `pid` is the child forked above, and no other wait
            // takes it.
            unsafe { crate::sys::waitpid(pid, std::ptr::null_mut(), 0) };
            let text = String::from_utf8_lossy(&seen);
            let keys: Vec<&str> = text
                .split("\x1b_G")
                .skip(1)
                .filter_map(|c| c.split([';', '\x1b']).next())
                .collect();
            println!("{mode}: kitten sent {keys:?}");
            println!("{mode}: {:?}", t.kitty());
            assert_eq!(t.images().placements().count(), 1, "{mode}: placed");
            let (_, p) = t.images().placements().next().unwrap();
            assert!(t.images().image(p.image).is_some(), "{mode}: decoded");
            // The picture's own command: `t=f` and `t=s` as asked; under
            // `detect`, the file, its `t=t` probe answered.
            let sent = keys.iter().rev().find(|k| k.contains("a=T")).unwrap();
            match mode {
                "memory" => assert!(sent.contains("t=s"), "{sent}"),
                _ => assert!(sent.contains("t=f"), "{mode}: {sent}"),
            }
        }
    }

    /// **kitten's detection, answered from the filesystem**: a query per
    /// medium before the Primary DA, `OK` where the medium works and the
    /// one error where it does not -- a path only a remote machine has --
    /// and the temporary file deleted.
    #[test]
    fn a_query_per_medium_answers_before_the_da() {
        let d = Dir::new();
        let px = [1u8, 2, 3];
        let file = d.put("home/q.rgb", &px);
        let temp = d.put("tmp/tty-graphics-protocol-q", &px);
        let mut s = apc("i=31,s=1,v=1,a=q,t=f,f=24", bytes_of(&file));
        s.extend(apc("i=32,s=1,v=1,a=q,t=t,f=24", bytes_of(&temp)));
        s.extend(apc("i=33,s=1,v=1,a=q,t=f,f=24", b"/remote/only/q.rgb"));
        s.extend_from_slice(b"\x1b[c");
        let (r, px) = run(Some(d.files()), &s);
        let r = String::from_utf8_lossy(&r).into_owned();
        let expect = "\x1b_Gi=31;OK\x1b\\\x1b_Gi=32;OK\x1b\\\
            \x1b_Gi=33;EBADF:Failed to read image file\x1b\\";
        assert!(r.starts_with(expect), "{r:?}");
        assert!(
            r[expect.len()..].starts_with("\x1b[?"),
            "then the DA: {r:?}"
        );
        assert_eq!(px, None, "a query places nothing");
        assert!(!temp.exists(), "the query's temporary file is deleted");
        assert!(file.exists());
    }

    /// Feed `bytes` in the reads `cuts` ends, the last running to the
    /// end, and answer every reply in order.
    fn replies_in_reads(files: LocalFiles, bytes: &[u8], cuts: &[usize]) -> Vec<u8> {
        let mut owned = Owned::new(&WS).with_images(ImageBudget::of(4096));
        let mut t = Term::new(owned.storage(), &WS);
        t.set_kitty_files(Box::leak(Box::new(files)));
        let mut out = Vec::new();
        let mut from = 0;
        for &to in cuts.iter().chain([bytes.len()].iter()) {
            let read = &bytes[from..to];
            from = to;
            t.write_at(BytePos::ZERO, read);
            t.feed(BytePos::ZERO, ReadLen::from_index(read.len()));
            while t.decode_step(baud_domain::DecodeBudget::ALL) == baud_domain::DecodeProgress::More
            {
            }
            out.extend_from_slice(t.reply());
            t.clear_reply();
        }
        out
    }

    /// **kitten's detection burst, byte for byte, gets one reply a
    /// query** (`[[source: kitty-icat-detect]]`): `t=d`, then a `t=t`
    /// temporary file holding the three bytes of a 1x1 RGB pixel with `S`
    /// set to the length of its path, then `t=s`, unpadded base64 as
    /// kitten writes it, then `CSI c`. With no file there the `t=t` answer
    /// is the one file error. With the file there, `S` past it is a bound,
    /// as kitty reads it (`[[source: kitty-graphics-c]]` `read_img_file`),
    /// so the probe answers `OK` and the file is deleted, and the DA comes
    /// last, in a whole read, in reads of every size and split at every
    /// byte. The same file claimed at its own size answers the same.
    #[test]
    fn kittens_detection_burst_gets_one_reply_a_query() {
        let d = Dir::new();
        let unpadded = |p: &[u8]| {
            let mut v = b64(p);
            v.retain(|&b| b != b'=');
            v
        };
        let burst = |path: &[u8], claim: usize| {
            let mut s = b"\x1b_Ga=q,f=24,s=1,v=1,S=3,i=1;MTIz\x1b\\".to_vec();
            s.extend(format!("\x1b_Ga=q,f=24,t=t,s=1,v=1,S={claim},i=2;").bytes());
            s.extend(unpadded(path));
            s.extend_from_slice(b"\x1b\\");
            let shm = b"/icat-baud-absent-object";
            s.extend(format!("\x1b_Ga=q,f=24,t=s,s=1,v=1,S={},i=3;", shm.len()).bytes());
            s.extend(unpadded(shm));
            s.extend_from_slice(b"\x1b\\\x1b[c");
            s
        };
        let probe = d.at("tmp/kitty-tty-graphics-protocol-4057690545");
        let path = bytes_of(&probe).to_vec();
        let kitten = burst(&path, path.len());
        let err = |i: u8| format!("\x1b_Gi={i};EBADF:Failed to read image file\x1b\\");
        let expect = format!("\x1b_Gi=1;OK\x1b\\{}{}\x1b[?1;2c", err(2), err(3));
        let missing = replies_in_reads(d.files(), &kitten, &[]);
        assert_eq!(String::from_utf8_lossy(&missing), expect, "no file there");
        let ok = format!("\x1b_Gi=1;OK\x1b\\\x1b_Gi=2;OK\x1b\\{}\x1b[?1;2c", err(3));
        let mut reads: Vec<Vec<usize>> = vec![vec![]];
        reads.extend((1..kitten.len()).map(|k| vec![k]));
        reads.extend((1..kitten.len()).map(|n| (n..kitten.len()).step_by(n).collect()));
        for cuts in &reads {
            d.put("tmp/kitty-tty-graphics-protocol-4057690545", &[1, 2, 3]);
            let got = replies_in_reads(d.files(), &kitten, cuts);
            assert_eq!(
                String::from_utf8_lossy(&got),
                ok,
                "S past the file is a bound, {cuts:?}"
            );
            assert!(!probe.exists(), "the probe is deleted, {cuts:?}");
        }
        d.put("tmp/kitty-tty-graphics-protocol-4057690545", &[1, 2, 3]);
        let own = replies_in_reads(d.files(), &burst(&path, 3), &[]);
        assert_eq!(String::from_utf8_lossy(&own), ok, "claimed at its size");
    }

    /// A count of the test's own, so no other test's reads fill it.
    fn running() -> &'static Running {
        Box::leak(Box::new(Running::new()))
    }

    /// **A stall a test holds**: a read of a path holding `marker` waits
    /// once its path is resolved until the sender is dropped.
    fn stall(marker: &'static str) -> (std::sync::mpsc::Sender<()>, Stall) {
        let (release, gate) = std::sync::mpsc::channel::<()>();
        let gate = std::sync::Mutex::new(gate);
        let held: Stall = Arc::new(move |p: &Path| {
            if contains(p.as_os_str().as_bytes(), marker.as_bytes()) {
                let _ = gate.lock().unwrap().recv();
            }
        });
        (release, held)
    }

    /// Wait for `running`'s reads to end; ten seconds only turn a hang into
    /// a failure.
    fn settled(running: &'static Running) {
        let end = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while running.now.load(Ordering::Acquire) > 0 {
            assert!(std::time::Instant::now() < end, "the reads never ended");
            std::thread::yield_now();
        }
    }

    /// `r` taken to its end, and what it handed over; ten seconds only
    /// turn a hang into a failure.
    fn finish(r: &mut Reading) -> (Took, Vec<u8>) {
        let end = std::time::Instant::now() + std::time::Duration::from_secs(10);
        let mut got = Vec::new();
        loop {
            let took = r.take(Drain::All, &mut |b| {
                got.extend_from_slice(b);
                KittyFlow::More
            });
            if took != Took::Running {
                return (took, got);
            }
            assert!(std::time::Instant::now() < end, "the read never ended");
            std::thread::yield_now();
        }
    }

    /// **A read off the feed hands over what the reader reads**, and ends
    /// in the one error where the reader refuses: a file, a missing one, a
    /// FIFO, and a temporary file, deleted.
    #[test]
    fn a_read_off_the_feed_hands_over_what_the_reader_reads() {
        let d = Dir::new();
        let data: Vec<u8> = (0..200_000u32)
            .map(|i| u8::try_from(i % 251).unwrap())
            .collect();
        let file = d.put("home/a.raw", &data);
        let temp = d.put("tmp/tty-graphics-protocol-r", &data[..24]);
        let fifo = d.at("home/fifo");
        let made = std::process::Command::new("mkfifo").arg(&fifo).status();
        assert!(made.unwrap().success(), "mkfifo");
        let missing = d.at("home/missing");
        let r = FileReader::for_test(&[d.at("tmp")], running(), None);
        let mut big = r.start(&whole(KittyMedium::File, bytes_of(&file)), None);
        assert_eq!(finish(&mut big), (Took::Ended(Ok(())), data.clone()));
        let t = KittyFileRequest {
            medium: KittyMedium::TempFile,
            ..whole(KittyMedium::TempFile, bytes_of(&temp))
        };
        assert_eq!(
            finish(&mut r.start(&t, None)),
            (Took::Ended(Ok(())), data[..24].to_vec())
        );
        assert!(!temp.exists(), "t=t is deleted on the read's thread");
        for p in [&missing, &fifo] {
            let got = finish(&mut r.start(&whole(KittyMedium::File, bytes_of(p)), None));
            assert_eq!(
                got,
                (Took::Ended(Err(KittyFileFailed)), Vec::new()),
                "{p:?}"
            );
        }
    }

    /// **A read given up still cleans up**: a `t=t` file whose read a
    /// stall holds, its `Reading` dropped, is deleted once the stall lets
    /// the thread go on, and a `t=s` object whose `Reading` is dropped at
    /// once is unlinked -- the thread stops at its next piece, unsent.
    #[test]
    fn a_reading_given_up_still_deletes_and_unlinks() {
        let d = Dir::new();
        let running = running();
        let (release, held) = stall("tty-graphics-protocol-held");
        let r = FileReader::for_test(&[d.at("tmp")], running, Some(held));
        let temp = d.put("tmp/tty-graphics-protocol-held", &[5u8; 24]);
        let t = KittyFileRequest {
            medium: KittyMedium::TempFile,
            ..whole(KittyMedium::TempFile, bytes_of(&temp))
        };
        let mut reading = r.start(&t, None);
        assert_eq!(
            reading.take(Drain::All, &mut |_| KittyFlow::More),
            Took::Running,
            "held"
        );
        drop(reading);
        assert!(temp.exists(), "not read yet");
        drop(release);
        settled(running);
        assert!(!temp.exists(), "deleted after its reading was dropped");
        static N: AtomicUsize = AtomicUsize::new(0);
        let name = format!(
            "/bkg{}-{}",
            std::process::id() % 100_000,
            N.fetch_add(1, Ordering::Relaxed)
        );
        make_shm(&name, &[9u8; 24]);
        drop(r.start(&whole(KittyMedium::SharedMemory, name.as_bytes()), None));
        settled(running);
        assert!(!shm_exists(&name), "unlinked after its reading was dropped");
    }

    /// **Past [`READERS`] a file medium is refused at once, unread, and
    /// counted**: with that many reads held by a stall, the next ends in
    /// the one error before any thread runs, its `t=t` file not opened and
    /// so not deleted; once the stalled reads end, it is read and deleted.
    #[test]
    fn past_the_readers_a_file_medium_is_refused_unread_and_counted() {
        let d = Dir::new();
        let running = running();
        let (release, held) = stall("stalled");
        let r = FileReader::for_test(&[d.at("tmp")], running, Some(held));
        let stalled = d.put("home/stalled.raw", &[1u8; 24]);
        let readings: Vec<Reading> = (0..READERS)
            .map(|_| r.start(&whole(KittyMedium::File, bytes_of(&stalled)), None))
            .collect();
        assert_eq!(running.now.load(Ordering::Acquire), READERS);
        let temp = d.put("tmp/tty-graphics-protocol-cap", &[3u8; 24]);
        let t = KittyFileRequest {
            medium: KittyMedium::TempFile,
            ..whole(KittyMedium::TempFile, bytes_of(&temp))
        };
        let mut refused = r.start(&t, None);
        assert_eq!(
            refused.take(Drain::Step, &mut |_| KittyFlow::More),
            Took::Ended(Err(KittyFileFailed)),
            "refused at once"
        );
        assert_eq!(r.refused(), RefusedAtCap(1));
        assert!(temp.exists(), "refused unopened, so not deleted");
        drop(readings);
        drop(release);
        settled(running);
        assert_eq!(
            finish(&mut r.start(&t, None)),
            (Took::Ended(Ok(())), vec![3u8; 24])
        );
        assert!(!temp.exists(), "read and deleted once a place is free");
        assert_eq!(r.refused(), RefusedAtCap(1));
    }
}
