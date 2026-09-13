//! Shared by the bench binaries: what the host is, asked once.
//!
//! `baud-core` never probes hardware. A bench, like the app, asks the
//! per-arch crate and hands the answer to the storage -- one function, so
//! every binary lays its grid out at the pitch the app would, which is what
//! makes an `ingest2` figure a figure about the shipping layout rather than
//! about a bench's own.

use baud_domain::CacheGeometry;

/// The cache hierarchy of this machine, or `UNKNOWN` where the probe cannot
/// ask -- which still resolves to a safe pitch.
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
