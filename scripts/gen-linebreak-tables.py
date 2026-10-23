#!/usr/bin/env python3
"""Generate baud-domain/src/linebreak_data.rs from pinned Unicode data.

Same contract as gen-grapheme-tables.py: generated and committed, because
baud-domain has no build script, and `linebreak_tables_match_the_pinned_data`
regenerates and compares so the committed file cannot drift.

Each code point gets one byte: its Line_Break class resolved as UAX #14's
LB1 resolves it in the absence of other criteria (AI, SG and XX to AL; SA
to CM for General_Category Mn and Mc, else to AL; CJ to NS), with a
quotation mark split by its General_Category into QU, QU and Pi, and QU and
Pf, which LB15a and LB15b ask for (`[[source: uax14]]`); bit 6 says the
character is East Asian (East_Asian_Width F, W or H), which LB19a and LB30
ask for; bit 7 says it is Extended_Pictographic and unassigned, which
LB30b asks for.
"""
import pathlib
import sys

SRC = pathlib.Path(__file__).resolve().parent.parent / "docs/research/sources"

# `LineClass`'s declaration order, asserted against the enum by
# `the_line_trie_agrees_with_the_class_enum`.
CLASSES = ["AL", "BA", "BB", "BK", "B2", "CB", "CL", "CM", "CP", "CR", "EB",
           "EM", "EX", "GL", "H2", "H3", "HH", "HL", "HY", "ID", "IN", "IS",
           "JL", "JT", "JV", "LF", "NL", "NS", "NU", "OP", "PO", "PR", "QU",
           "RI", "SP", "SY", "WJ", "ZW", "ZWJ", "AK", "AP", "AS", "VF", "VI",
           "QuPi", "QuPf"]
BLOCK = 256


def fields(path):
    """Each data line's (lo, hi, value), comments and blank lines dropped."""
    for line in (SRC / path).read_text(errors="ignore").splitlines():
        line = line.split("#", 1)[0].strip()
        if not line:
            continue
        parts = [p.strip() for p in line.split(";")]
        if len(parts) < 2:
            continue
        lo, _, hi = parts[0].partition("..")
        yield int(lo, 16), int(hi or lo, 16), parts[1]


def table(path, default):
    out = [default] * 0x110000
    for lo, hi, v in fields(path):
        for cp in range(lo, min(hi, 0x10FFFF) + 1):
            out[cp] = v
    return out


# LineBreak.txt's own default is `# @missing: 0000..10FFFF; XX`, and 17.0.0
# lists every other code point; EastAsianWidth.txt's is N.
lb = table("unicode-linebreak.txt", "XX")
gc = table("unicode-derivedgeneralcategory.txt", "Cn")
eaw = table("unicode-eastasianwidth.txt", "N")
ext = [False] * 0x110000
for lo, hi, v in fields("unicode-emoji-data.txt"):
    if v == "Extended_Pictographic":
        for cp in range(lo, min(hi, 0x10FFFF) + 1):
            ext[cp] = True


def resolve(cp):
    c = lb[cp]
    if c in ("AI", "SG", "XX"):
        return "AL"
    if c == "SA":
        return "CM" if gc[cp] in ("Mn", "Mc") else "AL"
    if c == "CJ":
        return "NS"
    if c == "QU" and gc[cp] == "Pi":
        return "QuPi"
    if c == "QU" and gc[cp] == "Pf":
        return "QuPf"
    return c


idx = {c: i for i, c in enumerate(CLASSES)}
assert len(CLASSES) <= 64, "six bits a class"
combined = bytearray(0x110000)
for cp in range(0x110000):
    v = idx[resolve(cp)]
    if eaw[cp] in ("F", "W", "H"):
        v |= 0x40
    if ext[cp] and gc[cp] == "Cn":
        v |= 0x80
    combined[cp] = v

blocks, index = {}, []
for base in range(0, 0x110000, BLOCK):
    key = bytes(combined[base:base + BLOCK])
    if key not in blocks:
        blocks[key] = len(blocks)
    index.append(blocks[key])
assert len(blocks) <= 256, f"{len(blocks)} blocks will not fit a u8 stage 1"
packed = bytearray()
for key in blocks:
    packed.extend(key)
for cp in range(0x110000):
    if packed[index[cp >> 8] * BLOCK + (cp & 0xFF)] != combined[cp]:
        raise SystemExit(f"trie disagrees at U+{cp:04X}")


def as_bytestring(name, data, doc):
    """A byte string, which rustfmt leaves as written (gen-grapheme-tables.py)."""
    body = []
    for i in range(0, len(data), 32):
        chunk = "".join(f"\\x{v:02x}" for v in data[i:i + 32])
        body.append(f"    {chunk}\\")
    return "\n".join([doc, f'pub static {name}: &[u8; {len(data)}] = b"\\', *body, '";'])


parts = ["""//! Line-breaking data, generated from the sources pinned in
//! `docs/research/sources/MANIFEST.toml`.
//!
//! **Do not edit by hand.** Regenerate with `scripts/gen-linebreak-tables.py`.
//! `linebreak_tables_match_the_pinned_data` fails if this file and the pinned
//! data disagree."""]
parts.append(as_bytestring("LINE_INDEX", bytes(index), "\n".join([
    "/// `[[source: uax14]]` Line_Break, resolved by LB1, as a two-stage trie.",
    f"/// Stage 1: codepoint >> 8 to one of {len(blocks)} distinct blocks.",
    "///",
    "/// **Low six bits** are `LineClass` discriminants in declaration order;",
    "/// bit 6 is East_Asian_Width F, W or H; bit 7 is Extended_Pictographic",
    "/// and unassigned (`[[source: unicode-linebreak]]`, `[[source: gc-data]]`,",
    "/// `[[source: eaw-data]]`, `[[source: emoji-data]]`).",
])))
parts.append(as_bytestring("LINE_BLOCKS", bytes(packed), "/// Stage 2: one byte a code point."))
out = "\n\n".join(parts) + "\n"
target = pathlib.Path(__file__).resolve().parent.parent / "baud-domain/src/linebreak_data.rs"
if "--check" in sys.argv:
    if target.read_text() != out:
        print("STALE")
        sys.exit(1)
    print("OK")
else:
    target.write_text(out)
    print(f"blocks={len(blocks)} bytes={len(out)}")
