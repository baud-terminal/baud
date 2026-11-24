//! A write-only view of memory the CPU maps write-combined: a GPU buffer the
//! frame fills and the device reads. A write there is buffered and streamed
//! out; a read is uncached and stalls until the bus answers, so filling such a
//! buffer reads nothing back from it, not even to compare before a store.
//!
//! [`WcSlice`] holds that rule. It lends the slice for writing only: element
//! stores and whole-range copies. It has no read, no index, no `Deref` to the
//! slice and no comparison, so code that writes into mapped memory cannot read
//! it by accident.

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
/// assert_eq!(wc.len(), 4);
/// drop(wc);
/// assert_eq!(mem, [7, 0, 8, 9]);
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
}
