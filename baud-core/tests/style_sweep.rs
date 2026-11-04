//! The rendition table's sweep: a slot no cell names is freed, and only such
//! a slot.
//!
//! `[[source: dec-std-070]]`'s SGR, page 5-104: "In no way does the selection
//! of current rendition affect the rendition of characters already entered
//! into the display." A table that evicts once every slot is filled breaks
//! that the first time a session has minted more renditions than it has
//! slots: the evicted slot takes the new rendition and every cell naming it
//! is recoloured. The sweep frees the slots no cell names, at 7/8 load and
//! before any eviction, so a cell keeps its rendition while the cells name
//! fewer renditions than the table has slots. Each test names the mechanism
//! it holds.

#[path = "support/mod.rs"]
mod support;

use baud_core::*;
use baud_domain::{Back, CellFlags, Chan, ColorWord, Content, StyleId, StyleWord, StyleWrites};

/// The 1,024-slot floor, so a program fills it in a few thousand renditions.
const SMALL: WinSize = WinSize {
    ws_row: 10,
    ws_col: 40,
};

fn feed(t: &mut Term<'_>, bytes: &[u8]) {
    support::feed(t, bytes);
    t.clear_reply();
}

/// `SGR 0`, then a truecolour foreground and background, both from `i`, so
/// every `(i, tag)` is a distinct rendition for `i` below 65,536.
fn rendition(i: usize, tag: u8) -> String {
    format!(
        "\x1b[0;38;2;{tag};{};{};48;2;{};{};{}m",
        i & 255,
        (i >> 8) & 255,
        (i * 7) & 255,
        tag,
        (i >> 3) & 255
    )
}

/// `n` distinct renditions, each written over the one cell at `row`, column 1,
/// so each dies when the next replaces it.
fn churn(row: usize, n: usize, tag: u8) -> Vec<u8> {
    let mut out = Vec::new();
    for i in 0..n {
        out.extend(format!("\x1b[{};1H{}C", row + 1, rendition(i, tag)).as_bytes());
    }
    out
}

/// `n` cells, each in a rendition of its own, from the screen's origin.
fn owned_cells(n: usize, tag: u8, ch: char) -> Vec<u8> {
    let mut out = Vec::new();
    for i in 0..n {
        out.extend(
            format!(
                "\x1b[{};{}H{}{ch}",
                i / 40 + 1,
                i % 40 + 1,
                rendition(i, tag)
            )
            .as_bytes(),
        );
    }
    out
}

/// What every cell of the screen resolves to.
fn screen(t: &Term<'_>) -> Vec<StyleWord> {
    let (_, rows) = t.geometry();
    let mut v = Vec::new();
    for r in 0..rows.index() {
        for c in t.row(Row::from_index(r)).expect("a row on the grid") {
            v.push(t.style_of(*c));
        }
    }
    v
}

/// The last 8 history lines' first 300 cells' renditions, oldest first, when
/// the screen was pushed up by `pushed` lines after 300 owned cells.
fn history_cells(t: &Term<'_>, pushed: usize) -> Vec<StyleWord> {
    let sb = t.scrollback();
    let mut v = Vec::new();
    for line in 0..8 {
        let back = Back::of(pushed - 1 - line);
        let (cells, _) = sb
            .line(back)
            .unwrap_or_else(|| panic!("history line {line} missing"));
        // Through the line's own table: a line frozen off the screen names
        // ids in its generation's words.
        let table = sb.styles(back);
        for cell in cells.iter().take(40) {
            if v.len() < 300 {
                v.push(table.word(t.styles(), cell.style));
            }
        }
    }
    v
}

