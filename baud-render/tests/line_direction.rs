//! **A program can choose which way its lines run**: SCP, `CSI Ps1 ; Ps2
//! SP k` (`[[source: ecma48]]` 8.3.111), selects the character path "for
//! the active line ... and subsequent lines", and SPD, `CSI Ps1 ; Ps2 SP
//! S` (8.3.130), for the whole screen. Under BDSM set the path is the
//! paragraph's level, set outright (`[[source: uax9-bidi]]` HL1); reset,
//! a right-to-left line is drawn as terminal-wg's explicit RTL, its cells
//! in reverse and its mirrored characters mirrored
//! (`[[source: terminal-wg-bidi-basic-modes]]`).
//!
//! Every expectation is a visual order worked by hand from UAX #9's rules;
//! `.` is an erased cell.

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

/// Row `r` of the window `back` lines into the history as `o` lays it out.
fn row_of(t: &Term<'_>, o: &mut Reorder, r: usize, back: Rows) -> String {
    o.lay_out(t, back);
    let vp = t.view(back);
    let row = Row::from_index(r);
    let mut out = vec![PackedCell::EMPTY; vp.cols().index()];
    o.orders().permute(row, vp.row(row), &mut out);
    out.iter()
        .map(|c| match c.content {
            x if x.is_empty() => '.',
            x if x.is_wide_tail() => '~',
            x => match x.as_codepoint() {
                Some(cp) => char::from_u32(cp.value()).unwrap_or('?'),
                None => '?',
            },
        })
        .collect()
}

/// Every row of the screen after `bytes`, from power-on.
fn screen(ws: &WinSize, bytes: &str) -> Vec<String> {
    let mut owned = Owned::new(ws);
    let mut t = Term::new(owned.storage(), ws);
    feed(&mut t, bytes);
    let mut o = Reorder::new();
    (0..usize::from(ws.ws_row))
        .map(|r| row_of(&t, &mut o, r, Rows::ZERO))
        .collect()
}

const HEBREW: &str = "\u{5D0}\u{5D1}\u{5D2}!";

/// **SCP 2 draws an English line right to left**: the paragraph's level is
/// 1 whatever its letters (HL1), so the Latin run sits at level 2 against
/// the right edge and the `!` after it, a neutral between an L and the
/// paragraph's end, takes the paragraph's direction (N1-N2) and is drawn
/// at the run's left.
#[test]
fn scp_2_draws_an_english_line_right_to_left() {
    assert_eq!(screen(&WS, "\x1b[2 kabc!")[0], "........!abc");
    // The control: as the terminal starts, first strong.
    assert_eq!(screen(&WS, "abc!")[0], "abc!........");
}

/// **SCP 1 keeps a Hebrew line left to right**: level 0 outright, so the
/// Hebrew run is reversed (L2) at the left edge and its `!` follows it.
#[test]
fn scp_1_keeps_a_hebrew_line_left_to_right() {
    assert_eq!(
        screen(&WS, &format!("\x1b[1 k{HEBREW}"))[0],
        "\u{5D2}\u{5D1}\u{5D0}!........"
    );
    assert_eq!(
        screen(&WS, HEBREW)[0],
        "........!\u{5D2}\u{5D1}\u{5D0}",
        "the control: first strong puts the line against the right edge"
    );
}

/// **The direction is the active line's at once and every later line's**
/// (8.3.111's "for the active line ... and subsequent lines ... This takes
/// effect immediately"): sent after the text, it turns the line already
/// written, and the lines a line feed reaches take it.
#[test]
fn the_direction_turns_the_active_line_and_carries_to_later_lines() {
    let s = screen(&WS, "abc!\x1b[2 k\r\ndef!\r\n\x1b[1 kghi!");
    assert_eq!(s[0], "........!abc", "the line written before it");
    assert_eq!(s[1], "........!def", "a line feed's new line");
    assert_eq!(s[2], "ghi!........", "SCP 1 on the third");
    assert_eq!(s[3], "............");
}

/// **A line the cursor only moves to keeps its own direction**: a move is
/// not a new line (terminal-wg's draft, "Cursor movement never applies
/// modes"), so text written there after `CUP` is first strong.
#[test]
fn a_line_the_cursor_moves_to_keeps_its_direction() {
    let s = screen(&WS, "\x1b[2 k\x1b[3;1Habc!");
    assert_eq!(s[2], "abc!........");
}

