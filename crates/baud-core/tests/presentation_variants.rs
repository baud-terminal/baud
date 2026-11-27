//! **SAPV, as the core keeps it**: `CSI Ps... SP ]` "is used to specify
//! one or more variants for the presentation of subsequent text"
//! (`[[source: ecma48]]` 8.3.108), each value folded into the rendition of
//! the characters written after it. What each variant draws is
//! `crates/baud-render/tests/presentation_variants.rs`.

mod support;

use baud_core::*;
use baud_domain::{
    DecimalMark, Digits, ErrorCharacter, Form, Mirroring, Shaping, Slant, StyleId, SubstituteMark,
    Variants, Vowels,
};

const WS: WinSize = WinSize {
    ws_row: 4,
    ws_col: 20,
};

const BEH: &str = "\u{0628}";

/// The variants cell `(r, c)` was written under.
fn variants(t: &Term<'_>, r: usize, c: usize) -> Variants {
    let cell = t.row(Row::from_index(r)).expect("a row")[c];
    t.styles().one().slots()[cell.style.index()]
        .word()
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

/// **`DECSTR` and `RIS` return the default presentation**, held forms
/// let go; `DECSC` and `DECRC` carry the variants with the rendition.
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
        let w = t.styles().one().slots()[cell.style.index()]
            .word()
            .expect("a live rendition");
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

/// A case of [`a_form_goes_to_the_next_character_by_any_writer`]: what it
/// is, its bytes, and the forms of the first two cells.
type Case = (&'static str, Vec<u8>, [Option<Form>; 2]);

/// **A form goes to the next character whichever writer writes it**,
/// one character at a time while it waits, and is left by a mark that
/// joins the letter before: a replacement for a byte that begins nothing,
/// a GR byte under ISO 8859-1 and the ASCII after it there, a `REP`'s
/// copy, a letter under iTerm2's Unicode 8 widths, a `SUB`'s mark, and
/// a fatha after the control, which starts a cell of its own (`[[source:
/// uax29]]` GB4). A fatha on the letter that took the form joins its cell
/// and leaves the form to the next character to take out.
#[test]
fn a_form_goes_to_the_next_character_by_any_writer() {
    const FATHA: &str = "\u{064e}";
    let cases: [Case; 8] = [
        (
            "a replacement",
            [sapv("6").as_bytes(), b"\xff", BEH.as_bytes()].concat(),
            [None, None],
        ),
        (
            "a GR byte under ISO 8859-1",
            [b"\x1b%@".as_slice(), sapv("6").as_bytes(), b"\xe9a"].concat(),
            [None, None],
        ),
        (
            "ASCII under ISO 8859-1",
            [b"\x1b%@".as_slice(), sapv("6").as_bytes(), b"ab"].concat(),
            [None, None],
        ),
        (
            "a REP's copy",
            [
                BEH.as_bytes(),
                sapv("6").as_bytes(),
                b"\x1b[b",
                BEH.as_bytes(),
            ]
            .concat(),
            [None, Some(Form::Initial)],
        ),
        (
            "Unicode 8's widths",
            [
                b"\x1b]1337;UnicodeVersion=8\x07".as_slice(),
                sapv("6").as_bytes(),
                BEH.as_bytes(),
                BEH.as_bytes(),
            ]
            .concat(),
            [Some(Form::Initial), None],
        ),
        (
            "a SUB's mark",
            [sapv("6").as_bytes(), b"\x1a", BEH.as_bytes()].concat(),
            [None, None],
        ),
        (
            "a mark after the control",
            [&sapv("6"), FATHA, BEH].concat().into_bytes(),
            [None, None],
        ),
        (
            "a mark on the letter that took it",
            [&sapv("6"), BEH, FATHA, BEH].concat().into_bytes(),
            [Some(Form::Initial), None],
        ),
    ];
    for (why, bytes, want) in cases {
        for size in [1, 2, 3, bytes.len()] {
            support::with_term(&WS, |t| {
                t.set_substitute_mark(SubstituteMark::Drawn(ErrorCharacter::DEC));
                for chunk in bytes.chunks(size) {
                    support::feed(t, chunk);
                }
                let got = [variants(t, 0, 0).form(), variants(t, 0, 1).form()];
                assert_eq!(got, want, "{why} at reads of {size}");
                assert_eq!(variants(t, 0, 2).form(), None, "{why}: the one after");
            });
        }
    }
}

/// **After the character a form went to, the text goes by the text
/// path again**: the screen past that cell is the one the same text
/// writes with no SAPV before it, a run of ASCII, of CJK and of two-byte
/// letters, at every read size.
#[test]
fn after_the_form_the_screen_is_the_text_path_s() {
    let text = format!(
        "{BEH}hello world, {}{}x",
        "\u{6f22}".repeat(5),
        "\u{00c4}".repeat(9)
    );
    let plain = support::with_term(&WS, |t| {
        support::feed(t, text.as_bytes());
        support::screen(t)
    });
    let bytes = format!("{}{text}", sapv("6"));
    for size in [1, 2, 3, 5, bytes.len()] {
        support::with_term(&WS, |t| {
            for chunk in bytes.as_bytes().chunks(size) {
                support::feed(t, chunk);
            }
            assert_eq!(variants(t, 0, 0).form(), Some(Form::Initial), "{size}");
            let got = support::screen(t);
            assert_eq!(got[1..], plain[1..], "reads of {size}");
            assert_eq!(got[0].content, plain[0].content, "reads of {size}");
        });
    }
}

/// **A rendition with variants keeps one slot however the table turns
/// over**: it lives on its own key's path, which the inline probe does not
/// walk, so it is found there at a miss, and the sweeps that free the
/// renditions written over and over around it keep that path. Written
/// again after each thousand, it is the slot it was, no other slot holds
/// it, and no empty slot lies between its key's bucket and it.
#[test]
fn a_rendition_with_variants_keeps_one_slot_through_sweeps() {
    support::with_term(&WS, |t| {
        support::feed(t, format!("{}\x1b[31ma", sapv("2")).as_bytes());
        let first = t.row(Row::ZERO).expect("a row")[0].style;
        let word = t.styles().one().slots()[first.index()]
            .word()
            .expect("a live rendition");
        for round in 0..8usize {
            let mut churn = Vec::new();
            for i in 0..1000usize {
                let n = round * 1000 + i;
                churn.extend(
                    format!(
                        "\x1b[3;1H\x1b[38;2;{};{};{}mx",
                        n & 255,
                        (n >> 8) & 255,
                        (n * 7) & 255
                    )
                    .as_bytes(),
                );
            }
            support::feed(t, &churn);
            support::feed(t, b"\x1b[1;2H\x1b[0;31mb");
            let again = t.row(Row::ZERO).expect("a row")[1].style;
            assert_eq!(again, first, "round {round}");
            let held = t
                .styles()
                .one()
                .slots()
                .iter()
                .filter(|w| **w == baud_domain::StyleSlot::Word(word))
                .count();
            assert_eq!(held, 1, "round {round}");
            // On its own key's path: no empty slot from its bucket to it.
            let len = t.styles().one().len();
            let mut k = StyleId::bucket(word, len);
            while k != first.index() {
                assert!(
                    t.styles().one().slots()[k].word().is_some(),
                    "round {round}: slot {k} empty"
                );
                k = (k + 1) & (len - 1);
            }
        }
    });
}
