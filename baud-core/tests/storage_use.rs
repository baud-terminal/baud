#![deny(dead_code)]
//! Every field of `Storage` must be *read* somewhere, not merely written.
//!
//! `Storage` is one 24 MB block and its fields are `pub`, so neither `rustc`
//! nor `clippy` will say a word about one that is written and never read: the
//! dead-code lint does not reach public fields, and a write is a use.
//!
//! `tmpl` was one full-width row per style slot -- **1 MB** -- filled with 2048
//! cells
//! on every style-table miss and read by nothing at all. On btop that was
//! 109,274 misses, about 223 million cell writes and 1.8 GB of traffic, for a
//! buffer no code path consulted. Removing the fill measured **+27%**.
//!
//! Nothing caught it because nothing was looking: the counters did not count
//! those writes either, so btop's cell amplification read 0.2x per byte, which
//! should have looked impossible for a terminal drawing a full screen.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// Every `.rs` file under `dir`, concatenated.
fn sources(dir: &Path, out: &mut String) {
    let Ok(rd) = fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            sources(&p, out);
        } else if p.extension().is_some_and(|x| x == "rs") {
            if let Ok(s) = fs::read_to_string(&p) {
                out.push_str(&s);
                out.push('\n');
            }
        }
    }
}

/// The declared field names of `pub struct Storage`.
fn storage_fields(src: &str) -> Vec<String> {
    let at = src
        .find("pub struct Storage {")
        .expect("Storage is declared");
    let body = &src[at..];
    let end = body.find("\n}").expect("Storage's body ends");
    body[..end]
        .lines()
        .filter_map(|l| {
            let t = l.trim();
            let t = t.strip_prefix("pub ")?;
            let (name, _) = t.split_once(':')?;
            name.chars()
                .all(|c| c.is_ascii_lowercase() || c == '_')
                .then(|| name.to_string())
        })
        .collect()
}

/// **A field is only used if something reads it.**
///
/// A write-only field is storage the program pays for and never consults, and
/// it is invisible to every other check here: it compiles, it is `pub` so the
/// dead-code lint ignores it, and tests pass because nothing depended on it.
#[test]
fn every_storage_field_is_read_somewhere() {
    let mut src = String::new();
    sources(&root().join("src"), &mut src);
    let decl = fs::read_to_string(root().join("src/storage.rs")).expect("storage.rs");
    let fields = storage_fields(&decl);
    assert!(
        fields.len() > 5,
        "found {} fields, parser is wrong",
        fields.len()
    );

    let mut write_only = BTreeSet::new();
    for f in &fields {
        // A read is any mention that is not `&mut`-borrowing it or assigning
        // to it. Crude on purpose: the question is whether the name appears in
        // a reading position anywhere at all, and a field with *no* such
        // mention is unambiguous.
        let mut read = false;
        for pat in [
            format!(".{f}["),
            format!(".{f}.len"),
            format!(".{f}.iter"),
            format!("&self.s.{f}"),
            format!("of(&self.s.{f})"),
            format!(".{f}.get"),
            format!("self.s.{f}["),
        ] {
            if src.contains(&pat) {
                read = true;
                break;
            }
        }
        if !read {
            write_only.insert(f.clone());
        }
    }
    assert!(
        write_only.is_empty(),
        "Storage fields written but never read: {write_only:?} -- this is what \
         `tmpl` was, 1 MB filled on every style miss and consulted by nothing"
    );
}