/// **Live cells keep their renditions however many die around them.** 300
/// cells each take a rendition of their own and are never written again;
/// then 5,120 renditions pass through one other cell, five times the table.
/// Evicting, that recolours live cells once the table has filled; swept,
/// nothing is evicted and no live cell changes.
#[test]
fn live_cells_keep_their_renditions_through_five_tables_of_churn() {
    support::with_term(&SMALL, |t| {
        let len = t.styles().len();
        assert_eq!(len, 1024, "the test is sized to the floor");
        feed(t, &owned_cells(300, 1, 'L'));
        let before = screen(t);
        feed(t, &churn(9, 5 * len, 2));
        let after = screen(t);
        let wrong = (0..300).filter(|&i| before[i] != after[i]).count();
        let r = t.style_reclaims();
        assert_eq!(wrong, 0, "live cells recoloured: {r:?}");
        assert_eq!(r.refusals, 0, "{r:?}");
        assert!(r.sweeps >= 4, "positive control: the churn swept: {r:?}");
    });
}

/// **A cell on the other screen is live.** Cells written on the primary
/// screen while the alternate is shown are named by the surface the view is
/// not addressing; a sweep that marked only the live surface would free
/// their slots, and the churn on the alternate screen would give those slots
/// other renditions. `?1049l` brings the primary back without clearing it.
#[test]
fn the_other_screens_cells_are_marked() {
    support::with_term(&SMALL, |t| {
        let len = t.styles().len();
        feed(t, &owned_cells(300, 3, 'P'));
        let before = screen(t);
        feed(t, b"\x1b[?1049h");
        feed(t, &churn(9, 5 * len, 4));
        feed(t, b"\x1b[?1049l");
        let after = screen(t);
        let wrong = (0..300).filter(|&i| before[i] != after[i]).count();
        let r = t.style_reclaims();
        assert_eq!(wrong, 0, "{r:?}");
        assert!(r.sweeps >= 4, "positive control: the churn swept: {r:?}");
    });
}

/// **A line in the history is live.** 300 cells, each its own rendition,
/// scrolled off the screen into the history; then the churn. Read back
/// through `Term::scrollback`, every one keeps its rendition.
#[test]
fn the_historys_cells_are_marked() {
    support::with_term(&SMALL, |t| {
        let len = t.styles().len();
        feed(t, &owned_cells(300, 5, 'H'));
        let before = screen(t);
        // Twenty newlines from the bottom row push the screen into the
        // history: the ten screen rows went in oldest first and ten blank
        // rows after them.
        feed(t, b"\x1b[0m\x1b[10;1H");
        feed(t, &[b'\n'; 20]);
        feed(t, &churn(9, 5 * len, 6));
        let kept = history_cells(t, 20);
        let r = t.style_reclaims();
        assert_eq!(kept.len(), 300);
        let wrong = (0..300).filter(|&i| kept[i] != before[i]).count();
        assert_eq!(wrong, 0, "{r:?}");
        assert!(r.sweeps >= 4, "positive control: the churn swept: {r:?}");
    });
}

/// **The primary's history is live while the alternate screen is shown.**
/// The history is the primary's, and a sweep run from the alternate screen
/// reaches it through the surface the view is not addressing.
#[test]
fn the_primarys_history_is_marked_from_the_alternate_screen() {
    support::with_term(&SMALL, |t| {
        let len = t.styles().len();
        feed(t, &owned_cells(300, 7, 'A'));
        let before = screen(t);
        feed(t, b"\x1b[0m\x1b[10;1H");
        feed(t, &[b'\n'; 20]);
        feed(t, b"\x1b[?1049h");
        feed(t, &churn(9, 5 * len, 8));
        feed(t, b"\x1b[?1049l");
        let kept = history_cells(t, 20);
        let r = t.style_reclaims();
        let wrong = (0..300).filter(|&i| kept[i] != before[i]).count();
        assert_eq!(wrong, 0, "{r:?}");
        assert!(r.sweeps >= 4, "positive control: the churn swept: {r:?}");
    });
}

