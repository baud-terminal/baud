#!/usr/bin/env python3
"""Generate baud-domain/src/search_data.rs: what a search compares.

A search matches a query against the text by **canonical caseless match**,
`[[source: unicode17-ch3]]` D145: NFD(toCasefold(NFD(X))). R4's toCasefold
maps each character C to Case_Folding(C), "the mappings with the status field
value C and F" in CaseFolding.txt (`[[source: unicode-casefolding]]`); NFD is
the full canonical decomposition of UnicodeData.txt's field 5, its untagged
mappings applied until none applies, then the canonical ordering of field 3's
combining classes (`[[source: unicode-data]]`). Hangul syllables decompose by
arithmetic (section 3.12) and are not listed.

Per code point the table holds three things, all indexed and none searched:

* `d`: its full canonical decomposition, the first NFD;
* `g`: the canonical decomposition of each character of its full case
  folding, in order -- toCasefold and the second NFD's decomposition in one
  lookup, applied to what the first NFD left;
* `ccc`: its canonical combining class, for the two reorderings.

Each is a two-stage trie on `codepoint >> SHIFT` to a block of property
indices; a property names its two sequences in one pool, where index 0 is
"the code point itself". The generator checks the trie against the direct
computation at every one of the 1,114,112 code points before it writes.

Positive controls, asserted: U+00DF folds to "ss", U+212A KELVIN SIGN to "k",
U+1E9E to "ss"; U+00C5 decomposes to A + U+030A and U+212B ANGSTROM SIGN to
the same; U+0345 COMBINING GREEK YPOGEGRAMMENI folds to U+03B9; the Turkic
status T rows are left out.

Usage: gen-search-tables.py [--check]
"""
import pathlib
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
SRC = ROOT / "docs/research/sources"
UD = SRC / "unicode-unicodedata.txt"
CF = SRC / "unicode-casefolding.txt"
OUT = ROOT / "baud-domain/src/search_data.rs"
SHIFT = 7
BLOCK = 1 << SHIFT
N = 0x110000
S_BASE, S_COUNT = 0xAC00, 11172
L_BASE, V_BASE, T_BASE = 0x1100, 0x1161, 0x11A7
V_COUNT, T_COUNT = 21, 28

ccc = [0] * N
decomp = {}
for line in UD.read_text().splitlines():
    f = line.split(";")
    cp = int(f[0], 16)
    ccc[cp] = int(f[3])
    if f[5] and not f[5].startswith("<"):
        decomp[cp] = [int(x, 16) for x in f[5].split()]
for cp in range(S_BASE, S_BASE + S_COUNT):
    if cp in decomp:
        sys.exit(f"U+{cp:04X} is a Hangul syllable with a listed decomposition")

fold = {}
for line in CF.read_text().splitlines():
    line = line.split("#", 1)[0].strip()
    if not line:
        continue
    code, status, mapping = [x.strip() for x in line.split(";")[:3]]
    if status in ("C", "F"):
        fold[int(code, 16)] = [int(x, 16) for x in mapping.split()]


