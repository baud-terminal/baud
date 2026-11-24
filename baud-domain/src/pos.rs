use crate::*;
use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout};

/// Where a byte sits in the caller's arena: an index, not a stream offset.
///
/// **`baud-core` uses it as an arena index and nothing more.** `Term::write_at`
/// places a read at `arena[base..]` and `Term::feed` parses it from there, so
/// a position names a byte only until the next read lands on it. It is neither
/// monotonic nor unique: the app, the benches and the tests' `support::feed`
/// place every read at [`BytePos::ZERO`], and the look-back that finds a
/// placing hide (`baud-core`'s `placed.rs`) subtracts the read's base to tell
/// the read's own bytes from the stream's tail before it.
///
/// **An absolute stream offset is the model's meaning, and the model is not
/// built.** [`Run`] and the [`Source`] slots read it as an offset into the
/// whole stream, monotonic and never reused, and nothing constructs any of
/// them.
///
/// No public field and no arithmetic operators, so nothing adds one position
/// to another: a move is named ([`BytePos::advance`],
/// [`BytePos::advance_read`]) and so is a distance ([`BytePos::gap`]).
///
/// An arena position has no public field and no arithmetic operators: one
/// position is never added to another; a move is named (advance, advance_read)
/// and a distance is a saturating ByteGap (gap).
///
/// ```compile_fail,E0369
/// let p = baud_domain::BytePos::ZERO;
/// let _ = p + p;
/// ```
///
/// The legal path:
///
/// ```
/// let p = baud_domain::BytePos::from_index(4);
/// let _ = p.advance(baud_domain::ByteLen::from_index(2)).gap(p);
/// ```
#[derive(
    Clone, Copy, PartialEq, Eq, PartialOrd, Ord, FromBytes, IntoBytes, Immutable, KnownLayout,
)]
#[repr(transparent)]
pub struct BytePos(pub(crate) u64);

impl BytePos {
    pub const ZERO: BytePos = BytePos(0);
    /// The position of byte `n` of the arena.
    pub fn from_index(n: usize) -> BytePos {
        BytePos(n as u64)
    }
    pub fn index(self) -> usize {
        self.0 as usize
    }
    pub fn advance(self, n: ByteLen) -> BytePos {
        BytePos(self.0 + n.get() as u64)
    }
    /// Forward by part of a read. Separate from `advance` because a `ByteLen`
    /// saturates at 65,535 and a read does not.
    pub fn advance_read(self, n: ReadLen) -> BytePos {
        BytePos(self.0 + n.index() as u64)
    }
    /// Distance, saturating. Deliberately not `Sub`, so a negative gap cannot
    /// wrap into a huge positive one.
    pub fn gap(self, earlier: BytePos) -> ByteGap {
        ByteGap(self.0.saturating_sub(earlier.0))
    }
}

/// Distance between two [`BytePos`]. Cannot be added to a position. Nothing
/// reads one: the store that compared it against its arena was never built.
#[derive(
    Clone, Copy, PartialEq, Eq, PartialOrd, Ord, FromBytes, IntoBytes, Immutable, KnownLayout,
)]
#[repr(transparent)]
pub struct ByteGap(pub(crate) u64);

/// Index of a logical line. **Stable across resize and font change** — this is
/// the only line identifier safe to persist.
///
/// An absolute line has no arithmetic: a distance between two lines is only
/// [`LineIdx::since`]'s `Option<Rows>`, `None` when the line is not after the
/// other, and a move is `plus` (saturating) or `minus` (`None` before the
/// first).
///
/// ```compile_fail,E0369
/// fn f(a: baud_domain::LineIdx, b: baud_domain::LineIdx) { let _ = a - b; }
/// ```
///
/// The legal path:
///
/// ```
/// fn f(a: baud_domain::LineIdx, b: baud_domain::LineIdx) -> Option<baud_domain::Rows> { a.since(b) }
/// ```
#[derive(
    Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, FromBytes, IntoBytes, Immutable, KnownLayout,
)]
#[repr(transparent)]
pub struct LineIdx(pub(crate) u64);

impl LineIdx {
    pub const FIRST: LineIdx = LineIdx(0);
    pub fn next(self) -> LineIdx {
        LineIdx(self.0 + 1)
    }
    /// An absolute line by number. A caller that tracks the scroll position
    /// (`scrolled - back`, or `hist + row`) names a line this way; an image
    /// placement anchors to one.
    pub const fn of(n: u64) -> LineIdx {
        LineIdx(n)
    }
    /// The absolute line number, for a renderer positioning against the scroll.
    pub fn value(self) -> u64 {
        self.0
    }
    /// The line `n` rows later.
    pub fn plus(self, n: Rows) -> LineIdx {
        LineIdx(
            self.0
                .saturating_add(u64::try_from(n.index()).unwrap_or(u64::MAX)),
        )
    }
    /// The line `n` rows earlier, or `None` before the first.
    pub fn minus(self, n: Rows) -> Option<LineIdx> {
        self.0
            .checked_sub(u64::try_from(n.index()).ok()?)
            .map(LineIdx)
    }
    /// How many rows after `earlier` this line is, or `None` if it is not
    /// after it.
    pub fn since(self, earlier: LineIdx) -> Option<Rows> {
        let d = self.0.checked_sub(earlier.0)?;
        usize::try_from(d).ok().map(Rows::of_count)
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
