//! DCS parsing and DECRQSS.
//!
//! `[[source: williams]]` splits DCS into `dcs_entry`, `dcs_param`,
//! `dcs_intermediate`, `dcs_passthrough` and `dcs_ignore`. The distinction that
//! matters for safety is the last one: an unhandled DCS must consume its
//! payload rather than fall back to Ground, or the payload runs as commands.
//!
//! `[[source: ctlseqs]]`: `DCS $ q Pt ST` is "Request Status String (DECRQSS),
//! VT420 and up", and "xterm responds with `DCS 1 $ r Pt ST` for valid
//! requests, replacing the Pt with the corresponding CSI string, or
//! `DCS 0 $ r ST` for invalid requests."
//!
//! **1 means valid and 0 means invalid, and the DEC manuals say the opposite.**
//! ctlseqs records the errata: "testing a DEC VT420 in 1996 showed that the
//! documented code for a valid or invalid response to DECRQSS was reversed",
//! and vttest and DEC STD 070 agree with the tested behaviour rather than with
//! the VT420 and VT525 manuals. baud follows xterm, because that is what
//! applications are written against. Reversed, every reply would tell the
//! application its setting is unsupported.

use baud_core::*;

fn run(bytes: &[u8]) -> (Vec<PackedCell>, Vec<u8>) {
    let mut s = Storage::boxed();
    let mut t = Term::new(&mut s, OpMode::Computed);
    let mut i = 0;
    while i < bytes.len() {
        let n = READ_CHUNK.min(bytes.len() - i);
        let mut slot = t.read_slot();
        let base = slot.base();
        let f = slot.write(StreamByte::slice_from(&bytes[i..i + n]));
        t.feed(base, f.len());
        t.commit_read(f.len());
        i += n;
    }
    (t.viewport().to_vec(), t.reply().to_vec())
}

fn replies(b: &[u8]) -> Vec<u8> {
    run(b).1
}
fn grid(b: &[u8]) -> Vec<PackedCell> {
    run(b).0
}

// ── DECRQSS answers ─────────────────────────────────────────────────────────

/// The scrolling region, in a form the application can send straight back.
#[test]
fn decrqss_reports_decstbm() {
    assert_eq!(replies(b"\x1b[3;20r\x1bP$qr\x1b\\"), b"\x1bP1$r3;20r\x1b\\");
}

/// The cursor style.
#[test]
fn decrqss_reports_decscusr() {
    assert_eq!(replies(b"\x1b[5 q\x1bP$q q\x1b\\"), b"\x1bP1$r5 q\x1b\\");
    assert_eq!(replies(b"\x1bP$q q\x1b\\"), b"\x1bP1$r1 q\x1b\\", "default");
}

/// The protection attribute.
#[test]
fn decrqss_reports_decsca() {
    assert_eq!(replies(b"\x1b[1\"q\x1bP$q\"q\x1b\\"), b"\x1bP1$r1\"q\x1b\\");
    assert_eq!(replies(b"\x1bP$q\"q\x1b\\"), b"\x1bP1$r0\"q\x1b\\");
}

/// SGR, which is the one with a variable-length body.
///
/// The reply always starts at `0`, because an application must be able to send
/// it back verbatim and reproduce the state — a string that only *added*
/// attributes would not clear ones already set.
#[test]
fn decrqss_reports_sgr() {
    assert_eq!(replies(b"\x1bP$qm\x1b\\"), b"\x1bP1$r0m\x1b\\", "default");
    assert_eq!(replies(b"\x1b[1;4m\x1bP$qm\x1b\\"), b"\x1bP1$r0;1;4m\x1b\\");
    assert_eq!(
        replies(b"\x1b[31m\x1bP$qm\x1b\\"),
        b"\x1bP1$r0;38;5;1m\x1b\\",
        "an indexed colour reports as 38;5"
    );
    assert_eq!(
        replies(b"\x1b[38;2;10;20;30m\x1bP$qm\x1b\\"),
        b"\x1bP1$r0;38;2;10;20;30m\x1b\\"
    );
}

