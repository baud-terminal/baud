//! **A region move on the alternate screen copies no row.**
//!
//! On the alternate screen a region's rows move by rotating its
//! window-slot index: the rows the move keeps are written nowhere, and only
//! the `k` rows it opens are blanked, each to its extent. A copy writes
//! every row of the region at full width. A region move rotates the index
//! when the region set turns it, or the index is already off the identity,
//! and copies otherwise, whichever read it falls in: a read through the
//! identity that sets such a region, or brings the screen up with one,
//! stops there, and the rest of it is fed through the index.
//!
//! And the rule follows the program, not its reads: a program run as one
//! read, a step a read and a byte a read shows the same rows and writes
//! the same cells.

mod support;

use baud_core::*;
use proptest::prelude::*;

const ROWS: usize = 50;
const COLS: usize = 200;
const WS: WinSize = WinSize {
    ws_row: 50,
    ws_col: 200,
};

/// Every row written at full width, so a blank it takes writes all of it.
fn fill(t: &mut Term<'_>) {
    let mut b = Vec::new();
    for y in 0..ROWS {
        let text: String = (0..COLS)
            .map(|x| char::from(b"abcdefghijklmnopqrstuvwxyz"[(x + y) % 26]))
            .collect();
        b.extend_from_slice(format!("\x1b[{};1H{text}", y + 1).as_bytes());
    }
    support::feed(t, &b);
}

/// The cells one read of `bytes` writes.
fn written(t: &mut Term<'_>, bytes: &[u8]) -> usize {
    let before = t.amplification().1;
    support::feed(t, bytes);
    t.amplification().1 - before
}

/// Each of the alternate screen's region movers, with the index off the
/// identity: the opened rows alone are written, `k` rows of `COLS`, but
/// for the ring's rotation and repair of a top region with fewer rows
/// under it than in it.
///
/// Named break: copy the region on the alternate screen, as the primary
/// does -- each mover writes the region's height in rows, not `k`.
#[test]
fn a_region_move_on_the_alternate_screen_writes_only_the_rows_it_opens() {
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    support::feed(&mut t, b"\x1b[?1049h");
    fill(&mut t);
    // A region move before the cases, through the index.
    support::feed(&mut t, b"\x1b[1;24r\x1b[S");
    let cases: [(&str, &[u8], &[u8], usize); 7] = [
        // A top region with more rows under it than in it: SU.
        ("SU, top region 1;24", b"\x1b[1;24r", b"\x1b[8S", 8),
        // A top region with fewer rows under it than in it: SU rotates the
        // ring and repairs the 5 rows under it, 6 rows a line, through the
        // index as through the identity.
        ("SU, top region 1;45", b"\x1b[1;45r", b"\x1b[3S", 3 * 6),
        // A region not at the top: SU and SD.
        ("SU, region 3;20", b"\x1b[3;20r", b"\x1b[5S", 5),
        ("SD, region 3;20", b"\x1b[3;20r", b"\x1b[5T", 5),
        // IL and DL from row 5 in the region.
        ("IL, region 3;20", b"\x1b[3;20r\x1b[5;1H", b"\x1b[4L", 4),
        ("DL, region 3;20", b"\x1b[3;20r\x1b[5;1H", b"\x1b[4M", 4),
        // A line feed at the region's bottom.
        ("IND, region 3;20", b"\x1b[3;20r\x1b[20;1H", b"\x1bD", 1),
    ];
    for (what, set, mv, k) in cases {
        support::feed(&mut t, b"\x1b[r");
        fill(&mut t);
        support::feed(&mut t, set);
        assert_eq!(written(&mut t, mv), k * COLS, "{what}");
    }
}

/// **A visit's region moves run through the index from its first**, in
/// the read that brings the alternate screen up as in every read after,
/// and leaving puts the index back for the next visit: each move writes
/// the `k` rows it opens.
///
/// Named break: feed the rest of the read that brings the screen up
/// through the identity (`Paint::up_through_index` holding nothing) --
/// the first move copies and writes the region, 24 rows, not 8.
#[test]
fn a_visit_moves_through_the_index_from_its_first_read() {
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    support::feed(&mut t, b"\x1b[?47h");
    fill(&mut t);
    support::feed(&mut t, b"\x1b[?47l");
    for _ in 0..2 {
        let up = b"\x1b[?47h\x1b[1;24r\x1b[8S";
        assert_eq!(written(&mut t, up), 8 * COLS, "the first move");
        fill(&mut t);
        assert_eq!(written(&mut t, b"\x1b[8S"), 8 * COLS, "the second move");
        fill(&mut t);
        support::feed(&mut t, b"\x1b[r\x1b[?47l");
    }
}