/// **An erased cell is live in the rendition the erase gave it.** `EL` fills
/// with the current rendition, here a background no character carries, and
/// a row whose last cell already equals the blank stops at its extent, so
/// the cells past it hold the rendition without the row's used extent
/// counting them. The mark reads every cell, not only to the extent.
#[test]
fn an_erased_rows_rendition_is_marked() {
    support::with_term(&SMALL, |t| {
        let len = t.styles().len();
        feed(t, b"\x1b[3;1H\x1b[0;48;2;1;2;3m\x1b[2K\x1b[0m");
        let row = |t: &Term<'_>| {
            t.row(Row::from_index(2)).expect("a row on the grid")[..40]
                .iter()
                .map(|c| t.style_of(*c))
                .collect::<Vec<_>>()
        };
        let before = row(t);
        let bg = StyleWord::DEFAULT.with_bg(ColorWord::rgb(
            Chan::from_u8(1),
            Chan::from_u8(2),
            Chan::from_u8(3),
        ));
        assert!(before.iter().all(|w| *w == bg), "the control: EL took it");
        feed(t, &churn(9, 5 * len, 9));
        let r = t.style_reclaims();
        assert_eq!(row(t), before, "{r:?}");
        assert!(r.sweeps >= 4, "positive control: the churn swept: {r:?}");
    });
}

/// Two renditions sharing a bucket at `len` slots, after `SGR 0`: the folded
/// word is the default with a truecolour foreground.
type Rgb = (u8, u8, u8);

fn fg(c: Rgb) -> StyleWord {
    StyleWord::DEFAULT.with_fg(ColorWord::rgb(
        Chan::from_u8(c.0),
        Chan::from_u8(c.1),
        Chan::from_u8(c.2),
    ))
}

fn colliding(len: usize) -> (Rgb, Rgb) {
    let mut first: Vec<Option<Rgb>> = vec![None; len];
    for g in 0..=255u8 {
        for b in 0..=255u8 {
            let c = (9, g, b);
            let k = StyleId::bucket(fg(c), len);
            match first[k] {
                Some(o) if k > 1 && k + 1 < len => return (o, c),
                Some(_) => {}
                None => first[k] = Some(c),
            }
        }
    }
    unreachable!("no two of 65,536 colours share one of 1,024 buckets")
}

fn sgr(c: Rgb) -> String {
    format!("\x1b[0;38;2;{};{};{}m", c.0, c.1, c.2)
}

/// **A dead slot on a live rendition's probe path is kept as a tombstone.**
/// `W1` and `W2` share a bucket, so `W2` sits one slot past `W1`. `W1` dies
/// and `W2` lives; a sweep that emptied `W1`'s slot would end `W2`'s probe
/// there, and naming `W2` again would intern it a second time. The tombstone
/// keeps the probe walking to the slot `W2` already has.
#[test]
fn a_tombstone_keeps_a_live_rendition_findable() {
    support::with_term(&SMALL, |t| {
        let len = t.styles().len();
        let (c1, c2) = colliding(len);
        feed(
            t,
            format!("\x1b[1;1H{}A\x1b[2;1H{}B", sgr(c1), sgr(c2)).as_bytes(),
        );
        let cell = |t: &Term<'_>, r: usize| t.row(Row::from_index(r)).expect("a row")[0];
        let (id1, id2) = (cell(t, 0).style, cell(t, 1).style);
        assert_eq!(t.styles()[id1.index()], Some(fg(c1)));
        assert_eq!(t.styles()[id2.index()], Some(fg(c2)));
        assert_eq!(id2.index(), (id1.index() + 1) & (len - 1), "W2 one past W1");
        feed(t, b"\x1b[1;1H\x1b[0mA");
        feed(t, &churn(9, len, 7));
        assert!(t.style_reclaims().sweeps >= 1, "positive control: it swept");
        // W2 named again, on row 3: the same slot.
        feed(t, format!("\x1b[3;1H{}C", sgr(c2)).as_bytes());
        assert_eq!(cell(t, 2).style, id2);
        let copies = t.styles().iter().filter(|s| **s == Some(fg(c2))).count();
        assert_eq!(copies, 1, "W2 interned twice");
        assert_eq!(
            t.styles()[id1.index()],
            Some(StyleWord::TOMBSTONE),
            "W1's slot"
        );
    });
}

