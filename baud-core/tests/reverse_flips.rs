//! `?5` DECSCNM's change counts, [`baud_core::ReverseFlips`].
//!
//! A driver compares a count at two frames. Moved while the mode reads the
//! same at both means the other video came and went with no frame drawing
//! it -- `tput flash` under `xterm-256color` writes `?5h` and `?5l` in one
//! burst after its `$<100/>` (a pty capture, 2026-09-25), so both halves land
//! in one read. A count is the only thing that can say so: the mode, the
//! cells and the renditions all read as they did.
//!
//! There are two: the changes made where a frame could show them
//! (`Term::reverse_flips`), and the changes made inside a synchronised
//! update (`Term::reverse_flips_held`), which the application asked not to be
//! seen -- and which a driver shows only once it has stopped honouring an
//! update left open.
//!
//! A driver only ever compares two readings, so the type offers equality and
//! nothing else, and these tests read it the same way. The ones that take a
//! stream feed it at read sizes 1, 3 and whole and require every size to end
//! on the same counts: they are state, and state that depends on where the
//! reads fell is the class this repo gates.
mod support;
use baud_core::{Owned, ReverseFlips, Term};

/// The two counts, shown and held.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Counts {
    shown: ReverseFlips,
    held: ReverseFlips,
}

fn counts(t: &Term<'_>) -> Counts {
    Counts {
        shown: t.reverse_flips(),
        held: t.reverse_flips_held(),
    }
}

/// The counts before `bytes` and after them, and the mode after, at each read
/// size; every size must end on the same counts.
fn each_read_size(setup: &[u8], bytes: &[u8], f: impl Fn(Counts, Counts, bool)) {
    let mut ends = Vec::new();
    for chunk in [1, 3, bytes.len().max(1)] {
        let mut owned = Owned::new(&support::CORPUS);
        let mut t = Term::new(owned.storage(), &support::CORPUS);
        support::feed(&mut t, setup);
        let before = counts(&t);
        for piece in bytes.chunks(chunk) {
            support::feed(&mut t, piece);
        }
        f(before, counts(&t), t.reverse_video());
        ends.push(counts(&t));
    }
    assert!(
        ends.windows(2).all(|w| w[0] == w[1]),
        "the counts depend on the read size: {ends:?}"
    );
}

/// The shape `tput flash` sends: both halves in one read. The mode reads as
/// it did, so only the count can say the reversed screen existed.
#[test]
fn a_pair_moves_the_count_and_leaves_the_mode() {
    each_read_size(b"", b"\x1b[?5h\x1b[?5l", |before, after, reverse| {
        assert!(!reverse, "the pair ends where it started");
        assert_ne!(before.shown, after.shown, "the reversed screen is counted");
        assert_eq!(before.held, after.held, "and not as held");
    });
    // The control: text moves nothing.
    each_read_size(b"", b"plain text\r\n", |before, after, reverse| {
        assert!(!reverse);
        assert_eq!(before, after);
    });
}

/// Each change moves it once, and a mode set again to what it is does not:
/// two readings a change apart differ, two readings a repeat apart do not.
#[test]
fn each_change_moves_it_and_a_repeat_does_not() {
    let mut owned = Owned::new(&support::CORPUS);
    let mut t = Term::new(owned.storage(), &support::CORPUS);
    let start = t.reverse_flips();
    support::feed(&mut t, b"\x1b[?5h");
    let set = t.reverse_flips();
    support::feed(&mut t, b"\x1b[?5h");
    let set_again = t.reverse_flips();
    support::feed(&mut t, b"\x1b[?5l");
    let reset = t.reverse_flips();
    support::feed(&mut t, b"\x1b[?5l");
    let reset_again = t.reverse_flips();
    assert_ne!(start, set, "a set is a change");
    assert_eq!(set, set_again, "a set over a set is none");
    assert_ne!(set_again, reset, "a reset is a change");
    assert_eq!(reset, reset_again, "a reset over a reset is none");
    assert_ne!(start, reset, "two changes are not none");
}

/// Inside `?2026` the application asked for the frames between not to be
/// seen, so a change there goes in the held count, not the shown one.
#[test]
fn a_change_inside_a_synchronised_update_is_held() {
    each_read_size(
        b"",
        b"\x1b[?2026h\x1b[?5h\x1b[?5l\x1b[?2026l",
        |before, after, reverse| {
            assert!(!reverse);
            assert_eq!(
                before.shown, after.shown,
                "a pair inside ?2026 is not shown"
            );
            assert_ne!(before.held, after.held, "it is held");
        },
    );
    // A change inside that stays is held too. What draws it is the frame
    // after the update, which reads the mode itself.
    each_read_size(
        b"",
        b"\x1b[?2026h\x1b[?5h\x1b[?2026l",
        |before, after, reverse| {
            assert!(reverse, "the control: the mode did change");
            assert_eq!(before.shown, after.shown);
            assert_ne!(before.held, after.held);
        },
    );
}

