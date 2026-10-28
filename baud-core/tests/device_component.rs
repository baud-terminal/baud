//! **DCSM, `CSI 9 h` / `CSI 9 l`** (`[[source: ecma48]]` 7.2.3): with BDSM
//! reset and DCSM at PRESENTATION, the editing functions 7.2.3 lists take
//! the cursor's presentation position on a line holding an SDS or SRS
//! string, and act on the cells drawn at the positions they reach. Each
//! test sets the line `a b c` reversed, drawn `c b a`, so data column 0 is
//! drawn at presentation position 2 and column 2 at 0, and compares
//! PRESENTATION with DATA (`CSI 9 h`) and with BDSM set, where DCSM has no
//! effect. The cursor is placed with `HVP` (`CSI Pn ; Pn f`), which moves
//! the active data position (8.3.63), so a setup names the same cell in
//! every state; `CUP`, `CHA`, `CUF`, `CUB`, `CUU`, `CUD`, `CNL`, `CPL`
//! and the tab functions move the active presentation position
//! (8.3.7-8.3.23), in every DCSM state, so they count the positions as
//! drawn under DATA too.

mod support;

use baud_core::*;

const WS: WinSize = WinSize {
    ws_row: 4,
    ws_col: 10,
};

/// The reversed string `abc` then `y` on row 0, BDSM reset, then `mode`
/// (DCSM and BDSM as a test asks) and `then`.
fn after(mode: &str, then: &str) -> (String, (Row, Col), String) {
    support::with_term(&WS, |t| {
        let setup = format!("\x1b[8l\x1b[1[abc\x1b[0[y{mode}{then}");
        support::feed(t, setup.as_bytes());
        let row: String = t.viewport().row(Row::ZERO)[..6]
            .iter()
            .map(|c| {
                c.content
                    .as_codepoint()
                    .and_then(|cp| char::from_u32(cp.value()))
                    .unwrap_or('.')
            })
            .collect();
        let cursor = t.cursor();
        let reply = String::from_utf8_lossy(t.reply()).into_owned();
        (row, cursor, reply)
    })
}

const PRESENTATION: &str = "\x1b[9l";
const DATA: &str = "\x1b[9h";
const IMPLICIT: &str = "\x1b[8h";

/// **`CPR` reports the presentation column**: data column 0, `a`, is
/// drawn at position 3, counted from 1.
#[test]
fn cpr_reports_the_presentation_column() {
    assert_eq!(after(PRESENTATION, "\x1b[1;1f\x1b[6n").2, "\x1b[1;3R");
    assert_eq!(after(DATA, "\x1b[1;1f\x1b[6n").2, "\x1b[1;1R");
    assert_eq!(after(IMPLICIT, "\x1b[1;1f\x1b[6n").2, "\x1b[1;1R");
}

/// **`CR` goes to the cell drawn at the line's home**: `c`, column 2.
#[test]
fn cr_goes_to_the_cell_drawn_at_the_home() {
    let at = |m| after(m, "\r").1;
    assert_eq!(at(PRESENTATION), (Row::ZERO, Col::at_index(2)));
    assert_eq!(at(DATA), (Row::ZERO, Col::ZERO));
    assert_eq!(at(IMPLICIT), (Row::ZERO, Col::ZERO));
}

/// **`LF`, `NEL` and `RI` keep the presentation position** on the line
/// they reach: from `a`, drawn at position 2, column 2 of a line with no
/// string.
#[test]
fn lf_nel_and_ri_keep_the_presentation_position() {
    let at = |m, then| after(m, then).1;
    assert_eq!(
        at(PRESENTATION, "\x1b[1;1f\n"),
        (Row::from_index(1), Col::at_index(2))
    );
    assert_eq!(at(DATA, "\x1b[1;1f\n"), (Row::from_index(1), Col::ZERO));
    assert_eq!(
        at(PRESENTATION, "\x1b[1;3f\x1bE"),
        (Row::from_index(1), Col::ZERO),
        "NEL: the home, which on row 1 is column 0"
    );
    assert_eq!(
        at(PRESENTATION, "\x1b[2;1f\x1bM\x1b[1;1f\x1b[2;3f\x1bM"),
        (Row::ZERO, Col::ZERO),
        "RI: column 2 of row 1 is position 2 of row 0, which holds a"
    );
}

