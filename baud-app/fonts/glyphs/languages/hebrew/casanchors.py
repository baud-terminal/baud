"""Cascadia's own dagesh anchors (letter anchor minus dagesh mark anchor), read from an instance's GPOS
the way round 1's cascadia/casanchors.py read cas400; plus the dagesh's ink centre. casanchors.py INST.ttf OUT.json"""
import sys, json
from fontTools.ttLib import TTFont
from fontTools.pens.boundsPen import BoundsPen
f = TTFont(sys.argv[1]); cm = f.getBestCmap(); rev = {v: k for k, v in cm.items()}
gp = f['GPOS'].table
out = {}
for li, l in enumerate(gp.LookupList.Lookup):
    for st in l.SubTable:
        if l.LookupType == 9: st = st.ExtSubTable
        if l.LookupType in (4,) or getattr(st, 'LookupType', 0) == 4:
            if not hasattr(st, 'BaseCoverage'): continue
            mk = [n for n in st.MarkCoverage.glyphs if n.startswith('uni05')]
            if not mk: continue
            ma = st.MarkArray.MarkRecord[st.MarkCoverage.glyphs.index(mk[0])].MarkAnchor
            for bi, bn in enumerate(st.BaseCoverage.glyphs):
                if bn in rev and 0x5d0 <= rev[bn] <= 0x5ea:
                    a = st.BaseArray.BaseRecord[bi].BaseAnchor[0]
                    if a: out.setdefault(','.join(mk), {})[hex(rev[bn])] = (a.XCoordinate - ma.XCoordinate, a.YCoordinate - ma.YCoordinate)
gs = f.getGlyphSet(); bp = BoundsPen(gs); gs['uni05BC'].draw(bp); b = bp.bounds
print('dagesh ink', b, 'centre', ((b[0] + b[2]) / 2, (b[1] + b[3]) / 2))
for k, v in out.items(): print(k, len(v))
json.dump(out, open(sys.argv[2], 'w'))
