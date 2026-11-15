//! **baud's version, and the two places outside Cargo that spell it.**
//!
//! The version is `1.0.0-unstable`, then `1.0.0-unstable.N`: SemVer orders
//! the bare one first, and the series goes from there to `1.0.0`, never
//! through an `-rc`, which sorts below every `-unstable`
//! (`release-plz.toml` has the rule). The workspace version is the one source:
//! every crate inherits it, `build.rs` adds the build's identifier after a
//! `+` for what the app says of itself -- `TERM_PROGRAM_VERSION` and
//! XTVERSION among it (`baud-app/src/pty.rs` holds those two to each other),
//! and `scripts/bundle-macos.sh` writes it into the app bundle's
//! Info.plist, which Apple allows only digits and periods. Beside it the
//! script writes the build number, a counter committed in
//! `packaging/macos/build-number` and raised by hand per release.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, SystemTime};

#[path = "../src/build_id.rs"]
#[allow(dead_code)]
mod build_id;

fn workspace() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..")
}

/// `[workspace.package]`'s `version`, read from the manifest rather than
/// from `CARGO_PKG_VERSION`, so a crate that stopped inheriting it shows.
fn workspace_version() -> String {
    let toml = std::fs::read_to_string(workspace().join("Cargo.toml")).expect("Cargo.toml");
    let table = &toml[toml
        .find("[workspace.package]")
        .expect("[workspace.package]")..];
    let line = table
        .lines()
        .find(|l| l.starts_with("version"))
        .expect("[workspace.package] has a version");
    line.split('"').nth(1).expect("a quoted version").to_owned()
}

/// A decimal integer as semver writes one: digits, and no leading zero.
fn integer(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()) && (s == "0" || !s.starts_with('0'))
}

/// The scheme: `1.0.0-unstable`, or `1.0.0-unstable.N` with N a positive
/// integer.
#[test]
fn the_version_is_the_unstable_scheme() {
    let v = workspace_version();
    if v != "1.0.0-unstable" {
        let n = v
            .strip_prefix("1.0.0-unstable.")
            .unwrap_or_else(|| panic!("{v} is neither 1.0.0-unstable nor 1.0.0-unstable.N"));
        assert!(integer(n) && n != "0", "{v}: N is a positive integer");
    }
    assert_eq!(env!("CARGO_PKG_VERSION"), v, "the baud package inherits it");
}

/// The `<string>` after `<key>{key}</key>`.
fn plist_string(plist: &str, key: &str) -> String {
    let k = format!("<key>{key}</key>");
    let rest = &plist[plist.find(&k).unwrap_or_else(|| panic!("no {key}")) + k.len()..];
    let open = rest.find("<string>").expect("a <string> follows") + "<string>".len();
    let close = rest.find("</string>").expect("and closes");
    rest[open..close].to_owned()
}

/// **The bundle's two version keys, as the script writes them.**
///
/// `CFBundleShortVersionString`: "The required format is three
/// period-separated integers, such as 10.14.1. The string can only contain
/// numeric characters (0-9) and periods"
/// (`[[source: apple-cfbundleshortversionstring]]`) -- so `1.0.0`, the
/// version less its pre-release part. `CFBundleVersion`: "one to three period-separated
/// integers ... can only contain numeric characters (0-9) and periods"
/// (`[[source: apple-cfbundleversion]]`). The script is run, not read, so
/// what is checked is what it emits.
#[test]
fn the_bundle_says_the_version_as_apple_allows() {
    let out = Command::new("bash")
        .arg(workspace().join("scripts/bundle-macos.sh"))
        .arg("--info-plist")
        .output()
        .expect("bash runs");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let plist = String::from_utf8(out.stdout).expect("UTF-8");
    assert!(
        !plist.contains("__"),
        "a placeholder left unsubstituted:\n{plist}"
    );

    let v = workspace_version();
    let release = v.split('-').next().unwrap();
    let short = plist_string(&plist, "CFBundleShortVersionString");
    assert_eq!(short, release, "the version less its pre-release part");
    let parts: Vec<&str> = short.split('.').collect();
    assert!(
        parts.len() == 3 && parts.iter().all(|p| integer(p)),
        "{short}"
    );

    let build = plist_string(&plist, "CFBundleVersion");
    let parts: Vec<&str> = build.split('.').collect();
    assert!(
        (1..=3).contains(&parts.len()) && parts.iter().all(|p| integer(p)),
        "CFBundleVersion {build}: one to three integers"
    );
}