/// **`FF` keeps the presentation position as `LF` does**: it is `LF` here,
/// DEC STD 070 p. 5-62's "identical in function to the Line Feed",
/// 8.3.51's move to the page home recorded as a departure.
#[test]
fn ff_keeps_the_presentation_position_as_lf_does() {
    let at = |m, then| after(m, then).1;
    assert_eq!(
        at(PRESENTATION, "\x1b[1;1f\x0c"),
        (Row::from_index(1), Col::at_index(2))
    );
    assert_eq!(at(DATA, "\x1b[1;1f\x0c"), at(DATA, "\x1b[1;1f\n"));
    assert_eq!(
        at(IMPLICIT, "\x1b[1;1f\x0c"),
        (Row::from_index(1), Col::ZERO)
    );
}

/// **SPA and EPA take the active presentation position** (8.3.129, 8.3.46:
/// "the active presentation position is the first", "the last ... of a
/// string of character positions in the presentation component"), in both
/// DCSM states, as the cursor functions do: SPA on `b`, drawn at position
/// 1, and EPA on `y`, at 3, guard the positions 1 to 3, drawn `b a y`, so
/// `EL 2` erases only `c`. With BDSM set they guard the cells `b c y`.
/// SPA on `a`, drawn at 2, guards `a y` alone.
#[test]
fn spa_and_epa_guard_the_positions_as_drawn() {
    let el = "\x1b[1;2f\x1bV\x1b[1;4f\x1bW\x1b[2K";
    assert_eq!(after(PRESENTATION, el).0, "ab.y..");
    assert_eq!(after(DATA, el).0, "ab.y..");
    assert_eq!(after(IMPLICIT, el).0, ".bcy..");
    let from_a = "\x1b[1;1f\x1bV\x1b[1;4f\x1bW\x1b[2K";
    assert_eq!(after(PRESENTATION, from_a).0, "a..y..");
    assert_eq!(after(IMPLICIT, from_a).0, "abcy..");
}

/// **`DCH` deletes the cells drawn at the positions it covers**: two from
/// `b`, position 1, take `b` and `a`, drawn after it; with DATA, `b` and
/// `c`, stored after it.
#[test]
fn dch_deletes_the_cells_drawn_at_its_positions() {
    assert_eq!(after(PRESENTATION, "\x1b[1;2f\x1b[2P").0, "cy....");
    assert_eq!(after(DATA, "\x1b[1;2f\x1b[2P").0, "ay....");
}

/// **`ECH` and `EL` erase the cells drawn at their positions**, each where
/// it stands.
#[test]
fn ech_and_el_erase_the_cells_drawn_at_their_positions() {
    assert_eq!(after(PRESENTATION, "\x1b[1;2f\x1b[2X").0, "..cy..");
    assert_eq!(after(DATA, "\x1b[1;2f\x1b[2X").0, "a..y..");
    assert_eq!(after(PRESENTATION, "\x1b[1;2f\x1b[K").0, "..c...");
    assert_eq!(after(PRESENTATION, "\x1b[1;2f\x1b[1K").0, "a..y..");
    assert_eq!(after(DATA, "\x1b[1;2f\x1b[1K").0, "..cy..");
}

/// **`ICH` opens its blanks at the presentation position**: at `b`, which
/// runs against the line, they go after it in the cells, so they are drawn
/// before it; with DATA, before it in the cells.
#[test]
fn ich_opens_its_blanks_at_the_presentation_position() {
    assert_eq!(after(PRESENTATION, "\x1b[1;2f\x1b[@").0, "ab.cy.");
    assert_eq!(after(DATA, "\x1b[1;2f\x1b[@").0, "a.bcy.");
}

