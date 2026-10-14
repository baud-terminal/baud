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
    (support::screen(&t), t.reply().to_vec())
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

// ── a part-full reply buffer ────────────────────────────────────────────────

/// Every SGR attribute this core reports, one underline style -- `4:3`, the
/// longest spelling -- one blink rate, `5` (`6` is as long), a font, and all
/// three colours as RGB at three digits a channel: a 111-byte reply.
///
/// **No `21`.** It carried one while `21` set its bit beside the family, and
/// then the rendition held curly *and* double and answered in the 114 bytes
/// note AG measured. `21` is the family's double style now, so after `4:3`
/// it would replace the curly and the reply would shrink to 110; the curly
/// is kept because `;4:3` is the longer of the two spellings.
const RICH: &[u8] = b"\x1b[1;2;3;4:3;5;7;8;9;20;26;51;52;53;60;61;62;63;64;11m\
\x1b[58;2;255;255;255;38;2;255;255;255;48;2;255;255;255m";

/// **DECRQSS is sent whole or not at all.** It was assembled fragment by
/// fragment into the reply buffer, each fragment checked on its own, so past
/// the point where the reply stopped fitting a fragment too long was skipped
/// and a later, shorter one written: a DCS with no `ST`, which swallows every
/// later reply on the application's side, or `ESC P 0 ;` with the `1 $ r`
/// gone. Swept in both C1 forms, where the frame is four bytes and two.
#[test]
fn decrqss_for_a_rich_rendition_is_sent_whole_or_not_at_all() {
    let q = |t: &mut Term<'_>| support::feed(t, b"\x1bP$qm\x1b\\");
    crate::replies::whole_or_nothing(RICH, q);
    crate::replies::whole_or_nothing(&[b"\x1b G", RICH].concat(), q);
}

/// **The bound the answer is built in holds the richest rendition there is.**
/// `RICH` sets every attribute an application can hold at once, at the widest
/// spelling of each, so its reply is the longest the encoder can be driven to:
/// 111 bytes. `DECRQSS_MAX` is 126 because it counts all five underline
/// spellings and both blink rates together, which no rendition holds -- the
/// thirteen bytes of `;4`, `;21`, `;4:4` and `;4:5` that `4:3` excludes, and
/// the two of `;6` that `5` does (124 and thirteen until 2026-10-14, when `6`
/// took a bit of its own). It was 114 while `21` could be held beside `4:3`,
/// and the slack ten.
#[test]
fn the_longest_decrqss_reply_is_within_its_bound() {
    let r = replies(&[RICH, b"\x1bP$qm\x1b\\"].concat());
    assert_eq!(r.len(), 111, "the richest rendition answers in 111 bytes");
    assert!(r.ends_with(b"m\x1b\\"), "and whole: {r:02x?}");
    assert_eq!(DECRQSS_MAX, 126, "the number DECRQSS_MAX's doc derives");
    assert_eq!(
        DECRQSS_MAX - r.len(),
        b";4;21;4:4;4:5".len() + b";6".len(),
        "and the slack is the two families"
    );
    const { assert!(DECRQSS_MAX <= REPLY, "so a drained buffer always takes it") };
}

