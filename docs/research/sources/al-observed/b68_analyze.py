"""The flash census of the O2 build (lane b68), read back: the base's
reproduction of the committed census, the FAIL counts per size on both
builds and both paths, the pixel check, and the frame hashes below the
gate. Run from this directory: python3 b68_analyze.py"""
import collections, csv, gzip, io, os

HERE = os.path.dirname(os.path.abspath(__file__))
KEY = ["pt", "scale", "rate", "ground", "ink", "arm", "motion"]


def load(path):
    opener = gzip.open if path.endswith(".gz") else open
    rows, ctl = {}, []
    with opener(path, "rt") as fh:
        for r in csv.DictReader(fh, delimiter="\t"):
            if r.get("ms") is None:
                continue
            k = tuple(r[c] for c in KEY)
            (ctl.append(r) if r["arm"].startswith("block") else rows.__setitem__(k, r))
    return rows, ctl


def summary(rows, label):
    by = collections.defaultdict(lambda: [0] * 7)
    for k, r in rows.items():
        s = by[(k[0], k[1])]
        s[0] += 1
        s[1] += r["outcome"] == "FAIL"
        s[2] += r["apart_outcome"] == "FAIL"
        s[3] += r["strict_outcome"] == "FAIL"
        s[4] += r["apart_strict_outcome"] == "FAIL"
        if "under_over" in r:
            s[5] += int(r["under_over"]) > 0 or int(r["under_red"]) > 0
            s[6] += int(r["over_cap"]) > 0
    print(f"{label}: size, cases, FAIL drawn, FAIL apart, strict, strict apart, pixel check fails, over the cap")
    for z in sorted(by, key=lambda z: (int(z[1]), float(z[0]))):
        print("  ", f"{z[0]}x{z[1]}", *by[z])


def controls(ctl, label):
    bad = [r for r in ctl if (r["arm"] == "block-4hz") != (r["outcome"] == "FAIL")]
    print(f"{label}: {len(ctl)} control rows, {len(bad)} wrong")


runs = {t: load(os.path.join(HERE, f"b68-{t}-census.tsv.gz")) for t in ["base", "o2", "cpu-base", "cpu-o2"]}
com, _ = load(os.path.join(HERE, "..", "aj-observed", "flash-census.tsv"))
base = runs["base"][0]
cols = [c for c in next(iter(com.values())) if c not in KEY and c != "ms"]
shared = [k for k in com if k in base]
diff = [k for k in shared if any(base[k][c] != com[k][c] for c in cols)]
print(f"committed census {len(com)} rows; the base run has {len(shared)}; {len(diff)} differ on {len(cols)} columns (ms left out)")
for t, (rows, ctl) in runs.items():
    summary(rows, t)
    controls(ctl, t)
for b, o in [("base", "o2"), ("cpu-base", "cpu-o2")]:
    for pt in ["13", "24"]:
        ks = [k for k in runs[o][0] if k[0] == pt and k in runs[b][0]]
        eq = sum(runs[o][0][k]["fnv"] == runs[b][0][k]["fnv"] for k in ks)
        print(f"{o} against {b} at {pt} pt: {eq} of {len(ks)} frame hashes equal")
    rep = [r for r in runs[o][0].values() if int(r["repeat"]) > 0]
    print(f"{o}: {len(rep)} cases with repeat frames, largest conditioned change {max(float(r['under_dl']) for r in rep):.4f}")
