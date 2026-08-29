//! Modes the core records because something outside it can act on them.
//!
//! `CLAUDE.md`: "Never record a mode you do not act on. A bit that is set and
//! never read is the same failure as a sequence parsed and dropped." The core
//! has no input path and no renderer, so for these two "acting on it" means
//! *exposing* it -- an embedder cannot implement either mode without being
//! told, and there is no other channel that could tell it.
//!
//! Bracketed paste is the one that matters for safety rather than looks:
//! without it, pasted text containing a newline is delivered to the shell as
//! typed input and runs. That is the paste-jacking class.

use baud_core2::*;

fn term(bytes: &[u8]) -> (bool, bool) {
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
    let mut s = Storage::boxed();
    let mut t = Term::new(&mut s, OpMode::Computed);
    let mut slot = t.read_slot();
    let base = slot.base();
    let f = slot.write(StreamByte::slice_from(b"\x1b[?2004h\x1b[?2026h"));
    t.feed(base, f.len());
    t.commit_read(f.len());
    assert!(t.cursor_visible(), "DECTCEM must be untouched");
}

/// RIS clears them, so a program cannot leave the next one in a held frame or
/// expecting paste brackets it will never send.
#[test]
fn ris_clears_both() {
    assert_eq!(term(b"\x1b[?2004h\x1b[?2026h\x1bc"), (false, false));
}

/// DECSTR clears them too.
#[test]
fn decstr_clears_both() {
    assert_eq!(term(b"\x1b[?2004h\x1b[?2026h\x1b[!p"), (false, false));
}

/// DECRQM now reports them, because they are implemented.
///
/// This is the pairing that makes the modes honest: an application can ask
/// whether baud supports bracketed paste and get "yes, and it is off" rather
/// than "no such mode".
#[test]
fn decrqm_reports_them_as_implemented() {
    let mut s = Storage::boxed();
    let mut t = Term::new(&mut s, OpMode::Computed);
    let mut slot = t.read_slot();
    let base = slot.base();
    let f = slot.write(StreamByte::slice_from(
        b"\x1b[?2004h\x1b[?2004$p\x1b[?2026$p",
    ));
    t.feed(base, f.len());
    t.commit_read(f.len());
    assert_eq!(t.reply(), b"\x1b[?2004;1$y\x1b[?2026;2$y");
}
