//! DECSLRM left/right margins, and the column operations that ride on them.
//!
//! * `[[source: vt510-decslrm]]`: "sets the left and right margins to define
//!   the scrolling region ... DECSLRM only works when vertical split screen
//!   mode (DECLRMM) is set ... The value of the left margin (Pl) must be less
//!   than the right margin (Pr) ... DECSLRM moves the cursor to column 1,
//!   line 1 of the page."
//! * `[[source: vt510-decic]]`: "As columns are inserted, the columns between
//!   the cursor and the right margin move to the right ... DECIC has no effect
//!   outside the scrolling margins."
//! * `[[source: vt510-decdc]]`: "the remaining columns between the cursor and
//!   the right margin move to the left. The terminal adds blank columns ... at
//!   the right margin."
//!
//! `CSI s` is **ambiguous**: SCOSC normally, DECSLRM when DECLRMM is set. That
//! ambiguity is in the protocol, so the parser is told which mode is in force
//! rather than guessing.

use super::origin::{find, seen};
use crate::support;
use baud_core::*;

fn run(bytes: &[u8]) -> (Vec<PackedCell>, (usize, usize), Vec<u8>) {
    let ws = support::CORPUS;
    let mut s = Owned::new(&ws);
    let mut t = Term::new(s.storage(), &ws);
    support::feed(&mut t, bytes);
    let (r, c) = t.cursor();
    (
        support::screen(&t),
        (r.index(), c.index()),
        t.reply().to_vec(),
    )
}

fn grid(b: &[u8]) -> Vec<PackedCell> {
    run(b).0
}
fn cursor(b: &[u8]) -> (usize, usize) {
    run(b).1
}
fn replies(b: &[u8]) -> Vec<u8> {
    run(b).2
}

/// Enable DECLRMM and set margins at columns 5..10 (1-based).
const LRM: &[u8] = b"\x1b[?69h\x1b[5;10s";

// ── the ambiguity ───────────────────────────────────────────────────────────

/// Without DECLRMM, `CSI s` is SCOSC and saves the cursor.
#[test]
fn csi_s_is_scosc_without_declrmm() {
    // Save at 3;3, move, restore: back to 3;3.
    assert_eq!(cursor(b"\x1b[3;3H\x1b[s\x1b[9;9H\x1b[u"), (2, 2));
}

/// With DECLRMM, the same bytes are DECSLRM and do **not** save the cursor.
#[test]
fn csi_s_is_decslrm_with_declrmm() {
    // `CSI 5;10 s` sets margins and homes the cursor, per the spec.
    assert_eq!(cursor(b"\x1b[?69h\x1b[9;9H\x1b[5;10s"), (0, 0));
}

/// Turning DECLRMM off restores full-width margins.
///
/// Leaving them set while the mode that gives them meaning is off would confine
/// writes with nothing to explain why.
#[test]
fn disabling_declrmm_restores_full_width() {
    let mut v = LRM.to_vec();
    v.extend_from_slice(b"\x1b[?69l");
    v.extend(std::iter::repeat_n(b'x', 20));
    // With margins gone, 20 characters occupy columns 0..20.
    assert_eq!(cursor(&v), (0, 20));
}

// ── setting them ────────────────────────────────────────────────────────────

/// DECSLRM homes the cursor. `[[source: vt510-decslrm]]`: "moves the cursor to
/// column 1, line 1 of the page."
#[test]
fn decslrm_homes_the_cursor() {
    assert_eq!(cursor(LRM), (0, 0));
}

/// An invalid range leaves the margins alone rather than clamping into
/// something the application did not ask for.
///
/// `[[source: vt510-decslrm]]`: "The value of the left margin (Pl) must be less
/// than the right margin (Pr)."
#[test]
fn an_inverted_range_is_rejected() {
    let mut v = b"\x1b[?69h\x1b[10;5s".to_vec();
    v.extend(std::iter::repeat_n(b'x', 20));
    assert_eq!(cursor(&v), (0, 20), "margins unchanged, so full width");
}

