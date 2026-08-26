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
NOTES="docs/research/notes"
fail=0

[ -f "$MANIFEST" ] || {
  echo "  FAIL $MANIFEST is missing"
  exit 1
}

tmp=$(mktemp -d)
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

# ── 3: every source file is accounted for ────────────────────────────────────
cut -f2 "$tmp/entries" | grep -v '^$' | sort -u >"$tmp/known"
for f in "$SRC"/*; do
  b=$(basename "$f")
  [ "$b" = "MANIFEST.toml" ] && continue
  grep -qxF "$b" "$tmp/known" || {
    echo "  FAIL $b is in $SRC/ with no manifest entry"
    fail=1
  }
done

# ── 4: every citation resolves to a readable source ──────────────────────────
cut -f1 "$tmp/entries" | sort -u >"$tmp/keys"
awk -F'\t' '$4 == "gated" { print $1 }' "$tmp/entries" | sort -u >"$tmp/gated"
if [ -d "$NOTES" ]; then
  grep -ohE '\[\[source: [a-z0-9_-]+\]\]' "$NOTES"/*.md 2>/dev/null |
    sed -E 's/\[\[source: (.*)\]\]/\1/' | sort -u >"$tmp/cited"
  while read -r c; do
    [ -n "$c" ] || continue
    grep -qxF "$c" "$tmp/keys" || {
      echo "  FAIL a note cites [[source: $c]], which the manifest does not carry"
      fail=1
    }
    grep -qxF "$c" "$tmp/gated" && {
      echo "  FAIL a note cites [[source: $c]], recorded as gated — never read"
      fail=1
    }
  done <"$tmp/cited"
  echo "  ok   $(wc -l <"$tmp/cited" | tr -d ' ') citations resolve, $(wc -l <"$tmp/keys" | tr -d ' ') sources in manifest"
fi

exit $fail
