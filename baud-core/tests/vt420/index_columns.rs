//! **DECBI and DECFI, `ESC 6` and `ESC 9`**: back and forward one column,
//! and at the margin the rectangle the margins bound moves instead.
//!
//! `[[source: dec-std-070]]` p. 5-37 (DECFI): "If the active position was
//! already at the right margin, the contents of the Logical Display Page
//! within the right, left, top and bottom margins shifts left one column. The
//! column shifting beyond the left margin is deleted. A new column is inserted
//! at the right margin with all attributes turned off and the cursor appears
//! in this column. If the active position is outside the left or right margin
//! when the command is received the active position moves forward one column.
//! If the active position was at the right edge of the page, the command is
//! ignored." DECBI (p. 5-39) is the mirror. Their algorithms test the column
//! alone, and the table on p. 5-24 marks both "O", not affected, for the top
//! and bottom margins.

use crate::support;
use baud_core::*;

const COLS: usize = 20;
const WS: WinSize = WinSize {
    ws_row: 8,
    ws_col: 20,
};

fn fill() -> String {
    let mut s = String::new();
    for r in 1..=8 {
        s.push_str(&format!("\x1b[{r};1H0123456789abcdefghij"));
    }
    s
}

fn row(t: &Term<'_>, y: usize) -> String {
    t.row(Row::from_index(y - 1)).expect("a row")[..COLS]
        .iter()
        .map(|c| match c.content.as_codepoint() {
            Some(cp) => char::from_u32(cp.value()).unwrap_or('?'),
            None => '.',
        })
        .collect()
}

/// Margins at rows 2-4 and columns 5-10 (1-based); `?69h` first.
const MARGINS: &str = "\x1b[2;4r\x1b[?69h\x1b[5;10s";

#[test]
fn decfi_at_the_right_margin_moves_the_rectangle_left() {
    let bytes = format!("{}{MARGINS}\x1b[3;10H\x1b9", fill());
    support::render(&WS, bytes.as_bytes(), |t| {
        assert_eq!(
            row(t, 3),
            "012356789.abcdefghij",
            "column 5 gone, a blank at 10"
        );
        assert_eq!(row(t, 2), "012356789.abcdefghij");
        assert_eq!(
            row(t, 1),
            "0123456789abcdefghij",
            "above the top margin, nothing"
        );
        assert_eq!(t.cursor(), (Row::from_index(2), Col::at_index(9)));
        let blank = t.row(Row::from_index(2)).expect("a row")[9];
        assert_eq!(
            t.style_of(blank),
            StyleWord::DEFAULT,
            "all attributes turned off"
        );
    });
}

#[test]
fn decbi_at_the_left_margin_moves_the_rectangle_right() {
    let bytes = format!("{}{MARGINS}\x1b[1;31m\x1b[3;5H\x1b6", fill());
    support::render(&WS, bytes.as_bytes(), |t| {
        assert_eq!(
            row(t, 3),
            "0123.45678abcdefghij",
            "column 10 gone, a blank at 5"
        );
        assert_eq!(t.cursor(), (Row::from_index(2), Col::at_index(4)));
        let blank = t.row(Row::from_index(2)).expect("a row")[4];
        assert_eq!(
            t.style_of(blank),
            StyleWord::DEFAULT,
            "not the current rendition"
        );
    });
}

/// Inside the margins, and outside them, it is a move of one column.
#[test]
fn decfi_and_decbi_elsewhere_move_one_column() {
    let bytes = format!("{}{MARGINS}\x1b[3;7H\x1b9", fill());
    support::render(&WS, bytes.as_bytes(), |t| {
        assert_eq!(row(t, 3), "0123456789abcdefghij");
        assert_eq!(t.cursor(), (Row::from_index(2), Col::at_index(7)));
    });
    let bytes = format!("{}{MARGINS}\x1b[3;2H\x1b6\x1b[3;15H\x1b9", fill());
    support::render(&WS, bytes.as_bytes(), |t| {
        assert_eq!(row(t, 3), "0123456789abcdefghij");
        assert_eq!(t.cursor(), (Row::from_index(2), Col::at_index(15)));
    });
}

/// **At the margin column from a row outside the top and bottom margins the
/// rectangle still moves**: the algorithm tests the column alone and the
/// table marks both controls "Not affected" by those margins. xterm moves
/// nothing from there (`xtermColScroll`).
#[test]
fn decfi_at_the_margin_column_outside_the_region_still_moves_it() {
    let bytes = format!("{}{MARGINS}\x1b[?6l\x1b[7;10H\x1b9", fill());
    support::render(&WS, bytes.as_bytes(), |t| {
        assert_eq!(row(t, 3), "012356789.abcdefghij");
        assert_eq!(
            row(t, 7),
            "0123456789abcdefghij",
            "the cursor's row is outside it"
        );
    });
}

/// "If the active position was at the right edge of the page, the command
/// is ignored", and DECBI at the left edge likewise.
#[test]
fn decfi_and_decbi_at_the_page_edge_do_nothing() {
    let bytes = format!("{}{MARGINS}\x1b[3;20H\x1b9", fill());
    support::render(&WS, bytes.as_bytes(), |t| {
        assert_eq!(row(t, 3), "0123456789abcdefghij");
        assert_eq!(t.cursor(), (Row::from_index(2), Col::at_index(19)));
    });
    let bytes = format!("{}{MARGINS}\x1b[3;1H\x1b6", fill());
    support::render(&WS, bytes.as_bytes(), |t| {
        assert_eq!(t.cursor(), (Row::from_index(2), Col::at_index(0)));
        assert_eq!(t.unsupported, 0);
    });
}

/// With no margins set they are the page's: DECFI in the last column moves
/// the whole screen left, a pending wrap included.
#[test]
fn decfi_with_a_pending_wrap_is_at_the_right_margin() {
    let bytes = format!("{}\x1b[3;1H0123456789abcdefghij\x1b9", fill());
    support::render(&WS, bytes.as_bytes(), |t| {
        assert_eq!(row(t, 3), "123456789abcdefghij.");
        assert_eq!(row(t, 1), "123456789abcdefghij.");
        assert_eq!(t.cursor(), (Row::from_index(2), Col::at_index(19)));
    });
}

/// **On a double-width line DECFI stops at the line's end**: the algorithm
/// moves the column on and then "active line may not be columns_per_page
/// long", `if (active_position.column > end_of_line(...)) active_position.column
/// = end_of_line(...)` (p. 5-38). It stepped into the half past the line,
/// where an insert under IRM then underflowed.
#[test]
fn decfi_on_a_double_width_line_stops_at_the_lines_end() {
    // From the last column it stays, with no wrap pending: the `X` lands
    // there, not on the next row.
    support::render(&WS, b"\x1b#6\x1b[1;10H\x1b9X", |t| {
        assert_eq!(&row(t, 1)[..10], ".........X");
        assert_eq!(&row(t, 2)[..10], "..........");
    });
    support::render(&WS, b"\x1b#6\x1b[1;9H\x1b9X", |t| {
        assert_eq!(&row(t, 1)[..10], ".........X");
    });
    support::render(&WS, b"\x1b#6abcdefghij\x1b[1;10H\x1b9\x1b[4hX", |t| {
        assert_eq!(&row(t, 1)[..10], "abcdefghiX");
    });
    // A second step from the line's end stays there too; an insert under
    // IRM after it underflowed.
    support::render(&WS, b"\x1b#6abcdefghij\x1b[1;10H\x1b9\x1b9\x1b[4hX", |t| {
        assert_eq!(&row(t, 1)[..10], "abcdefghiX");
    });
}
