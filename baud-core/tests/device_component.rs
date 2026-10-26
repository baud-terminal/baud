//! **DCSM, `CSI 9 h` / `CSI 9 l`** (`[[source: ecma48]]` 7.2.3): with BDSM
//! reset and DCSM at PRESENTATION, the editing functions 7.2.3 lists take
//! the cursor's presentation position on a line holding an SDS or SRS
//! string, and act on the cells drawn at the positions they reach. Each
//! test sets the line `a b c` reversed, drawn `c b a`, so data column 0 is
//! drawn at presentation position 2 and column 2 at 0, and compares
//! PRESENTATION with DATA (`CSI 9 h`) and with BDSM set, where DCSM has no
//! effect. The cursor is placed with `HVP` (`CSI Pn ; Pn f`), which moves
//! the active data position (8.3.63), so a setup names the same cell in
//! every state; `CUP`, `CHA`, `CUF`, `CUB`, `CUU`, `CUD` and the tab
//! functions move the active presentation position (8.3.9-8.3.22), and
//! under PRESENTATION they count the positions as drawn.

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
/// DATA it addresses the cells, as TR/53's worked example 2 has it
/// (`tr53_example_2_data_lines_address_the_cells`); with BDSM set the
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
    assert_eq!(at(DATA, "\x1b[1;1H"), col(0));
    assert_eq!(at(IMPLICIT, "\x1b[1;1H"), col(0));
    assert_eq!(after(PRESENTATION, "\x1b[1;1H\x1b[6n").2, "\x1b[1;1R");
}

