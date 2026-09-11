//! x86-64 scan implementations: AVX2, 32 bytes a step.
//!
//! Split from `baud-isa` so a consumer on another architecture never builds
//! it. The only `unsafe` in baud's scan path lives here, and
//! An AVX2 property required this to agree with the portable
//! form for every generated input.

#![no_std]
#![cfg(target_arch = "x86_64")]

use baud_domain::*;
use baud_isa::{Decode, DecodedBlock, Portable, Scan};

/// 32-byte lanes on x86-64. Requires AVX2.
pub struct Avx2;

/// **The safe surface, and why it is `cfg`'d where NEON's is not.**
///
/// `Neon` can expose these unconditionally: NEON is baseline on aarch64, so the
/// obligation is discharged by the target and there is nothing a caller can get
/// wrong. **AVX2 is not baseline on x86-64.** A caller really can invoke it on a
/// machine that cannot execute it, `no_std` has no `is_x86_feature_detected!`,
/// and the result is UB rather than a slow path -- so the `unsafe fn` on the
/// trait is a real contract here and must not be wrapped away in general.
///
/// `#[cfg(target_feature = "avx2")]` is the one case where it *is* discharged
/// the same way: the build itself targets AVX2, so it is baseline **for this
/// binary** exactly as NEON is for an aarch64 one. Reached with
/// `-C target-feature=+avx2` or a `target-cpu` that implies it; absent
/// otherwise, so a default x86-64 build cannot call these at all.
#[cfg(target_feature = "avx2")]
impl Avx2 {
    /// Length of the leading printable-ASCII run, 32 bytes a step.
    #[inline(always)]
    pub fn run(buf: &[StreamByte]) -> ByteLen {
        // SAFETY: this block is `cfg`'d to a build that targets AVX2, so the
        // feature the contract asks for is guaranteed by the compilation.
        unsafe { <Avx2 as Scan>::run(buf) }
    }
    /// Offset of the first `CSI` final byte.
    #[inline(always)]
    pub fn final_byte(buf: &[StreamByte]) -> Option<ByteLen> {
        // SAFETY: as `Avx2::run`.
        unsafe { <Avx2 as Scan>::final_byte(buf) }
    }
    /// Offset of the first `BEL`, or the `ESC` of an `ESC \` terminator.
    #[inline(always)]
    pub fn st(buf: &[StreamByte]) -> Option<ByteLen> {
        // SAFETY: as `Avx2::run`.
        unsafe { <Avx2 as Scan>::st(buf) }
    }
}

impl Scan for Avx2 {
    unsafe fn run(buf: &[StreamByte]) -> ByteLen {
        unsafe { scan_avx2(buf) }
    }
    unsafe fn final_byte(buf: &[StreamByte]) -> Option<ByteLen> {
        unsafe { find_final(buf) }
    }
    unsafe fn st(buf: &[StreamByte]) -> Option<ByteLen> {
        unsafe { find_st(buf) }
    }
    /// **Delegated to `Portable`, and that is the answer rather than a stub.**
    ///
    /// The contract caps this at five characters, because `baud-core`'s caller
    /// steps in `LANE_COUNT` = 16-byte blocks and fifteen bytes is five
    /// three-byte characters. A 256-bit register has **nothing to contribute to
    /// a fifteen-byte question** -- it would have to be given a wider block and
    /// a wider cap by the caller before width could pay, and that is a change to
    /// `run_of`'s arithmetic and `STRIDE_MAX`, not to this crate.
    ///
    /// So this is not "not done yet". Wiring `_mm256` in here would process
    /// thirty-two bytes and then throw seventeen of them away.
    ///
    /// # Safety
    /// `Portable` requires no target feature, so this arm requires none either.
    unsafe fn stride_three(buf: &[StreamByte]) -> Count {
        // SAFETY: `Portable` has no target-feature obligation.
        unsafe { <Portable as Scan>::stride_three(buf) }
    }
    /// Delegated to `Portable`, for the reason [`Avx2::stride_three`] gives:
    /// the cap is four characters in a sixteen-byte block.
    ///
    /// # Safety
    /// As [`Avx2::stride_three`].
    unsafe fn stride_four(buf: &[StreamByte]) -> Count {
        // SAFETY: `Portable` has no target-feature obligation.
        unsafe { <Portable as Scan>::stride_four(buf) }
    }
}