/// **Every version of the scheme reaches the plist, and no other does**: the
/// script run from a copy of what it reads, each version in that copy's
/// manifest, so the next bump is tested before it happens. Without this the
/// script stops, and with it the nix package. Perturbation: take `-unstable`
/// out of its pattern, or make the `.N` required; let it take any letters and
/// the `-rc` and the old `-beta` are written.
#[test]
fn the_bundle_takes_each_shape_of_the_scheme() {
    let plist_of = |version: &str, at: &str| {
        let root = Path::new(env!("CARGO_TARGET_TMPDIR"))
            .join("bundle-version")
            .join(at);
        std::fs::create_dir_all(root.join("scripts")).expect("the copy");
        std::fs::create_dir_all(root.join("packaging/macos")).expect("the copy");
        for file in [
            "scripts/bundle-macos.sh",
            "packaging/macos/Info.plist",
            "packaging/macos/build-number",
        ] {
            std::fs::copy(workspace().join(file), root.join(file)).expect(file);
        }
        std::fs::write(
            root.join("Cargo.toml"),
            format!("[workspace.package]\nversion = \"{version}\"\n"),
        )
        .expect("the manifest");
        Command::new("bash")
            .arg(root.join("scripts/bundle-macos.sh"))
            .arg("--info-plist")
            .output()
            .expect("bash runs")
    };
    for (at, version, short) in [
        ("release", "1.2.3", "1.2.3"),
        ("bare", "1.2.3-unstable", "1.2.3"),
        ("numbered", "1.2.3-unstable.4", "1.2.3"),
    ] {
        let out = plist_of(version, at);
        assert!(
            out.status.success(),
            "{version}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        let plist = String::from_utf8(out.stdout).expect("UTF-8");
        assert_eq!(
            plist_string(&plist, "CFBundleShortVersionString"),
            short,
            "{version}"
        );
    }
    for (at, version) in [
        ("empty-number", "1.2.3-unstable."),
        ("zero-led", "1.2.3-unstable.01"),
        ("two-numbers", "1.2.3-unstable.1.2"),
        ("alpha", "1.2.3-alpha"),
        ("beta", "1.2.3-beta"),
        ("rc", "1.2.3-rc.1"),
        ("capital", "1.2.3-Unstable"),
        ("two-parts", "1.2"),
    ] {
        let out = plist_of(version, at);
        assert!(!out.status.success(), "{version} is refused");
        assert!(out.stdout.is_empty(), "{version} writes no plist");
    }
}

/// The build number committed beside the plist: one line, raised by hand
/// for each release.
fn build_number() -> String {
    let path = workspace().join("packaging/macos/build-number");
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path:?}: {e}"));
    text.trim_end_matches('\n').to_owned()
}

/// **`CFBundleVersion` is the committed counter, not anything git knows.**
///
/// A counter raised by hand per release, kept in the repository, so a
/// build from a tarball or nix, with no `.git`, writes the same number.
/// The script is run, so what is checked is what it emits.
#[test]
fn the_bundles_build_number_is_the_committed_counter() {
    let counter = build_number();
    let parts: Vec<&str> = counter.split('.').collect();
    assert!(
        (1..=3).contains(&parts.len()) && parts.iter().all(|p| integer(p)),
        "packaging/macos/build-number {counter:?}: one to three integers, one line"
    );
    let out = Command::new("bash")
        .arg(workspace().join("scripts/bundle-macos.sh"))
        .arg("--info-plist")
        .output()
        .expect("bash runs");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let plist = String::from_utf8(out.stdout).expect("UTF-8");
    assert_eq!(
        plist_string(&plist, "CFBundleVersion"),
        counter,
        "the bundle's CFBundleVersion is packaging/macos/build-number"
    );
}

// ------------------------------------------------- the build's identifier

const ROOT: &str = "/repo";
const CRATE: &str = "/repo/baud-app";
const REV: &str = "0123abcdef0123abcdef0123abcdef0123abcdef";
/// 936f9c19's committer time, as `git log -1 --format=%ct` prints it and
/// nix's `lastModified` holds it.
const TIME: &str = "1794042906";

/// [`TIME`] as a time.
fn committed() -> SystemTime {
    SystemTime::UNIX_EPOCH + Duration::from_secs(1_794_042_906)
}

