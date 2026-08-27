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
//! data disagree.

use crate::GraphemeClass;''']

lines = ["/// `[[source: uax29]]` Grapheme_Cluster_Break, as sorted",
         "/// `(lo, hi, class)` ranges. Binary searched; anything not listed is",
         "/// [`GraphemeClass::Other`].",
         f"pub static BREAK_PROPERTY: [(u32, u32, GraphemeClass); {len(rows)}] = ["]
lines += [f"    (0x{a:04x}, 0x{b:04x}, GraphemeClass::{c})," for a, b, c in rows]
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
