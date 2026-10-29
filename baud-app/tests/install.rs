//! **Nix and Cargo build baud with the compiler the repository uses.**
//! The dev shell takes nixpkgs and rust-overlay at the revisions
//! `devenv.lock` holds, and its nightly from that rust-overlay; the root
//! `flake.nix`, which `nix run` and `nix build` read, pins its own inputs
//! in `flake.lock` and names the nightly by date; and the install guides
//! give that date to `cargo +nightly-…`. This file fails on a lock bumped
//! alone and on a guide left on an old nightly. The dev shell names no
//! date, so both locks moved together can take its nightly past the
//! flake's; that pair is not held here.

use std::path::PathBuf;

fn workspace() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..")
}

fn read(rel: &str) -> String {
    std::fs::read_to_string(workspace().join(rel)).unwrap_or_else(|e| panic!("{rel}: {e}"))
}

/// The string value of `"key": "…"` in `block`, if it is there.
fn field<'a>(block: &'a str, key: &str) -> Option<&'a str> {
    let open = format!("\"{key}\": \"");
    let at = block.find(&open)? + open.len();
    let len = block[at..].find('"')?;
    Some(&block[at..at + len])
}

/// Every `"locked"` entry of a nix lock file, as (owner/repo, rev).
/// A lock's `"locked"` objects are flat, so each ends at the first `}`.
fn locked(lock: &str) -> Vec<(String, String)> {
    lock.split("\"locked\": {")
        .skip(1)
        .filter_map(|rest| {
            let block = &rest[..rest.find('}')?];
            let repo = format!("{}/{}", field(block, "owner")?, field(block, "repo")?);
            Some((repo, field(block, "rev")?.to_owned()))
        })
        .collect()
}

#[test]
fn the_flake_locks_the_dev_shells_nixpkgs_and_rust_overlay() {
    let flake = locked(&read("flake.lock"));
    let devenv = locked(&read("devenv.lock"));
    for repo in ["cachix/devenv-nixpkgs", "oxalica/rust-overlay"] {
        assert!(
            flake.iter().any(|(r, _)| r == repo),
            "flake.lock locks no {repo}: {flake:?}"
        );
    }
    for (repo, rev) in &flake {
        let shell: Vec<_> = devenv.iter().filter(|(r, _)| r == repo).collect();
        assert!(
            !shell.is_empty(),
            "flake.lock locks {repo}, which devenv.lock does not"
        );
        assert!(
            shell.iter().all(|(_, r)| r == rev),
            "flake.lock has {repo} at {rev}, devenv.lock at {shell:?}"
        );
    }
}

#[test]
fn the_guides_give_the_nightly_the_flake_builds_with() {
    let flake = read("flake.nix");
    let open = "rust-bin.nightly.\"";
    let at = flake.find(open).expect("flake.nix names a dated nightly") + open.len();
    let date = &flake[at..at + flake[at..].find('"').expect("the date closes")];
    assert_eq!(date.len(), "2026-04-11".len(), "{date:?} is not a date");

    for guide in ["README.md", "docs/guide/install.md", "baud-app/README.md"] {
        let text = read(guide);
        let named: Vec<_> = text
            .match_indices("+nightly-")
            .map(|(i, m)| &text[i + m.len()..(i + m.len() + date.len()).min(text.len())])
            .collect();
        assert!(!named.is_empty(), "{guide} gives no `cargo +nightly-…`");
        assert!(
            named.iter().all(|d| *d == date),
            "{guide} names nightly {named:?}, the flake {date}"
        );
    }
}
