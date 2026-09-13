//! The cache hierarchy a host detected, and the grid stride derived from it.
//!
//! `baud-core` is `no_std` and never probes hardware: a host (`baud-isa`'s
//! per-arch crates, enabled by the app) fills a [`CacheGeometry`] and hands
//! it to [`store_stride`], a pure function that decides the grid's physical
//! row pitch. Everything here is plain arithmetic and is tested exhaustively
//! in `tests/cache.rs`.
//!
//! **Two rules, and the second is the one that was measured.**
//!
//! *An odd number of lines.* Every structure that can alias -- an L1 or L2
//! *way*, a *page* -- has a period that is an **even** multiple of the cache
//! line (a way is `sets * line` with `sets` a power of two; a page is `2^k`
//! lines). A byte pitch that is an **odd** multiple of the line is therefore
//! a multiple of none of them, at any level, without knowing associativity,
//! cache size or page size at all.
//!
//! *And a step that spreads.* That rule says nothing about *how far*
//! consecutive rows sit from each other in set space, and a row is not one
//! line: at 200 columns it is thirteen. One line past `MAX_COLS` -- the
//! minimal odd pitch, 2064 cells -- steps one set a row, so thirteen rows
//! stack into each set of an eight-way L1 while half the sets go unused.
//! Measured on a scrolling ASCII payload it cost **10%** against the dense
//! live-width layout, the same figure as the aliasing pitch it was meant to
//! fix; a step of three sets or more recovered all of it, cells identical.
//!
//! The golden fraction of one period was the first repair and it is not
//! portable: `0.618` of a 16 KB way is `0.47` of a 4 KB one, and rows two
//! apart nearly coincide on the L1 every x86-64 part has. So the pad is a
//! **table found by search** -- `scripts/stride-pad-search.py` lays every
//! odd-total pad against every way from 4 to 32 KB, three widths and every
//! height to 150 rows, and keeps the one that least often stacks a set past
//! eight lines where the dense layout does not. The table's violation
//! profile is pinned in `tests/cache.rs`, with the two pitches that lose as
//! the controls, so it cannot drift from the search.
//!
//! Why any of it matters: striding the grid by `MAX_COLS = 2048` cells put
//! rows exactly one 16 KB L1 way apart on this class of machine, so every
//! row mapped to the same sets and it cost 14% on anything that scrolls.
//! The live width fixed that and made the pitch follow the geometry, so a
//! resize re-packed every row; a pitch that is *fixed* and spreads as well
//! keeps the 14% and moves no cell.

use crate::cell::PackedCell;
use crate::quantity::Cols;

/// A cache line, in bytes. The one quantity the stride is padded against.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
#[repr(transparent)]
pub struct LineSize(usize);

impl LineSize {
    /// The smallest line any real cache uses. Assumed when the host could
    /// not report one: under-estimating keeps the odd-multiple guarantee
    /// (an odd multiple of 64 is never an even multiple of 128), where
    /// over-estimating would not.
    pub const FALLBACK: LineSize = LineSize(64);

    pub fn of(bytes: usize) -> LineSize {
        LineSize(bytes)
    }

    pub fn bytes(self) -> usize {
        self.0
    }
}

/// A cache's capacity, in bytes.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
#[repr(transparent)]
pub struct CacheSize(usize);

impl CacheSize {
    pub fn of(bytes: usize) -> CacheSize {
        CacheSize(bytes)
    }

    pub fn bytes(self) -> usize {
        self.0
    }
}

/// A cache's associativity: how many ways each set holds.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
#[repr(transparent)]
pub struct Ways(usize);

impl Ways {
    pub fn of(n: usize) -> Ways {
        Ways(n)
    }

    pub fn count(self) -> usize {
        self.0
    }
}

/// One way of a cache, in bytes: the period at which addresses alias the
/// same set. `size / ways`. Validation only -- [`store_stride`] does not
/// need it, but a host can check the chosen pitch against it.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
#[repr(transparent)]
pub struct WaySize(usize);

impl WaySize {
    pub fn bytes(self) -> usize {
        self.0
    }
}

/// What a host learned about one level of the hierarchy.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CacheLevel {
    pub size: CacheSize,
    pub ways: Ways,
}

impl CacheLevel {
    pub fn new(size: CacheSize, ways: Ways) -> CacheLevel {
        CacheLevel { size, ways }
    }

    /// The set-aliasing period of this level, or `None` when the
    /// associativity was not reported.
    pub fn way(self) -> Option<WaySize> {
        (self.ways.0 > 0).then(|| WaySize(self.size.0 / self.ways.0))
    }
}

