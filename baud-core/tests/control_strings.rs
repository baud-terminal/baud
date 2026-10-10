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

/// What one program left behind that a BEL could change: the screen, the
/// cursor, the reply and the title. The counts are read off the terminal in
/// a `fed` closure where each test needs them, so none is held in a field.
#[derive(Debug, PartialEq)]
struct Left {
    screen: Vec<PackedCell>,
    cursor: (Row, Col),
    reply: Vec<u8>,
    title: Vec<u8>,
}

fn left(bytes: &[u8], size: usize) -> Left {
    fed(bytes, size, |t| Left {
        screen: support::screen(t),
        cursor: t.cursor(),
        reply: t.reply().to_vec(),
        title: t.title().to_vec(),
    })
}

/// The DECRQSS answer for a request baud cannot name: `DCS 0 $ r ST`.
const INVALID: &[u8] = b"\x1bP0$r\x1b\\";

/// **A BEL inside a DCS body is payload, and only ST ends the string.**
///
/// `[[source: williams]]`'s `dcs passthrough` gives `00-17,19,1C-1F,20-7E` the
/// *put* action (line 388): the byte goes to the control function's handler
/// and the string goes on. `[[source: ecma48]]` 5.6 closes a control string
/// with ST alone, and `[[source: ctlseqs]]` allows BEL beside it "for
/// terminating OSC sequences" (2023-2024), and for no other string. baud's DCS body
/// took the OSC's edge, so a BEL ended a DECRQSS, answered it, and drew what
/// followed it.
///
/// `m BEL` is told apart three ways: put, the payload is `m\x07` and the
/// request is invalid; ignored, it is `m` and valid; a terminator, it is `m`
/// and the `X` after it is drawn. No bell rings: put executes nothing. Every
/// read size, and the 8-bit form under ISO 8859-1. The control is the same
/// request without the BEL, which answers valid, so `INVALID` is not the
/// only reply this harness can see.
#[test]
fn a_bel_in_a_dcs_body_is_payload_and_only_st_ends_it() {
    let control = b"\x1bP$qm\x1b\\";
    for size in 1..=control.len() {
        assert_eq!(
            left(control, size).reply,
            b"\x1bP1$r0m\x1b\\",
            "control: DECRQSS m answers valid, read size {size}"
        );
    }
    let cases: [(&str, &[u8], &[u8]); 4] = [
        ("DECRQSS m BEL", b"\x1bP$qm\x07\x1b\\Y", b"Y"),
        ("DECRQSS m BEL X", b"\x1bP$qm\x07X\x1b\\Y", b"Y"),
        (
            "DECRQSS m BEL X under ISO 8859-1, 90 and 9C",
            b"\x1b%@\x90$qm\x07X\x9cY",
            b"\x1b%@Y",
        ),
        (
            "DECUDK-shaped, ignored",
            b"\x1bP0;1|17/31\x07X\x1b\\Y",
            b"Y",
        ),
    ];
    for (name, bytes, drawn) in cases {
        let want = left(drawn, drawn.len());
        for size in 1..=bytes.len() {
            let got = left(bytes, size);
            assert_eq!(got.screen, want.screen, "{name}: screen, read size {size}");
            assert_eq!(got.cursor, want.cursor, "{name}: cursor, read size {size}");
            let decrqss = name.starts_with("DECRQSS");
            let reply: &[u8] = match decrqss {
                true => INVALID,
                false => b"",
            };
            assert_eq!(got.reply, reply, "{name}: reply, read size {size}");
            fed(bytes, size, |t| {
                assert_eq!(t.bells, 0, "{name}: no bell, read size {size}");
                if !decrqss {
                    assert_eq!(t.strings_ignored, 1, "{name}: one string, read size {size}");
                }
            });
        }
    }
}

/// **A BEL before a DCS body is ignored.**
///
/// `[[source: williams]]` gives `dcs entry`, `dcs param` and `dcs
/// intermediate` each the row `00-17,19,1C-1F / ignore` (lines 354, 361 and
/// 368), and says "C0 controls other than CAN, SUB and ESC are not executed
/// while recognising the first part of a device control string" (565). The prefix
/// nodes ended the string on BEL, so `ESC P BEL $ q m ST` lost its reply and
/// drew `$qm`. Each spelling must answer as the request without the BEL, and
/// ring nothing.
#[test]
fn a_bel_before_a_dcs_body_is_ignored() {
    let plain = b"\x1bP$qm\x1b\\Y";
    let want = left(plain, plain.len());
    assert_eq!(
        want.reply, b"\x1bP1$r0m\x1b\\",
        "control: the request answers"
    );
    let cases: [(&str, &[u8], &[u8]); 3] = [
        ("after ESC P", b"\x1bP\x07$qm\x1b\\Y", plain),
        ("after $", b"\x1bP$\x07qm\x1b\\Y", plain),
        (
            "after 90 under ISO 8859-1",
            b"\x1b%@\x90\x07$qm\x9cY",
            b"\x1b%@\x90$qm\x9cY",
        ),
    ];
    for (name, bytes, without) in cases {
        let want = left(without, without.len());
        assert_eq!(
            want.reply, b"\x1bP1$r0m\x1b\\",
            "control: {name} without the BEL answers"
        );
        for size in 1..=bytes.len() {
            let got = left(bytes, size);
            assert_eq!(got.reply, want.reply, "{name}: reply, read size {size}");
            assert_eq!(got.screen, want.screen, "{name}: screen, read size {size}");
            fed(bytes, size, |t| {
                assert_eq!(t.bells, 0, "{name}: no bell, read size {size}");
            });
        }
    }
}