/// The short replies too: a scrolling region (`1;50r`, twelve bytes), a
/// cursor style, and the invalid reply. Each lost its `ST` at the first
/// length past its room -- the region at 245 pending, the style at 247, and
/// the invalid reply at 250, as `ESC P 0 $ r` -- and at 252 the invalid reply
/// was `ESC P ESC \`, the introducer and terminator fitting around a `0 $ r`
/// that did not.
#[test]
fn every_decrqss_reply_is_sent_whole_or_not_at_all() {
    crate::replies::whole_or_nothing(b"", |t| support::feed(t, b"\x1bP$qr\x1b\\"));
    crate::replies::whole_or_nothing(b"", |t| support::feed(t, b"\x1bP$qzz\x1b\\"));
    crate::replies::whole_or_nothing(b"\x1b[5 q", |t| support::feed(t, b"\x1bP$q q\x1b\\"));
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

/// **A BEL does not terminate a DCS.** This test said it did, "as it does an
/// OSC", and pinned the answer; no text says so. `[[source: ctlseqs]]` gives
/// BEL as a terminator "for terminating OSC sequences" (2023-2024),
/// `[[source: ecma48]]` 5.6 closes every control string with ST, and
/// `[[source: williams]]` puts a BEL in `dcs passthrough` (line 388). So the
/// request is still open after the BEL and answers nothing, and the ST that
/// closes it answers for a `Pt` of `r BEL`, which names no setting.
/// `tests/control_strings.rs` holds the rest at every read size.
#[test]
fn a_bel_does_not_terminate_a_dcs() {
    assert_eq!(replies(b"\x1bP$qr\x07"), b"", "the request is still open");
    assert_eq!(replies(b"\x1bP$qr\x07\x1b\\"), b"\x1bP0$r\x1b\\");
    assert_eq!(
        replies(b"\x1bP$qr\x1b\\"),
        b"\x1bP1$r1;50r\x1b\\",
        "control: without the BEL the request is valid"
    );
}

/// The payload is bounded, like an OSC's.
#[test]
fn an_overlong_dcs_payload_is_bounded() {
    let mut v = b"\x1bP$q".to_vec();
    v.extend(std::iter::repeat_n(b'x', OSC_MAX * 3));
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

/// **A colon underline style round-trips through DECRQSS.** The three colon
/// styles have no one-parameter spelling, so the reply must write `4:n`
/// where the walk over `WIRE` writes a bare code for every other bit -- and
/// a reply that wrote `3` for curly would set *italic* when replayed. Each of
/// the three, replayed, reproduces itself and nothing else.
#[test]
fn a_colon_underline_style_round_trips() {
    for (set, name) in [
        (&b"\x1b[4:3m"[..], "curly"),
        (&b"\x1b[4:4m"[..], "dotted"),
        (&b"\x1b[4:5m"[..], "dashed"),
    ] {
        let mut query = set.to_vec();
        query.extend_from_slice(b"\x1bP$qm\x1b\\");
        let reply = replies(&query);
        let body = &reply[5..reply.len() - 2];
        assert!(
            body.windows(2).any(|w| w == b"4:"),
            "{name}: the reply spells the style as 4:n, got {:?}",
            String::from_utf8_lossy(body)
        );
        let mut replay = b"\x1b[".to_vec();
        replay.extend_from_slice(body);
        replay.push(b'X');
        let mut original = set.to_vec();
        original.push(b'X');
        assert_eq!(grid(&replay), grid(&original), "{name} round-trips");
    }
}

/// **The double underline reports alone, as `21`, and round-trips.** While
/// `21` set its bit beside the family, `4:3` then `21` held curly and double
/// and answered `0;21;4:3m` -- a reply that, sent back, sets double and then
/// replaces it with curly, so it did not reproduce what was asked about.
/// `4:2` answers in the same spelling, because it is the same state.
#[test]
fn a_double_underline_after_another_style_reports_alone_and_round_trips() {
    let mut wrong = Vec::new();
    for (set, name) in [
        (&b"\x1b[4:3m\x1b[21m"[..], "4:3 then 21"),
        (&b"\x1b[4m\x1b[21m"[..], "4 then 21"),
        (&b"\x1b[4:3m\x1b[4:2m"[..], "4:3 then 4:2"),
    ] {
        let reply = replies(&[set, b"\x1bP$qm\x1b\\"].concat());
        if reply != b"\x1bP1$r0;21m\x1b\\" {
            wrong.push(format!("{name}: {:?}", String::from_utf8_lossy(&reply)));
        }
        let body = &reply[5..reply.len() - 2];
        let replay = [&b"\x1b["[..], body, b"X"].concat();
        if grid(&replay) != grid(&[set, b"X"].concat()) {
            wrong.push(format!("{name}: the reply does not round-trip"));
        }
    }
    // Every case is answered before anything is asserted, so a red run names
    // each wrong reply rather than stopping at the first.
    assert!(wrong.is_empty(), "want 0;21m, round-tripping: {wrong:#?}");
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
