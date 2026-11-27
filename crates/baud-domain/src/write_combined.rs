//! A write-only view of memory the CPU maps write-combined: a GPU buffer the
//! frame fills and the device reads. A write there is buffered and streamed
//! out; a read is uncached and stalls until the bus answers, so filling such a
//! buffer reads nothing back from it, not even to compare before a store.
//!
//! [`WcSlice`] holds that rule. It lends the slice for writing only: element
//! stores and whole-range copies. It has no read, no index, no `Deref` to the
//! slice and no comparison, so code that writes into mapped memory cannot read
//! it by accident.
//!
//! [`WriteOnly`] is the same surface as a trait, for code that writes lent
//! memory without knowing how it is mapped: a caller lends a [`WcSlice`] over
//! write-combined memory or a plain `&mut [T]` over cached memory, and code
//! generic over `W: WriteOnly<T>` can do with either only what `WcSlice`
//! allows.

use core::ops::Range;

/// Mapped write-combined memory, writable and never read.
///
/// No index, so an element cannot be read back:
///
/// ```compile_fail,E0608
/// let mut mem = [0u32; 4];
/// let wc = baud_domain::WcSlice::new(&mut mem);
/// let _ = wc[0];
/// ```
///
/// No iterator, `get` or slice borrow either:
///
/// ```compile_fail,E0599
/// let mut mem = [0u32; 4];
/// let wc = baud_domain::WcSlice::new(&mut mem);
/// let _ = wc.iter();
/// ```
///
/// No comparison, which would read both sides:
///
/// ```compile_fail,E0369
/// let mut a = [0u32; 4];
/// let mut b = [0u32; 4];
/// let _ = baud_domain::WcSlice::new(&mut a) == baud_domain::WcSlice::new(&mut b);
/// ```
///
/// Writing is the legal path:
///
/// ```
/// let mut mem = [0u32; 4];
/// let mut wc = baud_domain::WcSlice::new(&mut mem);
/// wc.set(0, 7);
/// wc.slice(2..4).copy_from(&[8, 9]);
/// wc.slice(1..2).fill(6);
/// assert_eq!(wc.len(), 4);
/// drop(wc);
/// assert_eq!(mem, [7, 6, 8, 9]);
/// ```
pub struct WcSlice<'a, T> {
    dst: &'a mut [T],
}

impl<'a, T> WcSlice<'a, T> {
    /// Lends `dst` for writing only, for as long as the view lives.
    #[inline]
    pub fn new(dst: &'a mut [T]) -> WcSlice<'a, T> {
        WcSlice { dst }
    }

    /// The number of elements the view can write.
    #[inline]
    pub fn len(&self) -> usize {
        self.dst.len()
    }

    /// Whether the view has no element to write.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.dst.is_empty()
    }

    /// Stores `v` at `i`. Panics if `i` is past the end, as a slice store
    /// does.
    #[inline]
    pub fn set(&mut self, i: usize, v: T) {
        self.dst[i] = v;
    }

    /// The elements in `r`, as a write-only view of their own. Panics if `r`
    /// is out of bounds, as slicing does.
    #[inline]
    pub fn slice(&mut self, r: Range<usize>) -> WcSlice<'_, T> {
        WcSlice {
            dst: &mut self.dst[r],
        }
    }
}

impl<T: Copy> WcSlice<'_, T> {
    /// Stores all of `src`, which must be as long as the view: one copy,
    /// written front to back. Panics if the lengths differ, as
    /// `copy_from_slice` does.
    #[inline]
    pub fn copy_from(&mut self, src: &[T]) {
        self.dst.copy_from_slice(src);
    }

    /// Stores `v` in every element, front to back.
    #[inline]
    pub fn fill(&mut self, v: T) {
        self.dst.fill(v);
    }
}

/// Lent memory a caller may write and never read: [`WcSlice`]'s surface,
/// for code generic over how the memory is mapped. Nothing here returns a
/// `T`, a `&T` or a `&[T]`.
///
/// Code generic over `W` cannot index it:
///
/// ```compile_fail,E0608
/// fn read<W: baud_domain::WriteOnly<u32>>(w: &mut W) -> u32 {
///     w[0]
/// }
/// ```
///
/// nor iterate or borrow it as a slice:
///
/// ```compile_fail,E0599
/// fn read<W: baud_domain::WriteOnly<u32>>(w: &mut W) {
///     let _ = w.iter();
/// }
/// ```
///
/// Writing is the legal path, through a `WcSlice` and a plain slice alike:
///
/// ```
/// use baud_domain::{WcSlice, WriteOnly};
/// fn row<W: WriteOnly<u32>>(w: &mut W) {
///     w.fill(1);
///     w.slice(1..3).copy_from(&[2, 3]);
///     w.set(3, 4);
/// }
/// let mut a = [0u32; 4];
/// row(&mut WcSlice::new(&mut a));
/// let mut b = [0u32; 4];
/// row(&mut &mut b[..]);
/// assert_eq!(a, [1, 2, 3, 4]);
/// assert_eq!(a, b);
/// ```
pub trait WriteOnly<T> {
    /// A narrower write-only view, from [`WriteOnly::slice`].
    type Part<'b>: WriteOnly<T>
    where
        Self: 'b;

    /// The number of elements the view can write.
    fn len(&self) -> usize;

    /// Whether the view has no element to write.
    #[inline]
    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Stores `v` at `i`. Panics if `i` is past the end.
    fn set(&mut self, i: usize, v: T);

    /// The elements in `r`, as a write-only view of their own. Panics if `r`
    /// is out of bounds.
    fn slice(&mut self, r: Range<usize>) -> Self::Part<'_>;

    /// Stores all of `src`, which must be as long as the view. Panics if the
    /// lengths differ.
    fn copy_from(&mut self, src: &[T])
    where
        T: Copy;

    /// Stores `v` in every element.
    fn fill(&mut self, v: T)
    where
        T: Copy;
}

impl<T> WriteOnly<T> for WcSlice<'_, T> {
    type Part<'b>
        = WcSlice<'b, T>
    where
        Self: 'b;

    #[inline]
    fn len(&self) -> usize {
        WcSlice::len(self)
    }

    #[inline]
    fn set(&mut self, i: usize, v: T) {
        WcSlice::set(self, i, v);
    }

    #[inline]
    fn slice(&mut self, r: Range<usize>) -> WcSlice<'_, T> {
        WcSlice::slice(self, r)
    }

    #[inline]
    fn copy_from(&mut self, src: &[T])
    where
        T: Copy,
    {
        WcSlice::copy_from(self, src);
    }

    #[inline]
    fn fill(&mut self, v: T)
    where
        T: Copy,
    {
        WcSlice::fill(self, v);
    }
}

/// Cached memory lent as a plain slice: written through the same surface, so
/// code generic over [`WriteOnly`] reads it no more than a [`WcSlice`].
impl<T> WriteOnly<T> for &mut [T] {
    type Part<'b>
        = &'b mut [T]
    where
        Self: 'b;

    #[inline]
    fn len(&self) -> usize {
        <[T]>::len(self)
    }

    #[inline]
    fn set(&mut self, i: usize, v: T) {
        self[i] = v;
    }

    #[inline]
    fn slice(&mut self, r: Range<usize>) -> &mut [T] {
        &mut self[r]
    }

    #[inline]
    fn copy_from(&mut self, src: &[T])
    where
        T: Copy,
    {
        self.copy_from_slice(src);
    }

    #[inline]
    fn fill(&mut self, v: T)
    where
        T: Copy,
    {
        <[T]>::fill(self, v);
    }
}
