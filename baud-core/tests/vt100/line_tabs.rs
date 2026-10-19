//! Line tabulation stops, and the forms of TBC and CTC that clear and set
//! both kinds: VTS (`ESC J`), CVT (`CSI Pn Y`), VT, CTC (`CSI Ps... W`),
//! TBC 1/2/4/5 and DECST8C (`CSI ? 5 W`).
//!
//! `[[source: ecma48]]` 8.3.162: "VTS causes a line tabulation stop to be set
//! at the active line"; CVT "causes the active presentation position to be
//! moved to the corresponding character position of the line corresponding
//! to the n-th following line tabulation stop"; 8.3.161, VT moves it "to the
//! corresponding character position on the line at which the following line
//! tabulation stop is set". TBC (8.3.154) and CTC clear and set both kinds.
//! DEC's terminals and xterm keep TBC to 0 and 3; Jev decided ECMA-48's
//! stops whole on 2026-10-19 (BUILD_ECMA 0.98/0.94), VT reaching them
//! (NEXT_LINE_STOP 0.94/0.92, LF where none follows), and CVT going to the
//! bottom margin, or the last line, when none follows (BOTTOM 0.96/0.90).
//! baud's character stops are the same in every line, TSM MULTIPLE, so TBC 2
//! and CTC 4 clear every character stop and DECRQM answers mode 18
//! permanently reset (PERMANENTLY_RESET 0.98/0.98).

use crate::support;
use baud_core::*;

fn at(bytes: &[u8]) -> (usize, usize) {
    support::render(&support::CORPUS, bytes, |t| {
        let (r, c) = t.cursor();
        (r.index(), c.index())
    })
}

fn counted(bytes: &[u8]) -> (usize, usize) {
    support::render(&support::CORPUS, bytes, |t| {
        (t.unsupported, t.unsupported_param)
    })
}

// ── VTS and CVT ─────────────────────────────────────────────────────────────

#[test]
fn cvt_moves_to_the_line_vts_set_and_keeps_the_column() {
    assert_eq!(at(b"\x1b[10;1H\x1bJ\x1b[1;7H\x1b[Y"), (9, 6));
    assert_eq!(counted(b"\x1b[10;1H\x1bJ\x1b[1;7H\x1b[Y"), (0, 0));
}

#[test]
fn cvt_counts_its_stops() {
    let set = b"\x1b[5;1H\x1bJ\x1b[9;1H\x1bJ\x1b[20;1H\x1bJ\x1b[1;3H";
    assert_eq!(at(&[&set[..], b"\x1b[2Y"].concat()), (8, 2));
    assert_eq!(at(&[&set[..], b"\x1b[3Y"].concat()), (19, 2));
    assert_eq!(
        at(&[&set[..], b"\x1b[Y\x1b[Y"].concat()),
        (8, 2),
        "a stop on the cursor's own line is not a following one"
    );
}

/// With no stop ahead, the bottom margin inside the scrolling region and the
/// last line outside it, as HT stops at the right margin; never a scroll.
#[test]
fn cvt_past_the_last_stop_goes_to_the_bottom_margin() {
    assert_eq!(at(b"\x1b[3;4H\x1b[Y"), (49, 3), "no stops: the last line");
    assert_eq!(
        at(b"\x1b[5;20r\x1b[7;4H\x1b[Y"),
        (19, 3),
        "the bottom margin"
    );
    assert_eq!(
        at(b"\x1b[5;20r\x1b[30;1H\x1bJ\x1b[7;4H\x1b[Y"),
        (19, 3),
        "a stop below the region is not reached from inside it"
    );
    assert_eq!(
        at(b"\x1b[5;20r\x1b[30;1H\x1bJ\x1b[22;4H\x1b[Y"),
        (29, 3),
        "from below the region, the stop"
    );
    support::render(&support::CORPUS, b"x\x1b[50;1H\x1b[Y", |t| {
        assert_eq!(t.cursor().0, Row::from_index(49));
        let first = t.row(Row::ZERO).expect("a row")[0];
        assert_eq!(
            first.content.as_codepoint().map(|c| c.value()),
            Some(0x78),
            "no scroll"
        );
    });
}

/// `84`-`9F` under ISO 8859-1 are their 7-bit forms: `8A` is VTS.
#[test]
fn eight_bit_vts_under_latin1_sets_a_line_stop() {
    assert_eq!(at(b"\x1b%@\x1b[12;1H\x8a\x1b[1;1H\x1b[Y"), (11, 0));
}

