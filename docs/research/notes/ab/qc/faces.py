import sys, json
from fontTools.ttLib import TTFont, TTCollection
paths = sys.argv[1:]
out = []
for p in paths:
    n = len(TTCollection(p).fonts) if p.endswith(".ttc") else 1
    for i in range(n):
        f = TTFont(p, fontNumber=i, lazy=True)
        name = f["name"].getDebugName(4)
        cmap = f.getBestCmap() or {}
        heb = sum(1 for c in range(0x05B0, 0x05C8) if c in cmap)
        def scripts(tag):
            if tag not in f: return None
            t = f[tag].table
            if not t.ScriptList: return []
            r = []
            for s in t.ScriptList.ScriptRecord:
                feats = set()
                ls = [s.Script.DefaultLangSys] if s.Script.DefaultLangSys else []
                ls += [l.LangSys for l in s.Script.LangSysRecord]
                for l in ls:
                    for fi in l.FeatureIndex:
                        feats.add(t.FeatureList.FeatureRecord[fi].FeatureTag)
                r.append(s.ScriptTag + ":" + ",".join(sorted(feats)))
            return r
        out.append(dict(path=p, index=i, name=name, hebrew_points_mapped=heb,
            GSUB=scripts("GSUB"), GPOS=scripts("GPOS"), morx="morx" in f, GDEF="GDEF" in f,
            glyf="glyf" in f, CFF="CFF " in f, fvar="fvar" in f))
print(json.dumps(out, indent=1))
