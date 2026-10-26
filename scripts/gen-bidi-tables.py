#!/usr/bin/env python3
"""Generate baud-domain/src/bidi_data.rs from pinned Unicode data.

Same contract as gen-linebreak-tables.py: generated and committed, because
baud-domain has no build script, and `bidi_tables_match_the_pinned_data`
regenerates and compares so the committed file cannot drift.

Two tries, both indexed by `codepoint >> 8` and then the low byte:

* **BIDI**, one byte a code point: Bidi_Class in the low five bits
  (`BidiClass`'s declaration order, UAX #9 Table 4), Bidi_Mirrored in bit 5,
  and Bidi_Paired_Bracket_Type in bits 6-7 (0 none, 1 open, 2 close).
  Bidi_Class is DerivedBidiClass.txt with its `@missing` defaults, applied in
  the file's order, for the code points it does not list
  (`[[source: unicode-derivedbidiclass]]`); Bidi_Mirrored is UnicodeData.txt
  field 9 (`[[source: unicode-data]]`); the bracket type is BidiBrackets.txt
  (`[[source: unicode-bidibrackets]]`).
* **MIRROR**, two bytes a code point: the low nine bits index `MIRROR_GLYPHS`,
  the code point's Bidi_Mirroring_Glyph (`[[source: unicode-bidimirroring]]`),
  0 for none; bits 9-14 are the bracket pair it belongs to, 0 for none, so
  that BD16's comparison "or its canonical equivalent" is one equality: the
  two canonically equivalent brackets, U+2329 and U+232A, take the pair of
  U+3008 and U+3009, their decompositions in UnicodeData.txt.
* **BOX_COUNTERPART**, a byte for each Box Drawing character U+2500-U+257F:
  the offset in the block of the character whose UnicodeData.txt name says
  the same with LEFT and RIGHT exchanged (`[[source: unicode-data]]`), for
  terminal-wg's box mirroring, `?2500`. A name is read as what it draws --
  each arm and its weight, a dash's count, an arc, a diagonal -- so that
  "LIGHT LEFT AND HEAVY RIGHT" finds "HEAVY LEFT AND LIGHT RIGHT", whose
  words come in another order; a name with no side is its own.
"""
import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
SRC = ROOT / "docs/research/sources"
OUT = ROOT / "baud-domain/src/bidi_data.rs"

# `BidiClass`'s declaration order, UAX #9 Table 4's, asserted against the
# enum by `the_bidi_trie_agrees_with_the_class_enum`.
CLASSES = ["L", "R", "AL", "EN", "ES", "ET", "AN", "CS", "NSM", "BN", "B", "S",
           "WS", "ON", "LRE", "LRO", "RLE", "RLO", "PDF", "LRI", "RLI", "FSI",
           "PDI"]
# The long names `@missing` lines use (PropertyValueAliases.txt's bc).
LONG = {"Left_To_Right": "L", "Right_To_Left": "R", "Arabic_Letter": "AL",
        "European_Terminator": "ET", "Boundary_Neutral": "BN"}
BLOCK = 256


def data_lines(path):
    for line in (SRC / path).read_text(encoding="utf-8").splitlines():
        line = line.split("#", 1)[0].strip()
        if line:
            yield [p.strip() for p in line.split(";")]


def span(field):
    lo, _, hi = field.partition("..")
    return int(lo, 16), int(hi or lo, 16)


# Bidi_Class: the `@missing` defaults first, in the file's order (each later
# line narrows the one before), then every listed range.
bidi = [None] * 0x110000
missing = 0
for line in (SRC / "unicode-derivedbidiclass.txt").read_text(encoding="utf-8").splitlines():
    m = re.match(r"# @missing: ([0-9A-F]+)\.\.([0-9A-F]+); (\w+)$", line)
    if m:
        lo, hi = int(m[1], 16), int(m[2], 16)
        for cp in range(lo, hi + 1):
            bidi[cp] = LONG[m[3]]
        missing += 1
