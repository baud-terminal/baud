//! `cargo xtask test-scope` on a small workspace, between two commits: a
//! change to a crate pulls in every crate that builds from it, a change to
//! a document only the tests that read it, and a change to what every crate
//! builds from, everything.

mod common;

use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};

use common::Tree;
use xtask::test_scope::{Depended, Package, Target, binaries_of, is_doc, names};

static NEXT: AtomicUsize = AtomicUsize::new(0);

/// A repository holding four crates: `baud-core`, which compiles in a
/// table outside the crates; `baud`, which builds from it, compiles the
/// README into an example and has a test that reads it; `baud-font`; and
/// `other`, which builds from nothing.
struct Repo(Tree);

impl Repo {
    fn new() -> Self {
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let dir = env::temp_dir().join(format!("xtask-scope-{}-{n}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("the tree");
        let r = Self(Tree(dir));
        r.put(
            "Cargo.toml",
            "[workspace]\nmembers = [\"core\", \"app\", \"font\", \"other\"]\nresolver = \"2\"\n",
        );
        for (dir, name, deps) in [
            ("core", "baud-core", ""),
            (
                "app",
                "baud",
                "[dependencies]\nbaud-core = { path = \"../core\" }\n",
            ),
            ("font", "baud-font", ""),
            ("other", "other", ""),
        ] {
            r.put(
                &format!("{dir}/Cargo.toml"),
                &format!(
                    "[package]\nname = {name:?}\nversion = \"0.1.0\"\nedition = \"2021\"\n{deps}"
                ),
            );
            r.put(&format!("{dir}/src/lib.rs"), "pub fn f() {}\n");
        }
        r.put(
            "core/src/tables.rs",
            "pub const W: &str = include_str!(\"../../tables/w.txt\");\n",
        );
        r.put("tables/w.txt", "1\n");
        r.put(
            "app/examples/demo/main.rs",
            "const README: &str = include_str!(\"../../../README.md\");\nfn main() {}\n",
        );
        r.put(
            "app/tests/readme.rs",
            "#[test]\nfn reads() { let _ = std::fs::read(\"../README.md\"); }\n",
        );
        r.put("README.md", "# baud\n");
        r.put("LICENSES/x.txt", "a licence\n");
        r.git(&["init", "-q", "."]);
        r.commit("base");
        r
    }

    fn put(&self, rel: &str, text: &str) {
        self.0.put(rel, text);
    }

    fn git(&self, args: &[&str]) -> String {
        let mut c = Command::new("git");
        c.args(["-c", "user.name=x", "-c", "user.email=x@example.org"])
            .args(["-c", "gc.auto=0", "-c", "maintenance.auto=false"])
            .args(args)
            .current_dir(&self.0.0)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1");
        for v in ["GIT_DIR", "GIT_WORK_TREE", "GIT_INDEX_FILE", "GIT_PREFIX"] {
            c.env_remove(v);
        }
        let out = c.output().expect("git");
        assert!(out.status.success(), "git {args:?}: {out:?}");
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    fn commit(&self, message: &str) {
        self.git(&["add", "-A"]);
        self.git(&["commit", "-q", "--allow-empty", "-m", message]);
    }

    /// The plan's lines for the last commit, each cut to its step and
    /// what it names: the reasons are left out.
    fn scope(&self) -> Vec<String> {
        self.scope_tried(None)
    }

    /// The plan with `BAUD_TRIED_TREES` naming `tried`, a file of tree
    /// hashes a try ran clean, one a line.
    fn scope_tried(&self, tried: Option<&str>) -> Vec<String> {
        let mut c = Command::new(env!("CARGO_BIN_EXE_xtask"));
        c.args(["test-scope", "HEAD~1", "HEAD"])
            .current_dir(&self.0.0)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env_remove("BAUD_TRIED_TREES");
        if let Some(t) = tried {
            let list = self.path("tried-trees.txt");
            fs::write(&list, format!("{t} lane 0000 2026-11-15\n")).expect("the list");
            c.env("BAUD_TRIED_TREES", &list);
        }
        let out = c.stderr(Stdio::null()).output().expect("xtask");
        assert_eq!(out.status.code(), Some(0), "{out:?}");
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .map(|l| {
                let f: Vec<&str> = l.split('\t').collect();
                let keep = match f[0] {
                    "binary" => 3,
                    "package" | "clippy" | "doc" => 2,
                    _ => 1,
                };
                f[..keep.min(f.len())].join(" ")
            })
            .collect()
    }

    fn path(&self, rel: &str) -> PathBuf {
        self.0.path(rel)
    }
}

#[test]
fn a_change_in_core_pulls_in_every_crate_that_builds_from_it() {
    let r = Repo::new();
    r.put("core/src/lib.rs", "pub fn f() {}\npub fn g() {}\n");
    r.commit("core");
    assert_eq!(
        r.scope(),
        [
            "package baud",
            "package baud-core",
            "clippy baud",
            "clippy baud-core",
            "doc baud",
            "doc baud-core",
            "rig",
            "bare",
            "linux",
        ]
    );
}

#[test]
fn a_test_only_change_in_core_selects_only_its_own_binary() {
    let r = Repo::new();
    r.put("core/tests/t.rs", "#[test]\nfn t() {}\n");
    r.commit("core test");
    assert_eq!(
        r.scope(),
        ["binary baud-core baud-core::t", "clippy baud-core"]
    );
}

#[test]
fn a_change_in_a_crate_nothing_builds_from_stays_in_it() {
    let r = Repo::new();
    r.put("other/src/lib.rs", "pub fn f() {}\npub fn g() {}\n");
    r.commit("other");
    assert_eq!(r.scope(), ["package other", "clippy other", "doc other"]);
}

#[test]
fn a_readme_change_runs_only_the_tests_that_read_it() {
    let r = Repo::new();
    r.put("README.md", "# baud\n\nA terminal.\n");
    r.commit("readme");
    assert_eq!(
        r.scope(),
        ["binary baud baud::example/demo", "binary baud baud::readme"]
    );
}

#[test]
fn data_a_library_compiles_in_reaches_every_crate_that_builds_from_it() {
    let r = Repo::new();
    r.put("tables/w.txt", "2\n");
    r.commit("table");
    assert_eq!(
        r.scope(),
        [
            "package baud",
            "package baud-core",
            "clippy baud",
            "clippy baud-core",
            "doc baud",
            "doc baud-core",
            "rig",
            "bare",
            "linux",
        ]
    );
}

#[test]
fn a_tree_a_try_ran_clean_runs_only_the_gates_that_build_nothing() {
    let r = Repo::new();
    r.put("core/src/lib.rs", "pub fn f() {}\npub fn g() {}\n");
    r.commit("core");
    let tree = r.git(&["rev-parse", "HEAD^{tree}"]);
    assert_eq!(r.scope_tried(Some(&tree)), ["tried"]);
    let other = r.git(&["rev-parse", "HEAD~1^{tree}"]);
    assert_eq!(
        r.scope_tried(Some(&other)),
        r.scope(),
        "a tree no try ran is scoped as before"
    );
}

#[test]
fn a_tried_tree_that_reaches_every_crate_still_runs_everything() {
    let r = Repo::new();
    r.put(".config/nextest.toml", "[profile.default]\n");
    r.commit("nextest");
    let tree = r.git(&["rev-parse", "HEAD^{tree}"]);
    assert_eq!(r.scope_tried(Some(&tree)), ["full"]);
}

#[test]
fn a_file_no_test_reads_runs_nothing() {
    let r = Repo::new();
    r.put("LICENSES/x.txt", "a licence, changed\n");
    r.commit("licence");
    assert_eq!(r.scope(), Vec::<String>::new());
}

#[test]
fn what_every_crate_builds_from_runs_everything() {
    let r = Repo::new();
    let manifest = fs::read_to_string(r.path("Cargo.toml")).expect("the manifest");
    r.put("Cargo.toml", &format!("{manifest}\n[workspace.package]\n"));
    r.commit("manifest");
    assert_eq!(r.scope(), ["full"]);
}

#[test]
fn the_test_runners_configuration_runs_everything() {
    let r = Repo::new();
    r.put(".config/nextest.toml", "[profile.default]\n");
    r.commit("nextest");
    assert_eq!(r.scope(), ["full"]);
}

#[test]
fn the_gif_runs_its_census_and_no_crate() {
    let r = Repo::new();
    r.put("assets/readme/session.gif", "GIF89a");
    r.commit("gif");
    assert_eq!(r.scope(), ["gif"]);
}

#[test]
fn a_literal_names_a_path_its_directories_its_tail_or_its_extension() {
    for (lit, path) in [
        ("README.md", "README.md"),
        ("../../README.md", "README.md"),
        ("README.md", "docs/README.md"),
        ("docs", "docs/guide/x.md"),
        ("docs/", "docs/guide/x.md"),
        (".md", "docs/guide/x.md"),
        ("assets/readme", "assets/readme/session.gif"),
    ] {
        assert!(names(lit, path), "{lit:?} names {path:?}");
    }
    for (lit, path) in [
        ("", "README.md"),
        (".", "README.md"),
        ("../", "README.md"),
        ("ADME.md", "README.md"),
        ("doc", "docs/x.md"),
        (".m", "x.md"),
        ("{dir}/README.md", "README.md"),
        ("a README.md", "README.md"),
    ] {
        assert!(!names(lit, path), "{lit:?} does not name {path:?}");
    }
}

#[test]
fn a_source_belongs_to_the_binaries_whose_root_is_deepest_above_it() {
    let t = |name: &str, kind: &str, src: &str| Target {
        name: name.into(),
        kind: kind.into(),
        src: src.into(),
    };
    let p = Package {
        name: "baud".into(),
        dir: "app".into(),
        depended_on: Depended::No,
        targets: vec![
            t("baud", "lib", "app/src/lib.rs"),
            t("baud", "bin", "app/src/main.rs"),
            t("rig", "test", "app/tests/rig.rs"),
            t("conformance", "test", "app/tests/conformance/main.rs"),
            t("demo", "example", "app/examples/demo/main.rs"),
            t("frame", "bench", "app/benches/frame.rs"),
            t("build-script-build", "custom-build", "app/build.rs"),
        ],
    };
    let ids = |f: &str| binaries_of(&p, f);
    assert_eq!(
        ids("app/src/paths.rs"),
        Some(vec!["baud".to_string(), "baud::bin/baud".to_string()])
    );
    assert_eq!(ids("app/tests/rig.rs"), Some(vec!["baud::rig".to_string()]));
    assert_eq!(
        ids("app/tests/conformance/vt.rs"),
        Some(vec!["baud::conformance".to_string()])
    );
    assert_eq!(
        ids("app/examples/demo/session.rs"),
        Some(vec!["baud::example/demo".to_string()])
    );
    assert_eq!(
        ids("app/tests/common/mod.rs"),
        Some(vec!["baud::rig".to_string()])
    );
    assert_eq!(ids("app/benches/frame.rs"), Some(Vec::<String>::new()));
    assert_eq!(ids("app/build.rs"), Some(Vec::<String>::new()));
    assert_eq!(ids("app/tools/x.rs"), None);
}

#[test]
fn documentation_is_markdown_and_the_readme_assets() {
    assert!(is_doc("README.md"));
    assert!(is_doc("docs/guide/x.md"));
    assert!(is_doc("assets/readme/session.gif"));
    assert!(!is_doc("assets/demo/links.txt"));
    assert!(!is_doc("baud-core/src/lib.rs"));
}
