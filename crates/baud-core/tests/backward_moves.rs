//! HPB and VPB, ECMA-48's backward twins of HPR and VPR.
//!
//! `[[source: ecma48]]` 8.3.58, HPB, `CSI Pn 06/10`: "HPB causes the active
//! data position to be moved by n character positions in the data component
//! in the direction opposite to that of the character progression, where n
//! equals the value of Pn", default 1; 8.3.159, VPB, `CSI Pn 06/11`, "by n
//! line positions ... in a direction opposite to that of the line
//! progression". No DEC text and no ctlseqs entry has either, so each
//! mirrors its forward twin here: HPR goes to "the last position on the
//! line" whatever the side margins and VPR to the page's last line whatever
//! the scrolling region, each confined under DECOM. Every test here fails on
//! a `CSI j` or `CSI k` that does nothing, which is what both did: counted
//! and dropped.
mod support;
use baud_core::{Owned, Term, WinSize, WrapPending};
use baud_domain::{Col, Row};

const WS: WinSize = WinSize {
    ws_row: 8,
    ws_col: 20,
};
/// Top and bottom margins at rows 3..6 (1-based); 0-based 2..=5.
const TB: &str = "\x1b[3;6r";
/// `?69h`, then side margins at columns 5..10 (1-based); 0-based 4..=9.
const LRM: &str = "\x1b[?69h\x1b[5;10s";

/// The cursor after `bytes`, fed at read sizes 1, 3 and whole, each of
/// which must agree, and the parser's count of what it dropped.
fn after(bytes: &str) -> ((usize, usize), usize) {
    let mut seen = None;
    for chunk in [1, 3, bytes.len().max(1)] {
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        for piece in bytes.as_bytes().chunks(chunk) {
            support::feed(&mut t, piece);
        }
        let (r, c) = t.cursor();
        let got = ((r.index(), c.index()), t.unsupported);
        assert_eq!(*seen.get_or_insert(got), got, "read size {chunk}");
    }
    seen.expect("one read size")
}

fn at(bytes: &str) -> (usize, usize) {
    let (cursor, unsupported) = after(bytes);
    assert_eq!(unsupported, 0, "nothing dropped in {bytes:?}");
    cursor
}

/// **HPB moves left `n` columns, the row kept; bare and 0 mean 1.**
#[test]
fn hpb_moves_back_n_columns() {
    assert_eq!(at("\x1b[3;10H\x1b[4j"), (2, 5));
    assert_eq!(at("\x1b[3;10H\x1b[j"), (2, 8));
    assert_eq!(at("\x1b[3;10H\x1b[0j"), (2, 8));
    assert_eq!(at("\x1b[3;10H\x1b[30j"), (2, 0), "stops at column 1");
}

/// **VPB moves up `n` lines, the column kept; bare and 0 mean 1.**
#[test]
fn vpb_moves_up_n_lines() {
    assert_eq!(at("\x1b[7;9H\x1b[4k"), (2, 8));
    assert_eq!(at("\x1b[7;9H\x1b[k"), (5, 8));
    assert_eq!(at("\x1b[7;9H\x1b[0k"), (5, 8));
    assert_eq!(at("\x1b[7;9H\x1b[30k"), (0, 8), "stops at line 1");
}

/// **HPB passes the left margin, as HPR passes the right one**, to the
/// line's first position; the control is HPR from the mirror place.
#[test]
fn hpb_passes_the_left_margin_as_hpr_passes_the_right() {
    assert_eq!(at(&format!("{LRM}\x1b[3;7H\x1b[5j")), (2, 1));
    assert_eq!(at(&format!("{LRM}\x1b[3;7H\x1b[30j")), (2, 0));
    assert_eq!(at(&format!("{LRM}\x1b[3;8H\x1b[5a")), (2, 12));
}

/// **VPB passes the top margin, as VPR passes the bottom one.**
#[test]
fn vpb_passes_the_top_margin_as_vpr_passes_the_bottom() {
    assert_eq!(at(&format!("{TB}\x1b[5;4H\x1b[3k")), (1, 3));
    assert_eq!(at(&format!("{TB}\x1b[4;4H\x1b[3e")), (6, 3));
}

/// **Under DECOM both stop at the margins**, as every move is confined:
/// HPB at the left margin, VPB at the top margin.
#[test]
fn under_decom_hpb_and_vpb_stop_at_the_margins() {
    assert_eq!(at(&format!("{LRM}\x1b[?6h\x1b[1;3H\x1b[30j")), (0, 4));
    assert_eq!(at(&format!("{TB}\x1b[?6h\x1b[3;3H\x1b[30k")), (2, 2));
}

/// **From a pending wrap HPB counts from the column the cursor stands
/// on**, the last, as the text's "active data position" is, and the wrap
/// is gone; so does `CUB` (`[[source: dec-std-070]]` CUB, p. 5-47, and
/// D.6.1), as xterm's `CursorBack` does without `?45` or `?1045`
/// (`[[source: xterm-cursor-c]]` 152-158). **VPB leaves the wrap as VPR
/// leaves it**, and `CUU` ends it (D.6.1), the controls.
#[test]
fn from_a_pending_wrap_hpb_counts_from_the_last_column() {
    let full = "x".repeat(20);
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    let mut probe = |bytes: String| {
        support::feed(&mut t, bytes.as_bytes());
        let (r, c) = t.cursor();
        ((r.index(), c.index()), t.wrap_pending())
    };
    let row3 = |tail: &str| format!("\x1b[3;1H{full}{tail}");
    assert_eq!(probe(row3("\x1b[2j")), ((2, 17), WrapPending::No));
    assert_eq!(probe(row3("\x1b[2D")), ((2, 17), WrapPending::No));
    let vpb = probe(row3("\x1b[k"));
    assert_eq!(vpb.0, (1, 19));
    assert_eq!(vpb.1, WrapPending::Yes, "VPB keeps it");
    assert_eq!(
        probe(row3("\x1b[A")),
        ((1, 19), WrapPending::No),
        "CUU ends it"
    );
    let vpr = probe(row3("\x1b[e"));
    assert_eq!(vpb.1, vpr.1, "VPB as VPR");
}

/// **HPB does not wrap back past the line's start**, `?45` and `?1045`
/// set or not: those are BS's and CUB's, and HPB is neither.
#[test]
fn hpb_does_not_reverse_wrap() {
    assert_eq!(at("\x1b[?7h\x1b[?45h\x1b[?1045h\x1b[4;3H\x1b[5j"), (3, 0));
}

/// **What HPB leaves is where the next character lands**: written at
/// column 6 after `CSI 4 j` from column 10.
#[test]
fn a_character_after_hpb_lands_at_its_column() {
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    support::feed(&mut t, b"\x1b[2;10H\x1b[4jZ\x1b[2kY");
    let cell = |y: usize, x: usize| {
        t.row(Row::from_index(y)).expect("a row")[Col::at_index(x).index()]
            .content
            .as_codepoint()
            .map(|c| c.value())
    };
    assert_eq!(cell(1, 5), Some(u32::from('Z')));
    assert_eq!(cell(0, 6), Some(u32::from('Y')));
}
