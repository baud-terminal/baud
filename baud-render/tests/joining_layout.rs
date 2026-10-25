//! **Which form each cell's letter is drawn in**, as the layout finds it
//! for every draw path to read ([`Orders::joined`]): rules R1-R7 and the
//! lam-alef ligature (`[[source: unicode-ch9]]` 9.2.2 and 9.2.3) over the
//! logical text with BDSM set, and over the displayed order with it reset.
//!
//! Each expectation is worked from the rules by hand: the row's cells in
//! the order they are drawn, each with the form its letter takes, `.` for
//! a cell drawn as it would be alone (no letter, or a letter with nothing
//! to join).

use baud_core::{Owned, Term};
use baud_domain::{BytePos, Col, Cols, JoiningForm, LamAlef, ReadLen, Row, Rows, WinSize};
use baud_render::{PairEnd, PairPart, Reorder};

const WS: WinSize = WinSize {
    ws_row: 4,
    ws_col: 12,
};

fn feed(t: &mut Term<'_>, bytes: &str) {
    t.write_at(BytePos::ZERO, bytes.as_bytes());
    t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
}

/// Row `r`'s drawn forms after `bytes`: `i`, `m`, `f` for initial, medial
/// and final, `L` and `A` for the lam and the alef of a pair on one row,
/// `W` and `w` for those of a pair a soft wrap parts, `.` for none; with
/// `implicit`, BDSM is set first.
fn forms(bytes: &str, implicit: bool, r: usize) -> String {
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, if implicit { "\x1b[8h" } else { "\x1b[8l" });
    feed(&mut t, bytes);
    let mut o = Reorder::new();
    o.lay_out(&t, Rows::ZERO);
    let row = Row::from_index(r);
    (0..usize::from(WS.ws_col))
        .map(|v| match o.orders().joined(row, Col::at_index(v)) {
            None => '.',
            Some(j) => match (j.pair, j.form) {
                (Some((PairPart::Lam(..), _)), _) => 'L',
                (Some((PairPart::Alef(..), _)), _) => 'A',
                (Some((PairPart::LamWrapped(_), _)), _) => 'W',
                (Some((PairPart::AlefWrapped(_), _)), _) => 'w',
                (None, JoiningForm::Initial) => 'i',
                (None, JoiningForm::Medial) => 'm',
                (None, JoiningForm::Final) => 'f',
                (None, JoiningForm::Isolated) => 'n',
            },
        })
        .collect()
}

const BEH: &str = "\u{0628}";
const YEH: &str = "\u{064A}";
const TEH: &str = "\u{062A}";
const LAM: &str = "\u{0644}";
const ALEF: &str = "\u{0627}";

/// "bayt", beh-yeh-teh, a paragraph whose first strong letter is Arabic:
/// at level 1 against the right edge, columns 9-11, its first letter, the
/// beh, on the right and initial, the yeh medial and the teh, drawn first,
/// final.
#[test]
fn a_word_is_joined_in_its_logical_order_and_drawn_in_its_visual() {
    assert_eq!(forms(&format!("{BEH}{YEH}{TEH}"), true, 0), ".........fmi");
}

/// With nothing to join every letter is drawn as it is alone: a beh beside
/// a space and a letter of a word that holds one.
#[test]
fn a_letter_alone_has_no_entry() {
    assert_eq!(forms(&format!("{BEH} {BEH}"), true, 0), "............");
    assert_eq!(forms("hello", true, 0), "............");
    assert_eq!(
        forms("\u{05E9}\u{05DC}\u{05D5}\u{05DD}", true, 0),
        "............"
    );
}

/// "la": a lam joined to its alef is the pair, drawn alef on the left; and
/// after a beh the lam is medial, the pair still a pair. Each paragraph
/// runs right to left against the right edge.
#[test]
fn a_lam_joined_to_an_alef_is_a_pair() {
    assert_eq!(forms(&format!("{LAM}{ALEF}"), true, 0), "..........AL");
    assert_eq!(forms(&format!("{BEH}{LAM}{ALEF}"), true, 0), ".........ALi");
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, "\x1b[8h");
    feed(&mut t, &format!("{BEH}{LAM}{ALEF}"));
    let mut o = Reorder::new();
    o.lay_out(&t, Rows::ZERO);
    let lam = o.orders().joined(Row::ZERO, Col::at_index(10)).unwrap();
    assert_eq!(lam.form, JoiningForm::Medial);
    assert_eq!(
        lam.pair,
        Some((
            PairPart::Lam(Cols::of_index(2), PairEnd::Right),
            LamAlef::Joined
        ))
    );
}

/// A lam and an alef an erased cell parts are still a pair (L1: the
/// erased cell is transparent), drawn over the three cells (Jev
/// LIGATED_CENTRED): each holds the cells the pair spans, with BDSM set
/// and reset alike.
#[test]
fn a_pair_an_erased_cell_parts_is_still_a_pair() {
    assert_eq!(
        forms(&format!("{LAM}\x1b[C{ALEF}"), true, 0),
        ".........A.L"
    );
    assert_eq!(
        forms(&format!("{ALEF}\x1b[C{LAM}"), false, 0),
        "A.L........."
    );
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, &format!("\x1b[8h{LAM}\x1b[2C{ALEF}"));
    let mut o = Reorder::new();
    o.lay_out(&t, Rows::ZERO);
    let gap = Some((
        PairPart::Lam(Cols::of_index(4), PairEnd::Right),
        LamAlef::Nominal,
    ));
    assert_eq!(
        o.orders()
            .joined(Row::ZERO, Col::at_index(11))
            .unwrap()
            .pair,
        gap
    );
}

