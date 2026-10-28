# Run from the repository root.
import csv,sys
cen={}
with open('docs/research/sources/aj-observed/flash-census.tsv') as f:
    for r in csv.DictReader(f,delimiter='\t'):
        cen[(r['arm'],str(int(float(r['pt']))),r['scale'],r['rate'],r['ground'],r['ink'],r['motion'])]=r
cols=['area','apart','contiguous','strict','apart_strict','gen_ever','lit','gen_flashes','red_flashes','vfx','ignitions','gen_over','red_over','second']
n=bad=0
for fn in sys.argv[1:]:
    for line in open(fn):
        f=line.rstrip('\n').split('\t')
        eff=f[0]
        if f[7]!='all' or eff not in ('fire','plasma','lightning','ice','ufo','orb','liquid') and not eff.endswith('/v0'):
            continue
        eff=eff.split('/')[0]
        d={k:v for k,v in (kv.split('=') for kv in f[8:])}
        key=(eff,f[1],f[2],f[3],f[4],f[5],f[6])
        if key not in cen: continue
        r=cen[key]; n+=1
        diffs=[(c,d[c],r[c]) for c in cols if abs(float(d[c])-float(r[c]))>1e-6]
        print(('MATCH ' if not diffs else 'DIFF  ')+' '.join(key)+('' if not diffs else '  '+str(diffs)))
        bad+=bool(diffs)
print(f'{n} rows compared, {bad} differ')
