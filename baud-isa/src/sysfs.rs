//! Cache geometry from Linux's sysfs, for the targets that cannot ask the
//! hardware directly.
//!
//! `/sys/devices/system/cpu/cpu0/cache/index*/` carries one directory per
//! cache cpu0 sees, each with `level`, `type`, `coherency_line_size`, `size`
//! and `ways_of_associativity` as text. This reads them for cpu0 only: the
//! stride is derived once for the process and every core of one part shares
//! a line size, which is the field that matters.
//!
//! Shared by both per-arch crates through `baud-isa`'s `std` feature so the
//! parser is spelled once. On aarch64 it is the primary source, because
//! `CCSIDR_EL1` traps at EL0; on x86-64 it is the fallback for a `CPUID`
//! leaf 4 that a hypervisor has blanked.
//!
//! It compiles on every `std` host and not only Linux, because the layout is
//! a directory tree and [`detect_at`] takes the root: on a host without
//! `/sys` the walk finds no `index0` and answers unknown, and a test can
//! hand it a tree it wrote itself -- which is how the parser is checked on
//! the macOS box this was written on.

extern crate std;

use baud_domain::{CacheGeometry, CacheLevel, CacheSize, LineSize, Ways};
use std::format;
use std::fs;
use std::path::Path;
use std::string::String;

/// A number sysfs reported and nothing has typed yet. It becomes a line, a
/// size or a way count at the one place its file name says which.
#[derive(Clone, Copy)]
struct Reading(usize);

impl Reading {
    /// `size` files carry a `K` or `M` suffix; the others are plain decimal.
    fn parse(text: &str) -> Option<Reading> {
        let text = text.trim();
        let (digits, scale) = match text.strip_suffix('M') {
            Some(d) => (d, 1024 * 1024),
            None => match text.strip_suffix('K') {
                Some(d) => (d, 1024),
                None => (text, 1),
            },
        };
        digits
            .parse::<usize>()
            .ok()
            .and_then(|n| n.checked_mul(scale))
            .map(Reading)
    }

    fn line(self) -> LineSize {
        LineSize::of(self.0)
    }

    fn size(self) -> CacheSize {
        CacheSize::of(self.0)
    }

    fn ways(self) -> Ways {
        Ways::of(self.0)
    }
}

/// The first eight index directories are more than any Linux host exposes
/// for one core; the walk stops at the first that does not exist anyway.
const MAX_INDEX: usize = 8;

/// Where Linux mounts it. The `cpu0` in the path is deliberate: the stride is
/// derived once for the process, and every core of one part shares a line.
pub const SYSFS_CPU0_CACHE: &str = "/sys/devices/system/cpu/cpu0/cache";

fn read(dir: &Path, name: &str) -> Option<String> {
    fs::read_to_string(dir.join(name)).ok()
}

/// Walk cpu0's cache directories under [`SYSFS_CPU0_CACHE`].
pub fn detect() -> CacheGeometry {
    detect_at(Path::new(SYSFS_CPU0_CACHE))
}

/// Walk the `index*` directories under `root`. Returns
/// [`CacheGeometry::UNKNOWN`] when there is no `index0`, and partial data
/// on a tree that reports partially -- never an error, because construction
/// must not depend on it.
pub fn detect_at(root: &Path) -> CacheGeometry {
    let mut line = None;
    let mut l1d = None;
    let mut l2 = None;
    let mut l3 = None;
    for index in 0..MAX_INDEX {
        let dir = root.join(format!("index{index}"));
        let Some(level) = read(&dir, "level") else {
            break;
        };
        let kind = read(&dir, "type").unwrap_or_default();
        let kind = kind.trim();
        if kind != "Data" && kind != "Unified" {
            continue;
        }
        let entry = read(&dir, "size")
            .and_then(|t| Reading::parse(&t))
            .map(|size| {
                // Unknown associativity is `Ways::of(0)`, which `CacheLevel::way`
                // reads as "not reported" rather than as a divide by zero.
                let ways = read(&dir, "ways_of_associativity")
                    .and_then(|t| Reading::parse(&t))
                    .map_or(Ways::of(0), Reading::ways);
                CacheLevel::new(size.size(), ways)
            });
        match level.trim() {
            "1" => {
                l1d = entry;
                line = read(&dir, "coherency_line_size")
                    .and_then(|t| Reading::parse(&t))
                    .map(Reading::line);
            }
            "2" => l2 = entry,
            "3" => l3 = entry,
            _ => {}
        }
    }
    CacheGeometry::new(line, l1d, l2, l3)
}