assert missing == 24, missing
for p in data_lines("unicode-derivedbidiclass.txt"):
    lo, hi = span(p[0])
    for cp in range(lo, hi + 1):
        bidi[cp] = p[1]
assert all(c in CLASSES for c in bidi), set(bidi) - set(CLASSES)
# Positive controls, one a default range: an unassigned code point in the
# Hebrew block is R, one in Arabic is AL, one in Currency Symbols is ET,
# a noncharacter is BN, and U+0378 (unassigned Greek) takes the file's L.
assert bidi[0x05FF] == "R" and bidi[0x07BF] == "AL" and bidi[0x20CF] == "ET"
assert bidi[0xFDD0] == "BN" and bidi[0x0378] == "L" and bidi[0x0028] == "ON"
assert bidi[0x05D0] == "R" and bidi[0x0627] == "AL" and bidi[0x0030] == "EN"
assert bidi[0x2067] == "RLI" and bidi[0x202E] == "RLO" and bidi[0x2029] == "B"

# Bidi_Mirrored: UnicodeData.txt field 9, ranges given as First/Last pairs.
mirrored = bytearray(0x110000)
first = None
for line in (SRC / "unicode-unicodedata.txt").read_text(encoding="utf-8").splitlines():
    f = line.split(";")
    cp = int(f[0], 16)
    if f[1].endswith(", First>"):
        first = cp
        continue
    lo = first if f[1].endswith(", Last>") else cp
    first = None
    if f[9] == "Y":
        for c in range(lo, cp + 1):
            mirrored[c] = 1
assert mirrored[0x28] and mirrored[0x2211] and not mirrored[0x41]
# L4's own note: U+FD3E and U+FD3F "are not mirrored".
assert not mirrored[0xFD3E] and not mirrored[0xFD3F]

# Bidi_Mirroring_Glyph.
glyph = {}
for p in data_lines("unicode-bidimirroring.txt"):
    glyph[int(p[0], 16)] = int(p[1], 16)
assert len(glyph) == 428 and glyph[0x28] == 0x29 and glyph[0x29] == 0x28
for cp in glyph:
    assert mirrored[cp], hex(cp)
# Bidi_Mirrored=Yes with no glyph exists (U+2211 N-ARY SUMMATION): the two
# properties are different, and L4 needs the first, a font the second.
assert 0x2211 not in glyph

# Bidi_Paired_Bracket and its type. BidiBrackets.txt's header: "two
# characters, A and B, form a bracket pair if A has gc=Ps and B has gc=Pe,
# both have bc=ON and Bidi_M=Y, and bmg of A is B".
bpb, bpt = {}, {}
for p in data_lines("unicode-bidibrackets.txt"):
    if p[2] == "n":
        continue
    bpb[int(p[0], 16)], bpt[int(p[0], 16)] = int(p[1], 16), p[2]
assert len(bpb) == 128 and sum(t == "o" for t in bpt.values()) == 64
for cp, other in bpb.items():
    assert bpb[other] == cp and {bpt[cp], bpt[other]} == {"o", "c"}, hex(cp)
    assert bidi[cp] == "ON" and mirrored[cp], hex(cp)
    # So Bidi_Paired_Bracket is Bidi_Mirroring_Glyph for every bracket.
    assert glyph[cp] == other, hex(cp)
# BD16's canonical equivalents, from the canonical decompositions: "canonical
# equivalents only exist between U+3008/U+3009, and U+2329/U+232A".
canonical = {}
for line in (SRC / "unicode-unicodedata.txt").read_text(encoding="utf-8").splitlines():
    f = line.split(";")
    cp, dec = int(f[0], 16), f[5]
    if cp in bpb and dec and not dec.startswith("<") and " " not in dec:
        canonical[cp] = int(dec, 16)
assert canonical == {0x2329: 0x3008, 0x232A: 0x3009}, canonical
pair = {}
for cp in sorted(c for c in bpb if bpt[c] == "o" and c not in canonical):
    pair[cp] = len(pair) + 1
for cp, to in canonical.items():
    if bpt[cp] == "o":
        pair[cp] = pair[to]