/// **`CUP` goes to the cell drawn at the position it names** under
/// PRESENTATION (`[[source: ecma48]]` 8.3.21: "the active presentation
/// position"): position 1 holds `c`, column 2, and position 3 `a`. Under
/// DATA too: its definition has no DCSM paragraph and 7.2.3's NOTE does
/// not list it, where TR/53's worked example 2 addresses the cells
/// (`tr53_example_2_data_lines_by_the_data_movers`); with BDSM set the
/// control functions are performed in the data component (7.2.1).
#[test]
fn cup_goes_to_the_cell_drawn_at_its_position() {
    let at = |m, then| after(m, then).1;
    let col = |c| (Row::ZERO, Col::at_index(c));
    assert_eq!(at(PRESENTATION, "\x1b[1;1H"), col(2));
    assert_eq!(at(PRESENTATION, "\x1b[1;3H"), col(0));
    assert_eq!(
        at(PRESENTATION, "\x1b[2;1H\x1b[1;1H"),
        col(2),
        "from another line"
    );
    assert_eq!(at(DATA, "\x1b[1;1H"), col(2));
    assert_eq!(at(DATA, "\x1b[1;3H"), col(0));
    assert_eq!(at(IMPLICIT, "\x1b[1;1H"), col(0));
    assert_eq!(after(PRESENTATION, "\x1b[1;1H\x1b[6n").2, "\x1b[1;1R");
}

/// **`CHA` goes to the cell drawn at the position it names** (8.3.9):
/// position 1 holds `c`, 2 `b`, and 4 `y`, past the string, in both
/// DCSM states.
#[test]
fn cha_goes_to_the_cell_drawn_at_its_position() {
    let at = |m, then| after(m, then).1;
    let col = |c| (Row::ZERO, Col::at_index(c));
    assert_eq!(at(PRESENTATION, "\x1b[1G"), col(2));
    assert_eq!(at(PRESENTATION, "\x1b[2G"), col(1));
    assert_eq!(at(PRESENTATION, "\x1b[4G"), col(3));
    assert_eq!(at(DATA, "\x1b[1G"), col(2));
    assert_eq!(at(DATA, "\x1b[2G"), col(1));
    assert_eq!(at(IMPLICIT, "\x1b[1G"), col(0));
}

/// **`HVP` and `HPA` move the active data position** (8.3.63, 8.3.57), so
/// they address the cells in every state.
#[test]
fn hvp_and_hpa_address_the_cells() {
    let at = |m, then| after(m, then).1;
    assert_eq!(at(PRESENTATION, "\x1b[1;1f"), (Row::ZERO, Col::ZERO));
    assert_eq!(at(PRESENTATION, "\x1b[1;4H\x1b[1`"), (Row::ZERO, Col::ZERO));
    assert_eq!(at(DATA, "\x1b[1;1f"), (Row::ZERO, Col::ZERO));
}

/// **`CUF` steps by presentation positions** (8.3.20): from `c`, drawn at
/// position 1, one right is `b`, two `a` and three `y`, under DATA too.
#[test]
fn cuf_steps_by_presentation_positions() {
    let at = |m, then| after(m, then).1;
    let col = |c| (Row::ZERO, Col::at_index(c));
    assert_eq!(at(PRESENTATION, "\x1b[1;3f\x1b[C"), col(1));
    assert_eq!(at(PRESENTATION, "\x1b[1;3f\x1b[2C"), col(0));
    assert_eq!(at(PRESENTATION, "\x1b[1;3f\x1b[3C"), col(3));
    assert_eq!(at(DATA, "\x1b[1;3f\x1b[C"), col(1));
    assert_eq!(at(DATA, "\x1b[1;3f\x1b[3C"), col(3));
}

/// **`CUB` steps by presentation positions** (8.3.18): from `y`, drawn at
/// position 4, one left is `a` and three `c`, under DATA too.
#[test]
fn cub_steps_by_presentation_positions() {
    let at = |m, then| after(m, then).1;
    let col = |c| (Row::ZERO, Col::at_index(c));
    assert_eq!(at(PRESENTATION, "\x1b[1;4f\x1b[D"), col(0));
    assert_eq!(at(PRESENTATION, "\x1b[1;4f\x1b[3D"), col(2));
    assert_eq!(at(DATA, "\x1b[1;4f\x1b[D"), col(0));
    assert_eq!(at(DATA, "\x1b[1;4f\x1b[3D"), col(2));
}

