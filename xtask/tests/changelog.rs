//! Each leg of `cargo xtask changelog` loses the commit it is for, and a
//! history with no commits fails.

use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

use xtask::changelog::{Commits, Why, check, run};
use xtask::third_party::Outcome;

mod exec;

const PLZ: &str = r#"
[changelog]
commit_parsers = [
  { message = "^feat", group = "Added" },
  { message = "^fix", group = "Fixed" },
  { message = ".*", skip = true },
]
"#;

const NO_CATCH_ALL: &str = r#"
[changelog]
commit_parsers = [
  { message = "^feat", group = "Added" },
]
"#;

fn cliff(commits: &[(&str, bool, Option<&str>)]) -> String {
    let list: Vec<String> = commits
        .iter()
        .map(|(id, conv, group)| {
            let g = group.map_or("null".to_string(), |g| format!("\"{g}\""));
            format!(r#"{{"id": "{id}", "conventional": {conv}, "group": {g}}}"#)
        })
        .collect();
    format!(r#"[{{"commits": [{}]}}]"#, list.join(", "))
}

const A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

fn log(lines: &[(&str, &str)]) -> String {
    lines.iter().map(|(s, m)| format!("{s}\0{m}\n")).collect()
}

fn whys(log_text: &str, plz: &str, cliff_text: &str) -> Vec<Why> {
    check(log_text, plz, cliff_text)
        .expect("the inputs parse")
        .bad
        .iter()
        .map(|b| b.why)
        .collect()
}

#[test]
fn a_clean_history_passes() {
    let v = check(
        &log(&[(A, "feat(app): a thing"), (B, "chore: tidy")]),
        PLZ,
        &cliff(&[(A, true, Some("Added")), (B, true, Some("Chores"))]),
    )
    .expect("parses");
    assert_eq!(v.commits, Commits(2));
    let (out, ok) = v.report();
    assert!(ok, "{out}");
    assert_eq!(out, "check-changelog: 2 commits, 0 failures\n");
}

#[test]
fn leg_one_loses_a_subject_that_is_not_a_listed_type() {
    for subject in ["Update things", "wip: half", "feat(Bad Scope): x", "feat:x"] {
        let w = whys(&log(&[(A, subject)]), PLZ, &cliff(&[(A, true, None)]));
        assert_eq!(w, [Why::NotConventional], "{subject}");
    }
}

#[test]
fn leg_two_loses_a_subject_no_parser_matches() {
    let w = whys(
        &log(&[(A, "chore: tidy")]),
        NO_CATCH_ALL,
        &cliff(&[(A, true, None)]),
    );
    assert_eq!(w, [Why::NoParser]);
}

/// The parsers are run unanchored, as release-plz runs them.
#[test]
fn a_parser_matches_anywhere_in_the_subject() {
    let inside = "[changelog]\ncommit_parsers = [{ message = \"inside\" }]\n";
    let w = whys(
        &log(&[(A, "fix: feat inside")]),
        inside,
        &cliff(&[(A, true, None)]),
    );
    assert!(w.is_empty(), "{w:?}");
}

#[test]
fn leg_three_loses_a_commit_git_cliff_missed_or_left_unparsed() {
    let w = whys(&log(&[(B, "fix: x")]), PLZ, &cliff(&[(A, true, None)]));
    assert_eq!(w, [Why::MissingFromCliff]);
    let w = whys(&log(&[(A, "fix: x")]), PLZ, &cliff(&[(A, false, None)]));
    assert_eq!(w, [Why::UnparsedByCliff]);
    let w = whys(
        &log(&[(A, "fix: x")]),
        PLZ,
        &cliff(&[(A, true, Some("UNPARSED commits"))]),
    );
    assert_eq!(w, [Why::UnparsedByCliff]);
}

#[test]
fn a_failure_is_reported_by_its_short_hash() {
    let v = check(&log(&[(A, "Update")]), PLZ, &cliff(&[(A, false, None)])).expect("parses");
    let (out, ok) = v.report();
    assert!(!ok);
    assert_eq!(
        out,
        "aaaaaaaaaaaa not a Conventional Commit: Update\n\
         aaaaaaaaaaaa unparsed by git-cliff: Update\n\
         check-changelog: 1 commits, 2 failures\n"
    );
}

#[test]
fn no_commits_fails() {
    let v = check("", PLZ, &cliff(&[])).expect("parses");
    let (out, ok) = v.report();
    assert!(!ok);
    assert_eq!(out, "check-changelog: 0 commits, 0 failures\n");
}

/// The repository's own `release-plz.toml` parses and its parsers compile.
#[test]
fn the_repository_parsers_compile() {
    let plz = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../release-plz.toml"))
        .expect("release-plz.toml");
    let v = check(&log(&[(A, "feat: x")]), &plz, &cliff(&[(A, true, None)])).expect("parses");
    assert!(v.bad.is_empty(), "{:?}", v.bad);
}

#[test]
fn a_commit_kept_as_written_is_left_out_by_its_full_id_only() {
    let kept = xtask::changelog::KEPT_AS_WRITTEN[0];
    let subject = "bench(frame): FRAME_FORGET prices forgetting what each read erased whole";
    let v = check(
        &log(&[(kept, subject)]),
        PLZ,
        &cliff(&[(kept, false, None)]),
    )
    .expect("parses");
    let (out, ok) = v.report();
    assert!(ok, "the listed id is left out of legs 1 and 3: {out}");
    let missing = check(&log(&[(kept, subject)]), PLZ, &cliff(&[])).expect("parses");
    assert!(
        missing.report().1,
        "the listed id need not be in git-cliff's context"
    );
    let w = whys(&log(&[(A, subject)]), PLZ, &cliff(&[(A, false, None)]));
    assert_eq!(
        w,
        [Why::NotConventional, Why::UnparsedByCliff],
        "the same subject under another id is lost"
    );
}

/// Each commit listed as kept as written passes under its subject: one
/// whose scope lists three crates, which no scope pattern takes.
#[test]
fn the_three_crate_scope_is_kept_as_written() {
    let kept = "ef431c5e10e6834dbd758b9e69d0a9fd1a05dec3";
    assert!(xtask::changelog::KEPT_AS_WRITTEN.contains(&kept));
    let subject = "test(app, present-macos, render): copied text reaches the pasteboard as UTF-8";
    let v = check(
        &log(&[(kept, subject)]),
        PLZ,
        &cliff(&[(kept, false, None)]),
    )
    .expect("parses");
    let (out, ok) = v.report();
    assert!(ok, "{out}");
}

static NEXT: AtomicUsize = AtomicUsize::new(0);

/// A scratch directory holding `repo/`, a git repository with
/// `release-plz.toml` and git-cliff's configuration, and `bin/` for a
/// git-cliff that a test writes; removed on drop.
struct Repo(PathBuf);

impl Drop for Repo {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// A git-cliff that requires the command line `run` gives it and lists
/// every commit of the history as conventional.
const LISTS_ALL: &str = "#!/bin/sh\n\
    [ \"$1 $2 $3\" = '--config scripts/changelog/cliff.toml --context' ] || exit 9\n\
    [ -f \"$2\" ] || exit 8\n\
    printf '[{\"commits\": ['\n\
    git log --format='{\"id\": \"%H\", \"conventional\": true, \"group\": \"Added\"}' | paste -sd, -\n\
    printf ']}]\\n'\n";

impl Repo {
    fn new() -> Repo {
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let d = std::env::temp_dir().join(format!("xtask-changelog-{}-{n}", std::process::id()));
        let r = Repo(d);
        fs::create_dir_all(r.repo().join("scripts/changelog")).expect("the repository");
        fs::create_dir_all(r.0.join("bin")).expect("bin");
        fs::write(r.repo().join("release-plz.toml"), PLZ).expect("release-plz.toml");
        fs::write(r.repo().join("scripts/changelog/cliff.toml"), "").expect("cliff.toml");
        r.git(&r.repo(), &["init", "-q", "-b", "main"]);
        r
    }

    fn repo(&self) -> PathBuf {
        self.0.join("repo")
    }

    fn git(&self, dir: &Path, args: &[&str]) -> String {
        let mut c = Command::new("git");
        c.args(["-c", "user.name=x", "-c", "user.email=x@example.org"])
            .args(["-c", "gc.auto=0", "-c", "maintenance.auto=false"])
            .args(args)
            .current_dir(dir)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1");
        for v in ["GIT_DIR", "GIT_WORK_TREE", "GIT_INDEX_FILE", "GIT_PREFIX"] {
            c.env_remove(v);
        }
        let out = c.output().expect("git");
        assert!(out.status.success(), "git {args:?}: {out:?}");
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    /// An empty commit with `subject`; its full hash.
    fn commit(&self, subject: &str) -> String {
        self.git(
            &self.repo(),
            &["commit", "-q", "--allow-empty", "-m", subject],
        );
        self.git(&self.repo(), &["rev-parse", "HEAD"])
    }

    fn cliff(&self, body: &str) -> PathBuf {
        let p = self.0.join("bin/git-cliff");
        exec::script(&p, body);
        p
    }

    /// `run` against `dir` with `cliff`: the outcome, stdout and stderr.
    fn run_in(&self, dir: &Path, cliff: &Path) -> (Outcome, String, String) {
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let o = run(dir, OsStr::new(cliff), &mut out, &mut err).expect("the inputs parse");
        let text = |b: Vec<u8>| String::from_utf8(b).expect("UTF-8");
        (o, text(out), text(err))
    }
}

#[test]
fn the_whole_history_passes_through_git_cliff() {
    let r = Repo::new();
    r.commit("feat(app): one");
    r.commit("fix: two");
    let cliff = r.cliff(LISTS_ALL);
    let got = r.run_in(&r.repo(), &cliff);
    assert_eq!(
        got,
        (
            Outcome::Passed,
            "check-changelog: 2 commits, 0 failures\n".to_string(),
            String::new()
        )
    );
}

#[test]
fn a_commit_in_the_history_that_is_not_conventional_fails_by_its_short_hash() {
    let r = Repo::new();
    r.commit("feat: one");
    let sha = r.commit("Update things");
    let cliff = r.cliff(LISTS_ALL);
    let (o, out, err) = r.run_in(&r.repo(), &cliff);
    assert_eq!(o, Outcome::Failed);
    assert_eq!(
        out,
        format!(
            "{} not a Conventional Commit: Update things\n\
             check-changelog: 2 commits, 1 failures\n",
            &sha[..12]
        )
    );
    assert_eq!(err, "");
}

#[test]
fn a_merge_commit_is_not_read() {
    let r = Repo::new();
    r.commit("feat: one");
    r.git(&r.repo(), &["checkout", "-q", "-b", "side"]);
    r.commit("fix: two");
    r.git(&r.repo(), &["checkout", "-q", "main"]);
    r.commit("docs: three");
    r.git(
        &r.repo(),
        &["merge", "-q", "--no-ff", "-m", "Merge side", "side"],
    );
    let cliff = r.cliff(LISTS_ALL);
    let (o, out, _) = r.run_in(&r.repo(), &cliff);
    assert_eq!(o, Outcome::Passed, "{out}");
    assert_eq!(out, "check-changelog: 3 commits, 0 failures\n");
}

#[test]
fn a_git_cliff_that_fails_fails_the_gate() {
    let r = Repo::new();
    r.commit("feat: one");
    let cliff = r.cliff("#!/bin/sh\necho '[]'\nexit 1\n");
    let got = r.run_in(&r.repo(), &cliff);
    assert_eq!(
        got,
        (
            Outcome::Failed,
            String::new(),
            "check-changelog: git-cliff failed\n".to_string()
        )
    );
}

#[test]
fn a_git_cliff_that_cannot_start_fails_the_gate() {
    let r = Repo::new();
    r.commit("feat: one");
    let got = r.run_in(&r.repo(), &r.0.join("bin/no-such-git-cliff"));
    assert_eq!(got.0, Outcome::Failed);
    assert_eq!(got.2, "check-changelog: git-cliff failed\n");
}

#[test]
fn a_shallow_clone_fails_before_git_cliff_runs() {
    let r = Repo::new();
    r.commit("feat: one");
    r.commit("fix: two");
    let url = format!("file://{}", r.repo().display());
    let clone = r.0.join("clone");
    let clone_s = clone.to_string_lossy().into_owned();
    r.git(&r.0, &["clone", "-q", "--depth", "1", &url, &clone_s]);
    let cliff = r.cliff("#!/bin/sh\nexit 7\n");
    let got = r.run_in(&clone, &cliff);
    assert_eq!(
        got,
        (
            Outcome::Failed,
            String::new(),
            "check-changelog: a shallow clone; fetch the whole history\n".to_string()
        )
    );
}