/// Equal margins are rejected too: the minimum region is two columns.
#[test]
fn a_zero_width_region_is_rejected() {
    let mut v = b"\x1b[?69h\x1b[5;5s".to_vec();
    v.extend(std::iter::repeat_n(b'x', 20));
    assert_eq!(cursor(&v), (0, 20));
}

// ── the defaults ────────────────────────────────────────────────────────────
//
// `[[source: dec-std-070]]` DECSLRM, p. 5-27: "default Pr: Last Column",
// and its algorithm, p. 5-28, reads an omitted parameter and a 0 alike:
// "if (parv[1] == 0) parv[1] = columns_per_page;". `[[source:
// vt510-decslrm]]`: "Pr is the column number for the right margin. Default:
// Pr = 80 or 132 (depending on the page width)". xterm (`CASE_ANSI_SC`,
// charproc.c 4891-4894, `[[source: xterm-reset]]`) and ghostty
// (`[[source: ghostty-d9840f3-terminal]]` 2610) read an omitted or 0 `Pr` as
// the page's last column. Each case runs at 20 by 8
// at every read size (`origin::seen`), and shows the margins by where `X`
// lands under DECOM after `CUP 1;99`, and by `CPR`, which counts from the
// left margin.

/// [`LRM`] as text, for the cases that build on 5..10.
const LRM_STR: &str = "\x1b[?69h\x1b[5;10s";
/// Under DECOM, `CUP 1;99` stops at the right margin; `X` and `CPR` say where.
const PROBE: &str = "\x1b[?6h\x1b[1;99HX\x1b[6n";

/// **A bare `CSI s` under DECLRMM is the page's margins**, and homes. Its
/// right margin decoded as column 1 until 2026-10-13, so `Pl` = `Pr` and the
/// sequence was rejected: parsed and dropped, the margins 5..10 kept.
#[test]
fn a_bare_decslrm_sets_the_margins_to_the_page_borders() {
    let s = seen(&format!("{LRM_STR}\x1b[3;3H\x1b[sY{PROBE}"));
    assert_eq!(find(&s, 'Y'), Some((0, 0)), "homed");
    assert_eq!(
        (find(&s, 'X'), s.reply.as_slice()),
        (Some((0, 19)), &b"\x1b[1;20R"[..]),
        "margins 1..20"
    );
}

/// `CSI 0;0 s` is the same: a 0 `Pr` is the default, as an omitted one is.
#[test]
fn a_zero_right_margin_is_the_page_border() {
    let s = seen(&format!("{LRM_STR}\x1b[3;3H\x1b[0;0sY{PROBE}"));
    assert_eq!(find(&s, 'Y'), Some((0, 0)), "homed");
    assert_eq!(
        (find(&s, 'X'), s.reply.as_slice()),
        (Some((0, 19)), &b"\x1b[1;20R"[..]),
        "margins 1..20"
    );
    let s = seen(&format!("{LRM_STR}\x1b[5;0s{PROBE}"));
    assert_eq!(
        (find(&s, 'X'), s.reply.as_slice()),
        (Some((0, 19)), &b"\x1b[1;16R"[..]),
        "margins 5..20"
    );
}

/// `CSI 5 s` sets 5..20, from the screen's own margins and from 5..10. It
/// was rejected as `Pl` > `Pr` = 1.
#[test]
fn an_omitted_right_margin_is_the_page_border() {
    let s = seen(&format!("\x1b[?69h\x1b[5s\x1b[?6h\x1b[1;1HZ{PROBE}"));
    assert_eq!(find(&s, 'Z'), Some((0, 4)), "the origin is column 5");
    assert_eq!(
        (find(&s, 'X'), s.reply.as_slice()),
        (Some((0, 19)), &b"\x1b[1;16R"[..]),
        "margins 5..20"
    );
    let s = seen(&format!("{LRM_STR}\x1b[5s{PROBE}"));
    assert_eq!(
        (find(&s, 'X'), s.reply.as_slice()),
        (Some((0, 19)), &b"\x1b[1;16R"[..]),
        "margins 5..20 after 5..10"
    );
}

