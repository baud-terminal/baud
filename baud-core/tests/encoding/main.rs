//! Text decoding, as distinct from the control protocol.
//!
//! `tests/parser/` covers the bytes that are *not* text. This covers the bytes
//! that are: whether they decode, what happens when they do not, and how wide
//! the result is.

#[path = "../support/mod.rs"]
mod support;

mod graphemes;
mod latin1;
mod utf8;
mod width;

/// **One gap: the encoding never switches.** `ESC % @` selects ISO 8859-1 and
/// `ESC % G` UTF-8, and `baud-core` routes neither -- `Term::encoding` is
/// written nowhere, so the whole Latin-1 half is unreachable, 8-bit C1
/// controls included. The `Act::Latin1` row exists and nothing can reach it.
///
/// **The other one is closed, and it was worse than it was recorded as.** The
/// successor-byte defect was pinned here as "`Act::Char2` takes two bytes on
/// the strength of the lead alone" -- true, and the cause was one line further
/// down: `run_of`'s fast gate answered `Count::of(1)` without looking past the
/// lead, because the *graph* only looks at the lead and the scan is the thing
/// that was supposed to check. So `put_one` was handed an ill-formed sequence
/// and consumed every byte of it: `E2 41 61` swallowed the `A` and the `a`,
/// against `[[source: unicode-ch3]]` 3.9.5's "must not consume the successor
/// bytes".
///
/// **And the same line was an unbounded loop.** Where the scalar scan *did*
/// run -- a lead followed by another lead -- it answers zero, `chars` consumed
/// `len * 0`, and `step` had already moved the node to `Ground`; the byte was
/// looked up again, answered `Char3` again, and scanned to zero again. Nothing
/// bounded it. A pty delivers whatever the far end sends, so that is a hang on
/// hostile input, and what found it was `ansi`'s `acsc` capability -- a
/// hundred bytes of raw high bytes, which is what an alternate-character-set
/// map is.
///
/// Both are fixed by emitting one `U+FFFD` per **maximal subpart** and
/// re-dispatching the rest, checked against `baud-core2` rather than against a
/// reading of the clause by `malformed_utf8_renders_the_way_core2_renders_it`.
#[test]
fn the_known_encoding_gaps() {
    support::ignores_are(
        "encoding",
        &["latin1.rs"],
        &[
            "decrqm_1020_follows_the_live_encoding",
            "latin1_c1_bytes_are_controls_not_characters",
            "the_same_bytes_decode_differently_per_encoding",
            "latin1_has_no_ill_formed_bytes",
            "eight_bit_csi_works_in_latin1_and_not_in_utf8",
            "eight_bit_string_terminators_are_honoured",
            "eight_bit_osc_and_dcs_work_in_latin1",
            "the_encoding_switches_both_ways",
        ],
    );
}
