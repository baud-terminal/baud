//! **SAPV, as the core keeps it**: `CSI Ps... SP ]` "is used to specify
//! one or more variants for the presentation of subsequent text"
//! (`[[source: ecma48]]` 8.3.108), each value folded into the rendition of
//! the characters written after it. What each variant draws is
//! `baud-render/tests/presentation_variants.rs`.

mod support;

use baud_core::*;
use baud_domain::{DecimalMark, Digits, Form, Mirroring, Shaping, Slant, Variants, Vowels};

const WS: WinSize = WinSize {
    ws_row: 4,
    ws_col: 20,
};

const BEH: &str = "\u{0628}";

/// The variants cell `(r, c)` was written under.
fn variants(t: &Term<'_>, r: usize, c: usize) -> Variants {
    let cell = t.row(Row::from_index(r)).expect("a row")[c];
    t.styles()[cell.style.index()]
        .expect("a live rendition")
        .variants()
}

fn sapv(values: &str) -> String {
    format!("\x1b[{values} ]")
}

/// **Text written after a value carries it, text before it does not**:
/// "the presentation of subsequent text".
#[test]
fn a_value_is_carried_by_the_text_written_after_it() {
    support::with_term(&WS, |t| {
        support::feed(t, format!("1{}2", sapv("2")).as_bytes());
        assert_eq!(variants(t, 0, 0), Variants::DEFAULT, "written before");
        assert_eq!(variants(t, 0, 1).digits(), Digits::ArabicIndic);
    });
}

/// **Every value but 12 sets what 8.3.108 says**, folded in order, and 0
/// returns the default presentation.
#[test]
fn each_value_sets_its_variant_and_0_cancels_them_all() {
    support::with_term(&WS, |t| {
        support::feed(t, format!("{}a", sapv("2;15;10;16;14;17")).as_bytes());
        let v = variants(t, 0, 0);
        assert_eq!(v.slant(), Slant::ByDirection);
        assert_eq!(v.digits(), Digits::ArabicIndic);
        assert_eq!(v.paired(), Mirroring::Cancelled);
        assert_eq!(v.mathematical(), Mirroring::Cancelled);
        assert_eq!(v.decimal_mark(), DecimalMark::Comma);
        assert_eq!(v.vowels(), Vowels::Hidden);
        assert_eq!(v.shaping(), Shaping::Unligated);
        support::feed(t, format!("{}b", sapv("1;3;9;11;18")).as_bytes());
        let v = variants(t, 0, 1);
        assert_eq!(v.digits(), Digits::Stored);
        assert_eq!(v.paired(), Mirroring::Mirrored);
        assert_eq!(v.mathematical(), Mirroring::Cancelled, "4 not sent");
        assert_eq!(v.decimal_mark(), DecimalMark::FullStop);
        assert_eq!(v.vowels(), Vowels::Marked);
        assert_eq!(v.shaping(), Shaping::Stored);
        support::feed(t, format!("{}c{}d", sapv("4;19"), sapv("0")).as_bytes());
        assert_eq!(variants(t, 0, 2).mathematical(), Mirroring::Mirrored);
        assert_eq!(variants(t, 0, 2).shaping(), Shaping::StoredButDigits);
        assert_eq!(variants(t, 0, 3), Variants::DEFAULT, "0");
        assert_eq!(variants(t, 0, 3).slant(), Slant::AsFace, "0 cancels 17");
        support::feed(t, format!("{}e{}f", sapv("2"), sapv("")).as_bytes());
        assert_eq!(variants(t, 0, 5), Variants::DEFAULT, "omitted is 0");
    });
}

/// **`SGR 0` leaves them**: SAPV is no `SGR`, and SGR 0 "cancels the effect
/// of any preceding occurrence of SGR" (8.3.117).
#[test]
fn sgr_0_leaves_the_variants() {
    support::with_term(&WS, |t| {
        support::feed(t, format!("{}\x1b[1;31m\x1b[0ma", sapv("2")).as_bytes());
        assert_eq!(variants(t, 0, 0).digits(), Digits::ArabicIndic);
    });
}

