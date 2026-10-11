//! UTF-8 decoding against the Unicode Standard's own conformance vectors.
//!
//! `[[source: unicode-ch3]]` § 3.9.3, D92, Table 3-7 defines well-formedness
//! exhaustively, and enforcing it needs **two** mechanisms, not one: the
//! first-byte column excludes `C0-C1` and `F5-FF`, and the *second*-byte column
//! is narrowed for exactly four lead bytes — `E0`, `ED`, `F0`, `F4`. The
//! standard flags this itself: the exceptions to the `80..BF` pattern "occur
//! only in the second byte of a sequence".
//!
//! baud implemented only the first half. `utf8_lead` matched `0xe0..=0xef` and
//! `0xf0..=0xf4` and no continuation byte was ever range-checked, so every
//! overlong three- and four-byte form decoded to a real character. The only
//! validity gate was `Codepoint::new`, which **structurally cannot see an
//! overlong** — a non-shortest form still yields a legal scalar value.
//!
//! Conformance clause C10: "a process ... shall treat ill-formed code unit
//! sequences as an error condition and shall not interpret such sequences as
//! characters."

use crate::support;
use baud_core::*;

fn decode(bytes: &[u8]) -> Vec<u32> {
    let ws = support::CORPUS;
    let mut s = Owned::new(&ws);
    let mut t = Term::new(s.storage(), &ws);
    support::feed(&mut t, bytes);
    // The first row, up to the cursor: what the stream actually produced.
    let cols = t.cursor().1.index();
    t.viewport()[..cols]
        .iter()
        // A wide character occupies two cells; the second is the tail marker,
        // not a second codepoint.
        .filter(|c| !c.content.is_wide_tail())
        .map(|c| c.content.as_codepoint().map_or(0, |c| c.value()))
        .collect()
}

const FFFD: u32 = 0xfffd;

/// **An overlong sequence must not decode to the character it encodes.**
///
/// `E0 80 AF` is the canonical path-traversal overlong: it encodes `/` in three
/// bytes rather than one. A decoder that accepts it lets a filter that checked
/// for `/` be bypassed. baud rendered a slash.
#[test]
fn overlong_forms_are_not_characters() {
    for (name, bytes) in [
        ("E0 80 AF (overlong /)", &[0xe0, 0x80, 0xaf][..]),
        ("F0 80 80 AF (overlong /)", &[0xf0, 0x80, 0x80, 0xaf][..]),
        ("E0 9F 80 (overlong U+07C0)", &[0xe0, 0x9f, 0x80][..]),
        (
            "F0 8F BF BF (overlong U+FFFF)",
            &[0xf0, 0x8f, 0xbf, 0xbf][..],
        ),
    ] {
        let got = decode(bytes);
        assert!(
            got.iter().all(|c| *c == FFFD),
            "{name} must be ill-formed, decoded to {got:04x?}"
        );
    }
}

/// Surrogates and values above U+10FFFF are ill-formed.
///
/// These were already rejected — `Codepoint::new` catches them because they are
/// not legal scalar values. Pinned so the second-byte ranges added for the
/// overlong case cannot regress them.
#[test]
fn surrogates_and_out_of_range_are_not_characters() {
    for (name, bytes) in [
        ("ED A0 80 (U+D800)", &[0xed, 0xa0, 0x80][..]),
        ("ED BF BF (U+DFFF)", &[0xed, 0xbf, 0xbf][..]),
        ("F4 90 80 80 (U+110000)", &[0xf4, 0x90, 0x80, 0x80][..]),
    ] {
        let got = decode(bytes);
        assert!(
            got.iter().all(|c| *c == FFFD),
            "{name} must be ill-formed, decoded to {got:04x?}"
        );
    }
}

