"""Analyse the O3 flash census: the base's gate against the shipped census,
the hashes, O3 against the replica's v4, and the per-case table.

python3 -I o3-analyse.py <runs dir> <census tsv> <replica out56.tsv> <out dir>
"""
import collections
import csv
import glob
import os
import sys

RUNS, CENSUS, REPLICA, OUT = sys.argv[1:5]
T = 21824.0
KEY = ("arm", "pt", "scale", "rate", "ground", "ink", "motion")


def load(tag):
    rows = {}
    files = sorted(glob.glob(os.path.join(RUNS, f"{tag}-*.tsv")))
    for fn in files:
        base = os.path.basename(fn)[: -len(".tsv")]
        done = os.path.exists(os.path.join(RUNS, base + ".done"))
        with open(fn) as f:
            for r in csv.DictReader(f, delimiter="\t"):
                if r.get("ms") is None:
                    continue
                r["pt"] = str(int(float(r["pt"])))
                rows[tuple(r[k] for k in KEY)] = r
    return rows, files


def flashes(r):
    return max(int(r["gen_flashes"]), int(r["red_flashes"]))


def short(k):
    return f"{k[0]} {k[1]}x{k[2]} {k[3]}Hz {k[5]}/{k[4]} {k[6]}"


base, bfiles = load("base")
o3, ofiles = load("o3")
aa, afiles = load("base-aa")
print(f"rows: base {len(base)} ({len(bfiles)} files), o3 {len(o3)} ({len(ofiles)} files), base-aa {len(aa)}")
done = sorted(os.path.basename(p) for p in glob.glob(os.path.join(RUNS, "*.done")))
print(f"done shards: {len(done)}")

# 1. The base against the shipped census, every column but ms (and fnv).
census = {}
with open(CENSUS) as f:
    for r in csv.DictReader(f, delimiter="\t"):
        r["pt"] = str(int(float(r["pt"])))
        census[tuple(r[k] for k in KEY)] = r
cols = [c for c in next(iter(census.values())).keys() if c not in ("ms",)]
n = same = 0
diffs = []
for k, c in census.items():
    if k not in base:
        continue
    n += 1
    b = base[k]
    d = [(x, b[x], c[x]) for x in cols if b[x] != c[x]]
    if d:
        diffs.append((k, d))
    else:
        same += 1
by_size = collections.Counter(k[1] + "x" + k[2] for k, c in census.items() if k in base)
print(f"\n[gate] base vs shipped census: {n} common rows {dict(by_size)}, {same} identical in every column but ms, {len(diffs)} differ")
for k, d in diffs[:10]:
    print("   differs", short(k), d)
g = ("fire", "32", "1", "120", "light", "black", "every-100ms-diag")
if g in base:
    print(f"[gate] base fire 32x1 120 black light every-100ms-diag: area {base[g]['area']} {base[g]['outcome']}, apart {base[g]['apart']}")
for k, r in base.items():
    if k[0].startswith("block"):
        want = "FAIL" if k[0] == "block-4hz" else "pass"
        assert r["outcome"] == want, (k, r["outcome"])

# 2. Hashes.
def hash_cmp(a, b, size, name):
    ks = [k for k in a if k[1] + "x" + k[2] == size and "fnv" in a[k] and a[k]["fnv"]]
    have = [k for k in ks if k in b and b[k].get("fnv")]
    eq = sum(a[k]["fnv"] == b[k]["fnv"] for k in have)
    neq = [k for k in have if a[k]["fnv"] != b[k]["fnv"]]
    print(f"[hash] {name} at {size}: {len(have)} cases compared, {eq} equal, {len(neq)} differ")
    for k in neq[:5]:
        print("   differs", short(k))
    return len(have), eq


