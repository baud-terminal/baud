//! **`CUU` and `CUD` stop at the top and bottom margins, DECOM or not.**
//!
//! * `[[source: dec-std-070]]` CUU, p. 5-41, note 1: "If the Active
//!   Position is at or below the Top Margin when the CUU control is
//!   executed, and an attempt is made to move the Active Position above the
//!   Top Margin, the control will be executed until the Active Position
//!   reaches the Top Margin. The Active Position will not move beyond the
//!   Top Margin." Note 2: from above it, "until the Active Position reaches
//!   the first line of the Page".
//! * CUD, p. 5-43, notes 1 and 2, the mirror: from at or above the Bottom
//!   Margin it stops there; from below it, at "the bottom line of the
//!   Page".
//! * The algorithms (pp. 5-42, 5-44) test one side only --
//!   `active_position.line >= top_margin` for `CUU`, `<= bottom_margin`
//!   for `CUD` -- and nothing of origin mode, so a `CUU` from below the
//!   region stops at its **top**. xterm's `CursorUp` and `CursorDown`
//!   (`[[source: xterm-cursor-c]]` 251-285) and ghostty's `cursorUp` and
//!   `cursorDown` (`[[source: ghostty-d9840f3-terminal]]` 1849-1878) are
//!   the same test.
//! * `CNL` and `CPL` are `CUD` and `CUU` and then `CR`'s line start
//!   (`[[source: xterm-cursor-c]]` 527-549), so they stop where those do.
//!
//! 20 by 8, every row its own letter, the region rows 2..6 (1-based). Every
//! case lands the same at every read size and at every split into two
//! reads.

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
/// Top and bottom margins at rows 3..6 (1-based), so two rows sit above.
const TB3: &str = "\x1b[3;6r";
/// `?69h`, then side margins at columns 5..10 (1-based); 0-based 4..=9.
const LRM: &str = "\x1b[?69h\x1b[5;10s";

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

/// The cursor's row and column, 0-based, after `tail`, and an `X` written
/// there landing on that cell -- the screen's word for it as well as the
/// accessor's.
fn at(tail: &str) -> (usize, usize) {
    let s = seen(&format!("{tail}X"));
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
    assert_eq!(hits.len(), 1, "one X on the screen: {tail:?}");
    let (y, x) = hits[0];
    assert_eq!(s.cursor, (Row::from_index(y), Col::at_index(x + 1)));
    (y, x)
}

// ── CUU ────────────────────────────────────────────────────────────────────

/// **From inside the region `CUU` stops at the top margin**, note 1.
#[test]
fn cuu_inside_the_region_stops_at_the_top_margin() {
    assert_eq!(at(&format!("{TB}\x1b[4;3H\x1b[99A")), (1, 2));
    assert_eq!(at(&format!("{TB}\x1b[4;3H\x1b[3A")), (1, 2));
}

/// **On the top margin it stays there**: "at or below the Top Margin".
#[test]
fn cuu_on_the_top_margin_stays_there() {
    assert_eq!(at(&format!("{TB}\x1b[2;3H\x1b[A")), (1, 2));
    assert_eq!(at(&format!("{TB}\x1b[2;3H\x1b[9A")), (1, 2));
}

/// **From below the region too**: the algorithm's test is
/// `line >= top_margin`, so a `CUU` from under the bottom margin crosses the
/// region and stops at its top, not at the page's.
#[test]
fn cuu_from_below_the_region_stops_at_the_top_margin() {
    assert_eq!(at(&format!("{TB}\x1b[8;3H\x1b[99A")), (1, 2));
}

/// **From above the region it goes on to the first line**, note 2.
#[test]
fn cuu_above_the_region_goes_to_the_first_line() {
    assert_eq!(at(&format!("{TB3}\x1b[2;3H\x1b[99A")), (0, 2));
    assert_eq!(at(&format!("{TB}\x1b[1;3H\x1b[A")), (0, 2));
}

