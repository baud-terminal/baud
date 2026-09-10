//! Character width, against the Unicode data pinned in `MANIFEST.toml`.
//!
//! Width is a **wire contract**, not a rendering choice: `ncurses` on the far
//! end computes columns with `wcwidth`, so a terminal that measured differently
//! would drift from the application's model of the screen and the cursor would
//! land in the wrong place. That is why no font enters the computation.
//!
//! The ranges were hand-written until `docs/research/notes/g-text-encoding.md`
//! pointed out they were not the Unicode property they claimed to be: five
//! zero-width ranges covering `U+0300..U+036F` but not Arabic, Hebrew or
//! Devanagari combining marks. They are generated now.

use crate::support;
use baud_core::*;

fn cols(s: &str) -> usize {
    let mut st = Owned::new(&support::CORPUS);
    let mut t = Term::new(st.storage(), &support::CORPUS);
    let bytes = s.as_bytes();
    support::feed(&mut t, bytes);
    t.cursor().1.index()
}

/// Combining marks outside the Latin block occupy no column.
///
/// **This is the bug the old hand-written table had.** It listed
/// `U+0300..U+036F` — Combining Diacritical Marks, the Latin ones — and nothing
/// else in the general category. Arabic, Hebrew and Devanagari marks are all
/// `Mn` and all took a column each, so every one of them desynchronised the
/// cursor against an application counting with `wcwidth`.
#[test]
fn combining_marks_are_zero_width_in_every_script() {
    // base + mark, in four scripts. Each must be one column.
    for (name, s) in [
        ("Latin e + U+0301", "e\u{301}"),
        ("Arabic beh + shadda U+0651", "\u{628}\u{651}"),
        ("Hebrew alef + patah U+05B7", "\u{5d0}\u{5b7}"),
        ("Devanagari ka + virama U+094D", "\u{915}\u{94d}"),
    ] {
        assert_eq!(cols(s), 1, "{name} should be one column");
    }
}

/// Format characters are zero-width, but SOFT HYPHEN is not.
///
/// `U+00AD` is general category `Cf` and would fall out of a naive
/// `Mn|Me|Cf` rule, but every terminal gives it a column. The generator
/// subtracts it explicitly.
#[test]
fn format_characters_are_zero_width_except_soft_hyphen() {
    assert_eq!(cols("a\u{200b}"), 1, "ZERO WIDTH SPACE is Cf");
    assert_eq!(cols("a\u{200d}"), 1, "ZERO WIDTH JOINER is Cf");
    assert_eq!(cols("a\u{ad}"), 2, "SOFT HYPHEN takes a column");
}

/// East Asian Wide and Fullwidth are two columns.
#[test]
fn wide_and_fullwidth_are_two_columns() {
    assert_eq!(cols("中"), 2, "CJK ideograph is W");
    assert_eq!(cols("Ａ"), 2, "FULLWIDTH LATIN A is F");
    assert_eq!(cols("한"), 2, "Hangul syllable is W");
    assert_eq!(cols("🚀"), 2, "U+1F680 is W");
}

/// **Ambiguous width is now actually read.**
///
/// `WidthTable` carried `ambiguous` and `emoji` fields that nothing consulted:
/// `Width::of` took no table at all, and the two `measure` functions that did
/// take one discarded it — one returning a constant zero behind a "model only"
/// comment. A setting nothing acts on is the failure this project already
/// refuses for modes, and it had been sitting in the width path.
///
/// `[[source: uax11]]` § 5: narrow when the context is unavailable, which for a
/// core with no locale is always — so `Narrow` is the default and this is the
/// opt-in.
#[test]
fn ambiguous_width_follows_the_policy() {
    let measure = |a: AmbiguousWidth| {
        let mut st = Owned::new(&support::CORPUS);
        let mut t = Term::new(st.storage(), &support::CORPUS);
        t.set_ambiguous_width(a);
        // U+00B1 PLUS-MINUS SIGN and U+2018 LEFT SINGLE QUOTATION MARK are
        // both East Asian Ambiguous.
        let bytes = "±‘".as_bytes();
        t.write_at(BytePos::ZERO, bytes);
        t.feed(BytePos::ZERO, ReadLen::from_index((bytes).len()));
        t.cursor().1.index()
    };
    assert_eq!(
        measure(AmbiguousWidth::Narrow),
        2,
        "narrow: one column each"
    );
    assert_eq!(measure(AmbiguousWidth::Wide), 4, "wide: two columns each");
}

