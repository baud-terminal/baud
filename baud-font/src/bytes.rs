//! A bounds-checked view over a font file.
//!
//! **Every read answers `Option`, and the caller decides what an absent
//! answer means.** A table whose offset points past the file, a subtable
//! whose declared length exceeds its table, an array indexed past its count
//! -- each is `None` here and `Malformed` or "no glyph" one level up. The
//! OpenType specification names the directory's `searchRange` triple as "an
//! attack vector" and tells parsers to derive rather than trust
//! (`[[source: ms-opentype-otff]]`); this module is that rule applied to
//! every offset in the file, not only those three.
//!
//! **Typed offsets and counts, not `usize`.** An [`Offset`] is a position in
//! the file, a [`Len`] a byte count, a [`Count`] an element count and an
//! [`Index`] a position in an array; adding a count to an offset is a type
//! error, which is the class of bug the audit's raw-integer budget exists to
//! stop. The one place a primitive is unavoidable is the value read out of the
//! bytes, and [`FromBe`] is generic over it so the caller names the width in
//! its own binding rather than this module naming it in a signature.

use core::cmp::Ordering;
use core::marker::PhantomData;

/// A position in the file, from the start of the slice a [`Bytes`] views.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct Offset(u32);

impl Offset {
    pub const ZERO: Offset = Offset(0);

    pub fn of(v: impl Into<u32>) -> Offset {
        Offset(v.into())
    }

    /// `self + n`, or `None` past `u32::MAX` -- an overflowed offset is a
    /// wrapped-around read into the start of the file, which is exactly the
    /// escape a hostile `cmap` would want.
    pub fn plus(self, n: Len) -> Option<Offset> {
        self.0.checked_add(n.0).map(Offset)
    }

    /// The offset of element `i` in an array of `size`-byte records at `self`.
    pub fn at(self, i: Index, size: Len) -> Option<Offset> {
        i.0.checked_mul(size.0)
            .and_then(|b| self.0.checked_add(b))
            .map(Offset)
    }

    fn index(self) -> usize {
        usize::try_from(self.0).unwrap_or(usize::MAX)
    }
}

/// A byte count.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct Len(u32);

impl Len {
    pub const fn of(v: u32) -> Len {
        Len(v)
    }

    /// `count` records of `self` bytes each.
    pub fn times(self, count: Count) -> Option<Len> {
        self.0.checked_mul(count.0).map(Len)
    }
}

/// An element count.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct Count(u32);

impl Count {
    pub const ZERO: Count = Count(0);

    pub fn of(v: impl Into<u32>) -> Count {
        Count(v.into())
    }

    pub fn is_zero(self) -> bool {
        self.0 == 0
    }

    /// The last valid [`Index`], or `None` when empty.
    pub fn last(self) -> Option<Index> {
        self.0.checked_sub(1).map(Index)
    }

    pub fn holds(self, i: Index) -> bool {
        i.0 < self.0
    }

    /// The count as a position: the index one past the last element, which
    /// is where a trailing array begins.
    pub fn as_index(self) -> Index {
        Index(self.0)
    }
}

/// A position in an array.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct Index(u32);

impl Index {
    pub const ZERO: Index = Index(0);

    pub fn of(v: impl Into<u32>) -> Index {
        Index(v.into())
    }

    /// The distance from `base` to `self`, as a count, when `self` is at or
    /// past `base`.
    pub fn since(self, base: Index) -> Option<Count> {
        self.0.checked_sub(base.0).map(Count)
    }

    /// The next position. Saturates rather than wraps: a loop bounded by a
    /// [`Count`] stops at `u32::MAX` either way, and a wrapped index would
    /// re-read element zero.
    pub fn next(self) -> Index {
        Index(self.0.saturating_add(1))
    }

    /// `self` moved `n` elements on.
    pub fn plus(self, n: Count) -> Option<Index> {
        self.0.checked_add(n.0).map(Index)
    }

    /// The previous position, or `None` at zero.
    pub fn prev(self) -> Option<Index> {
        self.0.checked_sub(1).map(Index)
    }
}

