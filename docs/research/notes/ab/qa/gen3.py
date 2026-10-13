# Greek base + mark1 + mark2 over the mkmk coverage of latn's lookups, for faces with no grek/DFLT.
import sys
sys.path.insert(0, "/nix/store/v8mbi1jnamp837ncvppww8vj9aq9xczs-python3.14-fonttools-4.63.0/lib/python3.14/site-packages")
from fontTools.ttLib import TTFont
D = "/System/Library/Fonts/Supplemental/"
for n in ["Times New Roman.ttf", "Times New Roman Bold.ttf", "Tahoma Bold.ttf", "Microsoft Sans Serif.ttf"]:
    f = TTFont(D + n, lazy=True); rev = {}
    for cp, g in sorted(f.getBestCmap().items()): rev.setdefault(g, cp)
    t = f['GPOS'].table
    latn = [r for r in t.ScriptList.ScriptRecord if r.ScriptTag == 'latn'][0].Script.DefaultLangSys
    m1s, m2s = set(), set()
    for fi in latn.FeatureIndex:
        fr = t.FeatureList.FeatureRecord[fi]
        if fr.FeatureTag != 'mkmk': continue
        for li in fr.Feature.LookupListIndex:
            for st in t.LookupList.Lookup[li].SubTable:
                m2s |= {rev[g] for g in st.Mark2Coverage.glyphs if g in rev}
                m1s |= {rev[g] for g in st.Mark1Coverage.glyphs if g in rev}
    for base in (0x03B1, 0x03B5, 0x03C9, 0x0391):
        for a in sorted(m2s):
            for b in sorted(m1s):
                print("%s\t0\t%04X %04X %04X" % (D + n, base, a, b))
