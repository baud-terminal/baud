#!/usr/bin/env python3
"""Generate baud-domain/src/sequence_data.rs from pinned Unicode data.

    python3 scripts/gen-sequence-table.py
    python3 scripts/gen-sequence-table.py --check   # sequence_data.rs is current

The sequences the Unicode Standard names, as a trie the cluster store steps
through one codepoint at a time: UTS #51's RGI emoji sequences
(`[[source: emoji17-sequences]]`, `[[source: emoji17-zwj-sequences]]`) and
the standardized emoji variation sequences
(`[[source: emoji-variation-sequences]]`). Every prefix of one is a node,
up to a base and `CAPACITY` continuations, the most a cluster slot holds.

An edge -- a parent cell's content word and the next codepoint -- is found
by a perfect hash built here: one bucket seed, one entry, one compare. No
probe sequence and no search at run time.

Generated and committed, because baud-domain has no build script;
`the_sequence_table_matches_the_pinned_data` runs `--check`.
"""
import pathlib
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
SRC = ROOT / "docs/research/sources"
OUT = ROOT / "baud-domain/src/sequence_data.rs"

# `ClusterSlot::CAPACITY`: continuations after the base.
CAPACITY = 8
# `Content::CLUSTERS` and `ClusterId::SEQUENCES`: a node's cell word is
# CLUSTERS + SEQUENCES + node. `the_sequence_words_are_the_cell_words`
# holds these to the Rust constants.
CLUSTERS = 0x0012_0000
SEQUENCES = 0x8000
WORD0 = CLUSTERS + SEQUENCES
# A key is the parent's word above the codepoint, 21 bits each.
assert WORD0 + 0x7FFF < 1 << 21

# The perfect hash: 2^BUCKET_BITS seeds, 2^EDGE_BITS entries.
BUCKET_BITS = 11
EDGE_BITS = 13
M1 = 0x9E37_79B9_7F4A_7C15
M2 = 0xD6E8_FEB8_6659_FD93
MASK = (1 << 64) - 1


def parse(name):
    out = []
    for line in (SRC / name).read_text(encoding="utf-8").splitlines():
        line = line.split("#", 1)[0].strip()
        if not line:
            continue
        field = line.split(";", 1)[0].strip()
        if ".." in field:
            continue  # a range of single codepoints: no continuation
        cps = tuple(int(x, 16) for x in field.split())
        if len(cps) > 1:
            out.append(cps)
    return out


seqs = set()
for name in ["emoji17-sequences.txt", "emoji17-zwj-sequences.txt",
             "unicode-emoji-variation-sequences.txt"]:
    seqs.update(parse(name))
assert len(seqs) > 3000, len(seqs)

# Every prefix of two codepoints or more, up to a base and CAPACITY marks.
nodes = set()
for s in seqs:
    for n in range(2, min(len(s), 1 + CAPACITY) + 1):
        nodes.add(s[:n])
nodes = sorted(nodes, key=lambda s: (len(s), s))
assert len(nodes) <= 0x7FFF, len(nodes)
index = {s: i for i, s in enumerate(nodes)}

# Each node's codepoints, as a span of one flat array: a node shares the
# span of the first longest sequence it begins.
children = {}
for s in nodes:
    children.setdefault(s[:-1], []).append(s)
leaves = sorted(s for s in nodes if s not in children)
flat, span = [], {}
for s in leaves:
    off = len(flat)
    flat.extend(s)
    for n in range(2, len(s) + 1):
        span.setdefault(s[:n], off)
assert all(s in span for s in nodes)
assert len(flat) < 1 << 16, len(flat)


def word(parent):
    """The content word of the cell a continuation attaches to."""
    if len(parent) == 1:
        return parent[0]
    return WORD0 + index[parent]


edges = []
for s in nodes:
    key = (word(s[:-1]) << 21) | s[-1]
    edges.append((key, index[s]))
assert len(set(k for k, _ in edges)) == len(edges)


def bucket(key):
    return ((key * M1) & MASK) >> (64 - BUCKET_BITS)


def slot(key, seed):
    return (((key ^ seed) * M2) & MASK) >> (64 - EDGE_BITS)


by_bucket = {}
for k, v in edges:
    by_bucket.setdefault(bucket(k), []).append((k, v))