// ── CUD ────────────────────────────────────────────────────────────────────

/// **From inside the region `CUD` stops at the bottom margin**, note 1.
#[test]
fn cud_inside_the_region_stops_at_the_bottom_margin() {
    assert_eq!(at(&format!("{TB}\x1b[3;3H\x1b[99B")), (5, 2));
    assert_eq!(at(&format!("{TB}\x1b[3;3H\x1b[3B")), (5, 2));
}

/// **On the bottom margin it stays there**: "at or above the Bottom Margin".
#[test]
fn cud_on_the_bottom_margin_stays_there() {
    assert_eq!(at(&format!("{TB}\x1b[6;3H\x1b[B")), (5, 2));
    assert_eq!(at(&format!("{TB}\x1b[6;3H\x1b[9B")), (5, 2));
}

/// **From above the region too**, the mirror of `CUU` from below.
#[test]
fn cud_from_above_the_region_stops_at_the_bottom_margin() {
    assert_eq!(at(&format!("{TB}\x1b[1;3H\x1b[99B")), (5, 2));
}

/// **From below the region it goes on to the last line**, note 2.
#[test]
fn cud_below_the_region_goes_to_the_last_line() {
    assert_eq!(at(&format!("{TB}\x1b[7;3H\x1b[99B")), (7, 2));
}

// ── The controls ───────────────────────────────────────────────────────────

/// The control: **a move inside the region that reaches no margin moves by
/// its count**, and a zero moves one.
#[test]
fn a_move_inside_the_region_moves_by_its_count() {
    assert_eq!(at(&format!("{TB}\x1b[5;3H\x1b[2A")), (2, 2));
    assert_eq!(at(&format!("{TB}\x1b[2;3H\x1b[2B")), (3, 2));
    assert_eq!(at(&format!("{TB}\x1b[4;3H\x1b[0A")), (2, 2));
    assert_eq!(at(&format!("{TB}\x1b[4;3H\x1b[0B")), (4, 2));
}

/// The control: **with no region the margins are the page's edges.**
#[test]
fn without_a_region_they_stop_at_the_page() {
    assert_eq!(at("\x1b[5;3H\x1b[99A"), (0, 2));
    assert_eq!(at("\x1b[5;3H\x1b[99B"), (7, 2));
}

/// The control: **under DECOM nothing changes**, the cursor being inside
/// the region already.
#[test]
fn under_decom_they_stop_at_the_margins_as_before() {
    assert_eq!(at(&format!("{TB}\x1b[?6h\x1b[3;3H\x1b[99A")), (1, 2));
    assert_eq!(at(&format!("{TB}\x1b[?6h\x1b[3;3H\x1b[99B")), (5, 2));
}

// ── CNL and CPL ────────────────────────────────────────────────────────────

/// **`CNL` and `CPL` stop at the margins as `CUD` and `CUU` do**, then
/// start the line.
#[test]
fn cnl_and_cpl_stop_at_the_margins() {
    assert_eq!(at(&format!("{TB}\x1b[3;7H\x1b[99E")), (5, 0));
    assert_eq!(at(&format!("{TB}\x1b[5;7H\x1b[99F")), (1, 0));
    assert_eq!(at(&format!("{TB}\x1b[7;7H\x1b[99E")), (7, 0));
    assert_eq!(at(&format!("{TB3}\x1b[2;7H\x1b[99F")), (0, 0));
}

/// **And the line they stop on starts at the left margin**, being inside
/// the scroll area.
#[test]
fn cnl_and_cpl_stopped_at_a_margin_start_at_the_left_margin() {
    assert_eq!(at(&format!("{TB}{LRM}\x1b[3;7H\x1b[99E")), (5, 4));
    assert_eq!(at(&format!("{TB}{LRM}\x1b[8;7H\x1b[99F")), (1, 4));
}
