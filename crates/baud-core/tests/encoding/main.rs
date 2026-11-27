//! Text decoding, as distinct from the control protocol.
//!
//! `tests/parser/` covers the bytes that are *not* text. This covers the bytes
//! that are: whether they decode, what happens when they do not, and how wide
//! the result is.

#[path = "../support/mod.rs"]
mod support;

mod graphemes;
mod latin1;
mod maximal_subparts;
mod replacement_clusters;
mod utf8;
mod width;

/// **No gaps, and the gate stays so a new one has to say so.**
///
/// The encoding switches now: `ESC % @` selects ISO 8859-1 and `ESC % G`
/// UTF-8, the 8-bit C1 controls work under Latin-1 and not under UTF-8, and
/// `?1020` mirrors whichever is live rather than carrying a bit that could
/// disagree with the parser.
///
/// **Both tables have been compiled since the graph was written and neither
/// could be reached.** `Act::Latin1` appeared in no recording's action
/// histogram, so the row that answered `Act::Bad` for `80..9F` -- wrong, and
/// its own comment said so -- cost nothing and announced nothing for as long
/// as it stood. A row nothing can reach is a row nobody re-reads.
#[test]
fn the_known_encoding_gaps() {
    support::ignores_are(
        "encoding",
        &["latin1.rs", "maximal_subparts.rs", "utf8.rs"],
        &[],
    );
}
