//! **Under DECOM the origin is the top and left margins**, not the top margin
//! and column 1.
//!
//! * `[[source: dec-std-070]]` DECOM, p. 5-31: addressing is "relative to
//!   ... the origin of the current scrolling region (Top Margin, Left
//!   Margin) [set state = displaced]", and "the Active position cannot be
//!   moved above the Top Margin or left of the Left Margin". Its
//!   `set_origin_mode` puts the cursor at `top_margin` and `left_margin`,
//!   `reset_origin_mode` at line 1, column 1.
//! * CUP, pp. 5-49 and 5-50, note 1: "If Origin Mode is set, addressing is
//!   performed relative to the origin of the current scrolling region (the
//!   top and left margin)"; the algorithm's column is `(left_margin-1) + x`,
//!   and the right margin where that passes it. HVP is the same control here.
//! * DECSTBM note 1 (p. 5-25) and DECSLRM note 1 (p. 5-27), word for word:
//!   "Execution of this control causes the Active position to be set to the
//!   page origin obeying Origin Mode (DECOM): First column of first line if
//!   Origin Mode is in the reset (Absolute) state; Top and Left Margins if
//!   Origin Mode is in the set (Displaced) state."
//! * CPR, pp. 5-53 and 5-54, note 1 and its algorithm: under DECOM it reports
//!   `active_position.column - left_margin + 1`. DECXCPR (p. 5-55) says the
//!   same, and baud does not answer it (`?6n` is counted unsupported).
//!
//! 20 by 8. A test reads the cell a character lands in, not `Term::cursor`
//! alone, which clamps a cursor right of the fence to the right margin.

use baud_core::*;

const ROWS: usize = 8;
const COLS: usize = 20;
const WS: WinSize = WinSize {
    ws_row: 8,
    ws_col: 20,
};
/// Rows `abcdefgh`, each its letter to the last column, by `CUP`.
const LETTERS: &[u8; ROWS] = b"abcdefgh";

/// Top and bottom margins at rows 2..6 (1-based).
const TB: &str = "\x1b[2;6r";
/// `?69h`, then side margins at columns 5..10 (1-based).
const LRM: &str = "\x1b[?69h\x1b[5;10s";
/// DECOM set.
const OM: &str = "\x1b[?6h";

fn fill() -> String {
    let mut s = String::new();
    for (r, &letter) in LETTERS.iter().enumerate() {
        s.push_str(&format!("\x1b[{};1H", r + 1));
        s.extend(std::iter::repeat_n(char::from(letter), COLS));
    }
    s
}

/// The screen as strings (`.` blank), the cursor, and every reply, in order.
#[derive(Debug, PartialEq, Eq, Clone)]
pub(super) struct Seen {
    pub(super) screen: Vec<String>,
    pub(super) cursor: (Row, Col),
    pub(super) reply: Vec<u8>,
}

fn seen_at(bytes: &[u8], read: usize) -> Seen {
    let mut s = Owned::new(&WS);
    let mut t = Term::new(s.storage(), &WS);
    let mut reply = Vec::new();
    for piece in bytes.chunks(read.max(1)) {
        t.write_at(BytePos::ZERO, piece);
        t.feed(BytePos::ZERO, ReadLen::from_index(piece.len()));
        reply.extend_from_slice(t.reply());
        t.clear_reply();
    }
    let screen = (0..ROWS)
        .map(|y| {
            t.row(Row::from_index(y))[..COLS]
                .iter()
                .map(|c| {
                    c.content
                        .as_codepoint()
                        .map_or('.', |cp| char::from_u32(cp.value()).unwrap_or('?'))
                })
                .collect()
        })
        .collect();
    Seen {
        screen,
        cursor: t.cursor(),
        reply,
    }
}

/// The bytes whole, after requiring every read size to agree with them.
pub(super) fn seen(bytes: &str) -> Seen {
    let b = bytes.as_bytes();
    let whole = seen_at(b, b.len());
    for read in 1..b.len() {
        assert_eq!(seen_at(b, read), whole, "reads of {read} differ: {bytes:?}");
    }
    whole
}

