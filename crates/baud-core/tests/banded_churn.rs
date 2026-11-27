//! **The alternate screen entered and left a hundred times with no line
//! feed, at 960x254 in bands of 32 rows**: nothing refused, nothing
//! recoloured, and the frozen pool flat.
//!
//! A row frozen stays frozen until something writes it, so a cycle that
//! writes nothing allocates nothing; the alternate screen's generations
//! die where its rows are next written; and each mode's clearing rule
//! (`?1049` clears on entry, `?1047` on leave, `?47` never) decides what
//! the alternate screen holds. Every written cell is checked against a
//! model: its character, and the rendition a single-table terminal gives
//! the same SGR. Every cycle checks the rows it wrote; whole screens, both
//! of them, are checked at cycles 1, 2, 25, 50, 75 and 100.
//!
//! `[[source: dec-std-070]]`'s SGR, page 5-104, note 3: "In no way does the
//! selection of current rendition affect the rendition of characters already
//! entered into the display."

use baud_core::*;
use baud_domain::{BytePos, Content, ReadLen, Row, Rows, StyleWord};
use std::collections::HashMap;

const WS: WinSize = WinSize {
    ws_col: 960,
    ws_row: 254,
};
const COLS: usize = 960;
const ROWS: usize = 254;
const BAND: usize = 32;

/// Rendition `i`, its own for every `i` below 65,536: an indexed
/// foreground and background.
fn rendition(i: usize) -> String {
    let b = i.to_le_bytes();
    format!("\x1b[0;38;5;{};48;5;{}m", b[0], b[1])
}

fn letter(i: usize) -> char {
    char::from(b'a' + u8::try_from(i % 26).unwrap_or(0))
}

fn feed(t: &mut Term<'_>, bytes: &[u8]) {
    for piece in bytes.chunks(READ_CHUNK) {
        t.write_at(BytePos::ZERO, piece);
        t.feed(BytePos::ZERO, ReadLen::from_index(piece.len()));
        t.clear_reply();
    }
}

/// **The rendition a single table gives rendition `i`**: a 400x150
/// terminal whose table never fills, the SGR and a letter written at home.
struct Oracle<'b> {
    term: Term<'b>,
    seen: HashMap<usize, StyleWord>,
}

/// Which rendition of the test's numbering: `rendition(i)`'s.
#[derive(Clone, Copy)]
struct Rendition(usize);

impl Oracle<'_> {
    fn word(&mut self, Rendition(i): Rendition) -> StyleWord {
        let i = i & 0xffff;
        if let Some(&w) = self.seen.get(&i) {
            return w;
        }
        let mut b = b"\x1b[H".to_vec();
        b.extend(rendition(i).as_bytes());
        b.extend(b"x\x1b[0m");
        feed(&mut self.term, &b);
        let view = self.term.viewport();
        let cell = view.row(Row::ZERO)[0];
        let w = view.styles(Row::ZERO).word(cell.style);
        self.seen.insert(i, w);
        w
    }
}

/// What a surface was written with, cell by cell: the rendition's index,
/// or nothing (a blank in the default).
type Surface = Vec<Option<usize>>;

/// A shape's columns and rows.
fn sides(ws: &WinSize) -> (usize, usize) {
    (usize::from(ws.ws_col), usize::from(ws.ws_row))
}

fn blank(ws: &WinSize) -> Surface {
    let (cols, rows) = sides(ws);
    vec![None; cols * rows]
}

/// `cols` cells of row `r` from column 0, each in rendition `first + x`,
/// with the cursor put there and no line feed.
fn write_row(ws: &WinSize, r: usize, first: usize, cols: usize, model: &mut Surface) -> Vec<u8> {
    let width = sides(ws).0;
    let mut b = format!("\x1b[{};1H", r + 1).into_bytes();
    for x in 0..cols {
        b.extend(rendition(first + x).as_bytes());
        b.extend(letter(first + x).encode_utf8(&mut [0; 4]).as_bytes());
        model[r * width + x] = Some(first + x);
    }
    b.extend(b"\x1b[0m");
    b
}