print()
hash_cmp(base, aa, "13x1", "base vs base (A/A)")
hash_cmp(base, o3, "13x1", "base vs o3")
hash_cmp(base, o3, "24x1", "base vs o3")
hash_cmp(base, o3, "26x1", "base vs o3 (above the gate, expected to differ for effects and smears)")
# Above the gate the off arm must still hash equal.
ks = [k for k in base if k[1] == "26" and k[0] == "off" and k in o3 and base[k].get("fnv")]
print(f"[hash] off arm at 26x1: {sum(base[k]['fnv'] == o3[k]['fnv'] for k in ks)} of {len(ks)} equal")
# And whether every non-off 26x1 case differs or only some.
for arm in sorted({k[0] for k in base if k[1] == "26" and not k[0].startswith("block")}):
    ks = [k for k in base if k[1] == "26" and k[0] == arm and k in o3 and base[k].get("fnv")]
    if ks:
        print(f"   26x1 {arm:>9}: {sum(base[k]['fnv'] == o3[k]['fnv'] for k in ks)} of {len(ks)} equal")

# 3. Rollup per arm x size, both arms.
def rollup(rows, name):
    g = collections.defaultdict(list)
    for k, r in rows.items():
        if k[0].startswith("block"):
            continue
        g[(k[0], int(k[1]), k[2])].append((k, r))
    print(f"\n[{name}] arm  size | cases | FAIL drawn  FAIL apart | strict drawn  strict apart | worst drawn (case) | max flashes/s at a pixel")
    out = {}
    for (arm, pt, sc), rs in sorted(g.items(), key=lambda x: (x[0][1], x[0][0])):
        fd = sum(r["outcome"] == "FAIL" for _, r in rs)
        fa = sum(r["apart_outcome"] == "FAIL" for _, r in rs)
        sd = sum(r["strict_outcome"] == "FAIL" for _, r in rs)
        sa = sum(r["apart_strict_outcome"] == "FAIL" for _, r in rs)
        wk, wr = max(rs, key=lambda x: float(x[1]["area"]))
        mf = max(flashes(r) for _, r in rs)
        out[(arm, pt, sc)] = (len(rs), fd, fa, sd, sa, wk, wr, mf)
        print(f"  {arm:>9} {pt:>3}x{sc} | {len(rs):>5} | {fd:>10} {fa:>11} | {sd:>12} {sa:>13} | {float(wr['area']):>7,.0f} apart {float(wr['apart']):>7,.0f} ({wk[6]} {wk[5]}/{wk[4]} {wk[3]} Hz) | {mf}")
    return out


rb = rollup(base, "base")
ro = rollup(o3, "o3")

# 4. O3 against the replica's v4 on the matrix's keys.
v4 = {}
with open(REPLICA) as f:
    for line in f:
        p = line.rstrip("\n").split("\t")
        eff, var = (p[0].split("/") + ["shipped"])[:2]
        if var != "v4":
            continue
        d = dict(kv.split("=") for kv in p[8:])
        v4[(eff, p[1], p[2], p[3], p[4], p[5], p[6])] = d
n = same_verdict = 0
within = 0
worst = None
far = []
for k, d in v4.items():
    if k not in o3:
        continue
    n += 1
    r = o3[k]
    vv = (float(d["area"]) > T, float(d["apart"]) > T)
    ov = (r["outcome"] == "FAIL", r["apart_outcome"] == "FAIL")
    same_verdict += vv == ov
    dev = max(abs(float(r[c]) - float(d[c])) for c in ("area", "apart", "strict", "apart_strict"))
    exact = globals().setdefault("exact", [0])
    exact[0] += dev == 0
    if dev <= 11:
        within += 1
    else:
        far.append((dev, k, {c: (r[c], d[c]) for c in ("area", "apart", "strict", "apart_strict")}))
    if worst is None or float(r["area"]) > float(worst[1]["area"]):
        worst = (k, r)
print(f"\n[v4] o3 vs replica v4 on {n} of {len(v4)} matrix keys: {same_verdict} same verdict (drawn and apart), {within} within 11 pt^2 on area/apart/strict/apart_strict, {globals().get('exact', [0])[0]} exactly equal on all four")
if worst:
    print(f"[v4] o3's worst on those keys: {float(worst[1]['area']):,.0f} ({short(worst[0])}); v4's worst: " +
          max(((float(d['area']), k) for k, d in v4.items()), key=lambda x: x[0]).__repr__())