/// A big-endian value the file stores. Implemented for the widths OpenType
/// uses; the caller's binding names the width, not a method here.
pub trait FromBe: Sized {
    const SIZE: Len;
    fn parse_be(b: &[u8]) -> Option<Self>;
}

macro_rules! be {
    ($t:ty, $n:literal) => {
        impl FromBe for $t {
            const SIZE: Len = Len($n);
            fn parse_be(b: &[u8]) -> Option<Self> {
                b.get(..$n)?.try_into().ok().map(<$t>::from_be_bytes)
            }
        }
    };
}
be!(u8, 1);
be!(u16, 2);
be!(u32, 4);
be!(i16, 2);

/// A 24-bit unsigned value, which `cmap` format 14 stores codepoints in.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct U24(pub u32);

impl FromBe for U24 {
    const SIZE: Len = Len(3);
    fn parse_be(b: &[u8]) -> Option<Self> {
        let b = b.get(..3)?;
        Some(U24(u32::from_be_bytes([0, b[0], b[1], b[2]])))
    }
}

/// A view over a byte slice; every accessor is bounds-checked.
#[derive(Clone, Copy)]
pub struct Bytes<'a>(&'a [u8]);

impl<'a> Bytes<'a> {
    pub fn new(data: &'a [u8]) -> Bytes<'a> {
        Bytes(data)
    }

    pub fn len(self) -> Len {
        Len(u32::try_from(self.0.len()).unwrap_or(u32::MAX))
    }

    pub fn is_empty(self) -> bool {
        self.0.is_empty()
    }

    /// One value at `at`, or `None` if any byte of it lies past the end.
    pub fn read<T: FromBe>(self, at: Offset) -> Option<T> {
        T::parse_be(self.0.get(at.index()..)?)
    }

    /// `len` bytes from `at`, exactly.
    pub fn slice(self, at: Offset, len: Len) -> Option<Bytes<'a>> {
        let end = at.plus(len)?;
        self.0.get(at.index()..end.index()).map(Bytes)
    }

    /// Everything from `at` to the end; `None` only when `at` is past it.
    pub fn tail(self, at: Offset) -> Option<Bytes<'a>> {
        self.0.get(at.index()..).map(Bytes)
    }

    /// `count` records of `T` starting at `at`, all of them inside the view.
    pub fn array<T: FromBe>(self, at: Offset, count: Count) -> Option<Array<'a, T>> {
        let data = self.slice(at, T::SIZE.times(count)?)?;
        Some(Array {
            data,
            count,
            _t: PhantomData,
        })
    }
}

/// A fixed-count array of big-endian values, read on demand.
#[derive(Clone, Copy)]
pub struct Array<'a, T> {
    data: Bytes<'a>,
    count: Count,
    _t: PhantomData<T>,
}

impl<'a, T: FromBe> Array<'a, T> {
    pub fn count(&self) -> Count {
        self.count
    }

    pub fn get(&self, i: Index) -> Option<T> {
        if !self.count.holds(i) {
            return None;
        }
        self.data.read(Offset::ZERO.at(i, T::SIZE)?)
    }

    /// The first index whose element is `Equal` under `f`, or the index of the
    /// first element that is `Greater` -- the search `cmap` formats 4 and 12
    /// both need: the first end code at or above the character.
    ///
    /// **Not `binary_search_by` from `core`**, which needs a slice of `T` and
    /// this array is bytes. The loop is the same shape; an unreadable element
    /// -- impossible once `array` has bounded the slice, kept as a `None`
    /// rather than an `unwrap` -- ends the search with nothing.
    pub fn partition_point(&self, mut f: impl FnMut(&T) -> Ordering) -> Option<Index> {
        let mut lo = 0u32;
        let mut hi = self.count.0;
        while lo < hi {
            let mid = lo + (hi - lo) / 2;
            match f(&self.get(Index(mid))?) {
                Ordering::Less => lo = mid + 1,
                _ => hi = mid,
            }
        }
        Some(Index(lo))
    }
}
