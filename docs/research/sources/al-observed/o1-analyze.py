#!/usr/bin/env python3
"""Prototype O1's census, read: the gates, then base against the options.

Reads proto-O1/runs/*.tsv (the flash bench's output, one file a run, named
<arm>-<tag>.tsv with arm base, o1 or o1-29) and the committed census
(docs/research/sources/aj-observed/flash-census.tsv, at c00dfd1) as the base
for the cases a base run did not repeat. Writes results.tsv (one row a case,
base beside each option) and summary.tsv (worst per arm x size x rate x
ground), and prints the gates.

Usage: python3 o1-analyze.py <census tsv> [<proto-O1 dir>]; the prototype's
directory, which holds runs/ and receives the two tables, defaults to
proto-O1 under the working directory.
"""

import csv
import glob
import os
import sys
from collections import defaultdict

CENSUS = sys.argv[1] if len(sys.argv) > 1 else None
HERE = sys.argv[2] if len(sys.argv) > 2 else "proto-O1"
THRESHOLD = 21824.0

KEYCOLS = ["pt", "scale", "rate", "ground", "ink", "arm", "motion"]
SKIP = {"ms", "fnv"}


def variant_of(name):
    base = os.path.basename(name)
    if base.startswith("o1-29-"):
        return "o1-29"
    if base.startswith("o1-"):
        return "o1"
    if base.startswith("base-"):
        return "base"
    raise ValueError(name)


def load(path):
    with open(path) as f:
        rows = list(csv.DictReader(f, delimiter="\t"))
    return rows


def key(r):
    return tuple(r[c] for c in KEYCOLS)


runs = defaultdict(dict)  # variant -> key -> row
hashruns = defaultdict(dict)  # file tag -> key -> fnv
errors = []
for path in sorted(glob.glob(f"{HERE}/runs/*.tsv")):
    if os.path.basename(path).startswith("png-"):
        continue
    v = variant_of(path)
    err = path[:-4] + ".err"
    with open(err) as f:
        tail = f.read().strip().splitlines()
    if not tail or tail[-1] != "exit 0":
        errors.append((os.path.basename(path), tail[-1] if tail else "empty"))
    for r in load(path):
        k = key(r)
        if r.get("fnv"):
            hashruns[os.path.basename(path)][k] = r["fnv"]
        if k in runs[v]:
            # A repeat (the hash control): keep the first, check it agrees.
            a, b = runs[v][k], r
            diff = [c for c in a if c not in SKIP and a[c] != b.get(c)]
            if diff:
                print(f"REPEAT DIFFERS {v} {k}: {diff}")
            continue
        runs[v][k] = r

print("== runs that did not exit 0:", errors or "none")

census = {}
if CENSUS:
    for r in load(CENSUS):
        census[key(r)] = r

# Gate: the bench's own controls, every size and run.
for v, rows in runs.items():
    bad = [
        k
        for k, r in rows.items()
        if r["arm"].startswith("block")
        and (r["outcome"] == "FAIL") != (r["arm"] == "block-4hz")
    ]
    print(f"== controls {v}: {'ok' if not bad else bad}")

# Gate: the census row.
g = runs["base"].get(("32", "1", "120", "light", "black", "fire", "every-100ms-diag"))
if g:
    print(
        "== gate fire 32x1 120 black light every-100ms-diag:",
        g["area"],
        g["outcome"],
        "apart",
        g["apart"],
    )

# Gate: the hash's control, base twice at 13x1.
a = hashruns.get("base-13x1-60-hashA.tsv", {})
b = hashruns.get("base-13x1-60-hashB.tsv", {})
o = hashruns.get("o1-13x1-60-hash.tsv", {})
both = set(a) & set(b)
same = sum(a[k] == b[k] for k in both)
print(f"== hash control base A vs B at 13x1: {same}/{len(both)} equal")
both = set(a) & set(o)
same = [k for k in both if a[k] == o[k]]
diff = [k for k in both if a[k] != o[k]]
print(f"== 13x1 fnv base vs o1: {len(same)}/{len(both)} equal; differ: {sorted(diff)[:10]}")
o29 = {}
for name, h in hashruns.items():
    if name.startswith("o1-29-13x1"):
        o29.update(h)
both = set(a) & set(o29)
diff = [k for k in both if a[k] != o29[k]]
print(f"== 13x1 fnv base vs o1-29: {len(both) - len(diff)}/{len(both)} equal; differ: {sorted(diff)[:10]}")

# Gate: the base runs against the committed census, every column but ms.
over = [k for k in runs["base"] if k in census]
mism = []
for k in over:
    r, c = runs["base"][k], census[k]
    d = [col for col in c if col not in SKIP and c[col] != r.get(col)]
    if d:
        mism.append((k, d))
print(f"== base runs vs the committed census: {len(over) - len(mism)}/{len(over)} identical")
for k, d in mism[:10]:
    print("   differs", k, d)


def base_row(k):
    return runs["base"].get(k) or census.get(k)


