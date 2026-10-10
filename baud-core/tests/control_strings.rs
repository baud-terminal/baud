#![deny(dead_code)]
//! **Tier 0: a control string is consumed, whatever baud does with its body.**
//!
//! `docs/vt-coverage.md` puts image protocols out of scope and then says the
//! thing that is *not* optional: "the `APC` string must still be consumed --
//! that is Tier 0". A terminal that does not swallow the body renders the
//! payload as text, and a kitty graphics command carries base64 by the kilobyte.
//!
//! That row also said "and it is a bug today", which was true when written and
//! is not now. **The claim outlived the defect**, which is the same drift this
//! project keeps finding in its own documents -- a refusal whose reason expired,
//! a gate scoped to the wrong crate, a row no byte could reach. A document
//! asserting a live bug that no longer exists is as wrong as one asserting
//! coverage it does not have, and it is harder to notice because it reads as
//! caution.
//!
//! So the property is pinned here rather than described there: every control
//! string introducer swallows its body, and the text after the terminator still
//! lands on the screen.

#[path = "support/mod.rs"]
mod support;

use baud_core::*;

/// `[[source: williams]]` names the four string types; each is introduced
/// differently and all end at `ST`.
const STRINGS: [(&str, &[u8]); 5] = [
    // A real kitty graphics command: `APC G <control data> ; <base64> ST`.
    (
        "APC G -- kitty graphics",
        b"\x1b_Ga=T,f=100,s=10,v=10;iVBORw0KGgoAAAANSUhEUgAAAAEAAAAB\x1b\\",
    ),
    ("DCS -- device control", b"\x1bPsomething\x1b\\"),
    ("SOS -- start of string", b"\x1bXanything at all\x1b\\"),
    ("PM -- privacy message", b"\x1b^private\x1b\\"),
    // OSC is the one string type baud acts on, and it must still consume.
    ("OSC -- window title", b"\x1b]0;a title\x1b\\"),
];

/// The body never reaches the grid, and what follows it does.
///
/// **Both halves matter.** A parser that ate everything to the end of the read
/// would also pass a "nothing was drawn" assertion, so the text after the
/// terminator is what separates consuming the string from consuming the stream.
#[test]
fn every_control_string_is_consumed_and_the_text_after_it_is_not() {
    for (name, body) in STRINGS {
        let ws = support::CORPUS;
        let mut s = Owned::new(&ws);
        let mut t = Term::new(s.storage(), &ws);

        let mut bytes = body.to_vec();
        bytes.extend_from_slice(b"AFTER");
        t.write_at(BytePos::ZERO, &bytes);
        t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));

        let row = t.row(Row::from_index(0));
        let text: String = row
            .iter()
            .filter_map(|c| c.content.as_codepoint())
            .filter_map(|p| char::from_u32(p.value()))
            .collect();
        assert_eq!(
            text.trim_end(),
            "AFTER",
            "{name}: the body must be swallowed and the text after it must not"
        );
    }
}

/// **A string split across reads is still one string.**
///
/// A kitty graphics payload is measured in kilobytes, so it never arrives in one
/// `read(2)` -- the case the corpus cannot show, because no recording carries an
/// image. A parser that reset at a read boundary would render the tail of every
/// image as text, and it would do so only in production.
#[test]
fn a_control_string_survives_a_read_boundary() {
    let body = b"\x1b_Ga=T,f=100,s=10,v=10;iVBORw0KGgoAAAANSUhEUgAAAAEAAAAB\x1b\\AFTER";
    for chunk in [1usize, 2, 3, 7, 16] {
        let ws = support::CORPUS;
        let mut s = Owned::new(&ws);
        let mut t = Term::new(s.storage(), &ws);
        for part in body.chunks(chunk) {
            t.write_at(BytePos::ZERO, part);
            t.feed(BytePos::ZERO, ReadLen::from_index(part.len()));
        }
        let row = t.row(Row::from_index(0));
        let text: String = row
            .iter()
            .filter_map(|c| c.content.as_codepoint())
            .filter_map(|p| char::from_u32(p.value()))
            .collect();
        assert_eq!(
            text.trim_end(),
            "AFTER",
            "an APC split into {chunk}-byte reads leaked its body"
        );
    }
}

