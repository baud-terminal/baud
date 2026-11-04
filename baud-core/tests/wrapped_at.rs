//! **Where a row wrapped**: at the page's edge, or at DECSLRM side
//! margins narrower than the page (`[[source: vt510-decslrm]]`), where the
//! next line starts at the left margin and the row's cells outside the
//! margins belong to no paragraph of its. The row keeps the fact until it
//! wraps again at the page's edge or is erased whole ([`WrappedAt`]), so
//! the display's word wrap can keep such a paragraph at the terminal's
//! cut, and keeps the margins while it wraps at them, moving them with its
//! cells. Every case at every read size and every two-read split.
//!
//! **Only a wrap at the screen's edge marks the row as continuing**, since
//! 2026-11-03, as ghostty marks only a wrap at its last column: "We don't
//! mark the row as wrapped if we're in the middle due to
//! a right margin" (`printWrap`, `[[source: ghostty-d9840f3-terminal]]`
//! 1758-1763). A program that wraps its text at its own right margin, or
//! at SLL's line limit, inside the screen gets rows that end there, so a
//! reflow does not join them when the window widens; margins that end at
//! the screen's edge still join. Under DECRLM the edge is the left one.

use baud_core::*;
use baud_domain::{Wrap, WrappedAt};

const TWENTY: WinSize = WinSize {
    ws_row: 4,
    ws_col: 20,
};

/// Margins 4 to 20, inside the screen on the left and at its edge on the
/// right, and a run from column 4 that wraps at column 20 onto row 1.
const WROTE: &str = "\x1b[?69h\x1b[4;20s\x1b[1;4Habcdefghijklmnopqr";

/// Where rows 0 and 1 wrapped and whether each wraps, after `bytes` read
/// `read` bytes at a time.
fn wrapped(bytes: &[u8], read: usize) -> [(WrappedAt, Wrap); 2] {
    let pieces: Vec<&[u8]> = bytes.chunks(read).collect();
    wrapped_in(&pieces)
}

/// The same after `pieces`, each a read.
fn wrapped_in(pieces: &[&[u8]]) -> [(WrappedAt, Wrap); 2] {
    let mut owned = Owned::new(&TWENTY);
    let mut t = Term::new(owned.storage(), &TWENTY);
    for piece in pieces {
        t.write_at(BytePos::ZERO, piece);
        t.feed(BytePos::ZERO, ReadLen::from_index(piece.len()));
    }
    let vp = t.view(Rows::ZERO);
    [0, 1].map(|y| {
        let r = Row::from_index(y);
        (vp.bidi(r).wrapped_at(), vp.wrapped(r))
    })
}

/// `want` at every read size and at every split into two reads.
fn at_every_read_size(bytes: &[u8], want: [(WrappedAt, Wrap); 2]) {
    for read in 1..=bytes.len() {
        assert_eq!(
            wrapped(bytes, read),
            want,
            "{read}-byte reads of {:?}",
            String::from_utf8_lossy(bytes)
        );
    }
    for at in 1..bytes.len() {
        assert_eq!(
            wrapped_in(&[&bytes[..at], &bytes[at..]]),
            want,
            "{:?} split at {at}",
            String::from_utf8_lossy(bytes)
        );
    }
}

/// **A wrap at a right margin inside the screen leaves no mark**: columns
/// 4 to 12, and 1 to 12, a run past the right margin continuing at the
/// left margin of row 1, which ends where the program's margin cut it.
#[test]
fn a_wrap_at_a_right_margin_inside_the_screen_leaves_no_mark() {
    for bytes in [
        &b"\x1b[?69h\x1b[4;12s\x1b[1;4Habcdefghijk"[..],
        b"\x1b[?69h\x1b[1;12s\x1b[1;1Habcdefghijklmn",
    ] {
        at_every_read_size(
            bytes,
            [
                (WrappedAt::Edge, Wrap::Ended),
                (WrappedAt::Edge, Wrap::Ended),
            ],
        );
    }
}

/// **A wrap at side margins that end at the screen's edge marks the row it
/// leaves**, with the margins: columns 4 to 20.
#[test]
fn a_wrap_at_margins_ending_at_the_screen_edge_marks_the_row_it_leaves() {
    at_every_read_size(
        WROTE.as_bytes(),
        [
            (WrappedAt::Margins, Wrap::Continues),
            (WrappedAt::Edge, Wrap::Ended),
        ],
    );
}

/// **Text right of a narrow right margin wraps at the screen's edge, and
/// marks the row**: from column 15, right of margins 4 to 12, to column 20
/// and on at the left margin.
#[test]
fn a_wrap_right_of_the_margin_at_the_screen_edge_marks_the_row() {
    at_every_read_size(
        b"\x1b[?69h\x1b[4;12s\x1b[1;15Habcdefgh",
        [
            (WrappedAt::Margins, Wrap::Continues),
            (WrappedAt::Edge, Wrap::Ended),
        ],
    );
}

