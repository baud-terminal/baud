//! SL and SR, `CSI Ps SP @` and `CSI Ps SP A`: the screen's contents move
//! left or right, and `CSI Ps ^`, which is not SD.
//!
//! `[[source: ecma48]]` 8.3.121: "SL causes the data in the presentation
//! component to be moved by n character positions ... such that the data
//! appear to move to the left ... The active presentation position is not
//! affected by this control function." 8.3.135 is the mirror. Bounded by the
//! four margins, `[[source: dec-std-070]]` § 5.4.3: "All scrolling, both
//! vertical and horizontal, is limited to this area". Decided by Jev
//! 2026-10-19: no gate on where the cursor stands (NO_GATE, 0.96/0.96), and
//! the columns brought in take the fill SU and SD give a line (SCROLL_FILL,
//! 0.96/0.96).

use crate::support;
use baud_core::*;

const COLS: usize = 20;
const WS: WinSize = WinSize {
    ws_row: 8,
    ws_col: 20,
};

fn fill() -> String {
    let mut s = String::new();
    for r in 1..=8 {
        s.push_str(&format!("\x1b[{r};1H0123456789abcdefghij"));
    }
    s
}

fn row(t: &Term<'_>, y: usize) -> String {
    t.row(Row::from_index(y - 1)).expect("a row")[..COLS]
        .iter()
        .map(|c| match c.content.as_codepoint() {
            Some(cp) => char::from_u32(cp.value()).unwrap_or('?'),
            None => '.',
        })
        .collect()
}

#[test]
fn sl_moves_every_row_left_and_blanks_the_right() {
    let bytes = format!("{}\x1b[4;7H\x1b[3 @", fill());
    support::render(&WS, bytes.as_bytes(), |t| {
        for y in 1..=8 {
            assert_eq!(row(t, y), "3456789abcdefghij...", "row {y}");
        }
        assert_eq!(
            t.cursor(),
            (Row::from_index(3), Col::at_index(6)),
            "unmoved"
        );
        assert_eq!(t.unsupported, 0, "performed, not counted");
    });
}

#[test]
fn sr_moves_every_row_right_and_blanks_the_left() {
    let bytes = format!("{}\x1b[2 A", fill());
    support::render(&WS, bytes.as_bytes(), |t| {
        for y in 1..=8 {
            assert_eq!(row(t, y), "..0123456789abcdefgh", "row {y}");
        }
        assert_eq!(t.unsupported, 0);
    });
}

#[test]
fn an_omitted_or_zero_count_is_one() {
    for seq in ["\x1b[ @", "\x1b[0 @"] {
        let bytes = format!("{}{seq}", fill());
        support::render(&WS, bytes.as_bytes(), |t| {
            assert_eq!(row(t, 1), "123456789abcdefghij.", "{seq:?}");
        });
    }
}

#[test]
fn a_count_past_the_width_blanks_the_area() {
    let bytes = format!("{}\x1b[999 A", fill());
    support::render(&WS, bytes.as_bytes(), |t| {
        assert_eq!(row(t, 5), "....................");
    });
}

/// Inside the four margins only, and wherever the cursor is: here it stands
/// at the home position, outside both pairs.
#[test]
fn the_margins_bound_the_shift_and_the_cursor_does_not_gate_it() {
    let bytes = format!("{}\x1b[2;4r\x1b[?69h\x1b[5;10s\x1b[1;1H\x1b[2 @", fill());
    support::render(&WS, bytes.as_bytes(), |t| {
        assert_eq!(row(t, 1), "0123456789abcdefghij", "above the top margin");
        for y in 2..=4 {
            assert_eq!(row(t, y), "01236789..abcdefghij", "row {y}");
        }
        assert_eq!(row(t, 5), "0123456789abcdefghij", "below the bottom margin");
        assert_eq!(t.cursor(), (Row::from_index(0), Col::ZERO));
    });
    let bytes = format!("{}\x1b[2;4r\x1b[?69h\x1b[5;10s\x1b[8;20H\x1b[ A", fill());
    support::render(&WS, bytes.as_bytes(), |t| {
        assert_eq!(row(t, 3), "0123.45678abcdefghij");
    });
}

