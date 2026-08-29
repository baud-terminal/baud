use crate::{LANES, Scan};
use baud_domain::*;

/// Vectorised through `core::simd`, which lowers to NEON on aarch64, SSE2 on
/// baseline x86-64, and a scalar loop anywhere without vectors. Requires no
/// target feature, so it is always safe to call.
///
/// 16-byte lanes. [`Avx2`] stays for x86-64 because it does 32.
pub struct Portable;

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
    unsafe fn run(buf: &[StreamByte]) -> ByteLen {
        let n = buf.len();
        let mut i = 0usize;
        while i + LANES <= n {
            // The predicate lives on the vector type, so this loop never
            // writes `as` and never sees a `Simd`. Printable is `0x20..=0x7e`,
            // and `not_printable` is the one unsigned compare that answers it.
            let m = StreamVector::load(&buf[i..i + LANES]).not_printable();
            if let Some(lane) = m.first() {
                return lane.offset_from(i);
            }
            i += LANES;
        }
        while i < n && buf[i].is_printable() {
            i += 1;
        }
        ByteLen::from_lane(i)
    }

    #[inline(always)]
    unsafe fn final_byte(buf: &[StreamByte]) -> Option<ByteLen> {
        let n = buf.len();
        let mut i = 0usize;
        while i + LANES <= n {
            let m = StreamVector::load(&buf[i..i + LANES])
                .in_range(StreamByte::FINAL_LOW, StreamByte::FINAL_HIGH);
            if let Some(lane) = m.first() {
                return Some(lane.offset_from(i));
            }
            i += LANES;
        }
        while i < n {
            if FinalByte::from_stream(buf[i]).is_some() {
                return Some(ByteLen::from_lane(i));
            }
            i += 1;
        }
        None
    }

    #[inline(always)]
    unsafe fn st(buf: &[StreamByte]) -> Option<ByteLen> {
        let n = buf.len();
        let mut i = 0usize;
        while i + LANES <= n {
            let m = StreamVector::load(&buf[i..i + LANES]).either(StreamByte::BEL, StreamByte::ESC);
            if let Some(lane) = m.first() {
                return Some(lane.offset_from(i));
            }
            i += LANES;
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