/// **`CUU` keeps the presentation position** on the line it reaches, as
/// `RI` does (8.3.22): column 0 of row 1 is position 1, which on row 0
/// holds `c`; and from row 1 holding the string, `a`, column 0, drawn at
/// position 3, is column 2 of row 0. Under DATA too.
#[test]
fn cuu_keeps_the_presentation_position() {
    let at = |m, then| after(m, then).1;
    assert_eq!(
        at(PRESENTATION, "\x1b[2;1f\x1b[A"),
        (Row::ZERO, Col::at_index(2))
    );
    assert_eq!(at(DATA, "\x1b[2;1f\x1b[A"), (Row::ZERO, Col::at_index(2)));
    let from_the_string = |mode: &str| {
        support::with_term(&WS, |t| {
            let setup = format!("\x1b[8l\x1b[2;1f\x1b[1[abc\x1b[0[y{mode}\x1b[2;1f\x1b[A");
            support::feed(t, setup.as_bytes());
            t.cursor()
        })
    };
    assert_eq!(from_the_string(PRESENTATION), (Row::ZERO, Col::at_index(2)));
    assert_eq!(from_the_string(DATA), (Row::ZERO, Col::at_index(2)));
}

/// **`CUD` keeps the presentation position**, as `LF` does (8.3.19): `a`,
/// drawn at position 3, is column 2 of row 1, under DATA too.
#[test]
fn cud_keeps_the_presentation_position() {
    let at = |m, then| after(m, then).1;
    assert_eq!(
        at(PRESENTATION, "\x1b[1;1f\x1b[B"),
        (Row::from_index(1), Col::at_index(2))
    );
    assert_eq!(
        at(DATA, "\x1b[1;1f\x1b[B"),
        (Row::from_index(1), Col::at_index(2))
    );
}

/// The reversed string `abcdefghi` on row 0, drawn `ihgfedcba`, BDSM
/// reset, then `mode` and `then`: the cursor.
fn reversed_nine(mode: &str, then: &str) -> (Row, Col) {
    support::with_term(&WS, |t| {
        let setup = format!("\x1b[8l\x1b[1[abcdefghi\x1b[0[{mode}{then}");
        support::feed(t, setup.as_bytes());
        t.cursor()
    })
}

/// **`HT` counts the tab stops in presentation positions** (8.3.60: "in
/// the presentation component"): from `i`, drawn at position 1, the stop
/// at column 9 is position 9, which holds `a`, in both DCSM states.
#[test]
fn ht_counts_its_stops_in_presentation_positions() {
    let col = |c| (Row::ZERO, Col::at_index(c));
    assert_eq!(reversed_nine(PRESENTATION, "\x1b[1;9f\t"), col(0));
    assert_eq!(reversed_nine(DATA, "\x1b[1;9f\t"), col(0));
}

/// **`CHT` counts them so too** (8.3.10, "according to the character
/// path").
#[test]
fn cht_counts_its_stops_in_presentation_positions() {
    let col = |c| (Row::ZERO, Col::at_index(c));
    assert_eq!(reversed_nine(PRESENTATION, "\x1b[1;9f\x1b[I"), col(0));
    assert_eq!(reversed_nine(DATA, "\x1b[1;9f\x1b[I"), col(0));
}

/// **`CBT` counts them back** (8.3.7): from `a`, drawn at position 9, no
/// stop comes before the line's home, which holds `i`.
#[test]
fn cbt_counts_its_stops_in_presentation_positions() {
    let col = |c| (Row::ZERO, Col::at_index(c));
    assert_eq!(reversed_nine(PRESENTATION, "\x1b[1;1f\x1b[Z"), col(8));
    assert_eq!(reversed_nine(DATA, "\x1b[1;1f\x1b[Z"), col(8));
}

/// A line tabulation stop on row 3, then the cursor on `b`, drawn at
/// position 8.
const LINE_STOP: &str = "\x1b[3;1f\x1bJ\x1b[1;2f";

/// **`VT` keeps the presentation position** on the line of the stop
/// (8.3.161, "the corresponding character position"): column 8 of row 3.
#[test]
fn vt_keeps_the_presentation_position() {
    let vt = format!("{LINE_STOP}\x0b");
    assert_eq!(
        reversed_nine(PRESENTATION, &vt),
        (Row::from_index(2), Col::at_index(7))
    );
    assert_eq!(
        reversed_nine(DATA, &vt),
        (Row::from_index(2), Col::at_index(7))
    );
}