/// A terminal fed `bytes` in reads of `size`, handed to `f` while it lives.
fn fed<R>(bytes: &[u8], size: usize, f: impl FnOnce(&Term<'_>) -> R) -> R {
    let ws = WinSize {
        ws_row: 6,
        ws_col: 40,
    };
    let mut s = Owned::new(&ws);
    let mut t = Term::new(s.storage(), &ws);
    for part in bytes.chunks(size) {
        t.write_at(BytePos::ZERO, part);
        t.feed(BytePos::ZERO, ReadLen::from_index(part.len()));
    }
    f(&t)
}

/// **A bare `ST` closes nothing, and is counted nowhere.**
///
/// `[[source: ecma48]]` § 8.3.143 defines ST only as "the closing delimiter
/// of a control string opened by" APC, DCS, OSC, PM or SOS. With no string
/// open it closes nothing and is no function at all: `[[source: williams]]`
/// draws `9C / (no action)` from anywhere (line 268), says "there is never
/// any work to do for 9C" (972-973), and calls the `ESC \` after a string
/// its `ESC` has already ended "essentially a 'no-op' function" (505-507).
/// `ESC \` fell to `Esc::Unsupported` and was counted, and so was `9C` under
/// ISO 8859-1, from ground and, since a C1 cancels the sequence it
/// interrupts, from inside an escape or control sequence. An embedder reading
/// `unsupported` was told of a function baud lacks where none was asked for.
///
/// Each program is held, at every read size, to the same program with the ST
/// left out, and its counts to zero. One-byte reads walk the graph; a read
/// that holds the whole `ESC \` takes `fast_esc`, which dispatches it without
/// the table. Three controls: `ESC N`, which baud does not implement, is
/// counted in the same harness, so a zero is not a dead counter; an escape
/// whose final `\` follows an intermediate is not ST and is still counted;
/// and an ST that closes a string still closes it, and an ignored string it
/// closes is counted as it was.
#[test]
fn a_bare_st_closes_nothing_and_is_counted_nowhere() {
    let cases: [(&str, &[u8], &[u8]); 7] = [
        ("ESC \\ from ground", b"X\x1b\\Y", b"XY"),
        (
            "ESC \\ after a CSI its ESC cancels",
            b"X\x1b[1;\x1b\\Y",
            b"XY",
        ),
        (
            "ESC \\ after an OSC a CAN cancels",
            b"X\x1b]0;t\x18\x1b\\Y",
            b"X\x1b]0;t\x18Y",
        ),
        (
            "9C from ground under ISO 8859-1",
            b"\x1b%@X\x9cY",
            b"\x1b%@XY",
        ),
        (
            "9C after ESC under ISO 8859-1",
            b"\x1b%@X\x1b\x9cY",
            b"\x1b%@XY",
        ),
        (
            "9C after ESC ( under ISO 8859-1",
            b"\x1b%@X\x1b(\x9cY",
            b"\x1b%@XY",
        ),
        (
            "9C inside a CSI under ISO 8859-1",
            b"\x1b%@X\x1b[1;\x9cY",
            b"\x1b%@XY",
        ),
    ];
    for (name, bytes, without) in cases {
        fed(without, without.len(), |want| {
            assert_eq!(want.unsupported, 0, "{name}: the program without the ST");
            for size in 1..=bytes.len() {
                fed(bytes, size, |got| {
                    assert_eq!(got.unsupported, 0, "{name}: counted, read size {size}");
                    assert_eq!(
                        got.charset_unsupported, 0,
                        "{name}: designated, read size {size}"
                    );
                    assert_eq!(got.title(), want.title(), "{name}: title, read size {size}");
                    assert_eq!(
                        got.cursor(),
                        want.cursor(),
                        "{name}: cursor, read size {size}"
                    );
                    assert!(
                        support::screen(got) == support::screen(want),
                        "{name}: screen, read size {size}"
                    );
                });
            }
        });
    }

    for size in 1..=4 {
        fed(b"X\x1bNY", size, |t| {
            assert_eq!(
                t.unsupported, 1,
                "control: ESC N is counted, read size {size}"
            );
        });
        fed(b"X\x1b#\\Y", size, |t| {
            assert_eq!(
                t.unsupported, 1,
                "control: ESC # \\ is an escape, not ST, read size {size}"
            );
        });
        fed(b"X\x1b(\\Y", size, |t| {
            assert_eq!(
                t.charset_unsupported, 1,
                "control: ESC ( \\ designates a set baud does not name, read size {size}"
            );
        });
    }
    for (name, bytes) in [
        ("OSC closed by ESC \\", b"\x1b]0;t\x1b\\".as_slice()),
        ("OSC closed by 9C", b"\x1b%@\x9d0;t\x9c"),
    ] {
        for size in 1..=bytes.len() {
            fed(bytes, size, |t| {
                assert_eq!(t.title(), b"t", "control: {name} sets it, read size {size}");
                assert_eq!(t.unsupported, 0, "control: {name}, read size {size}");
            });
        }
    }
    for (name, bytes) in [
        ("SOS closed by ESC \\", b"\x1bXab\x1b\\".as_slice()),
        ("SOS closed by 9C", b"\x1b%@\x98ab\x9c"),
    ] {
        for size in 1..=bytes.len() {
            fed(bytes, size, |t| {
                assert_eq!(
                    (t.strings_ignored, t.unsupported),
                    (1, 1),
                    "control: {name} is an ignored string, counted, read size {size}"
                );
            });
        }
    }
}
