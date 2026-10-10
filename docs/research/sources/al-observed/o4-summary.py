"""Collate every run into one TSV a case (both arms side by side) and a
markdown summary per effect x size x rate.

usage: python3 -I summary.py <runs dir> <out dir>
"""

import collections
import glob
import os
import sys

runs, out = sys.argv[1], sys.argv[2]
PREFIXES = ["min", "hz120", "twox", "twox120", "rest"]
T = 21824.0

rows = {"base": {}, "o4": {}}
cells = {}
head = None
for prefix in PREFIXES:
    for arm in rows:
        for p in sorted(glob.glob(os.path.join(runs, f"{prefix}-{arm}-*.tsv"))):
            for line in open(p[:-4] + ".err"):
                if line.startswith("size "):
                    a = line.split()
                    cells[(a[1], a[3].rstrip(":").lstrip("x"))] = float(a[5].split("x")[1]) / float(a[3].rstrip(":").lstrip("x"))
            with open(p) as f:
                h = f.readline().rstrip("\n").split("\t")
                head = head or h
                for line in f:
                    r = dict(zip(h, line.rstrip("\n").split("\t")))
                    if r["arm"].startswith("block-"):
                        continue
                    k = (r["arm"], r["pt"], r["scale"], r["rate"], r["ground"], r["ink"], r["motion"])
                    rows[arm][k] = r

order = {"fire": 0, "orb": 1, "plasma": 2, "lightning": 3, "ice": 4, "ufo": 5, "liquid": 6, "quad": 7, "sweep": 8, "off": 9}
keys = sorted(set(rows["base"]) | set(rows["o4"]),
              key=lambda k: (order[k[0]], float(k[1]), k[2], int(k[3]), k[4], k[5], k[6]))

cols = ["area", "outcome", "apart", "apart_outcome", "strict", "strict_outcome",
        "apart_strict", "apart_strict_outcome", "gen_flashes", "red_flashes", "fnv"]
with open(os.path.join(out, "results.tsv"), "w") as w:
    w.write("\t".join(["effect", "pt", "scale", "cell_pt", "rate", "ground", "colour", "motion"]
                      + [f"base_{c}" for c in cols] + ["base_flashes_per_s"]
                      + [f"o4_{c}" for c in cols] + ["o4_flashes_per_s", "o4_hash_equals"]) + "\n")
    for k in keys:
        b, o = rows["base"].get(k), rows["o4"].get(k)
        cell = cells.get((k[1], k[2]), "")

        def part(r):
            if r is None:
                return [""] * (len(cols) + 1)
            return [r[c] for c in cols] + [str(max(int(r["gen_flashes"]), int(r["red_flashes"])))]

        # What the O4 case's frames must equal: the base's at or below the
        # gate and for the off arm; the O4 off arm's above it.
        eq = ""
        if o is not None:
            if cell != "" and cell > 32.0 and k[0] != "off":
                off = rows["o4"].get(("off",) + k[1:])
                if off is not None:
                    eq = "=off" if off["fnv"] == o["fnv"] else "off DIFFERS"
            elif b is not None:
                eq = "=base" if b["fnv"] == o["fnv"] else "base DIFFERS"
        w.write("\t".join([k[0], k[1], k[2], str(cell), k[3], k[4], k[5], k[6]] + part(b) + part(o) + [eq]) + "\n")

# Summary: effect x size x rate.
agg = collections.OrderedDict()
for k in keys:
    g = (k[0], f"{k[1]}x{k[2]}", k[3])
    a = agg.setdefault(g, {"base": None, "o4": None})
    for arm in ("base", "o4"):
        r = rows[arm].get(k)
        if r is None:
            continue
        s = a[arm] or {"n": 0, "fail": 0, "sfail": 0, "area": 0.0, "apart": 0.0, "strict": 0.0, "fl": 0, "worst": ""}
        s["n"] += 1
        s["fail"] += int("FAIL" in (r["outcome"], r["apart_outcome"]))
        s["sfail"] += int("FAIL" in (r["strict_outcome"], r["apart_strict_outcome"]))
        if float(r["area"]) >= s["area"]:
            s["area"] = float(r["area"])
            s["worst"] = f"{r['ground']} {r['ink']} {r['motion']}"
        s["apart"] = max(s["apart"], float(r["apart"]))
        s["strict"] = max(s["strict"], float(r["strict"]))
        s["fl"] = max(s["fl"], int(r["gen_flashes"]), int(r["red_flashes"]))
        a[arm] = s

sizes = sorted({g[1] for g in agg}, key=lambda s: (float(s.split("x")[0]), s))
with open(os.path.join(out, "summary.md"), "w") as w:
    w.write("| effect | size | cell pt | Hz | base: FAIL/cases | base max area / apart / strict | base fl/s | O4: FAIL/cases | O4 max area / apart / strict | O4 fl/s | O4 worst case |\n")
    w.write("|---|---|---|---|---|---|---|---|---|---|---|\n")
    for g, a in sorted(agg.items(), key=lambda kv: (order[kv[0][0]], float(kv[0][1].split("x")[0]), kv[0][1], int(kv[0][2]))):
        pt, sc = g[1].split("x")
        cell = cells.get((pt, sc), "")
        def fmt(s):
            if s is None:
                return ["-", "-", "-"]
            return [f"{s['fail']}/{s['n']}", f"{s['area']:,.0f} / {s['apart']:,.0f} / {s['strict']:,.0f}", str(s["fl"])]
        b, o = fmt(a["base"]), fmt(a["o4"])
        worst = a["o4"]["worst"] if a["o4"] else ""
        w.write(f"| {g[0]} | {g[1]} | {cell} | {g[2]} | " + " | ".join(b + o) + f" | {worst} |\n")
print(open(os.path.join(out, "summary.md")).read())