/// Where `ch` is on the screen, 0-based, if it is there once.
pub(super) fn find(s: &Seen, ch: char) -> Option<(usize, usize)> {
    let hits: Vec<(usize, usize)> = s
        .screen
        .iter()
        .enumerate()
        .flat_map(|(y, row)| {
            row.chars()
                .enumerate()
                .filter(move |(_, c)| *c == ch)
                .map(move |(x, _)| (y, x))
        })
        .collect();
    match hits.as_slice() {
        [one] => Some(*one),
        _ => None,
    }
}

// ── home ────────────────────────────────────────────────────────────────────

/// `?6h` homes to (Top Margin, Left Margin); `?6l` to line 1, column 1.
#[test]
fn setting_origin_mode_homes_to_the_top_and_left_margins() {
    let s = seen(&format!("{TB}{LRM}{OM}X"));
    assert_eq!(find(&s, 'X'), Some((1, 4)));
    let s = seen(&format!("{TB}{LRM}{OM}\x1b[?6lY"));
    assert_eq!(find(&s, 'Y'), Some((0, 0)), "reset is absolute");
    // Without side margins the left margin is column 1, the control.
    let s = seen(&format!("{TB}{OM}Z"));
    assert_eq!(find(&s, 'Z'), Some((1, 0)));
}

/// `DECSLRM` homes "obeying Origin Mode": the new margins' corner under
/// DECOM, line 1 column 1 without it.
#[test]
fn decslrm_homes_to_the_page_origin() {
    let s = seen(&format!("{TB}{OM}{LRM}X"));
    assert_eq!(find(&s, 'X'), Some((1, 4)));
    let s = seen(&format!("{TB}\x1b[4;7H{LRM}Y"));
    assert_eq!(find(&s, 'Y'), Some((0, 0)), "absolute");
}

/// `DECSTBM` homes "obeying Origin Mode" too: **with DECOM reset that is the
/// first column of the first line**, not the region's top row, which this
/// core used until the V10 oracle found it.
#[test]
fn decstbm_homes_to_the_page_origin() {
    let s = seen(&format!("\x1b[4;7H{TB}X"));
    assert_eq!(find(&s, 'X'), Some((0, 0)), "absolute: line 1, column 1");
    let s = seen(&format!("{LRM}{OM}{TB}Y"));
    assert_eq!(find(&s, 'Y'), Some((1, 4)), "displaced: top and left");
    let s = seen(&format!("{OM}{TB}Z"));
    assert_eq!(find(&s, 'Z'), Some((1, 0)), "displaced, full width");
}

// ── addressing ──────────────────────────────────────────────────────────────

/// `CUP` and `HVP` count columns from the left margin under DECOM, and stop
/// at the right margin.
#[test]
fn cup_under_origin_mode_addresses_from_the_left_margin() {
    let s = seen(&format!("{TB}{LRM}{OM}\x1b[1;1HXY"));
    assert_eq!((find(&s, 'X'), find(&s, 'Y')), (Some((1, 4)), Some((1, 5))));
    let s = seen(&format!("{TB}{LRM}{OM}\x1b[3;2HZ"));
    assert_eq!(find(&s, 'Z'), Some((3, 5)));
    let s = seen(&format!("{TB}{LRM}{OM}\x1b[2;3fW"));
    assert_eq!(find(&s, 'W'), Some((2, 6)), "HVP");
    let s = seen(&format!("{TB}{LRM}{OM}\x1b[1;99HQ"));
    assert_eq!(find(&s, 'Q'), Some((1, 9)), "clamped to the right margin");
    let s = seen(&format!("{TB}{LRM}{OM}\x1b[99;99HR"));
    assert_eq!(find(&s, 'R'), Some((5, 9)), "the bottom right corner");
    // DECOM reset: absolute, whatever the margins.
    let s = seen(&format!("{TB}{LRM}\x1b[3;2HV"));
    assert_eq!(find(&s, 'V'), Some((2, 1)));
}

/// **`IL` and `DL` from a displaced `CUP` inside the fence move the
/// rectangle.** With the origin at column 1, `CUP 2;2` landed left of the
/// fence, where `IL` does nothing.
#[test]
fn il_and_dl_from_a_displaced_cup_move_the_rectangle() {
    let none = seen(&format!("{}{TB}{LRM}", fill())).screen;
    let il = seen(&format!("{}{TB}{LRM}{OM}\x1b[2;2H\x1b[L", fill()));
    assert_ne!(il.screen, none, "IL acted");
    assert_eq!(il.screen[1], "bbbbbbbbbbbbbbbbbbbb", "above the cursor row");
    assert_eq!(il.screen[2], "cccc......cccccccccc", "the cursor row opens");
    assert_eq!(il.screen[3], "ddddccccccdddddddddd");
    assert_eq!(il.screen[5], "ffffeeeeeeffffffffff");
    let dl = seen(&format!("{}{TB}{LRM}{OM}\x1b[2;2H\x1b[M", fill()));
    assert_eq!(dl.screen[2], "ccccddddddcccccccccc");
    assert_eq!(dl.screen[5], "ffff......ffffffffff");
}

