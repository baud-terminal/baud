//! Text sizing's metadata and text, `OSC 66 ; metadata ; text`
//! (`[[source: kitty-text-sizing]]` 52-82): every key at each bound and one
//! past it, each refusal its own variant, and the text read as safe UTF-8.

use baud_domain::{
    Codepoint, SizeFrac, SizeHAlign, SizeScale, SizeVAlign, SizeWidth, SizedText, TextSize,
    TextSizeRefused,
};

fn parse(meta: &str) -> Result<TextSize, TextSizeRefused> {
    TextSize::parse(meta.as_bytes())
}

#[test]
fn no_metadata_is_every_default() {
    for meta in ["", ":", "::"] {
        let t = parse(meta).unwrap();
        assert_eq!(t, TextSize::DEFAULT, "{meta:?}");
        assert_eq!(t.scale.get(), 1);
        assert_eq!(t.width, SizeWidth::Split);
        assert!(!t.width.is_explicit());
        assert_eq!(t.frac, SizeFrac::Whole);
        assert_eq!(t.valign, SizeVAlign::Top);
        assert_eq!(t.halign, SizeHAlign::Left);
    }
    assert_eq!(TextSize::DEFAULT.scale, SizeScale::ONE);
}

#[test]
fn the_scale_is_one_to_seven() {
    assert_eq!(parse("s=1").unwrap().scale.get(), 1);
    assert_eq!(parse("s=7").unwrap().scale.get(), 7);
    assert_eq!(parse("s=0"), Err(TextSizeRefused::Scale));
    assert_eq!(parse("s=8"), Err(TextSizeRefused::Scale));
    assert_eq!(parse("s=-1"), Err(TextSizeRefused::Scale));
}

#[test]
fn the_width_is_zero_to_seven_and_zero_splits() {
    assert_eq!(parse("w=0").unwrap().width, SizeWidth::Split);
    let one = parse("w=1").unwrap().width;
    assert!(one.is_explicit());
    assert_eq!(one.cells(), Some(1));
    assert_eq!(parse("w=7").unwrap().width.cells(), Some(7));
    assert_eq!(SizeWidth::Split.cells(), None);
    assert_eq!(parse("w=8"), Err(TextSizeRefused::Width));
    assert_eq!(parse("w=-1"), Err(TextSizeRefused::Width));
}

#[test]
fn the_fraction_is_zero_to_fifteen_with_d_above_n() {
    let SizeFrac::Ratio(r) = parse("n=14:d=15").unwrap().frac else {
        panic!("14/15 is a ratio");
    };
    assert_eq!((r.numerator(), r.denominator()), (14, 15));
    let SizeFrac::Ratio(r) = parse("n=0:d=1").unwrap().frac else {
        panic!("0/1 is a ratio");
    };
    assert_eq!((r.numerator(), r.denominator()), (0, 1));
    // `d` zero is no fraction, whatever `n` is.
    assert_eq!(parse("n=15:d=0").unwrap().frac, SizeFrac::Whole);
    assert_eq!(parse("n=15").unwrap().frac, SizeFrac::Whole);
    assert_eq!(parse("n=16"), Err(TextSizeRefused::Numerator));
    assert_eq!(parse("n=-1"), Err(TextSizeRefused::Numerator));
    assert_eq!(parse("d=16"), Err(TextSizeRefused::Denominator));
    assert_eq!(parse("d=-1"), Err(TextSizeRefused::Denominator));
    // "Must be ``> n`` when non-zero."
    assert_eq!(parse("n=1:d=1"), Err(TextSizeRefused::Order));
    assert_eq!(parse("n=2:d=1"), Err(TextSizeRefused::Order));
    assert!(parse("n=1:d=2").is_ok());
}

#[test]
fn the_alignments_are_zero_to_two() {
    let t = parse("v=0:h=0").unwrap();
    assert_eq!((t.valign, t.halign), (SizeVAlign::Top, SizeHAlign::Left));
    let t = parse("v=1:h=1").unwrap();
    assert_eq!(
        (t.valign, t.halign),
        (SizeVAlign::Bottom, SizeHAlign::Right)
    );
    let t = parse("v=2:h=2").unwrap();
    assert_eq!(
        (t.valign, t.halign),
        (SizeVAlign::Centre, SizeHAlign::Centre)
    );
    assert_eq!(parse("v=3"), Err(TextSizeRefused::VAlign));
    assert_eq!(parse("v=-1"), Err(TextSizeRefused::VAlign));
    assert_eq!(parse("h=3"), Err(TextSizeRefused::HAlign));
    assert_eq!(parse("h=-1"), Err(TextSizeRefused::HAlign));
}

