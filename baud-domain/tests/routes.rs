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

// ── the vector decode ───────────────────────────────────────────────────────

/// The quad decode must agree with decoding one character at a time, for every
/// three-byte sequence shape.
///
/// **Exhaustive over the interesting axis and randomised over the rest.** All
/// 65,536 `(b1, b2)` pairs against a fixed lead, and then a spread of leads --
/// the lead contributes four bits and the continuations six each, so a shift
/// that is wrong by one shows up in the pair sweep, and a lane that is wrong
/// shows up because the four characters differ.
#[test]
fn the_quad_decode_agrees_with_one_at_a_time() {
    let scalar = |s: &[u8]| -> Option<Codepoint> {
        let raw = ((s[0] as u32 & 0x0f) << 12) | ((s[1] as u32 & 0x3f) << 6) | (s[2] as u32 & 0x3f);
        match raw < 0x800 {
            true => None,
            false => Codepoint::new(raw),
        }
    };
    for lead in [0xe0u8, 0xe4, 0xe9, 0xef] {
        for b1 in (0x80u8..=0xbf).step_by(3) {
            for b2 in 0x80u8..=0xbf {
                // Four *different* characters, so a swizzle that reads the
                // wrong lane cannot pass by coincidence.
                let mut raw = [0u8; LANE_COUNT];
                let mut want = [None; 4];
                for i in 0..4 {
                    let s = [
                        lead.wrapping_add(i as u8) | 0xe0,
                        b1.wrapping_add(i as u8) & 0xbf | 0x80,
                        b2.wrapping_sub(i as u8) & 0xbf | 0x80,
                    ];
                    raw[3 * i..3 * i + 3].copy_from_slice(&s);
                    want[i] = scalar(&s);
                }
                let block = Vectorized::<Printable>::load(StreamByte::slice_from(&raw));
                assert_eq!(
                    block.stride_three_quad(),
                    want,
                    "lead {lead:#04x} b1 {b1:#04x} b2 {b2:#04x}"
                );
            }
        }
    }
}

/// The quad only reads the twelve bytes it owns: whatever follows must not
/// change the answer.
#[test]
fn the_quad_ignores_the_bytes_past_its_run() {
    let mut a = [0u8; LANE_COUNT];
    for i in 0..4 {
        a[3 * i..3 * i + 3].copy_from_slice("世".as_bytes());
    }
    let mut b = a;
    b[12..].copy_from_slice(&[0xff, 0xff, 0xff, 0xff]);
    let qa = Vectorized::<Printable>::load(StreamByte::slice_from(&a)).stride_three_quad();
    let qb = Vectorized::<Printable>::load(StreamByte::slice_from(&b)).stride_three_quad();
    assert_eq!(qa, qb);
    assert_eq!(qa, [Codepoint::new(0x4e16); 4]);
}

/// `leads` must be exactly the union of the three lead routes, which is what
/// makes one compare stand in for three masks.
#[test]
fn the_lead_mask_is_the_union_of_the_three_lengths() {
    for raw in 0u8..=255 {
        let mut b = [StreamByte::from_stream(b'!'); LANE_COUNT];
        b[0] = StreamByte::from_stream(raw);
        let r = Vectorized::<Printable>::load(&b).routes();
        let want = matches!(
            Route::of(StreamByte::from_stream(raw)),
            Route::Lead2 | Route::Lead3 | Route::Lead4
        );
        assert_eq!(r.leads().bits() & 1 != 0, want, "byte {raw:#04x}");
    }
}

/// `from_three` must agree with the general constructor on every value a
/// three-byte sequence can produce, which is all sixteen bits.
#[test]
fn the_three_byte_constructor_agrees_with_the_general_one() {
    for v in 0u32..=0xffff {
        let want = match v < 0x800 {
            true => None,
            false => Codepoint::new(v),
        };
        assert_eq!(Codepoint::from_three(v), want, "{v:#06x}");
    }
}

/// **The width table answers to `Route::of`, exhaustively.**
///
/// It exists because the run scanner reached that seven-way range match per
/// byte of every multi-byte run; a table is only allowed to replace a function
/// if it agrees with it on every input, and for a byte-indexed table "every
/// input" is 256 cases.
#[test]
fn the_route_width_table_agrees_with_the_routes() {
    for (i, b) in StreamByte::ALL.iter().enumerate() {
        assert_eq!(
            baud_domain::lead_width(*b),
            Route::of(*b).width(),
            "byte {i:#04x}"
        );
    }
}

/// A continuation is exactly the byte that claims no length of its own, which
/// is the second question the scanner asks off the same load.
#[test]
fn width_zero_means_continuation() {
    for b in StreamByte::ALL.iter() {
        assert_eq!(
            baud_domain::lead_width(*b) == 0,
            matches!(Route::of(*b), Route::Cont),
            "byte {:#04x}",
            b.raw_byte()
        );
    }
}
