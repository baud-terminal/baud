import json, sys
rows = [json.loads(l) for l in open(sys.argv[1])]
ok = [r for r in rows if 'error' not in r]
def lay(r): return [r.get(t) for t in ('GSUB','GPOS') if r.get(t) and 'error' not in r.get(t) and r.get(t)['tags']]
n_lay = [r for r in ok if lay(r)]
n_nodflt = [r for r in ok if any('DFLT' not in d['tags'] for d in lay(r))]
n_nodflt_latn = [r for r in ok if any('DFLT' not in d['tags'] and 'latn' in d['tags'] for d in lay(r))]
n_nodflt_nolatn = [r for r in ok if any('DFLT' not in d['tags'] and 'latn' not in d['tags'] for d in lay(r))]
print("faces", len(rows), "parsed", len(ok), "with a GSUB/GPOS ScriptList", len(n_lay))
print("faces where some table lacks DFLT", len(n_nodflt), "; of those with latn", len(n_nodflt_latn), "; lacking both DFLT and latn", len(n_nodflt_nolatn))
print("script 'dflt' in any table:", sum(1 for r in ok for d in lay(r) if 'dflt' in d['tags']))
print("\nrepo faces:")
for r in ok:
    if '/Code/baud/' not in r['path']: continue
    p = r['path'].split('/Code/baud/')[1]
    s = []
    for t in ('GSUB','GPOS'):
        d = r.get(t)
        if not d or 'error' in d: s.append(t+":none"); continue
        latn = d.get('latn'); lk = latn and latn['default'] and latn['default']['baud_lookups']
        s.append("%s:%s DFLT=%s latn_baud_lookups=%d nocov=%s" % (t, ",".join(d['tags']), 'DFLT' in d['tags'], len(lk or []), ",".join(x for x,_ in d['cmap_scripts_without_table'])))
    print(" ", p, r['index'], " | ".join(s))
print("\nno-DFLT, no-latn table instances (A1=A2=A3: chain ends with nothing):")
for r in n_nodflt_nolatn:
    for t in ('GSUB','GPOS'):
        d = r.get(t)
        if d and 'error' not in d and d['tags'] and 'DFLT' not in d['tags'] and 'latn' not in d['tags']:
            print("  ", r['path'].replace('/System/Library/Fonts/',''), r['index'], t, d['tags'])
