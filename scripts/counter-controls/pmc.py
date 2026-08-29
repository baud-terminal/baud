import re, sys, itertools
xml = open(sys.argv[1]).read()
rows = re.findall(r'<row>(.*?)</row>', xml, re.S)
# resolve id/ref for core and sample-time
core_by_id, time_by_id = {}, {}
samples = []
for r in rows:
    m = re.search(r'<sample-time id="(\d+)"[^>]*>(\d+)</sample-time>', r)
    if m: time_by_id[m.group(1)] = int(m.group(2)); t = int(m.group(2))
    else:
        m = re.search(r'<sample-time ref="(\d+)"/>', r)
        t = time_by_id.get(m.group(1)) if m else None
    m = re.search(r'<core id="(\d+)"[^>]*>(\d+)</core>', r)
    if m: core_by_id[m.group(1)] = int(m.group(2)); c = int(m.group(2))
    else:
        m = re.search(r'<core ref="(\d+)"/>', r)
        c = core_by_id.get(m.group(1)) if m else None
    m = re.search(r'<pmc-events[^>]*>([\d ]+)</pmc-events>', r)
    if not (m and t is not None and c is not None): continue
    samples.append((t, c, [int(x) for x in m.group(1).split()]))
print(f"{len(samples)} counter samples, {len(samples[0][2])} columns")
n = len(samples[0][2])
tot = [0]*n
wall = 0
for (t0, c0, v0), (t1, c1, v1) in zip(samples, samples[1:]):
    if c0 != c1: continue
    d = [b - a for a, b in zip(v0, v1)]
    if any(x < 0 for x in d): continue
    dt = t1 - t0
    if dt <= 0 or dt > 5_000_000: continue
    wall += dt
    for i in range(n): tot[i] += d[i]
print(f"consecutive same-core wall time: {wall/1e9:.4f} s")
for i, v in enumerate(tot):
    rate = v / (wall/1e9) if wall else 0
    print(f"  idx{i:<2} {v:>16,}   {rate/1e9:8.3f} G/s")
# top-down identity: is one column the sum of four others?
print("\nadditive identities (target = sum of 4 others, within 2%):")
cand = [i for i in range(n) if tot[i] > 0]
for tgt in cand:
    others = [i for i in cand if i != tgt]
    for combo in itertools.combinations(others, 4):
        s = sum(tot[i] for i in combo)
        if tot[tgt] and abs(s - tot[tgt]) / tot[tgt] < 0.02:
            print(f"  idx{tgt} ~= " + " + ".join(f"idx{i}" for i in combo) +
                  f"   ({s:,} vs {tot[tgt]:,}, {100*(s-tot[tgt])/tot[tgt]:+.2f}%)")
