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

/// Two gaps, and the second is the one that matters.
///
/// * **The encoding never switches.** `ESC % @` selects ISO 8859-1 and
///   `ESC % G` UTF-8, and `baud-core` routes neither -- `Term::encoding` is
///   written nowhere, so the whole Latin-1 half is unreachable, 8-bit C1
///   controls included. The `Act::Latin1` row exists and nothing can reach it.
/// * **The successor byte after a malformed sequence.** `Act::Char2` takes two
///   bytes on the strength of the lead alone, so `C2 71` consumes the `71`
///   that `[[source: unicode-ch3]]` § 3.9.5 requires be re-dispatched: "must
///   not consume the successor bytes". A real conformance bug, in the hot
///   decode path, and the same one `vt100`'s pin names.
#[test]
fn the_known_encoding_gaps() {
    support::ignores_are(
        "encoding",
        &["latin1.rs", "utf8.rs"],
        &[
            "decrqm_1020_follows_the_live_encoding",
            "latin1_c1_bytes_are_controls_not_characters",
            "the_same_bytes_decode_differently_per_encoding",
            "latin1_has_no_ill_formed_bytes",
            "eight_bit_csi_works_in_latin1_and_not_in_utf8",
            "eight_bit_string_terminators_are_honoured",
            "eight_bit_osc_and_dcs_work_in_latin1",
            "the_encoding_switches_both_ways",
            "a_truncated_lead_does_not_consume_a_valid_successor",
            "an_incomplete_sequence_at_the_end_renders_nothing_until_flushed",
            "recovery_resynchronises_on_other_byte_classes",
        ],
    );
}
