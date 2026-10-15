//! **`HT` and `BS` under side margins: where the cursor starts decides
//! which edge stops it.**
//!
//! * `[[source: dec-std-070]]` HT, p. 5-67, note 1: "If no tabulation stop
//!   is reached before the Right Margin of the Active Line (within the Scroll
//!   Area), the Active Position will be set to the Right Margin of the Active
//!   Line. If the Active Column is beyond the Right Margin or the Active Line
//!   is outside the Scroll Area (due to absolute cursor positioning), the
//!   Active Position will move to the next tab stop, or last column of the
//!   Active Line, whichever comes first."
//! * BS, p. 5-63, notes 1 and 2: "If the Active Position is at or inside the
//!   Left Margin and within the Scroll Area, it will not move beyond the Left
//!   Margin." "If the Active Position is already before the Left Margin, the
//!   Active Position will move left one column unless it is at the first
//!   column of the Active Line."
//!
//! **The scroll area includes its margin rows.** HT's algorithm (p. 5-68)
//! tests `line > top_margin && line < bottom_margin`, which read literally
//! puts the top and bottom margin rows outside it -- and with no `DECSTBM`,
//! the first and last rows of the screen. Note 1's "within the Scroll Area"
//! and BS's algorithm (`line < top_margin || line > bottom_margin` is
//! outside) both include them; that reading is built, and the conflict is
//! open (`docs/record.md`, "HT and BS under side margins").
//!
//! 20 by 8, every row its own letter. **A probe that reads where the cursor
//! is, not where `Term::cursor` clamps it**: `?69l` puts the margins back
//! without moving the cursor, so the `X` written after it lands on the
//! cursor's own column, right of the old fence included, with no wrap first
//! (`a_cursor_right_of_the_fence_is_read_where_it_is` is the control). Every
//! case is required to land the same at every read size and at every split
//! into two reads.

use baud_core::*;

const ROWS: usize = 8;
const COLS: usize = 20;
const WS: WinSize = WinSize {
    ws_row: 8,
    ws_col: 20,
};
const LETTERS: &[u8; ROWS] = b"abcdefgh";

/// Top and bottom margins at rows 2..6 (1-based); 0-based 1..=5.
const TB: &str = "\x1b[2;6r";
/// `?69h`, then side margins at columns 5..10 (1-based); 0-based 4..=9.
const LRM: &str = "\x1b[?69h\x1b[5;10s";
/// The margins put back, the cursor left where it is, and a mark there.
const PROBE: &str = "\x1b[?69lX";
/// Reverse-wraparound, `?45`.
const RW: &str = "\x1b[?45h";

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
    screen: Vec<String>,
    cursor: (Row, Col),
    pending: WrapPending,
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
        pending: t.wrap_pending(),
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

/// Where the probe's `X` is, 0-based, if it is on the screen once.
fn x_at(tail: &str) -> Option<(usize, usize)> {
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
    match hits.as_slice() {
        [one] => Some(*one),
        _ => None,
    }
}

// ── the probe ──────────────────────────────────────────────────────────────

/// **The probe can see a cursor right of the fence.** `CUP 3;14` under
/// margins 5..10 puts the cursor at column 13 (0-based), which
/// `Term::cursor` reports as the right margin and a write there wraps first;
/// the probe's `X` lands on column 13 itself. And on the left of it.
#[test]
fn a_cursor_right_of_the_fence_is_read_where_it_is() {
    assert_eq!(x_at(&format!("{TB}{LRM}\x1b[3;14H")), Some((2, 13)));
    assert_eq!(x_at(&format!("{TB}{LRM}\x1b[3;2H")), Some((2, 1)));
    assert_eq!(x_at(&format!("{TB}{LRM}\x1b[3;7H")), Some((2, 6)));
}

// ── HT ─────────────────────────────────────────────────────────────────────

/// From inside the margins, inside the scroll area, `HT` stops at the right
/// margin when no stop comes first: from the stop at column 8 (0-based) the
/// next is 16, past the margin at 9.
#[test]
fn ht_inside_the_margins_stops_at_the_right_margin() {
    assert_eq!(x_at(&format!("{TB}{LRM}\x1b[3;9H\t")), Some((2, 9)));
    // A stop before the margin is still taken.
    assert_eq!(x_at(&format!("{TB}{LRM}\x1b[3;6H\t")), Some((2, 8)));
}

