#!/usr/bin/env bash
# check-moved-paths.sh [<root>]: no tracked text names a path at a place it moved from.
#
# tools/moved-paths.tsv lists each top-level entry that moved, old TAB new. A token "<old>/<rest>"
# (or the entry itself, for a moved file) fails when "<new>/<rest>" exists and the token resolves
# nowhere as written: not from the root, the file's directory or its crate's directory. Files that
# keep the paths they were written with are skipped: MEASUREMENTS.md, the vendored sources, the
# licences, the lockfiles, the recorded terminal data under tools/tests, and the table itself.
# Prints "<file>:<line>: <token> -> <new>" for each, then "check-moved-paths: N stale"; exit 1 on any.
set -u
root=${1:-$(cd "$(dirname "$0")/../.." && pwd)}
cd "$root" || exit 2
table=tools/moved-paths.tsv
[ -s "$table" ] || {
  echo "check-moved-paths: $table is missing"
  exit 2
}
olds=$(cut -f1 "$table" | sed 's/\./\\./g' | paste -sd'|')
declare -A new
while IFS=$'\t' read -r o n; do new[$o]=$n; done <"$table"
# tools/moved-paths-ok.tsv: "<file>\t<token>\t<why>", a token that names a moved entry from a base this check
# cannot see (a directory the code joins it to), so it is right as written.
declare -A ok
[ -f tools/moved-paths-ok.tsv ] && while IFS=$'\t' read -r f t _; do [ -n "$f" ] && ok["$f	$t"]=1; done < <(grep -v '^#' tools/moved-paths-ok.tsv)

# The nearest directory above a file whose Cargo.toml is a package (a workspace-only manifest is no crate).
crate_of() {
  local d
  d=$(dirname "$1")
  while [ "$d" != . ]; do
    grep -qs '^\[package\]' "$d/Cargo.toml" && {
      echo "$d"
      return
    }
    d=$(dirname "$d")
  done
}

stale=0
while IFS= read -r -d '' f; do
  [ -f "$f" ] && [ ! -L "$f" ] || continue
  grep -Iq . "$f" 2>/dev/null || continue
  dir=$(dirname "$f")
  crate=$(crate_of "$f")
  # p: a token at the start of a path; v: one after a root held in a variable ($root/, ${root}/, $(...)/, {}/),
  # where a bare directory name is a path too.
  while IFS=: read -r kind ln tok; do
    tok=${tok%%[.,:;]}
    tok=${tok%/}
    head=${tok%%/*}
    rest=${tok#"$head"}
    [ -n "${new[$head]:-}" ] || continue
    [ -n "$rest" ] || [ "$kind" = v ] || [ -f "${new[$head]}" ] || continue # a bare name: a path after a variable, or a moved file
    [ -e "${new[$head]}$rest" ] || continue
    [ -e "$tok" ] || [ -e "$dir/$tok" ] || { [ -n "$crate" ] && [ -e "$crate/$tok" ]; } && continue
    [ -n "${ok["$f	$tok"]:-}" ] && continue
    printf '%s:%s: %s -> %s\n' "$f" "$ln" "$tok" "${new[$head]}$rest"
    stale=$((stale + 1))
  done < <(
    grep -noE "(^|[^A-Za-z0-9_./-])(\./)?($olds)(/[A-Za-z0-9_./-]*[A-Za-z0-9_-])?($|[^A-Za-z0-9_-])" "$f" |
      sed -E "s/^([0-9]+):[^A-Za-z0-9_./-]?(\.\/)?/p:\1:/; s/[^A-Za-z0-9_./-]$//"
    grep -noE "(\\\$\{?[A-Za-z_][A-Za-z0-9_]*\}?|\\\$\([^()]*\)|\{\}|\{[A-Za-z_]+\})/($olds)(/[A-Za-z0-9_./-]*[A-Za-z0-9_-])?($|[^A-Za-z0-9_-])" "$f" |
      sed -E "s/^([0-9]+):(\\\$\{?[A-Za-z_][A-Za-z0-9_]*\}?|\\\$\([^()]*\)|\{\}|\{[A-Za-z_]+\})\//v:\1:/; s/[^A-Za-z0-9_./-]$//"
  )
done < <(git ls-files -z | grep -zvE '^(MEASUREMENTS\.md|Cargo\.lock|LICENSES/|docs/research/sources/|tools/moved-paths\.tsv)|/Cargo\.lock$' |
  grep -zvE '^tools/tests/.*\.(raw|txt|steps|sizes|ans|six|zshrc|Xresources|nu|fish)$|^tools/tests/[^.]*$')
echo "check-moved-paths: $stale stale"
[ "$stale" -eq 0 ]