FIELDS = [
    "area",
    "apart",
    "strict",
    "apart_strict",
    "outcome",
    "apart_outcome",
    "strict_outcome",
    "apart_strict_outcome",
    "gen_flashes",
    "red_flashes",
    "moves",
    "vfx",
    "ignitions",
    "off_colour",
    "translucent",
]

variants = [v for v in ("o1", "o1-29") if v in runs]
keys = sorted(
    {k for v in variants for k in runs[v] if not k[5].startswith("block")},
    key=lambda k: (int(k[0]), int(k[1]), int(k[2]), k[3], k[4], k[5], k[6]),
)

with open(f"{HERE}/results.tsv", "w") as f:
    w = csv.writer(f, delimiter="\t", lineterminator="\n")
    head = KEYCOLS[:]
    for v in ["base"] + variants:
        head += [f"{v}_{c}" for c in FIELDS]
    w.writerow(head)
    for k in keys:
        row = list(k)
        for v in ["base"] + variants:
            r = base_row(k) if v == "base" else runs[v].get(k)
            row += [r[c] if r else "" for c in FIELDS]
        w.writerow(row)

# Moves, effect frames and ignitions: what the driver did, base against o1.
for v in variants:
    dd = []
    for k in keys:
        r, b = runs[v].get(k), base_row(k)
        if r and b:
            d = [c for c in ("moves", "vfx", "ignitions") if r[c] != b[c]]
            if d:
                dd.append((k, {c: (b[c], r[c]) for c in d}))
    n = sum(1 for k in keys if runs[v].get(k) and base_row(k))
    print(f"== {v}: moves/vfx/ignitions differ from base on {len(dd)}/{n} cases")
    by = defaultdict(int)
    for k, d in dd:
        by[(k[0], k[5], tuple(sorted(d)))] += 1
    for x, c in sorted(by.items()):
        print("   ", x, c)
    bad = [k for k in keys if runs[v].get(k) and (runs[v][k]["off_colour"] != "0" or runs[v][k]["translucent"] != "0")]
    print(f"== {v}: cases with off_colour or translucent pixels: {len(bad)}")

# The summary: worst per arm x size x rate x ground.
groups = defaultdict(list)
for k in keys:
    groups[(k[5], k[0], k[1], k[2], k[3])].append(k)


def worst(rows):
    rows = [r for r in rows if r]
    if not rows:
        return None
    fl = lambda r: max(int(r["gen_flashes"]), int(r["red_flashes"]))
    return {
        "n": len(rows),
        "area": max(float(r["area"]) for r in rows),
        "apart": max(float(r["apart"]) for r in rows),
        "fail": sum(r["outcome"] == "FAIL" for r in rows),
        "fail_apart": sum(r["apart_outcome"] == "FAIL" for r in rows),
        "strict_fail": sum(r["strict_outcome"] == "FAIL" for r in rows),
        "apart_strict_fail": sum(r["apart_strict_outcome"] == "FAIL" for r in rows),
        "flashes": max(fl(r) for r in rows),
    }


ARMS = ["off", "fire", "orb", "plasma", "ufo", "liquid", "ice", "lightning", "quad", "sweep"]
with open(f"{HERE}/summary.tsv", "w") as f:
    w = csv.writer(f, delimiter="\t", lineterminator="\n")
    cols = ["n", "area", "apart", "fail", "fail_apart", "strict_fail", "apart_strict_fail", "flashes"]
    head = ["arm", "pt", "scale", "rate", "ground"]
    for v in ["base"] + variants:
        head += [f"{v}_{c}" for c in cols]
    w.writerow(head)
    for g in sorted(
        groups, key=lambda g: (ARMS.index(g[0]) if g[0] in ARMS else 99, int(g[1]), int(g[2]), int(g[3]), g[4])
    ):
        row = list(g)
        for v in ["base"] + variants:
            ws = worst([base_row(k) if v == "base" else runs[v].get(k) for k in groups[g]])
            row += [("" if ws is None else (f"{ws[c]:.0f}" if isinstance(ws[c], float) else ws[c])) for c in cols]
        w.writerow(row)

# Totals per variant per size.
for v in ["base"] + variants:
    tot = defaultdict(lambda: [0, 0, 0, 0.0, 0.0, 0, 0])
    for k in keys:
        r = base_row(k) if v == "base" else runs[v].get(k)
        if not r:
            continue
        t = tot[(k[0], k[1], k[2])]
        t[0] += 1
        t[1] += r["outcome"] == "FAIL"
        t[2] += r["apart_outcome"] == "FAIL"
        t[3] = max(t[3], float(r["area"]))
        t[4] = max(t[4], float(r["apart"]))
        t[5] += r["strict_outcome"] == "FAIL"
        t[6] += r["apart_strict_outcome"] == "FAIL"
    for s, t in sorted(tot.items(), key=lambda x: tuple(int(y) for y in x[0])):
        print(f"   {v:6} {s[0]}x{s[1]}@{s[2]}: {t[0]} cases, FAIL drawn {t[1]}, FAIL apart {t[2]}, worst drawn {t[3]:.0f}, worst apart {t[4]:.0f}, strict FAIL {t[5]}, apart_strict FAIL {t[6]}")
