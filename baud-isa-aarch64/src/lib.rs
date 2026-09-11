//! aarch64 scan implementations.
//!
//! **What this crate is for.** `Portable` lowers to NEON already, so the
//! question is never "does baud use NEON" but "does it use the *right* NEON".
//! `[[source: arm-neon-bitmask]]` argues that `shrn` is the cheap way to get a
//! lane mask out of a compare result, and that most libraries have not taken
//! it up. Disassembling `feed_with::<Portable>` on an Apple box confirms baud
//! had not either: `core::simd`'s `to_bitmask` lowers to `uzp1`, `zip1`,
//! `and.16b` and **`addv.8h`** -- a horizontal reduction, which carries a
//! serial dependency `shrn` does not.
//!
//! So this is the same three scans with one instruction changed, and it is a
//! measurement rather than a claim: `Neon` and `Portable` are both compiled,
//! and a NEON property required them to return the same
//! answer for every generated program, at 20,000 cases.

#![no_std]
#![cfg(target_arch = "aarch64")]

use baud_domain::*;
use baud_isa::{Decode, DecodedBlock, LANES, Portable, Scan};

/// The aarch64 scanner: `shrn` instead of a horizontal reduction.
///
/// NEON is baseline on aarch64 -- there is no feature to detect and no
/// fallback to pick -- which is why this crate has no runtime dispatch and
/// `Scan`'s safety contract is satisfied by the target alone.
pub struct Neon;

/// The mask of lanes where `v` compares unequal to zero, in nibble encoding.
///
/// `vshrn_n_u16(x, 4)` reads the sixteen compare bytes as eight halfwords,
/// shifts each right by four and truncates to a byte, so each output byte
/// carries the low nibble of one input byte and the high nibble of the next.
/// A lane matched iff its nibble is non-zero.
#[inline(always)]
unsafe fn nibbles(v: core::arch::aarch64::uint8x16_t) -> NibbleMask {
    use core::arch::aarch64::*;
    unsafe {
        let narrowed = vshrn_n_u16(vreinterpretq_u16_u8(v), 4);
        NibbleMask::from_shrn(vget_lane_u64(vreinterpret_u64_u8(narrowed), 0))
    }
}

impl Scan for Neon {
    /// # Safety
    /// aarch64 only, where NEON is baseline.
    #[inline(always)]
    unsafe fn run(buf: &[StreamByte]) -> ByteLen {
        use core::arch::aarch64::*;
        let raw = StreamByte::slice_as_bytes(buf);
        let n = raw.len();
        let mut i = 0usize;
        unsafe {
            // Printable is `0x20..=0x7e`: subtract the low bound and one
            // unsigned compare answers it, the same trick the portable form
            // uses and for the same reason.
            let lo = vdupq_n_u8(0x20);
            let span = vdupq_n_u8(0x7e - 0x20);
            while i + LANES <= n {
                let v = vld1q_u8(raw.as_ptr().add(i));
                let m = nibbles(vcgtq_u8(vsubq_u8(v, lo), span));
                if let Some(lane) = m.first() {
                    return lane.offset_from(i);
                }
                i += LANES;
            }
        }
        while i < n && buf[i].is_printable() {
            i += 1;
        }
        ByteLen::from_lane(i)
    }

    /// **The scan this crate was missing, and the one the corpus pays for.**
    ///
    /// Same predicate as `Vectorized::stride_three_run` -- lanes 0, 3, 6, 9 and
    /// 12 must be three-byte leads and every other lane a continuation -- and
    /// one instruction different. The portable form builds two masks, combines
    /// them with `&`/`|`/`!` and ends in `to_bitmask()`, which lowers to a
    /// horizontal `addv.8h`; here the combine is a single `vbslq_u8` -- bit
    /// select is exactly `(a & m) | (b & !m)` -- and the extraction is `shrn`,
    /// which has no reduction and no serial dependency.
    ///
    /// # Safety
    /// aarch64 only, where NEON is baseline.
    #[inline(always)]
    unsafe fn stride_three(buf: &[StreamByte]) -> Count {
        use core::arch::aarch64::*;
        let raw = StreamByte::slice_as_bytes(buf);
        if raw.len() < LANES {
            return Count::ZERO;
        }
        unsafe {
            let v = vld1q_u8(raw.as_ptr());
            let is_lead = vceqq_u8(vandq_u8(v, vdupq_n_u8(0xf0)), vdupq_n_u8(0xe0));
            let is_cont = vceqq_u8(vandq_u8(v, vdupq_n_u8(0xc0)), vdupq_n_u8(0x80));
            // `0xff` at every lane a lead belongs in. A constant array rather
            // than arithmetic on the lane index: the pattern is the stride and
            // spelling it out is what makes a wrong one visible.
            const AT_LEAD: [u8; 16] = [
                0xff, 0, 0, 0xff, 0, 0, 0xff, 0, 0, 0xff, 0, 0, 0xff, 0, 0, 0,
            ];
            let at_lead = vld1q_u8(AT_LEAD.as_ptr());
            // `(is_lead & at_lead) | (is_cont & !at_lead)`, in one instruction.
            let ok = vbslq_u8(at_lead, is_lead, is_cont);
            // Three lanes to a character, and five is what the register holds.
            Count::of((nibbles(ok).leading_run() / 3).min(5))
        }
    }

    /// # Safety
    /// aarch64 only, where NEON is baseline.
    #[inline(always)]
    unsafe fn final_byte(buf: &[StreamByte]) -> Option<ByteLen> {
        use core::arch::aarch64::*;
        let raw = StreamByte::slice_as_bytes(buf);
        let n = raw.len();
        let mut i = 0usize;
        unsafe {
            let lo = vdupq_n_u8(0x40);
            let hi = vdupq_n_u8(0x7e);
            while i + LANES <= n {
                let v = vld1q_u8(raw.as_ptr().add(i));
                let m = nibbles(vandq_u8(vcgeq_u8(v, lo), vcleq_u8(v, hi)));
                if let Some(lane) = m.first() {
                    return Some(lane.offset_from(i));
                }
                i += LANES;
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

    /// # Safety
    /// aarch64 only, where NEON is baseline.
    #[inline(always)]
    unsafe fn st(buf: &[StreamByte]) -> Option<ByteLen> {
        use core::arch::aarch64::*;
        let raw = StreamByte::slice_as_bytes(buf);
        let n = raw.len();
        let mut i = 0usize;
        unsafe {
            let bel = vdupq_n_u8(0x07);
            let esc = vdupq_n_u8(0x1b);
            while i + LANES <= n {
                let v = vld1q_u8(raw.as_ptr().add(i));
                let m = nibbles(vorrq_u8(vceqq_u8(v, bel), vceqq_u8(v, esc)));
                if let Some(lane) = m.first() {
                    return Some(lane.offset_from(i));
                }
                i += LANES;
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
}

impl Decode for Neon {
    /// # Safety
    /// Same contract as `Scan` for this type.
    ///
    /// **Delegates to `Portable` for now, deliberately.** The trait is the
    /// seam; a aarch64 gather can land behind it without the caller changing,
    /// and shipping the delegation first means the block decoder is exercised
    /// on this target from the day it exists rather than after a second
    /// implementation is written.
    #[inline(always)]
    unsafe fn block(buf: &[StreamByte]) -> DecodedBlock {
        // SAFETY: `Portable` requires no target feature.
        unsafe { Portable::block(buf) }
    }
}