/// **A new rendition takes the first tombstone on its path.** After the
/// sweep above leaves `W1`'s slot a tombstone, a rendition bucketing there
/// takes it rather than the first empty slot past `W2`: the slot is
/// reclaimed, and `W2` stays findable behind it.
#[test]
fn a_new_rendition_takes_a_tombstone_on_its_path() {
    support::with_term(&SMALL, |t| {
        let len = t.styles().len();
        let (c1, c2) = colliding(len);
        feed(
            t,
            format!("\x1b[1;1H{}A\x1b[2;1H{}B", sgr(c1), sgr(c2)).as_bytes(),
        );
        let cell = |t: &Term<'_>, r: usize| t.row(Row::from_index(r)).expect("a row")[0];
        let (id1, id2) = (cell(t, 0).style, cell(t, 1).style);
        feed(t, b"\x1b[1;1H\x1b[0mA");
        feed(t, &churn(9, len, 7));
        assert_eq!(t.styles()[id1.index()], Some(StyleWord::TOMBSTONE));
        // A third colour in the same bucket.
        let c3 = (0..=255u8)
            .flat_map(|g| (0..=255u8).map(move |b| (11u8, g, b)))
            .find(|&c| StyleId::bucket(fg(c), len) == StyleId::bucket(fg(c1), len))
            .expect("a third colour in the bucket");
        feed(t, format!("\x1b[4;1H{}D", sgr(c3)).as_bytes());
        assert_eq!(cell(t, 3).style, id1, "the tombstone was passed over");
        feed(t, format!("\x1b[5;1H{}E", sgr(c2)).as_bytes());
        assert_eq!(cell(t, 4).style, id2, "W2 lost behind the reused slot");
    });
}

/// **A churned table sweeps before it fills.** A miss into a table at load
/// `a` probes about `(1 + 1/(1-a)^2)/2` slots, the whole table at full load,
/// so a fill that would pass 7/8 sweeps first, at most once a sixteenth of
/// the table's writes: through five tables of churn the table never holds
/// more than 7/8 and a sixteenth of its slots.
#[test]
fn a_churned_table_sweeps_before_it_fills() {
    support::with_term(&SMALL, |t| {
        let len = t.styles().len();
        let mut most = 0;
        for i in 0..5 * len {
            feed(t, format!("\x1b[9;1H{}C", rendition(i, 14)).as_bytes());
            most = most.max(t.styles().iter().filter(|s| s.is_some()).count());
        }
        assert!(t.style_reclaims().sweeps >= 4, "the control: it swept");
        assert!(
            most <= len * 7 / 8 + len / 16 + 1,
            "{most} of {len} slots filled"
        );
    });
}

/// **A full table sweeps before it refuses.** 1,023 renditions on cells of
/// their own fill a table lent no generations with named slots; then one
/// screen row of them is overwritten in the default, so 40 slots are named
/// by nothing. Minting on fresh cells, the table is full, so only the full
/// table's own sweep can free them: one runs within a sixteenth of the
/// table's writes, frees the 40, and the renditions after it take freed
/// slots and refuse nothing.
#[test]
fn a_full_table_sweeps_before_it_refuses() {
    support::with_term_unfrozen(&SMALL, |t| {
        let len = t.styles().len();
        let mut out = Vec::new();
        for i in 0..len - 1 {
            out.extend(format!("{}X", rendition(i, 13)).as_bytes());
            // The line feed in the default rendition: a scroll under a
            // coloured one interns that background alone, one rendition
            // more than the cells were written in.
            if (i + 1) % 40 == 0 {
                out.extend(b"\x1b[0m\r\n");
            }
        }
        feed(t, &out);
        assert!(t.styles().iter().all(Option::is_some), "the control: full");
        let at_full = t.style_reclaims();
        assert_eq!(at_full.refusals, 0);
        // Row 5 is 40 named cells, each its own rendition: overwritten.
        feed(t, b"\x1b[0m\x1b[5;1H");
        feed(t, &[b'k'; 40]);
        feed(t, b"\x1b[10;1H\r\n");
        let mut i = len;
        while t.style_reclaims().sweeps == at_full.sweeps {
            assert!(i < 2 * len, "no sweep ran on a full table");
            feed(t, format!("{}N", rendition(i, 13)).as_bytes());
            i += 1;
        }
        let swept = t.style_reclaims();
        assert!(swept.freed - at_full.freed >= 40, "{swept:?}");
        for j in 0..30 {
            feed(t, format!("{}M", rendition(i + j, 13)).as_bytes());
        }
        assert_eq!(t.style_reclaims().refusals, swept.refusals, "{swept:?}");
    });
}

