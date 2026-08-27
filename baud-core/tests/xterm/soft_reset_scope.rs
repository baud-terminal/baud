//! What `DECSTR` resets, and what it must leave alone.
//!
//! `CSI ! p` is a DEC function, but two of the things baud used to reset with
//! it — the alternate screen and mouse tracking — are **xterm extensions**. A
//! DEC manual's list of what DECSTR resets cannot mention them, so xterm is
//! the authority, and in `ReallyReset` both sit inside the `if (full)` branch:
//! `FromAlternate` next to `ClearScreen`, and `send_mouse_pos = MOUSE_OFF`
//! next to `extend_coords = 0`. Nothing mouse-related happens on the soft path
//! at all.
//!
//! baud reset all four, so a full-screen application sending `CSI ! p` to
//! recover a confused terminal was dropped onto the primary screen mid-draw,
//! and the primary's saved contents went with it (`alt_saved = None`).
//!
//! Found by `the_alt_screen_never_leaks_into_the_primary` once the generator
//! could emit `CSI ! p` at all — it shrank to `[Decstr, Sgr(1), El(0), ...]`.
//! DECSTR is a `done` row and had been for a long time; what was missing was
//! an input shape, not an implementation.

use baud_core::*;

fn run(bytes: &[u8]) -> (Vec<PackedCell>, bool) {
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
    let alt = t.screen() == Screen::Alternate;
    (t.viewport().to_vec(), alt)
}

/// **DECSTR does not leave the alternate screen.**
#[test]
fn soft_reset_stays_on_the_alternate_screen() {
    let (_, alt) = run(b"\x1b[?1049h\x1b[!p");
    assert!(alt, "CSI ! p must not switch back to the primary screen");
}

/// And the primary's contents survive it.
///
/// This is the part that made the old behaviour destructive rather than merely
/// wrong: `alt_saved = None` discarded what `?1049 h` had saved, so leaving
/// the alt screen afterwards could not restore the primary.
#[test]
fn the_primary_screen_survives_a_soft_reset_on_the_alternate() {
    let baseline = run(b"PRIMARY").0;
    let after = run(b"PRIMARY\x1b[?1049h\x1b[!pALT\x1b[?1049l").0;
    assert_eq!(
        after, baseline,
        "the primary must come back exactly as it was"
    );
}

/// **DECSTR does not reset mouse tracking or its encoding.**
///
/// `send_mouse_pos` and `extend_coords` are both RIS-only in xterm. An
/// application that enables SGR mouse reporting and later soft-resets would
/// otherwise stop receiving the events it is still waiting for.
#[test]
fn soft_reset_leaves_mouse_tracking_alone() {
    let (tracking, reply) = mouse_state(b"\x1b[?1003h\x1b[?1006h\x1b[!p");
    assert_ne!(
        tracking,
        MouseTracking::Off,
        "CSI ! p must not turn mouse reporting off"
    );
    // DECRQM is the observable channel for the encoding: `1` means set.
    assert_eq!(
        reply,
        b"\x1b[?1006;1$y".to_vec(),
        "nor revert the encoding to X10"
    );
}

/// Drive `bytes`, then ask DECRQM about `?1006`.
fn mouse_state(bytes: &[u8]) -> (MouseTracking, Vec<u8>) {
    let mut s = Storage::boxed();
    let mut t = Term::new(&mut s, OpMode::Computed);
    for chunk in [bytes, &b"\x1b[?1006$p"[..]] {
        let mut slot = t.read_slot();
        let base = slot.base();
        let f = slot.write(StreamByte::slice_from(chunk));
        t.feed(base, f.len());
        t.commit_read(f.len());
    }
    (t.mouse_tracking(), t.reply().to_vec())
}

/// RIS still resets all of it — that is the difference between the two.
#[test]
fn a_hard_reset_still_resets_everything() {
    let (_, alt) = run(b"\x1b[?1049h\x1bc");
    assert!(!alt, "RIS returns to the primary screen");

    let (tracking, reply) = mouse_state(b"\x1b[?1003h\x1b[?1006h\x1bc");
    assert_eq!(tracking, MouseTracking::Off, "RIS clears tracking");
    assert_eq!(reply, b"\x1b[?1006;2$y".to_vec(), "and the encoding");
}
