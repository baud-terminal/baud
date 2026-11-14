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

mod support;

use baud_core::{Owned, Term};
use baud_domain::{BytePos, Codepoint, Col, Content, PackedCell, ReadLen, Row, Rows, WinSize};
use baud_render::{Changes, CursorSide, Mirrored, Rejoin, Reorder, Written};

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
/// set at power-on.
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

/// **A paragraph takes its direction from its first strong letter** (UAX
/// #9 P2-P3), and a right-to-left one sits against the right edge: a
/// reported sentence, all Hebrew, runs right to left at level 1, so its
/// closing period, a neutral between an R and the paragraph's end (N1-N2),
/// is drawn at the left end of the text, and the erased cells past it are
/// laid out before it, at the line's end in the paragraph direction.
#[test]
fn a_hebrew_paragraph_runs_right_to_left_against_the_right_edge() {
    let ws = WinSize {
        ws_row: 2,
        ws_col: 30,
    };
    let sentence = "\u{5E9}\u{5DC}\u{5D5}\u{5DD} \u{5E2}\u{5D5}\u{5DC}\u{5DD}! \u{5D6}\u{5D5} \u{5E9}\u{5D5}\u{5E8}\u{5D4} \u{5D1}\u{5E2}\u{5D1}\u{5E8}\u{5D9}\u{5EA}.";
    let reversed: String = sentence.chars().rev().collect();
    let (s, o) = drawn_with(&ws, sentence, 0, Rows::ZERO);
    assert_eq!(s, format!("....{reversed}"));
    assert!(
        s.ends_with('\u{5E9}'),
        "the shin, written first, drawn last"
    );
    assert_eq!(
        o.orders().visual_col(Row::ZERO, Col::at_index(25)),
        Col::at_index(4),
        "the period, written last, drawn at the text's left end"
    );
    assert_eq!(
        o.orders().visual_col(Row::ZERO, Col::at_index(26)),
        Col::at_index(3),
        "the cursor's cell past the text: just left of it"
    );
    assert_eq!(
        o.orders().side(Row::ZERO, Col::at_index(26)),
        CursorSide::Right
    );
    // A click anywhere names the cell drawn under it, the blank left of
    // the text included: the maps are each other's inverse.
    for c in 0..30 {
        let v = o.orders().visual_col(Row::ZERO, Col::at_index(c));
        assert_eq!(
            o.orders().logical_col(Row::ZERO, v),
            Col::at_index(c),
            "column {c}"
        );
    }
    assert_eq!(
        o.orders().logical_col(Row::ZERO, Col::ZERO),
        Col::at_index(29),
        "a click at the left edge: the row's last erased cell"
    );
    // The control: a line whose first strong letter is Latin stays at
    // level 0, at the left edge.
    let (s, _) = drawn_with(
        &ws,
        "hello world, 123 \u{5E9}\u{5DC}\u{5D5}\u{5DD}",
        0,
        Rows::ZERO,
    );
    assert_eq!(s, "hello world, 123 \u{5DD}\u{5D5}\u{5DC}\u{5E9}.........");
}

/// **A paragraph resolved in more than one piece keeps one direction**: a
/// window full of one paragraph, more codepoints than [`CONTEXT`], is
/// resolved in pieces, and each piece after the first is given the first
/// piece's level (HL1) rather than looking for a first strong letter of
/// its own. The paragraph starts with a Hebrew letter and goes on in
/// Latin, so its last row, in the last piece and all Latin, still sits
/// against the right edge.
#[test]
fn a_paragraph_resolved_in_pieces_keeps_its_first_pieces_direction() {
    let cols = 200;
    let rows = baud_render::CONTEXT / cols + 8;
    let ws = WinSize {
        ws_row: u16::try_from(rows).unwrap(),
        ws_col: u16::try_from(cols).unwrap(),
    };
    let tail = 50;
    let text = format!("\u{5D0}{}", "a".repeat(cols * (rows - 1) - 1 + tail));
    let (last, o) = drawn_with(&ws, &text, rows - 1, Rows::ZERO);
    assert!(o.orders().reordered(Row::from_index(rows - 1)));
    assert_eq!(
        last,
        format!("{}{}", ".".repeat(cols - tail), "a".repeat(tail))
    );
    let (first, _) = drawn_with(&ws, &text, 0, Rows::ZERO);
    assert!(first.ends_with('\u{5D0}'), "the alef at the right of row 0");
}