/// **A full table whose sweep freed nothing refuses, and writes no named
/// slot, until a sixteenth of its slots' worth of renditions have come.**
/// 1,023 named renditions fill a table lent no generations, and minting goes
/// on until a full table's sweep has run, freeing nothing; then 40 named
/// cells are overwritten, so 40 slots are named by nothing. The eviction
/// this replaced wrote the next rendition over a named slot; refused, the
/// cell takes the default and no slot changes. The refusals count toward the
/// next sweep as writes would, so within a sixteenth of the table one runs,
/// frees the 40, and the next rendition is placed.
#[test]
fn a_full_table_refuses_until_its_next_sweep_is_due() {
    support::with_term_unfrozen(&SMALL, |t| {
        let len = t.styles().len();
        let mut out = Vec::new();
        for i in 0..len - 1 {
            out.extend(format!("{}X", rendition(i, 15)).as_bytes());
            // The line feed in the default rendition: a scroll under a
            // coloured one interns that background alone, one rendition
            // more than the cells were written in.
            if (i + 1) % 40 == 0 {
                out.extend(b"\x1b[0m\r\n");
            }
        }
        feed(t, &out);
        assert!(t.styles().iter().all(Option::is_some), "the control: full");
        let before = t.style_reclaims();
        let mut i = len;
        while t.style_reclaims().sweeps == before.sweeps {
            assert!(i < 2 * len, "no sweep ran on a full table");
            feed(t, format!("{}N", rendition(i, 15)).as_bytes());
            i += 1;
        }
        let swept = t.style_reclaims();
        assert_eq!(swept.freed, before.freed, "the control: every slot named");
        // Row 5 is 40 named cells, each its own rendition: overwritten.
        feed(t, b"\x1b[0m\x1b[5;1H");
        feed(t, &[b'k'; 40]);
        feed(t, b"\x1b[10;1H\r\n");
        let table = t.styles().to_vec();
        feed(t, format!("{}M", rendition(i, 15)).as_bytes());
        let after = t.style_reclaims();
        assert_eq!(after.sweeps, swept.sweeps, "{after:?}");
        assert_eq!(after.refusals, swept.refusals + 1, "{after:?}");
        assert_eq!(t.styles(), &table[..], "a named slot was written");
        let mut j = i + 1;
        while t.style_reclaims().sweeps == after.sweeps {
            assert!(j < i + len / 16 + 2, "no sweep within a sixteenth");
            feed(t, format!("{}M", rendition(j, 15)).as_bytes());
            j += 1;
        }
        let freed = t.style_reclaims();
        assert!(freed.freed - swept.freed >= 40, "{freed:?}");
        feed(t, format!("{}M", rendition(j, 15)).as_bytes());
        assert_eq!(t.style_reclaims().refusals, freed.refusals, "placed");
    });
}

/// A program that sweeps: 80 live cells over two rows, each in a rendition
/// of its own, then 1,100 renditions churned through a third row, some of it
/// scrolled into the history with the live cells. 1,181 renditions in all
/// pass the 1,024 slots, so a table that only evicts recolours live cells.
fn sweeping_program() -> Vec<u8> {
    let mut out = b"\x1b[H".to_vec();
    for i in 0..80 {
        out.extend(format!("\x1b[38;2;8;{i};1mL").as_bytes());
    }
    out.extend(b"\x1b[9;1H");
    for i in 0..1100 {
        out.extend(format!("\x1b[38;2;9;{};{}mC\x08", i >> 8, i & 255).as_bytes());
        if i % 97 == 0 {
            out.extend(b"\x1b[10;1H\n\x1b[9;1H");
        }
    }
    out
}

