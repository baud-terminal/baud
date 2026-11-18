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
        let line = tried.map(|t| format!("{t} lane 0000 2026-11-15\n"));
        self.scope_from("HEAD~1", line.as_deref())
    }

    /// The plan from `from` to the last commit, with `BAUD_TRIED_TREES`
    /// naming a file that holds `list`, when there is one.
    fn scope_from(&self, from: &str, list: Option<&str>) -> Vec<String> {
        let mut c = Command::new(env!("CARGO_BIN_EXE_xtask"));
        c.args(["test-scope", from, "HEAD"])
            .current_dir(&self.0.0)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env_remove("BAUD_TRIED_TREES");
        if let Some(text) = list {
            let file = self.path("tried-trees.txt");
            fs::write(&file, text).expect("the list");
            c.env("BAUD_TRIED_TREES", &file);
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
    r.put(".pre-commit-config.yaml", "repos: []\n");
    r.commit("hooks");
    let tree = r.git(&["rev-parse", "HEAD^{tree}"]);
    assert_eq!(r.scope_tried(Some(&tree)), ["full"]);
}

/// The base, a commit whose tree a try ran clean (lane a's change to
/// `baud-core`), and the tip, lane b's `change` on top of it. The base's
/// tree, the tried tree and a tree this repository does not hold make the
/// tried list.
fn tried_then(change: (&str, &str)) -> (Repo, String) {
    let r = Repo::new();
    let base = r.git(&["rev-parse", "HEAD^{tree}"]);
    r.put("core/src/lib.rs", "pub fn f() {}\npub fn g() {}\n");
    r.commit("lane a");
    let tried = r.git(&["rev-parse", "HEAD^{tree}"]);
    r.put(change.0, change.1);
    r.commit("lane b");
    let list = format!(
        "{base} base 000000000000 2026-11-15T10:00\n\
         {tried} lane-a 111111111111 2026-11-15T11:00\n\
         {} gone 222222222222 2026-11-15T12:00\n",
        "e".repeat(40)
    );
    (r, list)
}

#[test]
fn a_tip_a_commit_past_a_tried_tree_plans_only_what_differs_from_it() {
    let (r, list) = tried_then(("other/src/lib.rs", "pub fn f() {}\npub fn g() {}\n"));
    assert_eq!(
        r.scope_from("HEAD~2", Some(&list)),
        ["from-tried", "package other", "clippy other", "doc other"]
    );
    assert_eq!(
        r.scope_from("HEAD~2", None),
        [
            "package baud",
            "package baud-core",
            "package other",
            "clippy baud",
            "clippy baud-core",
            "clippy other",
            "doc baud",
            "doc baud-core",
            "doc other",
            "rig",
            "bare",
            "linux",
        ],
        "with no tried tree the whole range is planned"
    );
}

#[test]
fn a_shell_input_past_a_tried_tree_still_runs_everything() {
    let (r, list) = tried_then(("devenv.nix", "{ }\n"));
    assert_eq!(r.scope_from("HEAD~2", Some(&list)), ["full"]);
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
fn a_runner_configuration_other_than_nextests_runs_everything() {
    let r = Repo::new();
    r.put(".config/other.toml", "[profile.default]\n");
    r.commit("other configuration");
    assert_eq!(r.scope(), ["full"]);
}

/// The test runner's configuration, with an override whose filter is
/// `filter`.
fn nextest(filter: &str) -> String {
    format!(
        "[profile.default]\nslow-timeout = \"60s\"\n\n\
         [[profile.default.overrides]]\nfilter = '{filter}'\ntest-group = '@global'\n"
    )
}

#[test]
fn a_nextest_change_plans_the_binaries_and_packages_its_changed_lines_name() {
    let r = Repo::new();
    r.put(".config/nextest.toml", &nextest("binary(=demo)"));
    r.commit("nextest");
    r.put(".config/nextest.toml", &nextest("binary(readme)"));
    r.commit("the readme in place of the example");
    assert_eq!(
        r.scope(),
        ["binary baud baud::example/demo", "binary baud baud::readme"],
        "the line taken out names the example, the line put in the readme"
    );
    r.put(".config/nextest.toml", &nextest("package(other)"));
    r.commit("other whole");
    assert_eq!(
        r.scope(),
        ["package other", "binary baud baud::readme", "doc other"]
    );
}

#[test]
fn a_nextest_change_naming_no_binary_plans_what_reads_it() {
    let r = Repo::new();
    r.put(".config/nextest.toml", &nextest("binary(readme)"));
    r.put(
        "other/tests/runner.rs",
        "#[test]\nfn reads() { let _ = std::fs::read(\"../.config/nextest.toml\"); }\n",
    );
    r.commit("nextest and its reader");
    r.put(".config/nextest.toml", &nextest("binary(nowhere)"));
    r.commit("a binary no package has");
    assert_eq!(
        r.scope(),
        ["binary baud baud::readme", "binary other other::runner"]
    );
}

#[test]
fn a_reader_that_joins_the_manifest_directory_to_the_path_is_found() {
    let r = Repo::new();
    r.put(".config/nextest.toml", &nextest("binary(readme)"));
    r.put(
        "other/tests/window.rs",
        "#[test]\nfn reads() {\n    let path = concat!(env!(\"CARGO_MANIFEST_DIR\"), \
         \"/../.config/nextest.toml\");\n    let _ = std::fs::read(path);\n}\n",
    );
    r.commit("nextest and its reader");
    let text = fs::read_to_string(r.path(".config/nextest.toml")).expect("the configuration");
    r.put(".config/nextest.toml", &text.replace("\"60s\"", "\"30s\""));
    r.commit("a shorter period");
    assert_eq!(r.scope(), ["binary other other::window"]);
}

#[test]
fn a_pre_push_change_plans_what_names_it() {
    let r = Repo::new();
    r.put("scripts/pre-push.sh", "#!/bin/sh\nexit 0\n");
    r.put(
        "other/tests/hook.rs",
        "#[test]\nfn reads() { let _ = std::fs::read(\"../scripts/pre-push.sh\"); }\n",
    );
    r.commit("the hook and its reader");
    r.put("scripts/pre-push.sh", "#!/bin/sh\nexit 1\n");
    r.commit("the hook");
    assert_eq!(r.scope(), ["binary other other::hook"]);
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

#[test]
fn a_nextest_filter_the_plan_cannot_list_runs_everything() {
    let r = Repo::new();
    r.put(".config/nextest.toml", &nextest("binary(readme)"));
    r.commit("nextest");
    r.put(".config/nextest.toml", &nextest("rdeps(baud-core)"));
    r.commit("what builds from core");
    assert_eq!(r.scope(), ["full"]);
}
