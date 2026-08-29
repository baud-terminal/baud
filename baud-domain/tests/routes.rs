//! The block classifier must agree with the scalar route on every byte.
//!
//! **Exhaustive and per-lane, on purpose.** A shuffle applies one table to
//! sixteen indices at once, so a table entry that is wrong is wrong for every
//! byte with that high nibble -- and a correction that is wrong is wrong in one
//! lane and right in fifteen. This repo has already shipped a lane-position bug
//! that was invisible whenever the first match landed in lane zero, which on
//! real text is most of the time.

use baud_domain::*;

/// Every byte, in every lane position.
#[test]
fn the_block_classifier_agrees_with_the_scalar_route() {
    for pos in 0..LANE_COUNT {
        for raw in 0u8..=255 {
            // Fill with a byte of a different route so a lane that reads its
            // neighbour is caught rather than accidentally right.
            let filler = match Route::of(StreamByte::from_stream(raw)) {
                Route::Text => 0x00,
                _ => b'A',
            };
            let mut block = [StreamByte::from_stream(filler); LANE_COUNT];
            block[pos] = StreamByte::from_stream(raw);
            let r = Vectorized::<Printable>::load(&block).routes();
            assert_eq!(
                r.at(pos),
                Route::of(StreamByte::from_stream(raw)),
                "byte {raw:#04x} in lane {pos}"
            );
        }
    }
}

/// The route's width must be what UTF-8 says the lead byte owes, so a caller
/// can step by it without asking the decoder.
#[test]
fn the_route_width_matches_the_lead_byte() {
    for raw in 0u8..=255 {
        let b = StreamByte::from_stream(raw);
        let want = match b.utf8_lead() {
            Some((rem, _, _)) => usize::from(rem) + 1,
            None if b.utf8_continuation().is_some() => 0,
            None => 1,
        };
        // `C0`, `C1` and `F5..FF` are not legal leads, so the decoder refuses
        // them where the route -- which reads only the nibble -- does not.
        // That is the route's job: it says *where a character may start*, and
        // the decoder still says whether one does.
        let overlong = (0xc0..=0xc1).contains(&raw) || raw >= 0xf5;
        if !overlong {
            assert_eq!(
                Route::of(b).width(),
                want,
                "byte {raw:#04x}: route width against the lead byte"
            );
        }
    }
}

/// A whole block of text is one route, which is what makes a run scannable.
#[test]
fn a_run_of_text_is_one_route() {
    let block = [StreamByte::from_stream(b'x'); LANE_COUNT];
    let r = Vectorized::<Printable>::load(&block).routes();
    assert!(
        r.is_not(Route::Text).first().is_none(),
        "no lane may differ"
    );
    assert!(r.is(Route::Text).first().is_some(), "every lane is text");
}

// ── stride runs ─────────────────────────────────────────────────────────────

fn block_of(bytes: &[u8]) -> RouteBlock {
    let mut b = [StreamByte::from_stream(b'!'); LANE_COUNT];
    for (i, x) in bytes.iter().take(LANE_COUNT).enumerate() {
        b[i] = StreamByte::from_stream(*x);
    }
    Vectorized::<Printable>::load(&b).routes()
}

/// Five three-byte characters fill a block exactly, and the sixteenth byte
/// cannot be part of a sixth.
#[test]
fn a_block_of_three_byte_characters_yields_five() {
    let mut v = Vec::new();
    for _ in 0..6 {
        v.extend_from_slice("世".as_bytes());
    }
    assert_eq!(block_of(&v).stride_three(), 5);
}

/// A run that stops is counted up to where it stops, not past it.
#[test]
fn a_run_is_counted_only_to_where_it_stops() {
    let mut v = Vec::new();
    v.extend_from_slice("世界".as_bytes()); // two characters, six bytes
    v.extend_from_slice(b"abc");
    assert_eq!(block_of(&v).stride_three(), 2);
}

/// **A different stride is not a three-stride run**, however many
/// continuations follow. A two-byte lead puts the next lead at the wrong lane
/// and the pattern test is what rejects it.
#[test]
fn a_two_byte_character_is_not_a_three_stride_run() {
    let mut v = Vec::new();
    v.extend_from_slice("é".as_bytes()); // two bytes
    v.extend_from_slice("世界界".as_bytes());
    assert_eq!(block_of(&v).stride_three(), 0);
}

/// A four-byte lead is where the emoji are, and they need the cluster machine
/// one codepoint at a time.
#[test]
fn a_four_byte_character_is_refused() {
    let mut v = Vec::new();
    v.extend_from_slice("🚀".as_bytes());
    v.extend_from_slice("世界".as_bytes());
    assert_eq!(block_of(&v).stride_three(), 0);
}

/// Text is not a stride run either -- it has its own, cheaper route.
#[test]
fn printable_text_yields_no_stride_run() {
    assert_eq!(block_of(b"hello world!!!!!").stride_three(), 0);
}

/// A truncated character at the end of the block is not counted: the caller
/// would read past the bytes it has.
#[test]
fn a_truncated_character_is_not_counted() {
    let mut v = Vec::new();
    for _ in 0..5 {
        v.extend_from_slice("世".as_bytes());
    }
    v.push(0xe4); // a lead with no continuations in this block
    let n = block_of(&v).stride_three();
    assert_eq!(n, 5, "the five complete characters, and not the sixth");
}
