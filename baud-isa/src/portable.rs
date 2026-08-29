use crate::{DECODE_LANES, Decode, DecodedBlock, LANES, Scan};
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
            let m = Vectorized::<Printable>::load(&buf[i..i + LANES]).not_printable();
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
            let m = Vectorized::<Printable>::load(&buf[i..i + LANES])
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
            let m = Vectorized::<Printable>::load(&buf[i..i + LANES])
                .either(StreamByte::BEL, StreamByte::ESC);
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

impl Decode for Portable {
    /// # Safety
    /// `Portable` requires no target feature.
    #[inline(always)]
    unsafe fn block(buf: &[StreamByte]) -> DecodedBlock {
        let none = DecodedBlock {
            cps: [Codepoint::REPLACEMENT; DECODE_LANES],
            widths: [Width::NARROW; DECODE_LANES],
            count: LaneCount::ZERO,
            bytes: ByteLen::ZERO,
        };
        if buf.len() < LANE_COUNT {
            return none;
        }
        let block = Vectorized::<Utf8>::load(buf);
        // Refuse the whole block rather than decode a byte no sequence may
        // contain. Every conformance decision stays in one place.
        if block.impossible().any() {
            return none;
        }
        // **Boundaries come from the bytes, not from what they decode to.**
        // That is UTF-8's self-synchronisation, and it is what lets one pass
        // handle ASCII, Latin, kana and Han without caring which comes next.
        let Some(starts) = block.leads().first_four() else {
            return none;
        };
        // Four characters, four gather lanes each, padded by indices at or
        // past the block, which read as zero. `swizzle_dyn` is `tbl` on
        // aarch64 and `pshufb` on x86-64, so the gather is *computed*: the
        // textbook decoder indexes a shuffle table by the lead pattern, which
        // is tens of kilobytes and would not stay resident.
        let mut idx = [LANE_COUNT as u8; LANE_COUNT];
        for k in 0..DECODE_LANES {
            let o = starts.start(k).index();
            for j in 0..starts.len(k).index() {
                idx[4 * k + j] = (o + j) as u8;
            }
        }
        let quads = block.gather(Vectorized::<Utf8>::indices(idx)).quads();
        let mut cps = [Codepoint::REPLACEMENT; DECODE_LANES];
        let mut widths = [Width::NARROW; DECODE_LANES];
        let mut bytes = 0usize;
        for k in 0..DECODE_LANES {
            let Some(cp) = Codepoint::from_quad(quads[k], starts.len(k)) else {
                // A shorter prefix is still useful: hand back what decoded and
                // let the scalar path take the character that did not.
                return match k {
                    0 => none,
                    _ => DecodedBlock {
                        cps,
                        widths,
                        count: LaneCount::of(k),
                        bytes: ByteLen::from_index(bytes),
                    },
                };
            };
            cps[k] = cp;
            widths[k] = Width::of(cp, AmbiguousWidth::Narrow);
            bytes += starts.len(k).index();
        }
        DecodedBlock {
            cps,
            widths,
            count: LaneCount::of(DECODE_LANES),
            bytes: ByteLen::from_index(bytes),
        }
    }
}
