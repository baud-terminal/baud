//! DECSET / DECRST: the modes baud actually acts on.
//!
//! DECSET/DECRST were `parsed` — recognised and dropped. Only modes baud acts
//! on get a bit, so mouse reporting, bracketed paste and the alternate screen
//! are deliberately still ignored rather than recorded: a bit that is set and
//! never read is the same "accepted, did nothing" failure.
//!
//! **A source conflict, recorded rather than smoothed over.**
//! `[[source: vt510-decawm]]` says of DECAWM: "Default: No autowrap".
//! `[[source: ctlseqs]]` documents xterm, where autowrap is on by default, and
//! every modern terminal wraps. baud follows xterm: an application that emits a
//! long line expects it to wrap, and defaulting to the VT510 behaviour would
//! break essentially all of them. The DEC default is the older and stricter
//! reading, and it is the one baud does not implement.

use baud_core::*;

fn term(bytes: &[u8]) -> (usize, usize) {
    let mut s = Storage::boxed();
    let mut t = Term::new(&mut s, OpMode::Computed);
    let mut i = 0;
    while i < bytes.len() {
        let n = READ_CHUNK.min(bytes.len() - i);
        let mut slot = t.read_slot();
        let base = slot.base();
        let filled = slot.write(StreamByte::slice_from(&bytes[i..i + n]));
        t.feed(base, filled.len());
        t.commit_read(filled.len());
        i += n;
    }
    let (r, c) = t.cursor();
    (r.index(), c.index())
}

fn visible(bytes: &[u8]) -> bool {
    let mut s = Storage::boxed();
    let mut t = Term::new(&mut s, OpMode::Computed);
    let mut slot = t.read_slot();
    let base = slot.base();
    let f = slot.write(StreamByte::slice_from(bytes));
    t.feed(base, f.len());
    t.commit_read(f.len());
    t.cursor_visible()
}

// ── DECTCEM ─────────────────────────────────────────────────────────────────

/// The cursor is visible on a fresh terminal, and `?25` toggles it.
///
/// baud has no renderer, but an embedder needs the flag; dropping it would
/// leave one no way to honour `?25 l`.
#[test]
fn dectcem_toggles_and_defaults_visible() {
    assert!(visible(b""), "a fresh terminal shows its cursor");
    assert!(!visible(b"\x1b[?25l"));
    assert!(visible(b"\x1b[?25l\x1b[?25h"));
}

// ── DECAWM ──────────────────────────────────────────────────────────────────

/// Autowrap is on by default: text past the right margin continues on the next
/// line. Following xterm, not the VT510 manual — see the module note.
#[test]
fn autowrap_is_on_by_default() {
    // 200 columns; 205 characters must land on row 1.
    let long: Vec<u8> = std::iter::repeat(b'x').take(205).collect();
    assert_eq!(term(&long), (1, 5));
}

/// With DECAWM reset the cursor sticks at the last column and characters
/// overwrite it. No wrap means no scroll.
#[test]
fn decawm_reset_stops_at_the_right_margin() {
    let mut v = b"\x1b[?7l".to_vec();
    v.extend(std::iter::repeat(b'x').take(205));
    assert_eq!(term(&v), (0, 199), "must stay on row 0, at the last column");
}

/// Re-setting it restores wrapping.
#[test]
fn decawm_can_be_set_again() {
    let mut v = b"\x1b[?7l\x1b[?7h".to_vec();
    v.extend(std::iter::repeat(b'x').take(205));
    assert_eq!(term(&v), (1, 5));
}

/// A no-wrap line must not scroll the screen either — the danger is that
/// "stop at the margin" is implemented as "wrap without moving", which still
/// scrolls at the bottom row.
#[test]
fn decawm_reset_does_not_scroll_at_the_last_row() {
    let mut v = b"\x1b[?7l\x1b[50;1H".to_vec();
    v.extend(std::iter::repeat(b'x').take(400));
    assert_eq!(term(&v), (49, 199), "cursor pinned to the last cell");
}

// ── DECOM ───────────────────────────────────────────────────────────────────

/// With DECOM set, row addressing is relative to the top margin.
///
/// `[[source: vt510-decom]]`: "The starting point for line numbers depends on
/// the current top margin setting."
#[test]
fn decom_makes_row_addressing_relative_to_the_margin() {
    // Region rows 4..10 (1-based). `CSI 1 ; 1 H` under DECOM is the region top.
    assert_eq!(term(b"\x1b[5;10r\x1b[?6h\x1b[1;1H"), (4, 0));
    assert_eq!(
        term(b"\x1b[5;10r\x1b[?6h\x1b[3;1H"),
        (6, 0),
        "third line of the region"
    );
    // Without DECOM the same CUP is absolute.
    assert_eq!(term(b"\x1b[5;10r\x1b[?6l\x1b[1;1H"), (0, 0));
}

/// VPA is relative too, since it is row addressing by another name.
#[test]
fn decom_applies_to_vpa() {
    assert_eq!(term(b"\x1b[5;10r\x1b[?6h\x1b[2d"), (5, 0));
}

/// The cursor cannot move outside the margins while DECOM is set.
///
/// `[[source: vt510-decom]]`: "The cursor cannot move outside of the margins."
#[test]
fn decom_confines_the_cursor_to_the_margins() {
    // Above the region.
    assert_eq!(term(b"\x1b[5;10r\x1b[?6h\x1b[1;1H\x1b[9A").0, 4);
    // Below it: an absolute address past the bottom clamps to the bottom.
    assert_eq!(term(b"\x1b[5;10r\x1b[?6h\x1b[40;1H").0, 9);
}

/// Setting or resetting DECOM homes the cursor, and home depends on the mode
/// just applied.
#[test]
fn decom_homes_the_cursor_on_change() {
    assert_eq!(
        term(b"\x1b[5;10r\x1b[20;5H\x1b[?6h"),
        (4, 0),
        "home is the margin"
    );
    assert_eq!(
        term(b"\x1b[5;10r\x1b[20;5H\x1b[?6l"),
        (0, 0),
        "home is the screen"
    );
}

/// A mode baud does not implement is ignored, not recorded and not acted on.
#[test]
fn unimplemented_private_modes_are_inert() {
    // Bracketed paste, alt screen, mouse: parsed, deliberately no effect here.
    assert_eq!(term(b"\x1b[?2004h\x1b[?1049h\x1b[?1000habc"), (0, 3));
}
