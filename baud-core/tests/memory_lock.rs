//! **HP's memory lock and unlock, `ESC l` and `ESC m`, done as xterm does
//! them: a top margin at the cursor's line, and the first line again.**
//!
//! `[[source: ctlseqs]]`: "ESC l Memory Lock (per HP terminals). Locks
//! memory above the cursor" and "ESC m Memory Unlock (per HP terminals)".
//! xterm's `CASE_HP_MEM_LOCK` and `CASE_HP_MEM_UNLOCK` call
//! `set_tb_margins(screen, screen->cur_row, screen->bot_marg)` and
//! `set_tb_margins(screen, 0, screen->bot_marg)` (`[[source: xterm-reset]]`
//! 4759-4770), which moves the margins only `if (bottom > top)` (1335-1349)
//! and moves no cursor. xterm's way is built, over a lock kept apart from
//! the margins as the HP 2392A's manual describes it, because a program
//! sends them as the `meml` and `memu` of the `xterm-256color` entry
//! baud's `TERM` names, and that entry's terminal is xterm.
//!
//! Each case is held to a program that reaches the same state through
//! DECSTBM and CUP, compared on the whole screen, the cursor, the region
//! and the count of what was not acted on, at every read size and at every
//! split into two reads, so `ESC` and its final meet in every place a read
//! can put them.

mod support;

use baud_core::*;

const WS: WinSize = WinSize {
    ws_row: 8,
    ws_col: 10,
};

/// Everything a case compares.
type State = (Vec<PackedCell>, (Row, Col), (Row, Row), usize);

fn state_of(pieces: &[&[u8]]) -> State {
    let mut s = Owned::new(&WS);
    let mut t = Term::new(s.storage(), &WS);
    for piece in pieces {
        t.write_at(BytePos::ZERO, piece);
        t.feed(BytePos::ZERO, ReadLen::from_index(piece.len()));
    }
    (
        support::screen(&t),
        t.cursor(),
        t.scroll_region(),
        t.unsupported,
    )
}

/// `got` leaves what `want` fed whole leaves, at every read size and every
/// two-read split; every size and split is tried before failing.
fn same_as(got: &[u8], want: &[u8]) {
    let base = state_of(&[want]);
    assert_eq!(base.3, 0, "the control program counts nothing");
    let mut wrong = Vec::new();
    for read in 1..=got.len() {
        let pieces: Vec<&[u8]> = got.chunks(read).collect();
        if state_of(&pieces) != base {
            wrong.push(format!("{read}-byte reads"));
        }
    }
    for at in 1..got.len() {
        if state_of(&[&got[..at], &got[at..]]) != base {
            wrong.push(format!("split at {at}"));
        }
    }
    assert!(
        wrong.is_empty(),
        "{:?} differs from {:?} at {wrong:?}",
        String::from_utf8_lossy(got),
        String::from_utf8_lossy(want)
    );
}

/// **`ESC l` keeps the lines above the cursor while output scrolls beneath
/// them**: the region from the cursor's line to the bottom margin, as
/// DECSTBM would set it, and the cursor where it was.
#[test]
fn memory_lock_keeps_the_lines_above_the_cursor() {
    same_as(
        b"1\r\n2\r\n3\r\n4\x1b[3;2H\x1bl\x1b[8;1H\n\nZ",
        b"1\r\n2\r\n3\r\n4\x1b[3;8r\x1b[8;1H\n\nZ",
    );
    same_as(b"ab\x1b[3;2H\x1bl", b"ab\x1b[3;8r\x1b[3;2H");
}

/// **`ESC m` puts the top margin back on the first line and keeps the
/// bottom one**, the cursor where it was: xterm's `set_tb_margins(screen,
/// 0, screen->bot_marg)`, so a region `2;6` becomes `1;6`.
#[test]
fn memory_unlock_takes_the_top_margin_to_the_first_line() {
    same_as(
        b"1\r\n2\x1b[2;6r\x1b[4;3H\x1bm\x1b[6;1H\nZ",
        b"1\r\n2\x1b[1;6r\x1b[6;1H\nZ",
    );
    same_as(b"\x1b[2;6r\x1b[4;3H\x1bm", b"\x1b[1;6r\x1b[4;3H");
    same_as(b"\x1b[3;2H\x1bl\x1bm", b"\x1b[3;2H");
}

/// **A cursor on or below the bottom margin locks nothing**: xterm moves
/// the margins only `if (bottom > top)`, and a region needs two lines.
#[test]
fn a_cursor_on_or_below_the_bottom_margin_locks_nothing() {
    same_as(b"\x1b[2;6r\x1b[6;1H\x1bl", b"\x1b[2;6r\x1b[6;1H");
    same_as(b"\x1b[2;6r\x1b[7;4H\x1bl", b"\x1b[2;6r\x1b[7;4H");
    same_as(b"\x1b[8;1H\x1bl", b"\x1b[8;1H");
}

/// **Under DECOM the locked line is the origin**, as a DECSTBM top margin
/// is: line 2 of the region is the line below the lock.
#[test]
fn origin_mode_counts_from_the_locked_line() {
    same_as(
        b"\x1b[4;1H\x1bl\x1b[?6h\x1b[2;1HX",
        b"\x1b[4;8r\x1b[?6h\x1b[2;1HX",
    );
}

/// **IL and DL in the locked lines do nothing**, as above any top margin:
/// the lines are not inserted or deleted, and the cursor goes to the left
/// margin as IL and DL leave it.
#[test]
fn il_and_dl_in_the_locked_lines_do_nothing() {
    same_as(
        b"1\r\n2\r\n3\x1b[3;1H\x1bl\x1b[1;4H\x1b[L\x1b[2;4H\x1b[M",
        b"1\r\n2\r\n3\x1b[3;8r\x1b[2;4H",
    );
}

/// **DECSTBM, `DECSTR` and `RIS` set the margins anew**, so each ends the
/// lock, and what follows scrolls the whole screen.
#[test]
fn decstbm_decstr_and_ris_end_the_lock() {
    for end in [&b"\x1b[r"[..], b"\x1b[!p", b"\x1bc"] {
        let got = [&b"1\r\n2\x1b[3;1H\x1bl"[..], end, b"\x1b[8;1H\n\nZ"].concat();
        let want = [&b"1\r\n2\x1b[3;1H"[..], end, b"\x1b[8;1H\n\nZ"].concat();
        same_as(&got, &want);
    }
}

/// **On the status line, one row, neither moves a margin** -- the region
/// there has no second line -- and the main display's region is left as
/// it was (DECSSDT 2, DECSASD 1 and back).
#[test]
fn on_the_status_line_neither_moves_a_margin() {
    same_as(
        b"\x1b[2$~\x1b[3;1H\x1b[1$}\x1bl\x1bm\x1b[0$}\x1b[8;1H\nZ",
        b"\x1b[2$~\x1b[3;1H\x1b[1$}\x1b[0$}\x1b[8;1H\nZ",
    );
}