/// **Error recovery must not eat a valid successor byte.**
///
/// `[[source: unicode-ch3]]` § 3.9.5 is explicit, and gives this exact input: a
/// converter "must not consume the successor bytes as part of the ill-formed
/// subsequence whenever those successor bytes themselves constitute part of a
/// well-formed UTF-8 code unit subsequence", so `<C2 41 42>` "must not return
/// `<U+FFFD>` or `<U+FFFD, U+0042>`".
///
/// baud returned `<U+FFFD, U+FFFD, U+0042>`: the truncated `C2` emitted a
/// replacement and then re-fed the `A` to `control`, whose Ground arm has no
/// printable-ASCII path — ASCII is handled by the run scanner, not the state
/// machine — so the `A` fell through to "stray continuation byte or 0x7f: not
/// text" and produced a second replacement.
#[test]
fn a_truncated_lead_does_not_consume_a_valid_successor() {
    assert_eq!(
        decode(&[0xc2, 0x41, 0x42]),
        vec![FFFD, 0x41, 0x42],
        "the A must survive the truncated C2"
    );
}

/// Recovery already worked for these, and must keep working.
#[test]
fn recovery_resynchronises_on_other_byte_classes() {
    assert_eq!(
        decode(&[0xc2, 0xe4, 0xb8, 0xad]),
        vec![FFFD, 0x4e2d],
        "a truncated lead followed by a valid multi-byte sequence"
    );
}

/// Well-formed input is unaffected by any of the above.
#[test]
fn well_formed_sequences_still_decode() {
    assert_eq!(decode("A".as_bytes()), vec![0x41]);
    assert_eq!(decode("é".as_bytes()), vec![0xe9]);
    assert_eq!(decode("中".as_bytes()), vec![0x4e2d]);
    // The boundary values of the four narrowed lead bytes, all legal.
    assert_eq!(decode("\u{800}".as_bytes()), vec![0x800]); // E0 A0 80
    assert_eq!(decode("\u{d7ff}".as_bytes()), vec![0xd7ff]); // ED 9F BF
    assert_eq!(decode("\u{10000}".as_bytes()), vec![0x10000]); // F0 90 80 80
    assert_eq!(decode("\u{10ffff}".as_bytes()), vec![0x10ffff]); // F4 8F BF BF
}

// ── how baud reports its own encoding ───────────────────────────────────────

/// `ESC % G` selects UTF-8 and `ESC % @` selects ISO 8859-1. Both are honoured.
///
/// Both used to be refusals of a sort: `ESC % G` was counted *satisfied* on the
/// grounds that baud was always UTF-8, and `ESC % @` was counted unsupported on
/// the grounds that it could not leave. The second was wrong -- the encodings
/// differ only in how bytes above `0x7F` are read -- and once it was
/// implemented, "always UTF-8" stopped being true and the first had to move
/// with it. Covered in full by `latin1.rs`; asserted here so the two sequences
/// are not silently dropped.
#[test]
fn both_charset_selectors_are_honoured() {
    let ws = support::CORPUS;
    let mut s = Owned::new(&ws);
    let mut t = Term::new(s.storage(), &ws);
    let feed = |t: &mut Term, bytes: &[u8]| {
        t.write_at(BytePos::ZERO, bytes);
        t.feed(BytePos::ZERO, ReadLen::from_index((bytes).len()));
    };
    feed(&mut t, b"\x1b%@");
    feed(&mut t, b"\x1b%G");
    // core2 read its `unsupported` counter here. What the two cores agreed
    // on, and what this still holds, is that neither selector is counted as
    // a charset designation this core could not name.
    assert_eq!(
        t.charset_unsupported, 0,
        "neither selector is dropped as unsupported"
    );
}

