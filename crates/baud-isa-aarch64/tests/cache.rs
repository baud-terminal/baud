//! **The probe has to answer something a real cache could have, and on this
//! box it has to answer what `sysctl` answers.**
//!
//! The pinned values are this development machine's -- `hw.cachelinesize`
//! 128, `hw.perflevel0.l1dcachesize` 131072, `hw.perflevel0.l2cachesize`
//! 16777216 -- and they are asserted twice: against those constants, and
//! against the `sysctl` command, which reaches the same kernel table through
//! a different door than `sysctlbyname`. The second form is what holds on a
//! different Mac; the first is what says this box has not changed.

#![cfg(target_arch = "aarch64")]

use baud_isa::CacheProbe;
use baud_isa_aarch64::Host;

#[test]
fn the_line_is_one_a_real_cache_uses() {
    let line = Host::detect().line().bytes();
    assert!([64, 128, 256].contains(&line), "line {line}");
}

/// Without an operating system to ask there is nothing EL0 may read --
/// `CTR_EL0` included, which is `SIGILL` under XNU -- so the honest answer
/// is unknown, and this pins that the probe says so rather than guessing.
#[cfg(not(feature = "std"))]
#[test]
fn without_std_the_probe_admits_it_knows_nothing() {
    use baud_domain::CacheGeometry;
    assert_eq!(Host::detect(), CacheGeometry::UNKNOWN);
}

#[cfg(all(feature = "std", target_os = "macos"))]
mod macos {
    use super::*;
    use std::process::Command;

    fn sysctl(key: &str) -> usize {
        let out = Command::new("sysctl")
            .arg("-n")
            .arg(key)
            .output()
            .expect("sysctl runs");
        String::from_utf8(out.stdout)
            .expect("utf-8")
            .trim()
            .parse()
            .expect("a number")
    }

    /// This box: an Apple part with a 128-byte line, 128 KiB of L1d on the
    /// performance cluster, 16 MiB of L2 and no L3. Associativity is not in
    /// `sysctl`, so the ways are reported as unknown rather than invented.
    #[test]
    fn this_machine_reads_as_its_sysctl_says() {
        let geom = Host::detect();
        assert_eq!(geom.line().bytes(), 128);
        let l1d = geom.l1d().expect("hw.perflevel0.l1dcachesize");
        let l2 = geom.l2().expect("hw.perflevel0.l2cachesize");
        assert_eq!(l1d.size.bytes(), 131_072);
        assert_eq!(l2.size.bytes(), 16_777_216);
        assert_eq!(l1d.ways.count(), 0, "sysctl carries no associativity");
        assert_eq!(l1d.way(), None);
        assert_eq!(geom.l3(), None, "Apple silicon has no L3");
    }

    /// The same three numbers through the `sysctl` command, so a different
    /// Mac holds too.
    #[test]
    fn the_probe_agrees_with_the_sysctl_command() {
        let geom = Host::detect();
        assert_eq!(geom.line().bytes(), sysctl("hw.cachelinesize"));
        assert_eq!(
            geom.l1d().unwrap().size.bytes(),
            sysctl("hw.perflevel0.l1dcachesize")
        );
        assert_eq!(
            geom.l2().unwrap().size.bytes(),
            sysctl("hw.perflevel0.l2cachesize")
        );
    }
}
