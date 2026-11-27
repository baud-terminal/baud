//! **The bytes each hop of the pipeline copies**, counted by the stage that
//! copies them. The ingest stage counts pty → grid, the renderer grid →
//! frame and the presenter frame → screen; each holds its own
//! [`CopyCounts`] as a plain value, and a count read on another thread
//! travels there inside the frame slot's data. The only functions that copy
//! on these hops take the count by `&mut`, so a copy is counted where it is
//! made.

/// One hop of the pipeline between the pty and the screen.
///
/// Used with `core`'s prelude alone; `std` is linked only to run the test:
///
/// ```
/// #![no_std]
/// # extern crate std;
/// use baud_domain::CopyHop;
///
/// fn first(hop: CopyHop) -> bool {
///     match hop {
///         CopyHop::PtyToGrid => true,
///         CopyHop::GridToFrame | CopyHop::FrameToScreen => false,
///     }
/// }
/// # fn main() {
/// assert!(first(CopyHop::ALL[0]));
/// # }
/// ```
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CopyHop {
    /// The bytes a read returns, into the grid's cells.
    PtyToGrid,
    /// The grid's rows, into what the frame draws from.
    GridToFrame,
    /// The frame, into what the screen shows.
    FrameToScreen,
}

impl CopyHop {
    /// Every hop, in pipeline order.
    pub const ALL: [CopyHop; 3] = [
        CopyHop::PtyToGrid,
        CopyHop::GridToFrame,
        CopyHop::FrameToScreen,
    ];
}

/// A count of bytes copied. It is made only from the slice a copy wrote, so
/// it is the slice's size in bytes and not its length in elements, and it
/// has no subtraction: a count only grows.
///
/// Used with `core`'s prelude alone; `std` is linked only to run the test:
///
/// ```
/// #![no_std]
/// # extern crate std;
/// use baud_domain::CopiedBytes;
///
/// # fn main() {
/// let row = [0u64; 4];
/// assert_eq!(CopiedBytes::of(&row).get(), 32);
/// assert_eq!(CopiedBytes::ZERO.get(), 0);
/// # }
/// ```
///
/// Named break: the field made `pub` -- a count is made from a number
/// rather than from what was copied, and this witness compiles:
///
/// ```compile_fail,E0423
/// let _ = baud_domain::CopiedBytes(32);
/// ```
///
/// Named break: the field made `pub` -- a count can be read and rewritten
/// in place, and this witness compiles:
///
/// ```compile_fail,E0616
/// let n = baud_domain::CopiedBytes::ZERO;
/// let _ = n.0;
/// ```
///
/// Named break: a `Sub` impl added -- a count can be lowered, and this
/// witness compiles:
///
/// ```compile_fail,E0369
/// let row = [0u8; 4];
/// let n = baud_domain::CopiedBytes::of(&row);
/// let _ = n - n;
/// ```
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Default)]
pub struct CopiedBytes(u64);

impl CopiedBytes {
    /// Nothing copied.
    pub const ZERO: CopiedBytes = CopiedBytes(0);

    /// The bytes of `copied`, the slice a copy wrote.
    pub const fn of<T>(copied: &[T]) -> CopiedBytes {
        CopiedBytes(size_of_val(copied) as u64)
    }

    /// The count in bytes.
    pub const fn get(self) -> u64 {
        self.0
    }

    /// The two counts together, held at `u64::MAX` rather than wrapped.
    const fn plus(self, other: CopiedBytes) -> CopiedBytes {
        CopiedBytes(self.0.saturating_add(other.0))
    }
}

/// Bytes in a megabyte of input, for [`CopyCounts::per_mb`].
const MB: u128 = 1_000_000;

/// A rate: bytes copied per megabyte (10^6 bytes) of input. It is made only
/// by [`CopyCounts::per_mb`] and [`CopyCounts::hop_per_mb`], and it does
/// not add to a [`CopiedBytes`], which is a count.
///
/// Used with `core`'s prelude alone; `std` is linked only to run the test:
///
/// ```
/// #![no_std]
/// # extern crate std;
/// use baud_domain::{CopiedBytes, CopiedPerMb, CopyCounts, CopyHop};
///
/// # fn main() {
/// let row = [0u8; 3];
/// let mut counts = CopyCounts::NONE;
/// counts.add(CopyHop::FrameToScreen, CopiedBytes::of(&row));
/// let rate = counts.per_mb(2_000_000);
/// assert_eq!(rate.map(CopiedPerMb::get), Some(2));
/// assert_eq!(CopyCounts::NONE.per_mb(1), Some(CopiedPerMb::ZERO));
/// # }
/// ```
///
/// Named break: an `Add<CopiedPerMb>` impl for `CopiedBytes` -- a rate is
/// summed into a count, and this witness compiles:
///
/// ```compile_fail,E0369
/// let row = [0u8; 4];
/// let n = baud_domain::CopiedBytes::of(&row);
/// let rate: baud_domain::CopiedPerMb = baud_domain::CopyCounts::NONE.per_mb(1).unwrap();
/// let _ = n + rate;
/// ```
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Default)]
pub struct CopiedPerMb(u64);

