//! **The suite's binary archive** (`suite-archive`, which only tests and
//! benches enable): the compiled code of every [`Pipe`], built once per
//! machine and read by every test and bench process, so that none of them
//! compiles a pipeline through the system's shader cache.
//!
//! **Why.** A pipeline compiled at run time goes through Metal's compiler
//! file cache, one directory per bundle id that unbundled processes, every
//! test binary included, all share. When a cache data file that a process
//! has open shrinks under it and the cache cannot reopen its files, Metal
//! reads a `FILE` through a null pointer inside
//! `newRenderPipelineStateWithDescriptor` (fscache_release ->
//! is_data_file_invalid -> fileno), and the process dies of `SIGSEGV`;
//! a mapped file shrunk by another process is a `SIGBUS` in the same cache.
//! `tests/shader_cache_fault.rs` makes the fault on demand in a private
//! cache, and the same pipeline built from this archive survives it. Which
//! process shrinks the shared cache's files during a suite run is inferred,
//! not found: many test processes, of different binaries with different
//! shaders, writing one directory under load.
//!
//! **A miss is refused, never compiled.** Every build passes
//! `MTLPipelineOptionFailOnBinaryArchiveMiss`, so a pipeline the archive
//! lacks fails with an error naming it, rather than compiling back into the
//! shared cache. [`Pipe::ALL`] is what the archive is written from, and
//! [`super::pipeline`] builds only a [`Pipe`], so the two cannot drift.
//!
//! **Keyed, so an OS update never strands the suite.** An archive is code
//! for one GPU and one OS build ("software updates of the OS or device
//! drivers may cause the archive to become outdated",
//! `[[source: macos-sdk-mtlbinaryarchive-h]]`). Its file is named by a hash
//! of the OS build, the device's name and architecture, `baud.metallib`, and
//! every [`Pipe`]'s descriptor as Metal describes it -- functions, formats,
//! blend factors, every property it holds -- so a change to any of them
//! names a new file, which is built.
//!
//! **One build through the cache.** Writing the archive compiles each
//! pipeline once, through the system's cache, in the one process that takes
//! the build's lock; the others wait on that lock and then read the file.
//! That build is the suite's only pass through the cache, and it happens
//! once for each key, not once each run.
//!
//! **A spoiled archive is written again, not read.** A file under the right
//! name that was cut short, or that a build leaving a pipeline out wrote,
//! would refuse every pipeline of every test. So an archive is taken only
//! when its first and last [`Pipe`] build from it; otherwise it is written
//! again, and one just written must give every [`Pipe`]. After a write,
//! other keys' archives a day old are removed, so OS updates do not pile
//! them up.
//!
//! The app never has this: `suite-archive` is enabled only by
//! dev-dependencies, which a build of the app does not use.

use super::{METALLIB, Pipe, descriptor};
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2_foundation::{NSArray, NSError, NSProcessInfo, NSString, NSURL};
use objc2_metal::{
    MTLBinaryArchive, MTLBinaryArchiveDescriptor, MTLDevice, MTLLibrary, MTLPipelineOption,
    MTLRenderPipelineDescriptor, MTLRenderPipelineState,
};
use std::cell::OnceCell;
use std::path::{Path, PathBuf};

thread_local! {
    /// The archive, loaded once on each thread that builds a pipeline: it
    /// never crosses a thread, so nothing need be claimed of it beyond what
    /// the device and the library already are.
    static ARCHIVE: OnceCell<Retained<ProtocolObject<dyn MTLBinaryArchive>>> =
        const { OnceCell::new() };
}

/// **`desc`'s pipeline, from the suite's archive**, refused when the
/// archive does not hold it.
pub(super) fn build(
    device: &ProtocolObject<dyn MTLDevice>,
    library: &ProtocolObject<dyn MTLLibrary>,
    desc: &MTLRenderPipelineDescriptor,
) -> Result<Retained<ProtocolObject<dyn MTLRenderPipelineState>>, Retained<NSError>> {
    let archive = ARCHIVE.with(|a| a.get_or_init(|| load(device, library)).clone());
    desc.setBinaryArchives(Some(&NSArray::from_retained_slice(&[archive])));
    device.newRenderPipelineStateWithDescriptor_options_reflection_error(
        desc,
        MTLPipelineOption::FailOnBinaryArchiveMiss,
        None,
    )
}

