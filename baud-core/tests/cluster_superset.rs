//! Every cell the store this replaced kept whole, this one keeps whole.
//!
//! **The superset of the base, cell by cell.** The 63-slot store this
//! replaced (0c0c30a8) refused far more than this one, but a count says
//! nothing about *which* cells: a store that refuses fewer points in all
//! can still lose a cluster the old one kept. A sweep that ran whenever one
//! cluster's eight slots were full did exactly that -- it freed the growing
//! prefixes of clusters still on cells, and when such a cluster was written
//! again its prefix was refused, so 35 cells the base drew right over
//! note AD's texts were drawn without a point (docs/record.md, "The cluster
//! store holds 1024 clusters and sweeps the dead ones"). Sweeping only once
//! every slot is filled, the owner's "when all slots are used", keeps them.
//!
//! The texts that showed it are external (`[[source: hi-bharat]]` and the
//! rest of note AD's session), so this test generates the mechanism instead:
//! clusters of three marks written once, a churn of one-mark clusters kept
//! on the screen that fills the store's windows short of filling the
//! store, and the first clusters written again. Each cell's truth is known
//! by construction. `BASE_RIGHT` is which clusters the base drew whole,
//! taken by running this file on `git archive 0c0c30a8`, where the test
//! prints the set it found before comparing: there the comparison holds
//! and the test fails at the second pass, which the base did not keep
//! whole.

#[path = "support/mod.rs"]
mod support;

use baud_core::{CacheGeometry, Depth, Owned, PackedCell, Row, Term, WinSize};

/// The terminal: note AD's 200x50, with 2,000 lines of history, none of
/// which the payload's twenty lines reach.
const WS: WinSize = WinSize {
    ws_row: 50,
    ws_col: 200,
};

/// Clusters a line, each a letter, its marks and a space.
const PER: usize = 50;

/// The recurring clusters: 24 letters, each with three of the combining
/// diacritical marks, so each cell interns two prefixes before it is whole.
const RECUR: usize = 24;

/// One-mark clusters between the two passes: enough to fill the windows
/// the store's eight probes take, and short of filling all 1024 slots with
/// the recurring clusters' 72.
const CHURN: usize = 900;

/// The `i`th recurring cluster.
fn recurring(i: usize) -> (u32, Vec<u32>) {
    let b = u32::from(b'a') + u32::try_from(i).expect("a letter");
    let m = 0x0300 + 3 * u32::try_from(i).expect("a mark");
    (b, vec![m, m + 1, m + 2])
}

/// The `i`th churn cluster: a Greek or Cyrillic small letter and one mark,
/// none of which a recurring cluster's prefix is.
fn churn(i: usize) -> (u32, Vec<u32>) {
    let bases: Vec<u32> = (0x03B1..=0x03C9).chain(0x0430..=0x044F).collect();
    let b = bases[i % bases.len()];
    let m = 0x0300 + u32::try_from(i / bases.len()).expect("a mark");
    (b, vec![m])
}

/// Every cluster in the order it is sent: the recurring ones, the churn,
/// the recurring ones again.
fn payload() -> Vec<(u32, Vec<u32>)> {
    (0..RECUR)
        .map(recurring)
        .chain((0..CHURN).map(churn))
        .chain((0..RECUR).map(recurring))
        .collect()
}

/// The bytes: `PER` clusters a line, each line ended `CR LF`, and the
/// second pass starting a line of its own.
fn bytes(cs: &[(u32, Vec<u32>)]) -> Vec<u8> {
    let mut out = String::new();
    for (k, (b, m)) in cs.iter().enumerate() {
        out.extend(
            core::iter::once(*b)
                .chain(m.iter().copied())
                .map(|c| char::from_u32(c).expect("a char")),
        );
        let end = (k + 1) % PER == 0 || k + 1 == RECUR + CHURN;
        out.push_str(match end {
            true => "\r\n",
            false => " ",
        });
    }
    out.into_bytes()
}

/// The screen's rows, top first.
fn lines<'t>(t: &'t Term<'_>) -> Vec<&'t [PackedCell]> {
    let mut out = Vec::new();
    let mut y = 0;
    while let Some(cells) = t.row(Row::from_index(y)) {
        out.push(cells);
        y += 1;
    }
    out
}

