//! **The kitty file mediums, read for the core** (`[[source:
//! kitty-graphics]]`, "The transmission medium"; note V, V19): `t=f` a
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
//! 4. **The open cannot be swapped under the check**: the resolved path,
//!    which has no link in it, is opened following none -- every component
//!    under Darwin's `O_NOFOLLOW_ANY`, the last under glibc's `O_NOFOLLOW`
//!    -- and with `O_NONBLOCK`, so a FIFO put there cannot block the open;
//!    then the descriptor's own `fstat` must be a regular file with the
//!    device and inode the check saw, so any other swap is refused before
//!    a byte is read. glibc's `open` has no flag for the directories above,
//!    so on Linux the descriptor's own path, read back from
//!    `/proc/self/fd`, must be the resolved path; the open has happened by
//!    then, as it has when kitty checks again (`[[source:
//!    kitty-utils-py]]`). Linux is type-checked, not run.
//!
//! The length is decided from that `fstat` before reading
//! ([`KittyFileRequest::read_len`]), so no object is read past what the
//! image store was given, and the read is `pread` in 64 KiB pieces on the
//! thread that feeds the core: bounded by the store's budget, a local
//! disk's few milliseconds, and the decode after it is stepped as `t=d`'s
//! is. A file on a network mount that stalls stalls that thread; this does
//! not answer that.
//!
//! **Every failure is one error**, kitty's `EBADF:Failed to read image
//! file`, so "the responses ... learn [nothing] about files it cannot read
//! itself"; the reason goes to the log at `warn`, which only the local
//! user reads, as kitty writes it to its own.
//!
//! **What is deleted.** A `t=t` file is deleted only once it was opened and
//! read as the file the path named, and only when its resolved path lies
//! in a known temporary directory -- `/tmp`, `/dev/shm`, `TMPDIR` and the
//! platform's own -- compared component by component, where kitty's
//! temporary-directory test compares strings and lets `/tmpfoo` pass for
//! `/tmp` (`[[source: kitty-utils-py]]`) -- and contains
//! `tty-graphics-protocol`; kitty deletes a file it opened even when the
//! load then fails, so it does not accumulate (`[[source:
//! kitty-graphics-c]]` `load_image_data`), and so does this. A `t=s`
//! object is unlinked once opened, read or not.
//!
//! `unsafe` here is the shared-memory read alone: `shm_open` and
//! `shm_unlink`, a descriptor taken into a `File`, and on Darwin the
//! mapping its shared memory is read through.

#![allow(unsafe_code)]

