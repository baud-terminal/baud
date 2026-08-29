//! The block decoder must agree with the scalar one, or be silent.
//!
//! **The contract is one-sided on purpose.** `Decode::block` may refuse -- a
//! truncated sequence, an overlong form, a surrogate, a byte no sequence may
//! contain -- and refusing costs the caller a scalar retry, which is always
//! correct. What it may never do is return a codepoint the scalar decoder
//! would not have produced, because then two spellings of the same operation
//! disagree and only one of them is tested by the conformance suite.
//!
//! That asymmetry is why this compares *what it produced* rather than
//! requiring it to produce anything.

use baud_domain::{AmbiguousWidth, Codepoint, StreamByte, Utf8Acc, Width};
use baud_isa::{Decode, Portable};

/// The scalar decoder, spelled out: this is what the parser does per character.
fn scalar(b: &[u8], at: usize) -> Option<(Codepoint, usize)> {
    let byte = *b.get(at)?;
    // ASCII is not a `utf8_lead`; the parser handles it before the decoder.
    if byte < 0x80 {
        return Codepoint::new(u32::from(byte)).map(|c| (c, 1));
    }
    let (rem, bits, min) = StreamByte::from_stream(byte).utf8_lead()?;
    let need = usize::from(rem) + 1;
    if at + need > b.len() {
        return None;
    }
    let mut acc = Utf8Acc::lead(bits);
    for k in 1..need {
        acc = acc.push(StreamByte::from_stream(b[at + k]).utf8_continuation()?);
    }
    let cp = acc.finish(min);
    (cp != Codepoint::REPLACEMENT).then_some((cp, need))
}

fn check(bytes: &[u8]) {
    if bytes.len() < 16 {
        return;
    }
    let sb = StreamByte::slice_from(bytes);
    let got = unsafe { Portable::block(sb) };
    let mut at = 0usize;
    for k in 0..got.count.index() {
        let Some((want, len)) = scalar(bytes, at) else {
            panic!(
                "block decoded {:?} at {at} where the scalar decoder refused: {bytes:02x?}",
                got.cps[k].value()
            );
        };
        assert_eq!(
            got.cps[k].value(),
            want.value(),
            "lane {k} at byte {at}: block says U+{:04X}, scalar says U+{:04X} -- {bytes:02x?}",
            got.cps[k].value(),
            want.value()
        );
        assert!(
            got.widths[k] == Width::of(want, AmbiguousWidth::Narrow),
            "lane {k}: width disagrees for U+{:04X}",
            want.value()
        );
        at += len;
    }
    assert_eq!(
        got.bytes.index(),
        at,
        "consumed {} bytes for {} characters that measure {at}",
        got.bytes.index(),
        got.count.index()
    );
}

#[test]
fn agrees_on_hand_written_shapes() {
    for s in [
        "hello world, this is plain ascii text",
        "日本語のテキストです。これはテストです。",
        "abc 日本語 def かたかな ghi",
        "\u{1f600}\u{1f601}\u{1f602}\u{1f603} emoji block",
        "\u{00e9}\u{00e8}\u{00ea}\u{00eb} latin1 range repeated over sixteen",
        "mixed \u{4e00}\u{4e8c}\u{4e09} and \u{ff21}\u{ff22} fullwidth",
    ] {
        let b = s.as_bytes();
        for start in 0..b.len().saturating_sub(16) {
            check(&b[start..]);
        }
    }
}

#[test]
fn refuses_or_agrees_on_malformed_input() {
    // Overlong, surrogate, truncated, and bytes no sequence may contain. The
    // decoder may refuse any of these; it may not decode them differently.
    let cases: &[&[u8]] = &[
        b"\xe0\x80\xaf aaaaaaaaaaaaaaaaaaaa",
        b"\xed\xa0\x80 aaaaaaaaaaaaaaaaaaaa",
        b"\xf5\x80\x80\x80 aaaaaaaaaaaaaaaaa",
        b"\xc0\xaf aaaaaaaaaaaaaaaaaaaaa",
        b"\xe4\xb8 aaaaaaaaaaaaaaaaaaaaaa",
        b"\x80\x80\x80 aaaaaaaaaaaaaaaaaaaa",
    ];
    for c in cases {
        check(c);
    }
}

#[test]
fn agrees_on_every_byte_pattern_of_a_small_alphabet() {
    // Exhaustive over a mixed alphabet: ASCII, a two-byte, a three-byte, a
    // continuation byte on its own, and a lead with no continuation.
    let alpha: [&[u8]; 6] = [b"a", b"\xc3\xa9", b"\xe4\xb8\x80", b"\x80", b"\xe4", b" "];
    let mut buf = Vec::with_capacity(64);
    for a in 0..6 {
        for b in 0..6 {
            for c in 0..6 {
                for d in 0..6 {
                    buf.clear();
                    for i in [a, b, c, d] {
                        buf.extend_from_slice(alpha[i]);
                    }
                    while buf.len() < 24 {
                        buf.push(b'z');
                    }
                    check(&buf);
                }
            }
        }
    }
}
