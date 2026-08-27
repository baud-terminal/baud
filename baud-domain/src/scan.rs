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