#[test]
fn every_key_together_and_the_last_of_a_repeated_key() {
    let t = parse("s=2:w=3:n=1:d=2:v=1:h=2").unwrap();
    assert_eq!(t.scale.get(), 2);
    assert_eq!(t.width.cells(), Some(3));
    assert_eq!(t.valign, SizeVAlign::Bottom);
    assert_eq!(t.halign, SizeHAlign::Centre);
    assert_eq!(parse("w=1:w=2").unwrap().width.cells(), Some(2));
    // A value is read after every key has been, so the last one is checked.
    assert_eq!(parse("w=9:w=2").unwrap().width.cells(), Some(2));
}

#[test]
fn what_the_table_does_not_define_is_refused_by_cause() {
    assert_eq!(parse("x=1"), Err(TextSizeRefused::UnknownKey));
    assert_eq!(parse("ss=1"), Err(TextSizeRefused::UnknownKey));
    assert_eq!(parse("S=1"), Err(TextSizeRefused::UnknownKey));
    assert_eq!(parse("w"), Err(TextSizeRefused::Malformed));
    assert_eq!(parse("=1"), Err(TextSizeRefused::Malformed));
    assert_eq!(parse("w=2;"), Err(TextSizeRefused::NotInteger));
    assert_eq!(parse("w="), Err(TextSizeRefused::NotInteger));
    assert_eq!(parse("w=-"), Err(TextSizeRefused::NotInteger));
    assert_eq!(parse("w=1.5"), Err(TextSizeRefused::NotInteger));
    assert_eq!(parse("w= 1"), Err(TextSizeRefused::NotInteger));
    assert_eq!(parse("w=+1"), Err(TextSizeRefused::NotInteger));
}

#[test]
fn a_huge_value_is_out_of_range_and_never_wraps() {
    // 2^32 + 1 and 2^64 + 1: a wrapping parse reads both as 1.
    assert_eq!(parse("w=4294967297"), Err(TextSizeRefused::Width));
    assert_eq!(parse("w=18446744073709551617"), Err(TextSizeRefused::Width));
    assert_eq!(
        parse("s=-18446744073709551615"),
        Err(TextSizeRefused::Scale)
    );
    let long = format!("w={}", "9".repeat(5000));
    assert_eq!(parse(&long), Err(TextSizeRefused::Width));
    assert_eq!(
        parse("w=0000000000000000000000002").unwrap().width.cells(),
        Some(2)
    );
}

#[test]
fn the_body_splits_at_the_first_semicolon() {
    assert_eq!(TextSize::split(b"w=2; "), Some((&b"w=2"[..], &b" "[..])));
    assert_eq!(TextSize::split(b";x"), Some((&b""[..], &b"x"[..])));
    assert_eq!(
        TextSize::split(b"w=1;a;b"),
        Some((&b"w=1"[..], &b"a;b"[..]))
    );
    assert_eq!(TextSize::split(b"w=1;"), Some((&b"w=1"[..], &b""[..])));
    assert_eq!(TextSize::split(b"w=1"), None);
    assert_eq!(TextSize::split(b""), None);
}

fn text(b: &[u8]) -> (Vec<u32>, usize) {
    let mut t = SizedText::new(b);
    let cps = t.by_ref().map(Codepoint::value).collect();
    (cps, t.dropped())
}

#[test]
fn the_text_is_decoded_as_written() {
    assert_eq!(text(b" "), (vec![0x20], 0));
    assert_eq!(text("ab\u{301}".as_bytes()), (vec![0x61, 0x62, 0x301], 0));
    assert_eq!(text("\u{1F408}".as_bytes()), (vec![0x1F408], 0));
    assert_eq!(text(b""), (vec![], 0));
}

#[test]
fn controls_in_the_text_are_left_out_and_counted() {
    // C0, DEL and C1 (U+0080..=U+009F, here as UTF-8).
    assert_eq!(text(b"a\x01b\x1fc\x7fd"), (vec![0x61, 0x62, 0x63, 0x64], 3));
    assert_eq!(
        text("a\u{80}b\u{9f}c".as_bytes()),
        (vec![0x61, 0x62, 0x63], 2)
    );
    // The neighbours of each range are kept.
    assert_eq!(text("\u{a0} ~".as_bytes()), (vec![0xA0, 0x20, 0x7E], 0));
}

#[test]
fn each_maximal_ill_formed_subpart_is_one_replacement() {
    let r = Codepoint::REPLACEMENT.value();
    // An overlong slash, a lone continuation, a cut three-byte form, a
    // surrogate and a form cut by the end of the text: one U+FFFD for each
    // maximal subpart. `C0` and a surrogate's `ED A0` begin no well-formed
    // sequence, so each of their bytes is a subpart of its own.
    assert_eq!(text(b"a\xc0\xafb"), (vec![0x61, r, r, 0x62], 0));
    assert_eq!(text(b"a\x80b"), (vec![0x61, r, 0x62], 0));
    assert_eq!(text(b"a\xe2\x86b"), (vec![0x61, r, 0x62], 0));
    assert_eq!(text(b"\xed\xa0\x80"), (vec![r, r, r], 0));
    assert_eq!(text(b"a\xe2\x86"), (vec![0x61, r], 0));
}
