//! **The corner: a cursor a move placed right of the right margin.**
//!
//! * `[[source: dec-std-070]]` INSERT OR REPLACE GRAPHIC CHARACTER, p.
//!   5-139: "If the Active Position is within the Scroll Area, it will
//!   advance to the Right Margin. If the Active Position is outside the
//!   Scroll Area, it will advance to the last column of the Active Line",
//!   and "If Auto Wrap mode is off, the Active Position will not advance
//!   beyond the Right Margin or last column of the Active Line"; its
//!   algorithm (p. 5-140) takes the right margin as the boundary only when
//!   `active_position.column <= right_margin`, else `end_of_line`.
//! * The Last Column Flag (070-D, D.6.1, pp. D-13, D-14) is set by writing
//!   into the last column and reset by CUP, HVP, CUF, CUB, BS, HT and the
//!   rest, so a cursor a move puts one past the right margin is no pending
//!   wrap.
//! * xterm's `dotext` (`[[source: xterm-reset]]`, charproc.c 6991-6995,
//!   7055-7058): "It is possible to use CUP, etc., to move outside margins.
//!   In that case, the right-margin is ineffective until the text (may)
//!   wrap and get within the margins"; `right` is `last_col` while
//!   `cur_col > rmargin`, and `WrapLine` (6983) sets the column to the left
//!   margin.
//!
//! 20 by 8, every row its own letter, the fence at columns 5..10 (1-based;
//! 0-based 4..=9), so one past the right margin is column 11 (0-based 10).
//! Every row a case writes on is inside the vertical scroll area: outside
//! it STD 070 gives the line its last column as the boundary even inside
//! the fence, which is not this corner. Every case lands the same at every
//! read size and at every split into two reads.

use baud_core::*;

const ROWS: usize = 8;
const COLS: usize = 20;
const WS: WinSize = WinSize {
    ws_row: 8,
    ws_col: 20,
};
const LETTERS: &[u8; ROWS] = b"abcdefgh";

/// `?69h`, then side margins at columns 5..10 (1-based); 0-based 4..=9.
const LRM: &str = "\x1b[?69h\x1b[5;10s";
/// The margins put back, the cursor left where it is, and a mark there.
const PROBE: &str = "\x1b[?69lX";

fn fill() -> String {
    let mut s = String::new();
    for (r, &letter) in LETTERS.iter().enumerate() {
        s.push_str(&format!("\x1b[{};1H", r + 1));
        s.extend(std::iter::repeat_n(char::from(letter), COLS));
    }
    s
}

#[derive(Debug, PartialEq, Eq)]
struct Seen {
    /// Each row, a wide character's tail as `_`.
    screen: Vec<String>,
    cursor: (usize, usize),
    pending: WrapPending,
    reply: Vec<u8>,
}

fn seen_in(pieces: &[&[u8]]) -> Seen {
    let mut s = Owned::new(&WS);
    let mut t = Term::new(s.storage(), &WS);
    for piece in pieces.iter().filter(|p| !p.is_empty()) {
        t.write_at(BytePos::ZERO, piece);
        t.feed(BytePos::ZERO, ReadLen::from_index(piece.len()));
    }
    let screen = (0..ROWS)
        .map(|y| {
            t.row(Row::from_index(y)).expect("a row on the grid")[..COLS]
                .iter()
                .map(|c| match c.content.is_wide_tail() {
                    true => '_',
                    false => c
                        .content
                        .as_codepoint()
                        .map_or('.', |cp| char::from_u32(cp.value()).unwrap_or('?')),
                })
                .collect()
        })
        .collect();
    let (r, c) = t.cursor();
    Seen {
        screen,
        cursor: (r.index(), c.index()),
        pending: t.wrap_pending(),
        reply: t.reply().to_vec(),
    }
}

/// The fill, then `tail`, whole -- after requiring every read size and
/// every split into two reads to reach the same state.
fn seen(tail: &str) -> Seen {
    let all = format!("{}{tail}", fill());
    let b = all.as_bytes();
    let whole = seen_in(&[b]);
    for read in 1..b.len() {
        let pieces: Vec<&[u8]> = b.chunks(read).collect();
        assert_eq!(seen_in(&pieces), whole, "reads of {read} differ: {tail:?}");
    }
    for at in 1..b.len() {
        let (a, z) = b.split_at(at);
        assert_eq!(seen_in(&[a, z]), whole, "a split at {at} differs: {tail:?}");
    }
    whole
}

