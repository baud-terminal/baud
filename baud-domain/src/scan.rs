use crate::StreamByte;
use core::marker::PhantomData;
use core::simd::Simd;
use core::simd::cmp::{SimdPartialEq, SimdPartialOrd};
use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout};

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

    /// The first four characters this lead mask describes, **starting at lane
    /// zero**.
    ///
    /// Needs **five** set lanes, not four: a character's length is the gap to
    /// the next lead, so the fifth boundary is what makes the fourth length
    /// known. Fewer than five means the block ends mid-character and the
    /// caller falls back to the scalar decoder, which owns that case.
    ///
    /// **Lane zero must be a lead**, and that is a correctness requirement
    /// rather than a convenience. Without it a block whose first byte is a
    /// continuation -- a slice taken mid-character, which happens on every
    /// read boundary -- would silently skip forward to the next boundary and
    /// decode a character the scalar decoder refuses at that position. The
    /// agreement test found exactly that.
    pub fn first_four(self) -> Option<CharStarts> {
        if self.0 & 1 == 0 {
            return None;
        }
        let mut bits = self.0;
        let mut at = [0usize; START_COUNT + 1];
        for slot in at.iter_mut() {
            if bits == 0 {
                return None;
            }
            let p = bits.trailing_zeros() as usize;
            *slot = p;
            bits &= bits - 1;
        }
        let mut starts = [Lane(0); START_COUNT];
        let mut lens = [ByteLen::ZERO; START_COUNT];
        for k in 0..START_COUNT {
            starts[k] = Lane(at[k]);
            lens[k] = ByteLen::from_index(at[k + 1] - at[k]);
        }
        Some(CharStarts { starts, lens })
    }
}

/// Which lane of a SIMD block matched. Not a byte length and not a byte
/// position -- it only becomes either by being added to the block's base.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Lane(pub(crate) usize);

impl Lane {
    /// This lane's position in the block.
    ///
    /// Distinct from `offset_from`, which adds a scan's base. This is the one
    /// to use when indexing *within* the block that produced the mask.
    pub fn index(self) -> usize {
        self.0
    }

    /// The byte length from the start of a scan to this lane, given how many
    /// bytes were already consumed by earlier blocks.
    ///
    /// The addition lives here so a caller cannot write `lane + something_else`
    /// and get a plausible-looking number.
    pub fn offset_from(self, consumed: usize) -> ByteLen {
        ByteLen::from_index(consumed + self.0)
    }
}

/// How many characters a decode step produced. Never a bare count.
#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct LaneCount(usize);

impl LaneCount {
    pub const ZERO: LaneCount = LaneCount(0);
    pub fn of(n: usize) -> LaneCount {
        LaneCount(n)
    }
    pub fn index(self) -> usize {
        self.0
    }
    pub fn is_zero(self) -> bool {
        self.0 == 0
    }
}

/// How many characters `CharStarts` describes.
pub const START_COUNT: usize = 4;

/// Where the first few characters of a block begin, and how long each is.
///
/// **Derived from the lead mask alone.** A character's length is the distance
/// to the next lead byte, so this needs no table and no per-character branch:
/// UTF-8's self-synchronisation puts boundaries in the bytes themselves. That
/// is the property a class-agnostic decoder rests on.
#[derive(Clone, Copy)]
pub struct CharStarts {
    starts: [Lane; START_COUNT],
    lens: [ByteLen; START_COUNT],
}

