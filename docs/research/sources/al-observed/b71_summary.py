"""b71_summary.py: the ghost arm's census, base against bound, per size,
scale and path, and the 13 and 24 pt frame hashes between the two.

Usage: python3 b71_summary.py b71-ghost-census.tsv.gz
"""
import collections
import gzip
import sys

rows = collections.defaultdict(dict)
with gzip.open(sys.argv[1], "rt") as f:
    hdr = f.readline().rstrip("\n").split("\t")
    for line in f:
        d = dict(zip(hdr, line.rstrip("\n").split("\t")))
        if d["arm"] != "ghost":
            continue
        k = (d["pt"], d["scale"], d["rate"], d["ground"], d["ink"], d["motion"], d["path"])
        rows[d["build"]][k] = d

for build in ("base", "bound"):
    g = collections.defaultdict(collections.Counter)
    for k, d in rows[build].items():
        c = g[(int(k[0]), k[1], k[6])]
        c["cases"] += 1
        c["fail"] += d["outcome"] == "FAIL"
        c["apart"] += d["apart_outcome"] == "FAIL"
        c["strict"] += d["strict_outcome"] == "FAIL"
        c["repeat"] += int(d["repeat"])
        c["over"] += int(d["under_over"])
        c["red"] += int(d["under_red"])
        c["cap"] += int(d["over_cap"])
        if d["outcome"] == "pass":
            c["worst"] = max(c["worst"], int(float(d["area"])))
    print(build)
    print("pt\tx\tpath\tcases\tFAIL drawn/apart\tstrict\tworst pass area\trepeat frames\tunder_over\tunder_red\tover_cap")
    for k in sorted(g):
        c = g[k]
        print(f"{k[0]}\t{k[1]}\t{k[2]}\t{c['cases']}\t{c['fail']}/{c['apart']}\t{c['strict']}\t{c['worst']}\t{c['repeat']}\t{c['over']}\t{c['red']}\t{c['cap']}")
base, bound = rows["base"], rows["bound"]
for pt in ("13", "24", "26", "32", "72"):
    ks = [k for k in bound if k[0] == pt and k in base]
    eq = sum(base[k]["fnv"] == bound[k]["fnv"] for k in ks)
    print(f"{pt} pt: {eq}/{len(ks)} frame hashes equal")
