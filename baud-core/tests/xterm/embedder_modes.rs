//! Modes the core records because something outside it can act on them.
//!
//! `CONTRIBUTING.md`: "Never record a mode you do not act on. A bit that is set
//! and never read is the same failure as a sequence parsed and dropped." The
//! core has no renderer, and its input path encodes keys rather than acting on
//! these, so for these two "acting on it" means *exposing* it -- an embedder
//! cannot implement either mode without being told, and there is no other
//! channel that could tell it.
//!
//! Bracketed paste is the one that matters for safety rather than looks:
//! without it, pasted text containing a newline is delivered to the shell as
//! typed input and runs. That is the paste-jacking class.

use crate::support;
use baud_core::*;

fn term(bytes: &[u8]) -> (bool, bool) {
    let ws = support::CORPUS;
    let mut s = Owned::new(&ws);
    let mut t = Term::new(s.storage(), &ws);
    support::feed(&mut t, bytes);
    (t.bracketed_paste(), t.synchronized_output())
}

/// Both are off on a fresh terminal: an application must ask.
#[test]
fn both_are_reset_at_power_on() {
    assert_eq!(term(b""), (false, false));
}

/// `?2004` toggles bracketed paste and nothing else.
#[test]
fn bracketed_paste_toggles() {
    assert_eq!(term(b"\x1b[?2004h"), (true, false));
    assert_eq!(term(b"\x1b[?2004h\x1b[?2004l"), (false, false));
}

/// `?2026` toggles synchronised output and nothing else.
#[test]
fn synchronized_output_toggles() {
    assert_eq!(term(b"\x1b[?2026h"), (false, true));
    assert_eq!(term(b"\x1b[?2026h\x1b[?2026l"), (false, false));
}

/// They are independent bits, not one flag.
#[test]
fn the_two_modes_are_independent() {
    assert_eq!(term(b"\x1b[?2004h\x1b[?2026h"), (true, true));
    assert_eq!(term(b"\x1b[?2004h\x1b[?2026h\x1b[?2004l"), (false, true));
}

/// Setting them does not disturb the modes that were already there.
#[test]
fn they_do_not_collide_with_other_modes() {
    let ws = support::CORPUS;
    let mut s = Owned::new(&ws);
    let mut t = Term::new(s.storage(), &ws);
    t.write_at(BytePos::ZERO, b"\x1b[?2004h\x1b[?2026h");
    t.feed(
        BytePos::ZERO,
        ReadLen::from_index((b"\x1b[?2004h\x1b[?2026h").len()),
    );
    assert!(t.cursor_visible(), "DECTCEM must be untouched");
}

/// RIS clears them, so a program cannot leave the next one in a held frame or
/// expecting paste brackets it will never send.
#[test]
fn ris_clears_both() {
    assert_eq!(term(b"\x1b[?2004h\x1b[?2026h\x1bc"), (false, false));
}

/// DECSTR releases a held frame and keeps the paste brackets. xterm's soft
/// reset leaves `paste_brackets` alone -- it is cleared only in `ReallyReset`'s
/// `if (full)` branch -- and xterm has no synchronised update, so releasing the
/// frame is baud's own choice: a soft reset is a recovery.
#[test]
fn decstr_releases_the_frame_and_keeps_the_brackets() {
    assert_eq!(
        term(b"\x1b[?2004h\x1b[?2026h"),
        (true, true),
        "the control: both set"
    );
    assert_eq!(term(b"\x1b[?2004h\x1b[?2026h\x1b[!p"), (true, false));
}

/// DECRQM now reports them, because they are implemented.
///
/// This is the pairing that makes the modes honest: an application can ask
/// whether baud supports bracketed paste and get "yes, and it is off" rather
/// than "no such mode".
#[test]
fn decrqm_reports_them_as_implemented() {
    let ws = support::CORPUS;
    let mut s = Owned::new(&ws);
    let mut t = Term::new(s.storage(), &ws);
    t.write_at(BytePos::ZERO, b"\x1b[?2004h\x1b[?2004$p\x1b[?2026$p");
    t.feed(
        BytePos::ZERO,
        ReadLen::from_index((b"\x1b[?2004h\x1b[?2004$p\x1b[?2026$p").len()),
    );
    assert_eq!(t.reply(), b"\x1b[?2004;1$y\x1b[?2026;2$y");
}
