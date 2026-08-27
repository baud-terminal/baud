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
