//! **Metal's shader cache faults, and the suite's archive is not reached
//! by it** (`src/metal/suite.rs`).
//!
//! A pipeline compiled at run time is looked up in Metal's compiler file
//! cache. When a cache data file the process has open shrinks under it and
//! the cache cannot reopen its files, the lookup reads a `FILE` through a
//! null pointer (fscache_release -> is_data_file_invalid -> fileno)
//! and the process dies of `SIGSEGV`. Either condition alone is survived.
//! Under load, other processes writing the cache that every unbundled
//! process shares made the same fault in a test suite; here it is made on
//! purpose, both conditions at once, in a cache no other process uses.
//!
//! Two children, each run twice: once to put the trail's pipeline in the
//! cache, once to build the background (which opens the cache), shrink
//! every cache data file to nothing and make the cache read-only, and build
//! the trail's pipeline again. The control builds it through the compiler,
//! as the app does, and must die by a signal; the arm builds it from the
//! suite's archive, as every test does, and must finish. If the control
//! ever finishes, this Metal no longer faults, and the arm proves nothing
//! here.
//!
//! **The children's cache is their own.** A test binary has no bundle id,
//! so its cache is the directory every unbundled process on the machine
//! shares. Each child runs a hard link of this binary inside a temporary
//! `.app` whose `Info.plist` names a bundle id of this run's own, and Metal
//! keeps that child's cache under that id. Every file this test changes is
//! asserted to lie under that directory, which is deleted afterwards.
//!
//! A child is started by its path, never through `open` or Launch
//! Services, and a run killed partway leaves its bundle and cache behind
//! only until the next run, which removes any older than ten minutes.
//!
//! **No window.** A child calls only Metal: the device, the library and
//! pipeline states ([`SuiteBuilds`]). Nothing here reaches AppKit.
//!
//! **No crash report.** The control's fault would otherwise leave a crash
//! report on every run. Just before its last build, a faulting child takes
//! every exception from its task's exception ports, the corpse
//! notification that reports are made from included, so the kernel's
//! signal ends it with nothing to report (`mach`).
//!
//! **A Mac only**, like every test in this crate.
#![cfg(target_os = "macos")]

use baud_present_macos::metal::{CodeFrom, Pipe, SuiteBuilds};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::Command;

/// The child's step, when this process is a child.
const STEP: &str = "BAUD_CACHE_FAULT_STEP";
/// The child's private cache, which it may change.
const CACHE: &str = "BAUD_CACHE_FAULT_DIR";
/// This test's name, which a child is run with.
const NAME: &str = "metal_faults_on_a_shrunk_cache_and_the_archive_is_not_reached";

/// The pipeline the fault is made on: one the background does not share.
const TRAIL: Pipe = Pipe::Fire;

#[test]
fn metal_faults_on_a_shrunk_cache_and_the_archive_is_not_reached() {
    if let Ok(step) = std::env::var(STEP) {
        child(&step);
        return;
    }
    sweep();
    // The archive written before any child, so a child only reads it.
    let builds = SuiteBuilds::new();
    builds
        .build(TRAIL, CodeFrom::Archive)
        .expect("the trail's pipeline from the suite's archive");
    drop(builds);

    let run = Run::new();
    let control = run.fault(CodeFrom::Compiler);
    let arm = run.fault(CodeFrom::Archive);
    // A process that builds every pipeline from the archive and nothing
    // through the compiler, as a test does: what it leaves in its cache.
    run.clear();
    let only = run.child("archive-only");
    let left = data_files(&run.cache);
    drop(run);
    assert!(only.success(), "every pipeline from the archive: {only:?}");
    assert!(
        left.is_empty(),
        "a process that builds only from the archive still wrote the shader cache: {left:?}"
    );

    assert!(
        arm.success(),
        "built from the suite's archive, the pipeline must survive the shrunk cache: {arm:?}"
    );
    let os = Command::new("/usr/bin/sw_vers")
        .arg("-buildVersion")
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default();
    assert!(
        matches!(control.signal(), Some(11 | 10)),
        "the control, compiled through the cache, must die of SIGSEGV or SIGBUS, and \
         ended {control:?}: Metal's cache no longer faults on {os}, the archive arm proves \
         nothing here; revisit the suite archive"
    );
}

/// **A run's private bundle, and the cache Metal keeps for it.**
struct Run {
    root: PathBuf,
    exe: PathBuf,
    cache: PathBuf,
}

