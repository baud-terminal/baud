"""Join the base and O2 census runs and summarise.

usage: analyze.py <runs dir> <out dir>
Writes <out>/census-o2.tsv (every case, both arms side by side) and prints
summaries: FAIL counts and worst areas per arm x size x ground, the pixel
check, the 13/24 pt hash comparison, and the strength tally."""
import collections
import csv
import glob
import os
import sys

runs, out = sys.argv[1], sys.argv[2]
KEY = ('pt', 'scale', 'rate', 'ground', 'ink', 'arm', 'motion')


def load(prefix):
    rows = {}
    for f in sorted(glob.glob(f'{runs}/{prefix}-*.tsv')):
        name = os.path.basename(f)[:-4]
        if name.endswith('-b') or 'gate' in name or 'cells' in name or 'png' in name or 'drop' in name or 'hash' in name:
            continue
        if prefix in ('base', 'o2') and (name.startswith('base2-') or name.startswith('o2b-')):
            continue
        with open(f) as fh:
            for r in csv.DictReader(fh, delimiter='\t'):
                if r['arm'].startswith('block'):
                    continue
                k = tuple(r[c] for c in KEY)
                rows[k] = r
    return rows


base = load('base')
base.update(load('base2'))
base.update(load('base3'))
o2 = load('o2')
o2.update(load('o2b'))
o2.update(load('o2c'))
cols = ['area', 'apart', 'outcome', 'apart_outcome', 'strict', 'apart_strict', 'strict_outcome',
        'apart_strict_outcome', 'gen_flashes', 'red_flashes', 'vfx', 'repeat', 'under_dl', 'under_raw', 'under_over',
        'under_red', 'over_cap', 'off_colour', 'translucent', 'fnv']
keys = sorted(set(base) | set(o2), key=lambda k: (float(k[0]), int(k[1]), int(k[2]), k[3], k[4], k[5], k[6]))
os.makedirs(out, exist_ok=True)
with open(f'{out}/census-o2.tsv', 'w') as fh:
    w = csv.writer(fh, delimiter='\t', lineterminator='\n')
    w.writerow(list(KEY) + [f'base_{c}' for c in cols] + [f'o2_{c}' for c in cols])
    for k in keys:
        b, o = base.get(k, {}), o2.get(k, {})
        w.writerow(list(k) + [b.get(c, '') for c in cols] + [o.get(c, '') for c in cols])
print(f'{len(keys)} cases; base {len(base)}, o2 {len(o2)}')

# Per arm x size x ground: FAIL counts (outcome / apart), worst area / apart,
# worst flashes a second, for each arm.
groups = collections.defaultdict(list)
for k in keys:
    groups[(k[5], f'{k[0]}x{k[1]}', k[2], k[3])].append(k)


def fnum(r, c):
    try:
        return float(r[c])
    except (KeyError, ValueError):
        return float('nan')


print('\narm\tsize\trate\tground\tn\tbase_FAIL\tbase_apartFAIL\tbase_max_area\tbase_max_apart\tbase_max_fps\to2_FAIL\to2_apartFAIL\to2_max_area\to2_max_apart\to2_max_fps\to2_strictFAIL\to2_apart_strictFAIL\to2_repeat_frames\to2_vfx_frames\to2_max_under_dl\to2_under_over\to2_under_red\to2_over_cap')
for g in sorted(groups, key=lambda g: (g[0], float(g[1].split('x')[0]), g[1], g[2], g[3])):
    ks = groups[g]
    def agg(rows):
        rows = [r for r in rows if r]
        if not rows:
            return ['-'] * 5
        return [sum(r['outcome'] == 'FAIL' for r in rows), sum(r['apart_outcome'] == 'FAIL' for r in rows),
                f"{max(fnum(r, 'area') for r in rows):.0f}", f"{max(fnum(r, 'apart') for r in rows):.0f}",
                max(max(int(r['gen_flashes']), int(r['red_flashes'])) for r in rows)]
    bs = [base.get(k, {}) for k in ks]
    os_ = [o2.get(k, {}) for k in ks]
    line = list(g) + [len(ks)] + agg(bs) + agg(os_)
    orows = [r for r in os_ if r]
    if orows:
        line += [sum(r['strict_outcome'] == 'FAIL' for r in orows), sum(r['apart_strict_outcome'] == 'FAIL' for r in orows),
                 sum(int(r['repeat']) for r in orows), sum(int(r['vfx']) for r in orows),
                 f"{max(fnum(r, 'under_dl') for r in orows):.4f}", sum(int(r['under_over']) for r in orows),
                 sum(int(r['under_red']) for r in orows), sum(int(r['over_cap']) for r in orows)]
    print('\t'.join(str(x) for x in line))

# Hashes at 13 and 24 pt.
print('\nhash comparison (base vs o2) by size:')
for size in ('13', '24'):
    ks = [k for k in keys if k[0] == size and k[1] == '1']
    both = [k for k in ks if base.get(k, {}).get('fnv', '-') != '-' and o2.get(k, {}).get('fnv', '-') != '-']
    diff = [k for k in both if base[k]['fnv'] != o2[k]['fnv']]
    print(f'  {size}x1: {len(both)} cases hashed on both, {len(diff)} differ')
    for k in diff[:10]:
        print('   ', k)
# The base's own hash control.
ctl = {}
for f in glob.glob(f'{runs}/base-13x1-b.tsv'):
    with open(f) as fh:
        for r in csv.DictReader(fh, delimiter='\t'):
            ctl[tuple(r[c] for c in KEY)] = r['fnv']
same = [k for k in ctl if k in base and base[k]['fnv'] == ctl[k]]
print(f'  base 13x1 twice: {len(ctl)} cases in the second run, {len(same)} equal to the first')

# Every FAIL under O2.
fails = [k for k in keys if o2.get(k) and (o2[k]['outcome'] == 'FAIL' or o2[k]['apart_outcome'] == 'FAIL')]
print(f'\nO2 FAILs: {len(fails)}')
for k in fails:
    r = o2[k]
    print('  ', '\t'.join(k), r['area'], r['apart'], r['repeat'], r['vfx'])
# The pixel check, worst cases.
pc = sorted((k for k in keys if o2.get(k)), key=lambda k: -fnum(o2[k], 'under_dl'))
print('\nO2 pixel check, worst 25 by under_dl:')
for k in pc[:25]:
    r = o2[k]
    print('  ', '\t'.join(k), r['under_dl'], r['under_over'], r['under_red'], r['over_cap'], r['repeat'], r['vfx'], r['area'], r['outcome'])