/// The control: an omitted `Pl` is column 1 and an explicit `Pr` is kept,
/// which the decoding got right before.
#[test]
fn an_omitted_left_margin_is_column_one() {
    let s = seen(&format!("\x1b[?69h\x1b[;10s{PROBE}"));
    assert_eq!(
        (find(&s, 'X'), s.reply.as_slice()),
        (Some((0, 9)), &b"\x1b[1;10R"[..]),
        "margins 1..10"
    );
}

// ── what the margins confine ────────────────────────────────────────────────

/// Text wraps at the right margin and resumes at the **left** margin.
///
/// Wrapping to column 0 would leave the scrolling region entirely, which is the
/// whole thing side margins exist to prevent.
#[test]
fn text_wraps_within_the_margins() {
    let mut v = LRM.to_vec();
    v.extend_from_slice(b"\x1b[1;5H"); // to the left margin
    v.extend(std::iter::repeat_n(b'x', 7)); // 6 fit (cols 5..10), 1 wraps
    let (_, (r, c), _) = run(&v);
    assert_eq!(r, 1, "wrapped to the next row");
    assert_eq!(c, 5, "and to the left margin, not column 0");
}

/// **A cursor right of the right margin wraps before it writes.** `CUP` is
/// not confined by the side margins, so `CSI 1;30 H` with margins at 5..10
/// leaves the cursor at column 30, past the margin. The one-character
/// writer and the wrapping loop both treat "past the right margin" as a
/// wrap and take it first; the run writers asked how much room was left
/// before the margin and subtracted past zero -- a panic in a debug build,
/// and in release a run taken as fitting and written on from column 30 past
/// the margin: eight characters landed in columns 30-37, and a run of 180
/// kept 171 and dropped the rest at the screen's edge. All three writers
/// agree now: an ASCII run, a narrow non-ASCII run, and one character each.
/// Whether a terminal should instead write on to the screen's edge from
/// there is a question no pinned source answers; this pins the rule baud's
/// own writers already shared.
#[test]
fn a_run_right_of_the_right_margin_wraps_to_the_left_margin() {
    for (text, want, at) in [
        ("XXXXXXXX", "\x1b[2;5HXXXXXX\x1b[3;5HXX", (2, 6)),
        ("éééééééé", "\x1b[2;5Héééééé\x1b[3;5Héé", (2, 6)),
        ("X", "\x1b[2;5HX", (1, 5)),
        ("é", "\x1b[2;5Hé", (1, 5)),
    ] {
        let mut v = LRM.to_vec();
        v.extend_from_slice(b"\x1b[1;30H");
        v.extend_from_slice(text.as_bytes());
        let mut w = LRM.to_vec();
        w.extend_from_slice(want.as_bytes());
        let want_grid = grid(&w);
        assert_eq!(grid(&v), want_grid, "{text}: wrapped to the left margin");
        assert_eq!(cursor(&v), at, "{text}: where the cursor ends");
        for chunk in 1..v.len() {
            let ws = support::CORPUS;
            let mut s = Owned::new(&ws);
            let mut t = Term::new(s.storage(), &ws);
            let mut i = 0;
            while i < v.len() {
                let n = chunk.min(v.len() - i);
                t.write_at(BytePos::ZERO, &v[i..i + n]);
                t.feed(BytePos::ZERO, ReadLen::from_index(n));
                i += n;
            }
            assert_eq!(support::screen(&t), want_grid, "{text} at chunk {chunk}");
        }
    }
}

/// With autowrap off the cursor pins to the right margin, not the last column.
#[test]
fn no_wrap_pins_to_the_right_margin() {
    let mut v = LRM.to_vec();
    v.extend_from_slice(b"\x1b[?7l\x1b[1;5H");
    v.extend(std::iter::repeat_n(b'x', 50));
    assert_eq!(cursor(&v), (0, 9), "the right margin is column index 9");
}

