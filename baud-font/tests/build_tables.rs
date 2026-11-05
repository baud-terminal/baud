//! **Each check the build modules make on the pinned files fires**: each
//! module, run here on its files with one row changed, reports the failure
//! that change must cause, and reports nothing on the files as pinned.
//! `build/compose.rs`, `build/indic.rs`, `build/script.rs` and
//! `build/standard_encoding.rs`: every check, each in
//! `each_check_the_four_builds_make_fires`.
//! `build/presentation.rs`: a file of another version, a decomposition
//! that is not code points, a form given twice, a lam-alef ligature in a
//! form it has none of, a ligature dropped, a row with a field missing,
//! and each positive control. `build/ignorable.rs`: a row dropped, two
//! rows over one code point, and each control. The modules only compute:
//! they read through the function they are given and write nothing, so
//! running them here touches no `OUT_DIR`.

#[path = "../build/compose.rs"]
mod compose;
#[path = "../build/ignorable.rs"]
mod ignorable;
#[path = "../build/indic.rs"]
mod indic;
#[path = "../build/presentation.rs"]
mod presentation;
#[path = "../build/script.rs"]
mod script;
#[path = "../build/standard_encoding.rs"]
mod standard_encoding;

use baud_tablegen::{Emit, Errors, Kind};

fn pinned(file: &str) -> std::io::Result<String> {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../docs/research/sources");
    std::fs::read_to_string(format!("{dir}/{file}"))
}

type Build = fn(&dyn Fn(&str) -> std::io::Result<String>) -> Result<Emit, Errors>;

/// The failures of `build` when `file` has its first `find` replaced.
fn failures_of(build: Build, file: &str, find: &str, replace: &str) -> Vec<(Kind, String)> {
    let read = |f: &str| {
        let text = pinned(f)?;
        if f != file {
            return Ok(text);
        }
        assert!(text.contains(find), "{file} has no {find:?}");
        Ok(text.replacen(find, replace, 1))
    };
    match build(&read) {
        Ok(_) => Vec::new(),
        Err(errs) => errs
            .list()
            .iter()
            .map(|e| (e.kind, e.to_string()))
            .collect(),
    }
}

const DATA: &str = "unicode-unicodedata.txt";
const SHAPING: &str = "unicode-arabicshaping.txt";
const DCP: &str = "unicode-derivedcoreproperties.txt";
const DNP: &str = "unicode-derivednormalizationprops.txt";
const ISC: &str = "unicode-indicsyllabiccategory.txt";
const IPC: &str = "unicode-indicpositionalcategory.txt";
const BLOCKS: &str = "unicode-blocks.txt";
const PVA: &str = "unicode-propertyvaluealiases.txt";
const SCRIPTS: &str = "unicode-scripts.txt";
const SCX: &str = "unicode-scriptextensions.txt";
const GC: &str = "unicode-derivedgeneralcategory.txt";
const BIDI: &str = "unicode-derivedbidiclass.txt";
const BRACKETS: &str = "unicode-bidibrackets.txt";
const CFF: &str = "adobe-5176-cff-standard-encoding.txt";

