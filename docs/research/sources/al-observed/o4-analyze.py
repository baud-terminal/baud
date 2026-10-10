"""Read proto-o4/runs/<prefix>-{base,o4}-*.tsv and check O4's acceptance.

usage: python3 -I analyze.py <runs dir> <prefix> [<prefix> ...]

Prints: per-arm FAIL counts by size and effect; the hash checks (base vs o4
below the gate, o4 trail arms vs o4 off arm above it, off vs off
everywhere); the worst case per effect x size per arm; and writes
<runs>/results-<prefix>.tsv with one row a case and both arms side by side.
"""

import collections
import glob
import os
import sys

GATE = 32.0
THRESH = 21824.0


def load(path):
    rows = []
    with open(path) as f:
        head = f.readline().rstrip("\n").split("\t")
        for line in f:
            v = line.rstrip("\n").split("\t")
            if len(v) != len(head):
                continue
            rows.append(dict(zip(head, v)))
    return rows


def key(r):
    return (r["arm"], r["pt"], r["scale"], r["rate"], r["ground"], r["ink"], r["motion"])


def main():
    runs = sys.argv[1]
    prefixes = sys.argv[2:]
    arms = {"base": {}, "o4": {}}
    cells = {}
    for prefix in prefixes:
        for arm in arms:
            for p in sorted(glob.glob(os.path.join(runs, f"{prefix}-{arm}-*.tsv"))):
                errp = p[:-4] + ".err"
                if os.path.exists(errp):
                    for line in open(errp):
                        if line.startswith("size "):
                            # size 24 pt x1: cell 14x32 px, ...
                            a = line.split()
                            pt, sc = a[1], a[3].rstrip(":").lstrip("x")
                            h = a[5].split("x")[1]
                            cells[(pt, sc)] = float(h) / float(sc)
                for r in load(p):
                    if r["arm"].startswith("block-"):
                        want = "FAIL" if r["arm"] == "block-4hz" else "pass"
                        assert r["outcome"] == want, (p, r["arm"], r["outcome"])
                        continue
                    arms[arm][key(r)] = r
    base, o4 = arms["base"], arms["o4"]
    print(f"cases: base {len(base)}, o4 {len(o4)}")
    print("cell heights (pt):", {f"{k[0]}x{k[1]}": v for k, v in sorted(cells.items())})

    # FAIL counts
    for name, d in arms.items():
        fails = collections.Counter()
        tot = collections.Counter()
        for k, r in d.items():
            sz = f"{k[1]}x{k[2]}@{k[3]}"
            tot[(sz, k[0])] += 1
            if "FAIL" in (r["outcome"], r["apart_outcome"]):
                fails[(sz, k[0])] += 1
        print(f"\n[{name}] FAIL (outcome or apart_outcome) / cases, by size@rate and arm:")
        sizes = sorted({s for s, _ in tot}, key=lambda s: (float(s.split("x")[0]), s))
        effs = sorted({e for _, e in tot})
        print("  size      " + " ".join(f"{e:>8}" for e in effs))
        for s in sizes:
            print(f"  {s:9} " + " ".join(
                f"{fails[(s, e)]:>3}/{tot[(s, e)]:<4}" if tot[(s, e)] else f"{'-':>8}" for e in effs))

    # hash checks
    print("\nhash checks:")
    below_eq = below_ne = 0
    above_eq = above_ne = 0
    off_eq = off_ne = 0
    ne_list = []
    for k, r in o4.items():
        cell = cells.get((k[1], k[2]))
        b = base.get(k)
        if k[0] == "off" and b is not None:
            if b["fnv"] == r["fnv"]:
                off_eq += 1
            else:
                off_ne += 1
                ne_list.append(("off-vs-off", k))
        if k[0] == "off":
            continue
        if cell is not None and cell <= GATE:
            if b is None:
                continue
            if b["fnv"] == r["fnv"]:
                below_eq += 1
            else:
                below_ne += 1
                ne_list.append(("below", k))
        elif cell is not None:
            off = o4.get(("off",) + k[1:])
            if off is None:
                continue
            if off["fnv"] == r["fnv"]:
                above_eq += 1
            else:
                above_ne += 1
                ne_list.append(("above", k))
    print(f"  below the gate, o4 == base: {below_eq} equal, {below_ne} differ")
    print(f"  above the gate, o4 trail == o4 off: {above_eq} equal, {above_ne} differ")
    print(f"  off arm, o4 == base: {off_eq} equal, {off_ne} differ")
    for x in ne_list[:20]:
        print("   ", x)

    # worst per effect x size
    for name, d in arms.items():
        worst = {}
        for k, r in d.items():
            g = (k[0], f"{k[1]}x{k[2]}")
            a = float(r["area"])
            if g not in worst or a > float(worst[g]["area"]):
                worst[g] = r
        print(f"\n[{name}] worst case per arm x size (area as drawn; apart; flashes/s):")
        for g in sorted(worst, key=lambda g: (g[0], float(g[1].split("x")[0]), g[1])):
            r = worst[g]
            fl = max(int(r["gen_flashes"]), int(r["red_flashes"]))
            print(f"  {g[0]:7} {g[1]:5} {r['rate']:>3} {r['ground']:5} {r['ink']:7} {r['motion']:17}"
                  f" area {float(r['area']):>7.0f} {r['outcome']:4} apart {float(r['apart']):>7.0f}"
                  f" {r['apart_outcome']:4} strict {float(r['strict']):>7.0f} fl/s {fl}")

    # side by side
    out = os.path.join(runs, f"results-{'-'.join(prefixes)}.tsv")
    cols = ["area", "apart", "strict", "apart_strict", "outcome", "apart_outcome",
            "strict_outcome", "apart_strict_outcome", "gen_flashes", "red_flashes", "vfx", "fnv"]
    with open(out, "w") as f:
        f.write("\t".join(["arm", "pt", "scale", "rate", "ground", "ink", "motion", "cell_pt"]
                          + [f"base_{c}" for c in cols] + [f"o4_{c}" for c in cols]) + "\n")
        for k in sorted(set(base) | set(o4), key=lambda k: (k[0], float(k[1]), k[2], k[3], k[4], k[5], k[6])):
            b, o = base.get(k, {}), o4.get(k, {})
            cell = cells.get((k[1], k[2]), "")
            f.write("\t".join(list(k) + [str(cell)] + [b.get(c, "") for c in cols]
                              + [o.get(c, "") for c in cols]) + "\n")
    print("\nwrote", out)


main()