/// Every cell of screen rows `rows` against `model`: the content, and the
/// rendition its row's table resolves.
fn check(
    t: &Term<'_>,
    o: &mut Oracle<'_>,
    (ws, model): (&WinSize, &Surface),
    rows: impl Iterator<Item = usize>,
    when: &str,
) {
    let width = sides(ws).0;
    let view = t.viewport();
    for r in rows {
        let row = Row::from_index(r);
        let table = view.styles(row);
        for (x, c) in view.row(row).iter().enumerate().take(width) {
            let (content, word) = match model[r * width + x] {
                Some(i) => (
                    Content::codepoint(baud_domain::Codepoint::from_char(letter(i))),
                    o.word(Rendition(i)),
                ),
                None => (Content::EMPTY, StyleWord::DEFAULT),
            };
            assert_eq!(c.content, content, "{when}: row {r} column {x}'s content");
            assert_eq!(
                table.word(c.style),
                word,
                "{when}: row {r} column {x} in another rendition"
            );
        }
    }
}

/// The shape every test here needs before anything it checks means
/// anything: storage holding the whole 960x254 screen in bands of 32.
fn banded() -> Owned {
    let p = Provision::banded(&WS, BandRows::of(Rows::of_index(BAND))).expect("admitted banded");
    let owned = Owned::new(p);
    assert_eq!(
        owned.band_rows(),
        BandRows::of(Rows::of_index(BAND)),
        "bands of 32"
    );
    assert_eq!(owned.rows().index(), ROWS, "the whole 960x254 screen");
    owned
}

fn oracle(owned: &mut Owned) -> Oracle<'_> {
    let big = WinSize {
        ws_col: 400,
        ws_row: 150,
    };
    Oracle {
        term: Term::new(owned.storage(), &big),
        seen: HashMap::new(),
    }
}

/// The cycles whose whole screens are checked.
const WHOLE: [usize; 6] = [1, 2, 25, 50, 75, 100];

/// How an alternate screen is entered and left, and when it is cleared.
#[derive(Clone, Copy, Debug)]
enum Alt {
    /// `?1049`: cleared on entry.
    S1049,
    /// `?1047`: cleared on leave.
    S1047,
    /// `?47`: never cleared.
    S47,
}

impl Alt {
    fn enter(self) -> &'static [u8] {
        match self {
            Alt::S1049 => b"\x1b[?1049h",
            Alt::S1047 => b"\x1b[?1047h",
            Alt::S47 => b"\x1b[?47h",
        }
    }

    fn leave(self) -> &'static [u8] {
        match self {
            Alt::S1049 => b"\x1b[?1049l",
            Alt::S1047 => b"\x1b[?1047l",
            Alt::S47 => b"\x1b[?47l",
        }
    }
}

/// What a run of cycles saw: the pool's words and live generations after
/// each cycle.
struct Pool {
    words: Vec<usize>,
    gens: Vec<usize>,
}