/// **A table row keeps its columns, and each cell its own direction**: a
/// row drawn with box-drawing column rules runs left to right with every
/// rule where it was written, and each cell between two rules is a
/// paragraph of its own (UAX #9 section 3, "the text in two different
/// cells of a table will be in different paragraphs"; HL4) at the level
/// its own first strong letter gives it, ordered inside its own columns:
/// a Hebrew cell reads right to left against its cell's right side (L1),
/// a cell of digits stays as it is. A reported row, 2026-10-24.
#[test]
fn a_table_row_keeps_its_columns_and_each_cell_its_own_direction() {
    let ws = WinSize {
        ws_row: 3,
        ws_col: 40,
    };
    let row = "\u{2502} \u{5DE}\u{5E1}\u{5DE}\u{5DA}.txt      \u{2502} 812  \u{2502} \u{5D3}\u{5E0}\u{5D4}   \u{2502}";
    let (s, o) = drawn_with(&ws, row, 0, Rows::ZERO);
    assert_eq!(
        s,
        "\u{2502}      txt.\u{5DA}\u{5DE}\u{5E1}\u{5DE} \u{2502} 812  \u{2502}   \u{5D4}\u{5E0}\u{5D3} \u{2502}........"
    );
    for rule in [0, 16, 23, 31] {
        assert_eq!(
            o.orders().visual_col(Row::ZERO, Col::at_index(rule)),
            Col::at_index(rule),
            "the rule at {rule} where it was written"
        );
        assert_eq!(
            o.orders().side(Row::ZERO, Col::at_index(rule)),
            CursorSide::Left
        );
    }
    // A row of the same table with no right-to-left letter is not
    // reordered at all.
    let (s, o) = drawn_with(
        &ws,
        "\u{2502} notes.txt \u{2502} 42 \u{2502}",
        0,
        Rows::ZERO,
    );
    assert_eq!(
        s,
        "\u{2502} notes.txt \u{2502} 42 \u{2502}......................"
    );
    assert!(!o.orders().reordered(Row::ZERO));
}

/// **The cell's padding may be erased cells**, as a program that moves the
/// cursor over a column leaves them: boundary neutrals the algorithm
/// removes (X9), so they move with their cell's text, and the trailing
/// ones take the cell's level (L1) and go to its left.
#[test]
fn a_table_cell_padded_with_erased_cells_keeps_its_columns() {
    assert_eq!(
        drawn("\u{2502}\x1b[C\u{5D3}\u{5E0}\u{5D4}\x1b[3C\u{2502}", 0),
        "\u{2502}...\u{5D4}\u{5E0}\u{5D3}.\u{2502}...",
    );
}

/// **DEC Special Graphics line drawing is a column rule**: `ESC ( 0` and
/// `x` store U+2502, so a border ncurses draws that way divides a row into
/// cells as the Unicode one does.
#[test]
fn a_dec_graphics_vertical_is_a_column_rule() {
    assert_eq!(
        drawn("\x1b(0x\x1b(B \u{5D3}\u{5E0}\u{5D4}  \x1b(0x\x1b(B", 0),
        "\u{2502}  \u{5D4}\u{5E0}\u{5D3} \u{2502}....",
    );
}