for cp in list(pair):
    pair[bpb[cp]] = pair[cp]
assert len(set(pair.values())) == 63 and max(pair.values()) < 64
assert pair[0x28] == pair[0x29] and pair[0x2329] == pair[0x3009] != pair[0x28]

# The two value arrays.
idx = {c: i for i, c in enumerate(CLASSES)}
props = bytearray(0x110000)
for cp in range(0x110000):
    v = idx[bidi[cp]] | (mirrored[cp] << 5)
    if cp in bpt:
        v |= (1 if bpt[cp] == "o" else 2) << 6
    props[cp] = v
glyphs = [0] + sorted(set(glyph.values()))
assert len(glyphs) < 512
at = {g: i for i, g in enumerate(glyphs)}
mirror = [0] * 0x110000
for cp, g in glyph.items():
    mirror[cp] = at[g]
for cp, n in pair.items():
    mirror[cp] |= n << 9


def trie(values):
    blocks, index = {}, []
    for base in range(0, 0x110000, BLOCK):
        key = tuple(values[base:base + BLOCK])
        if key not in blocks:
            blocks[key] = len(blocks)
        index.append(blocks[key])
    assert len(blocks) <= 256, f"{len(blocks)} blocks will not fit a u8 stage 1"
    flat = [v for key in blocks for v in key]
    for cp in range(0x110000):
        assert flat[index[cp >> 8] * BLOCK + (cp & 0xFF)] == values[cp], hex(cp)
    return index, flat, len(blocks)


bidi_index, bidi_flat, bidi_blocks = trie(props)
mirror_index, mirror_flat, mirror_blocks = trie(mirror)


def as_bytestring(name, data, doc):
    """A byte string, which rustfmt leaves as written (gen-grapheme-tables.py)."""
    body = []
    for i in range(0, len(data), 32):
        chunk = "".join(f"\\x{v:02x}" for v in data[i:i + 32])
        body.append(f"    {chunk}\\")
    return "\n".join([doc, f'pub static {name}: &[u8; {len(data)}] = b"\\', *body, '";'])


def rows(values, per, fmt):
    return ["    " + ", ".join(fmt(v) for v in values[i:i + per]) + ","
            for i in range(0, len(values), per)]


parts = ["""//! Bidirectional data, generated from the sources pinned in
//! `docs/research/sources/MANIFEST.toml`.
//!
//! **Do not edit by hand.** Regenerate with `scripts/gen-bidi-tables.py`.
//! `bidi_tables_match_the_pinned_data` fails if this file and the pinned
//! data disagree."""]
parts.append(as_bytestring("BIDI_INDEX", bytes(bidi_index), "\n".join([
    "/// Bidi_Class, Bidi_Mirrored and Bidi_Paired_Bracket_Type, as a two-stage",
    f"/// trie. Stage 1: codepoint >> 8 to one of {bidi_blocks} distinct blocks.",
    "///",
    "/// **Low five bits** are `BidiClass` discriminants in declaration order",
    "/// (`[[source: unicode-derivedbidiclass]]`, its `@missing` defaults",
    "/// included); bit 5 is Bidi_Mirrored (`[[source: unicode-data]]`);",
    "/// bits 6-7 are 1 for an opening and 2 for a closing paired bracket",
    "/// (`[[source: unicode-bidibrackets]]`).",
])))
parts.append(as_bytestring("BIDI_BLOCKS", bytes(bidi_flat), "/// Stage 2: one byte a code point."))
parts.append("\n".join([
    "/// Bidi_Mirroring_Glyph and the bracket pair, as a two-stage trie.",
    f"/// Stage 1: codepoint >> 8 to one of {mirror_blocks} distinct blocks.",
    "#[rustfmt::skip]",
    f"pub static MIRROR_INDEX: [u8; {len(mirror_index)}] = [",
    *rows(mirror_index, 24, str),
    "];",
    "",
    "/// Stage 2: the low nine bits index [`MIRROR_GLYPHS`], 0 for none",
    "/// (`[[source: unicode-bidimirroring]]`); bits 9-14 are the bracket pair,",
    "/// 0 for none, with U+2329 and U+232A in U+3008 and U+3009's pair.",
    "#[rustfmt::skip]",
    f"pub static MIRROR_BLOCKS: [u16; {len(mirror_flat)}] = [",
    *rows(mirror_flat, 16, str),
    "];",
    "",
    f"/// The {len(glyphs) - 1} distinct mirroring glyphs, after a 0 for none.",
    "#[rustfmt::skip]",
    f"pub static MIRROR_GLYPHS: [u32; {len(glyphs)}] = [",
    *rows(glyphs, 10, lambda v: f"0x{v:04X}"),
    "];",
]))
# Box Drawing counterparts: each name read as the set of what it draws.
WEIGHTS = {"LIGHT", "HEAVY", "DOUBLE", "SINGLE"}
ARMS = {"UP": ("UP",), "DOWN": ("DOWN",), "LEFT": ("LEFT",), "RIGHT": ("RIGHT",),
        "VERTICAL": ("UP", "DOWN"), "HORIZONTAL": ("LEFT", "RIGHT")}
