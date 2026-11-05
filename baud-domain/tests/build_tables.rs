//! **Each check the build modules make on the pinned files fires**: each
//! module, run here on its files with one row changed, reports the failure
//! that change must cause, and reports nothing on the files as pinned.
//! `build/bidi.rs`: a dropped `@missing` line, a class outside
//! PropertyValueAliases.txt's `bc` set, an overlapping row, a known code
//! point with another class, a mirroring glyph on a character that is not
//! mirrored, a bracket that is not its pair's pair, a canonical
//! decomposition moved, a Box Drawing name with no counterpart.
//! `build/grapheme.rs`, `build/width.rs` and `build/linebreak.rs`: a value
//! outside its alias set, a GCB value `GraphemeClass` has no class for, an
//! emoji-data.txt of another version, a General_Category that names a
//! group or leaves a code point out, a header default the header does not
//! give, and the rules the tables are written by -- width's zero over wide
//! over ambiguous, and its two exclusions, U+00AD and
//! Prepended_Concatenation_Mark. `build/caseless.rs`, `build/search.rs`,
//! `build/sequence.rs`, `build/joining.rs` and `build/ignorable.rs`: each
//! positive control the Python generators asserted, each count made exact,
//! a status, type or group outside its set, a duplicate, a Hangul
//! syllable given a decomposition, the derived joining file disagreeing,
//! and the perfect hash's seeds tried from 0 as the table was found. The
//! modules only compute: they read
//! through the function they are given and write nothing, so running them
//! here touches no `OUT_DIR`.

#[path = "../build/bidi.rs"]
mod bidi;
#[path = "../build/caseless.rs"]
mod caseless;
#[path = "../build/grapheme.rs"]
mod grapheme;
#[path = "../build/ignorable.rs"]
mod ignorable;
#[path = "../build/joining.rs"]
mod joining;
#[path = "../build/linebreak.rs"]
mod linebreak;
#[path = "../build/search.rs"]
mod search;
#[path = "../build/sequence.rs"]
mod sequence;
#[path = "../build/ucd.rs"]
mod ucd;
#[path = "../build/width.rs"]
mod width;

use baud_tablegen::{Emit, Errors, Kind};

fn pinned(file: &str) -> std::io::Result<String> {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../docs/research/sources");
    std::fs::read_to_string(format!("{dir}/{file}"))
}

type Build = fn(&dyn Fn(&str) -> std::io::Result<String>) -> Result<Emit, Errors>;

/// A build whose `file` has its first `find` replaced.
fn perturbed(build: Build, file: &str, find: &str, replace: &str) -> Result<Emit, Errors> {
    let read = |f: &str| {
        let text = pinned(f)?;
        if f != file {
            return Ok(text);
        }
        assert!(text.contains(find), "{file} has no {find:?}");
        Ok(text.replacen(find, replace, 1))
    };
    build(&read)
}

/// The failures of `build` when `file` has its first `find` replaced.
fn failures_of(build: Build, file: &str, find: &str, replace: &str) -> Vec<(Kind, String)> {
    match perturbed(build, file, find, replace) {
        Ok(_) => Vec::new(),
        Err(errs) => errs
            .list()
            .iter()
            .map(|e| (e.kind, e.to_string()))
            .collect(),
    }
}

/// The failures of the bidi build whose `file` has its first `find`
/// replaced.
fn failures(file: &str, find: &str, replace: &str) -> Vec<(Kind, String)> {
    failures_of(bidi::build, file, find, replace)
}

/// Each case is `(build, file, find, replace, kind, says)`: the build with
/// `file`'s first `find` replaced must report a `kind` failure saying `says`.
type Case<'a> = (Build, &'a str, &'a str, &'a str, Kind, &'a str);

