#!/usr/bin/env bash
# Source-provenance gate, after zebu's scripts/check-sources.sh.
#
# Fails if the research corpus and its manifest have drifted apart:
#
#   1. a manifest entry marked `retrieved` whose file is missing
#   2. a file whose contents no longer hash to the recorded sha256
#   3. a file in docs/research/sources/ with no manifest entry
#   4. a note citing a key the manifest does not carry, or carries as `gated`
#   5. a committed source with no licence row
#   6. a learn.microsoft.com page with no article-text pin: its bytes move with
#      the site's chrome, so its check is the text (scripts/check-page-text.py)
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
# them. `DOCS="docs"` sat under a comment reading "Every doc ... a gate
# that only scanned one directory would have let those citations rot
# unchecked" -- and left **the root guide's 39 citations** out, along with
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
if ! python3 - "$MANIFEST" "$tmp/page-text" >"$tmp/toml-keys" 2>"$tmp/toml-err" <<'PY'; then
import importlib.util, sys, tomllib
with open(sys.argv[1], "rb") as f:
    sources = tomllib.load(f)["sources"]
print("\n".join(sorted(sources)))
# Check 6 reads the parse this one makes; it reports below.
try:
    spec = importlib.util.spec_from_file_location("page_text", "scripts/check-page-text.py")
    page_text = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(page_text)
    problems = page_text.offline_problems(sources)
    pages = sum(map(page_text.in_scope, sources.values()))
except Exception as e:
    problems, pages = [f"scripts/check-page-text.py could not check the manifest: {e!r}"], 0
with open(sys.argv[2], "w") as f:
    f.writelines(f"FAIL {p}\n" for p in problems)
    if not problems:
        f.write(f"ok   {pages} learn.microsoft.com pages carry their article text's sha256\n")
PY
  echo "  FAIL $MANIFEST is not valid TOML: $(tail -n 1 "$tmp/toml-err")"
  exit 1
fi
if ! cmp -s "$tmp/awk-keys" "$tmp/toml-keys"; then
  echo "  FAIL the awk and a TOML parser read different [sources.*] keys:"
  diff "$tmp/awk-keys" "$tmp/toml-keys" | head -5 | sed 's/^/       /'
  fail=1
fi

# ── 6: a learn.microsoft.com page is pinned by its article text ──────────────
# Its bytes carry the site's asset version string and move with the markup
# around the article: on 2026-11-01 none of the 57 pages hashed as pinned,
# one pinned the day before among them. So each such entry also records the
# revision the page renders (where it has one), the sha256 of its article text
# and the text's length, and scripts/check-page-text.py fetches the pages and
# compares the text. That needs the network, so it runs on demand; this gate
# holds what it can offline: every page entry carries the text's sha256 and
# length, well formed, a revision is well formed where one is recorded, and
# the extractor's self-test, on synthetic pages, passes.
# Closed: only the line that says every page passed lets this one pass.
sed 's/^/  /' "$tmp/page-text"
grep -q '^ok' "$tmp/page-text" || fail=1

# ── The lists 1 to 4 read ────────────────────────────────────────────────────
# **The lists are made by the commands that always made them, and the checks
# on them run in one process, below.** Run in the shell, the checks started a
# `shasum` for each retrieved file and a `cut` or a `grep` for each entry, file
# and citation, which was nearly all of the gate's time. The lists' order,
# `find`'s and the locale's `sort`'s, is the one the checks always read.
cut -f2 "$tmp/entries" | grep -v '^$' | sort -u >"$tmp/known"
# Recursive: sources are grouped into subdirectories (terminfo/ and, as the
# corpus grows, one per protocol family), so a top-level-only walk would see a
# directory it could not account for and, worse, would never check the files
# inside it. Paths are compared relative to $SRC, matching the manifest's
# `file` values.
find "$SRC" -type f ! -name 'MANIFEST.toml' >"$tmp/found"
cut -f1 "$tmp/entries" | sort -u >"$tmp/keys"
awk -F'\t' '$4 == "gated" { print $1 }' "$tmp/entries" | sort -u >"$tmp/gated"
{
  # .claude/worktrees holds other branches' checkouts, each with its own
  # manifest: their citations are theirs, and a lane's new key reads as missing here.
  find . \( -path ./target -o -path ./.git -o -path ./.devenv -o -path ./node_modules -o -path ./.claude/worktrees \) -prune \
    -o -type f \( -name '*.md' -o -name '*.rs' \) -print0 |
    xargs -0 grep -ohE '\[\[source: [a-z0-9_-]+\]\]' 2>/dev/null
} | sed -E 's/\[\[source: (.*)\]\]/\1/' | sort -u >"$tmp/cited"

# `${#sha}` counts characters in a UTF-8 locale and bytes in C; this asks the
# shell running the gate which, so 2b counts as the loop it replaced did.
probe=$'\xc3\xa9'
chars=bytes
[ "${#probe}" -eq 1 ] && chars=utf-8

python3 -I - "$SRC" "$tmp" "$chars" <<'PY'
import hashlib, os, sys

src, tmp, chars = (os.fsencode(a) for a in sys.argv[1:4])
out = sys.stdout.buffer
fail = 0


def lines(name):
    """A file's newline-ended lines, as `read` takes them."""
    with open(os.path.join(tmp, name), "rb") as f:
        return f.read().split(b"\n")[:-1]


def say(*parts):
    out.write(b"".join(parts) + b"\n")