/// Row `y` of the fill with `text` written over it from column `x`.
fn over(y: usize, x: usize, text: &str) -> String {
    let mut row: Vec<char> = std::iter::repeat_n(char::from(LETTERS[y]), COLS).collect();
    for (i, ch) in text.chars().enumerate() {
        row[x + i] = ch;
    }
    row.into_iter().collect()
}

/// Where the probe's `X` lands after `tail`, 0-based.
fn x_at(tail: &str) -> (usize, usize) {
    let s = seen(&format!("{tail}{PROBE}"));
    let hits: Vec<(usize, usize)> = s
        .screen
        .iter()
        .enumerate()
        .flat_map(|(y, row)| {
            row.chars()
                .enumerate()
                .filter(|(_, c)| *c == 'X')
                .map(move |(x, _)| (y, x))
        })
        .collect();
    assert_eq!(hits.len(), 1, "one X: {:?}", s.screen);
    hits[0]
}

// ── a write right of the fence ──────────────────────────────────────────────

/// **Right of the fence the text writes on to the screen's edge**, by
/// every writer: one character, an ASCII run, a narrow non-ASCII run, and
/// a wide character -- and the cursor stays where the write left it, with
/// no wrap pending.
#[test]
fn a_cursor_right_of_the_fence_writes_on() {
    for (text, want) in [
        ("X", "X"),
        ("XYZ", "XYZ"),
        ("ééé", "ééé"),
        ("\u{4e2d}", "\u{4e2d}_"),
    ] {
        let s = seen(&format!("{LRM}\x1b[3;15H{text}"));
        let cells = want.chars().count();
        assert_eq!(s.screen[2], over(2, 14, want), "{text}");
        assert_eq!(s.screen[3], over(3, 0, ""), "{text}: nothing wrapped");
        assert_eq!(s.cursor, (2, 14 + cells), "{text}");
        assert_eq!(s.pending, WrapPending::No, "{text}");
    }
}

/// **A run longer than the room to the edge fills it, then wraps to the
/// left margin** and goes on inside the fence, as xterm's `WrapLine` does.
#[test]
fn a_run_right_of_the_fence_wraps_at_the_edge_to_the_left_margin() {
    for run in ["ABCDEFGHIJKLMNO", "ÀÁÂÃÄÅÆÇÈÉÊËÌÍÎ"] {
        let s = seen(&format!("{LRM}\x1b[3;15H{run}"));
        let c: Vec<char> = run.chars().collect();
        let piece = |a: usize, b: usize| c[a..b].iter().collect::<String>();
        assert_eq!(s.screen[2], over(2, 14, &piece(0, 6)), "{run}: to the edge");
        assert_eq!(s.screen[3], over(3, 4, &piece(6, 12)), "{run}: the fence");
        assert_eq!(s.screen[4], over(4, 4, &piece(12, 15)), "{run}: and on");
        assert_eq!(s.cursor, (4, 7), "{run}");
    }
}

/// **Filled to the edge, the wrap is pending there**, the cursor reported
/// on the last column, and the next character goes to the left margin.
#[test]
fn a_write_to_the_edge_leaves_a_wrap_pending_there() {
    let s = seen(&format!("{LRM}\x1b[3;15HABCDEF"));
    assert_eq!(
        (s.cursor, s.pending),
        ((2, 19), WrapPending::Yes),
        "pending at the edge"
    );
    let s = seen(&format!("{LRM}\x1b[3;15HABCDEFG"));
    assert_eq!(s.screen[3], over(3, 4, "G"));
    assert_eq!(s.cursor, (3, 5));
}

/// **A wide character with one column left before the edge wraps to the
/// left margin** (charproc.c 7130-7138 and 7160-7171: it does not fit,
/// so `force_wrap`), and one with two left writes there.
#[test]
fn a_wide_character_at_the_edge_wraps_to_the_left_margin() {
    let s = seen(&format!("{LRM}\x1b[3;20H\u{4e2d}"));
    assert_eq!(s.screen[2], over(2, 0, ""), "the last column untouched");
    assert_eq!(s.screen[3], over(3, 4, "\u{4e2d}_"));
    assert_eq!(s.cursor, (3, 6));
    let s = seen(&format!("{LRM}\x1b[3;19H\u{4e2d}"));
    assert_eq!(s.screen[2], over(2, 18, "\u{4e2d}_"));
    assert_eq!((s.cursor, s.pending), ((2, 19), WrapPending::Yes));
}

// ── one past the right margin: placed, or a pending wrap ───────────────────

