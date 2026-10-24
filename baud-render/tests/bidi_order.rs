//! **Each row laid out in the order UAX #9 gives its paragraph**, while
//! BDSM is set (`[[source: ecma48]]` 7.2.1, IMPLICIT), and in the order its
//! cells hold while it is reset: [`Reorder`], which every draw path reads.
//!
//! Every expectation is the visual order `[[source: uax9-bidi]]` gives the
//! line, worked by hand from its rules and named where a rule decides it;
//! `every_bidi_character_test_case_resolves_and_reorders_as_it_says` holds
//! the algorithm itself to Unicode's own cases. What is tested here is the
//! cell layer: what a cell gives the algorithm, how whole cells move, and
//! what a row with no right-to-left character costs (nothing reorders).

use baud_core::{Owned, Term};
use baud_domain::{BytePos, Col, PackedCell, ReadLen, Row, Rows, WinSize};
use baud_render::{CursorSide, Reorder};

const WS: WinSize = WinSize {
    ws_row: 4,
    ws_col: 12,
};

fn feed(t: &mut Term<'_>, bytes: &str) {
    t.write_at(BytePos::ZERO, bytes.as_bytes());
    t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
}

/// Row `r` as drawn after `bytes`, BDSM set first: each cell's character,
/// `.` for an erased one and `~` for a wide tail.
fn drawn_with(ws: &WinSize, bytes: &str, r: usize, back: Rows) -> (String, Reorder) {
    let mut owned = Owned::new(ws);
    let mut t = Term::new(owned.storage(), ws);
    feed(&mut t, "\x1b[8h");
    feed(&mut t, bytes);
    let mut o = Reorder::new();
    o.lay_out(&t, back);
    let vp = t.view(back);
    let row = Row::from_index(r);
    let mut out = vec![PackedCell::EMPTY; vp.cols().index()];
    o.orders().permute(row, vp.row(row), &mut out);
    let s = out
        .iter()
        .map(|c| match c.content {
            x if x.is_empty() => '.',
            x if x.is_wide_tail() => '~',
            x => match x.as_codepoint() {
                Some(cp) => char::from_u32(cp.value()).unwrap_or('?'),
                None => match t.base_of(*c).as_codepoint() {
                    Some(cp) => char::from_u32(cp.value()).unwrap_or('?'),
                    None => '?',
                },
            },
        })
        .collect();
    (s, o)
}

fn drawn(bytes: &str, r: usize) -> String {
    drawn_with(&WS, bytes, r, Rows::ZERO).0
}

/// A Hebrew word after Latin text: its run reversed (L2), the line left to
/// right at level 0.
#[test]
fn a_hebrew_word_reads_right_to_left_in_a_left_to_right_line() {
    assert_eq!(
        drawn("ab \u{5D0}\u{5D1}\u{5D2}", 0),
        "ab \u{5D2}\u{5D1}\u{5D0}......"
    );
}

/// **A terminal as it starts draws a Hebrew word right to left**: BDSM is
/// set at power-on, the owner's answer of 2026-10-24.
#[test]
fn a_fresh_terminal_draws_a_hebrew_word_right_to_left() {
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, "ab \u{5D0}\u{5D1}\u{5D2}");
    let mut o = Reorder::new();
    o.lay_out(&t, Rows::ZERO);
    assert!(o.orders().reordered(Row::ZERO), "the row is reordered");
    assert_eq!(
        o.orders().visual_col(Row::ZERO, Col::at_index(3)),
        Col::at_index(5),
        "the alef, written at 3, drawn at 5"
    );
}

