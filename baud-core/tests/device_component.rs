//! **DCSM, `CSI 9 h` / `CSI 9 l`** (`[[source: ecma48]]` 7.2.3): with BDSM
//! reset and DCSM at PRESENTATION, the editing functions 7.2.3 lists take
//! the cursor's presentation position on a line holding an SDS or SRS
//! string, and act on the cells drawn at the positions they reach. Each
//! test sets the line `a b c` reversed, drawn `c b a`, so data column 0 is
//! drawn at presentation position 2 and column 2 at 0, and compares
//! PRESENTATION with DATA (`CSI 9 h`) and with BDSM set, where DCSM has no
//! effect.

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
    assert_eq!(after(PRESENTATION, "\x1b[1;1H\x1b[6n").2, "\x1b[1;3R");
    assert_eq!(after(DATA, "\x1b[1;1H\x1b[6n").2, "\x1b[1;1R");
    assert_eq!(after(IMPLICIT, "\x1b[1;1H\x1b[6n").2, "\x1b[1;1R");
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
        at(PRESENTATION, "\x1b[1;1H\n"),
        (Row::from_index(1), Col::at_index(2))
    );
    assert_eq!(at(DATA, "\x1b[1;1H\n"), (Row::from_index(1), Col::ZERO));
    assert_eq!(
        at(PRESENTATION, "\x1b[1;3H\x1bE"),
        (Row::from_index(1), Col::ZERO),
        "NEL: the home, which on row 1 is column 0"
    );
    assert_eq!(
        at(PRESENTATION, "\x1b[2;1H\x1bM\x1b[1;1H\x1b[2;3H\x1bM"),
        (Row::ZERO, Col::ZERO),
        "RI: column 2 of row 1 is position 2 of row 0, which holds a"
    );
}

/// **`DCH` deletes the cells drawn at the positions it covers**: two from
/// `b`, position 1, take `b` and `a`, drawn after it; with DATA, `b` and
/// `c`, stored after it.
#[test]
fn dch_deletes_the_cells_drawn_at_its_positions() {
    assert_eq!(after(PRESENTATION, "\x1b[1;2H\x1b[2P").0, "cy....");
    assert_eq!(after(DATA, "\x1b[1;2H\x1b[2P").0, "ay....");
}

/// **`ECH` and `EL` erase the cells drawn at their positions**, each where
/// it stands.
#[test]
fn ech_and_el_erase_the_cells_drawn_at_their_positions() {
    assert_eq!(after(PRESENTATION, "\x1b[1;2H\x1b[2X").0, "..cy..");
    assert_eq!(after(DATA, "\x1b[1;2H\x1b[2X").0, "a..y..");
    assert_eq!(after(PRESENTATION, "\x1b[1;2H\x1b[K").0, "..c...");
    assert_eq!(after(PRESENTATION, "\x1b[1;2H\x1b[1K").0, "a..y..");
    assert_eq!(after(DATA, "\x1b[1;2H\x1b[1K").0, "..cy..");
}

/// **`ICH` opens its blanks at the presentation position**: at `b`, which
/// runs against the line, they go after it in the cells, so they are drawn
/// before it; with DATA, before it in the cells.
#[test]
fn ich_opens_its_blanks_at_the_presentation_position() {
    assert_eq!(after(PRESENTATION, "\x1b[1;2H\x1b[@").0, "ab.cy.");
    assert_eq!(after(DATA, "\x1b[1;2H\x1b[@").0, "a.bcy.");
}