/// A git that answers as `answers` says, each command by its arguments
/// joined with spaces, and fails every other: no real repository in any of
/// these, so what `build_id::read` makes of each answer is all that is
/// tested.
fn fake(answers: BTreeMap<&'static str, &'static str>) -> impl Fn(&[&str]) -> Option<String> {
    move |args| {
        answers
            .get(args.join(" ").as_str())
            .map(|a| (*a).to_owned())
    }
}

/// A clean checkout on a branch; each test changes what it is about.
fn clean() -> BTreeMap<&'static str, &'static str> {
    BTreeMap::from([
        ("rev-parse --show-toplevel", ROOT),
        ("rev-parse HEAD", REV),
        ("log -1 --no-show-signature --format=%ct HEAD", TIME),
        (
            "--no-optional-locks status --porcelain --untracked-files=no",
            "",
        ),
        ("rev-parse --path-format=absolute --git-dir", "/repo/.git"),
        (
            "rev-parse --path-format=absolute --git-common-dir",
            "/repo/.git",
        ),
        ("symbolic-ref -q HEAD", "refs/heads/master"),
        (
            "ls-tree --full-tree --name-only HEAD",
            "Cargo.toml\nbaud-app\ndocs",
        ),
    ])
}

/// **A clean tree is the commit's first seven digits**, nix's `shortRev`
/// (`[[source: nix-call-flake]]`). Red first: a `read` that returned the whole
/// rev would print differently from the flake.
#[test]
fn a_clean_tree_is_the_short_hash() {
    let read = build_id::read(Path::new(CRATE), &fake(clean())).expect("git has an answer");
    assert_eq!(read.id, "0123abc");
    assert!(build_id::is_build_metadata(&read.id));
}

/// **A tracked file differing from the commit is `.dirty`**; an empty status
/// is the positive control above.
#[test]
fn a_dirty_tree_says_so() {
    let mut a = clean();
    a.insert(
        "--no-optional-locks status --porcelain --untracked-files=no",
        "M baud-app/build.rs",
    );
    let read = build_id::read(Path::new(CRATE), &fake(a)).expect("git has an answer");
    assert_eq!(read.id, "0123abc.dirty");
    assert!(build_id::is_build_metadata(&read.id));
}

/// **No git, and a git that knows no commit, say nothing**, and the build
/// is `unknown`, a valid identifier. So does a failed status: a tree that
/// cannot be asked is not known clean.
#[test]
fn no_git_is_unknown() {
    let none = |_: &[&str]| None;
    assert_eq!(build_id::read(Path::new(CRATE), &none), None);
    for gone in [
        "rev-parse HEAD",
        "--no-optional-locks status --porcelain --untracked-files=no",
    ] {
        let mut a = clean();
        a.remove(gone);
        assert_eq!(build_id::read(Path::new(CRATE), &fake(a)), None, "{gone}");
    }
    assert!(build_id::is_build_metadata(build_id::UNKNOWN));
    assert_eq!(
        build_id::full("1.0.0-unstable", build_id::UNKNOWN),
        "1.0.0-unstable+unknown"
    );
}

/// **A crate unpacked under some other repository is not that repository's
/// commit**: `cargo package`'s folder, a registry under a home directory in
/// git. git walks upward and answers; the top of its tree is not this
/// package's parent, so that answer is not this build's. Perturbation:
/// drop the check and the unpacked crate prints the outer hash.
#[test]
fn another_repository_above_is_not_this_build() {
    for dir in [
        "/repo/target/package/baud-1.0.0",
        "/home/me/.cargo/registry/src/baud-1.0.0",
        "/repo",
    ] {
        assert_eq!(
            build_id::read(Path::new(dir), &fake(clean())),
            None,
            "{dir}"
        );
    }
}

/// **What the answer can change on is what Cargo watches.** `HEAD`, the
/// branch, `packed-refs` and a reftable for a new commit, the index for staging, and every
/// entry at the top of the tree for an edit, in a worktree each from where
/// git keeps it. Perturbation: watch only `HEAD` and the ref, and an edit
/// after a clean build leaves the binary saying it is clean.
#[test]
fn every_way_the_answer_can_change_is_watched() {
    let mut a = clean();
    a.insert(
        "rev-parse --path-format=absolute --git-dir",
        "/repo/.git/worktrees/lane",
    );
    let watch = build_id::read(Path::new(CRATE), &fake(a))
        .expect("read")
        .watch;
    for want in [
        "/repo/.git/worktrees/lane/HEAD",
        "/repo/.git/worktrees/lane/index",
        "/repo/.git/packed-refs",
        "/repo/.git/reftable",
        "/repo/.git/refs/heads/master",
        "/repo/Cargo.toml",
        "/repo/baud-app",
        "/repo/docs",
    ] {
        assert!(watch.contains(&PathBuf::from(want)), "{want} in {watch:?}");
    }
    let mut detached = clean();
    detached.remove("symbolic-ref -q HEAD");
    let watch = build_id::read(Path::new(CRATE), &fake(detached))
        .expect("detached is still a commit")
        .watch;
    assert!(watch.contains(&PathBuf::from("/repo/.git/HEAD")));
}

