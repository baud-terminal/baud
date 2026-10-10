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

use crate::support;
use baud_core::*;

struct T {
    s: Owned,
}

impl T {
    fn new() -> T {
        T {
            s: Owned::new(&support::CORPUS),
        }
    }
}

fn run(t: &mut T, bytes: &[u8]) -> (Vec<PackedCell>, (usize, usize), Screen) {
    let mut term = Term::new(t.s.storage(), &support::CORPUS);
    support::feed(&mut term, bytes);
    let (r, c) = term.cursor();
    (
        support::screen(&term),
        (r.index(), c.index()),
        term.screen(),
    )
}

fn grid(bytes: &[u8]) -> Vec<PackedCell> {
    run(&mut T::new(), bytes).0
}

fn cursor(bytes: &[u8]) -> (usize, usize) {
    run(&mut T::new(), bytes).1
}

fn on_alt(bytes: &[u8]) -> bool {
    run(&mut T::new(), bytes).2 == Screen::Alternate
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
    let ws = support::CORPUS;
    let mut s = Owned::new(&ws);
    let mut t = Term::new(s.storage(), &ws);
    let feed = |t: &mut Term, b: &[u8]| {
        t.write_at(BytePos::ZERO, b);
        t.feed(BytePos::ZERO, ReadLen::from_index((b).len()));
    };
    feed(&mut t, b"\x1b[?1049$p");
    assert_eq!(t.reply(), b"\x1b[?1049;2$y", "implemented, currently reset");
    t.clear_reply();
    feed(&mut t, b"\x1b[?1049h\x1b[?1049$p");
    assert_eq!(t.reply(), b"\x1b[?1049;1$y");
}

/// **What is written on the alternate screen is what it shows.**
///
/// `[[source: ctlseqs]]`, `?1049 h`: "switch to the Alternate Screen Buffer,
/// clearing it first." So `abc` written after the switch is the alternate
/// screen's whole first row -- the three characters, then blanks where the
/// primary had `hello world`. The text goes to an explicit `CSI 1;1H` rather
/// than wherever the switch leaves the cursor, and the primary has text of its
/// own, so a screen that showed the wrong buffer reads as `hello world` and
/// one that showed nothing reads as blanks, rather than either passing.
///
/// Blanks are rendered as `.` (`Content::EMPTY` is not U+0020), so each of
/// the eleven columns `hello world` held is compared, not only the three that
/// hold text.
///
/// It replaced a test that compared `baud-core2`'s `OpMode::Runs` with its
/// `Computed` on the same bytes. With one mode the two sides were one terminal
/// built twice, and that comparison could not fail.
#[test]
fn the_alt_screen_shows_what_is_written_on_it() {
    let (cells, _, screen) = run(&mut T::new(), b"hello world\x1b[?1049h\x1b[1;1Habc");
    assert_eq!(
        screen,
        Screen::Alternate,
        "the control: the switch happened"
    );
    let shown: String = cells[..11]
        .iter()
        .map(|c| {
            c.content
                .as_codepoint()
                .and_then(|x| char::from_u32(x.value()))
                .unwrap_or('.')
        })
        .collect();
    assert_eq!(
        shown, "abc........",
        "the alternate screen's first row is what was written on it"
    );
}

/// **Leaving the alternate screen brings the primary back as it was.**
///
/// The shape of `the_primary_screen_is_restored_intact`: enter, write and
/// leave once, then compare the whole screen with a terminal that never
/// switched. The name is `baud-core2`'s, where this ran in its `OpMode::Runs`;
/// it is kept because `docs/record.md` cites the test by it. Nothing scrolls
/// the primary here, so a restore that dropped the primary's rotation would
/// pass, as the record says of this test.
#[test]
fn runs_mode_restores_the_primary_screen() {
    let mut a = T::new();
    let before = run(&mut a, b"primary text").0;
    let mut b = T::new();
    let after = run(&mut b, b"primary text\x1b[?1049halt text\x1b[?1049l").0;
    assert_eq!(after, before);
}
