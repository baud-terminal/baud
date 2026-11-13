//! **What a frame did, counted where it does it**: the cells it compared,
//! copied and converted, the rows it laid out for their direction, the
//! cells the overlay built records for, the cells scanned for pictures, the
//! link records placed, the image tables built, the instances tallied
//! for the frame's totals, and the displays the re-cut laid out and the
//! rows' generations it read.
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
//!
//! **Each count is kept by the [`Site`] that did the work** as well as in
//! all: several parts of a frame compare and copy cells -- the copy of the
//! screen, the CPU path's conversion, the layout, a kept row --
//! and a row that brings one of them down must read that one alone
//! ([`Counts::at`]), not a total the others can hide a change in.

use crate::Tally;
use baud_domain::PackedCell;

/// One kind of work a frame does.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Counter {
    /// Cells a compare of two copies of a row may read -- both rows whole,
    /// an upper bound, since a compare stops at the first cell that
    /// differs: the conversion's
    /// against its last frame, the layout's cache against the rows it was
    /// found on, a kept row's lookup -- its
    /// compare and the fold that finds a row kept elsewhere -- and the
    /// cell path's copy against the last frame's.
    CellsCompared,
    /// Cells copied out of the window or the status line: the
    /// conversion's copy, the layout's copy in drawn order and of a row it
    /// tests, the cell path's copy into a set, and the set's copy into the
    /// view a slot is lent in.
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
    /// Displays the re-cut laid out past its cache: a layout of the
    /// window's finished paragraphs at their words that the last one kept
    /// did not answer (`crate::rebreak`).
    DisplaysLaid,
    /// Rows whose write generation the re-cut's kept layout read, to know
    /// whether a row it was laid out from was written since the seal it was
    /// read under (`crate::rebreak`).
    GensRead,
}

impl Counter {
    /// Every counter, in the order a harness prints them.
    pub const ALL: [Counter; 11] = [
        Counter::CellsCompared,
        Counter::CellsCopied,
        Counter::CellsConverted,
        Counter::BidiRowsLaid,
        Counter::OverlayCellsBuilt,
        Counter::PictureCellsScanned,
        Counter::LinkInstancesBuilt,
        Counter::ImageTablesBuilt,
        Counter::InstancesTallied,
        Counter::DisplaysLaid,
        Counter::GensRead,
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
            Counter::DisplaysLaid => "displays_laid",
            Counter::GensRead => "gens_read",
        }
    }
}

/// **Where in a frame a count was made.**
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Site {
    /// The cell path's copy of the window into a frame's set
    /// (`crate::screens`).
    Screens,
    /// The CPU and instance paths' conversion: the window into this path's
    /// own copy, its compare with the last frame's, and the rows converted.
    Convert,
    /// The status line's row converted for the cell path
    /// (`Renderer::status_records`).
    Status,
    /// The cell path's sized lines (`Renderer::sized_lines`): a line's
    /// cells taken into the copy it is converted from, and a line keyed by
    /// its cells compared with its key.
    Sized,
    /// The bidirectional layout (`crate::bidi`).
    Layout,
    /// The re-cut of finished paragraphs (`crate::rebreak`).
    Rebreak,
    /// A kept row's lookup (`crate::kept`).
    Kept,
    /// The overlay's walk (`crate::overlay`).
    Overlay,
    /// The picture finder (`crate::picture`, and the overlay's mask of it).
    Pictures,
    /// The link records.
    Links,
    /// The image placements' tables.
    Images,
    /// The frame's totals of its instances (`crate::per_row`).
    Totals,
    /// The cell path's lent view: the frame's set with every rendition
    /// a struck link or a frozen cell names lent a slot
    /// (`crate::cell_frame`).
    Lent,
}

impl Site {
    /// Every site, in the order a harness prints them.
    pub const ALL: [Site; 13] = [
        Site::Screens,
        Site::Convert,
        Site::Status,
        Site::Sized,
        Site::Layout,
        Site::Rebreak,
        Site::Kept,
        Site::Overlay,
        Site::Pictures,
        Site::Links,
        Site::Images,
        Site::Totals,
        Site::Lent,
    ];

    /// The site's name, as a harness prints it.
    pub const fn name(self) -> &'static str {
        match self {
            Site::Screens => "screens",
            Site::Convert => "convert",
            Site::Status => "status",
            Site::Sized => "sized",
            Site::Layout => "layout",
            Site::Rebreak => "rebreak",
            Site::Kept => "kept",
            Site::Overlay => "overlay",
            Site::Pictures => "pictures",
            Site::Links => "links",
            Site::Images => "images",
            Site::Totals => "totals",
            Site::Lent => "lent",
        }
    }
}

