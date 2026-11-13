//! **A full rendition table recolours no drawn cell, whatever moves the rows
//! across the active screen's edge**: generated programs of floods of
//! renditions, plain lines, reverse indexes at the top, the alternate
//! screen, resizes, `ED 3`, `RIS` and prompt marks, fed to a 1,024-slot
//! table lent generations (`Owned::new` at 40x10) and to a 65,536-slot one
//! that never fills (`Owned::new` at 400x150, drawn at the same window).
//! After every step each retained line and the screen up read, through its
//! row's table, the reference's contents and renditions, and the small
//! table refuses none.
//!
//! `[[source: dec-std-070]]`'s SGR, page 5-104, note 3: "In no way does the
//! selection of current rendition affect the rendition of characters already
//! entered into the display."
//!
//! [`the_generator_reaches_the_freeze`] is the reach control: a property
//! whose programs never froze would pass whatever the freeze did.

use baud_core::*;
use proptest::prelude::*;

const WS: WinSize = WinSize {
    ws_row: 10,
    ws_col: 40,
};

const BIG: WinSize = WinSize {
    ws_row: 150,
    ws_col: 400,
};

#[derive(Clone, Copy, Debug)]
enum Atom {
    /// Lines of 40 cells, each cell a rendition of its own.
    Flood(u8),
    /// Lines in the default rendition.
    Plain(u8),
    /// Reverse indexes at the top line, each taking a line down the screen.
    Ri(u8),
    /// `?1049` set, a flood on the alternate screen, and reset.
    Alt(u8),
    /// A window of these columns and rows, within 40x10.
    Resize(u16, u16),
    /// `ED 3`: the history erased.
    Ed3,
    /// `RIS`.
    Ris,
    /// `OSC 133 ; A`, and a prompt.
    Prompt,
}

fn atom() -> impl Strategy<Value = Atom> {
    prop_oneof![
        8 => (1u8..=12).prop_map(Atom::Flood),
        2 => (1u8..=3).prop_map(Atom::Plain),
        1 => (1u8..=4).prop_map(Atom::Ri),
        1 => (0u8..=4).prop_map(Atom::Alt),
        1 => (prop_oneof![Just(30u16), Just(40u16)], prop_oneof![Just(6u16), Just(8u16), Just(10u16)])
            .prop_map(|(c, r)| Atom::Resize(c, r)),
        1 => Just(Atom::Ed3),
        1 => Just(Atom::Ris),
        1 => Just(Atom::Prompt),
    ]
}

fn program() -> impl Strategy<Value = Vec<Atom>> {
    proptest::collection::vec(atom(), 4..24)
}

/// One step's bytes, its renditions numbered on from `next`.
fn bytes(a: Atom, next: &mut usize) -> Vec<u8> {
    let mut flood = |lines: u8, out: &mut Vec<u8>| {
        for _ in 0..lines {
            for _ in 0..40 {
                let b = next.to_le_bytes();
                *next += 1;
                out.extend(
                    format!(
                        "\x1b[0;38;2;{};{};3;48;2;3;{};{}m{}",
                        b[0],
                        b[1],
                        b[2],
                        b[0],
                        char::from(b'a' + b[0] % 26)
                    )
                    .as_bytes(),
                );
            }
            out.extend(b"\x1b[0m\r\n");
        }
    };
    let mut out = Vec::new();
    match a {
        Atom::Flood(n) => flood(n, &mut out),
        Atom::Plain(n) => {
            for i in 0..n {
                out.extend(format!("plain {i}\r\n").as_bytes());
            }
        }
        Atom::Ri(n) => {
            out.extend(b"\x1b[H");
            for _ in 0..n {
                out.extend(b"\x1bM");
            }
            out.extend(b"\x1b[999;1H");
        }
        Atom::Alt(n) => {
            out.extend(b"\x1b[?1049h");
            flood(n, &mut out);
            out.extend(b"\x1b[?1049l");
        }
        Atom::Ed3 => out.extend(b"\x1b[3J"),
        Atom::Ris => out.extend(b"\x1bc"),
        Atom::Prompt => out.extend(b"\x1b]133;A\x07$ \r\n"),
        Atom::Resize(..) => {}
    }
    out
}

fn feed(t: &mut Term<'_>, bytes: &[u8]) {
    for piece in bytes.chunks(4096) {
        t.write_at(BytePos::ZERO, piece);
        t.feed(BytePos::ZERO, ReadLen::from_index(piece.len()));
        t.clear_reply();
    }
}

/// Every retained line, newest first, then the screen up: each cell's
/// content and the rendition its row's table names.
fn read(t: &Term<'_>, lines: usize) -> Vec<Vec<(Content, StyleWord)>> {
    let mut out = Vec::new();
    let sb = t.scrollback();
    for back in 0..lines {
        let back = Back::of(back);
        let (cells, _) = sb.line(back).expect("a retained line");
        let table = sb.styles(back);
        out.push(
            cells
                .iter()
                .map(|c| (c.content, t.style_in(table, *c)))
                .collect(),
        );
    }
    let view = t.viewport();
    for y in 0..view.rows().index() {
        let r = Row::from_index(y);
        let table = view.styles(r);
        out.push(
            view.row(r)
                .iter()
                .map(|c| (c.content, t.style_in(table, *c)))
                .collect(),
        );
    }
    out
}

