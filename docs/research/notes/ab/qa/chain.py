import sys
sys.path.insert(0, "/nix/store/v8mbi1jnamp837ncvppww8vj9aq9xczs-python3.14-fonttools-4.63.0/lib/python3.14/site-packages")
sys.path.insert(0, sys.argv[4]); import survey
from fontTools.ttLib import TTFont
f = TTFont(sys.argv[1], fontNumber=int(sys.argv[2])); rev = {}
for cp, g in f.getBestCmap().items(): rev.setdefault(g, []).append(cp)
t = f['GSUB'].table; lk = t.LookupList.Lookup[int(sys.argv[3])]
for st in lk.SubTable:
    if hasattr(st, 'ExtSubTable'): st = st.ExtSubTable
    print("format", st.Format)
    for a in ('InputCoverage', 'BacktrackCoverage', 'LookAheadCoverage'):
        for c in getattr(st, a, []) or []:
            sc = {}
            for g in c.glyphs:
                for cp in rev.get(g, []): sc[survey.script_of(cp)] = sc.get(survey.script_of(cp), 0) + 1
            print(" ", a, len(c.glyphs), sc)
    for sl in getattr(st, 'SubstLookupRecord', []) or []: print("  subst", sl.SequenceIndex, sl.LookupListIndex)
