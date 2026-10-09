//! `ESC % @` / `ESC % G` — switching the input encoding mid-stream.
//!
//! `[[source: ctlseqs]]`: `ESC % @` is "Select default character set. That is
//! ISO 8859-1 (ISO 2022)" and `ESC % G` is "Select UTF-8 character set, ISO
//! 2022". An application may switch at any point, so this is live state.
//!
//! baud refused `ESC % @` until now, on the grounds that it "cannot leave
//! UTF-8". It can: the two encodings differ only in how bytes above `0x7F` are
//! read, and a single-byte encoding needs no decoder state at all.
//!
//! xterm's `utf8` resource is four-valued, and only an explicitly configured
//! `always` makes these sequences inert. By default xterm honours them.

use crate::support;
use baud_core::*;

fn run(bytes: &[u8]) -> (Vec<u32>, Vec<u8>) {
    let ws = support::CORPUS;
    let mut s = Owned::new(&ws);
    let mut t = Term::new(s.storage(), &ws);
    support::feed(&mut t, bytes);
    let cols = t.cursor().1.index();
    let cells = t.viewport()[..cols]
        .iter()
        .filter(|c| !c.content.is_wide_tail())
        .map(|c| c.content.as_codepoint().map_or(0, |c| c.value()))
        .collect();
    (cells, t.reply().to_vec())
}

/// **The same bytes mean different characters in the two encodings.**
///
/// `0xC3 0xA9` is `é` in UTF-8 — one character — and `Ã©` in ISO 8859-1, which
/// is two. That is the whole difference, and it is why an encoding a terminal
/// gets wrong is not a cosmetic problem.
#[test]
fn the_same_bytes_decode_differently_per_encoding() {
    assert_eq!(run(b"\xc3\xa9").0, vec![0xe9], "UTF-8: one `é`");
    assert_eq!(
        run(b"\x1b%@\xc3\xa9").0,
        vec![0xc3, 0xa9],
        "ISO 8859-1: `Ã` then `©`"
    );
}

/// `ESC % G` switches back, mid-stream.
#[test]
fn the_encoding_switches_both_ways() {
    assert_eq!(
        run(b"\x1b%@\xe9\x1b%G\xc3\xa9").0,
        vec![0xe9, 0xe9],
        "Latin-1 `é`, then UTF-8 `é`"
    );
}

/// Every Latin-1 byte above `0x9F` is a character, including ones UTF-8 would
/// reject as a lead byte.
#[test]
fn latin1_has_no_ill_formed_bytes() {
    // 0xFF is never a valid UTF-8 lead byte; in Latin-1 it is `ÿ`.
    assert_eq!(run(b"\x1b%@\xff").0, vec![0xff], "0xFF is `ÿ`");
    assert_eq!(run(b"\xff").0, vec![0xfffd], "and is ill-formed in UTF-8");
}

/// `80..9F` are C1 controls in ISO 8859-1, and are **acted on**, not printed.
///
/// This asserted they were ignored, on the reading that baud "cannot act on
/// 8-bit C1, having chosen UTF-8 as the other mode". True in UTF-8 and not
/// here: in Latin-1 there is no multi-byte sequence for `9B` to be part of.
///
/// `9B A` is `CSI A` — cursor up — so nothing prints and the cursor does not
/// move, being already at the top.
#[test]
fn latin1_c1_bytes_are_controls_not_characters() {
    assert_eq!(
        run(b"\x1b%@\x9bA").0,
        Vec::<u32>::new(),
        "CSI A prints nothing"
    );
    assert_eq!(
        run(b"\x1b%@\x9bA").0,
        run(b"\x1b%@\x1b[A").0,
        "and matches the 7-bit form"
    );
}

/// **DECRQM `?1020` reports the live encoding.**
///
/// It answered `3` (permanently set) while `ESC % @` was refused, and that was
/// true then. Implementing the switch made it false, so it moved in the same
/// commit — a mode report that outlives the fact it reported is exactly the
/// failure `docs/vt-coverage.md` refuses. xterm answers `MdBool` from the live
/// `screen->utf8_mode` for the same reason.
#[test]
fn decrqm_1020_follows_the_live_encoding() {
    assert_eq!(run(b"\x1b[?1020$p").1, b"\x1b[?1020;1$y", "UTF-8: set");
    assert_eq!(
        run(b"\x1b%@\x1b[?1020$p").1,
        b"\x1b[?1020;2$y",
        "Latin-1: reset"
    );
}

