//! DECKPAM/DECKPNM and XTVERSION.
//!
//! Both are here because both were `parsed` in the sense
//! `docs/vt-coverage.md` treats as a bug. `ESC =` and `ESC >` were in the fast
//! path's list of known two-byte escapes, were consumed, and then did nothing:
//! recognised, accepted, dropped, with no way for the application to find out.
//!
//! `[[source: ctlseqs]]`:
//! * "ESC = Application Keypad (DECKPAM)."
//! * "ESC > Normal Keypad (DECKPNM), VT100."
//! * "CSI > Ps q, Ps = 0 -> Report xterm name and version (XTVERSION). The
//!   response is a DSR sequence identifying the version: `DCS > | text ST`".

use crate::support;
use baud_core::*;

fn drive(bytes: &[u8]) -> (Vec<u8>, KeypadMode) {
    let ws = support::CORPUS;
    let mut s = Owned::new(&ws);
    let mut t = Term::new(s.storage(), &ws);
    support::feed(&mut t, bytes);
    (t.reply().to_vec(), t.keypad())
}

fn keypad(bytes: &[u8]) -> KeypadMode {
    drive(bytes).1
}

fn replies(bytes: &[u8]) -> Vec<u8> {
    drive(bytes).0
}

// ── DECKPAM / DECKPNM ───────────────────────────────────────────────────────

/// The keypad starts numeric and `ESC =` switches it.
#[test]
fn the_keypad_mode_is_recorded() {
    assert_eq!(keypad(b""), KeypadMode::Numeric, "power-on");
    assert_eq!(keypad(b"\x1b="), KeypadMode::Application);
    assert_eq!(keypad(b"\x1b=\x1b>"), KeypadMode::Numeric);
}

/// It survives surrounding text, so it is not being confused with a printable.
#[test]
fn the_designator_is_consumed_not_printed() {
    let ws = support::CORPUS;
    let mut s = Owned::new(&ws);
    let mut t = Term::new(s.storage(), &ws);
    t.write_at(BytePos::ZERO, b"ab\x1b=cd");
    t.feed(BytePos::ZERO, ReadLen::from_index((b"ab\x1b=cd").len()));
    assert_eq!(t.keypad(), KeypadMode::Application);
    assert_eq!(t.cursor().1.index(), 4, "abcd is four columns, not five");
}

/// RIS returns the keypad to numeric, so a crashed program cannot leave the
/// next one's arrow keys sending the wrong sequences.
#[test]
fn ris_restores_the_numeric_keypad() {
    assert_eq!(keypad(b"\x1b=\x1bc"), KeypadMode::Numeric);
}

/// Both feeds agree, whatever the chunking.
#[test]
fn keypad_mode_is_chunk_invariant() {
    let seq = b"text\x1b=more\x1b>tail";
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
        assert_eq!(t.keypad(), KeypadMode::Numeric, "at chunk {chunk}");
    }
}

// ── XTVERSION ───────────────────────────────────────────────────────────────

/// `CSI > 0 q` answers with a DCS, not a CSI.
///
/// The shape matters more than the text: an application parses whatever
/// arrives as the report it asked for, and this is the one report baud sends
/// that is not a CSI.
#[test]
fn xtversion_answers_with_a_dcs() {
    let r = replies(b"\x1b[>0q");
    assert_eq!(&r[..4], b"\x1bP>|", "DCS > |");
    assert_eq!(&r[r.len() - 2..], b"\x1b\\", "terminated by ST");
    assert!(r.len() > 6, "and it names something");
}

/// The text is the name and then the version, `baud(<workspace version>)`.
///
/// `[[source: ctlseqs]]` asks for both ("Report xterm name and version"),
/// and a bare `baud` identified no version. The form is xterm's own reply,
/// `XTerm(411)` (`[[source: xterm-version]]`, `sprintf("%.80s(%d)", ...)`
/// behind `CASE_REPORT_VERSION` in `[[source: xterm-sgr]]`), and foot's
/// (`[[source: foot-readme]]`); the version is the workspace's, which every
/// crate inherits, so this and `TERM_PROGRAM_VERSION` cannot drift apart.
#[test]
fn xtversion_names_baud_and_its_version() {
    let want = concat!("\x1bP>|baud(", env!("CARGO_PKG_VERSION"), ")\x1b\\");
    assert_eq!(replies(b"\x1b[>0q"), want.as_bytes());
}

/// **What the embedder tells, XTVERSION names**: the same shape,
/// `DCS > | baud(<text>) ST`, with build metadata in the version, at every
/// read size and every two-read split of the query. The text is longer than
/// `Report::MAX_LEN` would let `Report::Version.encode` carry, so a told text
/// that went through it would lose its `ST`. Perturbation: answer with
/// `VersionText::PACKAGE` in `Paint::want_reply` and this fails.
#[test]
fn xtversion_names_what_the_embedder_tells() {
    const TOLD: &str = "1.0.0-beta.1+0123abc.dirty.and.a.longer.tail";
    static TEXT: VersionText = match VersionText::new(TOLD) {
        Some(t) => t,
        None => panic!("printable, and within the bound"),
    };
    let want = format!("\x1bP>|baud({TOLD})\x1b\\");
    assert!(
        want.len() > baud_domain::Report::MAX_LEN,
        "the case: too long to encode"
    );
    let query = b"\x1b[>0q";
    for cut in 0..=query.len() {
        let ws = support::CORPUS;
        let mut s = Owned::new(&ws);
        let mut t = Term::new(s.storage(), &ws);
        t.set_version_text(&TEXT);
        support::feed(&mut t, &query[..cut]);
        support::feed(&mut t, &query[cut..]);
        assert_eq!(t.reply(), want.as_bytes(), "split at {cut}");
    }
    let ws = support::CORPUS;
    let mut s = Owned::new(&ws);
    let mut t = Term::new(s.storage(), &ws);
    t.set_version_text(&TEXT);
    for b in query {
        support::feed(&mut t, core::slice::from_ref(b));
    }
    assert_eq!(t.reply(), want.as_bytes(), "a byte at a time");
}

