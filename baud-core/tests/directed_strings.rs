//! **SDS and SRS, as the core keeps them**: `CSI Ps ]` and `CSI Ps [`
//! (`[[source: ecma48]]` 8.3.114, 8.3.137), each character written in a
//! string carrying it in its rendition, each string's entry naming the
//! string around it. What they draw is `baud-render/tests/strings.rs`.

mod support;

use baud_core::*;
use baud_domain::{StringEntry, StringId, StringKind};

const WS: WinSize = WinSize {
    ws_row: 4,
    ws_col: 12,
};

/// The string the cell at `(row, col)` was written in.
fn string_at(t: &Term<'_>, row: usize, col: usize) -> StringId {
    let cell = t.viewport().row(Row::from_index(row))[col];
    t.styles().one().slots()[cell.style.index()]
        .word()
        .map(|w| w.string())
        .unwrap_or(StringId::NONE)
}

/// The kinds of the chain of strings at `(row, col)`, innermost first.
fn kinds_at(t: &Term<'_>, row: usize, col: usize) -> Vec<StringKind> {
    let mut out = Vec::new();
    let mut s = string_at(t, row, col);
    while let Some(StringEntry { parent, kind }) = t.string_entry(s) {
        out.push(kind);
        s = parent;
    }
    out
}

/// **Each character takes the string it is written in**, SDS 1 and 2 and
/// SRS 1 each beginning one inside the last, each end putting back the
/// one around it; the text before and after is in none. Until now each
/// was counted and changed nothing.
#[test]
fn each_character_takes_the_string_it_is_written_in() {
    support::with_term(&WS, |t| {
        let refused = t.strings_refused();
        support::feed(t, b"a\x1b[2]b\x1b[1[c\x1b[1]d\x1b[0]e\x1b[0[f\x1b[0]g");
        assert_eq!(t.unsupported, 0);
        assert!(t.strings_seen());
        assert_eq!(kinds_at(t, 0, 0), []);
        assert_eq!(kinds_at(t, 0, 1), [StringKind::RightToLeft]);
        assert_eq!(
            kinds_at(t, 0, 2),
            [StringKind::Reversed, StringKind::RightToLeft]
        );
        assert_eq!(
            kinds_at(t, 0, 3),
            [
                StringKind::LeftToRight,
                StringKind::Reversed,
                StringKind::RightToLeft
            ]
        );
        assert_eq!(kinds_at(t, 0, 4), kinds_at(t, 0, 2));
        assert_eq!(kinds_at(t, 0, 5), [StringKind::RightToLeft]);
        assert_eq!(kinds_at(t, 0, 6), []);
        assert_eq!(t.strings_refused(), refused);
    });
}

/// **Two strings one after the other are two strings**: each SDS begins a
/// new one, so adjacent strings of one direction stay apart.
#[test]
fn adjacent_strings_are_apart() {
    support::with_term(&WS, |t| {
        support::feed(t, b"\x1b[2]ab\x1b[0]\x1b[2]cd\x1b[0]");
        assert_eq!(string_at(t, 0, 0), string_at(t, 0, 1));
        assert_ne!(string_at(t, 0, 1), string_at(t, 0, 2));
        assert_eq!(string_at(t, 0, 2), string_at(t, 0, 3));
    });
}

/// **An end ends only a string of its kind**: SRS 0 inside a directed
/// string ends nothing and is counted, as is an end with nothing open;
/// SDS's `Ps` 3 and SRS's 2 are unsupported.
#[test]
fn a_mismatched_or_stray_end_ends_nothing_and_is_counted() {
    support::with_term(&WS, |t| {
        let none = t.strings_refused();
        support::feed(t, b"\x1b[0]\x1b[0[");
        let two = t.strings_refused();
        assert_ne!(two, none);
        support::feed(t, b"\x1b[2]a\x1b[0[b");
        assert_eq!(kinds_at(t, 0, 1), [StringKind::RightToLeft], "still open");
        assert_ne!(t.strings_refused(), two);
        support::feed(t, b"\x1b[0]c\x1b[3]\x1b[2[");
        assert_eq!(kinds_at(t, 0, 2), []);
        assert_eq!(t.unsupported, 2);
    });
}

/// **`SGR 0` leaves the string, `DECSC` and `DECRC` keep the one open**,
/// and `DECSTR` ends it, the characters written keeping theirs.
#[test]
fn sgr_0_and_decrc_keep_the_string_and_decstr_ends_it() {
    support::with_term(&WS, |t| {
        support::feed(t, b"\x1b7\x1b[2]\x1b[31ma\x1b[0mb\x1b8c");
        assert_eq!(kinds_at(t, 0, 1), [StringKind::RightToLeft], "SGR 0");
        assert_eq!(kinds_at(t, 0, 0), [StringKind::RightToLeft], "DECRC");
        support::feed(t, b"\x1b[2;1H\x1b[!pd");
        assert_eq!(kinds_at(t, 1, 0), [], "DECSTR");
        assert_eq!(kinds_at(t, 0, 0), [StringKind::RightToLeft]);
    });
}

/// **An erased position stays in its string**: `ECH` inside a reversed
/// string leaves the blank in it, so nothing it presents moves; `ICH`'s
/// blanks join the string of the character they push on (TR/53 note A.1).
#[test]
fn an_erased_or_inserted_position_stays_in_the_string() {
    support::with_term(&WS, |t| {
        support::feed(t, b"x\x1b[1[abc\x1b[0[y\x1b[1;3H\x1b[X");
        assert_eq!(kinds_at(t, 0, 2), [StringKind::Reversed]);
        support::feed(t, b"\x1b[1;2H\x1b[@");
        assert_eq!(kinds_at(t, 0, 1), [StringKind::Reversed], "ICH");
        assert_eq!(kinds_at(t, 0, 2), [StringKind::Reversed], "a pushed on");
        assert_eq!(kinds_at(t, 0, 5), [], "y pushed on");
    });
}

/// **A string past UAX #9's depth is refused and counted**, its end
/// matched to it, so the end after it closes the deepest one taken.
#[test]
fn a_string_past_the_depth_is_refused_and_its_end_matched() {
    support::with_term(&WS, |t| {
        let open = "\x1b[1]".repeat(126);
        support::feed(t, open.as_bytes());
        let before = t.strings_refused();
        support::feed(t, b"a\x1b[0]b\x1b[0]c");
        assert_eq!(t.strings_refused(), before, "the refused one's end");
        assert_eq!(kinds_at(t, 0, 0).len(), 125);
        assert_eq!(kinds_at(t, 0, 1).len(), 125);
        assert_eq!(kinds_at(t, 0, 2).len(), 124);
    });
}

/// **The table frees the strings no cell names**: three hundred strings
/// one after another, each written over the last character, leave room
/// for the next.
#[test]
fn the_table_frees_strings_no_cell_names() {
    support::with_term(&WS, |t| {
        let refused = t.strings_refused();
        for _ in 0..300 {
            support::feed(t, b"\x1b[2]a\x1b[0]\x1b[1;1H");
        }
        assert_eq!(t.strings_refused(), refused);
        assert_eq!(kinds_at(t, 0, 0), [StringKind::RightToLeft]);
    });
}

/// **`RIS` ends every string and empties the table.**
#[test]
fn ris_ends_every_string() {
    support::with_term(&WS, |t| {
        support::feed(t, b"\x1b[2]a\x1bcb");
        assert_eq!(kinds_at(t, 0, 0), []);
        assert!(!t.strings_seen());
    });
}