/// **The control: BDSM reset draws every cell where it is**, as baud
/// always did, and so does a row with no right-to-left character.
#[test]
fn explicit_and_left_to_right_rows_keep_their_order() {
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, "\x1b[8lab \u{5D0}\u{5D1}\u{5D2}");
    let mut o = Reorder::new();
    o.lay_out(&t, Rows::ZERO);
    assert!(!o.orders().any(), "BDSM reset reorders nothing");
    assert_eq!(drawn("plain (text) 1", 0), "plain (text)");
    let (_, o) = drawn_with(&WS, "plain text", 0, Rows::ZERO);
    assert!(!o.orders().any(), "a left-to-right row is not reordered");
}

/// L4: a bracket resolved right to left draws its mirroring glyph. N0
/// gives the pair R (a strong R inside, R before the opener), so the run
/// `א(ב)ג` reverses whole and each bracket shows its mirror.
#[test]
fn a_bracket_resolved_right_to_left_draws_its_mirror() {
    assert_eq!(
        drawn("\u{5D0}(\u{5D1})\u{5D2}", 0),
        "\u{5D2}(\u{5D1})\u{5D0}......."
    );
}

/// **L4 holds on a bracket carrying a bidi control**: an RLM after the
/// closer, or after the opener, is stored on the bracket's cell (UAX #9
/// gives it to the algorithm there), and the bracket still draws its
/// mirror, the control drawing nothing of its own -- so the pair faces
/// the same way as without it.
#[test]
fn a_bracket_carrying_a_bidi_control_draws_its_mirror() {
    // The LRM is strong L at level 0 (W1-W7 leave it L), so it ends the
    // run: the gimel after it is a run of its own, drawn to its right.
    for (bytes, want, what) in [
        (
            "\u{5D0}(\u{5D1})\u{200F}\u{5D2}",
            "\u{5D2}(\u{5D1})\u{5D0}.......",
            "an RLM after the closer",
        ),
        (
            "\u{5D0}(\u{200F}\u{5D1})\u{5D2}",
            "\u{5D2}(\u{5D1})\u{5D0}.......",
            "an RLM after the opener",
        ),
        (
            "\u{5D0}(\u{5D1})\u{61C}\u{5D2}",
            "\u{5D2}(\u{5D1})\u{5D0}.......",
            "an ALM after the closer",
        ),
        (
            "\u{5D0}(\u{5D1})\u{200E}\u{5D2}",
            "(\u{5D1})\u{5D0}\u{5D2}.......",
            "an LRM after the closer",
        ),
    ] {
        let (s, o) = drawn_with(&WS, bytes, 0, Rows::ZERO);
        assert_eq!(s, want, "{what}");
        assert_eq!(o.unmirrored().value(), 0, "{what}: nothing unmirrored");
    }
    // The mirror is drawn as a cell of its own: no draw path reads the
    // cluster it no longer shows.
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, "\x1b[8h\u{5D0}(\u{5D1})\u{200F}\u{5D2}");
    let mut o = Reorder::new();
    o.lay_out(&t, Rows::ZERO);
    let vp = t.view(Rows::ZERO);
    assert!(vp.row(Row::ZERO)[3].flags.is_marked(), "the RLM is stored");
    let mut out = vec![PackedCell::EMPTY; 12];
    o.orders().permute(Row::ZERO, vp.row(Row::ZERO), &mut out);
    assert!(!out[1].flags.is_marked(), "the mirror is unmarked");
}

/// **The departure that is left, counted**: a mirrored character carrying
/// a mark that draws -- `<` and U+0338, the decomposed form of U+226E,
/// whose precomposed form mirrors to U+226F -- is drawn as written, its
/// cell whole, and counted in [`Reorder::unmirrored`].
#[test]
fn a_mirrored_character_carrying_a_drawn_mark_is_counted() {
    let (s, o) = drawn_with(&WS, "\u{5D0}<\u{338}\u{5D1}", 0, Rows::ZERO);
    assert_eq!(s, "\u{5D1}<\u{5D0}.........");
    assert_eq!(o.unmirrored().value(), 1);
    let (s, o) = drawn_with(&WS, "\u{5D0}\u{226E}\u{5D1}", 0, Rows::ZERO);
    assert_eq!(s, "\u{5D1}\u{226F}\u{5D0}.........", "the control");
    assert_eq!(o.unmirrored().value(), 0);
}

