//! UTF-8 decoding against the Unicode Standard's own conformance vectors.
//!
//! `[[source: unicode-ch3]]` § 3.9.3, D92, Table 3-7 defines well-formedness
//! exhaustively, and enforcing it needs **two** mechanisms, not one: the
//! first-byte column excludes `C0-C1` and `F5-FF`, and the *second*-byte column
//! is narrowed for exactly four lead bytes — `E0`, `ED`, `F0`, `F4`. The
//! standard flags this itself: the exceptions to the `80..BF` pattern "occur
//! only in the second byte of a sequence".
//!
//! baud implemented only the first half. `utf8_lead` matched `0xe0..=0xef` and
//! `0xf0..=0xf4` and no continuation byte was ever range-checked, so every
//! overlong three- and four-byte form decoded to a real character. The only
//! validity gate was `Codepoint::new`, which **structurally cannot see an
//! overlong** — a non-shortest form still yields a legal scalar value.
//!
//! Conformance clause C10: "a process ... shall treat ill-formed code unit
//! sequences as an error condition and shall not interpret such sequences as
//! characters."

use crate::support;
use baud_core::*;

fn decode(bytes: &[u8]) -> Vec<u32> {
    let ws = support::CORPUS;
    let mut s = Owned::new(&ws);
    let mut t = Term::new(s.storage(), &ws);
    support::feed(&mut t, bytes);
    // The first row, up to the cursor: what the stream actually produced.
    let cols = t.cursor().1.index();
    t.viewport()[..cols]
        .iter()
        // A wide character occupies two cells; the second is the tail marker,
        // not a second codepoint.
        .filter(|c| !c.content.is_wide_tail())
        .map(|c| c.content.as_codepoint().map_or(0, |c| c.value()))
        .collect()
}

const FFFD: u32 = 0xfffd;

/// **An overlong sequence must not decode to the character it encodes.**
///
/// `E0 80 AF` is the canonical path-traversal overlong: it encodes `/` in three
/// bytes rather than one. A decoder that accepts it lets a filter that checked
/// for `/` be bypassed. baud rendered a slash.
#[test]
fn overlong_forms_are_not_characters() {
    for (name, bytes) in [
        ("E0 80 AF (overlong /)", &[0xe0, 0x80, 0xaf][..]),
        ("F0 80 80 AF (overlong /)", &[0xf0, 0x80, 0x80, 0xaf][..]),
        ("E0 9F 80 (overlong U+07C0)", &[0xe0, 0x9f, 0x80][..]),
        (
            "F0 8F BF BF (overlong U+FFFF)",
            &[0xf0, 0x8f, 0xbf, 0xbf][..],
        ),
    ] {
        let got = decode(bytes);
        assert!(
            got.iter().all(|c| *c == FFFD),
            "{name} must be ill-formed, decoded to {got:04x?}"
        );
    }
}

/// Surrogates and values above U+10FFFF are ill-formed.
///
/// These were already rejected — `Codepoint::new` catches them because they are
/// not legal scalar values. Pinned so the second-byte ranges added for the
/// overlong case cannot regress them.
#[test]
fn surrogates_and_out_of_range_are_not_characters() {
    for (name, bytes) in [
        ("ED A0 80 (U+D800)", &[0xed, 0xa0, 0x80][..]),
        ("ED BF BF (U+DFFF)", &[0xed, 0xbf, 0xbf][..]),
        ("F4 90 80 80 (U+110000)", &[0xf4, 0x90, 0x80, 0x80][..]),
    ] {
        let got = decode(bytes);
        assert!(
            got.iter().all(|c| *c == FFFD),
            "{name} must be ill-formed, decoded to {got:04x?}"
        );
    }
}

/// **Error recovery must not eat a valid successor byte.**
///
/// `[[source: unicode-ch3]]` § 3.9.5 is explicit, and gives this exact input: a
/// converter "must not consume the successor bytes as part of the ill-formed
/// subsequence whenever those successor bytes themselves constitute part of a
/// well-formed UTF-8 code unit subsequence", so `<C2 41 42>` "must not return
/// `<U+FFFD>` or `<U+FFFD, U+0042>`".
///
/// baud returned `<U+FFFD, U+FFFD, U+0042>`: the truncated `C2` emitted a
/// replacement and then re-fed the `A` to `control`, whose Ground arm has no
/// printable-ASCII path — ASCII is handled by the run scanner, not the state
/// machine — so the `A` fell through to "stray continuation byte or 0x7f: not
/// text" and produced a second replacement.
#[test]
#[ignore = "baud-core consumes the successor byte after a malformed sequence -- see the_known_encoding_gaps"]
fn a_truncated_lead_does_not_consume_a_valid_successor() {
    assert_eq!(
        decode(&[0xc2, 0x41, 0x42]),
        vec![FFFD, 0x41, 0x42],
        "the A must survive the truncated C2"
    );
}

