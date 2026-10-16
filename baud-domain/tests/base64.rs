//! The base64 decoder against `[[source: rfc4648-base64]]`: its own test
//! vectors (section 10), every input up to two octets round-tripped through
//! a reference encoder written from section 4, and the rejections sections
//! 3.3 and 4 require.
//!
//! The decoder takes one byte at a time, so a split anywhere is the same
//! sequence of calls as no split at all; what the incremental shape has to
//! get right is that the carry survives between calls, which every vector
//! here exercises because none of them is decoded in one call.

use baud_domain::{Base64, Base64Step};
use proptest::prelude::*;

/// Section 4, Table 1.
const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// A reference encoder written from section 4: 24-bit groups, zero bits
/// added on the right of a short final group, `=` for each missing sextet.
fn encode(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    for group in data.chunks(3) {
        let mut b = [0u8; 3];
        b[..group.len()].copy_from_slice(group);
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        let sextets = [(n >> 18) & 63, (n >> 12) & 63, (n >> 6) & 63, n & 63];
        let keep = group.len() + 1;
        for (i, s) in sextets.iter().enumerate() {
            out.push(if i < keep {
                ALPHABET[usize::try_from(*s).expect("a sextet")]
            } else {
                b'='
            });
        }
    }
    out
}

/// Feed `input` one byte at a time and finish: the decoded bytes, or `None`
/// where the decoder rejected the data.
fn decode(input: &[u8]) -> Option<Vec<u8>> {
    let mut d = Base64::NEW;
    let mut out = Vec::new();
    for &b in input {
        match d.push(b) {
            Base64Step::Need => {}
            Base64Step::Octets(o) => out.extend_from_slice(o.as_slice()),
            Base64Step::Invalid => return None,
        }
    }
    match d.finish() {
        Ok(tail) => {
            out.extend_from_slice(tail.as_slice());
            Some(out)
        }
        Err(_) => None,
    }
}

#[test]
fn the_rfc_test_vectors_decode() {
    // `[[source: rfc4648-base64]]` section 10, verbatim.
    let vectors: [(&[u8], &[u8]); 7] = [
        (b"", b""),
        (b"f", b"Zg=="),
        (b"fo", b"Zm8="),
        (b"foo", b"Zm9v"),
        (b"foob", b"Zm9vYg=="),
        (b"fooba", b"Zm9vYmE="),
        (b"foobar", b"Zm9vYmFy"),
    ];
    for (plain, encoded) in vectors {
        assert_eq!(encode(plain), encoded, "the reference encoder is wrong");
        assert_eq!(
            decode(encoded).as_deref(),
            Some(plain),
            "{:?}",
            core::str::from_utf8(encoded)
        );
    }
}

#[test]
fn every_input_of_up_to_two_octets_round_trips() {
    let mut inputs: Vec<Vec<u8>> = vec![Vec::new()];
    for a in 0..=255u8 {
        inputs.push(vec![a]);
        for b in 0..=255u8 {
            inputs.push(vec![a, b]);
        }
    }
    for data in &inputs {
        assert_eq!(
            decode(&encode(data)).as_deref(),
            Some(&data[..]),
            "{data:?}"
        );
    }
}

#[test]
fn three_octet_groups_round_trip_across_the_alphabet() {
    // Every sextet value in every position of a group, and groups in a row,
    // so the carry between quanta is exercised as well as within one.
    let mut data = Vec::new();
    for i in 0..=255u8 {
        data.extend_from_slice(&[i, i.wrapping_mul(7), i.wrapping_add(91)]);
    }
    for end in 0..data.len() {
        assert_eq!(decode(&encode(&data[..end])).as_deref(), Some(&data[..end]));
    }
}

#[test]
fn a_character_outside_the_alphabet_is_rejected() {
    // Section 3.3: "Implementations MUST reject the encoded data if it
    // contains characters outside the base alphabet", and ctlseqs, which
    // names the RFC for OSC 52's Pd, states nothing otherwise.
    for bad in [&b"Zm9v\nYmFy"[..], b"Zm 9v", b"Zm9v?", b"-_-_", b"Zm9\x00v"] {
        assert_eq!(decode(bad), None, "{bad:?}");
    }
}

#[test]
fn a_pad_before_the_end_is_rejected() {
    // Section 3.3 lets a referring specification ignore a pad present before
    // the end; ctlseqs does not, so a pad there is data outside the alphabet.
    for bad in [&b"Zg==Zg=="[..], b"Z=g=", b"=Zg=", b"Zm8=Zm8=", b"Zg=a"] {
        assert_eq!(decode(bad), None, "{bad:?}");
    }
}

