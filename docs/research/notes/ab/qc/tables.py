import json, sys, collections
Q = sys.argv[1]
R = json.load(open(f"{Q}/measure.json"))
corp = {n: json.load(open(f"{Q}/corpus-{n}-occ.json")) for n in ("gen5", "pango")}
def points_only(k): return all(0x05B0 <= int(x,16) <= 0x05C7 for x in k.split()[1:])
# sanity: on clusters whose order does not differ, C1 and C2 raw output must be byte-identical
bad = [(f, k) for f, per in R.items() for k, r in per.items() if not r["differs"] and r["c1"]["raw"] != r["c2"]["raw"]]
print("sanity: clusters with same order but different C1/C2 output:", len(bad))
print()
print("| face | corpus, subset | n distinct / occ | C1 = hb | C2 = hb | C1 ids = hb | C2 ids = hb | marks outside cell C1 | C2 | hb | C1 != C2 |")
print("|---|---|---|---|---|---|---|---|---|---|---|")
for face, per in R.items():
    for corpus, sub in (("gen5", "order differs"), ("pango", "order differs, points only"), ("pango", "order differs, with cantillation"), ("gen5", "all"), ("pango", "all")):
        occ = corp[corpus]
        def keep(k):
            r = per[k]
            if sub == "all": return True
            if not r["differs"]: return False
            if sub.endswith("points only"): return points_only(k)
            if sub.endswith("cantillation"): return not points_only(k)
            return True
        ks = [k for k in occ if keep(k)]
        t = collections.Counter()
        for k in ks:
            r = per[k]; w = occ[k]
            t["n"] += 1; t["w"] += w
            for a in ("c1", "c2"):
                if r[a]["ink_equal"]: t[a] += 1; t[a+"w"] += w
                if r[a]["ids_equal"]: t[a+"i"] += 1; t[a+"iw"] += w
                if r[a]["outside"]: t[a+"o"] += 1; t[a+"ow"] += w
            if r["hb_out"]: t["ho"] += 1; t["how"] += w
            if not r["c1_eq_c2"]: t["ne"] += 1; t["new"] += w
        f = lambda a: f"{t[a]}/{t[a+'w']}"
        print(f"| {face} | {corpus}, {sub} | {t['n']}/{t['w']} | {f('c1')} | {f('c2')} | {t['c1i']}/{t['c1iw']} | {t['c2i']}/{t['c2iw']} | {t['c1o']}/{t['c1ow']} | {t['c2o']}/{t['c2ow']} | {t['ho']}/{t['how']} | {t['ne']}/{t['new']} |")
