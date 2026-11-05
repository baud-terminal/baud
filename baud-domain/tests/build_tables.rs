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
//! Prepended_Concatenation_Mark. The modules only compute: they read
//! through the function they are given and write nothing, so running them
//! here touches no `OUT_DIR`.

#[path = "../build/bidi.rs"]
mod bidi;
#[path = "../build/grapheme.rs"]
mod grapheme;
#[path = "../build/linebreak.rs"]
mod linebreak;
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