/// **`CHA` goes to the cell drawn at the position it names** (8.3.9):
/// position 1 holds `c`, 2 `b`, and 4 `y`, past the string.
#[test]
fn cha_goes_to_the_cell_drawn_at_its_position() {
    let at = |m, then| after(m, then).1;
    let col = |c| (Row::ZERO, Col::at_index(c));
    assert_eq!(at(PRESENTATION, "\x1b[1G"), col(2));
    assert_eq!(at(PRESENTATION, "\x1b[2G"), col(1));
    assert_eq!(at(PRESENTATION, "\x1b[4G"), col(3));
    assert_eq!(at(DATA, "\x1b[1G"), col(0));
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
/// position 1, one right is `b`, two `a` and three `y`. Under DATA it
/// steps by cells.
#[test]
fn cuf_steps_by_presentation_positions() {
    let at = |m, then| after(m, then).1;
    let col = |c| (Row::ZERO, Col::at_index(c));
    assert_eq!(at(PRESENTATION, "\x1b[1;3f\x1b[C"), col(1));
    assert_eq!(at(PRESENTATION, "\x1b[1;3f\x1b[2C"), col(0));
    assert_eq!(at(PRESENTATION, "\x1b[1;3f\x1b[3C"), col(3));
    assert_eq!(at(DATA, "\x1b[1;3f\x1b[C"), col(3));
}

/// **`CUB` steps by presentation positions** (8.3.18): from `y`, drawn at
/// position 4, one left is `a` and three `c`. Under DATA it steps by
/// cells.
#[test]
fn cub_steps_by_presentation_positions() {
    let at = |m, then| after(m, then).1;
    let col = |c| (Row::ZERO, Col::at_index(c));
    assert_eq!(at(PRESENTATION, "\x1b[1;4f\x1b[D"), col(0));
    assert_eq!(at(PRESENTATION, "\x1b[1;4f\x1b[3D"), col(2));
    assert_eq!(at(DATA, "\x1b[1;4f\x1b[D"), col(2));
}

/// **`CUU` keeps the presentation position** on the line it reaches, as
/// `RI` does (8.3.22): column 0 of row 1 is position 1, which on row 0
/// holds `c`; and from row 1 holding the string, `a`, column 0, drawn at
/// position 3, is column 2 of row 0.
#[test]
fn cuu_keeps_the_presentation_position() {
    let at = |m, then| after(m, then).1;
    assert_eq!(
        at(PRESENTATION, "\x1b[2;1f\x1b[A"),
        (Row::ZERO, Col::at_index(2))
    );
    assert_eq!(at(DATA, "\x1b[2;1f\x1b[A"), (Row::ZERO, Col::ZERO));
    let from_the_string = |mode: &str| {
        support::with_term(&WS, |t| {
            let setup = format!("\x1b[8l\x1b[2;1f\x1b[1[abc\x1b[0[y{mode}\x1b[2;1f\x1b[A");
            support::feed(t, setup.as_bytes());
            t.cursor()
        })
    };
    assert_eq!(from_the_string(PRESENTATION), (Row::ZERO, Col::at_index(2)));
    assert_eq!(from_the_string(DATA), (Row::ZERO, Col::ZERO));
}

/// **`CUD` keeps the presentation position**, as `LF` does (8.3.19): `a`,
/// drawn at position 3, is column 2 of row 1.
#[test]
fn cud_keeps_the_presentation_position() {
    let at = |m, then| after(m, then).1;
    assert_eq!(
        at(PRESENTATION, "\x1b[1;1f\x1b[B"),
        (Row::from_index(1), Col::at_index(2))
    );
    assert_eq!(at(DATA, "\x1b[1;1f\x1b[B"), (Row::from_index(1), Col::ZERO));
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
/// at column 9 is position 9, which holds `a`.
#[test]
fn ht_counts_its_stops_in_presentation_positions() {
    let col = |c| (Row::ZERO, Col::at_index(c));
    assert_eq!(reversed_nine(PRESENTATION, "\x1b[1;9f\t"), col(0));
    assert_eq!(reversed_nine(DATA, "\x1b[1;9f\t"), col(9));
}

/// **`CHT` counts them so too** (8.3.10, "according to the character
/// path").
#[test]
fn cht_counts_its_stops_in_presentation_positions() {
    let col = |c| (Row::ZERO, Col::at_index(c));
    assert_eq!(reversed_nine(PRESENTATION, "\x1b[1;9f\x1b[I"), col(0));
    assert_eq!(reversed_nine(DATA, "\x1b[1;9f\x1b[I"), col(9));
}

/// **`CBT` counts them back** (8.3.7): from `a`, drawn at position 9, no
/// stop comes before the line's home, which holds `i`.
#[test]
fn cbt_counts_its_stops_in_presentation_positions() {
    let col = |c| (Row::ZERO, Col::at_index(c));
    assert_eq!(reversed_nine(PRESENTATION, "\x1b[1;1f\x1b[Z"), col(8));
    assert_eq!(reversed_nine(DATA, "\x1b[1;1f\x1b[Z"), col(0));
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
        (Row::from_index(2), Col::at_index(1))
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
        (Row::from_index(2), Col::at_index(1))
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
/// column 2, and at the home, column 0, it stays. Under DATA, and with
/// BDSM set, it steps by cells.
#[test]
fn on_a_right_to_left_line_cuf_moves_right_on_the_screen() {
    let col = |c| (Row::ZERO, Col::at_index(c));
    assert_eq!(right_to_left(PRESENTATION, "\x1b[C"), col(2));
    assert_eq!(right_to_left(PRESENTATION, "\x1b[9C"), col(0));
    assert_eq!(right_to_left(PRESENTATION, "\x1b[1;1f\x1b[C"), col(0));
    assert_eq!(right_to_left(DATA, "\x1b[C"), col(4));
    let implicit = format!("{PRESENTATION}{IMPLICIT}");
    assert_eq!(right_to_left(&implicit, "\x1b[C"), col(4));
}

/// **And `CUB` left** (8.3.18, "leftwards"): from column 3, column 4.
#[test]
fn on_a_right_to_left_line_cub_moves_left_on_the_screen() {
    let col = |c| (Row::ZERO, Col::at_index(c));
    assert_eq!(right_to_left(PRESENTATION, "\x1b[D"), col(4));
    assert_eq!(right_to_left(DATA, "\x1b[D"), col(2));
}

/// **TR/53's worked example 2, its line 3, under DATA** (`[[source:
/// ecma-tr53]]` Annex A): worked example 1's line written from column 1
/// rather than SLH's 7, so each published column less 6. `CUP(3;11)`
/// inserts at data column 5, between `m` and `n`, where position 5 holds
/// `v`; `CHA(25)` `DCH(2)` deletes `r` and `s`; `CHA(27)` replaces `u v w`.
/// The published data component: `kl_mABC123DEFnop_qt_GHI_xyz`.
#[test]
fn tr53_example_2_data_lines_address_the_cells() {
    const WIDE: WinSize = WinSize {
        ws_row: 2,
        ws_col: 40,
    };
    support::with_term(&WIDE, |t| {
        let line = "kl_\x1b[1[mnop_\x1b[1[qrst\x1b[0[_uvw\x1b[0[_xyz";
        let edit = "\x1b[9h\x1b[4h\x1b[1;5H\x1b[2]ABC\x1b[1]123\x1b[0]DEF\x1b[0]\
                    \x1b[19G\x1b[2P\x1b[21G\x1b[4l\x1b[2]GHI\x1b[0]";
        support::feed(t, format!("\x1b[8l{line}{edit}").as_bytes());
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
}

/// **`HTS` sets the stop at the presentation position** (8.3.62, "at the
/// active presentation position"): on `a`, drawn at position 3, the stop
/// goes to column 3, so an `HT` from column 1 of row 2 stops there.
#[test]
fn hts_sets_its_stop_at_the_presentation_position() {
    let row1 = |c| (Row::from_index(1), Col::at_index(c));
    let hts = "\x1b[3g\x1b[1;1f\x1bH\x1b[2;1f\t";
    assert_eq!(after(PRESENTATION, hts).1, row1(2));
    assert_eq!(after(DATA, hts).1, row1(9));
}

/// **`CTC 0` sets it there too** (8.3.17).
#[test]
fn ctc_0_sets_its_stop_at_the_presentation_position() {
    let row1 = |c| (Row::from_index(1), Col::at_index(c));
    let ctc = "\x1b[3g\x1b[1;1f\x1b[0W\x1b[2;1f\t";
    assert_eq!(after(PRESENTATION, ctc).1, row1(2));
    assert_eq!(after(DATA, ctc).1, row1(9));
}

/// **`TBC 0` clears the stop at the presentation position** (8.3.154):
/// with a stop at column 3, `TBC 0` on `a` clears it, so the `HT` goes on
/// to column 9.
#[test]
fn tbc_0_clears_the_stop_at_the_presentation_position() {
    let row1 = |c| (Row::from_index(1), Col::at_index(c));
    let clear = "\x1b[2;3f\x1bH\x1b[1;1f\x1b[0g\x1b[2;1f\t";
    assert_eq!(after(PRESENTATION, clear).1, row1(8));
    assert_eq!(after(DATA, clear).1, row1(2));
}
