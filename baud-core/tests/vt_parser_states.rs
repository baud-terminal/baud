//! The parser state machine, per `[[source: williams]]`.
//!
//! Tier 0 in `docs/vt-coverage.md`: not a tier of features but the substrate
//! everything else stands on. Four states were missing, and their absence was
//! observable from the stream:
//!
//! * `sos_pm_apc_string` — DCS/SOS/PM/APC fell through to Ground, so a string's
//!   **payload was executed as commands**. That is a correctness and security
//!   bug, not a missing feature, and `apc_payload_is_not_executed` is the test
//!   that pins it.
//! * `csi_ignore` — a malformed CSI returned to Ground mid-sequence, printing
//!   the remainder as text.
//! * `escape_intermediate` — `ESC # 8` dropped the `#` and printed the `8`.
//! * `csi_intermediate` — the intermediate was discarded, so `CSI ! p` was
//!   indistinguishable from `CSI p`.

use baud_core::*;

fn run(bytes: &[u8]) -> (Vec<PackedCell>, (usize, usize)) {
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
    (t.viewport().to_vec(), (r.index(), c.index()))
}

fn grid(bytes: &[u8]) -> Vec<PackedCell> {
    run(bytes).0
}
fn cursor(bytes: &[u8]) -> (usize, usize) {
    run(bytes).1
}

// ── string states: the payload must never execute ───────────────────────────

/// An APC payload is consumed, not executed.
///
/// `ESC _` used to fall through to Ground, so everything up to the terminator
/// was interpreted. A stream could hide `CSI 2 J` in an APC string and clear
/// the screen -- kitty's graphics protocol uses `APC G`, so this is reachable
/// from ordinary image output, not only from a hostile stream.
#[test]
fn apc_payload_is_not_executed() {
    // A well-formed payload: no ESC, terminated by ST.
    assert_eq!(
        grid(b"hello\x1b_Gpayload;with=params,and:punctuation\x1b\\world"),
        grid(b"helloworld"),
        "an APC payload must have no effect on the screen"
    );
    assert_eq!(cursor(b"hello\x1b_Gpayload\x1b\\world"), (0, 10));
}

/// A payload containing a bare ESC cancels the string, and what follows is
/// interpreted. That is the specified behaviour, not a hole.
///
/// `[[source: williams]]`: "receiving the ESC character will 'cancel' the
/// control string, so the ST control function that is invoked by the arrival of
/// the following '\\' is essentially a no-op function."
///
/// So the guarantee is precisely: a payload that does not contain ESC is
/// consumed. A payload that does is malformed, and a terminal is entitled to
/// resume parsing. Asserting anything stronger would contradict the spec.
#[test]
fn a_bare_esc_cancels_a_control_string_by_design() {
    let cancelled = grid(b"\x1b_G\x1b[2Jx\x1b\\");
    let cleared = grid(b"\x1b[2Jx");
    assert_eq!(
        cancelled, cleared,
        "ESC inside a string cancels it, per the DEC parser"
    );
}

/// Same rule for PM, SOS and DCS.
#[test]
fn pm_sos_and_dcs_payloads_are_not_executed() {
    for (name, intro) in [("PM", &b"\x1b^"[..]), ("SOS", b"\x1bX"), ("DCS", b"\x1bP")] {
        let mut stream = b"ab".to_vec();
        stream.extend_from_slice(intro);
        // Well-formed: no ESC in the payload.
        stream.extend_from_slice(b"1;2;3|payload$data\x1b\\");
        stream.extend_from_slice(b"cd");
        assert_eq!(
            grid(&stream),
            grid(b"abcd"),
            "{name}: payload must not reach the screen"
        );
    }
}

/// BEL terminates a string too, as xterm accepts.
#[test]
fn a_string_may_be_terminated_by_bel() {
    assert_eq!(grid(b"ab\x1b_payload\x07cd"), grid(b"abcd"));
}

/// A truncated string does not swallow the rest of the stream forever: any ESC
/// leaves it, and the Escape state decides what the next byte means.
#[test]
fn a_truncated_string_does_not_swallow_everything() {
    // No terminator, then a fresh CSI that must still take effect.
    assert_eq!(cursor(b"\x1b_unterminated\x1b[5;9H"), (4, 8));
}

// ── csi_ignore ──────────────────────────────────────────────────────────────

/// A malformed CSI is consumed to its final byte, not printed.
#[test]
fn a_malformed_csi_is_consumed_whole() {
    // `<` is a private-marker byte in a position baud does not accept; whatever
    // the reason for rejection, the remainder must not reach the screen.
    assert_eq!(
        grid(b"ab\x1b[1;2\x00\x01m cd"),
        grid(b"ab cd"),
        "the rest of a bad CSI must not be printed"
    );
}

/// The sequence after a malformed one still works -- csi_ignore must end.
#[test]
fn parsing_recovers_after_a_malformed_csi() {
    assert_eq!(cursor(b"\x1b[1;2\x01m\x1b[7;3H"), (6, 2));
}

// ── escape_intermediate ─────────────────────────────────────────────────────

