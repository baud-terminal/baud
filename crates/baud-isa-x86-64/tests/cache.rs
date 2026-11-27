//! **The probe has to answer something a real cache could have.** `CPUID`
//! leaf 4 is present on every x86-64 part this can run on, so a geometry
//! with no L1 data cache in it is the walk being wrong rather than the
//! machine being exotic.
//!
//! Written on an aarch64 box that cannot compile this file; an x86-64 host
//! is the first machine it runs on (`devenv tasks run test:x86-avx2` runs
//! it), and a red first run there is the test working.

#![cfg(target_arch = "x86_64")]

use baud_isa::CacheProbe;
use baud_isa_x86_64::Host;

#[test]
fn the_line_is_one_a_real_cache_uses() {
    let line = Host::detect().line().bytes();
    assert!([64, 128, 256].contains(&line), "line {line}");
}

/// Every x86-64 part has a level-one data cache of at least 16 KiB, and the
/// leaf reports its associativity, so both are known rather than fallen
/// back on.
#[test]
fn cpuid_reports_the_first_level_with_its_ways() {
    let geom = Host::detect();
    let l1d = geom.l1d().expect("no L1d from CPUID leaf 4 or 0x8000001D");
    assert!(
        l1d.size.bytes() >= 16 * 1024,
        "L1d {} bytes",
        l1d.size.bytes()
    );
    assert!(l1d.ways.count() >= 2, "L1d ways {}", l1d.ways.count());
    let way = l1d.way().expect("ways reported, so the way size resolves");
    assert_eq!(way.bytes() % geom.line().bytes(), 0, "a way is whole lines");
}
