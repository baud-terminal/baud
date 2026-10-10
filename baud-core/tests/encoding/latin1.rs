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
                assert_same_outcome(&format!("{what}, read size {size}"), got, want);
            });
        }
    });
}

/// Everything a control string, a control sequence or a control that cancels
/// either can change, held equal between two terminals: the screen and
/// cursor, the two strings an `OSC` sets, the reply a DECRQSS queues, the
/// image store a Sixel writes, and the counters a string, a designation or a
/// control bumps.
fn assert_same_outcome(at: &str, got: &Term<'_>, want: &Term<'_>) {
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
    assert_eq!(
        got.charset_unsupported, want.charset_unsupported,
        "{at}: designations not named"
    );
    assert_eq!(got.cursor(), want.cursor(), "{at}: cursor");
    assert!(
        support::screen(got) == support::screen(want),
        "{at}: screen"
    );
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

/// A terminal fed `bytes` in two reads, the second starting at byte `at`.
///
/// A read size puts a boundary at each of its multiples, and one-byte reads
/// put one everywhere at once; a split leaves the rest of each read whole,
/// which is the shape the fast paths see, since a one-byte read gives
/// `fast_esc` and `fast_csi` nothing to look ahead at.
fn fed_split<R>(bytes: &[u8], at: usize, f: impl FnOnce(&Term<'_>) -> R) -> R {
    let ws = WinSize {
        ws_row: 6,
        ws_col: 40,
    };
    let mut s = Owned::new(&ws);
    let mut t = Term::new(s.storage(), &ws);
    let (head, rest) = bytes.split_at(at);
    for chunk in [head, rest] {
        t.write_at(BytePos::ZERO, chunk);
        t.feed(BytePos::ZERO, ReadLen::from_index(chunk.len()));
    }
    f(&t)
}

/// Holds `eight` against `seven` at every read size from one byte to the
/// whole program, and at every split into two reads.
fn same_outcome_at_every_split(what: &str, eight: &[u8], seven: &[u8]) {
    fed(seven, seven.len(), |want| {
        for size in 1..=eight.len() {
            fed(eight, size, |got| {
                assert_same_outcome(&format!("{what}, read size {size}"), got, want);
            });
        }
        for at in 1..eight.len() {
            fed_split(eight, at, |got| {
                assert_same_outcome(&format!("{what}, split at byte {at}"), got, want);
            });
        }
    });
}

/// **A C1 control after `ESC`, or inside a `CSI`, acts under ISO 8859-1, and
/// the sequence it interrupts is cancelled.**
///
/// `[[source: williams]]` (lines 612-615): "All C1 controls cancel any escape
/// sequence, control sequence or control string in progress and are executed.
/// Control functions special to this parser, i.e. DCS, SOS, CSI, OSC, PM and
/// APC, cause a transition to their appropriate states. All other C1 control
/// functions (even those with no defined meaning), cause a transition to
/// ground state." The diagram draws those edges from *anywhere* (268, 301,
/// 349, 402, 421), so from `escape`, `escape intermediate` and the `csi`
/// states as well. `escape()` sent every byte in `80-9F` to `Ground` and
/// consumed it, so `ESC 85` ended the escape and lost the NEL; the `csi_*`
/// rows consumed it too, and from `csi param` the sequence carried on, so
/// `CSI 2 ; 85 H` moved the cursor.
#[test]
fn a_c1_after_esc_or_inside_a_csi_acts_under_latin1() {
    // NEL after a bare `ESC`: `cd` at the start of the next line.
    fed(b"\x1b%@X\x1b\x85cd", usize::MAX, |t| {
        assert_eq!(
            t.cursor(),
            (Row::from_index(1), Col::at_index(2)),
            "ESC 85: the NEL ran"
        );
    });

    // IND after an intermediate keeps the column and designates nothing. A
    // C1 that kept the `(` would be read as `ESC ( D`, a set baud does not
    // name, and counted.
    fed(b"\x1b%@X\x1b(\x84q", usize::MAX, |t| {
        assert_eq!(
            t.cursor(),
            (Row::from_index(1), Col::at_index(2)),
            "ESC ( 84: the IND ran and `q` printed under the cell after `X`"
        );
        assert_eq!(t.charset_unsupported, 0, "ESC ( 84: nothing designated");
    });

    // `9B` after `ESC`: a CSI, so `Y` prints red, as after `ESC [ 31 m`.
    let red = fed(b"\x1b%@\x1b[31mY", usize::MAX, support::screen);
    fed(b"\x1b%@\x1b\x9b31mY", usize::MAX, |t| {
        assert!(support::screen(t) == red, "ESC 9B: the CSI ran");
    });

    // `85` inside a CSI's parameters: the CSI is cancelled, so no CUP, and
    // the NEL runs and `H` prints at the start of the next line.
    fed(b"\x1b%@X\x1b[2;\x85H", usize::MAX, |t| {
        assert_eq!(
            t.cursor(),
            (Row::from_index(1), Col::at_index(1)),
            "CSI 2 ; 85 H: the NEL ran and `H` printed"
        );
    });

    // `9B` inside a CSI's parameters begins a fresh one, whose entry clears:
    // `Y` is red, not bold and red as `CSI 1 ; 31 m` would make it.
    fed(b"\x1b%@\x1b[1;\x9b31mY", usize::MAX, |t| {
        assert!(
            support::screen(t) == red,
            "CSI 1 ; 9B: the second CSI alone"
        );
    });

    // `9D` inside a CSI: the CSI is cancelled and an OSC sets the title.
    fed(b"\x1b%@\x1b[1;\x9d0;t\x07", usize::MAX, |t| {
        assert_eq!(t.title(), b"t", "CSI 1 ; 9D: the OSC set the title");
        assert_eq!(t.cursor().1, Col::ZERO, "CSI 1 ; 9D: nothing printed");
    });
}

/// **Every C1 byte, from every state of an escape or control sequence, acts
/// as its 7-bit form would from ground.**
///
/// `[[source: williams]]` (lines 534-537), of `csi entry` and the states
/// after it: "C1 controls will cancel the sequence and then be executed. I
/// imagine this treatment of C1 controls is prompted by the consideration
/// that the 7-bit (ESC Fe) and 8-bit representations of C1 controls should
/// act in the same way. When the first character of the 7-bit
/// representation, ESC, is received, it will cancel the control sequence, so
/// the 8-bit representation should do so as well." So the 7-bit twin is one
/// oracle, the C1 replaced where it stands by `ESC Fe`, whose `ESC` the
/// anywhere `1B` edge takes; and the 7-bit form alone is the other, with the
/// interrupted sequence left out, since a cancelled sequence "will have no
/// effect" (498-500). Each 8-bit program is held against its twin at every
/// read size and every split into two reads, and the twin against the form
/// alone in one read, so each program at every split acts as its control
/// would from ground.
///
/// Exhaustive over `80-9F` and over every state a sequence passes through:
/// `escape` bare and after each kind of intermediate (`(`, `SP`, `#`, `%`,
/// and two of them), `csi entry` opened by `ESC [` and by `9B`, `csi param`
/// after a separator, after a digit (where the two-byte parameter step reads
/// the C1 as the byte after the digit) and after a private marker, `csi
/// intermediate` and `csi ignore`; and an `ESC` inside a control string,
/// which cancels the string and leaves the C1 to `escape`.
///
/// The three tails are read by whatever the C1 began, as in the string test
/// above, and each ends in `[}`, which a national set designated by a stale
/// intermediate would draw as other letters.
#[test]
fn a_c1_in_any_escape_or_control_sequence_acts_as_its_seven_bit_form_under_latin1() {
    let states: [(&str, &[u8]); 16] = [
        ("escape", b"\x1b"),
        ("escape intermediate (", b"\x1b("),
        ("escape intermediate SP", b"\x1b "),
        ("escape intermediate #", b"\x1b#"),
        ("escape intermediate %", b"\x1b%"),
        ("escape, two intermediates", b"\x1b(%"),
        ("csi entry, ESC [", b"\x1b["),
        ("csi entry, 9B", b"\x9b"),
        ("csi param, after a separator", b"\x1b[1;"),
        ("csi param, after a digit", b"\x1b[12"),
        ("csi param, after a marker", b"\x1b[?"),
        ("csi intermediate", b"\x1b[1 "),
        ("csi ignore", b"\x1b[1?"),
        ("ESC inside an OSC", b"\x1b]0;ab\x1b"),
        ("ESC inside a DECRQSS", b"\x1bP$qm\x1b"),
        ("ESC inside an APC", b"\x1b_ab\x1b"),
    ];
    let tails: [&[u8]; 3] = [b"5mY\x07\r\tZ[}", b"0;T\x07\r\tZ[}", b"$qm\x1b\\\r\tZ[}"];
    for (name, open) in states {
        for c in 0x80u8..=0x9f {
            for tail in tails {
                let what = format!("{c:#04x} in {name}, then {}", tail.escape_ascii());
                let eight = [b"\x1b%@X".as_slice(), open, &[c], tail].concat();
                let twin = [b"\x1b%@X".as_slice(), open, &[0x1b, c - 0x40], tail].concat();
                let alone = [b"\x1b%@X".as_slice(), &[0x1b, c - 0x40], tail].concat();
                same_outcome_at_every_split(&what, &eight, &twin);
                fed(&alone, alone.len(), |want| {
                    fed(&twin, twin.len(), |got| {
                        assert_same_outcome(&format!("{what}: the twin, from ground"), got, want);
                    });
                });
            }
        }
    }
}

/// **Under UTF-8 the same bytes inside a sequence are what they were.**
///
/// `80-9F` are continuation bytes there (`docs/vt-coverage.md`, "Encoding"),
/// so the anywhere rule's C1 half cannot apply and none of the thirty-two
/// executes. Each is held, at every split, to the program it has always been
/// equivalent to: after `ESC`, bare or past an intermediate, it ends the
/// escape and is dropped; inside a `CSI`'s parameters it is dropped and the
/// sequence carries on; straight after the introducer it sends the sequence
/// to `csi ignore`, which swallows it to the final. The control, last so that
/// an unfixed tree runs every UTF-8 case before it, is the first case under
/// Latin-1, where the same bytes run a NEL.
#[test]
fn under_utf8_a_c1_byte_in_a_sequence_is_what_it_was() {
    for c in 0x80u8..=0x9f {
        let cases: [(&str, Vec<u8>, &[u8]); 4] = [
            ("after ESC", [b"\x1b".as_slice(), &[c], b"X"].concat(), b"X"),
            (
                "after ESC (",
                [b"\x1b(".as_slice(), &[c], b"X"].concat(),
                b"X",
            ),
            (
                "inside CSI parameters",
                [b"\x1b[31".as_slice(), &[c], b"mX"].concat(),
                b"\x1b[31mX",
            ),
            (
                "after CSI",
                [b"\x1b[".as_slice(), &[c], b"31mX"].concat(),
                b"X",
            ),
        ];
        for (name, eight, seven) in cases {
            same_outcome_at_every_split(&format!("{c:#04x} {name}, under UTF-8"), &eight, seven);
        }
    }
    fed(b"\x1b%@\x1b\x85X", usize::MAX, |t| {
        assert_eq!(
            t.cursor(),
            (Row::from_index(1), Col::at_index(1)),
            "positive control: under Latin-1, ESC 85 is a NEL"
        );
    });
}