/// **A stream ending mid-sequence is an open state, not an error.**
///
/// `E2 86` is two thirds of `U+2192`. Rendering U+FFFD for it at the end of a
/// read would be wrong: a terminal cannot know the stream ended, and the third
/// byte routinely arrives in the next `read(2)`. So nothing is emitted and the
/// decoder holds the partial sequence.
///
/// The flush happens when a byte arrives that *cannot* continue it — here the
/// `ESC` of a following sequence. Unicode 3.9.5 makes the maximal subpart one
/// U+FFFD, and the offending byte is then reprocessed rather than eaten, which
/// is the recovery bug this file already records for `C2 41 42`.
///
/// This is pinned because it is the reason three generated properties were
/// wrong once the generator could emit multi-byte text: they assumed appending
/// ops to a program was invisible, and against a program ending mid-sequence
/// it is not.
#[test]
fn an_incomplete_sequence_at_the_end_renders_nothing_until_flushed() {
    assert!(
        decode(&[0xe2, 0x86]).is_empty(),
        "pending: nothing is emitted, not even U+FFFD"
    );
    assert_eq!(
        decode(&[0xe2, 0x86, 0x92]),
        vec![0x2192],
        "and it completes when the third byte arrives"
    );
    assert_eq!(
        decode(b"\xe2\x86\x1b[0\"q"),
        vec![FFFD],
        "a byte that cannot continue it flushes exactly one FFFD, and the \
         escape is applied rather than printed"
    );
}

/// **A three-byte lead followed by a byte that cannot continue it hung the
/// parser**, and `ansi`'s `acsc` capability is a hundred bytes of exactly that.
///
/// `run_of` counts whole characters, so `E2 41` counts zero and `chars`
/// consumed `3 * 0`. `step` had already moved the node to `Ground`, so the same
/// byte was looked up again, answered `Char3` again, and scanned to zero again.
/// Nothing in the loop bounded it: a pty can deliver any bytes at all, so this
/// is a hang on hostile input rather than on a corner case.
///
/// `[[source: unicode-ch3]]` 3.9.5 fixes what to emit -- the **maximal
/// subpart** is `E2` alone, because `41` cannot continue it -- so one `U+FFFD`
/// and the `A` is re-dispatched and rendered.
#[test]
fn a_lead_whose_continuation_is_malformed_does_not_hang() {
    for bad in [
        b"\xe2A".as_slice(),        // three-byte lead, ASCII after it
        b"\xe2\x80A".as_slice(),    // two of three, then ASCII
        b"\xc2A".as_slice(),        // two-byte lead
        b"\xf0A".as_slice(),        // four-byte lead
        b"\xe2\xe2\xe2".as_slice(), // leads all the way down
        b"\xf0\x9f\x98".as_slice(), // a truncated emoji at end of stream
    ] {
        support::with_corpus(|t| {
            support::feed(t, bad);
            // Reaching here at all is the assertion: the loop terminated.
            // What it drew is pinned by the row for the two that render.
            let (cols, _) = t.geometry();
            assert!(
                t.cursor().1.index() <= cols.index(),
                "cursor stayed in range"
            );
        });
    }

    // The maximal subpart is one unit, so the `A` survives.
    support::with_corpus(|t| {
        support::feed(t, b"\xe2A");
        let w = t.stride().index();
        let vp = t.viewport();
        assert_eq!(
            vp[0].content.as_codepoint().map(|c| c.value()),
            Some(0xfffd),
            "the lead becomes one replacement"
        );
        assert_eq!(
            vp[1].content.as_codepoint().map(|c| c.value()),
            Some(u32::from(b'A')),
            "and the byte that could not continue it is re-dispatched"
        );
        let _ = w;
    });
}

/// The screen, the cursor, and the unsupported and replacement counts after
/// `bytes` in reads of `size`.
fn state_after(bytes: &[u8], size: usize) -> (Vec<PackedCell>, (Row, Col), usize, usize) {
    let ws = support::CORPUS;
    let mut s = Owned::new(&ws);
    let mut t = Term::new(s.storage(), &ws);
    for chunk in bytes.chunks(size) {
        t.write_at(BytePos::ZERO, chunk);
        t.feed(BytePos::ZERO, ReadLen::from_index(chunk.len()));
    }
    (
        support::screen(&t),
        t.cursor(),
        t.unsupported,
        t.replacements,
    )
}

