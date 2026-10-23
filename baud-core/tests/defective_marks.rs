#![deny(dead_code)]
//! **A zero-width character after a control is a cluster of its own**
//! (`[[source: uax29]]` GB4 and GB5 break after and before Control, CR and
//! LF, and every control function is introduced by one).
//!
//! A combining mark there is a defective combining character sequence, TUS
//! D57: "Defective combining character sequences occur when a sequence of
//! combining characters appears at the start of a string or follows a
//! control or format character" (`[[source: unicode18-ch3]]`), "rendered as
//! if they had a no-break space as a base character" (TUS 5.13,
//! `[[source: unicode18-ch5]]`). It takes the cursor's cell, stored alone
//! with `Content::DEFECTIVE` for its base, so a copy returns what the
//! program wrote (note AB, Q24 item 1; Jev, OWN_CELL_MARK_ALONE, 0.96/0.99).
//! A default-ignorable one draws nothing and has no cell to take: dropped
//! and counted (Jev's lean, 0.22/0.48, over keeping a join GB4/GB5 forbid).
//! Until 2026-10-23 each went to the cell before the cursor.

use baud_core::*;

/// A terminal fed `bytes` in reads of `size`, handed to `f` while it lives.
fn fed<R>(bytes: &[u8], size: usize, f: impl FnOnce(&Term<'_>) -> R) -> R {
    let ws = WinSize {
        ws_row: 4,
        ws_col: 20,
    };
    let mut s = Owned::new(&ws);
    let mut t = Term::new(s.storage(), &ws);
    for part in bytes.chunks(size) {
        t.write_at(BytePos::ZERO, part);
        t.feed(BytePos::ZERO, ReadLen::from_index(part.len()));
    }
    f(&t)
}

/// One cell: `None` blank, else its base -- `Content::DEFECTIVE` as `None`
/// inside -- and its marks.
type Cell = Option<(Option<u32>, Vec<u32>)>;

/// Row `y`'s first eight cells.
fn cells(t: &Term<'_>, y: usize) -> Vec<Cell> {
    let row = t.row(Row::from_index(y)).expect("a row");
    (0..8)
        .map(|x| {
            let c = row[x];
            if c.content.is_empty() {
                return None;
            }
            let base = t.base_of(c);
            let marks = t.marks(Row::from_index(y), Col::at_index(x)).collect();
            let base = match base == Content::DEFECTIVE {
                true => None,
                false => base.as_codepoint().map(|b| b.value()),
            };
            Some((base, marks))
        })
        .collect()
}

fn letter(c: char) -> Cell {
    Some((Some(u32::from(c)), Vec::new()))
}

fn alone(marks: &[u32]) -> Cell {
    Some((None, marks.to_vec()))
}

/// **After each control a combining mark takes the cursor's cell alone**,
/// and the cursor moves past it: `ab` then the control then U+0308, for a
/// control that moves nothing (`NUL`, `BEL`, `DEL`, an `SGR`, a mode change,
/// an `OSC`), one that moves back (`BS`, `CUB`), forward (`CUF`, `HT`) or to
/// column 0 (`CR`), a line feed, and an erase (`EL 1`). Every read size. A
/// mark straight after the `b` is the control, joining it, unchanged.
#[test]
fn a_combining_mark_after_a_control_takes_a_cell_of_its_own() {
    let b = letter('b');
    let a = letter('a');
    let dia = alone(&[0x308]);
    let cases: [(&str, &str, usize, Vec<Cell>); 13] = [
        ("NUL", "\x00", 0, vec![a.clone(), b.clone(), dia.clone()]),
        ("BEL", "\x07", 0, vec![a.clone(), b.clone(), dia.clone()]),
        ("DEL", "\x7f", 0, vec![a.clone(), b.clone(), dia.clone()]),
        (
            "SGR 31",
            "\x1b[31m",
            0,
            vec![a.clone(), b.clone(), dia.clone()],
        ),
        (
            "DECTCEM",
            "\x1b[?25l",
            0,
            vec![a.clone(), b.clone(), dia.clone()],
        ),
        (
            "OSC 0",
            "\x1b]0;t\x07",
            0,
            vec![a.clone(), b.clone(), dia.clone()],
        ),
        ("BS", "\x08", 0, vec![a.clone(), dia.clone()]),
        ("CUB 2", "\x1b[2D", 0, vec![dia.clone(), b.clone()]),
        (
            "CUF 1",
            "\x1b[C",
            0,
            vec![a.clone(), b.clone(), None, dia.clone()],
        ),
        ("CR", "\r", 0, vec![dia.clone(), b.clone()]),
        ("EL 1", "\x1b[1K", 0, vec![None, None, dia.clone()]),
        ("LF", "\n", 1, vec![None, None, dia.clone()]),
        (
            "HT",
            "\t",
            0,
            vec![a.clone(), b.clone(), None, None, None, None, None, None],
        ),
    ];
    for (name, control, row, mut want) in cases {
        want.resize(8, None);
        let bytes = format!("ab{control}\u{308}");
        for size in 1..=bytes.len() {
            fed(bytes.as_bytes(), size, |t| {
                assert_eq!(cells(t, row), want, "{name}, read size {size}");
                assert_eq!(
                    t.marks_dropped, 0,
                    "{name}, read size {size}: nothing dropped"
                );
            });
        }
    }
    // HT lands on column 8, past the eight cells read: the mark is there.
    fed("ab\t\u{308}".as_bytes(), 1, |t| {
        let marks: Vec<u32> = t.marks(Row::ZERO, Col::at_index(8)).collect();
        assert_eq!(marks, vec![0x308]);
        assert_eq!(t.cursor().1.index(), 9);
    });
    // The control: no control between, the mark joins the `b`.
    fed("ab\u{308}".as_bytes(), 1, |t| {
        let mut want = vec![a.clone(), Some((Some(0x62), vec![0x308]))];
        want.resize(8, None);
        assert_eq!(cells(t, 0), want);
    });
}

/// **What joins a defective mark joins it**: a second mark, and a spacing
/// mark after it, go into its cell, the cursor one past it.
#[test]
fn what_follows_a_defective_mark_joins_it() {
    let bytes = "ab\x1b[m\u{301}\u{308}\u{903}x";
    for size in 1..=bytes.len() {
        fed(bytes.as_bytes(), size, |t| {
            let mut want = vec![
                letter('a'),
                letter('b'),
                alone(&[0x301, 0x308, 0x903]),
                letter('x'),
            ];
            want.resize(8, None);
            assert_eq!(cells(t, 0), want, "read size {size}");
        });
    }
}

/// **A default-ignorable after a control draws nothing and is dropped**, so
/// it cannot change the character before it: `U+2764 SGR U+FE0F` leaves the
/// heart its text presentation in one cell, where the selector widened it,
/// and a ZWJ and a tag go too, each counted. A mark after the dropped one
/// starts a cluster of its own, as after a control.
#[test]
fn a_default_ignorable_after_a_control_is_dropped() {
    let cases: [(&str, &str, Vec<Cell>, usize); 4] = [
        (
            "VS16",
            "\u{2764}\x1b[m\u{fe0f}",
            vec![letter('\u{2764}')],
            1,
        ),
        ("ZWJ", "x\x1b[m\u{200d}", vec![letter('x')], 1),
        ("a tag", "x\x1b[m\u{e0061}", vec![letter('x')], 1),
        (
            "ZWJ then a mark",
            "x\x1b[m\u{200d}\u{301}",
            vec![letter('x'), alone(&[0x301])],
            1,
        ),
    ];
    for (name, bytes, mut want, dropped) in cases {
        want.resize(8, None);
        for size in 1..=bytes.len() {
            fed(bytes.as_bytes(), size, |t| {
                assert_eq!(cells(t, 0), want, "{name}, read size {size}");
                assert_eq!(t.marks_dropped, dropped, "{name}, read size {size}");
            });
        }
    }
}

/// **`REP` repeats a defective mark as one**: each copy a cell of its own.
#[test]
fn rep_repeats_a_defective_mark_as_a_cell_of_its_own() {
    let bytes = "a\x1b[m\u{301}\x1b[2b";
    for size in 1..=bytes.len() {
        fed(bytes.as_bytes(), size, |t| {
            let mark = alone(&[0x301]);
            let mut want = vec![letter('a'), mark.clone(), mark.clone(), mark];
            want.resize(8, None);
            assert_eq!(cells(t, 0), want, "read size {size}");
        });
    }
}

/// **A defective mark is summed and printed as the character written**:
/// DECRQCRA with xterm's full codes (`CSI 16 # y`) sums its code, where its
/// base, no character, summed as `ESC`; and a printed line carries the mark
/// with no space for the base it is drawn on.
#[test]
fn a_defective_mark_is_summed_and_printed_as_the_mark() {
    let ws = WinSize {
        ws_row: 4,
        ws_col: 20,
    };
    let mut s = Owned::new(&ws).with_printer(PrintBudget::of_bytes(4096));
    let mut t = Term::new(s.storage(), &ws);
    t.set_printer(PrinterPort::Ready);
    for bytes in ["\x1b[16#ya\x00\u{301}", "\x1b[7;1;1;1;1;2*y", "\x1b[?1i"] {
        t.write_at(BytePos::ZERO, bytes.as_bytes());
        t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
    }
    let want = format!("\x1bP7!~{:04X}\x1b\\", (0x61u16 + 0x301).wrapping_neg());
    assert_eq!(String::from_utf8_lossy(t.reply()), want, "the sum");
    let jobs: Vec<String> = t
        .print_jobs()
        .map(|j| String::from_utf8_lossy(j.bytes).into_owned())
        .collect();
    assert_eq!(jobs, ["\x1b[0m\x1b#5a\u{301}\r\n\x1b[0m"], "the print");
}