#[test]
fn the_pinned_files_build_the_six_tables() {
    for (build, inputs, files) in [
        (
            compose::build as Build,
            &compose::INPUTS[..],
            &["compose.rs"][..],
        ),
        (
            indic::build,
            &indic::INPUTS,
            &["indic_data.rs", "dense.bin", "latin1.bin", "sparse.bin"],
        ),
        (
            script::build,
            &script::INPUTS,
            &[
                "script_data.rs",
                "codes.bin",
                "tags.bin",
                "extension_sets.bin",
                "script_index.bin",
                "script_blocks.bin",
                "extension_index.bin",
                "extension_blocks.bin",
                "directions.bin",
                "run_index.bin",
                "run_blocks.bin",
                "bracket_index.bin",
                "bracket_blocks.bin",
            ],
        ),
        (
            standard_encoding::build,
            &standard_encoding::INPUTS,
            &["standard_encoding.rs"],
        ),
    ] {
        let emit = build(&pinned).unwrap_or_else(|e| panic!("{e}"));
        let names: Vec<String> = emit.files().into_iter().map(|(f, _)| f).collect();
        assert_eq!(names, files);
        assert!(inputs.iter().all(|i| pinned(i.file).is_ok()));
    }
    let rs = compose::build(&pinned).unwrap().rust().to_string();
    assert!(rs.contains("pub const COMPOSE: &[(u32, u32, u32)] = &["));
    assert!(rs.contains("    (0x0065, 0x0301, 0x00E9),"));
    let rs = standard_encoding::build(&pinned)
        .unwrap()
        .rust()
        .to_string();
    assert!(rs.contains("pub const STANDARD_ENCODING: &[u8; 256] = b\"\\x00"));
    let rs = script::build(&pinned).unwrap().rust().to_string();
    assert!(rs.contains("pub const SCRIPT_COUNT: usize = 175;"));
    assert!(rs.contains("pub const EXTENSION_SLOT: usize = 24;"));
    let rs = indic::build(&pinned).unwrap().rust().to_string();
    assert!(rs.contains("pub static SPARSE_BLOCKS: [u32; 5] = ["));

    for (build, inputs, files) in [
        (
            ignorable::build as Build,
            &ignorable::INPUTS[..],
            &["ignorable.rs"][..],
        ),
        (
            presentation::build,
            &presentation::INPUTS,
            &["presentation.rs"],
        ),
    ] {
        let emit = build(&pinned).unwrap_or_else(|e| panic!("{e}"));
        let names: Vec<String> = emit.files().into_iter().map(|(f, _)| f).collect();
        assert_eq!(names, files);
        assert!(inputs.iter().all(|i| pinned(i.file).is_ok()));
    }
    let rs = presentation::build(&pinned).unwrap().rust().to_string();
    assert!(rs.contains("pub static FORMS: [(u32, u32); 242]"));
    assert!(rs.contains("(0x0627, 0xFEFB, 0xFEFC),"));
    assert_eq!(presentation::FORMS.len(), 4);
}

#[test]
fn each_check_the_two_builds_make_fires() {
    let (p, i): (Build, Build) = (presentation::build, ignorable::build);
    let cases: &[(Build, &str, &str, &str, Kind, &str)] = &[
        (
            p,
            SHAPING,
            "# ArabicShaping-17.0.0.txt",
            "# ArabicShaping-16.0.0.txt",
            Kind::Header,
            "is this the pinned file [unicode-arabicshaping]?",
        ),
        (
            p,
            SHAPING,
            "0627; ALEF; R; ALEF",
            "0627; ALEF; R",
            Kind::FieldCount,
            "3 fields, 4 expected",
        ),
        (
            p,
            DATA,
            "<isolated> 0628;",
            "<isolated> 628;",
            Kind::Malformed,
            "data/unicode-unicodedata.txt:16636:",
        ),
        (
            p,
            DATA,
            "FB57;ARABIC LETTER PEH FINAL FORM;Lo;0;AL;<final> 067E;",
            "FB57;ARABIC LETTER PEH FINAL FORM;Lo;0;AL;<isolated> 067E;",
            Kind::Duplicate,
            "U+067E in that form is already U+FB56 on line 15856",
        ),
        (
            p,
            DATA,
            "FB57;ARABIC LETTER PEH FINAL FORM;Lo;0;AL;<final> 067E;",
            "FB57;ARABIC LETTER PEH FINAL FORM;Lo;0;AL;<final> 0628;",
            Kind::Control,
            "U+067E's forms are [Some(FB56), None, Some(FB58), Some(FB59)]",
        ),
        (
            p,
            DATA,
            "FEFB;ARABIC LIGATURE LAM WITH ALEF ISOLATED FORM;Lo;0;AL;<isolated> 0644 0627;",
            "FEFB;ARABIC LIGATURE LAM WITH ALEF ISOLATED FORM;Lo;0;AL;<medial> 0644 0627;",
            Kind::Invariant,
            "a lam-alef ligature is <isolated> or <final>",
        ),
        (
            p,
            DATA,
            "FEFB;ARABIC LIGATURE LAM WITH ALEF ISOLATED FORM;Lo;0;AL;<isolated> 0644 0627;",
            "FEFB;ARABIC LIGATURE LAM WITH ALEF ISOLATED FORM;Lo;0;AL;<isolated> 0644 0628;",
            Kind::Control,
            "lam with alef is Some([0, FEFC]), [FEFB, FEFC] expected",
        ),
        (
            p,
            SHAPING,
            "0622; ALEF WITH MADDA ABOVE; R; ALEF",
            "0622; ALEF WITH MADDA ABOVE; R; BEH",
            Kind::Count,
            "3 lam-alef ligatures' alefs, 4 expected",
        ),
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
            "FE00..FE0F    ; Default_Ignorable_Code_Point",
            "FE00..FE0E    ; Default_Ignorable_Code_Point",
            Kind::Control,
            "U+FE0F is default-ignorable: false, true expected",
        ),
    ];
    let mut missed = Vec::new();
    for &(build, file, find, replace, kind, says) in cases {
        let got = failures_of(build, file, find, replace);
        if !got.iter().any(|(k, m)| *k == kind && m.contains(says)) {
            missed.push(format!(
                "{file}: {find:?} -> {replace:?}: no {kind:?} saying {says:?} in {got:#?}"
            ));
        }
    }
    assert!(missed.is_empty(), "{}", missed.join("\n"));
}