/// **A form is "the following graphic character"'s alone**: the next
/// letter takes it, however it is written -- one at a time, in a run of
/// two-byte characters, after a control -- and the one after it does not.
#[test]
fn a_form_is_the_following_characters_alone() {
    for (body, why) in [
        (format!("{BEH}{BEH}"), "two letters"),
        (BEH.repeat(12), "a run of two-byte letters"),
        (format!("\r\x1b[1;1H{BEH}{BEH}"), "after a control"),
    ] {
        support::with_term(&WS, |t| {
            support::feed(t, format!("{}{body}", sapv("6")).as_bytes());
            assert_eq!(variants(t, 0, 0).form(), Some(Form::Initial), "{why}");
            assert_eq!(variants(t, 0, 1).form(), None, "{why}: the next");
            support::feed(t, b"x");
            let row = t.row(Row::ZERO).unwrap();
            let x = row
                .iter()
                .rposition(|c| {
                    c.content
                        .as_codepoint()
                        .is_some_and(|cp| cp.value() == u32::from('x'))
                })
                .unwrap();
            assert_eq!(variants(t, 0, x).form(), None, "{why}: later text");
        });
    }
}

/// **A form given to a character that takes none is spent on it**: the
/// character is written as stored, one alone or a run of ASCII, and the
/// letter after it takes no form either.
#[test]
fn a_form_is_spent_on_a_character_that_takes_none() {
    for body in ["a", "abcdefghijklmn"] {
        support::with_term(&WS, |t| {
            support::feed(t, format!("{}{body}{BEH}", sapv("6")).as_bytes());
            assert_eq!(variants(t, 0, 0).form(), None, "{body}");
            assert_eq!(
                variants(t, 0, body.len()).form(),
                None,
                "{body}: the letter"
            );
        });
    }
}

/// **A form a letter took is not taken again**: a SAPV, a `DECSC` and
/// `DECRC`, or an `XTPUSHSGR` and `XTPOPSGR` between it and the next
/// character leave the next character without it.
#[test]
fn a_form_taken_is_not_taken_again() {
    for (between, why) in [
        (sapv("2"), "a SAPV"),
        ("\x1b7\x1b8".to_string(), "DECSC and DECRC"),
        ("\x1b[#{\x1b[#}".to_string(), "XTPUSHSGR and XTPOPSGR"),
    ] {
        support::with_term(&WS, |t| {
            support::feed(t, format!("{}{BEH}{between}{BEH}", sapv("6")).as_bytes());
            assert_eq!(variants(t, 0, 0).form(), Some(Form::Initial), "{why}");
            assert_eq!(variants(t, 0, 1).form(), None, "{why}: the next");
        });
    }
}

/// **The same at any read size**: a form waiting across a read boundary
/// is taken by the first character of the next read.
#[test]
fn a_form_waits_across_reads() {
    let bytes = format!("{}{BEH}{BEH}a", sapv("8"));
    support::with_term(&WS, |t| {
        for b in bytes.as_bytes() {
            support::feed(t, &[*b]);
        }
        assert_eq!(variants(t, 0, 0).form(), Some(Form::Final));
        assert_eq!(variants(t, 0, 1).form(), None);
    });
}

/// **21 holds a form for every following character until cancelled, and
/// 22 makes the form in force the next character's only.**
#[test]
fn value_21_holds_forms_and_22_lets_them_go() {
    support::with_term(&WS, |t| {
        support::feed(
            t,
            format!("{}{BEH}{BEH}{BEH}{}{BEH}{BEH}", sapv("21;7"), sapv("22")).as_bytes(),
        );
        for c in 0..3 {
            assert_eq!(variants(t, 0, c).form(), Some(Form::Medial), "held, {c}");
        }
        assert_eq!(variants(t, 0, 3).form(), Some(Form::Medial), "the next one");
        assert_eq!(variants(t, 0, 4).form(), None, "and no more");
    });
}

