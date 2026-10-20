//! `?30`, rxvt's scrollbar mode: whether the embedder shows its scrollbar.
//!
//! `[[source: ctlseqs]]`: DECSET `?30` "Show scrollbar (rxvt)", DECRST
//! "Don't show scrollbar (rxvt)". xterm toggles its scrollbar and answers
//! `DECRQM` from whether one is shown (`[[source: xterm-misc]]`
//! 5553-5554); no reset path touches it. Every test here fails on a `?30`
//! that does nothing, which is what it was: counted and dropped.
mod support;
use baud_core::{Owned, ScrollbarShown, Term, WinSize};

const WS: WinSize = WinSize {
    ws_row: 8,
    ws_col: 20,
};

/// What the embedder reads, `DECRQM`'s answer, and the dropped-parameter
/// count, after `bytes`.
fn after(bytes: &[u8]) -> (ScrollbarShown, Vec<u8>, usize) {
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    support::feed(&mut t, bytes);
    let (shown, dropped) = (t.scrollbar_shown(), t.unsupported_param);
    t.clear_reply();
    support::feed(&mut t, b"\x1b[?30$p");
    (shown, t.reply().to_vec(), dropped)
}

/// **Shown at power-on, and `DECRQM` says so**: baud's app has a
/// scrollbar.
#[test]
fn the_scrollbar_is_shown_at_power_on() {
    assert_eq!(
        after(b""),
        (ScrollbarShown::Shown, b"\x1b[?30;1$y".to_vec(), 0)
    );
}

/// **Reset hides it and set shows it again**, for the embedder to act on.
#[test]
fn reset_hides_it_and_set_shows_it() {
    assert_eq!(
        after(b"\x1b[?30l"),
        (ScrollbarShown::Hidden, b"\x1b[?30;2$y".to_vec(), 0)
    );
    assert_eq!(
        after(b"\x1b[?30l\x1b[?30h"),
        (ScrollbarShown::Shown, b"\x1b[?30;1$y".to_vec(), 0)
    );
}

/// **Both resets keep it**, as xterm's do, the scrollbar being the
/// window's; XTSAVE and XTRESTORE keep and bring it back.
#[test]
fn resets_keep_it_and_xtsave_restores_it() {
    assert_eq!(after(b"\x1b[?30l\x1bc").0, ScrollbarShown::Hidden, "RIS");
    assert_eq!(
        after(b"\x1b[?30l\x1b[!p").0,
        ScrollbarShown::Hidden,
        "DECSTR"
    );
    assert_eq!(
        after(b"\x1b[?30l\x1b[?30s\x1b[?30h\x1b[?30r").0,
        ScrollbarShown::Hidden
    );
}
