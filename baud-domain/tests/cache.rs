//! `store_stride` is the one formula in the cache abstraction, and it is
//! pure, so it is checked exhaustively rather than by example.
//!
//! Two claims it must hold, for every plausible line size. The pitch is at
//! least the minimum width and its byte length is an *odd* multiple of the
//! line, so it is a multiple of **no** power-of-two way size and no page
//! size. And its step through the sets spreads consecutive rows' starts as
//! well as the dense live-width layout does, for every height and width
//! that fits an eight-way L1 -- the property the measurement said matters,
//! with the two pitches that fail it kept as the controls.

use baud_domain::{
    CacheGeometry, CacheLevel, CacheSize, Cols, LineSize, Stride, Ways, store_stride,
};

const CELL: usize = 8;
const MAX_COLS: usize = 2048;

fn geom(line: usize) -> CacheGeometry {
    CacheGeometry::new(Some(LineSize::of(line)), None, None, None)
}

/// Every line size a real data cache uses, and the pow2 ways and set counts
/// that make up any real L1/L2/L3, and both page sizes baud runs under.
const LINES: [usize; 3] = [64, 128, 256];
const SETS: [usize; 8] = [32, 64, 128, 256, 512, 1024, 2048, 4096];
const PAGES: [usize; 3] = [4096, 16384, 65536];

#[test]
fn the_pitch_is_an_odd_multiple_of_the_line_and_at_least_the_width() {
    for line in LINES {
        let s = store_stride(&geom(line), Cols::of_index(MAX_COLS));
        assert!(
            s.index() >= MAX_COLS,
            "line {line}: {} < {MAX_COLS}",
            s.index()
        );
        assert_eq!(s.bytes(), s.index() * CELL);
        assert_eq!(s.bytes() % line, 0, "line {line}: pitch not line-aligned");
        assert_eq!(
            (s.bytes() / line) % 2,
            1,
            "line {line}: pitch {} bytes is an even number of lines",
            s.bytes()
        );
    }
}

/// The property the whole design rests on: a multiple of no way at any
/// level and of no page. Walked over every power-of-two geometry.
#[test]
fn the_pitch_aliases_no_power_of_two_way_and_no_page() {
    for line in LINES {
        let s = store_stride(&geom(line), Cols::of_index(MAX_COLS));
        for sets in SETS {
            let way = sets * line;
            assert_ne!(
                s.bytes() % way,
                0,
                "line {line}: pitch {} is a multiple of a {sets}-set way ({way} B)",
                s.bytes()
            );
        }
        for page in PAGES {
            assert_ne!(
                s.bytes() % page,
                0,
                "line {line}: pitch {} is a multiple of a {page} B page",
                s.bytes()
            );
        }
    }
}

/// On this class of machine -- 128 B line, 128 KB 8-way L1, so a 16 KB way
/// of 128 sets -- consecutive rows step 23 sets, the pad the search chose.
/// Not one: one set a row stacks a thirteen-line row over its twelve
/// predecessors and measured 10% down, the same as the aliasing pitch.
#[test]
fn on_a_128_byte_line_consecutive_rows_step_23_sets() {
    let s = store_stride(&geom(128), Cols::of_index(MAX_COLS));
    assert_eq!(s.index(), 2416, "MAX_COLS plus 23 lines of 128 B");
    let way = 128 * 1024 / 8;
    let step_bytes = s.bytes() % way;
    assert_eq!(step_bytes / 128, 23, "the searched step");
}

/// The maximum number of lines any one set of a `way`-byte, `line`-byte-line
/// cache holds when `rows` rows of `cols` cells are laid out at `pitch`
/// bytes a row: the quantity an associativity has to cover.
fn deepest_set(pitch: usize, line: usize, way: usize, cols: usize, rows: usize) -> usize {
    let sets = way / line;
    let per_row = (cols * CELL).div_ceil(line);
    let mut depth = vec![0usize; sets];
    for r in 0..rows {
        let start = r * pitch / line;
        for j in 0..per_row {
            depth[(start + j) % sets] += 1;
        }
    }
    depth.into_iter().max().unwrap_or(0)
}