/// **A hundred cycles on `owned` at `ws`**, the primary's screen filled
/// first, each cycle entering the alternate screen as `mode(cycle)` says,
/// writing `rows_a_cycle` rows of `cols` cells in new renditions from row
/// `cycle * rows_a_cycle`, and leaving. No line feed. Asserts no refusal and
/// every checked cell; returns the pool after each cycle.
fn cycles(
    (mut owned, ws): (Owned, WinSize),
    mode: impl Fn(usize) -> Alt,
    rows_a_cycle: Rows,
    cols: Cols,
) -> Pool {
    let (rows_a_cycle, cols) = (rows_a_cycle.index(), cols.index());
    let ws = &ws;
    let rows_on = sides(ws).1;
    let mut big = Owned::new(&WinSize {
        ws_col: 400,
        ws_row: 150,
    });
    let mut o = oracle(&mut big);
    let mut t = Term::new(owned.storage(), ws);
    let mut prim = blank(ws);
    let mut alt = blank(ws);
    let mut next = 0usize;
    let mut fill = Vec::new();
    for r in 0..rows_on {
        fill.extend(write_row(ws, r, next, 8, &mut prim));
        next += 8;
    }
    feed(&mut t, &fill);
    check(&t, &mut o, (ws, &prim), 0..rows_on, "the primary filled");
    let mut pool = Pool {
        words: Vec::new(),
        gens: Vec::new(),
    };
    for c in 1..=100 {
        let m = mode(c);
        if matches!(m, Alt::S1049) {
            alt = blank(ws);
        }
        let mut b = m.enter().to_vec();
        let at = (c * rows_a_cycle) % rows_on;
        let rows: Vec<usize> = (at..at + rows_a_cycle).map(|r| r % rows_on).collect();
        for &r in &rows {
            b.extend(write_row(ws, r, next, cols, &mut alt));
            next += cols;
        }
        feed(&mut t, &b);
        let when = format!("cycle {c} {m:?}, on the alternate screen");
        check(&t, &mut o, (ws, &alt), rows.iter().copied(), &when);
        if WHOLE.contains(&c) {
            check(&t, &mut o, (ws, &alt), 0..rows_on, &when);
        }
        feed(&mut t, m.leave());
        if matches!(m, Alt::S1047) {
            alt = blank(ws);
        }
        let when = format!("cycle {c} {m:?}, back on the primary");
        check(&t, &mut o, (ws, &prim), rows.iter().copied(), &when);
        if WHOLE.contains(&c) {
            check(&t, &mut o, (ws, &prim), 0..rows_on, &when);
        }
        assert_eq!(t.style_reclaims().refusals, 0, "cycle {c}: refused");
        let r = t.frozen();
        pool.words.push(r.words_held);
        pool.gens.push(r.gens_alive);
    }
    pool
}

/// **`?1049` churn**: a screen of new renditions on the alternate screen
/// each cycle. The pool after cycle 100 is what it was after cycle 1.
#[test]
fn the_1049_churn_with_no_line_feed_is_flat() {
    let p = cycles(
        (banded(), WS),
        |_| Alt::S1049,
        Rows::of_index(ROWS),
        Cols::of_index(8),
    );
    assert_eq!(
        (p.words[99], p.gens[99]),
        (p.words[0], p.gens[0]),
        "the pool moved"
    );
}

/// **`?47` churn** (the alternate screen is never cleared): six new rows a
/// cycle, so every alternate row has been written by cycle 43. The pool
/// stays within two screens' slack of cycle 1, and is flat from cycle 50.
#[test]
fn the_47_churn_with_no_line_feed_is_flat_once_every_row_is_written() {
    let p = cycles(
        (banded(), WS),
        |_| Alt::S47,
        Rows::of_index(6),
        Cols::of_index(8),
    );
    let most = p.words.iter().copied().max().unwrap_or(p.words[0]);
    assert!(
        most <= p.words[0] + 2 * ROWS * COLS,
        "the pool rose to {most} words from {}, past two screens",
        p.words[0]
    );
    for c in 50..100 {
        assert_eq!(
            (p.words[c], p.gens[c]),
            (p.words[49], p.gens[49]),
            "cycle {} moved",
            c + 1
        );
    }
}

/// **`?1047` churn**: cleared on leave, so the alternate screen freezes
/// nothing and holds no generation; the pool is what cycle 1 left it.
#[test]
fn the_1047_churn_freezes_nothing_for_the_alternate_screen() {
    let p = cycles(
        (banded(), WS),
        |_| Alt::S1047,
        Rows::of_index(6),
        Cols::of_index(8),
    );
    for c in 1..100 {
        assert_eq!(
            (p.words[c], p.gens[c]),
            (p.words[0], p.gens[0]),
            "cycle {} moved",
            c + 1
        );
    }
}