/// The SGR reply round-trips: sending it back reproduces the rendition.
///
/// This is the property that makes the report useful rather than merely
/// well-formed.
#[test]
fn the_sgr_reply_round_trips() {
    let original = grid(b"\x1b[1;4;7;41mX");
    let reply = replies(b"\x1b[1;4;7;41m\x1bP$qm\x1b\\");
    // Strip `DCS 1 $ r` and the ST, leaving the CSI tail.
    let body = &reply[5..reply.len() - 2];
    let mut replay = b"\x1b[".to_vec();
    replay.extend_from_slice(body);
    replay.push(b'X');
    assert_eq!(
        grid(&replay),
        original,
        "the report must reproduce the state"
    );
}

/// A setting baud does not implement gets the **invalid** reply, which carries
/// no body.
#[test]
fn an_unknown_setting_gets_the_invalid_reply() {
    assert_eq!(replies(b"\x1bP$q\"p\x1b\\"), b"\x1bP0$r\x1b\\", "DECSCL");
    assert_eq!(replies(b"\x1bP$qzz\x1b\\"), b"\x1bP0$r\x1b\\");
}

/// Valid is 1 and invalid is 0, never the other way round.
///
/// Pinned as its own test because the DEC manuals document it reversed, so a
/// future reader consulting them would "fix" it in exactly the wrong direction.
#[test]
fn valid_is_one_and_invalid_is_zero() {
    let valid = replies(b"\x1bP$qm\x1b\\");
    let invalid = replies(b"\x1bP$qzz\x1b\\");
    assert_eq!(&valid[..5], b"\x1bP1$r", "a known setting answers 1");
    assert_eq!(&invalid[..5], b"\x1bP0$r", "an unknown one answers 0");
}

// ── the parser states ───────────────────────────────────────────────────────

/// An unhandled DCS consumes its payload rather than executing it.
///
/// This is the `dcs_ignore` state earning its keep: without it the payload
/// falls through to Ground and the bytes inside it run as commands.
///
/// **Note what is and is not guaranteed.** The payload here contains no `ESC`,
/// and that is the precise condition. `[[source: williams]]` says a bare `ESC`
/// *cancels* a control string, so a payload containing one is not inert by
/// design — the string ends there and what follows is a fresh sequence. This
/// codebase has already once asserted the stronger, wrong property in an APC
/// test and had to weaken it when the spec was read.
#[test]
fn an_unhandled_dcs_payload_is_not_executed() {
    let attack = b"hello\x1bP1;2|[2J and some text\x1b\\";
    assert_eq!(
        grid(attack),
        grid(b"hello"),
        "an ESC-free payload must be wholly inert"
    );
}

/// A DCS payload with an `ESC` in it is cancelled there, and the rest runs.
///
/// Asserted explicitly so the limit is recorded rather than discovered.
#[test]
fn an_esc_in_a_dcs_payload_cancels_rather_than_hides() {
    // The ESC ends the string; `[2J` is then an ordinary ED.
    assert_eq!(grid(b"hello\x1bP1;2|\x1b[2J"), grid(b""));
}

/// The same for a DCS with parameters and intermediates, which walks
/// `dcs_entry` -> `dcs_param` -> `dcs_intermediate`.
#[test]
fn a_parameterised_dcs_is_consumed() {
    assert_eq!(grid(b"ab\x1bP0;1$p[2J payload\x1b\\cd"), grid(b"abcd"));
}

/// A bare `ESC` cancels the control string.
///
/// `[[source: williams]]`: "receiving the ESC character will cancel the control
/// string." The DECRQSS must not answer.
#[test]
fn a_bare_esc_cancels_the_dcs() {
    assert!(
        replies(b"\x1bP$qm\x1bE").is_empty(),
        "a cancelled DECRQSS must not reply"
    );
}

/// BEL terminates a DCS too, as it does an OSC.
#[test]
fn bel_terminates_a_dcs() {
    assert_eq!(replies(b"\x1bP$qr\x07"), b"\x1bP1$r1;50r\x1b\\");
}

