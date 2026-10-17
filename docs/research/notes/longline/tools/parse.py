import sys, re, statistics as st
d = {}; cells = {}
rnd = -1
for line in open(sys.argv[1]):
    if line.startswith("ROUND"): rnd = int(line.split()[1]); continue
    m = re.match(r"ARM (\S+) REC (\S+)", line)
    if m: arm, rec = m.groups(); continue
    m = re.search(r"a gesture: resize ([\d.]+) ms", line)
    if m and rnd > 0: d.setdefault((rec, arm, "g"), []).append(float(m.group(1)))
    m = re.search(r"parser (\d+)-(\d+)", line)
    if m: cells.setdefault((rec, arm), set()).update(m.groups())
    m = re.search(r"costs (\d+) \[", line)
    if m and rnd > 0: d.setdefault((rec, arm, "c"), []).append(int(m.group(1)))
arms = ["base", "aa", "held2", "clear2"]
recs = ["drag","fresh-burst","fresh-drag","fresh-zoom","live-drag","short","zoom"]
for rec in recs:
    print(rec, "cells", {a: sorted(cells[(rec,a)]) for a in arms})
for k, fmt in (("g", "{:.2f} [{:.2f}-{:.2f}]"), ("c", "{:,.0f} [{:,.0f}-{:,.0f}]")):
    print("gesture ms" if k=="g" else "control ns", "n=", len(d[(recs[0],"base",k)]))
    for rec in recs:
        row = []
        for a in arms:
            v = d[(rec,a,k)]
            row.append(fmt.format(st.median(v), min(v), max(v)))
        b = d[(rec,"base",k)]; aa = d[(rec,"aa",k)]
        flags = []
        for a in ("held2","clear2"):
            v = d[(rec,a,k)]
            if min(v) > max(b) and min(v) > max(aa): flags.append(a+" ABOVE")
            elif min(v) > max(b) or min(v) > max(aa): flags.append(a+" above one")
        print("| %s | %s |" % (rec, " | ".join(row)), flags)