/// **DEL in ground is ignored, not drawn as U+FFFD.**
///
/// `[[source: dec-std-070]]` 070-3 §3.4.2 (p. 3-17), for 94-character sets,
/// says DEL "shall be ignored"; `[[source: vt510-ch7]]` § 7.2.5 gives "DEL
/// DELETE 127 ... Ignored (unless in CRM)"; `[[source: williams]]`'s ground:
/// "7F will be ignored" with a 94-character set in GL. Under UTF-8, 0x7F is
/// well-formed -- U+007F, a control -- so the replacement character claimed a
/// decoding error that was not there; under ISO 8859-1 it is DEL. baud sent it
/// to `Act::Bad` in every ground. VT52 ground takes the same rule: it has only
/// 94-character sets, and no text read says DEL draws anything there (note AE,
/// D4; Jev 0.94). Each program is held against itself without the DEL, at
/// every read size.
#[test]
fn del_in_ground_is_ignored() {
    for (what, with, without) in [
        ("UTF-8", b"a\x7fb".as_slice(), b"ab".as_slice()),
        ("ISO 8859-1", b"\x1b%@a\x7fb", b"\x1b%@ab"),
        ("VT52", b"\x1b[?2la\x7fb\x1b<", b"\x1b[?2lab\x1b<"),
    ] {
        let want = state_after(without, without.len());
        for size in 1..=with.len() {
            let got = state_after(with, size);
            assert_eq!(got.1, want.1, "{what}: cursor moved at read size {size}");
            assert_eq!(
                got.2, want.2,
                "{what}: unsupported moved at read size {size}"
            );
            assert_eq!(
                got.3, want.3,
                "{what}: replacements moved at read size {size}"
            );
            assert!(
                got.0 == want.0,
                "{what}: screen differs at read size {size}"
            );
        }
    }
}

/// **A DEL in `csi_param` is ignored and the sequence goes on.**
///
/// `[[source: williams]]`'s `csi param` has "event 7F / ignore" and stays
/// (315), so `CSI 5 DEL C X` is `CSI 5 C X`: the cursor moves five and the
/// `X` lands at column 5. The `docs/vt-coverage.md` row "`7F` DEL in
/// `csi_param`" is `done` on this test. Held under both encodings, at every
/// read size, against the same program without the DEL, and at the column.
#[test]
fn a_del_in_csi_param_is_ignored_and_the_sequence_goes_on() {
    for (what, with, without) in [
        ("UTF-8", b"\x1b[5\x7fCX".as_slice(), b"\x1b[5CX".as_slice()),
        ("ISO 8859-1", b"\x1b%@\x1b[5\x7fCX", b"\x1b%@\x1b[5CX"),
    ] {
        let want = state_after(without, without.len());
        assert_eq!(
            want.0[5].content.as_codepoint().map(|c| c.value()),
            Some(u32::from(b'X')),
            "{what}: the control puts X at column 5"
        );
        for size in 1..=with.len() {
            let got = state_after(with, size);
            assert_eq!(
                got.0[5].content.as_codepoint().map(|c| c.value()),
                Some(u32::from(b'X')),
                "{what}: X is not at column 5 at read size {size}"
            );
            assert_eq!(got.1, want.1, "{what}: cursor differs at read size {size}");
            assert_eq!(
                got.2, want.2,
                "{what}: unsupported moved at read size {size}"
            );
            assert!(
                got.0 == want.0,
                "{what}: screen differs at read size {size}"
            );
        }
    }
}

