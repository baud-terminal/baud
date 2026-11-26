//! `cargo xtask code-map`: the fixture maps to its pinned rows, a planted
//! function appears and a dropped pin line goes red, every public function
//! of baud-core is mapped, two runs over the tree write the same bytes, a
//! failure still writes `MAP`, and pruning deletes only the generator's
//! maps.

use std::collections::BTreeSet;
use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, SystemTime};

use xtask::code_map::{self, Keep};
use xtask::walk::{self, Depth};

const FIXTURE: &str = include_str!("fixtures/code_map/sample.rs");

/// The fixture's rows: every public item, impl block and foreign item, and
/// nothing from a comment, a string, a doc example, a macro's body or a
/// function's body.
const PINNED: &str = concat!(
    "kind\tname\tpath\tat\tsignature\tdoc\n",
    "fn\tplain\tsample::plain\tsample.rs:7\tpub fn plain()\tA plain function.\n",
    "fn\tspread\tsample::spread\tsample.rs:10\t",
    "pub fn spread<T, U>(the_first_argument_with_a_long_name: T, the_second_argument_with_a_long_name: U) \
     -> Option<T> where T: Clone + fmt::Debug, U: Into<String>",
    "\tA signature over several lines, with a where clause.\n",
    "fn\ton_linux\tsample::on_linux\tsample.rs:24\tpub fn on_linux() -> &'static str\tBuilt only on Linux.\n",
    "struct\tSample\tsample::Sample\tsample.rs:30\tpub struct Sample\tA type with an impl of a trait.\n",
    "impl\tSample\tsample::Sample\tsample.rs:34\timpl fmt::Display for Sample\t\n",
    "impl\tSample\tsample::Sample\tsample.rs:40\timpl Sample\t\n",
    "fn\tnew\tsample::Sample::new\tsample.rs:42\tpub fn new(name: String) -> Self\tMakes one.\n",
    "const\tLIMIT\tsample::LIMIT\tsample.rs:53\tpub(crate) const LIMIT: char = '}'\tA limit the crate shares.\n",
    "enum\tShape\tsample::Shape\tsample.rs:60\tpub enum Shape\tThe example's item is in a code block.\n",
    "static\tGREETING\tsample::GREETING\tsample.rs:69\tpub static GREETING: &str = \"pub fn in_a_string() {}\"\t\n",
    "const\tRAW\tsample::RAW\tsample.rs:70\tpub const RAW: &str = r#\"pub struct InARawString; \"{\"#\t\n",
    "type\tAlias\tsample::Alias\tsample.rs:72\tpub(super) type Alias<'a> = &'a Sample\t\n",
    "trait\tNamed\tsample::Named\tsample.rs:74\tpub trait Named\t\n",
    "mod\tinner\tsample::inner\tsample.rs:78\tpub mod inner\t\n",
    "fn\tdeep\tsample::inner::deep\tsample.rs:82\tpub fn deep()\tInside a module.\n",
    "macro\tsample_macro\tsample::sample_macro\tsample.rs:86\tmacro_rules! sample_macro\t\n",
    "fn\tdeclared\tsample::declared\tsample.rs:99\t",
    "pub fn declared(code: core::ffi::c_int) -> core::ffi::c_int\tDeclared, not defined.\n",
    "impl\t(A, B)\tsample::(A, B)\tsample.rs:102\timpl<A, B> Named for (A, B)\t\n",
    "const\tLONG\tsample::LONG\tsample.rs:109\tpub const LONG: [&str; 4] = …",
    "\tIts value is past what a signature shows.\n",
);

fn map(src: &str) -> String {
    code_map::items_tsv(&code_map::items(src, "sample.rs", "sample"))
}

#[test]
fn the_fixture_maps_to_its_pinned_items() {
    assert_eq!(map(FIXTURE), PINNED);
}

#[test]
fn a_files_module_doc_is_its_first_sentence() {
    assert_eq!(
        code_map::module_doc(FIXTURE).as_deref(),
        Some("A sample module for the code map's extractor.")
    );
    assert_eq!(code_map::module_doc("pub fn bare() {}\n"), None);
}

#[test]
fn a_planted_function_appears_and_a_dropped_pin_line_goes_red() {
    let planted = format!("{FIXTURE}\n/// Planted.\npub fn planted_control() {{}}\n");
    let rows = map(&planted);
    assert!(
        rows.contains("fn\tplanted_control\tsample::planted_control\tsample.rs:"),
        "the planted function is not mapped:\n{rows}"
    );
    assert!(rows.contains("\tpub fn planted_control()\tPlanted.\n"));
    assert_ne!(rows, PINNED);

    let dropped: String = PINNED
        .lines()
        .filter(|l| !l.starts_with("fn\tdeep\t"))
        .map(|l| format!("{l}\n"))
        .collect();
    assert_ne!(dropped, PINNED, "the pin line to drop is not in the pin");
    assert_ne!(map(FIXTURE), dropped);
}

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..")
}

