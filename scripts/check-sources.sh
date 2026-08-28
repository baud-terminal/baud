#!/usr/bin/env bash
# Source-provenance gate, after zebu's scripts/check-sources.sh.
#
# Fails if the research corpus and its manifest have drifted apart:
#
#   1. a manifest entry marked `retrieved` whose file is missing
#   2. a file whose contents no longer hash to the recorded sha256
#   3. a file in docs/research/sources/ with no manifest entry
#   4. a note citing a key the manifest does not carry, or carries as `gated`
#
# (4) is the one that matters: it stops a design document resting on a citation
# nobody could check. Notes cite with `[[source: <key>]]`.

set -uo pipefail
cd "$(dirname "$0")/.." || exit 1

SRC="docs/research/sources"
MANIFEST="$SRC/MANIFEST.toml"
# Every doc, not just the notes: docs/vt-coverage.md cites sources too, and a
# gate that only scanned notes/ would have let those citations rot unchecked.
DOCS="docs"
# **And the code.** Doc comments in baud-core/ and baud-domain/ cite sources
# exactly as the notes do, and for most of this project's life nothing checked
# them -- `vt510-decscpp` was cited from a doc comment and had no manifest entry
# at all. A citation an engineer reads while changing the code is the one most
# likely to be acted on, so it is the last one that should go unverified.
CODE="baud-core baud-domain"
fail=0

[ -f "$MANIFEST" ] || {
  echo "  FAIL $MANIFEST is missing"
  exit 1
}

tmp=$(mktemp -d)
: >"$tmp/unaccounted"
trap 'rm -rf "$tmp"' EXIT

# key<TAB>file<TAB>sha256<TAB>status, one per [sources.*] block.
awk '
  /^\[sources\./ {
    if (key != "") print key "\t" file "\t" sha "\t" status
    key = $0; sub(/^\[sources\./, "", key); sub(/\]$/, "", key)
    file = ""; sha = ""; status = ""; next
  }
  /^file *=/   { v = $0; sub(/^file *= *"/, "", v);   sub(/"$/, "", v); file = v;   next }
  /^sha256 *=/ { v = $0; sub(/^sha256 *= *"/, "", v); sub(/"$/, "", v); sha = v;    next }
  /^status *=/ { v = $0; sub(/^status *= *"/, "", v); sub(/"$/, "", v); status = v; next }
  END { if (key != "") print key "\t" file "\t" sha "\t" status }
' "$MANIFEST" >"$tmp/entries"

# ── 1 & 2: retrieved entries must exist and still hash ───────────────────────
while IFS=$'\t' read -r key file sha status; do
  [ "$status" = "retrieved" ] || continue
  if [ ! -f "$SRC/$file" ]; then
    echo "  FAIL $key: status=retrieved but $SRC/$file is missing"
    fail=1
    continue
  fi
  got=$(shasum -a 256 "$SRC/$file" | cut -d' ' -f1)
  if [ "$got" != "$sha" ]; then
    echo "  FAIL $key: $file hashes $got, manifest says $sha"
    fail=1
  fi
done <"$tmp/entries"

# ── 2b: every sha256 is well-formed ─────────────────────────────────────────
# An `external` entry names a file this repo does not hold, so (2) cannot
# re-hash it and a malformed digest there is invisible. This check caught a
# literal placeholder -- `sha256 = "9c07f2a0a0a0a0a0"` -- in the commit that
# added it, on an entry a note already cited. It is the cheap half of a
# verification that cannot be done in full.
# `cut`, not `read`: tab is an IFS *whitespace* character, so `read` collapses
# a run of them and every entry with an empty `file` shifts a column left. The
# first version of this check read `status` as the digest and reported
# apple-cpu-guide's perfectly good 64 hex chars as "8 chars, want 64" -- a
# probe firing on the wrong data, which is the failure mode this repo already
# records twice. `cut -f` keeps empty fields.
while IFS= read -r line; do
  key=$(printf '%s' "$line" | cut -f1)
  sha=$(printf '%s' "$line" | cut -f3)
  status=$(printf '%s' "$line" | cut -f4)
  [ -n "$key" ] || continue
  # `gated` means never obtained, so there is nothing to have hashed.
  [ "$status" = "gated" ] && continue
  case "$sha" in
  [0-9a-f]*) [ "${#sha}" -eq 64 ] || {
    echo "  FAIL $key: sha256 is ${#sha} chars, want 64"
    fail=1
  } ;;
  *)
    echo "  FAIL $key: sha256 is not lowercase hex: '$sha'"
    fail=1
    ;;
  esac
done <"$tmp/entries"

# ── 3: every source file is accounted for ────────────────────────────────────
cut -f2 "$tmp/entries" | grep -v '^$' | sort -u >"$tmp/known"
# Recursive: sources are grouped into subdirectories (terminfo/ and, as the
# corpus grows, one per protocol family), so a top-level-only walk would see a
# directory it could not account for and, worse, would never check the files
# inside it. Paths are compared relative to $SRC, matching the manifest's
# `file` values.
find "$SRC" -type f ! -name 'MANIFEST.toml' | while read -r f; do
  b=${f#"$SRC"/}
  grep -qxF "$b" "$tmp/known" || {
    echo "  FAIL $b is in $SRC/ with no manifest entry"
    echo x >>"$tmp/unaccounted"
  }
done
[ -s "$tmp/unaccounted" ] && fail=1

# ── 4: every citation resolves to a readable source ──────────────────────────
cut -f1 "$tmp/entries" | sort -u >"$tmp/keys"
awk -F'\t' '$4 == "gated" { print $1 }' "$tmp/entries" | sort -u >"$tmp/gated"
if [ -d "$DOCS" ]; then
  {
    grep -rohE '\[\[source: [a-z0-9_-]+\]\]' --include='*.md' "$DOCS" 2>/dev/null
    for d in $CODE; do
      [ -d "$d" ] || continue
      grep -rohE '\[\[source: [a-z0-9_-]+\]\]' --include='*.rs' "$d" 2>/dev/null
    done
  } | sed -E 's/\[\[source: (.*)\]\]/\1/' | sort -u >"$tmp/cited"
  while read -r c; do
    [ -n "$c" ] || continue
    grep -qxF "$c" "$tmp/keys" || {
      echo "  FAIL a doc or doc comment cites [[source: $c]], which the manifest does not carry"
      fail=1
    }
    grep -qxF "$c" "$tmp/gated" && {
      echo "  FAIL a doc or doc comment cites [[source: $c]], recorded as gated — never read"
      fail=1
    }
  done <"$tmp/cited"
  echo "  ok   $(wc -l <"$tmp/cited" | tr -d ' ') citations resolve, $(wc -l <"$tmp/keys" | tr -d ' ') sources in manifest"
fi

exit $fail
