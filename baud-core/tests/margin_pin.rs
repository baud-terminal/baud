//! With `DECAWM` reset, a character written into the last column leaves the
//! cursor **on** that column, and what comes next has to know it.
//!
//! `[[source: vt510-decawm]]`: "If the DECAWM function is reset, then graphic
//! characters received when the cursor is at the right border of the page
//! replace characters already on the page." `Paint::advance` holds it by
//! leaving the cursor on the last column, with no pending-wrap sentinel, so
//! the next character lands there. Two paths did not
//! hold that, and the `decawm-reset` atom in `tests/properties.rs` found both
//! at once, shrinking to `?7l`, nineteen letters, `e U+0301` on twenty
//! columns:
//!
//! - **A continuation joins the character before the cursor**, `View::lead`
//!   of `cx - 1`. On a pinned cursor that is the column before the character
//!   just written, so `e U+0301` in the last column put the accent on the
//!   letter left of the `e`, and a `U+FE0F` after `☠ a` widened the `☠` over
//!   the `a`. `[[source: uax29]]` puts the mark in the `e`'s cluster.
//! - **A run that fits left the cursor past the margin.** `Paint::blit` and
//!   `Paint::run` set the cursor to the run's end, so a run ending in the last
//!   column put it one past, the sentinel `advance` exists to refuse, and one
//!   character at a time did not: `ICH` then inserted nowhere. Whichever path
//!   the bytes took decided which column the next write and the accent used,
//!   so the screen depended on where the reads fell.
//!
//! Every case is asserted at every read size. The accent cases have no
//! ISO 8859-1 arm, because that set has no combining character; the `ICH`
//! cases run under both.

mod support;

use baud_core::*;

const EIGHT: WinSize = WinSize {
    ws_row: 4,
    ws_col: 8,
};

/// One cell as a character: `.` for a blank and `]` for a wide tail.
fn glyph(c: PackedCell) -> char {
    if c.content.is_wide_tail() {
        return ']';
    }
    match c.content.as_codepoint() {
        Some(cp) => char::from_u32(cp.value()).unwrap_or('?'),
        None => '.',
    }
}

/// Row 0 and the marks on its last two columns, fed in reads of `read`.
fn seen(bytes: &[u8], read: ReadLen) -> (String, Vec<u32>, Vec<u32>) {
    support::with_term(&EIGHT, |t| {
        for piece in bytes.chunks(read.index()) {
            t.write_at(BytePos::ZERO, piece);
            t.feed(BytePos::ZERO, ReadLen::from_index(piece.len()));
        }
        let row = t.row(Row::ZERO)[..8].iter().map(|&c| glyph(c)).collect();
        let before = t.marks(Row::ZERO, Col::at_index(6)).collect();
        let last = t.marks(Row::ZERO, Col::at_index(7)).collect();
        (row, before, last)
    })
}

/// `bytes` shows `want` at every read size; each element of `arms` is one
/// encoding's spelling.
fn at_every_read_size(arms: &[(&str, &[u8])], want: (&str, &[u32], &[u32])) {
    let mut wrong = Vec::new();
    for &(enc, bytes) in arms {
        if let Some((read, got)) = (1..=bytes.len())
            .map(ReadLen::from_index)
            .map(|read| (read, seen(bytes, read)))
            .find(|(_, got)| (got.0.as_str(), got.1.as_slice(), got.2.as_slice()) != want)
        {
            wrong.push(format!(
                "{enc}, {}-byte reads of {bytes:02x?}: {got:x?}, not {want:x?}",
                read.index()
            ));
        }
    }
    assert!(wrong.is_empty(), "{wrong:#?}");
}

/// The atom's own shape on eight columns, with the base written three ways:
/// an ASCII letter after a run, a two-byte letter, and the end of a run that
/// fills the row, which `blit` writes whole when the read holds it.
#[test]
fn a_mark_after_a_letter_in_the_last_column_joins_that_letter() {
    for (base, want) in [("e", "xxxxxxxe"), ("\u{e9}", "xxxxxxx\u{e9}")] {
        let bytes = format!("\x1b[?7lxxxxxxx{base}\u{301}");
        at_every_read_size(&[("UTF-8", bytes.as_bytes())], (want, &[], &[0x301]));
    }
}

/// `U+FE0F` after `☠ a` in the last two columns belongs to the `a`, which
/// no selector widens: the `☠` keeps one cell and the `a` is not
/// overwritten.
#[test]
fn a_selector_after_a_letter_in_the_last_column_leaves_the_character_before_it_alone() {
    let bytes = "\x1b[?7lxxxxxx\u{2620}a\u{fe0f}";
    at_every_read_size(
        &[("UTF-8", bytes.as_bytes())],
        ("xxxxxx\u{2620}a", &[], &[0xfe0f]),
    );
}

/// A run that ends in the last column leaves the cursor on it, so `ICH`
/// inserts a blank there and the last character goes. The ASCII run is
/// `blit`'s, under both encodings.
#[test]
fn ich_after_an_ascii_run_that_ends_in_the_last_column_inserts_in_that_column() {
    at_every_read_size(
        &[
            ("UTF-8", b"\x1b[?7labcdefgh\x1b[@"),
            ("ISO 8859-1", b"\x1b%@\x1b[?7labcdefgh\x1b[@"),
        ],
        ("abcdefg.", &[], &[]),
    );
}

