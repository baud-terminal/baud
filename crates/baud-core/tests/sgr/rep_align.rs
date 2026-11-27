//! SGR completeness, REP, and DECALN.
//!
//! SGR parameter values are from `[[source: ecma48]]` § 8.3.117; the bright
//! colours and the 256/truecolour forms are xterm extensions from
//! `[[source: ctlseqs]]` § "Character Attributes". DECALN's behaviour is
//! `[[source: vt510-decaln]]`.
//!
//! Style is compared by observing cells rather than by reading style bits, so a
//! change in how a style is packed cannot make these vacuous: two runs that
//! should render alike are asserted equal, and two that should differ are
//! asserted different.

use crate::support;
use baud_core::*;

fn grid(bytes: &[u8]) -> Vec<PackedCell> {
    let ws = support::CORPUS;
    let mut s = Owned::new(&ws);
    let mut t = Term::new(s.storage(), &ws);
    support::feed(&mut t, bytes);
    support::screen(&t)
}

fn cursor(bytes: &[u8]) -> (usize, usize) {
    let ws = support::CORPUS;
    let mut s = Owned::new(&ws);
    let mut t = Term::new(s.storage(), &ws);
    support::feed(&mut t, bytes);
    let (r, c) = t.cursor();
    (r.index(), c.index())
}

// ── SGR attributes that had flags but no parameter ──────────────────────────

/// Blink, conceal and crossed-out each had an `Attrs` bit that nothing ever
/// set. A flag declared and never written is dead weight pretending to be
/// support.
#[test]
fn blink_conceal_and_strike_are_settable() {
    for (name, on, off) in [
        ("blink", &b"\x1b[5m"[..], &b"\x1b[25m"[..]),
        ("conceal", b"\x1b[8m", b"\x1b[28m"),
        ("strike", b"\x1b[9m", b"\x1b[29m"),
    ] {
        let mut set = on.to_vec();
        set.extend_from_slice(b"x");
        assert_ne!(
            grid(&set),
            grid(b"x"),
            "{name}: setting it must change the cell"
        );

        let mut cleared = on.to_vec();
        cleared.extend_from_slice(off);
        cleared.extend_from_slice(b"x");
        assert_eq!(
            grid(&cleared),
            grid(b"x"),
            "{name}: unsetting must restore default"
        );
    }
}

/// Double underline (21) is its own rendition, distinct from single (4) and
/// from plain -- a renderer draws it as two lines. See `sgr/underline.rs` for
/// the attribute; here it is enough that the cell differs from both.
#[test]
fn double_underline_is_distinct() {
    assert_ne!(
        grid(b"\x1b[21mx"),
        grid(b"\x1b[4mx"),
        "double is not single"
    );
    assert_ne!(grid(b"\x1b[21mx"), grid(b"x"), "double is not plain");
    assert_ne!(grid(b"\x1b[4mx"), grid(b"x"), "single is not plain");
}

// ── colours ─────────────────────────────────────────────────────────────────

/// aixterm bright colours are indices 8..15, not a separate kind of colour.
///
/// `[[source: ctlseqs]]`: "aixterm colors are the bright versions of the ISO
/// colors."
#[test]
fn bright_colours_are_palette_indices_eight_to_fifteen() {
    assert_eq!(grid(b"\x1b[90mx"), grid(b"\x1b[38;5;8mx"), "90 == index 8");
    assert_eq!(
        grid(b"\x1b[97mx"),
        grid(b"\x1b[38;5;15mx"),
        "97 == index 15"
    );
    assert_eq!(grid(b"\x1b[100mx"), grid(b"\x1b[48;5;8mx"), "background");
    assert_eq!(grid(b"\x1b[107mx"), grid(b"\x1b[48;5;15mx"));
    // and they are not the dim versions
    assert_ne!(grid(b"\x1b[90mx"), grid(b"\x1b[30mx"));
}

/// 256-colour and truecolour keep working, and a truecolour triple is consumed
/// as one attribute rather than five.
#[test]
fn extended_colour_forms_consume_their_parameters() {
    // If `38;2;r;g;b` were misread as separate attributes, the trailing `1`
    // would not be seen as bold and this would differ.
    assert_eq!(
        grid(b"\x1b[38;2;10;20;30;1mx"),
        grid(b"\x1b[1;38;2;10;20;30mx"),
        "the triple must be one attribute, so the bold still applies"
    );
}

/// SGR 0 cancels everything, per ECMA-48's "cancels the effect of any preceding
/// occurrence of SGR".
#[test]
fn sgr_zero_resets_every_attribute() {
    assert_eq!(
        grid(b"\x1b[1;3;4;5;7;8;9;38;2;1;2;3;48;5;9m\x1b[0mx"),
        grid(b"x")
    );
}

// ── REP ─────────────────────────────────────────────────────────────────────

/// REP repeats the preceding graphic character.
#[test]
fn rep_repeats_the_last_character() {
    assert_eq!(
        grid(b"x\x1b[3b"),
        grid(b"xxxx"),
        "one written plus three more"
    );
    assert_eq!(cursor(b"x\x1b[3b"), (0, 4));
}

