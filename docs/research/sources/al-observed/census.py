#!/usr/bin/env python3
"""Read-only analysis of the flash census (docs/research/sources/aj-observed/
flash-census.tsv, the `flash` bench's output at d098cb7).

It does three things, in order, and prints each:

1. **Reproduces the record's tables** (docs/record.md, "The cursor trails'
   flashes, counted on the frames they draw"): table A's worst areas and
   failing counts, the 205 / 170 totals, and table C. If any of these is off,
   the grouping below is wrong and nothing after it should be read.
2. **Scaling with the cell**: for every case key (effect, motion, ground,
   ink, rate) the area as drawn and apart at 13, 32 and 72 pt, 1x, against
   the cell's height in points (17, 42, 95). Fits area = a * h (per point of
   cell height) and a power law area = b * h^p through the three sizes, on
   matched keys, so a worst-of-192 that comes from a different case at each
   size cannot fake a slope. Also prints where each effect's worst key
   would cross 21,824 if the slope held (a linear extrapolation, labelled).
3. **The light ground**: per effect and size, failing counts and the worst
   apart area by ground and ink, and the share of the light page's area that
   the dark page's equivalent (same effect/motion/rate, the theme's own
   default cursor) reaches.

Pure Python 3, no third-party modules. Usage, from the repository root:
python3 docs/research/sources/al-observed/census.py [path]
"""
import csv
import math
import sys
from collections import defaultdict

PATH = sys.argv[1] if len(sys.argv) > 1 else \
    "docs/research/sources/aj-observed/flash-census.tsv"
THRESHOLD = 0.25 * 341 * 256  # 21,824 points squared
# The cell, points, from the record's size table (cell px / scale).
CELL_H = {13: 17.0, 32: 42.0, 72: 95.0}
CELL_W = {13: 8.0, 32: 19.0, 72: 43.0}
EFFECTS = ["off", "fire", "plasma", "lightning", "ice", "ufo", "orb", "liquid"]
SIZES = [(13, 1), (13, 2), (32, 1), (32, 2), (72, 1)]
MOTIONS = ["long-diag", "every-100ms-diag", "every-200ms-diag",
           "long-across", "every-100ms-across", "every-200ms-across"]
GROUNDS = ["dark", "light", "red", "darkred"]
INKS = ["default", "white", "black", "red"]


def load(path):
    rows = []
    with open(path, newline="") as f:
        for r in csv.DictReader(f, delimiter="\t"):
            if r["arm"].startswith("block-"):
                continue  # the positive control, not a case
            r["pt"] = int(float(r["pt"]))
            r["scale"] = int(r["scale"])
            r["rate"] = int(r["rate"])
            for k in ("area", "apart", "strict", "apart_strict", "contiguous",
                      "gen_over", "gen_ever", "red_over", "red_ever"):
                r[k] = float(r[k])
            for k in ("gen_flashes", "red_flashes", "gen_trans", "ignitions",
                      "lit", "moves", "vfx"):
                r[k] = int(r[k])
            rows.append(r)
    return rows


def controls(path):
    out = []
    with open(path, newline="") as f:
        for r in csv.DictReader(f, delimiter="\t"):
            if r["arm"].startswith("block-"):
                out.append(r)
    return out


def section(title):
    print()
    print("=" * 78)
    print(title)
    print("=" * 78)


def table_a(rows):
    section("1a. Table A: worst area as drawn (apart), failing as drawn / apart")
    by = defaultdict(list)
    for r in rows:
        by[(r["arm"], r["pt"], r["scale"])].append(r)
    head = "effect".ljust(10) + "".join(f"{p}pt{s}x".rjust(31) for p, s in SIZES)
    print(head)
    for e in EFFECTS:
        line = e.ljust(10)
        for p, s in SIZES:
            g = by.get((e, p, s), [])
            if not g:
                line += "-".rjust(31)
                continue
            wa = max(r["area"] for r in g)
            wp = max(r["apart"] for r in g)
            fa = sum(r["outcome"] == "FAIL" for r in g)
            fp = sum(r["apart_outcome"] == "FAIL" for r in g)
            line += f"{wa:,.0f} ({wp:,.0f}), {fa}/{fp} n={len(g)}".rjust(31)
        print(line)


def totals(rows):
    section("1b. Totals and table C (failing as drawn / apart, by ink x ground)")
    fa = sum(r["outcome"] == "FAIL" for r in rows)
    fp = sum(r["apart_outcome"] == "FAIL" for r in rows)
    sa = sum(r["strict_outcome"] == "FAIL" for r in rows)
    sp = sum(r["apart_strict_outcome"] == "FAIL" for r in rows)
    print(f"cases {len(rows)}; failing as drawn {fa}, apart {fp}; strict {sa} / {sp}")
    print("ink \\ ground".ljust(14) + "".join(g.rjust(12) for g in GROUNDS))
    for i in INKS:
        line = i.ljust(14)
        for g in GROUNDS:
            sub = [r for r in rows if r["ink"] == i and r["ground"] == g]
            a = sum(r["outcome"] == "FAIL" for r in sub)
            p = sum(r["apart_outcome"] == "FAIL" for r in sub)
            line += f"{a} / {p}".rjust(12)
        print(line)
    # Which failures are general-only vs red-only: the record's 166/23/15/1.
    gen = red = both = neither = 0
    for r in rows:
        if r["outcome"] != "FAIL":
            continue
        g, d = r["gen_over"] > THRESHOLD, r["red_over"] > THRESHOLD
        if g and not d:
            gen += 1
        elif d and not g:
            red += 1
        elif g and d:
            both += 1
        else:
            neither += 1
    print(f"failing as drawn: general alone {gen}, red alone {red}, either alone {both}, "
          f"only together {neither}")


