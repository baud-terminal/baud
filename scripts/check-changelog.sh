#!/usr/bin/env bash
# **Every commit reaches the changelog's parsers.** release-plz writes
# CHANGELOG.md from the commits since the last release, grouped by
# `release-plz.toml`'s `commit_parsers`, and a commit none of them matches is
# left out of it with no error. Their last parser skips every type a user
# would not notice, so a subject that is not a Conventional Commit of a
# listed type would be skipped as one of those. So this reads the whole
# history, from the root to HEAD, and fails on any commit that would be
# lost (`cargo xtask changelog`, `xtask/src/changelog.rs`):
#
#   1. its subject is a Conventional Commit of a listed type, `type: text`
#      or `type(scope): text`;
#   2. `release-plz.toml`'s parsers are read from the file and the commit is
#      run through them, compiled with the `regex` crate as release-plz and
#      git-cliff compile them; while the last parser is a catch-all that
#      skips, this leg cannot fail and only proves the file parses;
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

git log --no-merges --format='%H%x00%s' HEAD | cargo xtask changelog "$ctx"