impl CopiedPerMb {
    /// Nothing copied per megabyte.
    pub const ZERO: CopiedPerMb = CopiedPerMb(0);

    /// The rate in bytes copied per megabyte of input.
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// The bytes each hop has copied. Each stage holds one for the hops it
/// copies on, adds to it at each copy, and reads it through [`of`] and
/// [`total`]; two stages' counts are joined with [`plus`].
///
/// [`add`] saturates: a count held at `u64::MAX` still reads as a copy,
/// where a wrapped one could read as none and pass a bound of zero.
///
/// [`of`]: CopyCounts::of
/// [`total`]: CopyCounts::total
/// [`plus`]: CopyCounts::plus
/// [`add`]: CopyCounts::add
///
/// Used with `core`'s prelude alone; `std` is linked only to run the test:
///
/// ```
/// #![no_std]
/// # extern crate std;
/// use baud_domain::{CopiedBytes, CopiedPerMb, CopyCounts, CopyHop};
///
/// # fn main() {
/// let row = [0u64; 4];
/// let mut frame = CopyCounts::NONE;
/// frame.add(CopyHop::GridToFrame, CopiedBytes::of(&row));
/// let mut ingest = CopyCounts::NONE;
/// ingest.add(CopyHop::PtyToGrid, CopiedBytes::of(&row[..1]));
/// let both = frame.plus(ingest);
/// assert_eq!(both.of(CopyHop::GridToFrame).get(), 32);
/// assert_eq!(both.of(CopyHop::PtyToGrid).get(), 8);
/// assert_eq!(both.of(CopyHop::FrameToScreen), CopiedBytes::ZERO);
/// assert_eq!(both.total().get(), 40);
/// assert_eq!(both.per_mb(4_000_000).map(CopiedPerMb::get), Some(10));
/// assert_eq!(both.hop_per_mb(CopyHop::PtyToGrid, 16_000_000).map(CopiedPerMb::get), Some(1));
/// assert_eq!(both.per_mb(0), None);
/// # }
/// ```
///
/// Named break: the fields made `pub` -- a stage can set another hop's
/// count to any value, and this witness compiles:
///
/// ```compile_fail,E0451
/// let z = baud_domain::CopiedBytes::ZERO;
/// let _ = baud_domain::CopyCounts {
///     pty_to_grid: z,
///     grid_to_frame: z,
///     frame_to_screen: z,
/// };
/// ```
///
/// Named break: the fields made `pub` -- a count is read past its
/// accessor, and this witness compiles:
///
/// ```compile_fail,E0616
/// let counts = baud_domain::CopyCounts::NONE;
/// let _ = counts.pty_to_grid;
/// ```
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct CopyCounts {
    pty_to_grid: CopiedBytes,
    grid_to_frame: CopiedBytes,
    frame_to_screen: CopiedBytes,
}

impl CopyCounts {
    /// No hop has copied.
    pub const NONE: CopyCounts = CopyCounts {
        pty_to_grid: CopiedBytes::ZERO,
        grid_to_frame: CopiedBytes::ZERO,
        frame_to_screen: CopiedBytes::ZERO,
    };

    fn hop_mut(&mut self, hop: CopyHop) -> &mut CopiedBytes {
        match hop {
            CopyHop::PtyToGrid => &mut self.pty_to_grid,
            CopyHop::GridToFrame => &mut self.grid_to_frame,
            CopyHop::FrameToScreen => &mut self.frame_to_screen,
        }
    }

    /// Counts `bytes` copied on `hop`, held at `u64::MAX`.
    pub fn add(&mut self, hop: CopyHop, bytes: CopiedBytes) {
        let n = self.hop_mut(hop);
        *n = n.plus(bytes);
    }

    /// The bytes `hop` has copied.
    pub const fn of(&self, hop: CopyHop) -> CopiedBytes {
        match hop {
            CopyHop::PtyToGrid => self.pty_to_grid,
            CopyHop::GridToFrame => self.grid_to_frame,
            CopyHop::FrameToScreen => self.frame_to_screen,
        }
    }