/// `ESC # 8` is DECALN. It is not implemented, but the `8` must not print.
#[test]
fn esc_intermediate_final_is_not_printed() {
    assert_eq!(
        grid(b"ab\x1b#8cd"),
        grid(b"abcd"),
        "the final byte of ESC # 8 must be consumed"
    );
    assert_eq!(cursor(b"ab\x1b#8cd"), (0, 4));
}

/// Charset designators are consumed the same way.
#[test]
fn charset_designators_are_consumed() {
    assert_eq!(grid(b"ab\x1b(Bcd"), grid(b"abcd"));
    assert_eq!(grid(b"ab\x1b)0cd"), grid(b"abcd"));
}

// ── csi_intermediate and DECSTR ─────────────────────────────────────────────

/// `CSI ! p` is DECSTR and `CSI p` is not the same operation.
#[test]
fn decstr_is_distinguished_by_its_intermediate() {
    // DECSTR restores the full-screen region, so a line feed at the old region
    // bottom stops scrolling the region.
    let after_decstr = grid(b"\x1b[3;5r\x1b[!p\x1b[5;1HX\n\nY");
    let never_set = grid(b"\x1b[5;1HX\n\nY");
    assert_eq!(after_decstr, never_set, "DECSTR must reset the region");

    // Bare `CSI p` is not DECSTR: the region survives it.
    let after_bare = grid(b"\x1b[3;5r\x1b[p\x1b[5;1HX\n\nY");
    assert_ne!(after_bare, never_set, "CSI p must not be treated as DECSTR");
}

/// DECSTR is soft: it resets modes and the region but keeps the screen. That is
/// the whole difference from RIS.
#[test]
fn decstr_keeps_the_screen_and_ris_does_not() {
    let text = b"hello world";
    let mut soft = text.to_vec();
    soft.extend_from_slice(b"\x1b[!p");
    let mut hard = text.to_vec();
    hard.extend_from_slice(b"\x1bc");

    assert_ne!(
        grid(&soft),
        grid(&hard),
        "DECSTR must not clear the screen the way RIS does"
    );
}

/// DECSTR restores LNM, so LF stops carriage-returning.
#[test]
fn decstr_restores_modes() {
    assert_eq!(cursor(b"\x1b[20h\x1b[!pabcde\n"), (1, 5));
}

// ── string terminator ───────────────────────────────────────────────────────

/// Deliberately `Computed`, not `Runs`.
///
/// In `Runs` mode `viewport()` returns the raw cell array, which only reflects
/// *promoted* lines -- so it reports representation rather than rendered
/// content. MAXRUN is 8, and 112 single-byte reads make 112 spans where one
/// read makes one, so the line promotes under one chunking and not the other
/// and the viewports differ without the *content* differing. `Computed` writes
/// cells directly, so comparing it compares what a renderer would draw.
fn drive_chunked(bytes: &[u8], chunk: usize) -> Vec<PackedCell> {
    let mut s = Storage::boxed();
    let mut t = Term::new(&mut s, OpMode::Computed);
    let mut i = 0;
    while i < bytes.len() {
        let n = chunk.min(READ_CHUNK).min(bytes.len() - i);
        let mut slot = t.read_slot();
        let base = slot.base();
        let filled = slot.write(StreamByte::slice_from(&bytes[i..i + n]));
        t.feed(base, filled.len());
        t.commit_read(filled.len());
        i += n;
    }
    t.viewport().to_vec()
}

/// ST is `ESC \` and nothing else, so `OSC ... ESC E` is a *cancelled* string
/// followed by a NEL -- the NEL must run.
///
/// `fast_esc` scanned for a terminator and, on finding ESC, consumed the byte
/// after it without checking that it was the backslash. That swallowed the `E`.
/// The byte-at-a-time path cancelled the string and executed the NEL, so
/// whether a stream got its line break depended on where the read boundary
/// fell. Found by `ingest_is_chunk_invariant`; this pins it without relying on
/// proptest replaying a saved case.
///
/// `[[source: ctlseqs]]` line 201 defines ST; `[[source: williams]]` gives the
/// cancel rule.
#[test]
fn a_non_backslash_after_esc_does_not_terminate_a_string() {
    // NEL after a cancelled OSC moves to the next line.
    assert_eq!(
        cursor(b"\x1b]0;title\x1bErest"),
        (1, 4),
        "the NEL that cancelled the OSC must still execute"
    );
    // A real ST leaves the cursor on the same line.
    assert_eq!(cursor(b"\x1b]0;title\x1b\\rest"), (0, 4));
}

/// The same bytes must parse identically however they are split, including a
/// split that lands exactly on the ESC of a cancelling sequence.
#[test]
fn osc_cancel_is_chunk_invariant() {
    let mut v = vec![b'A'; 112];
    v.extend_from_slice(b"\x1b]");
    v.extend(std::iter::repeat(b'A').take(112));
    v.extend_from_slice(b"\x1bE\x9d");
    let whole = drive_chunked(&v, 4096);
    for c in [1usize, 113, 226, 227, 228] {
        assert_eq!(
            drive_chunked(&v, c),
            whole,
            "chunk {c} disagrees with a single read"
        );
    }
}
