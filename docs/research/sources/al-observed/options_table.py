#!/usr/bin/env python3
"""Note AL's results tables, built from the four prototypes' per-case census TSVs.

    python3 -I options_table.py [DIR]

DIR holds the four TSVs as staged beside this script (default: this script's
directory):

    o1-census.tsv   proto O1 (commit f5725ab): builds base, o1 (26 pt cap), o1-29 (29 pt cap)
    o2-census.tsv   proto O2 (commit 4ccd622): builds base, o2
    o3-census.tsv   proto O3 (commit dc2fce3): builds base, o3
    o4-census.tsv   proto O4 (commit 43b72d7): builds base, o4

Each row of a TSV is one case (effect x size x scale x rate x ground x ink x
motion) counted by the `flash` bench on this Mac's GPU, with one column group
per build. Nothing here re-measures anything: it groups and takes maxima, and
every figure it prints is scoped to the grid that option ran, which differs
between options (printed first).

Per build and size it prints: cases; FAIL as drawn and apart (area > 21,824
pt^2); FAIL on the strict (chained) reading, drawn and apart; the worst area
drawn and apart, with the case; and the most flashes in one second at any one
pixel (general or red), which includes the cursor's own cells.
"""
import csv
import os
import sys
from collections import defaultdict

DIR = sys.argv[1] if len(sys.argv) > 1 else os.path.dirname(os.path.abspath(__file__))

# file, builds, and how each file spells the common columns
SPECS = [
    ("O1", "o1-census.tsv", ["base", "o1", "o1-29"], {"effect": "arm", "ink": "ink"}),
    ("O2", "o2-census.tsv", ["base", "o2"], {"effect": "arm", "ink": "ink"}),
    ("O3", "o3-census.tsv", ["base", "o3"], {"effect": "effect", "ink": "ink"}),
    ("O4", "o4-census.tsv", ["base", "o4"], {"effect": "effect", "ink": "colour"}),
]


def num(s):
    s = (s or "").strip()
    if s in ("", "-"):
        return None
    return float(s)


def fps(row, b):
    """Most flashes in one second at any pixel, general or red."""
    for col in (f"{b}_max_flashes_per_s", f"{b}_flashes_per_s"):
        if col in row and num(row[col]) is not None:
            return int(num(row[col]))
    g, r = num(row.get(f"{b}_gen_flashes")), num(row.get(f"{b}_red_flashes"))
    if g is None and r is None:
        return None
    return int(max(g or 0, r or 0))


def load(fname, builds, names):
    out = []
    with open(os.path.join(DIR, fname), newline="") as f:
        for row in csv.DictReader(f, delimiter="\t"):
            key = dict(
                effect=row[names["effect"]], pt=row["pt"], scale=row["scale"], rate=row["rate"],
                ground=row["ground"], ink=row[names["ink"]], motion=row["motion"],
            )
            for b in builds:
                area = num(row.get(f"{b}_area"))
                if area is None:
                    continue  # this build did not run this case
                out.append(dict(
                    key, build=b, area=area, apart=num(row.get(f"{b}_apart")) or 0.0,
                    fail=row.get(f"{b}_outcome") == "FAIL",
                    fail_apart=row.get(f"{b}_apart_outcome") == "FAIL",
                    sfail=row.get(f"{b}_strict_outcome") == "FAIL",
                    sfail_apart=row.get(f"{b}_apart_strict_outcome") == "FAIL",
                    fps=fps(row, b),
                ))
    return out


def case(r):
    return f"{r['effect']} {r['pt']}x{r['scale']} {r['rate']}Hz {r['ink']}/{r['ground']} {r['motion']}"


def report(opt, rows):
    print(f"=== {opt}: {len(rows)} build-cases")
    grid = defaultdict(set)
    for r in rows:
        if r["build"] != "base":
            grid[(r["pt"], r["scale"], r["rate"])].add(r["ground"])
    for (pt, sc, rate), gs in sorted(grid.items(), key=lambda kv: (int(kv[0][0]), kv[0][1], kv[0][2])):
        n = sum(1 for r in rows if r["build"] != "base" and (r["pt"], r["scale"], r["rate"]) == (pt, sc, rate))
        print(f"  grid {pt}x{sc}@{rate}: grounds {','.join(sorted(gs))}; {n} option build-cases")
    by = defaultdict(list)
    for r in rows:
        by[(r["build"], int(r["pt"]), r["scale"])].append(r)
    print("  build  size  | cases | FAIL drawn/apart | strict drawn/apart | worst drawn / apart (case of the drawn) | max fl/s")
    for (b, pt, sc), rs in sorted(by.items(), key=lambda kv: (kv[0][0] != "base", kv[0][0], kv[0][1], kv[0][2])):
        w = max(rs, key=lambda r: r["area"])
        wa = max(r["apart"] for r in rs)
        f = [r["fps"] for r in rs if r["fps"] is not None]
        print(f"  {b:6} {pt:>2}x{sc} | {len(rs):5} | {sum(r['fail'] for r in rs):4} / {sum(r['fail_apart'] for r in rs):<4} |"
              f" {sum(r['sfail'] for r in rs):4} / {sum(r['sfail_apart'] for r in rs):<4} |"
              f" {int(w['area']):>6,} / {int(wa):>6,} ({case(w)}) | {max(f) if f else '-'}")
    # per effect above the option's gate (26, 32 and 72 pt, 1x), option builds only
    print("  per effect at 32 and 72 pt, 1x (option builds): FAIL drawn/apart, worst drawn/apart, max fl/s")
    pe = defaultdict(list)
    for r in rows:
        if r["scale"] == "1" and r["pt"] in ("32", "72"):
            pe[(r["build"], r["effect"], r["pt"])].append(r)
    for (b, e, pt), rs in sorted(pe.items()):
        f = [r["fps"] for r in rs if r["fps"] is not None]
        print(f"    {b:6} {e:9} {pt}: n={len(rs):4} FAIL {sum(r['fail'] for r in rs)}/{sum(r['fail_apart'] for r in rs)}"
              f" worst {int(max(r['area'] for r in rs)):,}/{int(max(r['apart'] for r in rs)):,}"
              f" strict {sum(r['sfail'] for r in rs)}/{sum(r['sfail_apart'] for r in rs)} fl/s {max(f) if f else '-'}")
    print()


for opt, fname, builds, names in SPECS:
    report(opt, load(fname, builds, names))
