//! **What SDS and SRS strings draw** (`[[source: ecma48]]` 8.3.114,
//! 8.3.137): with BDSM reset, TR/53's worked example 1 line for line
//! (`[[source: ecma-tr53]]`); with BDSM set, each string an isolate of its
//! direction. The core's side is `baud-core/tests/directed_strings.rs`.
//!
//! Every expectation is a visual order, `.` an erased cell.

use baud_core::{Owned, Term};
use baud_domain::{BytePos, PackedCell, ReadLen, Row, Rows, WinSize};
use baud_render::Reorder;

const WS: WinSize = WinSize {
    ws_row: 4,
    ws_col: 24,
};

fn feed(t: &mut Term<'_>, bytes: &str) {
    t.write_at(BytePos::ZERO, bytes.as_bytes());
    t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
}

fn row_of(t: &Term<'_>, o: &mut Reorder, r: usize) -> String {
    o.lay_out(t, Rows::ZERO);
    let vp = t.view(Rows::ZERO);
    let row = Row::from_index(r);
    let mut out = vec![PackedCell::EMPTY; vp.cols().index()];
    o.orders().permute(row, vp.row(row), &mut out);
    out.iter()
        .map(|c| match c.content {
            x if x.is_empty() => '.',
            x if x.is_wide_tail() => '~',
            x => x
                .as_codepoint()
                .and_then(|cp| char::from_u32(cp.value()))
                .unwrap_or('?'),
        })
        .collect()
}

/// Every row of the screen after `bytes`, from power-on, as drawn with any
/// mirrored character drawn as its mirror.
fn screen(bytes: &str) -> Vec<String> {
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, bytes);
    let mut o = Reorder::new();
    (0..usize::from(WS.ws_row))
        .map(|r| {
            let s = row_of(&t, &mut o, r);
            let row = Row::from_index(r);
            s.chars()
                .enumerate()
                .map(|(v, c)| {
                    o.orders()
                        .swapped(row, baud_domain::Col::at_index(v))
                        .and_then(|m| m.as_codepoint())
                        .and_then(|cp| char::from_u32(cp.value()))
                        .unwrap_or(c)
                })
                .collect()
        })
        .collect()
}

/// **TR/53's worked example 1, left to right** (BDSM EXPLICIT, SPD 0):
/// `a b c d _ SRS(1) e f g h SRS(0) _ i j` presented "abcd_hgfe_ij", the
/// nested `k l _ SRS(1) m n o p _ SRS(1) q r s t SRS(0) _ u v w SRS(0) _ x y
/// z` presented "kl_wvu_qrst_ponm_xyz", and `( a b c SRS(1) { d e f ]
/// SRS(0) g h i }` with its reversed brackets mirrored, as SAPV 3 asks
/// there. Until now SRS was counted and every line drawn as written.
#[test]
fn tr53_example_1_left_to_right() {
    let s = screen(
        "\x1b[8labcd \x1b[1[efgh\x1b[0[ ij\r\n\
         kl \x1b[1[mnop \x1b[1[qrst\x1b[0[ uvw\x1b[0[ xyz\r\n\
         (abc\x1b[1[{def]\x1b[0[ghi}",
    );
    assert_eq!(s[0], "abcd hgfe ij............");
    assert_eq!(s[1], "kl wvu qrst ponm xyz....");
    assert_eq!(s[2], "(abc[fed}ghi}...........");
}

/// **TR/53's worked example 1, right to left** (after `SCP(2;1)`): `a b c d
/// _ SDS(1) e f g h SDS(0) _ i j` presented "ji_efgh_dcba", and `k l _
/// SDS(1) m n o p _ SDS(2) q r s t SDS(0) _ u v w SDS(0) _ x y z` presented
/// "zyx_mnop_tsrq_uvw_lk", each against the right edge.
#[test]
fn tr53_example_1_right_to_left() {
    let s = screen(
        "\x1b[8l\x1b[2;1 kabcd \x1b[1]efgh\x1b[0] ij\r\n\
         kl \x1b[1]mnop \x1b[2]qrst\x1b[0] uvw\x1b[0] xyz",
    );
    assert_eq!(s[0], "............ji efgh dcba");
    assert_eq!(s[1], "....zyx mnop tsrq uvw lk");
}

/// **A wide character in a reversed string stays whole**: its right half
/// stays on its right.
#[test]
fn a_wide_character_in_a_reversed_string_stays_whole() {
    let s = screen("\x1b[8la\x1b[1[b\u{65e5}c\x1b[0[d");
    assert_eq!(s[0], "ac\u{65e5}~bd..................");
}

