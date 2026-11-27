//! One saved cursor per screen, and `?1049`'s save is the primary's.
//!
//! `[[source: xterm-cursor-c]]`: `CursorSave` writes
//! `screen->sc[screen->whichBuf]` and `CursorRestore` reads it, so `ESC 7`,
//! `ESC 8`, `CSI s`, `CSI u` and `?1048` each reach the slot of the screen
//! shown. `?1049` is "Save cursor as in DECSC ... This control combines the
//! effects of the 1 0 4 7 and 1 0 4 8 modes" (`[[source: ctlseqs]]`), and
//! xterm's set arm is `CursorSave; ToAlternate; ClearScreen`, its reset arm
//! `FromAlternate; CursorRestore` (`xterm-charproc.c`,
//! `srm_OPT_ALTBUF_CURSOR`): the save goes into the slot of the screen it is
//! made on, and the restore reads the primary's once the primary is back.
//! DECRQM `?1048` answers `sc[screen->whichBuf].saved` (`[[source:
//! xterm-misc]]` 5717).
mod support;
use baud_core::{Owned, Screen, Term};
use baud_domain::{Col, Row};

/// Feed `bytes` at read sizes 1, 3, 7 and whole, and hand each terminal to
/// `f`.
fn each_read_size(bytes: &[u8], f: impl Fn(&mut Term<'_>)) {
    for chunk in [1, 3, 7, bytes.len().max(1)] {
        let mut owned = Owned::new(&support::CORPUS);
        let mut t = Term::new(owned.storage(), &support::CORPUS);
        for piece in bytes.chunks(chunk) {
            support::feed(&mut t, piece);
        }
        f(&mut t);
    }
}

fn at(row: usize, col: usize) -> (Row, Col) {
    (Row::from_index(row), Col::at_index(col))
}

/// `DECRQM ?1048` after `bytes`.
fn decrqm_1048(bytes: &[u8]) -> Vec<u8> {
    let mut owned = Owned::new(&support::CORPUS);
    let mut t = Term::new(owned.storage(), &support::CORPUS);
    support::feed(&mut t, bytes);
    t.clear_reply();
    support::feed(&mut t, b"\x1b[?1048$p");
    t.reply().to_vec()
}

/// **rxvt's `smcup` and `rmcup` bring the shell's cursor back**: `?1048 h`
/// saves on the primary, `ESC 7` on the alternate screen saves into the
/// alternate screen's slot, and `?1048 l` on the primary reads the
/// primary's. One shared slot restored the alternate screen's cursor.
#[test]
fn a_save_on_the_alternate_screen_leaves_the_primarys() {
    each_read_size(
        b"\x1b[5;10H\x1b[?1048h\x1b[?1047h\x1b[3;3H\x1b7\x1b[?1047l\x1b[?1048l",
        |t| {
            assert!(t.screen() == Screen::Normal);
            assert_eq!(
                t.cursor(),
                at(4, 9),
                "the primary's save, not the alternate's"
            );
        },
    );
    // The same through `ESC 7` and `?47`, which save and switch nothing else.
    each_read_size(
        b"\x1b[5;5H\x1b7\x1b[?47h\x1b[3;3H\x1b7\x1b[?47l\x1b8",
        |t| assert_eq!(t.cursor(), at(4, 4)),
    );
}

/// **`DECRC` on the alternate screen reads the alternate screen's slot**,
/// which nothing has saved into: the cursor goes home, as `DECRC` with
/// nothing saved does. The primary's `ESC 7` is not the alternate screen's.
#[test]
fn the_alternate_screen_does_not_restore_the_primarys_save() {
    each_read_size(b"\x1b[5;5H\x1b7\x1b[?1049h\x1b[9;9H\x1b8", |t| {
        assert!(t.screen() == Screen::Alternate);
        assert_eq!(t.cursor(), at(0, 0));
    });
}

/// **`?1049 h` saves into the primary's `ESC 7` slot**, so `ESC 8` and
/// `?1048 l` on the primary find it after `?47 l` left without restoring,
/// and again after `?1049 l`, which restores without emptying it.
#[test]
fn the_1049_save_is_the_primarys_decsc() {
    each_read_size(b"\x1b[5;5H\x1b[?1049h\x1b[?47l\x1b[1;1H\x1b8", |t| {
        assert!(t.screen() == Screen::Normal);
        assert_eq!(t.cursor(), at(4, 4), "ESC 8 after ?47 l");
    });
    each_read_size(b"\x1b[5;5H\x1b[?1049h\x1b[?47l\x1b[1;1H\x1b[?1048l", |t| {
        assert_eq!(t.cursor(), at(4, 4), "?1048 l after ?47 l");
    });
    each_read_size(b"\x1b[5;5H\x1b[?1049h\x1b[?1049l\x1b[1;1H\x1b8", |t| {
        assert_eq!(t.cursor(), at(4, 4), "ESC 8 after ?1049 l");
    });
    // And an `ESC 7` before `?1049 h` is overwritten by it, as xterm's
    // `CursorSave` overwrites the one slot.
    each_read_size(b"\x1b[2;2H\x1b7\x1b[5;5H\x1b[?1049h\x1b[?1049l\x1b8", |t| {
        assert_eq!(t.cursor(), at(4, 4));
    });
}

/// **`?1049 h` on the alternate screen saves the alternate screen's
/// cursor**, xterm's `CursorSave` before a `ToAlternate` that does nothing
/// there; the primary's save stays, and `?1049 l` restores it.
#[test]
fn a_second_1049_set_saves_into_the_alternate_slot() {
    each_read_size(b"\x1b[?47h\x1b[3;3H\x1b[?1049h\x1b[9;9H\x1b8", |t| {
        assert!(t.screen() == Screen::Alternate);
        assert_eq!(t.cursor(), at(2, 2));
    });
    each_read_size(
        b"\x1b[5;5H\x1b[?1049h\x1b[3;3H\x1b[?1049h\x1b[?1049l",
        |t| {
            assert_eq!(t.cursor(), at(4, 4), "the primary's save is untouched");
        },
    );
}

/// **`?1049 l` with nothing saved restores as `DECRC` with nothing saved**:
/// home, origin mode off, the rendition normal. "Restore cursor as in
/// DECRC" (`[[source: ctlseqs]]`), and xterm's reset arm runs
/// `CursorRestore` on the primary too.
#[test]
fn leaving_with_nothing_saved_restores_as_decrc_does() {
    each_read_size(b"\x1b[9;20H\x1b[?1049l", |t| {
        assert_eq!(t.cursor(), at(0, 0));
    });
    let reset = |tail: &[u8]| {
        let mut owned = Owned::new(&support::CORPUS);
        let mut t = Term::new(owned.storage(), &support::CORPUS);
        support::feed(&mut t, b"\x1b[9;20H\x1b[31m");
        support::feed(&mut t, tail);
        support::feed(&mut t, b"X");
        t.row(Row::ZERO).expect("row 1")[0]
    };
    assert_eq!(reset(b"\x1b[?1049l"), reset(b"\x1b8"), "as DECRC unsaved");
}

/// **The alternate screen's slot outlives a visit**, as `sc[1]` does in
/// xterm: nothing empties it on the way in or out.
#[test]
fn the_alternate_slot_outlives_a_visit() {
    each_read_size(
        b"\x1b[?1049h\x1b[3;3H\x1b7\x1b[?1049l\x1b[?1049h\x1b[9;9H\x1b8",
        |t| assert_eq!(t.cursor(), at(2, 2)),
    );
}

/// **DECRQM `?1048` answers for the screen shown**: a save on the primary
/// is not one on the alternate screen.
#[test]
fn decrqm_1048_reads_the_shown_screens_slot() {
    assert_eq!(decrqm_1048(b"\x1b7"), b"\x1b[?1048;1$y");
    assert_eq!(decrqm_1048(b"\x1b7\x1b[?47h"), b"\x1b[?1048;2$y");
    assert_eq!(decrqm_1048(b"\x1b[?47h\x1b7\x1b[?47l"), b"\x1b[?1048;2$y");
    assert_eq!(decrqm_1048(b"\x1b[?1049h"), b"\x1b[?1048;2$y");
    assert_eq!(decrqm_1048(b"\x1b[?1049h\x1b[?47l"), b"\x1b[?1048;1$y");
}

/// **`DECSTR` empties the shown screen's slot and leaves the other's**, as
/// xterm's soft reset writes `sc[screen->whichBuf]` alone; **`RIS` empties
/// both**, the power-on state ("reset to its initial state", `[[source:
/// ecma48]]` § 8.3.105).
#[test]
fn resets_and_the_two_slots() {
    each_read_size(b"\x1b[5;5H\x1b[?1049h\x1b[!p\x1b[?1049l", |t| {
        assert_eq!(t.cursor(), at(4, 4), "DECSTR on the alternate screen");
    });
    each_read_size(
        b"\x1b[?47h\x1b[3;3H\x1b7\x1b[?47l\x1b[!p\x1b[?47h\x1b8",
        |t| {
            assert_eq!(t.cursor(), at(2, 2), "DECSTR on the primary");
        },
    );
    each_read_size(
        b"\x1b[?47h\x1b[3;3H\x1b7\x1b[?47l\x1bc\x1b[?47h\x1b[5;5H\x1b8",
        |t| {
            assert_eq!(t.cursor(), at(0, 0), "RIS empties the alternate slot");
        },
    );
}

/// The corpus terminal after `setup` fed at read sizes 1, 3, 7 and whole,
/// resized through `sizes` (columns, rows), and `ESC 8`: where the cursor
/// is, and the text of its row.
fn restored_after(setup: &[u8], sizes: &[(u16, u16)], then: &[u8]) -> Vec<((Row, Col), String)> {
    let mut out = Vec::new();
    for chunk in [1, 3, 7, setup.len().max(1)] {
        let mut owned = Owned::new(&support::CORPUS);
        let mut t = Term::new(owned.storage(), &support::CORPUS);
        for piece in setup.chunks(chunk) {
            support::feed(&mut t, piece);
        }
        for &(c, r) in sizes {
            let ws = baud_domain::WinSize {
                ws_col: c,
                ws_row: r,
            };
            assert_eq!(t.resize(&ws), baud_core::Resized::Ok);
        }
        support::feed(&mut t, then);
        support::feed(&mut t, b"\x1b8");
        let (y, x) = t.cursor();
        let text: String = t
            .row(y)
            .expect("a row on the grid")
            .iter()
            .map(|c| {
                c.content
                    .as_codepoint()
                    .and_then(|cp| char::from_u32(cp.value()))
                    .unwrap_or(' ')
            })
            .collect();
        out.push(((y, x), text.trim_end().to_string()));
    }
    out
}

/// Fifty numbered rows, `row 00` to `row 49`, the cursor left on the last.
fn numbered_rows() -> Vec<u8> {
    let mut b: Vec<u8> = (0..49)
        .flat_map(|i| format!("row {i:02}\r\n").into_bytes())
        .collect();
    b.extend_from_slice(b"row 49");
    b
}

/// A line of 150 characters on row 0, each column's character its own:
/// `0`-`9` in turn, with every tenth a letter, `a` at column 0.
fn long_line() -> Vec<u8> {
    (0..150u8)
        .map(|i| match i % 10 {
            0 => b'a' + i / 10,
            d => b'0' + d,
        })
        .collect()
}

/// **`ESC 7` follows its cell through a narrowing re-cut** (note V, V4):
/// saved on column 120 of a 150-character line, then the screen narrowed
/// from 200 columns to 100 while output runs, which re-cuts the line onto
/// two rows: `ESC 8` lands on row 1, column 20, the same character. It was
/// only clamped when restored, to column 99 of row 0, the character twenty
/// before it.
#[test]
fn decsc_follows_its_cell_through_a_narrowing_re_cut() {
    let mut setup = long_line();
    setup.extend_from_slice(b"\x1b[1;121H\x1b7\x1b[3;1H");
    for (at, text) in restored_after(&setup, &[(100, 50)], b"") {
        assert_eq!(at, (Row::from_index(1), Col::at_index(20)));
        assert_eq!(&text[20..21], "m", "column 120's character, {text:?}");
    }
}

/// **`ESC 7` follows its row through a shrink that retires rows, and back
/// through the growth that pulls them** (note V, V4): saved on `row 30`
/// with the cursor on the last row, so a shrink to 40 rows retires ten and
/// the row is row 20; the growth back to 50 pulls them and it is row 30
/// again. The live cursor's own arithmetic, applied to the saved row.
#[test]
fn decsc_follows_its_row_through_a_shrink_that_retires_rows() {
    let mut setup = numbered_rows();
    setup.extend_from_slice(b"\x1b[31;4H\x1b7\x1b[50;1H");
    for (at, text) in restored_after(&setup, &[(200, 40)], b"") {
        assert_eq!(at, (Row::from_index(20), Col::at_index(3)));
        assert_eq!(text, "row 30");
    }
    for (at, text) in restored_after(&setup, &[(200, 40), (200, 50)], b"") {
        assert_eq!(at, (Row::from_index(30), Col::at_index(3)));
        assert_eq!(text, "row 30");
    }
}

/// **A saved row the shrink retires lands on the top row**, the rule note V
/// proposes: `row 05` goes into the history with the ten rows above the
/// cursor's, and `ESC 8` comes back at the top in the column it saved.
#[test]
fn a_saved_row_the_shrink_retires_lands_on_the_top_row() {
    let mut setup = numbered_rows();
    setup.extend_from_slice(b"\x1b[6;4H\x1b7\x1b[50;1H");
    for (at, text) in restored_after(&setup, &[(200, 40)], b"") {
        assert_eq!(at, (Row::from_index(0), Col::at_index(3)));
        assert_eq!(text, "row 10");
    }
}

/// **A saved row the shrink cuts stays on its line**, and the growth that
/// shows it again finds it there; restored while it is hidden it is clamped
/// to the last row. The control for the cut half: the cursor on row 10, so
/// a shrink to 30 cuts rows 30 to 49 and retires none.
#[test]
fn a_saved_row_the_shrink_cuts_stays_on_its_line() {
    let mut setup = numbered_rows();
    setup.extend_from_slice(b"\x1b[41;4H\x1b7\x1b[11;1H");
    for (at, text) in restored_after(&setup, &[(200, 30), (200, 50)], b"") {
        assert_eq!(at, (Row::from_index(40), Col::at_index(3)));
        assert_eq!(text, "row 40");
    }
    for (at, _) in restored_after(&setup, &[(200, 30)], b"") {
        assert_eq!(at, (Row::from_index(29), Col::at_index(3)));
    }
}

/// **Under `?47` the primary's slot follows its cell too**: the alternate
/// screen is up and shows the one cursor both share, and the primary's
/// `ESC 7` is the other screen's slot, re-cut with the primary's lines.
#[test]
fn decsc_follows_its_cell_while_47_is_up() {
    let mut setup = long_line();
    setup.extend_from_slice(b"\x1b[1;121H\x1b7\x1b[3;1H\x1b[?47h");
    for (at, text) in restored_after(&setup, &[(100, 50)], b"\x1b[?47l") {
        assert_eq!(at, (Row::from_index(1), Col::at_index(20)));
        assert_eq!(&text[20..21], "m", "{text:?}");
    }
}

/// **A `?1049` exit after `?47` restores the primary's save re-cut**
/// (note V, V4, "a `?1049` save is not re-cut while `?47` is up"): the
/// save on column 120 of the long line, `?47` up, the screen narrowed to
/// 100 columns, and `?1049 l` leaves the alternate screen and restores the
/// primary's slot, which the re-cut moved with its cell to row 1, column
/// 20, with no `ESC 8`. Unmapped, the restore is clamped to row 0, column
/// 99.
#[test]
fn a_1049_exit_after_47_restores_the_primarys_save_re_cut() {
    let mut setup = long_line();
    setup.extend_from_slice(b"\x1b[1;121H\x1b7\x1b[3;1H\x1b[?47h\x1b[5;5H");
    for chunk in [1, 3, 7, setup.len()] {
        let mut owned = Owned::new(&support::CORPUS);
        let mut t = Term::new(owned.storage(), &support::CORPUS);
        for piece in setup.chunks(chunk) {
            support::feed(&mut t, piece);
        }
        let narrow = baud_domain::WinSize {
            ws_col: 100,
            ws_row: 50,
        };
        assert_eq!(t.resize(&narrow), baud_core::Resized::Ok);
        support::feed(&mut t, b"\x1b[?1049l");
        assert_eq!(t.screen(), Screen::Normal);
        assert_eq!(t.cursor(), at(1, 20), "read size {chunk}");
    }
}

/// **The alternate screen's slot is not moved**, the control: that screen
/// is cell-addressed, nothing re-cuts it and its rows do not retire on a
/// shrink, so `ESC 8` there comes back where `ESC 7` was taken.
#[test]
fn the_alternate_screens_slot_is_not_moved_by_a_resize() {
    let mut setup = numbered_rows();
    setup.extend_from_slice(b"\x1b[?1049h\x1b[3;6H\x1b7\x1b[20;1H");
    for (at, _) in restored_after(&setup, &[(100, 40), (200, 50)], b"") {
        assert_eq!(at, (Row::from_index(2), Col::at_index(5)));
    }
}