    /// The bytes every hop has copied, summed.
    pub const fn total(&self) -> CopiedBytes {
        self.pty_to_grid
            .plus(self.grid_to_frame)
            .plus(self.frame_to_screen)
    }

    /// Both stages' counts, hop by hop.
    #[must_use]
    pub const fn plus(self, other: CopyCounts) -> CopyCounts {
        CopyCounts {
            pty_to_grid: self.pty_to_grid.plus(other.pty_to_grid),
            grid_to_frame: self.grid_to_frame.plus(other.grid_to_frame),
            frame_to_screen: self.frame_to_screen.plus(other.frame_to_screen),
        }
    }

    /// The bytes every hop copied per megabyte (10^6 bytes) of `bytes_in`,
    /// summed over the hops; `None` when no byte came in. Rounded up, so a
    /// hop that copied anything never reads as zero copies.
    pub const fn per_mb(&self, bytes_in: u64) -> Option<CopiedPerMb> {
        per_mb(self.total(), bytes_in)
    }

    /// The bytes `hop` copied per megabyte of `bytes_in`, rounded up as
    /// [`per_mb`](CopyCounts::per_mb) is.
    pub const fn hop_per_mb(&self, hop: CopyHop, bytes_in: u64) -> Option<CopiedPerMb> {
        per_mb(self.of(hop), bytes_in)
    }
}

const fn per_mb(copied: CopiedBytes, bytes_in: u64) -> Option<CopiedPerMb> {
    if bytes_in == 0 {
        return None;
    }
    let scaled = (copied.0 as u128 * MB).div_ceil(bytes_in as u128);
    Some(CopiedPerMb(if scaled > u64::MAX as u128 {
        u64::MAX
    } else {
        scaled as u64
    }))
}

#[cfg(test)]
mod tests {
    use super::{CopiedBytes, CopiedPerMb, CopyCounts, CopyHop};

    /// Named break: `plus` made `wrapping_add` -- a count past `u64::MAX`
    /// reads as a few bytes, and the hop's assertion fails.
    #[test]
    fn a_count_at_its_ceiling_stays_there() {
        let mut counts = CopyCounts::NONE;
        counts.add(CopyHop::FrameToScreen, CopiedBytes(u64::MAX));
        counts.add(CopyHop::FrameToScreen, CopiedBytes(2));
        assert_eq!(counts.of(CopyHop::FrameToScreen).get(), u64::MAX);
        let mut other = CopyCounts::NONE;
        other.add(CopyHop::PtyToGrid, CopiedBytes(1));
        assert_eq!(counts.plus(other).total().get(), u64::MAX);
        assert_eq!(
            counts.plus(counts).of(CopyHop::FrameToScreen).get(),
            u64::MAX
        );
        assert_eq!(counts.per_mb(1).map(CopiedPerMb::get), Some(u64::MAX));
    }

    /// Named break: `div_ceil` made a plain division -- one byte copied
    /// over a gigabyte reads as zero per megabyte, and the assertion fails.
    #[test]
    fn a_copy_never_rounds_to_none_per_mb() {
        let mut counts = CopyCounts::NONE;
        counts.add(CopyHop::GridToFrame, CopiedBytes(1));
        assert_eq!(counts.per_mb(1_000_000_000).map(CopiedPerMb::get), Some(1));
        assert_eq!(
            counts
                .hop_per_mb(CopyHop::GridToFrame, 1_000_000_000)
                .map(CopiedPerMb::get),
            Some(1)
        );
        assert_eq!(
            counts
                .hop_per_mb(CopyHop::PtyToGrid, 1_000_000_000)
                .map(CopiedPerMb::get),
            Some(0)
        );
        assert_eq!(CopyCounts::NONE.per_mb(0), None);
    }

    /// Named break: two arms of `hop_mut` swapped -- an add lands on
    /// another hop, and the hop's read misses it.
    #[test]
    fn each_hop_counts_only_its_own_copies() {
        for (n, hop) in CopyHop::ALL.into_iter().enumerate() {
            let mut counts = CopyCounts::NONE;
            counts.add(hop, CopiedBytes(n as u64 + 1));
            for other in CopyHop::ALL {
                let want = if other == hop { n as u64 + 1 } else { 0 };
                assert_eq!(counts.of(other).get(), want, "{hop:?} read as {other:?}");
            }
            assert_eq!(counts.total().get(), n as u64 + 1);
        }
    }
}