/// The target folder the tests below build outside the tree, and inside the
/// crate; the filesystem, as far as `build_id::rerun_on` can ask.
const OUT_AWAY: &str = "/elsewhere/target/debug/build/baud-1/out";
const OUT_IN_CRATE: &str = "/repo/baud-app/target/debug/build/baud-1/out";

fn crate_entries(dir: &Path) -> Vec<PathBuf> {
    assert_eq!(dir, Path::new(CRATE), "only the crate's folder is listed");
    ["Cargo.toml", "build.rs", "src", "target"]
        .map(|e| dir.join(e))
        .to_vec()
}

/// `rerun_on` over `clean()` with `gone` the paths that do not exist.
fn rerun(answers: BTreeMap<&'static str, &'static str>, out: &str, gone: &[&str]) -> Vec<PathBuf> {
    let read = build_id::read(Path::new(CRATE), &fake(answers)).expect("read");
    let exists = |p: &Path| !gone.iter().any(|g| p == Path::new(g));
    build_id::rerun_on(&read, Path::new(out), &exists, &crate_entries)
}

fn holds(rerun: &[PathBuf], path: &str) -> bool {
    rerun.contains(&PathBuf::from(path))
}

/// **With everything there and the target elsewhere, Cargo is told what
/// `read` watches**, the crate's folder whole. The positive control for the
/// tests after it.
#[test]
fn everything_there_is_watched_as_it_is() {
    let read = build_id::read(Path::new(CRATE), &fake(clean())).expect("read");
    let told = rerun(clean(), OUT_AWAY, &[]);
    assert_eq!(told, read.watch);
    assert!(holds(&told, "/repo/.git/refs/heads/master"));
    assert!(holds(&told, CRATE));
}

/// **A ref `git gc` packed is watched through the folder its next commit
/// writes in.** Cargo reads a missing path as changed, so naming the ref
/// rebuilt `baud` on every build until the next commit, and naming nothing
/// would miss that commit. A branch with a slash, whose folder went with it,
/// takes the nearest there. Perturbation: print the path as it is, or skip it.
#[test]
fn a_packed_ref_is_watched_through_the_folder_its_next_commit_writes_in() {
    let told = rerun(clean(), OUT_AWAY, &["/repo/.git/refs/heads/master"]);
    assert!(!holds(&told, "/repo/.git/refs/heads/master"), "{told:?}");
    assert!(holds(&told, "/repo/.git/refs/heads"), "{told:?}");

    let mut nested = clean();
    nested.insert("symbolic-ref -q HEAD", "refs/heads/lane/b");
    let told = rerun(
        nested,
        OUT_AWAY,
        &["/repo/.git/refs/heads/lane/b", "/repo/.git/refs/heads/lane"],
    );
    assert!(!holds(&told, "/repo/.git/refs/heads/lane/b"), "{told:?}");
    assert!(!holds(&told, "/repo/.git/refs/heads/lane"), "{told:?}");
    assert!(holds(&told, "/repo/.git/refs/heads"), "{told:?}");
}

/// **No `packed-refs` and no `reftable` is nothing to watch**: `git init` and
/// one commit has neither, and packing never changes what `HEAD` resolves
/// to. Each is watched once it is there. Perturbation: print them anyway and
/// every build of a new repository rebuilds.
#[test]
fn what_git_has_not_written_is_not_watched() {
    let gone = ["/repo/.git/packed-refs", "/repo/.git/reftable"];
    let told = rerun(clean(), OUT_AWAY, &gone);
    for path in gone {
        assert!(!holds(&told, path), "{path} in {told:?}");
    }
    let told = rerun(clean(), OUT_AWAY, &[]);
    for path in gone {
        assert!(holds(&told, path), "{path} in {told:?}");
    }
}