fn each_fires(cases: &[Case<'_>]) {
    let mut missed = Vec::new();
    for case in cases {
        let (build, file, find, replace, kind, says) = *case;
        let got = failures_of(build, file, find, replace);
        if !got.iter().any(|(k, m)| *k == kind && m.contains(says)) {
            missed.push(format!(
                "{file}: {find:?} -> {replace:?}: no {kind:?} saying {says:?} in {got:#?}"
            ));
        }
    }
    assert!(missed.is_empty(), "{}", missed.join("\n"));
}

/// The bytes of `emit`'s `<name>.bin`.
fn bin(emit: &Emit, name: &str) -> Vec<u8> {
    let file = format!("{name}.bin");
    emit.files()
        .into_iter()
        .find(|(f, _)| *f == file)
        .unwrap_or_else(|| panic!("no {file}"))
        .1
        .to_vec()
}

const CLASSES: &str = "unicode-derivedbidiclass.txt";
const DATA: &str = "unicode-unicodedata.txt";
const MIRRORING: &str = "unicode-bidimirroring.txt";
const BRACKETS: &str = "unicode-bidibrackets.txt";
const ALIASES: &str = "unicode-propertyvaluealiases.txt";

#[test]
fn the_pinned_files_build_the_bidi_tables() {
    let emit = bidi::build(&pinned).unwrap_or_else(|e| panic!("{e}"));
    let files: Vec<String> = emit.files().into_iter().map(|(f, _)| f).collect();
    assert_eq!(files, ["bidi_data.rs", "bidi_index.bin", "bidi_blocks.bin"]);
    assert!(emit.rust().contains("one of 122 distinct blocks"));
    assert_eq!(
        bidi::INPUTS.map(|i| i.file),
        [ALIASES, CLASSES, DATA, MIRRORING, BRACKETS]
    );
}

#[test]
fn each_check_the_bidi_build_makes_fires() {
    let cases: &[(&str, &str, &str, Kind, &str)] = &[
        // The positive control on the version wired in.
        (
            CLASSES,
            "# DerivedBidiClass-17.0.0.txt",
            "# DerivedBidiClass-16.0.0.txt",
            Kind::Header,
            "data/unicode-derivedbidiclass.txt:1:1",
        ),
        // 24 `@missing` lines.
        (
            CLASSES,
            "# @missing: 20A0..20CF; European_Terminator",
            "#",
            Kind::Count,
            "23 @missing lines, 24 expected",
        ),
        (
            CLASSES,
            "# @missing: 20A0..20CF; European_Terminator",
            "#",
            Kind::Control,
            "U+20CF is Bidi_Class L, ET expected",
        ),
        // A class the alias file does not list, in a row and in a default.
        (
            CLASSES,
            "05D0..05EA    ; R #",
            "05D0..05EA    ; RTL #",
            Kind::UnknownValue,
            "data/unicode-derivedbidiclass.txt:1243:17: bc `RTL` is not one of the 23 values of data/unicode-propertyvaluealiases.txt:106-128",
        ),
        (
            CLASSES,
            "# @missing: 0590..05FF; Right_To_Left",
            "# @missing: 0590..05FF; Right_To_Lft",
            Kind::UnknownValue,
            "`Right_To_Lft`",
        ),
        // Two rows over one code point.
        (
            CLASSES,
            "05EF..05F2    ; R #",
            "05EF..05F2    ; R\n05F0..05F2    ; AL #",
            Kind::Overlap,
            "U+05F0 is already set by line 1244",
        ),
        // A known code point's class.
        (
            CLASSES,
            "# @missing: 0590..05FF; Right_To_Left",
            "# @missing: 0590..05FF; Left_To_Right",
            Kind::Control,
            "U+05FF is Bidi_Class L, R expected",
        ),
        // The alias file's `bc` set is the table's order.
        (
            ALIASES,
            "bc ; WS                               ; White_Space\n",
            "",
            Kind::Invariant,
            "[\"WS\"] are not values",
        ),
        // UnicodeData.txt: its count, its Y/N, and the mirrored controls.
        (
            DATA,
            "0041;LATIN CAPITAL LETTER A;Lu;0;L;;;;;N;;;;0061;\n",
            "",
            Kind::Count,
            "40554 UnicodeData.txt entries, 40555 expected",
        ),
        (
            DATA,
            "0028;LEFT PARENTHESIS;Ps;0;ON;;;;;Y;",
            "0028;LEFT PARENTHESIS;Ps;0;ON;;;;;X;",
            Kind::UnknownValue,
            "Bidi_Mirrored `X`",
        ),
        (
            DATA,
            "0028;LEFT PARENTHESIS;Ps;0;ON;;;;;Y;",
            "0028;LEFT PARENTHESIS;Ps;0;ON;;;;;N;",
            Kind::Control,
            "U+0028 is Bidi_Mirrored N",
        ),
        (
            DATA,
            "0028;LEFT PARENTHESIS;Ps;0;ON;;;;;Y;",
            "0028;LEFT PARENTHESIS;Ps;0;ON;;;;;N;",
            Kind::Invariant,
            "U+0028 has a mirroring glyph and is not Bidi_Mirrored",
        ),
        (
            DATA,
            "4E00;<CJK Ideograph, First>",
            "4E00;<CJK Ideograph>",
            Kind::FirstWithoutLast,
            "does not follow its First",
        ),
        // BidiMirroring.txt: 428 glyphs, each once, and none for U+2211.
        (
            MIRRORING,
            "0028; 0029 # LEFT PARENTHESIS\n",
            "",
            Kind::Count,
            "427 mirroring glyphs, 428 expected",
        ),
        (
            MIRRORING,
            "0029; 0028 #",
            "0028; 0029 #",
            Kind::Duplicate,
            "U+0028 is already set by line 76",
        ),
        (
            MIRRORING,
            "0028; 0029 #",
            "2211; 2211\n0028; 0029 #",
            Kind::Control,
            "U+2211's mirroring glyph is Some(2211), None expected",
        ),
        // BidiBrackets.txt: 128 brackets, 64 open, each its pair's pair.
        (
            BRACKETS,
            "0028; 0029; o",
            "0028; 0029; n",
            Kind::Count,
            "127 paired brackets, 128 expected",
        ),
        (
            BRACKETS,
            "0028; 0029; o",
            "0028; 0029; n",
            Kind::Count,
            "63 opening brackets, 64 expected",
        ),
        (
            BRACKETS,
            "0028; 0029; o",
            "0028; 0029; x",
            Kind::UnknownValue,
            "Bidi_Paired_Bracket_Type `x`",
        ),
        (
            BRACKETS,
            "0029; 0028; c",
            "0029; 005B; c",
            Kind::Invariant,
            "bracket U+0029 is not its pair's pair",
        ),
        (
            BRACKETS,
            "0029; 0028; c",
            "0029; 005B; c",
            Kind::Invariant,
            "bracket U+0029 is not its pair's mirroring glyph",
        ),
        // BD16's two canonical equivalents.
        (
            DATA,
            "2329;LEFT-POINTING ANGLE BRACKET;Ps;0;ON;3008;",
            "2329;LEFT-POINTING ANGLE BRACKET;Ps;0;ON;3009;",
            Kind::Invariant,
            "the brackets' canonical decompositions are",
        ),
        // Box Drawing: each name draws something of its own, and has a
        // counterpart.
        (
            DATA,
            "2501;BOX DRAWINGS HEAVY HORIZONTAL;",
            "2501;BOX DRAWINGS LIGHT HORIZONTAL;",
            Kind::Duplicate,
            "U+2501 draws what U+2500 draws",
        ),
        (
            DATA,
            "250C;BOX DRAWINGS LIGHT DOWN AND RIGHT;",
            "250C;BOX DRAWINGS LIGHT DOWN AND RIGHTWARDS;",
            Kind::Invariant,
            "U+2510 has no counterpart",
        ),
        (
            DATA,
            "2501;BOX DRAWINGS HEAVY HORIZONTAL;",
            "2501;BOX DRAWINGS LIGHT HORIZONTAL;",
            Kind::Invariant,
            "U+2500's counterpart's counterpart is not itself",
        ),
        (
            DATA,
            "2501;BOX DRAWINGS HEAVY HORIZONTAL;",
            "2501;HEAVY HORIZONTAL;",
            Kind::Invariant,
            "a Box Drawing name without BOX DRAWINGS",
        ),
        (
            DATA,
            "2501;BOX DRAWINGS HEAVY HORIZONTAL;So;0;ON;;;;;N;FORMS HEAVY HORIZONTAL;;;;\n",
            "",
            Kind::Count,
            "127 Box Drawing names, 128 expected",
        ),
        // A bracket is bc=ON and Bidi_Mirrored.
        (
            DATA,
            "0028;LEFT PARENTHESIS;Ps;0;ON;;;;;Y;",
            "0028;LEFT PARENTHESIS;Ps;0;ON;;;;;N;",
            Kind::Invariant,
            "bracket U+0028 is not bc=ON and Bidi_M=Y",
        ),
        // U+2329 without its decomposition takes a pair of its own: 64
        // pairs, and BD16's equivalence lost.
        (
            DATA,
            "2329;LEFT-POINTING ANGLE BRACKET;Ps;0;ON;3008;",
            "2329;LEFT-POINTING ANGLE BRACKET;Ps;0;ON;;",
            Kind::Invariant,
            "64 bracket pairs, up to Some(64): 63 below 64 expected",
        ),
        (
            DATA,
            "2329;LEFT-POINTING ANGLE BRACKET;Ps;0;ON;3008;",
            "2329;LEFT-POINTING ANGLE BRACKET;Ps;0;ON;;",
            Kind::Control,
            "U+0028/U+0029 and U+2329/U+3009 are not paired as BD16 says",
        ),
    ];
    let mut missed = Vec::new();
    for case in cases {
        let (file, find, replace, kind, says) = *case;
        let got = failures(file, find, replace);
        if !got.iter().any(|(k, m)| *k == kind && m.contains(says)) {
            missed.push(format!(
                "{file}: {find:?} -> {replace:?}: no {kind:?} saying {says:?} in {got:#?}"
            ));
        }
    }
    assert!(missed.is_empty(), "{}", missed.join("\n"));
}

/// **A mirroring glyph past nine bits is refused**: 83 more distinct
/// glyphs make 512 with the 0 for none, which MIRROR's low nine bits
/// cannot index.
#[test]
fn a_512th_mirroring_glyph_does_not_fit() {
    let more: String = (0..83)
        .map(|i| format!("{:04X}; {:04X}\n", 0xE000 + i, 0xF000 + i))
        .collect();
    let got = failures(MIRRORING, "0028; 0029 #", &format!("{more}0028; 0029 #"));
    assert!(
        got.iter()
            .any(|(k, m)| *k == Kind::Invariant && m.contains("512 glyphs will not fit nine bits")),
        "{got:#?}"
    );
}

const EAW: &str = "unicode-eastasianwidth.txt";
const EAW8: &str = "unicode-eastasianwidth-8.0.0.txt";
const GC: &str = "unicode-derivedgeneralcategory.txt";
const PROPS: &str = "unicode-proplist.txt";
const EMOJI: &str = "unicode-emoji-data.txt";
const GBP: &str = "unicode-graphemebreakproperty.txt";
const DCP: &str = "unicode-derivedcoreproperties.txt";
const LB: &str = "unicode-linebreak.txt";

/// The width `emit`'s `<table>` gives `cp`, read as `Width::of` reads it.
fn width_of(emit: &Emit, table: &str, cp: u32) -> u8 {
    let (index, blocks) = (
        bin(emit, &format!("{table}_index")),
        bin(emit, &format!("{table}_blocks")),
    );
    width::packed_get(&index, &blocks, usize::try_from(cp).expect("a code point"))
}

const NARROW: u8 = 0;
const ZERO: u8 = 1;
const WIDE: u8 = 2;
const AMBIGUOUS: u8 = 3;

#[test]
fn the_pinned_files_build_the_unicode_tables() {
    let names = |e: Emit| -> Vec<String> { e.files().into_iter().map(|(f, _)| f).collect() };
    let g = grapheme::build(&pinned).unwrap_or_else(|e| panic!("{e}"));
    assert!(
        g.rust().contains("one of 111 distinct blocks"),
        "{}",
        g.rust()
    );
    assert_eq!(
        names(g),
        ["grapheme_data.rs", "break_index.bin", "break_blocks.bin"]
    );
    let w = width::build(&pinned).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(
        names(w),
        [
            "width_data.rs",
            "width_index.bin",
            "width_blocks.bin",
            "width8_index.bin",
            "width8_blocks.bin"
        ]
    );
    let l = linebreak::build(&pinned).unwrap_or_else(|e| panic!("{e}"));
    assert!(
        l.rust().contains("one of 144 distinct blocks"),
        "{}",
        l.rust()
    );
    assert_eq!(
        names(l),
        ["linebreak_data.rs", "line_index.bin", "line_blocks.bin"]
    );
}

/// **Zero over wide over ambiguous**, the rule `width` is, at every
/// combination of the three.
#[test]
fn width_takes_zero_over_wide_over_ambiguous() {
    for (zero, wide, ambiguous, want) in [
        (true, true, true, ZERO),
        (true, true, false, ZERO),
        (true, false, true, ZERO),
        (true, false, false, ZERO),
        (false, true, true, WIDE),
        (false, true, false, WIDE),
        (false, false, true, AMBIGUOUS),
        (false, false, false, NARROW),
    ] {
        assert_eq!(
            width::width(zero, wide, ambiguous),
            want,
            "{zero} {wide} {ambiguous}"
        );
    }
}

/// The rule in the tables: U+0300 is Mn and East_Asian_Width A, U+302A Mn
/// and W, and both are zero. No code point of 17.0.0 is both wide and
/// ambiguous -- East_Asian_Width gives one value, and no Emoji_Presentation
/// character is A -- so U+00A7 SECTION SIGN, A, is made Emoji_Presentation
/// here, and is wide under the current tables and still ambiguous under
/// Unicode 8's, which read no Emoji_Presentation.
#[test]
fn the_width_tables_take_zero_over_wide_over_ambiguous() {
    let w = width::build(&pinned).unwrap_or_else(|e| panic!("{e}"));
    for table in ["width", "width8"] {
        assert_eq!(width_of(&w, table, 0x0300), ZERO, "{table} U+0300");
        assert_eq!(width_of(&w, table, 0x302A), ZERO, "{table} U+302A");
        assert_eq!(width_of(&w, table, 0x00A7), AMBIGUOUS, "{table} U+00A7");
    }
    let row = "231A..231B    ; Emoji_Presentation";
    let w = perturbed(
        width::build,
        EMOJI,
        row,
        &format!("00A7 ; Emoji_Presentation\n{row}"),
    )
    .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(width_of(&w, "width", 0x00A7), WIDE);
    assert_eq!(width_of(&w, "width8", 0x00A7), AMBIGUOUS);
}

/// **The two exclusions from zero.** U+00AD SOFT HYPHEN is Cf and takes a
/// column in both tables, its East_Asian_Width's A; each
/// Prepended_Concatenation_Mark, Cf too, takes a column under the current
/// tables and none under Unicode 8's, whose zero set it does not narrow.
#[test]
fn soft_hyphen_and_the_concatenation_marks_are_not_zero() {
    let w = width::build(&pinned).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(width_of(&w, "width", 0x00AD), AMBIGUOUS);
    assert_eq!(width_of(&w, "width8", 0x00AD), AMBIGUOUS);
    let marks = [
        0x0600, 0x0601, 0x0602, 0x0603, 0x0604, 0x0605, 0x06DD, 0x070F, 0x0890, 0x0891, 0x08E2,
        0x110BD, 0x110CD,
    ];
    for cp in marks {
        assert_eq!(width_of(&w, "width", cp), NARROW, "U+{cp:04X}");
        assert_eq!(width_of(&w, "width8", cp), ZERO, "U+{cp:04X}");
    }
    // A Cf beside them is zero in both: U+200B ZERO WIDTH SPACE.
    assert_eq!(width_of(&w, "width", 0x200B), ZERO);
    // And without the PropList row the mark is zero again, which the
    // build's own control refuses.
    let got = failures_of(
        width::build,
        PROPS,
        "0600..0605    ; Prepended_Concatenation_Mark",
        "0600..0605    ; Other_Math",
    );
    assert!(
        got.iter().any(|(k, m)| *k == Kind::Control
            && m.contains("U+0600 (ARABIC NUMBER SIGN, a PCM) is (1, 1)")),
        "{got:#?}"
    );
}

#[test]
fn each_check_the_unicode_builds_make_fires() {
    let g: Build = grapheme::build;
    let w: Build = width::build;
    let l: Build = linebreak::build;
    each_fires(&[
        // The versions wired in.
        (
            w,
            EAW,
            "# EastAsianWidth-17.0.0.txt",
            "# EastAsianWidth-16.0.0.txt",
            Kind::Header,
            "data/unicode-eastasianwidth.txt:1:1",
        ),
        (
            g,
            GBP,
            "# GraphemeBreakProperty-17.0.0.txt",
            "# GraphemeBreakProperty-16.0.0.txt",
            Kind::Header,
            "data/unicode-graphemebreakproperty.txt:1:1",
        ),
        (
            l,
            LB,
            "# LineBreak-17.0.0.txt",
            "# LineBreak-16.0.0.txt",
            Kind::Header,
            "data/unicode-linebreak.txt:1:1",
        ),
        (
            w,
            EMOJI,
            "# Version: 17.0",
            "# Version: 18.0",
            Kind::Header,
            "no \"# Version: 17.0\" header line",
        ),
        // East_Asian_Width: its value set, its default, its rows, the
        // header's W defaults.
        (
            w,
            EAW,
            "00A7           ; A ",
            "00A7           ; Amb ",
            Kind::UnknownValue,
            "data/unicode-eastasianwidth.txt:78:18: ea `Amb` is not one of the 6 values",
        ),
        (w, EAW8, "00A7;A ", "00A7;X ", Kind::UnknownValue, "ea `X`"),
        (
            w,
            EAW,
            "# @missing: 0000..10FFFF; N",
            "#",
            Kind::Count,
            "0 @missing lines, 1 expected",
        ),
        (
            w,
            EAW,
            "00A7           ; A ",
            "00A7           ; A\n00A6..00A7 ; N ",
            Kind::Overlap,
            "data/unicode-eastasianwidth.txt:79:1: U+00A6 is already set by line 77",
        ),
        (
            w,
            EAW8,
            "U+3400..U+4DBF",
            "U+3400..U+4DB5",
            Kind::Control,
            "the header does not give U+3400..U+4DBF as defaulting to \"W\"",
        ),
        (
            w,
            EAW,
            "0041..005A     ; Na",
            "0041..005A     ; W",
            Kind::Control,
            "U+0041 (LATIN CAPITAL LETTER A) is (2, 0)",
        ),
        // General_Category: a group is no code point's, and every code
        // point is listed.
        (
            w,
            GC,
            "0300..036F    ; Mn",
            "0300..036F    ; M",
            Kind::UnknownValue,
            "gc `M` is a group of categories",
        ),
        (
            l,
            GC,
            "0300..036F    ; Mn",
            "0300..036F    ; Nonspacing",
            Kind::UnknownValue,
            "gc `Nonspacing` is not one of the 38 values",
        ),
        (
            g,
            GC,
            "D800..DFFF    ; Cs",
            "#",
            Kind::Invariant,
            "U+D800 has no value",
        ),
        // PropList: the concatenation marks' rows.
        (
            w,
            PROPS,
            "110CD         ; Prepended_Concatenation_Mark",
            "110CD         ; Other_Math",
            Kind::Count,
            "6 Prepended_Concatenation_Mark rows, 7 expected",
        ),
        // emoji-data.txt: its properties and its rows.
        (
            w,
            EMOJI,
            "231A..231B    ; Emoji_Presentation",
            "231A..231B    ; Emoji_Presentaton",
            Kind::UnknownValue,
            "emoji-data.txt property `Emoji_Presentaton`",
        ),
        (
            w,
            EMOJI,
            "231A..231B    ; Emoji_Presentation",
            "231A..231B    ; Emoji_Presentation\n231B ; Emoji_Presentation",
            Kind::Duplicate,
            "U+231B is already set by line 449",
        ),
        // Grapheme_Cluster_Break: long names, the default, a class for
        // each value.
        (
            g,
            GBP,
            "; Regional_Indicator #",
            "; RI #",
            Kind::UnknownValue,
            "GCB `RI` is not one of the 18 values",
        ),
        (
            g,
            GBP,
            "; Regional_Indicator #",
            "; E_Base #",
            Kind::UnknownValue,
            "GCB `E_Base` has no GraphemeClass",
        ),
        (
            g,
            GBP,
            "# @missing: 0000..10FFFF; Other",
            "#",
            Kind::Invariant,
            "has no value, and no default covers it",
        ),
        (
            g,
            GBP,
            "0903          ; SpacingMark",
            "0903          ; Extend",
            Kind::Control,
            "U+0903 is 0x04, SpacingMark with lookback 0 expected",
        ),
        (
            g,
            GBP,
            "09BE          ; Extend",
            "09BE          ; SpacingMark",
            Kind::Control,
            "U+09BE BENGALI VOWEL SIGN AA is not the first spacing Extend",
        ),
        (
            g,
            GBP,
            "0BBE          ; Extend",
            "0BBE          ; SpacingMark",
            Kind::Count,
            "45 spacing Extend ranges, 46 expected",
        ),
        // InCB: its four values, three fields, 505 rows.
        (
            g,
            DCP,
            "0915..0939    ; InCB; Consonant",
            "0915..0939    ; InCB; Consonnant",
            Kind::UnknownValue,
            "InCB `Consonnant` is not one of the 4 values",
        ),
        (
            g,
            DCP,
            "0915..0939    ; InCB; Consonant",
            "0915..0939    ; InCB",
            Kind::FieldCount,
            "2 fields, 3 expected",
        ),
        (
            g,
            DCP,
            "0915..0939    ; InCB; Consonant",
            "0915..0939    ; Alphabetic",
            Kind::Count,
            "504 InCB rows, 505 expected",
        ),
        (
            g,
            DCP,
            "094D          ; InCB; Linker",
            "094D          ; InCB; Extend",
            Kind::Control,
            "U+094D is 0x84, Extend with lookback 2 expected",
        ),
        // Line_Break: its value set, its default, LB1.
        (
            l,
            LB,
            "0E31           ; SA ",
            "0E31           ; Sa ",
            Kind::UnknownValue,
            "lb `Sa` is not one of the 49 values",
        ),
        (
            l,
            LB,
            "# @missing: 0000..10FFFF; XX",
            "#",
            Kind::Count,
            "0 @missing lines, 1 expected",
        ),
        (
            l,
            LB,
            "0041..005A     ; AL",
            "0041..005A     ; AL\n005A ; AL",
            Kind::Duplicate,
            "U+005A is already set by line 81",
        ),
        (
            l,
            LB,
            "0E31           ; SA ",
            "0E31           ; AL ",
            Kind::Control,
            "U+0E31 is 0x00, CM with flags 0x00 expected",
        ),
    ]);
    // An emoji-data.txt of another version: 18.0's, beside 17.0's in the
    // sources, is refused by its version line.
    let read = |f: &str| {
        pinned(if f == EMOJI {
            "ucd18-emoji-data.txt"
        } else {
            f
        })
    };
    for build in [g, w, l] {
        let errs = build(&read).expect_err("refused");
        assert!(errs.list().iter().any(|e| e.kind == Kind::Header), "{errs}");
    }
}

const CASEFOLDING: &str = "unicode-casefolding.txt";
const SHAPING: &str = "unicode-arabicshaping.txt";
const DJT: &str = "unicode-derivedjoiningtype.txt";
const SEQUENCES: &str = "emoji17-sequences.txt";
const ZWJ: &str = "emoji17-zwj-sequences.txt";
const VARIATION: &str = "unicode-emoji-variation-sequences.txt";

#[test]
fn the_pinned_files_build_the_five_tables() {
    let names: Vec<Vec<String>> = [
        caseless::build as Build,
        search::build,
        sequence::build,
        joining::build,
        ignorable::build,
    ]
    .iter()
    .map(|b| {
        b(&pinned)
            .unwrap_or_else(|e| panic!("{e}"))
            .files()
            .into_iter()
            .map(|(f, _)| f)
            .collect()
    })
    .collect();
    assert_eq!(
        names,
        [
            vec!["caseless_data.rs"],
            vec!["search_data.rs"],
            vec!["sequence_data.rs"],
            vec!["joining_data.rs", "joining_index.bin", "joining_blocks.bin"],
            vec!["ignorable_data.rs"],
        ]
    );
    let inputs: Vec<Vec<&str>> = [
        &caseless::INPUTS[..],
        &search::INPUTS,
        &sequence::INPUTS,
        &joining::INPUTS,
        &ignorable::INPUTS,
    ]
    .iter()
    .map(|i| i.iter().map(|i| i.file).collect())
    .collect();
    assert_eq!(
        inputs,
        [
            vec![CASEFOLDING],
            vec![DATA, CASEFOLDING],
            vec![SEQUENCES, ZWJ, VARIATION],
            vec![ALIASES, GC, SHAPING, DJT],
            vec![DCP],
        ]
    );
}

#[test]
fn each_check_the_five_builds_make_fires() {
    let (c, s, q, j, i): (Build, Build, Build, Build, Build) = (
        caseless::build,
        search::build,
        sequence::build,
        joining::build,
        ignorable::build,
    );
    each_fires(&[
        // CaseFolding.txt: the version, a status outside the four, a second
        // full folding, a mapping that is not code points, and each
        // control.
        (
            c,
            CASEFOLDING,
            "# CaseFolding-17.0.0.txt",
            "# CaseFolding-16.0.0.txt",
            Kind::Header,
            "is this the pinned file [unicode-casefolding]?",
        ),
        (
            c,
            CASEFOLDING,
            "0041; C; 0061;",
            "0041; X; 0061;",
            Kind::UnknownValue,
            "data/unicode-casefolding.txt:63:7: CaseFolding.txt status `X`",
        ),
        (
            s,
            CASEFOLDING,
            "0041; C; 0061;",
            "0041; C; 0061; #\n0041; F; 0061;",
            Kind::Duplicate,
            "U+0041 already has a full folding on line 63",
        ),
        (
            c,
            CASEFOLDING,
            "00DF; F; 0073 0073;",
            "00DF; F; 0073 73;",
            Kind::Malformed,
            "data/unicode-casefolding.txt:121:10: \"73\" is not a code point",
        ),
        (
            c,
            CASEFOLDING,
            "00DF; F; 0073 0073;",
            "00DF; F; 0073;",
            Kind::Control,
            "U+00DF folds to Some([73]), [73, 73] expected",
        ),
        (
            c,
            CASEFOLDING,
            "0130; F; 0069 0307;",
            "0130; S; 0069 0307;",
            Kind::Control,
            "U+0130 folds to None",
        ),
        (
            c,
            CASEFOLDING,
            "212A; C; 006B;",
            "212A; C; 004B;",
            Kind::Control,
            "U+212A folds to Some([4B])",
        ),
        (
            c,
            CASEFOLDING,
            "00C5; C; 00E5;",
            "00C5; F; 0061 030A;",
            Kind::Control,
            "U+00C5 folds to Some([61, 30A])",
        ),
        (
            c,
            CASEFOLDING,
            "0049; T; 0131;",
            "0049; F; 0131;",
            Kind::Duplicate,
            "U+0049 already has a full folding",
        ),
        (
            s,
            CASEFOLDING,
            "1E9E; F; 0073 0073;",
            "1E9E; F; 0073;",
            Kind::Control,
            "U+1E9E folds to [73], [73, 73] expected",
        ),
        (
            s,
            CASEFOLDING,
            "0041; C; 0061;",
            "0041; C; AC00;",
            Kind::Invariant,
            "U+0041 folds to a Hangul syllable",
        ),
        // UnicodeData.txt for the search keys: a decomposition moved, a
        // combining class that is not one, a Hangul syllable listed.
        (
            s,
            DATA,
            "212B;ANGSTROM SIGN;Lu;0;L;00C5;",
            "212B;ANGSTROM SIGN;Lu;0;L;0041;",
            Kind::Control,
            "U+212B decomposes to [41], [41, 30A] expected",
        ),
        (
            s,
            DATA,
            "0300;COMBINING GRAVE ACCENT;Mn;230;",
            "0300;COMBINING GRAVE ACCENT;Mn;X;",
            Kind::Malformed,
            "a combining class is a number 0 to 254",
        ),
        (
            s,
            DATA,
            "D7A3;<Hangul Syllable, Last>;Lo;0;L;;",
            "D7A3;<Hangul Syllable, Last>;Lo;0;L;1100 1161;",
            Kind::Invariant,
            "U+AC00..U+D7A3 is a Hangul syllable with a listed decomposition",
        ),
        // The sequence files: each version line, a type outside the file's
        // set, a field missing, a sequence listed twice, each count.
        (
            q,
            SEQUENCES,
            "# Version: 17.0",
            "# Version: 16.0",
            Kind::Header,
            "no \"# Version: 17.0\" header line",
        ),
        (
            q,
            ZWJ,
            "; RGI_Emoji_ZWJ_Sequence  ; family: man, woman, girl ",
            "; RGI_Emoji_ZWJ_Sequenc  ; family: man, woman, girl ",
            Kind::UnknownValue,
            "data/emoji17-zwj-sequences.txt type_field `RGI_Emoji_ZWJ_Sequenc`",
        ),
        (
            q,
            VARIATION,
            "0023 FE0E  ; text style;",
            "0023 FE0E  ; text style",
            Kind::FieldCount,
            "data/unicode-emoji-variation-sequences.txt:12:",
        ),
        (
            q,
            SEQUENCES,
            "1F1E6 1F1E8   ; RGI_Emoji_Flag_Sequence      ; flag: Ascension Island",
            "0023 FE0F 20E3; RGI_Emoji_Flag_Sequence      ; flag: Ascension Island",
            Kind::Duplicate,
            "listed already on line 550",
        ),
        (
            q,
            SEQUENCES,
            "1F1E6 1F1E8   ; RGI_Emoji_Flag_Sequence      ; flag: Ascension Island",
            "1F1E6         ; Basic_Emoji                  ; flag: Ascension Island",
            Kind::Count,
            "1145 sequences of two codepoints or more, 1146 expected",
        ),
        (
            q,
            VARIATION,
            "0023 FE0E  ; text style;",
            "0023 FE0D  ; text style; #\n0023 FE0E  ; text style;",
            Kind::Count,
            "3296 distinct sequences in the three files, 3295 expected",
        ),
        (
            q,
            SEQUENCES,
            "231A..231B    ; Basic_Emoji",
            "231B..231A    ; Basic_Emoji",
            Kind::Malformed,
            "U+231B..U+231A runs backwards",
        ),
        // ArabicShaping.txt and DerivedJoiningType.txt: the version, a type
        // or group outside its set, a row given twice, a row dropped, each
        // control, and the derived file disagreeing.
        (
            j,
            SHAPING,
            "0628; BEH; D; BEH",
            "0628; BEH; X; BEH",
            Kind::UnknownValue,
            "jt `X` is not one of the 6 values",
        ),
        (
            j,
            SHAPING,
            "0628; BEH; D; BEH",
            "0628; BEH; D; BAH",
            Kind::UnknownValue,
            "data/unicode-arabicshaping.txt:139:15: jg `BAH`",
        ),
        (
            j,
            SHAPING,
            "0628; BEH; D; BEH",
            "0627; BEH; D; BEH",
            Kind::Duplicate,
            "U+0627 is already set by line 138",
        ),
        (
            j,
            SHAPING,
            "0628; BEH; D; BEH",
            "#",
            Kind::Count,
            "833 rows, 834 expected",
        ),
        (
            j,
            SHAPING,
            "0627; ALEF; R; ALEF",
            "0627; ALEF; D; ALEF",
            Kind::Control,
            "U+0627 is jt D, R expected",
        ),
        (
            j,
            SHAPING,
            "0644; LAM; D; LAM",
            "0644; LAM; D; BEH",
            Kind::Control,
            "U+0644 is in group 0, 1 expected",
        ),
        (
            j,
            SHAPING,
            "0710; ALAPH; R; ALAPH",
            "0710; ALAPH; R; BETH",
            Kind::Control,
            "group 3 is [], [710] expected",
        ),
        (
            j,
            SHAPING,
            "0715; DALATH; R; DALATH RISH",
            "0715; DALATH; R; DAL",
            Kind::Control,
            "group 4 is [716, 72A, 72F]",
        ),
        (
            j,
            DJT,
            "0640          ; C",
            "0640          ; D",
            Kind::Control,
            "U+0640 is jt D in this file and C from ArabicShaping.txt",
        ),
        (
            j,
            DJT,
            "# @missing: 0000..10FFFF; Non_Joining",
            "# @missing: 0000..10FFFF; Transparent",
            Kind::Control,
            "the default is not 0000..10FFFF Non_Joining",
        ),
        (
            j,
            DJT,
            "# @missing: 0000..10FFFF; Non_Joining",
            "#",
            Kind::Count,
            "0 @missing lines, 1 expected",
        ),
        // DerivedCoreProperties.txt for the ignorables: a row dropped, two
        // rows over one code point, a field too many, each control.
        (
            i,
            DCP,
            "3164          ; Default_Ignorable_Code_Point",
            "#",
            Kind::Count,
            "16 Default_Ignorable_Code_Point ranges, 17 expected",
        ),
        (
            i,
            DCP,
            "061C          ; Default_Ignorable_Code_Point",
            "034F          ; Default_Ignorable_Code_Point",
            Kind::Overlap,
            "U+034F is already given Default_Ignorable_Code_Point",
        ),
        (
            i,
            DCP,
            "061C          ; Default_Ignorable_Code_Point",
            "061C          ; Default_Ignorable_Code_Point ; Y",
            Kind::FieldCount,
            "3 fields, 2 expected",
        ),
        (
            i,
            DCP,
            "200B..200F    ; Default_Ignorable_Code_Point",
            "200B..200C    ; Default_Ignorable_Code_Point",
            Kind::Control,
            "U+200D is default-ignorable: false, true expected",
        ),
        (
            i,
            DCP,
            "3164          ; Default_Ignorable_Code_Point",
            "3164          ; Default_Ignorable_Code_Point\n1F600 ; Default_Ignorable_Code_Point",
            Kind::Control,
            "U+1F600 is default-ignorable: true, false expected",
        ),
    ]);
}

/// The sequence table's hash is the one found from seed 0 up: from 1 up,
/// as `perfect_hash` tries for the colour names, every edge is still
/// placed, and the seeds are not the table's.
#[test]
fn the_sequence_seeds_are_tried_from_zero() {
    let emit = sequence::build(&pinned).unwrap_or_else(|e| panic!("{e}"));
    let rs = emit.rust();
    let seeds: Vec<u16> = rs
        .split_once("pub static SEEDS: [u16; 2048] = [")
        .and_then(|(_, r)| r.split_once("];"))
        .expect("SEEDS")
        .0
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.parse().expect("a seed"))
        .collect();
    assert!(seeds.contains(&0), "a bucket placed at seed 0");
    // The keys as the build makes them, read back from EDGES.
    let edges: Vec<u64> = rs
        .split_once("pub static EDGES: [u64; 8192] = [")
        .and_then(|(_, r)| r.split_once("];"))
        .expect("EDGES")
        .0
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| u64::from_str_radix(s.trim_start_matches("0x"), 16).expect("an edge"))
        .filter(|&e| e != 0)
        .collect();
    assert_eq!(edges.len(), 4688);
    let keys: Vec<u64> = edges.iter().map(|e| e >> 16).collect();
    let buckets: Vec<usize> = keys.iter().map(|&k| sequence::bucket(k)).collect();
    let from = |first: u16| {
        let (b, s) = (baud_tablegen::Buckets(2048), baud_tablegen::Slots(8192));
        baud_tablegen::place("edges", &buckets, b, s, first..=u16::MAX, |k, s| {
            sequence::slot(keys[k], s)
        })
        .expect("placed")
        .seeds
    };
    let zero = from(0);
    let one = from(1);
    for (b, (&z, &s)) in zero.iter().zip(&seeds).enumerate() {
        if buckets.contains(&b) {
            assert_eq!(z, s, "bucket {b}");
        }
    }
    assert_ne!(one, zero, "seeds from 1 are another table");
}