/// **A DEL in any state of an escape or control sequence is ignored, and the
/// sequence goes on**, under both encodings.
///
/// `[[source: williams]]` gives `7F` the *ignore* action, the state staying
/// where it is, in `escape` (286), `escape intermediate` (293), `csi entry`
/// (306), `csi param` (315), `csi intermediate` (328) and `csi ignore`. baud's
/// one `Escape` node is both escape states, and `StrEsc`, the `ESC` inside a
/// control string, is williams' `escape` reached from a string, so `ESC DEL \`
/// there is the `ST` it ends. Three rows of `docs/vt-coverage.md` were
/// `partial` on this: `escape`, `escape_intermediate`, and "`7F` DEL in
/// `csi_entry` or `csi_intermediate`". `escape` ended the sequence at the
/// `DEL`, so `ab ESC DEL 7 cd ESC 8 X` lost its DECSC and drew the `7`;
/// `ESC ( DEL 0` designated nothing; and `csi_entry` and `csi_intermediate`
/// sent the sequence to `csi_ignore`, so `CSI DEL 5 C` moved nothing and
/// `CSI ! DEL p` was no DECSTR.
///
/// Each state is opened, given a `DEL`, and held at every read size and every
/// split into two reads against the same program without it, on four tails
/// that read differently wherever the `DEL` ends something: an escape final
/// and an `SGR` (`5m`), a parameter list (`0;T`), a DECRQSS (`$qm ST`), and
/// a `\`, the `ST` after an `ESC` in a string. The named cases below say what
/// the `DEL` used to cost.
#[test]
fn a_del_in_any_escape_or_control_sequence_is_ignored_and_the_sequence_goes_on() {
    let states: [(&str, &[u8]); 15] = [
        ("escape", b"\x1b"),
        ("escape intermediate (", b"\x1b("),
        ("escape intermediate SP", b"\x1b "),
        ("escape intermediate #", b"\x1b#"),
        ("escape intermediate %", b"\x1b%"),
        ("escape, two intermediates", b"\x1b(%"),
        ("csi entry", b"\x1b["),
        ("csi param, after a separator", b"\x1b[1;"),
        ("csi param, after a digit", b"\x1b[12"),
        ("csi param, after a marker", b"\x1b[?"),
        ("csi intermediate", b"\x1b[1 "),
        ("csi ignore", b"\x1b[1?"),
        ("ESC inside an OSC", b"\x1b]0;ab\x1b"),
        ("ESC inside a DECRQSS", b"\x1bP$qm\x1b"),
        ("ESC inside an APC", b"\x1b_ab\x1b"),
    ];
    let tails: [&[u8]; 4] = [
        b"5mY\x07\r\tZ[}",
        b"0;T\x07\r\tZ[}",
        b"$qm\x1b\\\r\tZ[}",
        b"\\Y\r\tZ[}",
    ];
    for (enc, pre) in [("UTF-8", b"".as_slice()), ("ISO 8859-1", b"\x1b%@")] {
        for (name, open) in states {
            for tail in tails {
                let what = format!("{enc}: DEL in {name}, then {}", tail.escape_ascii());
                let with = [pre, b"X", open, b"\x7f", tail].concat();
                let without = [pre, b"X", open, tail].concat();
                super::latin1::same_outcome_at_every_split(&what, &with, &without);
            }
        }
    }
    // What each departure cost, against its control.
    let named: [(&str, &[u8], &[u8]); 6] = [
        (
            "escape: the DECSC holds",
            b"ab\x1b\x7f7cd\x1b8X",
            b"ab\x1b7cd\x1b8X",
        ),
        (
            "escape intermediate: G0 designated",
            b"\x1b(\x7f0q",
            b"\x1b(0q",
        ),
        ("csi entry: the cursor moves", b"\x1b[\x7f5CX", b"\x1b[5CX"),
        (
            "csi entry: a marker after the DEL",
            b"\x1b[\x7f?25lX",
            b"\x1b[?25lX",
        ),
        (
            "csi intermediate: DECSTR",
            b"\x1b[31m\x1b[!\x7fpY",
            b"\x1b[31m\x1b[!pY",
        ),
        (
            "string ESC: ESC DEL \\ ends the OSC",
            b"\x1b]0;t\x1b\x7f\\Y",
            b"\x1b]0;t\x1b\\Y",
        ),
    ];
    for (what, with, without) in named {
        super::latin1::same_outcome_at_every_split(what, with, without);
    }
    // Positive controls: each named program without the DEL does what its
    // name says, so the comparison above is against a working function.
    let ws = support::CORPUS;
    let mut s = Owned::new(&ws);
    let mut t = Term::new(s.storage(), &ws);
    support::feed(&mut t, b"ab\x1b7cd\x1b8X");
    assert_eq!(decode_of(&t), [0x61, 0x62, 0x58, 0x64], "DECSC: abXd");
    let mut s = Owned::new(&ws);
    let mut t = Term::new(s.storage(), &ws);
    support::feed(&mut t, b"\x1b[?25lX");
    assert!(!t.cursor_visible(), "CSI ? 25 l hides the cursor");
    let mut s = Owned::new(&ws);
    let mut t = Term::new(s.storage(), &ws);
    support::feed(&mut t, b"\x1b]0;t\x1b\\Y");
    assert_eq!(t.title(), b"t", "ESC \\ ends the OSC");
}