// ── detection: reporting, not guessing ──────────────────────────────────────

/// **Pure ASCII is undecidable, and saying so is the point.**
///
/// UTF-8 and ISO 8859-1 agree on every byte below `0x80`, so a sample of only
/// ASCII carries no evidence either way. Answering anyway would be inventing
/// some — and this is the *common* case for a terminal, which is precisely why
/// detection cannot be a default and why baud reports rather than switches.
#[test]
fn ascii_alone_decides_nothing() {
    assert_eq!(InputEncoding::detect(b"hello world"), None);
    assert_eq!(InputEncoding::detect(b""), None);
    assert_eq!(InputEncoding::detect(b"\x1b[1;2H plain"), None);
}

/// A well-formed multi-byte sequence is evidence of UTF-8.
#[test]
fn a_multibyte_sequence_indicates_utf8() {
    assert_eq!(
        InputEncoding::detect("café".as_bytes()),
        Some(InputEncoding::Utf8)
    );
    assert_eq!(
        InputEncoding::detect("中文".as_bytes()),
        Some(InputEncoding::Utf8)
    );
}

/// A byte-order mark is a signature and decides on its own.
#[test]
fn a_bom_is_decisive() {
    assert_eq!(
        InputEncoding::detect(b"\xef\xbb\xbf"),
        Some(InputEncoding::Utf8),
        "the BOM alone, with no other evidence"
    );
}

/// Anything ill-formed as UTF-8 rules UTF-8 out.
///
/// ISO 8859-1 has no ill-formed bytes — all 256 are characters — so it is what
/// remains rather than a guess.
#[test]
fn ill_formed_utf8_indicates_latin1() {
    // 0xFF is never a UTF-8 lead byte; in Latin-1 it is `ÿ`.
    assert_eq!(
        InputEncoding::detect(b"caf\xff"),
        Some(InputEncoding::Latin1)
    );
    // A lone continuation byte.
    assert_eq!(
        InputEncoding::detect(b"a\x80b"),
        Some(InputEncoding::Latin1)
    );
    // An overlong, which is a legal scalar value wrongly encoded — the case
    // `Codepoint::new` structurally cannot see.
    assert_eq!(
        InputEncoding::detect(b"\xe0\x80\xaf"),
        Some(InputEncoding::Latin1)
    );
    // A surrogate.
    assert_eq!(
        InputEncoding::detect(b"\xed\xa0\x80"),
        Some(InputEncoding::Latin1)
    );
}

/// A sequence cut off by the end of the sample is not evidence.
///
/// The next read may complete it. Treating a truncated lead as ill-formed would
/// make detection depend on where the read boundary fell, which is the
/// chunk-invariance failure this project has had before.
#[test]
fn a_truncated_tail_is_not_evidence() {
    assert_eq!(
        InputEncoding::detect(b"ab\xc3"),
        None,
        "a lead byte at the very end decides nothing"
    );
    assert_eq!(
        InputEncoding::detect("中".as_bytes()),
        Some(InputEncoding::Utf8),
        "but a complete one does"
    );
}

/// Detection never runs on its own.
///
/// `Term` does not call `detect`, and feeding bytes that look like Latin-1 does
/// not switch the encoding. An embedder decides, because a wrong guess corrupts
/// text silently and gives the application no way to find out.
#[test]
fn detection_does_not_switch_the_terminal() {
    let ws = support::CORPUS;
    let mut s = Owned::new(&ws);
    let mut t = Term::new(s.storage(), &ws);
    t.write_at(BytePos::ZERO, b"caf\xff");
    t.feed(BytePos::ZERO, ReadLen::from_index((b"caf\xff").len()));
    // Still UTF-8: the 0xFF is a decode error, not a hint.
    assert_eq!(
        t.reply().len(),
        0,
        "no reply, and nothing switched behind the caller's back"
    );
    let cols = t.cursor().1.index();
    let last = t.viewport()[..cols]
        .last()
        .and_then(|c| c.content.as_codepoint())
        .map(|c| c.value());
    assert_eq!(last, Some(0xfffd), "0xFF is U+FFFD under UTF-8, not `ÿ`");
}

