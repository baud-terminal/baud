#!/usr/bin/env bash
# **A path dependency that leaves the repository resolves on the machine it
# was written on and nowhere else.** This repo has paid for that twice:
# `baud-difftest` pinned alacritty at an absolute `/home/user/...` and never
# compiled anywhere, while `BUILD.md` called it "the only oracle outside this
# repo"; `bench-others/alacritty-bench` pinned it at `../../../alacritty`, a
# sibling checkout no machine here has, and was excluded from the workspace so
# nothing ever tried. Both were green -- the first by not being built, the
# second by not being a member.
#
# So the rule is the whole rule: a `path` dependency must stay inside the
# repository. The one exception is named below with what clears it, and it
# fails on any machine where it does not resolve -- which is the truth about
# what can build there, rather than a green run that says nothing.
set -uo pipefail
cd "$(dirname "$0")/.." || exit 1
root=$(pwd -P)

# **The known one, and the single action that removes it.** `baud-app` and
# `baud-present-macos` take the window from the owner's crate, whose branch
# `feat/drawable-view` is not on its remote: `git ls-remote` shows `master`
# and the release-plz branches and nothing else. So the absolute path is not
# laziness, it is the only spelling available while the branch is local.
KNOWN="~/src/test/glassnote"
KNOWN_FIX='commit the five files open in that crate and push feat/drawable-view,
    then this becomes:
      macos-liquid-glass = { git = "https://github.com/i-am-logger/macos-liquid-glass", branch = "feat/drawable-view", features = ["drawable"] }'

fail=0
found_known=0
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
    if [ "$abs" = "$KNOWN" ]; then
      found_known=1
      if [ -d "$abs" ]; then
        echo "  known  $m -> $abs"
      else
        echo "FAIL $m: the known out-of-tree dependency does not resolve here."
        echo "    $KNOWN_FIX"
        fail=1
      fi
      continue
    fi
    echo "FAIL $m: \"$p\" leaves the repository and is not the known exception"
    echo "    A new one means a build that works on one machine. Vendor it,"
    echo "    take it from a registry, or make it a git dependency."
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
if [ "$found_known" -eq 1 ]; then
  echo "ok: $n manifests; the one out-of-tree dependency resolves here."
  echo "    It does not resolve anywhere else, and to clear it:"
  echo "    $KNOWN_FIX"
else
  echo "ok: $n manifests, every path dependency inside the repository"
fi