/// **A BEL inside an SOS, PM or APC is ignored, and only ST ends it.**
///
/// `[[source: williams]]`'s `sos/pm/apc string` ignores `00-17,19,1C-1F,20-7F`
/// (line 426); `[[source: ctlseqs]]` says of APC and PM that "Pt need not be
/// printable characters" (432-433, 2278-2279) and gives BEL as a terminator for OSC
/// only. The three shared the OSC's body node, so a BEL ended them and the
/// body after it was drawn.
///
/// *Ignored*, not put: an APC whose `OSC_MAX` printable bytes fill the
/// accumulator is not truncated by the BEL after them, where a put would
/// count one. The control is the same APC one byte longer, which does count,
/// so the counter is live for this string.
#[test]
fn a_bel_in_sos_pm_or_apc_is_ignored_and_only_st_ends_it() {
    let cases: [(&str, &[u8], &[u8]); 4] = [
        ("SOS", b"\x1bXa\x07X\x1b\\AFTER", b"AFTER"),
        ("PM", b"\x1b^a\x07X\x1b\\AFTER", b"AFTER"),
        ("APC", b"\x1b_Ga\x07X\x1b\\AFTER", b"AFTER"),
        (
            "APC under ISO 8859-1, 9F and 9C",
            b"\x1b%@\x9fa\x07X\x9cAFTER",
            b"\x1b%@AFTER",
        ),
    ];
    for (name, bytes, drawn) in cases {
        let want = left(drawn, drawn.len());
        for size in 1..=bytes.len() {
            let got = left(bytes, size);
            assert_eq!(got.screen, want.screen, "{name}: screen, read size {size}");
            fed(bytes, size, |t| {
                assert_eq!(t.strings_ignored, 1, "{name}: one string, read size {size}");
                assert_eq!(t.bells, 0, "{name}: no bell, read size {size}");
            });
        }
    }

    // `OSC_MAX` printable bytes and a BEL, against `OSC_MAX + 1` and none.
    let mut full = b"\x1b_".to_vec();
    full.extend(std::iter::repeat_n(b'x', OSC_MAX));
    let mut over = full.clone();
    full.push(0x07);
    over.push(b'x');
    for v in [&mut full, &mut over] {
        v.extend_from_slice(b"\x1b\\AFTER");
    }
    fed(&over, over.len(), |t| {
        assert_eq!(
            t.string_truncations, 1,
            "control: one byte past the accumulator counts"
        );
    });
    for size in [1, 2, 7, full.len()] {
        fed(&full, size, |t| {
            assert_eq!(
                t.string_truncations, 0,
                "a full APC and a BEL: read size {size}"
            );
            assert_eq!(
                t.strings_ignored, 1,
                "a full APC and a BEL: one string, read size {size}"
            );
        });
    }
}

/// **A BEL still ends an OSC**, the one string `[[source: ctlseqs]]` lets it
/// end: "XTerm accepts either BEL or ST for terminating OSC sequences"
/// (2023-2024). The positive control for the three tests above, at every read
/// size and in both encodings: the title is set, the text after the BEL is
/// drawn, and nothing rings.
#[test]
fn a_bel_still_ends_an_osc() {
    let cases: [(&str, &[u8], &[u8]); 2] = [
        ("ESC ]", b"\x1b]0;t\x07X", b"X"),
        ("9D under ISO 8859-1", b"\x1b%@\x9d0;t\x07X", b"\x1b%@X"),
    ];
    for (name, bytes, drawn) in cases {
        let want = left(drawn, drawn.len());
        for size in 1..=bytes.len() {
            let got = left(bytes, size);
            assert_eq!(got.title, b"t", "{name}: title, read size {size}");
            assert_eq!(got.screen, want.screen, "{name}: screen, read size {size}");
            fed(bytes, size, |t| {
                assert_eq!(t.bells, 0, "{name}: no bell, read size {size}");
            });
        }
    }
}
