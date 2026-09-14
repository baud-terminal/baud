//! A small leveled log, because baud owns its stack rather than pull a
//! logging crate. Five levels -- `err!`, `warn!`, `info!`, `debug!`,
//! `verbose!` -- gated by a threshold read once from `BAUD_LOG`
//! (`err`|`warn`|`info`|`debug`|`verbose`). The default is `warn`, so the
//! terminal baud was launched from is quiet: a resize rebuilds the
//! presenter on every step, and printing each rebuild flooded it. Errors
//! and warnings show; the diagnostics and the per-frame report are `info`
//! and below, opt-in.

use std::sync::atomic::{AtomicU8, Ordering};

/// A message's importance, most severe first. `enabled` gates on it.
#[derive(Clone, Copy)]
pub enum Level {
    Err,
    Warn,
    Info,
    Debug,
    Verbose,
}

/// The rank of a level, lower is more severe: a message shows when its rank
/// is at or below the threshold. Inlined as a match at each use so there is
/// no cast and no raw-primitive return for the audit to count.
static THRESHOLD: AtomicU8 = AtomicU8::new(1);

/// Read `BAUD_LOG` once. Every arm names a level, so every variant is
/// constructed and none reads as dead. An unknown value keeps the default.
pub fn init() {
    let level = match std::env::var("BAUD_LOG").ok().as_deref() {
        Some("err") => Level::Err,
        Some("warn") => Level::Warn,
        Some("info") => Level::Info,
        Some("debug") => Level::Debug,
        Some("verbose") => Level::Verbose,
        _ => Level::Warn,
    };
    let threshold = match level {
        Level::Err => 0u8,
        Level::Warn => 1,
        Level::Info => 2,
        Level::Debug => 3,
        Level::Verbose => 4,
    };
    THRESHOLD.store(threshold, Ordering::Relaxed);
}

/// Whether a message at `level` is written. A macro reads it, so the
/// arguments are not formatted when the message would be dropped.
pub fn enabled(level: Level) -> bool {
    let r = match level {
        Level::Err => 0u8,
        Level::Warn => 1,
        Level::Info => 2,
        Level::Debug => 3,
        Level::Verbose => 4,
    };
    r <= THRESHOLD.load(Ordering::Relaxed)
}

#[macro_export]
macro_rules! err {
    ($($arg:tt)*) => { if $crate::log::enabled($crate::log::Level::Err) { eprintln!($($arg)*); } };
}

#[macro_export]
macro_rules! warn {
    ($($arg:tt)*) => { if $crate::log::enabled($crate::log::Level::Warn) { eprintln!($($arg)*); } };
}

#[macro_export]
macro_rules! info {
    ($($arg:tt)*) => { if $crate::log::enabled($crate::log::Level::Info) { eprintln!($($arg)*); } };
}

#[macro_export]
macro_rules! debug {
    ($($arg:tt)*) => { if $crate::log::enabled($crate::log::Level::Debug) { eprintln!($($arg)*); } };
}

#[macro_export]
macro_rules! verbose {
    ($($arg:tt)*) => { if $crate::log::enabled($crate::log::Level::Verbose) { eprintln!($($arg)*); } };
}
