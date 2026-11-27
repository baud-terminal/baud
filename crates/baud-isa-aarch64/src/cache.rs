//! The aarch64 cache probe.
//!
//! **The hardware will not say, so the kernel has to.** `CCSIDR_EL1`, which
//! carries a cache's sets, ways and line size, is an EL1 register and traps at
//! EL0 -- a user process reading it is killed. `CTR_EL0` is the one cache
//! register the architecture *lets* EL0 read, behind `SCTLR_EL1.UCT`, and it
//! was tried here as the `no_std` floor: **it traps on macOS too.** `mrs x,
//! ctr_el0` is `SIGILL` under XNU on this box, measured against a control
//! that read `cntvct_el0` through the same `asm!` and succeeded. A probe that
//! kills the process on the platform baud is developed on is not a floor.
//!
//! So the geometry comes from the operating system, behind the `std`
//! feature -- `sysctlbyname` on macOS, sysfs on Linux -- and without it the
//! probe answers [`CacheGeometry::UNKNOWN`], which resolves to the safe
//! fallback line rather than to a guess.

use baud_domain::CacheGeometry;
use baud_isa::CacheProbe;

/// This machine, as its kernel describes it.
pub struct Host;

impl CacheProbe for Host {
    fn detect() -> CacheGeometry {
        #[cfg(all(feature = "std", target_os = "macos"))]
        {
            let g = sysctl::detect();
            if g != CacheGeometry::UNKNOWN {
                return g;
            }
        }
        #[cfg(all(feature = "std", target_os = "linux"))]
        {
            let g = baud_isa::sysfs::detect();
            if g != CacheGeometry::UNKNOWN {
                return g;
            }
        }
        CacheGeometry::UNKNOWN
    }
}

/// macOS: `sysctlbyname`. Associativity is not in `sysctl` at all, so every
/// level is reported with `Ways::of(0)` -- "not reported" -- rather than a
/// guessed number.
#[cfg(all(feature = "std", target_os = "macos"))]
mod sysctl {
    extern crate std;

    use baud_domain::{CacheGeometry, CacheLevel, CacheSize, LineSize, Ways};
    use core::ffi::{CStr, c_char, c_int, c_void};

    /// C's `size_t`, which is `usize` on every macOS target: a byte count
    /// the kernel reads and writes through the call below. Transparent, so
    /// the ABI is the C one and the number is still named.
    #[repr(transparent)]
    struct SizeT(usize);

    // The one libc call, declared rather than depended on: `libc` would be
    // the only crate in any ISA crate, for a signature that is four lines of
    // XNU's `sys/sysctl.h`.
    unsafe extern "C" {
        fn sysctlbyname(
            name: *const c_char,
            oldp: *mut c_void,
            oldlenp: *mut SizeT,
            newp: *mut c_void,
            newlen: SizeT,
        ) -> c_int;
    }

    /// A number the kernel reported and nothing has typed yet. `sysctl`
    /// stores some of these as 32-bit and some as 64-bit integers, and the
    /// length it hands back says which.
    #[derive(Clone, Copy)]
    struct Reading(usize);

    impl Reading {
        fn of(name: &CStr) -> Option<Reading> {
            let mut buf = [0u8; 8];
            let mut len = SizeT(buf.len());
            // SAFETY: `name` is a NUL-terminated string, `buf` is `len` bytes
            // of writable memory and `len` is passed by pointer so the kernel
            // can report how many it used; no new value is set, so the last
            // two arguments are null and zero as the call permits.
            let rc = unsafe {
                sysctlbyname(
                    name.as_ptr(),
                    buf.as_mut_ptr().cast(),
                    &mut len,
                    core::ptr::null_mut(),
                    SizeT(0),
                )
            };
            if rc != 0 {
                return None;
            }
            let value = match len.0 {
                4 => u64::from(u32::from_ne_bytes(buf[..4].try_into().ok()?)),
                8 => u64::from_ne_bytes(buf),
                _ => return None,
            };
            usize::try_from(value).ok().map(Reading)
        }

        fn line(self) -> LineSize {
            LineSize::of(self.0)
        }

        /// A level with the size sysctl gave and no associativity.
        fn level(self) -> CacheLevel {
            CacheLevel::new(CacheSize::of(self.0), Ways::of(0))
        }
    }

    /// `perflevel0` is the performance cluster on a machine that has more
    /// than one kind of core; the unprefixed name is the smallest cache of
    /// any core, which is not the one the process that asks is running on.
    /// The prefixed key is absent on an Intel Mac, so each has its fallback.
    pub fn detect() -> CacheGeometry {
        let line = Reading::of(c"hw.cachelinesize").map(Reading::line);
        let l1d = Reading::of(c"hw.perflevel0.l1dcachesize")
            .or_else(|| Reading::of(c"hw.l1dcachesize"))
            .map(Reading::level);
        let l2 = Reading::of(c"hw.perflevel0.l2cachesize")
            .or_else(|| Reading::of(c"hw.l2cachesize"))
            .map(Reading::level);
        // Apple silicon has no L3 and no such key; an Intel Mac has both.
        let l3 = Reading::of(c"hw.l3cachesize").map(Reading::level);
        CacheGeometry::new(line, l1d, l2, l3)
    }
}
