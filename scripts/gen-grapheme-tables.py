#!/usr/bin/env python3
"""Generate baud-domain/src/grapheme_data.rs from pinned Unicode data.

Same contract as gen-width-tables.py: generated and committed, because
baud-domain has no build script, and `grapheme_tables_match_the_pinned_data`
regenerates and compares so the committed file cannot drift.
"""
import sys, pathlib

SRC = pathlib.Path(__file__).resolve().parent.parent / "docs/research/sources"


def parse(path, wanted):
    """(lo, hi) ranges per property value, for the values in `wanted`."""
    out = {w: [] for w in wanted}
    for line in (SRC / path).read_text(errors="ignore").splitlines():
        line = line.split("#", 1)[0].strip()
        if not line:
            continue
        parts = [p.strip() for p in line.split(";")]
        if len(parts) < 2:
            continue
        field, value = parts[0], parts[1]
        if value not in out:
            continue
        lo, _, hi = field.partition("..")
        out[value].append((int(lo, 16), int(hi or lo, 16)))
    return out


def coalesce(rs):
    rs = sorted(rs)
    m = []
    for lo, hi in rs:
        if m and lo <= m[-1][1] + 1:
            m[-1][1] = max(m[-1][1], hi)
        else:
            m.append([lo, hi])
    return [(a, b) for a, b in m]


# GraphemeBreakProperty values baud needs. Anything not listed is `Other`.
GBP = ["CR", "LF", "Control", "Extend", "ZWJ", "Regional_Indicator",
       "Prepend", "SpacingMark", "L", "V", "T", "LV", "LVT"]
gbp = parse("unicode-graphemebreakproperty.txt", GBP)
# Extended_Pictographic lives in emoji-data.txt; InCB in DerivedCoreProperties.
ext = parse("unicode-emoji-data.txt", ["Extended_Pictographic"])
incb_raw = {}
for line in (SRC / "unicode-derivedcoreproperties.txt").read_text(errors="ignore").splitlines():
    line = line.split("#", 1)[0].strip()
    if not line or "InCB" not in line:
        continue
    parts = [p.strip() for p in line.split(";")]
    # `0300..036F    ; InCB; Extend`
    if len(parts) < 3 or parts[1] != "InCB":
        continue
    lo, _, hi = parts[0].partition("..")
    incb_raw.setdefault(parts[2], []).append((int(lo, 16), int(hi or lo, 16)))

# One enum discriminant per class; the table maps codepoint -> class.
CLASSES = [
    ("Other", []),
    ("Cr", gbp["CR"]),
    ("Lf", gbp["LF"]),
    ("Control", gbp["Control"]),
    ("Extend", gbp["Extend"]),
    ("Zwj", gbp["ZWJ"]),
    ("RegionalIndicator", gbp["Regional_Indicator"]),
    ("Prepend", gbp["Prepend"]),
    ("SpacingMark", gbp["SpacingMark"]),
    ("L", gbp["L"]),
    ("V", gbp["V"]),
    ("T", gbp["T"]),
    ("Lv", gbp["LV"]),
    ("Lvt", gbp["LVT"]),
]

rows = []
for name, ranges in CLASSES:
    if name == "Other":
        continue
    for lo, hi in coalesce(ranges):
        rows.append((lo, hi, name))
rows.sort()

def emit_ranges(name, ranges, doc):
    ranges = coalesce(ranges)
    out = [doc, f"pub static {name}: [(u32, u32); {len(ranges)}] = ["]
    out += [f"    (0x{a:04x}, 0x{b:04x})," for a, b in ranges]
    out.append("];")
    return "\n".join(out)

parts = ['''//! Grapheme-cluster data, generated from the sources pinned in
//! `docs/research/sources/MANIFEST.toml`.
//!
//! **Do not edit by hand.** Regenerate with `scripts/gen-grapheme-tables.py`.
//! `grapheme_tables_match_the_pinned_data` fails if this file and the pinned
//! data disagree.''']

# `GraphemeClass`'s declaration order. Asserted against the enum below rather
# than trusted: a silently reordered enum would remap every codepoint's class,
# and the result would look like working code.
CLASSES = ["Other", "Cr", "Lf", "Control", "Extend", "Zwj", "RegionalIndicator",
           "Prepend", "SpacingMark", "L", "V", "T", "Lv", "Lvt"]
BLOCK = 256