/// Which clusters the terminal holds whole, by their index in `payload`:
/// each at column `2k` of its line, read through the cell, and compared
/// with what was sent. The base character is the column's, which the
/// layout puts there whatever the store does.
fn whole(t: &Term<'_>, cs: &[(u32, Vec<u32>)]) -> Vec<bool> {
    let all = lines(t);
    // The payload starts at the screen's first row and scrolls nothing:
    // twenty lines on fifty rows.
    let first = t.scrollback().len();
    assert_eq!(first, 0, "nothing scrolled into the history");
    let pass = (RECUR + CHURN).div_ceil(PER);
    let at = (0..cs.len()).map(|i| match i < RECUR + CHURN {
        true => (first + i / PER, i % PER),
        false => {
            let j = i - RECUR - CHURN;
            (first + pass + j / PER, j % PER)
        }
    });
    cs.iter()
        .zip(at)
        .map(|((_, m), (line, k))| {
            let got: Vec<u32> = t.cluster_marks(all[line][2 * k]).collect();
            got == *m
        })
        .collect()
}

/// Ranges of indices, `[from, to)`, for the set as the test prints it.
fn ranges(set: &[bool]) -> Vec<(usize, usize)> {
    let mut out: Vec<(usize, usize)> = Vec::new();
    for (i, &w) in set.iter().enumerate() {
        match (w, out.last_mut()) {
            (true, Some(r)) if r.1 == i => r.1 = i + 1,
            (true, _) => out.push((i, i + 1)),
            (false, _) => {}
        }
    }
    out
}

/// The clusters the base drew whole, `[from, to)` into `payload`.
///
/// What 0c0c30a8 prints: nine of the 24 recurring clusters' first pass,
/// 18 of the 900 churn clusters and nine of the second pass, among
/// 924-936; it refused 936 points. A sweep run whenever one cluster's eight
/// slots were full drew 924, 926, 927, 930, 933, 934 and 936 without all
/// their points.
const BASE_RIGHT: &[(usize, usize)] = &[
    (0, 4),
    (5, 7),
    (9, 11),
    (12, 13),
    (24, 27),
    (28, 29),
    (34, 35),
    (36, 37),
    (39, 40),
    (44, 45),
    (56, 57),
    (58, 59),
    (61, 62),
    (75, 76),
    (118, 119),
    (155, 156),
    (166, 167),
    (170, 171),
    (174, 175),
    (504, 505),
    (924, 928),
    (929, 931),
    (933, 935),
    (936, 937),
];

#[test]
fn every_cluster_the_base_kept_whole_is_kept_whole() {
    let cs = payload();
    let mut st = Owned::with_history(&WS, &CacheGeometry::UNKNOWN, Depth::DEFAULT);
    let mut t = Term::new(st.storage(), &WS);
    support::feed(&mut t, &bytes(&cs));
    let got = whole(&t, &cs);
    let found = ranges(&got);
    println!("whole: {found:?}");
    println!("dropped: {}", t.marks_dropped);
    let missing: Vec<usize> = BASE_RIGHT
        .iter()
        .flat_map(|&(a, b)| a..b)
        .filter(|&i| !got[i])
        .collect();
    assert!(!BASE_RIGHT.is_empty(), "the base's set, taken");
    assert_eq!(missing, Vec::<usize>::new(), "clusters the base kept whole");
    // The store did not fill, so nothing swept, and the second pass found
    // every prefix the first left.
    assert!(got[RECUR + CHURN..].iter().all(|w| *w), "the second pass");
}

/// **On the committed recordings with marks, this store refuses none**, so
/// every cell holds what was sent, which covers whatever the base kept:
/// it refused 72 points of `cat-cjk` and 2 of `less-cjk`.
#[test]
fn the_recordings_with_marks_lose_none() {
    for name in ["cat-cjk", "less-cjk"] {
        let path = format!(
            "{}/../tests/recordings/utf8/{name}.raw",
            env!("CARGO_MANIFEST_DIR")
        );
        let bytes = std::fs::read(&path).expect("a committed recording");
        let mut st = Owned::with_history(&WS, &CacheGeometry::UNKNOWN, Depth::DEFAULT);
        let mut t = Term::new(st.storage(), &WS);
        support::feed(&mut t, &bytes);
        assert_eq!(t.marks_dropped, 0, "{name}");
    }
}
