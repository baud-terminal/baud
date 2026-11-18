//! **Every reader that assumes one band is named here**: a non-test
//! source that reads the terminal's live ids without its row's table --
//! `styles().one()`, `frozen::live` or `frozen::live_table` -- is listed with
//! how many times it does. `BandsView::one` panics on a terminal of more
//! bands than one, so a reader missing from this list would panic on the
//! first banded run instead of failing here; a banded mode is selectable
//! only once each listed reader reads its row's band.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Each non-test source that reads one band, and how many times.
const READERS: [(&str, usize); 10] = [
    ("baud-app/src/app.rs", 1),
    ("baud-present-macos/src/metal.rs", 1),
    ("baud-render/src/bidi.rs", 7),
    ("baud-render/src/cell_frame.rs", 2),
    ("baud-render/src/frozen.rs", 6),
    ("baud-render/src/overlay.rs", 1),
    ("baud-render/src/rebreak.rs", 4),
    ("baud-render/src/renderer.rs", 12),
    ("baud-render/src/select.rs", 3),
    ("baud-sfte-difftest/src/lib.rs", 1),
];

/// The workspace's root, this crate's parent.
fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the workspace")
        .to_path_buf()
}

/// Every `.rs` file under `dir`.
fn sources(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            sources(&p, out);
        } else if p.extension().is_some_and(|x| x == "rs") {
            out.push(p);
        }
    }
}

/// `text` without its top-level test modules -- each `#[cfg(test)] mod`
/// to the `}` that rustfmt sets alone at the line's start to close it, or
/// to its `;` where it is a file of its own; a brace count would miscount
/// the braces a source-text test's strings hold -- and with the space
/// before each `.` removed, so a call rustfmt broke across lines reads as
/// one.
fn non_test(text: &str) -> String {
    let mut kept = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find("#[cfg(test)]\nmod ") {
        kept.push_str(&rest[..at]);
        let module = &rest[at..];
        let line = module.find('\n').map_or(module.len(), |n| {
            n + 1 + module[n + 1..].find('\n').unwrap_or(module.len() - n - 1)
        });
        let end = match module[..line].trim_end().ends_with(';') {
            true => line,
            false => module.find("\n}").map_or(module.len(), |n| n + 2),
        };
        rest = &module[end..];
    }
    kept.push_str(rest);
    let mut out = String::with_capacity(kept.len());
    let mut chars = kept.chars().peekable();
    while let Some(c) = chars.next() {
        if c.is_whitespace() {
            let mut ws = String::from(c);
            while let Some(&n) = chars.peek().filter(|n| n.is_whitespace()) {
                ws.push(n);
                chars.next();
            }
            if chars.peek() != Some(&'.') {
                out.push_str(&ws);
            }
            continue;
        }
        out.push(c);
    }
    out
}

/// The one-band reads in `text`: `live` and `live_table` called on a `term`,
/// not as the end of another name nor as a method.
fn reads(text: &str) -> usize {
    let t = non_test(text);
    let named = |p: &str| {
        t.match_indices(p)
            .filter(|(i, _)| {
                !t[..*i]
                    .chars()
                    .next_back()
                    .is_some_and(|c| c.is_alphanumeric() || c == '_' || c == '.')
            })
            .count()
    };
    t.matches("styles().one()").count() + named("live(term)") + named("live_table(term)")
}

/// Every non-test source in the workspace that reads one band.
fn found() -> BTreeMap<String, usize> {
    let root = root();
    let mut files = Vec::new();
    for e in std::fs::read_dir(&root).expect("the workspace").flatten() {
        sources(&e.path().join("src"), &mut files);
    }
    files
        .iter()
        .filter(|p| !p.to_string_lossy().ends_with("_tests.rs"))
        .filter_map(|p| {
            let n = reads(&std::fs::read_to_string(p).ok()?);
            let rel = p.strip_prefix(&root).ok()?.to_string_lossy().into_owned();
            (n > 0).then_some((rel, n))
        })
        .collect()
}

/// **The one-band readers are the listed ones, each as often as listed.**
/// Perturbation: a new `term.styles().one()` in any crate's source fails
/// here by name.
#[test]
fn every_one_band_reader_is_listed() {
    let listed: BTreeMap<String, usize> =
        READERS.iter().map(|(f, n)| (f.to_string(), *n)).collect();
    assert_eq!(found(), listed, "a reader of one band not listed, or gone");
}

/// The control: the scan reads a call rustfmt broke across lines, and
/// skips a test module.
#[test]
fn the_scan_reads_a_broken_call_and_skips_tests() {
    assert_eq!(reads("let w = term\n    .styles()\n    .one();\n"), 1);
    assert_eq!(
        reads("let a = snap_live(term); let b = t.live();"),
        0,
        "other names"
    );
    assert_eq!(reads("crate::frozen::live(term)"), 1);
    let src = "#[cfg(test)]\nmod tests {\n    const S: &str = \"fn f() {\";\n}\nlet _ = term.styles().one();\n";
    assert_eq!(reads(src), 1, "a string's brace ends no module");
    assert_eq!(
        reads("#[cfg(test)]\nmod t;\nlet _ = term.styles().one();"),
        1
    );
    assert_eq!(
        reads("#[cfg(test)]\nmod tests {\n    t.styles().one();\n}\n"),
        0
    );
    assert!(
        found().contains_key("baud-render/src/frozen.rs"),
        "the scan finds files"
    );
}
