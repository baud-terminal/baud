#![deny(dead_code)]
//! **The C0 controls a control string's introducer and body carry**: a
//! shift in any string, and every C0 in a DCS introducer.
//!
//! `[[source: ecma48]]` §9 lets `SO` and `SI` occur "between the opening
//! delimiter of a control string and the STRING TERMINATOR", where they
//! "have no effect on the interpretation of ... a control string, but they
//! may, indeed, affect the meanings of bit combinations following in the data
//! stream" (PDF p. 89), and `[[source: dec-std-070]]` 3.5.1.4 says the same
//! (PDF p. 126). Its reference parser, 3.5.5, recognises "all other C0
//! controls ... within escape, control, or control string introducer
//! sequences", and the introducer "continues where it left off" (PDF p. 144).

use baud_core::*;
use baud_domain::ImageBudget;

/// A terminal fed `bytes` in reads of `size`, handed to `f` while it lives.
fn fed<R>(bytes: &[u8], size: usize, f: impl FnOnce(&Term<'_>) -> R) -> R {
    let ws = WinSize {
        ws_row: 6,
        ws_col: 40,
    };
    let mut s = Owned::new(&ws).with_images(ImageBudget::of(64 * 64));
    let mut t = Term::new(s.storage(), &ws);
    for part in bytes.chunks(size) {
        t.write_at(BytePos::ZERO, part);
        t.feed(BytePos::ZERO, ReadLen::from_index(part.len()));
    }
    f(&t)
}

/// The codepoint at row 0, column `col`.
fn at(t: &Term<'_>, col: usize) -> Option<u32> {
    let row = t.row(Row::from_index(0)).expect("a row on the grid");
    row[col].content.as_codepoint().map(|c| c.value())
}

/// `ESC ) 0`: G1 holds DEC Special Graphics, whose `q` is U+2500.
const G1_GRAPHICS: &[u8] = b"\x1b)0";
const HORIZONTAL: u32 = 0x2500;

/// Every kind of string, each with `{}` where the shift goes, and what it
/// leaves that the shift must not change: the title, the reply.
const STRINGS: [(&str, &[u8], &[u8]); 7] = [
    ("OSC 0", b"\x1b]0;t", b"i\x1b\\"),
    ("OSC 0 ended by BEL", b"\x1b]0;t", b"i\x07"),
    ("DECRQSS", b"\x1bP$q", b"m\x1b\\"),
    ("another DCS", b"\x1bP0;1|", b"17/31\x1b\\"),
    ("a Sixel", b"\x1bPq#1;2;100;0;0", b"~\x1b\\"),
    ("APC", b"\x1b_H", b"x\x1b\\"),
    ("PM and SOS", b"\x1b^", b"x\x1b\\\x1bXy\x1b\\"),
];

/// **`SO` inside a string invokes G1 for the text after it, and the string
/// reads as if it were not there; `SI` invokes G0.** The title, the reply,
/// the image and the cursor are the shift-less string's, and the `q` after
/// the terminator is DEC Special Graphics' horizontal line after `SO`, a
/// `q` after `SI`. Every read size. The control is the string with no shift
/// in it, which leaves the `q` a `q`, so the harness can see both.
#[test]
fn a_shift_inside_a_control_string_shifts_the_text_after_it() {
    for (name, head, tail) in STRINGS {
        let mut plain = G1_GRAPHICS.to_vec();
        plain.extend_from_slice(head);
        plain.extend_from_slice(tail);
        plain.push(b'q');
        for (shift, before, want) in [
            (0x0e, &b""[..], HORIZONTAL),
            (0x0f, &b"\x0e"[..], u32::from(b'q')),
        ] {
            let mut bytes = G1_GRAPHICS.to_vec();
            bytes.extend_from_slice(before);
            bytes.extend_from_slice(head);
            bytes.push(shift);
            bytes.extend_from_slice(tail);
            bytes.push(b'q');
            let mut control = G1_GRAPHICS.to_vec();
            control.extend_from_slice(before);
            control.extend_from_slice(head);
            control.extend_from_slice(tail);
            control.push(b'q');
            let base = fed(&plain, plain.len(), |t| {
                (
                    t.title().to_vec(),
                    t.reply().to_vec(),
                    t.cursor(),
                    t.images().placements().count(),
                )
            });
            let other = fed(&control, control.len(), |t| at(t, t.cursor().1.index() - 1));
            assert_ne!(
                other,
                Some(want),
                "{name}, {shift:#04x}: the control must differ"
            );
            for size in 1..=bytes.len() {
                fed(&bytes, size, |t| {
                    let got = (
                        t.title().to_vec(),
                        t.reply().to_vec(),
                        t.cursor(),
                        t.images().placements().count(),
                    );
                    assert_eq!(
                        got, base,
                        "{name}, {shift:#04x}, read size {size}: the string's effects"
                    );
                    let col = t.cursor().1.index() - 1;
                    assert_eq!(
                        at(t, col),
                        Some(want),
                        "{name}, {shift:#04x}, read size {size}: the q"
                    );
                });
            }
        }
    }
}
/// **A shift inside a kitty graphics command is its payload and shifts
/// nothing**: kitty keeps every byte but `BEL` and `ESC \\` in the command
/// (`find_st_terminator`, `[[source: kitty-vt-parser-c]]`), and an `APC G`
/// is read as kitty reads it, so the `q` after it stays a `q` under `SO` and
/// becomes the line under `SI`, where the `SO` before the command still
/// holds. The `SO` lands in the keys and the command is refused, as kitty
/// refuses it. Every read size.
#[test]
fn a_shift_inside_a_kitty_command_is_payload_and_shifts_nothing() {
    for (shift, before, want) in [
        (0x0e, &b""[..], u32::from(b'q')),
        (0x0f, &b"\x0e"[..], HORIZONTAL),
    ] {
        let mut bytes = G1_GRAPHICS.to_vec();
        bytes.extend_from_slice(before);
        bytes.extend_from_slice(b"\x1b_G");
        bytes.push(shift);
        bytes.extend_from_slice(b"a=q,i=1\x1b\\q");
        for size in 1..=bytes.len() {
            fed(&bytes, size, |t| {
                assert_eq!(t.cursor().1.index(), 1, "{shift:#04x}, read size {size}");
                assert_eq!(
                    at(t, 0),
                    Some(want),
                    "{shift:#04x}, read size {size}: the q"
                );
                assert_eq!(t.kitty().refused, 1, "{shift:#04x}, read size {size}");
            });
        }
    }
}

/// A program, its name, the cursor it leaves and the bells it rings.
type Case = (&'static str, &'static [u8], (usize, usize), usize);

/// **A C0 in a DCS introducer is executed and the introducer goes on**: a
/// line feed moves the cursor down and the DECRQSS still answers, a carriage
/// return after the `$` goes to column 0, a `BEL` rings once, an `SO` before
/// a Sixel's `q` shifts the text after the image, and the image is still
/// drawn. Every read size, and the 8-bit introducer under ISO 8859-1. The
/// controls are the same introducers without the C0.
#[test]
fn a_c0_in_a_dcs_introducer_is_executed_and_the_introducer_goes_on() {
    let valid = b"\x1bP1$r0m\x1b\\";
    let cases: [Case; 5] = [
        ("LF before $", b"AB\x1bP\n$qm\x1b\\", (1, 2), 0),
        ("CR after $", b"AB\x1bP$\rqm\x1b\\", (0, 0), 0),
        ("BEL after a parameter", b"AB\x1bP1\x07$qm\x1b\\", (0, 2), 1),
        ("LF, 8-bit, ISO 8859-1", b"\x1b%@AB\x90\n$qm\x9c", (1, 2), 0),
        (
            "VT before a Sixel's q",
            b"AB\x1bP0;1\x0bq~\x1b\\",
            (2, 2),
            0,
        ),
    ];
    for (name, bytes, (row, col), bells) in cases {
        for size in 1..=bytes.len() {
            fed(bytes, size, |t| {
                let sixel = name.contains("Sixel");
                let reply: &[u8] = if sixel { b"" } else { valid };
                assert_eq!(t.reply(), reply, "{name}, read size {size}: the reply");
                assert_eq!(t.bells, bells, "{name}, read size {size}: the bells");
                assert_eq!(
                    t.images().placements().count(),
                    usize::from(sixel),
                    "{name}, read size {size}: the image"
                );
                if !sixel {
                    assert_eq!(
                        (t.cursor().0.index(), t.cursor().1.index()),
                        (row, col),
                        "{name}, read size {size}: the cursor"
                    );
                }
            });
        }
    }
    // The controls: no C0, the cursor where `AB` left it, and a Sixel's
    // image drawn where the cursor was.
    for bytes in [&b"AB\x1bP$qm\x1b\\"[..], b"AB\x1bP1$qm\x1b\\"] {
        fed(bytes, bytes.len(), |t| {
            assert_eq!(t.reply(), valid);
            assert_eq!(
                (t.cursor().0.index(), t.cursor().1.index(), t.bells),
                (0, 2, 0)
            );
        });
    }
    // The Sixel's anchor follows the line feed: one row down with it.
    let anchor = |bytes: &[u8]| {
        fed(bytes, bytes.len(), |t| {
            t.images().placements().next().map(|(_, p)| p.line)
        })
    };
    assert_ne!(
        anchor(b"AB\x1bP0;1\x0bq~\x1b\\"),
        anchor(b"AB\x1bP0;1q~\x1b\\"),
        "the VT moved where the image is placed"
    );
}

/// **An `SO` in a DCS introducer shifts too**, a C0 like the rest, and the
/// DECRQSS after it answers.
#[test]
fn a_shift_in_a_dcs_introducer_shifts_the_text_after_it() {
    let bytes = b"\x1b)0\x1bP\x0e$qm\x1b\\q";
    for size in 1..=bytes.len() {
        fed(bytes, size, |t| {
            assert_eq!(t.reply(), b"\x1bP1$r0m\x1b\\", "read size {size}");
            assert_eq!(at(t, 0), Some(HORIZONTAL), "read size {size}");
        });
    }
}

/// **A C0 after a DCS intermediate or private marker is executed too**,
/// and the introducer goes on to its final byte: STD 070 3.5.5 names
/// every "control string introducer sequence", not only its parameters.
/// XTGETTCAP's `+`, DECAUPSS's `!`, a `=` marker, a `?` or `:` after a
/// parameter, and a byte after `$` that is no final each carry a line
/// feed, a `BEL` rings once, and the 8-bit introducer does the same under
/// ISO 8859-1. Every read size; each reply is the one the same string
/// sends without the C0. The controls: a C0 after the final byte is the
/// body's and moves nothing.
#[test]
fn a_c0_after_a_dcs_intermediate_or_marker_is_executed_and_the_introducer_goes_on() {
    // (name, with the C0, without it, the cursor, the bells)
    type Pair = (
        &'static str,
        &'static [u8],
        &'static [u8],
        (usize, usize),
        usize,
    );
    let cases: [Pair; 9] = [
        (
            "LF after +",
            b"AB\x1bP+\nq544e\x1b\\",
            b"AB\x1bP+q544e\x1b\\",
            (1, 2),
            0,
        ),
        (
            "LF after !",
            b"AB\x1bP0!\nuA\x1b\\",
            b"AB\x1bP0!uA\x1b\\",
            (1, 2),
            0,
        ),
        (
            "LF after =",
            b"AB\x1bP=\n1s\x1b\\",
            b"AB\x1bP=1s\x1b\\",
            (1, 2),
            0,
        ),
        (
            "LF after a parameter and ?",
            b"AB\x1bP1?\n$qm\x1b\\",
            b"AB\x1bP1?$qm\x1b\\",
            (1, 2),
            0,
        ),
        (
            "LF after a parameter and :",
            b"AB\x1bP1:\n$qm\x1b\\",
            b"AB\x1bP1:$qm\x1b\\",
            (1, 2),
            0,
        ),
        (
            "LF after $ and a parameter",
            b"AB\x1bP$1\nqm\x1b\\",
            b"AB\x1bP$1qm\x1b\\",
            (1, 2),
            0,
        ),
        (
            "CR between two intermediates",
            b"AB\x1bP+!\r!q\x1b\\",
            b"AB\x1bP+!!q\x1b\\",
            (0, 0),
            0,
        ),
        (
            "BEL after +",
            b"AB\x1bP+\x07q544e\x1b\\",
            b"AB\x1bP+q544e\x1b\\",
            (0, 2),
            1,
        ),
        (
            "LF after +, 8-bit, ISO 8859-1",
            b"\x1b%@AB\x90+\nq544e\x9c",
            b"\x1b%@AB\x90+q544e\x9c",
            (1, 2),
            0,
        ),
    ];
    for (name, bytes, control, (row, col), bells) in cases {
        let want = fed(control, control.len(), |t| {
            assert_eq!(
                (t.cursor().0.index(), t.cursor().1.index(), t.bells),
                (0, 2, 0),
                "{name}: the control"
            );
            t.reply().to_vec()
        });
        for size in 1..=bytes.len() {
            fed(bytes, size, |t| {
                assert_eq!(t.reply(), want, "{name}, read size {size}: the reply");
                assert_eq!(t.bells, bells, "{name}, read size {size}: the bells");
                assert_eq!(
                    (t.cursor().0.index(), t.cursor().1.index()),
                    (row, col),
                    "{name}, read size {size}: the cursor"
                );
            });
        }
    }
    // XTGETTCAP answers, so its reply is a reply and not two silences.
    let tcap = b"AB\x1bP+q544e\x1b\\";
    fed(tcap, tcap.len(), |t| {
        assert!(!t.reply().is_empty(), "XTGETTCAP answers")
    });
    // `CAN` and `SUB` still cancel the introducer past an intermediate or a
    // marker, and the text after them is the ground's.
    for cut in [&b"AB\x1bP+\x18x"[..], b"AB\x1bP=\x1ax", b"AB\x1bP0!\x18x"] {
        for size in 1..=cut.len() {
            fed(cut, size, |t| {
                assert_eq!(at(t, 2), Some(u32::from(b'x')), "{cut:?}, read size {size}");
            });
        }
    }
    // After the final byte a C0 is the body's: the cursor stays.
    for body in [
        &b"AB\x1bP+q\n544e\x1b\\"[..],
        b"AB\x1bP0!u\nA\x1b\\",
        b"AB\x1bP=1s\n\x1b\\",
    ] {
        for size in 1..=body.len() {
            fed(body, size, |t| {
                assert_eq!(
                    (t.cursor().0.index(), t.cursor().1.index()),
                    (0, 2),
                    "{body:?}, read size {size}: a C0 in the body moves nothing"
                );
            });
        }
    }
}
