#!/usr/bin/env bash
# **Every commit reaches the changelog's parsers.** release-plz writes
# CHANGELOG.md from the commits since the last release, grouped by
# `release-plz.toml`'s `commit_parsers`, and a commit none of them matches is
# left out of it with no error. Their last parser skips every type a user
# would not notice, so a subject that is not a Conventional Commit of a
# listed type would be skipped as one of those. So this reads the whole
# history, from the root to HEAD, three ways, and fails on any commit that
# would be lost:
#
#   1. its subject is a Conventional Commit of a listed type, `type: text`
#      or `type(scope): text`;
#   2. it matches one of `release-plz.toml`'s parsers, read from the file;
#   3. git-cliff, under `scripts/changelog/cliff.toml`, reads it as
#      conventional and puts it in no UNPARSED group.
#
# A merge commit is not read: its changes are its parents', each of which is,
# and the merge GitHub makes to test a pull request is named "Merge <sha> into
# <sha>". A shallow clone cannot show the history, so it fails rather than
# pass on the part it has.
set -uo pipefail
cd "$(dirname "$0")/.." || exit 1
if [ "$(git rev-parse --is-shallow-repository)" = true ]; then
  echo "check-changelog: a shallow clone; fetch the whole history" >&2
  exit 1
fi

ctx=$(mktemp)
trap 'rm -f "$ctx"' EXIT
if ! git-cliff --config scripts/changelog/cliff.toml --context >"$ctx" 2>/dev/null; then
  echo "check-changelog: git-cliff failed" >&2
  exit 1
fi

git log --no-merges --format='%H%x00%s' HEAD | python3 -c '
import json, re, sys, tomllib

TYPES = "feat|fix|perf|docs|refactor|test|build|ci|style|revert|chore"
SUBJECT = re.compile(rf"^(?:{TYPES})(?:\([a-z0-9][a-z0-9-]*\))?: \S")
with open("release-plz.toml", "rb") as f:
    parsers = [re.compile(p["message"])
               for p in tomllib.load(f)["changelog"]["commit_parsers"]]
with open(sys.argv[1]) as f:
    cliff = {c["id"]: c for r in json.load(f) for c in r["commits"]}

bad, n = [], 0
for line in sys.stdin.read().splitlines():
    sha, subj = line.split("\0", 1)
    n += 1
    if not SUBJECT.match(subj):
        bad.append((sha, "not a Conventional Commit", subj))
    if not any(p.search(subj) for p in parsers):
        bad.append((sha, "no release-plz parser matches", subj))
    c = cliff.get(sha)
    if c is None:
        bad.append((sha, "missing from git-cliff", subj))
    elif not c["conventional"] or "UNPARSED" in (c["group"] or ""):
        bad.append((sha, "unparsed by git-cliff", subj))
for sha, why, subj in bad:
    print(f"{sha[:12]} {why}: {subj}")
print(f"check-changelog: {n} commits, {len(bad)} failures")
sys.exit(1 if bad or n == 0 else 0)
' "$ctx"
