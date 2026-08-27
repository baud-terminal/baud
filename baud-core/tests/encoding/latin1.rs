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

use baud_core::*;

fn run(bytes: &[u8]) -> (Vec<u32>, Vec<u8>) {
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

/// `80..9F` are C1 controls in ISO 8859-1, not printable.
///
/// baud does not act on 8-bit C1 — it cannot, having chosen UTF-8 as the other
/// mode — so they are ignored rather than printed. That is xterm's
/// `CASE_IGNORE`, and what ghostty does citing xterm.
#[test]
fn latin1_c1_bytes_are_ignored_not_printed() {
    assert_eq!(run(b"\x1b%@\x9bA").0, vec![0x41], "the 0x9B is dropped");
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
    let mut s = Storage::boxed();
    let mut t = Term::new(&mut s, OpMode::Computed);
    let mut slot = t.read_slot();
    let base = slot.base();
    let f = slot.write(StreamByte::slice_from(b"caf\xff"));
    t.feed(base, f.len());
    t.commit_read(f.len());
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
