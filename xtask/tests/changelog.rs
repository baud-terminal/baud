//! Each leg of `cargo xtask changelog` loses the commit it is for, and a
//! history with no commits fails.

use xtask::changelog::{Commits, Why, check};

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
