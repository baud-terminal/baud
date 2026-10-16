#![deny(dead_code)]
//! **Tier 0: a control string is consumed, whatever baud does with its body.**
//!
//! `docs/vt-coverage.md`'s row for kitty graphics, unbuilt, says the
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

        let row = t.row(Row::from_index(0)).expect("a row on the grid");
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
        let row = t.row(Row::from_index(0)).expect("a row on the grid");
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
        // Not `G`: an `APC G` is a kitty graphics command since 2026-10-16,
        // read below.
        ("APC", b"\x1b_Ha\x07X\x1b\\AFTER", b"AFTER"),
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

    // An `APC G` ignores the `BEL` the same way, and is a kitty graphics
    // command, malformed here and refused, not a string ignored.
    let bytes: &[u8] = b"\x1b_Ga\x07X\x1b\\AFTER";
    for size in 1..=bytes.len() {
        assert_eq!(
            left(bytes, size).screen,
            left(b"AFTER", 5).screen,
            "APC G: screen"
        );
        fed(bytes, size, |t| {
            assert_eq!(t.strings_ignored, 0, "APC G: read, read size {size}");
            assert_eq!(t.kitty().refused, 1, "APC G: refused, read size {size}");
            assert_eq!(t.bells, 0, "APC G: no bell, read size {size}");
        });
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

/// The same program under ISO 8859-1: `ESC % @` first, `90` for `ESC P` and
/// `9C` for `ESC \`. Each program here opens with `ESC P` and holds one
/// `ESC \`.
fn latin1_dcs(seven: &[u8]) -> Vec<u8> {
    let body = seven.strip_prefix(b"\x1bP").expect("a 7-bit DCS");
    let st = body.windows(2).position(|w| w == b"\x1b\\").expect("an ST");
    [
        b"\x1b%@\x90".as_slice(),
        &body[..st],
        b"\x9c",
        &body[st + 2..],
    ]
    .concat()
}

/// Each program beside its ISO 8859-1 twin, both held to the same program
/// without the byte under test, at every read size: the reply, the screen
/// and the cursor. The control is that the program without the byte answers
/// `want`, so a harness that saw no reply at all could not pass.
fn same_as_without(cases: &[(&str, &[u8], &[u8])], want: &[u8]) {
    for (name, with, without) in cases {
        for (enc, with, without) in [
            ("UTF-8", with.to_vec(), without.to_vec()),
            ("ISO 8859-1", latin1_dcs(with), latin1_dcs(without)),
        ] {
            let base = left(&without, without.len());
            assert_eq!(base.reply, want, "control: {name}, {enc}, without the byte");
            for size in 1..=with.len() {
                let got = left(&with, size);
                assert_eq!(
                    got.reply, base.reply,
                    "{name}, {enc}: reply, read size {size}"
                );
                assert_eq!(
                    got.screen, base.screen,
                    "{name}, {enc}: screen, read size {size}"
                );
                assert_eq!(
                    got.cursor, base.cursor,
                    "{name}, {enc}: cursor, read size {size}"
                );
            }
        }
    }
}

/// **A `DEL` in a DECRQSS `Pt` is ignored, and the request answers.**
///
/// `[[source: williams]]`'s `dcs passthrough` ignores `7F` (line 388).
/// `[[source: dec-std-070]]` 3.5.4.4 recommends that control codes not used
/// in a command string be ignored, and 3.3.4.1 counts `DEL` a control
/// character; its 3.4.2 and `[[source: vt510-rm-b]]` Table 4-1 ignore it on
/// receipt. `[[source: ecma48]]` F.8.1: it "may be inserted into, or removed
/// from, a CC-data-element without affecting its information content". The
/// DCS body stored it, so `m DEL` named nothing and the answer was
/// `DCS 0 $ r ST`; xterm stores it too, and its `strcmp` fails the same way.
/// Before the `m`, after it, around it, and between the two bytes of `" q`,
/// each at every read size and in both encodings, against the same request
/// without it.
#[test]
fn a_del_in_a_decrqss_pt_is_ignored() {
    let sgr = b"\x1bP$qm\x1b\\Y".as_slice();
    same_as_without(
        &[
            ("DEL after m", b"\x1bP$qm\x7f\x1b\\Y", sgr),
            ("DEL before m", b"\x1bP$q\x7fm\x1b\\Y", sgr),
            ("two DELs around m", b"\x1bP$q\x7fm\x7f\x1b\\Y", sgr),
        ],
        b"\x1bP1$r0m\x1b\\",
    );
    same_as_without(
        &[(
            "DEL inside \" q",
            b"\x1bP$q\"\x7fq\x1b\\Y",
            b"\x1bP$q\"q\x1b\\Y",
        )],
        b"\x1bP1$r0\"q\x1b\\",
    );
}

/// **A format effector in a DECRQSS `Pt` changes nothing.** The six bytes
/// `08-0D` are put to the handler now, `[[source: williams]]`'s `put` (line
/// 388), where the graph ignored them; the DECRQSS handler gives them no
/// meaning, as `[[source: dec-std-070]]` 3.5.4 recommends ("that these
/// characters not affect the interpretation of the control string"), so the
/// answer is the one the graph's drop gave. A guard, green before the change
/// and after it: what it holds is that moving the drop from the graph to the
/// handler moved no reply, and that none of the six is executed -- a `CR`,
/// `LF` or `BS` that ran would move the cursor. xterm answers invalid, its
/// `strcmp` failing on the byte, practice not followed.
#[test]
fn a_format_effector_in_a_decrqss_pt_changes_nothing() {
    let sgr = b"\x1bP$qm\x1b\\Y".as_slice();
    let mut owned: Vec<(String, Vec<u8>)> = Vec::new();
    for v in [0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d] {
        owned.push((
            format!("{v:#04x} after m"),
            [b"\x1bP$qm".as_slice(), &[v], b"\x1b\\Y"].concat(),
        ));
        owned.push((
            format!("{v:#04x} before m"),
            [b"\x1bP$q".as_slice(), &[v], b"m\x1b\\Y"].concat(),
        ));
    }
    let cases: Vec<(&str, &[u8], &[u8])> = owned
        .iter()
        .map(|(n, b)| (n.as_str(), b.as_slice(), sgr))
        .collect();
    same_as_without(&cases, b"\x1bP1$r0m\x1b\\");
    same_as_without(
        &[(
            "CR LF inside \" q",
            b"\x1bP$q\"\r\nq\x1b\\Y",
            b"\x1bP$q\"q\x1b\\Y",
        )],
        b"\x1bP1$r0\"q\x1b\\",
    );
}

/// A DCS opened by `intro` whose `OSC_MAX` printable bytes fill the
/// accumulator, then `tail`, `ST` and `AFTER`, in the encoding asked for,
/// beside what it should draw.
fn full_dcs(intro: &[u8], tail: &[u8], latin1: bool) -> (Vec<u8>, Vec<u8>) {
    let mut body = intro.to_vec();
    body.extend(std::iter::repeat_n(b'x', OSC_MAX));
    body.extend_from_slice(tail);
    body.extend_from_slice(b"\x1b\\AFTER");
    match latin1 {
        false => (body, b"AFTER".to_vec()),
        true => (latin1_dcs(&body), b"\x1b%@AFTER".to_vec()),
    }
}

/// **A format effector in a DCS body reaches the handler, and a `DEL` does
/// not.** No handler here gives either a meaning, so only the accumulator
/// can tell: a DCS this core ignores (`StrKind::Other`, here DECUDK-shaped)
/// whose `OSC_MAX` printable bytes fill it counts one truncation for a byte
/// put after them and none for a byte ignored.
///
/// `[[source: ecma48]]` 8.3.27: DCS's command string "may consist of bit
/// combinations in the range 00/08 to 00/13 and 02/00 to 07/14";
/// `[[source: williams]]`'s `dcs passthrough` puts `00-17,19,1C-1F,20-7E`
/// and ignores `7F` (line 388), and "C0 controls are also passed to the
/// handler" (726). The graph ignored the six and put the `DEL`, the
/// `osc string` row, so a handler could never see the first and always saw
/// the second.
///
/// The control is one more printable byte, which counts one, so the counter
/// is live for this string. The classes the texts conflict on keep their
/// rows and are held here too: a `BEL` is put (one) and `SOH` ignored
/// (none). None of them is executed: the screen and cursor are `AFTER`'s.
#[test]
fn a_format_effector_in_a_dcs_body_reaches_the_handler_and_a_del_does_not() {
    let mut cases: Vec<(String, Vec<u8>, usize)> = vec![
        ("control: one more printable byte".into(), b"x".to_vec(), 1),
        ("DEL".into(), b"\x7f".to_vec(), 0),
        ("BEL, put as before".into(), b"\x07".to_vec(), 1),
        ("SOH, ignored as before".into(), b"\x01".to_vec(), 0),
    ];
    for v in [0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d] {
        cases.push((format!("{v:#04x}"), vec![v], 1));
    }
    for (name, tail, truncated) in &cases {
        for latin1 in [false, true] {
            let (bytes, drawn) = full_dcs(b"\x1bP0;1|", tail, latin1);
            let want = left(&drawn, drawn.len());
            for size in [1, 2, 7, bytes.len()] {
                let got = left(&bytes, size);
                let at = format!("{name}, latin1 {latin1}, read size {size}");
                assert_eq!(got.screen, want.screen, "{at}: screen");
                assert_eq!(got.cursor, want.cursor, "{at}: cursor");
                fed(&bytes, size, |t| {
                    assert_eq!(
                        (t.string_truncations, t.strings_ignored),
                        (*truncated, 1),
                        "{at}: truncations, strings ignored"
                    );
                });
            }
        }
    }
}

/// **An `OSC` and an `APC` keep their rows**, the positive control for the
/// three tests above: the DCS body's `08-0D` and `7F` moved, and the other
/// two bodies did not. `[[source: williams]]`'s `osc string` ignores
/// `00-17` and puts `20-7F` (line 407), so a title drops a `CR` and keeps a
/// `DEL`; an APC, williams' `sos/pm/apc string` (426), still ignores the
/// `CR` and stores the `DEL` it then discards, so only `string_truncations`
/// sees the second. Every read size, both encodings.
#[test]
fn an_osc_and_an_apc_keep_their_rows_for_a_format_effector_and_a_del() {
    let cases: [(&str, &[u8], &[u8]); 2] = [
        ("ESC ]", b"\x1b]0;ti\rt\x7fle\x07X", b"X"),
        (
            "9D under ISO 8859-1",
            b"\x1b%@\x9d0;ti\rt\x7fle\x07X",
            b"\x1b%@X",
        ),
    ];
    for (name, bytes, drawn) in cases {
        let want = left(drawn, drawn.len());
        for size in 1..=bytes.len() {
            let got = left(bytes, size);
            assert_eq!(got.title, b"tit\x7fle", "{name}: title, read size {size}");
            assert_eq!(got.screen, want.screen, "{name}: screen, read size {size}");
            assert_eq!(got.cursor, want.cursor, "{name}: cursor, read size {size}");
        }
    }
    for (tail, truncated) in [(b"\r".as_slice(), 0), (b"\x7f", 1)] {
        let mut apc = b"\x1b_".to_vec();
        apc.extend(std::iter::repeat_n(b'x', OSC_MAX));
        apc.extend_from_slice(tail);
        apc.extend_from_slice(b"\x1b\\AFTER");
        for size in [1, 2, 7, apc.len()] {
            fed(&apc, size, |t| {
                assert_eq!(
                    (t.string_truncations, t.strings_ignored),
                    (truncated, 1),
                    "APC and {tail:?}: read size {size}"
                );
            });
        }
    }
}

/// Each program with GR bytes in its prefix beside its GL spelling, held to
/// it under ISO 8859-1 in all four framings at every read size: the reply,
/// the screen and the cursor. The control is that the GL spelling answers
/// `want`, so a harness that saw no reply could not pass.
fn same_as_gl(cases: &[(&str, &[u8], &[u8])], want: &[u8]) {
    for (name, gr, gl) in cases {
        for ((frame, gr), (_, gl)) in support::latin1_framings(gr)
            .into_iter()
            .zip(support::latin1_framings(gl))
        {
            let base = left(&gl, gl.len());
            assert_eq!(
                base.reply, want,
                "control: {name}, {frame}, the GL spelling"
            );
            for size in 1..=gr.len() {
                let got = left(&gr, size);
                let at = format!("{name}, {frame}, read size {size}");
                assert_eq!(got.reply, base.reply, "{at}: reply");
                assert_eq!(got.screen, base.screen, "{at}: screen");
                assert_eq!(got.cursor, base.cursor, "{at}: cursor");
            }
        }
    }
}

/// **Under ISO 8859-1 a GR byte in a DCS prefix is read as its 7-bit
/// byte, so `ESC P A4 F1 m ST` is a DECRQSS.**
///
/// `[[source: dec-std-070]]` 3.5.4.5 (PDF p. 135): GR graphic characters
/// "in the introducer sequence to a Device Control String, the eighth bit
/// will be ignored, and they will be treated as their 7-bit equivalent".
/// `[[source: williams]]` (462): "In all states, these codes are treated
/// identically to GL codes 20-7F". `[[source: ecma48]]` §9: in an 8-bit code
/// the bit combinations of columns 10 to 15 may represent the contents of a
/// control string, and "the bit combinations in the range 10/00 to 15/14
/// shall have the same meanings as the corresponding bit combinations in the
/// range 02/00 to 07/14". xterm keeps `AsciiOf` of each string byte
/// (`[[source: xterm-reset]]`'s file, 3454-3456) and finds the `$ q` there
/// (`[[source: xterm-misc]]`'s file, 4896-4900). The prefix nodes sent every
/// GR byte to the body, where the string was ignored and the request got no
/// answer.
///
/// `A4` is `$`, `F1` is `q`, `B0-B9` are the digits and `BB` is `;`. `FF`,
/// `7F`'s twin, which `[[source: ecma48]]` §9 stops short of and williams
/// and `[[source: dec-std-070]]` 3.5.4.5 read as `DEL`, is ignored where
/// `DEL` is.
#[test]
fn a_gr_byte_in_a_dcs_prefix_is_read_as_its_seven_bit_byte_under_latin1() {
    same_as_gl(&GR_SGR, b"\x1bP1$r0m\x1b\\");
    same_as_gl(
        &[(
            "A4 F1 \" q: DECSCA",
            b"\x1bP\xa4\xf1\"q\x1b\\Y",
            b"\x1bP$q\"q\x1b\\Y",
        )],
        b"\x1bP1$r0\"q\x1b\\",
    );
}

/// DECRQSS requests for `m` with GR bytes in their prefixes, each beside its
/// GL spelling.
const GR_SGR: [(&str, &[u8], &[u8]); 7] = [
    (
        "A4 F1: $ and q",
        b"\x1bP\xa4\xf1m\x1b\\Y",
        b"\x1bP$qm\x1b\\Y",
    ),
    ("$ F1: q", b"\x1bP$\xf1m\x1b\\Y", b"\x1bP$qm\x1b\\Y"),
    ("A4 q: $", b"\x1bP\xa4qm\x1b\\Y", b"\x1bP$qm\x1b\\Y"),
    (
        "B1 BB B2 A4 F1: the parameters 1;2",
        b"\x1bP\xb1\xbb\xb2\xa4\xf1m\x1b\\Y",
        b"\x1bP1;2$qm\x1b\\Y",
    ),
    (
        "1 BB B2 $ F1: GL and GR mixed",
        b"\x1bP1\xbb\xb2$\xf1m\x1b\\Y",
        b"\x1bP1;2$qm\x1b\\Y",
    ),
    (
        "FF before A4, ignored",
        b"\x1bP\xff\xa4\xf1m\x1b\\Y",
        b"\x1bP$qm\x1b\\Y",
    ),
    (
        "FF between A4 and F1, ignored",
        b"\x1bP\xa4\xff\xf1m\x1b\\Y",
        b"\x1bP$qm\x1b\\Y",
    ),
];

/// **A GR byte in a DCS body, and the same prefixes under UTF-8, are what
/// they were**, the guard beside the test above, green before the change and
/// after it. The body is not the prefix: a GR byte in a `Pt` is stored as
/// it was, the owner's call (`dcs_passthrough`), so `$ q ED` answers
/// invalid. Under UTF-8 the GR bytes are a character's, nothing reads them
/// as a prefix, and each string is ignored: no reply, and only the `Y` drawn.
#[test]
fn a_gr_byte_in_a_dcs_body_or_under_utf8_is_what_it_was() {
    for (frame, bytes) in support::latin1_framings(b"\x1bP$q\xed\x1b\\Y") {
        for size in 1..=bytes.len() {
            assert_eq!(
                left(&bytes, size).reply,
                INVALID,
                "$ q ED, {frame}, read size {size}"
            );
        }
    }
    let want = left(b"Y", 1);
    for (name, gr, _) in GR_SGR {
        for size in 1..=gr.len() {
            let got = left(gr, size);
            let at = format!("UTF-8, {name}, read size {size}");
            assert_eq!(got.reply, b"", "{at}: reply");
            assert_eq!(got.screen, want.screen, "{at}: screen");
            assert_eq!(got.cursor, want.cursor, "{at}: cursor");
            fed(gr, size, |t| {
                assert_eq!(t.strings_ignored, 1, "{at}: one string")
            });
        }
    }
}