/// `n` more of `counter`'s work on this thread, done at `site`; nothing
/// without the `counters` feature.
#[inline(always)]
pub fn add(counter: Counter, site: Site, n: Tally) {
    #[cfg(feature = "counters")]
    on::add(counter, site, n);
    #[cfg(not(feature = "counters"))]
    let _ = (counter, site, n);
}

/// **`a` and `b` compared, the cells the compare may read counted** at
/// `site`: two rows of equal length whole, an upper bound, and two of
/// unequal length none. What every compare of two copies of a row is
/// spelled through, so none goes uncounted; without the `counters`
/// feature, the compare alone.
#[inline(always)]
pub fn same_cells(site: Site, a: &[PackedCell], b: &[PackedCell]) -> bool {
    #[cfg(feature = "counters")]
    if a.len() == b.len() {
        add(Counter::CellsCompared, site, Tally::of(a.len()));
    }
    #[cfg(not(feature = "counters"))]
    let _ = site;
    PackedCell::same_cells(a, b)
}

/// **The cells a compare of `a` and `b` may read, counted** at `site`,
/// beside a compare left as it is: both rows whole when both are there and
/// of equal length, an upper bound, and none otherwise. Nothing without
/// the `counters` feature.
#[inline(always)]
pub fn compared(site: Site, a: Option<&[PackedCell]>, b: Option<&[PackedCell]>) {
    #[cfg(not(feature = "counters"))]
    let _ = (site, a, b);
    #[cfg(feature = "counters")]
    if let (Some(a), Some(b)) = (a, b) {
        if a.len() == b.len() {
            add(Counter::CellsCompared, site, Tally::of(a.len()));
        }
    }
}

#[cfg(feature = "counters")]
pub use on::{Counts, take};

#[cfg(feature = "counters")]
mod on {
    use super::{Counter, Site};
    use crate::Tally;
    use core::cell::Cell;

    const COUNTERS: usize = Counter::ALL.len();
    const SITES: usize = Site::ALL.len();

    std::thread_local! {
        static COUNTS: [[Cell<Tally>; COUNTERS]; SITES] =
            const { [const { [const { Cell::new(Tally::ZERO) }; COUNTERS] }; SITES] };
    }

    /// `counter`'s place in a site's row, kept in [`Counter::ALL`]'s order.
    fn slot<T>(of: &[T; COUNTERS], counter: Counter) -> &T {
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
            Counter::DisplaysLaid => &of[9],
            Counter::GensRead => &of[10],
        }
    }

    /// `site`'s row, kept in [`Site::ALL`]'s order.
    fn row<T>(of: &[T; SITES], site: Site) -> &T {
        match site {
            Site::Screens => &of[0],
            Site::Convert => &of[1],
            Site::Status => &of[2],
            Site::Sized => &of[3],
            Site::Layout => &of[4],
            Site::Rebreak => &of[5],
            Site::Kept => &of[6],
            Site::Overlay => &of[7],
            Site::Pictures => &of[8],
            Site::Links => &of[9],
            Site::Images => &of[10],
            Site::Totals => &of[11],
            Site::Lent => &of[12],
        }
    }

    pub(super) fn add(counter: Counter, site: Site, n: Tally) {
        COUNTS.with(|c| {
            let at = slot(row(c, site), counter);
            let mut now = at.get();
            now.add(n);
            at.set(now);
        });
    }

    /// This thread's counts since the last [`take`], by site.
    #[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
    pub struct Counts([[Tally; COUNTERS]; SITES]);

    impl Counts {
        /// `counter`'s count, every site's summed.
        pub fn of(&self, counter: Counter) -> Tally {
            let mut sum = Tally::ZERO;
            for site in &self.0 {
                sum.add(*slot(site, counter));
            }
            sum
        }

        /// `counter`'s count at `site` alone.
        pub fn at(&self, site: Site, counter: Counter) -> Tally {
            *slot(row(&self.0, site), counter)
        }
    }

    /// This thread's counts since the last call, which start again at
    /// zero.
    pub fn take() -> Counts {
        COUNTS.with(|c| {
            let mut out = Counts::default();
            for (o, at) in out.0.iter_mut().zip(c) {
                for (o, at) in o.iter_mut().zip(at) {
                    *o = at.replace(Tally::ZERO);
                }
            }
            out
        })
    }
}