/// The columns brought in are blanked as SU and SD blank a line: the current
/// background, and the default under DECECM `?117` set.
#[test]
fn the_columns_brought_in_take_the_scroll_fill() {
    let bytes = format!("{}\x1b[44m\x1b[2 @", fill());
    support::render(&WS, bytes.as_bytes(), |t| {
        let r = t.row(Row::from_index(0)).expect("a row");
        let opened = t.style_of(r[19]);
        let kept = t.style_of(r[0]);
        assert_eq!(kept, StyleWord::DEFAULT, "moved text keeps its rendition");
        assert_ne!(opened, StyleWord::DEFAULT, "the current background");
        let su = format!("{}\x1b[44m\x1b[S", fill());
        let su_fill = support::render(&WS, su.as_bytes(), |u| {
            u.style_of(u.row(Row::from_index(7)).expect("a row")[19])
        });
        assert_eq!(opened, su_fill, "the fill SU gives a line");
    });
    let bytes = format!("{}\x1b[?117h\x1b[44m\x1b[2 A", fill());
    support::render(&WS, bytes.as_bytes(), |t| {
        let r = t.row(Row::from_index(0)).expect("a row");
        assert_eq!(t.style_of(r[0]), StyleWord::DEFAULT, "DECECM set");
    });
}

/// **On a double-width line the text moves to the right margin**, as
/// DEC's own horizontal shifts move it: DECFI and DECBI shift "one column
/// regardless of their line attributes", and the scroll_left and
/// scroll_right routines they call run every row of the region to the
/// right margin (`[[source: dec-std-070]]` pp. 5-37, 5-39, 5-212 and
/// 5-213). ECMA-48 has no double-width line. Decided by Jev, RIGHT_MARGIN,
/// 0.94/0.96 on a second ask (0.82/0.78 on the first); xterm's SL and SR
/// shift each row to its right margin too (`xtermColScroll`). What a shift
/// brings in at the half is what lay past it, blank here, since DECDWL
/// left the line's second half empty; what SR pushes past the half is
/// kept, and the line made single-width again shows it (p. 5-96, DECSWL
/// "does not cause data contained in the Active Line to be lost").
#[test]
fn a_double_width_line_shifts_to_the_right_margin() {
    let bytes = format!("{}\x1b[2;1H\x1b#6\x1b[2 @", fill());
    support::render(&WS, bytes.as_bytes(), |t| {
        assert_eq!(&row(t, 2)[..10], "23456789..", "what lay past the half");
        assert_eq!(
            row(t, 1),
            "23456789abcdefghij..",
            "a single line, its whole width"
        );
    });
    let bytes = format!("{}\x1b[2;1H\x1b#6\x1b[3 A\x1b#5", fill());
    support::render(&WS, bytes.as_bytes(), |t| {
        assert_eq!(row(t, 2), "...0123456789.......");
    });
}

/// `CSI Ps ^` is SIMD in the pinned ECMA-48 (`CSI Ps 05/14`), not SD: the
/// caret form of SD is the 1991 printing's error, which the corrected text
/// gives to SIMD. baud has no implicit-movement directions, so it is
/// counted and the screen does not move (Jev's lean, SIMD_COUNTED,
/// 0.80/0.84).
#[test]
fn csi_caret_is_simd_and_counted_not_a_scroll() {
    let bytes = format!("{}\x1b[2^", fill());
    support::render(&WS, bytes.as_bytes(), |t| {
        assert_eq!(row(t, 1), "0123456789abcdefghij", "nothing scrolled");
        assert_eq!(t.unsupported, 1, "counted");
    });
}

/// `CSI ? Ps SP @` and the other prefixes are no SL.
#[test]
fn a_prefixed_shift_is_counted_and_moves_nothing() {
    let bytes = format!("{}\x1b[?2 @\x1b[>2 A", fill());
    support::render(&WS, bytes.as_bytes(), |t| {
        assert_eq!(row(t, 1), "0123456789abcdefghij");
        assert_eq!(t.unsupported, 2);
    });
}
