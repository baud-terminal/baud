//! `?41`, xterm's more(1) fix: a tab at a pending wrap takes the wrap
//! first.
//!
//! `[[source: ctlseqs]]`: DECSET `?41` "more(1) fix (see curses resource)",
//! DECRST "No more(1) fix". `[[source: xterm-man]]`, `-cu`: "work around a
//! bug in the more(1) program that causes it to incorrectly display lines
//! that are exactly the width of the window and are followed by a line
//! beginning with a tab (the leading tabs are not displayed)"; the curses
//! resource's default is false. xterm's `TabNext` (`[[source: xterm-tabs-c]]`
//! 113-118): with it set, a wrap pending and wraparound on, `xtermIndex`,
//! column 0 and the wrap reset, then the next stop. Every test here but the
//! controls fails on a `?41` that does nothing, which is what it was:
//! counted and dropped.
mod support;
use baud_core::{Owned, Term, WinSize, WrapPending};
use baud_domain::{Col, Row, Wrap};

const WS: WinSize = WinSize {
    ws_row: 8,
    ws_col: 20,
};

fn full() -> String {
    "x".repeat(20)
}

/// The cursor, whether a wrap is pending, and the dropped-parameter count
/// after `bytes`, fed at read sizes 1, 3 and whole, which must agree.
fn after(bytes: &str) -> ((usize, usize), WrapPending, usize) {
    let mut seen = None;
    for chunk in [1, 3, bytes.len().max(1)] {
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        for piece in bytes.as_bytes().chunks(chunk) {
            support::feed(&mut t, piece);
        }
        let (r, c) = t.cursor();
        let got = (
            (r.index(), c.index()),
            t.wrap_pending(),
            t.unsupported_param,
        );
        assert_eq!(*seen.get_or_insert(got), got, "read size {chunk}");
    }
    seen.expect("one read size")
}

/// `DECRQM`'s answer for `?41` after `bytes`.
fn decrqm(bytes: &[u8]) -> Vec<u8> {
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    support::feed(&mut t, bytes);
    t.clear_reply();
    support::feed(&mut t, b"\x1b[?41$p");
    t.reply().to_vec()
}

/// The control: **reset, a tab at a pending wrap stays on the last
/// column**, the wrap gone, as it always did.
#[test]
fn reset_a_tab_at_a_pending_wrap_stays_on_the_last_column() {
    let f = full();
    assert_eq!(
        after(&format!("\x1b[3;1H{f}\t")),
        ((2, 19), WrapPending::No, 0)
    );
    assert_eq!(
        after(&format!("\x1b[?41l\x1b[3;1H{f}\t")),
        ((2, 19), WrapPending::No, 0)
    );
}

/// **Set, the tab wraps first and then tabs**: from the line below's
/// first column to its first stop, column 9.
#[test]
fn set_a_tab_at_a_pending_wrap_wraps_and_then_tabs() {
    let f = full();
    assert_eq!(
        after(&format!("\x1b[?41h\x1b[3;1H{f}\t")),
        ((3, 8), WrapPending::No, 0)
    );
    assert_eq!(
        after(&format!("\x1b[?41h\x1b[3;1H{f}\x1b[2I")),
        ((3, 16), WrapPending::No, 0),
        "CHT takes the wrap once, then its two stops"
    );
}

/// **Only a pending wrap**: the controls, a tab from inside the line and
/// one from the last column with nothing pending, move as without the mode.
#[test]
fn set_a_tab_with_no_wrap_pending_moves_as_always() {
    assert_eq!(after("\x1b[?41h\x1b[3;3H\t"), ((2, 8), WrapPending::No, 0));
    assert_eq!(
        after("\x1b[?41h\x1b[3;20H\t"),
        ((2, 19), WrapPending::No, 0)
    );
}

/// **Only with wraparound on**, xterm's `WRAPAROUND` test: with `?7` reset
/// nothing wraps, nor when it is reset after the line filled, the wrap
/// still pending behind it.
#[test]
fn without_autowrap_the_tab_does_not_wrap() {
    let f = full();
    assert_eq!(
        after(&format!("\x1b[?41h\x1b[?7l\x1b[3;1H{f}\t")),
        ((2, 19), WrapPending::No, 0)
    );
    assert_eq!(
        after(&format!("\x1b[?41h\x1b[3;1H{f}\x1b[?7l\t")),
        ((2, 19), WrapPending::No, 0)
    );
}

/// **At the bottom margin the wrap scrolls**, as `IND` does: the row that
/// was filled moves up and the tab lands on the bottom line.
#[test]
fn at_the_bottom_the_wrap_scrolls() {
    let f = full();
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    support::feed(&mut t, format!("\x1b[?41h\x1b[8;1H{f}\tY").as_bytes());
    let cp = |y: usize, x: usize| {
        t.row(Row::from_index(y)).expect("a row")[Col::at_index(x).index()]
            .content
            .as_codepoint()
            .map(|c| c.value())
    };
    assert_eq!(cp(6, 0), Some(u32::from('x')), "the filled row moved up");
    assert_eq!(cp(7, 8), Some(u32::from('Y')));
    assert_eq!(cp(7, 0), None, "nothing written before the tab's stop");
}

/// **The row left is not marked as continuing**, as xterm's `TabNext` does
/// not mark it where its `WrapLine` does: more(1)'s next line is a line of
/// its own. The control is a character's wrap, which marks it.
#[test]
fn the_row_left_is_not_marked_as_continuing() {
    let f = full();
    let wrapped = |tail: &str| {
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        support::feed(&mut t, format!("\x1b[?41h\x1b[3;1H{f}{tail}").as_bytes());
        t.viewport().wrapped(Row::from_index(2))
    };
    assert_eq!(wrapped("\tY"), Wrap::Ended, "a tab's wrap");
    assert_eq!(wrapped("Y"), Wrap::Continues, "a character's wrap");
}

/// **Under `DECLRMM` the tab goes from the screen's first column**, xterm's
/// `set_cur_col(screen, 0)`, not the left margin, then to the next stop,
/// which the right margin bounds.
#[test]
fn under_side_margins_the_tab_starts_from_the_first_column() {
    let fill = "x".repeat(6);
    assert_eq!(
        after(&format!("\x1b[?41h\x1b[?69h\x1b[5;10s\x1b[3;5H{fill}\t")),
        ((3, 8), WrapPending::No, 0)
    );
}

/// **`DECRQM` answers the mode**, reset at power-on.
#[test]
fn decrqm_answers_the_more_fix() {
    assert_eq!(decrqm(b""), b"\x1b[?41;2$y");
    assert_eq!(decrqm(b"\x1b[?41h"), b"\x1b[?41;1$y");
    assert_eq!(decrqm(b"\x1b[?41h\x1b[?41l"), b"\x1b[?41;2$y");
}

/// **XTSAVE and XTRESTORE keep it; `RIS` resets it and `DECSTR` keeps it**,
/// as the xterm resource modes go.
#[test]
fn xtsave_keeps_it_ris_resets_it_and_decstr_keeps_it() {
    assert_eq!(
        decrqm(b"\x1b[?41h\x1b[?41s\x1b[?41l\x1b[?41r"),
        b"\x1b[?41;1$y"
    );
    assert_eq!(decrqm(b"\x1b[?41h\x1bc"), b"\x1b[?41;2$y");
    assert_eq!(decrqm(b"\x1b[?41h\x1b[!p"), b"\x1b[?41;1$y");
}
