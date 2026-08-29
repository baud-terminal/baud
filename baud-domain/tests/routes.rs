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
