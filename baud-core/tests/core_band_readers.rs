//! **Every core read of the rendition table that names no row's band is
//! listed here**: `tmpl.as_slice()`, the band the write path is in, and
//! `LiveTable::of`, a table of no band. Banded, a row's ids name slots of
//! its own band's table, so a read of another row's cell through either
//! reads another band's renditions; that reader goes through
//! `Paint::row_table`, `Paint::band_table` or `Paint::word_in` instead.
//! Each listed file reads one of these for its own band's ids alone:
//!
//! * `bands.rs`: `BandsView::one`, the one-band view, which refuses a
//!   terminal of more bands.
//! * `freeze.rs`: freezing off the screen, the single mode's alone; its
//!   load test, the table's length; and the pen's id, the write band's.
//! * `ops.rs`: interning and the sweep, the write band's own slots.
//! * `storage.rs`: every band's tables, sliced by band, and the single
//!   mode's one.
//! * `strings.rs`: the blank's id, interned in the band its row's writer
//!   entered.
//! * `term.rs`: every band's tables, for `Term::styles`; and a change of
//!   width's refreeze, whose rows are each frozen through their band's
//!   table first in the banded mode, so its live table is the single
//!   mode's.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Each core source that reads the table with no row's band, and how
/// many times.
const READERS: [(&str, usize); 6] = [
    ("bands.rs", 1),
    ("freeze.rs", 3),
    ("ops.rs", 16),
    ("storage.rs", 2),
    ("strings.rs", 1),
    ("term.rs", 2),
];

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
/// to its `;` where it is a file of its own -- and with the space before
/// each `.` removed, so a call rustfmt broke across lines reads as one.
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

/// The reads in `text` that name no row's band.
fn reads(text: &str) -> usize {
    let t = non_test(text);
    t.matches("tmpl.as_slice()").count() + t.matches("LiveTable::of(").count()
}

/// Every non-test source of this crate that reads the table with no
/// row's band.
fn found() -> BTreeMap<String, usize> {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    sources(&src, &mut files);
    files
        .iter()
        .filter_map(|p| {
            let n = reads(&std::fs::read_to_string(p).ok()?);
            let rel = p.strip_prefix(&src).ok()?.to_string_lossy().into_owned();
            (n > 0).then_some((rel, n))
        })
        .collect()
}

/// **The core's band-less table reads are the listed ones, each as often
/// as listed.** Perturbation: a new `self.tmpl.as_slice()` reading a
/// cell's id in any core source fails here by name.
#[test]
fn every_band_less_table_read_in_the_core_is_listed() {
    let listed: BTreeMap<String, usize> =
        READERS.iter().map(|(f, n)| (f.to_string(), *n)).collect();
    assert_eq!(
        found(),
        listed,
        "a band-less table read not listed, or gone"
    );
}

/// The control: the scan reads a call rustfmt broke across lines, tells
/// a band's table from a band-less one, and skips a test module.
#[test]
fn the_scan_reads_a_broken_call_and_skips_tests() {
    assert_eq!(
        reads("let w = self\n    .tmpl\n    .as_slice()\n    .get(k);\n"),
        1
    );
    assert_eq!(reads("LiveTable::of(all)"), 1);
    assert_eq!(
        reads("LiveTable::of_band(s, b); self.tmpl.as_mut(); self.row_table(r)"),
        0,
        "a band's table, and a write"
    );
    assert_eq!(
        reads("#[cfg(test)]\nmod tests {\n    t.tmpl.as_slice();\n}\n"),
        0
    );
    assert!(found().contains_key("ops.rs"), "the scan finds files");
}
