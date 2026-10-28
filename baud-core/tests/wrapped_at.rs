//! **Where a row wrapped**: at the page's edge, or at DECSLRM side
//! margins narrower than the page (`[[source: vt510-decslrm]]`), where the
//! next line starts at the left margin and the row's cells outside the
//! margins belong to no paragraph of its. The row keeps the fact until it
//! is erased whole ([`WrappedAt`]), so the display's word wrap can keep
//! such a paragraph at the terminal's cut. Every case at every read size.

use baud_core::*;
use baud_domain::{Wrap, WrappedAt};

const TWENTY: WinSize = WinSize {
    ws_row: 4,
    ws_col: 20,
};

/// Where rows 0 and 1 wrapped and whether each wraps, after `bytes` read
/// `read` bytes at a time.
fn wrapped(bytes: &[u8], read: usize) -> [(WrappedAt, Wrap); 2] {
    let mut owned = Owned::new(&TWENTY);
    let mut t = Term::new(owned.storage(), &TWENTY);
    for piece in bytes.chunks(read) {
        t.write_at(BytePos::ZERO, piece);
        t.feed(BytePos::ZERO, ReadLen::from_index(piece.len()));
    }
    let vp = t.view(Rows::ZERO);
    [0, 1].map(|y| {
        let r = Row::from_index(y);
        (vp.bidi(r).wrapped_at(), vp.wrapped(r))
    })
}

fn at_every_read_size(bytes: &[u8], want: [(WrappedAt, Wrap); 2]) {
    for read in 1..=bytes.len() {
        assert_eq!(
            wrapped(bytes, read),
            want,
            "{read}-byte reads of {:?}",
            String::from_utf8_lossy(bytes)
        );
    }
}

/// **A wrap at side margins marks the row it leaves**: columns 4 to 12,
/// a run past the right margin continuing at the left margin of row 1.
#[test]
fn a_wrap_at_side_margins_marks_the_row_it_leaves() {
    at_every_read_size(
        b"\x1b[?69h\x1b[4;12s\x1b[1;4Habcdefghijk",
        [
            (WrappedAt::Margins, Wrap::Continues),
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

/// **The mark lasts until the row is erased whole**: an `EL 2` over the
/// row clears it, with its wrap; an `EL 0` from column 1 does too, an
/// `EL 0` from column 2 does not.
#[test]
fn erasing_the_row_whole_clears_the_mark() {
    let wrote = "\x1b[?69h\x1b[4;12s\x1b[1;4Habcdefghijk\x1b[?69l";
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
