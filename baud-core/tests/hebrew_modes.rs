//! **The VT510's Hebrew modes, as the core keeps them**: `CSI ? 96 h/l`
//! DECRLCM, right-to-left copy (`[[source: vt510-decrlcm]]`), and
//! `CSI ? 36 h/l` DECHEM, Hebrew encoding mode (`[[source: vt510-dechem]]`).
//! What a copy gives under DECRLCM is `baud-render/tests/select.rs`.

mod support;

use baud_core::*;
use baud_domain::{CopyDirection, PackedCell};

const WS: WinSize = WinSize {
    ws_row: 4,
    ws_col: 10,
};

/// DECRQM's answer for private mode `n`.
fn decrqm(t: &mut Term<'_>, n: u16) -> String {
    t.clear_reply();
    support::feed(t, format!("\x1b[?{n}$p").as_bytes());
    String::from_utf8_lossy(t.reply()).into_owned()
}

/// The first `n` characters of row 0.
fn row0(t: &Term<'_>, n: usize) -> String {
    let cells: Vec<PackedCell> = support::screen(t);
    cells[..n]
        .iter()
        .map(|c| {
            c.content
                .as_codepoint()
                .and_then(|cp| char::from_u32(cp.value()))
                .unwrap_or('.')
        })
        .collect()
}

/// **DECRQM answers DECRLCM, reset at power-on** ("Default: Disable
/// right-to-left-copy"), and set once `?96 h` sets it; the embedder reads
/// the direction. Until now `?96` was counted and answered `0`.
#[test]
fn decrqm_answers_decrlcm_and_the_embedder_reads_it() {
    support::with_term(&WS, |t| {
        assert_eq!(decrqm(t, 96), "\x1b[?96;2$y");
        assert_eq!(t.copy_direction(), CopyDirection::LeftToRight);
        support::feed(t, b"\x1b[?96h");
        assert_eq!(decrqm(t, 96), "\x1b[?96;1$y");
        assert_eq!(t.copy_direction(), CopyDirection::RightToLeft);
        support::feed(t, b"\x1b[?96l");
        assert_eq!(t.copy_direction(), CopyDirection::LeftToRight);
    });
}

/// **DECSTR leaves DECRLCM**, as its table does not list it
/// (`[[source: vt510-decstr]]`), and **RIS returns it to left to right**.
#[test]
fn decstr_keeps_decrlcm_and_ris_resets_it() {
    support::with_term(&WS, |t| {
        support::feed(t, b"\x1b[?96h\x1b[!p");
        assert_eq!(t.copy_direction(), CopyDirection::RightToLeft);
        support::feed(t, b"\x1bc");
        assert_eq!(t.copy_direction(), CopyDirection::LeftToRight);
    });
}

/// **DECHEM reset is a Hebrew VT510's national mode**: the DEC 7-bit Hebrew
/// NRCS in G0 to G3, so `` ` `` and `a` are drawn alef and bet, and so is
/// `` ` `` through G2; set returns the sets to their power-up state, the
/// multinational mode, which it is at power-on, line drawing in G0 gone
/// with the rest ("re-initializing the designated and invoked character
/// sets to their default state", STD 070's DECNRCM note 1). DECRQM answers
/// both. Until now `?36` was counted and answered `0`.
#[test]
fn dechem_reset_designates_the_hebrew_nrcs_and_set_ascii() {
    support::with_term(&WS, |t| {
        assert_eq!(decrqm(t, 36), "\x1b[?36;1$y");
        support::feed(t, b"\x1b[?36l`a\x1bN`");
        assert_eq!(row0(t, 3), "\u{5D0}\u{5D1}\u{5D0}");
        assert_eq!(decrqm(t, 36), "\x1b[?36;2$y");
        support::feed(t, b"\x1b(0\x1b[?36h`a");
        assert_eq!(row0(t, 5), "\u{5D0}\u{5D1}\u{5D0}`a");
        assert_eq!(decrqm(t, 36), "\x1b[?36;1$y");
    });
}

/// **DECSTR returns DECHEM to multinational with the sets**, as its table
/// returns DECNRCM, the mode DECHEM is the opposite of, to "Multinational
/// set" (`[[source: vt510-decstr]]`).
#[test]
fn decstr_returns_dechem_to_multinational() {
    support::with_term(&WS, |t| {
        support::feed(t, b"\x1b[?36l\x1b[!p`");
        assert_eq!(row0(t, 1), "`");
        assert_eq!(decrqm(t, 36), "\x1b[?36;1$y");
    });
}

/// **Each display keeps its own DECHEM**: the status line's national mode
/// leaves the main display's multinational one (`[[source: vt510-dechem]]`,
/// "maintains DECHEM for the main display and status line separately").
#[test]
fn the_status_line_keeps_its_own_dechem() {
    support::with_term(&WS, |t| {
        support::feed(t, b"\x1b[2$~\x1b[1$}\x1b[?36l\x1b[0$}`");
        assert_eq!(row0(t, 1), "`", "the main display's G0 is ASCII");
        assert_eq!(decrqm(t, 36), "\x1b[?36;1$y");
        support::feed(t, b"\x1b[1$}");
        assert_eq!(decrqm(t, 36), "\x1b[?36;2$y");
    });
}

/// **XTSAVE keeps both**, as it keeps every mode DECSET acts on.
#[test]
fn xtsave_and_xtrestore_keep_both() {
    support::with_term(&WS, |t| {
        support::feed(t, b"\x1b[?96h\x1b[?96;36s\x1b[?96l\x1b[?36l\x1b[?96;36r");
        assert_eq!(decrqm(t, 96), "\x1b[?96;1$y");
        assert_eq!(decrqm(t, 36), "\x1b[?36;1$y");
    });
}