/// **A tracked entry gone from the tree stays named.** The tree is dirty and
/// nothing else would see the entry come back, so it costs a rebuild per
/// build rather than a binary that says `.dirty` of a clean tree.
/// Perturbation: drop every missing path, as a packed ref's fix could.
#[test]
fn a_tracked_entry_gone_stays_watched() {
    let told = rerun(clean(), OUT_AWAY, &["/repo/docs"]);
    assert!(holds(&told, "/repo/docs"), "{told:?}");
}

/// **A target folder inside the crate is left out of the crate's watch.**
/// Cargo walks a watched folder, so every build changed what it walked and
/// the next one rebuilt: `CARGO_TARGET_DIR=./target`, as the dev shell has it,
/// from `baud-app/`. The crate's other entries still see an edit, and another
/// top-level entry stays whole. Perturbation: print the folder, or the entry
/// holding `OUT_DIR` among its entries.
#[test]
fn the_target_folder_is_left_out_of_the_crates_watch() {
    let told = rerun(clean(), OUT_IN_CRATE, &[]);
    assert!(!holds(&told, CRATE), "{told:?}");
    assert!(!holds(&told, "/repo/baud-app/target"), "{told:?}");
    for path in [
        "/repo/baud-app/Cargo.toml",
        "/repo/baud-app/build.rs",
        "/repo/baud-app/src",
        "/repo/docs",
    ] {
        assert!(holds(&told, path), "{path} in {told:?}");
    }
}

/// **The two together**: a packed ref and no `packed-refs`, with the target
/// inside the crate, leave nothing Cargo would read as changed.
#[test]
fn a_packed_ref_and_a_target_in_the_crate_hold_together() {
    let gone = [
        "/repo/.git/refs/heads/master",
        "/repo/.git/packed-refs",
        "/repo/.git/reftable",
    ];
    let told = rerun(clean(), OUT_IN_CRATE, &gone);
    for path in gone.iter().chain(&[CRATE, "/repo/baud-app/target"]) {
        assert!(!holds(&told, path), "{path} in {told:?}");
    }
    for path in [
        "/repo/.git/refs/heads",
        "/repo/baud-app/src",
        "/repo/.git/HEAD",
    ] {
        assert!(holds(&told, path), "{path} in {told:?}");
    }
}

/// **The commit's time is git's committer time of `HEAD`**, what nix's
/// `lastModified` is for a git flake; a git that does not say leaves the
/// time out and the build's name as it was, and an answer that is not a
/// time is no time. Perturbation: ask `HEAD`'s time with `?` and the second
/// case is `None` whole; read the author's time, `%at`, and the fake answers
/// nothing.
#[test]
fn the_commit_time_is_read_from_git() {
    let read = build_id::read(Path::new(CRATE), &fake(clean())).expect("git has an answer");
    assert_eq!(read.time, Some(committed()));
    let mut a = clean();
    a.remove("log -1 --no-show-signature --format=%ct HEAD");
    let read = build_id::read(Path::new(CRATE), &fake(a)).expect("the name without the time");
    assert_eq!((read.id.as_str(), read.time), ("0123abc", None));
    for bad in ["", "0", "abc", "-1", "1794042906 extra"] {
        let mut a = clean();
        a.insert("log -1 --no-show-signature --format=%ct HEAD", bad);
        let read = build_id::read(Path::new(CRATE), &fake(a)).expect("read");
        assert_eq!(read.time, None, "{bad:?}");
    }
}

/// **The flake's time, `BAUD_GIT_TIME`, read as git's**: seconds since 1970,
/// `0` or empty no time, anything else refused, as `told` refuses a name;
/// and the stamp is empty for `unknown` whatever time it was told, so no
/// age line counts from a commit nobody named. Perturbation: a `stamp`
/// that ignores the id writes the time beside `unknown`.
#[test]
fn the_commit_time_is_told_by_the_flake() {
    assert_eq!(build_id::told_time(TIME), Ok(Some(committed())));
    assert_eq!(build_id::told_time("0"), Ok(None));
    assert_eq!(build_id::told_time(""), Ok(None));
    for bad in ["abc", "-1", "1.5", "1794042906s"] {
        assert!(build_id::told_time(bad).is_err(), "{bad:?}");
    }
    assert_eq!(build_id::stamp("0123abc", Some(committed())), TIME);
    assert_eq!(build_id::stamp("0123abc.dirty", Some(committed())), TIME);
    assert_eq!(build_id::stamp("0123abc", None), "");
    assert_eq!(build_id::stamp(build_id::UNKNOWN, Some(committed())), "");
}

