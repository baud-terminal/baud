use crate::*;

mod avx2;
mod portable;

pub(crate) use portable::*;
// Gated to match the definitions in `avx2.rs`: an unconditional re-export of a
// `#[cfg(target_arch = "x86_64")]` item resolves to nothing elsewhere, and the
// host build never notices because the cfg is off there too. The bare-metal
// `x86_64-unknown-none` gate is what catches it.
#[cfg(target_arch = "x86_64")]
pub(crate) use avx2::*;

/// A scan backend.
///
/// The three scans below are the *only* ISA-dependent code in the parser. Every
/// fast path is written once against this trait and monomorphised per backend,
/// so the portable and accelerated feeds cannot diverge in logic — which is the
/// bug this codebase has already had once, when `ESC 7`/`8`/`M` were
/// implemented in the AVX2 path only and `feed` silently disagreed with
/// `feed_avx2`.
///
/// # Safety
/// An implementation may require a target feature. The caller establishes it
/// once, at startup; `no_std` has no runtime feature detection.
pub(crate) trait Scan {
    /// Length of the leading printable-ASCII run.
    unsafe fn run(buf: &[u8]) -> ByteLen;
    /// Offset of the first CSI final byte (`0x40..=0x7e`).
    unsafe fn final_byte(buf: &[u8]) -> Option<ByteLen>;
    /// Offset of the first BEL, or the ESC of an ESC-backslash terminator.
    unsafe fn st(buf: &[u8]) -> Option<ByteLen>;
}

/// Lane width. 16 bytes is what every SIMD ISA this targets has natively;
/// asking `core::simd` for more emits multiple registers per step and measured
/// no better.
///
/// **Re-tested for `run` specifically, which was scalar until it was
/// vectorised and so was never covered by the sentence above.** 32 lanes
/// measured *worse* on the streaming payload -- 1313/1374/1338 against
/// 1342/1410/1396 -- and flat on btop. Two NEON registers per step buys
/// nothing when the run being scanned averages ~32 bytes, because the loop
/// exits in its first or second iteration either way.
///
/// **aarch64 was the platform that had been paying for the scalar loop.**
/// x86-64 with AVX2 has always had `scan_avx2`, 32 bytes a step; the scalar
/// `run` was in `Portable`, which is what aarch64 uses -- so the machine this
/// project is developed and measured on was the one running the slow path.
pub(crate) const LANES: usize = 16;
