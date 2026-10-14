"""Per face, the clusters of both corpora that match hb-shape under C1 (the
commit before) and C2 (this commit), from measure-all.json.gz: measure_all.py
writes measure-all.json, and the .gz is `gzip -9` of it.
Usage: python3 survey.py measure-all.json.gz"""
import gzip, json, sys
r = json.load(gzip.open(sys.argv[1]))
tot = dict(faces=0, reg_ink=0, reg_exact=0, new_ink=0, new_exact=0, ink_c1_ne_c2=0, faces_moved=0)
for face, per in r.items():
    c = dict(reg_ink=0, reg_exact=0, new_ink=0, new_exact=0, ne=0, c1=0, c2=0)
    for k, row in per.items():
        a, b = row["c1"], row["c2"]
        c["c1"] += a["ink_equal"]; c["c2"] += b["ink_equal"]
        if a["ink_equal"] and not b["ink_equal"]: c["reg_ink"] += 1
        if a["exact"] and not b["exact"]: c["reg_exact"] += 1
        if b["ink_equal"] and not a["ink_equal"]: c["new_ink"] += 1
        if b["exact"] and not a["exact"]: c["new_exact"] += 1
        if not row["c1_eq_c2"]: c["ne"] += 1
    tot["faces"] += 1
    for x in ("reg_ink", "reg_exact", "new_ink", "new_exact"): tot[x] += c[x]
    tot["ink_c1_ne_c2"] += c["ne"]; tot["faces_moved"] += c["ne"] > 0
    print(f"{face:58} = hb C1 {c['c1']:3} -> C2 {c['c2']:3} / {len(per)}; regress ink {c['reg_ink']} exact {c['reg_exact']}; new ink {c['new_ink']} exact {c['new_exact']}; ink C1!=C2 {c['ne']}")
print(tot)
