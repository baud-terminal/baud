//! The copy functions in the crates' sources are as many as
//! `copy_audit::BUDGET`, and the finder that counts them finds a planted
//! one, in a file module and in an inline one.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use xtask::copy_audit::{BUDGET, copy_fns, find};
use xtask::impl_audit::Sites;

/// A directory under the system's temporary directory, removed on drop.
struct Scratch(PathBuf);

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

static NEXT: AtomicUsize = AtomicUsize::new(0);

fn scratch() -> Scratch {
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("xtask-copy-audit-{}-{n}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("a scratch directory");
    Scratch(dir)
}

fn put(root: &Path, file: &str, text: &str) {
    let path = root.join(file);
    fs::create_dir_all(path.parent().expect("a parent")).expect("a directory");
    fs::write(path, text).expect("a write");
}

fn repo() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("xtask sits in the repository root")
}

/// Named break: a copy function added under `copy::` with `BUDGET` left
/// as it was -- the count passes the budget and this fails; `BUDGET`
/// raised with no function added -- the count falls short and this fails.
#[test]
fn the_sources_hold_as_many_copy_fns_as_the_budget() {
    let found = find(repo());
    assert!(
        found
            .files
            .iter()
            .any(|f| f == "crates/baud-pipeline/src/handoff.rs"),
        "the walk did not read baud-pipeline: {} files",
        found.files.len()
    );
    assert_eq!(
        found.count(),
        BUDGET,
        "copy functions: {:?}; the budget changes in the commit that adds one",
        found.fns
    );
}

/// Named break: `find` stops reading `copy.rs` files or inline `mod copy`
/// blocks -- the planted functions go unfound and this fails.
#[test]
fn a_planted_copy_fn_is_found() {
    let s = scratch();
    let root = s.0.as_path();
    put(root, "c1/Cargo.toml", "[package]\nname = \"c1\"\n");
    put(root, "c1/src/lib.rs", "pub mod copy;\n");
    put(
        root,
        "c1/src/copy.rs",
        "/// fn in_a_doc()\npub fn x(counts: &mut Counts, row: &[Cell]) {}\n",
    );
    put(root, "c2/Cargo.toml", "[package]\nname = \"c2\"\n");
    put(
        root,
        "c2/src/lib.rs",
        "fn before() {}\npub(crate) mod copy {\n    pub fn y() {\n        if a {}\n    }\n}\nfn after() {}\n",
    );
    put(root, "c2/src/copy/thaw.rs", "pub(crate) const fn z() {}\n");
    put(root, "c3/Cargo.toml", "[package]\nname = \"c3\"\n");
    put(root, "c3/tests/copy.rs", "fn not_a_source() {}\n");
    let found = find(root);
    let names: Vec<(&str, &str)> = found
        .fns
        .iter()
        .map(|f| (f.at.as_str(), f.name.as_str()))
        .collect();
    assert_eq!(
        names,
        [
            ("c1/src/copy.rs:2", "x"),
            ("c2/src/copy/thaw.rs:1", "z"),
            ("c2/src/lib.rs:3", "y"),
        ]
    );
    assert_eq!(found.count(), Sites(3));
}

/// Named break: the inline module's end found at its first `}` -- `after`
/// is read as inside it and counted.
#[test]
fn an_inline_copy_module_ends_at_its_closing_brace() {
    let text = "mod copy {\n    fn a() {\n    }\n    fn b() {}\n}\nfn c() {}\nmod copy_like {\n    fn d() {}\n}\n";
    let names: Vec<String> = copy_fns("c/src/lib.rs", text)
        .into_iter()
        .map(|f| f.name)
        .collect();
    assert_eq!(names, ["a", "b"]);
    assert!(copy_fns("c/src/copy_audit.rs", "fn e() {}\n").is_empty());
    assert!(copy_fns("c/src/lib.rs", "// mod copy {\nfn f() {}\n").is_empty());
}
