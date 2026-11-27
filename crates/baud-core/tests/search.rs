//! **Search in the scrollback**: a query's matches over the ring, by
//! canonical caseless match (`[[source: unicode17-ch3]]` D145) over whole
//! clusters (`[[source: uts10]]` § 11), each named by its absolute line so it
//! survives output scrolling and goes when its line retires.

use baud_core::*;
use baud_domain::{Codepoint, LineIdx};

const WS: WinSize = WinSize {
    ws_col: 20,
    ws_row: 5,
};

fn feed(t: &mut Term<'_>, bytes: &[u8]) {
    t.write_at(BytePos::ZERO, bytes);
    t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
}

fn with_depth<R>(depth: usize, f: impl FnOnce(&mut Term<'_>) -> R) -> R {
    let mut owned = Owned::with_history(&WS, &CacheGeometry::UNKNOWN, Depth::of(depth));
    let mut t = Term::new(owned.storage(), &WS);
    f(&mut t)
}

fn needle(q: &str) -> Needle {
    let cps: Vec<Codepoint> = q.chars().map(Codepoint::from_char).collect();
    Needle::new(&cps).expect("a needle")
}

/// Every match of `q` on the live surface, as (line, first col, last line,
/// last col).
fn all(t: &Term<'_>, q: &str) -> Vec<(u64, usize, u64, usize)> {
    let lines = t.search_lines();
    let mut out = Vec::new();
    t.find(&needle(q), lines.oldest, lines.end, |f| {
        out.push((
            f.first.line.value(),
            f.first.col.index(),
            f.last.line.value(),
            f.last.col.index(),
        ))
    });
    out
}

fn top(t: &Term<'_>) -> u64 {
    t.search_lines().top.value()
}

#[test]
fn a_match_on_the_screen_names_its_first_and_last_cells() {
    with_depth(50, |t| {
        feed(t, b"hello world");
        let at = top(t);
        assert_eq!(all(t, "world"), vec![(at, 6, at, 10)]);
        assert_eq!(all(t, "o"), vec![(at, 4, at, 4), (at, 7, at, 7)]);
        assert!(all(t, "worlds").is_empty());
    });
}

#[test]
fn matches_do_not_overlap_and_are_leftmost_first() {
    with_depth(50, |t| {
        feed(t, b"aaaa");
        let at = top(t);
        assert_eq!(all(t, "aa"), vec![(at, 0, at, 1), (at, 2, at, 3)]);
    });
}

#[test]
fn a_match_in_the_history_keeps_its_line_as_output_scrolls() {
    with_depth(50, |t| {
        for i in 0..12 {
            feed(t, format!("line {i}\r\n").as_bytes());
        }
        let found = all(t, "line 3");
        assert_eq!(found.len(), 1);
        let line = LineIdx::of(found[0].0);
        assert!(
            line < t.search_lines().top,
            "line 3 has scrolled into the history"
        );
        let before = t.surface_line(line);
        for i in 12..20 {
            feed(t, format!("line {i}\r\n").as_bytes());
        }
        assert_eq!(
            all(t, "line 3"),
            found,
            "the same line number, the same cells"
        );
        let after = t.surface_line(line);
        match (before, after) {
            (Some(SurfaceLine::Above(a)), Some(SurfaceLine::Above(b))) => {
                assert_eq!(b.index(), a.index() + 8, "eight lines further up")
            }
            other => panic!("{other:?}"),
        }
    });
}

#[test]
fn a_retired_line_is_not_found_and_has_no_place() {
    with_depth(3, |t| {
        feed(t, b"needle\r\n");
        let found = all(t, "needle");
        assert_eq!(found.len(), 1);
        let line = LineIdx::of(found[0].0);
        for i in 0..20 {
            feed(t, format!("x{i}\r\n").as_bytes());
        }
        assert!(all(t, "needle").is_empty());
        assert!(line < t.search_lines().oldest);
        assert_eq!(t.surface_line(line), None);
    });
}

#[test]
fn a_wrapped_line_matches_across_the_wrap_and_a_newline_does_not() {
    with_depth(50, |t| {
        // Eighteen columns of `-`, then `needle` wraps at the twentieth.
        feed(t, b"------------------needle\r\nab\r\ncd");
        let at = top(t);
        assert_eq!(all(t, "needle"), vec![(at, 18, at + 1, 3)]);
        assert_eq!(t.line_start(LineIdx::of(at + 1)), LineIdx::of(at));
        assert!(all(t, "bc").is_empty(), "a newline is not joined");
        // From the second row of the wrapped line, the first is not read.
        let mut from_second = Vec::new();
        t.find(
            &needle("needle"),
            LineIdx::of(at + 1),
            LineIdx::of(at + 2),
            |f| from_second.push(f),
        );
        assert!(from_second.is_empty());
    });
}

#[test]
fn a_line_wrapped_into_the_screen_from_the_history_is_one_line() {
    with_depth(50, |t| {
        feed(t, b"------------------needle\r\n1\r\n2\r\n3\r\n4");
        let found = all(t, "needle");
        assert_eq!(found.len(), 1);
        let (first, last) = (LineIdx::of(found[0].0), LineIdx::of(found[0].2));
        assert_eq!(
            t.surface_line(first),
            Some(SurfaceLine::Above(Rows::of_count(1)))
        );
        assert_eq!(
            t.surface_line(last),
            Some(SurfaceLine::At(Row::from_index(0)))
        );
    });
}

#[test]
fn full_case_folding_matches_sharp_s_whole() {
    with_depth(50, |t| {
        feed(t, "Straße STRASSE".as_bytes());
        let at = top(t);
        assert_eq!(all(t, "strasse"), vec![(at, 0, at, 5), (at, 7, at, 13)]);
        assert_eq!(all(t, "ß"), vec![(at, 4, at, 4), (at, 11, at, 12)]);
        // `s` finds Straße's `S` and STRASSE's three, none inside ß.
        assert_eq!(all(t, "s").len(), 4);
    });
}

#[test]
fn folding_is_the_standards_not_ascii_alone() {
    with_depth(50, |t| {
        // KELVIN SIGN, GREEK CAPITAL SIGMA and final sigma, Cyrillic.
        feed(t, "\u{212A} \u{3A3} \u{3C2} ПРИВЕТ".as_bytes());
        let at = top(t);
        assert_eq!(all(t, "k"), vec![(at, 0, at, 0)]);
        assert_eq!(all(t, "σ").len(), 2);
        assert_eq!(all(t, "привет"), vec![(at, 6, at, 11)]);
    });
}

#[test]
fn canonical_equivalents_match_each_other() {
    with_depth(50, |t| {
        // Precomposed, then decomposed: e and U+0301 share one cell.
        feed(t, "caf\u{e9} cafe\u{301}".as_bytes());
        let at = top(t);
        let both = vec![(at, 0, at, 3), (at, 5, at, 8)];
        assert_eq!(all(t, "café"), both);
        assert_eq!(all(t, "cafe\u{301}"), both);
        assert_eq!(all(t, "CAFÉ"), both);
    });
}

#[test]
fn a_base_never_half_matches_its_cluster() {
    with_depth(50, |t| {
        feed(t, "cafe\u{301} cafe".as_bytes());
        let at = top(t);
        // The first `cafe` ends inside the cluster e + U+0301.
        assert_eq!(all(t, "cafe"), vec![(at, 5, at, 8)]);
        assert_eq!(all(t, "e"), vec![(at, 8, at, 8)]);
    });
}

#[test]
fn a_match_never_starts_inside_a_cluster() {
    with_depth(50, |t| {
        // ß keys as `ss`, so `se` would start on its second `s`; e + U+0301
        // keys as two, so U+0301 x would start on the mark.
        feed(t, "Straße e\u{301}x".as_bytes());
        assert!(all(t, "se").is_empty());
        assert!(all(t, "\u{301}x").is_empty());
        assert_eq!(all(t, "ße").len(), 1);
    });
}

#[test]
fn the_folding_follows_d145_through_ypogegrammeni() {
    with_depth(50, |t| {
        // U+1FB4 decomposes to alpha, acute and U+0345, which folds to
        // iota: NFD, then the folding, then NFD again.
        feed(t, "\u{1FB4}".as_bytes());
        let at = top(t);
        assert_eq!(all(t, "\u{3AC}\u{3B9}"), vec![(at, 0, at, 0)]);
        assert_eq!(all(t, "\u{386}\u{399}"), vec![(at, 0, at, 0)]);
    });
}

#[test]
fn a_wide_character_match_ends_on_its_tail() {
    with_depth(50, |t| {
        feed(t, "日本語のテキスト".as_bytes());
        let at = top(t);
        assert_eq!(all(t, "語の"), vec![(at, 4, at, 7)]);
    });
}

#[test]
fn an_empty_cell_inside_a_line_reads_as_a_space() {
    with_depth(50, |t| {
        // `a`, the cursor five columns on, `b`: five empty cells between.
        feed(t, b"a\x1b[5Cb");
        let at = top(t);
        assert_eq!(all(t, "a     b"), vec![(at, 0, at, 6)]);
        assert!(all(t, "ab").is_empty());
    });
}

#[test]
fn the_alternate_screen_searches_its_rows_alone() {
    with_depth(50, |t| {
        for i in 0..10 {
            feed(t, format!("primary {i}\r\n").as_bytes());
        }
        assert!(!all(t, "primary 1").is_empty());
        feed(t, b"\x1b[?1049h\x1b[Halternate");
        assert!(all(t, "primary").is_empty());
        let lines = t.search_lines();
        assert_eq!(
            lines.oldest, lines.top,
            "no history on the alternate screen"
        );
        assert_eq!(all(t, "alternate").len(), 1);
        feed(t, b"\x1b[?1049l");
        assert_eq!(all(t, "primary 1").len(), 1);
    });
}

#[test]
fn a_query_too_long_or_empty_is_refused() {
    assert_eq!(Needle::new(&[]).err(), Some(NeedleRefused::Empty));
    let long = vec![Codepoint::from_char('a'); NEEDLE_MAX + 1];
    assert_eq!(Needle::new(&long).err(), Some(NeedleRefused::TooLong));
    let fits = vec![Codepoint::from_char('a'); NEEDLE_MAX];
    assert!(Needle::new(&fits).is_ok());
}

/// **A mark stored alone is found as the mark and not as a space**: after
/// a control it is a cluster of its own with no base (`Content::DEFECTIVE`,
/// `[[source: uax29]]` GB4/GB5), and a copy returns the mark, so a search
/// reads the mark there too. The control is the same line with a real
/// space in that cell, which a space finds.
#[test]
fn a_mark_stored_alone_is_searched_as_the_mark_and_not_as_a_space() {
    with_depth(50, |t| {
        feed(t, "e\x1b[m\u{301}y".as_bytes());
        let at = top(t);
        assert!(all(t, " ").is_empty(), "no space was written");
        assert_eq!(all(t, "\u{301}"), vec![(at, 1, at, 1)]);
        assert_eq!(all(t, "\u{301}y"), vec![(at, 1, at, 2)]);
        assert!(all(t, "ey").is_empty(), "the mark's cell is between them");
    });
    with_depth(50, |t| {
        feed(t, b"e y");
        let at = top(t);
        assert_eq!(all(t, " "), vec![(at, 1, at, 1)], "the control");
    });
}
