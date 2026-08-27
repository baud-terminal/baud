//! The alternate screen buffer, `CSI ? 1049`.
//!
//! `[[source: ctlseqs]]`, `?1049 h`: "Save cursor as in DECSC, xterm. After
//! saving the cursor, switch to the Alternate Screen Buffer, clearing it
//! first." And `?1049 l`: "Use Normal Screen Buffer and restore cursor as in
//! DECRC."
//!
//! Every full-screen program -- vim, less, htop, tmux -- switches on start and
//! back on exit. Without it, quitting vim does not restore the shell's
//! scrollback, which reads to a user as lost session history rather than as a
//! missing feature.
//!
//! The property that makes it worth a whole second buffer, and the one most of
//! these tests are really about: **the primary screen must come back
//! unchanged.**

use baud_core::*;

struct T {
    s: Box<Storage>,
}

impl T {
    fn new() -> T {
        T {
            s: Storage::boxed(),
        }
    }
}

fn run(t: &mut T, mode: OpMode, bytes: &[u8]) -> (Vec<PackedCell>, (usize, usize), Screen) {
    let mut term = Term::new(&mut t.s, mode);
    let mut i = 0;
    while i < bytes.len() {
        let n = READ_CHUNK.min(bytes.len() - i);
        let mut slot = term.read_slot();
        let base = slot.base();
        let f = slot.write(StreamByte::slice_from(&bytes[i..i + n]));
        term.feed(base, f.len());
        term.commit_read(f.len());
        i += n;
    }
    let (r, c) = term.cursor();
    (
        term.viewport().to_vec(),
        (r.index(), c.index()),
        term.screen(),
    )
}

fn grid(bytes: &[u8]) -> Vec<PackedCell> {
    run(&mut T::new(), OpMode::Computed, bytes).0
}

fn cursor(bytes: &[u8]) -> (usize, usize) {
    run(&mut T::new(), OpMode::Computed, bytes).1
}

fn on_alt(bytes: &[u8]) -> bool {
    run(&mut T::new(), OpMode::Computed, bytes).2 == Screen::Alternate
}

/// Switching in and out is observable.
#[test]
fn the_switch_is_observable() {
    assert!(!on_alt(b""), "power-on is the normal screen");
    assert!(on_alt(b"\x1b[?1049h"));
    assert!(!on_alt(b"\x1b[?1049h\x1b[?1049l"));
}

/// The alternate screen starts blank even though the primary had content.
///
/// `[[source: ctlseqs]]`: "switch to the Alternate Screen Buffer, clearing it
/// first."
#[test]
fn the_alt_screen_starts_cleared() {
    assert_eq!(
        grid(b"hello world\x1b[?1049h"),
        grid(b"\x1b[?1049h"),
        "nothing from the primary screen may show through"
    );
}

/// **The point of the whole feature**: the primary screen survives untouched.
#[test]
fn the_primary_screen_is_restored_intact() {
    let before = grid(b"important scrollback");
    let after = grid(b"important scrollback\x1b[?1049h junk on the alt \x1b[?1049l");
    assert_eq!(after, before, "the primary screen must come back unchanged");
}

/// Including when the alt screen scrolled, which is where a shared buffer would
/// destroy history rather than merely overwrite a line.
#[test]
fn scrolling_the_alt_screen_does_not_disturb_the_primary() {
    let before = grid(b"line one\r\nline two\r\nline three");
    let mut v = b"line one\r\nline two\r\nline three\x1b[?1049h".to_vec();
    // Fill and scroll the alt screen well past its bottom.
    for i in 0..900 {
        v.extend_from_slice(format!("alt row {i}\r\n").as_bytes());
    }
    v.extend_from_slice(b"\x1b[?1049l");
    assert_eq!(grid(&v), before);
}

/// The cursor is saved on the way in and restored on the way out.
#[test]
fn the_cursor_is_saved_and_restored() {
    assert_eq!(
        cursor(b"\x1b[9;20H\x1b[?1049h\x1b[1;1Hx\x1b[?1049l"),
        (8, 19),
        "back where it was before the switch"
    );
}

