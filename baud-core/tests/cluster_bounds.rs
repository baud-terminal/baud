//! What the cluster store keeps: every point of a page of pointed Hebrew,
//! more clusters over a long session than it has slots, and, across the
//! sweeps, every cluster a cell held when the store filled, whichever
//! screen and however far back in the history the cell is.
//!
//! **The bounds, as decided on 2026-10-13**: 1024 slots, each cluster's
//! hash probing eight of them, the slot named from the cell's `Content`
//! above U+10FFFF, and, once every slot holds a cluster, a sweep of the
//! live cells reclaiming the slots no cell names. The store this replaced
//! held 63 in six bits of `CellFlags` and freed them only on RIS, so
//! `marks_dropped` read 0, 10 and 45 for Genesis 1:1, 1:1-2 and 1:1-5, and
//! a session refused every cluster past its 63rd distinct one.
//!
//! Every test here reads the terminal through what it shows a reader --
//! `Term::marks` on the screen, `Term::cluster_marks` on a history line,
//! `marks_dropped` -- so each one fails on the store it replaced.

#[path = "support/mod.rs"]
mod support;

use baud_core::{
    Back, BytePos, CacheGeometry, Col, Depth, NCLUSTER, Owned, PackedCell, ReadLen, Row, Term,
    WinSize,
};

/// Genesis 1:1-5 with vowel points and no cantillation, the public-domain
/// Masoretic text, one verse a line.
const GENESIS_1_1_TO_5: [&str; 5] = [
    "בְּרֵאשִׁית בָּרָא אֱלֹהִים אֵת הַשָּׁמַיִם וְאֵת הָאָרֶץ׃",
    "וְהָאָרֶץ הָיְתָה תֹהוּ וָבֹהוּ וְחֹשֶׁךְ עַל־פְּנֵי תְהוֹם וְרוּחַ אֱלֹהִים מְרַחֶפֶת עַל־פְּנֵי הַמָּיִם׃",
    "וַיֹּאמֶר אֱלֹהִים יְהִי אוֹר וַיְהִי־אוֹר׃",
    "וַיַּרְא אֱלֹהִים אֶת־הָאוֹר כִּי־טוֹב וַיַּבְדֵּל אֱלֹהִים בֵּין הָאוֹר וּבֵין הַחֹשֶׁךְ׃",
    "וַיִּקְרָא אֱלֹהִים לָאוֹר יוֹם וְלַחֹשֶׁךְ קָרָא לָיְלָה וַיְהִי־עֶרֶב וַיְהִי־בֹקֶר יוֹם אֶחָד׃",
];

/// A Hebrew point: U+0591-05C7 but the four that stand alone (maqaf
/// U+05BE, paseq U+05C0, sof pasuq U+05C3, nun hafukha U+05C6), which take
/// a cell of their own.
fn is_point(c: char) -> bool {
    matches!(u32::from(c), 0x0591..=0x05C7)
        && !matches!(u32::from(c), 0x05BE | 0x05C0 | 0x05C3 | 0x05C6)
}

fn verses(n: usize) -> Vec<u8> {
    GENESIS_1_1_TO_5[..n].join("\r\n").into_bytes()
}

/// Each cell's points as the first `n` verses spell them, row by row.
fn wanted(n: usize) -> Vec<Vec<u32>> {
    let mut out: Vec<Vec<u32>> = Vec::new();
    for line in &GENESIS_1_1_TO_5[..n] {
        for c in line.chars() {
            match is_point(c) {
                true => out
                    .last_mut()
                    .expect("a point after a letter")
                    .push(u32::from(c)),
                false => out.push(Vec::new()),
            }
        }
    }
    out
}

/// Each cell's points as the terminal holds them, in the cells `wanted`
/// walks: one a character that is not a point, from column 0 of each row.
fn held(t: &Term<'_>, n: usize) -> Vec<Vec<u32>> {
    let mut out = Vec::new();
    for (r, line) in GENESIS_1_1_TO_5[..n].iter().enumerate() {
        let cells = line.chars().filter(|c| !is_point(*c)).count();
        for c in 0..cells {
            out.push(t.marks(Row::from_index(r), Col::at_index(c)).collect());
        }
    }
    out
}

/// Feed `bytes` as the reads `cuts` ends.
fn feed_cut(t: &mut Term<'_>, bytes: &[u8], cuts: &[usize]) {
    let mut at = 0;
    for &end in cuts.iter().chain([&bytes.len()]) {
        if end > at {
            t.write_at(BytePos::ZERO, &bytes[at..end]);
            t.feed(BytePos::ZERO, ReadLen::from_index(end - at));
            at = end;
        }
    }
}

