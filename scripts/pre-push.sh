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