/// **`?47` and `?1047` taking turns over one alternate screen**: a `?1047`
/// leave blanks what a `?47` cycle left frozen, so those generations die
/// where they lie. Nothing refused or recoloured, and the pool within two
/// screens' slack of cycle 1.
#[test]
fn the_47_and_1047_churn_taking_turns_stays_within_the_slack() {
    let p = cycles(
        (banded(), WS),
        |c| match c % 2 {
            0 => Alt::S47,
            _ => Alt::S1047,
        },
        Rows::of_index(6),
        Cols::of_index(8),
    );
    let first = p.words[0];
    let most = p.words.iter().copied().max().unwrap_or(first);
    assert!(
        most <= first + 2 * ROWS * COLS,
        "the pool rose to {most} words from {first}, past two screens"
    );
}

/// **The model reads true where one table holds the screen**: the same
/// cycles, every mode, in single mode at 200x50. A failure here is the
/// model's, not the bands'; this is what lets a failure above be read as
/// the bands'.
#[test]
fn the_model_reads_a_single_table_terminal_true() {
    let ws = || WinSize {
        ws_col: 200,
        ws_row: 50,
    };
    for mode in [Alt::S1049, Alt::S1047, Alt::S47] {
        cycles(
            (Owned::new(ws()), ws()),
            |_| mode,
            Rows::of_index(6),
            Cols::of_index(8),
        );
    }
    cycles(
        (Owned::new(ws()), ws()),
        |c| match c % 2 {
            0 => Alt::S47,
            _ => Alt::S1047,
        },
        Rows::of_index(6),
        Cols::of_index(8),
    );
}

/// **Fifty alternate-screen cycles, each followed by one cell written into
/// every row of the primary's screen** on `owned` at `ws`: each write thaws
/// a frozen row out of the middle of the pool. Asserts no refusal and every
/// written cell; returns the pool's words after each cycle.
fn holes((mut owned, ws): (Owned, WinSize)) -> Vec<usize> {
    let ws = &ws;
    let rows_on = sides(ws).1;
    let mut big = Owned::new(&WinSize {
        ws_col: 400,
        ws_row: 150,
    });
    let mut o = oracle(&mut big);
    let mut t = Term::new(owned.storage(), ws);
    let mut prim = blank(ws);
    let mut next = 0usize;
    let mut fill = Vec::new();
    for r in 0..rows_on {
        fill.extend(write_row(ws, r, next, 8, &mut prim));
        next += 8;
    }
    feed(&mut t, &fill);
    let mut words = Vec::new();
    for c in 1..=50 {
        feed(&mut t, b"\x1b[?1049h\x1b[?1049l");
        let mut b = Vec::new();
        for r in 0..rows_on {
            b.extend(write_row(ws, r, next, 1, &mut prim));
            next += 1;
        }
        feed(&mut t, &b);
        let when = format!("cycle {c}, a cell written into every row");
        check(&t, &mut o, (ws, &prim), 0..rows_on, &when);
        assert_eq!(t.style_reclaims().refusals, 0, "cycle {c}: refused");
        words.push(t.frozen().words_held);
    }
    words
}

/// **The holes a written screen leaves stay within one screen**: fifty
/// cycles, each thawing every row of the primary's screen by a write, and
/// the pool never more than one screen of words above cycle 1.
#[test]
fn a_written_screen_leaves_at_most_one_screen_of_holes() {
    let words = holes((banded(), WS));
    let most = words.iter().copied().max().unwrap_or(words[0]);
    assert!(
        most <= words[0] + ROWS * COLS,
        "the pool rose to {most} words from {}, past one screen",
        words[0]
    );
}

/// The model of [`a_written_screen_leaves_at_most_one_screen_of_holes`],
/// read true in single mode at 200x50.
#[test]
fn the_holes_model_reads_a_single_table_terminal_true() {
    let ws = || WinSize {
        ws_col: 200,
        ws_row: 50,
    };
    holes((Owned::new(ws()), ws()));
}
