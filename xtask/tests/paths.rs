//! `cargo xtask paths`: a manifest's `path` that leaves the tree, or names
//! nothing, turns the gate red at that manifest; a target's source and a
//! path inside stay green; too few manifests fails the control; and the
//! repository itself passes.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use xtask::paths::{self, Verdict};
use xtask::third_party::Outcome;

static NEXT: AtomicUsize = AtomicUsize::new(0);

/// A scratch directory holding `repo/` with ten member manifests,
/// `m0` to `m9`, and `outside/`, a directory beside it.
struct Tree(PathBuf);

impl Drop for Tree {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

const MEMBERS: [&str; 10] = ["m0", "m1", "m2", "m3", "m4", "m5", "m6", "m7", "m8", "m9"];
const PACKAGE: &str = "[package]\nname = \"m\"\n";

impl Tree {
    fn new() -> Self {
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let d = std::env::temp_dir().join(format!("xtask-paths-{}-{n}", std::process::id()));
        fs::create_dir_all(d.join("outside")).expect("outside");
        let t = Self(d);
        for m in MEMBERS {
            t.write(&format!("{m}/Cargo.toml"), PACKAGE);
        }
        t
    }

    fn repo(&self) -> PathBuf {
        self.0.join("repo")
    }

    fn write(&self, path: &str, text: &str) -> &Self {
        let full = self.repo().join(path);
        fs::create_dir_all(full.parent().expect("a parent")).expect("the parent");
        fs::write(full, text).expect("the file");
        self
    }

    fn run(&self) -> (Outcome, String) {
        let mut out = Vec::new();
        let o = paths::run(&self.repo(), &mut out).expect("the gate");
        (o, String::from_utf8(out).expect("UTF-8"))
    }
}

const OK: &str = "ok: 10 manifests, every path dependency inside the repository\n";

#[test]
fn ten_manifests_with_paths_inside_pass() {
    let t = Tree::new();
    t.write(
        "m0/Cargo.toml",
        "[package]\nname = \"m0\"\n\n[dependencies]\nm1 = { path = \"../m1\" }\n\n\
         [[bin]]\nname = \"m0\"\npath = \"src/main.rs\"\n",
    );
    assert_eq!(t.run(), (Outcome::Passed, OK.to_string()));
}

#[test]
fn a_path_that_leaves_the_repository_fails_at_its_manifest() {
    let t = Tree::new();
    t.write(
        "m3/Cargo.toml",
        "[dependencies]\nfar = { path = \"../../outside\" }\n",
    );
    let (o, report) = t.run();
    assert_eq!(o, Outcome::Failed, "{report}");
    assert_eq!(
        report,
        "FAIL ./m3/Cargo.toml: \"../../outside\" leaves the repository\n    \
         That is a build that works on one machine. Vendor it, take it\n    \
         from a registry, or make it a git dependency.\n\
         FAIL: a path dependency leaves the repository\n"
    );
}

#[test]
fn a_path_that_names_no_directory_resolves_to_nothing() {
    let t = Tree::new();
    t.write("m1/notes.txt", "a file, not a directory\n");
    t.write(
        "m1/Cargo.toml",
        "[dependencies]\ngone = { path = \"../gone\" }\nfile = { path = \"notes.txt\" }\n",
    );
    let (o, report) = t.run();
    assert_eq!(o, Outcome::Failed, "{report}");
    assert_eq!(
        report,
        "FAIL ./m1/Cargo.toml: path \"../gone\" resolves to nothing on this machine\n\
         FAIL ./m1/Cargo.toml: path \"notes.txt\" resolves to nothing on this machine\n\
         FAIL: a path dependency leaves the repository\n"
    );
}

#[test]
fn an_absolute_path_is_compared_as_written() {
    let t = Tree::new();
    let top = fs::canonicalize(t.repo()).expect("the root");
    let outside = fs::canonicalize(t.0.join("outside")).expect("outside");
    let m = "./m0/Cargo.toml";
    let inside = top.join("m1");
    let inside = inside.to_str().expect("UTF-8");
    let outside = outside.to_str().expect("UTF-8");
    assert_eq!(paths::verdict(&t.repo(), &top, m, inside), Verdict::Inside);
    assert_eq!(
        paths::verdict(&t.repo(), &top, m, outside),
        Verdict::Outside
    );
    // Written absolute, a path inside is inside whether or not it exists.
    let unbuilt = top.join("not-yet");
    let unbuilt = unbuilt.to_str().expect("UTF-8");
    assert_eq!(paths::verdict(&t.repo(), &top, m, unbuilt), Verdict::Inside);
}

#[test]
fn every_path_on_a_line_is_read_and_a_source_file_is_not() {
    assert_eq!(
        paths::values(
            "a = { path = \"m1\" }, b = { path  =  \"../x\" }\n\
             # path = \"../commented\"\n\
             path = \"src/lib.rs\"\n\
             include-path = \"inc\"\n"
        ),
        ["m1", "../x", "../commented", "inc"]
    );
}

#[test]
fn fewer_than_the_floor_of_manifests_fails_the_control() {
    let t = Tree::new();
    fs::remove_file(t.repo().join("m9/Cargo.toml")).expect("one manifest gone");
    assert_eq!(
        t.run(),
        (
            Outcome::Failed,
            "FAIL: only 9 manifests found -- this gate scanned almost nothing\n".to_string()
        )
    );
}

#[test]
fn target_and_git_at_the_root_are_not_read_and_nested_ones_are() {
    let t = Tree::new();
    let far = "[dependencies]\nfar = { path = \"../../../outside\" }\n";
    t.write("target/x/Cargo.toml", far);
    t.write(".git/x/Cargo.toml", far);
    assert_eq!(t.run(), (Outcome::Passed, OK.to_string()));
    t.write("m0/target/Cargo.toml", far);
    let (o, report) = t.run();
    assert_eq!(o, Outcome::Failed, "{report}");
    assert!(
        report.starts_with("FAIL ./m0/target/Cargo.toml: \"../../../outside\" leaves"),
        "{report}"
    );
}

#[cfg(unix)]
#[test]
fn a_linked_directory_is_not_entered() {
    let t = Tree::new();
    std::os::unix::fs::symlink(t.repo().join("m0"), t.repo().join("again")).expect("the link");
    assert_eq!(t.run(), (Outcome::Passed, OK.to_string()));
}

#[test]
fn the_manifests_are_listed_in_byte_order() {
    let t = Tree::new();
    t.write("m0-b/Cargo.toml", PACKAGE);
    t.write("Cargo.toml", PACKAGE);
    let found = paths::manifests(&t.repo());
    assert_eq!(
        found[..3],
        ["./Cargo.toml", "./m0-b/Cargo.toml", "./m0/Cargo.toml"]
    );
    assert_eq!(found.len(), 12);
}

fn repository() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the repository")
}

#[test]
fn the_repository_s_path_dependencies_stay_inside_it() {
    let mut out = Vec::new();
    let o = paths::run(repository(), &mut out).expect("the gate");
    let report = String::from_utf8(out).expect("UTF-8");
    assert_eq!(o, Outcome::Passed, "{report}");
    assert!(report.starts_with("ok: "), "{report}");
}
