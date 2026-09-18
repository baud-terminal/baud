//! What both presentation layers set up the same way: the grid the storage
//! is provisioned for, the cache probe whose answer picks the grid's pitch,
//! and the faces the config names -- read once, parsed once, and leaked,
//! because the renderer borrows them for the life of the process.

use crate::config::Config;
use baud_domain::{CacheGeometry, CacheLevel, WinSize};
use baud_font::{Face, FaceIndex};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

/// The largest grid the storage is provisioned for.
pub const MAX_GRID: WinSize = WinSize {
    ws_col: 400,
    ws_row: 150,
};

/// The cache geometry of the machine this runs on, from the per-arch probe:
/// `UNKNOWN` where there is no probe, which still resolves to a pitch.
pub fn host_geometry() -> CacheGeometry {
    #[cfg(target_arch = "aarch64")]
    {
        <baud_isa_aarch64::Host as baud_isa::CacheProbe>::detect()
    }
    #[cfg(target_arch = "x86_64")]
    {
        <baud_isa_x86_64::Host as baud_isa::CacheProbe>::detect()
    }
    #[cfg(not(any(target_arch = "aarch64", target_arch = "x86_64")))]
    {
        CacheGeometry::UNKNOWN
    }
}

/// One line for the log: what the probe found, level by level, and what
/// was assumed where it found nothing.
pub fn describe(geom: &CacheGeometry) -> String {
    let level = |l: Option<CacheLevel>| match l {
        Some(l) => match l.way() {
            Some(w) => format!(
                "{} KiB {}-way, {} KiB a way",
                l.size.bytes() / 1024,
                l.ways.count(),
                w.bytes() / 1024
            ),
            None => format!("{} KiB", l.size.bytes() / 1024),
        },
        None => "unknown".to_string(),
    };
    let line = match geom.line_reported() {
        Some(l) => format!("{} B", l.bytes()),
        None => format!("unknown, {} B assumed", geom.line().bytes()),
    };
    format!(
        "line {line}; L1d {}; L2 {}; L3 {}",
        level(geom.l1d()),
        level(geom.l2()),
        level(geom.l3())
    )
}

/// Every face the config names, mapped and parsed. Each face's bytes are
/// a read-only mapping for the life of the process, because the faces
/// borrow them and a read of the whole file was the launch's largest
/// controllable cost; a face that cannot be mapped or parsed is fatal
/// with its path, because a terminal with no font draws nothing.
pub fn load_faces(cfg: &Config) -> &'static [Face<'static>] {
    let data: Vec<&'static [u8]> = cfg
        .faces
        .iter()
        .map(|f| {
            let bytes = crate::pty::map_readonly(&f.path)
                .unwrap_or_else(|e| panic!("font {}: {e}", f.path));
            startup_mark(&format!("map {}", f.path.rsplit('/').next().unwrap_or("")));
            bytes
        })
        .collect();
    Box::leak(
        data.iter()
            .zip(&cfg.faces)
            .map(|(d, f)| {
                let face = Face::parse_index(d, FaceIndex::of(f.index))
                    .unwrap_or_else(|_| panic!("font {}: not a face", f.path));
                startup_mark(&format!(
                    "parse {}",
                    f.path.rsplit('/').next().unwrap_or("")
                ));
                face
            })
            .collect::<Vec<_>>()
            .into_boxed_slice(),
    )
}

/// The launch clock: `startup_begin` is the first thing `main` does, and
/// `startup_mark` prints how long after it a stage was reached -- when
/// `BAUD_STARTUP` is set, so a normal launch prints nothing. What it cannot
/// see is the time before `main`, the loader's; `scripts/startup-bench.py`
/// measures that from outside and adds the two.
static START: OnceLock<Instant> = OnceLock::new();
static WANTED: OnceLock<Wanted> = OnceLock::new();

#[derive(Clone, Copy, PartialEq, Eq)]
enum Wanted {
    Print,
    Quiet,
}

pub fn startup_begin() {
    let _ = START.set(Instant::now());
    let _ = WANTED.set(match std::env::var_os("BAUD_STARTUP") {
        Some(_) => Wanted::Print,
        None => Wanted::Quiet,
    });
}

pub fn startup_mark(stage: &str) {
    if WANTED.get() != Some(&Wanted::Print) {
        return;
    }
    let since = START.get().map_or(Duration::ZERO, Instant::elapsed);
    eprintln!("startup: {stage:<18} {:9.3} ms", since.as_secs_f64() * 1e3);
}

/// A stage that happens many times and is worth printing once: the first
/// frame committed, the first completion, the first present.
pub fn startup_first(stage: &str, seen: &AtomicBool) {
    if !seen.swap(true, Ordering::Relaxed) {
        startup_mark(stage);
    }
}

/// A free-form line under the same flag, for a count the stages cannot
/// carry -- the first frame's map misses, a predicted inset that missed.
#[cfg(target_os = "macos")]
pub fn startup_note(text: &str) {
    if WANTED.get() != Some(&Wanted::Print) {
        return;
    }
    eprintln!("startup: {text}");
}

#[cfg(target_os = "macos")]
pub static FIRST_COMMIT: AtomicBool = AtomicBool::new(false);
/// The first bytes the child wrote, and the first frame committed after
/// them: what a user waits for, where `first commit` is an empty screen.
#[cfg(target_os = "macos")]
pub static FIRST_BYTES: AtomicBool = AtomicBool::new(false);
#[cfg(target_os = "macos")]
pub static FIRST_CONTENT: AtomicBool = AtomicBool::new(false);
#[cfg(target_os = "macos")]
pub static FIRST_DONE: AtomicBool = AtomicBool::new(false);
#[cfg(target_os = "macos")]
pub static FIRST_TICK: AtomicBool = AtomicBool::new(false);
/// The first display-link tick returned: what the tick's own work cost
/// the main thread, which is where the Dock icon used to be set.
#[cfg(target_os = "macos")]
pub static FIRST_TICK_DONE: AtomicBool = AtomicBool::new(false);
pub static FIRST_PRESENT: AtomicBool = AtomicBool::new(false);