impl Run {
    fn new() -> Run {
        let id = format!(
            "{ID}{}.{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("a clock after 1970")
                .as_nanos()
        );
        let root = std::env::temp_dir().join(format!("{id}.d"));
        let macos = root.join("Fault.app/Contents/MacOS");
        std::fs::create_dir_all(&macos).expect("the bundle");
        std::fs::write(
            root.join("Fault.app/Contents/Info.plist"),
            format!(
                "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
                 <!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \
                 \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
                 <plist version=\"1.0\"><dict>\
                 <key>CFBundleIdentifier</key><string>{id}</string>\
                 <key>CFBundleExecutable</key><string>fault</string>\
                 <key>CFBundlePackageType</key><string>APPL</string>\
                 </dict></plist>\n"
            ),
        )
        .expect("the bundle's Info.plist");
        let exe = macos.join("fault");
        let me = std::env::current_exe().expect("this binary");
        if std::fs::hard_link(&me, &exe).is_err() {
            std::fs::copy(&me, &exe).expect("this binary into the bundle");
        }
        let cache = user_cache_dir().join(&id);
        assert!(
            !cache.exists(),
            "a fresh bundle id has no cache yet: {}",
            cache.display()
        );
        Run { root, exe, cache }
    }

    /// A child at `step`, its status.
    fn child(&self, step: &str) -> std::process::ExitStatus {
        Command::new(&self.exe)
            .args(["--exact", NAME, "--nocapture", "--test-threads=1"])
            .env(STEP, step)
            .env(CACHE, &self.cache)
            .status()
            .expect("the child runs")
    }

    /// The trail's pipeline put in a fresh private cache, then built again
    /// after the cache is shrunk and made read-only, its code from `from`.
    fn fault(&self, from: CodeFrom) -> std::process::ExitStatus {
        self.clear();
        let warm = self.child("warm");
        assert!(warm.success(), "the warming child: {warm:?}");
        assert!(
            !data_files(&self.cache).is_empty(),
            "Metal kept no cache under the child's bundle id, {}: the fault cannot be made \
             without touching the shared one",
            self.cache.display()
        );
        let step = match from {
            CodeFrom::Compiler => "fault-compiler",
            CodeFrom::Archive => "fault-archive",
        };
        let ended = self.child(step);
        self.clear();
        ended
    }

    /// The private cache, writable again and gone.
    fn clear(&self) {
        writable(&self.cache);
        let _ = std::fs::remove_dir_all(&self.cache);
    }
}

impl Drop for Run {
    fn drop(&mut self) {
        self.clear();
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

/// **What a child does**, in its own bundle.
fn child(step: &str) {
    let cache = PathBuf::from(std::env::var(CACHE).expect("the private cache"));
    assert!(
        cache
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.starts_with(ID)),
        "not a private cache: {}",
        cache.display()
    );
    let builds = SuiteBuilds::new();
    match step {
        "warm" => {
            builds
                .build(Pipe::Bg, CodeFrom::Compiler)
                .expect("the background");
            builds.build(TRAIL, CodeFrom::Compiler).expect("the trail");
        }
        "archive-only" => {
            for pipe in Pipe::ALL {
                builds
                    .build(pipe, CodeFrom::Archive)
                    .expect("from the archive");
            }
        }
        "fault-compiler" | "fault-archive" => {
            builds
                .build(Pipe::Bg, CodeFrom::Compiler)
                .expect("the background");
            let files = data_files(&cache);
            assert!(!files.is_empty(), "the warmed cache's data files");
            for f in &files {
                assert!(
                    f.starts_with(&cache),
                    "outside the private cache: {}",
                    f.display()
                );
                std::fs::OpenOptions::new()
                    .write(true)
                    .open(f)
                    .and_then(|h| h.set_len(0))
                    .expect("a data file shrunk");
            }
            read_only(&cache);
            let from = match step {
                "fault-compiler" => CodeFrom::Compiler,
                _ => CodeFrom::Archive,
            };
            mach::no_report();
            builds
                .build(TRAIL, from)
                .expect("the trail, after the shrink");
        }
        other => panic!("no step {other}"),
    }
}

/// The prefix every bundle id of this test's begins with.
const ID: &str = "dev.baud.test.shader-cache-fault.";

/// **What killed runs left**, removed: this test's bundles in the temporary
/// folder and their caches, each older than ten minutes, so a run going on
/// beside this one keeps its own.
fn sweep() {
    let old = std::time::Duration::from_secs(10 * 60);
    for (dir, suffix) in [(std::env::temp_dir(), ".d"), (user_cache_dir(), "")] {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            let ours = name.starts_with(ID) && name.ends_with(suffix);
            let aged = e
                .metadata()
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| t.elapsed().ok())
                .is_some_and(|age| age > old);
            if ours && aged {
                let p = e.path();
                writable(&p);
                let _ = std::fs::remove_dir_all(&p);
            }
        }
    }
}

/// **The task's exception ports cleared** (`task_set_exception_ports`,
/// `<mach/task.h>`), so a fault in this process is ended by its signal and
/// reported to no one. The layouts and values are held to the headers by
/// `the_mach_declarations_match_the_headers`.
mod mach {
    /// `mach_port_t`, `task_t`: `natural_t`, an `unsigned int`.
    pub type Port = u32;
    /// `exception_mask_t`: an `unsigned int`.
    pub type Mask = u32;
    /// The C ints: exception_behavior_t, thread_state_flavor_t and
    /// kern_return_t.
    pub type Int = i32;

