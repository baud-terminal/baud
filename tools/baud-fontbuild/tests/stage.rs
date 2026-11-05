//! The Hebrew stage built from its committed sources: each source builds,
//! reads back from the font it builds as the same text, and is reached by
//! the comparison in every table; each kind of record, perturbed, moves
//! the table it says; and the build refuses a source it cannot honour.

use std::path::{Path, PathBuf};

use baud_fontbuild::stage::{build, extract, unpadded_len};
use baud_fontbuild::{compare, reach};

const STYLES: [&str; 4] = ["Regular", "Bold", "Italic", "BoldItalic"];

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn base(style: &str) -> Vec<u8> {
    std::fs::read(root().join(format!(
        "baud-app/fonts/jetbrains-mono/JetBrainsMono-{style}.ttf"
    )))
    .unwrap()
}

fn source(style: &str) -> String {
    std::fs::read_to_string(root().join(format!(
        "baud-app/fonts/glyphs/languages/hebrew/stage/{style}.txt"
    )))
    .unwrap()
}

#[test]
fn every_source_builds_and_reads_back_as_itself() {
    for style in STYLES {
        let (b, src) = (base(style), source(style));
        let font = build(&b, &src).unwrap_or_else(|e| panic!("{style}: {e}"));
        let again = extract(&b, &font, style).unwrap_or_else(|e| panic!("{style}: {e}"));
        assert!(
            again == src,
            "{style}: the built stage reads back differently"
        );
    }
}

#[test]
fn the_built_stages_are_expanded_with_nothing_left_as_bytes() {
    for style in STYLES {
        let font = build(&base(style), &source(style)).unwrap();
        let r = reach(&font).unwrap();
        assert!(r.bytes_only.is_empty(), "{style}: {:?}", r.bytes_only);
        for t in ["glyf", "cmap", "GDEF", "GSUB", "GPOS", "name", "OS/2"] {
            assert!(
                r.tables.iter().any(|(n, lines)| n == t && *lines > 0),
                "{style}: {t} not expanded"
            );
        }
    }
}

/// Each glyph's record, by loca.
fn glyph_spans(font: &[u8]) -> Vec<Vec<u8>> {
    use read_fonts::{FontRef, TableProvider};
    let f = FontRef::new(font).unwrap();
    let loca = f.loca(None).unwrap();
    let glyf = f.glyf().unwrap();
    let n = f.maxp().unwrap().num_glyphs() as usize;
    let data = glyf.offset_data().as_bytes();
    (0..n)
        .map(|g| {
            let (a, b) = (
                loca.get_raw(g).unwrap() as usize,
                loca.get_raw(g + 1).unwrap() as usize,
            );
            data[a..b].to_vec()
        })
        .collect()
}

#[test]
fn every_glyph_is_stored_without_padding_as_the_stage_stores_it() {
    // write-fonts pads an odd-length glyph to an even length; the stage
    // the sources were read from stores each glyph unpadded under a long
    // loca, and the Devanagari merge digests each glyph's stored bytes,
    // so the build keeps them unpadded.
    for style in STYLES {
        let font = build(&base(style), &source(style)).unwrap();
        let spans = glyph_spans(&font);
        let odd = spans.iter().filter(|g| g.len() % 2 == 1).count();
        assert!(
            odd > 0,
            "{style}: no glyph of odd length, so none was checked"
        );
        for (g, d) in spans.iter().enumerate() {
            if !d.is_empty() {
                assert_eq!(
                    unpadded_len(d).unwrap(),
                    d.len(),
                    "{style}: glyph {g} is padded"
                );
            }
        }
    }
}

#[test]
fn the_build_is_deterministic() {
    let (b, src) = (base("Bold"), source("Bold"));
    assert!(build(&b, &src).unwrap() == build(&b, &src).unwrap());
}

/// Regular's source with the first line that starts with `line_start`
/// changed by `edit`, built, and compared with the unchanged build.
fn perturbed(line_start: &str, edit: impl Fn(&str) -> String) -> Vec<String> {
    let (b, src) = (base("Regular"), source("Regular"));
    let at = src
        .lines()
        .position(|l| l.starts_with(line_start))
        .unwrap_or_else(|| panic!("no line starts with {line_start:?}"));
    let changed: Vec<String> = src
        .lines()
        .enumerate()
        .map(|(i, l)| if i == at { edit(l) } else { l.to_string() })
        .collect();
    let changed = changed.join("\n") + "\n";
    assert!(changed != src, "the edit of {line_start:?} changed nothing");
    let (want, got) = (build(&b, &src).unwrap(), build(&b, &changed).unwrap());
    compare(&want, &got).unwrap().differences
}