/// **Toggling the alternate screen with no region move swaps no row**: a
/// thousand visits, under `?47` and `?1047` in turn, each writing a line and
/// feeding it, and no region moved, so the index is never off the identity
/// and leaving costs no swap. A visit whose second region move runs through
/// the index swaps rows home when it leaves.
///
/// Named break: let a visit with no region move put the index off the
/// identity, or swap rows on leaving an index at the identity -- the
/// toggles' swaps are not 0.
#[test]
fn toggling_the_alternate_screen_with_no_region_move_swaps_no_row() {
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    for i in 0..1000 {
        let mode = if i % 2 == 0 { "47" } else { "1047" };
        support::feed(
            &mut t,
            format!("\x1b[?{mode}hline {i}\r\n\x1b[?{mode}l").as_bytes(),
        );
    }
    assert_eq!(t.slot_swaps(), baud_domain::Rows::ZERO, "1,000 toggles");
    support::feed(&mut t, b"\x1b[?47h");
    fill(&mut t);
    support::feed(&mut t, b"\x1b[1;24r\x1b[S");
    support::feed(&mut t, b"\x1b[3S");
    support::feed(&mut t, b"\x1b[r\x1b[?47l");
    assert!(
        t.slot_swaps() > baud_domain::Rows::ZERO,
        "a visit with a region move through the index"
    );
}
/// A small screen for the property, so a program fits one read.
const P_ROWS: usize = 12;
const P_WS: WinSize = WinSize {
    ws_row: 12,
    ws_col: 20,
};

/// One step of a program on the alternate screen.
#[derive(Clone, Debug)]
enum Op {
    /// DECSTBM, rows `top..=bot`, from 1; `0;0` is the whole screen.
    Region(usize, usize),
    Su(usize),
    Sd(usize),
    Il(usize, usize),
    Dl(usize, usize),
    Ind(usize),
    Ri(usize),
    Nel(usize),
    /// A line feed on the last row of the whole screen: the ring turns.
    Turn,
    /// A row of text, with a double-width line or a wrap.
    Text(usize, u8),
}

impl Op {
    fn bytes(&self) -> String {
        match *self {
            Op::Region(0, 0) => "\x1b[r".into(),
            Op::Region(a, b) => format!("\x1b[{a};{b}r"),
            Op::Su(k) => format!("\x1b[{k}S"),
            Op::Sd(k) => format!("\x1b[{k}T"),
            Op::Il(y, k) => format!("\x1b[{y};1H\x1b[{k}L"),
            Op::Dl(y, k) => format!("\x1b[{y};1H\x1b[{k}M"),
            Op::Ind(y) => format!("\x1b[{y};1H\x1bD"),
            Op::Ri(y) => format!("\x1b[{y};1H\x1bM"),
            Op::Nel(y) => format!("\x1b[{y};1H\x1bE"),
            Op::Turn => format!("\x1b[r\x1b[{P_ROWS};1H\n"),
            Op::Text(y, s) => match s % 3 {
                0 => format!("\x1b[{y};1H\x1b#6T{s}"),
                1 => format!("\x1b[{y};1H{}", "w".repeat(22)),
                _ => format!("\x1b[{y};1H\x1b#5t{s}"),
            },
        }
    }
}

fn op() -> impl Strategy<Value = Op> {
    let row = 1..=P_ROWS;
    let k = prop_oneof![Just(0usize), 1..4usize, P_ROWS..P_ROWS + 3];
    prop_oneof![
        3 => (1..=P_ROWS, 1..=P_ROWS).prop_map(|(a, b)| match a < b {
            true => Op::Region(a, b),
            false => Op::Region(0, 0),
        }),
        2 => k.clone().prop_map(Op::Su),
        2 => k.clone().prop_map(Op::Sd),
        2 => (row.clone(), k.clone()).prop_map(|(y, k)| Op::Il(y, k)),
        2 => (row.clone(), k).prop_map(|(y, k)| Op::Dl(y, k)),
        1 => row.clone().prop_map(Op::Ind),
        1 => row.clone().prop_map(Op::Ri),
        1 => row.clone().prop_map(Op::Nel),
        1 => Just(Op::Turn),
        2 => (row, any::<u8>()).prop_map(|(y, s)| Op::Text(y, s)),
    ]
}