entries = lines(b"entries")

# ── 1 & 2: retrieved entries must exist and still hash ───────────────────────
# As `IFS=$'\t' read -r key file sha status`: tab is an IFS whitespace
# character, so a run of tabs is one separator and an empty field moves the
# rest left -- a retrieved entry with no file or no sha256 reads a status that
# is not `retrieved`, and only 2b sees it.
for line in entries:
    rest, fields = line.strip(b"\t"), []
    for _ in range(3):
        head, _, rest = rest.partition(b"\t")
        fields.append(head)
        rest = rest.lstrip(b"\t")
    key, file, sha, status = *fields, rest
    if status != b"retrieved":
        continue
    path = src + b"/" + file
    if not os.path.isfile(path):
        say(b"  FAIL ", key, b": status=retrieved but ", path, b" is missing")
        fail = 1
        continue
    digest = hashlib.sha256()
    try:
        with open(path, "rb") as f:
            while block := f.read(1 << 20):
                digest.update(block)
        got = digest.hexdigest().encode()
    except OSError as e:
        print(f"check-sources: {os.fsdecode(path)}: {e.strerror}", file=sys.stderr)
        got = b""
    if got != sha:
        say(b"  FAIL ", key, b": ", file, b" hashes ", got, b", manifest says ", sha)
        fail = 1

# ── 2b: every sha256 is well-formed ─────────────────────────────────────────
# An `external` entry names a file this repo does not hold, so (2) cannot
# re-hash it and a malformed digest there is invisible. This check caught a
# literal placeholder -- `sha256 = "9c07f2a0a0a0a0a0"` -- in the commit that
# added it, on an entry a note already cited. It is the cheap half of a
# verification that cannot be done in full.
# As `cut -f`, not `read`: `cut` keeps empty fields. The first version of this
# check used `read`, read `status` as the digest and reported
# apple-cpu-guide's perfectly good 64 hex chars as "8 chars, want 64" -- a
# probe firing on the wrong data, which is the failure mode this repo already
# records twice. A line with no tab is every field of itself, as with `cut`.
for line in entries:
    cols = line.split(b"\t")

    def cut(n):
        return line if len(cols) == 1 else (cols[n - 1] if n <= len(cols) else b"")

    key, sha, status = cut(1), cut(3), cut(4)
    if not key:
        continue
    # `gated` means never obtained, so there is nothing to have hashed.
    if status == b"gated":
        continue
    # The shell's `[0-9a-f]*`: the first character only, as the loop had it.
    if sha[:1] and sha[:1] in b"0123456789abcdef":
        n = len(sha.decode("utf-8", "surrogateescape")) if chars == b"utf-8" else len(sha)
        if n != 64:
            say(b"  FAIL ", key, b": sha256 is ", str(n).encode(), b" chars, want 64")
            fail = 1
    else:
        say(b"  FAIL ", key, b": sha256 is not lowercase hex: '", sha, b"'")
        fail = 1

# ── 3: every source file is accounted for ────────────────────────────────────
# `find`'s list, each line trimmed of spaces and tabs as `read -r f` trims it.
known = set(lines(b"known"))
for f in lines(b"found"):
    f = f.strip(b" \t")
    b = f[len(src) + 1 :] if f.startswith(src + b"/") else f
    if b not in known:
        say(b"  FAIL ", b, b" is in ", src, b"/ with no manifest entry")
        fail = 1

# ── 4: every citation resolves to a readable source ──────────────────────────
# **A gate that scanned nothing read as a gate that passed.** The count below
# was printed and never asserted, so a broken `find`, a renamed directory or a
# changed citation syntax would have reported "ok 0 citations resolve" and
# exited 0 -- the failure this repository records under `inf x amplification`,
# the stale `scan path: AVX2` flag and `fast_hit`, in the gate written to stop
# design documents resting on unreadable sources. The floor is well under the
# real count and exists to catch a scan that found nothing, not to pin a
# number that grows with every note.
cited = lines(b"cited")
keys = lines(b"keys")
if len(cited) < 50:
    say(b"  FAIL only ", str(len(cited)).encode(), b" citations found; this gate scans the whole tree and should see far more")
    out.flush()
    sys.exit(3)
key_set, gated = set(keys), set(lines(b"gated"))
for c in cited:
    c = c.strip(b" \t")
    if not c:
        continue
    if c not in key_set:
        say(b"  FAIL a doc or doc comment cites [[source: ", c, b"]], which the manifest does not carry")
        fail = 1
    if c in gated:
        say(b"  FAIL a doc or doc comment cites [[source: ", c, "]], recorded as gated — never read".encode())
        fail = 1
say(b"  ok   ", str(len(cited)).encode(), b" citations resolve, ", str(len(keys)).encode(), b" sources in manifest")
out.flush()
sys.exit(fail)
PY
case $? in
0) ;;
3) exit 1 ;;
*) fail=1 ;;
esac

# ── 5: every committed source has a licence, and the index says so ──────────
# Committing a file is distributing it, so a source in the manifest is also a
# licence obligation. docs/research/third-party.toml records each file's
# licence and where its notice is; scripts/gen-third-party.py --check fails if
# a retrieved file has no row there, if a text in LICENSES/ drifted from its
# hash or is missing, or if THIRD_PARTY.md is not what the two files generate.
python3 scripts/gen-third-party.py --check || fail=1

exit $fail