/// It must see characters written through the bulk path too — plain ASCII never
/// goes through the per-codepoint path, so recording it only there would make
/// REP repeat something stale.
#[test]
fn rep_sees_bulk_ascii() {
    assert_eq!(grid(b"abc\x1b[2b"), grid(b"abccc"));
}

/// **Every writer leaves REP the character it wrote last**: ECMA-48
/// 8.3.103, "the preceding character in the data stream ... is to be
/// repeated n times". Each run here is long enough for its writer's bulk
/// arm and ends on a character unlike its first, so a record of the run's
/// first character, or of the wrong width, is a different screen.
///
/// Until 2026-10-19 the narrow and wide UTF-8 runs (`Paint::run`) told REP
/// nothing: sixteen `Ä` and `CSI 2 b` wrote no cell.
#[test]
fn rep_repeats_the_last_character_of_every_writer() {
    let runs: [(&str, &str, char); 7] = [
        ("short ASCII", "ab", 'b'),
        ("bulk ASCII", "abcdefghijklmnopq", 'q'),
        (
            "narrow two-byte",
            "\u{C0}\u{C1}\u{C2}\u{C3}\u{C4}\u{C5}\u{C6}\u{C7}",
            '\u{C7}',
        ),
        (
            "narrow three-byte",
            "\u{20A0}\u{20A1}\u{20A2}\u{20A3}\u{20A4}\u{20A5}",
            '\u{20A5}',
        ),
        (
            "wide three-byte",
            "\u{4E00}\u{4E8C}\u{4E09}\u{56DB}\u{4E94}\u{516D}",
            '\u{516D}',
        ),
        (
            "wide four-byte",
            "\u{20000}\u{20001}\u{20002}\u{20003}",
            '\u{20003}',
        ),
        (
            "sixteen U+00C4",
            "\u{C4}\u{C4}\u{C4}\u{C4}\u{C4}\u{C4}\u{C4}\u{C4}\u{C4}\u{C4}\u{C4}\u{C4}\u{C4}\u{C4}\u{C4}\u{C4}",
            '\u{C4}',
        ),
    ];
    let mut red = Vec::new();
    for (name, run, last) in runs {
        let rep = format!("{run}\x1b[2b");
        let typed = format!("{run}{last}{last}");
        if grid(rep.as_bytes()) != grid(typed.as_bytes())
            || cursor(rep.as_bytes()) != cursor(typed.as_bytes())
        {
            red.push(name);
        }
    }
    // GR bytes under ISO 8859-1, a writer of their own.
    let latin1 = b"\x1b%@\xc0\xc1\xc2\xc3\xc4\xc5";
    if grid(&[latin1.as_slice(), b"\x1b[2b"].concat())
        != grid(&[latin1.as_slice(), b"\xc5\xc5"].concat())
    {
        red.push("ISO 8859-1 GR bytes");
    }
    assert!(
        red.is_empty(),
        "REP after {red:?} is not the last character twice"
    );
}

/// **After a cluster REP repeats its first character**, each copy a cell
/// of its own: a bare `e` after `e U+0301`, the woman after a ZWJ family,
/// the first regional indicator after a flag, and a narrow U+2620 after
/// U+2620 U+FE0F widened it (xterm repeats the last codepoint, and
/// nothing when it takes no column).
#[test]
fn rep_after_a_cluster_repeats_its_first_character() {
    let cases: [(&str, &str, &str); 4] = [
        ("e U+0301", "e\u{301}", "ee"),
        (
            "woman ZWJ man",
            "\u{1F469}\u{200D}\u{1F468}",
            "\u{1F469}\u{1F469}",
        ),
        ("a flag", "\u{1F1E9}\u{1F1EA}", "\u{1F1E9}\x1b[m\u{1F1E9}"),
        (
            "U+2620 U+FE0F",
            "\u{2620}\u{FE0F}",
            "\u{2620}\x1b[m\u{2620}",
        ),
    ];
    for (name, cluster, copies) in cases {
        let rep = format!("{cluster}\x1b[2b");
        let typed = format!("{cluster}\x1b[m{copies}");
        assert_eq!(grid(rep.as_bytes()), grid(typed.as_bytes()), "{name}");
        assert_eq!(cursor(rep.as_bytes()), cursor(typed.as_bytes()), "{name}");
    }
}

/// An omitted parameter is 1.
#[test]
fn rep_defaults_to_one() {
    assert_eq!(grid(b"z\x1b[b"), grid(b"zz"));
}

/// With nothing printed yet, REP has nothing to repeat and must do nothing
/// rather than repeat a stale or default cell.
#[test]
fn rep_with_no_preceding_character_is_inert() {
    assert_eq!(grid(b"\x1b[5b"), grid(b""));
    assert_eq!(cursor(b"\x1b[5b"), (0, 0));
}

/// A huge count is bounded by the row rather than spinning.
#[test]
fn rep_with_a_huge_count_is_bounded() {
    let (_, c) = cursor(b"q\x1b[65535b");
    assert!(c <= 199, "cursor stayed on the row, at column {c}");
}

