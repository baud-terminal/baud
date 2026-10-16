#!/usr/bin/env python3
"""Generate baud-domain/src/placeholder_data.rs from the pinned diacritic list.

`[[source: kitty-graphics]]`, "Unicode placeholders": "The row and column
values are specified with diacritics listed in rowcolumn-diacritics.txt",
pinned as `[[source: kitty-rowcolumn-diacritics]]`. The data line at index n,
from zero, is the diacritic for the number n ("U+305 is the diacritic
corresponding to the number 0 and U+30D corresponds to 1").

Two tables come out, both indexed and neither searched: the number to its
codepoint, and the codepoint to its number as a two-stage trie on
`codepoint >> 6`, which is what a mark after U+10EEEE asks.
"""
import pathlib, sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
SRC = ROOT / "docs/research/sources/kitty-rowcolumn-diacritics.txt"
OUT = ROOT / "baud-domain/src/placeholder_data.rs"
SHIFT = 6
BLOCK = 1 << SHIFT

cps = []
for line in SRC.read_text().splitlines():
    if not line or line.startswith("#"):
        continue
    cps.append(int(line.split(";")[0], 16))
if len(cps) != 297 or cps[0] != 0x305 or cps[1] != 0x30D or cps[2] != 0x30E:
    sys.exit(f"unexpected list: {len(cps)} entries starting {[hex(c) for c in cps[:3]]}")
if len(set(cps)) != len(cps):
    sys.exit("a diacritic is listed twice")

pages = (max(cps) >> SHIFT) + 1
blocks = [[0] * BLOCK]
index = []
by_cp = {c: n for n, c in enumerate(cps)}
for p in range(pages):
    block = [by_cp.get((p << SHIFT) + i, -1) + 1 for i in range(BLOCK)]
    if block not in blocks:
        blocks.append(block)
    index.append(blocks.index(block))
if len(blocks) > 256:
    sys.exit("more blocks than a byte names")


def rows(values, per, fmt):
    out = []
    for i in range(0, len(values), per):
        out.append("    " + " ".join(fmt(v) + "," for v in values[i:i + per]))
    return out


lines = [
    "//! The Unicode placeholder diacritics, generated from the list pinned in",
    "//! `docs/research/sources/MANIFEST.toml` (`[[source: kitty-rowcolumn-diacritics]]`).",
    "//!",
    "//! **Do not edit by hand.** Regenerate with `scripts/gen-placeholder-table.py`;",
    "//! `placeholder_table_matches_the_pinned_list` fails if this file and the",
    "//! pinned list disagree.",
    "",
    f"/// The diacritic for each number, `0..{len(cps)}`: the list's data lines in order.",
    "#[rustfmt::skip]",
    f"pub static DIACRITICS: [u32; {len(cps)}] = [",
    *rows([c for c in cps], 8, lambda v: f"0x{v:05X}"),
    "];",
    "",
    f"/// How far a codepoint is shifted for [`DIACRITIC_INDEX`]: blocks of {BLOCK}.",
    f"pub const DIACRITIC_SHIFT: u32 = {SHIFT};",
    "",
    f"/// Stage 1: `codepoint >> {SHIFT}` to its block in [`DIACRITIC_BLOCKS`], for",
    f"/// every codepoint below `0x{pages << SHIFT:05X}`; above it none is a diacritic.",
    "#[rustfmt::skip]",
    f"pub static DIACRITIC_INDEX: [u8; {pages}] = [",
    *rows(index, 16, lambda v: f"{v}"),
    "];",
    "",
    f"/// Stage 2: {len(blocks)} blocks of {BLOCK}, each entry the number plus one, or",
    "/// zero for a codepoint that is not a diacritic. Block 0 is all zeros.",
    "#[rustfmt::skip]",
    f"pub static DIACRITIC_BLOCKS: [u16; {len(blocks) * BLOCK}] = [",
    *rows([v for b in blocks for v in b], 16, lambda v: f"{v}"),
    "];",
]
out = "\n".join(lines) + "\n"

if "--check" in sys.argv:
    if not OUT.exists() or OUT.read_text() != out:
        print("STALE")
        sys.exit(1)
    print("OK")
else:
    OUT.write_text(out)
    print(f"{len(cps)} diacritics, {pages} index entries, {len(blocks)} blocks")