    /// `EXC_MASK_ALL | EXC_MASK_CRASH | EXC_MASK_CORPSE_NOTIFY`.
    pub const MASK: Mask = 0x3ffe;
    /// `MACH_PORT_NULL`.
    pub const NULL: Port = 0;
    /// `EXCEPTION_DEFAULT`.
    pub const DEFAULT: Int = 1;
    /// `THREAD_STATE_NONE` on arm64.
    pub const NO_STATE: Int = 5;

    unsafe extern "C" {
        /// What `mach_task_self()` reads.
        static mach_task_self_: Port;
        fn task_set_exception_ports(
            task: Port,
            mask: Mask,
            new_port: Port,
            behavior: Int,
            flavor: Int,
        ) -> Int;
    }

    /// Every exception this task would report, sent nowhere.
    pub fn no_report() {
        // SAFETY: mach_task_self_ is set before `main` and never written
        // after; the call takes plain values, and `MACH_PORT_NULL` clears.
        let done =
            unsafe { task_set_exception_ports(mach_task_self_, MASK, NULL, DEFAULT, NO_STATE) };
        assert_eq!(done, 0, "task_set_exception_ports: {done}");
    }
}

/// **The declarations in [`mach`] are the headers'**: a C program built
/// from `<mach/mach.h>` prints each size and value, and each is compared.
#[test]
fn the_mach_declarations_match_the_headers() {
    let dir = std::env::temp_dir().join(format!("baud-mach-probe-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("the probe's folder");
    let src = dir.join("probe.c");
    std::fs::write(
        &src,
        "#include <mach/mach.h>\n#include <stdio.h>\n\
         int main(void) {\n\
         printf(\"%zu %zu %zu %zu %zu %zu\\n\", sizeof(mach_port_t), sizeof(task_t), \
         sizeof(exception_mask_t), sizeof(exception_behavior_t), \
         sizeof(thread_state_flavor_t), sizeof(kern_return_t));\n\
         printf(\"%u %u %d %d\\n\", (unsigned)(EXC_MASK_ALL | EXC_MASK_CRASH | \
         EXC_MASK_CORPSE_NOTIFY), (unsigned)MACH_PORT_NULL, EXCEPTION_DEFAULT, \
         THREAD_STATE_NONE);\n\
         return 0;\n}\n",
    )
    .expect("the probe's source");
    let exe = dir.join("probe");
    let cc = std::env::var("CC").unwrap_or_else(|_| "cc".to_string());
    let built = Command::new(&cc)
        .arg(&src)
        .arg("-o")
        .arg(&exe)
        .status()
        .unwrap_or_else(|e| panic!("the C header probe needs a C compiler ({cc}): {e}"));
    assert!(built.success(), "the probe did not build: {built:?}");
    let out = Command::new(&exe).output().expect("the probe runs");
    let _ = std::fs::remove_dir_all(&dir);
    let text = String::from_utf8(out.stdout).expect("the probe's text");
    let want = format!(
        "{} {} {} {} {} {}\n{} {} {} {}\n",
        size_of::<mach::Port>(),
        size_of::<mach::Port>(),
        size_of::<mach::Mask>(),
        size_of::<mach::Int>(),
        size_of::<mach::Int>(),
        size_of::<mach::Int>(),
        mach::MASK,
        mach::NULL,
        mach::DEFAULT,
        mach::NO_STATE,
    );
    assert_eq!(text, want, "the declarations differ from <mach/mach.h>");
}

/// `getconf DARWIN_USER_CACHE_DIR`.
fn user_cache_dir() -> PathBuf {
    let out = Command::new("/usr/bin/getconf")
        .arg("DARWIN_USER_CACHE_DIR")
        .output()
        .expect("getconf");
    assert!(out.status.success(), "getconf: {:?}", out.status);
    PathBuf::from(String::from_utf8(out.stdout).expect("a path").trim())
}

/// Every `*.data` file under `dir`.
fn data_files(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&d) else {
            continue;
        };
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().is_some_and(|x| x == "data") {
                out.push(p);
            }
        }
    }
    out
}

/// Every folder under `dir` read-only and every file read-only, so the
/// cache can neither recreate nor reopen its files for writing.
fn read_only(dir: &Path) {
    walk(dir, 0o500, 0o400);
}

/// Every folder and file under `dir` writable again.
fn writable(dir: &Path) {
    walk(dir, 0o700, 0o600);
}

/// Folders to `dirs` and files to `files`; folders made writable before
/// their entries are read, and read-only after.
fn walk(dir: &Path, dirs: u32, files: u32) {
    if !dir.is_dir() {
        return;
    }
    let opening = dirs & 0o200 != 0;
    if opening {
        let _ = std::fs::set_permissions(dir, std::fs::Permissions::from_mode(dirs));
    }
    if let Ok(entries) = std::fs::read_dir(dir) {
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() {
                walk(&p, dirs, files);
            } else {
                let _ = std::fs::set_permissions(&p, std::fs::Permissions::from_mode(files));
            }
        }
    }
    if !opening {
        let _ = std::fs::set_permissions(dir, std::fs::Permissions::from_mode(dirs));
    }
}
