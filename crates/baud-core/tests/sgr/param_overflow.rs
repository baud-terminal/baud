//! What happens past the sixteenth parameter.
//!
//! `Term::params` holds [`ParamIdx::CAPACITY`] entries and a CSI may carry
//! more. Discarding the excess is a legitimate limit — every terminal has one,
//! and xterm's is 30 — but the two feeds have to discard it the *same way*.
//!
//! They did not. `fast.rs` accumulates a parameter into a local and stores it
//! only under the capacity bound, so a seventeenth parameter was parsed and
//! dropped. `machine.rs` wrote each digit straight into
//! `params[nparam - 1]`, and `nparam` pinned *at* capacity — so once the
//! sixteenth parameter was in, the separator was swallowed and the
//! seventeenth's digits appended to the sixteenth. `...;61;62;63` stored 6263
//! where the fast path stored 62.
//!
//! That made the result depend on where a read boundary fell, which is the
//! chunk-invariance failure this project has had before. `ingest_is_chunk_
//! invariant` did not catch it because its generator does not emit sequences
//! with more than sixteen parameters.

use crate::support;
use baud_core::*;
use baud_domain::{Attrs, ParamIdx};

fn feed_in_chunks(bytes: &[u8], chunk: usize) -> Attrs {
    let ws = support::CORPUS;
    let mut s = Owned::new(&ws);
    let mut t = Term::new(s.storage(), &ws);
    let mut i = 0;
    while i < bytes.len() {
        let n = chunk.min(bytes.len() - i);
        t.write_at(BytePos::ZERO, &bytes[i..i + n]);
        t.feed(BytePos::ZERO, ReadLen::from_index(n));
        i += n;
    }
    let cell = t.viewport()[0];
    t.styles().one().word_of(cell).attrs()
}

/// The capacity is derived from what DECRQSS has to be able to re-read.
///
/// Not a chosen number: a leading `0`, every attribute in `Attrs::WIRE` but
/// one of the two blink rates, which replace each other, and one of the five
/// underline styles, which do too, at its widest, `4:n`; a font; and three
/// colours in T.416's direct form, `48:2::r:g:b`, six elements. Asserting the
/// derivation rather than the value is the point -- the constant moved from 16
/// to 35 when the ideogram aspects widened `WIRE`, and a test pinning `16`
/// would have had to be edited to say so.
#[test]
fn the_capacity_covers_the_longest_reply_baud_emits() {
    // One blink rate: 5 and 6 replace each other, so no reply holds both.
    assert_eq!(Attrs::wire_in(Attrs::BLINKS), 2);
    // One underline style, as `4:n`: the five replace each other.
    assert_eq!(Attrs::wire_in(Attrs::UNDERLINES), 5);
    assert_eq!(
        ParamIdx::CAPACITY,
        1 + (Attrs::WIRE.len() - 1 - 5 + 2) + 1 + 3 * 6
    );
    const _: () = assert!(
        ParamIdx::CAPACITY >= 35,
        "bold plus three truecolours already answers with seventeen"
    );
}

/// **The parameter past capacity is dropped, not merged into the last one.**
///
/// `CAPACITY - 1` zeroes, then `1` (bold) as the last storable parameter and
/// `4` (underline) one past it. A clean drop leaves bold set and underline
/// clear. A digit merge turns the last one into `14` -- a font selection --
/// and loses bold, which is how the bug announced itself.
#[test]
fn the_parameter_past_capacity_is_dropped_not_merged() {
    let mut p = b"\x1b[".to_vec();
    for _ in 0..ParamIdx::CAPACITY - 1 {
        p.extend_from_slice(b"0;");
    }
    p.extend_from_slice(b"1;4mX");

    let a = feed_in_chunks(&p, 4096);
    assert!(a.has(Attrs::BOLD), "the last storable parameter survives");
    assert!(!a.has(Attrs::UNDERLINE), "and the one past it is discarded");
    assert_eq!(a, feed_in_chunks(&p, 1), "and both feeds agree");
}

