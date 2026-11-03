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


# The four lookback tables, as bits 4-7 of each trie byte. Order is the
# contract with `ClusterBreak::Lookback` in grapheme.rs, and
# `the_break_trie_agrees_with_the_class_enum` pins it.
LOOKBACK = ["Extended_Pictographic", "Linker", "Consonant", "Extend"]


def build_break_trie(rows, ext, incb_raw):
    """One two-stage table carrying **both** the class and the lookback flags.

    Low four bits are the `GraphemeClass` discriminant, high four are membership
    of the four lookback tables in `LOOKBACK` order.

    They are together because they are needed together. `ClusterBreak` asked
    for the class, then tested a 32-codepoint bitmap to decide whether to
    consult the lookback tables, then binary searched up to four of them --
    `Extended_Pictographic` (156 ranges), `INCB_Consonant` (76), `INCB_Linker`
    (20), `INCB_Extend` (377). One indexed load now answers all of it, and a
    probe that *added* a whole extra table lookup to `Width::of` measured
    **flat**, so the loads were never the cost: the binary searches' branches
    were.

    It also makes `listed` exact rather than a 32-codepoint superset, so more
    codepoints reach the fast path.

    32,768 bytes against 21,032 -- the 16,000-byte class trie plus 5,032 of
    ranges -- and it retires the 4,352-byte bitmap as well.
    """
    idxof = {c: i for i, c in enumerate(CLASSES)}
    assert len(CLASSES) <= 16, "four bits per class"
    assert len(LOOKBACK) <= 4, "four bits per lookback set"
    cls = bytearray(0x110000)
    for lo, hi, k in rows:
        v = idxof[k]
        for cp in range(lo, min(hi, 0x10FFFF) + 1):
            cls[cp] = v

    flags = bytearray(0x110000)
    sets = {
        "Extended_Pictographic": ext.get("Extended_Pictographic", []),
        "Linker": incb_raw.get("Linker", []),
        "Consonant": incb_raw.get("Consonant", []),
        "Extend": incb_raw.get("Extend", []),
    }
    for bit, name in enumerate(LOOKBACK):
        for lo, hi in coalesce(sets[name]):
            for cp in range(lo, min(hi, 0x10FFFF) + 1):
                flags[cp] |= 1 << bit

    combined = bytes((cls[i] | (flags[i] << 4)) for i in range(0x110000))

    blocks, index = {}, []
    for base in range(0, 0x110000, BLOCK):
        key = combined[base : base + BLOCK]
        if key not in blocks:
            blocks[key] = len(blocks)
        index.append(blocks[key])
    assert len(blocks) <= 256, f"{len(blocks)} blocks will not fit a u8 stage 1"

    packed = bytearray()
    for key in blocks:
        packed.extend(key)

    # Exhaustive, against the ranges this file is generated from.
    for cp in range(0x110000):
        got = packed[index[cp >> 8] * BLOCK + (cp & 0xFF)]
        want = cls[cp] | (flags[cp] << 4)
        if got != want:
            raise SystemExit(f"trie disagrees at U+{cp:04X}: {got} != {want}")
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


bp_index, bp_packed, bp_blocks = build_break_trie(rows, ext, incb_raw)
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
        "/// **Low four bits** are `GraphemeClass` discriminants in declaration",
        "/// order; `the_break_trie_agrees_with_the_class_enum` pins that",
        "/// correspondence, because a reordered enum would remap every codepoint",
        "/// silently.",
        "///",
        "/// **High four bits** are membership of the four lookback tables, in",
        "/// the order `Extended_Pictographic`, `INCB_Linker`, `INCB_Consonant`,",
        "/// `INCB_Extend` -- see `ClusterBreak::Lookback`. They are in the same",
        "/// byte because they are needed in the same place: the cluster rules",
        "/// ask for the class and then, for the codepoints that matter, whether",
        "/// it is pictographic or an Indic consonant. That was a bitmap probe",
        "/// plus up to four binary searches.",
    ])))
parts.append(as_bytestring(
    "BREAK_BLOCKS", bp_packed,
    "/// Stage 2: four-bit class values, two per byte, low nibble first."))

# The lookback bitmap is gone: membership now rides in the high nibble of the
# class trie, exactly rather than as a 32-codepoint superset, so there is
# nothing left for a separate probe to answer.

# General category Mc that UAX #29 classes Extend: spacing marks with a
# positive advance (UAX #44) that GB9 joins in both variants.
mc = parse("unicode-derivedgeneralcategory.txt", ["Mc"])["Mc"]
extend = set()
for lo, hi in gbp["Extend"]:
    extend.update(range(lo, hi + 1))
spacing_extend = [(c, c) for lo, hi in mc for c in range(lo, hi + 1) if c in extend]
parts.append(emit_ranges(
    "SPACING_EXTEND", spacing_extend,
    "\n".join([
        "/// **The spacing marks UAX #29 classes Extend**: general category `Mc`",
        "/// (`[[source: gc-data]]`) and",
        "/// Grapheme_Cluster_Break `Extend` (`[[source: gbp-data]]`), such as",
        "/// U+09BE BENGALI VOWEL SIGN AA and U+0BBE TAMIL VOWEL SIGN AA, which GB9",
        "/// joins in both of `[[source: uax29]]`'s variants and which take a",
        "/// column while `?2027` is reset (`ClusterBreak::parts_at`). Sorted, for",
        "/// a binary search on the cold side of a join.",
    ])))


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
