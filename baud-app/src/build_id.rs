//! **What the build calls itself after the `+`**, for `build.rs` and, by
//! `#[path]`, for `baud-app/tests/version.rs`: pure, so every path is
//! tested with no git at all. It runs in a build script and depends on
//! nothing.
//!
//! The version is the package's, as the manifests have it, and `+` adds
//! SemVer build metadata (`[[source: semver]]` §10): "dot separated
//! identifiers immediately following the patch or pre-release version.
//! Identifiers MUST comprise only ASCII alphanumerics and hyphens
//! [0-9A-Za-z-]. Identifiers MUST NOT be empty." Precedence ignores it, so
//! a per-commit hash orders nothing, and it never enters a manifest.
//!
//! **Three answers and none lies**: what the packager says
//! (`BAUD_GIT_REV`, which the nix flake sets, since a nix build has no
//! `.git`); else the commit git names, `.dirty` when a tracked file differs
//! from it; else `unknown`, a tarball's and a registry's answer.

use std::path::{Path, PathBuf};

/// The answer with nothing to read: a tarball, a registry, no git.
pub(crate) const UNKNOWN: &str = "unknown";

/// **Hexadecimal digits of the commit, seven**: what nix's `self.shortRev`
/// has, so one commit prints the same from the flake and from a clone.
pub(crate) const HASH: usize = 7;