// ── 8-bit C1, which only exists outside UTF-8 ───────────────────────────────

/// **`9B` is CSI in ISO 8859-1, and a decode error in UTF-8.**
///
/// `docs/vt-coverage.md` lists 8-bit C1 introducers as out, because "they are
/// UTF-8 continuation bytes". True, and scoped to the encoding without saying
/// so: baud is not always in UTF-8 any more. In Latin-1 there is no ambiguity
/// — `9B` cannot be part of a multi-byte sequence, because there are none.
///
/// `[[source: ctlseqs]]` calls `ESC [` "a 7-bit equivalent to 8-bit" `9B`, so
/// the two are the same operation; the implementation dispatches through the
/// `Escape` state rather than duplicating its table, which is what stops them
/// drifting apart.
#[test]
fn eight_bit_csi_works_in_latin1_and_not_in_utf8() {
    // `9B 3 1 m` is `CSI 3 1 m` — set foreground red — then `X`.
    let latin1 = run(b"\x1b%@\x9b31mX");
    let seven_bit = run(b"\x1b%@\x1b[31mX");
    assert_eq!(latin1.0, seven_bit.0, "8-bit CSI matches the 7-bit form");

    // The same bytes in UTF-8: `9B` is a stray continuation byte.
    let utf8 = run(b"\x9b31mX");
    assert_eq!(
        utf8.0.first().copied(),
        Some(0xfffd),
        "in UTF-8 it is a decode error, not a control"
    );
}

/// The other introducers reach their string states.
#[test]
fn eight_bit_osc_and_dcs_work_in_latin1() {
    // `9D` is OSC: set the title, print nothing.
    let ws = support::CORPUS;
    let mut s = Owned::new(&ws);
    let mut t = Term::new(s.storage(), &ws);
    t.write_at(BytePos::ZERO, b"\x1b%@\x9d0;hi\x07");
    t.feed(
        BytePos::ZERO,
        ReadLen::from_index((b"\x1b%@\x9d0;hi\x07").len()),
    );
    assert_eq!(t.title(), b"hi", "8-bit OSC set the title");
    assert_eq!(t.cursor().1.index(), 0, "and printed nothing");
}

/// `9C` is ST, and `98`/`9E`/`9F` open strings that are consumed.
#[test]
fn eight_bit_string_terminators_are_honoured() {
    // `90` DCS ... `9C` ST: the payload must not reach the screen.
    let g = run(b"\x1b%@\x90q payload \x9cX");
    assert_eq!(
        g.0,
        run(b"\x1b%@X").0,
        "the DCS payload is consumed, only the X prints"
    );
}

/// The screen, the cursor and the unsupported count after feeding `bytes` in
/// reads of `size`, so an 8-bit control can be held against its 7-bit twin at
/// every split.
fn after_reads(bytes: &[u8], size: usize) -> (Vec<PackedCell>, (Row, Col), usize) {
    let ws = support::CORPUS;
    let mut s = Owned::new(&ws);
    let mut t = Term::new(s.storage(), &ws);
    for chunk in bytes.chunks(size) {
        t.write_at(BytePos::ZERO, chunk);
        t.feed(BytePos::ZERO, ReadLen::from_index(chunk.len()));
    }
    (support::screen(&t), t.cursor(), t.unsupported)
}

/// Holds `eight` against `seven` at every read size from one byte to the
/// whole program.
fn same_as_seven_bit(what: &str, eight: &[u8], seven: &[u8]) {
    let want = after_reads(seven, seven.len());
    assert_eq!(
        want.2, 0,
        "{what}: the 7-bit form is itself counted, so it is no control"
    );
    for size in 1..=eight.len() {
        let got = after_reads(eight, size);
        assert_eq!(
            got.2, want.2,
            "{what}: counted as unsupported at read size {size}"
        );
        assert_eq!(got.1, want.1, "{what}: cursor differs at read size {size}");
        assert!(
            got.0 == want.0,
            "{what}: screen differs at read size {size}"
        );
    }
}