fn names(diffs: &[String], table: &str, what: &str) {
    assert!(
        diffs
            .iter()
            .any(|d| d.starts_with(&format!("{table}: ")) && d.contains(what)),
        "no {table} difference naming {what:?}: {diffs:?}"
    );
}

#[test]
fn a_moved_point_is_named_at_its_glyph() {
    let d = perturbed("contour 437,0 ", |l| l.replacen("437,0", "438,0", 1));
    names(&d, "glyf", "heb_05D0");
}

#[test]
fn a_moved_anchor_is_named_in_gpos() {
    let d = perturbed("mark ", |l| {
        let (head, xy) = l.rsplit_once(' ').unwrap();
        let (x, y) = xy.split_once(',').unwrap();
        format!("{head} {},{y}", x.parse::<i32>().unwrap() + 1)
    });
    names(&d, "GPOS", "");
}

#[test]
fn a_flipped_lookup_flag_is_named_in_gpos() {
    let d = perturbed("lookup 5 markbase flag 0x0000", |l| {
        l.replace("flag 0x0000", "flag 0x0008")
    });
    names(&d, "GPOS", "");
}

#[test]
fn a_dropped_code_point_is_named_in_cmap() {
    let d = perturbed("glyph heb_05D0 ", |l| l.replace(" unicode 05D0", ""));
    names(&d, "cmap", "");
}

#[test]
fn a_changed_name_record_is_named() {
    let d = perturbed("name 1 3 1 ", |l| l.replace("Proto", "Proto2"));
    names(&d, "name", "");
}

#[test]
fn a_changed_os2_range_is_named() {
    let d = perturbed("os2.ulUnicodeRange1 ", |_| {
        "os2.ulUnicodeRange1 0x00000001".into()
    });
    names(&d, "OS/2", "");
}

#[test]
fn a_changed_glyph_class_is_named_in_gdef() {
    let d = perturbed("glyph heb_05D0 ", |l| l.replace(" class 1", " class 3"));
    names(&d, "GDEF", "");
}

#[test]
fn a_changed_advance_is_named_in_hmtx() {
    let d = perturbed("glyph heb_05D0 ", |l| {
        l.replace("advance 600", "advance 601")
    });
    names(&d, "hmtx", "");
}

#[test]
fn a_changed_feature_list_is_named() {
    let d = perturbed("feature GPOS 0 mark ", |l| {
        l.trim_end_matches(" 5").to_string()
    });
    names(&d, "GPOS", "");
}

#[test]
fn a_mark_set_short_of_a_glyph_is_named_in_gdef() {
    let d = perturbed("markset ", |l| l.rsplit_once(' ').unwrap().0.to_string());
    names(&d, "GDEF", "");
}

fn refused(edit: impl Fn(&str) -> String) -> String {
    let src = edit(&source("Regular"));
    build(&base("Regular"), &src).expect_err("the build took a source it cannot honour")
}

#[test]
fn a_component_naming_no_glyph_is_refused() {
    let e = refused(|s| {
        let at = s.find("\ncomponent ").unwrap() + "\ncomponent ".len();
        let end = at + s[at..].find(' ').unwrap();
        format!("{}nosuchglyph{}", &s[..at], &s[end..])
    });
    assert!(e.contains("no glyph named nosuchglyph"), "{e}");
}

#[test]
fn a_repeated_glyph_name_is_refused() {
    let e = refused(|s| s.replacen("glyph heb_05D1 ", "glyph heb_05D0 ", 1));
    assert!(e.contains("repeats"), "{e}");
}

#[test]
fn a_lookup_out_of_its_place_is_refused() {
    let e = refused(|s| s.replacen("lookup 5 markbase", "lookup 6 markbase", 1));
    assert!(e.contains("would be GPOS lookup 5"), "{e}");
}

#[test]
fn a_code_point_mapped_twice_is_refused() {
    let e = refused(|s| s.replacen("unicode 05D1", "unicode 05D0", 1));
    assert!(e.contains("mapped already"), "{e}");
}

#[test]
fn an_unknown_record_is_refused_by_its_line() {
    let e = refused(|s| s.replacen("glyph heb_05D0 ", "glif heb_05D0 ", 1));
    assert!(e.starts_with("line "), "{e}");
}

#[test]
fn a_source_against_another_style_is_refused_or_differs() {
    // Regular's source merged into Italic's stock face is not Italic's
    // stage: the extraction's own check is what stops such a source.
    let wrong = build(&base("Italic"), &source("Regular"));
    if let Ok(font) = wrong {
        let want = build(&base("Italic"), &source("Italic")).unwrap();
        assert!(!compare(&want, &font).unwrap().is_equal());
    }
}
