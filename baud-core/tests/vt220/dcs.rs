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

use crate::support;
use baud_core::*;

fn run(bytes: &[u8]) -> (Vec<PackedCell>, Vec<u8>) {
    let ws = support::CORPUS;
    let mut s = Owned::new(&ws);
    let mut t = Term::new(s.storage(), &ws);
    support::feed(&mut t, bytes);
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
#[ignore = "baud-core does not ignore C0/DEL inside a DCS prefix -- see the_known_vt220_gaps"]
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
#[ignore = "baud-core does not ignore C0/DEL inside a DCS prefix -- see the_known_vt220_gaps"]
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

/// **The widest rendition baud can hold still fits in the reply buffer.**
///
/// `Attrs::WIRE` went from 13 entries to 18 when the ideogram aspects landed,
/// and DECRQSS walks all of it — so the worst-case SGR reply grew by
/// `;60;61;62;63;64` on top of three truecolour specs and a font. `REPLY` is a
/// fixed 256 bytes in a core with no allocator, and a truncated reply is the
/// failure `docs/vt-coverage.md` calls non-negotiable: an application parses
/// whatever arrives as the report it asked for, so a short answer is worse
/// than none.
///
/// `report_encodings_fit` does not cover this. It pins the *mouse* report as
/// "the long one" against a hardcoded literal and derives nothing from
/// `WIRE.len()`, so it would keep passing however wide the rendition grew —
/// the same shape of blind spot as a search whose probe cannot match.
///
/// The assertion is a round-trip rather than a length, because truncation
/// produces a reply that is still well-formed and merely *shorter than what
/// was set*, which every "is this a valid CSI" check passes happily.
#[test]
fn the_widest_rendition_survives_the_reply_buffer() {
    // Every attribute in `WIRE`, the ninth alternate font, and all three
    // colours in their widest spelling.
    // Split across sequences because `Term::params` holds 16 and all eighteen
    // attributes plus a font is nineteen parameters. Sending them as one CSI
    // silently drops the last three, which is what the first version of this
    // test did — it then failed against the reply builder, which was innocent.
    const MAXIMAL: &[u8] = b"\x1b[1;2;3;4;5;7;8;9;20;26;51;52;53m\
\x1b[60;61;62;63;64;19m\
\x1b[38;2;255;255;255m\x1b[48;2;254;253;252m\x1b[58;2;251;250;249m";

    let mut query = MAXIMAL.to_vec();
    query.extend_from_slice(b"\x1bP$qm\x1b\\");
    let reply = replies(&query);

    println!(
        "REPLY len={} params={} bytes={:?}",
        reply.len(),
        reply.iter().filter(|b| **b == b';').count() + 1,
        String::from_utf8_lossy(&reply)
    );
    assert_eq!(&reply[..5], b"\x1bP1$r", "valid, and the header is intact");
    assert_eq!(
        &reply[reply.len() - 3..],
        b"m\x1b\\",
        "the final byte and ST both survived — truncation eats these first"
    );

    // The property that matters: replaying the reply reproduces the rendition.
    let body = &reply[5..reply.len() - 2];
    let mut replay = b"\x1b[".to_vec();
    replay.extend_from_slice(body);
    replay.push(b'X');

    let mut original = MAXIMAL.to_vec();
    original.push(b'X');
    assert_eq!(
        grid(&replay),
        grid(&original),
        "the widest reply still round-trips, so nothing was dropped"
    );
}

/// The *smallest* rendition whose reply does not round-trip.
///
/// Bold plus three truecolours is sixteen parameters going in — it fits — but
/// the reply spells them back as `0;1;58;2;..;38;2;..;48;2;..`, which is
/// seventeen. An editor drawing an undercurl in its own colour over truecolour
/// text is in this state, so this is not a corner.
#[test]
fn a_seventeen_parameter_reply_round_trips() {
    const SET: &[u8] = b"\x1b[1;38;2;10;20;30;48;2;40;50;60;58;2;70;80;90m";
    let mut query = SET.to_vec();
    query.extend_from_slice(b"\x1bP$qm\x1b\\");
    let reply = replies(&query);

    let body = &reply[5..reply.len() - 2];
    let mut replay = b"\x1b[".to_vec();
    replay.extend_from_slice(body);
    replay.push(b'X');
    let mut original = SET.to_vec();
    original.push(b'X');
    assert_eq!(
        grid(&replay),
        grid(&original),
        "reply {:?} did not reproduce the rendition",
        core::str::from_utf8(body).unwrap()
    );
}
