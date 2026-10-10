//! A small leveled log, because baud owns its stack rather than pull a
//! logging crate. Five levels -- `err!`, `warn!`, `info!`, `debug!`,
//! `verbose!` -- gated by a threshold read once from `BAUD_LOG`
//! (`err`|`warn`|`info`|`debug`|`verbose`). The default is `warn`, so the
//! terminal baud was launched from is quiet: a resize rebuilds the
//! presenter on every step, and printing each rebuild flooded it. Errors
//! and warnings show; the diagnostics and the per-frame report are `info`
//! and below, opt-in.

use std::sync::atomic::{AtomicU8, Ordering};

/// A message's importance, most severe first. The derived order is the
/// gate's: a message shows when its level sorts at or before the
/// threshold's, so the order of the variants is the policy.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Level {
    Err,
    Warn,
    Info,
    Debug,
    Verbose,
}

/// The threshold, stored as its rank, lower more severe: `Err` 0 to
/// `Verbose` 4. Written as a match at each end so there is no cast and no
/// raw-primitive return for the audit to count. Its value before `init` is
/// `warn`'s, the same default `named` gives an absent `BAUD_LOG`.
static THRESHOLD: AtomicU8 = AtomicU8::new(1);

/// The level a `BAUD_LOG` value names. Every arm names a level, so every
/// variant is constructed and none reads as dead. An unknown value, or
/// none, is the default, `warn`. Apart from `init` so a test can read the
/// default without the process's environment.
pub fn named(value: Option<&str>) -> Level {
    match value {
        Some("err") => Level::Err,
        Some("warn") => Level::Warn,
        Some("info") => Level::Info,
        Some("debug") => Level::Debug,
        Some("verbose") => Level::Verbose,
        _ => Level::Warn,
    }
}

/// Read `BAUD_LOG` once.
pub fn init() {
    let threshold = match named(std::env::var("BAUD_LOG").ok().as_deref()) {
        Level::Err => 0u8,
        Level::Warn => 1,
        Level::Info => 2,
        Level::Debug => 3,
        Level::Verbose => 4,
    };
    THRESHOLD.store(threshold, Ordering::Relaxed);
}

/// Whether a message at `level` is written under `threshold`: the gate
/// itself, apart from the stored threshold so a test can ask it.
pub fn shows(level: Level, threshold: Level) -> bool {
    level <= threshold
}

/// Whether a message at `level` is written. A macro reads it, so the
/// arguments are not formatted when the message would be dropped.
pub fn enabled(level: Level) -> bool {
    let threshold = match THRESHOLD.load(Ordering::Relaxed) {
        0 => Level::Err,
        1 => Level::Warn,
        2 => Level::Info,
        3 => Level::Debug,
        _ => Level::Verbose,
    };
    shows(level, threshold)
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

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: [Level; 5] = [
        Level::Err,
        Level::Warn,
        Level::Info,
        Level::Debug,
        Level::Verbose,
    ];

    /// **The default is `warn`, and each name is its level.** An absent
    /// `BAUD_LOG` and one baud does not know both fall back to it.
    #[test]
    fn baud_log_names_its_levels_and_defaults_to_warn() {
        assert_eq!(named(None), Level::Warn);
        assert_eq!(named(Some("loud")), Level::Warn);
        assert_eq!(named(Some("")), Level::Warn);
        for (name, level) in ["err", "warn", "info", "debug", "verbose"]
            .into_iter()
            .zip(ALL)
        {
            assert_eq!(named(Some(name)), level, "{name}");
        }
    }

    /// **A level shows at its own threshold and every quieter one**, and
    /// not under a more severe threshold: at the default, errors and
    /// warnings and nothing after them.
    #[test]
    fn a_level_shows_at_and_below_the_threshold() {
        for (i, &level) in ALL.iter().enumerate() {
            for (j, &threshold) in ALL.iter().enumerate() {
                assert_eq!(shows(level, threshold), i <= j, "{level:?} {threshold:?}");
            }
        }
        let at_default = named(None);
        assert!(shows(Level::Err, at_default) && shows(Level::Warn, at_default));
        assert!(!shows(Level::Info, at_default) && !shows(Level::Verbose, at_default));
    }

    /// **The threshold before `init` is the default `init` falls back to.**
    /// No test calls `init`, which reads the process's environment, so this
    /// reads the static's own value: two spellings of one default, held
    /// together.
    #[test]
    fn the_threshold_before_init_is_the_default() {
        for level in ALL {
            assert_eq!(enabled(level), shows(level, named(None)), "{level:?}");
        }
    }
}