/// **With BDSM set each string is an isolate of its direction**: two
/// adjacent right-to-left strings of Hebrew on an English line are drawn
/// apart, each reversed in its place; Latin letters and digits in a
/// right-to-left string keep their order, UAX #9 W7 making the digits L,
/// while Hebrew and digits are ordered right to left with the number
/// kept left to right; and a reversed string of Latin letters at the top
/// of a line with no path set, right to left by the path's left to
/// right, keeps its letters' order.
#[test]
fn with_bdsm_set_each_string_is_an_isolate() {
    let s = screen(
        "go \x1b[2]\u{5D0}\u{5D1}\x1b[0]\x1b[2]\u{5D2}\u{5D3}\x1b[0] now\r\n\
         \x1b[2]ab 12\x1b[0] \x1b[2]\u{5D0}\u{5D1} 12\x1b[0]\r\n\
         x \x1b[1[abc\x1b[0[ y",
    );
    assert_eq!(s[0], "go \u{5D1}\u{5D0}\u{5D3}\u{5D2} now.............");
    assert_eq!(s[1], "ab 12 12 \u{5D1}\u{5D0}.............");
    assert_eq!(s[2], "x abc y.................");
}

/// **The control: the same Hebrew without strings** is one run, reversed
/// whole.
#[test]
fn without_strings_the_hebrew_is_one_run() {
    let s = screen("go \u{5D0}\u{5D1}\u{5D2}\u{5D3} now");
    assert_eq!(s[0], "go \u{5D3}\u{5D2}\u{5D1}\u{5D0} now.............");
}

/// **A row DECRLM wrote is drawn as its cells hold**: "shalom abc" sent
/// right to left from the last column reads on the screen as the VT510
/// shows it, though the mode is on; the control, the same cells written
/// left to right, is reordered by UAX #9.
#[test]
fn a_row_decrlm_wrote_is_drawn_as_its_cells_hold() {
    let s = screen("\x1b[?34h\x1b[1;24H\u{5E9}\u{5DC}\u{5D5}\u{5DD} abc");
    assert_eq!(s[0], "................cba \u{5DD}\u{5D5}\u{5DC}\u{5E9}");
    let s = screen("\x1b[1;17Hcba \u{5DD}\u{5D5}\u{5DC}\u{5E9}");
    assert_eq!(s[0], "................cba \u{5E9}\u{5DC}\u{5D5}\u{5DD}");
}

/// **A row DECRLM wrote, erased whole, is drawn by UAX #9 again** (as its
/// cells hold "until it is erased whole"): after `ED 2` and after `EL 2`,
/// Hebrew written in order on a row DECRLM once wrote is drawn as on a row
/// it never wrote.
#[test]
fn a_row_decrlm_wrote_erased_whole_is_drawn_by_uax9_again() {
    let line = "\u{5D0}\u{5D1} ab";
    let probe = "\x1b[?34h\x1b[1;20H\u{5D2}\x1b[?34l";
    for erase in ["\x1b[2J\x1b[H", "\x1b[H\x1b[2K"] {
        let control = screen(&format!("{erase}{line}"));
        assert_eq!(
            screen(&format!("{probe}{erase}{line}"))[0],
            control[0],
            "{erase:?}"
        );
    }
}

/// **A layout kept across frames finds each row's strings as a fresh one
/// does**: the layout keeps, for each row, whether it holds a string while
/// its cells are the same, so a frame reads the renditions of the rows
/// that changed and no other. Across a string begun on another row, a row
/// gaining one, losing it, written over in a string, more of a string
/// still open written in the rendition already made for it, and a reset,
/// with BDSM set and reset, each row of the kept layout is drawn as a new
/// layout draws it.
#[test]
fn a_kept_layout_finds_each_rows_strings_as_a_fresh_one_does() {
    // A letter and a mark in a right-to-left string are drawn in its
    // direction, with BDSM set as with it reset.
    let steps = [
        "\x1b[4;20H\x1b[1]\x1b[0]",
        "\x1b[1;1Habc xy z",
        "\x1b[2;1Habc \x1b[2]x !\x1b[0] z",
        "\x1b[2;1H\x1b[2Kabc x y z",
        "\x1b[1;5H\x1b[2]x y\x1b[0]",
        "\x1b[3;1H\x1b[1[q r\x1b[0[ t",
        "\x1b[2;1H\x1b[2]mn",
        "\x1b[4;19Ho !\x1b[0]",
        "\x1bc\x1b[1;1Hab\x1b[2]c d\x1b[0]",
        "\x1b[1;1H\x1b[2Kabcd",
    ];
    for bdsm in ["", "\x1b[8l"] {
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        feed(&mut t, bdsm);
        let mut kept = Reorder::new();
        for step in steps {
            feed(&mut t, step);
            if step.starts_with("\x1bc") {
                feed(&mut t, bdsm);
            }
            for r in 0..usize::from(WS.ws_row) {
                let mut fresh = Reorder::new();
                assert_eq!(
                    row_of(&t, &mut kept, r),
                    row_of(&t, &mut fresh, r),
                    "{bdsm:?}, {step:?}, row {r}"
                );
            }
            let mut drawn = |r| row_of(&t, &mut kept, r);
            match step {
                "\x1b[2;1Habc \x1b[2]x !\x1b[0] z" => {
                    assert_eq!(drawn(1), "abc ! x z...............", "{bdsm:?}")
                }
                "\x1b[4;19Ho !\x1b[0]" => {
                    assert_eq!(drawn(3), "..................! o...", "{bdsm:?}")
                }
                _ => {}
            }
        }
        assert_eq!(
            row_of(&t, &mut kept, 0),
            "abcd....................",
            "{bdsm:?}"
        );
    }
}