/// **Ps1 0 or omitted is refused and counted**: ECMA-48 gives SCP "No
/// parameter default values" and defines only 1 and 2 (Jev's lean,
/// REFUSE). The direction in force stays; DECSTR returns it to the
/// default, as DEC's DECSTR returns DECRLM to left to right.
#[test]
fn ps1_zero_is_refused_and_decstr_returns_to_first_strong() {
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, "\x1b[2 k");
    let before = t.unsupported;
    feed(&mut t, "\x1b[0 k\x1b[ k\x1b[;0 k");
    assert_eq!(t.unsupported, before + 3, "each refusal counted");
    feed(&mut t, "abc!\r\n");
    let mut o = Reorder::new();
    assert_eq!(row_of(&t, &mut o, 0, Rows::ZERO), "........!abc");
    feed(&mut t, "\x1b[!pdef!");
    assert_eq!(
        row_of(&t, &mut o, 1, Rows::ZERO),
        "........!def",
        "DECSTR leaves the line it finds as it is"
    );
    feed(&mut t, "\r\nghi!");
    assert_eq!(
        row_of(&t, &mut o, 2, Rows::ZERO),
        "ghi!........",
        "the next line is first strong again"
    );
}

/// **Ps2 2 is refused and counted**: it updates the data from the
/// presentation, which a display-only model never does
/// (`[[source: terminal-wg-bidi-escape-sequences]]`, "should definitely
/// not be supported").
#[test]
fn ps2_two_is_refused_and_counted() {
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    let before = t.unsupported;
    feed(&mut t, "\x1b[2;2 kabc!");
    assert_eq!(t.unsupported, before + 1);
    let mut o = Reorder::new();
    assert_eq!(row_of(&t, &mut o, 0, Rows::ZERO), "abc!........");
}

/// **Ps2 1 moves the cursor to the line's first character position**, as
/// 8.3.111 states (Jev, 0.94/0.97); Ps2 0 and an omitted Ps2 leave it.
#[test]
fn ps2_one_moves_the_cursor_to_the_line_start() {
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, "abc\x1b[2;1 k");
    assert_eq!(t.cursor(), (Row::ZERO, Col::ZERO));
    feed(&mut t, "x\x1b[2;0 k");
    assert_eq!(t.cursor(), (Row::ZERO, Col::at_index(1)));
    feed(&mut t, "\x1b[1 k");
    assert_eq!(t.cursor(), (Row::ZERO, Col::at_index(1)));
}

/// **A wrapped line is one paragraph and keeps its first row's
/// direction**: the row the wrap brings in takes it.
#[test]
fn a_wrapped_line_keeps_its_direction_on_every_row() {
    let s = screen(&WS, "\x1b[2 kabcdefghijklmnop!");
    assert_eq!(s[0], "abcdefghijkl");
    assert_eq!(s[1], ".......!mnop");
}

/// **A paragraph whose first row has scrolled off keeps its direction**:
/// the row a wrap brings in takes the direction of the paragraph it joins,
/// so the rows still on the screen say it themselves.
#[test]
fn a_paragraph_whose_first_row_scrolled_off_keeps_its_direction() {
    let s = screen(&WS, "\x1b[2 kabcdefghijklmnopqrstuvwxyzABCDEFGHIJKLM!\r\n");
    assert_eq!(s[0], "mnopqrstuvwx");
    assert_eq!(s[2], "........!KLM");
}

/// **The direction goes into the history with its line and comes back
/// with it**: scrolled off and looked back at, a right-to-left line is
/// still drawn against the right edge.
#[test]
fn the_direction_survives_scrollback() {
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, "\x1b[2 kabc!\r\n\x1b[1 k1\r\n2\r\n3\r\n4\r\n5");
    let mut o = Reorder::new();
    assert_eq!(row_of(&t, &mut o, 0, Rows::of_index(2)), "........!abc");
    assert_eq!(row_of(&t, &mut o, 1, Rows::of_index(2)), "1...........");
}

/// **A reflow keeps each line's direction**: narrowed, the line wraps onto
/// two rows and both are drawn right to left; widened back, one row.
#[test]
fn the_direction_survives_reflow() {
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, "\x1b[2 kabcdefgh!\r\n\x1b[1 kxyz");
    let narrow = WinSize {
        ws_row: 4,
        ws_col: 6,
    };
    let _ = t.resize(&narrow);
    let mut o = Reorder::new();
    assert_eq!(row_of(&t, &mut o, 0, Rows::ZERO), "abcdef");
    assert_eq!(row_of(&t, &mut o, 1, Rows::ZERO), "...!gh");
    assert_eq!(row_of(&t, &mut o, 2, Rows::ZERO), "xyz...");
    let _ = t.resize(&WS);
    let mut o = Reorder::new();
    assert_eq!(row_of(&t, &mut o, 0, Rows::ZERO), "...!abcdefgh");
    assert_eq!(row_of(&t, &mut o, 1, Rows::ZERO), "xyz.........");
}

/// **RIS returns every line and the terminal to first strong**, the
/// initial state (8.3.105).
#[test]
fn ris_returns_every_line_to_first_strong() {
    let s = screen(&WS, "\x1b[2 kabc!\x1bcdef!\r\nghi!");
    assert_eq!(s[0], "def!........");
    assert_eq!(s[1], "ghi!........");
}