def fit_line_through_origin(xs, ys):
    sxx = sum(x * x for x in xs)
    sxy = sum(x * y for x, y in zip(xs, ys))
    a = sxy / sxx
    resid = max(abs(y - a * x) / y for x, y in zip(xs, ys) if y > 0) if any(ys) else 0.0
    return a, resid


def fit_power(xs, ys):
    pts = [(math.log(x), math.log(y)) for x, y in zip(xs, ys) if y > 0]
    if len(pts) < 2:
        return float("nan"), float("nan")
    n = len(pts)
    mx = sum(p[0] for p in pts) / n
    my = sum(p[1] for p in pts) / n
    sxx = sum((p[0] - mx) ** 2 for p in pts)
    sxy = sum((p[0] - mx) * (p[1] - my) for p in pts)
    p = sxy / sxx
    b = math.exp(my - p * mx)
    return b, p


def scaling(rows):
    section("2. Scaling with the cell: matched keys at 13/32/72 pt, 1x")
    print("Cell height h in points: 13 pt -> 17, 32 pt -> 42, 72 pt -> 95.")
    print("At 72 pt the paths (344 x 285 pt diagonal, 344 pt across) are larger than")
    print("the 341 x 256 window, so every 72 pt area is window-bounded: a slope read")
    print("through it under-states what an unbounded path would flash.")
    idx = {}
    for r in rows:
        if r["scale"] != 1:
            continue
        key = (r["arm"], r["motion"], r["ground"], r["ink"], r["rate"])
        idx[(key, r["pt"])] = r
    keys = sorted({k for (k, _) in idx})
    hs = [CELL_H[13], CELL_H[32], CELL_H[72]]
    for col in ("area", "apart"):
        print()
        print(f"--- column `{col}`: per effect, the key worst at 32 pt, and the spread of")
        print("    area/h over every key whose 32 pt area is at least 5,000 (so the")
        print("    cursor-cell-only and near-empty keys do not dominate)")
        print("effect".ljust(9) + "worst key at 32 pt".ljust(44) +
              "13/32/72 pt area".rjust(24) + "  area/h at 13/32/72".rjust(22) +
              "  power p".rjust(9) + "  cross h, pt-font")
        for e in EFFECTS[1:]:
            ks = [k for k in keys if k[0] == e and all((k, p) in idx for p in (13, 32, 72))]
            if not ks:
                continue
            worst = max(ks, key=lambda k: idx[(k, 32)][col])
            ys = [idx[(worst, p)][col] for p in (13, 32, 72)]
            per = [y / h for y, h in zip(ys, hs)]
            b, p = fit_power(hs, ys)
            # Linear-in-h crossing from the 13 and 32 pt points (both inside
            # the window), as a slope through the origin of those two.
            a2, _ = fit_line_through_origin(hs[:2], ys[:2])
            cross_h = THRESHOLD / a2 if a2 > 0 else float("inf")
            cross_pt = cross_h / (17.0 / 13.0)
            label = f"{worst[1]} {worst[3]}/{worst[2]} {worst[4]}Hz"
            print(e.ljust(9) + label.ljust(44) +
                  ("%s" % "/".join(f"{y:,.0f}" for y in ys)).rjust(24) +
                  ("  " + "/".join(f"{v:.0f}" for v in per)).rjust(22) +
                  f"  {p:7.2f}" + f"  h={cross_h:5.1f} (~{cross_pt:4.1f} pt)")
            sel = [k for k in ks if idx[(k, 32)][col] >= 5000]
            if sel:
                ratios = defaultdict(list)
                for k in sel:
                    for p_, h in zip((13, 32, 72), hs):
                        ratios[p_].append(idx[(k, p_)][col] / h)
                pw = [fit_power(hs, [idx[(k, p_)][col] for p_ in (13, 32, 72)])[1]
                      for k in sel]
                pw = sorted(x for x in pw if not math.isnan(x))
                med = lambda v: sorted(v)[len(v) // 2]
                print(" " * 9 + f"{len(sel)} keys: area/h median "
                      f"{med(ratios[13]):.0f}/{med(ratios[32]):.0f}/{med(ratios[72]):.0f}; "
                      f"power p median {med(pw):.2f}, range {pw[0]:.2f}..{pw[-1]:.2f}")
    # The fire's slope, cell height against cell width: the quad is the cursor
    # (h x w) and the band is h-scaled, so a fit on h alone is a fit on both.
    print()
    print("Fire, worst area / (path length in window) per size, both paths:")
    for p in (13, 32, 72):
        for m in ("every-100ms-diag", "every-100ms-across"):
            g = [r for r in rows if r["arm"] == "fire" and r["pt"] == p and r["scale"] == 1
                 and r["motion"] == m]
            w = max(g, key=lambda r: r["apart"])
            print(f"  {p:>2} pt {m:<20} apart {w['apart']:>8,.0f}  ({w['ink']} on {w['ground']}, "
                  f"{w['rate']} Hz)  = {w['apart'] / CELL_H[p]:.0f} x h")


def light_vs_dark(rows):
    section("3. The light ground: failing counts and worst apart area by ground")
    for p, s in SIZES:
        print(f"\n{p} pt {s}x".ljust(12) + "".join(g.rjust(22) for g in GROUNDS))
        for e in EFFECTS[1:]:
            line = ("  " + e).ljust(12)
            for g in GROUNDS:
                sub = [r for r in rows if r["arm"] == e and r["pt"] == p and r["scale"] == s
                       and r["ground"] == g]
                if not sub:
                    line += "-".rjust(22)
                    continue
                f = sum(r["outcome"] == "FAIL" for r in sub)
                w = max(sub, key=lambda r: r["apart"])
                line += f"{f:>3} {w['apart']:>8,.0f} {w['ink'][:5]:>5}".rjust(22)
            print(line)
    print()
    print("The theme's own cursor on its own page (ink=default), worst apart over")
    print("motions and rates: light page (#272727 on #EFEFEF) against dark page")
    print("(#D8D8D8 on #101010), and the ratio light/dark.")
    for p, s in SIZES:
        line = f"{p} pt {s}x".ljust(10)
        for e in EFFECTS[1:]:
            def worst(g):
                sub = [r for r in rows if r["arm"] == e and r["pt"] == p and r["scale"] == s
                       and r["ground"] == g and r["ink"] == "default"]
                return max(r["apart"] for r in sub) if sub else 0.0
            lt, dk = worst("light"), worst("dark")
            ratio = lt / dk if dk > 0 else float("inf")
            line += f"  {e[:5]} {lt:>6,.0f}/{dk:>6,.0f}={ratio:4.2f}"
        print(line)
    print()
    print("Every 32 pt failure, as drawn, with its general and red concurrent areas:")
    for r in rows:
        if r["pt"] == 32 and r["outcome"] == "FAIL":
            print(f"  {r['arm']:<7} {r['scale']}x {r['rate']:>3} Hz {r['motion']:<19} "
                  f"{r['ink']:<7} on {r['ground']:<7} area {r['area']:>7,.0f} "
                  f"apart {r['apart']:>7,.0f} gen {r['gen_over']:>7,.0f} red {r['red_over']:>6,.0f} "
                  f"piece {r['contiguous']:>7,.0f} ign {r['ignitions']} "
                  f"flashes {r['gen_flashes']}/{r['red_flashes']}")


def rate_effect(rows):
    section("4. Rate: 60 against 120 Hz on the same key (apart), failing effects")
    for p, s in [(32, 1), (32, 2), (72, 1)]:
        for e in ("fire", "orb", "plasma", "liquid", "ufo"):
            pairs = []
            idx = {}
            for r in rows:
                if r["arm"] == e and r["pt"] == p and r["scale"] == s:
                    idx[(r["motion"], r["ground"], r["ink"], r["rate"])] = r
            for (m, g, i, rt), r in idx.items():
                if rt == 60 and (m, g, i, 120) in idx:
                    a60, a120 = r["apart"], idx[(m, g, i, 120)]["apart"]
                    if a60 > 2000:
                        pairs.append(a120 / a60)
            if pairs:
                pairs.sort()
                print(f"  {e:<7} {p} pt {s}x: 120/60 apart ratio median {pairs[len(pairs)//2]:.3f}, "
                      f"range {pairs[0]:.3f}..{pairs[-1]:.3f} over {len(pairs)} keys")


def ignitions(rows):
    section("5. The driver's counts per motion (ignitions, moves), effect rows")
    seen = defaultdict(set)
    for r in rows:
        if r["arm"] in ("off",):
            continue
        seen[(r["motion"],)].add((r["moves"], r["ignitions"]))
    for k, v in sorted(seen.items()):
        print(f"  {k[0]:<20} (moves, ignitions): {sorted(v)}")


def main():
    rows = load(PATH)
    print(f"{len(rows)} case rows from {PATH}")
    ctl = controls(PATH)
    bad = [c for c in ctl if (c["arm"] == "block-4hz") != (c["outcome"] == "FAIL")]
    print(f"positive-control rows {len(ctl)}, any wrong verdict: {len(bad)}")
    table_a(rows)
    totals(rows)
    scaling(rows)
    light_vs_dark(rows)
    rate_effect(rows)
    ignitions(rows)


if __name__ == "__main__":
    main()
