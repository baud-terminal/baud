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
/// of 128 sets -- a 200-column grid's rows step 13 sets, the footprint
/// itself, and `MAX_COLS`'s step three: not one, because one set a row
/// stacks a thirteen-line row over its twelve predecessors and measured 10%
/// down, the same as the aliasing pitch.
#[test]
fn on_a_128_byte_line_the_step_is_the_footprint_kept_clear_of_a_wrap() {
    let way = 128 * 1024 / 8;
    let s = store_stride(&geom(128), Cols::of_index(200));
    assert_eq!(s.index(), 208, "13 lines of 128 B for 200 columns");
    assert_eq!((s.bytes() % way) / 128, 13);
    let s = store_stride(&geom(128), Cols::of_index(MAX_COLS));
    assert_eq!(s.index(), 2096, "131 lines: 129 would step one set a row");
    assert_eq!((s.bytes() % way) / 128, 3);
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

/// **The pitch is the width's footprint, made odd and stepped clear of every
/// way, and no wider.** For every line size and every width a caller might
/// provision: the pitch is at least the width, an odd number of lines, its
/// step through the sets of every L1 way from 4 to 32 KB is at least three
/// from a wrap, and no narrower pitch has those properties -- because with
/// the grid as the scrollback ring every line of history costs a row of the
/// pitch, and the searched totals this replaced put 2,000 lines at 27 MB,
/// past L2, for a scroll that measured 0.56x on `flat` against the dense
/// pitch's 0.92x with the same history.
#[test]
fn the_pitch_is_the_width_made_odd_and_stepped_clear_of_every_way() {
    let ways = [4096usize, 8192, 16384, 32768];
    let widths = [
        80usize, 120, 160, 200, 240, 320, 400, 512, 640, 800, 1024, 1280, 1600, 2048,
    ];
    let clear = |lines: usize, line: usize| {
        ways.iter().all(|way| {
            let sets = way / line;
            let step = lines % sets;
            sets < 6 || (3..=sets - 3).contains(&step)
        })
    };
    for line in LINES {
        for cols in widths {
            let s = store_stride(&geom(line), Cols::of_index(cols));
            let lines = s.bytes() / line;
            assert!(s.index() >= cols, "line {line}, {cols} columns");
            assert_eq!(lines % 2, 1, "line {line}, {cols} columns: even");
            assert!(
                clear(lines, line),
                "line {line}, {cols} columns: {lines} lines step into a wrap"
            );
            let need = (cols * CELL).div_ceil(line);
            for narrower in (need..lines).filter(|l| l % 2 == 1) {
                assert!(
                    !clear(narrower, line),
                    "line {line}, {cols} columns: {narrower} lines would do and {lines} were chosen"
                );
            }
            assert!(
                lines <= need + 5,
                "line {line}, {cols} columns: {lines} lines for a footprint of {need}"
            );
        }
    }
}

/// The three widths that matter, pinned as cells: 80 columns, the corpus's
/// 200 and the app's 400. The corpus at the searched total was 1680 cells and
/// is 208; the app's 400 columns is exactly its 25 lines.
#[test]
fn the_pitch_at_the_widths_the_corpus_and_the_app_use() {
    let pinned = [
        (64usize, 80usize, 88usize),
        (128, 80, 80),
        (256, 80, 96),
        (64, 200, 200),
        (128, 200, 208),
        (256, 200, 224),
        (64, 400, 408),
        (128, 400, 400),
        (256, 400, 416),
    ];
    for (line, cols, want) in pinned {
        assert_eq!(
            store_stride(&geom(line), Cols::of_index(cols)).index(),
            want,
            "line {line}, {cols} columns"
        );
    }
}

/// The set-depth score the searched totals were chosen by, kept as the
/// instrument that says what the dense pitch gives up: over every way from
/// 4 to 32 KB, the widths up to the base and every height to 150 rows,
/// wherever the dense live-width layout keeps every set within eight lines,
/// the configurations where the pitch does not. The corpus pitch scores 27 of
/// 721 where the searched total scored 3 -- and measured equal to it at no
/// depth and 1.65x it at 2,000 lines. The one pitch the score and the
/// measurement agree is bad, one line past a way, is what `CLEAR` refuses:
/// it scores 294.
#[test]
fn the_dense_pitch_scores_what_the_search_said_and_the_one_past_a_way_is_refused() {
    let ways = [4096usize, 8192, 16384, 32768];
    let score = |pitch: usize, line: usize, widths: &[usize]| {
        let (mut fits, mut over) = (0, 0);
        for way in ways {
            for &cols in widths {
                for rows in 1..=150 {
                    if deepest_set(cols * CELL, line, way, cols, rows) > 8 {
                        continue;
                    }
                    fits += 1;
                    over += usize::from(deepest_set(pitch, line, way, cols, rows) > 8);
                }
            }
        }
        (over, fits)
    };
    let corpus = store_stride(&geom(128), Cols::of_index(200)).bytes();
    assert_eq!(score(corpus, 128, &[80, 200]), (27, 721));
    assert_eq!(
        score(105 * 128, 128, &[80, 200]),
        (3, 721),
        "the searched total"
    );
    let one_past = 129 * 128;
    assert_eq!(
        score(one_past, 128, &[80, 200, 400]),
        (294, 872),
        "one line past MAX_COLS"
    );
    let chosen = store_stride(&geom(128), Cols::of_index(MAX_COLS)).bytes();
    assert_eq!(chosen, 131 * 128);
    // 155 by the score; 0.99x by the sweep at a live width of 200, which is
    // the measurement the score is a proxy for.
    assert_eq!(score(chosen, 128, &[80, 200, 400]), (155, 872));
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
    assert_eq!(
        s.index(),
        2072,
        "259 lines of 64 B: 257 would step one set a row"
    );
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