/// On the right margin it stays there.
#[test]
fn ht_on_the_right_margin_stays_there() {
    assert_eq!(x_at(&format!("{TB}{LRM}\x1b[3;10H\t")), Some((2, 9)));
}

/// From left of the left margin, inside the scroll area, the right margin
/// still stops it: the note's first sentence names no left side. With every
/// stop cleared, the right margin is where it ends.
#[test]
fn ht_from_left_of_the_left_margin_stops_at_the_right_margin() {
    assert_eq!(x_at(&format!("{TB}{LRM}\x1b[3g\x1b[3;2H\t")), Some((2, 9)));
    assert_eq!(x_at(&format!("{TB}{LRM}\x1b[3;2H\t")), Some((2, 8)));
}

/// **Beyond the right margin it moves to the next stop, or the last
/// column**, whichever comes first. Two columns right of the margin and
/// more: exactly one right of it is where a pending wrap stands, below.
#[test]
fn ht_right_of_the_right_margin_goes_to_the_next_stop_or_the_last_column() {
    assert_eq!(x_at(&format!("{TB}{LRM}\x1b[3;12H\t")), Some((2, 16)));
    assert_eq!(x_at(&format!("{TB}{LRM}\x1b[3;18H\t")), Some((2, 19)));
}

/// **One past the right margin with `?7l` is beyond it**: no pending wrap
/// exists without `DECAWM`, so the column is the cursor's own and note 1
/// sends it to the next stop.
#[test]
fn ht_one_past_the_right_margin_without_autowrap_goes_to_the_next_stop() {
    assert_eq!(
        x_at(&format!("{TB}{LRM}\x1b[?7l\x1b[3;11H\t")),
        Some((2, 16))
    );
}

/// **With `?7h` too, a cursor `CUP` placed one past the margin is beyond
/// it**, and note 1 sends it to the next stop: a move put it there, so no
/// wrap is pending (V10's step (d), `corner.rs`).
#[test]
fn ht_one_past_the_right_margin_with_autowrap_goes_to_the_next_stop() {
    assert_eq!(x_at(&format!("{TB}{LRM}\x1b[3;11H\t")), Some((2, 16)));
}

/// **On a row outside the scroll area the right margin does not stop it**:
/// above the region and below it.
#[test]
fn ht_on_a_row_outside_the_scroll_area_passes_the_right_margin() {
    assert_eq!(x_at(&format!("{TB}{LRM}\x1b[1;9H\t")), Some((0, 16)));
    assert_eq!(x_at(&format!("{TB}{LRM}\x1b[8;9H\t")), Some((7, 16)));
    assert_eq!(x_at(&format!("{TB}{LRM}\x1b[1;10H\t")), Some((0, 16)));
}

/// **The margin rows are inside the scroll area**: where the algorithm's
/// strict comparisons and note 1 part. And with no `DECSTBM` the scroll
/// area is the whole screen, its first and last rows included.
#[test]
fn ht_on_the_margin_rows_stops_at_the_right_margin() {
    assert_eq!(x_at(&format!("{TB}{LRM}\x1b[2;9H\t")), Some((1, 9)));
    assert_eq!(x_at(&format!("{TB}{LRM}\x1b[6;9H\t")), Some((5, 9)));
    assert_eq!(x_at(&format!("{LRM}\x1b[1;9H\t")), Some((0, 9)));
    assert_eq!(x_at(&format!("{LRM}\x1b[8;9H\t")), Some((7, 9)));
}

/// **At a pending wrap on the right margin**, `HT` leaves the cursor on the
/// right margin with nothing pending, as it does at full width on the last
/// column. `[[source: dec-std-070]]` D.6.1 (p. D-13): "The Last Column Flag
/// should also be reset by executing any control which potentially changes
/// the Active Position from the end-of-line position", and its list (p.
/// D-14) names `HORIZONTAL_TAB`. Until 2026-10-14 it went on to the stop at
/// 16, past the fence.
#[test]
fn ht_at_a_pending_wrap_stays_on_the_right_margin() {
    let s = seen(&format!("{TB}{LRM}\x1b[3;5Habcdef"));
    assert_eq!(
        s.pending,
        WrapPending::Yes,
        "the six letters fill the margins"
    );
    let s = seen(&format!("{TB}{LRM}\x1b[3;5Habcdef\t"));
    assert_eq!(s.pending, WrapPending::No);
    assert_eq!(s.cursor, (Row::from_index(2), Col::at_index(9)));
    assert_eq!(x_at(&format!("{TB}{LRM}\x1b[3;5Habcdef\t")), Some((2, 9)));
    // Full width: the last column, nothing pending.
    let s = seen(&format!("\x1b[3;1H{}\t", "z".repeat(COLS)));
    assert_eq!(s.pending, WrapPending::No);
    assert_eq!(s.cursor, (Row::from_index(2), Col::at_index(19)));
}

