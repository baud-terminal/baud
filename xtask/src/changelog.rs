//! **Every commit reaches the changelog's parsers.** release-plz writes
//! CHANGELOG.md from the commits since the last release, grouped by
//! `release-plz.toml`'s `commit_parsers`, and a commit none of them matches
//! is left out of it with no error. A commit fails here if any leg would
//! lose it:
//!
//!   1. its subject is a Conventional Commit of a listed type, `type: text`
//!      or `type(scope): text`;
//!   2. `release-plz.toml`'s parsers, read from the file, match it; while
//!      the last parser is a catch-all that skips, this leg cannot fail and
//!      only proves the file parses. The parsers are compiled with the
//!      `regex` crate and run unanchored, which is how release-plz and
//!      git-cliff run them;
//!   3. git-cliff's context has it, as conventional, in no UNPARSED group.
//!
//! The history comes in as `git log --format='%H%x00%s'`, one commit a line.

use std::collections::HashMap;

use regex::Regex;
use serde_json::Value;

use crate::text;

/// The types the changelog lists or skips by name.
pub const TYPES: &str = "feat|fix|perf|docs|refactor|test|build|ci|style|revert|chore";

/// Why a commit would be lost.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Why {
    /// Leg 1.
    NotConventional,
    /// Leg 2.
    NoParser,
    /// Leg 3: git-cliff did not list it.
    MissingFromCliff,
    /// Leg 3: git-cliff listed it unparsed.
    UnparsedByCliff,
}

impl Why {
    fn says(self) -> &'static str {
        match self {
            Why::NotConventional => "not a Conventional Commit",
            Why::NoParser => "no release-plz parser matches",
            Why::MissingFromCliff => "missing from git-cliff",
            Why::UnparsedByCliff => "unparsed by git-cliff",
        }
    }
}

/// One lost commit.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Bad {
    /// Its full hash.
    pub sha: String,
    /// The leg that loses it.
    pub why: Why,
    /// Its subject.
    pub subject: String,
}

/// What a run found.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Verdict {
    /// The commits read.
    pub commits: Commits,
    /// The failures, commit by commit and leg by leg.
    pub bad: Vec<Bad>,
}

/// A number of commits.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Commits(pub usize);

impl Verdict {
    /// The report and whether it passes: no failures, and at least one
    /// commit read.
    pub fn report(&self) -> (String, bool) {
        let mut out = String::new();
        for b in &self.bad {
            let short: String = b.sha.chars().take(12).collect();
            out += &format!("{short} {}: {}\n", b.why.says(), b.subject);
        }
        out += &format!(
            "check-changelog: {} commits, {} failures\n",
            self.commits.0,
            self.bad.len()
        );
        (out, self.bad.is_empty() && self.commits.0 != 0)
    }
}

/// Holds `log` to the three legs: `release_plz` is the text of
/// `release-plz.toml` and `cliff` git-cliff's `--context`.
pub fn check(log: &str, release_plz: &str, cliff: &str) -> Result<Verdict, String> {
    let subject = Regex::new(&format!(r"^(?:{TYPES})(?:\([a-z0-9][a-z0-9-]*\))?: \S"))
        .map_err(|e| e.to_string())?;
    let table: toml::Table = release_plz
        .parse()
        .map_err(|e| format!("release-plz.toml: {e}"))?;
    let parsers = table
        .get("changelog")
        .and_then(|c| c.get("commit_parsers"))
        .and_then(|p| p.as_array())
        .ok_or("release-plz.toml: no changelog.commit_parsers")?
        .iter()
        .map(|p| {
            let m = p
                .get("message")
                .and_then(|m| m.as_str())
                .ok_or("release-plz.toml: a parser without a message")?;
            Regex::new(m).map_err(|e| format!("release-plz.toml: {e}"))
        })
        .collect::<Result<Vec<_>, String>>()?;
    let context: Value = serde_json::from_str(cliff).map_err(|e| format!("git-cliff: {e}"))?;
    let mut listed: HashMap<&str, &Value> = HashMap::new();
    for release in context
        .as_array()
        .ok_or("git-cliff: not a list of releases")?
    {
        let commits = release
            .get("commits")
            .and_then(|c| c.as_array())
            .ok_or("git-cliff: a release without commits")?;
        for c in commits {
            let id = c
                .get("id")
                .and_then(|i| i.as_str())
                .ok_or("git-cliff: a commit without an id")?;
            listed.insert(id, c);
        }
    }
    let mut bad = Vec::new();
    let mut n = 0;
    for line in text::splitlines(log) {
        let (sha, subj) = line
            .split_once('\0')
            .ok_or_else(|| format!("a log line without a NUL: {line:?}"))?;
        n += 1;
        let mut lose = |why| {
            bad.push(Bad {
                sha: sha.to_string(),
                why,
                subject: subj.to_string(),
            })
        };
        if !subject.is_match(subj) {
            lose(Why::NotConventional);
        }
        if !parsers.iter().any(|p| p.is_match(subj)) {
            lose(Why::NoParser);
        }
        match listed.get(sha) {
            None => lose(Why::MissingFromCliff),
            Some(c) => {
                let conventional = c.get("conventional").is_some_and(truthy);
                let group = c.get("group").and_then(|g| g.as_str()).unwrap_or("");
                if !conventional || group.contains("UNPARSED") {
                    lose(Why::UnparsedByCliff);
                }
            }
        }
    }
    Ok(Verdict {
        commits: Commits(n),
        bad,
    })
}

/// A JSON value read as a condition: `false`, `null`, zero and the empty
/// string, list and map are false.
fn truthy(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().is_some_and(|f| f != 0.0),
        Value::String(s) => !s.is_empty(),
        Value::Array(a) => !a.is_empty(),
        Value::Object(o) => !o.is_empty(),
    }
}
