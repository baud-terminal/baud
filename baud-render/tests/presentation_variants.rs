//! **What SAPV's variants draw** (`[[source: ecma48]]` 8.3.108): each value
//! acts on the characters written after it, as the core keeps them
//! (`baud-core/tests/presentation_variants.rs`). Every expectation is the
//! drawn row, `.` an erased cell, or a cell's joined form.

mod support;

use baud_core::{Owned, Term};
use baud_domain::{BytePos, Col, Cols, JoiningForm, PackedCell, ReadLen, Row, Rows, WinSize};
use baud_font::Face;
use baud_render::{Blend, Damage, PxSize, Renderer, Reorder, Stack, Theme};
use support::{Canvas, font};

const WS: WinSize = WinSize {
    ws_row: 3,
    ws_col: 10,
};

fn feed(t: &mut Term<'_>, bytes: &str) {
    t.write_at(BytePos::ZERO, bytes.as_bytes());
    t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
}

/// Row 0 as drawn, and the laid-out maps, after `bytes`.
fn drawn(bytes: &str) -> (String, Reorder) {
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, bytes);
    let mut o = Reorder::new();
    o.lay_out(&t, Rows::ZERO);
    let vp = t.view(Rows::ZERO);
    let mut out = vec![PackedCell::EMPTY; vp.cols().index()];
    o.orders().permute(Row::ZERO, vp.row(Row::ZERO), &mut out);
    let s = out
        .iter()
        .map(|c| match c.content {
            x if x.is_empty() => '.',
            x => x
                .as_codepoint()
                .and_then(|cp| char::from_u32(cp.value()))
                .unwrap_or('?'),
        })
        .collect();
    (s, o)
}

/// The joined form of the cell written at logical column `c` of row 0.
fn form(o: &Reorder, c: usize) -> Option<JoiningForm> {
    let v = o.orders().visual_col(Row::ZERO, Col::at_index(c));
    o.orders().joined(Row::ZERO, v).map(|j| j.form)
}

/// Whether the cell at logical column `c` is drawn in a lam-alef ligature.
fn ligated(o: &Reorder, c: usize) -> bool {
    let v = o.orders().visual_col(Row::ZERO, Col::at_index(c));
    o.orders()
        .joined(Row::ZERO, v)
        .is_some_and(|j| j.pair.is_some())
}

/// **Value 2 draws the decimal digits as the Arabic script's**,
/// U+0660-U+0669, "the Hindi symbols"; 18 draws them as stored, 19 as 2
/// says, and 1 and 0 as stored. Text written before keeps its digits.
#[test]
fn value_2_draws_arabic_indic_digits() {
    assert_eq!(drawn("1\x1b[2 ]23").0, "1\u{662}\u{663}.......");
    assert_eq!(drawn("\x1b[2;18 ]23").0, "23........", "18");
    assert_eq!(drawn("\x1b[2;19 ]23").0, "\u{662}\u{663}........", "19");
    assert_eq!(drawn("\x1b[2 ]2\x1b[1 ]3").0, "\u{662}3........", "1");
    assert_eq!(drawn("23").0, "23........", "the control");
}

/// **Value 10 draws a decimal number's FULL STOP as a COMMA**: the one
/// between two digits, not one beside a letter; 9 as stored.
#[test]
fn value_10_draws_a_decimal_mark_between_digits_as_a_comma() {
    assert_eq!(drawn("\x1b[10 ]3.14 a.b").0, "3,14 a.b..");
    assert_eq!(drawn("\x1b[10;9 ]3.14").0, "3.14......", "9");
    assert_eq!(
        drawn("\x1b[10 ].5 5.").0,
        ".5 5......",
        "no digit on one side"
    );
}

/// **Values 5 to 8 draw the following letter in the form asked**, an
/// isolated beh initial; a form a letter has not got is the nearest it
/// has, an alef asked for its medial form drawn final; and the letter
/// after it is drawn as its neighbours give it.
#[test]
fn a_form_asked_draws_the_following_letter_in_it() {
    let (_, o) = drawn("\u{628}");
    assert_eq!(form(&o, 0), None, "the control: an isolated beh, R7");
    let (_, o) = drawn("\x1b[6 ]\u{628}");
    assert_eq!(form(&o, 0), Some(JoiningForm::Initial));
    let (_, o) = drawn("\x1b[7 ]\u{627}");
    assert_eq!(form(&o, 0), Some(JoiningForm::Final), "alef has no medial");
    let (_, o) = drawn("\x1b[5 ]\u{628}\u{628}");
    assert_eq!(form(&o, 0), Some(JoiningForm::Isolated), "asked");
    assert_eq!(form(&o, 1), Some(JoiningForm::Final), "joined to the first");
}

