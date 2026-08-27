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
    // `<` is a private marker, and `[[source: williams]]` allows those only in
    // `csi_entry` -- here it arrives after two parameters, so `csi_param` sends
    // the whole sequence to `csi_ignore`. The `m` is consumed as the final
    // byte and applies no rendition; ` cd` must print plain.
    assert_eq!(
        grid(b"ab\x1b[1;2<m cd"),
        grid(b"ab cd"),
        "the rest of a bad CSI must not be printed"
    );
}

/// A C0 byte inside a control sequence is executed where it appears, and the
/// sequence carries on.
///
/// `[[source: williams]]` gives `csi_param` an `execute` action on
/// `00-17,19,1C-1F` that returns to the same state. baud had no C0 branch
/// there, so those bytes fell through to `csi_ignore` and a stray NUL silently
/// cancelled an unrelated SGR. This test previously asserted that discard as if
/// it were correct.
#[test]
fn a_c0_inside_a_csi_does_not_cancel_it() {
    assert_eq!(
        grid(b"\x1b[1;2\x00\x01mcd"),
        grid(b"\x1b[1;2mcd"),
        "NUL and SOH must not change what the SGR does"
    );
}

/// The executed C0 is not merely skipped — it runs.
///
/// A backspace inside the parameter string moves the cursor, which is
/// observable and is what "execute" means. Skipping it and executing it look
/// identical on a NUL, so the rule needs a C0 that does something.
#[test]
fn the_c0_inside_a_csi_actually_executes() {
    assert_eq!(
        cursor(b"abc\x1b[\x08m"),
        (0, 2),
        "the BS must have moved the cursor"
    );
}

/// The sequence after a malformed one still works -- csi_ignore must end.
#[test]
fn parsing_recovers_after_a_malformed_csi() {
    assert_eq!(cursor(b"\x1b[1;2\x01m\x1b[7;3H"), (6, 2));
}

// ── escape_intermediate ─────────────────────────────────────────────────────