/// Entering homes the cursor: the alt screen is cleared, so leaving the cursor
/// mid-screen would put the first write of a full-screen program somewhere
/// arbitrary.
#[test]
fn entering_homes_the_cursor() {
    assert_eq!(cursor(b"\x1b[9;20H\x1b[?1049h"), (0, 0));
}

/// A second `?1049 h` must not overwrite the saved cursor.
///
/// An application that sets the mode twice -- or a shell prompt and a pager
/// that both do -- would otherwise be restored to a position from inside the
/// alt screen instead of the one it started from.
#[test]
fn entering_twice_does_not_clobber_the_saved_cursor() {
    assert_eq!(
        cursor(b"\x1b[9;20H\x1b[?1049h\x1b[3;3H\x1b[?1049h\x1b[?1049l"),
        (8, 19)
    );
}

/// Leaving when not on the alt screen is inert, not a restore of stale state.
#[test]
fn leaving_when_not_on_the_alt_screen_is_inert() {
    assert_eq!(cursor(b"\x1b[9;20H\x1b[?1049l"), (8, 19));
    assert_eq!(grid(b"abc\x1b[?1049l"), grid(b"abc"));
}

/// The rendition follows the cursor across the switch, as DECSC/DECRC would.
#[test]
fn the_rendition_is_saved_and_restored() {
    assert_eq!(
        grid(b"\x1b[31m\x1b[?1049h\x1b[32m\x1b[?1049lx"),
        grid(b"\x1b[31mx"),
        "the red must come back, not the green set on the alt screen"
    );
}

/// DECSC/DECRC keep their own saved cursor, independent of the switch.
///
/// The switch stashes a cursor and so does DECSC; sharing one slot would make
/// an application that does both get the wrong one back.
#[test]
fn the_switch_does_not_share_decsc_state() {
    // Save at 5;5, switch (which stashes 5;5 separately), restore inside the
    // alt screen -- DECRC must find 5;5, not be confused by the switch.
    assert_eq!(cursor(b"\x1b[5;5H\x1b7\x1b[?1049h\x1b[1;1H\x1b8"), (4, 4));
}

/// RIS returns to the normal screen.
#[test]
fn ris_returns_to_the_normal_screen() {
    assert!(!on_alt(b"\x1b[?1049h\x1bc"));
}

/// DECRQM reports the alt screen as implemented, and tracks it.
#[test]
fn decrqm_reports_the_alt_screen() {
    let mut s = Storage::boxed();
    let mut t = Term::new(&mut s, OpMode::Computed);
    let feed = |t: &mut Term, b: &[u8]| {
        let mut slot = t.read_slot();
        let base = slot.base();
        let f = slot.write(StreamByte::slice_from(b));
        t.feed(base, f.len());
        t.commit_read(f.len());
    };
    feed(&mut t, b"\x1b[?1049$p");
    assert_eq!(t.reply(), b"\x1b[?1049;2$y", "implemented, currently reset");
    t.clear_reply();
    feed(&mut t, b"\x1b[?1049h\x1b[?1049$p");
    assert_eq!(t.reply(), b"\x1b[?1049;1$y");
}

/// The alt screen works the same in the span-storage mode.
///
/// `OpMode::Runs` keeps lines as runs and promotes them on demand, but the alt
/// screen has no run table -- writes there are materialised directly. If that
/// were wrong, the alt screen would render blank in Runs mode while looking
/// correct in Computed.
#[test]
fn the_alt_screen_renders_in_runs_mode() {
    let seq = b"\x1b[?1049habc";
    let computed = run(&mut T::new(), OpMode::Computed, seq).0;
    let runs = run(&mut T::new(), OpMode::Runs, seq).0;
    assert_eq!(runs, computed, "Runs mode must materialise the alt screen");
}

/// And leaving Runs mode's alt screen still restores the primary.
#[test]
fn runs_mode_restores_the_primary_screen() {
    let mut a = T::new();
    let before = run(&mut a, OpMode::Runs, b"primary text").0;
    let mut b = T::new();
    let after = run(
        &mut b,
        OpMode::Runs,
        b"primary text\x1b[?1049halt text\x1b[?1049l",
    )
    .0;
    assert_eq!(after, before);
}