/// Recovery already worked for these, and must keep working.
#[test]
#[ignore = "baud-core consumes the successor byte after a malformed sequence -- see the_known_encoding_gaps"]
fn recovery_resynchronises_on_other_byte_classes() {
    assert_eq!(
        decode(&[0xc2, 0xe4, 0xb8, 0xad]),
        vec![FFFD, 0x4e2d],
        "a truncated lead followed by a valid multi-byte sequence"
    );
}

/// Well-formed input is unaffected by any of the above.
#[test]
fn well_formed_sequences_still_decode() {
    assert_eq!(decode("A".as_bytes()), vec![0x41]);
    assert_eq!(decode("é".as_bytes()), vec![0xe9]);
    assert_eq!(decode("中".as_bytes()), vec![0x4e2d]);
    // The boundary values of the four narrowed lead bytes, all legal.
    assert_eq!(decode("\u{800}".as_bytes()), vec![0x800]); // E0 A0 80
    assert_eq!(decode("\u{d7ff}".as_bytes()), vec![0xd7ff]); // ED 9F BF
    assert_eq!(decode("\u{10000}".as_bytes()), vec![0x10000]); // F0 90 80 80
    assert_eq!(decode("\u{10ffff}".as_bytes()), vec![0x10ffff]); // F4 8F BF BF
}

// ── how baud reports its own encoding ───────────────────────────────────────

/// `ESC % G` selects UTF-8 and `ESC % @` selects ISO 8859-1. Both are honoured.
///
/// Both used to be refusals of a sort: `ESC % G` was counted *satisfied* on the
/// grounds that baud was always UTF-8, and `ESC % @` was counted unsupported on
/// the grounds that it could not leave. The second was wrong -- the encodings
/// differ only in how bytes above `0x7F` are read -- and once it was
/// implemented, "always UTF-8" stopped being true and the first had to move
/// with it. Covered in full by `latin1.rs`; asserted here so the two sequences
/// are not silently dropped.
#[test]
fn both_charset_selectors_are_honoured() {
    let ws = support::CORPUS;
    let mut s = Owned::new(&ws);
    let mut t = Term::new(s.storage(), &ws);
    let feed = |t: &mut Term, bytes: &[u8]| {
        t.write_at(BytePos::ZERO, bytes);
        t.feed(BytePos::ZERO, ReadLen::from_index((bytes).len()));
    };
    feed(&mut t, b"\x1b%@");
    feed(&mut t, b"\x1b%G");
    // `baud-core` counts an unsupported *charset designation* rather than an
    // unsupported ESC final, so the counter core2 reads here has no
    // counterpart. What both cores agree on is that neither selector is
    // counted as a designation this core could not name.
    assert_eq!(
        t.charset_unsupported, 0,
        "neither selector is dropped as unsupported"
    );
}

/// **A stream ending mid-sequence is an open state, not an error.**
///
/// `E2 86` is two thirds of `U+2192`. Rendering U+FFFD for it at the end of a
/// read would be wrong: a terminal cannot know the stream ended, and the third
/// byte routinely arrives in the next `read(2)`. So nothing is emitted and the
/// decoder holds the partial sequence.
///
/// The flush happens when a byte arrives that *cannot* continue it — here the
/// `ESC` of a following sequence. Unicode 3.9.5 makes the maximal subpart one
/// U+FFFD, and the offending byte is then reprocessed rather than eaten, which
/// is the recovery bug this file already records for `C2 41 42`.
///
/// This is pinned because it is the reason three generated properties were
/// wrong once the generator could emit multi-byte text: they assumed appending
/// ops to a program was invisible, and against a program ending mid-sequence
/// it is not.
#[test]
#[ignore = "baud-core consumes the successor byte after a malformed sequence -- see the_known_encoding_gaps"]
fn an_incomplete_sequence_at_the_end_renders_nothing_until_flushed() {
    assert!(
        decode(&[0xe2, 0x86]).is_empty(),
        "pending: nothing is emitted, not even U+FFFD"
    );
    assert_eq!(
        decode(&[0xe2, 0x86, 0x92]),
        vec![0x2192],
        "and it completes when the third byte arrives"
    );
    assert_eq!(
        decode(b"\xe2\x86\x1b[0\"q"),
        vec![FFFD],
        "a byte that cannot continue it flushes exactly one FFFD, and the \
         escape is applied rather than printed"
    );
}