/// **What a packager says comes first, read as a clone would say it**: nix's
/// `dirtyShortRev` ends `-dirty`, and a mistake is refused rather than
/// printed.
#[test]
fn what_the_packager_says_is_normalised_or_refused() {
    assert_eq!(build_id::told("0123abc").as_deref(), Ok("0123abc"));
    assert_eq!(
        build_id::told("0123abc-dirty").as_deref(),
        Ok("0123abc.dirty")
    );
    assert_eq!(build_id::told("unknown").as_deref(), Ok("unknown"));
    for bad in ["a..b", "a_b", "a+b", "", ".", "a b"] {
        assert!(build_id::told(bad).is_err(), "{bad:?}");
    }
}

/// **SemVer's `<valid semver>`** (`[[source: semver]]`), written out here
/// rather than through `build_id`'s check, which is what is under test:
/// `<version core>`, then `-` and pre-release identifiers, then `+` and build
/// identifiers.
fn valid_semver(s: &str) -> bool {
    fn ids(s: &str, numeric_rule: bool) -> bool {
        s.split('.').all(|id| {
            !id.is_empty()
                && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
                && (!numeric_rule || !id.bytes().all(|b| b.is_ascii_digit()) || integer(id))
        })
    }
    let (rest, build) = match s.split_once('+') {
        Some((r, b)) => (r, Some(b)),
        None => (s, None),
    };
    let (core, pre) = match rest.split_once('-') {
        Some((c, p)) => (c, Some(p)),
        None => (rest, None),
    };
    let parts: Vec<&str> = core.split('.').collect();
    parts.len() == 3
        && parts.iter().all(|p| integer(p))
        && pre.is_none_or(|p| ids(p, true))
        && build.is_none_or(|b| ids(b, false))
}

/// **Each answer makes a valid SemVer version, this build's own among them**,
/// and SemVer's own examples and non-examples hold the checker to its text:
/// `1.0.0-beta+exp.sha.5114f85` and `1.0.0+21AF26D3----117B344092BD` are from
/// §10; a leading zero is fine in a build identifier and not in a
/// pre-release's numeric one; an empty identifier is refused anywhere.
#[test]
fn every_answer_is_valid_semver() {
    for good in [
        "1.0.0-alpha+001",
        "1.0.0+20130313144700",
        "1.0.0-beta+exp.sha.5114f85",
        "1.0.0+21AF26D3----117B344092BD",
        "1.0.0+0123abc",
    ] {
        assert!(valid_semver(good), "{good}");
    }
    for bad in [
        "1.0.0+",
        "1.0.0+a..b",
        "1.0.0+a_b",
        "1.0.0+a+b",
        "1.0.0-01+a",
        "1.0+a",
        "01.0.0+a",
    ] {
        assert!(!valid_semver(bad), "{bad}");
    }
    for id in ["0123abc", "0123abc.dirty", "0000000", build_id::UNKNOWN] {
        let full = build_id::full(&workspace_version(), id);
        assert!(valid_semver(&full), "{full}");
    }
    let this = env!("BAUD_VERSION");
    assert!(valid_semver(this), "this build: {this}");
    assert!(
        this.starts_with(&format!("{}+", workspace_version())),
        "{this}: the manifests' version, then the build"
    );
}

/// **A manifest never holds a hash**: the workspace's version has no `+`, and
/// no crate carries a version of its own beside it.
#[test]
fn the_manifests_keep_the_plain_version() {
    let v = workspace_version();
    assert!(!v.contains('+'), "{v}");
    assert_eq!(env!("CARGO_PKG_VERSION"), v);
}

/// **The flake tells the build its commit**: a nix build has no `.git`, and
/// `build.rs` reads `BAUD_GIT_REV`. Read from the flake, since a test cannot
/// run nix, and `self` has to be in the outputs' arguments for it to mean
/// anything.
#[test]
fn the_flake_says_the_commit() {
    let flake = std::fs::read_to_string(workspace().join("flake.nix")).expect("flake.nix");
    let outputs = &flake[flake.find("outputs =").expect("outputs")..];
    assert!(
        outputs[..outputs.find("}:").expect("arguments")].contains("self"),
        "outputs takes self"
    );
    assert!(
        flake.contains(r#"env.BAUD_GIT_REV = self.shortRev or self.dirtyShortRev or "unknown";"#),
        "the flake passes it"
    );
    assert!(
        flake.contains("env.BAUD_GIT_TIME = toString (self.lastModified or 0);"),
        "and its time"
    );
}
