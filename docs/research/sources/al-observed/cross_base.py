#!/usr/bin/env python3
"""Note AL's cross-check of the four prototypes' base rows.

    python3 -I cross_base.py [DIR] [CENSUS]

DIR holds o1- to o4-census.tsv (default: this script's directory); CENSUS is
the committed census, docs/research/sources/aj-observed/flash-census.tsv
(default: found from this script's place in the repo, or in .claude/).

Each prototype built its own base binary (c00dfd1 plus the harness) and ran
its own base census. The O2 and O4 prototypes built into one shared scratch
bin/, whose flash-base was overwritten (its last write, 20:35:32 on 2026-10-09,
carries O2's harness: gates-out.txt section 5), so a base run could have run
another prototype's binary. This joins every base row on
(effect, pt, scale, rate, ground, ink, motion) across the four files and the
committed census, compares the verdict columns, and compares the frame hashes
(fnv) where two files carry them. Any disagreement is printed.
"""
import csv
import os
import sys
from collections import defaultdict
from itertools import combinations

HERE = os.path.dirname(os.path.abspath(__file__))
DIR = sys.argv[1] if len(sys.argv) > 1 else HERE
CENSUS = sys.argv[2] if len(sys.argv) > 2 else None
if CENSUS is None:
    for c in (os.path.join(HERE, "..", "aj-observed", "flash-census.tsv"),
              os.path.join(HERE, "..", "..", "..", "..", "..", "..", "docs", "research", "sources", "aj-observed", "flash-census.tsv")):
        if os.path.exists(c):
            CENSUS = c
            break

COLS = ["area", "apart", "strict", "apart_strict", "outcome", "apart_outcome",
        "strict_outcome", "apart_strict_outcome", "gen_flashes", "red_flashes"]


def rows(path, eff, ink, prefix):
    out = {}
    with open(path, newline="") as f:
        for r in csv.DictReader(f, delimiter="\t"):
            if prefix and not (r.get(f"{prefix}area") or "").strip():
                continue
            k = (r[eff], r["pt"], r["scale"], r["rate"], r["ground"], r[ink], r["motion"])
            # "-" is how the bench prints a column it did not fill (no
            # FLASH_HASH): absent, not a value
            v = {c: (r.get(f"{prefix}{c}") or "").strip().replace("-", "") if c == "fnv"
                 else (r.get(f"{prefix}{c}") or "").strip() for c in COLS + ["fnv"]}
            out[k] = v
    return out


src = {
    "O1": rows(os.path.join(DIR, "o1-census.tsv"), "arm", "ink", "base_"),
    "O2": rows(os.path.join(DIR, "o2-census.tsv"), "arm", "ink", "base_"),
    "O3": rows(os.path.join(DIR, "o3-census.tsv"), "effect", "ink", "base_"),
    "O4": rows(os.path.join(DIR, "o4-census.tsv"), "effect", "colour", "base_"),
}
if CENSUS:
    src["census"] = rows(CENSUS, "arm", "ink", "")
print("base rows: " + ", ".join(f"{k} {len(v)}" for k, v in src.items()))

bad = 0
for a, b in combinations(src, 2):
    shared = set(src[a]) & set(src[b])
    if not shared:
        print(f"{a} vs {b}: no shared case")
        continue
    cols = [c for c in COLS if all(src[a][k][c] and src[b][k][c] for k in shared)]
    diff = [k for k in shared if any(src[a][k][c] != src[b][k][c] for c in cols)]
    fnv = [k for k in shared if src[a][k]["fnv"] and src[b][k]["fnv"]]
    fdiff = [k for k in fnv if src[a][k]["fnv"] != src[b][k]["fnv"]]
    print(f"{a} vs {b}: {len(shared)} shared cases, {len(shared) - len(diff)} equal on {len(cols)} verdict columns"
          f"; fnv on {len(fnv)}, {len(fnv) - len(fdiff)} equal")
    for k in sorted(diff)[:10]:
        d = {c: (src[a][k][c], src[b][k][c]) for c in cols if src[a][k][c] != src[b][k][c]}
        print(f"   DIFFERS {k} {d}")
    for k in sorted(fdiff)[:10]:
        print(f"   FNV DIFFERS {k}")
    bad += len(diff) + len(fdiff)

# every prototype's base row against at least one other source
for name, rs in src.items():
    if name == "census":
        continue
    others = set().union(*(set(v) for n, v in src.items() if n != name))
    alone = [k for k in rs if k not in others]
    print(f"{name}: {len(rs) - len(alone)} of {len(rs)} base rows are also in another source")
sys.exit(1 if bad else 0)