// ── DECALN ──────────────────────────────────────────────────────────────────

/// DECALN fills the screen with `E` and homes the cursor.
#[test]
fn decaln_fills_the_screen_and_homes_the_cursor() {
    let after = grid(b"\x1b[9;9Hjunk\x1b#8");
    assert_eq!(cursor(b"\x1b[9;9Hjunk\x1b#8"), (0, 0), "cursor goes home");

    // Every cell on every row is the same as the first.
    let first = after[0];
    assert!(
        after[..200].iter().all(|c| *c == first),
        "row 0 must be uniform"
    );
    // **The stride is `Term::stride`, not `MAX_COLS`.** `baud-core2` laid the
    // grid out at 2048 cells a row; this core's pitch is fixed from the
    // provisioned width, and leaving 2048 was the change worth +14% on
    // everything that scrolls. Keeping the constant would read row 307 of
    // this grid.
    let stride = support::with_corpus(|t| t.stride().index());
    assert_eq!(after[stride * 30], first, "and so must row 30");
    assert_ne!(first, grid(b"")[0], "the pattern is not blank");
}

/// It resets the margins to the extremes of the page.
///
/// `[[source: vt510-decaln]]`: "DECALN sets the margins to the extremes of the
/// page, and moves the cursor to the home position."
#[test]
fn decaln_resets_the_margins() {
    // After DECALN a line feed at the old region bottom must not scroll a
    // region, because there is no region left.
    let after = grid(b"\x1b[3;5r\x1b#8\x1b[5;1HX\n\nY");
    let never = grid(b"\x1b#8\x1b[5;1HX\n\nY");
    assert_eq!(after, never);
}

/// `ESC #` followed by anything else is consumed, not printed, and does not
/// fill the screen.
#[test]
fn other_hash_sequences_do_not_align() {
    assert_eq!(grid(b"ab\x1b#3cd"), grid(b"abcd"));
}

// ── what used to be dropped ─────────────────────────────────────────────────

/// Every attribute ECMA-48 § 8.3.117 defines and baud claims must actually
/// reach the cell.
///
/// `SgrAttr::fold` used to fall through to "no change" for `Set`, `Unset` and
/// every `Bg`, because `StyleWord` was a bare u32 holding only the foreground.
/// Bold, italic, underline, reverse and all background colours were parsed and
/// discarded — support in the parser, nothing on the screen.
#[test]
fn every_claimed_attribute_reaches_the_cell() {
    for (name, seq) in [
        ("bold", &b"\x1b[1m"[..]),
        ("dim", b"\x1b[2m"),
        ("italic", b"\x1b[3m"),
        ("underline", b"\x1b[4m"),
        ("blink", b"\x1b[5m"),
        ("reverse", b"\x1b[7m"),
        ("conceal", b"\x1b[8m"),
        ("strike", b"\x1b[9m"),
    ] {
        let mut v = seq.to_vec();
        v.extend_from_slice(b"x");
        assert_ne!(grid(&v), grid(b"x"), "{name} must change the rendered cell");
    }
}

/// Background colours are a separate slot from foreground, not the same one.
#[test]
fn background_is_independent_of_foreground() {
    assert_ne!(grid(b"\x1b[41mx"), grid(b"x"), "background must apply");
    assert_ne!(
        grid(b"\x1b[31mx"),
        grid(b"\x1b[41mx"),
        "fg red and bg red are different renditions"
    );
    // Setting one must not disturb the other.
    assert_ne!(grid(b"\x1b[31;41mx"), grid(b"\x1b[31mx"));
    assert_ne!(grid(b"\x1b[31;41mx"), grid(b"\x1b[41mx"));
}

/// Attributes compose rather than replace.
#[test]
fn attributes_accumulate() {
    assert_ne!(
        grid(b"\x1b[1;4mx"),
        grid(b"\x1b[1mx"),
        "bold+underline != bold"
    );
    assert_ne!(grid(b"\x1b[1;4mx"), grid(b"\x1b[4mx"));
    // and order does not matter
    assert_eq!(grid(b"\x1b[1;4mx"), grid(b"\x1b[4;1mx"));
}

/// Unsetting one attribute leaves the others alone.
#[test]
fn unsetting_is_selective() {
    assert_eq!(
        grid(b"\x1b[1;4m\x1b[24mx"),
        grid(b"\x1b[1mx"),
        "removing underline must leave bold"
    );
    assert_eq!(
        grid(b"\x1b[31;41m\x1b[39mx"),
        grid(b"\x1b[41mx"),
        "default fg only"
    );
    assert_eq!(
        grid(b"\x1b[31;41m\x1b[49mx"),
        grid(b"\x1b[31mx"),
        "default bg only"
    );
}

/// 22 clears both bold and dim, per ECMA-48's "neither bold nor faint".
#[test]
fn twenty_two_clears_bold_and_dim() {
    assert_eq!(grid(b"\x1b[1;2m\x1b[22mx"), grid(b"x"));
}
