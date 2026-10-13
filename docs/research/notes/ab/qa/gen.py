# Candidate strings per A1!=A2 face: singles, base+mark, and same-script
# pairs over the code points whose script has no table in the face (and Common).
import sys, json, random
sys.path.insert(0, "/nix/store/v8mbi1jnamp837ncvppww8vj9aq9xczs-python3.14-fonttools-4.63.0/lib/python3.14/site-packages")
sys.path.insert(0, sys.argv[1])
import survey  # noqa: reuse script_of
from fontTools.ttLib import TTFont
random.seed(1)
rows = [json.loads(l) for l in open(sys.argv[2])]
seen = set()
for r in rows:
    if 'error' in r: continue
    cand = False; uncovered = {'Zyyy'}
    for t in ('GSUB', 'GPOS'):
        d = r.get(t)
        if not d or 'error' in d or not d['tags'] or 'DFLT' in d['tags']: continue
        latn = d.get('latn')
        if latn and latn['default'] and latn['default']['baud_lookups']:
            cand = True
            uncovered |= {s for s, _ in d['cmap_scripts_without_table']}
    if not cand or (r['path'], r['index']) in seen: continue
    seen.add((r['path'], r['index']))
    f = TTFont(r['path'], fontNumber=r['index'], lazy=True)
    cmap = f.getBestCmap()
    by = {}
    for cp in cmap:
        if cp < 0x20 or 0x7f <= cp < 0xa0: continue
        by.setdefault(survey.script_of(cp), []).append(cp)
    marks = by.get('Zinh', [])
    lines = []
    for sc in sorted(uncovered):
        cps = by.get(sc, [])
        for c in cps: lines.append([c])
        for c in cps:
            for m in marks: lines.append([c, m])
        if sc != 'Zyyy':
            for a in cps:
                for b in cps: lines.append([a, b])
    for m in marks: lines.append([0x25CC, m]) if 0x25CC in cmap else None
    for l in lines:
        print("%s\t%d\t%s" % (r['path'], r['index'], " ".join("%04X" % c for c in l)))