/// **A reset does not change what the build is**: `RIS` and `DECSTR` keep the
/// text told, as they keep the `TERM` XTGETTCAP names.
#[test]
fn a_reset_keeps_the_version_told() {
    const TOLD: &str = "1.0.0-beta.1+abc1234";
    static TEXT: VersionText = match VersionText::new(TOLD) {
        Some(t) => t,
        None => panic!("told"),
    };
    let ws = support::CORPUS;
    let want = format!("\x1bP>|baud({TOLD})\x1b\\");
    for reset in [&b"\x1bc"[..], &b"\x1b[!p"[..]] {
        let mut s = Owned::new(&ws);
        let mut t = Term::new(s.storage(), &ws);
        t.set_version_text(&TEXT);
        support::feed(&mut t, reset);
        t.clear_reply();
        support::feed(&mut t, b"\x1b[>q");
        assert_eq!(t.reply(), want.as_bytes(), "after {reset:?}");
    }
}

/// **Nothing told can end the control string early**, and nothing past the
/// bound is taken: ESC would cancel it (`[[source: williams]]`), a space or
/// DEL has no place in a version, and xterm keeps 80 bytes of its text
/// (`[[source: xterm-version]]`, `sprintf("%.80s(%d)", ...)`, line 72). The
/// positive controls are the two limits.
#[test]
fn a_version_text_is_printable_and_bounded() {
    for bad in ["", "1.0\x1b\\", "1.0 beta", "1.0\x7f", "1.0\u{e9}", "1\n"] {
        assert_eq!(VersionText::new(bad), None, "{bad:?}");
    }
    const AT: &str =
        "01234567890123456789012345678901234567890123456789012345678901234567890123456789";
    assert_eq!(AT.len(), VersionText::MAX);
    assert_eq!(VersionText::new(AT).map(VersionText::as_str), Some(AT));
    const PAST: &str =
        "012345678901234567890123456789012345678901234567890123456789012345678901234567890";
    assert_eq!(VersionText::new(PAST), None);
    assert_eq!(VersionText::new("!~").map(VersionText::as_str), Some("!~"));
}

/// **A told text at the bound still goes whole or not at all**: into a full
/// buffer it is dropped and counted, as every report is.
#[test]
fn a_reply_with_no_room_is_counted_not_cut() {
    const AT: &str =
        "01234567890123456789012345678901234567890123456789012345678901234567890123456789";
    let ws = support::CORPUS;
    let mut s = Owned::new(&ws);
    let mut t = Term::new(s.storage(), &ws);
    static TEXT: VersionText = match VersionText::new(AT) {
        Some(t) => t,
        None => panic!("at the bound"),
    };
    t.set_version_text(&TEXT);
    let before = t.reply_overflows;
    let many = b"\x1b[>q".repeat(baud_core::REPLY);
    support::feed(&mut t, &many);
    let reply = t.reply().to_vec();
    assert!(t.reply_overflows > before, "the buffer filled");
    let one = TEXT.reply_len();
    assert_eq!(reply.len() % one, 0, "whole replies only");
    assert!(reply.ends_with(b"\x1b\\"), "and the last one closed");
}

/// An omitted parameter is 0, so a bare `CSI > q` is still XTVERSION.
#[test]
fn xtversion_defaults_to_zero() {
    assert_eq!(replies(b"\x1b[>q"), replies(b"\x1b[>0q"));
}

/// A `CSI > Ps q` baud does not implement gets no answer.
#[test]
fn an_unknown_secondary_query_is_not_answered() {
    assert!(replies(b"\x1b[>5q").is_empty());
}

/// The `>` namespace is not the `?` namespace.
///
/// `CSI ? 0 q` is unrelated to XTVERSION, and answering it with a version
/// string would be the wrong-shaped reply.
#[test]
fn the_secondary_marker_is_its_own_namespace() {
    assert!(replies(b"\x1b[?0q").is_empty());
    assert!(!replies(b"\x1b[>0q").is_empty());
}

/// `CSI > 0 q` must not be mistaken for DECSCUSR or DECLL, which also end `q`.
#[test]
fn xtversion_does_not_collide_with_other_q_finals() {
    let ws = support::CORPUS;
    let mut s = Owned::new(&ws);
    let mut t = Term::new(s.storage(), &ws);
    // A DECSCUSR before it must still apply, and must not answer.
    support::feed(&mut t, b"\x1b[5 q");
    assert_eq!(t.cursor_style(), CursorStyle::BlinkingBar);
    assert!(t.reply().is_empty(), "DECSCUSR is not a query");
}

/// The reply parses the same however the read boundaries fall.
#[test]
fn xtversion_is_chunk_invariant() {
    let seq = b"\x1b[>0q";
    let whole = replies(seq);
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