/// **`CVT` keeps it too** (8.3.23).
#[test]
fn cvt_keeps_the_presentation_position() {
    let cvt = format!("{LINE_STOP}\x1b[Y");
    assert_eq!(
        reversed_nine(PRESENTATION, &cvt),
        (Row::from_index(2), Col::at_index(7))
    );
    assert_eq!(
        reversed_nine(DATA, &cvt),
        (Row::from_index(2), Col::at_index(7))
    );
}

/// `SCP 2` on row 0 with BDSM reset, `mode`, `abc` written, then `then`:
/// the cursor. Column 0 is drawn at the right edge.
fn right_to_left(mode: &str, then: &str) -> (Row, Col) {
    support::with_term(&WS, |t| {
        let setup = format!("\x1b[8l{mode}\x1b[2 kabc{then}");
        support::feed(t, setup.as_bytes());
        t.cursor()
    })
}

/// **On a right-to-left line `CUF` moves right on the screen** (8.3.20,
/// "rightwards in the presentation component"): from column 3 right is
/// column 2, and at the home, column 0, it stays, under DATA too. With
/// BDSM set it steps by cells.
#[test]
fn on_a_right_to_left_line_cuf_moves_right_on_the_screen() {
    let col = |c| (Row::ZERO, Col::at_index(c));
    assert_eq!(right_to_left(PRESENTATION, "\x1b[C"), col(2));
    assert_eq!(right_to_left(PRESENTATION, "\x1b[9C"), col(0));
    assert_eq!(right_to_left(PRESENTATION, "\x1b[1;1f\x1b[C"), col(0));
    assert_eq!(right_to_left(DATA, "\x1b[C"), col(2));
    let implicit = format!("{PRESENTATION}{IMPLICIT}");
    assert_eq!(right_to_left(&implicit, "\x1b[C"), col(4));
}

/// **And `CUB` left** (8.3.18, "leftwards"): from column 3, column 4, in
/// both DCSM states.
#[test]
fn on_a_right_to_left_line_cub_moves_left_on_the_screen() {
    let col = |c| (Row::ZERO, Col::at_index(c));
    assert_eq!(right_to_left(PRESENTATION, "\x1b[D"), col(4));
    assert_eq!(right_to_left(DATA, "\x1b[D"), col(4));
    let implicit = format!("{PRESENTATION}{IMPLICIT}");
    assert_eq!(right_to_left(&implicit, "\x1b[D"), col(2));
}

/// **TR/53's worked example 2, its line 3, under DATA** (`[[source:
/// ecma-tr53]]` Annex A): worked example 1's line written from column 1
/// rather than SLH's 7, so each published column less 6. TR/53 reaches its
/// data columns with `CUP(3;11)`, `CHA(25)` and `CHA(27)`, where ECMA-48's
/// `CUP` and `CHA` move the active presentation position in both DCSM
/// states (8.3.21, 8.3.9); `HVP` and `HPA`, which move the active data
/// position (8.3.63, 8.3.57), reach them: the insertion at data column 5,
/// between `m` and `n`, `DCH(2)` deleting `r` and `s`, and `u v w`
/// replaced, the published data component `kl_mABC123DEFnop_qt_GHI_xyz`.
/// `CUP(1;5)` goes to the cell drawn at position 5, `v`, column 14
/// counted from 0.
#[test]
fn tr53_example_2_data_lines_by_the_data_movers() {
    const WIDE: WinSize = WinSize {
        ws_row: 2,
        ws_col: 40,
    };
    let line = "\x1b[8lkl_\x1b[1[mnop_\x1b[1[qrst\x1b[0[_uvw\x1b[0[_xyz";
    support::with_term(&WIDE, |t| {
        let edit = "\x1b[9h\x1b[4h\x1b[1;5f\x1b[2]ABC\x1b[1]123\x1b[0]DEF\x1b[0]\
                    \x1b[19`\x1b[2P\x1b[21`\x1b[4l\x1b[2]GHI\x1b[0]";
        support::feed(t, format!("{line}{edit}").as_bytes());
        let row: String = t.viewport().row(Row::ZERO)[..27]
            .iter()
            .map(|c| {
                c.content
                    .as_codepoint()
                    .and_then(|cp| char::from_u32(cp.value()))
                    .unwrap_or('.')
            })
            .collect();
        assert_eq!(row, "kl_mABC123DEFnop_qt_GHI_xyz");
    });
    support::with_term(&WIDE, |t| {
        support::feed(t, format!("{line}\x1b[9h\x1b[1;5H").as_bytes());
        assert_eq!(t.cursor(), (Row::ZERO, Col::at_index(14)));
    });
}

