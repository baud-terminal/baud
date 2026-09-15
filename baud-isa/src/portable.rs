use crate::{DECODE_LANES, Decode, DecodedBlock, LANES, Scan, Warm};
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
    /// 12% regression on the streaming payload". That measurement predates the
    /// in-memory ingest harness and was taken on one now known to
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

    /// # Safety
    /// None required; `Portable` is portable.
    #[inline(always)]
    unsafe fn stride_three(buf: &[StreamByte]) -> Count {
        Portable::stride_three_portable(buf)
    }

    /// # Safety
    /// None required; `Portable` is portable.
    #[inline(always)]
    unsafe fn stride_four(buf: &[StreamByte]) -> Count {
        Portable::stride_four_portable(buf)
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

impl Portable {
    /// The stride-three run, as `baud-domain` defines it.
    ///
    /// Delegating rather than re-deriving: `Vectorized::stride_three_run` is
    /// the description every other implementation answers to, and a second
    /// spelling of "lanes 0, 3, 6, 9, 12 must lead and the rest must continue"
    /// is the shape this repo has paid for twice -- `csi_whole` beside the
    /// table, and a hand-written `SGR` grammar beside `SgrAttr::parse`.
    /// The stride-four run, as `baud-domain` defines it.
    fn stride_four_portable(buf: &[StreamByte]) -> Count {
        match buf.len() >= LANES {
            true => Count::of(Vectorized::<Printable>::load(&buf[..LANES]).stride_four_run()),
            false => Count::ZERO,
        }
    }

    fn stride_three_portable(buf: &[StreamByte]) -> Count {
        match buf.len() >= LANES {
            true => Count::of(Vectorized::<Printable>::load(&buf[..LANES]).stride_three_run()),
            false => Count::ZERO,
        }
    }
}

impl Decode for Portable {
    /// # Safety
    /// `Portable` requires no target feature.
    #[inline(always)]
    unsafe fn block(buf: &[StreamByte]) -> DecodedBlock {
        let none = DecodedBlock {
            cps: [Codepoint::REPLACEMENT; DECODE_LANES],
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
        //
        // The offsets come from a table rather than a prefix sum: one load
        // against five `trailing_zeros` and a loop, which is the serial chain
        // that made the computed version 2.4x slower than the const-stride
        // collector. Four bytes an entry, 16 KB total, and measured on
        // `cat-cjk.raw` only 212 of the 4096 patterns occur at all -- a touched
        // set of ~848 bytes.
        let Some(starts) = block.leads().table_four() else {
            return none;
        };
        // Two vector steps build the gather: broadcast each offset four times,
        // add `[0,1,2,3]`, push the padding lanes out of range.
        // **Vector to the codepoint, not just to the boundary.** This was a
        // `Codepoint::from_quad` per lane -- the whole scalar decoder, lead
        // parse, continuation checks and shortest-form test, four times on top
        // of the vector setup. That is why the block path lost to `wide_run`
        // on homogeneous text and lost everywhere else too.
        let (cps, count) = block.gather_chars(starts).decode(starts);
        if count.is_zero() {
            return none;
        }
        // A shorter prefix is still useful: hand back what decoded and let the
        // scalar path take the character that did not.
        let mut bytes = 0usize;
        for k in 0..count.index() {
            bytes += starts.len(k).index();
        }
        if count.index() < DECODE_LANES {
            return DecodedBlock {
                cps,
                count,
                bytes: ByteLen::from_index(bytes),
            };
        }
        DecodedBlock {
            cps,
            count: LaneCount::of(DECODE_LANES),
            bytes: ByteLen::from_index(bytes),
        }
    }
}

/// No hint: `core::simd` has no prefetch, and a portable backend that did
/// nothing here is the baseline the ISA arms are measured against.
impl Warm for Portable {
    #[inline(always)]
    fn warm_stores(_cells: &[PackedCell]) {}
}
