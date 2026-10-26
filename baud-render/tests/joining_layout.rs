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
/// and final, `2`, `3` and `M` for alaph's fin2, fin3 and med2, `L` and `A`
/// for the lam and the alef of a pair on one row, `W` and `w` for those of
/// a pair a soft wrap parts, `.` for none; with `implicit`, BDSM is set
/// first.
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
                (None, JoiningForm::Final2) => '2',
                (None, JoiningForm::Final3) => '3',
                (None, JoiningForm::Medial2) => 'M',
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

/// **With BDSM reset a Mongolian or Phags-pa word is joined in its stored
/// order**: the two scripts are written left to right when laid out in
/// horizontal lines (`[[source: unicode-ch13]]` 13.5, "the text should be
/// laid out in horizontal lines running left to right";
/// `[[source: unicode-ch14]]` 14.4), so the row as displayed reads left to
/// right there: the first letter, on the left, initial, and the last
/// final. Read right to left, as an Arabic row is, the forms come out
/// mirrored.
#[test]
fn with_bdsm_reset_a_mongolian_word_is_joined_left_to_right() {
    assert_eq!(forms("\u{1820}\u{1821}\u{1822}", false, 0), "imf.........");
    assert_eq!(forms("\u{A840}\u{A841}\u{A842}", false, 0), "imf.........");
    // A word of each, a space between: each joined its own way.
    assert_eq!(
        forms(&format!("\u{1820}\u{1821} {TEH}{YEH}{BEH}"), false, 0),
        "if.fmi......"
    );
}

/// **A Mongolian letter and an Arabic one side by side do not join**: each
/// script is read in its own direction, a run of its own, as a level run
/// ends the joins with BDSM set (UAX #9 3.5).
#[test]
fn with_bdsm_reset_two_directions_of_joining_do_not_join() {
    assert_eq!(
        forms(&format!("\u{1820}\u{1821}{BEH}{BEH}"), false, 0),
        "iffi........"
    );
}

const ALAPH: &str = "\u{0710}";
const BETH: &str = "\u{0712}";
const DALATH: &str = "\u{0715}";
const HE: &str = "\u{0717}";

/// **Syriac alaph takes its three forms of its own** (`[[source:
/// unicode-ch9]]` 9.3, S1-S3), each paragraph running right to left
/// against the right edge: after a beth that joins it and at the end of
/// its word, final (Afj); after he, which does not join it, fin2 (Afn);
/// after dalath, fin3 (Afx); inside a word, joined, med2, the beth after
/// it standing alone; and alone after a space, Xn, as at the start of a
/// line (Jev ISOLATED_AFTER_SPACE).
#[test]
fn syriac_alaph_takes_its_own_forms() {
    assert_eq!(forms(&format!("{BETH}{ALAPH}"), true, 0), "..........fi");
    assert_eq!(forms(&format!("{HE}{ALAPH}"), true, 0), "..........2.");
    assert_eq!(forms(&format!("{DALATH}{ALAPH}"), true, 0), "..........3.");
    assert_eq!(
        forms(&format!("{BETH}{ALAPH}{BETH}"), true, 0),
        "..........Mi"
    );
    assert_eq!(forms(&format!("{BETH} {ALAPH}"), true, 0), "............");
}

/// The word around each joined cell of row `r` after `bytes`, BDSM set,
/// with a reach of `reach`: each joined cell's visual column, and the base
/// letters of the cells before it and after it in logical order.
fn words(bytes: &str, r: usize, reach: u32) -> Vec<(usize, Vec<u32>, Vec<u32>)> {
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, "\x1b[8h");
    feed(&mut t, bytes);
    let mut o = Reorder::new();
    o.set_reach(baud_font::bytes::Count::of(reach));
    o.lay_out(&t, Rows::ZERO);
    let row = Row::from_index(r);
    let letter =
        |w: &baud_render::WordCell| t.base_of(w.cell).drawn().as_codepoint().map(|c| c.value());
    (0..usize::from(WS.ws_col))
        .filter_map(|v| {
            let j = o.orders().joined(row, Col::at_index(v))?;
            let (before, after) = o.orders().around(row, &j);
            Some((
                v,
                before.iter().filter_map(letter).collect(),
                after.iter().filter_map(letter).collect(),
            ))
        })
        .collect()
}

/// **A joined cell keeps the cells of its word around it**, as many either
/// side as the reach, in logical order, for the face's contextual lookups
/// to read: "bayt" with a reach of one gives the yeh its beh and its teh,
/// and the beh and the teh one neighbour each; a reach of none keeps none.
#[test]
fn a_joined_cell_keeps_its_word_around_it() {
    let (b, y, t) = (0x0628, 0x064A, 0x062A);
    assert_eq!(
        words(&format!("{BEH}{YEH}{TEH}"), 0, 1),
        [
            (9, vec![y], vec![]),
            (10, vec![b], vec![t]),
            (11, vec![], vec![y])
        ]
    );
    assert_eq!(
        words(&format!("{BEH}{YEH}{TEH}"), 0, 0),
        [
            (9, vec![], vec![]),
            (10, vec![], vec![]),
            (11, vec![], vec![])
        ]
    );
    // A reach of four takes the whole word, and no more: the space ends
    // the run of letters but not the level run, so it is kept, and the
    // word after it, which nothing joins, too.
    let near = words(&format!("{BEH}{YEH}{TEH} {BEH}"), 0, 4);
    assert_eq!(near[2], (11, vec![], vec![y, t, 0x20, b]));
}

/// **Across an erased cell and a soft wrap**: an erased cell between two
/// letters, transparent to joining, is left out of the word; and a word a
/// soft wrap splits keeps the letters on the other row, as its forms do
/// (Jev ACROSS_WRAP).
#[test]
fn a_word_reaches_across_an_erased_cell_and_a_soft_wrap() {
    assert_eq!(
        words(&format!("{BEH}\x1b[C{BEH}"), 0, 2),
        [(9, vec![0x0628], vec![]), (11, vec![], vec![0x0628])]
    );
    let text = format!("abcdefghi{BEH}{BEH}{BEH}{BEH}{BEH}{BEH} ok");
    let first = words(&text, 0, 2);
    // Row 0's last letter in logical order, at column 9, reads the two
    // that wrapped to row 1.
    assert_eq!(first[0], (9, vec![0x0628, 0x0628], vec![0x0628, 0x0628]));
    let second = words(&text, 1, 2);
    assert_eq!(second[2], (2, vec![0x0628, 0x0628], vec![0x0628, 0x0628]));
}
