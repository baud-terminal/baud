//! **Each check `build/bidi.rs` makes on the pinned files fires**: the
//! build module, run here on the five files with one row changed, reports
//! the failure that change must cause -- a dropped `@missing` line, a class
//! outside PropertyValueAliases.txt's `bc` set, an overlapping row, a known
//! code point with another class, a mirroring glyph on a character that is
//! not mirrored, a bracket that is not its pair's pair, a canonical
//! decomposition moved, a Box Drawing name with no counterpart -- and
//! reports nothing on the files as pinned. The module only computes: it
//! reads through the function it is given and writes nothing, so running it
//! here touches no `OUT_DIR`.

#[path = "../build/bidi.rs"]
mod bidi;

use baud_tablegen::Kind;

fn pinned(file: &str) -> std::io::Result<String> {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../docs/research/sources");
    std::fs::read_to_string(format!("{dir}/{file}"))
}

/// The failures of a build whose `file` has its first `find` replaced.
fn failures(file: &str, find: &str, replace: &str) -> Vec<(Kind, String)> {
    let read = |f: &str| {
        let text = pinned(f)?;
        if f != file {
            return Ok(text);
        }
        assert!(text.contains(find), "{file} has no {find:?}");
        Ok(text.replacen(find, replace, 1))
    };
    match bidi::build(&read) {
        Ok(_) => Vec::new(),
        Err(errs) => errs
            .list()
            .iter()
            .map(|e| (e.kind, e.to_string()))
            .collect(),
    }
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