/// Everything a sweep's outcome shows: each screen cell's content, flags and
/// rendition, the live cells' renditions wherever they are, the whole table,
/// the write count and the reclaims.
#[derive(PartialEq, Debug)]
struct State {
    cells: Vec<(Content, CellFlags, StyleWord)>,
    live: Vec<StyleWord>,
    table: Vec<Option<StyleWord>>,
    writes: StyleWrites,
    reclaims: Reclaims,
}

fn state(t: &Term<'_>) -> State {
    let (_, rows) = t.geometry();
    let mut cells = Vec::new();
    for r in 0..rows.index() {
        for c in t.row(Row::from_index(r)).expect("a row on the grid") {
            cells.push((c.content, c.flags, t.style_of(*c)));
        }
    }
    // The live cells, `L`, oldest first: the history, then the screen.
    let is_l = |c: Content| {
        c.as_codepoint()
            .is_some_and(|cp| cp.value() == u32::from('L'))
    };
    let sb = t.scrollback();
    let mut live = Vec::new();
    let lines = (0..sb.len())
        .rev()
        .filter_map(|b| sb.line(Back::of(b)).map(|(l, _)| l));
    for line in lines {
        live.extend(
            line.iter()
                .filter(|c| is_l(c.content))
                .map(|c| t.style_of(*c)),
        );
    }
    live.extend(cells.iter().filter(|c| is_l(c.0)).map(|c| c.2));
    State {
        cells,
        live,
        table: t.styles().to_vec(),
        writes: t.style_writes(),
        reclaims: t.style_reclaims(),
    }
}

/// A short history, so a run of the program is cheap: the sweep's mark
/// walks every line the ring holds.
fn run(program: &[u8], cuts: &[usize]) -> State {
    let mut owned = Owned::with_history(&SMALL, &CacheGeometry::UNKNOWN, Depth::of(40));
    let t = &mut Term::new(owned.storage(), &SMALL);
    {
        let mut at = 0;
        for &e in cuts.iter().chain(core::iter::once(&program.len())) {
            let e = e.min(program.len());
            if e > at {
                feed(t, &program[at..e]);
                at = e;
            }
        }
        state(t)
    }
}

/// How many of the program's 80 live cells do not show the rendition they
/// were written in, or are missing.
fn recoloured(s: &State) -> usize {
    let want = (0..80).map(|i| fg((8, i, 1)));
    let right = want.zip(&s.live).filter(|(w, got)| w == *got).count();
    80 - right
}

/// **A sweep's outcome is the same at every read size and every split, and
/// keeps every live cell's rendition at each.** Reads of 1 to 64 bytes and
/// of `READ_CHUNK`, and the program cut in two at every byte: the screen,
/// the live cells, the table, the write count and the reclaims.
#[test]
fn a_sweep_is_chunk_invariant() {
    let p = sweeping_program();
    let whole = run(&p, &[]);
    let r = whole.reclaims;
    assert!(r.sweeps >= 1, "positive control: the program sweeps: {r:?}");
    assert_eq!(whole.live.len(), 80, "the control: every live cell found");
    let (mut runs, mut wrong, mut differ) = (0, 0, 0);
    let mut check = |s: State| {
        runs += 1;
        wrong += usize::from(recoloured(&s) > 0);
        differ += usize::from(s != whole);
    };
    check(run(&p, &[]));
    for n in (1..=64).chain([READ_CHUNK]) {
        let cuts: Vec<usize> = (1..).map(|k| k * n).take_while(|&e| e < p.len()).collect();
        check(run(&p, &cuts));
    }
    for split in 1..p.len() {
        check(run(&p, &[split]));
    }
    assert_eq!(
        (wrong, differ),
        (0, 0),
        "of {runs} runs: recoloured, differing"
    );
}
