//! **`gvar` and `cff2` off their pinned tables draw what the Python
//! drew**: each test changes one thing in the table a committed outline
//! file is written from, another location or another glyph step, and the
//! rows are the ones the Python scripts the two bins were ported from
//! (`scripts/gvar-oracle.py` and `scripts/cff2-oracle.py`, uharfbuzz
//! 0.53.2 on HarfBuzz 12.3.0) wrote with the same edit, pinned as their
//! count and the FNV-1a digest of the data lines joined by newlines. The
//! rows also differ from the committed file's, so the edit reached the
//! drawing and `--check` would fail on it.
//!
//! The locations reach what the committed rows do not: a weight between
//! masters not sampled there, and one past an end of the axis, which
//! HarfBuzz clamps to that end, so those rows are the end's. The steps
//! reach glyphs no committed row draws.

use baud_oracle::oracle::{Sample, cff2, gvar, weights};
use baud_oracle::repo;

/// FNV-1a over the bytes, kept here so the test does not take it from
/// the code it checks.
fn fnv64(data: &[u8]) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    for &b in data {
        h = (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3);
    }
    h
}

/// The committed file's data lines.
fn committed(file: &str) -> Vec<String> {
    std::fs::read_to_string(repo().join("baud-font/tests/var").join(file))
        .expect("the committed file")
        .lines()
        .filter(|l| !l.starts_with('#'))
        .map(str::to_string)
        .collect()
}

/// `rows` are the script's, `count` of them hashing to `digest`, and not
/// the committed file's.
fn assert_drawn(rows: &[String], count: usize, digest: &str, file: &str) {
    assert_eq!(rows.len(), count, "rows");
    assert_eq!(
        format!("{:016x}", fnv64(rows.join("\n").as_bytes())),
        digest,
        "the digest of the rows"
    );
    assert_ne!(rows, committed(file), "the committed rows");
}

/// The glyph ids and commands of `font`'s rows at `tag`.
fn at(rows: &[String], font: &str, tag: &str) -> Vec<String> {
    let head = format!("{font} {tag} ");
    rows.iter()
        .filter_map(|r| r.strip_prefix(head.as_str()))
        .map(str::to_string)
        .collect()
}

fn sample<'a>(samples: &'a mut [Sample], font: &str) -> &'a mut Sample {
    samples
        .iter_mut()
        .find(|s| s.font == font)
        .expect("a sampled font")
}

/// **The pinned tables are the committed files'**: the control that the
/// digests below are taken the same way as theirs.
#[test]
fn the_pinned_tables_hash_as_the_scripts_rows_did() {
    let rows = gvar::rows(&repo(), &gvar::samples());
    assert_eq!(rows, committed("outlines.txt"));
    assert_eq!(rows.len(), 1098);
    assert_eq!(
        format!("{:016x}", fnv64(rows.join("\n").as_bytes())),
        "75ea3687ca6110ae"
    );
    let rows = cff2::rows(&repo(), &cff2::samples());
    assert_eq!(rows, committed("cff2-outlines.txt"));
    assert_eq!(rows.len(), 1360);
    assert_eq!(
        format!("{:016x}", fnv64(rows.join("\n").as_bytes())),
        "a4441be357bc8cbd"
    );
}

/// JetBrains Mono at wght 550, between masters, and 1000, past the
/// axis's 800, in place of 350 and 800.
#[test]
fn gvar_at_other_locations_draws_what_the_script_drew() {
    let mut s = gvar::samples();
    sample(&mut s, "fonts/JetBrainsMono-Variable.ttf").settings = weights(&[100, 550, 700, 1000]);
    let rows = gvar::rows(&repo(), &s);
    assert_drawn(&rows, 1098, "c40591db997cb7bc", "outlines.txt");
    let font = "fonts/JetBrainsMono-Variable.ttf";
    let clamped = at(&rows, font, "wght=1000");
    assert!(!clamped.is_empty());
    assert_eq!(clamped, at(&committed("outlines.txt"), font, "wght=800"));
}

/// Fira Code's every 29th glyph in place of every 37th.
#[test]
fn gvar_on_another_glyph_list_draws_what_the_script_drew() {
    let mut s = gvar::samples();
    sample(&mut s, "fonts/FiraCode-VF.ttf").step = 29;
    let rows = gvar::rows(&repo(), &s);
    assert_drawn(&rows, 1158, "1d5a9ce409fe9cff", "outlines.txt");
}

/// Source Code VF at wght 150, below the axis's 200, and 555, between
/// masters, in place of 250 and 637.
#[test]
fn cff2_at_other_locations_draws_what_the_script_drew() {
    let mut s = cff2::samples();
    sample(&mut s, "fonts/SourceCodeVF-Upright.otf").settings = weights(&[150, 400, 555, 900]);
    let rows = cff2::rows(&repo(), &s);
    assert_drawn(&rows, 1360, "ebdace6b81bdb714", "cff2-outlines.txt");
    let font = "fonts/SourceCodeVF-Upright.otf";
    let clamped = at(&rows, font, "wght=150");
    assert!(!clamped.is_empty());
    assert_eq!(clamped, at(&rows, font, "-"));
}

/// Source Code VF's every fifth glyph in place of every seventh.
#[test]
fn cff2_on_another_glyph_list_draws_what_the_script_drew() {
    let mut s = cff2::samples();
    sample(&mut s, "fonts/SourceCodeVF-Upright.otf").step = 5;
    let rows = cff2::rows(&repo(), &s);
    assert_drawn(&rows, 1810, "6ce06aa1e74f0687", "cff2-outlines.txt");
}