/// **The pair that tells a placed cursor from a waiting one.** `CUP` to
/// one past the right margin writes there and on; a write that filled the
/// fence to its margin wraps first.
#[test]
fn one_past_the_margin_placed_writes_there() {
    let s = seen(&format!("{LRM}\x1b[3;11HAB"));
    assert_eq!(s.screen[2], over(2, 10, "AB"));
    assert_eq!(s.screen[3], over(3, 0, ""));
    assert_eq!(s.cursor, (2, 12));
}

/// The control: **filled to the right margin, the next character wraps
/// to the left margin** (D.6.1).
#[test]
fn one_past_the_margin_written_wraps_first() {
    let s = seen(&format!("{LRM}\x1b[3;5HABCDEF"));
    assert_eq!((s.cursor, s.pending), ((2, 9), WrapPending::Yes));
    let s = seen(&format!("{LRM}\x1b[3;5HABCDEFX"));
    assert_eq!(s.screen[2], over(2, 4, "ABCDEF"));
    assert_eq!(s.screen[3], over(3, 4, "X"));
}

/// **Every absolute and relative move that lands one past the margin
/// places the cursor there**: `CUP`, `HVP`, `CHA`, `HPA`, `HPR`, `VPA`
/// from it, `CUB` and `BS` from right of it.
#[test]
fn every_move_to_one_past_the_margin_places_the_cursor() {
    for tail in [
        "\x1b[3;11H",
        "\x1b[3;11f",
        "\x1b[3;1H\x1b[11G",
        "\x1b[3;1H\x1b[11`",
        "\x1b[3;8H\x1b[3a",
        "\x1b[1;11H\x1b[3d",
        "\x1b[3;13H\x1b[2D",
        "\x1b[3;12H\x08",
    ] {
        let s = seen(&format!("{LRM}{tail}"));
        assert_eq!(
            (s.cursor, s.pending),
            ((2, 10), WrapPending::No),
            "{tail:?}"
        );
        let s = seen(&format!("{LRM}{tail}Q"));
        assert_eq!(s.screen[2], over(2, 10, "Q"), "{tail:?}");
    }
}

// ── DECAWM reset ────────────────────────────────────────────────────────────

/// **With `?7l` the boundary right of the fence is the last column**, not
/// the right margin (p. 5-139: "will not advance beyond the Right Margin
/// or last column of the Active Line"), and characters past it overwrite
/// that column.
#[test]
fn without_autowrap_right_of_the_fence_writes_to_the_last_column() {
    let s = seen(&format!("{LRM}\x1b[?7l\x1b[3;15HXYZ"));
    assert_eq!(s.screen[2], over(2, 14, "XYZ"));
    assert_eq!(s.cursor, (2, 17));
    let s = seen(&format!("{LRM}\x1b[?7l\x1b[3;18HPQRS"));
    assert_eq!(s.screen[2], over(2, 17, "PQS"));
    assert_eq!((s.cursor, s.pending), ((2, 19), WrapPending::No));
    let s = seen(&format!("{LRM}\x1b[?7l\x1b[3;11HPQ"));
    assert_eq!(s.screen[2], over(2, 10, "PQ"));
}

// ── reports ─────────────────────────────────────────────────────────────────

/// **A placed cursor, and one right of the fence, report their own
/// column**; a pending wrap reports the margin it waits at, as before.
#[test]
fn cpr_reports_a_placed_cursor_where_it_is() {
    let s = seen(&format!("{LRM}\x1b[3;15H\x1b[6n"));
    assert_eq!(
        (s.cursor, s.reply.as_slice()),
        ((2, 14), &b"\x1b[3;15R"[..])
    );
    let s = seen(&format!("{LRM}\x1b[3;11H\x1b[6n"));
    assert_eq!(
        (s.cursor, s.reply.as_slice()),
        ((2, 10), &b"\x1b[3;11R"[..])
    );
    let s = seen(&format!("{LRM}\x1b[3;5HABCDEF\x1b[6n"));
    assert_eq!((s.cursor, s.reply.as_slice()), ((2, 9), &b"\x1b[3;10R"[..]));
}

// ── moves from one past the margin ──────────────────────────────────────────

/// **From a placed cursor the moves see a column beyond the margin**: `CUF`
/// note 2 (p. 5-45) runs on to the last column, `HT` note 1 (p. 5-67) goes
/// to the next stop, and `BS` and `CUB` (pp. 5-63, 5-47) step one left.
#[test]
fn moves_from_one_past_the_margin_start_beyond_it() {
    assert_eq!(x_at(&format!("{LRM}\x1b[3;11H\x1b[2C")), (2, 12));
    assert_eq!(x_at(&format!("{LRM}\x1b[3;11H\t")), (2, 16));
    assert_eq!(x_at(&format!("{LRM}\x1b[3;11H\x08")), (2, 9));
    assert_eq!(x_at(&format!("{LRM}\x1b[3;11H\x1b[D")), (2, 9));
}

