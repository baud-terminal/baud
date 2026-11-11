//! **What a frame did, counted where it does it**: the cells it compared,
//! copied and converted, the rows it laid out for their direction, the
//! cells the overlay built records for, the cells scanned for pictures, the
//! link records placed, the image tables built and the instances tallied
//! for the frame's totals.
//!
//! The counts are kept only under the `counters` feature, which nothing
//! that ships enables: each [`add`] is an empty function without it, so the
//! app and the benches carry no store on a per-cell path. A test turns the
//! feature on through a dev-dependency and reads the counts with [`take`],
//! which exists only under it, so a harness built without the feature does
//! not compile rather than reading zeros.
//!
//! The counts are the calling thread's: the renderer and the copy of the
//! screen it draws from run on the thread that draws the frame, and a test
//! reads them there.

use crate::Tally;
use baud_domain::PackedCell;

/// One kind of work a frame does.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Counter {
    /// Cells a compare of two copies of a row may read -- both rows whole,
    /// an upper bound, since a compare stops at the first cell that
    /// differs: the conversion's
    /// against its last frame, the layout's and the re-cut's caches
    /// against the rows they were found on, a kept row's lookup -- its
    /// compare and the fold that finds a row kept elsewhere -- and the
    /// cell path's copy against the last frame's.
    CellsCompared,
    /// Cells copied out of the window or the status line: the
    /// conversion's copy, the layout's copy in drawn order and of a row it
    /// tests, the re-cut's snapshot, and the cell path's copy into a set.
    CellsCopied,
    /// Cells a row's conversion into instances read.
    CellsConverted,
    /// Rows the bidirectional layout resolved or joined.
    BidiRowsLaid,
    /// Cells the overlay's walk visited: its pass over every cell and over
    /// a reordered row, the cells its records draw -- drawn again from the
    /// last frame's (`Painted`) included -- and the cells its ligature
    /// windows gathered.
    OverlayCellsBuilt,
    /// Cells scanned for a picture drawn in text.
    PictureCellsScanned,
    /// Records placed for the link lines and the link under the pointer.
    LinkInstancesBuilt,
    /// Image placements whose sampling tables were built.
    ImageTablesBuilt,
    /// Instances tallied for the frame's totals of them -- all, coloured,
    /// solid -- as a row's are set (`crate::per_row`).
    InstancesTallied,
}

impl Counter {
    /// Every counter, in the order a harness prints them.
    pub const ALL: [Counter; 9] = [
        Counter::CellsCompared,
        Counter::CellsCopied,
        Counter::CellsConverted,
        Counter::BidiRowsLaid,
        Counter::OverlayCellsBuilt,
        Counter::PictureCellsScanned,
        Counter::LinkInstancesBuilt,
        Counter::ImageTablesBuilt,
        Counter::InstancesTallied,
    ];

    /// The counter's name, as a harness prints it.
    pub const fn name(self) -> &'static str {
        match self {
            Counter::CellsCompared => "cells_compared",
            Counter::CellsCopied => "cells_copied",
            Counter::CellsConverted => "cells_converted",
            Counter::BidiRowsLaid => "bidi_rows_laid",
            Counter::OverlayCellsBuilt => "overlay_cells_built",
            Counter::PictureCellsScanned => "picture_cells_scanned",
            Counter::LinkInstancesBuilt => "link_instances_built",
            Counter::ImageTablesBuilt => "image_tables_built",
            Counter::InstancesTallied => "instances_tallied",
        }
    }
}

/// `n` more of `counter`'s work on this thread; nothing without the
/// `counters` feature.
#[inline(always)]
pub fn add(counter: Counter, n: Tally) {
    #[cfg(feature = "counters")]
    on::add(counter, n);
    #[cfg(not(feature = "counters"))]
    let _ = (counter, n);
}

/// **`a` and `b` compared, the cells the compare may read counted**: two
/// rows of equal length whole, an upper bound, and two of unequal length
/// none. What every compare of two copies of a row is spelled through, so
/// none goes uncounted; without the `counters` feature, the compare alone.
#[inline(always)]
pub fn same_cells(a: &[PackedCell], b: &[PackedCell]) -> bool {
    #[cfg(feature = "counters")]
    if a.len() == b.len() {
        add(Counter::CellsCompared, Tally::of(a.len()));
    }
    PackedCell::same_cells(a, b)
}

/// **The cells a compare of `a` and `b` may read, counted**, beside a
/// compare left as it is: both rows whole when both are there and of equal
/// length, an upper bound, and none otherwise. Nothing without the
/// `counters` feature.
#[inline(always)]
pub fn compared(a: Option<&[PackedCell]>, b: Option<&[PackedCell]>) {
    #[cfg(not(feature = "counters"))]
    let _ = (a, b);
    #[cfg(feature = "counters")]
    if let (Some(a), Some(b)) = (a, b) {
        if a.len() == b.len() {
            add(Counter::CellsCompared, Tally::of(a.len()));
        }
    }
}

#[cfg(feature = "counters")]
pub use on::{Counts, take};

#[cfg(feature = "counters")]
mod on {
    use super::Counter;
    use crate::Tally;
    use core::cell::Cell;

    std::thread_local! {
        static COUNTS: [Cell<Tally>; Counter::ALL.len()] =
            const { [const { Cell::new(Tally::ZERO) }; Counter::ALL.len()] };
    }

    /// `counter`'s place in `of`, kept in [`Counter::ALL`]'s order.
    fn slot<T>(of: &[T; Counter::ALL.len()], counter: Counter) -> &T {
        match counter {
            Counter::CellsCompared => &of[0],
            Counter::CellsCopied => &of[1],
            Counter::CellsConverted => &of[2],
            Counter::BidiRowsLaid => &of[3],
            Counter::OverlayCellsBuilt => &of[4],
            Counter::PictureCellsScanned => &of[5],
            Counter::LinkInstancesBuilt => &of[6],
            Counter::ImageTablesBuilt => &of[7],
            Counter::InstancesTallied => &of[8],
        }
    }

    pub(super) fn add(counter: Counter, n: Tally) {
        COUNTS.with(|c| {
            let at = slot(c, counter);
            let mut now = at.get();
            now.add(n);
            at.set(now);
        });
    }

    /// This thread's counts since the last [`take`].
    #[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
    pub struct Counts([Tally; Counter::ALL.len()]);

    impl Counts {
        /// `counter`'s count.
        pub fn of(&self, counter: Counter) -> Tally {
            *slot(&self.0, counter)
        }
    }

    /// This thread's counts since the last call, which start again at
    /// zero.
    pub fn take() -> Counts {
        COUNTS.with(|c| {
            let mut out = Counts::default();
            for (o, at) in out.0.iter_mut().zip(c) {
                *o = at.replace(Tally::ZERO);
            }
            out
        })
    }
}