/// **12 is refused and counted**, a cell grid having no column for a
/// vowel to follow its letter in, and **17 is not**: it sets the slant. A
/// value 8.3.108 does not define is counted in `unsupported_param`.
/// Neither 12 nor an undefined value changes the rendition.
#[test]
fn value_12_is_refused_and_an_undefined_value_counted() {
    support::with_term(&WS, |t| {
        let (before, refused) = (t.unsupported_param, t.variants_refused());
        support::feed(t, sapv("23").as_bytes());
        assert_eq!(t.variants_refused(), refused, "23 is not refused");
        assert_eq!(t.unsupported_param, before + 1, "but counted");
        support::feed(t, sapv("12").as_bytes());
        let twelve = t.variants_refused();
        assert_ne!(twelve, refused, "12");
        support::feed(t, format!("{}a", sapv("12;23")).as_bytes());
        assert_eq!(variants(t, 0, 0), Variants::DEFAULT);
        assert!(!t.variants_seen(), "nothing but the default selected");
        let again = t.variants_refused();
        assert_ne!(again, twelve, "12 again");
        support::feed(t, format!("{}b", sapv("17")).as_bytes());
        assert_eq!(t.variants_refused(), again, "17 is not refused");
        assert_eq!(variants(t, 0, 1).slant(), Slant::ByDirection, "17");
        assert!(t.variants_seen());
    });
}

/// **`DECSTR` and `RIS` return the default presentation** (Jev's lean,
/// RESET), held forms let go; `DECSC` and `DECRC` carry the variants
/// with the rendition.
#[test]
fn the_resets_return_the_default_and_decrc_restores_the_variants() {
    for reset in ["\x1b[!p", "\x1bc"] {
        support::with_term(&WS, |t| {
            support::feed(t, format!("{}{reset}a", sapv("2;21;5")).as_bytes());
            assert_eq!(variants(t, 0, 0), Variants::DEFAULT, "{reset:?}");
        });
    }
    support::with_term(&WS, |t| {
        support::feed(
            t,
            format!("{}\x1b7{}\x1b8a", sapv("2"), sapv("0")).as_bytes(),
        );
        assert_eq!(variants(t, 0, 0).digits(), Digits::ArabicIndic);
    });
}

/// **DECALN keeps the variants as `SGR 0` does**: it sets the rendition
/// normal (`[[source: dec-std-070]]`, DECALN), and SAPV's variants are no
/// rendition of DEC's. The alignment pattern is in the default rendition,
/// and a form waiting before it still waits after.
#[test]
fn decaln_keeps_the_variants() {
    support::with_term(&WS, |t| {
        support::feed(t, format!("{}\x1b[1m\x1b#83", sapv("2")).as_bytes());
        assert_eq!(variants(t, 0, 0).digits(), Digits::ArabicIndic);
        assert_eq!(variants(t, 0, 1), Variants::DEFAULT, "the pattern");
        let cell = t.row(Row::ZERO).expect("a row")[0];
        let w = t.styles()[cell.style.index()].expect("a live rendition");
        assert!(!w.attrs().has(Attrs::BOLD), "the rendition normal");
    });
    support::with_term(&WS, |t| {
        support::feed(t, format!("{}\x1b#8{BEH}{BEH}", sapv("6")).as_bytes());
        assert_eq!(variants(t, 0, 0).form(), Some(Form::Initial));
        assert_eq!(variants(t, 0, 1).form(), None, "the next letter's own");
    });
}

/// **The `SP ]` is SAPV in the standard namespace alone**: a private
/// marker makes another sequence, which leaves the rendition.
#[test]
fn a_private_marker_is_not_sapv() {
    support::with_term(&WS, |t| {
        support::feed(t, b"\x1b[?2 ]a");
        assert_eq!(variants(t, 0, 0), Variants::DEFAULT);
    });
}

/// **DECRQSS does not answer SAPV**: no DEC or xterm text lists it among
/// the controls DECRQSS reports, so it is answered as SCP is, as an
/// unknown request, `DCS 0 $ r ST`.
#[test]
fn decrqss_answers_sapv_as_invalid() {
    support::with_term(&WS, |t| {
        support::feed(t, format!("{}\x1bP$q ]\x1b\\", sapv("2")).as_bytes());
        assert_eq!(t.reply(), b"\x1bP0$r\x1b\\");
    });
}

/// **A new form asked right after a letter took one is the next letter's**:
/// the spent form leaves before the new one is folded in, so it is not
/// taken away with it.
#[test]
fn a_new_form_after_a_taken_one_is_the_next_letters() {
    support::with_term(&WS, |t| {
        support::feed(
            t,
            format!("{}{BEH}{}{BEH}{BEH}", sapv("6"), sapv("7")).as_bytes(),
        );
        assert_eq!(variants(t, 0, 0).form(), Some(Form::Initial));
        assert_eq!(variants(t, 0, 1).form(), Some(Form::Medial));
        assert_eq!(variants(t, 0, 2).form(), None);
    });
}
