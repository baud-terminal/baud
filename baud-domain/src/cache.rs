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
//! apart nearly coincide on the L1 every x86-64 part has. So the pad became
//! a **table found by search**, which laid every odd-total pad against every
//! way from 4 to 32 KB, three widths and every height to 150 rows, and kept
//! the one that least often stacked a set past eight lines where the dense
//! layout did not. [`store_stride`] says why the dense pitch replaced it.
//! `deepest_set` in `tests/cache.rs` is the search's score, pinned with the
//! pitch one line past a way as the control, so the rule cannot drift from it.
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
    /// The line sizes a real data cache uses; a reported line outside them
    /// is not trusted.
    pub const PLAUSIBLE: [LineSize; 3] = [LineSize(64), LineSize(128), LineSize(256)];
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
            Some(l) if LineSize::PLAUSIBLE.contains(&l) => l,
            _ => LineSize::FALLBACK,
        }
    }

    /// The line as reported, before the fallback: `None` when the host did
    /// not say or said something no cache has, so a log can distinguish
    /// "128" from "64, assumed".
    pub fn line_reported(&self) -> Option<LineSize> {
        self.line.filter(|l| LineSize::PLAUSIBLE.contains(l))
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

/// The L1 ways a pitch's set step is kept clear of: 4 to 32 KB, which is
/// every L1 a real machine runs baud on, expressed as their set counts for
/// each line size the way `store_stride` walks them.
const WAYS: [usize; 4] = [4096, 8192, 16384, 32768];

/// How close to a wrap of the sets a row's step may come. One set a row
/// stacks a thirteen-line row over its twelve predecessors and measured 10%
/// down on a scrolling payload, the same as the aliasing pitch itself;
/// three sets a row measured with the dense layout. Two lines further is the
/// most the rule ever moves a pitch.
const CLEAR: usize = 3;

/// The pitch for a grid `min_cols` wide: the width's footprint in cache
/// lines, made odd, stepped clear of every L1 way -- the smallest such.
///
/// **Odd, so it is a multiple of no power-of-two way or page**: no two row
/// starts share a set at any level. **Clear of a wrap by [`CLEAR`] sets**,
/// because the one pitch this repo shipped one line past a way stepped one
/// set a row, stacked thirteen rows into each set of an eight-way L1, and
/// lost 14% on a scrolling payload. **And no wider than that**, because with
/// the grid as the scrollback ring every line of history costs a row of the
/// pitch: the searched totals that replaced the pad -- 105 lines for a
/// 200-column grid on a 128-byte line, 13.4 KB a row -- put 2,000 lines of
/// history at 27 MB, past this box's L2, and the scroll into a cold row
/// measured **0.56x on `flat`** where the dense 13-line pitch measured 0.92x
/// with the same history. At depth 0 the two are equal, so what the wider
/// totals bought was never measurable and what they cost at depth was.
///
/// Pure. `min_cols` is the widest grid the storage must address, the width
/// a caller provisions. On a 128-byte line, 200 columns is 13 lines, 208
/// cells; 400 columns is 25 lines, 400 cells; `MAX_COLS` is 131 lines, 2096
/// cells, because 129 steps one set a row and 131 steps three.
pub fn store_stride(geom: &CacheGeometry, min_cols: Cols) -> Stride {
    let cell = core::mem::size_of::<PackedCell>();
    let line = geom.line().bytes();
    let mut lines = (min_cols.index() * cell).div_ceil(line).max(1);
    if lines.is_multiple_of(2) {
        lines += 1;
    }
    while !steps_clear(lines, line) {
        lines += 2;
    }
    // A line is a whole number of cells on every real machine (64, 128,
    // 256 bytes against an 8-byte cell); the ceiling keeps the guarantee
    // if one ever is not.
    Stride((lines * line).div_ceil(cell))
}

/// Whether a pitch of `lines` lines steps at least [`CLEAR`] sets away from
/// a wrap of every L1 way's sets. A way smaller than `CLEAR * 2` lines
/// cannot be stepped clear and is not asked.
fn steps_clear(lines: usize, line: usize) -> bool {
    WAYS.iter().all(|way| {
        let sets = way / line;
        let step = lines % sets;
        sets < CLEAR * 2 || (CLEAR..=sets - CLEAR).contains(&step)
    })
}