/// The controls: **from a pending wrap they see the margin**, as built in
/// step (c).
#[test]
fn moves_from_a_pending_wrap_start_on_the_margin() {
    assert_eq!(x_at(&format!("{LRM}\x1b[3;5HABCDEF\x1b[2C")), (2, 9));
    assert_eq!(x_at(&format!("{LRM}\x1b[3;5HABCDEF\t")), (2, 9));
}

/// **`IL` and `DL` are inert at a placed cursor**: it is outside the
/// fence, where they "do not work" (p. 5-24). At a pending wrap they act,
/// as before.
#[test]
fn il_at_a_placed_cursor_does_nothing() {
    let s = seen(&format!("{LRM}\x1b[3;11H\x1b[L"));
    assert_eq!(s.screen[3], over(3, 0, ""));
    assert_eq!(s.cursor, (2, 10));
    let s = seen(&format!("{LRM}\x1b[3;5HABCDEF\x1b[L"));
    assert_eq!(s.screen[3], over(3, 4, "ABCDEF"), "moved down a row");
}

// ── save and restore ────────────────────────────────────────────────────────

/// **`DECRC` restores a placed cursor placed**: the save is not a pending
/// wrap, so the restored cursor writes where it stands.
#[test]
fn decrc_restores_a_placed_cursor_placed() {
    let s = seen(&format!("{LRM}\x1b[3;11H\x1b7\x1b[1;1H\x1b8A"));
    assert_eq!(s.screen[2], over(2, 10, "A"));
    assert_eq!(s.screen[3], over(3, 0, ""));
}

/// The control: **a pending wrap saved under the fence still wraps** when
/// restored there.
#[test]
fn decrc_restores_a_pending_wrap_pending() {
    let s = seen(&format!("{LRM}\x1b[3;5HABCDEF\x1b7\x1b[1;1H\x1b8X"));
    assert_eq!(s.screen[3], over(3, 4, "X"));
}

// ── what the margins leave ──────────────────────────────────────────────────

/// **`?69l` leaves a placed cursor on its column**, which at full width is
/// an ordinary one.
#[test]
fn margins_off_leave_a_placed_cursor_a_column() {
    let s = seen(&format!("{LRM}\x1b[3;11H\x1b[?69l"));
    assert_eq!((s.cursor, s.pending), ((2, 10), WrapPending::No));
    assert_eq!(x_at(&format!("{LRM}\x1b[3;11H")), (2, 10));
}

/// **A mark after a move is a cluster of its own and takes the cursor's
/// cell as a character would**, there as anywhere else -- a placed cursor
/// is not past the screen. It attached to the cell before the cursor until
/// 2026-10-23 (`[[source: uax29]]` GB4/GB5; note AB, Q24 item 1).
#[test]
fn a_mark_at_a_placed_cursor_takes_the_cell_a_character_would() {
    let s = seen(&format!("{LRM}\x1b[3;11H\u{301}"));
    let z = seen(&format!("{LRM}\x1b[3;11Hz"));
    let full = seen("\x1b[3;11H\u{301}");
    assert_eq!(
        (s.cursor, s.pending),
        (z.cursor, z.pending),
        "where a character leaves it"
    );
    assert_eq!(s.screen, full.screen, "on column 10, as at full width");
}

/// The control: **a cursor left of the fence writes to the right margin
/// and wraps** -- its column is not past the margin (p. 5-140).
#[test]
fn left_of_the_fence_writes_to_the_right_margin() {
    let s = seen(&format!("{LRM}\x1b[3;2HABCDEFGHIJ"));
    assert_eq!(s.screen[2], over(2, 1, "ABCDEFGHI"));
    assert_eq!(s.screen[3], over(3, 4, "J"));
}

/// **`ICH` and `DCH` are ignored right of the fence**, `[[source:
/// dec-std-070]]` ICH note 2 (p. 5-142) and DCH note 3 (p. 5-144): "If the
/// Active Position is outside the left or right margins, this control is
/// ignored." Both took the cursor's column from one past the right margin
/// and underflowed there, a panic in a debug build.
#[test]
fn ich_and_dch_right_of_the_fence_do_nothing() {
    let untouched = seen("").screen;
    for op in ["\x1b[2@", "\x1b[2P"] {
        for at in ["\x1b[3;15H", "\x1b[3;11H"] {
            let s = seen(&format!("{LRM}{at}{op}"));
            assert_eq!(s.screen, untouched, "{at:?}{op:?}");
        }
    }
}
