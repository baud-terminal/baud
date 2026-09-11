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
