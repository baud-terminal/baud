#!/usr/bin/env bash
# **`devenv shell` starts in a tree nobody has started it in.** A folder that
# already has a `.devenv` says nothing about a reader's first run, and neither
# does `devenv print-dev-env`, which passed where `devenv shell` failed. So this
# makes two trees of one commit and starts the shell in each, as a reader does:
#
#   clone    `git clone --no-local`, what the install guide tells a reader
#   archive  `git archive`, a tree with no `.git`, as a ZIP download is
#
# The clone alone cannot see what this script was written for: with `.devenv/`
# among treefmt's excludes, the clone passed and formatted nothing, while the
# archive of the same commit changed files under `.devenv` and failed.
# `baud-app/tests/install.rs` holds those excludes on every test run; this is
# the run behind that, networked and minutes long, for landing to call and not
# for `enterTest`, which runs inside a dev shell.
#
# Each tree must start the shell, reach the toolchain, and format nothing:
# `devenv shell` runs treefmt on the way in, so a changed file is the shell
# rewriting what it was about to use, or the commit carrying an unformatted
# file. The run has its own HOME and XDG folders, so it reads no user's
# configuration and writes nothing to theirs, and because every tree is new it
# rewrites no git hooks but its own clone's: never run `devenv shell` itself in
# a worktree, whose hooks are its parent's.
#
#   scripts/check-fresh-shell.sh [REV]    REV defaults to HEAD
#   DEVENV=/path/to/devenv                a CLI other than the one on PATH
#   FRESH_SHELL_TIMEOUT=SECS              each `devenv shell`, default 1800
set -uo pipefail
cd "$(dirname "$0")/.." || exit 1
repo=$PWD
rev=$(git rev-parse --verify "${1:-HEAD}^{commit}") || exit 1
cli=${DEVENV:-$(command -v devenv)} || {
  echo "check-fresh-shell: no devenv on PATH" >&2
  exit 1
}
limit=${FRESH_SHELL_TIMEOUT:-1800}

# A dev shell's own Xcode shim settings would turn /usr/bin/git into an error.
unset DEVELOPER_DIR SDKROOT

work=$(mktemp -d "${TMPDIR:-/tmp}/baud-fresh-shell.XXXXXX") || exit 1
trap 'rm -rf "$work"' EXIT
mkdir -p "$work/home/.config" "$work/home/.cache" "$work/home/.local/share" \
  "$work/home/.local/state"

# Nothing of the caller's environment but what reaches the network and the
# daemon, so a run from inside a dev shell tests what a new terminal gets.
in_scratch() {
  "$repo/scripts/bounded" --timeout "$limit" -- env -i \
    PATH="$(dirname "$cli"):/nix/var/nix/profiles/default/bin:/usr/bin:/bin:/usr/sbin:/sbin" \
    USER="${USER:-$(id -un)}" \
    TERM=dumb \
    HOME="$work/home" \
    XDG_CONFIG_HOME="$work/home/.config" \
    XDG_CACHE_HOME="$work/home/.cache" \
    XDG_DATA_HOME="$work/home/.local/share" \
    XDG_STATE_HOME="$work/home/.local/state" \
    ${NIX_SSL_CERT_FILE:+NIX_SSL_CERT_FILE="$NIX_SSL_CERT_FILE"} \
    "$@"
}

# One tree: the shell starts, it has the compiler, and it formatted nothing.
check() {
  local name=$1 tree=$2 log=$work/$1.log status=0
  (cd "$tree" && in_scratch "$cli" shell -- true) </dev/null >"$log" 2>&1 || status=$?
  if [ "$status" -ne 0 ]; then
    echo "check-fresh-shell: $name: devenv shell -- true exited $status" >&2
    tail -n 20 "$log" >&2
    return 1
  fi
  # The positive control on the line below: a treefmt that did not run prints
  # no count, and "no count" must not read as "nothing changed".
  # Any line with a non-zero count fails, so a later clean pass cannot hide an
  # earlier rewrite.
  if ! grep -Eq 'formatted [0-9]+ files \(0 changed\)' "$log" ||
    grep -E 'formatted [0-9]+ files \([0-9]+ changed\)' "$log" | grep -Evq '\(0 changed\)'; then
    echo "check-fresh-shell: $name: treefmt changed files or did not run:" >&2
    grep -E 'formatted [0-9]+ files' "$log" >&2
    return 1
  fi
  local version
  version=$(cd "$tree" && in_scratch "$cli" shell -- rustc --version </dev/null 2>/dev/null | grep -E '^rustc ')
  if [ -z "$version" ]; then
    echo "check-fresh-shell: $name: the shell has no rustc" >&2
    return 1
  fi
  echo "check-fresh-shell: $name: ok, $version"
}

echo "check-fresh-shell: $("$cli" version) at $rev"
failed=0

git clone -q --no-local --no-checkout "$repo" "$work/clone" &&
  git -C "$work/clone" checkout -q --detach "$rev" || exit 1
check clone "$work/clone" || failed=1

mkdir "$work/archive" || exit 1
git archive "$rev" | tar -x -C "$work/archive" || exit 1
[ ! -e "$work/archive/.git" ] || exit 1
check archive "$work/archive" || failed=1

exit "$failed"