/// **A tree's branch divides its row too**: the name after `└──` is a cell
/// of its own, read right to left in place, and the branch stays at the
/// left edge where C would turn the whole row round.
#[test]
fn a_tree_branch_keeps_the_name_after_it_in_place() {
    let ws = WinSize {
        ws_row: 2,
        ws_col: 24,
    };
    let (s, _) = drawn_with(
        &ws,
        "\u{2514}\u{2500}\u{2500} \u{5DE}\u{5E1}\u{5DE}\u{5DA} \u{5D7}\u{5E9}\u{5D5}\u{5D1}.txt",
        0,
        Rows::ZERO,
    );
    assert_eq!(
        s,
        "\u{2514}\u{2500}\u{2500}txt.\u{5D1}\u{5D5}\u{5E9}\u{5D7} \u{5DA}\u{5DE}\u{5E1}\u{5DE} ......."
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

/// L4: a bracket resolved right to left draws its mirroring glyph. The
/// paragraph is at level 1 (its first strong letter is the alef, P2-P3)
/// and N0 gives the pair R (a strong R inside matching the embedding
/// direction), so the line reverses whole, against the right edge, and
/// each bracket shows its mirror. The same run after a Latin word, at
/// level 0, reverses in place.
#[test]
fn a_bracket_resolved_right_to_left_draws_its_mirror() {
    assert_eq!(
        drawn("\u{5D0}(\u{5D1})\u{5D2}", 0),
        ".......\u{5D2}(\u{5D1})\u{5D0}"
    );
    assert_eq!(
        drawn("x \u{5D0}(\u{5D1})\u{5D2}", 0),
        "x \u{5D2}(\u{5D1})\u{5D0}....."
    );
}

/// **L4 holds on a bracket carrying a bidi control**: an RLM after the
/// closer, or after the opener, is stored on the bracket's cell (UAX #9
/// gives it to the algorithm there), and the bracket still draws its
/// mirror, the control drawing nothing of its own -- so the pair faces
/// the same way as without it.
#[test]
fn a_bracket_carrying_a_bidi_control_draws_its_mirror() {
    // Each row after a Latin letter, so the paragraph is at level 0 and
    // the run reverses in place. The LRM is strong L at level 0 (W1-W7
    // leave it L), so it ends the run: the gimel after it is a run of its
    // own, drawn to its right.
    for (bytes, want, what) in [
        (
            "x\u{5D0}(\u{5D1})\u{200F}\u{5D2}",
            "x\u{5D2}(\u{5D1})\u{5D0}......",
            "an RLM after the closer",
        ),
        (
            "x\u{5D0}(\u{200F}\u{5D1})\u{5D2}",
            "x\u{5D2}(\u{5D1})\u{5D0}......",
            "an RLM after the opener",
        ),
        (
            "x\u{5D0}(\u{5D1})\u{61C}\u{5D2}",
            "x\u{5D2}(\u{5D1})\u{5D0}......",
            "an ALM after the closer",
        ),
        (
            "x\u{5D0}(\u{5D1})\u{200E}\u{5D2}",
            "x(\u{5D1})\u{5D0}\u{5D2}......",
            "an LRM after the closer",
        ),
        // At level 1 the LRM is an L of its own, at level 2, and the line
        // reverses whole: the closer it is stored on still mirrors.
        (
            "\u{5D0}(\u{5D1})\u{200E}\u{5D2}",
            ".......\u{5D2}(\u{5D1})\u{5D0}",
            "an LRM after the closer, at level 1",
        ),
    ] {
        let (s, o) = drawn_with(&WS, bytes, 0, Rows::ZERO);
        assert_eq!(s, want, "{what}");
        assert!(
            o.orders().mirrors(Row::ZERO).is_empty(),
            "{what}: drawn as its mirroring glyph"
        );
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
    assert!(!out[8].flags.is_marked(), "the mirror is unmarked");
}

/// **L4 for the cells a mirroring glyph alone cannot draw**: a mirrored
/// character carrying a mark that draws -- `<` and U+0338, the decomposed
/// form of U+226E, whose precomposed form mirrors to U+226F -- keeps its
/// cell, marks and all, and is drawn with `>` for its base
/// ([`Mirrored::Under`]); U+2211, which has no Bidi_Mirroring_Glyph, is
/// drawn flipped ([`Mirrored::Flipped`]), and so is U+2211 carrying an RLM,
/// its cell given the base alone as a bracket's is. A mirrored character
/// at an even level is drawn as it is.
#[test]
fn a_mirrored_character_a_glyph_alone_cannot_draw_is_drawn_mirrored() {
    let (s, o) = drawn_with(&WS, "\u{5D0}\u{2211}<\u{338}\u{5D1}", 0, Rows::ZERO);
    assert_eq!(s, "........\u{5D1}<\u{2211}\u{5D0}");
    let at = |v: usize| o.orders().mirrored(Row::ZERO, Col::at_index(v));
    assert_eq!(at(9), Some(Mirrored::Under(Codepoint::from_char('>'))));
    assert_eq!(at(10), Some(Mirrored::Flipped));
    assert_eq!((at(8), at(11)), (None, None), "the letters are not");
    let (s, o) = drawn_with(&WS, "\u{5D0}\u{226E}\u{5D1}", 0, Rows::ZERO);
    assert_eq!(s, ".........\u{5D1}\u{226F}\u{5D0}", "the control");
    assert!(o.orders().mirrors(Row::ZERO).is_empty());
    let (s, o) = drawn_with(&WS, "\u{5D0}\u{2211}\u{200F}\u{5D1}", 0, Rows::ZERO);
    assert_eq!(s, ".........\u{5D1}\u{2211}\u{5D0}", "the RLM left out");
    assert_eq!(
        o.orders().mirrors(Row::ZERO),
        &[(Col::at_index(10), Mirrored::Flipped)]
    );
    let (_, o) = drawn_with(&WS, "x\u{2211}<\u{338}", 0, Rows::ZERO);
    assert!(!o.orders().any(), "at level 0 nothing is mirrored");
}

/// A wide character in a right-to-left run moves as one, its lead on the
/// left: an emoji (ON) between two Hebrew letters resolves R by N1.
#[test]
fn a_wide_character_keeps_its_lead_on_the_left() {
    assert_eq!(
        drawn("\u{5D0}\u{1F600}\u{5D1}", 0),
        "........\u{5D1}\u{1F600}~\u{5D0}"
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
    // A right-to-left paragraph, against the right edge.
    assert_eq!(out[9], logical[2], "the vav and its holam drawn first");
    assert_eq!(out[10], logical[1]);
    assert_eq!(out[11], logical[0], "the shin and its qamats drawn last");
    assert!(out[9].flags.is_marked() && out[11].flags.is_marked());
}

/// An erased cell between characters is a boundary neutral (BN): X9
/// removes it and it is drawn with the character before it, so the two
/// Hebrew letters either side of the gap are one run.
#[test]
fn an_erased_cell_between_characters_moves_with_the_one_before_it() {
    // A, then CUF over one cell, then B: the gap was never written.
    assert_eq!(drawn("\u{5D0}\x1b[C\u{5D1}", 0), ".........\u{5D1}.\u{5D0}");
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

/// **The row a smooth scroll slides out is laid out as the first row of
/// the window one line up**: [`Reorder::lay_out_top`] lays out only that
/// row's paragraph, and reads it whole -- here a row of digits and
/// brackets, which holds no strong letter of its own, soft-wrapped into a
/// Hebrew row inside the window, so the paragraph is right to left (P2-P3)
/// only because of the row below it. Row 0's order is the one the whole
/// window's layout gives it.
#[test]
fn the_leaving_row_is_laid_out_as_the_first_row_one_line_up() {
    let ws = WinSize {
        ws_row: 4,
        ws_col: 12,
    };
    let mut owned = Owned::new(&ws);
    let mut t = Term::new(owned.storage(), &ws);
    // Twelve cells, wrapped into the Hebrew row, then three lines more:
    // the digits' row is the one line above the live screen.
    feed(
        &mut t,
        "\x1b[8h(12) [34] 56\u{5D0}\u{5D1}\u{5D2} x\r\none\r\ntwo\r\nsix",
    );
    let above = Rows::of_index(1);
    assert_eq!(t.looking_back(above), above, "one line of history");
    let mut whole = Reorder::new();
    whole.lay_out(&t, above);
    let mut top = Reorder::new();
    top.lay_out_top(&t, above, baud_render::Joins::Seen);
    assert!(
        top.orders().reordered(Row::ZERO),
        "the premise: the row reorders with the row below it"
    );
    let vp = t.view(above);
    let (mut a, mut b) = (vec![PackedCell::EMPTY; 12], vec![PackedCell::EMPTY; 12]);
    whole.orders().permute(Row::ZERO, vp.row(Row::ZERO), &mut a);
    top.orders().permute(Row::ZERO, vp.row(Row::ZERO), &mut b);
    assert_eq!(a, b, "drawn in the order the window one line up draws it");
    assert!(
        (0..12).all(|c| top.orders().visual_col(Row::ZERO, Col::at_index(c))
            == whole.orders().visual_col(Row::ZERO, Col::at_index(c))),
        "each cell where the window one line up draws it"
    );
    assert!(
        !top.orders().reordered(Row::from_index(2)),
        "and nothing past its paragraph laid out"
    );
}

/// **A composition on a right-to-left row draws as the same text written
/// there**, on the CPU path: its row is laid out with the composition's
/// text in its cells (`Written`), so `cd` composed after `ab` in a Hebrew
/// paragraph is drawn `abcd`, the `ab` moved left to make room, and
/// composed over the `xy` of `abxy` its records and the cells it blanks are
/// where that order puts them. The reference is `composition.rs`'s, the
/// same text written underlined at the cursor, the cursor on the cell after
/// it in logical order, which the paragraph draws left of `abcd`; the
/// instance and cell arms are held to it in `baud-present-macos`.
#[test]
fn a_composition_on_a_right_to_left_row_draws_as_the_text_written() {
    use baud_font::Face;
    use baud_render::{Caret, Composition, Damage};
    let data = std::fs::read(format!(
        "{}/../baud-app/fonts/built/Regular.ttf",
        env!("CARGO_MANIFEST_DIR")
    ))
    .expect("face");
    let faces = [Face::parse(&data).unwrap()];
    let mut r = support::renderer(&faces);
    let frame = |r: &mut baud_render::Renderer<'_, '_>, bytes: &str, composed: bool| {
        let mut owned = Owned::new(&support::WS);
        let mut t = Term::new(owned.storage(), &support::WS);
        feed(&mut t, bytes);
        let (row, col) = t.cursor();
        r.set_composition(composed.then(|| Composition {
            row,
            col,
            text: "cd".chars().map(Codepoint::from_char).collect(),
            measure: baud_render::Measure::default(),
            hints: Vec::new(),
            caret: Caret::End,
        }));
        let mut c = support::Canvas::for_renderer(r);
        support::paint(r, &mut t, &mut c, Damage::Full);
        let reordered = r.bidi().reordered(row);
        r.set_composition(None);
        (c.pixels, reordered)
    };
    // After `ab`, on erased cells, where the write moves `ab`; and over
    // `xy`, whose cells it covers where the order draws them.
    // `xy` is red, so the composition over it is drawn red, and written
    // red: the rendition is the logical cell's, wherever it is drawn.
    for (line, red) in [
        ("\x1bc\x1b[2 q\x1b[3;1H\u{5E9}\u{5DC}\u{5D5}\u{5DD} ab", ""),
        (
            "\x1bc\x1b[2 q\x1b[3;1H\u{5E9}\u{5DC}\u{5D5}\u{5DD} ab\x1b[31mxy\x1b[39m\x1b[2D",
            "\x1b[31m",
        ),
    ] {
        let (composed, reordered) = frame(&mut r, line, true);
        assert!(reordered, "the premise: the row is reordered");
        let (plain, _) = frame(&mut r, line, false);
        let (written, _) = frame(
            &mut r,
            &format!("{line}{red}\x1b[4mcd\x1b[24m\x1b[39m"),
            false,
        );
        assert_ne!(composed, plain, "the composition is drawn");
        assert!(
            composed == written,
            "{line:?}: as the same text written underlined, the cursor on the cell after it"
        );
    }
}

/// **A right-to-left line that is double-width runs against its half's
/// right edge**: UAX #9 3.4 reorders "for each line", and a line that is
/// not single-width is drawn over half the screen's columns
/// (`[[source: dec-std-070]]` § 5.14.1, `end_line /= 2`;
/// `[[source: vt510-decdwl]]`), so its erased cells are laid out before
/// its text within that half and the columns past it, never drawn, after.
/// Laid out over the screen's width, the text sat in the half not drawn.
/// With BDSM reset a line SCP sets right to left is reversed within the
/// half too, its first column in the half's rightmost cell. The control: a
/// left-to-right line keeps its order. Each of DECDWL and DECDHL's two
/// halves.
#[test]
fn a_double_width_right_to_left_line_is_laid_out_over_its_drawn_half() {
    let hebrew = "\u{5D0}(\u{5D1})\u{5D2}";
    let half = "\u{5D2}(\u{5D1})\u{5D0}";
    // DECDWL, and DECDHL's top and bottom halves.
    for size in ["\x1b#6", "\x1b#3", "\x1b#4"] {
        let (s, o) = drawn_with(&WS, &format!("{size}{hebrew}"), 0, Rows::ZERO);
        assert_eq!(s, format!(".{half}......"), "{size:?}: implicit");
        for c in 0..12 {
            let v = o.orders().visual_col(Row::ZERO, Col::at_index(c));
            assert_eq!(o.orders().logical_col(Row::ZERO, v), Col::at_index(c));
        }
        assert_eq!(
            o.orders().visual_col(Row::ZERO, Col::at_index(5)),
            Col::ZERO,
            "{size:?}: the cursor's cell past the text: the half's left end"
        );
        let (s, _) = drawn_with(
            &WS,
            &format!("\x1b[8l\x1b[2 k{size}{hebrew}"),
            0,
            Rows::ZERO,
        );
        assert_eq!(
            s,
            format!(".{half}......"),
            "{size:?}: explicit, right to left"
        );
        let (s, _) = drawn_with(&WS, &format!("{size}ab \u{5D0}\u{5D1}"), 0, Rows::ZERO);
        assert_eq!(s, "ab \u{5D1}\u{5D0}.......", "{size:?}: the control");
    }
}

/// **A kept layout reads a row as written over it** (BDSM reset, the last
/// frame's joins kept for the caller's compare): a wide character written
/// over two narrow cells of a line drawn in reverse keeps its two cells in
/// order, as a write of it would, so the cell under its lead is drawn a
/// column further left; and each frame that reads a row as written, and
/// the frame after the last, find every row's joins again, since the order
/// moved under cells no compare saw change. The control: frames with
/// nothing written keep them.
#[test]
fn a_kept_layout_reads_what_is_written_and_rejoins_around_it() {
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, "\x1b[8l\x1b[2 kabcdef");
    let vp = t.view(Rows::ZERO);
    let wide = |content| PackedCell {
        content,
        ..PackedCell::EMPTY
    };
    let mut written = Written::default();
    written.write(
        &vp,
        Row::ZERO,
        Col::at_index(1),
        &[
            wide(Content::codepoint(Codepoint::from_char('\u{4e2d}'))),
            wide(Content::WIDE_TAIL),
        ],
    );
    let none = Written::default();
    let mut o = Reorder::new();
    // Each frame as a caller draws it: every row joined again where the
    // answer is `Rejoin::Every`, then the rejoin ended, which that answer
    // waits on (`Reorder::rejoined`).
    let keep = |o: &mut Reorder, w: &Written| {
        let how = o.keep_displayed(&t, Rows::ZERO, Changes::Copied, w);
        if how == Rejoin::Every {
            for y in 0..usize::from(WS.ws_row) {
                o.rejoin(&t, &vp, Row::from_index(y), w);
            }
        }
        o.rejoined();
        how
    };
    let lead = |o: &Reorder| o.orders().visual_col(Row::ZERO, Col::at_index(1)).index();
    assert_eq!(keep(&mut o, &none), Rejoin::Every, "the first frame");
    assert_eq!(keep(&mut o, &none), Rejoin::Changed, "kept");
    assert_eq!(lead(&o), 10, "the cell as stored");
    assert_eq!(keep(&mut o, &written), Rejoin::Every, "read as written");
    assert_eq!(lead(&o), 9, "the wide character's lead, left of its tail");
    assert_eq!(
        keep(&mut o, &written),
        Rejoin::Every,
        "read as written again"
    );
    assert_eq!(keep(&mut o, &none), Rejoin::Every, "the frame after");
    assert_eq!(lead(&o), 10, "as stored again");
    assert_eq!(keep(&mut o, &none), Rejoin::Changed, "kept again");
}

/// **The context below a window that looks back reaches the live screen's
/// last row**: in a window one line up whose last row holds only digits
/// and a space, soft-wrapped onto a live last row that starts in Hebrew,
/// the paragraph takes its level from that Hebrew (UAX #9 P2-P3), so the
/// digits' row runs right to left, its space between the two numbers at
/// level 1 (N1). Reading the window's own last row again as the row below
/// found no right-to-left letter and drew it left to right.
#[test]
fn a_window_one_line_up_reads_the_live_last_row_as_context() {
    let ws = WinSize {
        ws_row: 4,
        ws_col: 10,
    };
    let bytes = "a\r\nb\r\nc\r\n12345 6789\u{5E9}\u{5DC}\u{5D5}\u{5DD}";
    let (s, _) = drawn_with(&ws, bytes, 3, Rows::of_count(1));
    assert_eq!(s, "6789 12345");
    let (live, _) = drawn_with(&ws, bytes, 2, Rows::ZERO);
    assert_eq!(live, "6789 12345", "the live window, the control");
}