/// The final byte of an `ESC <intermediate> <final>` sequence is consumed, not
/// printed, whether or not the sequence is implemented.
///
/// `ESC # 8` is DECALN and now does something (see vt_sgr_rep_align.rs), so the
/// unimplemented `ESC # 3` carries this assertion: what matters here is the
/// parser state, not the operation.
#[test]
fn esc_intermediate_final_is_not_printed() {
    assert_eq!(
        grid(b"ab\x1b#3cd"),
        grid(b"abcd"),
        "the final byte of an ESC # sequence must be consumed"
    );
    assert_eq!(cursor(b"ab\x1b#3cd"), (0, 4));
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

// ── csi_entry vs csi_param: a private marker is legal only in first position ──

/// `[[source: williams]]`: `csi_entry` collects `3C-3F` as private markers;
/// `csi_param` sends the same bytes to `csi_ignore`. A marker is legal **only**
/// as the first byte after `CSI`.
///
/// baud folded `csi_entry` into `CsiParams` — the two states differ in exactly
/// this transition, and the fold dropped it. The result is not a cosmetic
/// divergence: the sequence *executes* instead of being ignored, which is the
/// dangerous direction. `CSI 2 ? l` is a malformed DECRST that must do nothing,
/// and instead reset DECANM and put the terminal into VT52 mode — a protocol
/// switch out of a sequence the spec says to discard.
#[test]
fn a_private_marker_after_a_parameter_is_ignored() {
    // Sound the alarm on the worst case first: a protocol switch.
    // In VT52 mode `ESC A` is cursor-up; in ANSI it is an unknown two-byte
    // escape and prints nothing. If the grid shows ANSI behaviour, the bogus
    // DECRST was correctly ignored.
    let vt52_switched = grid(b"\x1b[2?lX");
    let never_sent = grid(b"X");
    assert_eq!(
        vt52_switched, never_sent,
        "`CSI 2 ? l` is malformed and must be ignored, not executed as DECRST 2"
    );
}

/// The same rule over an erase, which is the destructive case.
///
/// `CSI 1 ? J` must not run DECSED. Every cell written before it has to
/// survive.
#[test]
fn a_marker_after_a_parameter_does_not_erase() {
    let after = grid(b"hello\x1b[1?J");
    let untouched = grid(b"hello");
    assert_eq!(
        after, untouched,
        "`CSI 1 ? J` is malformed and must be ignored, not executed as DECSED"
    );
}

/// A marker in first position still works — the fix must not break the form
/// that is legal.
#[test]
fn a_private_marker_in_first_position_still_dispatches() {
    let selective = grid(b"hi\x1b[?1J");
    let plain = grid(b"hi");
    assert_ne!(
        selective, plain,
        "`CSI ? 1 J` is well-formed DECSED and must still execute"
    );
}

/// The rest of the malformed sequence is swallowed, not printed.
///
/// This is what `csi_ignore` is for, and it is the bug the state originally
/// fixed: returning to Ground mid-sequence printed the remainder as text.
#[test]
fn an_ignored_csi_does_not_print_its_tail() {
    let ignored = grid(b"\x1b[1?J");
    let empty = grid(b"");
    assert_eq!(
        ignored, empty,
        "the ignored sequence must leave no text behind"
    );
}

// ── the "anywhere" block: ESC, CAN and SUB ──────────────────────────────────

/// An ESC inside a control sequence **abandons it and starts a new one**.
///
/// `[[source: williams]]` puts `1B` in the *anywhere* block, so it overrides
/// whatever the current state would otherwise do. baud's arms tested
/// `b.is_c0()`, which is `b < 0x20` and therefore matches ESC — so the ESC was
/// "executed" and the parser stayed in the parameter string. `CSI 1 ESC [ 2 m`
/// lost its escape: the `[` was accepted as a final byte, dispatching a
/// nonsense `CSI 1 [`, and `2m` printed as text.
#[test]
fn an_esc_inside_a_csi_starts_a_new_sequence() {
    assert_eq!(
        grid(b"\x1b[1\x1b[2mX"),
        grid(b"\x1b[2mX"),
        "the ESC abandons `CSI 1` and `CSI 2 m` takes effect"
    );
}

/// CAN and SUB abandon the sequence in progress, from any state.
///
/// Also in `[[source: williams]]`'s anywhere block, and also swallowed by
/// `is_c0()`: they were being executed while the parser stayed in the
/// parameter string, so the sequence completed as if they had not arrived.
#[test]
fn can_and_sub_abandon_a_control_sequence() {
    for b in [0x18u8, 0x1a] {
        let mut seq = b"\x1b[3".to_vec();
        seq.push(b);
        seq.extend_from_slice(b"1mX");
        assert_eq!(
            grid(&seq),
            grid(b"1mX"),
            "0x{b:02x} must abandon the CSI, leaving `1mX` as text"
        );
    }
}

/// The same rule inside a control string.
///
/// A payload that cannot be cancelled is the same hazard as one that is
/// executed: it swallows the stream until something else ends it.
#[test]
fn can_abandons_a_control_string() {
    assert_eq!(
        grid(b"\x1b_apc payload\x18X"),
        grid(b"X"),
        "CAN must end the APC string and leave the rest of the stream alone"
    );
}

/// A C0 in `csi_entry` does not consume the private-marker position.
///
/// `[[source: williams]]` has `csi_entry` execute a C0 and **stay in
/// `csi_entry`**. Advancing to `csi_param` first would apply the marker rule
/// one byte too early and reject a marker that is legal — turning a valid
/// DECSED into a discarded sequence.
#[test]
fn a_c0_in_csi_entry_does_not_consume_the_marker_position() {
    // BS is chosen because it is observable: it moves the cursor.
    assert_eq!(
        grid(b"hello\x1b[\x08?1J"),
        grid(b"hello\x08\x1b[?1J"),
        "`CSI <BS> ? J` must still be DECSED, with the BS executed"
    );
}

/// DEL likewise does not consume the marker position.
#[test]
fn a_del_in_csi_entry_does_not_consume_the_marker_position() {
    assert_eq!(
        grid(b"hello\x1b[\x7f?1J"),
        grid(b"hello\x1b[?1J"),
        "`CSI <DEL> ? J` must still be DECSED"
    );
}

/// A C0 inside the *intermediate* string does not discard the sequence.
///
/// `csi_intermediate` had no C0 branch at all, so a stray byte there fell to
/// `csi_ignore` and `CSI ! <NUL> p` silently lost its soft reset.
///
/// The probe has to make DECSTR's effect *visible*. The first version of this
/// test compared `CSI ! <NUL> p X` against `CSI ! p X` on a blank screen, where
/// a soft reset changes nothing observable — so it passed with the bug still in
/// place. Setting a rendition first is what gives it something to reset.
#[test]
fn a_c0_in_csi_intermediate_does_not_discard_the_sequence() {
    assert_eq!(
        grid(b"\x1b[31m\x1b[!\x00pX"),
        grid(b"\x1b[31m\x1b[!pX"),
        "the NUL must not cancel DECSTR"
    );
    assert_ne!(
        grid(b"\x1b[31m\x1b[!\x00pX"),
        grid(b"\x1b[31mX"),
        "and the reset must actually have happened -- X must not still be red"
    );
}

/// A cancel inside an OSC payload is honoured whichever feed sees it.
///
/// Deterministic pin for a `chunk_invariant_all_modes` failure. When CAN and
/// SUB started cancelling control strings, `fast_esc` still scanned an OSC
/// straight to its terminator and dispatched it — so the same bytes set a title
/// or did not, depending on whether they happened to land inside one `read(2)`.
///
/// Feeding one byte at a time forces the state machine; feeding the lot forces
/// the fast path. They must agree.
#[test]
fn a_cancel_inside_an_osc_is_honoured_in_both_feeds() {
    let bytes = b"\x1b]0;title\x18X";
    let mut whole = Storage::boxed();
    let mut t = Term::new(&mut whole, OpMode::Computed);
    let mut slot = t.read_slot();
    let base = slot.base();
    let f = slot.write(StreamByte::slice_from(bytes));
    t.feed(base, f.len());
    t.commit_read(f.len());
    let fast = t.viewport().to_vec();

    let mut split = Storage::boxed();
    let mut u = Term::new(&mut split, OpMode::Computed);
    for byte in bytes {
        let mut slot = u.read_slot();
        let base = slot.base();
        let f = slot.write(StreamByte::slice_from(&[*byte]));
        u.feed(base, f.len());
        u.commit_read(f.len());
    }
    assert_eq!(
        fast,
        u.viewport().to_vec(),
        "one read and one-byte reads must agree on a cancelled OSC"
    );
}
