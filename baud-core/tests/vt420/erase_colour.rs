//! DECECM, `CSI ? 117 h` and `CSI ? 117 l`: what an erase leaves.
//!
//! `[[source: vt520-rm]]` p. 5-48: "Controls the background color used
//! when text is erased or new text is scrolled on to the screen. Default:
//! Erase to text background ... Set: erase to screen background (VT) ...
//! Reset: erase to text background (PC)", and Set-Up 2.9.7 (p. 2-32):
//! "Screen background causes newly erased areas or scrolled text to be
//! written using color index zero, the screen background ... Text
//! background ... is the factory default". Set, every erase and every line
//! scrolled or opened onto the screen takes the default rendition, the
//! empty rendition of `[[source: dec-std-070]]` ED and EL (pp. 5-154 and
//! 5-156); reset, the current rendition, which the `bce` of the entry baud
//! names in `TERM` promises (`[[source: terminfo-xterm-256color]]`).

use crate::support;
use baud_core::*;
use baud_domain::StyleId;

/// Every cell of row `y` after `bytes`, and the program's own red: the
/// rendition `ESC [ 41 m` puts on a written `X`, at `(0, 0)`.
fn after(bytes: &[u8], y: usize) -> (Vec<PackedCell>, StyleId) {
    support::with_corpus(|t| {
        support::feed(t, b"\x1b[41mX\x1b[0m");
        let red = t.row(Row::ZERO).unwrap()[0].style;
        support::feed(t, b"\x1b[H");
        support::feed(t, bytes);
        (t.row(Row::from_index(y)).unwrap().to_vec(), red)
    })
}

/// The `DECRQM` answer for `?117` after `bytes`.
fn reported(bytes: &[u8]) -> Vec<u8> {
    support::with_corpus(|t| {
        support::feed(t, bytes);
        t.clear_reply();
        support::feed(t, b"\x1b[?117$p");
        t.reply().to_vec()
    })
}

/// Each erase, and each way a line comes onto the screen, with the cursor
/// at row 0 column 0 under red, on row `.2`.
const ERASES: [(&str, &[u8], usize); 13] = [
    ("ED 0", b"\x1b[41m\x1b[J", 0),
    ("ED 1", b"\x1b[41m\x1b[1;200H\x1b[1J", 0),
    ("ED 2", b"\x1b[41m\x1b[2J", 0),
    ("EL 0", b"\x1b[41m\x1b[K", 0),
    ("EL 1", b"\x1b[41m\x1b[1;200H\x1b[1K", 0),
    ("EL 2", b"\x1b[41m\x1b[2K", 0),
    ("ECH", b"\x1b[41m\x1b[200X", 0),
    ("IL", b"\x1b[41m\x1b[L", 0),
    ("DL", b"\x1b[41m\x1b[M", 49),
    ("ICH", b"\x1b[41m\x1b[200@", 0),
    ("DCH", b"\x1b[41m\x1b[200P", 0),
    ("SU", b"\x1b[41m\x1b[S", 49),
    ("a line feed at the bottom", b"\x1b[41m\x1b[50;1H\n", 49),
];

/// **Reset, the power-on state, an erase takes the current rendition**:
/// every cell each one covers names the red an `X` was written in.
#[test]
fn reset_erases_in_the_current_rendition() {
    for (name, bytes, y) in ERASES {
        let (row, red) = after(bytes, y);
        assert!(row.iter().all(|c| c.style == red), "{name}");
    }
}

/// **Set, the screen background**: the same erases leave the default
/// rendition, with the current rendition red throughout.
#[test]
fn set_erases_to_the_screen_background() {
    for (name, bytes, y) in ERASES {
        let program = [b"\x1b[?117h".as_slice(), bytes].concat();
        let (row, red) = after(&program, y);
        assert_ne!(red, StyleId::DEFAULT, "{name}: the control");
        assert!(row.iter().all(|c| c.style == StyleId::DEFAULT), "{name}");
        assert!(row.iter().all(|c| c.content.is_empty()), "{name}");
    }
}

/// **Reset, an erase keeps the background and clears every other
/// attribute**: "EL clears all character attributes from erased character
/// positions" (`[[source: vt510-el]]`, and ED and ECH the same), and the
/// VT525's text background is "the current text background color". Bold,
/// underline, reverse, strike and a foreground go: the erased cells name
/// the rendition `ESC [ 41 m` alone gives. Reverse is an attribute too, so
/// the background is the colour word, not the swapped one (Jev's lean,
/// UNSWAPPED). The default rendition interns nothing.
#[test]
fn reset_keeps_the_background_alone() {
    for (name, bytes, y) in ERASES {
        let program = [b"\x1b[1;4;7;9;32m".as_slice(), bytes].concat();
        let (row, red) = after(&program, y);
        assert!(row.iter().all(|c| c.style == red), "{name}");
    }
    let (row, _) = after(b"\x1b[1;4;7m\x1b[2K", 0);
    assert!(row.iter().all(|c| c.style == StyleId::DEFAULT));
}

/// A selective erase keeps each character's own rendition under either
/// state: `[[source: dec-std-070]]` DECSED and DECSEL, "The character
/// rendition and attributes associated with each position do not change".
#[test]
fn a_selective_erase_keeps_the_characters_own_rendition() {
    let (row, red) = after(b"\x1b[?117h\x1b[42m\x1b[?2K", 0);
    assert_eq!(row[0].style, red);
    assert_eq!(row[0].content.as_codepoint(), Some(Codepoint::SPACE));
}

/// **`DECRQM` reports it, `DECSTR` keeps it and `RIS` resets it**: `2` at
/// power-on, `1` once set, `1` after a soft reset (the VT520's table 5-6
/// does not name it) and `2` again after `RIS`, the factory default.
#[test]
fn it_is_reported_kept_by_decstr_and_reset_by_ris() {
    assert_eq!(reported(b""), b"\x1b[?117;2$y");
    assert_eq!(reported(b"\x1b[?117h"), b"\x1b[?117;1$y");
    assert_eq!(reported(b"\x1b[?117h\x1b[?117l"), b"\x1b[?117;2$y");
    assert_eq!(reported(b"\x1b[?117h\x1b[!p"), b"\x1b[?117;1$y");
    assert_eq!(reported(b"\x1b[?117h\x1bc"), b"\x1b[?117;2$y");
}

/// XTSAVE and XTRESTORE carry it, as they carry every mode DECSET acts on.
#[test]
fn xtsave_and_xtrestore_carry_it() {
    assert_eq!(
        reported(b"\x1b[?117h\x1b[?117s\x1b[?117l\x1b[?117r"),
        b"\x1b[?117;1$y"
    );
    let (row, _) = after(
        b"\x1b[?117h\x1b[?117s\x1b[?117l\x1b[?117r\x1b[41m\x1b[2K",
        0,
    );
    assert!(row.iter().all(|c| c.style == StyleId::DEFAULT));
}
