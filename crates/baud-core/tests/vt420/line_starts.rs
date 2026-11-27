//! **Line starts and cursor moves under side margins, with DECOM reset.**
//!
//! * `[[source: dec-std-070]]` CR, p. 5-58: "The CR control moves the Active
//!   Position to the Left Margin. If the Active Position is before the left
//!   margin, or outside the top and bottom margins, the cursor moves to the
//!   first column of the Active Line."
//! * NEL, p. 5-64: the same rule, and its algorithm is `carriage_return();
//!   line_feed();`, so the line it leaves decides. LF's algorithm (p. 5-60)
//!   calls `carriage_return()` first under New Line Mode.
//! * CUF, p. 5-45, notes 1 and 2: from at or inside the Right Margin it stops
//!   there; from beyond it, at the last column. CUB, p. 5-47, the mirror.
//! * IL and DL, note 2 (pp. 5-146 and 5-148): "The Active Position is set to
//!   the Left Margin in the active line after execution of this control."
//! * `CNL`, `CPL` and `HPR` have no DEC algorithm: `[[source: vt510-cnl]]`
//!   and `[[source: vt510-cpl]]` say "the first character of the n-th
//!   following line" and `[[source: vt510-hpr]]` "the last position on the
//!   line". `CNL` and `CPL` take `CR`'s rule on the line they land on, as
//!   xterm's do (`[[source: xterm-cursor-c]]` 527-549); `HPR` goes to the
//!   line's last position, as xterm's `CursorSet` and ghostty do.
//!
//! 20 by 8, every row its own letter, and the probe of `tab_backspace.rs`:
//! `?69l` puts the margins back without moving the cursor, so its `X` lands
//! on the cursor's own column, right of the old fence included. Every case
//! lands the same at every read size and at every split into two reads.

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

