//! `?1047` and `?1048`, the two halves of `?1049`.
//!
//! `[[source: ctlseqs]]`: DECSET `?1047` "Use Alternate Screen Buffer",
//! DECRST "Use Normal Screen Buffer ... Clear the screen first if in the
//! Alternate Screen Buffer"; DECSET `?1048` "Save cursor as in DECSC",
//! DECRST "Restore cursor as in DECRC"; each "may be disabled by the
//! titeInhibit resource", which `?1046` reset is here. xterm's code
//! (`[[source: xterm-reset]]`'s file, `srm_OPT_ALTBUF` and
//! `srm_SAVE_CURSOR`) is `ToAlternate(xw, False)` in, `ClearScreen` if on the
//! alternate buffer and `FromAlternate(xw, False)` out, and `CursorSave` and
//! `CursorRestore`. Every test here fails on a `?1047` or `?1048` that does
//! nothing, which is what both did: counted and dropped.
mod support;
use baud_core::{Owned, Screen, Term};
use baud_domain::{Col, Row};

/// The first eight cells of screen row `y`, as text.
fn text(t: &Term<'_>, y: usize) -> String {
    t.row(Row::from_index(y)).expect("a row on the grid")[..8]
        .iter()
        .map(|c| {
            c.content
                .as_codepoint()
                .map_or(' ', |cp| char::from_u32(cp.value()).unwrap_or('?'))
        })
        .collect()
}