/// A lam and an alef a soft wrap parts are a pair (Jev ACROSS_WRAP), the
/// ligature drawn in the lam's cell (Jev's lean LIGATED_LAM_ROW): "abcdefghij",
/// a beh and a lam fill the twelve columns and the alef wraps. Each part
/// holds the other letter's cell.
#[test]
fn a_pair_a_soft_wrap_parts_is_drawn_in_the_lams_row() {
    let text = format!("abcdefghij{BEH}{LAM}{ALEF} ok");
    assert_eq!(forms(&text, true, 0), "..........Wi");
    assert_eq!(forms(&text, true, 1), "w...........");
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, "\x1b[8h");
    feed(&mut t, &text);
    let mut o = Reorder::new();
    o.lay_out(&t, Rows::ZERO);
    let part = |r: usize, c: usize| o.orders().joined(Row::from_index(r), Col::at_index(c));
    let letter = |cell: baud_domain::PackedCell| cell.content.as_codepoint().map(|c| c.value());
    match part(0, 10).and_then(|j| j.pair) {
        Some((PairPart::LamWrapped(alef), LamAlef::Joined)) => {
            assert_eq!(letter(alef), Some(0x0627));
        }
        other => panic!("the lam's part: {other:?}"),
    }
    match part(1, 0).and_then(|j| j.pair) {
        Some((PairPart::AlefWrapped(lam), LamAlef::Joined)) => {
            assert_eq!(letter(lam), Some(0x0644));
        }
        other => panic!("the alef's part: {other:?}"),
    }
}

/// A mark between two letters does not part them (R1), and ZWJ after a
/// beh joins it forward (it is join-causing), where ZWNJ parts two behs.
#[test]
fn marks_and_joiners_act_as_the_rules_say() {
    assert_eq!(
        forms(&format!("{BEH}\u{064E}{BEH}"), true, 0),
        "..........fi"
    );
    assert_eq!(forms(&format!("{BEH}\u{200D}"), true, 0), "...........i");
    assert_eq!(
        forms(&format!("{BEH}\u{200C}{BEH}"), true, 0),
        "............"
    );
}

/// An erased cell between two letters is transparent (Jev TRANSPARENT):
/// the behs either side join across it.
#[test]
fn an_erased_cell_between_letters_is_transparent() {
    assert_eq!(forms(&format!("{BEH}\x1b[C{BEH}"), true, 0), ".........f.i");
}

/// A soft wrap inside a word does not part it (Jev ACROSS_WRAP): the row
/// is twelve columns, so "abcdefghi" and three behs fill it and the next
/// three wrap; each run is drawn at its row's end or start, and the letters
/// either side of the wrap stay medial.
#[test]
fn a_soft_wrap_inside_a_word_keeps_it_whole() {
    let text = format!("abcdefghi{BEH}{BEH}{BEH}{BEH}{BEH}{BEH} ok");
    assert_eq!(forms(&text, true, 0), ".........mmi");
    assert_eq!(forms(&text, true, 1), "fmm.........");
}

/// With BDSM reset the row is joined in the order it is displayed: a word
/// an application wrote reversed is joined as it reads on screen, the teh
/// on the left final and the beh on the right initial; and an alef left of
/// a lam is the pair.
#[test]
fn with_bdsm_reset_a_row_is_joined_as_displayed() {
    assert_eq!(forms(&format!("{TEH}{YEH}{BEH}"), false, 0), "fmi.........");
    assert_eq!(forms(&format!("{ALEF}{LAM}"), false, 0), "AL..........");
    assert_eq!(
        forms(&format!("{ALEF}{LAM}{BEH}"), false, 0),
        "ALi........."
    );
}

/// An alef whose lam is on a row scrolled above the window keeps its final
/// form: the ligature is drawn in the lam's cell, which no row of the
/// window holds, so the alef's cell drawing nothing would lose its letter.
/// The word is written on the last row, the alef wraps, and three line
/// feeds scroll the lam's row into the history.
#[test]
fn an_alef_whose_lam_scrolled_away_keeps_its_form() {
    let text = format!("\x1b[4;1Habcdefghij{BEH}{LAM}{ALEF} ok\r\n\r\n\r\n");
    assert_eq!(forms(&text, true, 0), "f...........");
    // The control: one line feed fewer, and both rows are in the window.
    let both = format!("\x1b[4;1Habcdefghij{BEH}{LAM}{ALEF} ok\r\n\r\n");
    assert_eq!(forms(&both, true, 0), "..........Wi");
    assert_eq!(forms(&both, true, 1), "w...........");
}

/// **A Mongolian word is joined in a row nothing reorders**: its letters
/// are of joining type D and bidi class L, so the row holds no character
/// that can reorder its paragraph and is still read for its joins -- the
/// one pass that lets a row go loads the joining blocks with the
/// right-to-left ones (`baud_domain::LAYOUT_BLOCKS`). With BDSM set the
/// word is joined in its stored order, drawn where it is: the first letter
/// initial, the last final.
#[test]
fn a_mongolian_word_is_joined_where_nothing_reorders() {
    assert_eq!(forms("\u{1820}\u{1821}\u{1822}", true, 0), "imf.........");
}