/// **Values 18 and 19 draw Arabic as stored**, no letter joined; 13 and 0
/// join it again.
#[test]
fn values_18_and_19_draw_arabic_unjoined() {
    let (_, o) = drawn("\u{628}\u{628}");
    assert_eq!(form(&o, 0), Some(JoiningForm::Initial), "the control");
    for v in ["18", "19"] {
        let (_, o) = drawn(&format!("\x1b[{v} ]\u{628}\u{628}"));
        assert_eq!((form(&o, 0), form(&o, 1)), (None, None), "{v}");
    }
    let (_, o) = drawn("\x1b[18;13 ]\u{628}\u{628}");
    assert_eq!(form(&o, 0), Some(JoiningForm::Initial), "13");
}

/// **Value 14 joins Arabic without the lam-alef ligature**: the lam and
/// the alef each in its joined form, as 13, the default, draws them as one.
#[test]
fn value_14_draws_a_lam_and_an_alef_apart() {
    let (_, o) = drawn("\u{644}\u{627}");
    assert!(
        ligated(&o, 0) && ligated(&o, 1),
        "the control: the ligature"
    );
    let (_, o) = drawn("\x1b[14 ]\u{644}\u{627}");
    assert!(!ligated(&o, 0) && !ligated(&o, 1), "14");
    assert_eq!(form(&o, 0), Some(JoiningForm::Initial));
    assert_eq!(form(&o, 1), Some(JoiningForm::Final));
}

/// What one cell of a word reads its neighbour as: its letter and form.
type Read = (char, Option<JoiningForm>);

/// Row 0 laid out after `bytes`, each joined cell kept with two cells of
/// its word either side: the joined form of each of the cells written at
/// logical columns 0 to 2, and the word around the cell at each, before
/// it and after it, each neighbour read as its letter and form.
#[allow(clippy::type_complexity)]
fn with_words(bytes: &str) -> Vec<(Option<JoiningForm>, (Vec<Read>, Vec<Read>))> {
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, bytes);
    let mut o = Reorder::new();
    o.set_reach(baud_font::bytes::Count::of(2_u32));
    o.lay_out(&t, Rows::ZERO);
    let read = |w: &baud_render::WordCell| {
        let letter = t
            .base_of(w.cell)
            .drawn()
            .as_codepoint()
            .and_then(|c| char::from_u32(c.value()))
            .unwrap_or('?');
        (letter, w.form)
    };
    (0..3)
        .map(|c| {
            let v = o.orders().visual_col(Row::ZERO, Col::at_index(c));
            match o.orders().joined(Row::ZERO, v) {
                Some(j) => {
                    let (before, after) = o.orders().around(Row::ZERO, &j);
                    (
                        Some(j.form),
                        (
                            before.iter().map(read).collect(),
                            after.iter().map(read).collect(),
                        ),
                    )
                }
                None => (None, (Vec::new(), Vec::new())),
            }
        })
        .collect()
}