/// **SLL's line limit cuts as a margin does**: a wrap at a limit inside
/// the screen leaves no mark, and one at the last column marks the row.
#[test]
fn a_wrap_at_a_line_limit_inside_the_screen_leaves_no_mark() {
    at_every_read_size(
        b"\x1b[12 Vabcdefghijklmn",
        [
            (WrappedAt::Edge, Wrap::Ended),
            (WrappedAt::Edge, Wrap::Ended),
        ],
    );
    at_every_read_size(
        format!("\x1b[20 V{}", "x".repeat(23)).as_bytes(),
        [
            (WrappedAt::Edge, Wrap::Continues),
            (WrappedAt::Edge, Wrap::Ended),
        ],
    );
}

/// **A double-width line's edge is its last column**, the tenth of
/// twenty: a wrap there marks the row, and so does one under a line limit
/// past the line, which the line's last column bounds.
#[test]
fn a_wrap_at_a_double_width_line_end_marks_the_row() {
    for bytes in [&b"\x1b#6abcdefghijkl"[..], b"\x1b#6\x1b[20 Vabcdefghijkl"] {
        at_every_read_size(
            bytes,
            [
                (WrappedAt::Edge, Wrap::Continues),
                (WrappedAt::Edge, Wrap::Ended),
            ],
        );
    }
}

/// **Under DECRLM the edge is the screen's left one**: a run written
/// leftward from column 20 wraps at column 1 and marks the row; one that
/// wraps at a left margin, column 5, leaves no mark.
#[test]
fn under_decrlm_only_a_wrap_at_the_left_edge_marks_the_row() {
    at_every_read_size(
        format!("\x1b[?34h\x1b[1;20H{}", "x".repeat(23)).as_bytes(),
        [
            (WrappedAt::Edge, Wrap::Continues),
            (WrappedAt::Edge, Wrap::Ended),
        ],
    );
    at_every_read_size(
        format!("\x1b[?69h\x1b[5;20s\x1b[?34h\x1b[1;20H{}", "x".repeat(18)).as_bytes(),
        [
            (WrappedAt::Edge, Wrap::Ended),
            (WrappedAt::Edge, Wrap::Ended),
        ],
    );
}

/// **A wrap at the page's edge does not**, with DECLRMM set and the
/// margins at the page's borders too: the control.
#[test]
fn a_wrap_at_the_page_edge_leaves_no_mark() {
    let run = "x".repeat(23);
    for set in ["", "\x1b[?69h"] {
        at_every_read_size(
            format!("{set}{run}").as_bytes(),
            [
                (WrappedAt::Edge, Wrap::Continues),
                (WrappedAt::Edge, Wrap::Ended),
            ],
        );
    }
}

/// **An erase of the row whole clears the mark**: an `EL 2` over the
/// row clears it, with its wrap; an `EL 0` from column 1 does too, an
/// `EL 0` from column 2 does not.
#[test]
fn erasing_the_row_whole_clears_the_mark() {
    let wrote = format!("{WROTE}\x1b[?69l");
    at_every_read_size(
        format!("{wrote}\x1b[1;5H\x1b[2K").as_bytes(),
        [
            (WrappedAt::Edge, Wrap::Ended),
            (WrappedAt::Edge, Wrap::Ended),
        ],
    );
    at_every_read_size(
        format!("{wrote}\x1b[1;1H\x1b[K").as_bytes(),
        [
            (WrappedAt::Edge, Wrap::Ended),
            (WrappedAt::Edge, Wrap::Ended),
        ],
    );
    let kept = wrapped(format!("{wrote}\x1b[1;2H\x1b[K").as_bytes(), 4096);
    assert_eq!(kept[0].0, WrappedAt::Margins, "a partial erase keeps it");
}

/// The side margins rows 0 and 1 of the window `back` lines up wrapped at,
/// after `bytes` in one read: left and right, zero-based.
fn margins_after(bytes: &[u8], back: usize) -> [Option<(usize, usize)>; 2] {
    let mut owned = Owned::new(&TWENTY);
    let mut t = Term::new(owned.storage(), &TWENTY);
    t.write_at(BytePos::ZERO, bytes);
    t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
    let vp = t.view(Rows::of_count(back));
    [0, 1].map(|y| {
        vp.margins(Row::from_index(y))
            .map(|m| (m.left().index(), m.right().index()))
    })
}

/// **The row keeps the margins it wrapped at**: columns 4 to 20, zero-based
/// 3 to 19, beside the mark, at every read size; a row that wrapped at the
/// page's edge holds none, the control.
#[test]
fn a_wrap_at_side_margins_keeps_the_margins() {
    let bytes = WROTE.as_bytes();
    for read in 1..=bytes.len() {
        let mut owned = Owned::new(&TWENTY);
        let mut t = Term::new(owned.storage(), &TWENTY);
        for piece in bytes.chunks(read) {
            t.write_at(BytePos::ZERO, piece);
            t.feed(BytePos::ZERO, ReadLen::from_index(piece.len()));
        }
        let m = t.view(Rows::ZERO).margins(Row::ZERO).expect("recorded");
        assert_eq!((m.left().index(), m.right().index()), (3, 19), "{read}");
    }
    assert_eq!(
        margins_after("x".repeat(23).as_bytes(), 0),
        [None, None],
        "the control: the page's edge"
    );
}

