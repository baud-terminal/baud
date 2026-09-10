//! `ESC % @` / `ESC % G` — switching the input encoding mid-stream.
//!
//! `[[source: ctlseqs]]`: `ESC % @` is "Select default character set. That is
//! ISO 8859-1 (ISO 2022)" and `ESC % G` is "Select UTF-8 character set, ISO
//! 2022". An application may switch at any point, so this is live state.
//!
//! baud refused `ESC % @` until now, on the grounds that it "cannot leave
//! UTF-8". It can: the two encodings differ only in how bytes above `0x7F` are
//! read, and a single-byte encoding needs no decoder state at all.
//!
//! xterm's `utf8` resource is four-valued, and only an explicitly configured
//! `always` makes these sequences inert. By default xterm honours them.

use crate::support;
use baud_core::*;

fn run(bytes: &[u8]) -> (Vec<u32>, Vec<u8>) {
    let ws = support::CORPUS;
    let mut s = Owned::new(&ws);
    let mut t = Term::new(s.storage(), &ws);
    support::feed(&mut t, bytes);
    let cols = t.cursor().1.index();
    let cells = t.viewport()[..cols]
        .iter()
        .filter(|c| !c.content.is_wide_tail())
        .map(|c| c.content.as_codepoint().map_or(0, |c| c.value()))
        .collect();
    (cells, t.reply().to_vec())
}

/// **The same bytes mean different characters in the two encodings.**
///
/// `0xC3 0xA9` is `é` in UTF-8 — one character — and `Ã©` in ISO 8859-1, which
/// is two. That is the whole difference, and it is why an encoding a terminal
/// gets wrong is not a cosmetic problem.
#[test]
fn the_same_bytes_decode_differently_per_encoding() {
    assert_eq!(run(b"\xc3\xa9").0, vec![0xe9], "UTF-8: one `é`");
    assert_eq!(
        run(b"\x1b%@\xc3\xa9").0,
        vec![0xc3, 0xa9],
        "ISO 8859-1: `Ã` then `©`"
    );
}

/// `ESC % G` switches back, mid-stream.
#[test]
fn the_encoding_switches_both_ways() {
    assert_eq!(
        run(b"\x1b%@\xe9\x1b%G\xc3\xa9").0,
        vec![0xe9, 0xe9],
        "Latin-1 `é`, then UTF-8 `é`"
    );
}

/// Every Latin-1 byte above `0x9F` is a character, including ones UTF-8 would
/// reject as a lead byte.
#[test]
fn latin1_has_no_ill_formed_bytes() {
    // 0xFF is never a valid UTF-8 lead byte; in Latin-1 it is `ÿ`.
    assert_eq!(run(b"\x1b%@\xff").0, vec![0xff], "0xFF is `ÿ`");
    assert_eq!(run(b"\xff").0, vec![0xfffd], "and is ill-formed in UTF-8");
}

/// `80..9F` are C1 controls in ISO 8859-1, and are **acted on**, not printed.
///
/// This asserted they were ignored, on the reading that baud "cannot act on
/// 8-bit C1, having chosen UTF-8 as the other mode". True in UTF-8 and not
/// here: in Latin-1 there is no multi-byte sequence for `9B` to be part of.
///
/// `9B A` is `CSI A` — cursor up — so nothing prints and the cursor does not
/// move, being already at the top.
#[test]
fn latin1_c1_bytes_are_controls_not_characters() {
    assert_eq!(
        run(b"\x1b%@\x9bA").0,
        Vec::<u32>::new(),
        "CSI A prints nothing"
    );
    assert_eq!(
        run(b"\x1b%@\x9bA").0,
        run(b"\x1b%@\x1b[A").0,
        "and matches the 7-bit form"
    );
}

/// **DECRQM `?1020` reports the live encoding.**
///
/// It answered `3` (permanently set) while `ESC % @` was refused, and that was
/// true then. Implementing the switch made it false, so it moved in the same
/// commit — a mode report that outlives the fact it reported is exactly the
/// failure `docs/vt-coverage.md` refuses. xterm answers `MdBool` from the live
/// `screen->utf8_mode` for the same reason.
#[test]
fn decrqm_1020_follows_the_live_encoding() {
    assert_eq!(run(b"\x1b[?1020$p").1, b"\x1b[?1020;1$y", "UTF-8: set");
    assert_eq!(
        run(b"\x1b%@\x1b[?1020$p").1,
        b"\x1b[?1020;2$y",
        "Latin-1: reset"
    );
}

// ── detection: reporting, not guessing ──────────────────────────────────────

/// **Pure ASCII is undecidable, and saying so is the point.**
///
/// UTF-8 and ISO 8859-1 agree on every byte below `0x80`, so a sample of only
/// ASCII carries no evidence either way. Answering anyway would be inventing
/// some — and this is the *common* case for a terminal, which is precisely why
/// detection cannot be a default and why baud reports rather than switches.
#[test]
fn ascii_alone_decides_nothing() {
    assert_eq!(InputEncoding::detect(b"hello world"), None);
    assert_eq!(InputEncoding::detect(b""), None);
    assert_eq!(InputEncoding::detect(b"\x1b[1;2H plain"), None);
}

/// A well-formed multi-byte sequence is evidence of UTF-8.
#[test]
fn a_multibyte_sequence_indicates_utf8() {
    assert_eq!(
        InputEncoding::detect("café".as_bytes()),
        Some(InputEncoding::Utf8)
    );
    assert_eq!(
        InputEncoding::detect("中文".as_bytes()),
        Some(InputEncoding::Utf8)
    );
}