def hangul(cp):
    s = cp - S_BASE
    out = [L_BASE + s // (V_COUNT * T_COUNT), V_BASE + (s % (V_COUNT * T_COUNT)) // T_COUNT]
    if s % T_COUNT:
        out.append(T_BASE + s % T_COUNT)
    return out


def full(cp):
    """The full canonical decomposition, Hangul included."""
    if S_BASE <= cp < S_BASE + S_COUNT:
        return hangul(cp)
    if cp not in decomp:
        return [cp]
    return [y for x in decomp[cp] for y in full(x)]


def listed(cp):
    """The table's `d`: Hangul is the code's, by arithmetic."""
    if S_BASE <= cp < S_BASE + S_COUNT:
        return [cp]
    return full(cp)


def folded(cp):
    """The table's `g`: read only on what the first NFD left, which holds no
    Hangul syllable, and no folding produces one."""
    return [y for x in fold.get(cp, [cp]) for y in listed(x)]


for cp, out in fold.items():
    if any(S_BASE <= x < S_BASE + S_COUNT for x in out):
        sys.exit(f"U+{cp:04X} folds to a Hangul syllable")


for cp, want in [(0xDF, [0x73, 0x73]), (0x212A, [0x6B]), (0x1E9E, [0x73, 0x73]), (0x345, [0x3B9])]:
    if folded(cp) != want:
        sys.exit(f"control: U+{cp:04X} folds to {folded(cp)}, want {want}")
for cp in (0xC5, 0x212B):
    if full(cp) != [0x41, 0x30A]:
        sys.exit(f"control: U+{cp:04X} decomposes to {full(cp)}")
if fold.get(0x49) != [0x69]:
    sys.exit("control: U+0049 must fold to U+0069, not Turkic U+0131")

pool = [0]
seqs = {}


def seq(cps, cp):
    if cps == [cp]:
        return 0
    key = tuple(cps)
    if key not in seqs:
        seqs[key] = len(pool)
        pool.append(len(cps))
        pool.extend(cps)
    return seqs[key]


props = [(0, 0, 0)]
prop_ix = {(0, 0, 0): 0}
entry = [0] * N
d_max = g_max = 1
for cp in range(N):
    d, g = listed(cp), folded(cp)
    d_max, g_max = max(d_max, len(d)), max(g_max, len(g))
    p = (seq(d, cp), seq(g, cp), ccc[cp])
    if p not in prop_ix:
        prop_ix[p] = len(props)
        props.append(p)
    entry[cp] = prop_ix[p]
if len(props) > 0xFFFF or len(pool) > 0xFFFF:
    sys.exit(f"{len(props)} properties or {len(pool)} pool words do not fit a u16")

blocks = []
block_ix = {}
index = []
for b in range(N // BLOCK):
    block = tuple(entry[b * BLOCK:(b + 1) * BLOCK])
    if block not in block_ix:
        block_ix[block] = len(blocks)
        blocks.append(block)
    index.append(block_ix[block])
if len(blocks) > 0xFFFF:
    sys.exit("more blocks than a u16 names")


def at(cp, which):
    p = props[blocks[index[cp >> SHIFT]][cp & (BLOCK - 1)]]
    i = p[which]
    return [cp] if i == 0 else pool[i + 1:i + 1 + pool[i]]


for cp in range(N):
    if at(cp, 0) != listed(cp) or at(cp, 1) != folded(cp) or props[entry[cp]][2] != ccc[cp]:
        sys.exit(f"trie disagrees at U+{cp:04X}")


def rows(values, per, fmt):
    return ["    " + " ".join(fmt(v) + "," for v in values[i:i + per]) for i in range(0, len(values), per)]


flat = [v for b in blocks for v in b]
index_ty = "u8" if len(blocks) <= 256 else "u16"
lines = [
    "//! What a search compares, generated from the sources pinned in",
    "//! `docs/research/sources/MANIFEST.toml`: `[[source: unicode-data]]`'s",
    "//! canonical decompositions and combining classes and",
    "//! `[[source: unicode-casefolding]]`'s full case folding (status C and F),",
    "//! for the canonical caseless match of `[[source: unicode17-ch3]]` D145.",
    "//!",
    "//! **Do not edit by hand.** Regenerate with `scripts/gen-search-tables.py`;",
    "//! `search_tables_match_the_pinned_data` fails if this file and the pinned",
    "//! data disagree.",
    "",
    "/// The trie's block size, as a shift: `codepoint >> SEARCH_SHIFT` names a",
    "/// block in [`SEARCH_INDEX`].",
    f"pub const SEARCH_SHIFT: u32 = {SHIFT};",
    "",
    "/// The longest full canonical decomposition a code point has, Hangul",
    "/// syllables included.",
    f"pub const DECOMP_MAX: usize = {max(d_max, 3)};",
    "",
    "/// The longest sequence a code point folds and decomposes to.",
    f"pub const FOLD_MAX: usize = {g_max};",
    "",
    f"/// Stage 1: `codepoint >> {SHIFT}` to one of {len(blocks)} distinct blocks.",
    "#[rustfmt::skip]",
    f"pub static SEARCH_INDEX: [{index_ty}; {len(index)}] = [",
    *rows(index, 24, str),
    "];",
    "",
    f"/// Stage 2: {len(blocks)} blocks of {BLOCK}, each entry an index into",
    "/// [`SEARCH_PROPS`].",
    "#[rustfmt::skip]",
    f"pub static SEARCH_BLOCKS: [u16; {len(flat)}] = [",
    *rows(flat, 16, str),
    "];",
    "",
    "/// `(d, g, ccc)`: the code point's decomposition and its folding, each an",
    "/// index into [`SEARCH_POOL`] (0 for the code point itself), and its",
    "/// canonical combining class.",
    "#[rustfmt::skip]",
    f"pub static SEARCH_PROPS: [(u16, u16, u8); {len(props)}] = [",
    *rows(props, 6, lambda p: f"({p[0]}, {p[1]}, {p[2]})"),
    "];",
    "",
    "/// The sequences: at an index, a length and then that many code points.",
    "#[rustfmt::skip]",
    f"pub static SEARCH_POOL: [u32; {len(pool)}] = [",
    *rows(pool, 10, lambda v: f"0x{v:04X}"),
    "];",
]
out = "\n".join(lines) + "\n"
if "--check" in sys.argv:
    if OUT.read_text() != out:
        print("STALE")
        sys.exit(1)
    print("OK")
else:
    OUT.write_text(out)
    print(f"props={len(props)} pool={len(pool)} blocks={len(blocks)} d_max={d_max} g_max={g_max} file={len(out)}")