/// **The table is what the search found, and the search is what the
/// measurement generalises.** For every line size, every L1 way from 4 to
/// 32 KB, three widths and every height to 150 rows: wherever the dense
/// live-width layout keeps every set within eight lines, count the
/// configurations where the padded layout does not, and the deepest it
/// goes. The counts are pinned to `scripts/stride-pad-search.py`'s result,
/// and the two pitches that lost on the bench are scored by the same rule
/// so the bound cannot be vacuous.
#[test]
fn the_row_starts_spread_wherever_the_dense_grid_fits() {
    let ways = [4096usize, 8192, 16384, 32768];
    let widths = [80usize, 200, 400];
    let pinned = [(64usize, 15usize, 10usize), (128, 9, 9), (256, 1, 9)];
    for (line, want_over, want_deepest) in pinned {
        let fixed = store_stride(&geom(line), Cols::of_index(MAX_COLS)).bytes();
        // The two controls: one line past MAX_COLS (the first pitch this
        // repo shipped, step one), and MAX_COLS itself (step zero).
        let one_line = ((MAX_COLS * CELL).div_ceil(line) | 1) * line;
        let pad0 = MAX_COLS * CELL;
        let (mut fits, mut over, mut deepest) = (0, 0, 0);
        let (mut over_one_line, mut over_pad0) = (0, 0);
        for way in ways {
            for cols in widths {
                for rows in 1..=150 {
                    if deepest_set(cols * CELL, line, way, cols, rows) > 8 {
                        continue;
                    }
                    fits += 1;
                    let d = deepest_set(fixed, line, way, cols, rows);
                    deepest = deepest.max(d);
                    over += usize::from(d > 8);
                    over_one_line += usize::from(deepest_set(one_line, line, way, cols, rows) > 8);
                    over_pad0 += usize::from(deepest_set(pad0, line, way, cols, rows) > 8);
                }
            }
        }
        assert!(
            fits > 700,
            "line {line}: the sweep reached {fits} geometries"
        );
        assert_eq!(
            (over, deepest),
            (want_over, want_deepest),
            "line {line}: the pad's profile moved -- re-run the search before re-pinning"
        );
        assert!(
            over_one_line > 10 * over.max(1),
            "line {line}: one line past MAX_COLS fails only {over_one_line} against the pad's {over}"
        );
        assert!(
            over_pad0 > over_one_line,
            "line {line}: MAX_COLS itself fails only {over_pad0} against one line's {over_one_line}"
        );
    }
}

/// The pathology, named: the width itself is the aliasing pitch. This is
/// what the formula exists to never produce.
#[test]
fn max_cols_itself_is_the_aliasing_pitch_the_formula_avoids() {
    let bad = Stride::of_cells(MAX_COLS);
    let way = 128 * 1024 / 8;
    assert_eq!(bad.bytes() % way, 0, "2048 cells is exactly one 16 KB way");
    let good = store_stride(&geom(128), Cols::of_index(MAX_COLS));
    assert_ne!(good.bytes() % way, 0);
}

/// Unknown geometry falls back to the smallest plausible line and is still
/// safe -- an odd multiple of 64 is never an even multiple of 128 or 256.
#[test]
fn unknown_geometry_falls_back_safely() {
    let s = store_stride(&CacheGeometry::UNKNOWN, Cols::of_index(MAX_COLS));
    assert_eq!(s.index(), 3896, "MAX_COLS plus 231 lines of 64 B");
    for line in LINES {
        for sets in SETS {
            assert_ne!(
                s.bytes() % (sets * line),
                0,
                "fallback aliases a {line}x{sets} way"
            );
        }
    }
    assert_eq!(CacheGeometry::UNKNOWN.line(), LineSize::FALLBACK);
}

/// A reported line no real data cache has -- too small, too large, or in
/// between -- is treated as unknown rather than trusted: a probe misreading
/// must not become a pitch the search never scored.
#[test]
fn an_implausible_line_is_not_trusted() {
    for bytes in [8usize, 32, 96, 512] {
        let g = CacheGeometry::new(Some(LineSize::of(bytes)), None, None, None);
        assert_eq!(g.line(), LineSize::FALLBACK, "{bytes} B");
        assert_eq!(g.line_reported(), None, "{bytes} B is not a report");
    }
    let g = CacheGeometry::new(Some(LineSize::of(128)), None, None, None);
    assert_eq!(g.line_reported(), Some(LineSize::of(128)));
}

/// The formula is monotone and holds for narrower minimum widths too, so a
/// caller sizing a small fixed grid gets the same guarantee.
#[test]
fn holds_for_any_minimum_width() {
    for min in [1usize, 80, 200, 512, 1000, 2048] {
        for line in LINES {
            let s = store_stride(&geom(line), Cols::of_index(min));
            assert!(s.index() >= min);
            assert_eq!((s.bytes() / line) % 2, 1, "min {min} line {line}");
        }
    }
}

/// A 512-column terminal is 4096 B a row -- a divisor of every 16 KB way --
/// which the old live-width stride would have re-aliased. The fixed pitch
/// does not depend on the live width, so it cannot.
#[test]
fn the_live_width_hazard_is_gone_because_the_pitch_ignores_it() {
    let live_512 = Stride::of_cells(512);
    assert_eq!(live_512.bytes(), 4096);
    assert_eq!(16384 % live_512.bytes(), 0, "512 cols divides the L1 way");
    let fixed = store_stride(&geom(128), Cols::of_index(MAX_COLS));
    assert_ne!(16384 % fixed.bytes(), 0);
}

/// `CacheLevel::way` is validation a host may run against the chosen pitch.
#[test]
fn a_level_reports_its_way_or_none_without_associativity() {
    let l1 = CacheLevel::new(CacheSize::of(128 * 1024), Ways::of(8));
    assert_eq!(l1.way().map(|w| w.bytes()), Some(16384));
    let unknown_ways = CacheLevel::new(CacheSize::of(128 * 1024), Ways::of(0));
    assert!(unknown_ways.way().is_none());
}