#[test]
fn excess_pads_are_rejected() {
    // "the excess pad characters MAY also be ignored" -- by a specification
    // that says so, which ctlseqs does not.
    for bad in [&b"Zg==="[..], b"Zm8==", b"Zm9v=", b"===="] {
        assert_eq!(decode(bad), None, "{bad:?}");
    }
}

#[test]
fn a_lone_sextet_at_the_end_is_rejected() {
    // Six bits cannot make an octet: section 4 lists only the three cases a
    // final quantum can be, and a single character is none of them.
    for bad in [&b"Z"[..], b"Zm9vY", b"Z=", b"Z=="] {
        assert_eq!(decode(bad), None, "{bad:?}");
    }
}

#[test]
fn half_a_padding_is_rejected() {
    // Two characters and one pad is neither the padded form nor the
    // unpadded one.
    assert_eq!(decode(b"Zg="), None);
}

#[test]
fn an_unpadded_final_quantum_decodes() {
    // Section 3.2 obliges an *encoder* to pad. A decoder reading a string
    // whose end it knows -- an OSC body ends at its terminator -- can decode
    // the two short final quanta without the pads, and section 3.3's
    // rejection is of characters outside the alphabet, which a missing pad
    // is not. Accepted; see `Base64::finish`.
    assert_eq!(decode(b"Zg").as_deref(), Some(&b"f"[..]));
    assert_eq!(decode(b"Zm8").as_deref(), Some(&b"fo"[..]));
    assert_eq!(decode(b"Zm9vYg").as_deref(), Some(&b"foob"[..]));
}

#[test]
fn non_zero_pad_bits_decode() {
    // Section 3.5: decoders "MAY chose to reject an encoding if the pad bits
    // have not been set to zero". baud does not: `Zh==` is `Zg==` with a pad
    // bit set, and both decode to `f`.
    assert_eq!(decode(b"Zh==").as_deref(), Some(&b"f"[..]));
    assert_eq!(decode(b"Zm9=").as_deref(), Some(&b"fo"[..]));
}

#[test]
fn a_rejection_is_sticky() {
    // Once rejected, nothing that follows makes the data valid again.
    let mut d = Base64::NEW;
    assert_eq!(d.push(b'!'), Base64Step::Invalid);
    for &b in b"Zm9v" {
        assert_eq!(d.push(b), Base64Step::Invalid);
    }
    assert!(d.finish().is_err());
}

#[test]
fn the_decoder_holds_no_more_than_a_quantum() {
    // The carry a read boundary has to survive is at most three sextets:
    // the fourth completes a quantum and releases its octets at once.
    let mut d = Base64::NEW;
    assert_eq!(d.push(b'Z'), Base64Step::Need);
    assert_eq!(d.push(b'm'), Base64Step::Need);
    assert_eq!(d.push(b'9'), Base64Step::Need);
    match d.push(b'v') {
        Base64Step::Octets(o) => assert_eq!(o.as_slice(), b"foo"),
        other => panic!("{other:?}"),
    }
    assert_eq!(d, Base64::NEW, "a completed quantum leaves no carry");
}

/// Feeds `input` in the reads `cuts` makes of it (sorted offsets) through
/// [`Base64::feed`], each into an output window of at most `room` octets,
/// and finishes: the decoded bytes, or `None` where the data was rejected.
fn feed_cut(input: &[u8], cuts: &[usize], room: usize) -> Option<Vec<u8>> {
    let mut d = Base64::NEW;
    let mut out = Vec::new();
    let mut from = 0;
    for to in cuts.iter().copied().chain([input.len()]) {
        let to = to.clamp(from, input.len());
        let mut chunk = &input[from..to];
        while !chunk.is_empty() {
            let mut window = vec![0u8; room];
            let fed = d.feed(chunk, &mut window).ok()?;
            assert!(fed.read > 0 || room < 3, "no progress with room {room}");
            if fed.read == 0 {
                // A window too small for the next quantum: widen it, as a
                // caller moving its window on would.
                let mut wide = [0u8; 3];
                let fed = d.feed(chunk, &mut wide).ok()?;
                out.extend_from_slice(&wide[..fed.wrote]);
                chunk = &chunk[fed.read..];
                continue;
            }
            out.extend_from_slice(&window[..fed.wrote]);
            chunk = &chunk[fed.read..];
        }
        from = to;
    }
    let tail = d.finish().ok()?;
    out.extend_from_slice(tail.as_slice());
    Some(out)
}

