#![deny(dead_code)]
//! **A row whose mark is 0 holds no written cell.**
//!
//! `RowExtent` is a high-water mark of glyph writes (note F,
//! `docs/research/notes/f-row-extent.md`): a fill stores 0 only when what it
//! fills with is `Content::EMPTY`, whatever its rendition, and every glyph
//! write raises it. An embedder may therefore read a mark of 0 as "nothing
//! written on this row", and the app does: the line a part ended on is
//! judged empty by it (`Term::row_extent`, the app's `vouch::EndRow`). This
//! file pins that reading for every way content reaches a row -- printing,
//! `REP`, `DECALN`, `DECFRA` with a character and with a space, `DECCRA`,
//! `IL`/`DL`/`SU`/`SD`, scrolls inside top/bottom and side margins, `ICH`
//! and `DCH` on a written row, `ECH`/`EL`/`ED` under a coloured
//! background, and a wide character at the right edge -- after every atom
//! of a generated program, on both screens.

use baud_core::*;
use proptest::prelude::*;

const ROWS: u16 = 8;
const COLS: u16 = 20;

/// The program's atoms: each a complete sequence, so any order is a valid
/// stream.
const ATOMS: &[&[u8]] = &[
    b"ab",
    b"the quick brown fox jumps",
    "\u{4e2d}".as_bytes(),
    b"\x1b[20G\xe4\xb8\xad",
    b"\x1b[19G\xe4\xb8\xad",
    b"\x1b[3b",
    b"\x1b#8",
    b"\x1b[120;2;2;4;6$x",
    b"\x1b[32;1;1;3;20$x",
    b"\x1b[1;1;3;10;1;5;3;1$v",
    b"\x1b[2L",
    b"\x1b[2M",
    b"\x1b[2S",
    b"\x1b[2T",
    b"\x1b[2;5r",
    b"\x1b[r",
    b"\x1b[?69h\x1b[3;12s",
    b"\x1b[?69l",
    b"\n",
    b"\r\n",
    b"\x1bM",
    b"\x1bE",
    b"\x1b[3@",
    b"\x1b[3P",
    b"\x1b[44m\x1b[4X",
    b"\x1b[44m\x1b[K",
    b"\x1b[44m\x1b[1K",
    b"\x1b[44m\x1b[2K",
    b"\x1b[44m\x1b[J",
    b"\x1b[m",
    b"\x1b[3;18H",
    b"\x1b[8;1H",
    b"\x1b[H",
    b"\x1b[?1049h",
    b"\x1b[?1049l",
    b"\x1b[?7l",
    b"\x1b[?7h",
];

fn feed(t: &mut Term<'_>, b: &[u8]) {
    t.write_at(BytePos::ZERO, b);
    t.feed(BytePos::ZERO, ReadLen::from_index(b.len()));
}

/// The first row whose mark is 0 while a cell of it holds content, if any.
fn written_under_zero(t: &Term<'_>) -> Option<(usize, String)> {
    (0..usize::from(ROWS)).find_map(|y| {
        let r = Row::from_index(y);
        if t.row_extent(r).index() != 0 {
            return None;
        }
        let row = t.row(r).expect("a row on the grid");
        row.iter().any(|c| c.content != Content::EMPTY).then(|| {
            let shown: String = row
                .iter()
                .map(|c| match c.content {
                    Content::EMPTY => '.',
                    _ => '#',
                })
                .collect();
            (y, shown)
        })
    })
}

proptest! {
    /// **After every atom, a row at mark 0 holds only empty cells.** Named
    /// break: a glyph fill that stores mark 0 (`View::fill_row_as` storing
    /// 0 whatever it fills with) -- `DECALN`'s rows read 0 and hold `E`.
    #[test]
    fn a_row_at_mark_zero_holds_no_written_cell(
        prog in proptest::collection::vec(0..ATOMS.len(), 0..40)
    ) {
        let ws = WinSize { ws_row: ROWS, ws_col: COLS };
        let mut owned = Owned::new(&ws);
        let mut t = Term::new(owned.storage(), &ws);
        for (n, &i) in prog.iter().enumerate() {
            feed(&mut t, ATOMS[i]);
            prop_assert_eq!(
                written_under_zero(&t),
                None,
                "after atom {} of {:?}",
                n,
                prog.iter().map(|&k| String::from_utf8_lossy(ATOMS[k]).into_owned()).collect::<Vec<_>>()
            );
        }
    }
}

/// **Each fill the property reaches, by hand**: `DECALN`, `DECFRA` with a
/// character, a full row of one character, and spaces written under a
/// coloured background each leave a mark past 0; the control, a row erased
/// under a coloured background, reads 0 and holds nothing written.
#[test]
fn each_way_of_writing_a_row_leaves_its_mark_past_zero() {
    let ws = WinSize {
        ws_row: ROWS,
        ws_col: COLS,
    };
    for (what, bytes, row) in [
        ("DECALN", b"\x1b#8".as_slice(), 3),
        ("DECFRA x", b"\x1b[120;2;1;2;20$x", 1),
        (
            "a full row of one character",
            b"\x1b[2;1Hxxxxxxxxxxxxxxxxxxxx",
            1,
        ),
        ("spaces under a background", b"\x1b[44m\x1b[2;1H   ", 1),
    ] {
        let mut owned = Owned::new(&ws);
        let mut t = Term::new(owned.storage(), &ws);
        feed(&mut t, bytes);
        assert!(
            t.row_extent(Row::from_index(row)).index() > 0,
            "{what}: row {row}"
        );
    }
    let mut owned = Owned::new(&ws);
    let mut t = Term::new(owned.storage(), &ws);
    feed(&mut t, b"\x1b[2;1Hab\x1b[44m\x1b[2K");
    assert_eq!(written_under_zero(&t), None, "the control: erased");
    assert!(
        t.row(Row::from_index(1))
            .expect("row 1")
            .iter()
            .all(|c| c.content == Content::EMPTY),
        "the control holds nothing written"
    );
}