/// **`CNL` and `CPL` go to the cell drawn at the line's first position**
/// (8.3.12, 8.3.13: "the first character position of the n-th following
/// line in the presentation component"), in both DCSM states: on row 0
/// that is `c`, column 2, where `CR` under DATA goes to column 0.
#[test]
fn cnl_and_cpl_go_to_the_cell_drawn_at_the_first_position() {
    let at = |mode, then| after(mode, then).1;
    let col = |c| (Row::ZERO, Col::at_index(c));
    for m in [PRESENTATION, DATA] {
        assert_eq!(at(m, "\x1b[2;5f\x1b[F"), col(2), "CPL");
    }
    assert_eq!(at(IMPLICIT, "\x1b[2;5f\x1b[F"), col(0));
    let from_above = |mode: &str| {
        support::with_term(&WS, |t| {
            let setup = format!("\x1b[8l\x1b[2;1f\x1b[1[abc\x1b[0[{mode}\x1b[1;5f\x1b[E");
            support::feed(t, setup.as_bytes());
            t.cursor()
        })
    };
    assert_eq!(
        from_above(PRESENTATION),
        (Row::from_index(1), Col::at_index(2))
    );
    assert_eq!(from_above(DATA), (Row::from_index(1), Col::at_index(2)));
    assert_eq!(at(DATA, "\x1b[1;2f\r"), col(0), "CR under DATA");
}

/// **`HTS` sets the stop at the presentation position** (8.3.62, "at the
/// active presentation position"): on `a`, drawn at position 3, the stop
/// goes to column 3, so an `HT` from column 1 of row 2 stops there, in
/// both DCSM states.
#[test]
fn hts_sets_its_stop_at_the_presentation_position() {
    let row1 = |c| (Row::from_index(1), Col::at_index(c));
    let hts = "\x1b[3g\x1b[1;1f\x1bH\x1b[2;1f\t";
    assert_eq!(after(PRESENTATION, hts).1, row1(2));
    assert_eq!(after(DATA, hts).1, row1(2));
}

/// **`CTC 0` sets it there too** (8.3.17).
#[test]
fn ctc_0_sets_its_stop_at_the_presentation_position() {
    let row1 = |c| (Row::from_index(1), Col::at_index(c));
    let ctc = "\x1b[3g\x1b[1;1f\x1b[0W\x1b[2;1f\t";
    assert_eq!(after(PRESENTATION, ctc).1, row1(2));
    assert_eq!(after(DATA, ctc).1, row1(2));
}

/// **`TBC 0` clears the stop at the presentation position** (8.3.154):
/// with a stop at column 3, `TBC 0` on `a` clears it, so the `HT` goes on
/// to column 9.
#[test]
fn tbc_0_clears_the_stop_at_the_presentation_position() {
    let row1 = |c| (Row::from_index(1), Col::at_index(c));
    let clear = "\x1b[2;3f\x1bH\x1b[1;1f\x1b[0g\x1b[2;1f\t";
    assert_eq!(after(PRESENTATION, clear).1, row1(8));
    assert_eq!(after(DATA, clear).1, row1(8));
}

/// **`TSR` names a presentation position** (8.3.156, "character position
/// n in the active line"), the one `HTS` sets its stop at in both DCSM
/// states: the stop `HTS` set on `a`, drawn at position 3, goes with
/// `TSR 3`, and `TSR 1` leaves it.
#[test]
fn tsr_clears_the_stop_hts_set_at_the_presentation_position() {
    let row1 = |c| (Row::from_index(1), Col::at_index(c));
    let set = "\x1b[3g\x1b[2;9f\x1bH\x1b[1;1f\x1bH";
    let gone = format!("{set}\x1b[3 d\x1b[2;1f\t");
    assert_eq!(after(PRESENTATION, &gone).1, row1(8));
    assert_eq!(after(DATA, &gone).1, row1(8));
    let kept = format!("{set}\x1b[1 d\x1b[2;1f\t");
    assert_eq!(after(PRESENTATION, &kept).1, row1(2));
    assert_eq!(after(DATA, &kept).1, row1(2));
}