/// The first steps of every program: the screen drawn, the ring turned,
/// and a region moved, which has the reads after it take the index.
fn prelude() -> String {
    let mut s = String::from("\x1b[?1049h");
    for y in 1..=P_ROWS {
        s.push_str(&format!("\x1b[{y};1HL{y:02}"));
    }
    s
}

/// Every row's text, line size and wrap.
type Screen = Vec<(String, LineRendition, Wrap)>;

/// Every row's text, line size and wrap, read.
fn screen(t: &Term<'_>) -> Screen {
    (0..P_ROWS)
        .map(|y| {
            let r = Row::from_index(y);
            let text = t
                .row(r)
                .expect("a row")
                .iter()
                .filter_map(|c| {
                    c.content
                        .as_codepoint()
                        .and_then(|x| char::from_u32(x.value()))
                })
                .collect::<String>();
            (
                text,
                t.line_rendition(r).expect("a row"),
                t.viewport().wrapped(r),
            )
        })
        .collect()
}

/// The program's screen and the cells it wrote, run as one read, a step a
/// read, and a byte a read. One read runs through the identity until a
/// region that turns the index is set and through the index from there;
/// a byte a read starts a read after every byte, each choosing its own.
fn arms(ops: &[Op]) -> [(Screen, usize); 3] {
    let mut one = prelude();
    for o in ops {
        one.push_str(&o.bytes());
    }
    assert!(one.len() <= baud_core::READ_CHUNK, "one read");
    let run = |reads: &mut dyn Iterator<Item = Vec<u8>>| {
        let mut owned = Owned::new(&P_WS);
        let mut t = Term::new(owned.storage(), &P_WS);
        for r in reads {
            support::feed(&mut t, &r);
        }
        (screen(&t), t.amplification().1)
    };
    let steps = core::iter::once(prelude()).chain(ops.iter().map(Op::bytes));
    [
        run(&mut core::iter::once(one.clone().into_bytes())),
        run(&mut steps.map(String::into_bytes)),
        run(&mut one.bytes().map(|b| vec![b])),
    ]
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(
        std::env::var("PROPTEST_CASES").ok().and_then(|s| s.parse().ok()).unwrap_or(2000)
    ))]

    /// **A region move rotates or copies by the same rule wherever the
    /// reads end**: the rows, and the cells written, are the same run as
    /// one read, a step a read and a byte a read.
    ///
    /// Named break: hold nothing at a region set in a read through the
    /// identity (`Paint::up_through_index`) -- the one read copies the
    /// moves the byte-a-read run rotates, and writes more cells.
    #[test]
    fn a_program_moves_its_regions_alike_however_its_reads_split(
        ops in prop::collection::vec(op(), 1..40)
    ) {
        let [one, stepped, bytes] = arms(&ops);
        prop_assert_eq!(&stepped, &one);
        prop_assert_eq!(&bytes, &one);
    }
}

/// **The property's generator reaches every shape it is there for**: a
/// top region with more rows under it than in it and one with fewer, a
/// region not at the top, a move of 0 rows and of the region's height or
/// more, a one-row move (IL or DL on the region's last row), and a region
/// whose places straddle the ring's end after the ring turned.
///
/// Named break: drop `Turn` from the generator -- no region straddles.
#[test]
fn the_property_reaches_every_shape() {
    use proptest::strategy::ValueTree;
    use proptest::test_runner::TestRunner;
    let mut runner = TestRunner::deterministic();
    let programs = prop::collection::vec(op(), 1..40);
    let mut seen = [0usize; 7];
    for _ in 0..2000 {
        let ops = programs.new_tree(&mut runner).expect("a program").current();
        let (mut top, mut bot, mut turned) = (1usize, P_ROWS, 0usize);
        for o in &ops {
            match *o {
                Op::Region(0, 0) | Op::Turn => (top, bot) = (1, P_ROWS),
                Op::Region(a, b) => (top, bot) = (a, b),
                _ => {}
            }
            if matches!(o, Op::Turn) {
                turned += 1;
            }
            let h = bot + 1 - top;
            let whole = top == 1 && bot == P_ROWS;
            let moves = match *o {
                Op::Su(k) | Op::Sd(k) => Some(k),
                Op::Il(y, k) | Op::Dl(y, k) if (top..=bot).contains(&y) => {
                    if y == bot {
                        seen[4] += 1;
                    }
                    Some(k)
                }
                _ => None,
            };
            if let (Some(k), false) = (moves, whole) {
                let below = P_ROWS - bot;
                match (top == 1, below >= h) {
                    (true, true) => seen[0] += 1,
                    (true, false) => seen[1] += 1,
                    (false, _) => seen[2] += 1,
                }
                match k {
                    0 => seen[3] += 1,
                    k if k >= h => seen[5] += 1,
                    _ => {}
                }
                if turned % P_ROWS != 0 && (top - 1 + turned) % P_ROWS + h > P_ROWS {
                    seen[6] += 1;
                }
            }
        }
    }
    let names = [
        "top region, below >= height",
        "top region, below < height",
        "region not at the top",
        "k = 0",
        "one-row move",
        "k >= height",
        "a region straddling the ring's end",
    ];
    for (n, s) in names.iter().zip(seen) {
        assert!(s > 0, "the generator never reached: {n}");
    }
}