#[test]
fn every_public_function_of_baud_core_is_mapped() {
    let root = root();
    let mut found = BTreeSet::new();
    let mut mapped = BTreeSet::new();
    for file in walk::find_rs(&root, "baud-core/src", Depth(1)) {
        let src = fs::read_to_string(root.join(&file)).expect("a baud-core source");
        // The pattern `every_public_function_in_the_new_core_is_called_by_a_test`
        // finds them with.
        for line in src.lines() {
            let t = line.trim_start();
            let rest = t
                .strip_prefix("pub fn ")
                .or_else(|| t.strip_prefix("pub unsafe fn "));
            if let Some(rest) = rest {
                let name: String = rest
                    .chars()
                    .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                    .collect();
                if !name.is_empty() {
                    found.insert(name);
                }
            }
        }
        for item in code_map::items(&src, &file, "baud_core") {
            if item.kind == code_map::Kind::Fn {
                mapped.insert(item.name);
            }
        }
    }
    assert!(
        found.len() > 20,
        "only {} public functions found",
        found.len()
    );
    let missing: Vec<&String> = found.difference(&mapped).collect();
    assert!(
        missing.is_empty(),
        "public functions not mapped: {missing:?}"
    );
}

static NEXT: AtomicUsize = AtomicUsize::new(0);

/// A directory under the build's own scratch directory, removed on drop.
struct Scratch(PathBuf);

impl Scratch {
    fn new(what: &str) -> Self {
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let dir = Path::new(env!("CARGO_TARGET_TMPDIR"))
            .join(format!("code-map-{what}-{}-{n}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("a scratch directory");
        Scratch(dir)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

const FILES: [&str; 5] = ["MAP", "index.md", "items.tsv", "crates.tsv", "gates.tsv"];

#[test]
fn two_runs_over_the_tree_write_the_same_bytes() {
    let scratch = Scratch::new("twice");
    let (a, b) = (scratch.0.join("a"), scratch.0.join("b"));
    let first = code_map::generate(&root(), &a);
    let second = code_map::generate(&root(), &b);
    assert_eq!(first.error, None);
    assert_eq!(first, second);
    for f in FILES {
        let left = fs::read(a.join(f)).expect("a map file");
        let right = fs::read(b.join(f)).expect("a map file");
        assert!(left == right, "{f} differs between two runs");
    }
    let map = fs::read_to_string(a.join("MAP")).expect("MAP");
    assert_eq!(map.lines().next(), Some(code_map::MARKER));
    assert!(!map.contains("error:"), "{map}");
    let crates = fs::read_to_string(a.join("crates.tsv")).expect("crates.tsv");
    assert!(crates.contains("\nbaud-core\tbaud-core\tyes\t"), "{crates}");
    let gates = fs::read_to_string(a.join("gates.tsv")).expect("gates.tsv");
    for row in [
        "perf-gate\tingest\tfn=ingest2::timed",
        "audit:CORE\tbaud-core\ttotal=",
        "audit:TREES\txtask/tests\tlimit=0\t",
        "witnessed\tSafeText\tE0308\tbaud-domain/tests/contract.rs:",
        "pin\tbaud_domain::AmbiguousWidth::Narrow\tbaud_domain::AmbiguousWidth\t",
    ] {
        assert!(gates.contains(row), "gates.tsv has no `{row}`");
    }
    let items = fs::read_to_string(a.join("items.tsv")).expect("items.tsv");
    assert!(items.contains("\tbaud_core::feed::Term::feed\tbaud-core/src/feed.rs:"));
}

#[test]
fn a_tree_that_cannot_be_mapped_still_writes_map_with_its_error() {
    let scratch = Scratch::new("bare");
    let out = scratch.0.join("out");
    let report = code_map::generate(&scratch.0.join("no-such-tree"), &out);
    assert!(report.error.is_some());
    let map = fs::read_to_string(out.join("MAP")).expect("MAP is written");
    assert_eq!(map.lines().next(), Some(code_map::MARKER));
    assert_eq!(map.lines().filter(|l| l.starts_with("error: ")).count(), 1);
}

fn stamped(dir: &Path, first: &str, age: Duration) {
    fs::create_dir_all(dir).expect("a map directory");
    let map = dir.join("MAP");
    fs::write(&map, format!("{first}\ntree 0\n")).expect("MAP");
    File::options()
        .write(true)
        .open(&map)
        .and_then(|f| f.set_modified(SystemTime::now() - age))
        .expect("MAP's time");
}

#[test]
fn pruning_deletes_only_the_generators_oldest_maps() {
    let scratch = Scratch::new("prune");
    let names: Vec<String> = (0..12).map(|n| format!("map{n:02}")).collect();
    for (n, name) in names.iter().enumerate() {
        let age = Duration::from_secs(60) * u32::try_from(n).expect("a small age");
        stamped(&scratch.0.join(name), code_map::MARKER, age);
    }
    // No MAP at all, and a MAP the generator did not write: both stay.
    let decoy = scratch.0.join("decoy");
    fs::create_dir_all(&decoy).expect("the decoy");
    fs::write(decoy.join("notes.txt"), "kept\n").expect("the decoy's file");
    stamped(
        &scratch.0.join("foreign"),
        "a MAP of something else",
        Duration::from_secs(86_400),
    );

    let deleted = code_map::prune(&scratch.0, Keep(10)).expect("a prune");
    assert_eq!(
        deleted,
        vec![scratch.0.join("map10"), scratch.0.join("map11")]
    );
    for name in &names[..10] {
        assert!(scratch.0.join(name).is_dir(), "{name} was deleted");
    }
    assert!(!scratch.0.join("map10").exists());
    assert!(decoy.join("notes.txt").is_file());
    assert!(scratch.0.join("foreign").join("MAP").is_file());
}