/// **The margins move with the row**: an `IL` above it carries them a row
/// down, and a scroll into the history carries them there, read one line
/// up.
#[test]
fn the_margins_move_with_their_row() {
    let wrote = format!("{WROTE}\x1b[?69l");
    assert_eq!(
        margins_after(format!("{wrote}\x1b[1;1H\x1b[L").as_bytes(), 0),
        [None, Some((3, 19))],
        "IL above it"
    );
    assert_eq!(
        margins_after(format!("{wrote}\x1b[4;1H\n").as_bytes(), 1),
        [Some((3, 19)), None],
        "scrolled into the history, one line up"
    );
}

/// **A later wrap at other margins writes its own**, and an erase of the
/// row whole leaves none.
#[test]
fn a_later_wrap_writes_its_margins_and_an_erase_drops_them() {
    let wrote = WROTE;
    assert_eq!(
        margins_after(format!("{wrote}\x1b[2;20s\x1b[1;18Hmnopq").as_bytes(), 0),
        [Some((1, 19)), None],
        "the newer margins"
    );
    assert_eq!(
        margins_after(format!("{wrote}\x1b[1;5H\x1b[2K").as_bytes(), 0),
        [None, None],
        "erased whole"
    );
}

/// Each row's margins and wrap, after `bytes` read `read` bytes at a
/// time.
fn rows_after(bytes: &[u8], read: usize) -> [(Option<(usize, usize)>, Wrap); 4] {
    let mut owned = Owned::new(&TWENTY);
    let mut t = Term::new(owned.storage(), &TWENTY);
    for piece in bytes.chunks(read) {
        t.write_at(BytePos::ZERO, piece);
        t.feed(BytePos::ZERO, ReadLen::from_index(piece.len()));
    }
    let vp = t.view(Rows::ZERO);
    [0, 1, 2, 3].map(|y| {
        let r = Row::from_index(y);
        (
            vp.margins(r).map(|m| (m.left().index(), m.right().index())),
            vp.wrapped(r),
        )
    })
}

fn rows_at_every_read_size(bytes: &str, want: [(Option<(usize, usize)>, Wrap); 4]) {
    for read in 1..=bytes.len() {
        assert_eq!(
            rows_after(bytes.as_bytes(), read),
            want,
            "{read}-byte reads of {bytes:?}"
        );
    }
}

/// **A later wrap at the page's edge drops the margins**: after `?69l`,
/// or with DECLRMM set and the margins at the page's borders, the row
/// wraps where the page ends and keeps no margins.
#[test]
fn a_later_wrap_at_the_page_edge_drops_the_margins() {
    let wrote = WROTE;
    for leave in ["\x1b[?69l", "\x1b[s"] {
        let bytes = format!("{wrote}{leave}\x1b[1;1H{}", "x".repeat(23));
        at_every_read_size(
            bytes.as_bytes(),
            [
                (WrappedAt::Edge, Wrap::Continues),
                (WrappedAt::Edge, Wrap::Ended),
            ],
        );
        assert_eq!(rows_after(bytes.as_bytes(), 4096)[0].0, None, "{leave:?}");
    }
}

/// **A row that no longer wraps keeps no margins to read**: a newline that
/// ends it, or an erase to its end.
#[test]
fn a_row_that_no_longer_wraps_has_no_margins() {
    let wrote = format!("{WROTE}\x1b[?69l");
    for then in ["\x1b[1;1H\r\n", "\x1b[1;2H\x1b[K"] {
        let rows = rows_after(format!("{wrote}{then}").as_bytes(), 4096);
        assert_eq!(rows[0], (None, Wrap::Ended), "{then:?}");
    }
}

/// **A wrap at the bottom margin carries the margins up with the
/// rectangle**: the row the wrap leaves holds the margins and continues
/// once it has scrolled a row up, and the row the rectangle opened, where
/// "r" lands, holds none.
#[test]
fn a_wrap_at_the_bottom_margin_carries_the_margins_up() {
    rows_at_every_read_size(
        "\x1b[?69h\x1b[4;20s\x1b[4;4Habcdefghijklmnopqr",
        [
            (None, Wrap::Ended),
            (None, Wrap::Ended),
            (Some((3, 19)), Wrap::Continues),
            (None, Wrap::Ended),
        ],
    );
}

/// **`IL` and `DL` inside the rectangle move the margins with its cells**:
/// an `IL` carries row 1's a row down and leaves none on the row it opens;
/// a `DL` brings row 2's up, which has none.
#[test]
fn il_and_dl_inside_the_rectangle_move_the_margins() {
    let wrote = WROTE;
    rows_at_every_read_size(
        &format!("{wrote}\x1b[1;4H\x1b[L"),
        [
            (None, Wrap::Ended),
            (Some((3, 19)), Wrap::Continues),
            (None, Wrap::Ended),
            (None, Wrap::Ended),
        ],
    );
    rows_at_every_read_size(
        &format!("{wrote}\x1b[1;4H\x1b[M"),
        [
            (None, Wrap::Ended),
            (None, Wrap::Ended),
            (None, Wrap::Ended),
            (None, Wrap::Ended),
        ],
    );
}