/// **A form asked wins over the forms joining finds from the word**
/// (`[[source: ecma48]]` 8.3.108's 5-8 and 18, over
/// `[[source: unicode-ch9]]` 9.3's S1-S3): a Syriac alaph after he, fin2
/// by S2, is drawn final under 8 and isolated under 5; one inside a word,
/// med2, is drawn final under 8. The letters around it read it in the
/// form asked -- the beth before it, shaped with its word, reads a final
/// alaph, not med2 -- and under 18 as stored. The asked letter keeps its
/// word, joined or not: an alef after a dal and an alaph after he inside
/// a word, which no letter joins, and an alef between two dals, in a word
/// none of whose letters join; with BDSM reset on a row that also holds
/// Mongolian, read left to right, the alef's word is the one read right
/// to left.
#[test]
fn a_form_asked_wins_over_the_word_and_the_word_reads_it() {
    use JoiningForm::{Final, Final2, Initial, Isolated, Medial2};
    let (alaph, beth, he) = ('\u{0710}', '\u{0712}', '\u{0717}');
    let alaph_form = |bytes: String| with_words(&bytes)[1].0;
    assert_eq!(alaph_form(format!("{he}{alaph}")), Some(Final2), "S2");
    assert_eq!(alaph_form(format!("{he}\x1b[8 ]{alaph}")), Some(Final), "8");
    assert_eq!(
        alaph_form(format!("{he}\x1b[5 ]{alaph}")),
        Some(Isolated),
        "5"
    );
    let bab = |sapv: &str| with_words(&format!("{beth}{sapv}{alaph}\x1b[0 ]{beth}"));
    let plain = bab("");
    assert_eq!(plain[1].0, Some(Medial2), "the control");
    assert_eq!(
        plain[0].1.1,
        [(alaph, Some(Medial2)), (beth, None)],
        "the control: the beth reads a med2 alaph"
    );
    let asked = bab("\x1b[8 ]");
    assert_eq!(asked[1].0, Some(Final), "8 inside a word");
    assert_eq!(
        asked[0].1.1,
        [(alaph, Some(Final)), (beth, None)],
        "the beth reads the alaph asked"
    );
    assert_eq!(
        asked[1].1,
        (vec![(beth, Some(Initial))], vec![(beth, None)]),
        "the alaph asked keeps its word"
    );
    let (beh, dal, alef, lam) = ('\u{0628}', '\u{062F}', '\u{0627}', '\u{0644}');
    let alef_asked = with_words(&format!("{beh}{dal}\x1b[8 ]{alef}\x1b[0 ]{lam}"))[2].clone();
    assert_eq!(
        alef_asked,
        (
            Some(Final),
            (
                vec![(beh, Some(Initial)), (dal, Some(Final))],
                vec![(lam, None)]
            )
        ),
        "an alef after a dal, asked final, keeps its word"
    );
    let alone = with_words(&format!("{dal}\x1b[7 ]{alef}\x1b[0 ]{dal}"))[1].clone();
    assert_eq!(
        alone,
        (Some(Final), (vec![(dal, None)], vec![(dal, None)])),
        "in a word no letter of which joins, the letter asked keeps it"
    );
    let after_he = with_words(&format!("{beth}{he}\x1b[8 ]{alaph}\x1b[0 ]{beth}"))[2].clone();
    assert_eq!(after_he.0, Some(Final), "8 after he inside a word");
    assert_eq!(
        after_he.1.0,
        [(beth, Some(Initial)), (he, Some(Final))],
        "the alaph asked after he keeps the word before it"
    );
    let mong = '\u{1820}';
    let reversed = with_words(&format!(
        "\x1b[8l{dal}\x1b[7 ]{alef}\x1b[0 ]{dal}{mong}{mong}"
    ))[1]
        .clone();
    assert_eq!(
        reversed,
        (
            Some(Final),
            (vec![(mong, None), (dal, None)], vec![(dal, None)])
        ),
        "BDSM reset beside Mongolian: the alef's word read right to left"
    );
    let stored = bab("\x1b[18 ]");
    assert_eq!(stored[1].0, None, "18");
    assert_eq!(
        stored[0].1.1,
        [(alaph, None), (beth, None)],
        "the beth reads the alaph as stored"
    );
}

/// **A letter's word reads the digits and the decimal mark as SAPV draws
/// them** (`[[source: ecma48]]` 8.3.108, values 2 and 10): the cells of
/// the word a joined letter is shaped with hold the Arabic-Indic digit
/// and the comma drawn, not the digit and the FULL STOP stored, so a
/// face's contextual lookups read what is drawn beside the letter. With
/// BDSM reset, where a row is one run and its digits are in its letters'
/// words; a FULL STOP that is not between two digits stays one.
#[test]
fn a_word_reads_its_digits_as_drawn() {
    let beh = '\u{0628}';
    let word = |bytes: String| with_words(&bytes)[1].1.0.clone();
    assert_eq!(
        word(format!("\x1b[8l{beh}{beh}3.1")),
        [('.', None), ('3', None)],
        "the control: as stored"
    );
    assert_eq!(
        word(format!("\x1b[8l\x1b[2;10 ]{beh}{beh}3.1")),
        [(',', None), ('\u{0663}', None)],
        "2 and 10: as drawn"
    );
    assert_eq!(
        word(format!("\x1b[8l\x1b[2;10 ]{beh}{beh}3.")),
        [('.', None), ('\u{0663}', None)],
        "10: a FULL STOP after a digit alone"
    );
}