/// Where `ch` is, 0-based, if it is on the screen once.
fn find(s: &Seen, ch: char) -> Option<(usize, usize)> {
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

/// Where the probe's `X` lands after `tail`.
fn x_at(tail: &str) -> Option<(usize, usize)> {
    find(&seen(&format!("{tail}{PROBE}")), 'X')
}

// ── CR ─────────────────────────────────────────────────────────────────────

/// **From inside the fence `CR` goes to the left margin**, column 4
/// (0-based), with or without top and bottom margins.
#[test]
fn cr_inside_the_fence_goes_to_the_left_margin() {
    assert_eq!(x_at(&format!("{LRM}\x1b[3;7HAB\r")), Some((2, 4)));
    assert_eq!(x_at(&format!("{TB}{LRM}\x1b[3;7H\r")), Some((2, 4)));
    // On the left margin itself it stays there.
    assert_eq!(x_at(&format!("{LRM}\x1b[3;5H\r")), Some((2, 4)));
}

/// **Right of the fence too**: the algorithm tests `column >= left_margin`
/// and nothing about the right.
#[test]
fn cr_right_of_the_fence_goes_to_the_left_margin() {
    assert_eq!(x_at(&format!("{LRM}\x1b[3;15H\r")), Some((2, 4)));
}

/// **At a pending wrap**, one past the right margin, `CR` goes to the left
/// margin and the wrap is gone.
#[test]
fn cr_at_a_pending_wrap_goes_to_the_left_margin() {
    let s = seen(&format!("{LRM}\x1b[3;5HABCDEF\rX"));
    assert_eq!((find(&s, 'X'), s.pending), (Some((2, 4)), WrapPending::No));
}

/// The control: **before the left margin `CR` goes to column 1.**
#[test]
fn cr_left_of_the_fence_goes_to_column_one() {
    assert_eq!(x_at(&format!("{LRM}\x1b[3;2H\r")), Some((2, 0)));
    assert_eq!(x_at(&format!("{TB}{LRM}\x1b[3;4H\r")), Some((2, 0)));
}

/// **Outside the top and bottom margins `CR` goes to column 1**, at a
/// column inside the fence: the text's second condition, which xterm and
/// ghostty do not test.
#[test]
fn cr_outside_the_scroll_area_goes_to_column_one() {
    assert_eq!(x_at(&format!("{TB}{LRM}\x1b[8;7H\r")), Some((7, 0)));
    assert_eq!(x_at(&format!("{TB}{LRM}\x1b[1;7H\r")), Some((0, 0)));
    // The margin rows are inside.
    assert_eq!(x_at(&format!("{TB}{LRM}\x1b[2;7H\r")), Some((1, 4)));
    assert_eq!(x_at(&format!("{TB}{LRM}\x1b[6;7H\r")), Some((5, 4)));
}

/// **`CR LF`, folded into one step, starts the next line at the left
/// margin**, and a split between the two bytes lands the same.
#[test]
fn cr_lf_starts_the_next_line_at_the_left_margin() {
    assert_eq!(x_at(&format!("{LRM}\x1b[3;7H\r\n")), Some((3, 4)));
    assert_eq!(x_at(&format!("{LRM}\x1b[3;2H\r\n")), Some((3, 0)));
}

/// **A control went past**: a regional indicator after a `CR` to the left
/// margin does not pair with the one before it, and the second half takes
/// cells of its own -- as under DECOM (`origin.rs`).
#[test]
fn cr_to_the_left_margin_breaks_a_cluster() {
    let s = seen(&format!("{LRM}\x1b[3;7H\u{1F1FA}\r\u{1F1F8}X"));
    assert_eq!(
        (s.screen[2].chars().nth(4), find(&s, 'X')),
        (Some('\u{1F1F8}'), Some((2, 6)))
    );
}

/// **`?69h` with the margins at the page's edges** changes nothing: the
/// left margin is column 1. Nor does DECOM without `?69h`, whose origin is
/// column 1 then, and the inline test reads only the `?69` bit.
#[test]
fn cr_with_full_width_margins_goes_to_column_one() {
    assert_eq!(x_at("\x1b[?69h\x1b[3;7H\r"), Some((2, 0)));
    assert_eq!(x_at("\x1b[3;7H\r"), Some((2, 0)));
    // `CUP 2;3` under DECOM is the region's second line, row 2 (0-based).
    assert_eq!(x_at(&format!("{TB}\x1b[?6h\x1b[2;3H\r")), Some((2, 0)));
    // `?69l` puts the margins back, so the line starts at column 1 again.
    assert_eq!(x_at(&format!("{LRM}\x1b[?69l\x1b[3;7H\r")), Some((2, 0)));
    assert_eq!(x_at(&format!("{LRM}\x1b[!p\x1b[3;7H\r")), Some((2, 0)));
}

// ── NEL and LF under LNM ───────────────────────────────────────────────────

/// **`NEL` is `CR` and then `LF`**: from inside the fence the next line
/// starts at the left margin.
#[test]
fn nel_inside_the_fence_starts_at_the_left_margin() {
    assert_eq!(x_at(&format!("{LRM}\x1b[3;7H\x1bE")), Some((3, 4)));
    assert_eq!(x_at(&format!("{TB}{LRM}\x1b[3;7H\x1bE")), Some((3, 4)));
    assert_eq!(x_at(&format!("{LRM}\x1b[3;2H\x1bE")), Some((3, 0)));
}

/// **The line it leaves decides**: from the row above the top margin
/// `NEL` lands on the top margin's row at column 1, because the `CR` came
/// first, outside the scroll area. And `LF` under `LNM` the same.
#[test]
fn nel_and_lnm_judge_the_line_they_leave() {
    assert_eq!(x_at(&format!("{TB}{LRM}\x1b[1;7H\x1bE")), Some((1, 0)));
    assert_eq!(x_at(&format!("{TB}{LRM}\x1b[20h\x1b[1;7H\n")), Some((1, 0)));
    // From the bottom margin's row, leaving it inside, onto the row below
    // the region is not possible: the region scrolls and the line stays.
    assert_eq!(x_at(&format!("{TB}{LRM}\x1b[6;7H\x1bE")), Some((5, 4)));
}

/// **`LF` under `LNM`** starts the next line at the left margin, and without
/// `LNM` keeps its column.
#[test]
fn lf_under_lnm_starts_at_the_left_margin() {
    assert_eq!(x_at(&format!("{LRM}\x1b[20h\x1b[3;7H\n")), Some((3, 4)));
    assert_eq!(x_at(&format!("{LRM}\x1b[3;7H\n")), Some((3, 6)));
}

// ── CNL and CPL ────────────────────────────────────────────────────────────

/// **`CNL` and `CPL` start the line they land on as `CR` does.**
#[test]
fn cnl_and_cpl_start_at_the_left_margin() {
    assert_eq!(x_at(&format!("{LRM}\x1b[3;7H\x1b[E")), Some((3, 4)));
    assert_eq!(x_at(&format!("{LRM}\x1b[3;7H\x1b[2F")), Some((0, 4)));
    assert_eq!(x_at(&format!("{LRM}\x1b[3;2H\x1b[E")), Some((3, 0)));
    assert_eq!(x_at(&format!("{LRM}\x1b[3;15H\x1b[F")), Some((1, 4)));
}

/// **The line they land on decides**: from the row above the top margin a
/// `CNL` lands inside the scroll area and starts at the left margin, and a
/// `CPL` from below the region lands below it and starts at column 1.
#[test]
fn cnl_and_cpl_judge_the_line_they_land_on() {
    assert_eq!(x_at(&format!("{TB}{LRM}\x1b[1;7H\x1b[E")), Some((1, 4)));
    assert_eq!(x_at(&format!("{TB}{LRM}\x1b[8;7H\x1b[F")), Some((6, 0)));
}

// ── CUF and HPR ────────────────────────────────────────────────────────────

/// **From inside the fence `CUF` stops at the right margin**, column 9.
#[test]
fn cuf_inside_the_fence_stops_at_the_right_margin() {
    assert_eq!(x_at(&format!("{LRM}\x1b[3;8H\x1b[5C")), Some((2, 9)));
    assert_eq!(x_at(&format!("{LRM}\x1b[3;8H\x1b[C")), Some((2, 8)));
    assert_eq!(x_at(&format!("{LRM}\x1b[3;10H\x1b[C")), Some((2, 9)));
}

/// **From left of the fence too**: note 1 asks only "at or inside the Right
/// Margin".
#[test]
fn cuf_from_left_of_the_fence_stops_at_the_right_margin() {
    assert_eq!(x_at(&format!("{LRM}\x1b[3;2H\x1b[20C")), Some((2, 9)));
}

/// **From beyond the right margin it goes on to the last column**, note 2.
#[test]
fn cuf_beyond_the_right_margin_stops_at_the_last_column() {
    assert_eq!(x_at(&format!("{LRM}\x1b[3;13H\x1b[3C")), Some((2, 15)));
    assert_eq!(x_at(&format!("{LRM}\x1b[3;13H\x1b[30C")), Some((2, 19)));
    // Under `?7l` the column one past the margin is beyond it.
    assert_eq!(
        x_at(&format!("{LRM}\x1b[?7l\x1b[3;11H\x1b[2C")),
        Some((2, 12))
    );
}

/// **At a pending wrap `CUF` stays on the right margin**, and the wrap is
/// gone: the next character writes on the margin, not on the next line.
#[test]
fn cuf_at_a_pending_wrap_stays_on_the_right_margin() {
    assert_eq!(
        seen(&format!("{LRM}\x1b[3;5HABCDEF")).pending,
        WrapPending::Yes
    );
    let s = seen(&format!("{LRM}\x1b[3;5HABCDEF\x1b[C"));
    assert_eq!(s.pending, WrapPending::No);
    let s = seen(&format!("{LRM}\x1b[3;5HABCDEF\x1b[CX"));
    assert_eq!(find(&s, 'X'), Some((2, 9)));
}

/// **Under `?7h` a cursor placed one past the right margin is beyond it**
/// too, and note 2 moves it on: a move put it there, so no wrap is pending
/// (V10's step (d), `corner.rs`).
#[test]
fn cuf_from_a_cursor_placed_one_past_the_margin_moves_on() {
    assert_eq!(x_at(&format!("{LRM}\x1b[3;11H\x1b[2C")), Some((2, 12)));
}

/// The control: **`HPR` goes to the line's last position**, the right
/// margin aside ("the last position on the line").
#[test]
fn hpr_passes_the_right_margin() {
    assert_eq!(x_at(&format!("{LRM}\x1b[3;8H\x1b[5a")), Some((2, 12)));
    assert_eq!(x_at(&format!("{LRM}\x1b[3;13H\x1b[30a")), Some((2, 19)));
}

/// The control: **at full width nothing changes**, DECOM included, whose
/// margins are then the page's edges.
#[test]
fn cuf_at_full_width_stops_at_the_last_column() {
    assert_eq!(x_at("\x1b[3;8H\x1b[30C"), Some((2, 19)));
    assert_eq!(
        x_at(&format!("{TB}\x1b[?6h\x1b[2;3H\x1b[30C")),
        Some((2, 19))
    );
    assert_eq!(
        x_at(&format!("{TB}\x1b[?6h\x1b[2;3H\x1b[30D")),
        Some((2, 0))
    );
    let s = seen("\x1b[3;1Hxxxxxxxxxxxxxxxxxxxx\x1b[CX");
    assert_eq!(find(&s, 'X'), Some((2, 19)));
}

// ── CUB ────────────────────────────────────────────────────────────────────

/// **From inside the fence `CUB` stops at the left margin**, column 4.
#[test]
fn cub_inside_the_fence_stops_at_the_left_margin() {
    assert_eq!(x_at(&format!("{LRM}\x1b[3;7H\x1b[5D")), Some((2, 4)));
    assert_eq!(x_at(&format!("{LRM}\x1b[3;7H\x1b[D")), Some((2, 5)));
    assert_eq!(x_at(&format!("{LRM}\x1b[3;5H\x1b[D")), Some((2, 4)));
}

/// **From beyond the right margin too**: the algorithm tests `column >=
/// left_margin` only.
#[test]
fn cub_from_right_of_the_fence_stops_at_the_left_margin() {
    assert_eq!(x_at(&format!("{LRM}\x1b[3;15H\x1b[20D")), Some((2, 4)));
}

/// The control: **before the left margin it goes on to column 1**, note 2.
#[test]
fn cub_left_of_the_fence_goes_to_column_one() {
    assert_eq!(x_at(&format!("{LRM}\x1b[3;4H\x1b[5D")), Some((2, 0)));
    assert_eq!(x_at(&format!("{LRM}\x1b[3;4H\x1b[D")), Some((2, 2)));
}

// ── IL and DL ──────────────────────────────────────────────────────────────

/// **`IL` and `DL` put the cursor on the left margin**, on its own row:
/// column 1 at full width, which the cursor kept until this change.
#[test]
fn il_and_dl_at_full_width_go_to_column_one() {
    assert_eq!(x_at("\x1b[3;6H\x1b[L"), Some((2, 0)));
    assert_eq!(x_at("\x1b[3;6H\x1b[2M"), Some((2, 0)));
    assert_eq!(x_at(&format!("{TB}\x1b[3;6H\x1b[L")), Some((2, 0)));
}

/// **Under side margins, to the left margin**, the rectangle moved as before.
#[test]
fn il_and_dl_inside_the_fence_go_to_the_left_margin() {
    let s = seen(&format!("{TB}{LRM}\x1b[3;7H\x1b[L{PROBE}"));
    assert_eq!(find(&s, 'X'), Some((2, 4)));
    assert_eq!(s.screen[2], "ccccX.....cccccccccc");
    assert_eq!(s.screen[3], "ddddccccccdddddddddd");
    let s = seen(&format!("{TB}{LRM}\x1b[3;10H\x1b[M{PROBE}"));
    assert_eq!(find(&s, 'X'), Some((2, 4)));
    assert_eq!(s.screen[2], "ccccXdddddcccccccccc");
}

/// **At a pending wrap `IL` puts the cursor on the left margin**, and the
/// wrap is gone: full width and under the fence.
#[test]
fn il_at_a_pending_wrap_goes_to_the_left_margin() {
    let s = seen("\x1b[3;1Hxxxxxxxxxxxxxxxxxxxx\x1b[LX");
    assert_eq!(find(&s, 'X'), Some((2, 0)));
    let s = seen(&format!("{TB}{LRM}\x1b[3;5HABCDEF\x1b[LX"));
    assert_eq!(find(&s, 'X'), Some((2, 4)));
}

/// The control: **where `IL` and `DL` are ignored the cursor stays**, left
/// of the fence and outside the scroll area.
#[test]
fn ignored_il_and_dl_leave_the_cursor() {
    assert_eq!(x_at(&format!("{TB}{LRM}\x1b[3;2H\x1b[L")), Some((2, 1)));
    assert_eq!(x_at(&format!("{TB}{LRM}\x1b[3;15H\x1b[M")), Some((2, 14)));
    assert_eq!(x_at(&format!("{TB}\x1b[8;6H\x1b[L")), Some((7, 5)));
}
