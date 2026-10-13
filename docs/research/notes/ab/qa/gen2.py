# Candidate strings built from the latn lookups baud applies themselves:
# ligature components, single/multiple inputs, mark-to-base and mark-to-mark
# coverage pairs, mapped back through the cmap.
import sys, json
sys.path.insert(0, "/nix/store/v8mbi1jnamp837ncvppww8vj9aq9xczs-python3.14-fonttools-4.63.0/lib/python3.14/site-packages")
from fontTools.ttLib import TTFont
BAUD = {'ccmp', 'calt', 'mark', 'mkmk'}
rows = [json.loads(l) for l in open(sys.argv[1])]
done = set()
for r in rows:
    if 'error' in r: continue
    tabs = []
    for t in ('GSUB', 'GPOS'):
        d = r.get(t)
        if not d or 'error' in d or not d['tags'] or 'DFLT' in d['tags']: continue
        latn = d.get('latn')
        if latn and latn['default'] and latn['default']['baud_lookups']:
            tabs.append((t, latn['default']['baud_lookups']))
    if not tabs or (r['path'], r['index']) in done: continue
    done.add((r['path'], r['index']))
    f = TTFont(r['path'], fontNumber=r['index'], lazy=True)
    rev = {}
    for cp, g in sorted(f.getBestCmap().items()): rev.setdefault(g, []).append(cp)
    def cps(g): return rev.get(g, [])[:3]
    out = set()
    def emit(seq):
        if all(seq): out.add(tuple(seq))
    for t, lks in tabs:
        tbl = f[t].table
        for li in lks:
            lk = tbl.LookupList.Lookup[li]
            for st in lk.SubTable:
                typ = lk.LookupType
                if hasattr(st, 'ExtSubTable'): st, typ = st.ExtSubTable, st.ExtensionLookupType
                if t == 'GSUB' and typ == 4:
                    for first, ligs in st.ligatures.items():
                        for lg in ligs:
                            comps = [first] + list(lg.Component)
                            if all(cps(g) for g in comps):
                                for c0 in cps(first): emit([c0] + [cps(g)[0] for g in comps[1:]])
                elif t == 'GSUB' and typ in (1, 2, 3):
                    m = getattr(st, 'mapping', None) or getattr(st, 'alternates', None) or {}
                    for g in m:
                        for c in cps(g): emit([c]); emit([0x25CC, c])
                elif t == 'GPOS' and typ in (4, 5):
                    bases = st.BaseCoverage.glyphs if typ == 4 else st.LigatureCoverage.glyphs
                    marks = st.MarkCoverage.glyphs
                    for b in bases:
                        for m in marks:
                            if cps(b) and cps(m): emit([cps(b)[0], cps(m)[0]])
                elif t == 'GPOS' and typ == 6:
                    for b in st.Mark2Coverage.glyphs:
                        for m in st.Mark1Coverage.glyphs:
                            if cps(b) and cps(m): emit([0x25CC, cps(b)[0], cps(m)[0]])
                else:
                    print("skip", r['path'], t, li, typ, file=sys.stderr)
    for seq in sorted(out):
        print("%s\t%d\t%s" % (r['path'], r['index'], " ".join("%04X" % c for c in seq)))