/// **And it is dropped identically however the bytes are chunked.**
///
/// This is the assertion that was false. One byte at a time drives the state
/// machine; a single large read takes the fast path.
#[test]
fn parameter_overflow_is_chunk_invariant() {
    for payload in [
        &b"\x1b[0;0;0;0;0;0;0;0;0;0;0;0;0;0;0;1;4mX"[..],
        // Eighteen attributes: inside capacity now, but this is the payload
        // whose bulk/bytewise split first showed the bug (0xffff vs 0x7fff).
        &b"\x1b[1;2;3;4;5;7;8;9;20;26;51;52;53;60;61;62;63;64mX"[..],
        // Far past the limit, and separators with no digits at all — the shape
        // that would run `nparam` off the end of a `u8` if it did not saturate.
        &b"\x1b[1;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;4mX"[..],
    ] {
        let bulk = feed_in_chunks(payload, 4096);
        for chunk in [1, 2, 3, 7, 13] {
            assert_eq!(
                feed_in_chunks(payload, chunk),
                bulk,
                "chunk size {chunk} disagreed with a single read on {:?}",
                core::str::from_utf8(payload).unwrap()
            );
        }
    }
}

/// Every storable parameter still applies — the fix must not cost the last one.
#[test]
fn every_storable_parameter_still_applies() {
    let a = feed_in_chunks(b"\x1b[1;2;3;4;5;7;8;9;20;26;51;52;53;60;61;62mX", 1);
    for (bit, name) in [
        (Attrs::BOLD, "1"),
        (Attrs::STRIKE, "9"),
        (Attrs::FRAKTUR, "20"),
        (Attrs::OVERLINED, "53"),
        (Attrs::IDEOGRAM_OVERLINE, "62"),
    ] {
        assert!(a.has(bit), "SGR {name} applied");
    }
}

/// The DCS parameter path has the same bound, and needs the same proof.
///
/// `dcs_param_byte` got the identical fix, and every case above drives `CSI`.
/// No DCS function baud implements takes numeric parameters, so an over-long
/// prefix is ignored either way and the severity is lower — but this is a path
/// that was edited, and "the fix is symmetric so it must be fine" is exactly
/// the reasoning that let `machine.rs` diverge from `fast.rs` in the first
/// place.
///
/// The bound is load-bearing rather than tidy: with it removed this test does
/// not merely disagree between chunkings, it panics on an out-of-range index,
/// because `next_capped` lets the position reach `CAPACITY`. No shipped
/// version could reach that -- the old code pinned the index *at* capacity,
/// which corrupted instead -- but it means the check cannot be relaxed.
#[test]
fn dcs_parameter_overflow_is_chunk_invariant() {
    let mut p = b"\x1bP".to_vec();
    for k in 0..2 * ParamIdx::CAPACITY {
        if k > 0 {
            p.push(b';');
        }
        p.extend_from_slice(b"12");
    }
    // A DECRQSS for SGR, behind a parameter prefix far past capacity.
    p.extend_from_slice(b"$qm\x1b\\");

    let bulk = reply_for(&p, 4096);
    for chunk in [1, 2, 5, 17] {
        assert_eq!(
            reply_for(&p, chunk),
            bulk,
            "chunk size {chunk} disagreed with a single read"
        );
    }
}

fn reply_for(bytes: &[u8], chunk: usize) -> Vec<u8> {
    let ws = support::CORPUS;
    let mut s = Owned::new(&ws);
    let mut t = Term::new(s.storage(), &ws);
    let mut i = 0;
    while i < bytes.len() {
        let n = chunk.min(bytes.len() - i);
        t.write_at(BytePos::ZERO, &bytes[i..i + n]);
        t.feed(BytePos::ZERO, ReadLen::from_index(n));
        i += n;
    }
    t.reply().to_vec()
}