/// The payload is bounded, like an OSC's.
#[test]
fn an_overlong_dcs_payload_is_bounded() {
    let mut v = b"\x1bP$q".to_vec();
    v.extend(std::iter::repeat(b'x').take(OSC_MAX * 3));
    v.extend_from_slice(b"\x1b\\");
    // Truncated to an unknown setting, answered as invalid, and no panic.
    assert_eq!(replies(&v), b"\x1bP0$r\x1b\\");
}

/// DECRQSS parses the same however the read boundaries fall.
///
/// The DCS states are only in the byte-at-a-time machine — the fast path hands
/// DCS back to it — so this checks the handover as much as the states.
#[test]
fn dcs_is_chunk_invariant() {
    let seq = b"\x1b[3;20r\x1bP$qr\x1b\\tail";
    let whole = replies(seq);
    assert!(!whole.is_empty());
    for chunk in 1..seq.len() {
        let mut s = Storage::boxed();
        let mut t = Term::new(&mut s, OpMode::Computed);
        let mut i = 0;
        while i < seq.len() {
            let n = chunk.min(seq.len() - i);
            let mut slot = t.read_slot();
            let base = slot.base();
            let f = slot.write(StreamByte::slice_from(&seq[i..i + n]));
            t.feed(base, f.len());
            t.commit_read(f.len());
            i += n;
        }
        assert_eq!(t.reply(), &whole[..], "differed at chunk {chunk}");
    }
}

/// Text after a DCS is printed, not swallowed.
#[test]
fn text_after_a_dcs_still_prints() {
    assert_eq!(grid(b"\x1bP$qm\x1b\\abc"), grid(b"abc"));
}

// ── the DCS prefix states, per williams ─────────────────────────────────────
//
// `dcs_entry`, `dcs_param` and `dcs_intermediate` were one merged arm, so all
// three rules that distinguish them were lost — the same merge, and the same
// three bugs, that `csi_entry` had.

/// A C0 inside the DCS prefix does not discard the sequence.
///
/// `[[source: williams]]`: "C0 controls other than CAN, SUB and ESC are **not
/// executed** while recognising the first part of a device control string."
/// Not executed *and* not an error — they are ignored, and the sequence
/// continues. baud sent them to `dcs_ignore`, so a stray NUL silently cost an
/// application its DECRQSS reply.
///
/// Note this differs from a control *sequence*, where williams says "C0
/// controls are executed immediately". Same byte, different state, different
/// rule — which is exactly why the states are separate.
#[test]
fn a_c0_in_the_dcs_prefix_does_not_discard_it() {
    assert_eq!(
        replies(b"\x1bP\x00$qm\x1b\\"),
        replies(b"\x1bP$qm\x1b\\"),
        "the NUL must not cost the DECRQSS its reply"
    );
    assert!(
        !replies(b"\x1bP\x00$qm\x1b\\").is_empty(),
        "and there must be a reply to lose"
    );
}

/// DEL likewise.
#[test]
fn a_del_in_the_dcs_prefix_does_not_discard_it() {
    assert_eq!(
        replies(b"\x1bP\x7f$qm\x1b\\"),
        replies(b"\x1bP$qm\x1b\\"),
        "DEL is ignored, not an error"
    );
}

/// A private marker is legal only as the first byte of the DCS parameter
/// string, exactly as in `csi_entry`.
///
/// `[[source: williams]]`: "Occurrences of 3C to 3F in positions other than the
/// first character of the parameter string cause the entire control sequence to
/// be ignored." The merged arm accepted one anywhere.
#[test]
fn a_marker_after_a_dcs_parameter_is_ignored() {
    assert!(
        replies(b"\x1bP1?$qm\x1b\\").is_empty(),
        "`DCS 1 ? $ q` is malformed and must produce no reply"
    );
}

/// A parameter after an intermediate is malformed.
///
/// williams' `dcs_intermediate` sends `30-3F` to `dcs_ignore`: once an
/// intermediate has arrived, the parameter string is over. The merged arm
/// walked back into `dcs_param` and accepted the digit.
#[test]
fn a_parameter_after_a_dcs_intermediate_is_ignored() {
    assert!(
        replies(b"\x1bP$1qm\x1b\\").is_empty(),
        "`DCS $ 1 q` is malformed and must produce no reply"
    );
}
