#![deny(dead_code)]
//! The feature list and the roadmap are generated, and the data under them is
//! held to the tree.
//!
//! `docs/guide/features.md` and `docs/roadmap.md` are written by
//! `scripts/gen-features.py` from `docs/data/features.toml` and
//! `docs/data/roadmap.toml`. Its `--check` fails when a page differs from what
//! it writes, when a proof path or a proof test is gone, when a feature marked
//! built names no test, done coverage row or measurement, when a built
//! feature's coverage row is not done, when a shipped roadmap item is
//! claimed by no feature, and when README.md's "See all N features" is not
//! the list's count. Of twelve terminals surveyed, five keep a second
//! feature list by hand -- iTerm2, contour, rio, Windows Terminal and Wave --
//! and all five drifted from their programs; this one fails the build first.

/// The pages are what the generator writes, and every proof they rest on
/// still exists.
#[test]
fn feature_list_matches_its_data_and_its_proofs() {
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/..");
    let out = std::process::Command::new("python3")
        .arg(format!("{root}/scripts/gen-features.py"))
        .arg("--check")
        .current_dir(root)
        .output()
        .expect("python3 to run the generator");
    assert!(
        out.status.success(),
        "the feature list or the roadmap has drifted -- fix docs/data/ and rerun \
         scripts/gen-features.py\n{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}
