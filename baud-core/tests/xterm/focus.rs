//! `?1004` focus reporting: `CSI I` on gaining focus, `CSI O` on losing it,
//! and nothing at all unless the application asked.
//!
//! `[[source: ctlseqs]]`, "FocusIn/FocusOut": "When set, it causes xterm to
//! send CSI I when the terminal gains focus, and CSI O when it loses it."
//!
//! **This mode was dropped uncounted until 2026-09-20.** `1004` appeared
//! nowhere in the code, `docs/vt-coverage.md` carried no row for it, and
//! `set_private_mode`'s wildcard was a bare `{}` where the ANSI table's counts
//! -- so the request `vim-unicode.raw` sends on its first line arrived and
//! vanished. The sfte survey named it as the first thing to build, because it
//! is the one gap baud's own corpus exercises.

use crate::support;
use baud_core::*;

/// Feed `setup`, then report `f`, and return whether it was sent and what
/// went to the host.
fn after(setup: &[u8], f: Focus) -> (bool, Vec<u8>) {
    let mut s = Owned::new(&support::CORPUS);
    let mut t = Term::new(s.storage(), &support::CORPUS);
    support::feed(&mut t, setup);
    t.clear_reply();
    let sent = t.focus(f);
    (sent, t.reply().to_vec())
}

/// The two reports, byte for byte, once `?1004` is set.
#[test]
fn gain_sends_csi_i_and_loss_sends_csi_o() {
    assert_eq!(
        after(b"\x1b[?1004h", Focus::Gained),
        (true, b"\x1b[I".to_vec())
    );
    assert_eq!(
        after(b"\x1b[?1004h", Focus::Lost),
        (true, b"\x1b[O".to_vec())
    );
}

/// **The negative case is the one that matters.** An application that never
/// asked reads `CSI I` as a keypress; a fresh terminal, and one that asked and
/// then reset, must send nothing and say so.
#[test]
fn nothing_is_sent_unless_the_application_asked() {
    assert_eq!(after(b"", Focus::Gained), (false, Vec::new()));
    assert_eq!(
        after(b"\x1b[?1004h\x1b[?1004l", Focus::Lost),
        (false, Vec::new())
    );
}

/// DECRQM answers the truth: `1` set, `2` reset, and never `0` now that the
/// mode exists -- answering "not recognized" for a mode the terminal acts on
/// would tell the application to fall back to polling.
#[test]
fn decrqm_reports_the_mode() {
    let reply = |setup: &[u8]| {
        let mut s = Owned::new(&support::CORPUS);
        let mut t = Term::new(s.storage(), &support::CORPUS);
        support::feed(&mut t, setup);
        t.reply().to_vec()
    };
    assert_eq!(reply(b"\x1b[?1004h\x1b[?1004$p"), b"\x1b[?1004;1$y");
    assert_eq!(reply(b"\x1b[?1004$p"), b"\x1b[?1004;2$y");
}

/// **The request is counted where it used to vanish.** Setting `?1004` is no
/// longer an unsupported parameter; a private mode this core still does not
/// know is. That is the counter the wildcard grew for this change, and the
/// positive control is a mode nothing implements.
#[test]
fn a_known_mode_is_not_counted_and_an_unknown_one_is() {
    let count = |setup: &[u8]| {
        let mut s = Owned::new(&support::CORPUS);
        let mut t = Term::new(s.storage(), &support::CORPUS);
        support::feed(&mut t, setup);
        t.unsupported_param
    };
    assert_eq!(count(b"\x1b[?1004h"), 0, "?1004 is implemented");
    assert_eq!(
        count(b"\x1b[?9999h"),
        1,
        "an unknown private mode is counted"
    );
}

/// The corpus sends it: `vim-unicode.raw` enables focus reporting in its
/// first line, so after that recording a focus change is reported.
#[test]
fn the_vim_recording_enables_it() {
    let bytes = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../tests/recordings/utf8/vim-unicode.raw"
    ))
    .unwrap();
    assert_eq!(after(&bytes, Focus::Gained), (true, b"\x1b[I".to_vec()));
}

/// **A report the reply buffer cannot hold returns `false`.** `focus` said
/// `true` for a report `queue` had dropped -- counted in `ReplyCounts::overflows`,
/// and then announced as sent -- the same defect `mouse_report` had, found
/// beside it on 2026-10-09.
#[test]
fn a_report_that_does_not_fit_says_so() {
    let mut s = Owned::new(&support::CORPUS);
    let mut t = Term::new(s.storage(), &support::CORPUS);
    support::feed(&mut t, b"\x1b[?1004h");
    t.clear_reply();
    // `CSI I` is three bytes: 85 fit in REPLY and one byte is left over.
    let fit = REPLY / 3;
    for i in 0..fit {
        assert!(t.focus(Focus::Gained), "report {i} fits");
    }
    let before = t.reply_counts().overflows;
    assert!(
        !t.focus(Focus::Lost),
        "the next does not fit, and must say so"
    );
    assert_eq!(t.reply().len(), fit * 3, "dropped whole");
    assert_eq!(t.reply_counts().overflows, before + 1, "and counted");
    // Positive control: into a drained buffer it is queued.
    t.clear_reply();
    assert!(t.focus(Focus::Lost));
    assert_eq!(t.reply(), b"\x1b[O");
}
