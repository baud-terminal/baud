use crate::StreamByte;
use core::simd::Simd;
use core::simd::cmp::{SimdPartialEq, SimdPartialOrd};

use crate::*;

/// The result of a SIMD movemask: one bit per lane, set where the lane matched.
///
/// Typed because two different quantities were being carried as bare integers
/// here and they are trivially swappable: the *mask* is a bitset over lanes,
/// and `trailing_zeros()` of it is a lane *index*. Adding a mask to a byte
/// offset compiles; adding a lane index to one is what the code means. Six
/// sites across three scanners did this with `as u32` and `as usize`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LaneMask(pub(crate) u32);

impl LaneMask {
    /// From a `movemask` intrinsic or a portable `Mask::to_bitmask`.
    pub fn from_movemask(bits: u32) -> LaneMask {
        LaneMask(bits)
    }
    pub fn any(self) -> bool {
        self.0 != 0
    }
    /// The first matching lane, if any. The only way to get an index out.
    pub fn first(self) -> Option<Lane> {
        self.any().then(|| Lane(self.0.trailing_zeros() as usize))
    }
}

/// Which lane of a SIMD block matched. Not a byte length and not a byte
/// position -- it only becomes either by being added to the block's base.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Lane(pub(crate) usize);

impl Lane {
    /// The byte length from the start of a scan to this lane, given how many
    /// bytes were already consumed by earlier blocks.
    ///
    /// The addition lives here so a caller cannot write `lane + something_else`
    /// and get a plausible-looking number.
    pub fn offset_from(self, consumed: usize) -> ByteLen {
        ByteLen::from_index(consumed + self.0)
    }
}

/// A lane mask in aarch64's **nibble** encoding, as `shrn #4` produces it.
///
/// **A different encoding, not a different meaning, and that is exactly why it
/// is a different type.** `[[source: arm-neon-bitmask]]` shows that NEON's
/// cheapest `movemask` equivalent is `shrn` -- read the 128-bit compare result
/// as eight 16-bit lanes, shift right by four and narrow to eight bytes, which
/// leaves a 64-bit value carrying **four bits per input byte** rather than one.
///
/// So the first set lane is `trailing_zeros() / 4`, not `trailing_zeros()`.
/// Handing that `u64` to `LaneMask` would compile and be wrong by a factor of
/// four -- silently, and only on inputs where the match is not in lane zero.
/// Both types produce a `Lane` and neither lets an index out any other way.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct NibbleMask(u64);

impl NibbleMask {
    /// From `vshrn_n_u16(cmp, 4)` reinterpreted as a `u64`.
    pub fn from_shrn(bits: u64) -> NibbleMask {
        NibbleMask(bits)
    }
    pub fn any(self) -> bool {
        self.0 != 0
    }
    /// The first matching lane, if any. Four bits a lane, so the count of
    /// trailing zeros is four times the index.
    pub fn first(self) -> Option<Lane> {
        self.any()
            .then(|| Lane(self.0.trailing_zeros() as usize / 4))
    }
}

/// A block of `StreamByte`s as a vector, and the operations a scanner needs.
///
/// **The lane type, not `Simd<u8, 16>`.** Every scanner in this project used
/// to build a raw `Simd<u8, LANES>`, compare it, call `to_bitmask()` and cast
/// the result `as u32` to get a `LaneMask` -- and that cast is where an
/// intrinsic's return value enters the type system, which is the boundary the
/// audit exists to hold. Wrapping the vector moves the cast inside one type,
/// so a scanner never writes `as` at all and cannot hand a bitmask to
/// something expecting a byte offset.
///
/// `repr(transparent)` over the vector, so this costs nothing: the newtypes in
/// this crate lower to the same instructions, which was measured on the ingest
/// path with every range overlapping.
#[repr(transparent)]
#[derive(Clone, Copy)]
pub struct StreamVector(Simd<u8, LANE_COUNT>);

/// How many `StreamByte`s a `StreamVector` holds.
pub const LANE_COUNT: usize = 16;

impl StreamVector {
    /// Load one block. The caller has checked there are `LANE_COUNT` bytes.
    #[inline(always)]
    pub fn load(buf: &[StreamByte]) -> StreamVector {
        StreamVector(Simd::from_slice(StreamByte::slice_as_bytes(buf)))
    }

    /// Lanes that are **not** printable ASCII: below `0x20`, `0x7f`, or above.
    ///
    /// One subtract and one unsigned compare, which is what makes it a single
    /// test rather than three: `b - 0x20 > 0x5e` is false exactly for
    /// `0x20..=0x7e`.
    #[inline(always)]
    pub fn not_printable(self) -> LaneMask {
        let lo = Simd::splat(0x20);
        let span = Simd::splat(0x7e - 0x20);
        LaneMask::from_movemask((self.0 - lo).simd_gt(span).to_bitmask() as u32)
    }

    /// Lanes holding a byte in `lo..=hi`, inclusive.
    #[inline(always)]
    pub fn in_range(self, lo: StreamByte, hi: StreamByte) -> LaneMask {
        let l = self.0.simd_ge(Simd::splat(lo.0));
        let h = self.0.simd_le(Simd::splat(hi.0));
        LaneMask::from_movemask((l & h).to_bitmask() as u32)
    }

    /// Lanes equal to either of two bytes -- the string-terminator test.
    #[inline(always)]
    pub fn either(self, a: StreamByte, b: StreamByte) -> LaneMask {
        let x = self.0.simd_eq(Simd::splat(a.0));
        let y = self.0.simd_eq(Simd::splat(b.0));
        LaneMask::from_movemask((x | y).to_bitmask() as u32)
    }
}
