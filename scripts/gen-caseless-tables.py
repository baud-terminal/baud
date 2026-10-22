#!/usr/bin/env python3
"""Generate baud-domain/src/caseless_data.rs: what a font family name compares.

CSS Fonts 3 section 5.1 (`[[source: css-fonts-3]]`) matches a family name
"case insensitively, using the \"Default Caseless Matching\" algorithm" of
the Unicode Standard, "applied without normalizing the strings involved",
with "the case mappings with status field 'C' or 'F' in the CaseFolding.txt
file". That is D144 of `[[source: unicode17-ch3]]`: toCasefold(X) =
toCasefold(Y), R4's toCasefold mapping each character to its full case
folding (`[[source: unicode-casefolding]]`) and nothing else. The search
tables (`gen-search-tables.py`) fold and then decompose, the canonical
caseless match of D145, which would match a precomposed letter against its
decomposition -- the pair CSS says "will not match" -- so this is its own
table.

Per code point the table holds one thing: its full case folding, as an index
into a pool of sequences, index 0 meaning "the code point itself". A two-stage
trie on `codepoint >> SHIFT`; the generator checks it against the direct
computation at every one of the 1,114,112 code points before it writes.

Positive controls, asserted: U+00DF folds to "ss", U+0130 to "i" + U+0307,
U+212A KELVIN SIGN to "k", U+00C5 to U+00E5 (one code point, not a
decomposition), U+0049 to U+0069 (the Turkic status T rows are left out).

Usage: gen-caseless-tables.py [--check]
"""
import pathlib
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
CF = ROOT / "docs/research/sources/unicode-casefolding.txt"
OUT = ROOT / "baud-domain/src/caseless_data.rs"
SHIFT = 8
BLOCK = 1 << SHIFT
N = 0x110000

fold = {}
for line in CF.read_text().splitlines():
    line = line.split("#", 1)[0].strip()
    if not line:
        continue
    code, status, mapping = [x.strip() for x in line.split(";")[:3]]
    if status in ("C", "F"):
        fold[int(code, 16)] = [int(x, 16) for x in mapping.split()]

for cp, want in [
    (0xDF, [0x73, 0x73]),
    (0x130, [0x69, 0x307]),
    (0x212A, [0x6B]),
    (0xC5, [0xE5]),
    (0x49, [0x69]),
]:
    if fold.get(cp) != want:
        sys.exit(f"control: U+{cp:04X} folds to {fold.get(cp)}, want {want}")

pool = [0]
seqs = {}
entry = [0] * N
fold_max = 1
for cp, out in sorted(fold.items()):
    fold_max = max(fold_max, len(out))
    key = tuple(out)
    if key not in seqs:
        seqs[key] = len(pool)
        pool.append(len(out))
        pool.extend(out)
    entry[cp] = seqs[key]
if len(pool) > 0xFFFF:
    sys.exit(f"{len(pool)} pool words do not fit a u16")

blocks = []
block_ix = {}
index = []
for b in range(N // BLOCK):
    block = tuple(entry[b * BLOCK:(b + 1) * BLOCK])
    if block not in block_ix:
        block_ix[block] = len(blocks)
        blocks.append(block)
    index.append(block_ix[block])
if len(blocks) > 0xFF:
    sys.exit("more blocks than a u8 names")


def at(cp):
    i = blocks[index[cp >> SHIFT]][cp & (BLOCK - 1)]
    return [cp] if i == 0 else pool[i + 1:i + 1 + pool[i]]


for cp in range(N):
    if at(cp) != fold.get(cp, [cp]):
        sys.exit(f"trie disagrees at U+{cp:04X}")


def rows(values, per, fmt):
    return ["    " + " ".join(fmt(v) + "," for v in values[i:i + per]) for i in range(0, len(values), per)]


flat = [v for b in blocks for v in b]
lines = [
    "//! What a font family name compares, generated from",
    "//! `[[source: unicode-casefolding]]`'s full case folding (status C and F)",
    "//! pinned in `docs/research/sources/MANIFEST.toml`, for the default",
    "//! caseless match of `[[source: unicode17-ch3]]` D144 that",
    "//! `[[source: css-fonts-3]]` section 5.1 names.",
    "//!",
    "//! **Do not edit by hand.** Regenerate with `scripts/gen-caseless-tables.py`;",
    "//! `caseless_tables_match_the_pinned_data` fails if this file and the",
    "//! pinned data disagree.",
    "",
    "/// The trie's block size, as a shift: `codepoint >> CASELESS_SHIFT` names",
    "/// a block in [`CASELESS_INDEX`].",
    f"pub const CASELESS_SHIFT: u32 = {SHIFT};",
    "",
    f"/// Stage 1: `codepoint >> {SHIFT}` to one of {len(blocks)} distinct blocks.",
    "#[rustfmt::skip]",
    f"pub static CASELESS_INDEX: [u8; {len(index)}] = [",
    *rows(index, 24, str),
    "];",
    "",
    f"/// Stage 2: {len(blocks)} blocks of {BLOCK}, each entry an index into",
    "/// [`CASELESS_POOL`], 0 for the code point itself.",
    "#[rustfmt::skip]",
    f"pub static CASELESS_BLOCKS: [u16; {len(flat)}] = [",
    *rows(flat, 16, str),
    "];",
    "",
    "/// The foldings: at an index, a length and then that many code points.",
    "#[rustfmt::skip]",
    f"pub static CASELESS_POOL: [u32; {len(pool)}] = [",
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
    print(f"pool={len(pool)} blocks={len(blocks)} fold_max={fold_max} file={len(out)}")
