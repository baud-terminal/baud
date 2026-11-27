//! **No build that ships turns a test-only feature of `baud` on.**
//!
//! A feature of this package whose comment says it is never in a build
//! that ships is test-only: it changes what the binary does so a test can
//! read it, and a binary with it is not the program. The builds that ship
//! are the flake's package (`flake.nix`, `-p baud`), the macOS bundle
//! (`tools/scripts/bundle-macos.sh`, `cargo build --release -p baud`) and
//! `cargo install baud`, which builds the default features. Each takes the
//! features the manifest's `default` names and those its command line
//! names, so a test-only feature is refused in `default`, in any feature
//! that ships and turns it on, in the workspace's manifest, and in the
//! two build commands. The refusals are checked against planted texts
//! first, so a check that cannot fire fails.

use std::path::PathBuf;

/// The words that mark a feature test-only, in the comment above it.
const MARK: &str = "never in a build that ships";

/// The test-only features the manifest must mark, at the least.
const KNOWN: [&str; 2] = ["effect-probe", "slide-hold"];

fn read(rel: &str) -> String {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(rel);
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
}

/// The `[features]` block of a manifest, to the next section.
fn features(manifest: &str) -> &str {
    let (_, rest) = manifest.split_once("\n[features]\n").expect("[features]");
    rest.split("\n[").next().unwrap_or(rest)
}

/// Each feature of a `[features]` block: its name, the features its array
/// names, and the comment above it, joined to one line.
fn declared(block: &str) -> Vec<(String, String, String)> {
    let mut out = Vec::new();
    let mut comment = String::new();
    for line in block.lines() {
        let line = line.trim();
        if let Some(c) = line.strip_prefix('#') {
            comment.push(' ');
            comment.push_str(c.trim());
        } else if let Some((name, value)) = line.split_once('=') {
            let words: Vec<&str> = comment.split_whitespace().collect();
            out.push((
                name.trim().to_owned(),
                value.trim().to_owned(),
                words.join(" ").to_lowercase(),
            ));
            comment.clear();
        } else {
            comment.clear();
        }
    }
    out
}

/// The features of `manifest` marked test-only.
fn test_only(manifest: &str) -> Vec<String> {
    declared(features(manifest))
        .into_iter()
        .filter(|(_, _, comment)| comment.contains(MARK))
        .map(|(name, _, _)| name)
        .collect()
}

/// Whether `text` names the feature `name` as a word: quoted, or after
/// `--features`.
fn names(text: &str, name: &str) -> bool {
    text.match_indices(name).any(|(at, _)| {
        let before = text[..at].chars().next_back();
        let after = text[at + name.len()..].chars().next();
        let edge =
            |c: Option<char>| !c.is_some_and(|c| c.is_alphanumeric() || c == '-' || c == '_');
        edge(before) && edge(after)
    })
}

/// The lines of `text` that are not comments.
fn code(text: &str) -> String {
    text.lines()
        .filter(|l| !l.trim_start().starts_with('#'))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Every way the shipping builds turn one of `only` on: the package's
/// `default` or another of its features that ships naming it, the
/// workspace's manifest naming it, the flake's package passing features,
/// and the bundle's build passing them.
fn refusals(only: &[String], manifest: &str, root: &str, flake: &str, bundle: &str) -> Vec<String> {
    let mut found = Vec::new();
    for (name, value, _) in declared(features(manifest)) {
        if only.contains(&name) {
            continue;
        }
        for t in only {
            if names(&value, t) {
                found.push(format!("{name} turns {t} on: {value}"));
            }
        }
    }
    for t in only {
        if names(&code(root), t) {
            found.push(format!("the workspace's Cargo.toml names {t}"));
        }
    }
    for word in ["buildFeatures", "--features", "--all-features"] {
        if flake.contains(word) {
            found.push(format!("flake.nix's package passes {word}"));
        }
    }
    for t in only {
        if names(flake, t) {
            found.push(format!("flake.nix names {t}"));
        }
    }
    for line in bundle.lines().filter(|l| l.contains("cargo build")) {
        if ["--features", "--all-features", " -F "]
            .iter()
            .any(|w| line.contains(w))
        {
            found.push(format!(
                "the bundle's build passes features: {}",
                line.trim()
            ));
        }
    }
    found
}

/// **Every test-only feature is marked, and no shipping build has one.**
/// Positive control: the marked features include `effect-probe` and
/// `slide-hold`. Planted controls: `slide-hold` put in `default`, in a
/// feature that ships, in the workspace's manifest, in the flake's
/// `buildFeatures`, and on the bundle's build line, each refused.
#[test]
fn no_build_that_ships_turns_a_test_only_feature_on() {
    let manifest = read("crates/baud-app/Cargo.toml");
    let root = read("Cargo.toml");
    let flake = read("flake.nix");
    let bundle = read("tools/scripts/bundle-macos.sh");
    let only = test_only(&manifest);
    for k in KNOWN {
        assert!(
            only.iter().any(|o| o == k),
            "{k} is marked test-only (\"{MARK}\") in crates/baud-app/Cargo.toml: {only:?}"
        );
    }
    assert!(
        flake.contains("\"baud\"") && flake.contains("buildRustPackage"),
        "flake.nix builds the package baud"
    );
    assert!(
        bundle.contains("cargo build --release -p baud"),
        "the bundle builds the package baud"
    );

    let plant = |from: &str, to: &str, text: &str| {
        assert!(text.contains(from), "the plant's anchor {from:?}");
        text.replacen(from, to, 1)
    };
    let take = "take = []";
    let planted = [
        (
            "default",
            plant(
                "\n[features]\n",
                "\n[features]\ndefault = [\"slide-hold\"]\n",
                &manifest,
            ),
            root.clone(),
            flake.clone(),
            bundle.clone(),
        ),
        (
            "a shipping feature",
            plant(take, "take = [\"slide-hold\"]", &manifest),
            root.clone(),
            flake.clone(),
            bundle.clone(),
        ),
        (
            "the workspace",
            manifest.clone(),
            format!("{root}\n[workspace.metadata.plant]\non = [\"slide-hold\"]\n"),
            flake.clone(),
            bundle.clone(),
        ),
        (
            "the flake",
            manifest.clone(),
            root.clone(),
            plant(
                "cargoBuildFlags = [",
                "buildFeatures = [ \"slide-hold\" ];\n          cargoBuildFlags = [",
                &flake,
            ),
            bundle.clone(),
        ),
        (
            "the bundle",
            manifest.clone(),
            root.clone(),
            flake.clone(),
            plant(
                "cargo build --release -p baud",
                "cargo build --release -p baud --features slide-hold",
                &bundle,
            ),
        ),
    ];
    for (what, m, r, f, b) in &planted {
        let only = test_only(m);
        let found = refusals(&only, m, r, f, b);
        assert!(
            found.iter().any(|x| x.contains("slide-hold")),
            "slide-hold planted in {what} is refused: {found:?}"
        );
    }

    let found = refusals(&only, &manifest, &root, &flake, &bundle);
    assert_eq!(
        found,
        Vec::<String>::new(),
        "a build that ships turns a test-only feature on"
    );
}
