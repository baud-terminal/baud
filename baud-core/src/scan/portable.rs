use crate::*;

/// Vectorised through `core::simd`, which lowers to NEON on aarch64, SSE2 on
/// baseline x86-64, and a scalar loop anywhere without vectors. Requires no
/// target feature, so it is always safe to call.
///
/// 16-byte lanes. [`Avx2`] stays for x86-64 because it does 32.
pub(crate) struct Portable;

impl Scan for Portable {
    /// Vectorised, and re-measured.
    ///
    /// This was scalar with a comment saying a `core::simd` version "measured a
    /// 12% regression on the streaming payload". That measurement predates
    /// `baud-bench/src/bin/ingest.rs` and was taken on a harness now known to
    /// report the same ~230 MB/s for builds that differ by 2.5x, so it is worth
    /// nothing and the question was reopened. The profile is what reopened it:
    /// this loop is **38.7%** of the streaming cells path, the largest single
    /// item in it.
    #[inline(always)]
    unsafe fn run(buf: &[u8]) -> ByteLen {
        use core::simd::{Simd, cmp::SimdPartialOrd};
        let n = buf.len();
        let mut i = 0usize;
        // Printable is `0x20..=0x7e`. One unsigned compare after subtracting
        // the low bound, so the pair of comparisons becomes one.
        let lo = Simd::<u8, LANES>::splat(0x20);
        let span = Simd::<u8, LANES>::splat(0x7e - 0x20);
        while i + LANES <= n {
            let v = Simd::<u8, LANES>::from_slice(&buf[i..i + LANES]);
            let m = LaneMask::from_movemask((v - lo).simd_gt(span).to_bitmask() as u32);
            if let Some(lane) = m.first() {
                return lane.offset_from(i);
            }
            i += LANES;
        }
        while i < n && StreamByte::from_stream(buf[i]).is_printable() {
            i += 1;
        }
        ByteLen::from_lane(i)
    }

    #[inline(always)]
    unsafe fn final_byte(buf: &[u8]) -> Option<ByteLen> {
        use core::simd::{Simd, cmp::SimdPartialOrd};
        let n = buf.len();
        let mut i = 0usize;
        let lo = Simd::<u8, LANES>::splat(0x40);
        let hi = Simd::<u8, LANES>::splat(0x7e);
        while i + LANES <= n {
            let v = Simd::<u8, LANES>::from_slice(&buf[i..i + LANES]);
            let m = LaneMask::from_movemask((v.simd_ge(lo) & v.simd_le(hi)).to_bitmask() as u32);
            if let Some(lane) = m.first() {
                return Some(lane.offset_from(i));
            }
            i += LANES;
        }
        while i < n {
            if FinalByte::from_stream(StreamByte::from_stream(buf[i])).is_some() {
                return Some(ByteLen::from_lane(i));
            }
            i += 1;
        }
        None
    }

    #[inline(always)]
    unsafe fn st(buf: &[u8]) -> Option<ByteLen> {
        use core::simd::{Simd, cmp::SimdPartialEq};
        let n = buf.len();
        let mut i = 0usize;
        let bel = Simd::<u8, LANES>::splat(0x07);
        let esc = Simd::<u8, LANES>::splat(0x1b);
        while i + LANES <= n {
            let v = Simd::<u8, LANES>::from_slice(&buf[i..i + LANES]);
            let m = LaneMask::from_movemask((v.simd_eq(bel) | v.simd_eq(esc)).to_bitmask() as u32);
            if let Some(lane) = m.first() {
                return Some(lane.offset_from(i));
            }
            i += LANES;
        }
        while i < n {
            let b = StreamByte::from_stream(buf[i]);
            if b == StreamByte::BEL || b == StreamByte::ESC {
                return Some(ByteLen::from_lane(i));
            }
            i += 1;
        }
        None
    }
}