far.sort(reverse=True)
for dev, k, c in far[:12]:
    print(f"   off by {dev:.0f}: {short(k)} {c}")
for pt in ("32", "72"):
    ks = [k for k in v4 if k[1] == pt and k in o3]
    sd = sum(o3[k]["strict_outcome"] == "FAIL" for k in ks)
    fd = sum(o3[k]["outcome"] == "FAIL" for k in ks)
    print(f"[v4] o3 at {pt} pt on the matrix keys: {len(ks)} cases, {fd} FAIL drawn, {sd} strict FAIL (v4: 0 FAIL; strict 0 at 32, 54 at 72)")

# 5. Every O3 row at or above 80% of the threshold, and every FAIL.
print("\n[o3] every case at or above 80% of 21,824 (17,459), drawn or apart:")
hot = [(float(r["area"]), k, r) for k, r in o3.items() if not k[0].startswith("block") and max(float(r["area"]), float(r["apart"])) >= 0.8 * T]
for a, k, r in sorted(hot, reverse=True):
    print(f"   {a:>7,.0f} apart {float(r['apart']):>7,.0f} {r['outcome']}/{r['apart_outcome']} strict {float(r['strict']):,.0f} {short(k)}")
if not hot:
    print("   none")
fails = [k for k, r in o3.items() if not k[0].startswith("block") and (r["outcome"] == "FAIL" or r["apart_outcome"] == "FAIL")]
print(f"[o3] FAIL drawn or apart: {len(fails)}")
for k in fails[:40]:
    print("   ", short(k), o3[k]["area"], o3[k]["apart"])

# 6. The per-case table, side by side.
os.makedirs(OUT, exist_ok=True)
path = os.path.join(OUT, "results.tsv")
show = ["area", "apart", "outcome", "apart_outcome", "strict", "apart_strict", "strict_outcome", "apart_strict_outcome", "gen_flashes", "red_flashes", "vfx", "ignitions"]
with open(path, "w") as f:
    f.write("\t".join(["effect", "pt", "scale", "rate", "ground", "ink", "motion"] + [f"base_{c}" for c in show] + ["base_max_flashes_per_s"] + [f"o3_{c}" for c in show] + ["o3_max_flashes_per_s", "base_fnv", "o3_fnv"]) + "\n")
    for k in sorted(set(base) | set(o3), key=lambda k: (int(k[1]), k[2], k[0], k[3], k[4], k[5], k[6])):
        if k[0].startswith("block"):
            continue
        b, o = base.get(k), o3.get(k)
        row = list(k)
        row += [b[c] if b else "" for c in show] + [str(flashes(b)) if b else ""]
        row += [o[c] if o else "" for c in show] + [str(flashes(o)) if o else ""]
        row += [(b or {}).get("fnv", "") or "", (o or {}).get("fnv", "") or ""]
        f.write("\t".join(row) + "\n")
print(f"\n[table] {path}")

# 7. The rollup as a TSV too.
path = os.path.join(OUT, "rollup.tsv")
with open(path, "w") as f:
    f.write("arm\tpt\tscale\tbuild\tcases\tfail_drawn\tfail_apart\tstrict_fail_drawn\tstrict_fail_apart\tworst_area\tworst_apart\tworst_case\tmax_flashes_per_s\n")
    for name, rr in (("base", rb), ("o3", ro)):
        for (arm, pt, sc), (n_, fd, fa, sd, sa, wk, wr, mf) in sorted(rr.items(), key=lambda x: (x[0][1], x[0][0])):
            f.write(f"{arm}\t{pt}\t{sc}\t{name}\t{n_}\t{fd}\t{fa}\t{sd}\t{sa}\t{float(wr['area']):.0f}\t{float(wr['apart']):.0f}\t{wk[6]} {wk[5]}/{wk[4]} {wk[3]}Hz\t{mf}\n")
print(f"[table] {path}")