seeds = [0] * (1 << BUCKET_BITS)
table = [0] * (1 << EDGE_BITS)
for b, ks in sorted(by_bucket.items(), key=lambda kv: (-len(kv[1]), kv[0])):
    for seed in range(1 << 16):
        at = [slot(k, seed) for k, _ in ks]
        if len(set(at)) == len(at) and all(table[a] == 0 for a in at):
            break
    else:
        sys.exit(f"bucket {b}: no seed places its {len(ks)} keys")
    seeds[b] = seed
    for (k, v), a in zip(ks, at):
        # The key above the node's number plus one, so an empty entry,
        # zero, matches no key.
        table[a] = (k << 16) | (v + 1)

# The blocks (codepoint >> 8) any continuation in a sequence falls in: a
# continuation outside them -- a Hebrew point, a Devanagari sign, a kana
# voicing mark -- cannot step the trie, and one bit says so.
blocks = sorted({s[-1] >> 8 for s in nodes})
bitmap = [0] * (0x1100 // 8)
for b in blocks:
    bitmap[b >> 3] |= 1 << (b & 7)


def rows(vals, fmt, per):
    lines = []
    for i in range(0, len(vals), per):
        lines.append("    " + " ".join(fmt.format(v) + "," for v in vals[i:i + per]))
    return "\n".join(lines)


packed = [span[s] | (len(s) << 16) for s in nodes]
src = f'''//! The sequences the Unicode Standard names, as a trie: UTS #51's RGI
//! emoji sequences (`[[source: emoji17-sequences]]`,
//! `[[source: emoji17-zwj-sequences]]`) and the standardized emoji
//! variation sequences (`[[source: emoji-variation-sequences]]`).
//!
//! **Do not edit by hand.** Regenerate with `scripts/gen-sequence-table.py`;
//! `the_sequence_table_matches_the_pinned_data` fails if this file and the
//! pinned data disagree.
//!
//! {len(seqs)} sequences of two codepoints or more, {len(nodes)} prefixes of a
//! base and up to {CAPACITY} continuations, {len(flat)} codepoints of spans.

/// The first node's cell word: `Content::CLUSTERS + ClusterId::SEQUENCES`.
pub const WORD0: u32 = {WORD0:#x};
/// Nodes, each a prefix of two codepoints or more.
pub const NODES: usize = {len(nodes)};
/// Bucket and entry bits of the perfect hash.
pub const BUCKET_BITS: u32 = {BUCKET_BITS};
pub const EDGE_BITS: u32 = {EDGE_BITS};
pub const M1: u64 = {M1:#x};
pub const M2: u64 = {M2:#x};

/// A node's span of `SPANS`: the offset in the low 16 bits, the length in
/// codepoints above them.
#[rustfmt::skip]
pub static NODE_SPAN: [u32; {len(packed)}] = [
{rows(packed, "{:#08x}", 8)}
];

/// The nodes' codepoints: each longest sequence once, its prefixes sharing
/// its span.
#[rustfmt::skip]
pub static SPANS: [u32; {len(flat)}] = [
{rows(flat, "{:#07x}", 8)}
];

/// Each bucket's seed.
#[rustfmt::skip]
pub static SEEDS: [u16; {len(seeds)}] = [
{rows(seeds, "{}", 16)}
];

/// Each entry: the key -- the parent's word above the codepoint, 21 bits
/// each -- above the child node plus one; zero where no edge lands.
#[rustfmt::skip]
pub static EDGES: [u64; {len(table)}] = [
{rows(table, "{:#x}", 4)}
];

/// One bit a block of 256 codepoints: set where some sequence continues
/// with a codepoint of that block.
#[rustfmt::skip]
pub static CONTINUES: [u8; {len(bitmap)}] = [
{rows(bitmap, "{:#04x}", 16)}
];
'''

if "--check" in sys.argv:
    if OUT.read_text() != src:
        print("STALE: sequence_data.rs is not what the pinned data generates")
        sys.exit(1)
    sys.exit(0)
OUT.write_text(src)
print(f"{len(seqs)} sequences, {len(nodes)} nodes, {len(flat)} span codepoints, "
      f"{len(edges)} edges in {len(table)}, blocks {[hex(b) for b in blocks]}")