/// Every physical row of the small terminal's three surfaces: its cells,
/// its mark, and whether it was written after `kept`.
type Snap = Vec<(Which, usize, Vec<PackedCell>, RowExtent, RowDirt)>;

fn snap(t: &Term<'_>, kept: &Seal) -> Snap {
    [Which::Primary, Which::Alternate, Which::Status]
        .into_iter()
        .flat_map(|w| {
            t.stored_rows(w, kept)
                .map(move |(at, c, m, d)| (w, at.p.index(), c.to_vec(), m, d))
        })
        .collect()
}

/// What one program did to the small table: its freezes and thaws.
fn run(prog: &[Atom]) -> Result<baud_domain::FreezeCounts, TestCaseError> {
    let mut small_owned = Owned::new(&WS);
    let mut s = Term::new(small_owned.storage(), &WS);
    let mut big_owned = Owned::new(&BIG);
    let mut b = Term::new(big_owned.storage(), &WS);
    prop_assert_eq!(s.styles().len(), 1024);
    let mut next = 1usize;
    for (k, &a) in prog.iter().enumerate() {
        match a {
            Atom::Resize(cols, rows) => {
                let ws = WinSize {
                    ws_col: cols,
                    ws_row: rows,
                };
                let kept = s.seal_writes();
                let before = snap(&s, &kept);
                let _ = s.resize(&ws);
                let _ = b.resize(&ws);
                moved(&s, &kept, &before, k, a)?;
            }
            _ => {
                // The switch back from the alternate screen in a read of its
                // own, under a seal of its own: the thaw it runs rewrites
                // rows the flood before it froze, and the freeze stamped
                // them already under a seal shared by both.
                let bytes = bytes(a, &mut next);
                let back = b"\x1b[?1049l";
                let cut = bytes
                    .windows(back.len())
                    .position(|w| w == back)
                    .unwrap_or(bytes.len());
                for part in [&bytes[..cut], &bytes[cut..]] {
                    let kept = s.seal_writes();
                    let before = snap(&s, &kept);
                    feed(&mut s, part);
                    feed(&mut b, part);
                    moved(&s, &kept, &before, k, a)?;
                }
            }
        }
        let lines = s.scrollback().len().min(b.scrollback().len());
        let (rs, rb) = (read(&s, lines), read(&b, lines));
        prop_assert_eq!(rs.len(), rb.len(), "step {}: lines", k);
        for (i, (ls, lb)) in rs.iter().zip(&rb).enumerate() {
            prop_assert!(ls == lb, "step {} {:?}: line {} drawn otherwise", k, a, i);
        }
        prop_assert_eq!(s.style_reclaims().refusals, 0, "step {}: refused", k);
    }
    Ok(s.frozen().counts)
}

/// **A seal a step, and every row the step changed moves**: the freezes
/// rewrite rows' ids, the thaws in two passes, the rebuild and a re-cut's
/// `frozen_normalized` too, and a row whose ids changed under a stamp that
/// did not move would be drawn by a renderer through its old copy.
fn moved(s: &Term<'_>, kept: &Seal, before: &Snap, k: usize, a: Atom) -> Result<(), TestCaseError> {
    for (old, new) in before.iter().zip(snap(s, kept)) {
        if new.4 == RowDirt::Clean {
            prop_assert!(
                old.2 == new.2 && old.3 == new.3,
                "step {} {:?}: {:?} row {} changed and reads clean",
                k,
                a,
                new.0,
                new.1
            );
        }
    }
    Ok(())
}

proptest! {
    #[test]
    fn a_full_table_recolours_no_drawn_cell(prog in program()) {
        run(&prog)?;
    }
}

/// **A blank cell a re-cut writes into a generation stays blank**: a window
/// narrowed to 30 columns, floods that freeze its history, and a widening
/// that re-cuts the lines spanning two tables into one generation. Each
/// default cell of the line keeps id 0, as the generation's default is,
/// so the line still reads to its mark (`Scrollback::line`) and not to
/// the window's width. Found by the property above at 20,000 cases.
#[test]
fn a_recut_keeps_a_blank_cell_blank() {
    use Atom::*;
    let prog = [
        Resize(30, 6),
        Flood(4),
        Flood(4),
        Flood(5),
        Ri(2),
        Flood(9),
        Resize(40, 6),
    ];
    let ran = run(&prog).expect("the line read as the reference reads it");
    assert!(ran.freezes > 0, "the control: it froze");
}

/// **The generator reaches the freeze, and moves frozen rows back**: over a
/// fixed sample, most programs freeze and some thaw, so the property above
/// tests the freeze rather than a table that never filled.
#[test]
fn the_generator_reaches_the_freeze() {
    use proptest::strategy::ValueTree;
    let mut runner = proptest::test_runner::TestRunner::deterministic();
    let (mut froze, mut thawed) = (0, 0);
    let n = 200;
    for _ in 0..n {
        let prog = program().new_tree(&mut runner).expect("tree").current();
        let ran = run(&prog).expect("the property holds on the sample");
        froze += usize::from(ran.freezes > 0);
        thawed += usize::from(ran.rows_thawed > 0);
    }
    eprintln!("{froze} of {n} programs froze, {thawed} thawed a frozen row");
    assert!(froze * 2 > n, "only {froze} of {n} programs froze");
    assert!(thawed > 0, "no program thawed a frozen row");
}