/// The cache hierarchy as a host reported it. Any part may be unknown, and
/// [`CacheGeometry::UNKNOWN`] is a complete, valid geometry: a terminal must
/// never fail to construct because detection did.
///
/// Levels are a fixed three -- L1 data, L2, L3 -- because that is the
/// deepest any target baud runs on has; Apple Silicon reports no L3 and
/// leaves it `None`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CacheGeometry {
    line: Option<LineSize>,
    l1d: Option<CacheLevel>,
    l2: Option<CacheLevel>,
    l3: Option<CacheLevel>,
}

impl CacheGeometry {
    /// Nothing detected. Resolves to [`LineSize::FALLBACK`].
    pub const UNKNOWN: CacheGeometry = CacheGeometry {
        line: None,
        l1d: None,
        l2: None,
        l3: None,
    };

    pub fn new(
        line: Option<LineSize>,
        l1d: Option<CacheLevel>,
        l2: Option<CacheLevel>,
        l3: Option<CacheLevel>,
    ) -> CacheGeometry {
        CacheGeometry { line, l1d, l2, l3 }
    }

    /// The line the stride is padded against: what was reported, if it is
    /// a line a real data cache has -- 64, 128 or 256 bytes -- else the
    /// fallback. Anything else is a probe misreading rather than a cache.
    pub fn line(&self) -> LineSize {
        match self.line {
            Some(l) if PAD_LINES.iter().any(|(k, _)| *k == l) => l,
            _ => LineSize::FALLBACK,
        }
    }

    /// The line as reported, before the fallback: `None` when the host did
    /// not say or said something no cache has, so a log can distinguish
    /// "128" from "64, assumed".
    pub fn line_reported(&self) -> Option<LineSize> {
        self.line.filter(|l| PAD_LINES.iter().any(|(k, _)| k == l))
    }

    pub fn l1d(&self) -> Option<CacheLevel> {
        self.l1d
    }

    pub fn l2(&self) -> Option<CacheLevel> {
        self.l2
    }

    pub fn l3(&self) -> Option<CacheLevel> {
        self.l3
    }
}

/// The grid's physical row pitch, in cells: where row `r` begins is
/// `r * stride`. **Fixed for the life of the storage**, so a resize changes
/// only how many columns of each row are live and moves no cell.
///
/// Distinct from [`Cols`], the live width, on purpose. The two were equal
/// while the pitch followed the width, so swapping them at a call site was
/// invisible; now they diverge, and a swap has to fail to compile.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
#[repr(transparent)]
pub struct Stride(usize);

impl Stride {
    /// A pitch chosen by the caller rather than derived -- for the bench
    /// arms that reproduce the live-width and the aliasing layouts, and for
    /// tests. Not what an app should use; see [`store_stride`].
    pub fn of_cells(n: usize) -> Stride {
        Stride(n.max(1))
    }

    pub fn index(self) -> usize {
        self.0
    }

    /// The pitch in bytes.
    pub fn bytes(self) -> usize {
        self.0 * core::mem::size_of::<PackedCell>()
    }
}

/// The pad past `MAX_COLS`, in lines, per line size: what
/// `scripts/stride-pad-search.py` found. Over every L1 way from 4 to 32 KB,
/// three widths and every height to 150 rows, these stack a set past eight
/// lines where the dense layout does not in 15, 9 and 1 configurations of
/// ~870 each, never deeper than ten; one line stacks thirteen at the corpus
/// geometry, and `MAX_COLS` itself fifty.
const PAD_LINES: [(LineSize, usize); 3] = [
    (LineSize(64), 231),
    (LineSize(128), 23),
    (LineSize(256), 41),
];

/// The pitch of at least `min_cols` cells whose byte length is an odd number
/// of cache lines -- so it is a multiple of no power-of-two way or page --
/// padded by the searched step for the line, so consecutive rows' starts
/// spread over the sets rather than stacking.
///
/// Pure. `min_cols` is the widest grid the storage must address
/// (`MAX_COLS` for a terminal), so the pitch never has to change when the
/// live width does. On a 128-byte line it is 128 + 23 = 151 lines, 2416
/// cells; on the 64-byte fallback 256 + 231 = 487 lines, 3896 cells.
pub fn store_stride(geom: &CacheGeometry, min_cols: Cols) -> Stride {
    let cell = core::mem::size_of::<PackedCell>();
    let line = geom.line();
    let pad = PAD_LINES
        .iter()
        .find(|(l, _)| *l == line)
        .map_or(PAD_LINES[0].1, |(_, p)| *p);
    let line = line.bytes();
    let base = min_cols.index() * cell;
    let mut lines = base.div_ceil(line) + pad;
    if lines.is_multiple_of(2) {
        lines += 1;
    }
    // A line is a whole number of cells on every real machine (64, 128,
    // 256 bytes against an 8-byte cell); the ceiling keeps the guarantee
    // if one ever is not.
    Stride((lines * line).div_ceil(cell))
}