/// **With BDSM reset, value 15 leaves a right-to-left path's paired
/// characters as stored**, the line reversed and its brackets not
/// swapped, and 3 swaps them again; with BDSM set L4 still mirrors them.
#[test]
fn value_15_unmirrors_a_reversed_line_with_bdsm_reset() {
    let rtl = "\x1b[8l\x1b[2 k";
    assert_eq!(drawn(&format!("{rtl}(a)")).0, ".......(a)", "the control");
    assert_eq!(drawn(&format!("{rtl}\x1b[15 ](a)")).0, ".......)a(", "15");
    assert_eq!(
        drawn(&format!("{rtl}\x1b[15;3 ](a)")).0,
        ".......(a)",
        "3 after it"
    );
    assert_eq!(
        drawn("\x1b[15 ]\u{5D0}(\u{5D1})").0,
        "......(\u{5D1})\u{5D0}",
        "BDSM set: L4 mirrors them"
    );
}

/// **Value 16 hides the vowels**: a Hebrew bet with a sheva (a vowel) and
/// a dagesh (not one) paints as the bet and dagesh alone, and without the
/// value it does not.
#[test]
fn value_16_paints_a_letter_without_its_vowel() {
    let data = std::fs::read(format!(
        "{}/../baud-app/fonts/built/Regular.ttf",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap();
    let mono = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).unwrap(), Face::parse(&mono).unwrap()];
    let paint = |bytes: &str| {
        let mut r = Renderer::new(
            Stack::new(&faces).unwrap(),
            PxSize::of(14.0).unwrap(),
            Theme::dark(),
            Blend::Native,
            Cols::of_index(usize::from(WS.ws_col)),
            Rows::of_index(usize::from(WS.ws_row)),
        )
        .unwrap();
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        feed(&mut t, &format!("\x1b[?25l{bytes}"));
        let mut c = Canvas::for_renderer(&r);
        {
            let mut target = c.target();
            r.render(&mut t, &mut target, Damage::Full);
        }
        c.pixels
    };
    let bare = paint("\u{5D1}\u{5BC}");
    let vowelled = paint("\u{5D1}\u{5BC}\u{5B0}");
    assert_ne!(bare, vowelled, "the premise: the sheva draws");
    assert_eq!(paint("\x1b[16 ]\u{5D1}\u{5BC}\u{5B0}"), bare, "16");
    assert_eq!(paint("\x1b[16;11 ]\u{5D1}\u{5BC}\u{5B0}"), vowelled, "11");
}