/// `CHT` is `HT` n times, so the right margin stops it too: from column 5
/// (0-based) the first stop is 8, the second the margin at 9.
#[test]
fn cht_stops_at_the_right_margin_as_ht_does() {
    assert_eq!(x_at(&format!("{TB}{LRM}\x1b[3;6H\x1b[2I")), Some((2, 9)));
    assert_eq!(x_at(&format!("{TB}{LRM}\x1b[1;6H\x1b[2I")), Some((0, 16)));
}

// ── BS ─────────────────────────────────────────────────────────────────────

/// At the left margin inside the scroll area `BS` stops: note 1.
#[test]
fn bs_at_the_left_margin_stops_there() {
    assert_eq!(x_at(&format!("{TB}{LRM}\x1b[3;5H\x08")), Some((2, 4)));
    assert_eq!(
        x_at(&format!("{TB}{LRM}\x1b[3;7H\x08\x08\x08\x08")),
        Some((2, 4))
    );
}

/// **Left of the left margin it moves one column**, note 2, and stops at
/// the first column.
#[test]
fn bs_left_of_the_left_margin_moves_one_column() {
    assert_eq!(x_at(&format!("{TB}{LRM}\x1b[3;3H\x08")), Some((2, 1)));
    assert_eq!(
        x_at(&format!("{TB}{LRM}\x1b[3;3H\x08\x08\x08")),
        Some((2, 0))
    );
    assert_eq!(x_at(&format!("{TB}{LRM}\x1b[3;1H\x08")), Some((2, 0)));
}

/// **On a row outside the scroll area the left margin does not stop it**:
/// note 1 holds only "within the Scroll Area", and the algorithm moves a
/// cursor on the margin when its line is above the top margin or below the
/// bottom.
#[test]
fn bs_on_a_row_outside_the_scroll_area_passes_the_left_margin() {
    assert_eq!(x_at(&format!("{TB}{LRM}\x1b[1;5H\x08")), Some((0, 3)));
    assert_eq!(x_at(&format!("{TB}{LRM}\x1b[8;5H\x08")), Some((7, 3)));
    assert_eq!(x_at(&format!("{TB}{LRM}\x1b[8;2H\x08\x08")), Some((7, 0)));
}

/// The margin rows are inside, as for `HT`.
#[test]
fn bs_on_the_margin_rows_stops_at_the_left_margin() {
    assert_eq!(x_at(&format!("{TB}{LRM}\x1b[2;5H\x08")), Some((1, 4)));
    assert_eq!(x_at(&format!("{TB}{LRM}\x1b[6;5H\x08")), Some((5, 4)));
}

/// **`?45` acts only where `BS` would not move**: left of the margin and on
/// a row outside the region it moves one column as above; on the margin
/// inside the region, and on the first column, it reverse-wraps to the
/// previous row's right margin, as before.
#[test]
fn reverse_wrap_takes_over_only_where_bs_stops() {
    assert_eq!(x_at(&format!("{TB}{LRM}{RW}\x1b[3;3H\x08")), Some((2, 1)));
    assert_eq!(x_at(&format!("{TB}{LRM}{RW}\x1b[8;5H\x08")), Some((7, 3)));
    assert_eq!(x_at(&format!("{TB}{LRM}{RW}\x1b[3;5H\x08")), Some((1, 9)));
    assert_eq!(x_at(&format!("{TB}{LRM}{RW}\x1b[3;1H\x08")), Some((1, 9)));
}

/// Full width is the control: `BS` stops at the first column and `HT` at
/// the last, on every row, as before.
#[test]
fn full_width_is_unchanged() {
    assert_eq!(x_at(&format!("{TB}\x1b[3;1H\x08")), Some((2, 0)));
    assert_eq!(x_at(&format!("{TB}\x1b[1;2H\x08\x08")), Some((0, 0)));
    assert_eq!(x_at(&format!("{TB}\x1b[3;18H\t")), Some((2, 19)));
    assert_eq!(x_at(&format!("{TB}\x1b[8;10H\t")), Some((7, 16)));
}