/// **HTJ shifts the field's positions as drawn** (8.3.61, "the contents of
/// the active field (the field in the presentation component ...)"): with
/// a stop at 6 and the cursor on `y`, drawn at 3, the content drawn at 0
/// to 3, `c b a y`, moves to 2 to 5, each cell keeping its string, so `c`
/// is stored at column 0. In both DCSM states, as `HT` counts its stops;
/// with BDSM set the cells.
#[test]
fn htj_shifts_the_positions_drawn() {
    let htj = "\x1b[3g\x1b[1;7f\x1bH\x1b[1;4f\x1bI";
    assert_eq!(after(PRESENTATION, htj).0, "c..bay");
    assert_eq!(after(DATA, htj).0, "c..bay");
    assert_eq!(after(IMPLICIT, htj).0, "..abcy");
    assert_eq!(after(PRESENTATION, htj).1, (Row::ZERO, Col::at_index(6)));
    assert_eq!(
        after(PRESENTATION, &format!("{htj}\x1b[1;1f\x1b[6n")).2,
        "\x1b[1;3R",
        "column 0 is still in the string, drawn at position 2"
    );
}

/// **An aligned stop is found and laid out at the positions drawn**: TALE
/// at position 5, column 4; from `a`, drawn at 2, `HT` reaches it, and
/// `de` ends before it, `d` moved back to position 2, the cell drawn there
/// column 0.
#[test]
fn an_aligned_string_moves_by_the_positions_drawn() {
    let tale = "\x1b[3g\x1b[5 a\x1b[1;1f\tde";
    assert_eq!(after(PRESENTATION, tale).0, "dbce..");
    assert_eq!(after(DATA, tale).0, "dbce..");
    assert_eq!(after(IMPLICIT, tale).0, "abde..");
    // TALE at position 3, column 2, drawn over column 0: from `c`, at 0,
    // `HT` lands there, so the stop is found at the position, not at the
    // cell the cursor stands on.
    let inside = "\x1b[3g\x1b[3 a\x1b[1;3f\td";
    assert_ne!(
        after(PRESENTATION, inside).0,
        after(PRESENTATION, "\x1b[3g\x1b[3 `\x1b[1;3f\td").0
    );
}

/// **SEE's extents shift the positions drawn under PRESENTATION** (the
/// `ICH` they bound acts there "in the presentation component", 8.3.64):
/// `ICH` on `b`, drawn at 1, under the page moves `b a y` on to 2 to 4;
/// under DATA it shifts the cells.
#[test]
fn see_shifts_the_positions_drawn_under_presentation() {
    let ich = "\x1b[Q\x1b[1;2f\x1b[@";
    assert_eq!(after(PRESENTATION, ich).0, "b.cay.");
    assert_eq!(after(DATA, ich).0, "a.bcy.");
    assert_eq!(after(IMPLICIT, ich).0, "a.bcy.");
}

/// **`EF` erases the field holding the presentation position** (8.3.40:
/// "the field which contains the active presentation position in the
/// presentation component"), its stops counted in presentation positions
/// as `HT` counts them. A stop at column 3 of row 1, the line with no
/// string, is position 2 everywhere: the field `[2, 8)` holds `a` and `y`
/// as drawn, `[0, 2)` `c` and `b`. Under DATA the fields are the cells'.
#[test]
fn ef_erases_the_field_drawn_at_its_position() {
    let row = |m, then: &str| after(m, &format!("\x1b[2;3f\x1bH{then}")).0;
    assert_eq!(row(PRESENTATION, "\x1b[1;1f\x1b[N"), ".bc...");
    assert_eq!(row(DATA, "\x1b[1;1f\x1b[N"), "..cy..");
    assert_eq!(row(PRESENTATION, "\x1b[1;2f\x1b[1N"), "a..y..");
    assert_eq!(row(DATA, "\x1b[1;2f\x1b[1N"), "..cy..");
    assert_eq!(row(PRESENTATION, "\x1b[1;2f\x1b[2N"), "a..y..");
    assert_eq!(row(PRESENTATION, "\x1b[1;4f\x1b[2N"), ".bc...");
}

