//! `?47`, the alternate screen before `?1049`.
//!
//! xterm's `srm_ALTBUF` is `ToAlternate(xw, False)` in and
//! `FromAlternate(xw, False)` out (the pinned `xterm-charproc.c`): it switches
//! buffers and does nothing else -- no cursor save or restore, and no clear, so
//! the alternate screen shows what it held when it was last left, and
//! `SwitchBufs` leaves the cursor where it was. `DECRQM` reports it from
//! **`DECRQM` answers from which buffer is shown, for `?47` as for `?1049`.**
//! That is baud's choice: the pinned `xterm-charproc.c` records both modes
//! from `whichBuf` in `savemodes`, which is XTSAVE, and xterm's `DECRQM` code
//! is not pinned.
//!
//! `docs/vt-coverage.md` called it done while `baud-core` routed it nowhere,
//! and the one test written for it passed on a no-op: `?47 h` ignored,
//! `CUP 1;1`, `?47 l` ignored, and the cursor at home was what it expected.
//! **Every test here fails on a `?47` that does nothing**, each through
//! something that differs between the two screens -- text on one and not the
//! other, or which screen is up -- rather than through the cursor alone. Four
//! passed on the tree before this change in their first version, because a
//! no-op gave the same final state; each now checks the screen as well. The
//! tests that feed through `each_read_size` do so at read sizes 1, 3, 7 and
//! whole; the `DECRQM` test feeds once.
mod support;
use baud_core::{Owned, Term};
use baud_domain::{Col, Row};

/// The first eight cells of screen row `y`, as text.
fn text(t: &Term<'_>, y: usize) -> String {
    t.row(Row::from_index(y))[..8]
        .iter()
        .map(|c| {
            c.content
                .as_codepoint()
                .map_or(' ', |cp| char::from_u32(cp.value()).unwrap_or('?'))
        })
        .collect()
}

