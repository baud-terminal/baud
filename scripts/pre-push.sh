#!/usr/bin/env bash
# The pre-push hook: the gates the pushed commits need.
#
# prek gives the pushed range as PRE_COMMIT_FROM_REF and PRE_COMMIT_TO_REF.
# With a range, `cargo xtask scoped-test` runs the gates `cargo xtask
# test-scope` names for it; when it names every gate (exit 3), or there is
# no range to scope (a new branch), or BAUD_PUSH_FULL=1, every gate runs:
# `devenv test`, the suite `enterTest` lists.
set -u
cd "$(git rev-parse --show-toplevel)" || exit 1
from=${PRE_COMMIT_FROM_REF:-}
to=${PRE_COMMIT_TO_REF:-}
# The static gates run first, each in seconds and before any build: every
# crate's NOTICE against its files' rows, and the source manifest. A failure
# here refuses the push without starting the suite.
echo "pre-push: the static gates"
devenv shell -- bash -c 'cargo xtask publish && bash scripts/check-sources.sh' || {
  echo "pre-push: a static gate failed; no build or test ran"
  exit 1
}
full() {
  echo "pre-push: every gate ($1)"
  exec devenv test
}
[ "${BAUD_PUSH_FULL:-}" = 1 ] && full "BAUD_PUSH_FULL=1"
[ -n "$from" ] && [ -n "$to" ] || full "no pushed range"
git rev-parse -q --verify "$from^{commit}" >/dev/null || full "$from is not a commit here"
git rev-parse -q --verify "$to^{commit}" >/dev/null || full "$to is not a commit here"
devenv shell -- cargo xtask scoped-test "$from" "$to"
rc=$?
[ "$rc" -eq 3 ] && full "the scope names every gate"
exit "$rc"
