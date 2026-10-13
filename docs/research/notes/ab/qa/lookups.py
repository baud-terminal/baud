import sys
sys.path.insert(0, "/nix/store/v8mbi1jnamp837ncvppww8vj9aq9xczs-python3.14-fonttools-4.63.0/lib/python3.14/site-packages")
from fontTools.ttLib import TTFont
path, idx, tname = sys.argv[1], int(sys.argv[2]), sys.argv[3]
f = TTFont(path, fontNumber=idx)
rev = {}
for cp, g in f.getBestCmap().items(): rev.setdefault(g, cp)
t = f[tname].table
for sr in t.ScriptList.ScriptRecord:
    ls = sr.Script.DefaultLangSys
    print(sr.ScriptTag, [ (t.FeatureList.FeatureRecord[i].FeatureTag, t.FeatureList.FeatureRecord[i].Feature.LookupListIndex) for i in (ls.FeatureIndex if ls else [])], [r.LangSysTag for r in sr.Script.LangSysRecord])
want = set(int(x) for x in sys.argv[4].split(',')) if len(sys.argv) > 4 else set()
for li in sorted(want):
    lk = t.LookupList.Lookup[li]
    print("lookup", li, "type", lk.LookupType, "flag", lk.LookupFlag)
    for st in lk.SubTable:
        st2 = st.ExtSubTable if hasattr(st, 'ExtSubTable') else st
        cov = getattr(st2, 'Coverage', None) or getattr(st2, 'MarkCoverage', None)
        d = {}
        for attr in ('mapping', 'ligatures', 'alternates'):
            if hasattr(st2, attr): d[attr] = getattr(st2, attr)
        if d:
            for k, v in d.items():
                items = list(v.items())[:20]
                print("  ", k, [(g, hex(rev.get(g, 0)), val) for g, val in items])
        else:
            print("  format", getattr(st2, 'Format', None), type(st2).__name__, "cov", (cov.glyphs[:15] if cov else None))
            for a in ('BaseCoverage', 'Mark1Coverage', 'Mark2Coverage', 'InputCoverage', 'BacktrackCoverage', 'LookAheadCoverage'):
                c = getattr(st2, a, None)
                if c is not None:
                    gl = c.glyphs if hasattr(c, 'glyphs') else [x.glyphs for x in c]
                    print("    ", a, (gl[:25] if isinstance(gl, list) else gl))