/// **ED spreads the direction over the lines it erases**, terminal-wg's
/// draft rule ("It is crucial that this sequence spreads the current BiDi
/// modes into the entire cleared area"; Jev's lean, DRAFT_RULES): after
/// `ED 2` a line the cursor is moved to is right to left.
#[test]
fn ed_spreads_the_direction_over_the_erased_lines() {
    let s = screen(&WS, "\x1b[2 k\x1b[2J\x1b[3;1Habc!");
    assert_eq!(s[2], "........!abc");
}

/// **The lines a scroll brings in take the direction**: SU's new line, and
/// IL's, are right to left.
#[test]
fn the_lines_a_scroll_brings_in_take_the_direction() {
    let s = screen(&WS, "\x1b[2 k\x1b[S\x1b[4;1Habc!");
    assert_eq!(s[3], "........!abc", "SU's line");
    let s = screen(&WS, "\x1b[2 k\x1b[2;1H\x1b[Ldef!");
    assert_eq!(s[1], "........!def", "IL's line");
}

/// **A line SD or IL brings in under a wrapped row is a paragraph of its
/// own**, drawn in the direction in force rather than the paragraph
/// above's: the draft's rule for SD and SU ("wherever lines are removed or
/// inserted, we ensure that the boundaries are all hard newlines"), which
/// baud applies to IL as well.
#[test]
fn a_line_sd_or_il_brings_in_under_a_wrapped_row_is_its_own_paragraph() {
    let wrapped = "\x1b[1 kabcdefghijklmn\x1b[4;1H\x1b[2 k";
    let s = screen(&WS, &format!("{wrapped}\x1b[2;4r\x1b[T\x1b[2;1Hxyz!"));
    assert_eq!(s[1], "........!xyz", "SD's line");
    let s = screen(&WS, &format!("{wrapped}\x1b[2;1H\x1b[Lxyz!"));
    assert_eq!(s[1], "........!xyz", "IL's line");
}

/// **SPD 3 turns every line of the screen at once** (8.3.130, "in the
/// presentation component ... takes effect immediately"; Jev's lean,
/// WHOLE_SCREEN), and SPD 0 sets every line left to right, a Hebrew one
/// included.
#[test]
fn spd_turns_every_line_of_the_screen() {
    let s = screen(&WS, &format!("abc!\r\n{HEBREW}\x1b[3 S"));
    assert_eq!(s[0], "........!abc");
    assert_eq!(s[1], "........!\u{5D2}\u{5D1}\u{5D0}");
    assert_eq!(s[3], "............");
    let s = screen(&WS, &format!("abc!\r\n{HEBREW}\x1b[ S"));
    assert_eq!(s[1], "\u{5D2}\u{5D1}\u{5D0}!........");
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    let before = t.unsupported;
    feed(&mut t, "\x1b[1 S\x1b[3;2 S");
    assert_eq!(t.unsupported, before + 2, "vertical, and Ps2 2");
}

/// **A table row keeps its columns, and each cell takes the direction**
/// (Jev's lean, CELLS_AT_SCP): each cell is a paragraph of its own (UAX #9
/// section 3), so HL1 sets each cell's level.
#[test]
fn a_table_row_under_scp_keeps_its_columns_and_turns_each_cell() {
    let s = screen(&WS, "\x1b[2 k\u{2502}ab!\u{2502}");
    assert_eq!(s[0], "\u{2502}!ab\u{2502}.......");
}

/// **With BDSM reset, SCP 2 draws a line in reverse**: terminal-wg's
/// explicit RTL, "The model's first column is displayed in the rightmost
/// cell ... Mirrorable glyphs are mirrored. The I-Beam shaped cursor is
/// displayed on the right side of its cell." A wide character's two cells
/// stay in their order.
#[test]
fn with_bdsm_reset_scp_2_draws_the_line_in_reverse() {
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, "\x1b[8l\x1b[2 kab(\u{4E00}");
    let mut o = Reorder::new();
    assert_eq!(
        row_of(&t, &mut o, 0, Rows::ZERO),
        ".......\u{4E00}~)ba",
        "the parenthesis drawn mirrored"
    );
    assert_eq!(o.orders().side(Row::ZERO, Col::ZERO), CursorSide::Right);
    // SCP 1 and the default keep the stored order.
    feed(&mut t, "\r\n\x1b[1 kab(");
    assert_eq!(row_of(&t, &mut o, 1, Rows::ZERO), "ab(.........");
}

/// **A direction changed on a line whose cells did not change is drawn**:
/// the layout does not keep the last frame's order for a row because its
/// cells are the same.
#[test]
fn a_direction_change_alone_redraws_the_line() {
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, "abc!");
    let mut o = Reorder::new();
    assert_eq!(row_of(&t, &mut o, 0, Rows::ZERO), "abc!........");
    feed(&mut t, "\x1b[2 k");
    assert_eq!(row_of(&t, &mut o, 0, Rows::ZERO), "........!abc");
    feed(&mut t, "\x1b[1 k");
    assert_eq!(row_of(&t, &mut o, 0, Rows::ZERO), "abc!........");
}