/// ICH does not push text past the right margin.
#[test]
fn ich_stops_at_the_right_margin() {
    // Fill the row, set margins, then insert inside them.
    let mut base = b"".to_vec();
    base.extend(std::iter::repeat_n(b'a', 20));
    let mut v = base.clone();
    v.extend_from_slice(LRM);
    v.extend_from_slice(b"\x1b[1;5H\x1b[2@");
    let after = grid(&v);
    let before = grid(&base);
    // Columns *outside* the margins are untouched. The margins are 1-based
    // 5..10, so index 4 is the left margin and is inside -- it gets blanked by
    // the insert, which is the operation working, not a leak.
    for x in [0usize, 1, 2, 3, 10, 11, 19] {
        assert_eq!(after[x], before[x], "column {x} is outside the margins");
    }
    let blank = grid(b"")[0];
    assert_eq!(after[4], blank, "the left margin is inside, and was opened");
    assert_eq!(after[5], blank);
    assert_eq!(after[6], before[4], "old column 4 shifted right by two");
}

/// DCH does not pull text in from beyond the right margin.
#[test]
fn dch_does_not_pull_from_outside_the_margins() {
    let mut base = b"".to_vec();
    base.extend(std::iter::repeat_n(b'a', 20));
    let mut v = base.clone();
    v.extend_from_slice(LRM);
    v.extend_from_slice(b"\x1b[1;5H\x1b[2P");
    let after = grid(&v);
    let blank = grid(b"")[0];
    // The two columns at the right margin become blank, not copies of what is
    // beyond it.
    assert_eq!(after[8], blank, "column 8 blanked at the margin");
    assert_eq!(after[9], blank, "column 9 blanked at the margin");
    assert_eq!(after[10], grid(&base)[10], "column 10 untouched");
}

// ── DECIC / DECDC ───────────────────────────────────────────────────────────

/// DECIC inserts columns within the margins, on every row of the region.
#[test]
fn decic_inserts_columns_within_the_margins() {
    let mut v = b"".to_vec();
    v.extend(std::iter::repeat_n(b'a', 20));
    v.extend_from_slice(b"\r\n");
    v.extend(std::iter::repeat_n(b'b', 20));
    let base = v.clone();
    v.extend_from_slice(LRM);
    v.extend_from_slice(b"\x1b[1;5H\x1b['}");
    let after = grid(&v);
    let before = grid(&base);
    let blank = grid(b"")[0];
    assert_eq!(after[5 - 1 + 1 - 1], blank, "a blank column opened at 5");
    // Row 1 moved too: the region is the rectangle, not one line.
    let stride = support::with_corpus(|t| t.stride().index());
    assert_eq!(after[stride + 4], blank, "row 1 shifted as well");
    assert_eq!(after[0], before[0], "outside the margins, unchanged");
}

/// DECDC deletes columns and fills at the right margin.
#[test]
fn decdc_deletes_columns_and_fills_at_the_margin() {
    let mut v = b"".to_vec();
    v.extend(std::iter::repeat_n(b'a', 20));
    let base = v.clone();
    v.extend_from_slice(LRM);
    v.extend_from_slice(b"\x1b[1;5H\x1b['~");
    let after = grid(&v);
    let blank = grid(b"")[0];
    assert_eq!(after[9], blank, "filled at the right margin");
    assert_eq!(after[10], grid(&base)[10], "beyond it, untouched");
}

/// Both are no-ops with the cursor outside the margins.
///
/// `[[source: vt510-decic]]`: "DECIC has no effect outside the scrolling
/// margins."
#[test]
fn the_column_operations_are_inert_outside_the_margins() {
    let mut base = b"".to_vec();
    base.extend(std::iter::repeat_n(b'a', 20));
    let mut set = base.clone();
    set.extend_from_slice(LRM);
    let mut ic = set.clone();
    ic.extend_from_slice(b"\x1b[1;1H\x1b[5'}");
    let mut dc = set.clone();
    dc.extend_from_slice(b"\x1b[1;1H\x1b[5'~");
    assert_eq!(grid(&ic), grid(&set), "DECIC outside the margins");
    assert_eq!(grid(&dc), grid(&set), "DECDC outside the margins");
}

