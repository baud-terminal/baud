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
//!
//! **And when that commit was made**, for the About window's age line: what
//! the packager says (`BAUD_GIT_TIME`, which the flake sets from
//! `self.lastModified`), else git's committer time of `HEAD`, else nothing.
//! It is the commit's, not the build's, so two builds of one commit say the
//! same; on 936f9c19 `nix flake metadata` gave `lastModified` 1794042906 and
//! `git log -1 --format=%ct` the same number. A build named `unknown` has no
//! commit and so no time, whatever was set.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

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

/// **A commit time as git and nix write it**: whole seconds since 1970, in
/// decimal, or `None` for anything else. Zero is `None` too: it is no
/// commit's time, and what a source with no history could report.
pub(crate) fn time_of(s: &str) -> Option<SystemTime> {
    let secs = Some(s.trim())
        .filter(|t| !t.is_empty() && t.bytes().all(|b| b.is_ascii_digit()))?
        .parse()
        .ok()
        .filter(|&n| n != 0)?;
    SystemTime::UNIX_EPOCH.checked_add(Duration::from_secs(secs))
}

/// **`BAUD_GIT_TIME`'s text as a commit time**: empty or `0` is no time, as
/// the flake writes when nix has none, and anything else not a time is a
/// packager's mistake, which a build refuses as [`told`] does.
pub(crate) fn told_time(s: &str) -> Result<Option<SystemTime>, String> {
    match s.trim() {
        "" | "0" => Ok(None),
        t => time_of(t).map(Some).ok_or_else(|| {
            format!(
                "BAUD_GIT_TIME={s:?} is not a commit time: whole seconds since 1970, in decimal"
            )
        }),
    }
}

/// **What the build stamps as its commit's time**: whole seconds since
/// 1970, or empty with no time or no commit (`id` [`UNKNOWN`]), so the
/// variable is always set and `env!` compiles on every target.
pub(crate) fn stamp(id: &str, time: Option<SystemTime>) -> String {
    let since = time.and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok());
    match since {
        Some(since) if id != UNKNOWN => since.as_secs().to_string(),
        _ => String::new(),
    }
}

/// What git said of the tree.
#[derive(Debug, PartialEq)]
pub(crate) struct Read {
    /// `<hash>`, or `<hash>.dirty`.
    pub(crate) id: String,
    /// `HEAD`'s committer time, `%ct`, or `None` when git did not say: the
    /// age line is left out, and the build's name is unaffected.
    pub(crate) time: Option<SystemTime>,
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
    // `--no-show-signature`: a `log.showSignature` set in a config would
    // print a signature's lines before the time.
    let time = git(&["log", "-1", "--no-show-signature", "--format=%ct", "HEAD"])
        .as_deref()
        .and_then(time_of);
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
        time,
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