/// Every row of the alternate screen, cell for cell, with its line size
/// and wrap.
fn cells(t: &Term<'_>) -> Vec<(Vec<PackedCell>, LineRendition, Wrap)> {
    (0..ROWS)
        .map(|y| {
            let r = Row::from_index(y);
            (
                t.row(r).expect("a row").to_vec(),
                t.line_rendition(r).expect("a row"),
                t.viewport().wrapped(r),
            )
        })
        .collect()
}

/// **An IL or DL under SEE 3 moves the same rows whether it rotates or
/// copies**: SEE 3 bounds the move at the qualified area's last line, so
/// the region the move reads is narrower than the region set. The same
/// program runs on a storage lent the index, where each move rotates it,
/// and on one lent none, where each copies; as one read a step and as a
/// byte a read. The screen is the same cell for cell in all four after
/// each move, after leaving the screen under `?47`, and after coming back
/// to it; the primary's history, which the program never reaches, keeps
/// its count.
///
/// Named break: in a rotation, leave the opened rows unblanked
/// (`Paint::move_rows`) -- the IL under SEE 3 writes no cell on the
/// rotating runs, and they show the rows that left the area where the
/// copying runs show blanks.
#[test]
fn an_il_or_dl_under_see_3_rotates_and_copies_alike() {
    // A region that turns the index, moved once so the index is off the
    // identity; a qualified area on rows 6..12; SEE 3.
    let setup = b"\x1b[?47h\x1b[3;20r\x1b[S\x1b[6;1H\x1bV\x1b[12;5H\x1bW\x1b[3Q";
    let moves: [&[u8]; 4] = [
        b"\x1b[8;1H\x1b[2L",
        b"\x1b[7;1H\x1b[3M",
        b"\x1b[11;1H\x1b[9L",
        b"\x1b[6;1H\x1bD\x1b[M",
    ];
    let run = |lent: bool, bytewise: bool| {
        let mut owned = Owned::new(&WS);
        let storage = match lent {
            true => owned.storage(),
            false => owned.storage().with_window_slots(&mut []),
        };
        let mut t = Term::new(storage, &WS);
        let feed = |t: &mut Term<'_>, b: &[u8]| match bytewise {
            false => support::feed(t, b),
            true => b.iter().for_each(|x| support::feed(t, &[*x])),
        };
        let mut seen = Vec::new();
        let mut moved = Vec::new();
        feed(&mut t, b"\x1b[?47h");
        fill(&mut t);
        feed(&mut t, setup);
        for m in moves {
            let before = t.amplification().1;
            feed(&mut t, m);
            moved.push(t.amplification().1 - before);
            seen.push(cells(&t));
        }
        feed(&mut t, b"\x1b[?47l");
        let history = t.scrollback().len();
        feed(&mut t, b"\x1b[?47h");
        seen.push(cells(&t));
        (seen, history, moved)
    };
    let (rotated, history, rotated_cells) = run(true, false);
    let (copied, copied_history, copied_cells) = run(false, false);
    // The first move: rows 8..12 move down 2, the area's last line its
    // bottom; rotating writes the 2 opened rows, copying all 5.
    assert_eq!(rotated_cells[0], 2 * COLS, "the IL rotates");
    assert_eq!(copied_cells[0], 5 * COLS, "the IL copies");
    assert_eq!(copied, rotated, "rotating and copying show the same cells");
    assert_eq!(copied_history, history, "the primary's history");
    for lent in [true, false] {
        let (bytes, h, c) = run(lent, true);
        assert_eq!(bytes, rotated, "a byte a read, lent an index: {lent}");
        assert_eq!(h, history);
        let one = match lent {
            true => &rotated_cells,
            false => &copied_cells,
        };
        assert_eq!(&c, one, "the cells each move writes, a byte a read");
    }
}