// ── the pending wrap, as the V10 oracle reads it ────────────────────────────

/// **`Term::wrap_pending` reads the sentinel, and so conflates what the
/// sentinel conflates.** A write that fills to the right margin sets it, at
/// full width and under the fence; a carriage return and `?7l` clear it; and a
/// `CUP` past the fence answers `Yes` too, because baud wraps first from there
/// (`a_run_right_of_the_right_margin_wraps_to_the_left_margin`), where ghostty
/// keeps a flag and answers `No` -- the corner note V's V10 leaves to step (d).
#[test]
fn wrap_pending_reads_the_sentinel() {
    let pending = |b: &[u8]| {
        let ws = support::CORPUS;
        let mut s = Owned::new(&ws);
        let mut t = Term::new(s.storage(), &ws);
        support::feed(&mut t, b);
        t.wrap_pending()
    };
    let full: Vec<u8> = std::iter::repeat_n(b'x', 200).collect();
    assert_eq!(pending(b""), WrapPending::No);
    assert_eq!(pending(&full), WrapPending::Yes, "a full row");
    let mut cr = full.clone();
    cr.push(b'\r');
    assert_eq!(pending(&cr), WrapPending::No, "a carriage return");
    let mut off = b"\x1b[?7l".to_vec();
    off.extend_from_slice(&full);
    assert_eq!(pending(&off), WrapPending::No, "?7l pins to the margin");
    let mut fenced = LRM.to_vec();
    fenced.extend_from_slice(b"\x1b[1;5Hxxxxxx");
    assert_eq!(
        pending(&fenced),
        WrapPending::Yes,
        "filled to the right margin"
    );
    let mut moved = LRM.to_vec();
    moved.extend_from_slice(b"\x1b[1;30H");
    assert_eq!(pending(&moved), WrapPending::Yes, "a CUP past the fence");
}

// ── DECRQSS ─────────────────────────────────────────────────────────────────

/// DECRQSS reports the margins in a form that can be sent straight back.
#[test]
fn decrqss_reports_the_side_margins() {
    let mut v = LRM.to_vec();
    v.extend_from_slice(b"\x1bP$qs\x1b\\");
    assert_eq!(replies(&v), b"\x1bP1$r5;10s\x1b\\");
}

/// And the scrolling region.
#[test]
fn decrqss_reports_the_scroll_region() {
    assert_eq!(replies(b"\x1b[3;20r\x1bP$qr\x1b\\"), b"\x1bP1$r3;20r\x1b\\");
}

/// RIS resets the margins.
#[test]
fn ris_resets_the_side_margins() {
    let mut v = LRM.to_vec();
    v.extend_from_slice(b"\x1bc");
    v.extend(std::iter::repeat_n(b'x', 20));
    assert_eq!(cursor(&v), (0, 20));
}

/// Margins behave identically however the reads are split.
#[test]
fn side_margins_are_chunk_invariant() {
    let mut seq = LRM.to_vec();
    seq.extend_from_slice(b"\x1b[1;5Habcdefghij\x1b['}");
    let whole = grid(&seq);
    for chunk in 1..seq.len() {
        let ws = support::CORPUS;
        let mut s = Owned::new(&ws);
        let mut t = Term::new(s.storage(), &ws);
        let mut i = 0;
        while i < seq.len() {
            let n = chunk.min(seq.len() - i);
            t.write_at(BytePos::ZERO, &seq[i..i + n]);
            t.feed(BytePos::ZERO, ReadLen::from_index(n));
            i += n;
        }
        assert_eq!(support::screen(&t), whole, "differed at chunk {chunk}");
    }
}
