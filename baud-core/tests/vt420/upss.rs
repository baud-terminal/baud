//! **DECAUPSS, the assignment of the User-Preference Supplemental Set**,
//! `DCS Ps ! u Dscs ST` (`[[source: dec-std-070]]` p. 5-123, PDF 378):
//!
//! > This control string assigns a character set to be the
//! > User-Preference Supplemental Set (UPSS). ... Ps selects between 94 and
//! > 96 character sets (Ps = 0 selects a 94 character set). Dscs is the
//! > intermediate and final characters of the SCS escape sequence used to
//! > explicitly designated that character set. For example a value of
//! > "% 5" for Dscs indicates that the DEC Supplemental character set is
//! > the User-Preference Supplemental set. ... Attempts to assign an
//! > invalid supplemental set will be ignored.
//!
//! and its implementation guideline (p. 5-124): "The valid combinations
//! for the VT320 are: Ps = 0, Dscs = % 5 DEC Supplemental; Ps = 1, Dscs = A
//! ISO Latin-1 supplemental graphic". Its algorithm assigns on
//! `(parv[0] == 0) && (strcmp(string, "%5") == 0)` and on
//! `(parv[0] == 1) && (strcmp(string, "A") == 0)`, and nothing else.
//!
//! The UPSS is DEC Supplemental, and `0 ; % 5` is the assignment baud's
//! repertory holds: it is accepted, and DECRQUPSS (p. 5-125) goes on
//! answering it. `1 ; A` waits on which set `ESC ( <` designates (070-3
//! p. 3-63), and is ignored and counted with every invalid assignment.

use crate::support;
use baud_core::*;

const WS: WinSize = WinSize {
    ws_row: 8,
    ws_col: 20,
};

/// What `reads`, fed one read each after `setup`, leave: the strings
/// ignored and the controls unsupported that they added, the cursor, and
/// what DECRQUPSS then answers.
fn after(setup: &[u8], reads: &[&[u8]]) -> (usize, usize, (usize, usize), String) {
    support::with_term(&WS, |t| {
        support::feed(t, setup);
        let (s, u) = (t.strings_ignored, t.unsupported);
        for read in reads {
            support::feed(t, read);
        }
        let ignored = t.strings_ignored - s;
        let unsupported = t.unsupported - u;
        let cursor = (t.cursor().0.index(), t.cursor().1.index());
        t.clear_reply();
        support::feed(t, b"\x1b[&u");
        (
            ignored,
            unsupported,
            cursor,
            t.reply().escape_ascii().to_string(),
        )
    })
}

/// DECRQUPSS's answer while the UPSS is DEC Supplemental (p. 5-125,
/// `send_int(0); send_string("!u%5")`).
const DEC_SUPPLEMENTAL: &str = "\\x1bP0!u%5\\x1b\\\\";

/// **`0 ; % 5` is accepted**: nothing is counted, nothing is drawn, and
/// DECRQUPSS answers DEC Supplemental, in every read size. A `Ps` left out
/// is 0, the algorithm's `parv[0]` for an omitted parameter. Note 3: "This
/// sequence is sent from the terminal to the host in response to a
/// DECRQUPSS request", so DECRQUPSS's own answer, fed back, assigns.
#[test]
fn the_dec_supplemental_assignment_is_accepted() {
    for bytes in [&b"\x1bP0!u%5\x1b\\"[..], b"\x1bP!u%5\x1b\\"] {
        for size in 1..=bytes.len() {
            assert_eq!(
                after(b"", &bytes.chunks(size).collect::<Vec<_>>()),
                (0, 0, (0, 0), DEC_SUPPLEMENTAL.to_string()),
                "{:?}, read size {size}",
                bytes.escape_ascii().to_string()
            );
        }
    }
    let answer = support::with_term(&WS, |t| {
        support::feed(t, b"\x1b[&u");
        t.reply().to_vec()
    });
    assert_eq!(
        after(b"", &[answer.as_slice()]),
        (0, 0, (0, 0), DEC_SUPPLEMENTAL.to_string()),
        "DECRQUPSS's answer fed back"
    );
}

/// **The ISO Latin-1 assignment and every invalid one are ignored and
/// counted**, and the UPSS stays DEC Supplemental: `1 ; A`, which waits on
/// `ESC ( <`; `Dscs` and `Ps` crossed; a `Ps` past 1; a `Dscs` cut short or
/// run on; a final other than `u`; and a second intermediate.
#[test]
fn every_other_assignment_is_ignored_and_counted() {
    for bytes in [
        &b"\x1bP1!uA\x1b\\"[..],
        b"\x1bP0!uA\x1b\\",
        b"\x1bP1!u%5\x1b\\",
        b"\x1bP2!u%5\x1b\\",
        b"\x1bP0!u%\x1b\\",
        b"\x1bP0!u%5x\x1b\\",
        b"\x1bP0!u\x1b\\",
        b"\x1bP0!v%5\x1b\\",
        b"\x1bP0!!u%5\x1b\\",
    ] {
        assert_eq!(
            after(b"", &[bytes]),
            (1, 1, (0, 0), DEC_SUPPLEMENTAL.to_string()),
            "{:?}",
            bytes.escape_ascii().to_string()
        );
    }
}

/// **The eight-bit spellings assign as the seven-bit one does** under ISO
/// 8859-1: `DCS` and `ST` as `90` and `9C`, and every byte between in GR,
/// which STD 070 3.5.4.5 reads "as their 7-bit equivalent" in a DCS
/// introducer and 3.5.3 g in a control string's data.
#[test]
fn the_eight_bit_spellings_assign_under_latin1() {
    for bytes in [
        &b"\x900!u%5\x9c"[..],
        b"\x90\xb0\xa1\xf5\xa5\xb5\x9c",
        b"\x1bP\xb0\xa1u%\xb5\x1b\\",
    ] {
        assert_eq!(
            after(b"\x1b%@", &[bytes]),
            (0, 0, (0, 0), DEC_SUPPLEMENTAL.to_string()),
            "{:?}",
            bytes.escape_ascii().to_string()
        );
    }
}

/// **DECAUPSS is Level 2's** ("Levels: 2x (8-bit Interface Architecture),
/// 3-4"): at Level 1 the string is consumed, drawn nowhere and counted, as
/// a function above the level in force is; at Level 2 it assigns.
#[test]
fn decaupss_is_level_two() {
    let bytes: &[u8] = b"\x1bP0!u%5\x1b\\";
    let (ignored, unsupported, cursor, _) = after(b"\x1b[61\"p", &[bytes]);
    assert_eq!((ignored, cursor), (1, (0, 0)), "Level 1");
    assert!(unsupported >= 1, "Level 1: counted");
    let (ignored, unsupported, cursor, _) = after(b"\x1b[62\"p", &[bytes]);
    assert_eq!((ignored, unsupported, cursor), (0, 0, (0, 0)), "Level 2");
}