/// The built faces, regular and italic: the italic face is what an italic
/// leans by, and the regular one draws the upright glyph SAPV 17 shears.
fn built(name: &str) -> Vec<u8> {
    std::fs::read(format!(
        "{}/../baud-app/fonts/built/{name}.ttf",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap()
}

/// A frame of `bytes`, the cursor hidden, on the built regular and italic
/// faces, and its width.
fn italic_frame(bytes: &str) -> (Vec<u32>, usize) {
    use baud_render::{FaceRole, FaceStyle};
    let (regular, italic) = (built("Regular"), built("Italic"));
    let faces = [
        Face::parse(&regular).unwrap(),
        Face::parse(&italic).unwrap(),
    ];
    let roles = [FaceRole::Fallback, FaceRole::Style(FaceStyle::Italic)];
    let mut r = Renderer::new(
        Stack::with_roles(&faces, &roles).unwrap(),
        PxSize::of(28.0).unwrap(),
        Theme::dark(),
        Blend::Native,
        Cols::of_index(usize::from(WS.ws_col)),
        Rows::of_index(usize::from(WS.ws_row)),
    )
    .unwrap();
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, &format!("\x1b[?25l{bytes}"));
    let mut c = Canvas::for_renderer(&r);
    {
        let mut target = c.target();
        r.render(&mut t, &mut target, Damage::Full);
    }
    let w = c.width;
    (c.pixels, w)
}

/// **How far a frame's ink leans**, in pixels: the mean column of its ink
/// in the top third of the rows that hold any, less the bottom third's --
/// negative for ink that leans left. The background is the last pixel,
/// which no text reaches.
fn lean_of((pixels, w): &(Vec<u32>, usize)) -> f32 {
    let bg = *pixels.last().unwrap();
    let rows: Vec<f32> = pixels
        .chunks(*w)
        .filter_map(|row| {
            let ink: Vec<usize> = (0..row.len()).filter(|&x| row[x] != bg).collect();
            (!ink.is_empty()).then(|| ink.iter().sum::<usize>() as f32 / ink.len() as f32)
        })
        .collect();
    let third = (rows.len() / 3).max(1);
    let mean = |r: &[f32]| r.iter().sum::<f32>() / r.len() as f32;
    mean(&rows[..third]) - mean(&rows[rows.len() - third..])
}

/// **Value 17 slants a right-to-left italic to the left**, "when the string
/// direction is right-to-left, the italicized characters are slanted to the
/// left": a Hebrew vav, italic, leans left of its upright glyph under 17,
/// where without it the italic face draws it leaning right. Left to right,
/// and on a character not italic, 17 changes nothing; and 0 cancels it.
#[test]
fn value_17_slants_right_to_left_italics_to_the_left() {
    let vav = "\u{5D5}";
    let upright = italic_frame(vav);
    let italic = italic_frame(&format!("\x1b[3m{vav}"));
    let slanted = italic_frame(&format!("\x1b[3m\x1b[17 ]{vav}"));
    assert!(
        lean_of(&italic) > lean_of(&upright),
        "the premise: the italic face leans right, {} against {}",
        lean_of(&italic),
        lean_of(&upright)
    );
    assert!(
        lean_of(&slanted) < lean_of(&upright) - 1.0,
        "17 leans left of upright: {} against {}",
        lean_of(&slanted),
        lean_of(&upright)
    );
    assert!(
        italic_frame(&format!("\x1b[17 ]{vav}")).0 == upright.0,
        "not italic"
    );
    assert!(
        italic_frame("\x1b[3m\x1b[17 ]il").0 == italic_frame("\x1b[3mil").0,
        "left to right"
    );
    assert!(
        italic_frame(&format!("{vav} \x1b[3m\x1b[17 ]il")).0
            == italic_frame(&format!("{vav} \x1b[3mil")).0,
        "left to right inside a right-to-left paragraph, at level 2"
    );
    assert!(
        italic_frame(&format!("\x1b[3m\x1b[17;0 ]{vav}")).0 == italic.0,
        "0 cancels it"
    );
    // With BDSM reset, a right-to-left path is the string direction.
    let path = "\x1b[8l\x1b[2 k";
    let el = lean_of(&italic_frame(&format!("{path}l")));
    let leant = lean_of(&italic_frame(&format!("{path}\x1b[3m\x1b[17 ]l")));
    assert!(
        leant < el - 1.0,
        "a right-to-left path: {leant} against {el}"
    );
}

/// **Under DECRLM the letters join as written, right to left, and take the
/// forms SAPV asks**: a row DECRLM wrote is drawn as its cells hold, the
/// first letter sent in the last column, so it is read from the right as
/// the program wrote it, with BDSM set and reset; the letter after a form
/// asked is drawn as its neighbours give it, as on a row written left to
/// right.
#[test]
fn under_decrlm_letters_join_as_written_and_take_the_forms_asked() {
    use JoiningForm::{Final, Initial, Isolated};
    for bdsm in ["", "\x1b[8l"] {
        let at = |b: &str| {
            let (_, o) = drawn(&format!("{bdsm}\x1b[?34h\x1b[1;10H{b}"));
            (form(&o, 9), form(&o, 8))
        };
        assert_eq!(
            at("\u{628}\u{628}"),
            (Some(Initial), Some(Final)),
            "{bdsm:?}"
        );
        assert_eq!(
            at("\x1b[5 ]\u{628}\u{628}"),
            (Some(Isolated), Some(Final)),
            "{bdsm:?}: 5"
        );
        assert_eq!(
            at("\x1b[6 ]\u{628}\x1b[8 ]\u{628}"),
            (Some(Initial), Some(Final)),
            "{bdsm:?}: 6 and 8"
        );
        assert_eq!(
            at("\x1b[7 ]\u{628}a"),
            (Some(JoiningForm::Medial), None),
            "{bdsm:?}: 7"
        );
    }
}
