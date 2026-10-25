#!/usr/bin/env bash
# **A path dependency that leaves the repository resolves on the machine it
# was written on and nowhere else.** This repo has paid for that twice:
# `baud-difftest` pinned alacritty at an absolute `/home/<user>/...` and never
# compiled anywhere, while `BUILD.md` (now `docs/contributing/build.md`)
# called it "the only oracle outside this repo"; `bench-others/alacritty-bench` pinned it at `../../../alacritty`, a
# sibling checkout no machine here has, and was excluded from the workspace so
# nothing ever tried. Both were green -- the first by not being built, the
# second by not being a member.
#
# So the rule is the whole rule: a `path` dependency must stay inside the
# repository, and **there is no exception**. There was one -- the owner's
# window crate, taken by absolute path because `feat/drawable-view` existed on
# exactly one machine -- and the gate carried it with the single action that
# would clear it written out. That action was taken: the branch is pushed and
# both manifests name it by git revision, so the exemption has nothing to
# exempt. A gate that keeps an exception after its subject is gone is an
# exemption nobody re-reads, which is the shape this file was written against.
set -uo pipefail
cd "$(dirname "$0")/.." || exit 1
root=$(pwd -P)

fail=0
while IFS= read -r m; do
  while IFS= read -r p; do
    [ -z "$p" ] && continue
    # `[[bin]]`, `[[bench]]` and `[[test]]` spell their source with the same
    # key. A target path names a `.rs` file and a dependency path names a
    # directory, which is the whole difference and is exact here -- the first
    # run of this gate reported `baud-app`'s own `src/main.rs` as an escape.
    case "$p" in
    *.rs) continue ;;
    esac
    case "$p" in
    /*) abs="$p" ;;
    *) abs=$(cd "$(dirname "$m")" 2>/dev/null && cd "$p" 2>/dev/null && pwd -P) || abs="" ;;
    esac
    # Unresolvable relative path: it names a directory this checkout has not
    # got, which is the alacritty-bench defect exactly.
    if [ -z "$abs" ]; then
      echo "FAIL $m: path \"$p\" resolves to nothing on this machine"
      fail=1
      continue
    fi
    case "$abs" in
    "$root"/* | "$root") continue ;;
    esac
    echo "FAIL $m: \"$p\" leaves the repository"
    echo "    That is a build that works on one machine. Vendor it, take it"
    echo "    from a registry, or make it a git dependency -- which is what"
    echo "    the window crate's absolute path became once its branch was"
    echo "    pushed, and why this gate no longer carries an exception."
    fail=1
  done < <(grep -oE 'path *= *"[^"]+"' "$m" | sed 's/.*"\(.*\)"/\1/')
done < <(find . -name Cargo.toml -not -path './target/*' -not -path './.git/*' | sort)

# **A positive control**: a gate that walked no manifest would print nothing
# and exit 0, which reads exactly like a clean tree.
n=$(find . -name Cargo.toml -not -path './target/*' -not -path './.git/*' | wc -l | tr -d ' ')
if [ "$n" -lt 10 ]; then
  echo "FAIL: only $n manifests found -- this gate scanned almost nothing"
  exit 1
fi

if [ "$fail" -ne 0 ]; then
  echo "FAIL: a path dependency leaves the repository"
  exit 1
fi
echo "ok: $n manifests, every path dependency inside the repository"