/// VT52's `ESC J` erases to the end of the screen and sets no stop.
#[test]
fn vt52_esc_j_still_erases() {
    support::render(&support::CORPUS, b"abc\x1b[?2l\x1bH\x1bJ\x1b<\x1b[Y", |t| {
        let first = t.row(Row::ZERO).expect("a row")[0];
        assert_eq!(first.content.as_codepoint(), None, "erased");
        assert_eq!(t.cursor().0, Row::from_index(49), "and no stop was set");
    });
}

// ── VT ──────────────────────────────────────────────────────────────────────

#[test]
fn vt_moves_to_the_following_line_stop() {
    assert_eq!(at(b"\x1b[8;1H\x1bJ\x1b[2;5H\x0b"), (7, 4));
    assert_eq!(
        at(b"\x1b[20h\x1b[8;1H\x1bJ\x1b[2;5H\x0b"),
        (7, 4),
        "the corresponding character position, LNM or not"
    );
}

/// With no stop following, VT is LF, as DEC's table has it.
#[test]
fn vt_with_no_stop_following_is_a_line_feed() {
    assert_eq!(at(b"\x1b[2;5H\x0b"), (2, 4));
    assert_eq!(
        at(b"\x1b[20h\x1b[2;5H\x0b"),
        (2, 0),
        "LNM returns as LF does"
    );
    assert_eq!(
        at(b"\x1b[8;1H\x1bJ\x1b[9;5H\x0b"),
        (9, 4),
        "a stop above is not a following one"
    );
    support::render(&support::CORPUS, b"x\x1b[50;1H\x0b", |t| {
        let first = t.row(Row::ZERO).expect("a row")[0];
        assert_eq!(first.content.as_codepoint(), None, "scrolled, as LF does");
    });
}

/// Inside the region, a stop below the bottom margin is not reached: LF
/// scrolls the region, as it would.
#[test]
fn vt_inside_the_region_reads_stops_to_the_bottom_margin() {
    assert_eq!(at(b"\x1b[5;10r\x1b[12;1H\x1bJ\x1b[10;3H\x0b"), (9, 2));
    assert_eq!(at(b"\x1b[5;10r\x1b[8;1H\x1bJ\x1b[6;3H\x0b"), (7, 2));
}

/// A VT split from what follows in another read lands the same.
#[test]
fn vt_lands_the_same_at_every_read_split() {
    let bytes = b"\x1b[8;1H\x1bJ\x1b[2;5H\x0bZ";
    let whole = support::render(&support::CORPUS, bytes, |t| support::screen(t));
    for cut in 1..bytes.len() {
        let mut s = Owned::new(&support::CORPUS);
        let mut t = Term::new(s.storage(), &support::CORPUS);
        support::feed(&mut t, &bytes[..cut]);
        support::feed(&mut t, &bytes[cut..]);
        assert_eq!(support::screen(&t), whole, "cut at {cut}");
    }
}

// ── TBC ─────────────────────────────────────────────────────────────────────

#[test]
fn tbc_1_clears_the_line_stop_at_the_cursor_and_nothing_else() {
    let set = b"\x1b[5;1H\x1bJ\x1b[9;1H\x1bJ";
    assert_eq!(
        at(&[&set[..], b"\x1b[5;1H\x1b[1g\x1b[1;1H\x1b[Y"].concat()),
        (8, 0)
    );
    assert_eq!(
        at(&[&set[..], b"\x1b[5;1H\x1b[1g\x1b[1;1H\t"].concat()),
        (0, 8),
        "the character stops stay"
    );
}

#[test]
fn tbc_4_clears_every_line_stop_and_no_character_stop() {
    let set = b"\x1b[5;1H\x1bJ\x1b[9;1H\x1bJ\x1b[4g";
    assert_eq!(at(&[&set[..], b"\x1b[1;1H\x1b[Y"].concat()), (49, 0));
    assert_eq!(at(&[&set[..], b"\x1b[1;1H\t"].concat()), (0, 8));
}

/// Under TSM MULTIPLE a character stop is in every line, so clearing those
/// of the active line clears them all.
#[test]
fn tbc_2_clears_every_character_stop() {
    assert_eq!(at(b"\x1b[2g\x1b[1;1H\t"), (0, 199));
    assert_eq!(
        at(b"\x1b[5;1H\x1bJ\x1b[2g\x1b[1;1H\x1b[Y"),
        (4, 0),
        "line stops stay"
    );
}