#[test]
fn each_check_the_four_builds_make_fires() {
    let (c, n, s, e): (Build, Build, Build, Build) = (
        compose::build,
        indic::build,
        script::build,
        standard_encoding::build,
    );
    let cases: &[(Build, &str, &str, &str, Kind, &str)] = &[
        // build/standard_encoding.rs
        (
            e,
            CFF,
            "sha256 60e0696c",
            "sha256 00e0696c",
            Kind::Header,
            "is this the pinned file [adobe-5176-cff-standard-encoding]?",
        ),
        (
            e,
            CFF,
            "\n32; 1\n",
            "\n32; one\n",
            Kind::Malformed,
            "\"one\" is not a decimal number",
        ),
        (
            e,
            CFF,
            "\n32; 1\n",
            "\n32; 1; space\n",
            Kind::FieldCount,
            "3 fields, 2 expected",
        ),
        (
            e,
            CFF,
            "\n33; 2\n",
            "\n34; 2\n",
            Kind::Invariant,
            "code 34 where code 33 is next",
        ),
        (
            e,
            CFF,
            "\n201; 0\n",
            "\n201; 300\n",
            Kind::Invariant,
            "SID 300 does not fit the byte",
        ),
        (
            e,
            CFF,
            "\n255; 0\n",
            "\n",
            Kind::Count,
            "255 codes, 256 expected",
        ),
        (
            e,
            CFF,
            "\n65; 34\n",
            "\n65; 35\n",
            Kind::Control,
            "code 65 has SID Some(35), 34 expected",
        ),
        // build/compose.rs
        (
            c,
            DNP,
            "# DerivedNormalizationProps-17.0.0.txt",
            "# DerivedNormalizationProps-16.0.0.txt",
            Kind::Header,
            "is this the pinned file [unicode-dnp]?",
        ),
        (
            c,
            DATA,
            "0301;COMBINING ACUTE ACCENT;Mn;230;",
            "0301;COMBINING ACUTE ACCENT;Mn;23o;",
            Kind::Malformed,
            "data/unicode-unicodedata.txt:770:32: \"23o\" is not a combining class",
        ),
        (
            c,
            DATA,
            "Ll;0;L;0065 0301;",
            "Ll;0;L;0065 0301 0301;",
            Kind::Invariant,
            "a canonical decomposition of 3 code points, 1 or 2 expected",
        ),
        (
            c,
            DATA,
            "9FFF;<CJK Ideograph, Last>;Lo;0;",
            "9FFF;<CJK Ideograph, Last>;Lo;5;",
            Kind::Invariant,
            "a First/Last range has a combining class or a decomposition",
        ),
        (
            c,
            DATA,
            "Lu;0;L;0041 030A;",
            "Lu;0;L;0041 0301;",
            Kind::Duplicate,
            "U+0041 U+0301 composes to U+00C1 and to U+00C5",
        ),
        (
            c,
            DNP,
            "0340..0341    ; Full_Composition_Exclusion",
            "#",
            Kind::Count,
            "1118 Full_Composition_Exclusion code points, 1120 expected",
        ),
        (
            c,
            DNP,
            "0958..095F    ; Full_Composition_Exclusion",
            "#",
            Kind::Control,
            "U+0915 + U+093C composes to nothing",
        ),
        (
            c,
            DATA,
            "0F71;TIBETAN VOWEL SIGN AA;Mn;129;",
            "0F71;TIBETAN VOWEL SIGN AA;Mn;130;",
            Kind::Control,
            "U+0F71 has class 129: it does not",
        ),
        // build/indic.rs
        (
            n,
            ISC,
            "# IndicSyllabicCategory-17.0.0.txt",
            "# IndicSyllabicCategory-16.0.0.txt",
            Kind::Header,
            "is this the pinned file [unicode-indicsyllabiccategory]?",
        ),
        (
            n,
            ISC,
            "0915..0939    ; Consonant",
            "0915..0939    ; Reordering_Killer",
            Kind::UnknownValue,
            "data/unicode-indicsyllabiccategory.txt:819:17: \
             Indic_Syllabic_Category `Reordering_Killer` has no class",
        ),
        (
            n,
            IPC,
            "093F          ; Left",
            "093F          ; Bottom_And_Left",
            Kind::UnknownValue,
            "Indic_Positional_Category `Bottom_And_Left` has no class",
        ),
        (
            n,
            IPC,
            "093F          ; Left",
            "093F          ; Overstruck",
            Kind::Invariant,
            "U+093F: a matra at AFTER_MAIN, where no script places one",
        ),
        (
            n,
            IPC,
            "093F          ; Left",
            "093E          ; Left",
            Kind::Duplicate,
            "U+093E is already given on line 140",
        ),
        (
            n,
            BLOCKS,
            "0900..097F; Devanagari\n",
            "0900..097F; Devanagarii\n",
            Kind::Control,
            "no block is named \"Devanagari\"",
        ),
        (
            n,
            BLOCKS,
            "0980..09FF; Bengali",
            "0970..09FF; Bengali",
            Kind::Overlap,
            "U+0970..U+09FF overlaps Devanagari",
        ),
        (
            n,
            ISC,
            "09BC          ; Nukta",
            "#",
            Kind::Count,
            "797 classed characters in U+0900..U+0D7F, 798 expected",
        ),
        (
            n,
            IPC,
            "093F          ; Left",
            "093F          ; Right",
            Kind::Control,
            "U+093F is (\"M\", \"AFTER_SUB\"), (\"M\", \"PRE_M\") expected",
        ),
        // build/script.rs
        (
            s,
            SCRIPTS,
            "# Scripts-17.0.0.txt",
            "# Scripts-16.0.0.txt",
            Kind::Header,
            "is this the pinned file [unicode-scripts]?",
        ),
        (
            s,
            SCRIPTS,
            "05D0..05EA    ; Hebrew",
            "05D0..05EA    ; Hebrewish",
            Kind::UnknownValue,
            "`Hebrewish` is no sc long name",
        ),
        (
            s,
            SCX,
            "064B..0655    ; Arab Syrc",
            "064B..0655    ; Arab Syrx",
            Kind::UnknownValue,
            "`Syrx` is no script Scripts.txt gives a character",
        ),
        (
            s,
            SCX,
            "0342          ; Grek",
            "03B1          ; Grek",
            Kind::Invariant,
            "data/unicode-scriptextensions.txt:60:1: U+03B1: Script_Extensions is {Script}",
        ),
        (
            s,
            SCRIPTS,
            "0660..0669    ; Arabic",
            "05D0..0669    ; Arabic",
            Kind::Overlap,
            "data/unicode-scripts.txt:853:1: U+0600 is already given on line 841",
        ),
        (
            s,
            PVA,
            "sc ; Hebr                             ; Hebrew",
            "sc ; Hebx                             ; Hebrew",
            Kind::Control,
            "sc `Hebrew` is Some(\"Hebx\"), \"Hebr\" expected",
        ),
        (
            s,
            PVA,
            "sc ; Deva                             ; Devanagari",
            "sc ; Devx                             ; Devanagari",
            Kind::Control,
            "Deva's tags are None",
        ),
        (
            s,
            BIDI,
            "05D0..05EA    ; R",
            "05D0..05EA    ; L",
            Kind::Invariant,
            "Hebr: letters of both directions",
        ),
        (
            s,
            BIDI,
            "05D0..05EA    ; R",
            "#",
            Kind::Invariant,
            "U+05D0 is a letter the file gives no Bidi_Class",
        ),
        (
            s,
            GC,
            "0660..0669    ; Nd",
            "0660..0669    ; No",
            Kind::Control,
            "U+0661 is 0 to a right-to-left run, 2 expected",
        ),
        (
            s,
            BRACKETS,
            "# BidiBrackets-17.0.0.txt",
            "# BidiBrackets-16.0.0.txt",
            Kind::Header,
            "is this the pinned file [unicode-bidibrackets]?",
        ),
        (
            s,
            BRACKETS,
            "0028; 0029; o",
            "0028; 0029; x",
            Kind::UnknownValue,
            "Bidi_Paired_Bracket_Type `x`, one of o, c and n expected",
        ),
        (
            s,
            BRACKETS,
            "0028; 0029; o",
            "0028; 0029",
            Kind::FieldCount,
            "2 fields, 3 expected",
        ),
        (
            s,
            BRACKETS,
            "0029; 0028; c",
            "0028; 0028; c",
            Kind::Duplicate,
            "U+0028 is listed twice",
        ),
        (
            s,
            BRACKETS,
            "0028; 0029; o",
            "#",
            Kind::Count,
            "127 paired brackets, 128 expected",
        ),
        (
            s,
            BRACKETS,
            "0029; 0028; c",
            "0029; 005D; c",
            Kind::Invariant,
            "U+0029 and its pair are not each other's, one opening",
        ),
        (
            s,
            GC,
            "0028          ; Ps",
            "0028          ; Pe",
            Kind::Invariant,
            "U+0028 is gc Some(\"Pe\"), Ps expected",
        ),
        (
            s,
            BIDI,
            "0028          ; ON",
            "0028          ; L",
            Kind::Invariant,
            "U+0028 is bc Some(\"L\"), ON expected",
        ),
        (
            s,
            SCRIPTS,
            "169B          ; Ogham",
            "169B          ; Latin",
            Kind::Invariant,
            "U+169B and its pair are not in one script",
        ),
        (
            s,
            DATA,
            "2329;LEFT-POINTING ANGLE BRACKET;Ps;0;ON;3008;",
            "2329;LEFT-POINTING ANGLE BRACKET;Ps;0;ON;;",
            Kind::Control,
            "the brackets' canonical equivalents are {232A: 3009}",
        ),
    ];
    let mut missed = Vec::new();
    for &(build, file, find, replace, kind, says) in cases {
        let got = failures_of(build, file, find, replace);
        if !got.iter().any(|(k, m)| *k == kind && m.contains(says)) {
            missed.push(format!(
                "{file}: {find:?} -> {replace:?}: no {kind:?} saying {says:?} in {got:#?}"
            ));
        }
    }
    assert!(missed.is_empty(), "{}", missed.join("\n"));
}