/// **Every point of Genesis 1:1, 1:1-2 and 1:1-5 is kept**, in its cell and
/// in the order it was sent. The 63-slot store refused 0, 10 and 45 of them:
/// it interns every growing prefix of a cluster, the five verses need 67
/// distinct prefixes, and its hash took the slot from bits a change in the
/// last point never reached.
#[test]
fn every_point_of_genesis_1_1_to_5_is_kept() {
    let ws = support::CORPUS;
    let (mut dropped, mut cells) = (Vec::new(), Vec::new());
    for n in [1, 2, 5] {
        let mut st = Owned::new(&ws);
        let mut t = Term::new(st.storage(), &ws);
        support::feed(&mut t, &verses(n));
        dropped.push(t.marks_dropped);
        cells.push((n, held(&t, n)));
    }
    assert_eq!(dropped, [0, 0, 0], "points refused over 1, 2 and 5 verses");
    for (n, got) in cells {
        assert_eq!(got, wanted(n), "the points of {n} verses, cell by cell");
    }
    assert_eq!(
        wanted(5).iter().map(Vec::len).sum::<usize>(),
        146,
        "146 points over five verses"
    );
}

/// **Where the reads fall changes nothing**: every read size from one byte
/// to sixteen, and every split of the five verses into two reads, leaves
/// every point in its cell and none refused. A point split across two reads
/// mid-UTF-8 still arrives after its letter, and the store is a function of
/// the clusters in the order they complete.
#[test]
fn every_read_size_and_every_split_keeps_every_point() {
    let ws = support::CORPUS;
    let bytes = verses(5);
    let want = wanted(5);
    let mut st = Owned::new(&ws);
    for size in 1..=16 {
        let cuts: Vec<usize> = (size..bytes.len()).step_by(size).collect();
        let mut t = Term::new(st.storage(), &ws);
        feed_cut(&mut t, &bytes, &cuts);
        assert_eq!(t.marks_dropped, 0, "reads of {size} bytes");
        assert_eq!(held(&t, 5), want, "reads of {size} bytes");
    }
    for cut in 1..bytes.len() {
        let mut t = Term::new(st.storage(), &ws);
        feed_cut(&mut t, &bytes, &[cut]);
        assert_eq!(t.marks_dropped, 0, "split at {cut}");
        assert_eq!(held(&t, 5), want, "split at {cut}");
    }
}

/// The 52 ASCII letters, a cluster's base.
const LETTERS: &[u8; 52] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ";

/// The `i`th of 5,824 distinct one-mark clusters: a letter and one of the
/// 112 combining diacritical marks, U+0300-036F, each zero-width and
/// `Extend`, so each attaches to its letter.
fn cluster(i: usize) -> (u8, u32) {
    assert!(i < LETTERS.len() * 112, "5,824 distinct clusters");
    (
        LETTERS[i % LETTERS.len()],
        0x0300 + u32::try_from(i / LETTERS.len()).expect("one of 112 marks"),
    )
}

/// Clusters `from..to`, `per` to a line, a space between each, each line
/// ended `CR LF`.
fn lines(from: usize, to: usize, per: usize) -> Vec<u8> {
    let mut out = Vec::new();
    for (k, i) in (from..to).enumerate() {
        let (b, m) = cluster(i);
        out.push(b);
        out.extend(char::from_u32(m).expect("a mark").to_string().bytes());
        out.push(match (k + 1) % per == 0 {
            true => b'\r',
            false => b' ',
        });
        if (k + 1) % per == 0 {
            out.push(b'\n');
        }
    }
    out
}

/// The marks the screen row `r` holds at each of `per` clusters.
fn screen_marks(t: &Term<'_>, r: usize, per: usize) -> Vec<Vec<u32>> {
    (0..per)
        .map(|k| t.marks(Row::from_index(r), Col::at_index(2 * k)).collect())
        .collect()
}

/// The marks a history line holds at each of `per` clusters, read from the
/// line's own cells.
fn line_marks(t: &Term<'_>, cells: &[PackedCell], per: usize) -> Vec<Vec<u32>> {
    (0..per)
        .map(|k| t.cluster_marks(cells[2 * k]).collect())
        .collect()
}

/// What clusters `from..from + per` are, as marks.
fn want_marks(from: usize, per: usize) -> Vec<Vec<u32>> {
    (from..from + per).map(|i| vec![cluster(i).1]).collect()
}