#[test]
fn tbc_5_clears_both_kinds() {
    assert_eq!(at(b"\x1b[5;1H\x1bJ\x1b[5g\x1b[1;1H\x1b[Y"), (49, 0));
    assert_eq!(at(b"\x1b[5g\x1b[1;1H\t"), (0, 199));
}

#[test]
fn tbc_with_a_value_ecma48_does_not_define_is_counted() {
    assert_eq!(counted(b"\x1b[6g"), (0, 1));
    assert_eq!(at(b"\x1b[6g\x1b[1;1H\t"), (0, 8), "nothing cleared");
}

// ── CTC ─────────────────────────────────────────────────────────────────────

#[test]
fn ctc_sets_and_clears_both_kinds() {
    // 0: a character stop at the cursor; 2: clears it.
    assert_eq!(at(b"\x1b[3g\x1b[1;5H\x1b[0W\x1b[1;1H\t"), (0, 4));
    assert_eq!(at(b"\x1b[3g\x1b[1;5H\x1b[W\x1b[2W\x1b[1;1H\t"), (0, 199));
    // 1: a line stop at the active line; 3: clears it; 6: clears all.
    assert_eq!(at(b"\x1b[7;1H\x1b[1W\x1b[1;1H\x1b[Y"), (6, 0));
    assert_eq!(at(b"\x1b[7;1H\x1b[1W\x1b[3W\x1b[1;1H\x1b[Y"), (49, 0));
    assert_eq!(
        at(b"\x1b[7;1H\x1b[1W\x1b[9;1H\x1b[1W\x1b[6W\x1b[1;1H\x1b[Y"),
        (49, 0)
    );
    // 4 and 5: every character stop.
    assert_eq!(at(b"\x1b[4W\x1b[1;1H\t"), (0, 199));
    assert_eq!(at(b"\x1b[5W\x1b[1;1H\t"), (0, 199));
}

/// "Notation: (Ps...)": every parameter, in order.
#[test]
fn ctc_walks_every_parameter() {
    support::render(
        &support::CORPUS,
        b"\x1b[3g\x1b[4;5H\x1b[0;1W\x1b[1;1H\x1b[Y",
        |t| {
            assert_eq!(t.cursor(), (Row::from_index(3), Col::ZERO), "the line stop");
        },
    );
    support::render(
        &support::CORPUS,
        b"\x1b[3g\x1b[4;5H\x1b[0;1W\x1b[1;1H\t",
        |t| {
            assert_eq!(t.cursor().1, Col::at_index(4), "and the character stop");
        },
    );
    assert_eq!(
        counted(b"\x1b[0;7;1W"),
        (0, 1),
        "7 counted, the rest acted on"
    );
}

// ── DECST8C ─────────────────────────────────────────────────────────────────

#[test]
fn decst8c_sets_a_stop_every_eight_columns() {
    assert_eq!(at(b"\x1b[3g\x1b[1;5H\x1bH\x1b[?5W\x1b[1;1H\t"), (0, 8));
    assert_eq!(at(b"\x1b[3g\x1b[?5W\x1b[1;1H\t\t\t"), (0, 24));
    assert_eq!(counted(b"\x1b[?5W"), (0, 0));
}

/// VT510 and ctlseqs give one form, `CSI ? 5 W` (Jev's lean, FIVE_ONLY,
/// 0.74/0.90).
#[test]
fn decst8c_takes_only_five() {
    assert_eq!(at(b"\x1b[3g\x1b[?W\x1b[1;1H\t"), (0, 199));
    assert_eq!(at(b"\x1b[3g\x1b[?0W\x1b[1;1H\t"), (0, 199));
    assert_eq!(counted(b"\x1b[?W\x1b[?4W"), (2, 0));
}

// ── resets and reports ──────────────────────────────────────────────────────

#[test]
fn ris_clears_the_line_stops_and_decstr_keeps_them() {
    assert_eq!(at(b"\x1b[5;1H\x1bJ\x1bc\x1b[1;1H\x1b[Y"), (49, 0));
    assert_eq!(at(b"\x1b[5;1H\x1bJ\x1b[!p\x1b[1;1H\x1b[Y"), (4, 0));
}

#[test]
fn tsm_is_reported_permanently_reset_and_cannot_be_set() {
    support::render(&support::CORPUS, b"\x1b[18$p\x1b[18h\x1b[18$p", |t| {
        assert_eq!(t.reply(), b"\x1b[18;4$y\x1b[18;4$y");
        assert_eq!(t.unsupported_param, 1, "SM 18 counted");
    });
}