/// The archive for this machine and this source, written first if no
/// process has written it yet.
fn load(
    device: &ProtocolObject<dyn MTLDevice>,
    library: &ProtocolObject<dyn MTLLibrary>,
) -> Retained<ProtocolObject<dyn MTLBinaryArchive>> {
    let path = path(device, library);
    if let Some(a) = open(device, library, &path) {
        return a;
    }
    let dir = path.parent().expect("the archive's folder");
    std::fs::create_dir_all(dir)
        .unwrap_or_else(|e| panic!("the suite archive's folder {}: {e}", dir.display()));
    let lock = std::fs::File::create(path.with_extension("lock"))
        .unwrap_or_else(|e| panic!("the suite archive's lock: {e}"));
    lock.lock()
        .unwrap_or_else(|e| panic!("the suite archive's lock: {e}"));
    // Another process may have written it while this one waited.
    if let Some(a) = open(device, library, &path) {
        return a;
    }
    eprintln!(
        "baud-present-macos: writing the suite's binary archive, {}",
        path.display()
    );
    let archive = device
        .newBinaryArchiveWithDescriptor_error(&MTLBinaryArchiveDescriptor::new())
        .unwrap_or_else(|e| panic!("an empty binary archive: {e:?}"));
    for pipe in Pipe::ALL {
        let desc = descriptor(library, pipe)
            .unwrap_or_else(|| panic!("baud.metallib has {pipe:?}'s functions"));
        archive
            .addRenderPipelineFunctionsWithDescriptor_error(&desc)
            .unwrap_or_else(|e| panic!("{pipe:?} into the suite archive: {e:?}"));
    }
    // Written under another name and moved into place, so a reader never
    // finds half an archive.
    let partial = path.with_extension(format!("{}.partial", std::process::id()));
    archive
        .serializeToURL_error(&url(&partial))
        .unwrap_or_else(|e| panic!("the suite archive to {}: {e:?}", partial.display()));
    std::fs::rename(&partial, &path)
        .unwrap_or_else(|e| panic!("the suite archive into place: {e}"));
    prune(dir, &path);
    let archive = loaded(device, &path).expect("the suite archive just written loads");
    let missed: Vec<Pipe> = Pipe::ALL
        .into_iter()
        .filter(|&p| !hits(device, library, &archive, p))
        .collect();
    assert!(
        missed.is_empty(),
        "the suite archive just written lacks {missed:?}"
    );
    archive
}

/// **The archive at `path`, if there is one that loads and holds what it
/// should**: a file that loads but misses -- cut short, or written by a
/// build that left a pipeline out -- is passed over, so it is written again
/// rather than refusing every pipeline of every test. Two builds tell: the
/// first [`Pipe`] and the last.
fn open(
    device: &ProtocolObject<dyn MTLDevice>,
    library: &ProtocolObject<dyn MTLLibrary>,
    path: &Path,
) -> Option<Retained<ProtocolObject<dyn MTLBinaryArchive>>> {
    let archive = loaded(device, path)?;
    let probes = [Pipe::ALL[0], Pipe::ALL[Pipe::ALL.len() - 1]];
    if probes
        .into_iter()
        .all(|p| hits(device, library, &archive, p))
    {
        return Some(archive);
    }
    eprintln!(
        "baud-present-macos: the suite's binary archive {} misses; writing it again",
        path.display()
    );
    None
}

/// The archive at `path`, if there is a file there Metal loads.
fn loaded(
    device: &ProtocolObject<dyn MTLDevice>,
    path: &Path,
) -> Option<Retained<ProtocolObject<dyn MTLBinaryArchive>>> {
    if !path.is_file() {
        return None;
    }
    let d = MTLBinaryArchiveDescriptor::new();
    d.setUrl(Some(&url(path)));
    device.newBinaryArchiveWithDescriptor_error(&d).ok()
}

