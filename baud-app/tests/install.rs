//! **Nix builds baud with the compiler the repository uses, from the
//! repository's own address.** The dev shell takes nixpkgs and
//! rust-overlay at the revisions `devenv.lock` holds, and its nightly from
//! that rust-overlay; the root `flake.nix`, which `nix run` and `nix
//! build` read, pins its own inputs in `flake.lock` and names the nightly
//! by date; and the documents that name a nightly, CONTRIBUTING.md's
//! "Releases" and docs/embedding.md, give that date. This file fails on a
//! lock bumped alone and on a document left on an old nightly. The dev
//! shell names no date, so both locks moved together can take its nightly
//! past the flake's; that pair is not held here, and neither are the
//! date's other copies: the two `toolchain:` lines of the CI workflow and
//! the defaults of `scripts/linux-*.sh`.
//!
//! The install guides give `nix run` the address `Cargo.toml`'s
//! `repository` names, and no document, manifest or notice names another
//! home for the repository: a move of the repository is one line of
//! `Cargo.toml`, and this file lists every place left behind.

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
fn the_documents_give_the_nightly_the_flake_builds_with() {
    let flake = read("flake.nix");
    let open = "rust-bin.nightly.\"";
    let at = flake.find(open).expect("flake.nix names a dated nightly") + open.len();
    let date = &flake[at..at + flake[at..].find('"').expect("the date closes")];
    assert_eq!(date.len(), "2026-04-11".len(), "{date:?} is not a date");

    for doc in ["CONTRIBUTING.md", "docs/embedding.md"] {
        let text = read(doc);
        let named: Vec<_> = text
            .match_indices("nightly-")
            .map(|(i, m)| &text[i + m.len()..(i + m.len() + date.len()).min(text.len())])
            .collect();
        assert!(!named.is_empty(), "{doc} names no `nightly-…`");
        assert!(
            named.iter().all(|d| *d == date),
            "{doc} names nightly {named:?}, the flake {date}"
        );
    }
}

/// `account/name`, the repository's address on GitHub, from the
/// workspace's `repository`.
fn home() -> String {
    let cargo = read("Cargo.toml");
    let line = cargo
        .lines()
        .find(|l| l.starts_with("repository = \""))
        .expect("Cargo.toml names the repository");
    let url = line["repository = \"".len()..].trim_end_matches('"');
    url.strip_prefix("https://github.com/")
        .expect("the repository is on GitHub")
        .trim_end_matches('/')
        .to_owned()
}

/// Every file a reader or a build takes the repository's address from:
/// the Markdown, Nix and TOML files and the crates' NOTICEs that git
/// tracks. A file git does not track, such as a local `TODO.md`, is not
/// the repository's; the evidence is kept as it was fetched or recorded,
/// and the history as it was written.
fn addressed() -> Vec<PathBuf> {
    const EVIDENCE: [&str; 2] = ["docs/research/sources/", "tests/recordings/"];
    tracked()
        .iter()
        .map(String::as_str)
        .filter(|rel| {
            let name = rel.rsplit('/').next().unwrap_or(rel);
            let history = *rel == "docs/record.md" || name.starts_with("CHANGELOG");
            let kind =
                name == "NOTICE" || [".md", ".nix", ".toml"].iter().any(|x| name.ends_with(x));
            kind && !history && !EVIDENCE.iter().any(|e| rel.starts_with(e))
        })
        .map(|rel| workspace().join(rel))
        .collect()
}

/// The workspace's files, relative to its root: git's tracked list in a
/// checkout, and in a `git archive` export, which has no `.git` and holds
/// only tracked files, every file under the root but the build output.
fn tracked() -> Vec<String> {
    let root = workspace();
    if !root.join(".git").exists() {
        let mut found = Vec::new();
        let mut dirs = vec![root.clone()];
        while let Some(dir) = dirs.pop() {
            for e in std::fs::read_dir(&dir).expect("the workspace reads") {
                let e = e.expect("a directory entry");
                let kind = e.file_type().expect("its type");
                if kind.is_dir()
                    && !matches!(
                        e.file_name().to_str(),
                        Some("target" | ".devenv" | ".direnv")
                    )
                {
                    dirs.push(e.path());
                } else if kind.is_file() {
                    let rel = e
                        .path()
                        .strip_prefix(&root)
                        .expect("under the root")
                        .to_owned();
                    found.push(rel.to_str().expect("UTF-8 paths").to_owned());
                }
            }
        }
        return found;
    }
    let out = std::process::Command::new("git")
        .args(["ls-files", "-z"])
        .current_dir(&root)
        .output()
        .expect("git runs");
    assert!(
        out.status.success(),
        "git ls-files: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout)
        .expect("the tracked paths are UTF-8")
        .split('\0')
        .filter(|rel| !rel.is_empty())
        .map(str::to_owned)
        .collect()
}

/// Each `account/name` that `text` gives after `github.com/` or `github:`.
fn on_github(text: &str) -> Vec<String> {
    let part = |s: &str| {
        let n = s
            .find(|c: char| !(c.is_ascii_alphanumeric() || "-_.".contains(c)))
            .unwrap_or(s.len());
        s[..n].to_owned()
    };
    let mut found = Vec::new();
    for mark in ["github.com/", "github:"] {
        for (i, _) in text.match_indices(mark) {
            let rest = &text[i + mark.len()..];
            let account = part(rest);
            let Some(after) = rest[account.len()..].strip_prefix('/') else {
                continue;
            };
            let name = part(after);
            let name = name.trim_end_matches('.').trim_end_matches(".git");
            if !account.is_empty() && !name.is_empty() {
                found.push(format!("{account}/{name}"));
            }
        }
    }
    found
}

#[test]
fn every_address_of_the_repository_is_its_home() {
    let home = home();
    let name = home.split_once('/').map_or("", |(_, n)| n);
    assert!(!name.is_empty(), "{home:?} is not account/name");

    let files = addressed();
    let (mut homes, mut stale) = (0usize, Vec::new());
    for f in &files {
        let Ok(text) = std::fs::read_to_string(f) else {
            continue;
        };
        for at in on_github(&text) {
            if at == home {
                homes += 1;
            } else if at.split_once('/').is_some_and(|(_, n)| n == name) {
                let rel = f.strip_prefix(workspace()).expect("under the workspace");
                stale.push(format!("{}: {at}", rel.display()));
            }
        }
    }
    stale.sort();
    assert!(files.len() > 20, "only {} files read", files.len());
    assert!(
        stale.is_empty(),
        "the repository is {home}, and these name another home:\n  {}",
        stale.join("\n  ")
    );
    // The control: the probe can fire, and fires on the home itself.
    assert!(homes > 10, "the home {home} is named only {homes} times");

    // `nix run` with the home and nothing after its name.
    let run = format!("nix run github:{home}");
    for guide in ["README.md", "docs/guide/install.md", "baud-app/README.md"] {
        let text = read(guide);
        let gives = text.match_indices(&run).any(|(i, _)| {
            text[i + run.len()..]
                .chars()
                .next()
                .is_none_or(|c| c.is_whitespace() || c == '`')
        });
        assert!(gives, "{guide} does not give `{run}`");
    }
}