/// **Whether `s` is SemVer build metadata**, the grammar's
/// `<dot-separated build identifiers>` (`[[source: semver]]`): one or more
/// non-empty identifiers of `[0-9A-Za-z-]`, a leading zero allowed, since a
/// `<build identifier>` may be `<digits>`, unlike a pre-release's.
pub(crate) fn is_build_metadata(s: &str) -> bool {
    s.split('.')
        .all(|id| !id.is_empty() && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-'))
}

/// **`BAUD_GIT_REV`'s text as an identifier.** Nix's `self.dirtyShortRev`
/// ends `-dirty`, a valid identifier that reads differently from a clone's
/// answer, so it becomes `.dirty`. An error is a packager's mistake, which
/// a build refuses rather than print: the number is the build's name.
pub(crate) fn told(rev: &str) -> Result<String, String> {
    let id = match rev.strip_suffix("-dirty") {
        Some(hash) => format!("{hash}.dirty"),
        None => rev.to_owned(),
    };
    match is_build_metadata(&id) {
        true => Ok(id),
        false => Err(format!(
            "BAUD_GIT_REV={rev:?} is not SemVer build metadata: dot-separated, non-empty \
             identifiers of [0-9A-Za-z-]"
        )),
    }
}

/// What git said of the tree.
#[derive(Debug, PartialEq)]
pub(crate) struct Read {
    /// `<hash>`, or `<hash>.dirty`.
    pub(crate) id: String,
    /// What changes when the answer can: a new commit moves `HEAD`, the ref
    /// it names, `packed-refs` or a reftable, staging moves the index, and an
    /// edit of a tracked file moves its entry at the top of the tree. Cargo
    /// reruns the script on any of them, or an edit after a clean build
    /// would leave the binary saying it is clean. [`rerun_on`] says which of
    /// these Cargo can be given.
    pub(crate) watch: Vec<PathBuf>,
    /// The folder git shares between worktrees, which holds the refs.
    pub(crate) common: PathBuf,
}

/// **What git says of `crate_dir`'s tree**, or `None` when it has nothing
/// true to say. `git` runs one command in `crate_dir`, its trimmed output or
/// `None` on failure.
///
/// **The crate has to be a member at the top of git's tree**: git walks
/// upward, so a crate unpacked under some other repository -- a home
/// directory in git, `cargo package`'s `target/package/` -- would otherwise
/// report that repository's commit. A `crate_dir` whose parent is not the
/// tree's top says nothing.
pub(crate) fn read(crate_dir: &Path, git: &dyn Fn(&[&str]) -> Option<String>) -> Option<Read> {
    let top = git(&["rev-parse", "--show-toplevel"])?;
    if crate_dir.parent() != Some(Path::new(&top)) {
        return None;
    }
    let rev = git(&["rev-parse", "HEAD"])?;
    let hash = rev
        .get(..HASH)
        .filter(|h| rev.len() > HASH && h.bytes().all(|b| b.is_ascii_hexdigit()))?;
    // `--no-optional-locks`: a plain status refreshes the index, and the
    // index is watched, so every build would be followed by another.
    // `--untracked-files=no`: what nix's dirtiness means too, and an
    // untracked file reaches a build only through a tracked one naming it.
    let status = git(&[
        "--no-optional-locks",
        "status",
        "--porcelain",
        "--untracked-files=no",
    ])?;
    let id = match status.is_empty() {
        true => hash.to_owned(),
        false => format!("{hash}.dirty"),
    };
    let own = git(&["rev-parse", "--path-format=absolute", "--git-dir"])?;
    let common = git(&["rev-parse", "--path-format=absolute", "--git-common-dir"])?;
    let (own, common) = (Path::new(&own), Path::new(&common));
    let mut watch = vec![
        own.join("HEAD"),
        own.join("index"),
        common.join("packed-refs"),
        common.join("reftable"),
    ];
    // Detached, `HEAD` itself moves and this has no answer.
    if let Some(name) = git(&["symbolic-ref", "-q", "HEAD"]) {
        watch.push(common.join(name));
    }
    let tracked = git(&["ls-tree", "--full-tree", "--name-only", "HEAD"])?;
    watch.extend(tracked.lines().map(|entry| Path::new(&top).join(entry)));
    Some(Read {
        id,
        watch,
        common: common.to_owned(),
    })
}

/// **What Cargo is told to rerun for**, from [`Read::watch`]. Cargo reads a
/// `rerun-if-changed` path that does not exist as changed, on every build,
/// and walks one that is a folder, so two kinds of path would rebuild `baud`
/// on a build with nothing to do. `exists` and `entries` (a folder's
/// entries) are the filesystem; `out` is the build script's `OUT_DIR`.
///
/// **A path git has not written.** A ref that `git gc` or `git pack-refs`
/// moved into `packed-refs` is watched through the nearest folder that is
/// there, the one its next commit writes in. A missing `packed-refs` or
/// `reftable` needs no watch: packing never changes what `HEAD` resolves to.
/// A tracked entry gone from the tree stays as it is, rebuilding each time:
/// the tree is dirty, and nothing else would see the entry come back.
///
/// **The folder holding `OUT_DIR`.** A target folder inside a tracked entry,
/// which `CARGO_TARGET_DIR=./target` makes of a build from a crate's folder,
/// would change on every build the walk saw, so that entry is watched through
/// its own entries less the one holding `OUT_DIR`. A target folder deeper in
/// leaves out the whole of that one entry: Cargo has no watch that skips part
/// of a folder, and stepping down would watch the target's own `deps`.
pub(crate) fn rerun_on(
    read: &Read,
    out: &Path,
    exists: &dyn Fn(&Path) -> bool,
    entries: &dyn Fn(&Path) -> Vec<PathBuf>,
) -> Vec<PathBuf> {
    let refs = read.common.join("refs");
    let unwritten = [
        read.common.join("packed-refs"),
        read.common.join("reftable"),
    ];
    let mut rerun = Vec::with_capacity(read.watch.len());
    for path in &read.watch {
        if !exists(path) {
            if path.starts_with(&refs) {
                let there = path.ancestors().find(|a| exists(a)).unwrap_or(path);
                rerun.push(there.to_path_buf());
            } else if !unwritten.contains(path) {
                rerun.push(path.clone());
            }
        } else if out.starts_with(path) {
            rerun.extend(entries(path).into_iter().filter(|e| !out.starts_with(e)));
        } else {
            rerun.push(path.clone());
        }
    }
    rerun
}

/// **`<package version>+<identifier>`.**
pub(crate) fn full(package: &str, id: &str) -> String {
    format!("{package}+{id}")
}