/// Whether `archive` gives `pipe` with a miss refused.
fn hits(
    device: &ProtocolObject<dyn MTLDevice>,
    library: &ProtocolObject<dyn MTLLibrary>,
    archive: &Retained<ProtocolObject<dyn MTLBinaryArchive>>,
    pipe: Pipe,
) -> bool {
    let Some(desc) = descriptor(library, pipe) else {
        return false;
    };
    desc.setBinaryArchives(Some(&NSArray::from_retained_slice(std::slice::from_ref(
        archive,
    ))));
    device
        .newRenderPipelineStateWithDescriptor_options_reflection_error(
            &desc,
            MTLPipelineOption::FailOnBinaryArchiveMiss,
            None,
        )
        .is_ok()
}

/// **Other keys' archives, once a day old, removed** after a write, so an
/// OS update or a change of descriptors does not leave every earlier
/// archive behind. A day, so two checkouts with different descriptors,
/// each testing in turn, do not remove each other's and write them again.
/// A partial write is never touched.
fn prune(dir: &Path, kept: &Path) {
    let Some(stem) = kept.file_stem() else {
        return;
    };
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let day = std::time::Duration::from_secs(24 * 60 * 60);
    for e in entries.flatten() {
        let p = e.path();
        let ours = p
            .extension()
            .is_some_and(|x| x == "metallib-archive" || x == "lock");
        let stale = e
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.elapsed().ok())
            .is_some_and(|age| age > day);
        if ours && stale && p.file_stem() != Some(stem) {
            let _ = std::fs::remove_file(&p);
        }
    }
}

/// A file URL naming `path`.
fn url(path: &Path) -> Retained<NSURL> {
    NSURL::fileURLWithPath(&NSString::from_str(&path.display().to_string()))
}

/// **Where this machine's archive for this source lives**: the user's
/// temporary folder, named by the key's hash.
pub(super) fn path(
    device: &ProtocolObject<dyn MTLDevice>,
    library: &ProtocolObject<dyn MTLLibrary>,
) -> PathBuf {
    let os = NSProcessInfo::processInfo()
        .operatingSystemVersionString()
        .to_string();
    let gpu = device.name().to_string();
    let architecture = device.architecture().name().to_string();
    let descriptors: Vec<String> = Pipe::ALL
        .into_iter()
        .map(|pipe| {
            descriptor(library, pipe).map_or_else(
                || format!("{pipe:?}: none"),
                |d| described(&format!("{d:?}")),
            )
        })
        .collect();
    let mut key = Fnv::new();
    let parts = [
        os.as_bytes(),
        gpu.as_bytes(),
        architecture.as_bytes(),
        METALLIB,
    ];
    for part in parts
        .into_iter()
        .chain(descriptors.iter().map(String::as_bytes))
    {
        key.write(part);
        // A separator, so moving a byte from one part to the next is a
        // different key.
        key.write(&[0xFF]);
    }
    std::env::var_os(DIR)
        .map_or_else(
            || std::env::temp_dir().join("baud-suite-archive"),
            PathBuf::from,
        )
        .join(format!("{:016x}.metallib-archive", key.0))
}

/// **Where a test puts the archives instead** (`tests/suite_archive.rs`),
/// so that it may spoil one without touching the archive every other test
/// process reads.
const DIR: &str = "BAUD_SUITE_ARCHIVE_DIR";

/// **A descriptor's description without its objects' addresses**: Metal
/// prints `<Class: 0x...>` for the descriptor and each function in it,
/// which differs in every process, beside every property, which does not.
fn described(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find(": 0x") {
        let (head, tail) = rest.split_at(at);
        let hex = tail[4..]
            .find(|c: char| !c.is_ascii_hexdigit())
            .unwrap_or(tail.len() - 4);
        out.push_str(head);
        if tail[4 + hex..].starts_with('>') {
            // An address: dropped.
            rest = &tail[4 + hex..];
        } else {
            // A value, such as a sample mask: kept.
            out.push_str(&tail[..4 + hex]);
            rest = &tail[4 + hex..];
        }
    }
    out.push_str(rest);
    out
}

/// FNV-1a, 64 bits: a name for the key, not a check against tampering.
struct Fnv(u64);

impl Fnv {
    fn new() -> Fnv {
        Fnv(0xcbf2_9ce4_8422_2325)
    }

    fn write(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.0 = (self.0 ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
}