// ── reports ─────────────────────────────────────────────────────────────────

/// **`CPR` reports the column from the left margin under DECOM**, as the row
/// from the top: `active_position.column - left_margin + 1`. The cursor is
/// put by `VPA` and `CHA`, whose columns do not move with the origin here, so
/// the report is the only thing that differs; `CUP` and `CPR` must agree
/// with each other as well, and do.
#[test]
fn cpr_under_origin_mode_counts_from_the_left_margin() {
    let s = seen(&format!("{TB}{LRM}{OM}\x1b[3d\x1b[8G\x1b[6n"));
    assert_eq!(s.cursor, (Row::from_index(3), Col::at_index(7)));
    assert_eq!(s.reply, b"\x1b[3;4R");
    let s = seen(&format!("{TB}{LRM}{OM}\x1b[2;3H\x1b[6n"));
    assert_eq!(s.reply, b"\x1b[2;3R", "CUP's address back");
    let s = seen(&format!("{TB}{LRM}\x1b[3;8H\x1b[6n"));
    assert_eq!(s.reply, b"\x1b[3;8R", "absolute");
}

/// DECXCPR stays unanswered: baud implements no DECXCPR, under DECOM or not.
#[test]
fn decxcpr_stays_unanswered_under_origin_mode() {
    let s = seen(&format!("{TB}{LRM}{OM}\x1b[?6n"));
    assert!(s.reply.is_empty(), "{:?}", s.reply);
}

// ── confinement ─────────────────────────────────────────────────────────────
//
// `[[source: dec-std-070]]` DECOM, p. 5-31: "When Origin Mode is in the set
// state (displaced), the Active position cannot be moved above the Top Margin
// or left of the Left Margin of the scrolling region"; `[[source:
// vt510-decom]]`: "The cursor cannot move outside of the margins." Every test
// below starts at `CUP 2;3` under DECOM -- line 2 column 6 of the screen,
// 0-based -- with the margins at lines 2..6 and columns 5..10, and asks `CPR`
// after the move, so the report and the cell the next character lands in
// must agree with each other at every read size.

/// `{TB}{LRM}{OM}`, `CUP 2;3`, then `ctl`, then `CPR` and an `X`.
fn after(ctl: &str) -> Seen {
    seen(&format!("{TB}{LRM}{OM}\x1b[2;3H{ctl}\x1b[6nX"))
}

/// The cell `X` landed in and the `CPR` before it.
fn landed(ctl: &str) -> (Option<(usize, usize)>, String) {
    let s = after(ctl);
    (
        find(&s, 'X'),
        String::from_utf8_lossy(&s.reply).into_owned(),
    )
}