SWAP = {"LEFT": "RIGHT", "RIGHT": "LEFT"}


def drawn(words):
    """What a Box Drawing name draws: a frozenset of (arm, weight) and the
    words that are neither, or the words as they are for a diagonal. The
    name's groups are split at AND; a group's weight is its arms', and a
    group naming none takes the last named, so "LIGHT DOWN AND RIGHT" is
    light both ways and "DOWN LIGHT AND RIGHT HEAVY" is not."""
    if "DIAGONAL" in words:
        return ("words", tuple(words))
    groups, cur = [], []
    for w in words:
        if w == "AND":
            groups.append(cur)
            cur = []
        else:
            cur.append(w)
    groups.append(cur)
    weight, arms, rest = None, set(), []
    for g in groups:
        weight = next((w for w in g if w in WEIGHTS), weight)
        for w in g:
            if w in ARMS:
                arms.update((a, weight) for a in ARMS[w])
            elif w not in WEIGHTS:
                rest.append(w)
    return ("arms", frozenset(arms), tuple(rest))


box_names = {}
for line in (SRC / "unicode-unicodedata.txt").read_text(encoding="utf-8").splitlines():
    f = line.split(";")
    cp = int(f[0], 16)
    if 0x2500 <= cp <= 0x257F:
        assert f[1].startswith("BOX DRAWINGS "), f[1]
        box_names[cp] = f[1][len("BOX DRAWINGS "):].split()
assert len(box_names) == 0x80
by_drawn = {}
for cp, words in box_names.items():
    key = drawn(words)
    assert key not in by_drawn, (hex(cp), words)
    by_drawn[key] = cp
box = []
for cp in range(0x2500, 0x2580):
    other = by_drawn[drawn([SWAP.get(w, w) for w in box_names[cp]])]
    box.append(other - 0x2500)
for i, o in enumerate(box):
    assert box[o] == i, (hex(0x2500 + i), hex(0x2500 + o))
parts.append("\n".join([
    "/// Each Box Drawing character's left-right counterpart, as its offset",
    "/// in U+2500-U+257F: the name with LEFT and RIGHT exchanged",
    "/// (`[[source: unicode-data]]`), itself where the name has no side.",
    "#[rustfmt::skip]",
    f"pub static BOX_COUNTERPART: [u8; {len(box)}] = [",
    *rows(box, 16, lambda v: f"0x{v:02X}"),
    "];",
]))
out = "\n\n".join(parts) + "\n"
if "--check" in sys.argv:
    if OUT.read_text() != out:
        print("STALE")
        sys.exit(1)
    print("OK")
else:
    OUT.write_text(out)
    print(f"bidi blocks={bidi_blocks} mirror blocks={mirror_blocks} glyphs={len(glyphs) - 1} "
          f"bytes={len(bidi_index) + len(bidi_flat) + len(mirror_index) + 2 * len(mirror_flat) + 4 * len(glyphs)}")
