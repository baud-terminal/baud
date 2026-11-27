//! **No code outside baud-domain reads a `ColorWord` or `StyleWord` from
//! bytes.** zerocopy's derived `TryFromBytes` on the two types checks the
//! colour word's tag but not its payload, so a read of outside bytes could
//! build a word no constructor writes. The exact check exists, held back
//! for what it costs the style key (decisions/0031); it lands in the same
//! change as the first outside reader, which this test turns red.
//!
//! A reader is a line that names either type together with a zerocopy byte
//! read or a transmute. The types' own file, `crates/baud-domain/src/style.rs`,
//! and this crate's tests are exempt. A generic read whose type is only
//! inferred (`T::try_read_from_bytes` with `T = StyleWord`) names neither
//! type on its line and is not seen; a reader written that way still has
//! to name the type where `T` is chosen.

use std::path::{Path, PathBuf};
use std::process::Command;

/// The two types whose byte reads are held back.
const TYPES: [&str; 2] = ["ColorWord", "StyleWord"];

/// The zerocopy calls that build a value from bytes, and the transmutes.
const READS: [&str; 9] = [
    "try_read_from",
    "try_ref_from",
    "try_mut_from",
    "read_from_bytes",
    "ref_from_bytes",
    "mut_from_bytes",
    "read_from_prefix",
    "ref_from_prefix",
    "transmute",
];

/// Where reads are allowed: the types' own file and this crate's tests.
fn exempt(path: &str) -> bool {
    path == "crates/baud-domain/src/style.rs" || path.starts_with("crates/baud-domain/tests/")
}

/// Whether `line` reads one of the two types from bytes.
fn reads(line: &str) -> bool {
    TYPES.iter().any(|t| line.contains(t)) && READS.iter().any(|r| line.contains(r))
}

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("the workspace")
        .to_path_buf()
}

/// Every tracked Rust source, by its path from the workspace root.
fn sources(root: &Path) -> Vec<String> {
    let out = Command::new("git")
        .current_dir(root)
        .args(["ls-files", "-z", "--", "*.rs"])
        .output()
        .expect("git ls-files");
    assert!(out.status.success(), "git ls-files: {out:?}");
    String::from_utf8(out.stdout)
        .expect("UTF-8 paths")
        .split('\0')
        .filter(|p| !p.is_empty())
        .map(str::to_owned)
        .collect()
}

/// Each `path:line` that reads one of the two types from bytes, the exempt
/// files included when `with_exempt` is set.
fn readers(root: &Path, with_exempt: bool) -> Vec<String> {
    let mut found = Vec::new();
    for path in sources(root) {
        if exempt(&path) && !with_exempt {
            continue;
        }
        let text = std::fs::read_to_string(root.join(&path)).unwrap_or_default();
        for (k, line) in text.lines().enumerate() {
            if reads(line) {
                found.push(format!("{path}:{}", k + 1));
            }
        }
    }
    found
}

#[test]
fn no_code_outside_baud_domain_reads_a_rendition_from_bytes() {
    let outside = readers(&root(), false);
    assert!(
        outside.is_empty(),
        "a ColorWord or StyleWord is read from bytes outside baud-domain; \
         land fcc449a7's exact check in the same change (decisions/0031): {outside:?}"
    );
}

/// Positive control: the scan reaches the files and the matcher fires. The
/// types' own file reads them from bytes in its witnesses, so a scan that
/// includes the exempt files finds lines there.
#[test]
fn the_scan_finds_the_exempt_readers_it_skips() {
    let all = readers(&root(), true);
    assert!(
        all.iter()
            .any(|r| r.starts_with("crates/baud-domain/src/style.rs:")),
        "the scan found no reader in style.rs, so it reads nothing: {all:?}"
    );
}

/// The matcher on planted lines: each read form fires, and a line naming a
/// type with no read, or a read with neither type, does not.
#[test]
fn the_matcher_fires_on_each_read_form_and_only_there() {
    for r in READS {
        assert!(reads(&format!("let w = ColorWord::{r}(&b);")), "{r}");
        assert!(reads(&format!("let w = <[StyleWord]>::{r}(b);")), "{r}");
    }
    assert!(!reads("let w = ColorWord::rgb(r, g, b);"));
    assert!(!reads("let p = Pixel::read_from_bytes(&b);"));
}