/// The first row's characters up to the last written cell, as codepoints.
fn decode_of(t: &Term<'_>) -> Vec<u32> {
    let row = &t.viewport()[..usize::from(support::CORPUS.ws_col)];
    let end = row
        .iter()
        .rposition(|c| c.content.as_codepoint().is_some())
        .map_or(0, |i| i + 1);
    row[..end]
        .iter()
        .map(|c| c.content.as_codepoint().map_or(0, |c| c.value()))
        .collect()
}

/// The designations that load a slot a right half could hold: `ESC ) * +`
/// load G1-G3, with DEC Special Graphics (`0`) and the United Kingdom set
/// (`A`), two of the sets baud translates; the national sets, which it also
/// translates through `nrcs_table`, are not among the candidates. GL stays
/// G0 and ASCII, so the whole cell must match, flags included.
const GR_CANDIDATES: [(&str, &[u8]); 6] = [
    ("ESC ) 0", b"\x1b)0"),
    ("ESC * 0", b"\x1b*0"),
    ("ESC + 0", b"\x1b+0"),
    ("ESC ) A", b"\x1b)A"),
    ("ESC * A", b"\x1b*A"),
    ("ESC + A", b"\x1b+A"),
];

/// DEC Special Graphics in GL, by designation into G0 and by `SO` over G1.
/// Here the character must match and the flags may not: a cell written while
/// GL holds DEC graphics carries `CellFlags::GRAPHICS` whatever the character
/// (note AE Q16(a), decided at 0.98), so the flags are cleared on both sides
/// before comparing. The payloads set no other flag.
const GL_CANDIDATES: [(&str, &[u8]); 2] = [("ESC ( 0", b"\x1b(0"), ("ESC ) 0 SO", b"\x1b)0\x0e")];