/// **`CR` and every line start go to the left margin under DECOM**: `CR`
/// (STD 070 p. 5-58, "Move the Active Position to the Left Margin"), the same
/// `CR` spliced inside a control sequence, `NEL` (p. 5-64), `CNL`, `CPL`, and
/// `LF` under `LNM`, which NEL note 1 makes the same control.
#[test]
fn cr_and_the_line_starts_under_origin_mode_go_to_the_left_margin() {
    assert_eq!(landed("\r"), (Some((2, 4)), "\x1b[2;1R".into()), "CR");
    let s = seen(&format!("{TB}{LRM}{OM}\x1b[2;3H\x1b[\r6nX"));
    assert_eq!(
        (find(&s, 'X'), s.reply.as_slice()),
        (Some((2, 4)), &b"\x1b[2;1R"[..]),
        "CR inside a CSI"
    );
    assert_eq!(landed("\x1bE"), (Some((3, 4)), "\x1b[3;1R".into()), "NEL");
    assert_eq!(landed("\x1b[E"), (Some((3, 4)), "\x1b[3;1R".into()), "CNL");
    assert_eq!(landed("\x1b[F"), (Some((1, 4)), "\x1b[1;1R".into()), "CPL");
    // **A control went past**: a regional indicator after the `CR` does not
    // pair with the one before it, at the margin as at column 1 without
    // DECOM, where the second flag half takes cells of its own.
    let s = seen(&format!(
        "{TB}{LRM}{OM}\x1b[2;3H\u{1F1FA}\r\u{1F1F8}X\x1b[6n"
    ));
    assert_eq!(
        (
            s.screen[2].chars().nth(4),
            find(&s, 'X'),
            s.reply.as_slice()
        ),
        (Some('\u{1F1F8}'), Some((2, 6)), &b"\x1b[2;4R"[..]),
        "a flag half after CR"
    );
    let s = seen("\x1b[2;3H\u{1F1FA}\r\u{1F1F8}X\x1b[6n");
    assert_eq!(
        (
            s.screen[1].chars().next(),
            find(&s, 'X'),
            s.reply.as_slice()
        ),
        (Some('\u{1F1F8}'), Some((1, 2)), &b"\x1b[2;4R"[..]),
        "the same at column 1, the control"
    );
    assert_eq!(
        landed("\x1b[20h\n"),
        (Some((3, 4)), "\x1b[3;1R".into()),
        "LF under LNM"
    );
}

/// **`CUB` stops at the left margin**, STD 070 CUB note 1 (p. 5-47): from
/// inside, "the Active position will not move beyond the Left Margin".
#[test]
fn cub_under_origin_mode_stops_at_the_left_margin() {
    assert_eq!(landed("\x1b[5D"), (Some((2, 4)), "\x1b[2;1R".into()));
    assert_eq!(landed("\x1b[2D"), (Some((2, 4)), "\x1b[2;1R".into()));
    assert_eq!(landed("\x1b[D"), (Some((2, 5)), "\x1b[2;2R".into()));
}

/// **`CUF` and `HPR` stop at the right margin**, STD 070 CUF note 1
/// (p. 5-45).
#[test]
fn cuf_and_hpr_under_origin_mode_stop_at_the_right_margin() {
    assert_eq!(
        landed("\x1b[99C"),
        (Some((2, 9)), "\x1b[2;6R".into()),
        "CUF"
    );
    assert_eq!(
        landed("\x1b[99a"),
        (Some((2, 9)), "\x1b[2;6R".into()),
        "HPR"
    );
    assert_eq!(landed("\x1b[2a"), (Some((2, 8)), "\x1b[2;5R".into()), "HPR");
}

/// **`CHA` and `HPA` stay inside the margins under DECOM**, whichever way
/// their column is counted: column 1 is the left margin and column 99 the
/// right, under the built option and under xterm's alike.
#[test]
fn cha_and_hpa_under_origin_mode_stay_inside_the_margins() {
    assert_eq!(
        landed("\x1b[1G"),
        (Some((2, 4)), "\x1b[2;1R".into()),
        "CHA 1"
    );
    assert_eq!(
        landed("\x1b[99G"),
        (Some((2, 9)), "\x1b[2;6R".into()),
        "CHA 99"
    );
    assert_eq!(
        landed("\x1b[1`"),
        (Some((2, 4)), "\x1b[2;1R".into()),
        "HPA 1"
    );
    assert_eq!(
        landed("\x1b[99`"),
        (Some((2, 9)), "\x1b[2;6R".into()),
        "HPA 99"
    );
}

/// **The built option, (a'): `CHA` and `HPA` count from the screen's
/// column 1 and are clamped into the margins** -- ECMA-48's letter; xterm
/// counts from the left margin (`[[source: xterm-cursor-c]]` `CursorSet`),
/// which would put `CHA 3` on column 7 and `CHA 8` on the right margin. No
/// text settles which, listed for Jev (the record, "DECOM's origin is the
/// left margin too").
#[test]
fn cha_and_hpa_under_origin_mode_count_from_the_screen_option_a_prime() {
    assert_eq!(
        landed("\x1b[3G"),
        (Some((2, 4)), "\x1b[2;1R".into()),
        "CHA 3"
    );
    assert_eq!(
        landed("\x1b[8G"),
        (Some((2, 7)), "\x1b[2;4R".into()),
        "CHA 8"
    );
    assert_eq!(
        landed("\x1b[3`"),
        (Some((2, 4)), "\x1b[2;1R".into()),
        "HPA 3"
    );
    assert_eq!(
        landed("\x1b[8`"),
        (Some((2, 7)), "\x1b[2;4R".into()),
        "HPA 8"
    );
}

