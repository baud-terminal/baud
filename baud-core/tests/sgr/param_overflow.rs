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

use baud_core::*;
use baud_domain::{Attrs, ParamIdx};

fn feed_in_chunks(bytes: &[u8], chunk: usize) -> Attrs {
    let mut s = Storage::boxed();
    let mut t = Term::new(&mut s, OpMode::Computed);
    let mut i = 0;
    while i < bytes.len() {
        let n = chunk.min(bytes.len() - i);
        let mut slot = t.read_slot();
        let base = slot.base();
        let f = slot.write(StreamByte::slice_from(&bytes[i..i + n]));
        t.feed(base, f.len());
        t.commit_read(f.len());
        i += n;
    }
    let cell = t.viewport()[0];
    t.style_of(cell).attrs()
}

/// The capacity is sixteen, and it is the array's capacity, not a literal.
#[test]
fn the_parameter_limit_is_the_arrays_capacity() {
    assert_eq!(ParamIdx::CAPACITY, 16);
}

/// **The seventeenth parameter is dropped, not merged into the sixteenth.**
///
/// Fifteen zeroes, then `1` (bold) as the sixteenth and `4` (underline) as the
/// seventeenth. A clean drop leaves bold set and underline clear. A digit
/// merge turns the sixteenth into `14` — a font selection — and loses bold,
/// which is how the bug announced itself.
#[test]
fn the_seventeenth_parameter_is_dropped_not_merged() {
    let a = feed_in_chunks(b"\x1b[0;0;0;0;0;0;0;0;0;0;0;0;0;0;0;1;4mX", 4096);
    assert!(
        a.has(Attrs::BOLD),
        "the sixteenth parameter survives intact"
    );
    assert!(!a.has(Attrs::UNDERLINE), "and the seventeenth is discarded");
}

/// **And it is dropped identically however the bytes are chunked.**
///
/// This is the assertion that was false. One byte at a time drives the state
/// machine; a single large read takes the fast path.
#[test]
fn parameter_overflow_is_chunk_invariant() {
    for payload in [
        &b"\x1b[0;0;0;0;0;0;0;0;0;0;0;0;0;0;0;1;4mX"[..],
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

/// Sixteen parameters still all arrive — the fix must not cost the last one.
#[test]
fn all_sixteen_parameters_still_apply() {
    let a = feed_in_chunks(b"\x1b[1;2;3;4;5;7;8;9;20;26;51;52;53;60;61;62mX", 1);
    for (bit, name) in [
        (Attrs::BOLD, "1"),
        (Attrs::STRIKE, "9"),
        (Attrs::FRAKTUR, "20"),
        (Attrs::OVERLINED, "53"),
        (Attrs::IDEOGRAM_OVERLINE, "62 — the sixteenth"),
    ] {
        assert!(a.has(bit), "SGR {name} applied");
    }
}