/// The reversed string `a b c`, `b` guarded by `SPA` and `EPA`, then `y`,
/// on row 0 and `z` on row 1, BDSM reset, `mode` and `then`: rows 0 and 1.
fn guarded(mode: &str, then: &str) -> (String, String) {
    support::with_term(&WS, |t| {
        let setup = format!("\x1b[8l\x1b[1[a\x1bVb\x1bWc\x1b[0[y\x1b[2;1fz{mode}{then}");
        support::feed(t, setup.as_bytes());
        let row = |r: usize| -> String {
            t.viewport().row(Row::from_index(r))[..6]
                .iter()
                .map(|c| {
                    c.content
                        .as_codepoint()
                        .and_then(|cp| char::from_u32(cp.value()))
                        .unwrap_or('.')
                })
                .collect()
        };
        (row(0), row(1))
    })
}

/// **`EA` erases the qualified area holding the presentation position**
/// (8.3.37: "the qualified area in the presentation component which
/// contains the active presentation position"): drawn `c b a y`, the
/// guarded `b` at position 1 parts `c` from `a y` and what follows. From
/// `a`, `EA 0` erases `a`, `y` and row 1, where under DATA `b`, stored
/// after `a`, ends the area at once; from `y`, `EA 1` erases `a` and `y`,
/// where under DATA `c` and `y`.
#[test]
fn ea_erases_the_area_drawn_at_its_position() {
    let at = |m, then| guarded(m, then);
    assert_eq!(
        at(PRESENTATION, "\x1b[1;1f\x1b[O"),
        (".bc...".into(), "......".into())
    );
    assert_eq!(
        at(DATA, "\x1b[1;1f\x1b[O"),
        (".bcy..".into(), "z.....".into())
    );
    assert_eq!(
        at(PRESENTATION, "\x1b[1;4f\x1b[1O"),
        (".bc...".into(), "z.....".into())
    );
    assert_eq!(
        at(DATA, "\x1b[1;4f\x1b[1O"),
        ("ab....".into(), "z.....".into())
    );
    assert_eq!(
        at(PRESENTATION, "\x1b[1;3f\x1b[2O"),
        ("ab.y..".into(), "z.....".into()),
        "from c, position 0: the area is c alone"
    );
}

/// **A shift by the positions drawn keeps a wide character whole**: on the
/// reversed string `a中c` then `y`, in both DCSM states and from every
/// column, `ICH`, `DCH` and a write under IRM under SEE 0 and SEE 2, and
/// `HTJ`, leave no lead without its tail and no tail without its lead.
#[test]
fn a_shift_by_the_positions_drawn_keeps_a_wide_character_whole() {
    let mut split = Vec::new();
    for mode in [PRESENTATION, DATA] {
        for col in 1..=6 {
            for op in [
                "\x1b[Q\x1b[@",
                "\x1b[Q\x1b[P",
                "\x1b[Q\x1b[2@",
                "\x1b[Q\x1b[2P",
                "\x1b[Q\x1b[4hX",
                "\x1b[2Q\x1b[@",
                "\x1b[2Q\x1b[P",
                "\x1b[2Q\x1b[4hX",
                "\x1b[3g\x1b[1;9f\x1bH\x1b[1;{col}f\x1bI",
            ] {
                let op = op.replace("{col}", &col.to_string());
                let bytes = format!("\x1b[8l\x1b[1[a\u{4e2d}c\x1b[0[y{mode}\x1b[1;{col}f{op}");
                support::with_term(&WS, |t| {
                    support::feed(t, bytes.as_bytes());
                    let row = t.viewport().row(Row::ZERO);
                    let lead =
                        |i: usize| row[i].content.as_codepoint().map(|c| c.value()) == Some(0x4e2d);
                    for i in 0..10 {
                        let tail = row[i].content.is_wide_tail();
                        if (lead(i) && !row[i + 1].content.is_wide_tail())
                            || (tail && (i == 0 || !lead(i - 1)))
                        {
                            split.push(format!("{mode:?} col {col} {op:?} at {i}"));
                        }
                    }
                });
            }
        }
    }
    assert!(split.is_empty(), "{split:#?}");
}
