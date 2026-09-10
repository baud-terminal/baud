//! C0 control characters.
//!
//! The ones a terminal must not print and must not ignore. VT and FF were being
//! dropped entirely, so a stream containing them lost the line break; BEL was
//! dropped, which leaves an embedder no way to implement one.
//!
//! `[[source: ecma48]]` § 8.3 defines the C0 set; `[[source: ctlseqs]]` records
//! which of them xterm treats as an index.

use crate::support;
use baud_core::*;

fn term_of(bytes: &[u8]) -> (usize, usize, usize) {
    let ws = support::CORPUS;
    let mut s = Owned::new(&ws);
    let mut t = Term::new(s.storage(), &ws);
    support::feed(&mut t, bytes);
    let (r, c) = t.cursor();
    (r.index(), c.index(), t.bells)
}

fn cursor(bytes: &[u8]) -> (usize, usize) {
    let (r, c, _) = term_of(bytes);
    (r, c)
}

fn grid(bytes: &[u8]) -> Vec<PackedCell> {
    let ws = support::CORPUS;
    let mut s = Owned::new(&ws);
    let mut t = Term::new(s.storage(), &ws);
    support::feed(&mut t, bytes);
    t.viewport().to_vec()
}

/// NUL is a fill character with no effect: not printed, cursor unmoved.
#[test]
fn nul_is_discarded() {
    assert_eq!(
        cursor(b"ab\x00cd"),
        (0, 4),
        "NUL must not advance the cursor"
    );
    assert_eq!(
        grid(b"ab\x00cd"),
        grid(b"abcd"),
        "NUL must not occupy a cell"
    );
}

/// BEL does not print and does not move the cursor, but is observable.
#[test]
fn bel_is_counted_and_invisible() {
    let (r, c, bells) = term_of(b"ab\x07cd");
    assert_eq!((r, c), (0, 4), "BEL must not advance the cursor");
    assert_eq!(bells, 1, "BEL must be observable, not silently dropped");
    assert_eq!(
        grid(b"ab\x07cd"),
        grid(b"abcd"),
        "BEL must not occupy a cell"
    );
}

#[test]
fn bel_counts_accumulate() {
    assert_eq!(term_of(b"\x07\x07\x07").2, 3);
    assert_eq!(term_of(b"no bells here").2, 0);
}

/// VT and FF are an index: down one line, column preserved. Same as `ESC D`.
#[test]
fn vt_and_ff_are_an_index() {
    assert_eq!(cursor(b"abc\x0b"), (1, 3), "VT");
    assert_eq!(cursor(b"abc\x0c"), (1, 3), "FF");
    assert_eq!(cursor(b"abc\x0b"), cursor(b"abc\x1bD"), "VT must equal IND");
    assert_eq!(cursor(b"abc\x0c"), cursor(b"abc\x1bD"), "FF must equal IND");
}

/// They scroll at the region bottom, exactly as a line feed does.
#[test]
fn vt_scrolls_at_the_region_bottom() {
    assert_eq!(
        grid(b"\x1b[3;5r\x1b[5;1Hx\x0b"),
        grid(b"\x1b[3;5r\x1b[5;1Hx\x1bD")
    );
}

/// BS moves left and stops at column 0.
#[test]
fn bs_backs_up_and_stops_at_the_margin() {
    assert_eq!(cursor(b"abc\x08"), (0, 2));
    assert_eq!(cursor(b"\x08\x08\x08"), (0, 0));
}

/// CR returns to column 0 without changing the row.
#[test]
fn cr_returns_to_column_zero() {
    assert_eq!(cursor(b"\x1b[3;9Habc\r"), (2, 0));
}

/// A C0 byte baud does not act on is discarded, never printed. A terminal that
/// prints an unknown control renders mojibake into the user's screen.
#[test]
fn unhandled_c0_bytes_are_not_printed() {
    for b in [0x01u8, 0x02, 0x05, 0x06, 0x10, 0x11, 0x1a] {
        let bytes = [b'a', b, b'b'];
        assert_eq!(
            grid(&bytes),
            grid(b"ab"),
            "0x{b:02x} must be discarded, not printed"
        );
    }
}