/// **Under UTF-8, bytes `A0-FF` are UTF-8 and never a right half** (note AE,
/// AE5 part 1, Q5(a): no GR; Jev 0.92 on the asking and 0.92 on a replicate
/// with the options reordered, decided under the owner's 2026-10-10 rule).
///
/// The texts: `[[source: unicode18-ch3]]` C4, "A process shall interpret a
/// coded character sequence according to the character semantics established
/// by this standard", and D92, which gives every UTF-8 byte sequence its
/// scalar value or makes it ill-formed. Under note AE's P-A, UTF-8 decides
/// 80-FF and no ISO 2022 clause applies; under P-B, ECMA-35 9.1 (p. 20) gives
/// a 7-bit code only "the CL area" and "the GL area". Either way there is no
/// GR. The departure this guards against is xterm's: decoded
/// U+0080-U+00FF are mapped through GR, and raw bytes bypass the decoder,
/// whenever GR is not ASCII (`[[source: xterm-ptydata-c]]` ptydata.c:91-97),
/// so after `ESC * 0`, `C3 A9` shows `Ã©` (note AE Q5(b), derived by reading).
///
/// Three checks, each at every read size, so a split between `C3` and `A9`
/// -- where a raw-byte bypass would show -- is taken:
///
/// * note AE's own example: `ESC * 0 C3 A9` is `é`;
/// * every scalar U+00A0-U+00FF, encoded `C2 A0`..`C3 BF`, leaves the screen,
///   cursor and counters as it does with nothing designated, after each of
///   [`GR_CANDIDATES`] and [`GL_CANDIDATES`];
/// * every raw byte `A0-FF`, each followed by a space, does the same: a lone
///   continuation byte is still U+FFFD and `replacements` +1, not a glyph from
///   the set.
///
/// **A guard: green before and after.** The perturbation that turns it red is
/// in the record: a decoded U+00A1-U+00FE written through `g[2]`. Once
/// LS1R-LS3R exist (stage (c) of note AE §9) they join the candidates, for the
/// bytes `A0-FF` only: what `ESC ~ } |` do to GL under UTF-8 is Q5's other
/// half, still open.
#[test]
fn gr_bytes_under_utf8_are_utf8_whatever_is_designated() {
    for size in 1..=5 {
        let (screen, ..) = state_after(b"\x1b*0\xc3\xa9", size);
        assert_eq!(
            screen[0].content.as_codepoint().map(|c| c.value()),
            Some(0xe9),
            "ESC * 0 C3 A9 must be U+00E9 at read size {size}, not xterm's Ã©"
        );
    }

    let decoded: Vec<u8> = (0xa0u32..=0xff)
        .filter_map(char::from_u32)
        .flat_map(|c| c.encode_utf8(&mut [0; 4]).as_bytes().to_vec())
        .collect();
    assert_eq!(decoded.len(), 192, "96 scalars of two bytes each");
    let raw: Vec<u8> = (0xa0u8..=0xff).flat_map(|b| [b, b' ']).collect();

    let whole: fn(PackedCell) -> PackedCell = |c| c;
    let unflagged: fn(PackedCell) -> PackedCell = |c| PackedCell {
        flags: baud_domain::CellFlags::NONE,
        ..c
    };
    let cases = GR_CANDIDATES
        .iter()
        .map(|&(name, prefix)| (name, prefix, whole))
        .chain(
            GL_CANDIDATES
                .iter()
                .map(|&(name, prefix)| (name, prefix, unflagged)),
        );
    for (name, prefix, cell) in cases {
        for (payload_name, payload) in [("C2 A0..C3 BF", &decoded), ("raw A0..FF", &raw)] {
            // The same shift with nothing designated, so `SO`'s own effect
            // on what follows is not read as the set's.
            let bare: &[u8] = match prefix.last() {
                Some(0x0e) => b"\x0e",
                _ => b"",
            };
            let want = state_after(&[bare, payload.as_slice()].concat(), usize::MAX);
            let want_cells: Vec<PackedCell> = want.0.iter().copied().map(cell).collect();
            let bytes = [prefix, payload.as_slice()].concat();
            for size in 1..=bytes.len() {
                let got = state_after(&bytes, size);
                let what = format!("{payload_name} after {name} at read size {size}");
                assert_eq!(got.1, want.1, "{what}: the cursor moved");
                assert_eq!(got.2, want.2, "{what}: unsupported moved");
                assert_eq!(got.3, want.3, "{what}: replacements moved");
                let got_cells: Vec<PackedCell> = got.0.iter().copied().map(cell).collect();
                if let Some(at) = got_cells.iter().zip(&want_cells).position(|(a, b)| a != b) {
                    panic!(
                        "{what}: the screen differs first at cell {at}: {:?} for {:?}",
                        got_cells[at], want_cells[at]
                    );
                }
            }
        }
    }
}