/// Feed `bytes` at read sizes 1, 3, 7 and whole, and hand each terminal to
/// `f`: a mode split across two reads must land where one read puts it.
fn each_read_size(bytes: &[u8], f: impl Fn(&mut Term<'_>)) {
    for chunk in [1, 3, 7, bytes.len().max(1)] {
        let mut owned = Owned::new(&support::CORPUS);
        let mut t = Term::new(owned.storage(), &support::CORPUS);
        for piece in bytes.chunks(chunk) {
            support::feed(&mut t, piece);
        }
        f(&mut t);
    }
}

/// `DECRQM`'s answer for private mode `n`, after `bytes`.
fn decrqm(bytes: &[u8], n: u16) -> Vec<u8> {
    let mut owned = Owned::new(&support::CORPUS);
    let mut t = Term::new(owned.storage(), &support::CORPUS);
    support::feed(&mut t, bytes);
    t.clear_reply();
    support::feed(&mut t, format!("\x1b[?{n}$p").as_bytes());
    t.reply().to_vec()
}

/// **`?1047 h` switches and keeps what the alternate screen held**, as
/// `?47 h` does; it does not clear on the way in.
#[test]
fn mode_1047_shows_the_alternate_screen_as_it_was_left() {
    each_read_size(
        b"\x1b[1;1Hprimary\x1b[?47h\x1b[1;1Hsecond\x1b[?47l\x1b[?1047h",
        |t| {
            assert!(
                t.screen() == Screen::Alternate,
                "the alternate screen is up"
            );
            assert_eq!(text(t, 0), "second  ", "shown again, not cleared");
            assert_eq!(t.unsupported_param, 0);
        },
    );
}

/// **`?1047 l` clears the alternate screen first**, so the next visit finds
/// it empty, where `?47 l` leaves its text for `?47 h` to show again; the
/// primary comes back as it was, and the cursor is not restored.
#[test]
fn mode_1047_clears_the_alternate_screen_on_the_way_out() {
    each_read_size(
        b"\x1b[1;1Hprimary\x1b[?1047h\x1b[1;1Hsecond\x1b[3;5H\x1b[?1047l",
        |t| {
            assert!(t.screen() == Screen::Normal, "the primary is back");
            assert_eq!(text(t, 0), "primary ");
            assert_eq!(
                t.cursor(),
                (Row::from_index(2), Col::at_index(4)),
                "no restore"
            );
            support::feed(t, b"\x1b[?47h");
            assert_eq!(text(t, 0), "        ", "the alternate screen was cleared");
        },
    );
    // The control: `?47 l` keeps it.
    each_read_size(
        b"\x1b[1;1Hprimary\x1b[?1047h\x1b[1;1Hsecond\x1b[?47l\x1b[?47h",
        |t| assert_eq!(text(t, 0), "second  "),
    );
}

/// `?1047 l` on the primary clears nothing: "if in the Alternate Screen
/// Buffer".
#[test]
fn mode_1047_reset_on_the_primary_clears_nothing() {
    each_read_size(b"\x1b[1;1Hprimary\x1b[?1047l", |t| {
        assert_eq!(text(t, 0), "primary ");
        assert_eq!(t.unsupported_param, 0);
    });
}

/// **`?1048` is DECSC and DECRC**: the same position and rendition come
/// back as after `ESC 7` and `ESC 8`.
#[test]
fn mode_1048_saves_and_restores_the_cursor_as_decsc_and_decrc() {
    let run = |save: &[u8], restore: &[u8]| {
        let mut owned = Owned::new(&support::CORPUS);
        let mut t = Term::new(owned.storage(), &support::CORPUS);
        let mut bytes = b"\x1b[5;10H\x1b[1m".to_vec();
        bytes.extend_from_slice(save);
        bytes.extend_from_slice(b"\x1b[m\x1b[1;1H");
        bytes.extend_from_slice(restore);
        bytes.extend_from_slice(b"X");
        support::feed(&mut t, &bytes);
        let row = t.row(Row::from_index(4)).expect("row 5").to_vec();
        (t.cursor(), row[9], t.unsupported_param)
    };
    let (cursor, cell, counted) = run(b"\x1b[?1048h", b"\x1b[?1048l");
    let (want_cursor, want_cell, _) = run(b"\x1b7", b"\x1b8");
    assert_eq!(cursor, (Row::from_index(4), Col::at_index(10)));
    assert_eq!(
        (cursor, cell),
        (want_cursor, want_cell),
        "as DECSC and DECRC"
    );
    assert_eq!(counted, 0);
}

/// **`?1046` reset disables both**, as titeInhibit does xterm's.
#[test]
fn modes_1047_and_1048_wait_on_1046() {
    each_read_size(b"\x1b[?1046l\x1b[1;1Hprimary\x1b[?1047h", |t| {
        assert!(t.screen() == Screen::Normal, "no switch");
    });
    each_read_size(
        b"\x1b[?1046l\x1b[5;10H\x1b[?1048h\x1b[1;1H\x1b[?1048l",
        |t| {
            assert_eq!(
                t.cursor(),
                (Row::from_index(0), Col::at_index(0)),
                "no save"
            );
        },
    );
}

/// `DECRQM`: `?1047` answers which buffer is shown, as `?47` and `?1049`
/// do; `?1048` whether a cursor is saved, xterm's
/// `MdBool(screen->sc[screen->whichBuf].saved)` (`[[source: xterm-misc]]`).
#[test]
fn decrqm_reads_1047_and_1048() {
    assert_eq!(decrqm(b"", 1047), b"\x1b[?1047;2$y");
    assert_eq!(decrqm(b"\x1b[?1047h", 1047), b"\x1b[?1047;1$y");
    assert_eq!(decrqm(b"\x1b[?1049h", 1047), b"\x1b[?1047;1$y");
    assert_eq!(decrqm(b"", 1048), b"\x1b[?1048;2$y");
    assert_eq!(decrqm(b"\x1b[?1048h", 1048), b"\x1b[?1048;1$y");
    assert_eq!(decrqm(b"\x1b7", 1048), b"\x1b[?1048;1$y");
}

/// **XTSAVE of `?1048` saves the cursor and XTRESTORE restores it**, as
/// xterm's `savemodes` and `restoremodes` do (Jev, XTERM_ACTIONS,
/// 0.91/0.94); `?1047` keeps which buffer is shown and a restore goes
/// through DECSET's path, as `?1049`'s does.
#[test]
fn xtsave_and_xtrestore_of_1047_and_1048() {
    each_read_size(b"\x1b[3;3H\x1b[?1048s\x1b[1;1H\x1b[?1048r", |t| {
        assert_eq!(t.cursor(), (Row::from_index(2), Col::at_index(2)));
        assert_eq!(t.unsupported_param, 0);
    });
    each_read_size(
        b"\x1b[1;1Hprimary\x1b[?1047s\x1b[?1047h\x1b[1;1Hsecond\x1b[?1047r",
        |t| {
            assert!(t.screen() == Screen::Normal, "restored to the primary");
            assert_eq!(text(t, 0), "primary ");
            support::feed(t, b"\x1b[?47h");
            assert_eq!(text(t, 0), "        ", "left through ?1047 l, cleared");
        },
    );
}