/// Length of the leading printable-ASCII run, 32 bytes per step.
///
/// # Safety
/// AVX2 must be available. `no_std` has no runtime feature detection, so
/// the caller decides once at startup — see [`Term::feed_avx2`].
#[target_feature(enable = "avx2")]
pub unsafe fn scan_avx2(buf: &[StreamByte]) -> ByteLen {
    use core::arch::x86_64::*;
    let (n, p) = (buf.len(), buf.as_ptr());
    let sp = _mm256_set1_epi8(0x20);
    let del = _mm256_set1_epi8(0x7f);
    let z = _mm256_setzero_si256();
    let mut i = 0usize;
    unsafe {
        while i + 32 <= n {
            let v = _mm256_loadu_si256(p.add(i) as *const __m256i);
            let le = _mm256_cmpeq_epi8(_mm256_min_epu8(v, sp), v); // <= 0x20
            let eq = _mm256_cmpeq_epi8(v, sp);
            let lt = _mm256_andnot_si256(eq, le); // < 0x20
            let hi = _mm256_cmpgt_epi8(z, v); // >= 0x80, signed compare
            // 0x7f (DEL) is not printable and was missing from this mask, so
            // the vector body accepted it into a run while the scalar tail
            // below — which uses is_printable() — rejected it. Any input
            // containing a DEL byte then rendered differently depending on
            // whether it landed in a 32-byte block or the tail.
            let d = _mm256_cmpeq_epi8(v, del);
            let m = LaneMask::from_movemask(_mm256_movemask_epi8(_mm256_or_si256(
                _mm256_or_si256(lt, hi),
                d,
            )) as u32);
            if let Some(lane) = m.first() {
                return lane.offset_from(i);
            }
            i += 32;
        }
    }
    while i < n && buf[i].is_printable() {
        i += 1;
    }
    ByteLen::from_lane(i)
}

/// Position of the first CSI final byte (`0x40..=0x7e`), 32 at a time.
///
/// # Safety
/// AVX2 must be available.
#[target_feature(enable = "avx2")]
pub unsafe fn find_final(buf: &[StreamByte]) -> Option<ByteLen> {
    use core::arch::x86_64::*;
    let (n, p) = (buf.len(), buf.as_ptr());
    let lo = _mm256_set1_epi8(0x3f);
    let hi = _mm256_set1_epi8(0x7f);
    let mut i = 0usize;
    unsafe {
        while i + 32 <= n {
            let v = _mm256_loadu_si256(p.add(i) as *const __m256i);
            let m = LaneMask::from_movemask(_mm256_movemask_epi8(_mm256_and_si256(
                _mm256_cmpgt_epi8(v, lo),
                _mm256_cmpgt_epi8(hi, v),
            )) as u32);
            if let Some(lane) = m.first() {
                return Some(lane.offset_from(i));
            }
            i += 32;
        }
    }
    while i < n {
        if FinalByte::from_stream(buf[i]).is_some() {
            return Some(ByteLen::from_lane(i));
        }
        i += 1;
    }
    None
}

/// First BEL, or the ESC of an ESC-backslash string terminator.
///
/// # Safety
/// AVX2 must be available.
#[target_feature(enable = "avx2")]
pub unsafe fn find_st(buf: &[StreamByte]) -> Option<ByteLen> {
    use core::arch::x86_64::*;
    let (n, p) = (buf.len(), buf.as_ptr());
    let bel = _mm256_set1_epi8(0x07);
    let esc = _mm256_set1_epi8(0x1b);
    let mut i = 0usize;
    unsafe {
        while i + 32 <= n {
            let v = _mm256_loadu_si256(p.add(i) as *const __m256i);
            let m = LaneMask::from_movemask(_mm256_movemask_epi8(_mm256_or_si256(
                _mm256_cmpeq_epi8(v, bel),
                _mm256_cmpeq_epi8(v, esc),
            )) as u32);
            if let Some(lane) = m.first() {
                return Some(lane.offset_from(i));
            }
            i += 32;
        }
    }
    while i < n {
        let b = buf[i];
        if b == StreamByte::BEL || b == StreamByte::ESC {
            return Some(ByteLen::from_lane(i));
        }
        i += 1;
    }
    None
}

impl Decode for Avx2 {
    /// # Safety
    /// Same contract as `Scan` for this type.
    ///
    /// **Delegates to `Portable` for now, deliberately.** The trait is the
    /// seam; a x86-64 gather can land behind it without the caller changing,
    /// and shipping the delegation first means the block decoder is exercised
    /// on this target from the day it exists rather than after a second
    /// implementation is written.
    #[inline(always)]
    unsafe fn block(buf: &[StreamByte]) -> DecodedBlock {
        // SAFETY: `Portable` requires no target feature.
        unsafe { Portable::block(buf) }
    }
}