/// **A session that passes more distinct clusters than the store has slots
/// keeps more than the store holds, because the full store sweeps.** Five
/// rows and no history, 5,824 distinct clusters four to a line: while the
/// store fills, a cluster whose eight slots are taken is refused, and once
/// every slot is used the next refusal sweeps and frees every slot but the
/// one a cell on the screen names (`cluster_sweep.rs` counts the sweep,
/// which reads API the store this replaced did not have).
/// The store kept 2,008 of the 5,824, which
/// no store of 1024 slots that never frees can; it refused 3,816, the last
/// clusters among them, as the store filled again. Every cell on the screen
/// holds its own mark or none, never another's. The 63-slot store refused
/// 5,761.
#[test]
fn a_session_past_the_store_sweeps_once_the_store_is_full() {
    let ws = WinSize {
        ws_row: 5,
        ws_col: 40,
    };
    let mut st = Owned::with_history(&ws, &CacheGeometry::UNKNOWN, Depth::NONE);
    let mut t = Term::new(st.storage(), &ws);
    let total = LETTERS.len() * 112;
    support::feed(&mut t, &lines(0, total, 4));
    assert_eq!(t.marks_dropped, 3816, "clusters refused over {total}");
    assert!(total - t.marks_dropped > NCLUSTER, "more kept than slots");
    // The last four lines, above the empty row the last CR LF left.
    for r in 0..4 {
        let from = total - 16 + 4 * r;
        for (k, (got, want)) in screen_marks(&t, r, 4)
            .into_iter()
            .zip(want_marks(from, 4))
            .enumerate()
        {
            assert!(
                got.is_empty() || got == want,
                "row {r}, cluster {k}: {got:?}"
            );
        }
    }
}

/// **A sweep frees only what no cell names**: the history, the screen that
/// is not up and the rows outside a scroll region all keep their clusters
/// while a region on the live screen churns through thousands, past the
/// point the store fills and sweeps. The churn's own clusters are refused
/// while their slots are taken, 2,895 of its 4,000; the one sweep, once all
/// 1024 slots were used, freed 847 and left the 177 cells name.
///
/// Three holders, each live across the sweep run while it is not the screen
/// being written:
/// - 160 clusters on the primary, most of them scrolled into the history;
/// - 32 on the alternate screen's top eight rows, written under `?47` and
///   left;
/// - the alternate screen's top rows again, kept while a region below them
///   churns.
///
/// The churn runs inside `DECSTBM` regions whose top is not the screen's,
/// so the lines it scrolls out are gone rather than retained, and every
/// cluster on them is dead by the time the store fills.
#[test]
fn the_history_the_other_screen_and_the_rows_outside_a_region_keep_theirs() {
    let ws = WinSize {
        ws_row: 10,
        ws_col: 40,
    };
    let per = 4;
    let mut st = Owned::with_history(&ws, &CacheGeometry::UNKNOWN, Depth::DEFAULT);
    let mut t = Term::new(st.storage(), &ws);
    // 40 lines on the primary, and the empty line the last `CR LF` opens:
    // the oldest 31 in the history, the newest nine and the empty one on the
    // screen.
    let primary = 160;
    support::feed(&mut t, &lines(0, primary, per));
    // The alternate screen under `?47`, which keeps it as it was left: 32
    // clusters on its top eight rows, written from the home position.
    let alt_from = primary;
    let mut b = b"\x1b[?47h\x1b[H".to_vec();
    b.extend(lines(alt_from, alt_from + 32, per));
    b.extend(b"\x1b[?47l");
    support::feed(&mut t, &b);
    // Churn on the primary, in a region of rows 6-10: its lines scroll out
    // of the region and are gone. 2,000 distinct clusters, twice the store.
    let churn = alt_from + 32;
    let mut b = b"\x1b[6;10r\x1b[10;1H".to_vec();
    b.extend(lines(churn, churn + 2000, per));
    b.extend(b"\x1b[r");
    support::feed(&mut t, &b);
    // Then on the alternate screen, in its rows 9-10, below the 32.
    let churn2 = churn + 2000;
    let mut b = b"\x1b[?47h\x1b[9;10r\x1b[10;1H".to_vec();
    b.extend(lines(churn2, churn2 + 2000, per));
    b.extend(b"\x1b[r");
    support::feed(&mut t, &b);
    assert_eq!(t.marks_dropped, 2895, "the churn's clusters refused");
    // The alternate screen's first eight rows hold what `?47` left there.
    for r in 0..8 {
        assert_eq!(
            screen_marks(&t, r, per),
            want_marks(alt_from + per * r, per),
            "alternate row {r}"
        );
    }
    support::feed(&mut t, b"\x1b[?47l");
    // The primary's history: its 31 oldest lines, newest first. The region
    // churned below row 5 and never scrolled the screen, so no churn line
    // reached the history.
    let hist = t.scrollback();
    assert_eq!(hist.len(), 31, "lines in the history");
    for k in 0..hist.len() {
        let (cells, _) = hist.line(Back::of(k)).expect("a retained line");
        let line = 30 - k;
        assert_eq!(
            line_marks(&t, cells, per),
            want_marks(per * line, per),
            "history line {line}"
        );
    }
    // The primary screen's top five rows, above the region: lines 31-35.
    for r in 0..5 {
        assert_eq!(
            screen_marks(&t, r, per),
            want_marks(per * (31 + r), per),
            "primary row {r}"
        );
    }
}