/// A byte-order mark is a signature and decides on its own.
#[test]
fn a_bom_is_decisive() {
    assert_eq!(
        InputEncoding::detect(b"\xef\xbb\xbf"),
        Some(InputEncoding::Utf8),
        "the BOM alone, with no other evidence"
    );
}

/// Anything ill-formed as UTF-8 rules UTF-8 out.
///
/// ISO 8859-1 has no ill-formed bytes — all 256 are characters — so it is what
/// remains rather than a guess.
#[test]
fn ill_formed_utf8_indicates_latin1() {
    // 0xFF is never a UTF-8 lead byte; in Latin-1 it is `ÿ`.
    assert_eq!(
        InputEncoding::detect(b"caf\xff"),
        Some(InputEncoding::Latin1)
    );
    // A lone continuation byte.
    assert_eq!(
        InputEncoding::detect(b"a\x80b"),
        Some(InputEncoding::Latin1)
    );
    // An overlong, which is a legal scalar value wrongly encoded — the case
    // `Codepoint::new` structurally cannot see.
    assert_eq!(
        InputEncoding::detect(b"\xe0\x80\xaf"),
        Some(InputEncoding::Latin1)
    );
    // A surrogate.
    assert_eq!(
        InputEncoding::detect(b"\xed\xa0\x80"),
        Some(InputEncoding::Latin1)
    );
}

/// A sequence cut off by the end of the sample is not evidence.
///
/// The next read may complete it. Treating a truncated lead as ill-formed would
/// make detection depend on where the read boundary fell, which is the
/// chunk-invariance failure this project has had before.
#[test]
fn a_truncated_tail_is_not_evidence() {
    assert_eq!(
        InputEncoding::detect(b"ab\xc3"),
        None,
        "a lead byte at the very end decides nothing"
    );
    assert_eq!(
        InputEncoding::detect("中".as_bytes()),
        Some(InputEncoding::Utf8),
        "but a complete one does"
    );
}

/// Detection never runs on its own.
///
/// `Term` does not call `detect`, and feeding bytes that look like Latin-1 does
/// not switch the encoding. An embedder decides, because a wrong guess corrupts
/// text silently and gives the application no way to find out.
#[test]
fn detection_does_not_switch_the_terminal() {
    let ws = support::CORPUS;
    let mut s = Owned::new(&ws);
    let mut t = Term::new(s.storage(), &ws);
    t.write_at(BytePos::ZERO, b"caf\xff");
    t.feed(BytePos::ZERO, ReadLen::from_index((b"caf\xff").len()));
    // Still UTF-8: the 0xFF is a decode error, not a hint.
    assert_eq!(
        t.reply().len(),
        0,
        "no reply, and nothing switched behind the caller's back"
    );
    let cols = t.cursor().1.index();
    let last = t.viewport()[..cols]
        .last()
        .and_then(|c| c.content.as_codepoint())
        .map(|c| c.value());
    assert_eq!(last, Some(0xfffd), "0xFF is U+FFFD under UTF-8, not `ÿ`");
}

// ── 8-bit C1, which only exists outside UTF-8 ───────────────────────────────

/// **`9B` is CSI in ISO 8859-1, and a decode error in UTF-8.**
///
/// `docs/vt-coverage.md` lists 8-bit C1 introducers as out, because "they are
/// UTF-8 continuation bytes". True, and scoped to the encoding without saying
/// so: baud is not always in UTF-8 any more. In Latin-1 there is no ambiguity
/// — `9B` cannot be part of a multi-byte sequence, because there are none.
///
/// `[[source: ctlseqs]]` calls `ESC [` "a 7-bit equivalent to 8-bit" `9B`, so
/// the two are the same operation; the implementation dispatches through the
/// `Escape` state rather than duplicating its table, which is what stops them
/// drifting apart.
#[test]
fn eight_bit_csi_works_in_latin1_and_not_in_utf8() {
    // `9B 3 1 m` is `CSI 3 1 m` — set foreground red — then `X`.
    let latin1 = run(b"\x1b%@\x9b31mX");
    let seven_bit = run(b"\x1b%@\x1b[31mX");
    assert_eq!(latin1.0, seven_bit.0, "8-bit CSI matches the 7-bit form");

    // The same bytes in UTF-8: `9B` is a stray continuation byte.
    let utf8 = run(b"\x9b31mX");
    assert_eq!(
        utf8.0.first().copied(),
        Some(0xfffd),
        "in UTF-8 it is a decode error, not a control"
    );
}

/// The other introducers reach their string states.
#[test]
fn eight_bit_osc_and_dcs_work_in_latin1() {
    // `9D` is OSC: set the title, print nothing.
    let ws = support::CORPUS;
    let mut s = Owned::new(&ws);
    let mut t = Term::new(s.storage(), &ws);
    t.write_at(BytePos::ZERO, b"\x1b%@\x9d0;hi\x07");
    t.feed(
        BytePos::ZERO,
        ReadLen::from_index((b"\x1b%@\x9d0;hi\x07").len()),
    );
    assert_eq!(t.title(), b"hi", "8-bit OSC set the title");
    assert_eq!(t.cursor().1.index(), 0, "and printed nothing");
}

/// `9C` is ST, and `98`/`9E`/`9F` open strings that are consumed.
#[test]
fn eight_bit_string_terminators_are_honoured() {
    // `90` DCS ... `9C` ST: the payload must not reach the screen.
    let g = run(b"\x1b%@\x90q payload \x9cX");
    assert_eq!(
        g.0,
        run(b"\x1b%@X").0,
        "the DCS payload is consumed, only the X prints"
    );
}