/// A wide character in a right-to-left run moves as one, its lead on the
/// left: an emoji (ON) between two Hebrew letters resolves R by N1.
#[test]
fn a_wide_character_keeps_its_lead_on_the_left() {
    assert_eq!(
        drawn("\u{5D0}\u{1F600}\u{5D1}", 0),
        "\u{5D1}\u{1F600}~\u{5D0}........"
    );
}

/// A cell moves with its marks, which stay in the cell (L3): pointed
/// letters keep their points.
#[test]
fn marks_move_with_their_cell() {
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, "\x1b[8h\u{5E9}\u{5B8}\u{5DC}\u{5D5}\u{5B9}");
    let mut o = Reorder::new();
    o.lay_out(&t, Rows::ZERO);
    let vp = t.view(Rows::ZERO);
    let mut out = vec![PackedCell::EMPTY; 12];
    o.orders().permute(Row::ZERO, vp.row(Row::ZERO), &mut out);
    let logical = vp.row(Row::ZERO);
    assert_eq!(out[0], logical[2], "the vav and its holam drawn first");
    assert_eq!(out[1], logical[1]);
    assert_eq!(out[2], logical[0], "the shin and its qamats drawn last");
    assert!(out[0].flags.is_marked() && out[2].flags.is_marked());
}

/// An erased cell between characters is a boundary neutral (BN): X9
/// removes it and it is drawn with the character before it, so the two
/// Hebrew letters either side of the gap are one run.
#[test]
fn an_erased_cell_between_characters_moves_with_the_one_before_it() {
    // A, then CUF over one cell, then B: the gap was never written.
    assert_eq!(drawn("\u{5D0}\x1b[C\u{5D1}", 0), "\u{5D1}.\u{5D0}.........");
}

/// A paragraph is the rows joined by soft wraps: an RLO on the first row
/// overrides the Latin text of the second, which holds no right-to-left
/// character of its own. The RLO follows a letter, which stores it: at
/// column 0 there is no cell to store it on and it is dropped.
#[test]
fn an_override_on_one_row_reaches_the_next_row_of_its_paragraph() {
    let line = format!("a\u{202E}{}", "bcdefghijkl");
    assert_eq!(drawn(&format!("{line}xyz"), 1), "zyx.........");
}

/// The paragraph's first rows can be above the window: the override is in
/// the history and still reaches the row drawn.
#[test]
fn the_paragraph_reaches_above_the_window() {
    let line = format!("a\u{202E}{}", "bcdefghijkl");
    let bytes = format!("{line}xyz\r\n\r\n\r\n\r\n");
    let ws = WinSize {
        ws_row: 4,
        ws_col: 12,
    };
    // Four newlines scroll the wrapped row off; looking back one line, the
    // window's first row is the second row of the paragraph.
    let mut owned = Owned::new(&ws);
    let mut t = Term::new(owned.storage(), &ws);
    feed(&mut t, "\x1b[8h");
    feed(&mut t, &bytes);
    let back = Rows::of_index(1);
    assert_eq!(t.looking_back(back), back, "one line of history");
    let mut o = Reorder::new();
    o.lay_out(&t, back);
    let vp = t.view(back);
    let mut out = vec![PackedCell::EMPTY; 12];
    o.orders().permute(Row::ZERO, vp.row(Row::ZERO), &mut out);
    assert_eq!(
        out[0].content.as_codepoint().map(|c| c.value()),
        Some(u32::from('z'))
    );
}