/// **IND, NEL, HTS and RI work as 8-bit controls under Latin-1.**
///
/// `[[source: ctlseqs]]` lines 157-171 pair each with its 7-bit form --
/// `84` with `ESC D`, `85` with `ESC E`, `88` with `ESC H`, `8D` with
/// `ESC M` -- and calls the pairs equivalent. The graph routes a Latin-1 C1
/// byte as `escape(b - 0x40)`, but `Act::EscFinal` re-read the raw byte from
/// the arena, so `Esc::parse` saw `0x84` and counted it: only the string and
/// CSI introducers, which never reach `esc_final`, worked.
#[test]
fn eight_bit_ind_nel_hts_and_ri_match_their_seven_bit_forms() {
    for (c1, fin, name) in [
        (0x84u8, b'D', "IND"),
        (0x85, b'E', "NEL"),
        (0x8d, b'M', "RI"),
    ] {
        let eight = [b"\x1b%@\x1b[5;5H".as_slice(), &[c1], b"X"].concat();
        let seven = [b"\x1b%@\x1b[5;5H".as_slice(), &[0x1b, fin], b"X"].concat();
        same_as_seven_bit(name, &eight, &seven);
    }
    // HTS shows only through a tab: set a stop at column 4, home, tab.
    let eight = b"\x1b%@\x1b[1;5H\x88\x1b[1;1H\tX";
    let seven = b"\x1b%@\x1b[1;5H\x1bH\x1b[1;1H\tX";
    same_as_seven_bit("HTS", eight, seven);
}

/// **A C1 byte does not inherit the last escape's intermediate.**
///
/// A C1 control taken from Ground never passes `EscStart`, which is where the
/// intermediate is cleared, so `IND` after `ESC ( 0` reached the designation
/// arm as though it were `ESC ( <84>` and replaced G0 with an unknown set:
/// the line drawing after it came out as `q`. Right after `ESC % @` the
/// leftover `%` sent it to the encoding arm instead, which only counts.
#[test]
fn a_c1_control_does_not_inherit_the_last_escapes_intermediate() {
    same_as_seven_bit(
        "IND after a designation",
        b"\x1b%@\x1b(0q\x84q",
        b"\x1b%@\x1b(0q\x1bDq",
    );
    same_as_seven_bit("IND right after ESC % @", b"\x1b%@\x84X", b"\x1b%@\x1bDX");
}