/// The default is Narrow, matching xterm's `cjkWidth` default of false.
#[test]
fn ambiguous_defaults_to_narrow() {
    let mut st = Owned::new(&support::CORPUS);
    let t = Term::new(st.storage(), &support::CORPUS);
    assert_eq!(t.ambiguous_width(), AmbiguousWidth::Narrow);
}

/// ASCII never consults a table.
#[test]
fn ascii_is_one_column() {
    assert_eq!(cols("hello"), 5);
}

/// **The committed width tables match the pinned Unicode data.**
///
/// `baud-domain/src/width_data.rs` is generated, and `baud-domain` has no build
/// script and no dependencies, so nothing regenerates it automatically. Without
/// this check a Unicode update, or a hand edit, would leave the tables silently
/// stale — which is how the hand-written set got five ranges deep in the first
/// place. A stale table is a test failure here rather than a cursor that lands
/// in the wrong column on someone else's machine.
#[test]
fn width_tables_match_the_pinned_data() {
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/..");
    let out = std::process::Command::new("python3")
        .arg(format!("{root}/scripts/gen-width-tables.py"))
        .arg("--check")
        .current_dir(root)
        .output()
        .expect("python3 to run the generator");
    assert!(
        out.status.success(),
        "width_data.rs is stale -- rerun scripts/gen-width-tables.py\n{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}

/// DECRQM `?2027` answers `3`, permanently set.
///
/// Mode 2027 is grapheme clustering. This answered `0` (not recognized) while
/// clustering was unimplemented, which was the honest answer then: `2` would
/// have told an application the mode existed and was merely off, whereupon it
/// would set it and believe it took effect.
///
/// baud clusters now, per `[[source: uax29]]` and against Unicode's own
/// conformance suite, and has no mode that stops it — so the answer is `3`, the
/// shape Windows Terminal arrived at independently.
#[test]
fn decrqm_reports_grapheme_clustering_as_permanent() {
    let mut s = Owned::new(&support::CORPUS);
    let mut t = Term::new(s.storage(), &support::CORPUS);
    let bytes = b"\x1b[?2027$p";
    t.write_at(BytePos::ZERO, bytes);
    t.feed(BytePos::ZERO, ReadLen::from_index((bytes).len()));
    assert_eq!(t.reply(), b"\x1b[?2027;3$y", "?2027 must answer 3");
}

/// **`Emoji_Presentation=Yes` is two columns, even where EAW says one.**
///
/// Regional indicators are East Asian Width `N` — Neutral, one column — and
/// carry `Emoji_Presentation`. Measured from EAW alone a flag is half as wide
/// as every terminal draws it, and the drift is per-flag, so a line of them
/// walks the cursor steadily out of step with the application.
///
/// Both properties are needed and neither subsumes the other: EAW `W`/`F`
/// covers CJK, which is not emoji, and `Emoji_Presentation` covers this, which
/// EAW calls neutral.
#[test]
fn emoji_presentation_is_wide_even_when_eaw_is_neutral() {
    // A lone regional indicator: EAW N, Emoji_Presentation yes.
    assert_eq!(cols("\u{1f1ef}"), 2, "regional indicator renders as emoji");
    // And a CJK ideograph is still wide for the other reason.
    assert_eq!(cols("中"), 2, "EAW W, no emoji property");
}