/// The maps a cursor and a mouse go through are each other's inverse, and
/// a bar on a right-to-left character sits on its right side.
#[test]
fn the_maps_are_inverse_and_the_side_follows_the_level() {
    let (_, o) = drawn_with(&WS, "ab \u{5D0}\u{5D1}\u{5D2}", 0, Rows::ZERO);
    let r = Row::ZERO;
    for c in 0..12 {
        let v = o.orders().visual_col(r, Col::at_index(c));
        assert_eq!(o.orders().logical_col(r, v), Col::at_index(c), "column {c}");
    }
    assert_eq!(
        o.orders().visual_col(r, Col::at_index(3)),
        Col::at_index(5),
        "alef drawn third of the run"
    );
    assert_eq!(o.orders().side(r, Col::at_index(3)), CursorSide::Right);
    assert_eq!(o.orders().side(r, Col::at_index(0)), CursorSide::Left);
    assert_eq!(
        o.orders().side(r, Col::at_index(9)),
        CursorSide::Left,
        "an erased cell: the paragraph's side"
    );
}

/// Shaping stops at every level change and around every right-to-left
/// cell (UAX #9 3.5), and runs on inside a left-to-right run.
#[test]
fn seams_bound_every_level_run() {
    let (_, o) = drawn_with(&WS, "ab \u{5D0}\u{5D1} cd", 0, Rows::ZERO);
    let r = Row::ZERO;
    let seams: Vec<usize> = (0..12)
        .filter(|&v| o.orders().seam(r, Col::at_index(v)))
        .collect();
    // `ab ` level 0, then the two letters at level 1 (each alone), then
    // ` cd` at level 0 again.
    assert_eq!(seams, vec![3, 4, 5]);
}

/// **A row's answer is kept only while the row is unchanged**: laid out
/// again after a Hebrew word replaces Latin text on the same row, and
/// after it goes again, the row follows each time.
#[test]
fn a_row_is_tested_again_when_it_changes() {
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, "\x1b[8hab cd");
    let mut o = Reorder::new();
    o.lay_out(&t, Rows::ZERO);
    assert!(!o.orders().reordered(Row::ZERO), "Latin");
    o.lay_out(&t, Rows::ZERO);
    assert!(!o.orders().reordered(Row::ZERO), "unchanged");
    feed(&mut t, "\x1b[1;4H\u{5D0}\u{5D1}");
    o.lay_out(&t, Rows::ZERO);
    assert!(o.orders().reordered(Row::ZERO), "a Hebrew word arrived");
    assert_eq!(
        o.orders().visual_col(Row::ZERO, Col::at_index(3)),
        Col::at_index(4)
    );
    feed(&mut t, "\x1b[1;4Hcd");
    o.lay_out(&t, Rows::ZERO);
    assert!(!o.orders().reordered(Row::ZERO), "and left");
}

/// A logical range is drawn in as many visual runs as the order splits it
/// into: what a find mark and a link underline are drawn as.
#[test]
fn a_logical_range_is_drawn_in_its_visual_runs() {
    let (_, o) = drawn_with(&WS, "ab \u{5D0}\u{5D1}\u{5D2}", 0, Rows::ZERO);
    let r = Row::ZERO;
    let runs = |from: usize, to: usize| -> Vec<(usize, usize)> {
        o.orders()
            .runs(r, Col::at_index(from), Col::at_index(to))
            .into_iter()
            .map(|(c, n)| (c.index(), n.index()))
            .collect()
    };
    // `b`, the space and the alef: columns 1 and 2, then the alef drawn
    // last of its word at column 5.
    assert_eq!(runs(1, 3), vec![(1, 2), (5, 1)]);
    // The whole word: one run, reversed in place.
    assert_eq!(runs(3, 5), vec![(3, 3)]);
    // A row in its own order: the range as it is.
    assert_eq!(
        o.orders()
            .runs(Row::from_index(1), Col::at_index(2), Col::at_index(4))
            .into_iter()
            .map(|(c, n)| (c.index(), n.index()))
            .collect::<Vec<_>>(),
        vec![(2, 3)]
    );
}