use baud_domain::{KittyFileFailed, KittyFileRequest, KittyFiles, KittyFlow, KittyMedium};
use std::ffi::{CString, OsStr};
use std::fs::{File, Metadata, OpenOptions};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{FileExt, MetadataExt, OpenOptionsExt};
use std::path::{Component, Path, PathBuf};

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
    /// The descriptor is not the file the check saw, or not regular.
    Swapped,
    /// The descriptor's own path is not the resolved one: a directory
    /// above it was swapped for a link (Linux).
    #[cfg(not(target_os = "macos"))]
    Elsewhere,
    /// "smaller than the client claimed", or past the store's bound.
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
    temp: Vec<PathBuf>,
    buf: Vec<u8>,
    /// Run between the check and the open, so a test can swap the file
    /// there and need no race.
    #[cfg(test)]
    between: Option<Between>,
    /// Run between resolving the path and checking it.
    #[cfg(test)]
    resolved: Option<Between>,
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
        let mut dirs = vec![PathBuf::from("/tmp"), PathBuf::from("/dev/shm")];
        if let Some(t) = std::env::var_os("TMPDIR") {
            dirs.push(PathBuf::from(t));
        }
        dirs.push(std::env::temp_dir());
        LocalFiles::with_temp(&dirs)
    }

    /// The reader with `dirs` as the temporary directories.
    pub fn with_temp(dirs: &[PathBuf]) -> LocalFiles {
        let mut temp: Vec<PathBuf> = dirs
            .iter()
            .filter_map(|d| std::fs::canonicalize(d).ok())
            .collect();
        temp.sort();
        temp.dedup();
        LocalFiles {
            temp,
            buf: Vec::new(),
            #[cfg(test)]
            between: None,
            #[cfg(test)]
            resolved: None,
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
        #[cfg(test)]
        if let Some(f) = self.between.as_mut() {
            f(&real);
        }
        let file = OpenOptions::new()
            .read(true)
            .custom_flags(crate::sys::NO_LINKS | crate::sys::O_NONBLOCK)
            .open(&real)
            .map_err(|_| Refusal::Open)?;
        #[cfg(not(target_os = "macos"))]
        {
            use std::os::fd::AsRawFd;
            let at = std::fs::read_link(format!("/proc/self/fd/{}", file.as_raw_fd()));
            if at.ok().as_deref() != Some(real.as_path()) {
                return Err(Refusal::Elsewhere);
            }
        }
        let opened = file.metadata().map_err(|_| Refusal::Swapped)?;
        if !same_regular_file(&before, &opened) {
            return Err(Refusal::Swapped);
        }
        let read = self.pread(&file, req, &opened, sink);
        if req.medium == KittyMedium::TempFile && self.deletable(&real) {
            if let Err(e) = std::fs::remove_file(&real) {
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

/// Whether `hay` contains `needle`.
fn contains(hay: &[u8], needle: &[u8]) -> bool {
    hay.windows(needle.len()).any(|w| w == needle)
}

#[cfg(test)]
mod tests {
    use super::*;
    use baud_core::{BytePos, Owned, ReadLen, Term, WinSize};
    use baud_domain::{ImageBudget, ImagePx, KittyPastLimit};
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
            (bytes_of(&short), Err(Refusal::Size)),
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
    /// resolved is refused**: the checks were made on the resolved path,
    /// and the open follows no link in it. The link points at a directory
    /// holding a regular file of the same name, standing for a sensitive
    /// one: every other check passes on it.
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
        let got = ask(&mut f, &whole(KittyMedium::File, bytes_of(&real)));
        #[cfg(target_os = "macos")]
        assert_eq!(got, Err(Refusal::Open));
        #[cfg(not(target_os = "macos"))]
        assert_eq!(got, Err(Refusal::Elsewhere));
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

    /// The bridge between `kitten` and the harness below: a pty of 24x80
    /// cells of 10x20 pixels, kitten on its subsidiary, and its manager
    /// relayed to this process's stdin and stdout.
    const BRIDGE: &str = r#"
import fcntl, os, pty, select, struct, sys, termios
kitten, mode, image = sys.argv[1:4]
pid, m = pty.fork()
if pid == 0:
    fcntl.ioctl(0, termios.TIOCSWINSZ, struct.pack("HHHH", 24, 80, 800, 480))
    os.environ["TERM"] = "xterm-256color"
    os.execv(kitten, [kitten, "icat", "--transfer-mode=" + mode, image])
done = False
while True:
    r, _, _ = select.select([m, 0], [], [], 0.2)
    if m in r:
        try:
            d = os.read(m, 65536)
        except OSError:
            d = b""
        if not d:
            break
        os.write(1, d)
    if 0 in r:
        d = os.read(0, 65536)
        if d:
            os.write(m, d)
os.waitpid(pid, 0)
"#;

    /// **`kitten icat` through the reader, live**: `--transfer-mode=file`,
    /// `memory` and `detect` each run against a terminal reading through
    /// [`LocalFiles`], its replies written back, and the picture is placed,
    /// by `t=f` and `t=s` where those were asked for. Under `detect`
    /// kitten's `t=t` query carries `S` as its path's length, which the
    /// text's "smaller than the client claimed" refuses, and kitten sends
    /// `t=d`. Not in the suite: it needs kitten, named by
    /// `BAUD_KITTEN`. Prints the control data kitten sent.
    #[test]
    #[ignore = "needs kitten: BAUD_KITTEN=<path to kitten>"]
    fn kitten_icat_reads_through_the_reader_live() {
        use std::io::{Read as _, Write as _};
        let kitten = std::env::var("BAUD_KITTEN").expect("BAUD_KITTEN");
        let d = Dir::new();
        let image = d.put("home/icat.png", PNG);
        for mode in ["file", "memory", "detect"] {
            let mut child = std::process::Command::new("python3")
                .args(["-I", "-c", BRIDGE, &kitten, mode])
                .arg(&image)
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::piped())
                .spawn()
                .expect("python3");
            let mut out = child.stdout.take().unwrap();
            let mut inp = child.stdin.take().unwrap();
            let (tx, rx) = std::sync::mpsc::channel::<Vec<u8>>();
            std::thread::spawn(move || {
                let mut b = vec![0u8; 65536];
                while let Ok(n) = out.read(&mut b) {
                    if n == 0 || tx.send(b[..n].to_vec()).is_err() {
                        break;
                    }
                }
            });
            let mut owned = Owned::new(&WS).with_images(ImageBudget::of(1 << 20));
            let mut t = Term::new(owned.storage(), &WS);
            t.set_cell_size(ImagePx::of(10), ImagePx::of(20));
            t.set_kitty_files(Box::leak(Box::new(LocalFiles::new())));
            // kitten's detection waits ten seconds on `CSI ? 6 c` and goes
            // on at once on `CSI ? 6 ; 4 c`, baud's other answer (the
            // record, "A `kitten icat` session, recorded").
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
                    let _ = inp.write_all(t.reply());
                    t.clear_reply();
                }
            }
            let _ = child.wait();
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
            // `detect`, whatever kitten picked from the answers.
            let sent = keys.iter().rev().find(|k| k.contains("a=T")).unwrap();
            match mode {
                "file" => assert!(sent.contains("t=f"), "{sent}"),
                "memory" => assert!(sent.contains("t=s"), "{sent}"),
                _ => {}
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
}