/// Feed `bytes` at every read size from one byte up, and hand each terminal
/// to `f`: a mode switch split across two reads must land where one read puts
/// it.
fn each_read_size(bytes: &[u8], f: impl Fn(&Term<'_>)) {
    for chunk in [1, 3, 7, bytes.len().max(1)] {
        let mut owned = Owned::new(&support::CORPUS);
        let mut t = Term::new(owned.storage(), &support::CORPUS);
        for piece in bytes.chunks(chunk) {
            support::feed(&mut t, piece);
        }
        f(&t);
    }
}

#[test]
fn a_47_round_trip_leaves_the_primary_as_it_was() {
    each_read_size(b"\x1b[1;1Hprimary\x1b[?47h\x1b[1;1Hsecond\x1b[?47l", |t| {
        assert_eq!(
            text(t, 0),
            "primary ",
            "the alternate screen's text went on the alternate screen"
        );
    });
}

#[test]
fn the_alternate_screen_keeps_its_text_between_47_visits() {
    each_read_size(
        b"\x1b[1;1Hprimary\x1b[?47h\x1b[1;1Hsecond\x1b[?47l\x1b[?47h",
        |t| {
            assert_eq!(text(t, 0), "second  ", "shown again, not cleared");
            assert!(
                t.screen() == baud_core::Screen::Alternate,
                "the control: the alternate screen is up"
            );
        },
    );
}

/// `?1049 l` does not clear the alternate screen either, so `?47 h` after it
/// shows what the full-screen program left.
#[test]
fn entering_47_shows_what_the_alternate_screen_was_left_holding() {
    each_read_size(b"\x1b[?1049h\x1b[3;3Hleft\x1b[?1049l", |t| {
        assert_eq!(text(t, 2), "        ", "the control: the primary is blank");
    });
    each_read_size(b"\x1b[?1049h\x1b[3;3Hleft\x1b[?1049l\x1b[?47h", |t| {
        assert_eq!(text(t, 2), "  left  ");
    });
}

/// The rows come back in the order the alternate screen showed them. After
/// sixty lines on fifty rows its rotation is not zero, and resetting it on a
/// `?47` entry -- which `?1049` does, because it clears -- would put them out
/// of order.
#[test]
fn entering_47_keeps_a_scrolled_alternate_screens_rows_in_order() {
    let mut bytes = b"\x1b[?1049h".to_vec();
    for i in 0..60 {
        bytes.extend_from_slice(format!("line {i:03}\r\n").as_bytes());
    }
    bytes.extend_from_slice(b"\x1b[?1049l\x1b[?47h");
    each_read_size(&bytes, |t| {
        assert_eq!(text(t, 0), "line 011");
        assert_eq!(text(t, 48), "line 059");
    });
}

#[test]
fn _47_neither_saves_nor_restores_the_cursor() {
    each_read_size(b"\x1b[9;20H\x1b[?47h", |t| {
        assert!(
            t.screen() == baud_core::Screen::Alternate,
            "the control: the alternate screen is up"
        );
        assert_eq!(
            t.cursor(),
            (Row::from_index(8), Col::at_index(19)),
            "entry does not home the cursor"
        );
    });
    // Text written on the alternate screen, then the exit: the text is not on
    // the primary (so the screen switched), and the cursor is where the
    // writing left it (so nothing restored the 9;20 it was at on entry).
    each_read_size(b"\x1b[9;20H\x1b[?47h\x1b[5;7Halt\x1b[?47l", |t| {
        assert!(t.screen() == baud_core::Screen::Normal);
        assert_eq!(text(t, 4), "        ", "the alternate text stayed there");
        assert_eq!(
            t.cursor(),
            (Row::from_index(4), Col::at_index(9)),
            "exit does not restore it: it is where the alternate screen left it"
        );
    });
}

/// A cursor `?1049 h` saved outlives a `?47 l` exit, as xterm's saved cursor
/// does, so a `?1049 l` issued from the alternate screen afterwards still
/// restores it. (One issued from the primary does nothing here, where xterm
/// restores; see CLAUDE.md.)
#[test]
fn a_47_exit_leaves_what_1049_saved() {
    each_read_size(
        b"\x1b[9;20H\x1b[?1049h\x1b[?47l\x1b[3;3Hp\x1b[?47h\x1b[?1049l",
        |t| {
            assert_eq!(t.cursor(), (Row::from_index(8), Col::at_index(19)));
            assert_eq!(
                text(t, 2),
                "  p     ",
                "the `?47 l` put the primary up, so `p` landed on it"
            );
        },
    );
}

/// **And a reset drops that save.** `RIS` leaves the alternate screen first,
/// which on the primary does nothing, so a save a `?47 l` left pending
/// survived it: `?1049 l` after the reset restored the cursor and the
/// rendition from before it.
#[test]
fn a_reset_drops_a_save_a_47_exit_left_pending() {
    each_read_size(b"\x1b[5;5H\x1b[31m\x1b[?1049h\x1b[?47l", |t| {
        assert!(
            t.screen() == baud_core::Screen::Normal,
            "the control, which a no-op fails: the save is pending on the primary"
        );
    });
    each_read_size(
        b"\x1b[5;5H\x1b[31m\x1b[?1049h\x1b[?47l\x1bc\x1b[?47h\x1b[?1049lx",
        |t| {
            assert_eq!(
                t.cursor(),
                (Row::ZERO, Col::at_index(1)),
                "nothing restored the 5;5 from before the reset"
            );
            let mut plain = Owned::new(&support::CORPUS);
            let mut fresh = Term::new(plain.storage(), &support::CORPUS);
            support::feed(&mut fresh, b"x");
            assert_eq!(
                t.style_of(t.row(Row::ZERO)[0]),
                fresh.style_of(fresh.row(Row::ZERO)[0]),
                "and nothing restored the red"
            );
        },
    );
}

#[test]
fn decrqm_reports_47_as_the_buffer_shown() {
    let ask = |b: &[u8]| {
        let mut owned = Owned::new(&support::CORPUS);
        let mut t = Term::new(owned.storage(), &support::CORPUS);
        support::feed(&mut t, b);
        t.reply().to_vec()
    };
    assert_eq!(ask(b"\x1b[?47$p"), b"\x1b[?47;2$y", "reset at power-on");
    assert_eq!(ask(b"\x1b[?47h\x1b[?47$p"), b"\x1b[?47;1$y");
    assert_eq!(ask(b"\x1b[?47h\x1b[?47l\x1b[?47$p"), b"\x1b[?47;2$y");
    assert_eq!(
        ask(b"\x1b[?1049h\x1b[?47$p"),
        b"\x1b[?47;1$y",
        "either mode shows the alternate buffer"
    );
    assert_eq!(ask(b"\x1b[?47h\x1b[?1049$p"), b"\x1b[?1049;1$y");
}

/// **A reset leaves nothing for `?47` to show**, from either screen. xterm
/// clears the alternate buffer only when the reset comes from it; baud clears
/// it from both, because `RIS` frees the cluster and link tables on the
/// promise that no cell a reader can reach still names a slot, and the parked
/// alternate surface is a cell a reader can reach through `?47`.
#[test]
fn a_reset_leaves_nothing_on_the_alternate_screen() {
    for from in [&b"\x1b[?47l"[..], b""] {
        let mut bytes = b"\x1b[?47h\x1b[1;1Hsecret".to_vec();
        bytes.extend_from_slice(from);
        bytes.extend_from_slice(b"\x1bc\x1b[?47h");
        each_read_size(&bytes, |t| {
            assert!(t.screen() == baud_core::Screen::Alternate);
            assert_eq!(text(t, 0), "        ", "the old alternate text is gone");
        });
    }
}

/// **And the alternate surface names nothing the reset freed.** A cell with
/// a mark carries its cluster's slot in its flags, and `RIS` frees every slot
/// on the promise that no cell a reader can reach still names one; a stale
/// id would read whichever cluster with the same base takes that slot next.
/// So the cell must come back blank and unmarked, and the row its line
/// wrapped from must not still say it continues -- `RowExtent`'s wrap flag,
/// which `Viewport::wrapped` and a selection's copy read.
#[test]
fn a_reset_leaves_the_alternate_cells_naming_nothing() {
    let mut bytes = "\x1b[?47h\x1b[1;1He\u{301}".as_bytes().to_vec();
    bytes.extend_from_slice(&[b'w'; 250]);
    bytes.extend_from_slice(b"\x1b[?47l\x1bc\x1b[?47h");
    each_read_size(&bytes, |t| {
        assert!(t.screen() == baud_core::Screen::Alternate);
        let cell = t.row(Row::ZERO)[0];
        assert!(!cell.flags.is_marked(), "the cell names no cluster");
        assert_eq!(text(t, 0), "        ");
        assert!(
            t.view(baud_domain::Rows::ZERO).wrapped(Row::ZERO) == baud_domain::Wrap::Ended,
            "the row that wrapped before the reset does not still continue"
        );
    });
}

/// **A shrink while `?47`'s alternate screen is up anchors the primary on the
/// cursor it will have** -- the one both screens share, since `?47 l`
/// restores nothing. It read `alt_saved`, which `?47` never writes, and
/// anchored at row 0: the shrink cut the primary's bottom rows and retired
/// none. The same with `?1049`, which anchors on its save, is the control.
#[test]
fn a_shrink_under_47_anchors_the_primary_on_the_shared_cursor() {
    for (mode, name) in [(&b"47"[..], "?47"), (b"1049", "?1049")] {
        support::with_room(50, |t| {
            for i in 0..50 {
                support::feed(t, format!("line {i:03}").as_bytes());
                if i < 49 {
                    support::feed(t, b"\r\n");
                }
            }
            support::feed(t, &[b"\x1b[?".as_slice(), mode, b"h"].concat());
            assert!(
                t.screen() == baud_core::Screen::Alternate,
                "{name}: the control, which a no-op fails: the alternate screen is up"
            );
            let (cols, rows) = t.geometry();
            assert_eq!(
                t.resize(&baud_domain::WinSize {
                    ws_col: u16::try_from(cols.index()).unwrap(),
                    ws_row: u16::try_from(rows.index() - 6).unwrap(),
                }),
                baud_core::Resized::Ok
            );
            support::feed(t, &[b"\x1b[?".as_slice(), mode, b"l"].concat());
            assert_eq!(
                text(t, 0),
                "line 006",
                "{name}: the top six retired, the cursor's row kept"
            );
            assert_eq!(text(t, 43), "line 049", "{name}: the bottom stays");
        });
    }
}

/// **A width change under `?47` anchors the primary's re-cut on the shared
/// cursor too.** Forty lines of thirty characters, narrowed to fifteen
/// columns, are eighty rows: the window must slide down to keep the cursor's
/// line on the screen. Anchored at row 0 it stayed at the top.
#[test]
fn a_recut_under_47_anchors_the_primary_on_the_shared_cursor() {
    for (mode, name) in [(&b"47"[..], "?47"), (b"1049", "?1049")] {
        support::with_room(50, |t| {
            for i in 0..40 {
                support::feed(t, format!("{i:02}{}", "x".repeat(28)).as_bytes());
                if i < 39 {
                    support::feed(t, b"\r\n");
                }
            }
            support::feed(t, &[b"\x1b[?".as_slice(), mode, b"h"].concat());
            assert!(
                t.screen() == baud_core::Screen::Alternate,
                "{name}: the control, which a no-op fails: the alternate screen is up"
            );
            let (_, rows) = t.geometry();
            assert_eq!(
                t.resize(&baud_domain::WinSize {
                    ws_col: 15,
                    ws_row: u16::try_from(rows.index()).unwrap(),
                }),
                baud_core::Resized::Ok
            );
            support::feed(t, &[b"\x1b[?".as_slice(), mode, b"l"].concat());
            assert_eq!(
                &text(t, 0)[..2],
                "15",
                "{name}: the window slid to keep the cursor's line on screen"
            );
        });
    }
}

/// **A growth while the alternate screen is parked blanks the rows it will
/// reveal**, which only `?47` can show: `?1049` clears on entry. Sixty lines
/// on a fifty-row alternate screen, left, grown by six on the primary, and
/// shown again with `?47 h`.
#[test]
fn a_growth_while_parked_blanks_the_rows_47_shows() {
    support::with_room(60, |t| {
        support::feed(t, b"\x1b[?1049h");
        for i in 0..60 {
            support::feed(t, format!("line {i:03}\r\n").as_bytes());
        }
        support::feed(t, b"\x1b[?1049l");
        let (cols, rows) = t.geometry();
        assert_eq!(
            t.resize(&baud_domain::WinSize {
                ws_col: u16::try_from(cols.index()).unwrap(),
                ws_row: u16::try_from(rows.index() + 6).unwrap(),
            }),
            baud_core::Resized::Ok
        );
        support::feed(t, b"\x1b[?47h");
        assert_eq!(text(t, 0), "line 011", "the control: the parked rows show");
        for y in rows.index()..rows.index() + 6 {
            assert_eq!(
                text(t, y),
                "        ",
                "row {y} was revealed and must be blank"
            );
        }
    });
}

/// **`?1049 h` while the alternate screen is up clears it**, without saving
/// or homing: xterm's set arm clears whatever buffer is up, `ClearScreen`
/// leaves the cursor, and the save belongs to the primary. After `?47 h` a
/// program asking for `?1049` got the `?47` screen's leftovers; after a first
/// `?1049 h` a second one did nothing.
#[test]
fn _1049_while_the_alternate_screen_is_up_clears_it() {
    for enter in [&b"\x1b[?47h"[..], b"\x1b[9;20H\x1b[?1049h"] {
        let mut bytes = enter.to_vec();
        bytes.extend_from_slice(b"\x1b[3;3Hleft\x1b[5;5H\x1b[?1049h");
        each_read_size(&bytes, |t| {
            assert!(t.screen() == baud_core::Screen::Alternate);
            assert_eq!(text(t, 2), "        ", "the screen was cleared");
            assert_eq!(
                t.cursor(),
                (Row::from_index(4), Col::at_index(4)),
                "and the cursor was not homed"
            );
        });
    }
    // The first `?1049 h`'s save survives the second, so the exit restores it.
    each_read_size(
        b"\x1b[9;20H\x1b[?1049h\x1b[5;5H\x1b[?1049h\x1b[?1049l",
        |t| {
            assert_eq!(t.cursor(), (Row::from_index(8), Col::at_index(19)));
        },
    );
}

/// **`?1049 l` on the primary restores a save a `?47 l` left pending**, as
/// xterm's `CursorRestore` runs whether or not `FromAlternate` did anything.
#[test]
fn _1049_l_on_the_primary_restores_a_pending_save() {
    each_read_size(
        b"\x1b[9;20H\x1b[?1049h\x1b[?47l\x1b[3;3Hp\x1b[?1049l",
        |t| {
            assert!(t.screen() == baud_core::Screen::Normal);
            assert_eq!(t.cursor(), (Row::from_index(8), Col::at_index(19)));
            assert_eq!(
                text(t, 2),
                "  p     ",
                "the `?47 l` put the primary up, so the `?1049 l` ran on it"
            );
        },
    );
}