/// The box-drawing run is `put_narrow_run`'s. Under ISO 8859-1 each `é` is
/// one character, through `advance`, which was right already: the control.
#[test]
fn ich_after_a_narrow_run_that_ends_in_the_last_column_inserts_in_that_column() {
    let boxes = "\x1b[?7l\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\x1b[@";
    at_every_read_size(
        &[("UTF-8", boxes.as_bytes())],
        (
            "\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}.",
            &[],
            &[],
        ),
    );
    at_every_read_size(
        &[(
            "ISO 8859-1",
            b"\x1b%@\x1b[?7l\xe9\xe9\xe9\xe9\xe9\xe9\xe9\xe9\x1b[@",
        )],
        ("\u{e9}\u{e9}\u{e9}\u{e9}\u{e9}\u{e9}\u{e9}.", &[], &[]),
    );
}

/// Four wide characters are `put_wide_run`'s; they fill the row and leave
/// the cursor on the last one's tail, and `ICH` there cuts the character,
/// so both halves go.
#[test]
fn ich_after_a_wide_run_that_ends_in_the_last_column_inserts_in_that_column() {
    let wide = "\x1b[?7l\u{4e16}\u{4e16}\u{4e16}\u{4e16}\x1b[@";
    at_every_read_size(
        &[("UTF-8", wide.as_bytes())],
        ("\u{4e16}]\u{4e16}]\u{4e16}]..", &[], &[]),
    );
}

/// Every row's cells, and each cell that carries marks with its marks.
type Screen = (Vec<String>, Vec<(usize, usize, Vec<u32>)>);

/// Every row's cells and where the marks are, after `bytes` on `from`
/// in reads of `read`, a resize to `to` and then `after` in one read.
fn across_a_resize(
    from: &WinSize,
    to: &WinSize,
    bytes: &[u8],
    read: ReadLen,
    after: &[u8],
) -> Screen {
    let room = WinSize {
        ws_row: from.ws_row.max(to.ws_row),
        ws_col: from.ws_col.max(to.ws_col),
    };
    let mut owned = Owned::new(&room);
    let mut t = Term::new(owned.storage(), from);
    for piece in bytes.chunks(read.index()) {
        t.write_at(BytePos::ZERO, piece);
        t.feed(BytePos::ZERO, ReadLen::from_index(piece.len()));
    }
    assert_eq!(t.resize(to), Resized::Ok, "the resize took");
    t.write_at(BytePos::ZERO, after);
    t.feed(BytePos::ZERO, ReadLen::from_index(after.len()));
    let (rows, cols) = (usize::from(to.ws_row), usize::from(to.ws_col));
    let text = (0..rows)
        .map(|y| {
            t.row(Row::from_index(y))[..cols]
                .iter()
                .map(|&c| glyph(c))
                .collect()
        })
        .collect();
    let marks = (0..rows)
        .flat_map(|y| (0..cols).map(move |x| (y, x)))
        .map(|(y, x)| {
            let m: Vec<u32> = t.marks(Row::from_index(y), Col::at_index(x)).collect();
            (y, x, m)
        })
        .filter(|(_, _, m)| !m.is_empty())
        .collect();
    (text, marks)
}

/// **A resize moves the margin and not the cursor's column**, so the pin is
/// still the last write's. `e` in the last of eight columns, the screen
/// widened to ten, then `U+0301`: the cluster machine joined the mark to the
/// `e`, and it goes on the `e`'s cell, column 7, not the `x` before it. A
/// test that the cursor sits on the right margin as well as on a pin put it
/// on column 6 once the margin had moved to 9.
#[test]
fn a_mark_after_a_resize_wider_still_joins_the_letter_written_last() {
    const TEN: WinSize = WinSize {
        ws_row: 4,
        ws_col: 10,
    };
    let bytes = b"\x1b[?7lxxxxxxxe";
    let want_row = "xxxxxxxe..";
    for read in (1..=bytes.len()).map(ReadLen::from_index) {
        let (text, marks) = across_a_resize(&EIGHT, &TEN, bytes, read, "\u{301}".as_bytes());
        assert_eq!(text[0], want_row, "{}-byte reads", read.index());
        assert_eq!(
            marks,
            [(0, 7, vec![0x301])],
            "{}-byte reads: the mark is on the e",
            read.index()
        );
    }
}

/// Narrower, the line reflows and the cursor goes with the `e` it was on,
/// to column 1 of the next row: the mark goes there too.
#[test]
fn a_mark_after_a_resize_narrower_joins_the_letter_written_last() {
    const TEN: WinSize = WinSize {
        ws_row: 4,
        ws_col: 10,
    };
    let bytes = b"\x1b[?7lxxxxxxxxxe";
    for read in (1..=bytes.len()).map(ReadLen::from_index) {
        let (text, marks) = across_a_resize(&TEN, &EIGHT, bytes, read, "\u{301}".as_bytes());
        assert_eq!(
            text[..2],
            ["xxxxxxxx", "xe......"],
            "{}-byte reads",
            read.index()
        );
        assert_eq!(
            marks,
            [(1, 1, vec![0x301])],
            "{}-byte reads: the mark is on the e",
            read.index()
        );
    }
}
