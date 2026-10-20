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
