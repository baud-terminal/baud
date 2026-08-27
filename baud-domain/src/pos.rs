use crate::*;
use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout};

/// Absolute offset in the total byte stream. Monotonic, never reused.
///
/// No public field and no arithmetic: you cannot accidentally treat it as a
/// ring offset. Converting to one is [`Store::resolve`]'s job, and it is the
/// only place that knows about `ARENA`.
#[derive(
    Clone, Copy, PartialEq, Eq, PartialOrd, Ord, FromBytes, IntoBytes, Immutable, KnownLayout,
)]
#[repr(transparent)]
pub struct BytePos(pub(crate) u64);

impl BytePos {
    pub const ZERO: BytePos = BytePos(0);
    /// Absolute stream position from an arena offset. The arena is the only
    /// thing that knows the mapping, so this is where it is stated.
    pub fn from_index(n: usize) -> BytePos {
        BytePos(n as u64)
    }
    pub fn index(self) -> usize {
        self.0 as usize
    }
    pub fn advance(self, n: ByteLen) -> BytePos {
        BytePos(self.0 + n.get() as u64)
    }
    /// Distance, saturating. Deliberately not `Sub`, so a negative gap cannot
    /// wrap into a huge positive one.
    pub fn gap(self, earlier: BytePos) -> ByteGap {
        ByteGap(self.0.saturating_sub(earlier.0))
    }
}

/// Index of a logical line. **Stable across resize and font change** — this is
/// the only line identifier safe to persist.
/// Distance between two [`BytePos`]. Cannot be added to a position; only
/// compared against a capacity.
#[derive(
    Clone, Copy, PartialEq, Eq, PartialOrd, Ord, FromBytes, IntoBytes, Immutable, KnownLayout,
)]
#[repr(transparent)]
pub struct ByteGap(pub(crate) u64);

impl ByteGap {
    pub(crate) fn exceeds(self, capacity: usize) -> bool {
        self.0 > capacity as u64
    }
}

#[derive(
    Clone, Copy, PartialEq, Eq, PartialOrd, Ord, FromBytes, IntoBytes, Immutable, KnownLayout,
)]
#[repr(transparent)]
pub struct LineIdx(pub(crate) u64);

impl LineIdx {
    pub const FIRST: LineIdx = LineIdx(0);
    pub fn next(self) -> LineIdx {
        LineIdx(self.0 + 1)
    }
}

/// Index into the shared run pool.
#[derive(
    Clone, Copy, PartialEq, Eq, PartialOrd, Ord, FromBytes, IntoBytes, Immutable, KnownLayout,
)]
#[repr(transparent)]
pub struct RunIdx(pub(crate) u32);

impl RunIdx {
    pub const FIRST: RunIdx = RunIdx(0);
    pub fn index(self) -> usize {
        self.0 as usize
    }
    pub fn advance(self, n: RunCount) -> RunIdx {
        RunIdx(self.0 + n.0 as u32)
    }
}

/// A ring position in *slots*, not bytes.
///
/// `u8` is not an optimisation, it is the proof: 256 slots means the ring wrap
/// is `wrapping_add(1)` with no modulo, no comparison, and no `if tail + 4096 >
/// ARENA { tail = 0 }` to get wrong.
#[derive(Clone, Copy, PartialEq, Eq, FromBytes, IntoBytes, Immutable, KnownLayout)]
#[repr(transparent)]
pub struct SlotIdx(pub(crate) u8);

impl SlotIdx {
    pub const FIRST: SlotIdx = SlotIdx(0);
    /// Wraps at the end of the arena, by construction.
    pub fn next(self) -> SlotIdx {
        SlotIdx(self.0.wrapping_add(1))
    }
    pub fn index(self) -> usize {
        self.0 as usize
    }
    /// Byte offset of this slot within the arena.
    /// Byte offset of this slot within the arena. Named as an index because
    /// that is the only thing it may be used for.
    pub fn byte_index(self) -> usize {
        self.0 as usize * SLOT_BYTES
    }
}

/// Position within the scrollback ring. Wraps by construction.
#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct RingPos(pub(crate) usize);

impl RingPos {
    pub const ZERO: RingPos = RingPos(0);
    pub fn advance(self, modulus: usize) -> RingPos {
        RingPos((self.0 + 1) % modulus)
    }
    pub fn plus(self, r: Row, modulus: usize) -> RingPos {
        RingPos((self.0 + r.index()) % modulus)
    }
    pub fn back(self, n: usize, modulus: usize) -> RingPos {
        RingPos((self.0 + modulus - n % modulus) % modulus)
    }
    pub fn index(self) -> usize {
        self.0
    }
}

/// Write offset into the arena, in bytes, wrapping at capacity.
#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct ArenaTail(pub(crate) usize);

impl ArenaTail {
    pub const ZERO: ArenaTail = ArenaTail(0);
    /// Wraps when a full slot would not fit, so a read never straddles the end.
    pub fn next_slot(self, capacity: usize) -> ArenaTail {
        if self.0 + SLOT_BYTES > capacity {
            ArenaTail(0)
        } else {
            self
        }
    }
    pub fn advance(self, n: ByteLen) -> ArenaTail {
        ArenaTail(self.0 + n.index())
    }
    pub fn index(self) -> usize {
        self.0
    }
    pub fn pos(self) -> BytePos {
        BytePos::from_index(self.0)
    }
}

/// Index of a line slot in the scrollback ring. Distinct from [`LineIdx`],
/// which is a stable logical line number: this one wraps.
#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct LineSlot(pub(crate) usize);

impl LineSlot {
    pub fn of(pos: RingPos) -> LineSlot {
        LineSlot(pos.index())
    }
    pub fn index(self) -> usize {
        self.0
    }
    pub fn run(self, r: RunCount, maxrun: usize) -> usize {
        self.0 * maxrun + r.index()
    }
}