/// **`HT`, `CHT` and `CBT` stay inside the margins**: HT note 1 (STD 070
/// p. 5-67) stops at the right margin when no stop comes first. The stops
/// are at columns 9 and 17, 0-based 8 and 16.
#[test]
fn ht_cht_and_cbt_under_origin_mode_stay_inside_the_margins() {
    assert_eq!(landed("\t"), (Some((2, 8)), "\x1b[2;5R".into()), "HT");
    assert_eq!(landed("\t\t"), (Some((2, 9)), "\x1b[2;6R".into()), "HT, HT");
    assert_eq!(
        landed("\x1b[3I"),
        (Some((2, 9)), "\x1b[2;6R".into()),
        "CHT 3"
    );
    assert_eq!(landed("\x1b[Z"), (Some((2, 4)), "\x1b[2;1R".into()), "CBT");
    let s = seen(&format!("{TB}{LRM}{OM}\x1b[2;6H\t\x1b[6nX"));
    assert_eq!(
        (find(&s, 'X'), s.reply.as_slice()),
        (Some((2, 9)), &b"\x1b[2;6R"[..]),
        "HT on the right margin"
    );
}

/// **`DECRC` lands inside the margins when the restored mode is displaced**:
/// STD 070's `restore_cursor` (p. 5-190) clamps the line to the top and
/// bottom margins and the column to the left and right. Its note 3 (p. 5-189), written
/// before side margins, says the column "will remain as the stored column
/// value"; the algorithm and DECOM's "cannot be moved ... left of the Left
/// Margin" both clamp it. `?1049 l` restores through the same path.
#[test]
fn decrc_under_origin_mode_lands_inside_the_margins() {
    let col = landed("\x1b[1G\x1b7\x1b[8;12s\x1b8");
    assert_eq!(col, (Some((2, 7)), "\x1b[2;1R".into()), "the column");
    let row = landed("\x1b[1d\x1b7\x1b[4;6r\x1b8");
    assert_eq!(row, (Some((3, 6)), "\x1b[1;3R".into()), "the line");
    let alt = landed("\x1b[1G\x1b[?1049h\x1b[8;12s\x1b[?1049l");
    assert_eq!(alt, (Some((2, 7)), "\x1b[2;1R".into()), "?1049 l");
    // The mode restored decides, not the mode in force: saved displaced and
    // restored over `?6l`, then saved absolute and restored over `?6h`.
    let set = landed("\x1b[1G\x1b7\x1b[?6l\x1b[8;12s\x1b8");
    assert_eq!(
        set,
        (Some((2, 7)), "\x1b[2;1R".into()),
        "restored displaced"
    );
    let s = seen(&format!("{TB}{LRM}\x1b[3;3H\x1b7\x1b[?6h\x1b8\x1b[6nX"));
    assert_eq!(
        (find(&s, 'X'), s.reply.as_slice()),
        (Some((2, 2)), &b"\x1b[3;3R"[..]),
        "restored absolute"
    );
    // A pending wrap on the right margin comes back pending under side
    // margins narrower than the screen: the next character wraps to the left
    // margin of the next line. Saved and restored with the margins at full
    // width, the restore's clamp to the screen drops it, as before this
    // commit, and saved at full width and restored after DECSLRM 1;19 it
    // comes back pending; which wrap state DECRC restores is note AE's Q12.
    let s = seen(&format!("{TB}{LRM}{OM}\x1b[2;6HA\x1b7\x1b8B\x1b[6n"));
    assert_eq!(
        (find(&s, 'A'), find(&s, 'B'), s.reply.as_slice()),
        (Some((2, 9)), Some((3, 4)), &b"\x1b[3;2R"[..]),
        "a pending wrap"
    );
    // DECOM reset when saved: the restore is absolute, left of the fence.
    let s = seen(&format!("{TB}{LRM}\x1b[2;3H\x1b7\x1b[8;12s\x1b8\x1b[6nX"));
    assert_eq!(
        (find(&s, 'X'), s.reply.as_slice()),
        (Some((1, 2)), &b"\x1b[2;3R"[..]),
        "absolute"
    );
}

