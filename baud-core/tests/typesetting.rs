//! **ECMA-48's typesetting functions are counted and write nothing.** The
//! 25 control functions `[[source: ecma48]]` 8.3 gives a `02/00`
//! intermediate and baud does not act on: what each sets is a font, a
//! glyph's size, its spacing, rotation, justification, a paper format or a
//! printer's tray, or it is sent by a device and not received (FNK). Note
//! BF classifies them; the eight not to build have a row each in
//! `docs/vt-coverage.md`, with its reason.

mod support;

use baud_core::*;

const WS: WinSize = WinSize {
    ws_row: 6,
    ws_col: 20,
};

/// Each function's name and final byte, `CSI Pn 02/00 Ft`.
const TYPESETTING: [(&str, u8); 25] = [
    ("GSM", b'B'),
    ("GSS", b'C'),
    ("FNT", b'D'),
    ("TSS", b'E'),
    ("JFY", b'F'),
    ("SPI", b'G'),
    ("QUAD", b'H'),
    ("PFS", b'J'),
    ("SHS", b'K'),
    ("SVS", b'L'),
    ("IGS", b'M'),
    ("IDCS", b'O'),
    ("DTA", b'T'),
    ("FNK", b'W'),
    ("SPQR", b'X'),
    ("SEF", b'Y'),
    ("PEC", b'Z'),
    ("SSW", b'['),
    ("SACS", b'\\'),
    ("STAB", b'^'),
    ("GCC", b'_'),
    ("SCO", b'e'),
    ("SRCS", b'f'),
    ("SCS", b'g'),
    ("SLS", b'h'),
];

/// The screen, the cursor, the count and the reply after `bytes`.
fn after(bytes: &[u8]) -> (Vec<PackedCell>, (Row, Col), usize, Vec<u8>) {
    support::render(&WS, bytes, |t| {
        (
            support::screen(t),
            t.cursor(),
            t.unsupported,
            t.reply().to_vec(),
        )
    })
}

/// **Each is counted once, answers nothing, and leaves the screen and the
/// cursor as the same text without it**, bare, with one parameter and with
/// two, and with text written after it on the line and the next.
#[test]
fn the_typesetting_functions_are_counted_and_write_nothing() {
    let without = after(b"AB\x1b[2;3HCD\r\nEF");
    assert_eq!(without.2, 0);
    for (name, f) in TYPESETTING {
        for params in ["", "1", "2;3"] {
            let seq = format!("AB\x1b[2;3H\x1b[{params} {}CD\r\nEF", f as char);
            let with = after(seq.as_bytes());
            assert_eq!(with.0, without.0, "{name} with {params:?} writes nothing");
            assert_eq!(with.1, without.1, "{name} with {params:?} moves nothing");
            assert!(with.3.is_empty(), "{name} with {params:?} answers nothing");
            assert_eq!(with.2, 1, "{name} with {params:?} is counted once");
        }
    }
}

/// The seventeen baud could act on, which note BF lists to build: the
/// twelve that set a character's size or spacing, on the cell machinery
/// `OSC 66` would bring; JFY and QUAD, which lay a string out between the
/// line home and limit; IDCS, whose 2 names DECDLD's DRCS; GCC, which
/// images characters as one symbol; and SCO, which rotates them.
const TO_BUILD: [&str; 17] = [
    "GSM", "GSS", "TSS", "SPI", "SHS", "SVS", "PEC", "SSW", "SACS", "SRCS", "SCS", "SLS", "JFY",
    "QUAD", "IDCS", "GCC", "SCO",
];

/// **The table gives each of the rest its own row, at `out`, with its
/// reason**: the document checks the code, so a function built later has
/// to move its row.
#[test]
fn the_typesetting_functions_each_have_an_out_row() {
    let src = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../docs/vt-coverage.md"
    ))
    .expect("vt-coverage.md");
    for (name, f) in TYPESETTING
        .into_iter()
        .filter(|(n, _)| !TO_BUILD.contains(n))
    {
        let fin = if f == b'\\' {
            "\\".to_string()
        } else {
            (f as char).to_string()
        };
        let row = src
            .lines()
            .find(|l| {
                let mut cells = l.trim().trim_matches('|').split('|');
                let form = cells.next().unwrap_or("").trim();
                let what = cells.next().unwrap_or("").trim();
                form.starts_with("`CSI") && form.ends_with(&format!(" SP {fin}`")) && {
                    what.split_whitespace().next() == Some(name)
                }
            })
            .unwrap_or_else(|| panic!("{name}, SP {fin}: no row"));
        let status = row.trim().trim_matches('|').split('|').nth(2).unwrap_or("");
        assert!(
            status.trim().starts_with("**out**"),
            "{name}: its row is not out"
        );
    }
}

/// **The control strings note BF read and rows `out` are counted and
/// answered with silence**: DECSTUI, DECPFK and DECPAK, VS Code's
/// `OSC 633`, kitty's `OSC 5522` read and its `OSC 5113` file transfer.
/// `DECRQM ? 5522` answers not recognized.
#[test]
fn the_refused_strings_are_counted_and_answered_with_silence() {
    let without = after(b"AB\x1b[2;3HCD");
    for (what, s) in [
        ("DECSTUI", &b"\x1bP!{0102ABCD\x1b\\"[..]),
        ("DECPFK", b"\x1bP\"x1/1/100/4142/0;\x1b\\"),
        ("DECPAK", b"\x1bP\"y1/1/100/4142/0;\x1b\\"),
        ("OSC 633 A", b"\x1b]633;A\x1b\\"),
        ("OSC 633 D", b"\x1b]633;D;0\x07"),
        ("OSC 633 P", b"\x1b]633;P;Cwd=/tmp\x07"),
        (
            "OSC 5522 read",
            b"\x1b]5522;type=read;dGV4dC9wbGFpbg==\x1b\\",
        ),
        ("OSC 5522 list", b"\x1b]5522;type=read;Lg==\x1b\\"),
        ("OSC 5113 send", b"\x1b]5113;ac=send;id=x\x1b\\"),
    ] {
        let mut bytes = b"AB\x1b[2;3H".to_vec();
        bytes.extend_from_slice(s);
        bytes.extend_from_slice(b"CD");
        let (screen, cursor, unsupported, ignored, reads, reply) =
            support::render(&WS, &bytes, |t| {
                (
                    support::screen(t),
                    t.cursor(),
                    t.unsupported,
                    t.strings_ignored,
                    t.clipboard_reads_refused,
                    t.reply().to_vec(),
                )
            });
        assert_eq!(screen, without.0, "{what} writes nothing");
        assert_eq!(cursor, without.1, "{what} moves nothing");
        assert!(reply.is_empty(), "{what} answers nothing");
        assert_eq!(unsupported, 1, "{what} is counted");
        assert_eq!(ignored, 1, "{what} is a string recognised and not acted on");
        assert_eq!(reads, 0, "{what} is no OSC 52 read");
    }
    let reply = support::render(&WS, b"\x1b[?5522$p", |t| t.reply().to_vec());
    assert_eq!(reply, b"\x1b[?5522;0$y", "DECRQM ? 5522: not recognized");
}
