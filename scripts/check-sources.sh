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
# **No list of what to scan, because every list here has gone stale.**
#
# This gate has carried two, and both were narrower than the sentence above
# them. `DOCS="docs"` sat under a comment reading "Every doc, not just the
# notes ... a gate that only scanned notes/ would have let those citations rot
# unchecked" -- and left **CLAUDE.md's 39 citations** out, along with
# README.md's and MEASUREMENTS.md's, because those are at the repository root
# rather than under docs/. `CODE` named nine crates and omitted the three ISA
# ones, where `baud-isa-aarch64` cites `arm-neon-bitmask` in the doc comment
# this project's NEON work was reasoned from.
#
# That is the fourth list in this repository to be scoped narrower than its
# own claim, after `docs.rs`'s ROOTS, `audit_all.sh`'s trees and
# `check-features.sh`'s crates -- and unlike those three, widening it finds
# **nothing**: all twenty out-of-scope keys resolve and none is gated. It was
# latent, which is the only reason to remove the mechanism rather than patch
# the entries: a list cannot go stale if there is no list.
#
# So: every markdown file and every Rust file in the working tree, minus the
# build and VCS directories. A new crate, a new document at any depth, and a
# crate the workspace excludes are all covered without anyone remembering to
# add them.
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

# ── 0: the manifest is TOML, and both readers see the same keys ──────────────
# The awk above reads four fields by line prefix and never parses the file, so
# a title with unescaped quotes -- `"VTE #2919, discussion -- "resizing-
# rewrapping is an atomic step""`, which note U's commit added -- left the
# manifest unreadable by any TOML parser while every check below passed. So
# parse it, and require the parser's `[sources.*]` keys to be exactly the
# awk's: a header the awk misreads, or one only a parser sees, is the same
# drift between two readers of one file. The byte order is `LC_ALL=C`'s,
# because Python's `sorted` orders by code point and a UTF-8 locale's collation
# does not: under glibc's it ignores `-`, and the two lists would differ on an
# unchanged manifest.
cut -f1 "$tmp/entries" | LC_ALL=C sort >"$tmp/awk-keys"
command -v python3 >/dev/null || {
  echo "  FAIL python3 is not on PATH, so the manifest cannot be parsed; devenv provides it"
  exit 1
}
if ! python3 - "$MANIFEST" >"$tmp/toml-keys" 2>"$tmp/toml-err" <<'PY'; then
import sys, tomllib
with open(sys.argv[1], "rb") as f:
    print("\n".join(sorted(tomllib.load(f)["sources"])))
PY
  echo "  FAIL $MANIFEST is not valid TOML: $(tail -n 1 "$tmp/toml-err")"
  exit 1
fi
if ! cmp -s "$tmp/awk-keys" "$tmp/toml-keys"; then
  echo "  FAIL the awk and a TOML parser read different [sources.*] keys:"
  diff "$tmp/awk-keys" "$tmp/toml-keys" | head -5 | sed 's/^/       /'
  fail=1
fi

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
{
  # .claude/worktrees holds other branches' checkouts (agent lanes), each with its own
  # manifest: their citations are theirs, and a lane's new key reads as missing here.
  find . \( -path ./target -o -path ./.git -o -path ./.devenv -o -path ./node_modules -o -path ./.claude/worktrees \) -prune \
    -o -type f \( -name '*.md' -o -name '*.rs' \) -print0 |
    xargs -0 grep -ohE '\[\[source: [a-z0-9_-]+\]\]' 2>/dev/null
} | sed -E 's/\[\[source: (.*)\]\]/\1/' | sort -u >"$tmp/cited"

# **A gate that scanned nothing read as a gate that passed.** The count below
# was printed and never asserted, so a broken `find`, a renamed directory or a
# changed citation syntax would have reported "ok 0 citations resolve" and
# exited 0 -- the failure this repository records under `inf x amplification`,
# the stale `scan path: AVX2` flag and `fast_hit`, in the gate written to stop
# design documents resting on unreadable sources. The floor is well under the
# real count and exists to catch a scan that found nothing, not to pin a
# number that grows with every note.
cited_n=$(wc -l <"$tmp/cited" | tr -d ' ')
if [ "$cited_n" -lt 50 ]; then
  echo "  FAIL only $cited_n citations found; this gate scans the whole tree and should see far more"
  exit 1
fi
if true; then
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

# ── 5: every committed source has a licence, and the index says so ──────────
# Committing a file is distributing it, so a source in the manifest is also a
# licence obligation. docs/research/third-party.toml records each file's
# licence and where its notice is; scripts/gen-third-party.py --check fails if
# a retrieved file has no row there, if a text in LICENSES/ drifted from its
# hash or is missing, or if THIRD_PARTY.md is not what the two files generate.
python3 scripts/gen-third-party.py --check || fail=1

exit $fail