/// **A column saved on the right margin and restored under a narrower one is
/// clamped, not read as a pending wrap.** `CUP 2;6` puts the cursor on the
/// right margin, column 10, with nothing pending; after `DECSLRM 3;9` the
/// saved column is the new right margin + 1, where the pending-wrap sentinel
/// sits. STD 070's `restore_cursor` (p. 5-190) clamps it to the right margin,
/// column 9, and its save buffer holds no wrap item, so no reading restores a
/// wrap that was not pending; xterm's `CursorRestore` clamps it through
/// `CursorSet` and sets `do_wrap` from the saved flag, false here
/// (`[[source: xterm-cursor-c]]` 79-84 and 484-494). `SCORC` restores the
/// `DECSC` save -- under `?69h` `CSI s` is `DECSLRM`, so the save is `ESC 7`
/// -- and `?1049 l` its own.
#[test]
fn a_right_margin_column_restored_under_a_narrower_margin_is_clamped() {
    for (name, ctl) in [
        ("DECRC", "\x1b[2;6H\x1b7\x1b[3;9s\x1b8"),
        ("SCORC", "\x1b[2;6H\x1b7\x1b[3;9s\x1b[u"),
        ("?1049 l", "\x1b[2;6H\x1b[?1049h\x1b[3;9s\x1b[?1049l"),
    ] {
        assert_eq!(landed(ctl), (Some((2, 8)), "\x1b[2;7R".into()), "{name}");
    }
}

/// **A home under DECOM is the origin**, wherever a control homes the cursor
/// with the mode left set: `DECALN` and `DECCOLM` (whose own margin and mode
/// resets are on the record's open list). `?1049 h` was the third until the
/// owner's decision of 2026-10-13; it leaves the cursor, the next test.
#[test]
fn the_homes_under_origin_mode_go_to_the_origin() {
    let s = after("\x1b#8");
    assert_eq!(
        (find(&s, 'X'), s.reply.as_slice()),
        (Some((0, 4)), &b"\x1b[1;1R"[..]),
        "DECALN"
    );
    let s = after("\x1b[?3h");
    assert_eq!(
        (find(&s, 'X'), s.reply.as_slice()),
        (Some((0, 4)), &b"\x1b[1;1R"[..]),
        "DECCOLM"
    );
}

/// **`?1049 h` under DECOM leaves the cursor where it was**, inside the
/// margins, and `CPR` answers the same origin-relative 2;3 before and after:
/// `[[source: ctlseqs]]`'s `1049` saves, switches and clears and names no
/// move, and xterm's `ClearScreen` does not home (`xterm-util.c`). Before the
/// owner's decision of 2026-10-13 this homed to the origin, 1;1.
#[test]
fn entering_the_alternate_screen_under_origin_mode_leaves_the_cursor() {
    assert_eq!(
        landed("\x1b[?1049h"),
        (Some((2, 6)), "\x1b[2;3R".into()),
        "?1049 h"
    );
}

/// **VT52's moves keep DECOM's confinement.** STD 070's VT52 emulation
/// leaves Origin Mode and the scrolling region "undefined ... both when
/// entering and exiting VT52 Mode" (§ A.5.5), so no text says otherwise;
/// the address is absolute and clamped, as `CHA` is.
#[test]
fn vt52_moves_under_origin_mode_stay_inside_the_margins() {
    let vt52 = |ctl: &str| landed(&format!("\x1b[?2l{ctl}\x1b<"));
    assert_eq!(vt52("\x1bH"), (Some((1, 4)), "\x1b[1;1R".into()), "home");
    assert_eq!(
        vt52("\x1bY  "),
        (Some((1, 4)), "\x1b[1;1R".into()),
        "ESC Y 1;1"
    );
    assert_eq!(
        vt52("\x1bY#~"),
        (Some((3, 9)), "\x1b[3;6R".into()),
        "ESC Y 4;95"
    );
    assert_eq!(
        vt52("\x1bD\x1bD\x1bD"),
        (Some((2, 4)), "\x1b[2;1R".into()),
        "left"
    );
    assert_eq!(
        vt52("\x1bC\x1bC\x1bC\x1bC"),
        (Some((2, 9)), "\x1b[2;6R".into()),
        "right"
    );
}