impl CharStarts {
    /// Where character `k` begins, as a lane of the block.
    pub fn start(self, k: usize) -> Lane {
        self.starts[k]
    }
    /// How many bytes character `k` occupies.
    pub fn len(self, k: usize) -> ByteLen {
        self.lens[k]
    }
    /// Total bytes the described characters occupy.
    pub fn bytes(self) -> ByteLen {
        let mut n = 0usize;
        for l in self.lens.iter() {
            n += l.index();
        }
        ByteLen::from_index(n)
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

/// How many `StreamByte`s one vector holds -- one register.
///
/// Sixteen and not more on purpose. This is the whole working set of a scan
/// step, and the rule this project is built on is that structures touched per
/// byte stay resident; a wider block would not change what is resident but a
/// *table* indexed by the block would, which is why the UTF-8 operations below
/// compute rather than look up.
pub const LANE_COUNT: usize = 16;

/// A block of `StreamByte`s as a vector, tagged with what it has been proven
/// to hold.
///
/// **The register counterpart of [`Bytes`].** `Bytes<K>` is a byte slice
/// carrying a proof of its contents; this is sixteen of those bytes in a
/// vector, carrying the same proof. Same `Kind`, same markers, same
/// `Kind::ENCODING` bridge to the runtime `Encoding` -- there is deliberately
/// no second vocabulary for this, and an earlier draft of this type invented
/// one and collided with the domain three times before the existing one was
/// found.
///
/// The tag is a type and not a field because the *operations* differ:
/// `leads()` is meaningful on UTF-8 and meaningless on printable ASCII, so it
/// lives on `Vectorized<Utf8>` and asking for it on `Vectorized<Printable>`
/// does not compile. A runtime tag would put every method on one type, allow
/// the wrong call, and cost a branch in a loop measured in single percent.
///
/// `repr(transparent)` over the vector, so this costs nothing: the newtypes in
/// this crate lower to the same instructions, measured on the ingest path with
/// every range overlapping.
///
/// **The point is that a scanner never writes `as`.** Every scanner here used
/// to build a raw `Simd<u8, 16>`, compare it, call `to_bitmask()` and cast the
/// result `as u32` -- and that cast is where an intrinsic's return value enters
/// the type system, which is the boundary the audit exists to hold.
/// **Zerocopy, like every other wire-facing type here.** The lanes are stored
/// as an array rather than a `Simd` so the type can derive `FromBytes` and
/// `IntoBytes`: a block of stream bytes *is* sixteen bytes, and reinterpreting
/// the arena as one must not need a copy or a cast. `Simd::from_array` on the
/// way into an operation is free -- it is a register move the optimiser
/// removes -- and the tag is a `PhantomData`, so the whole struct is exactly
/// its sixteen bytes.
#[repr(transparent)]
#[derive(FromBytes, IntoBytes, Immutable, KnownLayout)]
pub struct Vectorized<K: Kind> {
    v: [u8; LANE_COUNT],
    _kind: PhantomData<K>,
}

// `Clone` and `Copy` by hand: the markers are uninhabited enums, so a derive
// would demand `K: Copy` and no `Kind` can satisfy it. The tag is a
// `PhantomData` and the payload is sixteen bytes, so copying is trivially
// sound however uninhabited the tag is.
impl<K: Kind> Clone for Vectorized<K> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<K: Kind> Copy for Vectorized<K> {}

impl<K: Kind> Vectorized<K> {
    /// The lanes as a vector. Free: a register move the optimiser removes.
    #[inline(always)]
    fn lanes(self) -> Simd<u8, LANE_COUNT> {
        Simd::from_array(self.v)
    }

    /// Load one block. The caller has checked there are `LANE_COUNT` bytes.
    #[inline(always)]
    pub fn load(buf: &[StreamByte]) -> Vectorized<K> {
        let mut v = [0u8; LANE_COUNT];
        v.copy_from_slice(&StreamByte::slice_as_bytes(buf)[..LANE_COUNT]);
        Vectorized {
            v,
            _kind: PhantomData,
        }
    }
}

impl Vectorized<Printable> {
    /// Lanes that are **not** printable ASCII: below `0x20`, `0x7f`, or above.
    ///
    /// One subtract and one unsigned compare: `b - 0x20 > 0x5e` is false
    /// exactly for `0x20..=0x7e`, so the pair of comparisons becomes one.
    #[inline(always)]
    pub fn not_printable(self) -> LaneMask {
        let v = self.lanes();
        let lo = Simd::splat(0x20);
        let span = Simd::splat(0x7e - 0x20);
        LaneMask::from_movemask((v - lo).simd_gt(span).to_bitmask() as u32)
    }

    /// Lanes holding a byte in `lo..=hi`, inclusive.
    #[inline(always)]
    pub fn in_range(self, lo: StreamByte, hi: StreamByte) -> LaneMask {
        let v = self.lanes();
        let l = v.simd_ge(Simd::splat(lo.0));
        let h = v.simd_le(Simd::splat(hi.0));
        LaneMask::from_movemask((l & h).to_bitmask() as u32)
    }

    /// Lanes equal to either of two bytes -- the string-terminator test.
    #[inline(always)]
    pub fn either(self, a: StreamByte, b: StreamByte) -> LaneMask {
        let v = self.lanes();
        let x = v.simd_eq(Simd::splat(a.0));
        let y = v.simd_eq(Simd::splat(b.0));
        LaneMask::from_movemask((x | y).to_bitmask() as u32)
    }
}

impl Vectorized<Utf8> {
    /// Lanes that **begin** a character: anything that is not `10xxxxxx`.
    ///
    /// This is the whole of UTF-8's self-synchronisation in one instruction,
    /// and it is what makes a class-agnostic decoder possible: character
    /// boundaries are a property of the bytes, not of what they decode to, so
    /// finding them needs no table and no branch per character.
    #[inline(always)]
    pub fn leads(self) -> LaneMask {
        let cont = (self.lanes() & Simd::splat(0xc0)).simd_eq(Simd::splat(0x80));
        LaneMask::from_movemask((!cont).to_bitmask() as u32)
    }

    /// Lanes holding a byte no UTF-8 sequence may contain.
    ///
    /// `0xc0`, `0xc1` would only ever encode an overlong form and `0xf5`
    /// upwards is past U+10FFFF. A block containing one is refused outright
    /// rather than decoded, which keeps every conformance decision in the
    /// scalar path instead of duplicated per instruction set.
    #[inline(always)]
    pub fn impossible(self) -> LaneMask {
        let v = self.lanes();
        let a = v.simd_eq(Simd::splat(0xc0));
        let b = v.simd_eq(Simd::splat(0xc1));
        let c = v.simd_ge(Simd::splat(0xf5));
        LaneMask::from_movemask((a | b | c).to_bitmask() as u32)
    }

    /// Gather bytes by index, four per character, into one register.
    ///
    /// `swizzle_dyn` is a runtime shuffle -- `tbl` on aarch64, `pshufb` on
    /// x86-64 -- so the gather is *computed* rather than looked up. The
    /// textbook decoder indexes a shuffle table by the lead-byte pattern,
    /// which for sixteen bytes is tens of kilobytes; this project's second
    /// rule is that what is touched per byte stays in cache, and a table that
    /// size does not.
    ///
    /// Indices at or past `LANE_COUNT` read as zero, which is what pads a
    /// one-, two- or three-byte sequence out to four lanes.
    #[inline(always)]
    pub fn gather(self, index: Vectorized<Utf8>) -> Vectorized<Utf8> {
        Vectorized {
            v: self.lanes().swizzle_dyn(index.lanes()).to_array(),
            _kind: PhantomData,
        }
    }

    /// Build an index block from raw lane positions.
    #[inline(always)]
    pub fn indices(idx: [u8; LANE_COUNT]) -> Vectorized<Utf8> {
        Vectorized {
            v: idx,
            _kind: PhantomData,
        }
    }

    /// The block as four 32-bit groups, one per character, little-endian.
    ///
    /// A plain array and not a `Simd`, so a consumer needs no `portable_simd`
    /// feature to read the result. The vector work stays behind this type.
    #[inline(always)]
    pub fn quads(self) -> [u32; START_COUNT] {
        let mut out = [0u32; START_COUNT];
        let b = self.v;
        for (k, o) in out.iter_mut().enumerate() {
            *o = u32::from_le_bytes([b[4 * k], b[4 * k + 1], b[4 * k + 2], b[4 * k + 3]]);
        }
        out
    }
}
