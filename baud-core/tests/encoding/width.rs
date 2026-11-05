//! Character width, against the Unicode data pinned in `MANIFEST.toml`.
//!
//! Width is a **wire contract**, not a rendering choice: `ncurses` on the far
//! end computes columns with `wcwidth`, so a terminal that measured differently
//! would drift from the application's model of the screen and the cursor would
//! land in the wrong place. That is why no font enters the computation.
//!
//! The ranges were hand-written until a reading of UAX #11 against them
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

/// DECRQM `?2027` answers `2`, reset, at power-on.
///
/// Mode 2027 is grapheme cluster mode. This answered `0` (not recognized)
/// while clustering was unimplemented, which was the honest answer then: `2`
/// would have told an application the mode existed and was merely off,
/// whereupon it would set it and believe it took effect. Then `3`, while
/// baud clustered by the extended rules with no mode to stop it. It is a
/// switch now, reset at power-on: a cell holds a legacy cluster, and set,
/// an extended one, as the draft has the mode (`[[source: mode2027]]`;
/// `tests/syllable_columns.rs`).
#[test]
fn decrqm_reports_grapheme_cluster_mode_reset_at_power_on() {
    let mut s = Owned::new(&support::CORPUS);
    let mut t = Term::new(s.storage(), &support::CORPUS);
    let bytes = b"\x1b[?2027$p";
    t.write_at(BytePos::ZERO, bytes);
    t.feed(BytePos::ZERO, ReadLen::from_index((bytes).len()));
    assert_eq!(t.reply(), b"\x1b[?2027;2$y", "?2027 must answer 2");
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

/// A terminal at `a` fed `bytes` in reads ending at `cuts`: its screen, its
/// cursor column, its reply and its count of unsupported parameters.
fn under(
    a: AmbiguousWidth,
    bytes: &[u8],
    cuts: &[usize],
) -> (Vec<PackedCell>, usize, Vec<u8>, usize) {
    let mut st = Owned::new(&support::CORPUS);
    let mut t = Term::new(st.storage(), &support::CORPUS);
    t.set_ambiguous_width(a);
    let mut from = 0;
    for &end in cuts.iter().chain([bytes.len()].iter()) {
        support::feed(&mut t, &bytes[from..end]);
        from = end;
    }
    (
        support::screen(&t),
        t.cursor().1.index(),
        t.reply().to_vec(),
        t.unsupported_param,
    )
}

/// Every read size from one byte up, then every split into two reads: where
/// each read but the last ends.
fn every_cut(n: usize) -> Vec<Vec<usize>> {
    let sizes = (1..=n).map(|size| (size..n).step_by(size).collect());
    let splits = (1..n).map(|at| vec![at]);
    sizes.chain(splits).collect()
}

/// **A Private Use Area character takes one column under either policy**:
/// xterm's default `privateWidth`, which `mk_wcwidth_cjk` reads before its
/// ambiguous table (`[[source: xterm-wcwidth-c]]` 887-888).
/// `[[source: uax11]]` § 6.1 puts all private use in `A` by default, and § 2
/// invites a terminal to tailor it; the embedded icon face, sized to one
/// cell, is the § 4.2 context.
///
/// Each end of the three ranges, and U+E0B0 and U+F0186 from the middle,
/// in runs of one to five, so the write path's three places that read the
/// policy are all reached: four three-byte characters decoded at once, the
/// two or three left over, and one alone; the astral ones take the
/// four-byte path. `±` is the control, two columns under `Wide`, and
/// U+F900, just past the BMP range, is `W` and two columns under both.
/// Each at every read size and every split into two reads, against the
/// whole read.
#[test]
fn private_use_takes_one_column_under_either_policy() {
    let pua = [
        '\u{E000}',
        '\u{E0B0}',
        '\u{F8FF}',
        '\u{F0000}',
        '\u{F0186}',
        '\u{FFFFD}',
        '\u{100000}',
        '\u{10FFFD}',
    ];
    let mut checked = 0;
    for a in [AmbiguousWidth::Narrow, AmbiguousWidth::Wide] {
        let ambiguous = match a {
            AmbiguousWidth::Narrow => 1,
            AmbiguousWidth::Wide => 2,
        };
        for (c, each) in pua
            .iter()
            .map(|&c| (c, 1))
            .chain([('\u{B1}', ambiguous), ('\u{F900}', 2)])
        {
            for run in 1..=5 {
                let s: String = core::iter::repeat_n(c, run).collect();
                let bytes = s.as_bytes();
                let whole = under(a, bytes, &[]);
                assert_eq!(
                    whole.1,
                    run * each,
                    "{a:?}: {run} of U+{:04X}",
                    u32::from(c)
                );
                for cut in every_cut(bytes.len()) {
                    let got = under(a, bytes, &cut);
                    assert!(
                        got.0 == whole.0 && got.1 == whole.1,
                        "{a:?}: {run} of U+{:04X} cut at {cut:?}",
                        u32::from(c)
                    );
                    checked += 1;
                }
            }
        }
    }
    assert!(checked > 1000, "the control: {checked} cuts compared");
}

/// What `DECRQM` answers for private mode `mode` under `a`.
fn rqm(a: AmbiguousWidth, prefix: &str, mode: u16) -> String {
    let bytes = format!("{prefix}\x1b[?{mode}$p");
    String::from_utf8(under(a, bytes.as_bytes(), &[]).2).expect("an ASCII reply")
}

/// **`?1021` reports the policy, `1` under `Wide` and `2` under `Narrow`**,
/// as xterm answers its read-only `cjkWidth` resource with `MdBool`
/// (`[[source: xterm-misc]]` 5845-5853).
/// `[[source: ctlseqs]]`: "Ps = 1 0 2 1  -> cjkWidth resource (read-only),
/// xterm." A full reset (`RIS`) and a soft one (`DECSTR`) leave the
/// policy, and the answer with it, as they leave the widths.
#[test]
fn decrqm_1021_reports_the_ambiguous_width_policy() {
    for prefix in ["", "\x1bc", "\x1b[!p"] {
        assert_eq!(
            rqm(AmbiguousWidth::Narrow, prefix, 1021),
            "\x1b[?1021;2$y",
            "{prefix:?}"
        );
        assert_eq!(
            rqm(AmbiguousWidth::Wide, prefix, 1021),
            "\x1b[?1021;1$y",
            "{prefix:?}"
        );
        let after = format!("{prefix}±");
        assert_eq!(
            under(AmbiguousWidth::Wide, after.as_bytes(), &[]).1,
            2,
            "{prefix:?} keeps the policy"
        );
    }
}

/// **`?1023` answers set, and `?1022` is not recognized**, under either
/// policy. `?1023` reports `privateWidth`, "whether to assume that
/// codes in the Unicode PUA (private use area) are single-width"
/// (`[[source: xterm-man]]` 5202-5206), which baud always does. `?1022`
/// reports an `emojiWidth` baud has no switch for, so it answers `0`:
/// its `Vs16Width` neither narrows on VS15 nor widens a bare listed
/// emoji.
#[test]
fn decrqm_1023_answers_set_and_1022_is_not_recognized() {
    for a in [AmbiguousWidth::Narrow, AmbiguousWidth::Wide] {
        assert_eq!(rqm(a, "", 1023), "\x1b[?1023;1$y", "{a:?}");
        assert_eq!(rqm(a, "", 1022), "\x1b[?1022;0$y", "{a:?}");
    }
}

/// **DECSET and DECRST change none of the three, and are counted**: they are
/// read-only in `[[source: ctlseqs]]`, and xterm's `dpmodes` skips them,
/// "these are readonly, not saved or restored" (`[[source: xterm-reset]]`
/// charproc.c 7998-8003). So `?1021h` does not widen `±`, and `DECRQM` still
/// reports what the embedder configured.
#[test]
fn decset_of_the_width_resources_changes_nothing_and_is_counted() {
    for (a, set) in [(AmbiguousWidth::Narrow, 'h'), (AmbiguousWidth::Wide, 'l')] {
        let want = under(a, "±".as_bytes(), &[]);
        for mode in [1021, 1022, 1023] {
            let bytes = format!("\x1b[?{mode}{set}±");
            let got = under(a, bytes.as_bytes(), &[]);
            assert_eq!(got.1, want.1, "{a:?} ?{mode}{set}: the width stayed");
            assert_eq!(got.3, want.3 + 1, "{a:?} ?{mode}{set}: counted");
        }
        let before = rqm(a, "", 1021);
        assert_eq!(rqm(a, &format!("\x1b[?1021{set}"), 1021), before, "{a:?}");
    }
}