/// Every split into two reads and every read size, at two window sizes,
/// against the decoder fed a byte at a time.
fn every_split_and_size(input: &[u8]) {
    let whole = decode(input);
    for room in [1, 3, 4096] {
        for k in 0..=input.len() {
            assert_eq!(
                feed_cut(input, &[k], room),
                whole,
                "{input:?} split at {k}, room {room}"
            );
        }
        for size in 1..=input.len().max(1) {
            let cuts: Vec<usize> = (size..input.len()).step_by(size).collect();
            assert_eq!(
                feed_cut(input, &cuts, room),
                whole,
                "{input:?} reads of {size}, room {room}"
            );
        }
    }
}

#[test]
fn a_feed_cut_anywhere_decodes_as_a_byte_at_a_time() {
    // kitty's chunks need not be multiples of four (Jev, 2026-10-15), and a
    // read boundary falls anywhere, so a quantum straddles feeds: padded and
    // unpadded ends, every rejection above, and data long enough for the
    // four-at-a-time path to run between straddling quanta.
    let mut data = Vec::new();
    for i in 0..=255u8 {
        data.extend_from_slice(&[i, i.wrapping_mul(7), i.wrapping_add(91)]);
    }
    let long = encode(&data[..100]);
    let mut cases: Vec<Vec<u8>> = vec![long.clone(), long[..long.len() - 2].to_vec()];
    for v in [
        &b""[..],
        b"Zg==",
        b"Zm8=",
        b"Zm9v",
        b"Zg",
        b"Zm8",
        b"Zm9vYg",
        b"Zh==",
        b"Zm9vYmFy",
        b"Zm9v\nYmFy",
        b"Zg==Zg==",
        b"Zg===",
        b"Z",
        b"Zg=",
        b"Zm9vY",
        b"Zm9v?",
        b"====",
    ] {
        cases.push(v.to_vec());
    }
    let mut bad = long.clone();
    bad[61] = b'!';
    cases.push(bad);
    for c in &cases {
        every_split_and_size(c);
    }
    // The control: the long case decodes to what was encoded.
    assert_eq!(feed_cut(&long, &[], 4096).as_deref(), Some(&data[..100]));
}

#[test]
fn a_full_window_stops_the_feed_before_the_quantum_it_cannot_hold() {
    let mut d = Base64::NEW;
    let mut out = [0u8; 4];
    // One quantum fits; of the next, three characters are carried and the
    // fourth, whose octets would not fit, is left unread.
    let fed = d.feed(b"Zm9vYmFy", &mut out).expect("valid");
    assert_eq!((fed.read, fed.wrote), (7, 3));
    assert_eq!(&out[..3], b"foo");
    let fed = d.feed(b"y", &mut out).expect("valid");
    assert_eq!((fed.read, fed.wrote), (1, 3));
    assert_eq!(&out[..3], b"bar");
}

#[test]
fn a_rejected_feed_stays_rejected() {
    let mut d = Base64::NEW;
    let mut out = [0u8; 16];
    assert!(d.feed(b"Zm9v!", &mut out).is_err());
    assert!(d.feed(b"Zm9v", &mut out).is_err());
    assert!(d.finish().is_err());
}

proptest! {
    /// Generated data, encoded padded or not, with a character perhaps
    /// replaced, cut at generated points into windows of generated size:
    /// the same result as a byte at a time, and the data back when valid.
    #[test]
    fn a_generated_feed_cut_anywhere_decodes_as_a_byte_at_a_time(
        data in prop::collection::vec(any::<u8>(), 0..400),
        strip in any::<bool>(),
        hit in prop::option::of((0usize..600, any::<u8>())),
        cuts in prop::collection::vec(0usize..600, 0..10),
        room in 1usize..40,
    ) {
        let mut input = encode(&data);
        // Both final forms: padded, and with every pad dropped (a single
        // pad dropped from `xx==` is half a padding, rejected above).
        if strip {
            let pads = input.iter().rev().take_while(|&&b| b == b'=').count();
            input.truncate(input.len() - pads);
        }
        if let Some((at, b)) = hit {
            let len = input.len().max(1);
            if let Some(slot) = input.get_mut(at % len) {
                *slot = b;
            }
        }
        let mut cuts = cuts;
        cuts.sort_unstable();
        let whole = decode(&input);
        if hit.is_none() {
            prop_assert_eq!(whole.as_deref(), Some(&data[..]));
        }
        prop_assert_eq!(feed_cut(&input, &cuts, room), whole);
    }
}
