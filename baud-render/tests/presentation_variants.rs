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