def build_break_trie(rows):
    """A two-stage table for Grapheme_Cluster_Break, checked against the ranges.

    `GraphemeClass::of` binary searched 1,386 ranges for every non-ASCII
    codepoint -- about eleven data-dependent branches. It profiled at **27.1%**
    of the CJK path, the single largest item, and a ceiling arm returning a
    constant measured +21 to +29%.

    Fourteen classes fit four bits, two per byte. At 256-codepoint blocks the
    result is 16,000 bytes against the range table's ~16,632, so this is
    smaller as well as flatter.
    """
    idxof = {c: i for i, c in enumerate(CLASSES)}
    assert len(CLASSES) <= 16, "four bits per value"
    cls = bytearray(0x110000)
    for lo, hi, k in rows:
        v = idxof[k]
        for cp in range(lo, min(hi, 0x10FFFF) + 1):
            cls[cp] = v

    blocks, index = {}, []
    for base in range(0, 0x110000, BLOCK):
        key = bytes(cls[base : base + BLOCK])
        if key not in blocks:
            blocks[key] = len(blocks)
        index.append(blocks[key])
    assert len(blocks) <= 256, f"{len(blocks)} blocks will not fit a u8 stage 1"

    packed = bytearray()
    for key in blocks:
        for i in range(0, BLOCK, 2):
            packed.append(key[i] | (key[i + 1] << 4))

    # Exhaustive, against the ranges this file is generated from.
    for cp in range(0x110000):
        blk = index[cp >> 8]
        byte = packed[blk * (BLOCK // 2) + ((cp & 0xFF) >> 1)]
        got = (byte >> ((cp & 1) * 4)) & 0xF
        if got != cls[cp]:
            raise SystemExit(f"trie disagrees at U+{cp:04X}: {got} != {cls[cp]}")
    return index, packed, len(blocks)


def as_bytestring(name, data, doc):
    """A byte string, not an array literal.

    rustfmt rewraps array literals to its width, so a generator emitting one
    can never satisfy its own staleness check -- the file it writes is not the
    file that gets committed. String literals are left alone and `b"..."` is
    byte-for-byte the same rodata.
    """
    body = []
    for i in range(0, len(data), 32):
        chunk = "".join(f"\\x{v:02x}" for v in data[i : i + 32])
        body.append(f"    {chunk}\\")
    return "\n".join([doc, f'pub static {name}: &[u8; {len(data)}] = b"\\', *body, '";'])


bp_index, bp_packed, bp_blocks = build_break_trie(rows)
parts.append(as_bytestring(
    "BREAK_INDEX", bp_index,
    "\n".join([
        "/// `[[source: uax29]]` Grapheme_Cluster_Break, as a two-stage trie.",
        "/// Stage 1: codepoint >> 8 to one of "
        + str(bp_blocks)
        + " distinct blocks.",
        "///",
        "/// This was 1,386 binary-searched `(lo, hi, class)` ranges, consulted",
        "/// once per non-ASCII codepoint and **27.1%** of the CJK path -- the",
        "/// single largest item in that profile. A ceiling arm returning a",
        "/// constant measured +21 to +29%.",
        "///",
        "/// Values are `GraphemeClass` discriminants in declaration order;",
        "/// `the_break_trie_agrees_with_the_class_enum` pins that correspondence,",
        "/// because a reordered enum would remap every codepoint silently.",
    ])))
parts.append(as_bytestring(
    "BREAK_BLOCKS", bp_packed,
    "/// Stage 2: four-bit class values, two per byte, low nibble first."))

# A membership bitmap over the four *lookback* tables, at 32 codepoints a bit.
#
# `ClusterBreak::advance` runs for every codepoint and, above the `cp < 0xa9`
# guard, did two unconditional binary searches -- `EXTENDED_PICTOGRAPHIC` and
# `INCB_CONSONANT` -- to learn that an ordinary character is neither an emoji
# nor an Indic consonant. btop draws with `U+2500..U+257F` and paid both on
# every box-drawing character.
#
# 32 rather than 256 codepoints a bit because the block matters: `U+25xx`
# contains box drawing *and* emoji (`U+25AA`, `U+25B6`, `U+25FB..FE`), so a
# 256-wide block cannot separate them and a 32-wide one can.
LOOKBACK_SHIFT = 5
LB_N = 0x110000 >> LOOKBACK_SHIFT
lb = [0] * ((LB_N + 63) // 64)
for tbl in (ext["Extended_Pictographic"],
            incb_raw.get("Consonant", []),
            incb_raw.get("Linker", []),
            incb_raw.get("Extend", [])):
    for a, b in coalesce(tbl):
        for blk in range(a >> LOOKBACK_SHIFT, (b >> LOOKBACK_SHIFT) + 1):
            lb[blk // 64] |= 1 << (blk % 64)
lb_set = sum(bin(w).count("1") for w in lb)
lines = [f"/// One bit per {1 << LOOKBACK_SHIFT} codepoints: set when the block holds any",
         "/// entry of the four lookback tables `ClusterBreak::advance` consults.",
         "///",
         "/// A clear bit means the codepoint is in none of them, so all four",
         "/// searches are skipped -- `advance` ran two of them unconditionally for",
         f"/// every codepoint above `U+00A9`. {lb_set} of {LB_N} blocks are set.",
         f"pub static LOOKBACK_BLOCKS: [u64; {len(lb)}] = ["]
lines += [f"    0x{w:016x}," for w in lb]
lines.append("];")
parts.append("\n".join(lines))

parts.append(emit_ranges("EXTENDED_PICTOGRAPHIC", ext["Extended_Pictographic"],
    "/// `Extended_Pictographic`, from emoji-data.txt. GB11 needs it: an emoji\n"
    "/// joined by ZWJ to another emoji is **one** cluster."))
parts.append(emit_ranges("INCB_LINKER", incb_raw.get("Linker", []),
    "/// `InCB=Linker` — the virama. GB9c, added in Unicode 15.1, keeps an Indic\n"
    "/// conjunct together."))
parts.append(emit_ranges("INCB_CONSONANT", incb_raw.get("Consonant", []),
    "/// `InCB=Consonant`. GB9c."))
parts.append(emit_ranges("INCB_EXTEND", incb_raw.get("Extend", []),
    "/// `InCB=Extend`. GB9c."))

out = "\n\n".join(parts) + "\n"
target = pathlib.Path("baud-domain/src/grapheme_data.rs")
if "--check" in sys.argv:
    if target.read_text() != out:
        print("STALE"); sys.exit(1)
    print("OK")
else:
    target.write_text(out)
    print(f"break_property={len(rows)} extpict={len(coalesce(ext['Extended_Pictographic']))} "
          f"linker={len(coalesce(incb_raw.get('Linker', [])))} "
          f"consonant={len(coalesce(incb_raw.get('Consonant', [])))} "
          f"incb_extend={len(coalesce(incb_raw.get('Extend', [])))} bytes={len(out)}")