/// A terminal fed `bytes` in reads of `size`, handed to `f` while it lives.
///
/// Small, so the exhaustive walk below builds thousands of terminals cheaply,
/// and wide enough that nothing here wraps.
fn fed<R>(bytes: &[u8], size: usize, f: impl FnOnce(&Term<'_>) -> R) -> R {
    let ws = WinSize {
        ws_row: 6,
        ws_col: 40,
    };
    let mut s = Owned::new(&ws);
    let mut t = Term::new(s.storage(), &ws);
    for chunk in bytes.chunks(size) {
        t.write_at(BytePos::ZERO, chunk);
        t.feed(BytePos::ZERO, ReadLen::from_index(chunk.len()));
    }
    f(&t)
}

/// Holds `eight` against `seven` at every read size from one byte to the
/// whole program, on everything a control string or a control that cancels
/// one can change: the screen and cursor, the two strings an `OSC` sets, the
/// reply a DECRQSS queues, the image store a Sixel writes, and the counters a
/// string or a control bumps. Unlike `same_as_seven_bit` it does not require
/// the 7-bit form to be uncounted: `ESC X`, `ESC Y` and an `ST` that ends an
/// unrecognised string are counted, and their 8-bit forms must be counted
/// the same.
fn same_outcome_as_seven_bit(what: &str, eight: &[u8], seven: &[u8]) {
    fed(seven, seven.len(), |want| {
        for size in 1..=eight.len() {
            fed(eight, size, |got| {
                let at = format!("{what}, read size {size}");
                assert_eq!(got.title(), want.title(), "{at}: title");
                assert_eq!(got.icon_name(), want.icon_name(), "{at}: icon name");
                assert_eq!(got.reply(), want.reply(), "{at}: reply");
                assert_eq!(
                    got.images().generation(),
                    want.images().generation(),
                    "{at}: image store"
                );
                assert_eq!(got.bells, want.bells, "{at}: bells");
                assert_eq!(
                    got.strings_ignored, want.strings_ignored,
                    "{at}: strings ignored"
                );
                assert_eq!(got.unsupported, want.unsupported, "{at}: unsupported");
                assert_eq!(got.cursor(), want.cursor(), "{at}: cursor");
                assert!(
                    support::screen(got) == support::screen(want),
                    "{at}: screen"
                );
            });
        }
    });
}

/// **A C1 control inside an `OSC` acts under ISO 8859-1, and is not stored.**
///
/// `[[source: williams]]` (lines 612-615): "All C1 controls cancel any escape
/// sequence, control sequence or control string in progress and are executed.
/// Control functions special to this parser, i.e. DCS, SOS, CSI, OSC, PM and
/// APC, cause a transition to their appropriate states." Of an OSC itself
/// (797): "C1 controls will cancel the sequence." The string node ended only
/// on `9C` and took every other C1 as payload, so `85` landed in the window
/// title and the `NEL` never ran.
///
/// A cancelled string is not dispatched here, as with `CAN`, `SUB` and a bare
/// `ESC`, so the title stays unset. Whether a cancel should keep what the
/// string had done is open with the owner (`docs/record.md`).
#[test]
fn a_c1_inside_an_osc_acts_under_latin1_and_is_not_stored() {
    // NEL: `X`, then the string, then `cd` at the start of the next line.
    fed(b"\x1b%@X\x9d0;ab\x85cd\x07", usize::MAX, |t| {
        assert_eq!(t.title(), b"", "NEL: the cancelled title is not set");
        assert_eq!(
            t.cursor(),
            (Row::from_index(1), Col::at_index(2)),
            "NEL ran"
        );
        assert_eq!(t.bells, 1, "NEL: the BEL after it rang, in ground");
    });

    // IND keeps the column: `cd` lands under the cell after `X`.
    fed(b"\x1b%@X\x9d0;ab\x84cd\x07", usize::MAX, |t| {
        assert_eq!(t.title(), b"", "IND: the cancelled title is not set");
        assert_eq!(
            t.cursor(),
            (Row::from_index(1), Col::at_index(3)),
            "IND ran"
        );
    });

    // CSI: `9B 3 1 m` sets the rendition and `cd` prints in it.
    let (screen, cursor) = fed(b"\x1b%@X\x1b[31mcd\x07", usize::MAX, |t| {
        (support::screen(t), t.cursor())
    });
    fed(b"\x1b%@X\x9d0;ab\x9b31mcd\x07", usize::MAX, |t| {
        assert_eq!(t.title(), b"", "CSI: the cancelled title is not set");
        assert!(
            support::screen(t) == screen,
            "CSI: `cd` printed red, as after ESC [ 31 m"
        );
        assert_eq!(t.cursor(), cursor, "CSI: cursor after `cd`");
    });

    // OSC: a second `9D` begins a fresh string, as `osc_start` on entry says;
    // the first one's `0;ab` is gone, not prefixed to it.
    fed(b"\x1b%@\x9d0;ab\x9d0;cd\x9c", usize::MAX, |t| {
        assert_eq!(
            t.title(),
            b"cd",
            "OSC: the second string alone sets the title"
        );
        assert_eq!(t.cursor().1, Col::ZERO, "OSC: nothing printed");
    });
}

/// **Under UTF-8 the same bytes stay in the payload, as they always have.**
///
/// `80-9F` are continuation bytes there (`docs/vt-coverage.md`, "Encoding"),
/// so a title carrying `€` (`E2 82 AC`), `…` (`E2 80 A6`) or `œ` (`C5 93`)
/// has bytes in the C1 range that belong to its characters. Each of the 32
/// is stored byte for byte, alone as well as inside a character.
#[test]
fn under_utf8_c1_bytes_in_a_string_stay_in_the_payload() {
    let title = "\x1b]0;\u{20ac}\u{2026}\u{153}\x07".as_bytes();
    fed(title, usize::MAX, |t| {
        assert_eq!(
            t.title(),
            "\u{20ac}\u{2026}\u{153}".as_bytes(),
            "a UTF-8 title keeps its continuation bytes"
        );
    });
    for c in 0x80u8..=0x9f {
        let bytes = [b"\x1b]0;a".as_slice(), &[c], b"b\x07"].concat();
        for size in 1..=bytes.len() {
            fed(&bytes, size, |t| {
                assert_eq!(
                    t.title(),
                    [b'a', c, b'b'],
                    "{c:#04x} is payload under UTF-8, read size {size}"
                );
                assert_eq!(t.cursor().1, Col::ZERO, "{c:#04x}: nothing printed");
            });
        }
    }
}

/// **Every C1 byte, from every control string, acts as its 7-bit form.**
///
/// `[[source: williams]]` (lines 534-537), of a C1 inside a control sequence:
/// "C1 controls will cancel the sequence and then be executed. ... the 7-bit
/// (ESC Fe) and 8-bit representations of C1 controls should act in the same
/// way. When the first character of the 7-bit representation, ESC, is
/// received, it will cancel the control sequence, so the 8-bit representation
/// should do so as well." The anywhere rule (612-615) says the same of a
/// control string, so the 7-bit program is the oracle: `ESC E` inside an
/// `OSC` cancels it and is `NEL`, so `85` there must be.
///
/// Exhaustive over `80-9F` and over every node a control string passes
/// through: an `OSC` and its icon form, a DECRQSS body (which would queue a
/// reply), a Sixel body (which would store an image), the DCS prefix bare,
/// with parameters and after its `$` intermediate, an unrecognised DCS body,
/// and `SOS`, `PM` and `APC`.
///
/// Three tails follow the C1, because each is read by whatever the C1 opened:
/// `5mY BEL` drives a `CSI` to its final and an executed control to the text
/// after it, `0;T BEL` makes an `OSC` the C1 began set a title, and
/// `$qm ST` makes a DCS it began a DECRQSS that answers. On the unfixed tree
/// 323 of the 352 pairs failed. The 29 that passed were `9C` in the eight
/// string bodies, which already ended there, and `98`, `9E` and `9F` in the
/// strings whose payload is ignored, where an ignored string replaced by
/// another ignored string is invisible by construction; the table gate in
/// `graph.rs` holds those edges.
#[test]
fn a_c1_in_any_control_string_acts_as_its_seven_bit_form_under_latin1() {
    let strings: [(&str, &[u8]); 11] = [
        ("OSC title", b"\x1b]0;ab"),
        ("OSC icon", b"\x1b]1;ab"),
        ("DECRQSS body", b"\x1bP$qm"),
        ("Sixel body", b"\x1bPq#0;2;100;0;0#0~~"),
        ("DCS prefix", b"\x1bP"),
        ("DCS parameters", b"\x1bP1;2"),
        ("DCS $ intermediate", b"\x1bP$"),
        ("DCS unrecognised body", b"\x1bP+qab"),
        ("SOS", b"\x1bXab"),
        ("PM", b"\x1b^ab"),
        ("APC", b"\x1b_ab"),
    ];
    let tails: [&[u8]; 3] = [b"5mY\x07Z", b"0;T\x07Z", b"$qm\x1b\\Z"];
    for (name, open) in strings {
        for c in 0x80u8..=0x9f {
            for tail in tails {
                let eight = [b"\x1b%@X".as_slice(), open, &[c], tail].concat();
                let seven = [b"\x1b%@X".as_slice(), open, &[0x1b, c - 0x40], tail].concat();
                same_outcome_as_seven_bit(&format!("{c:#04x} in {name}"), &eight, &seven);
            }
        }
    }
}
