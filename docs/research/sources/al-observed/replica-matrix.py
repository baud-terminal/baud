"""Summarise the variant matrix (out56.tsv): per variant x size, how many of
the cases pass on the primary and strict readings, as drawn and apart; the
worst case of each; and the shipped-driver gate against the census.
Run from the repository root."""
import csv, collections, sys
T = 21824
cen = {}
with open('docs/research/sources/aj-observed/flash-census.tsv') as f:
    for r in csv.DictReader(f, delimiter='\t'):
        cen[(r['arm'], str(int(float(r['pt']))), r['scale'], r['rate'], r['ground'], r['ink'], r['motion'])] = r
cols = ['area','apart','contiguous','strict','apart_strict','gen_over','red_over','gen_flashes','red_flashes','second','vfx','ignitions']
rows = []
for fn in sys.argv[1:]:
    for line in open(fn):
        f = line.rstrip('\n').split('\t')
        d = {k: v for k, v in (kv.split('=') for kv in f[8:])}
        eff, var = (f[0].split('/') + ['shipped'])[:2]
        rows.append(dict(eff=eff, var=var, pt=f[1], scale=f[2], rate=f[3], ground=f[4], ink=f[5], motion=f[6], arm=f[7], **d))
# The gate.
n = ok = 0; near = 0; bad = []
for r in rows:
    if r['var'] != 'shipped' or r['arm'] != 'all': continue
    k = (r['eff'], r['pt'], r['scale'], r['rate'], r['ground'], r['ink'], r['motion'])
    if k not in cen: continue
    n += 1
    c = cen[k]
    diffs = [(x, r[x], c[x]) for x in cols if abs(float(r[x]) - float(c[x])) > 1e-6]
    if not diffs: ok += 1
    else: bad.append((k, diffs))
    if (float(r['area']) > T) != (c['outcome'] == 'FAIL'): near += 1
print(f"gate: {n} shipped rows compared with the census, {ok} equal in every verdict column, {near} with a different verdict")
for k, d in bad[:20]: print('  differs', k, d)
# The matrix.
g = collections.defaultdict(list)
for r in rows:
    if r['var'] == 'shipped': continue
    g[(r['var'], r['pt'])].append(r)
print("\nvariant size  cases | fail drawn  fail apart | strict drawn  strict apart | worst drawn (case)")
for (v, pt), rs in sorted(g.items()):
    fd = sum(float(r['area']) > T for r in rs); fa = sum(float(r['apart']) > T for r in rs)
    sd = sum(float(r['strict']) > T for r in rs); sa = sum(float(r['apart_strict']) > T for r in rs)
    w = max(rs, key=lambda r: float(r['area']))
    print(f"{v:>7} {pt:>3} pt {len(rs):>5} | {fd:>10} {fa:>11} | {sd:>12} {sa:>13} | {float(w['area']):,.0f} ({w['eff']} {w['motion']} {w['ink']}/{w['ground']} {w['rate']} Hz; apart {float(w['apart']):,.0f})")
    by = collections.Counter(r['eff'] for r in rs if float(r['area']) > T)
    bya = collections.Counter(r['eff'] for r in rs if float(r['apart']) > T)
    bys = collections.Counter(r['eff'] for r in rs if float(r['strict']) > T)
    if by or bys:
        print(f"{'':>17} failing drawn by effect {dict(by)}; apart {dict(bya)}; strict drawn {dict(bys)}")
# The shipped driver on the same keys, for comparison (from the census).
print("\nthe census on the same keys (shipped driver, retarget motions only):")
keys = {(r['eff'], r['pt'], r['scale'], r['rate'], r['ground'], r['ink'], r['motion']) for r in rows if r['var'] != 'shipped'}
cnt = collections.defaultdict(lambda: [0, 0, 0, 0])
for k in keys:
    if k in cen:
        c = cen[k]; e = cnt[k[1]]
        e[0] += 1; e[1] += c['outcome'] == 'FAIL'; e[2] += c['apart_outcome'] == 'FAIL'; e[3] += c['strict_outcome'] == 'FAIL'
for pt, e in sorted(cnt.items()):
    print(f"  {pt} pt: {e[0]} keys in the census; failing drawn {e[1]}, apart {e[2]}, strict drawn {e[3]}")