/// **An update left open is a real state, and a pair inside it is held**,
/// not lost: a program killed between its `?2026h` and `?2026l` leaves one,
/// and a driver that stops honouring it after its bound reads the held count
/// to show what arrived. The whole stream never closes the update.
#[test]
fn a_pair_inside_an_update_left_open_is_held() {
    each_read_size(
        b"\x1b[?2026h",
        b"\x1b[?5h\x1b[?5l",
        |before, after, reverse| {
            assert!(!reverse);
            assert_eq!(before.shown, after.shown);
            assert_ne!(
                before.held, after.held,
                "the held count is the only witness"
            );
        },
    );
}

/// The orders that mix the two: a set before the update and a reset inside
/// it, and the other way round. Each change lands in the count of where it
/// was made, so the pair moves both counts once.
#[test]
fn a_change_is_counted_where_it_was_made() {
    for bytes in [
        &b"\x1b[?5h\x1b[?2026h\x1b[?5l\x1b[?2026l"[..],
        b"\x1b[?2026h\x1b[?5h\x1b[?2026l\x1b[?5l",
    ] {
        each_read_size(b"", bytes, |before, after, reverse| {
            assert!(!reverse, "{bytes:?}");
            assert_ne!(before.shown, after.shown, "{bytes:?}: one shown");
            assert_ne!(before.held, after.held, "{bytes:?}: one held");
        });
    }
}

/// `RIS` returns `?5` to normal through the counted writer, as of *before*
/// the reset: a reset from reverse video is shown, and one issued inside an
/// open update is held -- a reset abandons the update rather than committing
/// it, so a reverse video that only ever existed inside one was never
/// visible.
#[test]
fn a_reset_is_counted_where_it_was_issued() {
    each_read_size(b"\x1b[?5h", b"\x1bc", |before, after, reverse| {
        assert!(!reverse, "RIS restores normal video");
        assert_ne!(before.shown, after.shown);
        assert_eq!(before.held, after.held);
    });
    // Reverse video set inside the update and reset by a RIS inside it: the
    // whole reversed screen was held, and nothing is shown.
    each_read_size(
        b"",
        b"\x1b[?2026h\x1b[?5h\x1bc",
        |before, after, reverse| {
            assert!(!reverse);
            assert_eq!(before.shown, after.shown, "never visible");
            assert_ne!(before.held, after.held);
        },
    );
    // The control: a reset from normal video changes nothing.
    each_read_size(b"", b"\x1bc", |before, after, reverse| {
        assert!(!reverse);
        assert_eq!(before, after);
    });
}

/// **Each opening of an update is counted, and nothing else is**: a set
/// over a set, a close, and a soft reset releasing one move nothing, and a
/// close and a reopen in one read move it -- which is what a driver timing
/// how long it honours an update has to see, or the new update inherits the
/// old one's start.
#[test]
fn an_update_opening_is_counted_once() {
    let mut owned = Owned::new(&support::CORPUS);
    let mut t = Term::new(owned.storage(), &support::CORPUS);
    let start = t.updates_opened();
    support::feed(&mut t, b"\x1b[?2026h");
    let opened = t.updates_opened();
    assert_ne!(start, opened, "an opening");
    support::feed(&mut t, b"\x1b[?2026h");
    assert_eq!(opened, t.updates_opened(), "a set over a set is none");
    support::feed(&mut t, b"\x1b[?2026l\x1b[?2026h");
    let reopened = t.updates_opened();
    assert_ne!(opened, reopened, "a close and a reopen in one read");
    support::feed(&mut t, b"\x1b[?2026l");
    assert_eq!(reopened, t.updates_opened(), "a close is none");
    support::feed(&mut t, b"\x1b[?2026h\x1b[!p");
    let released = t.updates_opened();
    assert_ne!(reopened, released, "the control: it opened");
    assert!(!t.synchronized_output(), "and DECSTR released it");
    support::feed(&mut t, b"\x1b[!p");
    assert_eq!(released, t.updates_opened(), "a release is none");
}

/// `DECSTR` keeps `?5` (xterm's soft reset does), so it moves nothing.
#[test]
fn a_soft_reset_keeps_reverse_video_and_the_count() {
    each_read_size(b"\x1b[?5h", b"\x1b[!p", |before, after, reverse| {
        assert!(reverse);
        assert_eq!(before, after);
    });
}
