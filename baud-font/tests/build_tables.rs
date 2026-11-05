//! **Each check the build modules make on the pinned files fires**: each
//! module, run here on its files with one row changed, reports the failure
//! that change must cause, and reports nothing on the files as pinned.
//! `build/presentation.rs`: a file of another version, a decomposition
//! that is not code points, a form given twice, a lam-alef ligature in a
//! form it has none of, a ligature dropped, a row with a field missing,
//! and each positive control. `build/ignorable.rs`: a row dropped, two
//! rows over one code point, and each control. The modules only compute:
//! they read through the function they are given and write nothing, so
//! running them here touches no `OUT_DIR`.

#[path = "../build/ignorable.rs"]
mod ignorable;
#[path = "../build/presentation.rs"]
mod presentation;

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

#[test]
fn the_pinned_files_build_the_two_tables() {
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
